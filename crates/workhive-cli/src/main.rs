//! # workhive CLI
//!
//! Entry point for the workhive terminal workspace manager.
//!
//! Architecture:
//! - `CommandContext` holds all shared dependencies (config, deps, i18n)
//! - `CommandPolicy` declares preflight requirements per command
//! - `ConfirmPolicy` enum drives confirmation behavior
//! - `dispatch()` runs preflight then delegates to handlers
//! - Each handler receives only `&CommandContext` — no loose parameters

mod cli;
mod commands;
mod composition;
mod confirm;
mod context;
mod error;
mod error_render;
mod localize;
mod logging;
mod path_display;
mod policy;
mod table;
mod ui;
mod wizard;

use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches};

use cli::{
    Cli, Commands, CompletionAction, ConfigAction, InternalAction, InternalActivityAction,
    InternalCompletionAction, PaneAction, ProjectAction, ProjectRootsAction, RuntimeAction,
    SessionAction, WorkspaceAction, WorktreeAction,
};
use commands::*;
use context::CommandContext;
use error::CliError;
use policy::{CommandPolicy, ConfirmPolicy};
use workhive_core::domain::Locale;
use workhive_infra::i18n::StaticI18n;

fn main() -> ExitCode {
    restore_default_sigpipe();

    // Resolve locale before clap renders any output so help/usage/parse
    // errors land in the user's configured language. Failures fall back to
    // English silently; locale resolution must never block startup.
    let locale = localize::peek_locale();
    let i18n = StaticI18n::new();

    let cli = match parse_args(locale, &i18n) {
        Ok(cli) => cli,
        Err(code) => return code,
    };

    if !is_quiet_completion_command(&cli.command) {
        logging::init(cli.verbose);

        let resolved_global_config = workhive_infra::config::resolve_global_config_dir()
            .map(|p| p.to_string())
            .unwrap_or_else(|_| "<unresolvable>".to_owned());

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            command = ?cli.command,
            locale = ?locale,
            home = %std::env::var("HOME").unwrap_or_else(|_| "<unset>".to_owned()),
            xdg_config_home = %std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| "<unset>".to_owned()),
            resolved_global_config_path = %resolved_global_config,
            resolved_log_dir = %format!("{}/logs", resolved_global_config),
            "workhive started"
        );
    }

    let ctx = CommandContext::with_verbose(cli.verbose);
    let original_command = format!(
        "workhive {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );
    match dispatch(cli.command, &original_command, &ctx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::ProjectJustInitialized(hint)) => {
            eprintln!("{hint}");
            ExitCode::SUCCESS
        }
        Err(CliError::SessionLookupFailed {
            session,
            availability,
        }) => {
            let rendered = render_session_lookup_error(&ctx, &session, &availability);
            tracing::debug!(session, %availability, "session lookup failed");
            eprintln!("{rendered}");
            ExitCode::from(1)
        }
        Err(err) => {
            tracing::debug!(error = %err, "command failed");
            let code = err.exit_code();
            // Typed errors at the CLI boundary go through the
            // localized renderer so users never see the raw English
            // `Display` of `RuntimeError` / wrapped tmux details.
            // Variants without a typed renderer continue to fall
            // back to `miette` so chained-source diagnostics still
            // appear for unstructured errors.
            if let Some(rendered) = error_render::render_localized_error(&err, locale, &i18n) {
                eprintln!("{rendered}");
            } else {
                eprintln!("{:?}", miette::Report::new(err));
            }
            code
        }
    }
}

/// Restore the default `SIGPIPE` disposition so truncated stdout (e.g.
/// `workhive config show | head`) terminates the process the way standard
/// Unix CLIs do, instead of panicking on a `BrokenPipe` write error.
///
/// The Rust runtime ignores `SIGPIPE` before `main` runs; resetting it to
/// `SIG_DFL` here must happen before the first byte of output.
#[cfg(unix)]
fn restore_default_sigpipe() {
    // SAFETY: `signal` with `SIG_DFL` only swaps the process-wide signal
    // disposition; no Rust-visible state is touched and no handler code
    // runs. Called once, single-threaded, before any I/O.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

/// Completion generation and dynamic completion candidate queries are
/// stdout-only interfaces consumed by shells. They must not emit logging
/// diagnostics to stderr, even when the user's file logging config is broken.
///
/// Snapshot internals intentionally do not use this bypass: monitor/notify
/// commands are background diagnostics paths and should keep normal logging.
fn is_quiet_completion_command(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Completion { .. }
            | Commands::Internal {
                action: InternalAction::Completion { .. }
            }
    )
}

/// Parse argv into [`Cli`] using a locale-aware command tree.
///
/// Returns `Err(exit_code)` if clap rejected input (unknown command,
/// invalid value, etc.) or printed help/version (in which case the exit
/// code is 0 / clap-defined).
///
/// For real parse errors under `zh-CN`, clap's hardcoded English error
/// block is replaced with a Workhive-rendered localized block via
/// [`localize::format_parse_error`]. Help/version display events fall
/// through to clap's printer because the underlying command tree is
/// already localized at this point.
fn parse_args(locale: Locale, i18n: &StaticI18n) -> Result<Cli, ExitCode> {
    let mut cmd = localize::localize_command(Cli::command(), locale, i18n);
    match cmd.try_get_matches_from_mut(std::env::args_os()) {
        Ok(matches) => Cli::from_arg_matches(&matches).map_err(|e| {
            handle_parse_error(&e, locale, i18n);
            ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(2))
        }),
        Err(e) => {
            let code = u8::try_from(e.exit_code()).unwrap_or(2);
            handle_parse_error(&e, locale, i18n);
            Err(ExitCode::from(code))
        }
    }
}

/// Print a clap parse error in the configured locale.
///
/// `--help`/`--version` events are printed via clap so the localized help
/// template (already wired into the command tree) is rendered exactly as
/// clap would render it. Real errors are printed to stderr through the
/// Workhive-localized formatter so zh-CN users never see an English
/// `error:` / `Usage:` block.
fn handle_parse_error(err: &clap::Error, locale: Locale, i18n: &StaticI18n) {
    if localize::is_display_event(err.kind()) {
        let _ = err.print();
        return;
    }
    let block = localize::format_parse_error(err, locale, i18n);
    eprintln!("{block}");
}

fn dispatch(
    command: Commands,
    original_command: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let pol = build_policy(&command);
    policy::run_preflight(&pol, &command, original_command, ctx)?;

    match command {
        Commands::Init {
            global,
            backend,
            force,
        } => cmd_init(global, force, backend.as_deref(), ctx),
        Commands::Config { action } => cmd_config(action, original_command, ctx),
        Commands::Doctor { fix } => cmd_doctor(fix, ctx),
        Commands::Completion { action } => match action {
            CompletionAction::Install { shell } => cmd_completion_install(shell, ctx),
            CompletionAction::Script { shell } => cmd_completions(shell, ctx),
        },
        Commands::Update {
            force,
            dry_run,
            prerelease,
            target_version,
        } => cmd_update(force, dry_run, prerelease, target_version, ctx),
        Commands::Project { action } => match action {
            ProjectAction::List { view } => cmd_project_list(view, ctx),
            ProjectAction::Reindex {
                root,
                config_roots,
                clear_first,
                dry_run,
                force,
            } => cmd_reindex(root, config_roots, clear_first, dry_run, force, ctx),
            ProjectAction::Roots { action } => match action {
                ProjectRootsAction::List => cmd_project_roots_list(ctx),
                ProjectRootsAction::Add { path, force } => cmd_project_roots_add(&path, force, ctx),
                ProjectRootsAction::Remove { path, force } => {
                    let _ = &pol;
                    cmd_project_roots_remove(&path, force, ctx)
                }
                ProjectRootsAction::Edit => {
                    let _ = &pol;
                    cmd_project_roots_edit(ctx).map(|_| ())
                }
                ProjectRootsAction::Status => cmd_project_roots_status(ctx),
            },
        },
        Commands::Workspace { action } => match action {
            WorkspaceAction::Open {
                workspace,
                no_attach,
                force,
                mode,
                init,
                attach_existing_branch,
            } => cmd_open(
                workspace.as_deref(),
                no_attach,
                force,
                &mode,
                init,
                attach_existing_branch,
                &pol,
                ctx,
            ),
            WorkspaceAction::List { view } => cmd_workspace_list(view, ctx),
            WorkspaceAction::Attach { workspace } => {
                cmd_attach(workspace.as_deref(), original_command, &pol, ctx)
            }
            WorkspaceAction::Close { kill, force } => {
                cmd_close(kill, force, original_command, &pol, ctx)
            }
            WorkspaceAction::Remove {
                workspace,
                delete_branch,
                keep_branch,
                ..
            } => cmd_workspace_remove(
                &workspace,
                delete_branch,
                keep_branch,
                original_command,
                &pol,
                ctx,
            ),
            WorkspaceAction::Destroy {
                delete_branch,
                keep_branch,
                ..
            } => cmd_destroy(delete_branch, keep_branch, &pol, ctx),
            WorkspaceAction::Worktree { action } => cmd_worktree_dispatch(action, ctx),
        },
        Commands::Session { action } => match action {
            SessionAction::List { view } => cmd_sessions(view, ctx),
            SessionAction::Attach { target } => cmd_session_attach(target.as_deref(), ctx),
            SessionAction::Resume { session } => cmd_session_resume(session.as_deref(), ctx),
        },
        Commands::Runtime { action } => match action {
            RuntimeAction::Stop { force } => {
                #[cfg(unix)]
                {
                    workhive_infra::dependency::stop_managed_workhive_herdr_server(force)
                        .map_err(CliError::RuntimeOp)
                }
                #[cfg(not(unix))]
                {
                    let _ = force;
                    Err(CliError::Runtime(
                        "Herdr runtime backend requires Unix sockets".to_owned(),
                    ))
                }
            }
        },
        Commands::Pane { action } => cmd_pane(action, original_command, &pol, ctx),
        Commands::Internal { action } => match action {
            InternalAction::Activity { action } => match action {
                InternalActivityAction::Record => cmd_internal_activity_record(ctx),
                InternalActivityAction::Hook { provider } => {
                    cmd_internal_activity_hook(provider, ctx)
                }
                InternalActivityAction::CodexNotify { args } => {
                    cmd_internal_activity_codex_notify(args, ctx)
                }
            },
            InternalAction::Completion { action } => match action {
                InternalCompletionAction::Candidates { kind } => {
                    cmd_internal_completion_candidates(kind)
                }
            },
            InternalAction::PaneTitleGuard {
                backend,
                scope_id,
                workspace_id,
                pane,
                interval_ms,
                duration_ms,
            } => {
                let backend_id = workhive_core::domain::RuntimeBackendId::new(backend)
                    .ok_or_else(|| CliError::Runtime("invalid backend identifier".to_owned()))?;
                let binding =
                    workhive_core::domain::RuntimeBinding::new(backend_id, scope_id, workspace_id)
                        .ok_or_else(|| CliError::Runtime("invalid runtime binding".to_owned()))?;
                cmd_internal_pane_title_guard(&binding, pane, interval_ms, duration_ms, ctx)
            }
            InternalAction::ControlPlane { action } => match action {
                cli::InternalControlPlaneAction::Serve {
                    project_root,
                    server,
                    workspace,
                    parent_pid,
                } => cmd_internal_control_plane_serve(
                    &project_root,
                    &server,
                    &workspace,
                    parent_pid,
                    ctx,
                ),
            },
            InternalAction::Reconcile { config_dir } => cmd_internal_reconcile(&config_dir, ctx),
            InternalAction::PaneSupervisor {
                project_root,
                runtime_root,
                group,
                workspace,
                unit,
                tool,
                program_path,
                env_vars,
                argv,
            } => match cmd_internal_pane_supervisor(
                project_root,
                runtime_root,
                group,
                workspace,
                unit,
                tool,
                program_path,
                env_vars,
                argv,
                ctx,
            ) {
                Ok(code) => {
                    if code != 0 {
                        // Surface the wrapped tool's exit code so the
                        // caller (and observers reading metadata) see
                        // it. The shell itself will not reattribute a
                        // non-zero status to "command not found"
                        // because we are an executed Rust binary, not
                        // a missing executable.
                        std::process::exit(code);
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            },
        },
    }
}

/// Shared policy for managed-worktree subcommands.
///
/// `workhive workspace worktree ...` dispatches through a single handler
/// (`cmd_worktree_dispatch`). Centralizing the policy here keeps the
/// preflight requirements for every worktree action consistent.
fn worktree_policy(action: Option<&WorktreeAction>) -> CommandPolicy {
    use workhive_core::domain::{CommandKind, ConfirmationStage, ProjectInitBehavior};

    let is_remove = matches!(action, Some(WorktreeAction::Remove { .. }));
    CommandPolicy {
        command_kind: if is_remove {
            CommandKind::Mutating
        } else {
            CommandKind::ReadOnly
        },
        project_init_behavior: ProjectInitBehavior::Forbidden,
        confirmation_stage: if is_remove {
            ConfirmationStage::BeforeMutation
        } else {
            ConfirmationStage::None
        },
        // Managed worktree commands resolve the project themselves and do
        // not need global configuration.
        needs_global_config: false,
        // Listing is Git-only; removal also cleans the backing runtime.
        check_deps: is_remove,
        confirm: if is_remove {
            ConfirmPolicy::HandlerManaged
        } else {
            ConfirmPolicy::None
        },
        force: !is_remove,
    }
}

/// Build the preflight policy for each command.
///
/// `init --global` is the bootstrap command — it checks deps itself and
/// manages its own confirmation flow via the wizard.
fn build_policy(command: &Commands) -> CommandPolicy {
    use workhive_core::domain::{CommandKind, ConfirmationStage, ProjectInitBehavior};

    match command {
        Commands::Init {
            global: true,
            force,
            ..
        } => CommandPolicy {
            command_kind: CommandKind::Mutating,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::BeforeMutation,
            needs_global_config: false,
            check_deps: false, // wizard handles deps
            confirm: ConfirmPolicy::HandlerManaged,
            force: *force,
        },
        Commands::Init {
            global: false,
            force,
            ..
        } => CommandPolicy {
            command_kind: CommandKind::Mutating,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::BeforeMutation,
            needs_global_config: false, // handler ensures global first
            check_deps: false,          // wizard handles deps
            confirm: ConfirmPolicy::HandlerManaged,
            force: *force,
        },
        Commands::Config { action } => {
            let force = match action {
                ConfigAction::Show { .. }
                | ConfigAction::Edit { .. }
                | ConfigAction::Explain { .. } => true,
                ConfigAction::Set { force, .. }
                | ConfigAction::Unset { force, .. }
                | ConfigAction::EnvSet { force, .. }
                | ConfigAction::EnvUnset { force, .. } => *force,
            };
            let (kind, confirm, stage) = match action {
                ConfigAction::Show { .. } => (
                    CommandKind::ReadOnly,
                    ConfirmPolicy::None,
                    ConfirmationStage::None,
                ),
                ConfigAction::Explain { .. } => (
                    CommandKind::ReadOnly,
                    ConfirmPolicy::None,
                    ConfirmationStage::None,
                ),
                ConfigAction::Edit { .. } => (
                    CommandKind::Mutating,
                    ConfirmPolicy::None,
                    ConfirmationStage::BeforeMutation,
                ),
                _ => (
                    CommandKind::Mutating,
                    ConfirmPolicy::HandlerManaged,
                    ConfirmationStage::BeforeMutation,
                ),
            };
            CommandPolicy {
                command_kind: kind,
                project_init_behavior: ProjectInitBehavior::Forbidden,
                confirmation_stage: stage,
                needs_global_config: true,
                check_deps: false,
                confirm,
                force,
            }
        }
        Commands::Workspace { action } => match action {
            WorkspaceAction::Open { .. } => CommandPolicy {
                command_kind: CommandKind::Mutating,
                project_init_behavior: ProjectInitBehavior::AllowAutoInitWithExplicitConsent,
                confirmation_stage: ConfirmationStage::None,
                needs_global_config: true,
                check_deps: true,
                confirm: ConfirmPolicy::None,
                force: true,
            },
            WorkspaceAction::Attach { .. } => CommandPolicy {
                command_kind: CommandKind::ReadOnly,
                project_init_behavior: ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized,
                confirmation_stage: ConfirmationStage::None,
                needs_global_config: true,
                // This command may fall back to the global attach flow when
                // the current project is uninitialized, so cwd backend
                // preflight must not block sessions from another backend.
                check_deps: false,
                confirm: ConfirmPolicy::None,
                force: true,
            },
            WorkspaceAction::Close { force, .. } => CommandPolicy {
                command_kind: CommandKind::Mutating,
                project_init_behavior: ProjectInitBehavior::RequireInitialized,
                confirmation_stage: ConfirmationStage::BeforeMutation,
                needs_global_config: true,
                // Close resolves the active runtime from the current session;
                // cwd backend preflight can block a valid tmux close when the
                // cwd's configured Herdr server is unavailable.
                check_deps: false,
                confirm: ConfirmPolicy::HandlerManaged,
                force: *force,
            },
            WorkspaceAction::List { .. } => CommandPolicy {
                command_kind: CommandKind::ReadOnly,
                project_init_behavior: ProjectInitBehavior::Forbidden,
                confirmation_stage: ConfirmationStage::None,
                needs_global_config: true,
                check_deps: true,
                confirm: ConfirmPolicy::None,
                force: true,
            },
            WorkspaceAction::Remove { force, .. } => CommandPolicy {
                command_kind: CommandKind::Mutating,
                project_init_behavior: ProjectInitBehavior::RequireInitialized,
                confirmation_stage: ConfirmationStage::BeforeMutation,
                needs_global_config: true,
                check_deps: true,
                confirm: ConfirmPolicy::HandlerManaged,
                force: *force,
            },
            WorkspaceAction::Destroy { force, .. } => CommandPolicy {
                command_kind: CommandKind::Mutating,
                // `workspace destroy` must NEVER trigger project init or
                // create `.workhive/config.toml`. We disable the default
                // "require initialized → maybe prompt to init" preflight
                // here and let `cmd_destroy` inspect project state itself.
                project_init_behavior: ProjectInitBehavior::NotApplicable,
                confirmation_stage: ConfirmationStage::BeforeMutation,
                needs_global_config: true,
                check_deps: true,
                // The destructive warning + irreversible confirm are owned
                // by `cmd_destroy` so they fire AFTER project-state checks
                // (and never before them, which would create the bug where
                // the user agrees to "destroy everything" only to be
                // immediately asked to initialize the project).
                confirm: ConfirmPolicy::HandlerManaged,
                force: *force,
            },
            WorkspaceAction::Worktree { action } => worktree_policy(action.as_ref()),
        },
        Commands::Pane { action } => {
            let (kind, confirm, force, stage) = match action {
                PaneAction::Split { force, .. } => (
                    CommandKind::Mutating,
                    ConfirmPolicy::HandlerManaged,
                    *force,
                    ConfirmationStage::BeforeMutation,
                ),
                PaneAction::Close { force } => (
                    CommandKind::Mutating,
                    ConfirmPolicy::HandlerManaged,
                    *force,
                    ConfirmationStage::BeforeMutation,
                ),
                PaneAction::List { .. } | PaneAction::Show { .. } => (
                    CommandKind::ReadOnly,
                    ConfirmPolicy::None,
                    true,
                    ConfirmationStage::None,
                ),
                PaneAction::ApproveHooks { .. } => (
                    CommandKind::Mutating,
                    ConfirmPolicy::None,
                    true,
                    ConfirmationStage::None,
                ),
                PaneAction::Send { force, .. } => (
                    CommandKind::Mutating,
                    ConfirmPolicy::HandlerManaged,
                    *force,
                    ConfirmationStage::BeforeMutation,
                ),
            };
            CommandPolicy {
                command_kind: kind,
                project_init_behavior: ProjectInitBehavior::RequireInitialized,
                confirmation_stage: stage,
                needs_global_config: true,
                check_deps: true,
                confirm,
                force,
            }
        }
        Commands::Project { action } => match action {
            ProjectAction::List { .. } => CommandPolicy {
                command_kind: CommandKind::ReadOnly,
                project_init_behavior: ProjectInitBehavior::Forbidden,
                confirmation_stage: ConfirmationStage::None,
                needs_global_config: true,
                check_deps: false,
                confirm: ConfirmPolicy::None,
                force: true,
            },
            ProjectAction::Reindex { force, dry_run, .. } => CommandPolicy {
                command_kind: if *dry_run {
                    CommandKind::ReadOnly
                } else {
                    CommandKind::Mutating
                },
                project_init_behavior: ProjectInitBehavior::Forbidden,
                confirmation_stage: if *dry_run {
                    ConfirmationStage::None
                } else {
                    ConfirmationStage::BeforeMutation
                },
                needs_global_config: true,
                check_deps: false,
                confirm: if *dry_run {
                    ConfirmPolicy::None
                } else {
                    ConfirmPolicy::HandlerManaged
                },
                force: *force || *dry_run,
            },
            ProjectAction::Roots { action } => match action {
                ProjectRootsAction::List | ProjectRootsAction::Status => CommandPolicy {
                    command_kind: CommandKind::ReadOnly,
                    project_init_behavior: ProjectInitBehavior::Forbidden,
                    confirmation_stage: ConfirmationStage::None,
                    needs_global_config: true,
                    check_deps: false,
                    confirm: ConfirmPolicy::None,
                    force: true,
                },
                ProjectRootsAction::Add { force, .. } => CommandPolicy {
                    command_kind: CommandKind::Mutating,
                    project_init_behavior: ProjectInitBehavior::Forbidden,
                    confirmation_stage: ConfirmationStage::None,
                    needs_global_config: true,
                    check_deps: false,
                    confirm: ConfirmPolicy::HandlerManaged,
                    force: *force,
                },
                ProjectRootsAction::Remove { force, .. } => CommandPolicy {
                    command_kind: CommandKind::Mutating,
                    project_init_behavior: ProjectInitBehavior::Forbidden,
                    confirmation_stage: ConfirmationStage::None,
                    needs_global_config: true,
                    check_deps: false,
                    confirm: ConfirmPolicy::HandlerManaged,
                    force: *force,
                },
                ProjectRootsAction::Edit => CommandPolicy {
                    command_kind: CommandKind::Mutating,
                    project_init_behavior: ProjectInitBehavior::Forbidden,
                    confirmation_stage: ConfirmationStage::None,
                    needs_global_config: true,
                    check_deps: false,
                    confirm: ConfirmPolicy::HandlerManaged,
                    force: false,
                },
            },
        },
        Commands::Session {
            action: SessionAction::List { .. },
        } => CommandPolicy {
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: true,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Session {
            action: SessionAction::Attach { .. },
        } => CommandPolicy {
            // `session attach` resolves every persisted runtime binding
            // globally. A dependency check based on the current directory
            // would let one unavailable backend block unrelated sessions;
            // the global listing queries each backend scope independently and
            // the final attach resolves the selected binding exactly.
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: true,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Session {
            action: SessionAction::Resume { .. },
        } => CommandPolicy {
            // Resuming a native Herdr session is deliberately independent of
            // the current project. Herdr owns startup and restoration; this
            // command only resolves the global session name and delegates to
            // Herdr's native client entry point.
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Doctor { fix } => CommandPolicy {
            command_kind: if *fix {
                CommandKind::Mutating
            } else {
                CommandKind::ReadOnly
            },
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Completion { .. } => CommandPolicy {
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Update { force, .. } => CommandPolicy {
            // `update` is a mutating operation that replaces the binary,
            // but it does not require project initialization or global config.
            // The command handles its own confirmation flow unless --force is set.
            command_kind: CommandKind::Mutating,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: *force,
        },
        Commands::Runtime { .. } => CommandPolicy {
            command_kind: CommandKind::Mutating,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
        Commands::Internal { .. } => CommandPolicy {
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::Forbidden,
            confirmation_stage: ConfirmationStage::None,
            needs_global_config: false,
            check_deps: false,
            confirm: ConfirmPolicy::None,
            force: true,
        },
    }
}

// ── Shared utilities ──

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;

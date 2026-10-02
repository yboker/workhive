//! Command execution policy — declarative preflight checks.
//!
//! Each command declares its requirements (global config, project, session,
//! confirmation, dependency check) via `CommandPolicy`. The dispatcher runs
//! all preflight checks before handing control to the command handler.
//!
//! Design: enum + struct fields, per the enum-first modeling rule — no Java-style trait objects.

use camino::Utf8Path;
use workhive_core::domain::{
    CommandKind, ConfirmationStage, Locale, ProjectInitBehavior, ProjectInitDecision,
    ProjectInitState, RuntimeBackendId,
};
use workhive_core::i18n_keys as k;
use workhive_core::ports::{ConfigPort, I18nPort, WorkspaceFsPort};
use workhive_infra::i18n::StaticI18n;

use crate::cli::{Commands, WorkspaceAction};
use crate::context::CommandContext;
use crate::error::CliError;

/// What kind of confirmation a command requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmPolicy {
    /// No confirmation needed (read-only commands).
    None,
    /// Generic "Apply this change?" prompt.
    #[allow(dead_code)]
    GenericMutate,
    /// "Permanently destroy?" prompt with extra warning.
    ///
    /// Reserved for future destructive commands that prefer the global
    /// preflight to own the warning. `workspace destroy` deliberately
    /// uses [`ConfirmPolicy::HandlerManaged`] instead so its destructive
    /// warning fires AFTER project-state checks.
    #[allow(dead_code)]
    Destroy,
    /// Custom prompt — the handler manages its own confirmation.
    HandlerManaged,
}

/// Declarative policy for a command's preflight requirements.
///
/// The semantic fields (`command_kind`, `project_init_behavior`,
/// `confirmation_stage`) express *what* the command is, not *how* it runs.
/// This prevents handlers from inventing ad-hoc safety checks.
#[derive(Debug)]
pub struct CommandPolicy {
    /// Whether this command reads or writes state.
    pub command_kind: CommandKind,
    /// How this command interacts with project initialization.
    pub project_init_behavior: ProjectInitBehavior,
    /// When user confirmation is required relative to side effects.
    pub confirmation_stage: ConfirmationStage,
    /// Whether global configuration must exist before this command runs.
    pub needs_global_config: bool,
    /// Whether dependencies (git, tmux) must be verified.
    pub check_deps: bool,
    /// Confirmation behavior (legacy field driving the actual prompt style).
    pub confirm: ConfirmPolicy,
    /// Whether `--force` was passed.
    pub force: bool,
}

/// Runs preflight checks before handing control to the command handler.
/// Interactive first-run commands initialize global config before dependency
/// checks; non-interactive commands retain dependency-first short-circuiting.
///
/// The semantic fields (`command_kind`, `confirmation_stage`) are used
/// for compile-time documentation and test assertions. Runtime dispatch
/// still relies on `confirm` and `needs_global_config` to preserve
/// backward-compatible behavior while the policy vocabulary is adopted.
pub fn run_preflight(
    policy: &CommandPolicy,
    command: &Commands,
    original_command: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();

    tracing::debug!(
        command = original_command,
        kind = ?policy.command_kind,
        init_behavior = ?policy.project_init_behavior,
        confirmation = ?policy.confirmation_stage,
        "policy preflight start"
    );

    use std::io::IsTerminal;

    let interactive_global_init = policy.needs_global_config && std::io::stdin().is_terminal();
    if interactive_global_init {
        ensure_global_config(original_command, ctx)?;
    }

    if policy.check_deps {
        let project_root = crate::commands::project::detect_project()
            .ok()
            .map(|(root, _)| root);
        let backend = ctx.runtime_backend_for(project_root.as_deref())?;
        if backend == RuntimeBackendId::herdr() {
            crate::wizard::check_dependency_binaries_for_backend(
                &*ctx.deps, &backend, &ctx.i18n, locale, false,
            )?;
            if matches!(
                command,
                Commands::Workspace {
                    action: WorkspaceAction::Open { .. },
                }
            ) {
                #[cfg(unix)]
                {
                    let session = ctx.herdr_session_for(project_root.as_deref())?;
                    workhive_infra::dependency::ensure_herdr_session_server(&session)
                        .map_err(CliError::RuntimeOp)?;
                }
            }
        } else {
            crate::wizard::check_dependencies_for_backend(
                &*ctx.deps, &backend, &ctx.i18n, locale, false,
            )?;
        }
    }

    if policy.needs_global_config && !interactive_global_init {
        ensure_global_config(original_command, ctx)?;
    }

    run_confirm(policy, &ctx.i18n, locale)?;

    Ok(())
}

/// Enforce `project_init_behavior` after the handler has resolved the project root.
///
/// This is the **single entry point** for project-level policy enforcement.
/// Handlers call this immediately after `detect_project()`, before any naming
/// resolution or state mutation.
///
/// Returns the verified `instance_id` when the policy requires an initialized
/// project. Returns `None` for behaviors that do not require immediate
/// project-local state (`Forbidden`, global fallback, and auto-init).
pub fn enforce_project_policy(
    policy: &CommandPolicy,
    project_root: &Utf8Path,
    ws_fs: &dyn WorkspaceFsPort,
    ctx: &CommandContext,
) -> Result<Option<String>, CliError> {
    match policy.project_init_behavior {
        ProjectInitBehavior::Forbidden => Ok(None),
        ProjectInitBehavior::RequireInitialized => {
            let init_state =
                workhive_core::usecases::project_state::resolve_init_state(ws_fs, project_root);
            match init_state {
                ProjectInitState::Initialized { instance_id } => Ok(Some(instance_id)),
                ProjectInitState::PartiallyInitialized { .. } => {
                    let locale = ctx.locale();
                    Err(CliError::Runtime(
                        ctx.i18n
                            .resolve(locale, k::ERROR_PROJECT_NOT_INITIALIZED_HINT),
                    ))
                }
                ProjectInitState::Uninitialized => {
                    let locale = ctx.locale();
                    Err(CliError::Runtime(
                        ctx.i18n
                            .resolve(locale, k::ERROR_PROJECT_NOT_INITIALIZED_HINT),
                    ))
                }
            }
        }
        ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized => Ok(None),
        ProjectInitBehavior::AllowAutoInitWithExplicitConsent => Ok(None),
        // The handler owns all project-state decisions. Returning Ok(None)
        // here keeps the global preflight free of side effects so the
        // handler can short-circuit (e.g. on missing `.workhive/`) WITHOUT
        // any prior destructive prompt being shown.
        ProjectInitBehavior::NotApplicable => Ok(None),
    }
}

/// Resolve the project initialization decision for commands with
/// `AllowAutoInitWithExplicitConsent` behavior.
///
/// Returns a domain-level `ProjectInitDecision` or `ProjectInitError`.
/// The CLI caller is responsible for rendering errors via i18n.
pub fn resolve_project_init_decision(
    project_root: &Utf8Path,
    ws_fs: &dyn WorkspaceFsPort,
    is_interactive: bool,
    explicit_init_flag: bool,
) -> Result<ProjectInitDecision, workhive_core::errors::ProjectInitError> {
    let init_state =
        workhive_core::usecases::project_state::resolve_init_state(ws_fs, project_root);

    tracing::info!(
        ?init_state,
        is_interactive,
        explicit_init_flag,
        "resolving project init decision"
    );

    match init_state {
        ProjectInitState::Initialized { instance_id } => {
            Ok(ProjectInitDecision::AlreadyInitialized { instance_id })
        }
        ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => Err(
            workhive_core::errors::ProjectInitError::PartiallyInitialized {
                root: project_root.to_owned(),
                has_dir,
                has_instance_id,
                has_config,
            },
        ),
        ProjectInitState::Uninitialized => {
            if explicit_init_flag {
                tracing::info!("project init approved via --init flag");
                Ok(ProjectInitDecision::ApprovedAutoInit)
            } else if is_interactive {
                Ok(ProjectInitDecision::NeedsConfirmation)
            } else {
                tracing::warn!("non-interactive mode: project not initialized and no --init flag");
                Ok(ProjectInitDecision::DeniedNonInteractive)
            }
        }
    }
}

/// Assert that a policy is self-consistent.
///
/// Returns `true` if the semantic fields agree with the legacy fields.
/// Used in tests to catch policy declaration bugs at compile time rather
/// than at runtime via subtle behavioral regressions.
#[cfg(test)]
pub fn is_policy_consistent(policy: &CommandPolicy) -> bool {
    use workhive_core::domain::{CommandKind, ConfirmationStage};

    let kind_ok = match policy.command_kind {
        CommandKind::ReadOnly => {
            policy.confirm == ConfirmPolicy::None
                && policy.confirmation_stage == ConfirmationStage::None
        }
        CommandKind::Mutating => true,
    };

    let stage_ok = match policy.confirmation_stage {
        ConfirmationStage::None => policy.confirm == ConfirmPolicy::None || policy.force,
        ConfirmationStage::BeforeMutation => policy.command_kind == CommandKind::Mutating,
    };

    kind_ok && stage_ok
}

/// Centralized confirmation dispatcher.
fn run_confirm(policy: &CommandPolicy, i18n: &StaticI18n, locale: Locale) -> Result<(), CliError> {
    let msg = match policy.confirm {
        ConfirmPolicy::None | ConfirmPolicy::HandlerManaged => return Ok(()),
        ConfirmPolicy::GenericMutate => i18n.resolve(locale, k::CONFIRM_GENERIC_MUTATE),
        ConfirmPolicy::Destroy => {
            eprintln!("{}", i18n.resolve(locale, k::DESTROY_WARNING));
            i18n.resolve(locale, k::CONFIRM_DESTROY)
        }
    };

    crate::confirm::require_confirm(&msg, policy.force)
}

/// Ensures global configuration exists. In interactive mode, initializes it
/// and returns to the current dispatch so the original command runs once.
fn ensure_global_config(original_command: &str, ctx: &CommandContext) -> Result<(), CliError> {
    use std::io::IsTerminal;

    let locale = ctx.locale();

    match ctx.config.global_exists() {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => return Err(CliError::from(e)),
    }

    if !std::io::stdin().is_terminal() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::ERROR_GLOBAL_NOT_INITIALIZED),
        ));
    }

    eprintln!();
    eprintln!("{}", ctx.i18n.resolve(locale, k::INIT_GUIDE_GLOBAL));
    eprintln!();

    let global_config = crate::wizard::run_wizard(crate::wizard::WizardRunRequest {
        force: false,
        config_exists: false,
        config_path: "~/.config/workhive/config.toml",
        target: crate::wizard::InitConfigTarget::Global,
        backend: None,
        deps: &*ctx.deps,
        exec_checker: &ctx.composition.executable_checker(),
        config_port: &ctx.config,
        i18n: &ctx.i18n,
        locale,
    })?;
    workhive_core::usecases::init::init_global_force(&ctx.config, global_config)?;

    let locale_after = ctx.locale();
    eprintln!();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve(locale_after, k::INIT_GLOBAL_COMPLETE_CONTINUE)
    );
    tracing::info!(
        command = original_command,
        "global init complete; resuming command"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::cell::RefCell;
    use workhive_core::ports::{AppConfig, DependencyKind, DependencyPort, DependencyStatus};

    use std::rc::Rc;
    struct MissingHerdr(Rc<RefCell<Vec<DependencyKind>>>);

    impl DependencyPort for MissingHerdr {
        fn check_all(&self) -> Vec<DependencyStatus> {
            vec![
                self.check(DependencyKind::Git),
                self.check(DependencyKind::Herdr),
            ]
        }

        fn check(&self, kind: DependencyKind) -> DependencyStatus {
            self.0.borrow_mut().push(kind);
            if kind == DependencyKind::Herdr {
                DependencyStatus::Missing { kind }
            } else {
                DependencyStatus::Builtin { kind }
            }
        }
    }

    #[test]
    fn preflight_checks_herdr_once_and_blocks_before_starting_it() {
        let command = crate::cli::Cli::try_parse_from(["workhive", "workspace", "open", "main"])
            .unwrap()
            .command;
        let config = AppConfig {
            backend: Some(RuntimeBackendId::herdr()),
            ..Default::default()
        };
        let mut ctx = CommandContext::new();
        ctx.config = Box::new(
            workhive_testkit::fakes::FakeConfig::new()
                .with_global(config.clone())
                .with_project(config),
        );
        let checks = Rc::new(RefCell::new(Vec::new()));
        ctx.deps = Box::new(MissingHerdr(Rc::clone(&checks)));
        let policy = CommandPolicy {
            check_deps: true,
            needs_global_config: false,
            command_kind: CommandKind::ReadOnly,
            project_init_behavior: ProjectInitBehavior::NotApplicable,
            confirmation_stage: ConfirmationStage::None,
            confirm: ConfirmPolicy::None,
            force: false,
        };
        assert!(run_preflight(&policy, &command, "test", &ctx).is_err());
        assert_eq!(
            *checks.borrow(),
            [DependencyKind::Git, DependencyKind::Herdr]
        );
    }
}

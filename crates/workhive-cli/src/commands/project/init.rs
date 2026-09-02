#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

// ══════════════════════════════════════════════════════════════════════════════
// Command handlers — each receives only `&CommandContext`
// ══════════════════════════════════════════════════════════════════════════════

// ── Init ──

/// Unified init mode enum — Global vs Project.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InitMode {
    Global,
    Project,
}

/// Unified init strategy — Interactive vs NonInteractive.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InitStrategy {
    Interactive,
    NonInteractive { force: bool },
}

pub(crate) fn init_target_for_mode(mode: InitMode) -> wizard::InitConfigTarget {
    match mode {
        InitMode::Global => wizard::InitConfigTarget::Global,
        InitMode::Project => wizard::InitConfigTarget::Project,
    }
}

pub(crate) fn init_completion_guidance_interactive(strategy: InitStrategy) -> bool {
    matches!(strategy, InitStrategy::Interactive)
}

/// Single init execution kernel, driven by mode + strategy.
pub(crate) fn execute_init(
    mode: InitMode,
    strategy: InitStrategy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    // For project init, ensure global config exists first
    if matches!(mode, InitMode::Project) && !ctx.config.global_exists()? {
        match strategy {
            InitStrategy::NonInteractive { .. } if !std::io::stdin().is_terminal() => {
                return Err(CliError::Runtime(
                    ctx.i18n.resolve(locale, k::ERROR_GLOBAL_NOT_INITIALIZED),
                ));
            }
            _ => {
                eprintln!("{}", ctx.i18n.resolve(locale, k::INIT_GUIDE_GLOBAL));
                let global_config = wizard::run_wizard(wizard::WizardRunRequest {
                    force: false,
                    config_exists: false,
                    config_path: "~/.config/workhive/config.toml",
                    target: wizard::InitConfigTarget::Global,
                    deps: &*ctx.deps,
                    exec_checker: &ctx.composition.executable_checker(),
                    config_port: &ctx.config,
                    i18n: &ctx.i18n,
                    locale,
                })?;
                usecases::init::init_global_force(&ctx.config, global_config)?;
                let locale_after = ctx.locale();
                eprintln!();
                let hint = ctx.i18n.resolve_with_args(
                    locale_after,
                    k::INTERCEPT_RERUN_HINT,
                    &["workhive init"],
                );
                return Err(CliError::GlobalConfigJustInitialized(hint));
            }
        }
    }

    let (config_exists, config_path) = match mode {
        InitMode::Global => (ctx.config.global_exists()?, display_global_config_path()),
        InitMode::Project => {
            let (root, _) = detect_project()?;
            let exists = ctx.config.project_exists(&root)?;
            let path = display_project_config_path();
            (exists, path)
        }
    };

    let app_config = match strategy {
        InitStrategy::Interactive => wizard::run_wizard(wizard::WizardRunRequest {
            force: false,
            config_exists,
            config_path: &config_path,
            target: init_target_for_mode(mode),
            deps: &*ctx.deps,
            exec_checker: &ctx.composition.executable_checker(),
            config_port: &ctx.config,
            i18n: &ctx.i18n,
            locale,
        })?,
        InitStrategy::NonInteractive { force } => {
            wizard::check_dependencies(&*ctx.deps, &ctx.i18n, locale)?;
            let confirm_key = match mode {
                InitMode::Global => k::CONFIRM_INIT_GLOBAL,
                InitMode::Project => k::CONFIRM_INIT_PROJECT,
            };
            confirm::require_confirm(&ctx.i18n.resolve(locale, confirm_key), force)?;
            let selected_locale = select_language(force, &ctx.i18n, locale)?;
            wizard::build_default_config(
                selected_locale,
                init_target_for_mode(mode),
                &ctx.composition.executable_checker(),
            )
        }
    };

    match mode {
        InitMode::Global => {
            let configured_roots = app_config.trusted_roots();
            // After the
            // interactive wizard chose the language, all downstream
            // success/next-steps copy must honor *that* selection, not
            // the pre-wizard `locale`. Fall back to the CLI-level locale
            // when the wizard did not express an explicit preference
            // (non-interactive bootstrapping path).
            let effective_locale = app_config.language.unwrap_or(locale);
            // AI integration feature gate is resolved against the same
            // `AppConfig` value we are about to persist, so the
            // opportunistic prepare loop below sees exactly the
            // configuration the user just authored. There is no project
            // config at this point (global init).
            let ai_gate = workhive_core::usecases::ai_integrations::FeatureGate::from_config(Some(
                &app_config,
            ));
            usecases::init::init_global_force(&ctx.config, app_config)?;
            eprintln!(
                "{}",
                ctx.i18n.resolve(effective_locale, k::INIT_GLOBAL_SUCCESS)
            );
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve(effective_locale, k::INIT_GLOBAL_NEXT_STEPS)
            );

            // If the user configured trusted roots in the wizard, offer to
            // build an initial project index right now so the global
            // project/session list works without a separate manual step.
            if !configured_roots.is_empty() {
                offer_initial_reindex(&configured_roots, ctx, Some(effective_locale))?;
            }

            // Explain `workhive completion install`, ask for explicit
            // consent on a TTY before touching shell startup files,
            // and surface a manual command otherwise. The guidance
            // function is the ONLY place in the global init flow that
            // may invoke the shell-completion install routine, and it
            // gates that invocation behind an interactive Yes.
            crate::commands::completions::guide_completion_install_after_global_init(
                ctx,
                effective_locale,
                init_completion_guidance_interactive(strategy),
            )?;

            // Opportunistic pre-maintenance:
            // only prepare integrations for AI providers that are
            // *already discoverable* on PATH. Tools the user has not
            // installed are skipped entirely — no phantom third-party
            // configs are created. The pane supervisor performs the
            // mandatory strong-fallback prepare on the very first
            // launch of any provider that appears later.
            let checker = ctx.composition.executable_checker();
            let _ = crate::commands::integrations::opportunistic_prepare_discoverable(
                &checker, &ai_gate,
            );
        }
        InitMode::Project => {
            let (root, name) = detect_project()?;
            // Pre-flight trusted-roots admission gate — must run BEFORE we
            // touch the filesystem, lock files, or the project index.
            enforce_trusted_root_or_error(root.as_ref(), ctx)?;
            let ws_fs = ctx.composition.workspace_fs();
            let index = ctx.composition.project_index()?;
            let lock_adapter = ctx.composition.project_init_lock();
            let was_initialized = usecases::init::init_project_transactional_with_config(
                &ctx.config,
                &ws_fs,
                &index,
                &lock_adapter,
                &root,
                &name,
                app_config.clone(),
            )?;
            if !was_initialized {
                ctx.config.save_project(&root, &app_config)?;
            }

            // Create project identity metadata after successful init.
            let instance_id = ws_fs.read_instance_id(root.as_ref()).ok_or_else(|| {
                CliError::Runtime(
                    "instance_id must exist after init — cannot create identity metadata"
                        .to_owned(),
                )
            })?;
            let canonical_project = SessionNamingPolicy::sanitize_component(&name);
            let identity_adapter = ctx.composition.workspace_identity(root.as_ref());
            usecases::identity::ensure_project_identity(
                &identity_adapter,
                &instance_id,
                &root,
                &name,
                &canonical_project,
            )?;

            // Same locale-after-wizard rule as the global branch above.
            let effective_locale = app_config.language.unwrap_or(locale);
            eprintln!(
                "{}",
                ctx.i18n.resolve(effective_locale, k::INIT_PROJECT_SUCCESS)
            );
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve(effective_locale, k::INIT_PROJECT_NEXT_STEPS)
            );
        }
    }

    Ok(())
}

pub(crate) fn cmd_init(global: bool, force: bool, ctx: &CommandContext) -> Result<(), CliError> {
    let mode = if global {
        InitMode::Global
    } else {
        InitMode::Project
    };

    let strategy = if std::io::stdin().is_terminal() && !force {
        InitStrategy::Interactive
    } else {
        InitStrategy::NonInteractive { force }
    };

    execute_init(mode, strategy, ctx)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MissingProjectInitFallback {
    ConfigError,
    RuntimeHint,
}

pub(crate) fn ensure_project_initialized_or_delegate(
    project_root: &camino::Utf8Path,
    original_command: &str,
    fallback: MissingProjectInitFallback,
    ws_fs: &dyn WorkspaceFsPort,
    ctx: &CommandContext,
) -> Result<Option<String>, CliError> {
    use workhive_core::domain::ProjectInitState;
    use workhive_core::i18n_keys as k;

    match usecases::project_state::resolve_init_state(ws_fs, project_root) {
        ProjectInitState::Initialized { instance_id } => Ok(Some(instance_id)),
        ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => {
            let locale = ctx.locale();
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_PROJECT_PARTIAL_INIT,
                &[
                    project_root.as_str(),
                    &has_dir.to_string(),
                    &has_instance_id.to_string(),
                    &has_config.to_string(),
                ],
            )))
        }
        ProjectInitState::Uninitialized => {
            let locale = ctx.locale();
            let fallback_err = || match fallback {
                MissingProjectInitFallback::ConfigError => {
                    workhive_core::errors::ConfigError::ProjectNotInitialized.into()
                }
                MissingProjectInitFallback::RuntimeHint => CliError::Runtime(ctx.i18n.resolve(
                    locale,
                    workhive_core::i18n_keys::ERROR_PROJECT_NOT_INITIALIZED_HINT,
                )),
            };

            if !std::io::stdin().is_terminal() {
                return Err(fallback_err());
            }

            let init_prompt = ctx.i18n.resolve(locale, k::INTERCEPT_PROJECT_INIT_PROMPT);
            let should_init =
                crate::commands::confirm_with_transcript(ctx, locale, &init_prompt, true)?;
            if !should_init {
                return Err(fallback_err());
            }

            cmd_init(false, false, ctx)?;

            let locale_after = ctx.locale();
            let hint = ctx.i18n.resolve_with_args(
                locale_after,
                k::INTERCEPT_RERUN_HINT,
                &[original_command],
            );
            Err(CliError::ProjectJustInitialized(hint))
        }
    }
}

pub(crate) fn enforce_or_delegate_project_policy(
    pol: &policy::CommandPolicy,
    project_root: &camino::Utf8Path,
    original_command: &str,
    ws_fs: &dyn WorkspaceFsPort,
    ctx: &CommandContext,
) -> Result<Option<String>, CliError> {
    use workhive_core::domain::ProjectInitBehavior;

    match pol.project_init_behavior {
        ProjectInitBehavior::RequireInitialized => ensure_project_initialized_or_delegate(
            project_root,
            original_command,
            MissingProjectInitFallback::RuntimeHint,
            ws_fs,
            ctx,
        ),
        ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized => {
            match usecases::project_state::resolve_init_state(ws_fs, project_root) {
                workhive_core::domain::ProjectInitState::Initialized { instance_id } => {
                    Ok(Some(instance_id))
                }
                workhive_core::domain::ProjectInitState::PartiallyInitialized { .. }
                | workhive_core::domain::ProjectInitState::Uninitialized => Ok(None),
            }
        }
        _ => policy::enforce_project_policy(pol, project_root, ws_fs, ctx),
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeInfo};
    #[allow(unused_imports)]
    use workhive_core::usecases::worktree_safety::{
        UnmergedCommitSummary, WorktreeRisk, WorktreeSafetyReport,
    };

    #[test]
    fn init_strategy_allows_completion_prompt_only_for_interactive_runs() {
        assert!(
            init_completion_guidance_interactive(InitStrategy::Interactive),
            "interactive init may offer the post-init completion prompt"
        );
        assert!(
            !init_completion_guidance_interactive(InitStrategy::NonInteractive { force: true }),
            "--force must keep the completion guidance non-interactive"
        );
        assert!(
            !init_completion_guidance_interactive(InitStrategy::NonInteractive { force: false }),
            "piped or CI init must keep the completion guidance non-interactive"
        );
    }
}

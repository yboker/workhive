#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::{ExecutableAvailabilityPort, WorkspaceIdentityPort};

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

#[cfg(test)]
pub(crate) fn init_completion_guidance_interactive(strategy: InitStrategy) -> bool {
    matches!(strategy, InitStrategy::Interactive)
}

pub(crate) trait GlobalInitTrustedRootsInput {
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool, CliError>;
}

/// Completion guidance boundary for post-init shell completion setup.
trait CompletionGuidance {
    fn guide(
        &self,
        ctx: &CommandContext,
        locale: workhive_core::domain::Locale,
        interactive: bool,
    ) -> Result<(), CliError>;
}

struct TtyCompletionGuidance;

impl CompletionGuidance for TtyCompletionGuidance {
    fn guide(
        &self,
        ctx: &CommandContext,
        locale: workhive_core::domain::Locale,
        interactive: bool,
    ) -> Result<(), CliError> {
        crate::commands::completions::guide_completion_install_after_global_init(
            ctx,
            locale,
            interactive,
        )
    }
}

struct TtyGlobalInitTrustedRootsInput;

impl GlobalInitTrustedRootsInput for TtyGlobalInitTrustedRootsInput {
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool, CliError> {
        crate::ui::screens::confirm::run_with_context(prompt, None, default)
    }
}

fn run_global_init_trusted_roots_step(
    configured_roots: &[camino::Utf8PathBuf],
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    interactive: bool,
    input: &dyn GlobalInitTrustedRootsInput,
    editor_input: &dyn super::roots_cmd::RootsEditorInput,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;

    if !interactive {
        return Ok(());
    }

    let prompt_variant = resolve_global_init_trusted_root_prompt(configured_roots.len());
    let default_yes = global_init_prompt_default_yes(prompt_variant);
    let prompt_text = match prompt_variant {
        GlobalInitTrustedRootPrompt::OfferConfigure => ctx
            .i18n
            .resolve(locale, k::INIT_TRUSTED_ROOTS_CONFIGURE_NOW),
        GlobalInitTrustedRootPrompt::OfferViewModify { count } => {
            let count = count.to_string();
            ctx.i18n
                .resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_VIEW_MODIFY, &[&count])
        }
    };

    if input.confirm(&prompt_text, default_yes)? {
        // The global init flow continues regardless of editor outcome (Saved, Unchanged, or Cancelled).
        // All outcomes are acceptable; no post-save action is required here.
        let _ = super::roots_cmd::cmd_project_roots_edit_with_input(ctx, editor_input)?;
    }
    Ok(())
}

fn render_global_init_config(
    config: &workhive_core::ports::AppConfig,
    preserved_projects: Option<&toml_edit::Item>,
) -> Result<String, CliError> {
    let raw = toml::to_string_pretty(config).map_err(|error| {
        CliError::Runtime(format!("failed to serialize global config: {error}"))
    })?;
    let Some(projects) = preserved_projects else {
        return Ok(raw);
    };
    let mut document = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| CliError::Runtime(format!("failed to render global config: {error}")))?;
    document.insert("projects", projects.clone());
    Ok(document.to_string())
}

/// Global init production kernel: overwrite/restore preservation + trusted roots step.
///
/// Accepts injectable confirmation/editor/completion-guidance boundaries and terminal decision.
/// Backend changes use the same transactional replacement path as `config set`.
fn execute_global_init_kernel(
    ctx: &CommandContext,
    app_config: workhive_core::ports::AppConfig,
    interactive: bool,
    input: &dyn GlobalInitTrustedRootsInput,
    editor_input: &dyn super::roots_cmd::RootsEditorInput,
    completion_guidance: &dyn CompletionGuidance,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    app_config.validate()?;
    let locale = ctx.locale();

    let preserved_projects_table = preserve_global_projects_toml(&ctx.config)?;
    let previous_raw = if ctx.config.global_exists()? {
        Some(
            ctx.config
                .load_raw_toml(None, true)
                .map_err(|e| CliError::Runtime(format!("failed to read global config: {e}")))?,
        )
    } else {
        None
    };
    let previous_config = if previous_raw.is_some() {
        ctx.config
            .load_global()
            .map_err(|e| CliError::Runtime(format!("failed to read global config: {e}")))?
    } else {
        None
    };
    let configured_roots = match preserved_projects_table.as_ref() {
        Some(_) => previous_config
            .as_ref()
            .map(workhive_core::ports::AppConfig::trusted_roots)
            .unwrap_or_default(),
        None => app_config.trusted_roots(),
    };
    // After the
    // interactive wizard chose the language, all downstream
    // success/next-steps copy must honor *that* selection, not
    // the pre-wizard `locale`. Fall back to the CLI-level locale
    // when the wizard did not express an explicit preference
    // (non-interactive bootstrapping path).
    let effective_locale = app_config.language.unwrap_or(locale);
    let ai_gate =
        workhive_core::usecases::ai_integrations::FeatureGate::from_config(Some(&app_config));
    let target_backend = app_config
        .backend
        .clone()
        .unwrap_or_else(workhive_core::domain::RuntimeBackendId::tmux);
    let previous_backend = previous_config
        .as_ref()
        .and_then(|config| config.backend.clone())
        .unwrap_or_else(workhive_core::domain::RuntimeBackendId::tmux);
    let backend_changed = previous_config.is_some() && previous_backend != target_backend;

    if backend_changed {
        let previous_raw = previous_raw.as_deref().ok_or_else(|| {
            CliError::Runtime("global config content is missing for backend replacement".to_owned())
        })?;
        let updated_raw =
            render_global_init_config(&app_config, preserved_projects_table.as_ref())?;
        let roots = global_backend_impact(ctx, &target_backend)?
            .into_iter()
            .map(|(root, _)| root)
            .collect::<Vec<_>>();
        replace_global_backend(ctx, &roots, &target_backend, &updated_raw, previous_raw)?;
    } else {
        usecases::init::init_global_force(&ctx.config, app_config)?;
        if let Some(ref projects_item) = preserved_projects_table {
            restore_global_projects_toml(&ctx.config, projects_item)?;
        }
    }
    eprintln!(
        "{}",
        ctx.i18n.resolve(effective_locale, k::INIT_GLOBAL_SUCCESS)
    );
    eprintln!(
        "{}",
        ctx.i18n
            .resolve(effective_locale, k::INIT_GLOBAL_NEXT_STEPS)
    );

    // Explain `workhive completion install`, ask for explicit
    // consent on a TTY before touching shell startup files,
    // and surface a manual command otherwise. The guidance
    // function is the ONLY place in the global init flow that
    // may invoke the shell-completion install routine, and it
    // gates that invocation behind an interactive Yes.
    completion_guidance.guide(ctx, effective_locale, interactive)?;

    // Opportunistic pre-maintenance:
    // only prepare integrations for AI providers that are
    // *already discoverable* on PATH. Tools the user has not
    // installed are skipped entirely — no phantom third-party
    // configs are created. The pane supervisor performs the
    // mandatory strong-fallback prepare on the very first
    // launch of any provider that appears later.
    let checker = ctx.composition.executable_checker();
    let _ = crate::commands::integrations::opportunistic_prepare_discoverable(&checker, &ai_gate);

    // Trusted-directory configuration is the final optional
    // onboarding step in global init.
    run_global_init_trusted_roots_step(
        &configured_roots,
        ctx,
        effective_locale,
        interactive,
        input,
        editor_input,
    )?;

    Ok(())
}

/// Single init execution kernel, driven by mode + strategy.
pub(crate) fn execute_init(
    mode: InitMode,
    strategy: InitStrategy,
    requested_backend: Option<workhive_core::domain::RuntimeBackendId>,
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
                let global_config = wizard::run_wizard(wizard::WizardRunRequest {
                    force: false,
                    config_exists: false,
                    config_path: "~/.config/workhive/config.toml",
                    target: wizard::InitConfigTarget::Global,
                    backend: requested_backend.clone(),
                    deps: &*ctx.deps,
                    exec_checker: &ctx.composition.executable_checker(),
                    config_port: &ctx.config,
                    i18n: &ctx.i18n,
                    locale,
                })?;
                usecases::init::init_global_force(&ctx.config, global_config)?;
                let locale_after = ctx.locale();
                eprintln!();
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve(locale_after, k::INIT_GLOBAL_COMPLETE_CONTINUE)
                );
                tracing::info!("global init complete; continuing project initialization");
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
            backend: requested_backend.clone(),
            deps: &*ctx.deps,
            exec_checker: &ctx.composition.executable_checker(),
            config_port: &ctx.config,
            i18n: &ctx.i18n,
            locale,
        })?,
        InitStrategy::NonInteractive { force } => {
            if matches!(mode, InitMode::Global) && requested_backend.is_none() {
                return Err(CliError::Runtime(
                    "global initialization requires --backend tmux or --backend herdr".to_owned(),
                ));
            }
            let effective_backend = match &requested_backend {
                Some(backend) => backend.clone(),
                None => workhive_core::usecases::config_mgmt::resolve_effective_backend(
                    None,
                    ctx.config.load_global()?.as_ref(),
                ),
            };
            wizard::check_dependency_binaries_for_backend(
                &*ctx.deps,
                &effective_backend,
                &ctx.i18n,
                locale,
                true,
            )?;
            let confirm_key = match mode {
                InitMode::Global => k::CONFIRM_INIT_GLOBAL,
                InitMode::Project => k::CONFIRM_INIT_PROJECT,
            };
            confirm::require_confirm(&ctx.i18n.resolve(locale, confirm_key), force)?;
            let selected_locale = select_language(force, &ctx.i18n, locale)?;
            wizard::build_default_config(
                selected_locale,
                init_target_for_mode(mode),
                requested_backend.clone(),
                &ctx.composition.executable_checker(),
            )
        }
    };

    match mode {
        InitMode::Global => {
            execute_global_init_kernel(
                ctx,
                app_config,
                std::io::IsTerminal::is_terminal(&std::io::stdin()),
                &TtyGlobalInitTrustedRootsInput,
                &super::roots_cmd::TtyRootsEditorInput,
                &TtyCompletionGuidance,
            )?;
        }
        InitMode::Project => {
            let (root, name) = detect_project()?;
            // Pre-flight trusted-roots admission gate — must run BEFORE we
            // touch the filesystem, lock files, or the project index.
            enforce_trusted_root_or_error(root.as_ref(), ctx, AdmissionCaller::ProjectInit)?;
            let ws_fs = ctx.composition.workspace_fs();
            let index = ctx.composition.project_index()?;
            let lock_adapter = ctx.composition.project_init_lock();

            // Capture old instance_id BEFORE init for cleanup.
            // After init, the instance_id may change (PartiallyInitialized branch),
            // but we need the OLD id to match existing sessions.
            let old_instance_id = ws_fs.read_instance_id(&root);
            let identity_adapter = ctx.composition.workspace_identity(root.as_ref());
            let old_backend = match identity_adapter.list_workspace_identities()?.first() {
                Some(record) => record.runtime_binding().backend,
                None => ctx.runtime_backend_for(Some(root.as_ref()))?,
            };

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

            // After init, clean up old tmux sessions so the next `open` starts fresh.
            // The helper deliberately leaves Herdr sessions intact: they are shared
            // native runtime resources, not Workhive-owned servers.
            cleanup_project_sessions(&root, &name, old_instance_id.as_deref(), &old_backend, ctx)?;

            // Always ensure .gitignore contains workhive entries,
            // even on re-init of an already-initialized project.
            ws_fs.ensure_gitignore_workhive_entry(root.as_ref())?;

            // Create project identity metadata after successful init.
            let instance_id = ws_fs.read_instance_id(root.as_ref()).ok_or_else(|| {
                CliError::Runtime(
                    "instance_id must exist after init — cannot create identity metadata"
                        .to_owned(),
                )
            })?;
            let canonical_project = SessionNamingPolicy::sanitize_component(&name);
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

/// Re-init cleans up tmux workspaces, preserving the existing backend lifecycle.
/// Herdr sessions are shared user resources, so Workhive leaves them intact.
///
/// Safety features:
/// - Skips attached sessions (don't kill active user work)
/// - Treats only genuine runtime absence as "already gone" (idempotent)
/// - Propagates capability, permission, protocol, and other runtime errors
///
/// Uses `old_instance_id` (captured before init) to match existing sessions,
/// since init may have changed the instance_id on disk.
fn cleanup_project_sessions(
    project_root: &camino::Utf8Path,
    project_name: &str,
    old_instance_id: Option<&str>,
    backend: &workhive_core::domain::RuntimeBackendId,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::ports::RuntimeError;

    if backend == &workhive_core::domain::RuntimeBackendId::herdr() {
        tracing::info!(
            project = project_name,
            "re-init leaves shared Herdr sessions intact"
        );
        return Ok(());
    }

    let Some(instance_id) = old_instance_id else {
        tracing::debug!(
            project = project_name,
            "re-init session cleanup: no old instance_id (first init)"
        );
        return Ok(());
    };

    let ws_fs = ctx.composition.workspace_fs();
    let git = ctx.composition.git();

    // Build naming context using the old instance_id.
    let naming_ctx =
        build_naming_context(project_name, "", &project_root.to_path_buf(), &git, &ws_fs);
    let np = ctx.naming_policy_for(Some(project_root))?;
    let server = np.resolve_server_for_project(&naming_ctx)?;
    let runtime = match ctx.composition.runtime_for_project(project_root, backend) {
        Ok(runtime) => runtime,
        Err(CliError::RuntimeOp(RuntimeError::BackendUnavailable { reason, .. }))
            if reason.is_runtime_absent() =>
        {
            tracing::debug!(
                project = project_name,
                reason = ?reason,
                "re-init session cleanup: runtime absent (already cleaned)"
            );
            return Ok(());
        }
        Err(error) => return Err(error),
    };

    let workspaces = match runtime.list_project_workspaces(&server, instance_id, project_name) {
        Ok(ws) => ws,
        Err(RuntimeError::BackendUnavailable { reason, detail }) if reason.is_runtime_absent() => {
            tracing::debug!(
                server = %server,
                instance_id = %instance_id,
                detail = %detail,
                "re-init session cleanup: runtime absent (already cleaned)"
            );
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };

    if workspaces.is_empty() {
        tracing::debug!(
            project = project_name,
            server = %server,
            instance_id = %instance_id,
            "re-init session cleanup: no stale sessions found"
        );
        return Ok(());
    }

    let mut destroyed_count = 0usize;
    let mut skipped_attached = 0usize;

    for ws in &workspaces {
        // Safety check: don't kill attached sessions (user may be active).
        if ws.attached {
            tracing::info!(
                session = %ws.runtime_id,
                "re-init cleanup: skipping attached session (user may be active)"
            );
            skipped_attached += 1;
            continue;
        }

        match runtime.destroy_workspace(&server, &ws.runtime_id) {
            Ok(()) | Err(RuntimeError::WorkspaceNotFound { .. }) => {
                destroyed_count += 1;
            }
            Err(RuntimeError::BackendUnavailable { reason, .. }) if reason.is_runtime_absent() => {
                tracing::debug!(
                    session = %ws.runtime_id,
                    reason = ?reason,
                    "re-init cleanup: runtime absent (session already gone)"
                );
                destroyed_count += 1;
            }
            Err(error) => return Err(error.into()),
        }
    }

    eprintln!(
        "Re-init session cleanup for project '{}': {} destroyed, {} attached (skipped)",
        project_name, destroyed_count, skipped_attached
    );
    Ok(())
}

pub(crate) fn cmd_init(
    global: bool,
    force: bool,
    backend: Option<&str>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
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

    let backend = backend.map(|value| match value {
        "tmux" => workhive_core::domain::RuntimeBackendId::tmux(),
        "herdr" => workhive_core::domain::RuntimeBackendId::herdr(),
        _ => unreachable!("clap validates runtime backend values"),
    });
    execute_init(mode, strategy, backend, ctx)
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

            cmd_init(false, false, None, ctx)?;

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

/// Read the existing global config raw TOML and extract the `[projects]`
/// table item (preserving comments and formatting). Returns `Ok(None)` when the
/// global config does not exist or has no `[projects]` section. Returns `Err`
/// when the global config exists but cannot be read or parsed.
fn preserve_global_projects_toml(
    config: &dyn workhive_core::ports::ConfigPort,
) -> Result<Option<toml_edit::Item>, CliError> {
    use toml_edit::DocumentMut;
    let exists = config
        .global_exists()
        .map_err(|e| CliError::Runtime(format!("failed to check global config existence: {e}")))?;
    if !exists {
        return Ok(None);
    }
    let raw = config
        .load_raw_toml(None, true)
        .map_err(|e| CliError::Runtime(format!("failed to read global config: {e}")))?;
    let doc: DocumentMut = raw
        .parse()
        .map_err(|e| CliError::Runtime(format!("failed to parse global config: {e}")))?;
    Ok(doc.get("projects").cloned())
}

/// After a global config overwrite, merge the preserved `[projects]` table
/// back into the new raw TOML document so the authored trusted roots,
/// comments, and formatting survive re-init.
fn restore_global_projects_toml(
    config: &dyn workhive_core::ports::ConfigPort,
    projects_item: &toml_edit::Item,
) -> Result<(), CliError> {
    use toml_edit::DocumentMut;
    let raw = config
        .load_raw_toml(None, true)
        .map_err(|e| CliError::Runtime(format!("failed to read global config for restore: {e}")))?;
    let mut doc: DocumentMut = raw.parse().map_err(|e| {
        CliError::Runtime(format!("failed to parse global config for restore: {e}"))
    })?;
    doc.insert("projects", projects_item.clone());
    config
        .save_raw_toml(None, true, &doc.to_string())
        .map_err(|e| CliError::Runtime(format!("failed to restore [projects] section: {e}")))?;
    Ok(())
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;

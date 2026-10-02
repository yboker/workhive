#[allow(unused_imports)]
use super::*;

fn run_after_open_scope_resolution<T>(
    resolution: Result<
        (
            workhive_core::usecases::open::ResolvedOpenScope,
            Option<workhive_core::ports::ProvisionOutcome>,
        ),
        workhive_core::usecases::open::OpenError,
    >,
    action: impl FnOnce(
        (
            workhive_core::usecases::open::ResolvedOpenScope,
            Option<workhive_core::ports::ProvisionOutcome>,
        ),
    ) -> Result<T, workhive_core::usecases::open::OpenError>,
) -> Result<T, workhive_core::usecases::open::OpenError> {
    action(resolution?)
}
use crate::commands::open_readiness;
#[allow(unused_imports)]
use crate::commands::*;

/// Map `OpenError` to `CliError` with i18n-resolved session availability text.
pub(crate) fn render_open_error(ctx: &CommandContext, err: usecases::open::OpenError) -> CliError {
    match err {
        usecases::open::OpenError::SessionLookupFailed {
            session,
            availability,
        } => CliError::Runtime(render_session_lookup_error(ctx, &session, &availability)),
        other => CliError::from(other),
    }
}

fn log_open_phase(step: &'static str, started_at: std::time::Instant) {
    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::debug!(
        target: "workhive.open",
        step,
        elapsed_ms,
        "workspace open phase completed"
    );
}

/// Removes a staged runtime and clears its pending binding unless commit wins.
struct ForceRebuildGuard<'a> {
    runtime: &'a dyn workhive_core::ports::WorkspaceRuntimePort,
    identity: &'a dyn workhive_core::ports::WorkspaceIdentityPort,
    launch_identity: &'a dyn workhive_core::ports::PaneLaunchIdentityPort,
    record: workhive_core::domain::WorkspaceIdentityRecord,
    target: workhive_core::domain::RuntimeBinding,
    target_owned: bool,
    pending_persisted: bool,
    armed: bool,
}

impl ForceRebuildGuard<'_> {
    fn disarm(&mut self) {
        self.armed = false;
    }
}

fn cleanup_error_is_already_done(error: &workhive_core::ports::RuntimeError) -> bool {
    match error {
        workhive_core::ports::RuntimeError::WorkspaceNotFound { .. } => true,
        workhive_core::ports::RuntimeError::BackendUnavailable { reason, .. } => {
            reason.is_runtime_absent()
        }
        _ => false,
    }
}

fn cleanup_runtime(
    runtime: &dyn workhive_core::ports::WorkspaceRuntimePort,
    binding: &workhive_core::domain::RuntimeBinding,
) -> Result<(), workhive_core::ports::RuntimeError> {
    match runtime.destroy_workspace_binding(binding) {
        Ok(()) => Ok(()),
        Err(error) if cleanup_error_is_already_done(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

fn cleanup_replacement_resources(
    runtime: &dyn workhive_core::ports::WorkspaceRuntimePort,
    launch_identity: &dyn workhive_core::ports::PaneLaunchIdentityPort,
    binding: &workhive_core::domain::RuntimeBinding,
) -> Result<(), CliError> {
    cleanup_runtime(runtime, binding)?;
    launch_identity
        .forget_workspace(&binding.scope_id, &binding.workspace_id)
        .map_err(|error| {
            CliError::Runtime(format!(
                "failed to clean pane launch identity for '{}': {error}",
                binding.workspace_id
            ))
        })?;
    Ok(())
}

fn handle_herdr_force_close_refusal(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    runtime: &dyn workhive_core::ports::WorkspaceRuntimePort,
    previous: &workhive_core::domain::RuntimeBinding,
    workspace: &str,
    no_attach: bool,
    interaction_policy: workhive_core::ports::RuntimeInteractionPolicy,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;

    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::OPEN_FORCE_REBUILD_NON_INTERACTIVE,
            &[workspace],
        )));
    }

    let title = ctx
        .i18n
        .resolve(locale, k::OPEN_FORCE_REBUILD_REFUSED_TITLE);
    let first_choice = if no_attach {
        k::OPEN_FORCE_REBUILD_KEEP_EXISTING
    } else {
        k::OPEN_FORCE_REBUILD_ENTER_EXISTING
    };
    let choices = vec![
        ctx.i18n.resolve(locale, first_choice),
        ctx.i18n.resolve(locale, k::OPEN_FORCE_REBUILD_CANCEL),
    ];
    if crate::ui::screens::select::run(&title, &choices, 0)? != 0 {
        return Err(CliError::UserAborted);
    }

    if !no_attach {
        runtime.attach_workspace_binding(previous, interaction_policy)?;
    }
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::OPEN_REUSING_SESSION, &[workspace])
    );
    Ok(())
}

fn register_runtime_checkout_exact(
    runtime: &dyn workhive_core::ports::WorkspaceRuntimePort,
    binding: &workhive_core::domain::RuntimeBinding,
    project_root: &camino::Utf8Path,
    workspace_root: &camino::Utf8Path,
) -> Result<(), CliError> {
    let authoritative = runtime.register_workspace_checkout(
        workhive_core::ports::RegisterWorkspaceCheckoutRequest {
            binding: binding.clone(),
            project_root: project_root.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
        },
    )?;
    if authoritative != *binding {
        return Err(CliError::Runtime(format!(
            "runtime checkout resolved to '{}', expected '{}'",
            authoritative.workspace_id, binding.workspace_id
        )));
    }
    Ok(())
}

fn remap_launch_identities(
    launch_identity: &dyn workhive_core::ports::PaneLaunchIdentityPort,
    group: &str,
    from: &str,
    to: &str,
) -> Result<(), workhive_core::ports::PaneLaunchIdentityError> {
    let entries = launch_identity.list_for_group(group)?;
    for mut entry in entries {
        if entry.workspace == from {
            entry.workspace = to.to_owned();
            launch_identity.record(group, entry)?;
        }
    }
    launch_identity.forget_workspace(group, from)?;
    Ok(())
}

fn require_runtime_binding(
    ctx: &CommandContext,
    locale: Locale,
    backend: workhive_core::domain::RuntimeBackendId,
    scope_id: String,
    workspace_id: String,
) -> Result<workhive_core::domain::RuntimeBinding, CliError> {
    workhive_core::domain::RuntimeBinding::new(backend, scope_id, workspace_id).ok_or_else(|| {
        CliError::Runtime(ctx.i18n.resolve(
            locale,
            workhive_core::i18n_keys::OPEN_RUNTIME_IDENTIFIERS_EMPTY,
        ))
    })
}

fn require_canonical_tmux_binding<'a>(
    ctx: &CommandContext,
    locale: Locale,
    binding: Option<&'a workhive_core::domain::RuntimeBinding>,
) -> Result<&'a workhive_core::domain::RuntimeBinding, CliError> {
    binding.ok_or_else(|| {
        CliError::Runtime(ctx.i18n.resolve(
            locale,
            workhive_core::i18n_keys::OPEN_TMUX_STAGED_BINDING_MISSING,
        ))
    })
}

fn note_pending_replacement_cleanup(
    ctx: &CommandContext,
    locale: Locale,
    binding: &workhive_core::domain::RuntimeBinding,
    error: &CliError,
    log_message: &'static str,
    i18n_key: &'static str,
) {
    tracing::warn!(
        server = %binding.scope_id,
        session = %binding.workspace_id,
        error = %error,
        "{log_message}"
    );
    eprintln!(
        "{}",
        ctx.i18n.resolve_with_args(
            locale,
            i18n_key,
            &[binding.workspace_id.as_str(), &error.to_string()],
        )
    );
}

/// Persist the promoted runtime after the backend rename has been resolved.
fn commit_promoted_runtime(
    identity: &dyn workhive_core::ports::WorkspaceIdentityPort,
    record: &mut workhive_core::domain::WorkspaceIdentityRecord,
) -> Result<(), CliError> {
    usecases::identity::finish_runtime_promotion(identity, record)?;
    usecases::identity::finish_runtime_replacement(identity, record)?;
    Ok(())
}

impl Drop for ForceRebuildGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if self.target_owned
            && let Err(error) =
                cleanup_replacement_resources(self.runtime, self.launch_identity, &self.target)
        {
            tracing::error!(
                server = %self.target.scope_id,
                session = %self.target.workspace_id,
                error = %error,
                "failed to compensate staged force-rebuild resources"
            );
            return;
        }
        if self.pending_persisted {
            self.record.runtime_replacement = None;
            if let Err(error) = self.identity.save_workspace_identity(&self.record) {
                tracing::error!(
                    error = %error,
                    "staged force-rebuild runtime was removed but pending binding could not be cleared"
                );
            }
        }
    }
}

fn should_commit_prepared_backend_change(
    active: &workhive_core::domain::RuntimeBackendId,
    target: &workhive_core::domain::RuntimeBackendId,
    effective: &workhive_core::domain::RuntimeBackendId,
) -> bool {
    active != target && target == effective
}

fn staged_runtime_name(session: &str) -> String {
    let mut canonical = session;
    while let Some((base, suffix)) = canonical.rsplit_once("--rebuild-") {
        let Some((pid, nonce)) = suffix.split_once('-') else {
            break;
        };
        if pid.parse::<u32>().is_err()
            || nonce.is_empty()
            || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            break;
        }
        canonical = base;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{canonical}--rebuild-{}-{nonce:x}", std::process::id())
}

fn staged_session_for_backend(
    backend: &workhive_core::domain::RuntimeBackendId,
    session: &str,
    has_previous_runtime: bool,
) -> Option<String> {
    (backend == &workhive_core::domain::RuntimeBackendId::tmux() && has_previous_runtime)
        .then(|| staged_runtime_name(session))
}

// ── Open ──

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_open(
    workspace: Option<&str>,
    no_attach: bool,
    force: bool,
    mode_str: &str,
    init_flag: bool,
    attach_existing_branch: bool,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::domain::WorkspaceInteractionMode;
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    // Total-elapsed step timing for `workspace open`. The tracing
    // boundary is kept concentrated at the entry and completion points
    // rather than spreading per-step span instrumentation across this
    // large flow. The two anchor logs below are the observable contract
    // every `workspace open` emits: a `step="open.start"` line at entry
    // and a
    // `step="open.complete"` line just before the final `Ok(())`, both
    // under the `workhive.open` tracing target and carrying
    // `elapsed_ms`. With `WORKHIVE_LOG=workhive.open=debug` an operator
    // can confirm where time is actually being spent without a new span
    // hierarchy threaded through the entire function.
    let __open_started_at = std::time::Instant::now();
    let mut __open_phase_started_at = __open_started_at;
    tracing::debug!(
        target: "workhive.open",
        step = "open.start",
        no_attach,
        force,
        mode = mode_str,
        "workspace open started"
    );

    let interaction_mode = match mode_str {
        "manual" => WorkspaceInteractionMode::Manual,
        "primary-assisted" | "primary_assisted" => WorkspaceInteractionMode::PrimaryAssisted,
        other => {
            return Err(CliError::Runtime(format!(
                "unknown mode '{other}', expected 'manual' or 'primary-assisted'"
            )));
        }
    };

    let (root, name) = detect_project()?;

    // Validate workspace ID IMMEDIATELY when explicitly provided, before any
    // other operations that might create files or have side effects.
    // This ensures that invalid workspace names fail fast without creating
    // index.json or other artifacts.
    if let Some(ws) = workspace {
        WorkspaceId::new(ws)?;
    }

    // Trusted-root admission gate — must run BEFORE every mutating
    // open path, regardless of init state or open intent. An
    // already-initialized project outside trusted roots is NOT
    // grandfathered: it must still be rejected before the CLI creates
    // tmux sessions, runtime state, workspace identity, or worktrees.
    crate::commands::project::enforce_trusted_root_or_error(
        root.as_ref(),
        ctx,
        crate::commands::project::AdmissionCaller::WorkspaceOpen,
    )?;

    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();
    let invocation_cwd = std::env::current_dir()
        .map_err(|e| CliError::Runtime(format!("cannot determine cwd: {e}")))
        .and_then(|p| {
            Utf8PathBuf::try_from(p)
                .map_err(|e| CliError::Runtime(format!("cwd is not valid UTF-8: {e}")))
        })?;

    let _ = policy::enforce_project_policy(pol, root.as_ref(), &ws_fs, ctx)?;
    log_open_phase("config_global_preflight", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    let worktrees = git.list_worktrees()?;

    let workspace_name = match workspace {
        Some(ws) => ws.to_owned(),
        None => {
            // The interactive selector needs a real terminal. Scripted and
            // agent callers must name the workspace explicitly; failing here
            // deterministically beats surfacing a raw terminal-mode error
            // from inside the TUI.
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                return Err(CliError::Runtime(
                    ctx.i18n.resolve(locale, k::OPEN_SELECTOR_NON_INTERACTIVE),
                ));
            }
            let (ws_name, _) = select_workspace_interactive(&git, ctx)?;
            WorkspaceId::new(&ws_name)?;
            ws_name
        }
    };
    let ws_id = WorkspaceId::new(&workspace_name)?;
    let open_intent = usecases::open::resolve_open_intent(&worktrees, &ws_id);
    log_open_phase("project_git_root", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Branch provenance must be recorded against the managed worktree
    // after provisioning succeeds. `None` means "leave the stored
    // provenance untouched" (reuse path / non-provision intent).
    let provenance_to_record: Option<workhive_core::ports::BranchProvenance> = if matches!(
        open_intent,
        workhive_core::domain::WorkspaceOpenIntent::Provision
    ) {
        resolve_provision_branch_provenance(
            &git,
            &ws_fs,
            ctx,
            root.as_ref(),
            &workspace_name,
            attach_existing_branch,
        )?
    } else {
        None
    };

    let was_auto_initialized =
        resolve_project_init_and_index(ctx, &ws_fs, &root, &name, init_flag)?;

    // ── Topology-first scope resolution ──
    // Provision the worktree (if needed) and resolve the typed open scope
    // BEFORE naming, runtime lookup, or identity persistence. Missing or
    // non-directory mapped targets fail here before any side effects.
    let provisioner = ctx.composition.worktree_provisioner(&git);
    let (scope, _provision_outcome) = run_after_open_scope_resolution(
        usecases::open::resolve_open_topology_scope(
            &worktrees,
            invocation_cwd.as_ref(),
            &root,
            &ws_id,
            open_intent,
            &provisioner,
            &ws_fs,
        ),
        Ok,
    )
    .map_err(|error| render_open_error(ctx, error))?;

    log_open_phase("topology_scope", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Build naming context from the resolved scope, not from cwd.
    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let branch = git.current_branch().ok();
    let instance_id = ws_fs.read_instance_id(root.as_ref());
    let naming_ctx = usecases::naming::assemble_naming_context(
        &name,
        &workspace_name,
        &root,
        branch,
        instance_id,
        scope.relative_project_path.clone(),
    );
    let server = policy.resolve_server(&naming_ctx)?;
    let session = policy.resolve_session(&naming_ctx)?;

    tracing::debug!(
        project = %naming_ctx.project,
        workspace = %naming_ctx.workspace,
        instance_id = ?naming_ctx.instance_id,
        server = %server,
        session = %session,
        "resolved naming for open"
    );

    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::OPEN_CREATING_SESSION, &[&session])
    );

    use workhive_core::ports::ProjectInitLockPort;
    let open_lock = Some(
        ctx.composition
            .project_init_lock()
            .acquire_init_lock(root.as_ref())?,
    );

    let open_mode = if force {
        usecases::open::OpenMode::ForceRecreateFromLayout
    } else {
        usecases::open::OpenMode::Normal
    };

    let real_instance_id = naming_ctx.instance_id.clone().ok_or_else(|| {
        CliError::Runtime(
            "instance_id must exist after project initialization — cannot build workspace identity"
                .to_owned(),
        )
    })?;

    let identity_adapter = ctx.composition.workspace_identity(root.as_ref());
    let canonical_project = SessionNamingPolicy::sanitize_component(&name);
    usecases::identity::ensure_project_identity(
        &identity_adapter,
        &real_instance_id,
        &root,
        &name,
        &canonical_project,
    )?;

    let subdir_key = scope.relative_project_path.clone();
    let relative_display = usecases::naming::resolve_relative_display_path(
        &name,
        subdir_key.as_ref(),
        &workspace_name,
    );
    let identity_key =
        workhive_core::domain::WorkspaceIdentityKey::new(&workspace_name, subdir_key.clone());
    let had_persisted_identity =
        workhive_core::ports::WorkspaceIdentityPort::load_workspace_identity(
            &identity_adapter,
            &identity_key,
        )?
        .is_some();
    let effective_backend = ctx.runtime_backend_for(Some(root.as_ref()))?;
    let runtime_for_new_workspace = || {
        let session = (effective_backend == workhive_core::domain::RuntimeBackendId::herdr())
            .then(|| ctx.herdr_session_for(Some(root.as_ref())))
            .transpose()?;
        ctx.composition
            .runtime_for_new_workspace(&effective_backend, session.as_deref())
    };
    let mut workspace_identity = usecases::identity::prepare_workspace_identity(
        &identity_adapter,
        &policy,
        &naming_ctx,
        effective_backend.clone(),
        subdir_key.clone(),
        relative_display.as_deref(),
    )?;

    // Backend replacement commits configuration before switching identities. If
    // that switch was interrupted, finish it; same-backend force rebuilds still
    // roll back their staged runtime.
    if let Some(workhive_core::domain::RuntimeReplacement::Preparing { target }) =
        workspace_identity.runtime_replacement.clone()
        && should_commit_prepared_backend_change(
            &workspace_identity.runtime_binding.backend,
            &target.backend,
            &effective_backend,
        )
    {
        usecases::identity::commit_runtime_replacement(&identity_adapter, &mut workspace_identity)?;
    }

    // Resume interrupted replacement cleanup before planning another open.
    let mut replacement_cleanup_pending = false;
    if let Some(replacement) = workspace_identity.runtime_replacement.clone() {
        match replacement {
            workhive_core::domain::RuntimeReplacement::Preparing { target } => {
                let launch_identity_store =
                    ctx.composition.pane_launch_identity_store(root.as_ref());
                let cleanup_runtime = ctx.composition.runtime_for_binding(&target)?;
                if let Err(error) = cleanup_replacement_resources(
                    cleanup_runtime.as_ref(),
                    &launch_identity_store,
                    &target,
                ) {
                    note_pending_replacement_cleanup(
                        ctx,
                        locale,
                        &target,
                        &error,
                        "staged replacement cleanup remains pending; continuing with active binding",
                        workhive_core::i18n_keys::OPEN_REPLACEMENT_CLEANUP_STAGED,
                    );
                    replacement_cleanup_pending = true;
                } else {
                    usecases::identity::finish_runtime_replacement(
                        &identity_adapter,
                        &mut workspace_identity,
                    )?;
                }
            }
            workhive_core::domain::RuntimeReplacement::Promoting {
                staged,
                final_binding,
                previous,
            } => {
                let launch_identity_store =
                    ctx.composition.pane_launch_identity_store(root.as_ref());
                let previous_runtime = ctx.composition.runtime_for_binding(&previous)?;
                if let Err(error) = cleanup_replacement_resources(
                    previous_runtime.as_ref(),
                    &launch_identity_store,
                    &previous,
                ) {
                    note_pending_replacement_cleanup(
                        ctx,
                        locale,
                        &previous,
                        &error,
                        "runtime promotion remains pending after previous cleanup failure",
                        workhive_core::i18n_keys::OPEN_REPLACEMENT_CLEANUP_PROMOTION,
                    );
                    replacement_cleanup_pending = true;
                } else {
                    let staged_runtime = ctx.composition.runtime_for_binding(&staged)?;
                    let staged_exists = staged_runtime.has_workspace_binding(&staged)?;
                    let final_exists = staged_runtime.has_workspace_binding(&final_binding)?;
                    match (staged_exists, final_exists) {
                        (true, false) => staged_runtime
                            .rename_workspace_binding(&staged, &final_binding)
                            .map_err(CliError::RuntimeOp)?,
                        (false, true) => {}
                        (true, true) => {
                            return Err(CliError::Runtime(format!(
                                "cannot resume runtime promotion: both staged '{}' and final '{}' exist",
                                staged.workspace_id, final_binding.workspace_id
                            )));
                        }
                        (false, false) => {
                            return Err(CliError::Runtime(format!(
                                "cannot resume runtime promotion: neither staged '{}' nor final '{}' exists",
                                staged.workspace_id, final_binding.workspace_id
                            )));
                        }
                    }
                    if let Err(error) = remap_launch_identities(
                        &launch_identity_store,
                        &staged.scope_id,
                        &staged.workspace_id,
                        &final_binding.workspace_id,
                    ) {
                        tracing::warn!(
                            staged = %staged.workspace_id,
                            final_name = %final_binding.workspace_id,
                            error = %error,
                            "runtime promotion completed but pane launch identity remap remains pending"
                        );
                    }
                    commit_promoted_runtime(&identity_adapter, &mut workspace_identity)?;
                }
            }
            workhive_core::domain::RuntimeReplacement::RegisteringCheckout { previous } => {
                let launch_identity_store =
                    ctx.composition.pane_launch_identity_store(root.as_ref());
                let previous_runtime = ctx.composition.runtime_for_binding(&previous)?;
                if let Err(error) = cleanup_replacement_resources(
                    previous_runtime.as_ref(),
                    &launch_identity_store,
                    &previous,
                ) {
                    note_pending_replacement_cleanup(
                        ctx,
                        locale,
                        &previous,
                        &error,
                        "checkout registration remains pending after previous cleanup failure",
                        workhive_core::i18n_keys::OPEN_REPLACEMENT_CLEANUP_CHECKOUT,
                    );
                    replacement_cleanup_pending = true;
                } else {
                    let active = workspace_identity.runtime_binding();
                    let active_runtime = ctx.composition.runtime_for_binding(&active)?;
                    register_runtime_checkout_exact(
                        active_runtime.as_ref(),
                        &active,
                        root.as_ref(),
                        &scope.selected_workspace_root,
                    )?;
                    usecases::identity::finish_runtime_checkout_registration(
                        &identity_adapter,
                        &mut workspace_identity,
                    )?;
                }
            }
            workhive_core::domain::RuntimeReplacement::Cleaning { previous } => {
                let launch_identity_store =
                    ctx.composition.pane_launch_identity_store(root.as_ref());
                let cleanup_runtime = ctx.composition.runtime_for_binding(&previous)?;
                if let Err(error) = cleanup_replacement_resources(
                    cleanup_runtime.as_ref(),
                    &launch_identity_store,
                    &previous,
                ) {
                    note_pending_replacement_cleanup(
                        ctx,
                        locale,
                        &previous,
                        &error,
                        "previous runtime cleanup remains pending; continuing with active binding",
                        workhive_core::i18n_keys::OPEN_REPLACEMENT_CLEANUP_PREVIOUS,
                    );
                    replacement_cleanup_pending = true;
                } else {
                    usecases::identity::finish_runtime_replacement(
                        &identity_adapter,
                        &mut workspace_identity,
                    )?;
                }
            }
        }
    }

    let active_runtime = if !had_persisted_identity {
        runtime_for_new_workspace()?
    } else {
        ctx.composition
            .runtime_for_binding(&workspace_identity.runtime_binding())?
    };

    if force && replacement_cleanup_pending {
        return Err(CliError::Runtime(
            "cannot start another force rebuild while previous runtime cleanup is pending"
                .to_owned(),
        ));
    }

    let active = workspace_identity.runtime_binding();
    let bound_exists = if had_persisted_identity {
        match active_runtime.has_workspace_binding(&active) {
            Ok(exists) => exists,
            Err(
                ref error @ workhive_core::ports::RuntimeError::BackendUnavailable {
                    ref reason, ..
                },
            ) if reason.is_runtime_absent() => {
                tracing::debug!(
                    reason = ?reason,
                    error = %error,
                    "active runtime is absent; open will create it"
                );
                false
            }
            Err(error) => return Err(error.into()),
        }
    } else {
        false
    };
    let existing_session = if bound_exists {
        Some(usecases::open::ExistingSession::Canonical {
            server: active.scope_id.clone(),
            session: active.workspace_id.clone(),
        })
    } else if had_persisted_identity {
        None
    } else {
        usecases::open::resolve_open_session(
            active_runtime.as_ref(),
            &server,
            &session,
            &name,
            &workspace_name,
        )?
    };
    let previous_force_runtime = if force {
        Some(if had_persisted_identity {
            Some(workspace_identity.runtime_binding())
        } else {
            match existing_session.as_ref() {
                Some(existing) => Some(require_runtime_binding(
                    ctx,
                    locale,
                    effective_backend.clone(),
                    existing.server().to_owned(),
                    existing.session().to_owned(),
                )?),
                None => None,
            }
        })
    } else {
        None
    };
    let has_previous_runtime = previous_force_runtime
        .as_ref()
        .and_then(Option::as_ref)
        .is_some();
    if force
        && !had_persisted_identity
        && let Some(previous) = previous_force_runtime.as_ref().and_then(Option::as_ref)
    {
        workspace_identity.runtime_binding = previous.clone();
    }
    let tmux_staged_replacement = force
        && has_previous_runtime
        && effective_backend == workhive_core::domain::RuntimeBackendId::tmux();
    let staged_session =
        staged_session_for_backend(&effective_backend, &session, has_previous_runtime);
    let (plan_server, plan_session) = match (open_mode, existing_session.as_ref()) {
        (usecases::open::OpenMode::Normal, Some(existing)) => {
            (existing.server(), existing.session())
        }
        (usecases::open::OpenMode::Normal, None) if had_persisted_identity => {
            (active.scope_id.as_str(), active.workspace_id.as_str())
        }
        _ => (
            server.as_str(),
            staged_session.as_deref().unwrap_or(session.as_str()),
        ),
    };

    let runtime = if force {
        runtime_for_new_workspace()?
    } else {
        active_runtime
    };
    let mut plan = usecases::open::prepare_open_plan_with_scope(
        runtime.as_ref(),
        plan_server,
        plan_session,
        root.as_ref(),
        &scope,
        &ws_id,
        open_mode,
    )
    .map_err(|e| render_open_error(ctx, e))?;
    plan.defer_checkout_registration = force
        && has_previous_runtime
        && effective_backend == workhive_core::domain::RuntimeBackendId::herdr();

    // Resolve effective `.env` (global ∪ project, project overrides global)
    // and stamp it onto the plan so `create_workspace` carries the env into
    // the runtime.
    //
    // Failures to load `.env` must not block `open`: a missing file is
    // treated as no env. Other I/O errors are logged at warn and we
    // continue with whatever was successfully resolved (fail-open posture
    // for the optional `.env` overlay; the config layering policy).
    let (effective_env, env_parse_issues) = resolve_open_effective_env(ctx, root.as_ref());
    plan.env = effective_env
        .entries
        .iter()
        .map(|e| (e.key.clone(), e.value.clone()))
        .collect();

    {
        plan.interaction_policy =
            crate::commands::resolve_interaction_policy(&ctx.config, Some(root.as_ref()))?;
    }

    // Surface invalid-line warnings BEFORE the injection summary so the
    // user reads "this line was skipped" before "these N keys were
    // injected".
    print_env_parse_warnings(ctx, locale, &env_parse_issues);
    print_env_injection_summary(ctx, locale, &effective_env);

    tracing::debug!(
        plan_session = %plan.session,
        plan_server = %plan.server,
        session_availability = %plan.session_availability,
        provenance = ?plan.provenance,
        env_count = plan.env.len(),
        "open plan prepared"
    );
    log_open_phase("plan_preparation", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    let canonical_tmux_binding = tmux_staged_replacement
        .then(|| workhive_core::domain::RuntimeBinding::tmux(server.clone(), session.clone()));
    let target_binding = require_runtime_binding(
        ctx,
        locale,
        effective_backend.clone(),
        plan.server.clone(),
        plan.session.clone(),
    )?;
    let launch_identity_store = ctx.composition.pane_launch_identity_store(root.as_ref());
    let mut force_rebuild = if force {
        let pending_persisted = has_previous_runtime;
        if pending_persisted {
            usecases::identity::prepare_runtime_replacement(
                &identity_adapter,
                &mut workspace_identity,
                target_binding.clone(),
            )?;
        }
        Some(ForceRebuildGuard {
            runtime: runtime.as_ref(),
            identity: &identity_adapter,
            launch_identity: &launch_identity_store,
            record: workspace_identity.clone(),
            target: target_binding.clone(),
            // The planned binding is not owned until the runtime executor
            // returns the authoritative prepared binding. If creation fails,
            // dropping the guard must never destroy a pre-existing target.
            target_owned: false,
            pending_persisted,
            armed: true,
        })
    } else {
        None
    };
    let result = usecases::open::execute_open_plan(runtime.as_ref(), &plan)?;
    if let Some(prepared_runtime) = &result.prepared_runtime {
        let prepared_binding = &prepared_runtime.binding;
        if let Some(guard) = force_rebuild.as_mut() {
            // Claim ownership before the next durable write. If that write
            // fails, Drop still has the authoritative binding to clean up.
            guard.target = prepared_binding.clone();
            guard.target_owned = matches!(
                prepared_runtime.ownership,
                usecases::open::PreparedRuntimeOwnership::CreatedByOperation
            );
        }
        if force && has_previous_runtime {
            usecases::identity::prepare_runtime_replacement(
                &identity_adapter,
                &mut workspace_identity,
                prepared_binding.clone(),
            )?;
            if let Some(guard) = force_rebuild.as_mut() {
                guard.pending_persisted = true;
            }
        } else {
            workspace_identity.runtime_binding = prepared_binding.clone();
            workspace_identity.canonical_server_name = prepared_binding.scope_id.clone();
            workspace_identity.canonical_session_name = prepared_binding.workspace_id.clone();
        }
        plan.server = prepared_binding.scope_id.clone();
        plan.session = prepared_binding.workspace_id.clone();
        if let Some(guard) = force_rebuild.as_mut()
            && !tmux_staged_replacement
        {
            guard.record = workspace_identity.clone();
        }
    }

    if !force {
        usecases::identity::commit_workspace_identity(&identity_adapter, &workspace_identity)?;
    }

    log_open_phase("tmux_session_create_or_reuse", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Persist the managed-worktree branch provenance once provisioning has
    // succeeded. Recorded only when this open created or explicitly
    // attached the backing branch; the reuse path leaves existing stored
    // provenance intact. A persistence failure is non-fatal: removal
    // defaults to the conservative "keep the branch" behavior when
    // provenance is missing.
    if let Some(provenance) = provenance_to_record {
        use workhive_core::ports::WorktreeProvenancePort;
        let provenance_store = ctx.composition.worktree_provenance(root.as_ref());
        if let Err(e) = provenance_store.record(&workspace_name, provenance) {
            tracing::warn!(
                workspace = %workspace_name,
                error = %e,
                "failed to record worktree branch provenance"
            );
        }
    }

    // Display the runtime path taken during open.
    let _ = &result;

    // Display reattach outcome — truthful recoverable/unrecoverable/manual messaging.
    {
        use workhive_core::domain::{ReattachOutcome, SessionRecoveryAction};
        let action = result.reattach_outcome.recovery_action();
        match &action {
            SessionRecoveryAction::None => {}
            SessionRecoveryAction::Recoverable { description } => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve_with_args(
                        locale,
                        k::RECOVERY_ACTION_RECOVERABLE,
                        &[description]
                    )
                );
            }
            SessionRecoveryAction::ManualIntervention { reason } => {
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::RECOVERY_ACTION_MANUAL, &[reason])
                );
            }
        }
        if let ReattachOutcome::Unrecoverable { reason } = &result.reattach_outcome {
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve_with_args(locale, k::REATTACH_UNRECOVERABLE, &[reason])
            );
        }
    }
    log_open_phase("runtime_layout_apply", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Reused live sessions stay untouched. Titles are populated by the
    // effective layout only; there is no persisted runtime state to load.
    let layout_application = if !result.outcome.is_session_reused() {
        apply_layout_and_register(ctx, runtime.as_ref(), &plan, root.as_ref())?
    } else {
        LayoutApplication::default()
    };
    let layout_pane_count = layout_application.layout_pane_count;
    let degraded_launches = layout_application.degraded_launches;
    let managed_pane_titles = layout_application.managed_pane_titles;

    // Always validate pane CWDs, even for reused sessions.
    // This ensures that panes with wrong CWD are fixed regardless of whether
    // layout was applied. Only clean panes (no running tools) are respawned.
    let cwds_fixed = validate_and_fix_pane_cwds(
        runtime.as_ref(),
        &plan.server,
        &plan.session,
        plan.working_dir.as_path(),
    )?;
    if cwds_fixed > 0 {
        tracing::info!(
            session = %plan.session,
            panes_respawned = cwds_fixed,
            "validated and fixed pane CWDs after open"
        );
    }

    if force {
        let expected = layout_pane_count.ok_or_else(|| {
            CliError::Runtime(
                "force rebuild target validation failed: layout pane count unavailable".to_owned(),
            )
        })? as usize;
        let units = runtime.list_units(&plan.server, &plan.session)?;
        if units.len() != expected
            || (effective_backend == workhive_core::domain::RuntimeBackendId::tmux()
                && units.iter().any(|unit| unit.command.trim().is_empty()))
            || !degraded_launches.is_empty()
        {
            let degraded = if degraded_launches.is_empty() {
                "none".to_owned()
            } else {
                degraded_launches.join("; ")
            };
            return Err(CliError::Runtime(format!(
                "force rebuild target validation failed: expected {expected} panes, found {}; degraded launches: {degraded}",
                units.len()
            )));
        }
    }

    // Runtime sync: reconcile baseline-registered panes with actual runtime state.
    sync_pane_registry_after_open(ctx, runtime.as_ref(), &plan);

    log_open_phase("runtime_sync_baseline_persistence", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    eprintln!(
        "{}",
        render_interaction_mode_message(ctx, interaction_mode, &ws_id)
    );

    // Bounded pane-readiness wait before attach. Skipped for reused live
    // sessions (no fresh pane lifecycle) and for `--no-attach` runs (no
    // attach to gate). The wait is hard-bounded by
    // `open_readiness::DEFAULT_BUDGET_MS`; on timeout the user sees a
    // degraded warning and `open` continues, never hanging.
    //
    // Under `--no-attach` we still emit a deterministic progress line so
    // the user always sees the same lifecycle summary regardless of
    // attach mode (review item: deterministic --no-attach progress).
    if no_attach {
        eprintln!(
            "{}",
            ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_PROGRESS_NO_ATTACH_SUMMARY,
                &[&plan.session]
            )
        );
    }
    if !no_attach && !result.outcome.is_session_reused() {
        let expected_panes: usize = layout_pane_count.map(|n| n as usize).unwrap_or(1);
        if expected_panes > 0 {
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::OPEN_PROGRESS_WAITING_PANES)
            );
            let wait_outcome = open_readiness::wait_for_panes_ready(
                runtime.as_ref(),
                &plan.server,
                &plan.session,
                expected_panes,
                open_readiness::DEFAULT_BUDGET_MS,
                open_readiness::DEFAULT_POLL_INTERVAL_MS,
                &open_readiness::ThreadSleeper,
                std::time::Instant::now(),
            );
            if let open_readiness::ReadinessOutcome::Failed { error } = wait_outcome {
                return Err(CliError::RuntimeOp(error));
            }
            if matches!(
                wait_outcome,
                open_readiness::ReadinessOutcome::TimedOut { .. }
                    | open_readiness::ReadinessOutcome::Degraded { .. }
            ) {
                tracing::warn!(
                    workspace = %plan.session,
                    outcome = ?wait_outcome,
                    "pane readiness wait did not converge before attach"
                );
            }
            if force && !open_readiness::is_force_acceptable(&wait_outcome) {
                return Err(CliError::Runtime(format!(
                    "force rebuild target readiness failed: {wait_outcome:?}"
                )));
            }
            if let Some(line) = open_readiness::render_outcome(&wait_outcome, &ctx.i18n, locale) {
                eprintln!("{line}");
            }
        }
    }
    if force {
        let units = runtime.list_units(&plan.server, &plan.session)?;
        if units.len() != layout_pane_count.unwrap_or(1) as usize
            || (effective_backend == workhive_core::domain::RuntimeBackendId::tmux()
                && units.iter().any(|unit| unit.command.trim().is_empty()))
        {
            return Err(CliError::Runtime(
                "force rebuild target validation failed: a pane launch command was not observed"
                    .to_owned(),
            ));
        }
        if previous_force_runtime
            .as_ref()
            .and_then(Option::as_ref)
            .is_some()
        {
            if plan.defer_checkout_registration {
                usecases::identity::commit_runtime_checkout_registration(
                    &identity_adapter,
                    &mut workspace_identity,
                )?;
            } else if tmux_staged_replacement {
                usecases::identity::commit_runtime_promotion(
                    &identity_adapter,
                    &mut workspace_identity,
                    require_canonical_tmux_binding(ctx, locale, canonical_tmux_binding.as_ref())?
                        .clone(),
                )?;
            } else {
                usecases::identity::commit_runtime_replacement(
                    &identity_adapter,
                    &mut workspace_identity,
                )?;
            }
        } else {
            usecases::identity::commit_workspace_identity(&identity_adapter, &workspace_identity)?;
        }
        // Validation has established a usable target and its binding is now
        // durable. From this point onward interrupted cleanup must resume from
        // the replacement state instead of deleting the validated target.
        if let Some(guard) = force_rebuild.as_mut() {
            guard.disarm();
        }
        if let Some(previous) = previous_force_runtime.as_ref().and_then(Option::as_ref) {
            let previous_runtime = ctx.composition.runtime_for_binding(previous)?;
            if let Err(error) = cleanup_replacement_resources(
                previous_runtime.as_ref(),
                &launch_identity_store,
                previous,
            ) {
                if previous.backend == workhive_core::domain::RuntimeBackendId::herdr()
                    && matches!(
                        &error,
                        CliError::RuntimeOp(
                            workhive_core::ports::RuntimeError::WorkspaceGroupCloseRequired { .. }
                        )
                    )
                {
                    let staged = workspace_identity.runtime_binding();
                    usecases::identity::rollback_runtime_replacement(
                        &identity_adapter,
                        &mut workspace_identity,
                    )?;
                    let target_owned = if let Some(guard) = force_rebuild.as_mut() {
                        guard.record = workspace_identity.clone();
                        guard.armed = true;
                        guard.target_owned
                    } else {
                        false
                    };
                    if target_owned {
                        cleanup_replacement_resources(
                            runtime.as_ref(),
                            &launch_identity_store,
                            &staged,
                        )?;
                    }
                    if let Some(guard) = force_rebuild.as_mut() {
                        guard.disarm();
                    }
                    drop(open_lock);
                    return handle_herdr_force_close_refusal(
                        ctx,
                        locale,
                        previous_runtime.as_ref(),
                        previous,
                        &workspace_name,
                        no_attach,
                        plan.interaction_policy,
                    );
                }
                return Err(error);
            }

            if plan.defer_checkout_registration {
                let active = workspace_identity.runtime_binding();
                register_runtime_checkout_exact(
                    runtime.as_ref(),
                    &active,
                    &plan.project_root,
                    &plan.workspace_root,
                )?;
            }

            if tmux_staged_replacement {
                let final_binding =
                    require_canonical_tmux_binding(ctx, locale, canonical_tmux_binding.as_ref())?;
                let staged = workspace_identity.runtime_binding();
                runtime
                    .rename_workspace_binding(&staged, final_binding)
                    .map_err(CliError::RuntimeOp)?;
                remap_launch_identities(
                    &launch_identity_store,
                    &staged.scope_id,
                    &staged.workspace_id,
                    &final_binding.workspace_id,
                )
                .map_err(|error| {
                    CliError::Runtime(format!(
                        "failed to promote pane launch identity for '{}': {error}",
                        staged.workspace_id
                    ))
                })?;
                commit_promoted_runtime(&identity_adapter, &mut workspace_identity)?;
                plan.server = final_binding.scope_id.clone();
                plan.session = final_binding.workspace_id.clone();
            } else if plan.defer_checkout_registration {
                usecases::identity::finish_runtime_checkout_registration(
                    &identity_adapter,
                    &mut workspace_identity,
                )?;
            } else {
                usecases::identity::finish_runtime_replacement(
                    &identity_adapter,
                    &mut workspace_identity,
                )?;
            }
        }
    }

    // The supervisor starts only after a staged tmux runtime has been promoted
    // to its final name, so its session-liveness probe never observes a stale
    // runtime binding.
    let supervisor_degraded_reason =
        ensure_control_plane_supervisor(ctx, root.as_ref(), &plan.server, &plan.session);
    log_open_phase("control_plane_probe_spawn", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Do not hold the project lock while attach blocks in tmux.
    drop(open_lock);

    log_open_phase("pane_readiness_wait", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Pane title reassertion — defends against runtime tools (e.g. Codex)
    // that overwrite the terminal title between layout application and
    // user attach. Order is fixed: layout -> readiness wait -> reassert ->
    // attach. We deliberately skip reassertion for live sessions Workhive
    // authoritative managed-pane metadata for those sessions and a
    // blind rewrite could clobber a meaningful state the user already
    // sees.
    if !result.outcome.is_session_reused() {
        reassert_managed_pane_titles(runtime.as_ref(), &plan.server, &managed_pane_titles);
    }

    // Bounded post-attach pane-title guard.
    //
    // Pre-attach reassertion alone is not enough: interactive tools
    // (Codex, OpenCode, …) often write terminal-title escape sequences
    // shortly *after* they boot, leaving the user looking at a polluted
    // `#{pane_title}` (frequently the project name). We spawn a
    // detached child that reasserts the launch-identity title every
    // ~500ms for a bounded ~8s window, then exits on its own. Skipped
    // for reused live sessions because we lack authoritative managed-pane
    // metadata for those.
    if !managed_pane_titles.is_empty() && !result.outcome.is_session_reused() {
        let binding = workspace_identity.runtime_binding();
        if let Err(e) = ctx.composition.spawn_pane_title_guard_process(
            &binding,
            &managed_pane_titles,
            500,
            8_000,
        ) {
            tracing::warn!(
                error = %e,
                count = managed_pane_titles.len(),
                server = %plan.server,
                session = %plan.session,
                "failed to spawn post-attach pane-title guard; pre-attach reassert still applied"
            );
        } else {
            tracing::debug!(
                count = managed_pane_titles.len(),
                server = %plan.server,
                session = %plan.session,
                "spawned bounded post-attach pane-title guard"
            );
        }
    }

    // Mark last-used once preparation succeeds, before any blocking
    // attach: the attach call can keep the process inside tmux for
    // hours, and usage metadata must reflect when the workspace was
    // opened, not when the user eventually detaches. Under
    // `--no-attach` the usage write still runs because the workspace
    // runtime was intentionally opened/refreshed.
    let env = ctx.composition.environment();
    let exec_ctx = env.execution_context()?;

    let active_binding = workspace_identity.runtime_binding();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::LIST_BACKEND, &[active_binding.backend.as_str()])
    );
    crate::commands::attach_usage::mark_used_then_maybe_attach_binding(
        runtime.as_ref(),
        &active_binding,
        workhive_core::ports::AttachWorkspaceRequest {
            group: plan.server.clone(),
            workspace_name: plan.session.clone(),
            interaction_policy: plan.interaction_policy,
        },
        !no_attach,
        &exec_ctx,
        crate::commands::attach_usage::AttachLifecycleCallbacks {
            mark_used: || {
                crate::commands::list::mark_workspace_used_now(&root, &workspace_name, ctx)
            },
            emit_ready_output: || {
                // Success + summary output BEFORE attach (or if --no-attach).
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::OPEN_SUCCESS, &[&plan.session])
                );
                print_open_summary(
                    ctx,
                    locale,
                    was_auto_initialized,
                    &result.provenance,
                    &result.outcome,
                    &plan.session,
                    &degraded_launches,
                    no_attach,
                );
            },
            emit_guidance: || crate::commands::session::print_attach_guidance(ctx),
            emit_already_current: || crate::commands::session::print_already_current(ctx),
        },
    )
    .map_err(|e| crate::commands::attach_usage::map_navigation_error(e, &ctx.i18n, locale))?;
    if !no_attach && result.outcome.is_session_reused() {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::OPEN_REUSING_SESSION, &[&plan.session])
        );
    }
    log_open_phase("attach", __open_phase_started_at);
    if let Some(reason) = &supervisor_degraded_reason {
        eprintln!(
            "{}",
            ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_SUMMARY_SUPERVISOR_DEGRADED,
                &[reason.as_str()],
            )
        );
    }

    let total_elapsed_ms =
        u64::try_from(__open_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::debug!(
        target: "workhive.open",
        step = "open.complete",
        elapsed_ms = total_elapsed_ms,
        no_attach,
        session = %plan.session,
        "workspace open completed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cleanup_error_is_already_done, run_after_open_scope_resolution,
        should_commit_prepared_backend_change, staged_runtime_name, staged_session_for_backend,
    };
    use camino::Utf8PathBuf;
    use workhive_core::domain::RuntimeBackendId;
    use workhive_core::usecases::naming::{resolve_relative_display_path, resolve_subdir_key};

    #[test]
    fn cleanup_treats_missing_runtime_as_already_done() {
        use workhive_core::ports::{BackendUnavailableReason, RuntimeError};

        let absent = [
            RuntimeError::WorkspaceNotFound {
                workspace: "w1".to_owned(),
            },
            RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::ServerNotRunning {
                    server: "old.sock".to_owned(),
                },
                detail: "stopped".to_owned(),
            },
            RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::SocketNotFound {
                    path: "old.sock".to_owned(),
                },
                detail: "missing".to_owned(),
            },
            RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::BackendBinaryNotFound {
                    binary: "herdr".to_owned(),
                },
                detail: "missing".to_owned(),
            },
        ];
        assert!(absent.iter().all(cleanup_error_is_already_done));
        assert!(!cleanup_error_is_already_done(
            &RuntimeError::OperationFailed {
                message: "permission denied".to_owned(),
            }
        ));
    }
    #[test]
    fn staged_runtime_name_keeps_canonical_at_delimiters() {
        let canonical = "wh@3922@demo@main";
        assert!(staged_runtime_name(canonical).starts_with("wh@3922@demo@main--rebuild-"));

        // Historic interrupted rebuild IDs must not keep growing on retry.
        let stale_staged = "wh@3922@demo@main--rebuild-1-deadbeef--rebuild-2-cafe";
        let next = staged_runtime_name(stale_staged);
        assert!(next.starts_with("wh@3922@demo@main--rebuild-"));
        assert_eq!(next.matches("--rebuild-").count(), 1);
    }

    #[test]
    fn force_rebuild_stages_only_tmux_runtime_names() {
        let canonical = "wh@3922@demo@main";
        assert!(
            staged_session_for_backend(&RuntimeBackendId::tmux(), canonical, true)
                .expect("tmux replacement should have a staging name")
                .starts_with("wh@3922@demo@main--rebuild-")
        );
        assert_eq!(
            staged_session_for_backend(&RuntimeBackendId::herdr(), canonical, true),
            None,
            "Herdr uses its canonical label and returns an opaque workspace ID"
        );
    }
    #[test]
    fn interrupted_backend_change_follows_committed_configuration() {
        assert!(should_commit_prepared_backend_change(
            &RuntimeBackendId::tmux(),
            &RuntimeBackendId::herdr(),
            &RuntimeBackendId::herdr(),
        ));
        assert!(!should_commit_prepared_backend_change(
            &RuntimeBackendId::tmux(),
            &RuntimeBackendId::herdr(),
            &RuntimeBackendId::tmux(),
        ));
        assert!(!should_commit_prepared_backend_change(
            &RuntimeBackendId::tmux(),
            &RuntimeBackendId::tmux(),
            &RuntimeBackendId::tmux(),
        ));
    }

    #[test]
    fn open_project_root_provenance_display_path_from_provenance() {
        for ws in ["main", "develop", "feature/api/login", "hotfix/v2.1"] {
            assert_eq!(
                resolve_relative_display_path("myrepo", None, ws),
                None,
                "project-root workspace '{ws}' must have display_path=None"
            );
        }
    }

    // ── open_scope_context_ — production topology/scope resolver ──

    #[test]
    fn open_scope_context_root_cwd_produces_no_rpp() {
        let root = Utf8PathBuf::from("/proj");
        let cwd = Utf8PathBuf::from("/proj");
        let rpp = resolve_subdir_key(&root, Some(cwd.as_ref()));
        assert!(rpp.is_none(), "project root cwd must produce no rpp");
    }

    #[test]
    fn open_scope_context_subdir_cwd_produces_rpp() {
        let root = Utf8PathBuf::from("/proj");
        let cwd = Utf8PathBuf::from("/proj/services/api");
        let rpp = resolve_subdir_key(&root, Some(cwd.as_ref()));
        assert_eq!(
            rpp.as_ref().map(|p| p.as_str()),
            Some("services/api"),
            "subdir cwd must produce matching rpp"
        );
    }

    #[test]
    fn open_scope_context_managed_worktree_root_produces_no_rpp() {
        let root = Utf8PathBuf::from("/proj");
        let cwd = Utf8PathBuf::from("/proj/.workhive/worktrees/feat-x");
        let rpp = resolve_subdir_key(&root, Some(cwd.as_ref()));
        assert!(rpp.is_none(), "managed worktree root must produce no rpp");
    }

    #[test]
    fn open_scope_context_managed_worktree_subdir_preserves_rpp() {
        let root = Utf8PathBuf::from("/proj");
        let cwd = Utf8PathBuf::from("/proj/.workhive/worktrees/feat-x/services/api");
        let rpp = resolve_subdir_key(&root, Some(cwd.as_ref()));
        assert_eq!(
            rpp.as_ref().map(|p| p.as_str()),
            Some("services/api"),
            "managed worktree subdirectory must be preserved"
        );
    }

    // ── open_scope_flow_ — topology-first production resolver ──

    use workhive_core::domain::WorkspaceProvenance;
    use workhive_core::ports::{DirectoryClassification, WorktreeInfo};
    use workhive_core::usecases::open::{OpenError, resolve_open_scope};

    struct ClassifyFs {
        classified: std::collections::HashMap<String, DirectoryClassification>,
    }

    impl ClassifyFs {
        fn directory_at(path: &str) -> Self {
            let mut map = std::collections::HashMap::new();
            map.insert(path.to_owned(), DirectoryClassification::Directory);
            Self { classified: map }
        }

        fn missing_at(path: &str) -> Self {
            let mut map = std::collections::HashMap::new();
            map.insert(path.to_owned(), DirectoryClassification::Missing);
            Self { classified: map }
        }
    }

    impl workhive_core::ports::WorkspaceFsPort for ClassifyFs {
        fn project_root_exists(&self, _: &camino::Utf8Path) -> bool {
            true
        }
        fn create_workhive_dir(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            unreachable!()
        }
        fn remove_workhive_dir(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            unreachable!()
        }
        fn workhive_dir_exists(&self, _: &camino::Utf8Path) -> bool {
            true
        }
        fn workhive_dir_path(&self, p: &camino::Utf8Path) -> Utf8PathBuf {
            p.join(".workhive")
        }
        fn list_workspace_dirs(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<Vec<String>, workhive_core::errors::WorkspaceFsError> {
            Ok(vec![])
        }
        fn read_instance_id(&self, _: &camino::Utf8Path) -> Option<String> {
            None
        }
        fn write_instance_id(
            &self,
            _: &camino::Utf8Path,
            _: &str,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            unreachable!()
        }
        fn project_config_exists(&self, _: &camino::Utf8Path) -> bool {
            true
        }
        fn worktree_dir_exists(&self, _: &camino::Utf8Path, _: &str) -> bool {
            false
        }
        fn workspace_last_used_epoch_secs(&self, _: &camino::Utf8Path, _: &str) -> Option<u64> {
            None
        }
        fn mark_workspace_used_epoch_secs(
            &self,
            _: &camino::Utf8Path,
            _: &str,
            _: u64,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            Ok(())
        }
        fn commit_staged_workhive(
            &self,
            _: &camino::Utf8Path,
            _: &camino::Utf8Path,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            unreachable!()
        }
        fn cleanup_staging(&self, _: &camino::Utf8Path) {}
        fn ensure_gitignore_workhive_entry(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
            unreachable!()
        }

        fn classify_path(
            &self,
            path: &camino::Utf8Path,
        ) -> Result<DirectoryClassification, workhive_core::errors::WorkspaceFsError> {
            Ok(self
                .classified
                .get(path.as_str())
                .cloned()
                .unwrap_or(DirectoryClassification::Directory))
        }
    }

    #[test]
    fn open_scope_flow_maps_main_subdir_to_provisioned_target() {
        let worktrees = vec![WorktreeInfo {
            path: Utf8PathBuf::from("/repo"),
            branch: Some("main".to_owned()),
            is_main: true,
            resolved_strategy: None,
        }];
        let invocation_cwd = Utf8PathBuf::from("/repo/services/api");
        let selected_root = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-x");
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-x/services/api");

        let scope = resolve_open_scope(
            &worktrees,
            invocation_cwd.as_ref(),
            &selected_root,
            WorkspaceProvenance::Provisioned {
                provider: workhive_core::domain::WorkspaceProvider::Git,
                materialization: workhive_core::domain::MaterializationKind::FromNewReference,
            },
            &fs,
        )
        .expect("scope resolution must succeed");

        assert_eq!(
            scope.working_dir,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-x/services/api"),
            "main-root subdirectory must map into provisioned target"
        );
        assert_eq!(
            scope.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("services/api"),
        );
    }

    #[test]
    fn open_scope_flow_maps_managed_worktree_subdir_to_other_target() {
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
                branch: Some("feature-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];
        let invocation_cwd = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a/services/api");
        let selected_root = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-b");
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-b/services/api");

        let scope = resolve_open_scope(
            &worktrees,
            invocation_cwd.as_ref(),
            &selected_root,
            WorkspaceProvenance::Provisioned {
                provider: workhive_core::domain::WorkspaceProvider::Git,
                materialization: workhive_core::domain::MaterializationKind::FromNewReference,
            },
            &fs,
        )
        .expect("scope resolution must succeed");

        assert_eq!(
            scope.working_dir,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-b/services/api"),
        );
        assert_eq!(
            scope.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("services/api"),
        );
    }

    #[test]
    fn open_scope_flow_rejects_missing_mapped_directory_before_runtime_identity_writes() {
        let worktrees = vec![WorktreeInfo {
            path: Utf8PathBuf::from("/repo"),
            branch: Some("main".to_owned()),
            is_main: true,
            resolved_strategy: None,
        }];
        let invocation_cwd = Utf8PathBuf::from("/repo/services/api");
        let selected_root = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-x");
        let fs = ClassifyFs::missing_at("/repo/.workhive/worktrees/feature-x/services/api");

        let err = resolve_open_scope(
            &worktrees,
            invocation_cwd.as_ref(),
            &selected_root,
            WorkspaceProvenance::Provisioned {
                provider: workhive_core::domain::WorkspaceProvider::Git,
                materialization: workhive_core::domain::MaterializationKind::FromNewReference,
            },
            &fs,
        )
        .unwrap_err();

        assert!(
            matches!(err, OpenError::MappedTargetMissing { .. }),
            "missing mapped directory must fail with typed error, got: {err:?}"
        );
    }

    #[test]
    fn open_scope_flow_current_linked_workspace_reuses_containing_root() {
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
                branch: Some("feature-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];
        let invocation_cwd = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a");
        let selected_root = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a");
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-a");

        let scope = resolve_open_scope(
            &worktrees,
            invocation_cwd.as_ref(),
            &selected_root,
            WorkspaceProvenance::Provisioned {
                provider: workhive_core::domain::WorkspaceProvider::Git,
                materialization: workhive_core::domain::MaterializationKind::ReusedExisting,
            },
            &fs,
        )
        .expect("reuse of containing linked worktree must succeed");

        assert_eq!(
            scope.selected_workspace_root,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
            "selected root must be the existing linked worktree"
        );
        assert!(
            scope.relative_project_path.is_none(),
            "worktree root invocation must produce no relative path"
        );
        assert!(
            matches!(
                scope.provenance,
                WorkspaceProvenance::Provisioned {
                    materialization: workhive_core::domain::MaterializationKind::ReusedExisting,
                    ..
                }
            ),
            "provenance must indicate ReusedExisting"
        );
    }

    // ── open_production_flow_ — tests that exercise the same production orchestration as cmd_open ──

    use std::cell::{Cell, RefCell};
    use workhive_core::ports::{
        ProvisionOutcome, ProvisionWorkspaceRequest, WorkspaceProvisionPort, WorkspaceRootInfo,
        WorktreeRemovePolicy,
    };

    struct FakeProvisioner {
        result_path: Utf8PathBuf,
        outcome: ProvisionOutcome,
    }

    impl FakeProvisioner {
        fn new(path: &str, outcome: ProvisionOutcome) -> Self {
            Self {
                result_path: Utf8PathBuf::from(path),
                outcome,
            }
        }
    }

    impl WorkspaceProvisionPort for FakeProvisioner {
        fn ensure_workspace_root(
            &self,
            _req: ProvisionWorkspaceRequest,
        ) -> Result<WorkspaceRootInfo, workhive_core::ports::ProvisionError> {
            Ok(WorkspaceRootInfo {
                path: self.result_path.clone(),
                outcome: self.outcome,
            })
        }

        fn remove_workspace_root(
            &self,
            _path: &camino::Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), workhive_core::ports::ProvisionError> {
            unreachable!()
        }
    }

    #[test]
    fn open_production_flow_maps_main_subdir_before_runtime_lookup() {
        let worktrees = vec![WorktreeInfo {
            path: Utf8PathBuf::from("/repo"),
            branch: Some("main".to_owned()),
            is_main: true,
            resolved_strategy: None,
        }];
        let provisioner = FakeProvisioner::new(
            "/repo/.workhive/worktrees/feature-x",
            ProvisionOutcome::MaterializedFromNewReference,
        );
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-x/services/api");
        let ws_id = workhive_core::domain::WorkspaceId::new("feature-x").unwrap();

        let (scope, _) = workhive_core::usecases::open::resolve_open_topology_scope(
            &worktrees,
            camino::Utf8Path::new("/repo/services/api"),
            &Utf8PathBuf::from("/repo"),
            &ws_id,
            workhive_core::domain::WorkspaceOpenIntent::Provision,
            &provisioner,
            &fs,
        )
        .expect("scope resolution must succeed");

        assert_eq!(
            scope.working_dir,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-x/services/api"),
            "main-root subdirectory must map into provisioned target"
        );
        assert_eq!(
            scope.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("services/api"),
        );
    }

    #[test]
    fn open_production_flow_maps_managed_worktree_subdir() {
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
                branch: Some("feature-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];
        let provisioner = FakeProvisioner::new(
            "/repo/.workhive/worktrees/feature-b",
            ProvisionOutcome::MaterializedFromNewReference,
        );
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-b/services/api");
        let ws_id = workhive_core::domain::WorkspaceId::new("feature-b").unwrap();

        let (scope, _) = workhive_core::usecases::open::resolve_open_topology_scope(
            &worktrees,
            camino::Utf8Path::new("/repo/.workhive/worktrees/feature-a/services/api"),
            &Utf8PathBuf::from("/repo"),
            &ws_id,
            workhive_core::domain::WorkspaceOpenIntent::Provision,
            &provisioner,
            &fs,
        )
        .expect("scope resolution must succeed");

        assert_eq!(
            scope.working_dir,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-b/services/api"),
        );
        assert_eq!(
            scope.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("services/api"),
        );
    }

    #[test]
    fn open_production_flow_rejects_missing_target_before_side_effects() {
        let worktrees = vec![WorktreeInfo {
            path: Utf8PathBuf::from("/repo"),
            branch: Some("main".to_owned()),
            is_main: true,
            resolved_strategy: None,
        }];
        let provisioner = FakeProvisioner::new(
            "/repo/.workhive/worktrees/feature-x",
            ProvisionOutcome::MaterializedFromNewReference,
        );
        let fs = ClassifyFs::missing_at("/repo/.workhive/worktrees/feature-x/services/api");
        let ws_id = workhive_core::domain::WorkspaceId::new("feature-x").unwrap();

        let runtime_lookup_count = Cell::new(0usize);
        let identity_save_count = Cell::new(0usize);
        let event_order = RefCell::new(Vec::new());

        let result = run_after_open_scope_resolution(
            workhive_core::usecases::open::resolve_open_topology_scope(
                &worktrees,
                camino::Utf8Path::new("/repo/services/api"),
                &Utf8PathBuf::from("/repo"),
                &ws_id,
                workhive_core::domain::WorkspaceOpenIntent::Provision,
                &provisioner,
                &fs,
            ),
            |resolved| {
                event_order.borrow_mut().push("runtime_lookup");
                runtime_lookup_count.set(runtime_lookup_count.get() + 1);
                event_order.borrow_mut().push("identity_save");
                identity_save_count.set(identity_save_count.get() + 1);
                Ok(resolved)
            },
        );

        assert!(
            matches!(
                result,
                Err(workhive_core::usecases::open::OpenError::MappedTargetMissing { .. })
            ),
            "missing mapped directory must fail with typed error, got: {result:?}"
        );

        assert_eq!(
            runtime_lookup_count.get(),
            0,
            "runtime lookup must not happen before scope validation"
        );
        assert_eq!(
            identity_save_count.get(),
            0,
            "identity writes must not happen before scope validation"
        );
        assert!(
            event_order.borrow().is_empty(),
            "no downstream event may run after scope validation fails"
        );
    }

    #[test]
    fn open_production_flow_reuses_containing_linked_worktree() {
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
                branch: Some("feature-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];
        let provisioner = FakeProvisioner::new(
            "/repo/.workhive/worktrees/feature-a",
            ProvisionOutcome::ReusedExistingRoot,
        );
        let fs = ClassifyFs::directory_at("/repo/.workhive/worktrees/feature-a");
        let ws_id = workhive_core::domain::WorkspaceId::new("feature-a").unwrap();

        let (scope, outcome) = workhive_core::usecases::open::resolve_open_topology_scope(
            &worktrees,
            camino::Utf8Path::new("/repo/.workhive/worktrees/feature-a"),
            &Utf8PathBuf::from("/repo"),
            &ws_id,
            workhive_core::domain::WorkspaceOpenIntent::Provision,
            &provisioner,
            &fs,
        )
        .expect("reuse of containing linked worktree must succeed");

        assert_eq!(
            scope.selected_workspace_root,
            Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
        );
        assert!(
            scope.relative_project_path.is_none(),
            "worktree root invocation must produce no relative path"
        );
        assert_eq!(
            outcome,
            Some(ProvisionOutcome::ReusedExistingRoot),
            "provision outcome must indicate reuse"
        );
    }
}

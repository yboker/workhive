#[allow(unused_imports)]
use super::*;
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

    // Trusted-root admission gate — must run BEFORE every mutating
    // open path, regardless of init state or open intent. An
    // already-initialized project outside trusted roots is NOT
    // grandfathered: it must still be rejected before the CLI creates
    // tmux sessions, runtime state, workspace identity, snapshots, or
    // worktrees.
    crate::commands::project::enforce_trusted_root_or_error(root.as_ref(), ctx)?;

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

    if let Some(ws) = workspace {
        WorkspaceId::new(ws)?;
    }

    let (workspace_name, open_intent) = match workspace {
        Some(ws) => {
            let main_branch = git.current_branch()?;
            let intent = if ws == main_branch {
                workhive_core::domain::WorkspaceOpenIntent::UseProjectRoot
            } else {
                workhive_core::domain::WorkspaceOpenIntent::Provision
            };
            (ws.to_owned(), intent)
        }
        None => {
            let (ws_name, intent) = select_workspace_interactive(&git, ctx)?;
            WorkspaceId::new(&ws_name)?;
            (ws_name, intent)
        }
    };
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

    let ws_id = WorkspaceId::new(&workspace_name)?;

    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context_with_cwd(
        &name,
        &workspace_name,
        &root,
        &git,
        &ws_fs,
        Some(invocation_cwd.as_path()),
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

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);
    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());

    let open_mode = if force {
        usecases::open::OpenMode::ForceRecreateFromLayout
    } else {
        usecases::open::OpenMode::Normal
    };

    // Dual-path lookup: canonical naming first, then legacy naming.
    let existing_session =
        usecases::open::resolve_open_session(&runtime, &server, &session, &name, &workspace_name)?;
    if open_mode == usecases::open::OpenMode::ForceRecreateFromLayout
        && let Some(existing) = existing_session.as_ref()
        && existing.is_legacy()
    {
        tracing::info!(
            legacy_server = %existing.server(),
            legacy_session = %existing.session(),
            canonical_server = %server,
            canonical_session = %session,
            "open -f: destroying legacy session before recreating canonical session"
        );
        runtime.destroy_workspace(existing.server(), existing.session())?;
    }

    let (plan_server, plan_session) = match (open_mode, existing_session.as_ref()) {
        (usecases::open::OpenMode::Normal, Some(existing)) => {
            (existing.server(), existing.session())
        }
        _ => (server.as_str(), session.as_str()),
    };

    let provisioner = ctx.composition.worktree_provisioner(&git);

    // Read snapshot.enabled from effective config (global + project merge).
    // Config errors are propagated, not coerced to false.
    let snapshot_enabled = resolve_effective_snapshot_enabled(ctx, root.as_ref())?;

    tracing::debug!(
        snapshot_enabled,
        ?open_mode,
        "open: resolved snapshot/mode flags"
    );

    let real_instance_id = naming_ctx.instance_id.clone().ok_or_else(|| {
        CliError::Runtime(
            "instance_id must exist after project initialization — cannot build snapshot identity"
                .to_owned(),
        )
    })?;

    // Ensure project + workspace identity metadata is created/consistent.
    // canonical_project_component must derive from the project name alone (repo name),
    // never from naming_ctx.project which includes cwd subdir offset.
    let identity_adapter = ctx.composition.workspace_identity(root.as_ref());
    let canonical_project = SessionNamingPolicy::sanitize_component(&name);
    usecases::identity::ensure_project_identity(
        &identity_adapter,
        &real_instance_id,
        &root,
        &name,
        &canonical_project,
    )?;

    let subdir_key = usecases::naming::resolve_subdir_key(&root, Some(invocation_cwd.as_path()));
    let relative_display = usecases::naming::resolve_relative_display_path(
        &name,
        &root,
        &workspace_name,
        Some(invocation_cwd.as_path()),
    );
    usecases::identity::ensure_workspace_identity(
        &identity_adapter,
        &policy,
        &naming_ctx,
        &subdir_key,
        relative_display.as_deref(),
    )?;

    let ws_identity_key =
        workhive_core::domain::WorkspaceIdentityKey::new(&workspace_name, &subdir_key);
    let workspace_state_key =
        usecases::identity::derive_workspace_state_key(&identity_adapter, &ws_identity_key)?;

    let snapshot_identity = usecases::open::SnapshotIdentityContext {
        instance_id: real_instance_id,
        project: naming_ctx.project.clone(),
        runtime_backend: ctx.composition.runtime_backend(),
        state_key: Some(workspace_state_key.clone()),
    };

    // Trusted-root admission already gated at the top of `cmd_open`. Both
    // `UseProjectRoot` and `Provision` mutate state (sessions, runtime,
    // identity, snapshots, worktrees), so the single early gate covers
    // them all.

    // Stage 1-2: validate + prepare plan (read-only, no mutations)
    let mut plan = usecases::open::prepare_open_plan_with_invocation_cwd(
        &provisioner,
        &runtime,
        &ws_fs,
        &root,
        &ws_id,
        plan_server,
        plan_session,
        open_intent,
        Some(invocation_cwd.as_path()),
        snapshot_enabled,
        open_mode,
        Some(snapshot_identity),
    )
    .map_err(|e| render_open_error(ctx, e))?;

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

    // Stage 3-4: execute plan (all mutations)
    let restore_catalog =
        usecases::launch_resolver::effective_catalog_from_config(&ctx.config, Some(root.as_ref()))?;
    let restore_exec_checker = ctx.composition.executable_checker();
    let restore_instrumentation = ctx
        .composition
        .tool_launch_instrumentation(root.as_ref())
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                "failed to construct tool launch instrumentation; tool panes that require restore wrapping will fail closed"
            );
            e
        })
        .ok();
    let mut result = usecases::open::execute_open_plan(
        &runtime,
        &runtime,
        &snapshot_store,
        &plan,
        &restore_catalog,
        &restore_exec_checker,
        restore_instrumentation
            .as_ref()
            .map(|i| i as &dyn workhive_core::ports::ToolLaunchInstrumentationPort),
    )?;
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

    // Snapshot open-policy gate.
    //
    // When snapshot.enabled = false the entire snapshot runtime branch must be
    // skipped — no hook install, no hook removal, no monitor spawn, no baseline
    // write, no `.workhive/snapshots/` write — including the previous
    // "cleanup-on-open" call into `disable_snapshot_runtime_for_session`, which
    // would surface tmux hook compatibility warnings on hosts where hooks are
    // not supported. We mark the supervisor outcome explicitly and emit a
    // user-facing "snapshot skipped" notice instead.
    let snapshot_policy =
        usecases::snapshot_open_policy::resolve_snapshot_open_policy(&SnapshotConfig {
            enabled: snapshot_enabled,
            ..SnapshotConfig::default()
        });
    if !snapshot_policy.is_runtime_active() {
        result.snapshot_supervisor_outcome =
            usecases::open::SnapshotSupervisorOutcome::NotRequested;
        let _ = &snapshot_store; // kept in scope for the enabled branch below
    }

    // Display recovery outcome — no silent fallbacks.
    match &result.outcome {
        usecases::open::OpenOutcome::SnapshotRestoreFailed { reason } => {
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_SNAPSHOT_FALLBACK_FAILED,
                    &[reason.as_str()],
                )
            );
        }
        usecases::open::OpenOutcome::SnapshotMissing => {
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve(locale, k::OPEN_SNAPSHOT_FALLBACK_NOT_FOUND)
            );
        }
        usecases::open::OpenOutcome::ForceRecreated => {
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::OPEN_SNAPSHOT_FORCE_LAYOUT)
            );
        }
        usecases::open::OpenOutcome::LayoutApplied => {}
        _ => {}
    }

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
    log_open_phase("snapshot_restore_layout_apply", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Workhive-owned pane titles captured this run are reasserted after
    // the readiness wait but before `attach_workspace` so external
    // runtime tools (Codex, shell title integrations, etc.) cannot leave
    // a stale terminal title visible to the user. Only sessions this
    // `open` call actually created or restored are filled in — reused
    // live sessions stay untouched.
    let layout_application = if result.outcome.should_apply_layout() {
        apply_layout_and_register(ctx, &runtime, &plan, root.as_ref())?
    } else {
        LayoutApplication::default()
    };
    let layout_pane_count = layout_application.layout_pane_count;
    let degraded_launches = layout_application.degraded_launches;
    let mut managed_pane_titles = layout_application.managed_pane_titles;

    // Runtime sync: reconcile baseline-registered panes with actual runtime state.
    sync_pane_registry_after_open(ctx, &runtime, &plan);

    // Route snapshot side-effects through the typed plan in
    // `workhive_core::usecases::snapshot_open_side_effects` so that the
    // disabled policy is provably short-circuited at a single seam — the
    // default-running fake-port test in core proves no baseline
    // persistence, monitor spawn, or hook install/remove can fire when
    // `snapshot.enabled = false`.
    let recovery_path = result.recovery_path.clone();
    let session_name_for_side_effects = result.session_name.clone();
    let mut side_effects = CliSnapshotOpenSideEffects {
        composition: &ctx.composition,
        snapshot_store: &snapshot_store,
        snapshot_runtime: &runtime,
        pane_registry: &ctx.pane_registry,
        i18n: &ctx.i18n,
        locale,
        project_root: root.as_ref(),
        server: &plan.server,
        session: session_name_for_side_effects.as_str(),
        workspace_state_key: &workspace_state_key,
    };
    let dispatch_outcome =
        usecases::snapshot_open_side_effects::dispatch_snapshot_open_side_effects(
            snapshot_policy,
            &recovery_path,
            &mut side_effects,
        );
    // Side effects must drop before `record_snapshot_supervisor_outcome`
    // mutably borrows `result`. Reassigning to `_` ends the borrow held in
    // `side_effects.session` (which captured `session_name_for_side_effects`).
    let _ = side_effects;
    if let usecases::snapshot_open_side_effects::SnapshotOpenSideEffectOutcome::Active(
        monitor_start,
    ) = dispatch_outcome
        && let Some(reason) = record_snapshot_supervisor_outcome(&mut result, monitor_start)
    {
        tracing::warn!(
            workspace = %result.session_name,
            error = %reason,
            "failed to spawn snapshot monitor process (degraded open outcome)"
        );
        let reason_str = reason.to_string();
        eprintln!(
            "{}",
            ctx.i18n.resolve_with_args(
                locale,
                workhive_core::i18n_keys::OPEN_WARNING_SNAPSHOT_MONITOR_FAILED,
                &[&reason_str],
            )
        );
    }
    log_open_phase("runtime_sync_baseline_persistence", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    eprintln!(
        "{}",
        render_interaction_mode_message(ctx, interaction_mode, &ws_id)
    );

    // ── Pane runtime control plane supervisor lifecycle ──
    //
    // `pane send` requires the supervisor to be running on the project's
    // canonical UDS. We launch the hidden `internal control-plane serve`
    // subprocess here so the supervisor outlives the parent's `tmux
    // attach` exec.
    //
    // Liveness contract: we MUST NOT trust
    // the socket file's existence on its own — a Unix socket file can
    // outlive a crashed supervisor and would let `workspace open` report
    // success while `pane send` cannot communicate. We instead use a
    // typed `LocalPaneControlPlaneClient::ping()` to decide whether the
    // supervisor is genuinely live:
    //
    //   * `Ok(_)`                       → reuse the running supervisor;
    //   * `SupervisorUnavailable`       → stale socket; remove + respawn;
    //   * `ProtocolVersionMismatch` /
    //     `Backend` / `Transport`       → degraded summary, do not silently
    //                                     skip spawn.
    //
    // We do NOT silently fall back to direct tmux for `pane send` even
    // when this spawn fails — the `pane send` self-heal retry path will
    // attempt one more spawn at delivery time, and only after that
    // attempt fails is a localized error returned.
    let supervisor_degraded_reason: Option<String> =
        ensure_control_plane_supervisor(ctx, root.as_ref(), &plan.server, &result.session_name);
    log_open_phase("control_plane_probe_spawn", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

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
                &[&result.session_name]
            )
        );
    }
    if !no_attach && !result.outcome.is_session_reused() {
        let expected_panes: usize = layout_pane_count.map(|n| n as usize).unwrap_or_else(|| {
            if let usecases::open::OpenOutcome::SnapshotRestored { panes, .. } = &result.outcome {
                *panes as usize
            } else {
                1
            }
        });
        if expected_panes > 0 {
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::OPEN_PROGRESS_WAITING_PANES)
            );
            let wait_outcome = open_readiness::wait_for_panes_ready(
                &runtime,
                &plan.server,
                &result.session_name,
                expected_panes,
                open_readiness::DEFAULT_BUDGET_MS,
                open_readiness::DEFAULT_POLL_INTERVAL_MS,
                &open_readiness::ThreadSleeper,
                std::time::Instant::now(),
            );
            if matches!(
                wait_outcome,
                open_readiness::ReadinessOutcome::TimedOut { .. }
                    | open_readiness::ReadinessOutcome::Degraded { .. }
            ) {
                tracing::warn!(
                    workspace = %result.session_name,
                    outcome = ?wait_outcome,
                    "pane readiness wait did not converge before attach"
                );
            }
            if let Some(line) = open_readiness::render_outcome(&wait_outcome, &ctx.i18n, locale) {
                eprintln!("{line}");
            }
        }
    }
    log_open_phase("pane_readiness_wait", __open_phase_started_at);
    __open_phase_started_at = std::time::Instant::now();

    // Pane title reassertion — defends against runtime tools (e.g. Codex)
    // that overwrite the terminal title between layout application and
    // user attach. Order is fixed: layout/restore -> readiness wait ->
    // reassert -> attach. We deliberately skip reassertion for live
    // sessions Workhive merely reused this run, because we do not have
    // authoritative managed-pane metadata for those sessions and a
    // blind rewrite could clobber a meaningful state the user already
    // sees.
    //
    // For SnapshotRestored we also extend `managed_pane_titles` from the
    // pane registry so each restored pane gets a launch-intent-derived
    // title written back, instead of trusting whatever string the
    // captured tmux state carries (which may be a stale project name).
    //
    // Before reading the registry, populate it from the snapshot's
    // typed `launch_intent` field (already filtered by the freshness
    // rule in `hydrate_snapshot_with_baseline`). Otherwise the
    // in-memory registry only carries `EmptyShell` defaults from the
    // post-restore runtime sync, and every restored pane would surface
    // as the literal `shell` title.
    if !result.outcome.is_session_reused() {
        if let usecases::open::OpenOutcome::SnapshotRestored { .. } = &result.outcome {
            register_restored_panes_from_snapshot(
                ctx,
                &runtime,
                &snapshot_store,
                &workspace_state_key,
                &plan.server,
                &result.session_name,
                root.as_ref(),
            );
            extend_managed_titles_from_registry(
                &ctx.pane_registry,
                &plan.server,
                &result.session_name,
                &mut managed_pane_titles,
            );
        }
        reassert_managed_pane_titles(&runtime, &plan.server, &managed_pane_titles);
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
        if let Err(e) = ctx.composition.spawn_pane_title_guard_process(
            &plan.server,
            &result.session_name,
            &managed_pane_titles,
            500,
            8_000,
        ) {
            tracing::warn!(
                error = %e,
                count = managed_pane_titles.len(),
                server = %plan.server,
                session = %result.session_name,
                "failed to spawn post-attach pane-title guard; pre-attach reassert still applied"
            );
        } else {
            tracing::debug!(
                count = managed_pane_titles.len(),
                server = %plan.server,
                session = %result.session_name,
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
    crate::commands::attach_usage::mark_used_then_maybe_attach(
        &runtime,
        workhive_core::ports::AttachWorkspaceRequest {
            group: plan.server.clone(),
            workspace_name: result.session_name.clone(),
        },
        !no_attach,
        || crate::commands::list::mark_workspace_used_now(&root, &workspace_name, ctx),
    )?;
    if !no_attach && result.outcome.is_session_reused() {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::OPEN_REUSING_SESSION, &[&result.session_name])
        );
    }
    log_open_phase("attach", __open_phase_started_at);

    // Success + summary output AFTER attach succeeds (or if --no-attach).
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::OPEN_SUCCESS, &[&result.session_name])
    );

    print_open_summary(
        ctx,
        locale,
        was_auto_initialized,
        &result.provenance,
        &result.outcome,
        &result.session_name,
        result.restored_panes,
        layout_pane_count,
        &degraded_launches,
        no_attach,
    );

    if let Some(reason) = supervisor_degraded_reason {
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
        session = %result.session_name,
        "workspace open completed"
    );
    Ok(())
}

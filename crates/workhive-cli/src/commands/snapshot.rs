use super::*;

pub(crate) fn cmd_snapshot(
    action: InternalSnapshotAction,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    match action {
        InternalSnapshotAction::Monitor {
            project_root,
            server,
            session,
            state_key,
        } => cmd_snapshot_monitor(&project_root, &server, &session, &state_key, ctx),
        InternalSnapshotAction::Notify {
            project_root,
            server,
            session,
            state_key,
            snapshot_event,
        } => cmd_snapshot_notify(
            &project_root,
            &server,
            &session,
            &state_key,
            &snapshot_event,
            ctx,
        ),
    }
}

pub(crate) fn cmd_snapshot_notify(
    project_root: &str,
    server: &str,
    session: &str,
    state_key_raw: &str,
    snapshot_event: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let root = Utf8PathBuf::from(project_root);
    let snapshot_config = resolve_effective_snapshot_config(ctx, root.as_ref())?;
    if !snapshot_config.enabled || !snapshot_config.save_on_event {
        return Ok(());
    }
    let state_key = WorkspaceStateKey::parse(state_key_raw).ok_or_else(|| {
        CliError::Runtime(format!(
            "invalid state_key for snapshot notify: '{state_key_raw}'"
        ))
    })?;

    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());
    let now_ms = epoch_millis_now();
    if let Some(manifest) = snapshot_store.read_manifest(&state_key)?
        && should_debounce(&manifest.last_saved_at, now_ms, snapshot_config.debounce_ms)
    {
        tracing::debug!(
            state_key = %state_key,
            event = snapshot_event,
            debounce_ms = snapshot_config.debounce_ms,
            "snapshot notify debounced"
        );
        return Ok(());
    }

    hydrate_registry_from_baseline(&snapshot_store, &state_key, server, session, ctx)?;
    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);
    let event_kind = parse_snapshot_event_kind(snapshot_event);
    let persist_ctx = SnapshotPersistContext {
        snapshot_runtime: &runtime,
        snapshot_store: &snapshot_store,
        server,
        session,
        state_key: &state_key,
        snapshot_config: &snapshot_config,
    };
    let _ = capture_and_persist_snapshot(
        &persist_ctx,
        SnapshotSource::EventSave,
        event_kind,
        Some(format!("hook_event={snapshot_event}")),
    )?;
    Ok(())
}

pub(crate) fn cmd_snapshot_monitor(
    project_root: &str,
    server: &str,
    session: &str,
    state_key_raw: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let root = Utf8PathBuf::from(project_root);
    let snapshot_config = resolve_effective_snapshot_config(ctx, root.as_ref())?;
    if !snapshot_config.enabled {
        return Ok(());
    }
    let state_key = WorkspaceStateKey::parse(state_key_raw).ok_or_else(|| {
        CliError::Runtime(format!(
            "invalid state_key for snapshot monitor: '{state_key_raw}'"
        ))
    })?;

    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());
    hydrate_registry_from_baseline(&snapshot_store, &state_key, server, session, ctx)?;
    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);

    let lock = SupervisorLock {
        pid: std::process::id(),
        heartbeat: snapshot_timestamp_now(),
        state_key: state_key.clone(),
    };
    if !snapshot_store.acquire_lock(&lock)? {
        tracing::debug!(
            state_key = %state_key,
            session,
            "snapshot monitor already running (lock held)"
        );
        return Ok(());
    }

    let mut hooks_installed = false;
    let run_result = (|| -> Result<(), CliError> {
        snapshot_store.set_hooks_registered_state(&state_key, false)?;
        if snapshot_config.save_on_event {
            let current_exe = std::env::current_exe().map_err(|e| {
                CliError::Runtime(format!("cannot resolve current executable: {e}"))
            })?;
            let current_exe = Utf8PathBuf::try_from(current_exe).map_err(|e| {
                CliError::Runtime(format!("current executable path is not valid UTF-8: {e}"))
            })?;
            let notify_prefix = build_snapshot_notify_prefix(
                current_exe.as_ref(),
                root.as_ref(),
                server,
                session,
                &state_key,
            );
            runtime.configure_snapshot_hooks(
                server,
                session,
                SnapshotHookMode::Install,
                &notify_prefix,
            )?;
            hooks_installed = true;
            snapshot_store.set_hooks_registered_state(&state_key, true)?;
        }

        let persist_ctx = SnapshotPersistContext {
            snapshot_runtime: &runtime,
            snapshot_store: &snapshot_store,
            server,
            session,
            state_key: &state_key,
            snapshot_config: &snapshot_config,
        };

        let now_secs = epoch_seconds_now();
        let mut last_heartbeat = now_secs;
        let mut last_timed_save = now_secs;
        let mut last_full_save = now_secs;
        let mut last_command_poll = now_secs;

        let baseline_topology = runtime.capture_topology(server, session)?;
        let mut last_command_signature = Some(command_signature(&baseline_topology));
        if snapshot_store.read_manifest(&state_key)?.is_none() {
            let _ = capture_and_persist_snapshot(
                &persist_ctx,
                SnapshotSource::ManualSave,
                SnapshotEventKind::ManualSave,
                Some("supervisor bootstrap".to_owned()),
            )?;
        }

        loop {
            match runtime.has_workspace(server, session) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    tracing::warn!(
                        session,
                        server,
                        error = %e,
                        "snapshot monitor stopping after workspace probe error"
                    );
                    break;
                }
            }

            let now_secs = epoch_seconds_now();
            let now_ms = epoch_millis_now();
            if now_secs.saturating_sub(last_heartbeat) >= 5 {
                snapshot_store.update_heartbeat(&state_key, &snapshot_timestamp_now())?;
                last_heartbeat = now_secs;
            }

            if snapshot_config.save_on_command_change
                && snapshot_config.command_poll_seconds > 0
                && now_secs.saturating_sub(last_command_poll)
                    >= u64::from(snapshot_config.command_poll_seconds)
            {
                let topology = runtime.capture_topology(server, session)?;
                let signature = command_signature(&topology);
                if let Some(previous) = &last_command_signature
                    && previous != &signature
                    && !is_snapshot_save_debounced(
                        &snapshot_store,
                        &state_key,
                        snapshot_config.debounce_ms,
                        now_ms,
                    )?
                {
                    let _ = capture_and_persist_snapshot(
                        &persist_ctx,
                        SnapshotSource::CommandChangeSave,
                        SnapshotEventKind::PaneCommandChanged,
                        Some("command fingerprint changed".to_owned()),
                    )?;
                }
                last_command_signature = Some(signature);
                last_command_poll = now_secs;
            }

            if snapshot_config.interval_minutes > 0
                && now_secs.saturating_sub(last_timed_save)
                    >= u64::from(snapshot_config.interval_minutes) * 60
                && !is_snapshot_save_debounced(
                    &snapshot_store,
                    &state_key,
                    snapshot_config.debounce_ms,
                    now_ms,
                )?
            {
                let _ = capture_and_persist_snapshot(
                    &persist_ctx,
                    SnapshotSource::TimedSave,
                    SnapshotEventKind::PeriodicReconcile,
                    Some("interval tick".to_owned()),
                )?;
                last_timed_save = now_secs;
            }

            if snapshot_config.full_snapshot_interval_minutes > 0
                && now_secs.saturating_sub(last_full_save)
                    >= u64::from(snapshot_config.full_snapshot_interval_minutes) * 60
                && !is_snapshot_save_debounced(
                    &snapshot_store,
                    &state_key,
                    snapshot_config.debounce_ms,
                    now_ms,
                )?
            {
                let _ = capture_and_persist_snapshot(
                    &persist_ctx,
                    SnapshotSource::PeriodicReconcile,
                    SnapshotEventKind::PeriodicReconcile,
                    Some("full snapshot interval tick".to_owned()),
                )?;
                last_full_save = now_secs;
            }

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        Ok(())
    })();

    if hooks_installed {
        let _ = runtime.configure_snapshot_hooks(server, session, SnapshotHookMode::Remove, "");
    }
    let _ = snapshot_store.set_hooks_registered_state(&state_key, false);
    let _ = snapshot_store.release_lock(&state_key);
    run_result
}

pub(crate) struct SnapshotPersistContext<'a> {
    pub(crate) snapshot_runtime: &'a dyn SnapshotRuntimePort,
    pub(crate) snapshot_store: &'a dyn SnapshotStorePort,
    pub(crate) server: &'a str,
    pub(crate) session: &'a str,
    pub(crate) state_key: &'a WorkspaceStateKey,
    pub(crate) snapshot_config: &'a SnapshotConfig,
}

pub(crate) fn capture_and_persist_snapshot(
    ctx: &SnapshotPersistContext<'_>,
    source: SnapshotSource,
    event_kind: SnapshotEventKind,
    detail: Option<String>,
) -> Result<Option<SnapshotRevisionId>, CliError> {
    let topology = ctx
        .snapshot_runtime
        .capture_topology(ctx.server, ctx.session)?;
    if topology.panes.is_empty() {
        return Ok(None);
    }

    let next_revision = match ctx.snapshot_store.read_manifest(ctx.state_key)? {
        Some(manifest) => manifest.head_revision.next(),
        None => SnapshotRevisionId::first(),
    };
    let captured_at = snapshot_timestamp_now();
    let capture_ctx = usecases::snapshot_capture::CaptureContext {
        state_key: ctx.state_key.clone(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: usecases::close::current_platform().to_owned(),
        captured_at: captured_at.clone(),
        source,
        next_revision_id: next_revision,
        config: ctx.snapshot_config.clone(),
    };
    let mut snapshot =
        usecases::snapshot_capture::build_snapshot_from_topology(&capture_ctx, &topology)?;
    snapshot.metadata.content_hash =
        Some(usecases::snapshot_capture::compute_content_hash(&snapshot)?);

    let revision = ctx.snapshot_store.save_revision(&snapshot)?;
    let event = SnapshotEvent {
        timestamp: captured_at.clone(),
        revision_id: Some(revision),
        kind: event_kind,
        detail,
    };
    ctx.snapshot_store.append_events(ctx.state_key, &[event])?;
    ctx.snapshot_store.apply_retention(
        ctx.state_key,
        ctx.snapshot_config.retain_full_revisions,
        ctx.snapshot_config.retain_event_days,
    )?;

    Ok(Some(revision))
}

pub(crate) fn persist_open_launch_baseline_from_registry(
    snapshot_store: &dyn SnapshotStorePort,
    pane_registry: &dyn PaneRegistryPort,
    state_key: &WorkspaceStateKey,
    server: &str,
    session: &str,
    updated_at: &str,
) -> Result<usize, CliError> {
    let descriptors = pane_registry
        .list_workspace(session)
        .map_err(|e| CliError::Runtime(format!("failed to read pane registry: {e}")))?;
    let descriptors = descriptors
        .into_iter()
        .filter(|descriptor| descriptor.group == server)
        .collect::<Vec<_>>();
    let entries = usecases::snapshot_baseline::entries_from_descriptors(&descriptors);
    if entries.is_empty() {
        return Err(SnapshotError::LaunchBaselineMissing {
            state_key: state_key.to_string(),
        }
        .into());
    }
    usecases::snapshot_baseline::persist_launch_baseline(
        snapshot_store,
        state_key,
        entries,
        updated_at.to_owned(),
    )
    .map_err(Into::into)
}

pub(crate) fn ensure_launch_baseline_available_for_monitor(
    snapshot_store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
) -> Result<(), CliError> {
    let baseline = snapshot_store.load_launch_baseline(state_key)?;
    match baseline {
        Some(baseline) if !baseline.entries.is_empty() => Ok(()),
        Some(_) | None => Err(SnapshotError::LaunchBaselineMissing {
            state_key: state_key.to_string(),
        }
        .into()),
    }
}

pub(crate) fn ensure_or_bootstrap_reused_session_launch_baseline(
    snapshot_runtime: &dyn SnapshotRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
    server: &str,
    session: &str,
    updated_at: &str,
) -> Result<(), CliError> {
    if ensure_launch_baseline_available_for_monitor(snapshot_store, state_key).is_ok() {
        return Ok(());
    }

    let topology = snapshot_runtime.capture_topology(server, session)?;
    let entries = usecases::snapshot_baseline::entries_from_topology(&topology, server, session);
    if entries.is_empty() {
        return Err(SnapshotError::LaunchBaselineMissing {
            state_key: state_key.to_string(),
        }
        .into());
    }

    usecases::snapshot_baseline::persist_launch_baseline(
        snapshot_store,
        state_key,
        entries,
        updated_at.to_owned(),
    )?;
    tracing::info!(
        state_key = %state_key,
        server,
        session,
        "bootstrapped launch baseline for reused live session"
    );
    Ok(())
}

pub(crate) fn parse_snapshot_event_kind(raw: &str) -> SnapshotEventKind {
    match raw {
        "new_window" => SnapshotEventKind::WindowAdded,
        "kill_window" => SnapshotEventKind::WindowRemoved,
        "split_window" => SnapshotEventKind::PaneAdded,
        "kill_pane" => SnapshotEventKind::PaneRemoved,
        "select_pane" => SnapshotEventKind::PaneFocusChanged,
        "layout_changed" => SnapshotEventKind::LayoutChanged,
        "pane_died" | "pane_exited" => SnapshotEventKind::PaneRemoved,
        _ => SnapshotEventKind::ManualSave,
    }
}

pub(crate) fn command_signature(topology: &RuntimeTopologySnapshot) -> String {
    let mut panes = topology
        .panes
        .iter()
        .map(|pane| {
            (
                pane.window_index,
                pane.pane_index,
                pane.current_command.clone().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    panes.sort_by_key(|(window_index, pane_index, _)| (*window_index, *pane_index));
    panes
        .into_iter()
        .map(|(window_index, pane_index, command)| format!("{window_index}:{pane_index}:{command}"))
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) fn parse_epoch_seconds(raw: &str) -> Option<u64> {
    if let Some(stripped) = raw.strip_suffix('s') {
        return stripped.parse::<u64>().ok();
    }
    raw.parse::<u64>().ok()
}

pub(crate) fn should_debounce(last_saved_at: &str, now_ms: u128, debounce_ms: u64) -> bool {
    if debounce_ms == 0 {
        return false;
    }
    let Some(last_seconds) = parse_epoch_seconds(last_saved_at) else {
        return false;
    };
    let last_ms = u128::from(last_seconds).saturating_mul(1000);
    now_ms.saturating_sub(last_ms) < u128::from(debounce_ms)
}

pub(crate) fn is_snapshot_save_debounced(
    snapshot_store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
    debounce_ms: u64,
    now_ms: u128,
) -> Result<bool, SnapshotError> {
    let manifest = snapshot_store.read_manifest(state_key)?;
    Ok(manifest
        .as_ref()
        .is_some_and(|m| should_debounce(&m.last_saved_at, now_ms, debounce_ms)))
}

pub(crate) fn snapshot_timestamp_now() -> String {
    format!("{}s", epoch_seconds_now())
}

pub(crate) fn epoch_seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn epoch_millis_now() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub(crate) fn shell_quote_arg(value: &str) -> String {
    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Load persisted launch baseline into the in-memory pane registry so
/// that cross-process monitor/notify invocations have registry-first
/// launch metadata available for topology capture.
pub(crate) fn hydrate_registry_from_baseline(
    store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
    server: &str,
    session: &str,
    ctx: &CommandContext,
) -> Result<usize, CliError> {
    let baseline = match store.load_launch_baseline(state_key) {
        Ok(Some(b)) => b,
        Ok(None) => {
            return Err(SnapshotError::LaunchBaselineMissing {
                state_key: state_key.to_string(),
            }
            .into());
        }
        Err(e) => return Err(e.into()),
    };
    if baseline.entries.is_empty() {
        return Err(SnapshotError::LaunchBaselineMissing {
            state_key: state_key.to_string(),
        }
        .into());
    }
    for entry in &baseline.entries {
        let runtime_id =
            workhive_core::domain::PaneRuntimeId::derive(server, session, &entry.unit_id);
        let descriptor = workhive_core::domain::PaneDescriptor {
            runtime_id,
            group: server.to_owned(),
            workspace: session.to_owned(),
            unit_id: workhive_core::domain::UnitId::new(&entry.unit_id),
            cwd: String::new(),
            command: String::new(),
            kind: workhive_core::domain::PaneKind::Terminal,
            state: workhive_core::domain::PaneState::Unknown,
            observed_status: workhive_core::domain::PaneRuntimeStatus::Unknown,
            role: None,
            launch_origin: entry.launch_origin.clone(),
            capabilities: std::collections::BTreeSet::new(),
        };
        if let Err(e) = ctx.pane_registry.upsert(descriptor) {
            tracing::warn!(
                unit_id = %entry.unit_id,
                error = %e,
                "failed to hydrate pane registry from baseline"
            );
        }
    }
    let entries = baseline.entries.len();
    tracing::debug!(
        state_key = %state_key,
        entries,
        "hydrated pane registry from persisted launch baseline"
    );
    Ok(entries)
}

pub(crate) fn build_snapshot_notify_prefix(
    workhive_executable: &camino::Utf8Path,
    project_root: &camino::Utf8Path,
    server: &str,
    session: &str,
    state_key: &WorkspaceStateKey,
) -> String {
    format!(
        "{} internal snapshot notify --project-root {} --server {} --session {} --state-key {}",
        shell_quote_arg(workhive_executable.as_str()),
        shell_quote_arg(project_root.as_str()),
        shell_quote_arg(server),
        shell_quote_arg(session),
        shell_quote_arg(&state_key.to_string())
    )
}

pub(crate) fn disable_snapshot_runtime_for_session(
    snapshot_runtime: &dyn SnapshotRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    server: &str,
    session: &str,
    state_key: &WorkspaceStateKey,
) -> Result<(), CliError> {
    snapshot_runtime.configure_snapshot_hooks(server, session, SnapshotHookMode::Remove, "")?;
    snapshot_store.set_hooks_registered_state(state_key, false)?;
    snapshot_store.release_lock(state_key)?;
    Ok(())
}

// ── List ──

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    #[test]
    fn notify_prefix_uses_explicit_executable_path() {
        let executable = Utf8PathBuf::from("/usr/local/bin/workhive");
        let root = Utf8PathBuf::from("/tmp/repo");
        let key = WorkspaceStateKey::new(
            "abcd".to_owned(),
            "repo".to_owned(),
            String::new(),
            "feat".to_owned(),
        );

        let cmd =
            build_snapshot_notify_prefix(executable.as_ref(), root.as_ref(), "srv", "sess", &key);
        assert!(
            cmd.starts_with("'/usr/local/bin/workhive' internal snapshot notify"),
            "hook command must invoke the hidden internal snapshot path; got: {cmd}"
        );
        assert!(
            !cmd.starts_with("workhive snapshot notify"),
            "hook command must not rely on PATH lookup"
        );
    }

    #[test]
    fn notify_prefix_quotes_unsafe_arguments() {
        let executable = Utf8PathBuf::from("/Applications/Work Hive/workhive");
        let root = Utf8PathBuf::from("/tmp/my repo");
        let key = WorkspaceStateKey::new(
            "abcd".to_owned(),
            "repo".to_owned(),
            "sub".to_owned(),
            "feat".to_owned(),
        );

        let cmd = build_snapshot_notify_prefix(
            executable.as_ref(),
            root.as_ref(),
            "srv name",
            "sess",
            &key,
        );
        assert!(cmd.contains("'/Applications/Work Hive/workhive'"));
        assert!(cmd.contains("'/tmp/my repo'"));
        assert!(cmd.contains("'srv name'"));
    }
}

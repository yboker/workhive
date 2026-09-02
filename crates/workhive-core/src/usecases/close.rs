//! `close` use case — detach or kill the current workspace session.
//!
//! On kill, snapshot persistence uses the canonical revision-based
//! `SnapshotStorePort` with runtime topology captured through
//! `SnapshotRuntimePort`.

use crate::domain::{
    RuntimeBackendId, SnapshotEvent, SnapshotEventKind, SnapshotRevisionId, SnapshotSource,
    WorkspaceId, WorkspaceStateKey,
};
use crate::ports::{
    RuntimeError, SnapshotConfig, SnapshotPaneRuntimePort, SnapshotStorePort, WorkspaceRuntimePort,
};

/// Errors from the `close` use case.
#[derive(Debug, thiserror::Error)]
pub enum CloseError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Close result indicating what action was taken and any partial failures.
pub enum CloseAction {
    /// Session was detached (user remains in terminal, session keeps running).
    Detached,
    /// Session was killed after attempting to persist state.
    Killed {
        snapshot_status: SnapshotPersistStatus,
    },
}

/// What happened with snapshot persistence during `close --kill`.
pub enum SnapshotPersistStatus {
    Saved,
    Disabled,
    Failed { reason: String },
    SkippedNoUnits,
}

/// Identity context required by snapshot persistence.
#[derive(Debug, Clone)]
pub struct SnapshotContext {
    pub instance_id: String,
    pub project: String,
    pub server_name: String,
    pub runtime_backend: RuntimeBackendId,
    pub platform: String,
    /// Typed state key display string. Required for snapshot persistence.
    pub state_key: Option<String>,
}

/// Return the current platform identifier string.
pub fn current_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

/// Close the current session.
///
/// - Default: detach
/// - With `kill`: save snapshot, then kill session
///
/// Snapshot failure does not block the kill. The result communicates the
/// status so the CLI can inform the user.
#[allow(clippy::too_many_arguments)]
pub fn close_session(
    ws_runtime: &dyn WorkspaceRuntimePort,
    pane_runtime: &dyn SnapshotPaneRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    server: &str,
    session: &str,
    workspace: &WorkspaceId,
    _generation: u64,
    kill: bool,
    snapshot_enabled: bool,
    snap_ctx: &SnapshotContext,
) -> Result<CloseAction, CloseError> {
    let mut effective_ctx = snap_ctx.clone();
    if effective_ctx.state_key.is_none() {
        let inferred = WorkspaceStateKey::new(
            snap_ctx.instance_id.clone(),
            snap_ctx.project.clone(),
            String::new(),
            workspace.as_str().to_owned(),
        );
        effective_ctx.state_key = Some(inferred.to_string());
    }

    let snapshot_config = SnapshotConfig {
        enabled: snapshot_enabled,
        ..SnapshotConfig::default()
    };
    close_session_with_config(
        ws_runtime,
        pane_runtime,
        snapshot_store,
        server,
        session,
        kill,
        &snapshot_config,
        &effective_ctx,
    )
}

/// Close the current session using the fully resolved snapshot config.
#[allow(clippy::too_many_arguments)]
pub fn close_session_with_config(
    ws_runtime: &dyn WorkspaceRuntimePort,
    pane_runtime: &dyn SnapshotPaneRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    server: &str,
    session: &str,
    kill: bool,
    snapshot_config: &SnapshotConfig,
    snap_ctx: &SnapshotContext,
) -> Result<CloseAction, CloseError> {
    if kill {
        let snapshot_status = if !snapshot_config.enabled {
            SnapshotPersistStatus::Disabled
        } else {
            match snap_ctx
                .state_key
                .as_deref()
                .and_then(WorkspaceStateKey::parse)
            {
                Some(state_key) => persist_snapshot_on_close(
                    pane_runtime,
                    snapshot_store,
                    server,
                    session,
                    snapshot_config,
                    snap_ctx,
                    &state_key,
                ),
                None => SnapshotPersistStatus::Failed {
                    reason: "missing or invalid state_key in close snapshot context".to_owned(),
                },
            }
        };

        ws_runtime.destroy_workspace(server, session)?;
        tracing::info!(
            session,
            snapshot_enabled = snapshot_config.enabled,
            "session killed"
        );
        Ok(CloseAction::Killed { snapshot_status })
    } else {
        ws_runtime.detach_workspace(server)?;
        tracing::info!(session, "client detached");
        Ok(CloseAction::Detached)
    }
}

fn persist_snapshot_on_close(
    snapshot_runtime: &dyn SnapshotPaneRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    server: &str,
    session: &str,
    snapshot_config: &SnapshotConfig,
    snap_ctx: &SnapshotContext,
    state_key: &WorkspaceStateKey,
) -> SnapshotPersistStatus {
    let topology = match snapshot_runtime.capture_topology(server, session) {
        Ok(topology) => topology,
        Err(e) => {
            let reason = format!("topology capture failed: {e}");
            tracing::warn!(error = %reason, session, "failed to capture topology before close");
            return SnapshotPersistStatus::Failed { reason };
        }
    };

    if topology.panes.is_empty() {
        return SnapshotPersistStatus::SkippedNoUnits;
    }

    let next_revision = match snapshot_store.read_manifest(state_key) {
        Ok(Some(manifest)) => manifest.head_revision.next(),
        Ok(None) => SnapshotRevisionId::first(),
        Err(e) => {
            let reason = format!("manifest read failed: {e}");
            tracing::warn!(error = %reason, "failed to read manifest before close snapshot save");
            return SnapshotPersistStatus::Failed { reason };
        }
    };

    let captured_at = chrono_now_iso();
    let capture_ctx = crate::usecases::snapshot_capture::CaptureContext {
        state_key: state_key.clone(),
        runtime_backend: snap_ctx.runtime_backend.clone(),
        platform: snap_ctx.platform.clone(),
        captured_at: captured_at.clone(),
        source: SnapshotSource::SessionClosing,
        next_revision_id: next_revision,
        config: snapshot_config.clone(),
    };

    let mut snapshot = match crate::usecases::snapshot_capture::build_snapshot_from_topology(
        &capture_ctx,
        &topology,
    ) {
        Ok(snapshot) => snapshot,
        Err(e) => {
            let reason = format!("snapshot capture failed: {e}");
            tracing::warn!(error = %reason, "failed to build snapshot from topology");
            return SnapshotPersistStatus::Failed { reason };
        }
    };

    // F2: Hydrate snapshot with prior baseline so that close-time capture
    // preserves durable launch metadata even when tmux start_command is a
    // bare shell.
    let prior_baseline = match crate::usecases::snapshot_baseline::build_prior_baseline_map(
        snapshot_store,
        state_key,
        &snapshot,
    ) {
        Ok(prior_baseline) => prior_baseline,
        Err(e) => {
            let reason = format!("load launch baseline failed: {e}");
            tracing::warn!(error = %reason, "failed to load launch baseline for close snapshot hydration");
            return SnapshotPersistStatus::Failed { reason };
        }
    };
    if !prior_baseline.is_empty() {
        let hydrated = crate::usecases::snapshot_baseline::hydrate_snapshot_with_baseline(
            &mut snapshot,
            &prior_baseline,
        );
        if hydrated > 0 {
            tracing::info!(
                hydrated,
                "hydrated {hydrated} snapshot panes with prior baseline launch metadata on close"
            );
        }
    }

    match crate::usecases::snapshot_capture::compute_content_hash(&snapshot) {
        Ok(hash) => snapshot.metadata.content_hash = Some(hash),
        Err(e) => {
            let reason = format!("snapshot hashing failed: {e}");
            tracing::warn!(error = %reason, "failed to compute snapshot hash");
            return SnapshotPersistStatus::Failed { reason };
        }
    }

    let revision_id = match snapshot_store.save_revision(&snapshot) {
        Ok(revision_id) => revision_id,
        Err(e) => {
            let reason = format!("save_revision failed: {e}");
            tracing::warn!(error = %reason, "failed to persist snapshot revision");
            return SnapshotPersistStatus::Failed { reason };
        }
    };

    let event = SnapshotEvent {
        timestamp: captured_at.clone(),
        revision_id: Some(revision_id),
        kind: SnapshotEventKind::SessionClosing,
        detail: Some(format!("revision={revision_id}")),
    };
    if let Err(e) = snapshot_store.append_events(state_key, &[event]) {
        let reason = format!("append_events failed: {e}");
        tracing::warn!(error = %reason, "failed to append close snapshot event");
        return SnapshotPersistStatus::Failed { reason };
    }

    if let Err(e) = snapshot_store.apply_retention(
        state_key,
        snapshot_config.retain_full_revisions,
        snapshot_config.retain_event_days,
    ) {
        let reason = format!("apply_retention failed: {e}");
        tracing::warn!(error = %reason, "failed to apply snapshot retention");
        return SnapshotPersistStatus::Failed { reason };
    }

    SnapshotPersistStatus::Saved
}

/// Current time in ISO-8601 format (UTC, second precision).
///
/// Falls back to `"unknown"` with a warning if the system clock is before
/// the UNIX epoch — this must never silently produce `"0s"`.
fn chrono_now_iso() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(dur) => format!("{}s", dur.as_secs()),
        Err(e) => {
            tracing::warn!(error = %e, "system clock is before UNIX epoch; snapshot timestamp unavailable");
            "unknown".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use camino::Utf8PathBuf;

    use super::*;
    use crate::domain::{
        PaneDirection, PaneId, PaneLaunchOrigin, SnapshotHealthCheck, SnapshotHistoryEntry,
        SnapshotManifest, SupervisorLock, UnitId, WorkspaceStateKey,
    };
    use crate::errors::SnapshotError;
    use crate::ports::{
        AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, PaneRuntimePort,
        RuntimePaneSnapshot, RuntimeTopologySnapshot, RuntimeUnitSnapshot, RuntimeWindowSnapshot,
        SendInputRequest, SetUnitTitleRequest, SnapshotHookMode, SnapshotRuntimePort,
        SpawnUnitRequest, WorkspaceRuntimeInfo,
    };

    struct FakeRuntime {
        topology: RuntimeTopologySnapshot,
        destroyed: RefCell<Vec<(String, String)>>,
        detached: RefCell<Vec<String>>,
    }

    impl FakeRuntime {
        fn with_topology(topology: RuntimeTopologySnapshot) -> Self {
            Self {
                topology,
                destroyed: RefCell::new(Vec::new()),
                detached: RefCell::new(Vec::new()),
            }
        }
    }

    impl WorkspaceRuntimePort for FakeRuntime {
        fn has_workspace(&self, _group: &str, _workspace_name: &str) -> Result<bool, RuntimeError> {
            Ok(true)
        }

        fn create_workspace(
            &self,
            _request: CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn configure_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn detach_workspace(&self, group: &str) -> Result<(), RuntimeError> {
            self.detached.borrow_mut().push(group.to_owned());
            Ok(())
        }

        fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
            self.destroyed
                .borrow_mut()
                .push((group.to_owned(), workspace_name.to_owned()));
            Ok(())
        }

        fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            Ok(Vec::new())
        }

        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    impl PaneRuntimePort for FakeRuntime {
        fn spawn_unit(
            &self,
            _request: SpawnUnitRequest,
        ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "spawn_unit is not used by close tests".to_owned(),
            })
        }

        fn close_unit(&self, _group: &str, _unit_id: &UnitId) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn list_units(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
            Ok(Vec::new())
        }

        fn current_unit(&self, _group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "current_unit is not used by close tests".to_owned(),
            })
        }

        fn send_input(&self, _request: SendInputRequest) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn set_unit_title(&self, _request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    impl SnapshotRuntimePort for FakeRuntime {
        fn capture_topology(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<RuntimeTopologySnapshot, RuntimeError> {
            Ok(self.topology.clone())
        }

        fn create_window(
            &self,
            _group: &str,
            _workspace_name: &str,
            _window_index: u32,
            _name: Option<&str>,
            _cwd: &camino::Utf8Path,
        ) -> Result<RuntimePaneSnapshot, RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "create_window is not used by close tests".to_owned(),
            })
        }

        fn split_pane(
            &self,
            _group: &str,
            _workspace_name: &str,
            _target_unit: &UnitId,
            _direction: PaneDirection,
            _prep: crate::ports::SplitPanePreparation,
        ) -> Result<RuntimePaneSnapshot, RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "split_pane is not used by close tests".to_owned(),
            })
        }

        fn respawn_unit_with_cwd(
            &self,
            _group: &str,
            _workspace_name: &str,
            _unit_id: &UnitId,
            _cwd: &camino::Utf8Path,
        ) -> Result<(), RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "respawn_unit_with_cwd is not used by close tests".to_owned(),
            })
        }

        fn respawn_unit_with_command(
            &self,
            _group: &str,
            _workspace_name: &str,
            _unit_id: &UnitId,
            _cwd: &camino::Utf8Path,
            _command: &str,
        ) -> Result<(), RuntimeError> {
            Err(RuntimeError::OperationFailed {
                message: "respawn_unit_with_command is not used by close tests".to_owned(),
            })
        }

        fn apply_window_layout(
            &self,
            _group: &str,
            _workspace_name: &str,
            _window_index: u32,
            _layout: &str,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn select_window(
            &self,
            _group: &str,
            _workspace_name: &str,
            _window_index: u32,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn select_pane(&self, _group: &str, _unit_id: &UnitId) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn configure_snapshot_hooks(
            &self,
            _group: &str,
            _workspace_name: &str,
            _mode: SnapshotHookMode,
            _notify_command_prefix: &str,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    struct FakeStore {
        manifests: RefCell<Option<SnapshotManifest>>,
        saved: RefCell<Vec<crate::domain::WorkspaceSnapshot>>,
        events: RefCell<Vec<SnapshotEvent>>,
        retention_calls: RefCell<Vec<(u32, u32)>>,
        baselines: RefCell<Vec<crate::domain::LaunchBaseline>>,
        preloaded_baseline: RefCell<Option<crate::domain::LaunchBaseline>>,
        baseline_load_error: bool,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                manifests: RefCell::new(None),
                saved: RefCell::new(Vec::new()),
                events: RefCell::new(Vec::new()),
                retention_calls: RefCell::new(Vec::new()),
                baselines: RefCell::new(Vec::new()),
                preloaded_baseline: RefCell::new(None),
                baseline_load_error: false,
            }
        }

        fn with_baseline(baseline: crate::domain::LaunchBaseline) -> Self {
            Self {
                manifests: RefCell::new(None),
                saved: RefCell::new(Vec::new()),
                events: RefCell::new(Vec::new()),
                retention_calls: RefCell::new(Vec::new()),
                baselines: RefCell::new(Vec::new()),
                preloaded_baseline: RefCell::new(Some(baseline)),
                baseline_load_error: false,
            }
        }

        fn with_baseline_load_error() -> Self {
            Self {
                manifests: RefCell::new(None),
                saved: RefCell::new(Vec::new()),
                events: RefCell::new(Vec::new()),
                retention_calls: RefCell::new(Vec::new()),
                baselines: RefCell::new(Vec::new()),
                preloaded_baseline: RefCell::new(None),
                baseline_load_error: true,
            }
        }
    }

    impl SnapshotStorePort for FakeStore {
        fn save_revision(
            &self,
            snapshot: &crate::domain::WorkspaceSnapshot,
        ) -> Result<SnapshotRevisionId, SnapshotError> {
            self.saved.borrow_mut().push(snapshot.clone());
            self.manifests.replace(Some(SnapshotManifest {
                state_key: snapshot.state_key.clone(),
                head_revision: snapshot.revision_id,
                total_revisions: snapshot.revision_id.0,
                events_since_full: 0,
                last_saved_at: snapshot.captured_at.clone(),
                last_full_revision_at: snapshot.captured_at.clone(),
            }));
            Ok(snapshot.revision_id)
        }

        fn load_latest(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<crate::domain::WorkspaceSnapshot>, SnapshotError> {
            Ok(self.saved.borrow().last().cloned())
        }

        fn load_revision(
            &self,
            _state_key: &WorkspaceStateKey,
            revision_id: SnapshotRevisionId,
        ) -> Result<Option<crate::domain::WorkspaceSnapshot>, SnapshotError> {
            Ok(self
                .saved
                .borrow()
                .iter()
                .find(|snapshot| snapshot.revision_id == revision_id)
                .cloned())
        }

        fn append_events(
            &self,
            _state_key: &WorkspaceStateKey,
            events: &[SnapshotEvent],
        ) -> Result<(), SnapshotError> {
            self.events.borrow_mut().extend_from_slice(events);
            Ok(())
        }

        fn load_events_since(
            &self,
            _state_key: &WorkspaceStateKey,
            _since_revision: SnapshotRevisionId,
        ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
            Ok(self.events.borrow().clone())
        }

        fn read_manifest(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<SnapshotManifest>, SnapshotError> {
            Ok(self.manifests.borrow().clone())
        }

        fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
            self.manifests.replace(Some(manifest.clone()));
            Ok(())
        }

        fn list_history(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
            Ok(Vec::new())
        }

        fn acquire_lock(&self, _lock: &SupervisorLock) -> Result<bool, SnapshotError> {
            Ok(true)
        }

        fn release_lock(&self, _state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
            Ok(())
        }

        fn update_heartbeat(
            &self,
            _state_key: &WorkspaceStateKey,
            _heartbeat: &str,
        ) -> Result<(), SnapshotError> {
            Ok(())
        }

        fn read_lock(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<SupervisorLock>, SnapshotError> {
            Ok(None)
        }

        fn health_check(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<SnapshotHealthCheck, SnapshotError> {
            Ok(SnapshotHealthCheck {
                enabled: true,
                supervisor_running: false,
                hooks_registered: false,
                latest_save_time: None,
                latest_revision: None,
                manifest_readable: false,
                checksum_valid: None,
                state_key_match: true,
            })
        }

        fn verify_revision(
            &self,
            _state_key: &WorkspaceStateKey,
            _revision_id: SnapshotRevisionId,
        ) -> Result<bool, SnapshotError> {
            Ok(true)
        }

        fn apply_retention(
            &self,
            _state_key: &WorkspaceStateKey,
            retain_revisions: u32,
            retain_event_days: u32,
        ) -> Result<u32, SnapshotError> {
            self.retention_calls
                .borrow_mut()
                .push((retain_revisions, retain_event_days));
            Ok(0)
        }

        fn delete_all(&self, _state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
            Ok(())
        }

        fn delete_all_global(&self) -> Result<(), SnapshotError> {
            Ok(())
        }

        fn save_launch_baseline(
            &self,
            baseline: &crate::domain::LaunchBaseline,
        ) -> Result<(), SnapshotError> {
            self.baselines.borrow_mut().push(baseline.clone());
            Ok(())
        }

        fn load_launch_baseline(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<crate::domain::LaunchBaseline>, SnapshotError> {
            if self.baseline_load_error {
                return Err(SnapshotError::ReadFailed {
                    path: Utf8PathBuf::from("/tmp/baseline.json"),
                    source: std::io::Error::other("baseline read failed"),
                });
            }
            Ok(self.preloaded_baseline.borrow().clone())
        }
    }

    fn sample_topology() -> RuntimeTopologySnapshot {
        RuntimeTopologySnapshot {
            windows: vec![RuntimeWindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                layout: "tiled".to_owned(),
                active: true,
            }],
            panes: vec![RuntimePaneSnapshot {
                unit_id: UnitId::new("pane-0"),
                pane_id: Some(PaneId::new("%0").as_str().to_owned()),
                window_index: 0,
                pane_index: 0,
                cwd: Utf8PathBuf::from("/tmp/project"),
                title: Some("shell".to_owned()),
                current_command: Some("zsh".to_owned()),
                start_command: Some("zsh".to_owned()),
                active: true,
                dead: false,
                launch_origin: Some(PaneLaunchOrigin::EmptyShell),
                managed: true,
                tty: None,
                process_pid: None,
            }],
            active_window: Some(0),
            active_pane_runtime_id: Some("pane-0".to_owned()),
        }
    }

    fn sample_snapshot_context(state_key: Option<String>) -> SnapshotContext {
        SnapshotContext {
            instance_id: "a1b2".to_owned(),
            project: "proj".to_owned(),
            server_name: "srv".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            platform: "test".to_owned(),
            state_key,
        }
    }

    #[test]
    fn close_kill_with_snapshot_disabled_short_circuits_persistence() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::new();
        let cfg = SnapshotConfig {
            enabled: false,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(Some("a1b2@proj@main".to_owned()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Disabled
            }
        ));
        assert!(
            store.saved.borrow().is_empty(),
            "disabled mode must skip revision writes"
        );
        assert!(
            store.events.borrow().is_empty(),
            "disabled mode must skip event writes"
        );
        assert!(
            store.retention_calls.borrow().is_empty(),
            "disabled mode must skip retention processing"
        );
        assert_eq!(
            runtime.destroyed.borrow().len(),
            1,
            "session must still be destroyed"
        );
    }

    #[test]
    fn close_kill_with_missing_snapshot_state_key_still_destroys_session() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::new();
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(None);

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        match action {
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Failed { reason },
            } => {
                assert!(
                    reason.contains("state_key"),
                    "snapshot failure reason should identify the missing state key: {reason}"
                );
            }
            _ => panic!("expected failed snapshot status"),
        }
        assert_eq!(
            runtime.destroyed.borrow().as_slice(),
            &[("srv".to_owned(), "sess".to_owned())],
            "session must still be destroyed when snapshot state is unavailable"
        );
        assert!(
            store.saved.borrow().is_empty(),
            "missing state key must skip revision writes"
        );
    }

    #[test]
    fn close_kill_with_snapshot_enabled_persists_revision_event_and_retention() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::new();
        let cfg = SnapshotConfig {
            enabled: true,
            retain_full_revisions: 7,
            retain_event_days: 14,
            ..SnapshotConfig::default()
        };
        let state_key_raw = "a1b2@proj@main".to_owned();
        let ctx = sample_snapshot_context(Some(state_key_raw.clone()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Saved
            }
        ));
        assert_eq!(store.saved.borrow().len(), 1, "must persist one revision");
        let events = store.events.borrow();
        assert_eq!(events.len(), 1, "must append one close event");
        assert_eq!(events[0].kind, SnapshotEventKind::SessionClosing);
        assert_eq!(events[0].revision_id, Some(SnapshotRevisionId::first()));
        drop(events);
        assert_eq!(
            store.retention_calls.borrow().as_slice(),
            &[(7, 14)],
            "must apply retention with configured values"
        );
    }

    #[test]
    fn close_kill_with_snapshot_enabled_does_not_write_launch_baseline() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::new();
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(Some("a1b2@proj@main".to_owned()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Saved
            }
        ));
        assert_eq!(store.saved.borrow().len(), 1, "close still saves revision");
        assert!(
            store.baselines.borrow().is_empty(),
            "close intentionally does not refresh launch_baseline.json"
        );
    }

    /// Baseline freshness rule: when the live pane has returned to a shell and the
    /// prior baseline says the pane was a tool, the baseline is **stale**
    /// and must NOT hydrate the snapshot. Otherwise re-opening the
    /// workspace would silently re-launch a tool the user explicitly
    /// exited (the regression this test prevents).
    #[test]
    fn close_snapshot_skips_stale_tool_baseline_when_pane_exited_to_shell() {
        use crate::domain::{LaunchBaseline, LaunchBaselineEntry, PaneLaunchRef, ToolName};

        let topology = RuntimeTopologySnapshot {
            windows: vec![RuntimeWindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                layout: "tiled".to_owned(),
                active: true,
            }],
            panes: vec![RuntimePaneSnapshot {
                unit_id: UnitId::new("pane-0"),
                pane_id: Some(PaneId::new("%0").as_str().to_owned()),
                window_index: 0,
                pane_index: 0,
                cwd: Utf8PathBuf::from("/tmp/project"),
                title: Some("shell".to_owned()),
                current_command: Some("zsh".to_owned()),
                start_command: Some("zsh".to_owned()),
                active: true,
                dead: false,
                launch_origin: None,
                managed: true,
                tty: None,
                process_pid: None,
            }],
            active_window: Some(0),
            active_pane_runtime_id: Some("pane-0".to_owned()),
        };

        let baseline = LaunchBaseline {
            state_key: crate::domain::WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "proj".to_owned(),
                String::new(),
                "main".to_owned(),
            ),
            entries: vec![LaunchBaselineEntry {
                runtime_id: "pane-0".to_owned(),
                unit_id: "pane-0".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: ToolName::new("claude").expect("valid"),
                },
            }],
            updated_at: "100s".to_owned(),
        };

        let runtime = FakeRuntime::with_topology(topology);
        let store = FakeStore::with_baseline(baseline);
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(Some("a1b2@proj@main".to_owned()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Saved
            }
        ));

        let saved = store.saved.borrow();
        assert_eq!(saved.len(), 1);
        let pane = &saved[0].panes[0];
        assert!(
            matches!(
                &pane.launch_intent,
                crate::domain::PaneLaunchIntent::EmptyShell
            ),
            "freshness rule: stale tool baseline must NOT hydrate a pane \
             whose live state is a shell; got {:?}",
            pane.launch_intent,
        );
    }

    /// Companion to the freshness test above: when the pane is still
    /// running the tool live but the snapshot capture lost its
    /// `start_command`/`launch_origin` (so it would otherwise be
    /// `NonRestorable`), the prior baseline IS allowed to hydrate the
    /// pane. This is the legitimate "snapshot lost durable metadata"
    /// recovery path the original close-time hydration was added for.
    #[test]
    fn close_snapshot_hydrates_running_tool_from_prior_baseline() {
        use crate::domain::{LaunchBaseline, LaunchBaselineEntry, PaneLaunchRef, ToolName};

        let topology = RuntimeTopologySnapshot {
            windows: vec![RuntimeWindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                layout: "tiled".to_owned(),
                active: true,
            }],
            panes: vec![RuntimePaneSnapshot {
                unit_id: UnitId::new("pane-0"),
                pane_id: Some(PaneId::new("%0").as_str().to_owned()),
                window_index: 0,
                pane_index: 0,
                cwd: Utf8PathBuf::from("/tmp/project"),
                title: Some("claude".to_owned()),
                // Tool is still live in the foreground.
                current_command: Some("claude".to_owned()),
                // start_command lost — without baseline, snapshot would
                // be NonRestorable.
                start_command: None,
                active: true,
                dead: false,
                launch_origin: None,
                managed: true,
                tty: None,
                process_pid: None,
            }],
            active_window: Some(0),
            active_pane_runtime_id: Some("pane-0".to_owned()),
        };

        let baseline = LaunchBaseline {
            state_key: crate::domain::WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "proj".to_owned(),
                String::new(),
                "main".to_owned(),
            ),
            entries: vec![LaunchBaselineEntry {
                runtime_id: "pane-0".to_owned(),
                unit_id: "pane-0".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: ToolName::new("claude").expect("valid"),
                },
            }],
            updated_at: "100s".to_owned(),
        };

        let runtime = FakeRuntime::with_topology(topology);
        let store = FakeStore::with_baseline(baseline);
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(Some("a1b2@proj@main".to_owned()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Saved
            }
        ));

        let saved = store.saved.borrow();
        assert_eq!(saved.len(), 1);
        let pane = &saved[0].panes[0];
        assert!(
            matches!(
                &pane.launch_intent,
                crate::domain::PaneLaunchIntent::ToolRef { tool_name } if tool_name == "claude"
            ),
            "live tool pane with lost durable metadata must hydrate from baseline: got {:?}",
            pane.launch_intent,
        );
    }

    #[test]
    fn close_snapshot_reports_failed_when_prior_baseline_read_fails() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::with_baseline_load_error();
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(Some("a1b2@proj@main".to_owned()));

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close command should still return a structured action");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Failed { .. }
            }
        ));
        assert!(
            store.saved.borrow().is_empty(),
            "baseline read failures must not silently continue with a degraded snapshot"
        );
    }

    #[test]
    fn close_kill_with_invalid_state_key_reports_failed_without_writes() {
        let runtime = FakeRuntime::with_topology(sample_topology());
        let store = FakeStore::new();
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        let ctx = sample_snapshot_context(None);

        let action =
            close_session_with_config(&runtime, &runtime, &store, "srv", "sess", true, &cfg, &ctx)
                .expect("close should succeed even when persistence fails");

        assert!(matches!(
            action,
            CloseAction::Killed {
                snapshot_status: SnapshotPersistStatus::Failed { .. }
            }
        ));
        assert!(
            store.saved.borrow().is_empty() && store.events.borrow().is_empty(),
            "invalid state key must prevent snapshot writes"
        );
    }
}

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use super::*;
use crate::errors::TmuxError;
use crate::tmux::{
    AttachRequest, CreateSessionRequest, PaneInfo, PaneTopologyInfo, SendKeysRequest, SessionInfo,
    SplitPaneRequest, TmuxPort, WindowInfo,
};
use camino::Utf8PathBuf;
use workhive_core::domain::{
    PaneCapability, PaneDescriptor, PaneId, PaneKind, PaneRuntimeStatus, PaneState, ToolName,
    UnitId,
};
use workhive_core::ports::{
    BackendUnavailableReason, PaneRegistryPort, RegistryDelta, RegistryError,
};

// ── StubTmux ─────────────────────────────────────────────────────────────
//
// Minimal in-process stub for `TmuxPort`. Each `list_sessions` call returns
// the next error from the queue; all other methods are no-ops.

struct StubTmux {
    list_sessions_error: Option<TmuxError>,
    create_session_error: Option<TmuxError>,
    hook_calls: RefCell<Vec<HookCall>>,
    windows: Vec<WindowInfo>,
    topology_panes: Vec<PaneTopologyInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HookCall {
    server: String,
    session: String,
    hook_name: String,
    command: Option<String>,
}

impl StubTmux {
    fn returning_list_sessions_err(err: TmuxError) -> Self {
        Self {
            list_sessions_error: Some(err),
            create_session_error: None,
            hook_calls: RefCell::new(Vec::new()),
            windows: Vec::new(),
            topology_panes: Vec::new(),
        }
    }

    fn returning_create_session_err(err: TmuxError) -> Self {
        Self {
            list_sessions_error: None,
            create_session_error: Some(err),
            hook_calls: RefCell::new(Vec::new()),
            windows: Vec::new(),
            topology_panes: Vec::new(),
        }
    }

    fn always_ok() -> Self {
        Self {
            list_sessions_error: None,
            create_session_error: None,
            hook_calls: RefCell::new(Vec::new()),
            windows: Vec::new(),
            topology_panes: Vec::new(),
        }
    }

    fn with_topology(windows: Vec<WindowInfo>, panes: Vec<PaneTopologyInfo>) -> Self {
        Self {
            list_sessions_error: None,
            create_session_error: None,
            hook_calls: RefCell::new(Vec::new()),
            windows,
            topology_panes: panes,
        }
    }
}

impl TmuxPort for StubTmux {
    fn list_sessions(&self, _group: &str) -> Result<Vec<SessionInfo>, TmuxError> {
        if let Some(ref e) = self.list_sessions_error {
            return Err(clone_tmux_error(e));
        }
        Ok(Vec::new())
    }

    fn create_session(&self, _req: CreateSessionRequest) -> Result<(), TmuxError> {
        if let Some(ref e) = self.create_session_error {
            return Err(clone_tmux_error(e));
        }
        Ok(())
    }

    fn has_session(&self, _group: &str, _name: &str) -> Result<bool, TmuxError> {
        Ok(false)
    }

    fn configure_session(&self, _group: &str, _name: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    fn attach(&self, _req: AttachRequest) -> Result<(), TmuxError> {
        Ok(())
    }

    fn detach_client(&self, _group: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    fn kill_session(&self, _group: &str, _name: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    fn kill_server(&self, _group: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    fn split_pane(&self, _req: SplitPaneRequest) -> Result<PaneInfo, TmuxError> {
        unimplemented!("not needed for these tests")
    }

    fn close_pane(&self, _group: &str, _pane_id: &PaneId) -> Result<(), TmuxError> {
        Ok(())
    }

    fn list_panes(&self, _group: &str, _workspace_name: &str) -> Result<Vec<PaneInfo>, TmuxError> {
        Ok(Vec::new())
    }

    fn current_pane(&self, _group: &str) -> Result<PaneInfo, TmuxError> {
        unimplemented!("not needed for these tests")
    }

    fn send_keys(&self, _req: SendKeysRequest) -> Result<(), TmuxError> {
        Ok(())
    }

    fn set_pane_title(
        &self,
        _server: &str,
        _pane_id: &PaneId,
        _title: &str,
    ) -> Result<(), TmuxError> {
        Ok(())
    }

    fn list_windows(&self, _server: &str, _session: &str) -> Result<Vec<WindowInfo>, TmuxError> {
        Ok(self.windows.clone())
    }

    fn list_panes_topology(
        &self,
        _server: &str,
        _session: &str,
    ) -> Result<Vec<PaneTopologyInfo>, TmuxError> {
        Ok(self.topology_panes.clone())
    }

    fn create_window(
        &self,
        _server: &str,
        _session: &str,
        _window_index: u32,
        _name: Option<&str>,
        _cwd: &camino::Utf8Path,
    ) -> Result<PaneTopologyInfo, TmuxError> {
        Err(TmuxError::CommandFailed {
            message: "not implemented in stub".to_owned(),
        })
    }

    fn apply_layout(
        &self,
        _server: &str,
        _session: &str,
        _window_index: u32,
        _layout: &str,
    ) -> Result<(), TmuxError> {
        Ok(())
    }

    fn select_window(
        &self,
        _server: &str,
        _session: &str,
        _window_index: u32,
    ) -> Result<(), TmuxError> {
        Ok(())
    }

    fn select_pane(&self, _server: &str, _pane_id: &PaneId) -> Result<(), TmuxError> {
        Ok(())
    }

    fn respawn_pane(
        &self,
        _server: &str,
        _pane_id: &PaneId,
        _cwd: &camino::Utf8Path,
    ) -> Result<(), TmuxError> {
        Ok(())
    }

    fn respawn_pane_with_command(
        &self,
        _server: &str,
        _pane_id: &PaneId,
        _cwd: &camino::Utf8Path,
        _command: &str,
    ) -> Result<(), TmuxError> {
        Ok(())
    }

    fn set_hook(
        &self,
        server: &str,
        session: &str,
        hook_name: &str,
        command: Option<&str>,
    ) -> Result<(), TmuxError> {
        self.hook_calls.borrow_mut().push(HookCall {
            server: server.to_owned(),
            session: session.to_owned(),
            hook_name: hook_name.to_owned(),
            command: command.map(str::to_owned),
        });
        Ok(())
    }

    fn capture_pane_text(
        &self,
        _request: crate::tmux::CapturePaneRequest,
    ) -> Result<String, TmuxError> {
        // StubTmux is used by runtime composition tests that do not exercise
        // the terminal-status probe path. Return an empty buffer so any
        // accidental call downgrades to "no signal" rather than panicking.
        Ok(String::new())
    }
}

fn clone_tmux_error(e: &TmuxError) -> TmuxError {
    match e {
        TmuxError::TmuxNotFound => TmuxError::TmuxNotFound,
        TmuxError::ServerNotRunning { server } => TmuxError::ServerNotRunning {
            server: server.clone(),
        },
        TmuxError::SocketNotFound { path } => TmuxError::SocketNotFound { path: path.clone() },
        TmuxError::PermissionDenied { detail } => TmuxError::PermissionDenied {
            detail: detail.clone(),
        },
        TmuxError::SessionNotFound { name } => TmuxError::SessionNotFound { name: name.clone() },
        TmuxError::SessionAlreadyExists { name } => {
            TmuxError::SessionAlreadyExists { name: name.clone() }
        }
        TmuxError::SessionCreateFailed { .. } => {
            // io::Error is not Clone; use CommandFailed as a substitute for
            // the clone_tmux_error helper (this arm is not exercised in these tests)
            TmuxError::CommandFailed {
                message: "session create failed (cloned)".to_owned(),
            }
        }
        TmuxError::SessionCreateNotVerified { server, name } => {
            TmuxError::SessionCreateNotVerified {
                server: server.clone(),
                name: name.clone(),
            }
        }
        TmuxError::PaneNotFound { id } => TmuxError::PaneNotFound { id: id.clone() },
        TmuxError::LastPaneCloseRejected => TmuxError::LastPaneCloseRejected,
        TmuxError::CommandFailed { message } => TmuxError::CommandFailed {
            message: message.clone(),
        },
    }
}

// ── Regression: ServerNotRunning → BackendUnavailable ────────────────────

#[test]
fn list_workspaces_server_not_running_maps_to_backend_unavailable() {
    let stub = StubTmux::returning_list_sessions_err(TmuxError::ServerNotRunning {
        server: "whsrv-myproject".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("whsrv-myproject");
    assert!(result.is_err(), "expected error from list_workspaces");
    match result.unwrap_err() {
        RuntimeError::BackendUnavailable { detail, reason } => {
            assert!(
                detail.contains("whsrv-myproject"),
                "detail should contain server name, got: {detail}"
            );
            assert!(
                matches!(reason, BackendUnavailableReason::ServerNotRunning { .. }),
                "ServerNotRunning TmuxError should produce typed reason; got {reason:?}"
            );
        }
        other => panic!(
            "list_workspaces ServerNotRunning must map to BackendUnavailable, got: {other:?}"
        ),
    }
}

#[test]
fn list_workspaces_tmux_not_found_maps_to_backend_unavailable() {
    let stub = StubTmux::returning_list_sessions_err(TmuxError::TmuxNotFound);
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("any-group");
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), RuntimeError::BackendUnavailable { .. }),
        "TmuxNotFound must map to BackendUnavailable"
    );
}

#[test]
fn list_workspaces_socket_not_found_maps_to_backend_unavailable() {
    let stub = StubTmux::returning_list_sessions_err(TmuxError::SocketNotFound {
        path: "/run/tmux/default".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("any-group");
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), RuntimeError::BackendUnavailable { .. }),
        "SocketNotFound must map to BackendUnavailable"
    );
}

#[test]
fn list_workspaces_session_not_found_maps_to_workspace_not_found() {
    // SessionNotFound during list_sessions is unusual but exercises that
    // the SessionNotFound → WorkspaceNotFound mapping is still in place.
    let stub = StubTmux::returning_list_sessions_err(TmuxError::SessionNotFound {
        name: "wh--proj--feat-a".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("any-group");
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), RuntimeError::WorkspaceNotFound { .. }),
        "SessionNotFound must map to WorkspaceNotFound"
    );
}

#[test]
fn list_workspaces_permission_denied_maps_correctly() {
    let stub = StubTmux::returning_list_sessions_err(TmuxError::PermissionDenied {
        detail: "socket permission denied".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("any-group");
    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), RuntimeError::PermissionDenied { .. }),
        "PermissionDenied must map to RuntimeError::PermissionDenied"
    );
}

#[test]
fn list_workspaces_ok_returns_info_list() {
    let stub = StubTmux::always_ok();
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.list_workspaces("any-group");
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ── Core contract parity: BackendUnavailable folds into BackendUnavailable ─
//
// This test proves end-to-end that when TmuxRuntimeAdapter::list_workspaces
// encounters ServerNotRunning, the result is exactly the RuntimeError variant
// that resolve_runtime_session_state absorbs as RuntimeSessionState::BackendUnavailable.
// It is the regression guard for that mapping.

#[test]
fn server_not_running_satisfies_core_project_state_contract() {
    use workhive_core::domain::RuntimeSessionState;
    use workhive_core::ports::WorkspaceRuntimeInfo;

    // Simulate what resolve_runtime_session_state does with the adapter result.
    let stub = StubTmux::returning_list_sessions_err(TmuxError::ServerNotRunning {
        server: "whsrv-myproject".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let runtime_result: Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> =
        adapter.list_workspaces("whsrv-myproject");

    // Mirror the logic in resolve_runtime_session_state:
    let session_state = match runtime_result {
        Ok(workspaces) if workspaces.is_empty() => RuntimeSessionState::NoSession,
        Ok(_) => RuntimeSessionState::HasSessions,
        Err(RuntimeError::BackendUnavailable { .. }) => RuntimeSessionState::BackendUnavailable,
        Err(e) => panic!("unexpected error propagated to core: {e:?}"),
    };

    assert_eq!(
        session_state,
        RuntimeSessionState::BackendUnavailable,
        "ServerNotRunning must resolve to BackendUnavailable session state"
    );
}

// ── create_workspace error mapping ───────────────────────────────────────

#[test]
fn create_workspace_server_not_running_maps_to_backend_unavailable() {
    // When creating a workspace and the server is not running, the adapter
    // should also surface BackendUnavailable (consistent mapping).
    let stub = StubTmux::returning_create_session_err(TmuxError::ServerNotRunning {
        server: "whsrv-myproject".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.create_workspace(workhive_core::ports::CreateWorkspaceRuntimeRequest {
        group: "whsrv-myproject".to_owned(),
        workspace_name: "feat-a".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/feat-a"),
        env: Vec::new(),
    });

    assert!(result.is_err());
    assert!(
        matches!(result.unwrap_err(), RuntimeError::BackendUnavailable { .. }),
        "create_workspace ServerNotRunning must map to BackendUnavailable"
    );
}

#[test]
fn create_workspace_not_verified_maps_to_operation_failed() {
    let stub = StubTmux::returning_create_session_err(TmuxError::SessionCreateNotVerified {
        server: "whsrv-myproject".to_owned(),
        name: "feat-phantom".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.create_workspace(workhive_core::ports::CreateWorkspaceRuntimeRequest {
        group: "whsrv-myproject".to_owned(),
        workspace_name: "feat-phantom".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/feat-phantom"),
        env: Vec::new(),
    });

    assert!(result.is_err());
    match result.unwrap_err() {
        RuntimeError::OperationFailed { message } => {
            assert!(
                message.contains("feat-phantom"),
                "error should reference the session name: {message}"
            );
            assert!(
                message.contains("false success"),
                "error should mention false success: {message}"
            );
        }
        other => panic!("SessionCreateNotVerified must map to OperationFailed, got: {other:?}"),
    }
}

#[test]
fn create_workspace_duplicate_session_maps_to_workspace_already_exists() {
    let stub = StubTmux::returning_create_session_err(TmuxError::SessionAlreadyExists {
        name: "feat-a".to_owned(),
    });
    let adapter = TmuxRuntimeAdapter::new(&stub);

    let result = adapter.create_workspace(workhive_core::ports::CreateWorkspaceRuntimeRequest {
        group: "whsrv-myproject".to_owned(),
        workspace_name: "feat-a".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/feat-a"),
        env: Vec::new(),
    });

    assert!(result.is_err());
    match result.unwrap_err() {
        RuntimeError::WorkspaceAlreadyExists { workspace } => {
            assert_eq!(workspace, "feat-a");
        }
        other => panic!(
            "create_workspace duplicate session must map to WorkspaceAlreadyExists, got: {other:?}"
        ),
    }
}

struct StubRegistry {
    descriptors: HashMap<String, PaneDescriptor>,
}

impl StubRegistry {
    fn with_descriptor(descriptor: PaneDescriptor) -> Self {
        let mut descriptors = HashMap::new();
        descriptors.insert(descriptor.runtime_id.as_str().to_owned(), descriptor);
        Self { descriptors }
    }
}

impl PaneRegistryPort for StubRegistry {
    fn upsert(&self, _descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
        Err(RegistryError::Internal {
            message: "not implemented in stub".to_owned(),
        })
    }

    fn remove(
        &self,
        _id: &workhive_core::domain::PaneRuntimeId,
    ) -> Result<Option<PaneDescriptor>, RegistryError> {
        Ok(None)
    }

    fn get(
        &self,
        id: &workhive_core::domain::PaneRuntimeId,
    ) -> Result<Option<PaneDescriptor>, RegistryError> {
        Ok(self.descriptors.get(id.as_str()).cloned())
    }

    fn list_workspace(&self, _workspace: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(Vec::new())
    }

    fn list(&self, _workspace: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(Vec::new())
    }

    fn drain_events(
        &self,
    ) -> Result<Vec<workhive_core::domain::PaneLifecycleEvent>, RegistryError> {
        Ok(Vec::new())
    }

    fn update_from_observation(
        &self,
        _observed: workhive_core::domain::ObservedPaneState,
    ) -> Result<Vec<workhive_core::domain::PaneStateChange>, RegistryError> {
        Ok(Vec::new())
    }

    fn find_by_capability(
        &self,
        _capability: workhive_core::domain::PaneCapability,
    ) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(Vec::new())
    }
}

#[test]
fn capture_topology_prefers_registry_launch_origin_over_start_command() {
    let panes = vec![PaneTopologyInfo {
        id: PaneId::new("%1"),
        window_index: 0,
        pane_index: 0,
        cwd: Utf8PathBuf::from("/tmp/proj"),
        current_command: "zsh".to_owned(),
        start_command: Some("zsh".to_owned()),
        title: None,
        active: true,
        dead: false,
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }];
    let windows = vec![WindowInfo {
        index: 0,
        name: Some("main".to_owned()),
        active: true,
        layout: "tiled".to_owned(),
    }];
    let stub = StubTmux::with_topology(windows, panes);

    let runtime_id = workhive_core::domain::PaneRuntimeId::derive("srv", "sess", "%1");
    let registry = StubRegistry::with_descriptor(PaneDescriptor {
        runtime_id,
        group: "srv".to_owned(),
        workspace: "sess".to_owned(),
        unit_id: UnitId::new("%1"),
        cwd: "/tmp/proj".to_owned(),
        command: "claude".to_owned(),
        kind: PaneKind::Agent,
        state: PaneState::Running,
        observed_status: PaneRuntimeStatus::Running,
        role: None,
        launch_origin: PaneLaunchOrigin::Tool {
            name: ToolName::new("claude").expect("valid tool name"),
        },
        capabilities: BTreeSet::from([PaneCapability::CommandExecutor]),
    });

    let adapter = TmuxRuntimeAdapter::with_pane_registry(&stub, &registry);
    let topology = adapter.capture_topology("srv", "sess").expect("capture");
    assert_eq!(topology.panes.len(), 1);
    match topology.panes[0]
        .launch_origin
        .as_ref()
        .expect("launch origin must exist")
    {
        PaneLaunchOrigin::Tool { name } => assert_eq!(name.as_str(), "claude"),
        other => panic!("expected registry tool launch origin, got {other:?}"),
    }
}

#[test]
fn configure_snapshot_hooks_install_sets_expected_notify_commands() {
    let stub = StubTmux::always_ok();
    let adapter = TmuxRuntimeAdapter::new(&stub);

    adapter
        .configure_snapshot_hooks(
            "srv",
            "sess",
            SnapshotHookMode::Install,
            "workhive internal snapshot notify --state-key key",
        )
        .expect("install hooks should succeed");

    let calls = stub.hook_calls.borrow();
    assert_eq!(calls.len(), 8, "expected eight hook install calls");
    for call in calls.iter() {
        let cmd = call
            .command
            .as_ref()
            .expect("install hook must have command");
        assert!(
            cmd.starts_with("run-shell -b '"),
            "hook body must be a valid tmux command starting with run-shell -b, got: {cmd}"
        );
        assert!(
            cmd.contains("--snapshot-event"),
            "hook body must include --snapshot-event, got: {cmd}"
        );
    }
    let hook_names = calls
        .iter()
        .map(|call| call.hook_name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for required in [
        "after-new-window",
        "after-kill-window",
        "after-split-window",
        "after-kill-pane",
        "after-select-pane",
        "window-layout-changed",
        "pane-died",
        "pane-exited",
    ] {
        assert!(
            hook_names.contains(required),
            "missing required hook registration: {required}"
        );
    }
}

#[test]
fn configure_snapshot_hooks_remove_clears_registered_hooks() {
    let stub = StubTmux::always_ok();
    let adapter = TmuxRuntimeAdapter::new(&stub);

    adapter
        .configure_snapshot_hooks("srv", "sess", SnapshotHookMode::Remove, "unused")
        .expect("remove hooks should succeed");

    let calls = stub.hook_calls.borrow();
    assert_eq!(calls.len(), 8, "expected eight hook remove calls");
    assert!(
        calls.iter().all(|call| call.command.is_none()),
        "remove mode must clear hook command payload"
    );
}

#[test]
fn render_hook_run_shell_wraps_correctly() {
    let result = render_hook_run_shell("workhive notify --key x");
    assert_eq!(result, "run-shell -b 'workhive notify --key x'");
}

#[test]
fn render_hook_run_shell_escapes_single_quotes() {
    let result = render_hook_run_shell("echo 'hello'");
    assert_eq!(result, "run-shell -b 'echo '\\''hello'\\'''");
}

/// Item 4 regression guard: a shell-looking runtime start command must
/// not silently fabricate `PaneLaunchOrigin::EmptyShell`. The pane
/// registry / launch baseline is the only authoritative source of
/// `EmptyShell`; `derive_launch_origin` returning `None` leaves callers
/// free to keep an earlier, well-known tool identity.
#[test]
fn derive_launch_origin_returns_none_for_shell_start_command() {
    for shell in ["zsh", "bash", "sh", "fish", "nushell"] {
        assert!(
            derive_launch_origin(Some(shell), Some(shell)).is_none(),
            "shell start '{shell}' must not be promoted to a launch origin"
        );
    }
}

/// `derive_launch_origin` still recognizes known tools from start
/// commands so first-time capture (before the registry has an entry)
/// is not blinded.
#[test]
fn derive_launch_origin_recognizes_known_tool_start_command() {
    let origin =
        derive_launch_origin(Some("claude"), Some("claude")).expect("known tool must resolve");
    match origin {
        PaneLaunchOrigin::Tool { name } => assert_eq!(name.as_str(), "claude"),
        other => panic!("expected Tool origin, got {other:?}"),
    }
}

/// Regression guard: an internal supervisor wrapper with a
/// recoverable `--tool <name>` must round-trip back to a Tool origin
/// at the runtime adapter boundary, not a user `InlineCommand`. This
/// keeps `workhive workspace open` from rebroadcasting the wrapper
/// argv via visible `send_input` on the next cycle.
#[test]
fn derive_launch_origin_recovers_tool_from_supervisor_wrapper() {
    let wrapper = "workhive internal pane-supervisor \
            --runtime-id wh@i@p@w/%1 --tool codex --shell zsh -- codex --resume";
    let origin = derive_launch_origin(Some(wrapper), None)
        .expect("supervisor wrapper with --tool must restore as Tool");
    match origin {
        PaneLaunchOrigin::Tool { name } => assert_eq!(name.as_str(), "codex"),
        other => panic!("expected Tool origin, got {other:?}"),
    }
}

/// Regression guard: a malformed supervisor wrapper that
/// cannot recover `--tool` must collapse to `None` so prior
/// authoritative identity (registry / hydrated baseline) wins, and
/// must NEVER be promoted to a replayable `InlineCommand`.
#[test]
fn derive_launch_origin_collapses_malformed_supervisor_wrapper_to_none() {
    let malformed = "workhive internal pane-supervisor --runtime-id wh@i@p@w/%1 --shell zsh";
    assert!(
        derive_launch_origin(Some(malformed), None).is_none(),
        "malformed supervisor wrapper must not become a replayable InlineCommand"
    );
}

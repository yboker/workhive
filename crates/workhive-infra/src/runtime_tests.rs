use std::cell::RefCell;

use super::*;
use crate::tmux::{
    AttachRequest, CreateSessionRequest, PaneInfo, PaneTopologyInfo, SendKeysRequest, SessionInfo,
    SplitPaneRequest, TmuxPort, WindowInfo,
};
use crate::tmux_errors::TmuxError;
use camino::Utf8PathBuf;
use workhive_core::domain::{PaneId, RuntimeBinding};
use workhive_core::ports::BackendUnavailableReason;

#[test]
fn runtime_assigned_unit_identity_overrides_inherited_workhive_identity() {
    let actual = runtime_unit_id_from(|name| match name {
        "HERDR_PANE_ID" => Some("opaque-runtime-pane".to_owned()),
        "WORKHIVE_UNIT_ID" => Some("stale-parent-pane".to_owned()),
        _ => None,
    });

    assert_eq!(actual.as_deref(), Some("opaque-runtime-pane"));
}

// ── StubTmux ─────────────────────────────────────────────────────────────
//
// Minimal in-process stub for `TmuxPort`. Each `list_sessions` call returns
// the next error from the queue; all other methods are no-ops.

struct StubTmux {
    list_sessions_error: Option<TmuxError>,
    create_session_error: Option<TmuxError>,
    configure_error: RefCell<Option<TmuxError>>,
    switch_error: RefCell<Option<TmuxError>>,
    rename_error: RefCell<Option<TmuxError>>,
    sessions: RefCell<Vec<(String, String)>>,
    attach_events: RefCell<Vec<&'static str>>,
    configure_records: RefCell<Vec<ConfigureRecord>>,
}

#[derive(Debug, Clone)]
struct ConfigureRecord {
    group: String,
    session: String,
    policy: workhive_core::ports::RuntimeInteractionPolicy,
}

impl Default for StubTmux {
    fn default() -> Self {
        Self {
            list_sessions_error: None,
            create_session_error: None,
            configure_error: RefCell::new(None),
            switch_error: RefCell::new(None),
            rename_error: RefCell::new(None),
            sessions: RefCell::new(Vec::new()),
            attach_events: RefCell::new(Vec::new()),
            configure_records: RefCell::new(Vec::new()),
        }
    }
}

impl StubTmux {
    fn returning_list_sessions_err(err: TmuxError) -> Self {
        Self {
            list_sessions_error: Some(err),
            ..Default::default()
        }
    }

    fn returning_create_session_err(err: TmuxError) -> Self {
        Self {
            create_session_error: Some(err),
            ..Default::default()
        }
    }

    fn always_ok() -> Self {
        Self::default()
    }

    fn with_sessions(sessions: &[(&str, &str)]) -> Self {
        let stub = Self::default();
        stub.sessions.borrow_mut().extend(
            sessions
                .iter()
                .map(|(server, session)| ((*server).to_owned(), (*session).to_owned())),
        );
        stub
    }

    fn returning_rename_err(err: TmuxError) -> Self {
        let stub = Self::default();
        *stub.rename_error.borrow_mut() = Some(err);
        stub
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

    fn has_session(&self, group: &str, name: &str) -> Result<bool, TmuxError> {
        Ok(self
            .sessions
            .borrow()
            .iter()
            .any(|(server, session)| server == group && session == name))
    }

    fn configure_session(
        &self,
        group: &str,
        name: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), TmuxError> {
        self.attach_events.borrow_mut().push("configure");
        self.configure_records.borrow_mut().push(ConfigureRecord {
            group: group.to_owned(),
            session: name.to_owned(),
            policy: *policy,
        });
        if let Some(err) = self.configure_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(())
    }

    fn attach(&self, _req: AttachRequest) -> Result<(), TmuxError> {
        self.attach_events.borrow_mut().push("attach");
        Ok(())
    }

    fn switch_client(&self, _group: &str, _target: &str) -> Result<(), TmuxError> {
        self.attach_events.borrow_mut().push("switch_client");
        if let Some(err) = self.switch_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(())
    }

    fn detach_client(&self, _group: &str, _session: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    fn rename_session(&self, server: &str, session: &str, new_name: &str) -> Result<(), TmuxError> {
        if let Some(ref error) = *self.rename_error.borrow() {
            return Err(clone_tmux_error(error));
        }
        let mut sessions = self.sessions.borrow_mut();
        if let Some(existing) = sessions
            .iter_mut()
            .find(|(existing_server, existing_name)| {
                existing_server == server && existing_name == session
            })
        {
            existing.1 = new_name.to_owned();
        }
        Ok(())
    }

    fn kill_session(&self, group: &str, name: &str) -> Result<(), TmuxError> {
        self.sessions
            .borrow_mut()
            .retain(|(server, session)| !(server == group && session == name));
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

    fn current_pane(&self, _group: &str, _session: &str) -> Result<PaneInfo, TmuxError> {
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
        Ok(Vec::new())
    }

    fn list_panes_topology(
        &self,
        _server: &str,
        _session: &str,
    ) -> Result<Vec<PaneTopologyInfo>, TmuxError> {
        Ok(Vec::new())
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
        _server: &str,
        _session: &str,
        _hook_name: &str,
        _command: Option<&str>,
    ) -> Result<(), TmuxError> {
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

#[test]
fn attach_refreshes_managed_tmux_options_before_connecting() {
    let stub = StubTmux::always_ok();
    let adapter = TmuxRuntimeAdapter::new(&stub);

    adapter
        .attach_workspace(workhive_core::ports::AttachWorkspaceRequest {
            group: "whsrv-demo".to_owned(),
            workspace_name: "wh@demo@main".to_owned(),
            interaction_policy: workhive_core::ports::RuntimeInteractionPolicy::default(),
        })
        .expect("attach should succeed");

    assert_eq!(*stub.attach_events.borrow(), vec!["configure", "attach"]);
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

// ── Switch interaction policy tests ──────────────────────────────────

#[test]
fn tmux_switch_policy_configures_before_switch() {
    let tmux = StubTmux::default();
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let policy = workhive_core::ports::RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    adapter
        .switch_workspace("whsrv-inst-proj", "wh@inst@proj@feat", &policy)
        .expect("switch must succeed");
    let events = tmux.attach_events.borrow();
    assert_eq!(
        &*events,
        &["configure", "switch_client"],
        "configure must precede switch_client"
    );
}

#[test]
fn tmux_switch_policy_configuration_failure_stops_switch() {
    let tmux = StubTmux {
        configure_error: RefCell::new(Some(TmuxError::CommandFailed {
            message: "simulated config failure".to_owned(),
        })),
        ..Default::default()
    };
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let policy = workhive_core::ports::RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let result = adapter.switch_workspace("whsrv-inst-proj", "wh@inst@proj@feat", &policy);
    assert!(result.is_err(), "configuration failure must propagate");
    let events = tmux.attach_events.borrow();
    assert_eq!(
        &*events,
        &["configure"],
        "switch_client must not be called after configure failure"
    );
}

#[test]
fn tmux_switch_policy_switch_failure_propagates() {
    let tmux = StubTmux {
        switch_error: RefCell::new(Some(TmuxError::CommandFailed {
            message: "simulated switch failure".to_owned(),
        })),
        ..Default::default()
    };
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let policy = workhive_core::ports::RuntimeInteractionPolicy::default();
    let result = adapter.switch_workspace("whsrv-inst-proj", "wh@inst@proj@feat", &policy);
    assert!(result.is_err(), "switch failure must propagate");
    let events = tmux.attach_events.borrow();
    assert_eq!(
        &*events,
        &["configure", "switch_client"],
        "both configure and switch were attempted"
    );
}

#[test]
fn tmux_runtime_promotion_renames_staged_session_and_verifies_both_names() {
    let tmux = StubTmux::with_sessions(&[("whsrv", "staged"), ("other", "kept")]);
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let staged = RuntimeBinding::tmux("whsrv", "staged");
    let final_binding = RuntimeBinding::tmux("whsrv", "canonical");

    adapter
        .rename_workspace_binding(&staged, &final_binding)
        .expect("staged session should be promoted");

    assert!(!tmux.has_session("whsrv", "staged").unwrap());
    assert!(tmux.has_session("whsrv", "canonical").unwrap());
    assert!(tmux.has_session("other", "kept").unwrap());
}

#[test]
fn tmux_runtime_promotion_rejects_canonical_name_collision_before_rename() {
    let tmux = StubTmux::with_sessions(&[("whsrv", "staged"), ("whsrv", "canonical")]);
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let result = adapter.rename_workspace_binding(
        &RuntimeBinding::tmux("whsrv", "staged"),
        &RuntimeBinding::tmux("whsrv", "canonical"),
    );

    assert!(matches!(
        result,
        Err(RuntimeError::WorkspaceAlreadyExists { workspace }) if workspace == "canonical"
    ));
    assert!(tmux.has_session("whsrv", "staged").unwrap());
    assert!(tmux.has_session("whsrv", "canonical").unwrap());
}

#[test]
fn tmux_runtime_promotion_propagates_rename_failure_without_losing_staged_session() {
    let tmux = StubTmux::returning_rename_err(TmuxError::CommandFailed {
        message: "rename denied".to_owned(),
    });
    tmux.sessions
        .borrow_mut()
        .push(("whsrv".to_owned(), "staged".to_owned()));
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let result = adapter.rename_workspace_binding(
        &RuntimeBinding::tmux("whsrv", "staged"),
        &RuntimeBinding::tmux("whsrv", "canonical"),
    );

    assert!(matches!(result, Err(RuntimeError::OperationFailed { .. })));
    assert!(tmux.has_session("whsrv", "staged").unwrap());
    assert!(!tmux.has_session("whsrv", "canonical").unwrap());
}

#[test]
fn tmux_switch_policy_transports_mouse_and_clipboard_values() {
    let tmux = StubTmux::default();
    let adapter = TmuxRuntimeAdapter::new(&tmux);
    let policy = workhive_core::ports::RuntimeInteractionPolicy {
        mouse_enabled: false,
        clipboard_enabled: true,
    };
    adapter
        .switch_workspace("whsrv-inst-proj", "wh@inst@proj@feat", &policy)
        .expect("switch must succeed");

    let events = tmux.attach_events.borrow();
    assert_eq!(
        &*events,
        &["configure", "switch_client"],
        "configure must precede switch_client"
    );

    let records = tmux.configure_records.borrow();
    assert_eq!(records.len(), 1, "exactly one configure call expected");
    let rec = &records[0];
    assert_eq!(rec.group, "whsrv-inst-proj");
    assert_eq!(rec.session, "wh@inst@proj@feat");
    assert!(
        !rec.policy.mouse_enabled,
        "mouse_enabled must be transported as false"
    );
    assert!(
        rec.policy.clipboard_enabled,
        "clipboard_enabled must be transported as true"
    );
}

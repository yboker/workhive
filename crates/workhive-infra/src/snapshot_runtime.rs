//! Snapshot runtime adapter backed by tmux.
//!
//! Bridges `TmuxPort` primitives to the runtime-neutral `SnapshotRuntimePort`.

use std::sync::Arc;

use camino::Utf8Path;

use workhive_core::domain::{PaneDirection, PaneId, PaneLaunchOrigin, UnitId};
use workhive_core::ports::{
    RuntimeError, RuntimePaneSnapshot, RuntimeTopologySnapshot, RuntimeWindowSnapshot,
    SnapshotHookMode, SnapshotRuntimePort, SplitPanePreparation,
};
use workhive_core::usecases::runtime_start_command::{
    classification_to_launch_origin, classify_start_command,
};

use crate::errors::TmuxError;
use crate::tmux::{SplitPaneRequest, TmuxPort};

/// tmux-backed implementation of snapshot runtime operations.
pub struct TmuxSnapshotRuntimeAdapter {
    tmux: Arc<dyn TmuxPort>,
}

impl TmuxSnapshotRuntimeAdapter {
    /// Create a new adapter from a shared tmux port.
    pub fn new(tmux: Arc<dyn TmuxPort>) -> Self {
        Self { tmux }
    }
}

impl SnapshotRuntimePort for TmuxSnapshotRuntimeAdapter {
    fn capture_topology(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeTopologySnapshot, RuntimeError> {
        let windows = self
            .tmux
            .list_windows(group, workspace_name)
            .map_err(map_tmux_error)?
            .into_iter()
            .map(|window| RuntimeWindowSnapshot {
                index: window.index,
                name: window.name,
                layout: window.layout,
                active: window.active,
            })
            .collect::<Vec<_>>();

        let panes = self
            .tmux
            .list_panes_topology(group, workspace_name)
            .map_err(map_tmux_error)?
            .into_iter()
            .map(|pane| RuntimePaneSnapshot {
                unit_id: UnitId::new(pane.id.as_str().to_owned()),
                pane_id: Some(pane.id.as_str().to_owned()),
                window_index: pane.window_index,
                pane_index: pane.pane_index,
                cwd: pane.cwd,
                title: pane.title,
                current_command: Some(pane.current_command.clone()),
                start_command: pane.start_command.clone(),
                active: pane.active,
                dead: pane.dead,
                launch_origin: derive_launch_origin(
                    pane.start_command.as_deref(),
                    Some(pane.current_command.as_str()),
                ),
                managed: true,
                tty: pane.tty,
                process_pid: pane.process_pid,
            })
            .collect::<Vec<_>>();

        let active_window = windows.iter().find(|w| w.active).map(|w| w.index);
        let active_pane_runtime_id = panes
            .iter()
            .find(|p| p.active)
            .map(|p| p.unit_id.as_str().to_owned());

        Ok(RuntimeTopologySnapshot {
            windows,
            panes,
            active_window,
            active_pane_runtime_id,
        })
    }

    fn create_window(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
        name: Option<&str>,
        cwd: &Utf8Path,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        let pane = self
            .tmux
            .create_window(group, workspace_name, window_index, name, cwd)
            .map_err(map_tmux_error)?;
        Ok(RuntimePaneSnapshot {
            unit_id: UnitId::new(pane.id.as_str().to_owned()),
            pane_id: Some(pane.id.as_str().to_owned()),
            window_index: pane.window_index,
            pane_index: pane.pane_index,
            cwd: pane.cwd,
            title: pane.title,
            current_command: Some(pane.current_command.clone()),
            start_command: pane.start_command.clone(),
            active: pane.active,
            dead: pane.dead,
            launch_origin: derive_launch_origin(
                pane.start_command.as_deref(),
                Some(pane.current_command.as_str()),
            ),
            managed: true,
            tty: pane.tty,
            process_pid: pane.process_pid,
        })
    }

    fn split_pane(
        &self,
        group: &str,
        workspace_name: &str,
        target_unit: &UnitId,
        direction: PaneDirection,
        prep: SplitPanePreparation,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        let target_pane = PaneId::new(target_unit.as_str().to_owned());
        let pane = self
            .tmux
            .split_pane(SplitPaneRequest {
                server_name: group.to_owned(),
                session_name: workspace_name.to_owned(),
                target_pane: Some(target_pane),
                direction,
                command: prep.command,
                size_percent: None,
                env: Vec::new(),
                cwd: prep.cwd,
            })
            .map_err(map_tmux_error)?;

        let detailed = self
            .tmux
            .list_panes_topology(group, workspace_name)
            .map_err(map_tmux_error)?
            .into_iter()
            .find(|candidate| candidate.id.as_str() == pane.id.as_str())
            .ok_or_else(|| RuntimeError::OperationFailed {
                message: format!(
                    "split pane succeeded but topology lookup missing pane '{}'",
                    pane.id
                ),
            })?;

        Ok(RuntimePaneSnapshot {
            unit_id: UnitId::new(detailed.id.as_str().to_owned()),
            pane_id: Some(detailed.id.as_str().to_owned()),
            window_index: detailed.window_index,
            pane_index: detailed.pane_index,
            cwd: detailed.cwd,
            title: detailed.title,
            current_command: Some(detailed.current_command.clone()),
            start_command: detailed.start_command.clone(),
            active: detailed.active,
            dead: detailed.dead,
            launch_origin: derive_launch_origin(
                detailed.start_command.as_deref(),
                Some(detailed.current_command.as_str()),
            ),
            managed: true,
            tty: detailed.tty,
            process_pid: detailed.process_pid,
        })
    }

    fn respawn_unit_with_cwd(
        &self,
        group: &str,
        _workspace_name: &str,
        unit_id: &UnitId,
        cwd: &Utf8Path,
    ) -> Result<(), RuntimeError> {
        let pane_id = PaneId::new(unit_id.as_str().to_owned());
        self.tmux
            .respawn_pane(group, &pane_id, cwd)
            .map_err(map_tmux_error)
    }

    fn respawn_unit_with_command(
        &self,
        group: &str,
        _workspace_name: &str,
        unit_id: &UnitId,
        cwd: &Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        let pane_id = PaneId::new(unit_id.as_str().to_owned());
        self.tmux
            .respawn_pane_with_command(group, &pane_id, cwd, command)
            .map_err(map_tmux_error)
    }

    fn apply_window_layout(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
        layout: &str,
    ) -> Result<(), RuntimeError> {
        self.tmux
            .apply_layout(group, workspace_name, window_index, layout)
            .map_err(map_tmux_error)
    }

    fn select_window(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
    ) -> Result<(), RuntimeError> {
        self.tmux
            .select_window(group, workspace_name, window_index)
            .map_err(map_tmux_error)
    }

    fn select_pane(&self, group: &str, unit_id: &UnitId) -> Result<(), RuntimeError> {
        self.tmux
            .select_pane(group, &PaneId::new(unit_id.as_str().to_owned()))
            .map_err(map_tmux_error)
    }

    fn configure_snapshot_hooks(
        &self,
        group: &str,
        workspace_name: &str,
        mode: SnapshotHookMode,
        notify_command_prefix: &str,
    ) -> Result<(), RuntimeError> {
        let hooks = [
            (
                "after-new-window",
                format!("{notify_command_prefix} --snapshot-event new_window"),
            ),
            (
                "after-kill-window",
                format!("{notify_command_prefix} --snapshot-event kill_window"),
            ),
            (
                "after-split-window",
                format!("{notify_command_prefix} --snapshot-event split_window"),
            ),
            (
                "after-kill-pane",
                format!("{notify_command_prefix} --snapshot-event kill_pane"),
            ),
            (
                "after-select-pane",
                format!("{notify_command_prefix} --snapshot-event select_pane"),
            ),
            (
                "window-layout-changed",
                format!("{notify_command_prefix} --snapshot-event layout_changed"),
            ),
            (
                "pane-died",
                format!("{notify_command_prefix} --snapshot-event pane_died"),
            ),
            (
                "pane-exited",
                format!("{notify_command_prefix} --snapshot-event pane_exited"),
            ),
        ];

        for (name, shell_cmd) in hooks {
            match mode {
                SnapshotHookMode::Install => {
                    let tmux_cmd = crate::runtime::render_hook_run_shell(&shell_cmd);
                    self.tmux
                        .set_hook(group, workspace_name, name, Some(&tmux_cmd))
                        .map_err(map_tmux_error)?;
                }
                SnapshotHookMode::Remove => self
                    .tmux
                    .set_hook(group, workspace_name, name, None)
                    .map_err(map_tmux_error)?,
            }
        }

        Ok(())
    }
}

fn map_tmux_error(err: TmuxError) -> RuntimeError {
    use workhive_core::ports::BackendUnavailableReason;
    match err {
        TmuxError::TmuxNotFound => RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::BackendBinaryNotFound {
                binary: "tmux".to_owned(),
            },
            detail: "tmux executable not found".to_owned(),
        },
        TmuxError::ServerNotRunning { server } => RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: server.clone(),
            },
            detail: format!("tmux server unavailable: {server}"),
        },
        TmuxError::SocketNotFound { path } => RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound { path: path.clone() },
            detail: format!("tmux socket not found: {path}"),
        },
        TmuxError::PermissionDenied { detail } => RuntimeError::PermissionDenied { detail },
        TmuxError::SessionNotFound { name } => RuntimeError::WorkspaceNotFound { workspace: name },
        TmuxError::SessionAlreadyExists { name } => {
            RuntimeError::WorkspaceAlreadyExists { workspace: name }
        }
        TmuxError::SessionCreateFailed { name, source } => RuntimeError::OperationFailed {
            message: format!("failed to create session '{name}': {source}"),
        },
        TmuxError::SessionCreateNotVerified { server, name } => RuntimeError::OperationFailed {
            message: format!("session '{name}' not verified on server '{server}'"),
        },
        TmuxError::PaneNotFound { id } => RuntimeError::UnitNotFound { id },
        TmuxError::LastPaneCloseRejected => RuntimeError::LastUnitCloseRejected,
        TmuxError::CommandFailed { message } => RuntimeError::OperationFailed { message },
    }
}

/// Adapter-side wrapper over the shared start-command classifier.
///
/// Forwards to `classify_start_command` in `workhive-core` so the
/// snapshot runtime path and the live tmux runtime path share one
/// typed source of truth. In particular, this guarantees that a
/// `workhive internal pane-supervisor ... --tool <name>` argv recovers
/// as `PaneLaunchOrigin::Tool { name }` and a malformed wrapper
/// collapses to `None`, never to a replayable `InlineCommand`. See
/// `workhive_core::usecases::runtime_start_command` for the full
/// classification matrix.
fn derive_launch_origin(
    start_command: Option<&str>,
    _fallback_current_command: Option<&str>,
) -> Option<PaneLaunchOrigin> {
    let classification = classify_start_command(start_command);
    classification_to_launch_origin(classification)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{
        AttachRequest, CreateSessionRequest, PaneInfo, SendKeysRequest, SessionInfo,
        SplitPaneRequest, TmuxPort, WindowInfo,
    };
    use std::sync::Arc;

    #[test]
    fn snapshot_adapter_hooks_use_run_shell_wrapper() {
        let hook_log: Arc<std::sync::Mutex<Vec<Option<String>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let stub = HookRecordingStub {
            hooks: Arc::clone(&hook_log),
        };
        let adapter = TmuxSnapshotRuntimeAdapter::new(Arc::new(stub));

        adapter
            .configure_snapshot_hooks(
                "srv",
                "sess",
                SnapshotHookMode::Install,
                "workhive internal snapshot notify --state-key key",
            )
            .expect("install hooks should succeed");

        let hooks = hook_log.lock().unwrap();
        assert_eq!(hooks.len(), 8, "expected eight hook install calls");
        for cmd in hooks.iter() {
            let body = cmd.as_ref().expect("install hook must have command");
            assert!(
                body.starts_with("run-shell -b '"),
                "hook body must be a valid tmux command, got: {body}"
            );
            assert!(
                body.contains("--snapshot-event"),
                "hook body must include --snapshot-event, got: {body}"
            );
        }
    }

    struct HookRecordingStub {
        hooks: Arc<std::sync::Mutex<Vec<Option<String>>>>,
    }

    impl TmuxPort for HookRecordingStub {
        fn has_session(&self, _s: &str, _n: &str) -> Result<bool, TmuxError> {
            Ok(true)
        }
        fn create_session(&self, _req: CreateSessionRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn configure_session(&self, _s: &str, _n: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn attach(&self, _req: AttachRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn detach_client(&self, _s: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn kill_session(&self, _s: &str, _n: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_sessions(&self, _s: &str) -> Result<Vec<SessionInfo>, TmuxError> {
            Ok(vec![])
        }
        fn kill_server(&self, _s: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn split_pane(&self, _req: SplitPaneRequest) -> Result<PaneInfo, TmuxError> {
            unimplemented!()
        }
        fn close_pane(&self, _s: &str, _p: &PaneId) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_panes(&self, _s: &str, _n: &str) -> Result<Vec<PaneInfo>, TmuxError> {
            Ok(vec![])
        }
        fn current_pane(&self, _s: &str) -> Result<PaneInfo, TmuxError> {
            unimplemented!()
        }
        fn send_keys(&self, _req: SendKeysRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn set_pane_title(&self, _s: &str, _p: &PaneId, _t: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_windows(&self, _s: &str, _n: &str) -> Result<Vec<WindowInfo>, TmuxError> {
            Ok(vec![])
        }
        fn list_panes_topology(
            &self,
            _s: &str,
            _n: &str,
        ) -> Result<Vec<crate::tmux::PaneTopologyInfo>, TmuxError> {
            Ok(vec![])
        }
        fn create_window(
            &self,
            _s: &str,
            _n: &str,
            _i: u32,
            _name: Option<&str>,
            _c: &camino::Utf8Path,
        ) -> Result<crate::tmux::PaneTopologyInfo, TmuxError> {
            unimplemented!()
        }
        fn apply_layout(&self, _s: &str, _n: &str, _i: u32, _l: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn select_window(&self, _s: &str, _n: &str, _i: u32) -> Result<(), TmuxError> {
            Ok(())
        }
        fn select_pane(&self, _s: &str, _p: &PaneId) -> Result<(), TmuxError> {
            Ok(())
        }
        fn respawn_pane(&self, _s: &str, _p: &PaneId, _cwd: &Utf8Path) -> Result<(), TmuxError> {
            Ok(())
        }
        fn respawn_pane_with_command(
            &self,
            _s: &str,
            _p: &PaneId,
            _cwd: &Utf8Path,
            _command: &str,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn set_hook(
            &self,
            _s: &str,
            _n: &str,
            _hook: &str,
            command: Option<&str>,
        ) -> Result<(), TmuxError> {
            self.hooks.lock().unwrap().push(command.map(String::from));
            Ok(())
        }

        fn capture_pane_text(
            &self,
            _request: crate::tmux::CapturePaneRequest,
        ) -> Result<String, TmuxError> {
            // Snapshot-instrumentation tests do not exercise terminal-status
            // capture; return empty so any unintended call degrades quietly.
            Ok(String::new())
        }
    }
}

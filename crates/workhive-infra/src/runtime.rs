//! Tmux-backed runtime adapter.
//!
//! Bridges `WorkspaceRuntimePort` and `PaneRuntimePort` to the underlying
//! `TmuxPort`, translating runtime-neutral operations into tmux commands.
//!
//! This is the only place in the codebase that bridges `TmuxError` →
//! `RuntimeError`. `workhive-core` use cases never see `TmuxError`.

use workhive_core::domain::{PaneId, PaneLaunchOrigin, PaneRuntimeId, UnitId};
use workhive_core::ports::{
    AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, PaneRegistryPort, PaneRuntimePort,
    RuntimeError, RuntimePaneSnapshot, RuntimeTopologySnapshot, RuntimeUnitSnapshot,
    RuntimeWindowSnapshot, SendInputRequest, SetUnitTitleRequest, SnapshotHookMode,
    SnapshotRuntimePort, SpawnUnitRequest, WorkspaceRuntimeInfo, WorkspaceRuntimePort,
};
use workhive_core::usecases::runtime_start_command::{
    classification_to_launch_origin, classify_start_command,
};

use crate::errors::TmuxError;
use crate::tmux::{
    AttachRequest, CreateSessionRequest, SendKeysRequest, SplitPaneRequest, TmuxPort,
};

/// Adapts a `TmuxPort` into `WorkspaceRuntimePort` + `PaneRuntimePort`.
pub struct TmuxRuntimeAdapter<'a> {
    tmux: &'a dyn TmuxPort,
    pane_registry: Option<&'a dyn PaneRegistryPort>,
}

impl<'a> TmuxRuntimeAdapter<'a> {
    pub fn new(tmux: &'a dyn TmuxPort) -> Self {
        Self {
            tmux,
            pane_registry: None,
        }
    }

    /// Construct a runtime adapter with registry-aware launch metadata lookup.
    pub fn with_pane_registry(
        tmux: &'a dyn TmuxPort,
        pane_registry: &'a dyn PaneRegistryPort,
    ) -> Self {
        Self {
            tmux,
            pane_registry: Some(pane_registry),
        }
    }

    fn resolve_launch_origin(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        start_command: Option<&str>,
        current_command: Option<&str>,
    ) -> Option<PaneLaunchOrigin> {
        if let Some(registry) = self.pane_registry {
            let runtime_id = PaneRuntimeId::derive(group, workspace_name, unit_id.as_str());
            match registry.get(&runtime_id) {
                Ok(Some(descriptor)) => return Some(descriptor.launch_origin),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(
                        runtime_id = %runtime_id,
                        error = %error,
                        "failed to read pane launch metadata from registry; falling back to runtime start command"
                    );
                }
            }
        }
        derive_launch_origin(start_command, current_command)
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
        // ServerNotRunning means the tmux daemon is not running — this is a
        // backend-availability condition, not a missing workspace. Mapping to
        // WorkspaceNotFound was incorrect: core's `resolve_runtime_session_state`
        // only folds BackendUnavailable into the expected "not running" state.
        TmuxError::ServerNotRunning { server } => RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: server.clone(),
            },
            detail: format!("tmux server not running: {server}"),
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
            message: format!("failed to create workspace runtime '{name}': {source}"),
        },
        TmuxError::SessionCreateNotVerified { server, name } => RuntimeError::OperationFailed {
            message: format!(
                "session '{name}' on server '{server}' was not found after creation; \
                 tmux may have reported false success"
            ),
        },
        TmuxError::PaneNotFound { id } => RuntimeError::UnitNotFound { id },
        TmuxError::LastPaneCloseRejected => RuntimeError::LastUnitCloseRejected,
        TmuxError::CommandFailed { message } => RuntimeError::OperationFailed { message },
    }
}

fn unit_id_to_pane_id(unit_id: &UnitId) -> PaneId {
    PaneId::new(unit_id.as_str())
}

impl WorkspaceRuntimePort for TmuxRuntimeAdapter<'_> {
    fn has_workspace(&self, group: &str, workspace_name: &str) -> Result<bool, RuntimeError> {
        self.tmux
            .has_session(group, workspace_name)
            .map_err(map_tmux_error)
    }

    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
        self.tmux
            .create_session(CreateSessionRequest {
                server_name: request.group,
                session_name: request.workspace_name,
                working_dir: request.working_dir,
                env: request.env,
            })
            .map_err(map_tmux_error)
    }

    fn configure_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        self.tmux
            .configure_session(group, workspace_name)
            .map_err(map_tmux_error)
    }

    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        self.tmux
            .attach(AttachRequest {
                server_name: request.group,
                session_name: request.workspace_name,
            })
            .map_err(map_tmux_error)
    }

    fn detach_workspace(&self, group: &str) -> Result<(), RuntimeError> {
        self.tmux.detach_client(group).map_err(map_tmux_error)
    }

    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        self.tmux
            .kill_session(group, workspace_name)
            .map_err(map_tmux_error)
    }

    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        let sessions = self.tmux.list_sessions(group).map_err(map_tmux_error)?;
        Ok(sessions
            .into_iter()
            .map(|s| WorkspaceRuntimeInfo {
                workspace_name: s.name,
                unit_count: s.panes,
                attached: s.attached,
            })
            .collect())
    }

    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError> {
        self.tmux.kill_server(group).map_err(map_tmux_error)
    }
}

impl PaneRuntimePort for TmuxRuntimeAdapter<'_> {
    fn spawn_unit(&self, request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let pane = self
            .tmux
            .split_pane(SplitPaneRequest {
                server_name: request.group,
                session_name: request.workspace_name,
                direction: request.direction,
                command: request.command,
                target_pane: request.target_unit.map(|u| unit_id_to_pane_id(&u)),
                size_percent: request.size_percent,
                env: request.env,
                cwd: request.cwd,
            })
            .map_err(map_tmux_error)?;
        Ok(RuntimeUnitSnapshot {
            id: UnitId::from(pane.id),
            index: pane.index,
            cwd: pane.cwd,
            command: pane.command,
            active: pane.active,
            kind: pane.kind,
            state: pane.state,
            tty: pane.tty,
            process_pid: pane.process_pid,
        })
    }

    fn close_unit(&self, group: &str, unit_id: &UnitId) -> Result<(), RuntimeError> {
        let pane_id = unit_id_to_pane_id(unit_id);
        self.tmux
            .close_pane(group, &pane_id)
            .map_err(map_tmux_error)
    }

    fn list_units(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        let panes = self
            .tmux
            .list_panes(group, workspace_name)
            .map_err(map_tmux_error)?;
        Ok(panes
            .into_iter()
            .map(|p| RuntimeUnitSnapshot {
                id: UnitId::from(p.id),
                index: p.index,
                cwd: p.cwd,
                command: p.command,
                active: p.active,
                kind: p.kind,
                state: p.state,
                tty: p.tty,
                process_pid: p.process_pid,
            })
            .collect())
    }

    fn current_unit(&self, group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let pane = self.tmux.current_pane(group).map_err(map_tmux_error)?;
        Ok(RuntimeUnitSnapshot {
            id: UnitId::from(pane.id),
            index: pane.index,
            cwd: pane.cwd,
            command: pane.command,
            active: pane.active,
            kind: pane.kind,
            state: pane.state,
            tty: pane.tty,
            process_pid: pane.process_pid,
        })
    }

    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        let pane_id = unit_id_to_pane_id(&request.target_unit);
        self.tmux
            .send_keys(SendKeysRequest {
                server_name: request.group,
                target_pane: pane_id,
                text: request.text,
                enter: request.enter,
            })
            .map_err(map_tmux_error)
    }

    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
        let pane_id = unit_id_to_pane_id(&request.target_unit);
        self.tmux
            .set_pane_title(&request.group, &pane_id, &request.title)
            .map_err(map_tmux_error)
    }
}

impl SnapshotRuntimePort for TmuxRuntimeAdapter<'_> {
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
            .map(|pane| {
                let unit_id = UnitId::new(pane.id.as_str().to_owned());
                RuntimePaneSnapshot {
                    unit_id: unit_id.clone(),
                    pane_id: Some(pane.id.as_str().to_owned()),
                    window_index: pane.window_index,
                    pane_index: pane.pane_index,
                    cwd: pane.cwd,
                    title: pane.title,
                    current_command: Some(pane.current_command.clone()),
                    start_command: pane.start_command.clone(),
                    active: pane.active,
                    dead: pane.dead,
                    launch_origin: self.resolve_launch_origin(
                        group,
                        workspace_name,
                        &unit_id,
                        pane.start_command.as_deref(),
                        Some(pane.current_command.as_str()),
                    ),
                    managed: true,
                    tty: pane.tty,
                    process_pid: pane.process_pid,
                }
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
        cwd: &camino::Utf8Path,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        let pane = self
            .tmux
            .create_window(group, workspace_name, window_index, name, cwd)
            .map_err(map_tmux_error)?;
        let unit_id = UnitId::new(pane.id.as_str().to_owned());

        Ok(RuntimePaneSnapshot {
            unit_id: unit_id.clone(),
            pane_id: Some(pane.id.as_str().to_owned()),
            window_index: pane.window_index,
            pane_index: pane.pane_index,
            cwd: pane.cwd,
            title: pane.title,
            current_command: Some(pane.current_command.clone()),
            start_command: pane.start_command.clone(),
            active: pane.active,
            dead: pane.dead,
            launch_origin: self.resolve_launch_origin(
                group,
                workspace_name,
                &unit_id,
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
        direction: workhive_core::domain::PaneDirection,
        prep: workhive_core::ports::SplitPanePreparation,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        let pane = self
            .tmux
            .split_pane(SplitPaneRequest {
                server_name: group.to_owned(),
                session_name: workspace_name.to_owned(),
                direction,
                command: prep.command,
                target_pane: Some(unit_id_to_pane_id(target_unit)),
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
            .find(|candidate| candidate.id.as_str() == pane.id.as_str());

        let detailed = detailed.ok_or_else(|| RuntimeError::OperationFailed {
            message: format!(
                "split pane succeeded but topology lookup missing pane '{}'",
                pane.id
            ),
        })?;
        let unit_id = UnitId::new(detailed.id.as_str().to_owned());

        Ok(RuntimePaneSnapshot {
            unit_id: unit_id.clone(),
            pane_id: Some(detailed.id.as_str().to_owned()),
            window_index: detailed.window_index,
            pane_index: detailed.pane_index,
            cwd: detailed.cwd,
            title: detailed.title,
            current_command: Some(detailed.current_command.clone()),
            start_command: detailed.start_command.clone(),
            active: detailed.active,
            dead: detailed.dead,
            launch_origin: self.resolve_launch_origin(
                group,
                workspace_name,
                &unit_id,
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
        cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        let pane_id = unit_id_to_pane_id(unit_id);
        self.tmux
            .respawn_pane(group, &pane_id, cwd)
            .map_err(map_tmux_error)
    }

    fn respawn_unit_with_command(
        &self,
        group: &str,
        _workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        let pane_id = unit_id_to_pane_id(unit_id);
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
        let pane_id = unit_id_to_pane_id(unit_id);
        self.tmux
            .select_pane(group, &pane_id)
            .map_err(map_tmux_error)
    }

    fn configure_snapshot_hooks(
        &self,
        group: &str,
        workspace_name: &str,
        mode: SnapshotHookMode,
        notify_command_prefix: &str,
    ) -> Result<(), RuntimeError> {
        let hook_events = [
            ("after-new-window", "new_window"),
            ("after-kill-window", "kill_window"),
            ("after-split-window", "split_window"),
            ("after-kill-pane", "kill_pane"),
            ("after-select-pane", "select_pane"),
            ("window-layout-changed", "layout_changed"),
            ("pane-died", "pane_died"),
            ("pane-exited", "pane_exited"),
        ];

        for (hook_name, event_name) in hook_events {
            match mode {
                SnapshotHookMode::Install => {
                    let shell_cmd =
                        format!("{notify_command_prefix} --snapshot-event {event_name}");
                    let tmux_cmd = render_hook_run_shell(&shell_cmd);
                    self.tmux
                        .set_hook(group, workspace_name, hook_name, Some(&tmux_cmd))
                        .map_err(map_tmux_error)?;
                }
                SnapshotHookMode::Remove => {
                    self.tmux
                        .set_hook(group, workspace_name, hook_name, None)
                        .map_err(map_tmux_error)?;
                }
            }
        }

        Ok(())
    }
}

/// Wrap a shell command string in `run-shell -b '...'` for use as a tmux hook body.
///
/// Tmux hooks interpret their body as a tmux command, not a shell command.
/// `run-shell -b` executes the argument in a background shell process.
/// Single quotes inside the command are escaped as `'\''` (end-quote, literal quote, start-quote).
pub(crate) fn render_hook_run_shell(shell_cmd: &str) -> String {
    let escaped = shell_cmd.replace('\'', "'\\''");
    format!("run-shell -b '{escaped}'")
}

/// Adapter-side wrapper over the shared start-command classifier.
///
/// Delegates to `classify_start_command` in `workhive-core` so that tmux
/// runtime adapters and the snapshot runtime share a single, typed
/// understanding of what tmux's `pane_start_command` means. Classifying
/// `workhive internal pane-supervisor ...` as a wrapper (and recovering
/// `--tool <name>` when present) is centralised there; this function is the
/// adapter-facing seam that converts the classification into a
/// `PaneLaunchOrigin`. Returning `None` for shells, supervisor wrappers
/// without a recoverable tool, and empty start commands preserves any prior
/// authoritative identity supplied by the registry or hydrated baseline.
fn derive_launch_origin(
    start_command: Option<&str>,
    _fallback_current_command: Option<&str>,
) -> Option<PaneLaunchOrigin> {
    let classification = classify_start_command(start_command);
    classification_to_launch_origin(classification)
}

// ─── Adapter error-mapping tests ─────────────────────────────────────────────
//
// These tests verify that `TmuxError` variants are mapped to the expected
// `RuntimeError` variants at the adapter boundary:
//
//   `ServerNotRunning` on the `list_workspaces` path must produce
//   `RuntimeError::BackendUnavailable`, not `WorkspaceNotFound`.
//   `resolve_runtime_session_state` in core only absorbs
//   `BackendUnavailable` as the expected "not running" state.

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

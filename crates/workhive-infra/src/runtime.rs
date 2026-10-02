//! Tmux-backed runtime adapter.
//!
//! Bridges `WorkspaceRuntimePort` and `PaneRuntimePort` to the underlying
//! `TmuxPort`, translating runtime-neutral operations into tmux commands.
//!
//! This is the only place in the codebase that bridges `TmuxError` →
//! `RuntimeError`. `workhive-core` use cases never see `TmuxError`.

use std::ops::Deref;
use std::sync::Arc;

use workhive_core::domain::{PaneId, RuntimeBackendId, RuntimeBinding, UnitId};
use workhive_core::ports::{
    AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, PaneRuntimePort, RuntimeError,
    RuntimeUnitSnapshot, SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest,
    WorkspaceRuntimeInfo, WorkspaceRuntimePort,
};

use crate::tmux::{
    AttachRequest, CreateSessionRequest, PaneInfo, SendKeysRequest, SplitPaneRequest, TmuxPort,
};
use crate::tmux_errors::TmuxError;

/// Resolve a unit identity supplied by the active runtime process context.
///
/// Backend-specific environment names remain inside infrastructure; callers
/// receive only Workhive's opaque runtime unit identity.
pub fn inherited_runtime_unit_id() -> Option<String> {
    runtime_unit_id_from(|name| std::env::var(name).ok())
}

fn runtime_unit_id_from(mut read: impl FnMut(&str) -> Option<String>) -> Option<String> {
    ["HERDR_PANE_ID", "WORKHIVE_UNIT_ID"]
        .into_iter()
        .find_map(|name| read(name).filter(|value| !value.is_empty()))
}

enum TmuxHandle<'a> {
    Borrowed(&'a dyn TmuxPort),
    Owned(Arc<dyn TmuxPort>),
}

impl<'a> Deref for TmuxHandle<'a> {
    type Target = dyn TmuxPort + 'a;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(tmux) => *tmux,
            Self::Owned(tmux) => tmux.as_ref(),
        }
    }
}

/// Adapts a `TmuxPort` into `WorkspaceRuntimePort` + `PaneRuntimePort`.
pub struct TmuxRuntimeAdapter<'a> {
    tmux: TmuxHandle<'a>,
}

impl<'a> TmuxRuntimeAdapter<'a> {
    pub fn new(tmux: &'a dyn TmuxPort) -> Self {
        Self {
            tmux: TmuxHandle::Borrowed(tmux),
        }
    }

    pub fn from_owned<T>(tmux: T) -> TmuxRuntimeAdapter<'static>
    where
        T: TmuxPort + 'static,
    {
        TmuxRuntimeAdapter {
            tmux: TmuxHandle::Owned(Arc::new(tmux)),
        }
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

fn tmux_binding_parts(binding: &RuntimeBinding) -> Result<(&str, &str), RuntimeError> {
    if binding.backend != RuntimeBackendId::tmux() {
        return Err(RuntimeError::OperationFailed {
            message: format!(
                "tmux adapter cannot operate on '{}' runtime bindings",
                binding.backend
            ),
        });
    }
    Ok((&binding.scope_id, &binding.workspace_id))
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

    fn has_workspace_binding(&self, binding: &RuntimeBinding) -> Result<bool, RuntimeError> {
        let (group, workspace_name) = tmux_binding_parts(binding)?;
        self.has_workspace(group, workspace_name)
    }

    fn create_workspace_binding(
        &self,
        request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        let binding = RuntimeBinding::tmux(&request.group, &request.workspace_name);
        self.create_workspace(request)?;
        Ok(binding)
    }

    fn rename_workspace_binding(
        &self,
        from: &RuntimeBinding,
        to: &RuntimeBinding,
    ) -> Result<(), RuntimeError> {
        let (from_group, from_workspace) = tmux_binding_parts(from)?;
        let (to_group, to_workspace) = tmux_binding_parts(to)?;
        if from_group != to_group {
            return Err(RuntimeError::OperationFailed {
                message: "tmux workspace promotion cannot change servers".to_owned(),
            });
        }
        if from_workspace == to_workspace {
            return Ok(());
        }
        if !self.has_workspace(from_group, from_workspace)? {
            return Err(RuntimeError::WorkspaceNotFound {
                workspace: from_workspace.to_owned(),
            });
        }
        if self.has_workspace(to_group, to_workspace)? {
            return Err(RuntimeError::WorkspaceAlreadyExists {
                workspace: to_workspace.to_owned(),
            });
        }
        self.tmux
            .rename_session(from_group, from_workspace, to_workspace)
            .map_err(map_tmux_error)?;
        if self.has_workspace(from_group, from_workspace)?
            || !self.has_workspace(to_group, to_workspace)?
        {
            return Err(RuntimeError::OperationFailed {
                message: format!(
                    "tmux workspace promotion from '{from_workspace}' to '{to_workspace}' was not verified"
                ),
            });
        }
        Ok(())
    }
    fn attach_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        let (group, workspace_name) = tmux_binding_parts(binding)?;
        self.attach_workspace(AttachWorkspaceRequest {
            group: group.to_owned(),
            workspace_name: workspace_name.to_owned(),
            interaction_policy: policy,
        })
    }

    fn detach_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        let (group, workspace_name) = tmux_binding_parts(binding)?;
        self.detach_workspace(group, workspace_name)
    }

    fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        let (group, workspace_name) = tmux_binding_parts(binding)?;
        self.destroy_workspace(group, workspace_name)
    }

    fn configure_workspace(
        &self,
        group: &str,
        workspace_name: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.tmux
            .configure_session(group, workspace_name, policy)
            .map_err(map_tmux_error)
    }

    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        self.tmux
            .configure_session(
                &request.group,
                &request.workspace_name,
                &request.interaction_policy,
            )
            .map_err(map_tmux_error)?;
        self.tmux
            .attach(AttachRequest {
                server_name: request.group,
                session_name: request.workspace_name,
            })
            .map_err(map_tmux_error)
    }

    fn switch_workspace(
        &self,
        group: &str,
        target_workspace: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.tmux
            .configure_session(group, target_workspace, policy)
            .map_err(map_tmux_error)?;
        self.tmux
            .switch_client(group, target_workspace)
            .map_err(map_tmux_error)
    }

    fn detach_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        self.tmux
            .detach_client(group, workspace_name)
            .map_err(map_tmux_error)
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
                runtime_id: s.name,
                label: None,
                unit_count: s.panes,
                attached: s.attached,
            })
            .collect())
    }

    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError> {
        self.tmux.kill_server(group).map_err(map_tmux_error)
    }
}

fn runtime_unit_snapshot(pane: PaneInfo) -> RuntimeUnitSnapshot {
    RuntimeUnitSnapshot {
        id: UnitId::from(pane.id),
        index: pane.index,
        cwd: pane.cwd,
        command: pane.command,
        active: pane.active,
        kind: pane.kind,
        state: pane.state,
        tty: pane.tty,
        process_pid: pane.process_pid,
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
        Ok(runtime_unit_snapshot(pane))
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
        Ok(panes.into_iter().map(runtime_unit_snapshot).collect())
    }

    fn current_unit(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let pane = self
            .tmux
            .current_pane(group, workspace_name)
            .map_err(map_tmux_error)?;
        Ok(runtime_unit_snapshot(pane))
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

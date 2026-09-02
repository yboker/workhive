//! Fake runtime adapter for testing.
//!
//! `FakeRuntime` is an in-memory implementation of `WorkspaceRuntimePort` and
//! `PaneRuntimePort`. It does **not** implement `TmuxPort` — that is an
//! infra-level detail. Test code uses the runtime-neutral ports exclusively.
//!
//! # Error injection
//!
//! Use `FakeRuntime::inject_errors()` to configure per-operation failures.
//! Injected errors are cloned/replicated on each call so they can be triggered
//! multiple times.

use std::cell::RefCell;
use std::collections::HashMap;

use camino::Utf8PathBuf;
use workhive_core::domain::{
    CommandSpec, PaneDirection, PaneKind, PaneLaunchOrigin, PaneState, ToolName, UnitId,
};
use workhive_core::ports::{
    AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, PaneRuntimePort, RuntimeError,
    RuntimePaneSnapshot, RuntimeTopologySnapshot, RuntimeUnitSnapshot, RuntimeWindowSnapshot,
    SendInputRequest, SnapshotHookMode, SnapshotRuntimePort, SpawnUnitRequest,
    WorkspaceRuntimeInfo, WorkspaceRuntimePort,
};

// ─── Error injection ──────────────────────────────────────────────────────

/// Per-operation error injection for `FakeRuntime`.
///
/// Set a field to inject a specific `RuntimeError` when that operation is
/// called. `None` means the operation succeeds normally. Errors are
/// replicated on each call (not consumed), so they can fire repeatedly.
#[derive(Default)]
pub struct RuntimeErrorInjection {
    pub list_workspaces: Option<RuntimeError>,
    pub list_units: Option<RuntimeError>,
    pub send_input: Option<RuntimeError>,
    pub spawn_unit: Option<RuntimeError>,
    pub has_workspace: Option<RuntimeError>,
    pub attach_workspace: Option<RuntimeError>,
    pub destroy_group: Option<RuntimeError>,
    /// Inject failures for the typed `respawn_unit_with_command` verb. This
    /// path replaces visible `send_input` for installing supervisor-wrapped
    /// or inline tool launches without visible keystrokes.
    pub respawn_command: Option<RuntimeError>,
}

fn replicate_error(err: &RuntimeError) -> RuntimeError {
    match err {
        RuntimeError::BackendUnavailable { reason, detail } => RuntimeError::BackendUnavailable {
            reason: reason.clone(),
            detail: detail.clone(),
        },
        RuntimeError::WorkspaceNotFound { workspace } => RuntimeError::WorkspaceNotFound {
            workspace: workspace.clone(),
        },
        RuntimeError::WorkspaceAlreadyExists { workspace } => {
            RuntimeError::WorkspaceAlreadyExists {
                workspace: workspace.clone(),
            }
        }
        RuntimeError::UnitNotFound { id } => RuntimeError::UnitNotFound { id: id.clone() },
        RuntimeError::LastUnitCloseRejected => RuntimeError::LastUnitCloseRejected,
        RuntimeError::OperationFailed { message } => RuntimeError::OperationFailed {
            message: message.clone(),
        },
        RuntimeError::PermissionDenied { detail } => RuntimeError::PermissionDenied {
            detail: detail.clone(),
        },
    }
}

// ─── In-memory workspace/unit state ──────────────────────────────────────

struct StoredWorkspace {
    group: String,
    name: String,
    attached: bool,
}

struct StoredUnit {
    group: String,
    workspace: String,
    id: UnitId,
    window_index: u32,
    pane_index: u32,
    cwd: Utf8PathBuf,
    command: String,
    active: bool,
}

// ─── FakeRuntime ─────────────────────────────────────────────────────────

/// In-memory fake for `WorkspaceRuntimePort` + `PaneRuntimePort`.
///
/// This fake is runtime-neutral: it stores workspaces and units without any
/// knowledge of tmux. Use it in unit tests for `workhive-core` use cases.
pub struct FakeRuntime {
    workspaces: RefCell<Vec<StoredWorkspace>>,
    units: RefCell<Vec<StoredUnit>>,
    sent_inputs: RefCell<Vec<(String, String)>>,
    sent_input_requests: RefCell<Vec<SendInputRequest>>,
    split_directions: RefCell<Vec<PaneDirection>>,
    spawn_requests: RefCell<Vec<SpawnUnitRequest>>,
    title_updates: RefCell<Vec<(String, String)>>,
    create_env: RefCell<Vec<RecordedCreateEnv>>,
    /// Records every `respawn_unit_with_cwd` invocation in `(unit_id, cwd)`
    /// form. Restore tests use this to assert that existing-pane cwd
    /// repair must go through the infra-level respawn path, not through
    /// visible `cd --` keystrokes carried by `send_input`.
    respawn_calls: RefCell<Vec<(String, Utf8PathBuf)>>,
    /// Records every `respawn_unit_with_command` invocation in
    /// `(unit_id, cwd, command)` form. Restore / fresh-open tests use this
    /// to assert that the pane supervisor wrapper is installed via
    /// `tmux respawn-pane <command>` rather than typed into a visible
    /// shell.
    respawn_command_calls: RefCell<Vec<(String, Utf8PathBuf, String)>>,
    errors: RefCell<RuntimeErrorInjection>,
    // Per-server `list_workspaces` failures. A server present here fails only
    // for that server, modeling one project's runtime backend being unreachable
    // while other projects stay healthy.
    list_workspaces_errors_by_server: RefCell<HashMap<String, RuntimeError>>,
    next_unit_idx: RefCell<u32>,
}

/// Workspace name + the env vars passed to `create_workspace`. Captured for
/// observability tests (e.g. `.env` injection) without leaking tmux details
/// into the assertion sites.
pub type RecordedCreateEnv = (String, Vec<(String, String)>);

impl FakeRuntime {
    pub fn new() -> Self {
        Self {
            workspaces: RefCell::new(Vec::new()),
            units: RefCell::new(Vec::new()),
            sent_inputs: RefCell::new(Vec::new()),
            sent_input_requests: RefCell::new(Vec::new()),
            split_directions: RefCell::new(Vec::new()),
            spawn_requests: RefCell::new(Vec::new()),
            title_updates: RefCell::new(Vec::new()),
            create_env: RefCell::new(Vec::new()),
            respawn_calls: RefCell::new(Vec::new()),
            respawn_command_calls: RefCell::new(Vec::new()),
            errors: RefCell::new(RuntimeErrorInjection::default()),
            list_workspaces_errors_by_server: RefCell::new(HashMap::new()),
            next_unit_idx: RefCell::new(0),
        }
    }

    /// Inspect all `(unit_id, cwd)` pairs the snapshot runtime port has
    /// respawned. Tests assert this is non-empty for any reused-pane restore
    /// path that needs cwd correction.
    pub fn respawn_calls(&self) -> Vec<(String, Utf8PathBuf)> {
        self.respawn_calls.borrow().clone()
    }

    /// Inspect all `(unit_id, cwd, command)` triples the snapshot runtime
    /// port has been asked to respawn-with-command. Tests assert this is
    /// non-empty for any restore / fresh-open pane that runs a tool or an
    /// inline command, proving the supervisor wrapper was installed via
    /// the typed runtime launch verb instead of being typed into a
    /// visible shell.
    pub fn respawn_command_calls(&self) -> Vec<(String, Utf8PathBuf, String)> {
        self.respawn_command_calls.borrow().clone()
    }

    /// Inspect every `(workspace_name, env)` snapshot captured by
    /// `create_workspace`. Tests use this to assert environment-variable
    /// injection actually reached the runtime port.
    pub fn create_workspace_env(&self) -> Vec<RecordedCreateEnv> {
        self.create_env.borrow().clone()
    }

    /// Inspect what inputs were sent to units during tests.
    pub fn sent_inputs(&self) -> Vec<(String, String)> {
        self.sent_inputs.borrow().clone()
    }

    /// Inspect the full `SendInputRequest`s as observed by the fake. Use this
    /// when a test needs to assert `enter` semantics (item 7) or verify
    /// routing through the control plane supervisor.
    pub fn sent_input_requests(&self) -> Vec<SendInputRequest> {
        self.sent_input_requests.borrow().clone()
    }

    /// Inspect split directions requested by restore/layout orchestration.
    pub fn split_directions(&self) -> Vec<PaneDirection> {
        self.split_directions.borrow().clone()
    }

    /// Inspect all spawn requests in creation order.
    pub fn spawn_requests(&self) -> Vec<SpawnUnitRequest> {
        self.spawn_requests.borrow().clone()
    }

    /// Inspect all title updates in `(unit_id, title)` form.
    pub fn title_updates(&self) -> Vec<(String, String)> {
        self.title_updates.borrow().clone()
    }

    /// Configure per-operation error injection.
    pub fn inject_errors(&self, injection: RuntimeErrorInjection) {
        *self.errors.borrow_mut() = injection;
    }

    /// Inject a `list_workspaces` failure for a single server, leaving every
    /// other server reachable.
    ///
    /// This models the real adapter contract where one project's tmux socket
    /// is missing (`BackendUnavailable`) while sibling projects respond
    /// normally — letting tests exercise per-project degradation that the
    /// global `inject_errors` switch cannot express.
    pub fn inject_list_workspaces_error_for_server(&self, server: &str, err: RuntimeError) {
        self.list_workspaces_errors_by_server
            .borrow_mut()
            .insert(server.to_owned(), err);
    }

    fn check_error(&self, field: &Option<RuntimeError>) -> Result<(), RuntimeError> {
        if let Some(err) = field {
            Err(replicate_error(err))
        } else {
            Ok(())
        }
    }

    fn alloc_unit_id(&self) -> UnitId {
        let idx = *self.next_unit_idx.borrow();
        *self.next_unit_idx.borrow_mut() = idx + 1;
        UnitId::new(format!("unit-{idx}"))
    }
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// ─── WorkspaceRuntimePort impl ────────────────────────────────────────────

impl WorkspaceRuntimePort for FakeRuntime {
    fn has_workspace(&self, group: &str, workspace_name: &str) -> Result<bool, RuntimeError> {
        self.check_error(&self.errors.borrow().has_workspace)?;
        Ok(self
            .workspaces
            .borrow()
            .iter()
            .any(|w| w.group == group && w.name == workspace_name))
    }

    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
        let group = request.group.clone();
        let name = request.workspace_name.clone();

        // Record the env handed to the port so tests can assert injection.
        self.create_env
            .borrow_mut()
            .push((name.clone(), request.env.clone()));

        // Enforce (group, workspace_name) uniqueness — mirrors the real tmux adapter
        // which returns RuntimeError::WorkspaceAlreadyExists when new-session fails
        // with "duplicate session".
        if self
            .workspaces
            .borrow()
            .iter()
            .any(|w| w.group == group && w.name == name)
        {
            return Err(RuntimeError::WorkspaceAlreadyExists { workspace: name });
        }

        self.workspaces.borrow_mut().push(StoredWorkspace {
            group: group.clone(),
            name: name.clone(),
            attached: false,
        });

        let id = self.alloc_unit_id();
        self.units.borrow_mut().push(StoredUnit {
            group,
            workspace: name,
            id,
            window_index: 0,
            pane_index: 0,
            cwd: request.working_dir,
            command: "zsh".to_owned(),
            active: true,
        });

        Ok(())
    }

    fn configure_workspace(&self, _group: &str, _workspace_name: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        self.check_error(&self.errors.borrow().attach_workspace)?;
        let mut workspaces = self.workspaces.borrow_mut();
        if let Some(ws) = workspaces
            .iter_mut()
            .find(|w| w.group == request.group && w.name == request.workspace_name)
        {
            ws.attached = true;
            Ok(())
        } else {
            Err(RuntimeError::WorkspaceNotFound {
                workspace: request.workspace_name,
            })
        }
    }

    fn detach_workspace(&self, _group: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        self.workspaces
            .borrow_mut()
            .retain(|w| !(w.group == group && w.name == workspace_name));
        self.units
            .borrow_mut()
            .retain(|u| !(u.group == group && u.workspace == workspace_name));
        Ok(())
    }

    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        self.check_error(&self.errors.borrow().list_workspaces)?;
        if let Some(err) = self.list_workspaces_errors_by_server.borrow().get(group) {
            return Err(replicate_error(err));
        }
        let workspaces = self.workspaces.borrow();
        let units = self.units.borrow();
        Ok(workspaces
            .iter()
            .filter(|w| w.group == group)
            .map(|w| {
                let unit_count = units
                    .iter()
                    .filter(|u| u.group == group && u.workspace == w.name)
                    .count() as u32;
                WorkspaceRuntimeInfo {
                    workspace_name: w.name.clone(),
                    unit_count,
                    attached: w.attached,
                }
            })
            .collect())
    }

    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError> {
        self.check_error(&self.errors.borrow().destroy_group)?;
        self.workspaces.borrow_mut().retain(|w| w.group != group);
        self.units.borrow_mut().retain(|u| u.group != group);
        Ok(())
    }
}

// ─── PaneRuntimePort impl ─────────────────────────────────────────────────

impl PaneRuntimePort for FakeRuntime {
    fn spawn_unit(&self, request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        self.check_error(&self.errors.borrow().spawn_unit)?;
        self.spawn_requests.borrow_mut().push(request.clone());

        let id = self.alloc_unit_id();
        let window_index = request
            .target_unit
            .as_ref()
            .and_then(|target| {
                self.units
                    .borrow()
                    .iter()
                    .find(|unit| unit.id == *target)
                    .map(|unit| unit.window_index)
            })
            .unwrap_or(0);
        let pane_index = self
            .units
            .borrow()
            .iter()
            .filter(|u| {
                u.group == request.group
                    && u.workspace == request.workspace_name
                    && u.window_index == window_index
            })
            .count() as u32;

        let snapshot = RuntimeUnitSnapshot {
            id: id.clone(),
            index: pane_index,
            cwd: Utf8PathBuf::from("/tmp"),
            command: request.command.unwrap_or_else(|| "zsh".to_owned()),
            active: false,
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        };

        self.units.borrow_mut().push(StoredUnit {
            group: request.group,
            workspace: request.workspace_name,
            id,
            window_index,
            pane_index: snapshot.index,
            cwd: snapshot.cwd.clone(),
            command: snapshot.command.clone(),
            active: false,
        });

        Ok(snapshot)
    }

    fn close_unit(&self, _group: &str, unit_id: &UnitId) -> Result<(), RuntimeError> {
        let mut units = self.units.borrow_mut();
        if units.len() <= 1 {
            return Err(RuntimeError::LastUnitCloseRejected);
        }
        units.retain(|u| u.id != *unit_id);
        Ok(())
    }

    fn list_units(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        self.check_error(&self.errors.borrow().list_units)?;
        Ok(self
            .units
            .borrow()
            .iter()
            .filter(|u| u.group == group && u.workspace == workspace_name)
            .map(|u| RuntimeUnitSnapshot {
                id: u.id.clone(),
                index: u.pane_index,
                cwd: u.cwd.clone(),
                command: u.command.clone(),
                active: u.active,
                kind: PaneKind::Terminal,
                state: PaneState::Idle,
                tty: None,
                process_pid: None,
            })
            .collect())
    }

    fn current_unit(&self, group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let units = self.units.borrow();
        units
            .iter()
            .filter(|u| u.group == group && u.active)
            .map(|u| RuntimeUnitSnapshot {
                id: u.id.clone(),
                index: u.pane_index,
                cwd: u.cwd.clone(),
                command: u.command.clone(),
                active: u.active,
                kind: PaneKind::Terminal,
                state: PaneState::Idle,
                tty: None,
                process_pid: None,
            })
            .next()
            .or_else(|| {
                // fall back to first unit in the group
                units
                    .iter()
                    .find(|u| u.group == group)
                    .map(|u| RuntimeUnitSnapshot {
                        id: u.id.clone(),
                        index: u.pane_index,
                        cwd: u.cwd.clone(),
                        command: u.command.clone(),
                        active: u.active,
                        kind: PaneKind::Terminal,
                        state: PaneState::Idle,
                        tty: None,
                        process_pid: None,
                    })
            })
            .ok_or(RuntimeError::UnitNotFound {
                id: "current".to_owned(),
            })
    }

    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        self.check_error(&self.errors.borrow().send_input)?;
        self.sent_inputs.borrow_mut().push((
            request.target_unit.as_str().to_owned(),
            request.text.clone(),
        ));
        self.sent_input_requests.borrow_mut().push(request);
        Ok(())
    }

    fn set_unit_title(
        &self,
        request: workhive_core::ports::SetUnitTitleRequest,
    ) -> Result<(), RuntimeError> {
        self.title_updates
            .borrow_mut()
            .push((request.target_unit.as_str().to_owned(), request.title));
        Ok(())
    }
}

impl SnapshotRuntimePort for FakeRuntime {
    fn capture_topology(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeTopologySnapshot, RuntimeError> {
        let mut panes = self
            .units
            .borrow()
            .iter()
            .filter(|unit| unit.group == group && unit.workspace == workspace_name)
            .map(|unit| RuntimePaneSnapshot {
                unit_id: unit.id.clone(),
                pane_id: None,
                window_index: unit.window_index,
                pane_index: unit.pane_index,
                cwd: unit.cwd.clone(),
                title: None,
                current_command: Some(unit.command.clone()),
                start_command: Some(unit.command.clone()),
                active: unit.active,
                dead: false,
                launch_origin: derive_fake_launch_origin(&unit.command),
                managed: true,
                tty: None,
                process_pid: None,
            })
            .collect::<Vec<_>>();
        panes.sort_by_key(|pane| (pane.window_index, pane.pane_index));

        let mut windows = panes
            .iter()
            .map(|pane| pane.window_index)
            .collect::<Vec<_>>();
        windows.sort_unstable();
        windows.dedup();

        let active_pane_runtime_id = panes
            .iter()
            .find(|pane| pane.active)
            .map(|pane| pane.unit_id.as_str().to_owned());
        let active_window = panes
            .iter()
            .find(|pane| pane.active)
            .map(|pane| pane.window_index)
            .or_else(|| panes.first().map(|pane| pane.window_index));

        let windows = windows
            .into_iter()
            .map(|index| RuntimeWindowSnapshot {
                index,
                name: Some(format!("window-{index}")),
                layout: "tiled".to_owned(),
                active: Some(index) == active_window,
            })
            .collect::<Vec<_>>();

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
        _name: Option<&str>,
        cwd: &camino::Utf8Path,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        let id = self.alloc_unit_id();
        let snapshot = RuntimePaneSnapshot {
            unit_id: id.clone(),
            pane_id: None,
            window_index,
            pane_index: 0,
            cwd: cwd.to_owned(),
            title: None,
            current_command: Some("zsh".to_owned()),
            start_command: Some("zsh".to_owned()),
            active: false,
            dead: false,
            launch_origin: Some(PaneLaunchOrigin::EmptyShell),
            managed: true,
            tty: None,
            process_pid: None,
        };
        self.units.borrow_mut().push(StoredUnit {
            group: group.to_owned(),
            workspace: workspace_name.to_owned(),
            id,
            window_index,
            pane_index: 0,
            cwd: snapshot.cwd.clone(),
            command: "zsh".to_owned(),
            active: false,
        });
        Ok(snapshot)
    }

    fn split_pane(
        &self,
        group: &str,
        workspace_name: &str,
        target_unit: &UnitId,
        direction: workhive_core::domain::PaneDirection,
        prep: workhive_core::ports::SplitPanePreparation,
    ) -> Result<RuntimePaneSnapshot, RuntimeError> {
        self.check_error(&self.errors.borrow().spawn_unit)?;
        self.split_directions.borrow_mut().push(direction);
        let window_index = self
            .units
            .borrow()
            .iter()
            .find(|unit| unit.id == *target_unit)
            .map(|unit| unit.window_index)
            .unwrap_or(0);
        let id = self.alloc_unit_id();
        let pane_index = self
            .units
            .borrow()
            .iter()
            .filter(|unit| {
                unit.group == group
                    && unit.workspace == workspace_name
                    && unit.window_index == window_index
            })
            .count() as u32;
        // Honor the adapter-level cwd prep so fakes stay faithful to the
        // contract that the real tmux adapter creates the pane in `prep.cwd`
        // directly (no visible `cd --`).
        let cwd = prep
            .cwd
            .clone()
            .unwrap_or_else(|| Utf8PathBuf::from("/tmp"));
        // Similarly, record the pre-baked command as the pane's initial
        // process when supplied, otherwise assume a fresh shell. Real tmux
        // starts a shell implicitly when no command is given.
        let command = prep.command.clone().unwrap_or_else(|| "zsh".to_owned());
        self.units.borrow_mut().push(StoredUnit {
            group: group.to_owned(),
            workspace: workspace_name.to_owned(),
            id: id.clone(),
            window_index,
            pane_index,
            cwd: cwd.clone(),
            command: command.clone(),
            active: false,
        });
        Ok(RuntimePaneSnapshot {
            unit_id: id,
            pane_id: None,
            window_index,
            pane_index,
            cwd,
            title: None,
            current_command: Some(command.clone()),
            start_command: Some(command),
            active: false,
            dead: false,
            launch_origin: Some(PaneLaunchOrigin::EmptyShell),
            managed: true,
            tty: None,
            process_pid: None,
        })
    }

    fn respawn_unit_with_cwd(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        // Mirror the real adapter contract: keep the same unit id and layout
        // slot, kill any in-progress process, and update the recorded cwd.
        let mut units = self.units.borrow_mut();
        let unit = units
            .iter_mut()
            .find(|u| u.group == group && u.workspace == workspace_name && u.id == *unit_id)
            .ok_or_else(|| RuntimeError::UnitNotFound {
                id: unit_id.as_str().to_owned(),
            })?;
        unit.cwd = cwd.to_path_buf();
        // Default tmux behavior on respawn-pane is to launch the user's
        // login shell, mirrored here so subsequent `list_units` rendering
        // is consistent with what the real tmux adapter would emit.
        unit.command = "zsh".to_owned();
        self.respawn_calls
            .borrow_mut()
            .push((unit_id.as_str().to_owned(), cwd.to_path_buf()));
        Ok(())
    }

    fn respawn_unit_with_command(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        // Mirror the real adapter contract: same unit id, same layout slot,
        // existing process killed, the supplied `command` becomes the
        // pane's initial process. Crucially, no visible keystroke is
        // recorded — `send_input` MUST stay empty for any path that uses
        // this verb.
        if let Some(err) = self.errors.borrow().respawn_command.as_ref() {
            return Err(replicate_error(err));
        }
        let mut units = self.units.borrow_mut();
        let unit = units
            .iter_mut()
            .find(|u| u.group == group && u.workspace == workspace_name && u.id == *unit_id)
            .ok_or_else(|| RuntimeError::UnitNotFound {
                id: unit_id.as_str().to_owned(),
            })?;
        unit.cwd = cwd.to_path_buf();
        unit.command = command.to_owned();
        self.respawn_command_calls.borrow_mut().push((
            unit_id.as_str().to_owned(),
            cwd.to_path_buf(),
            command.to_owned(),
        ));
        Ok(())
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

fn derive_fake_launch_origin(command: &str) -> Option<PaneLaunchOrigin> {
    let program = command.split_whitespace().next().unwrap_or(command);
    if matches!(
        program,
        "zsh" | "bash" | "sh" | "fish" | "nu" | "nushell" | "dash" | "ksh"
    ) {
        return Some(PaneLaunchOrigin::EmptyShell);
    }
    if let Ok(name) = ToolName::new(program.to_lowercase()) {
        return Some(PaneLaunchOrigin::Tool { name });
    }
    Some(PaneLaunchOrigin::InlineCommand {
        spec: CommandSpec::new(program, std::iter::empty::<&str>()),
    })
}

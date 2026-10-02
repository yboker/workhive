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
use workhive_core::domain::{PaneDirection, PaneKind, PaneState, RuntimeBinding, UnitId};
use workhive_core::ports::{
    AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, MaterializeConfiguredLayoutRequest,
    MaterializedConfiguredLayout, PaneRuntimePort, RegisterWorkspaceCheckoutRequest, RuntimeError,
    RuntimeUnitSnapshot, SendInputRequest, SpawnUnitRequest, WorkspaceRuntimeInfo,
    WorkspaceRuntimePort,
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
    pub create_workspace: Option<RuntimeError>,
    pub register_workspace_checkout: Option<RuntimeError>,
    pub configure_workspace: Option<RuntimeError>,
    pub attach_workspace: Option<RuntimeError>,
    pub destroy_workspace: Option<RuntimeError>,
    pub destroy_group: Option<RuntimeError>,
    /// Inject failures for the typed `respawn_unit_with_command` verb. This
    /// path replaces visible `send_input` for installing supervisor-wrapped
    /// or inline tool launches without visible keystrokes.
    pub respawn_command: Option<RuntimeError>,
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

impl StoredUnit {
    fn snapshot(&self) -> RuntimeUnitSnapshot {
        RuntimeUnitSnapshot {
            id: self.id.clone(),
            index: self.pane_index,
            cwd: self.cwd.clone(),
            command: self.command.clone(),
            active: self.active,
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        }
    }
}

// ─── FakeRuntime ─────────────────────────────────────────────────────────

/// In-memory fake for `WorkspaceRuntimePort` + `PaneRuntimePort`.
///
/// This fake is runtime-neutral: it stores workspaces and units without any
/// knowledge of tmux. Use it in unit tests for `workhive-core` use cases.
pub struct FakeRuntime {
    workspaces: RefCell<Vec<StoredWorkspace>>,
    workspace_labels: RefCell<HashMap<(String, String), String>>,
    units: RefCell<Vec<StoredUnit>>,
    sent_inputs: RefCell<Vec<(String, String)>>,
    sent_input_requests: RefCell<Vec<SendInputRequest>>,
    split_directions: RefCell<Vec<PaneDirection>>,
    spawn_requests: RefCell<Vec<SpawnUnitRequest>>,
    title_updates: RefCell<Vec<(String, String)>>,
    create_env: RefCell<Vec<RecordedCreateEnv>>,
    /// Records every `respawn_unit_with_cwd` invocation in `(unit_id, cwd)`
    /// form. Tests use this to assert that existing-pane cwd repair goes
    /// through the runtime respawn path, not visible `cd --` keystrokes.
    respawn_calls: RefCell<Vec<(String, Utf8PathBuf)>>,
    /// Records every `respawn_unit_with_command` invocation in
    /// `(unit_id, cwd, command)` form. Tests use this to assert that the
    /// pane supervisor wrapper is installed through the typed runtime launch
    /// verb rather than typed into a visible shell.
    respawn_command_calls: RefCell<Vec<(String, Utf8PathBuf, String)>>,
    materialized_layout: RefCell<Option<MaterializedConfiguredLayout>>,
    materialized_layout_requests: RefCell<Vec<MaterializeConfiguredLayoutRequest>>,
    checkout_registration_binding: RefCell<Option<RuntimeBinding>>,
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
            workspace_labels: RefCell::new(HashMap::new()),
            units: RefCell::new(Vec::new()),
            sent_inputs: RefCell::new(Vec::new()),
            sent_input_requests: RefCell::new(Vec::new()),
            split_directions: RefCell::new(Vec::new()),
            spawn_requests: RefCell::new(Vec::new()),
            title_updates: RefCell::new(Vec::new()),
            create_env: RefCell::new(Vec::new()),
            respawn_calls: RefCell::new(Vec::new()),
            respawn_command_calls: RefCell::new(Vec::new()),
            materialized_layout: RefCell::new(None),
            materialized_layout_requests: RefCell::new(Vec::new()),
            checkout_registration_binding: RefCell::new(None),
            errors: RefCell::new(RuntimeErrorInjection::default()),
            list_workspaces_errors_by_server: RefCell::new(HashMap::new()),
            next_unit_idx: RefCell::new(0),
        }
    }

    /// Set a display label independently from the runtime identity.
    pub fn set_workspace_label(&self, group: &str, runtime_id: &str, label: &str) {
        self.workspace_labels
            .borrow_mut()
            .insert((group.to_owned(), runtime_id.to_owned()), label.to_owned());
    }

    /// Configure the fake to support native configured-layout materialization.
    pub fn set_materialized_layout(&self, result: MaterializedConfiguredLayout) {
        *self.materialized_layout.borrow_mut() = Some(result);
    }

    /// Inspect configured-layout requests received through the runtime seam.
    pub fn materialized_layout_requests(&self) -> Vec<MaterializeConfiguredLayoutRequest> {
        self.materialized_layout_requests.borrow().clone()
    }

    /// Make checkout registration adopt an existing authoritative binding.
    pub fn set_checkout_registration_binding(&self, binding: RuntimeBinding) {
        *self.checkout_registration_binding.borrow_mut() = Some(binding);
    }

    /// Inspect all `(unit_id, cwd)` pairs the runtime port has respawned.
    /// Tests use this for existing-pane cwd correction.
    pub fn respawn_calls(&self) -> Vec<(String, Utf8PathBuf)> {
        self.respawn_calls.borrow().clone()
    }

    /// Inspect all `(unit_id, cwd, command)` triples requested through the
    /// typed runtime launch verb rather than visible shell input.
    pub fn respawn_command_calls(&self) -> Vec<(String, Utf8PathBuf, String)> {
        self.respawn_command_calls.borrow().clone()
    }

    /// Inspect every `(workspace_name, env)` request captured by
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

    /// Inspect split directions requested by layout orchestration.
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
            Err(err.clone())
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
        self.check_error(&self.errors.borrow().create_workspace)?;
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

    fn create_workspace_binding(
        &self,
        request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        let binding = RuntimeBinding::tmux(request.group.clone(), request.workspace_name.clone());
        self.create_workspace(request)?;
        Ok(binding)
    }

    fn register_workspace_checkout(
        &self,
        request: RegisterWorkspaceCheckoutRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        if let Some(error) = &self.errors.borrow().register_workspace_checkout {
            return Err(error.clone());
        }
        Ok(self
            .checkout_registration_binding
            .borrow()
            .clone()
            .unwrap_or(request.binding))
    }

    fn rename_workspace_binding(
        &self,
        from: &RuntimeBinding,
        to: &RuntimeBinding,
    ) -> Result<(), RuntimeError> {
        if from.backend != to.backend || from.scope_id != to.scope_id {
            return Err(RuntimeError::OperationFailed {
                message: "fake runtime cannot rename across backend scopes".to_owned(),
            });
        }
        if from.workspace_id == to.workspace_id {
            return Ok(());
        }
        if self.has_workspace(&to.scope_id, &to.workspace_id)? {
            return Err(RuntimeError::WorkspaceAlreadyExists {
                workspace: to.workspace_id.clone(),
            });
        }
        let mut workspaces = self.workspaces.borrow_mut();
        let Some(workspace) = workspaces.iter_mut().find(|workspace| {
            workspace.group == from.scope_id && workspace.name == from.workspace_id
        }) else {
            return Err(RuntimeError::WorkspaceNotFound {
                workspace: from.workspace_id.clone(),
            });
        };
        workspace.name = to.workspace_id.clone();
        drop(workspaces);

        for unit in self.units.borrow_mut().iter_mut() {
            if unit.group == from.scope_id && unit.workspace == from.workspace_id {
                unit.workspace = to.workspace_id.clone();
            }
        }
        if let Some(label) = self
            .workspace_labels
            .borrow_mut()
            .remove(&(from.scope_id.clone(), from.workspace_id.clone()))
        {
            self.workspace_labels
                .borrow_mut()
                .insert((to.scope_id.clone(), to.workspace_id.clone()), label);
        }
        Ok(())
    }
    fn configure_workspace(
        &self,
        _group: &str,
        _workspace_name: &str,
        _policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.check_error(&self.errors.borrow().configure_workspace)
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

    fn attach_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.attach_workspace(AttachWorkspaceRequest {
            group: binding.scope_id.clone(),
            workspace_name: binding.workspace_id.clone(),
            interaction_policy: policy,
        })
    }

    fn switch_workspace(
        &self,
        _group: &str,
        _target: &str,
        _policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn detach_workspace(&self, _group: &str, _workspace_name: &str) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        self.check_error(&self.errors.borrow().destroy_workspace)?;
        self.workspaces
            .borrow_mut()
            .retain(|w| !(w.group == group && w.name == workspace_name));
        self.units
            .borrow_mut()
            .retain(|u| !(u.group == group && u.workspace == workspace_name));
        Ok(())
    }

    fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        self.destroy_workspace(&binding.scope_id, &binding.workspace_id)
    }

    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        self.check_error(&self.errors.borrow().list_workspaces)?;
        if let Some(err) = self.list_workspaces_errors_by_server.borrow().get(group) {
            return Err(err.clone());
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
                    runtime_id: w.name.clone(),
                    label: self
                        .workspace_labels
                        .borrow()
                        .get(&(w.group.clone(), w.name.clone()))
                        .cloned(),
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
    fn supports_configured_layout(&self) -> bool {
        self.materialized_layout.borrow().is_some()
    }

    fn materialize_configured_layout(
        &self,
        request: MaterializeConfiguredLayoutRequest,
    ) -> Result<MaterializedConfiguredLayout, RuntimeError> {
        self.materialized_layout_requests.borrow_mut().push(request);
        self.materialized_layout
            .borrow()
            .clone()
            .ok_or_else(|| RuntimeError::OperationFailed {
                message: "fake native layout result is not configured".to_owned(),
            })
    }

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
            .map(StoredUnit::snapshot)
            .collect())
    }

    fn current_unit(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let units = self.units.borrow();
        units
            .iter()
            .filter(|u| u.group == group && u.workspace == workspace_name && u.active)
            .map(StoredUnit::snapshot)
            .next()
            .or_else(|| {
                // Fall back only within the requested workspace.
                units
                    .iter()
                    .find(|u| u.group == group && u.workspace == workspace_name)
                    .map(StoredUnit::snapshot)
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
            return Err(err.clone());
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
}

//! Private Herdr workspace client.
//!
//! Herdr's wire format and transport stay here. The rest of Workhive sees
//! only the existing synchronous runtime ports and opaque runtime IDs.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use workhive_core::domain::{RuntimeBackendId, RuntimeBinding, UnitId};
use workhive_core::ports::{
    AttachWorkspaceRequest, ConfiguredLayoutNode, CreateWorkspaceRuntimeRequest,
    MaterializeConfiguredLayoutRequest, MaterializedConfiguredLayout, MaterializedLayoutPane,
    PaneRuntimePort, RegisterWorkspaceCheckoutRequest, RuntimeError, RuntimeUnitSnapshot,
    SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest, WorkspaceRuntimeInfo,
    WorkspaceRuntimePort,
};

const SUPPORTED_PROTOCOL: u64 = 22;

#[derive(Debug, Clone, Deserialize)]
pub struct HerdrSessionSnapshot {
    pub version: String,
    pub protocol: u32,
    pub workspaces: Vec<HerdrWorkspaceSnapshot>,
    pub tabs: Vec<HerdrTabSnapshot>,
    pub panes: Vec<HerdrPaneSnapshot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HerdrWorkspaceSnapshot {
    pub workspace_id: String,
    pub number: u32,
    pub label: String,
    pub focused: bool,
    pub pane_count: u32,
    pub tab_count: u32,
    pub active_tab_id: String,
    pub agent_status: String,
    #[serde(default)]
    pub worktree: Option<HerdrWorktreeSnapshot>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HerdrWorktreeSnapshot {
    pub repo_key: String,
    pub repo_name: String,
    pub repo_root: String,
    pub checkout_path: String,
    pub is_linked_worktree: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HerdrTabSnapshot {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: u32,
    pub label: String,
    pub focused: bool,
    pub pane_count: u32,
    pub agent_status: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HerdrPaneSnapshot {
    pub pane_id: String,
    pub terminal_id: String,
    pub workspace_id: String,
    pub tab_id: String,
    pub focused: bool,
    #[serde(default)]
    pub agent_status: Option<String>,
    pub revision: u64,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub terminal_title: Option<String>,
    #[serde(default)]
    pub terminal_title_stripped: Option<String>,
}

pub struct HerdrPreflight {
    pub installed: bool,
    pub startable: bool,
    pub running: bool,
    pub compatible: bool,
    pub detail: Option<String>,
}

#[cfg(unix)]
pub struct HerdrDependencyProvider;

#[cfg(unix)]
impl HerdrDependencyProvider {
    pub fn preflight(socket_path: impl Into<std::path::PathBuf>) -> HerdrPreflight {
        let installed = which::which("herdr").is_ok();
        if !installed {
            return HerdrPreflight {
                installed: false,
                startable: false,
                running: false,
                compatible: false,
                detail: Some("herdr executable was not found on PATH".to_owned()),
            };
        }

        let socket_path = socket_path.into();
        match HerdrClient::connect_with_scope(
            UnixSocketTransport::new(socket_path.clone()),
            socket_path.to_string_lossy().to_string(),
        ) {
            Ok(_) => HerdrPreflight {
                installed: true,
                startable: true,
                running: true,
                compatible: true,
                detail: None,
            },
            Err(error) => HerdrPreflight {
                installed: true,
                startable: true,
                running: false,
                compatible: false,
                detail: Some(error.to_string()),
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum HerdrClientError {
    #[error("failed to connect to Herdr socket {path}: {message}")]
    Connect {
        path: std::path::PathBuf,
        kind: std::io::ErrorKind,
        message: String,
    },
    #[error("Herdr transport failed: {0}")]
    Transport(String),
    #[error("unsupported Herdr protocol {protocol}")]
    Incompatible { protocol: u64 },
    #[error("Herdr API error {code}: {message}")]
    Api { code: String, message: String },
    #[error("invalid Herdr response: {0}")]
    InvalidResponse(String),
}

/// Transport boundary used by the client and deterministic tests.
trait HerdrTransport {
    fn exchange(&self, request: &HerdrRequest) -> Result<HerdrResponse, HerdrClientError>;
}

#[derive(Debug, Clone)]
struct HerdrRequest {
    id: String,
    method: String,
    params: Value,
}

#[derive(Debug, Clone)]
struct HerdrResponse {
    result: Option<Value>,
    error: Option<HerdrResponseError>,
}

#[derive(Debug, Clone)]
struct HerdrResponseError {
    code: String,
    message: String,
}

/// Synchronous Herdr protocol client. Its transport and wire types stay
/// private so callers depend only on Workhive's runtime ports.
struct HerdrClient<T> {
    scope_id: String,
    transport: T,
    next_id: AtomicU64,
}

impl<T: HerdrTransport> HerdrClient<T> {
    /// Establish a client only after the server protocol is validated.
    #[cfg(test)]
    fn connect(transport: T) -> Result<Self, HerdrClientError> {
        Self::connect_with_scope(transport, "herdr")
    }

    fn connect_with_scope(
        transport: T,
        scope_id: impl Into<String>,
    ) -> Result<Self, HerdrClientError> {
        let client = Self {
            scope_id: scope_id.into(),
            transport,
            next_id: AtomicU64::new(1),
        };
        client.probe()?;
        Ok(client)
    }

    fn create_workspace_binding(
        &self,
        request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        let display_label = request.workspace_name.clone();
        self.create_workspace_binding_labeled(request, &display_label)
    }

    fn create_workspace_binding_labeled(
        &self,
        request: CreateWorkspaceRuntimeRequest,
        display_label: &str,
    ) -> Result<RuntimeBinding, RuntimeError> {
        let result = self
            .call_unchecked(
                "workspace.create",
                json!({
                    "label": display_label,
                    "cwd": request.working_dir.as_str(),
                    "env": request.env.into_iter().collect::<std::collections::HashMap<_, _>>(),
                    "focus": false,
                }),
            )
            .map_err(map_client_error)?;
        let workspace = result
            .get("workspace")
            .ok_or_else(|| invalid_response("workspace.create did not return workspace"))?;
        let workspace_id = workspace
            .get("workspace_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("workspace.create.workspace_id is missing"))?;
        RuntimeBinding::new(
            RuntimeBackendId::new("herdr").expect("static backend id is non-empty"),
            self.scope_id.clone(),
            workspace_id,
        )
        .ok_or_else(|| invalid_response("workspace.create returned an empty runtime identity"))
    }

    fn register_existing_worktree(
        &self,
        binding: &RuntimeBinding,
        project_root: &camino::Utf8Path,
        workspace_root: &camino::Utf8Path,
    ) -> Result<RuntimeBinding, RuntimeError> {
        self.binding_params(binding)?;
        let result = self.call(
            "worktree.open",
            json!({
                "cwd": project_root.as_str(),
                "path": workspace_root.as_str(),
                "focus": false,
            }),
        )?;
        let registered_workspace_id = result
            .get("workspace")
            .and_then(|workspace| workspace.get("workspace_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("worktree.open.workspace.workspace_id is missing"))?;
        if registered_workspace_id != binding.workspace_id && project_root != workspace_root {
            return Err(RuntimeError::OperationFailed {
                message: format!(
                    "Herdr registered checkout '{}' as workspace '{}', expected '{}'",
                    workspace_root, registered_workspace_id, binding.workspace_id
                ),
            });
        }
        RuntimeBinding::new(
            binding.backend.clone(),
            binding.scope_id.clone(),
            registered_workspace_id,
        )
        .ok_or_else(|| invalid_response("worktree.open returned an empty workspace identity"))
    }

    fn active_tab_id(&self, workspace_id: &str) -> Result<String, RuntimeError> {
        let listed = self.call("workspace.list", json!({}))?;
        listed
            .get("workspaces")
            .and_then(Value::as_array)
            .and_then(|workspaces| {
                workspaces.iter().find(|workspace| {
                    workspace.get("workspace_id").and_then(Value::as_str) == Some(workspace_id)
                })
            })
            .ok_or_else(|| RuntimeError::WorkspaceNotFound {
                workspace: workspace_id.to_owned(),
            })?
            .get("active_tab_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| invalid_response("workspace.active_tab_id is missing"))
    }

    fn probe(&self) -> Result<(), HerdrClientError> {
        let result = self.call_unchecked("ping", json!({}))?;
        let protocol = result
            .get("protocol")
            .and_then(Value::as_u64)
            .ok_or_else(|| HerdrClientError::InvalidResponse("ping.protocol is missing".into()))?;
        if protocol != SUPPORTED_PROTOCOL {
            return Err(HerdrClientError::Incompatible { protocol });
        }
        Ok(())
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, RuntimeError> {
        self.call_unchecked(method, params)
            .map_err(map_client_error)
    }

    fn session_snapshot(&self) -> Result<HerdrSessionSnapshot, RuntimeError> {
        let result = self.call("session.snapshot", json!({}))?;
        let snapshot = result.get("snapshot").unwrap_or(&result);
        serde_json::from_value(snapshot.clone()).map_err(|error| {
            invalid_response(&format!("session.snapshot payload is invalid: {error}"))
        })
    }

    fn call_for_workspace(
        &self,
        method: &str,
        params: Value,
        workspace_id: &str,
    ) -> Result<Value, RuntimeError> {
        self.call_unchecked(method, params)
            .map_err(|error| map_workspace_client_error(error, workspace_id))
    }

    fn send_agent_prompt(&self, pane_id: &UnitId, text: &str) -> Result<bool, RuntimeError> {
        let pane = self.call("pane.get", json!({ "pane_id": pane_id.as_str() }))?;
        let agent_status = pane
            .get("pane")
            .unwrap_or(&pane)
            .get("agent_status")
            .and_then(Value::as_str);
        if !matches!(agent_status, Some("idle" | "working" | "blocked" | "done")) {
            return Ok(false);
        }

        match self.call_unchecked(
            "agent.prompt",
            json!({ "target": pane_id.as_str(), "text": text }),
        ) {
            Ok(_) => Ok(true),
            Err(HerdrClientError::Api { code, .. })
                if matches!(
                    code.as_str(),
                    "agent_not_found"
                        | "capability_unavailable"
                        | "method_not_found"
                        | "unknown_method"
                        | "unsupported_method"
                ) =>
            {
                Ok(false)
            }
            Err(error) => Err(map_client_error(error)),
        }
    }

    fn call_unchecked(&self, method: &str, params: Value) -> Result<Value, HerdrClientError> {
        let id = format!("workhive-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        let response = self.transport.exchange(&HerdrRequest {
            id,
            method: method.to_owned(),
            params,
        })?;
        if let Some(error) = response.error {
            return Err(HerdrClientError::Api {
                code: error.code,
                message: error.message,
            });
        }
        response
            .result
            .ok_or_else(|| HerdrClientError::InvalidResponse("result is missing".into()))
    }

    fn binding_params(&self, binding: &RuntimeBinding) -> Result<Value, RuntimeError> {
        if binding.backend != RuntimeBackendId::herdr() {
            return Err(RuntimeError::OperationFailed {
                message: format!(
                    "Herdr adapter cannot operate on '{}' runtime bindings",
                    binding.backend
                ),
            });
        }
        if binding.scope_id != self.scope_id {
            return Err(RuntimeError::OperationFailed {
                message: format!(
                    "Herdr runtime binding belongs to scope '{}', not '{}'",
                    binding.scope_id, self.scope_id
                ),
            });
        }
        Ok(json!({ "workspace_id": binding.workspace_id }))
    }

    /// Attach an existing workspace by its persisted opaque runtime identity.
    fn attach_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        _policy: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        let params = self.binding_params(binding)?;
        self.call_for_workspace("workspace.focus", params, &binding.workspace_id)?;
        Ok(())
    }

    fn detach_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        // Herdr keeps detached workspaces alive; there is no detach request.
        // The persisted binding remains authoritative and no runtime mutation
        // is needed for this operation.
        self.binding_params(binding)?;
        Ok(())
    }

    fn close_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        let mut params = self.binding_params(binding)?;
        params["close_group"] = json!(false);
        self.call_for_workspace("workspace.close", params, &binding.workspace_id)?;
        Ok(())
    }

    fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        self.close_workspace_binding(binding)
    }

    fn unsupported<U>(&self, capability: &str) -> Result<U, RuntimeError> {
        Err(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing {
                capability: capability.to_owned(),
            },
            detail: format!("Herdr capability is not available: {capability}"),
        })
    }
}

fn map_client_error(error: HerdrClientError) -> RuntimeError {
    let detail = error.to_string();
    match error {
        HerdrClientError::Connect {
            path,
            kind: std::io::ErrorKind::NotFound,
            ..
        } => RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::SocketNotFound {
                path: path.to_string_lossy().into_owned(),
            },
            detail,
        },
        HerdrClientError::Connect {
            path,
            kind: std::io::ErrorKind::ConnectionRefused,
            ..
        } => RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                server: path.to_string_lossy().into_owned(),
            },
            detail,
        },
        HerdrClientError::Connect {
            kind: std::io::ErrorKind::PermissionDenied,
            ..
        } => RuntimeError::PermissionDenied { detail },
        HerdrClientError::Connect { .. } | HerdrClientError::Transport(_) => {
            RuntimeError::BackendUnavailable {
                reason: workhive_core::ports::BackendUnavailableReason::Other,
                detail,
            }
        }
        HerdrClientError::Incompatible { protocol } => RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing {
                capability: format!(
                    "compatible Herdr protocol (server protocol {protocol}, supported {SUPPORTED_PROTOCOL})"
                ),
            },
            detail,
        },
        HerdrClientError::Api { .. } | HerdrClientError::InvalidResponse(_) => {
            RuntimeError::OperationFailed { message: detail }
        }
    }
}

fn map_workspace_client_error(error: HerdrClientError, workspace_id: &str) -> RuntimeError {
    match error {
        HerdrClientError::Api { code, .. } if code == "workspace_not_found" => {
            RuntimeError::WorkspaceNotFound {
                workspace: workspace_id.to_owned(),
            }
        }
        HerdrClientError::Api { code, .. } if code == "workspace_group_close_required" => {
            RuntimeError::WorkspaceGroupCloseRequired {
                workspace: workspace_id.to_owned(),
            }
        }
        other => map_client_error(other),
    }
}

fn workspace_info(value: &Value) -> Result<WorkspaceRuntimeInfo, RuntimeError> {
    let workspace = value.get("workspace").unwrap_or(value);
    let label = workspace
        .get("label")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("workspace.label is missing"))?;
    let runtime_id = workspace
        .get("workspace_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("workspace.workspace_id is missing"))?;
    let unit_count = workspace
        .get("pane_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid_response("workspace.pane_count is missing"))?;
    Ok(WorkspaceRuntimeInfo {
        runtime_id: runtime_id.to_owned(),
        label: Some(label.to_owned()),
        unit_count: u32::try_from(unit_count)
            .map_err(|_| invalid_response("workspace.unit_count is too large"))?,
        attached: workspace
            .get("focused")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn invalid_response(message: &str) -> RuntimeError {
    RuntimeError::OperationFailed {
        message: format!("invalid Herdr response: {message}"),
    }
}

impl<T: HerdrTransport> WorkspaceRuntimePort for HerdrClient<T> {
    fn has_workspace(&self, _group: &str, workspace_name: &str) -> Result<bool, RuntimeError> {
        let result = self.call("workspace.list", json!({}))?;
        let values = result
            .get("workspaces")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("workspace.list.workspaces is missing"))?;
        Ok(values.iter().any(|workspace| {
            workspace.get("workspace_id").and_then(Value::as_str) == Some(workspace_name)
        }))
    }

    fn has_workspace_binding(&self, binding: &RuntimeBinding) -> Result<bool, RuntimeError> {
        self.binding_params(binding)?;
        self.has_workspace(&binding.scope_id, &binding.workspace_id)
    }

    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
        HerdrClient::create_workspace_binding(self, request).map(|_| ())
    }

    fn create_workspace_binding(
        &self,
        request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        HerdrClient::create_workspace_binding(self, request)
    }

    fn create_workspace_binding_with_label(
        &self,
        request: CreateWorkspaceRuntimeRequest,
        display_label: &str,
    ) -> Result<RuntimeBinding, RuntimeError> {
        self.create_workspace_binding_labeled(request, display_label)
    }

    fn register_workspace_checkout(
        &self,
        request: RegisterWorkspaceCheckoutRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        self.register_existing_worktree(
            &request.binding,
            request.project_root.as_ref(),
            request.workspace_root.as_ref(),
        )
    }

    fn configure_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        _policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.binding_params(binding).map(|_| ())
    }

    fn configure_workspace(
        &self,
        _group: &str,
        _workspace_name: &str,
        _policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.unsupported("workspace.configure requires a persisted runtime binding")
    }

    fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        self.unsupported("workspace.attach requires a persisted runtime binding")
    }

    fn attach_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        HerdrClient::attach_workspace_binding(self, binding, policy)
    }

    fn switch_workspace(
        &self,
        _group: &str,
        _target_workspace: &str,
        _policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.unsupported("workspace.switch requires a persisted runtime binding")
    }

    fn detach_workspace(&self, _group: &str, _workspace_name: &str) -> Result<(), RuntimeError> {
        self.unsupported("workspace.detach requires a persisted runtime binding")
    }

    fn detach_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        HerdrClient::detach_workspace_binding(self, binding)
    }

    fn close_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        HerdrClient::close_workspace_binding(self, binding)
    }

    fn destroy_workspace(&self, _group: &str, _workspace_name: &str) -> Result<(), RuntimeError> {
        self.unsupported("workspace.destroy requires a persisted runtime binding")
    }

    fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        HerdrClient::destroy_workspace_binding(self, binding)
    }

    fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        let result = self.call("workspace.list", json!({}))?;
        let values = result
            .get("workspaces")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("workspace.list.workspaces is missing"))?;
        values.iter().map(workspace_info).collect()
    }

    fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
        self.unsupported("destroying an entire Herdr socket scope")
    }
}

fn pane_snapshot_from_herdr(
    value: &Value,
    index: u32,
) -> Result<RuntimeUnitSnapshot, RuntimeError> {
    let pane = value.get("pane").unwrap_or(value);
    let pane_id = pane
        .get("pane_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_response("pane.pane_id is missing"))?;
    let cwd = pane
        .get("cwd")
        .and_then(Value::as_str)
        .map(camino::Utf8PathBuf::from)
        .unwrap_or_default();
    let active = pane
        .get("focused")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let agent_status = pane.get("agent_status").and_then(Value::as_str);
    let (state, kind) = match agent_status {
        Some("working") => (
            workhive_core::domain::PaneState::Running,
            workhive_core::domain::PaneKind::Agent,
        ),
        Some("idle" | "blocked" | "done") => (
            workhive_core::domain::PaneState::Idle,
            workhive_core::domain::PaneKind::Agent,
        ),
        Some("unknown") | None => (
            workhive_core::domain::PaneState::Idle,
            workhive_core::domain::PaneKind::Terminal,
        ),
        Some(other) => {
            tracing::debug!(status = other, "unrecognized Herdr agent status");
            (
                workhive_core::domain::PaneState::Idle,
                workhive_core::domain::PaneKind::Terminal,
            )
        }
    };
    Ok(RuntimeUnitSnapshot {
        id: UnitId::new(pane_id),
        index,
        cwd,
        command: String::new(),
        active,
        kind,
        state,
        tty: None,
        process_pid: None,
    })
}

fn configured_layout_node_to_herdr(node: &ConfiguredLayoutNode) -> Result<Value, RuntimeError> {
    match node {
        ConfiguredLayoutNode::Pane(pane) => {
            let mut value = json!({
                "type": "pane",
                "cwd": pane.cwd.as_str(),
                "env": pane.env.iter().cloned().collect::<std::collections::HashMap<_, _>>(),
                "label": pane.title,
            });
            if let Some(command) = &pane.command {
                value["command"] = json!(["sh", "-c", command]);
            }
            Ok(value)
        }
        ConfiguredLayoutNode::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let direction = match direction {
                workhive_core::domain::PaneDirection::Right => "right",
                workhive_core::domain::PaneDirection::Down => "down",
                workhive_core::domain::PaneDirection::Left
                | workhive_core::domain::PaneDirection::Up => {
                    return Err(RuntimeError::OperationFailed {
                        message: "Herdr configured layouts support right and down splits only"
                            .to_owned(),
                    });
                }
            };
            Ok(json!({
                "type": "split",
                "direction": direction,
                "ratio": ratio,
                "first": configured_layout_node_to_herdr(first)?,
                "second": configured_layout_node_to_herdr(second)?,
            }))
        }
    }
}

fn collect_materialized_layout_panes(
    configured: &ConfiguredLayoutNode,
    materialized: &Value,
    index: &mut u32,
    panes: &mut Vec<MaterializedLayoutPane>,
) -> Result<(), RuntimeError> {
    match configured {
        ConfiguredLayoutNode::Pane(configured_pane) => {
            let mut unit = pane_snapshot_from_herdr(materialized, *index)?;
            unit.command = configured_pane.command.clone().unwrap_or_default();
            panes.push(MaterializedLayoutPane {
                position: configured_pane.position,
                unit,
            });
            *index += 1;
            Ok(())
        }
        ConfiguredLayoutNode::Split { first, second, .. } => {
            let materialized_first = materialized
                .get("first")
                .ok_or_else(|| invalid_response("layout.apply.layout.root.first is missing"))?;
            let materialized_second = materialized
                .get("second")
                .ok_or_else(|| invalid_response("layout.apply.layout.root.second is missing"))?;
            collect_materialized_layout_panes(first, materialized_first, index, panes)?;
            collect_materialized_layout_panes(second, materialized_second, index, panes)
        }
    }
}

impl<T: HerdrTransport> PaneRuntimePort for HerdrClient<T> {
    fn supports_configured_layout(&self) -> bool {
        true
    }

    fn materialize_configured_layout(
        &self,
        request: MaterializeConfiguredLayoutRequest,
    ) -> Result<MaterializedConfiguredLayout, RuntimeError> {
        let tab_id = self.active_tab_id(&request.workspace_id)?;
        let result = self.call(
            "layout.apply",
            json!({
                "tab_id": tab_id,
                "focus": false,
                "root": configured_layout_node_to_herdr(&request.root)?,
            }),
        )?;
        let root = result
            .get("layout")
            .and_then(|layout| layout.get("root"))
            .ok_or_else(|| invalid_response("layout.apply.layout.root is missing"))?;
        let mut panes = Vec::new();
        let mut index = 0;
        collect_materialized_layout_panes(&request.root, root, &mut index, &mut panes)?;
        Ok(MaterializedConfiguredLayout { panes })
    }

    fn spawn_unit(&self, request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let direction_str = match request.direction {
            workhive_core::domain::PaneDirection::Right => "right",
            workhive_core::domain::PaneDirection::Down => "down",
            workhive_core::domain::PaneDirection::Left
            | workhive_core::domain::PaneDirection::Up => {
                return self.unsupported("pane.split before-target direction");
            }
        };
        let env_map: std::collections::HashMap<_, _> = request.env.into_iter().collect();
        let mut params = json!({
            "direction": direction_str,
            "focus": true,
            "workspace_id": request.workspace_name.clone(),
        });
        if let Some(cwd) = request.cwd {
            params["cwd"] = json!(cwd.as_str());
        }
        if !env_map.is_empty() {
            params["env"] = json!(env_map);
        }
        if let Some(target) = request.target_unit {
            params["target_pane_id"] = json!(target.as_str());
        }
        if let Some(ratio_pct) = request.size_percent {
            params["ratio"] = json!(f64::from(ratio_pct) / 100.0);
        }

        let result = self.call("pane.split", params)?;

        let mut current = if result.get("pane").is_some() || result.get("pane_id").is_some() {
            pane_snapshot_from_herdr(&result, 0)?
        } else {
            self.current_unit(&request.group, &request.workspace_name)?
        };

        if let Ok(panes) = self.list_units(&request.group, &request.workspace_name)
            && let Some((idx, p)) = panes
                .into_iter()
                .enumerate()
                .find(|(_, p)| p.id == current.id)
        {
            current.index = idx as u32;
            if current.cwd.as_str().is_empty() {
                current.cwd = p.cwd;
            }
        }

        if let Some(cmd) = request.command {
            self.send_input(SendInputRequest {
                group: request.group,
                target_unit: current.id.clone(),
                text: cmd,
                enter: true,
            })?;
        }
        Ok(current)
    }

    fn close_unit(&self, _group: &str, unit_id: &UnitId) -> Result<(), RuntimeError> {
        let pane = self.call("pane.get", json!({ "pane_id": unit_id.as_str() }))?;
        let workspace_id = pane
            .get("pane")
            .unwrap_or(&pane)
            .get("workspace_id")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid_response("pane.get.pane.workspace_id is missing"))?;
        if self.list_units("", workspace_id)?.len() <= 1 {
            return Err(RuntimeError::LastUnitCloseRejected);
        }
        self.call("pane.close", json!({ "pane_id": unit_id.as_str() }))?;
        Ok(())
    }

    fn list_units(
        &self,
        _group: &str,
        workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        let params = if workspace_name.is_empty() {
            json!({})
        } else {
            json!({ "workspace_id": workspace_name })
        };
        let result = self.call("pane.list", params)?;
        let panes = result
            .get("panes")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("pane.list.panes is missing"))?;
        panes
            .iter()
            .enumerate()
            .map(|(i, val)| pane_snapshot_from_herdr(val, i as u32))
            .collect()
    }

    fn current_unit(
        &self,
        _group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        let active_tab_id = self.active_tab_id(workspace_name)?;
        let result = self.call("pane.list", json!({ "workspace_id": workspace_name }))?;
        let panes = result
            .get("panes")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_response("pane.list.panes is missing"))?;
        let mut active_tab_panes = panes
            .iter()
            .enumerate()
            .filter(|(_, pane)| {
                pane.get("tab_id").and_then(Value::as_str) == Some(active_tab_id.as_str())
            })
            .map(|(index, pane)| pane_snapshot_from_herdr(pane, index as u32))
            .collect::<Result<Vec<_>, _>>()?;
        let index = active_tab_panes
            .iter()
            .position(|pane| pane.active)
            .unwrap_or(0);
        active_tab_panes
            .drain(..)
            .nth(index)
            .ok_or_else(|| RuntimeError::UnitNotFound {
                id: format!("current pane in workspace {workspace_name}"),
            })
    }

    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        if request.enter
            && !request.text.is_empty()
            && self.send_agent_prompt(&request.target_unit, &request.text)?
        {
            return Ok(());
        }

        self.call(
            "pane.send_text",
            json!({
                "pane_id": request.target_unit.as_str(),
                "text": request.text
            }),
        )?;
        if request.enter {
            self.call(
                "pane.send_keys",
                json!({
                    "pane_id": request.target_unit.as_str(),
                    "keys": ["enter"]
                }),
            )?;
        }
        Ok(())
    }

    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
        self.call(
            "pane.rename",
            json!({
                "pane_id": request.target_unit.as_str(),
                "label": request.title
            }),
        )?;
        Ok(())
    }

    fn respawn_unit_with_cwd(
        &self,
        _group: &str,
        _workspace_name: &str,
        _unit_id: &UnitId,
        _cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        self.unsupported("pane respawn")
    }

    fn respawn_unit_with_command(
        &self,
        _group: &str,
        _workspace_name: &str,
        _unit_id: &UnitId,
        _cwd: &camino::Utf8Path,
        _command: &str,
    ) -> Result<(), RuntimeError> {
        self.unsupported("pane respawn")
    }
}

#[cfg(unix)]
struct UnixSocketTransport {
    socket_path: std::path::PathBuf,
}

#[cfg(unix)]
impl UnixSocketTransport {
    fn new(socket_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }
}

#[cfg(unix)]
impl HerdrTransport for UnixSocketTransport {
    fn exchange(&self, request: &HerdrRequest) -> Result<HerdrResponse, HerdrClientError> {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let mut stream =
            UnixStream::connect(&self.socket_path).map_err(|error| HerdrClientError::Connect {
                path: self.socket_path.clone(),
                kind: error.kind(),
                message: error.to_string(),
            })?;
        let wire = json!({ "id": request.id, "method": request.method, "params": request.params });
        writeln!(stream, "{wire}")
            .map_err(|error| HerdrClientError::Transport(error.to_string()))?;
        let mut line = String::new();
        BufReader::new(&mut stream)
            .read_line(&mut line)
            .map_err(|error| HerdrClientError::Transport(error.to_string()))?;
        let response: Value = serde_json::from_str(&line)
            .map_err(|error| HerdrClientError::Transport(error.to_string()))?;
        Ok(parse_response(response))
    }
}

fn parse_response(value: Value) -> HerdrResponse {
    let error = value.get("error").map(|error| HerdrResponseError {
        code: error
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned(),
        message: error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown Herdr error")
            .to_owned(),
    });
    HerdrResponse {
        result: value.get("result").cloned(),
        error,
    }
}
/// Validated synchronous Herdr adapter for Workhive runtime ports.
#[cfg(unix)]
pub struct HerdrRuntimeAdapter {
    client: HerdrClient<UnixSocketTransport>,
}

#[cfg(unix)]
impl HerdrRuntimeAdapter {
    /// Connect to Herdr after validating its protocol.
    pub fn connect(socket_path: impl Into<std::path::PathBuf>) -> Result<Self, RuntimeError> {
        let socket_path = socket_path.into();
        HerdrClient::connect_with_scope(
            UnixSocketTransport::new(socket_path.clone()),
            socket_path.to_string_lossy().to_string(),
        )
        .map(|client| Self { client })
        .map_err(map_client_error)
    }

    /// Read the native session hierarchy from Herdr's documented snapshot API.
    pub fn session_snapshot(&self) -> Result<HerdrSessionSnapshot, RuntimeError> {
        self.client.session_snapshot()
    }
}

#[cfg(unix)]
fn validate_herdr_session_name(session: &str, action: &str) -> Result<(), RuntimeError> {
    if session.trim().is_empty() {
        return Err(RuntimeError::OperationFailed {
            message: format!("cannot {action} an empty Herdr session name"),
        });
    }
    Ok(())
}

/// Resume a native Herdr session through Herdr's own session entry point.
#[cfg(unix)]
pub fn resume_session(session: &str) -> Result<(), RuntimeError> {
    validate_herdr_session_name(session, "resume")?;

    let mut command = std::process::Command::new(crate::dependency::herdr_program());
    command
        .arg("--session")
        .arg(session)
        // Workhive must not let an inherited test-only socket override
        // redirect a production named-session client.
        .env_remove("HERDR_SOCKET_PATH");
    use std::os::unix::process::CommandExt;
    Err(map_herdr_launch_error(command.exec()))
}

/// Invoke Herdr's native whole-session attach command.
#[cfg(unix)]
pub fn attach_session(session: &str) -> Result<(), RuntimeError> {
    validate_herdr_session_name(session, "attach to")?;
    let status = std::process::Command::new(crate::dependency::herdr_program())
        .args(["session", "attach", session])
        .env_remove("HERDR_SOCKET_PATH")
        .status()
        .map_err(map_herdr_launch_error)?;
    if status.success() {
        Ok(())
    } else {
        Err(RuntimeError::OperationFailed {
            message: format!("Herdr session attach exited with status {status}"),
        })
    }
}

#[cfg(unix)]
fn herdr_tui_command(socket_path: &str) -> std::process::Command {
    let mut command = std::process::Command::new("herdr");
    command.env("HERDR_SOCKET_PATH", socket_path);
    command
}

fn map_herdr_launch_error(error: std::io::Error) -> RuntimeError {
    if error.kind() == std::io::ErrorKind::NotFound {
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::BackendBinaryNotFound {
                binary: "herdr".to_owned(),
            },
            detail: error.to_string(),
        }
    } else {
        RuntimeError::OperationFailed {
            message: format!("failed to launch Herdr client: {error}"),
        }
    }
}

#[cfg(unix)]
impl WorkspaceRuntimePort for HerdrRuntimeAdapter {
    fn has_workspace(&self, group: &str, workspace_name: &str) -> Result<bool, RuntimeError> {
        WorkspaceRuntimePort::has_workspace(&self.client, group, workspace_name)
    }

    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::create_workspace(&self.client, request)
    }

    fn create_workspace_binding(
        &self,
        request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        WorkspaceRuntimePort::create_workspace_binding(&self.client, request)
    }

    fn create_workspace_binding_with_label(
        &self,
        request: CreateWorkspaceRuntimeRequest,
        display_label: &str,
    ) -> Result<RuntimeBinding, RuntimeError> {
        WorkspaceRuntimePort::create_workspace_binding_with_label(
            &self.client,
            request,
            display_label,
        )
    }

    fn register_workspace_checkout(
        &self,
        request: RegisterWorkspaceCheckoutRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        WorkspaceRuntimePort::register_workspace_checkout(&self.client, request)
    }

    fn configure_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::configure_workspace_binding(&self.client, binding, policy)
    }

    fn configure_workspace(
        &self,
        group: &str,
        workspace_name: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::configure_workspace(&self.client, group, workspace_name, policy)
    }

    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::attach_workspace(&self.client, request)
    }

    fn attach_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::attach_workspace_binding(&self.client, binding, policy)?;
        let status = herdr_tui_command(&binding.scope_id)
            .status()
            .map_err(map_herdr_launch_error)?;
        if status.success() {
            Ok(())
        } else {
            Err(RuntimeError::OperationFailed {
                message: format!("Herdr client exited with status {status}"),
            })
        }
    }

    fn switch_workspace(
        &self,
        group: &str,
        target_workspace: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::switch_workspace(&self.client, group, target_workspace, policy)
    }

    fn detach_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::detach_workspace(&self.client, group, workspace_name)
    }

    fn detach_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::detach_workspace_binding(&self.client, binding)
    }

    fn close_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::close_workspace_binding(&self.client, binding)
    }

    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::destroy_workspace(&self.client, group, workspace_name)
    }

    fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::destroy_workspace_binding(&self.client, binding)
    }

    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        WorkspaceRuntimePort::list_workspaces(&self.client, group)
    }

    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError> {
        WorkspaceRuntimePort::destroy_group(&self.client, group)
    }
}

#[cfg(unix)]
impl PaneRuntimePort for HerdrRuntimeAdapter {
    fn supports_configured_layout(&self) -> bool {
        true
    }

    fn materialize_configured_layout(
        &self,
        request: MaterializeConfiguredLayoutRequest,
    ) -> Result<MaterializedConfiguredLayout, RuntimeError> {
        PaneRuntimePort::materialize_configured_layout(&self.client, request)
    }

    fn spawn_unit(&self, request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        PaneRuntimePort::spawn_unit(&self.client, request)
    }

    fn close_unit(&self, group: &str, unit_id: &UnitId) -> Result<(), RuntimeError> {
        PaneRuntimePort::close_unit(&self.client, group, unit_id)
    }

    fn list_units(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        PaneRuntimePort::list_units(&self.client, group, workspace_name)
    }

    fn current_unit(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        PaneRuntimePort::current_unit(&self.client, group, workspace_name)
    }

    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        PaneRuntimePort::send_input(&self.client, request)
    }

    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
        PaneRuntimePort::set_unit_title(&self.client, request)
    }

    fn respawn_unit_with_cwd(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        PaneRuntimePort::respawn_unit_with_cwd(&self.client, group, workspace_name, unit_id, cwd)
    }

    fn respawn_unit_with_command(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        PaneRuntimePort::respawn_unit_with_command(
            &self.client,
            group,
            workspace_name,
            unit_id,
            cwd,
            command,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::sync::Mutex;

    struct FakeHerdr {
        workspaces: Mutex<Vec<Value>>,
        tabs: Mutex<Vec<Value>>,
        panes: Mutex<Vec<Value>>,
        focused_pane_id: Mutex<Option<String>>,
        requests: std::sync::Arc<Mutex<Vec<HerdrRequest>>>,
        close_refusal: std::sync::Arc<Mutex<Option<String>>>,
        inputs: std::sync::Arc<Mutex<Vec<(String, String)>>>,
        agent_prompts: std::sync::Arc<Mutex<Vec<(String, String)>>>,
        agent_prompt_supported: bool,
        agent_prompt_not_found: bool,
        compatible: bool,
    }

    impl FakeHerdr {
        fn new(compatible: bool) -> Self {
            Self {
                workspaces: Mutex::new(Vec::new()),
                tabs: Mutex::new(Vec::new()),
                panes: Mutex::new(Vec::new()),
                focused_pane_id: Mutex::new(None),
                requests: std::sync::Arc::new(Mutex::new(Vec::new())),
                close_refusal: std::sync::Arc::new(Mutex::new(None)),
                inputs: std::sync::Arc::new(Mutex::new(Vec::new())),
                agent_prompts: std::sync::Arc::new(Mutex::new(Vec::new())),
                agent_prompt_supported: true,
                agent_prompt_not_found: false,
                compatible,
            }
        }
    }

    fn fake_materialize_layout_node(
        configured: &Value,
        workspace_id: &str,
        tab_id: &str,
        next_pane: &mut usize,
        panes: &mut Vec<Value>,
    ) -> Value {
        if configured.get("type").and_then(Value::as_str) == Some("split") {
            return json!({
                "type": "split",
                "direction": configured["direction"],
                "ratio": configured["ratio"],
                "first": fake_materialize_layout_node(
                    &configured["first"], workspace_id, tab_id, next_pane, panes,
                ),
                "second": fake_materialize_layout_node(
                    &configured["second"], workspace_id, tab_id, next_pane, panes,
                ),
            });
        }

        *next_pane += 1;
        let pane_id = format!("{workspace_id}:materialized-p{next_pane}");
        let pane = json!({
            "type": "pane",
            "pane_id": pane_id,
            "tab_id": tab_id,
            "workspace_id": workspace_id,
            "cwd": configured["cwd"],
            "env": configured["env"],
            "label": configured["label"],
            "command": configured.get("command").cloned().unwrap_or(Value::Null),
            "focused": *next_pane == 1,
            "agent_status": null,
        });
        panes.push(pane.clone());
        pane
    }

    impl HerdrTransport for FakeHerdr {
        fn exchange(&self, request: &HerdrRequest) -> Result<HerdrResponse, HerdrClientError> {
            self.requests.lock().unwrap().push(request.clone());
            let workspace_matches = |workspace: &Value| {
                request.params.get("workspace_id").and_then(Value::as_str)
                    == workspace["workspace_id"].as_str()
            };
            let result = match request.method.as_str() {
                "ping" => json!({
                    "type": "pong",
                    "version": "0.8.0",
                    "protocol": if self.compatible { SUPPORTED_PROTOCOL } else { SUPPORTED_PROTOCOL - 1 },
                }),
                "workspace.create" => {
                    let label = request.params["label"].as_str().unwrap().to_owned();
                    let ws_idx = self.workspaces.lock().unwrap().len() + 1;
                    let ws_id = format!("opaque-w{ws_idx}");
                    let tab_id = format!("{ws_id}:t1");
                    let pane_id = format!("{ws_id}:p1");
                    let cwd = request
                        .params
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("/tmp");

                    let ws_val = json!({
                        "label": label,
                        "workspace_id": ws_id,
                        "number": ws_idx - 1,
                        "focused": false,
                        "pane_count": 1,
                        "tab_count": 1,
                        "active_tab_id": tab_id,
                        "agent_status": "idle"
                    });
                    let tab_val = json!({
                        "tab_id": tab_id,
                        "workspace_id": ws_id,
                        "label": "main",
                        "focused": true,
                        "number": 0
                    });
                    let pane_val = json!({
                        "pane_id": pane_id,
                        "tab_id": tab_id,
                        "workspace_id": ws_id,
                        "cwd": cwd,
                        "focused": true,
                        "agent_status": null
                    });

                    self.workspaces.lock().unwrap().push(ws_val.clone());
                    self.tabs.lock().unwrap().push(tab_val.clone());
                    self.panes.lock().unwrap().push(pane_val.clone());
                    *self.focused_pane_id.lock().unwrap() = Some(pane_id.clone());

                    json!({
                        "type": "workspace_created",
                        "workspace": ws_val,
                        "tab": tab_val,
                        "root_pane": pane_val,
                    })
                }
                "worktree.open" => {
                    let path = request
                        .params
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let pane = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|pane| pane["cwd"].as_str() == Some(path))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let workspace_id = pane
                        .get("workspace_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let workspace = self
                        .workspaces
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|workspace| workspace["workspace_id"].as_str() == Some(workspace_id))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let tab = self
                        .tabs
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|tab| tab["workspace_id"].as_str() == Some(workspace_id))
                        .cloned()
                        .unwrap_or(Value::Null);
                    json!({
                        "type": "worktree_opened",
                        "workspace": workspace,
                        "tab": tab,
                        "root_pane": pane,
                        "already_open": true,
                    })
                }
                "workspace.list" => json!({
                    "workspaces": self.workspaces.lock().unwrap().clone()
                }),
                "session.snapshot" => json!({
                    "snapshot": {
                        "version": "0.9.1",
                        "protocol": SUPPORTED_PROTOCOL,
                        "workspaces": [{
                            "workspace_id": "snapshot-w1",
                            "number": 0,
                            "label": "snapshot",
                            "focused": true,
                            "pane_count": 1,
                            "tab_count": 1,
                            "active_tab_id": "snapshot-w1:t1",
                            "agent_status": "unknown",
                            "worktree": null
                        }],
                        "tabs": [{
                            "tab_id": "snapshot-w1:t1",
                            "workspace_id": "snapshot-w1",
                            "number": 0,
                            "label": "main",
                            "focused": true,
                            "pane_count": 1,
                            "agent_status": "unknown"
                        }],
                        "panes": [{
                            "pane_id": "snapshot-w1:p1",
                            "terminal_id": "terminal-1",
                            "workspace_id": "snapshot-w1",
                            "tab_id": "snapshot-w1:t1",
                            "focused": true,
                            "agent_status": "unknown",
                            "revision": 0,
                            "agent": null,
                            "cwd": null,
                            "label": null,
                            "terminal_title": null,
                            "terminal_title_stripped": null
                        }]
                    }
                }),
                "workspace.focus" => {
                    let mut workspaces = self.workspaces.lock().unwrap();
                    let workspace = workspaces
                        .iter_mut()
                        .find(|workspace| workspace_matches(workspace))
                        .unwrap();
                    workspace["focused"] = json!(true);
                    json!({})
                }
                "workspace.close" => {
                    if request.params["close_group"].as_bool().unwrap_or(false) {
                        self.workspaces.lock().unwrap().clear();
                        self.tabs.lock().unwrap().clear();
                        self.panes.lock().unwrap().clear();
                    } else {
                        let target_id = request
                            .params
                            .get("workspace_id")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if self.close_refusal.lock().unwrap().as_deref() == Some(target_id) {
                            return Ok(HerdrResponse {
                                result: None,
                                error: Some(HerdrResponseError {
                                    code: "workspace_group_close_required".to_owned(),
                                    message: "linked workspaces remain open".to_owned(),
                                }),
                            });
                        }
                        self.workspaces.lock().unwrap().retain(|workspace| {
                            workspace["workspace_id"].as_str() != Some(target_id)
                        });
                        self.tabs
                            .lock()
                            .unwrap()
                            .retain(|tab| tab["workspace_id"].as_str() != Some(target_id));
                        self.panes
                            .lock()
                            .unwrap()
                            .retain(|pane| pane["workspace_id"].as_str() != Some(target_id));
                    }
                    json!({})
                }
                "layout.apply" => {
                    let tab_id = request
                        .params
                        .get("tab_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let workspace_id = self
                        .tabs
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|tab| tab["tab_id"].as_str() == Some(tab_id))
                        .and_then(|tab| tab["workspace_id"].as_str())
                        .unwrap_or("")
                        .to_owned();
                    let mut replacement_panes = Vec::new();
                    let mut next_pane = 0;
                    let root = fake_materialize_layout_node(
                        &request.params["root"],
                        &workspace_id,
                        tab_id,
                        &mut next_pane,
                        &mut replacement_panes,
                    );
                    let pane_count = replacement_panes.len();
                    let focused_pane_id = replacement_panes
                        .iter()
                        .find(|pane| pane["focused"].as_bool() == Some(true))
                        .and_then(|pane| pane["pane_id"].as_str())
                        .map(str::to_owned);
                    self.panes
                        .lock()
                        .unwrap()
                        .retain(|pane| pane["tab_id"].as_str() != Some(tab_id));
                    self.panes.lock().unwrap().extend(replacement_panes);
                    *self.focused_pane_id.lock().unwrap() = focused_pane_id;
                    if let Some(workspace) = self
                        .workspaces
                        .lock()
                        .unwrap()
                        .iter_mut()
                        .find(|workspace| workspace["workspace_id"].as_str() == Some(&workspace_id))
                    {
                        workspace["pane_count"] = json!(pane_count);
                    }
                    json!({
                        "type": "layout_apply",
                        "layout": {
                            "workspace_id": workspace_id,
                            "tab_id": tab_id,
                            "root": root,
                        }
                    })
                }
                "pane.split" => {
                    let cwd = request
                        .params
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("/tmp");
                    let target_id = request.params.get("target_pane_id").and_then(Value::as_str);
                    let target = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|pane| target_id.is_some() && pane["pane_id"].as_str() == target_id)
                        .cloned()
                        .or_else(|| {
                            self.focused_pane_id
                                .lock()
                                .unwrap()
                                .as_deref()
                                .and_then(|focused_id| {
                                    self.panes
                                        .lock()
                                        .unwrap()
                                        .iter()
                                        .find(|pane| pane["pane_id"].as_str() == Some(focused_id))
                                        .cloned()
                                })
                        });
                    let pane_idx = self.panes.lock().unwrap().len() + 1;
                    let pane_id = format!("opaque-p{pane_idx}");
                    let focus = request
                        .params
                        .get("focus")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let pane_val = json!({
                        "pane_id": pane_id,
                        "tab_id": target.as_ref().and_then(|pane| pane.get("tab_id")).cloned().unwrap_or(Value::Null),
                        "workspace_id": target.as_ref().and_then(|pane| pane.get("workspace_id")).cloned().unwrap_or(Value::Null),
                        "cwd": cwd,
                        "focused": focus,
                        "agent_status": null
                    });
                    if focus {
                        for pane in self.panes.lock().unwrap().iter_mut() {
                            pane["focused"] = json!(false);
                        }
                        *self.focused_pane_id.lock().unwrap() = Some(pane_id.clone());
                    }
                    self.panes.lock().unwrap().push(pane_val.clone());
                    if let Some(workspace_id) = pane_val["workspace_id"].as_str() {
                        let pane_count = self
                            .panes
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|pane| pane["workspace_id"].as_str() == Some(workspace_id))
                            .count();
                        if let Some(workspace) =
                            self.workspaces
                                .lock()
                                .unwrap()
                                .iter_mut()
                                .find(|workspace| {
                                    workspace["workspace_id"].as_str() == Some(workspace_id)
                                })
                        {
                            workspace["pane_count"] = json!(pane_count);
                        }
                    }
                    json!({
                        "type": "pane_info",
                        "pane": pane_val
                    })
                }
                "pane.get" => {
                    let pane_id = request
                        .params
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let pane = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|pane| pane["pane_id"].as_str() == Some(pane_id))
                        .cloned()
                        .unwrap_or(Value::Null);
                    json!({ "type": "pane_info", "pane": pane })
                }
                "pane.close" => {
                    let pane_id = request
                        .params
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let removed = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|pane| pane.get("pane_id").and_then(Value::as_str) == Some(pane_id))
                        .cloned();
                    self.panes.lock().unwrap().retain(|pane| {
                        pane.get("pane_id").and_then(Value::as_str) != Some(pane_id)
                    });
                    if let Some(workspace_id) = removed
                        .as_ref()
                        .and_then(|pane| pane.get("workspace_id"))
                        .and_then(Value::as_str)
                    {
                        let pane_count = self
                            .panes
                            .lock()
                            .unwrap()
                            .iter()
                            .filter(|pane| pane["workspace_id"].as_str() == Some(workspace_id))
                            .count();
                        if let Some(workspace) =
                            self.workspaces
                                .lock()
                                .unwrap()
                                .iter_mut()
                                .find(|workspace| {
                                    workspace["workspace_id"].as_str() == Some(workspace_id)
                                })
                        {
                            workspace["pane_count"] = json!(pane_count);
                        }
                    }
                    json!({})
                }
                "pane.list" => {
                    let workspace_id = request.params.get("workspace_id").and_then(Value::as_str);
                    let panes: Vec<Value> = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|pane| {
                            workspace_id.is_none()
                                || pane.get("workspace_id").and_then(Value::as_str) == workspace_id
                        })
                        .cloned()
                        .collect();
                    json!({
                        "type": "pane_list",
                        "panes": panes
                    })
                }
                "pane.current" => {
                    let focused = self.focused_pane_id.lock().unwrap().clone();
                    let panes = self.panes.lock().unwrap();
                    let pane = if let Some(ref fid) = focused {
                        panes
                            .iter()
                            .find(|p| p.get("pane_id").and_then(Value::as_str) == Some(fid))
                    } else {
                        panes.first()
                    };
                    let pane = pane.cloned().unwrap_or_else(|| {
                        json!({
                            "pane_id": "opaque-p1",
                            "cwd": "/tmp",
                            "focused": true,
                            "agent_status": null
                        })
                    });
                    json!({
                        "type": "pane_current",
                        "pane": pane
                    })
                }
                "pane.send_text" => {
                    let pane_id = request
                        .params
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let text = request
                        .params
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    self.inputs.lock().unwrap().push((pane_id, text));
                    json!({ "type": "ok" })
                }
                "pane.send_keys" => {
                    let pane_id = request
                        .params
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned();
                    let keys = request
                        .params
                        .get("keys")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    self.inputs.lock().unwrap().push((pane_id, keys));
                    json!({ "type": "ok" })
                }
                "agent.prompt" if self.agent_prompt_supported => {
                    let target = request.params["target"].as_str().unwrap().to_owned();
                    if self.agent_prompt_not_found {
                        return Ok(HerdrResponse {
                            result: None,
                            error: Some(HerdrResponseError {
                                code: "agent_not_found".to_owned(),
                                message: format!("agent target {target} not found"),
                            }),
                        });
                    }
                    let recognized = self
                        .panes
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|pane| {
                            pane.get("pane_id").and_then(Value::as_str) == Some(target.as_str())
                        })
                        .and_then(|pane| pane.get("agent_status"))
                        .and_then(Value::as_str)
                        .is_some_and(|status| {
                            matches!(status, "idle" | "working" | "blocked" | "done")
                        });
                    if !recognized {
                        return Ok(HerdrResponse {
                            result: None,
                            error: Some(HerdrResponseError {
                                code: "agent_not_found".to_owned(),
                                message: format!("agent target {target} not found"),
                            }),
                        });
                    }
                    let text = request.params["text"].as_str().unwrap().to_owned();
                    self.agent_prompts.lock().unwrap().push((target, text));
                    json!({ "type": "agent_prompted" })
                }
                "agent.prompt" => {
                    return Ok(HerdrResponse {
                        result: None,
                        error: Some(HerdrResponseError {
                            code: "method_not_found".to_owned(),
                            message: "agent.prompt is unavailable".to_owned(),
                        }),
                    });
                }
                "pane.rename" => {
                    let pane_id = request
                        .params
                        .get("pane_id")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let label = request
                        .params
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let mut panes = self.panes.lock().unwrap();
                    if let Some(p) = panes
                        .iter_mut()
                        .find(|p| p.get("pane_id").and_then(Value::as_str) == Some(pane_id))
                    {
                        p["label"] = json!(label);
                    }
                    json!({ "type": "ok" })
                }
                method => return Err(HerdrClientError::InvalidResponse(method.to_owned())),
            };
            Ok(HerdrResponse {
                result: Some(result),
                error: None,
            })
        }
    }

    fn create_request(name: &str) -> CreateWorkspaceRuntimeRequest {
        CreateWorkspaceRuntimeRequest {
            workspace_name: name.into(),
            group: "opaque-group".into(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        }
    }

    #[test]
    fn fake_transport_proves_empty_workspace_lifecycle() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        assert_eq!(binding.backend.as_str(), "herdr");
        assert_eq!(binding.scope_id, "herdr");
        assert_eq!(binding.workspace_id, "opaque-w1");
        assert!(!WorkspaceRuntimePort::has_workspace(&client, "", "w1").unwrap());
        assert!(WorkspaceRuntimePort::has_workspace(&client, "", "opaque-w1").unwrap());
        WorkspaceRuntimePort::attach_workspace_binding(&client, &binding, Default::default())
            .unwrap();
        WorkspaceRuntimePort::detach_workspace_binding(&client, &binding).unwrap();
        WorkspaceRuntimePort::close_workspace_binding(&client, &binding).unwrap();
        WorkspaceRuntimePort::destroy_workspace_binding(&client, &binding).unwrap();
        assert!(!WorkspaceRuntimePort::has_workspace(&client, "", "w1").unwrap());
        assert!(matches!(
            WorkspaceRuntimePort::destroy_group(&client, &binding.scope_id),
            Err(RuntimeError::BackendUnavailable { .. })
        ));

        let requests = requests.lock().unwrap();
        assert!(
            requests
                .iter()
                .any(|request| request.method == "workspace.create")
        );
        assert!(
            requests
                .iter()
                .any(|request| request.method == "workspace.close")
        );
    }

    #[test]
    fn session_snapshot_preserves_native_resource_tree() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        let snapshot = client.session_snapshot().unwrap();
        assert_eq!(snapshot.version, "0.9.1");
        assert_eq!(snapshot.protocol, SUPPORTED_PROTOCOL as u32);
        assert_eq!(snapshot.workspaces[0].workspace_id, "snapshot-w1");
        assert_eq!(snapshot.tabs[0].tab_id, "snapshot-w1:t1");
        assert_eq!(snapshot.panes[0].agent_status.as_deref(), Some("unknown"));
        assert!(
            requests
                .lock()
                .unwrap()
                .iter()
                .any(|request| request.method == "session.snapshot")
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_session_attach_invokes_the_documented_cli() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("herdr");
        let log = temp.path().join("attach.log");
        std::fs::write(
            &script,
            "#!/bin/sh\nprintf '%s' \"$*\" > \"$WORKHIVE_ATTACH_LOG\"\n",
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        crate::test_env::with_exclusive_env(
            &[
                ("WORKHIVE_HERDR_BIN", script.to_str()),
                ("WORKHIVE_ATTACH_LOG", log.to_str()),
            ],
            || attach_session("shared").unwrap(),
        );

        assert_eq!(
            std::fs::read_to_string(log).unwrap(),
            "session attach shared"
        );
    }

    #[test]
    fn workspace_creation_uses_display_label_but_persists_opaque_identity() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        let binding = WorkspaceRuntimePort::create_workspace_binding_with_label(
            &client,
            create_request("wh@instance@project@main"),
            "project",
        )
        .unwrap();

        assert_eq!(binding.workspace_id, "opaque-w1");
        let requests = requests.lock().unwrap();
        let create = requests
            .iter()
            .find(|request| request.method == "workspace.create")
            .unwrap();
        assert_eq!(create.params["label"], "project");
    }

    #[test]
    fn registers_an_existing_worktree_without_transferring_git_ownership() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();
        let mut create = create_request("feature");
        create.working_dir = Utf8PathBuf::from("/repo/.workhive/worktrees/feature");
        let binding = WorkspaceRuntimePort::create_workspace_binding(&client, create).unwrap();

        client
            .register_existing_worktree(
                &binding,
                camino::Utf8Path::new("/repo"),
                camino::Utf8Path::new("/repo/.workhive/worktrees/feature"),
            )
            .unwrap();

        let requests = requests.lock().unwrap();
        let request = requests
            .iter()
            .find(|request| request.method == "worktree.open")
            .expect("existing worktree registration must use worktree.open");
        assert_eq!(request.params["cwd"], "/repo");
        assert_eq!(request.params["path"], "/repo/.workhive/worktrees/feature");
        assert_eq!(request.params["focus"], false);
        assert!(request.params.get("workspace_id").is_none());
    }

    #[test]
    fn rejects_worktree_registration_that_resolves_to_another_workspace() {
        let fake = FakeHerdr::new(true);
        let client = HerdrClient::connect(fake).unwrap();
        let mut first = create_request("first");
        first.working_dir = Utf8PathBuf::from("/repo/first");
        let first_binding = WorkspaceRuntimePort::create_workspace_binding(&client, first).unwrap();
        let mut second = create_request("second");
        second.working_dir = Utf8PathBuf::from("/repo/second");
        WorkspaceRuntimePort::create_workspace_binding(&client, second).unwrap();

        let error = client
            .register_existing_worktree(
                &first_binding,
                camino::Utf8Path::new("/repo"),
                camino::Utf8Path::new("/repo/second"),
            )
            .expect_err("registration must preserve the persisted workspace identity");

        assert!(matches!(error, RuntimeError::OperationFailed { .. }));
        assert!(error.to_string().contains("expected 'opaque-w1'"));
    }

    #[test]
    fn adopts_the_herdr_parent_when_registering_the_project_root() {
        let fake = FakeHerdr::new(true);
        let client = HerdrClient::connect(fake).unwrap();
        let mut staged = create_request("staged-main");
        staged.working_dir = Utf8PathBuf::from("/repo/.staged-main");
        let staged_binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, staged).unwrap();
        let mut parent = create_request("parent");
        parent.working_dir = Utf8PathBuf::from("/repo");
        let parent_binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, parent).unwrap();

        let authoritative = client
            .register_existing_worktree(
                &staged_binding,
                camino::Utf8Path::new("/repo"),
                camino::Utf8Path::new("/repo"),
            )
            .expect("the source checkout must adopt Herdr's existing parent workspace");

        assert_eq!(authoritative, parent_binding);
    }

    #[test]
    fn maps_workspace_api_errors_without_losing_the_bound_identity() {
        let missing = map_workspace_client_error(
            HerdrClientError::Api {
                code: "workspace_not_found".to_owned(),
                message: "workspace not found".to_owned(),
            },
            "w-missing",
        );
        assert_eq!(
            missing,
            RuntimeError::WorkspaceNotFound {
                workspace: "w-missing".to_owned()
            }
        );

        let grouped = map_workspace_client_error(
            HerdrClientError::Api {
                code: "workspace_group_close_required".to_owned(),
                message: "close the workspace group explicitly".to_owned(),
            },
            "w-primary",
        );
        assert_eq!(
            grouped,
            RuntimeError::WorkspaceGroupCloseRequired {
                workspace: "w-primary".to_owned()
            }
        );
    }

    #[test]
    fn current_unit_never_leaks_the_focused_pane_from_another_workspace() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();
        let first =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("first"))
                .unwrap();
        WorkspaceRuntimePort::create_workspace_binding(&client, create_request("second")).unwrap();

        let current = client.current_unit("", &first.workspace_id).unwrap();

        assert_eq!(current.id, UnitId::new("opaque-w1:p1"));
        let requests = requests.lock().unwrap();
        assert!(
            !requests
                .iter()
                .any(|request| request.method == "pane.current")
        );
        assert!(requests.iter().any(|request| {
            request.method == "pane.list"
                && request.params["workspace_id"].as_str() == Some(first.workspace_id.as_str())
        }));
    }

    #[test]
    fn current_unit_is_scoped_to_the_workspace_active_tab() {
        let fake = FakeHerdr::new(true);
        fake.workspaces.lock().unwrap().push(json!({
            "workspace_id": "workspace-1",
            "active_tab_id": "tab-active",
        }));
        fake.panes.lock().unwrap().extend([
            json!({
                "pane_id": "pane-inactive-tab",
                "tab_id": "tab-inactive",
                "workspace_id": "workspace-1",
                "cwd": "/repo",
                "focused": true,
                "agent_status": null,
            }),
            json!({
                "pane_id": "pane-active-tab",
                "tab_id": "tab-active",
                "workspace_id": "workspace-1",
                "cwd": "/repo",
                "focused": false,
                "agent_status": null,
            }),
        ]);
        let client = HerdrClient::connect(fake).unwrap();

        let current = client.current_unit("", "workspace-1").unwrap();

        assert_eq!(current.id, UnitId::new("pane-active-tab"));
    }

    #[cfg(unix)]
    #[test]
    fn herdr_tui_uses_the_bound_socket() {
        use std::ffi::OsStr;

        let command = herdr_tui_command("/tmp/exact-herdr.sock");
        let socket = command
            .get_envs()
            .find(|(key, _)| *key == OsStr::new("HERDR_SOCKET_PATH"))
            .and_then(|(_, value)| value);

        assert_eq!(socket, Some(OsStr::new("/tmp/exact-herdr.sock")));
    }

    #[cfg(unix)]
    #[test]
    fn missing_socket_is_classified_as_runtime_absence() {
        let socket = tempfile::tempdir().unwrap().path().join("missing.sock");

        let error = match HerdrRuntimeAdapter::connect(&socket) {
            Ok(_) => panic!("missing socket unexpectedly connected"),
            Err(error) => error,
        };

        match error {
            RuntimeError::BackendUnavailable { reason, .. } => {
                assert_eq!(
                    reason,
                    workhive_core::ports::BackendUnavailableReason::SocketNotFound {
                        path: socket.to_string_lossy().into_owned(),
                    }
                );
                assert!(reason.is_runtime_absent());
            }
            other => panic!("expected SocketNotFound, got {other:?}"),
        }
    }

    #[test]
    fn refused_socket_is_classified_as_runtime_absence() {
        let path = std::path::PathBuf::from("/tmp/refused-herdr.sock");
        let error = map_client_error(HerdrClientError::Connect {
            path: path.clone(),
            kind: std::io::ErrorKind::ConnectionRefused,
            message: "connection refused".to_owned(),
        });

        match error {
            RuntimeError::BackendUnavailable { reason, .. } => {
                assert_eq!(
                    reason,
                    workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                        server: path.to_string_lossy().into_owned(),
                    }
                );
                assert!(reason.is_runtime_absent());
            }
            other => panic!("expected ServerNotRunning, got {other:?}"),
        }
    }

    #[test]
    fn permission_denied_socket_is_typed() {
        let path = std::path::PathBuf::from("/tmp/denied-herdr.sock");
        let error = map_client_error(HerdrClientError::Connect {
            path,
            kind: std::io::ErrorKind::PermissionDenied,
            message: "permission denied".to_owned(),
        });

        match error {
            RuntimeError::PermissionDenied { detail } => {
                assert!(
                    detail.contains("permission denied"),
                    "permission failures must keep the connect detail: {detail}"
                );
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn missing_herdr_binary_is_classified_as_runtime_absence() {
        let error = map_herdr_launch_error(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "herdr not found",
        ));

        match error {
            RuntimeError::BackendUnavailable { reason, .. } => {
                assert_eq!(
                    reason,
                    workhive_core::ports::BackendUnavailableReason::BackendBinaryNotFound {
                        binary: "herdr".to_owned(),
                    }
                );
                assert!(reason.is_runtime_absent());
            }
            other => panic!("expected BackendBinaryNotFound, got {other:?}"),
        }
    }

    #[test]
    fn validated_client_probes_once_before_operations() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        client.list_workspaces("").unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "ping")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "workspace.list")
                .count(),
            1
        );
    }

    #[test]
    fn bound_lifecycle_requests_use_opaque_runtime_ids() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();

        WorkspaceRuntimePort::attach_workspace_binding(&client, &binding, Default::default())
            .unwrap();
        WorkspaceRuntimePort::detach_workspace_binding(&client, &binding).unwrap();
        WorkspaceRuntimePort::close_workspace_binding(&client, &binding).unwrap();
        WorkspaceRuntimePort::destroy_workspace_binding(&client, &binding).unwrap();

        let requests = requests.lock().unwrap();
        for method in ["workspace.focus", "workspace.close"] {
            let request = requests
                .iter()
                .find(|request| request.method == method)
                .unwrap_or_else(|| panic!("missing {method} request"));
            assert_eq!(request.params["workspace_id"], "opaque-w1");
            assert!(request.params.get("scope_id").is_none());
            assert!(request.params.get("name").is_none());
        }
        let close_requests = requests
            .iter()
            .filter(|request| request.method == "workspace.close")
            .collect::<Vec<_>>();
        assert_eq!(close_requests.len(), 2);
        assert_eq!(close_requests[0].params["close_group"], false);
        assert_eq!(close_requests[1].params["close_group"], false);
    }

    #[test]
    fn target_only_close_refusal_preserves_other_herdr_workspaces() {
        let fake = FakeHerdr::new(true);
        let close_refusal = fake.close_refusal.clone();
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();
        let parent =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("parent"))
                .unwrap();
        let linked_child =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("linked-child"))
                .unwrap();
        let unassociated =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("unassociated"))
                .unwrap();
        *close_refusal.lock().unwrap() = Some(parent.workspace_id.clone());

        let error = WorkspaceRuntimePort::close_workspace_binding(&client, &parent).unwrap_err();

        assert_eq!(
            error,
            RuntimeError::WorkspaceGroupCloseRequired {
                workspace: parent.workspace_id.clone(),
            }
        );
        assert!(WorkspaceRuntimePort::has_workspace_binding(&client, &parent).unwrap());
        assert!(WorkspaceRuntimePort::has_workspace_binding(&client, &linked_child).unwrap());
        assert!(WorkspaceRuntimePort::has_workspace_binding(&client, &unassociated).unwrap());
        let close_requests = requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.method == "workspace.close")
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(close_requests.len(), 1);
        assert_eq!(
            close_requests[0].params["workspace_id"],
            parent.workspace_id
        );
        assert_eq!(close_requests[0].params["close_group"], false);
    }

    #[test]
    fn binding_for_another_backend_is_rejected_before_transport() {
        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        let error = WorkspaceRuntimePort::attach_workspace_binding(
            &client,
            &RuntimeBinding::tmux("server", "workspace"),
            Default::default(),
        )
        .unwrap_err();
        assert!(
            matches!(error, RuntimeError::OperationFailed { message } if message.contains("tmux"))
        );
        assert_eq!(
            requests
                .lock()
                .unwrap()
                .iter()
                .map(|request| request.method.as_str())
                .collect::<Vec<_>>(),
            ["ping"]
        );
    }
    #[test]
    fn incompatible_protocol_is_typed_and_actionable() {
        let fake = FakeHerdr::new(false);
        let error = match HerdrClient::connect(fake) {
            Ok(_) => panic!("incompatible protocol must fail during connect"),
            Err(error) => error,
        };
        let RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing { capability },
            detail,
        } = map_client_error(error)
        else {
            panic!("incompatible protocol must map to a typed backend diagnostic")
        };
        assert!(capability.contains("protocol 21"));
        assert!(capability.contains("supported 22"));
        assert!(detail.contains("protocol 21"));
    }

    #[test]
    fn fake_transport_proves_pane_lifecycle_and_split() {
        let fake = FakeHerdr::new(true);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();

        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        let ws_id = &binding.workspace_id;

        // Initial pane created with workspace
        let units = client.list_units("", ws_id).unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].index, 0);

        // Current unit returns the initial pane
        let current = client.current_unit("", &binding.workspace_id).unwrap();
        assert_eq!(current.id, units[0].id);

        // Split unit (spawn_unit)
        let split_unit = client
            .spawn_unit(SpawnUnitRequest {
                group: "".to_owned(),
                workspace_name: ws_id.clone(),
                direction: workhive_core::domain::PaneDirection::Right,
                command: Some("echo hello".to_owned()),
                target_unit: Some(current.id.clone()),
                size_percent: Some(50),
                env: vec![("AGENT_MODE".to_owned(), "worker".to_owned())],
                cwd: Some(camino::Utf8PathBuf::from("/tmp/split")),
            })
            .unwrap();

        assert_eq!(split_unit.index, 1);
        assert_eq!(split_unit.cwd, camino::Utf8PathBuf::from("/tmp/split"));
        let split_request = requests
            .lock()
            .unwrap()
            .iter()
            .find(|request| request.method == "pane.split")
            .cloned()
            .expect("pane.split request");
        assert_eq!(split_request.params["workspace_id"], ws_id.as_str());
        assert_eq!(split_request.params["target_pane_id"], current.id.as_str());
        assert!(split_request.params.get("pane_id").is_none());
        assert_eq!(split_request.params["env"]["AGENT_MODE"], "worker");

        // Verify input was sent to the newly split unit
        let recorded_inputs = inputs.lock().unwrap();
        assert!(recorded_inputs.iter().any(
            |(pane_id, keys)| pane_id == split_unit.id.as_str() && keys.contains("echo hello")
        ));
        drop(recorded_inputs);

        // List units now returns 2
        let units_after_split = client.list_units("", ws_id).unwrap();
        assert_eq!(units_after_split.len(), 2);

        // Rename unit
        client
            .set_unit_title(SetUnitTitleRequest {
                group: "".to_owned(),
                target_unit: split_unit.id.clone(),
                title: "agent-worker".to_owned(),
            })
            .unwrap();

        // Herdr has no native same-pane respawn operation. Sending `cd` would
        // violate the port contract because it is visible input.
        assert!(matches!(
            client.respawn_unit_with_cwd(
                "",
                ws_id,
                &split_unit.id,
                &camino::Utf8PathBuf::from("/tmp/respawned"),
            ),
            Err(RuntimeError::BackendUnavailable { .. })
        ));

        // Close unit
        client.close_unit("", &split_unit.id).unwrap();
        let units_after_close = client.list_units("", ws_id).unwrap();
        assert_eq!(units_after_close.len(), 1);
        assert_eq!(units_after_close[0].id, units[0].id);
        assert!(matches!(
            client.close_unit("", &units[0].id),
            Err(RuntimeError::LastUnitCloseRejected)
        ));
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "real-herdr: requires the running Workhive-managed Herdr session"]
    fn real_herdr_protocol_and_workspace_list_smoke() {
        // Socket discovery reads process-global HOME/XDG variables. Serialize
        // this read with isolated Herdr tests that temporarily replace them.
        crate::test_env::with_exclusive_env(&[], || {
            let socket = crate::dependency::discover_workhive_herdr_socket_path()
                .expect("discover the default Workhive Herdr socket");
            let adapter = HerdrRuntimeAdapter::connect(socket).expect("connect to real Herdr");

            adapter
                .list_workspaces("")
                .expect("list real Herdr workspaces");
        });
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "real-herdr: creates a temporary Git worktree and Herdr workspace group"]
    fn real_herdr_registers_an_existing_git_worktree() {
        use std::process::Command;

        fn git(args: &[&str]) {
            let output = Command::new("git").args(args).output().expect("run git");
            assert!(
                output.status.success(),
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let temp = tempfile::tempdir().expect("create temporary E2E directory");
        let home = temp.path().join("home");
        let xdg = home.join(".config");
        std::fs::create_dir_all(&xdg).expect("create isolated Herdr HOME");
        let socket = temp.path().join("herdr.sock");
        let child = Command::new("herdr")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("HERDR_SOCKET_PATH", &socket)
            .arg("server")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("start isolated Herdr server");

        struct IsolatedHerdr {
            socket: std::path::PathBuf,
            home: std::path::PathBuf,
            child: Option<std::process::Child>,
        }

        impl Drop for IsolatedHerdr {
            fn drop(&mut self) {
                if let Ok(adapter) = HerdrRuntimeAdapter::connect(self.socket.clone())
                    && let Ok(listed) = adapter.client.call("workspace.list", json!({}))
                    && let Some(workspaces) = listed["workspaces"].as_array()
                {
                    let mut owned = workspaces
                        .iter()
                        .filter_map(|workspace| {
                            Some((
                                workspace["worktree"]["is_linked_worktree"]
                                    .as_bool()
                                    .unwrap_or(false),
                                workspace["workspace_id"].as_str()?.to_owned(),
                            ))
                        })
                        .collect::<Vec<_>>();
                    owned.sort_by_key(|(linked, _)| !linked);
                    for (_, workspace_id) in owned {
                        let _ = adapter.client.call_for_workspace(
                            "workspace.close",
                            json!({ "workspace_id": workspace_id.clone(), "close_group": false }),
                            &workspace_id,
                        );
                    }
                }

                let stopped = Command::new("herdr")
                    .env("HOME", &self.home)
                    .env("HERDR_SOCKET_PATH", &self.socket)
                    .args(["server", "stop"])
                    .status()
                    .is_ok_and(|status| status.success());
                if let Some(mut child) = self.child.take() {
                    if !stopped {
                        let _ = child.kill();
                    }
                    let _ = child.wait();
                }
            }
        }

        let _cleanup = IsolatedHerdr {
            socket: socket.clone(),
            home,
            child: Some(child),
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let adapter = loop {
            match HerdrRuntimeAdapter::connect(socket.clone()) {
                Ok(adapter) => break adapter,
                Err(error) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let _ = &error;
                }
                Err(error) => panic!("isolated Herdr server did not become ready: {error}"),
            }
        };
        let checkout = temp.path().join("feature");
        let source = temp.path().join("source");
        let source_text = source.to_str().expect("UTF-8 source path");
        let checkout_text = checkout.to_str().expect("UTF-8 checkout path");
        git(&["init", source_text]);
        git(&[
            "-C",
            source_text,
            "config",
            "user.email",
            "e2e@workhive.test",
        ]);
        git(&["-C", source_text, "config", "user.name", "Workhive E2E"]);
        std::fs::write(source.join("README.md"), "workhive e2e\n").expect("write fixture");
        git(&["-C", source_text, "add", "README.md"]);
        git(&["-C", source_text, "commit", "-m", "initial"]);
        git(&[
            "-C",
            source_text,
            "worktree",
            "add",
            "-b",
            "workhive-e2e-feature",
            checkout_text,
        ]);

        let binding = WorkspaceRuntimePort::create_workspace_binding_with_label(
            &adapter,
            CreateWorkspaceRuntimeRequest {
                workspace_name: "workhive-e2e-worktree".to_owned(),
                group: adapter.client.scope_id.clone(),
                working_dir: camino::Utf8PathBuf::from(checkout_text),
                env: vec![("WORKHIVE_E2E".to_owned(), "1".to_owned())],
            },
            "workhive-e2e",
        )
        .expect("create real Herdr workspace");
        WorkspaceRuntimePort::register_workspace_checkout(
            &adapter,
            RegisterWorkspaceCheckoutRequest {
                binding: binding.clone(),
                project_root: camino::Utf8PathBuf::from(source_text),
                workspace_root: camino::Utf8PathBuf::from(checkout_text),
            },
        )
        .expect("register existing worktree with Herdr");

        let listed = adapter
            .client
            .call("workspace.list", json!({}))
            .expect("list real Herdr workspaces");
        let workspaces = listed["workspaces"]
            .as_array()
            .expect("workspace.list.workspaces");
        let registered = workspaces
            .iter()
            .find(|workspace| {
                workspace["workspace_id"].as_str() == Some(binding.workspace_id.as_str())
            })
            .expect("registered workspace remains bound to the created opaque ID");
        assert_eq!(registered["label"].as_str(), Some("workhive-e2e"));
        assert_eq!(
            registered["worktree"]["checkout_path"].as_str(),
            Some(checkout_text)
        );
        assert_eq!(
            registered["worktree"]["is_linked_worktree"].as_bool(),
            Some(true)
        );

        let staged_main = WorkspaceRuntimePort::create_workspace_binding(
            &adapter,
            CreateWorkspaceRuntimeRequest {
                workspace_name: "workhive-e2e-main-staged".to_owned(),
                group: adapter.client.scope_id.clone(),
                working_dir: camino::Utf8PathBuf::from(source_text),
                env: Vec::new(),
            },
        )
        .expect("create staged main workspace");
        let adopted_parent = WorkspaceRuntimePort::register_workspace_checkout(
            &adapter,
            RegisterWorkspaceCheckoutRequest {
                binding: staged_main.clone(),
                project_root: camino::Utf8PathBuf::from(source_text),
                workspace_root: camino::Utf8PathBuf::from(source_text),
            },
        )
        .expect("adopt Herdr parent when main is opened after a feature");
        assert_ne!(adopted_parent, staged_main);
        WorkspaceRuntimePort::destroy_workspace_binding(&adapter, &staged_main)
            .expect("remove superseded staged main workspace");

        let replacement = WorkspaceRuntimePort::create_workspace_binding(
            &adapter,
            CreateWorkspaceRuntimeRequest {
                workspace_name: "workhive-e2e-replacement".to_owned(),
                group: adapter.client.scope_id.clone(),
                working_dir: camino::Utf8PathBuf::from(checkout_text),
                env: Vec::new(),
            },
        )
        .expect("create staged replacement workspace");
        let collision = WorkspaceRuntimePort::register_workspace_checkout(
            &adapter,
            RegisterWorkspaceCheckoutRequest {
                binding: replacement.clone(),
                project_root: camino::Utf8PathBuf::from(source_text),
                workspace_root: camino::Utf8PathBuf::from(checkout_text),
            },
        )
        .expect_err("old workspace registration must win until it is retired");
        assert!(matches!(collision, RuntimeError::OperationFailed { .. }));

        WorkspaceRuntimePort::close_workspace_binding(&adapter, &binding)
            .expect("retire old registered workspace");
        WorkspaceRuntimePort::register_workspace_checkout(
            &adapter,
            RegisterWorkspaceCheckoutRequest {
                binding: replacement.clone(),
                project_root: camino::Utf8PathBuf::from(source_text),
                workspace_root: camino::Utf8PathBuf::from(checkout_text),
            },
        )
        .expect("register staged replacement after retiring old workspace");

        let cleanup_list = adapter
            .client
            .call("workspace.list", json!({}))
            .expect("list E2E workspaces for cleanup");
        let cleanup_workspaces = cleanup_list["workspaces"]
            .as_array()
            .expect("workspace.list.workspaces");
        let replacement_workspace = cleanup_workspaces
            .iter()
            .find(|workspace| {
                workspace["workspace_id"].as_str() == Some(replacement.workspace_id.as_str())
            })
            .expect("replacement workspace remains bound after registration");
        assert_eq!(
            replacement_workspace["worktree"]["checkout_path"].as_str(),
            Some(checkout_text)
        );

        let mut owned = cleanup_workspaces
            .iter()
            .filter_map(|workspace| {
                let worktree = workspace.get("worktree")?;
                let path = worktree.get("checkout_path")?.as_str()?;
                path.starts_with(temp.path().to_str()?).then(|| {
                    (
                        worktree["is_linked_worktree"].as_bool().unwrap_or(false),
                        workspace["workspace_id"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned(),
                    )
                })
            })
            .collect::<Vec<_>>();
        owned.sort_by_key(|(linked, _)| !linked);
        for (_, workspace_id) in owned {
            adapter
                .client
                .call_for_workspace(
                    "workspace.close",
                    json!({ "workspace_id": workspace_id.clone(), "close_group": false }),
                    &workspace_id,
                )
                .expect("close E2E-owned Herdr workspace");
        }
    }

    #[test]
    fn recognized_agent_uses_structured_prompt_delivery() {
        let fake = FakeHerdr::new(true);
        let prompts = std::sync::Arc::clone(&fake.agent_prompts);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        let unit = client
            .list_units("", &binding.workspace_id)
            .unwrap()
            .remove(0);
        client.transport.panes.lock().unwrap()[0]["agent_status"] = json!("idle");

        client
            .send_input(SendInputRequest {
                group: String::new(),
                target_unit: unit.id.clone(),
                text: "review this".to_owned(),
                enter: true,
            })
            .unwrap();

        assert_eq!(
            prompts.lock().unwrap().as_slice(),
            &[(unit.id.as_str().to_owned(), "review this".to_owned())]
        );
        assert!(inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn missing_agent_prompt_capability_falls_back_to_pane_input() {
        let mut fake = FakeHerdr::new(true);
        fake.agent_prompt_supported = false;
        let prompts = std::sync::Arc::clone(&fake.agent_prompts);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        let unit = client
            .list_units("", &binding.workspace_id)
            .unwrap()
            .remove(0);
        client.transport.panes.lock().unwrap()[0]["agent_status"] = json!("idle");

        client
            .send_input(SendInputRequest {
                group: String::new(),
                target_unit: unit.id.clone(),
                text: "fallback".to_owned(),
                enter: true,
            })
            .unwrap();

        assert!(prompts.lock().unwrap().is_empty());
        assert_eq!(
            inputs.lock().unwrap().as_slice(),
            &[
                (unit.id.as_str().to_owned(), "fallback".to_owned()),
                (unit.id.as_str().to_owned(), "enter".to_owned()),
            ]
        );
    }

    #[test]
    fn unknown_agent_status_falls_back_to_pane_input() {
        let fake = FakeHerdr::new(true);
        let prompts = std::sync::Arc::clone(&fake.agent_prompts);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        client.transport.panes.lock().unwrap()[0]["agent_status"] = json!("unknown");
        let unit = client
            .list_units("", &binding.workspace_id)
            .unwrap()
            .remove(0);
        assert_eq!(unit.kind, workhive_core::domain::PaneKind::Terminal);

        client
            .send_input(SendInputRequest {
                group: String::new(),
                target_unit: unit.id.clone(),
                text: "ordinary shell input".to_owned(),
                enter: true,
            })
            .unwrap();

        assert!(prompts.lock().unwrap().is_empty());
        assert_eq!(
            inputs.lock().unwrap().as_slice(),
            &[
                (
                    unit.id.as_str().to_owned(),
                    "ordinary shell input".to_owned()
                ),
                (unit.id.as_str().to_owned(), "enter".to_owned()),
            ]
        );
    }

    #[test]
    fn agent_not_found_falls_back_to_pane_input() {
        let mut fake = FakeHerdr::new(true);
        fake.agent_prompt_not_found = true;
        let prompts = std::sync::Arc::clone(&fake.agent_prompts);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        let unit = client
            .list_units("", &binding.workspace_id)
            .unwrap()
            .remove(0);
        client.transport.panes.lock().unwrap()[0]["agent_status"] = json!("idle");

        client
            .send_input(SendInputRequest {
                group: String::new(),
                target_unit: unit.id.clone(),
                text: "retry as pane input".to_owned(),
                enter: true,
            })
            .unwrap();

        assert!(prompts.lock().unwrap().is_empty());
        assert_eq!(
            inputs.lock().unwrap().as_slice(),
            &[
                (
                    unit.id.as_str().to_owned(),
                    "retry as pane input".to_owned()
                ),
                (unit.id.as_str().to_owned(), "enter".to_owned()),
            ]
        );
    }

    #[test]
    fn text_only_agent_input_does_not_submit_a_structured_prompt() {
        let fake = FakeHerdr::new(true);
        let prompts = std::sync::Arc::clone(&fake.agent_prompts);
        let inputs = std::sync::Arc::clone(&fake.inputs);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("w1")).unwrap();
        let unit = client
            .list_units("", &binding.workspace_id)
            .unwrap()
            .remove(0);
        client.transport.panes.lock().unwrap()[0]["agent_status"] = json!("working");

        client
            .send_input(SendInputRequest {
                group: String::new(),
                target_unit: unit.id.clone(),
                text: "draft".to_owned(),
                enter: false,
            })
            .unwrap();

        assert!(prompts.lock().unwrap().is_empty());
        assert_eq!(
            inputs.lock().unwrap().as_slice(),
            &[(unit.id.as_str().to_owned(), "draft".to_owned())]
        );
    }

    #[test]
    fn fake_transport_materializes_configured_layout_with_native_launches() {
        use workhive_core::ports::{
            ConfiguredLayoutNode, ConfiguredLayoutPane, LayoutPanePosition,
            MaterializeConfiguredLayoutRequest,
        };

        let fake = FakeHerdr::new(true);
        let requests = std::sync::Arc::clone(&fake.requests);
        let client = HerdrClient::connect(fake).unwrap();
        let binding =
            WorkspaceRuntimePort::create_workspace_binding(&client, create_request("app-ws"))
                .unwrap();
        client.transport.tabs.lock().unwrap().insert(
            0,
            json!({
                "tab_id": "opaque-w1:decoy",
                "workspace_id": binding.workspace_id.clone(),
                "label": "decoy",
                "focused": false,
                "number": 1
            }),
        );
        client.transport.panes.lock().unwrap().insert(
            0,
            json!({
                "pane_id": "opaque-w1:decoy-p1",
                "tab_id": "opaque-w1:decoy",
                "workspace_id": binding.workspace_id.clone(),
                "cwd": "/tmp/decoy",
                "focused": false,
                "agent_status": null
            }),
        );

        let result = client
            .materialize_configured_layout(MaterializeConfiguredLayoutRequest {
                workspace_id: binding.workspace_id.clone(),
                root: ConfiguredLayoutNode::Split {
                    direction: workhive_core::domain::PaneDirection::Right,
                    ratio: 0.4,
                    first: Box::new(ConfiguredLayoutNode::Pane(ConfiguredLayoutPane {
                        position: LayoutPanePosition { column: 0, row: 0 },
                        cwd: camino::Utf8PathBuf::from("/tmp/app"),
                        env: vec![("ROLE".to_owned(), "editor".to_owned())],
                        title: "editor".to_owned(),
                        command: None,
                    })),
                    second: Box::new(ConfiguredLayoutNode::Pane(ConfiguredLayoutPane {
                        position: LayoutPanePosition { column: 1, row: 0 },
                        cwd: camino::Utf8PathBuf::from("/tmp/app/tests"),
                        env: vec![("ROLE".to_owned(), "tests".to_owned())],
                        title: "tests".to_owned(),
                        command: Some("cargo test".to_owned()),
                    })),
                },
            })
            .unwrap();

        assert_eq!(result.panes.len(), 2);
        assert_eq!(
            result.panes[0].position,
            LayoutPanePosition { column: 0, row: 0 }
        );
        assert_eq!(
            result.panes[1].position,
            LayoutPanePosition { column: 1, row: 0 }
        );
        assert_ne!(result.panes[0].unit.id, result.panes[1].unit.id);
        assert_eq!(
            client.current_unit("", &binding.workspace_id).unwrap().id,
            result.panes[0].unit.id
        );
        assert_eq!(
            client.list_units("", &binding.workspace_id).unwrap().len(),
            3
        );

        let requests = requests.lock().unwrap();
        let apply = requests
            .iter()
            .find(|request| request.method == "layout.apply")
            .expect("layout.apply request");
        assert_eq!(apply.params["tab_id"], "opaque-w1:t1");
        assert!(apply.params.get("workspace_id").is_none());
        let second = &apply.params["root"]["second"];
        assert_eq!(second["cwd"], "/tmp/app/tests");
        assert_eq!(second["env"]["ROLE"], "tests");
        assert_eq!(second["label"], "tests");
        assert_eq!(second["command"], json!(["sh", "-c", "cargo test"]));
    }
}

//! Runtime port — abstracts workspace and pane runtime operations.
//!
//! These traits express business-level capabilities without exposing
//! the semantics of any particular runtime backend (tmux, native PTY,
//! wezterm, remote agent, etc.).
//!
//! `TmuxPort` is preserved as an infra-level adapter port. Business
//! use cases depend on `WorkspaceRuntimePort` and `PaneRuntimePort`
//! instead.

use camino::Utf8PathBuf;

use crate::domain::{
    PaneDirection, PaneKind, PaneState, RuntimeBinding, SessionNamingPolicy, UnitId,
};

/// Default named Herdr session used by legacy compatibility helpers.
pub const WORKHIVE_HERDR_SESSION: &str = super::DEFAULT_HERDR_SESSION;

/// Typed reason for a [`RuntimeError::BackendUnavailable`] result.
///
/// The CLI boundary maps each variant to a localized i18n key so
/// `workhive workspace attach` (and any other command that surfaces a
/// runtime error) renders user-facing text in the user's preferred
/// language. Adapters MUST set this field so the `Display` text
/// (still preserved for logs) does not become the user-facing
/// message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendUnavailableReason {
    /// Backend executable not found in `PATH` (e.g. `tmux` is not
    /// installed). The `path` field carries the binary name the
    /// adapter looked up.
    BackendBinaryNotFound { binary: String },
    /// Backend daemon is not running for the given server / socket
    /// scope. Carries the server name so the localized renderer can
    /// surface "server `wh-srv` 未运行".
    ServerNotRunning { server: String },
    /// Backend socket path does not exist on disk. Carries the socket
    /// path so users can find it (it is part of `WORKHIVE_RUNTIME_DIR`
    /// or the system tmux directory).
    SocketNotFound { path: String },
    /// A required backend capability is not available.
    CapabilityMissing { capability: String },
    /// Other / generic backend-not-available case — the adapter could
    /// not classify the reason. The CLI renderer emits a localized generic
    /// message and logs the detail for operators; it does not expose raw
    /// diagnostic text as user-facing copy.
    Other,
}

impl BackendUnavailableReason {
    /// Whether this reason means the runtime is absent and may be treated as
    /// an empty scope by read/idempotent cleanup workflows.
    pub fn is_runtime_absent(&self) -> bool {
        matches!(
            self,
            Self::BackendBinaryNotFound { .. }
                | Self::ServerNotRunning { .. }
                | Self::SocketNotFound { .. }
        )
    }

    /// i18n key for the reason. The CLI boundary uses this with the
    /// runtime locale to render a localized error message.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::BackendBinaryNotFound { .. } => {
                crate::i18n_keys::CLI_RUNTIME_BACKEND_BINARY_NOT_FOUND
            }
            Self::ServerNotRunning { .. } => crate::i18n_keys::CLI_RUNTIME_SERVER_NOT_RUNNING,
            Self::SocketNotFound { .. } => crate::i18n_keys::CLI_RUNTIME_SOCKET_NOT_FOUND,
            Self::CapabilityMissing { .. } => crate::i18n_keys::CLI_RUNTIME_CAPABILITY_MISSING,
            Self::Other => crate::i18n_keys::CLI_RUNTIME_BACKEND_UNAVAILABLE_GENERIC,
        }
    }

    /// Positional i18n arguments that match the catalog template for
    /// [`Self::i18n_key`]. The CLI renderer feeds these into
    /// `I18nPort::resolve_with_args`.
    pub fn i18n_args(&self) -> Vec<String> {
        match self {
            Self::BackendBinaryNotFound { binary } => vec![binary.clone()],
            Self::ServerNotRunning { server } => vec![server.clone()],
            Self::SocketNotFound { path } => vec![path.clone()],
            Self::CapabilityMissing { capability } => vec![capability.clone()],
            Self::Other => Vec::new(),
        }
    }
}

/// Errors from workspace or pane runtime operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    /// The runtime backend (e.g. tmux) is not available. The
    /// `reason` field is typed so the CLI boundary can render
    /// localized text; `detail` is kept for diagnostics / logs and
    /// for backward compatibility with code that pattern-matches on
    /// the old struct shape.
    #[error("runtime backend not available: {detail}")]
    BackendUnavailable {
        reason: BackendUnavailableReason,
        detail: String,
    },

    #[error("session not found: {workspace}")]
    WorkspaceNotFound { workspace: String },

    #[error("workspace group must be closed explicitly: {workspace}")]
    WorkspaceGroupCloseRequired { workspace: String },

    #[error("workspace runtime already exists: {workspace}")]
    WorkspaceAlreadyExists { workspace: String },

    #[error("unit not found: {id}")]
    UnitNotFound { id: String },

    #[error("cannot close last unit in workspace")]
    LastUnitCloseRejected,

    #[error("runtime operation failed: {message}")]
    OperationFailed { message: String },

    #[error("runtime permission denied: {detail}")]
    PermissionDenied { detail: String },
}

/// Request to create a new workspace runtime environment.
#[derive(Debug, Clone)]
pub struct CreateWorkspaceRuntimeRequest {
    /// Stable operational name used by backends that address workspaces by
    /// caller-chosen identifiers (for example, a tmux session name).
    ///
    /// Backends with server-assigned identities must return those identities
    /// through [`RuntimeBinding`], rather than deriving them from this name.
    pub workspace_name: String,
    /// Isolation group for process separation.
    pub group: String,
    /// Working directory for the initial unit.
    pub working_dir: Utf8PathBuf,
    /// Effective environment to install for newly spawned shells.
    ///
    /// When non-empty the runtime adapter MUST inject these key/value pairs
    /// into both the workspace environment (so subsequent split panes inherit
    /// them) and the initial unit. Order is preserved by the caller; the
    /// adapter is free to reuse a hash map internally.
    pub env: Vec<(String, String)>,
}

/// Associate a runtime workspace with an existing Git checkout.
///
/// The checkout has already been created or adopted by Workhive. Runtime
/// backends may use this metadata for native grouping, but must not create,
/// remove, or otherwise take ownership of the checkout.
#[derive(Debug, Clone)]
pub struct RegisterWorkspaceCheckoutRequest {
    pub binding: RuntimeBinding,
    pub project_root: Utf8PathBuf,
    pub workspace_root: Utf8PathBuf,
}

/// Resolved, non-optional interaction policy for tmux sessions.
///
/// Produced at the CLI/config boundary by resolving the layered
/// `TmuxConfig` chain. Runtime adapters consume this without reading
/// config files themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeInteractionPolicy {
    /// Whether tmux mouse support is enabled.
    pub mouse_enabled: bool,
    /// Whether clipboard integration (OSC 52 + copy-mode bindings) is
    /// enabled.
    pub clipboard_enabled: bool,
}

impl Default for RuntimeInteractionPolicy {
    fn default() -> Self {
        Self {
            mouse_enabled: true,
            clipboard_enabled: true,
        }
    }
}

/// Request to attach to an existing workspace runtime.
#[derive(Debug, Clone)]
pub struct AttachWorkspaceRequest {
    /// Isolation group.
    pub group: String,
    /// Workspace environment name.
    pub workspace_name: String,
    /// Interaction policy to (re-)apply before attaching.
    pub interaction_policy: RuntimeInteractionPolicy,
}

/// Summary of a workspace runtime instance.
#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeInfo {
    /// Backend-owned opaque identity used for runtime operations.
    pub runtime_id: String,
    /// Optional human-readable runtime label. Labels are not identities.
    pub label: Option<String>,
    /// Number of active units (panes/shells) in this workspace.
    pub unit_count: u32,
    /// Whether a user terminal is attached.
    pub attached: bool,
}

/// Snapshot of a single runtime unit — read on demand.
///
/// `tty` and `process_pid` are present so adapters can hand a typed
/// process lookup to [`crate::ports::ProcessObserverPort`] without
/// reaching back into tmux from the orchestration layer. Both fields
/// are `Option` because `PaneRuntimePort` is runtime-agnostic; backends
/// that cannot supply them (e.g. fakes used in unit tests, or future
/// non-tmux adapters) leave them `None`. The layered pane runtime
/// observer treats their absence as "this layer cannot help, fall
/// through to the next observation source".
#[derive(Debug, Clone)]
pub struct RuntimeUnitSnapshot {
    /// Runtime-neutral identifier for this unit.
    pub id: UnitId,
    /// Zero-based index.
    pub index: u32,
    /// Current working directory.
    pub cwd: Utf8PathBuf,
    /// Current foreground command name.
    pub command: String,
    /// Whether this unit is focused.
    pub active: bool,
    /// Best-effort classification.
    pub kind: PaneKind,
    /// Best-effort state.
    pub state: PaneState,
    /// Terminal tty path for this unit if the runtime can observe one
    /// (e.g. tmux exposes `#{pane_tty}` like `/dev/ttys003`). Empty
    /// strings from the backend are normalized to `None`.
    pub tty: Option<String>,
    /// Process id of the unit's primary process (shell pid for tmux
    /// panes). The OS process observer uses this purely as a sanity
    /// hint when no foreground tpgid can be determined; it is *not*
    /// trusted as the foreground command on its own.
    pub process_pid: Option<u32>,
}

/// Request to split (spawn) a new unit within a workspace runtime.
#[derive(Debug, Clone)]
pub struct SpawnUnitRequest {
    /// Isolation group.
    pub group: String,
    /// Workspace environment name.
    pub workspace_name: String,
    /// Split direction.
    pub direction: PaneDirection,
    /// Optional command to run in the new unit.
    pub command: Option<String>,
    /// Target unit to split from. When `None`, the active unit is split.
    pub target_unit: Option<UnitId>,
    /// Size as a percentage. When `None`, the backend default is used.
    pub size_percent: Option<u8>,
    /// Per-spawn environment overrides. Empty means "inherit the workspace
    /// environment as-is". When non-empty these key/value pairs are passed
    /// to the underlying backend for that single spawn.
    pub env: Vec<(String, String)>,
    /// Optional initial working directory for the new unit. When `Some`, the
    /// adapter MUST set it through a native backend capability (e.g. tmux
    /// `split-window -c`) and MUST NOT fabricate a visible `cd -- ...`
    /// keystroke.
    pub cwd: Option<Utf8PathBuf>,
}

/// Logical pane position in Workhive's configured column layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutPanePosition {
    pub column: usize,
    pub row: usize,
}

/// One configured pane passed to a runtime's native layout materializer.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredLayoutPane {
    pub position: LayoutPanePosition,
    pub cwd: Utf8PathBuf,
    pub env: Vec<(String, String)>,
    pub title: String,
    /// Shell command to start as the pane's primary process.
    pub command: Option<String>,
}

/// Runtime-neutral binary split tree for one configured window or tab.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfiguredLayoutNode {
    Pane(ConfiguredLayoutPane),
    Split {
        direction: PaneDirection,
        /// Fraction allocated to the second child.
        ratio: f64,
        first: Box<ConfiguredLayoutNode>,
        second: Box<ConfiguredLayoutNode>,
    },
}

/// Request to atomically materialize one configured window or tab.
#[derive(Debug, Clone, PartialEq)]
pub struct MaterializeConfiguredLayoutRequest {
    pub workspace_id: String,
    pub root: ConfiguredLayoutNode,
}

/// One materialized pane and its logical configured position.
#[derive(Debug, Clone)]
pub struct MaterializedLayoutPane {
    pub position: LayoutPanePosition,
    pub unit: RuntimeUnitSnapshot,
}

/// Result of native configured-layout materialization.
#[derive(Debug, Clone)]
pub struct MaterializedConfiguredLayout {
    pub panes: Vec<MaterializedLayoutPane>,
}

/// Request to send input to a runtime unit.
#[derive(Debug, Clone)]
pub struct SendInputRequest {
    /// Isolation group.
    pub group: String,
    /// Target unit.
    pub target_unit: UnitId,
    /// Text to send.
    pub text: String,
    /// Whether to append a newline/enter after the text.
    pub enter: bool,
}

/// Request to set the display title of a runtime unit.
#[derive(Debug, Clone)]
pub struct SetUnitTitleRequest {
    /// Isolation group.
    pub group: String,
    /// Target unit.
    pub target_unit: UnitId,
    /// Title to display (e.g. tool name like "claude", "codex").
    pub title: String,
}

/// Workspace-level runtime operations.
///
/// Abstracts the lifecycle of a workspace runtime environment.
/// Business use cases depend on this trait, not on `TmuxPort`.
pub trait WorkspaceRuntimePort {
    /// Check if a workspace runtime exists within a group.
    fn has_workspace(&self, group: &str, workspace_name: &str) -> Result<bool, RuntimeError>;

    /// Check whether the runtime identified by a persisted binding exists.
    fn has_workspace_binding(&self, binding: &RuntimeBinding) -> Result<bool, RuntimeError> {
        self.has_workspace(&binding.scope_id, &binding.workspace_id)
    }

    /// Create a new workspace runtime environment.
    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError>;

    /// Return a concrete binding after creating a runtime workspace.
    ///
    /// The default implementation is deliberately unsupported: adapters must
    /// opt in when the backend returns an authoritative runtime identity.
    fn create_workspace_binding(
        &self,
        _request: CreateWorkspaceRuntimeRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not return persisted workspace bindings".to_owned(),
        })
    }

    /// Create a workspace and assign a human-readable display label.
    ///
    /// `request.workspace_name` remains the backend's stable operational
    /// identifier. Adapters that do not distinguish labels from identities
    /// retain their existing behavior through this default implementation.
    fn create_workspace_binding_with_label(
        &self,
        request: CreateWorkspaceRuntimeRequest,
        _display_label: &str,
    ) -> Result<RuntimeBinding, RuntimeError> {
        self.create_workspace_binding(request)
    }

    /// Register an existing checkout with backend-native workspace grouping.
    ///
    /// Backends without checkout grouping intentionally treat this as a no-op.
    fn register_workspace_checkout(
        &self,
        request: RegisterWorkspaceCheckoutRequest,
    ) -> Result<RuntimeBinding, RuntimeError> {
        Ok(request.binding)
    }

    /// Rename a runtime binding within a backend when the backend supports it.
    ///
    /// This is intentionally optional: tmux can promote a staged session with
    /// `rename-session`, while Herdr identities are server-assigned and must
    /// never be renamed by Workhive.
    fn rename_workspace_binding(
        &self,
        _from: &RuntimeBinding,
        _to: &RuntimeBinding,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not support renaming workspace bindings".to_owned(),
        })
    }

    /// Attach using a persisted binding.
    fn attach_workspace_binding(
        &self,
        _binding: &RuntimeBinding,
        _policy: RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not support persisted workspace bindings".to_owned(),
        })
    }

    /// Detach using a persisted binding.
    fn detach_workspace_binding(&self, _binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not support persisted workspace bindings".to_owned(),
        })
    }

    /// Close using a persisted binding.
    fn close_workspace_binding(&self, _binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not support persisted workspace bindings".to_owned(),
        })
    }

    /// Destroy using a persisted binding.
    fn destroy_workspace_binding(&self, _binding: &RuntimeBinding) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime adapter does not support persisted workspace bindings".to_owned(),
        })
    }

    /// Apply interaction configuration using a persisted binding.
    fn configure_workspace_binding(
        &self,
        binding: &RuntimeBinding,
        policy: &RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.configure_workspace(&binding.scope_id, &binding.workspace_id, policy)
    }

    /// Apply interaction configuration to a workspace runtime.
    fn configure_workspace(
        &self,
        group: &str,
        workspace_name: &str,
        policy: &RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError>;

    /// Attach the current terminal to a workspace runtime.
    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError>;

    /// Switch the current client to another session on the same server.
    ///
    /// Configures the target session with the given interaction policy
    /// before switching. Configuration failure prevents the switch.
    fn switch_workspace(
        &self,
        group: &str,
        target_workspace: &str,
        policy: &RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError>;

    /// Detach the current terminal from the specified workspace runtime.
    ///
    /// Defaulted so adapters and test doubles that never detach do not have to
    /// restate the signature.
    fn detach_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
        let _ = group;
        Err(RuntimeError::OperationFailed {
            message: format!("runtime does not support detaching '{workspace_name}'"),
        })
    }

    /// Destroy a workspace runtime.
    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError>;

    /// List all workspace runtimes in a group.
    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>;

    /// List the workspace runtimes in `group` that belong to one project.
    ///
    /// A group is a shared namespace: it also holds sessions of other projects
    /// and sessions the user created by hand. Callers that act on a single
    /// project MUST funnel through this method so the ownership rule lives in
    /// exactly one place.
    fn list_project_workspaces(
        &self,
        group: &str,
        instance_id: &str,
        project_name: &str,
    ) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
        debug_assert!(
            !instance_id.is_empty(),
            "instance_id must be resolved before scoping runtime operations to a project"
        );
        let project_component = SessionNamingPolicy::sanitize_component(project_name);
        Ok(self
            .list_workspaces(group)?
            .into_iter()
            .filter(|ws| {
                SessionNamingPolicy::session_belongs_to_project(
                    &ws.runtime_id,
                    instance_id,
                    &project_component,
                )
            })
            .collect())
    }

    /// Destroy the entire runtime group (all workspaces).
    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError>;
}

/// Unit-level runtime operations.
///
/// Abstracts individual shell/process unit management within a workspace.
pub trait PaneRuntimePort {
    /// Whether this adapter can atomically materialize a configured layout.
    fn supports_configured_layout(&self) -> bool {
        false
    }

    /// Materialize one configured window or tab through a native runtime operation.
    fn materialize_configured_layout(
        &self,
        _request: MaterializeConfiguredLayoutRequest,
    ) -> Result<MaterializedConfiguredLayout, RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "runtime does not support native configured layouts".to_owned(),
        })
    }

    /// Spawn a new unit within a workspace runtime.
    fn spawn_unit(&self, request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError>;

    /// Close a specific unit.
    fn close_unit(&self, group: &str, unit_id: &UnitId) -> Result<(), RuntimeError>;

    /// List all units in a workspace runtime.
    fn list_units(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError>;

    /// Get the currently focused unit within one runtime workspace.
    fn current_unit(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeUnitSnapshot, RuntimeError>;

    /// Send input to a unit.
    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError>;

    /// Restart a unit with the runtime's default shell in `cwd`.
    ///
    /// This is used to repair an idle unit's working directory without
    /// sending a visible command through its shell.
    fn respawn_unit_with_cwd(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        let _ = (group, workspace_name, unit_id, cwd);
        Err(RuntimeError::OperationFailed {
            message: "runtime does not support respawning units".to_owned(),
        })
    }

    /// Restart a unit with an explicit command and working directory.
    ///
    /// Adapters should use a native runtime operation so the command is not
    /// echoed as visible input in the unit.
    fn respawn_unit_with_command(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &camino::Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        let _ = (group, workspace_name, unit_id, cwd, command);
        Err(RuntimeError::OperationFailed {
            message: "runtime does not support respawning units".to_owned(),
        })
    }

    /// Set the display title of a unit (shown in pane borders and status bar).
    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_interaction_policy_default_enables_all() {
        let policy = RuntimeInteractionPolicy::default();
        assert!(policy.mouse_enabled);
        assert!(policy.clipboard_enabled);
    }

    #[test]
    fn tmux_interaction_policy_attach_request_carries_policy() {
        let policy = RuntimeInteractionPolicy {
            mouse_enabled: false,
            clipboard_enabled: true,
        };
        let req = AttachWorkspaceRequest {
            group: "test-grp".into(),
            workspace_name: "test-ws".into(),
            interaction_policy: policy,
        };
        assert!(!req.interaction_policy.mouse_enabled);
        assert!(req.interaction_policy.clipboard_enabled);
    }

    #[test]
    fn tmux_interaction_policy_copy_preserves_values() {
        let original = RuntimeInteractionPolicy {
            mouse_enabled: false,
            clipboard_enabled: false,
        };
        let copied = original;
        assert_eq!(original, copied);
    }
}

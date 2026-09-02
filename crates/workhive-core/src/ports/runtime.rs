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

use crate::domain::{PaneDirection, PaneKind, PaneState, UnitId};

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
    /// Other / generic backend-not-available case — the adapter could
    /// not classify the reason. The CLI renderer falls back to the
    /// stored `detail` string in that case, but logs a `warn`-level
    /// breadcrumb so operators can spot adapters that should be
    /// reporting a typed reason instead.
    Other,
}

impl BackendUnavailableReason {
    /// i18n key for the reason. The CLI boundary uses this with the
    /// runtime locale to render a localized error message.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            Self::BackendBinaryNotFound { .. } => {
                crate::i18n_keys::CLI_RUNTIME_BACKEND_BINARY_NOT_FOUND
            }
            Self::ServerNotRunning { .. } => crate::i18n_keys::CLI_RUNTIME_SERVER_NOT_RUNNING,
            Self::SocketNotFound { .. } => crate::i18n_keys::CLI_RUNTIME_SOCKET_NOT_FOUND,
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
            Self::Other => Vec::new(),
        }
    }
}

/// Errors from workspace or pane runtime operations.
#[derive(Debug, thiserror::Error)]
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
    /// Display name for the workspace environment.
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

/// Request to attach to an existing workspace runtime.
#[derive(Debug, Clone)]
pub struct AttachWorkspaceRequest {
    /// Isolation group.
    pub group: String,
    /// Workspace environment name.
    pub workspace_name: String,
}

/// Summary of a workspace runtime instance.
#[derive(Debug, Clone)]
pub struct WorkspaceRuntimeInfo {
    /// Workspace environment name.
    pub workspace_name: String,
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

    /// Create a new workspace runtime environment.
    fn create_workspace(&self, request: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError>;

    /// Apply default configuration to a workspace runtime.
    fn configure_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError>;

    /// Attach the current terminal to a workspace runtime.
    fn attach_workspace(&self, request: AttachWorkspaceRequest) -> Result<(), RuntimeError>;

    /// Detach the current terminal from its workspace runtime.
    fn detach_workspace(&self, group: &str) -> Result<(), RuntimeError>;

    /// Destroy a workspace runtime.
    fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError>;

    /// List all workspace runtimes in a group.
    fn list_workspaces(&self, group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>;

    /// Destroy the entire runtime group (all workspaces).
    fn destroy_group(&self, group: &str) -> Result<(), RuntimeError>;
}

/// Unit-level runtime operations.
///
/// Abstracts individual shell/process unit management within a workspace.
pub trait PaneRuntimePort {
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

    /// Get the currently focused unit.
    fn current_unit(&self, group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError>;

    /// Send input to a unit.
    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError>;

    /// Set the display title of a unit (shown in pane borders and status bar).
    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError>;
}

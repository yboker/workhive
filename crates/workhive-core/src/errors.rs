//! Typed error model for workhive.
//!
//! Each subsystem has its own error enum with context-carrying variants.
//! All errors use `thiserror` — no string-based control flow.

use camino::Utf8PathBuf;

/// Errors from input validation (workspace names, config keys, etc.).
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("workspace ID must not be empty")]
    EmptyWorkspaceId,

    #[error("invalid workspace ID '{id}': {reason}")]
    InvalidWorkspaceId { id: String, reason: String },

    #[error("invalid config key '{key}': {reason}")]
    InvalidConfigKey { key: String, reason: String },
}

/// Errors from configuration loading, parsing, or writing.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found: {path}")]
    NotFound { path: Utf8PathBuf },

    #[error("failed to read config at {path}")]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid TOML in {path}: {message}")]
    ParseFailed { path: Utf8PathBuf, message: String },

    #[error("failed to write config to {path}")]
    WriteFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("global config not initialized — run 'workhive init --global' first")]
    GlobalNotInitialized,

    #[error("global config already initialized — use --force to reinitialize")]
    AlreadyInitialized,

    #[error("project not initialized — run 'workhive init' first")]
    ProjectNotInitialized,

    #[error("invalid config value for '{field}': {reason}")]
    InvalidValue { field: String, reason: String },
}

/// Errors from Git operations (worktree management, repo discovery).
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git executable not found")]
    GitNotFound,

    #[error("not inside a git repository")]
    NotARepo,

    #[error("failed to create worktree at {path}")]
    WorktreeCreateFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to remove worktree at {path}")]
    WorktreeRemoveFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("git command failed: {message}")]
    CommandFailed { message: String },
}

/// Errors from the global project index.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("failed to read project index at {path}")]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write project index to {path}: {source}")]
    WriteFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("corrupted project index at {path}")]
    Corrupted { path: Utf8PathBuf },

    #[error("index writer lease acquisition exhausted after {attempts} attempts")]
    Exhausted { attempts: usize },
}

/// Errors from i18n message resolution.
#[derive(Debug, thiserror::Error)]
pub enum I18nError {
    #[error("missing translation for key '{key}' in locale '{locale}'")]
    MissingKey { key: String, locale: String },
}

/// Errors from workspace filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceFsError {
    #[error("failed to create directory {path}")]
    CreateDirFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to remove directory {path}")]
    RemoveDirFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("path does not exist: {path}")]
    PathNotFound { path: Utf8PathBuf },

    #[error("failed to inspect directory entry under {path}")]
    ReadDirEntryFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read file {path}")]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write file {path}")]
    WriteFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid workspace metadata at {path}: {reason}")]
    InvalidMetadata { path: Utf8PathBuf, reason: String },
}

/// Errors from project discovery (filesystem scanning for workhive projects).
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error(
        "no discovery roots configured — provide --root or set [discovery] project_roots in global config"
    )]
    NoRootsConfigured,

    #[error("discovery root not found: {path}")]
    RootNotFound { path: Utf8PathBuf },

    #[error("failed to read directory {path}")]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("project metadata invalid at {path}: {reason}")]
    MetadataInvalid { path: Utf8PathBuf, reason: String },
}

/// Errors from environment detection.
#[derive(Debug, thiserror::Error)]
pub enum EnvironmentError {
    #[error("not inside a workhive-managed tmux session")]
    NotInWorkspaceSession,

    #[error(
        "instance_id mismatch: session contains '{session_id}' \
         but project has '{project_id}' — refusing to operate on mismatched session"
    )]
    InstanceIdMismatch {
        session_id: String,
        project_id: String,
    },

    #[error(
        "cannot verify session identity: session '{session_name}' does not contain \
         a parseable instance_id — legacy or non-canonical names are not allowed \
         for identity-sensitive commands"
    )]
    SessionIdentityUnverifiable { session_name: String },
}

/// Errors from orchestration operations.
#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error("workspace not in primary-assisted mode: {workspace}")]
    NotPrimaryAssisted { workspace: String },

    #[error("no primary pane found in workspace: {workspace}")]
    NoPrimaryPane { workspace: String },

    #[error("target worker not found: {worker}")]
    WorkerNotFound { worker: String },

    #[error("target worker lacks required capability: {capability:?}")]
    WorkerLacksCapability {
        capability: crate::domain::PaneCapability,
    },

    #[error("intent {intent:?} requires a target worker")]
    TargetWorkerRequired {
        intent: crate::domain::OrchestrationIntent,
    },

    #[error("intent {intent:?} must not specify a target worker (broadcast)")]
    TargetWorkerForbidden {
        intent: crate::domain::OrchestrationIntent,
    },

    #[error("invalid state transition: {0}")]
    InvalidTransition(#[from] crate::domain::InvalidTransition),

    #[error("registry error: {0}")]
    Registry(#[from] crate::ports::RegistryError),

    #[error("message bus error: {0}")]
    MessageBus(#[from] crate::ports::PaneMessageBusError),
}

/// Errors from task scheduler operations.
#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("task not found: {task_id}")]
    TaskNotFound { task_id: String },

    #[error("task storage error: {message}")]
    Storage { message: String },

    #[error(transparent)]
    InvalidTransition(#[from] crate::domain::InvalidTaskTransition),
}

/// Errors from project initialization transactions.
#[derive(Debug, thiserror::Error)]
pub enum ProjectInitError {
    #[error("project initialization denied: not in interactive mode and no --init flag")]
    DeniedNonInteractive,

    #[error("project initialization declined by user")]
    DeclinedByUser,

    #[error(
        "project is partially initialized at {root}: dir={has_dir}, instance_id={has_instance_id}, config={has_config}"
    )]
    PartiallyInitialized {
        root: Utf8PathBuf,
        has_dir: bool,
        has_instance_id: bool,
        has_config: bool,
    },

    #[error("failed to acquire project init lock at {lock_path}")]
    LockFailed {
        lock_path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("project init transaction rolled back at {root}: {reason}")]
    TransactionRolledBack { root: Utf8PathBuf, reason: String },

    #[error("failed to create staging directory at {path}")]
    StagingFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to commit project init at {root}")]
    CommitFailed {
        root: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    WorkspaceFs(#[from] WorkspaceFsError),

    #[error(transparent)]
    Init(#[from] crate::usecases::init::InitError),

    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Errors from process launch operations.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("failed to resolve executable: {0}")]
    ExecutableResolution(#[source] std::io::Error),

    #[error("failed to spawn process: {0}")]
    SpawnFailed(#[source] std::io::Error),
}

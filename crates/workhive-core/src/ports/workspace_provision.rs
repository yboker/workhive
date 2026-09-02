//! Workspace provision port — abstracts how workspace root directories
//! are created, located, and removed.
//!
//! The workspace lifecycle no longer assumes Git worktrees. Git is merely
//! one possible provision strategy. Future strategies might include plain
//! directory provisioners, remote provisioners, or container-based ones.

use camino::{Utf8Path, Utf8PathBuf};

use super::git::WorktreeRemovePolicy;

/// Errors from workspace provisioning.
#[derive(Debug, thiserror::Error)]
pub enum ProvisionError {
    #[error("workspace root already exists: {path}")]
    AlreadyExists { path: Utf8PathBuf },

    #[error("workspace root not found: {path}")]
    NotFound { path: Utf8PathBuf },

    #[error("failed to provision workspace root at {path}: {detail}")]
    CreateFailed { path: Utf8PathBuf, detail: String },

    #[error("failed to remove workspace root at {path}: {detail}")]
    RemoveFailed { path: Utf8PathBuf, detail: String },

    #[error("provision backend error: {message}")]
    BackendError { message: String },

    #[error("workspace name '{name}' is not valid for this provider: {reason}")]
    InvalidWorkspaceName { name: String, reason: String },
}

/// Request to provision a workspace root directory.
#[derive(Debug, Clone)]
pub struct ProvisionWorkspaceRequest {
    /// Base directory under which the workspace root will be created.
    pub base_path: Utf8PathBuf,
    /// Logical workspace name (used as directory name).
    pub workspace_name: String,
}

/// How a workspace root was resolved by the provisioner.
///
/// Each variant encodes two independent lifecycle facts:
///   1. Was the root reused or materialized during this call?
///   2. If materialized, did the backing reference already exist?
///
/// Provision adapters must return the truthful variant so that upstream
/// use cases do not need to infer lifecycle from side-channel data.
///
/// Note: using the project root directly (i.e. `WorkspaceProvenance::ProjectRoot`)
/// is NOT a provision outcome — it represents a control-flow decision by the
/// use case to skip provisioning entirely. It lives in the domain
/// provenance model, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvisionOutcome {
    /// A subordinate workspace root already existed and was reused as-is.
    /// No new root and no new reference were created during this call.
    ReusedExistingRoot,
    /// A new subordinate root was materialized during this call.
    /// The backing reference/source already existed before this call.
    MaterializedFromExistingReference,
    /// A new subordinate root was materialized during this call.
    /// The backing reference/source was also created during this call.
    MaterializedFromNewReference,
}

/// Information about a provisioned workspace root.
#[derive(Debug, Clone)]
pub struct WorkspaceRootInfo {
    /// Absolute path to the workspace root directory.
    pub path: Utf8PathBuf,
    /// How this root was resolved — truthful lifecycle from the adapter.
    pub outcome: ProvisionOutcome,
}

/// Abstracts workspace root directory provisioning.
///
/// Implementations decide *how* a workspace directory comes into existence.
/// The core business logic only cares that a directory path is returned,
/// not whether it was created via `git worktree add`, `mkdir`, or a
/// remote mount.
pub trait WorkspaceProvisionPort {
    /// Ensure a workspace root directory exists, creating it if necessary.
    fn ensure_workspace_root(
        &self,
        request: ProvisionWorkspaceRequest,
    ) -> Result<WorkspaceRootInfo, ProvisionError>;

    /// Remove a provisioned workspace root directory.
    fn remove_workspace_root(
        &self,
        path: &Utf8Path,
        policy: WorktreeRemovePolicy,
    ) -> Result<(), ProvisionError>;
}

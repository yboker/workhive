use camino::Utf8PathBuf;

use crate::domain::{
    ReattachOutcome, RuntimeBinding, SessionAvailability, WorkspaceId, WorkspaceProvenance,
    classify_reattach_outcome,
};
use crate::ports::{ProvisionError, RuntimeError};

/// How `workhive workspace open` should behave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Reuse a live runtime when present; otherwise create one from the effective layout.
    Normal,
    /// `workhive workspace open -f`: destroy any live runtime and recreate from layout.
    ForceRecreateFromLayout,
}

/// Errors from the `open` use case.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    #[error(transparent)]
    Provision(#[from] ProvisionError),
    #[error("session lookup failed for '{session}': {availability}")]
    SessionLookupFailed {
        session: String,
        availability: SessionAvailability,
    },
    #[error("invocation cwd '{cwd}' is not inside any Git worktree")]
    CwdOutsideTopology { cwd: Utf8PathBuf },
    #[error("mapped target directory does not exist: {path}")]
    MappedTargetMissing { path: Utf8PathBuf },
    #[error("mapped target path is not a directory: {path}")]
    MappedTargetNotDirectory { path: Utf8PathBuf },
}

/// Resolved open scope produced by topology-first resolution.
#[derive(Debug, Clone)]
pub struct ResolvedOpenScope {
    pub selected_workspace_root: Utf8PathBuf,
    pub relative_project_path: Option<crate::domain::RelativeProjectPath>,
    pub working_dir: Utf8PathBuf,
    pub provenance: WorkspaceProvenance,
}

/// Existing session discovered before `open` executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingSession {
    Canonical { server: String, session: String },
    Legacy { server: String, session: String },
}

impl ExistingSession {
    pub fn server(&self) -> &str {
        match self {
            Self::Canonical { server, .. } | Self::Legacy { server, .. } => server,
        }
    }

    pub fn session(&self) -> &str {
        match self {
            Self::Canonical { session, .. } | Self::Legacy { session, .. } => session,
        }
    }
}

/// Declarative plan for opening a workspace.
#[derive(Debug, Clone)]
pub struct OpenPlan {
    pub workspace: WorkspaceId,
    pub provenance: WorkspaceProvenance,
    pub server: String,
    pub session: String,
    pub session_availability: SessionAvailability,
    /// Canonical project checkout that a backend-native checkout
    /// registration groups child workspaces under.
    pub project_root: Utf8PathBuf,
    /// Root of the selected checkout, before any invocation-relative suffix.
    pub workspace_root: Utf8PathBuf,
    pub working_dir: Utf8PathBuf,
    /// Delay backend-native checkout grouping until an old runtime using the
    /// same checkout has been retired.
    pub defer_checkout_registration: bool,
    pub relative_project_path: Option<crate::domain::RelativeProjectPath>,
    pub open_mode: OpenMode,
    pub env: Vec<(String, String)>,
    pub interaction_policy: crate::ports::RuntimeInteractionPolicy,
}

/// Which runtime path was taken during `open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRecoveryPath {
    ReusedLiveSession,
    CreatedFromLayout,
    ForceRecreatedFromLayout,
}

impl std::fmt::Display for OpenRecoveryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReusedLiveSession => f.write_str("reused live session"),
            Self::CreatedFromLayout => f.write_str("created from layout"),
            Self::ForceRecreatedFromLayout => f.write_str("force-recreated from layout"),
        }
    }
}

/// Consolidated user-facing outcome of an `open` operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    ExistingSessionReused,
    LayoutApplied,
    ForceRecreated,
}

impl OpenOutcome {
    pub fn is_session_reused(&self) -> bool {
        matches!(self, Self::ExistingSessionReused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedRuntimeOwnership {
    CreatedByOperation,
    AdoptedExisting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRuntimeBinding {
    pub binding: RuntimeBinding,
    pub ownership: PreparedRuntimeOwnership,
}

/// Result of executing an `OpenPlan`.
pub struct OpenResult {
    pub session_name: String,
    /// Runtime prepared by this open and its compensation ownership.
    pub prepared_runtime: Option<PreparedRuntimeBinding>,
    pub worktree_path: Utf8PathBuf,
    pub provenance: WorkspaceProvenance,
    pub recovery_path: OpenRecoveryPath,
    pub outcome: OpenOutcome,
    pub reattach_outcome: ReattachOutcome,
}

pub(crate) fn reattach_outcome_for_path(
    recovery_path: &OpenRecoveryPath,
    availability: SessionAvailability,
) -> ReattachOutcome {
    match recovery_path {
        OpenRecoveryPath::ReusedLiveSession => ReattachOutcome::Reattached,
        OpenRecoveryPath::CreatedFromLayout => classify_reattach_outcome(&availability, true),
        OpenRecoveryPath::ForceRecreatedFromLayout => ReattachOutcome::CreatedFresh,
    }
}

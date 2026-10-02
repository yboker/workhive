//! Workspace identity port — abstracts identity metadata persistence.
//!
//! Identity metadata lives under `.workhive/identity/`:
//! - `project.json` — project-level invariants
//! - `workspaces/<subdir_key>__<workspace>.json` — per-workspace identity records

use crate::domain::{
    IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
};

/// Abstraction over workspace identity metadata storage.
///
/// Implementations must guarantee atomic writes (no partial metadata on disk).
pub trait WorkspaceIdentityPort {
    /// Load the project-level identity record, if it exists.
    fn load_project_identity(&self) -> Result<Option<ProjectIdentityRecord>, IdentityError>;

    /// Save the project-level identity record (atomic write).
    ///
    /// Callers must not call this when a conflicting record already exists
    /// — use `load_project_identity` first and handle conflicts explicitly.
    fn save_project_identity(&self, record: &ProjectIdentityRecord) -> Result<(), IdentityError>;

    /// Load a workspace identity record by typed key (workspace + subdir).
    fn load_workspace_identity(
        &self,
        key: &WorkspaceIdentityKey,
    ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError>;

    /// Save a workspace identity record (atomic write).
    ///
    /// Idempotent: if the record already exists and matches, this is a no-op.
    fn save_workspace_identity(
        &self,
        record: &WorkspaceIdentityRecord,
    ) -> Result<(), IdentityError>;

    /// Atomically replace the complete project workspace-identity set.
    ///
    /// On success, readers observe exactly `records`; on failure, they continue
    /// to observe the previous complete set.
    fn replace_workspace_identities(
        &self,
        _records: &[WorkspaceIdentityRecord],
    ) -> Result<(), IdentityError> {
        Err(IdentityError::WriteFailed {
            path: "<workspace-identity-batch>".into(),
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "atomic workspace identity replacement is unsupported",
            ),
        })
    }

    /// List all workspace identity records for the project.
    fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError>;
}

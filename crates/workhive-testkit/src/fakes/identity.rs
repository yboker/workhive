//! Fake identity adapter for testing.

use std::cell::RefCell;
use std::collections::HashMap;

use camino::Utf8PathBuf;
use workhive_core::domain::{
    IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
    WorkspaceStateKey,
};
use workhive_core::ports::WorkspaceIdentityPort;

/// In-memory fake for `WorkspaceIdentityPort`.
pub struct FakeIdentityPort {
    project: RefCell<Option<ProjectIdentityRecord>>,
    workspaces: RefCell<HashMap<WorkspaceIdentityKey, WorkspaceIdentityRecord>>,
}

impl FakeIdentityPort {
    pub fn new() -> Self {
        Self {
            project: RefCell::new(None),
            workspaces: RefCell::new(HashMap::new()),
        }
    }
}

impl Default for FakeIdentityPort {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceIdentityPort for FakeIdentityPort {
    fn load_project_identity(&self) -> Result<Option<ProjectIdentityRecord>, IdentityError> {
        Ok(self.project.borrow().clone())
    }

    fn save_project_identity(&self, record: &ProjectIdentityRecord) -> Result<(), IdentityError> {
        *self.project.borrow_mut() = Some(record.clone());
        Ok(())
    }

    fn load_workspace_identity(
        &self,
        key: &WorkspaceIdentityKey,
    ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError> {
        Ok(self.workspaces.borrow().get(key).cloned())
    }

    fn save_workspace_identity(
        &self,
        record: &WorkspaceIdentityRecord,
    ) -> Result<(), IdentityError> {
        self.workspaces
            .borrow_mut()
            .insert(record.identity_key(), record.clone());
        Ok(())
    }

    fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
        Ok(self.workspaces.borrow().values().cloned().collect())
    }

    fn derive_state_key(
        &self,
        key: &WorkspaceIdentityKey,
    ) -> Result<WorkspaceStateKey, IdentityError> {
        let proj =
            self.project
                .borrow()
                .clone()
                .ok_or_else(|| IdentityError::ProjectIdentityMissing {
                    path: Utf8PathBuf::from("<fake>"),
                })?;
        let ws = self.workspaces.borrow().get(key).cloned().ok_or_else(|| {
            IdentityError::WorkspaceIdentityMissing {
                workspace: key.to_string(),
                path: Utf8PathBuf::from("<fake>"),
            }
        })?;
        Ok(WorkspaceStateKey::from_identity(&proj, &ws))
    }
}

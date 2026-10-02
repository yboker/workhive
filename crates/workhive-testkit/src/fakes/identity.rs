//! Fake identity adapter for testing.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use workhive_core::domain::{
    IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
};
use workhive_core::ports::WorkspaceIdentityPort;

/// In-memory fake for `WorkspaceIdentityPort`.
pub struct FakeIdentityPort {
    project: RefCell<Option<ProjectIdentityRecord>>,
    workspaces: RefCell<HashMap<WorkspaceIdentityKey, WorkspaceIdentityRecord>>,
    fail_next_workspace_save: Cell<bool>,
}

impl FakeIdentityPort {
    pub fn new() -> Self {
        Self {
            project: RefCell::new(None),
            workspaces: RefCell::new(HashMap::new()),
            fail_next_workspace_save: Cell::new(false),
        }
    }
}

impl FakeIdentityPort {
    /// Make the next workspace identity save fail, for transaction tests.
    pub fn fail_next_workspace_save(&self) {
        self.fail_next_workspace_save.set(true);
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
        if self.fail_next_workspace_save.replace(false) {
            return Err(IdentityError::WriteFailed {
                path: "<fake-workspace-identity>".into(),
                source: std::io::Error::other("injected workspace identity save failure"),
            });
        }
        self.workspaces
            .borrow_mut()
            .insert(record.identity_key(), record.clone());
        Ok(())
    }

    fn replace_workspace_identities(
        &self,
        records: &[WorkspaceIdentityRecord],
    ) -> Result<(), IdentityError> {
        if self.fail_next_workspace_save.replace(false) {
            return Err(IdentityError::WriteFailed {
                path: "<fake-workspace-identities>".into(),
                source: std::io::Error::other("injected workspace identity save failure"),
            });
        }
        let replacement = records
            .iter()
            .cloned()
            .map(|record| (record.identity_key(), record))
            .collect::<HashMap<_, _>>();
        if replacement.len() != records.len() {
            return Err(IdentityError::Corrupted {
                path: "<fake-workspace-identities>".into(),
                reason: "duplicate workspace identity key".to_owned(),
            });
        }
        *self.workspaces.borrow_mut() = replacement;
        Ok(())
    }

    fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
        Ok(self.workspaces.borrow().values().cloned().collect())
    }
}

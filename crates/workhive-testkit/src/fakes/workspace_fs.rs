//! Fake workspace filesystem adapter for testing.

use std::cell::RefCell;
use std::collections::HashSet;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::WorkspaceFsError;
use workhive_core::ports::WorkspaceFsPort;

/// In-memory fake for `WorkspaceFsPort`.
pub struct FakeWorkspaceFs {
    created_dirs: RefCell<HashSet<Utf8PathBuf>>,
    workspace_dirs: RefCell<Vec<String>>,
    instance_ids: RefCell<std::collections::HashMap<Utf8PathBuf, String>>,
    existing_roots: RefCell<HashSet<Utf8PathBuf>>,
    workspace_last_used: RefCell<std::collections::HashMap<(Utf8PathBuf, String), u64>>,
    configs_written: RefCell<HashSet<Utf8PathBuf>>,
}

impl FakeWorkspaceFs {
    pub fn new() -> Self {
        Self {
            created_dirs: RefCell::new(HashSet::new()),
            workspace_dirs: RefCell::new(Vec::new()),
            instance_ids: RefCell::new(std::collections::HashMap::new()),
            existing_roots: RefCell::new(HashSet::new()),
            workspace_last_used: RefCell::new(std::collections::HashMap::new()),
            configs_written: RefCell::new(HashSet::new()),
        }
    }

    /// Mark a project as having a config.toml written.
    pub fn mark_config_written(&self, project_root: &Utf8Path) {
        self.configs_written
            .borrow_mut()
            .insert(project_root.to_owned());
    }

    /// Pre-populate workspace directories for list tests.
    pub fn with_workspaces(self, names: Vec<String>) -> Self {
        *self.workspace_dirs.borrow_mut() = names;
        self
    }

    /// Register a single workspace directory (e.g., after open creates a worktree).
    pub fn register_workspace(&self, name: &str) {
        let mut dirs = self.workspace_dirs.borrow_mut();
        if !dirs.iter().any(|d| d == name) {
            dirs.push(name.to_owned());
        }
    }

    /// Mark a project root as existing.
    pub fn with_existing_root(self, root: &str) -> Self {
        self.existing_roots
            .borrow_mut()
            .insert(Utf8PathBuf::from(root));
        self
    }

    /// Set the last-used timestamp for a workspace (epoch seconds).
    pub fn with_workspace_last_used(
        self,
        project_root: &str,
        workspace_name: &str,
        epoch_secs: u64,
    ) -> Self {
        self.workspace_last_used.borrow_mut().insert(
            (Utf8PathBuf::from(project_root), workspace_name.to_owned()),
            epoch_secs,
        );
        self
    }
}

impl Default for FakeWorkspaceFs {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceFsPort for FakeWorkspaceFs {
    fn project_root_exists(&self, project_root: &Utf8Path) -> bool {
        self.existing_roots.borrow().contains(project_root)
    }

    fn create_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
        self.created_dirs
            .borrow_mut()
            .insert(project_root.to_owned());
        Ok(())
    }

    fn remove_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
        self.created_dirs.borrow_mut().remove(project_root);
        Ok(())
    }

    fn workhive_dir_exists(&self, project_root: &Utf8Path) -> bool {
        self.created_dirs.borrow().contains(project_root)
    }

    fn workhive_dir_path(&self, project_root: &Utf8Path) -> Utf8PathBuf {
        project_root.join(".workhive")
    }

    fn list_workspace_dirs(
        &self,
        _project_root: &Utf8Path,
    ) -> Result<Vec<String>, WorkspaceFsError> {
        Ok(self.workspace_dirs.borrow().clone())
    }

    fn read_instance_id(&self, project_root: &Utf8Path) -> Option<String> {
        self.instance_ids.borrow().get(project_root).cloned()
    }

    fn write_instance_id(&self, project_root: &Utf8Path, id: &str) -> Result<(), WorkspaceFsError> {
        self.instance_ids
            .borrow_mut()
            .insert(project_root.to_owned(), id.to_owned());
        Ok(())
    }

    fn project_config_exists(&self, project_root: &Utf8Path) -> bool {
        self.configs_written.borrow().contains(project_root)
    }

    fn worktree_dir_exists(&self, _project_root: &Utf8Path, worktree_name: &str) -> bool {
        self.workspace_dirs
            .borrow()
            .iter()
            .any(|d| d == worktree_name)
    }

    fn workspace_last_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
    ) -> Option<u64> {
        self.workspace_last_used
            .borrow()
            .get(&(project_root.to_owned(), workspace_name.to_owned()))
            .copied()
    }

    fn mark_workspace_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
        epoch_secs: u64,
    ) -> Result<(), WorkspaceFsError> {
        self.workspace_last_used.borrow_mut().insert(
            (project_root.to_owned(), workspace_name.to_owned()),
            epoch_secs,
        );
        Ok(())
    }

    fn commit_staged_workhive(
        &self,
        staging_root: &Utf8Path,
        project_root: &Utf8Path,
    ) -> Result<(), WorkspaceFsError> {
        {
            let mut ids = self.instance_ids.borrow_mut();
            if let Some(id) = ids.remove(staging_root) {
                ids.insert(project_root.to_owned(), id);
            }
        }

        {
            let mut configs = self.configs_written.borrow_mut();
            if configs.remove(staging_root) {
                configs.insert(project_root.to_owned());
            }
        }

        {
            let mut dirs = self.created_dirs.borrow_mut();
            dirs.remove(staging_root);
            dirs.insert(project_root.to_owned());
        }

        Ok(())
    }

    fn cleanup_staging(&self, staging_root: &Utf8Path) {
        self.created_dirs.borrow_mut().remove(staging_root);
        self.instance_ids.borrow_mut().remove(staging_root);
        self.configs_written.borrow_mut().remove(staging_root);
    }

    fn ensure_gitignore_workhive_entry(
        &self,
        _project_root: &Utf8Path,
    ) -> Result<(), workhive_core::errors::WorkspaceFsError> {
        Ok(())
    }
}

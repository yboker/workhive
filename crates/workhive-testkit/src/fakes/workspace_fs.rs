//! Fake workspace filesystem adapter for testing.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::WorkspaceFsError;
use workhive_core::ports::WorkspaceFsPort;

/// Exact-match directory registry shared by cooperating fakes.
///
/// Tracks the set of directory paths that currently exist. Only paths
/// explicitly added via `register` or materialized by fake operations (e.g.,
/// `FakeGit::ensure_worktree`) return `Directory` from `classify_path`.
/// Descendants of registered paths are not automatically classified as
/// directories; they return `Missing` unless individually registered.
///
/// The registry is cloneable and shareable between `FakeGit` and `FakeWorkspaceFs`
/// so that worktree materialization in one fake is visible to path classification
/// in the other.
#[derive(Clone, Default)]
pub struct FakeDirectoryRegistry(Rc<RefCell<HashSet<Utf8PathBuf>>>);

impl FakeDirectoryRegistry {
    /// Build an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a directory path as existing.
    ///
    /// After registration, `FakeWorkspaceFs::classify_path` returns `Directory`
    /// for this exact path. Descendants are not automatically registered.
    pub fn register(&self, path: impl Into<Utf8PathBuf>) {
        self.0.borrow_mut().insert(path.into());
    }

    /// Remove a directory path from the registry.
    ///
    /// After unregistration, `classify_path` returns `Missing` for this path.
    pub fn unregister(&self, path: &Utf8Path) {
        self.0.borrow_mut().remove(path);
    }

    /// Check whether a path is registered.
    pub fn contains(&self, path: &Utf8Path) -> bool {
        self.0.borrow().contains(path)
    }
}

/// In-memory fake for `WorkspaceFsPort`.
pub struct FakeWorkspaceFs {
    created_dirs: RefCell<HashSet<Utf8PathBuf>>,
    workspace_dirs: RefCell<Vec<String>>,
    instance_ids: RefCell<std::collections::HashMap<Utf8PathBuf, String>>,
    directories: FakeDirectoryRegistry,
    workspace_last_used: RefCell<std::collections::HashMap<(Utf8PathBuf, String), u64>>,
    configs_written: RefCell<HashSet<Utf8PathBuf>>,
}

impl FakeWorkspaceFs {
    pub fn new() -> Self {
        Self {
            created_dirs: RefCell::new(HashSet::new()),
            workspace_dirs: RefCell::new(Vec::new()),
            instance_ids: RefCell::new(std::collections::HashMap::new()),
            directories: FakeDirectoryRegistry::new(),
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
        self.directories.register(Utf8PathBuf::from(root));
        self
    }

    /// Build a fake that shares its directory registry with another fake
    /// (typically `FakeGit`). Materialized worktrees in the git fake become
    /// visible to `classify_path` in this filesystem fake, and vice versa.
    pub fn with_directory_registry(directories: FakeDirectoryRegistry) -> Self {
        Self {
            directories,
            ..Self::new()
        }
    }

    /// Register a directory path as existing.
    ///
    /// After registration, `classify_path` returns `Directory` for this exact
    /// path. Descendants are not automatically registered; each materialized
    /// path must be registered individually.
    pub fn register_directory(&self, path: impl Into<Utf8PathBuf>) {
        self.directories.register(path);
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
        self.directories.contains(project_root)
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

    fn classify_path(
        &self,
        path: &Utf8Path,
    ) -> Result<
        workhive_core::ports::DirectoryClassification,
        workhive_core::errors::WorkspaceFsError,
    > {
        let dirs = self.created_dirs.borrow();
        if self.directories.contains(path) || dirs.contains(path) {
            return Ok(workhive_core::ports::DirectoryClassification::Directory);
        }
        Ok(workhive_core::ports::DirectoryClassification::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::ports::DirectoryClassification;

    #[test]
    fn fake_workspace_fs_descendant_requires_exact_registration() {
        let fs = FakeWorkspaceFs::new().with_existing_root("/tmp/project");
        assert_eq!(
            fs.classify_path(Utf8Path::new("/tmp/project/missing"))
                .unwrap(),
            DirectoryClassification::Missing
        );
        fs.register_directory("/tmp/project/materialized");
        assert_eq!(
            fs.classify_path(Utf8Path::new("/tmp/project/materialized"))
                .unwrap(),
            DirectoryClassification::Directory
        );
    }
}

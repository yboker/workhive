//! Workspace filesystem port — abstracts `.workhive/` directory operations.

use camino::{Utf8Path, Utf8PathBuf};

use crate::errors::WorkspaceFsError;

/// Typed directory classification returned by `WorkspaceFsPort::classify_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryClassification {
    /// Path exists and is a directory.
    Directory,
    /// Path does not exist.
    Missing,
    /// Path exists but is not a directory (e.g. a regular file).
    NotDirectory,
}

/// Abstraction over `.workhive/` directory management.
pub trait WorkspaceFsPort {
    /// Check whether the project root directory itself exists on the filesystem.
    ///
    /// Adapter/fake must implement this explicitly — no default implementation
    /// that touches the real filesystem is permitted in the core port.
    fn project_root_exists(&self, project_root: &Utf8Path) -> bool;

    /// Create the full `.workhive/` directory structure.
    fn create_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError>;

    /// Remove the entire `.workhive/` directory tree.
    fn remove_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError>;

    /// Check whether `.workhive/` exists at the given project root.
    fn workhive_dir_exists(&self, project_root: &Utf8Path) -> bool;

    /// Return the path to the `.workhive/` directory for a project root.
    fn workhive_dir_path(&self, project_root: &Utf8Path) -> Utf8PathBuf;

    /// List all workspace directories under `.workhive/worktrees/`.
    fn list_workspace_dirs(&self, project_root: &Utf8Path)
    -> Result<Vec<String>, WorkspaceFsError>;

    /// Read the project instance ID (4-hex random, generated at init).
    ///
    /// Returns `None` if the file does not exist.
    fn read_instance_id(&self, project_root: &Utf8Path) -> Option<String>;

    /// Write a new instance ID to `.workhive/instance_id`.
    fn write_instance_id(&self, project_root: &Utf8Path, id: &str) -> Result<(), WorkspaceFsError>;

    /// Check whether `.workhive/config.toml` exists at the given project root.
    fn project_config_exists(&self, project_root: &Utf8Path) -> bool;

    /// Check whether a worktree directory exists under `.workhive/worktrees/<name>`.
    fn worktree_dir_exists(&self, project_root: &Utf8Path, worktree_name: &str) -> bool;

    /// Return the last time Workhive successfully opened or attached to a
    /// workspace, as seconds since the UNIX epoch.
    fn workspace_last_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
    ) -> Option<u64>;

    /// Persist the last time Workhive successfully opened or attached to a
    /// workspace.
    fn mark_workspace_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
        epoch_secs: u64,
    ) -> Result<(), WorkspaceFsError>;

    /// Atomically promote a staged `.workhive/` tree to its final location.
    ///
    /// `staging_root` is a temporary directory containing a `.workhive/` subtree.
    /// The implementation moves `{staging_root}/.workhive` to `{project_root}/.workhive`.
    ///
    /// On real filesystems this is an `fs::rename`.
    /// In-memory fakes copy the state and remove the staging entry.
    fn commit_staged_workhive(
        &self,
        staging_root: &Utf8Path,
        project_root: &Utf8Path,
    ) -> Result<(), WorkspaceFsError>;

    /// Remove a staging directory tree left over from a failed init attempt.
    ///
    /// Implementations should best-effort remove the entire `staging_root` tree.
    /// Errors are logged but not propagated — this is a cleanup-only operation.
    fn cleanup_staging(&self, staging_root: &Utf8Path);

    /// Ensure the project `.gitignore` contains the `/.workhive/` entry.
    ///
    /// - If `.gitignore` does not exist, create it with a minimal template.
    /// - If it exists, check for `/.workhive/` or equivalent rules. If absent,
    ///   idempotently append `/.workhive/` without modifying existing content.
    /// - Must never overwrite or remove existing entries.
    fn ensure_gitignore_workhive_entry(
        &self,
        project_root: &Utf8Path,
    ) -> Result<(), WorkspaceFsError>;

    /// Classify a path as `Directory`, `Missing`, or `NotDirectory`.
    ///
    /// Propagates I/O errors rather than coercing them to `Missing`.
    /// Used by topology-first open to validate mapped target directories
    /// before runtime, identity, or layout operations.
    fn classify_path(&self, path: &Utf8Path) -> Result<DirectoryClassification, WorkspaceFsError>;
}

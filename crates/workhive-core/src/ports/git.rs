//! Git port — abstracts all git operations behind a trait.
//!
//! MVP adapter uses `std::process::Command` calling system git.
//! Future adapters may use gitoxide (gix) when its worktree management API stabilizes.

use camino::{Utf8Path, Utf8PathBuf};

use crate::errors::GitError;

/// Removal policy for linked worktrees.
///
/// `Safe` maps to `git worktree remove` and fails on dirty worktrees.
/// `Force` maps to `git worktree remove --force`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeRemovePolicy {
    Safe,
    Force,
}

impl WorktreeRemovePolicy {
    /// Whether the policy requires force removal.
    pub fn is_force(self) -> bool {
        matches!(self, Self::Force)
    }
}

/// Kind of change detected by `git status --porcelain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Staged,
    Unstaged,
    Untracked,
}

/// One changed file entry from a worktree status scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// Raw two-character porcelain status code (`M `, ` M`, `??`, ...).
    pub status_code: String,
    /// Display path (or rename form `old -> new`) from porcelain output.
    pub path: String,
    /// Normalized change kinds represented by this entry.
    pub kinds: Vec<ChangeKind>,
}

/// Parsed status summary for one worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeStatus {
    /// Parsed changed files from porcelain status output.
    pub changed_files: Vec<ChangedFile>,
}

impl WorktreeStatus {
    /// Total changed files represented in this status.
    pub fn total_changed_files(&self) -> usize {
        self.changed_files.len()
    }

    /// Number of files with staged changes.
    pub fn staged_count(&self) -> usize {
        self.changed_files
            .iter()
            .filter(|f| f.kinds.contains(&ChangeKind::Staged))
            .count()
    }

    /// Number of files with unstaged changes.
    pub fn unstaged_count(&self) -> usize {
        self.changed_files
            .iter()
            .filter(|f| f.kinds.contains(&ChangeKind::Unstaged))
            .count()
    }

    /// Number of untracked files.
    pub fn untracked_count(&self) -> usize {
        self.changed_files
            .iter()
            .filter(|f| f.kinds.contains(&ChangeKind::Untracked))
            .count()
    }
}

/// Request to create or reuse a git worktree.
#[derive(Debug, Clone)]
pub struct EnsureWorktreeRequest {
    /// Base path under which the worktree will be created.
    pub base_path: Utf8PathBuf,
    /// Name of the workspace (used as directory name and branch name).
    pub workspace_name: String,
}

/// Information about an existing worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree directory.
    pub path: Utf8PathBuf,
    /// Branch checked out in this worktree.
    pub branch: Option<String>,
    /// Whether this is the main worktree (not a linked one).
    pub is_main: bool,
    /// How `ensure_worktree` resolved this worktree.
    ///
    /// `None` for worktrees returned by `list_worktrees` (no resolution
    /// context). `Some(strategy)` when returned by `ensure_worktree`.
    pub resolved_strategy: Option<WorktreeStrategy>,
}

/// Strategy for how `ensure_worktree` should create or reuse a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeStrategy {
    /// An existing worktree already has the target branch checked out — reuse it.
    ReuseExisting,
    /// The branch exists but is not attached to any worktree — run `git worktree add <path> <branch>`.
    AttachBranch,
    /// The branch does not exist — run `git worktree add -b <branch> <path>`.
    CreateBranch,
}

/// Abstraction over git operations.
///
/// All git interactions in workhive go through this trait.
/// The `GitPort` is designed so that the underlying implementation
/// can be swapped without touching any business logic.
pub trait GitPort {
    /// Create a worktree if it doesn't exist, or return info about the existing one.
    ///
    /// Three-step decision:
    /// 1. If a worktree for the branch already exists → reuse its path.
    /// 2. If branch exists but has no worktree → `git worktree add <path> <branch>`.
    /// 3. If branch doesn't exist → `git worktree add -b <branch> <path>`.
    fn ensure_worktree(&self, request: EnsureWorktreeRequest) -> Result<WorktreeInfo, GitError>;

    /// Remove a linked worktree at the given path.
    fn remove_worktree(
        &self,
        path: &Utf8Path,
        policy: WorktreeRemovePolicy,
    ) -> Result<(), GitError>;

    /// Query changed files in a linked worktree.
    fn worktree_status(&self, path: &Utf8Path) -> Result<WorktreeStatus, GitError>;

    /// Count commits reachable from `branch` but not from `base`.
    fn unmerged_commit_count(&self, base: &str, branch: &str) -> Result<u32, GitError>;

    /// Delete a local branch.
    fn delete_branch(&self, branch: &str) -> Result<(), GitError>;

    /// Discover the root of the current git repository.
    fn repo_root(&self) -> Result<Utf8PathBuf, GitError>;

    /// List all worktrees in the current repository.
    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError>;

    /// Get the current branch name (e.g., `main`, `feature-x`).
    ///
    /// Returns `Err` if in detached HEAD state or not in a git repo.
    fn current_branch(&self) -> Result<String, GitError>;

    /// Check whether a branch with the given name exists locally.
    fn branch_exists(&self, branch: &str) -> Result<bool, GitError>;

    /// Check whether the current repository has at least one commit.
    fn has_commits(&self) -> Result<bool, GitError>;

    /// Create an empty initial commit in the current repository.
    fn create_empty_initial_commit(&self, message: &str) -> Result<(), GitError>;

    /// Initialize a new git repository at `path`.
    fn init_repo(&self, path: &Utf8Path) -> Result<(), GitError>;
}

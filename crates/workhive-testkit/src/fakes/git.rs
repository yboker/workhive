//! Fake git adapter for testing.

use std::cell::RefCell;
use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::GitError;
use workhive_core::ports::{
    EnsureWorktreeRequest, GitPort, ProvisionError, ProvisionOutcome, ProvisionWorkspaceRequest,
    WorkspaceProvisionPort, WorkspaceRootInfo, WorktreeInfo, WorktreeRemovePolicy, WorktreeStatus,
    WorktreeStrategy,
};

/// In-memory fake for `GitPort`.
///
/// Supports three-step worktree decision logic:
/// - Tracks worktrees and known branches independently
/// - `ensure_worktree` checks existing worktrees first, then known branches
///
/// Error injection: call `set_error()` to make subsequent git operations fail.
pub struct FakeGit {
    worktrees: RefCell<Vec<WorktreeInfo>>,
    /// Branches that exist but may or may not have worktrees.
    known_branches: RefCell<Vec<String>>,
    repo_root: Utf8PathBuf,
    /// When set, all GitPort methods return `GitError::CommandFailed` with this message.
    forced_error_message: RefCell<Option<String>>,
    has_commits: RefCell<bool>,
    worktree_statuses: RefCell<HashMap<Utf8PathBuf, WorktreeStatus>>,
    unmerged_counts: RefCell<HashMap<(String, String), u32>>,
    /// Paths where `remove_worktree` always fails regardless of policy.
    remove_blocked_paths: RefCell<Vec<Utf8PathBuf>>,
}

impl FakeGit {
    pub fn new(repo_root: impl Into<Utf8PathBuf>) -> Self {
        let root: Utf8PathBuf = repo_root.into();
        Self {
            worktrees: RefCell::new(vec![WorktreeInfo {
                path: root.clone(),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            }]),
            known_branches: RefCell::new(vec!["main".to_owned()]),
            repo_root: root,
            forced_error_message: RefCell::new(None),
            has_commits: RefCell::new(true),
            worktree_statuses: RefCell::new(HashMap::new()),
            unmerged_counts: RefCell::new(HashMap::new()),
            remove_blocked_paths: RefCell::new(Vec::new()),
        }
    }

    /// Register a branch as existing (without creating a worktree for it).
    pub fn add_known_branch(&self, branch: &str) {
        let mut branches = self.known_branches.borrow_mut();
        if !branches.iter().any(|b| b == branch) {
            branches.push(branch.to_owned());
        }
    }

    /// Add a worktree at a custom path (for testing non-default locations).
    pub fn add_worktree(&self, path: Utf8PathBuf, branch: &str) {
        self.add_known_branch(branch);
        self.worktrees.borrow_mut().push(WorktreeInfo {
            path,
            branch: Some(branch.to_owned()),
            is_main: false,
            resolved_strategy: None,
        });
    }

    /// Set synthetic `git status --porcelain` state for one worktree path.
    pub fn set_worktree_status(&self, path: Utf8PathBuf, status: WorktreeStatus) {
        self.worktree_statuses.borrow_mut().insert(path, status);
    }

    /// Set synthetic unmerged commit count for `(base, branch)`.
    pub fn set_unmerged_count(&self, base: &str, branch: &str, count: u32) {
        self.unmerged_counts
            .borrow_mut()
            .insert((base.to_owned(), branch.to_owned()), count);
    }

    /// Block `remove_worktree` for a specific path (simulates hard removal failure).
    pub fn block_remove_for_path(&self, path: Utf8PathBuf) {
        self.remove_blocked_paths.borrow_mut().push(path);
    }

    /// Make all subsequent `GitPort` calls return `GitError::CommandFailed`.
    pub fn set_error(&self, message: &str) {
        *self.forced_error_message.borrow_mut() = Some(message.to_owned());
    }

    /// Clear any previously set forced error.
    pub fn clear_error(&self) {
        *self.forced_error_message.borrow_mut() = None;
    }

    /// Set whether the fake repository has at least one commit.
    pub fn set_has_commits(&self, has_commits: bool) {
        *self.has_commits.borrow_mut() = has_commits;
    }

    fn check_error(&self) -> Result<(), GitError> {
        if let Some(ref msg) = *self.forced_error_message.borrow() {
            return Err(GitError::CommandFailed {
                message: msg.clone(),
            });
        }
        Ok(())
    }
}

impl GitPort for FakeGit {
    fn ensure_worktree(&self, request: EnsureWorktreeRequest) -> Result<WorktreeInfo, GitError> {
        self.check_error()?;
        let branch = &request.workspace_name;

        let worktrees = self.worktrees.borrow();
        if let Some(existing) = worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch))
        {
            let mut reused = existing.clone();
            reused.resolved_strategy = Some(WorktreeStrategy::ReuseExisting);
            return Ok(reused);
        }
        drop(worktrees);

        let branch_known = self.known_branches.borrow().iter().any(|b| b == branch);
        let strategy = if branch_known {
            WorktreeStrategy::AttachBranch
        } else {
            WorktreeStrategy::CreateBranch
        };

        let default_path = request.base_path.join(branch);
        let info = WorktreeInfo {
            path: default_path,
            branch: Some(branch.to_owned()),
            is_main: false,
            resolved_strategy: Some(strategy),
        };
        self.known_branches.borrow_mut().push(branch.to_owned());
        self.worktrees.borrow_mut().push(info.clone());
        Ok(info)
    }

    fn remove_worktree(
        &self,
        path: &Utf8Path,
        policy: WorktreeRemovePolicy,
    ) -> Result<(), GitError> {
        self.check_error()?;
        if self
            .remove_blocked_paths
            .borrow()
            .iter()
            .any(|p| p.as_path() == path)
        {
            return Err(GitError::CommandFailed {
                message: format!("removal blocked for path: {}", path),
            });
        }
        if matches!(policy, WorktreeRemovePolicy::Safe)
            && self.worktree_status(path)?.total_changed_files() > 0
        {
            return Err(GitError::CommandFailed {
                message: format!("dirty worktree: {}", path),
            });
        }
        self.worktrees
            .borrow_mut()
            .retain(|w| w.path.as_path() != path);
        Ok(())
    }

    fn worktree_status(&self, path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
        self.check_error()?;
        Ok(self
            .worktree_statuses
            .borrow()
            .get(path)
            .cloned()
            .unwrap_or_default())
    }

    fn unmerged_commit_count(&self, base: &str, branch: &str) -> Result<u32, GitError> {
        self.check_error()?;
        Ok(*self
            .unmerged_counts
            .borrow()
            .get(&(base.to_owned(), branch.to_owned()))
            .unwrap_or(&0))
    }

    fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
        self.check_error()?;
        if branch == "main" {
            return Err(GitError::CommandFailed {
                message: "cannot delete checked-out main branch".to_owned(),
            });
        }
        let mut branches = self.known_branches.borrow_mut();
        if let Some(index) = branches.iter().position(|entry| entry == branch) {
            branches.remove(index);
            Ok(())
        } else {
            Err(GitError::CommandFailed {
                message: format!("branch not found: {branch}"),
            })
        }
    }

    fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
        self.check_error()?;
        Ok(self.repo_root.clone())
    }

    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
        self.check_error()?;
        Ok(self.worktrees.borrow().clone())
    }

    fn current_branch(&self) -> Result<String, GitError> {
        self.check_error()?;
        Ok("main".to_owned())
    }

    fn branch_exists(&self, branch: &str) -> Result<bool, GitError> {
        self.check_error()?;
        Ok(self.known_branches.borrow().iter().any(|b| b == branch))
    }

    fn has_commits(&self) -> Result<bool, GitError> {
        self.check_error()?;
        Ok(*self.has_commits.borrow())
    }

    fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
        self.check_error()?;
        *self.has_commits.borrow_mut() = true;
        Ok(())
    }

    fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
        self.check_error()?;
        Ok(())
    }
}

fn map_git_error_to_provision(err: GitError) -> ProvisionError {
    match err {
        GitError::GitNotFound => ProvisionError::BackendError {
            message: "git executable not found".to_owned(),
        },
        GitError::NotARepo => ProvisionError::BackendError {
            message: "not inside a git repository".to_owned(),
        },
        GitError::WorktreeCreateFailed { path, source } => ProvisionError::CreateFailed {
            path,
            detail: source.to_string(),
        },
        GitError::WorktreeRemoveFailed { path, source } => ProvisionError::RemoveFailed {
            path,
            detail: source.to_string(),
        },
        GitError::CommandFailed { message } => ProvisionError::BackendError { message },
    }
}

impl WorkspaceProvisionPort for FakeGit {
    fn ensure_workspace_root(
        &self,
        request: ProvisionWorkspaceRequest,
    ) -> Result<WorkspaceRootInfo, ProvisionError> {
        let wt_info = self
            .ensure_worktree(EnsureWorktreeRequest {
                base_path: request.base_path,
                workspace_name: request.workspace_name,
            })
            .map_err(map_git_error_to_provision)?;

        let outcome = match wt_info.resolved_strategy {
            Some(WorktreeStrategy::ReuseExisting) => ProvisionOutcome::ReusedExistingRoot,
            Some(WorktreeStrategy::AttachBranch) => {
                ProvisionOutcome::MaterializedFromExistingReference
            }
            Some(WorktreeStrategy::CreateBranch) => ProvisionOutcome::MaterializedFromNewReference,
            None => {
                return Err(ProvisionError::BackendError {
                    message: format!(
                        "fake git invariant violated: worktree at {} has no resolved_strategy",
                        wt_info.path
                    ),
                });
            }
        };

        Ok(WorkspaceRootInfo {
            path: wt_info.path,
            outcome,
        })
    }

    fn remove_workspace_root(
        &self,
        path: &Utf8Path,
        policy: WorktreeRemovePolicy,
    ) -> Result<(), ProvisionError> {
        self.remove_worktree(path, policy)
            .map_err(map_git_error_to_provision)
    }
}

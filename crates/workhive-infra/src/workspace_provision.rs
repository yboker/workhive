//! Git-backed workspace provisioner.
//!
//! Wraps `GitPort` to implement `WorkspaceProvisionPort`, bridging the
//! git worktree lifecycle into the generic workspace provision abstraction.

use workhive_core::errors::GitError;
use workhive_core::ports::{
    EnsureWorktreeRequest, GitPort, ProvisionError, ProvisionOutcome, ProvisionWorkspaceRequest,
    WorkspaceProvisionPort, WorkspaceRootInfo, WorktreeRemovePolicy, WorktreeStrategy,
};

use camino::Utf8Path;

/// Adapts a `GitPort` into a `WorkspaceProvisionPort`.
pub struct GitWorktreeProvisioner<'a> {
    git: &'a dyn GitPort,
}

impl<'a> GitWorktreeProvisioner<'a> {
    pub fn new(git: &'a dyn GitPort) -> Self {
        Self { git }
    }
}

fn map_git_error(err: GitError) -> ProvisionError {
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

/// Validate that a workspace name is acceptable as a git branch/ref name.
///
/// Git branch names reject characters and patterns that `WorkspaceId`
/// deliberately allows (because `WorkspaceId` is provider-agnostic).
/// This validation MUST run before any git CLI call so that failures
/// are intentional and typed, never incidental stderr.
fn validate_git_workspace_name(name: &str) -> Result<(), ProvisionError> {
    if name.starts_with('-') {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot start with '-' (ambiguous with CLI flags)".to_owned(),
        });
    }
    if name.contains("//") {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot contain '//' (empty path component)".to_owned(),
        });
    }
    if name.contains('~') {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot contain '~' (tilde is ref ancestry syntax)".to_owned(),
        });
    }
    if name.contains('^') {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot contain '^' (caret is ref parent syntax)".to_owned(),
        });
    }
    if name.ends_with(".lock") {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot end with '.lock' (reserved for lock files)".to_owned(),
        });
    }
    if name.contains("..") {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot contain '..' (range syntax)".to_owned(),
        });
    }
    if name.contains("@{") {
        return Err(ProvisionError::InvalidWorkspaceName {
            name: name.to_owned(),
            reason: "git refs cannot contain '@{' (reflog syntax)".to_owned(),
        });
    }
    Ok(())
}

/// Design invariant: `GitCliAdapter::ensure_worktree()` always sets
/// `resolved_strategy` to `Some(...)`.  The provision adapter therefore
/// has no `MainRoot` path — main-branch provisioning is handled by the
/// calling use case (which skips the provision adapter entirely).
impl WorkspaceProvisionPort for GitWorktreeProvisioner<'_> {
    fn ensure_workspace_root(
        &self,
        request: ProvisionWorkspaceRequest,
    ) -> Result<WorkspaceRootInfo, ProvisionError> {
        validate_git_workspace_name(&request.workspace_name)?;

        let wt_info = self
            .git
            .ensure_worktree(EnsureWorktreeRequest {
                base_path: request.base_path,
                workspace_name: request.workspace_name,
            })
            .map_err(map_git_error)?;

        let outcome = match wt_info.resolved_strategy {
            Some(WorktreeStrategy::ReuseExisting) => ProvisionOutcome::ReusedExistingRoot,
            Some(WorktreeStrategy::AttachBranch) => {
                ProvisionOutcome::MaterializedFromExistingReference
            }
            Some(WorktreeStrategy::CreateBranch) => ProvisionOutcome::MaterializedFromNewReference,
            None => {
                return Err(ProvisionError::BackendError {
                    message: format!(
                        "git adapter invariant violated: worktree at {} has no resolved_strategy",
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
        self.git
            .remove_worktree(path, policy)
            .map_err(map_git_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use workhive_core::ports::{WorktreeInfo, WorktreeStatus};

    /// Minimal stub that returns `resolved_strategy: None`,
    /// simulating a broken or non-git backend.
    struct NoneStrategyGit;

    struct FixedStrategyGit {
        strategy: WorktreeStrategy,
    }

    impl GitPort for NoneStrategyGit {
        fn ensure_worktree(
            &self,
            request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, GitError> {
            Ok(WorktreeInfo {
                path: request.base_path.join(&request.workspace_name),
                branch: Some(request.workspace_name),
                is_main: false,
                resolved_strategy: None,
            })
        }

        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), GitError> {
            Ok(())
        }

        fn worktree_status(&self, _path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
            Ok(WorktreeStatus::default())
        }

        fn unmerged_commit_count(&self, _base: &str, _branch: &str) -> Result<u32, GitError> {
            Ok(0)
        }

        fn delete_branch(&self, _branch: &str) -> Result<(), GitError> {
            Ok(())
        }

        fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
            Ok(Utf8PathBuf::from("/stub"))
        }

        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
            Ok(vec![])
        }

        fn current_branch(&self) -> Result<String, GitError> {
            Ok("main".to_owned())
        }

        fn branch_exists(&self, _branch: &str) -> Result<bool, GitError> {
            Ok(false)
        }

        fn has_commits(&self) -> Result<bool, GitError> {
            Ok(true)
        }

        fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
            Ok(())
        }

        fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
            Ok(())
        }
    }

    impl GitPort for FixedStrategyGit {
        fn ensure_worktree(
            &self,
            request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, GitError> {
            Ok(WorktreeInfo {
                path: request.base_path.join(&request.workspace_name),
                branch: Some(request.workspace_name),
                is_main: false,
                resolved_strategy: Some(self.strategy.clone()),
            })
        }

        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), GitError> {
            Ok(())
        }

        fn worktree_status(&self, _path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
            Ok(WorktreeStatus::default())
        }

        fn unmerged_commit_count(&self, _base: &str, _branch: &str) -> Result<u32, GitError> {
            Ok(0)
        }

        fn delete_branch(&self, _branch: &str) -> Result<(), GitError> {
            Ok(())
        }

        fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
            Ok(Utf8PathBuf::from("/stub"))
        }

        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
            Ok(vec![])
        }

        fn current_branch(&self) -> Result<String, GitError> {
            Ok("main".to_owned())
        }

        fn branch_exists(&self, _branch: &str) -> Result<bool, GitError> {
            Ok(false)
        }

        fn has_commits(&self) -> Result<bool, GitError> {
            Ok(true)
        }

        fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
            Ok(())
        }

        fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
            Ok(())
        }
    }

    #[test]
    fn provision_adapter_rejects_none_resolved_strategy() {
        let git = NoneStrategyGit;
        let provisioner = GitWorktreeProvisioner::new(&git);

        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/test"),
            workspace_name: "some-workspace".to_owned(),
        });

        assert!(result.is_err(), "None resolved_strategy must be rejected");
        let err = result.unwrap_err();
        match err {
            ProvisionError::BackendError { ref message } => {
                assert!(
                    message.contains("no resolved_strategy"),
                    "error message must mention the invariant violation: {message}"
                );
            }
            other => panic!("expected BackendError, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_tilde_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "has~tilde".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "has~tilde");
                assert!(
                    reason.contains("tilde"),
                    "reason must mention tilde: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_caret_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "has^caret".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "has^caret");
                assert!(
                    reason.contains("caret"),
                    "reason must mention caret: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_dot_lock_suffix_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "branch.lock".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "branch.lock");
                assert!(
                    reason.contains("lock"),
                    "reason must mention lock: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_leading_dash_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "-bad".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "-bad");
                assert!(
                    reason.contains("'-'"),
                    "reason must mention leading dash: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_double_slash_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "fix//a".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "fix//a");
                assert!(
                    reason.contains("'//'"),
                    "reason must mention double slash: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_rejects_at_brace_with_typed_error() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "foo@{bar".to_owned(),
        });
        assert!(result.is_err());
        match result.unwrap_err() {
            ProvisionError::InvalidWorkspaceName { name, reason } => {
                assert_eq!(name, "foo@{bar");
                assert!(
                    reason.contains("'@{'"),
                    "reason must mention @{{ sequence: {reason}"
                );
            }
            other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
        }
    }

    #[test]
    fn git_provider_accepts_at_and_brace_separately() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: Utf8PathBuf::from("/tmp/ws"),
            workspace_name: "foo@bar{baz".to_owned(),
        });
        assert!(
            result.is_ok(),
            "foo@bar{{baz is valid git ref — '@' and '{{' appear separately, not as '@{{'"
        );
    }

    #[test]
    fn git_provider_accepts_normal_workspace_names() {
        let git = FixedStrategyGit {
            strategy: WorktreeStrategy::CreateBranch,
        };
        let provisioner = GitWorktreeProvisioner::new(&git);
        for name in ["feature-auth", "fix/login-bug", "release", "my-feature-123"] {
            let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
                base_path: Utf8PathBuf::from("/tmp/ws"),
                workspace_name: name.to_owned(),
            });
            assert!(result.is_ok(), "valid git name '{name}' should be accepted");
        }
    }

    #[test]
    fn provision_adapter_maps_all_strategies_exhaustively() {
        use WorktreeStrategy::{AttachBranch, CreateBranch, ReuseExisting};

        let strategies = [
            (ReuseExisting, ProvisionOutcome::ReusedExistingRoot),
            (
                AttachBranch,
                ProvisionOutcome::MaterializedFromExistingReference,
            ),
            (CreateBranch, ProvisionOutcome::MaterializedFromNewReference),
        ];

        for (strategy, expected_outcome) in strategies {
            let git = FixedStrategyGit {
                strategy: strategy.clone(),
            };
            let provisioner = GitWorktreeProvisioner::new(&git);
            let result = provisioner
                .ensure_workspace_root(ProvisionWorkspaceRequest {
                    base_path: Utf8PathBuf::from("/tmp/ws"),
                    workspace_name: "test".to_owned(),
                })
                .expect("fixed strategy stub must succeed");

            assert_eq!(
                result.outcome, expected_outcome,
                "strategy {strategy:?} must map to {expected_outcome:?}"
            );
        }
    }
}

//! Worktree safety analysis shared by destructive commands.
//!
//! This module builds typed risk reports for linked worktrees before mutation.

use camino::Utf8PathBuf;

use crate::errors::GitError;
use crate::ports::{ChangedFile, GitPort};

/// Default maximum number of changed files shown per worktree in reports.
pub const DEFAULT_CHANGED_FILE_DISPLAY_LIMIT: usize = 8;

/// One worktree selected for safety scanning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeScanTarget {
    /// Logical workspace/worktree identifier used by CLI output.
    pub workspace: String,
    /// Branch checked out in this worktree, if known.
    pub branch: Option<String>,
    /// Absolute worktree path.
    pub path: Utf8PathBuf,
}

/// Summary for commits reachable from branch but not from base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmergedCommitSummary {
    pub base_branch: String,
    pub branch: String,
    pub count: u32,
}

/// Risk details for a single worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRisk {
    pub workspace: String,
    pub branch: Option<String>,
    pub path: Utf8PathBuf,
    pub staged_count: usize,
    pub unstaged_count: usize,
    pub untracked_count: usize,
    pub total_changed_files: usize,
    pub displayed_changed_files: Vec<ChangedFile>,
    pub hidden_changed_files: usize,
    pub unmerged_commits: Option<UnmergedCommitSummary>,
}

/// Project-level worktree safety report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeSafetyReport {
    pub base_branch: Option<String>,
    pub scanned_worktrees: usize,
    pub display_limit: usize,
    pub risky_worktrees: Vec<WorktreeRisk>,
}

impl WorktreeSafetyReport {
    /// Whether at least one worktree has dirty files or unmerged commits.
    pub fn has_risks(&self) -> bool {
        !self.risky_worktrees.is_empty()
    }
}

/// Build a typed safety report for the provided worktree targets.
pub fn build_worktree_safety_report(
    git: &dyn GitPort,
    targets: &[WorktreeScanTarget],
    base_branch: Option<&str>,
    display_limit: usize,
) -> Result<WorktreeSafetyReport, GitError> {
    let bounded_limit = display_limit.max(1);
    let mut risky_worktrees = Vec::new();

    for target in targets {
        let status = git.worktree_status(target.path.as_ref())?;
        let total_changed_files = status.total_changed_files();
        let displayed_changed_files = status
            .changed_files
            .iter()
            .take(bounded_limit)
            .cloned()
            .collect::<Vec<_>>();
        let hidden_changed_files =
            total_changed_files.saturating_sub(displayed_changed_files.len());

        let unmerged_commits = match (base_branch, target.branch.as_deref()) {
            (Some(base), Some(branch)) if base != branch => {
                let count = git.unmerged_commit_count(base, branch)?;
                if count == 0 {
                    None
                } else {
                    Some(UnmergedCommitSummary {
                        base_branch: base.to_owned(),
                        branch: branch.to_owned(),
                        count,
                    })
                }
            }
            _ => None,
        };

        let has_risk = total_changed_files > 0 || unmerged_commits.is_some();
        if has_risk {
            risky_worktrees.push(WorktreeRisk {
                workspace: target.workspace.clone(),
                branch: target.branch.clone(),
                path: target.path.clone(),
                staged_count: status.staged_count(),
                unstaged_count: status.unstaged_count(),
                untracked_count: status.untracked_count(),
                total_changed_files,
                displayed_changed_files,
                hidden_changed_files,
                unmerged_commits,
            });
        }
    }

    Ok(WorktreeSafetyReport {
        base_branch: base_branch.map(str::to_owned),
        scanned_worktrees: targets.len(),
        display_limit: bounded_limit,
        risky_worktrees,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};

    use crate::ports::{
        ChangeKind, ChangedFile, EnsureWorktreeRequest, GitPort, WorktreeInfo,
        WorktreeRemovePolicy, WorktreeStatus,
    };

    struct StubGit {
        status: WorktreeStatus,
        unmerged_count: u32,
    }

    impl GitPort for StubGit {
        fn ensure_worktree(
            &self,
            _request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn worktree_status(
            &self,
            _path: &Utf8Path,
        ) -> Result<WorktreeStatus, crate::errors::GitError> {
            Ok(self.status.clone())
        }

        fn unmerged_commit_count(
            &self,
            _base: &str,
            _branch: &str,
        ) -> Result<u32, crate::errors::GitError> {
            Ok(self.unmerged_count)
        }

        fn delete_branch(&self, _branch: &str) -> Result<(), crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn repo_root(&self) -> Result<Utf8PathBuf, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn current_branch(&self) -> Result<String, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn branch_exists(&self, _branch: &str) -> Result<bool, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn has_commits(&self) -> Result<bool, crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn create_empty_initial_commit(
            &self,
            _message: &str,
        ) -> Result<(), crate::errors::GitError> {
            unreachable!("not used in test")
        }

        fn init_repo(&self, _path: &Utf8Path) -> Result<(), crate::errors::GitError> {
            unreachable!("not used in test")
        }
    }

    #[test]
    fn report_bounds_displayed_files_and_keeps_totals() {
        let git = StubGit {
            status: WorktreeStatus {
                changed_files: vec![
                    ChangedFile {
                        status_code: " M".to_owned(),
                        path: "src/a.rs".to_owned(),
                        kinds: vec![ChangeKind::Unstaged],
                    },
                    ChangedFile {
                        status_code: "A ".to_owned(),
                        path: "src/b.rs".to_owned(),
                        kinds: vec![ChangeKind::Staged],
                    },
                    ChangedFile {
                        status_code: "??".to_owned(),
                        path: "notes.txt".to_owned(),
                        kinds: vec![ChangeKind::Untracked],
                    },
                ],
            },
            unmerged_count: 0,
        };

        let report = build_worktree_safety_report(
            &git,
            &[WorktreeScanTarget {
                workspace: "feat".to_owned(),
                branch: Some("feat".to_owned()),
                path: Utf8PathBuf::from("/tmp/repo/.workhive/worktrees/feat"),
            }],
            Some("main"),
            2,
        )
        .expect("report should build");

        assert!(report.has_risks());
        let risk = &report.risky_worktrees[0];
        assert_eq!(risk.total_changed_files, 3);
        assert_eq!(risk.displayed_changed_files.len(), 2);
        assert_eq!(risk.hidden_changed_files, 1);
        assert_eq!(risk.staged_count, 1);
        assert_eq!(risk.unstaged_count, 1);
        assert_eq!(risk.untracked_count, 1);
    }

    #[test]
    fn report_tracks_unmerged_commits_separately() {
        let git = StubGit {
            status: WorktreeStatus::default(),
            unmerged_count: 4,
        };
        let report = build_worktree_safety_report(
            &git,
            &[WorktreeScanTarget {
                workspace: "feat".to_owned(),
                branch: Some("feat".to_owned()),
                path: Utf8PathBuf::from("/tmp/repo/.workhive/worktrees/feat"),
            }],
            Some("main"),
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
        )
        .expect("report should build");

        let risk = &report.risky_worktrees[0];
        assert_eq!(risk.total_changed_files, 0);
        assert_eq!(
            risk.unmerged_commits.as_ref().map(|summary| summary.count),
            Some(4)
        );
    }
}

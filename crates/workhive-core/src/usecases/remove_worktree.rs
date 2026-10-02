//! Remove one linked worktree with shared safety checks.

use camino::Utf8PathBuf;

use crate::domain::RuntimeBinding;
use crate::errors::GitError;
use crate::ports::{
    BackendUnavailableReason, GitPort, ProvisionError, RuntimeError, WorkspaceProvisionPort,
    WorkspaceRuntimePort, WorktreeInfo, WorktreeRemovePolicy,
};
use crate::usecases::worktree_safety::{
    DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, WorktreeSafetyReport, WorktreeScanTarget,
    build_worktree_safety_report,
};

/// Error type for single-worktree removal execution.
#[derive(Debug, thiserror::Error)]
pub enum RemoveWorktreeError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Provision(#[from] ProvisionError),

    #[error(transparent)]
    Git(#[from] GitError),
}

/// Typed request for removing one linked worktree.
#[derive(Debug, Clone)]
pub struct RemoveWorktreeRequest {
    /// Worktree/branch selector from CLI.
    pub worktree: String,
    /// Persisted backend-neutral runtime binding for this workspace.
    pub runtime_binding: RuntimeBinding,
    /// Keep runtime session alive.
    pub keep_session: bool,
    /// Delete local branch after worktree removal.
    pub delete_branch: bool,
    /// Force destructive path (skip safety block).
    pub force: bool,
    /// Optional base branch used for unmerged-commit analysis.
    pub base_branch: Option<String>,
    /// Max changed files shown in report.
    pub changed_file_display_limit: usize,
    /// Workhive-managed worktree base path (e.g. `<project>/.workhive/worktrees`).
    /// Core rejects any resolved target whose path is not under this base.
    pub managed_worktree_base: Utf8PathBuf,
}

impl RemoveWorktreeRequest {
    /// Construct a request from an authoritative persisted runtime binding.
    pub fn for_binding(
        worktree: impl Into<String>,
        runtime_binding: RuntimeBinding,
        managed_worktree_base: impl Into<Utf8PathBuf>,
    ) -> Self {
        Self {
            worktree: worktree.into(),
            runtime_binding,
            keep_session: false,
            delete_branch: false,
            force: false,
            base_branch: None,
            changed_file_display_limit: DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
            managed_worktree_base: managed_worktree_base.into(),
        }
    }
}

/// Typed outcomes for single-worktree removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveWorktreeOutcome {
    Removed {
        worktree: String,
        removed_session: bool,
        removed_worktree: bool,
        deleted_branch: bool,
    },
    SafetyBlocked {
        report: WorktreeSafetyReport,
    },
    NoWorktreeFound {
        worktree: String,
    },
    MainWorktreeForbidden {
        worktree: String,
        path: Utf8PathBuf,
    },
    /// The selector matched a worktree that is not under Workhive management.
    /// No session, worktree, or branch mutation was performed.
    NotWorkhiveManaged {
        worktree: String,
        path: Utf8PathBuf,
        managed_base: Utf8PathBuf,
    },
    BranchDeleteFailed {
        worktree: String,
        branch: String,
        reason: String,
        removed_session: bool,
        removed_worktree: bool,
    },
}

/// Remove one linked worktree using explicit safety policy.
pub fn remove_worktree(
    git: &dyn GitPort,
    provision: &dyn WorkspaceProvisionPort,
    runtime: &dyn WorkspaceRuntimePort,
    request: &RemoveWorktreeRequest,
) -> Result<RemoveWorktreeOutcome, RemoveWorktreeError> {
    let target = match resolve_target_worktree(git, &request.worktree)? {
        Some(found) => found,
        None => {
            return Ok(RemoveWorktreeOutcome::NoWorktreeFound {
                worktree: request.worktree.clone(),
            });
        }
    };

    if target.is_main {
        return Ok(RemoveWorktreeOutcome::MainWorktreeForbidden {
            worktree: request.worktree.clone(),
            path: target.path,
        });
    }

    if !path_strictly_inside(&target.path, &request.managed_worktree_base) {
        return Ok(RemoveWorktreeOutcome::NotWorkhiveManaged {
            worktree: request.worktree.clone(),
            path: target.path,
            managed_base: request.managed_worktree_base.clone(),
        });
    }

    let report = build_worktree_safety_report(
        git,
        &[WorktreeScanTarget {
            workspace: request.worktree.clone(),
            branch: target.branch.clone(),
            path: target.path.clone(),
        }],
        request.base_branch.as_deref(),
        request.changed_file_display_limit,
    )?;

    if report.has_risks() && !request.force {
        return Ok(RemoveWorktreeOutcome::SafetyBlocked { report });
    }

    let removed_session = if request.keep_session {
        false
    } else {
        match runtime.destroy_workspace_binding(&request.runtime_binding) {
            Ok(()) => true,
            // Safety checks have already cleared the worktree for
            // removal. A runtime session that is already gone — the
            // workspace was never opened, the server has been stopped,
            // or the backend socket has been wiped — is benign for
            // this single close-session sub-step and must not block
            // metadata/worktree cleanup. The classification is
            // localized to the removal close-session step on purpose;
            // every other runtime command path continues to surface
            // these failures so callers that depend on an active
            // runtime still see the real error.
            Err(err) if close_session_skip_reason(&err).is_some() => false,
            Err(err) => return Err(RemoveWorktreeError::Runtime(err)),
        }
    };

    let remove_policy = if request.force || report.has_risks() {
        WorktreeRemovePolicy::Force
    } else {
        WorktreeRemovePolicy::Safe
    };
    provision.remove_workspace_root(target.path.as_ref(), remove_policy)?;

    if request.delete_branch
        && let Some(branch) = target.branch.as_deref()
        && let Err(err) = git.delete_branch(branch)
    {
        return Ok(RemoveWorktreeOutcome::BranchDeleteFailed {
            worktree: request.worktree.clone(),
            branch: branch.to_owned(),
            reason: err.to_string(),
            removed_session,
            removed_worktree: true,
        });
    }

    Ok(RemoveWorktreeOutcome::Removed {
        worktree: request.worktree.clone(),
        removed_session,
        removed_worktree: true,
        deleted_branch: request.delete_branch && target.branch.is_some(),
    })
}

/// Classify a [`RuntimeError`] returned by the close-session
/// sub-step of [`remove_worktree`] into a "session is already gone"
/// signal.
///
/// Returns `Some(reason)` for runtime errors that mean the session
/// is observably absent at the close-session sub-step:
///
///   * [`RuntimeError::WorkspaceNotFound`] — the session is already
///     missing from a running backend;
///   * [`RuntimeError::BackendUnavailable`] with reason
///     [`BackendUnavailableReason::SocketNotFound`] — the backend
///     socket has been wiped, so no session can exist on it;
///   * [`RuntimeError::BackendUnavailable`] with reason
///     [`BackendUnavailableReason::ServerNotRunning`] — the backend
///     server is not running, so no session can exist on it.
///
/// Returns `None` for every other variant so real backend failures
/// (binary missing, permission denied, operation failed) still
/// propagate. This classification is scoped to the close-session
/// sub-step of workspace removal; other runtime call sites still
/// see and surface the original error.
///
/// The returned `&'static str` is the human-readable reason intended
/// for logs / diagnostics. The CLI boundary owns the user-facing
/// localized text; this helper does not synthesize one.
fn close_session_skip_reason(err: &RuntimeError) -> Option<&'static str> {
    match err {
        RuntimeError::WorkspaceNotFound { .. } => Some("session already absent"),
        RuntimeError::BackendUnavailable { reason, .. } => match reason {
            BackendUnavailableReason::SocketNotFound { .. } => Some("backend socket absent"),
            BackendUnavailableReason::ServerNotRunning { .. } => Some("backend server not running"),
            BackendUnavailableReason::BackendBinaryNotFound { .. }
            | BackendUnavailableReason::CapabilityMissing { .. }
            | BackendUnavailableReason::Other => None,
        },
        _ => None,
    }
}

fn resolve_target_worktree(
    git: &dyn GitPort,
    selector: &str,
) -> Result<Option<WorktreeInfo>, GitError> {
    let worktrees = git.list_worktrees()?;
    Ok(worktrees.into_iter().find(|worktree| {
        worktree.branch.as_deref() == Some(selector)
            || worktree
                .path
                .file_name()
                .is_some_and(|name| name == selector)
    }))
}

/// Component-aware "is `path` strictly inside `base`?" check.
///
/// Strict means the base directory itself is *not* considered "inside"
/// the base — only entries that produce a non-empty relative suffix
/// qualify. Using component-aware `strip_prefix` instead of plain
/// string comparison rejects lookalike paths such as
/// `<base>-archive/foo`.
fn path_strictly_inside(path: &camino::Utf8Path, base: &camino::Utf8Path) -> bool {
    path.strip_prefix(base)
        .is_ok_and(|rel| !rel.as_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use camino::{Utf8Path, Utf8PathBuf};

    use crate::ports::{
        ChangeKind, ChangedFile, EnsureWorktreeRequest, ProvisionWorkspaceRequest,
        WorkspaceRootInfo, WorktreeStatus, WorktreeStrategy,
    };

    struct StubGit {
        worktrees: Vec<WorktreeInfo>,
        status: WorktreeStatus,
        unmerged_count: u32,
        delete_branch_error: Option<String>,
    }

    impl GitPort for StubGit {
        fn ensure_worktree(
            &self,
            _request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, GitError> {
            unreachable!("not used in test")
        }

        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), GitError> {
            unreachable!("not used in test")
        }

        fn worktree_status(&self, _path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
            Ok(self.status.clone())
        }

        fn unmerged_commit_count(&self, _base: &str, _branch: &str) -> Result<u32, GitError> {
            Ok(self.unmerged_count)
        }

        fn delete_branch(&self, _branch: &str) -> Result<(), GitError> {
            match &self.delete_branch_error {
                Some(message) => Err(GitError::CommandFailed {
                    message: message.clone(),
                }),
                None => Ok(()),
            }
        }

        fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
            unreachable!("not used in test")
        }

        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
            Ok(self.worktrees.clone())
        }

        fn current_branch(&self) -> Result<String, GitError> {
            unreachable!("not used in test")
        }

        fn branch_exists(&self, _branch: &str) -> Result<bool, GitError> {
            unreachable!("not used in test")
        }

        fn has_commits(&self) -> Result<bool, GitError> {
            unreachable!("not used in test")
        }

        fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
            unreachable!("not used in test")
        }

        fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
            unreachable!("not used in test")
        }
    }

    struct StubProvision {
        removed: RefCell<Vec<(Utf8PathBuf, WorktreeRemovePolicy)>>,
    }

    impl StubProvision {
        fn new() -> Self {
            Self {
                removed: RefCell::new(Vec::new()),
            }
        }
    }

    impl WorkspaceProvisionPort for StubProvision {
        fn ensure_workspace_root(
            &self,
            _request: ProvisionWorkspaceRequest,
        ) -> Result<WorkspaceRootInfo, ProvisionError> {
            unreachable!("not used in test")
        }

        fn remove_workspace_root(
            &self,
            path: &Utf8Path,
            policy: WorktreeRemovePolicy,
        ) -> Result<(), ProvisionError> {
            self.removed.borrow_mut().push((path.to_owned(), policy));
            Ok(())
        }
    }

    struct StubRuntime {
        destroyed: RefCell<Vec<(String, String)>>,
        destroy_error: RefCell<Option<RuntimeError>>,
    }

    impl StubRuntime {
        fn new() -> Self {
            Self {
                destroyed: RefCell::new(Vec::new()),
                destroy_error: RefCell::new(None),
            }
        }

        fn with_destroy_error(error: RuntimeError) -> Self {
            Self {
                destroyed: RefCell::new(Vec::new()),
                destroy_error: RefCell::new(Some(error)),
            }
        }
    }

    impl WorkspaceRuntimePort for StubRuntime {
        fn has_workspace(&self, _group: &str, _workspace_name: &str) -> Result<bool, RuntimeError> {
            unreachable!("not used in test")
        }

        fn create_workspace(
            &self,
            _request: crate::ports::CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!("not used in test")
        }

        fn configure_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
            _policy: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!("not used in test")
        }

        fn attach_workspace(
            &self,
            _request: crate::ports::AttachWorkspaceRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!("not used in test")
        }

        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!("not used in test")
        }

        fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
            if let Some(err) = self.destroy_error.borrow_mut().take() {
                return Err(err);
            }
            self.destroyed
                .borrow_mut()
                .push((group.to_owned(), workspace_name.to_owned()));
            Ok(())
        }

        fn destroy_workspace_binding(&self, binding: &RuntimeBinding) -> Result<(), RuntimeError> {
            self.destroy_workspace(&binding.scope_id, &binding.workspace_id)
        }

        fn list_workspaces(
            &self,
            _group: &str,
        ) -> Result<Vec<crate::ports::WorkspaceRuntimeInfo>, RuntimeError> {
            unreachable!("not used in test")
        }

        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!("not used in test")
        }
    }

    const MANAGED_BASE: &str = "/tmp/repo/.workhive/worktrees";

    /// Tmux-shaped request for the cases below. Production callers always hold
    /// a persisted binding and construct through `for_binding`.
    fn tmux_request(worktree: &str) -> RemoveWorktreeRequest {
        RemoveWorktreeRequest::for_binding(
            worktree,
            RuntimeBinding::tmux("srv".to_owned(), "sess".to_owned()),
            MANAGED_BASE,
        )
    }

    fn linked_worktree(name: &str) -> WorktreeInfo {
        WorktreeInfo {
            path: Utf8PathBuf::from(format!("/tmp/repo/.workhive/worktrees/{name}")),
            branch: Some(name.to_owned()),
            is_main: false,
            resolved_strategy: Some(WorktreeStrategy::AttachBranch),
        }
    }

    #[test]
    fn clean_worktree_is_removed_with_safe_policy() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let request = tmux_request("feat");

        let outcome =
            remove_worktree(&git, &provision, &runtime, &request).expect("remove succeeds");
        assert!(matches!(outcome, RemoveWorktreeOutcome::Removed { .. }));
        assert_eq!(runtime.destroyed.borrow().len(), 1);
        assert_eq!(
            provision.removed.borrow().as_slice(),
            &[(
                Utf8PathBuf::from("/tmp/repo/.workhive/worktrees/feat"),
                WorktreeRemovePolicy::Safe
            )]
        );
    }

    #[test]
    fn dirty_worktree_without_force_is_safety_blocked() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus {
                changed_files: vec![ChangedFile {
                    status_code: " M".to_owned(),
                    path: "src/main.rs".to_owned(),
                    kinds: vec![ChangeKind::Unstaged],
                }],
            },
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let request = tmux_request("feat");

        let outcome = remove_worktree(&git, &provision, &runtime, &request).expect("remove runs");
        assert!(matches!(
            outcome,
            RemoveWorktreeOutcome::SafetyBlocked { .. }
        ));
        assert!(runtime.destroyed.borrow().is_empty());
        assert!(provision.removed.borrow().is_empty());
    }

    #[test]
    fn dirty_worktree_with_force_uses_force_policy() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus {
                changed_files: vec![ChangedFile {
                    status_code: "??".to_owned(),
                    path: "notes.txt".to_owned(),
                    kinds: vec![ChangeKind::Untracked],
                }],
            },
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let mut request = tmux_request("feat");
        request.force = true;

        let outcome =
            remove_worktree(&git, &provision, &runtime, &request).expect("remove succeeds");
        assert!(matches!(outcome, RemoveWorktreeOutcome::Removed { .. }));
        assert_eq!(provision.removed.borrow()[0].1, WorktreeRemovePolicy::Force);
    }

    #[test]
    fn main_worktree_is_not_removable() {
        let git = StubGit {
            worktrees: vec![WorktreeInfo {
                path: Utf8PathBuf::from("/tmp/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            }],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let request = tmux_request("main");

        let outcome = remove_worktree(&git, &provision, &runtime, &request).expect("remove runs");
        assert!(matches!(
            outcome,
            RemoveWorktreeOutcome::MainWorktreeForbidden { .. }
        ));
        assert!(runtime.destroyed.borrow().is_empty());
        assert!(provision.removed.borrow().is_empty());
    }

    #[test]
    fn external_worktree_is_rejected_by_core_boundary_check() {
        let git = StubGit {
            worktrees: vec![WorktreeInfo {
                path: Utf8PathBuf::from("/external/repos/feat-ext"),
                branch: Some("feat-ext".to_owned()),
                is_main: false,
                resolved_strategy: None,
            }],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let request = tmux_request("feat-ext");

        let outcome = remove_worktree(&git, &provision, &runtime, &request).expect("remove runs");
        match &outcome {
            RemoveWorktreeOutcome::NotWorkhiveManaged {
                worktree,
                path,
                managed_base,
            } => {
                assert_eq!(worktree, "feat-ext");
                assert_eq!(path.as_str(), "/external/repos/feat-ext");
                assert_eq!(managed_base.as_str(), MANAGED_BASE);
            }
            other => panic!("expected NotWorkhiveManaged, got: {other:?}"),
        }
        assert!(
            runtime.destroyed.borrow().is_empty(),
            "session must not be destroyed for external worktree"
        );
        assert!(
            provision.removed.borrow().is_empty(),
            "provisioner must not be called for external worktree"
        );
    }

    #[test]
    fn managed_base_directory_itself_is_rejected_by_boundary() {
        // The managed base directory must not be treated as a managed
        // worktree even if a stub entry says so.
        let git = StubGit {
            worktrees: vec![WorktreeInfo {
                path: Utf8PathBuf::from(MANAGED_BASE),
                branch: Some("worktrees".to_owned()),
                is_main: false,
                resolved_strategy: None,
            }],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let request = tmux_request("worktrees");

        let outcome = remove_worktree(&git, &provision, &runtime, &request).expect("remove runs");
        assert!(
            matches!(outcome, RemoveWorktreeOutcome::NotWorkhiveManaged { .. }),
            "expected NotWorkhiveManaged, got: {outcome:?}"
        );
        assert!(runtime.destroyed.borrow().is_empty());
        assert!(provision.removed.borrow().is_empty());
    }

    #[test]
    fn path_strictly_inside_helper_rejects_base_and_lookalikes() {
        let base = camino::Utf8Path::new("/tmp/repo/.workhive/worktrees");
        assert!(path_strictly_inside(
            camino::Utf8Path::new("/tmp/repo/.workhive/worktrees/feat"),
            base
        ));
        assert!(path_strictly_inside(
            camino::Utf8Path::new("/tmp/repo/.workhive/worktrees/feat/sub"),
            base
        ));
        assert!(!path_strictly_inside(base, base));
        assert!(!path_strictly_inside(
            camino::Utf8Path::new("/tmp/repo/.workhive/worktrees-archive/feat"),
            base
        ));
        assert!(!path_strictly_inside(
            camino::Utf8Path::new("/elsewhere/sandbox"),
            base
        ));
    }

    /// After safety checks pass, a runtime backend socket that has
    /// already been wiped must be treated as "session already
    /// closed" by the close-session sub-step so workspace metadata
    /// cleanup proceeds. The remove outcome must record
    /// `removed_session = false` (the close was a no-op) while
    /// still reporting `removed_worktree = true`.
    #[test]
    fn close_session_treats_missing_runtime_socket_as_already_closed() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::with_destroy_error(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound {
                path: "/private/tmp/tmux-501/whsrv-19b5-test-workhive-06".to_owned(),
            },
            detail: "socket missing".to_owned(),
        });
        let request = tmux_request("feat");

        let outcome =
            remove_worktree(&git, &provision, &runtime, &request).expect("remove succeeds");
        match outcome {
            RemoveWorktreeOutcome::Removed {
                removed_session,
                removed_worktree,
                ..
            } => {
                assert!(
                    !removed_session,
                    "close-session sub-step ran but was a no-op; \
                     stale runtime must not be reported as actually closed"
                );
                assert!(
                    removed_worktree,
                    "metadata/worktree cleanup must still proceed after safety checks"
                );
            }
            other => panic!("expected Removed with no-op close, got: {other:?}"),
        }
        assert_eq!(
            provision.removed.borrow().len(),
            1,
            "provisioner must still run when the runtime is already gone"
        );
    }

    /// A stopped runtime server (no daemon process) must behave the
    /// same way as a missing socket — the close-session sub-step is
    /// a benign no-op and removal proceeds.
    #[test]
    fn close_session_treats_runtime_server_not_running_as_already_closed() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::with_destroy_error(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: "whsrv-19b5-test-workhive-06".to_owned(),
            },
            detail: "server not running".to_owned(),
        });
        let request = tmux_request("feat");

        let outcome =
            remove_worktree(&git, &provision, &runtime, &request).expect("remove succeeds");
        assert!(matches!(
            outcome,
            RemoveWorktreeOutcome::Removed {
                removed_session: false,
                removed_worktree: true,
                ..
            }
        ));
    }

    /// `RuntimeError::WorkspaceNotFound` continues to be a benign
    /// no-op in the close-session sub-step. This preserves existing
    /// "session already absent" behavior.
    #[test]
    fn close_session_treats_session_not_found_as_already_closed() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::with_destroy_error(RuntimeError::WorkspaceNotFound {
            workspace: "sess".to_owned(),
        });
        let request = tmux_request("feat");

        let outcome =
            remove_worktree(&git, &provision, &runtime, &request).expect("remove succeeds");
        assert!(matches!(
            outcome,
            RemoveWorktreeOutcome::Removed {
                removed_session: false,
                removed_worktree: true,
                ..
            }
        ));
    }

    /// Real runtime failures that do not indicate "session already
    /// gone" must continue to fail the removal so the user sees a
    /// genuine runtime problem instead of a silent data-loss path.
    /// Permission denied is the canonical "real failure" example.
    #[test]
    fn close_session_propagates_real_runtime_failure() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::with_destroy_error(RuntimeError::PermissionDenied {
            detail: "cannot signal runtime server".to_owned(),
        });
        let request = tmux_request("feat");

        let result = remove_worktree(&git, &provision, &runtime, &request);
        assert!(
            matches!(result, Err(RemoveWorktreeError::Runtime(_))),
            "real runtime failures must continue to propagate, got: {result:?}"
        );
        assert!(
            provision.removed.borrow().is_empty(),
            "provisioner must not run when the close-session sub-step fails for a non-benign reason"
        );
    }

    /// A backend binary that is missing entirely (e.g. tmux not
    /// installed) is NOT classified as "session already closed".
    /// The user is in an actionable broken-environment state and
    /// must see that before the workspace metadata is wiped.
    #[test]
    fn close_session_propagates_missing_backend_binary() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: None,
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::with_destroy_error(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::BackendBinaryNotFound {
                binary: "tmux".to_owned(),
            },
            detail: "tmux not installed".to_owned(),
        });
        let request = tmux_request("feat");

        let result = remove_worktree(&git, &provision, &runtime, &request);
        assert!(
            matches!(result, Err(RemoveWorktreeError::Runtime(_))),
            "missing backend binary must continue to propagate, got: {result:?}"
        );
        assert!(provision.removed.borrow().is_empty());
    }

    /// Defensive guard: the close-session skip-classifier must
    /// answer the same way on each variant so future runtime
    /// errors do not accidentally get demoted to "already closed".
    #[test]
    fn close_session_skip_reason_classifies_only_session_already_gone_errors() {
        assert!(
            close_session_skip_reason(&RuntimeError::WorkspaceNotFound {
                workspace: "x".to_owned()
            })
            .is_some()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::SocketNotFound {
                    path: "/tmp/x".to_owned()
                },
                detail: "x".to_owned(),
            })
            .is_some()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::ServerNotRunning {
                    server: "x".to_owned()
                },
                detail: "x".to_owned(),
            })
            .is_some()
        );

        assert!(
            close_session_skip_reason(&RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::BackendBinaryNotFound {
                    binary: "tmux".to_owned()
                },
                detail: "x".to_owned(),
            })
            .is_none()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::Other,
                detail: "x".to_owned(),
            })
            .is_none()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::PermissionDenied {
                detail: "x".to_owned()
            })
            .is_none()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::OperationFailed {
                message: "x".to_owned()
            })
            .is_none()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::WorkspaceAlreadyExists {
                workspace: "x".to_owned()
            })
            .is_none()
        );
        assert!(
            close_session_skip_reason(&RuntimeError::UnitNotFound { id: "x".to_owned() }).is_none()
        );
        assert!(close_session_skip_reason(&RuntimeError::LastUnitCloseRejected).is_none());
    }

    #[test]
    fn branch_delete_failure_is_reported_without_fake_success() {
        let git = StubGit {
            worktrees: vec![linked_worktree("feat")],
            status: WorktreeStatus::default(),
            unmerged_count: 0,
            delete_branch_error: Some("branch is not fully merged".to_owned()),
        };
        let provision = StubProvision::new();
        let runtime = StubRuntime::new();
        let mut request = tmux_request("feat");
        request.delete_branch = true;

        let outcome = remove_worktree(&git, &provision, &runtime, &request).expect("remove runs");
        match outcome {
            RemoveWorktreeOutcome::BranchDeleteFailed {
                removed_worktree,
                reason,
                ..
            } => {
                assert!(removed_worktree);
                assert!(reason.contains("not fully merged"));
            }
            other => panic!("expected BranchDeleteFailed, got: {other:?}"),
        }
    }
}

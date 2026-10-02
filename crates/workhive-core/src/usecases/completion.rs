//! Read-only completion candidate resolvers for shell tab-completion.
//!
//! Workhive's shell completion delegates dynamic argument candidates back
//! to the binary through a hidden subcommand. The hidden subcommand calls
//! these resolvers; they must therefore obey three contracts:
//!
//! 1. **Read-only.** No filesystem writes, no tmux/git mutations, no
//!    configuration mutations — completions trigger on every keystroke.
//! 2. **Failure-tolerant.** Missing tmux server, missing project,
//!    permission errors, IO errors must all degrade to an empty
//!    candidate set rather than an error. A noisy stderr would pollute
//!    the user's completion UI.
//! 3. **Stable ordering.** Candidate output must be deterministic for
//!    a given filesystem state so shells can de-duplicate consistently.
//!
//! These functions are pure use-case logic over ports — adapter wiring
//! lives in the CLI layer.

use camino::Utf8Path;

use crate::ports::{PaneRuntimePort, WorkspaceFsPort};

/// Resolve workspace-name candidates for the given project root.
///
/// Reads the project's `.workhive/worktrees/` directory listing through
/// [`WorkspaceFsPort::list_workspace_dirs`]. Hidden entries (those whose
/// name starts with `.`) and empty names are filtered out so shells do
/// not propose noise such as `.DS_Store`.
///
/// Any port error is treated as "no candidates available" — see the
/// module docs for the rationale.
pub fn workspace_candidates(ws_fs: &dyn WorkspaceFsPort, project_root: &Utf8Path) -> Vec<String> {
    let mut candidates: Vec<String> = ws_fs
        .list_workspace_dirs(project_root)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| !name.is_empty() && !name.starts_with('.'))
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

/// Resolve pane-target candidates for the given runtime workspace.
///
/// Returns the unit identifiers (e.g. tmux pane IDs) currently visible
/// in the workspace runtime. Used by `workhive pane send <TAB>` to
/// propose pane targets without forcing the user to memorize tmux pane
/// IDs.
///
/// Errors from the runtime backend are treated as "no candidates":
/// completion never blocks on tmux availability, and a missing server
/// is the common case (the user has not opened any workspace yet).
pub fn pane_candidates(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    workspace: &str,
) -> Vec<String> {
    let mut candidates: Vec<String> = runtime
        .list_units(server, workspace)
        .unwrap_or_default()
        .into_iter()
        .map(|unit| unit.id.to_string())
        .filter(|id| !id.is_empty())
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PaneKind, PaneState, UnitId};
    use crate::errors::WorkspaceFsError;
    use crate::ports::{
        RuntimeError, RuntimeUnitSnapshot, SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest,
    };
    use camino::Utf8PathBuf;
    use std::cell::RefCell;

    /// Stub WorkspaceFsPort that returns a canned `list_workspace_dirs`
    /// result and panics on every other method. Completion only reads
    /// the workspace listing, so other adapter methods are out of scope.
    struct StubWsFs {
        listing: RefCell<Result<Vec<String>, WorkspaceFsError>>,
    }

    impl StubWsFs {
        fn ok(items: &[&str]) -> Self {
            Self {
                listing: RefCell::new(Ok(items.iter().map(|s| (*s).to_owned()).collect())),
            }
        }

        fn err(err: WorkspaceFsError) -> Self {
            Self {
                listing: RefCell::new(Err(err)),
            }
        }
    }

    impl WorkspaceFsPort for StubWsFs {
        fn project_root_exists(&self, _: &Utf8Path) -> bool {
            true
        }
        fn create_workhive_dir(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn remove_workhive_dir(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn workhive_dir_exists(&self, _: &Utf8Path) -> bool {
            true
        }
        fn workhive_dir_path(&self, _: &Utf8Path) -> Utf8PathBuf {
            Utf8PathBuf::from(".workhive")
        }
        fn list_workspace_dirs(&self, _: &Utf8Path) -> Result<Vec<String>, WorkspaceFsError> {
            // Replace the stub with a placeholder so the caller can ignore
            // the original; tests typically only call once per stub.
            std::mem::replace(&mut *self.listing.borrow_mut(), Ok(Vec::new()))
        }
        fn read_instance_id(&self, _: &Utf8Path) -> Option<String> {
            None
        }
        fn write_instance_id(&self, _: &Utf8Path, _: &str) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn project_config_exists(&self, _: &Utf8Path) -> bool {
            false
        }
        fn worktree_dir_exists(&self, _: &Utf8Path, _: &str) -> bool {
            false
        }
        fn workspace_last_used_epoch_secs(&self, _: &Utf8Path, _: &str) -> Option<u64> {
            None
        }
        fn mark_workspace_used_epoch_secs(
            &self,
            _: &Utf8Path,
            _: &str,
            _: u64,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn commit_staged_workhive(
            &self,
            _: &Utf8Path,
            _: &Utf8Path,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn cleanup_staging(&self, _: &Utf8Path) {}
        fn ensure_gitignore_workhive_entry(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            unreachable!("completion must not write")
        }
        fn classify_path(
            &self,
            _: &Utf8Path,
        ) -> Result<crate::ports::DirectoryClassification, WorkspaceFsError> {
            Ok(crate::ports::DirectoryClassification::Directory)
        }
    }

    #[test]
    fn workspace_candidates_filters_hidden_and_empty() {
        let fs = StubWsFs::ok(&["", ".cache", "feat-a", "feat-b", ".DS_Store"]);
        let candidates = workspace_candidates(&fs, &Utf8PathBuf::from("/tmp/proj"));
        assert_eq!(candidates, vec!["feat-a", "feat-b"]);
    }

    #[test]
    fn workspace_candidates_sorts_and_dedupes() {
        let fs = StubWsFs::ok(&["zeta", "alpha", "alpha", "beta"]);
        let candidates = workspace_candidates(&fs, &Utf8PathBuf::from("/tmp/proj"));
        assert_eq!(candidates, vec!["alpha", "beta", "zeta"]);
    }

    #[test]
    fn workspace_candidates_returns_empty_on_fs_error() {
        let fs = StubWsFs::err(WorkspaceFsError::PathNotFound {
            path: ".workhive".into(),
        });
        let candidates = workspace_candidates(&fs, &Utf8PathBuf::from("/tmp/proj"));
        assert!(
            candidates.is_empty(),
            "FS error must degrade to no candidates, got: {candidates:?}"
        );
    }

    /// Inline fake of [`PaneRuntimePort`] for completion tests.
    ///
    /// `workhive-core` cannot depend on `workhive-testkit`
    /// (workspace-layer rule: testkit -> core, not the other way), so
    /// the dynamic-completion use cases ship their own minimal fake.
    /// Only `list_units` is exercised; everything else short-circuits to
    /// a deterministic test-only error.
    struct FakePaneRuntime {
        units: Vec<&'static str>,
        backend_unavailable: bool,
    }

    impl PaneRuntimePort for FakePaneRuntime {
        fn spawn_unit(
            &self,
            _request: SpawnUnitRequest,
        ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unreachable!("completion must not spawn")
        }
        fn close_unit(&self, _: &str, _: &UnitId) -> Result<(), RuntimeError> {
            unreachable!("completion must not close")
        }
        fn list_units(&self, _: &str, _: &str) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
            if self.backend_unavailable {
                return Err(RuntimeError::BackendUnavailable {
                    reason: crate::ports::BackendUnavailableReason::Other,
                    detail: "tmux server gone".to_owned(),
                });
            }
            Ok(self
                .units
                .iter()
                .enumerate()
                .map(|(i, id)| RuntimeUnitSnapshot {
                    id: UnitId::new(*id),
                    index: u32::try_from(i).unwrap_or(0),
                    cwd: Utf8PathBuf::from("/tmp"),
                    command: String::new(),
                    active: false,
                    kind: PaneKind::Terminal,
                    state: PaneState::Running,
                    tty: None,
                    process_pid: None,
                })
                .collect())
        }
        fn current_unit(&self, _: &str, _: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unreachable!("completion must not query current unit")
        }
        fn send_input(&self, _: SendInputRequest) -> Result<(), RuntimeError> {
            unreachable!("completion must not send input")
        }
        fn set_unit_title(&self, _: SetUnitTitleRequest) -> Result<(), RuntimeError> {
            unreachable!("completion must not set titles")
        }
    }

    #[test]
    fn pane_candidates_returns_unit_ids_sorted() {
        let runtime = FakePaneRuntime {
            units: vec!["unit-2", "unit-0", "unit-1"],
            backend_unavailable: false,
        };
        let candidates = pane_candidates(&runtime, "srv", "sess");
        assert_eq!(
            candidates,
            vec![
                "unit-0".to_owned(),
                "unit-1".to_owned(),
                "unit-2".to_owned()
            ]
        );
    }

    #[test]
    fn pane_candidates_returns_empty_on_runtime_error() {
        let runtime = FakePaneRuntime {
            units: Vec::new(),
            backend_unavailable: true,
        };
        let candidates = pane_candidates(&runtime, "srv", "sess");
        assert!(
            candidates.is_empty(),
            "runtime error must degrade to no candidates, got: {candidates:?}"
        );
    }

    #[test]
    fn pane_candidates_returns_distinct_count() {
        let runtime = FakePaneRuntime {
            units: vec!["unit-0", "unit-1"],
            backend_unavailable: false,
        };
        let candidates = pane_candidates(&runtime, "srv", "sess");
        assert_eq!(candidates.len(), 2);
    }
}

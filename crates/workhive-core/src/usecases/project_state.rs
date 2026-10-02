//! Project state resolution — unified query for project runtime state.
//!
//! Combines `.workhive/` existence, `instance_id` presence, and runtime
//! session availability into a single `ProjectRuntimeState`. CLI handlers
//! consume this instead of duplicating the same multi-source probe logic.
//!
//! This module is runtime-neutral: it depends on `WorkspaceRuntimePort`,
//! not on `TmuxPort` or any tmux-specific types.

use camino::Utf8Path;

use crate::domain::{ProjectInitState, ProjectRuntimeState, RuntimeSessionState};
use crate::ports::{RuntimeError, WorkspaceFsPort, WorkspaceRuntimePort};

/// Resolve the full runtime state for a project at the given root.
///
/// Queries:
/// 1. `.workhive/` existence + `instance_id`
/// 2. Runtime backend/session availability via `WorkspaceRuntimePort`
///
/// The `group` identifies the runtime isolation group (e.g., project server name).
pub fn resolve_project_state(
    ws_fs: &dyn WorkspaceFsPort,
    runtime: &dyn WorkspaceRuntimePort,
    project_root: &Utf8Path,
    group: &str,
) -> Result<ProjectRuntimeState, RuntimeError> {
    let init = resolve_init_state(ws_fs, project_root);
    let session = resolve_runtime_session_state(runtime, group)?;

    Ok(ProjectRuntimeState { init, session })
}

/// Check project initialization state without runtime queries.
///
/// Detects three states:
/// - `Uninitialized`: no `.workhive/` directory at all.
/// - `PartiallyInitialized`: `.workhive/` exists but is missing the
///   instance_id file, indicating an interrupted init.
/// - `Initialized`: `.workhive/` and instance_id are present.
///   `config.toml` is optional — `workhive workspace open` runtime-only init does
///   not create it; only explicit `workhive init` does.
pub fn resolve_init_state(
    ws_fs: &dyn WorkspaceFsPort,
    project_root: &Utf8Path,
) -> ProjectInitState {
    let has_dir = ws_fs.workhive_dir_exists(project_root);
    if !has_dir {
        return ProjectInitState::Uninitialized;
    }

    let instance_id_opt = ws_fs.read_instance_id(project_root);
    let has_instance_id = instance_id_opt.is_some();
    let has_config = ws_fs.project_config_exists(project_root);

    if has_instance_id {
        let instance_id = instance_id_opt.unwrap_or_default();
        ProjectInitState::Initialized { instance_id }
    } else {
        ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        }
    }
}

/// Query runtime session availability for a project's isolation group.
///
/// Translates `RuntimeError::BackendUnavailable` into
/// `RuntimeSessionState::BackendUnavailable` so callers get a clean state
/// value rather than an error for expected "not running" conditions.
///
/// All other errors are propagated — they are unexpected infrastructure failures,
/// including permission errors that should remain visible to the caller.
fn resolve_runtime_session_state(
    runtime: &dyn WorkspaceRuntimePort,
    group: &str,
) -> Result<RuntimeSessionState, RuntimeError> {
    match runtime.list_workspaces(group) {
        Ok(workspaces) => {
            if workspaces.is_empty() {
                Ok(RuntimeSessionState::NoSession)
            } else {
                Ok(RuntimeSessionState::HasSessions)
            }
        }
        Err(RuntimeError::BackendUnavailable { .. }) => Ok(RuntimeSessionState::BackendUnavailable),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;

    use crate::domain::ProjectInitState;
    use crate::ports::{
        AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, RuntimeError, WorkspaceFsPort,
        WorkspaceRuntimeInfo, WorkspaceRuntimePort,
    };

    /// Stub runtime that returns a configurable list_workspaces result.
    struct StubRuntime {
        result: RefCell<Option<Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>>>,
    }

    impl StubRuntime {
        fn returning(result: Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>) -> Self {
            Self {
                result: RefCell::new(Some(result)),
            }
        }
    }

    impl WorkspaceRuntimePort for StubRuntime {
        fn has_workspace(&self, _: &str, _: &str) -> Result<bool, RuntimeError> {
            Ok(false)
        }
        fn create_workspace(&self, _: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn configure_workspace(
            &self,
            _: &str,
            _: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn attach_workspace(&self, _: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn destroy_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            self.result.borrow_mut().take().unwrap_or(Ok(Vec::new()))
        }
        fn destroy_group(&self, _: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    struct StubWsFs;

    impl WorkspaceFsPort for StubWsFs {
        fn project_root_exists(&self, _: &camino::Utf8Path) -> bool {
            false
        }
        fn workhive_dir_exists(&self, _: &camino::Utf8Path) -> bool {
            false
        }
        fn workhive_dir_path(&self, project_root: &camino::Utf8Path) -> camino::Utf8PathBuf {
            project_root.join(".workhive")
        }
        fn list_workspace_dirs(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<Vec<String>, crate::errors::WorkspaceFsError> {
            Ok(Vec::new())
        }
        fn create_workhive_dir(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn read_instance_id(&self, _: &camino::Utf8Path) -> Option<String> {
            None
        }
        fn write_instance_id(
            &self,
            _: &camino::Utf8Path,
            _: &str,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn remove_workhive_dir(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn project_config_exists(&self, _: &camino::Utf8Path) -> bool {
            false
        }
        fn worktree_dir_exists(&self, _: &camino::Utf8Path, _: &str) -> bool {
            false
        }
        fn workspace_last_used_epoch_secs(&self, _: &camino::Utf8Path, _: &str) -> Option<u64> {
            None
        }
        fn mark_workspace_used_epoch_secs(
            &self,
            _: &camino::Utf8Path,
            _: &str,
            _: u64,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn commit_staged_workhive(
            &self,
            _: &camino::Utf8Path,
            _: &camino::Utf8Path,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn cleanup_staging(&self, _: &camino::Utf8Path) {}
        fn ensure_gitignore_workhive_entry(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<(), crate::errors::WorkspaceFsError> {
            Ok(())
        }
        fn classify_path(
            &self,
            _: &camino::Utf8Path,
        ) -> Result<crate::ports::DirectoryClassification, crate::errors::WorkspaceFsError>
        {
            Ok(crate::ports::DirectoryClassification::Directory)
        }
    }

    #[test]
    fn resolve_returns_backend_unavailable_when_backend_not_running() {
        let runtime = StubRuntime::returning(Err(RuntimeError::BackendUnavailable {
            reason: crate::ports::BackendUnavailableReason::Other,
            detail: "no server".to_owned(),
        }));
        let result = resolve_runtime_session_state(&runtime, "test-group");
        assert_eq!(result.unwrap(), RuntimeSessionState::BackendUnavailable);
    }

    #[test]
    fn resolve_returns_no_session_when_workspace_list_empty() {
        let runtime = StubRuntime::returning(Ok(Vec::new()));
        let result = resolve_runtime_session_state(&runtime, "test-group");
        assert_eq!(result.unwrap(), RuntimeSessionState::NoSession);
    }

    #[test]
    fn resolve_returns_has_sessions_when_workspaces_present() {
        let ws = WorkspaceRuntimeInfo {
            runtime_id: "my-ws".to_owned(),
            label: None,
            unit_count: 2,
            attached: false,
        };
        let runtime = StubRuntime::returning(Ok(vec![ws]));
        let result = resolve_runtime_session_state(&runtime, "test-group");
        assert_eq!(result.unwrap(), RuntimeSessionState::HasSessions);
    }

    #[test]
    fn resolve_propagates_permission_denied() {
        let runtime = StubRuntime::returning(Err(RuntimeError::PermissionDenied {
            detail: "access denied".to_owned(),
        }));
        let result = resolve_runtime_session_state(&runtime, "test-group");
        assert!(result.is_err());
        match result.unwrap_err() {
            RuntimeError::PermissionDenied { .. } => {}
            other => panic!("expected PermissionDenied, got: {other:?}"),
        }
    }

    #[test]
    fn resolve_full_state_uninitialized_backend_unavailable() {
        let ws_fs = StubWsFs;
        let runtime = StubRuntime::returning(Err(RuntimeError::BackendUnavailable {
            reason: crate::ports::BackendUnavailableReason::Other,
            detail: "server not running".to_owned(),
        }));
        let state =
            resolve_project_state(&ws_fs, &runtime, "/tmp/project".into(), "group").unwrap();
        assert_eq!(state.init, ProjectInitState::Uninitialized);
        assert_eq!(state.session, RuntimeSessionState::BackendUnavailable);
    }
}

//! `close` use case — detach or kill the current workspace session.

use crate::domain::{RuntimeBackendId, RuntimeBinding};
use crate::ports::{RuntimeError, WorkspaceRuntimePort};

/// Errors from the `close` use case.
#[derive(Debug, thiserror::Error)]
pub enum CloseError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

/// Close result indicating what action was taken.
pub enum CloseAction {
    /// Session was detached (user remains in terminal, session keeps running).
    Detached,
    /// Session was killed.
    Killed,
}

/// Close a workspace using its persisted runtime binding.
///
/// The binding is the authority for the concrete runtime identity; callers do
/// not get to replace it with display names or a newly inferred backend.
pub fn close_session_binding(
    runtime: &dyn WorkspaceRuntimePort,
    binding: &RuntimeBinding,
    kill: bool,
) -> Result<CloseAction, CloseError> {
    if kill {
        runtime.destroy_workspace_binding(binding)?;
        Ok(CloseAction::Killed)
    } else {
        runtime.detach_workspace_binding(binding)?;
        Ok(CloseAction::Detached)
    }
}

/// Close a workspace session by group/workspace pair.
///
/// - With `kill = false`: detach from the workspace (leave it running).
/// - With `kill = true`: kill the workspace.
pub fn close_session(
    runtime: &dyn WorkspaceRuntimePort,
    _backend_id: &RuntimeBackendId,
    group: &str,
    workspace_name: &str,
    kill: bool,
) -> Result<CloseAction, CloseError> {
    if kill {
        runtime.destroy_workspace(group, workspace_name)?;
        Ok(CloseAction::Killed)
    } else {
        runtime.detach_workspace(group, workspace_name)?;
        Ok(CloseAction::Detached)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::RuntimeBackendId;
    use crate::ports::{
        AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, RuntimeInteractionPolicy,
        WorkspaceRuntimeInfo,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    struct FakeRuntime {
        detach_calls: Rc<RefCell<Vec<(String, String)>>>,
        destroy_calls: Rc<RefCell<Vec<(String, String)>>>,
    }

    impl FakeRuntime {
        fn new() -> Self {
            Self {
                detach_calls: Rc::new(RefCell::new(Vec::new())),
                destroy_calls: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }

    impl WorkspaceRuntimePort for FakeRuntime {
        fn has_workspace(&self, _group: &str, _workspace_name: &str) -> Result<bool, RuntimeError> {
            Ok(true)
        }

        fn create_workspace(
            &self,
            _request: CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn configure_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
            _policy: &RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn switch_workspace(
            &self,
            _group: &str,
            _target_workspace: &str,
            _policy: &RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn detach_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
            self.detach_calls
                .borrow_mut()
                .push((group.to_owned(), workspace_name.to_owned()));
            Ok(())
        }

        fn destroy_workspace(&self, group: &str, workspace_name: &str) -> Result<(), RuntimeError> {
            self.destroy_calls
                .borrow_mut()
                .push((group.to_owned(), workspace_name.to_owned()));
            Ok(())
        }

        fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            Ok(vec![])
        }

        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    #[test]
    fn close_detach_calls_runtime_detach() {
        let runtime = FakeRuntime::new();
        let backend_id = RuntimeBackendId::tmux();

        let result = close_session(&runtime, &backend_id, "grp", "ws", false);

        assert!(matches!(result, Ok(CloseAction::Detached)));
        assert_eq!(runtime.detach_calls.borrow().len(), 1);
        assert_eq!(runtime.destroy_calls.borrow().len(), 0);
    }

    #[test]
    fn close_kill_calls_runtime_destroy() {
        let runtime = FakeRuntime::new();
        let backend_id = RuntimeBackendId::tmux();

        let result = close_session(&runtime, &backend_id, "grp", "ws", true);

        assert!(matches!(result, Ok(CloseAction::Killed)));
        assert_eq!(runtime.detach_calls.borrow().len(), 0);
        assert_eq!(runtime.destroy_calls.borrow().len(), 1);
    }
}

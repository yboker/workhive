//! Ordering seam between usage-metadata writes and blocking attach.
//!
//! Attaching transfers the terminal into the workspace runtime and only
//! returns when the user detaches — potentially hours later. Usage
//! metadata ("last used") must therefore be persisted *before* the
//! attach call, so it reflects when the workspace was entered rather
//! than when the session ended.

use workhive_core::ports::{AttachWorkspaceRequest, RuntimeError, WorkspaceRuntimePort};

/// Persist usage metadata, then block on the runtime attach.
///
/// `mark_used` runs exactly once, before `attach_workspace`. It is
/// expected to be non-fatal (log-and-continue) so a metadata write
/// failure never blocks the attach itself.
pub(crate) fn mark_used_then_attach(
    runtime: &dyn WorkspaceRuntimePort,
    request: AttachWorkspaceRequest,
    mark_used: impl FnOnce(),
) -> Result<(), RuntimeError> {
    mark_used();
    runtime.attach_workspace(request)
}

/// Persist usage metadata, then attach only when `attach` is true.
///
/// `workspace open` uses this seam for both attach modes: the usage
/// write always runs after successful workspace preparation, and the
/// blocking attach is skipped entirely under `--no-attach` while the
/// timestamp still records the open event.
pub(crate) fn mark_used_then_maybe_attach(
    runtime: &dyn WorkspaceRuntimePort,
    request: AttachWorkspaceRequest,
    attach: bool,
    mark_used: impl FnOnce(),
) -> Result<(), RuntimeError> {
    mark_used();
    if attach {
        runtime.attach_workspace(request)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use workhive_core::ports::{CreateWorkspaceRuntimeRequest, WorkspaceRuntimeInfo};

    /// Records the order of observable events so the test can assert
    /// the usage write happens strictly before the attach call.
    struct RecordingRuntime {
        events: RefCell<Vec<&'static str>>,
        attach_result: RefCell<Option<RuntimeError>>,
    }

    impl RecordingRuntime {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                attach_result: RefCell::new(None),
            }
        }

        fn failing(err: RuntimeError) -> Self {
            let rt = Self::new();
            *rt.attach_result.borrow_mut() = Some(err);
            rt
        }
    }

    impl WorkspaceRuntimePort for RecordingRuntime {
        fn has_workspace(&self, _: &str, _: &str) -> Result<bool, RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn create_workspace(&self, _: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn configure_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn attach_workspace(&self, _: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            self.events.borrow_mut().push("attach");
            match self.attach_result.borrow_mut().take() {
                Some(err) => Err(err),
                None => Ok(()),
            }
        }
        fn detach_workspace(&self, _: &str) -> Result<(), RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn destroy_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn list_workspaces(&self, _: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
        fn destroy_group(&self, _: &str) -> Result<(), RuntimeError> {
            unreachable!("not exercised by the ordering seam")
        }
    }

    fn request() -> AttachWorkspaceRequest {
        AttachWorkspaceRequest {
            group: "whsrv-instance-demo".to_owned(),
            workspace_name: "wh@instance@demo@main".to_owned(),
        }
    }

    #[test]
    fn usage_is_marked_before_attach_is_called() {
        let runtime = RecordingRuntime::new();
        mark_used_then_attach(&runtime, request(), || {
            runtime.events.borrow_mut().push("mark_used");
        })
        .expect("attach succeeds");

        assert_eq!(
            *runtime.events.borrow(),
            vec!["mark_used", "attach"],
            "usage metadata must be written before the blocking attach"
        );
    }

    #[test]
    fn usage_is_marked_even_when_attach_fails() {
        let runtime = RecordingRuntime::failing(RuntimeError::OperationFailed {
            message: "no tty".to_owned(),
        });
        let result = mark_used_then_attach(&runtime, request(), || {
            runtime.events.borrow_mut().push("mark_used");
        });

        assert!(result.is_err(), "attach failure must propagate");
        assert_eq!(
            *runtime.events.borrow(),
            vec!["mark_used", "attach"],
            "usage write happens before attach regardless of attach outcome"
        );
    }

    #[test]
    fn maybe_attach_marks_usage_before_attach_when_attaching() {
        let runtime = RecordingRuntime::new();
        mark_used_then_maybe_attach(&runtime, request(), true, || {
            runtime.events.borrow_mut().push("mark_used");
        })
        .expect("attach succeeds");

        assert_eq!(
            *runtime.events.borrow(),
            vec!["mark_used", "attach"],
            "usage metadata must be written before the blocking attach"
        );
    }

    #[test]
    fn maybe_attach_marks_usage_and_skips_attach_when_not_attaching() {
        let runtime = RecordingRuntime::new();
        mark_used_then_maybe_attach(&runtime, request(), false, || {
            runtime.events.borrow_mut().push("mark_used");
        })
        .expect("no-attach path succeeds");

        assert_eq!(
            *runtime.events.borrow(),
            vec!["mark_used"],
            "usage metadata must still be written when attach is skipped, \
             and attach_workspace must not be called"
        );
    }
}

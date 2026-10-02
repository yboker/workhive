#[allow(unused_imports)]
use super::*;
use std::cell::RefCell;
use workhive_core::ports::{CreateWorkspaceRuntimeRequest, TmuxMembership, WorkspaceRuntimeInfo};

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
    fn configure_workspace(
        &self,
        _: &str,
        _: &str,
        _: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        unreachable!("not exercised by the ordering seam")
    }
    fn attach_workspace(&self, _: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
        self.events.borrow_mut().push("attach");
        match self.attach_result.borrow_mut().take() {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }
    fn attach_workspace_binding(
        &self,
        _: &RuntimeBinding,
        _: workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.events.borrow_mut().push("attach_binding");
        Ok(())
    }
    fn switch_workspace(
        &self,
        _: &str,
        _: &str,
        _: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), RuntimeError> {
        self.events.borrow_mut().push("switch");
        Ok(())
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
        interaction_policy: workhive_core::ports::RuntimeInteractionPolicy::default(),
    }
}

fn outside_tmux() -> ExecutionContext {
    ExecutionContext {
        is_interactive: true,
        tmux: TmuxMembership::Outside,
    }
}

fn exact_target_ctx() -> ExecutionContext {
    ExecutionContext {
        is_interactive: true,
        tmux: TmuxMembership::Resolved {
            server: "whsrv-instance-demo".to_owned(),
            session: "wh@instance@demo@main".to_owned(),
            pane: Some("%0".to_owned()),
        },
    }
}

fn same_group_ctx() -> ExecutionContext {
    ExecutionContext {
        is_interactive: true,
        tmux: TmuxMembership::Resolved {
            server: "whsrv-instance-demo".to_owned(),
            session: "wh@instance@demo@feature".to_owned(),
            pane: Some("%1".to_owned()),
        },
    }
}

fn cross_group_ctx() -> ExecutionContext {
    ExecutionContext {
        is_interactive: true,
        tmux: TmuxMembership::Resolved {
            server: "whsrv-instance-other".to_owned(),
            session: "wh@instance@other@main".to_owned(),
            pane: Some("%2".to_owned()),
        },
    }
}

fn unresolved_ctx() -> ExecutionContext {
    ExecutionContext {
        is_interactive: true,
        tmux: TmuxMembership::Unresolved {
            reason: "TMUX present but session probe failed".to_owned(),
        },
    }
}

// ── attach_navigation tests ───────────────────────────────────────

#[test]
fn herdr_attach_uses_persisted_binding_without_tmux_navigation() {
    let runtime = RecordingRuntime::new();
    let binding = RuntimeBinding::new(RuntimeBackendId::herdr(), "socket", "opaque-w1").unwrap();

    let result = mark_used_then_attach_binding(
        &runtime,
        &binding,
        request(),
        &cross_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );

    assert_eq!(result.unwrap(), AttachOutcome::Attached);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready", "attach_binding"]
    );
}

#[test]
fn attach_navigation_outside_tmux_guides_then_attaches() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::Attached);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["ready", "guidance", "attach"]
    );
}

#[test]
fn attach_navigation_exact_target_reports_without_runtime_call() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &exact_target_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::AlreadyCurrent);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["already_current"],
        "no runtime attach/switch call"
    );
}

#[test]
fn attach_navigation_same_group_guides_then_switches() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &same_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::Switched);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["ready", "guidance", "switch"]
    );
}

#[test]
fn attach_navigation_cross_group_fails_without_guidance_or_runtime_call() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &cross_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        AttachNavigationError::CrossGroup {
            current_server,
            target_server,
        } => {
            assert_eq!(current_server, "whsrv-instance-other");
            assert_eq!(target_server, "whsrv-instance-demo");
        }
        other => panic!("expected CrossGroup, got: {other:?}"),
    }
    assert!(
        runtime.events.borrow().is_empty(),
        "no guidance or runtime call on cross-group reject"
    );
}

#[test]
fn attach_navigation_no_attach_preserves_metadata_only() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        false,
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready"],
        "no-attach must mark used then emit ready"
    );
}

// ── existing ordering tests ───────────────────────────────────────

#[test]
fn attach_usage_guidance_before_attach() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["ready", "guidance", "attach"]
    );
}

#[test]
fn attach_usage_mark_used_guidance_attach_ordering() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready", "guidance", "attach"]
    );
}

#[test]
fn attach_usage_maybe_attach_full_ordering_when_attaching() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        true,
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready", "guidance", "attach"]
    );
}

#[test]
fn attach_usage_failure_propagates() {
    let runtime = RecordingRuntime::failing(RuntimeError::OperationFailed {
        message: "no tty".to_owned(),
    });
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_err());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["ready", "guidance", "attach"]
    );
}

// ── Metadata ordering tests ────────────────────────────────────────

#[test]
fn attach_navigation_explicit_attach_marks_once_before_attach() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::Attached);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready", "guidance", "attach"]
    );
}

#[test]
fn attach_navigation_explicit_switch_marks_once_before_switch() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &same_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::Switched);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready", "guidance", "switch"]
    );
}

#[test]
fn attach_navigation_exact_target_does_not_mark_or_call_runtime() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &exact_target_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::AlreadyCurrent);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["already_current"],
        "exact target must not mark_used or call runtime"
    );
}

#[test]
fn attach_navigation_cross_group_has_no_side_effects() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &cross_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    assert!(
        runtime.events.borrow().is_empty(),
        "cross-group must have no side effects: no metadata, guidance, or runtime"
    );
}

// ── workspace-open metadata tests ───────────────────────────────

#[test]
fn attach_navigation_open_exact_target_marks_once_without_runtime_call() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        true,
        &exact_target_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), AttachOutcome::AlreadyCurrent);
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "already_current", "ready"],
        "workspace-open exact target must mark_used, emit already_current, then emit ready output, no runtime call"
    );
}

#[test]
fn attach_navigation_open_cross_group_has_no_side_effects() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        true,
        &cross_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    assert!(
        runtime.events.borrow().is_empty(),
        "workspace-open cross-group must have no side effects"
    );
}

#[test]
fn attach_navigation_unresolved_context_has_no_side_effects() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &unresolved_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    assert!(
        runtime.events.borrow().is_empty(),
        "unresolved context must have no side effects"
    );
}

// ── cross-group production mapper tests ───────────────────────────

#[test]
fn attach_navigation_cross_group_mapper_renders_target_then_current_in_all_locales() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;

    let i18n = StaticI18n::new();
    let target = "whsrv-TARGET-proj";
    let current = "whsrv-CURRENT-proj";

    for locale in [Locale::En, Locale::ZhCn] {
        let cross_err = AttachNavigationError::CrossGroup {
            current_server: current.to_owned(),
            target_server: target.to_owned(),
        };
        let cli_err = map_navigation_error(cross_err, &i18n, locale);
        let msg = format!("{cli_err}");
        let target_pos = msg
            .find(target)
            .unwrap_or_else(|| panic!("{locale:?}: target server must appear in message: {msg}"));
        let current_pos = msg
            .find(current)
            .unwrap_or_else(|| panic!("{locale:?}: current server must appear in message: {msg}"));
        assert!(
            target_pos < current_pos,
            "{locale:?}: target must appear BEFORE current in cross-group mapper output.\n\
             target_pos={target_pos}, current_pos={current_pos}\n\
             message: {msg}"
        );
    }
}

// ── ready-output-before-runtime tests ───────────────────────────────

#[test]
fn attach_emits_ready_before_blocking_attach() {
    use std::sync::{Arc, Mutex};
    struct PausingRuntime {
        events: Arc<Mutex<Vec<&'static str>>>,
    }
    impl WorkspaceRuntimePort for PausingRuntime {
        fn has_workspace(&self, _: &str, _: &str) -> Result<bool, RuntimeError> {
            unreachable!()
        }
        fn create_workspace(&self, _: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn configure_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn attach_workspace(&self, _: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            // Check that ready output was already emitted before we enter the blocking call
            let events = self.events.lock().unwrap();
            assert!(
                events.contains(&"ready"),
                "ready output must be emitted before blocking attach"
            );
            drop(events);
            self.events.lock().unwrap().push("attach");
            Ok(())
        }
        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn destroy_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn list_workspaces(&self, _: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            unreachable!()
        }
        fn destroy_group(&self, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let runtime = PausingRuntime {
        events: events.clone(),
    };
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || {},
            emit_ready_output: || events.lock().unwrap().push("ready"),
            emit_guidance: || events.lock().unwrap().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    assert_eq!(*events.lock().unwrap(), vec!["ready", "guidance", "attach"]);
}

#[test]
fn switch_emits_ready_before_blocking_switch() {
    use std::sync::{Arc, Mutex};
    struct PausingRuntime {
        events: Arc<Mutex<Vec<&'static str>>>,
    }
    impl WorkspaceRuntimePort for PausingRuntime {
        fn has_workspace(&self, _: &str, _: &str) -> Result<bool, RuntimeError> {
            unreachable!()
        }
        fn create_workspace(&self, _: CreateWorkspaceRuntimeRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn configure_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn attach_workspace(&self, _: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            let events = self.events.lock().unwrap();
            assert!(
                events.contains(&"ready"),
                "ready output must be emitted before blocking switch"
            );
            drop(events);
            self.events.lock().unwrap().push("switch");
            Ok(())
        }
        fn destroy_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn list_workspaces(&self, _: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            unreachable!()
        }
        fn destroy_group(&self, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let runtime = PausingRuntime {
        events: events.clone(),
    };
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &same_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || events.lock().unwrap().push("mark_used"),
            emit_ready_output: || events.lock().unwrap().push("ready"),
            emit_guidance: || events.lock().unwrap().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *events.lock().unwrap(),
        vec!["mark_used", "ready", "guidance", "switch"]
    );
}

#[test]
fn workspace_open_no_attach_emits_ready_without_runtime() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        false, // no_attach
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "ready"],
        "no-attach must emit ready without runtime call"
    );
}

#[test]
fn workspace_open_already_current_emits_ready_after_already_current() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        true,
        &exact_target_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["mark_used", "already_current", "ready"]
    );
}

#[test]
fn explicit_attach_emits_ready_before_runtime() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    let events = runtime.events.borrow();
    let ready_pos = events.iter().position(|&e| e == "ready").unwrap();
    let attach_pos = events.iter().position(|&e| e == "attach").unwrap();
    assert!(
        ready_pos < attach_pos,
        "ready must be emitted before attach"
    );
}

#[test]
fn explicit_switch_emits_ready_before_runtime() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &same_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    let events = runtime.events.borrow();
    let ready_pos = events.iter().position(|&e| e == "ready").unwrap();
    let switch_pos = events.iter().position(|&e| e == "switch").unwrap();
    assert!(
        ready_pos < switch_pos,
        "ready must be emitted before switch"
    );
}

#[test]
fn already_current_explicit_attach_does_not_emit_ready() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &exact_target_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_ok());
    assert_eq!(
        *runtime.events.borrow(),
        vec!["already_current"],
        "explicit attach already-current must not emit ready"
    );
}

#[test]
fn cross_group_rejection_has_zero_effects() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &cross_group_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    assert!(
        runtime.events.borrow().is_empty(),
        "cross-group rejection must have zero effects"
    );
}

#[test]
fn unresolved_context_has_zero_effects() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_attach(
        &runtime,
        request(),
        &unresolved_ctx(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || runtime.events.borrow_mut().push("already_current"),
        },
    );
    assert!(result.is_err());
    assert!(
        runtime.events.borrow().is_empty(),
        "unresolved context must have zero effects"
    );
}

#[test]
fn workspace_open_attach_emits_ready_before_runtime() {
    let runtime = RecordingRuntime::new();
    let result = mark_used_then_maybe_attach(
        &runtime,
        request(),
        true,
        &outside_tmux(),
        AttachLifecycleCallbacks {
            mark_used: || runtime.events.borrow_mut().push("mark_used"),
            emit_ready_output: || runtime.events.borrow_mut().push("ready"),
            emit_guidance: || runtime.events.borrow_mut().push("guidance"),
            emit_already_current: || {},
        },
    );
    assert!(result.is_ok());
    let events = runtime.events.borrow();
    let ready_pos = events.iter().position(|&e| e == "ready").unwrap();
    let attach_pos = events.iter().position(|&e| e == "attach").unwrap();
    assert!(
        ready_pos < attach_pos,
        "ready must be emitted before attach in workspace-open"
    );
}

use super::output::*;
use super::*;
use workhive_core::domain::{PaneKind, PaneState};

#[test]
fn parse_sessions_handles_typical_output() {
    let sessions = parse_sessions_with_pane_counts(
        "mysession\t3\t1\nother\t1\t0",
        "mysession\nmysession\nmysession\nmysession\nmysession\nother\n",
    );
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "mysession");
    assert_eq!(sessions[0].windows, 3);
    assert_eq!(sessions[0].panes, 5);
    assert!(sessions[0].attached);
    assert_eq!(sessions[1].panes, 1);
    assert!(!sessions[1].attached);
}

#[test]
fn parse_sessions_with_pane_counts_uses_authoritative_pane_rows() {
    // `#{session_panes}` is unsupported by tmux 3.6 and would otherwise
    // produce an empty field that the legacy parser coerces to zero.
    let sessions = parse_sessions_with_pane_counts(
        "mysession\t1\t1\nother\t2\t0",
        "mysession\nmysession\nother\n",
    );
    assert_eq!(sessions[0].panes, 2);
    assert_eq!(sessions[1].panes, 1);
    assert!(sessions[0].attached);
    assert!(!sessions[1].attached);
}

#[test]
fn list_sessions_queries_panes_instead_of_unsupported_session_panes() {
    let executed = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        executed
            .borrow_mut()
            .push(args.iter().map(|arg| (*arg).to_owned()).collect());
        match args.get(2).copied() {
            Some("list-sessions") => Ok("demo\t1\t0".to_owned()),
            Some("list-panes") => Ok("demo\ndemo\n".to_owned()),
            other => panic!("unexpected tmux command: {other:?}"),
        }
    };

    let sessions = TmuxCliAdapter::list_sessions_with_runner("srv", &runner).unwrap();
    assert_eq!(sessions[0].panes, 2);

    let log = executed.borrow();
    assert_eq!(log.len(), 2);
    assert_eq!(log[0][2], "list-sessions");
    assert!(!log[0].iter().any(|arg| arg.contains("session_panes")));
    assert_eq!(
        log[1],
        vec!["-L", "srv", "list-panes", "-a", "-F", "#{session_name}"]
    );
}

#[test]
fn parse_single_pane_valid() {
    let line = "%0\t0\t/Users/me/proj\tzsh\t1";
    let pane = parse_single_pane(line).unwrap();
    assert_eq!(pane.id.as_str(), "%0");
    assert!(pane.active);
    assert_eq!(pane.kind, PaneKind::Terminal);
    assert_eq!(pane.state, PaneState::Idle);
}

#[test]
fn classify_pane_kind_agent() {
    assert_eq!(classify_pane_kind("cursor"), PaneKind::Agent);
    assert_eq!(classify_pane_kind("claude"), PaneKind::Agent);
    assert_eq!(classify_pane_kind("codex"), PaneKind::Agent);
    assert_eq!(classify_pane_kind("aider"), PaneKind::Agent);
    assert_eq!(classify_pane_kind("opencode"), PaneKind::Agent);
}

/// Codex ships as architecture-suffixed launcher binaries such as
/// `codex-x86_64-apple-darwin`. These must still
/// classify as Agent.
#[test]
fn classify_pane_kind_codex_with_arch_suffix() {
    assert_eq!(
        classify_pane_kind("codex-x86_64-apple-darwin"),
        PaneKind::Agent
    );
    assert_eq!(
        classify_pane_kind("codex-aarch64-unknown-linux-gnu"),
        PaneKind::Agent
    );
}

/// The classifier must accept a path-prefixed binary as the same
/// agent — production tmux often reports `pane_current_command`
/// as a full path.
#[test]
fn classify_pane_kind_handles_path_prefixed_command() {
    assert_eq!(classify_pane_kind("/usr/local/bin/codex"), PaneKind::Agent);
    assert_eq!(classify_pane_kind("/opt/Cursor/cursor"), PaneKind::Agent);
}

#[test]
fn classify_pane_kind_gitui() {
    assert_eq!(classify_pane_kind("lazygit"), PaneKind::GitUi);
    assert_eq!(classify_pane_kind("tig"), PaneKind::GitUi);
    assert_eq!(classify_pane_kind("gitui"), PaneKind::GitUi);
}

#[test]
fn classify_pane_kind_terminal_only_for_exact_shell_basenames() {
    assert_eq!(classify_pane_kind("zsh"), PaneKind::Terminal);
    assert_eq!(classify_pane_kind("/bin/bash"), PaneKind::Terminal);
    // Substring matching on `sh` would have wrongly classified
    // `ssh` as Terminal — only
    // canonical shell basenames count as Terminal.
    assert_eq!(classify_pane_kind("ssh"), PaneKind::Unknown);
}

#[test]
fn classify_pane_state_running() {
    assert_eq!(classify_pane_state("node"), PaneState::Running);
    assert_eq!(classify_pane_state("cargo"), PaneState::Running);
    assert_eq!(classify_pane_state("codex"), PaneState::Running);
}

#[test]
fn classify_pane_state_idle_only_for_canonical_shells() {
    assert_eq!(classify_pane_state("zsh"), PaneState::Idle);
    assert_eq!(classify_pane_state("/bin/bash"), PaneState::Idle);
    assert_eq!(classify_pane_state(""), PaneState::Unknown);
}

#[test]
fn classify_server_not_running() {
    let args = &["-L", "whsrv-main", "list-sessions"];
    match classify_tmux_error("no server running on /tmp/tmux-501/whsrv-main", args) {
        TmuxError::ServerNotRunning { server } => assert_eq!(server, "whsrv-main"),
        other => panic!("expected ServerNotRunning, got: {other:?}"),
    }
}

#[test]
fn classify_no_sessions() {
    let args = &["-L", "whsrv-test", "list-sessions"];
    match classify_tmux_error("no sessions", args) {
        TmuxError::ServerNotRunning { server } => assert_eq!(server, "whsrv-test"),
        other => panic!("expected ServerNotRunning, got: {other:?}"),
    }
}

#[test]
fn classify_server_not_found() {
    let args = &["-L", "srv", "has-session"];
    match classify_tmux_error("server not found", args) {
        TmuxError::ServerNotRunning { .. } => {}
        other => panic!("expected ServerNotRunning, got: {other:?}"),
    }
}

#[test]
fn classify_server_exited_unexpectedly_as_not_running() {
    let args = &["-L", "test-server", "list-sessions"];
    match classify_tmux_error("server exited unexpectedly", args) {
        TmuxError::ServerNotRunning { server } => assert_eq!(server, "test-server"),
        other => panic!("expected ServerNotRunning, got {other:?}"),
    }
}

#[test]
fn classify_socket_not_found() {
    let args = &["-L", "srv", "list-sessions"];
    let stderr = "error connecting to /private/tmp/tmux-501/whsrv-abc (No such file or directory)";
    match classify_tmux_error(stderr, args) {
        TmuxError::SocketNotFound { path } => {
            assert!(path.contains("whsrv-abc"));
        }
        other => panic!("expected SocketNotFound, got: {other:?}"),
    }
}

#[test]
fn classify_permission_denied() {
    let args = &["-L", "srv", "list-sessions"];
    match classify_tmux_error(
        "error connecting to /tmp/tmux-501/default (Permission denied)",
        args,
    ) {
        TmuxError::PermissionDenied { detail } => {
            assert!(detail.contains("Permission denied"));
        }
        other => panic!("expected PermissionDenied, got: {other:?}"),
    }
}

#[test]
fn classify_session_not_found() {
    let args = &["-L", "srv", "has-session", "-t", "test"];
    match classify_tmux_error("can't find session: test", args) {
        TmuxError::SessionNotFound { name } => assert_eq!(name, "test"),
        other => panic!("expected SessionNotFound, got: {other:?}"),
    }
}

#[test]
fn classify_unknown_falls_to_command_failed() {
    let args = &["-L", "srv", "list-sessions"];
    match classify_tmux_error("ambiguous session: test", args) {
        TmuxError::CommandFailed { message } => {
            assert!(message.contains("ambiguous"));
        }
        other => panic!("expected CommandFailed, got: {other:?}"),
    }
}

#[test]
fn classify_duplicate_session() {
    let args = &["-L", "srv", "new-session", "-d", "-s", "feat-a"];
    match classify_tmux_error("duplicate session: feat-a", args) {
        TmuxError::SessionAlreadyExists { name } => assert_eq!(name, "feat-a"),
        other => panic!("expected SessionAlreadyExists, got: {other:?}"),
    }
}

#[test]
fn session_create_not_verified_error_includes_context() {
    let err = TmuxError::SessionCreateNotVerified {
        server: "whsrv-proj".to_owned(),
        name: "wh@inst@proj@ws".to_owned(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("wh@inst@proj@ws"),
        "must include session name: {msg}"
    );
    assert!(
        msg.contains("whsrv-proj"),
        "must include server name: {msg}"
    );
}

#[test]
fn extract_server_arg_from_args() {
    assert_eq!(
        extract_server_arg(&["-L", "myserver", "list-sessions"]),
        "myserver"
    );
    assert_eq!(extract_server_arg(&["list-sessions"]), "default");
}

#[test]
fn tmux_navigation_attach_uses_attach_session() {
    let request = AttachRequest {
        server_name: "whsrv-abcd-demo".to_owned(),
        session_name: "wh@abcd@demo@main".to_owned(),
    };
    let command = build_attach_command(&request);
    let args: Vec<_> = command
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"attach-session".to_owned()));
    assert!(args.contains(&"-L".to_owned()));
    assert!(args.contains(&"whsrv-abcd-demo".to_owned()));
    assert!(args.contains(&"-t".to_owned()));
    assert!(args.contains(&"wh@abcd@demo@main".to_owned()));
}

#[test]
fn tmux_navigation_switch_uses_production_runner() {
    let executed = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        executed
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(String::new())
    };
    TmuxCliAdapter::switch_client_with_runner("whsrv-inst-proj", "wh@inst@proj@feat", &runner)
        .expect("switch must succeed");
    let log = executed.borrow();
    assert_eq!(log.len(), 1, "exactly one call");
    assert_eq!(
        log[0],
        vec![
            "-L",
            "whsrv-inst-proj",
            "switch-client",
            "-t",
            "wh@inst@proj@feat"
        ],
        "exact ordered argv"
    );
}

#[test]
fn tmux_navigation_switch_error_propagates_without_retry() {
    let call_count = std::cell::RefCell::new(0u32);
    let runner = |_args: &[&str]| -> Result<String, TmuxError> {
        *call_count.borrow_mut() += 1;
        Err(TmuxError::CommandFailed {
            message: "switch failed".to_owned(),
        })
    };
    let result =
        TmuxCliAdapter::switch_client_with_runner("whsrv-inst-proj", "wh@inst@proj@feat", &runner);
    assert!(result.is_err(), "error must propagate");
    assert_eq!(*call_count.borrow(), 1, "exactly one call, no retry");
}

#[test]
fn tmux_navigation_never_clears_tmux_to_force_nesting() {
    let request = AttachRequest {
        server_name: "whsrv-abcd-demo".to_owned(),
        session_name: "wh@abcd@demo@main".to_owned(),
    };
    let command = build_attach_command(&request);
    let removed: Vec<_> = command
        .get_envs()
        .filter_map(|(key, value)| {
            value
                .is_none()
                .then_some(key.to_string_lossy().into_owned())
        })
        .collect();
    assert!(
        !removed.iter().any(|key| key == "TMUX"),
        "attach must not remove TMUX to force nesting"
    );
    assert!(
        !removed.iter().any(|key| key == "TMUX_PANE"),
        "attach must not remove TMUX_PANE to force nesting"
    );
}

// ─── Policy planner tests ────────────────────────────────────────────────

use workhive_core::ports::RuntimeInteractionPolicy;

fn has_session_option(plan: &SessionConfigPlan, key: &str, value: &str) -> bool {
    plan.ops.iter().any(|op| {
        matches!(op,
            SessionConfigOp::SetSessionOption { key: k, value: v }
            if *k == key && *v == value
        )
    })
}

fn has_bind(plan: &SessionConfigPlan, key: &str) -> bool {
    plan.ops
        .iter()
        .any(|op| matches!(op, SessionConfigOp::BindCopyModeKey { key: k, .. } if *k == key))
}

fn has_unbind(plan: &SessionConfigPlan, key: &str) -> bool {
    plan.ops
        .iter()
        .any(|op| matches!(op, SessionConfigOp::UnbindCopyModeKey { key: k } if *k == key))
}

fn has_set_marker(plan: &SessionConfigPlan, key: &str) -> bool {
    plan.ops
        .iter()
        .any(|op| matches!(op, SessionConfigOp::SetWorkhiveMarker { key: k } if *k == key))
}

fn has_clear_marker(plan: &SessionConfigPlan, key: &str) -> bool {
    plan.ops
        .iter()
        .any(|op| matches!(op, SessionConfigOp::ClearWorkhiveMarker { key: k } if *k == key))
}

fn both_absent() -> Vec<(&'static str, BindingOwnership)> {
    vec![
        ("y", BindingOwnership::Absent),
        ("MouseDragEnd1Pane", BindingOwnership::Absent),
    ]
}

fn both_workhive() -> Vec<(&'static str, BindingOwnership)> {
    vec![
        ("y", BindingOwnership::WorkhiveOwned),
        ("MouseDragEnd1Pane", BindingOwnership::WorkhiveOwned),
    ]
}

fn both_user() -> Vec<(&'static str, BindingOwnership)> {
    vec![
        ("y", BindingOwnership::UserOwned),
        ("MouseDragEnd1Pane", BindingOwnership::UserOwned),
    ]
}

#[test]
fn tmux_policy_plan_mouse_on_clipboard_on_absent() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    let plan = plan_session_config(&policy, &both_absent(), false);
    assert!(has_session_option(&plan, "mouse", "on"));
    assert!(has_session_option(&plan, "set-clipboard", "external"));
    assert!(has_bind(&plan, "y"));
    assert!(has_bind(&plan, "MouseDragEnd1Pane"));
    assert!(has_set_marker(&plan, "y"));
    assert!(has_set_marker(&plan, "MouseDragEnd1Pane"));
}

#[test]
fn tmux_policy_plan_mouse_off_clipboard_off_absent() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: false,
        clipboard_enabled: false,
    };
    let plan = plan_session_config(&policy, &both_absent(), false);
    assert!(has_session_option(&plan, "mouse", "off"));
    assert!(has_session_option(&plan, "set-clipboard", "off"));
    assert!(!has_bind(&plan, "y"));
    assert!(!has_unbind(&plan, "y"));
}

#[test]
fn tmux_policy_plan_includes_base_options() {
    let policy = RuntimeInteractionPolicy::default();
    let plan = plan_session_config(&policy, &both_absent(), false);
    assert!(has_session_option(&plan, "mode-keys", "vi"));
    assert!(has_session_option(&plan, "history-limit", "50000"));
}

// ─── Marker-based binding ownership tests ────────────────────────────────

#[test]
fn tmux_binding_marker_enable_absent_installs_and_marks() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    let plan = plan_session_config(&policy, &both_absent(), false);
    assert!(has_bind(&plan, "y"));
    assert!(has_set_marker(&plan, "y"));
    assert!(has_bind(&plan, "MouseDragEnd1Pane"));
    assert!(has_set_marker(&plan, "MouseDragEnd1Pane"));
}

#[test]
fn tmux_binding_marker_enable_user_binding_preserved() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    let plan = plan_session_config(&policy, &both_user(), false);
    assert!(
        !has_bind(&plan, "y"),
        "user-owned y must not be overwritten"
    );
    assert!(!has_set_marker(&plan, "y"));
    assert!(
        !has_bind(&plan, "MouseDragEnd1Pane"),
        "user-owned MouseDragEnd1Pane must not be overwritten"
    );
}

#[test]
fn tmux_binding_marker_enable_workhive_owned_reapplied() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    let plan = plan_session_config(&policy, &both_workhive(), false);
    assert!(has_bind(&plan, "y"), "workhive-owned y is reapplied");
    assert!(!has_set_marker(&plan, "y"), "marker already set");
}

#[test]
fn tmux_binding_marker_enable_stale_marker_clears_marker() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: true,
    };
    let stale = vec![
        ("y", BindingOwnership::StaleMarker),
        ("MouseDragEnd1Pane", BindingOwnership::StaleMarker),
    ];
    let plan = plan_session_config(&policy, &stale, false);
    assert!(
        !has_bind(&plan, "y"),
        "stale marker must not overwrite user binding"
    );
    assert!(has_clear_marker(&plan, "y"), "stale marker must be cleared");
}

#[test]
fn tmux_binding_marker_disable_workhive_owned_unbinds_and_clears() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let plan = plan_session_config(&policy, &both_workhive(), false);
    assert!(has_unbind(&plan, "y"));
    assert!(has_clear_marker(&plan, "y"));
    assert!(has_unbind(&plan, "MouseDragEnd1Pane"));
    assert!(has_clear_marker(&plan, "MouseDragEnd1Pane"));
}

#[test]
fn tmux_binding_marker_disable_user_bindings_preserved() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let plan = plan_session_config(&policy, &both_user(), false);
    assert!(!has_unbind(&plan, "y"), "user-owned must not be unbound");
    assert!(!has_clear_marker(&plan, "y"));
}

#[test]
fn tmux_binding_marker_disable_absent_no_ops() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let plan = plan_session_config(&policy, &both_absent(), false);
    assert!(!has_unbind(&plan, "y"));
    assert!(!has_clear_marker(&plan, "y"));
}

#[test]
fn tmux_binding_marker_disable_stale_clears_marker_only() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let stale = vec![
        ("y", BindingOwnership::StaleMarker),
        ("MouseDragEnd1Pane", BindingOwnership::StaleMarker),
    ];
    let plan = plan_session_config(&policy, &stale, false);
    assert!(!has_unbind(&plan, "y"), "stale must not unbind");
    assert!(has_clear_marker(&plan, "y"), "stale marker must be cleared");
}

#[test]
fn tmux_binding_marker_full_lifecycle() {
    let absent = both_absent();
    let enable = plan_session_config(
        &RuntimeInteractionPolicy {
            mouse_enabled: true,
            clipboard_enabled: true,
        },
        &absent,
        false,
    );
    assert!(has_bind(&enable, "y") && has_set_marker(&enable, "y"));

    let owned = both_workhive();
    let disable = plan_session_config(
        &RuntimeInteractionPolicy {
            mouse_enabled: true,
            clipboard_enabled: false,
        },
        &owned,
        false,
    );
    assert!(has_unbind(&disable, "y") && has_clear_marker(&disable, "y"));
}

#[test]
fn tmux_binding_marker_mixed_ownership_on_disable() {
    let policy = RuntimeInteractionPolicy {
        mouse_enabled: true,
        clipboard_enabled: false,
    };
    let mixed = vec![
        ("y", BindingOwnership::UserOwned),
        ("MouseDragEnd1Pane", BindingOwnership::WorkhiveOwned),
    ];
    let plan = plan_session_config(&policy, &mixed, false);
    assert!(!has_unbind(&plan, "y"), "user-owned y must not be unbound");
    assert!(has_unbind(&plan, "MouseDragEnd1Pane"));
    assert!(has_clear_marker(&plan, "MouseDragEnd1Pane"));
}

// ─── Detection tests (via fake runner) ───────────────────────────────────

#[test]
fn tmux_binding_detection_error_permission_propagates() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Err(TmuxError::PermissionDenied {
                detail: "tmux socket".to_owned(),
            });
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner);
    assert!(result.is_err(), "permission error must propagate");
}

#[test]
fn tmux_binding_detection_error_server_not_running_propagates() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Err(TmuxError::ServerNotRunning {
                server: "srv".to_owned(),
            });
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner);
    assert!(result.is_err(), "server-not-running must propagate");
}

#[test]
fn tmux_binding_detection_error_absent_binding_is_ok() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Ok(String::new());
        }
        if args.contains(&"list-keys") {
            let queried = queried_key_from_args(args);
            return Err(TmuxError::CommandFailed {
                message: format!("invalid key: {queried}"),
            });
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner).expect("must succeed");
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].1, BindingOwnership::Absent);
    assert_eq!(result[1].1, BindingOwnership::Absent);
}

#[test]
fn tmux_binding_detection_error_marker_present_command_matches() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Ok("installed".to_owned());
        }
        if args.contains(&"list-keys") {
            return Ok(
                "bind-key -T copy-mode-vi y send-keys -X copy-selection-and-cancel".to_owned(),
            );
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner).expect("must succeed");
    assert_eq!(result[0].1, BindingOwnership::WorkhiveOwned);
}

#[test]
fn tmux_binding_detection_error_marker_present_command_changed() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Ok("installed".to_owned());
        }
        if args.contains(&"list-keys") {
            return Ok("bind-key -T copy-mode-vi y send-keys -X copy-pipe pbcopy".to_owned());
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner).expect("must succeed");
    assert_eq!(result[0].1, BindingOwnership::StaleMarker);
}

#[test]
fn tmux_binding_detection_error_no_marker_binding_exists() {
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Ok(String::new());
        }
        if args.contains(&"list-keys") {
            return Ok("bind-key -T copy-mode-vi y send-keys -X copy-pipe pbcopy".to_owned());
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner).expect("must succeed");
    assert_eq!(result[0].1, BindingOwnership::UserOwned);
}

// ─── Single-call and classification tests ────────────────────────────────

fn queried_key_from_args<'a>(args: &[&'a str]) -> &'a str {
    args.iter()
        .position(|a| *a == "copy-mode-vi")
        .and_then(|i| args.get(i + 1))
        .copied()
        .unwrap_or("?")
}

#[test]
fn tmux_detection_single_call_marker_error_is_returned() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let marker_calls = AtomicUsize::new(0);
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            let n = marker_calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Err(TmuxError::ServerNotRunning {
                    server: "srv".to_owned(),
                });
            }
            return Ok(String::new());
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner);
    assert!(result.is_err(), "first marker error must propagate");
    match result.unwrap_err() {
        TmuxError::ServerNotRunning { .. } => {}
        other => panic!("expected ServerNotRunning, got {other:?}"),
    }
    assert_eq!(
        marker_calls.load(Ordering::SeqCst),
        1,
        "marker probe must execute exactly once"
    );
}

#[test]
fn tmux_detection_single_call_binding_error_is_returned() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let binding_calls = AtomicUsize::new(0);
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"show-options") {
            return Ok(String::new());
        }
        if args.contains(&"list-keys") {
            let n = binding_calls.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Err(TmuxError::SocketNotFound {
                    path: "/tmp/srv.sock".to_owned(),
                });
            }
            return Ok(String::new());
        }
        Ok(String::new())
    };
    let result = detect_binding_state("srv", &runner);
    assert!(result.is_err(), "first binding error must propagate");
    match result.unwrap_err() {
        TmuxError::SocketNotFound { .. } => {}
        other => panic!("expected SocketNotFound, got {other:?}"),
    }
    assert_eq!(
        binding_calls.load(Ordering::SeqCst),
        1,
        "binding probe must execute exactly once"
    );
}

#[test]
fn tmux_missing_classification_rejects_generic_failures() {
    let cases: Vec<(&str, &str)> = vec![
        ("empty message", ""),
        ("no current", "no current session"),
        ("unrelated error", "something went wrong"),
    ];
    for (label, msg) in cases {
        let message = msg.to_owned();
        let runner = move |args: &[&str]| -> Result<String, TmuxError> {
            if args.contains(&"show-options") {
                return Err(TmuxError::CommandFailed {
                    message: message.clone(),
                });
            }
            Ok(String::new())
        };
        let result = detect_binding_state("srv", &runner);
        assert!(
            result.is_err(),
            "CommandFailed with '{label}' must propagate as error, not map to Absent"
        );
    }
}

#[test]
fn tmux_missing_classification_accepts_exact_key_missing() {
    let accepted: Vec<&str> = vec![
        "invalid key: y",
        "unknown key: y",
        "  invalid key: y  ",
        "\tinvalid key: y\n",
    ];
    let rejected: Vec<&str> = vec![
        "unrelated failure containing invalid key: y",
        "invalid key: OtherKey",
        "",
        "no current session",
        "no entries",
    ];

    for msg in &accepted {
        let message = msg.to_string();
        let runner = move |args: &[&str]| -> Result<String, TmuxError> {
            if args.contains(&"show-options") {
                return Ok(String::new());
            }
            if args.contains(&"list-keys") {
                let queried = queried_key_from_args(args);
                if queried == "y" {
                    return Err(TmuxError::CommandFailed {
                        message: message.clone(),
                    });
                }
                return Err(TmuxError::CommandFailed {
                    message: format!("invalid key: {queried}"),
                });
            }
            Ok(String::new())
        };
        let result = detect_binding_state("srv", &runner);
        assert!(
            result.is_ok(),
            "'{msg}' must be treated as key-not-found (Absent)"
        );
    }

    for msg in &rejected {
        let message = msg.to_string();
        let runner = move |args: &[&str]| -> Result<String, TmuxError> {
            if args.contains(&"show-options") {
                return Ok(String::new());
            }
            if args.contains(&"list-keys") {
                let queried = queried_key_from_args(args);
                if queried == "y" {
                    return Err(TmuxError::CommandFailed {
                        message: message.clone(),
                    });
                }
                return Err(TmuxError::CommandFailed {
                    message: format!("invalid key: {queried}"),
                });
            }
            Ok(String::new())
        };
        let result = detect_binding_state("srv", &runner);
        assert!(
            result.is_err(),
            "'{msg}' must propagate as error, not map to Absent"
        );
    }
}

// ─── Executor tests ──────────────────────────────────────────────────────

#[test]
fn tmux_policy_execution_operations_execute_in_order() {
    let plan = plan_session_config(
        &RuntimeInteractionPolicy {
            mouse_enabled: true,
            clipboard_enabled: true,
        },
        &both_absent(),
        false,
    );

    let executed = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        executed
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(String::new())
    };

    TmuxCliAdapter::execute_plan_with_runner("srv", "sess", &plan, &runner).unwrap();
    let log = executed.borrow();
    assert_eq!(log.len(), plan.ops.len());
    assert!(log[0].contains(&"set-option".to_string()));
}

#[test]
fn tmux_policy_execution_first_error_propagates() {
    let plan = SessionConfigPlan {
        ops: vec![
            SessionConfigOp::SetSessionOption {
                key: "mode-keys",
                value: "vi",
            },
            SessionConfigOp::SetSessionOption {
                key: "mouse",
                value: "on",
            },
            SessionConfigOp::SetSessionOption {
                key: "set-clipboard",
                value: "external",
            },
        ],
    };

    let call_count = std::cell::Cell::new(0u32);
    let runner = |_args: &[&str]| -> Result<String, TmuxError> {
        let n = call_count.get();
        call_count.set(n + 1);
        if n == 1 {
            return Err(TmuxError::CommandFailed {
                message: "injected failure".to_owned(),
            });
        }
        Ok(String::new())
    };

    let result = TmuxCliAdapter::execute_plan_with_runner("srv", "sess", &plan, &runner);
    assert!(result.is_err());
    assert_eq!(call_count.get(), 2, "must stop after first error");
}

#[test]
fn tmux_policy_execution_no_ops_after_failure() {
    let plan = SessionConfigPlan {
        ops: vec![
            SessionConfigOp::SetSessionOption {
                key: "a",
                value: "1",
            },
            SessionConfigOp::SetSessionOption {
                key: "b",
                value: "2",
            },
            SessionConfigOp::SetSessionOption {
                key: "c",
                value: "3",
            },
            SessionConfigOp::SetSessionOption {
                key: "d",
                value: "4",
            },
        ],
    };

    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        if args.contains(&"b") {
            return Err(TmuxError::CommandFailed {
                message: "fail on b".to_owned(),
            });
        }
        Ok(String::new())
    };

    let result = TmuxCliAdapter::execute_plan_with_runner("srv", "sess", &plan, &runner);
    match result {
        Err(TmuxError::CommandFailed { message }) => {
            assert!(message.contains("fail on b"));
        }
        other => panic!("expected CommandFailed, got: {other:?}"),
    }
}

#[test]
fn tmux_policy_execution_marker_ops_produce_correct_commands() {
    let plan = SessionConfigPlan {
        ops: vec![
            SessionConfigOp::BindCopyModeKey {
                key: "y",
                command: ["send-keys", "-X", "copy-selection-and-cancel"],
            },
            SessionConfigOp::SetWorkhiveMarker { key: "y" },
            SessionConfigOp::ClearWorkhiveMarker {
                key: "MouseDragEnd1Pane",
            },
        ],
    };

    let executed = std::cell::RefCell::new(Vec::<Vec<String>>::new());
    let runner = |args: &[&str]| -> Result<String, TmuxError> {
        executed
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(String::new())
    };

    TmuxCliAdapter::execute_plan_with_runner("srv", "sess", &plan, &runner).unwrap();
    let log = executed.borrow();
    assert_eq!(log.len(), 3);
    assert!(log[1].contains(&"@workhive_managed_y".to_string()));
    assert!(log[1].contains(&"installed".to_string()));
    assert!(log[2].contains(&"@workhive_managed_MouseDragEnd1Pane".to_string()));
    assert!(log[2].contains(&"-gu".to_string()));
}

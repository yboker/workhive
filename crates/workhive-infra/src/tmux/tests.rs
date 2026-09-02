use super::output::*;
use super::*;
use workhive_core::domain::{PaneKind, PaneState};

#[test]
fn parse_sessions_handles_typical_output() {
    let output = "mysession\t3\t5\t1\nother\t1\t1\t0";
    let sessions = parse_sessions(output);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "mysession");
    assert_eq!(sessions[0].windows, 3);
    assert_eq!(sessions[0].panes, 5);
    assert!(sessions[0].attached);
    assert_eq!(sessions[1].panes, 1);
    assert!(!sessions[1].attached);
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

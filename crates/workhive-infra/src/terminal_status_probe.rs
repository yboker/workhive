//! Tmux-backed terminal status probe.
//!
//! This adapter implements [`TerminalStatusProbePort`] (a runtime-neutral
//! `workhive-core` port) on top of the infra-level [`TmuxPort`] verb
//! [`TmuxPort::capture_pane_text`]. It deliberately does **not** call
//! `Command::new("tmux")` directly — every tmux interaction must flow
//! through the typed port boundary so all process-level coupling stays
//! audited inside `TmuxCliAdapter`. See the tmux adapter boundary rule.

use workhive_core::domain::PaneId;
use workhive_core::ports::{
    TerminalStatusCapture, TerminalStatusProbeError, TerminalStatusProbePort,
    TerminalStatusProbeRequest,
};

use crate::tmux::{CapturePaneRequest, TmuxCliAdapter, TmuxPort};
use crate::tmux_errors::TmuxError;

/// How many lines of recent buffer to capture per probe call.
///
/// Negative offset matches tmux `capture-pane -S` semantics: include the
/// last 20 lines on top of the current screen. The conservative working
/// classifier in [`workhive_core::usecases::terminal_status`] only inspects
/// the most recent visible lines, so a wider window would just waste IO.
const CAPTURE_START_LINE: i32 = -20;

/// Captures visible pane text by delegating to a [`TmuxPort`].
///
/// The probe stays generic over the port so production builds reuse the
/// same `TmuxCliAdapter` everything else does, while tests can inject a
/// fake to assert that capture failures degrade quietly (no panic, no
/// silent default coercion). The default constructor builds a fresh
/// [`TmuxCliAdapter`] which is a zero-sized type, matching the previous
/// public surface used by `CliComposition`.
pub struct TmuxTerminalStatusProbe<T: TmuxPort = TmuxCliAdapter> {
    tmux: T,
}

impl TmuxTerminalStatusProbe<TmuxCliAdapter> {
    /// Build a probe that talks to the real tmux CLI through
    /// [`TmuxCliAdapter`]. This is the production wiring used by
    /// `workhive-cli`'s composition root.
    pub fn new() -> Self {
        Self {
            tmux: TmuxCliAdapter::new(),
        }
    }
}

impl<T: TmuxPort> TmuxTerminalStatusProbe<T> {
    /// Build a probe that delegates capture to the given [`TmuxPort`].
    ///
    /// Used in tests to verify that the probe routes every capture through
    /// the typed port and degrades correctly when the adapter reports
    /// failures.
    pub fn with_tmux(tmux: T) -> Self {
        Self { tmux }
    }

    fn capture_one(
        &self,
        request: &TerminalStatusProbeRequest,
    ) -> Result<Option<TerminalStatusCapture>, TerminalStatusProbeError> {
        let capture_req = CapturePaneRequest {
            server_name: request.group.clone(),
            target_pane: PaneId::new(request.target_unit.as_str()),
            start_line: CAPTURE_START_LINE,
        };
        match self.tmux.capture_pane_text(capture_req) {
            Ok(text) => {
                if text.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(TerminalStatusCapture {
                        unit_id: request.target_unit.clone(),
                        text,
                    }))
                }
            }
            Err(err) => Err(map_tmux_error(request, err)),
        }
    }
}

/// Map an infra-level [`TmuxError`] back to a runtime-neutral
/// [`TerminalStatusProbeError`].
///
/// Errors that mean "the whole probe surface is unavailable" become
/// [`TerminalStatusProbeError::Unavailable`] so the use case can short
/// circuit the entire batch. Per-pane failures (missing pane, command
/// failure) become [`TerminalStatusProbeError::CaptureFailed`] so the
/// observation ladder can fall through to the next source instead of
/// panicking. This is what the error-handling policy calls "no silent fallback":
/// every failure is surfaced as a typed error.
fn map_tmux_error(
    request: &TerminalStatusProbeRequest,
    err: TmuxError,
) -> TerminalStatusProbeError {
    match err {
        TmuxError::TmuxNotFound => TerminalStatusProbeError::Unavailable {
            reason: "tmux executable not found".to_owned(),
        },
        TmuxError::ServerNotRunning { server } => TerminalStatusProbeError::Unavailable {
            reason: format!("tmux server not running: {server}"),
        },
        TmuxError::SocketNotFound { path } => TerminalStatusProbeError::Unavailable {
            reason: format!("tmux socket not found: {path}"),
        },
        TmuxError::PermissionDenied { detail } => TerminalStatusProbeError::Unavailable {
            reason: format!("tmux permission denied: {detail}"),
        },
        TmuxError::PaneNotFound { id } => TerminalStatusProbeError::CaptureFailed {
            unit_id: request.target_unit.as_str().to_owned(),
            detail: format!("pane not found: {id}"),
        },
        TmuxError::CommandFailed { message } => TerminalStatusProbeError::CaptureFailed {
            unit_id: request.target_unit.as_str().to_owned(),
            detail: message,
        },
        // Any remaining variant is a true tmux runtime fault: surface as
        // CaptureFailed so the source ladder can demote terminal-status
        // for this pane without aborting unrelated panes in the batch.
        other => TerminalStatusProbeError::CaptureFailed {
            unit_id: request.target_unit.as_str().to_owned(),
            detail: other.to_string(),
        },
    }
}

impl Default for TmuxTerminalStatusProbe<TmuxCliAdapter> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: TmuxPort> TerminalStatusProbePort for TmuxTerminalStatusProbe<T> {
    fn capture_many(
        &self,
        requests: &[TerminalStatusProbeRequest],
    ) -> Result<Vec<Option<TerminalStatusCapture>>, TerminalStatusProbeError> {
        requests.iter().map(|req| self.capture_one(req)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use workhive_core::domain::UnitId;

    use crate::tmux::{
        AttachRequest, CreateSessionRequest, PaneInfo, PaneTopologyInfo, SendKeysRequest,
        SessionInfo, SplitPaneRequest, WindowInfo,
    };

    /// Recording fake `TmuxPort` that lets tests assert which capture
    /// requests reached the port (and on what server / pane id) and lets
    /// them stage either a success payload or a failure.
    struct RecordingTmux {
        // Use RefCell so we can record from a `&self` trait method while
        // keeping the fake `Send`/`Sync`-compatible profile minimal — these
        // tests are single-threaded.
        captures: RefCell<Vec<CapturePaneRequest>>,
        responses: RefCell<Vec<Result<String, TmuxError>>>,
    }

    impl RecordingTmux {
        fn new(responses: Vec<Result<String, TmuxError>>) -> Self {
            Self {
                captures: RefCell::new(Vec::new()),
                responses: RefCell::new(responses),
            }
        }

        fn captured(&self) -> Vec<CapturePaneRequest> {
            self.captures.borrow().clone()
        }
    }

    impl TmuxPort for RecordingTmux {
        fn has_session(&self, _server: &str, _session: &str) -> Result<bool, TmuxError> {
            Ok(false)
        }
        fn create_session(&self, _req: CreateSessionRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn configure_session(
            &self,
            _server: &str,
            _session: &str,
            _policy: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn attach(&self, _req: AttachRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn switch_client(&self, _server: &str, _target: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn detach_client(&self, _server: &str, _session: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn rename_session(
            &self,
            _server: &str,
            _session: &str,
            _new_name: &str,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn kill_session(&self, _server: &str, _session: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_sessions(&self, _server: &str) -> Result<Vec<SessionInfo>, TmuxError> {
            Ok(Vec::new())
        }
        fn kill_server(&self, _server: &str) -> Result<(), TmuxError> {
            Ok(())
        }
        fn split_pane(&self, _req: SplitPaneRequest) -> Result<PaneInfo, TmuxError> {
            unimplemented!("not used in terminal-status probe tests")
        }
        fn close_pane(&self, _server: &str, _pane: &PaneId) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_panes(&self, _server: &str, _session: &str) -> Result<Vec<PaneInfo>, TmuxError> {
            Ok(Vec::new())
        }
        fn current_pane(&self, _server: &str, _session: &str) -> Result<PaneInfo, TmuxError> {
            unimplemented!("not used in terminal-status probe tests")
        }
        fn send_keys(&self, _req: SendKeysRequest) -> Result<(), TmuxError> {
            Ok(())
        }
        fn set_pane_title(
            &self,
            _server: &str,
            _pane: &PaneId,
            _title: &str,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn list_windows(
            &self,
            _server: &str,
            _session: &str,
        ) -> Result<Vec<WindowInfo>, TmuxError> {
            Ok(Vec::new())
        }
        fn list_panes_topology(
            &self,
            _server: &str,
            _session: &str,
        ) -> Result<Vec<PaneTopologyInfo>, TmuxError> {
            Ok(Vec::new())
        }
        fn create_window(
            &self,
            _server: &str,
            _session: &str,
            _idx: u32,
            _name: Option<&str>,
            _cwd: &camino::Utf8Path,
        ) -> Result<PaneTopologyInfo, TmuxError> {
            unimplemented!("not used in terminal-status probe tests")
        }
        fn apply_layout(
            &self,
            _server: &str,
            _session: &str,
            _idx: u32,
            _layout: &str,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn select_window(&self, _server: &str, _session: &str, _idx: u32) -> Result<(), TmuxError> {
            Ok(())
        }
        fn select_pane(&self, _server: &str, _pane: &PaneId) -> Result<(), TmuxError> {
            Ok(())
        }
        fn respawn_pane(
            &self,
            _server: &str,
            _pane: &PaneId,
            _cwd: &camino::Utf8Path,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn respawn_pane_with_command(
            &self,
            _server: &str,
            _pane: &PaneId,
            _cwd: &camino::Utf8Path,
            _command: &str,
        ) -> Result<(), TmuxError> {
            Ok(())
        }
        fn set_hook(
            &self,
            _server: &str,
            _session: &str,
            _hook: &str,
            _command: Option<&str>,
        ) -> Result<(), TmuxError> {
            Ok(())
        }

        fn capture_pane_text(&self, request: CapturePaneRequest) -> Result<String, TmuxError> {
            self.captures.borrow_mut().push(request);
            // Pop responses front-to-back so the order matches request order.
            let mut responses = self.responses.borrow_mut();
            assert!(
                !responses.is_empty(),
                "RecordingTmux ran out of staged responses"
            );
            responses.remove(0)
        }
    }

    fn req(group: &str, unit: &str) -> TerminalStatusProbeRequest {
        TerminalStatusProbeRequest {
            group: group.to_owned(),
            target_unit: UnitId::new(unit),
        }
    }

    #[test]
    fn capture_routes_through_tmux_port_and_returns_text() {
        let tmux = RecordingTmux::new(vec![Ok("waiting for approval\n".to_owned())]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let out = probe
            .capture_many(&[req("whsrv-test-proj", "%42")])
            .expect("probe must succeed");

        // Returned exactly one capture with the recorded text.
        assert_eq!(out.len(), 1);
        let capture = out[0].as_ref().expect("non-empty buffer maps to Some");
        assert_eq!(capture.unit_id.as_str(), "%42");
        assert_eq!(capture.text, "waiting for approval\n");

        // The probe routed exactly one CapturePaneRequest through the port,
        // with the correct server, pane id, and recent-window offset.
        let recorded = probe.tmux.captured();
        assert_eq!(recorded.len(), 1, "expected exactly one tmux call");
        assert_eq!(recorded[0].server_name, "whsrv-test-proj");
        assert_eq!(recorded[0].target_pane.as_str(), "%42");
        assert_eq!(
            recorded[0].start_line, CAPTURE_START_LINE,
            "must request bounded recent window, not full scrollback"
        );
    }

    #[test]
    fn empty_buffer_degrades_to_none_without_silent_default() {
        let tmux = RecordingTmux::new(vec![Ok("   \n\n".to_owned())]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let out = probe
            .capture_many(&[req("whsrv-test-proj", "%7")])
            .expect("whitespace-only output is not an error");

        // Whitespace-only buffer collapses to None so the source ladder
        // demotes terminal-status for this pane (no Some(text)
        // misclassified as a working signal).
        assert_eq!(out.len(), 1);
        assert!(
            out[0].is_none(),
            "empty buffer must be reported as None, not as empty TerminalStatusCapture"
        );
    }

    #[test]
    fn pane_capture_failure_surfaces_as_capture_failed() {
        // PaneNotFound is the realistic per-pane failure: a pane was
        // killed between snapshot and capture. We must surface it as a
        // typed error so the use case can fall through, not panic and
        // not silently coerce to Ok(None).
        let tmux = RecordingTmux::new(vec![Err(TmuxError::PaneNotFound {
            id: "%99".to_owned(),
        })]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let err = probe
            .capture_many(&[req("whsrv-test-proj", "%99")])
            .expect_err("PaneNotFound must surface as Err, not Ok(None)");

        match err {
            TerminalStatusProbeError::CaptureFailed { unit_id, detail } => {
                assert_eq!(unit_id, "%99");
                assert!(
                    detail.contains("pane not found"),
                    "detail should preserve tmux failure context, got: {detail}"
                );
            }
            TerminalStatusProbeError::Unavailable { reason } => {
                panic!("PaneNotFound must NOT be classified Unavailable, got: {reason}")
            }
        }
    }

    #[test]
    fn tmux_not_found_surfaces_as_unavailable_not_panic() {
        let tmux = RecordingTmux::new(vec![Err(TmuxError::TmuxNotFound)]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let err = probe
            .capture_many(&[req("whsrv-test-proj", "%1")])
            .expect_err("missing tmux executable must surface as Err");

        match err {
            TerminalStatusProbeError::Unavailable { reason } => {
                assert!(
                    reason.contains("tmux"),
                    "reason should explain why probe is unavailable, got: {reason}"
                );
            }
            TerminalStatusProbeError::CaptureFailed { detail, .. } => {
                panic!("TmuxNotFound must classify as Unavailable, got CaptureFailed: {detail}")
            }
        }
    }

    #[test]
    fn server_not_running_surfaces_as_unavailable() {
        let tmux = RecordingTmux::new(vec![Err(TmuxError::ServerNotRunning {
            server: "whsrv-test-proj".to_owned(),
        })]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let err = probe
            .capture_many(&[req("whsrv-test-proj", "%1")])
            .expect_err("missing tmux server must surface as Err");

        match err {
            TerminalStatusProbeError::Unavailable { reason } => {
                assert!(
                    reason.contains("server not running"),
                    "reason should preserve tmux context, got: {reason}"
                );
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn batch_request_propagates_first_failure_without_panic() {
        // Mix of success then failure: capture_many uses .collect() over
        // Result, so the batch returns the first Err. The point of this
        // test is to prove the probe does NOT swallow the second error
        // via unwrap_or_default or similar — it propagates a typed error.
        let tmux = RecordingTmux::new(vec![
            Ok("running tests".to_owned()),
            Err(TmuxError::CommandFailed {
                message: "no such pane".to_owned(),
            }),
        ]);
        let probe = TmuxTerminalStatusProbe::with_tmux(tmux);

        let err = probe
            .capture_many(&[req("whsrv", "%1"), req("whsrv", "%2")])
            .expect_err("second capture failure must propagate");

        match err {
            TerminalStatusProbeError::CaptureFailed { unit_id, detail } => {
                assert_eq!(unit_id, "%2");
                assert!(detail.contains("no such pane"));
            }
            other => panic!("expected CaptureFailed for second pane, got {other:?}"),
        }
    }
}

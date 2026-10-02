//! Internal `control-plane serve` handler.
//!
//! This is the in-process entry point for the pane runtime control plane
//! supervisor. The caller (`workspace open`, the `pane send` self-heal
//! retry path, or a hidden invocation from a future tooling smoke-test)
//! is expected to have already determined that no supervisor is bound
//! to the socket — this handler unconditionally binds it and serves
//! until the listener exits.
//!
//! Lifecycle:
//!
//!   * The supervisor binds at the canonical socket path returned by
//!     [`workhive_infra::control_plane::control_plane_socket_path`].
//!   * If a stale socket is left over from a previous crash, it is
//!     removed before bind (`remove_stale_socket`).
//!   * When `parent_pid > 0`, a background watchdog polls the parent's
//!     existence every second and signals the listener to stop once the
//!     parent disappears. This mode is reserved for non-production
//!     tooling (e.g. ad-hoc debugging where the user wants the
//!     supervisor to die with the launching shell).
//!   * When `parent_pid == 0` (the production path used by both
//!     `workspace open` and `pane send` self-heal), a session-liveness
//!     probe runs instead: the supervisor periodically calls
//!     `runtime.has_workspace(server, workspace)` and stops once the
//!     tmux session is gone. A startup grace window prevents the probe
//!     from racing `workspace open`'s session-create step. This decouples
//!     the supervisor's lifetime from the short-lived `workhive` CLI
//!     process.
//!
//! Both modes also clean up the bound socket file on exit so a crashed
//! or session-aged supervisor doesn't leave a stale UDS that would later
//! defeat `LocalPaneControlPlaneClient::ping()`-based liveness checks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use camino::Utf8Path;
use workhive_infra::control_plane::{
    ControlPlaneSupervisor, control_plane_socket_path, prepare_socket_parent, remove_stale_socket,
};
use workhive_infra::pane_launch_identity::hydrate_pane_registry_from_identity_store;

use crate::context::CommandContext;
use crate::error::CliError;

/// Initial delay before the session-liveness probe starts polling tmux.
///
/// `workspace open` spawns the supervisor before the tmux session is
/// guaranteed to be visible to a fresh runtime listing (especially under
/// `--no-attach`, where the session is created lazily). The grace window
/// keeps the supervisor alive across that race so the probe doesn't kill
/// it the instant it starts. 5s is empirical: comfortably longer than a
/// normal open's session-create step and shorter than any human-scale
/// follow-up `pane send`.
const SESSION_PROBE_STARTUP_GRACE: Duration = Duration::from_secs(5);

/// Polling interval for the session-liveness probe. One second matches
/// the parent watchdog cadence so resource cost stays comparable
/// regardless of which lifecycle the caller picks.
const SESSION_PROBE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionLivenessObservation {
    WorkspacePresent,
    WorkspaceMissing,
    RuntimeError(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionLivenessDecision {
    Continue { session_seen: bool },
    Stop { reason: String },
}

fn decide_session_liveness(
    observation: SessionLivenessObservation,
    session_seen: bool,
    within_startup_grace: bool,
) -> SessionLivenessDecision {
    match observation {
        SessionLivenessObservation::WorkspacePresent => {
            SessionLivenessDecision::Continue { session_seen: true }
        }
        SessionLivenessObservation::WorkspaceMissing if !session_seen && within_startup_grace => {
            SessionLivenessDecision::Continue {
                session_seen: false,
            }
        }
        SessionLivenessObservation::WorkspaceMissing => SessionLivenessDecision::Stop {
            reason: "workspace session no longer exists".to_owned(),
        },
        SessionLivenessObservation::RuntimeError(reason) if within_startup_grace => {
            tracing::debug!(
                error = %reason,
                "control-plane: session liveness probe encountered a transient runtime error during startup grace"
            );
            SessionLivenessDecision::Continue { session_seen }
        }
        SessionLivenessObservation::RuntimeError(reason) => SessionLivenessDecision::Stop {
            reason: format!("runtime liveness probe failed after startup grace: {reason}"),
        },
    }
}

/// Run the control plane supervisor until the parent dies. This blocks
/// the calling thread by design — the caller is a detached child process
/// spawned by
/// [`crate::composition::CliComposition::spawn_control_plane_supervisor`].
pub(crate) fn cmd_internal_control_plane_serve(
    project_root: &str,
    server: &str,
    workspace: &str,
    parent_pid: i32,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let project_root_path = Utf8Path::new(project_root).to_path_buf();
    let socket = control_plane_socket_path(&project_root_path, server);
    prepare_socket_parent(&socket).map_err(|e| {
        CliError::Runtime(format!(
            "control-plane: prepare socket parent {socket}: {e}"
        ))
    })?;
    remove_stale_socket(&socket).map_err(|e| {
        CliError::Runtime(format!("control-plane: remove stale socket {socket}: {e}"))
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    if parent_pid > 0 {
        // Non-production lifecycle: bind to a specific parent process.
        spawn_parent_watchdog(parent_pid, Arc::clone(&stop), socket.clone());
    } else {
        // Production lifecycle: bind to the workspace/tmux session.
        spawn_session_liveness_probe(
            project_root_path.clone(),
            server.to_owned(),
            workspace.to_owned(),
            Arc::clone(&stop),
            socket.clone(),
        );
    }

    let runtime = ctx
        .composition
        .runtime_for_workspace(&project_root_path, server, workspace)?;
    let registry = ctx.composition.pane_registry();

    // Hydrate the brand-new in-memory registry from the durable
    // identity store. This is the cross-process bridge: the supervisor is a
    // detached child process that cannot share the parent CLI's
    // in-memory registry, so we replay launch identities from
    // `<project_root>/.workhive/runtime/pane-identity/<server>.json`
    // before serving any request.
    let identity_store = ctx
        .composition
        .pane_launch_identity_store(&project_root_path);
    match hydrate_pane_registry_from_identity_store(&identity_store, &registry, server) {
        Ok(report) => {
            tracing::info!(
                server,
                workspace,
                entries_loaded = report.entries_loaded,
                entries_applied = report.entries_applied,
                "control-plane: durable launch-identity hydration complete"
            );
        }
        Err(error) => {
            // Fail-open: an unhydrated supervisor still serves
            // pane sends; only `display_title` will be `None`.
            // We do not refuse to bind the socket because the user
            // may have legitimately removed the identity file or
            // never recorded any tool launches.
            tracing::warn!(
                server,
                workspace,
                error = %error,
                "control-plane: durable launch-identity hydration failed; \
                 display_title will be unavailable until the next workspace open"
            );
        }
    }

    let supervisor = ControlPlaneSupervisor::with_registry(
        runtime.as_ref(),
        &registry,
        server.to_owned(),
        workspace.to_owned(),
    );
    tracing::info!(
        socket = %socket,
        server,
        workspace,
        parent_pid,
        lifecycle = if parent_pid > 0 { "parent_watchdog" } else { "session" },
        "control-plane: supervisor binding socket"
    );
    let stop_for_serve = Arc::clone(&stop);
    let serve_result = supervisor
        .serve_blocking(&socket, move || stop_for_serve.load(Ordering::SeqCst))
        .map_err(|e| CliError::Runtime(format!("control-plane: serve_blocking failed: {e}")));
    // Best-effort: remove the bound socket on exit so a future
    // `workspace open` / `pane send` does not see a stale UDS file. We
    // intentionally swallow the error here — if the listener already
    // drained the file or the user removed it manually, that is fine;
    // the next supervisor will call `remove_stale_socket` anyway.
    if let Err(error) = remove_stale_socket(&socket) {
        tracing::debug!(
            socket = %socket,
            error = %error,
            "control-plane: best-effort socket cleanup on exit failed"
        );
    }
    serve_result?;
    tracing::info!(
        server,
        workspace,
        "control-plane: supervisor exited cleanly"
    );
    Ok(())
}

/// Poll the parent process every second; once the parent disappears,
/// flip the stop flag and connect to the supervisor's own socket so
/// `serve_blocking` unblocks from `accept`. The existence probe lives in
/// `workhive_infra::host_probe` so this module never constructs external
/// system commands directly; the cost is one fork+exec per second per
/// supervisor, which is well below noise on modern systems.
fn spawn_parent_watchdog(parent_pid: i32, stop: Arc<AtomicBool>, socket: camino::Utf8PathBuf) {
    std::thread::spawn(move || {
        loop {
            let alive = workhive_infra::host_probe::process_alive(parent_pid);
            if !alive {
                tracing::info!(
                    parent_pid,
                    "control-plane: parent process gone, stopping supervisor"
                );
                stop.store(true, Ordering::SeqCst);
                let _ = std::os::unix::net::UnixStream::connect(socket.as_std_path());
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

/// Production lifecycle probe: poll `has_workspace(server, workspace)`
/// against a fresh `PaneRuntimePort` and stop the supervisor once the
/// tmux session disappears. The fresh runtime is constructed inside the
/// thread so we don't borrow `ctx` across thread boundaries.
///
/// The startup grace window is deliberately wider than a normal open's
/// session-create step so the probe never kills a freshly spawned
/// supervisor before the caller has finished setting up the tmux
/// session.
fn spawn_session_liveness_probe(
    project_root: camino::Utf8PathBuf,
    server: String,
    workspace: String,
    stop: Arc<AtomicBool>,
    socket: camino::Utf8PathBuf,
) {
    std::thread::spawn(move || {
        let started_at = Instant::now();
        let mut session_seen = false;
        // Construct the runtime once — its lifetime is the thread's, and
        // each call goes through `tmux list-sessions` on the configured
        // server so dynamic state changes are picked up.
        let composition = crate::composition::CliComposition::new();
        let runtime = composition.runtime_for_workspace(&project_root, &server, &workspace);
        let runtime = match runtime {
            Ok(runtime) => runtime,
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    server = %server,
                    workspace = %workspace,
                    "control-plane: runtime backend unavailable for liveness probe"
                );
                return;
            }
        };
        loop {
            std::thread::sleep(SESSION_PROBE_INTERVAL);
            if stop.load(Ordering::SeqCst) {
                return;
            }
            let observation = match runtime.has_workspace(&server, &workspace) {
                Ok(true) => SessionLivenessObservation::WorkspacePresent,
                Ok(false) => SessionLivenessObservation::WorkspaceMissing,
                Err(error) => SessionLivenessObservation::RuntimeError(error.to_string()),
            };
            let within_startup_grace = started_at.elapsed() < SESSION_PROBE_STARTUP_GRACE;
            match decide_session_liveness(observation, session_seen, within_startup_grace) {
                SessionLivenessDecision::Continue {
                    session_seen: next_seen,
                } => {
                    session_seen = next_seen;
                }
                SessionLivenessDecision::Stop { reason } => {
                    tracing::info!(
                        server = %server,
                        workspace = %workspace,
                        reason = %reason,
                        "control-plane: stopping supervisor after session liveness probe"
                    );
                    stop.store(true, Ordering::SeqCst);
                    // Unblock `accept` in `serve_blocking` by connecting to our
                    // own socket; the connection is closed immediately since
                    // the supervisor breaks out of the loop on the next
                    // `should_stop` check.
                    let _ = std::os::unix::net::UnixStream::connect(socket.as_std_path());
                    return;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_liveness_keeps_transient_error_alive_during_startup_grace() {
        let decision = decide_session_liveness(
            SessionLivenessObservation::RuntimeError("tmux warming up".to_owned()),
            false,
            true,
        );

        assert_eq!(
            decision,
            SessionLivenessDecision::Continue {
                session_seen: false
            }
        );
    }

    #[test]
    fn session_liveness_stops_missing_session_after_grace() {
        let decision =
            decide_session_liveness(SessionLivenessObservation::WorkspaceMissing, false, false);

        assert!(matches!(decision, SessionLivenessDecision::Stop { .. }));
    }

    #[test]
    fn session_liveness_marks_success_without_logging_branch() {
        let decision =
            decide_session_liveness(SessionLivenessObservation::WorkspacePresent, false, false);

        assert_eq!(
            decision,
            SessionLivenessDecision::Continue { session_seen: true }
        );
    }

    #[test]
    fn session_liveness_stops_runtime_error_after_grace() {
        let decision = decide_session_liveness(
            SessionLivenessObservation::RuntimeError("tmux server not running".to_owned()),
            true,
            false,
        );

        match decision {
            SessionLivenessDecision::Stop { reason } => {
                assert!(reason.contains("after startup grace"));
                assert!(reason.contains("tmux server not running"));
            }
            other => panic!("expected Stop after grace, got {other:?}"),
        }
    }
}

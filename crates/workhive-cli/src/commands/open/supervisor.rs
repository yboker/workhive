#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Result of probing the runtime control plane supervisor. Each variant
/// drives a single, distinct branch in [`ensure_control_plane_supervisor`]
/// so the decision tree is testable without mocking the production
/// socket / spawn path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SupervisorProbeOutcome {
    /// Live supervisor responded to `ping()`. Reuse it as-is.
    Live,
    /// `SupervisorUnavailable` (no listener / connect refused). The
    /// socket file may still exist on disk; the caller must remove the
    /// stale socket and respawn.
    StaleSocket { detail: String },
    /// Transport/protocol/backend response that proves the listener is
    /// reachable but the conversation is broken. The caller surfaces a
    /// degraded warning in the open summary instead of silently
    /// respawning, so the user can investigate. Returning `Ok` here keeps
    /// `workspace open` non-fatal while still flagging the problem.
    Degraded { detail: String },
}

const CONTROL_PLANE_STARTUP_DEADLINE: std::time::Duration = std::time::Duration::from_millis(750);
const CONTROL_PLANE_STARTUP_POLL: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
enum SupervisorStartupOutcome {
    Ready,
    Degraded {
        detail: String,
    },
    TimedOut {
        deadline_ms: u64,
        last_detail: String,
    },
}

/// Probe the supervisor at `socket_path` with a typed `ping()` and
/// classify the outcome. Returning a typed value keeps the
/// `ensure_control_plane_supervisor` logic readable and unit-testable.
fn probe_control_plane_supervisor(socket_path: &camino::Utf8Path) -> SupervisorProbeOutcome {
    use workhive_core::control_plane::{PaneControlPlaneClient, PaneControlPlaneError};
    use workhive_infra::control_plane::LocalPaneControlPlaneClient;

    if !socket_path.as_std_path().exists() {
        // No socket file at all means there is no supervisor on disk.
        // Treat this as "stale" so the caller spawns a fresh one; the
        // `remove_stale_socket` step is a no-op when the file is absent.
        return SupervisorProbeOutcome::StaleSocket {
            detail: "no socket file present".to_owned(),
        };
    }

    let client = LocalPaneControlPlaneClient::new(socket_path.to_path_buf());
    match client.ping() {
        Ok(_server_version) => SupervisorProbeOutcome::Live,
        Err(PaneControlPlaneError::SupervisorUnavailable { detail }) => {
            SupervisorProbeOutcome::StaleSocket { detail }
        }
        Err(PaneControlPlaneError::ProtocolVersionMismatch { client, server }) => {
            SupervisorProbeOutcome::Degraded {
                detail: format!("protocol version mismatch: client={client}, server={server}"),
            }
        }
        Err(other) => SupervisorProbeOutcome::Degraded {
            detail: other.to_string(),
        },
    }
}

fn wait_for_control_plane_startup<P, S>(
    mut probe: P,
    mut sleep: S,
    deadline: std::time::Duration,
    poll_interval: std::time::Duration,
    started_at: std::time::Instant,
) -> SupervisorStartupOutcome
where
    P: FnMut() -> SupervisorProbeOutcome,
    S: FnMut(std::time::Duration),
{
    loop {
        match probe() {
            SupervisorProbeOutcome::Live => return SupervisorStartupOutcome::Ready,
            SupervisorProbeOutcome::Degraded { detail } => {
                return SupervisorStartupOutcome::Degraded { detail };
            }
            SupervisorProbeOutcome::StaleSocket { detail } => {
                if started_at.elapsed() >= deadline {
                    return SupervisorStartupOutcome::TimedOut {
                        deadline_ms: u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
                        last_detail: detail,
                    };
                }
            }
        }

        sleep(poll_interval);
    }
}

/// Production lifecycle helper: ensure a live runtime control plane
/// supervisor is responding on the canonical socket for `(server,
/// session_name)`. Returns the optional degraded reason that
/// [`print_open_summary`] surfaces — `None` means the supervisor is
/// healthy, `Some(reason)` means `pane send` will need to self-heal or
/// the user must investigate.
///
/// Decision tree:
///
/// 1. `ping()` succeeds → reuse the supervisor.
/// 2. `ping()` returns `SupervisorUnavailable` → remove the stale
///    socket file and spawn a new supervisor.
/// 3. `ping()` returns protocol/transport/backend error → surface a
///    typed degraded reason and **do not** silently skip spawn.
///
/// Spawn failures are also captured as degraded reasons so the user is
/// not surprised by a later `pane send` error; the `pane send` self-heal
/// path will still attempt one more spawn at delivery time.
pub(crate) fn ensure_control_plane_supervisor(
    ctx: &CommandContext,
    project_root: &camino::Utf8Path,
    server: &str,
    workspace: &str,
) -> Option<String> {
    let socket_path =
        workhive_infra::control_plane::control_plane_socket_path(project_root, server);

    let outcome = probe_control_plane_supervisor(&socket_path);

    match outcome {
        SupervisorProbeOutcome::Live => {
            tracing::debug!(
                socket = %socket_path,
                "control-plane: live supervisor confirmed via ping; skipping spawn"
            );
            None
        }
        SupervisorProbeOutcome::StaleSocket { detail } => {
            tracing::info!(
                socket = %socket_path,
                detail = %detail,
                "control-plane: stale or missing socket; respawning supervisor"
            );
            // A stale socket file MUST be removed before bind, otherwise
            // the new supervisor's `UnixListener::bind` would fail with
            // `EADDRINUSE`. The infra helper is idempotent against ENOENT.
            if let Err(error) = workhive_infra::control_plane::remove_stale_socket(&socket_path) {
                tracing::warn!(
                    socket = %socket_path,
                    error = %error,
                    "control-plane: stale socket removal failed; spawn may fail to bind"
                );
                return Some(format!("stale socket cleanup failed: {error}"));
            }
            match ctx
                .composition
                .spawn_control_plane_supervisor(project_root, server, workspace)
            {
                Ok(child_pid) => {
                    tracing::info!(
                        child_pid,
                        server,
                        workspace,
                        "control-plane: spawned supervisor subprocess"
                    );
                    match wait_for_control_plane_startup(
                        || probe_control_plane_supervisor(&socket_path),
                        std::thread::sleep,
                        CONTROL_PLANE_STARTUP_DEADLINE,
                        CONTROL_PLANE_STARTUP_POLL,
                        std::time::Instant::now(),
                    ) {
                        SupervisorStartupOutcome::Ready => {
                            tracing::debug!(
                                child_pid,
                                server,
                                workspace,
                                "control-plane: spawned supervisor passed bounded startup ping"
                            );
                            None
                        }
                        SupervisorStartupOutcome::Degraded { detail } => {
                            tracing::warn!(
                                child_pid,
                                server,
                                workspace,
                                detail = %detail,
                                "control-plane: spawned supervisor returned degraded startup ping"
                            );
                            Some(detail)
                        }
                        SupervisorStartupOutcome::TimedOut {
                            deadline_ms,
                            last_detail,
                        } => {
                            let reason = format!(
                                "control-plane startup did not respond within {deadline_ms}ms; last probe: {last_detail}"
                            );
                            tracing::warn!(
                                child_pid,
                                server,
                                workspace,
                                reason = %reason,
                                "control-plane: spawned supervisor did not pass bounded startup ping"
                            );
                            Some(reason)
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        server,
                        workspace,
                        "control-plane: spawn failed; pane send will return localized error"
                    );
                    Some(error.to_string())
                }
            }
        }
        SupervisorProbeOutcome::Degraded { detail } => {
            tracing::warn!(
                socket = %socket_path,
                detail = %detail,
                "control-plane: supervisor responded but conversation is broken; \
                 surfacing degraded warning in open summary"
            );
            Some(detail)
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    #[test]
    fn supervisor_probe_returns_stale_socket_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let socket =
            camino::Utf8PathBuf::from_path_buf(tmp.path().join("no-such.sock")).expect("utf8 path");
        let outcome = probe_control_plane_supervisor(&socket);
        match outcome {
            SupervisorProbeOutcome::StaleSocket { .. } => {}
            other => panic!(
                "missing socket file MUST classify as StaleSocket so open respawns; got={other:?}"
            ),
        }
    }

    #[test]
    fn supervisor_probe_returns_stale_socket_for_dead_socket_file() {
        // A regular file at the socket path is not a live UDS; ping
        // surfaces a connect refusal which the probe must classify as
        // `StaleSocket` so the helper removes the file and respawns.
        // This is exactly the failure mode the previous
        // `socket_path.exists()` branch used to mask.
        let tmp = tempfile::tempdir().expect("tmpdir");
        let socket_path = tmp.path().join("dead.sock");
        std::fs::write(&socket_path, b"not a socket").expect("write stub file");
        let socket = camino::Utf8PathBuf::from_path_buf(socket_path.clone()).expect("utf8 path");
        let outcome = probe_control_plane_supervisor(&socket);
        match outcome {
            SupervisorProbeOutcome::StaleSocket { .. } => {}
            other => panic!(
                "dead UDS file MUST classify as StaleSocket; \
                 raw `socket_path.exists()` would have returned true here. got={other:?}"
            ),
        }
    }

    #[test]
    fn supervisor_probe_returns_live_for_real_listener() {
        // Bind a real UDS so `connect` succeeds. The probe sends a
        // ping; the test listener does not implement the wire protocol
        // so the response read fails — we classify that as `Degraded`,
        // because the listener IS reachable. This pins the
        // protocol/transport branch of the decision tree (the path
        // that MUST surface a degraded summary instead of silently
        // respawning).
        use std::os::unix::net::UnixListener;
        let tmp = tempfile::tempdir().expect("tmpdir");
        let socket_path = tmp.path().join("live.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind unix listener");
        // Drain a single connection in the background so `ping`'s
        // connect call succeeds; we close immediately, which the probe
        // sees as a transport error. That maps to `Degraded`.
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                drop(stream);
            }
        });
        let socket = camino::Utf8PathBuf::from_path_buf(socket_path.clone()).expect("utf8 path");
        let outcome = probe_control_plane_supervisor(&socket);
        match outcome {
            SupervisorProbeOutcome::Degraded { .. } => {}
            SupervisorProbeOutcome::Live | SupervisorProbeOutcome::StaleSocket { .. } => {
                panic!(
                    "a reachable listener that breaks the conversation MUST classify as \
                     Degraded so open surfaces a typed warning; got={outcome:?}"
                )
            }
        }
    }

    #[test]
    fn control_plane_startup_wait_returns_ready_after_spawn_ping() {
        let mut calls = 0usize;
        let outcome = wait_for_control_plane_startup(
            || {
                calls += 1;
                if calls == 1 {
                    SupervisorProbeOutcome::StaleSocket {
                        detail: "socket not ready".to_owned(),
                    }
                } else {
                    SupervisorProbeOutcome::Live
                }
            },
            |_| {},
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(1),
            std::time::Instant::now(),
        );

        assert_eq!(outcome, SupervisorStartupOutcome::Ready);
        assert_eq!(calls, 2);
    }

    #[test]
    fn control_plane_startup_wait_records_degraded_reason() {
        let outcome = wait_for_control_plane_startup(
            || SupervisorProbeOutcome::Degraded {
                detail: "protocol version mismatch".to_owned(),
            },
            |_| {},
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(1),
            std::time::Instant::now(),
        );

        assert_eq!(
            outcome,
            SupervisorStartupOutcome::Degraded {
                detail: "protocol version mismatch".to_owned()
            }
        );
    }

    #[test]
    fn control_plane_startup_wait_times_out_with_last_probe_reason() {
        let started_at = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_millis(200))
            .expect("instant subtraction");
        let outcome = wait_for_control_plane_startup(
            || SupervisorProbeOutcome::StaleSocket {
                detail: "no socket file present".to_owned(),
            },
            |_| {},
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(1),
            started_at,
        );

        assert_eq!(
            outcome,
            SupervisorStartupOutcome::TimedOut {
                deadline_ms: 100,
                last_detail: "no socket file present".to_owned()
            }
        );
    }

    // ── Runtime synchronization notes
    //
    // Runtime observations are reconciled with the launch registry by the
    // open flow; no persisted runtime reconstruction data is involved.
}

//! Command-layer wiring for routing `workhive pane` text delivery
//! through the runtime control plane.
//!
//! This module is the single command-layer entry point used by
//! [`crate::commands::pane`] dispatchers (`PaneAction::ApproveHooks`
//! and `PaneAction::Send`) to deliver pane input. It encapsulates the
//! sanctioned supervisor self-heal sequence and the bounded socket
//! wait so the dispatcher itself stays free of control-plane wiring.
//!
//! The module intentionally uses explicit imports rather than a glob
//! re-export from its parent so the symbol surface it actually depends
//! on is visible at the top of the file and stable across future moves.
//!
//! ## Production contract
//!
//! 1. The pane supervisor is the only sanctioned delivery path.
//! 2. If the supervisor socket is missing, Workhive performs ONE
//!    controlled self-heal: it spawns the hidden
//!    `internal control-plane serve` subprocess via the existing
//!    composition root, waits up to a bounded deadline for the socket
//!    to materialize, and retries the send.
//! 3. If the retry still cannot reach a supervisor, Workhive returns a
//!    typed, localized error keyed by
//!    `workhive_core::i18n_keys::PANE_CONTROL_PLANE_UNAVAILABLE`. There
//!    is NO silent fallback to direct tmux and NO public flag that
//!    bypasses the control plane.

use std::time::{Duration, Instant};

use camino::Utf8Path;
use workhive_core::control_plane::{
    PaneControlPlaneClient, PaneControlPlaneError, SendPaneInputOutcome,
};
use workhive_core::domain::UnitId;
use workhive_core::i18n_keys as k;
use workhive_core::ports::{I18nPort, PaneRuntimePort};
use workhive_infra::control_plane::{LocalPaneControlPlaneClient, control_plane_socket_path};

use crate::commands::detect_project;
use crate::context::CommandContext;
use crate::error::CliError;

/// Route a `pane send` (or `pane approve-hooks`) text payload through
/// the runtime control plane.
///
/// The `_runtime` parameter is retained on the public boundary so the
/// dispatcher's call site keeps its existing argument shape; this
/// helper deliberately never touches the runtime port itself because
/// the typed delivery path lives behind
/// [`LocalPaneControlPlaneClient::send_pane_input`].
pub(crate) fn dispatch_pane_send(
    ctx: &CommandContext,
    _runtime: &dyn PaneRuntimePort,
    server: &str,
    workspace: &str,
    unit_id: &UnitId,
    text: &str,
    submit: bool,
) -> Result<(), CliError> {
    let (project_root, _project_name) = detect_project()?;
    let socket_path = control_plane_socket_path(&project_root, server);
    let client = LocalPaneControlPlaneClient::new(socket_path.clone());

    match client.send_pane_input(unit_id, text, submit) {
        Ok(SendPaneInputOutcome::Delivered) => return Ok(()),
        Ok(SendPaneInputOutcome::RejectedByPolicy { reason }) => {
            return Err(CliError::Runtime(reason));
        }
        Err(PaneControlPlaneError::UnknownPane { unit_id }) => {
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                ctx.locale(),
                k::PANE_CONTROL_PLANE_UNKNOWN_PANE,
                &[unit_id.as_str()],
            )));
        }
        Err(PaneControlPlaneError::SupervisorUnavailable { detail }) => {
            tracing::warn!(
                socket = %socket_path,
                detail = %detail,
                "pane control plane unavailable; attempting supervised self-heal retry"
            );
        }
        Err(other) => return Err(CliError::Runtime(other.to_string())),
    }

    // Self-heal: spawn the supervisor through the sanctioned
    // composition path and retry the send exactly once.
    if let Err(error) =
        ctx.composition
            .spawn_control_plane_supervisor(&project_root, server, workspace)
    {
        tracing::warn!(%error, "pane control plane self-heal: spawn failed");
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            ctx.locale(),
            k::PANE_CONTROL_PLANE_UNAVAILABLE,
            &[unit_id.as_str()],
        )));
    }
    if !wait_for_socket(&socket_path, Duration::from_secs(2)) {
        tracing::warn!(
            socket = %socket_path,
            "pane control plane self-heal: socket did not materialize"
        );
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            ctx.locale(),
            k::PANE_CONTROL_PLANE_UNAVAILABLE,
            &[unit_id.as_str()],
        )));
    }
    let retry_client = LocalPaneControlPlaneClient::new(socket_path.clone());
    match retry_client.send_pane_input(unit_id, text, submit) {
        Ok(SendPaneInputOutcome::Delivered) => Ok(()),
        Ok(SendPaneInputOutcome::RejectedByPolicy { reason }) => Err(CliError::Runtime(reason)),
        Err(PaneControlPlaneError::UnknownPane { unit_id }) => {
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                ctx.locale(),
                k::PANE_CONTROL_PLANE_UNKNOWN_PANE,
                &[unit_id.as_str()],
            )))
        }
        Err(PaneControlPlaneError::SupervisorUnavailable { detail }) => {
            tracing::warn!(
                socket = %socket_path,
                detail = %detail,
                "pane control plane self-heal: retry still unavailable"
            );
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                ctx.locale(),
                k::PANE_CONTROL_PLANE_UNAVAILABLE,
                &[unit_id.as_str()],
            )))
        }
        Err(other) => Err(CliError::Runtime(other.to_string())),
    }
}

/// Poll the canonical control-plane socket path until it exists, the
/// deadline elapses, or the wait is interrupted. Returns `true` only
/// when the file actually appears within `timeout`.
fn wait_for_socket(socket: &Utf8Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if socket.as_std_path().exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    socket.as_std_path().exists()
}

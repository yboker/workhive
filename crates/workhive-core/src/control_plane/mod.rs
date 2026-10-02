//! Workhive pane runtime control plane — typed protocol and client port.
//!
//! This module is the runtime-neutral brain of the pane control plane.
//! It introduces:
//!
//!   * a typed, versioned request/response protocol (`protocol` submodule);
//!   * a [`PaneControlPlaneClient`] port describing what a CLI caller needs
//!     from the local supervisor without knowing whether the transport is a
//!     Unix socket, in-process channel, or future TCP endpoint;
//!   * typed [`PaneControlPlaneError`] values that cover every real
//!     user-visible failure — "supervisor unavailable", "version mismatch",
//!     "protocol error", "backend failure", "policy rejection".
//!
//! `workhive-core` deliberately does NOT own the socket path, the `serde_json`
//! wire encoding, or the `UnixStream` transport: those belong to
//! `workhive-infra`. Core's contract is that any transport which speaks this
//! typed protocol is a valid control plane.
//!
//! ### Why a separate module
//!
//! The existing `PaneRuntimePort` is still the primitive "what can a runtime
//! do to a pane" boundary. The control plane is the in-process / on-socket
//! facade that:
//!
//!   1. centralizes pane registry + state observation behind a single owner;
//!   2. turns pane send into a typed, acknowledged, request-id-tagged
//!      exchange instead of a fire-and-forget `tmux send-keys`;
//!   3. keeps the future door open for retries, delivery receipts, and
//!      primary/worker orchestration — all of which need a request/response
//!      shape, not a trait of fire-and-forget method calls.
//!
//! Core use cases (`usecases::pane::send_keys`) still call `PaneRuntimePort`
//! directly today and will keep doing so; the CLI boundary opportunistically
//! upgrades to the control plane when the supervisor socket is live.

pub mod protocol;

use crate::domain::UnitId;

pub use protocol::{
    CONTROL_PLANE_PROTOCOL_VERSION, ControlPlaneRequest, ControlPlaneResponse,
    ControlPlaneResponseKind, PaneSnapshot, ProtocolVersion, RequestEnvelope, RequestId,
    ResponseEnvelope, SendInputAction, SendInputRequestPayload,
};

/// Typed errors surfaced by the client port.
///
/// The CLI maps these to localized messages at the boundary; core refuses to
/// stringify into arbitrary English sentences.
#[derive(Debug, thiserror::Error)]
pub enum PaneControlPlaneError {
    /// The supervisor socket could not be reached at all. This is expected
    /// whenever the supervisor has not been spawned yet and is NOT an error
    /// that should stop the CLI — the caller is free to fall back to a
    /// degraded direct-runtime path.
    #[error("pane control plane supervisor unavailable: {detail}")]
    SupervisorUnavailable { detail: String },

    /// The supervisor answered but spoke a protocol the client cannot honour.
    #[error("pane control plane protocol version mismatch: client={client}, server={server}")]
    ProtocolVersionMismatch {
        client: ProtocolVersion,
        server: ProtocolVersion,
    },

    /// Transport succeeded but the payload could not be decoded.
    #[error("pane control plane transport error: {detail}")]
    Transport { detail: String },

    /// The supervisor reported a typed backend failure.
    #[error("pane control plane backend error: {detail}")]
    Backend { detail: String },

    /// The request was understood but the supervisor refused to honour it.
    /// This is reserved for policy (e.g. secret redaction, allow-list) rather
    /// than transient IO issues.
    #[error("pane control plane policy rejection: {reason}")]
    Policy { reason: String },

    /// The supervisor did not find the requested pane in its registry /
    /// runtime listing. Surfaced as a typed variant (rather than a generic
    /// `Backend` error) so the CLI can render a localized "未知 pane" message
    /// without string-matching English detail.
    #[error("pane control plane unknown pane: {unit_id}")]
    UnknownPane { unit_id: UnitId },
}

/// Outcome for a successful `send_pane_input` round-trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendPaneInputOutcome {
    /// Supervisor acknowledges delivery. The subsequent pane state is
    /// observable through `query_pane`.
    Delivered,
    /// Supervisor refused the send with a typed, already-localized reason.
    /// This keeps locale inversion out of core; the CLI only surfaces it.
    RejectedByPolicy { reason: String },
}

/// Port implemented by concrete transports (UDS, future TCP, in-process).
///
/// The trait stays small on purpose: every extension point must be a typed
/// request/response variant in the `protocol` module, not a new bespoke
/// method that duplicates state.
pub trait PaneControlPlaneClient {
    /// Send literal text, optionally followed by an Enter key event, to the
    /// named pane. Transport is responsible for:
    ///   * attaching a fresh [`RequestId`];
    ///   * negotiating [`ProtocolVersion`];
    ///   * mapping transport-layer errors into [`PaneControlPlaneError`].
    fn send_pane_input(
        &self,
        unit_id: &UnitId,
        text: &str,
        submit: bool,
    ) -> Result<SendPaneInputOutcome, PaneControlPlaneError>;

    /// Probe whether the supervisor is reachable without performing any
    /// side-effecting pane operation. Intended for diagnostics only.
    fn ping(&self) -> Result<ProtocolVersion, PaneControlPlaneError>;

    /// Read-only projection of a pane's state, owned by the supervisor.
    /// Returns [`PaneControlPlaneError::UnknownPane`] when the requested
    /// unit is not registered. The supervisor refreshes the registry from
    /// the runtime listing before answering so callers observe a consistent
    /// `(kind, status, cwd, command, display_title)` tuple.
    fn query_pane(&self, unit_id: &UnitId) -> Result<PaneSnapshot, PaneControlPlaneError>;
}

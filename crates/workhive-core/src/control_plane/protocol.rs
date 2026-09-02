//! Versioned, typed request/response protocol for the pane runtime control
//! plane. This is the wire contract between the CLI (client) and the
//! supervisor (server); transports in `workhive-infra` wrap these types in
//! their concrete framing and IO.
//!
//! ### Design rules
//!
//! 1. Every request carries a [`ProtocolVersion`] and a [`RequestId`] so the
//!    supervisor can do forward-compatible rejection and multi-request
//!    correlation.
//! 2. Requests and responses are fully typed enums with named fields. No
//!    free-form strings, no stringly-typed command dispatch.
//! 3. Responses are always enveloped and tagged with the request id so a
//!    future multiplexed transport can correlate them without relying on
//!    ordering.
//! 4. Errors travel as typed [`ControlPlaneResponseKind::Error`] variants,
//!    not as transport-level errors, so "backend said no" and "transport
//!    failed" stay distinguishable to callers.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::domain::{PaneKind, PaneRuntimeStatus, UnitId};

/// Current protocol version. Bump when adding non-forward-compatible fields.
///
/// Minor bumps are allowed only when new variants/fields are strictly
/// additive; the server must still accept the older minor as compatible.
pub const CONTROL_PLANE_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// Two-component version with explicit major/minor semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
    }
}

impl ProtocolVersion {
    /// Client-side compatibility predicate: the caller can speak this
    /// protocol iff the server's major matches and the server's minor is at
    /// least as new as the client's declared minor.
    pub fn is_compatible_with(self, other: ProtocolVersion) -> bool {
        self.major == other.major && other.minor >= self.minor
    }
}

/// Opaque, caller-supplied correlation id. Transports typically derive this
/// from a monotonic counter or a UUID; neither semantics leaks into core.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub String);

impl RequestId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Top-level envelope carrying protocol metadata plus the typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    /// Protocol version the client believes it is speaking. The server must
    /// reject incompatible majors with a typed error envelope.
    pub version: ProtocolVersion,
    /// Caller-owned correlation id, surfaced back on the response.
    pub request_id: RequestId,
    /// Typed request body.
    pub request: ControlPlaneRequest,
}

/// Response envelope. The server MUST echo the client's request id so a
/// future multiplexed transport (e.g. one long-lived connection carrying
/// many requests) can correlate responses without relying on ordering.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub version: ProtocolVersion,
    pub request_id: RequestId,
    pub response: ControlPlaneResponse,
}

/// Every typed request variant the supervisor understands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlPlaneRequest {
    /// Probe endpoint. Returns server [`ProtocolVersion`] for version
    /// negotiation and stale-socket detection.
    Ping,
    /// Deliver literal text and optional Enter to a pane. Honours the same
    /// item-7 semantics as [`crate::ports::SendInputRequest`].
    SendInput(SendInputRequestPayload),
    /// Read-only snapshot of a single pane. Intended for diagnostics.
    QueryPane { unit_id: UnitId },
}

/// Response body. Success variants are explicit, and errors are typed rather
/// than stringly wrapped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlPlaneResponse {
    Pong { server: ProtocolVersion },
    SendInput { outcome: SendInputOutcome },
    QueryPane { snapshot: PaneSnapshot },
    Error { error: ControlPlaneResponseError },
}

/// The subset of response-body shapes we expose for classification. Having
/// a dedicated `kind()` accessor keeps callers from matching on the serde
/// representation directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPlaneResponseKind {
    Pong,
    SendInput,
    QueryPane,
    Error,
}

impl ControlPlaneResponse {
    pub fn kind(&self) -> ControlPlaneResponseKind {
        match self {
            Self::Pong { .. } => ControlPlaneResponseKind::Pong,
            Self::SendInput { .. } => ControlPlaneResponseKind::SendInput,
            Self::QueryPane { .. } => ControlPlaneResponseKind::QueryPane,
            Self::Error { .. } => ControlPlaneResponseKind::Error,
        }
    }
}

/// Typed error returned in a successful transport round-trip. Transport
/// failures are NOT encoded here; they surface as
/// [`crate::control_plane::PaneControlPlaneError::Transport`] or friends.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ControlPlaneResponseError {
    /// The supervisor recognized the request but the backend (e.g. tmux)
    /// reported a failure. `detail` is debug-grade English intended for
    /// logs — the CLI must not render it raw to end users in zh-CN mode;
    /// it maps to a localized key instead.
    Backend { detail: String },
    /// The supervisor refused the request for policy reasons (secret
    /// redaction, allow-list). The reason is already-localized display
    /// copy so it can be surfaced directly.
    Policy { reason: String },
    /// Requested unit id is not known to the supervisor.
    UnknownPane { unit_id: UnitId },
    /// Incompatible protocol version announced by the client.
    ProtocolVersionMismatch {
        client: ProtocolVersion,
        server: ProtocolVersion,
    },
}

/// Payload for [`ControlPlaneRequest::SendInput`]. Lifted into a named
/// struct so the wire shape stays stable when we later add fields like
/// `per_request_timeout_ms`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendInputRequestPayload {
    pub unit_id: UnitId,
    /// Ordered list of actions. The supervisor delivers them in order, with
    /// each [`SendInputAction::LiteralText`] going through tmux `send-keys
    /// -l` and each [`SendInputAction::Key`] going through a separate
    /// `send-keys` invocation. This is exactly the literal+Enter shape
    /// required by item 7.
    pub actions: Vec<SendInputAction>,
}

/// Individual send-side action.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendInputAction {
    /// Deliver `text` as a run of literal keystrokes. MUST NOT be
    /// interpreted as tmux key names by the adapter.
    LiteralText { text: String },
    /// Deliver a single key event. `name` is the tmux-style key name such
    /// as `Enter`, `C-c`, `BSpace`. Core does NOT enumerate the tmux key
    /// alphabet; the adapter validates the name.
    Key { name: String },
}

/// Delivery outcome as observed by the supervisor after it routed a send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SendInputOutcome {
    Delivered,
    RejectedByPolicy { reason: String },
}

/// Read-only projection of a pane's state, sourced by the supervisor from
/// the existing [`crate::ports::PaneRegistryPort`] + runtime listing. The
/// fields below are the wire-stable subset of [`crate::domain::PaneDescriptor`]
/// that callers are expected to depend on.
///
/// The supervisor populates each field from the registry-backed
/// `PaneDescriptor` for the requested `unit_id`; if no descriptor exists
/// after a refresh, the supervisor returns
/// [`ControlPlaneResponseError::UnknownPane`] instead of fabricating a
/// snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSnapshot {
    /// Runtime-neutral pane identifier, echoed back so the caller can
    /// correlate the response with its request without round-tripping
    /// the request body.
    pub unit_id: UnitId,
    /// Human-readable title (typically tmux `pane_title`); `None` when
    /// the backend reports no title.
    pub display_title: Option<String>,
    /// Best-effort kind classification (`Agent`, `Terminal`, `GitUi`,
    /// `Unknown`). Mirrors `PaneDescriptor::kind`.
    pub kind: PaneKind,
    /// Registry-level lifecycle status. Mirrors
    /// `PaneDescriptor::observed_status`.
    pub status: PaneRuntimeStatus,
    /// Current working directory as last observed, or `None` when the
    /// backend has not yet reported one.
    pub cwd: Option<String>,
    /// Foreground command name, or `None` when the backend has not yet
    /// reported one.
    pub command: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_compatibility_is_forward_only_within_major() {
        let v1_0 = ProtocolVersion { major: 1, minor: 0 };
        let v1_1 = ProtocolVersion { major: 1, minor: 1 };
        let v2_0 = ProtocolVersion { major: 2, minor: 0 };
        assert!(v1_0.is_compatible_with(v1_1));
        assert!(v1_0.is_compatible_with(v1_0));
        assert!(!v1_1.is_compatible_with(v1_0));
        assert!(!v1_0.is_compatible_with(v2_0));
    }

    #[test]
    fn request_envelope_round_trips_through_serde_json() {
        let envelope = RequestEnvelope {
            version: CONTROL_PLANE_PROTOCOL_VERSION,
            request_id: RequestId::new("req-1"),
            request: ControlPlaneRequest::SendInput(SendInputRequestPayload {
                unit_id: UnitId::new("4"),
                actions: vec![
                    SendInputAction::LiteralText {
                        text: "hello world".to_owned(),
                    },
                    SendInputAction::Key {
                        name: "Enter".to_owned(),
                    },
                ],
            }),
        };
        let json = serde_json::to_string(&envelope).expect("serialize");
        let decoded: RequestEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.request_id.as_str(), "req-1");
        match decoded.request {
            ControlPlaneRequest::SendInput(payload) => {
                assert_eq!(payload.unit_id.as_str(), "4");
                assert_eq!(payload.actions.len(), 2);
            }
            _ => panic!("expected SendInput"),
        }
    }

    #[test]
    fn response_envelope_tags_kind_accessor() {
        let err_response = ControlPlaneResponse::Error {
            error: ControlPlaneResponseError::Backend {
                detail: "tmux died".to_owned(),
            },
        };
        assert_eq!(err_response.kind(), ControlPlaneResponseKind::Error);
        let pong = ControlPlaneResponse::Pong {
            server: CONTROL_PLANE_PROTOCOL_VERSION,
        };
        assert_eq!(pong.kind(), ControlPlaneResponseKind::Pong);
    }
}

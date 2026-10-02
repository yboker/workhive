//! Message bus domain types for pane-to-pane communication.
//!
//! These types define the contract for the pane messaging bus introduced
//! in Phase 7. They are intentionally separate from the point-to-point
//! [`PaneMessage`](super::pane::PaneMessage) type from Phase 6, which
//! continues to exist for direct delivery.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::pane::{PaneCapability, PaneMessagePayload, PaneRole, PaneRuntimeId};
use super::workspace_id::WorkspaceId;

/// Stable, unique identifier for a message in the bus.
///
/// Wraps a UUID v4 to guarantee uniqueness without coordination.
/// The inner value survives serialization round-trips and can be
/// used as a map key or log correlation token.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Generate a new random message ID.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Reconstruct from a previously serialized UUID.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Return the inner UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Addressing target for a message on the bus.
///
/// Messages may target a specific pane, all panes with a given role,
/// all panes with a given capability, or every message-capable pane
/// in the workspace (broadcast).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum MessageTarget {
    /// Deliver to a specific pane by runtime ID.
    Pane(PaneRuntimeId),
    /// Deliver to all panes with the given role.
    Role(PaneRole),
    /// Deliver to all panes with the given capability.
    Capability(PaneCapability),
    /// Deliver to all message-capable panes in the workspace.
    Broadcast,
}

/// Envelope wrapping a message for bus submission.
///
/// The envelope carries routing metadata (source, workspace, target)
/// alongside the payload. The bus assigns an [`MessageId`] at creation
/// time and uses the target to resolve recipients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Unique identifier assigned at envelope creation.
    pub id: MessageId,
    /// Sender pane, or `None` for control-plane messages.
    pub source: Option<PaneRuntimeId>,
    /// Workspace scope for routing.
    pub workspace: WorkspaceId,
    /// Addressing target.
    pub target: MessageTarget,
    /// Message payload.
    pub payload: PaneMessagePayload,
}

/// Result of submitting a message to the bus.
///
/// `Accepted` means the bus has taken ownership of the message and
/// will attempt delivery. `Rejected` means the bus refused the message
/// before any delivery attempt — the caller must handle the rejection.
///
/// **Critical contract:** `Accepted` does NOT mean the message was
/// delivered or acknowledged. Delivery status is tracked separately
/// via [`MessageStatus`] and [`MessageEvent`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MessageAcceptance {
    /// The bus accepted the message for delivery.
    Accepted { message_id: MessageId },
    /// The bus rejected the message before delivery.
    Rejected(MessageRejectReason),
}

/// Reason the bus rejected a message at submission time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRejectReason {
    /// No pane matches the target (direct, role, or capability).
    NoRouteForTarget,
    /// The target pane exists but lacks `MessageReceiver` capability.
    TargetNotCapable,
}

/// Current lifecycle status of a message in the bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    /// Message accepted and waiting for routing.
    Queued,
    /// Recipients resolved; delivery in progress.
    Routed,
    /// Delivery attempted to at least one recipient.
    Dispatched,
    /// All recipients acknowledged receipt.
    Acknowledged,
    /// Delivery failed for at least one recipient.
    Failed,
    /// Message delivery timed out.
    TimedOut,
}

/// Event recording a state transition in the message lifecycle.
///
/// Events are produced by the bus during `publish()` and stored in
/// an internal buffer. Consumers drain events via
/// [`PaneMessageBusPort::drain_events`](crate::ports::PaneMessageBusPort::drain_events).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum MessageEvent {
    /// Message was accepted and queued.
    Queued { message_id: MessageId },
    /// Recipients were resolved from the target.
    Routed {
        message_id: MessageId,
        recipients: Vec<PaneRuntimeId>,
    },
    /// Delivery was attempted to a specific recipient.
    Dispatched {
        message_id: MessageId,
        recipient: PaneRuntimeId,
    },
    /// A recipient acknowledged the message.
    Acknowledged {
        message_id: MessageId,
        recipient: PaneRuntimeId,
    },
    /// Delivery failed for a recipient (or globally).
    Failed {
        message_id: MessageId,
        recipient: Option<PaneRuntimeId>,
        reason: MessageFailureReason,
    },
}

/// Reason a message delivery failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageFailureReason {
    /// The runtime delivery mechanism reported an error.
    DeliveryFailed { detail: String },
    /// The delivery attempt timed out.
    Timeout,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_is_unique() {
        let a = MessageId::new();
        let b = MessageId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn message_id_display() {
        let id = MessageId::new();
        let display = id.to_string();
        assert!(!display.is_empty());
        assert!(display.contains('-'), "UUID display must contain dashes");
    }

    #[test]
    fn message_id_serde_round_trip() {
        let id = MessageId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        let restored: MessageId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, restored);
    }

    #[test]
    fn message_id_hash_works() {
        use std::collections::HashSet;
        let a = MessageId::new();
        let b = a.clone();
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn message_target_pane_serde() {
        let target = MessageTarget::Pane(PaneRuntimeId::derive("g", "ws", "%0"));
        let json = serde_json::to_string(&target).expect("serialize");
        assert!(json.contains("\"kind\":\"pane\""));
    }

    #[test]
    fn message_target_broadcast_serde() {
        let target = MessageTarget::Broadcast;
        let json = serde_json::to_string(&target).expect("serialize");
        assert!(json.contains("\"kind\":\"broadcast\""));
    }

    #[test]
    fn message_acceptance_accepted_serde() {
        let acc = MessageAcceptance::Accepted {
            message_id: MessageId::new(),
        };
        let json = serde_json::to_string(&acc).expect("serialize");
        assert!(json.contains("\"result\":\"accepted\""));
    }

    #[test]
    fn message_acceptance_rejected_serde() {
        let rej = MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget);
        let json = serde_json::to_string(&rej).expect("serialize");
        assert!(json.contains("\"result\":\"rejected\""));
    }

    #[test]
    fn message_status_serde_round_trip() {
        let statuses = [
            MessageStatus::Queued,
            MessageStatus::Routed,
            MessageStatus::Dispatched,
            MessageStatus::Acknowledged,
            MessageStatus::Failed,
            MessageStatus::TimedOut,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).expect("serialize");
            let restored: MessageStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*status, restored);
        }
    }

    #[test]
    fn message_event_queued_serde() {
        let event = MessageEvent::Queued {
            message_id: MessageId::new(),
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"event\":\"queued\""));
    }

    #[test]
    fn message_event_failed_serde() {
        let event = MessageEvent::Failed {
            message_id: MessageId::new(),
            recipient: None,
            reason: MessageFailureReason::Timeout,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"event\":\"failed\""));
        assert!(json.contains("\"kind\":\"timeout\""));
    }

    #[test]
    fn message_failure_delivery_carries_detail() {
        let reason = MessageFailureReason::DeliveryFailed {
            detail: "connection refused".to_owned(),
        };
        let json = serde_json::to_string(&reason).expect("serialize");
        assert!(json.contains("connection refused"));
    }

    #[test]
    fn message_envelope_serde_round_trip() {
        let envelope = MessageEnvelope {
            id: MessageId::new(),
            source: Some(PaneRuntimeId::derive("g", "ws", "%0")),
            workspace: WorkspaceId::new("test-ws").expect("valid ws"),
            target: MessageTarget::Broadcast,
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        };
        let json = serde_json::to_string(&envelope).expect("serialize");
        let restored: MessageEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(envelope.id, restored.id);
    }

    #[test]
    fn reject_reason_exhaustive() {
        let reasons = [
            MessageRejectReason::NoRouteForTarget,
            MessageRejectReason::TargetNotCapable,
        ];
        for reason in &reasons {
            let json = serde_json::to_string(reason).expect("serialize");
            let restored: MessageRejectReason = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*reason, restored);
        }
    }
}

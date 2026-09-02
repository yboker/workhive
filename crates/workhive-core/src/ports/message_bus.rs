//! Port traits for the pane message bus and delivery subsystem.
//!
//! [`PaneMessageBusPort`] defines the bus contract: publish messages,
//! query status, and drain lifecycle events. It is implemented by
//! `InMemoryMessageBus` in `workhive-infra`.
//!
//! [`PaneMessageDeliveryPort`] is the runtime delivery seam: the bus
//! delegates actual pane-level I/O through this trait so that delivery
//! mechanics (tmux send-keys, etc.) stay behind the port boundary.

use crate::domain::{
    MessageAcceptance, MessageEnvelope, MessageEvent, MessageId, MessageStatus, PaneMessagePayload,
    PaneRuntimeId,
};

/// Errors from message bus operations.
#[derive(Debug, thiserror::Error)]
pub enum PaneMessageBusError {
    /// The bus encountered an internal error.
    #[error("message bus error: {message}")]
    Internal { message: String },

    /// The registry was unavailable or returned an error during routing.
    #[error("registry lookup failed during routing: {detail}")]
    RegistryLookupFailed { detail: String },
}

/// Errors from runtime message delivery.
#[derive(Debug, thiserror::Error)]
pub enum PaneMessageDeliveryError {
    /// The runtime delivery mechanism reported an error.
    #[error("delivery to {target} failed: {detail}")]
    DeliveryFailed { target: String, detail: String },
}

/// Result of a single delivery attempt to a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryAttempt {
    /// The pane that was targeted.
    pub target: PaneRuntimeId,
    /// Whether the delivery succeeded at the transport level.
    pub success: bool,
    /// Human-readable detail on failure (empty on success).
    pub detail: String,
}

/// Bus contract for pane-to-pane messaging.
///
/// The bus accepts messages via [`publish`](Self::publish), resolves
/// targets to recipients, attempts delivery, and records lifecycle
/// events. Callers consume the event stream via
/// [`drain_events`](Self::drain_events).
///
/// ## Contract invariants
///
/// - `publish()` returns [`MessageAcceptance`] only — never business
///   output from the target pane.
/// - Status transitions are explicit and recorded as [`MessageEvent`]
///   values.
/// - Message IDs are stable once assigned.
pub trait PaneMessageBusPort {
    /// Submit a message to the bus for routing and delivery.
    ///
    /// Returns `Accepted` if the bus took ownership, or `Rejected` if
    /// the message cannot be routed (e.g., no matching target).
    fn publish(
        &mut self,
        envelope: MessageEnvelope,
    ) -> Result<MessageAcceptance, PaneMessageBusError>;

    /// Query the current lifecycle status of a previously published message.
    ///
    /// Returns `None` if the message ID is unknown to the bus.
    fn status(&self, message_id: &MessageId) -> Result<Option<MessageStatus>, PaneMessageBusError>;

    /// Drain all pending lifecycle events since the last drain.
    ///
    /// This is destructive — events are removed from the buffer once
    /// returned.
    fn drain_events(&mut self) -> Result<Vec<MessageEvent>, PaneMessageBusError>;
}

/// Runtime delivery seam for the message bus.
///
/// The bus calls [`deliver`](Self::deliver) for each resolved recipient.
/// The implementation translates this into backend-specific I/O (e.g.,
/// tmux `send-keys`).
///
/// This trait is intentionally narrow: it handles single-pane delivery
/// only. Fan-out, retry, and timeout logic belong in the bus, not here.
pub trait PaneMessageDeliveryPort {
    /// Attempt to deliver a payload to a specific pane.
    fn deliver(
        &mut self,
        target: &PaneRuntimeId,
        payload: &PaneMessagePayload,
    ) -> Result<DeliveryAttempt, PaneMessageDeliveryError>;
}

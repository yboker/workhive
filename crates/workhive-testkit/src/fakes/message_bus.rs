//! Fake message bus and delivery port for testing.
//!
//! [`FakeMessageBus`] records all published messages and returns
//! configurable acceptance/rejection. [`FakeMessageDelivery`] records
//! delivery attempts and can be configured to succeed or fail.

use workhive_core::domain::{
    MessageAcceptance, MessageEnvelope, MessageEvent, MessageId, MessageStatus, PaneMessagePayload,
    PaneRuntimeId,
};
use workhive_core::ports::{
    DeliveryAttempt, PaneMessageBusError, PaneMessageBusPort, PaneMessageDeliveryError,
    PaneMessageDeliveryPort,
};

/// Fake message bus that records published envelopes.
///
/// By default, all publishes return `Accepted`. Set `reject_all` to
/// true to make all publishes return `Rejected(NoRouteForTarget)`.
pub struct FakeMessageBus {
    published: Vec<MessageEnvelope>,
    events: Vec<MessageEvent>,
    reject_all: bool,
}

impl FakeMessageBus {
    pub fn new() -> Self {
        Self {
            published: Vec::new(),
            events: Vec::new(),
            reject_all: false,
        }
    }

    /// Configure the bus to reject all publishes.
    pub fn reject_all(mut self) -> Self {
        self.reject_all = true;
        self
    }

    /// Return all published envelopes (non-destructive).
    pub fn published(&self) -> &[MessageEnvelope] {
        &self.published
    }
}

impl Default for FakeMessageBus {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneMessageBusPort for FakeMessageBus {
    fn publish(
        &mut self,
        envelope: MessageEnvelope,
    ) -> Result<MessageAcceptance, PaneMessageBusError> {
        if self.reject_all {
            return Ok(MessageAcceptance::Rejected(
                workhive_core::domain::MessageRejectReason::NoRouteForTarget,
            ));
        }

        let message_id = envelope.id.clone();
        self.events.push(MessageEvent::Queued {
            message_id: message_id.clone(),
        });
        self.published.push(envelope);
        Ok(MessageAcceptance::Accepted { message_id })
    }

    fn status(&self, message_id: &MessageId) -> Result<Option<MessageStatus>, PaneMessageBusError> {
        if self.published.iter().any(|e| e.id == *message_id) {
            Ok(Some(MessageStatus::Acknowledged))
        } else {
            Ok(None)
        }
    }

    fn drain_events(&mut self) -> Result<Vec<MessageEvent>, PaneMessageBusError> {
        Ok(self.events.drain(..).collect())
    }
}

/// Fake delivery port that records delivery attempts.
///
/// By default, all deliveries succeed. Set `fail_all` to true to
/// make all deliveries return failure.
pub struct FakeMessageDelivery {
    deliveries: Vec<(PaneRuntimeId, PaneMessagePayload)>,
    fail_all: bool,
    fail_message: String,
}

impl FakeMessageDelivery {
    pub fn new() -> Self {
        Self {
            deliveries: Vec::new(),
            fail_all: false,
            fail_message: String::new(),
        }
    }

    /// Configure the delivery to fail all attempts.
    pub fn fail_all(mut self, message: &str) -> Self {
        self.fail_all = true;
        self.fail_message = message.to_owned();
        self
    }

    /// Return all delivery attempts (non-destructive).
    pub fn deliveries(&self) -> &[(PaneRuntimeId, PaneMessagePayload)] {
        &self.deliveries
    }
}

impl Default for FakeMessageDelivery {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneMessageDeliveryPort for FakeMessageDelivery {
    fn deliver(
        &mut self,
        target: &PaneRuntimeId,
        payload: &PaneMessagePayload,
    ) -> Result<DeliveryAttempt, PaneMessageDeliveryError> {
        self.deliveries.push((target.clone(), payload.clone()));

        if self.fail_all {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: false,
                detail: self.fail_message.clone(),
            })
        } else {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: true,
                detail: String::new(),
            })
        }
    }
}

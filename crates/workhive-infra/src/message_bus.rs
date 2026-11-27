//! In-memory message bus implementation.
//!
//! [`InMemoryMessageBus`] implements [`PaneMessageBusPort`] using
//! in-memory storage for messages, statuses, and events. It resolves
//! [`MessageTarget`] variants using a [`PaneRegistryPort`] reference
//! and delegates delivery to a [`PaneMessageDeliveryPort`].
//!
//! This is the MVP implementation — no persistence, no background
//! threads, no async. Delivery is attempted synchronously during
//! [`publish`](InMemoryMessageBus::publish).

use workhive_core::domain::{
    MessageAcceptance, MessageEnvelope, MessageEvent, MessageFailureReason, MessageId,
    MessageRejectReason, MessageStatus, MessageTarget, PaneCapability, PaneRuntimeId,
};
use workhive_core::ports::{
    PaneMessageBusError, PaneMessageBusPort, PaneMessageDeliveryPort, PaneRegistryPort,
};

/// Resolution outcome distinguishing "no match" from "exists but not capable".
enum ResolveOutcome {
    Recipients(Vec<PaneRuntimeId>),
    TargetNotCapable,
}

/// Record for a message stored in the bus.
struct StoredMessage {
    envelope: MessageEnvelope,
    status: MessageStatus,
}

/// In-memory message bus for pane-to-pane communication.
///
/// Holds references to the pane registry (for target resolution) and
/// a delivery port (for runtime I/O). Messages, statuses, and events
/// are stored in plain `Vec`s.
pub struct InMemoryMessageBus<'a> {
    registry: &'a dyn PaneRegistryPort,
    delivery: &'a mut dyn PaneMessageDeliveryPort,
    messages: Vec<StoredMessage>,
    events: Vec<MessageEvent>,
}

impl<'a> InMemoryMessageBus<'a> {
    /// Create a new bus backed by the given registry and delivery port.
    pub fn new(
        registry: &'a dyn PaneRegistryPort,
        delivery: &'a mut dyn PaneMessageDeliveryPort,
    ) -> Self {
        Self {
            registry,
            delivery,
            messages: Vec::new(),
            events: Vec::new(),
        }
    }

    /// Resolve a [`MessageTarget`] to concrete recipient runtime IDs.
    ///
    /// For `Pane` targets, distinguishes between "pane not found / wrong
    /// workspace" (empty recipients) and "pane found but lacks
    /// `MessageReceiver`" (`TargetNotCapable`).
    fn resolve_recipients(
        &self,
        target: &MessageTarget,
        workspace: &str,
    ) -> Result<ResolveOutcome, PaneMessageBusError> {
        match target {
            MessageTarget::Pane(id) => {
                let desc = self.registry.get(id).map_err(|e| {
                    PaneMessageBusError::RegistryLookupFailed {
                        detail: e.to_string(),
                    }
                })?;
                match desc {
                    Some(d) if d.workspace == workspace => {
                        if d.capabilities.contains(&PaneCapability::MessageReceiver) {
                            Ok(ResolveOutcome::Recipients(vec![d.runtime_id]))
                        } else {
                            Ok(ResolveOutcome::TargetNotCapable)
                        }
                    }
                    _ => Ok(ResolveOutcome::Recipients(vec![])),
                }
            }
            MessageTarget::Role(role) => {
                let all = self.registry.list_workspace(workspace).map_err(|e| {
                    PaneMessageBusError::RegistryLookupFailed {
                        detail: e.to_string(),
                    }
                })?;
                Ok(ResolveOutcome::Recipients(
                    all.into_iter()
                        .filter(|p| p.role == Some(*role))
                        .filter(|p| p.capabilities.contains(&PaneCapability::MessageReceiver))
                        .map(|p| p.runtime_id)
                        .collect(),
                ))
            }
            MessageTarget::Capability(cap) => {
                let matching = self.registry.find_by_capability(*cap).map_err(|e| {
                    PaneMessageBusError::RegistryLookupFailed {
                        detail: e.to_string(),
                    }
                })?;
                Ok(ResolveOutcome::Recipients(
                    matching
                        .into_iter()
                        .filter(|p| p.workspace == workspace)
                        .map(|p| p.runtime_id)
                        .collect(),
                ))
            }
            MessageTarget::Broadcast => {
                let all = self.registry.list_workspace(workspace).map_err(|e| {
                    PaneMessageBusError::RegistryLookupFailed {
                        detail: e.to_string(),
                    }
                })?;
                Ok(ResolveOutcome::Recipients(
                    all.into_iter()
                        .filter(|p| p.capabilities.contains(&PaneCapability::MessageReceiver))
                        .map(|p| p.runtime_id)
                        .collect(),
                ))
            }
        }
    }
}

impl PaneMessageBusPort for InMemoryMessageBus<'_> {
    fn publish(
        &mut self,
        envelope: MessageEnvelope,
    ) -> Result<MessageAcceptance, PaneMessageBusError> {
        let message_id = envelope.id.clone();
        let workspace_str = envelope.workspace.as_str().to_owned();

        let recipients = match self.resolve_recipients(&envelope.target, &workspace_str)? {
            ResolveOutcome::TargetNotCapable => {
                return Ok(MessageAcceptance::Rejected(
                    MessageRejectReason::TargetNotCapable,
                ));
            }
            ResolveOutcome::Recipients(r) if r.is_empty() => {
                return Ok(MessageAcceptance::Rejected(
                    MessageRejectReason::NoRouteForTarget,
                ));
            }
            ResolveOutcome::Recipients(r) => r,
        };

        // Store the message and record Queued
        self.messages.push(StoredMessage {
            envelope: envelope.clone(),
            status: MessageStatus::Queued,
        });
        self.events.push(MessageEvent::Queued {
            message_id: message_id.clone(),
        });

        // Record Routed
        self.events.push(MessageEvent::Routed {
            message_id: message_id.clone(),
            recipients: recipients.clone(),
        });
        self.update_status(&message_id, MessageStatus::Routed);

        // Attempt delivery to each recipient
        let mut any_failed = false;
        for recipient in &recipients {
            match self.delivery.deliver(recipient, &envelope.payload) {
                Ok(attempt) => {
                    self.events.push(MessageEvent::Dispatched {
                        message_id: message_id.clone(),
                        recipient: recipient.clone(),
                    });

                    if attempt.success {
                        self.events.push(MessageEvent::Acknowledged {
                            message_id: message_id.clone(),
                            recipient: recipient.clone(),
                        });
                    } else {
                        any_failed = true;
                        self.events.push(MessageEvent::Failed {
                            message_id: message_id.clone(),
                            recipient: Some(recipient.clone()),
                            reason: MessageFailureReason::DeliveryFailed {
                                detail: attempt.detail,
                            },
                        });
                    }
                }
                Err(e) => {
                    any_failed = true;
                    self.events.push(MessageEvent::Dispatched {
                        message_id: message_id.clone(),
                        recipient: recipient.clone(),
                    });
                    self.events.push(MessageEvent::Failed {
                        message_id: message_id.clone(),
                        recipient: Some(recipient.clone()),
                        reason: MessageFailureReason::DeliveryFailed {
                            detail: e.to_string(),
                        },
                    });
                }
            }
        }

        // Update final status
        if any_failed {
            self.update_status(&message_id, MessageStatus::Failed);
        } else {
            self.update_status(&message_id, MessageStatus::Acknowledged);
        }

        Ok(MessageAcceptance::Accepted { message_id })
    }

    fn status(&self, message_id: &MessageId) -> Result<Option<MessageStatus>, PaneMessageBusError> {
        Ok(self
            .messages
            .iter()
            .find(|m| m.envelope.id == *message_id)
            .map(|m| m.status))
    }

    fn drain_events(&mut self) -> Result<Vec<MessageEvent>, PaneMessageBusError> {
        Ok(std::mem::take(&mut self.events))
    }
}

impl InMemoryMessageBus<'_> {
    fn update_status(&mut self, message_id: &MessageId, status: MessageStatus) {
        if let Some(stored) = self
            .messages
            .iter_mut()
            .find(|m| m.envelope.id == *message_id)
        {
            stored.status = status;
        }
    }

    /// Mark a message as timed out.
    ///
    /// This is exposed for testing and future scheduler use. The MVP
    /// bus does not have a background timer, so timeout must be
    /// triggered externally.
    pub fn mark_timed_out(&mut self, message_id: &MessageId) {
        if let Some(stored) = self
            .messages
            .iter_mut()
            .find(|m| m.envelope.id == *message_id)
        {
            stored.status = MessageStatus::TimedOut;
            self.events.push(MessageEvent::Failed {
                message_id: message_id.clone(),
                recipient: None,
                reason: MessageFailureReason::Timeout,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use workhive_core::domain::{
        PaneDescriptor, PaneKind, PaneLaunchOrigin, PaneMessagePayload, PaneRuntimeStatus,
        PaneState, UnitId, WorkspaceId,
    };
    use workhive_core::ports::DeliveryAttempt;

    // ── Test helpers ──────────────────────────────────────────────────

    struct TestRegistry {
        panes: RefCell<Vec<PaneDescriptor>>,
    }

    impl TestRegistry {
        fn new() -> Self {
            Self {
                panes: RefCell::new(Vec::new()),
            }
        }

        fn add(&self, desc: PaneDescriptor) {
            self.panes.borrow_mut().push(desc);
        }
    }

    impl PaneRegistryPort for TestRegistry {
        fn upsert(
            &self,
            _: PaneDescriptor,
        ) -> Result<workhive_core::ports::RegistryDelta, workhive_core::ports::RegistryError>
        {
            Ok(workhive_core::ports::RegistryDelta {
                changes: Vec::new(),
            })
        }
        fn remove(
            &self,
            _: &PaneRuntimeId,
        ) -> Result<Option<PaneDescriptor>, workhive_core::ports::RegistryError> {
            Ok(None)
        }
        fn get(
            &self,
            id: &PaneRuntimeId,
        ) -> Result<Option<PaneDescriptor>, workhive_core::ports::RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .find(|p| p.runtime_id == *id)
                .cloned())
        }
        fn list_workspace(
            &self,
            workspace: &str,
        ) -> Result<Vec<PaneDescriptor>, workhive_core::ports::RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .filter(|p| p.workspace == workspace)
                .cloned()
                .collect())
        }
        fn list(
            &self,
            _: Option<&str>,
        ) -> Result<Vec<PaneDescriptor>, workhive_core::ports::RegistryError> {
            Ok(self.panes.borrow().clone())
        }
        fn drain_events(
            &self,
        ) -> Result<
            Vec<workhive_core::domain::PaneLifecycleEvent>,
            workhive_core::ports::RegistryError,
        > {
            Ok(vec![])
        }
        fn update_from_observation(
            &self,
            _: workhive_core::domain::ObservedPaneState,
        ) -> Result<Vec<workhive_core::domain::PaneStateChange>, workhive_core::ports::RegistryError>
        {
            Ok(vec![])
        }
        fn find_by_capability(
            &self,
            cap: PaneCapability,
        ) -> Result<Vec<PaneDescriptor>, workhive_core::ports::RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .filter(|p| p.capabilities.contains(&cap))
                .cloned()
                .collect())
        }
    }

    struct SuccessDelivery;

    impl PaneMessageDeliveryPort for SuccessDelivery {
        fn deliver(
            &mut self,
            target: &PaneRuntimeId,
            _payload: &PaneMessagePayload,
        ) -> Result<DeliveryAttempt, workhive_core::ports::PaneMessageDeliveryError> {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: true,
                detail: String::new(),
            })
        }
    }

    struct FailingDelivery {
        fail_message: String,
    }

    impl FailingDelivery {
        fn new(msg: &str) -> Self {
            Self {
                fail_message: msg.to_owned(),
            }
        }
    }

    impl PaneMessageDeliveryPort for FailingDelivery {
        fn deliver(
            &mut self,
            target: &PaneRuntimeId,
            _payload: &PaneMessagePayload,
        ) -> Result<DeliveryAttempt, workhive_core::ports::PaneMessageDeliveryError> {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: false,
                detail: self.fail_message.clone(),
            })
        }
    }

    fn make_agent_descriptor(ws: &str, unit: &str) -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("g", ws, unit),
            group: "g".to_owned(),
            workspace: ws.to_owned(),
            unit_id: UnitId::new(unit),
            cwd: "/tmp".to_owned(),
            command: "agent".to_owned(),
            kind: PaneKind::Agent,
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [
                PaneCapability::CommandExecutor,
                PaneCapability::MessageReceiver,
                PaneCapability::MessageResponder,
            ]
            .into_iter()
            .collect(),
        }
    }

    fn make_terminal_descriptor(ws: &str, unit: &str) -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("g", ws, unit),
            group: "g".to_owned(),
            workspace: ws.to_owned(),
            unit_id: UnitId::new(unit),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            observed_status: PaneRuntimeStatus::Idle,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
        }
    }

    fn make_envelope(ws: &str, target: MessageTarget) -> MessageEnvelope {
        MessageEnvelope {
            id: MessageId::new(),
            source: None,
            workspace: WorkspaceId::new(ws).expect("valid ws"),
            target,
            payload: PaneMessagePayload::Text {
                content: "hello".to_owned(),
            },
        }
    }

    // ── Test matrix from spec ─────────────────────────────────────────

    /// 1. Accept valid direct message and record queued event
    #[test]
    fn accept_valid_direct_message_records_queued() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let result = bus.publish(envelope).expect("publish must succeed");

        assert!(
            matches!(result, MessageAcceptance::Accepted { .. }),
            "valid message must be accepted"
        );

        let events = bus.drain_events().expect("drain events");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MessageEvent::Queued { .. })),
            "queued event must be recorded"
        );
    }

    /// 2. Reject unroutable target truthfully
    #[test]
    fn reject_unroutable_target() {
        let reg = TestRegistry::new();
        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Pane(PaneRuntimeId::derive("g", "ws", "%99")),
        );
        let result = bus.publish(envelope).expect("publish must not error");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "nonexistent pane must be rejected with NoRouteForTarget"
        );
    }

    /// 3. Route by capability across multiple recipients
    #[test]
    fn route_by_capability_multiple_recipients() {
        let reg = TestRegistry::new();
        reg.add(make_agent_descriptor("ws", "%0"));
        reg.add(make_agent_descriptor("ws", "%1"));
        reg.add(make_terminal_descriptor("ws", "%2"));

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Capability(PaneCapability::MessageReceiver),
        );
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));

        let events = bus.drain_events().expect("drain");
        let routed = events
            .iter()
            .find(|e| matches!(e, MessageEvent::Routed { .. }));
        assert!(routed.is_some(), "routed event must be present");

        if let Some(MessageEvent::Routed { recipients, .. }) = routed {
            assert_eq!(
                recipients.len(),
                2,
                "two agent panes must be recipients, terminal excluded"
            );
        }
    }

    /// 4. Record dispatched and acknowledged transitions separately
    #[test]
    fn dispatched_and_acknowledged_separate() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        bus.publish(envelope).expect("publish");

        let events = bus.drain_events().expect("drain");

        let has_dispatched = events
            .iter()
            .any(|e| matches!(e, MessageEvent::Dispatched { .. }));
        let has_acknowledged = events
            .iter()
            .any(|e| matches!(e, MessageEvent::Acknowledged { .. }));

        assert!(has_dispatched, "dispatched event must be recorded");
        assert!(has_acknowledged, "acknowledged event must be recorded");

        let dispatched_pos = events
            .iter()
            .position(|e| matches!(e, MessageEvent::Dispatched { .. }))
            .expect("dispatched");
        let ack_pos = events
            .iter()
            .position(|e| matches!(e, MessageEvent::Acknowledged { .. }))
            .expect("acknowledged");
        assert!(
            dispatched_pos < ack_pos,
            "dispatched must come before acknowledged"
        );
    }

    /// 5. Record failure when runtime delivery fails
    #[test]
    fn record_failure_on_delivery_error() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = FailingDelivery::new("connection lost");
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id.clone()));
        let msg_id = envelope.id.clone();
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(result, MessageAcceptance::Accepted { .. }),
            "message must be accepted even if delivery fails"
        );

        let events = bus.drain_events().expect("drain");
        let failed = events
            .iter()
            .find(|e| matches!(e, MessageEvent::Failed { .. }));
        assert!(failed.is_some(), "failed event must be recorded");

        if let Some(MessageEvent::Failed {
            recipient, reason, ..
        }) = failed
        {
            assert_eq!(*recipient, Some(target_id));
            assert!(matches!(
                reason,
                MessageFailureReason::DeliveryFailed { .. }
            ));
        }

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(
            status,
            MessageStatus::Failed,
            "message status must be Failed"
        );
    }

    /// 6. Time out a message without collapsing into generic failure
    #[test]
    fn timeout_distinct_from_generic_failure() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let msg_id = envelope.id.clone();
        bus.publish(envelope).expect("publish");

        // Externally trigger timeout
        bus.mark_timed_out(&msg_id);

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(
            status,
            MessageStatus::TimedOut,
            "timed out status must be distinct from Failed"
        );

        let events = bus.drain_events().expect("drain");
        let timeout_event = events.iter().find(|e| {
            matches!(
                e,
                MessageEvent::Failed {
                    reason: MessageFailureReason::Timeout,
                    ..
                }
            )
        });
        assert!(
            timeout_event.is_some(),
            "timeout must produce a Failed event with Timeout reason"
        );
    }

    /// 7. Manual workspace remains valid even if no message-capable pane exists
    #[test]
    fn manual_workspace_no_message_capable_panes() {
        let reg = TestRegistry::new();
        reg.add(make_terminal_descriptor("ws", "%0"));

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Broadcast);
        let result = bus.publish(envelope).expect("publish must not error");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "broadcast with no message-capable panes must reject gracefully"
        );
    }

    // ── Additional edge case tests ────────────────────────────────────

    #[test]
    fn status_returns_none_for_unknown_id() {
        let reg = TestRegistry::new();
        let mut delivery = SuccessDelivery;
        let bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let status = bus.status(&MessageId::new()).expect("status");
        assert!(status.is_none());
    }

    #[test]
    fn drain_events_is_destructive() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        bus.publish(envelope).expect("publish");

        let first = bus.drain_events().expect("drain");
        assert!(!first.is_empty());

        let second = bus.drain_events().expect("drain again");
        assert!(second.is_empty(), "drain_events must be destructive");
    }

    #[test]
    fn publish_returns_accepted_not_business_output() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let result = bus.publish(envelope).expect("publish");

        match result {
            MessageAcceptance::Accepted { message_id } => {
                assert!(
                    !message_id.to_string().is_empty(),
                    "accepted message must have valid ID"
                );
            }
            other => panic!("expected Accepted, got {:?}", other),
        }
    }

    #[test]
    fn role_target_requires_message_receiver_capability() {
        let reg = TestRegistry::new();
        let mut desc = make_terminal_descriptor("ws", "%0");
        desc.role = Some(workhive_core::domain::PaneRole::Worker);
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Role(workhive_core::domain::PaneRole::Worker),
        );
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "terminal-only worker pane must not be routed by Role (no MessageReceiver)"
        );
    }

    #[test]
    fn role_target_routes_to_message_capable_worker() {
        let reg = TestRegistry::new();
        let mut desc = make_agent_descriptor("ws", "%0");
        desc.role = Some(workhive_core::domain::PaneRole::Worker);
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Role(workhive_core::domain::PaneRole::Worker),
        );
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(result, MessageAcceptance::Accepted { .. }),
            "agent worker with MessageReceiver must be routed"
        );
    }

    #[test]
    fn cross_workspace_pane_not_routed() {
        let reg = TestRegistry::new();
        reg.add(make_agent_descriptor("other-ws", "%0"));

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Pane(PaneRuntimeId::derive("g", "other-ws", "%0")),
        );
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "pane in different workspace must not be routed"
        );
    }

    #[test]
    fn multiple_failures_all_recorded() {
        let reg = TestRegistry::new();
        reg.add(make_agent_descriptor("ws", "%0"));
        reg.add(make_agent_descriptor("ws", "%1"));

        let mut delivery = FailingDelivery::new("transport error");
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Capability(PaneCapability::MessageReceiver),
        );
        bus.publish(envelope).expect("publish");

        let events = bus.drain_events().expect("drain");
        let failed_count = events
            .iter()
            .filter(|e| matches!(e, MessageEvent::Failed { .. }))
            .count();
        assert_eq!(
            failed_count, 2,
            "each failed delivery must produce its own Failed event"
        );
    }

    #[test]
    fn acknowledged_status_after_successful_delivery() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let msg_id = envelope.id.clone();
        bus.publish(envelope).expect("publish");

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(status, MessageStatus::Acknowledged);
    }

    /// Direct pane target to a terminal (no MessageReceiver) must be
    /// rejected with TargetNotCapable, not silently delivered.
    #[test]
    fn direct_pane_without_message_receiver_rejected_as_not_capable() {
        let reg = TestRegistry::new();
        let desc = make_terminal_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let result = bus.publish(envelope).expect("publish must not error");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::TargetNotCapable)
            ),
            "terminal pane without MessageReceiver must be rejected as TargetNotCapable"
        );
    }

    /// Direct pane target to an agent (has MessageReceiver) must succeed.
    #[test]
    fn direct_pane_with_message_receiver_accepted() {
        let reg = TestRegistry::new();
        let desc = make_agent_descriptor("ws", "%0");
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(result, MessageAcceptance::Accepted { .. }),
            "agent pane with MessageReceiver must be accepted"
        );
    }

    /// Direct pane target to an ObserverOnly pane must reject.
    #[test]
    fn direct_pane_observer_only_rejected_as_not_capable() {
        let reg = TestRegistry::new();
        let mut desc = make_terminal_descriptor("ws", "%0");
        desc.capabilities = [PaneCapability::ObserverOnly].into_iter().collect();
        let target_id = desc.runtime_id.clone();
        reg.add(desc);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let result = bus.publish(envelope).expect("publish must not error");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::TargetNotCapable)
            ),
            "ObserverOnly pane must be rejected as TargetNotCapable"
        );
    }

    /// Nonexistent pane still yields NoRouteForTarget (not TargetNotCapable).
    #[test]
    fn direct_pane_nonexistent_yields_no_route() {
        let reg = TestRegistry::new();
        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Pane(PaneRuntimeId::derive("g", "ws", "%99")),
        );
        let result = bus.publish(envelope).expect("publish must not error");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "nonexistent pane must yield NoRouteForTarget, not TargetNotCapable"
        );
    }

    #[test]
    fn broadcast_target_capability_filters_correctly() {
        let reg = TestRegistry::new();
        reg.add(make_agent_descriptor("ws", "%0"));
        reg.add(make_terminal_descriptor("ws", "%1"));

        let mut caps_with_observer: BTreeSet<PaneCapability> = BTreeSet::new();
        caps_with_observer.insert(PaneCapability::ObserverOnly);
        let mut observer = make_terminal_descriptor("ws", "%2");
        observer.capabilities = caps_with_observer;
        reg.add(observer);

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&reg, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Broadcast);
        bus.publish(envelope).expect("publish");

        let events = bus.drain_events().expect("drain");
        let routed = events
            .iter()
            .find(|e| matches!(e, MessageEvent::Routed { .. }));
        if let Some(MessageEvent::Routed { recipients, .. }) = routed {
            assert_eq!(
                recipients.len(),
                1,
                "only the agent pane with MessageReceiver should be a broadcast recipient"
            );
        }
    }
}

//! Integration tests for InMemoryMessageBus with InMemoryPaneRegistry.
//!
//! These tests exercise the full publish flow through production registry
//! and bus implementations, verifying that target resolution, delivery,
//! and event recording work end-to-end.

#[cfg(test)]
mod tests {
    use workhive_core::domain::{
        MessageAcceptance, MessageEvent, MessageFailureReason, MessageRejectReason, MessageStatus,
        MessageTarget, PaneCapability, PaneDescriptor, PaneKind, PaneLaunchOrigin,
        PaneMessagePayload, PaneRole, PaneRuntimeId, PaneRuntimeStatus, PaneState, UnitId,
        WorkspaceId,
    };
    use workhive_core::ports::{
        DeliveryAttempt, PaneMessageBusPort, PaneMessageDeliveryError, PaneMessageDeliveryPort,
        PaneRegistryPort,
    };

    use crate::message_bus::InMemoryMessageBus;
    use crate::pane_registry::InMemoryPaneRegistry;

    struct SuccessDelivery;
    impl PaneMessageDeliveryPort for SuccessDelivery {
        fn deliver(
            &mut self,
            target: &PaneRuntimeId,
            _payload: &PaneMessagePayload,
        ) -> Result<DeliveryAttempt, PaneMessageDeliveryError> {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: true,
                detail: String::new(),
            })
        }
    }

    struct FailDelivery;
    impl PaneMessageDeliveryPort for FailDelivery {
        fn deliver(
            &mut self,
            target: &PaneRuntimeId,
            _payload: &PaneMessagePayload,
        ) -> Result<DeliveryAttempt, PaneMessageDeliveryError> {
            Ok(DeliveryAttempt {
                target: target.clone(),
                success: false,
                detail: "simulated failure".to_owned(),
            })
        }
    }

    fn register_agent(registry: &InMemoryPaneRegistry, ws: &str, unit: &str) -> PaneRuntimeId {
        let id = PaneRuntimeId::derive("g", ws, unit);
        let desc = PaneDescriptor {
            runtime_id: id.clone(),
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
        };
        registry.upsert(desc).expect("upsert");
        id
    }

    fn register_terminal(registry: &InMemoryPaneRegistry, ws: &str, unit: &str) -> PaneRuntimeId {
        let id = PaneRuntimeId::derive("g", ws, unit);
        let desc = PaneDescriptor {
            runtime_id: id.clone(),
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
        };
        registry.upsert(desc).expect("upsert");
        id
    }

    fn make_envelope(ws: &str, target: MessageTarget) -> workhive_core::domain::MessageEnvelope {
        workhive_core::domain::MessageEnvelope {
            id: workhive_core::domain::MessageId::new(),
            source: None,
            workspace: WorkspaceId::new(ws).expect("valid ws"),
            target,
            payload: PaneMessagePayload::Text {
                content: "integration test".to_owned(),
            },
        }
    }

    #[test]
    fn integration_direct_message_through_production_registry() {
        let registry = InMemoryPaneRegistry::new();
        let target_id = register_agent(&registry, "ws", "%0");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let msg_id = envelope.id.clone();
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(status, MessageStatus::Acknowledged);

        let events = bus.drain_events().expect("drain");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MessageEvent::Queued { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MessageEvent::Routed { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MessageEvent::Dispatched { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, MessageEvent::Acknowledged { .. }))
        );
    }

    #[test]
    fn integration_broadcast_with_mixed_panes() {
        let registry = InMemoryPaneRegistry::new();
        register_agent(&registry, "ws", "%0");
        register_agent(&registry, "ws", "%1");
        register_terminal(&registry, "ws", "%2");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Broadcast);
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));

        let events = bus.drain_events().expect("drain");
        if let Some(MessageEvent::Routed { recipients, .. }) = events
            .iter()
            .find(|e| matches!(e, MessageEvent::Routed { .. }))
        {
            assert_eq!(recipients.len(), 2, "only MessageReceiver panes");
        }

        let ack_count = events
            .iter()
            .filter(|e| matches!(e, MessageEvent::Acknowledged { .. }))
            .count();
        assert_eq!(ack_count, 2, "both agents acknowledged");
    }

    #[test]
    fn integration_capability_routing_with_production_find_by_capability() {
        let registry = InMemoryPaneRegistry::new();
        register_agent(&registry, "ws", "%0");
        register_terminal(&registry, "ws", "%1");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope(
            "ws",
            MessageTarget::Capability(PaneCapability::MessageResponder),
        );
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));

        let events = bus.drain_events().expect("drain");
        if let Some(MessageEvent::Routed { recipients, .. }) = events
            .iter()
            .find(|e| matches!(e, MessageEvent::Routed { .. }))
        {
            assert_eq!(recipients.len(), 1, "only agent has MessageResponder");
        }
    }

    #[test]
    fn integration_role_routing() {
        let registry = InMemoryPaneRegistry::new();
        let agent_id = register_agent(&registry, "ws", "%0");

        // Assign Worker role to the agent
        let mut desc = registry.get(&agent_id).expect("get").expect("found");
        desc.role = Some(PaneRole::Worker);
        registry.upsert(desc).expect("upsert");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Role(PaneRole::Worker));
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));
    }

    #[test]
    fn integration_delivery_failure_records_events() {
        let registry = InMemoryPaneRegistry::new();
        let target_id = register_agent(&registry, "ws", "%0");

        let mut delivery = FailDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let msg_id = envelope.id.clone();
        bus.publish(envelope).expect("publish");

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(status, MessageStatus::Failed);

        let events = bus.drain_events().expect("drain");
        assert!(events.iter().any(|e| matches!(
            e,
            MessageEvent::Failed {
                reason: MessageFailureReason::DeliveryFailed { .. },
                ..
            }
        )));
    }

    #[test]
    fn integration_empty_workspace_broadcast_rejects() {
        let registry = InMemoryPaneRegistry::new();

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Broadcast);
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(
            result,
            MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
        ));
    }

    #[test]
    fn integration_timeout_after_successful_delivery() {
        let registry = InMemoryPaneRegistry::new();
        let target_id = register_agent(&registry, "ws", "%0");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(target_id));
        let msg_id = envelope.id.clone();
        bus.publish(envelope).expect("publish");

        bus.mark_timed_out(&msg_id);

        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(status, MessageStatus::TimedOut);
    }

    #[test]
    fn integration_direct_terminal_rejected_as_not_capable() {
        let registry = InMemoryPaneRegistry::new();
        let terminal_id = register_terminal(&registry, "ws", "%0");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(terminal_id));
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::TargetNotCapable)
            ),
            "direct message to terminal (no MessageReceiver) must reject as TargetNotCapable"
        );
    }

    #[test]
    fn integration_direct_agent_accepted_and_acknowledged() {
        let registry = InMemoryPaneRegistry::new();
        let agent_id = register_agent(&registry, "ws", "%0");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        let envelope = make_envelope("ws", MessageTarget::Pane(agent_id));
        let msg_id = envelope.id.clone();
        let result = bus.publish(envelope).expect("publish");

        assert!(matches!(result, MessageAcceptance::Accepted { .. }));
        let status = bus.status(&msg_id).expect("status").expect("found");
        assert_eq!(status, MessageStatus::Acknowledged);
    }

    #[test]
    fn integration_cross_workspace_isolation() {
        let registry = InMemoryPaneRegistry::new();
        let agent_id = register_agent(&registry, "ws-alpha", "%0");
        register_agent(&registry, "ws-beta", "%1");

        let mut delivery = SuccessDelivery;
        let mut bus = InMemoryMessageBus::new(&registry, &mut delivery);

        // Try to message agent in ws-alpha from ws-beta context
        let envelope = make_envelope("ws-beta", MessageTarget::Pane(agent_id));
        let result = bus.publish(envelope).expect("publish");

        assert!(
            matches!(
                result,
                MessageAcceptance::Rejected(MessageRejectReason::NoRouteForTarget)
            ),
            "cross-workspace direct messaging must be rejected"
        );
    }
}

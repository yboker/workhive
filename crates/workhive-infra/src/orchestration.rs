//! In-memory orchestration adapter.
//!
//! Implements `OrchestrationPort` by composing `PaneRegistryPort` and
//! `PaneMessageBusPort` with an in-memory state machine. All
//! orchestration state is per-workspace and does not survive process
//! restart.

use std::collections::HashMap;

use workhive_core::domain::{
    MessageAcceptance, OrchestrationPlan, OrchestrationRequest, OrchestrationState, WorkspaceId,
    WorkspaceInteractionMode, validate_transition,
};
use workhive_core::errors::OrchestrationError;
use workhive_core::ports::{OrchestrationPort, PaneMessageBusPort, PaneRegistryPort};
use workhive_core::usecases::orchestration::{
    build_orchestration_plan, execute_orchestration_request,
};

/// In-memory implementation of `OrchestrationPort`.
///
/// Wraps a pane registry, message bus, and per-workspace state machine.
/// The registry and bus are borrowed via trait references passed at
/// construction; the adapter itself only owns the state map.
pub struct InMemoryOrchestrationAdapter<'a> {
    registry: &'a dyn PaneRegistryPort,
    bus: &'a mut dyn PaneMessageBusPort,
    states: HashMap<String, OrchestrationState>,
}

impl<'a> InMemoryOrchestrationAdapter<'a> {
    /// Create a new adapter wrapping the given registry and bus.
    pub fn new(registry: &'a dyn PaneRegistryPort, bus: &'a mut dyn PaneMessageBusPort) -> Self {
        Self {
            registry,
            bus,
            states: HashMap::new(),
        }
    }

    fn mode_for_state(state: &OrchestrationState) -> WorkspaceInteractionMode {
        match state {
            OrchestrationState::Manual => WorkspaceInteractionMode::Manual,
            _ => WorkspaceInteractionMode::PrimaryAssisted,
        }
    }
}

impl OrchestrationPort for InMemoryOrchestrationAdapter<'_> {
    fn build_plan(&self, workspace: &WorkspaceId) -> Result<OrchestrationPlan, OrchestrationError> {
        let current_state = self.state(workspace);
        let mode = Self::mode_for_state(&current_state);
        build_orchestration_plan(self.registry, workspace, mode)
    }

    fn execute(
        &mut self,
        request: OrchestrationRequest,
    ) -> Result<MessageAcceptance, OrchestrationError> {
        let current_state = self.state(&request.workspace);

        match &current_state {
            OrchestrationState::Manual | OrchestrationState::DegradedToManual { .. } => {
                return Err(OrchestrationError::NotPrimaryAssisted {
                    workspace: request.workspace.to_string(),
                });
            }
            OrchestrationState::PrimarySetup => {
                return Err(OrchestrationError::NoPrimaryPane {
                    workspace: request.workspace.to_string(),
                });
            }
            OrchestrationState::PrimaryActive { .. } => {}
        }

        let mode = Self::mode_for_state(&current_state);
        let plan = build_orchestration_plan(self.registry, &request.workspace, mode)?;
        execute_orchestration_request(&request, &plan, self.bus)
    }

    fn state(&self, workspace: &WorkspaceId) -> OrchestrationState {
        self.states
            .get(workspace.as_str())
            .cloned()
            .unwrap_or_else(OrchestrationState::default)
    }

    fn transition(
        &mut self,
        workspace: &WorkspaceId,
        new_state: OrchestrationState,
    ) -> Result<(), OrchestrationError> {
        let current = self.state(workspace);
        validate_transition(&current, &new_state)?;
        self.states.insert(workspace.as_str().to_owned(), new_state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::{
        DegradationReason, OrchestrationIntent, PaneCapability, PaneDescriptor, PaneKind,
        PaneLaunchOrigin, PaneMessagePayload, PaneRole, PaneRuntimeId, PaneRuntimeStatus,
        PaneState, UnitId,
    };
    use workhive_core::ports::OrchestrationPort;
    use workhive_testkit::fakes::{FakeMessageBus, FakePaneRegistry};

    fn test_workspace_id() -> WorkspaceId {
        WorkspaceId::new("test-ws").expect("valid workspace id")
    }

    fn make_descriptor(
        unit: &str,
        role: Option<PaneRole>,
        capabilities: std::collections::BTreeSet<PaneCapability>,
    ) -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive("srv", "test-ws", unit),
            group: "srv".to_owned(),
            workspace: "test-ws".to_owned(),
            unit_id: UnitId::new(unit),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            observed_status: PaneRuntimeStatus::Unknown,
            role,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities,
        }
    }

    fn agent_capabilities() -> std::collections::BTreeSet<PaneCapability> {
        [
            PaneCapability::CommandExecutor,
            PaneCapability::MessageReceiver,
            PaneCapability::MessageResponder,
        ]
        .into_iter()
        .collect()
    }

    fn worker_capabilities() -> std::collections::BTreeSet<PaneCapability> {
        [
            PaneCapability::MessageReceiver,
            PaneCapability::CommandExecutor,
        ]
        .into_iter()
        .collect()
    }

    // ── I1: manual workspace no primary fully valid ──────────────────

    #[test]
    fn manual_workspace_no_primary_fully_valid() {
        let registry = FakePaneRegistry::new();
        registry
            .upsert(make_descriptor(
                "%0",
                None,
                [PaneCapability::CommandExecutor].into_iter().collect(),
            ))
            .expect("upsert");

        let mut bus = FakeMessageBus::new();
        let adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);

        let ws = test_workspace_id();
        assert_eq!(adapter.state(&ws), OrchestrationState::Manual);

        let plan = adapter.build_plan(&ws).expect("build_plan");
        assert_eq!(plan.primary, None);
        assert!(plan.workers.is_empty());
    }

    // ── I2: primary assisted end to end ──────────────────────────────

    #[test]
    fn primary_assisted_end_to_end() {
        let registry = FakePaneRegistry::new();
        let primary_id = PaneRuntimeId::derive("srv", "test-ws", "%0");
        let worker_id = PaneRuntimeId::derive("srv", "test-ws", "%1");

        registry
            .upsert(make_descriptor(
                "%0",
                Some(PaneRole::Primary),
                agent_capabilities(),
            ))
            .expect("upsert primary");
        registry
            .upsert(make_descriptor(
                "%1",
                Some(PaneRole::Worker),
                worker_capabilities(),
            ))
            .expect("upsert worker");

        let mut bus = FakeMessageBus::new();
        let mut adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);

        let ws = test_workspace_id();

        // Transition: Manual -> PrimarySetup
        adapter
            .transition(&ws, OrchestrationState::PrimarySetup)
            .expect("transition to setup");

        // Transition: PrimarySetup -> PrimaryActive
        adapter
            .transition(
                &ws,
                OrchestrationState::PrimaryActive {
                    primary: primary_id.clone(),
                },
            )
            .expect("transition to active");

        assert!(matches!(
            adapter.state(&ws),
            OrchestrationState::PrimaryActive { .. }
        ));

        // Execute a delegation
        let request = OrchestrationRequest {
            workspace: ws.clone(),
            intent: OrchestrationIntent::DelegateTask,
            payload: PaneMessagePayload::Command {
                name: "run-tests".to_owned(),
                args: vec![],
            },
            target_worker: Some(worker_id),
        };

        let result = adapter.execute(request).expect("execute should succeed");
        assert!(matches!(result, MessageAcceptance::Accepted { .. }));
    }

    // ── I3: primary failure degrades gracefully ──────────────────────

    #[test]
    fn primary_failure_degrades_gracefully() {
        let registry = FakePaneRegistry::new();
        let primary_id = PaneRuntimeId::derive("srv", "test-ws", "%0");

        registry
            .upsert(make_descriptor(
                "%0",
                Some(PaneRole::Primary),
                agent_capabilities(),
            ))
            .expect("upsert primary");

        let mut bus = FakeMessageBus::new();
        let mut adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);
        let ws = test_workspace_id();

        // Setup -> Active
        adapter
            .transition(&ws, OrchestrationState::PrimarySetup)
            .expect("setup");
        adapter
            .transition(
                &ws,
                OrchestrationState::PrimaryActive {
                    primary: primary_id.clone(),
                },
            )
            .expect("active");

        // Remove primary from registry
        registry.remove(&primary_id).expect("remove primary");

        // Degrade to manual
        adapter
            .transition(
                &ws,
                OrchestrationState::DegradedToManual {
                    reason: DegradationReason::PrimaryPaneLost,
                },
            )
            .expect("degrade");

        assert!(matches!(
            adapter.state(&ws),
            OrchestrationState::DegradedToManual { .. }
        ));

        // Attempt execute should fail
        let request = OrchestrationRequest {
            workspace: ws.clone(),
            intent: OrchestrationIntent::DelegateTask,
            payload: PaneMessagePayload::Text {
                content: "task".to_owned(),
            },
            target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%1")),
        };

        let result = adapter.execute(request);
        assert!(matches!(
            result,
            Err(OrchestrationError::NotPrimaryAssisted { .. })
        ));
    }

    // ── I4: orchestration through bus not adapter ─────────────────────

    #[test]
    fn orchestration_request_through_bus_not_adapter() {
        let registry = FakePaneRegistry::new();
        let worker_id = PaneRuntimeId::derive("srv", "test-ws", "%1");

        registry
            .upsert(make_descriptor(
                "%0",
                Some(PaneRole::Primary),
                agent_capabilities(),
            ))
            .expect("upsert");
        registry
            .upsert(make_descriptor(
                "%1",
                Some(PaneRole::Worker),
                worker_capabilities(),
            ))
            .expect("upsert");

        let mut bus = FakeMessageBus::new();
        let mut adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);
        let ws = test_workspace_id();

        adapter
            .transition(&ws, OrchestrationState::PrimarySetup)
            .expect("setup");
        adapter
            .transition(
                &ws,
                OrchestrationState::PrimaryActive {
                    primary: PaneRuntimeId::derive("srv", "test-ws", "%0"),
                },
            )
            .expect("active");

        let request = OrchestrationRequest {
            workspace: ws,
            intent: OrchestrationIntent::DelegateTask,
            payload: PaneMessagePayload::Text {
                content: "work".to_owned(),
            },
            target_worker: Some(worker_id),
        };

        adapter.execute(request).expect("execute");

        // Verify bus received the message
        assert_eq!(bus.published().len(), 1);
    }

    // ── I5: mode reporting truthful ──────────────────────────────────

    #[test]
    fn mode_reporting_truthful() {
        let registry = FakePaneRegistry::new();
        let mut bus = FakeMessageBus::new();
        let mut adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);
        let ws = test_workspace_id();

        // Default: Manual
        assert_eq!(adapter.state(&ws), OrchestrationState::Manual);

        // After setup request
        adapter
            .transition(&ws, OrchestrationState::PrimarySetup)
            .expect("setup");
        assert_eq!(adapter.state(&ws), OrchestrationState::PrimarySetup);

        // After degradation
        adapter
            .transition(
                &ws,
                OrchestrationState::DegradedToManual {
                    reason: DegradationReason::NoPrimaryPaneFound,
                },
            )
            .expect("degrade");
        assert!(matches!(
            adapter.state(&ws),
            OrchestrationState::DegradedToManual {
                reason: DegradationReason::NoPrimaryPaneFound,
            }
        ));

        // Re-request
        adapter
            .transition(&ws, OrchestrationState::PrimarySetup)
            .expect("re-setup");
        assert_eq!(adapter.state(&ws), OrchestrationState::PrimarySetup);
    }

    // ── Forbidden transitions ────────────────────────────────────────

    #[test]
    fn forbidden_manual_to_active() {
        let registry = FakePaneRegistry::new();
        let mut bus = FakeMessageBus::new();
        let mut adapter = InMemoryOrchestrationAdapter::new(&registry, &mut bus);
        let ws = test_workspace_id();

        let result = adapter.transition(
            &ws,
            OrchestrationState::PrimaryActive {
                primary: PaneRuntimeId::derive("srv", "test-ws", "%0"),
            },
        );
        assert!(result.is_err());
    }
}

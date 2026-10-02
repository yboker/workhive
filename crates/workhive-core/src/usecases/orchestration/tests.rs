//! Orchestration use-case tests.
//!
//! Moved verbatim from the original `usecases/orchestration.rs` as part
//! of the directory split. No assertions or
//! fixtures were modified; the only changes are the `use super::*;` /
//! `use crate::domain::*;` paths required by the new module location.

use super::*;
use crate::domain::{
    PaneCapability, PaneDescriptor, PaneKind, PaneLaunchOrigin, PaneMessagePayload, PaneRuntimeId,
    PaneRuntimeStatus, PaneState, UnitId, WorkspaceInteractionMode,
};
use std::cell::RefCell;
use std::collections::BTreeSet;

fn test_workspace_id() -> WorkspaceId {
    WorkspaceId::new("test-ws").expect("valid workspace id")
}

fn make_descriptor(
    unit: &str,
    role: Option<PaneRole>,
    capabilities: BTreeSet<PaneCapability>,
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

fn agent_capabilities() -> BTreeSet<PaneCapability> {
    [
        PaneCapability::CommandExecutor,
        PaneCapability::MessageReceiver,
        PaneCapability::MessageResponder,
    ]
    .into_iter()
    .collect()
}

fn worker_capabilities() -> BTreeSet<PaneCapability> {
    [
        PaneCapability::MessageReceiver,
        PaneCapability::CommandExecutor,
    ]
    .into_iter()
    .collect()
}

/// Minimal in-test fake registry (not the testkit one, to avoid
/// circular dependency).
struct TestRegistry {
    panes: RefCell<Vec<PaneDescriptor>>,
}

impl TestRegistry {
    fn new(panes: Vec<PaneDescriptor>) -> Self {
        Self {
            panes: RefCell::new(panes),
        }
    }
}

impl PaneRegistryPort for TestRegistry {
    fn upsert(
        &self,
        _descriptor: PaneDescriptor,
    ) -> Result<crate::ports::RegistryDelta, crate::ports::RegistryError> {
        unimplemented!("not used in orchestration tests")
    }

    fn remove(
        &self,
        _id: &PaneRuntimeId,
    ) -> Result<Option<PaneDescriptor>, crate::ports::RegistryError> {
        unimplemented!("not used in orchestration tests")
    }

    fn get(
        &self,
        id: &PaneRuntimeId,
    ) -> Result<Option<PaneDescriptor>, crate::ports::RegistryError> {
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
    ) -> Result<Vec<PaneDescriptor>, crate::ports::RegistryError> {
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
        workspace: Option<&str>,
    ) -> Result<Vec<PaneDescriptor>, crate::ports::RegistryError> {
        match workspace {
            Some(ws) => self.list_workspace(ws),
            None => Ok(self.panes.borrow().clone()),
        }
    }

    fn drain_events(
        &self,
    ) -> Result<Vec<crate::domain::PaneLifecycleEvent>, crate::ports::RegistryError> {
        Ok(vec![])
    }

    fn update_from_observation(
        &self,
        _observed: crate::domain::ObservedPaneState,
    ) -> Result<Vec<crate::domain::PaneStateChange>, crate::ports::RegistryError> {
        unimplemented!("not used in orchestration tests")
    }

    fn find_by_capability(
        &self,
        capability: PaneCapability,
    ) -> Result<Vec<PaneDescriptor>, crate::ports::RegistryError> {
        Ok(self
            .panes
            .borrow()
            .iter()
            .filter(|p| p.capabilities.contains(&capability))
            .cloned()
            .collect())
    }
}

/// Minimal in-test fake bus.
struct TestBus {
    published: RefCell<Vec<MessageEnvelope>>,
    reject_all: bool,
}

impl TestBus {
    fn new() -> Self {
        Self {
            published: RefCell::new(Vec::new()),
            reject_all: false,
        }
    }

    fn rejecting() -> Self {
        Self {
            published: RefCell::new(Vec::new()),
            reject_all: true,
        }
    }

    fn published(&self) -> Vec<MessageEnvelope> {
        self.published.borrow().clone()
    }
}

impl PaneMessageBusPort for TestBus {
    fn publish(
        &mut self,
        envelope: MessageEnvelope,
    ) -> Result<MessageAcceptance, crate::ports::PaneMessageBusError> {
        if self.reject_all {
            return Ok(MessageAcceptance::Rejected(
                crate::domain::MessageRejectReason::NoRouteForTarget,
            ));
        }
        let id = envelope.id.clone();
        self.published.borrow_mut().push(envelope);
        Ok(MessageAcceptance::Accepted { message_id: id })
    }

    fn status(
        &self,
        _message_id: &MessageId,
    ) -> Result<Option<crate::domain::MessageStatus>, crate::ports::PaneMessageBusError> {
        Ok(None)
    }

    fn drain_events(
        &mut self,
    ) -> Result<Vec<crate::domain::MessageEvent>, crate::ports::PaneMessageBusError> {
        Ok(vec![])
    }
}

// ── P1: build_plan manual workspace ──────────────────────────────

#[test]
fn build_plan_manual_workspace() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor("%1", Some(PaneRole::Worker), worker_capabilities()),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::Manual,
    )
    .expect("build_plan should succeed");

    assert_eq!(plan.primary, None);
    assert!(plan.workers.is_empty());
}

// ── P2: build_plan primary_assisted selects declared primary ─────

#[test]
fn build_plan_primary_assisted_selects_declared_primary() {
    let primary_id = PaneRuntimeId::derive("srv", "test-ws", "%0");
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor("%1", Some(PaneRole::Worker), worker_capabilities()),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    assert_eq!(plan.primary, Some(primary_id));
    assert_eq!(plan.workers.len(), 1);
}

// ── P3: build_plan workers filtered by role and capability ───────

#[test]
fn build_plan_workers_filtered_by_role_and_capability() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor("%1", Some(PaneRole::Worker), worker_capabilities()),
        make_descriptor(
            "%2",
            Some(PaneRole::Utility),
            [PaneCapability::MessageReceiver].into_iter().collect(),
        ),
        make_descriptor(
            "%3",
            Some(PaneRole::Worker),
            [PaneCapability::CommandExecutor].into_iter().collect(),
        ),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    // Only %1 is Worker + MessageReceiver. %2 is Utility, %3 lacks MessageReceiver.
    assert_eq!(plan.workers.len(), 1);
    assert_eq!(
        plan.workers[0],
        PaneRuntimeId::derive("srv", "test-ws", "%1")
    );
}

// ── P4: build_plan workers deterministic order ───────────────────

#[test]
fn build_plan_workers_deterministic_order() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor("%2", Some(PaneRole::Worker), worker_capabilities()),
        make_descriptor("%1", Some(PaneRole::Worker), worker_capabilities()),
        make_descriptor("%3", Some(PaneRole::Worker), worker_capabilities()),
    ]);
    let plan_a = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    let plan_b = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    assert_eq!(plan_a.workers, plan_b.workers);
    // Verify sorted order
    for i in 1..plan_a.workers.len() {
        assert!(plan_a.workers[i - 1].as_str() <= plan_a.workers[i].as_str());
    }
}

// ── P5: worker selection by capability ───────────────────────────

#[test]
fn worker_selection_by_capability() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor(
            "%1",
            Some(PaneRole::Worker),
            [PaneCapability::MessageReceiver].into_iter().collect(),
        ),
        make_descriptor("%2", Some(PaneRole::Worker), worker_capabilities()),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    let criteria = WorkerSelectionCriteria {
        required_capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
        required_role: PaneRole::Worker,
    };

    let selected = select_workers(&plan, &criteria, &registry).expect("selection should work");
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0], PaneRuntimeId::derive("srv", "test-ws", "%2"));
}

// ── P6: worker selection no match returns empty ──────────────────

#[test]
fn worker_selection_no_match_returns_empty() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor(
            "%1",
            Some(PaneRole::Worker),
            [PaneCapability::MessageReceiver].into_iter().collect(),
        ),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    let criteria = WorkerSelectionCriteria {
        required_capabilities: [PaneCapability::MessageResponder].into_iter().collect(),
        required_role: PaneRole::Worker,
    };

    let selected = select_workers(&plan, &criteria, &registry).expect("selection should work");
    assert!(selected.is_empty());
}

// ── P7: non-message-capable pane excluded from workers ───────────

#[test]
fn non_message_capable_pane_excluded_from_workers() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor(
            "%1",
            Some(PaneRole::Worker),
            [PaneCapability::Tui].into_iter().collect(),
        ),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    assert!(plan.workers.is_empty());
}

// ── E1: execute delegate publishes targeted message ──────────────

#[test]
fn execute_delegate_publishes_targeted_message() {
    let worker_id = PaneRuntimeId::derive("srv", "test-ws", "%1");
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![worker_id.clone()],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Command {
            name: "run-tests".to_owned(),
            args: vec![],
        },
        target_worker: Some(worker_id.clone()),
    };

    let mut bus = TestBus::new();
    let result =
        execute_orchestration_request(&request, &plan, &mut bus).expect("execute should work");

    assert!(matches!(result, MessageAcceptance::Accepted { .. }));
    let published = bus.published();
    assert_eq!(published.len(), 1);
    assert!(matches!(
        published[0].target,
        MessageTarget::Pane(ref id) if *id == worker_id
    ));
}

// ── E2: execute broadcast publishes to role ──────────────────────

#[test]
fn execute_broadcast_publishes_to_role() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![PaneRuntimeId::derive("srv", "test-ws", "%1")],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::BroadcastContext,
        payload: PaneMessagePayload::Text {
            content: "new context".to_owned(),
        },
        target_worker: None,
    };

    let mut bus = TestBus::new();
    let result =
        execute_orchestration_request(&request, &plan, &mut bus).expect("execute should work");

    assert!(matches!(result, MessageAcceptance::Accepted { .. }));
    let published = bus.published();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].target, MessageTarget::Role(PaneRole::Worker));
}

// ── E3: execute rejected in manual mode ──────────────────────────

#[test]
fn execute_rejected_in_manual_mode() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::Manual,
        primary: None,
        workers: vec![],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%1")),
    };

    let mut bus = TestBus::new();
    let result = execute_orchestration_request(&request, &plan, &mut bus);

    assert!(matches!(
        result,
        Err(OrchestrationError::NotPrimaryAssisted { .. })
    ));
}

// ── E4: execute rejected when degraded ───────────────────────────
// (DegradedToManual is modeled in the state machine, not the plan.
//  In plan terms, a degraded workspace has mode=PrimaryAssisted but
//  primary=None, which triggers NoPrimaryPane.)

#[test]
fn execute_rejected_when_no_primary() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: None,
        workers: vec![],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%1")),
    };

    let mut bus = TestBus::new();
    let result = execute_orchestration_request(&request, &plan, &mut bus);

    assert!(matches!(
        result,
        Err(OrchestrationError::NoPrimaryPane { .. })
    ));
}

// ── E5: execute rejected for nonexistent worker ──────────────────

#[test]
fn execute_rejected_for_nonexistent_worker() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![PaneRuntimeId::derive("srv", "test-ws", "%1")],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%99")),
    };

    let mut bus = TestBus::new();
    let result = execute_orchestration_request(&request, &plan, &mut bus);

    assert!(matches!(
        result,
        Err(OrchestrationError::WorkerNotFound { .. })
    ));
}

// ── E6: execute surfaces bus rejection ───────────────────────────

#[test]
fn execute_surfaces_bus_rejection() {
    let worker_id = PaneRuntimeId::derive("srv", "test-ws", "%1");
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![worker_id.clone()],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: Some(worker_id),
    };

    let mut bus = TestBus::rejecting();
    let result =
        execute_orchestration_request(&request, &plan, &mut bus).expect("should not error");

    assert!(matches!(result, MessageAcceptance::Rejected(..)));
}

// ── Validation: target_worker required ───────────────────────────

#[test]
fn validate_request_delegate_requires_target() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: None,
    };

    let result = validate_orchestration_request(&request, &plan);
    assert!(matches!(
        result,
        Err(OrchestrationError::TargetWorkerRequired { .. })
    ));
}

// ── Validation: broadcast rejects target ─────────────────────────

#[test]
fn validate_request_broadcast_rejects_target() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![PaneRuntimeId::derive("srv", "test-ws", "%1")],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::BroadcastContext,
        payload: PaneMessagePayload::Text {
            content: "context".to_owned(),
        },
        target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%1")),
    };

    let result = validate_orchestration_request(&request, &plan);
    assert!(matches!(
        result,
        Err(OrchestrationError::TargetWorkerForbidden { .. })
    ));
}

// ── Routing depends on capability, not tool name ──────────────────

#[test]
fn routing_uses_capability_not_tool_name() {
    let custom_agent_caps: BTreeSet<PaneCapability> = [
        PaneCapability::CommandExecutor,
        PaneCapability::MessageReceiver,
        PaneCapability::MessageResponder,
    ]
    .into_iter()
    .collect();

    let mut desc = make_descriptor("%1", Some(PaneRole::Worker), custom_agent_caps);
    desc.command = "my-completely-custom-agent".to_owned();

    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        desc,
    ]);

    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    assert_eq!(
        plan.workers.len(),
        1,
        "worker with correct capabilities must be included regardless of command name"
    );
}

#[test]
fn routing_excludes_pane_lacking_capability_regardless_of_name() {
    let mut desc = make_descriptor(
        "%1",
        Some(PaneRole::Worker),
        [PaneCapability::Tui].into_iter().collect(),
    );
    desc.command = "claude".to_owned();

    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        desc,
    ]);

    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    assert!(
        plan.workers.is_empty(),
        "pane named 'claude' but with only Tui capability must not be a worker"
    );
}

// ── WorkerSelectionCriteria filters by capability ──────────────────

#[test]
fn worker_selection_criteria_filters_by_multiple_capabilities() {
    let registry = TestRegistry::new(vec![
        make_descriptor("%0", Some(PaneRole::Primary), agent_capabilities()),
        make_descriptor("%1", Some(PaneRole::Worker), agent_capabilities()),
        make_descriptor("%2", Some(PaneRole::Worker), worker_capabilities()),
    ]);
    let plan = build_orchestration_plan(
        &registry,
        &test_workspace_id(),
        WorkspaceInteractionMode::PrimaryAssisted,
    )
    .expect("build_plan should succeed");

    let criteria = WorkerSelectionCriteria {
        required_capabilities: [
            PaneCapability::MessageReceiver,
            PaneCapability::MessageResponder,
        ]
        .into_iter()
        .collect(),
        required_role: PaneRole::Worker,
    };

    let selected = select_workers(&plan, &criteria, &registry).expect("selection should work");
    assert_eq!(
        selected.len(),
        1,
        "only %1 has both MessageReceiver and MessageResponder"
    );
    assert_eq!(selected[0], PaneRuntimeId::derive("srv", "test-ws", "%1"));
}

// ── No is_ai field in orchestration types ─────────────────────────

#[test]
fn no_is_ai_field_in_orchestration_types() {
    let plan = OrchestrationPlan {
        workspace: test_workspace_id(),
        mode: WorkspaceInteractionMode::PrimaryAssisted,
        primary: Some(PaneRuntimeId::derive("srv", "test-ws", "%0")),
        workers: vec![PaneRuntimeId::derive("srv", "test-ws", "%1")],
    };

    let request = OrchestrationRequest {
        workspace: test_workspace_id(),
        intent: OrchestrationIntent::DelegateTask,
        payload: PaneMessagePayload::Text {
            content: "task".to_owned(),
        },
        target_worker: Some(PaneRuntimeId::derive("srv", "test-ws", "%1")),
    };

    let criteria = WorkerSelectionCriteria {
        required_capabilities: agent_capabilities(),
        required_role: PaneRole::Worker,
    };

    assert!(
        !format!("{plan:?}").contains("is_ai"),
        "OrchestrationPlan must not contain is_ai"
    );
    assert!(
        !format!("{request:?}").contains("is_ai"),
        "OrchestrationRequest must not contain is_ai"
    );
    assert!(
        !format!("{criteria:?}").contains("is_ai"),
        "WorkerSelectionCriteria must not contain is_ai"
    );
}

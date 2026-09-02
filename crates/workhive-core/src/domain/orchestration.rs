//! Orchestration domain types for primary-pane coordination.
//!
//! These types define the vocabulary for optional primary-assisted mode,
//! where a designated primary pane can coordinate work across worker
//! panes in a workspace.
//!
//! Key invariant: `Manual` mode is the default. Orchestration is opt-in
//! and gracefully degrades to manual mode on failure.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::pane::{
    PaneCapability, PaneMessagePayload, PaneRole, PaneRuntimeId, WorkspaceInteractionMode,
};
use super::workspace_id::WorkspaceId;

/// Snapshot of orchestration topology within a single workspace.
///
/// This is a derived view, not a stored entity. It is rebuilt from
/// registry state each time orchestration logic runs.
///
/// ## Invariants
///
/// - If `mode == Manual`, then `primary` MUST be `None` and `workers`
///   MUST be empty.
/// - If `mode == PrimaryAssisted` and `primary` is `Some`, the pane
///   MUST exist in the registry with `role == Some(PaneRole::Primary)`.
/// - `workers` contains only panes with `role == Some(PaneRole::Worker)`
///   that have `PaneCapability::MessageReceiver`.
/// - `workers` ordering is deterministic (sorted by `PaneRuntimeId`).
#[derive(Debug, Clone)]
pub struct OrchestrationPlan {
    /// The workspace this plan applies to.
    pub workspace: WorkspaceId,
    /// Active interaction mode.
    pub mode: WorkspaceInteractionMode,
    /// The pane currently acting as primary, if any.
    pub primary: Option<PaneRuntimeId>,
    /// Panes available to receive delegated work.
    pub workers: Vec<PaneRuntimeId>,
}

/// High-level coordination action requested through orchestration.
///
/// Intents are the orchestration layer's vocabulary. They are
/// translated into `MessageEnvelope` values for delivery via the bus.
///
/// Each intent maps to a specific message-bus routing pattern:
/// - `DelegateTask` → targeted message to a specific worker
/// - `RequestStatus` → targeted message to a specific worker
/// - `BroadcastContext` → role-based delivery to all workers
/// - `InterruptTask` → targeted message to a specific worker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrchestrationIntent {
    /// Delegate a task to a specific worker.
    DelegateTask,
    /// Request status/progress from a worker.
    RequestStatus,
    /// Broadcast contextual information to all workers.
    BroadcastContext,
    /// Interrupt an in-progress task on a worker.
    InterruptTask,
}

/// A request to perform an orchestration action.
///
/// Combines workspace identity, intent, and message payload.
/// The orchestration use case validates the request against
/// the current `OrchestrationPlan` before publishing to the bus.
///
/// ## Invariants
///
/// - For `DelegateTask`, `RequestStatus`, and `InterruptTask`:
///   `target_worker` MUST be `Some`.
/// - For `BroadcastContext`: `target_worker` MUST be `None`.
#[derive(Debug, Clone)]
pub struct OrchestrationRequest {
    /// Target workspace.
    pub workspace: WorkspaceId,
    /// What kind of coordination action.
    pub intent: OrchestrationIntent,
    /// The message content to deliver.
    pub payload: PaneMessagePayload,
    /// Explicit target worker, if applicable.
    pub target_worker: Option<PaneRuntimeId>,
}

/// Runtime state of orchestration for a single workspace.
///
/// Tracks state machine transitions. Lives in memory only —
/// does not survive process restart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum OrchestrationState {
    /// No orchestration active. Default state.
    #[default]
    Manual,
    /// Primary-assisted mode requested; searching for primary pane.
    PrimarySetup,
    /// Primary pane found and active; orchestration is operational.
    PrimaryActive { primary: PaneRuntimeId },
    /// Primary pane was lost; workspace remains usable in manual mode.
    DegradedToManual { reason: DegradationReason },
}

/// Why orchestration fell back to manual mode.
///
/// Every `DegradedToManual` state carries a typed reason — no silent failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DegradationReason {
    /// No pane with `PaneRole::Primary` found in the workspace.
    NoPrimaryPaneFound,
    /// Primary pane was removed from the registry (closed/crashed).
    PrimaryPaneLost,
    /// Primary pane lost required capabilities.
    PrimaryCapabilityLost,
    /// Message bus delivery to primary failed.
    PrimaryUnreachable,
}

impl std::fmt::Display for DegradationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoPrimaryPaneFound => write!(f, "no primary pane found"),
            Self::PrimaryPaneLost => write!(f, "primary pane was lost"),
            Self::PrimaryCapabilityLost => write!(f, "primary pane lost required capabilities"),
            Self::PrimaryUnreachable => write!(f, "primary pane is unreachable"),
        }
    }
}

/// Criteria for selecting worker panes from the registry.
///
/// Workers must have ALL capabilities in `required_capabilities` and
/// match the `required_role`. Selection is deterministic: workers are
/// sorted by `PaneRuntimeId`.
#[derive(Debug, Clone)]
pub struct WorkerSelectionCriteria {
    /// Required capabilities for the worker.
    pub required_capabilities: BTreeSet<PaneCapability>,
    /// Required role (always `Worker` for Phase 8).
    pub required_role: PaneRole,
}

// ── State Machine Transition Validation ──────────────────────────────

/// Validate an orchestration state transition.
///
/// This is a pure function that enforces the state machine rules:
///
/// | From | To | Allowed |
/// |------|----|---------|
/// | `Manual` | `PrimarySetup` | yes |
/// | `PrimarySetup` | `PrimaryActive` | yes |
/// | `PrimarySetup` | `DegradedToManual` | yes |
/// | `PrimaryActive` | `DegradedToManual` | yes |
/// | `DegradedToManual` | `PrimarySetup` | yes |
/// | `Manual` | `PrimaryActive` | **forbidden** |
/// | `DegradedToManual` | `PrimaryActive` | **forbidden** |
pub fn validate_transition(
    from: &OrchestrationState,
    to: &OrchestrationState,
) -> Result<(), InvalidTransition> {
    match (from, to) {
        // Manual -> PrimarySetup: user requests PrimaryAssisted
        (OrchestrationState::Manual, OrchestrationState::PrimarySetup) => Ok(()),

        // PrimarySetup -> PrimaryActive: primary found + capable
        (OrchestrationState::PrimarySetup, OrchestrationState::PrimaryActive { .. }) => Ok(()),

        // PrimarySetup -> DegradedToManual: no primary found
        (OrchestrationState::PrimarySetup, OrchestrationState::DegradedToManual { .. }) => Ok(()),

        // PrimaryActive -> DegradedToManual: primary lost/caps lost/unreachable
        (OrchestrationState::PrimaryActive { .. }, OrchestrationState::DegradedToManual { .. }) => {
            Ok(())
        }

        // DegradedToManual -> PrimarySetup: user re-requests
        (OrchestrationState::DegradedToManual { .. }, OrchestrationState::PrimarySetup) => Ok(()),

        // All other transitions are forbidden
        _ => Err(InvalidTransition {
            from: format!("{from:?}"),
            to: format!("{to:?}"),
        }),
    }
}

/// A state machine transition that violates the orchestration rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    /// Debug representation of the source state.
    pub from: String,
    /// Debug representation of the target state.
    pub to: String,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid orchestration state transition: {} -> {}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidTransition {}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_workspace_id() -> WorkspaceId {
        WorkspaceId::new("test-ws").expect("valid workspace id")
    }

    fn test_runtime_id(unit: &str) -> PaneRuntimeId {
        PaneRuntimeId::derive("srv", "test-ws", unit)
    }

    // ── U1: OrchestrationPlan Manual has no primary ──────────────────

    #[test]
    fn orchestration_plan_manual_has_no_primary() {
        let plan = OrchestrationPlan {
            workspace: test_workspace_id(),
            mode: WorkspaceInteractionMode::Manual,
            primary: None,
            workers: vec![],
        };
        assert_eq!(plan.primary, None);
        assert!(plan.workers.is_empty());
    }

    // ── U2: OrchestrationPlan PrimaryAssisted with primary ───────────

    #[test]
    fn orchestration_plan_primary_assisted_with_primary() {
        let primary_id = test_runtime_id("%0");
        let worker_id = test_runtime_id("%1");
        let plan = OrchestrationPlan {
            workspace: test_workspace_id(),
            mode: WorkspaceInteractionMode::PrimaryAssisted,
            primary: Some(primary_id.clone()),
            workers: vec![worker_id],
        };
        assert_eq!(plan.primary, Some(primary_id));
        assert_eq!(plan.workers.len(), 1);
    }

    // ── U3: OrchestrationIntent serde round-trip ─────────────────────

    #[test]
    fn orchestration_intent_serde_round_trip() {
        let intents = [
            OrchestrationIntent::DelegateTask,
            OrchestrationIntent::RequestStatus,
            OrchestrationIntent::BroadcastContext,
            OrchestrationIntent::InterruptTask,
        ];
        for intent in &intents {
            let json = serde_json::to_string(intent).expect("serialize");
            let restored: OrchestrationIntent = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*intent, restored);
        }
    }

    // ── U4: OrchestrationRequest DelegateTask requires target ────────

    #[test]
    fn orchestration_request_delegate_requires_target() {
        let request = OrchestrationRequest {
            workspace: test_workspace_id(),
            intent: OrchestrationIntent::DelegateTask,
            payload: PaneMessagePayload::Text {
                content: "do work".to_owned(),
            },
            target_worker: None,
        };
        // The request itself can be constructed, but validation
        // (in use case layer) will reject this combination.
        assert!(request.target_worker.is_none());
        assert_eq!(request.intent, OrchestrationIntent::DelegateTask);
    }

    // ── U5: OrchestrationRequest BroadcastContext rejects target ─────

    #[test]
    fn orchestration_request_broadcast_rejects_target() {
        let request = OrchestrationRequest {
            workspace: test_workspace_id(),
            intent: OrchestrationIntent::BroadcastContext,
            payload: PaneMessagePayload::Text {
                content: "context update".to_owned(),
            },
            target_worker: Some(test_runtime_id("%1")),
        };
        // The request itself can be constructed, but validation
        // (in use case layer) will reject this combination.
        assert!(request.target_worker.is_some());
        assert_eq!(request.intent, OrchestrationIntent::BroadcastContext);
    }

    // ── U6: DegradationReason is typed (exhaustive match) ────────────

    #[test]
    fn degradation_reason_is_typed() {
        let reasons = [
            DegradationReason::NoPrimaryPaneFound,
            DegradationReason::PrimaryPaneLost,
            DegradationReason::PrimaryCapabilityLost,
            DegradationReason::PrimaryUnreachable,
        ];
        for reason in &reasons {
            let display = reason.to_string();
            assert!(
                !display.is_empty(),
                "every degradation reason must have a display"
            );
        }
    }

    // ── U7: OrchestrationError carries context ──────────────────────
    // (tested in errors.rs since OrchestrationError lives there)

    // ── S1: Initial state is Manual ──────────────────────────────────

    #[test]
    fn initial_state_is_manual() {
        let state = OrchestrationState::default();
        assert_eq!(state, OrchestrationState::Manual);
    }

    // ── S2: Manual -> PrimarySetup ───────────────────────────────────

    #[test]
    fn manual_to_setup_on_primary_request() {
        let from = OrchestrationState::Manual;
        let to = OrchestrationState::PrimarySetup;
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S3: PrimarySetup -> PrimaryActive ────────────────────────────

    #[test]
    fn setup_to_active_on_primary_found() {
        let from = OrchestrationState::PrimarySetup;
        let to = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S4: PrimarySetup -> DegradedToManual ─────────────────────────

    #[test]
    fn setup_to_degraded_on_no_primary() {
        let from = OrchestrationState::PrimarySetup;
        let to = OrchestrationState::DegradedToManual {
            reason: DegradationReason::NoPrimaryPaneFound,
        };
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S5: PrimaryActive -> DegradedToManual (lost) ─────────────────

    #[test]
    fn active_to_degraded_on_primary_lost() {
        let from = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        let to = OrchestrationState::DegradedToManual {
            reason: DegradationReason::PrimaryPaneLost,
        };
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S6: PrimaryActive -> DegradedToManual (capability lost) ──────

    #[test]
    fn active_to_degraded_on_capability_lost() {
        let from = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        let to = OrchestrationState::DegradedToManual {
            reason: DegradationReason::PrimaryCapabilityLost,
        };
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S7: PrimaryActive -> DegradedToManual (unreachable) ──────────

    #[test]
    fn active_to_degraded_on_bus_failure() {
        let from = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        let to = OrchestrationState::DegradedToManual {
            reason: DegradationReason::PrimaryUnreachable,
        };
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S8: DegradedToManual -> PrimarySetup ─────────────────────────

    #[test]
    fn degraded_to_setup_on_re_request() {
        let from = OrchestrationState::DegradedToManual {
            reason: DegradationReason::PrimaryPaneLost,
        };
        let to = OrchestrationState::PrimarySetup;
        assert!(validate_transition(&from, &to).is_ok());
    }

    // ── S9: Manual -> PrimaryActive is forbidden ─────────────────────

    #[test]
    fn manual_to_active_direct_is_forbidden() {
        let from = OrchestrationState::Manual;
        let to = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        assert!(validate_transition(&from, &to).is_err());
    }

    // ── S10: DegradedToManual -> PrimaryActive is forbidden ──────────

    #[test]
    fn degraded_to_active_direct_is_forbidden() {
        let from = OrchestrationState::DegradedToManual {
            reason: DegradationReason::NoPrimaryPaneFound,
        };
        let to = OrchestrationState::PrimaryActive {
            primary: test_runtime_id("%0"),
        };
        assert!(validate_transition(&from, &to).is_err());
    }

    // ── Additional: self-transitions are forbidden ───────────────────

    #[test]
    fn manual_to_manual_is_forbidden() {
        let from = OrchestrationState::Manual;
        let to = OrchestrationState::Manual;
        assert!(validate_transition(&from, &to).is_err());
    }

    #[test]
    fn orchestration_state_default_is_manual() {
        assert_eq!(OrchestrationState::default(), OrchestrationState::Manual);
    }

    #[test]
    fn worker_selection_criteria_construction() {
        let criteria = WorkerSelectionCriteria {
            required_capabilities: [
                PaneCapability::MessageReceiver,
                PaneCapability::CommandExecutor,
            ]
            .into_iter()
            .collect(),
            required_role: PaneRole::Worker,
        };
        assert_eq!(criteria.required_capabilities.len(), 2);
        assert_eq!(criteria.required_role, PaneRole::Worker);
    }

    #[test]
    fn invalid_transition_display() {
        let err = InvalidTransition {
            from: "Manual".to_owned(),
            to: "PrimaryActive".to_owned(),
        };
        let display = err.to_string();
        assert!(display.contains("Manual"));
        assert!(display.contains("PrimaryActive"));
    }
}

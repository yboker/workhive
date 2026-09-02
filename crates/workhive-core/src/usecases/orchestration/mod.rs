//! Orchestration use-case functions.
//!
//! Pure functions for building orchestration plans, selecting workers,
//! validating requests, and executing orchestration actions. These
//! functions are the core logic consumed by `OrchestrationPort`
//! implementations.
//!
//! ## Module split
//!
//! This module used to live in a single 950-line `orchestration.rs`
//! file. Per the file-size discipline (800-line cap) the file was converted into a
//! directory module with the implementation kept in `mod.rs` and the
//! test suite moved to a sibling `tests.rs`. No public symbol was
//! renamed or removed.

use crate::domain::{
    MessageAcceptance, MessageEnvelope, MessageId, MessageTarget, OrchestrationIntent,
    OrchestrationPlan, OrchestrationRequest, PaneCapability, PaneDescriptor, PaneRole,
    PaneRuntimeId, WorkerSelectionCriteria, WorkspaceId, WorkspaceInteractionMode,
};
use crate::errors::OrchestrationError;
use crate::ports::{PaneMessageBusPort, PaneRegistryPort};

/// Build an orchestration plan from registry state.
///
/// Queries the registry for all panes in the workspace, then derives
/// primary and worker lists based on role and capability assignments.
///
/// For `Manual` mode, always returns a plan with `primary: None` and
/// `workers: []` — no orchestration in manual mode.
pub fn build_orchestration_plan(
    registry: &dyn PaneRegistryPort,
    workspace: &WorkspaceId,
    mode: WorkspaceInteractionMode,
) -> Result<OrchestrationPlan, OrchestrationError> {
    if mode == WorkspaceInteractionMode::Manual {
        return Ok(OrchestrationPlan {
            workspace: workspace.clone(),
            mode,
            primary: None,
            workers: vec![],
        });
    }

    let panes = registry.list_workspace(workspace.as_str())?;

    let primary = find_primary(&panes);

    let mut workers = find_workers(&panes);
    workers.sort_by(|a, b| a.as_str().cmp(b.as_str()));

    Ok(OrchestrationPlan {
        workspace: workspace.clone(),
        mode,
        primary,
        workers,
    })
}

/// Find the primary pane from a list of descriptors.
fn find_primary(panes: &[PaneDescriptor]) -> Option<PaneRuntimeId> {
    panes
        .iter()
        .find(|p| p.role == Some(PaneRole::Primary))
        .map(|p| p.runtime_id.clone())
}

/// Find worker panes that can receive messages.
fn find_workers(panes: &[PaneDescriptor]) -> Vec<PaneRuntimeId> {
    panes
        .iter()
        .filter(|p| {
            p.role == Some(PaneRole::Worker)
                && p.capabilities.contains(&PaneCapability::MessageReceiver)
        })
        .map(|p| p.runtime_id.clone())
        .collect()
}

/// Select workers from an orchestration plan by capability criteria.
///
/// Returns the subset of `plan.workers` whose descriptors (looked up
/// from the registry) satisfy all `criteria.required_capabilities`.
///
/// Workers are returned in the plan's deterministic order.
pub fn select_workers(
    plan: &OrchestrationPlan,
    criteria: &WorkerSelectionCriteria,
    registry: &dyn PaneRegistryPort,
) -> Result<Vec<PaneRuntimeId>, OrchestrationError> {
    let mut selected = Vec::new();

    for worker_id in &plan.workers {
        let descriptor = registry.get(worker_id)?;
        if let Some(desc) = descriptor
            && desc.role == Some(criteria.required_role)
            && criteria
                .required_capabilities
                .iter()
                .all(|cap| desc.capabilities.contains(cap))
        {
            selected.push(worker_id.clone());
        }
    }

    Ok(selected)
}

/// Validate an orchestration request against the current plan.
///
/// Checks:
/// 1. Workspace is in `PrimaryAssisted` mode
/// 2. A primary pane exists
/// 3. Target worker constraints match the intent
/// 4. Target worker exists in the plan's worker list
pub fn validate_orchestration_request(
    request: &OrchestrationRequest,
    plan: &OrchestrationPlan,
) -> Result<(), OrchestrationError> {
    if plan.mode == WorkspaceInteractionMode::Manual {
        return Err(OrchestrationError::NotPrimaryAssisted {
            workspace: request.workspace.to_string(),
        });
    }

    if plan.primary.is_none() {
        return Err(OrchestrationError::NoPrimaryPane {
            workspace: request.workspace.to_string(),
        });
    }

    match request.intent {
        OrchestrationIntent::DelegateTask
        | OrchestrationIntent::RequestStatus
        | OrchestrationIntent::InterruptTask => {
            let target =
                request
                    .target_worker
                    .as_ref()
                    .ok_or(OrchestrationError::TargetWorkerRequired {
                        intent: request.intent,
                    })?;

            if !plan.workers.contains(target) {
                return Err(OrchestrationError::WorkerNotFound {
                    worker: target.to_string(),
                });
            }
        }
        OrchestrationIntent::BroadcastContext => {
            if request.target_worker.is_some() {
                return Err(OrchestrationError::TargetWorkerForbidden {
                    intent: request.intent,
                });
            }
        }
    }

    Ok(())
}

/// Execute an orchestration request: validate, build envelope, publish.
///
/// This is the main orchestration execution path. It:
/// 1. Validates the request against the plan
/// 2. Translates the intent into a `MessageEnvelope`
/// 3. Publishes via the message bus
/// 4. Returns the bus acceptance result
pub fn execute_orchestration_request(
    request: &OrchestrationRequest,
    plan: &OrchestrationPlan,
    bus: &mut dyn PaneMessageBusPort,
) -> Result<MessageAcceptance, OrchestrationError> {
    validate_orchestration_request(request, plan)?;

    let target =
        match request.intent {
            OrchestrationIntent::DelegateTask
            | OrchestrationIntent::RequestStatus
            | OrchestrationIntent::InterruptTask => {
                let worker = request.target_worker.clone().ok_or(
                    OrchestrationError::TargetWorkerRequired {
                        intent: request.intent,
                    },
                )?;
                MessageTarget::Pane(worker)
            }
            OrchestrationIntent::BroadcastContext => MessageTarget::Role(PaneRole::Worker),
        };

    let envelope = MessageEnvelope {
        id: MessageId::new(),
        source: plan.primary.clone(),
        workspace: request.workspace.clone(),
        target,
        payload: request.payload.clone(),
    };

    let acceptance = bus.publish(envelope)?;
    Ok(acceptance)
}

#[cfg(test)]
mod tests;

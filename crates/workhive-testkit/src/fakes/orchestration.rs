//! Fake orchestration port for testing CLI and use-case code.
//!
//! Provides controllable state without requiring a real registry or bus.

use std::collections::HashMap;

use workhive_core::domain::{
    MessageAcceptance, MessageId, OrchestrationPlan, OrchestrationRequest, OrchestrationState,
    WorkspaceId, WorkspaceInteractionMode, validate_transition,
};
use workhive_core::errors::OrchestrationError;
use workhive_core::ports::OrchestrationPort;

/// Fake `OrchestrationPort` with controllable state and responses.
///
/// By default, `build_plan` returns a manual-mode plan and `execute`
/// returns `NotPrimaryAssisted`. Configure via `set_plan` and
/// `set_state` to test different scenarios.
pub struct FakeOrchestration {
    states: HashMap<String, OrchestrationState>,
    plans: HashMap<String, OrchestrationPlan>,
    execute_count: usize,
}

impl FakeOrchestration {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            plans: HashMap::new(),
            execute_count: 0,
        }
    }

    /// Pre-configure the plan returned by `build_plan` for a workspace.
    pub fn set_plan(&mut self, workspace: &WorkspaceId, plan: OrchestrationPlan) {
        self.plans.insert(workspace.as_str().to_owned(), plan);
    }

    /// Pre-configure the state for a workspace (bypasses transition rules).
    pub fn set_state(&mut self, workspace: &WorkspaceId, state: OrchestrationState) {
        self.states.insert(workspace.as_str().to_owned(), state);
    }

    /// Return how many times `execute` was called.
    pub fn execute_count(&self) -> usize {
        self.execute_count
    }
}

impl Default for FakeOrchestration {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchestrationPort for FakeOrchestration {
    fn build_plan(&self, workspace: &WorkspaceId) -> Result<OrchestrationPlan, OrchestrationError> {
        if let Some(plan) = self.plans.get(workspace.as_str()) {
            return Ok(plan.clone());
        }

        Ok(OrchestrationPlan {
            workspace: workspace.clone(),
            mode: WorkspaceInteractionMode::Manual,
            primary: None,
            workers: vec![],
        })
    }

    fn execute(
        &mut self,
        request: OrchestrationRequest,
    ) -> Result<MessageAcceptance, OrchestrationError> {
        self.execute_count += 1;

        let state = self.state(&request.workspace);
        match state {
            OrchestrationState::PrimaryActive { .. } => Ok(MessageAcceptance::Accepted {
                message_id: MessageId::new(),
            }),
            _ => Err(OrchestrationError::NotPrimaryAssisted {
                workspace: request.workspace.to_string(),
            }),
        }
    }

    fn state(&self, workspace: &WorkspaceId) -> OrchestrationState {
        self.states
            .get(workspace.as_str())
            .cloned()
            .unwrap_or_default()
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

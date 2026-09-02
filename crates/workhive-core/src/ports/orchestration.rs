//! Port trait for workspace orchestration operations.
//!
//! Encapsulates the coordination logic for primary-assisted mode.
//! Implementations depend on `PaneRegistryPort` and `PaneMessageBusPort`
//! but do not directly access tmux or any runtime backend.

use crate::domain::{
    MessageAcceptance, OrchestrationPlan, OrchestrationRequest, OrchestrationState, WorkspaceId,
};
use crate::errors::OrchestrationError;

/// Port for workspace orchestration operations.
///
/// ## Contract rules
///
/// - `build_plan()` is a pure derivation from registry state; it does not
///   mutate anything.
/// - `execute()` validates the request before publishing — invalid requests
///   are rejected with typed errors, not passed to the bus.
/// - `state()` returns the current state machine position; default is `Manual`.
/// - `transition()` enforces the state machine transition rules.
/// - The port MUST NOT directly call tmux, git, or any system command.
/// - The port MUST NOT read configuration files — mode selection is passed
///   in from the CLI/use-case layer.
pub trait OrchestrationPort {
    /// Build the current orchestration plan for a workspace.
    ///
    /// Queries the registry for the workspace's panes and derives
    /// the plan from roles and capabilities.
    fn build_plan(&self, workspace: &WorkspaceId) -> Result<OrchestrationPlan, OrchestrationError>;

    /// Execute an orchestration request.
    ///
    /// Validates the request against the current plan, translates
    /// the intent into a message envelope, and publishes via the bus.
    fn execute(
        &mut self,
        request: OrchestrationRequest,
    ) -> Result<MessageAcceptance, OrchestrationError>;

    /// Query the current orchestration state for a workspace.
    fn state(&self, workspace: &WorkspaceId) -> OrchestrationState;

    /// Transition the orchestration state.
    ///
    /// Enforces the state machine rules. Returns an error if the
    /// transition is forbidden.
    fn transition(
        &mut self,
        workspace: &WorkspaceId,
        new_state: OrchestrationState,
    ) -> Result<(), OrchestrationError>;
}

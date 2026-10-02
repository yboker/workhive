//! Task and scheduler domain types.
//!
//! These types model task identity, lifecycle, and assignment using
//! runtime-neutral workspace and unit identifiers.

use serde::{Deserialize, Serialize};

use super::{UnitId, WorkspaceId};

/// Stable identifier for a task within workhive orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(String);

impl TaskId {
    /// Construct a task id from a non-empty string.
    pub fn new(id: impl Into<String>) -> Result<Self, TaskValidationError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(TaskValidationError::EmptyTaskId);
        }
        Ok(Self(id))
    }

    /// Borrow the inner identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Runtime-neutral task lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Assigned,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
}

/// Assignment of a task to a runtime-neutral worker unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAssignment {
    pub task_id: TaskId,
    pub workspace: WorkspaceId,
    pub assignee: UnitId,
}

/// Invalid task construction.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskValidationError {
    #[error("task ID must not be empty")]
    EmptyTaskId,
}

/// Invalid task lifecycle transition.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid task status transition: {from:?} -> {to:?}")]
pub struct InvalidTaskTransition {
    pub from: TaskStatus,
    pub to: TaskStatus,
}

/// Validate a task lifecycle transition.
pub fn validate_task_transition(
    from: TaskStatus,
    to: TaskStatus,
) -> Result<(), InvalidTaskTransition> {
    match (from, to) {
        (TaskStatus::Pending, TaskStatus::Assigned) => Ok(()),
        (TaskStatus::Pending, TaskStatus::Cancelled) => Ok(()),
        (TaskStatus::Assigned, TaskStatus::Running) => Ok(()),
        (TaskStatus::Assigned, TaskStatus::Cancelled) => Ok(()),
        (TaskStatus::Running, TaskStatus::Waiting) => Ok(()),
        (TaskStatus::Running, TaskStatus::Succeeded) => Ok(()),
        (TaskStatus::Running, TaskStatus::Failed) => Ok(()),
        (TaskStatus::Running, TaskStatus::Cancelled) => Ok(()),
        (TaskStatus::Waiting, TaskStatus::Running) => Ok(()),
        (TaskStatus::Waiting, TaskStatus::Failed) => Ok(()),
        (TaskStatus::Waiting, TaskStatus::Cancelled) => Ok(()),
        _ if from == to => Err(InvalidTaskTransition { from, to }),
        _ => Err(InvalidTaskTransition { from, to }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_id_rejects_empty() {
        let err = TaskId::new("   ").unwrap_err();
        assert_eq!(err, TaskValidationError::EmptyTaskId);
    }

    #[test]
    fn task_id_accepts_non_empty() {
        let id = TaskId::new("task-123").unwrap();
        assert_eq!(id.as_str(), "task-123");
    }

    #[test]
    fn valid_task_transitions_are_accepted() {
        assert!(validate_task_transition(TaskStatus::Pending, TaskStatus::Assigned).is_ok());
        assert!(validate_task_transition(TaskStatus::Assigned, TaskStatus::Running).is_ok());
        assert!(validate_task_transition(TaskStatus::Running, TaskStatus::Waiting).is_ok());
        assert!(validate_task_transition(TaskStatus::Waiting, TaskStatus::Running).is_ok());
        assert!(validate_task_transition(TaskStatus::Running, TaskStatus::Succeeded).is_ok());
    }

    #[test]
    fn invalid_task_transition_is_rejected() {
        let err = validate_task_transition(TaskStatus::Pending, TaskStatus::Running).unwrap_err();
        assert_eq!(
            err,
            InvalidTaskTransition {
                from: TaskStatus::Pending,
                to: TaskStatus::Running,
            }
        );
    }
}

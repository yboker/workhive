//! Minimal scheduler use cases.
//!
//! This module provides a runtime-neutral task lifecycle and assignment
//! layer over [`SchedulerPort`].

use crate::domain::{
    TaskAssignment, TaskId, TaskStatus, UnitId, WorkspaceId, validate_task_transition,
};
use crate::errors::SchedulerError;
use crate::ports::SchedulerPort;

/// Assign a task to a worker unit in a workspace.
///
/// This transitions the task from `Pending` to `Assigned` and persists both
/// the assignment and the new status through the scheduler port.
pub fn assign_task(
    scheduler: &dyn SchedulerPort,
    task_id: TaskId,
    workspace: WorkspaceId,
    assignee: UnitId,
) -> Result<TaskAssignment, SchedulerError> {
    let current =
        scheduler
            .load_task_status(&task_id)?
            .ok_or_else(|| SchedulerError::TaskNotFound {
                task_id: task_id.to_string(),
            })?;

    validate_task_transition(current, TaskStatus::Assigned)?;

    let assignment = TaskAssignment {
        task_id: task_id.clone(),
        workspace,
        assignee,
    };
    scheduler.save_task_assignment(&assignment)?;
    scheduler.save_task_status(&task_id, TaskStatus::Assigned)?;
    Ok(assignment)
}

/// Transition a task to a new lifecycle status.
pub fn transition_task_status(
    scheduler: &dyn SchedulerPort,
    task_id: &TaskId,
    new_status: TaskStatus,
) -> Result<TaskStatus, SchedulerError> {
    let current =
        scheduler
            .load_task_status(task_id)?
            .ok_or_else(|| SchedulerError::TaskNotFound {
                task_id: task_id.to_string(),
            })?;
    validate_task_transition(current, new_status)?;
    scheduler.save_task_status(task_id, new_status)?;
    Ok(new_status)
}

// Scheduler usecase tests live in `workhive-testkit/tests/scheduler_usecases.rs`
// to avoid a circular dev-dependency between core and testkit.

//! Port trait for task scheduler persistence and coordination state.

use crate::domain::{TaskAssignment, TaskId, TaskStatus};
use crate::errors::SchedulerError;

/// Storage/control seam for scheduler state.
///
/// This port is runtime-neutral: it persists task status and assignment
/// without depending on tmux or any concrete runtime backend.
pub trait SchedulerPort {
    /// Load the current status for a task.
    fn load_task_status(&self, task_id: &TaskId) -> Result<Option<TaskStatus>, SchedulerError>;

    /// Persist the current status for a task.
    fn save_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<(), SchedulerError>;

    /// Persist task assignment to a worker unit.
    fn save_task_assignment(&self, assignment: &TaskAssignment) -> Result<(), SchedulerError>;
}

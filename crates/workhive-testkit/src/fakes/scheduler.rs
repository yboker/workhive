//! Fake scheduler adapter for testing.

use std::cell::RefCell;
use std::collections::HashMap;

use workhive_core::domain::{TaskAssignment, TaskId, TaskStatus};
use workhive_core::errors::SchedulerError;
use workhive_core::ports::SchedulerPort;

/// Determines which storage operation will fail when an error is injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerErrorTarget {
    /// Fail on `save_task_assignment`.
    SaveAssignment,
    /// Fail on `save_task_status`.
    SaveStatus,
}

/// In-memory fake for [`SchedulerPort`].
///
/// Supports preloading task statuses, recording saved assignments,
/// and optional injection of [`SchedulerError::Storage`] on specific
/// write operations.
pub struct FakeScheduler {
    statuses: RefCell<HashMap<String, TaskStatus>>,
    assignments: RefCell<Vec<TaskAssignment>>,
    error_injection: RefCell<Option<SchedulerErrorTarget>>,
}

impl FakeScheduler {
    /// Create an empty fake scheduler.
    pub fn new() -> Self {
        Self {
            statuses: RefCell::new(HashMap::new()),
            assignments: RefCell::new(Vec::new()),
            error_injection: RefCell::new(None),
        }
    }

    /// Preload a task status for testing.
    pub fn with_status(self, task_id: &str, status: TaskStatus) -> Self {
        self.statuses
            .borrow_mut()
            .insert(task_id.to_owned(), status);
        self
    }

    /// Inject a storage error on the specified target operation.
    ///
    /// The error fires once and is consumed on the first matching call.
    pub fn with_error(self, target: SchedulerErrorTarget) -> Self {
        *self.error_injection.borrow_mut() = Some(target);
        self
    }

    /// Read the current statuses map for test assertions.
    pub fn statuses(&self) -> HashMap<String, TaskStatus> {
        self.statuses.borrow().clone()
    }

    /// Read recorded assignments for test assertions.
    pub fn assignments(&self) -> Vec<TaskAssignment> {
        self.assignments.borrow().clone()
    }
}

impl Default for FakeScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerPort for FakeScheduler {
    fn load_task_status(&self, task_id: &TaskId) -> Result<Option<TaskStatus>, SchedulerError> {
        Ok(self.statuses.borrow().get(task_id.as_str()).copied())
    }

    fn save_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<(), SchedulerError> {
        if self.error_injection.borrow().as_ref() == Some(&SchedulerErrorTarget::SaveStatus) {
            *self.error_injection.borrow_mut() = None;
            return Err(SchedulerError::Storage {
                message: "injected save_task_status failure".to_owned(),
            });
        }
        self.statuses
            .borrow_mut()
            .insert(task_id.as_str().to_owned(), status);
        Ok(())
    }

    fn save_task_assignment(&self, assignment: &TaskAssignment) -> Result<(), SchedulerError> {
        if self.error_injection.borrow().as_ref() == Some(&SchedulerErrorTarget::SaveAssignment) {
            *self.error_injection.borrow_mut() = None;
            return Err(SchedulerError::Storage {
                message: "injected save_task_assignment failure".to_owned(),
            });
        }
        self.assignments.borrow_mut().push(assignment.clone());
        Ok(())
    }
}

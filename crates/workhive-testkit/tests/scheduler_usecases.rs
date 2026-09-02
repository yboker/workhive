//! Scheduler use-case tests exercised via the shared `FakeScheduler`.
//!
//! These tests validate `assign_task` and `transition_task_status` from
//! `workhive_core::usecases::scheduler` against the reusable testkit fake,
//! covering happy paths, invalid transitions, missing tasks, and storage
//! error injection.

use workhive_core::domain::{TaskId, TaskStatus, UnitId, WorkspaceId};
use workhive_core::errors::SchedulerError;
use workhive_core::usecases::scheduler::{assign_task, transition_task_status};
use workhive_testkit::fakes::{FakeScheduler, SchedulerErrorTarget};

// ---------------------------------------------------------------------------
// assign_task
// ---------------------------------------------------------------------------

#[test]
fn assign_task_happy_path() {
    let scheduler = FakeScheduler::new().with_status("task-1", TaskStatus::Pending);
    let task_id = TaskId::new("task-1").expect("valid task id");
    let workspace = WorkspaceId::new("feat-x").expect("valid workspace id");
    let assignee = UnitId::new("%1");

    let assignment = assign_task(
        &scheduler,
        task_id.clone(),
        workspace.clone(),
        assignee.clone(),
    )
    .expect("assign_task should succeed");

    assert_eq!(assignment.task_id, task_id);
    assert_eq!(assignment.workspace, workspace);
    assert_eq!(assignment.assignee, assignee);
    assert_eq!(
        scheduler.statuses().get("task-1"),
        Some(&TaskStatus::Assigned)
    );
    assert_eq!(scheduler.assignments().len(), 1);
}

#[test]
fn assign_task_rejects_invalid_transition() {
    let scheduler = FakeScheduler::new().with_status("task-2", TaskStatus::Succeeded);

    let err = assign_task(
        &scheduler,
        TaskId::new("task-2").expect("valid task id"),
        WorkspaceId::new("feat-x").expect("valid workspace id"),
        UnitId::new("%2"),
    )
    .expect_err("should reject invalid transition");

    assert!(matches!(err, SchedulerError::InvalidTransition(_)));
}

#[test]
fn assign_task_errors_when_task_missing() {
    let scheduler = FakeScheduler::new();

    let err = assign_task(
        &scheduler,
        TaskId::new("missing").expect("valid task id"),
        WorkspaceId::new("feat-x").expect("valid workspace id"),
        UnitId::new("%1"),
    )
    .expect_err("should return TaskNotFound");

    assert!(matches!(err, SchedulerError::TaskNotFound { .. }));
}

#[test]
fn assign_task_storage_failure_on_save_assignment() {
    let scheduler = FakeScheduler::new()
        .with_status("task-3", TaskStatus::Pending)
        .with_error(SchedulerErrorTarget::SaveAssignment);

    let err = assign_task(
        &scheduler,
        TaskId::new("task-3").expect("valid task id"),
        WorkspaceId::new("feat-x").expect("valid workspace id"),
        UnitId::new("%1"),
    )
    .expect_err("should propagate storage error");

    assert!(matches!(err, SchedulerError::Storage { .. }));
}

#[test]
fn assign_task_storage_failure_on_save_status() {
    let scheduler = FakeScheduler::new()
        .with_status("task-4", TaskStatus::Pending)
        .with_error(SchedulerErrorTarget::SaveStatus);

    let err = assign_task(
        &scheduler,
        TaskId::new("task-4").expect("valid task id"),
        WorkspaceId::new("feat-x").expect("valid workspace id"),
        UnitId::new("%1"),
    )
    .expect_err("should propagate storage error");

    assert!(matches!(err, SchedulerError::Storage { .. }));
}

// ---------------------------------------------------------------------------
// transition_task_status
// ---------------------------------------------------------------------------

#[test]
fn transition_task_status_happy_path() {
    let scheduler = FakeScheduler::new().with_status("task-5", TaskStatus::Assigned);
    let task_id = TaskId::new("task-5").expect("valid task id");

    let new_status = transition_task_status(&scheduler, &task_id, TaskStatus::Running)
        .expect("transition should succeed");

    assert_eq!(new_status, TaskStatus::Running);
    assert_eq!(
        scheduler.statuses().get("task-5"),
        Some(&TaskStatus::Running)
    );
}

#[test]
fn transition_task_status_rejects_invalid_transition() {
    let scheduler = FakeScheduler::new().with_status("task-6", TaskStatus::Pending);

    let err = transition_task_status(
        &scheduler,
        &TaskId::new("task-6").expect("valid task id"),
        TaskStatus::Running,
    )
    .expect_err("should reject invalid transition");

    assert!(matches!(err, SchedulerError::InvalidTransition(_)));
}

#[test]
fn transition_task_status_errors_when_task_missing() {
    let scheduler = FakeScheduler::new();

    let err = transition_task_status(
        &scheduler,
        &TaskId::new("missing").expect("valid task id"),
        TaskStatus::Running,
    )
    .expect_err("should return TaskNotFound");

    assert!(matches!(err, SchedulerError::TaskNotFound { .. }));
}

#[test]
fn transition_task_status_storage_failure_on_save() {
    let scheduler = FakeScheduler::new()
        .with_status("task-7", TaskStatus::Assigned)
        .with_error(SchedulerErrorTarget::SaveStatus);

    let err = transition_task_status(
        &scheduler,
        &TaskId::new("task-7").expect("valid task id"),
        TaskStatus::Running,
    )
    .expect_err("should propagate storage error");

    assert!(matches!(err, SchedulerError::Storage { .. }));
}

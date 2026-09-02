//! Fake project index adapter for testing.

use std::cell::RefCell;

use camino::Utf8PathBuf;
use workhive_core::errors::IndexError;
use workhive_core::ports::{IndexState, ProjectIndexPort, ProjectSummary};

/// In-memory fake for `ProjectIndexPort`.
///
/// Starts in `initialized = true` state (simulating an existing but empty
/// index file). Call `with_not_found()` to simulate a missing index.
pub struct FakeProjectIndex {
    projects: RefCell<Vec<ProjectSummary>>,
    /// When `true`, `list_projects` returns `IndexState::NotFound`.
    not_found: RefCell<bool>,
    upsert_error: RefCell<Option<IndexError>>,
    /// One-shot error returned by the next `list_projects` call.
    ///
    /// `IndexError` carries a non-`Clone` `std::io::Error` source on the
    /// `ReadFailed`/`WriteFailed` variants, so we install the value once and
    /// the next caller takes it. Trusted-root removal safety tests only need
    /// to drive a single `list_projects` invocation per command, so
    /// take-once semantics are sufficient.
    list_error: RefCell<Option<IndexError>>,
}

impl FakeProjectIndex {
    pub fn new() -> Self {
        Self {
            projects: RefCell::new(Vec::new()),
            not_found: RefCell::new(false),
            upsert_error: RefCell::new(None),
            list_error: RefCell::new(None),
        }
    }

    /// Create a fake that simulates a missing index file.
    pub fn with_not_found() -> Self {
        Self {
            projects: RefCell::new(Vec::new()),
            not_found: RefCell::new(true),
            upsert_error: RefCell::new(None),
            list_error: RefCell::new(None),
        }
    }

    /// Inject an error that `upsert_project()` will return once.
    pub fn set_upsert_error(&self, err: IndexError) {
        *self.upsert_error.borrow_mut() = Some(err);
    }

    /// Install a sticky error for `list_projects()`.
    ///
    /// Used by trusted-root removal safety tests to prove the safety gate
    /// fails closed when the project index cannot be read or parsed —
    /// `IndexError::ReadFailed` and `IndexError::Corrupted` must both
    /// propagate up to `CliError::Index`, never silently degrade to "no
    /// affected projects".
    pub fn set_list_error(&self, err: IndexError) {
        *self.list_error.borrow_mut() = Some(err);
    }
}

impl Default for FakeProjectIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectIndexPort for FakeProjectIndex {
    fn list_projects(&self) -> Result<IndexState, IndexError> {
        if let Some(err) = self.list_error.borrow_mut().take() {
            return Err(err);
        }
        if *self.not_found.borrow() {
            return Ok(IndexState::NotFound);
        }
        Ok(IndexState::Loaded(self.projects.borrow().clone()))
    }

    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError> {
        if let Some(err) = self.upsert_error.borrow_mut().take() {
            return Err(err);
        }
        *self.not_found.borrow_mut() = false;
        let mut projects = self.projects.borrow_mut();
        projects.retain(|p| p.project_root != summary.project_root);
        projects.push(summary.clone());
        Ok(())
    }

    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError> {
        self.projects
            .borrow_mut()
            .retain(|p| &p.project_root != project_root);
        Ok(())
    }

    fn clear(&self) -> Result<(), IndexError> {
        *self.not_found.borrow_mut() = false;
        self.projects.borrow_mut().clear();
        Ok(())
    }

    fn rebuild(&self) -> Result<(), IndexError> {
        Ok(())
    }
}

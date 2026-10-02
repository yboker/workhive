//! Fake project index adapter for testing.

use std::cell::RefCell;

use camino::Utf8PathBuf;
use workhive_core::errors::IndexError;
use workhive_core::ports::{
    IndexSnapshotToken, IndexState, PairedIndexState, ProjectIndexPort, ProjectSummary,
    SnapshotPublishOutcome,
};

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
    publish_hook: RefCell<Option<Box<dyn Fn()>>>,
    /// Content-derived token for optimistic concurrency
    token: RefCell<IndexSnapshotToken>,
}

impl FakeProjectIndex {
    pub fn new() -> Self {
        // Compute token for empty projects list
        let empty_projects: Vec<ProjectSummary> = Vec::new();
        let content = serde_json::to_string(&empty_projects).unwrap_or_default();
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let result = hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&result);

        Self {
            projects: RefCell::new(Vec::new()),
            not_found: RefCell::new(false),
            upsert_error: RefCell::new(None),
            list_error: RefCell::new(None),
            publish_hook: RefCell::new(None),
            token: RefCell::new(IndexSnapshotToken::from_digest(digest)),
        }
    }

    /// Create a fake that simulates a missing index file.
    pub fn with_not_found() -> Self {
        Self {
            projects: RefCell::new(Vec::new()),
            not_found: RefCell::new(true),
            upsert_error: RefCell::new(None),
            list_error: RefCell::new(None),
            publish_hook: RefCell::new(None),
            token: RefCell::new(IndexSnapshotToken::missing()),
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

    /// Install a callback invoked during conditional publication.
    ///
    /// The hook runs while the publication is in progress, allowing tests to
    /// observe or contend with the publication (e.g., attempt to acquire a
    /// scheduling lease to verify the worker holds it during publication).
    pub fn set_publish_hook(&self, hook: impl Fn() + 'static) {
        *self.publish_hook.borrow_mut() = Some(Box::new(hook));
    }

    fn update_token(&self) {
        // Derive a new token from the current projects state
        let projects = self.projects.borrow();
        let content = serde_json::to_string(&*projects).unwrap_or_default();
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let result = hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&result);
        *self.token.borrow_mut() = IndexSnapshotToken::from_digest(digest);
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

    fn read_paired_state(&self) -> Result<PairedIndexState, IndexError> {
        let state = self.list_projects()?;
        let token = self.token.borrow().clone();
        Ok(PairedIndexState { state, token })
    }

    fn capture_snapshot_token(&self) -> Result<IndexSnapshotToken, IndexError> {
        Ok(self.token.borrow().clone())
    }

    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError> {
        if let Some(err) = self.upsert_error.borrow_mut().take() {
            return Err(err);
        }
        *self.not_found.borrow_mut() = false;
        let mut projects = self.projects.borrow_mut();
        projects.retain(|p| p.project_root != summary.project_root);
        projects.push(summary.clone());
        drop(projects);
        self.update_token();
        Ok(())
    }

    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError> {
        self.projects
            .borrow_mut()
            .retain(|p| &p.project_root != project_root);
        self.update_token();
        Ok(())
    }

    fn clear(&self) -> Result<(), IndexError> {
        *self.not_found.borrow_mut() = false;
        self.projects.borrow_mut().clear();
        self.update_token();
        Ok(())
    }

    fn replace_all(&self, projects: &[ProjectSummary]) -> Result<(), IndexError> {
        *self.not_found.borrow_mut() = false;
        *self.projects.borrow_mut() = projects.to_vec();
        self.update_token();
        Ok(())
    }

    fn publish_snapshot_conditional(
        &self,
        projects: &[ProjectSummary],
        expected_token: IndexSnapshotToken,
    ) -> Result<SnapshotPublishOutcome, IndexError> {
        if let Some(hook) = self.publish_hook.borrow().as_ref() {
            hook();
        }
        if let Some(err) = self.list_error.borrow_mut().take() {
            return Err(err);
        }

        let current_token = self.token.borrow().clone();
        if current_token != expected_token {
            return Ok(SnapshotPublishOutcome::Stale);
        }

        *self.not_found.borrow_mut() = false;
        *self.projects.borrow_mut() = projects.to_vec();
        self.update_token();
        Ok(SnapshotPublishOutcome::Published)
    }
}

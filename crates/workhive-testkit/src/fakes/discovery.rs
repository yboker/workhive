//! Fake project discovery adapter for testing.

use std::cell::RefCell;

use camino::Utf8PathBuf;
use workhive_core::errors::DiscoveryError;
use workhive_core::ports::{
    DiscoveredProject, DiscoveryReport, DiscoveryWarning, ProjectDiscoveryPort,
};

/// In-memory fake for `ProjectDiscoveryPort`.
///
/// Pre-load with discovered projects for deterministic testing.
/// Tracks the number of `discover_projects_report` invocations so tests can
/// assert how many times discovery was run.
pub struct FakeDiscovery {
    projects: RefCell<Vec<DiscoveredProject>>,
    warnings: RefCell<Vec<DiscoveryWarning>>,
    error: RefCell<Option<String>>,
    call_count: RefCell<usize>,
}

impl FakeDiscovery {
    /// Build an empty discovery fake with no pre-loaded projects.
    pub fn new() -> Self {
        Self {
            projects: RefCell::new(Vec::new()),
            warnings: RefCell::new(Vec::new()),
            error: RefCell::new(None),
            call_count: RefCell::new(0),
        }
    }

    /// Add a discovered project to the fake.
    pub fn add_project(&self, project: DiscoveredProject) {
        self.projects.borrow_mut().push(project);
    }

    /// Add a non-fatal warning to be returned alongside discovered
    /// projects on the next scan.
    pub fn add_warning(&self, warning: DiscoveryWarning) {
        self.warnings.borrow_mut().push(warning);
    }

    /// Inject an error for the next `discover_projects` call.
    pub fn set_error(&self, msg: &str) {
        *self.error.borrow_mut() = Some(msg.to_owned());
    }

    /// Return how many times `discover_projects_report` has been invoked.
    ///
    /// Useful for asserting that discovery ran exactly once (e.g., that a
    /// bounded lease-retry loop did not re-scan on each contention attempt).
    pub fn call_count(&self) -> usize {
        *self.call_count.borrow()
    }
}

impl Default for FakeDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectDiscoveryPort for FakeDiscovery {
    fn discover_projects_report(
        &self,
        roots: &[Utf8PathBuf],
    ) -> Result<DiscoveryReport, DiscoveryError> {
        *self.call_count.borrow_mut() += 1;
        if let Some(msg) = self.error.borrow().as_deref() {
            return Err(DiscoveryError::ReadFailed {
                path: roots.first().cloned().unwrap_or_default(),
                source: std::io::Error::other(msg),
            });
        }
        if roots.is_empty() {
            return Err(DiscoveryError::NoRootsConfigured);
        }
        Ok(DiscoveryReport {
            projects: self.projects.borrow().clone(),
            warnings: self.warnings.borrow().clone(),
        })
    }
}

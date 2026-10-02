//! Fake dependency checker for testing.

use std::collections::HashMap;

use workhive_core::ports::{DependencyKind, DependencyPort, DependencyStatus, RuntimeError};

/// Controllable fake for dependency checks in tests.
///
/// By default, all dependencies report as satisfied. Tests can override
/// individual dependency results to simulate missing/outdated tools.
pub struct FakeDependency {
    overrides: HashMap<DependencyKind, DependencyStatus>,
    preflight_errors: HashMap<DependencyKind, RuntimeError>,
}

impl FakeDependency {
    pub fn new() -> Self {
        Self {
            overrides: HashMap::new(),
            preflight_errors: HashMap::new(),
        }
    }

    /// Override the status for a specific dependency kind.
    pub fn set_status(&mut self, kind: DependencyKind, status: DependencyStatus) {
        self.overrides.insert(kind, status);
    }

    /// Mark a dependency as missing.
    pub fn set_missing(&mut self, kind: DependencyKind) {
        self.overrides
            .insert(kind, DependencyStatus::Missing { kind });
    }

    /// Mark a dependency as built-in (future integration scenario).
    pub fn set_builtin(&mut self, kind: DependencyKind) {
        self.overrides
            .insert(kind, DependencyStatus::Builtin { kind });
    }

    /// Make backend readiness return a typed runtime failure.
    pub fn set_preflight_error(&mut self, kind: DependencyKind, error: RuntimeError) {
        self.preflight_errors.insert(kind, error);
    }

    fn default_status(kind: DependencyKind) -> DependencyStatus {
        DependencyStatus::Satisfied {
            kind,
            version: match kind {
                DependencyKind::Git => "2.43.0".to_owned(),
                DependencyKind::Tmux => "3.4".to_owned(),
                DependencyKind::Herdr => "22".to_owned(),
            },
        }
    }
}

impl Default for FakeDependency {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyPort for FakeDependency {
    fn check_all(&self) -> Vec<DependencyStatus> {
        vec![
            self.check(DependencyKind::Git),
            self.check(DependencyKind::Tmux),
        ]
    }

    fn preflight(&self, kind: DependencyKind) -> workhive_core::ports::BackendPreflight {
        let mut status = workhive_core::ports::BackendPreflight::from_dependency(self.check(kind));
        if let Some(error) = self.preflight_errors.get(&kind) {
            status.installed = true;
            status.startable = false;
            status.compatible = false;
            status.running = false;
            status.detail = Some(error.to_string());
            status.runtime_error = Some(error.clone());
        }
        status
    }

    fn check(&self, kind: DependencyKind) -> DependencyStatus {
        self.overrides
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| Self::default_status(kind))
    }
}

//! Fake dependency checker for testing.

use std::collections::HashMap;

use workhive_core::ports::{DependencyKind, DependencyPort, DependencyStatus};

/// Controllable fake for dependency checks in tests.
///
/// By default, all dependencies report as satisfied. Tests can override
/// individual dependency results to simulate missing/outdated tools.
pub struct FakeDependency {
    overrides: HashMap<DependencyKind, DependencyStatus>,
}

impl FakeDependency {
    pub fn new() -> Self {
        Self {
            overrides: HashMap::new(),
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

    fn default_status(kind: DependencyKind) -> DependencyStatus {
        DependencyStatus::Satisfied {
            kind,
            version: match kind {
                DependencyKind::Git => "2.43.0".to_owned(),
                DependencyKind::Tmux => "3.4".to_owned(),
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

    fn check(&self, kind: DependencyKind) -> DependencyStatus {
        self.overrides
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| Self::default_status(kind))
    }
}

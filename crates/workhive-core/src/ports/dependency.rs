//! Dependency port — abstracts external dependency detection.
//!
//! Checks whether required system tools (git, tmux, etc.) are available
//! and meet minimum version requirements. Designed for future extensibility:
//! when git/tmux become built-in (via gitoxide or native integration),
//! the corresponding `DependencyKind` variants can report `Builtin`
//! without changing any calling code.

use std::fmt;

/// Identifier for a required external dependency.
///
/// Enum-first design (ADR-002): each dependency is an explicit variant,
/// ensuring exhaustive handling across checks and display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyKind {
    Git,
    Tmux,
    Herdr,
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git => write!(f, "git"),
            Self::Tmux => write!(f, "tmux"),
            Self::Herdr => write!(f, "herdr"),
        }
    }
}

/// Result of checking a single dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyStatus {
    /// Found and meets minimum version requirement.
    Satisfied {
        kind: DependencyKind,
        version: String,
    },
    /// Found but version is below the minimum.
    VersionTooLow {
        kind: DependencyKind,
        found: String,
        minimum: String,
    },
    /// Not found on the system.
    Missing { kind: DependencyKind },
    /// Built into workhive — no external binary needed.
    /// Reserved for future integration (e.g., gitoxide replaces system git).
    Builtin { kind: DependencyKind },
}

impl DependencyStatus {
    /// Whether this dependency is usable (satisfied or built-in).
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Satisfied { .. } | Self::Builtin { .. })
    }

    /// The dependency kind this status refers to.
    pub fn kind(&self) -> DependencyKind {
        match self {
            Self::Satisfied { kind, .. }
            | Self::VersionTooLow { kind, .. }
            | Self::Missing { kind }
            | Self::Builtin { kind } => *kind,
        }
    }
}

/// Backend readiness dimensions returned by the shared preflight contract.
///
/// `running` is informational: a backend such as tmux can be startable even
/// when its daemon is not running yet. `is_ready` therefore does not require
/// it; the runtime adapter starts or connects to the backend when needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendPreflight {
    pub kind: DependencyKind,
    pub installed: bool,
    pub startable: bool,
    pub running: bool,
    pub compatible: bool,
    pub version: Option<String>,
    pub detail: Option<String>,
    /// The adapter's typed failure, when readiness reached the runtime
    /// connection and could preserve the exact cause.
    pub runtime_error: Option<super::runtime::RuntimeError>,
}

impl BackendPreflight {
    pub fn is_ready(&self) -> bool {
        self.installed && self.startable && self.compatible
    }

    pub fn from_dependency(status: DependencyStatus) -> Self {
        match status {
            DependencyStatus::Satisfied { kind, version } => Self {
                kind,
                installed: true,
                startable: true,
                running: false,
                compatible: true,
                version: Some(version),
                detail: None,
                runtime_error: None,
            },
            DependencyStatus::VersionTooLow {
                kind,
                found,
                minimum,
            } => Self {
                kind,
                installed: true,
                startable: false,
                running: false,
                compatible: false,
                version: Some(found),
                detail: Some(format!("minimum compatible version is {minimum}")),
                runtime_error: None,
            },
            DependencyStatus::Missing { kind } => Self {
                kind,
                installed: false,
                startable: false,
                running: false,
                compatible: false,
                version: None,
                detail: Some("executable was not found on PATH".to_owned()),
                runtime_error: None,
            },
            DependencyStatus::Builtin { kind } => Self {
                kind,
                installed: true,
                startable: true,
                running: true,
                compatible: true,
                version: None,
                detail: None,
                runtime_error: None,
            },
        }
    }
}

/// Port for checking external dependencies and backend readiness.
///
/// This trait allows the CLI to verify system requirements before any
/// operation. The `SystemDependencyChecker` in `workhive-infra` implements
/// the real checks; `FakeDependencyChecker` in `workhive-testkit` provides
/// a controllable test double.
///
/// The default `preflight` mapping keeps existing dependency providers
/// source-compatible while giving callers one shared readiness result.
pub trait DependencyPort {
    /// Check all required dependencies and return their statuses.
    fn check_all(&self) -> Vec<DependencyStatus>;

    /// Check a single dependency.
    fn check(&self, kind: DependencyKind) -> DependencyStatus;

    /// Run backend-neutral readiness preflight for one backend dependency.
    fn preflight(&self, kind: DependencyKind) -> BackendPreflight {
        BackendPreflight::from_dependency(self.check(kind))
    }
}

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
}

impl fmt::Display for DependencyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git => write!(f, "git"),
            Self::Tmux => write!(f, "tmux"),
        }
    }
}

/// Result of checking a single dependency.
#[derive(Debug, Clone)]
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

/// Port for checking external dependencies.
///
/// This trait allows the CLI to verify system requirements before any
/// operation. The `SystemDependencyChecker` in `workhive-infra` implements
/// the real checks; `FakeDependencyChecker` in `workhive-testkit` provides
/// a controllable test double.
///
/// Design intent: when git or tmux become built-in, the infra adapter
/// simply returns `DependencyStatus::Builtin` — no interface change needed.
pub trait DependencyPort {
    /// Check all required dependencies and return their statuses.
    fn check_all(&self) -> Vec<DependencyStatus>;

    /// Check a single dependency.
    fn check(&self, kind: DependencyKind) -> DependencyStatus;
}

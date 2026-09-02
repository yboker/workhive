//! Executable availability port — abstracts PATH-based binary detection.
//!
//! Replaces direct `which::which(...)` calls in CLI code so that both
//! `open` and `init` use the same availability-check boundary, and tests
//! can substitute a fake without filesystem access.

/// Port for checking whether an executable is available on the system PATH.
///
/// The production adapter (`SystemExecutableChecker`) delegates to `which::which`.
/// Test code uses `FakeExecutableChecker` from `workhive-testkit`.
pub trait ExecutableAvailabilityPort {
    /// Returns `true` if the named program is found on PATH.
    fn is_available(&self, program: &str) -> bool;
}

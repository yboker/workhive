//! System executable availability checker — delegates to `which::which`.

use workhive_core::ports::ExecutableAvailabilityPort;

/// Production adapter: checks PATH via the `which` crate.
pub struct SystemExecutableChecker;

impl SystemExecutableChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemExecutableChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutableAvailabilityPort for SystemExecutableChecker {
    fn is_available(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }
}

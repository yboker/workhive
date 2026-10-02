//! Fake executable availability checker for testing.

use std::cell::RefCell;
use std::collections::HashSet;

use workhive_core::ports::ExecutableAvailabilityPort;

/// In-memory fake: reports specific programs as available.
pub struct FakeExecutableChecker {
    available: RefCell<HashSet<String>>,
}

impl FakeExecutableChecker {
    pub fn new() -> Self {
        Self {
            available: RefCell::new(HashSet::new()),
        }
    }

    /// Mark a program as available.
    pub fn with_program(self, program: impl Into<String>) -> Self {
        self.available.borrow_mut().insert(program.into());
        self
    }

    /// Mark all given programs as available.
    pub fn add_all(self, programs: impl IntoIterator<Item = impl Into<String>>) -> Self {
        for p in programs {
            self.available.borrow_mut().insert(p.into());
        }
        self
    }
}

impl Default for FakeExecutableChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutableAvailabilityPort for FakeExecutableChecker {
    fn is_available(&self, program: &str) -> bool {
        self.available.borrow().contains(program)
    }
}

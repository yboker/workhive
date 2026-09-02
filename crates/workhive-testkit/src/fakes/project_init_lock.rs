//! Fake in-memory project init lock for testing.

use camino::Utf8Path;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use workhive_core::errors::ProjectInitError;
use workhive_core::ports::{ProjectInitLockGuard, ProjectInitLockPort};

struct FakeGuard {
    held: Arc<AtomicBool>,
}

impl Drop for FakeGuard {
    fn drop(&mut self) {
        self.held.store(false, Ordering::SeqCst);
    }
}

impl ProjectInitLockGuard for FakeGuard {}

/// Fake lock adapter for unit tests.
///
/// Tracks whether the lock is currently held and optionally injects errors.
pub struct FakeProjectInitLock {
    held: Arc<AtomicBool>,
    fail_on_acquire: RefCell<bool>,
}

impl FakeProjectInitLock {
    pub fn new() -> Self {
        Self {
            held: Arc::new(AtomicBool::new(false)),
            fail_on_acquire: RefCell::new(false),
        }
    }

    /// Make all subsequent `acquire_init_lock` calls fail.
    pub fn set_fail_on_acquire(&self, fail: bool) {
        *self.fail_on_acquire.borrow_mut() = fail;
    }
}

impl Default for FakeProjectInitLock {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectInitLockPort for FakeProjectInitLock {
    fn acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Box<dyn ProjectInitLockGuard>, ProjectInitError> {
        if *self.fail_on_acquire.borrow() {
            return Err(ProjectInitError::LockFailed {
                lock_path: project_root.join(".workhive.init.lock"),
                source: std::io::Error::other("fake lock failure"),
            });
        }
        self.held.store(true, Ordering::SeqCst);
        Ok(Box::new(FakeGuard {
            held: Arc::clone(&self.held),
        }))
    }

    fn try_acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Option<Box<dyn ProjectInitLockGuard>>, ProjectInitError> {
        if *self.fail_on_acquire.borrow() {
            return Err(ProjectInitError::LockFailed {
                lock_path: project_root.join(".workhive.init.lock"),
                source: std::io::Error::other("fake lock failure"),
            });
        }
        if self.held.load(Ordering::SeqCst) {
            return Ok(None);
        }
        self.held.store(true, Ordering::SeqCst);
        Ok(Some(Box::new(FakeGuard {
            held: Arc::clone(&self.held),
        })))
    }
}

//! Port for cross-process project initialization locking.
//!
//! Provides an OS-level file lock abstraction to prevent concurrent
//! `workhive workspace open` / `workhive init` processes from racing to initialize
//! the same project. The lock must be a cross-process OS lock (not an
//! in-process mutex) and must be managed via an RAII guard.

use camino::Utf8Path;

use crate::errors::ProjectInitError;

/// RAII guard that holds the project init lock.
///
/// When dropped, the lock is released. Implementations may keep the lock
/// file on disk to avoid inode split races.
pub trait ProjectInitLockGuard: Send {}

/// Port for acquiring cross-process project initialization locks.
///
/// Adapters must use OS file locks (e.g., `flock`, `LockFile`), not
/// in-process mutexes. The returned guard releases the lock on drop.
pub trait ProjectInitLockPort {
    /// Acquire an exclusive lock for project initialization at the given root.
    ///
    /// Returns an RAII guard that releases the lock on drop.
    /// Blocks until the lock is acquired or returns an error if the lock
    /// cannot be obtained.
    fn acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Box<dyn ProjectInitLockGuard>, ProjectInitError>;

    /// Try to acquire an exclusive lock without blocking.
    ///
    /// Returns `Ok(Some(guard))` if the lock was acquired, `Ok(None)` if
    /// another process holds it, or `Err` on I/O failure.
    fn try_acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Option<Box<dyn ProjectInitLockGuard>>, ProjectInitError>;
}

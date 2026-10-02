//! File-based cross-process lock adapter for project initialization.
//!
//! Uses `fslock` to acquire an OS-level file lock on `.workhive.init.lock`
//! in the project root. The lock file is placed **outside** `.workhive/`
//! so acquiring it never creates the `.workhive/` directory as a side effect.
//!
//! The RAII guard releases the OS lock on drop but does **not** delete the
//! lock file to avoid inode split races with concurrent processes.

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::ProjectInitError;
use workhive_core::ports::{ProjectInitLockGuard, ProjectInitLockPort};

const LOCK_FILE_NAME: &str = ".workhive.init.lock";

/// RAII guard wrapping an `fslock::LockFile`.
///
/// Releases the OS lock on drop. Does **not** remove the lock file;
/// removal would introduce inode split races when other processes are
/// waiting on the same path.
struct FsInitLockGuard {
    lock: fslock::LockFile,
    lock_path: Utf8PathBuf,
}

impl Drop for FsInitLockGuard {
    fn drop(&mut self) {
        if let Err(e) = self.lock.unlock() {
            tracing::warn!(path = %self.lock_path, error = %e, "failed to release init lock");
        } else {
            tracing::debug!(path = %self.lock_path, "released project init lock");
        }
    }
}

impl ProjectInitLockGuard for FsInitLockGuard {}

/// Real filesystem adapter for project initialization locks.
pub struct FsProjectInitLockAdapter;

impl FsProjectInitLockAdapter {
    pub fn new() -> Self {
        Self
    }

    fn lock_path(project_root: &Utf8Path) -> Utf8PathBuf {
        project_root.join(LOCK_FILE_NAME)
    }
}

impl Default for FsProjectInitLockAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectInitLockPort for FsProjectInitLockAdapter {
    fn acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Box<dyn ProjectInitLockGuard>, ProjectInitError> {
        let path = Self::lock_path(project_root);

        tracing::debug!(path = %path, "acquiring project init lock (blocking)");

        let mut lock = fslock::LockFile::open(path.as_std_path()).map_err(|source| {
            ProjectInitError::LockFailed {
                lock_path: path.clone(),
                source,
            }
        })?;

        lock.lock().map_err(|source| ProjectInitError::LockFailed {
            lock_path: path.clone(),
            source,
        })?;

        tracing::info!(path = %path, "acquired project init lock");

        Ok(Box::new(FsInitLockGuard {
            lock,
            lock_path: path,
        }))
    }

    fn try_acquire_init_lock(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Option<Box<dyn ProjectInitLockGuard>>, ProjectInitError> {
        let path = Self::lock_path(project_root);

        tracing::debug!(path = %path, "trying to acquire project init lock (non-blocking)");

        let mut lock = fslock::LockFile::open(path.as_std_path()).map_err(|source| {
            ProjectInitError::LockFailed {
                lock_path: path.clone(),
                source,
            }
        })?;

        let acquired = lock
            .try_lock()
            .map_err(|source| ProjectInitError::LockFailed {
                lock_path: path.clone(),
                source,
            })?;

        if acquired {
            tracing::info!(path = %path, "acquired project init lock (non-blocking)");
            Ok(Some(Box::new(FsInitLockGuard {
                lock,
                lock_path: path,
            })))
        } else {
            tracing::info!(path = %path, "project init lock held by another process");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn lock_file_lives_outside_workhive_dir() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsProjectInitLockAdapter::new();

        let _guard = adapter.acquire_init_lock(&root).unwrap();
        let lock_path = root.join(".workhive.init.lock");
        assert!(
            lock_path.as_std_path().exists(),
            "lock file must be at project root, not inside .workhive/"
        );
        assert!(
            !root.join(".workhive").as_std_path().exists(),
            "acquiring the lock must not create .workhive/"
        );
    }

    #[test]
    fn lock_file_persists_after_guard_drop() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsProjectInitLockAdapter::new();

        let guard = adapter.acquire_init_lock(&root).unwrap();
        let lock_path = root.join(".workhive.init.lock");
        drop(guard);
        assert!(
            lock_path.as_std_path().exists(),
            "lock file must not be deleted on drop"
        );
    }

    #[test]
    fn try_lock_succeeds_when_not_held() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsProjectInitLockAdapter::new();

        let guard = adapter.try_acquire_init_lock(&root).unwrap();
        assert!(guard.is_some());
    }

    #[test]
    fn try_lock_returns_none_when_held() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsProjectInitLockAdapter::new();

        let _guard1 = adapter.acquire_init_lock(&root).unwrap();
        let guard2 = adapter.try_acquire_init_lock(&root).unwrap();
        assert!(guard2.is_none());
    }
}

//! Index reconciliation infrastructure: state persistence, inter-process
//! locking, and revision tracking.
//!
//! The project index is a recoverable projection of trusted-root configuration.
//! This module provides the concrete filesystem implementations for the
//! reconciliation lifecycle defined in `workhive_core::domain::index_reconciliation`.
//!
//! Atomic index publication is performed through `ProjectIndexPort` methods
//! (`replace_all`, `publish_snapshot_conditional`, or individual mutations like
//! `upsert_project`), NOT through free functions that bypass the sanctioned port boundary.

use std::time::SystemTime;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::index_reconciliation::{
    ReconciliationRevision, ReconciliationState, RootsFingerprint,
};
use workhive_core::errors::IndexError;
use workhive_core::ports::{
    IndexReconciliationPort, ReconciliationLease, ReconciliationLeaseOutcome,
};

/// Filesystem-backed reconciliation persistence and inter-process coordinator.
#[derive(Debug, Default, Clone, Copy)]
pub struct FsIndexReconciliation;

impl FsIndexReconciliation {
    pub fn new() -> Self {
        Self
    }
}

/// Reconciliation state file (JSON). Sits beside index.json.
const STATE_FILE: &str = "reconciliation_state.json";

/// Tagged serde enum for reconciliation state status.
/// Replaces the error-prone `status: String` with optional fields pattern.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status")]
enum ReconciliationStatus {
    #[serde(rename = "updating")]
    Updating,
    #[serde(rename = "completed")]
    Completed { discovered_count: usize },
    #[serde(rename = "failed")]
    Failed { error_detail: String },
}

/// Persistent reconciliation state record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ReconciliationStateRecord {
    #[serde(flatten)]
    status: ReconciliationStatus,
    revision: u64,
    fingerprint: String,
    timestamp_epoch_ms: u64,
}

impl ReconciliationStateRecord {
    fn to_domain(&self, state_path: &Utf8Path) -> Result<ReconciliationState, IndexError> {
        let revision = ReconciliationRevision::try_from_persisted(self.revision).map_err(|_| {
            IndexError::Corrupted {
                path: state_path.to_owned(),
            }
        })?;
        let fingerprint = RootsFingerprint::from_str_raw(&self.fingerprint);
        let time =
            SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(self.timestamp_epoch_ms);
        match &self.status {
            ReconciliationStatus::Updating => Ok(ReconciliationState::Updating {
                revision,
                fingerprint,
                started_at: time,
            }),
            ReconciliationStatus::Completed { discovered_count } => {
                Ok(ReconciliationState::Completed {
                    revision,
                    fingerprint,
                    completed_at: time,
                    discovered_count: *discovered_count,
                })
            }
            ReconciliationStatus::Failed { error_detail } => Ok(ReconciliationState::Failed {
                revision,
                fingerprint,
                failed_at: time,
                error_detail: error_detail.clone(),
            }),
        }
    }

    fn from_domain(
        state: &ReconciliationState,
        state_path: &Utf8Path,
    ) -> Result<Option<Self>, IndexError> {
        match state {
            ReconciliationState::Idle => Ok(None),
            ReconciliationState::Updating {
                revision,
                fingerprint,
                started_at,
            } => Ok(Some(Self {
                status: ReconciliationStatus::Updating,
                revision: revision.value(),
                fingerprint: fingerprint.as_str().to_owned(),
                timestamp_epoch_ms: epoch_ms(*started_at, state_path)?,
            })),
            ReconciliationState::Completed {
                revision,
                fingerprint,
                completed_at,
                discovered_count,
            } => Ok(Some(Self {
                status: ReconciliationStatus::Completed {
                    discovered_count: *discovered_count,
                },
                revision: revision.value(),
                fingerprint: fingerprint.as_str().to_owned(),
                timestamp_epoch_ms: epoch_ms(*completed_at, state_path)?,
            })),
            ReconciliationState::Failed {
                revision,
                fingerprint,
                failed_at,
                error_detail,
            } => Ok(Some(Self {
                status: ReconciliationStatus::Failed {
                    error_detail: error_detail.clone(),
                },
                revision: revision.value(),
                fingerprint: fingerprint.as_str().to_owned(),
                timestamp_epoch_ms: epoch_ms(*failed_at, state_path)?,
            })),
        }
    }
}

/// Read the reconciliation state from the state file beside the index.
///
/// Returns `Ok(Idle)` only when the file does not exist (legitimate initial
/// state). Corrupted or permission-denied files return a typed error so
/// callers can distinguish real failures from absence.
fn read_reconciliation_state(config_dir: &Utf8Path) -> Result<ReconciliationState, IndexError> {
    let state_path = config_dir.join(STATE_FILE);
    let content = match std::fs::read_to_string(&state_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ReconciliationState::Idle);
        }
        Err(source) => {
            return Err(IndexError::ReadFailed {
                path: state_path,
                source,
            });
        }
    };
    let record: ReconciliationStateRecord =
        serde_json::from_str(&content).map_err(|_| IndexError::Corrupted {
            path: state_path.clone(),
        })?;
    record.to_domain(&state_path)
}

/// Write the reconciliation state atomically.
fn write_reconciliation_state(
    config_dir: &Utf8Path,
    state: &ReconciliationState,
) -> Result<(), IndexError> {
    let state_path = config_dir.join(STATE_FILE);
    if let Some(revision) = state.revision() {
        ReconciliationRevision::try_from_persisted(revision.value()).map_err(|_| {
            IndexError::Corrupted {
                path: state_path.clone(),
            }
        })?;
    }
    let record = match ReconciliationStateRecord::from_domain(state, &state_path)? {
        Some(r) => r,
        None => {
            match std::fs::remove_file(&state_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(IndexError::WriteFailed {
                        path: state_path,
                        source,
                    });
                }
            }
            return Ok(());
        }
    };
    let content = serde_json::to_string_pretty(&record).map_err(|_| IndexError::Corrupted {
        path: state_path.clone(),
    })?;
    let tmp = sibling_tmp_path(&state_path);
    std::fs::write(&tmp, content).map_err(|source| IndexError::WriteFailed {
        path: state_path.clone(),
        source,
    })?;
    std::fs::rename(&tmp, state_path.as_std_path()).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        IndexError::WriteFailed {
            path: state_path.to_owned(),
            source,
        }
    })?;
    Ok(())
}

/// Read the latest requested revision from a revision file.
///
/// Returns `Ok(None)` only when the file does not exist. Permission or
/// parse failures return typed errors.
fn read_latest_revision(
    config_dir: &Utf8Path,
) -> Result<Option<ReconciliationRevision>, IndexError> {
    let rev_path = config_dir.join("reconciliation_revision");
    let content = match std::fs::read_to_string(&rev_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IndexError::ReadFailed {
                path: rev_path,
                source,
            });
        }
    };
    let value = content
        .trim()
        .parse::<u64>()
        .map_err(|_| IndexError::Corrupted {
            path: rev_path.clone(),
        })?;
    let revision = ReconciliationRevision::try_from_persisted(value)
        .map_err(|_| IndexError::Corrupted { path: rev_path })?;
    Ok(Some(revision))
}

/// Write the latest requested revision atomically.
fn write_latest_revision(
    config_dir: &Utf8Path,
    revision: ReconciliationRevision,
) -> Result<(), IndexError> {
    let rev_path = config_dir.join("reconciliation_revision");
    ReconciliationRevision::try_from_persisted(revision.value()).map_err(|_| {
        IndexError::Corrupted {
            path: rev_path.clone(),
        }
    })?;
    let tmp = sibling_tmp_path(&rev_path);
    std::fs::write(&tmp, revision.value().to_string()).map_err(|source| {
        IndexError::WriteFailed {
            path: rev_path.clone(),
            source,
        }
    })?;
    std::fs::rename(&tmp, rev_path.as_std_path()).map_err(|source| {
        let _ = std::fs::remove_file(&tmp);
        IndexError::WriteFailed {
            path: rev_path.to_owned(),
            source,
        }
    })?;
    Ok(())
}

/// Outcome of attempting to acquire the reconciliation lock.
#[derive(Debug)]
enum LockOutcome {
    /// Lock acquired successfully.
    Acquired(ReconciliationLock),
    /// Another process holds the lock (legitimate contention).
    Contended,
}

/// Inter-process lock for reconciliation workers.
///
/// Uses `fslock` for cross-platform advisory file locking. Only one worker
/// at a time may hold the lock.
struct ReconciliationLock {
    lock: fslock::LockFile,
    lock_path: Utf8PathBuf,
}

impl std::fmt::Debug for ReconciliationLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReconciliationLock")
            .field("lock_path", &self.lock_path)
            .finish()
    }
}

impl ReconciliationLock {
    /// Try to acquire the reconciliation lock.
    ///
    /// Returns `Ok(Acquired(lock))` on success, `Ok(Contended)` when another
    /// process legitimately holds the lock, and `Err` for actual I/O failures
    /// (permission denied, directory missing, etc.).
    fn try_acquire(config_dir: &Utf8Path) -> Result<LockOutcome, IndexError> {
        let lock_path = config_dir.join("reconciliation.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            })?;
        }
        let mut lock = fslock::LockFile::open(lock_path.as_std_path()).map_err(|source| {
            IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            }
        })?;
        match lock.try_lock_with_pid() {
            Ok(true) => Ok(LockOutcome::Acquired(Self { lock, lock_path })),
            Ok(false) => Ok(LockOutcome::Contended),
            Err(source) => Err(IndexError::WriteFailed {
                path: lock_path,
                source,
            }),
        }
    }
}

impl Drop for ReconciliationLock {
    fn drop(&mut self) {
        let _ = self.lock.unlock();
    }
}

/// Scheduling-side lock for atomic read-decide-write of reconciliation revision.
///
/// Prevents concurrent mutations from reading the same revision and writing
/// identical next values.
struct SchedulingLock;

impl SchedulingLock {
    /// Try to acquire the scheduling lock without blocking.
    fn try_acquire(config_dir: &Utf8Path) -> Result<LockOutcome, IndexError> {
        let lock_path = config_dir.join("reconciliation_schedule.lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            })?;
        }
        let mut lock = fslock::LockFile::open(lock_path.as_std_path()).map_err(|source| {
            IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            }
        })?;
        match lock.try_lock_with_pid() {
            Ok(true) => Ok(LockOutcome::Acquired(ReconciliationLock {
                lock,
                lock_path,
            })),
            Ok(false) => Ok(LockOutcome::Contended),
            Err(source) => Err(IndexError::WriteFailed {
                path: lock_path,
                source,
            }),
        }
    }
}

impl ReconciliationLease for ReconciliationLock {}

impl IndexReconciliationPort for FsIndexReconciliation {
    fn read_state(&self, config_dir: &Utf8Path) -> Result<ReconciliationState, IndexError> {
        read_reconciliation_state(config_dir)
    }

    fn write_state(
        &self,
        config_dir: &Utf8Path,
        state: &ReconciliationState,
    ) -> Result<(), IndexError> {
        write_reconciliation_state(config_dir, state)
    }

    fn read_latest_revision(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<Option<ReconciliationRevision>, IndexError> {
        read_latest_revision(config_dir)
    }

    fn write_latest_revision(
        &self,
        config_dir: &Utf8Path,
        revision: ReconciliationRevision,
    ) -> Result<(), IndexError> {
        write_latest_revision(config_dir, revision)
    }

    fn try_acquire_worker_lease(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, IndexError> {
        Ok(map_lock_outcome(ReconciliationLock::try_acquire(
            config_dir,
        )?))
    }

    fn try_acquire_scheduling_lease(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, IndexError> {
        Ok(map_lock_outcome(SchedulingLock::try_acquire(config_dir)?))
    }
}

fn map_lock_outcome(outcome: LockOutcome) -> ReconciliationLeaseOutcome {
    match outcome {
        LockOutcome::Acquired(lease) => ReconciliationLeaseOutcome::Acquired(Box::new(lease)),
        LockOutcome::Contended => ReconciliationLeaseOutcome::Contended,
    }
}

fn sibling_tmp_path(path: &Utf8Path) -> std::path::PathBuf {
    let file_name = path.file_name().unwrap_or("tmp");
    let tmp_name = format!(".{file_name}.tmp.{}", std::process::id());
    path.parent()
        .map(|p| p.join(&tmp_name).into_std_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from(&tmp_name))
}

fn epoch_ms(time: SystemTime, path: &Utf8Path) -> Result<u64, IndexError> {
    let duration =
        time.duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| IndexError::Corrupted {
                path: path.to_owned(),
            })?;
    let millis = duration.as_millis();
    u64::try_from(millis).map_err(|_| IndexError::Corrupted {
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn index_reconciliation_infra_state_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        let state = ReconciliationState::Completed {
            revision: ReconciliationRevision::new(5),
            fingerprint: RootsFingerprint::from_trusted_roots(
                &workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ),
            ),
            completed_at: SystemTime::now(),
            discovered_count: 3,
        };
        adapter.write_state(&config_dir, &state).unwrap();
        let loaded = adapter.read_state(&config_dir).unwrap();
        assert!(loaded.is_completed());
        assert_eq!(loaded.revision(), Some(ReconciliationRevision::new(5)));
    }

    #[test]
    fn index_reconciliation_infra_state_missing_returns_idle() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        let loaded = adapter.read_state(&config_dir).unwrap();
        assert!(loaded.is_idle());
    }

    #[test]
    fn index_reconciliation_infra_state_corrupted_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        std::fs::write(tmp.path().join(STATE_FILE), "garbage").unwrap();
        let result = adapter.read_state(&config_dir);
        assert!(
            result.is_err(),
            "corrupted state file must produce a typed error, not Idle"
        );
    }

    #[test]
    fn index_reconciliation_infra_revision_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        adapter
            .write_latest_revision(&config_dir, ReconciliationRevision::new(42))
            .unwrap();
        let loaded = adapter.read_latest_revision(&config_dir).unwrap();
        assert_eq!(loaded, Some(ReconciliationRevision::new(42)));
    }

    #[test]
    fn index_reconciliation_infra_revision_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        assert_eq!(adapter.read_latest_revision(&config_dir).unwrap(), None);
    }

    #[test]
    fn index_reconciliation_infra_revision_corrupted_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        std::fs::write(tmp.path().join("reconciliation_revision"), "not_a_number").unwrap();
        let result = adapter.read_latest_revision(&config_dir);
        assert!(
            result.is_err(),
            "malformed revision file must produce a typed error, not None"
        );
    }

    #[test]
    fn index_reconciliation_infra_revision_zero_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        std::fs::write(tmp.path().join("reconciliation_revision"), "0").unwrap();
        assert!(adapter.read_latest_revision(&config_dir).is_err());
    }

    #[test]
    fn index_reconciliation_infra_revision_max_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        std::fs::write(
            tmp.path().join("reconciliation_revision"),
            u64::MAX.to_string(),
        )
        .unwrap();
        assert!(adapter.read_latest_revision(&config_dir).is_err());
    }

    #[test]
    fn index_reconciliation_infra_scheduling_lock_reports_contention() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let first = SchedulingLock::try_acquire(&config_dir).unwrap();
        assert!(matches!(first, LockOutcome::Acquired(_)));
        let second = SchedulingLock::try_acquire(&config_dir).unwrap();
        assert!(matches!(second, LockOutcome::Contended));
    }

    #[test]
    fn index_reconciliation_infra_lock_acquisition() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let outcome = ReconciliationLock::try_acquire(&config_dir).unwrap();
        assert!(
            matches!(outcome, LockOutcome::Acquired(_)),
            "first lock acquisition must succeed"
        );

        let outcome2 = ReconciliationLock::try_acquire(&config_dir).unwrap();
        assert!(
            matches!(outcome2, LockOutcome::Contended),
            "second lock while first held must report Contended"
        );
        drop(outcome);
    }

    #[test]
    fn index_reconciliation_infra_lock_released_on_drop() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        {
            let outcome = ReconciliationLock::try_acquire(&config_dir).unwrap();
            assert!(matches!(outcome, LockOutcome::Acquired(_)));
        }
        let outcome2 = ReconciliationLock::try_acquire(&config_dir).unwrap();
        assert!(
            matches!(outcome2, LockOutcome::Acquired(_)),
            "lock must be reacquirable after drop"
        );
    }

    #[test]
    fn index_reconciliation_infra_atomic_publish_through_port() {
        use workhive_core::ports::{ProjectIndexPort, ProjectSummary};
        let tmp = TempDir::new().unwrap();
        let index_path = Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
        let index = crate::project_index::JsonProjectIndex::with_path(index_path.clone());
        let projects = vec![
            ProjectSummary {
                project_name: "alpha".to_owned(),
                project_root: Utf8PathBuf::from("/dev/alpha"),
                config_path: Utf8PathBuf::from("/dev/alpha/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
            ProjectSummary {
                project_name: "beta".to_owned(),
                project_root: Utf8PathBuf::from("/dev/beta"),
                config_path: Utf8PathBuf::from("/dev/beta/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            },
        ];
        index.replace_all(&projects).unwrap();

        let loaded = index.list_projects().unwrap();
        assert_eq!(loaded.projects().len(), 2);
    }

    #[test]
    fn index_reconciliation_infra_publish_retains_prior_on_failure() {
        use workhive_core::ports::{ProjectIndexPort, ProjectSummary};
        let tmp = TempDir::new().unwrap();
        let index_path = Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
        let index = crate::project_index::JsonProjectIndex::with_path(index_path.clone());

        let initial = vec![ProjectSummary {
            project_name: "prior".to_owned(),
            project_root: Utf8PathBuf::from("/dev/prior"),
            config_path: Utf8PathBuf::from("/dev/prior/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        }];
        index.replace_all(&initial).unwrap();

        // Make the parent directory read-only to force a write failure on the
        // tmp file. This simulates a permission error mid-publication.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let parent = tmp.path();
            let perms = std::fs::Permissions::from_mode(0o555);
            std::fs::set_permissions(parent, perms).unwrap();
            let result = index.replace_all(&[]);
            // Restore permissions before assertions so cleanup works.
            let restore = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(parent, restore).unwrap();
            assert!(result.is_err(), "write to read-only dir must fail");
            // Prior index must still be readable.
            let loaded = index.list_projects().unwrap();
            assert_eq!(
                loaded.projects().len(),
                1,
                "failed publication must retain prior valid index"
            );
        }
    }

    #[test]
    fn index_reconciliation_infra_unknown_status_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        let record = serde_json::json!({
            "status": "unknown_state",
            "revision": 1,
            "fingerprint": "abc",
            "timestamp_epoch_ms": 1000
        });
        std::fs::write(
            tmp.path().join(STATE_FILE),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        let result = adapter.read_state(&config_dir);
        assert!(result.is_err(), "unknown status must return a typed error");
    }

    #[test]
    fn index_reconciliation_infra_completed_missing_count_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        let record = serde_json::json!({
            "status": "completed",
            "revision": 2,
            "fingerprint": "def",
            "timestamp_epoch_ms": 2000
        });
        std::fs::write(
            tmp.path().join(STATE_FILE),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        let result = adapter.read_state(&config_dir);
        assert!(
            result.is_err(),
            "completed without discovered_count must return error"
        );
    }

    #[test]
    fn index_reconciliation_infra_failed_missing_detail_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();
        let record = serde_json::json!({
            "status": "failed",
            "revision": 3,
            "fingerprint": "ghi",
            "timestamp_epoch_ms": 3000
        });
        std::fs::write(
            tmp.path().join(STATE_FILE),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();
        let result = adapter.read_state(&config_dir);
        assert!(
            result.is_err(),
            "failed without error_detail must return error"
        );
    }

    #[test]
    fn index_reconciliation_infra_idle_delete_non_notfound_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        // Write a valid state file first so there is something to remove.
        let state = ReconciliationState::Completed {
            revision: ReconciliationRevision::new(1),
            fingerprint: RootsFingerprint::from_trusted_roots(
                &workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ),
            ),
            completed_at: SystemTime::now(),
            discovered_count: 0,
        };
        adapter.write_state(&config_dir, &state).unwrap();

        // Make the config directory read-only so remove_file fails with a
        // non-NotFound error (PermissionDenied on Unix).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o555);
            std::fs::set_permissions(tmp.path(), perms).unwrap();
            let result = adapter.write_state(&config_dir, &ReconciliationState::Idle);
            // Restore permissions before assertions so TempDir cleanup works.
            let restore = std::fs::Permissions::from_mode(0o755);
            std::fs::set_permissions(tmp.path(), restore).unwrap();
            assert!(
                result.is_err(),
                "Idle state delete with non-NotFound error must return typed error"
            );
        }
    }

    #[test]
    fn index_reconciliation_infra_revision_checked_next_exhaustion() {
        // A persisted revision of u64::MAX - 1 cannot produce a valid next.
        let revision = ReconciliationRevision::new(u64::MAX - 1);
        let result = revision.checked_next();
        assert!(
            result.is_err(),
            "checked_next at u64::MAX - 1 must return Exhausted, not wrap"
        );
    }

    // ─── Malformed/roundtrip tests through the adapter ────────────────────

    #[test]
    fn index_reconciliation_port_state_roundtrip_through_adapter() {
        use workhive_core::ports::IndexReconciliationPort;

        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        // Test all three state variants through the port interface
        let updating = ReconciliationState::Updating {
            revision: ReconciliationRevision::new(1),
            fingerprint: RootsFingerprint::from_trusted_roots(
                &workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ),
            ),
            started_at: SystemTime::now(),
        };
        adapter.write_state(&config_dir, &updating).unwrap();
        let loaded = adapter.read_state(&config_dir).unwrap();
        assert!(loaded.is_updating());

        let completed = ReconciliationState::Completed {
            revision: ReconciliationRevision::new(2),
            fingerprint: RootsFingerprint::from_trusted_roots(
                &workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ),
            ),
            completed_at: SystemTime::now(),
            discovered_count: 5,
        };
        adapter.write_state(&config_dir, &completed).unwrap();
        let loaded = adapter.read_state(&config_dir).unwrap();
        assert!(loaded.is_completed());
        if let ReconciliationState::Completed {
            discovered_count, ..
        } = loaded
        {
            assert_eq!(discovered_count, 5);
        } else {
            panic!("expected Completed state");
        }

        let failed = ReconciliationState::Failed {
            revision: ReconciliationRevision::new(3),
            fingerprint: RootsFingerprint::from_trusted_roots(
                &workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ),
            ),
            failed_at: SystemTime::now(),
            error_detail: "test error".to_owned(),
        };
        adapter.write_state(&config_dir, &failed).unwrap();
        let loaded = adapter.read_state(&config_dir).unwrap();
        assert!(loaded.is_failed());
        if let ReconciliationState::Failed { error_detail, .. } = loaded {
            assert_eq!(error_detail, "test error");
        } else {
            panic!("expected Failed state");
        }
    }

    #[test]
    fn index_reconciliation_port_malformed_tagged_enum_missing_variant_field() {
        use workhive_core::ports::IndexReconciliationPort;

        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        // Write a completed state without discovered_count field
        let record = serde_json::json!({
            "status": "completed",
            "revision": 1,
            "fingerprint": "abc",
            "timestamp_epoch_ms": 1000
        });
        std::fs::write(
            tmp.path().join(STATE_FILE),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();

        let result = adapter.read_state(&config_dir);
        assert!(
            result.is_err(),
            "completed state without discovered_count must fail through port"
        );
    }

    #[test]
    fn index_reconciliation_port_malformed_tagged_enum_unknown_status() {
        use workhive_core::ports::IndexReconciliationPort;

        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        // Write a record with unknown status value
        let record = serde_json::json!({
            "status": "bogus_status",
            "revision": 1,
            "fingerprint": "abc",
            "timestamp_epoch_ms": 1000
        });
        std::fs::write(
            tmp.path().join(STATE_FILE),
            serde_json::to_string(&record).unwrap(),
        )
        .unwrap();

        let result = adapter.read_state(&config_dir);
        assert!(
            result.is_err(),
            "unknown status value must fail through port"
        );
    }

    #[test]
    fn index_reconciliation_port_revision_roundtrip_through_adapter() {
        use workhive_core::ports::IndexReconciliationPort;

        let tmp = TempDir::new().unwrap();
        let config_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let adapter = FsIndexReconciliation::new();

        let rev = ReconciliationRevision::new(42);
        adapter.write_latest_revision(&config_dir, rev).unwrap();
        let loaded = adapter.read_latest_revision(&config_dir).unwrap();
        assert_eq!(loaded, Some(rev));

        // Test None case (no file)
        let tmp2 = TempDir::new().unwrap();
        let config_dir2 = Utf8PathBuf::try_from(tmp2.path().to_path_buf()).unwrap();
        let loaded2 = adapter.read_latest_revision(&config_dir2).unwrap();
        assert_eq!(loaded2, None);
    }
}

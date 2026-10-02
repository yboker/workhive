//! JSON-based global project index adapter.
//!
//! Index file: `~/.config/workhive/index.json`
//!
//! All mutations (upsert, remove, clear, rebuild) and snapshot publication
//! share an adapter-owned inter-process writer lease to prevent lost updates
//! between foreground operations and background reconciliation workers.
//!
//! Conditional snapshot publication uses optimistic concurrency: capture a
//! content-derived token before discovery, publish only if the token still
//! matches under the writer lease.

use camino::Utf8PathBuf;
use sha2::{Digest, Sha256};
use workhive_core::errors::IndexError;
use workhive_core::ports::{
    IndexSnapshotToken, IndexState, PairedIndexState, ProjectIndexPort, ProjectSummary,
    SnapshotPublishOutcome,
};

const INDEX_FILE: &str = "index.json";
const INDEX_LOCK_FILE: &str = "index.lock";

/// File-based project index adapter.
pub struct JsonProjectIndex {
    index_path: Utf8PathBuf,
    lock_path: Utf8PathBuf,
}

impl JsonProjectIndex {
    pub fn new() -> Result<Self, IndexError> {
        let config_dir =
            crate::config::resolve_global_config_dir().map_err(|_| IndexError::ReadFailed {
                path: Utf8PathBuf::from("<global-config-dir>/index.json"),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "failed to resolve global config directory (HOME not set)",
                ),
            })?;
        let index_path = config_dir.join(INDEX_FILE);
        let lock_path = config_dir.join(INDEX_LOCK_FILE);
        Ok(Self {
            index_path,
            lock_path,
        })
    }

    /// Create with an explicit path (for testing).
    pub fn with_path(path: Utf8PathBuf) -> Self {
        let parent = path.parent().unwrap_or_else(|| camino::Utf8Path::new("."));
        let lock_path = parent.join(INDEX_LOCK_FILE);
        Self {
            index_path: path,
            lock_path,
        }
    }

    /// Acquire the writer lease for index mutations.
    fn acquire_writer_lease(&self) -> Result<IndexWriterLease, IndexError> {
        IndexWriterLease::try_acquire(&self.lock_path)
    }

    fn read_all(&self) -> Result<IndexState, IndexError> {
        let (state, _token) = self.read_paired_state_internal()?;
        Ok(state)
    }

    /// Read the raw project list for internal mutation operations.
    ///
    /// Unlike `read_all`, this always returns a `Vec` — treating a missing
    /// index file as empty. Used by `upsert`, `remove`, and `rebuild`.
    fn read_all_vec(&self) -> Result<Vec<ProjectSummary>, IndexError> {
        Ok(self.read_all()?.into_projects())
    }

    /// Internal single-read primitive that returns both state and token from one file read.
    ///
    /// This is the core primitive that ensures state and token cannot diverge.
    /// All other read operations delegate to this.
    fn read_paired_state_internal(&self) -> Result<(IndexState, IndexSnapshotToken), IndexError> {
        // Single read attempt - no separate exists() check
        match std::fs::read(&self.index_path) {
            Ok(content) => {
                // Parse JSON from the same bytes
                let projects: Vec<ProjectSummary> =
                    serde_json::from_slice(&content).map_err(|_| IndexError::Corrupted {
                        path: self.index_path.clone(),
                    })?;

                // Compute SHA-256 digest from the same bytes
                let mut hasher = Sha256::new();
                hasher.update(&content);
                let result = hasher.finalize();
                let mut digest = [0u8; 32];
                digest.copy_from_slice(&result);
                let token = IndexSnapshotToken::from_digest(digest);

                Ok((IndexState::Loaded(projects), token))
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist - return NotFound state and Missing token
                Ok((IndexState::NotFound, IndexSnapshotToken::missing()))
            }
            Err(source) => {
                // Other I/O error
                Err(IndexError::ReadFailed {
                    path: self.index_path.clone(),
                    source,
                })
            }
        }
    }

    /// Compute the snapshot token from the current index content.
    ///
    /// Returns a distinguished missing-index token if the file doesn't exist.
    /// The token is derived by hashing the exact persisted bytes, ensuring
    /// token and content cannot diverge.
    ///
    /// This method reads the file independently of JSON parsing, so it can
    /// compute a token even from corrupted content.
    fn compute_snapshot_token(&self) -> Result<IndexSnapshotToken, IndexError> {
        match std::fs::read(&self.index_path) {
            Ok(content) => {
                let mut hasher = Sha256::new();
                hasher.update(&content);
                let result = hasher.finalize();
                let mut digest = [0u8; 32];
                digest.copy_from_slice(&result);
                Ok(IndexSnapshotToken::from_digest(digest))
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                Ok(IndexSnapshotToken::missing())
            }
            Err(source) => Err(IndexError::ReadFailed {
                path: self.index_path.clone(),
                source,
            }),
        }
    }

    /// Atomically write all projects.
    ///
    /// This is the centralized write path for all mutations and snapshot
    /// publication. The caller must hold the writer lease.
    fn write_all_atomic(&self, projects: &[ProjectSummary]) -> Result<(), IndexError> {
        use std::io::Write;
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::WriteFailed {
                path: self.index_path.clone(),
                source,
            })?;
        }
        let tmp_name = format!(".index.json.tmp.{}", std::process::id());
        let tmp_path = self
            .index_path
            .parent()
            .map(|p| p.join(&tmp_name))
            .unwrap_or_else(|| Utf8PathBuf::from(&tmp_name));
        let content =
            serde_json::to_string_pretty(projects).map_err(|_| IndexError::Corrupted {
                path: self.index_path.clone(),
            })?;
        {
            let mut file = std::fs::File::create(tmp_path.as_std_path()).map_err(|source| {
                let _ = std::fs::remove_file(tmp_path.as_std_path());
                IndexError::WriteFailed {
                    path: self.index_path.clone(),
                    source,
                }
            })?;
            file.write_all(content.as_bytes()).map_err(|source| {
                let _ = std::fs::remove_file(tmp_path.as_std_path());
                IndexError::WriteFailed {
                    path: self.index_path.clone(),
                    source,
                }
            })?;
            file.sync_all().map_err(|source| {
                let _ = std::fs::remove_file(tmp_path.as_std_path());
                IndexError::WriteFailed {
                    path: self.index_path.clone(),
                    source,
                }
            })?;
        }
        std::fs::rename(tmp_path.as_std_path(), self.index_path.as_std_path()).map_err(
            |source| {
                let _ = std::fs::remove_file(tmp_path.as_std_path());
                IndexError::WriteFailed {
                    path: self.index_path.clone(),
                    source,
                }
            },
        )?;

        tracing::debug!(
            path = %self.index_path,
            count = projects.len(),
            "index written atomically"
        );
        Ok(())
    }
}

/// Inter-process writer lease for index mutations.
///
/// Prevents concurrent mutations between foreground operations (init, open,
/// reindex) and background reconciliation workers. All mutations and snapshot
/// publication share this lease.
///
/// The underlying `fslock::LockFile` automatically releases the lock when dropped.
struct IndexWriterLease {
    _lock: fslock::LockFile,
}

impl IndexWriterLease {
    /// Try to acquire the writer lease with bounded retries.
    fn try_acquire(lock_path: &Utf8PathBuf) -> Result<Self, IndexError> {
        let mut lock = fslock::LockFile::open(lock_path.as_std_path()).map_err(|source| {
            IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            }
        })?;

        // Retry loop with exponential backoff to handle transient contention
        // from background workers or other processes.
        const MAX_ATTEMPTS: usize = 10;
        let mut delay_ms = 10;

        for attempt in 0..MAX_ATTEMPTS {
            match lock.try_lock_with_pid() {
                Ok(true) => return Ok(Self { _lock: lock }),
                Ok(false) => {
                    if attempt < MAX_ATTEMPTS - 1 {
                        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                        delay_ms *= 2; // Exponential backoff
                    }
                }
                Err(source) => {
                    return Err(IndexError::WriteFailed {
                        path: lock_path.clone(),
                        source,
                    });
                }
            }
        }

        // All retries exhausted
        Err(IndexError::Exhausted {
            attempts: MAX_ATTEMPTS,
        })
    }

    /// Inner acquisition function with explicit policy and wait callback.
    /// Used for deterministic testing.
    #[cfg(test)]
    fn try_acquire_with_policy<F>(
        lock_path: &Utf8PathBuf,
        max_attempts: usize,
        mut wait: F,
    ) -> Result<Self, IndexError>
    where
        F: FnMut(usize),
    {
        let mut lock = fslock::LockFile::open(lock_path.as_std_path()).map_err(|source| {
            IndexError::WriteFailed {
                path: lock_path.clone(),
                source,
            }
        })?;

        for attempt in 0..max_attempts {
            match lock.try_lock_with_pid() {
                Ok(true) => return Ok(Self { _lock: lock }),
                Ok(false) => {
                    if attempt < max_attempts - 1 {
                        wait(attempt);
                    }
                }
                Err(source) => {
                    return Err(IndexError::WriteFailed {
                        path: lock_path.clone(),
                        source,
                    });
                }
            }
        }

        Err(IndexError::Exhausted {
            attempts: max_attempts,
        })
    }
}

impl ProjectIndexPort for JsonProjectIndex {
    fn list_projects(&self) -> Result<IndexState, IndexError> {
        self.read_all()
    }

    fn read_paired_state(&self) -> Result<PairedIndexState, IndexError> {
        let (state, token) = self.read_paired_state_internal()?;
        Ok(PairedIndexState { state, token })
    }

    fn capture_snapshot_token(&self) -> Result<IndexSnapshotToken, IndexError> {
        self.compute_snapshot_token()
    }

    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError> {
        let _lease = self.acquire_writer_lease()?;
        let mut projects = self.read_all_vec()?;
        projects.retain(|p| p.project_root != summary.project_root);
        projects.push(summary.clone());
        self.write_all_atomic(&projects)
    }

    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError> {
        let _lease = self.acquire_writer_lease()?;
        let mut projects = self.read_all_vec()?;
        projects.retain(|p| &p.project_root != project_root);
        self.write_all_atomic(&projects)
    }

    fn clear(&self) -> Result<(), IndexError> {
        let _lease = self.acquire_writer_lease()?;
        self.write_all_atomic(&[])
    }

    fn replace_all(&self, projects: &[ProjectSummary]) -> Result<(), IndexError> {
        let _lease = self.acquire_writer_lease()?;
        self.write_all_atomic(projects)
    }

    fn publish_snapshot_conditional(
        &self,
        projects: &[ProjectSummary],
        expected_token: IndexSnapshotToken,
    ) -> Result<SnapshotPublishOutcome, IndexError> {
        let _lease = self.acquire_writer_lease()?;
        let current_token = self.compute_snapshot_token()?;

        if current_token != expected_token {
            return Ok(SnapshotPublishOutcome::Stale);
        }

        self.write_all_atomic(projects)?;
        Ok(SnapshotPublishOutcome::Published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_index(tmp: &TempDir) -> JsonProjectIndex {
        let path = Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
        JsonProjectIndex::with_path(path)
    }

    fn sample_project(name: &str) -> ProjectSummary {
        ProjectSummary {
            project_name: name.to_owned(),
            project_root: Utf8PathBuf::from(format!("/tmp/{name}")),
            config_path: Utf8PathBuf::from(format!("/tmp/{name}/.workhive/config.toml")),
            last_opened_at: None,
            last_updated_at: None,
        }
    }

    /// Token capture on missing index returns distinguished missing token.
    #[test]
    fn token_capture_missing_index_returns_missing_token() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        let token = idx.capture_snapshot_token().unwrap();
        assert!(
            token.is_missing(),
            "missing index must return missing token"
        );
        assert!(matches!(token, IndexSnapshotToken::Missing));
    }

    /// Token is derived from exact persisted bytes, not separate metadata.
    #[test]
    fn token_derived_from_content_bytes_no_separate_metadata() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Write initial content
        let project1 = sample_project("alpha");
        idx.upsert_project(&project1).unwrap();

        // Capture token
        let token1 = idx.capture_snapshot_token().unwrap();
        assert!(
            !token1.is_missing(),
            "token must not be missing after write"
        );

        // Verify token changes when content changes
        let project2 = sample_project("beta");
        idx.upsert_project(&project2).unwrap();

        let token2 = idx.capture_snapshot_token().unwrap();
        assert_ne!(token1, token2, "token must change when content changes");

        // Verify no separate metadata file exists
        let generation_path = tmp.path().join("index.generation");
        assert!(
            !generation_path.exists(),
            "no separate generation metadata file must exist"
        );
    }

    /// Token capture after write/read cycle is consistent.
    #[test]
    fn token_capture_consistent_after_write_read_cycle() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        let project = sample_project("test");
        idx.upsert_project(&project).unwrap();

        // Capture token multiple times
        let token1 = idx.capture_snapshot_token().unwrap();
        let token2 = idx.capture_snapshot_token().unwrap();
        let token3 = idx.capture_snapshot_token().unwrap();

        assert_eq!(token1, token2, "token must be consistent across reads");
        assert_eq!(token2, token3, "token must be consistent across reads");

        // Verify the index file is valid JSON
        let content = std::fs::read_to_string(&idx.index_path).unwrap();
        let parsed: Result<Vec<ProjectSummary>, _> = serde_json::from_str(&content);
        assert!(parsed.is_ok(), "index must be valid JSON");
    }

    /// Conditional publication with stale token returns Stale.
    #[test]
    fn conditional_publish_stale_token_returns_stale() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Capture initial token (missing)
        let initial_token = idx.capture_snapshot_token().unwrap();
        assert!(initial_token.is_missing());

        // Write content (changes token)
        let project1 = sample_project("alpha");
        idx.upsert_project(&project1).unwrap();

        // Try to publish with stale (missing) token
        let project2 = sample_project("beta");
        let outcome = idx
            .publish_snapshot_conditional(&[project2], initial_token)
            .unwrap();

        assert_eq!(
            outcome,
            SnapshotPublishOutcome::Stale,
            "publication with stale token must return Stale"
        );

        // Verify original content is preserved
        let state = idx.list_projects().unwrap();
        let projects = state.projects();
        assert_eq!(projects.len(), 1, "only original project must remain");
        assert_eq!(projects[0].project_name, "alpha");
    }

    /// Conditional publication with current token succeeds.
    #[test]
    fn conditional_publish_current_token_succeeds() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Write initial content
        let project1 = sample_project("alpha");
        idx.upsert_project(&project1).unwrap();

        // Capture current token
        let current_token = idx.capture_snapshot_token().unwrap();

        // Publish with current token
        let project2 = sample_project("beta");
        let outcome = idx
            .publish_snapshot_conditional(&[project1.clone(), project2], current_token)
            .unwrap();

        assert_eq!(
            outcome,
            SnapshotPublishOutcome::Published,
            "publication with current token must succeed"
        );

        // Verify new content
        let state = idx.list_projects().unwrap();
        let projects = state.projects();
        assert_eq!(projects.len(), 2, "both projects must be present");
    }

    /// Token and content cannot diverge.
    #[test]
    fn token_and_content_cannot_diverge() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Write content
        let project = sample_project("test");
        idx.upsert_project(&project).unwrap();

        // Capture token
        let token = idx.capture_snapshot_token().unwrap();

        // Manually modify the index file (simulate corruption)
        std::fs::write(&idx.index_path, "corrupted content").unwrap();

        // Capture token again - it must be different because content changed
        let new_token = idx.capture_snapshot_token().unwrap();
        assert_ne!(
            token, new_token,
            "token must change when content changes, even if corrupted"
        );

        // Note: the new token will be a hash of "corrupted content", not a valid index
        // This proves token is derived from bytes, not from parsed content
    }

    /// Deterministic concurrency - stale publication with channels
    #[test]
    fn concurrent_stale_publication_with_channels() {
        use std::sync::mpsc::channel;
        use std::thread;
        use std::time::Duration;

        let tmp = TempDir::new().unwrap();
        let idx1 = test_index(&tmp);
        let idx2 = test_index(&tmp); // Second handle to same index

        // Initial setup
        let project1 = sample_project("initial");
        idx1.upsert_project(&project1).unwrap();

        // Worker captures token
        let worker_token = idx1.capture_snapshot_token().unwrap();

        // Channels for coordination
        let (candidate_ready_tx, candidate_ready_rx) = channel();
        let (foreground_done_tx, foreground_done_rx) = channel();
        let (worker_release_tx, worker_release_rx) = channel();

        // Foreground thread: upsert after worker captures token
        let foreground_handle = thread::spawn(move || {
            // Wait for worker to signal candidate ready
            candidate_ready_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();

            // Foreground upsert
            let project2 = sample_project("foreground");
            idx2.upsert_project(&project2).unwrap();

            // Signal foreground done
            foreground_done_tx.send(()).unwrap();

            // Wait for worker release
            worker_release_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        });

        // Worker thread: attempt publication
        let worker_handle = thread::spawn(move || {
            // Signal candidate ready
            candidate_ready_tx.send(()).unwrap();

            // Wait for foreground to complete
            foreground_done_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();

            // Attempt publication with stale token
            let project3 = sample_project("worker");
            let outcome = idx1
                .publish_snapshot_conditional(&[project1.clone(), project3.clone()], worker_token)
                .unwrap();

            // Must be Stale
            assert_eq!(
                outcome,
                SnapshotPublishOutcome::Stale,
                "publication with stale token must return Stale"
            );

            // Verify foreground entry remains
            let state = idx1.list_projects().unwrap();
            let projects = state.projects();
            assert_eq!(projects.len(), 2, "must have initial + foreground");
            assert!(
                projects.iter().any(|p| p.project_name == "foreground"),
                "foreground entry must remain"
            );

            // Now capture fresh token and publish successfully
            let fresh_token = idx1.capture_snapshot_token().unwrap();
            let outcome2 = idx1
                .publish_snapshot_conditional(
                    &[project1, sample_project("foreground"), project3],
                    fresh_token,
                )
                .unwrap();

            assert_eq!(
                outcome2,
                SnapshotPublishOutcome::Published,
                "publication with fresh token must succeed"
            );

            // Release foreground thread
            worker_release_tx.send(()).unwrap();
        });

        foreground_handle.join().unwrap();
        worker_handle.join().unwrap();

        // Final verification
        let idx3 = test_index(&tmp);
        let state = idx3.list_projects().unwrap();
        let projects = state.projects();
        assert_eq!(projects.len(), 3, "must have all three projects");
    }

    /// Clear-first production test
    #[test]
    fn clear_first_destructive_replacement() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Setup: existing projects
        let project1 = sample_project("existing1");
        let project2 = sample_project("existing2");
        idx.upsert_project(&project1).unwrap();
        idx.upsert_project(&project2).unwrap();

        // Verify initial state
        let state = idx.list_projects().unwrap();
        assert_eq!(state.projects().len(), 2);

        // Clear-first: replace with new set
        let new_project = sample_project("new");
        idx.replace_all(std::slice::from_ref(&new_project)).unwrap();

        // Verify destructive replacement
        let state = idx.list_projects().unwrap();
        let projects = state.projects();
        assert_eq!(projects.len(), 1, "clear-first must replace all");
        assert_eq!(projects[0].project_name, "new");
        assert!(
            !projects
                .iter()
                .any(|p| p.project_name.starts_with("existing")),
            "existing projects must be gone"
        );
    }

    /// IndexError::Exhausted variant exists with correct structure
    #[test]
    fn index_error_exhausted_variant_exists() {
        // Verify the Exhausted error variant exists and has the correct structure
        let error = IndexError::Exhausted { attempts: 10 };
        assert_eq!(
            format!("{}", error),
            "index writer lease acquisition exhausted after 10 attempts"
        );

        // Verify it can be created with different attempt counts
        let error5 = IndexError::Exhausted { attempts: 5 };
        assert_eq!(
            format!("{}", error5),
            "index writer lease acquisition exhausted after 5 attempts"
        );
    }

    /// Paired state read returns consistent state and token
    #[test]
    fn paired_state_read_returns_consistent_state_and_token() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Write some content
        let project1 = sample_project("alpha");
        let project2 = sample_project("beta");
        idx.upsert_project(&project1).unwrap();
        idx.upsert_project(&project2).unwrap();

        // Read paired state
        let paired = idx.read_paired_state().unwrap();

        // Verify state and token are from the same read
        assert_eq!(paired.state.projects().len(), 2);
        assert!(!paired.token.is_missing());

        // Token should match what we'd get from capture_snapshot_token
        let token = idx.capture_snapshot_token().unwrap();
        assert_eq!(
            paired.token, token,
            "paired token must match captured token"
        );

        // Modify and verify token changes
        let project3 = sample_project("gamma");
        idx.upsert_project(&project3).unwrap();

        let new_paired = idx.read_paired_state().unwrap();
        assert_ne!(
            paired.token, new_paired.token,
            "token must change after modification"
        );
        assert_eq!(new_paired.state.projects().len(), 3);
    }

    /// Conditional publish with missing token works for new index
    #[test]
    fn conditional_publish_with_missing_token_creates_new_index() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Index doesn't exist yet
        assert!(!idx.index_path.exists());

        // Capture missing token
        let missing_token = idx.capture_snapshot_token().unwrap();
        assert!(missing_token.is_missing());

        // Publish with missing token should succeed
        let project = sample_project("first");
        let outcome = idx
            .publish_snapshot_conditional(std::slice::from_ref(&project), missing_token)
            .unwrap();

        assert_eq!(
            outcome,
            SnapshotPublishOutcome::Published,
            "publication with missing token must succeed for new index"
        );

        // Verify index was created
        assert!(idx.index_path.exists());
        let state = idx.list_projects().unwrap();
        assert_eq!(state.projects().len(), 1);
        assert_eq!(state.projects()[0].project_name, "first");
    }

    /// Multiple conditional publishes in sequence work correctly
    #[test]
    fn multiple_conditional_publishes_in_sequence() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // First publish with missing token
        let token1 = idx.capture_snapshot_token().unwrap();
        assert!(token1.is_missing());

        let project1 = sample_project("first");
        let outcome1 = idx
            .publish_snapshot_conditional(std::slice::from_ref(&project1), token1.clone())
            .unwrap();
        assert_eq!(outcome1, SnapshotPublishOutcome::Published);

        // Second publish with new token
        let token2 = idx.capture_snapshot_token().unwrap();
        assert!(!token2.is_missing());
        assert_ne!(token1, token2);

        let project2 = sample_project("second");
        let outcome2 = idx
            .publish_snapshot_conditional(&[project1.clone(), project2.clone()], token2.clone())
            .unwrap();
        assert_eq!(outcome2, SnapshotPublishOutcome::Published);

        // Third publish with current token
        let token3 = idx.capture_snapshot_token().unwrap();
        assert_ne!(token2, token3);

        let project3 = sample_project("third");
        let outcome3 = idx
            .publish_snapshot_conditional(&[project1, project2, project3], token3)
            .unwrap();
        assert_eq!(outcome3, SnapshotPublishOutcome::Published);

        // Verify final state
        let state = idx.list_projects().unwrap();
        assert_eq!(state.projects().len(), 3);
    }

    /// Step 1: Single-read invariant test
    #[test]
    fn paired_read_uses_single_file_read() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        // Write some content
        let project1 = sample_project("alpha");
        let project2 = sample_project("beta");
        idx.upsert_project(&project1).unwrap();
        idx.upsert_project(&project2).unwrap();

        // Read paired state
        let paired = idx.read_paired_state().unwrap();

        // Verify the token matches what we'd compute from the same content
        // This proves they came from the same read
        let content = std::fs::read(&idx.index_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let result = hasher.finalize();
        let mut expected_digest = [0u8; 32];
        expected_digest.copy_from_slice(&result);
        let expected_token = IndexSnapshotToken::from_digest(expected_digest);

        assert_eq!(
            paired.token, expected_token,
            "paired token must match token computed from same file content"
        );

        // Also verify the state is correct
        assert_eq!(paired.state.projects().len(), 2);
        assert!(!paired.token.is_missing());
    }

    /// Step 4: Lease acquisition tests

    #[test]
    fn lease_acquire_immediate_success() {
        let tmp = TempDir::new().unwrap();
        let lock_path = Utf8PathBuf::try_from(tmp.path().join("test.lock")).unwrap();

        let mut wait_calls = Vec::new();
        let result = IndexWriterLease::try_acquire_with_policy(&lock_path, 3, |attempt| {
            wait_calls.push(attempt)
        });

        assert!(result.is_ok(), "should acquire immediately");
        assert!(wait_calls.is_empty(), "no waits on immediate success");
    }

    #[test]
    fn lease_acquire_exhaustion_exact_attempts() {
        use std::thread;

        let tmp = TempDir::new().unwrap();
        let lock_path = Utf8PathBuf::try_from(tmp.path().join("test.lock")).unwrap();

        // Hold the lease in another thread
        let holder_lock_path = lock_path.clone();
        let _holder = thread::spawn(move || {
            let _lease = IndexWriterLease::try_acquire(&holder_lock_path).unwrap();
            // Hold for longer than the test timeout
            thread::sleep(std::time::Duration::from_secs(10));
        });

        // Give holder time to acquire
        thread::sleep(std::time::Duration::from_millis(50));

        // Try to acquire with limited attempts
        let mut wait_calls = Vec::new();
        let result = IndexWriterLease::try_acquire_with_policy(&lock_path, 3, |attempt| {
            wait_calls.push(attempt)
        });

        assert!(result.is_err(), "should fail after exhaustion");
        match result {
            Err(IndexError::Exhausted { attempts }) => {
                assert_eq!(attempts, 3, "should report exact attempt count");
            }
            _ => panic!("expected Exhausted error"),
        }

        // Should have waited 2 times (attempts 0 and 1, not after attempt 2)
        assert_eq!(wait_calls.len(), 2, "should wait between attempts");
        assert_eq!(
            wait_calls,
            vec![0, 1],
            "wait calls should be for attempts 0 and 1"
        );

        // Verify no index file was written (lease acquisition doesn't write index)
        let index_path = tmp.path().join("index.json");
        assert!(
            !index_path.exists(),
            "lease acquisition must not create index file"
        );
    }

    #[test]
    fn lease_acquire_io_error_immediate() {
        // Try to acquire from a non-existent directory
        let lock_path = Utf8PathBuf::from("/nonexistent/path/test.lock");

        let mut wait_calls = Vec::new();
        let result = IndexWriterLease::try_acquire_with_policy(&lock_path, 3, |attempt| {
            wait_calls.push(attempt)
        });

        assert!(result.is_err(), "should fail immediately on I/O error");
        match result {
            Err(IndexError::WriteFailed { path, source }) => {
                assert_eq!(path, lock_path, "error should include path");
                assert!(
                    source.kind() == std::io::ErrorKind::NotFound
                        || source.kind() == std::io::ErrorKind::PermissionDenied,
                    "should be I/O error"
                );
            }
            _ => panic!("expected WriteFailed error"),
        }

        assert!(wait_calls.is_empty(), "no waits on immediate I/O error");
    }
}

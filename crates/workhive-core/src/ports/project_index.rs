//! Project index port — abstracts global project registry.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::errors::IndexError;

/// Summary of a workhive-managed project in the global index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    /// Display name of the project.
    pub project_name: String,
    /// Absolute path to the project root.
    pub project_root: Utf8PathBuf,
    /// Path to the project's `.workhive/config.toml`.
    pub config_path: Utf8PathBuf,
    /// ISO 8601 timestamp of last `open` command.
    pub last_opened_at: Option<String>,
    /// ISO 8601 timestamp of last modification.
    pub last_updated_at: Option<String>,
}

/// Three-state representation of the project index file.
///
/// Distinguishes between "index file does not exist" and "index file exists
/// but contains zero projects". `Corrupted` is conveyed via `IndexError`.
#[derive(Debug, Clone)]
pub enum IndexState {
    /// The index file does not exist on disk.
    NotFound,
    /// The index file exists and was parsed successfully.
    Loaded(Vec<ProjectSummary>),
}

impl IndexState {
    /// Returns `true` if the index file does not exist.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }

    /// Returns the project list, or an empty slice if the index is missing.
    pub fn projects(&self) -> &[ProjectSummary] {
        match self {
            Self::NotFound => &[],
            Self::Loaded(v) => v,
        }
    }

    /// Consumes `self` and returns the project list (empty if index missing).
    pub fn into_projects(self) -> Vec<ProjectSummary> {
        match self {
            Self::NotFound => Vec::new(),
            Self::Loaded(v) => v,
        }
    }
}

/// Paired index state and snapshot token from the same bytes.
///
/// Ensures the state and token cannot diverge because they are derived from
/// the exact same read of the index file.
#[derive(Debug, Clone)]
pub struct PairedIndexState {
    /// The parsed index state.
    pub state: IndexState,
    /// The snapshot token derived from the same bytes.
    pub token: IndexSnapshotToken,
}

/// Content-derived snapshot token for optimistic concurrency.
///
/// Represents the exact content of the index file at a point in time. The token
/// is derived by hashing the persisted index bytes, so it cannot diverge from
/// the actual content. Used for optimistic concurrency control: a worker captures
/// the token before scanning, then conditionally publishes only if the token
/// still matches (no intervening mutations).
///
/// The token is an enum with two variants:
/// - `Missing`: The index file does not exist
/// - `Digest([u8; 32])`: SHA-256 hash of the index file content
///
/// This design makes it impossible to represent malformed tokens or forged Missing values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexSnapshotToken {
    /// Index file does not exist.
    Missing,
    /// SHA-256 digest of the index file content.
    Digest([u8; 32]),
}

impl IndexSnapshotToken {
    /// Create a Missing token.
    pub fn missing() -> Self {
        Self::Missing
    }

    /// Create a Digest token from SHA-256 bytes.
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self::Digest(digest)
    }

    /// Check if this token represents a missing index.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }
}

/// Outcome of a conditional snapshot publication.
///
/// The worker captures a token before discovery, then attempts to publish.
/// If another mutation occurred in between, the publication is rejected as stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPublishOutcome {
    /// The snapshot was published successfully.
    Published,
    /// The snapshot was stale — another mutation occurred after the token
    /// was captured. The caller should discard the snapshot and rescan.
    Stale,
}

/// Abstraction over the global project index at `~/.config/workhive/index.json`.
///
/// The index is a discovery layer for fast cross-project queries.
/// It is NOT the source of truth for individual project state.
///
/// All mutations share an adapter-owned inter-process writer lease. Conditional
/// snapshot publication uses optimistic concurrency: capture a content-derived
/// token before scanning, publish only if the token still matches.
pub trait ProjectIndexPort {
    /// Read the index and return its state.
    ///
    /// Returns `IndexState::NotFound` when the index file is absent,
    /// `IndexState::Loaded(vec)` when it exists (possibly empty), and
    /// `Err(IndexError::Corrupted)` when parsing fails.
    fn list_projects(&self) -> Result<IndexState, IndexError>;

    /// Read the index state and its snapshot token from the same bytes.
    ///
    /// Ensures the state and token cannot diverge because they are derived from
    /// the exact same read of the index file. Used by merge reindex to capture
    /// a consistent baseline for conditional publication.
    fn read_paired_state(&self) -> Result<PairedIndexState, IndexError>;

    /// Capture the current index snapshot token.
    ///
    /// Used by the reconciliation worker before discovery. The captured
    /// token is later passed to `publish_snapshot_conditional` to detect
    /// intervening mutations. The token is derived from the actual index
    /// content bytes.
    fn capture_snapshot_token(&self) -> Result<IndexSnapshotToken, IndexError>;

    /// Add or update a project in the index.
    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError>;

    /// Remove a project from the index by its root path.
    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError>;

    /// Clear all entries from the index.
    fn clear(&self) -> Result<(), IndexError>;

    /// Replace the entire index with a new set of projects in one atomic operation.
    ///
    /// Used by `rebuild_project_index` to commit a complete discovery result
    /// under a single lease acquisition. Preserves the `clear_first` semantic
    /// by replacing (not merging) the index content.
    fn replace_all(&self, projects: &[ProjectSummary]) -> Result<(), IndexError>;

    /// Conditionally publish a snapshot if the content token still matches.
    ///
    /// The worker captures a token before discovery, scans, then calls
    /// this method. Under the writer lease, the adapter re-reads the current
    /// index content and derives its token. If the tokens match, the snapshot
    /// is published and `Published` is returned. If another mutation occurred,
    /// the snapshot is rejected and `Stale` is returned.
    ///
    /// The caller must discard the stale snapshot and rescan. Do not merge
    /// blindly — reconciliation must still remove projects that are no longer
    /// valid or in scope.
    fn publish_snapshot_conditional(
        &self,
        projects: &[ProjectSummary],
        expected_token: IndexSnapshotToken,
    ) -> Result<SnapshotPublishOutcome, IndexError>;
}

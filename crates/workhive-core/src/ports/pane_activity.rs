//! Port abstraction for the per-pane AI activity fact store.
//!
//! The store is owned by the infra crate (it writes JSON to
//! `<runtime_root>/<group>/<workspace>/activity/<unit_id>.json` using
//! atomic-replace semantics). The trait stays in core so use cases
//! and fakes can target a runtime-neutral interface.

use thiserror::Error;

use crate::domain::AiActivityFact;

/// Composite key identifying which pane an activity fact belongs to.
///
/// Mirrors the runtime correlation tuple `(group, workspace, unit_id)`,
/// the same tuple used by `PaneRuntimeId::derive`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PaneActivityKey {
    pub group: String,
    pub workspace: String,
    pub unit_id: String,
}

impl PaneActivityKey {
    /// Convenience constructor used by adapters/tests.
    pub fn new(
        group: impl Into<String>,
        workspace: impl Into<String>,
        unit_id: impl Into<String>,
    ) -> Self {
        Self {
            group: group.into(),
            workspace: workspace.into(),
            unit_id: unit_id.into(),
        }
    }

    /// Derive the key from a fact. Avoids surprises when callers
    /// reorder fields after editing the fact struct.
    pub fn from_fact(fact: &AiActivityFact) -> Self {
        Self {
            group: fact.group.clone(),
            workspace: fact.workspace.clone(),
            unit_id: fact.unit_id.clone(),
        }
    }
}

/// Adapter-level errors surfaced by the activity store.
#[derive(Debug, Error)]
pub enum PaneActivityIoError {
    /// Filesystem failure (write/read/replace returned an OS error).
    #[error("activity store I/O failed: {0}")]
    Io(String),
    /// The stored payload could not be deserialized as a known fact
    /// (corrupt or written by an incompatible schema). The recorder
    /// must surface this as a soft warning, not block `pane list`.
    #[error("activity store payload is corrupt: {0}")]
    CorruptPayload(String),
}

/// Per-pane activity fact store.
///
/// Adapters are expected to:
/// - write each pane's fact to a stable, predictable path;
/// - replace the file atomically (no half-written JSON);
/// - return `None` from `read` when the file is missing or corrupt
///   so callers can fall back to "open_unverified" without crashing.
pub trait PaneActivityStorePort {
    /// Persist `fact` as the latest known fact for its pane.
    /// Overwrites any previous fact for the same `(group, workspace,
    /// unit_id)` key.
    fn record(&self, fact: &AiActivityFact) -> Result<(), PaneActivityIoError>;

    /// Return the latest fact for the pane, or `None` if no fact
    /// exists (or it was corrupt and must be ignored).
    fn read(&self, key: &PaneActivityKey) -> Result<Option<AiActivityFact>, PaneActivityIoError>;

    /// Remove any fact for the pane. No-op when nothing is stored.
    fn remove(&self, key: &PaneActivityKey) -> Result<(), PaneActivityIoError>;
}

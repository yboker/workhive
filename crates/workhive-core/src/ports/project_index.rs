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

/// Abstraction over the global project index at `~/.config/workhive/index.json`.
///
/// The index is a discovery layer for fast cross-project queries.
/// It is NOT the source of truth for individual project state.
pub trait ProjectIndexPort {
    /// Read the index and return its state.
    ///
    /// Returns `IndexState::NotFound` when the index file is absent,
    /// `IndexState::Loaded(vec)` when it exists (possibly empty), and
    /// `Err(IndexError::Corrupted)` when parsing fails.
    fn list_projects(&self) -> Result<IndexState, IndexError>;

    /// Add or update a project in the index.
    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError>;

    /// Remove a project from the index by its root path.
    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError>;

    /// Clear all entries from the index.
    fn clear(&self) -> Result<(), IndexError>;

    /// Rebuild the index by scanning known project directories.
    fn rebuild(&self) -> Result<(), IndexError>;
}

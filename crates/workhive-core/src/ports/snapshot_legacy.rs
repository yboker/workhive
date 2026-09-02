//! Snapshot port — abstracts session layout persistence.
//!
//! Snapshot is the **sole formal recovery mechanism** for workhive. Documents
//! use a versioned schema with typed identity, metadata, and unit descriptions.
//! The [`LaunchCapture`] type (defined in the pane domain) captures how each
//! unit was originally launched for faithful restoration.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::domain::{LaunchCapture, WorkspaceId, WorkspaceStateKey};
use crate::errors::SnapshotError;

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

/// Current snapshot schema version.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

// ---------------------------------------------------------------------------
// Versioned snapshot document
// ---------------------------------------------------------------------------

/// A versioned snapshot document for a workspace session.
///
/// This is the on-disk and in-memory representation. `workspace` inside
/// `identity` is the business key; `session_name` and `server_name` are
/// validation anchors used to reject stale/incompatible snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDocument {
    /// Schema version — must equal [`SNAPSHOT_SCHEMA_VERSION`] to be loadable.
    pub schema_version: u32,
    /// Identity block — workspace is the primary key.
    pub identity: SnapshotIdentity,
    /// Capture metadata.
    pub meta: SnapshotMeta,
    /// Ordered list of units (panes) captured.
    pub units: Vec<SnapshotUnit>,
}

/// Identity section of a snapshot.
///
/// `workspace` is the primary business identity. `server_name` and
/// `session_name` are validation anchors derived from naming policy.
///
/// `state_key` is the typed lookup key derived from `WorkspaceStateKey`.
/// New snapshots always populate this; older snapshots may lack it and
/// will be validated via the legacy field-by-field comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotIdentity {
    pub instance_id: String,
    pub project: String,
    pub workspace: String,
    pub server_name: String,
    pub session_name: String,
    /// Typed state key for stable lookup, populated by legacy schema hardening.
    /// Format: `{instance_id}@{project_component}@{workspace}` for repo-root
    /// workspaces, or `{instance_id}@{project_component}@{subdir_key}@{workspace}`
    /// for subdirectory workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_key: Option<String>,
}

/// Metadata section of a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMeta {
    /// Runtime backend that produced this snapshot (e.g. `"tmux"`).
    pub runtime_backend: String,
    /// Platform where the snapshot was captured (e.g. `"macos"`, `"linux"`).
    pub platform: String,
    /// ISO-8601 timestamp of capture.
    pub captured_at: String,
    /// Monotonic generation counter (user-managed layout version).
    pub generation: u64,
    /// Opaque fingerprint of the config at capture time (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_fingerprint: Option<String>,
}

/// A single unit (pane) within a snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotUnit {
    /// Positional index within the session.
    pub index: u32,
    /// Whether this was the active (focused) unit.
    pub active: bool,
    /// User-visible title (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Working directory at capture time.
    pub cwd: Utf8PathBuf,
    /// Semantic kind of this unit (e.g. `"agent"`, `"terminal"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Role within the workspace (e.g. `"primary"`, `"worker"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Typed launch descriptor for faithful restoration.
    pub launch: LaunchCapture,
    /// Observed runtime state at capture (e.g. `"running"`, `"idle"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Last error message observed (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// ISO-8601 timestamp of last state change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Snapshot validation
// ---------------------------------------------------------------------------

/// Outcome of snapshot validation before restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotValidation {
    /// Snapshot is valid and compatible — restore may proceed.
    Valid,
    /// Snapshot exists but is incompatible or stale.
    Invalid(SnapshotInvalidReason),
}

/// Why a snapshot was rejected during pre-restore validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotInvalidReason {
    /// Schema version is unsupported.
    UnsupportedVersion { found: u32, expected: u32 },
    /// Instance ID does not match.
    InstanceMismatch { expected: String, found: String },
    /// Project name does not match.
    ProjectMismatch { expected: String, found: String },
    /// Workspace name does not match.
    WorkspaceMismatch { expected: String, found: String },
    /// Server name does not match current naming result.
    ServerMismatch { expected: String, found: String },
    /// Session name does not match current naming result.
    SessionMismatch { expected: String, found: String },
    /// Runtime backend is incompatible.
    BackendMismatch { expected: String, found: String },
    /// Composite state key does not match (instance+project+workspace key mismatch).
    StateKeyMismatch { expected: String, found: String },
}

impl std::fmt::Display for SnapshotInvalidReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion { found, expected } => {
                write!(
                    f,
                    "unsupported schema version: found {found}, expected {expected}"
                )
            }
            Self::InstanceMismatch { expected, found } => {
                write!(
                    f,
                    "instance_id mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::ProjectMismatch { expected, found } => {
                write!(
                    f,
                    "project mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::WorkspaceMismatch { expected, found } => {
                write!(
                    f,
                    "workspace mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::ServerMismatch { expected, found } => {
                write!(
                    f,
                    "server_name mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::SessionMismatch { expected, found } => {
                write!(
                    f,
                    "session_name mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::BackendMismatch { expected, found } => {
                write!(
                    f,
                    "runtime backend mismatch: expected '{expected}', found '{found}'"
                )
            }
            Self::StateKeyMismatch { expected, found } => {
                write!(
                    f,
                    "state key mismatch: expected '{expected}', found '{found}'"
                )
            }
        }
    }
}

/// Validate a snapshot document against the current runtime context.
///
/// Returns [`SnapshotValidation::Valid`] if all checks pass, otherwise
/// [`SnapshotValidation::Invalid`] with the first mismatch found.
pub fn validate_snapshot(
    doc: &SnapshotDocument,
    expected_instance_id: &str,
    expected_project: &str,
    expected_workspace: &str,
    expected_server: &str,
    expected_session: &str,
    expected_backend: &str,
) -> SnapshotValidation {
    if doc.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::UnsupportedVersion {
            found: doc.schema_version,
            expected: SNAPSHOT_SCHEMA_VERSION,
        });
    }
    if doc.identity.instance_id != expected_instance_id {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::InstanceMismatch {
            expected: expected_instance_id.to_owned(),
            found: doc.identity.instance_id.clone(),
        });
    }
    if doc.identity.project != expected_project {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::ProjectMismatch {
            expected: expected_project.to_owned(),
            found: doc.identity.project.clone(),
        });
    }
    if doc.identity.workspace != expected_workspace {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::WorkspaceMismatch {
            expected: expected_workspace.to_owned(),
            found: doc.identity.workspace.clone(),
        });
    }
    if doc.identity.server_name != expected_server {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::ServerMismatch {
            expected: expected_server.to_owned(),
            found: doc.identity.server_name.clone(),
        });
    }
    if doc.identity.session_name != expected_session {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::SessionMismatch {
            expected: expected_session.to_owned(),
            found: doc.identity.session_name.clone(),
        });
    }
    if doc.meta.runtime_backend != expected_backend {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::BackendMismatch {
            expected: expected_backend.to_owned(),
            found: doc.meta.runtime_backend.clone(),
        });
    }
    SnapshotValidation::Valid
}

/// Validate a snapshot using a typed `WorkspaceStateKey`.
///
/// This is the preferred validation path for new code. Falls back to
/// field-by-field comparison only when the snapshot lacks a `state_key`.
pub fn validate_snapshot_by_state_key(
    doc: &SnapshotDocument,
    expected_state_key: &crate::domain::WorkspaceStateKey,
    expected_server: &str,
    expected_session: &str,
    expected_backend: &str,
) -> SnapshotValidation {
    if doc.schema_version != SNAPSHOT_SCHEMA_VERSION {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::UnsupportedVersion {
            found: doc.schema_version,
            expected: SNAPSHOT_SCHEMA_VERSION,
        });
    }

    if let Some(stored_key) = &doc.identity.state_key {
        let expected_display = expected_state_key.to_string();
        if *stored_key != expected_display {
            return SnapshotValidation::Invalid(SnapshotInvalidReason::StateKeyMismatch {
                expected: expected_display,
                found: stored_key.clone(),
            });
        }
    } else {
        // Legacy snapshot without state_key — fall back to field-by-field
        return validate_snapshot(
            doc,
            &expected_state_key.instance_id,
            &expected_state_key.project_component,
            &expected_state_key.workspace,
            expected_server,
            expected_session,
            expected_backend,
        );
    }

    if doc.identity.server_name != expected_server {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::ServerMismatch {
            expected: expected_server.to_owned(),
            found: doc.identity.server_name.clone(),
        });
    }
    if doc.identity.session_name != expected_session {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::SessionMismatch {
            expected: expected_session.to_owned(),
            found: doc.identity.session_name.clone(),
        });
    }
    if doc.meta.runtime_backend != expected_backend {
        return SnapshotValidation::Invalid(SnapshotInvalidReason::BackendMismatch {
            expected: expected_backend.to_owned(),
            found: doc.meta.runtime_backend.clone(),
        });
    }
    SnapshotValidation::Valid
}

// ---------------------------------------------------------------------------
// Lightweight metadata (for listing)
// ---------------------------------------------------------------------------

/// Lightweight metadata about a snapshot (without full unit data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Workspace name.
    pub workspace: String,
    /// When the snapshot was created.
    pub created_at: String,
    /// File path of the snapshot.
    pub path: Utf8PathBuf,
}

// ---------------------------------------------------------------------------
// Port trait
// ---------------------------------------------------------------------------

/// Abstraction over snapshot storage.
pub trait SnapshotPort {
    /// Save a snapshot document for a workspace (atomic write).
    fn save(&self, snapshot: &SnapshotDocument) -> Result<(), SnapshotError>;

    /// Load the most recent snapshot document for a typed workspace state key.
    fn load_latest_by_state_key(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotDocument>, SnapshotError>;

    /// Load the most recent snapshot document for a workspace, if one exists.
    ///
    /// This is compatibility-only. New runtime paths must prefer
    /// [`SnapshotPort::load_latest_by_state_key`] to avoid collisions between
    /// same-named workspaces in different subdirectories.
    fn load_latest(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<Option<SnapshotDocument>, SnapshotError>;

    /// List all available snapshots.
    fn list(&self) -> Result<Vec<SnapshotMetadata>, SnapshotError>;

    /// Delete the snapshot for a specific workspace.
    ///
    /// This is compatibility-only. New runtime paths must prefer
    /// [`SnapshotPort::delete_by_state_key`].
    fn delete_for_workspace(&self, workspace: &WorkspaceId) -> Result<(), SnapshotError>;

    /// Delete the snapshot for a typed workspace state key.
    fn delete_by_state_key(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError>;

    /// Delete all snapshots.
    fn delete_all(&self) -> Result<(), SnapshotError>;
}

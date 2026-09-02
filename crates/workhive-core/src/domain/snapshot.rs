//! Snapshot domain model — production-grade snapshot subsystem types.
//!
//! This module defines the formal data model for the snapshot architecture.
//! Key design principles:
//!
//! - `launch_intent` is the recovery source of truth, not `current_command`
//! - Topology (windows + layout) is captured and restored faithfully
//! - Every revision is versioned, checksummed, and auditable
//! - Migration is explicit: old formats pass through typed migrators

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use super::workspace_identity::WorkspaceStateKey;

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

/// Current snapshot schema version.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 3;

// ---------------------------------------------------------------------------
// Revision identity
// ---------------------------------------------------------------------------

/// Unique identifier for a snapshot revision.
///
/// Monotonically increasing within a workspace state key scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SnapshotRevisionId(pub u64);

impl SnapshotRevisionId {
    /// The first revision ID.
    pub fn first() -> Self {
        Self(1)
    }

    /// Increment to the next revision.
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl std::fmt::Display for SnapshotRevisionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:06}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Snapshot source
// ---------------------------------------------------------------------------

/// How a snapshot was triggered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSource {
    /// Periodic timed save.
    TimedSave,
    /// Triggered by a topology event (pane/window added/removed).
    EventSave,
    /// Triggered by a foreground command change.
    CommandChangeSave,
    /// Manual save via CLI command.
    ManualSave,
    /// Save on session closing.
    SessionClosing,
    /// Periodic full reconciliation snapshot.
    PeriodicReconcile,
}

impl std::fmt::Display for SnapshotSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedSave => f.write_str("timed_save"),
            Self::EventSave => f.write_str("event_save"),
            Self::CommandChangeSave => f.write_str("command_change_save"),
            Self::ManualSave => f.write_str("manual_save"),
            Self::SessionClosing => f.write_str("session_closing"),
            Self::PeriodicReconcile => f.write_str("periodic_reconcile"),
        }
    }
}

// ---------------------------------------------------------------------------
// Event model
// ---------------------------------------------------------------------------

/// Snapshot event kinds — the specific topology or state changes that
/// can trigger a snapshot save or be recorded in the event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotEventKind {
    /// A pane was added to a window.
    PaneAdded,
    /// A pane was removed from a window.
    PaneRemoved,
    /// A new window was created.
    WindowAdded,
    /// A window was destroyed.
    WindowRemoved,
    /// Window layout changed (resize, drag, etc.).
    LayoutChanged,
    /// A pane's foreground command changed.
    PaneCommandChanged,
    /// Active pane focus changed.
    PaneFocusChanged,
    /// Periodic reconciliation tick.
    PeriodicReconcile,
    /// Manual save request.
    ManualSave,
    /// Session is closing.
    SessionClosing,
}

impl std::fmt::Display for SnapshotEventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::PaneAdded => "pane_added",
            Self::PaneRemoved => "pane_removed",
            Self::WindowAdded => "window_added",
            Self::WindowRemoved => "window_removed",
            Self::LayoutChanged => "layout_changed",
            Self::PaneCommandChanged => "pane_command_changed",
            Self::PaneFocusChanged => "pane_focus_changed",
            Self::PeriodicReconcile => "periodic_reconcile",
            Self::ManualSave => "manual_save",
            Self::SessionClosing => "session_closing",
        };
        f.write_str(s)
    }
}

/// A recorded snapshot event with timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEvent {
    /// ISO-8601 timestamp of the event.
    pub timestamp: String,
    /// Revision checkpoint associated with the event, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_id: Option<SnapshotRevisionId>,
    /// The kind of event.
    pub kind: SnapshotEventKind,
    /// Optional description or context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Pane launch intent
// ---------------------------------------------------------------------------

/// Describes how a pane should be restored — the recovery source of truth.
///
/// Unlike `pane_current_command` (which captures whatever happens to be
/// running at save time), `PaneLaunchIntent` records the *intended* launch
/// configuration that should be replayed on restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLaunchIntent {
    /// Restore by looking up a tool catalog entry.
    ToolRef { tool_name: String },
    /// Restore by running a specific program with arguments.
    InlineCommand { program: String, args: Vec<String> },
    /// Restore as an empty interactive shell.
    EmptyShell,
    /// Cannot be restored — display reason to user.
    NonRestorable { display: String, reason: String },
}

impl PaneLaunchIntent {
    /// Whether this intent can be used for restoration.
    pub fn is_restorable(&self) -> bool {
        !matches!(self, Self::NonRestorable { .. })
    }
}

// ---------------------------------------------------------------------------
// Pane observed state (diagnostic, not recovery)
// ---------------------------------------------------------------------------

/// Observed runtime state of a pane at capture time.
///
/// This is diagnostic information — restore uses `PaneLaunchIntent`, not
/// the observed state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneObservedState {
    /// Foreground command reported by the runtime at capture time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_command: Option<String>,
    /// Command that started the pane, when the runtime can report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    /// Whether this pane was the active (focused) pane.
    pub active: bool,
    /// Whether the pane process has exited.
    pub dead: bool,
}

// ---------------------------------------------------------------------------
// Restore policy
// ---------------------------------------------------------------------------

/// Per-pane restore policy.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestorePolicy {
    /// Restore this pane normally.
    #[default]
    Restore,
    /// Skip this pane during restore.
    Skip,
    /// Restore as an empty shell regardless of launch intent.
    ShellOnly,
}

// ---------------------------------------------------------------------------
// Pane reference (window -> pane mapping)
// ---------------------------------------------------------------------------

/// Reference from a window to a pane within the topology.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRef {
    /// Index of the pane within its window.
    pub pane_index: u32,
    /// Window index this pane belongs to.
    pub window_index: u32,
}

// ---------------------------------------------------------------------------
// Pane snapshot
// ---------------------------------------------------------------------------

/// Full snapshot of a single pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSnapshot {
    /// Runtime ID if available from the pane registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    /// Index of the window this pane belongs to.
    pub window_index: u32,
    /// Index of the pane within its window.
    pub pane_index: u32,
    /// Backend-specific pane ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    /// User-visible pane title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Working directory at capture time.
    pub cwd: Utf8PathBuf,
    /// Whether this pane is managed by workhive (vs. user-created).
    pub managed: bool,
    /// Per-pane restore policy.
    #[serde(default)]
    pub restore_policy: RestorePolicy,
    /// The intended launch configuration for restore.
    pub launch_intent: PaneLaunchIntent,
    /// Observed diagnostic state at capture time.
    pub observed: PaneObservedState,
}

// ---------------------------------------------------------------------------
// Window snapshot
// ---------------------------------------------------------------------------

/// Snapshot of a single runtime window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSnapshot {
    /// Window index within the session.
    pub index: u32,
    /// Window name, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Opaque backend layout data for faithful topology reconstruction.
    pub layout: String,
    /// References to panes in this window.
    pub panes: Vec<PaneRef>,
}

// ---------------------------------------------------------------------------
// Workspace topology
// ---------------------------------------------------------------------------

/// The full topology of a workspace session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceTopology {
    /// All windows in the session.
    pub windows: Vec<WindowSnapshot>,
    /// Index of the active (focused) window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window: Option<u32>,
    /// Runtime ID of the active pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pane_runtime_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Snapshot metadata
// ---------------------------------------------------------------------------

/// Typed runtime backend identifier used at runtime boundaries.
///
/// Snapshot persistence keeps storing the raw string in `runtime_backend` for
/// schema compatibility; this value object is for typed comparisons and new
/// runtime-facing code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBackendId(String);

impl RuntimeBackendId {
    /// Built-in tmux runtime backend identifier.
    pub fn tmux() -> Self {
        Self("tmux".to_owned())
    }

    /// Construct a runtime backend identifier from a non-empty string.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            None
        } else {
            Some(Self(value))
        }
    }

    /// Return the persisted backend identifier string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeBackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Metadata about a snapshot revision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Runtime backend that produced this snapshot.
    pub runtime_backend: String,
    /// Platform where the snapshot was captured (e.g. `"macos"`, `"linux"`).
    pub platform: String,
    /// Monotonic generation counter.
    pub generation: u64,
    /// SHA-256 content hash for deduplication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    /// Opaque config fingerprint at capture time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_fingerprint: Option<String>,
}

// ---------------------------------------------------------------------------
// Top-level workspace snapshot
// ---------------------------------------------------------------------------

/// A complete workspace snapshot — the primary persisted artifact.
///
/// This replaces the flat `SnapshotDocument` model with topology-aware,
/// intent-based snapshots. Restore consumes this model exclusively.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    /// Schema version — must equal [`SNAPSHOT_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Unique revision identifier.
    pub revision_id: SnapshotRevisionId,
    /// Typed workspace state key for scoped lookup.
    pub state_key: WorkspaceStateKey,
    /// ISO-8601 timestamp of capture.
    pub captured_at: String,
    /// How this snapshot was triggered.
    pub source: SnapshotSource,
    /// Session topology (windows, layout, active state).
    pub topology: WorkspaceTopology,
    /// All panes captured in this snapshot.
    pub panes: Vec<PaneSnapshot>,
    /// Snapshot metadata.
    pub metadata: SnapshotMetadata,
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// Manifest pointing to the current head revision.
///
/// The manifest is the entry point for the snapshot store. It points to
/// the latest full revision and tracks event log progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    /// State key this manifest belongs to.
    pub state_key: WorkspaceStateKey,
    /// The latest full revision ID.
    pub head_revision: SnapshotRevisionId,
    /// Total number of revisions written.
    pub total_revisions: u64,
    /// Number of event log entries since last full revision.
    pub events_since_full: u64,
    /// ISO-8601 timestamp of last successful save.
    pub last_saved_at: String,
    /// ISO-8601 timestamp of last full revision.
    pub last_full_revision_at: String,
}

// ---------------------------------------------------------------------------
// Lock metadata
// ---------------------------------------------------------------------------

/// Supervisor lock metadata for single-writer ownership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorLock {
    /// PID of the owning supervisor process.
    pub pid: u32,
    /// ISO-8601 timestamp of last heartbeat.
    pub heartbeat: String,
    /// State key this lock covers.
    pub state_key: WorkspaceStateKey,
}

// ---------------------------------------------------------------------------
// Restore plan
// ---------------------------------------------------------------------------

/// A restore action for a single pane.
#[derive(Debug, Clone)]
pub enum PaneRestoreAction {
    /// Create a new pane with the given launch intent.
    Create {
        window_index: u32,
        pane_index: u32,
        cwd: Utf8PathBuf,
        launch_intent: PaneLaunchIntent,
        title: Option<String>,
    },
    /// Skip this pane.
    Skip { reason: String },
}

/// A restore action for a window.
#[derive(Debug, Clone)]
pub enum WindowRestoreAction {
    /// Create this window with its layout.
    Create {
        index: u32,
        name: Option<String>,
        layout: String,
        pane_actions: Vec<PaneRestoreAction>,
    },
}

/// A declarative restore plan built from a snapshot.
///
/// The plan is validated before execution. On failure during execution,
/// the system rolls back to a clean state.
#[derive(Debug, Clone)]
pub struct RestorePlan {
    /// State key being restored.
    pub state_key: WorkspaceStateKey,
    /// Revision being restored.
    pub revision_id: SnapshotRevisionId,
    /// Window actions in order.
    pub window_actions: Vec<WindowRestoreAction>,
    /// Index of the window to activate after restore.
    pub active_window: Option<u32>,
    /// Whether this is a dry-run (no side effects).
    pub dry_run: bool,
}

impl RestorePlan {
    /// Total number of panes that will be created.
    pub fn pane_count(&self) -> usize {
        self.window_actions
            .iter()
            .map(|w| match w {
                WindowRestoreAction::Create { pane_actions, .. } => pane_actions
                    .iter()
                    .filter(|a| matches!(a, PaneRestoreAction::Create { .. }))
                    .count(),
            })
            .sum()
    }

    /// Whether the plan has any actionable restore operations.
    pub fn is_empty(&self) -> bool {
        self.window_actions.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Restore outcome
// ---------------------------------------------------------------------------

/// Result of executing a restore plan.
#[derive(Debug, Clone)]
pub enum RestoreOutcome {
    /// Restore completed successfully.
    Success {
        panes_restored: usize,
        windows_restored: usize,
        fidelity: RestoreFidelity,
    },
    /// Restore was a dry run — no side effects.
    DryRun {
        panes_planned: usize,
        windows_planned: usize,
        fidelity: RestoreFidelity,
    },
    /// Restore failed and was rolled back.
    RolledBack { reason: String },
    /// No snapshot available to restore from.
    NoSnapshot,
}

/// Describes how completely a snapshot was restored.
///
/// `LaunchRestorable` means all panes had valid launch intents and were
/// restored with their original commands. `TopologyOnly` means the window/pane
/// structure was recreated but some or all panes fell back to an empty shell
/// because launch metadata was missing. `LayoutFallback` means only spatial
/// arrangement was preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreFidelity {
    /// Every pane was restored with its original launch command.
    LaunchRestorable {
        command_restored: usize,
        shell_only: usize,
    },
    /// Structure preserved, but some panes degraded to shell.
    TopologyOnly {
        command_restored: usize,
        shell_only: usize,
    },
    /// Only layout geometry was preserved.
    LayoutFallback { shell_only: usize },
}

// ---------------------------------------------------------------------------
// Launch baseline — cross-process launch metadata persistence
// ---------------------------------------------------------------------------

/// Per-pane launch metadata stored durably so that monitor/notify processes
/// (which are separate CLI invocations with fresh in-memory state) can
/// reconstruct registry-first launch origins without falling back to
/// `start_command`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchBaselineEntry {
    /// Runtime-scoped pane identifier (group + workspace + unit_id hash).
    pub runtime_id: String,
    /// Unit identifier within the workspace.
    pub unit_id: String,
    /// How the pane was originally launched.
    pub launch_origin: super::tool::PaneLaunchRef,
}

/// Complete baseline document persisted alongside the snapshot store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchBaseline {
    /// State key this baseline belongs to.
    pub state_key: WorkspaceStateKey,
    /// Per-pane launch metadata entries.
    pub entries: Vec<LaunchBaselineEntry>,
    /// ISO-8601 timestamp of last update.
    pub updated_at: String,
}

// ---------------------------------------------------------------------------
// Migration types
// ---------------------------------------------------------------------------

/// Schema version identifier for migration routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaVersion {
    /// Original flat snapshot format.
    V1,
    /// State-key-aware format (current `SnapshotDocument`).
    V2,
    /// Full topology + intent-based format.
    V3,
}

impl SchemaVersion {
    /// Parse from a numeric schema version field.
    pub fn from_version_number(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::V1),
            2 => Some(Self::V2),
            3 => Some(Self::V3),
            _ => None,
        }
    }

    /// The numeric version number.
    pub fn as_number(self) -> u32 {
        match self {
            Self::V1 => 1,
            Self::V2 => 2,
            Self::V3 => 3,
        }
    }
}

/// Result of a migration attempt.
#[derive(Debug, Clone)]
pub enum MigrationOutcome {
    /// Successfully migrated from one version to another.
    Migrated {
        from: SchemaVersion,
        to: SchemaVersion,
    },
    /// No migration needed — already at target version.
    AlreadyCurrent,
    /// Migration is not possible — snapshot must be discarded.
    Incompatible { version: u32, reason: String },
}

// ---------------------------------------------------------------------------
// Snapshot runtime configuration extension
// ---------------------------------------------------------------------------

/// Extended snapshot runtime configuration.
///
/// Extends the existing `SnapshotConfig` with granular controls for
/// timed save, event save, command-change detection, and retention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRuntimeConfig {
    /// Whether snapshot save/restore is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Timed save interval in minutes (default 5).
    #[serde(default = "default_interval_minutes")]
    pub interval_minutes: u32,
    /// Whether to save on topology events (pane/window add/remove).
    #[serde(default = "default_true")]
    pub save_on_event: bool,
    /// Whether to save when a pane's foreground command changes.
    #[serde(default = "default_true")]
    pub save_on_command_change: bool,
    /// Whether to include unmanaged panes in snapshots.
    #[serde(default)]
    pub include_unmanaged_panes: bool,
    /// Number of full revisions to retain.
    #[serde(default = "default_retain_full_revisions")]
    pub retain_full_revisions: u32,
    /// Days to retain event logs.
    #[serde(default = "default_retain_event_days")]
    pub retain_event_days: u32,
    /// Debounce window for event saves in milliseconds.
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    /// Polling interval for command-change detection in seconds.
    #[serde(default = "default_command_poll_seconds")]
    pub command_poll_seconds: u32,
    /// Interval for periodic full snapshots in minutes.
    #[serde(default = "default_full_snapshot_interval_minutes")]
    pub full_snapshot_interval_minutes: u32,
}

fn default_true() -> bool {
    true
}
fn default_interval_minutes() -> u32 {
    5
}
fn default_retain_full_revisions() -> u32 {
    20
}
fn default_retain_event_days() -> u32 {
    30
}
fn default_debounce_ms() -> u64 {
    1500
}
fn default_command_poll_seconds() -> u32 {
    10
}
fn default_full_snapshot_interval_minutes() -> u32 {
    30
}

impl Default for SnapshotRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: default_interval_minutes(),
            save_on_event: true,
            save_on_command_change: true,
            include_unmanaged_panes: false,
            retain_full_revisions: default_retain_full_revisions(),
            retain_event_days: default_retain_event_days(),
            debounce_ms: default_debounce_ms(),
            command_poll_seconds: default_command_poll_seconds(),
            full_snapshot_interval_minutes: default_full_snapshot_interval_minutes(),
        }
    }
}

// ---------------------------------------------------------------------------
// Doctor / status types
// ---------------------------------------------------------------------------

/// Snapshot health check result.
#[derive(Debug, Clone)]
pub struct SnapshotHealthCheck {
    /// Whether snapshots are enabled in config.
    pub enabled: bool,
    /// Whether the supervisor is currently running.
    pub supervisor_running: bool,
    /// Whether tmux hooks are registered.
    pub hooks_registered: bool,
    /// ISO-8601 timestamp of the latest successful save.
    pub latest_save_time: Option<String>,
    /// Latest revision ID.
    pub latest_revision: Option<SnapshotRevisionId>,
    /// Whether the manifest is readable.
    pub manifest_readable: bool,
    /// Checksum verification passed.
    pub checksum_valid: Option<bool>,
    /// Whether state key matches current runtime.
    pub state_key_match: bool,
}

/// Snapshot history entry for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotHistoryEntry {
    /// Revision ID.
    pub revision_id: SnapshotRevisionId,
    /// When it was captured.
    pub captured_at: String,
    /// How it was triggered.
    pub source: SnapshotSource,
    /// Number of panes in this revision.
    pub pane_count: usize,
    /// Number of windows in this revision.
    pub window_count: usize,
    /// Content hash if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_id_display_is_zero_padded() {
        assert_eq!(SnapshotRevisionId(1).to_string(), "000001");
        assert_eq!(SnapshotRevisionId(42).to_string(), "000042");
        assert_eq!(SnapshotRevisionId(999999).to_string(), "999999");
    }

    #[test]
    fn revision_id_ordering() {
        let a = SnapshotRevisionId(1);
        let b = SnapshotRevisionId(2);
        assert!(a < b);
        assert_eq!(a.next(), b);
    }

    #[test]
    fn launch_intent_restorability() {
        assert!(PaneLaunchIntent::EmptyShell.is_restorable());
        assert!(
            PaneLaunchIntent::ToolRef {
                tool_name: "vim".to_owned()
            }
            .is_restorable()
        );
        assert!(
            PaneLaunchIntent::InlineCommand {
                program: "python".to_owned(),
                args: vec![],
            }
            .is_restorable()
        );
        assert!(
            !PaneLaunchIntent::NonRestorable {
                display: "ssh".to_owned(),
                reason: "remote session".to_owned(),
            }
            .is_restorable()
        );
    }

    #[test]
    fn restore_policy_default_is_restore() {
        assert_eq!(RestorePolicy::default(), RestorePolicy::Restore);
    }

    #[test]
    fn schema_version_roundtrip() {
        for v in [1, 2, 3] {
            let sv = SchemaVersion::from_version_number(v).unwrap();
            assert_eq!(sv.as_number(), v);
        }
        assert!(SchemaVersion::from_version_number(0).is_none());
        assert!(SchemaVersion::from_version_number(99).is_none());
    }

    #[test]
    fn snapshot_runtime_config_defaults() {
        let cfg = SnapshotRuntimeConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.interval_minutes, 5);
        assert!(cfg.save_on_event);
        assert!(cfg.save_on_command_change);
        assert!(!cfg.include_unmanaged_panes);
        assert_eq!(cfg.retain_full_revisions, 20);
        assert_eq!(cfg.retain_event_days, 30);
        assert_eq!(cfg.debounce_ms, 1500);
        assert_eq!(cfg.command_poll_seconds, 10);
        assert_eq!(cfg.full_snapshot_interval_minutes, 30);
    }

    #[test]
    fn restore_plan_pane_count() {
        let plan = RestorePlan {
            state_key: WorkspaceStateKey::new(
                "a1b2".into(),
                "proj".into(),
                String::new(),
                "main".into(),
            ),
            revision_id: SnapshotRevisionId::first(),
            window_actions: vec![WindowRestoreAction::Create {
                index: 0,
                name: None,
                layout: "tiled".into(),
                pane_actions: vec![
                    PaneRestoreAction::Create {
                        window_index: 0,
                        pane_index: 0,
                        cwd: Utf8PathBuf::from("/tmp"),
                        launch_intent: PaneLaunchIntent::EmptyShell,
                        title: None,
                    },
                    PaneRestoreAction::Skip {
                        reason: "non-restorable".into(),
                    },
                    PaneRestoreAction::Create {
                        window_index: 0,
                        pane_index: 2,
                        cwd: Utf8PathBuf::from("/tmp"),
                        launch_intent: PaneLaunchIntent::ToolRef {
                            tool_name: "vim".into(),
                        },
                        title: Some("editor".into()),
                    },
                ],
            }],
            active_window: Some(0),
            dry_run: false,
        };
        assert_eq!(plan.pane_count(), 2);
        assert!(!plan.is_empty());
    }

    #[test]
    fn empty_restore_plan() {
        let plan = RestorePlan {
            state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
            revision_id: SnapshotRevisionId::first(),
            window_actions: vec![],
            active_window: None,
            dry_run: true,
        };
        assert!(plan.is_empty());
        assert_eq!(plan.pane_count(), 0);
    }

    #[test]
    fn snapshot_source_display() {
        assert_eq!(SnapshotSource::TimedSave.to_string(), "timed_save");
        assert_eq!(SnapshotSource::EventSave.to_string(), "event_save");
        assert_eq!(
            SnapshotSource::SessionClosing.to_string(),
            "session_closing"
        );
    }

    #[test]
    fn snapshot_event_kind_display() {
        assert_eq!(SnapshotEventKind::PaneAdded.to_string(), "pane_added");
        assert_eq!(
            SnapshotEventKind::LayoutChanged.to_string(),
            "layout_changed"
        );
    }

    #[test]
    fn migration_outcome_variants() {
        let migrated = MigrationOutcome::Migrated {
            from: SchemaVersion::V1,
            to: SchemaVersion::V3,
        };
        assert!(matches!(migrated, MigrationOutcome::Migrated { .. }));

        let current = MigrationOutcome::AlreadyCurrent;
        assert!(matches!(current, MigrationOutcome::AlreadyCurrent));

        let incompat = MigrationOutcome::Incompatible {
            version: 99,
            reason: "unknown format".into(),
        };
        assert!(matches!(incompat, MigrationOutcome::Incompatible { .. }));
    }

    #[test]
    fn pane_snapshot_serde_roundtrip() {
        let pane = PaneSnapshot {
            runtime_id: Some("rt-001".to_owned()),
            window_index: 0,
            pane_index: 1,
            pane_id: Some("%1".to_owned()),
            title: Some("editor".to_owned()),
            cwd: Utf8PathBuf::from("/home/user/project"),
            managed: true,
            restore_policy: RestorePolicy::Restore,
            launch_intent: PaneLaunchIntent::ToolRef {
                tool_name: "vim".to_owned(),
            },
            observed: PaneObservedState {
                current_command: Some("vim".to_owned()),
                start_command: Some("vim".to_owned()),
                active: true,
                dead: false,
            },
        };

        let json = serde_json::to_string(&pane).expect("serialize");
        let restored: PaneSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.runtime_id, pane.runtime_id);
        assert_eq!(restored.cwd, pane.cwd);
        assert_eq!(restored.managed, pane.managed);
    }

    #[test]
    fn workspace_snapshot_serde_roundtrip() {
        let snap = WorkspaceSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            revision_id: SnapshotRevisionId(42),
            state_key: WorkspaceStateKey::new(
                "a1b2".into(),
                "proj".into(),
                String::new(),
                "feat-x".into(),
            ),
            captured_at: "2026-04-24T10:00:00Z".to_owned(),
            source: SnapshotSource::ManualSave,
            topology: WorkspaceTopology {
                windows: vec![WindowSnapshot {
                    index: 0,
                    name: Some("main".to_owned()),
                    layout: "tiled".to_owned(),
                    panes: vec![PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    }],
                }],
                active_window: Some(0),
                active_pane_runtime_id: None,
            },
            panes: vec![PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: Some("%0".to_owned()),
                title: None,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::EmptyShell,
                observed: PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: None,
                    active: true,
                    dead: false,
                },
            }],
            metadata: SnapshotMetadata {
                runtime_backend: "tmux".to_owned(),
                platform: "macos".to_owned(),
                generation: 1,
                content_hash: None,
                config_fingerprint: None,
            },
        };

        let json = serde_json::to_string_pretty(&snap).expect("serialize");
        let restored: WorkspaceSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(restored.revision_id, SnapshotRevisionId(42));
        assert_eq!(restored.panes.len(), 1);
        assert_eq!(restored.topology.windows.len(), 1);
    }

    #[test]
    fn health_check_construction() {
        let check = SnapshotHealthCheck {
            enabled: true,
            supervisor_running: false,
            hooks_registered: false,
            latest_save_time: None,
            latest_revision: None,
            manifest_readable: false,
            checksum_valid: None,
            state_key_match: true,
        };
        assert!(check.enabled);
        assert!(!check.supervisor_running);
    }
}

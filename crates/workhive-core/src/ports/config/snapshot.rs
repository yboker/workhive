#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// Snapshot settings.
///
/// Snapshot configuration extends the original `enabled` flag with granular controls for
/// timed save, event save, command-change detection, and retention.
/// All new fields use `serde(default)` for backwards
/// compatibility with existing config files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Whether snapshot save/restore is enabled.
    #[serde(default = "SnapshotConfig::default_enabled")]
    pub enabled: bool,
    /// Timed save interval in minutes (default 5).
    #[serde(default = "SnapshotConfig::default_interval_minutes")]
    pub interval_minutes: u32,
    /// Whether to save on topology events (pane/window add/remove).
    #[serde(default = "SnapshotConfig::default_enabled")]
    pub save_on_event: bool,
    /// Whether to save when a pane's foreground command changes.
    #[serde(default = "SnapshotConfig::default_enabled")]
    pub save_on_command_change: bool,
    /// Whether to include unmanaged panes in snapshots.
    #[serde(default)]
    pub include_unmanaged_panes: bool,
    /// Number of full revisions to retain (default 20).
    #[serde(default = "SnapshotConfig::default_retain_full_revisions")]
    pub retain_full_revisions: u32,
    /// Days to retain event logs (default 30).
    #[serde(default = "SnapshotConfig::default_retain_event_days")]
    pub retain_event_days: u32,
    /// Debounce window for event saves in milliseconds (default 1500).
    #[serde(default = "SnapshotConfig::default_debounce_ms")]
    pub debounce_ms: u64,
    /// Polling interval for command-change detection in seconds (default 10).
    #[serde(default = "SnapshotConfig::default_command_poll_seconds")]
    pub command_poll_seconds: u32,
    /// Interval for periodic full snapshots in minutes (default 30).
    #[serde(default = "SnapshotConfig::default_full_snapshot_interval_minutes")]
    pub full_snapshot_interval_minutes: u32,
}

impl SnapshotConfig {
    fn default_enabled() -> bool {
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
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            interval_minutes: Self::default_interval_minutes(),
            save_on_event: Self::default_enabled(),
            save_on_command_change: Self::default_enabled(),
            include_unmanaged_panes: false,
            retain_full_revisions: Self::default_retain_full_revisions(),
            retain_event_days: Self::default_retain_event_days(),
            debounce_ms: Self::default_debounce_ms(),
            command_poll_seconds: Self::default_command_poll_seconds(),
            full_snapshot_interval_minutes: Self::default_full_snapshot_interval_minutes(),
        }
    }
}

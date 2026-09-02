//! Snapshot open-policy use case.
//!
//! Encapsulates the typed decision of "should the snapshot runtime be active
//! for this `workspace open`?" so the CLI handler doesn't have to scatter ad-hoc
//! `if snapshot.enabled` branches across baseline writing, hook installation,
//! monitor spawning, and tmux warning paths.
//!
//! Per the snapshot directory creation policy,
//! `snapshot.enabled = false` must mean *no* snapshot runtime is touched
//! during open, including hook removal — to avoid spurious tmux compatibility
//! warnings and unwanted writes to `.workhive/snapshots/`.

use crate::ports::SnapshotConfig;

/// Typed decision returned by [`resolve_snapshot_open_policy`].
///
/// CLI code branches on this enum once and gets a complete picture of which
/// snapshot operations are permitted for this `open` invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOpenPolicy {
    /// Snapshot runtime is fully active.
    ///
    /// All snapshot-related operations may proceed: baseline writes, hook
    /// installation, monitor spawn, lazy creation of `.workhive/snapshots/`.
    Enabled,
    /// Snapshot runtime is disabled.
    ///
    /// No snapshot port methods may be called for this open. Specifically:
    /// - no hook install
    /// - no hook removal (pre-existing hooks are NOT cleaned up here)
    /// - no monitor spawn
    /// - no baseline persistence
    /// - no tmux compatibility warnings
    ///
    /// `workspace open` continues to apply the layout normally and surfaces a
    /// "snapshot skipped" notice to the user.
    Disabled,
}

impl SnapshotOpenPolicy {
    /// Whether the snapshot runtime branch is permitted to execute.
    pub fn is_runtime_active(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// Resolve the snapshot-open policy from effective config.
///
/// The CLI layer remains responsible for the project-over-global config merge;
/// this function consumes the merged [`SnapshotConfig`] and produces the
/// typed decision.
pub fn resolve_snapshot_open_policy(snapshot: &SnapshotConfig) -> SnapshotOpenPolicy {
    if snapshot.enabled {
        SnapshotOpenPolicy::Enabled
    } else {
        SnapshotOpenPolicy::Disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_config_yields_enabled_policy() {
        let cfg = SnapshotConfig {
            enabled: true,
            ..SnapshotConfig::default()
        };
        assert_eq!(
            resolve_snapshot_open_policy(&cfg),
            SnapshotOpenPolicy::Enabled
        );
    }

    #[test]
    fn disabled_config_yields_disabled_policy() {
        let cfg = SnapshotConfig {
            enabled: false,
            ..SnapshotConfig::default()
        };
        assert_eq!(
            resolve_snapshot_open_policy(&cfg),
            SnapshotOpenPolicy::Disabled
        );
    }

    #[test]
    fn disabled_policy_blocks_all_runtime_operations() {
        assert!(!SnapshotOpenPolicy::Disabled.is_runtime_active());
    }

    #[test]
    fn enabled_policy_permits_runtime_operations() {
        assert!(SnapshotOpenPolicy::Enabled.is_runtime_active());
    }
}

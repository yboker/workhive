//! Snapshot store port — abstracts revision-based snapshot persistence.
//!
//! This port replaces the flat single-file `SnapshotPort` with a
//! revision-aware store supporting event logs, manifests, and locks.

use crate::domain::{
    LaunchBaseline, SnapshotEvent, SnapshotHealthCheck, SnapshotHistoryEntry, SnapshotManifest,
    SnapshotRevisionId, SupervisorLock, WorkspaceSnapshot, WorkspaceStateKey,
};
use crate::errors::SnapshotError;

/// Abstraction over the snapshot storage system.
///
/// Supports revision-based persistence with event logs, manifests, and
/// single-writer locking. Implementations must guarantee atomic writes
/// for revisions and manifests.
pub trait SnapshotStorePort {
    /// Save a full snapshot revision atomically.
    ///
    /// Returns the revision ID assigned to this save.
    fn save_revision(
        &self,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<SnapshotRevisionId, SnapshotError>;

    /// Load the latest full snapshot revision for a state key.
    fn load_latest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError>;

    /// Load a specific revision by ID.
    fn load_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError>;

    /// Append events to the event log for a state key.
    fn append_events(
        &self,
        state_key: &WorkspaceStateKey,
        events: &[SnapshotEvent],
    ) -> Result<(), SnapshotError>;

    /// Load events since the given revision.
    fn load_events_since(
        &self,
        state_key: &WorkspaceStateKey,
        since_revision: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError>;

    /// Read the manifest for a state key.
    fn read_manifest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError>;

    /// Write the manifest atomically.
    fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError>;

    /// List revision history for a state key.
    fn list_history(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError>;

    /// Acquire the supervisor lock for a state key.
    ///
    /// Returns `Ok(true)` if the lock was acquired, `Ok(false)` if already held
    /// by another live process.
    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError>;

    /// Release the supervisor lock.
    fn release_lock(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError>;

    /// Update the heartbeat timestamp on an existing lock.
    fn update_heartbeat(
        &self,
        state_key: &WorkspaceStateKey,
        heartbeat: &str,
    ) -> Result<(), SnapshotError>;

    /// Read the current lock metadata.
    fn read_lock(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SupervisorLock>, SnapshotError>;

    /// Run health checks for snapshot infrastructure.
    fn health_check(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<SnapshotHealthCheck, SnapshotError>;

    /// Verify the integrity of a specific revision (checksum validation).
    fn verify_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError>;

    /// Apply retention policy — prune old revisions and events.
    fn apply_retention(
        &self,
        state_key: &WorkspaceStateKey,
        retain_revisions: u32,
        retain_event_days: u32,
    ) -> Result<u32, SnapshotError>;

    /// Delete all snapshot data for a state key.
    fn delete_all(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError>;

    /// Delete all snapshots across all state keys.
    fn delete_all_global(&self) -> Result<(), SnapshotError>;

    /// Save launch metadata baseline for cross-process registry hydration.
    ///
    /// Written after `open` creates/restores panes so that subsequent
    /// `monitor`/`notify` processes can reconstruct registry-first launch
    /// origins without relying on ephemeral in-memory state.
    fn save_launch_baseline(&self, baseline: &LaunchBaseline) -> Result<(), SnapshotError>;

    /// Load launch metadata baseline for a state key.
    fn load_launch_baseline(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<LaunchBaseline>, SnapshotError>;

    /// Persist whether runtime snapshot hooks are currently registered.
    fn set_hooks_registered_state(
        &self,
        _state_key: &WorkspaceStateKey,
        _hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        Ok(())
    }
}

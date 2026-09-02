//! Call-counting decorator for [`SnapshotStorePort`].
//!
//! Wraps any inner port and increments per-method counters on every call.
//! Used by tests that must prove a control-flow gate keeps the snapshot
//! runtime branch from touching the store at all (e.g. `snapshot.enabled =
//! false` open paths per the snapshot directory creation policy).
//!
//! The wrapper deliberately delegates state to the inner store rather than
//! mocking responses: tests assert on the counters, not on simulated data.

use std::cell::Cell;

use workhive_core::domain::{
    LaunchBaseline, SnapshotEvent, SnapshotHealthCheck, SnapshotHistoryEntry, SnapshotManifest,
    SnapshotRevisionId, SupervisorLock, WorkspaceSnapshot, WorkspaceStateKey,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::SnapshotStorePort;

/// Snapshot of how many times each [`SnapshotStorePort`] method was invoked.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotStoreCallCounts {
    pub save_revision: u32,
    pub load_latest: u32,
    pub load_revision: u32,
    pub delete_all: u32,
    pub delete_all_global: u32,
    pub save_launch_baseline: u32,
    pub load_launch_baseline: u32,
    pub set_hooks_registered_state: u32,
    pub append_events: u32,
    pub apply_retention: u32,
}

impl SnapshotStoreCallCounts {
    /// Returns `true` iff none of the runtime-mutation methods were called.
    pub fn no_runtime_calls(&self) -> bool {
        self.save_revision == 0
            && self.load_latest == 0
            && self.load_revision == 0
            && self.delete_all == 0
            && self.delete_all_global == 0
            && self.save_launch_baseline == 0
            && self.set_hooks_registered_state == 0
            && self.append_events == 0
            && self.apply_retention == 0
    }
}

/// Decorator that records calls into [`SnapshotStoreCallCounts`] before
/// delegating to the inner port.
pub struct TrackingSnapshotStore<S: SnapshotStorePort> {
    inner: S,
    save_revision: Cell<u32>,
    load_latest: Cell<u32>,
    load_revision: Cell<u32>,
    delete_all: Cell<u32>,
    delete_all_global: Cell<u32>,
    save_launch_baseline: Cell<u32>,
    load_launch_baseline: Cell<u32>,
    set_hooks_registered_state: Cell<u32>,
    append_events: Cell<u32>,
    apply_retention: Cell<u32>,
}

impl<S: SnapshotStorePort> TrackingSnapshotStore<S> {
    /// Wrap `inner` and reset all counters to zero.
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            save_revision: Cell::new(0),
            load_latest: Cell::new(0),
            load_revision: Cell::new(0),
            delete_all: Cell::new(0),
            delete_all_global: Cell::new(0),
            save_launch_baseline: Cell::new(0),
            load_launch_baseline: Cell::new(0),
            set_hooks_registered_state: Cell::new(0),
            append_events: Cell::new(0),
            apply_retention: Cell::new(0),
        }
    }

    /// Snapshot of current call counts.
    pub fn calls(&self) -> SnapshotStoreCallCounts {
        SnapshotStoreCallCounts {
            save_revision: self.save_revision.get(),
            load_latest: self.load_latest.get(),
            load_revision: self.load_revision.get(),
            delete_all: self.delete_all.get(),
            delete_all_global: self.delete_all_global.get(),
            save_launch_baseline: self.save_launch_baseline.get(),
            load_launch_baseline: self.load_launch_baseline.get(),
            set_hooks_registered_state: self.set_hooks_registered_state.get(),
            append_events: self.append_events.get(),
            apply_retention: self.apply_retention.get(),
        }
    }
}

fn bump(cell: &Cell<u32>) {
    cell.set(cell.get().saturating_add(1));
}

impl<S: SnapshotStorePort> SnapshotStorePort for TrackingSnapshotStore<S> {
    fn save_revision(
        &self,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<SnapshotRevisionId, SnapshotError> {
        bump(&self.save_revision);
        self.inner.save_revision(snapshot)
    }

    fn load_latest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        bump(&self.load_latest);
        self.inner.load_latest(state_key)
    }

    fn load_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        bump(&self.load_revision);
        self.inner.load_revision(state_key, revision_id)
    }

    fn append_events(
        &self,
        state_key: &WorkspaceStateKey,
        events: &[SnapshotEvent],
    ) -> Result<(), SnapshotError> {
        bump(&self.append_events);
        self.inner.append_events(state_key, events)
    }

    fn load_events_since(
        &self,
        state_key: &WorkspaceStateKey,
        since_revision: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
        self.inner.load_events_since(state_key, since_revision)
    }

    fn read_manifest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError> {
        self.inner.read_manifest(state_key)
    }

    fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
        self.inner.write_manifest(manifest)
    }

    fn list_history(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
        self.inner.list_history(state_key)
    }

    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError> {
        self.inner.acquire_lock(lock)
    }

    fn release_lock(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        self.inner.release_lock(state_key)
    }

    fn update_heartbeat(
        &self,
        state_key: &WorkspaceStateKey,
        heartbeat: &str,
    ) -> Result<(), SnapshotError> {
        self.inner.update_heartbeat(state_key, heartbeat)
    }

    fn read_lock(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SupervisorLock>, SnapshotError> {
        self.inner.read_lock(state_key)
    }

    fn health_check(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<SnapshotHealthCheck, SnapshotError> {
        self.inner.health_check(state_key)
    }

    fn verify_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError> {
        self.inner.verify_revision(state_key, revision_id)
    }

    fn apply_retention(
        &self,
        state_key: &WorkspaceStateKey,
        retain_revisions: u32,
        retain_event_days: u32,
    ) -> Result<u32, SnapshotError> {
        bump(&self.apply_retention);
        self.inner
            .apply_retention(state_key, retain_revisions, retain_event_days)
    }

    fn delete_all(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        bump(&self.delete_all);
        self.inner.delete_all(state_key)
    }

    fn delete_all_global(&self) -> Result<(), SnapshotError> {
        bump(&self.delete_all_global);
        self.inner.delete_all_global()
    }

    fn save_launch_baseline(&self, baseline: &LaunchBaseline) -> Result<(), SnapshotError> {
        bump(&self.save_launch_baseline);
        self.inner.save_launch_baseline(baseline)
    }

    fn load_launch_baseline(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<LaunchBaseline>, SnapshotError> {
        bump(&self.load_launch_baseline);
        self.inner.load_launch_baseline(state_key)
    }

    fn set_hooks_registered_state(
        &self,
        state_key: &WorkspaceStateKey,
        hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        bump(&self.set_hooks_registered_state);
        self.inner
            .set_hooks_registered_state(state_key, hooks_registered)
    }
}

//! In-memory fake for [`SnapshotStorePort`].
//!
//! Stores revisions, events, manifests, and locks in `RefCell<HashMap>`
//! collections keyed by `WorkspaceStateKey`.

use std::cell::RefCell;
use std::collections::HashMap;

use workhive_core::domain::{
    LaunchBaseline, SnapshotEvent, SnapshotHealthCheck, SnapshotHistoryEntry, SnapshotManifest,
    SnapshotRevisionId, SupervisorLock, WorkspaceSnapshot, WorkspaceStateKey,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::SnapshotStorePort;
use workhive_core::usecases::snapshot_capture::compute_content_hash;

/// In-memory fake implementing [`SnapshotStorePort`].
pub struct FakeSnapshotStore {
    revisions: RefCell<HashMap<String, Vec<WorkspaceSnapshot>>>,
    events: RefCell<HashMap<String, Vec<SnapshotEvent>>>,
    manifests: RefCell<HashMap<String, SnapshotManifest>>,
    locks: RefCell<HashMap<String, SupervisorLock>>,
    baselines: RefCell<HashMap<String, LaunchBaseline>>,
    hooks_registered: RefCell<HashMap<String, bool>>,
}

impl FakeSnapshotStore {
    /// Create a new, empty fake store.
    pub fn new() -> Self {
        Self {
            revisions: RefCell::new(HashMap::new()),
            events: RefCell::new(HashMap::new()),
            manifests: RefCell::new(HashMap::new()),
            locks: RefCell::new(HashMap::new()),
            baselines: RefCell::new(HashMap::new()),
            hooks_registered: RefCell::new(HashMap::new()),
        }
    }

    fn key(state_key: &WorkspaceStateKey) -> String {
        state_key.to_file_key()
    }
}

impl Default for FakeSnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotStorePort for FakeSnapshotStore {
    fn save_revision(
        &self,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<SnapshotRevisionId, SnapshotError> {
        let key = Self::key(&snapshot.state_key);
        let mut snapshot_to_store = snapshot.clone();
        if snapshot_to_store.metadata.content_hash.is_none() {
            snapshot_to_store.metadata.content_hash =
                Some(compute_content_hash(&snapshot_to_store)?);
        }
        let rev = snapshot_to_store.revision_id;
        self.revisions
            .borrow_mut()
            .entry(key.clone())
            .or_default()
            .push(snapshot_to_store.clone());
        let mut manifests = self.manifests.borrow_mut();
        let manifest = manifests.entry(key).or_insert(SnapshotManifest {
            state_key: snapshot_to_store.state_key.clone(),
            head_revision: rev,
            total_revisions: 0,
            events_since_full: 0,
            last_saved_at: snapshot_to_store.captured_at.clone(),
            last_full_revision_at: snapshot_to_store.captured_at.clone(),
        });
        manifest.head_revision = rev;
        manifest.total_revisions = manifest.total_revisions.max(rev.0);
        manifest.events_since_full = 0;
        manifest.last_saved_at = snapshot_to_store.captured_at.clone();
        manifest.last_full_revision_at = snapshot_to_store.captured_at;
        Ok(rev)
    }

    fn load_latest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        let key = Self::key(state_key);
        if let Some(manifest) = self.manifests.borrow().get(&key) {
            return self.load_revision(state_key, manifest.head_revision);
        }
        Ok(self
            .revisions
            .borrow()
            .get(&key)
            .and_then(|revs| revs.iter().max_by_key(|s| s.revision_id).cloned()))
    }

    fn load_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self
            .revisions
            .borrow()
            .get(&key)
            .and_then(|revs| revs.iter().find(|s| s.revision_id == revision_id).cloned()))
    }

    fn append_events(
        &self,
        state_key: &WorkspaceStateKey,
        events: &[SnapshotEvent],
    ) -> Result<(), SnapshotError> {
        let key = Self::key(state_key);
        self.events
            .borrow_mut()
            .entry(key)
            .or_default()
            .extend(events.iter().cloned());
        Ok(())
    }

    fn load_events_since(
        &self,
        state_key: &WorkspaceStateKey,
        since_revision: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self
            .events
            .borrow()
            .get(&key)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|event| {
                match event
                    .revision_id
                    .or_else(|| parse_revision_from_detail(event.detail.as_deref()))
                {
                    Some(rev) => rev > since_revision,
                    None => since_revision.0 == 0,
                }
            })
            .collect())
    }

    fn read_manifest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self.manifests.borrow().get(&key).cloned())
    }

    fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
        let key = Self::key(&manifest.state_key);
        self.manifests.borrow_mut().insert(key, manifest.clone());
        Ok(())
    }

    fn list_history(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self
            .revisions
            .borrow()
            .get(&key)
            .map(|revs| {
                revs.iter()
                    .map(|s| SnapshotHistoryEntry {
                        revision_id: s.revision_id,
                        captured_at: s.captured_at.clone(),
                        source: s.source.clone(),
                        pane_count: s.panes.len(),
                        window_count: s.topology.windows.len(),
                        content_hash: s.metadata.content_hash.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError> {
        let key = Self::key(&lock.state_key);
        self.locks.borrow_mut().insert(key, lock.clone());
        Ok(true)
    }

    fn release_lock(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let key = Self::key(state_key);
        self.locks.borrow_mut().remove(&key);
        Ok(())
    }

    fn update_heartbeat(
        &self,
        state_key: &WorkspaceStateKey,
        heartbeat: &str,
    ) -> Result<(), SnapshotError> {
        let key = Self::key(state_key);
        if let Some(lock) = self.locks.borrow_mut().get_mut(&key) {
            lock.heartbeat = heartbeat.to_owned();
            Ok(())
        } else {
            Err(SnapshotError::LockConflict {
                state_key: state_key.to_string(),
                holder_pid: 0,
            })
        }
    }

    fn read_lock(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SupervisorLock>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self.locks.borrow().get(&key).cloned())
    }

    fn health_check(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<SnapshotHealthCheck, SnapshotError> {
        let has_revisions = self
            .revisions
            .borrow()
            .get(&Self::key(state_key))
            .is_some_and(|r| !r.is_empty());
        let latest_rev = self
            .revisions
            .borrow()
            .get(&Self::key(state_key))
            .and_then(|r| r.last().map(|s| s.revision_id));

        Ok(SnapshotHealthCheck {
            enabled: has_revisions || self.manifests.borrow().contains_key(&Self::key(state_key)),
            supervisor_running: self.locks.borrow().contains_key(&Self::key(state_key)),
            hooks_registered: self
                .hooks_registered
                .borrow()
                .get(&Self::key(state_key))
                .copied()
                .unwrap_or(false),
            latest_save_time: self
                .manifests
                .borrow()
                .get(&Self::key(state_key))
                .map(|m| m.last_saved_at.clone()),
            latest_revision: latest_rev,
            manifest_readable: self.manifests.borrow().contains_key(&Self::key(state_key)),
            checksum_valid: latest_rev.map(|revision_id| {
                self.verify_revision(state_key, revision_id)
                    .unwrap_or(false)
            }),
            state_key_match: true,
        })
    }

    fn verify_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self
            .revisions
            .borrow()
            .get(&key)
            .and_then(|r| r.iter().find(|s| s.revision_id == revision_id))
            .is_some_and(|snapshot| {
                snapshot
                    .metadata
                    .content_hash
                    .as_deref()
                    .and_then(|expected| {
                        compute_content_hash(snapshot)
                            .ok()
                            .map(|actual| expected == actual)
                    })
                    .unwrap_or(false)
            }))
    }

    fn apply_retention(
        &self,
        state_key: &WorkspaceStateKey,
        retain_revisions: u32,
        _retain_event_days: u32,
    ) -> Result<u32, SnapshotError> {
        let key = Self::key(state_key);
        let mut revs = self.revisions.borrow_mut();
        let revisions = revs.entry(key).or_default();
        let to_remove = revisions.len().saturating_sub(retain_revisions as usize);
        let removed = to_remove as u32;
        *revisions = revisions.split_off(to_remove);
        Ok(removed)
    }

    fn delete_all(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let key = Self::key(state_key);
        self.revisions.borrow_mut().remove(&key);
        self.events.borrow_mut().remove(&key);
        self.manifests.borrow_mut().remove(&key);
        self.locks.borrow_mut().remove(&key);
        self.baselines.borrow_mut().remove(&key);
        self.hooks_registered.borrow_mut().remove(&key);
        Ok(())
    }

    fn delete_all_global(&self) -> Result<(), SnapshotError> {
        self.revisions.borrow_mut().clear();
        self.events.borrow_mut().clear();
        self.manifests.borrow_mut().clear();
        self.locks.borrow_mut().clear();
        self.baselines.borrow_mut().clear();
        self.hooks_registered.borrow_mut().clear();
        Ok(())
    }

    fn save_launch_baseline(&self, baseline: &LaunchBaseline) -> Result<(), SnapshotError> {
        let key = Self::key(&baseline.state_key);
        self.baselines.borrow_mut().insert(key, baseline.clone());
        Ok(())
    }

    fn load_launch_baseline(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<LaunchBaseline>, SnapshotError> {
        let key = Self::key(state_key);
        Ok(self.baselines.borrow().get(&key).cloned())
    }

    fn set_hooks_registered_state(
        &self,
        state_key: &WorkspaceStateKey,
        hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        let key = Self::key(state_key);
        self.hooks_registered
            .borrow_mut()
            .insert(key, hooks_registered);
        Ok(())
    }
}

fn parse_revision_from_detail(detail: Option<&str>) -> Option<SnapshotRevisionId> {
    let detail = detail?;
    let marker = "revision=";
    let start = detail.find(marker)? + marker.len();
    let digits: String = detail[start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok().map(SnapshotRevisionId)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use workhive_core::domain::{
        PaneLaunchIntent, PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy,
        SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, SnapshotSource, WindowSnapshot,
        WorkspaceTopology,
    };

    fn state_key() -> WorkspaceStateKey {
        WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "proj".to_owned(),
            String::new(),
            "main".to_owned(),
        )
    }

    fn sample_snapshot(rev: u64) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            revision_id: SnapshotRevisionId(rev),
            state_key: state_key(),
            captured_at: format!("2026-04-24T12:{rev:02}:00Z"),
            source: SnapshotSource::ManualSave,
            topology: WorkspaceTopology {
                windows: vec![WindowSnapshot {
                    index: 0,
                    name: None,
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
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
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
                generation: rev,
                content_hash: None,
                config_fingerprint: None,
            },
        }
    }

    #[test]
    fn roundtrip_save_load() {
        let store = FakeSnapshotStore::new();
        store.save_revision(&sample_snapshot(1)).expect("save");
        let loaded = store
            .load_latest(&state_key())
            .expect("load")
            .expect("exists");
        assert_eq!(loaded.revision_id, SnapshotRevisionId(1));
    }

    #[test]
    fn retention_prunes() {
        let store = FakeSnapshotStore::new();
        for i in 1..=5 {
            store.save_revision(&sample_snapshot(i)).expect("save");
        }
        let removed = store.apply_retention(&state_key(), 2, 30).expect("ret");
        assert_eq!(removed, 3);
        let history = store.list_history(&state_key()).expect("hist");
        assert_eq!(history.len(), 2);
    }
}

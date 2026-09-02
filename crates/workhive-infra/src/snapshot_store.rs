//! File-based snapshot store adapter.
//!
//! Directory layout under `.workhive/snapshots/<state-key-file-key>/`:
//!
//! ```text
//! revisions/
//!   000001.json
//!   000002.json
//! events/
//!   000001.jsonl  (append-only event log per revision epoch)
//! manifest.json
//! lock.json
//! hooks_state.json
//! ```
//!
//! Writes use atomic rename via `tempfile::NamedTempFile`.

use camino::{Utf8Path, Utf8PathBuf};
use tempfile::NamedTempFile;
use workhive_core::domain::{
    LaunchBaseline, SnapshotEvent, SnapshotHealthCheck, SnapshotHistoryEntry, SnapshotManifest,
    SnapshotRevisionId, SupervisorLock, WorkspaceSnapshot, WorkspaceStateKey,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::SnapshotStorePort;
use workhive_core::usecases::snapshot_capture::compute_content_hash;

/// File-system implementation of [`SnapshotStorePort`].
pub struct FileSnapshotStoreAdapter {
    root: Utf8PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct HookRegistrationState {
    hooks_registered: bool,
    updated_at: String,
}

impl FileSnapshotStoreAdapter {
    /// Create a new adapter rooted at a project directory.
    pub fn new(project_root: &Utf8Path) -> Self {
        Self {
            root: project_root.join(".workhive").join("snapshots"),
        }
    }

    fn scope_dir(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.root.join(state_key.to_file_key())
    }

    fn revisions_dir(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("revisions")
    }

    fn events_dir(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("events")
    }

    fn revision_path(&self, state_key: &WorkspaceStateKey, rev: SnapshotRevisionId) -> Utf8PathBuf {
        self.revisions_dir(state_key).join(format!("{rev}.json"))
    }

    fn events_path(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.events_dir(state_key).join("events.jsonl")
    }

    fn manifest_path(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("manifest.json")
    }

    fn lock_path(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("lock.json")
    }

    fn hooks_state_path(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("hooks_state.json")
    }

    fn launch_baseline_path(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.scope_dir(state_key).join("launch_baseline.json")
    }

    fn read_hooks_state(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<HookRegistrationState>, SnapshotError> {
        self.read_json_opt(&self.hooks_state_path(state_key))
    }

    /// Persist whether snapshot hooks are currently registered for this workspace.
    pub fn set_hooks_registered_state(
        &self,
        state_key: &WorkspaceStateKey,
        hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        let state = HookRegistrationState {
            hooks_registered,
            updated_at: format!("{}s", epoch_seconds_now().unwrap_or(0)),
        };
        self.atomic_write_json(&self.hooks_state_path(state_key), &state)
    }

    fn atomic_write_json<T: serde::Serialize>(
        &self,
        target: &Utf8Path,
        value: &T,
    ) -> Result<(), SnapshotError> {
        let parent = target.parent().unwrap_or(target);
        std::fs::create_dir_all(parent.as_std_path()).map_err(|source| {
            SnapshotError::SaveFailed {
                path: parent.to_owned(),
                source,
            }
        })?;

        let json = serde_json::to_string_pretty(value).map_err(|e| SnapshotError::Corrupted {
            path: target.to_owned(),
            reason: format!("serialization error: {e}"),
        })?;

        let mut tmp = NamedTempFile::new_in(parent.as_std_path()).map_err(|source| {
            SnapshotError::SaveFailed {
                path: target.to_owned(),
                source,
            }
        })?;
        use std::io::Write;
        tmp.write_all(json.as_bytes())
            .and_then(|_| tmp.flush())
            .map_err(|source| SnapshotError::SaveFailed {
                path: target.to_owned(),
                source,
            })?;
        tmp.persist(target.as_std_path())
            .map_err(|e| SnapshotError::SaveFailed {
                path: target.to_owned(),
                source: e.error,
            })?;
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &Utf8Path,
    ) -> Result<T, SnapshotError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| SnapshotError::ReadFailed {
                path: path.to_owned(),
                source,
            })?;
        serde_json::from_str(&content).map_err(|e| SnapshotError::Corrupted {
            path: path.to_owned(),
            reason: e.to_string(),
        })
    }

    fn read_json_opt<T: serde::de::DeserializeOwned>(
        &self,
        path: &Utf8Path,
    ) -> Result<Option<T>, SnapshotError> {
        if !path.exists() {
            return Ok(None);
        }
        self.read_json(path).map(Some)
    }

    /// Find the highest revision ID in the revisions directory.
    fn find_latest_revision_id(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotRevisionId>, SnapshotError> {
        let dir = self.revisions_dir(state_key);
        if !dir.exists() {
            return Ok(None);
        }
        let entries =
            std::fs::read_dir(dir.as_std_path()).map_err(|source| SnapshotError::ReadFailed {
                path: dir.clone(),
                source,
            })?;

        let mut max_id: Option<u64> = None;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stem) = name_str.strip_suffix(".json")
                && let Ok(id) = stem.parse::<u64>()
            {
                max_id = Some(max_id.map_or(id, |m: u64| m.max(id)));
            }
        }
        Ok(max_id.map(SnapshotRevisionId))
    }

    fn merge_manifest(
        &self,
        existing: Option<SnapshotManifest>,
        snapshot: &WorkspaceSnapshot,
    ) -> SnapshotManifest {
        let mut manifest = existing.unwrap_or(SnapshotManifest {
            state_key: snapshot.state_key.clone(),
            head_revision: snapshot.revision_id,
            total_revisions: 0,
            events_since_full: 0,
            last_saved_at: snapshot.captured_at.clone(),
            last_full_revision_at: snapshot.captured_at.clone(),
        });
        manifest.state_key = snapshot.state_key.clone();
        manifest.head_revision = snapshot.revision_id;
        manifest.total_revisions = manifest.total_revisions.max(snapshot.revision_id.0);
        manifest.events_since_full = 0;
        manifest.last_saved_at = snapshot.captured_at.clone();
        manifest.last_full_revision_at = snapshot.captured_at.clone();
        manifest
    }

    fn event_revision(event: &SnapshotEvent) -> Option<SnapshotRevisionId> {
        event
            .revision_id
            .or_else(|| parse_revision_from_detail(event.detail.as_deref()))
    }

    fn prune_event_log(
        &self,
        state_key: &WorkspaceStateKey,
        retain_event_days: u32,
    ) -> Result<(), SnapshotError> {
        let path = self.events_path(state_key);
        if !path.exists() {
            return Ok(());
        }
        if retain_event_days == 0 {
            std::fs::remove_file(path.as_std_path()).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: path.clone(),
                    source,
                }
            })?;
            return Ok(());
        }

        let cutoff = retention_cutoff_epoch_seconds(retain_event_days);
        if cutoff.is_none() {
            return Ok(());
        }
        let cutoff = cutoff.expect("checked is_some");
        let content = std::fs::read_to_string(path.as_std_path()).map_err(|source| {
            SnapshotError::ReadFailed {
                path: path.clone(),
                source,
            }
        })?;
        let mut kept = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let event: SnapshotEvent =
                serde_json::from_str(line).map_err(|e| SnapshotError::Corrupted {
                    path: path.clone(),
                    reason: format!("event line parse error: {e}"),
                })?;
            match parse_epoch_seconds(&event.timestamp) {
                Some(event_ts) if event_ts < cutoff => {}
                _ => kept.push(event),
            }
        }

        if kept.is_empty() {
            std::fs::remove_file(path.as_std_path()).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: path.clone(),
                    source,
                }
            })?;
            return Ok(());
        }
        self.atomic_write_jsonl(&path, &kept)
    }

    fn atomic_write_jsonl<T: serde::Serialize>(
        &self,
        target: &Utf8Path,
        entries: &[T],
    ) -> Result<(), SnapshotError> {
        let parent = target.parent().unwrap_or(target);
        std::fs::create_dir_all(parent.as_std_path()).map_err(|source| {
            SnapshotError::SaveFailed {
                path: parent.to_owned(),
                source,
            }
        })?;

        let mut tmp = NamedTempFile::new_in(parent.as_std_path()).map_err(|source| {
            SnapshotError::SaveFailed {
                path: target.to_owned(),
                source,
            }
        })?;
        use std::io::Write;
        for entry in entries {
            let line = serde_json::to_string(entry).map_err(|e| SnapshotError::Corrupted {
                path: target.to_owned(),
                reason: format!("event serialization error: {e}"),
            })?;
            writeln!(tmp, "{line}").map_err(|source| SnapshotError::SaveFailed {
                path: target.to_owned(),
                source,
            })?;
        }
        tmp.flush().map_err(|source| SnapshotError::SaveFailed {
            path: target.to_owned(),
            source,
        })?;
        tmp.persist(target.as_std_path())
            .map_err(|e| SnapshotError::SaveFailed {
                path: target.to_owned(),
                source: e.error,
            })?;
        Ok(())
    }
}

impl SnapshotStorePort for FileSnapshotStoreAdapter {
    fn save_revision(
        &self,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<SnapshotRevisionId, SnapshotError> {
        let mut snapshot_to_store = snapshot.clone();
        if snapshot_to_store.metadata.content_hash.is_none() {
            snapshot_to_store.metadata.content_hash =
                Some(compute_content_hash(&snapshot_to_store)?);
        }
        let path = self.revision_path(&snapshot_to_store.state_key, snapshot_to_store.revision_id);
        self.atomic_write_json(&path, &snapshot_to_store)?;
        let manifest = self.read_manifest(&snapshot_to_store.state_key)?;
        let merged_manifest = self.merge_manifest(manifest, &snapshot_to_store);
        self.write_manifest(&merged_manifest)?;
        tracing::debug!(revision = %snapshot_to_store.revision_id, path = %path, "snapshot revision saved");
        Ok(snapshot_to_store.revision_id)
    }

    fn load_latest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        let manifest = self.read_manifest(state_key)?;
        if let Some(m) = manifest {
            return self.load_revision(state_key, m.head_revision);
        }
        match self.find_latest_revision_id(state_key)? {
            Some(rev_id) => self.load_revision(state_key, rev_id),
            None => Ok(None),
        }
    }

    fn load_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        let path = self.revision_path(state_key, revision_id);
        self.read_json_opt(&path)
    }

    fn append_events(
        &self,
        state_key: &WorkspaceStateKey,
        events: &[SnapshotEvent],
    ) -> Result<(), SnapshotError> {
        let dir = self.events_dir(state_key);
        std::fs::create_dir_all(dir.as_std_path()).map_err(|source| SnapshotError::SaveFailed {
            path: dir.clone(),
            source,
        })?;

        let path = self.events_path(state_key);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_std_path())
            .map_err(|source| SnapshotError::SaveFailed {
                path: path.clone(),
                source,
            })?;

        use std::io::Write;
        for event in events {
            let line = serde_json::to_string(event).map_err(|e| SnapshotError::Corrupted {
                path: path.clone(),
                reason: format!("event serialization error: {e}"),
            })?;
            writeln!(file, "{line}").map_err(|source| SnapshotError::SaveFailed {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }

    fn load_events_since(
        &self,
        state_key: &WorkspaceStateKey,
        since_revision: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
        let path = self.events_path(state_key);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(path.as_std_path()).map_err(|source| {
            SnapshotError::ReadFailed {
                path: path.clone(),
                source,
            }
        })?;

        let mut events = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let event: SnapshotEvent =
                serde_json::from_str(line).map_err(|e| SnapshotError::Corrupted {
                    path: path.clone(),
                    reason: format!("event line parse error: {e}"),
                })?;
            let include = match Self::event_revision(&event) {
                Some(revision) => revision > since_revision,
                None => since_revision.0 == 0,
            };
            if include {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn read_manifest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError> {
        let path = self.manifest_path(state_key);
        self.read_json_opt(&path)
    }

    fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
        let path = self.manifest_path(&manifest.state_key);
        self.atomic_write_json(&path, manifest)
    }

    fn list_history(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
        let dir = self.revisions_dir(state_key);
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let entries =
            std::fs::read_dir(dir.as_std_path()).map_err(|source| SnapshotError::ReadFailed {
                path: dir.clone(),
                source,
            })?;

        let mut history = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.ends_with(".json") {
                continue;
            }
            let Ok(utf8_path) = Utf8PathBuf::try_from(entry.path()) else {
                continue;
            };
            let snapshot: WorkspaceSnapshot = self.read_json(&utf8_path)?;
            history.push(SnapshotHistoryEntry {
                revision_id: snapshot.revision_id,
                captured_at: snapshot.captured_at.clone(),
                source: snapshot.source.clone(),
                pane_count: snapshot.panes.len(),
                window_count: snapshot.topology.windows.len(),
                content_hash: snapshot.metadata.content_hash.clone(),
            });
        }

        history.sort_by_key(|h| h.revision_id);
        Ok(history)
    }

    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError> {
        let path = self.lock_path(&lock.state_key);
        if path.exists() {
            let existing: SupervisorLock = self.read_json(&path)?;
            if existing.pid != lock.pid {
                // Check if holder is still alive
                let alive = is_process_alive(existing.pid);
                if alive {
                    return Ok(false);
                }
                tracing::warn!(
                    stale_pid = existing.pid,
                    "stale supervisor lock detected, overwriting"
                );
            }
        }
        self.atomic_write_json(&path, lock)?;
        Ok(true)
    }

    fn release_lock(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let path = self.lock_path(state_key);
        if path.exists() {
            std::fs::remove_file(path.as_std_path()).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: path.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }

    fn update_heartbeat(
        &self,
        state_key: &WorkspaceStateKey,
        heartbeat: &str,
    ) -> Result<(), SnapshotError> {
        let path = self.lock_path(state_key);
        if !path.exists() {
            return Err(SnapshotError::LockConflict {
                state_key: state_key.to_string(),
                holder_pid: 0,
            });
        }
        let mut lock: SupervisorLock = self.read_json(&path)?;
        lock.heartbeat = heartbeat.to_owned();
        self.atomic_write_json(&path, &lock)
    }

    fn read_lock(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SupervisorLock>, SnapshotError> {
        let path = self.lock_path(state_key);
        self.read_json_opt(&path)
    }

    fn health_check(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<SnapshotHealthCheck, SnapshotError> {
        let manifest = self.read_manifest(state_key)?;
        let lock = self.read_lock(state_key)?;
        let hooks_state = self.read_hooks_state(state_key)?;
        let latest_rev = manifest
            .as_ref()
            .map(|m| m.head_revision)
            .or(self.find_latest_revision_id(state_key)?);

        let checksum_valid = if let Some(rev) = latest_rev {
            Some(self.verify_revision(state_key, rev)?)
        } else {
            None
        };

        let latest_snapshot = if let Some(rev) = latest_rev {
            self.load_revision(state_key, rev)?
        } else {
            None
        };
        let latest_save_time = manifest
            .as_ref()
            .map(|m| m.last_saved_at.clone())
            .or_else(|| latest_snapshot.as_ref().map(|s| s.captured_at.clone()));
        let supervisor_running = lock.as_ref().is_some_and(|l| is_process_alive(l.pid));
        let hooks_registered = hooks_state
            .as_ref()
            .is_some_and(|state| state.hooks_registered);
        let state_key_match = manifest
            .as_ref()
            .map(|m| m.state_key == *state_key)
            .unwrap_or(true)
            && latest_snapshot
                .as_ref()
                .map(|s| s.state_key == *state_key)
                .unwrap_or(true);

        Ok(SnapshotHealthCheck {
            enabled: manifest.is_some()
                || latest_rev.is_some()
                || lock.is_some()
                || hooks_state.is_some(),
            supervisor_running,
            hooks_registered,
            latest_save_time,
            latest_revision: latest_rev,
            manifest_readable: manifest.is_some(),
            checksum_valid,
            state_key_match,
        })
    }

    fn verify_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError> {
        let path = self.revision_path(state_key, revision_id);
        if !path.exists() {
            return Ok(false);
        }
        let snap: WorkspaceSnapshot = self.read_json(&path)?;
        if snap.revision_id != revision_id || snap.state_key != *state_key {
            return Ok(false);
        }
        let Some(expected_hash) = snap.metadata.content_hash.as_deref() else {
            tracing::warn!(revision = %revision_id, "snapshot revision is missing content hash");
            return Ok(false);
        };
        let actual_hash = compute_content_hash(&snap)?;
        if actual_hash == expected_hash {
            Ok(true)
        } else {
            tracing::warn!(
                revision = %revision_id,
                expected = %expected_hash,
                found = %actual_hash,
                "snapshot checksum mismatch"
            );
            Ok(false)
        }
    }

    fn apply_retention(
        &self,
        state_key: &WorkspaceStateKey,
        retain_revisions: u32,
        retain_event_days: u32,
    ) -> Result<u32, SnapshotError> {
        let dir = self.revisions_dir(state_key);
        if !dir.exists() {
            self.prune_event_log(state_key, retain_event_days)?;
            return Ok(0);
        }

        let mut rev_files: Vec<(u64, std::path::PathBuf)> = Vec::new();
        let entries =
            std::fs::read_dir(dir.as_std_path()).map_err(|source| SnapshotError::ReadFailed {
                path: dir.clone(),
                source,
            })?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(stem) = name_str.strip_suffix(".json")
                && let Ok(id) = stem.parse::<u64>()
            {
                rev_files.push((id, entry.path()));
            }
        }

        rev_files.sort_by_key(|(id, _)| *id);

        let to_remove = rev_files.len().saturating_sub(retain_revisions as usize);
        let mut removed = 0u32;
        for (_, path) in rev_files.iter().take(to_remove) {
            if std::fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
        self.prune_event_log(state_key, retain_event_days)?;

        let manifest_path = self.manifest_path(state_key);
        let latest_revision = self.find_latest_revision_id(state_key)?;
        match (self.read_manifest(state_key)?, latest_revision) {
            (Some(mut manifest), Some(head_revision)) => {
                manifest.head_revision = head_revision;
                manifest.total_revisions = rev_files.len().saturating_sub(to_remove) as u64;
                if let Some(snapshot) = self.load_revision(state_key, head_revision)? {
                    manifest.last_saved_at = snapshot.captured_at.clone();
                    manifest.last_full_revision_at = snapshot.captured_at;
                }
                self.write_manifest(&manifest)?;
            }
            (Some(_), None) => {
                if manifest_path.exists() {
                    std::fs::remove_file(manifest_path.as_std_path()).map_err(|source| {
                        SnapshotError::DeleteFailed {
                            path: manifest_path.clone(),
                            source,
                        }
                    })?;
                }
            }
            (None, _) => {}
        }
        Ok(removed)
    }

    fn delete_all(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let dir = self.scope_dir(state_key);
        if dir.exists() {
            std::fs::remove_dir_all(dir.as_std_path()).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: dir.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }

    fn delete_all_global(&self) -> Result<(), SnapshotError> {
        if self.root.exists() {
            std::fs::remove_dir_all(self.root.as_std_path()).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: self.root.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }

    fn save_launch_baseline(&self, baseline: &LaunchBaseline) -> Result<(), SnapshotError> {
        self.atomic_write_json(&self.launch_baseline_path(&baseline.state_key), baseline)
    }

    fn load_launch_baseline(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<LaunchBaseline>, SnapshotError> {
        self.read_json_opt(&self.launch_baseline_path(state_key))
    }

    fn set_hooks_registered_state(
        &self,
        state_key: &WorkspaceStateKey,
        hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        FileSnapshotStoreAdapter::set_hooks_registered_state(self, state_key, hooks_registered)
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

fn parse_epoch_seconds(raw: &str) -> Option<u64> {
    if let Some(stripped) = raw.strip_suffix('s') {
        return stripped.parse::<u64>().ok();
    }
    raw.parse::<u64>().ok()
}

fn epoch_seconds_now() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn retention_cutoff_epoch_seconds(retain_event_days: u32) -> Option<u64> {
    let now = epoch_seconds_now()?;
    let seconds = u64::from(retain_event_days) * 24 * 60 * 60;
    now.checked_sub(seconds)
}

/// Best-effort PID liveness check using `kill -0`.
fn is_process_alive(pid: u32) -> bool {
    match std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(source) => {
            tracing::warn!(pid, error = %source, "failed to check snapshot supervisor process");
            false
        }
    }
}

#[cfg(test)]
#[path = "snapshot_store_tests.rs"]
mod tests;

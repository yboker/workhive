//! JSON file-based snapshot adapter.
//!
//! Snapshots are stored at `.workhive/snapshots/<state-key>.json`.
//!
//! Save uses atomic write: serialize to a temp file in the target directory,
//! then rename into the final location. This prevents partial/corrupt writes.

use camino::Utf8PathBuf;
use tempfile::NamedTempFile;
use workhive_core::domain::{WorkspaceId, WorkspaceStateKey};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::{
    LegacySnapshotDocument as SnapshotDocument, LegacySnapshotMetadata as SnapshotMetadata,
    LegacySnapshotPort as SnapshotPort,
};

/// File-based snapshot adapter rooted at a project directory.
pub struct FileSnapshotAdapter {
    snapshots_dir: Utf8PathBuf,
}

impl FileSnapshotAdapter {
    pub fn new(project_root: &camino::Utf8Path) -> Self {
        Self {
            snapshots_dir: project_root.join(".workhive").join("snapshots"),
        }
    }

    fn snapshot_path_for_state_key(&self, state_key: &WorkspaceStateKey) -> Utf8PathBuf {
        self.snapshots_dir
            .join(format!("{}.json", state_key.to_file_key()))
    }

    fn legacy_snapshot_path(&self, workspace: &str) -> Utf8PathBuf {
        self.snapshots_dir.join(format!("{workspace}.json"))
    }

    fn state_key_from_document(
        snapshot: &SnapshotDocument,
        path: &Utf8PathBuf,
    ) -> Result<WorkspaceStateKey, SnapshotError> {
        let state_key = snapshot
            .identity
            .state_key
            .as_deref()
            .ok_or_else(|| SnapshotError::MissingStateKey { path: path.clone() })?;
        WorkspaceStateKey::parse(state_key).ok_or_else(|| SnapshotError::Corrupted {
            path: path.clone(),
            reason: format!("invalid state_key format: {state_key}"),
        })
    }

    fn read_snapshot(&self, path: &Utf8PathBuf) -> Result<SnapshotDocument, SnapshotError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| SnapshotError::ReadFailed {
                path: path.clone(),
                source,
            })?;
        serde_json::from_str(&content).map_err(|e| SnapshotError::Corrupted {
            path: path.clone(),
            reason: e.to_string(),
        })
    }
}

impl SnapshotPort for FileSnapshotAdapter {
    fn save(&self, snapshot: &SnapshotDocument) -> Result<(), SnapshotError> {
        std::fs::create_dir_all(&self.snapshots_dir).map_err(|source| {
            SnapshotError::SaveFailed {
                path: self.snapshots_dir.clone(),
                source,
            }
        })?;

        let legacy_path = self.legacy_snapshot_path(&snapshot.identity.workspace);
        let final_path = match Self::state_key_from_document(snapshot, &legacy_path) {
            Ok(state_key) => self.snapshot_path_for_state_key(&state_key),
            Err(SnapshotError::MissingStateKey { .. }) => legacy_path,
            Err(err) => return Err(err),
        };
        let content =
            serde_json::to_string_pretty(snapshot).map_err(|e| SnapshotError::Corrupted {
                path: final_path.clone(),
                reason: e.to_string(),
            })?;

        // Atomic write: create a unique temp file in the same directory, write
        // content, then persist over the final path.
        let mut temp = NamedTempFile::new_in(&self.snapshots_dir).map_err(|source| {
            SnapshotError::SaveFailed {
                path: self.snapshots_dir.clone(),
                source,
            }
        })?;
        use std::io::Write;
        temp.write_all(content.as_bytes())
            .and_then(|_| temp.flush())
            .map_err(|source| SnapshotError::SaveFailed {
                path: final_path.clone(),
                source,
            })?;

        temp.persist(final_path.as_std_path())
            .map_err(|e| SnapshotError::SaveFailed {
                path: final_path.clone(),
                source: e.error,
            })?;

        tracing::debug!(path = %final_path, "snapshot saved (atomic)");
        Ok(())
    }

    fn load_latest_by_state_key(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotDocument>, SnapshotError> {
        let path = self.snapshot_path_for_state_key(state_key);
        if path.exists() {
            return self.read_snapshot(&path).map(Some);
        }
        let legacy_path = self.legacy_snapshot_path(&state_key.workspace);
        if legacy_path.exists() {
            return self.read_snapshot(&legacy_path).map(Some);
        }
        let workspace =
            WorkspaceId::new(&state_key.workspace).map_err(|err| SnapshotError::Corrupted {
                path: self.snapshots_dir.clone(),
                reason: format!(
                    "invalid workspace in state key '{}': {err}",
                    state_key.workspace
                ),
            })?;
        self.load_latest(&workspace)
    }

    fn load_latest(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<Option<SnapshotDocument>, SnapshotError> {
        if !self.snapshots_dir.is_dir() {
            return Ok(None);
        }

        let mut latest: Option<(std::time::SystemTime, SnapshotDocument)> = None;
        let entries =
            std::fs::read_dir(&self.snapshots_dir).map_err(|source| SnapshotError::ReadFailed {
                path: self.snapshots_dir.clone(),
                source,
            })?;

        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, dir = %self.snapshots_dir, "failed to read snapshot directory entry");
                    continue;
                }
            };
            let path = entry.path();
            let is_json = path.extension().is_some_and(|e| e == "json");
            let is_hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if !is_json || is_hidden {
                continue;
            }
            let Ok(utf8_path) = Utf8PathBuf::try_from(path.clone()) else {
                continue;
            };
            let doc = self.read_snapshot(&utf8_path)?;
            if doc.identity.workspace != workspace.as_str() {
                continue;
            }
            let modified = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            match &latest {
                Some((prev_modified, _)) if modified <= *prev_modified => {}
                _ => latest = Some((modified, doc)),
            }
        }

        Ok(latest.map(|(_, doc)| doc))
    }

    fn list(&self) -> Result<Vec<SnapshotMetadata>, SnapshotError> {
        if !self.snapshots_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut results = Vec::new();
        let entries =
            std::fs::read_dir(&self.snapshots_dir).map_err(|source| SnapshotError::ReadFailed {
                path: self.snapshots_dir.clone(),
                source,
            })?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, dir = %self.snapshots_dir, "failed to read snapshot directory entry");
                    continue;
                }
            };
            let path = entry.path();
            let is_json = path.extension().is_some_and(|e| e == "json");
            let is_hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if is_json && !is_hidden {
                let Ok(utf8_path) = Utf8PathBuf::try_from(path.clone()) else {
                    tracing::debug!(?path, "skipping non-UTF-8 snapshot path");
                    continue;
                };
                let doc = self.read_snapshot(&utf8_path)?;
                results.push(SnapshotMetadata {
                    workspace: doc.identity.workspace,
                    created_at: file_modified_iso(&path),
                    path: utf8_path,
                });
            }
        }
        Ok(results)
    }

    fn delete_for_workspace(&self, workspace: &WorkspaceId) -> Result<(), SnapshotError> {
        if !self.snapshots_dir.is_dir() {
            return Ok(());
        }

        let entries =
            std::fs::read_dir(&self.snapshots_dir).map_err(|source| SnapshotError::ReadFailed {
                path: self.snapshots_dir.clone(),
                source,
            })?;
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, dir = %self.snapshots_dir, "failed to read snapshot directory entry");
                    continue;
                }
            };
            let path = entry.path();
            let is_json = path.extension().is_some_and(|e| e == "json");
            if !is_json {
                continue;
            }
            let Ok(utf8_path) = Utf8PathBuf::try_from(path.clone()) else {
                continue;
            };
            let doc = self.read_snapshot(&utf8_path)?;
            if doc.identity.workspace == workspace.as_str() {
                std::fs::remove_file(&path).map_err(|source| SnapshotError::DeleteFailed {
                    path: utf8_path.clone(),
                    source,
                })?;
                tracing::debug!(path = %utf8_path, workspace = %workspace, "snapshot deleted by workspace fallback");
            }
        }
        Ok(())
    }

    fn delete_by_state_key(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let path = self.snapshot_path_for_state_key(state_key);
        let legacy_path = self.legacy_snapshot_path(&state_key.workspace);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|source| SnapshotError::DeleteFailed {
                path: path.clone(),
                source,
            })?;
            tracing::debug!(path = %path, state_key = %state_key, "snapshot deleted by state key");
        }
        if legacy_path.exists() {
            std::fs::remove_file(&legacy_path).map_err(|source| SnapshotError::DeleteFailed {
                path: legacy_path.clone(),
                source,
            })?;
            tracing::debug!(path = %legacy_path, state_key = %state_key, "legacy snapshot deleted by state key fallback");
        }
        Ok(())
    }

    fn delete_all(&self) -> Result<(), SnapshotError> {
        if self.snapshots_dir.is_dir() {
            std::fs::remove_dir_all(&self.snapshots_dir).map_err(|source| {
                SnapshotError::DeleteFailed {
                    path: self.snapshots_dir.clone(),
                    source,
                }
            })?;
        }
        Ok(())
    }
}

fn file_modified_iso(path: &std::path::Path) -> String {
    let modified = match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, ?path, "failed to read file modified time");
            return "unknown".to_owned();
        }
    };
    match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(dur) => format!("{}s", dur.as_secs()),
        Err(e) => {
            tracing::warn!(error = %e, ?path, "file modified time is before UNIX epoch");
            "unknown".to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workhive_core::domain::LaunchCapture;
    use workhive_core::ports::{
        LEGACY_SNAPSHOT_SCHEMA_VERSION as SNAPSHOT_SCHEMA_VERSION,
        LegacySnapshotIdentity as SnapshotIdentity, LegacySnapshotMeta as SnapshotMeta,
        LegacySnapshotUnit as SnapshotUnit,
    };

    fn setup() -> (TempDir, FileSnapshotAdapter) {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        std::fs::create_dir_all(root.join(".workhive/snapshots")).unwrap();
        let adapter = FileSnapshotAdapter::new(&root);
        (tmp, adapter)
    }

    fn sample_snapshot() -> SnapshotDocument {
        SnapshotDocument {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            identity: SnapshotIdentity {
                instance_id: "a230".to_owned(),
                project: "proj".to_owned(),
                workspace: "feat-a".to_owned(),
                server_name: "whsrv-a230-proj".to_owned(),
                session_name: "wh@a230@proj@feat-a".to_owned(),
                state_key: Some("a230@proj@feat-a".to_owned()),
            },
            meta: SnapshotMeta {
                runtime_backend: "tmux".to_owned(),
                platform: "macos".to_owned(),
                captured_at: "2026-04-20T11:30:00Z".to_owned(),
                generation: 0,
                config_fingerprint: None,
            },
            units: vec![SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "zsh".to_owned(),
                    args: Vec::new(),
                },
                state: Some("running".to_owned()),
                last_error: None,
                updated_at: None,
            }],
        }
    }

    #[test]
    fn save_and_load() {
        let (_tmp, adapter) = setup();
        let snap = sample_snapshot();
        adapter.save(&snap).unwrap();

        let state_key = WorkspaceStateKey::new(
            "a230".to_owned(),
            "proj".to_owned(),
            String::new(),
            "feat-a".to_owned(),
        );
        let loaded = adapter
            .load_latest_by_state_key(&state_key)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.identity.workspace, "feat-a");
        assert_eq!(loaded.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(loaded.units.len(), 1);
    }

    #[test]
    fn load_returns_none_when_missing() {
        let (_tmp, adapter) = setup();
        let state_key = WorkspaceStateKey::new(
            "a230".to_owned(),
            "proj".to_owned(),
            String::new(),
            "nonexistent".to_owned(),
        );
        assert!(
            adapter
                .load_latest_by_state_key(&state_key)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn list_snapshots() {
        let (_tmp, adapter) = setup();
        adapter.save(&sample_snapshot()).unwrap();
        let list = adapter.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].workspace, "feat-a");
    }

    #[test]
    fn delete_all_clears() {
        let (_tmp, adapter) = setup();
        adapter.save(&sample_snapshot()).unwrap();
        adapter.delete_all().unwrap();
        assert!(adapter.list().unwrap().is_empty());
    }

    #[test]
    fn atomic_save_writes_complete_final_file() {
        let (_tmp, adapter) = setup();
        let snap = sample_snapshot();
        adapter.save(&snap).unwrap();

        let final_path = adapter.snapshot_path_for_state_key(&WorkspaceStateKey::new(
            "a230".to_owned(),
            "proj".to_owned(),
            String::new(),
            "feat-a".to_owned(),
        ));
        let content = std::fs::read_to_string(&final_path).unwrap();
        let parsed: SnapshotDocument = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(parsed.identity.workspace, "feat-a");

        // No hidden temp files should linger after a successful save.
        let has_temp = std::fs::read_dir(&adapter.snapshots_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with('.'))
                    .unwrap_or(false)
            });
        assert!(!has_temp, "no hidden temp files should remain after save");
    }

    #[test]
    fn list_ignores_temp_files() {
        let (_tmp, adapter) = setup();
        adapter.save(&sample_snapshot()).unwrap();
        // Create leftover temp files (both old fixed-name and new unique-name styles)
        std::fs::write(adapter.snapshots_dir.join(".feat-a.tmp"), "{}").unwrap();
        std::fs::write(
            adapter.snapshots_dir.join(".feat-a.12345-9876543210.tmp"),
            "{}",
        )
        .unwrap();

        let list = adapter.list().unwrap();
        assert_eq!(list.len(), 1, "temp files must not appear in listing");
    }

    #[test]
    fn distinct_state_keys_do_not_collide_for_same_workspace_name() {
        let (_tmp, adapter) = setup();
        let mut snap_a = sample_snapshot();
        snap_a.identity.workspace = "feat-x".to_owned();
        snap_a.identity.state_key = Some("a230@proj@subdir-a@feat-x".to_owned());
        let mut snap_b = sample_snapshot();
        snap_b.identity.workspace = "feat-x".to_owned();
        snap_b.identity.state_key = Some("a230@proj@subdir-b@feat-x".to_owned());

        adapter.save(&snap_a).unwrap();
        adapter.save(&snap_b).unwrap();

        let key_a = WorkspaceStateKey::new(
            "a230".to_owned(),
            "proj".to_owned(),
            "subdir-a".to_owned(),
            "feat-x".to_owned(),
        );
        let key_b = WorkspaceStateKey::new(
            "a230".to_owned(),
            "proj".to_owned(),
            "subdir-b".to_owned(),
            "feat-x".to_owned(),
        );

        let loaded_a = adapter.load_latest_by_state_key(&key_a).unwrap().unwrap();
        let loaded_b = adapter.load_latest_by_state_key(&key_b).unwrap().unwrap();
        assert_eq!(
            loaded_a.identity.state_key.as_deref(),
            Some("a230@proj@subdir-a@feat-x")
        );
        assert_eq!(
            loaded_b.identity.state_key.as_deref(),
            Some("a230@proj@subdir-b@feat-x")
        );
    }
}

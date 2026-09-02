//! File-based workspace identity adapter.
//!
//! Storage layout:
//! ```text
//! .workhive/identity/project.json
//! .workhive/identity/workspaces/<workspace>.json
//! ```
//!
//! All writes are atomic: serialize to temp file, then rename.

use camino::Utf8PathBuf;
use tempfile::NamedTempFile;
use workhive_core::domain::{
    IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
    WorkspaceStateKey,
};
use workhive_core::ports::WorkspaceIdentityPort;

/// File-based identity metadata adapter rooted at a project directory.
pub struct FileIdentityAdapter {
    identity_dir: Utf8PathBuf,
}

impl FileIdentityAdapter {
    pub fn new(project_root: &camino::Utf8Path) -> Self {
        Self {
            identity_dir: project_root.join(".workhive").join("identity"),
        }
    }

    fn project_path(&self) -> Utf8PathBuf {
        self.identity_dir.join("project.json")
    }

    fn workspaces_dir(&self) -> Utf8PathBuf {
        self.identity_dir.join("workspaces")
    }

    fn workspace_path(&self, key: &WorkspaceIdentityKey) -> Utf8PathBuf {
        self.workspaces_dir()
            .join(format!("{}.json", key.to_file_stem()))
    }

    fn ensure_identity_dir(&self) -> Result<(), IdentityError> {
        std::fs::create_dir_all(&self.identity_dir).map_err(|source| {
            IdentityError::DirCreateFailed {
                path: self.identity_dir.clone(),
                source,
            }
        })
    }

    fn ensure_workspaces_dir(&self) -> Result<(), IdentityError> {
        let dir = self.workspaces_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|source| IdentityError::DirCreateFailed { path: dir, source })
    }
}

fn atomic_write_json<T: serde::Serialize>(
    final_path: &Utf8PathBuf,
    parent_dir: &Utf8PathBuf,
    value: &T,
) -> Result<(), IdentityError> {
    let content = serde_json::to_string_pretty(value).map_err(|e| IdentityError::Corrupted {
        path: final_path.clone(),
        reason: format!("serialization failed: {e}"),
    })?;

    let mut temp = NamedTempFile::new_in(parent_dir.as_std_path()).map_err(|source| {
        IdentityError::WriteFailed {
            path: final_path.clone(),
            source,
        }
    })?;

    use std::io::Write;
    temp.write_all(content.as_bytes())
        .and_then(|_| temp.flush())
        .map_err(|source| IdentityError::WriteFailed {
            path: final_path.clone(),
            source,
        })?;

    temp.persist(final_path.as_std_path())
        .map_err(|e| IdentityError::WriteFailed {
            path: final_path.clone(),
            source: e.error,
        })?;

    Ok(())
}

impl WorkspaceIdentityPort for FileIdentityAdapter {
    fn load_project_identity(&self) -> Result<Option<ProjectIdentityRecord>, IdentityError> {
        let path = self.project_path();
        if !path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&path).map_err(|source| IdentityError::ReadFailed {
                path: path.clone(),
                source,
            })?;
        let record: ProjectIdentityRecord =
            serde_json::from_str(&content).map_err(|e| IdentityError::Corrupted {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        Ok(Some(record))
    }

    fn save_project_identity(&self, record: &ProjectIdentityRecord) -> Result<(), IdentityError> {
        self.ensure_identity_dir()?;
        let path = self.project_path();
        atomic_write_json(&path, &self.identity_dir, record)?;
        tracing::debug!(path = %path, "project identity saved");
        Ok(())
    }

    fn load_workspace_identity(
        &self,
        key: &WorkspaceIdentityKey,
    ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError> {
        let path = self.workspace_path(key);
        if !path.exists() {
            return Ok(None);
        }
        let content =
            std::fs::read_to_string(&path).map_err(|source| IdentityError::ReadFailed {
                path: path.clone(),
                source,
            })?;
        let record: WorkspaceIdentityRecord =
            serde_json::from_str(&content).map_err(|e| IdentityError::Corrupted {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        Ok(Some(record))
    }

    fn save_workspace_identity(
        &self,
        record: &WorkspaceIdentityRecord,
    ) -> Result<(), IdentityError> {
        self.ensure_workspaces_dir()?;
        let ws_dir = self.workspaces_dir();
        let key = record.identity_key();
        let path = self.workspace_path(&key);
        atomic_write_json(&path, &ws_dir, record)?;
        tracing::debug!(path = %path, workspace = %record.workspace, "workspace identity saved");
        Ok(())
    }

    fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
        let ws_dir = self.workspaces_dir();
        if !ws_dir.is_dir() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&ws_dir).map_err(|source| IdentityError::ReadFailed {
            path: ws_dir.clone(),
            source,
        })?;

        let mut records = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, dir = %ws_dir, "failed to read identity directory entry");
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
            let Ok(utf8_path) = Utf8PathBuf::try_from(path) else {
                continue;
            };
            let content = std::fs::read_to_string(&utf8_path).map_err(|source| {
                IdentityError::ReadFailed {
                    path: utf8_path.clone(),
                    source,
                }
            })?;
            let record: WorkspaceIdentityRecord =
                serde_json::from_str(&content).map_err(|e| IdentityError::Corrupted {
                    path: utf8_path,
                    reason: e.to_string(),
                })?;
            records.push(record);
        }

        Ok(records)
    }

    fn derive_state_key(
        &self,
        key: &WorkspaceIdentityKey,
    ) -> Result<WorkspaceStateKey, IdentityError> {
        let project =
            self.load_project_identity()?
                .ok_or_else(|| IdentityError::ProjectIdentityMissing {
                    path: self.project_path(),
                })?;

        let ws = self.load_workspace_identity(key)?.ok_or_else(|| {
            IdentityError::WorkspaceIdentityMissing {
                workspace: key.to_string(),
                path: self.workspace_path(key),
            }
        })?;

        Ok(WorkspaceStateKey::from_identity(&project, &ws))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, FileIdentityAdapter) {
        let tmp = TempDir::new().expect("create temp dir");
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("utf8 path");
        let adapter = FileIdentityAdapter::new(&root);
        (tmp, adapter)
    }

    fn sample_project() -> ProjectIdentityRecord {
        ProjectIdentityRecord {
            instance_id: "a1b2".to_owned(),
            project_root: Utf8PathBuf::from("/Users/dev/my-project"),
            project_display: "my-project".to_owned(),
            canonical_project_component: "my-project".to_owned(),
        }
    }

    fn sample_workspace(name: &str) -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: name.to_owned(),
            subdir_key: String::new(),
            relative_display_path: if name == "main" {
                None
            } else {
                Some(format!("subdir/{name}"))
            },
            canonical_workspace_component: name.to_owned(),
            canonical_session_name: format!("wh@a1b2@my-project@{name}"),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    fn sample_workspace_in_subdir(name: &str, subdir: &str) -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: name.to_owned(),
            subdir_key: subdir.to_owned(),
            relative_display_path: Some(format!("{subdir}/{name}")),
            canonical_workspace_component: name.to_owned(),
            canonical_session_name: format!("wh@a1b2@my-project@{name}"),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    #[test]
    fn save_and_load_project_identity() {
        let (_tmp, adapter) = setup();
        let record = sample_project();

        assert!(adapter.load_project_identity().unwrap().is_none());
        adapter.save_project_identity(&record).unwrap();
        let loaded = adapter.load_project_identity().unwrap().unwrap();
        assert_eq!(loaded, record);
    }

    #[test]
    fn save_and_load_workspace_identity() {
        let (_tmp, adapter) = setup();
        let record = sample_workspace("main");
        let key = record.identity_key();

        assert!(adapter.load_workspace_identity(&key).unwrap().is_none());
        adapter.save_workspace_identity(&record).unwrap();
        let loaded = adapter.load_workspace_identity(&key).unwrap().unwrap();
        assert_eq!(loaded, record);
    }

    #[test]
    fn save_workspace_identity_is_idempotent() {
        let (_tmp, adapter) = setup();
        let record = sample_workspace("feat-x");
        let key = record.identity_key();

        adapter.save_workspace_identity(&record).unwrap();
        adapter.save_workspace_identity(&record).unwrap();

        let loaded = adapter.load_workspace_identity(&key).unwrap().unwrap();
        assert_eq!(loaded, record);
    }

    #[test]
    fn list_workspace_identities_returns_all() {
        let (_tmp, adapter) = setup();
        adapter
            .save_workspace_identity(&sample_workspace("main"))
            .unwrap();
        adapter
            .save_workspace_identity(&sample_workspace("feat-a"))
            .unwrap();

        let list = adapter.list_workspace_identities().unwrap();
        assert_eq!(list.len(), 2);
        let names: Vec<_> = list.iter().map(|r| r.workspace.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"feat-a"));
    }

    #[test]
    fn same_workspace_different_subdirs_no_collision() {
        let (_tmp, adapter) = setup();
        let ws_a = sample_workspace_in_subdir("feat-x", "subdir-a");
        let ws_b = sample_workspace_in_subdir("feat-x", "subdir-b");

        adapter.save_workspace_identity(&ws_a).unwrap();
        adapter.save_workspace_identity(&ws_b).unwrap();

        let key_a = ws_a.identity_key();
        let key_b = ws_b.identity_key();

        let loaded_a = adapter.load_workspace_identity(&key_a).unwrap().unwrap();
        let loaded_b = adapter.load_workspace_identity(&key_b).unwrap().unwrap();
        assert_eq!(loaded_a.subdir_key, "subdir-a");
        assert_eq!(loaded_b.subdir_key, "subdir-b");

        let list = adapter.list_workspace_identities().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn list_empty_returns_empty() {
        let (_tmp, adapter) = setup();
        assert!(adapter.list_workspace_identities().unwrap().is_empty());
    }

    #[test]
    fn derive_state_key_succeeds_with_both_records() {
        let (_tmp, adapter) = setup();
        adapter.save_project_identity(&sample_project()).unwrap();
        adapter
            .save_workspace_identity(&sample_workspace("main"))
            .unwrap();

        let ws_key = WorkspaceIdentityKey::new("main", "");
        let key = adapter.derive_state_key(&ws_key).unwrap();
        assert_eq!(key.instance_id, "a1b2");
        assert_eq!(key.project_component, "my-project");
        assert_eq!(key.workspace, "main");
        assert_eq!(key.subdir_key, "");
    }

    #[test]
    fn derive_state_key_fails_without_project() {
        let (_tmp, adapter) = setup();
        adapter
            .save_workspace_identity(&sample_workspace("main"))
            .unwrap();

        let ws_key = WorkspaceIdentityKey::new("main", "");
        let err = adapter.derive_state_key(&ws_key).unwrap_err();
        assert!(matches!(err, IdentityError::ProjectIdentityMissing { .. }));
    }

    #[test]
    fn derive_state_key_fails_without_workspace() {
        let (_tmp, adapter) = setup();
        adapter.save_project_identity(&sample_project()).unwrap();

        let ws_key = WorkspaceIdentityKey::new("nonexistent", "");
        let err = adapter.derive_state_key(&ws_key).unwrap_err();
        assert!(matches!(
            err,
            IdentityError::WorkspaceIdentityMissing { .. }
        ));
    }

    #[test]
    fn derive_state_key_differs_for_same_workspace_different_subdirs() {
        let (_tmp, adapter) = setup();
        adapter.save_project_identity(&sample_project()).unwrap();
        adapter
            .save_workspace_identity(&sample_workspace_in_subdir("feat-x", "subdir-a"))
            .unwrap();
        adapter
            .save_workspace_identity(&sample_workspace_in_subdir("feat-x", "subdir-b"))
            .unwrap();

        let key_a = WorkspaceIdentityKey::new("feat-x", "subdir-a");
        let key_b = WorkspaceIdentityKey::new("feat-x", "subdir-b");

        let sk_a = adapter.derive_state_key(&key_a).unwrap();
        let sk_b = adapter.derive_state_key(&key_b).unwrap();
        assert_ne!(sk_a, sk_b);
        assert_eq!(sk_a.subdir_key, "subdir-a");
        assert_eq!(sk_b.subdir_key, "subdir-b");
    }

    #[test]
    fn corrupted_project_identity_returns_error() {
        let (_tmp, adapter) = setup();
        adapter.ensure_identity_dir().unwrap();
        std::fs::write(adapter.project_path().as_std_path(), "not json").unwrap();

        let err = adapter.load_project_identity().unwrap_err();
        assert!(matches!(err, IdentityError::Corrupted { .. }));
    }
}

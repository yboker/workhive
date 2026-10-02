//! File-based workspace identity adapter.
//!
//! Storage layout:
//! ```text
//! .workhive/identity/project.json
//! .workhive/identity/workspaces/<canonical-identity-key>.json
//! ```
//!
//! All writes are atomic: serialize to temp file, then rename.

use camino::{Utf8Path, Utf8PathBuf};
use tempfile::NamedTempFile;
use workhive_core::domain::{
    IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
};
use workhive_core::ports::WorkspaceIdentityPort;

fn load_workspace_identity_paths(
    ws_dir: &Utf8Path,
    paths: impl IntoIterator<Item = Result<std::path::PathBuf, std::io::Error>>,
) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
    let mut records = Vec::new();
    for path in paths {
        let path = path.map_err(|source| IdentityError::ReadFailed {
            path: ws_dir.to_path_buf(),
            source,
        })?;
        let is_json = path
            .extension()
            .is_some_and(|extension| extension == "json");
        let is_hidden = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if !is_json || is_hidden {
            continue;
        }

        let utf8_path =
            Utf8PathBuf::try_from(path.clone()).map_err(|_| IdentityError::ReadFailed {
                path: Utf8PathBuf::from(path.to_string_lossy().as_ref()),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "identity candidate path is not valid UTF-8",
                ),
            })?;
        let content =
            std::fs::read_to_string(&utf8_path).map_err(|source| IdentityError::ReadFailed {
                path: utf8_path.clone(),
                source,
            })?;
        let record = serde_json::from_str(&content).map_err(|error| IdentityError::Corrupted {
            path: utf8_path,
            reason: error.to_string(),
        })?;
        records.push(record);
    }
    Ok(records)
}

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

    fn replace_workspace_identities(
        &self,
        records: &[WorkspaceIdentityRecord],
    ) -> Result<(), IdentityError> {
        self.ensure_identity_dir()?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let suffix = format!("{}-{nonce:x}", std::process::id());
        let staging = self
            .identity_dir
            .join(format!(".workspaces-staging-{suffix}"));
        let backup = self
            .identity_dir
            .join(format!(".workspaces-backup-{suffix}"));
        std::fs::create_dir(&staging).map_err(|source| IdentityError::DirCreateFailed {
            path: staging.clone(),
            source,
        })?;

        let mut keys = std::collections::HashSet::with_capacity(records.len());
        for record in records {
            let key = record.identity_key();
            if !keys.insert(key.clone()) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(IdentityError::Corrupted {
                    path: staging,
                    reason: format!("duplicate workspace identity key '{}'", key.to_file_stem()),
                });
            }
            let path = staging.join(format!("{}.json", key.to_file_stem()));
            if let Err(error) = atomic_write_json(&path, &staging, record) {
                let _ = std::fs::remove_dir_all(&staging);
                return Err(error);
            }
        }

        let current = self.workspaces_dir();
        if current.exists() {
            std::fs::rename(&current, &backup).map_err(|source| IdentityError::WriteFailed {
                path: current.clone(),
                source,
            })?;
        }
        if let Err(source) = std::fs::rename(&staging, &current) {
            if backup.exists() {
                let _ = std::fs::rename(&backup, &current);
            }
            let _ = std::fs::remove_dir_all(&staging);
            return Err(IdentityError::WriteFailed {
                path: current,
                source,
            });
        }
        if backup.exists()
            && let Err(error) = std::fs::remove_dir_all(&backup)
        {
            tracing::warn!(path = %backup, error = %error, "stale workspace identity backup requires cleanup");
        }
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

        load_workspace_identity_paths(
            &ws_dir,
            entries.map(|entry| entry.map(|entry| entry.path())),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workhive_core::domain::{RelativeProjectPath, RuntimeBinding};

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
            relative_project_path: None,
            relative_display_path: if name == "main" {
                None
            } else {
                Some(format!("subdir/{name}"))
            },
            canonical_workspace_component: name.to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                format!("wh@a1b2@my-project@{name}"),
            ),
            runtime_replacement: None,
            canonical_session_name: format!("wh@a1b2@my-project@{name}"),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    fn sample_workspace_in_subdir(name: &str, subdir: &str) -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: name.to_owned(),
            relative_project_path: Some(RelativeProjectPath::new(subdir).unwrap()),
            relative_display_path: Some(format!("{subdir}/{name}")),
            canonical_workspace_component: name.to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                format!("wh@a1b2@my-project@{name}"),
            ),
            runtime_replacement: None,
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
    fn replace_workspace_identities_swaps_the_complete_set() {
        let (_tmp, adapter) = setup();
        adapter.save_project_identity(&sample_project()).unwrap();
        adapter
            .save_workspace_identity(&sample_workspace("stale"))
            .unwrap();
        let replacement = [sample_workspace("main"), sample_workspace("feat-a")];

        adapter.replace_workspace_identities(&replacement).unwrap();

        let mut names = adapter
            .list_workspace_identities()
            .unwrap()
            .into_iter()
            .map(|record| record.workspace)
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["feat-a", "main"]);
        assert_eq!(
            adapter.load_project_identity().unwrap(),
            Some(sample_project())
        );
    }

    #[test]
    fn duplicate_batch_key_preserves_previous_complete_set() {
        let (_tmp, adapter) = setup();
        let previous = sample_workspace("main");
        adapter.save_workspace_identity(&previous).unwrap();

        assert!(
            adapter
                .replace_workspace_identities(&[
                    sample_workspace("duplicate"),
                    sample_workspace("duplicate"),
                ])
                .is_err()
        );
        assert_eq!(adapter.list_workspace_identities().unwrap(), [previous]);
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
        assert_eq!(
            loaded_a.relative_project_path.as_ref().unwrap().as_str(),
            "subdir-a"
        );
        assert_eq!(
            loaded_b.relative_project_path.as_ref().unwrap().as_str(),
            "subdir-b"
        );

        let list = adapter.list_workspace_identities().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn list_empty_returns_empty() {
        let (_tmp, adapter) = setup();
        assert!(adapter.list_workspace_identities().unwrap().is_empty());
    }

    // ── workspace_identity_list_entry_error_ — fail-closed enumeration ──

    #[test]
    fn workspace_identity_list_entry_error_is_propagated() {
        let (_tmp, adapter) = setup();
        adapter
            .save_workspace_identity(&sample_workspace("main"))
            .unwrap();

        let ws_dir = adapter.workspaces_dir();
        std::fs::write(ws_dir.join("corrupt.json").as_std_path(), "not valid json").unwrap();

        let result = adapter.list_workspace_identities();
        assert!(
            result.is_err(),
            "corrupted entry must cause list to fail, not return partial results"
        );
        match result.unwrap_err() {
            IdentityError::Corrupted { path, .. } => {
                assert!(
                    path.as_str().contains("corrupt.json"),
                    "error must reference the corrupted file"
                );
            }
            other => panic!("expected Corrupted error, got: {other:?}"),
        }
    }

    #[test]
    fn corrupted_project_identity_returns_error() {
        let (_tmp, adapter) = setup();
        adapter.ensure_identity_dir().unwrap();
        std::fs::write(adapter.project_path().as_std_path(), "not json").unwrap();

        let err = adapter.load_project_identity().unwrap_err();
        assert!(matches!(err, IdentityError::Corrupted { .. }));
    }

    #[test]
    fn workspace_identity_slash_save_load_roundtrip() {
        let (_tmp, adapter) = setup();
        let record = WorkspaceIdentityRecord {
            workspace: "chore/ignore-resources-and-commit".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "chore-ignore-resources-and-commit".to_owned(),
            canonical_session_name: "wh@a1b2@my-project@chore-ignore-resources-and-commit"
                .to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                "wh@a1b2@my-project@chore-ignore-resources-and-commit",
            ),
            runtime_replacement: None,
        };
        adapter.save_workspace_identity(&record).unwrap();
        let key = record.identity_key();
        let loaded = adapter
            .load_workspace_identity(&key)
            .unwrap()
            .expect("should exist");
        assert_eq!(loaded, record);
    }

    #[test]
    fn workspace_identity_slash_appears_in_list() {
        let (_tmp, adapter) = setup();
        let record = WorkspaceIdentityRecord {
            workspace: "chore/ignore-resources-and-commit".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "chore-ignore-resources-and-commit".to_owned(),
            canonical_session_name: "wh@a1b2@my-project@chore-ignore-resources-and-commit"
                .to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                "wh@a1b2@my-project@chore-ignore-resources-and-commit",
            ),
            runtime_replacement: None,
        };
        adapter.save_workspace_identity(&record).unwrap();
        let list = adapter.list_workspace_identities().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].workspace, "chore/ignore-resources-and-commit");
    }

    // ── workspace_identity_enumeration_ ──

    #[test]
    fn workspace_identity_enumeration_entry_error_is_propagated() {
        let (_tmp, adapter) = setup();
        let record = sample_workspace("main");
        adapter.save_workspace_identity(&record).unwrap();

        let ws_dir = adapter.workspaces_dir();
        let valid_path = adapter
            .workspace_path(&record.identity_key())
            .into_std_path_buf();
        let result = load_workspace_identity_paths(
            &ws_dir,
            [
                Ok(valid_path),
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "directory iteration denied",
                )),
            ],
        );

        match result {
            Err(IdentityError::ReadFailed { source, .. }) => {
                assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
                assert_eq!(source.to_string(), "directory iteration denied");
            }
            other => panic!("expected original directory-entry error, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn workspace_identity_enumeration_non_utf8_entry_is_rejected() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let (_tmp, adapter) = setup();
        let non_utf8_name = OsStr::from_bytes(b"\xff\xfe.json");
        let non_utf8_path = std::path::Path::new("/identity").join(non_utf8_name);
        let result = load_workspace_identity_paths(&adapter.workspaces_dir(), [Ok(non_utf8_path)]);

        assert!(matches!(
            result,
            Err(IdentityError::ReadFailed { source, .. })
                if source.kind() == std::io::ErrorKind::InvalidData
        ));
    }
}

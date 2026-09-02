//! Filesystem adapter for `.workhive/` directory structure.

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use workhive_core::errors::WorkspaceFsError;
use workhive_core::ports::WorkspaceFsPort;

const GITIGNORE_TEMPLATE: &str = include_str!("../templates/gitignore");

const WORKHIVE_DIR: &str = ".workhive";
const INSTANCE_ID_FILE: &str = "instance_id";
const WORKSPACE_USAGE_FILE: &str = "workspace_usage.json";

#[derive(Debug, Default, Deserialize, Serialize)]
struct WorkspaceUsageDocument {
    workspaces: BTreeMap<String, WorkspaceUsageEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct WorkspaceUsageEntry {
    last_used_at: u64,
}

fn read_workspace_usage_document(
    path: &Utf8Path,
) -> Result<WorkspaceUsageDocument, WorkspaceFsError> {
    if !path.as_std_path().exists() {
        return Ok(WorkspaceUsageDocument::default());
    }

    let text = std::fs::read_to_string(path.as_std_path()).map_err(|source| {
        WorkspaceFsError::ReadFailed {
            path: path.to_owned(),
            source,
        }
    })?;
    serde_json::from_str::<WorkspaceUsageDocument>(&text).map_err(|e| {
        WorkspaceFsError::InvalidMetadata {
            path: path.to_owned(),
            reason: e.to_string(),
        }
    })
}

// Eagerly-created subdirectories of `.workhive/`.
//
// Snapshot storage (`.workhive/snapshots/`) is intentionally absent.
// the snapshot tree is created lazily by the snapshot store on the first
// real write. This keeps the on-disk footprint of a project consistent
// with `snapshot.enabled = false` and avoids producing empty
// `snapshots/` directories that suggest snapshot runtime is active when
// it is not.
const SUBDIRS: &[&str] = &["worktrees", "runtime"];

/// Real filesystem adapter for `.workhive/` operations.
pub struct FsWorkspaceAdapter;

impl FsWorkspaceAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FsWorkspaceAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceFsPort for FsWorkspaceAdapter {
    fn project_root_exists(&self, project_root: &Utf8Path) -> bool {
        project_root.exists()
    }

    fn create_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
        let base = project_root.join(WORKHIVE_DIR);
        tracing::debug!(path = %base, "creating .workhive directory structure");

        std::fs::create_dir_all(&base).map_err(|source| WorkspaceFsError::CreateDirFailed {
            path: base.clone(),
            source,
        })?;

        for sub in SUBDIRS {
            let sub_path = base.join(sub);
            std::fs::create_dir_all(&sub_path).map_err(|source| {
                WorkspaceFsError::CreateDirFailed {
                    path: sub_path,
                    source,
                }
            })?;
        }

        Ok(())
    }

    fn remove_workhive_dir(&self, project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
        let base = project_root.join(WORKHIVE_DIR);
        tracing::debug!(path = %base, "removing .workhive directory");

        if base.exists() {
            std::fs::remove_dir_all(&base)
                .map_err(|source| WorkspaceFsError::RemoveDirFailed { path: base, source })?;
        }
        Ok(())
    }

    fn workhive_dir_exists(&self, project_root: &Utf8Path) -> bool {
        project_root.join(WORKHIVE_DIR).is_dir()
    }

    fn workhive_dir_path(&self, project_root: &Utf8Path) -> Utf8PathBuf {
        project_root.join(WORKHIVE_DIR)
    }

    fn list_workspace_dirs(
        &self,
        project_root: &Utf8Path,
    ) -> Result<Vec<String>, WorkspaceFsError> {
        let wt_dir = project_root.join(WORKHIVE_DIR).join("worktrees");
        if !wt_dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut names = Vec::new();
        let entries =
            std::fs::read_dir(&wt_dir).map_err(|source| WorkspaceFsError::CreateDirFailed {
                path: wt_dir.clone(),
                source,
            })?;

        for entry in entries {
            let entry = entry.map_err(|source| WorkspaceFsError::CreateDirFailed {
                path: wt_dir.clone(),
                source,
            })?;
            let file_type =
                entry
                    .file_type()
                    .map_err(|source| WorkspaceFsError::ReadDirEntryFailed {
                        path: wt_dir.clone(),
                        source,
                    })?;
            if file_type.is_dir()
                && let Some(name) = entry.file_name().to_str()
            {
                names.push(name.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    fn read_instance_id(&self, project_root: &Utf8Path) -> Option<String> {
        let path = project_root.join(WORKHIVE_DIR).join(INSTANCE_ID_FILE);
        std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    fn write_instance_id(&self, project_root: &Utf8Path, id: &str) -> Result<(), WorkspaceFsError> {
        let dir = project_root.join(WORKHIVE_DIR);
        std::fs::create_dir_all(&dir).map_err(|source| WorkspaceFsError::CreateDirFailed {
            path: dir.clone(),
            source,
        })?;
        let path = dir.join(INSTANCE_ID_FILE);
        std::fs::write(&path, id)
            .map_err(|source| WorkspaceFsError::CreateDirFailed { path, source })?;
        tracing::debug!(instance_id = id, "wrote instance_id");
        Ok(())
    }

    fn project_config_exists(&self, project_root: &Utf8Path) -> bool {
        project_root
            .join(WORKHIVE_DIR)
            .join("config.toml")
            .is_file()
    }

    fn worktree_dir_exists(&self, project_root: &Utf8Path, worktree_name: &str) -> bool {
        project_root
            .join(WORKHIVE_DIR)
            .join("worktrees")
            .join(worktree_name)
            .is_dir()
    }

    fn workspace_last_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
    ) -> Option<u64> {
        let path = project_root.join(WORKHIVE_DIR).join(WORKSPACE_USAGE_FILE);
        let doc = match read_workspace_usage_document(&path) {
            Ok(doc) => doc,
            Err(e) => {
                tracing::warn!(
                    path = %path,
                    error = %e,
                    "failed to read workspace usage metadata"
                );
                return None;
            }
        };
        doc.workspaces.get(workspace_name).map(|e| e.last_used_at)
    }

    fn mark_workspace_used_epoch_secs(
        &self,
        project_root: &Utf8Path,
        workspace_name: &str,
        epoch_secs: u64,
    ) -> Result<(), WorkspaceFsError> {
        let base = project_root.join(WORKHIVE_DIR);
        std::fs::create_dir_all(base.as_std_path()).map_err(|source| {
            WorkspaceFsError::CreateDirFailed {
                path: base.clone(),
                source,
            }
        })?;
        let path = base.join(WORKSPACE_USAGE_FILE);
        let mut doc = read_workspace_usage_document(&path)?;
        doc.workspaces.insert(
            workspace_name.to_owned(),
            WorkspaceUsageEntry {
                last_used_at: epoch_secs,
            },
        );
        let text =
            serde_json::to_string_pretty(&doc).map_err(|e| WorkspaceFsError::InvalidMetadata {
                path: path.clone(),
                reason: e.to_string(),
            })?;
        std::fs::write(path.as_std_path(), text)
            .map_err(|source| WorkspaceFsError::WriteFailed { path, source })
    }

    fn commit_staged_workhive(
        &self,
        staging_root: &Utf8Path,
        project_root: &Utf8Path,
    ) -> Result<(), WorkspaceFsError> {
        let staging_workhive = staging_root.join(WORKHIVE_DIR);
        let final_workhive = project_root.join(WORKHIVE_DIR);
        std::fs::rename(staging_workhive.as_std_path(), final_workhive.as_std_path()).map_err(
            |e| WorkspaceFsError::CreateDirFailed {
                path: final_workhive,
                source: e,
            },
        )?;
        Ok(())
    }

    fn cleanup_staging(&self, staging_root: &Utf8Path) {
        if staging_root.as_std_path().exists()
            && let Err(e) = std::fs::remove_dir_all(staging_root.as_std_path())
        {
            tracing::warn!(
                staging = %staging_root,
                error = %e,
                "failed to clean up staging directory"
            );
        }
    }

    fn ensure_gitignore_workhive_entry(
        &self,
        project_root: &Utf8Path,
    ) -> Result<(), WorkspaceFsError> {
        let gitignore_path = project_root.join(".gitignore");
        let entry = "/.workhive/";

        if gitignore_path.as_std_path().exists() {
            let content = std::fs::read_to_string(gitignore_path.as_std_path()).map_err(|e| {
                WorkspaceFsError::ReadFailed {
                    path: gitignore_path.clone(),
                    source: e,
                }
            })?;

            let dominated = content.lines().any(|line| {
                let trimmed = line.trim();
                trimmed == "/.workhive/"
                    || trimmed == "/.workhive"
                    || trimmed == ".workhive/"
                    || trimmed == ".workhive"
            });

            if dominated {
                tracing::debug!("gitignore already contains workhive entry, skipping");
                return Ok(());
            }

            let separator = if content.ends_with('\n') || content.is_empty() {
                ""
            } else {
                "\n"
            };
            let appended = format!("{content}{separator}\n# workhive runtime directory\n{entry}\n");
            std::fs::write(gitignore_path.as_std_path(), appended).map_err(|e| {
                WorkspaceFsError::CreateDirFailed {
                    path: gitignore_path,
                    source: e,
                }
            })?;
        } else {
            let template = ensure_template_has_workhive_entry(GITIGNORE_TEMPLATE, entry);
            std::fs::write(gitignore_path.as_std_path(), template).map_err(|e| {
                WorkspaceFsError::CreateDirFailed {
                    path: gitignore_path,
                    source: e,
                }
            })?;
        }

        Ok(())
    }
}

fn ensure_template_has_workhive_entry(template: &str, entry: &str) -> String {
    if template.lines().any(|line| line.trim() == entry) {
        template.to_owned()
    } else {
        let separator = if template.ends_with('\n') || template.is_empty() {
            ""
        } else {
            "\n"
        };
        format!("{template}{separator}{entry}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_and_verify_structure() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.create_workhive_dir(&root).unwrap();

        assert!(fs.workhive_dir_exists(&root));
        assert!(root.join(".workhive/worktrees").is_dir());
        assert!(root.join(".workhive/runtime").is_dir());
        // Snapshot directory is intentionally NOT created eagerly.
        // It is created lazily by the snapshot store on first write.
        assert!(
            !root.join(".workhive/snapshots").exists(),
            ".workhive/snapshots/ must not be created during init; lazy creation only"
        );
    }

    #[test]
    fn create_workhive_dir_does_not_create_snapshots_dir() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.create_workhive_dir(&root).unwrap();

        assert!(
            !root.join(".workhive/snapshots").exists(),
            "snapshot.enabled regardless, init must not eagerly create snapshots/"
        );
    }

    #[test]
    fn remove_cleans_up() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.create_workhive_dir(&root).unwrap();
        fs.remove_workhive_dir(&root).unwrap();

        assert!(!fs.workhive_dir_exists(&root));
    }

    #[test]
    fn list_workspace_dirs_empty() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.create_workhive_dir(&root).unwrap();
        let dirs = fs.list_workspace_dirs(&root).unwrap();
        assert!(dirs.is_empty());
    }

    #[test]
    fn list_workspace_dirs_returns_sorted_names() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.create_workhive_dir(&root).unwrap();
        let wt_dir = root.join(".workhive/worktrees");
        std::fs::create_dir(wt_dir.join("beta")).unwrap();
        std::fs::create_dir(wt_dir.join("alpha")).unwrap();

        let dirs = fs.list_workspace_dirs(&root).unwrap();
        assert_eq!(dirs, vec!["alpha", "beta"]);
    }

    #[test]
    fn workspace_usage_round_trips_last_used_metadata() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.mark_workspace_used_epoch_secs(&root, "main", 1_714_800_000)
            .unwrap();
        fs.mark_workspace_used_epoch_secs(&root, "feature-a", 1_714_800_060)
            .unwrap();

        assert_eq!(
            fs.workspace_last_used_epoch_secs(&root, "main"),
            Some(1_714_800_000)
        );
        assert_eq!(
            fs.workspace_last_used_epoch_secs(&root, "feature-a"),
            Some(1_714_800_060)
        );
        assert_eq!(fs.workspace_last_used_epoch_secs(&root, "missing"), None);
    }

    #[test]
    fn workspace_usage_write_rejects_corrupt_metadata() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();
        let base = root.join(".workhive");
        std::fs::create_dir_all(base.as_std_path()).unwrap();
        let path = base.join("workspace_usage.json");
        std::fs::write(path.as_std_path(), "{not-json").unwrap();

        let err = fs
            .mark_workspace_used_epoch_secs(&root, "main", 1_714_800_000)
            .unwrap_err();
        assert!(
            matches!(err, WorkspaceFsError::InvalidMetadata { .. }),
            "corrupt usage metadata must not be overwritten silently: {err:?}"
        );
    }

    #[test]
    fn gitignore_creates_new_file_when_missing() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        fs.ensure_gitignore_workhive_entry(&root).unwrap();

        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(
            content.contains("/.workhive/"),
            "new gitignore must contain /.workhive/"
        );
    }

    #[test]
    fn gitignore_appends_entry_to_existing_file() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();

        fs.ensure_gitignore_workhive_entry(&root).unwrap();

        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(
            content.starts_with("target/"),
            "existing content must be preserved"
        );
        assert!(content.contains("/.workhive/"), "must append /.workhive/");
    }

    #[test]
    fn gitignore_idempotent_no_duplicate_entry() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        std::fs::write(root.join(".gitignore"), "target/\n/.workhive/\n").unwrap();

        fs.ensure_gitignore_workhive_entry(&root).unwrap();

        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        let count = content.matches("/.workhive/").count();
        assert_eq!(count, 1, "must not duplicate /.workhive/ entry");
    }

    #[test]
    fn gitignore_recognizes_equivalent_patterns() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        for existing in [".workhive/", ".workhive", "/.workhive"] {
            std::fs::write(root.join(".gitignore"), format!("{existing}\n")).unwrap();

            fs.ensure_gitignore_workhive_entry(&root).unwrap();

            let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
            assert!(
                !content.contains("# workhive runtime directory"),
                "should not append when equivalent pattern '{existing}' exists"
            );
        }
    }

    #[test]
    fn gitignore_preserves_existing_content_without_trailing_newline() {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let fs = FsWorkspaceAdapter::new();

        std::fs::write(root.join(".gitignore"), "node_modules/").unwrap();

        fs.ensure_gitignore_workhive_entry(&root).unwrap();

        let content = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(content.starts_with("node_modules/"));
        assert!(content.contains("/.workhive/"));
    }
}

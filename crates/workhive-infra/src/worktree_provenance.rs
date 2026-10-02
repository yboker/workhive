//! File-based managed-worktree branch provenance adapter.
//!
//! Storage layout:
//! ```text
//! .workhive/worktree_provenance.json
//! ```
//!
//! The store is a single JSON document keyed by managed-worktree
//! workspace name. Each entry records only whether Workhive created the
//! backing branch; an absent entry resolves to
//! [`BranchProvenance::Unknown`]. Writes are atomic (temp file + rename).
//!
//! Reads are deliberately conservative: a missing, unreadable, or
//! unparseable store resolves to `Unknown` so removal never deletes a
//! branch on an optimistic assumption.

use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use workhive_core::ports::{BranchProvenance, WorktreeProvenanceError, WorktreeProvenancePort};

/// Current on-disk schema version for the provenance store.
const PROVENANCE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct ProvenanceFile {
    version: u32,
    #[serde(default)]
    worktrees: BTreeMap<String, WorktreeProvenanceRecord>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct WorktreeProvenanceRecord {
    branch_created_by_workhive: bool,
}

/// File-based provenance store rooted at a project directory.
pub struct FileWorktreeProvenanceAdapter {
    workhive_dir: Utf8PathBuf,
    store_path: Utf8PathBuf,
}

impl FileWorktreeProvenanceAdapter {
    pub fn new(project_root: &Utf8Path) -> Self {
        let workhive_dir = project_root.join(".workhive");
        let store_path = workhive_dir.join("worktree_provenance.json");
        Self {
            workhive_dir,
            store_path,
        }
    }

    /// Load the store, resolving any missing/unreadable/unparseable file
    /// to an empty document so reads stay conservative and writes never
    /// fail just because the damaged existing store could not be parsed.
    fn load_resilient(&self) -> ProvenanceFile {
        if !self.store_path.exists() {
            return ProvenanceFile::default();
        }
        match std::fs::read_to_string(&self.store_path) {
            Ok(content) => match serde_json::from_str::<ProvenanceFile>(&content) {
                Ok(file) => file,
                Err(e) => {
                    tracing::warn!(
                        path = %self.store_path,
                        error = %e,
                        "worktree provenance store unparseable; treating as empty"
                    );
                    ProvenanceFile::default()
                }
            },
            Err(e) => {
                tracing::warn!(
                    path = %self.store_path,
                    error = %e,
                    "worktree provenance store unreadable; treating as empty"
                );
                ProvenanceFile::default()
            }
        }
    }

    fn persist(&self, file: &ProvenanceFile) -> Result<(), WorktreeProvenanceError> {
        std::fs::create_dir_all(&self.workhive_dir).map_err(|source| {
            WorktreeProvenanceError::Io {
                path: self.workhive_dir.clone(),
                source,
            }
        })?;

        let content = serde_json::to_string_pretty(file).map_err(|e| {
            WorktreeProvenanceError::Serialization {
                path: self.store_path.clone(),
                detail: e.to_string(),
            }
        })?;

        let mut temp =
            NamedTempFile::new_in(self.workhive_dir.as_std_path()).map_err(|source| {
                WorktreeProvenanceError::Io {
                    path: self.store_path.clone(),
                    source,
                }
            })?;
        use std::io::Write;
        temp.write_all(content.as_bytes())
            .and_then(|_| temp.flush())
            .map_err(|source| WorktreeProvenanceError::Io {
                path: self.store_path.clone(),
                source,
            })?;
        temp.persist(self.store_path.as_std_path())
            .map_err(|e| WorktreeProvenanceError::Io {
                path: self.store_path.clone(),
                source: e.error,
            })?;
        Ok(())
    }
}

impl WorktreeProvenancePort for FileWorktreeProvenanceAdapter {
    fn record(
        &self,
        workspace: &str,
        provenance: BranchProvenance,
    ) -> Result<(), WorktreeProvenanceError> {
        let mut file = self.load_resilient();
        file.version = PROVENANCE_SCHEMA_VERSION;
        match provenance {
            BranchProvenance::WorkhiveCreated => {
                file.worktrees.insert(
                    workspace.to_owned(),
                    WorktreeProvenanceRecord {
                        branch_created_by_workhive: true,
                    },
                );
            }
            BranchProvenance::PreExistingAttached => {
                file.worktrees.insert(
                    workspace.to_owned(),
                    WorktreeProvenanceRecord {
                        branch_created_by_workhive: false,
                    },
                );
            }
            // Recording `Unknown` clears any stored decision so the
            // worktree resolves back to the conservative default.
            BranchProvenance::Unknown => {
                file.worktrees.remove(workspace);
            }
        }
        self.persist(&file)
    }

    fn lookup(&self, workspace: &str) -> BranchProvenance {
        match self.load_resilient().worktrees.get(workspace) {
            Some(record) if record.branch_created_by_workhive => BranchProvenance::WorkhiveCreated,
            Some(_) => BranchProvenance::PreExistingAttached,
            None => BranchProvenance::Unknown,
        }
    }

    fn forget(&self, workspace: &str) -> Result<(), WorktreeProvenanceError> {
        if !self.store_path.exists() {
            return Ok(());
        }
        let mut file = self.load_resilient();
        if file.worktrees.remove(workspace).is_some() {
            file.version = PROVENANCE_SCHEMA_VERSION;
            self.persist(&file)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (TempDir, FileWorktreeProvenanceAdapter) {
        let tmp = TempDir::new().expect("create temp dir");
        let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("utf8 path");
        let adapter = FileWorktreeProvenanceAdapter::new(&root);
        (tmp, adapter)
    }

    #[test]
    fn missing_store_resolves_to_unknown() {
        let (_tmp, adapter) = setup();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::Unknown);
    }

    #[test]
    fn records_and_reads_workhive_created() {
        let (_tmp, adapter) = setup();
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::WorkhiveCreated);
    }

    #[test]
    fn records_and_reads_pre_existing_attached() {
        let (_tmp, adapter) = setup();
        adapter
            .record("demo01", BranchProvenance::PreExistingAttached)
            .unwrap();
        assert_eq!(
            adapter.lookup("demo01"),
            BranchProvenance::PreExistingAttached
        );
    }

    #[test]
    fn record_is_idempotent_and_scoped_per_worktree() {
        let (_tmp, adapter) = setup();
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        adapter
            .record("demo02", BranchProvenance::PreExistingAttached)
            .unwrap();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::WorkhiveCreated);
        assert_eq!(
            adapter.lookup("demo02"),
            BranchProvenance::PreExistingAttached
        );
    }

    #[test]
    fn forget_removes_only_the_named_worktree() {
        let (_tmp, adapter) = setup();
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        adapter
            .record("demo02", BranchProvenance::WorkhiveCreated)
            .unwrap();
        adapter.forget("demo01").unwrap();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::Unknown);
        assert_eq!(adapter.lookup("demo02"), BranchProvenance::WorkhiveCreated);
    }

    #[test]
    fn forget_absent_entry_is_ok() {
        let (_tmp, adapter) = setup();
        adapter.forget("never-recorded").unwrap();
        assert_eq!(adapter.lookup("never-recorded"), BranchProvenance::Unknown);
    }

    #[test]
    fn recording_unknown_clears_prior_decision() {
        let (_tmp, adapter) = setup();
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        adapter.record("demo01", BranchProvenance::Unknown).unwrap();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::Unknown);
    }

    #[test]
    fn unparseable_store_resolves_to_unknown() {
        let (_tmp, adapter) = setup();
        std::fs::create_dir_all(adapter.workhive_dir.as_std_path()).unwrap();
        std::fs::write(adapter.store_path.as_std_path(), "{ not valid json").unwrap();
        // A damaged store must read as Unknown (conservative), never error.
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::Unknown);
        // And a subsequent record must succeed by rewriting a clean file.
        adapter
            .record("demo01", BranchProvenance::WorkhiveCreated)
            .unwrap();
        assert_eq!(adapter.lookup("demo01"), BranchProvenance::WorkhiveCreated);
    }
}

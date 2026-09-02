//! Resolve project index entries against the live filesystem.
//!
//! Classifies each `ProjectSummary` as either a valid `DiscoveredProject`
//! (with live `instance_id`) or a `StaleProjectEntry` (with typed reason).
//!
//! This module returns structured data only — no rendering, no i18n,
//! no `eprintln!`. CLI is solely responsible for warning text.

use crate::ports::{DiscoveredProject, ProjectSummary, WorkspaceFsPort};

/// Typed, non-localized reason why a project index entry is stale.
///
/// CLI maps these to user-visible i18n messages. Core never formats
/// human-readable text from these variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    /// `.workhive/instance_id` is absent or unreadable at the indexed root.
    InstanceIdMissing,
    /// The project root directory no longer exists on the filesystem.
    ProjectRootGone,
}

/// A stale index entry that could not be resolved to a live project.
#[derive(Debug, Clone)]
pub struct StaleProjectEntry {
    pub summary: ProjectSummary,
    pub reason: StaleReason,
}

/// Result of resolving a project index against the live filesystem.
pub struct ProjectResolutionResult {
    /// Projects with valid `instance_id` — ready for downstream use.
    pub valid: Vec<DiscoveredProject>,
    /// Index entries that could not be resolved, with typed staleness reason.
    pub stale: Vec<StaleProjectEntry>,
}

/// Resolve project summaries into valid discovered projects and stale entries.
///
/// For each summary, checks whether the project root exists and whether
/// `instance_id` can be read. Returns structured facts only — no side effects.
pub fn resolve_discovered_projects(
    summaries: &[ProjectSummary],
    ws_fs: &dyn WorkspaceFsPort,
) -> ProjectResolutionResult {
    let mut valid = Vec::new();
    let mut stale = Vec::new();

    for summary in summaries {
        if !ws_fs.project_root_exists(summary.project_root.as_ref()) {
            stale.push(StaleProjectEntry {
                summary: summary.clone(),
                reason: StaleReason::ProjectRootGone,
            });
            continue;
        }

        match ws_fs.read_instance_id(summary.project_root.as_ref()) {
            Some(instance_id) => {
                valid.push(DiscoveredProject {
                    project_name: summary.project_name.clone(),
                    project_root: summary.project_root.clone(),
                    instance_id,
                    initialized: true,
                });
            }
            None => {
                stale.push(StaleProjectEntry {
                    summary: summary.clone(),
                    reason: StaleReason::InstanceIdMissing,
                });
            }
        }
    }

    ProjectResolutionResult { valid, stale }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::WorkspaceFsError;
    use camino::{Utf8Path, Utf8PathBuf};

    struct FakeWsFs {
        instance_ids: Vec<(Utf8PathBuf, String)>,
        existing_roots: Vec<Utf8PathBuf>,
    }

    impl FakeWsFs {
        fn new(ids: Vec<(&str, &str)>) -> Self {
            let existing_roots: Vec<Utf8PathBuf> =
                ids.iter().map(|(p, _)| Utf8PathBuf::from(*p)).collect();
            Self {
                instance_ids: ids
                    .into_iter()
                    .map(|(p, id)| (Utf8PathBuf::from(p), id.to_owned()))
                    .collect(),
                existing_roots,
            }
        }

        fn with_roots(mut self, roots: Vec<&str>) -> Self {
            for r in roots {
                let path = Utf8PathBuf::from(r);
                if !self.existing_roots.contains(&path) {
                    self.existing_roots.push(path);
                }
            }
            self
        }
    }

    impl WorkspaceFsPort for FakeWsFs {
        fn project_root_exists(&self, root: &Utf8Path) -> bool {
            self.existing_roots
                .iter()
                .any(|p| p.as_str() == root.as_str())
        }
        fn workhive_dir_exists(&self, _: &Utf8Path) -> bool {
            true
        }
        fn workhive_dir_path(&self, root: &Utf8Path) -> Utf8PathBuf {
            root.join(".workhive")
        }
        fn list_workspace_dirs(&self, _: &Utf8Path) -> Result<Vec<String>, WorkspaceFsError> {
            Ok(Vec::new())
        }
        fn create_workhive_dir(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
        fn read_instance_id(&self, root: &Utf8Path) -> Option<String> {
            self.instance_ids
                .iter()
                .find(|(p, _)| p.as_str() == root.as_str())
                .map(|(_, id)| id.clone())
        }
        fn write_instance_id(&self, _: &Utf8Path, _: &str) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
        fn remove_workhive_dir(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
        fn project_config_exists(&self, _: &Utf8Path) -> bool {
            true
        }
        fn worktree_dir_exists(&self, _: &Utf8Path, _: &str) -> bool {
            false
        }
        fn workspace_last_used_epoch_secs(&self, _: &Utf8Path, _: &str) -> Option<u64> {
            None
        }
        fn mark_workspace_used_epoch_secs(
            &self,
            _: &Utf8Path,
            _: &str,
            _: u64,
        ) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
        fn commit_staged_workhive(
            &self,
            _: &Utf8Path,
            _: &Utf8Path,
        ) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
        fn cleanup_staging(&self, _: &Utf8Path) {}
        fn ensure_gitignore_workhive_entry(&self, _: &Utf8Path) -> Result<(), WorkspaceFsError> {
            Ok(())
        }
    }

    fn make_summary(name: &str, root: &str) -> ProjectSummary {
        ProjectSummary {
            project_name: name.to_owned(),
            project_root: Utf8PathBuf::from(root),
            config_path: Utf8PathBuf::from(format!("{root}/.workhive/config.toml")),
            last_opened_at: None,
            last_updated_at: None,
        }
    }

    #[test]
    fn valid_project_resolved_correctly() {
        let ws_fs = FakeWsFs::new(vec![("/tmp/exists", "a1b2")]);
        let summaries = vec![make_summary("myproj", "/tmp/exists")];
        let result = resolve_discovered_projects(&summaries, &ws_fs);
        assert_eq!(result.valid.len(), 1);
        assert!(result.stale.is_empty());
        assert_eq!(result.valid[0].instance_id, "a1b2");
    }

    #[test]
    fn missing_instance_id_produces_stale_entry() {
        let ws_fs = FakeWsFs::new(vec![]).with_roots(vec!["/tmp/no-id"]);
        let summaries = vec![make_summary("stale", "/tmp/no-id")];
        let result = resolve_discovered_projects(&summaries, &ws_fs);
        assert!(result.valid.is_empty());
        assert_eq!(result.stale.len(), 1);
        assert_eq!(result.stale[0].reason, StaleReason::InstanceIdMissing);
    }

    #[test]
    fn nonexistent_root_produces_project_root_gone() {
        let ws_fs = FakeWsFs::new(vec![]);
        let summaries = vec![make_summary("gone", "/nonexistent/path/xyz123")];
        let result = resolve_discovered_projects(&summaries, &ws_fs);
        assert!(result.valid.is_empty());
        assert_eq!(result.stale.len(), 1);
        assert_eq!(result.stale[0].reason, StaleReason::ProjectRootGone);
    }

    #[test]
    fn mixed_projects_classified_correctly() {
        let ws_fs = FakeWsFs::new(vec![("/tmp", "beef")]);
        let summaries = vec![
            make_summary("good", "/tmp"),
            make_summary("gone", "/nonexistent/path/xyz123"),
        ];
        let result = resolve_discovered_projects(&summaries, &ws_fs);
        assert_eq!(result.valid.len(), 1);
        assert_eq!(result.stale.len(), 1);
        assert_eq!(result.valid[0].project_name, "good");
        assert_eq!(result.stale[0].summary.project_name, "gone");
    }

    #[test]
    fn empty_input_returns_empty_result() {
        let ws_fs = FakeWsFs::new(vec![]);
        let result = resolve_discovered_projects(&[], &ws_fs);
        assert!(result.valid.is_empty());
        assert!(result.stale.is_empty());
    }
}

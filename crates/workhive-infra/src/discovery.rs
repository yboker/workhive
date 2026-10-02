//! Filesystem-based project discovery adapter.
//!
//! Recursively scans root directories for workhive-managed projects
//! (identified by `.workhive/instance_id`; project config is optional).

use camino::Utf8PathBuf;
use workhive_core::errors::DiscoveryError;
use workhive_core::ports::{
    DiscoveredProject, DiscoveryReport, DiscoveryWarning, ProjectDiscoveryPort,
};

/// Directories to skip during recursive scanning.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".cargo",
    "target",
    ".venv",
    "__pycache__",
    ".tox",
    ".mypy_cache",
    "vendor",
    "dist",
    "build",
];

/// Filesystem adapter for project discovery.
///
/// Scans directories recursively, skipping known large/irrelevant paths.
/// Once a `.workhive/` directory is found, the parent is recorded as a
/// project and its `worktrees/` subdirectory is not re-entered.
pub struct FsProjectDiscoveryAdapter;

impl FsProjectDiscoveryAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FsProjectDiscoveryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectDiscoveryPort for FsProjectDiscoveryAdapter {
    fn discover_projects_report(
        &self,
        roots: &[Utf8PathBuf],
    ) -> Result<DiscoveryReport, DiscoveryError> {
        if roots.is_empty() {
            return Err(DiscoveryError::NoRootsConfigured);
        }

        let mut report = DiscoveryReport::default();

        for root in roots {
            if !root.exists() {
                // A missing/unreadable *top-level* trusted root must
                // still hard-fail. Only unreadable *descendant*
                // directories degrade to typed warnings.
                return Err(DiscoveryError::RootNotFound { path: root.clone() });
            }
            let workhive_dir = root.join(".workhive");
            if workhive_dir.is_dir() {
                if let Some(project) = try_read_project(root, &workhive_dir) {
                    report.projects.push(project);
                }
                continue;
            }
            scan_directory_at_root(root, &mut report)?;
        }

        tracing::info!(
            roots_count = roots.len(),
            discovered_count = report.projects.len(),
            warning_count = report.warnings.len(),
            "project discovery complete"
        );

        Ok(report)
    }
}

/// Recursive scan starting from a verified trusted root.
///
/// Top-level (depth == 0) read failure is fatal because it indicates the
/// trusted-root entry itself is unusable. Descendant (depth > 0) read
/// failures degrade to typed [`DiscoveryWarning::UnreadableDescendant`]
/// so a single unreadable subtree (e.g. macOS' `MobileSync` backup
/// directory under `~/Library`) cannot wipe out an entire reindex.
fn scan_directory_at_root(
    root: &Utf8PathBuf,
    report: &mut DiscoveryReport,
) -> Result<(), DiscoveryError> {
    scan_directory(root, report, 0)
}

fn scan_directory(
    dir: &Utf8PathBuf,
    report: &mut DiscoveryReport,
    depth: usize,
) -> Result<(), DiscoveryError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(source) => {
            if depth == 0 {
                // The trusted root itself is unreadable. Surface as a
                // hard failure so the user's misconfiguration is fixed,
                // not silently swallowed.
                return Err(DiscoveryError::ReadFailed {
                    path: dir.clone(),
                    source,
                });
            }
            // Descendant. Treat as a non-fatal warning and stop
            // recursing into this subtree, but let siblings continue.
            tracing::warn!(
                path = %dir,
                error = %source,
                "skipping unreadable descendant subtree; siblings will still be scanned"
            );
            report
                .warnings
                .push(DiscoveryWarning::UnreadableDescendant {
                    path: dir.clone(),
                    reason: source.to_string(),
                });
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(dir = %dir, error = %e, "skipping unreadable entry");
                continue;
            }
        };

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(e) => {
                tracing::warn!(path = ?entry.path(), error = %e, "skipping entry with unknown type");
                continue;
            }
        };

        if !file_type.is_dir() {
            continue;
        }

        let entry_path = match Utf8PathBuf::try_from(entry.path()) {
            Ok(p) => p,
            Err(_) => {
                tracing::debug!(path = ?entry.path(), "skipping non-UTF-8 path");
                continue;
            }
        };

        let dir_name = entry_path.file_name().unwrap_or("");

        if SKIP_DIRS.contains(&dir_name) {
            continue;
        }

        // Check if this directory is a workhive project
        let workhive_dir = entry_path.join(".workhive");
        if workhive_dir.is_dir() {
            if let Some(project) = try_read_project(&entry_path, &workhive_dir) {
                tracing::debug!(
                    project_name = %project.project_name,
                    project_root = %project.project_root,
                    instance_id = %project.instance_id,
                    "discovered workhive project"
                );
                report.projects.push(project);
            }
            // Do not recurse into .workhive/worktrees
            continue;
        }

        // Recurse into subdirectory. Descendant read failures degrade
        // to a warning inside `scan_directory`; only top-level failures
        // bubble up as errors. Siblings of an unreadable subtree are
        // unaffected.
        scan_directory(&entry_path, report, depth + 1)?;
    }

    Ok(())
}

/// Attempt to read project metadata from a `.workhive/` directory.
///
/// Returns `None` if required files are missing or unreadable (the project
/// is skipped rather than failing the entire scan).
fn try_read_project(
    project_root: &Utf8PathBuf,
    workhive_dir: &Utf8PathBuf,
) -> Option<DiscoveredProject> {
    let instance_id_path = workhive_dir.join("instance_id");

    let instance_id = std::fs::read_to_string(&instance_id_path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());

    let initialized = instance_id.is_some();

    if !initialized {
        tracing::debug!(
            path = %project_root,
            "skipping incomplete workhive project without instance identity"
        );
        return None;
    }

    let project_name = project_root.file_name().unwrap_or("unknown").to_owned();

    Some(DiscoveredProject {
        project_name,
        project_root: project_root.clone(),
        instance_id: instance_id.unwrap_or_default(),
        initialized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_project(root: &std::path::Path, name: &str) -> std::path::PathBuf {
        let project = root.join(name);
        let workhive = project.join(".workhive");
        std::fs::create_dir_all(&workhive).unwrap();
        std::fs::write(workhive.join("config.toml"), "[default]\n").unwrap();
        std::fs::write(workhive.join("instance_id"), "a1b2").unwrap();
        project
    }

    fn utf8(p: &std::path::Path) -> Utf8PathBuf {
        Utf8PathBuf::try_from(p.to_path_buf()).unwrap()
    }

    #[test]
    fn discovers_project_in_root() {
        let tmp = TempDir::new().unwrap();
        setup_project(tmp.path(), "myproject");

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_name, "myproject");
        assert_eq!(projects[0].instance_id, "a1b2");
        assert!(projects[0].initialized);
    }

    #[test]
    fn discovers_nested_projects() {
        let tmp = TempDir::new().unwrap();
        let dev = tmp.path().join("development");
        std::fs::create_dir_all(&dev).unwrap();
        setup_project(&dev, "proj-a");
        setup_project(&dev, "proj-b");

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 2);
        let names: Vec<&str> = projects.iter().map(|p| p.project_name.as_str()).collect();
        assert!(names.contains(&"proj-a"));
        assert!(names.contains(&"proj-b"));
    }

    #[test]
    fn skips_incomplete_project_missing_instance_id() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("incomplete");
        let workhive = project.join(".workhive");
        std::fs::create_dir_all(&workhive).unwrap();
        std::fs::write(workhive.join("config.toml"), "[default]\n").unwrap();
        // No instance_id file

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert!(projects.is_empty());
    }

    #[test]
    fn discovers_initialized_project_without_config() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().join("no-config");
        let workhive = project.join(".workhive");
        std::fs::create_dir_all(&workhive).unwrap();
        std::fs::write(workhive.join("instance_id"), "a1b2").unwrap();
        // No config.toml

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].instance_id, "a1b2");
    }

    #[test]
    fn discovers_trusted_root_when_it_is_a_project() {
        let tmp = TempDir::new().unwrap();
        let project = setup_project(tmp.path(), "project");

        let adapter = FsProjectDiscoveryAdapter::new();
        let projects = adapter.discover_projects(&[utf8(&project)]).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_root, utf8(&project));
    }

    #[test]
    fn skips_git_and_node_modules() {
        let tmp = TempDir::new().unwrap();
        // Place a project inside .git — should be skipped
        setup_project(&tmp.path().join(".git"), "hidden");
        // Place a project inside node_modules — should be skipped
        setup_project(&tmp.path().join("node_modules"), "deps");
        // Place a real project
        setup_project(tmp.path(), "real");

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_name, "real");
    }

    #[test]
    fn does_not_recurse_into_worktrees_subdir() {
        let tmp = TempDir::new().unwrap();
        let project = setup_project(tmp.path(), "parent");
        // Create a nested "project" inside .workhive/worktrees — should NOT be found
        let worktrees = project.join(".workhive").join("worktrees");
        std::fs::create_dir_all(&worktrees).unwrap();
        let nested = worktrees.join("nested");
        let nested_wh = nested.join(".workhive");
        std::fs::create_dir_all(&nested_wh).unwrap();
        std::fs::write(nested_wh.join("config.toml"), "").unwrap();
        std::fs::write(nested_wh.join("instance_id"), "c3d4").unwrap();

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_name, "parent");
    }

    #[test]
    fn error_on_empty_roots() {
        let adapter = FsProjectDiscoveryAdapter::new();
        let result = adapter.discover_projects(&[]);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("no discovery roots"));
    }

    #[test]
    fn error_on_nonexistent_root() {
        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![Utf8PathBuf::from("/nonexistent/path/xyz")];
        let result = adapter.discover_projects(&roots);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("not found"));
    }

    #[test]
    fn multiple_roots_aggregates_results() {
        let tmp1 = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        setup_project(tmp1.path(), "alpha");
        setup_project(tmp2.path(), "beta");

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp1.path()), utf8(tmp2.path())];
        let projects = adapter.discover_projects(&roots).unwrap();

        assert_eq!(projects.len(), 2);
    }

    /// An unreadable descendant directory beneath an otherwise valid
    /// trusted root must not abort the whole scan. Sibling Workhive
    /// projects under the same trusted root MUST still be discovered,
    /// and the unreadable subtree must surface as a typed warning.
    ///
    /// Skipped on Windows because chmod(0o000) is not the right
    /// mechanism there; the production code path is identical, only
    /// the test fixture differs.
    #[cfg(unix)]
    #[test]
    fn unreadable_descendant_is_skipped_with_warning_and_siblings_still_discovered() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        // A genuine sibling Workhive project that lives next to the
        // unreadable subtree.
        setup_project(tmp.path(), "good-project");
        // An unreadable descendant — mimics `~/Library/Application
        // Support/MobileSync` on macOS. Directory exists but we cannot
        // read its contents.
        let unreadable = tmp.path().join("locked-subtree");
        std::fs::create_dir_all(&unreadable).unwrap();
        // Place a *would-be* project inside; it must NOT be discovered.
        setup_project(&unreadable, "hidden-by-permissions");
        std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(tmp.path())];
        let report = adapter.discover_projects_report(&roots);

        // Restore permissions so the temp dir can be cleaned up even if
        // the assertion below fails.
        let _ = std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755));

        let report = report.expect(
            "unreadable descendant must NOT abort the scan; \
             top-level root is readable and contains a sibling project",
        );

        // Sibling Workhive project is still discovered.
        let names: Vec<&str> = report
            .projects
            .iter()
            .map(|p| p.project_name.as_str())
            .collect();
        assert!(
            names.contains(&"good-project"),
            "sibling project under the same trusted root must still be \
             discovered when a peer subtree is unreadable, got: {names:?}"
        );
        // The would-be project inside the locked subtree is NOT
        // discovered, because we never descended into it.
        assert!(
            !names.contains(&"hidden-by-permissions"),
            "project under unreadable subtree must not leak through, got: {names:?}",
        );
        // The skip is recorded as a typed warning so the CLI can echo
        // it to stderr.
        assert!(
            !report.warnings.is_empty(),
            "skipped unreadable descendant must produce a typed warning"
        );
        let warned_paths: Vec<&str> = report
            .warnings
            .iter()
            .map(|w| match w {
                DiscoveryWarning::UnreadableDescendant { path, .. } => path.as_str(),
            })
            .collect();
        assert!(
            warned_paths.iter().any(|p| p.ends_with("locked-subtree")),
            "warning must reference the unreadable descendant, got: {warned_paths:?}",
        );
    }

    /// Top-level trusted roots that are themselves unreadable MUST still
    /// fail the entire reindex — they indicate a misconfigured trusted
    /// root, not a runtime permissions accident on some descendant.
    #[cfg(unix)]
    #[test]
    fn unreadable_top_level_root_still_hard_fails() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let locked_root = tmp.path().join("locked-root");
        std::fs::create_dir_all(&locked_root).unwrap();
        std::fs::set_permissions(&locked_root, std::fs::Permissions::from_mode(0o000)).unwrap();

        let adapter = FsProjectDiscoveryAdapter::new();
        let roots = vec![utf8(&locked_root)];
        let result = adapter.discover_projects_report(&roots);

        // Restore permissions so the temp dir can be cleaned up.
        let _ = std::fs::set_permissions(&locked_root, std::fs::Permissions::from_mode(0o755));

        let err = result.expect_err(
            "unreadable top-level trusted root must still produce a hard discovery error",
        );
        match err {
            DiscoveryError::ReadFailed { .. } => {}
            other => panic!("expected ReadFailed for top-level unreadable root, got: {other:?}"),
        }
    }
}

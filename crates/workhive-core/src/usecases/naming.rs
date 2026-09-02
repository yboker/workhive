//! Naming context assembly — pure functions for constructing `NamingContext`.
//!
//! These functions accept explicit inputs and perform no I/O, no environment
//! reads, and no port calls. The CLI call site is responsible for gathering
//! raw inputs (cwd, branch, instance_id) and passing them in.

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::NamingContext;

/// Assemble a `NamingContext` from explicit pre-collected inputs.
///
/// All environment/port reads must be performed by the caller before
/// invoking this function. This is a pure data transformation.
pub fn assemble_naming_context(
    project_name: &str,
    workspace: &str,
    project_root: &Utf8PathBuf,
    branch: Option<String>,
    instance_id: Option<String>,
    cwd: Option<&Utf8Path>,
) -> NamingContext {
    let project_with_subdir = resolve_project_with_subdir(project_name, project_root, cwd);
    NamingContext {
        project: project_with_subdir,
        workspace: workspace.to_owned(),
        branch,
        dir_path: Some(project_root.to_string()),
        instance_id,
    }
}

/// Compute project name with sub-directory offset relative to project root.
///
/// Returns `"reponame/subpath"` if `cwd` is inside a subdirectory of `project_root`,
/// otherwise just `"reponame"`. Pure function — no `std::env::current_dir()`.
pub fn resolve_project_with_subdir(
    project_name: &str,
    project_root: &Utf8PathBuf,
    cwd: Option<&Utf8Path>,
) -> String {
    if let Some(cwd) = cwd
        && let Ok(suffix) = cwd.strip_prefix(project_root)
    {
        let suffix_str = suffix.as_str();
        if !suffix_str.is_empty() && !is_managed_worktree_offset(suffix_str) {
            return format!("{project_name}/{suffix_str}");
        }
    }

    project_name.to_owned()
}

fn is_managed_worktree_offset(suffix: &str) -> bool {
    suffix == ".workhive/worktrees" || suffix.starts_with(".workhive/worktrees/")
}

/// Compute the unsanitized relative display path for a workspace.
///
/// For `main` workspace: always returns `None`.
/// For non-main workspace with a subdir offset: returns `"{project_with_subdir}/{workspace}"`,
/// preserving the real filesystem path characters (dots, etc.).
/// For non-main workspace without subdir: returns `"{workspace}"`.
///
/// Pure function — no I/O.
pub fn resolve_relative_display_path(
    project_name: &str,
    project_root: &Utf8PathBuf,
    workspace: &str,
    cwd: Option<&Utf8Path>,
) -> Option<String> {
    if workspace == "main" {
        return None;
    }

    if let Some(cwd) = cwd
        && let Ok(suffix) = cwd.strip_prefix(project_root)
    {
        let suffix_str = suffix.as_str();
        if !suffix_str.is_empty() && !is_managed_worktree_offset(suffix_str) {
            return Some(format!("{project_name}/{suffix_str}/{workspace}"));
        }
    }

    Some(workspace.to_owned())
}

/// Compute the sanitized subdirectory key for workspace identity.
///
/// Returns the sanitized subdirectory offset relative to `project_root`,
/// or an empty string if there is no subdirectory context.
/// This key is stable and file-safe, used for persistence and lookup.
///
/// Pure function — no I/O.
pub fn resolve_subdir_key(project_root: &Utf8PathBuf, cwd: Option<&Utf8Path>) -> String {
    if let Some(cwd) = cwd
        && let Ok(suffix) = cwd.strip_prefix(project_root)
    {
        let suffix_str = suffix.as_str();
        if !suffix_str.is_empty() && !is_managed_worktree_offset(suffix_str) {
            return sanitize_persistence_path(suffix_str);
        }
    }

    String::new()
}

/// Sanitize a relative path into a persistence-safe key.
///
/// This differs from tmux naming sanitization:
/// - each path component is sanitized independently
/// - path separators are replaced with `~`
/// - resulting key is file-safe and never contains `/`
pub fn sanitize_persistence_path(relative_path: &str) -> String {
    use crate::domain::SessionNamingPolicy;

    relative_path
        .split('/')
        .filter(|component| !component.is_empty())
        .map(SessionNamingPolicy::sanitize_component)
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>()
        .join("~")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subdir_offset_appended_when_cwd_inside_project() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/src/lib");
        let result = resolve_project_with_subdir("myrepo", &root, Some(cwd.as_ref()));
        assert_eq!(result, "myrepo/src/lib");
    }

    #[test]
    fn no_subdir_offset_when_cwd_is_root() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo");
        let result = resolve_project_with_subdir("myrepo", &root, Some(cwd.as_ref()));
        assert_eq!(result, "myrepo");
    }

    #[test]
    fn no_subdir_offset_when_cwd_is_none() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let result = resolve_project_with_subdir("myrepo", &root, None);
        assert_eq!(result, "myrepo");
    }

    #[test]
    fn no_subdir_offset_when_cwd_outside_project() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/tmp/other");
        let result = resolve_project_with_subdir("myrepo", &root, Some(cwd.as_ref()));
        assert_eq!(result, "myrepo");
    }

    #[test]
    fn no_subdir_offset_when_cwd_inside_managed_worktree() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/.workhive/worktrees/feature-x");
        let result = resolve_project_with_subdir("myrepo", &root, Some(cwd.as_ref()));
        assert_eq!(result, "myrepo");
    }

    #[test]
    fn no_subdir_offset_when_cwd_inside_managed_worktree_subdir() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd =
            Utf8PathBuf::from("/home/user/myrepo/.workhive/worktrees/feature-x/src/components");
        let result = resolve_project_with_subdir("myrepo", &root, Some(cwd.as_ref()));
        assert_eq!(result, "myrepo");
    }

    #[test]
    fn assemble_produces_correct_context() {
        let root = Utf8PathBuf::from("/projects/app");
        let cwd = Utf8PathBuf::from("/projects/app/backend");
        let ctx = assemble_naming_context(
            "app",
            "feature-x",
            &root,
            Some("main".to_owned()),
            Some("a1b2".to_owned()),
            Some(cwd.as_ref()),
        );
        assert_eq!(ctx.project, "app/backend");
        assert_eq!(ctx.workspace, "feature-x");
        assert_eq!(ctx.branch, Some("main".to_owned()));
        assert_eq!(ctx.instance_id, Some("a1b2".to_owned()));
        assert_eq!(ctx.dir_path, Some("/projects/app".to_owned()));
    }

    #[test]
    fn assemble_without_optional_fields() {
        let root = Utf8PathBuf::from("/projects/app");
        let ctx = assemble_naming_context("app", "ws1", &root, None, None, None);
        assert_eq!(ctx.project, "app");
        assert_eq!(ctx.workspace, "ws1");
        assert!(ctx.branch.is_none());
        assert!(ctx.instance_id.is_none());
    }

    // ── resolve_relative_display_path ──

    #[test]
    fn display_path_is_none_for_main_workspace() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python/1.0pingtai");
        assert_eq!(
            resolve_relative_display_path("python", &root, "main", Some(cwd.as_ref())),
            None
        );
    }

    #[test]
    fn display_path_includes_subdir_and_workspace() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python/1.0pingtai");
        assert_eq!(
            resolve_relative_display_path("python", &root, "feat.auth", Some(cwd.as_ref())),
            Some("python/1.0pingtai/feat.auth".to_owned())
        );
    }

    #[test]
    fn display_path_without_subdir_is_just_workspace() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo");
        assert_eq!(
            resolve_relative_display_path("myrepo", &root, "feat-x", Some(cwd.as_ref())),
            Some("feat-x".to_owned())
        );
    }

    #[test]
    fn display_path_without_cwd_is_just_workspace() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        assert_eq!(
            resolve_relative_display_path("myrepo", &root, "feat-x", None),
            Some("feat-x".to_owned())
        );
    }

    #[test]
    fn subdir_key_is_file_safe_for_nested_path() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/services/api");
        assert_eq!(
            resolve_subdir_key(&root, Some(cwd.as_ref())),
            "services~api".to_owned()
        );
    }

    #[test]
    fn subdir_key_sanitizes_special_chars_without_preserving_slashes() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python/1.0 platform/api.next");
        assert_eq!(
            resolve_subdir_key(&root, Some(cwd.as_ref())),
            "1_0-platform~api_next".to_owned()
        );
    }
}

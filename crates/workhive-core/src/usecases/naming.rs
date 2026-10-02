//! Naming context assembly — pure functions for constructing `NamingContext`.
//!
//! These functions accept explicit inputs and perform no I/O, no environment
//! reads, and no port calls. The CLI call site is responsible for gathering
//! raw inputs (cwd, branch, instance_id) and passing them in.

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::{NamingContext, RelativeProjectPath};

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
    relative_project_path: Option<RelativeProjectPath>,
) -> NamingContext {
    NamingContext {
        project: project_name.to_owned(),
        workspace: workspace.to_owned(),
        branch,
        dir_path: Some(project_root.to_string()),
        instance_id,
        relative_project_path,
    }
}

/// Compute the raw relative project path from cwd within the project root.
///
/// Returns the unsanitized, lossless relative path suffix, or empty string
/// for project root. For managed worktree paths (`.workhive/worktrees/<ws>`),
/// preserves subdirectory offsets relative to the containing worktree root.
/// Pure function — no I/O.
pub fn resolve_relative_project_path(project_root: &Utf8PathBuf, cwd: Option<&Utf8Path>) -> String {
    if let Some(cwd) = cwd
        && let Ok(suffix) = cwd.strip_prefix(project_root)
    {
        let suffix_str = suffix.as_str();
        if suffix_str.is_empty() {
            return String::new();
        }
        if let Some(inner) = extract_managed_worktree_subdir(suffix_str) {
            return inner;
        }
        return suffix_str.to_owned();
    }
    String::new()
}

/// For a managed worktree path `.workhive/worktrees/<ws>/<subdir>`,
/// returns the `<subdir>` portion. Returns empty string for the
/// worktree root itself, and `None` for non-worktree paths.
fn extract_managed_worktree_subdir(suffix: &str) -> Option<String> {
    const MANAGED_PREFIX: &str = ".workhive/worktrees/";
    if !suffix.starts_with(MANAGED_PREFIX) && suffix != ".workhive/worktrees" {
        return None;
    }
    if suffix == ".workhive/worktrees" {
        return Some(String::new());
    }
    let after_prefix = &suffix[MANAGED_PREFIX.len()..];
    match after_prefix.find('/') {
        Some(slash_pos) => {
            let subdir = &after_prefix[slash_pos + 1..];
            if subdir.is_empty() {
                Some(String::new())
            } else {
                Some(subdir.to_owned())
            }
        }
        None => Some(String::new()),
    }
}

/// Compute the unsanitized relative display path for a workspace.
///
/// Returns `None` when there is no subdirectory context (project-root provenance).
/// Returns `Some` with the display path when the caller is in a subdirectory.
/// Project-root status is determined by the absence of a subdirectory offset,
/// not by the workspace name.
///
/// Pure function — no I/O.
pub fn resolve_relative_display_path(
    project_name: &str,
    relative_project_path: Option<&RelativeProjectPath>,
    workspace: &str,
) -> Option<String> {
    let relative_path = relative_project_path?;
    Some(format!(
        "{project_name}/{}/{workspace}",
        relative_path.as_str()
    ))
}

/// Compute the lossless subdirectory key for workspace identity.
///
/// Returns the validated relative path from cwd within project root,
/// or `None` for project root. This value is used as a typed identity
/// input and must never be sanitized at the source.
///
/// Pure function — no I/O.
pub fn resolve_subdir_key(
    project_root: &Utf8PathBuf,
    cwd: Option<&Utf8Path>,
) -> Option<RelativeProjectPath> {
    let raw = resolve_relative_project_path(project_root, cwd);
    if raw.is_empty() {
        return None;
    }
    RelativeProjectPath::new(&raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_subdir_offset_when_cwd_is_root() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo");
        assert!(resolve_subdir_key(&root, Some(cwd.as_ref())).is_none());
    }

    #[test]
    fn no_subdir_offset_when_cwd_is_none() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        assert!(resolve_subdir_key(&root, None).is_none());
    }

    #[test]
    fn no_subdir_offset_when_cwd_outside_project() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/tmp/other");
        assert!(resolve_subdir_key(&root, Some(cwd.as_ref())).is_none());
    }

    #[test]
    fn no_subdir_offset_when_cwd_at_managed_worktree_root() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/.workhive/worktrees/feature-x");
        assert!(resolve_subdir_key(&root, Some(cwd.as_ref())).is_none());
    }

    #[test]
    fn managed_worktree_subdir_preserves_relative_path() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd =
            Utf8PathBuf::from("/home/user/myrepo/.workhive/worktrees/feature-x/src/components");
        let key = resolve_subdir_key(&root, Some(cwd.as_ref()));
        assert_eq!(
            key.as_ref().map(|p| p.as_str()),
            Some("src/components"),
            "subdirectory within managed worktree must be preserved"
        );
    }

    #[test]
    fn assemble_produces_correct_context() {
        let root = Utf8PathBuf::from("/projects/app");
        let subdir = RelativeProjectPath::new("backend").unwrap();
        let ctx = assemble_naming_context(
            "app",
            "feature-x",
            &root,
            Some("main".to_owned()),
            Some("a1b2".to_owned()),
            Some(subdir),
        );
        assert_eq!(ctx.project, "app");
        assert_eq!(ctx.workspace, "feature-x");
        assert_eq!(ctx.branch, Some("main".to_owned()));
        assert_eq!(ctx.instance_id, Some("a1b2".to_owned()));
        assert_eq!(ctx.dir_path, Some("/projects/app".to_owned()));
        assert_eq!(
            ctx.relative_project_path
                .as_ref()
                .map(RelativeProjectPath::as_str),
            Some("backend")
        );
    }

    #[test]
    fn assemble_without_optional_fields() {
        let root = Utf8PathBuf::from("/projects/app");
        let ctx = assemble_naming_context("app", "ws1", &root, None, None, None);
        assert_eq!(ctx.project, "app");
        assert_eq!(ctx.workspace, "ws1");
        assert!(ctx.branch.is_none());
        assert!(ctx.instance_id.is_none());
        assert!(ctx.relative_project_path.is_none());
    }

    // ── resolve_relative_display_path ──

    #[test]
    fn display_path_is_none_at_project_root() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python");
        assert_eq!(resolve_relative_display_path("python", None, "main"), None);
        assert_eq!(
            resolve_relative_display_path(
                "python",
                resolve_subdir_key(&root, Some(cwd.as_ref())).as_ref(),
                "feature/x"
            ),
            None,
        );
    }

    #[test]
    fn display_path_includes_subdir_and_workspace() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python/1.0pingtai");
        let subdir = resolve_subdir_key(&root, Some(cwd.as_ref())).unwrap();
        assert_eq!(
            resolve_relative_display_path("python", Some(&subdir), "feat.auth"),
            Some("python/1.0pingtai/feat.auth".to_owned())
        );
    }

    #[test]
    fn display_path_without_subdir_is_none() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo");
        assert_eq!(
            resolve_relative_display_path(
                "myrepo",
                resolve_subdir_key(&root, Some(cwd.as_ref())).as_ref(),
                "feat-x"
            ),
            None
        );
    }

    #[test]
    fn display_path_without_cwd_is_none() {
        assert_eq!(
            resolve_relative_display_path("myrepo", None, "feat-x"),
            None
        );
    }

    #[test]
    fn subdir_key_is_raw_lossless_path() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/services/api");
        assert_eq!(
            resolve_subdir_key(&root, Some(cwd.as_ref()))
                .as_ref()
                .map(RelativeProjectPath::as_str),
            Some("services/api")
        );
    }

    #[test]
    fn subdir_key_preserves_special_chars_losslessly() {
        let root = Utf8PathBuf::from("/home/user/python");
        let cwd = Utf8PathBuf::from("/home/user/python/1.0 platform/api.next");
        assert_eq!(
            resolve_subdir_key(&root, Some(cwd.as_ref()))
                .as_ref()
                .map(RelativeProjectPath::as_str),
            Some("1.0 platform/api.next")
        );
    }

    // ── typed_workspace_scope_ — project-relative path fidelity ──

    #[test]
    fn typed_workspace_scope_display_path_none_for_root_provenance() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo");
        assert_eq!(
            resolve_relative_display_path(
                "myrepo",
                resolve_subdir_key(&root, Some(cwd.as_ref())).as_ref(),
                "feature/api/login"
            ),
            None,
            "project-root workspace must have display_path=None regardless of name"
        );
    }

    #[test]
    fn typed_workspace_scope_subdir_key_preserves_dots() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/api.v1");
        let key = resolve_subdir_key(&root, Some(cwd.as_ref())).unwrap();
        assert_eq!(
            key.as_str(),
            "api.v1",
            "subdir key must preserve dots losslessly"
        );
    }

    #[test]
    fn typed_workspace_scope_dotted_subdirs_distinct() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd1 = Utf8PathBuf::from("/home/user/myrepo/api.v1");
        let cwd2 = Utf8PathBuf::from("/home/user/myrepo/api_v1");
        let k1 = resolve_subdir_key(&root, Some(cwd1.as_ref())).unwrap();
        let k2 = resolve_subdir_key(&root, Some(cwd2.as_ref())).unwrap();
        assert_ne!(k1, k2, "api.v1 and api_v1 must be distinct subdir keys");
    }

    // ── naming_context_scope_ — NamingContext project isolation ──

    #[test]
    fn naming_context_scope_project_is_name_only() {
        let root = Utf8PathBuf::from("/home/user/myrepo");
        let cwd = Utf8PathBuf::from("/home/user/myrepo/services/api");
        let subdir = resolve_subdir_key(&root, Some(cwd.as_ref())).unwrap();
        let ctx = assemble_naming_context(
            "myrepo",
            "main",
            &root,
            Some("main".to_owned()),
            Some("a1b2".to_owned()),
            Some(subdir),
        );
        assert_eq!(
            ctx.project, "myrepo",
            "project must not contain subdirectory path"
        );
        assert_eq!(
            ctx.relative_project_path
                .as_ref()
                .map(RelativeProjectPath::as_str),
            Some("services/api")
        );
    }

    #[test]
    fn naming_context_scope_subdir_does_not_leak_into_project() {
        let root = Utf8PathBuf::from("/projects/app");
        let cwd = Utf8PathBuf::from("/projects/app/backend/api");
        let subdir = resolve_subdir_key(&root, Some(cwd.as_ref())).unwrap();
        let ctx = assemble_naming_context("app", "main", &root, None, None, Some(subdir));
        assert_eq!(
            ctx.project, "app",
            "project field must remain the bare project name"
        );
        assert_eq!(
            ctx.relative_project_path
                .as_ref()
                .map(RelativeProjectPath::as_str),
            Some("backend/api")
        );
    }
}

//! Session naming policy — configurable template-based naming for tmux sessions.
//!
//! Canonical format uses `@` as structural separator (four segments):
//! - session: `wh@{instance_id}@{project}@{workspace}`
//! - server:  `whsrv-{instance_id}-{project}`
//!
//! Supports sub-directory semantics: project field may contain `/`
//! (e.g., `wh@a1b2@myrepo/subdir@main`).
//!
//! `instance_id`: 4-hex random from `.workhive/instance_id`, generated at `init`,
//! deleted on `destroy`. Enables isolation for same-named projects.
//!
//! Legacy templates (for backward compat with `-` / `--` era):
//! - server: `whsrv-{project}`
//! - session: `wh--{project}--{workspace}`

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Configuration for session naming behavior.
///
/// All fields have sensible defaults matching the canonical `@` convention.
#[derive(Debug, Clone)]
pub struct NamingConfig {
    pub server_template: String,
    pub session_template: String,
    pub sanitize: bool,
    pub max_component_len: usize,
}

impl Default for NamingConfig {
    fn default() -> Self {
        Self {
            // Server template uses `-` (not `@`) because it becomes the tmux
            // Unix socket filename via `-L`. Socket names must be filesystem-safe.
            server_template: "whsrv-{instance_id}-{project}".to_owned(),
            session_template: "wh@{instance_id}@{project}@{workspace}".to_owned(),
            sanitize: true,
            max_component_len: 32,
        }
    }
}

/// Context for resolving template placeholders.
///
/// # instance_id contract
///
/// Commands that produce canonical naming (server/session names) MUST
/// guarantee a real `instance_id` before constructing this context.
/// A missing `instance_id` in a canonical naming path is a caller bug.
///
/// Only `init` and `open` (via auto-init) are allowed to create
/// `instance_id`. All other commands must read an existing one.
#[derive(Debug, Clone)]
pub struct NamingContext {
    pub project: String,
    pub workspace: String,
    pub branch: Option<String>,
    pub dir_path: Option<String>,
    /// 4-hex project instance ID from `.workhive/instance_id`.
    /// Generated at `init`, deleted on `destroy`. Enables isolation for
    /// same-named projects in different directories.
    ///
    /// Must be `Some` before canonical naming resolution.
    /// `None` is only valid for project-level queries that do not
    /// produce canonical names (e.g., listing projects).
    pub instance_id: Option<String>,
}

/// Errors from naming context validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum NamingError {
    #[error(
        "instance_id is required for canonical naming but was not set — project may not be initialized"
    )]
    MissingInstanceId,
}

/// Pure-function policy that resolves session and server names from templates.
///
/// This is a value object — no I/O, no side effects. Designed for testability.
pub struct SessionNamingPolicy {
    config: NamingConfig,
}

impl SessionNamingPolicy {
    /// Create a policy from the given config.
    pub fn new(config: NamingConfig) -> Self {
        Self { config }
    }

    /// Create a policy with default config (`@` separator convention).
    pub fn default_policy() -> Self {
        Self::new(NamingConfig::default())
    }

    /// Resolve the tmux server name from the template and context.
    ///
    /// Server names become Unix socket filenames, so `/` (from sub-directory
    /// project names) is replaced with `-`.
    ///
    /// Returns `Err(NamingError::MissingInstanceId)` when `ctx.instance_id`
    /// is `None` — callers must guarantee a real instance_id before resolving.
    pub fn resolve_server(&self, ctx: &NamingContext) -> Result<String, NamingError> {
        let raw = self.expand_template(&self.config.server_template, ctx)?;
        Ok(raw.replace('/', "-"))
    }

    /// Resolve the tmux server name for project-level queries (e.g., session listing).
    ///
    /// Server names are project-scoped: they must not depend on `{workspace}` or
    /// `{branch}`. If the configured `server_template` contains these placeholders,
    /// this method falls back to the default template and logs a warning.
    pub fn resolve_server_for_project(&self, ctx: &NamingContext) -> Result<String, NamingError> {
        let template = &self.config.server_template;
        if template.contains("{workspace}") || template.contains("{branch}") {
            tracing::warn!(
                template = %template,
                "server_template contains {{workspace}}/{{branch}} placeholders; \
                 servers are project-scoped and must not vary by workspace/branch. \
                 Run `workhive config unset tmux.naming.server_template` to fix. \
                 Falling back to default template."
            );
            let default = NamingConfig::default();
            let raw = self.expand_template(&default.server_template, ctx)?;
            return Ok(raw.replace('/', "-"));
        }
        self.resolve_server(ctx)
    }

    /// Resolve the tmux session name from the template and context.
    ///
    /// Returns `Err(NamingError::MissingInstanceId)` when `ctx.instance_id`
    /// is `None`.
    pub fn resolve_session(&self, ctx: &NamingContext) -> Result<String, NamingError> {
        self.expand_template(&self.config.session_template, ctx)
    }

    /// Generate the legacy server name for fallback compatibility.
    pub fn legacy_server(project: &str) -> String {
        format!("whsrv-{project}")
    }

    /// Generate the legacy session name for fallback compatibility.
    pub fn legacy_session(project: &str, workspace: &str) -> String {
        format!("wh--{project}--{workspace}")
    }

    /// Sanitize a single naming component using the same rules as template expansion.
    ///
    /// This is the canonical way to obtain the tmux-safe version of a workspace
    /// or project component without reverse-parsing a session name.
    pub fn sanitize_component(input: &str) -> String {
        sanitize_component(input)
    }

    /// Parse a `wh@{instance_id}@{project}@{workspace}` session name.
    ///
    /// Returns `(instance_id, project, workspace)` or `None` if format invalid.
    pub fn parse_session(session: &str) -> Option<(&str, &str, &str)> {
        let rest = session.strip_prefix("wh@")?;
        let (instance_id, rest) = rest.split_once('@')?;
        let (project, workspace) = rest.rsplit_once('@')?;
        if instance_id.is_empty() || project.is_empty() || workspace.is_empty() {
            return None;
        }
        Some((instance_id, project, workspace))
    }

    /// Parse a `whsrv-{instance_id}-{project}` server name.
    ///
    /// Returns `(instance_id, project)` or `None` if format invalid.
    /// Server names use `-` (filesystem-safe for tmux sockets).
    pub fn parse_server(server: &str) -> Option<(&str, &str)> {
        let rest = server.strip_prefix("whsrv-")?;
        let (instance_id, project) = rest.split_once('-')?;
        if instance_id.is_empty() || project.is_empty() {
            return None;
        }
        Some((instance_id, project))
    }

    fn expand_template(&self, template: &str, ctx: &NamingContext) -> Result<String, NamingError> {
        let max = self.config.max_component_len;
        let sanitize = self.config.sanitize;

        let mut project = truncate_component(&ctx.project, max);
        let mut workspace = truncate_component(&ctx.workspace, max);
        let branch_raw = ctx.branch.as_deref().unwrap_or("detached");
        let mut branch = truncate_component(branch_raw, max);
        let dir_name_raw = ctx
            .dir_path
            .as_deref()
            .and_then(|p| p.rsplit('/').next())
            .unwrap_or(&ctx.project);
        let mut dir_name = truncate_component(dir_name_raw, max);
        let dir_hash8 = ctx
            .dir_path
            .as_deref()
            .map(|p| short_hash(p, 8))
            .unwrap_or_else(|| "00000000".to_owned());

        let instance_id_raw = ctx
            .instance_id
            .as_deref()
            .ok_or(NamingError::MissingInstanceId)?;
        let instance_id = truncate_component(instance_id_raw, 4);

        // `@` is a reserved structural separator — ALWAYS strip from components,
        // regardless of the `sanitize` setting. Full sanitization is optional.
        project = strip_reserved_separator(&project);
        workspace = strip_reserved_separator(&workspace);
        branch = strip_reserved_separator(&branch);
        dir_name = strip_reserved_separator(&dir_name);

        if sanitize {
            project = sanitize_component(&project);
            workspace = sanitize_component(&workspace);
            branch = sanitize_component(&branch);
            dir_name = sanitize_component(&dir_name);
        }

        Ok(template
            .replace("{project}", &project)
            .replace("{workspace}", &workspace)
            .replace("{branch}", &branch)
            .replace("{dir_name}", &dir_name)
            .replace("{dir_hash8}", &dir_hash8)
            .replace("{instance_id}", &instance_id))
    }
}

impl fmt::Debug for SessionNamingPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionNamingPolicy")
            .field("config", &self.config)
            .finish()
    }
}

/// Strip `@` from a component value unconditionally.
///
/// `@` is the structural separator in the canonical naming format.
/// Even when full sanitization is disabled, bare `@` in component
/// values would break the fixed-segment parse contract.
fn strip_reserved_separator(input: &str) -> String {
    input.replace('@', "_")
}

/// Sanitize a single template component for tmux compatibility.
///
/// Applied to each placeholder value individually so that the template's
/// structural `@` separators are preserved verbatim.
///
/// Rules:
/// - Lowercase
/// - Whitespace → `-`
/// - `@` → `_` (MUST NOT appear in component values; it's the structural separator)
/// - Non-alphanumeric (except `-`, `_`, `/`) → `_`
/// - `/` is preserved (sub-directory semantics: `project/subdir`)
/// - Collapse consecutive separators (not `/`)
/// - Trim leading/trailing separators
fn sanitize_component(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut result = String::with_capacity(lower.len());

    for ch in lower.chars() {
        // IMPORTANT:
        // `.` is intentionally NOT preserved because tmux target parsing may
        // misinterpret dotted names in `-t` contexts (session/window.pane).
        let replacement = if ch.is_ascii_alphanumeric() || ch == '/' {
            ch
        } else if ch.is_whitespace() {
            '-'
        } else if ch == '-' || ch == '_' {
            ch
        } else {
            // `@`, `#`, `$`, etc. → `_`
            '_'
        };
        result.push(replacement);
    }

    // Collapse consecutive separators (-, _) but NOT `/`
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_sep = false;
    for ch in result.chars() {
        let is_sep = ch == '-' || ch == '_';
        if is_sep {
            if !prev_sep {
                collapsed.push(ch);
            }
            prev_sep = true;
        } else {
            collapsed.push(ch);
            prev_sep = false;
        }
    }

    // Trim leading/trailing separators (not `/`)
    collapsed
        .trim_matches(|c: char| c == '-' || c == '_')
        .to_owned()
}

/// Truncate a component to `max_len` characters, preserving meaningful prefix.
/// Truncate to at most `max_len` **characters** (not bytes).
///
/// This is UTF-8 safe: it never splits a multi-byte character boundary,
/// so CJK project names, emoji branches, etc. will not panic.
fn truncate_component(s: &str, max_len: usize) -> String {
    s.chars().take(max_len).collect()
}

/// Compute a short hex hash of a string path for uniqueness.
fn short_hash(input: &str, len: usize) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();
    let hex = format!("{hash:016x}");
    hex[..len.min(16)].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_ctx() -> NamingContext {
        NamingContext {
            project: "my-project".to_owned(),
            workspace: "feature-auth".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/my-project".to_owned()),
            instance_id: Some("a1b2".to_owned()),
        }
    }

    // ── Default @ template ──

    #[test]
    fn default_server_socket_format() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_ctx();
        assert_eq!(
            policy.resolve_server(&ctx).unwrap(),
            "whsrv-a1b2-my-project"
        );
    }

    #[test]
    fn default_session_at_format() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_ctx();
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@a1b2@my-project@feature-auth"
        );
    }

    #[test]
    fn missing_instance_id_returns_error() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            instance_id: None,
            ..default_ctx()
        };
        assert!(
            policy.resolve_server(&ctx).is_err(),
            "resolve_server must fail when instance_id is None"
        );
        assert!(
            policy.resolve_session(&ctx).is_err(),
            "resolve_session must fail when instance_id is None"
        );
        assert!(
            matches!(
                policy.resolve_server(&ctx),
                Err(NamingError::MissingInstanceId)
            ),
            "must return MissingInstanceId variant"
        );
    }

    // ── Sub-directory semantics ──

    #[test]
    fn subdir_project_preserved_in_session() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo/test01".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/myrepo".to_owned()),
            instance_id: Some("a1b2".to_owned()),
        };
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@a1b2@myrepo/test01@main"
        );
        assert_eq!(
            policy.resolve_server(&ctx).unwrap(),
            "whsrv-a1b2-myrepo-test01"
        );
    }

    // ── Custom templates ──

    #[test]
    fn custom_template_with_branch() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            session_template: "wh@{instance_id}@{project}@{branch}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = default_ctx();
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@a1b2@my-project@main@feature-auth"
        );
    }

    #[test]
    fn template_with_dir_hash() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            server_template: "whsrv@{project}@{dir_hash8}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = default_ctx();
        let server = policy.resolve_server(&ctx).unwrap();
        assert!(server.starts_with("whsrv@my-project@"));
        let hash_part = server.strip_prefix("whsrv@my-project@").unwrap();
        assert_eq!(hash_part.len(), 8);
        assert!(hash_part.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn dir_name_uses_last_path_component() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            session_template: "{dir_name}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "fallback".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: Some("/home/user/deep/nested/repo-name".to_owned()),
            instance_id: Some("abcd".to_owned()),
        };
        assert_eq!(policy.resolve_session(&ctx).unwrap(), "repo-name@ws");
    }

    // ── Fallback values ──

    #[test]
    fn missing_branch_falls_back_to_detached() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            session_template: "wh@{project}@{branch}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "proj".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("1234".to_owned()),
        };
        assert_eq!(policy.resolve_session(&ctx).unwrap(), "wh@proj@detached@ws");
    }

    #[test]
    fn missing_dir_path_hash_falls_back_to_zeros() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            server_template: "{project}@{dir_hash8}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "proj".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        assert_eq!(policy.resolve_server(&ctx).unwrap(), "proj@00000000");
    }

    #[test]
    fn missing_dir_path_uses_project_as_dir_name() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            session_template: "{dir_name}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "my-proj".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        assert_eq!(policy.resolve_session(&ctx).unwrap(), "my-proj");
    }

    // ── Sanitization ──

    #[test]
    fn sanitize_removes_at_sign_from_components() {
        assert_eq!(sanitize_component("user@host"), "user_host");
    }

    #[test]
    fn sanitize_removes_special_chars() {
        assert_eq!(sanitize_component("hello world!@#$%"), "hello-world");
    }

    #[test]
    fn sanitize_lowercases() {
        assert_eq!(sanitize_component("My-Project"), "my-project");
    }

    #[test]
    fn sanitize_collapses_separators() {
        assert_eq!(sanitize_component("a---b___c"), "a-b_c");
    }

    #[test]
    fn sanitize_trims_leading_trailing_separators() {
        assert_eq!(sanitize_component("--hello--"), "hello");
    }

    #[test]
    fn sanitize_preserves_slash_for_subdir() {
        assert_eq!(sanitize_component("project/subdir"), "project/subdir");
        assert_eq!(sanitize_component("My-Repo/Sub Dir"), "my-repo/sub-dir");
    }

    #[test]
    fn sanitize_rewrites_dot_to_underscore_for_tmux_target_safety() {
        assert_eq!(sanitize_component("python/1.0pingtai"), "python/1_0pingtai");
        assert_eq!(sanitize_component("a..b"), "a_b");
    }

    #[test]
    fn sanitize_applied_to_components_not_template() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "My-Project".to_owned(),
            workspace: "Feature".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("AB12".to_owned()),
        };
        // Components are lowercased; template @ separators preserved
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@AB12@my-project@feature"
        );
    }

    #[test]
    fn sanitize_disabled_preserves_raw() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            sanitize: false,
            session_template: "wh@{instance_id}@{project}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "My-Project".to_owned(),
            workspace: "Feature".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("ab12".to_owned()),
        };
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@ab12@My-Project@Feature"
        );
    }

    #[test]
    fn at_sign_stripped_even_when_sanitize_disabled() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            sanitize: false,
            session_template: "wh@{instance_id}@{project}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "user@host".to_owned(),
            workspace: "feat@fix".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("ab12".to_owned()),
        };
        // @ in components replaced with _ even without sanitize
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@ab12@user_host@feat_fix"
        );
    }

    // ── Truncation ──

    #[test]
    fn long_component_truncated() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            max_component_len: 8,
            session_template: "{project}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "very-long-project-name-here".to_owned(),
            workspace: "also-very-long-workspace".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        assert_eq!(policy.resolve_session(&ctx).unwrap(), "very-lon@also-ver");
    }

    #[test]
    fn truncate_cjk_does_not_panic() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            max_component_len: 4,
            session_template: "{project}".to_owned(),
            sanitize: false,
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "ＡＢＣＤＥＦ".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        let result = policy.resolve_session(&ctx).unwrap();
        assert_eq!(result, "ＡＢＣＤ");
        assert_eq!(result.chars().count(), 4);
    }

    #[test]
    fn truncate_emoji_does_not_panic() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            max_component_len: 3,
            session_template: "{project}".to_owned(),
            sanitize: false,
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "🚀🎉💻🔥".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        let result = policy.resolve_session(&ctx).unwrap();
        assert_eq!(result, "🚀🎉💻");
    }

    #[test]
    fn truncate_mixed_ascii_cjk_safe() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            max_component_len: 6,
            session_template: "{project}".to_owned(),
            sanitize: false,
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "abcＡＢdef".to_owned(),
            workspace: "ws".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("abcd".to_owned()),
        };
        let result = policy.resolve_session(&ctx).unwrap();
        assert_eq!(result, "abcＡＢd");
    }

    // ── Parse ──

    #[test]
    fn parse_session_four_segments() {
        let parsed = SessionNamingPolicy::parse_session("wh@a1b2@my-project@feature-auth");
        assert_eq!(parsed, Some(("a1b2", "my-project", "feature-auth")));
    }

    #[test]
    fn parse_session_with_subdir() {
        let parsed = SessionNamingPolicy::parse_session("wh@a1b2@myrepo/test01@main");
        assert_eq!(parsed, Some(("a1b2", "myrepo/test01", "main")));
    }

    #[test]
    fn parse_session_invalid_returns_none() {
        assert!(SessionNamingPolicy::parse_session("random-name").is_none());
        assert!(SessionNamingPolicy::parse_session("wh@only@two").is_none());
        assert!(SessionNamingPolicy::parse_session("wh@@proj@ws").is_none());
    }

    #[test]
    fn parse_server_three_segments() {
        let parsed = SessionNamingPolicy::parse_server("whsrv-a1b2-my-project");
        assert_eq!(parsed, Some(("a1b2", "my-project")));
    }

    #[test]
    fn parse_server_invalid_returns_none() {
        assert!(SessionNamingPolicy::parse_server("random-name").is_none());
        assert!(SessionNamingPolicy::parse_server("whsrv-").is_none());
    }

    // ── Legacy helpers ──

    #[test]
    fn legacy_server_format() {
        assert_eq!(SessionNamingPolicy::legacy_server("myproj"), "whsrv-myproj");
    }

    #[test]
    fn legacy_session_format() {
        assert_eq!(
            SessionNamingPolicy::legacy_session("proj", "feat"),
            "wh--proj--feat"
        );
    }

    // ── Hash determinism ──

    #[test]
    fn dir_hash_is_deterministic() {
        let h1 = short_hash("/Users/dev/project", 8);
        let h2 = short_hash("/Users/dev/project", 8);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn different_paths_produce_different_hashes() {
        let h1 = short_hash("/Users/dev/project-a", 8);
        let h2 = short_hash("/Users/dev/project-b", 8);
        assert_ne!(h1, h2);
    }

    // ── resolve_server_for_project ──

    #[test]
    fn resolve_server_for_project_uses_default_template() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            workspace: String::new(),
            ..default_ctx()
        };
        assert_eq!(
            policy.resolve_server_for_project(&ctx).unwrap(),
            "whsrv-a1b2-my-project"
        );
    }

    #[test]
    fn resolve_server_for_project_falls_back_when_template_has_workspace() {
        let config = NamingConfig {
            server_template: "whsrv-{instance_id}-{project}-{workspace}".to_owned(),
            ..NamingConfig::default()
        };
        let policy = SessionNamingPolicy::new(config);
        let ctx = NamingContext {
            workspace: String::new(),
            ..default_ctx()
        };
        let result = policy.resolve_server_for_project(&ctx).unwrap();
        assert_eq!(result, "whsrv-a1b2-my-project");
    }

    #[test]
    fn resolve_server_for_project_falls_back_when_template_has_branch() {
        let config = NamingConfig {
            server_template: "srv-{project}-{branch}".to_owned(),
            ..NamingConfig::default()
        };
        let policy = SessionNamingPolicy::new(config);
        let ctx = default_ctx();
        let result = policy.resolve_server_for_project(&ctx).unwrap();
        assert_eq!(result, "whsrv-a1b2-my-project");
    }
}

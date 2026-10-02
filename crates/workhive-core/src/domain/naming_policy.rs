//! Session naming policy — configurable template-based naming for tmux sessions.
//!
//! Canonical format uses `@` as structural separator (four segments):
//! - session: `wh@{instance_id}@{project}@{workspace}`
//! - server:  `whsrv-{instance_id}-{project}`
//!
//! Scoped naming for subdirectory workspaces uses a digest-based project
//! component: `<prefix>-s-<16-hex-digest>`. The digest is a domain-separated
//! SHA-256 over the normalized relative path.
//!
//! `instance_id`: 4-hex random from `.workhive/instance_id`, generated at `init`,
//! deleted on `destroy`. Enables isolation for same-named projects.
//!
//! Legacy templates (for backward compat with `-` / `--` era):
//! - server: `whsrv-{project}`
//! - session: `wh--{project}--{workspace}`

use std::fmt;

use sha2::{Digest, Sha256};

use super::RelativeProjectPath;

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
            // Use the default tmux socket so sessions are visible to plain
            // `tmux ls` and to mobile clients (Moshi) that cannot specify `-L`.
            // Isolation is provided by globally-unique session names, not by
            // separate server processes.
            server_template: "default".to_owned(),
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
    /// Validated relative project path for subdirectory scope.
    /// `None` for project root.
    pub relative_project_path: Option<RelativeProjectPath>,
}

/// Minimum `max_component_len` required for scoped project encoding.
///
/// The suffix `-s-` (3 chars) + 16 hex digest = 19 characters must fit,
/// plus at least 1 character of readable project prefix.
const SCOPED_SUFFIX_LEN: usize = 19; // "-s-" (3) + 16 hex

/// Errors from naming context validation.
#[derive(Debug, Clone, thiserror::Error)]
pub enum NamingError {
    #[error(
        "instance_id is required for canonical naming but was not set — project may not be initialized"
    )]
    MissingInstanceId,

    #[error(
        "max_component_len ({configured}) is too small for scoped naming; \
         minimum is {minimum} (1 prefix char + 19 suffix chars)"
    )]
    ScopedComponentTooShort { configured: usize, minimum: usize },
}

/// Pure-function policy that resolves session and server names from templates.
///
/// This is a value object — no I/O, no side effects. Designed for testability.
#[derive(Clone)]
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

    /// Check whether a session name belongs to a specific project.
    ///
    /// Handles both canonical (`wh@{iid}@{project}@{ws}`) and legacy
    /// (`wh--{project}--{ws}`) formats. Returns `false` for session names
    /// that don't match either format (e.g. user-created sessions).
    pub fn session_belongs_to_project(
        session_name: &str,
        instance_id: &str,
        project_component: &str,
    ) -> bool {
        if let Some((iid, proj, _ws)) = Self::parse_session(session_name) {
            return iid == instance_id && proj == project_component;
        }
        if let Some(rest) = session_name.strip_prefix("wh--") {
            return rest.starts_with(&format!("{project_component}--"));
        }
        false
    }

    fn expand_template(&self, template: &str, ctx: &NamingContext) -> Result<String, NamingError> {
        let max = self.config.max_component_len;
        let sanitize = self.config.sanitize;

        let scoped_project = match &ctx.relative_project_path {
            Some(rp) => {
                let min_required = SCOPED_SUFFIX_LEN + 1;
                if max < min_required {
                    return Err(NamingError::ScopedComponentTooShort {
                        configured: max,
                        minimum: min_required,
                    });
                }
                let digest = scope_digest(rp.as_str());
                let prefix_budget = max - SCOPED_SUFFIX_LEN;
                let sanitized_bare = sanitize_component(&ctx.project);
                let prefix = truncate_component(&sanitized_bare, prefix_budget);
                format!("{prefix}-s-{digest}")
            }
            None => ctx.project.clone(),
        };
        let mut project = truncate_component(&scoped_project, max);
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
/// - Non-alphanumeric (except `-`, `_`) → `_`
/// - `/` → `-` (scoped naming uses digest, not path separators)
/// - Collapse consecutive separators
/// - Trim leading/trailing separators
fn sanitize_component(input: &str) -> String {
    let lower = input.to_lowercase();
    let mut result = String::with_capacity(lower.len());

    for ch in lower.chars() {
        let replacement = if ch.is_ascii_alphanumeric() {
            ch
        } else if ch.is_whitespace() || ch == '/' {
            '-'
        } else if ch == '-' || ch == '_' {
            ch
        } else {
            '_'
        };
        result.push(replacement);
    }

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

/// Compute a short hex hash of a string using the default hasher.
///
/// Used only for non-identity purposes like `{dir_hash8}` template
/// placeholder. Identity-sensitive hashing uses `scope_digest`.
fn short_hash(input: &str, len: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();
    let hex = format!("{hash:016x}");
    hex[..len.min(16)].to_owned()
}

/// Domain-separated SHA-256 digest over a normalized relative path.
///
/// Returns the first 16 lowercase hexadecimal characters. The domain
/// prefix ensures that the digest namespace is disjoint from any
/// other hash usage in the system.
fn scope_digest(relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"workhive:scope:");
    hasher.update(relative_path.as_bytes());
    let hash = hasher.finalize();
    let hex = format!("{hash:x}");
    hex[..16].to_owned()
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
            relative_project_path: None,
        }
    }

    // ── Default @ template ──

    #[test]
    fn default_server_socket_format() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_ctx();
        assert_eq!(policy.resolve_server(&ctx).unwrap(), "default");
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
    fn subdir_project_sanitized_in_session() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo/test01".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/myrepo".to_owned()),
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        assert_eq!(
            policy.resolve_session(&ctx).unwrap(),
            "wh@a1b2@myrepo-test01@main"
        );
        assert_eq!(policy.resolve_server(&ctx).unwrap(), "default");
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
    fn sanitize_converts_slash_to_dash() {
        assert_eq!(sanitize_component("project/subdir"), "project-subdir");
        assert_eq!(sanitize_component("My-Repo/Sub Dir"), "my-repo-sub-dir");
    }

    #[test]
    fn sanitize_rewrites_dot_to_underscore_for_tmux_target_safety() {
        assert_eq!(sanitize_component("python/1.0pingtai"), "python-1_0pingtai");
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
            relative_project_path: None,
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
        assert_eq!(policy.resolve_server_for_project(&ctx).unwrap(), "default");
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
        assert_eq!(result, "default");
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
        assert_eq!(result, "default");
    }

    // ── runtime_scope_isolation_ — distinct relative paths produce distinct names ──

    #[test]
    fn runtime_scope_isolation_root_vs_subdir_session() {
        let policy = SessionNamingPolicy::default_policy();
        let root_ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        let subdir_ctx = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
            ..root_ctx.clone()
        };
        let root_session = policy.resolve_session(&root_ctx).unwrap();
        let subdir_session = policy.resolve_session(&subdir_ctx).unwrap();
        assert_ne!(
            root_session, subdir_session,
            "root vs services/api must differ"
        );
    }

    #[test]
    fn runtime_scope_isolation_root_vs_subdir_server() {
        let policy = SessionNamingPolicy::default_policy();
        let root_ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        let subdir_ctx = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
            ..root_ctx.clone()
        };
        let root_server = policy.resolve_server(&root_ctx).unwrap();
        let subdir_server = policy.resolve_server(&subdir_ctx).unwrap();
        assert_eq!(
            root_server, subdir_server,
            "root and subdir share the same default server"
        );
        assert_eq!(root_server, "default");
    }

    #[test]
    fn runtime_scope_isolation_different_subdirs() {
        let policy = SessionNamingPolicy::default_policy();
        let base = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        let api_ctx = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
            ..base.clone()
        };
        let web_ctx = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("services/web").unwrap()),
            ..base.clone()
        };
        assert_ne!(
            policy.resolve_session(&api_ctx).unwrap(),
            policy.resolve_session(&web_ctx).unwrap(),
            "services/api vs services/web sessions must differ"
        );
    }

    #[test]
    fn runtime_scope_isolation_dotted_vs_underscored() {
        let policy = SessionNamingPolicy::default_policy();
        let base = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        let dotted = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("api.v1").unwrap()),
            ..base.clone()
        };
        let underscored = NamingContext {
            relative_project_path: Some(RelativeProjectPath::new("api_v1").unwrap()),
            ..base.clone()
        };
        assert_ne!(
            policy.resolve_session(&dotted).unwrap(),
            policy.resolve_session(&underscored).unwrap(),
            "api.v1 vs api_v1 sessions must differ"
        );
    }

    // ── naming_context_scope_ — contract-required scoped encoding shape ──

    #[test]
    fn naming_context_scope_scoped_project_contains_no_slash() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
        };
        let server = policy.resolve_server(&ctx).unwrap();
        let session = policy.resolve_session(&ctx).unwrap();
        // The {project} component must not contain `/` for tmux safety
        let project_in_session = session
            .strip_prefix("wh@a1b2@")
            .unwrap()
            .rsplit_once('@')
            .unwrap()
            .0;
        assert!(
            !project_in_session.contains('/'),
            "scoped project in session must not contain '/', got: {project_in_session}"
        );
        assert!(
            !server.contains('/'),
            "server name must not contain '/', got: {server}"
        );
    }

    #[test]
    fn naming_context_scope_scoped_project_uses_digest_suffix() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
        };
        let session = policy.resolve_session(&ctx).unwrap();
        let project_in_session = session
            .strip_prefix("wh@a1b2@")
            .unwrap()
            .rsplit_once('@')
            .unwrap()
            .0;
        // Must contain `-s-` separator and 16 hex digest
        assert!(
            project_in_session.contains("-s-"),
            "scoped project must use -s- separator, got: {project_in_session}"
        );
        let digest_part = project_in_session.rsplit_once("-s-").unwrap().1;
        assert_eq!(
            digest_part.len(),
            16,
            "scope digest must be 16 hex chars, got {}: {}",
            digest_part.len(),
            digest_part
        );
        assert!(
            digest_part.chars().all(|c| c.is_ascii_hexdigit()),
            "scope digest must be lowercase hex, got: {digest_part}"
        );
    }

    #[test]
    fn naming_context_scope_root_project_unchanged() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        };
        let session = policy.resolve_session(&ctx).unwrap();
        assert_eq!(session, "wh@a1b2@myrepo@main");
    }

    #[test]
    fn naming_context_scope_min_component_len_error() {
        let policy = SessionNamingPolicy::new(NamingConfig {
            max_component_len: 19,
            ..NamingConfig::default()
        });
        let ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
        };
        let result = policy.resolve_session(&ctx);
        assert!(
            result.is_err(),
            "max_component_len < 20 with scoped path must error"
        );
    }

    #[test]
    fn naming_context_scope_scoped_no_whitespace_or_control() {
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "myrepo".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
        };
        let server = policy.resolve_server(&ctx).unwrap();
        let session = policy.resolve_session(&ctx).unwrap();
        let project_in_session = session
            .strip_prefix("wh@a1b2@")
            .unwrap()
            .rsplit_once('@')
            .unwrap()
            .0;
        for ch in project_in_session.chars() {
            assert!(
                !ch.is_whitespace() && !ch.is_control() && ch != '\\' && ch != '@',
                "scoped project must not contain whitespace/control/backslash/@, found: {ch:?}"
            );
        }
        for ch in server.chars() {
            assert!(
                !ch.is_whitespace() && !ch.is_control() && ch != '/' && ch != '\\' && ch != '@',
                "server must not contain whitespace/control/slash/backslash/@, found: {ch:?}"
            );
        }
    }
}

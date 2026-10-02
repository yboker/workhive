//! Environment-variable use cases — parsing, validation, merging, redaction.
//!
//! Implements the `.env` template and pane-injection observability
//! contract.
//!
//! Responsibilities:
//! - Pure `KEY=VALUE` syntax parsing with comment / blank-line handling.
//! - Surfacing invalid lines (e.g. `KEY: VALUE`) instead of silently ignoring them.
//! - A self-documenting global `.env` template that the wizard / config writer
//!   uses to seed new files.
//! - Effective-env resolution (global merged with project, project wins).
//! - Secret-aware logging redaction so values never leak through `tracing`,
//!   user output, or error strings.
//!
//! All I/O lives in the infra `ConfigPort` adapter; this module only contains
//! pure logic so it can be exhaustively unit tested.

use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};

use crate::errors::ConfigError;
use crate::ports::{ConfigPort, EnvLineIssue};

/// Standard self-documenting header written at the top of every Workhive
/// `.env` file.
///
/// Includes:
/// - An explicit `KEY=VALUE` syntax statement.
/// - A note rejecting YAML-style `KEY: VALUE`.
/// - A pointer to the Claude Code fullscreen rendering setting, kept
///   commented so the user opts in deliberately.
///
/// The header MUST end with a single trailing blank line so it visually
/// separates from any user-defined entries.
pub const ENV_TEMPLATE_HEADER: &str = "\
# Workhive environment variables.
# Syntax: KEY=VALUE
# Blank lines and lines starting with # are ignored.
# Do not use YAML syntax such as KEY: VALUE.
#
# Global file: ~/.config/workhive/.env
# Project file: <project>/.workhive/.env
# Project entries override global entries with the same key.
#
# Claude Code fullscreen rendering / reduced flicker.
# See: https://code.claude.com/docs/en/fullscreen#enable-fullscreen-rendering
# CLAUDE_CODE_NO_FLICKER=1
";

/// Substrings (case-insensitive) that mark a key as secret-like and force
/// value redaction even at `debug` level.
///
/// We deliberately use simple substring matching so the policy is obvious
/// to operators reading the source. Keep the list short and conservative;
/// when in doubt the policy still redacts ALL values from `.env`.
pub const SECRET_KEY_HINTS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "KEY",
    "PASSWORD",
    "PASS",
    "CREDENTIAL",
    "AUTH",
    "API",
    "PRIVATE",
];

/// One parsed `.env` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvEntry {
    /// 1-based line number in the source file.
    pub line: u32,
    /// Variable key.
    pub key: String,
    /// Variable raw value (UTF-8, no quote stripping).
    pub value: String,
}

/// One invalid `.env` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvParseIssue {
    /// 1-based line number in the source file.
    pub line: u32,
    /// Original line text (whitespace preserved) — surfaced to the user.
    pub raw: String,
    /// Why the line was rejected.
    pub reason: EnvParseReason,
}

/// Why an `.env` line was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvParseReason {
    /// Looks like YAML (`KEY: VALUE`).
    LooksLikeYaml,
    /// No `=` separator and not a comment / blank line.
    MissingEquals,
    /// Empty key — line was `=foo` or starts with `=`.
    EmptyKey,
    /// Key contains characters disallowed for environment variable names.
    InvalidKeyChars,
}

impl EnvParseReason {
    /// Stable English description suitable for logs and error chains.
    /// User-visible copy goes through i18n at the CLI boundary.
    pub fn description(&self) -> &'static str {
        match self {
            Self::LooksLikeYaml => {
                ".env uses KEY=VALUE syntax; YAML-style 'KEY: VALUE' is not supported"
            }
            Self::MissingEquals => "missing '=' separator (expected KEY=VALUE)",
            Self::EmptyKey => "empty key before '=' separator",
            Self::InvalidKeyChars => {
                "key contains characters that are not allowed in environment variable names"
            }
        }
    }
}

/// Result of parsing a `.env` file body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvParseResult {
    /// Successfully parsed entries, in source order.
    pub entries: Vec<EnvEntry>,
    /// Lines that violated the syntax contract.
    pub issues: Vec<EnvParseIssue>,
}

impl EnvParseResult {
    /// Convenience accessor: borrow entries as `&[(key, value)]`-shaped
    /// pairs without cloning. Useful when downstream APIs only need the
    /// pair representation.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect()
    }
}

/// Parse a `.env` file body into entries plus issues.
///
/// Rules (kept identical to the user-facing template):
/// 1. A line whose trimmed start begins with `#` is a comment.
/// 2. A whitespace-only line is ignored.
/// 3. Otherwise the line MUST contain `=`. The portion before `=` is the
///    key, after is the raw value. Both sides are trimmed of surrounding
///    whitespace.
/// 4. A key with a leading `export ` prefix (POSIX shell habit) is accepted
///    and the prefix stripped — common enough that rejecting it would be
///    user-hostile.
/// 5. A key containing characters outside `[A-Za-z0-9_]` is rejected.
/// 6. A line that contains `:` before any `=` and looks like `KEY: VALUE`
///    is reported as `LooksLikeYaml` so users get an explicit fix hint.
pub fn parse_env_body(body: &str) -> EnvParseResult {
    let mut result = EnvParseResult::default();
    for (idx, raw_line) in body.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Detect YAML-style `KEY: VALUE` BEFORE generic missing-equals so
        // the user sees the actionable reason. We only flag when the colon
        // sits to the LEFT of any `=` and the part before the colon is a
        // plausible env key.
        if let Some(colon_idx) = trimmed.find(':') {
            let before = trimmed[..colon_idx].trim();
            let has_equals_before_colon =
                trimmed.find('=').is_some_and(|eq_idx| eq_idx < colon_idx);
            if !before.is_empty() && !has_equals_before_colon && is_plausible_key(before) {
                result.issues.push(EnvParseIssue {
                    line: line_no,
                    raw: raw_line.to_owned(),
                    reason: EnvParseReason::LooksLikeYaml,
                });
                continue;
            }
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            result.issues.push(EnvParseIssue {
                line: line_no,
                raw: raw_line.to_owned(),
                reason: EnvParseReason::MissingEquals,
            });
            continue;
        };

        let key = strip_export(raw_key.trim());
        if key.is_empty() {
            result.issues.push(EnvParseIssue {
                line: line_no,
                raw: raw_line.to_owned(),
                reason: EnvParseReason::EmptyKey,
            });
            continue;
        }
        if !is_valid_env_key(key) {
            result.issues.push(EnvParseIssue {
                line: line_no,
                raw: raw_line.to_owned(),
                reason: EnvParseReason::InvalidKeyChars,
            });
            continue;
        }

        result.entries.push(EnvEntry {
            line: line_no,
            key: key.to_owned(),
            value: raw_value.trim().to_owned(),
        });
    }
    result
}

fn strip_export(key: &str) -> &str {
    key.strip_prefix("export ")
        .map(str::trim_start)
        .unwrap_or(key)
}

fn is_plausible_key(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(is_valid_env_key_char)
}

fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(is_valid_env_key_char)
}

fn is_valid_env_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Render entries plus the standard header back to the on-disk form.
///
/// Preserves the standard header from [`ENV_TEMPLATE_HEADER`] so that
/// `env-set` / `env-unset` mutations never lose the syntax documentation.
/// Existing user comments are NOT preserved by this minimal renderer; the
/// header itself is the safety net.
pub fn render_env_file(entries: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ENV_TEMPLATE_HEADER.len() + entries.len() * 32);
    out.push_str(ENV_TEMPLATE_HEADER);
    if !entries.is_empty() {
        out.push('\n');
        for (k, v) in entries {
            out.push_str(k);
            out.push('=');
            out.push_str(v);
            out.push('\n');
        }
    }
    out
}

/// Source of a single env entry in the effective-env merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvSource {
    /// Entry came from `~/.config/workhive/.env`.
    Global,
    /// Entry came from `<project>/.workhive/.env`.
    Project,
    /// Entry came from a project value that overrode a same-key global value.
    ProjectOverride,
}

/// One entry in the effective env, after global + project merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveEnvEntry {
    /// Variable key.
    pub key: String,
    /// Variable value (NEVER logged at info/warn levels).
    pub value: String,
    /// Where this entry came from.
    pub source: EnvSource,
}

/// Result of resolving the effective env for a `workspace open` invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveEnv {
    /// Entries in deterministic order: global keys first (in their original
    /// order), then project-only keys (in their original order). Overrides
    /// keep their original global position.
    pub entries: Vec<EffectiveEnvEntry>,
    /// Resolved global file path (for log/UI display). `None` if the file
    /// does not exist or the global config is missing.
    pub global_path: Option<Utf8PathBuf>,
    /// Resolved project file path. `None` if no project root was supplied
    /// or the file does not exist.
    pub project_path: Option<Utf8PathBuf>,
    /// Number of global keys that contributed.
    pub global_count: usize,
    /// Number of project keys that contributed (including overrides).
    pub project_count: usize,
    /// Number of project keys that overrode a same-key global entry.
    pub override_count: usize,
}

impl EffectiveEnv {
    /// Convert the merged entries into a key-value map suitable for spawn
    /// requests / shell exports.
    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect()
    }

    /// Total number of effective entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the merge resolved zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One parse issue from one of the two `.env` overlays, tagged with the
/// resolved source path so the CLI can render `path:line: reason`
/// warnings without re-deriving the file location.
///
/// We deliberately do NOT carry the raw line text or the value: the
/// warning surfaces include only the source path, the 1-based line
/// number, and a typed reason. The reason is rendered via i18n at the
/// CLI boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveEnvIssue {
    /// Resolved on-disk path of the offending file.
    pub source: Utf8PathBuf,
    /// Port-level issue (line + typed kind).
    pub issue: EnvLineIssue,
}

/// Resolve the effective env for `workspace open`.
///
/// Reads global + project `.env` via the supplied [`ConfigPort`], parses
/// each, and merges with project keys overriding global keys.
///
/// Returns the parsed [`EffectiveEnv`] together with the parse issues
/// from BOTH overlays. Issues are tagged with their resolved source
/// path so the CLI can render localized "warning: .env at <path>
/// line <n>: <reason>" messages — the user-visible half of the `.env`
/// diagnostics contract (a malformed line must not just write a
/// tracing warn; workspace open must show path, line, reason).
pub fn resolve_effective_env(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global_path: Option<Utf8PathBuf>,
    project_path: Option<Utf8PathBuf>,
) -> Result<(EffectiveEnv, Vec<EffectiveEnvIssue>), ConfigError> {
    let global_report = config_port.load_env_report(project_root, true)?;
    let project_report = if project_root.is_some() {
        config_port.load_env_report(project_root, false)?
    } else {
        crate::ports::EnvLoadReport::default()
    };

    let merged = merge_env_pairs(&global_report.entries, &project_report.entries);
    let mut effective = merged;
    // Prefer adapter-resolved paths (so the CLI shows the actual file
    // location) but allow the caller to pre-populate them — matters for
    // dry-run callers that already know the path.
    effective.global_path = global_path.or(global_report.path.clone());
    effective.project_path = project_path.or(project_report.path.clone());

    let mut issues: Vec<EffectiveEnvIssue> = Vec::new();
    if let Some(src) = global_report.path.as_ref() {
        for issue in global_report.issues {
            issues.push(EffectiveEnvIssue {
                source: src.clone(),
                issue,
            });
        }
    }
    if let Some(src) = project_report.path.as_ref() {
        for issue in project_report.issues {
            issues.push(EffectiveEnvIssue {
                source: src.clone(),
                issue,
            });
        }
    }
    Ok((effective, issues))
}

/// Merge two pre-parsed env lists into a deterministic effective env.
///
/// Project keys override global keys with the same name; the overriding
/// entry is tagged [`EnvSource::ProjectOverride`].
pub fn merge_env_pairs(global: &[(String, String)], project: &[(String, String)]) -> EffectiveEnv {
    use std::collections::HashSet;

    let project_keys: HashSet<&str> = project.iter().map(|(k, _)| k.as_str()).collect();
    let mut entries: Vec<EffectiveEnvEntry> = Vec::new();
    let mut override_count = 0usize;

    // Step 1: emit globals in their original order, replacing values where
    // the project overrides them.
    for (k, v) in global {
        if let Some((_, pv)) = project.iter().find(|(pk, _)| pk == k) {
            entries.push(EffectiveEnvEntry {
                key: k.clone(),
                value: pv.clone(),
                source: EnvSource::ProjectOverride,
            });
            override_count += 1;
        } else {
            entries.push(EffectiveEnvEntry {
                key: k.clone(),
                value: v.clone(),
                source: EnvSource::Global,
            });
        }
    }

    // Step 2: append project-only keys not already represented above.
    let global_keys: HashSet<&str> = global.iter().map(|(k, _)| k.as_str()).collect();
    for (k, v) in project {
        if !global_keys.contains(k.as_str()) {
            entries.push(EffectiveEnvEntry {
                key: k.clone(),
                value: v.clone(),
                source: EnvSource::Project,
            });
        }
    }

    // Maintain insertion order for stable rendering. Deduplicate within
    // global and project lists conservatively (last-wins) to match common
    // dotenv expectations.
    dedup_entries_last_wins(&mut entries);

    let global_count = entries
        .iter()
        .filter(|e| e.source == EnvSource::Global)
        .count();
    let project_count = entries
        .iter()
        .filter(|e| matches!(e.source, EnvSource::Project | EnvSource::ProjectOverride))
        .count();
    let _ = project_keys; // silence dead-code lint until we add cross-validation
    EffectiveEnv {
        entries,
        global_path: None,
        project_path: None,
        global_count,
        project_count,
        override_count,
    }
}

fn dedup_entries_last_wins(entries: &mut Vec<EffectiveEnvEntry>) {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut keep = vec![true; entries.len()];
    for (idx, entry) in entries.iter().enumerate() {
        if let Some(prev) = seen.insert(entry.key.clone(), idx) {
            keep[prev] = false;
        }
    }
    let mut i = 0;
    entries.retain(|_| {
        let k = keep[i];
        i += 1;
        k
    });
}

/// Whether a key's value should be redacted when displayed to the user.
///
/// Today the policy is the strongest available: ALL values from `.env`
/// are considered sensitive. The heuristic in [`SECRET_KEY_HINTS`] is
/// retained so future relaxations (e.g. an opt-in "show non-secret values"
/// flag) can detect the high-risk subset without reintroducing leaks.
pub fn is_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    SECRET_KEY_HINTS.iter().any(|hint| upper.contains(hint))
}

/// Render a single value for log output.
///
/// Always returns `"<redacted>"` to satisfy the env logging
/// rule: even non-secret-looking keys are still raw `.env` material that
/// may carry credentials.
pub fn redact_value(_value: &str) -> &'static str {
    REDACTED_PLACEHOLDER
}

/// Placeholder string used in log/UI surfaces in place of raw env values.
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_basic_key_value() {
        let result = parse_env_body("FOO=bar\nBAZ=qux\n");
        assert!(result.issues.is_empty());
        assert_eq!(
            result.entries,
            vec![
                EnvEntry {
                    line: 1,
                    key: "FOO".into(),
                    value: "bar".into()
                },
                EnvEntry {
                    line: 2,
                    key: "BAZ".into(),
                    value: "qux".into()
                },
            ]
        );
    }

    #[test]
    fn parse_ignores_comments_and_blank_lines() {
        let result = parse_env_body("# top\n\n   \n# CLAUDE_CODE_NO_FLICKER=1\nA=1\n");
        assert_eq!(result.entries.len(), 1);
        assert!(result.issues.is_empty());
        assert_eq!(result.entries[0].key, "A");
    }

    #[test]
    fn parse_reports_yaml_style_line() {
        let result = parse_env_body("CLAUDE_CODE_NO_FLICKER: 1\n");
        assert!(result.entries.is_empty(), "yaml-style line must not parse");
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].line, 1);
        assert_eq!(result.issues[0].reason, EnvParseReason::LooksLikeYaml);
    }

    #[test]
    fn parse_reports_missing_equals() {
        let result = parse_env_body("FOO_BAR_NO_SEPARATOR\n");
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].reason, EnvParseReason::MissingEquals);
    }

    #[test]
    fn parse_accepts_export_prefix() {
        let result = parse_env_body("export FOO=bar\n");
        assert!(result.issues.is_empty());
        assert_eq!(result.entries[0].key, "FOO");
        assert_eq!(result.entries[0].value, "bar");
    }

    #[test]
    fn parse_rejects_invalid_key_chars() {
        let result = parse_env_body("FOO-BAR=1\n");
        assert_eq!(result.entries.len(), 0);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].reason, EnvParseReason::InvalidKeyChars);
    }

    #[test]
    fn parse_rejects_empty_key() {
        let result = parse_env_body("=value\n");
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].reason, EnvParseReason::EmptyKey);
    }

    #[test]
    fn render_includes_template_header_and_example() {
        let body = render_env_file(&[]);
        assert!(body.contains("Syntax: KEY=VALUE"));
        assert!(body.contains("Do not use YAML"));
        assert!(body.contains("CLAUDE_CODE_NO_FLICKER=1"));
        assert!(body.contains("https://code.claude.com/docs/en/fullscreen"));
    }

    #[test]
    fn render_preserves_user_entries_after_header() {
        let body = render_env_file(&[("FOO".into(), "1".into())]);
        assert!(body.contains("Syntax: KEY=VALUE"));
        assert!(body.ends_with("FOO=1\n"));
    }

    #[test]
    fn template_header_does_not_leak_active_examples_as_real_entries() {
        // The example MUST stay commented so a fresh init does not enable
        // CLAUDE_CODE_NO_FLICKER without explicit user opt-in.
        let parsed = parse_env_body(ENV_TEMPLATE_HEADER);
        assert!(
            parsed.entries.is_empty(),
            "template header must not export any value"
        );
        assert!(
            parsed.issues.is_empty(),
            "template header must be syntactically clean"
        );
    }

    #[test]
    fn merge_global_only() {
        let merged = merge_env_pairs(&[("A".into(), "1".into()), ("B".into(), "2".into())], &[]);
        assert_eq!(merged.global_count, 2);
        assert_eq!(merged.project_count, 0);
        assert_eq!(merged.override_count, 0);
        assert_eq!(merged.entries.len(), 2);
    }

    #[test]
    fn merge_project_overrides_global() {
        let merged = merge_env_pairs(
            &[("A".into(), "g".into()), ("B".into(), "g".into())],
            &[("B".into(), "p".into()), ("C".into(), "p".into())],
        );
        assert_eq!(merged.entries.len(), 3);
        let entry_b = merged.entries.iter().find(|e| e.key == "B").unwrap();
        assert_eq!(entry_b.value, "p", "project value must win");
        assert_eq!(entry_b.source, EnvSource::ProjectOverride);
        assert_eq!(merged.override_count, 1);
        assert_eq!(merged.project_count, 2);
        assert_eq!(merged.global_count, 1);
    }

    #[test]
    fn merge_dedupes_repeated_keys_in_same_source() {
        let merged = merge_env_pairs(&[("A".into(), "1".into()), ("A".into(), "2".into())], &[]);
        assert_eq!(merged.entries.len(), 1);
        assert_eq!(merged.entries[0].value, "2");
    }

    #[test]
    fn redact_value_returns_placeholder() {
        assert_eq!(redact_value("super-secret"), REDACTED_PLACEHOLDER);
    }

    #[test]
    fn is_secret_key_detects_common_secret_patterns() {
        for k in [
            "AWS_SECRET_ACCESS_KEY",
            "GITHUB_TOKEN",
            "MY_PASSWORD",
            "OPENAI_API_KEY",
            "AUTH_HEADER",
            "PRIVATE_NOTE",
        ] {
            assert!(
                is_secret_key(k),
                "expected key '{k}' to be classified secret"
            );
        }
    }

    #[test]
    fn is_secret_key_does_not_flag_obviously_safe_keys() {
        for k in ["LANG", "PATH", "USER", "TZ", "CLAUDE_CODE_NO_FLICKER"] {
            assert!(
                !is_secret_key(k),
                "'{k}' should not be flagged as a secret-like key"
            );
        }
    }

    // ---------------------------------------------------------------
    // resolve_effective_env: propagation of parse issues through the
    // ConfigPort boundary: the
    // CLI must receive (path, line, typed-reason) for every malformed
    // .env line, with no raw line and no value side.
    // ---------------------------------------------------------------

    use crate::errors::ConfigError;
    use crate::ports::{AppConfig, ConfigPort, EnvLineIssue, EnvLineIssueKind, EnvLoadReport};
    use std::cell::RefCell;

    /// Minimal in-memory ConfigPort for env-resolver tests. Only the
    /// `load_env_report` arm is meaningful — everything else falls
    /// back to either ConfigError or no-op behavior. Kept inside the
    /// test module so it cannot leak into production wiring.
    struct EnvIssueFakeConfig {
        global_report: RefCell<EnvLoadReport>,
        project_report: RefCell<EnvLoadReport>,
    }

    impl ConfigPort for EnvIssueFakeConfig {
        fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
            Ok(None)
        }
        fn save_global(&self, _config: &AppConfig) -> Result<(), ConfigError> {
            Ok(())
        }
        fn load_project(&self, _root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
            Ok(None)
        }
        fn save_project(&self, _root: &Utf8Path, _config: &AppConfig) -> Result<(), ConfigError> {
            Ok(())
        }
        fn global_exists(&self) -> Result<bool, ConfigError> {
            Ok(false)
        }
        fn project_exists(&self, _root: &Utf8Path) -> Result<bool, ConfigError> {
            Ok(false)
        }
        fn load_raw_toml(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
        ) -> Result<String, ConfigError> {
            Ok(String::new())
        }
        fn save_raw_toml(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
            _contents: &str,
        ) -> Result<(), ConfigError> {
            Ok(())
        }
        fn load_env(
            &self,
            _project_root: Option<&Utf8Path>,
            global: bool,
        ) -> Result<Vec<(String, String)>, ConfigError> {
            Ok(if global {
                self.global_report.borrow().entries.clone()
            } else {
                self.project_report.borrow().entries.clone()
            })
        }
        fn load_env_report(
            &self,
            _project_root: Option<&Utf8Path>,
            global: bool,
        ) -> Result<EnvLoadReport, ConfigError> {
            Ok(if global {
                self.global_report.borrow().clone()
            } else {
                self.project_report.borrow().clone()
            })
        }
        fn save_env(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
            _entries: &[(String, String)],
        ) -> Result<(), ConfigError> {
            Ok(())
        }
        fn global_config_dir(&self) -> Result<Utf8PathBuf, ConfigError> {
            Ok(Utf8PathBuf::from("/tmp/env-issue-fake"))
        }
    }

    #[test]
    fn resolve_effective_env_propagates_issues_with_source_path() {
        let global_path = Utf8PathBuf::from("/g/.env");
        let project_path = Utf8PathBuf::from("/p/.env");
        let global_report = EnvLoadReport {
            path: Some(global_path.clone()),
            entries: vec![("A".into(), "g".into())],
            issues: vec![EnvLineIssue {
                line: 7,
                kind: EnvLineIssueKind::LooksLikeYaml,
            }],
        };
        let project_report = EnvLoadReport {
            path: Some(project_path.clone()),
            entries: vec![("B".into(), "p".into())],
            issues: vec![
                EnvLineIssue {
                    line: 2,
                    kind: EnvLineIssueKind::MissingEquals,
                },
                EnvLineIssue {
                    line: 9,
                    kind: EnvLineIssueKind::InvalidKeyChars,
                },
            ],
        };
        let fake = EnvIssueFakeConfig {
            global_report: RefCell::new(global_report),
            project_report: RefCell::new(project_report),
        };

        let project_root = Utf8PathBuf::from("/p");
        let (effective, issues) =
            resolve_effective_env(&fake, Some(&project_root), None, None).unwrap();

        // Effective merge: global A + project B.
        assert_eq!(effective.entries.len(), 2);
        assert_eq!(
            effective.global_path.as_deref(),
            Some(global_path.as_path())
        );
        assert_eq!(
            effective.project_path.as_deref(),
            Some(project_path.as_path())
        );

        // Issues from BOTH overlays must be returned, tagged with the
        // resolved source path. The CLI uses these to render the
        // mandatory `.env at <path> line <n>: <reason>` warning. We
        // deliberately do NOT carry the raw line / value through this
        // boundary — only line + typed kind.
        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].source, global_path);
        assert_eq!(issues[0].issue.line, 7);
        assert_eq!(issues[0].issue.kind, EnvLineIssueKind::LooksLikeYaml);
        assert_eq!(issues[1].source, project_path);
        assert_eq!(issues[1].issue.line, 2);
        assert_eq!(issues[1].issue.kind, EnvLineIssueKind::MissingEquals);
        assert_eq!(issues[2].source, project_path);
        assert_eq!(issues[2].issue.line, 9);
        assert_eq!(issues[2].issue.kind, EnvLineIssueKind::InvalidKeyChars);
    }

    #[test]
    fn resolve_effective_env_skips_project_when_no_root() {
        let fake = EnvIssueFakeConfig {
            global_report: RefCell::new(EnvLoadReport {
                path: Some(Utf8PathBuf::from("/g/.env")),
                entries: vec![("A".into(), "g".into())],
                issues: vec![EnvLineIssue {
                    line: 1,
                    kind: EnvLineIssueKind::EmptyKey,
                }],
            }),
            project_report: RefCell::new(EnvLoadReport {
                path: Some(Utf8PathBuf::from("/p/.env")),
                entries: vec![("DO_NOT_LOAD".into(), "p".into())],
                issues: vec![EnvLineIssue {
                    line: 99,
                    kind: EnvLineIssueKind::MissingEquals,
                }],
            }),
        };
        let (effective, issues) = resolve_effective_env(&fake, None, None, None).unwrap();
        assert_eq!(effective.entries.len(), 1, "project must not be loaded");
        assert_eq!(issues.len(), 1, "only global issues when no project");
        assert_eq!(issues[0].issue.kind, EnvLineIssueKind::EmptyKey);
    }
}

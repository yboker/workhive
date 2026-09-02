//! TOML config + .env adapter for `ConfigPort`.
//!
//! Global config: `$XDG_CONFIG_HOME/workhive/config.toml` (or `~/.config/workhive/config.toml`)
//! Project config: `<project>/.workhive/config.toml` + `<project>/.workhive/.env`

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::ConfigError;
use workhive_core::ports::{AppConfig, ConfigPort, EnvLineIssue, EnvLineIssueKind, EnvLoadReport};
use workhive_core::usecases::env::EnvParseReason;

const CONFIG_FILE: &str = "config.toml";
const ENV_FILE: &str = ".env";

/// Translate a usecase-level `EnvParseReason` to the port-level
/// `EnvLineIssueKind` the CLI consumes for localized warnings.
///
/// The mapping is intentionally exhaustive (no fallback arm) so that
/// adding a new reason variant fails compilation here, forcing the i18n
/// keys + CLI mapping to stay in sync.
fn map_env_parse_reason(reason: EnvParseReason) -> EnvLineIssueKind {
    match reason {
        EnvParseReason::LooksLikeYaml => EnvLineIssueKind::LooksLikeYaml,
        EnvParseReason::MissingEquals => EnvLineIssueKind::MissingEquals,
        EnvParseReason::EmptyKey => EnvLineIssueKind::EmptyKey,
        EnvParseReason::InvalidKeyChars => EnvLineIssueKind::InvalidKeyChars,
    }
}

/// Resolve the global workhive config root directory.
///
/// Priority: `XDG_CONFIG_HOME/workhive` > `HOME/.config/workhive`.
/// All config reads, writes, and log directory computations MUST use this resolver.
pub fn resolve_global_config_dir() -> Result<Utf8PathBuf, ConfigError> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        let dir = Utf8PathBuf::from(xdg).join("workhive");
        tracing::debug!(
            xdg_config_home = %dir,
            source = "XDG_CONFIG_HOME",
            "resolved global config dir"
        );
        return Ok(dir);
    }

    let home = std::env::var("HOME").map_err(|_| ConfigError::GlobalNotInitialized)?;
    let dir = Utf8PathBuf::from(home).join(".config").join("workhive");
    tracing::debug!(
        global_config_dir = %dir,
        source = "HOME/.config",
        "resolved global config dir"
    );
    Ok(dir)
}

/// Real filesystem config adapter.
pub struct TomlConfigAdapter;

impl TomlConfigAdapter {
    pub fn new() -> Self {
        Self
    }

    fn global_dir() -> Result<Utf8PathBuf, ConfigError> {
        resolve_global_config_dir()
    }

    fn read_config(path: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        let exists = path.exists();
        if !exists {
            tracing::debug!(
                resolved_path = %path,
                exists = false,
                read_result = "missing",
                "config read"
            );
            return Ok(None);
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match toml::from_str::<AppConfig>(&content) {
                Ok(config) => {
                    config.validate().map_err(|e| {
                        tracing::warn!(
                            resolved_path = %path,
                            error = %e,
                            "config validation failed after load"
                        );
                        e
                    })?;
                    tracing::debug!(
                        resolved_path = %path,
                        exists = true,
                        read_result = "ok",
                        language = ?config.language,
                        has_tmux = config.tmux.is_some(),
                        has_layout = config.layout.is_some(),
                        layout_template = ?config.layout.as_ref().map(|l| l.template.to_string()),
                        layout_columns = config.layout.as_ref().map_or(0, |l| l.columns.len()),
                        has_snapshot = config.snapshot.is_some(),
                        has_naming = config.tmux.as_ref().and_then(|t| t.naming.as_ref()).is_some(),
                        "config read"
                    );
                    Ok(Some(config))
                }
                Err(e) => {
                    tracing::warn!(
                        resolved_path = %path,
                        exists = true,
                        read_result = "parse_error",
                        error = %e,
                        "config read"
                    );
                    Err(ConfigError::ParseFailed {
                        path: path.to_owned(),
                        message: e.to_string(),
                    })
                }
            },
            Err(source) => {
                let read_result = if source.kind() == std::io::ErrorKind::PermissionDenied {
                    "permission_error"
                } else {
                    "read_error"
                };
                tracing::warn!(
                    resolved_path = %path,
                    exists = true,
                    read_result = read_result,
                    error = %source,
                    "config read"
                );
                Err(ConfigError::ReadFailed {
                    path: path.to_owned(),
                    source,
                })
            }
        }
    }

    /// Create `.env` file if it does not exist (idempotent).
    ///
    /// New files are seeded with the self-documenting template from
    /// [`workhive_core::usecases::env::ENV_TEMPLATE_HEADER`] so users
    /// immediately see the `KEY=VALUE` syntax contract and the commented
    /// `CLAUDE_CODE_NO_FLICKER` example.
    fn ensure_env_file(path: &Utf8Path) -> Result<(), ConfigError> {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| ConfigError::WriteFailed {
                    path: path.to_owned(),
                    source,
                })?;
            }
            let template = workhive_core::usecases::env::ENV_TEMPLATE_HEADER;
            std::fs::write(path, template).map_err(|source| ConfigError::WriteFailed {
                path: path.to_owned(),
                source,
            })?;
            tracing::debug!(path = %path, ".env file created from template");
        }
        Ok(())
    }

    fn env_path(project_root: Option<&Utf8Path>, global: bool) -> Result<Utf8PathBuf, ConfigError> {
        if global {
            Ok(Self::global_dir()?.join(ENV_FILE))
        } else if let Some(root) = project_root {
            Ok(root.join(".workhive").join(ENV_FILE))
        } else {
            Ok(Self::global_dir()?.join(ENV_FILE))
        }
    }

    fn read_env_file(path: &Utf8Path) -> Result<Vec<(String, String)>, ConfigError> {
        Ok(Self::read_env_file_report(path)?.entries)
    }

    /// Read a `.env` file and return both the parsed entries and the
    /// list of parser issues that should be surfaced to the user.
    ///
    /// Implements the file-side half of the `.env` diagnostics
    /// contract: invalid `.env` lines must produce localized, user-visible
    /// warnings during `workspace open` (path + line + typed reason)
    /// rather than only landing in `tracing` logs. We still emit the
    /// `tracing::warn!` so structured log surfaces stay informed, but
    /// the `EnvLoadReport` we return is what the CLI uses to render
    /// the human-readable warning. We deliberately never expose
    /// `issue.raw` to the CLI — the raw line can include the value side
    /// of a malformed entry (e.g. `API_KEY: secret`), and the active
    /// doc bans surfacing values to any user-facing surface.
    fn read_env_file_report(path: &Utf8Path) -> Result<EnvLoadReport, ConfigError> {
        if !path.exists() {
            return Ok(EnvLoadReport::default());
        }
        let content = std::fs::read_to_string(path).map_err(|source| ConfigError::ReadFailed {
            path: path.to_owned(),
            source,
        })?;
        let parsed = workhive_core::usecases::env::parse_env_body(&content);
        if !parsed.issues.is_empty() {
            for issue in &parsed.issues {
                tracing::warn!(
                    path = %path,
                    line = issue.line,
                    reason = issue.reason.description(),
                    "invalid .env line ignored"
                );
            }
        }
        let key_names: Vec<&str> = parsed.entries.iter().map(|e| e.key.as_str()).collect();
        tracing::debug!(
            path = %path,
            entry_count = parsed.entries.len(),
            issue_count = parsed.issues.len(),
            keys = ?key_names,
            ".env file parsed"
        );
        let entries = parsed.pairs();
        let issues = parsed
            .issues
            .iter()
            .map(|i| EnvLineIssue {
                line: i.line,
                kind: map_env_parse_reason(i.reason),
            })
            .collect();
        Ok(EnvLoadReport {
            path: Some(path.to_owned()),
            entries,
            issues,
        })
    }

    fn write_env_file(path: &Utf8Path, entries: &[(String, String)]) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::WriteFailed {
                path: path.to_owned(),
                source,
            })?;
        }
        let content = workhive_core::usecases::env::render_env_file(entries);
        std::fs::write(path, &content).map_err(|source| ConfigError::WriteFailed {
            path: path.to_owned(),
            source,
        })?;
        let key_names: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        tracing::debug!(
            path = %path,
            entries = entries.len(),
            keys = ?key_names,
            ".env file written"
        );
        Ok(())
    }

    /// Atomic raw write: write `contents` to a sibling temp file in the
    /// same directory and then rename it into place. On a same-filesystem
    /// rename is atomic, so readers never observe a torn or partially
    /// written file. This is the shared crash-safety primitive that both
    /// typed (`write_config`) and raw (`save_raw_toml`) writers funnel
    /// through.
    ///
    /// The caller owns:
    /// - validation (typed schemas) and serialization (e.g. via toml_edit);
    /// - parent-directory creation (kept here for symmetry with the legacy
    ///   `write_config` contract).
    ///
    /// All error paths return `ConfigError::WriteFailed` carrying the
    /// final target path so the user sees an actionable diagnostic.
    fn write_raw_config(path: &Utf8Path, contents: &str) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ConfigError::WriteFailed {
                path: path.to_owned(),
                source,
            })?;
        }

        let tmp_path = Utf8PathBuf::from(format!("{}.tmp", path));
        std::fs::write(&tmp_path, contents).map_err(|source| {
            tracing::warn!(
                tmp_path = %tmp_path,
                target_path = %path,
                error = %source,
                "failed to write temp config file"
            );
            ConfigError::WriteFailed {
                path: path.to_owned(),
                source,
            }
        })?;

        std::fs::rename(&tmp_path, path).map_err(|source| {
            let _ = std::fs::remove_file(&tmp_path);
            tracing::warn!(
                tmp_path = %tmp_path,
                target_path = %path,
                error = %source,
                "atomic rename failed, temp file cleaned up"
            );
            ConfigError::WriteFailed {
                path: path.to_owned(),
                source,
            }
        })?;

        Ok(())
    }

    /// Atomic typed-config write: serialize via `render_user_facing_toml`,
    /// then funnel through [`write_raw_config`] for the temp-file +
    /// rename dance.
    fn write_config(path: &Utf8Path, config: &AppConfig) -> Result<(), ConfigError> {
        config.validate()?;
        let content = render_user_facing_toml(path, config)?;
        Self::write_raw_config(path, &content)?;

        tracing::info!(
            target_path = %path,
            language = ?config.language,
            has_tmux = config.tmux.is_some(),
            has_layout = config.layout.is_some(),
            layout_template = ?config.layout.as_ref().map(|l| l.template.to_string()),
            layout_columns = config.layout.as_ref().map_or(0, |l| l.columns.len()),
            has_snapshot = config.snapshot.is_some(),
            has_naming = config.tmux.as_ref().and_then(|t| t.naming.as_ref()).is_some(),
            "config written (atomic rename)"
        );
        Ok(())
    }
}

fn render_user_facing_toml(path: &Utf8Path, config: &AppConfig) -> Result<String, ConfigError> {
    let raw = toml::to_string_pretty(config).map_err(|e| ConfigError::ParseFailed {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    let mut doc = raw
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| ConfigError::ParseFailed {
            path: path.to_owned(),
            message: e.to_string(),
        })?;

    if let Some(file_cfg) = config.logging.as_ref().and_then(|log| log.file.as_ref()) {
        if let Some(rotation) = file_cfg.rotation {
            doc["logging"]["file"]["rotation"] = toml_edit::value(rotation.to_string());
        }
        if let Some(retention) = file_cfg.retention {
            doc["logging"]["file"]["retention"] = toml_edit::value(retention.to_string());
        }
    }

    let mut rendered = doc.to_string();
    inject_config_comments(&mut rendered);
    Ok(rendered)
}

fn inject_config_comments(content: &mut String) {
    insert_comment_block_before_prefix(
        content,
        "language = ",
        &[
            "# User interface language. Supported values: en, zh-CN.",
            "# If omitted in project config, the global language is used.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[tmux]\n",
        &[
            "# Runtime backend settings for tmux.",
            "# server_name is the legacy fallback server name; naming templates are preferred.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[tmux.naming]\n",
        &[
            "# Naming templates for runtime server and session identifiers.",
            "# Available placeholders include {instance_id}, {project}, {workspace}, and {branch}.",
            "# Server templates must stay project-scoped; session templates may include workspace.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[layout]\n",
        &[
            "# Pane layout used when no snapshot is restored.",
            "# template is informational; columns are the source of truth.",
            "# Template values include ai_coding_left, ai_coding_dual, half_split, quad, single, custom.",
        ],
    );
    insert_comment_block_before_prefix(
        content,
        "[[layout.columns]]",
        &["# Layout columns. Width values are relative percentages across the window."],
    );
    insert_comment_block_before_prefix(
        content,
        "[[layout.columns.panes]]",
        &[
            "# Panes inside a column, ordered from top to bottom.",
            "# height is the percentage within its column.",
            "# launch describes what starts in the pane: EmptyShell, Tool, or InlineCommand.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[snapshot]\n",
        &[
            "# Snapshot persistence controls whether workspace state can be restored.",
            "# enabled = true allows close/open flows to save and restore pane state.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[logging.console]\n",
        &[
            "# Enable console logging.",
            "# Level values: trace, debug, info, warn, error.",
            "# Format values: compact, json.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[logging.file]\n",
        &[
            "# Enable file logging. File logging failures must not block CLI execution.",
            "# Level values: trace, debug, info, warn, error.",
            "# Format values: compact, json.",
            "# compact: human-readable single-line format. json: machine-parseable structured format.",
            "# Scope values: global or project.",
            "# Base name must end with .log.",
            "# Rotation values: never, hourly, daily, size_mb(N).",
            "# Retention values: unlimited, max_files(N).",
            "# Retention applies to rotated files only and is ignored when rotation = \"never\".",
        ],
    );
    insert_comment_block_before_prefix(
        content,
        "[tools.catalog",
        &[
            "# Global tool catalog. Project layouts reference these tools by name.",
            "# Edit command.program and command.args here to change startup commands globally.",
            "# kind values: agent, tui, cli.",
            "# enabled = false hides the tool from init-time command selection.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[projects]\n",
        &[
            "# Trusted project root directories \u{2014} the canonical safety/discovery boundary.",
            "# Workhive may create .workhive/, worktrees, and runtime state only under these roots.",
            "# Values must be absolute paths or start with ~/.",
            "# Manage with: workhive project roots add|remove|list|status.",
        ],
    );
    insert_comment_block_after_header(
        content,
        "[discovery]\n",
        &[
            "# Legacy alias for trusted project roots. Prefer [projects].trusted_roots above.",
            "# When both are set, [projects].trusted_roots wins.",
            "# Values must be absolute paths or start with ~/.",
        ],
    );
    insert_comment_block_before_prefix(
        content,
        "worktree_root = ",
        &["# Optional project-local override for linked worktree storage."],
    );
    insert_comment_block_before_prefix(
        content,
        "snapshot_root = ",
        &["# Optional project-local override for snapshot storage."],
    );
    insert_comment_block_before_prefix(
        content,
        "runtime_root = ",
        &["# Optional project-local override for runtime metadata storage."],
    );
}

fn insert_comment_block_after_header(content: &mut String, header: &str, comments: &[&str]) {
    if let Some(start) = content.find(header) {
        let insert_at = start + header.len();
        let block = comments
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        content.insert_str(insert_at, &block);
    }
}

fn insert_comment_block_before_prefix(content: &mut String, prefix: &str, comments: &[&str]) {
    if let Some(start) = content.find(prefix) {
        let block = comments
            .iter()
            .map(|line| format!("{line}\n"))
            .collect::<String>();
        content.insert_str(start, &block);
    }
}

impl Default for TomlConfigAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigPort for TomlConfigAdapter {
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
        let dir = Self::global_dir()?;
        let path = dir.join(CONFIG_FILE);
        Self::read_config(&path)
    }

    fn save_global(&self, config: &AppConfig) -> Result<(), ConfigError> {
        let dir = Self::global_dir()?;
        let path = dir.join(CONFIG_FILE);
        tracing::info!(command = "save_global", target_path = %path, reason = "global config save");
        Self::write_config(&path, config)?;
        Self::ensure_env_file(&dir.join(ENV_FILE))?;
        Ok(())
    }

    fn load_project(&self, project_root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        let path = project_root.join(".workhive").join(CONFIG_FILE);
        Self::read_config(&path)
    }

    fn save_project(&self, project_root: &Utf8Path, config: &AppConfig) -> Result<(), ConfigError> {
        let workhive_dir = project_root.join(".workhive");
        let path = workhive_dir.join(CONFIG_FILE);
        tracing::info!(command = "save_project", target_path = %path, reason = "project config save");
        Self::write_config(&path, config)?;
        Self::ensure_env_file(&workhive_dir.join(ENV_FILE))?;
        Ok(())
    }

    fn global_exists(&self) -> Result<bool, ConfigError> {
        let dir = Self::global_dir()?;
        Ok(dir.join(CONFIG_FILE).exists())
    }

    fn project_exists(&self, project_root: &Utf8Path) -> Result<bool, ConfigError> {
        Ok(project_root.join(".workhive").join(CONFIG_FILE).exists())
    }

    fn load_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<String, ConfigError> {
        let path = if global {
            Self::global_dir()?.join(CONFIG_FILE)
        } else if let Some(root) = project_root {
            root.join(".workhive").join(CONFIG_FILE)
        } else {
            Self::global_dir()?.join(CONFIG_FILE)
        };
        if !path.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(&path).map_err(|source| ConfigError::ReadFailed {
            path: path.to_owned(),
            source,
        })
    }

    fn save_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        contents: &str,
    ) -> Result<(), ConfigError> {
        let path = if global {
            Self::global_dir()?.join(CONFIG_FILE)
        } else if let Some(root) = project_root {
            root.join(".workhive").join(CONFIG_FILE)
        } else {
            Self::global_dir()?.join(CONFIG_FILE)
        };
        // Raw-TOML mutation must be as crash-safe as typed
        // `write_config`. Funnel through the shared atomic
        // temp-file + rename primitive so a partially-written
        // `config.toml` is impossible. We do NOT parse, reserialize, or
        // otherwise touch `contents` — `toml_edit`-preserved comments
        // and formatting must round-trip byte-for-byte.
        tracing::info!(
            command = "save_raw_toml",
            target_path = %path,
            reason = "raw TOML mutation (toml_edit-preserved, atomic rename)",
        );
        Self::write_raw_config(&path, contents)
    }

    fn load_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<Vec<(String, String)>, ConfigError> {
        let path = Self::env_path(project_root, global)?;
        Self::read_env_file(&path)
    }

    fn load_env_report(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<EnvLoadReport, ConfigError> {
        let path = Self::env_path(project_root, global)?;
        Self::read_env_file_report(&path)
    }

    fn save_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        entries: &[(String, String)],
    ) -> Result<(), ConfigError> {
        let path = Self::env_path(project_root, global)?;
        Self::write_env_file(&path, entries)
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

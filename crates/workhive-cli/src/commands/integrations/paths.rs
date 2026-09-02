//! Resolution of the *official* tool config file paths each
//! provider's CLI loads, plus the Workhive-owned script directory.
//!
//! Every resolver here respects the upstream tool's documented env
//! var override (so isolated integration tests can point each tool
//! at a tempdir without touching `$HOME`). Falling back to `$HOME`
//! is per each tool's own docs:
//!
//! - Claude Code: `$CLAUDE_CONFIG_DIR/settings.json` else
//!   `$HOME/.claude/settings.json` (see
//!   <https://code.claude.com/docs/en/settings>).
//! - Codex: `$CODEX_HOME/config.toml` else
//!   `$HOME/.codex/config.toml` (see
//!   <https://developers.openai.com/codex/config>).
//! - OpenCode plugins: `$XDG_CONFIG_HOME/opencode/plugins/` else
//!   `$HOME/.config/opencode/plugins/` (see
//!   <https://opencode.ai/docs/plugins/>); files in this directory
//!   are auto-loaded by the OpenCode CLI on startup. Note: the
//!   directory name is plural `plugins/` (matches the official docs);
//!   the previous round used singular `plugin/`, which the OpenCode
//!   CLI does not load.

use camino::Utf8PathBuf;

use super::error::{IntegrationError, path_err};

fn home_dir() -> Result<Utf8PathBuf, IntegrationError> {
    std::env::var("HOME").map(Utf8PathBuf::from).map_err(|_| {
        path_err(
            "$HOME is not set",
            std::io::Error::new(std::io::ErrorKind::NotFound, "HOME"),
        )
    })
}

/// `~/.claude/settings.json`, with `$CLAUDE_CONFIG_DIR` override.
pub(super) fn claude_settings_path() -> Result<Utf8PathBuf, IntegrationError> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR")
        && !dir.is_empty()
    {
        return Ok(Utf8PathBuf::from(dir).join("settings.json"));
    }
    Ok(home_dir()?.join(".claude").join("settings.json"))
}

/// `~/.codex/config.toml`, with `$CODEX_HOME` override.
pub(super) fn codex_config_path() -> Result<Utf8PathBuf, IntegrationError> {
    if let Ok(dir) = std::env::var("CODEX_HOME")
        && !dir.is_empty()
    {
        return Ok(Utf8PathBuf::from(dir).join("config.toml"));
    }
    Ok(home_dir()?.join(".codex").join("config.toml"))
}

/// `~/.config/opencode/plugins/workhive.mjs`, with
/// `$XDG_CONFIG_HOME` override. OpenCode auto-discovers every
/// `.js`/`.mjs`/`.ts` file in this directory at startup, so dropping
/// the Workhive plugin file here is the install — no additional
/// config key needs to be merged.
///
/// The directory name is the plural `plugins/` per the official
/// docs at <https://opencode.ai/docs/plugins/>; the singular
/// `plugin/` form is not auto-loaded by the OpenCode CLI.
pub(super) fn opencode_plugin_path() -> Result<Utf8PathBuf, IntegrationError> {
    let base = if let Ok(dir) = std::env::var("XDG_CONFIG_HOME")
        && !dir.is_empty()
    {
        Utf8PathBuf::from(dir)
    } else {
        home_dir()?.join(".config")
    };
    Ok(base.join("opencode").join("plugins").join("workhive.mjs"))
}

/// `<workhive_global_config>/integrations/<provider>/` directory
/// where Workhive's own hook scripts live. The official tool config
/// files reference scripts by absolute path inside this directory.
pub(super) fn workhive_scripts_dir(provider_wire: &str) -> Result<Utf8PathBuf, IntegrationError> {
    let base = workhive_infra::config::resolve_global_config_dir().map_err(|e| {
        path_err(
            "resolve global config dir",
            std::io::Error::other(e.to_string()),
        )
    })?;
    Ok(base.join("integrations").join(provider_wire))
}

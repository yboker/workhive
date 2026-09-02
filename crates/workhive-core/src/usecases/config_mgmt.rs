//! `config` use case — show, set, unset configuration values.

use camino::Utf8Path;

use crate::errors::ConfigError;
use crate::ports::{AppConfig, ConfigPort, LayoutConfig, TmuxConfig};

/// Load the effective config (project merged over global, or global alone).
pub fn show_config(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global_only: bool,
) -> Result<AppConfig, ConfigError> {
    if global_only {
        Ok(config_port.load_global()?.unwrap_or_default())
    } else if let Some(root) = project_root {
        let global = config_port.load_global()?.unwrap_or_default();
        let project = config_port.load_project(root)?.unwrap_or_default();
        Ok(merge_configs(&global, &project))
    } else {
        Ok(config_port.load_global()?.unwrap_or_default())
    }
}

/// Set a dot-separated key to a value in the config file.
pub fn set_config_value(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
    key: &str,
    value: &str,
) -> Result<(), ConfigError> {
    let mut config = load_target(config_port, project_root, global)?;
    apply_set(&mut config, key, value)?;
    save_target(config_port, project_root, global, &config)
}

/// Remove a dot-separated key from the config file.
pub fn unset_config_value(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
    key: &str,
) -> Result<(), ConfigError> {
    let mut config = load_target(config_port, project_root, global)?;
    apply_unset(&mut config, key)?;
    save_target(config_port, project_root, global, &config)
}

fn load_target(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
) -> Result<AppConfig, ConfigError> {
    if global {
        Ok(config_port.load_global()?.unwrap_or_default())
    } else if let Some(root) = project_root {
        Ok(config_port.load_project(root)?.unwrap_or_default())
    } else {
        Ok(config_port.load_global()?.unwrap_or_default())
    }
}

fn save_target(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
    config: &AppConfig,
) -> Result<(), ConfigError> {
    if global {
        config_port.save_global(config)
    } else if let Some(root) = project_root {
        config_port.save_project(root, config)
    } else {
        config_port.save_global(config)
    }
}

/// Apply a set operation to a config in memory (for preview, no persistence).
pub fn apply_set_preview(
    config: &mut AppConfig,
    key: &str,
    value: &str,
) -> Result<(), ConfigError> {
    apply_set(config, key, value)
}

/// Apply an unset operation to a config in memory (for preview, no persistence).
pub fn apply_unset_preview(config: &mut AppConfig, key: &str) -> Result<(), ConfigError> {
    apply_unset(config, key)
}

fn apply_set(config: &mut AppConfig, key: &str, value: &str) -> Result<(), ConfigError> {
    match key {
        "language" => {
            let locale = crate::domain::Locale::from_str_loose(value).ok_or_else(|| {
                ConfigError::InvalidValue {
                    field: "language".to_owned(),
                    reason: format!("unsupported language '{value}'; expected 'en' or 'zh-cn'"),
                }
            })?;
            config.language = Some(locale);
        }
        "tmux.server_name" => {
            config.tmux.get_or_insert_with(Default::default).server_name = value.to_owned();
        }
        "tmux.naming.server_template" => {
            let tmux = config.tmux.get_or_insert_with(Default::default);
            let naming = tmux.naming.get_or_insert(crate::ports::NamingSettings {
                server_template: None,
                session_template: None,
                sanitize: None,
                max_component_len: None,
            });
            naming.server_template = Some(value.to_owned());
            naming.validate()?;
        }
        "tmux.naming.session_template" => {
            let tmux = config.tmux.get_or_insert_with(Default::default);
            let naming = tmux.naming.get_or_insert(crate::ports::NamingSettings {
                server_template: None,
                session_template: None,
                sanitize: None,
                max_component_len: None,
            });
            naming.session_template = Some(value.to_owned());
            naming.validate()?;
        }
        "worktree_root" => {
            config.worktree_root = Some(camino::Utf8PathBuf::from(value));
        }
        "snapshot_root" => {
            config.snapshot_root = Some(camino::Utf8PathBuf::from(value));
        }
        "runtime_root" => {
            config.runtime_root = Some(camino::Utf8PathBuf::from(value));
        }
        "logging.console.enabled" => {
            let enabled = parse_bool_value("logging.console.enabled", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .console
                .get_or_insert_with(Default::default)
                .enabled = Some(enabled);
        }
        "logging.console.level" => {
            let level = parse_log_level("logging.console.level", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .console
                .get_or_insert_with(Default::default)
                .level = Some(level);
        }
        "logging.console.format" => {
            let format = parse_log_format("logging.console.format", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .console
                .get_or_insert_with(Default::default)
                .format = Some(format);
        }
        "logging.file.enabled" => {
            let enabled = parse_bool_value("logging.file.enabled", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .enabled = Some(enabled);
        }
        "logging.file.level" => {
            let level = parse_log_level("logging.file.level", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .level = Some(level);
        }
        "logging.file.format" => {
            let format = parse_log_format("logging.file.format", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .format = Some(format);
        }
        "logging.file.scope" => {
            let scope = parse_file_log_scope("logging.file.scope", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .scope = Some(scope);
        }
        "logging.file.base_name" => {
            if !value.ends_with(".log") {
                return Err(ConfigError::InvalidValue {
                    field: "logging.file.base_name".to_owned(),
                    reason: "must end with .log".to_owned(),
                });
            }
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .base_name = Some(value.to_owned());
        }
        "logging.file.directory" => {
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .directory = Some(camino::Utf8PathBuf::from(value));
        }
        "logging.file.rotation" => {
            let rotation = parse_log_rotation("logging.file.rotation", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .rotation = Some(rotation);
        }
        "logging.file.retention" => {
            let retention = parse_log_retention("logging.file.retention", value)?;
            config
                .logging
                .get_or_insert_with(Default::default)
                .file
                .get_or_insert_with(Default::default)
                .retention = Some(retention);
        }
        "pane.exit.tool_exit_behavior" => {
            let behavior = crate::ports::ManagedToolExitBehavior::from_str_value(value)
                .ok_or_else(|| ConfigError::InvalidValue {
                    field: "pane.exit.tool_exit_behavior".to_owned(),
                    reason: format!(
                        "'{value}' is not a valid managed-tool exit behavior (expected one of: respawn_shell, close_pane)"
                    ),
                })?;
            config
                .pane
                .get_or_insert_with(Default::default)
                .exit
                .get_or_insert_with(Default::default)
                .tool_exit_behavior = Some(behavior);
        }
        _ => {
            return Err(ConfigError::ParseFailed {
                path: camino::Utf8PathBuf::from("<config>"),
                message: format!("unknown config key: {key}"),
            });
        }
    }
    tracing::info!(key, value, "config value set");
    Ok(())
}

fn apply_unset(config: &mut AppConfig, key: &str) -> Result<(), ConfigError> {
    match key {
        "language" => config.language = None,
        "tmux.server_name" => {
            if let Some(tmux) = &mut config.tmux {
                tmux.server_name = TmuxConfig::default().server_name;
            }
        }
        "tmux.naming.server_template" => {
            if let Some(tmux) = &mut config.tmux
                && let Some(naming) = &mut tmux.naming
            {
                naming.server_template = None;
            }
        }
        "tmux.naming.session_template" => {
            if let Some(tmux) = &mut config.tmux
                && let Some(naming) = &mut tmux.naming
            {
                naming.session_template = None;
            }
        }
        "worktree_root" => config.worktree_root = None,
        "snapshot_root" => config.snapshot_root = None,
        "runtime_root" => config.runtime_root = None,
        "logging.console.enabled" => {
            if let Some(logging) = &mut config.logging
                && let Some(console) = &mut logging.console
            {
                console.enabled = None;
            }
        }
        "logging.console.level" => {
            if let Some(logging) = &mut config.logging
                && let Some(console) = &mut logging.console
            {
                console.level = None;
            }
        }
        "logging.console.format" => {
            if let Some(logging) = &mut config.logging
                && let Some(console) = &mut logging.console
            {
                console.format = None;
            }
        }
        "logging.file.enabled" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.enabled = None;
            }
        }
        "logging.file.level" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.level = None;
            }
        }
        "logging.file.format" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.format = None;
            }
        }
        "logging.file.scope" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.scope = None;
            }
        }
        "logging.file.base_name" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.base_name = None;
            }
        }
        "logging.file.directory" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.directory = None;
            }
        }
        "logging.file.rotation" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.rotation = None;
            }
        }
        "logging.file.retention" => {
            if let Some(logging) = &mut config.logging
                && let Some(file) = &mut logging.file
            {
                file.retention = None;
            }
        }
        "logging.console" => {
            if let Some(logging) = &mut config.logging {
                logging.console = None;
            }
        }
        "logging.file" => {
            if let Some(logging) = &mut config.logging {
                logging.file = None;
            }
        }
        "pane.exit.tool_exit_behavior" => {
            if let Some(pane) = &mut config.pane
                && let Some(exit) = &mut pane.exit
            {
                exit.tool_exit_behavior = None;
            }
        }
        "pane.exit" => {
            if let Some(pane) = &mut config.pane {
                pane.exit = None;
            }
        }
        _ => {
            return Err(ConfigError::ParseFailed {
                path: camino::Utf8PathBuf::from("<config>"),
                message: format!("unknown config key: {key}"),
            });
        }
    }
    tracing::info!(key, "config value unset");
    Ok(())
}

fn parse_bool_value(field: &str, value: &str) -> Result<bool, ConfigError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ConfigError::InvalidValue {
            field: field.to_owned(),
            reason: format!("expected 'true' or 'false', got '{value}'"),
        }),
    }
}

fn parse_log_level(field: &str, value: &str) -> Result<crate::ports::LogLevel, ConfigError> {
    match value {
        "error" => Ok(crate::ports::LogLevel::Error),
        "warn" => Ok(crate::ports::LogLevel::Warn),
        "info" => Ok(crate::ports::LogLevel::Info),
        "debug" => Ok(crate::ports::LogLevel::Debug),
        "trace" => Ok(crate::ports::LogLevel::Trace),
        _ => Err(ConfigError::InvalidValue {
            field: field.to_owned(),
            reason: format!(
                "'{value}' is not a valid log level (expected one of: error, warn, info, debug, trace)"
            ),
        }),
    }
}

fn parse_log_format(field: &str, value: &str) -> Result<crate::ports::LogFormat, ConfigError> {
    match value {
        "compact" => Ok(crate::ports::LogFormat::Compact),
        "json" => Ok(crate::ports::LogFormat::Json),
        _ => Err(ConfigError::InvalidValue {
            field: field.to_owned(),
            reason: format!("'{value}' is not a valid log format (expected one of: compact, json)"),
        }),
    }
}

fn parse_file_log_scope(
    field: &str,
    value: &str,
) -> Result<crate::ports::FileLogScope, ConfigError> {
    match value {
        "global" => Ok(crate::ports::FileLogScope::Global),
        "project" => Ok(crate::ports::FileLogScope::Project),
        _ => Err(ConfigError::InvalidValue {
            field: field.to_owned(),
            reason: format!(
                "'{value}' is not a valid file scope (expected one of: global, project)"
            ),
        }),
    }
}

fn parse_log_rotation(field: &str, value: &str) -> Result<crate::ports::LogRotation, ConfigError> {
    match value {
        "never" => Ok(crate::ports::LogRotation::Never),
        "hourly" => Ok(crate::ports::LogRotation::Hourly),
        "daily" => Ok(crate::ports::LogRotation::Daily),
        _ => {
            if let Some(stripped) = value
                .strip_prefix("size_mb(")
                .and_then(|s| s.strip_suffix(')'))
            {
                let mb: u32 = stripped.parse().map_err(|_| ConfigError::InvalidValue {
                    field: field.to_owned(),
                    reason: format!("invalid size_mb value: '{stripped}'"),
                })?;
                let rotation = crate::ports::LogRotation::SizeMb(mb);
                rotation.validate(field)?;
                Ok(rotation)
            } else {
                Err(ConfigError::InvalidValue {
                    field: field.to_owned(),
                    reason: format!(
                        "'{value}' is not a valid rotation \
                         (expected one of: never, hourly, daily, size_mb(N))"
                    ),
                })
            }
        }
    }
}

fn parse_log_retention(
    field: &str,
    value: &str,
) -> Result<crate::ports::LogRetention, ConfigError> {
    match value {
        "unlimited" => Ok(crate::ports::LogRetention::Unlimited),
        _ => {
            if let Some(stripped) = value
                .strip_prefix("max_files(")
                .and_then(|s| s.strip_suffix(')'))
            {
                let n: u32 = stripped.parse().map_err(|_| ConfigError::InvalidValue {
                    field: field.to_owned(),
                    reason: format!("invalid max_files value: '{stripped}'"),
                })?;
                let retention = crate::ports::LogRetention::MaxFiles(n);
                retention.validate(field)?;
                Ok(retention)
            } else {
                Err(ConfigError::InvalidValue {
                    field: field.to_owned(),
                    reason: format!(
                        "'{value}' is not a valid retention \
                         (expected one of: unlimited, max_files(N))"
                    ),
                })
            }
        }
    }
}

/// Set an environment variable in the `.env` file.
///
/// Logging policy: `tracing` events MUST NOT include the raw value. We log
/// only the key name, the scope (global vs project), and whether this was a
/// create or update. The redaction policy lives in
/// [`crate::usecases::env::redact_value`] so any future relaxation must go
/// through that one seam.
pub fn env_set(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
    key: &str,
    value: &str,
) -> Result<(), ConfigError> {
    let mut entries = config_port.load_env(project_root, global)?;
    let was_update = entries.iter().any(|(k, _)| k == key);
    if let Some(existing) = entries.iter_mut().find(|(k, _)| k == key) {
        existing.1 = value.to_owned();
    } else {
        entries.push((key.to_owned(), value.to_owned()));
    }
    config_port.save_env(project_root, global, &entries)?;
    tracing::info!(
        key,
        scope = if global { "global" } else { "project" },
        action = if was_update { "update" } else { "create" },
        value = crate::usecases::env::REDACTED_PLACEHOLDER,
        "env variable set"
    );
    Ok(())
}

/// Remove an environment variable from the `.env` file.
///
/// Logging policy: only the key name and scope are emitted; values are
/// never logged. See [`env_set`].
pub fn env_unset(
    config_port: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
    global: bool,
    key: &str,
) -> Result<(), ConfigError> {
    let mut entries = config_port.load_env(project_root, global)?;
    let before = entries.len();
    entries.retain(|(k, _)| k != key);
    if entries.len() == before {
        tracing::debug!(key, "env key not found, nothing to remove");
    }
    config_port.save_env(project_root, global, &entries)?;
    tracing::info!(
        key,
        scope = if global { "global" } else { "project" },
        "env variable unset"
    );
    Ok(())
}

/// Overlay wizard-produced config onto an existing config.
///
/// The wizard only manages a subset of fields (language, tmux, layout, snapshot).
/// Fields the wizard does not touch (discovery, worktree_root, snapshot_root,
/// runtime_root) are preserved from the existing config so that a `config edit`
/// cycle never silently drops unrelated settings.
pub fn overlay_wizard_config(existing: &AppConfig, wizard: &AppConfig) -> AppConfig {
    AppConfig {
        language: wizard.language.or(existing.language),
        tmux: wizard.tmux.clone().or(existing.tmux.clone()),
        layout: wizard.layout.clone().or(existing.layout.clone()),
        snapshot: wizard.snapshot.clone().or(existing.snapshot.clone()),
        logging: wizard.logging.clone().or(existing.logging.clone()),
        projects: wizard.projects.clone().or(existing.projects.clone()),
        discovery: wizard.discovery.clone().or(existing.discovery.clone()),
        tools: existing.tools.clone().or(wizard.tools.clone()),
        worktree_root: wizard
            .worktree_root
            .clone()
            .or(existing.worktree_root.clone()),
        snapshot_root: wizard
            .snapshot_root
            .clone()
            .or(existing.snapshot_root.clone()),
        runtime_root: wizard
            .runtime_root
            .clone()
            .or(existing.runtime_root.clone()),
        features: crate::ports::FeaturesConfig::merge(
            existing.features.as_ref(),
            wizard.features.as_ref(),
        ),
        pane: crate::ports::PaneRuntimeConfig::merge(existing.pane.as_ref(), wizard.pane.as_ref()),
    }
}

/// Resolve the effective layout using config precedence: project > global > default.
///
/// This is a pure transformation — no I/O. The caller (CLI) is responsible
/// for loading configs via `ConfigPort` and passing in the typed values.
pub fn resolve_effective_layout(
    project_config: Option<&AppConfig>,
    global_config: Option<&AppConfig>,
) -> LayoutConfig {
    let project_layout = project_config.and_then(|c| c.layout.clone());
    let global_layout = global_config.and_then(|c| c.layout.clone());
    project_layout.or(global_layout).unwrap_or_default()
}

/// Public re-export of the project-over-global config merge, for
/// CLI surfaces that need a single resolved `AppConfig` (e.g. the
/// AI integration feature gate in pane rendering). Mirrors the
/// precedence used by every other config consumer in this module.
pub fn resolve_effective_config(global: &AppConfig, project: &AppConfig) -> AppConfig {
    merge_configs(global, project)
}

/// Resolve the effective managed-tool exit behavior using config
/// precedence: project > global > built-in default (`respawn_shell`).
///
/// Pure — no I/O. Callers (CLI, supervisor exit path) pass already-loaded
/// configs and receive a typed, non-`Option` value. This keeps the
/// pane lifecycle policy decision a single typed call instead of an
/// `if-let` chain at the call site.
pub fn resolve_effective_tool_exit_behavior(
    project_config: Option<&AppConfig>,
    global_config: Option<&AppConfig>,
) -> crate::ports::ManagedToolExitBehavior {
    let project_pane = project_config.and_then(|c| c.pane.as_ref());
    let global_pane = global_config.and_then(|c| c.pane.as_ref());
    crate::ports::PaneRuntimeConfig::merge(global_pane, project_pane)
        .and_then(|p| p.exit)
        .unwrap_or_default()
        .resolved_tool_exit_behavior()
}

fn merge_configs(global: &AppConfig, project: &AppConfig) -> AppConfig {
    use crate::ports::LogConfig;

    let logging = match (&global.logging, &project.logging) {
        (Some(g), Some(p)) => Some(LogConfig::merge(g, p)),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), None) => Some(g.clone()),
        (None, None) => None,
    };

    AppConfig {
        language: project.language.or(global.language),
        tmux: project.tmux.clone().or(global.tmux.clone()),
        layout: project.layout.clone().or(global.layout.clone()),
        snapshot: project.snapshot.clone().or(global.snapshot.clone()),
        logging,
        projects: project.projects.clone().or(global.projects.clone()),
        discovery: project.discovery.clone().or(global.discovery.clone()),
        tools: project.tools.clone().or(global.tools.clone()),
        worktree_root: project
            .worktree_root
            .clone()
            .or(global.worktree_root.clone()),
        snapshot_root: project
            .snapshot_root
            .clone()
            .or(global.snapshot_root.clone()),
        runtime_root: project.runtime_root.clone().or(global.runtime_root.clone()),
        features: crate::ports::FeaturesConfig::merge(
            global.features.as_ref(),
            project.features.as_ref(),
        ),
        pane: crate::ports::PaneRuntimeConfig::merge(global.pane.as_ref(), project.pane.as_ref()),
    }
}

#[cfg(test)]
#[path = "config_mgmt_tests.rs"]
mod tests;

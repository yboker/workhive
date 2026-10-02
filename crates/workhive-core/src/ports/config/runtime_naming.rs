#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// Current schema version of the `[tmux]` config table.
///
/// - v1: each project ran its own tmux server (`whsrv-{instance_id}-{project}`).
/// - v2: all projects share the default tmux socket; isolation comes from
///   globally unique session names.
pub const TMUX_CONFIG_SCHEMA_VERSION: u32 = 2;

/// The v1 built-in default server template. Retained only to recognize v1
/// configs that never overrode the default, so they can be upgraded silently
/// while an author's own template is preserved.
const V1_DEFAULT_SERVER_TEMPLATE: &str = "whsrv-{instance_id}-{project}";

pub const DEFAULT_HERDR_SESSION: &str = "workhive";

/// Herdr-specific native session configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HerdrConfig {
    /// Native Herdr session used for newly opened workspaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl HerdrConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.session.as_deref().is_some_and(|session| {
            session.trim().is_empty()
                || matches!(session, "." | "..")
                || session
                    .chars()
                    .any(|ch| matches!(ch, '/' | '\\') || ch.is_control())
        }) {
            return Err(ConfigError::InvalidValue {
                field: "herdr.session".to_owned(),
                reason:
                    "must be a non-empty session name without path separators or control characters"
                        .to_owned(),
            });
        }
        Ok(())
    }
}

/// Tmux-specific configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TmuxConfig {
    /// Schema version of this table. Absent means the table predates
    /// versioning, i.e. v1. See [`TMUX_CONFIG_SCHEMA_VERSION`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,

    /// Name of the dedicated tmux server. `None` = inherit or use
    /// built-in default `"default"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// Session naming configuration. `None` = use built-in defaults.
    #[serde(default)]
    pub naming: Option<NamingSettings>,

    /// Enable tmux mouse support. `None` = inherit from parent config
    /// or built-in default (`true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mouse: Option<bool>,

    /// Enable clipboard integration (OSC 52, vi copy-mode bindings).
    /// `None` = inherit from parent config or built-in default (`true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipboard: Option<bool>,
}

impl TmuxConfig {
    /// The built-in default server name.
    pub fn default_server_name() -> String {
        "default".to_owned()
    }

    /// Resolved server name: explicit value or built-in default.
    pub fn resolved_server_name(&self) -> String {
        self.server_name
            .clone()
            .unwrap_or_else(Self::default_server_name)
    }

    /// Build a `NamingConfig` from the stored settings (or defaults).
    ///
    /// Partial settings inherit from `NamingConfig::default()` (@ convention).
    /// Tables older than [`TMUX_CONFIG_SCHEMA_VERSION`] are migrated in memory;
    /// the stored file is left untouched until it is next written.
    pub fn naming_config(&self) -> crate::domain::NamingConfig {
        let defaults = crate::domain::NamingConfig::default();
        let mut config = match &self.naming {
            Some(s) => crate::domain::NamingConfig {
                server_template: s
                    .server_template
                    .clone()
                    .unwrap_or_else(|| defaults.server_template.clone()),
                session_template: s
                    .session_template
                    .clone()
                    .unwrap_or_else(|| defaults.session_template.clone()),
                sanitize: s.sanitize.unwrap_or(defaults.sanitize),
                max_component_len: s
                    .max_component_len
                    .map(|v| v as usize)
                    .unwrap_or(defaults.max_component_len),
            },
            None => defaults.clone(),
        };
        self.migrate_naming(&mut config, &defaults);
        config
    }

    /// In-memory schema upgrades for tables written by older versions.
    ///
    /// v1 → v2 moved every project onto the shared default socket. A v1 table
    /// that still carries the v1 built-in template is upgraded; a template the
    /// author chose themselves is kept so their intent is never discarded.
    fn migrate_naming(
        &self,
        config: &mut crate::domain::NamingConfig,
        defaults: &crate::domain::NamingConfig,
    ) {
        let version = self.schema_version.unwrap_or(1);
        if version >= TMUX_CONFIG_SCHEMA_VERSION {
            return;
        }
        if config.server_template == V1_DEFAULT_SERVER_TEMPLATE {
            tracing::info!(
                from_version = version,
                to_version = TMUX_CONFIG_SCHEMA_VERSION,
                server_template = %defaults.server_template,
                "migrated per-project tmux server template to the shared socket"
            );
            config.server_template = defaults.server_template.clone();
        }
    }

    /// Field-level merge: project fields override global fields
    /// independently. `None` in the project config inherits the global
    /// value rather than replacing the entire section.
    pub fn merge(global: &TmuxConfig, project: &TmuxConfig) -> TmuxConfig {
        TmuxConfig {
            // A table without a version is v1, so the older half wins and the
            // merged result stays migratable.
            schema_version: match (global.schema_version, project.schema_version) {
                (Some(g), Some(p)) => Some(g.min(p)),
                _ => None,
            },
            server_name: project.server_name.clone().or(global.server_name.clone()),
            naming: NamingSettings::merge_option(global.naming.as_ref(), project.naming.as_ref()),
            mouse: project.mouse.or(global.mouse),
            clipboard: project.clipboard.or(global.clipboard),
        }
    }

    /// Resolve `mouse` to a non-optional boolean for runtime consumption.
    /// Built-in default is `true`.
    pub fn resolved_mouse(&self) -> bool {
        self.mouse.unwrap_or(true)
    }

    /// Resolve `clipboard` to a non-optional boolean for runtime consumption.
    /// Built-in default is `true`.
    pub fn resolved_clipboard(&self) -> bool {
        self.clipboard.unwrap_or(true)
    }
}

/// Configurable session naming templates.
///
/// All fields use `Option<T>` so partial config inherits defaults.
/// Default conventions (see the session naming convention):
/// - server: `"default"` (the shared tmux socket)
/// - session: `"wh@{instance_id}@{project}@{workspace}"` (parseable with `@`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingSettings {
    /// Server name template, e.g., `"default"`.
    pub server_template: Option<String>,
    /// Session name template, e.g., `"wh@{instance_id}@{project}@{workspace}"`.
    pub session_template: Option<String>,
    /// Whether to sanitize names for tmux compatibility.
    pub sanitize: Option<bool>,
    /// Maximum length for each template component.
    pub max_component_len: Option<u32>,
}

/// Required placeholders for server name templates.
const SERVER_TEMPLATE_REQUIRED: &[&str] = &["{instance_id}", "{project}"];
/// Placeholders forbidden in server templates (servers are project-scoped).
const SERVER_TEMPLATE_FORBIDDEN: &[&str] = &["{workspace}", "{branch}"];
/// Required placeholders for session name templates.
const SESSION_TEMPLATE_REQUIRED: &[&str] = &["{instance_id}", "{project}", "{workspace}"];

impl NamingSettings {
    /// Field-level merge: project fields override global fields,
    /// omitted project fields inherit the global value.
    pub fn merge_option(
        global: Option<&NamingSettings>,
        project: Option<&NamingSettings>,
    ) -> Option<NamingSettings> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(NamingSettings {
                server_template: p.server_template.clone().or(g.server_template.clone()),
                session_template: p.session_template.clone().or(g.session_template.clone()),
                sanitize: p.sanitize.or(g.sanitize),
                max_component_len: p.max_component_len.or(g.max_component_len),
            }),
        }
    }

    /// Validate naming templates contain required placeholders and
    /// do not contain forbidden ones.
    ///
    /// This is the **single source of truth** for template semantic
    /// constraints. Both config-file loading and `config set` CLI
    /// must funnel through this function.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref tmpl) = self.server_template {
            // A server template may be a literal socket name (all projects share
            // one server). Once it interpolates anything, it must be complete and
            // project-scoped.
            let required: &[&str] = if tmpl.contains('{') {
                SERVER_TEMPLATE_REQUIRED
            } else {
                &[]
            };
            validate_template(
                "tmux.naming.server_template",
                tmpl,
                required,
                SERVER_TEMPLATE_FORBIDDEN,
            )?;
        }
        if let Some(ref tmpl) = self.session_template {
            validate_template(
                "tmux.naming.session_template",
                tmpl,
                SESSION_TEMPLATE_REQUIRED,
                &[],
            )?;
        }
        Ok(())
    }
}

fn validate_template(
    field: &str,
    template: &str,
    required: &[&str],
    forbidden: &[&str],
) -> Result<(), crate::errors::ConfigError> {
    if template.is_empty() {
        return Err(crate::errors::ConfigError::InvalidValue {
            field: field.to_owned(),
            reason: "template must not be empty".to_owned(),
        });
    }
    for placeholder in required {
        if !template.contains(placeholder) {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: format!(
                    "template '{}' is missing required placeholder '{}'",
                    template, placeholder
                ),
            });
        }
    }
    for placeholder in forbidden {
        if template.contains(placeholder) {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: format!(
                    "template '{}' must not contain '{}' — servers are project-scoped. \
                     Use session_template for workspace/branch differentiation.",
                    template, placeholder
                ),
            });
        }
    }
    Ok(())
}

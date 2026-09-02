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

/// Tmux-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxConfig {
    /// Name of the dedicated tmux server.
    #[serde(default = "TmuxConfig::default_server_name")]
    pub server_name: String,

    /// Session naming configuration. `None` = use built-in defaults.
    #[serde(default)]
    pub naming: Option<NamingSettings>,
}

impl TmuxConfig {
    fn default_server_name() -> String {
        "whsrv-main".to_owned()
    }

    /// Build a `NamingConfig` from the stored settings (or defaults).
    ///
    /// Partial settings inherit from `NamingConfig::default()` (@ convention).
    pub fn naming_config(&self) -> crate::domain::NamingConfig {
        let defaults = crate::domain::NamingConfig::default();
        match &self.naming {
            Some(s) => crate::domain::NamingConfig {
                server_template: s
                    .server_template
                    .clone()
                    .unwrap_or(defaults.server_template),
                session_template: s
                    .session_template
                    .clone()
                    .unwrap_or(defaults.session_template),
                sanitize: s.sanitize.unwrap_or(defaults.sanitize),
                max_component_len: s
                    .max_component_len
                    .map(|v| v as usize)
                    .unwrap_or(defaults.max_component_len),
            },
            None => defaults,
        }
    }
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            server_name: Self::default_server_name(),
            naming: None,
        }
    }
}

/// Configurable session naming templates.
///
/// All fields use `Option<T>` so partial config inherits defaults.
/// Default conventions (see the session naming convention):
/// - server: `"whsrv-{instance_id}-{project}"` (filesystem-safe for tmux sockets)
/// - session: `"wh@{instance_id}@{project}@{workspace}"` (parseable with `@`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamingSettings {
    /// Server name template, e.g., `"whsrv-{instance_id}-{project}"`.
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
    /// Validate naming templates contain required placeholders and
    /// do not contain forbidden ones.
    ///
    /// This is the **single source of truth** for template semantic
    /// constraints. Both config-file loading and `config set` CLI
    /// must funnel through this function.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref tmpl) = self.server_template {
            validate_template(
                "tmux.naming.server_template",
                tmpl,
                SERVER_TEMPLATE_REQUIRED,
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

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

/// Legacy project discovery settings (global config only).
///
/// Kept for backward compatibility with older `[discovery]` TOML sections.
/// The canonical store is now `[projects] trusted_roots` — see
/// [`ProjectsConfig`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Legacy alias for trusted project roots.
    ///
    /// Treated as trusted project roots for runtime purposes. New code paths
    /// should write to `[projects] trusted_roots` instead; this field is
    /// preserved verbatim during a `toml_edit` rewrite to avoid clobbering
    /// hand-edited configs.
    #[serde(default)]
    pub project_roots: Vec<Utf8PathBuf>,
}

impl DiscoveryConfig {
    /// Validate that project roots are non-empty absolute-looking paths.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        validate_root_list("discovery.project_roots", &self.project_roots)
    }
}

/// Trusted project directories and project-level safety settings (global config only).
///
/// `trusted_roots` is the safety boundary for any operation that mutates
/// `.workhive/`, creates worktrees, or writes runtime state. It is also
/// the default discovery source used by `workhive project reindex`,
/// `workhive project list`, and `workhive session list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectsConfig {
    /// Allowed parent directories for Workhive-managed projects.
    ///
    /// An entry counts as "trusted" iff the project root equals the entry or
    /// is a strict descendant of it after canonical normalization. String
    /// prefix matching is explicitly NOT used (`/foo/dev` does not cover
    /// `/foo/dev2`).
    #[serde(default)]
    pub trusted_roots: Vec<Utf8PathBuf>,
}

impl ProjectsConfig {
    /// Validate that trusted roots are non-empty absolute-looking paths.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        validate_root_list("projects.trusted_roots", &self.trusted_roots)
    }
}

fn validate_root_list(
    field: &str,
    roots: &[Utf8PathBuf],
) -> Result<(), crate::errors::ConfigError> {
    for root in roots {
        if root.as_str().is_empty() {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: "trusted root path must not be empty".to_owned(),
            });
        }
        if !root.as_str().starts_with('/') && !root.as_str().starts_with('~') {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: format!(
                    "trusted root '{}' must be an absolute path (starting with / or ~)",
                    root
                ),
            });
        }
    }
    Ok(())
}

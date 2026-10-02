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

/// Abstraction over configuration storage.
///
/// Supports both global (`~/.config/workhive/`) and project (`.workhive/`) configurations.
pub trait ConfigPort {
    /// Load the global configuration, if it exists.
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError>;

    /// Save the global configuration.
    fn save_global(&self, config: &AppConfig) -> Result<(), ConfigError>;

    /// Load a project configuration from the given path.
    fn load_project(&self, project_root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError>;

    /// Save a project configuration.
    fn save_project(&self, project_root: &Utf8Path, config: &AppConfig) -> Result<(), ConfigError>;

    /// Check whether the global configuration has been initialized.
    fn global_exists(&self) -> Result<bool, ConfigError>;

    /// Check whether a project configuration exists at the given root.
    fn project_exists(&self, project_root: &Utf8Path) -> Result<bool, ConfigError>;

    /// Load the raw TOML string from the config file (preserving formatting).
    fn load_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<String, ConfigError>;

    /// Persist a raw TOML string back to the config file, replacing its contents.
    ///
    /// Used by callers that mutate the on-disk document via `toml_edit` (which
    /// preserves comments and formatting) instead of round-tripping through
    /// the strongly-typed `AppConfig`. The file is created with default
    /// scaffolding if it does not yet exist.
    fn save_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        contents: &str,
    ) -> Result<(), ConfigError>;

    /// Load the `.env` file content as key-value pairs.
    /// `global = true` reads global `.env`, otherwise project `.env`.
    fn load_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<Vec<(String, String)>, ConfigError>;

    /// Load the `.env` file along with its parser issues so the CLI
    /// can surface localized, user-visible warnings for malformed
    /// lines.
    ///
    /// The default implementation delegates to [`ConfigPort::load_env`]
    /// and returns `issues = []` / `path = None`. The real
    /// `TomlConfigAdapter` overrides this to thread `parse_env_body`'s
    /// `EnvParseIssue` list through to the user. Test fakes can opt
    /// into the richer surface by overriding too, but most fakes do
    /// not need to.
    fn load_env_report(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<EnvLoadReport, ConfigError> {
        let entries = self.load_env(project_root, global)?;
        Ok(EnvLoadReport {
            path: None,
            entries,
            issues: Vec::new(),
        })
    }

    /// Save key-value pairs to the `.env` file.
    fn save_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        entries: &[(String, String)],
    ) -> Result<(), ConfigError>;

    /// Resolve the global config directory path.
    ///
    /// Returns `~/.config/workhive/` (via `XDG_CONFIG_HOME` or platform
    /// default) on success, or a typed `ConfigError` on resolution failure.
    fn global_config_dir(&self) -> Result<Utf8PathBuf, ConfigError>;
}

impl<T: ConfigPort + ?Sized> ConfigPort for Box<T> {
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
        (**self).load_global()
    }
    fn save_global(&self, config: &AppConfig) -> Result<(), ConfigError> {
        (**self).save_global(config)
    }
    fn load_project(&self, project_root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        (**self).load_project(project_root)
    }
    fn save_project(&self, project_root: &Utf8Path, config: &AppConfig) -> Result<(), ConfigError> {
        (**self).save_project(project_root, config)
    }
    fn global_exists(&self) -> Result<bool, ConfigError> {
        (**self).global_exists()
    }
    fn project_exists(&self, project_root: &Utf8Path) -> Result<bool, ConfigError> {
        (**self).project_exists(project_root)
    }
    fn load_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<String, ConfigError> {
        (**self).load_raw_toml(project_root, global)
    }
    fn save_raw_toml(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        contents: &str,
    ) -> Result<(), ConfigError> {
        (**self).save_raw_toml(project_root, global, contents)
    }
    fn load_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<Vec<(String, String)>, ConfigError> {
        (**self).load_env(project_root, global)
    }
    fn load_env_report(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<EnvLoadReport, ConfigError> {
        (**self).load_env_report(project_root, global)
    }
    fn save_env(
        &self,
        project_root: Option<&Utf8Path>,
        global: bool,
        entries: &[(String, String)],
    ) -> Result<(), ConfigError> {
        (**self).save_env(project_root, global, entries)
    }
    fn global_config_dir(&self) -> Result<Utf8PathBuf, ConfigError> {
        (**self).global_config_dir()
    }
}

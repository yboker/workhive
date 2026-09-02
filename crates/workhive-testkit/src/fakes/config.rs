//! Fake config adapter for testing.

use std::cell::RefCell;

use camino::Utf8Path;
use workhive_core::errors::ConfigError;
use workhive_core::ports::{AppConfig, ConfigPort};

/// In-memory fake for `ConfigPort`.
pub struct FakeConfig {
    global: RefCell<Option<AppConfig>>,
    project: RefCell<Option<AppConfig>>,
    global_env: RefCell<Vec<(String, String)>>,
    project_env: RefCell<Vec<(String, String)>>,
    load_global_error: RefCell<Option<ConfigError>>,
    load_project_error: RefCell<Option<ConfigError>>,
    save_project_error: RefCell<Option<ConfigError>>,
    /// Raw TOML mirror updated by `save_raw_toml`; preserves comments and
    /// formatting for tests that exercise toml_edit-based mutation.
    global_raw: RefCell<Option<String>>,
    project_raw: RefCell<Option<String>>,
}

impl FakeConfig {
    pub fn new() -> Self {
        Self {
            global: RefCell::new(None),
            project: RefCell::new(None),
            global_env: RefCell::new(Vec::new()),
            project_env: RefCell::new(Vec::new()),
            load_global_error: RefCell::new(None),
            load_project_error: RefCell::new(None),
            save_project_error: RefCell::new(None),
            global_raw: RefCell::new(None),
            project_raw: RefCell::new(None),
        }
    }

    /// Pre-load a global config for testing.
    pub fn with_global(self, config: AppConfig) -> Self {
        *self.global.borrow_mut() = Some(config);
        self
    }

    /// Pre-load a project config for testing.
    pub fn with_project(self, config: AppConfig) -> Self {
        *self.project.borrow_mut() = Some(config);
        self
    }

    /// Inject an error that `load_global()` will return once.
    pub fn with_global_error(self, err: ConfigError) -> Self {
        *self.load_global_error.borrow_mut() = Some(err);
        self
    }

    /// Inject an error that `load_project()` will return once.
    pub fn with_project_error(self, err: ConfigError) -> Self {
        *self.load_project_error.borrow_mut() = Some(err);
        self
    }

    /// Inject an error that `save_project()` will return once.
    pub fn set_save_project_error(&self, err: ConfigError) {
        *self.save_project_error.borrow_mut() = Some(err);
    }
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigPort for FakeConfig {
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
        if let Some(err) = self.load_global_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(self.global.borrow().clone())
    }

    fn save_global(&self, config: &AppConfig) -> Result<(), ConfigError> {
        *self.global.borrow_mut() = Some(config.clone());
        Ok(())
    }

    fn load_project(&self, _project_root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        if let Some(err) = self.load_project_error.borrow_mut().take() {
            return Err(err);
        }
        Ok(self.project.borrow().clone())
    }

    fn save_project(
        &self,
        _project_root: &Utf8Path,
        config: &AppConfig,
    ) -> Result<(), ConfigError> {
        if let Some(err) = self.save_project_error.borrow_mut().take() {
            return Err(err);
        }
        *self.project.borrow_mut() = Some(config.clone());
        Ok(())
    }

    fn global_exists(&self) -> Result<bool, ConfigError> {
        Ok(self.global.borrow().is_some())
    }

    fn project_exists(&self, _project_root: &Utf8Path) -> Result<bool, ConfigError> {
        Ok(self.project.borrow().is_some())
    }

    fn load_raw_toml(
        &self,
        _project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<String, ConfigError> {
        let raw = if global {
            self.global_raw.borrow().clone()
        } else {
            self.project_raw.borrow().clone()
        };
        if let Some(body) = raw {
            return Ok(body);
        }
        let config = if global {
            self.global.borrow().clone()
        } else {
            self.project.borrow().clone()
        };
        match config {
            Some(c) => Ok(toml::to_string_pretty(&c).unwrap_or_default()),
            None => Ok(String::new()),
        }
    }

    fn save_raw_toml(
        &self,
        _project_root: Option<&Utf8Path>,
        global: bool,
        contents: &str,
    ) -> Result<(), ConfigError> {
        if global {
            *self.global_raw.borrow_mut() = Some(contents.to_owned());
            // Best-effort re-deserialize so subsequent `load_global` reflects
            // the mutation; keep the strongly-typed copy in sync where it is
            // valid TOML.
            if let Ok(cfg) = toml::from_str::<AppConfig>(contents) {
                *self.global.borrow_mut() = Some(cfg);
            }
        } else {
            *self.project_raw.borrow_mut() = Some(contents.to_owned());
            if let Ok(cfg) = toml::from_str::<AppConfig>(contents) {
                *self.project.borrow_mut() = Some(cfg);
            }
        }
        Ok(())
    }

    fn load_env(
        &self,
        _project_root: Option<&Utf8Path>,
        global: bool,
    ) -> Result<Vec<(String, String)>, ConfigError> {
        if global {
            Ok(self.global_env.borrow().clone())
        } else {
            Ok(self.project_env.borrow().clone())
        }
    }

    fn save_env(
        &self,
        _project_root: Option<&Utf8Path>,
        global: bool,
        entries: &[(String, String)],
    ) -> Result<(), ConfigError> {
        if global {
            *self.global_env.borrow_mut() = entries.to_vec();
        } else {
            *self.project_env.borrow_mut() = entries.to_vec();
        }
        Ok(())
    }
}

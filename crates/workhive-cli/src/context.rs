//! CommandContext — shared dependencies for all command handlers.
//!
//! Eliminates per-handler parameter sprawl and removes unused/placeholder
//! parameters (e.g., `let _ = config`).

use crate::composition::CliComposition;
use workhive_core::domain::{Locale, RuntimeBackendId, SessionNamingPolicy};
use workhive_core::errors::ConfigError;
use workhive_core::ports::{ConfigPort, DependencyPort, ProcessLauncher};
use workhive_infra::i18n::StaticI18n;
use workhive_infra::pane_registry::InMemoryPaneRegistry;

/// Shared dependencies available to all command handlers.
pub struct CommandContext {
    pub composition: CliComposition,
    pub config: Box<dyn ConfigPort>,
    pub deps: Box<dyn DependencyPort>,
    pub i18n: StaticI18n,
    pub pane_registry: InMemoryPaneRegistry,
    /// Whether the user passed the global `-v` / `--verbose` flag.
    ///
    /// Renderers that present a "concise by default + detailed on
    /// request" surface (e.g. `pane list`) read this flag instead of
    /// taking a separate parameter. The flag is plumbed from
    /// `Cli::verbose` so the legacy logging-verbosity meaning is
    /// preserved while letting human renderers opt into expanded
    /// output (column legends, full hook/plugin paragraphs).
    pub verbose: bool,
    /// Background reconciliation launcher boundary.
    pub(crate) process_launcher: Box<dyn ProcessLauncher>,
}

/// Records launch calls without spawning processes.
#[cfg(test)]
pub(crate) struct FakeProcessLauncher {
    inner: std::rc::Rc<FakeProcessLauncherInner>,
}

#[cfg(test)]
struct FakeProcessLauncherInner {
    launch_reconciliation_calls: std::cell::RefCell<Vec<camino::Utf8PathBuf>>,
    fail_next: std::cell::Cell<bool>,
}

#[cfg(test)]
impl Clone for FakeProcessLauncher {
    fn clone(&self) -> Self {
        Self {
            inner: std::rc::Rc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
impl FakeProcessLauncher {
    pub fn new() -> Self {
        Self {
            inner: std::rc::Rc::new(FakeProcessLauncherInner {
                launch_reconciliation_calls: std::cell::RefCell::new(Vec::new()),
                fail_next: std::cell::Cell::new(false),
            }),
        }
    }

    pub fn launch_reconciliation_call_count(&self) -> usize {
        self.inner.launch_reconciliation_calls.borrow().len()
    }

    pub fn set_fail_next(&self, fail: bool) {
        self.inner.fail_next.set(fail);
    }
}

#[cfg(test)]
impl ProcessLauncher for FakeProcessLauncher {
    fn launch_reconciliation(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<(), workhive_core::errors::LaunchError> {
        if self.inner.fail_next.get() {
            self.inner.fail_next.set(false);
            return Err(workhive_core::errors::LaunchError::SpawnFailed(
                std::io::Error::other("fake: reconciliation spawn failed"),
            ));
        }
        self.inner
            .launch_reconciliation_calls
            .borrow_mut()
            .push(config_dir.to_owned());
        Ok(())
    }
}

impl CommandContext {
    pub fn new() -> Self {
        Self::with_verbose(false)
    }

    /// Build a context with the given global verbose flag.
    ///
    /// Used by `main.rs` to forward `Cli::verbose`. Tests can call
    /// `CommandContext::new()` (verbose defaults to false) or
    /// `CommandContext::with_verbose(true)` when they need to exercise
    /// the detailed render path.
    pub fn with_verbose(verbose: bool) -> Self {
        let composition = CliComposition::new();
        Self {
            config: Box::new(composition.config()),
            deps: Box::new(composition.dependencies()),
            i18n: composition.i18n(),
            pane_registry: composition.pane_registry(),
            composition,
            verbose,
            process_launcher: Box::new(workhive_infra::process_launcher::OsProcessLauncher),
        }
    }

    /// Build a test context with injected config and launcher boundaries.
    #[cfg(test)]
    pub(crate) fn with_test_boundaries(
        config: Box<dyn ConfigPort>,
        launcher: FakeProcessLauncher,
    ) -> Self {
        let composition = CliComposition::new();
        Self {
            config,
            deps: Box::new(composition.dependencies()),
            i18n: composition.i18n(),
            pane_registry: composition.pane_registry(),
            composition,
            verbose: false,
            process_launcher: Box::new(launcher),
        }
    }

    fn load_layered_config(
        &self,
        project_root: Option<&camino::Utf8Path>,
    ) -> Result<
        (
            Option<workhive_core::ports::AppConfig>,
            Option<workhive_core::ports::AppConfig>,
        ),
        ConfigError,
    > {
        let global = self.config.load_global()?;
        let project = match project_root {
            Some(root) => self.config.load_project(root)?,
            None => None,
        };
        Ok((project, global))
    }

    /// Resolve the user's locale from global config, falling back to English.
    ///
    /// Logs a warning if config read fails so invalid configuration is not silent.
    pub fn locale(&self) -> Locale {
        match self.config.load_global() {
            Ok(Some(c)) => c.language.unwrap_or(Locale::En),
            Ok(None) => Locale::En,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    fallback = "en",
                    "failed to load global config for locale, falling back to English"
                );
                Locale::En
            }
        }
    }

    /// Resolve project > global runtime backend, defaulting to tmux when no
    /// scope selects a backend.
    pub fn runtime_backend_for(
        &self,
        project_root: Option<&camino::Utf8Path>,
    ) -> Result<RuntimeBackendId, ConfigError> {
        let (project, global) = self.load_layered_config(project_root)?;
        Ok(
            workhive_core::usecases::config_mgmt::resolve_effective_backend(
                project.as_ref(),
                global.as_ref(),
            ),
        )
    }

    /// Resolve the configured Herdr session using project > global > default precedence.
    pub fn herdr_session_for(
        &self,
        project_root: Option<&camino::Utf8Path>,
    ) -> Result<String, ConfigError> {
        let (project, global) = self.load_layered_config(project_root)?;
        Ok(
            workhive_core::usecases::config_mgmt::resolve_effective_herdr_session(
                project.as_ref(),
                global.as_ref(),
            ),
        )
    }

    /// Build a `SessionNamingPolicy` from the effective config.
    ///
    /// Priority: project config > global config > defaults.
    /// Reads project config if a project root is provided.
    ///
    /// Returns `Err` when a config file **exists but is invalid/unreadable**.
    /// "Config does not exist" (`Ok(None)`) is fine and falls through to defaults.
    pub fn naming_policy_for(
        &self,
        project_root: Option<&camino::Utf8Path>,
    ) -> Result<SessionNamingPolicy, ConfigError> {
        let global_naming = match self.config.load_global() {
            Ok(Some(c)) => c.tmux.map(|t| t.naming_config()),
            Ok(None) => None,
            Err(e) => return Err(e),
        };

        let project_naming = match project_root {
            Some(root) => match self.config.load_project(root) {
                Ok(Some(c)) => c.tmux.map(|t| t.naming_config()),
                Ok(None) => None,
                Err(e) => return Err(e),
            },
            None => None,
        };

        let config = project_naming.or(global_naming).unwrap_or_default();
        Ok(SessionNamingPolicy::new(config))
    }
}

//! CommandContext — shared dependencies for all command handlers.
//!
//! Eliminates per-handler parameter sprawl and removes unused/placeholder
//! parameters (e.g., `let _ = config`).

use crate::composition::CliComposition;
use workhive_core::domain::{Locale, SessionNamingPolicy};
use workhive_core::errors::ConfigError;
use workhive_core::ports::{ConfigPort, DependencyPort};
use workhive_infra::config::TomlConfigAdapter;
use workhive_infra::i18n::StaticI18n;
use workhive_infra::pane_registry::InMemoryPaneRegistry;

/// Shared dependencies available to all command handlers.
pub struct CommandContext {
    pub composition: CliComposition,
    pub config: TomlConfigAdapter,
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
            config: composition.config(),
            deps: Box::new(composition.dependencies()),
            i18n: composition.i18n(),
            pane_registry: composition.pane_registry(),
            composition,
            verbose,
        }
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

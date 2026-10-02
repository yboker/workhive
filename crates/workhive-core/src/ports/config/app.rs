#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, RuntimeBackendId, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// The application configuration model.
///
/// Represents the merged result of global + project config.
/// All fields use `Option` to distinguish "explicitly set" from "inherited default".
/// When merging, `None` means "not explicitly set, inherit from global".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    /// Runtime backend selected at this config scope. `None` inherits the
    /// built-in tmux default for global config and the global selection for projects.
    pub backend: Option<RuntimeBackendId>,

    /// Herdr native runtime settings.
    pub herdr: Option<HerdrConfig>,

    /// User-selected language.
    pub language: Option<Locale>,

    /// Tmux-related settings. `None` = inherit from global.
    pub tmux: Option<TmuxConfig>,

    /// Default pane layout template. `None` = inherit from global.
    pub layout: Option<LayoutConfig>,

    /// Logging settings. `None` = use defaults (console: warn, file: debug).
    pub logging: Option<LogConfig>,

    /// Trusted project directories and project-level safety settings (global config only).
    ///
    /// The canonical home for "trusted project roots" — directories where
    /// Workhive is allowed to create `.workhive/`, materialize worktrees,
    /// and persist runtime state.
    pub projects: Option<ProjectsConfig>,

    /// Legacy project discovery settings (global config only).
    ///
    /// Deprecated in favor of [`AppConfig::projects`]. When both are present
    /// `projects.trusted_roots` is authoritative; when only `discovery` is
    /// present the runtime treats `discovery.project_roots` as trusted roots
    /// for backward compatibility. See [`AppConfig::trusted_roots`].
    pub discovery: Option<DiscoveryConfig>,

    /// Tool catalog configuration. `None` = inherit from global / builtin defaults.
    ///
    /// The catalog defines reusable tools (claude, codex, lazygit, …) that
    /// layouts can reference via `Tool(ToolName)`.  Users can override builtin
    /// entries, add custom tools, or disable specific tools.
    pub tools: Option<ToolCatalogConfig>,

    /// Root directory for git worktrees (project-level only).
    pub worktree_root: Option<Utf8PathBuf>,
    /// Root directory for runtime state (project-level only).
    pub runtime_root: Option<Utf8PathBuf>,

    /// First-class capability gates. Currently models only
    /// `features.ai_integrations`; future gated capabilities live
    /// under the same `[features]` table so the surface stays
    /// discoverable in one place.
    ///
    /// `None` means "no `[features]` table was authored" and built-in
    /// defaults apply (see [`FeaturesConfig::effective`]).
    pub features: Option<FeaturesConfig>,

    /// Top-level `[pane]` runtime behavior. Currently models the
    /// managed tool exit policy (`pane.exit.tool_exit_behavior`).
    ///
    /// Distinct from the layout-scoped `PaneConfig` (which describes
    /// height + launch inside `[[layout.columns.panes]]`): this table
    /// configures how Workhive-managed panes behave at runtime, not
    /// what they launch.
    ///
    /// `None` means built-in defaults apply (`respawn_shell`).
    pub pane: Option<PaneRuntimeConfig>,
}

impl AppConfig {
    /// Get tmux config with fallback to defaults.
    pub fn tmux_or_default(&self) -> TmuxConfig {
        self.tmux.clone().unwrap_or_default()
    }

    /// Get layout config with fallback to defaults.
    pub fn layout_or_default(&self) -> LayoutConfig {
        self.layout.clone().unwrap_or_default()
    }

    /// Resolve the canonical trusted-project-root list from this config.
    ///
    /// Precedence:
    /// 1. `projects.trusted_roots` (canonical, written by current versions).
    /// 2. `discovery.project_roots` (legacy alias, kept for backward compat).
    ///
    /// The returned slice is empty when no trusted roots are configured.
    /// Callers should NOT mutate two storages — when persisting changes the
    /// CLI funnels through `[projects] trusted_roots`.
    pub fn trusted_roots(&self) -> Vec<Utf8PathBuf> {
        if let Some(p) = self.projects.as_ref()
            && !p.trusted_roots.is_empty()
        {
            return p.trusted_roots.clone();
        }
        if let Some(d) = self.discovery.as_ref() {
            return d.project_roots.clone();
        }
        Vec::new()
    }

    /// Validate all semantically constrained config fields.
    ///
    /// Called after deserialization to ensure that valid TOML does not
    /// silently produce broken runtime behavior ("write legal, load crash").
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(backend) = &self.backend
            && !matches!(backend.as_str(), "tmux" | "herdr")
        {
            return Err(ConfigError::InvalidValue {
                field: "backend".to_owned(),
                reason: format!(
                    "unsupported backend '{}'; expected 'tmux' or 'herdr'",
                    backend
                ),
            });
        }
        if let Some(ref herdr) = self.herdr {
            herdr.validate()?;
        }
        if let Some(ref tmux) = self.tmux
            && let Some(ref naming) = tmux.naming
        {
            naming.validate()?;
        }
        if let Some(ref logging) = self.logging {
            logging.validate()?;
        }
        if let Some(ref discovery) = self.discovery {
            discovery.validate()?;
        }
        if let Some(ref projects) = self.projects {
            projects.validate()?;
        }
        if let Some(ref layout) = self.layout {
            crate::usecases::layout_planner::validate_layout_config(layout)?;
        }
        if let Some(ref features) = self.features {
            features.validate()?;
        }
        if let Some(ref pane) = self.pane {
            pane.validate()?;
        }
        Ok(())
    }
}

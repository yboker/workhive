//! Non-interactive default config construction and tool catalog selection.

use super::*;

/// Build the default `AppConfig` for non-interactive (--force) mode.
///
/// Uses the same sensible defaults as the interactive wizard:
/// quad layout and standard naming templates.
pub fn build_default_config(
    selected_locale: Locale,
    target: InitConfigTarget,
    backend: Option<RuntimeBackendId>,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> AppConfig {
    use workhive_core::domain::NamingConfig;
    use workhive_core::ports::NamingSettings;

    let naming_defaults = NamingConfig::default();
    let columns =
        workhive_core::usecases::layout_template::expand_layout_template(&LayoutTemplate::Quad);

    AppConfig {
        backend,
        herdr: None,
        language: Some(selected_locale),
        tmux: Some(TmuxConfig {
            schema_version: Some(workhive_core::ports::TMUX_CONFIG_SCHEMA_VERSION),
            server_name: Some(TmuxConfig::default_server_name()),
            naming: Some(NamingSettings {
                server_template: Some(naming_defaults.server_template),
                session_template: Some(naming_defaults.session_template),
                sanitize: Some(naming_defaults.sanitize),
                max_component_len: Some(naming_defaults.max_component_len as u32),
            }),
            mouse: Some(true),
            clipboard: Some(true),
        }),
        layout: Some(LayoutConfig {
            template: LayoutTemplate::Quad,
            columns,
        }),
        logging: Some(default_logging_config()),
        projects: None,
        discovery: None,
        tools: tools_for_target(target, exec_checker),
        worktree_root: None,
        runtime_root: None,
        features: None,
        pane: None,
    }
}

pub(super) fn tools_for_target(
    target: InitConfigTarget,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Option<workhive_core::ports::ToolCatalogConfig> {
    match target {
        InitConfigTarget::Global => Some(
            workhive_core::usecases::layout_template::global_tool_catalog_config_for_environment(
                exec_checker,
            ),
        ),
        InitConfigTarget::Project => None,
    }
}

pub(super) fn catalog_for_target(
    target: InitConfigTarget,
    config_port: &dyn workhive_core::ports::ConfigPort,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Result<ToolCatalog, workhive_core::errors::ConfigError> {
    match target {
        InitConfigTarget::Global => Ok(
            workhive_core::usecases::layout_template::global_tool_catalog_config_for_environment(
                exec_checker,
            )
            .catalog,
        ),
        InitConfigTarget::Project => {
            workhive_core::usecases::launch_resolver::effective_catalog_from_config(
                config_port,
                None,
            )
        }
    }
}

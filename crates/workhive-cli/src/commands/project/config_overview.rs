#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

// `cmd_sessions` was moved to `commands/session.rs` (Task 2): the global
// session surface (list + attach) lives in its own module so `project.rs`
// stays focused on project-management commands.

/// Render a beginner-friendly visual overview of the effective config.
///
/// Shared by `config show` (default mode) and `config edit` (preview step).
pub(crate) fn render_config_overview(
    cfg: &workhive_core::ports::AppConfig,
    scope: ConfigScope<'_>,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> Result<(), CliError> {
    use workhive_core::domain::PaneLaunchRef;
    use workhive_core::i18n_keys as k;

    let t = |key: &str| ctx.i18n.resolve(locale, key);
    let ta = |key: &str, args: &[&str]| ctx.i18n.resolve_with_args(locale, key, args);

    let scope_label = if scope.is_global() {
        t(k::CONFIG_OVERVIEW_SCOPE_GLOBAL)
    } else {
        t(k::CONFIG_OVERVIEW_SCOPE_PROJECT)
    };
    println!("\n  {}  [{scope_label}]", t(k::CONFIG_OVERVIEW_TITLE));
    println!("  {}", "\u{2500}".repeat(44));

    let (effective_scope_raw, effective_path) = resolve_config_source(scope, ctx)?;
    let effective_scope = if effective_scope_raw == "project" {
        ctx.i18n.resolve(locale, k::INIT_SCOPE_PROJECT)
    } else {
        ctx.i18n.resolve(locale, k::INIT_SCOPE_GLOBAL)
    };
    let effective_path_human =
        crate::path_display::display_path_for_humans(camino::Utf8Path::new(&effective_path));
    println!(
        "\n  {}",
        ta(
            k::CONFIG_OVERVIEW_EFFECTIVE_SOURCE,
            &[&effective_scope, &effective_path_human],
        )
    );

    // -- Config paths --
    let global_path = display_global_config_path();
    let project_path = display_project_config_path();
    println!(
        "\n  {}",
        ta(k::CONFIG_OVERVIEW_GLOBAL_PATH, &[&global_path])
    );
    println!(
        "  {}",
        ta(k::CONFIG_OVERVIEW_PROJECT_PATH, &[&project_path])
    );

    // -- Language --
    let lang = cfg
        .language
        .as_ref()
        .map(|l| l.to_string())
        .unwrap_or_else(|| t(k::CONFIG_OVERVIEW_DEFAULT));
    println!("\n  {}: {lang}", t(k::CONFIG_OVERVIEW_LANGUAGE));

    let backend = cfg
        .backend
        .clone()
        .unwrap_or_else(workhive_core::domain::RuntimeBackendId::tmux);
    println!("\n  backend: {backend}");
    if backend == workhive_core::domain::RuntimeBackendId::herdr() {
        let session = cfg
            .herdr
            .as_ref()
            .and_then(|herdr| herdr.session.as_deref())
            .unwrap_or(workhive_core::ports::DEFAULT_HERDR_SESSION);
        println!("  herdr.session: {session}");
    }

    // -- Tmux --
    let tmux = cfg.tmux_or_default();
    println!("\n  {}", t(k::CONFIG_OVERVIEW_TMUX));
    println!(
        "    {}: {}",
        t(k::CONFIG_OVERVIEW_FIELD_SERVER_NAME),
        tmux.resolved_server_name()
    );
    if let Some(ref naming) = tmux.naming {
        if let Some(ref tpl) = naming.server_template {
            println!("    {}: {tpl}", t(k::CONFIG_OVERVIEW_FIELD_SERVER_TEMPLATE));
        }
        if let Some(ref tpl) = naming.session_template {
            println!(
                "    {}: {tpl}",
                t(k::CONFIG_OVERVIEW_FIELD_SESSION_TEMPLATE)
            );
        }
    }
    println!(
        "    mouse: {}",
        localized_bool(ctx, locale, tmux.resolved_mouse())
    );
    println!(
        "    clipboard: {}",
        localized_bool(ctx, locale, tmux.resolved_clipboard())
    );

    // -- Layout --
    let layout = cfg.layout_or_default();
    println!("\n  {}", t(k::CONFIG_OVERVIEW_LAYOUT));
    println!(
        "    {}: {}",
        t(k::CONFIG_OVERVIEW_FIELD_LAYOUT_TEMPLATE),
        layout.template
    );
    if layout.columns.is_empty() {
        println!(
            "    {}: {}",
            t(k::CONFIG_OVERVIEW_FIELD_COLUMNS),
            t(k::CONFIG_OVERVIEW_LAYOUT_NONE)
        );
    } else {
        for (ci, col) in layout.columns.iter().enumerate() {
            let col_index = (ci + 1).to_string();
            let width = col.width.to_string();
            println!(
                "    \u{250c} {}",
                ta(k::CONFIG_OVERVIEW_FIELD_COLUMN, &[&col_index, &width])
            );
            for (pi, pane) in col.panes.iter().enumerate() {
                let launch_desc = match &pane.launch {
                    PaneLaunchRef::EmptyShell => t(k::CONFIG_OVERVIEW_PANE_SHELL),
                    PaneLaunchRef::Tool { name } => {
                        ta(k::CONFIG_OVERVIEW_PANE_TOOL, &[name.as_str()])
                    }
                    PaneLaunchRef::InlineCommand { spec } => {
                        let full = format!("{} {}", spec.program(), spec.args().join(" "));
                        ta(k::CONFIG_OVERVIEW_PANE_CMD, &[full.trim()])
                    }
                };
                let connector = if pi + 1 < col.panes.len() {
                    "\u{251c}"
                } else {
                    "\u{2514}"
                };
                let pane_index = (pi + 1).to_string();
                let pane_height = pane.height.to_string();
                println!(
                    "    {connector} {}",
                    ta(
                        k::CONFIG_OVERVIEW_FIELD_PANE,
                        &[&pane_index, &pane_height, &launch_desc],
                    )
                );
            }
        }
    }

    // -- Tools --
    let catalog = cfg
        .tools
        .as_ref()
        .map(|tc| &tc.catalog)
        .cloned()
        .unwrap_or_else(workhive_core::domain::ToolCatalog::builtin_defaults);
    if catalog.is_empty() {
        println!(
            "\n  {}: {}",
            t(k::CONFIG_OVERVIEW_TOOLS),
            t(k::CONFIG_OVERVIEW_TOOLS_NONE)
        );
    } else {
        let count_str = catalog.len().to_string();
        println!(
            "\n  {} ({})",
            t(k::CONFIG_OVERVIEW_TOOLS),
            ta(k::CONFIG_OVERVIEW_TOOLS_COUNT, &[&count_str])
        );
        for entry in catalog.iter() {
            let marker = if entry.enabled { "+" } else { "-" };
            let missing_suffix = if entry.enabled
                && !ctx
                    .composition
                    .executable_checker()
                    .is_available(entry.command.program())
            {
                format!(" ({})", t(k::CONFIG_OVERVIEW_TOOL_MISSING_EXECUTABLE))
            } else {
                String::new()
            };
            println!(
                "    [{marker}] {}: {}{}",
                entry.name.as_str(),
                entry.command,
                missing_suffix
            );
        }
    }

    // -- Logging --
    if let Some(ref log) = cfg.logging {
        println!("\n  {}", t(k::CONFIG_OVERVIEW_LOGGING));
        let console = log.effective_console();
        let c_enabled = if console.is_enabled() {
            t(k::CONFIG_OVERVIEW_ENABLED)
        } else {
            t(k::CONFIG_OVERVIEW_DISABLED)
        };
        let c_level = console
            .level
            .unwrap_or(workhive_core::ports::LogLevel::Warn);
        let c_format = console
            .format
            .as_ref()
            .map(|f| f.to_string())
            .unwrap_or_else(|| "compact".to_owned());
        println!(
            "    {}: {c_enabled} ({}: {c_level}, {}: {c_format})",
            t(k::CONFIG_OVERVIEW_FIELD_CONSOLE),
            t(k::CONFIG_OVERVIEW_FIELD_LEVEL),
            t(k::CONFIG_OVERVIEW_FIELD_FORMAT)
        );

        let file = log.effective_file();
        let f_enabled = if file.is_enabled() {
            t(k::CONFIG_OVERVIEW_ENABLED)
        } else {
            t(k::CONFIG_OVERVIEW_DISABLED)
        };
        let f_level = file.level.unwrap_or(workhive_core::ports::LogLevel::Debug);
        let f_base = file.base_name.as_deref().unwrap_or("workhive.log");
        let f_rotation = file
            .rotation
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "never".to_owned());
        let f_retention = file
            .retention
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "unlimited".to_owned());
        println!(
            "    {}: {f_enabled} ({}: {f_level})",
            t(k::CONFIG_OVERVIEW_FIELD_FILE),
            t(k::CONFIG_OVERVIEW_FIELD_LEVEL)
        );
        println!("      {}: {f_base}", t(k::CONFIG_OVERVIEW_FIELD_BASE_NAME));
        println!(
            "      {}: {f_rotation}",
            t(k::CONFIG_OVERVIEW_FIELD_ROTATION)
        );
        let is_never = file.rotation.is_none()
            || matches!(
                file.rotation,
                Some(workhive_core::ports::LogRotation::Never)
            );
        if is_never {
            println!(
                "      {}: {f_retention} {}",
                t(k::CONFIG_OVERVIEW_FIELD_RETENTION),
                t(k::CONFIG_OVERVIEW_RETENTION_IGNORED)
            );
        } else {
            println!(
                "      {}: {f_retention}",
                t(k::CONFIG_OVERVIEW_FIELD_RETENTION)
            );
        }
        if let Some(ref scope) = file.scope {
            println!("      {}: {scope}", t(k::CONFIG_OVERVIEW_FIELD_SCOPE));
        }
    } else {
        println!(
            "\n  {}: {}",
            t(k::CONFIG_OVERVIEW_LOGGING),
            t(k::CONFIG_OVERVIEW_LOGGING_DEFAULTS)
        );
    }

    // -- Edit guidance --
    println!("\n  {}", t(k::CONFIG_OVERVIEW_TIP_EDIT));
    println!("  {}", t(k::CONFIG_OVERVIEW_TIP_RAW));
    Ok(())
}

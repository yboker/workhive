//! Main interactive wizard flow: step sequencing and summary.

use super::*;
use std::io::IsTerminal;

// ══════════════════════════════════════════════════════════════════════════════
// Main wizard flow
// ══════════════════════════════════════════════════════════════════════════════

pub fn run_wizard(request: WizardRunRequest<'_>) -> Result<AppConfig, CliError> {
    let WizardRunRequest {
        force,
        config_exists,
        config_path,
        target,
        backend,
        deps,
        exec_checker,
        config_port,
        i18n,
        locale,
    } = request;

    eprintln!();

    let selected_locale = crate::commands::select_language(force, i18n, locale)?;
    let loc = selected_locale;
    eprintln!();
    eprintln!("━━━ {} ━━━", i18n.resolve(loc, k::INIT_WIZARD_TITLE));
    eprintln!();
    step_feedback(i18n, loc, &i18n.resolve(loc, k::INIT_SELECT_LANGUAGE));
    if !config_exists && matches!(target, InitConfigTarget::Global) {
        eprintln!("{}", i18n.resolve(loc, k::INIT_GUIDE_GLOBAL));
    }
    eprintln!();

    if !force {
        check_overwrite(config_exists, config_path, i18n, loc)?;
    }

    let backend = match (backend, target) {
        (Some(backend), _) => Some(backend),
        (None, InitConfigTarget::Project) => {
            // Project init also asks for backend selection in interactive mode.
            if force || !std::io::stdin().is_terminal() {
                None // Inherit from global config
            } else {
                let title = i18n.resolve(loc, k::INIT_SELECT_BACKEND);
                let items = vec![
                    i18n.resolve(loc, k::INIT_BACKEND_INHERIT),
                    i18n.resolve(loc, k::INIT_BACKEND_TMUX),
                    i18n.resolve(loc, k::INIT_BACKEND_HERDR),
                ];
                let selection = crate::ui::screens::select::run(&title, &items, 0)?;
                let backend = match selection {
                    0 => None, // Inherit
                    1 => Some(RuntimeBackendId::tmux()),
                    2 => Some(RuntimeBackendId::herdr()),
                    _ => None,
                };
                if let Some(ref b) = backend {
                    crate::commands::print_choice_transcript(title.as_str(), b.as_str());
                } else {
                    crate::commands::print_choice_transcript(title.as_str(), "inherit");
                }
                backend
            }
        }
        (None, InitConfigTarget::Global) if force || !std::io::stdin().is_terminal() => {
            return Err(CliError::Runtime(
                "global initialization requires --backend tmux or --backend herdr".to_owned(),
            ));
        }
        (None, InitConfigTarget::Global) => {
            let title = i18n.resolve(loc, k::INIT_SELECT_BACKEND);
            let items = vec![
                i18n.resolve(loc, k::INIT_BACKEND_TMUX),
                i18n.resolve(loc, k::INIT_BACKEND_HERDR),
            ];
            let selection = crate::ui::screens::select::run(&title, &items, 0)?;
            let backend = if selection == 1 {
                RuntimeBackendId::herdr()
            } else {
                RuntimeBackendId::tmux()
            };
            crate::commands::print_choice_transcript(title.as_str(), backend.as_str());
            Some(backend)
        }
    };

    let effective_backend = match &backend {
        Some(backend) => backend.clone(),
        None => workhive_core::usecases::config_mgmt::resolve_effective_backend(
            None,
            config_port.load_global()?.as_ref(),
        ),
    };
    use workhive_core::ports::HerdrConfig;

    let herdr = if target == InitConfigTarget::Project
        && effective_backend == RuntimeBackendId::herdr()
        && !force
        && std::io::stdin().is_terminal()
    {
        let global = config_port.load_global()?;
        let inherited_session =
            workhive_core::usecases::config_mgmt::resolve_effective_herdr_session(
                None,
                global.as_ref(),
            );
        let title = i18n.resolve(loc, k::INIT_SELECT_HERDR_SESSION);
        let inherit =
            i18n.resolve_with_args(loc, k::INIT_HERDR_SESSION_INHERIT, &[&inherited_session]);
        let items = vec![inherit, i18n.resolve(loc, k::INIT_HERDR_SESSION_OVERRIDE)];
        if crate::ui::screens::select::run(&title, &items, 0)? == 1 {
            let session = crate::ui::screens::text_input::run(
                &i18n.resolve(loc, k::INIT_HERDR_SESSION_INPUT),
                &inherited_session,
            )?;
            let herdr = HerdrConfig {
                session: Some(session),
            };
            herdr.validate()?;
            Some(herdr)
        } else {
            Some(HerdrConfig::default())
        }
    } else {
        None
    };

    check_dependency_binaries_for_backend(deps, &effective_backend, i18n, loc, true)?;
    step_feedback(i18n, loc, &i18n.resolve(loc, k::DEP_ALL_OK));
    eprintln!();

    // Step 1: Layout (preview + select in one step)
    eprintln!("── {} ──", i18n.resolve(loc, k::INIT_STEP_LAYOUT));
    eprintln!();
    let catalog = catalog_for_target(target, config_port, exec_checker)?;
    let (template, mut columns) = ask_layout(&catalog, exec_checker, i18n, loc)?;
    step_feedback(
        i18n,
        loc,
        &format!(
            "{}: {}",
            i18n.resolve(loc, k::INIT_LAYOUT_TEMPLATE),
            template
        ),
    );
    eprintln!();

    // Step 2: Pane commands
    eprintln!("── {} ──", i18n.resolve(loc, k::INIT_STEP_PANE_COMMANDS));
    eprintln!();

    // Context echo: selected layout, total pane count, and the current
    // launch/default for every pane position, so the user can answer
    // "Modify default pane commands?" without scrolling back.
    for line in pane_defaults_summary_lines(&template, &columns, i18n, loc) {
        eprintln!("{line}");
    }
    eprintln!();

    let has_any_command = columns.iter().any(|c| {
        c.panes
            .iter()
            .any(|p| p.launch != PaneLaunchRef::EmptyShell)
    });
    let want_modify = if has_any_command {
        let prompt = i18n.resolve(loc, k::INIT_PANE_CMD_MODIFY_ASK);
        confirm_with_transcript(i18n, loc, &prompt, false)?
    } else {
        true
    };

    if want_modify {
        ask_pane_commands(&template, &mut columns, &catalog, exec_checker, i18n, loc)?;
    }
    step_feedback(i18n, loc, &i18n.resolve(loc, k::INIT_STEP_PANE_COMMANDS));
    eprintln!();

    // Summary (no confirmation needed — user already made all choices)
    eprintln!("── {} ──", i18n.resolve(loc, k::INIT_STEP_CONFIRM));
    eprintln!();
    print_summary(i18n, loc, &selected_locale, &template, &columns);
    eprintln!();

    use workhive_core::domain::NamingConfig;
    use workhive_core::ports::NamingSettings;

    let naming_defaults = NamingConfig::default();
    // Trusted roots are populated by the first-open/init trust confirmation
    // (or `project roots add`), never by the wizard.
    let projects_section: Option<ProjectsConfig> = None;
    Ok(AppConfig {
        backend,
        herdr,
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
        layout: Some(LayoutConfig { template, columns }),
        logging: Some(default_logging_config()),
        projects: projects_section,
        discovery: None,
        tools: tools_for_target(target, exec_checker),
        worktree_root: None,
        runtime_root: None,
        features: None,
        pane: None,
    })
}

// ══════════════════════════════════════════════════════════════════════════════
// Summary
// ══════════════════════════════════════════════════════════════════════════════

pub(super) fn print_summary(
    i18n: &StaticI18n,
    locale: Locale,
    selected_locale: &Locale,
    template: &LayoutTemplate,
    columns: &[ColumnConfig],
) {
    let empty_label = i18n.resolve(locale, k::INIT_PANE_CMD_EMPTY);

    eprintln!(
        "  {}: {}",
        i18n.resolve(locale, k::INIT_SELECT_LANGUAGE),
        selected_locale
    );
    eprintln!(
        "  {}: {}",
        i18n.resolve(locale, k::INIT_LAYOUT_TEMPLATE),
        template
    );
    eprintln!();
    eprint!("{}", render_diagram(columns, i18n, locale));

    for (ci, col) in columns.iter().enumerate() {
        for (ri, pane) in col.panes.iter().enumerate() {
            let display = launch_display_name(&pane.launch, i18n, locale);
            let display = if matches!(pane.launch, PaneLaunchRef::EmptyShell) {
                format!("({})", empty_label)
            } else {
                display
            };
            eprintln!("    C{}R{} → {}", ci + 1, ri + 1, display);
        }
    }
}

//! Main interactive wizard flow: step sequencing, snapshot prompt, and summary.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// Main wizard flow
// ══════════════════════════════════════════════════════════════════════════════

pub fn run_wizard(request: WizardRunRequest<'_>) -> Result<AppConfig, CliError> {
    let WizardRunRequest {
        force,
        config_exists,
        config_path,
        target,
        deps,
        exec_checker,
        config_port,
        i18n,
        locale,
    } = request;

    eprintln!();
    eprintln!("━━━ {} ━━━", i18n.resolve(locale, k::INIT_WIZARD_TITLE));
    eprintln!();

    if !force {
        check_overwrite(config_exists, config_path, i18n, locale)?;
    }

    check_dependencies(deps, i18n, locale)?;
    step_feedback(i18n, locale, &i18n.resolve(locale, k::DEP_ALL_OK));
    eprintln!();

    let selected_locale = crate::commands::select_language(force, i18n, locale)?;
    let loc = selected_locale;
    step_feedback(i18n, loc, &i18n.resolve(loc, k::INIT_SELECT_LANGUAGE));
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
    // "Modify default pane commands?" (and the per-pane prompts that
    // follow) without scrolling back to the layout step.
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

    // Step 3: Snapshot
    eprintln!("── {} ──", i18n.resolve(loc, k::INIT_STEP_SNAPSHOT));
    eprintln!();
    eprintln!("  {}", i18n.resolve(loc, k::INIT_SNAPSHOT_DESC));
    eprintln!();
    let snapshot_enabled = ask_snapshot(i18n, loc)?;
    let snap_str = if snapshot_enabled {
        i18n.resolve(loc, k::BOOL_YES)
    } else {
        i18n.resolve(loc, k::BOOL_NO)
    };
    step_feedback(
        i18n,
        loc,
        &format!("{}: {}", i18n.resolve(loc, k::INIT_SNAPSHOT_ASK), snap_str),
    );
    eprintln!();

    // Step 4: Trusted project roots (global init only).
    let trusted_roots = if matches!(target, InitConfigTarget::Global) {
        let roots = ask_trusted_roots(i18n, loc)?;
        eprintln!();
        roots
    } else {
        Vec::new()
    };

    // Summary (no confirmation needed — user already made all choices)
    eprintln!("── {} ──", i18n.resolve(loc, k::INIT_STEP_CONFIRM));
    eprintln!();
    print_summary(
        i18n,
        loc,
        &selected_locale,
        &template,
        &columns,
        snapshot_enabled,
        &trusted_roots,
    );
    eprintln!();

    use workhive_core::domain::NamingConfig;
    use workhive_core::ports::NamingSettings;

    let naming_defaults = NamingConfig::default();
    let projects_section = if trusted_roots.is_empty() {
        None
    } else {
        Some(ProjectsConfig {
            trusted_roots: trusted_roots.clone(),
        })
    };
    Ok(AppConfig {
        language: Some(selected_locale),
        tmux: Some(TmuxConfig {
            naming: Some(NamingSettings {
                server_template: Some(naming_defaults.server_template),
                session_template: Some(naming_defaults.session_template),
                sanitize: Some(naming_defaults.sanitize),
                max_component_len: Some(naming_defaults.max_component_len as u32),
            }),
            ..TmuxConfig::default()
        }),
        layout: Some(LayoutConfig { template, columns }),
        snapshot: Some(SnapshotConfig {
            enabled: snapshot_enabled,
            ..SnapshotConfig::default()
        }),
        logging: Some(default_logging_config()),
        projects: projects_section,
        discovery: None,
        tools: tools_for_target(target, exec_checker),
        worktree_root: None,
        snapshot_root: None,
        runtime_root: None,
        features: None,
        pane: None,
    })
}

/// Default selection presented for "enable snapshot?" in the global init
/// wizard. Locked to `false`:
/// snapshots remain a stateful recovery feature whose stability is still
/// being hardened, so new configs should be opt-in. Existing user configs
/// are never silently rewritten — the wizard only writes its answer into
/// freshly generated configs.
pub(crate) const WIZARD_SNAPSHOT_PROMPT_DEFAULT: bool = false;

/// Global init wizard prompt for snapshot enablement.
pub(super) fn ask_snapshot(i18n: &StaticI18n, locale: Locale) -> Result<bool, CliError> {
    let prompt = i18n.resolve(locale, k::INIT_SNAPSHOT_ASK);
    confirm_with_transcript(i18n, locale, &prompt, WIZARD_SNAPSHOT_PROMPT_DEFAULT)
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
    snapshot_enabled: bool,
    trusted_roots: &[Utf8PathBuf],
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

    let snap_str = if snapshot_enabled {
        i18n.resolve(locale, k::BOOL_YES)
    } else {
        i18n.resolve(locale, k::BOOL_NO)
    };
    eprintln!(
        "  {}: {}",
        i18n.resolve(locale, k::INIT_SNAPSHOT_ASK),
        snap_str
    );

    if !trusted_roots.is_empty() {
        eprintln!("  {}:", i18n.resolve(locale, k::INIT_STEP_TRUSTED_ROOTS),);
        for r in trusted_roots {
            eprintln!("    - {}", r);
        }
    }
}

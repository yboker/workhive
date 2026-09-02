//! Pane startup command selection step and its preset catalog.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// Pane command presets — built-in tool registry
// ══════════════════════════════════════════════════════════════════════════════

/// Built-in pane command presets.
///
/// Maps tool binary names to i18n label keys for display in the wizard.
/// The label is resolved at runtime via the i18n catalog.
pub(super) const TOOL_LABEL_KEYS: &[(&str, &str)] = &[
    ("claude", k::PANE_PRESET_CLAUDE),
    ("codex", k::PANE_PRESET_CODEX),
    ("opencode", k::PANE_PRESET_OPENCODE),
    ("cursor-agent", k::PANE_PRESET_CURSOR_AGENT),
    ("lazygit", k::PANE_PRESET_LAZYGIT),
    ("yazi", k::PANE_PRESET_YAZI),
    ("htop", k::PANE_PRESET_HTOP),
    ("btm", k::PANE_PRESET_BOTTOM),
];

pub(super) fn tool_label_key(name: &str) -> Option<&'static str> {
    TOOL_LABEL_KEYS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, key)| *key)
}

pub(super) struct CatalogCommand {
    pub(super) label: String,
    /// Whether the tool binary was found on the system.
    /// Used by tests to verify catalog display behavior.
    #[allow(dead_code)]
    pub(super) available: bool,
    pub(super) launch: Option<PaneLaunchRef>,
}

impl CatalogCommand {
    pub(super) fn is_custom_sentinel(&self) -> bool {
        self.launch.is_none()
    }
}

/// List ALL tools from the catalog, marking each as available or not.
/// Uses the unified launch resolver pipeline (same as `open`).
pub(super) fn detect_catalog_commands(
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<CatalogCommand> {
    use workhive_core::usecases::launch_resolver;

    let mut cmds = Vec::new();

    for entry in catalog.iter() {
        if !entry.enabled {
            continue;
        }
        let launch_ref = PaneLaunchRef::Tool {
            name: entry.name.clone(),
        };
        let resolved = launch_resolver::resolve_launch(&launch_ref, catalog, exec_checker);
        let available = resolved.has_command();
        let not_found = i18n.resolve(locale, k::INIT_TOOL_NOT_FOUND);

        let base_label = tool_label_key(entry.name.as_str())
            .map(|key| i18n.resolve(locale, key))
            .unwrap_or_else(|| entry.name.as_str().to_owned());
        let label = if available {
            base_label
        } else {
            format!("{base_label} {not_found}")
        };

        cmds.push(CatalogCommand {
            label,
            available,
            launch: Some(launch_ref),
        });
    }

    cmds.push(CatalogCommand {
        label: i18n.resolve(locale, k::INIT_PANE_CMD_EMPTY_OPTION),
        available: true,
        launch: Some(PaneLaunchRef::EmptyShell),
    });
    cmds.push(CatalogCommand {
        label: i18n.resolve(locale, k::INIT_PANE_CMD_CUSTOM_OPTION),
        available: true,
        launch: None,
    });

    cmds
}

// ══════════════════════════════════════════════════════════════════════════════
// Pane command prompts (select from presets or custom input)
// ══════════════════════════════════════════════════════════════════════════════

pub(super) fn ask_pane_commands(
    template: &LayoutTemplate,
    columns: &mut [ColumnConfig],
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(), CliError> {
    let available = detect_catalog_commands(catalog, exec_checker, i18n, locale);
    let labels: Vec<String> = available.iter().map(|c| c.label.clone()).collect();

    for ci in 0..columns.len() {
        for ri in 0..columns[ci].panes.len() {
            let pos = format!("C{}R{}", ci + 1, ri + 1);

            eprintln!(
                "{}",
                pane_command_prompt_line(&pos, &columns[ci].panes[ri].launch, i18n, locale)
            );

            // Default selection: match current launch against available presets.
            let default_idx = available
                .iter()
                .position(|c| c.launch.as_ref() == Some(&columns[ci].panes[ri].launch))
                .unwrap_or(available.len().saturating_sub(2));

            let selector = pane_command_selector_model(
                template,
                columns,
                &pos,
                &labels,
                default_idx,
                i18n,
                locale,
            );

            // Scope the immutable borrows the live preview needs so they
            // end before the confirmed selection mutates this pane.
            let selection_row = {
                let columns_ref: &[ColumnConfig] = columns;
                let available_ref = &available;
                let selector_ref = &selector;
                let preview = crate::ui::screens::select::SidePreview {
                    title: i18n.resolve(locale, k::INIT_PANE_PREVIEW_TITLE),
                    min_width: pane_command_preview_min_width(
                        columns_ref,
                        ci,
                        ri,
                        available_ref,
                        i18n,
                        locale,
                    ),
                    min_height: pane_command_preview_height(columns_ref, ci, ri, i18n, locale),
                    lines_for_row: Box::new(move |row| {
                        let candidate = match selector_ref.command_index_for_row(row) {
                            Ok(idx) => pane_preview_candidate(&available_ref[idx]),
                            Err(_) => PanePreviewCandidate::Launch(
                                columns_ref[ci].panes[ri].launch.clone(),
                            ),
                        };
                        pane_command_preview_lines(columns_ref, ci, ri, &candidate, i18n, locale)
                    }),
                };
                crate::ui::screens::select::run_with_preview(
                    &selector.title,
                    &selector.items,
                    &selector.disabled,
                    selector.default_row,
                    Some(preview),
                )?
            };
            let selection = selector.command_index_for_row(selection_row)?;

            let final_launch = if available[selection].is_custom_sentinel() {
                // Free-form input: parse into an InlineCommand.
                let custom_prompt =
                    i18n.resolve_with_args(locale, k::INIT_PANE_CMD_CUSTOM_INPUT, &[&pos]);
                let raw = crate::ui::screens::text_input::run(&custom_prompt, "")?;
                if raw.trim().is_empty() {
                    PaneLaunchRef::EmptyShell
                } else {
                    let mut parts = raw.split_whitespace();
                    let program = parts.next().unwrap_or("").to_owned();
                    let args: Vec<String> = parts.map(|s| s.to_owned()).collect();
                    PaneLaunchRef::InlineCommand {
                        spec: CommandSpec::new(&program, args.iter().map(|s| s.as_str())),
                    }
                }
            } else {
                available[selection]
                    .launch
                    .clone()
                    .unwrap_or(PaneLaunchRef::EmptyShell)
            };

            eprintln!(
                "{}",
                pane_command_echo_line(&pos, &final_launch, i18n, locale)
            );
            columns[ci].panes[ri].launch = final_launch;
        }
    }

    Ok(())
}

/// Render the per-pane prompt that introduces a pane command pick.
///
/// Format: `  {prompt} [{current_display}]`. Pure so the wizard's
/// pre-select prompt can be verified without spinning up a real
/// terminal selector.
pub(super) fn pane_command_prompt_line(
    pos: &str,
    current_launch: &PaneLaunchRef,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    let display = match current_launch {
        PaneLaunchRef::EmptyShell => {
            format!("({})", i18n.resolve(locale, k::INIT_PANE_CMD_EMPTY))
        }
        _ => launch_display_name(current_launch, i18n, locale),
    };
    let prompt = i18n.resolve_with_args(locale, k::INIT_PANE_CMD_PROMPT, &[pos]);
    format!("  {} [{}]", prompt, display)
}

/// Render the per-pane echo line emitted immediately after a pane
/// command selection is confirmed.
///
/// Format: `    {pos} → {display}`. The contract is that this line
/// appears in the transcript before the next pane's prompt, so each
/// answered pane is visible above the question that follows it.
pub(super) fn pane_command_echo_line(
    pos: &str,
    final_launch: &PaneLaunchRef,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    let display = match final_launch {
        PaneLaunchRef::EmptyShell => {
            format!("({})", i18n.resolve(locale, k::INIT_PANE_CMD_EMPTY))
        }
        _ => launch_display_name(final_launch, i18n, locale),
    };
    format!("    {} → {}", pos, display)
}

pub(super) struct PaneCommandSelectorModel {
    pub(super) title: String,
    pub(super) items: Vec<String>,
    pub(super) disabled: Vec<bool>,
    pub(super) default_row: usize,
    pub(super) command_start_row: usize,
}

impl PaneCommandSelectorModel {
    pub(super) fn command_index_for_row(&self, row: usize) -> Result<usize, CliError> {
        row.checked_sub(self.command_start_row).ok_or_else(|| {
            CliError::Runtime("pane command selector returned a context row".to_owned())
        })
    }
}

pub(super) fn pane_command_selector_model(
    template: &LayoutTemplate,
    columns: &[ColumnConfig],
    pos: &str,
    command_labels: &[String],
    default_command_index: usize,
    i18n: &StaticI18n,
    locale: Locale,
) -> PaneCommandSelectorModel {
    let context = pane_command_selector_context_lines(template, columns, i18n, locale);
    let command_start_row = context.len();
    let title = i18n.resolve_with_args(locale, k::INIT_PANE_CMD_SELECTOR_TITLE, &[pos]);
    let mut items = context;
    items.extend(command_labels.iter().cloned());
    let mut disabled = vec![true; command_start_row];
    disabled.extend(std::iter::repeat_n(false, command_labels.len()));
    PaneCommandSelectorModel {
        title,
        items,
        disabled,
        default_row: command_start_row + default_command_index.min(command_labels.len() - 1),
        command_start_row,
    }
}

pub(super) fn pane_command_selector_context_lines(
    template: &LayoutTemplate,
    columns: &[ColumnConfig],
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<String> {
    let template_label = i18n.resolve(locale, template_label_key(template));
    let total: usize = columns.iter().map(|c| c.panes.len()).sum();
    let header = i18n.resolve_with_args(
        locale,
        k::INIT_PANE_CONTEXT_HEADER,
        &[&template_label, &total.to_string()],
    );
    let panes = pane_command_selector_pane_summary(columns, i18n, locale);
    vec![
        header,
        i18n.resolve_with_args(locale, k::INIT_PANE_CMD_SELECTOR_PANES, &[&panes]),
    ]
}

pub(super) fn pane_command_selector_pane_summary(
    columns: &[ColumnConfig],
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    columns
        .iter()
        .enumerate()
        .flat_map(|(ci, col)| {
            col.panes.iter().enumerate().map(move |(ri, pane)| {
                let pos = format!("C{}R{}", ci + 1, ri + 1);
                let display = match &pane.launch {
                    PaneLaunchRef::EmptyShell => i18n.resolve(locale, k::INIT_PANE_CMD_EMPTY),
                    launch => launch_display_name(launch, i18n, locale),
                };
                format!("{pos}: {display}")
            })
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Compact line-based context echo emitted before the pane-command step
/// asks its first question: the selected layout label, the total pane
/// count, and one line per pane position with its current launch (or
/// empty-shell default).
///
/// Pure so transcript content and ordering can be verified without a
/// terminal. Intentionally line-based — no boxed previews or repeated
/// diagrams.
pub(super) fn pane_defaults_summary_lines(
    template: &LayoutTemplate,
    columns: &[ColumnConfig],
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<String> {
    let template_label = i18n.resolve(locale, template_label_key(template));
    let total: usize = columns.iter().map(|c| c.panes.len()).sum();
    let mut lines = vec![format!(
        "  {}",
        i18n.resolve_with_args(
            locale,
            k::INIT_PANE_CONTEXT_HEADER,
            &[&template_label, &total.to_string()]
        )
    )];
    for (ci, col) in columns.iter().enumerate() {
        for (ri, pane) in col.panes.iter().enumerate() {
            let pos = format!("C{}R{}", ci + 1, ri + 1);
            lines.push(pane_command_echo_line(&pos, &pane.launch, i18n, locale));
        }
    }
    lines
}

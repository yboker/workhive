//! Layout template selection and the custom column-based layout step.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// Layout selection (preview + select unified — no repeated diagram after pick)
// ══════════════════════════════════════════════════════════════════════════════

pub(super) const ALL_TEMPLATES: &[LayoutTemplate] = &[
    LayoutTemplate::AiCodingLeft,
    LayoutTemplate::AiCodingDual,
    LayoutTemplate::HalfSplit,
    LayoutTemplate::Quad,
    LayoutTemplate::Single,
    LayoutTemplate::Custom,
];

pub(super) fn ask_layout(
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(LayoutTemplate, Vec<ColumnConfig>), CliError> {
    let template_names: Vec<String> = vec![
        i18n.resolve(locale, k::INIT_LAYOUT_AI_LEFT),
        i18n.resolve(locale, k::INIT_LAYOUT_AI_DUAL),
        i18n.resolve(locale, k::INIT_LAYOUT_HALF),
        i18n.resolve(locale, k::INIT_LAYOUT_QUAD),
        i18n.resolve(locale, k::INIT_LAYOUT_SINGLE),
        i18n.resolve(locale, k::INIT_LAYOUT_CUSTOM),
    ];

    // Build select items with preview diagrams
    use crate::ui::screens::select_with_preview::SelectItem;
    let items: Vec<SelectItem> = ALL_TEMPLATES
        .iter()
        .enumerate()
        .map(|(idx, tpl)| {
            let preview = if *tpl == LayoutTemplate::Custom {
                i18n.resolve(locale, k::INIT_LAYOUT_CUSTOM_DSL)
            } else {
                let cols = template_columns_for_environment(tpl, catalog, exec_checker);
                render_diagram(&cols, i18n, locale)
            };
            SelectItem {
                label: template_names[idx].clone(),
                preview,
            }
        })
        .collect();

    let title = i18n.resolve(locale, k::INIT_LAYOUT_TEMPLATE);
    let preview_title = i18n.resolve(locale, k::INIT_LAYOUT_PREVIEW);
    let selection =
        crate::ui::screens::select_with_preview::run(&title, &items, 0, 14, &preview_title)?;

    let tpl = ALL_TEMPLATES.get(selection).cloned().unwrap_or_default();
    if let Some(label) = template_names.get(selection) {
        eprintln!("  {} → {}", title, label);
    }
    tracing::debug!(template = %tpl, "layout template selected");

    let columns = if tpl == LayoutTemplate::Custom {
        ask_custom_layout(i18n, locale)?
    } else {
        template_columns_for_environment(&tpl, catalog, exec_checker)
    };

    eprintln!();
    eprint!("{}", render_diagram(&columns, i18n, locale));

    Ok((tpl, columns))
}

pub(super) fn template_columns_for_environment(
    template: &LayoutTemplate,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Vec<ColumnConfig> {
    let columns = workhive_core::usecases::layout_template::expand_layout_template(template);
    workhive_core::usecases::layout_template::degrade_unavailable_template_tools(
        &columns,
        catalog,
        exec_checker,
    )
}

/// Custom layout wizard: flexible column-based layout.
///
/// 1. Ask column count (1..=5)
/// 2. For each column, ask pane count (1..=remaining), dynamically constrained
/// 3. Total panes across all columns ≤ MAX_TOTAL_PANES
/// 4. Width and height are auto-distributed evenly
pub(super) fn ask_custom_layout(
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<Vec<ColumnConfig>, CliError> {
    let max_cols: usize = 5;
    let max_cols_str = max_cols.to_string();
    let cols_prompt = i18n.resolve_with_args(locale, k::INIT_CUSTOM_COLS, &[&max_cols_str]);

    // Preserve a rejected count as the next prompt's initial buffer so
    // the user edits one digit instead of retyping after an
    // out-of-range entry.
    let mut cols_retry = ValidatedInputRetry::new();
    let col_count: usize = loop {
        let col_input = crate::ui::screens::text_input::run(&cols_prompt, cols_retry.initial())?;
        match col_input.parse::<usize>() {
            Ok(n) if (1..=max_cols).contains(&n) => {
                eprintln!("  {} → {}", cols_prompt, n);
                break n;
            }
            _ => {
                let range = i18n.resolve(locale, k::VALIDATION_COLUMNS_RANGE);
                eprintln!("  {} {}  × {}", cols_prompt, col_input, range);
                cols_retry.reject(col_input.as_str());
            }
        }
    };

    let col_width = (100 / col_count) as u8;
    let mut columns = Vec::with_capacity(col_count);
    let mut total_panes: usize = 0;
    let mut per_column_panes: Vec<usize> = Vec::with_capacity(col_count);

    for ci in 0..col_count {
        let remaining = MAX_TOTAL_PANES - total_panes;
        let cols_left = col_count - ci;
        let max_for_this_col = remaining - (cols_left - 1);
        let max_panes = max_for_this_col.min(MAX_TOTAL_PANES);

        let col_num = (ci + 1).to_string();
        let max_str = max_panes.to_string();
        let pane_prompt =
            i18n.resolve_with_args(locale, k::INIT_CUSTOM_COL_ROWS, &[&col_num, &max_str]);

        let pane_count: usize = if max_panes == 1 {
            eprintln!("  {} → 1", pane_prompt);
            1
        } else {
            let mut pane_retry = ValidatedInputRetry::new();
            loop {
                let input =
                    crate::ui::screens::text_input::run(&pane_prompt, pane_retry.initial())?;
                match input.parse::<usize>() {
                    Ok(n) if (1..=max_panes).contains(&n) => {
                        eprintln!("  {} → {}", pane_prompt, n);
                        break n;
                    }
                    _ => {
                        let range =
                            i18n.resolve_with_args(locale, k::VALIDATION_ROWS_RANGE, &[&max_str]);
                        eprintln!("  {} {}  × {}", pane_prompt, input, range);
                        pane_retry.reject(input.as_str());
                    }
                }
            }
        };

        total_panes += pane_count;
        per_column_panes.push(pane_count);
        // Running context echo: configured vs pending columns plus the
        // pane total so far, kept to one compact line per answer.
        eprintln!(
            "{}",
            custom_layout_progress_line(&per_column_panes, col_count, i18n, locale)
        );
        let pane_height = (100 / pane_count) as u8;
        let panes: Vec<PaneConfig> = (0..pane_count)
            .map(|_| PaneConfig {
                height: pane_height,
                launch: PaneLaunchRef::EmptyShell,
            })
            .collect();

        columns.push(ColumnConfig {
            width: col_width,
            panes,
        });
    }

    tracing::debug!(
        cols = col_count,
        total_panes,
        "custom layout (flexible per-column)"
    );

    Ok(columns)
}

/// i18n label key for a layout template, matching the labels presented
/// by the layout selector so context echoes reuse the exact wording the
/// user picked.
pub(super) fn template_label_key(template: &LayoutTemplate) -> &'static str {
    match template {
        LayoutTemplate::AiCodingLeft => k::INIT_LAYOUT_AI_LEFT,
        LayoutTemplate::AiCodingDual => k::INIT_LAYOUT_AI_DUAL,
        LayoutTemplate::HalfSplit => k::INIT_LAYOUT_HALF,
        LayoutTemplate::Quad => k::INIT_LAYOUT_QUAD,
        LayoutTemplate::Single => k::INIT_LAYOUT_SINGLE,
        LayoutTemplate::Custom => k::INIT_LAYOUT_CUSTOM,
    }
}

///
/// Emitted after each accepted per-column pane-count answer so the user
/// always sees how many columns are configured, which pane counts they
/// hold, which columns are still pending (marked explicitly, e.g.
/// `C3=pending`), and the running pane total. Pure for unit testing.
pub(super) fn custom_layout_progress_line(
    per_column_panes: &[usize],
    total_columns: usize,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    let configured = per_column_panes.len();
    let total_panes: usize = per_column_panes.iter().sum();
    let pending_label = i18n.resolve(locale, k::INIT_CUSTOM_PROGRESS_PENDING);
    let detail = (0..total_columns)
        .map(|i| match per_column_panes.get(i) {
            Some(n) => format!("C{}={}", i + 1, n),
            None => format!("C{}={}", i + 1, pending_label),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "  {}",
        i18n.resolve_with_args(
            locale,
            k::INIT_CUSTOM_PROGRESS,
            &[
                &configured.to_string(),
                &total_columns.to_string(),
                &detail,
                &total_panes.to_string(),
            ]
        )
    )
}

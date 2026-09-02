#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;

/// Pure data row for the global (cross-project) attach selector.
///
/// Session id leads every row so the cross-project picker matches the
/// project-scoped attach selector and the same identifier `tmux ls`
/// would print. Project and workspace columns follow as separate
/// fields so the responsive renderer can drop them independently on
/// narrow terminals without garbling the row's identity.
#[derive(Debug, Clone)]
pub(crate) struct GlobalAttachRow {
    pub session_id: String,
    pub project: String,
    pub workspace: String,
    pub path: String,
    pub attached: bool,
}

/// Pre-localized column header labels for the global attach selector.
#[derive(Debug, Clone)]
pub(crate) struct GlobalAttachColumnHeaders {
    pub session: String,
    pub project: String,
    pub workspace: String,
    pub path: String,
    pub attached: String,
}

/// Built global attach plan: rows in their discovery order plus the
/// pre-localized column headers used by the responsive renderer.
#[derive(Debug, Clone)]
pub(crate) struct GlobalAttachPlan {
    pub rows: Vec<GlobalAttachRow>,
    pub column_headers: GlobalAttachColumnHeaders,
}

/// Visibility decision for each optional column in the aligned global
/// attach table. Session id is the leading identity column and never
/// drops. Optional columns drop right-to-left (path → attached →
/// workspace → project) when the terminal is too narrow to fit them
/// alongside the longest session id. Once the core decision columns
/// (project + workspace) can no longer fit, the renderer switches to a
/// compact per-row summary instead of collapsing to bare session ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GlobalAttachVisibleColumns {
    pub project: bool,
    pub workspace: bool,
    pub path: bool,
    pub attached: bool,
}

impl GlobalAttachVisibleColumns {
    fn all() -> Self {
        Self {
            project: true,
            workspace: true,
            path: true,
            attached: true,
        }
    }

    fn none() -> Self {
        Self {
            project: false,
            workspace: false,
            path: false,
            attached: false,
        }
    }

    fn any_visible(&self) -> bool {
        self.project || self.workspace || self.path || self.attached
    }
}

/// Build the global attach selector plan from cross-project session
/// records. Pure helper: no IO, no probes. Selection dispatch is by
/// row index so the renderer can insert a disabled column-header row
/// without callers having to re-derive any row identity from the
/// rendered label.
pub(crate) fn build_global_attach_plan(
    records: &[usecases::sessions::SessionRecord],
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> GlobalAttachPlan {
    use workhive_core::i18n_keys as k;
    let rows: Vec<GlobalAttachRow> = records
        .iter()
        .map(|r| GlobalAttachRow {
            session_id: r.session_name.clone(),
            project: r.project_name.clone(),
            workspace: r.workspace.clone(),
            path: display_path_for_humans(camino::Utf8Path::new(&r.project_root)),
            attached: r.attached,
        })
        .collect();
    GlobalAttachPlan {
        rows,
        column_headers: GlobalAttachColumnHeaders {
            session: i18n.resolve(locale, k::ATTACH_COL_SESSION),
            project: i18n.resolve(locale, k::ATTACH_COL_PROJECT),
            workspace: i18n.resolve(locale, k::ATTACH_COL_WORKSPACE),
            path: i18n.resolve(locale, k::ATTACH_COL_PATH),
            attached: i18n.resolve(locale, k::ATTACH_COL_ATTACHED),
        },
    }
}

/// Decide which optional global-attach columns to render based on the
/// available terminal width. Deterministic given the plan and width.
pub(crate) fn decide_global_attach_columns(
    plan: &GlobalAttachPlan,
    term_width: usize,
) -> GlobalAttachVisibleColumns {
    const CURSOR_RESERVE: usize = 4;
    let mut visible = GlobalAttachVisibleColumns::all();
    loop {
        let total = compute_global_attach_table_width(plan, &visible);
        if total + CURSOR_RESERVE <= term_width {
            return visible;
        }
        if visible.path {
            visible.path = false;
            continue;
        }
        if visible.attached {
            visible.attached = false;
            continue;
        }
        if visible.workspace {
            visible.workspace = false;
            continue;
        }
        if visible.project {
            visible.project = false;
            continue;
        }
        return GlobalAttachVisibleColumns::none();
    }
}

fn compute_global_attach_table_width(
    plan: &GlobalAttachPlan,
    visible: &GlobalAttachVisibleColumns,
) -> usize {
    let widths = compute_global_attach_column_widths(plan, visible);
    if widths.is_empty() {
        return 0;
    }
    let col_sum: usize = widths.iter().sum();
    let gap_sum = widths.len().saturating_sub(1) * 2;
    col_sum + gap_sum
}

fn compute_global_attach_column_widths(
    plan: &GlobalAttachPlan,
    visible: &GlobalAttachVisibleColumns,
) -> Vec<usize> {
    use crate::table::display_width;
    let headers = &plan.column_headers;

    let mut max_session = display_width(&headers.session);
    let mut max_project = display_width(&headers.project);
    let mut max_workspace = display_width(&headers.workspace);
    let mut max_path = display_width(&headers.path);
    let mut max_attached = display_width(&headers.attached);

    for row in &plan.rows {
        max_session = max_session.max(display_width(&row.session_id));
        max_project = max_project.max(display_width(&row.project));
        max_workspace = max_workspace.max(display_width(&row.workspace));
        max_path = max_path.max(display_width(&row.path));
        if row.attached {
            max_attached = max_attached.max(display_width(&headers.attached));
        }
    }

    let mut widths: Vec<usize> = Vec::new();
    widths.push(max_session);
    if visible.project {
        widths.push(max_project);
    }
    if visible.workspace {
        widths.push(max_workspace);
    }
    if visible.path {
        widths.push(max_path);
    }
    if visible.attached {
        widths.push(max_attached);
    }
    widths
}

fn last_global_visible_column_index(visible: &GlobalAttachVisibleColumns) -> usize {
    let mut last = 0usize;
    if visible.project {
        last = 1;
    }
    if visible.workspace {
        last = 2;
    }
    if visible.path {
        last = 3;
    }
    if visible.attached {
        last = 4;
    }
    last
}

fn global_attach_format_column_header_row(
    plan: &GlobalAttachPlan,
    visible: &GlobalAttachVisibleColumns,
    widths: &[usize],
) -> String {
    use crate::table::pad_to_width;
    let headers = &plan.column_headers;
    let last = last_global_visible_column_index(visible);
    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;

    let session_is_last = last == 0;
    if session_is_last {
        parts.push(headers.session.clone());
    } else {
        parts.push(pad_to_width(&headers.session, widths[idx]));
    }
    idx += 1;

    let push = |want: bool, label: &str, slot: usize, parts: &mut Vec<String>, idx: &mut usize| {
        if want {
            if slot == last {
                parts.push(label.to_owned());
            } else {
                parts.push(pad_to_width(label, widths[*idx]));
            }
            *idx += 1;
        }
    };

    push(visible.project, &headers.project, 1, &mut parts, &mut idx);
    push(
        visible.workspace,
        &headers.workspace,
        2,
        &mut parts,
        &mut idx,
    );
    push(visible.path, &headers.path, 3, &mut parts, &mut idx);
    push(visible.attached, &headers.attached, 4, &mut parts, &mut idx);

    let _ = idx;
    parts.join("  ")
}

fn global_attach_format_data_row(
    row: &GlobalAttachRow,
    visible: &GlobalAttachVisibleColumns,
    widths: &[usize],
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> String {
    use crate::table::pad_to_width;
    use workhive_core::i18n_keys as k;
    let last = last_global_visible_column_index(visible);

    let attached_text = if row.attached {
        i18n.resolve(locale, k::ATTACH_MARKER_ATTACHED)
    } else {
        String::new()
    };

    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;

    let session_is_last = last == 0;
    if session_is_last {
        parts.push(row.session_id.clone());
    } else {
        parts.push(pad_to_width(&row.session_id, widths[idx]));
    }
    idx += 1;

    let push = |want: bool, value: &str, slot: usize, parts: &mut Vec<String>, idx: &mut usize| {
        if want {
            if slot == last {
                parts.push(value.to_owned());
            } else {
                parts.push(pad_to_width(value, widths[*idx]));
            }
            *idx += 1;
        }
    };

    push(visible.project, &row.project, 1, &mut parts, &mut idx);
    push(visible.workspace, &row.workspace, 2, &mut parts, &mut idx);
    push(visible.path, &row.path, 3, &mut parts, &mut idx);
    push(visible.attached, &attached_text, 4, &mut parts, &mut idx);

    let _ = idx;
    parts.join("  ")
}

/// Render the global attach plan to the parallel arrays consumed by
/// `select::run_with_disabled`, with a column-header row and aligned
/// data rows when the terminal can fit them.
pub(crate) fn render_global_attach_lines(
    plan: &GlobalAttachPlan,
    term_width: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> (Vec<String>, Vec<bool>) {
    let visible = decide_global_attach_columns(plan, term_width);
    let widths = compute_global_attach_column_widths(plan, &visible);

    // Compact summary mode: the aligned table cannot keep the core
    // decision columns (project + workspace) alongside the session id,
    // so each row collapses to a width-bounded summary instead of bare
    // session ids.
    let compact = !plan.rows.is_empty() && (!visible.project || !visible.workspace);
    let compact_budget = term_width.saturating_sub(GLOBAL_ATTACH_COMPACT_RESERVE);

    let mut labels: Vec<String> = Vec::new();
    let mut disabled: Vec<bool> = Vec::new();

    if !compact && visible.any_visible() && !plan.rows.is_empty() {
        labels.push(global_attach_format_column_header_row(
            plan, &visible, &widths,
        ));
        disabled.push(true);
    }

    for row in &plan.rows {
        let label = if compact {
            global_attach_format_compact_row(row, compact_budget, i18n, locale)
        } else {
            global_attach_format_data_row(row, &visible, &widths, i18n, locale)
        };
        labels.push(label);
        disabled.push(false);
    }

    (labels, disabled)
}

/// Headroom reserved for the selector cursor glyph when a global attach
/// row is rendered as a compact summary.
const GLOBAL_ATTACH_COMPACT_RESERVE: usize = 4;

/// Render a global attach row as a compact single-line summary for
/// terminals too narrow for the aligned table.
///
/// The session id leads, followed by the project and workspace as the
/// core cross-project context (truncated with an ellipsis only when
/// unavoidable). Path is the lowest priority and is appended only when
/// it still fits.
fn global_attach_format_compact_row(
    row: &GlobalAttachRow,
    budget: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> String {
    use crate::table::{CompactSegment, compose_compact_summary};
    use workhive_core::i18n_keys as k;

    let attached = if row.attached {
        i18n.resolve(locale, k::ATTACH_MARKER_ATTACHED)
    } else {
        String::new()
    };

    let segments = [
        CompactSegment {
            text: &row.session_id,
            required: true,
            min_width: 10,
        },
        CompactSegment {
            text: &row.project,
            required: true,
            min_width: 6,
        },
        CompactSegment {
            text: &row.workspace,
            required: true,
            min_width: 6,
        },
        CompactSegment {
            text: &attached,
            required: false,
            min_width: 0,
        },
        CompactSegment {
            text: &row.path,
            required: false,
            min_width: 0,
        },
    ];
    compose_compact_summary(&segments, "  ", budget)
}

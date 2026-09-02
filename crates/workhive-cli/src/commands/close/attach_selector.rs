#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;

/// Inferred workspace kind for an attach selector row.
///
/// Derived from the parsed workspace name vs the project's main branch.
/// `Unknown` is used when the session name does not parse into a
/// workspace identifier (legacy or non-conforming session) — operators
/// still need to be able to attach, so we surface the row but flag the
/// kind as unknown rather than silently mislabelling it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachWorkspaceKind {
    ProjectRoot,
    Worktree,
    Unknown,
}

impl AttachWorkspaceKind {
    pub(crate) fn label(self, i18n: &dyn workhive_core::ports::I18nPort, locale: Locale) -> String {
        use workhive_core::i18n_keys as k;
        match self {
            AttachWorkspaceKind::ProjectRoot => i18n.resolve(locale, k::ATTACH_KIND_PROJECT_ROOT),
            AttachWorkspaceKind::Worktree => i18n.resolve(locale, k::ATTACH_KIND_WORKTREE),
            AttachWorkspaceKind::Unknown => i18n.resolve(locale, k::ATTACH_KIND_UNKNOWN),
        }
    }
}

/// Pure data row for the project-scoped attach selector.
///
/// Session id leads every row so a tmux-aware operator can scan the
/// selector by the same identifier `tmux ls` would print. The
/// workspace identifier and kind are paraphrasings of the same
/// underlying session — they are intentionally redundant so a row
/// stays decipherable when the session id is truncated to fit a
/// narrow terminal.
#[derive(Debug, Clone)]
pub(crate) struct AttachSelectorRow {
    pub session_id: String,
    pub workspace: String,
    pub kind: AttachWorkspaceKind,
    pub attached: bool,
    /// Human-friendly worktree path, when resolvable. `None` keeps the
    /// row session-first without a path field rather than printing a
    /// placeholder.
    pub path: Option<String>,
    /// Per-column layout/pane + recent-activity metadata. `None` when
    /// enrichment is unavailable, so the row degrades to its
    /// session-first identity instead of fabricating status. Kept as
    /// separate fields (not pre-joined) so the renderer can compute
    /// display-width-aware column padding and progressively drop the
    /// lowest-priority columns to fit narrow terminals.
    pub metadata: Option<AttachRowMetadata>,
}

/// Per-row attach enrichment resolved from the live workspace listing.
///
/// Keyed by workspace name and applied by `decorate_attach_selector_plan`
/// so the pure plan builder stays free of runtime probes.
#[derive(Debug, Clone, Default)]
pub(crate) struct AttachRowEnrichment {
    pub path: Option<String>,
    pub metadata: Option<AttachRowMetadata>,
}

/// Pre-localized per-column attach metadata.
///
/// Each field is the final display string for its column. Sourced via
/// the same display helpers as the `workspace list` table so the two
/// surfaces describe the same workspace identically.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct AttachRowMetadata {
    /// Layout / pane-count summary, aligned with the `workspace list`
    /// `Panes / layout` column.
    pub panes_layout: String,
    /// Last-used timestamp, aligned with the `workspace list`
    /// `Last used` column.
    pub recent: String,
}

/// Pre-localized column header labels for the project-scoped attach
/// selector. Stored on the plan so the renderer never re-resolves i18n
/// strings while building the header row, and so tests can assert the
/// localized copy without instantiating the live catalog at every site.
#[derive(Debug, Clone)]
pub(crate) struct AttachSelectorColumnHeaders {
    pub session: String,
    pub workspace: String,
    pub kind: String,
    pub path: String,
    pub attached: String,
    pub panes: String,
    pub recent: String,
}

/// Built attach selector plan: rows grouped project-root-first, then
/// the pre-localized column headers and section labels used by the
/// responsive renderer.
#[derive(Debug, Clone)]
pub(crate) struct AttachSelectorPlan {
    pub rows: Vec<AttachSelectorRow>,
    pub column_headers: AttachSelectorColumnHeaders,
    /// Localized peer-section labels. The project-scoped attach
    /// selector renders root sessions, worktree sessions, and any
    /// sessions whose name does not parse into a project workspace as
    /// distinct peer groups, mirroring `workspace open` / `workspace
    /// ls`, while each selectable row still leads with its session id.
    pub group_root_header: String,
    pub group_worktree_header: String,
    /// Truthful fallback section for sessions that are neither the
    /// project root nor a linked worktree (e.g. legacy or
    /// non-conforming session names). They are never folded into the
    /// worktree section.
    pub group_other_header: String,
}

/// Build the project-scoped attach selector plan.
///
/// Pure helper: takes the runtime listing, the project's main branch,
/// and i18n, then returns the data rows in runtime order plus the
/// pre-localized column headers. Selection dispatch is by row index so
/// the renderer can insert disabled header rows without callers having
/// to re-derive any row identity from the rendered label.
pub(crate) fn build_attach_selector_plan(
    workspaces: &[workhive_core::ports::WorkspaceRuntimeInfo],
    main_branch: &str,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> AttachSelectorPlan {
    use workhive_core::i18n_keys as k;

    let mut rows: Vec<AttachSelectorRow> = Vec::with_capacity(workspaces.len());
    for ws in workspaces {
        let parsed = SessionName::from_raw(&ws.workspace_name);
        let workspace = parsed
            .extract_workspace()
            .map(|s| s.to_owned())
            .unwrap_or_else(|| ws.workspace_name.clone());
        let kind = if parsed.extract_workspace().is_none() {
            AttachWorkspaceKind::Unknown
        } else if workspace == main_branch {
            AttachWorkspaceKind::ProjectRoot
        } else {
            AttachWorkspaceKind::Worktree
        };
        rows.push(AttachSelectorRow {
            session_id: ws.workspace_name.clone(),
            workspace,
            kind,
            attached: ws.attached,
            path: None,
            metadata: None,
        });
    }

    // Group sessions into project-root, then worktree, then any
    // non-conforming sessions so the renderer can present them as peer
    // sections. The sort is stable, so the original runtime order is
    // preserved within each group; selection dispatch maps the chosen
    // data row back to this (already grouped) `rows` order.
    rows.sort_by_key(|r| match r.kind {
        AttachWorkspaceKind::ProjectRoot => 0u8,
        AttachWorkspaceKind::Worktree => 1u8,
        AttachWorkspaceKind::Unknown => 2u8,
    });

    AttachSelectorPlan {
        rows,
        column_headers: AttachSelectorColumnHeaders {
            session: i18n.resolve(locale, k::ATTACH_COL_SESSION),
            workspace: i18n.resolve(locale, k::ATTACH_COL_WORKSPACE),
            kind: i18n.resolve(locale, k::ATTACH_COL_KIND),
            path: i18n.resolve(locale, k::ATTACH_COL_PATH),
            attached: i18n.resolve(locale, k::ATTACH_COL_ATTACHED),
            panes: i18n.resolve(locale, k::ATTACH_COL_PANES),
            recent: i18n.resolve(locale, k::ATTACH_COL_RECENT),
        },
        group_root_header: i18n.resolve(locale, k::LIST_GROUP_PROJECT_ROOT),
        group_worktree_header: i18n.resolve(locale, k::LIST_GROUP_WORKTREE),
        group_other_header: i18n.resolve(locale, k::ATTACH_GROUP_OTHER),
    }
}

/// Apply path + layout/recent enrichment to attach rows, keyed by the
/// parsed workspace name.
///
/// Best-effort: rows whose workspace has no enrichment entry keep their
/// bare session-first identity, so a missing or failed probe never
/// blanks the selector. The pure `build_attach_selector_plan` stays
/// probe-free; this is the single seam where runtime context is folded
/// into the rows.
pub(crate) fn decorate_attach_selector_plan(
    plan: &mut AttachSelectorPlan,
    lookup: &dyn Fn(&str) -> Option<AttachRowEnrichment>,
) {
    for row in plan.rows.iter_mut() {
        if let Some(found) = lookup(&row.workspace) {
            row.path = found.path;
            row.metadata = found.metadata;
        }
    }
}

/// Visibility decision for each optional column in the aligned
/// project-scoped attach table. Session id is the leading identity
/// column and never drops. Optional columns drop right-to-left
/// (recent → panes → attached → path → kind → workspace) when the
/// terminal is too narrow to fit them alongside the longest session id.
/// Once the core decision columns (workspace + kind) can no longer fit,
/// the renderer switches to a compact per-row summary instead of
/// collapsing to bare session ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttachSelectorVisibleColumns {
    pub workspace: bool,
    pub kind: bool,
    pub path: bool,
    pub attached: bool,
    pub panes: bool,
    pub recent: bool,
}

impl AttachSelectorVisibleColumns {
    pub(crate) fn all() -> Self {
        Self {
            workspace: true,
            kind: true,
            path: true,
            attached: true,
            panes: true,
            recent: true,
        }
    }

    fn none() -> Self {
        Self {
            workspace: false,
            kind: false,
            path: false,
            attached: false,
            panes: false,
            recent: false,
        }
    }

    fn any_visible(&self) -> bool {
        self.workspace || self.kind || self.path || self.attached || self.panes || self.recent
    }
}

/// Decide which optional columns to render based on the available
/// terminal width.
///
/// Deterministic given the plan and width — no IO and no plan
/// mutation, so tests can pin specific widths to the expected
/// layout. `term_width` is the full terminal width in display columns;
/// the renderer reserves a small slack for the selector cursor and
/// pad so populated rows never wrap mid-column.
pub(crate) fn decide_attach_selector_columns(
    plan: &AttachSelectorPlan,
    term_width: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> AttachSelectorVisibleColumns {
    // Leave headroom for the selector cursor glyph ("▶ ") + trailing
    // pad so a row that exactly fits the table width does not get
    // wrapped or chopped by the terminal.
    const CURSOR_RESERVE: usize = 4;

    let mut visible = AttachSelectorVisibleColumns::all();
    loop {
        let total = compute_attach_selector_table_width(plan, &visible, i18n, locale);
        if total + CURSOR_RESERVE <= term_width {
            return visible;
        }
        if visible.recent {
            visible.recent = false;
            continue;
        }
        if visible.panes {
            visible.panes = false;
            continue;
        }
        if visible.attached {
            visible.attached = false;
            continue;
        }
        if visible.path {
            visible.path = false;
            continue;
        }
        if visible.kind {
            visible.kind = false;
            continue;
        }
        if visible.workspace {
            visible.workspace = false;
            continue;
        }
        return AttachSelectorVisibleColumns::none();
    }
}

fn compute_attach_selector_table_width(
    plan: &AttachSelectorPlan,
    visible: &AttachSelectorVisibleColumns,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> usize {
    let widths = compute_attach_selector_column_widths(plan, visible, i18n, locale);
    if widths.is_empty() {
        return 0;
    }
    let col_sum: usize = widths.iter().sum();
    let gap_sum = widths.len().saturating_sub(1) * 2;
    col_sum + gap_sum
}

pub(crate) fn compute_attach_selector_column_widths(
    plan: &AttachSelectorPlan,
    visible: &AttachSelectorVisibleColumns,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> Vec<usize> {
    use crate::table::display_width;

    let headers = &plan.column_headers;

    let mut max_session = display_width(&headers.session);
    let mut max_workspace = display_width(&headers.workspace);
    let mut max_kind = display_width(&headers.kind);
    let mut max_path = display_width(&headers.path);
    let mut max_attached = display_width(&headers.attached);
    let mut max_panes = display_width(&headers.panes);
    let mut max_recent = display_width(&headers.recent);

    for row in &plan.rows {
        max_session = max_session.max(display_width(&row.session_id));
        max_workspace = max_workspace.max(display_width(&row.workspace));
        // The kind column is sized from the widest localized kind cell
        // and the localized header so trailing columns share the same
        // display-width boundary in English and zh-CN.
        let kind_label = display_width(&row.kind.label(i18n, locale));
        max_kind = max_kind.max(kind_label);
        if let Some(path) = row.path.as_deref() {
            max_path = max_path.max(display_width(path));
        }
        if row.attached {
            max_attached = max_attached.max(display_width(&headers.attached));
        }
        if let Some(meta) = row.metadata.as_ref() {
            max_panes = max_panes.max(display_width(&meta.panes_layout));
            max_recent = max_recent.max(display_width(&meta.recent));
        }
    }

    let mut widths: Vec<usize> = Vec::new();
    widths.push(max_session);
    if visible.workspace {
        widths.push(max_workspace);
    }
    if visible.kind {
        widths.push(max_kind);
    }
    if visible.path {
        widths.push(max_path);
    }
    if visible.attached {
        widths.push(max_attached);
    }
    if visible.panes {
        widths.push(max_panes);
    }
    if visible.recent {
        widths.push(max_recent);
    }
    widths
}

fn attach_selector_format_column_header_row(
    plan: &AttachSelectorPlan,
    visible: &AttachSelectorVisibleColumns,
    widths: &[usize],
) -> String {
    use crate::table::pad_to_width;

    let headers = &plan.column_headers;
    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;

    parts.push(pad_to_width(&headers.session, widths[idx]));
    idx += 1;

    let push_header = |parts: &mut Vec<String>,
                       want: bool,
                       label: &str,
                       widths: &[usize],
                       idx: &mut usize,
                       is_last: bool| {
        if want {
            if is_last {
                parts.push(label.to_owned());
            } else {
                parts.push(pad_to_width(label, widths[*idx]));
            }
            *idx += 1;
        }
    };

    let last_visible = last_visible_column_index(visible);

    push_header(
        &mut parts,
        visible.workspace,
        &headers.workspace,
        widths,
        &mut idx,
        last_visible == 1,
    );
    push_header(
        &mut parts,
        visible.kind,
        &headers.kind,
        widths,
        &mut idx,
        last_visible == 2,
    );
    push_header(
        &mut parts,
        visible.path,
        &headers.path,
        widths,
        &mut idx,
        last_visible == 3,
    );
    push_header(
        &mut parts,
        visible.attached,
        &headers.attached,
        widths,
        &mut idx,
        last_visible == 4,
    );
    push_header(
        &mut parts,
        visible.panes,
        &headers.panes,
        widths,
        &mut idx,
        last_visible == 5,
    );
    push_header(
        &mut parts,
        visible.recent,
        &headers.recent,
        widths,
        &mut idx,
        last_visible == 6,
    );

    let _ = idx;
    parts.join("  ")
}

/// Maximum optional-column slot index that is currently visible.
///
/// Slot indices: workspace=1, kind=2, path=3, attached=4, panes=5,
/// recent=6. The session column (slot 0) is always visible, so the
/// returned value is at least 0.
fn last_visible_column_index(visible: &AttachSelectorVisibleColumns) -> usize {
    let mut last = 0usize;
    if visible.workspace {
        last = 1;
    }
    if visible.kind {
        last = 2;
    }
    if visible.path {
        last = 3;
    }
    if visible.attached {
        last = 4;
    }
    if visible.panes {
        last = 5;
    }
    if visible.recent {
        last = 6;
    }
    last
}

fn attach_selector_format_data_row(
    row: &AttachSelectorRow,
    headers: &AttachSelectorColumnHeaders,
    visible: &AttachSelectorVisibleColumns,
    widths: &[usize],
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> String {
    use crate::table::pad_to_width;
    use workhive_core::i18n_keys as k;

    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;
    let any_after_session = visible.any_visible();

    if any_after_session {
        parts.push(pad_to_width(&row.session_id, widths[idx]));
    } else {
        parts.push(row.session_id.clone());
    }
    idx += 1;

    let workspace = row.workspace.as_str();
    let kind = row.kind.label(i18n, locale);
    let path = row.path.as_deref().unwrap_or("");
    let attached = if row.attached {
        i18n.resolve(locale, k::ATTACH_MARKER_ATTACHED)
    } else {
        String::new()
    };
    let panes = row
        .metadata
        .as_ref()
        .map(|m| m.panes_layout.as_str())
        .unwrap_or("");
    let recent = row
        .metadata
        .as_ref()
        .map(|m| m.recent.as_str())
        .unwrap_or("");

    let last_visible = last_visible_column_index(visible);
    let _ = headers; // headers fed widths; values themselves are localized above.

    let mut push_optional =
        |value: &str, want: bool, slot: usize, widths: &[usize], idx: &mut usize| {
            if want {
                if slot == last_visible {
                    parts.push(value.to_owned());
                } else {
                    parts.push(pad_to_width(value, widths[*idx]));
                }
                *idx += 1;
            }
        };

    push_optional(workspace, visible.workspace, 1, widths, &mut idx);
    push_optional(&kind, visible.kind, 2, widths, &mut idx);
    push_optional(path, visible.path, 3, widths, &mut idx);
    push_optional(&attached, visible.attached, 4, widths, &mut idx);
    push_optional(panes, visible.panes, 5, widths, &mut idx);
    push_optional(recent, visible.recent, 6, widths, &mut idx);

    let _ = idx;
    parts.join("  ")
}

/// Render the attach selector plan to the parallel arrays consumed by
/// `select::run_with_disabled`, with a column-header row and aligned
/// data rows when the terminal can fit them.
///
/// Returns `(labels, disabled)` where:
/// - `labels[i]` is the rendered display string for display row `i`.
/// - `disabled[i] == true` marks display row `i` as a non-selectable
///   display row. In aligned mode the non-selectable rows are the
///   peer-section labels (project root / worktree / other sessions),
///   the per-section column-header rows, and the blank separator lines
///   between sections. In compact mode no non-selectable rows are
///   emitted.
///
/// Selection mapping: selectable data rows are appended in plan order
/// across the sections, so a caller recovers the chosen
/// `AttachSelectorRow` by counting the non-selectable display rows that
/// precede the selected display index and subtracting that count to
/// land on the matching `plan.rows` entry.
pub(crate) fn render_attach_selector_lines(
    plan: &AttachSelectorPlan,
    term_width: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> (Vec<String>, Vec<bool>) {
    let visible = decide_attach_selector_columns(plan, term_width, i18n, locale);
    let widths = compute_attach_selector_column_widths(plan, &visible, i18n, locale);

    // Compact summary mode: the aligned table cannot keep the core
    // decision columns (workspace + kind) alongside the session id, so
    // each row collapses to a width-bounded summary instead of bare
    // session ids.
    let compact = !plan.rows.is_empty() && (!visible.workspace || !visible.kind);
    let compact_budget = term_width.saturating_sub(ATTACH_COMPACT_RESERVE);

    let mut labels: Vec<String> = Vec::new();
    let mut disabled: Vec<bool> = Vec::new();

    let format_data_row = |row: &AttachSelectorRow| -> String {
        if compact {
            attach_selector_format_compact_row(row, compact_budget, i18n, locale)
        } else {
            attach_selector_format_data_row(
                row,
                &plan.column_headers,
                &visible,
                &widths,
                i18n,
                locale,
            )
        }
    };

    if compact {
        // Narrow terminals keep the selector flat and session-first;
        // there is no room for peer-section headers without crowding
        // out the row identity. Rows still arrive grouped (root-first)
        // from the plan, so related sessions stay adjacent.
        for row in &plan.rows {
            labels.push(format_data_row(row));
            disabled.push(false);
        }
        return (labels, disabled);
    }

    // Aligned mode: render project-root, worktree, and non-conforming
    // sessions as distinct peer sections, mirroring `workspace open` /
    // `workspace ls`. Each non-empty section gets its label and (when
    // any optional column is visible) its own column-header row, and a
    // blank line separates adjacent sections. Selectable rows still
    // lead with the session id; only the section/column-header/blank
    // rows are disabled. `Unknown` sessions are kept in their own
    // truthful fallback section instead of being folded under worktree.
    let roots: Vec<&AttachSelectorRow> = plan
        .rows
        .iter()
        .filter(|r| matches!(r.kind, AttachWorkspaceKind::ProjectRoot))
        .collect();
    let worktrees: Vec<&AttachSelectorRow> = plan
        .rows
        .iter()
        .filter(|r| matches!(r.kind, AttachWorkspaceKind::Worktree))
        .collect();
    let others: Vec<&AttachSelectorRow> = plan
        .rows
        .iter()
        .filter(|r| matches!(r.kind, AttachWorkspaceKind::Unknown))
        .collect();

    let push_section = |labels: &mut Vec<String>,
                        disabled: &mut Vec<bool>,
                        header: &str,
                        rows: &[&AttachSelectorRow]| {
        if rows.is_empty() {
            return;
        }
        // A blank, non-selectable separator precedes every section
        // after the first rendered one.
        if !labels.is_empty() {
            labels.push(String::new());
            disabled.push(true);
        }
        labels.push(header.to_owned());
        disabled.push(true);
        if visible.any_visible() {
            labels.push(attach_selector_format_column_header_row(
                plan, &visible, &widths,
            ));
            disabled.push(true);
        }
        for row in rows {
            labels.push(format_data_row(row));
            disabled.push(false);
        }
    };

    push_section(&mut labels, &mut disabled, &plan.group_root_header, &roots);
    push_section(
        &mut labels,
        &mut disabled,
        &plan.group_worktree_header,
        &worktrees,
    );
    push_section(
        &mut labels,
        &mut disabled,
        &plan.group_other_header,
        &others,
    );

    (labels, disabled)
}

/// Headroom reserved for the selector cursor glyph when an attach row is
/// rendered as a compact summary.
const ATTACH_COMPACT_RESERVE: usize = 4;

/// Render an attach row as a compact single-line summary for terminals
/// too narrow for the aligned table.
///
/// The session id leads (it is the identifier `tmux ls` prints), and the
/// workspace name plus its kind are kept as the core decision context
/// (truncated with an ellipsis only when unavoidable). The kind label is
/// bracketed so it reads as a category tag and is never mistaken for a
/// second workspace name once the aligned column headers are gone.
/// Panes/recent/path are appended in that priority order while they fit.
fn attach_selector_format_compact_row(
    row: &AttachSelectorRow,
    budget: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> String {
    use crate::table::{CompactSegment, compose_compact_summary};

    // Workspace name and bracketed kind share one segment joined by a
    // single space: the tighter join keeps the full `[Kind]` tag visible
    // at narrow widths, and right-side truncation degrades the kind tag
    // before it ever eats into the workspace name.
    let workspace_kind = format!("{} [{}]", row.workspace, row.kind.label(i18n, locale));
    let path = row.path.as_deref().unwrap_or("");
    let panes = row
        .metadata
        .as_ref()
        .map(|m| m.panes_layout.as_str())
        .unwrap_or("");
    let recent = row
        .metadata
        .as_ref()
        .map(|m| m.recent.as_str())
        .unwrap_or("");

    let segments = [
        CompactSegment {
            text: &row.session_id,
            required: true,
            min_width: 10,
        },
        CompactSegment {
            text: &workspace_kind,
            required: true,
            min_width: 6,
        },
        CompactSegment {
            text: panes,
            required: false,
            min_width: 0,
        },
        CompactSegment {
            text: recent,
            required: false,
            min_width: 0,
        },
        CompactSegment {
            text: path,
            required: false,
            min_width: 0,
        },
    ];
    compose_compact_summary(&segments, "  ", budget)
}

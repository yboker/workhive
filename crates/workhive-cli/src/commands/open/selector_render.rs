#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Visibility decision for each optional metadata column in the
/// aligned open-selector table. Workspace name is the leading identity
/// column and never drops. The optional columns drop right-to-left
/// (recent → snapshot → layout → session) when the terminal is too
/// narrow to fit them alongside the longest workspace name. When even
/// the session column cannot fit, every flag stays `false`; the
/// renderer then switches enriched rows to a compact summary (workspace
/// name + session/running status) rather than dropping to bare names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpenSelectorVisibleColumns {
    pub session: bool,
    pub layout: bool,
    pub snapshot: bool,
    pub recent: bool,
}

impl OpenSelectorVisibleColumns {
    fn all() -> Self {
        Self {
            session: true,
            layout: true,
            snapshot: true,
            recent: true,
        }
    }

    fn none() -> Self {
        Self {
            session: false,
            layout: false,
            snapshot: false,
            recent: false,
        }
    }

    fn any_visible(&self) -> bool {
        self.session || self.layout || self.snapshot || self.recent
    }
}

/// Decide which metadata columns to render based on the available
/// terminal width.
///
/// The decision is deterministic given the plan and width — it does no
/// IO and never mutates the plan, so tests can pin specific widths to
/// the expected layout. `term_width` is the full terminal width in
/// display columns; the renderer reserves a small slack for the
/// selector cursor and pad so populated rows never wrap mid-column.
pub(crate) fn decide_open_selector_columns(
    plan: &OpenSelectorPlan,
    term_width: usize,
) -> OpenSelectorVisibleColumns {
    // Leave headroom for the selector cursor glyph ("▶ ") + trailing
    // pad so a row that exactly fits the table width does not get
    // wrapped or chopped by the terminal.
    const CURSOR_RESERVE: usize = 4;

    if !plan_has_any_enrichment(plan) {
        return OpenSelectorVisibleColumns::none();
    }

    let mut visible = OpenSelectorVisibleColumns::all();
    loop {
        let total = compute_open_selector_table_width(plan, &visible);
        if total + CURSOR_RESERVE <= term_width {
            return visible;
        }
        if visible.recent {
            visible.recent = false;
            continue;
        }
        if visible.snapshot {
            visible.snapshot = false;
            continue;
        }
        if visible.layout {
            visible.layout = false;
            continue;
        }
        if visible.session {
            visible.session = false;
            continue;
        }
        return visible;
    }
}

fn plan_has_any_enrichment(plan: &OpenSelectorPlan) -> bool {
    plan.roots
        .iter()
        .chain(plan.worktrees.iter())
        .any(|row| row.enrichment.is_some())
}

/// Width of the rendered table assuming the given column visibility,
/// using display columns (not bytes) so CJK fullwidth glyphs and ASCII
/// align consistently. Returns 0 when no columns are visible.
fn compute_open_selector_table_width(
    plan: &OpenSelectorPlan,
    visible: &OpenSelectorVisibleColumns,
) -> usize {
    let widths = compute_open_selector_column_widths(plan, visible);
    if widths.is_empty() {
        return 0;
    }
    let col_sum: usize = widths.iter().sum();
    let gap_sum = widths.len().saturating_sub(1) * 2;
    col_sum + gap_sum
}

/// Per-column display widths sized to the longest value in the plan.
///
/// Includes the workspace column (always present) and any visible
/// optional columns. Header labels participate in the max so the
/// header row never wraps under data values shorter than the header.
fn compute_open_selector_column_widths(
    plan: &OpenSelectorPlan,
    visible: &OpenSelectorVisibleColumns,
) -> Vec<usize> {
    use crate::table::display_width;

    let headers = &plan.column_headers;
    let mut widths: Vec<usize> = Vec::new();

    let mut max_workspace = display_width(&headers.workspace);
    let mut max_session = display_width(&headers.session);
    let mut max_layout = display_width(&headers.layout);
    let mut max_snapshot = display_width(&headers.snapshot);
    let mut max_recent = display_width(&headers.recent);

    for row in plan.roots.iter().chain(plan.worktrees.iter()) {
        max_workspace = max_workspace.max(display_width(&row.label));
        if let Some(meta) = row.enrichment.as_ref() {
            max_session = max_session.max(display_width(&meta.session));
            max_layout = max_layout.max(display_width(&meta.layout));
            max_snapshot = max_snapshot.max(display_width(&meta.snapshot));
            max_recent = max_recent.max(display_width(&meta.recent));
        }
    }

    widths.push(max_workspace);
    if visible.session {
        widths.push(max_session);
    }
    if visible.layout {
        widths.push(max_layout);
    }
    if visible.snapshot {
        widths.push(max_snapshot);
    }
    if visible.recent {
        widths.push(max_recent);
    }
    widths
}

fn open_selector_format_column_header_row(
    plan: &OpenSelectorPlan,
    identity_header: &str,
    visible: &OpenSelectorVisibleColumns,
    widths: &[usize],
) -> String {
    use crate::table::pad_to_width;

    let headers = &plan.column_headers;
    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;
    parts.push(pad_to_width(identity_header, widths[idx]));
    idx += 1;
    if visible.session {
        parts.push(pad_to_width(&headers.session, widths[idx]));
        idx += 1;
    }
    if visible.layout {
        parts.push(pad_to_width(&headers.layout, widths[idx]));
        idx += 1;
    }
    if visible.snapshot {
        parts.push(pad_to_width(&headers.snapshot, widths[idx]));
        idx += 1;
    }
    if visible.recent {
        // The final column is intentionally not padded so trailing
        // whitespace does not consume the cursor highlight.
        parts.push(headers.recent.clone());
        idx += 1;
    }
    let _ = idx;
    parts.join("  ")
}

fn open_selector_format_data_row(
    row: &OpenSelectorRow,
    visible: &OpenSelectorVisibleColumns,
    widths: &[usize],
) -> String {
    use crate::table::pad_to_width;

    let mut parts: Vec<String> = Vec::new();
    let mut idx = 0;
    let any_after_workspace =
        visible.session || visible.layout || visible.snapshot || visible.recent;

    // Workspace column is always padded if any later column follows it
    // so the next column aligns even when this row has no enrichment.
    if any_after_workspace {
        parts.push(pad_to_width(&row.label, widths[idx]));
    } else {
        parts.push(row.label.clone());
    }
    idx += 1;

    // Rows without enrichment leave each later column blank but
    // padded, keeping all rows column-aligned under the header row.
    let blank = String::new();
    let meta = row.enrichment.as_ref();
    let session = meta.map(|m| m.session.as_str()).unwrap_or("");
    let layout = meta.map(|m| m.layout.as_str()).unwrap_or("");
    let snapshot = meta.map(|m| m.snapshot.as_str()).unwrap_or("");
    let recent = meta.map(|m| m.recent.as_str()).unwrap_or("");
    let _ = &blank;

    let last_visible_index = {
        let mut last = 0usize;
        let mut cursor = 1usize;
        if visible.session {
            last = cursor;
            cursor += 1;
        }
        if visible.layout {
            last = cursor;
            cursor += 1;
        }
        if visible.snapshot {
            last = cursor;
            cursor += 1;
        }
        if visible.recent {
            last = cursor;
        }
        last
    };

    let mut push_optional = |value: &str, want: bool, widths: &[usize], idx: &mut usize| {
        if want {
            if *idx == last_visible_index {
                parts.push(value.to_owned());
            } else {
                parts.push(pad_to_width(value, widths[*idx]));
            }
            *idx += 1;
        }
    };

    push_optional(session, visible.session, widths, &mut idx);
    push_optional(layout, visible.layout, widths, &mut idx);
    push_optional(snapshot, visible.snapshot, widths, &mut idx);
    push_optional(recent, visible.recent, widths, &mut idx);

    let _ = idx;
    parts.join("  ")
}

/// Headroom reserved for the selector cursor glyph in compact mode, so
/// a fitted summary row is not wrapped or clipped by the terminal.
const OPEN_COMPACT_RESERVE: usize = 4;

/// Render an enriched row as a compact single-line summary used when the
/// terminal is too narrow for the aligned table but the row still has
/// status to convey.
///
/// The workspace name and the session/running-status field are kept as a
/// pair (truncated with an ellipsis only when unavoidable); layout,
/// snapshot and recent are appended in that priority order while they
/// fit. A row without enrichment renders as its bare workspace name.
fn open_selector_format_compact_row(row: &OpenSelectorRow, budget: usize) -> String {
    use crate::table::{CompactSegment, compose_compact_summary};

    let meta = row.enrichment.as_ref();
    let session = meta.map(|m| m.session.as_str()).unwrap_or("");
    let layout = meta.map(|m| m.layout.as_str()).unwrap_or("");
    let snapshot = meta.map(|m| m.snapshot.as_str()).unwrap_or("");
    let recent = meta.map(|m| m.recent.as_str()).unwrap_or("");

    let segments = [
        CompactSegment {
            text: &row.label,
            required: true,
            min_width: 8,
        },
        CompactSegment {
            text: session,
            required: !session.is_empty(),
            min_width: 4,
        },
        CompactSegment {
            text: layout,
            required: false,
            min_width: 0,
        },
        CompactSegment {
            text: snapshot,
            required: false,
            min_width: 0,
        },
        CompactSegment {
            text: recent,
            required: false,
            min_width: 0,
        },
    ];
    compose_compact_summary(&segments, "  ", budget)
}

/// Render the selector plan to the parallel arrays consumed by
/// `select::run_with_disabled`, with column-header rows and aligned
/// data rows when the terminal can fit them.
///
/// Returns `(labels, disabled, actions)` where:
/// - `labels[i]` is the rendered display string for row `i`.
/// - `disabled[i] == true` marks row `i` as a non-selectable group
///   header, column-header, or empty-state hint.
/// - `actions[i]` is `Some(_)` for selectable rows and `None` for
///   disabled rows, so callers dispatch by index without parsing the
///   rendered label.
///
/// Layout, narrow → wide:
/// - Enriched rows in a terminal too narrow for the aligned session
///   column render as a compact summary that keeps the workspace name
///   plus its session/running status (and layout/snapshot/recent while
///   they fit), never a bare name. Compact mode emits no column-header
///   row.
/// - Wider → an aligned table that progressively re-enables session,
///   layout, snapshot, recent. Whenever any optional column is visible,
///   each populated section adds a disabled column-header row above its
///   data rows.
///
/// Rows that carry no status at all render as their bare workspace name
/// in every mode (there is nothing to summarize).
///
/// Section ordering (`项目根目录` → `Worktree 工作区` → create-new)
/// and disabled-row placement are preserved, so selection-mapping by
/// `actions` index stays valid.
pub(crate) fn render_open_selector_lines(
    plan: &OpenSelectorPlan,
    term_width: usize,
) -> (Vec<String>, Vec<bool>, Vec<Option<OpenSelectorAction>>) {
    let visible = decide_open_selector_columns(plan, term_width);
    let widths = compute_open_selector_column_widths(plan, &visible);

    // Compact summary mode: the plan has status to show, but the
    // terminal cannot fit the session column in aligned table form.
    let compact = plan_has_any_enrichment(plan) && !visible.session;
    let compact_budget = term_width.saturating_sub(OPEN_COMPACT_RESERVE);

    let mut labels: Vec<String> = Vec::new();
    let mut disabled: Vec<bool> = Vec::new();
    let mut actions: Vec<Option<OpenSelectorAction>> = Vec::new();

    let mut push = |label: String, action: Option<OpenSelectorAction>| {
        let is_disabled = action.is_none();
        labels.push(label);
        disabled.push(is_disabled);
        actions.push(action);
    };

    let format_row = |row: &OpenSelectorRow| -> String {
        if compact {
            open_selector_format_compact_row(row, compact_budget)
        } else {
            open_selector_format_data_row(row, &visible, &widths)
        }
    };

    // Project root section. Its identity column is the main branch, so
    // the header reads "Branch" rather than "Workspace".
    push(plan.group_root_header.clone(), None);
    if !compact && visible.any_visible() && plan.roots.iter().any(|r| r.enrichment.is_some()) {
        push(
            open_selector_format_column_header_row(
                plan,
                &plan.column_headers.branch,
                &visible,
                &widths,
            ),
            None,
        );
    }
    for row in &plan.roots {
        push(format_row(row), Some(row.action.clone()));
    }

    // Peer-section break: a blank, non-selectable line separates the
    // project-root section from the Worktree section so the two groups
    // read as distinct peers instead of one continuous list.
    push(String::new(), None);

    // Worktree section. Its identity column is the workspace name.
    push(plan.group_worktree_header.clone(), None);
    if plan.worktrees.is_empty() {
        push(plan.group_worktree_empty.clone(), None);
    } else {
        if !compact
            && visible.any_visible()
            && plan.worktrees.iter().any(|r| r.enrichment.is_some())
        {
            push(
                open_selector_format_column_header_row(
                    plan,
                    &plan.column_headers.workspace,
                    &visible,
                    &widths,
                ),
                None,
            );
        }
        for row in &plan.worktrees {
            push(format_row(row), Some(row.action.clone()));
        }
    }

    // Create-new action — always bare, never carries metadata.
    push(
        plan.create_new.label.clone(),
        Some(plan.create_new.action.clone()),
    );

    (labels, disabled, actions)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    /// Wide layout: every populated section gets a column-header row
    /// directly above its data rows. The columns are display-width
    /// aligned and the workspace name is the leading column on every
    /// row including the header. Built against the real localized
    /// catalog so the root row's label is the actual workspace name
    /// (`main`), not an i18n key placeholder.
    #[test]
    fn render_open_selector_lines_wide_inserts_column_headers_per_section() {
        use workhive_infra::i18n::StaticI18n;
        let i18n = StaticI18n::new();
        let worktrees = vec![worktree_info("main", true), worktree_info("feat-a", false)];

        let mut plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        decorate_open_selector_plan(&mut plan, &|action| match action {
            OpenSelectorAction::Root { .. } => Some(OpenSelectorRowMetadata {
                session: "running".to_owned(),
                layout: "3 panes".to_owned(),
                snapshot: "yes".to_owned(),
                recent: "5m".to_owned(),
            }),
            OpenSelectorAction::Worktree { .. } => Some(OpenSelectorRowMetadata {
                session: "no session".to_owned(),
                layout: "1 pane".to_owned(),
                snapshot: "no".to_owned(),
                recent: "-".to_owned(),
            }),
            _ => None,
        });

        // 200-column terminal: every metadata column fits, so both
        // sections render a column-header row above their data rows.
        let (labels, disabled, actions) = render_open_selector_lines(&plan, 200);

        // Expected sequence:
        //   0 root group header (disabled)
        //   1 root column header (disabled) — identity column is Branch
        //   2 root data row     (selectable)
        //   3 blank peer-break  (disabled)
        //   4 worktree group header (disabled)
        //   5 worktree column header (disabled) — identity is Workspace
        //   6 worktree data row  (selectable)
        //   7 create-new row     (selectable)
        assert_eq!(
            disabled,
            vec![true, true, false, true, true, true, false, false]
        );
        assert_eq!(labels[0], plan.group_root_header);
        assert!(
            labels[1].contains(&plan.column_headers.branch)
                && labels[1].contains(&plan.column_headers.session),
            "row 1 must be the project-root column-header row labelled Branch: {}",
            labels[1]
        );
        assert!(
            labels[2].starts_with("main"),
            "row 2 must lead with workspace name: {}",
            labels[2]
        );
        assert!(
            labels[2].contains("running") && labels[2].contains("3 panes"),
            "row 2 must surface session and layout: {}",
            labels[2]
        );
        assert!(
            labels[3].is_empty(),
            "a blank line must separate the two peer sections: {:?}",
            labels[3]
        );
        assert_eq!(labels[4], plan.group_worktree_header);
        assert!(
            labels[5].contains(&plan.column_headers.workspace),
            "row 5 must be the worktree column-header row labelled Workspace: {}",
            labels[5]
        );
        assert!(
            labels[6].starts_with("feat-a"),
            "row 6 must lead with workspace name: {}",
            labels[6]
        );

        let create_action = actions.last().and_then(Option::as_ref).cloned();
        assert_eq!(create_action, Some(OpenSelectorAction::CreateNew));
    }

    /// Narrow layout: when the aligned session column does not fit,
    /// enriched rows render as a compact summary that keeps the
    /// workspace name together with its session/running status — never
    /// a bare name. Compact mode emits no per-section column-header row,
    /// and selection mapping (by `actions` index) is preserved.
    #[test]
    fn render_open_selector_lines_narrow_keeps_workspace_and_status_compact() {
        use workhive_infra::i18n::StaticI18n;
        let i18n = StaticI18n::new();
        let worktrees = vec![worktree_info("main", true), worktree_info("feat-a", false)];

        let mut plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        decorate_open_selector_plan(&mut plan, &|action| match action {
            OpenSelectorAction::Root { .. } => Some(OpenSelectorRowMetadata {
                session: "running".to_owned(),
                layout: "3 panes".to_owned(),
                snapshot: "yes".to_owned(),
                recent: "5m".to_owned(),
            }),
            OpenSelectorAction::Worktree { .. } => Some(OpenSelectorRowMetadata {
                session: "no session".to_owned(),
                layout: "1 pane".to_owned(),
                snapshot: "no".to_owned(),
                recent: "-".to_owned(),
            }),
            _ => None,
        });

        // 24-column terminal: the aligned session column no longer
        // fits, so the renderer switches to compact summaries. The
        // budget still holds the workspace name and its status.
        let (labels, disabled, actions) = render_open_selector_lines(&plan, 24);
        // Expected sequence (no column-header rows in compact mode):
        //   0 root group header
        //   1 root data row (compact)
        //   2 blank peer-break
        //   3 worktree group header
        //   4 worktree data row (compact)
        //   5 create-new row
        assert_eq!(disabled, vec![true, false, true, true, false, false]);
        assert_eq!(labels[0], plan.group_root_header);
        assert!(
            labels[1].contains("main") && labels[1].contains("running"),
            "narrow root row must keep workspace name + running status: {:?}",
            labels[1]
        );
        assert!(
            labels[2].is_empty(),
            "a blank line must separate the two peer sections: {:?}",
            labels[2]
        );
        assert!(
            labels[4].contains("feat-a") && labels[4].contains("no session"),
            "narrow worktree row must keep workspace name + session status: {:?}",
            labels[4]
        );
        let create_action = actions.last().and_then(Option::as_ref).cloned();
        assert_eq!(create_action, Some(OpenSelectorAction::CreateNew));
    }

    /// Column-drop ordering: as the terminal shrinks, optional
    /// columns disappear in the order recent → snapshot → layout →
    /// session. The workspace column never drops.
    #[test]
    fn decide_open_selector_columns_drops_columns_right_to_left() {
        let i18n = FakeI18n;
        let worktrees = vec![worktree_info("main", true), worktree_info("feat-a", false)];

        let mut plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        decorate_open_selector_plan(&mut plan, &|_| {
            Some(OpenSelectorRowMetadata {
                session: "running".to_owned(),
                layout: "wide_split (3)".to_owned(),
                snapshot: "captured".to_owned(),
                recent: "12m".to_owned(),
            })
        });

        // Wide enough for every column.
        let wide = decide_open_selector_columns(&plan, 200);
        assert!(wide.session && wide.layout && wide.snapshot && wide.recent);

        // Progressively shrink. Visibility should fall right-to-left.
        let mut last = wide;
        for w in (10..200).rev().step_by(5) {
            let now = decide_open_selector_columns(&plan, w);
            // Once a column is hidden it must stay hidden as the
            // terminal continues to shrink (monotone decrease).
            if !last.recent {
                assert!(!now.recent, "recent must not reappear at width {w}");
            }
            if !last.snapshot {
                assert!(!now.snapshot, "snapshot must not reappear at width {w}");
            }
            if !last.layout {
                assert!(!now.layout, "layout must not reappear at width {w}");
            }
            if !last.session {
                assert!(!now.session, "session must not reappear at width {w}");
            }
            last = now;
        }
        // At the tightest widths, no optional column survives.
        let very_narrow = decide_open_selector_columns(&plan, 6);
        assert_eq!(very_narrow, OpenSelectorVisibleColumns::none());
    }

    /// Rows without metadata in a wide-layout render must still
    /// align under the column headers — they leave each downstream
    /// column blank but padded so the section reads as a coherent
    /// table.
    #[test]
    fn render_open_selector_lines_aligns_rows_without_metadata_under_headers() {
        use workhive_infra::i18n::StaticI18n;
        let i18n = StaticI18n::new();
        let worktrees = vec![worktree_info("main", true), worktree_info("feat-a", false)];

        let mut plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        decorate_open_selector_plan(&mut plan, &|action| match action {
            // Only the worktree row carries metadata; the project
            // root row stays bare. The project-root section must
            // skip the column-header row in that case so the bare
            // row is not crowned by a misleading header line.
            OpenSelectorAction::Worktree { .. } => Some(OpenSelectorRowMetadata {
                session: "running".to_owned(),
                layout: "1 pane".to_owned(),
                snapshot: "no".to_owned(),
                recent: "1m".to_owned(),
            }),
            _ => None,
        });

        let (labels, disabled, _) = render_open_selector_lines(&plan, 200);

        // Sequence: [root group header, root bare row, blank peer-break,
        // worktree group header, worktree column header, worktree row,
        // create-new row].
        assert_eq!(disabled, vec![true, false, true, true, true, false, false]);
        assert!(
            labels[1].starts_with("main"),
            "project root row stays bare: {:?}",
            labels[1]
        );
        assert!(
            labels[2].is_empty(),
            "a blank line must separate the two peer sections: {:?}",
            labels[2]
        );
        assert!(
            labels[4].contains(&plan.column_headers.workspace),
            "worktree section gets its own column header: {:?}",
            labels[4]
        );
    }

    /// Both locales must resolve the column-header copy used in the
    /// open selector — the localized catalog reuses the same column
    /// labels as the `workspace list` table so the two surfaces
    /// describe a workspace with identical terminology.
    #[test]
    fn open_selector_column_headers_are_localized() {
        use workhive_infra::i18n::StaticI18n;
        let i18n = StaticI18n::new();
        let plan_en = build_open_selector_plan("main", &[], &i18n, Locale::En);
        assert_eq!(plan_en.column_headers.workspace, "Workspace");
        assert_eq!(plan_en.column_headers.branch, "Branch");
        assert_eq!(plan_en.column_headers.session, "Session");
        assert_eq!(plan_en.column_headers.layout, "Layout");
        assert_eq!(plan_en.column_headers.snapshot, "Snapshot");
        assert_eq!(plan_en.column_headers.recent, "Last used");

        let plan_zh = build_open_selector_plan("main", &[], &i18n, Locale::ZhCn);
        assert_eq!(plan_zh.column_headers.workspace, "工作区");
        assert_eq!(plan_zh.column_headers.branch, "分支");
        assert_eq!(plan_zh.column_headers.session, "会话");
        assert_eq!(plan_zh.column_headers.layout, "布局");
        assert_eq!(plan_zh.column_headers.snapshot, "快照");
        assert_eq!(plan_zh.column_headers.recent, "最后使用");
    }
}

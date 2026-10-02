//! CJK-aware width utilities and the ASCII layout diagram/preview renderers.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// CJK-aware string width & padding (public utility for reuse)
// ══════════════════════════════════════════════════════════════════════════════

/// Display width in terminal columns, handling CJK double-width characters.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Pad a string to `target_width` display columns, handling CJK double-width.
pub fn pad_to(s: &str, target_width: usize) -> String {
    let w = display_width(s);
    if w >= target_width {
        s.to_owned()
    } else {
        format!("{}{}", s, " ".repeat(target_width - w))
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Dynamic ASCII diagram renderer (Column-based DSL)
// ══════════════════════════════════════════════════════════════════════════════

pub(super) const MIN_COL_INNER_WIDTH: usize = 14;

/// Render an ASCII box-drawing diagram for a column-based layout.
///
/// Each cell shows two lines: the pane label and size info.
/// Automatically adjusts column widths for CJK characters.
pub fn render_diagram(columns: &[ColumnConfig], i18n: &StaticI18n, locale: Locale) -> String {
    if columns.is_empty() {
        return String::new();
    }
    let labels = diagram_launch_labels(columns, i18n, locale);
    render_diagram_grid(columns, &labels)
}

/// Build the `[ci][ri]` matrix of pane labels from each pane's saved
/// launch, matching the labels the layout diagram renders for the
/// current configuration.
pub(super) fn diagram_launch_labels(
    columns: &[ColumnConfig],
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<Vec<String>> {
    columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            col.panes
                .iter()
                .enumerate()
                .map(|(ri, pane)| pane_label(ci, ri, &pane.launch, i18n, locale))
                .collect()
        })
        .collect()
}

/// Render the box-drawing layout diagram for `columns` using a
/// pre-computed `[ci][ri]` label matrix.
///
/// Keeping geometry driven by an explicit label matrix lets the live
/// command-selector preview reuse the exact same column sizing and
/// border drawing while substituting the candidate label for the pane
/// being configured.
pub(super) fn render_diagram_grid(columns: &[ColumnConfig], labels: &[Vec<String>]) -> String {
    if columns.is_empty() {
        return String::new();
    }

    let col_count = columns.len();
    let max_rows = columns.iter().map(|c| c.panes.len()).max().unwrap_or(1);

    let col_widths = compute_col_widths(columns, labels);

    let mut out = String::new();

    // Top border
    out.push_str("     ┌");
    for (ci, w) in col_widths.iter().enumerate() {
        if ci > 0 {
            out.push('┬');
        }
        out.push_str(&"─".repeat(*w));
    }
    out.push_str("┐\n");

    for row in 0..max_rows {
        if row > 0 {
            render_row_separator(&mut out, columns, &col_widths, row, col_count);
        }
        render_row_content(&mut out, columns, &col_widths, labels, row);
    }

    // Bottom border
    out.push_str("     └");
    for (ci, w) in col_widths.iter().enumerate() {
        if ci > 0 {
            out.push('┴');
        }
        out.push_str(&"─".repeat(*w));
    }
    out.push_str("┘\n");

    out
}

pub(super) fn compute_col_widths(columns: &[ColumnConfig], labels: &[Vec<String>]) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            let mut max_w = MIN_COL_INNER_WIDTH;
            for (ri, pane) in col.panes.iter().enumerate() {
                let label = &labels[ci][ri];
                let sl = size_label(col.width, pane.height, col.panes.len());
                // +2 for left/right padding spaces
                max_w = max_w.max(display_width(label) + 2);
                max_w = max_w.max(display_width(&sl) + 2);
            }
            max_w
        })
        .collect()
}

pub(super) fn render_row_separator(
    out: &mut String,
    columns: &[ColumnConfig],
    col_widths: &[usize],
    row: usize,
    col_count: usize,
) {
    out.push_str("     ");
    for (ci, w) in col_widths.iter().enumerate() {
        let has_row = row < columns[ci].panes.len();
        let joint = if ci == 0 {
            if has_row { "├" } else { "│" }
        } else {
            let left_has = row < columns[ci - 1].panes.len();
            match (left_has, has_row) {
                (true, true) => "┼",
                (true, false) => "┤",
                (false, true) => "├",
                (false, false) => "│",
            }
        };
        out.push_str(joint);
        if has_row {
            out.push_str(&"─".repeat(*w));
        } else {
            out.push_str(&" ".repeat(*w));
        }
    }
    let last_has = row < columns[col_count - 1].panes.len();
    out.push_str(if last_has { "┤\n" } else { "│\n" });
}

pub(super) fn render_row_content(
    out: &mut String,
    columns: &[ColumnConfig],
    col_widths: &[usize],
    labels: &[Vec<String>],
    row: usize,
) {
    // Label line
    out.push_str("     │");
    for (ci, col) in columns.iter().enumerate() {
        let inner_w = col_widths[ci];
        if row < col.panes.len() {
            let label = &labels[ci][row];
            out.push(' ');
            out.push_str(&pad_to(label, inner_w - 1));
            out.push('│');
        } else {
            out.push_str(&" ".repeat(inner_w));
            out.push('│');
        }
    }
    out.push('\n');

    // Size line
    out.push_str("     │");
    for (ci, col) in columns.iter().enumerate() {
        let inner_w = col_widths[ci];
        if row < col.panes.len() {
            let sl = size_label(col.width, col.panes[row].height, col.panes.len());
            out.push(' ');
            out.push_str(&pad_to(&sl, inner_w - 1));
            out.push('│');
        } else {
            out.push_str(&" ".repeat(inner_w));
            out.push('│');
        }
    }
    out.push('\n');
}

pub(super) fn pane_label(
    col: usize,
    row: usize,
    launch: &PaneLaunchRef,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    pane_label_with_display(col, row, &launch_display_name(launch, i18n, locale))
}

/// Format a pane label from its coordinate and an arbitrary display
/// string. Splitting the coordinate from the display lets the live
/// preview substitute a candidate command name (or a custom-input
/// placeholder) for the pane being configured while reusing the same
/// `C{col}R{row} {display}` shape as the saved-state diagram.
pub(super) fn pane_label_with_display(col: usize, row: usize, display: &str) -> String {
    format!("C{}R{} {}", col + 1, row + 1, display)
}

/// Marker prefixed to the pane currently being configured in the live
/// command-selector preview, so the active pane is visually distinct
/// from the others.
pub(super) const PREVIEW_CURRENT_PANE_MARKER: &str = "▸ ";

/// Candidate launch state the live preview should show for the pane
/// being configured, derived from the highlighted command option.
pub(super) enum PanePreviewCandidate {
    /// A resolved launch: a preset tool, an empty shell, or an inline
    /// command.
    Launch(PaneLaunchRef),
    /// The custom-command option is highlighted but no inline command
    /// has been entered yet, so the preview shows a manual-input
    /// placeholder for the current pane.
    CustomPending,
}

/// Derive the preview candidate for a highlighted catalog command.
///
/// A command with a resolved launch previews that launch; the
/// custom-command sentinel (no launch) previews a manual-input
/// placeholder until the operator types an inline command.
pub(super) fn pane_preview_candidate(command: &CatalogCommand) -> PanePreviewCandidate {
    match &command.launch {
        Some(launch) => PanePreviewCandidate::Launch(launch.clone()),
        None => PanePreviewCandidate::CustomPending,
    }
}

/// Display name the live preview shows for a candidate in the pane being
/// configured.
pub(super) fn pane_preview_candidate_display(
    candidate: &PanePreviewCandidate,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    match candidate {
        PanePreviewCandidate::Launch(launch) => launch_display_name(launch, i18n, locale),
        PanePreviewCandidate::CustomPending => i18n.resolve(locale, k::INIT_PANE_PREVIEW_CUSTOM),
    }
}

/// Build the `[ci][ri]` label matrix for the live preview while the pane
/// at (`cur_ci`, `cur_ri`) is being configured and the highlighted
/// command maps to `candidate`.
///
/// The current pane shows the candidate's display name and carries the
/// active-pane marker; every other pane keeps its saved launch label
/// unchanged, so moving the highlight never overwrites another pane's
/// state.
pub(super) fn pane_command_preview_labels(
    columns: &[ColumnConfig],
    cur_ci: usize,
    cur_ri: usize,
    candidate: &PanePreviewCandidate,
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<Vec<String>> {
    columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            col.panes
                .iter()
                .enumerate()
                .map(|(ri, pane)| {
                    if ci == cur_ci && ri == cur_ri {
                        let display = pane_preview_candidate_display(candidate, i18n, locale);
                        format!(
                            "{PREVIEW_CURRENT_PANE_MARKER}{}",
                            pane_label_with_display(ci, ri, &display)
                        )
                    } else {
                        pane_label(ci, ri, &pane.launch, i18n, locale)
                    }
                })
                .collect()
        })
        .collect()
}

/// Render the live preview diagram for the pane command selector as a
/// list of lines, reusing the layout diagram geometry with the current
/// pane substituted by the highlighted candidate.
pub(super) fn pane_command_preview_lines(
    columns: &[ColumnConfig],
    cur_ci: usize,
    cur_ri: usize,
    candidate: &PanePreviewCandidate,
    i18n: &StaticI18n,
    locale: Locale,
) -> Vec<String> {
    let labels = pane_command_preview_labels(columns, cur_ci, cur_ri, candidate, i18n, locale);
    render_diagram_grid(columns, &labels)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Widest preview content (in display columns) the current pane's
/// command options can produce, used to size the side preview so the
/// box never clips as the highlight moves between options of differing
/// label widths. Includes the two border columns.
pub(super) fn pane_command_preview_min_width(
    columns: &[ColumnConfig],
    cur_ci: usize,
    cur_ri: usize,
    available: &[CatalogCommand],
    i18n: &StaticI18n,
    locale: Locale,
) -> u16 {
    let content = available
        .iter()
        .map(|cmd| {
            let candidate = pane_preview_candidate(cmd);
            pane_command_preview_lines(columns, cur_ci, cur_ri, &candidate, i18n, locale)
                .iter()
                .map(|line| display_width(line))
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);
    u16::try_from(content.saturating_add(2)).unwrap_or(u16::MAX)
}

/// Preview box height (in rows) for the pane command selector,
/// including the two border rows. The diagram height depends only on
/// the column/row geometry, not on which candidate is highlighted, so
/// the current pane's saved launch is a stable basis.
pub(super) fn pane_command_preview_height(
    columns: &[ColumnConfig],
    cur_ci: usize,
    cur_ri: usize,
    i18n: &StaticI18n,
    locale: Locale,
) -> u16 {
    let candidate = PanePreviewCandidate::Launch(columns[cur_ci].panes[cur_ri].launch.clone());
    let rows = pane_command_preview_lines(columns, cur_ci, cur_ri, &candidate, i18n, locale).len();
    u16::try_from(rows.saturating_add(2)).unwrap_or(u16::MAX)
}

/// Map a `PaneLaunchRef` to its human-readable display name for the diagram.
///
/// - `EmptyShell` → localized "shell" label
/// - `Tool { name }` → looks up `BUILTIN_PANE_COMMANDS` for a localized label, else raw name
/// - `InlineCommand { spec }` → `program args...`
pub(super) fn launch_display_name(
    launch: &PaneLaunchRef,
    i18n: &StaticI18n,
    locale: Locale,
) -> String {
    match launch {
        PaneLaunchRef::EmptyShell => i18n.resolve(locale, k::INIT_DIAGRAM_SHELL),
        PaneLaunchRef::Tool { name } => {
            let name_str = name.as_str();
            if let Some(key) = tool_label_key(name_str) {
                return i18n.resolve(locale, key);
            }
            name_str.to_owned()
        }
        PaneLaunchRef::InlineCommand { spec } => {
            if spec.args().is_empty() {
                spec.program().to_owned()
            } else {
                format!("{} {}", spec.program(), spec.args().join(" "))
            }
        }
    }
}

pub(super) fn size_label(col_width: u8, pane_height: u8, pane_count: usize) -> String {
    if pane_count == 1 {
        format!("({}%)", col_width)
    } else {
        format!("({}%, {}%)", col_width, pane_height)
    }
}

//! Display-width-aware table formatter for CLI output.
//!
//! Handles CJK double-width characters correctly so that columns align
//! in terminals regardless of character width. All table output in
//! workhive CLI should use this module instead of raw `format!` padding.
//!
//! Supports compact mode: when terminal width is insufficient, columns
//! marked as droppable are hidden in priority order to fit the display.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// A column definition for table rendering.
pub struct Column {
    pub header: String,
    pub min_width: usize,
}

/// Render a table with display-width-aware column alignment.
///
/// - `columns`: column definitions (header + minimum width).
/// - `rows`: each row is a `Vec<String>` with one value per column.
/// - Output goes to stdout (data) with no stderr mixing.
///
/// The last column is never padded (avoids trailing whitespace).
pub fn print_table(columns: &[Column], rows: &[Vec<String>]) {
    for line in format_table_lines(columns, rows) {
        println!("{}", line);
    }
}

/// Return the same lines that [`print_table`] would emit, without
/// touching stdout. Used by higher-level renderers that need to
/// assemble multiple sections (peer-section breaks, custom headers)
/// or by unit tests that inspect the rendered output as plain
/// text. Each entry in the returned vector is a single line with
/// no trailing newline.
pub fn format_table_lines(columns: &[Column], rows: &[Vec<String>]) -> Vec<String> {
    format_table_lines_internal(columns, rows, &[])
}

#[allow(dead_code)]
/// Render a table with compact mode support.
///
/// `drop_order` lists column indices in the order they should be hidden
/// when terminal width is insufficient. The first index in the list is
/// dropped first, then the second, etc.
///
/// Detects terminal width via `terminal_size`. Falls back to full
/// rendering when width cannot be determined (e.g., piped output).
pub fn print_table_compact(columns: &[Column], rows: &[Vec<String>], drop_order: &[usize]) {
    render_table_internal(columns, rows, drop_order);
}

/// Detect terminal width, returning `None` when not connected to a TTY.
fn detect_terminal_width() -> Option<u16> {
    terminal_size::terminal_size().map(|(w, _)| w.0)
}

/// Detect the current terminal width in display columns.
///
/// Returns `None` when stdout is not connected to a TTY (e.g. piped to a
/// file or another process). Callers should fall back to a fixed default
/// (typically 120 or 80) so non-interactive renderings stay deterministic
/// without truncating randomly based on the parent process's window size.
#[allow(dead_code)]
pub fn detect_terminal_width_columns() -> Option<usize> {
    detect_terminal_width().map(|w| w as usize)
}

/// Compute total table width from column widths (including 2-char gaps).
fn total_table_width(widths: &[usize]) -> usize {
    if widths.is_empty() {
        return 0;
    }
    let col_sum: usize = widths.iter().sum();
    let gap_sum = (widths.len() - 1) * 2;
    col_sum + gap_sum
}

/// Core rendering logic shared by `print_table` and `print_table_compact`.
fn render_table_internal(columns: &[Column], rows: &[Vec<String>], drop_order: &[usize]) {
    for line in format_table_lines_internal(columns, rows, drop_order) {
        println!("{}", line);
    }
}

/// Pure formatter used by both stdout printers and tests. Builds
/// the header row, the unicode rule below it, and one row per data
/// entry, with the same column-hiding logic as the printer path.
fn format_table_lines_internal(
    columns: &[Column],
    rows: &[Vec<String>],
    drop_order: &[usize],
) -> Vec<String> {
    if columns.is_empty() {
        return Vec::new();
    }

    let hidden = compute_hidden_columns(columns, rows, drop_order);
    let visible: Vec<usize> = (0..columns.len()).filter(|i| !hidden.contains(i)).collect();

    if visible.is_empty() {
        return Vec::new();
    }

    let vis_columns: Vec<&Column> = visible.iter().map(|&i| &columns[i]).collect();
    let vis_rows: Vec<Vec<&str>> = rows
        .iter()
        .map(|row| {
            visible
                .iter()
                .map(|&i| row.get(i).map_or("", |v| v.as_str()))
                .collect()
        })
        .collect();

    let widths = compute_column_widths_refs(&vis_columns, &vis_rows);

    let mut out: Vec<String> = Vec::with_capacity(2 + vis_rows.len());
    out.push(format_row(
        &vis_columns
            .iter()
            .map(|c| c.header.as_str())
            .collect::<Vec<_>>(),
        &widths,
    ));

    let separators: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    out.push(format_row(
        &separators.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        &widths,
    ));

    for row in &vis_rows {
        out.push(format_row(row, &widths));
    }
    out
}

/// Determine which column indices to hide based on terminal width.
///
/// Returns empty vec when no columns need hiding.
pub fn compute_hidden_columns(
    columns: &[Column],
    rows: &[Vec<String>],
    drop_order: &[usize],
) -> Vec<usize> {
    if drop_order.is_empty() {
        return Vec::new();
    }

    let term_width = match detect_terminal_width() {
        Some(w) => w as usize,
        None => return Vec::new(),
    };

    let full_widths = compute_column_widths(columns, rows);
    let full_total = total_table_width(&full_widths);

    if full_total <= term_width {
        return Vec::new();
    }

    let mut hidden: Vec<usize> = Vec::new();
    for &drop_idx in drop_order {
        if drop_idx >= columns.len() {
            continue;
        }
        hidden.push(drop_idx);

        let remaining: Vec<usize> = (0..columns.len()).filter(|i| !hidden.contains(i)).collect();
        let rem_widths: Vec<usize> = remaining.iter().map(|&i| full_widths[i]).collect();
        if total_table_width(&rem_widths) <= term_width {
            break;
        }
    }

    hidden
}

/// Variant of `compute_hidden_columns` that accepts an explicit width
/// instead of querying the terminal. Used for deterministic testing.
#[cfg(test)]
pub fn compute_hidden_columns_with_width(
    columns: &[Column],
    rows: &[Vec<String>],
    drop_order: &[usize],
    term_width: usize,
) -> Vec<usize> {
    if drop_order.is_empty() {
        return Vec::new();
    }

    let full_widths = compute_column_widths(columns, rows);
    let full_total = total_table_width(&full_widths);

    if full_total <= term_width {
        return Vec::new();
    }

    let mut hidden: Vec<usize> = Vec::new();
    for &drop_idx in drop_order {
        if drop_idx >= columns.len() {
            continue;
        }
        hidden.push(drop_idx);

        let remaining: Vec<usize> = (0..columns.len()).filter(|i| !hidden.contains(i)).collect();
        let rem_widths: Vec<usize> = remaining.iter().map(|&i| full_widths[i]).collect();
        if total_table_width(&rem_widths) <= term_width {
            break;
        }
    }

    hidden
}

/// Compute the display width of a string, handling CJK double-width characters.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate `s` to at most `max_width` display columns.
///
/// When the string already fits, it is returned unchanged. When it is
/// wider, a leading prefix is kept and a single-column ellipsis (`…`)
/// is appended, so the result never exceeds `max_width` display
/// columns. Width-aware: CJK fullwidth glyphs count as two columns and
/// are never split across the boundary. A `max_width` of 0 yields an
/// empty string; a `max_width` of 1 yields just the ellipsis.
pub fn truncate_to_width(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    // Reserve one display column for the trailing ellipsis.
    let budget = max_width - 1;
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + w > budget {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}

/// One segment of a compact, single-line summary.
///
/// Segments are supplied in priority order. `required` segments always
/// appear in the rendered line (truncated with an ellipsis when the
/// budget is tight) so their information category is never dropped
/// entirely. Optional segments are appended only when they fit in full.
pub struct CompactSegment<'a> {
    /// Text to render for this segment.
    pub text: &'a str,
    /// When true, the segment is always included even if it must be
    /// truncated. When false, it is appended only if it fits in full.
    pub required: bool,
    /// Lowest display width to preserve for a required segment before
    /// other required segments are shrunk further. Ignored for optional
    /// segments.
    pub min_width: usize,
}

/// Compose a width-bounded, single-line compact summary from
/// prioritized segments joined by `sep`.
///
/// Contract:
/// - Every `required` segment appears in the output. When the combined
///   width exceeds `max_width`, required segments are shrunk
///   (largest-first, down to their `min_width` floor, then below it
///   only as a last resort) and truncated with an ellipsis, so no
///   required category is ever discarded outright.
/// - Optional segments are appended in order, each only when it fits in
///   full alongside everything already placed.
///
/// Width is measured in display columns, so CJK fullwidth glyphs are
/// accounted for and never split.
pub fn compose_compact_summary(
    segments: &[CompactSegment<'_>],
    sep: &str,
    max_width: usize,
) -> String {
    let sep_w = display_width(sep);

    let required: Vec<&CompactSegment<'_>> = segments.iter().filter(|s| s.required).collect();
    if required.is_empty() {
        return String::new();
    }

    let full: Vec<usize> = required.iter().map(|s| display_width(s.text)).collect();
    let floors: Vec<usize> = required
        .iter()
        .zip(&full)
        .map(|(s, &fw)| s.min_width.min(fw).max(1))
        .collect();
    let total_sep = sep_w * required.len().saturating_sub(1);

    // Start each required segment at its full width, then shrink the
    // currently-largest segment one column at a time until the line
    // fits its budget. Floors protect smaller segments first; only when
    // every segment has reached its floor do we shrink below it.
    let mut alloc = full.clone();
    loop {
        let line_w: usize = alloc.iter().sum::<usize>() + total_sep;
        if line_w <= max_width {
            break;
        }
        let mut target: Option<usize> = None;
        let mut best = 0usize;
        for (i, &a) in alloc.iter().enumerate() {
            if a > floors[i] && a > best {
                best = a;
                target = Some(i);
            }
        }
        match target {
            Some(i) => alloc[i] -= 1,
            None => break,
        }
    }
    // Last-resort shrink below floors for an extremely narrow terminal.
    loop {
        let line_w: usize = alloc.iter().sum::<usize>() + total_sep;
        if line_w <= max_width {
            break;
        }
        let mut target: Option<usize> = None;
        let mut best = 0usize;
        for (i, &a) in alloc.iter().enumerate() {
            if a > 1 && a > best {
                best = a;
                target = Some(i);
            }
        }
        match target {
            Some(i) => alloc[i] -= 1,
            None => break,
        }
    }

    let mut rendered: Vec<String> = required
        .iter()
        .zip(&alloc)
        .map(|(s, &w)| truncate_to_width(s.text, w))
        .collect();
    let mut used: usize = rendered.iter().map(|s| display_width(s)).sum::<usize>() + total_sep;

    // Append optional segments, in order, while each fits in full.
    for seg in segments.iter().filter(|s| !s.required) {
        if seg.text.is_empty() {
            continue;
        }
        let add = sep_w + display_width(seg.text);
        if used + add <= max_width {
            rendered.push(seg.text.to_owned());
            used += add;
        }
    }

    rendered.join(sep)
}

/// Pad a string to `target_width` display columns using spaces.
pub fn pad_to_width(s: &str, target_width: usize) -> String {
    let w = display_width(s);
    if w >= target_width {
        s.to_owned()
    } else {
        format!("{}{}", s, " ".repeat(target_width - w))
    }
}

fn compute_column_widths(columns: &[Column], rows: &[Vec<String>]) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let header_w = display_width(&col.header);
            let max_data_w = rows
                .iter()
                .map(|row| row.get(i).map_or(0, |v| display_width(v)))
                .max()
                .unwrap_or(0);
            header_w.max(max_data_w).max(col.min_width)
        })
        .collect()
}

fn compute_column_widths_refs(columns: &[&Column], rows: &[Vec<&str>]) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let header_w = display_width(&col.header);
            let max_data_w = rows
                .iter()
                .map(|row| row.get(i).map_or(0, |v| display_width(v)))
                .max()
                .unwrap_or(0);
            header_w.max(max_data_w).max(col.min_width)
        })
        .collect()
}

fn format_row(values: &[&str], widths: &[usize]) -> String {
    let last = values.len().saturating_sub(1);
    let parts: Vec<String> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            if i == last {
                (*v).to_owned()
            } else {
                pad_to_width(v, widths[i])
            }
        })
        .collect();
    parts.join("  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULLWIDTH_PAIR: &str = "ＡＢ";
    const FULLWIDTH_TRIPLE: &str = "ＡＢＣ";
    const FULLWIDTH_QUAD: &str = "ＡＢＣＤ";

    #[test]
    fn display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn display_width_cjk() {
        assert_eq!(display_width(FULLWIDTH_PAIR), 4);
        assert_eq!(display_width(FULLWIDTH_TRIPLE), 6);
    }

    #[test]
    fn display_width_mixed() {
        assert_eq!(display_width(&format!("hello{FULLWIDTH_PAIR}")), 9);
    }

    #[test]
    fn pad_to_width_ascii() {
        assert_eq!(pad_to_width("hi", 6), "hi    ");
    }

    #[test]
    fn pad_to_width_cjk() {
        assert_eq!(pad_to_width(FULLWIDTH_PAIR, 6), "ＡＢ  ");
    }

    #[test]
    fn pad_to_width_no_pad_when_wider() {
        assert_eq!(pad_to_width("hello", 3), "hello");
    }

    #[test]
    fn compute_widths_respects_header_and_data() {
        let cols = vec![
            Column {
                header: "Name".into(),
                min_width: 0,
            },
            Column {
                header: FULLWIDTH_TRIPLE.into(),
                min_width: 0,
            },
        ];
        let rows = vec![
            vec!["short".into(), "ab".into()],
            vec!["a-very-long-name".into(), FULLWIDTH_QUAD.into()],
        ];
        let widths = compute_column_widths(&cols, &rows);
        assert_eq!(widths[0], 16); // "a-very-long-name".len() == 16
        assert_eq!(widths[1], 8);
    }

    #[test]
    fn compute_widths_respects_min_width() {
        let cols = vec![Column {
            header: "X".into(),
            min_width: 10,
        }];
        let rows = vec![vec!["ab".into()]];
        let widths = compute_column_widths(&cols, &rows);
        assert_eq!(widths[0], 10);
    }

    // -- Compact mode tests --

    fn sample_columns() -> Vec<Column> {
        vec![
            Column {
                header: "Workspace".into(),
                min_width: 0,
            },
            Column {
                header: "Status".into(),
                min_width: 8,
            },
            Column {
                header: "Units".into(),
                min_width: 5,
            },
            Column {
                header: "Layout".into(),
                min_width: 0,
            },
            Column {
                header: "Session".into(),
                min_width: 0,
            },
            Column {
                header: "Path".into(),
                min_width: 0,
            },
        ]
    }

    fn sample_rows() -> Vec<Vec<String>> {
        vec![
            vec![
                "main (main)".into(),
                "active".into(),
                "3".into(),
                "ai-coding-left".into(),
                "wh@abc@proj@main".into(),
                "/Users/user/projects/myproject".into(),
            ],
            vec![
                "feat-auth".into(),
                "inactive".into(),
                "-".into(),
                "-".into(),
                "wh@abc@proj@feat-auth".into(),
                "/Users/user/projects/myproject/.workhive/worktrees/feat-auth".into(),
            ],
        ]
    }

    #[test]
    fn compact_no_drop_when_width_sufficient() {
        let cols = sample_columns();
        let rows = sample_rows();
        let hidden = compute_hidden_columns_with_width(&cols, &rows, &[5], 300);
        assert!(
            hidden.is_empty(),
            "no columns should be hidden with sufficient width"
        );
    }

    #[test]
    fn compact_drops_path_first_when_narrow() {
        let cols = sample_columns();
        let rows = sample_rows();
        // Path is col index 5; compute full width and subtract enough to require dropping
        let full_widths = compute_column_widths(&cols, &rows);
        let full_total = total_table_width(&full_widths);
        let narrow_width = full_total - full_widths[5];
        let hidden = compute_hidden_columns_with_width(&cols, &rows, &[5], narrow_width);
        assert_eq!(
            hidden,
            vec![5],
            "should hide Path (index 5) when width is tight"
        );
    }

    #[test]
    fn compact_drops_multiple_columns_progressively() {
        let cols = sample_columns();
        let rows = sample_rows();
        let full_widths = compute_column_widths(&cols, &rows);
        let full_total = total_table_width(&full_widths);
        // Make it so narrow that both Path (5) and Session (4) need to be dropped
        let very_narrow = full_total - full_widths[5] - full_widths[4] - 4;
        let hidden = compute_hidden_columns_with_width(&cols, &rows, &[5, 4], very_narrow);
        assert!(hidden.contains(&5), "should hide Path");
        assert!(
            hidden.contains(&4),
            "should hide Session when still too wide"
        );
    }

    #[test]
    fn compact_empty_drop_order_never_hides() {
        let cols = sample_columns();
        let rows = sample_rows();
        let hidden = compute_hidden_columns_with_width(&cols, &rows, &[], 10);
        assert!(
            hidden.is_empty(),
            "empty drop order means no columns can be hidden"
        );
    }

    #[test]
    fn total_table_width_calculation() {
        assert_eq!(total_table_width(&[10, 20, 30]), 10 + 20 + 30 + 2 * 2);
        assert_eq!(total_table_width(&[10]), 10);
        assert_eq!(total_table_width(&[]), 0);
    }
}

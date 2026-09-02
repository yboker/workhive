//! Simple selection screen — vertical list with arrow-key navigation.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

use crate::error::CliError;
use crate::ui::events::{UiAction, read_action};
use crate::ui::screens::text_fit::{display_width, truncate_display};
use crate::ui::session::{TerminalSession, modal_content_area};

/// Optional live preview rendered beside the selection list in wide
/// terminals.
///
/// `lines_for_row` produces the preview body (already rendered text
/// lines) for whichever row is currently highlighted, so the preview
/// refreshes as the operator moves the selection. `min_width` and
/// `min_height` describe the smallest box that renders the preview
/// without clipping; when the terminal is too narrow to satisfy both
/// the list and the preview, the preview is dropped and the list spans
/// the full width.
pub struct SidePreview<'a> {
    /// Title shown on the preview block border.
    pub title: String,
    /// Smallest preview box width (content + borders) that renders
    /// without clipping the widest possible body.
    pub min_width: u16,
    /// Smallest preview box height (content + borders).
    pub min_height: u16,
    /// Produce the preview body for the highlighted row.
    pub lines_for_row: Box<dyn Fn(usize) -> Vec<String> + 'a>,
}

/// Run a simple selection prompt.
///
/// Returns the zero-based index of the selected item.
pub fn run(title: &str, items: &[String], default: usize) -> Result<usize, CliError> {
    let disabled = vec![false; items.len()];
    run_with_disabled(title, items, &disabled, default)
}

/// Run a selection prompt with non-selectable rows.
///
/// `disabled[i] == true` marks row `i` as a non-selectable header
/// (e.g., a group label). Up/Down navigation skips disabled rows;
/// `Confirm` only succeeds on a selectable row. The visual rendering
/// applies a dim accent to disabled rows so operators can recognise
/// them as decoration, not as selectable items.
pub fn run_with_disabled(
    title: &str,
    items: &[String],
    disabled: &[bool],
    default: usize,
) -> Result<usize, CliError> {
    run_with_preview(title, items, disabled, default, None)
}

/// Run a selection prompt with non-selectable rows and an optional live
/// side preview.
///
/// Behaves exactly like [`run_with_disabled`] for navigation,
/// confirmation, abort, and resize repaint. When `preview` is present
/// and the terminal is wide enough, the list renders on the left and
/// the preview box renders on the right, refreshing for the highlighted
/// row. In narrow terminals the preview is dropped and the list spans
/// the full width so command labels stay readable.
pub fn run_with_preview(
    title: &str,
    items: &[String],
    disabled: &[bool],
    default: usize,
    preview: Option<SidePreview<'_>>,
) -> Result<usize, CliError> {
    let item_count = items.len();
    if item_count == 0 {
        return Err(CliError::Runtime("no items to select from".into()));
    }
    if disabled.len() != item_count {
        return Err(CliError::Runtime(
            "disabled mask length must match items length".into(),
        ));
    }
    let mut selectable: Vec<usize> = (0..item_count).filter(|i| !disabled[*i]).collect();
    if selectable.is_empty() {
        return Err(CliError::Runtime(
            "no selectable items to choose from".into(),
        ));
    }
    selectable.sort();

    let list_min_width = selector_list_min_width(items);

    // The bordered list renders in a modal alternate screen so a
    // terminal resize is handled by a full-frame redraw of a clean
    // canvas instead of an inline-viewport rebuild that left stale
    // borders and stacked list fragments behind. The list is anchored
    // at the top of the screen via `modal_content_area`, keeping the
    // compact look rather than stretching to fill the terminal.
    let list_height = (item_count as u16 + 2).min(20);
    let content_height = match &preview {
        Some(p) => list_height.max(p.min_height),
        None => list_height,
    };
    let mut session = TerminalSession::modal()?;
    let mut state = ListState::default();
    let initial = first_selectable_at_or_after(&selectable, default).unwrap_or(selectable[0]);
    state.select(Some(initial));

    loop {
        session
            .terminal()
            .draw(|frame| {
                let area = modal_content_area(frame.area(), content_height);
                let preview_min_width = preview.as_ref().map_or(0, |p| p.min_width);
                let layout = decide_selector_layout(area.width, list_min_width, preview_min_width);
                let (list_area, preview_area) = split_selector_area(area, &layout);
                render(frame, list_area, title, items, disabled, &mut state);
                if let (Some(p), Some(preview_area)) = (preview.as_ref(), preview_area) {
                    let selected = state.selected().unwrap_or(selectable[0]);
                    let lines = (p.lines_for_row)(selected);
                    render_preview(frame, preview_area, &p.title, &lines);
                }
            })
            .map_err(|e| CliError::Runtime(e.to_string()))?;

        match read_action()? {
            UiAction::Resize => {
                // A terminal resize is a layout event, not input. The
                // next loop iteration repaints the full alternate
                // screen and re-anchors the top content box, so no
                // stale borders or list fragments survive; the current
                // selection is preserved in `state`.
            }
            UiAction::Up => {
                let current = state.selected().unwrap_or(selectable[0]);
                let prev = previous_selectable(&selectable, current);
                state.select(Some(prev));
            }
            UiAction::Down => {
                let current = state.selected().unwrap_or(selectable[0]);
                let next = next_selectable(&selectable, current);
                state.select(Some(next));
            }
            UiAction::Confirm => {
                let current = state.selected().unwrap_or(selectable[0]);
                if !disabled[current] {
                    return Ok(current);
                }
                // Confirm on a header is a no-op; advance to the next
                // selectable row so Enter never silently picks a header.
                let next = next_selectable(&selectable, current);
                state.select(Some(next));
            }
            UiAction::Abort => return Err(CliError::UserAborted),
            _ => {}
        }
    }
}

/// Gutter (in columns) between the list and the side preview.
const SELECTOR_PREVIEW_GUTTER: u16 = 2;

/// Smallest list width (in columns) the layout will keep when also
/// showing a preview, so command labels never collapse to a few
/// unreadable characters.
const SELECTOR_LIST_MIN_WIDTH: u16 = 28;

/// Cap applied to the list width derived from the longest item, so a
/// single very long label does not force the preview off-screen.
const SELECTOR_LIST_MAX_WIDTH: u16 = 56;

/// Horizontal split decision between the selection list and an optional
/// side preview.
#[derive(Debug, PartialEq, Eq)]
enum SelectorLayout {
    /// The list spans the full content width (narrow terminal or no
    /// preview requested).
    ListOnly,
    /// The list and preview share the row, separated by a gutter.
    WithPreview {
        list_width: u16,
        gutter: u16,
        preview_width: u16,
    },
}

/// Minimum list box width that keeps the longest item readable,
/// bounded to a sensible range so the preview still has room.
fn selector_list_min_width(items: &[String]) -> u16 {
    let widest = items
        .iter()
        .map(|item| display_width(item))
        .max()
        .unwrap_or(0);
    // Cursor marker (2) + left/right padding and borders (~4).
    let derived = u16::try_from(widest.saturating_add(6)).unwrap_or(u16::MAX);
    derived.clamp(SELECTOR_LIST_MIN_WIDTH, SELECTOR_LIST_MAX_WIDTH)
}

/// Decide whether the preview fits beside the list for a given content
/// width. Pure so the wide/narrow fallback is unit-testable without a
/// terminal.
fn decide_selector_layout(total_width: u16, list_min: u16, preview_min: u16) -> SelectorLayout {
    if preview_min == 0 {
        return SelectorLayout::ListOnly;
    }
    let needed = list_min
        .saturating_add(SELECTOR_PREVIEW_GUTTER)
        .saturating_add(preview_min);
    if total_width < needed {
        return SelectorLayout::ListOnly;
    }
    let preview_width = preview_min;
    let list_width = total_width - SELECTOR_PREVIEW_GUTTER - preview_width;
    SelectorLayout::WithPreview {
        list_width,
        gutter: SELECTOR_PREVIEW_GUTTER,
        preview_width,
    }
}

/// Slice the content area into a list rect and an optional preview rect
/// per the layout decision.
fn split_selector_area(area: Rect, layout: &SelectorLayout) -> (Rect, Option<Rect>) {
    match *layout {
        SelectorLayout::ListOnly => (area, None),
        SelectorLayout::WithPreview {
            list_width,
            gutter,
            preview_width,
        } => {
            let list_area = Rect {
                width: list_width,
                ..area
            };
            let preview_area = Rect {
                x: area.x + list_width + gutter,
                width: preview_width,
                ..area
            };
            (list_area, Some(preview_area))
        }
    }
}

/// Render the side preview box: a bordered block whose body is the
/// pre-rendered preview lines for the highlighted row.
fn render_preview(frame: &mut Frame, area: Rect, title: &str, lines: &[String]) {
    let title_width = area.width.saturating_sub(4) as usize;
    let rendered_title = truncate_display(title, title_width);
    let body: Vec<Line> = lines
        .iter()
        .map(|l| Line::from(Span::raw(l.clone())))
        .collect();
    let paragraph = Paragraph::new(Text::from(body)).block(
        Block::default()
            .title(format!(" {} ", rendered_title))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(paragraph, area);
}

/// Return the smallest index in `selectable` that is `>= target`,
/// or `None` if no such index exists.
fn first_selectable_at_or_after(selectable: &[usize], target: usize) -> Option<usize> {
    selectable.iter().copied().find(|i| *i >= target)
}

/// Return the next selectable index after `current`, wrapping around.
///
/// Assumes `selectable` is non-empty and sorted.
fn next_selectable(selectable: &[usize], current: usize) -> usize {
    selectable
        .iter()
        .copied()
        .find(|i| *i > current)
        .unwrap_or(selectable[0])
}

/// Return the previous selectable index before `current`, wrapping around.
///
/// Assumes `selectable` is non-empty and sorted.
fn previous_selectable(selectable: &[usize], current: usize) -> usize {
    selectable
        .iter()
        .copied()
        .rev()
        .find(|i| *i < current)
        .unwrap_or_else(|| *selectable.last().expect("selectable is non-empty"))
}

fn render(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    disabled: &[bool],
    state: &mut ListState,
) {
    let title_width = area.width.saturating_sub(4) as usize;
    let item_width = area.width.saturating_sub(4) as usize;
    let rendered_title = truncate_display(title, title_width);
    let selected_index = state.selected();

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let is_header = disabled.get(i).copied().unwrap_or(false);
            ListItem::new(build_list_line(
                i,
                text,
                selected_index,
                item_width,
                is_header,
            ))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .title(format!(" {} ", rendered_title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(selected_row_highlight_style())
        .highlight_symbol("");

    frame.render_stateful_widget(list, area, state);
}

/// Build one rendered row for the selector list.
///
/// Splits the row into two spans so the selected row stays
/// readable on terminals where the background highlight does not
/// span the entire line:
/// - the leading cursor marker carries the strongest emphasis;
/// - the label text carries the inline accent style.
///
/// `selected_row_highlight_style` separately paints a full-row
/// background through `List::highlight_style`, giving the
/// selected row three converging signals (cursor marker, bold
/// accent text, full-row background).
fn build_list_line<'a>(
    index: usize,
    text: &'a str,
    selected_index: Option<usize>,
    item_width: usize,
    is_header: bool,
) -> Line<'a> {
    let is_selected = selected_index == Some(index) && !is_header;
    let cursor_marker = if is_selected { "▸ " } else { "  " };

    let cursor_style = if is_selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let text_style = if is_header {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD)
    } else if is_selected {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };

    let truncated = truncate_display(
        &format!("{cursor_marker}{text}"),
        item_width.max(display_width(cursor_marker)),
    );
    let (marker_part, label_part) = split_after_marker(&truncated, cursor_marker);

    Line::from(vec![
        Span::styled(marker_part.to_owned(), cursor_style),
        Span::styled(label_part.to_owned(), text_style),
    ])
}

/// Full-row highlight applied through `List::highlight_style`.
///
/// Ratatui paints this across the entire row width (background
/// included) for the row whose index matches `ListState::selected`.
/// We pair it with the inline cursor marker + bold accent so
/// selected-row prominence stays visible even on terminals that
/// suppress background colors.
pub(crate) fn selected_row_highlight_style() -> Style {
    Style::default()
        .bg(Color::DarkGray)
        .add_modifier(Modifier::BOLD)
}

fn split_after_marker<'a>(line: &'a str, marker: &str) -> (&'a str, &'a str) {
    if let Some(rest) = line.strip_prefix(marker) {
        let split_at = line.len() - rest.len();
        return line.split_at(split_at);
    }
    (line, "")
}

#[cfg(test)]
mod tests {
    use super::{build_list_line, selected_row_highlight_style, split_after_marker};
    use ratatui::style::{Color, Modifier};

    #[test]
    fn split_after_marker_separates_cursor_from_label() {
        let (marker, label) = split_after_marker("▸ Cursor Agent", "▸ ");
        assert_eq!(marker, "▸ ");
        assert_eq!(label, "Cursor Agent");
    }

    #[test]
    fn split_after_marker_returns_empty_label_when_marker_missing() {
        let (marker, label) = split_after_marker("abc", "▸ ");
        assert_eq!(marker, "abc");
        assert!(label.is_empty());
    }

    #[test]
    fn selected_row_uses_strong_emphasis_signals() {
        let line = build_list_line(1, "Cursor Agent (AI Agent)", Some(1), 80, false);
        // The selected row produces two spans: a cursor marker and the
        // label, both styled with the accent color so the row stays
        // recognizable even when the terminal suppresses the row
        // background highlight.
        assert_eq!(line.spans.len(), 2);

        let marker = &line.spans[0];
        assert!(marker.content.starts_with("▸ "));
        assert_eq!(marker.style.fg, Some(Color::Cyan));
        assert!(marker.style.add_modifier.contains(Modifier::BOLD));

        let label = &line.spans[1];
        assert_eq!(label.content, "Cursor Agent (AI Agent)");
        assert_eq!(label.style.fg, Some(Color::Cyan));
        assert!(label.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn unselected_row_uses_neutral_styling() {
        let line = build_list_line(0, "Codex (AI Agent)", Some(1), 80, false);
        assert_eq!(line.spans.len(), 2);

        let marker = &line.spans[0];
        assert!(marker.content.starts_with("  "));
        assert_eq!(marker.style.fg, Some(Color::DarkGray));
        assert!(!marker.style.add_modifier.contains(Modifier::BOLD));

        let label = &line.spans[1];
        assert_eq!(label.content, "Codex (AI Agent)");
        assert_eq!(label.style.fg, None);
        assert!(!label.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn selected_row_highlight_style_paints_full_row_background() {
        let style = selected_row_highlight_style();
        assert_eq!(style.bg, Some(Color::DarkGray));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn header_row_renders_dimmed_and_without_cursor_even_when_index_matches_state() {
        // Index 0 is a non-selectable group header. Even when the
        // selector's `state.selected()` happens to equal 0, the row
        // must render without the active cursor marker so operators
        // never mistake it for a selectable row.
        let line = super::build_list_line(0, "── Project root ──", Some(0), 80, true);
        assert_eq!(line.spans.len(), 2);

        let marker = &line.spans[0];
        assert!(
            marker.content.starts_with("  "),
            "header rows must not show the active `▸ ` cursor"
        );

        let label = &line.spans[1];
        assert_eq!(label.content, "── Project root ──");
        assert_eq!(
            label.style.fg,
            Some(Color::DarkGray),
            "header rows must render in the dim accent so they look like decoration"
        );
    }

    #[test]
    fn next_selectable_skips_disabled_rows() {
        // Layout: [header, item, header, item, item, header].
        let selectable = vec![1, 3, 4];
        assert_eq!(super::next_selectable(&selectable, 1), 3);
        assert_eq!(super::next_selectable(&selectable, 3), 4);
        // wrap-around: from the last selectable, return to the first.
        assert_eq!(super::next_selectable(&selectable, 4), 1);
    }

    #[test]
    fn previous_selectable_skips_disabled_rows() {
        let selectable = vec![1, 3, 4];
        assert_eq!(super::previous_selectable(&selectable, 4), 3);
        assert_eq!(super::previous_selectable(&selectable, 3), 1);
        // wrap-around: from the first selectable, return to the last.
        assert_eq!(super::previous_selectable(&selectable, 1), 4);
    }

    #[test]
    fn first_selectable_at_or_after_lands_on_default_when_selectable() {
        let selectable = vec![1, 3, 4];
        assert_eq!(super::first_selectable_at_or_after(&selectable, 0), Some(1));
        assert_eq!(super::first_selectable_at_or_after(&selectable, 3), Some(3));
        assert_eq!(super::first_selectable_at_or_after(&selectable, 5), None);
    }

    #[test]
    fn layout_drops_preview_when_no_preview_requested() {
        assert_eq!(
            super::decide_selector_layout(200, 30, 0),
            super::SelectorLayout::ListOnly
        );
    }

    #[test]
    fn layout_drops_preview_in_narrow_terminal() {
        // list_min(30) + gutter(2) + preview_min(40) = 72 needed; 60 is
        // too narrow, so the list spans the full width.
        assert_eq!(
            super::decide_selector_layout(60, 30, 40),
            super::SelectorLayout::ListOnly
        );
    }

    #[test]
    fn layout_shows_preview_and_gives_list_the_remainder_when_wide() {
        // 100 wide, list_min 30, preview_min 40: preview takes its
        // minimum, the gutter is reserved, the list gets the rest.
        let layout = super::decide_selector_layout(100, 30, 40);
        assert_eq!(
            layout,
            super::SelectorLayout::WithPreview {
                list_width: 100 - super::SELECTOR_PREVIEW_GUTTER - 40,
                gutter: super::SELECTOR_PREVIEW_GUTTER,
                preview_width: 40,
            }
        );
    }

    #[test]
    fn layout_shows_preview_exactly_at_the_minimum_width() {
        let needed = 30 + super::SELECTOR_PREVIEW_GUTTER + 40;
        let layout = super::decide_selector_layout(needed, 30, 40);
        assert!(
            matches!(layout, super::SelectorLayout::WithPreview { .. }),
            "preview must appear once the terminal is exactly wide enough: {layout:?}"
        );
    }

    #[test]
    fn list_min_width_is_clamped_to_a_readable_range() {
        // Empty list still keeps the floor.
        assert_eq!(
            super::selector_list_min_width(&[]),
            super::SELECTOR_LIST_MIN_WIDTH
        );
        // A very long label is capped so the preview keeps room.
        let long = vec!["x".repeat(200)];
        assert_eq!(
            super::selector_list_min_width(&long),
            super::SELECTOR_LIST_MAX_WIDTH
        );
    }

    #[test]
    fn split_area_reserves_gutter_between_list_and_preview() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 12,
        };
        let layout = super::SelectorLayout::WithPreview {
            list_width: 58,
            gutter: 2,
            preview_width: 40,
        };
        let (list_area, preview_area) = super::split_selector_area(area, &layout);
        let preview_area = preview_area.expect("preview rect present");
        assert_eq!(list_area.width, 58);
        assert_eq!(
            preview_area.x, 60,
            "preview must start after the list plus the gutter"
        );
        assert_eq!(preview_area.width, 40);
        assert_eq!(
            list_area.height, preview_area.height,
            "both panes share the content row height"
        );
    }

    #[test]
    fn split_area_gives_full_width_to_list_only_layout() {
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 12,
        };
        let (list_area, preview_area) =
            super::split_selector_area(area, &super::SelectorLayout::ListOnly);
        assert_eq!(list_area.width, 100);
        assert!(preview_area.is_none());
    }
}

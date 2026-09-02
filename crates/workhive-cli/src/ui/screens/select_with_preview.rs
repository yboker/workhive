//! Select-with-preview screen — side-by-side list + preview panel.
//!
//! Renders in a modal alternate screen via the shared
//! `TerminalSession`, anchored to a top content box. Layout uses
//! `Constraint::Percentage(45) + Constraint::Fill(1)` so the right
//! pane absorbs terminal-cell rounding gaps.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::error::CliError;
use crate::ui::events::{UiAction, read_action};
use crate::ui::screens::text_fit::truncate_display;
use crate::ui::session::{TerminalSession, modal_content_area};

/// An item in the selection list with an optional multi-line preview.
pub struct SelectItem {
    pub label: String,
    pub preview: String,
}

/// Run a selection prompt with a live preview panel.
///
/// Returns the zero-based index of the selected item.
pub fn run(
    title: &str,
    items: &[SelectItem],
    default: usize,
    viewport_height: u16,
    preview_title: &str,
) -> Result<usize, CliError> {
    if items.is_empty() {
        return Err(CliError::Runtime("no items to select from".into()));
    }

    // The list + side-by-side preview renders in a modal alternate
    // screen so a terminal resize is handled by a full-frame redraw of
    // a clean canvas instead of an inline-viewport rebuild that left
    // stale borders and stacked fragments behind. The content is
    // anchored at the top of the screen via `modal_content_area`,
    // keeping the compact look rather than stretching to fill the
    // terminal.
    let mut session = TerminalSession::modal()?;
    let item_count = items.len();
    let mut state = ListState::default();
    state.select(Some(default.min(item_count - 1)));

    loop {
        session
            .terminal()
            .draw(|frame| {
                let area = modal_content_area(frame.area(), viewport_height);
                render(frame, area, title, items, &mut state, preview_title);
            })
            .map_err(|e| CliError::Runtime(e.to_string()))?;

        match read_action()? {
            UiAction::Resize => {
                // A terminal resize is a layout event, not input. The
                // next loop iteration repaints the full alternate
                // screen and re-anchors the top content box, so no
                // stale borders or fragments survive; the current
                // selection is preserved.
            }
            UiAction::Up => {
                let i = state.selected().unwrap_or(0);
                state.select(Some(if i == 0 { item_count - 1 } else { i - 1 }));
            }
            UiAction::Down => {
                let i = state.selected().unwrap_or(0);
                state.select(Some(if i >= item_count - 1 { 0 } else { i + 1 }));
            }
            UiAction::Confirm => return Ok(state.selected().unwrap_or(default)),
            UiAction::Abort => return Err(CliError::UserAborted),
            _ => {}
        }
    }
}

fn render(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: &[SelectItem],
    state: &mut ListState,
    preview_title: &str,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Fill(1)])
        .split(area);

    render_list(frame, title, items, state, chunks[0]);
    render_preview(frame, items, state, chunks[1], preview_title);
}

fn render_list(
    frame: &mut Frame,
    title: &str,
    items: &[SelectItem],
    state: &mut ListState,
    area: Rect,
) {
    let title_width = area.width.saturating_sub(4) as usize;
    let item_width = area.width.saturating_sub(4) as usize;
    let rendered_title = truncate_display(title, title_width);
    let list_items: Vec<ListItem> = items
        .iter()
        .map(|item| {
            ListItem::new(Line::from(truncate_display(
                &format!(" {}", item.label),
                item_width.max(1),
            )))
        })
        .collect();

    let list = List::new(list_items)
        .block(
            Block::default()
                .title(format!(" {} ", rendered_title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸");

    frame.render_stateful_widget(list, area, state);
}

fn render_preview(
    frame: &mut Frame,
    items: &[SelectItem],
    state: &ListState,
    area: Rect,
    preview_title: &str,
) {
    let selected_idx = state.selected().unwrap_or(0);
    let preview_text = items
        .get(selected_idx)
        .map(|i| i.preview.as_str())
        .unwrap_or("");
    let rendered_preview_title =
        truncate_display(preview_title, area.width.saturating_sub(4) as usize);

    let preview = Paragraph::new(Text::from(preview_text))
        .block(
            Block::default()
                .title(format!(" {} ", rendered_preview_title))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(preview, area);
}

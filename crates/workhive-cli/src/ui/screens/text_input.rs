//! Text input screen — single-line text entry rendered inline.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::error::CliError;
use crate::ui::session::{TerminalSession, modal_content_area};

#[derive(Debug, Clone, PartialEq, Eq)]
enum TextInputAction {
    Insert(char),
    Paste(String),
    Confirm,
    Abort,
    Backspace,
    Delete,
    Left,
    Right,
    /// Terminal was resized — a layout event, handled by re-reserving
    /// the inline viewport, never as text input.
    Resize,
    Noop,
}

/// Run a single-line text input prompt.
///
/// Returns the entered text, or `default` if the user confirms an empty input.
pub fn run(prompt: &str, default: &str) -> Result<String, CliError> {
    let mut session = TerminalSession::modal()?;
    let mut buffer: Vec<char> = default.chars().collect();
    let mut cursor_pos = buffer.len();

    loop {
        let buf_str: String = buffer.iter().collect();
        session
            .terminal()
            .draw(|frame| {
                let height = text_input_viewport_height_for_width(prompt, frame.area().width);
                let area = modal_content_area(frame.area(), height);
                render(frame, area, prompt, &buf_str, cursor_pos, default);
            })
            .map_err(|e| CliError::Runtime(e.to_string()))?;

        match read_text_input_action()? {
            TextInputAction::Insert(c) => {
                buffer.insert(cursor_pos, c);
                cursor_pos += 1;
            }
            TextInputAction::Paste(text) => {
                for c in text.chars() {
                    buffer.insert(cursor_pos, c);
                    cursor_pos += 1;
                }
            }
            TextInputAction::Backspace if cursor_pos > 0 => {
                cursor_pos -= 1;
                buffer.remove(cursor_pos);
            }
            TextInputAction::Delete if cursor_pos < buffer.len() => {
                buffer.remove(cursor_pos);
            }
            TextInputAction::Left => {
                cursor_pos = cursor_pos.saturating_sub(1);
            }
            TextInputAction::Right if cursor_pos < buffer.len() => {
                cursor_pos += 1;
            }
            TextInputAction::Confirm => {
                let result: String = buffer.iter().collect();
                if result.is_empty() {
                    return Ok(default.to_string());
                }
                return Ok(result);
            }
            TextInputAction::Resize => {
                // The next modal frame recomputes wrapping from the
                // live terminal width while preserving buffer state.
            }
            TextInputAction::Abort => return Err(CliError::UserAborted),
            _ => {}
        }
    }
}

fn read_text_input_action() -> Result<TextInputAction, CliError> {
    loop {
        let ev = event::read().map_err(|e| CliError::Runtime(e.to_string()))?;

        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                return Ok(classify_text_input_key(key));
            }
            Event::Paste(text) => return Ok(TextInputAction::Paste(text)),
            // A resize is a layout event surfaced to the run loop so it
            // can re-reserve the inline viewport; it never edits text.
            Event::Resize(_, _) => return Ok(TextInputAction::Resize),
            _ => {}
        }
    }
}

fn classify_text_input_key(key: KeyEvent) -> TextInputAction {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return TextInputAction::Abort;
    }

    match key.code {
        KeyCode::Enter => TextInputAction::Confirm,
        KeyCode::Esc => TextInputAction::Abort,
        KeyCode::Left => TextInputAction::Left,
        KeyCode::Right => TextInputAction::Right,
        KeyCode::Backspace => TextInputAction::Backspace,
        KeyCode::Delete => TextInputAction::Delete,
        KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
            TextInputAction::Insert(c)
        }
        _ => TextInputAction::Noop,
    }
}

fn render(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    prompt: &str,
    value: &str,
    cursor: usize,
    default: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [prompt_area, input_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let input_width = input_area.width.saturating_sub(1) as usize;

    let display = if value.is_empty() && !default.is_empty() {
        default
    } else {
        value
    };
    let is_placeholder = value.is_empty() && !default.is_empty();

    let (before, cursor_char, after) = if !is_placeholder {
        let (visible, visible_cursor) = window_around_cursor(display, cursor, input_width);
        split_for_cursor(&visible, visible_cursor)
    } else {
        (display.to_string(), ' ', String::new())
    };

    let text_style = if is_placeholder {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    let line = Line::from(vec![
        Span::raw(" "),
        Span::styled(&before, text_style),
        Span::styled(
            cursor_char.to_string(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&after, text_style),
    ]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(" {} ", prompt))]))
            .wrap(Wrap { trim: true }),
        prompt_area,
    );
    frame.render_widget(Paragraph::new(line), input_area);
}

fn split_for_cursor(display: &str, cursor: usize) -> (String, char, String) {
    let char_len = display.chars().count();
    let safe_cursor = cursor.min(char_len);
    let cursor_byte = byte_index_at_char(display, safe_cursor);
    let next_byte = byte_index_at_char(display, safe_cursor.saturating_add(1));

    let before = display[..cursor_byte].to_string();
    let cursor_char = display[cursor_byte..].chars().next().unwrap_or(' ');
    let after = if next_byte >= display.len() {
        String::new()
    } else {
        display[next_byte..].to_string()
    };
    (before, cursor_char, after)
}

fn byte_index_at_char(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn text_input_viewport_height_for_width(prompt: &str, width: u16) -> u16 {
    let content_width = width.saturating_sub(4).max(20) as usize;
    text_input_viewport_height_for_content_width(prompt, content_width)
}

fn text_input_viewport_height_for_content_width(prompt: &str, content_width: usize) -> u16 {
    let prompt_lines = wrapped_line_count(prompt, content_width);
    (prompt_lines as u16 + 3).clamp(4, 12)
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    let safe_width = width.max(1);
    text.split('\n')
        .map(|line| {
            let line_width = UnicodeWidthStr::width(line);
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(safe_width)
            }
        })
        .sum::<usize>()
        .max(1)
}

fn window_around_cursor(display: &str, cursor: usize, width: usize) -> (String, usize) {
    if width <= 1 {
        return (" ".to_owned(), 0);
    }

    let chars: Vec<char> = display.chars().collect();
    let total = chars.len();
    let safe_cursor = cursor.min(total);
    if total == 0 {
        return (String::new(), 0);
    }

    let cursor_idx = safe_cursor.min(total.saturating_sub(1));
    let mut start = cursor_idx;
    let mut end = cursor_idx + 1;
    let mut used_width = char_display_width(chars[cursor_idx]);

    while start > 0 {
        let next_width = char_display_width(chars[start - 1]);
        if used_width + next_width > width {
            break;
        }
        start -= 1;
        used_width += next_width;
    }

    while end < total {
        let next_width = char_display_width(chars[end]);
        if used_width + next_width > width {
            break;
        }
        end += 1;
        used_width += next_width;
    }

    while start > 0 {
        let first_width = char_display_width(chars[start]);
        let next_width = char_display_width(chars[start - 1]);
        if used_width - first_width + next_width > width {
            break;
        }
        start -= 1;
        end -= 1;
        used_width = used_width - first_width + next_width;
    }

    let visible: String = chars[start..end].iter().collect();
    let visible_cursor = cursor_idx
        .saturating_sub(start)
        .min(visible.chars().count());
    (visible, visible_cursor)
}

fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{
        TextInputAction, classify_text_input_key, split_for_cursor,
        text_input_viewport_height_for_content_width, window_around_cursor, wrapped_line_count,
    };

    const FULLWIDTH_SAMPLE: &str = "ＡＢＣＤＥＦ";
    const LONG_FULLWIDTH_PROMPT: &str = "ＡＢＣＤＥＦＧＨ ";

    #[test]
    fn split_for_cursor_handles_cjk_safely() {
        let (before, cursor_char, after) = split_for_cursor("ＡaＢ", 1);
        assert_eq!(before, "Ａ");
        assert_eq!(cursor_char, 'a');
        assert_eq!(after, "Ｂ");
    }

    #[test]
    fn split_for_cursor_handles_emoji_safely() {
        let (before, cursor_char, after) = split_for_cursor("a🙂b", 1);
        assert_eq!(before, "a");
        assert_eq!(cursor_char, '🙂');
        assert_eq!(after, "b");
    }

    #[test]
    fn window_around_cursor_keeps_cursor_visible() {
        let (visible, cursor) = window_around_cursor("abcdefghijklmnopqrstuvwxyz", 20, 8);
        assert_eq!(visible.chars().count(), 8);
        assert!(cursor <= visible.chars().count());
        assert!(!visible.is_empty());
    }

    #[test]
    fn wrapped_line_count_handles_cjk_width() {
        assert_eq!(wrapped_line_count(FULLWIDTH_SAMPLE, 4), 3);
    }

    #[test]
    fn text_input_viewport_height_grows_for_long_prompt() {
        let prompt = LONG_FULLWIDTH_PROMPT.repeat(8);
        assert!(text_input_viewport_height_for_content_width(&prompt, 20) > 4);
    }

    #[test]
    fn text_input_keeps_selection_shortcut_letters_as_literal_text() {
        for ch in ['j', 'k', 'q', 'y', 'Y', 'n', 'N'] {
            let key = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
            assert_eq!(
                classify_text_input_key(key),
                TextInputAction::Insert(ch),
                "text input must insert shortcut character {ch:?} literally"
            );
        }
    }

    #[test]
    fn text_input_can_classify_cursor_agent_yolo_without_swallowing_letters() {
        let mut text = String::new();
        for ch in "cursor-agent --yolo".chars() {
            let key = KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE);
            match classify_text_input_key(key) {
                TextInputAction::Insert(c) => text.push(c),
                other => panic!("expected Insert({ch:?}), got {other:?}"),
            }
        }
        assert_eq!(text, "cursor-agent --yolo");
    }

    #[test]
    fn text_input_keeps_editing_and_abort_controls() {
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            TextInputAction::Confirm
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            TextInputAction::Abort
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            TextInputAction::Abort
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            TextInputAction::Left
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            TextInputAction::Right
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            TextInputAction::Backspace
        );
        assert_eq!(
            classify_text_input_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            TextInputAction::Delete
        );
    }
}

//! Candidate-list windowing and cursor text layout for the
//! path-input screen.
//!
//! Pure helpers that decide which slice of the candidate list is
//! visible, the scroll affordance per row, and how the editable
//! buffer is windowed around the cursor. Width-aware (CJK
//! fullwidth glyphs count as two columns).

use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CandidateWindow {
    pub(super) start: usize,
    pub(super) len: usize,
    pub(super) local_highlighted: Option<usize>,
}

pub(super) fn candidate_window(
    total: usize,
    highlighted: Option<usize>,
    max_visible: usize,
) -> CandidateWindow {
    if total == 0 || max_visible == 0 {
        return CandidateWindow {
            start: 0,
            len: 0,
            local_highlighted: None,
        };
    }

    let len = total.min(max_visible);
    let highlighted = highlighted.map(|idx| idx.min(total - 1));
    let start = highlighted
        .map(|idx| {
            if idx < len {
                0
            } else {
                (idx + 1).saturating_sub(len)
            }
        })
        .unwrap_or(0)
        .min(total.saturating_sub(len));
    let local_highlighted = highlighted.map(|idx| idx.saturating_sub(start));

    CandidateWindow {
        start,
        len,
        local_highlighted,
    }
}

pub(super) fn candidate_scroll_suffix(
    global_idx: usize,
    total: usize,
    window: CandidateWindow,
) -> &'static str {
    if total <= window.len {
        return "";
    }
    if global_idx == window.start && window.start > 0 {
        "  ↑"
    } else if global_idx + 1 == window.start + window.len && global_idx + 1 < total {
        "  ↓"
    } else {
        ""
    }
}

pub(super) fn split_for_cursor(display: &str, cursor: usize) -> (String, char, String) {
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

pub(super) fn byte_index_at_char(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

pub(super) fn window_around_cursor(display: &str, cursor: usize, width: usize) -> (String, usize) {
    if width <= 1 {
        return (" ".to_owned(), 0);
    }
    let chars: Vec<char> = display.chars().collect();
    let total = chars.len();
    if total == 0 {
        return (String::new(), 0);
    }

    let cursor_idx = cursor.min(total).min(total.saturating_sub(1));
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

    let visible: String = chars[start..end].iter().collect();
    let visible_cursor = cursor_idx
        .saturating_sub(start)
        .min(visible.chars().count());
    (visible, visible_cursor)
}

pub(super) fn char_display_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0).max(1)
}

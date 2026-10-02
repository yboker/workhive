//! Display-width-aware text fitting shared by selector screens.
//!
//! Selector list rows and panel titles must never overflow their
//! bordered area or split a CJK fullwidth glyph across the boundary.
//! Both the simple selector and the preview selector size their text
//! the same way, so the fitting decision lives here as a pure function
//! instead of being duplicated per screen. Low-level screens stay
//! presentational and call this to size their text.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display column width of `s`, counting CJK fullwidth glyphs as two
/// columns.
pub(crate) fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Truncate `text` to at most `max_width` display columns.
///
/// When the text already fits, it is returned unchanged. When it is
/// wider, a leading prefix is kept and a three-dot ellipsis (`...`) is
/// appended so the result never exceeds `max_width` display columns.
/// Width-aware: fullwidth glyphs count as two columns and are never
/// split across the boundary. A `max_width` of 0 yields an empty
/// string; a `max_width` of 1..=3 yields that many dots.
pub(crate) fn truncate_display(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    if display_width(text) <= max_width {
        return text.to_owned();
    }

    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let target = max_width - 3;
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if used + ch_width > target {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::truncate_display;

    #[test]
    fn truncate_display_adds_ellipsis_for_long_text() {
        assert_eq!(
            truncate_display("abcdefghijklmnopqrstuvwxyz", 8),
            "abcde..."
        );
    }

    #[test]
    fn truncate_display_preserves_short_text() {
        assert_eq!(truncate_display("short", 8), "short");
    }

    #[test]
    fn truncate_display_adds_ellipsis_for_preview_titles() {
        assert_eq!(
            truncate_display("very long preview title", 10),
            "very lo..."
        );
    }
}

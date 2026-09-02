//! Typed message severity / style helper for interactive TUI screens.
//!
//! Background:
//! before this module existed, individual `path_input` branches picked
//! `Color::Red` / `Color::Yellow` / `Color::Green` directly based on
//! ad-hoc bool flags. Blocking validation errors emitted from
//! `wizard.rs` bypassed the TUI entirely via `eprintln!`, so the user
//! had to scroll back through history to see what went wrong.
//!
//! This helper centralizes the styling decision into a single
//! `MessageSeverity` enum so:
//!
//! 1. Renderers query `severity.style()` instead of branching on the
//!    semantic source of the message.
//! 2. Plain / no-color output (test snapshots, terminals without ANSI
//!    color support) can derive a stable textual prefix from
//!    `severity.plain_prefix()` so error vs warning vs success stays
//!    visually distinct even when color is unavailable.
//! 3. New screens get a consistent palette by construction; we never
//!    have to grep for `Color::Red` again.
//!
//! The semantic palette:
//!
//! | Variant                        | Use                                              | Color (color-capable terminals) |
//! | ------------------------------ | ------------------------------------------------ | ------------------------------- |
//! | `Normal`                       | Plain text, neutral status                       | White                           |
//! | `Help`                         | Footer hints, secondary guidance                 | DarkGray                        |
//! | `Preview` / `Success`          | Positive preview, accepted action                | Green + Bold                    |
//! | `Warning`                      | Non-blocking caution / mid-typing guidance       | Yellow                          |
//! | `Danger`                       | Blocking validation failure, destructive danger  | Red + Bold                      |
//!
//! Styling MUST be chosen from a typed severity, not from matching
//! localized strings — `Color` decisions must stay language-agnostic.

use ratatui::style::{Color, Modifier, Style};

/// Typed severity for messages shown by interactive screens.
///
/// Picks the visual treatment based on the semantic role of the
/// message (blocking failure vs hint vs preview) rather than by
/// matching text. New screens should always go through this enum
/// instead of selecting a `Color` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSeverity {
    /// Plain, neutral status text. Use for body copy and labels that
    /// carry no special urgency.
    Normal,
    /// Secondary, low-urgency guidance such as footer hints. Renders
    /// dimmer than `Normal` so it does not compete with the main
    /// content.
    Help,
    /// Positive preview of an action the user is about to take, e.g.
    /// the `Will add: ~/Downloads` commit preview in `path_input`.
    Preview,
    /// Positive confirmation that an action succeeded, e.g. the most
    /// recently accepted trusted root in the added-roots summary.
    /// Shares the visual treatment with `Preview` because both carry
    /// the same "good outcome" semantics; the variant is kept
    /// separate so call sites stay self-documenting.
    ///
    /// Currently only consumed by tests in production builds — the
    /// trusted-roots renderer uses `Preview` for the most-recent
    /// highlight. Kept here as an explicit, self-documenting
    /// alternative for future call sites that want "this action
    /// completed" semantics instead of "this action is about to
    /// happen".
    #[cfg_attr(not(test), allow(dead_code))]
    Success,
    /// Non-blocking caution. The user may still submit, but should
    /// reconsider — e.g. a bare-scope buffer, a scoped-filter
    /// no-match hint, or the no-candidates Tab hint.
    Warning,
    /// Blocking validation failure or destructive danger. The
    /// referenced action will not proceed without correction.
    Danger,
}

impl MessageSeverity {
    /// Map a severity to a ratatui [`Style`] for color-capable
    /// terminals.
    ///
    /// We pick concrete colors here so call sites get a stable
    /// palette by construction. Renderers MUST use this helper
    /// instead of `Style::default().fg(Color::Red)` so the palette
    /// stays centralized.
    pub fn style(self) -> Style {
        match self {
            Self::Normal => Style::default().fg(Color::White),
            Self::Help => Style::default().fg(Color::DarkGray),
            Self::Preview | Self::Success => Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            Self::Warning => Style::default().fg(Color::Yellow),
            Self::Danger => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        }
    }

    /// Short textual prefix for plain / no-color renderings.
    ///
    /// Used by snapshot tests and by any non-color terminal so that
    /// `Danger` and `Warning` are still visually distinguishable
    /// from `Normal` / `Preview` / `Success` even without ANSI
    /// color. The prefix intentionally uses single-cell glyphs so
    /// the wider message remains aligned across rows.
    ///
    /// `Normal`, `Help`, `Preview`, and `Success` return an empty
    /// prefix because the surrounding context already carries enough
    /// signal — there is no ambiguity to disambiguate textually.
    ///
    /// Currently invoked only by tests; production renderers
    /// always have ANSI color available (TUI rendering path).
    /// Kept on the public API so non-TUI fallbacks (e.g. plain
    /// dump for snapshot diagnostics) can opt in without touching
    /// this module.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn plain_prefix(self) -> &'static str {
        match self {
            Self::Normal | Self::Help | Self::Preview | Self::Success => "",
            Self::Warning => "! ",
            Self::Danger => "x ",
        }
    }

    /// `true` for blocking failures. Used by render code that wants
    /// to know whether to bold the line / play an audible bell /
    /// reserve a row even when the message body is empty.
    ///
    /// Currently invoked only by tests; renderers branch on the
    /// `Option<String>` validation_error slot directly. Kept on
    /// the public API so future severity-driven layout helpers
    /// (e.g. always reserving a row for any `Danger` line) can use
    /// it without re-deriving the predicate.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_blocking(self) -> bool {
        matches!(self, Self::Danger)
    }
}

#[cfg(test)]
mod tests {
    use super::MessageSeverity;
    use ratatui::style::{Color, Modifier};

    #[test]
    fn danger_uses_red_bold_style() {
        let style = MessageSeverity::Danger.style();
        assert_eq!(
            style.fg,
            Some(Color::Red),
            "Danger MUST render in red so blocking validation \
             failures are unambiguous on color-capable terminals"
        );
        assert!(
            style.add_modifier.contains(Modifier::BOLD),
            "Danger MUST be bold so it stands out next to other \
             colored lines"
        );
    }

    #[test]
    fn warning_uses_yellow_style_without_bold() {
        let style = MessageSeverity::Warning.style();
        assert_eq!(style.fg, Some(Color::Yellow));
        assert!(
            !style.add_modifier.contains(Modifier::BOLD),
            "Warning must NOT inherit the Danger bold treatment so \
             non-blocking guidance is visually softer"
        );
    }

    #[test]
    fn preview_and_success_share_positive_palette() {
        let preview = MessageSeverity::Preview.style();
        let success = MessageSeverity::Success.style();
        assert_eq!(preview.fg, Some(Color::Green));
        assert_eq!(success.fg, Some(Color::Green));
        assert!(preview.add_modifier.contains(Modifier::BOLD));
        assert!(success.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn help_uses_dim_neutral_color() {
        let style = MessageSeverity::Help.style();
        assert_eq!(
            style.fg,
            Some(Color::DarkGray),
            "Help/footer hints must render dimmer than Normal so \
             they do not compete with primary content"
        );
    }

    #[test]
    fn normal_uses_neutral_color() {
        let style = MessageSeverity::Normal.style();
        assert_eq!(style.fg, Some(Color::White));
        assert!(!style.add_modifier.contains(Modifier::BOLD));
    }

    /// Plain/no-color fallback: blocking and cautionary severities
    /// MUST attach a textual prefix so terminals without ANSI color
    /// still surface the severity. Positive/neutral severities stay
    /// prefix-free because the surrounding copy carries the signal.
    #[test]
    fn plain_prefix_distinguishes_blocking_from_cautionary_from_neutral() {
        assert_eq!(MessageSeverity::Danger.plain_prefix(), "x ");
        assert_eq!(MessageSeverity::Warning.plain_prefix(), "! ");
        assert_eq!(MessageSeverity::Normal.plain_prefix(), "");
        assert_eq!(MessageSeverity::Help.plain_prefix(), "");
        assert_eq!(MessageSeverity::Preview.plain_prefix(), "");
        assert_eq!(MessageSeverity::Success.plain_prefix(), "");
    }

    /// `is_blocking` MUST be true only for `Danger` — `Warning`
    /// is intentionally non-blocking (the user may still submit but
    /// should reconsider).
    #[test]
    fn only_danger_is_blocking() {
        assert!(MessageSeverity::Danger.is_blocking());
        assert!(!MessageSeverity::Warning.is_blocking());
        assert!(!MessageSeverity::Preview.is_blocking());
        assert!(!MessageSeverity::Success.is_blocking());
        assert!(!MessageSeverity::Help.is_blocking());
        assert!(!MessageSeverity::Normal.is_blocking());
    }
}

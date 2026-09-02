//! Event translation for interactive UI.
//!
//! Translates raw crossterm events into typed UI actions.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::error::CliError;

/// Typed UI action returned from event processing.
pub enum UiAction {
    /// Move selection up (arrow up, k).
    Up,
    /// Move selection down (arrow down, j).
    Down,
    /// Confirm current selection (Enter).
    Confirm,
    /// User wants to abort (Esc, q, Ctrl+C).
    Abort,
    /// Backspace pressed.
    Backspace,
    /// Delete pressed.
    Delete,
    /// Move cursor left.
    Left,
    /// Move cursor right.
    Right,
    /// Toggle between options (Tab, Left/Right in confirm mode).
    Toggle,
    /// Affirmative response (y/Y).
    Yes,
    /// Negative response (n/N).
    No,
    /// Terminal was resized. This is a layout event, not a key press:
    /// the caller must clear and re-reserve its inline viewport so the
    /// prompt repaints cleanly, and must never treat it as a confirm,
    /// cancel, or text edit.
    Resize,
    /// No action for this event (ignore).
    Noop,
}

/// Read one event and translate it to a `UiAction`.
///
/// Blocks until a key press or a terminal resize is available.
/// A terminal resize returns [`UiAction::Resize`] so the caller can
/// repaint its inline viewport without mistaking the reflow for a key.
/// Ctrl+C is mapped to `UiAction::Abort`.
pub fn read_action() -> Result<UiAction, CliError> {
    loop {
        let ev = event::read().map_err(|e| CliError::Runtime(e.to_string()))?;

        match ev {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    return Ok(UiAction::Abort);
                }

                return Ok(classify_shortcut_key(key.code));
            }
            // A resize is a layout event, surfaced to the caller so it
            // can clear and re-reserve its inline viewport. Treating it
            // as a key press would risk a stray confirm/cancel.
            Event::Resize(_, _) => return Ok(UiAction::Resize),
            _ => {}
        }
    }
}

fn classify_shortcut_key(code: KeyCode) -> UiAction {
    match code {
        KeyCode::Up | KeyCode::Char('k') => UiAction::Up,
        KeyCode::Down | KeyCode::Char('j') => UiAction::Down,
        KeyCode::Enter => UiAction::Confirm,
        KeyCode::Esc | KeyCode::Char('q') => UiAction::Abort,
        KeyCode::Char('y') | KeyCode::Char('Y') => UiAction::Yes,
        KeyCode::Char('n') | KeyCode::Char('N') => UiAction::No,
        KeyCode::Tab => UiAction::Toggle,
        KeyCode::Left => UiAction::Left,
        KeyCode::Right => UiAction::Right,
        KeyCode::Backspace => UiAction::Backspace,
        KeyCode::Delete => UiAction::Delete,
        _ => UiAction::Noop,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyCode;

    use super::{UiAction, classify_shortcut_key};

    #[test]
    fn shortcut_key_mapping_keeps_selection_and_confirm_shortcuts() {
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('j')),
            UiAction::Down
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('k')),
            UiAction::Up
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('q')),
            UiAction::Abort
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('y')),
            UiAction::Yes
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('Y')),
            UiAction::Yes
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('n')),
            UiAction::No
        ));
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('N')),
            UiAction::No
        ));
    }

    #[test]
    fn shortcut_key_mapping_ignores_unreserved_text_keys() {
        assert!(matches!(
            classify_shortcut_key(KeyCode::Char('x')),
            UiAction::Noop
        ));
    }

    /// Esc must abort the prompt, not toggle, not noop.
    ///
    /// Pane-close confirmation requires q/Esc/Ctrl+C
    /// to abort promptly, so the keystroke mapping needs a
    /// regression test that catches accidentally re-routing Esc to
    /// e.g. `UiAction::Noop` or treating it as a literal character.
    #[test]
    fn esc_maps_to_abort() {
        assert!(matches!(
            classify_shortcut_key(KeyCode::Esc),
            UiAction::Abort
        ));
    }

    /// Enter must always map to Confirm regardless of the current
    /// selection. Combined with the confirm-screen's
    /// `next_confirm_decision` tests this gives a complete proof that
    /// pressing Enter takes the default highlighted choice and that
    /// pressing y/Y returns true even when the default is No.
    #[test]
    fn enter_maps_to_confirm() {
        assert!(matches!(
            classify_shortcut_key(KeyCode::Enter),
            UiAction::Confirm
        ));
    }
}

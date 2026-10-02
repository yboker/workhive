//! Provider terminal-status policies.
//!
//! The probe adapter captures terminal text; this module is the only
//! place that interprets provider-specific UI fragments.

use crate::domain::{AiActivityProvider, TerminalStatusObservation};

/// Interpret captured terminal UI for a known AI provider.
pub fn interpret_terminal_status(
    provider: AiActivityProvider,
    captured_text: &str,
) -> Option<TerminalStatusObservation> {
    match provider {
        AiActivityProvider::ClaudeCode => interpret_claude_code(captured_text),
        AiActivityProvider::Codex => interpret_codex(captured_text),
        AiActivityProvider::Opencode | AiActivityProvider::None => None,
    }
}

fn interpret_codex(text: &str) -> Option<TerminalStatusObservation> {
    let normalized = normalize_status_text(text);
    let has_working = normalized.lines().any(|line| line.trim() == "working");
    let has_interrupt_hint = normalized
        .lines()
        .any(|line| line.trim() == "esc to interrupt");
    if has_working || has_interrupt_hint {
        return Some(TerminalStatusObservation::working(
            AiActivityProvider::Codex,
        ));
    }
    None
}

fn interpret_claude_code(text: &str) -> Option<TerminalStatusObservation> {
    let normalized = normalize_status_text(text);
    let known_working = [
        "esc to interrupt",
        "ctrl-c to interrupt",
        "thinking",
        "working",
    ];
    if normalized.lines().any(|line| {
        let trimmed = line.trim();
        known_working.contains(&trimmed)
    }) {
        return Some(TerminalStatusObservation::working(
            AiActivityProvider::ClaudeCode,
        ));
    }
    None
}

fn normalize_status_text(text: &str) -> String {
    text.lines()
        .map(|line| line.trim().to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_working_ui_maps_to_terminal_status() {
        let signal =
            interpret_terminal_status(AiActivityProvider::Codex, "Working\nesc to interrupt");
        assert_eq!(
            signal,
            Some(TerminalStatusObservation::working(
                AiActivityProvider::Codex
            ))
        );
    }

    #[test]
    fn claude_code_working_ui_maps_to_terminal_status() {
        let signal = interpret_terminal_status(AiActivityProvider::ClaudeCode, "Thinking");
        assert_eq!(
            signal,
            Some(TerminalStatusObservation::working(
                AiActivityProvider::ClaudeCode
            ))
        );
    }

    #[test]
    fn opencode_unknown_returns_no_signal() {
        assert_eq!(
            interpret_terminal_status(AiActivityProvider::Opencode, "Working"),
            None
        );
    }

    #[test]
    fn unknown_text_returns_no_signal() {
        assert_eq!(
            interpret_terminal_status(AiActivityProvider::Codex, "ready for input"),
            None
        );
    }

    #[test]
    fn broad_substrings_do_not_match() {
        assert_eq!(
            interpret_terminal_status(AiActivityProvider::Codex, "not working anymore"),
            None
        );
        assert_eq!(
            interpret_terminal_status(AiActivityProvider::ClaudeCode, "prethinking"),
            None
        );
    }
}

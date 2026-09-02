//! Narrow retry state for interactive inputs that re-prompt after a
//! validation failure.
//!
//! Several wizard and command flows ask for a value, validate it, and
//! re-prompt on failure while staying in the same logical loop (trusted
//! root path entry, trusted-roots edit add-path, custom-layout numeric
//! counts). Those loops must preserve the rejected text as the next
//! prompt's initial buffer so the user can edit or complete it instead
//! of retyping, and must clear that text once a value is accepted or the
//! field is cancelled.
//!
//! This type is the single, typed expression of that contract so call
//! sites do not hand-roll their own preserve/clear bookkeeping. It holds
//! only retry state; the low-level input screens stay presentational and
//! receive the seed value through their existing `initial`/`default`
//! parameters. It is intentionally not a form framework: it owns one
//! field's retry value plus the localized feedback from the last
//! rejection.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidatedInputRetry {
    /// Text to seed the next prompt's input buffer. Empty until a
    /// submission is rejected; reset to empty once a value is accepted
    /// or the field is cancelled/cleared.
    initial: String,
    /// Localized blocking feedback from the previous rejected
    /// submission. Screens that render an in-frame error (e.g. path
    /// input) surface this; screens without an in-frame error slot may
    /// ignore it and print feedback separately. Cleared alongside the
    /// preserved value.
    feedback: Option<String>,
}

impl ValidatedInputRetry {
    /// Create empty retry state with no preserved value and no feedback.
    pub fn new() -> Self {
        Self::default()
    }

    /// Value to seed the next prompt's initial buffer. Returns `""`
    /// before any rejection and after the state is cleared.
    pub fn initial(&self) -> &str {
        &self.initial
    }

    /// Localized blocking feedback from the previous rejected
    /// submission, or `None` when there is nothing to surface.
    pub fn feedback(&self) -> Option<&str> {
        self.feedback.as_deref()
    }

    /// Record a rejected submission: preserve its raw text as the next
    /// initial buffer and store the localized feedback for display. A
    /// later rejection replaces both with the newest values.
    pub fn reject(&mut self, submitted: impl Into<String>, feedback: impl Into<String>) {
        self.initial = submitted.into();
        self.feedback = Some(feedback.into());
    }

    /// Clear the preserved value and feedback after an accepted
    /// submission, a cancel, or a completed step so the next prompt
    /// starts from an empty buffer.
    pub fn clear(&mut self) {
        self.initial.clear();
        self.feedback = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_initial_and_no_feedback() {
        let retry = ValidatedInputRetry::new();
        assert_eq!(retry.initial(), "");
        assert_eq!(retry.feedback(), None);
    }

    #[test]
    fn reject_preserves_submitted_value_as_next_initial() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop", "trusted roots must be absolute or start with ~/");
        assert_eq!(retry.initial(), "develop");
        assert_eq!(
            retry.feedback(),
            Some("trusted roots must be absolute or start with ~/")
        );
    }

    #[test]
    fn second_reject_replaces_preserved_value_with_newest() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop", "first error");
        retry.reject("./relative", "second error");
        assert_eq!(retry.initial(), "./relative");
        assert_eq!(retry.feedback(), Some("second error"));
    }

    #[test]
    fn clear_resets_initial_and_feedback_after_acceptance() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop", "some error");
        retry.clear();
        assert_eq!(retry.initial(), "");
        assert_eq!(retry.feedback(), None);
    }

    #[test]
    fn reject_after_clear_starts_fresh() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop", "error");
        retry.clear();
        retry.reject("3", "columns must be between 1 and 5");
        assert_eq!(retry.initial(), "3");
        assert_eq!(retry.feedback(), Some("columns must be between 1 and 5"));
    }
}

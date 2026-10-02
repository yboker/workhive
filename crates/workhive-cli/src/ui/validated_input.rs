//! Narrow retry state for interactive inputs that re-prompt after a
//! validation failure.
//!
//! Several wizard and command flows ask for a value, validate it, and
//! re-prompt on failure while staying in the same logical loop (trusted
//! roots edit add-path, custom-layout numeric counts). Those loops must
//! preserve the rejected text as the next prompt's initial buffer so the
//! user can edit or complete it instead of retyping, and must clear that
//! text once a value is accepted or the field is cancelled.
//!
//! This type is the single, typed expression of that contract so call
//! sites do not hand-roll their own preserve/clear bookkeeping. It holds
//! only retry state; the low-level input screens stay presentational and
//! receive the seed value through their existing `initial`/`default`
//! parameters. Call sites print their own localized rejection feedback
//! before re-prompting.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ValidatedInputRetry {
    /// Text to seed the next prompt's input buffer. Empty until a
    /// submission is rejected; reset to empty once a value is accepted
    /// or the field is cancelled/cleared.
    initial: String,
}

impl ValidatedInputRetry {
    /// Create empty retry state with no preserved value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Value to seed the next prompt's initial buffer. Returns `""`
    /// before any rejection and after the state is cleared.
    pub fn initial(&self) -> &str {
        &self.initial
    }

    /// Record a rejected submission: preserve its raw text as the next
    /// initial buffer. A later rejection replaces it with the newest
    /// value.
    pub fn reject(&mut self, submitted: impl Into<String>) {
        self.initial = submitted.into();
    }

    /// Clear the preserved value after an accepted submission, a cancel,
    /// or a completed step so the next prompt starts from an empty
    /// buffer.
    pub fn clear(&mut self) {
        self.initial.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_has_empty_initial() {
        let retry = ValidatedInputRetry::new();
        assert_eq!(retry.initial(), "");
    }

    #[test]
    fn reject_preserves_submitted_value_as_next_initial() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop");
        assert_eq!(retry.initial(), "develop");
    }

    #[test]
    fn second_reject_replaces_preserved_value_with_newest() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop");
        retry.reject("./relative");
        assert_eq!(retry.initial(), "./relative");
    }

    #[test]
    fn clear_resets_initial_after_acceptance() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop");
        retry.clear();
        assert_eq!(retry.initial(), "");
    }

    #[test]
    fn reject_after_clear_starts_fresh() {
        let mut retry = ValidatedInputRetry::new();
        retry.reject("develop");
        retry.clear();
        retry.reject("3");
        assert_eq!(retry.initial(), "3");
    }
}

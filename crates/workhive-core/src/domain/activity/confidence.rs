//! Confidence and freshness scales used by the activity control plane.
//!
//! These two enums answer separate questions:
//!
//! - [`ObservationConfidence`]: "how strong is this signal?"
//!   Confidence is a property of the **signal channel** (a Codex
//!   `notify` callback is medium for completion regardless of when
//!   it arrived; a tmux fallback is low even if observed seconds
//!   ago).
//!
//! - [`SignalFreshness`]: "is the signal still timely enough to
//!   trust as current state?" Freshness is a property of the
//!   projection clock — even a high-confidence hook event becomes
//!   `Stale` when it is old enough to no longer reflect what the
//!   pane is doing right now.
//!
//! Mixing them into a single scale would let the projector silently
//! coerce a stale hook into a fresh fallback or vice versa, which
//! we explicitly forbid in the requirement.

use serde::{Deserialize, Serialize};

/// How much trust the projector and decision policy may place in an
/// observation. The five levels match the requirement vocabulary.
///
/// `None` is reserved for the explicit "no signal" case (e.g. the
/// no-foreground-recognised-AI projection); it is **not** a fallback
/// for "signal exists but we cannot grade it".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationConfidence {
    /// No observation at all. Excluded from every drive-eligibility
    /// check.
    None,
    /// Best-effort tmux/process/title-parsing inference.
    Low,
    /// OS process observation, supervisor record without lifecycle
    /// confirmation, terminal-status parsing.
    MediumLow,
    /// Coarse provider notification (e.g. Codex `notify` completion).
    /// Trusted for the fact it carries (a turn finished) but **not**
    /// generalisable to "the agent is working".
    Medium,
    /// Provider lifecycle hook or plugin event delivered through a
    /// Workhive-managed integration.
    High,
}

impl ObservationConfidence {
    /// Whether this confidence level is allowed to drive
    /// orchestration-eligible decisions (waits, all-complete
    /// aggregation, attention routing). Per the requirement, only
    /// `High` qualifies. `Medium` is OK for completion evidence
    /// (e.g. notify-only flows) but the [`crate::domain::activity::DecisionPolicy`]
    /// layer applies that nuance, not this enum.
    pub fn is_orchestration_eligible(self) -> bool {
        matches!(self, Self::High)
    }
}

/// How fresh an event is relative to the projection clock.
///
/// `Stale` is recoverable: a fresh follow-up event upgrades the
/// projected state back to the new fact's freshness. `Expired` is a
/// stronger downgrade used when the event store carries data older
/// than any rendering policy will trust. `Unavailable` is for the
/// "we have no event at all in the relevant window" case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalFreshness {
    Fresh,
    Stale,
    Expired,
    Unavailable,
}

impl SignalFreshness {
    /// Decide freshness from an event's age (in milliseconds) given
    /// per-state policy windows. The two windows are:
    ///
    /// - `stale_after_ms`: above this age the event becomes `Stale`
    ///   — display can still mention what was reported but must
    ///   downgrade confidence and refuse orchestration.
    /// - `expired_after_ms`: above this age the event is
    ///   `Expired` — display falls back to "open_unverified" /
    ///   "stale" rendering.
    ///
    /// Both bounds must be positive and ordered (`stale <= expired`).
    /// On invalid bounds the function returns `Unavailable` so a
    /// misconfigured caller cannot accidentally pin a state to
    /// fresh.
    pub fn classify(age_ms: i64, stale_after_ms: i64, expired_after_ms: i64) -> Self {
        if age_ms < 0 || stale_after_ms <= 0 || expired_after_ms < stale_after_ms {
            return Self::Unavailable;
        }
        if age_ms >= expired_after_ms {
            Self::Expired
        } else if age_ms >= stale_after_ms {
            Self::Stale
        } else {
            Self::Fresh
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_ordering_matches_requirement_ladder() {
        // Per the requirement, confidence ladders strictly low ->
        // medium-low -> medium -> high; `None` is the absence of a
        // signal and sits below all real grades.
        assert!(ObservationConfidence::None < ObservationConfidence::Low);
        assert!(ObservationConfidence::Low < ObservationConfidence::MediumLow);
        assert!(ObservationConfidence::MediumLow < ObservationConfidence::Medium);
        assert!(ObservationConfidence::Medium < ObservationConfidence::High);
    }

    #[test]
    fn only_high_confidence_is_orchestration_eligible() {
        assert!(ObservationConfidence::High.is_orchestration_eligible());
        for lower in [
            ObservationConfidence::None,
            ObservationConfidence::Low,
            ObservationConfidence::MediumLow,
            ObservationConfidence::Medium,
        ] {
            assert!(
                !lower.is_orchestration_eligible(),
                "{lower:?} must NOT be orchestration-eligible (requirement Trust And Approval Boundaries)",
            );
        }
    }

    #[test]
    fn freshness_classify_is_fresh_below_stale_window() {
        assert_eq!(
            SignalFreshness::classify(5_000, 30_000, 90_000),
            SignalFreshness::Fresh
        );
    }

    #[test]
    fn freshness_classify_is_stale_in_stale_window() {
        assert_eq!(
            SignalFreshness::classify(45_000, 30_000, 90_000),
            SignalFreshness::Stale
        );
    }

    #[test]
    fn freshness_classify_is_expired_above_expired_window() {
        assert_eq!(
            SignalFreshness::classify(120_000, 30_000, 90_000),
            SignalFreshness::Expired
        );
    }

    #[test]
    fn freshness_classify_is_unavailable_for_invalid_bounds() {
        // Negative age: rejected.
        assert_eq!(
            SignalFreshness::classify(-1, 30_000, 90_000),
            SignalFreshness::Unavailable
        );
        // Zero stale window: rejected.
        assert_eq!(
            SignalFreshness::classify(10, 0, 90_000),
            SignalFreshness::Unavailable
        );
        // Inverted bounds (expired < stale): rejected.
        assert_eq!(
            SignalFreshness::classify(10, 90_000, 30_000),
            SignalFreshness::Unavailable
        );
    }
}

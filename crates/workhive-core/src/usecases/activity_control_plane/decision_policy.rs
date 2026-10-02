//! Orchestration / notification / metrics decision policy.
//!
//! The decision policy is the **single gate** between the
//! projected [`PaneActivityFact`] and any orchestration-eligible
//! action (waits, all-complete aggregation, attention routing,
//! metrics rollup). It centralises three independent concerns the
//! requirement keeps separate:
//!
//! 1. **Disabled-by-config**: when a feature gate (global or
//!    provider) reports `Disabled`, the policy MUST refuse the
//!    action regardless of any other signal — disabled means
//!    silent, no reads/writes/diagnostics/guidance.
//! 2. **Confidence floor**: only `High` confidence drives
//!    orchestration. `Medium` evidence is allowed *only* for
//!    `TurnNotifiedComplete` (notify-only completion is a
//!    legitimate trigger; medium evidence elsewhere is not).
//! 3. **Freshness floor**: stale/expired/unavailable freshness
//!    blocks orchestration even at `High` confidence.
//!
//! The policy is purposely small: a single
//! [`DecisionPolicy::evaluate`] call returning a typed
//! [`OrchestrationDecision`]. Callers branch on the decision
//! variant; they never re-derive the rules.

use crate::domain::activity::{
    ObservationConfidence, PaneActivityFact, PaneActivityState, SignalChannel, SignalFreshness,
};

/// Disabled / enabled state for a (global, provider) gate pair.
///
/// A single struct so the policy can resolve "globally on but
/// provider off" vs "globally off" without callers passing two
/// booleans in the wrong order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureGate {
    pub globally_enabled: bool,
    pub provider_enabled: bool,
}

impl FeatureGate {
    pub fn enabled(self) -> bool {
        self.globally_enabled && self.provider_enabled
    }

    pub fn fully_enabled() -> Self {
        Self {
            globally_enabled: true,
            provider_enabled: true,
        }
    }

    pub fn globally_disabled() -> Self {
        Self {
            globally_enabled: false,
            provider_enabled: true,
        }
    }

    pub fn provider_disabled() -> Self {
        Self {
            globally_enabled: true,
            provider_enabled: false,
        }
    }
}

/// Why the policy refused an orchestration-eligible action.
///
/// Variants are exhaustively distinct so callers / tests can
/// assert on the *reason*, not just on a boolean refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationRefusal {
    /// Global feature gate is off. No reads/writes/diagnostics.
    GloballyDisabled,
    /// Provider feature gate is off. Same suppression contract.
    ProviderDisabled,
    /// State is `Unknown` / `OpenUnverified` / `Disabled` /
    /// `Unsupported` / `Degraded`. Insufficient evidence to drive
    /// orchestration.
    NoUsableState,
    /// Confidence is below the orchestration floor and the event
    /// is not the special-cased medium-completion case.
    LowConfidence,
    /// Underlying signal is stale / expired / unavailable.
    StaleOrExpired,
}

/// Outcome of a [`DecisionPolicy::evaluate`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationDecision {
    /// Action is allowed.
    Allow,
    /// Action is refused; the caller MUST honour the refusal and
    /// suppress reads/writes/diagnostics/guidance per the
    /// associated reason.
    Refuse(OrchestrationRefusal),
}

impl OrchestrationDecision {
    pub fn is_allow(self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn refusal(self) -> Option<OrchestrationRefusal> {
        match self {
            Self::Refuse(r) => Some(r),
            Self::Allow => None,
        }
    }
}

/// Trait so non-default policies can be plugged in for tests or
/// future config-driven policies.
pub trait DecisionPolicy {
    fn evaluate(&self, fact: &PaneActivityFact, gate: FeatureGate) -> OrchestrationDecision;
}

/// Default Phase-1 decision policy.
///
/// The defaults match the requirement; bumping any of these
/// thresholds is a product-level decision.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultDecisionPolicy;

impl DecisionPolicy for DefaultDecisionPolicy {
    fn evaluate(&self, fact: &PaneActivityFact, gate: FeatureGate) -> OrchestrationDecision {
        // 1. Disabled-by-config wins over everything else.
        if !gate.globally_enabled {
            return OrchestrationDecision::Refuse(OrchestrationRefusal::GloballyDisabled);
        }
        if !gate.provider_enabled {
            return OrchestrationDecision::Refuse(OrchestrationRefusal::ProviderDisabled);
        }

        // 2. Inactive / not-yet-classified states cannot drive
        //    orchestration regardless of any other field.
        if !fact.state.is_orchestration_eligible() {
            return OrchestrationDecision::Refuse(OrchestrationRefusal::NoUsableState);
        }

        // 3. Freshness floor.
        if !matches!(fact.freshness, SignalFreshness::Fresh) {
            return OrchestrationDecision::Refuse(OrchestrationRefusal::StaleOrExpired);
        }

        // 4. Confidence floor — High is always allowed.
        if matches!(fact.confidence, ObservationConfidence::High) {
            return OrchestrationDecision::Allow;
        }

        // 5. Special case: medium-confidence notify-completion
        //    fact is the legitimate Codex notify-only completion
        //    flow. The state is `Completed` and the channel is
        //    `NotifyCallback`. Anything else at medium or below
        //    is refused.
        if matches!(fact.state, PaneActivityState::Completed)
            && matches!(fact.channel, SignalChannel::NotifyCallback)
            && matches!(fact.confidence, ObservationConfidence::Medium)
        {
            return OrchestrationDecision::Allow;
        }

        OrchestrationDecision::Refuse(OrchestrationRefusal::LowConfidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityEventId, ActivityOutcome, PaneCorrelationKey, ProviderKind, SignalChannel,
        SignalFreshness,
    };
    use uuid::Uuid;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    fn fact(
        state: PaneActivityState,
        channel: SignalChannel,
        confidence: ObservationConfidence,
        freshness: SignalFreshness,
    ) -> PaneActivityFact {
        PaneActivityFact {
            pane: pane(),
            provider: Some(ProviderKind::Codex),
            state,
            outcome: ActivityOutcome::NotApplicable,
            confidence,
            channel,
            observed_at_unix_ms: 1_000,
            freshness,
            source_event_id: Some(ActivityEventId(Uuid::nil())),
        }
    }

    /// Disabled global gate suppresses *every* orchestration
    /// decision regardless of how strong the signal is.
    #[test]
    fn globally_disabled_gate_refuses_even_high_confidence_fresh_signal() {
        let f = fact(
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::globally_disabled());
        assert_eq!(
            d,
            OrchestrationDecision::Refuse(OrchestrationRefusal::GloballyDisabled),
            "globally disabled MUST refuse even with strongest signal"
        );
    }

    /// Provider gate disabled refuses with a distinct typed reason
    /// so callers can render "this provider is off" guidance
    /// (when the user explicitly inspects providers).
    #[test]
    fn provider_disabled_gate_refuses_with_distinct_reason() {
        let f = fact(
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::provider_disabled());
        assert_eq!(
            d,
            OrchestrationDecision::Refuse(OrchestrationRefusal::ProviderDisabled),
        );
    }

    /// High-confidence fresh lifecycle event allows orchestration.
    #[test]
    fn high_confidence_fresh_lifecycle_event_allows_orchestration() {
        let f = fact(
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
        assert_eq!(d, OrchestrationDecision::Allow);
    }

    /// Notify-completion at medium confidence is allowed (this is
    /// the legitimate Codex notify-only completion flow).
    #[test]
    fn notify_completion_medium_confidence_completed_allows_orchestration() {
        let f = fact(
            PaneActivityState::Completed,
            SignalChannel::NotifyCallback,
            ObservationConfidence::Medium,
            SignalFreshness::Fresh,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
        assert_eq!(d, OrchestrationDecision::Allow);
    }

    /// Low/medium-low/observer-poll signals NEVER drive
    /// orchestration. Phase-1 invariant: low-confidence
    /// observer/supervisor/process/tmux signals refuse.
    #[test]
    fn low_and_medium_low_confidence_observer_signals_refuse_orchestration() {
        for (channel, confidence) in [
            (SignalChannel::ObserverPoll, ObservationConfidence::Low),
            (
                SignalChannel::SupervisorRecord,
                ObservationConfidence::MediumLow,
            ),
        ] {
            let f = fact(
                PaneActivityState::Working,
                channel,
                confidence,
                SignalFreshness::Fresh,
            );
            let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
            assert_eq!(
                d,
                OrchestrationDecision::Refuse(OrchestrationRefusal::LowConfidence),
                "low/medium-low signals via {channel:?} must refuse orchestration"
            );
        }
    }

    /// Stale freshness blocks orchestration even at High confidence.
    #[test]
    fn stale_signal_refuses_even_at_high_confidence() {
        let f = fact(
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Stale,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
        assert_eq!(
            d,
            OrchestrationDecision::Refuse(OrchestrationRefusal::StaleOrExpired)
        );
    }

    /// Unknown / OpenUnverified / Disabled / Unsupported /
    /// Degraded all refuse with `NoUsableState`.
    #[test]
    fn inactive_states_refuse_with_no_usable_state() {
        for state in [
            PaneActivityState::Unknown,
            PaneActivityState::OpenUnverified,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
            PaneActivityState::Stale,
        ] {
            let f = fact(
                state,
                SignalChannel::LifecycleHook,
                ObservationConfidence::High,
                SignalFreshness::Fresh,
            );
            let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
            assert_eq!(
                d,
                OrchestrationDecision::Refuse(OrchestrationRefusal::NoUsableState),
                "{state:?} must refuse with NoUsableState"
            );
        }
    }

    /// Notify-completion at low confidence (e.g. confidence
    /// downgraded by the projector) is **not** the legitimate
    /// completion flow and refuses.
    #[test]
    fn notify_completion_low_confidence_refuses() {
        let f = fact(
            PaneActivityState::Completed,
            SignalChannel::NotifyCallback,
            ObservationConfidence::Low,
            SignalFreshness::Fresh,
        );
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
        assert_eq!(
            d,
            OrchestrationDecision::Refuse(OrchestrationRefusal::LowConfidence)
        );
    }

    /// Disabled-then-enabled gate transitions: when the gate is
    /// turned back on, the policy resumes evaluating signals
    /// normally — disabled is not sticky beyond the gate state.
    #[test]
    fn disabled_gate_does_not_affect_subsequent_enabled_evaluations() {
        let f = fact(
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let _ = DefaultDecisionPolicy.evaluate(&f, FeatureGate::globally_disabled());
        let d = DefaultDecisionPolicy.evaluate(&f, FeatureGate::fully_enabled());
        assert_eq!(d, OrchestrationDecision::Allow);
    }
}

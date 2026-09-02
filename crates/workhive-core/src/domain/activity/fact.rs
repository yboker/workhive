//! Per-pane normalized fact stored in the projected read model.
//!
//! [`PaneActivityFact`] is the rendering-shaped projection produced
//! by the [`crate::usecases::activity_control_plane::StateProjector`]
//! from the append-only event log. It is **not** the durable
//! source of truth — the event log is. Loss of the read model is
//! recoverable by re-projecting the events.

use serde::{Deserialize, Serialize};

use super::confidence::{ObservationConfidence, SignalFreshness};
use super::event::{ActivityEventId, ActivityOutcome};
use super::kinds::{PaneCorrelationKey, ProviderKind, SignalChannel};
use super::state::PaneActivityState;

/// Projected per-pane activity fact. One per
/// `(group, workspace, unit_id)` tuple at any time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneActivityFact {
    /// Pane identity.
    pub pane: PaneCorrelationKey,
    /// Provider that produced the event the projection is anchored
    /// on. `None` means the projection produced an integration-state
    /// outcome (`Disabled` / `Unsupported` / `Degraded`) without
    /// pinning a specific provider event.
    pub provider: Option<ProviderKind>,
    /// Most recent normalized state.
    pub state: PaneActivityState,
    /// Outcome lens carried over from the latest decisive event
    /// (success/failure for completed/failed states; N/A otherwise).
    pub outcome: ActivityOutcome,
    /// Confidence at projection time. May be lower than the
    /// underlying event's recorded confidence if the projector
    /// downgraded for staleness.
    pub confidence: ObservationConfidence,
    /// Channel that produced the underlying event, retained so
    /// renderers can show "via notify" / "via lifecycle hook".
    pub channel: SignalChannel,
    /// Wall-clock timestamp of the underlying event.
    pub observed_at_unix_ms: i64,
    /// Freshness graded against the projection clock.
    pub freshness: SignalFreshness,
    /// Identifier of the underlying event for traceability.
    pub source_event_id: Option<ActivityEventId>,
}

impl PaneActivityFact {
    /// Construct the conservative "pane exists, no event yet"
    /// projection used when the event log has no entry for the
    /// pane and the foreground supervisor cannot prove a provider
    /// is open.
    pub fn unknown(pane: PaneCorrelationKey) -> Self {
        Self {
            pane,
            provider: None,
            state: PaneActivityState::Unknown,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::None,
            channel: SignalChannel::ObserverPoll,
            observed_at_unix_ms: 0,
            freshness: SignalFreshness::Unavailable,
            source_event_id: None,
        }
    }

    /// Construct an `OpenUnverified` projection: the supervisor
    /// proved a provider is open, but no event has been received
    /// yet.
    pub fn open_unverified(pane: PaneCorrelationKey, provider: ProviderKind) -> Self {
        Self {
            pane,
            provider: Some(provider),
            state: PaneActivityState::OpenUnverified,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::Low,
            channel: SignalChannel::SupervisorRecord,
            observed_at_unix_ms: 0,
            freshness: SignalFreshness::Fresh,
            source_event_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    #[test]
    fn unknown_constructor_is_conservative_and_carries_no_provider() {
        let f = PaneActivityFact::unknown(pane());
        assert_eq!(f.state, PaneActivityState::Unknown);
        assert!(f.provider.is_none(), "unknown fact must not pin a provider");
        assert_eq!(f.confidence, ObservationConfidence::None);
        assert_eq!(f.freshness, SignalFreshness::Unavailable);
        assert!(f.source_event_id.is_none());
    }

    #[test]
    fn open_unverified_constructor_records_supervisor_channel_and_provider() {
        let f = PaneActivityFact::open_unverified(pane(), ProviderKind::Codex);
        assert_eq!(f.state, PaneActivityState::OpenUnverified);
        assert_eq!(f.provider, Some(ProviderKind::Codex));
        assert_eq!(f.channel, SignalChannel::SupervisorRecord);
        assert_eq!(f.confidence, ObservationConfidence::Low);
    }

    #[test]
    fn fact_round_trips_through_serde() {
        let f = PaneActivityFact::open_unverified(pane(), ProviderKind::OpenCode);
        let json = serde_json::to_string(&f).unwrap();
        let back: PaneActivityFact = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }
}

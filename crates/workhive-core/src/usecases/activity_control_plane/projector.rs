//! Append-only event log -> projected [`ReadModel`].
//!
//! The projector is a **pure fold**: given an ordered slice of
//! events for a pane plus a projection clock + freshness windows,
//! it produces the corresponding [`PaneActivityFact`]. The
//! [`StateProjector`] convenience type wires the per-pane fold
//! into a full read-model rebuild from the
//! [`crate::ports::ActivityEventStore`].
//!
//! Phase-1 invariants enforced here:
//!
//! - **Append-only rebuild**: a `replay_all` over the event log
//!   produces the same read model as appending events
//!   incrementally, because the projector's fold is a pure
//!   function of the event sequence.
//! - **Freshness downgrade**: an event whose `observed_at_unix_ms`
//!   is older than `stale_after_ms` downgrades the projected
//!   confidence and changes the state to `Stale`. Older than
//!   `expired_after_ms` projects as `OpenUnverified` (we still
//!   know the provider was open, but the data is too old to
//!   trust).
//! - **Notify-completion does not imply working**: a stream that
//!   contains only a `TurnNotifiedComplete` projects to
//!   `Completed` with `Medium` confidence — never to `Working`
//!   for any earlier (unobserved) point in the turn.
//! - **Integration lifecycle wins over signals**: an
//!   `IntegrationDisabled` event projects to `Disabled` and
//!   suppresses the rest of the per-pane derivation. Same for
//!   `IntegrationUnsupported` -> `Unsupported`,
//!   `IntegrationDegraded` -> `Degraded`.

use std::collections::BTreeMap;

use crate::domain::activity::{
    ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, PaneActivityFact,
    PaneActivityState, PaneCorrelationKey, ReadModel, SignalFreshness,
};
use crate::ports::{ActivityEventStore, ActivityEventStoreError};

/// Project the events for a single pane into a
/// [`PaneActivityFact`].
///
/// `events` must be in append order (oldest first). Empty slices
/// project to `PaneActivityFact::unknown(pane)`.
pub fn project_pane_events(
    pane: &PaneCorrelationKey,
    events: &[ActivityEvent],
    now_ms: i64,
    stale_after_ms: i64,
    expired_after_ms: i64,
) -> PaneActivityFact {
    if events.is_empty() {
        return PaneActivityFact::unknown(pane.clone());
    }

    // Lifecycle events override the lifecycle dimension. We pick
    // the most recent one (last write wins for that dimension).
    let mut lifecycle: Option<&ActivityEvent> = None;
    for e in events.iter().rev() {
        if matches!(
            e.kind,
            ActivityEventKind::IntegrationDisabled
                | ActivityEventKind::IntegrationUnsupported
                | ActivityEventKind::IntegrationDegraded
        ) {
            lifecycle = Some(e);
            break;
        }
    }
    if let Some(ev) = lifecycle {
        return PaneActivityFact {
            pane: pane.clone(),
            provider: Some(ev.provider),
            state: lifecycle_to_state(ev.kind),
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::High,
            channel: ev.channel,
            observed_at_unix_ms: ev.observed_at_unix_ms,
            freshness: SignalFreshness::Fresh,
            source_event_id: Some(ev.id),
        };
    }

    // No lifecycle override: pick the latest non-telemetry event.
    let latest = events
        .iter()
        .rev()
        .find(|e| !matches!(e.kind, ActivityEventKind::TelemetryStale));
    let Some(latest) = latest else {
        // Only telemetry events on record: project as Stale.
        let stale = events.last().expect("non-empty");
        return PaneActivityFact {
            pane: pane.clone(),
            provider: Some(stale.provider),
            state: PaneActivityState::Stale,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::Low,
            channel: stale.channel,
            observed_at_unix_ms: stale.observed_at_unix_ms,
            freshness: SignalFreshness::Stale,
            source_event_id: Some(stale.id),
        };
    };

    let age_ms = now_ms.saturating_sub(latest.observed_at_unix_ms);
    let freshness = SignalFreshness::classify(age_ms, stale_after_ms, expired_after_ms);

    let (state, confidence) = match freshness {
        SignalFreshness::Fresh => (event_to_state(latest.kind), latest.confidence),
        SignalFreshness::Stale => (PaneActivityState::Stale, downgrade(latest.confidence)),
        SignalFreshness::Expired => (
            PaneActivityState::OpenUnverified,
            ObservationConfidence::Low,
        ),
        SignalFreshness::Unavailable => (PaneActivityState::Unknown, ObservationConfidence::None),
    };

    PaneActivityFact {
        pane: pane.clone(),
        provider: Some(latest.provider),
        state,
        outcome: latest.outcome,
        confidence,
        channel: latest.channel,
        observed_at_unix_ms: latest.observed_at_unix_ms,
        freshness,
        source_event_id: Some(latest.id),
    }
}

fn lifecycle_to_state(kind: ActivityEventKind) -> PaneActivityState {
    match kind {
        ActivityEventKind::IntegrationDisabled => PaneActivityState::Disabled,
        ActivityEventKind::IntegrationUnsupported => PaneActivityState::Unsupported,
        ActivityEventKind::IntegrationDegraded => PaneActivityState::Degraded,
        // Unreachable in practice — caller filters lifecycle kinds.
        _ => PaneActivityState::Unknown,
    }
}

fn event_to_state(kind: ActivityEventKind) -> PaneActivityState {
    match kind {
        ActivityEventKind::PromptSubmitted | ActivityEventKind::ToolStarted => {
            PaneActivityState::Working
        }
        ActivityEventKind::ToolFinished => PaneActivityState::Working,
        ActivityEventKind::PermissionRequested => PaneActivityState::WaitingForPermission,
        ActivityEventKind::PermissionResolved => PaneActivityState::Working,
        ActivityEventKind::TurnCompleted | ActivityEventKind::TurnNotifiedComplete => {
            PaneActivityState::Completed
        }
        ActivityEventKind::TurnFailed => PaneActivityState::Failed,
        ActivityEventKind::SessionIdle => PaneActivityState::Idle,
        ActivityEventKind::SessionStarted => PaneActivityState::OpenUnverified,
        ActivityEventKind::TelemetryStale => PaneActivityState::Stale,
        ActivityEventKind::IntegrationDisabled => PaneActivityState::Disabled,
        ActivityEventKind::IntegrationUnsupported => PaneActivityState::Unsupported,
        ActivityEventKind::IntegrationDegraded => PaneActivityState::Degraded,
    }
}

fn downgrade(c: ObservationConfidence) -> ObservationConfidence {
    match c {
        ObservationConfidence::High => ObservationConfidence::Medium,
        ObservationConfidence::Medium => ObservationConfidence::MediumLow,
        ObservationConfidence::MediumLow => ObservationConfidence::Low,
        ObservationConfidence::Low => ObservationConfidence::Low,
        ObservationConfidence::None => ObservationConfidence::None,
    }
}

/// Project a full event log (all panes mixed) into a [`ReadModel`].
///
/// Events are first grouped by pane, then projected pane-by-pane.
pub fn project_read_model(
    events: &[ActivityEvent],
    now_ms: i64,
    stale_after_ms: i64,
    expired_after_ms: i64,
) -> ReadModel {
    let mut grouped: BTreeMap<PaneCorrelationKey, Vec<&ActivityEvent>> = BTreeMap::new();
    for e in events {
        grouped.entry(e.pane.clone()).or_default().push(e);
    }
    let mut rm = ReadModel::empty(now_ms);
    for (pane, ev_refs) in grouped {
        let owned: Vec<ActivityEvent> = ev_refs.into_iter().cloned().collect();
        let fact = project_pane_events(&pane, &owned, now_ms, stale_after_ms, expired_after_ms);
        rm.upsert(fact);
    }
    rm
}

/// Convenience wrapper around an [`ActivityEventStore`].
///
/// Holds projection-clock + freshness windows so callers don't
/// repeat them at every call site.
pub struct StateProjector<'a> {
    store: &'a dyn ActivityEventStore,
    now_ms: i64,
    stale_after_ms: i64,
    expired_after_ms: i64,
}

impl<'a> StateProjector<'a> {
    pub fn new(
        store: &'a dyn ActivityEventStore,
        now_ms: i64,
        stale_after_ms: i64,
        expired_after_ms: i64,
    ) -> Self {
        Self {
            store,
            now_ms,
            stale_after_ms,
            expired_after_ms,
        }
    }

    /// Rebuild the full read model from the store's full event
    /// history. Surfaces typed errors from the store unchanged so
    /// callers can render `Unavailable`/`Degraded` instead of
    /// silently falling back to an empty model.
    pub fn rebuild(&self) -> Result<ReadModel, ActivityEventStoreError> {
        let events = self.store.replay_all()?;
        Ok(project_read_model(
            &events,
            self.now_ms,
            self.stale_after_ms,
            self.expired_after_ms,
        ))
    }

    /// Project a single pane's events.
    pub fn project_pane(
        &self,
        pane: &PaneCorrelationKey,
    ) -> Result<PaneActivityFact, ActivityEventStoreError> {
        let events = self.store.events_for_pane(pane)?;
        Ok(project_pane_events(
            pane,
            &events,
            self.now_ms,
            self.stale_after_ms,
            self.expired_after_ms,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, ProviderKind,
        ProviderSignalSource, SignalChannel,
    };

    fn pane(unit: &str) -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", unit)
    }

    fn ev(
        provider: ProviderKind,
        channel: SignalChannel,
        kind: ActivityEventKind,
        outcome: ActivityOutcome,
        confidence: ObservationConfidence,
        observed_at_unix_ms: i64,
    ) -> ActivityEvent {
        ActivityEvent::new(
            pane("u1"),
            provider,
            channel,
            ProviderSignalSource::LifecycleHook,
            kind,
            outcome,
            confidence,
            observed_at_unix_ms,
            None,
        )
    }

    #[test]
    fn empty_pane_events_project_to_unknown() {
        let f = project_pane_events(&pane("u1"), &[], 1_000, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Unknown);
        assert!(f.provider.is_none());
        assert_eq!(f.confidence, ObservationConfidence::None);
    }

    /// Phase-1 invariant: a notify-completion-only stream projects
    /// to `Completed` (medium confidence). It must NOT project to
    /// `Working` even though the turn obviously had work going on.
    #[test]
    fn notify_completion_only_projects_to_completed_not_working() {
        let events = vec![ev(
            ProviderKind::Codex,
            SignalChannel::NotifyCallback,
            ActivityEventKind::TurnNotifiedComplete,
            ActivityOutcome::Success,
            ObservationConfidence::Medium,
            1_000,
        )];
        let f = project_pane_events(&pane("u1"), &events, 1_500, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Completed);
        assert_eq!(f.confidence, ObservationConfidence::Medium);
        assert_ne!(f.state, PaneActivityState::Working);
        assert_ne!(f.state, PaneActivityState::ToolRunning);
    }

    /// Lifecycle hook tool-started projects to high-confidence
    /// `Working`.
    #[test]
    fn lifecycle_hook_tool_started_projects_to_working_high_confidence() {
        let events = vec![ev(
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            1_000,
        )];
        let f = project_pane_events(&pane("u1"), &events, 1_500, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Working);
        assert_eq!(f.confidence, ObservationConfidence::High);
    }

    /// Stale event downgrades confidence and projects to `Stale`.
    #[test]
    fn old_event_projects_to_stale_with_downgraded_confidence() {
        let events = vec![ev(
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            0,
        )];
        let f = project_pane_events(&pane("u1"), &events, 60_000, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Stale);
        assert_eq!(
            f.confidence,
            ObservationConfidence::Medium,
            "stale must downgrade High -> Medium"
        );
    }

    /// Expired event projects to `OpenUnverified` (we still know
    /// the provider was open, but the data is too old to trust).
    #[test]
    fn very_old_event_projects_to_open_unverified() {
        let events = vec![ev(
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            0,
        )];
        let f = project_pane_events(&pane("u1"), &events, 1_000_000, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::OpenUnverified);
        assert_eq!(f.confidence, ObservationConfidence::Low);
    }

    /// `IntegrationDisabled` lifecycle event wins over any other
    /// event in the log. Phase-1 requirement: disabled gates
    /// suppress everything else.
    #[test]
    fn integration_disabled_event_overrides_other_signals() {
        let events = vec![
            ev(
                ProviderKind::Codex,
                SignalChannel::LifecycleHook,
                ActivityEventKind::ToolStarted,
                ActivityOutcome::NotApplicable,
                ObservationConfidence::High,
                500,
            ),
            ev(
                ProviderKind::Codex,
                SignalChannel::SupervisorRecord,
                ActivityEventKind::IntegrationDisabled,
                ActivityOutcome::NotApplicable,
                ObservationConfidence::High,
                1_000,
            ),
        ];
        let f = project_pane_events(&pane("u1"), &events, 1_500, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Disabled);
    }

    /// Append-only rebuild contract: full replay produces the same
    /// per-pane fact as the per-pane fold did.
    #[test]
    fn read_model_rebuild_is_equivalent_to_per_pane_fold() {
        let p1 = pane("u1");
        let p2 = pane("u2");
        let mut events = vec![
            // pane u1: lifecycle Working
            ActivityEvent::new(
                p1.clone(),
                ProviderKind::ClaudeCode,
                SignalChannel::LifecycleHook,
                ProviderSignalSource::LifecycleHook,
                ActivityEventKind::ToolStarted,
                ActivityOutcome::NotApplicable,
                ObservationConfidence::High,
                100,
                None,
            ),
            // pane u2: notify completion
            ActivityEvent::new(
                p2.clone(),
                ProviderKind::Codex,
                SignalChannel::NotifyCallback,
                ProviderSignalSource::Notify,
                ActivityEventKind::TurnNotifiedComplete,
                ActivityOutcome::Success,
                ObservationConfidence::Medium,
                200,
                None,
            ),
        ];
        let rm = project_read_model(&events, 1_000, 30_000, 90_000);
        assert_eq!(rm.get(&p1).unwrap().fact.state, PaneActivityState::Working);
        assert_eq!(
            rm.get(&p2).unwrap().fact.state,
            PaneActivityState::Completed
        );
        // Reorder events: append-only contract says the latest
        // event still wins per pane regardless of insertion order
        // *between* panes.
        events.reverse();
        let rm2 = project_read_model(&events, 1_000, 30_000, 90_000);
        assert_eq!(rm.panes, rm2.panes);
    }

    /// `project_read_model` consumes the
    /// `(observed_at_unix_ms, id)`-sorted slice that
    /// [`crate::ports::ActivityEventStore::replay_all`] is required
    /// to produce. With that contract, the latest-by-timestamp
    /// event for a pane wins, regardless of which order the
    /// underlying store appended them. This pins the per-pane fold
    /// to the timestamp-ordered slice rather than the (irrelevant)
    /// raw append order.
    #[test]
    fn project_read_model_picks_latest_by_timestamp_when_input_is_timestamp_sorted() {
        let p = pane("u1");
        // Caller already provides `(observed_at_unix_ms, id)`-sorted
        // events, exactly as `replay_all` must do.
        let events = vec![
            ActivityEvent::new(
                p.clone(),
                ProviderKind::ClaudeCode,
                SignalChannel::LifecycleHook,
                ProviderSignalSource::LifecycleHook,
                ActivityEventKind::ToolStarted,
                ActivityOutcome::NotApplicable,
                ObservationConfidence::High,
                100,
                None,
            ),
            ActivityEvent::new(
                p.clone(),
                ProviderKind::ClaudeCode,
                SignalChannel::LifecycleHook,
                ProviderSignalSource::LifecycleHook,
                ActivityEventKind::TurnCompleted,
                ActivityOutcome::Success,
                ObservationConfidence::High,
                500,
                None,
            ),
        ];
        let rm = project_read_model(&events, 1_000, 30_000, 90_000);
        assert_eq!(
            rm.get(&p).unwrap().fact.state,
            PaneActivityState::Completed,
            "latest-by-timestamp event must dominate per-pane projection"
        );
    }

    /// Latest-event-wins per pane: a follow-up `TurnCompleted`
    /// upgrades the projection from `Working` to `Completed`.
    #[test]
    fn latest_event_per_pane_wins_in_projection() {
        let p = pane("u1");
        let events = vec![
            ActivityEvent::new(
                p.clone(),
                ProviderKind::ClaudeCode,
                SignalChannel::LifecycleHook,
                ProviderSignalSource::LifecycleHook,
                ActivityEventKind::ToolStarted,
                ActivityOutcome::NotApplicable,
                ObservationConfidence::High,
                100,
                None,
            ),
            ActivityEvent::new(
                p.clone(),
                ProviderKind::ClaudeCode,
                SignalChannel::LifecycleHook,
                ProviderSignalSource::LifecycleHook,
                ActivityEventKind::TurnCompleted,
                ActivityOutcome::Success,
                ObservationConfidence::High,
                200,
                None,
            ),
        ];
        let f = project_pane_events(&p, &events, 1_000, 30_000, 90_000);
        assert_eq!(f.state, PaneActivityState::Completed);
        assert_eq!(f.outcome, ActivityOutcome::Success);
    }
}

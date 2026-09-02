//! Projected pane / session read model contract.
//!
//! [`ReadModel`] is the projector's output: a snapshot keyed by
//! pane correlation, ready to be consumed by `pane list`,
//! `pane show`, dashboards, notifications, and orchestration. The
//! read model carries no provider-specific payload — every
//! provider-specific quirk has already been resolved into the
//! normalized [`PaneActivityFact`] vocabulary by the projector.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::fact::PaneActivityFact;
use super::kinds::PaneCorrelationKey;
use crate::usecases::activity_control_plane::metrics::{
    FreshnessThresholds, PaneActivityMetrics, compute_pane_metrics,
};

/// Per-pane projected activity row consumed by renderers and
/// orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneActivityReadModel {
    pub fact: PaneActivityFact,
}

/// Snapshot read model for the activity control plane. Keyed by
/// the pane correlation tuple to keep deterministic ordering.
///
/// `BTreeMap` (not `HashMap`) is intentional: deterministic
/// iteration order makes snapshot tests stable and lets the CLI
/// renderer produce reproducible output.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReadModel {
    /// Per-pane projected facts.
    pub panes: BTreeMap<PaneCorrelationKey, PaneActivityReadModel>,
    /// Wall-clock time at which the read model was projected, in
    /// Unix milliseconds. Used by renderers to compute "elapsed
    /// time in current state" without re-projecting.
    pub projected_at_unix_ms: i64,
}

impl ReadModel {
    /// Construct an empty read model anchored at `now_ms`.
    pub fn empty(now_ms: i64) -> Self {
        Self {
            panes: BTreeMap::new(),
            projected_at_unix_ms: now_ms,
        }
    }

    /// Insert or replace the fact for `pane`. Convenience used by
    /// the projector to build the model row by row.
    pub fn upsert(&mut self, fact: PaneActivityFact) {
        self.panes
            .insert(fact.pane.clone(), PaneActivityReadModel { fact });
    }

    /// Borrow the projected row for a pane.
    pub fn get(&self, pane: &PaneCorrelationKey) -> Option<&PaneActivityReadModel> {
        self.panes.get(pane)
    }

    /// Compute typed timing and freshness metrics for `pane` using
    /// the supplied event history and `now_ms` anchor.
    ///
    /// `events` must be the append-ordered slice for this pane (e.g.
    /// from [`crate::ports::ActivityEventStore::events_for_pane`]).
    /// Passing an empty slice produces `Unavailable` metrics with
    /// `reason: NoHistory`.
    ///
    /// ## ACP-preferred-with-legacy-fallback
    ///
    /// When `events` is empty **and** no projected fact exists for
    /// this pane, `acp_source = false` on the returned metrics and
    /// the caller is expected to check the legacy
    /// `PaneActivityStorePort` for a `PaneActivityFact` and render
    /// accordingly.
    ///
    /// Callers (Agent C, Agent D) must not reach back into raw
    /// events after calling this method — all necessary information
    /// is in the returned [`PaneActivityMetrics`].
    pub fn pane_metrics(
        &self,
        pane: &PaneCorrelationKey,
        events: &[crate::domain::activity::ActivityEvent],
        now_ms: i64,
        thresholds: FreshnessThresholds,
    ) -> PaneActivityMetrics {
        let projected_fact = self.get(pane).map(|row| &row.fact);
        compute_pane_metrics(pane, events, projected_fact, now_ms, thresholds)
    }
}

// `BTreeMap<PaneCorrelationKey, _>` requires `Ord` on the key. We
// derive it manually here (the auto-derive cannot reach across the
// module boundary because `PaneCorrelationKey` does not yet derive
// `Ord` — it is a `(group, workspace, unit_id)` tuple-shaped struct
// and lexicographic ordering is meaningful for renderers).
impl Ord for PaneCorrelationKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.group
            .cmp(&other.group)
            .then(self.workspace.cmp(&other.workspace))
            .then(self.unit_id.cmp(&other.unit_id))
    }
}

impl PartialOrd for PaneCorrelationKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, ProviderKind,
        ProviderSignalSource, SignalChannel, SignalFreshness,
    };
    use crate::usecases::activity_control_plane::metrics::{
        FreshnessThresholds, MetricValue, UnavailableReason,
    };

    const NOW: i64 = 1_700_000_100_000;

    fn pane(unit: &str) -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", unit)
    }

    fn thresholds() -> FreshnessThresholds {
        FreshnessThresholds::default_conservative()
    }

    fn make_event(p: &PaneCorrelationKey, kind: ActivityEventKind, at_ms: i64) -> ActivityEvent {
        ActivityEvent::new(
            p.clone(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            kind,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            at_ms,
            None,
        )
    }

    #[test]
    fn empty_read_model_is_anchored_at_now() {
        let rm = ReadModel::empty(1_700_000_000_000);
        assert_eq!(rm.projected_at_unix_ms, 1_700_000_000_000);
        assert!(rm.panes.is_empty());
    }

    #[test]
    fn upsert_replaces_existing_pane_row() {
        let mut rm = ReadModel::empty(0);
        let p = pane("u1");
        rm.upsert(PaneActivityFact::unknown(p.clone()));
        rm.upsert(PaneActivityFact::open_unverified(
            p.clone(),
            ProviderKind::Codex,
        ));
        let got = rm.get(&p).expect("pane row present");
        assert_eq!(got.fact.provider, Some(ProviderKind::Codex));
    }

    #[test]
    fn read_model_iterates_in_lexicographic_pane_order() {
        let mut rm = ReadModel::empty(0);
        for unit in ["u3", "u1", "u2"] {
            rm.upsert(PaneActivityFact::unknown(pane(unit)));
        }
        let keys: Vec<&str> = rm.panes.keys().map(|k| k.unit_id.as_str()).collect();
        assert_eq!(keys, vec!["u1", "u2", "u3"]);
    }

    /// When ACP event history is absent (empty slice) and no projected
    /// fact exists, the read model produces `acp_source = false` metrics.
    /// The caller is then responsible for checking the legacy
    /// `PaneActivityStorePort` latest-fact path as fallback.
    ///
    /// This test exercises the ACP-preferred-with-legacy-fallback contract
    /// described in the `pane_metrics` doc comment.
    #[test]
    fn read_model_falls_back_to_legacy_latest_fact_when_acp_absent() {
        let rm = ReadModel::empty(NOW);
        let p = pane("u1");

        // No events and no projected fact → acp_source = false.
        let metrics = rm.pane_metrics(&p, &[], NOW, thresholds());

        assert!(
            !metrics.acp_source,
            "acp_source must be false when no events exist (legacy fallback required)"
        );
        assert!(
            matches!(
                metrics.elapsed,
                MetricValue::Unavailable {
                    reason: UnavailableReason::NoHistory
                }
            ),
            "elapsed must be Unavailable/NoHistory when no ACP history, got {:?}",
            metrics.elapsed
        );
    }

    /// When ACP event history is present, the read model uses it as the
    /// authoritative source and sets `acp_source = true`. The legacy
    /// `PaneActivityStorePort` is NOT consulted.
    #[test]
    fn read_model_prefers_acp_over_legacy_when_acp_present() {
        let p = pane("u1");
        let event_at = NOW - 3_000;
        let events = vec![make_event(&p, ActivityEventKind::TurnCompleted, event_at)];

        let mut rm = ReadModel::empty(NOW);
        rm.upsert(PaneActivityFact {
            pane: p.clone(),
            provider: Some(ProviderKind::ClaudeCode),
            state: crate::domain::activity::PaneActivityState::Completed,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::High,
            channel: SignalChannel::LifecycleHook,
            observed_at_unix_ms: event_at,
            freshness: SignalFreshness::Fresh,
            source_event_id: None,
        });

        let metrics = rm.pane_metrics(&p, &events, NOW, thresholds());

        assert!(
            metrics.acp_source,
            "acp_source must be true when ACP events are present"
        );
        assert_eq!(
            metrics.provider_freshness_age,
            MetricValue::Known(std::time::Duration::from_millis(3_000)),
            "freshness age must reflect the 3s-old ACP event"
        );
    }
}

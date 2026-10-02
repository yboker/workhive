//! In-memory fake for [`ActivityEventStore`].
//!
//! The fake keeps an append-ordered `Vec<ActivityEvent>` per pane
//! plus a global flat list mirroring true append order. Tests use
//! it to drive the projector/decision-policy without touching the
//! infra-level JSONL adapter.
//!
//! Ordering contract (matches the port):
//!
//! - [`InMemoryActivityEventStore::events_for_pane`] returns per-pane
//!   append order (the literal flat list captured per pane).
//! - [`InMemoryActivityEventStore::replay_all`] returns events sorted
//!   by `(observed_at_unix_ms, id)`. This matches
//!   `FsActivityEventStore::replay_all` so the projector produces a
//!   bit-for-bit identical read model regardless of which adapter
//!   served the events.
//!
//! It also offers controlled error injection so tests can prove
//! the typed-error paths (corrupt entry / unavailable / I/O) wire
//! through the projector and into the rendered fact.

use std::cell::RefCell;
use std::collections::HashMap;

use workhive_core::domain::activity::{ActivityEvent, PaneCorrelationKey};
use workhive_core::ports::{ActivityEventStore, ActivityEventStoreError};

/// Error injection hook for the in-memory fake.
///
/// Not `Clone` because [`ActivityEventStoreError`] is not `Clone`.
/// Tests construct a fresh injection per setup which keeps the
/// surface ergonomic enough.
#[derive(Debug, Default)]
pub struct ActivityEventStoreErrorInjection {
    /// If `Some`, every `append` call returns the configured error.
    pub append_error: Option<ActivityEventStoreError>,
    /// If `Some`, every `events_for_pane` call returns the
    /// configured error.
    pub events_for_pane_error: Option<ActivityEventStoreError>,
    /// If `Some`, every `replay_all` call returns the configured
    /// error.
    pub replay_all_error: Option<ActivityEventStoreError>,
}

/// In-memory append-only fake.
pub struct InMemoryActivityEventStore {
    /// True append order, global across panes.
    flat: RefCell<Vec<ActivityEvent>>,
    /// Per-pane append order index. Keyed by the correlation tuple.
    by_pane: RefCell<HashMap<PaneCorrelationKey, Vec<ActivityEvent>>>,
    errors: RefCell<ActivityEventStoreErrorInjection>,
}

impl InMemoryActivityEventStore {
    pub fn new() -> Self {
        Self {
            flat: RefCell::new(Vec::new()),
            by_pane: RefCell::new(HashMap::new()),
            errors: RefCell::new(ActivityEventStoreErrorInjection::default()),
        }
    }

    /// Inject typed errors for test paths that need to prove
    /// degraded/unavailable behavior wires through the projector.
    pub fn set_error_injection(&self, injection: ActivityEventStoreErrorInjection) {
        *self.errors.borrow_mut() = injection;
    }

    /// Number of appended events.
    pub fn len(&self) -> usize {
        self.flat.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.flat.borrow().is_empty()
    }
}

impl Default for InMemoryActivityEventStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ActivityEventStore for InMemoryActivityEventStore {
    fn append(&self, event: &ActivityEvent) -> Result<(), ActivityEventStoreError> {
        if let Some(err) = self.errors.borrow().append_error.as_ref() {
            return Err(clone_error(err));
        }
        self.flat.borrow_mut().push(event.clone());
        self.by_pane
            .borrow_mut()
            .entry(event.pane.clone())
            .or_default()
            .push(event.clone());
        Ok(())
    }

    fn events_for_pane(
        &self,
        pane: &PaneCorrelationKey,
    ) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
        if let Some(err) = self.errors.borrow().events_for_pane_error.as_ref() {
            return Err(clone_error(err));
        }
        Ok(self.by_pane.borrow().get(pane).cloned().unwrap_or_default())
    }

    fn replay_all(&self) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
        if let Some(err) = self.errors.borrow().replay_all_error.as_ref() {
            return Err(clone_error(err));
        }
        // Deterministic `(observed_at_unix_ms, id)`
        // global order so the projector's output matches the
        // file-backed adapter regardless of append interleaving.
        // Per-pane append order remains observable through
        // [`Self::events_for_pane`].
        let mut events = self.flat.borrow().clone();
        events.sort_by_key(|e| (e.observed_at_unix_ms, e.id.0));
        Ok(events)
    }
}

/// `ActivityEventStoreError` is not `Clone` (it carries `String`s
/// but `thiserror` does not derive `Clone` for it). The fake
/// reconstructs the error variant for each injected return.
fn clone_error(err: &ActivityEventStoreError) -> ActivityEventStoreError {
    match err {
        ActivityEventStoreError::Io(s) => ActivityEventStoreError::Io(s.clone()),
        ActivityEventStoreError::CorruptEntry(s) => {
            ActivityEventStoreError::CorruptEntry(s.clone())
        }
        ActivityEventStoreError::Unavailable(s) => ActivityEventStoreError::Unavailable(s.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, ProviderKind,
        ProviderSignalSource, SignalChannel,
    };

    fn pane(unit: &str) -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", unit)
    }

    fn ev(unit: &str, observed_at: i64) -> ActivityEvent {
        ActivityEvent::new(
            pane(unit),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            observed_at,
            None,
        )
    }

    /// Append-only ordering contract: events for a single pane are
    /// returned in append order even when interleaved with another
    /// pane's events.
    #[test]
    fn events_for_pane_returns_events_in_append_order() {
        let store = InMemoryActivityEventStore::new();
        let a1 = ev("u1", 100);
        let b1 = ev("u2", 200);
        let a2 = ev("u1", 300);
        store.append(&a1).unwrap();
        store.append(&b1).unwrap();
        store.append(&a2).unwrap();

        let u1 = store.events_for_pane(&pane("u1")).unwrap();
        assert_eq!(u1.len(), 2);
        assert_eq!(u1[0].observed_at_unix_ms, 100);
        assert_eq!(u1[1].observed_at_unix_ms, 300);
        let u2 = store.events_for_pane(&pane("u2")).unwrap();
        assert_eq!(u2.len(), 1);
        assert_eq!(u2[0].observed_at_unix_ms, 200);
    }

    /// `replay_all` returns events in
    /// deterministic `(observed_at_unix_ms, id)` order. With
    /// append order that already matches timestamp order, the
    /// observed sequence is identical to append order.
    #[test]
    fn replay_all_returns_deterministic_observed_at_order_when_append_matches_timestamps() {
        let store = InMemoryActivityEventStore::new();
        let a = ev("u1", 100);
        let b = ev("u2", 200);
        let c = ev("u1", 300);
        store.append(&a).unwrap();
        store.append(&b).unwrap();
        store.append(&c).unwrap();
        let all = store.replay_all().unwrap();
        let units: Vec<&str> = all.iter().map(|e| e.pane.unit_id.as_str()).collect();
        assert_eq!(units, vec!["u1", "u2", "u1"]);
        let ts: Vec<i64> = all.iter().map(|e| e.observed_at_unix_ms).collect();
        assert_eq!(ts, vec![100, 200, 300]);
    }

    /// Ordering regression: appending two events for the
    /// **same pane** with timestamps in the opposite order of append
    /// must surface in `replay_all` sorted by
    /// `(observed_at_unix_ms, id)`. Per-pane `events_for_pane`
    /// continues to expose append order — these are deliberately
    /// different views.
    #[test]
    fn replay_all_sorts_by_observed_at_when_append_order_disagrees_with_timestamps() {
        let store = InMemoryActivityEventStore::new();
        // Append later-timestamp event first.
        let later = ev("u1", 500);
        let earlier = ev("u1", 100);
        store.append(&later).unwrap();
        store.append(&earlier).unwrap();

        // events_for_pane preserves append order.
        let per_pane = store.events_for_pane(&pane("u1")).unwrap();
        assert_eq!(
            per_pane
                .iter()
                .map(|e| e.observed_at_unix_ms)
                .collect::<Vec<_>>(),
            vec![500, 100],
            "events_for_pane must preserve append order"
        );

        // replay_all sorts by (observed_at_unix_ms, id).
        let all = store.replay_all().unwrap();
        assert_eq!(
            all.iter()
                .map(|e| e.observed_at_unix_ms)
                .collect::<Vec<_>>(),
            vec![100, 500],
            "replay_all must order by (observed_at_unix_ms, id)"
        );
    }

    /// Empty pane returns Ok(empty), not an error.
    #[test]
    fn missing_pane_returns_empty_not_error() {
        let store = InMemoryActivityEventStore::new();
        assert!(store.events_for_pane(&pane("u1")).unwrap().is_empty());
    }

    /// Injected `Unavailable` propagates as the typed error.
    #[test]
    fn injected_unavailable_error_surfaces_typed() {
        let store = InMemoryActivityEventStore::new();
        store.set_error_injection(ActivityEventStoreErrorInjection {
            replay_all_error: Some(ActivityEventStoreError::Unavailable("disk gone".into())),
            ..Default::default()
        });
        let err = store.replay_all().expect_err("must surface error");
        assert!(matches!(err, ActivityEventStoreError::Unavailable(_)));
    }

    /// Injected `CorruptEntry` propagates as the typed error so the
    /// projector can degrade rather than silently fall back to an
    /// empty model.
    #[test]
    fn injected_corrupt_entry_surfaces_typed() {
        let store = InMemoryActivityEventStore::new();
        store.set_error_injection(ActivityEventStoreErrorInjection {
            events_for_pane_error: Some(ActivityEventStoreError::CorruptEntry("bad json".into())),
            ..Default::default()
        });
        let err = store
            .events_for_pane(&pane("u1"))
            .expect_err("must surface error");
        assert!(matches!(err, ActivityEventStoreError::CorruptEntry(_)));
    }

    /// Injected `Io` on append surfaces typed.
    #[test]
    fn injected_io_on_append_surfaces_typed() {
        let store = InMemoryActivityEventStore::new();
        store.set_error_injection(ActivityEventStoreErrorInjection {
            append_error: Some(ActivityEventStoreError::Io("disk full".into())),
            ..Default::default()
        });
        let err = store.append(&ev("u1", 0)).expect_err("must surface error");
        assert!(matches!(err, ActivityEventStoreError::Io(_)));
    }
}

//! End-to-end pipeline tests for the activity control plane.
//!
//! These tests exercise the full
//! `Observation -> normalize -> ActivityEventStore.append ->
//! StateProjector.rebuild -> ReadModel` flow against the
//! [`InMemoryActivityEventStore`] fake. They are the read-model
//! rebuild evidence: the projector
//! produces the same rendering-shaped read model regardless of
//! whether events arrive in interleaved order, and degrades
//! cleanly when the store reports `Unavailable`.

use workhive_core::domain::activity::{
    Observation, ObservationConfidence, ObservationPayload, PaneActivityState, PaneCorrelationKey,
    ProviderEvent, ProviderEventKind, ProviderKind, ProviderSignalSource, SignalChannel,
};
use workhive_core::ports::{ActivityEventStore, ActivityEventStoreError};
use workhive_core::usecases::activity_control_plane::{StateProjector, normalize};
use workhive_testkit::fakes::{ActivityEventStoreErrorInjection, InMemoryActivityEventStore};

fn pane(unit: &str) -> PaneCorrelationKey {
    PaneCorrelationKey::new("g", "w", unit)
}

fn obs(
    provider: ProviderKind,
    channel: SignalChannel,
    source: ProviderSignalSource,
    kind: ProviderEventKind,
    confidence: ObservationConfidence,
    pane: PaneCorrelationKey,
    observed_at_unix_ms: i64,
) -> Observation {
    Observation {
        provider,
        channel,
        source,
        pane,
        observed_at_unix_ms,
        confidence,
        payload: ObservationPayload::ProviderEvent(ProviderEvent { kind, detail: None }),
    }
}

/// Round-trip: appended events rebuild into a read model that
/// reflects the latest event per pane.
#[test]
fn full_pipeline_rebuilds_read_model_from_append_only_history() {
    let store = InMemoryActivityEventStore::new();

    // Pane u1: Claude Code lifecycle ToolStarted at t=100.
    let claude = obs(
        ProviderKind::ClaudeCode,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ProviderEventKind::ToolStarted,
        ObservationConfidence::High,
        pane("u1"),
        100,
    );
    store.append(&normalize(&claude).unwrap()).unwrap();

    // Pane u2: Codex notify completion at t=200.
    let codex_notify = obs(
        ProviderKind::Codex,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ProviderEventKind::NotifyCompletion,
        ObservationConfidence::Medium,
        pane("u2"),
        200,
    );
    store.append(&normalize(&codex_notify).unwrap()).unwrap();

    // Pane u1: TurnCompleted at t=300 (latest event for u1).
    let claude_done = obs(
        ProviderKind::ClaudeCode,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ProviderEventKind::TurnCompleted,
        ObservationConfidence::High,
        pane("u1"),
        300,
    );
    store.append(&normalize(&claude_done).unwrap()).unwrap();

    let projector = StateProjector::new(&store, 1_000, 30_000, 90_000);
    let rm = projector.rebuild().expect("rebuild");

    let u1 = rm.get(&pane("u1")).expect("u1 row");
    assert_eq!(
        u1.fact.state,
        PaneActivityState::Completed,
        "u1 latest event TurnCompleted -> Completed"
    );
    let u2 = rm.get(&pane("u2")).expect("u2 row");
    assert_eq!(
        u2.fact.state,
        PaneActivityState::Completed,
        "u2 notify completion -> Completed"
    );
    assert_eq!(u2.fact.confidence, ObservationConfidence::Medium);
}

/// Phase-1 invariant carried end-to-end: a stream that contains
/// only Codex notify completion projects to `Completed` (medium),
/// never to `Working` for any earlier point in the turn.
#[test]
fn full_pipeline_codex_notify_completion_does_not_imply_working() {
    let store = InMemoryActivityEventStore::new();
    let codex_notify = obs(
        ProviderKind::Codex,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ProviderEventKind::NotifyCompletion,
        ObservationConfidence::Medium,
        pane("u1"),
        100,
    );
    store.append(&normalize(&codex_notify).unwrap()).unwrap();

    let rm = StateProjector::new(&store, 200, 30_000, 90_000)
        .rebuild()
        .expect("rebuild");
    let f = rm.get(&pane("u1")).unwrap();
    assert_eq!(f.fact.state, PaneActivityState::Completed);
    assert_ne!(f.fact.state, PaneActivityState::Working);
    assert_ne!(f.fact.state, PaneActivityState::ToolRunning);
}

/// Typed-error propagation: when the store reports `Unavailable`,
/// the projector surfaces the typed error instead of silently
/// returning an empty read model.
#[test]
fn projector_surfaces_typed_unavailable_when_store_is_unavailable() {
    let store = InMemoryActivityEventStore::new();
    store.set_error_injection(ActivityEventStoreErrorInjection {
        replay_all_error: Some(ActivityEventStoreError::Unavailable("scratch wiped".into())),
        ..Default::default()
    });
    let projector = StateProjector::new(&store, 0, 30_000, 90_000);
    let err = projector.rebuild().expect_err("must surface typed error");
    assert!(
        matches!(err, ActivityEventStoreError::Unavailable(_)),
        "expected Unavailable, got {err:?}"
    );
}

/// Typed-error propagation: corrupt-entry on per-pane reads is
/// surfaced as `CorruptEntry` so the renderer can degrade to the
/// `Degraded` state without pretending the pane has no events.
#[test]
fn projector_surfaces_typed_corrupt_entry_on_per_pane_read() {
    let store = InMemoryActivityEventStore::new();
    store.set_error_injection(ActivityEventStoreErrorInjection {
        events_for_pane_error: Some(ActivityEventStoreError::CorruptEntry(
            "missing field".into(),
        )),
        ..Default::default()
    });
    let projector = StateProjector::new(&store, 0, 30_000, 90_000);
    let err = projector
        .project_pane(&pane("u1"))
        .expect_err("must surface typed error");
    assert!(
        matches!(err, ActivityEventStoreError::CorruptEntry(_)),
        "expected CorruptEntry, got {err:?}"
    );
}

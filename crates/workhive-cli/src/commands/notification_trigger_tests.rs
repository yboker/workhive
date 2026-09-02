use super::*;
use std::sync::Mutex;

use workhive_core::domain::activity::{
    ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, PaneCorrelationKey,
    ProviderKind, ProviderSignalSource, SignalChannel,
};
use workhive_core::errors::ConfigError;
use workhive_core::ports::{
    AppConfig, FeaturesConfig, NotificationCategory, NotificationCategoryConfig,
    NotificationDeliveryError, NotificationPort, NotificationsConfig, PendingNotification,
};
use workhive_testkit::fakes::{
    ActivityEventStoreErrorInjection, FakeConfig, InMemoryActivityEventStore,
};

// ── Capturing port ────────────────────────────────────────────────────────

/// Recording port that logs every delivery for assertion. Holds
/// its captures in a `Mutex<Vec<…>>` so the test can inspect
/// across the trigger boundary.
struct CapturingPort {
    delivered: Mutex<Vec<PendingNotification>>,
}

impl CapturingPort {
    fn new() -> Self {
        Self {
            delivered: Mutex::new(Vec::new()),
        }
    }

    fn into_inner(self) -> Vec<PendingNotification> {
        self.delivered.into_inner().unwrap()
    }
}

impl NotificationPort for CapturingPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        self.delivered.lock().unwrap().push(notification.clone());
        Ok(())
    }
}

// ── Fixture builders ──────────────────────────────────────────────────────

/// Build a notifications-fully-enabled `FakeConfig`. We turn on
/// `completed` and `all_panes_complete` (off by default) so the
/// triggers under test can fire when their preconditions are met.
fn fake_config_all_enabled() -> FakeConfig {
    let cfg = AppConfig {
        features: Some(FeaturesConfig {
            notifications: Some(NotificationsConfig {
                enabled: Some(true),
                permission_needed: Some(NotificationCategoryConfig {
                    enabled: Some(true),
                }),
                failed: Some(NotificationCategoryConfig {
                    enabled: Some(true),
                }),
                completed: Some(NotificationCategoryConfig {
                    enabled: Some(true),
                }),
                all_panes_complete: Some(NotificationCategoryConfig {
                    enabled: Some(true),
                }),
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    FakeConfig::new().with_global(cfg)
}

/// Build a notifications-globally-disabled `FakeConfig`.
fn fake_config_globally_disabled() -> FakeConfig {
    let cfg = AppConfig {
        features: Some(FeaturesConfig {
            notifications: Some(NotificationsConfig {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    FakeConfig::new().with_global(cfg)
}

/// Build a config that fails to load (untrusted decision path).
fn fake_config_global_load_error() -> FakeConfig {
    FakeConfig::new().with_global_error(ConfigError::ParseFailed {
        path: camino::Utf8PathBuf::from("/nonexistent/config.toml"),
        message: "synthetic load failure for trigger test".to_owned(),
    })
}

fn pane(group: &str, workspace: &str, unit: &str) -> PaneCorrelationKey {
    PaneCorrelationKey::new(group, workspace, unit)
}

/// Synthetic ACP event with all the canonical knobs the
/// notification policy inspects. We default `outcome` to
/// `NotApplicable` for non-completion kinds and to the matching
/// outcome for completion kinds.
fn synth_event(
    pane: &PaneCorrelationKey,
    kind: ActivityEventKind,
    channel: SignalChannel,
    source: ProviderSignalSource,
    confidence: ObservationConfidence,
    observed_ms: i64,
) -> ActivityEvent {
    let outcome = match kind {
        ActivityEventKind::TurnCompleted | ActivityEventKind::TurnNotifiedComplete => {
            ActivityOutcome::Success
        }
        ActivityEventKind::TurnFailed => ActivityOutcome::Failure,
        _ => ActivityOutcome::NotApplicable,
    };
    ActivityEvent::new(
        pane.clone(),
        ProviderKind::Codex,
        channel,
        source,
        kind,
        outcome,
        confidence,
        observed_ms,
        None,
    )
}

// ── Per-pane trigger tests ────────────────────────────────────────────────

/// `permission-needed` events recorded mid-turn must trigger a
/// notification at evaluation time, not wait for supervisor
/// exit.
#[test]
fn permission_needed_triggers_notification_on_event_record() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::PermissionNeeded)),
        "permission-needed event must trigger a PermissionNeeded notification, \
             observed deliveries: {delivered:?}"
    );
}

/// `Failed` events at high confidence must produce a `Failed`
/// notification on the same event-driven path.
#[test]
fn failed_event_triggers_failed_notification_on_event_record() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnFailed,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Failed)),
        "failed event must trigger a Failed notification, got {delivered:?}"
    );
}

/// `Completed` events from the Codex notify-callback channel
/// must trigger at medium confidence. This is the closed-loop
/// notify-callback path.
#[test]
fn completed_notify_callback_event_triggers_completed_notification() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Completed)),
        "notify-callback completion must trigger a Completed notification, got {delivered:?}"
    );
}

// ── Suppression tests ────────────────────────────────────────────────────

/// When the notifications feature is globally disabled the
/// trigger must produce zero deliveries even for a fresh
/// permission-needed event.
#[test]
fn disabled_notifications_feature_suppresses_all_deliveries() {
    let cfg = fake_config_globally_disabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms + 1,
    );
    assert!(
        port.into_inner().is_empty(),
        "disabled notifications config must suppress all deliveries"
    );
}

/// Stale events (well outside the freshness window) must NOT
/// trigger a notification. Default conservative thresholds say
/// stale_after = 30s and expired_after = 90s, so we anchor `now`
/// well past `expired_after` and verify zero deliveries.
#[test]
fn stale_event_is_suppressed_by_freshness_gate() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let stale_ms: i64 = 1_700_000_000_000;
    let now_ms: i64 = stale_ms + 5_000_000; // ~83 minutes later
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        stale_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms,
    );
    assert!(
        port.into_inner().is_empty(),
        "stale event must be silenced by the freshness gate"
    );
}

/// Low-confidence completed events (notify-callback at LOW) must
/// be suppressed — only high-confidence lifecycle hooks or the
/// medium-confidence notify-callback combination is allowed to
/// fire `Completed`.
#[test]
fn low_confidence_completed_event_is_suppressed() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Low,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );
    assert!(
        port.into_inner().is_empty(),
        "low-confidence completed event must NOT trigger a notification"
    );
}

/// Untrusted config (load failure) must silence the trigger
/// entirely — the gate is fail-closed.
#[test]
fn untrusted_config_silences_trigger_completely() {
    let cfg = fake_config_global_load_error();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms + 1,
    );
    assert!(
        port.into_inner().is_empty(),
        "untrusted config decision must silence the trigger"
    );
}

// ── Cross-pane all-panes-complete ────────────────────────────────────────

/// `deliver_all_panes_complete` must evaluate across every pane
/// in the same `(group, workspace)` cohort. We seed two panes,
/// drive both into completion-like states, then trigger
/// evaluation from the second pane. The cross-pane evaluation
/// must see BOTH panes and emit one `AllPanesComplete`
/// notification.
#[test]
fn all_panes_complete_evaluates_cross_pane_cohort() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane_a = pane("g", "w", "u-a");
    let pane_b = pane("g", "w", "u-b");
    let now_ms: i64 = 1_700_000_000_000;

    let ev_a = synth_event(
        &pane_a,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    let ev_b = synth_event(
        &pane_b,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    store.append(&ev_a).expect("append a");
    store.append(&ev_b).expect("append b");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane_b,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    let all_count = delivered
        .iter()
        .filter(|n| matches!(n.category, NotificationCategory::AllPanesComplete))
        .count();
    assert_eq!(
        all_count, 1,
        "exactly one AllPanesComplete must be emitted per cohort transition, \
             got {delivered:?}"
    );
}

/// When one pane in the cohort is still working, the cross-pane
/// evaluation MUST NOT emit `AllPanesComplete`.
#[test]
fn all_panes_complete_does_not_fire_when_one_pane_still_working() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane_a = pane("g", "w", "u-a");
    let pane_b = pane("g", "w", "u-b");
    let now_ms: i64 = 1_700_000_000_000;

    let ev_a_done = synth_event(
        &pane_a,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    let ev_b_working = synth_event(
        &pane_b,
        ActivityEventKind::ToolStarted,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&ev_a_done).expect("append a");
    store.append(&ev_b_working).expect("append b");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane_a,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        !delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::AllPanesComplete)),
        "AllPanesComplete must NOT fire when at least one pane is still working, \
             got {delivered:?}"
    );
}

/// Cross-pane evaluation must scope to the trigger pane's
/// `(group, workspace)` and ignore panes from other workspaces
/// entirely.
#[test]
fn all_panes_complete_ignores_other_workspaces() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane_target = pane("g", "w", "u-1");
    let pane_other_ws = pane("g", "w-other", "u-1");
    let now_ms: i64 = 1_700_000_000_000;

    // Other workspace is "still working" — but it must not
    // affect the target workspace's evaluation.
    let ev_other = synth_event(
        &pane_other_ws,
        ActivityEventKind::ToolStarted,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    let ev_target = synth_event(
        &pane_target,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    store.append(&ev_other).expect("append other");
    store.append(&ev_target).expect("append target");

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane_target,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::AllPanesComplete)),
        "target workspace cohort is fully complete; AllPanesComplete must fire \
             regardless of other workspaces' state, got {delivered:?}"
    );
}

// ── Fail-open envelope ───────────────────────────────────────────────────

/// When the per-pane event store read fails the trigger must
/// silently swallow the failure (fail-open) and emit no
/// notification. We exercise this with the testkit's typed
/// error injection.
#[test]
fn per_pane_store_unavailable_results_in_zero_deliveries_without_panic() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    store.set_error_injection(ActivityEventStoreErrorInjection {
        events_for_pane_error: Some(workhive_core::ports::ActivityEventStoreError::Unavailable(
            "synthetic per-pane unavailable".into(),
        )),
        ..Default::default()
    });

    let pane = pane("g", "w", "u-1");
    let port = CapturingPort::new();
    let now_ms: i64 = 1_700_000_000_000;
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms,
    );
    assert!(
        port.into_inner().is_empty(),
        "per-pane store error must result in zero deliveries (fail-open)"
    );
}

/// When the `replay_all` cross-pane read fails, the per-pane
/// branch may still have emitted notifications, but the
/// cross-pane branch must degrade silently — no panic, no
/// AllPanesComplete delivery.
#[test]
fn cross_pane_replay_unavailable_does_not_emit_all_panes_complete() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::Medium,
        now_ms,
    );
    store.append(&event).expect("append event");

    // Inject the replay_all failure AFTER the append so the
    // per-pane read still works; only the cross-pane branch
    // breaks.
    store.set_error_injection(ActivityEventStoreErrorInjection {
        replay_all_error: Some(workhive_core::ports::ActivityEventStoreError::Unavailable(
            "synthetic replay unavailable".into(),
        )),
        ..Default::default()
    });

    let port = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::CodexNotify,
        &port,
        now_ms + 1,
    );

    let delivered = port.into_inner();
    assert!(
        !delivered
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::AllPanesComplete)),
        "AllPanesComplete must NOT fire when cross-pane replay errors, \
             got {delivered:?}"
    );
}

/// The trigger must not panic when invoked with a port that
/// always returns a delivery error. The diagnostic surface is
/// the outbox `DeliveryFailed` entry — we verify by
/// instrumenting the port's call counter and confirming the
/// trigger reached the deliver path even though it errored.
#[test]
fn delivery_error_is_swallowed_per_fail_open_contract() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    struct AlwaysFailPort {
        calls: Mutex<usize>,
    }
    impl NotificationPort for AlwaysFailPort {
        fn deliver(&self, _n: &PendingNotification) -> Result<(), NotificationDeliveryError> {
            *self.calls.lock().unwrap() += 1;
            Err(NotificationDeliveryError {
                reason: "synthetic adapter failure".to_string(),
            })
        }
    }

    let port = AlwaysFailPort {
        calls: Mutex::new(0),
    };
    // Must not panic.
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        None,
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port,
        now_ms + 1,
    );
    assert!(
        *port.calls.lock().unwrap() >= 1,
        "trigger must call the port at least once even though delivery errors"
    );
}

// ── Anti-spam: cross-invocation dedupe ─────────────────────────────────────
//
// These tests model the production scenario: the trigger is invoked
// by two *separate* logical events (a second ACP record/hook, a
// codex-notify follow-up, a supervisor-exit fan-in). Each invocation
// builds its *own* in-memory `NotificationOutbox` — so deduplication
// is only achievable by seeding from / flushing to a persistent
// `NotificationOutboxStore`. We mount an in-memory store that
// mirrors what `FsNotificationOutboxStore` does on disk.

/// In-memory `NotificationOutboxStore` used to model cross-invocation
/// persistence in tests without touching the filesystem.
struct InMemoryOutboxStore {
    sigs: Mutex<Vec<workhive_core::ports::TransitionSignature>>,
}

impl InMemoryOutboxStore {
    fn new() -> Self {
        Self {
            sigs: Mutex::new(Vec::new()),
        }
    }
}

impl workhive_core::ports::NotificationOutboxStore for InMemoryOutboxStore {
    fn load_delivered_signatures(
        &self,
    ) -> Result<
        Vec<workhive_core::ports::TransitionSignature>,
        workhive_core::ports::NotificationOutboxStoreError,
    > {
        Ok(self.sigs.lock().unwrap().clone())
    }

    fn record_delivered(
        &self,
        sig: &workhive_core::ports::TransitionSignature,
    ) -> Result<(), workhive_core::ports::NotificationOutboxStoreError> {
        self.sigs.lock().unwrap().push(sig.clone());
        Ok(())
    }
}

/// Two logical invocations of the trigger over the *same*
/// `permission-needed` event history must only physically deliver
/// once, courtesy of the persistent outbox store seeding.
#[test]
fn permission_needed_dedupes_across_two_trigger_invocations() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let outbox_store = InMemoryOutboxStore::new();

    // Invocation #1 — the recording-time trigger.
    let port_a = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port_a,
        now_ms + 1,
    );
    let first = port_a.into_inner();
    assert!(
        first
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::PermissionNeeded)),
        "first invocation must deliver permission-needed once: {first:?}"
    );

    // Invocation #2 — e.g. supervisor-exit fan-in, *different* `now_ms`.
    let port_b = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::SupervisorExit,
        &port_b,
        now_ms + 60_000,
    );
    let second = port_b.into_inner();
    assert!(
        !second
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::PermissionNeeded)),
        "second invocation over the same event history MUST be \
             deduped by the persistent outbox store: {second:?}"
    );
}

/// Same as above for the `failed` category.
#[test]
fn failed_dedupes_across_two_trigger_invocations() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnFailed,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let outbox_store = InMemoryOutboxStore::new();

    let port_a = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port_a,
        now_ms + 1,
    );
    let first = port_a.into_inner();
    assert!(
        first
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Failed)),
        "first invocation must deliver failed once: {first:?}"
    );

    let port_b = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::SupervisorExit,
        &port_b,
        now_ms + 30_000,
    );
    let second = port_b.into_inner();
    assert!(
        !second
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Failed)),
        "second invocation over the same failed event history MUST \
             NOT re-deliver: {second:?}"
    );
}

/// `completed` category dedupes across two invocations, even when
/// `now_ms` differs and the original signature predates the second
/// call by minutes (would have produced a different stale freshness
/// age under the old `provider_freshness_age` signature).
#[test]
fn completed_dedupes_across_two_trigger_invocations() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let outbox_store = InMemoryOutboxStore::new();

    let port_a = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::CodexNotify,
        &port_a,
        now_ms + 1,
    );
    let first = port_a.into_inner();
    assert!(
        first
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Completed)),
        "first invocation must deliver completed once: {first:?}"
    );

    let port_b = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::SupervisorExit,
        &port_b,
        now_ms + 120_000,
    );
    let second = port_b.into_inner();
    assert!(
        !second
            .iter()
            .any(|n| matches!(n.category, NotificationCategory::Completed)),
        "second invocation over the same completed event history MUST \
             NOT re-deliver even at later now_ms: {second:?}"
    );
}

/// `all-panes-complete` (cross-pane cohort) must dedupe across two
/// trigger invocations. We seed a 2-pane cohort, finish both, and
/// then trigger evaluation twice — once from pane-b record, once
/// from supervisor-exit on pane-a. The cross-pane notification must
/// fire exactly once.
#[test]
fn all_panes_complete_dedupes_across_two_trigger_invocations() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane_a = pane("g", "w", "u-a");
    let pane_b = pane("g", "w", "u-b");
    let now_ms: i64 = 1_700_000_000_000;

    let event_a = synth_event(
        &pane_a,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event_a).expect("append event a");

    let event_b = synth_event(
        &pane_b,
        ActivityEventKind::TurnNotifiedComplete,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event_b).expect("append event b");

    let outbox_store = InMemoryOutboxStore::new();

    // Invocation #1 — codex-notify on pane-b.
    let port_a = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane_b,
        NotificationTriggerSource::CodexNotify,
        &port_a,
        now_ms + 1,
    );
    let first = port_a.into_inner();
    let first_all_panes = first
        .iter()
        .filter(|n| matches!(n.category, NotificationCategory::AllPanesComplete))
        .count();
    assert_eq!(
        first_all_panes, 1,
        "first invocation must deliver exactly one all-panes-complete: \
             {first:?}"
    );

    // Invocation #2 — supervisor-exit on pane-a, later now_ms.
    let port_b = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane_a,
        NotificationTriggerSource::SupervisorExit,
        &port_b,
        now_ms + 90_000,
    );
    let second = port_b.into_inner();
    let second_all_panes = second
        .iter()
        .filter(|n| matches!(n.category, NotificationCategory::AllPanesComplete))
        .count();
    assert_eq!(
        second_all_panes, 0,
        "second invocation over the same cohort completion history \
             MUST NOT re-deliver all-panes-complete: {second:?}"
    );
}

/// Same event history evaluated at two *different* `now_ms` values
/// must produce the *same* `TransitionSignature` identity and thus
/// dedupe. This guards the invariant that the
/// signature must not depend on wall-clock or freshness-age.
///
/// We model this as two separate trigger invocations to keep the
/// surface aligned with the cross-invocation tests above.
#[test]
fn same_event_history_different_now_ms_does_not_double_deliver() {
    let cfg = fake_config_all_enabled();
    let store = InMemoryActivityEventStore::new();
    let pane = pane("g", "w", "u-1");
    let now_ms: i64 = 1_700_000_000_000;
    let event = synth_event(
        &pane,
        ActivityEventKind::PermissionRequested,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ObservationConfidence::High,
        now_ms,
    );
    store.append(&event).expect("append event");

    let outbox_store = InMemoryOutboxStore::new();

    let port_a = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port_a,
        now_ms + 1,
    );
    let first = port_a.into_inner();
    assert_eq!(
        first
            .iter()
            .filter(|n| matches!(n.category, NotificationCategory::PermissionNeeded))
            .count(),
        1,
        "first evaluation must deliver permission-needed exactly once"
    );

    // Same history, *different* now_ms. A freshness-keyed signature
    // would have flipped `provider_freshness_age` and re-delivered.
    let port_b = CapturingPort::new();
    evaluate_after_acp_append(
        &cfg,
        None,
        &store,
        Some(&outbox_store),
        &pane,
        NotificationTriggerSource::ActivityHook,
        &port_b,
        now_ms + 5_000,
    );
    let second = port_b.into_inner();
    assert_eq!(
        second
            .iter()
            .filter(|n| matches!(n.category, NotificationCategory::PermissionNeeded))
            .count(),
        0,
        "stable signature must dedupe at different now_ms: {second:?}"
    );
}

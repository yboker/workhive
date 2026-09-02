//! Local notification delivery use case.
//!
//! This module implements the conservative policy-gated notification
//! pipeline:
//!
//! ```text
//! PaneActivityMetrics  →  policy check  →  dedupe  →  NotificationPort
//!       ↑                                      ↑
//!   acp_source flag               NotificationOutbox
//! ```
//!
//! ## Contract
//!
//! - Notifications are only emitted when `acp_source == true`.
//!   Legacy-only panes are silently refused with
//!   [`NotificationRefusal::LegacySource`].
//! - State must be fresh + orchestration-eligible + high (or
//!   medium-notify-completion) confidence.
//! - Every delivery attempt goes through the outbox for inspection;
//!   dedupe is enforced regardless of port outcome.
//! - Port failures are logged at `warn` and do NOT propagate. The
//!   caller continues executing.
//! - No prompt / message content ever enters a `PendingNotification`.

use tracing::warn;

use crate::domain::activity::{
    ActivityEventId, ObservationConfidence, PaneActivityState, SignalChannel, SignalFreshness,
};
use crate::ports::{
    NotificationCategory, NotificationHint, NotificationOutbox, NotificationPort,
    NotificationRefusal, NotificationsConfig, PendingNotification, TransitionSignature,
};
use crate::usecases::activity_control_plane::metrics::PaneActivityMetrics;

// ── NotificationCandidate ─────────────────────────────────────────────────────

/// A notification candidate: the projected pane metrics paired with
/// the stable source identity of the ACP event that produced the
/// notification-relevant transition.
///
/// ## Why this type exists
///
/// Deriving a [`TransitionSignature`] from
/// the metrics' freshness age — which is time-relative — produces
/// different signatures for the same ACP event evaluated at different
/// wall-clock instants. That breaks dedupe whenever
/// [`evaluate_after_acp_append`] is called twice on the same event
/// history.
///
/// `NotificationCandidate` carries the event identity through the
/// notification pipeline without polluting [`PaneActivityMetrics`]
/// (which is consumed by dashboards / read models that have no
/// dedupe concern).
///
/// ## Stable signature rules
///
/// * When [`Self::source_event_id`] is `Some`, [`build_signature`]
///   uses [`TransitionSignature::from_event_id`]. The same event id
///   always produces the same signature regardless of `now_ms`.
/// * When [`Self::source_event_id`] is `None`, [`build_signature`]
///   falls back to [`TransitionSignature::from_seq`] keyed on the
///   pane + category + [`Self::latest_event_observed_at_unix_ms`].
///   The observed-at-unix-ms field is the monotonic event-driven
///   sequence captured at append time; it does NOT change between
///   evaluations of the same history.
///
/// Either way, the resulting [`TransitionSignature`] is stable for
/// the same event history. This is the contract documented on
/// [`TransitionSignature`].
///
/// [`evaluate_after_acp_append`]: crate::ports::NotificationPort
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCandidate {
    /// Projected metrics for the pane this candidate is for.
    pub metrics: PaneActivityMetrics,
    /// Identity of the ACP event that produced this candidate.
    ///
    /// `Some` when the candidate was built directly from an event
    /// store read (the production path). `None` is reserved for
    /// callers that do not yet have a concrete event in hand (e.g.
    /// the `deliver_all_panes_complete` cohort path uses the
    /// fallback seq form because the "transition" spans many events).
    pub source_event_id: Option<ActivityEventId>,
    /// `observed_at_unix_ms` of the latest notification-relevant
    /// event seen so far for the pane.
    ///
    /// Used as the dedupe-fallback seq when `source_event_id` is
    /// absent. Captured from the event itself (NOT from
    /// `now_ms`), so it is identical across evaluations of the same
    /// history.
    pub latest_event_observed_at_unix_ms: Option<i64>,
}

impl NotificationCandidate {
    /// Build a candidate from metrics alone (no event identity).
    ///
    /// Used by tests and by paths that intentionally rely on the
    /// `(pane, category, 0)` fallback signature.
    pub fn from_metrics(metrics: PaneActivityMetrics) -> Self {
        Self {
            metrics,
            source_event_id: None,
            latest_event_observed_at_unix_ms: None,
        }
    }

    /// Build a candidate with an explicit source event id.
    pub fn with_source_event(
        metrics: PaneActivityMetrics,
        source_event_id: ActivityEventId,
        observed_at_unix_ms: i64,
    ) -> Self {
        Self {
            metrics,
            source_event_id: Some(source_event_id),
            latest_event_observed_at_unix_ms: Some(observed_at_unix_ms),
        }
    }
}

// ── Policy helpers ────────────────────────────────────────────────────────────

/// Reason a notification was refused before the outbox.
///
/// This mirrors [`NotificationRefusal`] but is returned by the policy
/// layer so callers / tests can inspect the typed refusal reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryRefusal {
    /// Feature gate is disabled for this category.
    FeatureGateDisabled,
    /// Source is the legacy path (`acp_source = false`).
    LegacySource,
    /// State is not orchestration-eligible (unknown, stale, disabled,
    /// unsupported, degraded, or open-unverified).
    NoUsableState,
    /// Confidence is below the floor for this category.
    LowConfidence,
    /// Signal is stale or expired.
    Stale,
    /// Dedupe: the same transition was already notified.
    AlreadyNotified,
}

impl From<DeliveryRefusal> for NotificationRefusal {
    fn from(r: DeliveryRefusal) -> Self {
        match r {
            DeliveryRefusal::FeatureGateDisabled => NotificationRefusal::FeatureGateDisabled,
            DeliveryRefusal::LegacySource => NotificationRefusal::LegacySource,
            DeliveryRefusal::NoUsableState => NotificationRefusal::IntegrationDisabled,
            DeliveryRefusal::LowConfidence => NotificationRefusal::LowConfidence,
            DeliveryRefusal::Stale => NotificationRefusal::Stale,
            DeliveryRefusal::AlreadyNotified => NotificationRefusal::AlreadyNotified,
        }
    }
}

// ── Main entry point ─────────────────────────────────────────────────────────

/// Deliver a local notification derived from `metrics`.
///
/// Checks policy (feature gate, legacy source, confidence, freshness),
/// constructs the appropriate [`NotificationCategory`] and
/// [`PendingNotification`], deduplicates via `outbox`, then calls
/// `port.deliver`. Port failures are logged at `warn` and NOT
/// propagated.
///
/// Returns the [`DeliveryRefusal`] if the notification was refused
/// before delivery, or `None` when the notification was attempted
/// (whether delivery succeeded or failed). Callers may use this for
/// metrics / diagnostics.
///
/// # Arguments
///
/// * `config`  – Resolved notifications feature config. Absent config
///   is treated as the built-in default (enabled with conservative
///   per-category settings).
/// * `metrics` – ACP-derived pane metrics (from Agent B's
///   `compute_pane_metrics`).
/// * `outbox`  – Mutable reference to the in-process notification
///   outbox (dedupe + inspection store).
/// * `port`    – Delivery port. `None` is allowed for tests / when
///   the port has not been wired; in that case the method records the
///   notification in the outbox but skips physical delivery.
pub fn deliver_notification(
    config: Option<&NotificationsConfig>,
    candidate: &NotificationCandidate,
    outbox: &mut NotificationOutbox,
    port: Option<&dyn NotificationPort>,
) -> Option<DeliveryRefusal> {
    let metrics = &candidate.metrics;
    // 1. Legacy source guard: non-ACP metrics carry no reliable state.
    if !metrics.acp_source {
        return Some(DeliveryRefusal::LegacySource);
    }

    // 2. State eligibility — must be orchestration-eligible.
    if !metrics.state.is_orchestration_eligible() {
        return Some(DeliveryRefusal::NoUsableState);
    }

    // 3. Freshness floor.
    if !matches!(metrics.freshness, SignalFreshness::Fresh) {
        return Some(DeliveryRefusal::Stale);
    }

    // 4. Determine category from state; check confidence floor.
    let (category, hint, minimum_refusal) = category_for(metrics);
    if let Some(refusal) = minimum_refusal {
        return Some(refusal);
    }

    // 5. Feature gate check for the resolved category.
    let cfg = config;
    if !category_enabled(cfg, category) {
        return Some(DeliveryRefusal::FeatureGateDisabled);
    }

    // 6. Build transition signature.
    let sig = build_signature(candidate, category);

    // 7. Dedupe check.
    if outbox.is_duplicate(&sig) {
        return Some(DeliveryRefusal::AlreadyNotified);
    }

    // 8. Build notification (metadata only — no prompt content).
    let notification = PendingNotification {
        pane: metrics.pane.clone(),
        provider: metrics.provider,
        category,
        hint,
        transition_sig: sig,
    };

    // 9. Deliver via port (fail-open).
    match port {
        Some(p) => match p.deliver(&notification) {
            Ok(()) => {
                outbox.record_delivered(notification);
            }
            Err(e) => {
                warn!(
                    pane_group = %notification.pane.group,
                    pane_workspace = %notification.pane.workspace,
                    pane_unit = %notification.pane.unit_id,
                    category = ?notification.category,
                    error = %e,
                    "notification delivery failed (fail-open: continuing)"
                );
                outbox.record_failed(notification, e.reason);
            }
        },
        None => {
            // No port wired — record as delivered for test inspection.
            outbox.record_delivered(notification);
        }
    }

    None
}

// ── deliver_all helper ────────────────────────────────────────────────────────

/// Attempt notification delivery for each pane in `metrics_list`.
///
/// Skips panes that have no notification-relevant state change.
/// All refusals are recorded in the outbox. Port failures are
/// logged at `warn` and not propagated.
///
/// This is the top-level "run the notification loop" function that
/// the control-plane orchestrator calls after projecting facts.
pub fn deliver_notifications_for_panes(
    config: Option<&NotificationsConfig>,
    candidates: &[NotificationCandidate],
    outbox: &mut NotificationOutbox,
    port: Option<&dyn NotificationPort>,
) {
    for candidate in candidates {
        // deliver_notification handles its own refusal recording via
        // the outbox for Refused entries. We call it for every pane
        // and let the outbox track outcomes.
        let refusal = deliver_notification(config, candidate, outbox, port);

        // Record refused entries in the outbox for inspection.
        if let Some(r) = refusal {
            // Build a minimal notification for the outbox refused entry.
            // We use category Unknown placeholder; callers inspect by
            // pane + refusal, not by category here.
            if let Some((category, hint)) = refusal_category_hint(&candidate.metrics) {
                let sig = build_signature(candidate, category);
                let notification = PendingNotification {
                    pane: candidate.metrics.pane.clone(),
                    provider: candidate.metrics.provider,
                    category,
                    hint,
                    transition_sig: sig,
                };
                outbox.record_refused(notification, r.into());
            }
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Determine the notification category, hint, and confidence refusal
/// from a pane's current state and confidence.
///
/// Returns `(category, hint, Option<DeliveryRefusal>)`.
/// If `DeliveryRefusal` is `Some`, the notification should be refused.
fn category_for(
    metrics: &PaneActivityMetrics,
) -> (
    NotificationCategory,
    Option<NotificationHint>,
    Option<DeliveryRefusal>,
) {
    match metrics.state {
        PaneActivityState::WaitingForPermission => {
            // High-confidence required for PermissionNeeded.
            if !is_high_confidence(metrics) {
                return (
                    NotificationCategory::PermissionNeeded,
                    None,
                    Some(DeliveryRefusal::LowConfidence),
                );
            }
            (
                NotificationCategory::PermissionNeeded,
                Some(NotificationHint::PermissionUnresolved),
                None,
            )
        }
        PaneActivityState::Failed => {
            if !is_high_confidence(metrics) {
                return (
                    NotificationCategory::Failed,
                    None,
                    Some(DeliveryRefusal::LowConfidence),
                );
            }
            (
                NotificationCategory::Failed,
                Some(NotificationHint::TurnFailed),
                None,
            )
        }
        PaneActivityState::Completed => {
            // Codex notify-callback at Medium is the special-cased
            // medium-confidence completion flow.
            let (confidence_ok, hint) = if matches!(metrics.confidence, ObservationConfidence::High)
            {
                (true, Some(NotificationHint::LifecycleHook))
            } else if matches!(metrics.confidence, ObservationConfidence::Medium)
                && matches!(metrics.channel, SignalChannel::NotifyCallback)
            {
                (true, Some(NotificationHint::NotifyCallback))
            } else {
                (false, None)
            };
            if !confidence_ok {
                return (
                    NotificationCategory::Completed,
                    None,
                    Some(DeliveryRefusal::LowConfidence),
                );
            }
            (NotificationCategory::Completed, hint, None)
        }
        // Working / ToolRunning / Idle — not notification-relevant states.
        // They are orchestration-eligible but no notification category maps
        // to them. Return NoUsableState so the caller silently skips.
        _ => (
            NotificationCategory::Completed, // placeholder; will be refused
            None,
            Some(DeliveryRefusal::NoUsableState),
        ),
    }
}

/// Like `category_for` but infallible — returns `None` when no
/// category applies. Used by `deliver_notifications_for_panes` to
/// build outbox refused entries.
fn refusal_category_hint(
    metrics: &PaneActivityMetrics,
) -> Option<(NotificationCategory, Option<NotificationHint>)> {
    match metrics.state {
        PaneActivityState::WaitingForPermission => Some((
            NotificationCategory::PermissionNeeded,
            Some(NotificationHint::PermissionUnresolved),
        )),
        PaneActivityState::Failed => Some((
            NotificationCategory::Failed,
            Some(NotificationHint::TurnFailed),
        )),
        PaneActivityState::Completed => Some((
            NotificationCategory::Completed,
            Some(NotificationHint::LifecycleHook),
        )),
        _ => None,
    }
}

/// Whether the category is enabled per the notifications config.
fn category_enabled(config: Option<&NotificationsConfig>, category: NotificationCategory) -> bool {
    let cfg = config.cloned().unwrap_or_default();
    match category {
        NotificationCategory::PermissionNeeded => cfg.permission_needed_enabled(),
        NotificationCategory::Failed => cfg.failed_enabled(),
        NotificationCategory::Completed => cfg.completed_enabled(),
        NotificationCategory::AllPanesComplete => cfg.all_panes_complete_enabled(),
    }
}

/// Whether `metrics` has the minimum required confidence level for
/// single-signal notifications (excludes the notify-callback medium
/// path — that is handled in `category_for`).
fn is_high_confidence(metrics: &PaneActivityMetrics) -> bool {
    matches!(metrics.confidence, ObservationConfidence::High)
}

/// Build the deterministic [`TransitionSignature`] for `candidate`.
///
/// ## Stability invariant
///
/// The signature MUST be stable for the same ACP event history. We
/// derive it purely from event-driven inputs:
///
/// 1. If `candidate.source_event_id` is `Some`, the signature is
///    [`TransitionSignature::from_event_id`] using the event's UUID.
///    Same event ↔ same signature, regardless of `now_ms`.
/// 2. Otherwise, the signature is
///    [`TransitionSignature::from_seq`] keyed on
///    `(pane, category, candidate.latest_event_observed_at_unix_ms)`.
///    The observed-at-unix-ms field is captured from the event
///    itself at append time — it is monotonically increasing in the
///    append-only event log and identical across evaluations of the
///    same history.
///
/// This function MUST NOT read freshness age, elapsed time, or any
/// other `now_ms`-relative metric. Doing so makes the signature
/// unstable and re-delivers duplicate notifications.
fn build_signature(
    candidate: &NotificationCandidate,
    category: NotificationCategory,
) -> TransitionSignature {
    if let Some(id) = candidate.source_event_id {
        return TransitionSignature::from_event_id(&id.0.to_string());
    }
    let seq = candidate.latest_event_observed_at_unix_ms.unwrap_or(0);
    TransitionSignature::from_seq(&candidate.metrics.pane, category, seq)
}

// ── all_panes_complete delivery ───────────────────────────────────────────────

/// Check whether all AI panes in `metrics_list` are in a fresh,
/// high-confidence completion-like state and, if so, deliver a single
/// [`NotificationCategory::AllPanesComplete`] notification.
///
/// Returns `true` if the notification was attempted (delivered or
/// failed); `false` if the check did not pass or the gate was disabled.
///
/// The `group_sig_seq` parameter is a monotonically increasing
/// sequence counter for the group (e.g. the highest event seq across
/// all panes); it forms the dedup key.
pub fn deliver_all_panes_complete(
    config: Option<&NotificationsConfig>,
    metrics_list: &[PaneActivityMetrics],
    group_sig_seq: i64,
    outbox: &mut NotificationOutbox,
    port: Option<&dyn NotificationPort>,
) -> bool {
    // Gate: feature must be enabled.
    let cfg = config.cloned().unwrap_or_default();
    if !cfg.all_panes_complete_enabled() {
        return false;
    }

    // All relevant ACP panes must be fresh + completion-like.
    let relevant: Vec<&PaneActivityMetrics> =
        metrics_list.iter().filter(|m| m.acp_source).collect();

    if relevant.is_empty() {
        return false;
    }

    let all_complete = relevant.iter().all(|m| {
        matches!(m.freshness, SignalFreshness::Fresh)
            && matches!(
                m.state,
                PaneActivityState::Completed | PaneActivityState::Idle
            )
            && (matches!(m.confidence, ObservationConfidence::High)
                || (matches!(m.state, PaneActivityState::Completed)
                    && matches!(m.channel, SignalChannel::NotifyCallback)
                    && matches!(m.confidence, ObservationConfidence::Medium)))
    });

    if !all_complete {
        return false;
    }

    // Use the first pane as the key (group event); a real impl would
    // use a group-level identity.
    let representative = relevant[0];

    // Build a group-level signature using the representative pane + seq.
    let sig = TransitionSignature::from_seq(
        &representative.pane,
        NotificationCategory::AllPanesComplete,
        group_sig_seq,
    );

    if outbox.is_duplicate(&sig) {
        return false;
    }

    let notification = PendingNotification {
        pane: representative.pane.clone(),
        provider: representative.provider,
        category: NotificationCategory::AllPanesComplete,
        hint: None,
        transition_sig: sig,
    };

    match port {
        Some(p) => match p.deliver(&notification) {
            Ok(()) => {
                outbox.record_delivered(notification);
                true
            }
            Err(e) => {
                warn!(
                    category = ?NotificationCategory::AllPanesComplete,
                    error = %e,
                    "all-panes-complete notification delivery failed (fail-open)"
                );
                outbox.record_failed(notification, e.reason);
                true
            }
        },
        None => {
            outbox.record_delivered(notification);
            true
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityEventId, ObservationConfidence, PaneActivityState, PaneCorrelationKey,
        ProviderKind, SignalChannel, SignalFreshness,
    };
    use crate::ports::{
        NotificationCategory, NotificationDeliveryError, NotificationHint, NotificationOutbox,
        NotificationPort, NotificationsConfig, OutboxEntryStatus, PendingNotification,
        TransitionSignature,
    };
    use crate::usecases::activity_control_plane::metrics::{
        MetricValue, PaneActivityMetrics, UnavailableReason,
    };
    use std::cell::RefCell;

    // ── Test helpers ───────────────────────────────────────────────

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u1")
    }

    /// Build a [`NotificationCandidate`] from metrics with a fixed
    /// event id + observed-at seq. Tests that need to assert dedupe
    /// stability MUST use this helper to mimic the production
    /// trigger boundary, which always supplies a stable
    /// event-driven signature.
    fn cand(metrics: PaneActivityMetrics) -> NotificationCandidate {
        // Deterministic UUID so the signature is fully stable across
        // test runs.
        let id = ActivityEventId(uuid::Uuid::from_u128(
            0x1234_5678_9abc_def0_1122_3344_5566_7788,
        ));
        NotificationCandidate::with_source_event(metrics, id, 5_000)
    }

    /// Build a [`NotificationCandidate`] from metrics + an explicit
    /// event identity, for the stability tests that need
    /// to assert "different now_ms, same id → same signature".
    fn cand_with(
        metrics: PaneActivityMetrics,
        id: ActivityEventId,
        observed_at_unix_ms: i64,
    ) -> NotificationCandidate {
        NotificationCandidate::with_source_event(metrics, id, observed_at_unix_ms)
    }

    fn base_metrics(
        state: PaneActivityState,
        confidence: ObservationConfidence,
        freshness: SignalFreshness,
    ) -> PaneActivityMetrics {
        PaneActivityMetrics {
            pane: pane(),
            provider: Some(ProviderKind::Codex),
            state,
            channel: SignalChannel::LifecycleHook,
            confidence,
            freshness,
            elapsed: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            current_turn_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            last_completed_turn_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            last_tool_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            aggregate_tool_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            permission_wait_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            idle_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            provider_freshness_age: MetricValue::Known(std::time::Duration::from_millis(5_000)),
            acp_source: true,
        }
    }

    /// A fake port that records deliveries and can optionally fail.
    struct FakePort {
        should_fail: bool,
        deliveries: RefCell<Vec<PendingNotification>>,
    }

    impl FakePort {
        fn succeeding() -> Self {
            Self {
                should_fail: false,
                deliveries: RefCell::new(Vec::new()),
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                deliveries: RefCell::new(Vec::new()),
            }
        }

        #[allow(dead_code)]
        fn delivery_count(&self) -> usize {
            self.deliveries.borrow().len()
        }
    }

    impl NotificationPort for FakePort {
        fn deliver(&self, n: &PendingNotification) -> Result<(), NotificationDeliveryError> {
            if self.should_fail {
                Err(NotificationDeliveryError {
                    reason: "test failure".to_owned(),
                })
            } else {
                self.deliveries.borrow_mut().push(n.clone());
                Ok(())
            }
        }
    }

    // ── Test: notification emitted once per permission-needed
    //         transition ─────────────────────────────────────────
    #[test]
    fn notification_emitted_once_per_permission_needed_transition() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert!(r.is_none(), "first delivery should succeed without refusal");
        assert_eq!(
            outbox.delivered_count(),
            1,
            "outbox must record one delivery"
        );
    }

    // ── Test: notification deduped when signature unchanged ──────
    #[test]
    fn notification_deduped_when_state_transition_signature_unchanged() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        // First delivery.
        deliver_notification(None, &cand(metrics.clone()), &mut outbox, Some(&port));
        // Second delivery with the same metrics (same signature).
        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(
            r,
            Some(DeliveryRefusal::AlreadyNotified),
            "duplicate must be refused with AlreadyNotified"
        );
        assert_eq!(
            outbox.delivered_count(),
            1,
            "only one delivery after dedupe"
        );
    }

    // ── Test: failed notification emitted and deduped ────────────
    #[test]
    fn failed_notification_emitted_and_deduped() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::Failed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        deliver_notification(None, &cand(metrics.clone()), &mut outbox, Some(&port));
        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(r, Some(DeliveryRefusal::AlreadyNotified));
    }

    // ── Test: completed respects low-noise config ─────────────────
    #[test]
    fn completed_notification_respects_low_noise_config() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::Completed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        // Default config has completed disabled (low-noise default).
        let r = deliver_notification(None, &cand(metrics.clone()), &mut outbox, Some(&port));
        assert_eq!(
            r,
            Some(DeliveryRefusal::FeatureGateDisabled),
            "completed must be refused by default (low-noise)"
        );

        // Enable completed via config.
        let cfg = NotificationsConfig {
            enabled: Some(true),
            completed: Some(crate::ports::NotificationCategoryConfig {
                enabled: Some(true),
            }),
            ..Default::default()
        };
        let r = deliver_notification(Some(&cfg), &cand(metrics), &mut outbox, Some(&port));
        assert!(
            r.is_none(),
            "completed must be allowed when explicitly enabled"
        );
    }

    // ── Test: refused for low-confidence state ────────────────────
    #[test]
    fn notification_refused_for_low_confidence_state() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::Low,
            SignalFreshness::Fresh,
        );

        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(
            r,
            Some(DeliveryRefusal::LowConfidence),
            "low-confidence must be refused"
        );
    }

    // ── Test: refused for stale state ─────────────────────────────
    #[test]
    fn notification_refused_for_stale_state() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Stale,
        );

        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(
            r,
            Some(DeliveryRefusal::Stale),
            "stale signal must be refused"
        );
    }

    // ── Test: refused for disabled feature gate (silently) ────────
    #[test]
    fn notification_refused_for_disabled_feature_gate_silently() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        let cfg = NotificationsConfig {
            enabled: Some(false),
            ..Default::default()
        };
        let r = deliver_notification(Some(&cfg), &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(
            r,
            Some(DeliveryRefusal::FeatureGateDisabled),
            "disabled gate must silently refuse"
        );
        // No entries in outbox for disabled-gate refusals (silent).
        assert_eq!(outbox.delivered_count(), 0);
    }

    // ── Test: delivery failure is fail-open ───────────────────────
    #[test]
    fn notification_delivery_failure_is_fail_open() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::failing();
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        // Must NOT panic or return an error.
        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert!(
            r.is_none(),
            "fail-open: delivery failure must not propagate"
        );

        // Outbox marks the entry as DeliveryFailed.
        let entries = outbox.entries();
        assert_eq!(entries.len(), 1);
        assert!(
            matches!(entries[0].status, OutboxEntryStatus::DeliveryFailed(_)),
            "port failure must record DeliveryFailed"
        );
        assert_eq!(
            outbox.delivered_count(),
            0,
            "failed delivery does not count as delivered"
        );
    }

    // ── Test: notification never stores prompt or message content ─
    #[test]
    fn notification_never_stores_prompt_or_message_content() {
        // PendingNotification must have no message body / prompt field.
        // This is a structural assertion: if the type ever grows a
        // message field this test must be updated (and reviewed for
        // privacy).
        let n = PendingNotification {
            pane: pane(),
            provider: Some(ProviderKind::Codex),
            category: NotificationCategory::Completed,
            hint: Some(NotificationHint::LifecycleHook),
            transition_sig: TransitionSignature::from_event_id("evt-abc"),
        };
        // The struct has 5 fields: pane, provider, category, hint,
        // transition_sig. None of them is a message body.
        // Asserting that `n.hint` is a typed enum (not a String).
        let _: Option<NotificationHint> = n.hint;
        // Asserting that no `message`, `body`, `prompt`, or `content`
        // field exists by destructuring.
        let PendingNotification {
            pane: _,
            provider: _,
            category: _,
            hint: _,
            transition_sig: _,
        } = n;
        // If the struct had a message field, this destructuring would
        // fail to compile.
    }

    // ── Test: legacy source is refused ────────────────────────────
    #[test]
    fn legacy_source_pane_is_refused_for_notification() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let mut metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        metrics.acp_source = false; // legacy path

        let r = deliver_notification(None, &cand(metrics), &mut outbox, Some(&port));
        assert_eq!(r, Some(DeliveryRefusal::LegacySource));
        assert_eq!(outbox.delivered_count(), 0);
    }

    // ── Test: all panes complete fires only when fresh ────────────
    #[test]
    fn all_panes_complete_notification_fires_only_when_all_acp_panes_are_fresh_completion() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();

        let m1 = base_metrics(
            PaneActivityState::Completed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let m2 = {
            let mut m = base_metrics(
                PaneActivityState::Completed,
                ObservationConfidence::High,
                SignalFreshness::Fresh,
            );
            m.pane = PaneCorrelationKey::new("g", "w", "u2");
            m
        };

        let cfg = NotificationsConfig {
            all_panes_complete: Some(crate::ports::NotificationCategoryConfig {
                enabled: Some(true),
            }),
            ..Default::default()
        };

        let fired =
            deliver_all_panes_complete(Some(&cfg), &[m1, m2], 12345, &mut outbox, Some(&port));
        assert!(
            fired,
            "all-complete must fire when all panes are fresh+complete"
        );
        assert_eq!(outbox.delivered_count(), 1);
    }

    #[test]
    fn all_panes_complete_does_not_fire_when_one_pane_in_progress() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();

        let m1 = base_metrics(
            PaneActivityState::Completed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let m2 = base_metrics(
            PaneActivityState::Working,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        let cfg = NotificationsConfig {
            all_panes_complete: Some(crate::ports::NotificationCategoryConfig {
                enabled: Some(true),
            }),
            ..Default::default()
        };

        let fired =
            deliver_all_panes_complete(Some(&cfg), &[m1, m2], 12345, &mut outbox, Some(&port));
        assert!(!fired, "must not fire when at least one pane is Working");
    }

    // ── Stable TransitionSignature ────────────────────────────────
    //
    // build_signature MUST NOT depend on `now_ms`, freshness age, or
    // any wall-clock metric. The signature for a given ACP event
    // history must be identical across evaluations. These tests pin
    // that contract.

    /// Same source ActivityEventId at two different observed-at
    /// timestamps and two different `provider_freshness_age` values
    /// MUST produce the same `TransitionSignature`.
    #[test]
    fn transition_signature_is_stable_across_now_ms_when_source_event_id_unchanged() {
        let id = ActivityEventId(uuid::Uuid::from_u128(
            0xDEAD_BEEF_CAFE_F00D_0011_2233_4455_6677,
        ));

        // Same metrics state but two very different freshness ages —
        // mimicking the trigger being called at t=100ms and again at
        // t=10_000ms with the same underlying event.
        let mut m_at_t1 = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m_at_t1.provider_freshness_age = MetricValue::Known(std::time::Duration::from_millis(100));

        let mut m_at_t2 = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m_at_t2.provider_freshness_age =
            MetricValue::Known(std::time::Duration::from_millis(10_000));

        // The candidate uses the same event id at both evaluations
        // (the observed-at timestamp is also the same because it
        // comes from the event itself, not from `now_ms`).
        let c1 = cand_with(m_at_t1, id, 5_000);
        let c2 = cand_with(m_at_t2, id, 5_000);

        let sig1 = build_signature(&c1, NotificationCategory::PermissionNeeded);
        let sig2 = build_signature(&c2, NotificationCategory::PermissionNeeded);

        assert_eq!(
            sig1, sig2,
            "TransitionSignature MUST be stable across different now_ms / freshness_age \
             when the source ActivityEventId is unchanged"
        );
    }

    /// In the fallback path (no source_event_id) the signature is
    /// keyed on `latest_event_observed_at_unix_ms`, which is captured
    /// from the event itself. Two evaluations of the same event
    /// history MUST still produce the same signature, even when
    /// `provider_freshness_age` changes wildly.
    #[test]
    fn transition_signature_fallback_is_stable_across_now_ms_when_observed_at_unchanged() {
        let mut m_at_t1 = base_metrics(
            PaneActivityState::Failed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m_at_t1.provider_freshness_age = MetricValue::Known(std::time::Duration::from_millis(50));

        let mut m_at_t2 = base_metrics(
            PaneActivityState::Failed,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m_at_t2.provider_freshness_age =
            MetricValue::Known(std::time::Duration::from_millis(60_000));

        // Fallback path: no source_event_id, but the same captured
        // observed-at unix ms (the event log is append-only and
        // monotonic).
        let c1 = NotificationCandidate {
            metrics: m_at_t1,
            source_event_id: None,
            latest_event_observed_at_unix_ms: Some(12_345),
        };
        let c2 = NotificationCandidate {
            metrics: m_at_t2,
            source_event_id: None,
            latest_event_observed_at_unix_ms: Some(12_345),
        };

        let sig1 = build_signature(&c1, NotificationCategory::Failed);
        let sig2 = build_signature(&c2, NotificationCategory::Failed);

        assert_eq!(
            sig1, sig2,
            "Fallback TransitionSignature MUST be stable when the source \
             observed-at-unix-ms is unchanged"
        );
    }

    /// Two evaluations that share the same source_event_id must
    /// dedupe even when `provider_freshness_age` is different.
    #[test]
    fn second_evaluation_with_same_event_id_is_deduped_even_when_freshness_age_differs() {
        let mut outbox = NotificationOutbox::new();
        let port = FakePort::succeeding();
        let id = ActivityEventId(uuid::Uuid::from_u128(
            0xAAAA_BBBB_CCCC_DDDD_1111_2222_3333_4444,
        ));

        // First eval: freshness_age = 100ms.
        let mut m1 = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m1.provider_freshness_age = MetricValue::Known(std::time::Duration::from_millis(100));
        let c1 = cand_with(m1, id, 7_000);

        let r1 = deliver_notification(None, &c1, &mut outbox, Some(&port));
        assert!(r1.is_none(), "first evaluation must deliver");
        assert_eq!(outbox.delivered_count(), 1);

        // Second eval: same event id, but freshness_age advanced to
        // 30_000ms because the trigger ran again at a later wall
        // clock. MUST still dedupe.
        let mut m2 = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        m2.provider_freshness_age = MetricValue::Known(std::time::Duration::from_millis(30_000));
        let c2 = cand_with(m2, id, 7_000);

        let r2 = deliver_notification(None, &c2, &mut outbox, Some(&port));
        assert_eq!(
            r2,
            Some(DeliveryRefusal::AlreadyNotified),
            "second evaluation with same event id MUST dedupe regardless of freshness_age"
        );
        assert_eq!(outbox.delivered_count(), 1);
    }

    /// Cross-invocation dedupe via the persistent
    /// outbox-store boundary. Simulates two distinct trigger
    /// invocations that each construct a fresh `NotificationOutbox`,
    /// seeded from a shared persistent store.
    #[test]
    fn dedupe_holds_across_two_evaluations_when_outbox_is_seeded_from_persistent_store() {
        // Single source-of-truth dedupe set, mimicking the
        // FsNotificationOutboxStore on disk.
        let mut persistent: Vec<TransitionSignature> = Vec::new();

        let id = ActivityEventId(uuid::Uuid::from_u128(
            0x1357_2468_ACEF_BDB0_DEAD_BEEF_FEED_FACE,
        ));
        let metrics = base_metrics(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );

        // ── Invocation 1: fresh outbox, seeded from empty store ──
        let mut outbox1 = NotificationOutbox::new();
        outbox1.seed_delivered(persistent.iter().cloned());
        let port1 = FakePort::succeeding();
        let c1 = cand_with(metrics.clone(), id, 1_000);
        let r1 = deliver_notification(None, &c1, &mut outbox1, Some(&port1));
        assert!(r1.is_none(), "first invocation must deliver");
        assert_eq!(outbox1.delivered_count(), 1);
        // Flush newly-delivered sigs to the persistent store.
        for sig in outbox1.delivered_signatures() {
            persistent.push(sig.clone());
        }

        // ── Invocation 2: fresh outbox, seeded from the prior store ──
        let mut outbox2 = NotificationOutbox::new();
        outbox2.seed_delivered(persistent.iter().cloned());
        let port2 = FakePort::succeeding();
        let c2 = cand_with(metrics, id, 1_000);
        let r2 = deliver_notification(None, &c2, &mut outbox2, Some(&port2));
        assert_eq!(
            r2,
            Some(DeliveryRefusal::AlreadyNotified),
            "second invocation MUST dedupe via seeded persistent store"
        );
        assert_eq!(
            outbox2.delivered_count(),
            0,
            "no new delivery in second invocation"
        );
    }
}

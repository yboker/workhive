//! Event-driven notification trigger boundary.
//!
//! ## Why this module exists
//!
//! The prior trigger surface — a single
//! evaluation in [`pane_supervisor::deliver_supervisor_notifications`]
//! after the supervised child process exits — was insufficient:
//!
//! * `permission-needed` events are recorded mid-turn but the
//!   notification only fired at exit, so the user was not alerted
//!   until the provider had already given up; defeating the
//!   high-priority signal.
//! * `failed` / `completed` events that landed close to exit happened
//!   to fire, but only as a side effect of supervisor exit, not as a
//!   first-class event-driven evaluation.
//! * `all-panes-complete` was *implementable* in the use case layer
//!   ([`deliver_all_panes_complete`]) but had **no production wiring**;
//!   it could only be exercised by tests.
//!
//! This module is the missing event-driven boundary. Every CLI ACP
//! ingestion entry point — `cmd_internal_activity_record`,
//! `cmd_internal_activity_hook`, and the inner of
//! `cmd_internal_activity_codex_notify` — calls
//! [`evaluate_after_acp_append`] **immediately after a successful ACP
//! event append**. Evaluation:
//!
//! 1. Resolves the notifications config through the existing typed
//!    fail-closed gate ([`resolve_supervisor_notifications_config`]).
//!    An untrusted decision silences the trigger entirely.
//! 2. Loads the affected pane's events via
//!    [`ActivityEventStore::events_for_pane`].
//! 3. Projects a [`PaneActivityFact`] from those events via
//!    [`project_pane_events`] so the metrics carry a real state /
//!    confidence / freshness instead of `Unknown` defaults.
//! 4. Computes [`PaneActivityMetrics`] from the events + projected
//!    fact and calls [`deliver_notifications_for_panes`] for the
//!    per-pane categories (permission-needed / failed / completed).
//! 5. Loads the cross-pane stream via
//!    [`ActivityEventStore::replay_all`], filters to panes in the same
//!    `(group, workspace)` cohort as the trigger, projects per-pane
//!    facts + metrics, and calls [`deliver_all_panes_complete`] so the
//!    workspace-level notification fires exactly when the *last* pane
//!    in the workspace transitions to a fresh completion-like state.
//!
//! ## Fail-open envelope
//!
//! Per the error-handling policy and the [`NotificationPort`] fail-open contract,
//! every error path inside this module degrades to a `tracing` event
//! under [`workhive_infra::notification::NOTIFICATION_TRACING_TARGET`]
//! and returns to the caller. Recording activity must never block on
//! notification side effects, even when the event store is
//! temporarily unavailable. The fail-open tests in this module
//! exercise the failure paths to keep that property regression-safe.
//!
//! ## Why the trigger lives in `workhive-cli`
//!
//! The trigger composes a [`NotificationPort`] (an infra adapter)
//! with the [`deliver_notification`] use case (core). That fan-in is
//! the textbook composition root responsibility, and `workhive-cli`
//! is the only place allowed to depend on both `workhive-core` and
//! `workhive-infra` (per the crate dependency direction rule). Putting
//! the trigger in `workhive-core` would force the use case layer to
//! know about `osascript`; putting it in `workhive-infra` would
//! invert the dependency graph.
//!
//! ## Outbox lifetime
//!
//! Each evaluation uses a **fresh** [`NotificationOutbox`] in
//! memory, but the fresh outbox is **seeded** from a durable
//! [`NotificationOutboxStore`] at the start of every invocation and
//! **flushes** newly-delivered signatures back to that store before
//! returning. The store is rooted at
//! `<runtime_root>/notifications/outbox.jsonl` and is the dedupe
//! boundary that makes the trigger spam-safe across multiple
//! `evaluate_after_acp_append` invocations — without it the second
//! ACP event for the same pane projection would re-fire delivery
//! because each invocation otherwise sees an empty outbox.
//!
//! Store I/O follows the fail-open contract: load failures degrade
//! to an empty seed (worst case: one duplicate delivery), and
//! record failures degrade to a `warn` log. The supervisor and
//! provider hook are NEVER blocked on outbox persistence.
//!
//! Within one CLI invocation we may evaluate the same pane twice
//! (per-pane + cross-pane); using a single outbox per call lets
//! the cross-pane evaluation see what the per-pane evaluation
//! just dispatched, so we do not double-fire within a single
//! invocation either.
//!
//! [`PaneActivityFact`]: workhive_core::domain::activity::PaneActivityFact
//! [`project_pane_events`]: workhive_core::usecases::activity_control_plane::project_pane_events
//! [`pane_supervisor::deliver_supervisor_notifications`]: crate::commands::pane_supervisor::deliver_supervisor_notifications

use std::collections::BTreeMap;

use camino::Utf8Path;
use workhive_core::domain::activity::{ActivityEvent, PaneActivityFact, PaneCorrelationKey};
use workhive_core::ports::{
    ActivityEventStore, ActivityEventStoreError, ConfigPort, NotificationOutbox,
    NotificationOutboxStore, NotificationPort, NotificationsConfig,
};
use workhive_core::usecases::activity_control_plane::metrics::{
    FreshnessThresholds, PaneActivityMetrics, compute_pane_metrics,
};
use workhive_core::usecases::activity_control_plane::project_pane_events;
use workhive_core::usecases::deliver_notification::{
    NotificationCandidate, deliver_all_panes_complete, deliver_notifications_for_panes,
};

use crate::commands::pane_supervisor::{
    NotificationsConfigDecision, resolve_supervisor_notifications_config,
};

// ── Trigger source ────────────────────────────────────────────────────────────

/// Why the notification trigger fired.
///
/// Recorded in the trigger's tracing breadcrumb so a future review
/// can audit which ACP ingestion path each delivered notification
/// came from. Trigger-source metadata never leaks into the
/// user-facing [`PendingNotification`].
///
/// [`PendingNotification`]: workhive_core::ports::PendingNotification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationTriggerSource {
    /// A `workhive internal activity record` invocation appended an
    /// ACP event.
    ActivityRecord,
    /// A `workhive internal activity hook --provider …` invocation
    /// appended an ACP event from a provider hook payload.
    ActivityHook,
    /// The Codex notify wrapper invoked
    /// `workhive internal activity codex-notify` and an ACP event
    /// was appended.
    CodexNotify,
    /// The supervised child process exited; the supervisor performs
    /// a final evaluation. Kept for backward compatibility with the
    /// exit-only trigger; new callers prefer the per-event paths
    /// above.
    SupervisorExit,
}

impl NotificationTriggerSource {
    fn label(self) -> &'static str {
        match self {
            Self::ActivityRecord => "activity_record",
            Self::ActivityHook => "activity_hook",
            Self::CodexNotify => "codex_notify",
            Self::SupervisorExit => "supervisor_exit",
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Evaluate notifications immediately after an ACP event append.
///
/// `config_port` is the trusted process config (a load failure
/// returns [`NotificationsConfigDecision::Untrusted`] and silences
/// the trigger entirely).
/// `project_root` is the trusted project root the supervisor staged
/// events for; pass `None` only when the project root cannot be
/// derived.
/// `event_store` is the same ACP event store the just-completed
/// append wrote to.
/// `outbox_store` is the durable, local-only dedupe boundary
/// for delivered signatures. When `Some`, the trigger seeds its in-memory
/// outbox from the store at the start of the call and flushes
/// newly-delivered signatures back to the store before returning.
/// Pass `None` only in tests that want to exercise the
/// memory-only behaviour; every production call site MUST pass
/// the file-backed adapter.
/// `pane_key` is the pane whose event was just appended.
/// `source` is the ingestion entry point that triggered evaluation
/// (used for diagnostics only — never reaches the
/// [`PendingNotification`]).
/// `port` is a borrowed [`NotificationPort`]. Production callers use
/// [`workhive_infra::notification::production_notification_port`]; tests
/// use a capturing fake.
///
/// This is the **only** event-driven trigger path the CLI exposes.
///
/// [`PendingNotification`]: workhive_core::ports::PendingNotification
#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_after_acp_append<C: ConfigPort + ?Sized>(
    config_port: &C,
    project_root: Option<&Utf8Path>,
    event_store: &dyn ActivityEventStore,
    outbox_store: Option<&dyn NotificationOutboxStore>,
    pane_key: &PaneCorrelationKey,
    source: NotificationTriggerSource,
    port: &dyn NotificationPort,
    now_ms: i64,
) {
    // Resolve the notifications config via the existing typed
    // fail-closed gate. An untrusted config decision MUST silence
    // the trigger entirely — we do NOT fall back to built-in
    // defaults.
    let cfg = match resolve_supervisor_notifications_config(config_port, project_root) {
        NotificationsConfigDecision::Trusted(cfg) => cfg,
        NotificationsConfigDecision::Untrusted { reason } => {
            tracing::warn!(
                target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
                source = source.label(),
                pane_group = %pane_key.group,
                pane_workspace = %pane_key.workspace,
                pane_unit_id = %pane_key.unit_id,
                reason = %reason,
                "notification trigger: config untrusted; silencing evaluation (fail-closed)"
            );
            return;
        }
    };

    // Per-pane evaluation: load this pane's events, project a fact,
    // build metrics, and run the per-pane delivery loop.
    let per_pane_events = match event_store.events_for_pane(pane_key) {
        Ok(evts) => evts,
        Err(err) => {
            log_event_store_unavailable(&err, pane_key, source);
            return;
        }
    };

    let metrics = build_pane_metrics(pane_key, &per_pane_events, now_ms);

    // ── Seed the outbox from the persistent store ──────────────────
    //
    // Without this seed, the fresh `NotificationOutbox` would treat
    // every invocation as the first, and the second ACP event for
    // the same pane projection would re-fire delivery. The seed
    // restores prior-delivered signatures so dedupe holds across
    // invocations.
    let mut outbox = NotificationOutbox::default();
    if let Some(store) = outbox_store {
        match store.load_delivered_signatures() {
            Ok(sigs) => outbox.seed_delivered(sigs),
            Err(err) => {
                tracing::warn!(
                    target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
                    source = source.label(),
                    pane_group = %pane_key.group,
                    pane_workspace = %pane_key.workspace,
                    pane_unit_id = %pane_key.unit_id,
                    error = %err,
                    "notification trigger: outbox store seed failed; \
                     evaluating with empty dedupe set (fail-open)"
                );
            }
        }
    }

    // Build the stable notification candidate: the
    // signature is keyed on the latest ACP event id in the pane's
    // history, not on freshness age. Two evaluations of the same
    // history at different `now_ms` produce identical signatures.
    let candidate = build_candidate(metrics, &per_pane_events);
    deliver_notifications_for_panes(
        cfg.as_ref(),
        std::slice::from_ref(&candidate),
        &mut outbox,
        Some(port),
    );

    // Cross-pane evaluation: load the global event stream, filter to
    // (group, workspace) of the trigger pane, build per-pane metrics
    // for the cohort, and check `deliver_all_panes_complete`. Re-use
    // the same outbox so a workspace notification is not
    // double-emitted by per-pane + cross-pane paths within this
    // single invocation.
    evaluate_all_panes_complete(
        event_store,
        pane_key,
        cfg.as_ref(),
        &mut outbox,
        port,
        now_ms,
        source,
    );

    // ── Flush newly-delivered signatures back to
    //    the persistent store so the next invocation dedupes them.
    //
    // Refused and failed entries are NOT flushed because they did
    // not produce a user-visible notification.
    if let Some(store) = outbox_store {
        for sig in outbox.delivered_signatures() {
            if let Err(err) = store.record_delivered(sig) {
                tracing::warn!(
                    target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
                    source = source.label(),
                    pane_group = %pane_key.group,
                    pane_workspace = %pane_key.workspace,
                    pane_unit_id = %pane_key.unit_id,
                    error = %err,
                    "notification trigger: outbox store record failed; \
                     next invocation may re-deliver this signature (fail-open)"
                );
            }
        }
    }

    tracing::debug!(
        target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
        source = source.label(),
        pane_group = %pane_key.group,
        pane_workspace = %pane_key.workspace,
        pane_unit_id = %pane_key.unit_id,
        outbox_len = outbox.entries().len(),
        "notification trigger: evaluation complete"
    );
}

/// Build a stable [`NotificationCandidate`] from pane metrics + the
/// pane's event history.
///
/// The candidate carries the latest ACP event id and
/// its `observed_at_unix_ms` so the [`TransitionSignature`] built
/// downstream is purely event-driven. Two evaluations of the same
/// history at different `now_ms` will compute the same signature.
///
/// [`TransitionSignature`]: workhive_core::ports::TransitionSignature
fn build_candidate(
    metrics: PaneActivityMetrics,
    events: &[ActivityEvent],
) -> NotificationCandidate {
    match events.last() {
        Some(latest) => {
            NotificationCandidate::with_source_event(metrics, latest.id, latest.observed_at_unix_ms)
        }
        None => NotificationCandidate::from_metrics(metrics),
    }
}

// ── Cross-pane evaluation ─────────────────────────────────────────────────────

/// Evaluate `all-panes-complete` for the workspace cohort that owns
/// `trigger_pane`. Reads `replay_all`, partitions by
/// `(group, workspace)`, projects per-pane facts + metrics for the
/// cohort, and calls [`deliver_all_panes_complete`] with a
/// deterministic dedupe seq derived from the cohort's latest event
/// timestamp.
fn evaluate_all_panes_complete(
    event_store: &dyn ActivityEventStore,
    trigger_pane: &PaneCorrelationKey,
    cfg: Option<&NotificationsConfig>,
    outbox: &mut NotificationOutbox,
    port: &dyn NotificationPort,
    now_ms: i64,
    source: NotificationTriggerSource,
) {
    let all_events = match event_store.replay_all() {
        Ok(evts) => evts,
        Err(err) => {
            tracing::debug!(
                target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
                source = source.label(),
                pane_group = %trigger_pane.group,
                pane_workspace = %trigger_pane.workspace,
                error = %err,
                "notification trigger: cross-pane replay unavailable; \
                 skipping all-panes-complete evaluation (fail-open)"
            );
            return;
        }
    };

    // Bucket by pane within the trigger's (group, workspace).
    let mut cohort: BTreeMap<PaneCorrelationKey, Vec<ActivityEvent>> = BTreeMap::new();
    let mut max_seq: i64 = 0;
    for ev in &all_events {
        if ev.pane.group != trigger_pane.group || ev.pane.workspace != trigger_pane.workspace {
            continue;
        }
        if ev.observed_at_unix_ms > max_seq {
            max_seq = ev.observed_at_unix_ms;
        }
        cohort.entry(ev.pane.clone()).or_default().push(ev.clone());
    }

    if cohort.is_empty() {
        // No ACP-source panes in this workspace — nothing to evaluate.
        return;
    }

    let metrics_list: Vec<PaneActivityMetrics> = cohort
        .iter()
        .map(|(pane, events)| build_pane_metrics(pane, events, now_ms))
        .collect();

    let _attempted = deliver_all_panes_complete(cfg, &metrics_list, max_seq, outbox, Some(port));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a [`PaneActivityMetrics`] anchored at `now_ms` by projecting
/// the events through the canonical projector + metrics pipeline.
///
/// Uses the same conservative freshness window
/// ([`FreshnessThresholds::default_conservative`]) the legacy
/// supervisor path used so behavior is identical between the two
/// surfaces.
fn build_pane_metrics(
    pane: &PaneCorrelationKey,
    events: &[ActivityEvent],
    now_ms: i64,
) -> PaneActivityMetrics {
    let thresholds = FreshnessThresholds::default_conservative();
    let fact: PaneActivityFact = project_pane_events(
        pane,
        events,
        now_ms,
        thresholds.stale_after_ms,
        thresholds.expired_after_ms,
    );
    compute_pane_metrics(pane, events, Some(&fact), now_ms, thresholds)
}

fn log_event_store_unavailable(
    err: &ActivityEventStoreError,
    pane_key: &PaneCorrelationKey,
    source: NotificationTriggerSource,
) {
    tracing::debug!(
        target: workhive_infra::notification::NOTIFICATION_TRACING_TARGET,
        source = source.label(),
        pane_group = %pane_key.group,
        pane_workspace = %pane_key.workspace,
        pane_unit_id = %pane_key.unit_id,
        error = %err,
        "notification trigger: per-pane events unavailable; \
         skipping evaluation (fail-open)"
    );
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "notification_trigger_tests.rs"]
mod tests;

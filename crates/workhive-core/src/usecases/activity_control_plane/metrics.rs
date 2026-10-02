//! ACP event-derived timing and freshness metrics.
//!
//! All metric values are derived purely from the **append-only**
//! [`ActivityEvent`] history and projected read models. This module
//! never parses raw provider payloads, never calls out to the
//! network, and never modifies provider hook installation.
//!
//! ## Design
//!
//! Each metric returns a [`MetricValue<T>`] instead of a raw `T`:
//!
//! - [`MetricValue::Known`] — the value was computed with full
//!   confidence from a complete, coherent event window.
//! - [`MetricValue::Partial`] — the value was computed but the
//!   history was truncated, the confidence was low, or the pane
//!   was in an in-progress state (e.g. a turn that has not yet
//!   completed). The `reason` field documents exactly why.
//! - [`MetricValue::Unavailable`] — no usable data exists (e.g.
//!   empty history, provider disabled, store corrupt). Callers
//!   must not fabricate a fallback.
//!
//! ## Time representation
//!
//! All span/duration values use [`std::time::Duration`]. Event
//! timestamps are stored as `i64` Unix milliseconds (matching
//! [`ActivityEvent::observed_at_unix_ms`]). The "now" anchor is
//! supplied by callers through the [`ClockPort`] port so the
//! function is fully deterministic under tests.
//!
//! ## Legacy fallback
//!
//! [`PaneActivityMetrics`] carries an `acp_source` flag. When ACP
//! event history is empty or unavailable the caller (`ReadModel`)
//! falls back to the legacy `PaneActivityStorePort` latest-fact
//! path. In that case `acp_source = false` and timing metrics are
//! `Unavailable` (the legacy fact carries no timing history).
//!
//! Legacy producers / consumers at the time of this implementation
//! batch (Activity Intelligence v1, 2026-05-15):
//!
//! - **Legacy producer**: `workhive_infra::pane_activity` — writes
//!   the latest [`AiActivityFact`] to a per-pane JSON file on every
//!   provider hook/plugin event. Still alive as of this batch.
//! - **Legacy consumer**: `workhive_cli` pane list/show rendering —
//!   reads via `PaneActivityStorePort::read`. Continues to work for
//!   panes whose ACP event history is absent or corrupt.
//! - **ACP-preferred consumer**: this module and the
//!   `overlay_bridge` module — prefer the append-only event log and
//!   produce richer typed metrics; fall back to legacy when
//!   unavailable.
//!
//! The legacy latest-fact surface is scheduled for removal once all
//! consumers migrate to the event-projection read model.

use std::time::Duration;

use thiserror::Error;

use crate::domain::activity::{
    ActivityEvent, ActivityEventKind, ObservationConfidence, PaneActivityFact, PaneActivityState,
    PaneCorrelationKey, ProviderKind, SignalChannel, SignalFreshness,
};

// ── MetricValue ──────────────────────────────────────────────────────────────

/// A metric value that may be completely known, partially computed,
/// or unavailable due to missing / corrupt / disabled history.
///
/// Callers must pattern-match on this enum; they must not treat
/// [`Partial`] or [`Unavailable`] as zero.
///
/// [`Partial`]: MetricValue::Partial
/// [`Unavailable`]: MetricValue::Unavailable
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricValue<T> {
    /// The metric was derived with full confidence from a complete,
    /// coherent event window.
    Known(T),
    /// The metric was computed from an incomplete window. The `value`
    /// field holds a best-effort lower bound (e.g. elapsed since the
    /// start of an in-progress turn). Do not treat it as exact.
    Partial {
        /// Best-effort lower bound. Present when computation was
        /// possible even if history was incomplete.
        value: Option<T>,
        /// Why the metric is only partial.
        reason: PartialReason,
    },
    /// No usable data at all for this metric. Do not fabricate a
    /// fallback; surface the reason directly to the caller.
    Unavailable {
        /// Why the metric is unavailable.
        reason: UnavailableReason,
    },
}

/// Why a metric value is [`MetricValue::Partial`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartialReason {
    /// The event window is shorter than a complete event pair (e.g.
    /// only one end of a turn/tool pair was recorded, meaning the
    /// turn/tool is still in progress or the start event was not
    /// captured).
    InsufficientHistory,
    /// The latest relevant event is stale enough to lower confidence
    /// but not completely absent.
    Stale,
    /// The underlying observation had low or medium-low confidence
    /// (e.g. tmux/process observer, supervisor record without
    /// lifecycle confirmation).
    LowConfidence,
    /// A state transition is in progress; the metric will be `Known`
    /// once the transition completes.
    InProgress,
    /// The event history appears corrupt (e.g. impossible timestamps
    /// or missing required fields).
    Corrupt,
    /// The reason cannot be determined from the available history.
    Unknown,
}

/// Why a metric value is [`MetricValue::Unavailable`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableReason {
    /// No events have been recorded for this pane yet.
    NoHistory,
    /// The provider integration is disabled by feature gate.
    Disabled,
    /// The provider integration is unsupported on this platform.
    Unsupported,
    /// The provider integration is in a degraded state.
    Degraded,
    /// The state is completely unknown (no evidence at all).
    Unknown,
}

// ── PaneActivityMetrics ───────────────────────────────────────────────────────

/// Provider freshness thresholds used by [`PaneActivityMetrics`].
///
/// Both values must be positive and ordered (`stale_after_ms <=
/// expired_after_ms`). Invalid configurations produce
/// [`MetricValue::Unavailable`] for freshness metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreshnessThresholds {
    /// Age (in ms) above which the latest event is considered stale.
    pub stale_after_ms: i64,
    /// Age (in ms) above which the latest event is considered expired
    /// (no longer useful for rendering).
    pub expired_after_ms: i64,
}

impl FreshnessThresholds {
    /// Default conservative thresholds.
    ///
    /// - Stale after 30 s (30_000 ms)
    /// - Expired after 90 s (90_000 ms)
    pub const fn default_conservative() -> Self {
        Self {
            stale_after_ms: 30_000,
            expired_after_ms: 90_000,
        }
    }

    /// Validate that the thresholds are sane (positive and ordered).
    pub fn is_valid(self) -> bool {
        self.stale_after_ms > 0 && self.expired_after_ms >= self.stale_after_ms
    }
}

/// All timing and freshness metrics for a single pane, derived from
/// the append-only [`ActivityEvent`] history.
///
/// This is the stable typed output consumed by Agent C (dashboard /
/// CLI rendering) and Agent D (notifications / orchestration).
/// Fields named `*_duration` carry [`MetricValue<Duration>`] values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneActivityMetrics {
    /// Pane these metrics belong to.
    pub pane: PaneCorrelationKey,
    /// The provider the metrics were computed for, if known.
    pub provider: Option<ProviderKind>,
    /// Most recently projected pane state.
    pub state: PaneActivityState,
    /// Signal channel for the most recent event.
    pub channel: SignalChannel,
    /// Confidence of the most recent projected fact.
    pub confidence: ObservationConfidence,
    /// Freshness of the most recent projected fact.
    pub freshness: SignalFreshness,

    /// How long the pane has been in its current state (measured
    /// from the wall-clock time of the last state-changing event to
    /// `now`). Partial when the pane state has never had a
    /// state-change event recorded; Unavailable when history is empty.
    pub elapsed: MetricValue<Duration>,

    /// Duration of the **current in-progress agent turn** (from the
    /// most recent `PromptSubmitted` event to `now`). Partial when
    /// the turn is still running; Known only after `TurnCompleted` /
    /// `TurnFailed` / `TurnNotifiedComplete` closes the pair.
    pub current_turn_duration: MetricValue<Duration>,

    /// Duration of the **last completed** agent turn (from its
    /// `PromptSubmitted` event to its `TurnCompleted` /
    /// `TurnFailed` / `TurnNotifiedComplete` event). Unavailable if
    /// no completed turn has been recorded.
    pub last_completed_turn_duration: MetricValue<Duration>,

    /// Duration of the **last completed** tool invocation (from its
    /// `ToolStarted` event to its `ToolFinished` event).
    /// Unavailable if no completed tool pair was found.
    pub last_tool_duration: MetricValue<Duration>,

    /// Aggregate tool duration over all completed tool pairs in the
    /// event window. Partial when the window contains at least one
    /// incomplete tool pair or when only a subset of pairs falls
    /// inside the retention window.
    pub aggregate_tool_duration: MetricValue<Duration>,

    /// How long the pane has been waiting for user permission (from
    /// the most recent `PermissionRequested` event to `now`). Known
    /// only when a `PermissionResolved` event has been recorded
    /// (i.e. the wait is complete); Partial while the permission is
    /// still unresolved.
    pub permission_wait_duration: MetricValue<Duration>,

    /// How long the pane has been idle (from the most recent
    /// `SessionIdle` event to `now`). Known when an `SessionIdle`
    /// event was the last state-changing event; Unavailable when no
    /// `SessionIdle` has been recorded.
    pub idle_duration: MetricValue<Duration>,

    /// Age of the most recent ACP event for this pane.
    /// `Known` when events exist; `Unavailable` when no events have
    /// been recorded. Callers compare against
    /// [`FreshnessThresholds`] to determine whether the provider is
    /// fresh, stale, or expired.
    pub provider_freshness_age: MetricValue<Duration>,

    /// Whether the metrics were derived from the ACP event history
    /// (`true`) or from the legacy latest-fact fallback (`false`).
    /// When `false`, all timing metrics are `Unavailable` because the
    /// legacy fact carries no timing history.
    pub acp_source: bool,
}

/// Error type for metric computation failures that prevent producing
/// even a degraded [`PaneActivityMetrics`].
#[derive(Debug, Error)]
pub enum MetricsError {
    /// The event store returned an I/O or corrupt-entry error.
    #[error("metric computation failed due to store error: {0}")]
    StoreError(String),
}

// ── Core computation ──────────────────────────────────────────────────────────

/// Compute [`PaneActivityMetrics`] for `pane` from `events` (ordered
/// oldest-first) anchored at `now_ms`.
///
/// This is the **pure** inner function. The [`ReadModel`] helper calls
/// it after loading events from the store; tests call it directly
/// with hand-crafted event slices and a known `now_ms` from a
/// `FakeClock`.
///
/// ## Partial / Unavailable semantics
///
/// - Empty `events` → all timing metrics are `Unavailable { reason:
///   NoHistory }` (but state/confidence/etc. are filled from the
///   `projected_fact` if provided).
/// - Provider disabled / unsupported / degraded → timing metrics are
///   `Unavailable { reason: Disabled / Unsupported / Degraded }`.
/// - Low-confidence events → timing metrics that rely solely on those
///   events carry `Partial { reason: LowConfidence }`.
/// - In-progress turn (no close event yet) → `current_turn_duration`
///   is `Partial { reason: InProgress, value: Some(elapsed_so_far) }`.
///
/// [`ReadModel`]: crate::domain::activity::ReadModel
pub fn compute_pane_metrics(
    pane: &PaneCorrelationKey,
    events: &[ActivityEvent],
    projected_fact: Option<&PaneActivityFact>,
    now_ms: i64,
    thresholds: FreshnessThresholds,
) -> PaneActivityMetrics {
    // Establish base fields from the projected fact (or defaults when
    // the fact is absent).
    let (state, provider, confidence, freshness, channel) = projected_fact
        .map(|f| (f.state, f.provider, f.confidence, f.freshness, f.channel))
        .unwrap_or((
            PaneActivityState::Unknown,
            None,
            ObservationConfidence::None,
            SignalFreshness::Unavailable,
            SignalChannel::ObserverPoll,
        ));

    // Lifecycle override: if the state is Disabled/Unsupported/
    // Degraded/Unknown, all timing metrics are Unavailable with the
    // matching reason — no computation needed.
    if let Some(unavail) = lifecycle_unavailable_reason(state) {
        return PaneActivityMetrics {
            pane: pane.clone(),
            provider,
            state,
            channel,
            confidence,
            freshness,
            elapsed: MetricValue::Unavailable { reason: unavail },
            current_turn_duration: MetricValue::Unavailable { reason: unavail },
            last_completed_turn_duration: MetricValue::Unavailable { reason: unavail },
            last_tool_duration: MetricValue::Unavailable { reason: unavail },
            aggregate_tool_duration: MetricValue::Unavailable { reason: unavail },
            permission_wait_duration: MetricValue::Unavailable { reason: unavail },
            idle_duration: MetricValue::Unavailable { reason: unavail },
            provider_freshness_age: MetricValue::Unavailable { reason: unavail },
            acp_source: false,
        };
    }

    // No ACP events yet — produce minimal Unavailable metrics.
    if events.is_empty() {
        return PaneActivityMetrics {
            pane: pane.clone(),
            provider,
            state,
            channel,
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
            provider_freshness_age: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            acp_source: false,
        };
    }

    // Derive individual metrics.
    let elapsed = compute_elapsed(events, projected_fact, now_ms);
    let (current_turn_duration, last_completed_turn_duration) =
        compute_turn_durations(events, now_ms);
    let (last_tool_duration, aggregate_tool_duration) = compute_tool_durations(events, now_ms);
    let permission_wait_duration = compute_permission_wait(events, now_ms);
    let idle_duration = compute_idle_duration(events, now_ms);
    let provider_freshness_age = compute_freshness_age(events, now_ms, thresholds);

    PaneActivityMetrics {
        pane: pane.clone(),
        provider,
        state,
        channel,
        confidence,
        freshness,
        elapsed,
        current_turn_duration,
        last_completed_turn_duration,
        last_tool_duration,
        aggregate_tool_duration,
        permission_wait_duration,
        idle_duration,
        provider_freshness_age,
        acp_source: true,
    }
}

// ── Per-metric computation helpers ───────────────────────────────────────────

/// Elapsed time since the pane entered its current state.
///
/// Uses the `observed_at_unix_ms` of the projected fact's source
/// event as the state-entry timestamp. Returns `Partial` with
/// `InProgress` reason when state is a busy state (Working /
/// ToolRunning / WaitingForPermission) because the state is
/// actively changing.
fn compute_elapsed(
    _events: &[ActivityEvent],
    projected_fact: Option<&PaneActivityFact>,
    now_ms: i64,
) -> MetricValue<Duration> {
    let Some(fact) = projected_fact else {
        return MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        };
    };

    if fact.observed_at_unix_ms == 0 {
        return MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        };
    }

    let age_ms = now_ms.saturating_sub(fact.observed_at_unix_ms);
    if age_ms < 0 {
        return MetricValue::Partial {
            value: None,
            reason: PartialReason::Corrupt,
        };
    }
    let duration = Duration::from_millis(age_ms as u64);

    // Busy states: the elapsed time is a lower bound (more events may
    // still arrive), so it is Partial/InProgress.
    if fact.state.is_busy() {
        return MetricValue::Partial {
            value: Some(duration),
            reason: PartialReason::InProgress,
        };
    }

    // Low-confidence facts: return Partial.
    if fact.confidence < ObservationConfidence::Medium {
        return MetricValue::Partial {
            value: Some(duration),
            reason: PartialReason::LowConfidence,
        };
    }

    MetricValue::Known(duration)
}

/// Compute turn durations.
///
/// Returns `(current_turn_duration, last_completed_turn_duration)`.
///
/// A turn is bounded by a `PromptSubmitted` → `TurnCompleted` /
/// `TurnFailed` / `TurnNotifiedComplete` pair. If the last
/// `PromptSubmitted` has no closing event, the current turn is
/// Partial/InProgress. The last *completed* pair (if any) is Known.
fn compute_turn_durations(
    events: &[ActivityEvent],
    now_ms: i64,
) -> (MetricValue<Duration>, MetricValue<Duration>) {
    // Walk the event slice to find turn boundaries. We need to find:
    // 1. The most recent PromptSubmitted event (open turn).
    // 2. All PromptSubmitted → close pairs (completed turns).

    // Collect (start_ms, close_ms_or_none) pairs in reverse order.
    let mut completed_pairs: Vec<(i64, i64)> = Vec::new();
    let mut open_start: Option<i64> = None;

    // Forward scan to pair events.
    let mut i = 0;
    while i < events.len() {
        if events[i].kind == ActivityEventKind::PromptSubmitted {
            let start_ms = events[i].observed_at_unix_ms;
            // Look for the closing event after this start.
            let close = events[i + 1..]
                .iter()
                .find(|e| is_turn_close(e.kind))
                .map(|e| e.observed_at_unix_ms);
            if let Some(close_ms) = close {
                completed_pairs.push((start_ms, close_ms));
            } else {
                // No close found: this is the open (in-progress) turn.
                open_start = Some(start_ms);
            }
        }
        i += 1;
    }

    // last_completed_turn_duration: the last pair in completed_pairs.
    let last_completed = last_completed_pair_duration(completed_pairs.last());

    // current_turn_duration: an open turn is Partial/InProgress; with no
    // open turn the last completed pair gives the "just finished" elapsed.
    let current_turn = match open_start {
        Some(start_ms) => {
            let elapsed_ms = now_ms.saturating_sub(start_ms);
            MetricValue::Partial {
                value: Some(Duration::from_millis(elapsed_ms as u64)),
                reason: PartialReason::InProgress,
            }
        }
        None => last_completed.clone(),
    };

    (current_turn, last_completed)
}

/// Map the most recent completed `(start_ms, close_ms)` turn pair to a
/// metric value: `Known` duration when ordered, `Partial/Corrupt` when the
/// close precedes the start, and `Unavailable/NoHistory` with no pair.
fn last_completed_pair_duration(pair: Option<&(i64, i64)>) -> MetricValue<Duration> {
    match pair {
        None => MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        },
        Some(&(start_ms, close_ms)) => {
            if close_ms < start_ms {
                MetricValue::Partial {
                    value: None,
                    reason: PartialReason::Corrupt,
                }
            } else {
                MetricValue::Known(Duration::from_millis((close_ms - start_ms) as u64))
            }
        }
    }
}

fn is_turn_close(kind: ActivityEventKind) -> bool {
    matches!(
        kind,
        ActivityEventKind::TurnCompleted
            | ActivityEventKind::TurnNotifiedComplete
            | ActivityEventKind::TurnFailed
    )
}

/// Compute tool durations.
///
/// Returns `(last_tool_duration, aggregate_tool_duration)`.
///
/// A tool pair is `ToolStarted` → `ToolFinished`. Incomplete pairs
/// (no `ToolFinished`) contribute to `aggregate_tool_duration` as
/// Partial/InProgress.
fn compute_tool_durations(
    events: &[ActivityEvent],
    now_ms: i64,
) -> (MetricValue<Duration>, MetricValue<Duration>) {
    let mut completed_pairs: Vec<Duration> = Vec::new();
    let mut has_incomplete = false;

    let mut i = 0;
    while i < events.len() {
        if events[i].kind == ActivityEventKind::ToolStarted {
            let start_ms = events[i].observed_at_unix_ms;
            // Find the matching ToolFinished.
            let finish = events[i + 1..]
                .iter()
                .find(|e| e.kind == ActivityEventKind::ToolFinished)
                .map(|e| e.observed_at_unix_ms);
            if let Some(fin_ms) = finish {
                if fin_ms >= start_ms {
                    completed_pairs.push(Duration::from_millis((fin_ms - start_ms) as u64));
                } else {
                    // Impossible timestamp: treat as partial.
                    has_incomplete = true;
                }
            } else {
                // Tool is still running or finish event was not captured.
                has_incomplete = true;
                let _ = now_ms; // incorporated via has_incomplete flag
            }
        }
        i += 1;
    }

    let last_tool = match completed_pairs.last() {
        Some(&d) => MetricValue::Known(d),
        None => {
            if has_incomplete {
                MetricValue::Partial {
                    value: None,
                    reason: PartialReason::InProgress,
                }
            } else {
                MetricValue::Unavailable {
                    reason: UnavailableReason::NoHistory,
                }
            }
        }
    };

    let aggregate_tool = if completed_pairs.is_empty() && !has_incomplete {
        MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        }
    } else {
        let total: Duration = completed_pairs.iter().sum();
        if has_incomplete {
            MetricValue::Partial {
                value: Some(total),
                reason: PartialReason::InProgress,
            }
        } else {
            MetricValue::Known(total)
        }
    };

    (last_tool, aggregate_tool)
}

/// Compute permission wait duration.
///
/// The wait starts at the most recent `PermissionRequested` event.
/// If it is followed by a `PermissionResolved` event, the duration
/// is Known. If it has not been resolved, it is Partial/InProgress.
fn compute_permission_wait(events: &[ActivityEvent], now_ms: i64) -> MetricValue<Duration> {
    // Find the most recent PermissionRequested.
    let Some(req_idx) = events
        .iter()
        .rposition(|e| e.kind == ActivityEventKind::PermissionRequested)
    else {
        return MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        };
    };

    let req_ms = events[req_idx].observed_at_unix_ms;

    // Is there a PermissionResolved after it?
    let resolved = events[req_idx + 1..]
        .iter()
        .find(|e| e.kind == ActivityEventKind::PermissionResolved)
        .map(|e| e.observed_at_unix_ms);

    match resolved {
        Some(res_ms) => {
            if res_ms < req_ms {
                MetricValue::Partial {
                    value: None,
                    reason: PartialReason::Corrupt,
                }
            } else {
                MetricValue::Known(Duration::from_millis((res_ms - req_ms) as u64))
            }
        }
        None => {
            // Permission is still unresolved.
            let elapsed_ms = now_ms.saturating_sub(req_ms);
            MetricValue::Partial {
                value: Some(Duration::from_millis(elapsed_ms as u64)),
                reason: PartialReason::InProgress,
            }
        }
    }
}

/// Compute idle duration.
///
/// The idle period starts at the most recent `SessionIdle` event and
/// runs to `now`. Known only when `SessionIdle` is the most recent
/// state-changing event; Unavailable if no `SessionIdle` was recorded.
fn compute_idle_duration(events: &[ActivityEvent], now_ms: i64) -> MetricValue<Duration> {
    let Some(idle_ev) = events
        .iter()
        .rev()
        .find(|e| e.kind == ActivityEventKind::SessionIdle)
    else {
        return MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        };
    };

    // If there is a more-recent non-telemetry, non-lifecycle event
    // after the SessionIdle, the pane is no longer idle.
    let has_later = events
        .iter()
        .rev()
        .take_while(|e| {
            e.kind != ActivityEventKind::SessionIdle
                || e.observed_at_unix_ms > idle_ev.observed_at_unix_ms
        })
        .any(|e| {
            !matches!(
                e.kind,
                ActivityEventKind::TelemetryStale
                    | ActivityEventKind::SessionIdle
                    | ActivityEventKind::IntegrationDisabled
                    | ActivityEventKind::IntegrationUnsupported
                    | ActivityEventKind::IntegrationDegraded
            ) && e.observed_at_unix_ms > idle_ev.observed_at_unix_ms
        });

    if has_later {
        return MetricValue::Partial {
            value: None,
            reason: PartialReason::InsufficientHistory,
        };
    }

    let idle_ms = now_ms.saturating_sub(idle_ev.observed_at_unix_ms);
    MetricValue::Known(Duration::from_millis(idle_ms as u64))
}

/// Compute provider freshness age.
///
/// Returns the age (in ms as Duration) of the most recent ACP event
/// for the pane. Known when events exist; Unavailable when history
/// is empty or thresholds are invalid. Also returns Partial with
/// `Stale` reason when the freshness threshold marks the event stale.
fn compute_freshness_age(
    events: &[ActivityEvent],
    now_ms: i64,
    thresholds: FreshnessThresholds,
) -> MetricValue<Duration> {
    if !thresholds.is_valid() {
        return MetricValue::Partial {
            value: None,
            reason: PartialReason::Unknown,
        };
    }

    let Some(latest) = events.last() else {
        return MetricValue::Unavailable {
            reason: UnavailableReason::NoHistory,
        };
    };

    let age_ms = now_ms.saturating_sub(latest.observed_at_unix_ms);
    if age_ms < 0 {
        return MetricValue::Partial {
            value: None,
            reason: PartialReason::Corrupt,
        };
    }
    let age = Duration::from_millis(age_ms as u64);

    // Mark Partial/Stale when the freshness threshold says the event
    // is stale (but not yet expired). Per requirement, the caller
    // should surface stale hints in this case.
    if age_ms >= thresholds.stale_after_ms {
        MetricValue::Partial {
            value: Some(age),
            reason: PartialReason::Stale,
        }
    } else {
        MetricValue::Known(age)
    }
}

/// Map a lifecycle-terminal state to an [`UnavailableReason`] when
/// the state precludes useful timing metrics.
///
/// Only explicit integration-outcome states (`Disabled`,
/// `Unsupported`, `Degraded`) short-circuit computation. `Unknown`
/// is **not** a lifecycle terminal — it simply means no evidence has
/// been recorded yet, so computation continues to the empty-history
/// path (which returns `Unavailable { NoHistory }`).
fn lifecycle_unavailable_reason(state: PaneActivityState) -> Option<UnavailableReason> {
    match state {
        PaneActivityState::Disabled => Some(UnavailableReason::Disabled),
        PaneActivityState::Unsupported => Some(UnavailableReason::Unsupported),
        PaneActivityState::Degraded => Some(UnavailableReason::Degraded),
        // Unknown, OpenUnverified, and all lifecycle states with
        // evidence allow metric computation to continue (empty-history
        // fallback applies when events is empty).
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, PaneActivityFact,
        PaneActivityState, ProviderKind, ProviderSignalSource, SignalChannel, SignalFreshness,
    };

    const NOW: i64 = 1_700_000_100_000;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u1")
    }

    fn thresholds() -> FreshnessThresholds {
        FreshnessThresholds {
            stale_after_ms: 30_000,
            expired_after_ms: 90_000,
        }
    }

    fn make_event(kind: ActivityEventKind, at_ms: i64) -> ActivityEvent {
        ActivityEvent::new(
            pane(),
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

    fn make_fact(
        state: PaneActivityState,
        at_ms: i64,
        confidence: ObservationConfidence,
    ) -> PaneActivityFact {
        PaneActivityFact {
            pane: pane(),
            provider: Some(ProviderKind::ClaudeCode),
            state,
            outcome: ActivityOutcome::NotApplicable,
            confidence,
            channel: SignalChannel::LifecycleHook,
            observed_at_unix_ms: at_ms,
            freshness: SignalFreshness::Fresh,
            source_event_id: None,
        }
    }

    // ── Elapsed time ──────────────────────────────────────────────

    #[test]
    fn elapsed_time_since_state_change_is_known_when_history_has_state_event() {
        // Pane entered Completed state 5 seconds ago.
        let state_at = NOW - 5_000;
        let events = vec![make_event(ActivityEventKind::TurnCompleted, state_at)];
        let fact = make_fact(
            PaneActivityState::Completed,
            state_at,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.elapsed,
            MetricValue::Known(Duration::from_millis(5_000)),
            "elapsed must be 5 seconds"
        );
    }

    #[test]
    fn elapsed_time_is_partial_when_history_is_empty() {
        let metrics = compute_pane_metrics(&pane(), &[], None, NOW, thresholds());

        assert!(
            matches!(
                metrics.elapsed,
                MetricValue::Unavailable {
                    reason: UnavailableReason::NoHistory
                }
            ),
            "empty history must yield Unavailable/NoHistory, got {:?}",
            metrics.elapsed
        );
    }

    // ── Turn duration ─────────────────────────────────────────────

    #[test]
    fn turn_duration_known_after_complete_turn() {
        let start = NOW - 10_000;
        let end = NOW - 2_000;
        let events = vec![
            make_event(ActivityEventKind::PromptSubmitted, start),
            make_event(ActivityEventKind::TurnCompleted, end),
        ];
        let fact = make_fact(
            PaneActivityState::Completed,
            end,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.last_completed_turn_duration,
            MetricValue::Known(Duration::from_millis(8_000)),
            "completed turn must be Known(8s)"
        );
    }

    #[test]
    fn turn_duration_partial_when_turn_in_progress() {
        let start = NOW - 7_000;
        let events = vec![make_event(ActivityEventKind::PromptSubmitted, start)];
        let fact = make_fact(
            PaneActivityState::Working,
            start,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        match &metrics.current_turn_duration {
            MetricValue::Partial {
                value: Some(d),
                reason: PartialReason::InProgress,
            } => {
                assert_eq!(*d, Duration::from_millis(7_000), "lower bound must be 7s");
            }
            other => panic!("expected Partial/InProgress, got {other:?}"),
        }
    }

    // ── Tool duration ─────────────────────────────────────────────

    #[test]
    fn tool_duration_known_for_last_completed_tool() {
        let start = NOW - 5_000;
        let end = NOW - 1_000;
        let events = vec![
            make_event(ActivityEventKind::ToolStarted, start),
            make_event(ActivityEventKind::ToolFinished, end),
        ];
        let fact = make_fact(PaneActivityState::Working, end, ObservationConfidence::High);
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.last_tool_duration,
            MetricValue::Known(Duration::from_millis(4_000)),
            "tool duration must be Known(4s)"
        );
    }

    #[test]
    fn tool_duration_aggregate_partial_when_history_truncated() {
        // One completed tool + one in-progress tool.
        let events = vec![
            make_event(ActivityEventKind::ToolStarted, NOW - 10_000),
            make_event(ActivityEventKind::ToolFinished, NOW - 8_000),
            // Second tool started but not finished.
            make_event(ActivityEventKind::ToolStarted, NOW - 3_000),
        ];
        let fact = make_fact(
            PaneActivityState::ToolRunning,
            NOW - 3_000,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        match &metrics.aggregate_tool_duration {
            MetricValue::Partial {
                value: Some(d),
                reason: PartialReason::InProgress,
            } => {
                // The known part is 2s (the completed tool).
                assert_eq!(*d, Duration::from_millis(2_000));
            }
            other => panic!("expected Partial/InProgress with Some(2s), got {other:?}"),
        }
    }

    // ── Permission wait ───────────────────────────────────────────

    #[test]
    fn permission_wait_known_for_resolved_permission() {
        let req_at = NOW - 8_000;
        let res_at = NOW - 2_000;
        let events = vec![
            make_event(ActivityEventKind::PermissionRequested, req_at),
            make_event(ActivityEventKind::PermissionResolved, res_at),
        ];
        let fact = make_fact(
            PaneActivityState::Working,
            res_at,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.permission_wait_duration,
            MetricValue::Known(Duration::from_millis(6_000)),
            "resolved permission must be Known(6s)"
        );
    }

    #[test]
    fn permission_wait_partial_when_permission_unresolved() {
        let req_at = NOW - 5_000;
        let events = vec![make_event(ActivityEventKind::PermissionRequested, req_at)];
        let fact = make_fact(
            PaneActivityState::WaitingForPermission,
            req_at,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        match &metrics.permission_wait_duration {
            MetricValue::Partial {
                value: Some(d),
                reason: PartialReason::InProgress,
            } => {
                assert_eq!(*d, Duration::from_millis(5_000));
            }
            other => panic!("expected Partial/InProgress(5s), got {other:?}"),
        }
    }

    // ── Idle duration ─────────────────────────────────────────────

    #[test]
    fn idle_duration_known_after_idle_transition() {
        let idle_at = NOW - 20_000;
        let events = vec![make_event(ActivityEventKind::SessionIdle, idle_at)];
        let fact = make_fact(
            PaneActivityState::Idle,
            idle_at,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.idle_duration,
            MetricValue::Known(Duration::from_millis(20_000)),
            "idle duration must be Known(20s)"
        );
    }

    // ── Provider freshness ────────────────────────────────────────

    #[test]
    fn provider_freshness_known_when_recent_event() {
        let at = NOW - 5_000;
        let events = vec![make_event(ActivityEventKind::TurnCompleted, at)];
        let fact = make_fact(
            PaneActivityState::Completed,
            at,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert_eq!(
            metrics.provider_freshness_age,
            MetricValue::Known(Duration::from_millis(5_000)),
            "recent event must produce Known freshness age"
        );
    }

    #[test]
    fn provider_freshness_stale_threshold_marks_stale() {
        let at = NOW - 40_000; // older than 30s stale threshold.
        let events = vec![make_event(ActivityEventKind::SessionIdle, at)];
        let fact = make_fact(PaneActivityState::Stale, at, ObservationConfidence::Low);
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        match &metrics.provider_freshness_age {
            MetricValue::Partial {
                value: Some(d),
                reason: PartialReason::Stale,
            } => {
                assert_eq!(*d, Duration::from_millis(40_000));
            }
            other => panic!("stale event must yield Partial/Stale with 40s, got {other:?}"),
        }
    }

    // ── Disabled / low-confidence / corrupt ───────────────────────

    #[test]
    fn metrics_unavailable_when_provider_disabled() {
        // The projected fact says Disabled — no events needed.
        let fact = PaneActivityFact {
            pane: pane(),
            provider: Some(ProviderKind::Codex),
            state: PaneActivityState::Disabled,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::High,
            channel: SignalChannel::SupervisorRecord,
            observed_at_unix_ms: NOW - 1_000,
            freshness: SignalFreshness::Fresh,
            source_event_id: None,
        };
        let metrics = compute_pane_metrics(&pane(), &[], Some(&fact), NOW, thresholds());

        assert!(
            matches!(
                metrics.elapsed,
                MetricValue::Unavailable {
                    reason: UnavailableReason::Disabled
                }
            ),
            "Disabled state must produce Unavailable/Disabled, got {:?}",
            metrics.elapsed
        );
        assert!(
            matches!(
                metrics.provider_freshness_age,
                MetricValue::Unavailable {
                    reason: UnavailableReason::Disabled
                }
            ),
            "Disabled must also mark freshness unavailable"
        );
    }

    #[test]
    fn metrics_partial_for_low_confidence_events() {
        let at = NOW - 5_000;
        let events = vec![make_event(ActivityEventKind::TurnCompleted, at)];
        // Low confidence fact (e.g. from tmux observer).
        let fact = make_fact(PaneActivityState::Completed, at, ObservationConfidence::Low);
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());

        assert!(
            matches!(
                metrics.elapsed,
                MetricValue::Partial {
                    reason: PartialReason::LowConfidence,
                    ..
                }
            ),
            "low confidence must produce Partial/LowConfidence for elapsed, got {:?}",
            metrics.elapsed
        );
    }

    #[test]
    fn metrics_partial_for_corrupt_or_unknown_state() {
        // An Unknown state projects to Unavailable/Unknown.
        let metrics = compute_pane_metrics(&pane(), &[], None, NOW, thresholds());

        assert!(
            matches!(
                metrics.elapsed,
                MetricValue::Unavailable {
                    reason: UnavailableReason::NoHistory
                }
            ),
            "unknown state must produce Unavailable/NoHistory, got {:?}",
            metrics.elapsed
        );
    }

    // ── acp_source flag ───────────────────────────────────────────

    #[test]
    fn acp_source_is_true_when_events_present() {
        let events = vec![make_event(ActivityEventKind::TurnCompleted, NOW - 1_000)];
        let fact = make_fact(
            PaneActivityState::Completed,
            NOW - 1_000,
            ObservationConfidence::High,
        );
        let metrics = compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds());
        assert!(
            metrics.acp_source,
            "must be acp_source=true when events present"
        );
    }

    #[test]
    fn acp_source_is_false_when_events_empty() {
        let metrics = compute_pane_metrics(&pane(), &[], None, NOW, thresholds());
        assert!(
            !metrics.acp_source,
            "must be acp_source=false when no events"
        );
    }
}

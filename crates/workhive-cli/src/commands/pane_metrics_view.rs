//! Render helpers for [`PaneActivityMetrics`] in `pane list` and `pane show`.
//!
//! This module is a pure render layer: it consumes typed
//! [`PaneActivityMetrics`] values from the read model and produces
//!
//! - JSON-serialisable output structs (additive — existing JSON consumers
//!   must not break).
//! - Human-readable formatted strings that go through [`I18nPort`].
//! - Typed hint structs for permission-needed / completed / failed signals.
//!
//! No I/O, no tmux, no git. All time values must be passed in as `now_ms`;
//! callers must NOT call `SystemTime::now()` directly.
//!
//! ## JSON contract
//!
//! `PaneListMetricsJson` is additive to the existing `pane list` JSON
//! shape. Fields use `snake_case` and are `#[serde(skip_serializing_if)]`
//! guarded so a null/missing value leaves the pre-existing shape intact.
//!
//! `PaneShowMetricsJson` extends the `pane show` shape with per-metric
//! `MetricValue` representations for turn, tool, permission_wait, idle, and
//! freshness_age.
//!
//! ## Legacy fallback
//!
//! When `PaneActivityMetrics::acp_source == false` the caller must still
//! render whatever the legacy path provides. The helpers in this module
//! produce source = `"legacy"` and mark all timing metrics as unavailable
//! rather than hiding them. The consumer (pane.rs) is responsible for
//! deciding whether to fall back to the legacy overlay entry.

use std::time::Duration;

use serde::Serialize;
use workhive_core::{
    domain::activity::{PaneCorrelationKey, ProviderKind},
    ports::ActivityEventStore,
    usecases::{
        activity_control_plane::metrics::{
            FreshnessThresholds, MetricValue, PaneActivityMetrics, PartialReason, UnavailableReason,
        },
        pane::PaneListRow,
    },
};

#[allow(unused_imports)]
pub(crate) use super::pane_metrics_human::*;
use workhive_core::domain::activity::ReadModel;

// ─────────────────────────────────────────────────────────────────────────────
// JSON structs
// ─────────────────────────────────────────────────────────────────────────────

/// Serialisable freshness value for `pane list` and `pane show` JSON output.
///
/// Three-variant tagged union:
/// - `{ "kind": "known",       "age_ms": <u64> }` — fresh, age in ms.
/// - `{ "kind": "stale",       "age_ms": <u64> }` — past stale threshold, age in ms.
/// - `{ "kind": "unavailable", "reason": "<str>" }` — no data.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FreshnessJson {
    Known { age_ms: u64 },
    Stale { age_ms: u64 },
    Unavailable { reason: String },
}

/// Serialisable elapsed / duration value.
///
/// Three-variant tagged union matching the `MetricValue<Duration>` shape:
/// - `{ "kind": "known",       "value_ms": <u64> }` — complete value.
/// - `{ "kind": "partial",     "value_ms": <u64>, "reason": "<str>" }` — lower-bound.
/// - `{ "kind": "unavailable", "reason": "<str>" }` — no data.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DurationMetricJson {
    Known { value_ms: u64 },
    Partial { value_ms: u64, reason: String },
    Unavailable { reason: String },
}

/// Serialisable hint attached to each `pane list` entry.
///
/// Typed so machine consumers never have to parse free-form strings.
/// Human-readable guidance is rendered separately in the non-JSON path.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActivityHintJson {
    /// Agent is waiting for the user to approve or deny a tool permission.
    PermissionNeeded,
    /// The last agent turn completed successfully.
    Completed,
    /// The last agent turn failed.
    Failed,
    /// No actionable hint at this time.
    None,
}

/// Additional AI-metrics fields for a single `pane list` JSON entry.
///
/// All fields are additive (new) — existing fields are emitted by the
/// existing pane list code and remain unchanged. Add this struct's fields
/// alongside the pre-existing entry fields.
#[derive(Debug, Clone, Serialize)]
pub struct PaneListMetricsJson {
    /// AI provider slug, e.g. `"claude_code"`, `"codex"`, `"opencode"`.
    /// Null when no provider is known for this pane.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Most recently projected pane activity state as a snake_case string.
    pub state: String,

    /// Data source for the metrics. `"acp"` when derived from the
    /// append-only event log, `"legacy"` when from the latest-fact store,
    /// `"none"` when neither is available.
    pub source: String,

    /// Confidence tier: `"high"`, `"medium"`, `"low"`, or null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,

    /// Provider freshness (age of the most recent ACP event).
    pub freshness: FreshnessJson,

    /// How long the pane has been in its current state.
    pub elapsed_ms: DurationMetricJson,

    /// Typed next-action hint for machine consumers.
    pub hint: ActivityHintJson,
}

/// Additional AI-metrics fields for `pane show` JSON output.
///
/// Additive on top of `PaneListMetricsJson` with per-metric `MetricValue`
/// representations for turn, tool, permission_wait, idle, and freshness_age.
#[derive(Debug, Clone, Serialize)]
pub struct PaneShowMetricsJson {
    /// All list-level fields.
    #[serde(flatten)]
    pub list: PaneListMetricsJson,

    /// Duration of the current in-progress agent turn (lower bound when
    /// in-progress).
    pub current_turn: DurationMetricJson,

    /// Duration of the last completed agent turn.
    pub last_completed_turn: DurationMetricJson,

    /// Duration of the last completed tool invocation.
    pub last_tool: DurationMetricJson,

    /// Aggregate tool duration over all completed pairs in the event window.
    pub aggregate_tool: DurationMetricJson,

    /// How long the pane has been waiting for user permission.
    pub permission_wait: DurationMetricJson,

    /// How long the pane has been idle.
    pub idle: DurationMetricJson,

    /// Age of the most recent ACP event (same as `list.freshness.age_ms`
    /// but preserved as a plain `DurationMetricJson` for programmatic
    /// comparison without reparsing the tagged union).
    pub freshness_age: DurationMetricJson,
}

/// JSON entry for a single pane in the `pane list` machine-readable
/// output.
///
/// Combines stable pane identity fields with the typed metrics block.
/// `PaneListMetricsJson` is flattened via `#[serde(flatten)]` so the
/// resulting JSON object has both identity fields and metric fields at
/// the top level — additive on top of the historical pane identity
/// shape, with no field renames.
#[derive(Debug, Clone, Serialize)]
pub struct PaneListJsonEntry {
    /// Backend-specific target id (e.g. tmux `%2`). This is the pane
    /// key consumed by `workhive pane send` and remains stable across
    /// the table and JSON paths.
    pub target_id: String,
    /// Workspace-local index assigned by tmux (`0`, `1`, `2`, ...).
    pub index: u32,
    /// Whether this pane is the currently focused pane.
    pub active: bool,
    /// Pane kind slug (`terminal`, `agent`, `git_ui`, `unknown`).
    pub kind: String,
    /// Best-effort launch label carried forward from the identity
    /// store, when present and non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Flattened metric fields: `provider`, `state`, `source`,
    /// `confidence`, `freshness`, `elapsed_ms`, `hint`.
    #[serde(flatten)]
    pub metrics: PaneListMetricsJson,
}

// ─────────────────────────────────────────────────────────────────────────────
// Conversion helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Render `PartialReason` as a stable snake_case string for JSON.
pub fn partial_reason_str(r: PartialReason) -> &'static str {
    match r {
        PartialReason::InsufficientHistory => "insufficient_history",
        PartialReason::Stale => "stale",
        PartialReason::LowConfidence => "low_confidence",
        PartialReason::InProgress => "in_progress",
        PartialReason::Corrupt => "corrupt",
        PartialReason::Unknown => "unknown",
    }
}

/// Render `UnavailableReason` as a stable snake_case string for JSON.
pub fn unavailable_reason_str(r: UnavailableReason) -> &'static str {
    match r {
        UnavailableReason::NoHistory => "no_history",
        UnavailableReason::Disabled => "disabled",
        UnavailableReason::Unsupported => "unsupported",
        UnavailableReason::Degraded => "degraded",
        UnavailableReason::Unknown => "unknown",
    }
}

/// Convert a `MetricValue<Duration>` to `DurationMetricJson`.
pub fn duration_metric_json(mv: &MetricValue<Duration>) -> DurationMetricJson {
    match mv {
        MetricValue::Known(d) => DurationMetricJson::Known {
            value_ms: d.as_millis() as u64,
        },
        MetricValue::Partial { value, reason } => DurationMetricJson::Partial {
            value_ms: value.map(|d| d.as_millis() as u64).unwrap_or(0),
            reason: partial_reason_str(*reason).to_owned(),
        },
        MetricValue::Unavailable { reason } => DurationMetricJson::Unavailable {
            reason: unavailable_reason_str(*reason).to_owned(),
        },
    }
}

/// Convert `MetricValue<Duration>` for freshness to `FreshnessJson`.
///
/// The freshness metric uses `Partial { reason: Stale }` to signal
/// stale data, which maps to the `stale` variant; all other `Partial`
/// branches map to `unavailable` because the age cannot be trusted.
pub fn freshness_json(mv: &MetricValue<Duration>) -> FreshnessJson {
    match mv {
        MetricValue::Known(d) => FreshnessJson::Known {
            age_ms: d.as_millis() as u64,
        },
        MetricValue::Partial { value, reason } => {
            if *reason == PartialReason::Stale {
                FreshnessJson::Stale {
                    age_ms: value.map(|d| d.as_millis() as u64).unwrap_or(0),
                }
            } else {
                FreshnessJson::Unavailable {
                    reason: partial_reason_str(*reason).to_owned(),
                }
            }
        }
        MetricValue::Unavailable { reason } => FreshnessJson::Unavailable {
            reason: unavailable_reason_str(*reason).to_owned(),
        },
    }
}

/// Map `ProviderKind` to the stable JSON slug.
pub fn provider_slug(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::ClaudeCode => "claude_code",
        ProviderKind::Codex => "codex",
        ProviderKind::OpenCode => "opencode",
        ProviderKind::Future => "future",
    }
}

/// Map `PaneKind` to a stable snake_case JSON slug.
///
/// Mirrors the EN i18n labels but uses stable enum-derived strings so
/// machine consumers do not have to parse localized text.
pub fn pane_kind_slug(kind: workhive_core::domain::PaneKind) -> &'static str {
    use workhive_core::domain::PaneKind;
    match kind {
        PaneKind::Terminal => "terminal",
        PaneKind::Agent => "agent",
        PaneKind::GitUi => "git_ui",
        PaneKind::Unknown => "unknown",
    }
}

/// Map the ACP-level `ObservationConfidence` to a JSON confidence string.
pub fn confidence_str(c: workhive_core::domain::activity::ObservationConfidence) -> &'static str {
    use workhive_core::domain::activity::ObservationConfidence;
    match c {
        ObservationConfidence::High => "high",
        ObservationConfidence::Medium => "medium",
        ObservationConfidence::MediumLow | ObservationConfidence::Low => "low",
        ObservationConfidence::None => "none",
    }
}

/// Derive the `ActivityHintJson` from a `PaneActivityMetrics`.
///
/// Priority:
/// 1. `WaitingForPermission` state → `PermissionNeeded`.
/// 2. Completed state → `Completed`.
/// 3. Failed state → `Failed`.
/// 4. Otherwise → `None`.
pub fn derive_hint(metrics: &PaneActivityMetrics) -> ActivityHintJson {
    use workhive_core::domain::activity::PaneActivityState;
    match metrics.state {
        PaneActivityState::WaitingForPermission => ActivityHintJson::PermissionNeeded,
        PaneActivityState::Completed => ActivityHintJson::Completed,
        PaneActivityState::Failed => ActivityHintJson::Failed,
        _ => ActivityHintJson::None,
    }
}

/// Compute the `source` string from `PaneActivityMetrics::acp_source`.
pub fn source_str(acp_source: bool, has_any_data: bool) -> &'static str {
    if acp_source {
        "acp"
    } else if has_any_data {
        "legacy"
    } else {
        "none"
    }
}

/// Build the `PaneListMetricsJson` for a pane.
///
/// `has_legacy_data` should be `true` when the legacy overlay entry
/// provides at least a label/source, indicating the legacy path produced
/// something meaningful.
pub fn build_list_metrics_json(
    metrics: &PaneActivityMetrics,
    has_legacy_data: bool,
) -> PaneListMetricsJson {
    use workhive_core::domain::activity::PaneActivityState;

    let provider = metrics.provider.map(|p| provider_slug(p).to_owned());
    let state = pane_activity_state_str(metrics.state);
    let source = source_str(metrics.acp_source, has_legacy_data).to_owned();
    let confidence = if metrics.acp_source {
        Some(confidence_str(metrics.confidence).to_owned())
    } else {
        None
    };
    let freshness = freshness_json(&metrics.provider_freshness_age);
    let elapsed_ms = duration_metric_json(&metrics.elapsed);
    let hint = derive_hint(metrics);

    // Disabled/unsupported panes with no ACP data and no legacy data
    // get `source = "none"`.
    let source = if !metrics.acp_source
        && !has_legacy_data
        && matches!(
            metrics.state,
            PaneActivityState::Disabled
                | PaneActivityState::Unsupported
                | PaneActivityState::Unknown
        ) {
        "none".to_owned()
    } else {
        source
    };

    PaneListMetricsJson {
        provider,
        state,
        source,
        confidence,
        freshness,
        elapsed_ms,
        hint,
    }
}

/// Build the full `PaneShowMetricsJson` for a pane.
pub fn build_show_metrics_json(
    metrics: &PaneActivityMetrics,
    has_legacy_data: bool,
) -> PaneShowMetricsJson {
    let list = build_list_metrics_json(metrics, has_legacy_data);
    PaneShowMetricsJson {
        list,
        current_turn: duration_metric_json(&metrics.current_turn_duration),
        last_completed_turn: duration_metric_json(&metrics.last_completed_turn_duration),
        last_tool: duration_metric_json(&metrics.last_tool_duration),
        aggregate_tool: duration_metric_json(&metrics.aggregate_tool_duration),
        permission_wait: duration_metric_json(&metrics.permission_wait_duration),
        idle: duration_metric_json(&metrics.idle_duration),
        freshness_age: duration_metric_json(&metrics.provider_freshness_age),
    }
}

/// Render `PaneActivityState` as stable snake_case for JSON and display.
pub fn pane_activity_state_str(
    state: workhive_core::domain::activity::PaneActivityState,
) -> String {
    use workhive_core::domain::activity::PaneActivityState;
    let s = match state {
        PaneActivityState::Unknown => "unknown",
        PaneActivityState::OpenUnverified => "open_unverified",
        PaneActivityState::Working => "working",
        PaneActivityState::ToolRunning => "tool_running",
        PaneActivityState::WaitingForPermission => "waiting_for_permission",
        PaneActivityState::Completed => "completed",
        PaneActivityState::Failed => "failed",
        PaneActivityState::Idle => "idle",
        PaneActivityState::Stale => "stale",
        PaneActivityState::Disabled => "disabled",
        PaneActivityState::Unsupported => "unsupported",
        PaneActivityState::Degraded => "degraded",
    };
    s.to_owned()
}

// ─────────────────────────────────────────────────────────────────────────────
// Human-readable render helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Build a single `PaneListJsonEntry` from a row + its computed metrics.
///
/// `has_legacy_data` should be `true` when the legacy latest-fact
/// store produced any signal for this pane that the renderer is
/// already surfacing in the activity column (i.e. the table row is
/// not blank). It is `false` when neither ACP nor legacy provided
/// any data, in which case the resulting entry will report
/// `source = "none"`.
pub fn build_pane_list_json_entry(
    row: &PaneListRow,
    metrics: &PaneActivityMetrics,
    has_legacy_data: bool,
) -> PaneListJsonEntry {
    let metrics_json = build_list_metrics_json(metrics, has_legacy_data);
    let label = workhive_core::usecases::pane::pane_selection_label(row);
    PaneListJsonEntry {
        target_id: row.target_id.as_str().to_owned(),
        index: row.index,
        active: row.active,
        kind: pane_kind_slug(row.kind).to_owned(),
        label,
        metrics: metrics_json,
    }
}

/// Build the full vector of `PaneListJsonEntry` for a `pane list`
/// invocation. Returns one entry per row in input order.
///
/// `rows`, `metrics_per_row`, and `has_legacy_per_row` must have
/// equal lengths; mismatched inputs are treated as missing data on
/// the shorter side and the affected entry falls back to a
/// no-history `PaneActivityMetrics` so the schema never breaks.
pub fn build_pane_list_json_entries(
    rows: &[PaneListRow],
    metrics_per_row: &[PaneActivityMetrics],
    has_legacy_per_row: &[bool],
) -> Vec<PaneListJsonEntry> {
    rows.iter()
        .enumerate()
        .filter_map(|(idx, row)| {
            let metrics = metrics_per_row.get(idx)?;
            let has_legacy = has_legacy_per_row.get(idx).copied().unwrap_or(false);
            Some(build_pane_list_json_entry(row, metrics, has_legacy))
        })
        .collect()
}

/// Load `PaneActivityMetrics` for every row in a `pane list` invocation.
///
/// Returns two parallel vectors of length `rows.len()`:
///
/// * `metrics_per_row[i]` — the typed metrics computed from the ACP
///   event log for `rows[i]`. When the event store is unavailable or
///   the pane has no events, this is a `no-history` metrics value
///   whose `acp_source` field is `false` — exactly the same shape the
///   renderers expect on the legacy-fallback path.
/// * `has_legacy_per_row[i]` — best-effort signal of whether the
///   legacy latest-fact path produced any data the table is already
///   surfacing. Today we approximate this with "events exist but the
///   computed metrics report `acp_source == false`", matching the
///   `pane show` integration.
///
/// Time is injected via `now_ms` (`SystemClock` in production,
/// `FakeClock` or constants in tests) so callers can replay the same
/// instant across overlay + metrics computation and tests stay
/// deterministic. Event-store failures are logged at `debug` and
/// degrade to empty events for the affected row rather than
/// propagating — this matches the `pane show` fail-open pattern (see
/// the error-handling policy).
pub fn load_pane_list_metrics(
    event_store: &dyn ActivityEventStore,
    server: &str,
    workspace: &str,
    rows: &[PaneListRow],
    now_ms: i64,
) -> (Vec<PaneActivityMetrics>, Vec<bool>) {
    let thresholds = FreshnessThresholds::default_conservative();
    let read_model = ReadModel::empty(now_ms);

    let mut metrics_per_row = Vec::with_capacity(rows.len());
    let mut has_legacy_per_row = Vec::with_capacity(rows.len());
    for row in rows.iter() {
        let pane_key = PaneCorrelationKey::new(
            server.to_owned(),
            workspace.to_owned(),
            row.target_id.as_str(),
        );
        let events = match event_store.events_for_pane(&pane_key) {
            Ok(evts) => evts,
            Err(err) => {
                tracing::debug!(
                    unit_id = %row.target_id.as_str(),
                    error = %err,
                    "pane list: activity events unavailable for metrics row"
                );
                Vec::new()
            }
        };
        let metrics = read_model.pane_metrics(&pane_key, &events, now_ms, thresholds);
        let has_legacy = !events.is_empty() && !metrics.acp_source;
        metrics_per_row.push(metrics);
        has_legacy_per_row.push(has_legacy);
    }
    (metrics_per_row, has_legacy_per_row)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "pane_metrics_view_tests.rs"]
mod tests;

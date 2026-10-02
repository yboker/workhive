//! Human-locale renderers for pane activity metrics: duration/freshness
//! formatting, per-pane metric lines, hints, and the composed
//! `pane list` metrics block.

use std::time::Duration;

use workhive_core::domain::Locale;
use workhive_core::i18n_keys as k;
use workhive_core::ports::I18nPort;
use workhive_core::usecases::activity_control_plane::metrics::{
    MetricValue, PaneActivityMetrics, PartialReason,
};
use workhive_core::usecases::pane::PaneListRow;

#[allow(unused_imports)]
use super::pane_metrics_view::*;

/// Format a `Duration` for human display. Uses seconds for values >= 1s,
/// milliseconds for sub-second values.
pub fn format_duration_human(d: Duration) -> String {
    let secs = d.as_secs();
    if secs >= 3600 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs > 0 {
        format!("{}s", secs)
    } else {
        format!("{}ms", d.as_millis())
    }
}

/// Render the `freshness` field as a short human-readable string.
///
/// Used in both `pane list` (compact column) and `pane show` (detail line).
pub fn render_freshness_human(
    mv: &MetricValue<Duration>,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> String {
    match mv {
        MetricValue::Known(d) => format_duration_human(*d),
        MetricValue::Partial { value, reason } => {
            if *reason == PartialReason::Stale {
                if let Some(d) = value {
                    format!(
                        "{} ({})",
                        format_duration_human(*d),
                        i18n.resolve(locale, k::PANE_AI_ACTIVITY_STALE_HINT)
                    )
                } else {
                    i18n.resolve(locale, k::PANE_AI_ACTIVITY_STALE_HINT)
                }
            } else {
                i18n.resolve(locale, k::PANE_METRICS_PARTIAL)
            }
        }
        MetricValue::Unavailable { .. } => i18n.resolve(locale, k::PANE_METRICS_UNAVAILABLE),
    }
}

/// Render a `DurationMetricJson`-equivalent metric as a short human string.
///
/// `known` → formatted duration, `partial` → duration + suffix, `unavailable` → `—`.
pub fn render_duration_metric_human(
    mv: &MetricValue<Duration>,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> String {
    match mv {
        MetricValue::Known(d) => format_duration_human(*d),
        MetricValue::Partial { value, reason: _ } => {
            let prefix = value.map(format_duration_human).unwrap_or_default();
            if prefix.is_empty() {
                i18n.resolve(locale, k::PANE_METRICS_PARTIAL)
            } else {
                format!("{}+", prefix)
            }
        }
        MetricValue::Unavailable { .. } => i18n.resolve(locale, k::PANE_METRICS_UNAVAILABLE),
    }
}

/// Render the hint as a short human-readable line for `pane show`.
pub fn render_hint_human(
    metrics: &PaneActivityMetrics,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> Option<String> {
    use workhive_core::domain::activity::PaneActivityState;
    match metrics.state {
        PaneActivityState::WaitingForPermission => {
            Some(i18n.resolve(locale, k::PANE_METRICS_HINT_PERMISSION_NEEDED))
        }
        PaneActivityState::Completed => Some(i18n.resolve(locale, k::PANE_METRICS_HINT_COMPLETED)),
        PaneActivityState::Failed => Some(i18n.resolve(locale, k::PANE_METRICS_HINT_FAILED)),
        _ => None,
    }
}

/// Build a compact metrics summary line for `pane list`.
///
/// Format: `<provider> · <state> · <freshness>` (capped to avoid column
/// overflow on narrow terminals).
///
/// When `acp_source == false` and `has_legacy_data`, the line simply
/// shows `legacy` so the existing legacy-rendered label in the activity
/// column stays authoritative.
///
/// Wired into `print_pane_list` via [`compose_pane_list_metrics_block`].
/// Decide whether a metrics row carries no useful AI signal worth
/// surfacing under the default human "AI metrics" block.
///
/// A row is low-information when **all** of the following hold:
///
/// * the legacy-fallback path is empty (`has_legacy_data == false`),
///   so the existing legacy-rendered label cannot redeem the row;
/// * the row has no provider attribution (`metrics.provider.is_none()`);
/// * the read-model state is `Unknown`, meaning no provider event has
///   been ingested into the ACP read model for this pane.
///
/// Freshness/age timing alone is not a meaningful AI signal — it only
/// means the projector recorded *some* event at *some* time without
/// being able to classify the pane. Surfacing it as `— · unknown ·
/// Ns` next to a truthful `shell idle` or `open (waiting for event)`
/// table row is exactly the contradiction this view forbids. The
/// JSON path (`build_pane_list_json_entry`) keeps the typed
/// `provider`/`state` fields so machine consumers retain full signal.
fn metrics_row_is_low_information(metrics: &PaneActivityMetrics, has_legacy_data: bool) -> bool {
    use workhive_core::domain::activity::PaneActivityState as AcpPaneActivityState;
    !has_legacy_data
        && metrics.provider.is_none()
        && matches!(metrics.state, AcpPaneActivityState::Unknown)
}

pub fn render_list_metrics_line(
    metrics: &PaneActivityMetrics,
    has_legacy_data: bool,
    i18n: &dyn I18nPort,
    locale: Locale,
    max_width: usize,
) -> String {
    if !metrics.acp_source && has_legacy_data {
        return i18n.resolve(locale, k::PANE_METRICS_SOURCE_LEGACY);
    }
    if !metrics.acp_source && !has_legacy_data {
        return i18n.resolve(locale, k::PANE_METRICS_SOURCE_NONE);
    }

    let provider_part = metrics
        .provider
        .map(|p| provider_slug(p).replace('_', " "))
        .unwrap_or_else(|| "—".to_owned());
    let state_part = pane_activity_state_str(metrics.state).replace('_', " ");
    let freshness_part = render_freshness_human(&metrics.provider_freshness_age, i18n, locale);

    let full = format!("{provider_part} · {state_part} · {freshness_part}");
    if full.len() > max_width && max_width > 3 {
        format!("{}…", &full[..max_width.saturating_sub(1)])
    } else {
        full
    }
}

/// Build the detailed metrics block lines for `pane show`.
///
/// Returns a Vec of `"<label>: <value>"` strings. Empty when there are
/// no ACP metrics and no legacy data.
pub fn render_show_metrics_lines(
    metrics: &PaneActivityMetrics,
    has_legacy_data: bool,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // Provider
    if let Some(provider) = metrics.provider {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_PROVIDER),
            provider_slug(provider).replace('_', " ")
        ));
    }

    // State from ACP read model (distinct from the supervisor-derived state).
    out.push(format!(
        "{}: {}",
        i18n.resolve(locale, k::PANE_METRICS_LABEL_STATE),
        pane_activity_state_str(metrics.state).replace('_', " ")
    ));

    // Source (acp vs legacy vs none)
    let source = source_str(metrics.acp_source, has_legacy_data);
    out.push(format!(
        "{}: {}",
        i18n.resolve(locale, k::PANE_METRICS_LABEL_SOURCE),
        i18n.resolve(locale, source_i18n_key(source))
    ));

    // Confidence
    if metrics.acp_source {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_CONFIDENCE),
            confidence_str(metrics.confidence)
        ));
    }

    // Freshness
    out.push(format!(
        "{}: {}",
        i18n.resolve(locale, k::PANE_METRICS_LABEL_FRESHNESS),
        render_freshness_human(&metrics.provider_freshness_age, i18n, locale)
    ));

    // Elapsed
    out.push(format!(
        "{}: {}",
        i18n.resolve(locale, k::PANE_METRICS_LABEL_ELAPSED),
        render_duration_metric_human(&metrics.elapsed, i18n, locale)
    ));

    // Timing metrics (ACP only)
    if metrics.acp_source {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_CURRENT_TURN),
            render_duration_metric_human(&metrics.current_turn_duration, i18n, locale)
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_LAST_TURN),
            render_duration_metric_human(&metrics.last_completed_turn_duration, i18n, locale)
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_LAST_TOOL),
            render_duration_metric_human(&metrics.last_tool_duration, i18n, locale)
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_AGGREGATE_TOOL),
            render_duration_metric_human(&metrics.aggregate_tool_duration, i18n, locale)
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_PERMISSION_WAIT),
            render_duration_metric_human(&metrics.permission_wait_duration, i18n, locale)
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_IDLE),
            render_duration_metric_human(&metrics.idle_duration, i18n, locale)
        ));
    }

    // Hint (human-readable)
    if let Some(hint) = render_hint_human(metrics, i18n, locale) {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_METRICS_LABEL_HINT),
            hint
        ));
    }

    out
}

/// Map a source string to its i18n key.
fn source_i18n_key(source: &str) -> &'static str {
    match source {
        "acp" => k::PANE_METRICS_SOURCE_ACP,
        "legacy" => k::PANE_METRICS_SOURCE_LEGACY,
        _ => k::PANE_METRICS_SOURCE_NONE,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pane-list integration (human + JSON)
// ─────────────────────────────────────────────────────────────────────────────

/// Compose the compact "AI metrics" block emitted immediately after the
/// `pane list` table.
///
/// Layout (English example):
///
/// ```text
/// AI metrics:
///   %1: claude code · working · 5s
///   %2: codex · waiting for permission · 3s · waiting for permission — approve or deny in the pane
/// ```
///
/// Each summary line is `  <target_id>: <render_list_metrics_line>` with
/// the per-row line trimmed to `max_width` characters (excluding the
/// `<target_id>: ` prefix). Rows without ACP or legacy data are skipped
/// silently; the heading appears only when at least one row produced a
/// non-empty summary.
///
/// `rows`, `metrics_per_row`, and `has_legacy_per_row` must have equal
/// lengths; mismatched inputs are treated as missing data on the
/// shorter side rather than panicking (CLI render must never crash on
/// caller bookkeeping mistakes — the error-handling policy).
pub fn compose_pane_list_metrics_block(
    rows: &[PaneListRow],
    metrics_per_row: &[PaneActivityMetrics],
    has_legacy_per_row: &[bool],
    i18n: &dyn I18nPort,
    locale: Locale,
    max_width: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let none_source = i18n.resolve(locale, k::PANE_METRICS_SOURCE_NONE);

    for (idx, row) in rows.iter().enumerate() {
        let Some(metrics) = metrics_per_row.get(idx) else {
            continue;
        };
        let has_legacy = has_legacy_per_row.get(idx).copied().unwrap_or(false);

        // Suppress low-information default metrics
        // rows that contradict the main `pane list` activity table.
        //
        // When a metrics row has no provider AND no meaningful state,
        // its `render_list_metrics_line` output collapses to lines
        // like `— · unknown · 2s`. That phrase is misleading next to
        // a table row already showing `shell idle`, `open (activity
        // unverified)`, or another truthful activity state, and the
        // freshness-only timing carries no useful AI signal.
        //
        // The legacy fallback path (`acp_source == false` with
        // legacy data) remains, so any pane that genuinely belongs
        // under the AI metrics heading still appears with a
        // localized `legacy` label. JSON output is untouched.
        if metrics_row_is_low_information(metrics, has_legacy) {
            continue;
        }

        let line = render_list_metrics_line(metrics, has_legacy, i18n, locale, max_width);
        // Skip rows that produce only the bare `none` token — they
        // carry no signal worth surfacing and the absence of metrics
        // is already reflected in the table's existing source column.
        if line == none_source {
            continue;
        }
        out.push(format!("  {}: {}", row.target_id.as_str(), line));
    }

    if out.is_empty() {
        return out;
    }

    let mut block: Vec<String> = Vec::with_capacity(out.len() + 1);
    block.push(i18n.resolve(locale, k::PANE_METRICS_BLOCK_HEADING));
    block.extend(out);
    block
}

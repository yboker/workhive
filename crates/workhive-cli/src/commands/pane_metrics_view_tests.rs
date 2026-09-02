use super::*;
use std::time::Duration;
use workhive_core::ports::ClockPort as _;
use workhive_core::{
    domain::{
        Locale,
        activity::{
            ActivityOutcome, ObservationConfidence, PaneActivityFact, PaneActivityState,
            PaneCorrelationKey, ProviderKind, SignalChannel, SignalFreshness,
        },
    },
    usecases::activity_control_plane::metrics::{
        FreshnessThresholds, MetricValue, PaneActivityMetrics, PartialReason, UnavailableReason,
        compute_pane_metrics,
    },
};
use workhive_infra::i18n::StaticI18n;
use workhive_testkit::fakes::FakeClock;

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

fn make_fact(
    state: PaneActivityState,
    provider: ProviderKind,
    at_ms: i64,
    confidence: ObservationConfidence,
    freshness: SignalFreshness,
) -> PaneActivityFact {
    PaneActivityFact {
        pane: pane(),
        provider: Some(provider),
        state,
        outcome: ActivityOutcome::NotApplicable,
        confidence,
        channel: SignalChannel::LifecycleHook,
        observed_at_unix_ms: at_ms,
        freshness,
        source_event_id: None,
    }
}

fn acp_metrics_working() -> PaneActivityMetrics {
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ProviderSignalSource,
    };
    let start = NOW - 5_000;
    let events = vec![ActivityEvent::new(
        pane(),
        ProviderKind::ClaudeCode,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ActivityEventKind::PromptSubmitted,
        ActivityOutcome::NotApplicable,
        ObservationConfidence::High,
        start,
        None,
    )];
    let fact = make_fact(
        PaneActivityState::Working,
        ProviderKind::ClaudeCode,
        start,
        ObservationConfidence::High,
        SignalFreshness::Fresh,
    );
    compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds())
}

fn acp_metrics_permission_needed() -> PaneActivityMetrics {
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ProviderSignalSource,
    };
    let req_at = NOW - 3_000;
    let events = vec![ActivityEvent::new(
        pane(),
        ProviderKind::ClaudeCode,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ActivityEventKind::PermissionRequested,
        ActivityOutcome::NotApplicable,
        ObservationConfidence::High,
        req_at,
        None,
    )];
    let fact = make_fact(
        PaneActivityState::WaitingForPermission,
        ProviderKind::ClaudeCode,
        req_at,
        ObservationConfidence::High,
        SignalFreshness::Fresh,
    );
    compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds())
}

fn acp_metrics_completed() -> PaneActivityMetrics {
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ProviderSignalSource,
    };
    let start = NOW - 10_000;
    let end = NOW - 1_000;
    let events = vec![
        ActivityEvent::new(
            pane(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::PromptSubmitted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            start,
            None,
        ),
        ActivityEvent::new(
            pane(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::TurnCompleted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            end,
            None,
        ),
    ];
    let fact = make_fact(
        PaneActivityState::Completed,
        ProviderKind::ClaudeCode,
        end,
        ObservationConfidence::High,
        SignalFreshness::Fresh,
    );
    compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds())
}

fn acp_metrics_failed() -> PaneActivityMetrics {
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ProviderSignalSource,
    };
    let start = NOW - 8_000;
    let end = NOW - 500;
    let events = vec![
        ActivityEvent::new(
            pane(),
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::PromptSubmitted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            start,
            None,
        ),
        ActivityEvent::new(
            pane(),
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::TurnFailed,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            end,
            None,
        ),
    ];
    let fact = make_fact(
        PaneActivityState::Failed,
        ProviderKind::Codex,
        end,
        ObservationConfidence::High,
        SignalFreshness::Fresh,
    );
    compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds())
}

fn no_history_metrics() -> PaneActivityMetrics {
    compute_pane_metrics(&pane(), &[], None, NOW, thresholds())
}

fn stale_metrics() -> PaneActivityMetrics {
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ProviderSignalSource,
    };
    let old_at = NOW - 40_000; // beyond 30s stale threshold
    let events = vec![ActivityEvent::new(
        pane(),
        ProviderKind::ClaudeCode,
        SignalChannel::LifecycleHook,
        ProviderSignalSource::LifecycleHook,
        ActivityEventKind::SessionIdle,
        ActivityOutcome::NotApplicable,
        ObservationConfidence::Low,
        old_at,
        None,
    )];
    let fact = make_fact(
        PaneActivityState::Stale,
        ProviderKind::ClaudeCode,
        old_at,
        ObservationConfidence::Low,
        SignalFreshness::Stale,
    );
    compute_pane_metrics(&pane(), &events, Some(&fact), NOW, thresholds())
}

// ── JSON output tests ────────────────────────────────────────────────────

/// ACP-backed pane with Working state must produce provider, state,
/// source=acp, confidence=high, and freshness=known in the JSON output.
#[test]
fn pane_list_renders_provider_state_source_confidence_freshness_elapsed_for_acp_backed_pane() {
    let metrics = acp_metrics_working();
    let json = build_list_metrics_json(&metrics, false);

    assert_eq!(json.provider.as_deref(), Some("claude_code"));
    assert_eq!(json.state, "working");
    assert_eq!(json.source, "acp");
    assert_eq!(json.confidence.as_deref(), Some("high"));
    // Freshness: the most recent event was 5s ago — Known.
    assert!(matches!(json.freshness, FreshnessJson::Known { age_ms: _ }));
    // Elapsed: in-progress Working state → Partial/InProgress.
    assert!(matches!(
        json.elapsed_ms,
        DurationMetricJson::Partial { .. }
    ));
}

/// When acp_source=false but has_legacy_data=true, the source field
/// must be "legacy" and confidence must be None.
#[test]
fn pane_list_falls_back_to_legacy_when_acp_absent_and_marks_source_legacy() {
    let metrics = no_history_metrics();
    // has_legacy_data=true simulates the legacy overlay path having data.
    let json = build_list_metrics_json(&metrics, true);

    assert_eq!(json.source, "legacy");
    assert!(
        json.confidence.is_none(),
        "legacy path must not produce confidence tier"
    );
}

/// Disabled provider must produce source="none" when there is no legacy data.
#[test]
fn pane_list_marks_unavailable_when_disabled_via_feature_gate() {
    // Simulate a Disabled metrics (no events, no legacy data).
    let fact = make_fact(
        PaneActivityState::Disabled,
        ProviderKind::Codex,
        NOW - 1_000,
        ObservationConfidence::High,
        SignalFreshness::Fresh,
    );
    let metrics = compute_pane_metrics(&pane(), &[], Some(&fact), NOW, thresholds());
    let json = build_list_metrics_json(&metrics, false);

    assert_eq!(json.source, "none");
    assert_eq!(json.state, "disabled");
    assert!(
        matches!(json.elapsed_ms, DurationMetricJson::Unavailable { .. }),
        "disabled pane must have Unavailable elapsed"
    );
}

/// Low-confidence or stale events must produce Partial metrics.
#[test]
fn pane_list_marks_partial_for_low_confidence_or_stale_or_corrupt_events() {
    let metrics = stale_metrics();
    let json = build_list_metrics_json(&metrics, false);

    // Freshness should be Stale (40s > 30s threshold).
    assert!(
        matches!(json.freshness, FreshnessJson::Stale { .. }),
        "stale event must yield FreshnessJson::Stale, got {:?}",
        json.freshness
    );
    assert_eq!(json.source, "acp");
}

/// JSON output for pane list must serialize cleanly and contain no
/// free-form guidance text in the machine-readable fields.
#[test]
fn pane_list_json_output_is_machine_readable_and_free_of_unstructured_guidance() {
    let metrics = acp_metrics_working();
    let json = build_list_metrics_json(&metrics, false);
    let serialized = serde_json::to_string(&json).expect("must serialize");

    // Must be valid JSON.
    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("must be valid JSON");

    // `hint` must be a typed object, not a free-form string.
    let hint = parsed.get("hint").expect("hint field must be present");
    assert!(
        hint.is_object(),
        "hint must be a JSON object, not a string: {hint}"
    );
    let kind = hint.get("kind").expect("hint.kind must be present");
    assert!(kind.is_string(), "hint.kind must be a string, got {kind}");
    // Must be one of the known variants.
    let kind_str = kind.as_str().unwrap();
    assert!(
        ["permission_needed", "completed", "failed", "none"].contains(&kind_str),
        "hint.kind must be a known variant, got {kind_str}"
    );

    // No top-level guidance text (must not contain "workhive dashboard"
    // or "workhive integrations").
    assert!(
        !serialized.contains("workhive dashboard"),
        "JSON must not reference workhive dashboard"
    );
    assert!(
        !serialized.contains("workhive integrations"),
        "JSON must not reference workhive integrations"
    );
}

/// pane show JSON must include turn, tool, permission_wait, idle,
/// and freshness_age metrics.
#[test]
fn pane_show_renders_turn_tool_permission_idle_freshness_metrics() {
    let metrics = acp_metrics_completed();
    let json = build_show_metrics_json(&metrics, false);
    let serialized = serde_json::to_string(&json).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("must be valid JSON");

    for field in &[
        "current_turn",
        "last_completed_turn",
        "last_tool",
        "aggregate_tool",
        "permission_wait",
        "idle",
        "freshness_age",
    ] {
        assert!(
            parsed.get(field).is_some(),
            "pane show JSON must include `{field}` field"
        );
    }
}

/// When a PermissionRequested event is unresolved, the hint must be
/// permission_needed.
#[test]
fn pane_show_renders_permission_needed_hint_when_permission_pending() {
    let metrics = acp_metrics_permission_needed();
    let json = build_list_metrics_json(&metrics, false);

    assert!(
        matches!(json.hint, ActivityHintJson::PermissionNeeded),
        "WaitingForPermission state must produce PermissionNeeded hint, got {:?}",
        json.hint
    );
}

/// When a TurnCompleted event is present, the hint must be completed.
#[test]
fn pane_show_renders_completed_hint_when_completion_event_recent() {
    let metrics = acp_metrics_completed();
    let json = build_list_metrics_json(&metrics, false);

    assert!(
        matches!(json.hint, ActivityHintJson::Completed),
        "Completed state must produce Completed hint, got {:?}",
        json.hint
    );
}

/// When a TurnFailed event is present, the hint must be failed.
#[test]
fn pane_show_renders_failed_hint_when_failure_event_recent() {
    let metrics = acp_metrics_failed();
    let json = build_list_metrics_json(&metrics, false);

    assert!(
        matches!(json.hint, ActivityHintJson::Failed),
        "Failed state must produce Failed hint, got {:?}",
        json.hint
    );
}

/// pane show JSON must use typed hint object, not free-form strings.
#[test]
fn pane_show_json_output_uses_typed_hint_struct_not_free_form_strings() {
    let metrics = acp_metrics_permission_needed();
    let json = build_show_metrics_json(&metrics, false);
    let serialized = serde_json::to_string(&json).expect("must serialize");
    let parsed: serde_json::Value = serde_json::from_str(&serialized).expect("must be valid JSON");

    let hint = parsed.get("hint").expect("hint must be present");
    assert!(hint.is_object(), "hint must be an object");
    let kind = hint.get("kind").unwrap().as_str().unwrap();
    assert_eq!(kind, "permission_needed");
}

// ── Human render tests ───────────────────────────────────────────────────

/// FakeClock must produce deterministic now_ms for all render tests.
#[test]
fn fake_clock_produces_deterministic_now_ms() {
    let clock = FakeClock::new(NOW);
    assert_eq!(
        clock.now_unix_ms(),
        NOW,
        "FakeClock must return the pinned now_ms"
    );
}

/// Duration formatting must produce compact human strings.
#[test]
fn format_duration_human_produces_compact_strings() {
    assert_eq!(format_duration_human(Duration::from_millis(500)), "500ms");
    assert_eq!(format_duration_human(Duration::from_secs(5)), "5s");
    assert_eq!(format_duration_human(Duration::from_secs(125)), "2m 5s");
    assert_eq!(format_duration_human(Duration::from_secs(3665)), "1h 1m");
}

/// Stale freshness must render the stale marker.
#[test]
fn render_freshness_stale_includes_age_and_stale_marker() {
    let i18n = StaticI18n::new();
    let stale = MetricValue::Partial {
        value: Some(Duration::from_secs(40)),
        reason: PartialReason::Stale,
    };
    let rendered = render_freshness_human(&stale, &i18n, Locale::En);
    assert!(
        rendered.contains("40s"),
        "stale freshness must show age, got: {rendered}"
    );
    assert!(
        rendered.contains("stale") || rendered.contains("(stale)"),
        "stale freshness must include stale marker, got: {rendered}"
    );
}

/// Unavailable metrics must render as the em-dash marker.
#[test]
fn render_duration_metric_unavailable_shows_dash() {
    let i18n = StaticI18n::new();
    let unavail = MetricValue::Unavailable {
        reason: UnavailableReason::NoHistory,
    };
    let rendered = render_duration_metric_human(&unavail, &i18n, Locale::En);
    // The PANE_METRICS_UNAVAILABLE key resolves to "—".
    assert!(
        rendered.contains('—') || rendered.to_lowercase().contains("unavail"),
        "unavailable must render as — or 'unavailable', got: {rendered}"
    );
}

/// pane list metrics line must stay within max_width on narrow terminals.
#[test]
fn pane_list_remains_readable_on_narrow_terminal() {
    let metrics = acp_metrics_working();
    let i18n = StaticI18n::new();
    let max_width = 20;
    let line = render_list_metrics_line(&metrics, false, &i18n, Locale::En, max_width);
    assert!(
        line.chars().count() <= max_width,
        "metrics line must fit in {max_width} chars, got {} chars: `{line}`",
        line.chars().count()
    );
}

/// EN locale must resolve all metric label keys.
#[test]
fn pane_list_strings_resolve_through_i18n_for_en() {
    let i18n = StaticI18n::new();
    let metrics = acp_metrics_working();
    let lines = render_show_metrics_lines(&metrics, false, &i18n, Locale::En);
    // Must produce at least provider, state, source, confidence,
    // freshness, elapsed, current_turn, last_turn.
    assert!(
        lines.len() >= 8,
        "EN render must produce ≥8 lines, got {}: {lines:?}",
        lines.len()
    );
    for line in &lines {
        assert!(
            !line.contains("pane.metrics."),
            "EN render must not contain raw i18n keys, got: {line}"
        );
    }
}

/// zh-CN locale must resolve all metric label keys.
#[test]
fn pane_list_strings_resolve_through_i18n_for_zh_cn() {
    let i18n = StaticI18n::new();
    let metrics = acp_metrics_working();
    let lines = render_show_metrics_lines(&metrics, false, &i18n, Locale::ZhCn);
    assert!(
        lines.len() >= 8,
        "zh-CN render must produce ≥8 lines, got {}: {lines:?}",
        lines.len()
    );
    for line in &lines {
        assert!(
            !line.contains("pane.metrics."),
            "zh-CN render must not contain raw i18n keys, got: {line}"
        );
    }
}

/// EN and zh-CN must both produce non-empty metric lines for pane show.
#[test]
fn pane_show_strings_resolve_through_i18n_for_en_and_zh_cn() {
    let i18n = StaticI18n::new();
    let metrics = acp_metrics_permission_needed();

    let en_lines = render_show_metrics_lines(&metrics, false, &i18n, Locale::En);
    let zh_lines = render_show_metrics_lines(&metrics, false, &i18n, Locale::ZhCn);

    assert!(!en_lines.is_empty(), "EN must produce metric lines");
    assert!(!zh_lines.is_empty(), "zh-CN must produce metric lines");

    // At minimum the hint line must appear for WaitingForPermission.
    let en_has_hint = en_lines
        .iter()
        .any(|l| l.to_lowercase().contains("permission") || l.to_lowercase().contains("hint"));
    assert!(
        en_has_hint,
        "EN pane show must surface permission hint for WaitingForPermission state"
    );
}

/// JSON output must not reference workhive dashboard or integrations text.
#[test]
fn pane_list_does_not_introduce_workhive_dashboard_command_text() {
    let metrics = acp_metrics_working();
    let json = build_list_metrics_json(&metrics, false);
    let serialized = serde_json::to_string(&json).unwrap();
    assert!(
        !serialized.to_lowercase().contains("dashboard"),
        "JSON must not reference dashboard: {serialized}"
    );
}

/// JSON output must not mention workhive integrations.
#[test]
fn pane_list_does_not_introduce_workhive_integrations_user_text() {
    let metrics = acp_metrics_working();
    let json = build_list_metrics_json(&metrics, false);
    let serialized = serde_json::to_string(&json).unwrap();
    assert!(
        !serialized.to_lowercase().contains("integrations"),
        "JSON must not reference integrations: {serialized}"
    );
}

/// When no new metrics data is available (acp_source=false, no legacy),
/// the existing columns (state, source, hint) must still have valid defaults.
#[test]
fn pane_list_existing_columns_remain_unchanged_when_no_new_metrics_data() {
    let metrics = no_history_metrics();
    // No ACP, no legacy.
    let json = build_list_metrics_json(&metrics, false);

    // state must still be a string (not empty).
    assert!(!json.state.is_empty(), "state must be non-empty");
    // hint must be typed.
    let serialized = serde_json::to_string(&json.hint).unwrap();
    assert!(
        serialized.contains("kind"),
        "hint must always be a typed object"
    );
}

// ─────────────────────────────────────────────────────────────────────
// End-to-end `pane list` integration tests
//
// These exercise the same builders that `print_pane_list` uses so the
// human "AI metrics:" block and the per-pane JSON entries stay
// contract-stable. The tests are deliberately driven by raw
// `PaneListRow` + `PaneActivityMetrics` fixtures (no tmux, no fs)
// so they remain fast and runtime-independent per the testing policy.
// ─────────────────────────────────────────────────────────────────────

fn sample_row(target_id: &str, index: u32) -> PaneListRow {
    use workhive_core::domain::{
        PaneKind, PaneMessageSafety, PaneObservationConfidence, PaneObservationSource, PaneState,
        UnitId,
    };
    PaneListRow {
        active: false,
        index,
        target_id: UnitId::new(target_id),
        current_command: "claude".to_owned(),
        kind: PaneKind::Agent,
        state: PaneState::Idle,
        // `PaneListRow::activity` uses the pane-level
        // `domain::PaneActivityState` enum, which is shadowed in
        // this test module by the activity-level one imported at
        // the top. Reach the pane-level variant via the full
        // re-export path here.
        activity: workhive_core::domain::PaneActivityState::AgentForeground,
        message_safety: PaneMessageSafety::ShellInput,
        launch_identity_title: Some("claude".to_owned()),
        source: PaneObservationSource::TmuxCurrentCommand,
        confidence: PaneObservationConfidence::High,
        cwd: "/Users/test/project".to_owned(),
    }
}

/// Human output: `compose_pane_list_metrics_block` must emit a
/// heading line and one indented summary line per ACP-backed pane.
///
/// Acceptance: "Human output / default non-JSON path: append a
/// compact ACP metrics summary below or adjacent to each pane row,
/// indented one level with an i18n label."
#[test]
fn pane_list_human_output_includes_acp_metrics_line_for_acp_backed_pane() {
    let i18n = StaticI18n::default();
    let row = sample_row("%1", 0);
    let metrics = acp_metrics_working();

    let lines = compose_pane_list_metrics_block(
        std::slice::from_ref(&row),
        std::slice::from_ref(&metrics),
        &[false],
        &i18n,
        Locale::En,
        80,
    );

    assert!(
        lines.len() >= 2,
        "must produce heading + at least one summary line, got {lines:?}"
    );
    let heading = &lines[0];
    assert!(
        heading.to_lowercase().contains("ai") && heading.to_lowercase().contains("metric"),
        "heading must be the AI metrics block heading: {heading:?}"
    );
    let summary = lines
        .iter()
        .find(|l| l.contains("%1"))
        .expect("must include the target_id %1 in a summary line");
    assert!(
        summary.starts_with("  "),
        "summary line must be indented: {summary:?}"
    );
    // Provider + state tokens must appear (typed source: ACP).
    let lowered = summary.to_lowercase();
    assert!(
        lowered.contains("claude") && lowered.contains("working"),
        "summary must surface provider + state tokens: {summary:?}"
    );
}

// ───────────────────────────────────────────────────────────────
// 2026-05-16 — AI metrics consistency with the main pane table.
//
// Rows whose only AI metrics signal is freshness/age over an
// unknown provider must NOT print as `— · unknown · 2s` because
// they contradict the truthful activity column of the main pane
// table. The JSON path keeps the typed fields so machine
// consumers retain full signal.
// ───────────────────────────────────────────────────────────────

/// Build a metrics value with `acp_source = true` but no provider
/// attribution and an Unknown read-model state. This is the exact
/// "low information" shape the consistency policy requires to
/// suppress in the human-readable list.
fn low_information_acp_metrics() -> PaneActivityMetrics {
    PaneActivityMetrics {
        pane: pane(),
        provider: None,
        state: PaneActivityState::Unknown,
        channel: SignalChannel::LifecycleHook,
        confidence: ObservationConfidence::None,
        freshness: SignalFreshness::Fresh,
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
        // Known freshness age — exactly the case the screenshot
        // demonstrated (`— · unknown · 2s`).
        provider_freshness_age: MetricValue::Known(Duration::from_secs(2)),
        acp_source: true,
    }
}

/// Suppress the row when provider/state carry no useful signal
/// even if a freshness/age timing exists. The output must skip
/// the entire `<id>: — · unknown · 2s` line, and because that is
/// the only row, the heading itself must not render.
#[test]
fn pane_list_default_metrics_suppresses_unknown_provider_unknown_state_freshness_only_row() {
    let i18n = StaticI18n::default();
    let row = sample_row("%1", 0);
    let metrics = low_information_acp_metrics();

    for locale in [Locale::En, Locale::ZhCn] {
        let lines = compose_pane_list_metrics_block(
            std::slice::from_ref(&row),
            std::slice::from_ref(&metrics),
            &[false],
            &i18n,
            locale,
            80,
        );

        assert!(
            lines.is_empty(),
            "low-info provider=None+state=Unknown metrics row must be suppressed (no heading, no row) ({locale:?}): {lines:?}"
        );
    }
}

/// Useful ACP-backed metrics must continue to render — this is
/// the regression guard for the suppression logic. We re-check
/// that the Working/Claude metrics row still produces a heading
/// + summary even after the suppression filter is in place.
#[test]
fn pane_list_default_metrics_still_renders_meaningful_acp_metrics_after_suppression() {
    let i18n = StaticI18n::default();
    let row = sample_row("%1", 0);
    let metrics = acp_metrics_working();

    let lines = compose_pane_list_metrics_block(
        std::slice::from_ref(&row),
        std::slice::from_ref(&metrics),
        &[false],
        &i18n,
        Locale::En,
        80,
    );

    assert!(
        !lines.is_empty(),
        "meaningful ACP metrics must still render after suppression: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("%1") && l.contains("claude")),
        "meaningful Claude metrics row must still appear: {lines:?}"
    );
}

/// Suppression must not strip rows that ride the legacy fallback
/// path. A pane with `acp_source = false` but `has_legacy_data =
/// true` is precisely the case the main table already labels as
/// legacy; the metrics block surfaces the localized `legacy` token
/// so users see a non-contradictory signal.
#[test]
fn pane_list_default_metrics_does_not_suppress_legacy_backed_row() {
    let i18n = StaticI18n::default();
    let row = sample_row("%1", 0);
    let metrics = no_history_metrics();

    let lines = compose_pane_list_metrics_block(
        std::slice::from_ref(&row),
        std::slice::from_ref(&metrics),
        &[true], // has_legacy_data = true
        &i18n,
        Locale::En,
        80,
    );

    assert!(
        !lines.is_empty(),
        "legacy-backed row must still render: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.contains("%1")),
        "legacy-backed row must include the target_id: {lines:?}"
    );
}

/// JSON output for the same low-information metrics must remain
/// stable: identity fields stay, typed `provider`/`state` fields
/// stay; the suppression rule is human-render-only.
#[test]
fn pane_list_json_entry_preserves_typed_state_even_for_low_information_metrics() {
    let row = sample_row("%9", 8);
    let metrics = low_information_acp_metrics();

    let entry = build_pane_list_json_entry(&row, &metrics, false);
    let serialized = serde_json::to_string(&entry).expect("entry must serialize");
    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("entry must be valid json");

    assert_eq!(
        parsed.get("target_id").and_then(|v| v.as_str()),
        Some("%9"),
        "target_id must survive low-info suppression in JSON: {serialized}"
    );
    // Even when human render suppresses, the typed `state` stays.
    assert_eq!(
        parsed.get("state").and_then(|v| v.as_str()),
        Some("unknown"),
        "typed state must remain in JSON: {serialized}"
    );
}

/// JSON entry: identity fields + flattened typed metric fields
/// for an ACP-backed pane.
///
/// Acceptance: "JSON output: flatten `PaneListMetricsJson` into
/// each pane entry via `#[serde(flatten)]`, additively, preserving
/// any existing identity fields."
#[test]
fn pane_list_json_entry_includes_typed_metrics_fields_for_acp_backed_pane() {
    let row = sample_row("%2", 1);
    let metrics = acp_metrics_permission_needed();

    let entry = build_pane_list_json_entry(&row, &metrics, false);
    let serialized = serde_json::to_string(&entry).expect("entry must serialize");
    let parsed: serde_json::Value =
        serde_json::from_str(&serialized).expect("entry must be valid json");

    // Identity fields stay at the top level.
    assert_eq!(
        parsed.get("target_id").and_then(|v| v.as_str()),
        Some("%2"),
        "target_id must be present: {serialized}"
    );
    assert_eq!(
        parsed.get("index").and_then(|v| v.as_u64()),
        Some(1),
        "index must be present: {serialized}"
    );
    assert_eq!(
        parsed.get("kind").and_then(|v| v.as_str()),
        Some("agent"),
        "kind slug must be present: {serialized}"
    );

    // Flattened metric fields appear at the SAME top level.
    assert_eq!(
        parsed.get("provider").and_then(|v| v.as_str()),
        Some("claude_code"),
        "provider must be flattened to the top level: {serialized}"
    );
    assert_eq!(
        parsed.get("source").and_then(|v| v.as_str()),
        Some("acp"),
        "source must be flattened and typed: {serialized}"
    );
    assert_eq!(
        parsed.get("confidence").and_then(|v| v.as_str()),
        Some("high"),
        "confidence must be flattened and typed: {serialized}"
    );
    assert!(
        parsed.get("freshness").is_some(),
        "freshness must be flattened: {serialized}"
    );
    assert!(
        parsed.get("elapsed_ms").is_some(),
        "elapsed_ms must be flattened: {serialized}"
    );
    let hint = parsed
        .get("hint")
        .expect("hint must be flattened typed object");
    assert!(
        hint.get("kind").is_some(),
        "hint must always carry a `kind` discriminator: {serialized}"
    );
}

/// JSON entry legacy-fallback contract: when ACP data is absent
/// but the legacy latest-fact path produced something for the row,
/// the JSON entry must report `source = "legacy"` rather than
/// silently inheriting an ACP source.
///
/// Acceptance: "Legacy fallback must remain visible end-to-end:
/// rendered when ACP data is absent."
#[test]
fn pane_list_json_entry_marks_source_legacy_when_acp_absent() {
    let row = sample_row("%3", 2);
    let metrics = no_history_metrics();
    assert!(!metrics.acp_source, "fixture must have acp_source=false");

    let entry = build_pane_list_json_entry(&row, &metrics, true);
    let serialized = serde_json::to_string(&entry).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

    assert_eq!(
        parsed.get("source").and_then(|v| v.as_str()),
        Some("legacy"),
        "source must be 'legacy' when ACP is absent but legacy data exists: {serialized}"
    );
    // Confidence is omitted on the legacy path (skip_serializing_if).
    assert!(
        parsed.get("confidence").is_none()
            || parsed
                .get("confidence")
                .map(|v| v.is_null())
                .unwrap_or(false),
        "confidence must not be reported on the legacy path: {serialized}"
    );

    // Identity fields remain present.
    assert_eq!(parsed.get("target_id").and_then(|v| v.as_str()), Some("%3"));
    assert_eq!(parsed.get("index").and_then(|v| v.as_u64()), Some(2));
}

/// Bonus coverage: `build_pane_list_json_entries` aligns vectors
/// by index and falls back to legacy=false when the parallel flag
/// vector is shorter than `rows`.
#[test]
fn pane_list_json_entries_aligns_rows_with_metrics_by_index() {
    let rows = vec![sample_row("%1", 0), sample_row("%2", 1)];
    let metrics = vec![acp_metrics_working(), no_history_metrics()];
    let entries = build_pane_list_json_entries(&rows, &metrics, &[false]);
    assert_eq!(entries.len(), 2, "one entry per row");
    assert_eq!(entries[0].target_id, "%1");
    assert_eq!(entries[1].target_id, "%2");
    // Row 0 uses ACP metrics → source = acp.
    assert_eq!(entries[0].metrics.source, "acp");
    // Row 1 has shorter has_legacy slice → default to legacy=false →
    // source = none (no ACP, no legacy claim).
    assert_eq!(entries[1].metrics.source, "none");
}

/// `pane_kind_slug` must map every PaneKind variant deterministically.
#[test]
fn pane_kind_slug_round_trips_every_variant() {
    use workhive_core::domain::PaneKind;
    assert_eq!(pane_kind_slug(PaneKind::Terminal), "terminal");
    assert_eq!(pane_kind_slug(PaneKind::Agent), "agent");
    assert_eq!(pane_kind_slug(PaneKind::GitUi), "git_ui");
    assert_eq!(pane_kind_slug(PaneKind::Unknown), "unknown");
}

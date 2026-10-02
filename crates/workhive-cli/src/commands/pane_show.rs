//! `workhive pane show` rendering.
//!
//! Extracts the `print_pane_show` renderer and the AI activity
//! overlay line builder (`render_ai_overlay_lines`) from `pane.rs`
//! without changing any user-visible output. Imports the AI
//! activity overlay primitives and the shared `display_pane_cwd`
//! helper from their respective sibling modules so the show
//! renderer stays a thin display layer.
//!
//! The focused tests in this file cover the show renderer and overlay
//! line builder without duplicating dispatcher fixtures.

use camino::Utf8Path;
use workhive_core::domain::activity::PaneCorrelationKey;
use workhive_core::domain::{Locale, ToolCatalog};
use workhive_core::i18n_keys as k;
use workhive_core::ports::I18nPort;
use workhive_core::usecases::pane::PaneListRow;

use crate::commands::pane_activity_overlay::{
    AiActivityOverlayEntry, now_unix_ms_from, read_pane_ai_activity_overlay,
    read_terminal_status_observations, resolve_ai_integration_gate, resolve_pane_observation_gate,
};
use crate::commands::pane_list::display_pane_cwd;
use crate::commands::pane_metrics_view;
use crate::context::CommandContext;

/// Render `workhive pane show` using the typed view produced by the
/// core use case. Reuses the same i18n catalog as `pane list` and
/// surfaces launch identity as a strictly auxiliary label, never
/// substituting it for the live `command` / `kind` / `state`.
pub(crate) fn print_pane_show(
    ctx: &CommandContext,
    locale: Locale,
    row: &PaneListRow,
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
    catalog: &ToolCatalog,
) {
    let i18n = &ctx.i18n;

    println!("{}", i18n.resolve(locale, k::PANE_SHOW_TITLE));
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_ID),
        row.target_id.as_str()
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_INDEX),
        row.index
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_COMMAND),
        row.current_command
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_KIND),
        i18n.resolve(locale, row.kind.i18n_key())
    );
    // Diagnostic raw `state` retained for visibility (deeply technical),
    // but the user-facing activity / message-safety fields are the ones
    // callers should consume. The raw state can be ambiguous (e.g.
    // `Running` for any non-shell foreground process), so it must not
    // be the sole signal a user reads.
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_STATE),
        i18n.resolve(locale, row.state.i18n_key())
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_ACTIVITY),
        i18n.resolve(locale, row.activity.i18n_key())
    );

    // Layer the per-pane AI activity fact, if any. The supervisor-
    // derived `activity` field above stays exactly as it was so the
    // diagnostic baseline is never lost; the AI lines below add the
    // hook/plugin-sourced view used for higher-confidence AI status.
    // The AI integrations feature gate is resolved once and applied
    // by `compute_pane_ai_overlay_entry` so gated-off providers
    // surface no AI overlay block at all. When gate resolution is
    // `Unavailable`, the display gate fails closed and the overlay
    // suppresses every AI line without affecting the supervisor row.
    let resolution = resolve_ai_integration_gate(ctx, project_root);
    let gate = resolution.effective_display_gate();
    let overlay_rows = std::slice::from_ref(row);
    let pane_gate = resolve_pane_observation_gate(ctx, project_root);
    let terminal_status_probe = ctx.composition.terminal_status_probe();
    let terminal_status = read_terminal_status_observations(
        overlay_rows,
        server,
        catalog,
        &gate,
        &pane_gate,
        &terminal_status_probe,
    );
    let clock = workhive_infra::clock::SystemClock;
    let now_ms = now_unix_ms_from(&clock);
    let overlay = read_pane_ai_activity_overlay(
        &ctx.composition,
        project_root,
        server,
        workspace,
        overlay_rows,
        locale,
        &ctx.i18n,
        catalog,
        &gate,
        &terminal_status,
        now_ms,
    );
    for line in render_ai_overlay_lines(overlay.into_iter().next(), locale, &ctx.i18n) {
        println!("  {line}");
    }

    // Activity Intelligence v1: render typed ACP metrics block when available.
    // Falls back gracefully when ACP data is absent (acp_source == false).
    // Metrics are fetched via the same `now_ms` used for the overlay so time is
    // consistent within a single `pane show` invocation.
    {
        use workhive_core::domain::activity::ReadModel;
        use workhive_core::ports::ActivityEventStore;
        use workhive_core::usecases::activity_control_plane::metrics::FreshnessThresholds;

        let event_store = ctx.composition.activity_event_store(project_root);
        let pane_key = PaneCorrelationKey::new(
            server.to_owned(),
            workspace.to_owned(),
            row.target_id.as_str(),
        );
        let thresholds = FreshnessThresholds::default_conservative();
        let events = match event_store.events_for_pane(&pane_key) {
            Ok(evts) => evts,
            Err(err) => {
                tracing::debug!(
                    unit_id = %row.target_id.as_str(),
                    error = %err,
                    "pane show: activity events unavailable for metrics render"
                );
                Vec::new()
            }
        };
        let read_model = ReadModel::empty(now_ms);
        let metrics = read_model.pane_metrics(&pane_key, &events, now_ms, thresholds);
        let has_legacy_data = !events.is_empty() && !metrics.acp_source;
        let metrics_lines = pane_metrics_view::render_show_metrics_lines(
            &metrics,
            has_legacy_data,
            &ctx.i18n as &dyn I18nPort,
            locale,
        );
        if !metrics_lines.is_empty() {
            println!();
            for line in metrics_lines {
                println!("  {line}");
            }
        }

        // Pane-show JSON contract: build the same typed payload the
        // JSON output path consumes and emit it as a structured
        // `debug` log event so the builder stays on the production
        // code path (no dead_code) without changing user-visible CLI
        // behavior. Schema-breaking checks live in
        // `pane_metrics_view::tests`.
        let json_payload = pane_metrics_view::build_show_metrics_json(&metrics, has_legacy_data);
        tracing::debug!(
            target: "workhive::pane::show",
            target_id = %row.target_id.as_str(),
            payload = ?json_payload,
            "computed pane show json payload"
        );
    }

    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_MESSAGE_SAFETY),
        i18n.resolve(locale, row.message_safety.i18n_key())
    );
    let identity_display = row
        .launch_identity_title
        .clone()
        .unwrap_or_else(|| i18n.resolve(locale, k::PANE_LAUNCH_IDENTITY_NONE));
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_LAUNCH_IDENTITY),
        identity_display
    );
    // Surface the layered observation provenance so users (and code
    // reviewers) can tell whether a row came from the Workhive
    // supervisor, the OS process observer, or the tmux fallback.
    // Both labels are localized: Chinese output never exposes raw
    // enum names like `Supervisor` or `ProcessObserver`.
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_OBSERVATION_SOURCE),
        i18n.resolve(locale, row.source.i18n_key())
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_OBSERVATION_CONFIDENCE),
        i18n.resolve(locale, row.confidence.i18n_key())
    );
    println!(
        "  {}: {}",
        i18n.resolve(locale, k::PANE_LABEL_CWD),
        display_pane_cwd(&row.cwd)
    );
}

/// Render the `pane show` AI activity overlay as a Vec of fully
/// localized `"<label>: <value>"` lines. Pure (no I/O, no globals) so
/// tests can assert each label key is distinct and never reuses the
/// supervisor's `pane.label.activity` for AI fields.
pub(crate) fn render_ai_overlay_lines(
    entry: Option<AiActivityOverlayEntry>,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(entry) = entry else { return out };
    // The overlay must stay silent on non-AI rows. Both the legacy
    // "label only" entries and the new source-driven entries set at
    // least one of the two fields when there is anything to say.
    if entry.label.is_none() && entry.source.is_none() {
        return out;
    }
    if let Some(label) = entry.label.as_ref() {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_LABEL),
            label
        ));
    }
    if let Some(provider) = entry.provider {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_PROVIDER_LABEL),
            i18n.resolve(locale, provider.i18n_key())
        ));
    }
    if let Some(confidence) = entry.confidence {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_CONFIDENCE_LABEL),
            i18n.resolve(locale, confidence.i18n_key())
        ));
    }
    let stale_value = if entry.stale {
        i18n.resolve(locale, k::COMMON_VALUE_YES)
    } else {
        i18n.resolve(locale, k::COMMON_VALUE_NO)
    };
    out.push(format!(
        "{}: {}",
        i18n.resolve(locale, k::PANE_AI_ACTIVITY_STALE_LABEL),
        stale_value
    ));
    if let Some(reason) = entry.reason {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_REASON_LABEL),
            reason
        ));
    }
    // Source + accuracy + guidance labels never reuse the supervisor
    // row's `pane.label.activity` or `pane.label.observation.*` keys;
    // the user can tell the displayed AI source apart from the
    // observation source at a glance.
    if let Some(source) = entry.source {
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_SOURCE_LABEL),
            i18n.resolve(locale, source.i18n_key())
        ));
        out.push(format!(
            "{}: {}",
            i18n.resolve(locale, k::PANE_AI_ACTIVITY_ACCURACY_LABEL),
            i18n.resolve(locale, source.accuracy().i18n_key())
        ));
        if let Some(guidance_key) = source.guidance(entry.stale) {
            // The catalog stores guidance as a multi-line block so
            // translators control the line breaks; we expand each
            // line so the caller can prefix every one with the
            // `pane show` indent.
            for line in i18n.resolve(locale, guidance_key).split('\n') {
                out.push(line.to_owned());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render_ai_overlay_lines;
    use crate::commands::pane_activity_overlay::AiActivityOverlayEntry;
    use workhive_core::domain::{
        AiActivityConfidence, AiActivityProvider, AiActivitySource, Locale,
    };
    use workhive_infra::i18n::StaticI18n;

    /// `pane show` must label AI activity, provider, confidence,
    /// stale, and reason with their own distinct i18n keys. When the
    /// entry carries a typed `source`, it also renders `AI source`,
    /// `AI accuracy`, and the multi-line guidance block. Verifies
    /// both EN and ZH output never reuses the supervisor's
    /// `pane.label.activity` label for any AI line.
    #[test]
    fn pane_show_displays_distinct_ai_activity_labels() {
        let i18n = StaticI18n::new();
        let overlay = AiActivityOverlayEntry {
            label: Some("processing".to_owned()),
            reason: Some("running edit_file".to_owned()),
            provider: Some(AiActivityProvider::ClaudeCode),
            confidence: Some(AiActivityConfidence::Hook),
            stale: true,
            source: Some(AiActivitySource::ClaudeCodeHook),
            state: Some(workhive_core::domain::AiActivityState::Processing),
        };
        let lines_en = render_ai_overlay_lines(Some(overlay.clone()), Locale::En, &i18n);
        let lines_zh = render_ai_overlay_lines(Some(overlay), Locale::ZhCn, &i18n);
        // Existing five lines: activity, provider, confidence, stale,
        // reason. The new productized lines (source, accuracy, plus
        // a multi-line stale guidance block) follow them.
        assert!(
            lines_en.len() >= 7,
            "expected at least 7 AI overlay lines, got {lines_en:?}"
        );
        assert!(lines_en[0].starts_with("AI activity:"));
        assert!(lines_en[1].starts_with("AI provider:"));
        assert!(lines_en[2].starts_with("AI confidence:"));
        assert!(lines_en[3].starts_with("AI stale:"));
        assert!(lines_en[4].starts_with("reason:"));
        assert_eq!(lines_en[5], "AI source: Claude Code hook");
        assert_eq!(lines_en[6], "AI accuracy: high");
        // Stale Claude Code hook → guidance.stale block follows the
        // accuracy line. The block intentionally never names the
        // rescinded `workhive integrations` command.
        assert!(
            lines_en[7..]
                .iter()
                .any(|l| l.contains("`workhive doctor`")),
            "expected doctor guidance in EN overlay, got {lines_en:?}"
        );
        for line in lines_en.iter() {
            assert!(
                !line.contains("workhive integrations"),
                "EN overlay leaks rescinded `workhive integrations` text: {line:?}"
            );
        }
        // Stale rendered as a localized yes/no, not glued onto the
        // activity label.
        assert!(
            lines_en.iter().any(|l| l == "AI stale: yes"),
            "expected AI stale: yes line, got {lines_en:?}"
        );
        // ZH parity. Each AI line uses an AI-prefixed Chinese label;
        // none of them reuse the supervisor's `活动` label alone.
        assert!(lines_zh.len() >= 7);
        assert!(lines_zh[0].starts_with("AI 活动:"));
        assert!(lines_zh[1].starts_with("AI 提供方:"));
        assert!(lines_zh[2].starts_with("AI 置信度:"));
        assert!(lines_zh[3].starts_with("AI 已过期:"));
        assert!(lines_zh[4].starts_with("原因:"));
        assert_eq!(lines_zh[5], "AI 来源: Claude Code hook");
        assert_eq!(lines_zh[6], "AI 准确性: 高");
        assert!(
            lines_zh[7..]
                .iter()
                .any(|l| l.contains("`workhive doctor`")),
            "expected doctor guidance in ZH overlay, got {lines_zh:?}"
        );
        for line in lines_zh.iter() {
            assert!(
                !line.contains("workhive integrations"),
                "ZH overlay leaks rescinded `workhive integrations` text: {line:?}"
            );
        }
        // No line uses the supervisor's bare label `Activity` / `活动`
        // (with no AI prefix), which would mean a label collision with
        // the supervisor-observed activity row.
        for line in lines_en.iter().chain(lines_zh.iter()) {
            assert!(
                !line.starts_with("Activity:") && !line.starts_with("活动:"),
                "AI overlay line `{line}` reuses supervisor label",
            );
        }
    }

    /// When there is no fact for the pane and no source is projected
    /// (i.e. the row is a shell or non-AI process), the overlay
    /// renders nothing. The supervisor row alone speaks for the pane,
    /// matching the conservative fail-open policy.
    #[test]
    fn pane_show_renders_no_ai_lines_without_fact_or_source() {
        let i18n = StaticI18n::new();
        assert!(render_ai_overlay_lines(None, Locale::En, &i18n).is_empty());
        let empty_entry = AiActivityOverlayEntry::default();
        assert!(render_ai_overlay_lines(Some(empty_entry), Locale::En, &i18n).is_empty());
    }

    /// When the pane has an AI tool in the foreground but no hook
    /// fact has landed yet, the overlay must honestly render
    /// `Workhive supervisor` as the source with `medium` accuracy and
    /// an "unverified" guidance block. The guidance never names
    /// `workhive integrations` and never claims hook-confirmed
    /// activity.
    #[test]
    fn pane_show_renders_unverified_overlay_for_ai_tool_without_hook_fact() {
        let i18n = StaticI18n::new();
        // Source = WorkhiveSupervisor models the "AI tool open,
        // hook event missing" case. The recorder confidence stays
        // `Fallback` for this row since no provider event drove the
        // state.
        let entry = AiActivityOverlayEntry {
            // Reflects the user-facing wording owned by the
            // `PANE_AI_ACTIVITY_OPEN_UNVERIFIED` i18n key.
            label: Some("open (waiting for event)".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::ClaudeCode),
            confidence: Some(AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::WorkhiveSupervisor),
            state: Some(workhive_core::domain::AiActivityState::OpenUnverified),
        };
        let lines_en = render_ai_overlay_lines(Some(entry.clone()), Locale::En, &i18n);
        let lines_zh = render_ai_overlay_lines(Some(entry), Locale::ZhCn, &i18n);
        assert!(
            lines_en
                .iter()
                .any(|l| l == "AI source: Workhive observation"),
            "EN overlay must label the source as Workhive observation: {lines_en:?}"
        );
        assert!(
            lines_en.iter().any(|l| l == "AI accuracy: medium"),
            "EN overlay must label the accuracy as medium: {lines_en:?}"
        );
        assert!(
            lines_en
                .iter()
                .any(|l| l.contains("Launch the AI tool through Workhive")),
            "EN guidance must point at launching through Workhive so the user knows what to fix: {lines_en:?}"
        );
        assert!(
            lines_en.iter().any(|l| l.contains("`workhive doctor`")),
            "EN guidance must point at workhive doctor: {lines_en:?}"
        );
        for line in lines_en.iter().chain(lines_zh.iter()) {
            assert!(
                !line.contains("workhive integrations"),
                "Overlay leaks rescinded `workhive integrations` text: {line:?}"
            );
            assert!(
                !line.contains("supervisor handshake") && !line.contains("监管握手"),
                "Overlay leaks internal supervisor-handshake vocabulary to the user: {line:?}"
            );
        }
        assert!(
            lines_zh.iter().any(|l| l == "AI 来源: Workhive 监控进程"),
            "ZH overlay must label the source as Workhive 监控进程: {lines_zh:?}"
        );
        assert!(
            lines_zh.iter().any(|l| l == "AI 准确性: 中"),
            "ZH overlay must label the accuracy as 中: {lines_zh:?}"
        );
    }

    /// tmux fallback rows render the lowest accuracy tier and the
    /// `guidance.fallback` block.
    #[test]
    fn pane_show_renders_tmux_fallback_overlay() {
        let i18n = StaticI18n::new();
        let entry = AiActivityOverlayEntry {
            // Match the updated user-facing label owned by the
            // `PANE_AI_ACTIVITY_OPEN_UNVERIFIED` i18n key.
            label: Some("open (waiting for event)".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::Codex),
            confidence: Some(AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::TmuxFallback),
            state: Some(workhive_core::domain::AiActivityState::OpenUnverified),
        };
        let lines_en = render_ai_overlay_lines(Some(entry.clone()), Locale::En, &i18n);
        let lines_zh = render_ai_overlay_lines(Some(entry), Locale::ZhCn, &i18n);
        assert!(
            lines_en.iter().any(|l| l == "AI source: tmux fallback"),
            "EN overlay must label the source as tmux fallback: {lines_en:?}"
        );
        assert!(
            lines_en.iter().any(|l| l == "AI accuracy: low"),
            "EN overlay must label the accuracy as low: {lines_en:?}"
        );
        assert!(
            lines_zh.iter().any(|l| l == "AI 来源: tmux 回退"),
            "ZH overlay must label the source as tmux 回退: {lines_zh:?}"
        );
        assert!(
            lines_zh.iter().any(|l| l == "AI 准确性: 低"),
            "ZH overlay must label the accuracy as 低: {lines_zh:?}"
        );
    }
}

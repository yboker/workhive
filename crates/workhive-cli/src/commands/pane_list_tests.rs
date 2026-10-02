use super::{
    build_pane_list_hint_block, build_pane_list_legend_block, build_pane_list_no_hook_event_hint,
    build_pane_list_render, compose_pane_list_post_table_lines,
};
use crate::commands::pane_activity_overlay::AiActivityOverlayEntry;
use crate::context::CommandContext;
use workhive_core::domain::{AiActivityProvider, AiActivitySource, PaneKind, PaneState, UnitId};
use workhive_core::usecases::ai_integrations::{FeatureGate, FeatureGateResolution};
use workhive_core::usecases::pane::PaneListRow;

fn list_row(
    active: bool,
    index: u32,
    target_id: &str,
    current_command: &str,
    launch_identity_title: Option<&str>,
) -> PaneListRow {
    PaneListRow {
        active,
        index,
        target_id: UnitId::new(target_id),
        current_command: current_command.to_owned(),
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        activity: workhive_core::domain::PaneActivityState::ShellIdle,
        message_safety: workhive_core::domain::PaneMessageSafety::ShellInput,
        launch_identity_title: launch_identity_title.map(str::to_owned),
        source: workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
        confidence: workhive_core::domain::PaneObservationConfidence::Fallback,
        cwd: "/Users/test/project".to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn list_row_with_activity(
    active: bool,
    index: u32,
    target_id: &str,
    current_command: &str,
    kind: PaneKind,
    state: PaneState,
    activity: workhive_core::domain::PaneActivityState,
    message_safety: workhive_core::domain::PaneMessageSafety,
    source: workhive_core::domain::PaneObservationSource,
) -> PaneListRow {
    PaneListRow {
        active,
        index,
        target_id: UnitId::new(target_id),
        current_command: current_command.to_owned(),
        kind,
        state,
        activity,
        message_safety,
        launch_identity_title: None,
        source,
        confidence: source.baseline_confidence(),
        cwd: "/Users/test/project".to_owned(),
    }
}

fn test_context() -> CommandContext {
    CommandContext::new()
}

/// Verbose-mode test builder mirrors what the production
/// `workhive --verbose pane list` path triggers. Default mode
/// uses [`test_context`] for the concise post-table layout.
fn test_context_verbose() -> CommandContext {
    CommandContext::with_verbose(true)
}

#[test]
fn pane_list_default_render_omits_diagnostic_columns_zh() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let rows = [list_row(true, 0, "%0", "zsh", Some("codex"))];
    let (headers, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);

    assert_eq!(table_rows.len(), 1);
    assert!(
        !headers.iter().any(|h| h.contains("启动身份")),
        "headers must not include launch identity in default mode: {headers:?}",
    );
    for row in &table_rows {
        for cell in row {
            assert_ne!(
                cell, "启动身份",
                "table row must not echo the launch identity header label"
            );
        }
    }
}

#[test]
fn pane_list_default_render_uses_label_column() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let rows = [
        list_row(true, 0, "%0", "zsh", Some("codex")),
        list_row(false, 1, "%1", "lazygit", None),
    ];
    let (headers, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);

    // Label column lives at the same offset as the historical
    // "command" column (index 3); the new header must reflect the
    // user-recognizable selection label, not the live command.
    assert_eq!(headers[3], "label");
    assert_eq!(table_rows[0][3], "codex");
    assert_eq!(table_rows[1][3], "lazygit");
}

#[test]
fn pane_list_default_render_suppresses_internal_workhive_label() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let rows = [list_row(
        false,
        2,
        "%2",
        "workhive internal pane-supervisor --unit %2",
        None,
    )];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    assert_eq!(
        table_rows[0][3], "—",
        "internal workhive supervisor wrapper must not be surfaced as a label"
    );
}

/// 2026-05-13 UX contract: `pane list` must surface the detection
/// source as its own column, separate from the activity column.
/// The old implementation appended a compact `[source/accuracy]`
/// suffix inside the activity cell which conflated two pieces of
/// information and made the activity column unscannable. The new
/// layout keeps the activity cell concise and adds a dedicated
/// `source` column so users can scan "where is this signal
/// coming from?" independently of "what state is it in?".
///
/// Column layout after this change (8 columns):
///
///   index 0 active marker
///   index 1 number
///   index 2 target id
///   index 3 label
///   index 4 kind
///   index 5 activity   (bare; no `[...]` suffix)
///   index 6 source     (NEW)
///   index 7 cwd
#[test]
fn pane_list_render_emits_source_column_per_row_en() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let rows = [
        list_row(true, 0, "%0", "claude-code", Some("claude-code")),
        list_row(false, 1, "%1", "codex", Some("codex")),
        list_row(false, 2, "%2", "opencode", Some("opencode")),
    ];
    let overlay = [
        AiActivityOverlayEntry {
            label: Some("processing".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::ClaudeCode),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Hook),
            stale: false,
            source: Some(AiActivitySource::ClaudeCodeHook),
            state: Some(workhive_core::domain::AiActivityState::Processing),
        },
        AiActivityOverlayEntry {
            // Pin the user-facing wording. The fixture mirrors what
            // production resolves through `PANE_AI_ACTIVITY_OPEN_UNVERIFIED`
            // so the test documents what users actually see in the cell.
            label: Some("open (waiting for event)".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::Codex),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::WorkhiveSupervisor),
            state: Some(workhive_core::domain::AiActivityState::OpenUnverified),
        },
        AiActivityOverlayEntry {
            label: Some("unknown".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::Opencode),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::TmuxFallback),
            state: Some(workhive_core::domain::AiActivityState::Unknown),
        },
    ];
    let (headers, table_rows) = build_pane_list_render(&ctx, locale, &rows, &overlay);
    assert_eq!(
        headers.len(),
        9,
        "header row must have 9 columns now (including dedicated source and confidence columns), got {headers:?}"
    );
    assert_eq!(headers[5], "Activity");
    assert_eq!(headers[6], "source");
    assert_eq!(headers[7], "confidence");
    assert_eq!(headers[8], "cwd");
    assert_eq!(table_rows.len(), 3);
    // Concise contract: `pane list` shows one of five labels only.
    // Claude row's hook fact maps Processing -> running; Codex row's
    // unverified-open maps to waiting-input; Opencode's unknown maps
    // to the explicit unknown bucket.
    assert_eq!(table_rows[0][5], "running");
    assert_eq!(table_rows[1][5], "waiting for input");
    assert_eq!(table_rows[2][5], "unknown");
    assert_eq!(table_rows[0][6], "hook");
    assert_eq!(table_rows[1][6], "observer");
    assert_eq!(table_rows[2][6], "tmux");
    assert_eq!(table_rows[0][7], "high");
    assert_eq!(table_rows[1][7], "medium");
    assert_eq!(table_rows[2][7], "low");
    for row in &table_rows {
        assert!(
            !row[5].contains('['),
            "activity cell must not carry a `[...]` suffix anymore, got {:?}",
            row[5]
        );
        for cell in row {
            assert!(
                !cell.contains("workhive integrations"),
                "table cell leaked rescinded command text: {cell:?}"
            );
        }
    }
}

#[test]
fn pane_list_render_emits_source_column_per_row_zh() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let rows = [
        list_row(true, 0, "%0", "claude-code", Some("claude-code")),
        list_row(false, 1, "%1", "codex", Some("codex")),
        list_row(false, 2, "%2", "opencode", Some("opencode")),
    ];
    let overlay = [
        AiActivityOverlayEntry {
            label: Some("处理中".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::ClaudeCode),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Hook),
            stale: false,
            source: Some(AiActivitySource::ClaudeCodeHook),
            state: Some(workhive_core::domain::AiActivityState::Processing),
        },
        AiActivityOverlayEntry {
            // ZH counterpart of the EN fixture above; the typed
            // OpenUnverified state collapses to the concise
            // waiting-input label in `pane list`.
            label: Some("已打开（等待事件）".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::Codex),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::WorkhiveSupervisor),
            state: Some(workhive_core::domain::AiActivityState::OpenUnverified),
        },
        AiActivityOverlayEntry {
            label: Some("未知".to_owned()),
            reason: None,
            provider: Some(AiActivityProvider::Opencode),
            confidence: Some(workhive_core::domain::AiActivityConfidence::Fallback),
            stale: false,
            source: Some(AiActivitySource::TmuxFallback),
            state: Some(workhive_core::domain::AiActivityState::Unknown),
        },
    ];
    let (headers, table_rows) = build_pane_list_render(&ctx, locale, &rows, &overlay);
    assert_eq!(headers.len(), 9);
    assert_eq!(headers[5], "活动状态");
    assert_eq!(headers[6], "来源");
    assert_eq!(headers[7], "可信度");
    // Concise contract in ZH: the same five labels in their
    // localized form, regardless of how detailed the overlay's
    // typed state is.
    assert_eq!(table_rows[0][5], "执行中");
    assert_eq!(table_rows[1][5], "等待输入");
    assert_eq!(table_rows[2][5], "未知");
    assert_eq!(table_rows[0][6], "钩子");
    assert_eq!(table_rows[1][6], "监控");
    assert_eq!(table_rows[2][6], "tmux");
    assert_eq!(table_rows[0][7], "高");
    assert_eq!(table_rows[1][7], "中");
    assert_eq!(table_rows[2][7], "低");
    for row in &table_rows {
        assert!(
            !row[5].contains('['),
            "ZH activity cell must not carry a `[...]` suffix anymore, got {:?}",
            row[5]
        );
        for cell in row {
            assert!(
                !cell.contains("workhive integrations"),
                "ZH table cell leaked rescinded command text: {cell:?}"
            );
        }
    }
}

/// Non-AI rows (shells, generic foreground processes) must
/// render the source column as the conservative `none` token,
/// not a bogus AI source. The activity cell stays bare and the
/// source column makes the absence of an AI signal explicit.
#[test]
fn pane_list_render_source_column_renders_none_for_non_ai_rows() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "zsh", None)];
    let overlay = [AiActivityOverlayEntry::default()];
    for (locale, expected_none) in [
        (workhive_core::domain::Locale::En, "none"),
        (workhive_core::domain::Locale::ZhCn, "无"),
    ] {
        let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &overlay);
        assert!(
            !table_rows[0][5].contains('['),
            "shell row activity must stay bare, got {:?}",
            table_rows[0][5]
        );
        assert_eq!(
            table_rows[0][6], expected_none,
            "shell row must render the conservative `{expected_none}` source token"
        );
    }
}

/// English-locale contract: the hint block must be a structured
/// five-line list that:
///   * leads with a `Tips:` heading,
///   * explains the `*` active marker on its own line,
///   * introduces the example commands on its own line,
///   * renders each example command as its own indented line so
///     the user can copy/paste without the line being broken by
///     terminal wrapping.
#[test]
fn pane_list_hint_block_en_includes_active_marker_and_unsplit_examples() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let lines = build_pane_list_hint_block(&ctx, locale);

    assert_eq!(
        lines.len(),
        5,
        "structured hint block must have exactly 5 lines, got {lines:?}"
    );
    assert_eq!(lines[0], "Tips:");
    assert!(
        lines[1].contains("active") && lines[1].contains('*'),
        "active-marker explanation must be present, got {:?}",
        lines[1]
    );
    assert_eq!(lines[3], "    workhive pane send 2 \"...\"");
    assert_eq!(lines[4], "    workhive pane send %2 \"...\"");
}

/// Chinese-locale contract: the same structure as English, with
/// the localized active-marker explanation and the command examples
/// kept verbatim — Chinese wrapping is the historical regression
/// source. The example lines MUST NOT be translated; they are
/// literal commands the user copy/pastes.
#[test]
fn pane_list_hint_block_zh_includes_active_marker_and_unsplit_examples() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let lines = build_pane_list_hint_block(&ctx, locale);

    assert_eq!(
        lines.len(),
        5,
        "structured hint block must have exactly 5 lines, got {lines:?}"
    );
    assert_eq!(lines[0], "提示：");
    assert!(
        lines[1].contains('*') && lines[1].contains("当前"),
        "Chinese active-marker explanation must mention `*` and the localized active-row term, got {:?}",
        lines[1]
    );
    assert_eq!(lines[3], "    workhive pane send 2 \"...\"");
    assert_eq!(lines[4], "    workhive pane send %2 \"...\"");
}

/// The column legend block must explain the three column
/// semantics that previously had no inline documentation:
/// The activity column now renders only the five concise labels.
/// The legend body explains that vocabulary (waiting for input /
/// running / completed / exited / unknown) and directs users at
/// `pane show` for the underlying source, confidence, and reason.
/// Both locales share the same shape (heading + 3 explanation lines).
///
/// Workhive no longer installs provider-side hooks/plugins, so the
/// legend MUST NOT leak the deprecated "hook/plugin" vocabulary or
/// guide users toward any provider-side approval flow.
#[test]
fn pane_list_legend_block_en_explains_activity_source_and_confidence() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let lines = build_pane_list_legend_block(&ctx, locale);

    assert_eq!(
        lines.len(),
        4,
        "legend block must have heading + 3 explanation lines, got {lines:?}"
    );
    assert_eq!(lines[0], "Column legend:");
    for token in [
        "waiting for input",
        "running",
        "completed",
        "exited",
        "unknown",
    ] {
        assert!(
            lines[1].contains(token),
            "EN activity legend must enumerate concise label `{token}`, got {:?}",
            lines[1]
        );
    }
    assert!(
        lines[1].contains("pane show"),
        "EN activity legend must point users at `pane show` for detail, got {:?}",
        lines[1]
    );
    assert!(
        !lines[1].contains("(waiting for signal)"),
        "EN activity legend must not regress to the verbose AI-specific wording, got {:?}",
        lines[1]
    );
    assert!(
        lines[2].contains("source")
            && lines[2].contains("completion signal")
            && lines[2].contains("tmux"),
        "source legend must include the accuracy ordering, got {:?}",
        lines[2]
    );
    assert!(
        lines[3].contains("confidence") && lines[3].contains("completion signal"),
        "confidence legend must name the column and ranking, got {:?}",
        lines[3]
    );
    for forbidden in ["hook", "plugin", "approve", "approval", "/hooks"] {
        for line in &lines {
            assert!(
                !line.contains(forbidden),
                "legend must not surface deprecated provider hook/plugin/approval vocabulary `{forbidden}`, got {line:?}",
            );
        }
    }
}

/// Chinese-locale counterpart of the legend block test. The
/// English-phrase markers (`*`, `pane send`) are user-visible
/// even in zh-CN output, but the legend body must be in
/// Chinese; we assert on the localized terms verbatim because
/// they are exactly what users will see in the table. The
/// legend MUST NOT leak the deprecated `钩子/插件` vocabulary
/// or any provider-side approval guidance.
#[test]
fn pane_list_legend_block_zh_explains_activity_source_and_confidence() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let lines = build_pane_list_legend_block(&ctx, locale);

    assert_eq!(lines.len(), 4, "legend block must have 4 lines: {lines:?}");
    assert_eq!(lines[0], "列说明：");
    for token in ["等待输入", "执行中", "已完成", "已退出", "未知"] {
        assert!(
            lines[1].contains(token),
            "ZH activity legend must enumerate concise label `{token}`, got {:?}",
            lines[1]
        );
    }
    assert!(
        lines[1].contains("pane show"),
        "ZH activity legend must point users at `pane show` for detail, got {:?}",
        lines[1]
    );
    assert!(
        !lines[1].contains("等待信号"),
        "ZH activity legend must not regress to the verbose AI-specific wording, got {:?}",
        lines[1]
    );
    assert!(
        lines[2].contains("来源") && lines[2].contains("完成信号"),
        "source legend must mention 来源 column and the zh tag ordering, got {:?}",
        lines[2]
    );
    assert!(
        lines[3].contains("可信度") && lines[3].contains("完成信号"),
        "confidence legend must mention 可信度 column and the zh source ranking, got {:?}",
        lines[3]
    );
    for forbidden in ["钩子", "插件", "/hooks", "批准", "审核"] {
        for line in &lines {
            assert!(
                !line.contains(forbidden),
                "legend must not surface deprecated provider hook/plugin/approval vocabulary `{forbidden}`, got {line:?}",
            );
        }
    }
}

/// Legend block must keep the same indentation contract as the
/// existing Tips block: heading flush left, body lines
/// two-space-indented so the legend reads as a single visual
/// unit under the heading.
#[test]
fn pane_list_legend_block_uses_consistent_indentation() {
    let ctx = test_context();
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_legend_block(&ctx, locale);
        assert!(
            !lines[0].starts_with(' '),
            "legend heading must be flush left: {:?}",
            lines[0]
        );
        for (idx, line) in lines.iter().enumerate().skip(1) {
            assert!(
                line.starts_with("  "),
                "legend body line {idx} must be two-space indented: {line:?}",
            );
        }
    }
}

/// The production `print_pane_list` calls
/// [`compose_pane_list_post_table_lines`] for everything after the
/// rendered table. This is the production composition path — any
/// future regression that drops the tips block, the legend block,
/// or their ordering will break the assertions below. The case
/// here covers a Codex pane with a fresh hook event: the
/// `/hooks` review hint must NOT appear because the lifecycle
/// resolver classifies the pane as `Active`.
#[test]
fn compose_pane_list_post_table_lines_high_confidence_codex_emits_tips_then_legend_only() {
    let ctx = test_context_verbose();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::CodexHook),
        stale: false,
        ..AiActivityOverlayEntry::default()
    }];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = compose_pane_list_post_table_lines(
            &ctx,
            locale,
            &rows,
            &overlay,
            &catalog,
            &resolution,
        );
        assert_eq!(
            lines.first().map(String::as_str),
            Some(""),
            "post-table block must start with blank separator ({locale:?}): {lines:?}"
        );
        let tips_idx = lines
            .iter()
            .position(|l| l.starts_with("Tips:") || l.starts_with("提示"))
            .unwrap_or_else(|| panic!("tips heading missing ({locale:?}): {lines:?}"));
        let legend_idx = lines
            .iter()
            .position(|l| l.starts_with("Column legend:") || l.starts_with("列说明"))
            .unwrap_or_else(|| panic!("legend heading missing ({locale:?}): {lines:?}"));
        assert!(
            tips_idx < legend_idx,
            "tips block must precede legend block ({locale:?}), got tips@{tips_idx} legend@{legend_idx}: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.contains("/hooks")),
            "high-confidence Codex hook row must NOT emit `/hooks` review hint ({locale:?}): {lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|l| { l == "Hook/plugin events:" || l == "钩子 / 插件事件：" }),
            "high-confidence Codex hook row must NOT emit no-hook-event hint heading ({locale:?}): {lines:?}"
        );
    }
}

/// Codex pane with no recorded completion signal yet must emit
/// the neutral "completion signals" hint after the tips + legend
/// blocks. Workhive no longer installs provider-side hooks, so
/// the verbose post-table block MUST NOT push the user toward
/// `/hooks`, "Workhive managed hook", "approve", or any
/// provider-side approval flow.
#[test]
fn compose_pane_list_post_table_lines_codex_no_hook_event_emits_neutral_completion_hint() {
    let ctx = test_context_verbose();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = compose_pane_list_post_table_lines(
            &ctx,
            locale,
            &rows,
            &overlay,
            &catalog,
            &resolution,
        );
        let legend_idx = lines
            .iter()
            .position(|l| l.starts_with("Column legend:") || l.starts_with("列说明"))
            .unwrap_or_else(|| panic!("legend heading missing ({locale:?}): {lines:?}"));
        let completion_idx = lines
            .iter()
            .position(|l| l.starts_with("Completion signals:") || l.starts_with("完成信号"))
            .unwrap_or_else(|| {
                panic!(
                    "completion-signal hint missing for degraded Codex pane ({locale:?}): {lines:?}"
                )
            });
        assert!(
            legend_idx < completion_idx,
            "completion-signal hint must follow the legend block ({locale:?}), got legend@{legend_idx} completion@{completion_idx}: {lines:?}"
        );
        let separator_between = lines[legend_idx..completion_idx]
            .iter()
            .any(|l| l.is_empty());
        assert!(
            separator_between,
            "blank separator must exist between legend and completion-signal hint ({locale:?}): {lines:?}"
        );
        for forbidden in ["/hooks", "Workhive managed hook", "approve", "approval"] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "verbose post-table block must NOT push the user into a provider hook approval flow (`{forbidden}`) ({locale:?}): {lines:?}",
            );
        }
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("钩子") || l.contains("批准") || l.contains("审核")),
            "verbose post-table block must NOT push the user into a provider hook approval flow (zh wording) ({locale:?}): {lines:?}",
        );
    }
}

/// The composer is the single production path that decides
/// ordering. Even with NO rows, the tips + legend blocks must
/// still render so users see the column explanations the first
/// time they run `pane list`. The no-hook-event sub-block must
/// NOT fire when there are no AI panes (otherwise users would
/// see unsolicited Codex guidance).
#[test]
fn compose_pane_list_post_table_lines_emits_tips_and_legend_even_without_ai_panes() {
    let ctx = test_context_verbose();
    let rows = [list_row(false, 0, "%0", "zsh", None)];
    let overlay = [AiActivityOverlayEntry::default()];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    let lines = compose_pane_list_post_table_lines(
        &ctx,
        workhive_core::domain::Locale::En,
        &rows,
        &overlay,
        &catalog,
        &resolution,
    );

    assert!(
        lines.iter().any(|l| l.starts_with("Tips:")),
        "tips heading must always render: {lines:?}"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("Column legend:")),
        "legend heading must always render: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("/hooks")),
        "non-AI panes must not trigger Codex `/hooks` hint: {lines:?}"
    );
}

// ───────────────────────────────────────────────────────────────
// Concise default `pane list` post-table contract.
//
// The default `workhive pane list` invocation (no `--verbose`)
// must emit a single short hint plus, when relevant, one short
// degraded line. The full Tips / Column legend / no-hook-event
// blocks live behind `--verbose`.
// ───────────────────────────────────────────────────────────────

/// English default-mode post-table block: single concise hint, no
/// `Tips:` heading, no `Column legend:` heading, no hook/plugin
/// paragraph. Degraded line absent because all rows are AI-clean.
#[test]
fn compose_pane_list_post_table_lines_concise_default_en_omits_full_blocks() {
    let ctx = test_context();
    let rows = [list_row(false, 0, "%0", "zsh", None)];
    let overlay = [AiActivityOverlayEntry::default()];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    let lines = compose_pane_list_post_table_lines(
        &ctx,
        workhive_core::domain::Locale::En,
        &rows,
        &overlay,
        &catalog,
        &resolution,
    );

    assert_eq!(
        lines.len(),
        2,
        "concise default block must be `blank + 1 hint` for AI-clean rows: {lines:?}"
    );
    assert_eq!(
        lines[0], "",
        "concise block must start with separator: {lines:?}"
    );
    assert!(
        lines[1].contains("workhive pane send") && lines[1].contains("--verbose"),
        "concise default hint must mention send command and --verbose: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("Tips:")),
        "concise default must NOT emit Tips heading: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("Column legend:")),
        "concise default must NOT emit Column legend heading: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "Hook/plugin events:"),
        "concise default must NOT emit no-hook-event paragraph heading: {lines:?}"
    );
}

/// Chinese default-mode mirror of the English contract above.
#[test]
fn compose_pane_list_post_table_lines_concise_default_zh_omits_full_blocks() {
    let ctx = test_context();
    let rows = [list_row(false, 0, "%0", "zsh", None)];
    let overlay = [AiActivityOverlayEntry::default()];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    let lines = compose_pane_list_post_table_lines(
        &ctx,
        workhive_core::domain::Locale::ZhCn,
        &rows,
        &overlay,
        &catalog,
        &resolution,
    );

    assert_eq!(
        lines.len(),
        2,
        "concise default block must be `blank + 1 hint` for AI-clean rows (zh): {lines:?}"
    );
    assert!(
        lines[1].contains("workhive pane send") && lines[1].contains("--verbose"),
        "concise zh default hint must mention send command and --verbose: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("提示")),
        "concise default must NOT emit zh tips heading: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("列说明")),
        "concise default must NOT emit zh legend heading: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("钩子 / 插件事件")),
        "concise default must NOT emit zh no-hook-event paragraph heading: {lines:?}"
    );
}

/// Codex pane with no recorded completion signal must emit exactly
/// one short, neutral degraded line under the concise hint, and
/// MUST NOT push the user toward `/hooks` or any provider-side
/// approval flow. The full paragraph stays out of the default
/// render.
#[test]
fn compose_pane_list_post_table_lines_concise_default_codex_review_emits_neutral_short_line() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());

    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = compose_pane_list_post_table_lines(
            &ctx,
            locale,
            &rows,
            &overlay,
            &catalog,
            &resolution,
        );

        assert_eq!(
            lines.len(),
            3,
            "concise codex-review block must be `blank + hint + degraded line` ({locale:?}): {lines:?}"
        );
        let degraded = lines.last().expect("non-empty lines");
        let degraded_matches = match locale {
            workhive_core::domain::Locale::En => {
                degraded.contains("completion signal")
                    && degraded.contains("source")
                    && degraded.contains("--verbose")
            }
            workhive_core::domain::Locale::ZhCn => {
                degraded.contains("完成信号")
                    && degraded.contains("来源")
                    && degraded.contains("--verbose")
            }
        };
        assert!(
            degraded_matches,
            "concise default degraded line must read as a neutral completion-signal hint ({locale:?}): {lines:?}",
        );
        for forbidden in ["/hooks", "Workhive managed hook", "approve", "approval"] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "concise default must NOT push the user into a provider hook approval flow (`{forbidden}`) ({locale:?}): {lines:?}",
            );
        }
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("钩子") || l.contains("批准") || l.contains("审核")),
            "concise default must NOT push the user into a provider hook approval flow (zh wording) ({locale:?}): {lines:?}",
        );
        assert!(
            !lines
                .iter()
                .any(|l| l.starts_with("Tips:") || l.starts_with("提示")),
            "concise default must NOT emit full Tips block ({locale:?}): {lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|l| l.starts_with("Column legend:") || l.starts_with("列说明")),
            "concise default must NOT emit full legend block ({locale:?}): {lines:?}"
        );
        assert!(
            !lines
                .iter()
                .any(|l| l == "Hook/plugin events:" || l == "钩子 / 插件事件："),
            "concise default must NOT emit deprecated hook/plugin paragraph heading ({locale:?}): {lines:?}"
        );
    }
}

/// Indentation contract: every line in the hint block must share
/// a stable left edge — either zero indent (the heading) or a
/// fixed two/four-space indent that aligns the bullet text and the
/// command examples. This is what makes the block read as a single
/// visual unit instead of a wall of text.
#[test]
fn pane_list_hint_block_uses_consistent_indentation() {
    let ctx = test_context();
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_hint_block(&ctx, locale);
        assert!(
            !lines[0].starts_with(' '),
            "hint heading must be flush left: {:?}",
            lines[0]
        );
        for idx in [1, 2] {
            assert!(
                lines[idx].starts_with("  ") && !lines[idx].starts_with("    "),
                "hint line {idx} must use a two-space indent: {:?}",
                lines[idx]
            );
        }
        for idx in [3, 4] {
            assert!(
                lines[idx].starts_with("    "),
                "hint command example must use a four-space indent: {:?}",
                lines[idx]
            );
        }
    }
}

/// 2026-05-13 contract: when at least one AI pane (kind = Agent
/// or overlay carries a known provider) is rendering with a
/// non-hook/plugin source, `pane list` must emit a conservative
/// post-table hint sub-block. The block must:
///   * announce the situation with a heading,
///   * describe the observable fact (no hook/plugin event has
///     been observed yet) and point at the new `source` column,
///   * NEVER assert that hooks are uninstalled (we have no
///     evidence of installation state from outside the pane),
///   * NEVER mention the rescinded `workhive integrations`
///     command in either locale.
#[test]
fn pane_list_no_hook_event_hint_emits_conservative_block_when_any_ai_pane_lacks_event() {
    let ctx = test_context();
    let rows = [
        list_row(true, 0, "%0", "claude", Some("claude-code")),
        list_row(false, 1, "%1", "zsh", None),
    ];
    let overlay = [
        AiActivityOverlayEntry {
            provider: Some(AiActivityProvider::ClaudeCode),
            source: Some(AiActivitySource::WorkhiveSupervisor),
            ..AiActivityOverlayEntry::default()
        },
        AiActivityOverlayEntry::default(),
    ];
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
        );
        assert!(
            !lines.is_empty(),
            "hint block must fire when any AI pane lacks a hook/plugin event ({locale:?})"
        );
        assert_eq!(
            lines.len(),
            2,
            "Claude-only degraded pane must produce heading + body only (no Codex line), got {lines:?}"
        );
        let body_lower = lines[1].to_lowercase();
        assert!(
            body_lower.contains("source") || lines[1].contains("来源"),
            "hint body must reference the `source` column so users can find affected rows, got {:?}",
            lines[1]
        );
        for forbidden in [
            "not installed",
            "uninstalled",
            "missing hook",
            "未安装",
            "尚未安装",
            "没有安装",
        ] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "hint must stay conservative — must not contain {forbidden:?}, got {lines:?}"
            );
        }
        for line in &lines {
            assert!(
                !line.contains("workhive integrations"),
                "no-hook-event hint must never reference rescinded `workhive integrations` command, got {line:?}"
            );
        }
    }
}

/// Workhive no longer installs provider-side hooks. For a degraded
/// Codex pane the hint block must still fire (so the user knows
/// completion signals are pending) but it MUST NOT push the user
/// into a `/hooks` approval flow or any other provider-side
/// approval workflow. The hint stays the same neutral
/// "completion-signal" copy emitted for any other provider.
#[test]
fn pane_list_no_hook_event_hint_stays_neutral_for_codex_pane_without_signal() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
        );
        assert_eq!(
            lines.len(),
            2,
            "degraded Codex pane must produce neutral heading + body only (no provider-specific approval line) ({locale:?}), got {lines:?}"
        );
        for forbidden in ["/hooks", "Workhive managed hook", "approve", "approval"] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "hint must not push the user into a provider hook approval flow (`{forbidden}`) ({locale:?}), got {lines:?}",
            );
        }
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("钩子") || l.contains("批准") || l.contains("审核")),
            "hint must not push the user into a provider hook approval flow (zh wording) ({locale:?}), got {lines:?}",
        );
        for forbidden in ["not installed", "uninstalled", "未安装"] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "hint must stay conservative — must not contain {forbidden:?}, got {lines:?}"
            );
        }
    }
}

/// The Codex `/hooks` review line must NOT appear when no
/// degraded AI pane resembles Codex. A user running only
/// Claude/OpenCode should not be shown unrelated Codex
/// instructions.
#[test]
fn pane_list_no_hook_event_hint_omits_codex_line_for_non_codex_degraded_panes() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "claude", Some("claude-code"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::ClaudeCode),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
        );
        assert!(
            !lines.iter().any(|l| l.contains("/hooks")),
            "Codex `/hooks` line must NOT appear when no degraded pane is Codex ({locale:?}), got {lines:?}"
        );
    }
}

/// The hint block must be empty when every AI pane is rendering
/// with a fresh hook/plugin event — there is nothing to warn
/// about, and emitting noise here would train users to ignore
/// the block.
#[test]
fn pane_list_no_hook_event_hint_is_empty_when_all_ai_panes_have_fresh_hook_events() {
    let ctx = test_context();
    let rows = [
        list_row(true, 0, "%0", "claude", Some("claude-code")),
        list_row(false, 1, "%1", "codex", Some("codex")),
        list_row(false, 2, "%2", "opencode", Some("opencode")),
    ];
    let overlay = [
        AiActivityOverlayEntry {
            provider: Some(AiActivityProvider::ClaudeCode),
            source: Some(AiActivitySource::ClaudeCodeHook),
            stale: false,
            ..AiActivityOverlayEntry::default()
        },
        AiActivityOverlayEntry {
            provider: Some(AiActivityProvider::Codex),
            source: Some(AiActivitySource::CodexHook),
            stale: false,
            ..AiActivityOverlayEntry::default()
        },
        AiActivityOverlayEntry {
            provider: Some(AiActivityProvider::Opencode),
            source: Some(AiActivitySource::OpencodePlugin),
            stale: false,
            ..AiActivityOverlayEntry::default()
        },
    ];
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
        );
        assert!(
            lines.is_empty(),
            "hint must stay quiet when every AI pane has a fresh hook/plugin event ({locale:?}), got {lines:?}"
        );
    }
}

/// Stale hook/plugin facts count as "no fresh event observed":
/// the user might still need to relaunch through Workhive to
/// re-attach. The hint block must therefore fire on stale
/// hook/plugin sources too.
#[test]
fn pane_list_no_hook_event_hint_fires_for_stale_hook_overlay() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "claude", Some("claude-code"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::ClaudeCode),
        source: Some(AiActivitySource::ClaudeCodeHook),
        stale: true,
        ..AiActivityOverlayEntry::default()
    }];
    let lines = build_pane_list_no_hook_event_hint(
        &ctx,
        workhive_core::domain::Locale::En,
        &rows,
        &overlay,
        &workhive_core::domain::ToolCatalog::builtin_defaults(),
        &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
    );
    assert!(
        !lines.is_empty(),
        "stale hook overlay must still trigger the no-fresh-event hint, got {lines:?}"
    );
}

/// Regression contract: the resolver must still recognize
/// architecture-suffixed Codex pane commands
/// (e.g. `codex-x86_64-apple-darwin`) so the hint block fires
/// when they have no recorded completion signal yet. Substring
/// matching is *not* used; the shared resolver consults
/// [`workhive_core::domain::ToolCatalog`]'s built-in defaults
/// which already model these exact platform-suffixed names.
/// The hint stays the neutral completion-signal copy and MUST
/// NOT push the user toward `/hooks` approval.
#[test]
fn pane_list_no_hook_event_hint_fires_for_architecture_suffixed_codex_command() {
    let ctx = test_context();
    for arch_suffixed in [
        "codex-x86_64-apple-darwin",
        "codex-aarch64-apple-darwin",
        "codex-x86_64-unknown-linux-gnu",
        "codex-aarch64-unknown-linux-gnu",
    ] {
        let rows = [list_row_with_activity(
            true,
            0,
            "%0",
            arch_suffixed,
            PaneKind::Agent,
            PaneState::Running,
            workhive_core::domain::PaneActivityState::AgentForeground,
            workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
            workhive_core::domain::PaneObservationSource::Supervisor,
        )];
        let overlay = [AiActivityOverlayEntry {
            provider: None,
            source: Some(AiActivitySource::WorkhiveSupervisor),
            ..AiActivityOverlayEntry::default()
        }];
        for locale in [
            workhive_core::domain::Locale::En,
            workhive_core::domain::Locale::ZhCn,
        ] {
            let lines = build_pane_list_no_hook_event_hint(
                &ctx,
                locale,
                &rows,
                &overlay,
                &workhive_core::domain::ToolCatalog::builtin_defaults(),
                &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
            );
            assert_eq!(
                lines.len(),
                2,
                "architecture-suffixed Codex pane `{arch_suffixed}` must trigger the neutral two-line hint ({locale:?}), got {lines:?}",
            );
            for forbidden in ["/hooks", "Workhive managed hook", "approve", "approval"] {
                assert!(
                    !lines.iter().any(|l| l.contains(forbidden)),
                    "architecture-suffixed Codex pane `{arch_suffixed}` must not push the user into a provider hook approval flow (`{forbidden}`) ({locale:?}), got {lines:?}",
                );
            }
            assert!(
                !lines
                    .iter()
                    .any(|l| l.contains("钩子") || l.contains("批准") || l.contains("审核")),
                "architecture-suffixed Codex pane `{arch_suffixed}` must not push the user into a provider hook approval flow (zh wording) ({locale:?}), got {lines:?}",
            );
        }
    }
}

/// Negative-guard contract: commands that merely *look* like
/// Codex (e.g. `codex-extra`, `mycodex`, or a catalog-unmodeled
/// extra-suffixed name) must NOT trigger the Codex `/hooks` line.
/// This proves the resolver does no `contains("codex")` substring
/// matching and that the no-hook-event hint inherits that
/// strictness.
#[test]
fn pane_list_no_hook_event_hint_omits_codex_line_for_unmodeled_codex_lookalike_commands() {
    let ctx = test_context();
    for lookalike in [
        "codex-extra",
        "codex-x86_64-apple-darwin-extra",
        "mycodex",
        "not-codex",
    ] {
        let rows = [list_row_with_activity(
            true,
            0,
            "%0",
            lookalike,
            PaneKind::Agent,
            PaneState::Running,
            workhive_core::domain::PaneActivityState::AgentForeground,
            workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
            workhive_core::domain::PaneObservationSource::Supervisor,
        )];
        let overlay = [AiActivityOverlayEntry {
            provider: None,
            source: Some(AiActivitySource::WorkhiveSupervisor),
            ..AiActivityOverlayEntry::default()
        }];
        for locale in [
            workhive_core::domain::Locale::En,
            workhive_core::domain::Locale::ZhCn,
        ] {
            let lines = build_pane_list_no_hook_event_hint(
                &ctx,
                locale,
                &rows,
                &overlay,
                &workhive_core::domain::ToolCatalog::builtin_defaults(),
                &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
            );
            assert!(
                !lines.iter().any(|l| l.contains("/hooks")),
                "lookalike command `{lookalike}` must NOT trigger /hooks line — no substring matching allowed ({locale:?}), got {lines:?}",
            );
        }
    }
}

/// The hint must stay quiet for shells, lazygit, and other
/// non-AI panes — there is nothing AI-specific to advise about.
#[test]
fn pane_list_no_hook_event_hint_is_empty_when_no_ai_panes_present() {
    let ctx = test_context();
    let rows = [
        list_row(true, 0, "%0", "zsh", None),
        list_row(false, 1, "%1", "lazygit", None),
    ];
    let overlay = [
        AiActivityOverlayEntry::default(),
        AiActivityOverlayEntry::default(),
    ];
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &FeatureGateResolution::Resolved(FeatureGate::builtin_default()),
        );
        assert!(
            lines.is_empty(),
            "hint must stay quiet when no AI pane is present ({locale:?}), got {lines:?}"
        );
    }
}

/// Stability contract: the active column header must NOT be
/// renamed (renaming would be a churn-y break for callers
/// scripting against `pane list`), but the marker MUST appear in
/// the row when the pane is active. Combined with the hint block's
/// dedicated explanation line, the user has a self-readable view.
#[test]
fn pane_list_render_active_marker_is_present_for_focused_pane() {
    let ctx = test_context();
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let rows = [
            list_row(true, 0, "%0", "zsh", Some("codex")),
            list_row(false, 1, "%1", "zsh", None),
        ];
        let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
        assert_eq!(table_rows[0][0], "*", "active row must show `*`");
        assert_eq!(table_rows[1][0], "", "non-active row must be blank");
    }
}

/// Label-normalization contract: tmux's `pane_current_command`
/// reports `-zsh` for login shells. The simplified `pane list`
/// label must drop the leading dash. This pins the end-to-end
/// view-model path: the row's selection label, as it would appear
/// in the rendered table, never carries the dash.
#[test]
fn pane_list_render_strips_login_shell_dash_in_label() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let rows = [list_row(false, 0, "%0", "-zsh", None)];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    assert_eq!(
        table_rows[0][3], "zsh",
        "login-shell dash must be stripped from the rendered label"
    );
}

// ── Activity column semantics ───────────────────────────────
//
//   * The visible status column must not render "运行中" /
//     "running" for a pane that merely has a non-shell foreground
//     process. That conflates the ambiguous `PaneState::Running`
//     with "AI is busy".
//   * Unverified agents (foreground command looks like `codex` /
//     `claude` but no supervisor handshake) must not surface as
//     "AI 助手运行中" / "agent busy".
//   * `Unknown` activity must be conservative.

#[test]
fn pane_list_render_unverified_codex_does_not_show_zh_running_in_status_column() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let rows = [list_row_with_activity(
        false,
        1,
        "%1",
        "codex",
        PaneKind::Agent,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::ForegroundProcess,
        workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
        workhive_core::domain::PaneObservationSource::ProcessObserver,
    )];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    let status = &table_rows[0][5];
    // Concise contract: a foreground codex process with no busy
    // signal must collapse to the waiting-input bucket, not 执行中
    // (running). The activity column never claims AI work without
    // an explicit busy signal.
    assert_eq!(
        status, "等待输入",
        "unsupervised foreground codex must surface the concise waiting-input label, got {status}"
    );
    assert_ne!(
        status, "执行中",
        "unsupervised foreground codex must NOT show 执行中 in the activity column: {status}"
    );
    assert!(
        !status.contains("已纳管"),
        "user-facing status must not leak the internal term 已纳管: {status}"
    );
}

/// Regression contract: a supervised AI agent whose recorded pid
/// is alive must NOT render as "running" without an explicit busy
/// signal. Under the concise contract this is expressed as the
/// waiting-input bucket — the agent process is reachable and idle.
/// The activity column must never include 运行中 / 忙碌 / 已纳管.
#[test]
fn pane_list_render_supervised_live_agent_uses_conservative_zh_label() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let rows = [list_row_with_activity(
        true,
        0,
        "%0",
        "claude",
        PaneKind::Agent,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::AgentForeground,
        workhive_core::domain::PaneMessageSafety::SafeStructured,
        workhive_core::domain::PaneObservationSource::Supervisor,
    )];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    let status = &table_rows[0][5];
    assert_eq!(
        status, "等待输入",
        "supervised live agent must surface the concise waiting-input \
             label without an explicit busy signal, got {status}"
    );
    assert!(
        !status.contains("已纳管"),
        "default zh output must not expose internal term 已纳管: {status}"
    );
    assert!(
        !status.contains("执行中") && !status.contains("运行中"),
        "supervised live agent label must not claim running: {status}"
    );
    assert!(
        !status.contains("忙碌") && !status.contains("处理中"),
        "supervised live agent label must not claim busy/processing \
             without an explicit busy signal: {status}"
    );
}

#[test]
fn pane_list_render_supervised_live_agent_uses_conservative_en_label() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::En;
    let rows = [list_row_with_activity(
        true,
        0,
        "%0",
        "claude",
        PaneKind::Agent,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::AgentForeground,
        workhive_core::domain::PaneMessageSafety::SafeStructured,
        workhive_core::domain::PaneObservationSource::Supervisor,
    )];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    let status = &table_rows[0][5];
    assert_eq!(
        status, "waiting for input",
        "supervised live agent must surface the concise waiting-input \
             label without an explicit busy signal, got {status}"
    );
    let lowered = status.to_lowercase();
    assert!(
        !lowered.contains("running") && !lowered.contains("busy") && !lowered.contains("active"),
        "en label must avoid running/busy/active wording: {status}"
    );
}

#[test]
fn pane_list_render_shell_idle_uses_shell_idle_label() {
    let ctx = test_context();
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let rows = [list_row_with_activity(
            false,
            2,
            "%2",
            "zsh",
            PaneKind::Terminal,
            PaneState::Idle,
            workhive_core::domain::PaneActivityState::ShellIdle,
            workhive_core::domain::PaneMessageSafety::ShellInput,
            workhive_core::domain::PaneObservationSource::ProcessObserver,
        )];
        let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
        let status = &table_rows[0][5];
        assert!(
            !status.is_empty(),
            "shell idle must render a status, got empty"
        );
        assert_ne!(status, "running");
        assert_ne!(status, "运行中");
    }
}

#[test]
fn pane_list_render_unknown_activity_does_not_imply_running() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let rows = [list_row_with_activity(
        false,
        3,
        "%3",
        "",
        PaneKind::Unknown,
        PaneState::Unknown,
        workhive_core::domain::PaneActivityState::Unknown,
        workhive_core::domain::PaneMessageSafety::Unknown,
        workhive_core::domain::PaneObservationSource::Unknown,
    )];
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &rows, &[]);
    let status = &table_rows[0][5];
    assert_ne!(
        status, "运行中",
        "Unknown activity must never collapse to 运行中: {status}"
    );
    assert_eq!(status, "未知");
}

/// Stale launch identity (e.g. `pane split --cmd "codex"`) must
/// never *make* the activity column claim AI work. The activity
/// is taken from the live merged truth, not the historical
/// launch identity title. A live `ShellIdle` observation collapses
/// to the concise waiting-input label regardless of stale title.
#[test]
fn pane_list_render_stale_launch_identity_does_not_force_agent_busy() {
    let ctx = test_context();
    let locale = workhive_core::domain::Locale::ZhCn;
    let row = PaneListRow {
        active: false,
        index: 1,
        target_id: UnitId::new("%1"),
        current_command: "zsh".to_owned(),
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        activity: workhive_core::domain::PaneActivityState::ShellIdle,
        message_safety: workhive_core::domain::PaneMessageSafety::ShellInput,
        launch_identity_title: Some("codex".to_owned()),
        source: workhive_core::domain::PaneObservationSource::ProcessObserver,
        confidence: workhive_core::domain::PaneObservationConfidence::High,
        cwd: "/Users/test/project".to_owned(),
    };
    let (_, table_rows) = build_pane_list_render(&ctx, locale, &[row], &[]);
    let status = &table_rows[0][5];
    assert_eq!(
        status, "等待输入",
        "stale `codex` launch identity must NOT force the activity column \
             to claim AI work; live observation wins. got {status}"
    );
    assert!(
        !status.contains("执行中") && !status.contains("运行中"),
        "stale launch identity must not surface a running label: {status}"
    );
}

// ── AI integrations feature gate ────────────────────────────────
//
//   * Globally-disabled gate → the post-table "no hook/plugin event"
//     hint block is empty even when AI panes are missing fresh
//     events. No body, no Codex review line, no heading.
//   * Per-provider-disabled gate → the row is filtered out of the
//     hint inputs. For Codex specifically, gating Codex off must
//     suppress the Codex `/hooks` review line even when other
//     providers stay enabled.

/// When the AI integrations feature gate is disabled globally,
/// the post-table "no hook/plugin event" hint must be empty
/// regardless of how many AI panes lack a fresh event. This is
/// the most visible "completely silent when disabled" surface.
#[test]
fn pane_list_no_hook_event_hint_is_empty_when_feature_gate_globally_disabled() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let resolution =
        FeatureGateResolution::Resolved(FeatureGate::from_switches(false, true, true, true));
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &resolution,
        );
        assert!(
            lines.is_empty(),
            "globally-disabled feature gate must suppress the entire hint block ({locale:?}), \
                 got {lines:?}"
        );
    }
}

/// Disabling only Codex must suppress the Codex `/hooks` review
/// line even if the rest of the gate is on. The generic body line
/// is also suppressed because Codex was the only AI pane in the
/// row set.
#[test]
fn pane_list_no_hook_event_hint_skips_codex_review_when_codex_provider_disabled() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let resolution =
        FeatureGateResolution::Resolved(FeatureGate::from_switches(true, true, false, true));
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &resolution,
        );
        assert!(
            lines.is_empty(),
            "per-provider gate-off must suppress hint output for the disabled provider \
                 ({locale:?}), got {lines:?}"
        );
    }
}

/// When the AI integration feature-gate resolution is
/// `Unavailable` (e.g. unreadable global config), the post-table
/// hint block must be empty: we cannot prove the user intends AI
/// integrations to be on, so we suppress every hook/plugin and
/// `/hooks` guidance line. Ordinary supervisor/process/tmux
/// information continues to render elsewhere.
#[test]
fn pane_list_no_hook_event_hint_is_empty_when_gate_resolution_is_unavailable() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let resolution = FeatureGateResolution::Unavailable {
        reason: "test: unreadable config".to_owned(),
    };
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &resolution,
        );
        assert!(
            lines.is_empty(),
            "Unavailable gate resolution must suppress every AI integration hint \
                 ({locale:?}), got {lines:?}"
        );
    }
}

/// Lifecycle-driven hint behaviour: when Codex is enabled and the
/// pane has no accurate completion-signal event,
/// `resolve_pane_lifecycle_state` returns `ReviewRequired`. The
/// hint block must still fire (so the user knows signals are
/// pending) but it MUST emit only the neutral completion-signal
/// copy. Workhive no longer installs provider-side hooks, so the
/// hint MUST NOT push the user toward `/hooks` or any
/// provider-side approval flow.
#[test]
fn pane_list_no_hook_event_hint_stays_neutral_when_lifecycle_is_review_required() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "codex", Some("codex"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let resolution =
        FeatureGateResolution::Resolved(FeatureGate::from_switches(true, true, true, true));
    for locale in [
        workhive_core::domain::Locale::En,
        workhive_core::domain::Locale::ZhCn,
    ] {
        let lines = build_pane_list_no_hook_event_hint(
            &ctx,
            locale,
            &rows,
            &overlay,
            &workhive_core::domain::ToolCatalog::builtin_defaults(),
            &resolution,
        );
        assert_eq!(
            lines.len(),
            2,
            "ReviewRequired lifecycle must still produce the neutral two-line hint ({locale:?}), got {lines:?}"
        );
        for forbidden in ["/hooks", "Workhive managed hook", "approve", "approval"] {
            assert!(
                !lines.iter().any(|l| l.contains(forbidden)),
                "ReviewRequired lifecycle must not push the user into a provider hook approval flow (`{forbidden}`) ({locale:?}), got {lines:?}",
            );
        }
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("钩子") || l.contains("批准") || l.contains("审核")),
            "ReviewRequired lifecycle must not push the user into a provider hook approval flow (zh wording) ({locale:?}), got {lines:?}",
        );
    }
}

/// Lifecycle gating: a Claude Code pane without an accurate event
/// resolves to `PendingFirstEvent`, NOT `ReviewRequired`. The
/// generic body line should appear, but the Codex `/hooks` review
/// line must not.
#[test]
fn pane_list_no_hook_event_hint_does_not_emit_codex_review_for_claude_code_pending() {
    let ctx = test_context();
    let rows = [list_row(true, 0, "%0", "claude", Some("claude"))];
    let overlay = [AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::ClaudeCode),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    }];
    let resolution =
        FeatureGateResolution::Resolved(FeatureGate::from_switches(true, true, true, true));
    let lines = build_pane_list_no_hook_event_hint(
        &ctx,
        workhive_core::domain::Locale::En,
        &rows,
        &overlay,
        &workhive_core::domain::ToolCatalog::builtin_defaults(),
        &resolution,
    );
    assert!(
        !lines.is_empty(),
        "PendingFirstEvent must still produce the generic hint body, got {lines:?}",
    );
    for line in &lines {
        assert!(
            !line.contains("/hooks"),
            "Claude PendingFirstEvent must not emit Codex review hint, got {line:?}",
        );
    }
}

/// Pins the `pane list --view json` envelope: a top-level `panes`
/// array whose rows carry the stable identity + flattened metric
/// keys agents depend on. Per-field metric semantics are covered in
/// `pane_metrics_view` tests; this guards the wrapper contract.
#[test]
fn pane_list_json_envelope_is_stable() {
    use super::render_pane_list_json;
    use crate::commands::pane_metrics_view::build_pane_list_json_entries;
    use workhive_core::domain::activity::PaneCorrelationKey;
    use workhive_core::usecases::activity_control_plane::metrics::{
        FreshnessThresholds, compute_pane_metrics,
    };

    let rows = vec![list_row(true, 0, "%1", "claude", Some("claude"))];
    let metrics = vec![compute_pane_metrics(
        &PaneCorrelationKey::new("g", "w", "u1"),
        &[],
        None,
        1_700_000_100_000,
        FreshnessThresholds {
            stale_after_ms: 30_000,
            expired_after_ms: 90_000,
        },
    )];
    let entries = build_pane_list_json_entries(&rows, &metrics, &[false]);
    let rendered = render_pane_list_json(&entries);
    let value: serde_json::Value =
        serde_json::from_str(&rendered).expect("rendered pane list JSON must parse");

    let panes = value
        .get("panes")
        .and_then(|v| v.as_array())
        .expect("payload must carry a `panes` array");
    assert_eq!(panes.len(), 1);

    let row = panes[0].as_object().expect("row must be an object");
    for key in [
        "target_id",
        "index",
        "active",
        "kind",
        "state",
        "source",
        "freshness",
        "elapsed_ms",
        "hint",
    ] {
        assert!(
            row.contains_key(key),
            "pane list --view json row must carry `{key}`: {rendered}"
        );
    }
    assert_eq!(row["target_id"], "%1");
    assert_eq!(row["index"], 0);
    assert_eq!(row["active"], true);
}

/// An empty pane snapshot must still produce a parseable machine
/// document (`{"panes": []}`) rather than localized prose.
#[test]
fn pane_list_json_envelope_renders_empty_array() {
    use super::render_pane_list_json;
    let rendered = render_pane_list_json(&[]);
    let value: serde_json::Value = serde_json::from_str(&rendered).expect("must parse");
    assert_eq!(
        value.get("panes").and_then(|v| v.as_array()).map(Vec::len),
        Some(0),
        "empty snapshot must render an empty panes array: {rendered}"
    );
}

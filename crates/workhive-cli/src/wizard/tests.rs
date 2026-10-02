use super::*;
use workhive_core::domain::{ToolEntry, ToolKind, ToolName};
use workhive_core::ports::{DependencyKind, DependencyStatus, RuntimeError};
use workhive_infra::dependency::{Platform, install_hint_for};
use workhive_testkit::fakes::FakeDependency;

fn check_dependencies_quiet(
    deps: &dyn DependencyPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(), CliError> {
    check_dependencies_for_backend(deps, &RuntimeBackendId::tmux(), i18n, locale, false)
}

/// A missing hard dependency must fail the quiet preflight check so the
/// caller aborts before any mutation. Preflight runs this check ahead of
/// every handler, so an `Err` here is what keeps a first-run command
/// from mutating state when Git or tmux is unavailable.
#[test]
fn dependency_preflight_separates_running_from_startable() {
    let deps = FakeDependency::new();
    let status = deps.preflight(DependencyKind::Tmux);
    assert!(status.installed);
    assert!(status.startable);
    assert!(status.compatible);
    assert!(!status.running);
    assert!(status.is_ready());
}

#[test]
fn dependency_preflight_marks_version_mismatch_unready() {
    let mut deps = FakeDependency::new();
    deps.set_status(
        DependencyKind::Tmux,
        DependencyStatus::VersionTooLow {
            kind: DependencyKind::Tmux,
            found: "2.9".to_owned(),
            minimum: "3.0".to_owned(),
        },
    );
    let status = deps.preflight(DependencyKind::Tmux);
    assert!(status.installed);
    assert!(!status.startable);
    assert!(!status.compatible);
    assert!(!status.is_ready());
}

/// A missing hard dependency must fail the quiet preflight check so the
/// caller aborts before any mutation. Preflight runs this check ahead of
/// every handler, so an `Err` here is what keeps a first-run command
/// from mutating state when Git or tmux is unavailable.
#[test]
fn quiet_dependency_check_blocks_when_git_missing() {
    let mut deps = FakeDependency::new();
    deps.set_missing(DependencyKind::Git);
    let i18n = StaticI18n::new();
    assert!(
        check_dependencies_quiet(&deps, &i18n, Locale::En).is_err(),
        "missing git must block the command before mutation"
    );
}

#[test]
fn quiet_dependency_check_blocks_when_tmux_missing() {
    let mut deps = FakeDependency::new();
    deps.set_missing(DependencyKind::Tmux);
    let i18n = StaticI18n::new();
    assert!(
        check_dependencies_quiet(&deps, &i18n, Locale::ZhCn).is_err(),
        "missing tmux must block the command before mutation"
    );
}

#[test]
fn selected_herdr_does_not_require_tmux() {
    let mut deps = FakeDependency::new();
    deps.set_missing(DependencyKind::Tmux);
    let i18n = StaticI18n::new();

    assert!(
        check_dependencies_for_backend(
            &deps,
            &RuntimeBackendId::herdr(),
            &i18n,
            Locale::En,
            false,
        )
        .is_ok()
    );
}

#[test]
fn init_dependency_check_only_requires_herdr_binary() {
    let mut deps = FakeDependency::new();
    deps.set_preflight_error(
        DependencyKind::Herdr,
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                server: "/tmp/herdr.sock".to_owned(),
            },
            detail: "connection refused".to_owned(),
        },
    );

    check_dependency_binaries_for_backend(
        &deps,
        &RuntimeBackendId::herdr(),
        &StaticI18n::new(),
        Locale::En,
        false,
    )
    .expect("init only needs the Herdr binary; server readiness belongs to runtime commands");
}

#[test]
fn selected_herdr_never_falls_back_when_unavailable() {
    let mut deps = FakeDependency::new();
    deps.set_missing(DependencyKind::Herdr);
    let i18n = StaticI18n::new();

    assert!(
        check_dependencies_for_backend(
            &deps,
            &RuntimeBackendId::herdr(),
            &i18n,
            Locale::En,
            false,
        )
        .is_err()
    );
}

#[test]
fn selected_herdr_version_too_low_includes_install_guidance() {
    let i18n = StaticI18n::new();
    let report = build_dependency_report(
        &[DependencyStatus::VersionTooLow {
            kind: DependencyKind::Herdr,
            found: "0.1.0".to_owned(),
            minimum: "0.8.0".to_owned(),
        }],
        &i18n,
        Locale::En,
        false,
    );
    assert!(report.has_failure);
    let text = report.lines.join("\n");
    assert!(text.contains("0.1.0"), "{text}");
    assert!(text.contains("0.8.0"), "{text}");
    assert!(text.contains("herdr") || text.contains("Herdr"), "{text}");
    assert!(text.contains("github.com/herdrdev/herdr"), "{text}");
}

#[test]
fn selected_herdr_missing_binary_includes_install_guidance() {
    let i18n = StaticI18n::new();
    let report = build_dependency_report(
        &[DependencyStatus::Missing {
            kind: DependencyKind::Herdr,
        }],
        &i18n,
        Locale::En,
        false,
    );
    assert!(report.has_failure);
    let text = report.lines.join("\n");
    assert!(text.contains("Herdr") || text.contains("herdr"), "{text}");
    assert!(text.contains("github.com/herdrdev/herdr"), "{text}");
}

#[test]
fn selected_herdr_incompatible_protocol_keeps_typed_error() {
    let mut deps = FakeDependency::new();
    deps.set_preflight_error(
        DependencyKind::Herdr,
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing {
                capability: "compatible Herdr protocol (server protocol 21, supported 22)"
                    .to_owned(),
            },
            detail: "unsupported Herdr protocol 21".to_owned(),
        },
    );
    let error = check_dependencies_for_backend(
        &deps,
        &RuntimeBackendId::herdr(),
        &StaticI18n::new(),
        Locale::En,
        false,
    )
    .expect_err("incompatible Herdr protocol must block before mutation");
    assert!(matches!(
        error,
        CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing { .. },
            ..
        })
    ));
}

#[test]
fn typed_backend_readiness_error_survives_dependency_preflight() {
    let mut deps = FakeDependency::new();
    deps.set_preflight_error(
        DependencyKind::Herdr,
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                server: "/tmp/herdr.sock".to_owned(),
            },
            detail: "connection refused".to_owned(),
        },
    );
    let error = check_dependencies_for_backend(
        &deps,
        &RuntimeBackendId::herdr(),
        &StaticI18n::new(),
        Locale::En,
        false,
    )
    .expect_err("unreachable Herdr must block before mutation");
    assert!(matches!(
        error,
        CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning { .. },
            ..
        })
    ));
}

#[test]
fn quiet_dependency_check_passes_when_satisfied() {
    let deps = FakeDependency::new();
    let i18n = StaticI18n::new();
    assert!(
        check_dependencies_quiet(&deps, &i18n, Locale::En).is_ok(),
        "satisfied dependencies must not block"
    );
}

/// Missing-Git guidance must name the dependency, show the install-hint
/// heading, and include the platform-specific install command, in both
/// English and Chinese.
#[test]
fn missing_git_report_includes_name_heading_and_platform_command() {
    let i18n = StaticI18n::new();
    let results = vec![DependencyStatus::Missing {
        kind: DependencyKind::Git,
    }];
    let platform_command = install_hint_for(DependencyKind::Git, Platform::detect());

    let en = build_dependency_report(&results, &i18n, Locale::En, false);
    assert!(en.has_failure, "missing git must mark a failure");
    let en_text = en.lines.join("\n");
    assert!(
        en_text.contains("Git"),
        "EN must name the dependency: {en_text}"
    );
    assert!(
        en_text.contains("Install Git:"),
        "EN must show the install-hint heading: {en_text}"
    );
    assert!(
        en_text.contains(&platform_command),
        "EN must include the platform install command: {en_text}"
    );

    let zh = build_dependency_report(&results, &i18n, Locale::ZhCn, false);
    let zh_text = zh.lines.join("\n");
    assert!(
        zh_text.contains("Git"),
        "zh-CN must name the dependency: {zh_text}"
    );
    assert!(
        zh_text.contains("安装 Git："),
        "zh-CN must show the install-hint heading: {zh_text}"
    );
    assert!(
        zh_text.contains(&platform_command),
        "zh-CN must include the platform install command: {zh_text}"
    );
}

/// Missing-tmux guidance must name the dependency, show the install-hint
/// heading, and include the platform-specific install command, in both
/// English and Chinese.
#[test]
fn missing_tmux_report_includes_name_heading_and_platform_command() {
    let i18n = StaticI18n::new();
    let results = vec![DependencyStatus::Missing {
        kind: DependencyKind::Tmux,
    }];
    let platform_command = install_hint_for(DependencyKind::Tmux, Platform::detect());

    let en = build_dependency_report(&results, &i18n, Locale::En, false);
    assert!(en.has_failure, "missing tmux must mark a failure");
    let en_text = en.lines.join("\n");
    assert!(
        en_text.contains("tmux"),
        "EN must name the dependency: {en_text}"
    );
    assert!(
        en_text.contains("Install tmux:"),
        "EN must show the install-hint heading: {en_text}"
    );
    assert!(
        en_text.contains(&platform_command),
        "EN must include the platform install command: {en_text}"
    );

    let zh = build_dependency_report(&results, &i18n, Locale::ZhCn, false);
    let zh_text = zh.lines.join("\n");
    assert!(
        zh_text.contains("tmux"),
        "zh-CN must name the dependency: {zh_text}"
    );
    assert!(
        zh_text.contains("安装 tmux："),
        "zh-CN must show the install-hint heading: {zh_text}"
    );
    assert!(
        zh_text.contains(&platform_command),
        "zh-CN must include the platform install command: {zh_text}"
    );
}

struct NoExecutables;

impl ExecutableAvailabilityPort for NoExecutables {
    fn is_available(&self, _program: &str) -> bool {
        false
    }
}

#[test]
fn detect_catalog_commands_marks_missing_tools_without_hiding_them() {
    let mut catalog = ToolCatalog::empty();
    catalog
        .add(ToolEntry::new(
            ToolName::new("custom-agent").expect("valid tool name"),
            "Custom agent",
            CommandSpec::new("custom-agent", ["--serve"]),
            ToolKind::Agent,
        ))
        .expect("unique tool");

    let i18n = StaticI18n::new();
    let commands = detect_catalog_commands(&catalog, &NoExecutables, &i18n, Locale::En);

    assert!(
        commands
            .iter()
            .any(|cmd| cmd.label == "custom-agent [not found]"
                && !cmd.available
                && matches!(cmd.launch, Some(PaneLaunchRef::Tool { .. }))),
        "unavailable configured tools must remain visible and marked as missing"
    );
    assert!(
        commands.iter().any(CatalogCommand::is_custom_sentinel),
        "custom command sentinel should still be present"
    );
}

// -----------------------------------------------------------------
// Cursor Agent wizard-list invariants
//
// `cursor-agent` is the canonical Cursor Agent tool identity;
// `agent` ships as an alias executable but never as a separate
// catalog identity. The wizard command-selection list must
// therefore expose Cursor Agent under exactly one entry —
// regardless of which executable happens to be on PATH — and
// never as two parallel options.
// -----------------------------------------------------------------

/// Tiny [`ExecutableAvailabilityPort`] stub: returns `true` only
/// for the explicit allow-list. Used by the Cursor Agent wizard
/// tests below so we can run the real environment-filtered
/// catalog path that production uses (`global_tool_catalog_config_for_environment`).
struct AllowedExecutables(&'static [&'static str]);

impl ExecutableAvailabilityPort for AllowedExecutables {
    fn is_available(&self, program: &str) -> bool {
        self.0.contains(&program)
    }
}

fn cursor_agent_options(commands: &[CatalogCommand]) -> Vec<&CatalogCommand> {
    commands
        .iter()
        .filter(|cmd| match &cmd.launch {
            Some(PaneLaunchRef::Tool { name }) => name.as_str() == "cursor-agent",
            _ => false,
        })
        .collect()
}

/// When the canonical `cursor-agent` executable is on PATH, the
/// wizard command-selection list must contain exactly one
/// Cursor Agent option that resolves to the canonical tool
/// name. There must be no parallel `agent`-keyed entry.
#[test]
fn wizard_command_list_has_exactly_one_cursor_agent_option_when_canonical_executable_present() {
    let exec = AllowedExecutables(&["cursor-agent"]);
    let catalog =
        workhive_core::usecases::layout_template::global_tool_catalog_config_for_environment(&exec)
            .catalog;
    let i18n = StaticI18n::new();
    let commands = detect_catalog_commands(&catalog, &exec, &i18n, Locale::En);

    let cursor_agent_entries = cursor_agent_options(&commands);
    assert_eq!(
        cursor_agent_entries.len(),
        1,
        "exactly one Cursor Agent option must appear when `cursor-agent` is \
             available, found {}: {:?}",
        cursor_agent_entries.len(),
        commands.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
    assert!(
        cursor_agent_entries[0].available,
        "Cursor Agent option must be marked available when `cursor-agent` is \
             on PATH"
    );
    assert!(
        !commands.iter().any(|cmd| matches!(
            &cmd.launch,
            Some(PaneLaunchRef::Tool { name }) if name.as_str() == "agent"
        )),
        "wizard must never expose a separate `agent`-keyed Cursor Agent option"
    );
}

/// When only the `agent` alias executable is on PATH, the
/// environment-filtered global catalog must still expose Cursor
/// Agent as exactly one canonical entry (keyed `cursor-agent`,
/// with `command.program` rewritten to the available alias).
/// The wizard command-selection list must surface it as a
/// single option — not as a second parallel entry.
#[test]
fn wizard_command_list_has_exactly_one_canonical_cursor_agent_option_when_only_agent_alias_present()
{
    let exec = AllowedExecutables(&["agent"]);
    let filtered =
        workhive_core::usecases::layout_template::global_tool_catalog_config_for_environment(&exec)
            .catalog;

    let cursor_agent_entry = filtered
        .find(&ToolName::new("cursor-agent").expect("canonical name"))
        .expect("filtered catalog must keep Cursor Agent under its canonical name");
    assert_eq!(
        cursor_agent_entry.command.program(),
        "agent",
        "alias fallback must rewrite command.program to the available alias \
             while preserving the canonical tool identity"
    );
    assert!(
        filtered
            .find(&ToolName::new("agent").expect("name is syntactically valid"))
            .is_none(),
        "filtered catalog must not invent a separate `agent`-keyed tool identity"
    );

    let i18n = StaticI18n::new();
    let commands = detect_catalog_commands(&filtered, &exec, &i18n, Locale::En);

    let cursor_agent_entries = cursor_agent_options(&commands);
    assert_eq!(
        cursor_agent_entries.len(),
        1,
        "exactly one canonical Cursor Agent option must appear when only the \
             `agent` alias is on PATH, found {}: {:?}",
        cursor_agent_entries.len(),
        commands.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
    assert!(
        cursor_agent_entries[0].available,
        "Cursor Agent option must be marked available when its alias is on PATH"
    );
}

/// Even when both `cursor-agent` and `agent` happen to be on
/// PATH (the typical install — `agent` is a symlink to
/// `cursor-agent`), the wizard must show Cursor Agent only
/// once. There must be no double entry.
#[test]
fn wizard_command_list_never_shows_cursor_agent_and_agent_as_separate_options() {
    let exec = AllowedExecutables(&["cursor-agent", "agent"]);
    let catalog =
        workhive_core::usecases::layout_template::global_tool_catalog_config_for_environment(&exec)
            .catalog;
    let i18n = StaticI18n::new();
    let commands = detect_catalog_commands(&catalog, &exec, &i18n, Locale::En);

    let cursor_agent_entries = cursor_agent_options(&commands);
    assert_eq!(
        cursor_agent_entries.len(),
        1,
        "Cursor Agent must appear exactly once even when both `cursor-agent` \
             and `agent` are on PATH, found {}: {:?}",
        cursor_agent_entries.len(),
        commands.iter().map(|c| &c.label).collect::<Vec<_>>()
    );
    assert!(
        !commands.iter().any(|cmd| matches!(
            &cmd.launch,
            Some(PaneLaunchRef::Tool { name }) if name.as_str() == "agent"
        )),
        "no parallel `agent`-keyed entry is allowed in the wizard list"
    );
}

/// EN / zh-CN parity for trusted-root onboarding keys.
#[test]
fn onboarding_keys_have_en_and_zh_cn_localizations() {
    let i18n = StaticI18n::new();
    for key in [k::TRUSTED_ROOTS_CONFIRM_ASK, k::TRUSTED_ROOTS_CONFIRM_ADDED] {
        let en = i18n.resolve(Locale::En, key);
        let zh = i18n.resolve(Locale::ZhCn, key);
        assert!(
            !en.is_empty() && en != key,
            "EN translation missing for {key}"
        );
        assert!(
            !zh.is_empty() && zh != key,
            "zh-CN translation missing for {key}"
        );
    }
}

// ── Pane-command per-pick echo contract ─────────────────────────
//
// The init wizard asks pane commands one position at a time
// (`C1R1`, `C1R2`, `C2R1`, …). The contract verified here:
//
//   1. Each pane prints a per-pane prompt before its picker
//      runs, formatted as `  <prompt> [<current_display>]`.
//   2. After the picker confirms, an echo line is printed in
//      the form `    <pos> → <display>`.
//   3. The echo for pane N appears in the transcript before the
//      prompt for pane N+1 — never as a batched summary at the
//      end of the step.
//
// We exercise the pure helpers `pane_command_prompt_line` and
// `pane_command_echo_line` (and assert their relative order in
// a simulated transcript), which is the same mapping the
// wizard uses around the selector call.

#[test]
fn pane_prompt_line_uses_pos_and_current_display() {
    let i18n = StaticI18n::new();
    let line = pane_command_prompt_line("C1R1", &PaneLaunchRef::EmptyShell, &i18n, Locale::En);
    assert!(
        line.starts_with("  "),
        "prompt line must keep two-space indent: {line:?}"
    );
    assert!(
        line.contains("C1R1"),
        "prompt line must reference the position: {line:?}"
    );
    assert!(
        line.contains('[') && line.contains(']'),
        "prompt line must show current default in brackets: {line:?}"
    );
}

#[test]
fn pane_echo_line_uses_arrow_and_display_name() {
    let i18n = StaticI18n::new();
    let echo = pane_command_echo_line("C1R1", &PaneLaunchRef::EmptyShell, &i18n, Locale::En);
    assert!(
        echo.starts_with("    "),
        "echo line must keep four-space indent: {echo:?}"
    );
    assert!(
        echo.contains("C1R1"),
        "echo line must reference the position: {echo:?}"
    );
    assert!(
        echo.contains('→'),
        "echo line must use an arrow separator: {echo:?}"
    );
}

#[test]
fn pane_echo_for_each_pane_appears_before_next_pane_prompt() {
    // Simulate the per-pane interaction for three panes in
    // a 2-column layout: C1R1 → C1R2 → C2R1. Build the
    // expected transcript by composing prompt+echo for each
    // pane in order, then assert that for every adjacent
    // pair, the echo for pane N precedes the prompt for
    // pane N+1.
    let i18n = StaticI18n::new();
    let positions = ["C1R1", "C1R2", "C2R1"];
    let mut transcript: Vec<String> = Vec::new();
    for pos in positions {
        transcript.push(pane_command_prompt_line(
            pos,
            &PaneLaunchRef::EmptyShell,
            &i18n,
            Locale::En,
        ));
        transcript.push(pane_command_echo_line(
            pos,
            &PaneLaunchRef::EmptyShell,
            &i18n,
            Locale::En,
        ));
    }

    let find_index = |needle: &str, after: usize| -> usize {
        transcript
            .iter()
            .enumerate()
            .skip(after)
            .find(|(_, line)| line.contains(needle))
            .map(|(idx, _)| idx)
            .expect("expected line in transcript")
    };

    let mut search_from = 0usize;
    for win in positions.windows(2) {
        let echo_marker = format!("{} →", win[0]);
        let next_prompt_marker = win[1];
        let echo_idx = find_index(&echo_marker, search_from);
        let prompt_idx = find_index(next_prompt_marker, echo_idx + 1);
        assert!(
            echo_idx < prompt_idx,
            "echo for {} must appear before prompt for {}: echo@{echo_idx}, prompt@{prompt_idx}",
            win[0],
            win[1]
        );
        search_from = prompt_idx;
    }
}

// -----------------------------------------------------------------
// Cursor Agent display-label normalization
//
// The wizard renders pane command options through
// `launch_display_name`, which routes built-in tool identities
// through `TOOL_LABEL_KEYS` so each AI agent shares the same
// category suffix style. Cursor Agent must follow the same
// `<Name> (AI Agent)` / `<Name> (AI 助手)` shape as Claude Code,
// Codex, and OpenCode in both locales. The saved tool identity
// stays `cursor-agent`; only the user-visible label changes.
// -----------------------------------------------------------------

#[test]
fn cursor_agent_uses_normalized_ai_agent_label_in_en_and_zh() {
    let i18n = StaticI18n::new();
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("cursor-agent").expect("canonical name"),
    };

    let en = launch_display_name(&launch, &i18n, Locale::En);
    assert_eq!(
        en, "Cursor Agent (AI Agent)",
        "Cursor Agent EN label must match other built-in AI agents"
    );

    let zh = launch_display_name(&launch, &i18n, Locale::ZhCn);
    assert_eq!(
        zh, "Cursor Agent (AI 助手)",
        "Cursor Agent zh-CN label must match other built-in AI agents"
    );
}

#[test]
fn cursor_agent_echo_line_uses_normalized_label() {
    let i18n = StaticI18n::new();
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("cursor-agent").expect("canonical name"),
    };
    let echo_zh = pane_command_echo_line("C1R1", &launch, &i18n, Locale::ZhCn);
    assert!(
        echo_zh.contains("Cursor Agent (AI 助手)"),
        "selection echo must surface the normalized Cursor Agent label: {echo_zh:?}"
    );
}

#[test]
fn built_in_ai_agent_labels_share_the_same_category_suffix_shape() {
    // All four built-in AI agents must share the same trailing
    // category suffix so the selector list reads as a single
    // semantic group without explicit grouping. This guards
    // against regressing one agent's label and re-introducing
    // visual ambiguity.
    let i18n = StaticI18n::new();
    for name in ["claude", "codex", "opencode", "cursor-agent"] {
        let launch = PaneLaunchRef::Tool {
            name: ToolName::new(name).expect("canonical name"),
        };
        let en = launch_display_name(&launch, &i18n, Locale::En);
        assert!(
            en.ends_with("(AI Agent)"),
            "built-in AI agent EN label must end with `(AI Agent)`: {en:?}"
        );
        let zh = launch_display_name(&launch, &i18n, Locale::ZhCn);
        assert!(
            zh.ends_with("(AI 助手)"),
            "built-in AI agent zh-CN label must end with `(AI 助手)`: {zh:?}"
        );
    }
}

// ── Pane-command context echo (pre-prompt layout summary) ───────

/// Two-column fixture: C1 has an AI tool + an empty shell, C2 has
/// one empty shell — three panes total.
fn summary_fixture_columns() -> Vec<ColumnConfig> {
    vec![
        ColumnConfig {
            width: 50,
            panes: vec![
                PaneConfig {
                    height: 50,
                    launch: PaneLaunchRef::Tool {
                        name: ToolName::new("claude").expect("canonical name"),
                    },
                },
                PaneConfig {
                    height: 50,
                    launch: PaneLaunchRef::EmptyShell,
                },
            ],
        },
        ColumnConfig {
            width: 50,
            panes: vec![PaneConfig {
                height: 100,
                launch: PaneLaunchRef::EmptyShell,
            }],
        },
    ]
}

#[test]
fn pane_defaults_summary_lists_layout_total_and_each_pane_in_order() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let lines =
        pane_defaults_summary_lines(&LayoutTemplate::AiCodingLeft, &columns, &i18n, Locale::En);

    assert_eq!(lines.len(), 4, "header + one line per pane: {lines:?}");
    let header = &lines[0];
    let template_label = i18n.resolve(Locale::En, k::INIT_LAYOUT_AI_LEFT);
    assert!(
        header.contains(&template_label),
        "header must echo the selected layout label: {header}"
    );
    assert!(
        header.contains('3'),
        "header must carry the total pane count: {header}"
    );

    // Pane lines appear in column-major position order and show the
    // current launch (preset label) or the empty-shell default.
    assert!(lines[1].contains("C1R1") && lines[1].contains("Claude Code"));
    assert!(lines[2].contains("C1R2") && lines[2].contains("(empty shell)"));
    assert!(lines[3].contains("C2R1") && lines[3].contains("(empty shell)"));
}

#[test]
fn pane_defaults_summary_uses_zh_cn_copy() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let lines =
        pane_defaults_summary_lines(&LayoutTemplate::AiCodingLeft, &columns, &i18n, Locale::ZhCn);

    let header = &lines[0];
    assert!(
        header.contains("布局") && header.contains("窗格"),
        "zh-CN header must use localized copy: {header}"
    );
    assert!(
        lines[2].contains("（空终端）"),
        "zh-CN empty-shell default must be localized: {}",
        lines[2]
    );
}

#[test]
fn pane_command_selector_model_prepends_disabled_context_rows() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let commands = vec![
        "Claude Code (AI Agent)".to_owned(),
        "Empty shell".to_owned(),
    ];
    let model = pane_command_selector_model(
        &LayoutTemplate::AiCodingLeft,
        &columns,
        "C1R1",
        &commands,
        0,
        &i18n,
        Locale::En,
    );

    assert_eq!(model.title, "C1R1 startup command");
    assert_eq!(
        model.command_start_row, 2,
        "layout and pane summary rows must precede command choices"
    );
    assert_eq!(
        &model.disabled[..model.command_start_row],
        &[true, true],
        "context rows must be non-selectable"
    );
    assert!(
        model.items[0].contains("Layout:") && model.items[0].contains("3 pane"),
        "first context row must carry layout and pane count: {:?}",
        model.items
    );
    assert!(
        model.items[1].contains("C1R1: Claude Code")
            && model.items[1].contains("C1R2: (empty shell)")
            && model.items[1].contains("C2R1: (empty shell)"),
        "second context row must summarize pane defaults: {:?}",
        model.items
    );
}

#[test]
fn pane_command_selector_model_offsets_default_and_selection_mapping() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let commands = vec![
        "Claude Code (AI Agent)".to_owned(),
        "Empty shell".to_owned(),
        "Custom command".to_owned(),
    ];
    let model = pane_command_selector_model(
        &LayoutTemplate::AiCodingLeft,
        &columns,
        "C1R2",
        &commands,
        1,
        &i18n,
        Locale::En,
    );

    assert_eq!(
        model.default_row, 3,
        "default command index must be offset by context rows"
    );
    assert_eq!(
        model.command_index_for_row(2).expect("first command row"),
        0
    );
    assert_eq!(
        model.command_index_for_row(4).expect("third command row"),
        2
    );
    assert!(
        model.command_index_for_row(1).is_err(),
        "context rows must not map to command indexes"
    );
}

#[test]
fn pane_command_selector_model_uses_zh_cn_context_copy() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let commands = vec!["空终端（无启动命令）".to_owned()];
    let model = pane_command_selector_model(
        &LayoutTemplate::AiCodingLeft,
        &columns,
        "C1R1",
        &commands,
        0,
        &i18n,
        Locale::ZhCn,
    );

    assert_eq!(model.title, "C1R1 启动命令");
    assert!(
        model.items[0].contains("布局：") && model.items[0].contains("共 3 个窗格"),
        "zh-CN layout context must use localized copy: {:?}",
        model.items
    );
    assert!(
        model.items[1].contains("窗格：") && model.items[1].contains("C1R2: （空终端）"),
        "zh-CN pane summary must use localized copy: {:?}",
        model.items
    );
}

// ── Custom-layout running context echo ──────────────────────────

#[test]
fn custom_layout_progress_reports_configured_pending_and_total() {
    let i18n = StaticI18n::new();
    let line = custom_layout_progress_line(&[2, 3], 3, &i18n, Locale::En);
    assert!(
        line.contains("2/3"),
        "must show configured vs total columns: {line}"
    );
    assert!(
        line.contains("C1=2, C2=3"),
        "must list per-column pane counts: {line}"
    );
    assert!(
        line.contains("C3=pending"),
        "must mark unanswered columns with an explicit pending marker: {line}"
    );
    assert!(
        line.contains(": 5"),
        "must carry the running pane total: {line}"
    );
}

#[test]
fn custom_layout_progress_omits_pending_marker_when_all_columns_configured() {
    let i18n = StaticI18n::new();
    let line = custom_layout_progress_line(&[2, 3, 1], 3, &i18n, Locale::En);
    assert!(
        line.contains("C1=2, C2=3, C3=1"),
        "fully configured layout must list every column's count: {line}"
    );
    assert!(
        !line.contains("pending"),
        "no pending marker may remain once every column is answered: {line}"
    );
}

#[test]
fn custom_layout_progress_uses_zh_cn_copy() {
    let i18n = StaticI18n::new();
    let line = custom_layout_progress_line(&[2, 3], 3, &i18n, Locale::ZhCn);
    assert!(
        line.contains("已配置列") && line.contains("2/3"),
        "zh-CN progress line must use localized copy: {line}"
    );
    assert!(
        line.contains("C3=待配置"),
        "zh-CN progress line must mark pending columns explicitly: {line}"
    );
    assert!(
        line.contains("窗格总数") && line.contains("5"),
        "zh-CN progress line must carry the running pane total: {line}"
    );
}

// ── Live layout preview model (pane command selector) ───────────

fn tool_command(name: &str, display_label: &str) -> CatalogCommand {
    CatalogCommand {
        label: display_label.to_owned(),
        available: true,
        launch: Some(PaneLaunchRef::Tool {
            name: ToolName::new(name).expect("canonical name"),
        }),
    }
}

fn custom_command(label: &str) -> CatalogCommand {
    CatalogCommand {
        label: label.to_owned(),
        available: true,
        launch: None,
    }
}

#[test]
fn preview_marks_only_the_current_pane_with_the_active_marker() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let candidate = pane_preview_candidate(&tool_command("codex", "Codex (AI Agent)"));
    let labels = pane_command_preview_labels(&columns, 0, 1, &candidate, &i18n, Locale::En);

    assert!(
        labels[0][1].starts_with(PREVIEW_CURRENT_PANE_MARKER),
        "the pane being configured must carry the active marker: {:?}",
        labels[0][1]
    );
    assert!(
        !labels[0][0].starts_with(PREVIEW_CURRENT_PANE_MARKER)
            && !labels[1][0].starts_with(PREVIEW_CURRENT_PANE_MARKER),
        "no other pane may carry the active marker: {labels:?}"
    );
}

#[test]
fn preview_shows_candidate_for_current_pane_and_preserves_others() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    // Configure C1R2 (currently an empty shell) with a Codex candidate.
    let candidate = pane_preview_candidate(&tool_command("codex", "Codex (AI Agent)"));
    let labels = pane_command_preview_labels(&columns, 0, 1, &candidate, &i18n, Locale::En);

    assert!(
        labels[0][1].contains("C1R2") && labels[0][1].contains("Codex"),
        "current pane must preview the highlighted candidate launch: {:?}",
        labels[0][1]
    );
    // Other panes keep their saved launch labels untouched.
    assert!(
        labels[0][0].contains("C1R1") && labels[0][0].contains("Claude Code"),
        "other panes must keep their saved launch: {:?}",
        labels[0][0]
    );
    assert!(
        labels[1][0].contains("C2R1") && labels[1][0].contains("Shell"),
        "other panes must keep their saved empty-shell label: {:?}",
        labels[1][0]
    );
}

#[test]
fn preview_labels_custom_sentinel_as_pending_input_before_entry() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let candidate = pane_preview_candidate(&custom_command("Custom command (manual input)"));
    let placeholder = i18n.resolve(Locale::En, k::INIT_PANE_PREVIEW_CUSTOM);
    let labels = pane_command_preview_labels(&columns, 0, 0, &candidate, &i18n, Locale::En);

    assert!(
        labels[0][0].contains("C1R1") && labels[0][0].contains(&placeholder),
        "custom sentinel must preview a manual-input placeholder: {:?}",
        labels[0][0]
    );
}

#[test]
fn preview_custom_placeholder_uses_zh_cn_copy() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let candidate = pane_preview_candidate(&custom_command("自定义命令（手动输入）"));
    let placeholder = i18n.resolve(Locale::ZhCn, k::INIT_PANE_PREVIEW_CUSTOM);
    let labels = pane_command_preview_labels(&columns, 0, 0, &candidate, &i18n, Locale::ZhCn);

    assert!(
        labels[0][0].contains(&placeholder),
        "zh-CN custom placeholder must be localized: {:?}",
        labels[0][0]
    );
}

#[test]
fn preview_lines_render_a_bordered_diagram_for_the_candidate() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let candidate = pane_preview_candidate(&tool_command("codex", "Codex (AI Agent)"));
    let lines = pane_command_preview_lines(&columns, 0, 1, &candidate, &i18n, Locale::En);

    assert!(!lines.is_empty(), "preview must produce diagram lines");
    assert!(
        lines.first().is_some_and(|l| l.contains('┌')),
        "preview must reuse the box-drawing diagram top border: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains(PREVIEW_CURRENT_PANE_MARKER)),
        "preview diagram must mark the pane being configured: {lines:?}"
    );
}

#[test]
fn preview_min_width_covers_widest_candidate_label() {
    let i18n = StaticI18n::new();
    let columns = summary_fixture_columns();
    let available = vec![
        tool_command("codex", "Codex (AI Agent)"),
        custom_command("Custom command (manual input)"),
    ];
    let min_width = pane_command_preview_min_width(&columns, 0, 1, &available, &i18n, Locale::En);

    let widest = available
        .iter()
        .map(|cmd| {
            let candidate = pane_preview_candidate(cmd);
            pane_command_preview_lines(&columns, 0, 1, &candidate, &i18n, Locale::En)
                .iter()
                .map(|line| display_width(line))
                .max()
                .unwrap_or(0)
        })
        .max()
        .unwrap_or(0);

    assert_eq!(
        min_width,
        u16::try_from(widest + 2).unwrap(),
        "preview width must fit the widest candidate plus border columns"
    );
}

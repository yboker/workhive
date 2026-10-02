#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Build the compact open-summary lines.
///
/// Pure function with no I/O, so it can be unit-tested without `eprintln!`
/// capture. Returns the ordered lines to render after `OPEN_SUCCESS`.
///
/// The compact policy:
/// - no leading blank line (the caller already printed `OPEN_SUCCESS`);
/// - one line each for project / worktree / session / outcome;
/// - layout-applied is intentionally omitted here because progress
///   already emitted `OPEN_LAYOUT_APPLIED` during layout application;
/// - degraded-launches header + bullets only when present;
/// - attach-skipped line only when `no_attach`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_open_summary_lines(
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
    was_auto_initialized: bool,
    provenance: &workhive_core::domain::WorkspaceProvenance,
    outcome: &usecases::open::OpenOutcome,
    session_name: &str,
    degraded_launches: &[String],
    no_attach: bool,
) -> Vec<String> {
    use workhive_core::domain::{MaterializationKind, WorkspaceProvenance};
    use workhive_core::i18n_keys as k;

    let project_line = if was_auto_initialized {
        i18n.resolve(locale, k::OPEN_SUMMARY_PROJECT_INIT)
    } else {
        i18n.resolve(locale, k::OPEN_SUMMARY_PROJECT_EXISTING)
    };

    let worktree_line = match provenance {
        WorkspaceProvenance::ProjectRoot => i18n.resolve(locale, k::OPEN_SUMMARY_WORKTREE_MAIN),
        WorkspaceProvenance::Provisioned {
            materialization, ..
        } => match materialization {
            MaterializationKind::ReusedExisting => {
                i18n.resolve(locale, k::OPEN_SUMMARY_WORKTREE_REUSED)
            }
            MaterializationKind::FromExistingReference => {
                i18n.resolve(locale, k::OPEN_SUMMARY_WORKTREE_ATTACHED)
            }
            MaterializationKind::FromNewReference => {
                i18n.resolve(locale, k::OPEN_SUMMARY_WORKTREE_CREATED)
            }
        },
    };

    let session_line = if outcome.is_session_reused() {
        i18n.resolve_with_args(locale, k::OPEN_SUMMARY_SESSION_REUSED, &[session_name])
    } else {
        i18n.resolve_with_args(locale, k::OPEN_SUMMARY_SESSION_CREATED, &[session_name])
    };

    let outcome_line = match outcome {
        usecases::open::OpenOutcome::ExistingSessionReused => {
            i18n.resolve(locale, k::OPEN_OUTCOME_SESSION_REUSED)
        }
        usecases::open::OpenOutcome::LayoutApplied => {
            i18n.resolve(locale, k::OPEN_OUTCOME_LAYOUT_APPLIED)
        }
        usecases::open::OpenOutcome::ForceRecreated => {
            i18n.resolve(locale, k::OPEN_OUTCOME_LAYOUT_APPLIED)
        }
    };

    let mut lines = Vec::with_capacity(6 + degraded_launches.len());
    lines.push(project_line);
    lines.push(worktree_line);
    lines.push(session_line);
    lines.push(outcome_line);

    if !degraded_launches.is_empty() {
        lines.push(i18n.resolve(locale, k::OPEN_SUMMARY_DEGRADED_LAUNCHES_HEADER));
        for entry in degraded_launches {
            lines.push(format!("  - {entry}"));
        }
    }

    if no_attach {
        lines.push(i18n.resolve(locale, k::OPEN_SUMMARY_ATTACH_SKIPPED));
    }

    lines
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn print_open_summary(
    ctx: &CommandContext,
    locale: Locale,
    was_auto_initialized: bool,
    provenance: &workhive_core::domain::WorkspaceProvenance,
    outcome: &usecases::open::OpenOutcome,
    session_name: &str,
    degraded_launches: &[String],
    no_attach: bool,
) {
    let lines = build_open_summary_lines(
        &ctx.i18n,
        locale,
        was_auto_initialized,
        provenance,
        outcome,
        session_name,
        degraded_launches,
        no_attach,
    );

    for line in lines {
        eprintln!("{line}");
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    // ---------- Compact open-summary renderer tests ----------
    //
    // These tests exercise `build_open_summary_lines` (the pure renderer
    // behind `print_open_summary`). They lock in the compact-output policy:
    //
    // 1. No unconditional leading blank line.
    // 2. Layout-applied is NOT repeated in the summary because progress
    //    already emitted `OPEN_LAYOUT_APPLIED` during layout application.
    // 3. Degraded launches, attach-skipped, and warning lines are still
    //    surfaced verbatim.

    use workhive_core::domain::WorkspaceProvenance;

    #[test]
    fn no_attach_summary_message_renders_in_both_locales() {
        use workhive_core::i18n_keys as ck;
        use workhive_core::ports::I18nPort;
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let session = "wh@inst@proj@feat-test";

        let en =
            i18n.resolve_with_args(Locale::En, ck::OPEN_PROGRESS_NO_ATTACH_SUMMARY, &[session]);
        assert!(
            en.contains(session),
            "EN no-attach summary must include the session name: {en}"
        );
        assert!(
            en.to_lowercase().contains("--no-attach"),
            "EN no-attach summary must mention `--no-attach`: {en}"
        );

        let zh = i18n.resolve_with_args(
            Locale::ZhCn,
            ck::OPEN_PROGRESS_NO_ATTACH_SUMMARY,
            &[session],
        );
        assert!(
            zh.contains(session),
            "zh-CN no-attach summary must include the session name: {zh}"
        );
        assert!(
            zh.contains("--no-attach"),
            "zh-CN no-attach summary must reference `--no-attach`: {zh}"
        );
    }

    #[test]
    fn open_summary_compact_does_not_start_with_blank_line() {
        let i18n = FakeI18n;
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::ForceRecreated,
            "wh-test",
            &[],
            false,
        );
        assert!(!lines.is_empty(), "summary must contain content");
        assert!(
            !lines.first().expect("first line").is_empty(),
            "summary must not start with an empty separator line"
        );
        for line in &lines {
            assert!(
                !line.is_empty(),
                "compact summary must not contain interior blank lines: {lines:?}"
            );
        }
    }

    #[test]
    fn open_summary_does_not_repeat_layout_applied_message() {
        // Progress already emitted OPEN_LAYOUT_APPLIED; the summary must
        // not repeat it as OPEN_SUMMARY_LAYOUT_APPLIED.
        let i18n = FakeI18n;
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &[],
            false,
        );
        for line in &lines {
            assert!(
                !line.contains(workhive_core::i18n_keys::OPEN_SUMMARY_LAYOUT_APPLIED),
                "OPEN_SUMMARY_LAYOUT_APPLIED must not appear in compact summary: {line}"
            );
        }
    }

    #[test]
    fn open_summary_retains_degraded_launches_block() {
        let i18n = FakeI18n;
        let degraded = vec!["col=0 row=0: fallback to shell".to_owned()];
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &degraded,
            false,
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains(workhive_core::i18n_keys::OPEN_SUMMARY_DEGRADED_LAUNCHES_HEADER),
            "degraded header must remain visible: {joined}"
        );
        assert!(
            joined.contains("  - col=0 row=0: fallback to shell"),
            "degraded bullet must remain visible: {joined}"
        );
    }

    #[test]
    fn open_summary_keeps_attach_skipped_notice_when_no_attach() {
        let i18n = FakeI18n;
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &[],
            true,
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains(workhive_core::i18n_keys::OPEN_SUMMARY_ATTACH_SKIPPED)),
            "no-attach summary must explain that attach was skipped: {lines:?}"
        );
    }

    /// A selected provider whose binary is missing must reach the open
    /// summary with the tool name and program, routed through the existing
    /// `open.summary.degraded_launches_header` path, in both locales. The
    /// degraded entry mirrors what `cmd_open` builds from a resolved pane's
    /// `degradation_reason()`.
    #[test]
    fn open_summary_surfaces_missing_provider_name_and_program_in_both_locales() {
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let missing = workhive_core::domain::ResolvedPaneLaunch::ExecutableNotFound {
            name: ToolName::new("claude").expect("tool name"),
            program: "claude".to_owned(),
        };
        let reason = missing
            .degradation_reason()
            .expect("missing executable must have a degradation reason");
        let degraded = vec![format!("col=0 row=0: {reason}")];

        for locale in [Locale::En, Locale::ZhCn] {
            let lines = build_open_summary_lines(
                &i18n,
                locale,
                false,
                &WorkspaceProvenance::ProjectRoot,
                &usecases::open::OpenOutcome::LayoutApplied,
                "wh-test",
                &degraded,
                false,
            );
            let joined = lines.join("\n");
            assert!(
                joined.contains("claude"),
                "{locale:?} summary must name the missing provider: {joined}"
            );
            assert!(
                joined.contains("not found on PATH"),
                "{locale:?} summary must explain the provider is unavailable: {joined}"
            );
        }
    }

    /// A shell-only open (no degraded launches) must not render the degraded
    /// launches block. This proves absent AI providers do not add a
    /// degraded/blocking message to a plain shell workspace summary.
    #[test]
    fn open_summary_has_no_degraded_block_for_shell_only() {
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            true,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &[],
            true,
        );
        let joined = lines.join("\n");
        assert!(
            !joined.contains(&i18n.resolve(
                Locale::En,
                workhive_core::i18n_keys::OPEN_SUMMARY_DEGRADED_LAUNCHES_HEADER
            )),
            "shell-only summary must not render the degraded launches block: {joined}"
        );
    }

    #[test]
    fn open_summary_includes_session_and_outcome_lines() {
        let i18n = FakeI18n;
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            true,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::ForceRecreated,
            "my-session",
            &[],
            false,
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains(workhive_core::i18n_keys::OPEN_SUMMARY_PROJECT_INIT),
            "auto-init project line must be present: {joined}"
        );
        assert!(
            joined.contains(workhive_core::i18n_keys::OPEN_SUMMARY_WORKTREE_MAIN),
            "main-root worktree line must be present: {joined}"
        );
        assert!(
            joined.contains(workhive_core::i18n_keys::OPEN_SUMMARY_SESSION_CREATED),
            "session-created line must be present: {joined}"
        );
        assert!(
            joined.contains(workhive_core::i18n_keys::OPEN_OUTCOME_LAYOUT_APPLIED),
            "force-recreate outcome line must be present: {joined}"
        );
    }

    #[test]
    fn open_summary_project_root_wording_en_is_truthful() {
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let lines = build_open_summary_lines(
            &i18n,
            Locale::En,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &[],
            false,
        );
        let joined = lines.join("\n");
        assert!(
            !joined.contains("main branch"),
            "EN summary must not say main branch for project root: {joined}"
        );
        assert!(
            joined.contains("project root (current checked-out branch)"),
            "EN summary must use truthful project-root wording: {joined}"
        );
    }

    #[test]
    fn open_summary_project_root_wording_zh_is_truthful() {
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let lines = build_open_summary_lines(
            &i18n,
            Locale::ZhCn,
            false,
            &WorkspaceProvenance::ProjectRoot,
            &usecases::open::OpenOutcome::LayoutApplied,
            "wh-test",
            &[],
            false,
        );
        let joined = lines.join("\n");
        assert!(
            !joined.contains("\u{4E3B}\u{5206}\u{652F}"),
            "ZH summary must not say main branch for project root: {joined}"
        );
        assert!(
            joined.contains("\u{9879}\u{76EE}\u{6839}\u{76EE}\u{5F55}\u{FF08}\u{5F53}\u{524D}\u{68C0}\u{51FA}\u{5206}\u{652F}\u{FF09}"),
            "ZH summary must use truthful project-root wording: {joined}"
        );
    }
}

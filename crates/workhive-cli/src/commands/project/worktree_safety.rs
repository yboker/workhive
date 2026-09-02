#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

/// Render a `SessionLookupFailed` into a localized user-facing string.
///
/// Used both by `render_open_error` (primary path) and `main()` (fallback
/// for any `CliError::SessionLookupFailed` that escaped unrendered).
pub(crate) fn render_session_lookup_error(
    ctx: &CommandContext,
    session: &str,
    availability: &workhive_core::domain::SessionAvailability,
) -> String {
    use workhive_core::domain::SessionAvailability;
    use workhive_core::i18n_keys as k;

    let locale = ctx.locale();
    let state_text = match availability {
        SessionAvailability::Running => ctx.i18n.resolve(locale, k::ERROR_SESSION_LOOKUP_RUNNING),
        SessionAvailability::Missing => ctx.i18n.resolve(locale, k::ERROR_SESSION_LOOKUP_MISSING),
        SessionAvailability::ServerUnreachable => ctx
            .i18n
            .resolve(locale, k::ERROR_SESSION_LOOKUP_UNREACHABLE),
        SessionAvailability::LookupFailed => {
            ctx.i18n.resolve(locale, k::ERROR_SESSION_LOOKUP_FAILED)
        }
    };
    ctx.i18n.resolve_with_args(
        locale,
        k::ERROR_SESSION_LOOKUP_FAILED_MSG,
        &[session, &state_text],
    )
}

pub(crate) fn infer_base_branch(
    worktrees: &[workhive_core::ports::WorktreeInfo],
) -> Option<String> {
    worktrees
        .iter()
        .find(|entry| entry.is_main)
        .and_then(|entry| entry.branch.clone())
}

/// Component-aware "is `path` strictly inside `base`?" check.
///
/// Strict means the base directory itself is *not* considered "inside"
/// the base. String prefix comparisons
/// are not used so lookalike paths such as `.workhive/worktrees-archive/`
/// stay outside the managed base.
pub(crate) fn path_is_under_base(path: &camino::Utf8Path, base: &camino::Utf8Path) -> bool {
    path.strip_prefix(base)
        .is_ok_and(|rel| !rel.as_str().is_empty())
}

pub(crate) fn linked_worktree_scan_targets(
    worktrees: &[workhive_core::ports::WorktreeInfo],
    workhive_worktree_base: &camino::Utf8Path,
) -> Vec<workhive_core::usecases::worktree_safety::WorktreeScanTarget> {
    worktrees
        .iter()
        .filter(|entry| !entry.is_main && entry.path.starts_with(workhive_worktree_base))
        .map(|entry| {
            let workspace = entry
                .branch
                .clone()
                .or_else(|| entry.path.file_name().map(str::to_owned))
                .unwrap_or_else(|| entry.path.to_string());
            workhive_core::usecases::worktree_safety::WorktreeScanTarget {
                workspace,
                branch: entry.branch.clone(),
                path: entry.path.clone(),
            }
        })
        .collect()
}

pub(crate) fn find_worktree(
    worktrees: &[workhive_core::ports::WorktreeInfo],
    selector: &str,
) -> Option<workhive_core::ports::WorktreeInfo> {
    worktrees.iter().find_map(|entry| {
        let matches = entry.branch.as_deref() == Some(selector)
            || entry.path.file_name().is_some_and(|name| name == selector);
        if matches { Some(entry.clone()) } else { None }
    })
}

pub(crate) fn render_worktree_safety_report(
    report: &workhive_core::usecases::worktree_safety::WorktreeSafetyReport,
    ctx: &CommandContext,
    heading_key: &str,
    no_risk_key: &str,
) {
    let locale = ctx.locale();
    let mode = SafetyStyleMode::detect_for_stderr();
    for line in format_worktree_safety_report_lines(
        report,
        &ctx.i18n,
        locale,
        heading_key,
        no_risk_key,
        mode,
    ) {
        eprintln!("{line}");
    }
}

/// Whether to embed terminal style escape sequences in safety-report
/// output. `Plain` is the safe default for non-TTY consumers (CI logs,
/// `tee`, redirections, `NO_COLOR=1`). `Color` is opt-in by detecting
/// an interactive stderr.
///
/// Kept as an enum (not a `bool`) so future modes (256-color,
/// truecolor, accessibility variants) can extend without breaking
/// callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafetyStyleMode {
    Color,
    Plain,
}

impl SafetyStyleMode {
    /// Detect the right mode for stderr output.
    ///
    /// Honors the [`NO_COLOR`](https://no-color.org/) convention. Falls
    /// back to `Plain` when stderr is not a TTY so logs and CI pipelines
    /// stay readable and don't contain raw escape sequences.
    pub(crate) fn detect_for_stderr() -> Self {
        use std::io::IsTerminal;
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::Plain;
        }
        if std::io::stderr().is_terminal() {
            Self::Color
        } else {
            Self::Plain
        }
    }
}

const ANSI_RED_BOLD: &str = "\x1b[1;31m";
const ANSI_YELLOW_BOLD: &str = "\x1b[1;33m";
const ANSI_RESET: &str = "\x1b[0m";

pub(crate) fn stylize(text: &str, code: &str, mode: SafetyStyleMode) -> String {
    match mode {
        SafetyStyleMode::Color => format!("{code}{text}{ANSI_RESET}"),
        SafetyStyleMode::Plain => text.to_owned(),
    }
}

/// Classification of a single safety-report row.
///
/// Drives confirmation display priority: numeric risk facts
/// (`ChangedFileCount`, `UnmergedSummary`) are decision-critical and
/// stay visible at the prompt, while per-file rows (`ChangedFileDetail`,
/// `MoreChangedFiles`) are truncatable when the viewport is short.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SafetyReportLineKind {
    Heading,
    NoRisk,
    Danger,
    WorkspaceTarget,
    ChangedFileCount,
    ChangedFileDetail,
    MoreChangedFiles,
    UnmergedSummary,
    UnmergedExplanation,
}

/// One rendered safety-report row plus its classification and modal
/// severity.
///
/// `severity` is the authoritative, typed source of how the row is
/// emphasized inside the confirmation modal — the danger headline and
/// the decision-critical risk counts are flagged at construction time
/// (where the underlying counts are known) so the modal never has to
/// re-derive danger by matching localized text.
pub(crate) struct SafetyReportEntry {
    pub text: String,
    pub kind: SafetyReportLineKind,
    pub severity: crate::ui::screens::confirm::ConfirmLineSeverity,
}

/// Build the decision-local confirmation context for a destructive
/// worktree removal, reusing the safety-report formatter so the
/// changed-file breakdown shown inside the confirmation screen is
/// byte-for-byte the same localized summary printed to stderr.
///
/// Returns `None` when the report carries no risks: a clean removal
/// needs no extra decision context and keeps the original compact
/// prompt. Rows are rendered in [`SafetyStyleMode::Plain`] because the
/// confirmation modal applies its own styling and must not receive raw
/// ANSI escape sequences. Each row carries a display priority so a
/// short viewport keeps the decision-critical counts visible while
/// per-file detail rows truncate first.
pub(crate) fn worktree_safety_confirm_context(
    report: &workhive_core::usecases::worktree_safety::WorktreeSafetyReport,
    i18n: &dyn I18nPort,
    locale: Locale,
    heading_key: &str,
) -> Option<crate::ui::screens::confirm::ConfirmContext> {
    use crate::ui::screens::confirm::ConfirmContextLine;
    use workhive_core::i18n_keys as k;

    if !report.has_risks() {
        return None;
    }
    let lines = format_worktree_safety_report_entries(
        report,
        i18n,
        locale,
        heading_key,
        k::DESTROY_SAFETY_NO_RISK,
        SafetyStyleMode::Plain,
    )
    .into_iter()
    .map(|entry| ConfirmContextLine {
        text: entry.text,
        priority: confirm_line_priority(entry.kind),
        severity: entry.severity,
    })
    .collect();
    Some(crate::ui::screens::confirm::ConfirmContext {
        lines,
        truncation_notice: i18n.resolve(locale, k::CONFIRM_CONTEXT_TRUNCATED),
        too_small_notice: i18n.resolve(locale, k::CONFIRM_CONTEXT_TOO_SMALL),
    })
}

/// Map a safety-report row kind to its confirmation display priority.
///
/// Numeric risk counts stay visible at the decision point; orienting
/// rows are kept next; per-file detail rows are the first truncated.
fn confirm_line_priority(
    kind: SafetyReportLineKind,
) -> crate::ui::screens::confirm::ConfirmLinePriority {
    use crate::ui::screens::confirm::ConfirmLinePriority;
    match kind {
        SafetyReportLineKind::ChangedFileCount | SafetyReportLineKind::UnmergedSummary => {
            ConfirmLinePriority::Essential
        }
        SafetyReportLineKind::Heading
        | SafetyReportLineKind::NoRisk
        | SafetyReportLineKind::Danger
        | SafetyReportLineKind::WorkspaceTarget
        | SafetyReportLineKind::UnmergedExplanation => ConfirmLinePriority::Important,
        SafetyReportLineKind::ChangedFileDetail | SafetyReportLineKind::MoreChangedFiles => {
            ConfirmLinePriority::Detail
        }
    }
}

pub(crate) fn format_worktree_safety_report_lines(
    report: &workhive_core::usecases::worktree_safety::WorktreeSafetyReport,
    i18n: &dyn I18nPort,
    locale: Locale,
    heading_key: &str,
    no_risk_key: &str,
    mode: SafetyStyleMode,
) -> Vec<String> {
    format_worktree_safety_report_entries(report, i18n, locale, heading_key, no_risk_key, mode)
        .into_iter()
        .map(|entry| entry.text)
        .collect()
}

pub(crate) fn format_worktree_safety_report_entries(
    report: &workhive_core::usecases::worktree_safety::WorktreeSafetyReport,
    i18n: &dyn I18nPort,
    locale: Locale,
    heading_key: &str,
    no_risk_key: &str,
    mode: SafetyStyleMode,
) -> Vec<SafetyReportEntry> {
    use crate::ui::screens::confirm::ConfirmLineSeverity;
    use workhive_core::i18n_keys as k;

    let entry = |text: String, kind: SafetyReportLineKind, severity: ConfirmLineSeverity| {
        SafetyReportEntry {
            text,
            kind,
            severity,
        }
    };

    let mut entries = vec![entry(
        i18n.resolve(locale, heading_key),
        SafetyReportLineKind::Heading,
        ConfirmLineSeverity::Normal,
    )];
    if report.risky_worktrees.is_empty() {
        entries.push(entry(
            format!("  {}", i18n.resolve(locale, no_risk_key)),
            SafetyReportLineKind::NoRisk,
            ConfirmLineSeverity::Normal,
        ));
        return entries;
    }

    // Urgent banner above the per-worktree details so the dirty/unmerged
    // warnings do not read like ordinary informational lines. The
    // wording itself conveys "DANGER" / "危险" so the signal survives
    // even when color is disabled.
    let danger_text = i18n.resolve(locale, k::SAFETY_REPORT_DANGER_HEADLINE);
    entries.push(entry(
        stylize(&danger_text, ANSI_RED_BOLD, mode),
        SafetyReportLineKind::Danger,
        ConfirmLineSeverity::Danger,
    ));

    for risk in &report.risky_worktrees {
        entries.push(entry(
            format!("  - {} [{}]", risk.workspace, risk.path),
            SafetyReportLineKind::WorkspaceTarget,
            ConfirmLineSeverity::Normal,
        ));
        let total_changed_files = risk.total_changed_files.to_string();
        let staged_count = risk.staged_count.to_string();
        let unstaged_count = risk.unstaged_count.to_string();
        let untracked_count = risk.untracked_count.to_string();
        let changed_files_text = i18n.resolve_with_args(
            locale,
            k::SAFETY_REPORT_CHANGED_FILES,
            &[
                &total_changed_files,
                &staged_count,
                &unstaged_count,
                &untracked_count,
            ],
        );
        // Emphasize the dirty-file count only when at least one changed
        // file exists, so a `0 changed files + N unmerged commits`
        // worktree does not get a red "0 changed files" line that
        // misrepresents the risk.
        // The count is a decision-critical risk fact only when changed
        // files exist; a `0 changed files` row (risky purely from
        // unmerged commits) must not be painted red, so its severity and
        // its stderr ANSI emphasis share the same condition.
        let (changed_files_line, changed_files_severity) = if risk.total_changed_files > 0 {
            (
                stylize(&changed_files_text, ANSI_YELLOW_BOLD, mode),
                ConfirmLineSeverity::RiskCount,
            )
        } else {
            (changed_files_text, ConfirmLineSeverity::Normal)
        };
        entries.push(entry(
            format!("    {changed_files_line}"),
            SafetyReportLineKind::ChangedFileCount,
            changed_files_severity,
        ));
        entries.extend(risk.displayed_changed_files.iter().map(|changed| {
            entry(
                format!("      {} {}", changed.status_code, changed.path),
                SafetyReportLineKind::ChangedFileDetail,
                ConfirmLineSeverity::Normal,
            )
        }));
        if risk.hidden_changed_files > 0 {
            let hidden_changed_files = risk.hidden_changed_files.to_string();
            entries.push(entry(
                format!(
                    "      {}",
                    i18n.resolve_with_args(
                        locale,
                        k::SAFETY_REPORT_MORE_CHANGED_FILES,
                        &[&hidden_changed_files],
                    )
                ),
                SafetyReportLineKind::MoreChangedFiles,
                ConfirmLineSeverity::Normal,
            ));
        }
        if let Some(unmerged) = &risk.unmerged_commits {
            let count = unmerged.count.to_string();
            let unmerged_text = i18n.resolve_with_args(
                locale,
                k::SAFETY_REPORT_UNMERGED_COMMITS,
                &[&unmerged.base_branch, &count],
            );
            entries.push(entry(
                format!("    {}", stylize(&unmerged_text, ANSI_YELLOW_BOLD, mode)),
                SafetyReportLineKind::UnmergedSummary,
                ConfirmLineSeverity::RiskCount,
            ));
            // Two distinct explanations of what the unmerged-commit count
            // means:
            //   * `0` changed files + N unmerged commits: state that no
            //     uncommitted file changes exist but the branch still
            //     diverges, so a `0` count does not read as a false
            //     positive.
            //   * otherwise: gloss "unmerged commits against {base}" as
            //     Git history divergence rather than a working-tree diff.
            let explanation = if risk.total_changed_files == 0 {
                i18n.resolve_with_args(
                    locale,
                    k::SAFETY_REPORT_CLEAN_WORKTREE_DIVERGED_BRANCH,
                    &[&count, &unmerged.base_branch],
                )
            } else {
                i18n.resolve_with_args(
                    locale,
                    k::SAFETY_REPORT_UNMERGED_COMMITS_EXPLANATION,
                    &[&count, &unmerged.base_branch],
                )
            };
            entries.push(entry(
                format!("      {explanation}"),
                SafetyReportLineKind::UnmergedExplanation,
                ConfirmLineSeverity::Normal,
            ));
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeInfo};
    #[allow(unused_imports)]
    use workhive_core::usecases::worktree_safety::{
        UnmergedCommitSummary, WorktreeRisk, WorktreeSafetyReport,
    };

    #[test]
    fn scan_targets_excludes_external_linked_worktrees() {
        let workhive_base = camino::Utf8Path::new("/project/.workhive/worktrees");
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/project"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/project/.workhive/worktrees/feat-a"),
                branch: Some("feat-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/external/path/feat-b"),
                branch: Some("feat-b".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];

        let targets = linked_worktree_scan_targets(&worktrees, workhive_base);
        assert_eq!(
            targets.len(),
            1,
            "only Workhive-managed worktree should be included"
        );
        assert_eq!(targets[0].workspace, "feat-a");
    }

    #[test]
    fn find_worktree_matches_external_but_is_not_under_managed_base() {
        let worktrees = vec![
            WorktreeInfo {
                path: Utf8PathBuf::from("/project"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: Utf8PathBuf::from("/external/repos/feat-ext"),
                branch: Some("feat-ext".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];

        let found = find_worktree(&worktrees, "feat-ext");
        assert!(found.is_some(), "find_worktree should match by branch");

        let managed_base = camino::Utf8Path::new("/project/.workhive/worktrees");
        let target = found.unwrap();
        assert!(
            !target.path.starts_with(managed_base),
            "external worktree must not be under Workhive-managed base"
        );
    }

    /// A risky report must yield a confirmation context whose rows are
    /// the exact localized safety-report rows — including the
    /// Simplified Chinese changed-file count — so the decision point
    /// shows the same breakdown printed to stderr without duplicating
    /// count-formatting logic. The changed-file count is tagged
    /// `Essential` and per-file rows `Detail` so a short viewport keeps
    /// the count visible while file rows truncate first.
    #[test]
    fn confirm_context_reuses_localized_safety_report_lines() {
        use crate::ui::screens::confirm::ConfirmLinePriority;

        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "demo01".to_owned(),
                branch: Some("demo01".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/demo01"),
                staged_count: 0,
                unstaged_count: 2,
                untracked_count: 0,
                total_changed_files: 2,
                displayed_changed_files: vec![ChangedFile {
                    status_code: " M".to_owned(),
                    path: "src/lib.rs".to_owned(),
                    kinds: vec![ChangeKind::Unstaged],
                }],
                hidden_changed_files: 0,
                unmerged_commits: None,
            }],
        };

        let context = worktree_safety_confirm_context(
            &report,
            &i18n,
            Locale::ZhCn,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
        )
        .expect("a risky report must produce a confirmation context");

        let body = context
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            body.contains("工作区移除安全报告"),
            "context must carry the localized heading: {body}"
        );
        assert!(
            body.contains("变更文件：2（已暂存 0，未暂存 2，未跟踪 0）"),
            "the localized changed-file count breakdown must be present in the \
             confirmation context: {body}"
        );
        assert!(
            !body.contains("\x1b["),
            "confirmation context must be plain text with no ANSI escapes: {body:?}"
        );

        let count_line = context
            .lines
            .iter()
            .find(|line| line.text.contains("变更文件：2"))
            .expect("the changed-file count row must exist");
        assert_eq!(
            count_line.priority,
            ConfirmLinePriority::Essential,
            "the changed-file count must be decision-critical so it survives a short viewport"
        );
        let detail_line = context
            .lines
            .iter()
            .find(|line| line.text.contains("src/lib.rs"))
            .expect("the per-file detail row must exist");
        assert_eq!(
            detail_line.priority,
            ConfirmLinePriority::Detail,
            "per-file rows must be the first truncated when space is constrained"
        );

        assert_eq!(
            context.truncation_notice,
            i18n.resolve(
                Locale::ZhCn,
                workhive_core::i18n_keys::CONFIRM_CONTEXT_TRUNCATED
            ),
            "the truncation notice must be the localized confirm-context string"
        );
        assert_eq!(
            context.too_small_notice,
            i18n.resolve(
                Locale::ZhCn,
                workhive_core::i18n_keys::CONFIRM_CONTEXT_TOO_SMALL
            ),
            "the too-small notice must be the localized confirm-context string"
        );
    }

    /// The danger headline and the decision-critical risk counts must
    /// carry danger severity, while neutral orienting rows and per-file
    /// detail rows stay `Normal` so the modal can paint the risk
    /// hierarchy without matching localized text.
    #[test]
    fn confirm_context_flags_danger_headline_and_risk_counts_with_severity() {
        use crate::ui::screens::confirm::ConfirmLineSeverity;

        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 5,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "demo01".to_owned(),
                branch: Some("demo01".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/demo01"),
                staged_count: 0,
                unstaged_count: 2,
                untracked_count: 0,
                total_changed_files: 2,
                displayed_changed_files: vec![ChangedFile {
                    status_code: " M".to_owned(),
                    path: "src/lib.rs".to_owned(),
                    kinds: vec![ChangeKind::Unstaged],
                }],
                hidden_changed_files: 0,
                unmerged_commits: Some(UnmergedCommitSummary {
                    base_branch: "main".to_owned(),
                    branch: "demo01".to_owned(),
                    count: 3,
                }),
            }],
        };

        let context = worktree_safety_confirm_context(
            &report,
            &i18n,
            Locale::En,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
        )
        .expect("a risky report must produce a confirmation context");

        let severity_of = |needle: &str| {
            context
                .lines
                .iter()
                .find(|line| line.text.contains(needle))
                .unwrap_or_else(|| panic!("expected a context row containing {needle:?}"))
                .severity
        };

        assert_eq!(
            severity_of("DANGER:"),
            ConfirmLineSeverity::Danger,
            "the danger headline must carry danger severity"
        );
        assert_eq!(
            severity_of("changed files: 2"),
            ConfirmLineSeverity::RiskCount,
            "a positive changed-file count is decision-critical risk"
        );
        assert_eq!(
            severity_of("unmerged commits against main: 3"),
            ConfirmLineSeverity::RiskCount,
            "the unmerged-commit count is decision-critical risk"
        );
        assert_eq!(
            severity_of("src/lib.rs"),
            ConfirmLineSeverity::Normal,
            "per-file detail rows must stay neutral"
        );
        assert_eq!(
            context.lines[0].severity,
            ConfirmLineSeverity::Normal,
            "the heading is a neutral orienting row"
        );
        assert_eq!(
            severity_of("- demo01 ["),
            ConfirmLineSeverity::Normal,
            "the target path is a neutral orienting row"
        );
    }

    /// A worktree that is risky only because its branch diverged (0
    /// changed files but N unmerged commits) must NOT paint its
    /// `0 changed files` count as danger, while the unmerged-commit
    /// count stays decision-critical.
    #[test]
    fn confirm_context_zero_changed_files_count_is_neutral() {
        use crate::ui::screens::confirm::ConfirmLineSeverity;

        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 5,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "demo01".to_owned(),
                branch: Some("demo01".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/demo01"),
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
                total_changed_files: 0,
                displayed_changed_files: vec![],
                hidden_changed_files: 0,
                unmerged_commits: Some(UnmergedCommitSummary {
                    base_branch: "main".to_owned(),
                    branch: "demo01".to_owned(),
                    count: 19,
                }),
            }],
        };

        let context = worktree_safety_confirm_context(
            &report,
            &i18n,
            Locale::En,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
        )
        .expect("a diverged-branch report is still risky");

        let count = context
            .lines
            .iter()
            .find(|line| line.text.contains("changed files: 0"))
            .expect("the 0 changed-files count row must exist");
        assert_eq!(
            count.severity,
            ConfirmLineSeverity::Normal,
            "a 0 changed-files count must not be painted as danger"
        );
        let unmerged = context
            .lines
            .iter()
            .find(|line| line.text.contains("unmerged commits against main: 19"))
            .expect("the unmerged-commit count row must exist");
        assert_eq!(
            unmerged.severity,
            ConfirmLineSeverity::RiskCount,
            "the unmerged-commit count stays decision-critical even with 0 changed files"
        );
    }

    /// A clean report carries no decision risk, so no confirmation
    /// context is built and the original compact prompt is preserved.
    #[test]
    fn confirm_context_is_none_when_no_risks() {
        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![],
        };
        assert!(
            worktree_safety_confirm_context(
                &report,
                &i18n,
                Locale::En,
                workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            )
            .is_none(),
            "a clean report must not produce a confirmation context"
        );
    }

    #[test]
    fn safety_report_details_are_localized_in_chinese() {
        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "feat".to_owned(),
                branch: Some("feat".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/feat"),
                staged_count: 1,
                unstaged_count: 1,
                untracked_count: 1,
                total_changed_files: 2,
                displayed_changed_files: vec![ChangedFile {
                    status_code: " M".to_owned(),
                    path: "src/lib.rs".to_owned(),
                    kinds: vec![ChangeKind::Unstaged],
                }],
                hidden_changed_files: 1,
                unmerged_commits: Some(UnmergedCommitSummary {
                    base_branch: "main".to_owned(),
                    branch: "feat".to_owned(),
                    count: 2,
                }),
            }],
        };

        let rendered = format_worktree_safety_report_lines(
            &report,
            &i18n,
            Locale::ZhCn,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
            SafetyStyleMode::Plain,
        )
        .join("\n");

        assert!(rendered.contains("工作区移除安全报告"));
        assert!(rendered.contains("变更文件：2（已暂存 1，未暂存 1，未跟踪 1）"));
        assert!(rendered.contains("... 另有 1 个变更文件"));
        assert!(rendered.contains("相对于 main 的未合并提交：2"));
        assert!(
            rendered.contains("危险：即将被删除"),
            "Chinese safety report must surface a 'DANGER' banner above the \
             per-worktree risk list: {rendered}"
        );
        assert!(
            rendered.contains("该分支包含 2 个仅存在于本分支"),
            "Chinese safety report must include the unmerged-commit \
             explanation so '相对于 main 的未合并提交' is not opaque: {rendered}"
        );
        assert!(!rendered.contains("changed files:"));
        assert!(!rendered.contains("more changed file"));
        assert!(!rendered.contains("unmerged commits against"));
    }

    /// English regression for the destructive safety banner.
    /// Verifies the headline + unmerged-commit explanation appear in EN
    /// and that existing per-worktree details survive the rewrite.
    #[test]
    fn safety_report_emits_english_danger_headline_and_explanation() {
        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "feat".to_owned(),
                branch: Some("feat".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/feat"),
                staged_count: 0,
                unstaged_count: 1,
                untracked_count: 0,
                total_changed_files: 1,
                displayed_changed_files: vec![ChangedFile {
                    status_code: " M".to_owned(),
                    path: "src/lib.rs".to_owned(),
                    kinds: vec![ChangeKind::Unstaged],
                }],
                hidden_changed_files: 0,
                unmerged_commits: Some(UnmergedCommitSummary {
                    base_branch: "main".to_owned(),
                    branch: "feat".to_owned(),
                    count: 3,
                }),
            }],
        };

        let rendered = format_worktree_safety_report_lines(
            &report,
            &i18n,
            Locale::En,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
            SafetyStyleMode::Plain,
        )
        .join("\n");

        assert!(
            rendered.contains("DANGER:"),
            "English safety report must start risky output with a DANGER \
             banner so the warning is not lost in normal output: {rendered}"
        );
        assert!(
            rendered.contains("changed files: 1 (staged 0, unstaged 1, untracked 0)"),
            "existing changed-files detail line must still appear: {rendered}"
        );
        assert!(
            rendered.contains("unmerged commits against main: 3"),
            "existing unmerged-commits count must still appear: {rendered}"
        );
        assert!(
            rendered.contains("3 commit(s) reachable from it but not from `main`"),
            "English explanation must clarify what 'unmerged commits' means: {rendered}"
        );
        assert!(
            !rendered.contains("No uncommitted file changes were detected"),
            "the clean-worktree explanation is reserved for the 0-changed-file \
             case: {rendered}"
        );
    }

    /// `0` changed files but `N` unmerged commits must STILL warn and
    /// must use the distinct clean-worktree/diverged-branch explanation
    /// so the user does not assume "0 file changes means no risk".
    #[test]
    fn safety_report_clean_worktree_with_unmerged_commits_uses_distinct_explanation() {
        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "demo01".to_owned(),
                branch: Some("demo01".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/demo01"),
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
                total_changed_files: 0,
                displayed_changed_files: vec![],
                hidden_changed_files: 0,
                unmerged_commits: Some(UnmergedCommitSummary {
                    base_branch: "main".to_owned(),
                    branch: "demo01".to_owned(),
                    count: 19,
                }),
            }],
        };

        for (locale, banner, clean_marker) in [
            (
                Locale::En,
                "DANGER:",
                "No uncommitted file changes were detected",
            ),
            (Locale::ZhCn, "危险：即将被删除", "未检测到未提交的文件变更"),
        ] {
            let rendered = format_worktree_safety_report_lines(
                &report,
                &i18n,
                locale,
                workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
                workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
                SafetyStyleMode::Plain,
            )
            .join("\n");

            assert!(
                rendered.contains(banner),
                "0-changed-files + unmerged-commits MUST still surface the \
                 DANGER banner in {locale:?}: {rendered}"
            );
            assert!(
                rendered.contains(clean_marker),
                "clean-worktree-but-diverged-branch must use the distinct \
                 explanation in {locale:?}: {rendered}"
            );
            assert!(
                !rendered.contains("reachable from it but not from")
                    && !rendered.contains("仅存在于本分支"),
                "the generic unmerged-commit explanation must NOT appear in \
                 the clean-worktree case in {locale:?}; the distinct \
                 explanation replaces it: {rendered}"
            );
        }
    }

    /// Color mode must wrap the danger banner in ANSI escape codes so
    /// interactive terminals show it in red+bold. Plain mode must emit
    /// no escape sequences at all so CI logs, `tee`, and `NO_COLOR=1`
    /// stay clean.
    #[test]
    fn safety_report_color_mode_emits_ansi_and_plain_mode_does_not() {
        let i18n = StaticI18n::new();
        let report = WorktreeSafetyReport {
            base_branch: Some("main".to_owned()),
            scanned_worktrees: 1,
            display_limit: 1,
            risky_worktrees: vec![WorktreeRisk {
                workspace: "feat".to_owned(),
                branch: Some("feat".to_owned()),
                path: Utf8PathBuf::from("/project/.workhive/worktrees/feat"),
                staged_count: 0,
                unstaged_count: 1,
                untracked_count: 0,
                total_changed_files: 1,
                displayed_changed_files: vec![],
                hidden_changed_files: 0,
                unmerged_commits: None,
            }],
        };

        let color = format_worktree_safety_report_lines(
            &report,
            &i18n,
            Locale::En,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
            SafetyStyleMode::Color,
        )
        .join("\n");
        assert!(
            color.contains("\x1b[1;31m") && color.contains("\x1b[0m"),
            "Color mode must wrap the DANGER banner in red+bold ANSI: {color:?}"
        );

        let plain = format_worktree_safety_report_lines(
            &report,
            &i18n,
            Locale::En,
            workhive_core::i18n_keys::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
            workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
            SafetyStyleMode::Plain,
        )
        .join("\n");
        assert!(
            !plain.contains("\x1b["),
            "Plain mode must NOT emit ANSI escape sequences so CI/`tee`/\
             `NO_COLOR=1` output stays clean: {plain:?}"
        );
        assert!(
            plain.contains("DANGER:"),
            "Plain mode must still convey 'DANGER' through wording so the \
             warning is visible without color: {plain:?}"
        );
    }
}

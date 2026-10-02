//! Confirm screen — Y/N prompt rendered via the shared TerminalSession.

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::error::CliError;
use crate::ui::events::{UiAction, read_action};
use crate::ui::session::{TerminalSession, modal_content_area};

/// Upper bound on the confirmation modal's reserved height in rows.
///
/// A destructive confirmation may carry a multiline safety context
/// above the Yes/No choice. The reserved height grows with that
/// context but is capped here so a very long report can never reserve
/// an unbounded modal; [`modal_content_area`] additionally caps the
/// drawn height at the live terminal height, and any context lines
/// that still do not fit are truncated by priority (see
/// [`fit_context_lines`]).
const CONFIRM_MAX_HEIGHT: u16 = 20;

/// Display priority of a single safety-context row when the
/// confirmation viewport cannot show every row.
///
/// The fitter keeps higher-priority rows and truncates lower-priority
/// rows first, so the decision-critical risk counts survive a short
/// viewport while bulk file listings give way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmLinePriority {
    /// Numeric risk facts the user must read before confirming
    /// deletion — the changed-file count breakdown and the
    /// unmerged-commit count. These stay visible whenever any context
    /// is shown.
    Essential,
    /// Orienting rows: heading, danger banner, target workspace, and
    /// risk explanations. Kept after `Essential` rows, dropped only
    /// when the viewport cannot hold them alongside the counts.
    Important,
    /// Representative file detail and overflow rows. These are the
    /// first content truncated when space is constrained.
    Detail,
}

/// Visual severity of a single safety-context row inside the
/// confirmation modal.
///
/// Independent from [`ConfirmLinePriority`]: priority decides which
/// rows survive a short viewport, while severity decides how a
/// surviving row is painted. The danger headline and the
/// decision-critical risk counts render in red so destructive risk is
/// unmistakable at the prompt, while neutral orienting rows and
/// per-file detail rows stay visually subordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmLineSeverity {
    /// Danger headline — rendered red and bold.
    Danger,
    /// Decision-critical risk count (changed-file count, unmerged-commit
    /// count) — rendered red and emphasized.
    RiskCount,
    /// Neutral orienting or secondary detail — rendered without
    /// emphasis so it cannot compete with the danger rows.
    Normal,
}

impl ConfirmLineSeverity {
    /// Modal style for a context row of this severity.
    ///
    /// `Danger` and `RiskCount` both render red so the headline and the
    /// risk counts dominate; `Danger` additionally carries bold to keep
    /// the headline at the top of the visual hierarchy. `Normal` rows
    /// keep the terminal's default foreground so they read as secondary.
    pub(crate) fn style(self) -> Style {
        match self {
            Self::Danger => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Self::RiskCount => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            Self::Normal => Style::default(),
        }
    }
}

/// One row of decision-local safety context plus its display priority
/// and visual severity.
#[derive(Debug, Clone)]
pub(crate) struct ConfirmContextLine {
    pub text: String,
    pub priority: ConfirmLinePriority,
    pub severity: ConfirmLineSeverity,
}

/// Decision-local context shown above the Yes/No choice in a
/// destructive confirmation.
///
/// Destructive removals (`workspace remove`, `workspace destroy`,
/// forced risky `worktree remove`) print a safety report to stderr
/// before the prompt, but the modal alternate screen can cover that
/// output at the moment of decision. Threading the same safety rows
/// into the confirmation keeps the changed-file count breakdown
/// visible at the decision point.
///
/// `lines` are already-localized, plain-text rows (no ANSI escapes —
/// the modal applies its own styling), each tagged with a
/// [`ConfirmLinePriority`]. When the viewport is too short, lower-
/// priority detail rows are truncated first and `truncation_notice`
/// marks that truncation explicitly. `too_small_notice` is shown when
/// the viewport cannot fit even the decision-critical counts, so the
/// user is never presented a normal-looking confirmation that hides
/// the safety counts.
#[derive(Debug, Clone)]
pub(crate) struct ConfirmContext {
    pub lines: Vec<ConfirmContextLine>,
    pub truncation_notice: String,
    pub too_small_notice: String,
}

/// Pure decision step for the confirm screen's event loop.
///
/// Extracted from [`run`] so the y/Y → return-true,
/// n/N → return-false, q/Esc/Ctrl+C → abort, Toggle → flip-default,
/// Enter → take-default, and unrelated-key → keep-looping invariants
/// can be exercised without spinning up a
/// real terminal or capturing real key events. The variants intentionally
/// mirror the three possible loop outcomes used by `run`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ConfirmDecision {
    /// Stay in the prompt loop; `current` reflects the highlighted choice
    /// after applying any side effect of the action (e.g. Toggle flips it).
    Continue { current: bool },
    /// Resolve the prompt with the given Yes/No answer.
    Return(bool),
    /// User asked to abort (q, Esc, or Ctrl+C).
    Abort,
}

/// Apply a single [`UiAction`] to the confirm loop's state.
///
/// This is the production decision logic used by [`run`]; keeping it
/// pure makes the per-key behavior unit-testable and prevents the
/// "Enter accidentally maps to N because default was N" class of bugs
/// from creeping back in without a regression test catching it.
pub(crate) fn next_confirm_decision(action: UiAction, current: bool) -> ConfirmDecision {
    match action {
        UiAction::Yes => ConfirmDecision::Return(true),
        UiAction::No => ConfirmDecision::Return(false),
        UiAction::Toggle | UiAction::Left | UiAction::Right => {
            ConfirmDecision::Continue { current: !current }
        }
        UiAction::Confirm => ConfirmDecision::Return(current),
        UiAction::Abort => ConfirmDecision::Abort,
        _ => ConfirmDecision::Continue { current },
    }
}

/// Run a Y/N confirmation prompt within the given terminal session.
///
/// Returns `true` for yes, `false` for no.
pub fn run(prompt: &str, default: bool) -> Result<bool, CliError> {
    run_with_context(prompt, None, default)
}

/// Run a Y/N confirmation prompt, optionally rendering a safety
/// `context` block above the Yes/No choice.
///
/// The decision behavior (`y`/`n`, Enter-takes-default, Tab/arrows
/// toggle, Esc/q/Ctrl+C abort, resize redraw) is identical to
/// [`run`]: only the rendered body gains the context lines. When
/// `context` is `None`, the layout collapses to the same prompt +
/// choice surface [`run`] has always drawn.
pub fn run_with_context(
    prompt: &str,
    context: Option<&ConfirmContext>,
    default: bool,
) -> Result<bool, CliError> {
    let mut session = TerminalSession::modal()?;
    let mut current = default;
    let context_lines = context.map(|c| c.lines.as_slice()).unwrap_or(&[]);

    loop {
        session
            .terminal()
            .draw(|frame| {
                let height =
                    confirm_viewport_height_for_width(prompt, context_lines, frame.area().width);
                let area = modal_content_area(frame.area(), height);
                render(frame, area, prompt, context, current);
            })
            .map_err(|e| CliError::Runtime(e.to_string()))?;

        let action = read_action()?;
        if matches!(action, UiAction::Resize) {
            // The next modal frame recomputes wrapping from the live
            // terminal width and redraws the clean alternate screen.
            continue;
        }
        match next_confirm_decision(action, current) {
            ConfirmDecision::Return(answer) => return Ok(answer),
            ConfirmDecision::Continue { current: next } => current = next,
            ConfirmDecision::Abort => return Err(CliError::UserAborted),
        }
    }
}

fn render(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    prompt: &str,
    context: Option<&ConfirmContext>,
    current: bool,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let content_width = inner.width as usize;
    // The prompt is rendered with a single leading/trailing space, so
    // its wrap width is two columns narrower than the inner area.
    let prompt_width = content_width.saturating_sub(2).max(1);
    let prompt_rows = wrapped_line_count(prompt, prompt_width) as u16;
    // Choice always takes one row; the prompt takes its wrapped rows;
    // the context block takes whatever remains above the prompt.
    let context_rows = inner.height.saturating_sub(prompt_rows).saturating_sub(1);

    let [context_area, prompt_area, choice_area] = Layout::vertical([
        Constraint::Length(context_rows),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    if let Some(context) = context
        && context_rows > 0
        && !context.lines.is_empty()
    {
        let notice_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let fit = fit_context_lines(&context.lines, context_rows, content_width);
        let mut body: Vec<Line> = Vec::new();
        if fit.too_small {
            // The viewport cannot hold the decision-critical counts, so
            // show an explicit recap instead of a partial context that
            // would hide them behind a normal-looking prompt.
            body.push(Line::from(Span::styled(
                context.too_small_notice.clone(),
                notice_style,
            )));
        } else {
            body.extend(
                fit.visible
                    .iter()
                    .map(|line| Line::from(Span::styled(line.text.clone(), line.severity.style()))),
            );
            if fit.hidden > 0 {
                body.push(Line::from(Span::styled(
                    context.truncation_notice.clone(),
                    notice_style,
                )));
            }
        }
        frame.render_widget(Paragraph::new(body), context_area);
    }

    let yes_style = if current {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let no_style = if !current {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let prompt_line = Line::from(vec![Span::raw(format!(" {} ", prompt))]);
    let choice_line = Line::from(vec![
        Span::styled("Yes", yes_style),
        Span::raw(" / "),
        Span::styled("No", no_style),
    ]);

    frame.render_widget(
        Paragraph::new(prompt_line).wrap(Wrap { trim: true }),
        prompt_area,
    );
    frame.render_widget(Paragraph::new(choice_line), choice_area);
}

fn confirm_viewport_height_for_width(
    prompt: &str,
    context: &[ConfirmContextLine],
    width: u16,
) -> u16 {
    let content_width = width.saturating_sub(4).max(20) as usize;
    confirm_viewport_height_for_content_width(prompt, context, content_width)
}

fn confirm_viewport_height_for_content_width(
    prompt: &str,
    context: &[ConfirmContextLine],
    content_width: usize,
) -> u16 {
    let prompt_lines = wrapped_line_count(prompt, content_width);
    if context.is_empty() {
        // Preserve the original compact prompt-only sizing: borders (2)
        // + choice (1) on top of the wrapped prompt lines.
        return (prompt_lines as u16 + 3).clamp(4, 12);
    }
    let context_rows: usize = context
        .iter()
        .map(|line| wrapped_line_count(&line.text, content_width))
        .sum();
    // borders (2) + context rows + one row reserved for the truncation
    // notice + wrapped prompt rows + choice (1).
    let desired = prompt_lines + context_rows + 4;
    (desired as u16).clamp(4, CONFIRM_MAX_HEIGHT)
}

/// Outcome of fitting a safety context into a bounded number of rows.
struct ContextFit<'a> {
    /// Context rows to draw, in natural report order. Each row keeps its
    /// severity so the renderer can paint danger rows red.
    visible: Vec<&'a ConfirmContextLine>,
    /// Number of context rows omitted by truncation.
    hidden: usize,
    /// The viewport cannot hold the decision-critical counts; the
    /// caller must surface an explicit terminal-too-small recap rather
    /// than a partial context.
    too_small: bool,
}

/// Fit a safety context into `max_rows` display rows at `content_width`,
/// truncating by priority so decision-critical rows survive.
///
/// When everything fits, all rows are returned untruncated. Otherwise
/// one row is reserved for an explicit truncation notice and the
/// remaining budget is filled by priority: [`ConfirmLinePriority::Essential`]
/// count rows first, then [`ConfirmLinePriority::Important`] orienting
/// rows, then [`ConfirmLinePriority::Detail`] file rows. Lower-priority
/// detail is therefore dropped before the changed-file count breakdown.
/// All [`ConfirmLinePriority::Essential`] count rows are kept together:
/// if the budget cannot hold every essential count at once, `too_small`
/// is set so the caller shows a terminal-too-small recap instead of a
/// confirmation that hides any one count.
fn fit_context_lines(
    context: &[ConfirmContextLine],
    max_rows: u16,
    content_width: usize,
) -> ContextFit<'_> {
    if context.is_empty() {
        return ContextFit {
            visible: Vec::new(),
            hidden: 0,
            too_small: false,
        };
    }
    let max_rows = max_rows as usize;
    let row_count = |line: &ConfirmContextLine| wrapped_line_count(&line.text, content_width);
    let total: usize = context.iter().map(row_count).sum();
    if max_rows > 0 && total <= max_rows {
        return ContextFit {
            visible: context.iter().collect(),
            hidden: 0,
            too_small: false,
        };
    }

    // Reserve the final row for the explicit truncation notice.
    let budget = max_rows.saturating_sub(1);

    // Every Essential count row is decision-critical, so the counts are
    // all-or-nothing: a report can carry several risky targets, each
    // with its own changed-file or unmerged-count row. If the budget
    // cannot hold all of them together, refuse a partial render and
    // signal the too-small recap rather than a confirmation that hides
    // any one count.
    let essential_rows: usize = context
        .iter()
        .filter(|line| line.priority == ConfirmLinePriority::Essential)
        .map(row_count)
        .sum();
    if essential_rows > budget {
        return ContextFit {
            visible: Vec::new(),
            hidden: context.len(),
            too_small: true,
        };
    }

    // Select rows highest-priority first while the running height fits.
    // All Essential rows fit (checked above); Important orienting rows
    // and then Detail file rows fill the remaining budget, so detail is
    // dropped before any count.
    let mut selected = vec![false; context.len()];
    let mut used = 0usize;
    for tier in [
        ConfirmLinePriority::Essential,
        ConfirmLinePriority::Important,
        ConfirmLinePriority::Detail,
    ] {
        for (idx, line) in context.iter().enumerate() {
            if line.priority != tier {
                continue;
            }
            let rows = row_count(line);
            if used + rows > budget {
                continue;
            }
            selected[idx] = true;
            used += rows;
        }
    }

    let visible: Vec<&ConfirmContextLine> = context
        .iter()
        .zip(&selected)
        .filter_map(|(line, &keep)| keep.then_some(line))
        .collect();
    let hidden = context.len() - visible.len();
    ContextFit {
        visible,
        hidden,
        too_small: false,
    }
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    let safe_width = width.max(1);
    text.split('\n')
        .map(|line| {
            let line_width = UnicodeWidthStr::width(line);
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(safe_width)
            }
        })
        .sum::<usize>()
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::{
        ConfirmContextLine, ConfirmDecision, ConfirmLinePriority, ConfirmLineSeverity,
        confirm_viewport_height_for_content_width, fit_context_lines, next_confirm_decision,
        wrapped_line_count,
    };
    use crate::ui::events::UiAction;

    #[test]
    fn wrapped_line_count_handles_long_single_line_prompt() {
        let text = "a".repeat(45);
        assert_eq!(wrapped_line_count(&text, 20), 3);
    }

    #[test]
    fn wrapped_line_count_preserves_explicit_line_breaks() {
        assert_eq!(wrapped_line_count("hello\nworld", 20), 2);
    }

    #[test]
    fn confirm_viewport_height_grows_for_long_prompt() {
        let prompt = "ＡＢＣＤＥＦＧＨ ".repeat(8);
        assert!(confirm_viewport_height_for_content_width(&prompt, &[], 20) > 4);
    }

    // ── Confirm-with-context layout ────────────────────────────────
    //
    // Destructive confirmations thread the safety report into the
    // modal body. The product invariant: the reserved height accounts
    // for the context, and when the viewport is too short the
    // decision-critical changed-file count stays visible while lower-
    // priority file detail rows are truncated first behind an explicit
    // notice.

    const CHANGED_FILES_COUNT: &str = "changed files: 2 (staged 0, unstaged 2, untracked 0)";

    fn line(text: &str, priority: ConfirmLinePriority) -> ConfirmContextLine {
        // Fitting behavior is severity-agnostic, so these rows default to
        // `Normal`; severity rendering is covered by dedicated tests.
        ConfirmContextLine {
            text: text.to_owned(),
            priority,
            severity: ConfirmLineSeverity::Normal,
        }
    }

    /// Mirrors the row order and priority tagging produced by the
    /// safety-report confirm-context builder: heading/danger/target are
    /// orienting rows, the changed-file count is essential, and the
    /// per-file rows are truncatable detail.
    fn sample_safety_context() -> Vec<ConfirmContextLine> {
        vec![
            line(
                "Workspace remove safety report:",
                ConfirmLinePriority::Important,
            ),
            line(
                "DANGER: workspace about to be removed contains unsaved work.",
                ConfirmLinePriority::Important,
            ),
            line(
                "  - demo01 [/repo/.workhive/worktrees/demo01]",
                ConfirmLinePriority::Important,
            ),
            line(
                &format!("    {CHANGED_FILES_COUNT}"),
                ConfirmLinePriority::Essential,
            ),
            line("      M src/lib.rs", ConfirmLinePriority::Detail),
            line("      M src/main.rs", ConfirmLinePriority::Detail),
        ]
    }

    const SECOND_CHANGED_FILES_COUNT: &str = "changed files: 3 (staged 1, unstaged 2, untracked 0)";

    /// Mirrors a project-level safety report with two risky worktrees,
    /// each contributing its own essential changed-file count plus a
    /// truncatable file detail row. Each target's count is decision-
    /// critical, so both essential rows must survive together.
    fn sample_multi_worktree_context() -> Vec<ConfirmContextLine> {
        vec![
            line("Destroy safety report:", ConfirmLinePriority::Important),
            line(
                "DANGER: 2 worktrees about to be removed contain unsaved work.",
                ConfirmLinePriority::Important,
            ),
            line(
                "  - demo01 [/repo/.workhive/worktrees/demo01]",
                ConfirmLinePriority::Important,
            ),
            line(
                &format!("    {CHANGED_FILES_COUNT}"),
                ConfirmLinePriority::Essential,
            ),
            line("      M src/lib.rs", ConfirmLinePriority::Detail),
            line(
                "  - demo02 [/repo/.workhive/worktrees/demo02]",
                ConfirmLinePriority::Important,
            ),
            line(
                &format!("    {SECOND_CHANGED_FILES_COUNT}"),
                ConfirmLinePriority::Essential,
            ),
            line("      M src/main.rs", ConfirmLinePriority::Detail),
        ]
    }

    #[test]
    fn confirm_viewport_height_accounts_for_context_lines() {
        let prompt = "Remove workspace 'demo01' and close its session?";
        let context = sample_safety_context();
        let with_context = confirm_viewport_height_for_content_width(prompt, &context, 60);
        let without_context = confirm_viewport_height_for_content_width(prompt, &[], 60);
        assert!(
            with_context > without_context,
            "a multiline safety context must reserve more rows than a bare \
             prompt: with={with_context} without={without_context}"
        );
        assert!(
            with_context <= super::CONFIRM_MAX_HEIGHT,
            "reserved height must stay capped at CONFIRM_MAX_HEIGHT"
        );
    }

    #[test]
    fn fit_context_lines_keeps_all_lines_when_they_fit() {
        let context = sample_safety_context();
        let fit = fit_context_lines(&context, 20, 80);
        assert_eq!(fit.hidden, 0, "nothing is hidden when every line fits");
        assert!(!fit.too_small);
        assert_eq!(fit.visible.len(), context.len());
        assert!(
            fit.visible
                .iter()
                .any(|line| line.text.contains(CHANGED_FILES_COUNT)),
            "the changed-file count line must be present: {:?}",
            fit.visible
        );
    }

    /// Short viewport: the decision-critical changed-file count must
    /// stay visible. This fails on a physical-order fitter that drops
    /// the count along with the detail rows.
    #[test]
    fn fit_context_lines_keeps_changed_file_count_in_short_viewport() {
        let context = sample_safety_context();
        let fit = fit_context_lines(&context, 4, 80);
        assert!(
            !fit.too_small,
            "a 4-row budget can still show the count recap"
        );
        assert!(fit.hidden > 0, "a short viewport must report hidden lines");
        assert!(
            fit.visible
                .iter()
                .any(|line| line.text.contains(CHANGED_FILES_COUNT)),
            "the changed-file count breakdown must remain visible before \
             confirmation in a short viewport: {:?}",
            fit.visible
        );
        assert_eq!(
            fit.visible.len() + fit.hidden,
            context.len(),
            "every context line is either visible or counted as hidden"
        );
    }

    /// Representative per-file detail rows are the first content dropped
    /// when space is constrained; the count and target survive.
    #[test]
    fn fit_context_lines_truncates_detail_rows_before_count() {
        let context = sample_safety_context();
        // Five rows fit everything except the two detail rows (one row
        // is reserved for the truncation notice).
        let fit = fit_context_lines(&context, 5, 80);
        assert!(!fit.too_small);
        assert!(fit.hidden > 0);
        assert!(
            fit.visible
                .iter()
                .any(|line| line.text.contains(CHANGED_FILES_COUNT)),
            "the count line must survive: {:?}",
            fit.visible
        );
        assert!(
            !fit.visible
                .iter()
                .any(|line| line.text.contains("src/lib.rs"))
                && !fit
                    .visible
                    .iter()
                    .any(|line| line.text.contains("src/main.rs")),
            "the representative file detail rows must be the first dropped: {:?}",
            fit.visible
        );
    }

    /// When the budget cannot hold even the essential count, the fitter
    /// reports `too_small` so the caller shows an explicit terminal-too-
    /// small recap instead of a confirmation that hides the count.
    #[test]
    fn fit_context_lines_flags_too_small_when_count_cannot_fit() {
        let context = sample_safety_context();
        let fit = fit_context_lines(&context, 1, 80);
        assert!(
            fit.too_small,
            "a one-row budget cannot hold the count recap and must flag too_small"
        );
        assert!(fit.visible.is_empty());
        assert_eq!(fit.hidden, context.len());
    }

    /// Multiple risky targets each contribute a decision-critical count.
    /// When the budget can hold only one of them, a partial render that
    /// shows one count and hides the other is unsafe, so the fitter must
    /// flag `too_small` for the explicit recap instead.
    #[test]
    fn fit_context_lines_flags_too_small_when_not_every_count_fits() {
        let context = sample_multi_worktree_context();
        // A two-row viewport reserves one row for the notice, leaving a
        // one-row budget — room for a single count but not both.
        let fit = fit_context_lines(&context, 2, 80);
        assert!(
            fit.too_small,
            "a budget that fits only one of two essential counts must flag \
             too_small rather than hide the other count: {:?}",
            fit.visible
        );
        assert!(fit.visible.is_empty());
        assert_eq!(fit.hidden, context.len());
    }

    /// When the budget can hold every essential count but not the file
    /// detail rows, all counts stay visible and the detail rows are the
    /// only content truncated.
    #[test]
    fn fit_context_lines_keeps_all_counts_when_only_details_truncate() {
        let context = sample_multi_worktree_context();
        // Eight rows total; a seven-row viewport drops both detail rows
        // (one row reserved for the notice) while keeping both counts.
        let fit = fit_context_lines(&context, 7, 80);
        assert!(
            !fit.too_small,
            "all essential counts fit, so the fitter must render partial \
             context rather than the too-small recap"
        );
        assert!(fit.hidden > 0, "the detail rows are truncated");
        assert!(
            fit.visible
                .iter()
                .any(|line| line.text.contains(CHANGED_FILES_COUNT)),
            "the first target's count must stay visible: {:?}",
            fit.visible
        );
        assert!(
            fit.visible
                .iter()
                .any(|line| line.text.contains(SECOND_CHANGED_FILES_COUNT)),
            "the second target's count must stay visible: {:?}",
            fit.visible
        );
        assert!(
            !fit.visible
                .iter()
                .any(|line| line.text.contains("src/lib.rs"))
                && !fit
                    .visible
                    .iter()
                    .any(|line| line.text.contains("src/main.rs")),
            "per-file detail rows are the first content dropped: {:?}",
            fit.visible
        );
    }

    // ── Confirm-context severity styling ───────────────────────────
    //
    // The modal must make destructive risk unmistakable: the danger
    // headline renders red+bold, the decision-critical risk counts
    // render red, and per-file detail rows stay neutral so they cannot
    // compete with the danger rows. These are pinned at two levels: a
    // pure severity→style mapping and a full ratatui render that proves
    // the painted buffer cells carry the expected colors.

    #[test]
    fn severity_style_maps_danger_and_counts_to_red_and_detail_to_neutral() {
        use ratatui::style::{Color, Modifier};

        let danger = ConfirmLineSeverity::Danger.style();
        assert_eq!(danger.fg, Some(Color::Red), "danger headline must be red");
        assert!(
            danger.add_modifier.contains(Modifier::BOLD),
            "danger headline must be bold"
        );

        let count = ConfirmLineSeverity::RiskCount.style();
        assert_eq!(
            count.fg,
            Some(Color::Red),
            "risk-count rows must be red/emphasized"
        );

        let normal = ConfirmLineSeverity::Normal.style();
        assert_ne!(
            normal.fg,
            Some(Color::Red),
            "neutral detail rows must not use danger red"
        );
    }

    /// Full render: paint a danger headline, a changed-file count, and a
    /// per-file detail row into a test buffer and assert the danger and
    /// count rows are red while the detail row is not.
    #[test]
    fn modal_render_paints_danger_and_counts_red_and_detail_neutral() {
        use super::{ConfirmContext, render};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::style::{Color, Modifier};

        const W: u16 = 80;
        const H: u16 = 20;

        let context = ConfirmContext {
            lines: vec![
                ConfirmContextLine {
                    text: "DANGER: workspace contains unsaved work.".to_owned(),
                    priority: ConfirmLinePriority::Important,
                    severity: ConfirmLineSeverity::Danger,
                },
                ConfirmContextLine {
                    text: "changed files: 2 (staged 0, unstaged 2, untracked 0)".to_owned(),
                    priority: ConfirmLinePriority::Essential,
                    severity: ConfirmLineSeverity::RiskCount,
                },
                ConfirmContextLine {
                    text: "M src/lib.rs".to_owned(),
                    priority: ConfirmLinePriority::Detail,
                    severity: ConfirmLineSeverity::Normal,
                },
            ],
            truncation_notice: "more hidden".to_owned(),
            too_small_notice: "too small".to_owned(),
        };

        let mut terminal = Terminal::new(TestBackend::new(W, H)).expect("test terminal");
        terminal
            .draw(|frame| {
                render(
                    frame,
                    frame.area(),
                    "Remove workspace?",
                    Some(&context),
                    false,
                );
            })
            .expect("render must succeed");
        let buf = terminal.backend().buffer();

        // Style of the first inner (non-border) content cell on the row
        // whose reconstructed text contains `needle`.
        let row_style = |needle: &str| -> Option<(Color, Modifier)> {
            for y in 1..H - 1 {
                let mut row = String::new();
                for x in 1..W - 1 {
                    row.push_str(buf[(x, y)].symbol());
                }
                if row.contains(needle) {
                    for x in 1..W - 1 {
                        let cell = &buf[(x, y)];
                        if cell.symbol().trim().is_empty() {
                            continue;
                        }
                        return Some((cell.fg, cell.modifier));
                    }
                }
            }
            None
        };

        let (danger_fg, danger_mod) = row_style("DANGER").expect("danger row must render");
        assert_eq!(danger_fg, Color::Red, "danger headline must be red");
        assert!(
            danger_mod.contains(Modifier::BOLD),
            "danger headline must be bold"
        );

        let (count_fg, _) =
            row_style("changed files: 2").expect("changed-file count row must render");
        assert_eq!(
            count_fg,
            Color::Red,
            "changed-file count row must be red/emphasized"
        );

        let (detail_fg, _) = row_style("src/lib.rs").expect("detail row must render");
        assert_ne!(
            detail_fg,
            Color::Red,
            "per-file detail row must stay neutral and not use danger red"
        );
    }

    // ── Confirm decision matrix ─────────────────────────────────────
    //
    // These tests pin the per-key decision invariants:
    //   * y/Y immediately confirms regardless of the default
    //   * n/N immediately cancels regardless of the default
    //   * q/Esc/Ctrl+C all abort (UiAction::Abort)
    //   * Enter takes the highlighted default
    //   * Tab/Left/Right toggle the highlighted default but keep looping
    //   * Unrelated keys keep the loop running without flipping state
    //
    // We exercise the production decision function `next_confirm_decision`
    // directly. The crossterm `event::read()` boundary lives in
    // `ui::events::read_action` and remains out of scope for unit tests
    // (covered by `classify_shortcut_key` tests and manual verification).

    #[test]
    fn yes_returns_true_regardless_of_default() {
        assert_eq!(
            next_confirm_decision(UiAction::Yes, false),
            ConfirmDecision::Return(true)
        );
        assert_eq!(
            next_confirm_decision(UiAction::Yes, true),
            ConfirmDecision::Return(true)
        );
    }

    #[test]
    fn no_returns_false_regardless_of_default() {
        assert_eq!(
            next_confirm_decision(UiAction::No, false),
            ConfirmDecision::Return(false)
        );
        assert_eq!(
            next_confirm_decision(UiAction::No, true),
            ConfirmDecision::Return(false)
        );
    }

    #[test]
    fn abort_action_aborts() {
        // UiAction::Abort is the canonical mapping for q, Esc, and
        // Ctrl+C inside `read_action`. Aborting must propagate as
        // ConfirmDecision::Abort so `require_confirm` returns the
        // typed `UserAborted` error, NOT silently coerce to `false`.
        assert_eq!(
            next_confirm_decision(UiAction::Abort, false),
            ConfirmDecision::Abort
        );
        assert_eq!(
            next_confirm_decision(UiAction::Abort, true),
            ConfirmDecision::Abort
        );
    }

    #[test]
    fn confirm_with_default_no_returns_false() {
        assert_eq!(
            next_confirm_decision(UiAction::Confirm, false),
            ConfirmDecision::Return(false)
        );
    }

    #[test]
    fn confirm_with_default_yes_returns_true() {
        assert_eq!(
            next_confirm_decision(UiAction::Confirm, true),
            ConfirmDecision::Return(true)
        );
    }

    #[test]
    fn toggle_flips_current_and_keeps_looping() {
        assert_eq!(
            next_confirm_decision(UiAction::Toggle, false),
            ConfirmDecision::Continue { current: true }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Left, true),
            ConfirmDecision::Continue { current: false }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Right, false),
            ConfirmDecision::Continue { current: true }
        );
    }

    /// A terminal resize is a layout event: it must keep the prompt
    /// open with the highlighted choice unchanged, never silently
    /// confirm or abort. `run` intercepts `UiAction::Resize` before the
    /// decision function to re-reserve the viewport, but the decision
    /// function must also classify it as a no-op so a stray resize can
    /// never collapse the prompt.
    #[test]
    fn resize_keeps_current_state_and_never_resolves() {
        assert_eq!(
            next_confirm_decision(UiAction::Resize, false),
            ConfirmDecision::Continue { current: false }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Resize, true),
            ConfirmDecision::Continue { current: true }
        );
    }

    #[test]
    fn unrelated_keys_keep_current_state() {
        assert_eq!(
            next_confirm_decision(UiAction::Up, false),
            ConfirmDecision::Continue { current: false }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Down, true),
            ConfirmDecision::Continue { current: true }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Noop, false),
            ConfirmDecision::Continue { current: false }
        );
        assert_eq!(
            next_confirm_decision(UiAction::Backspace, true),
            ConfirmDecision::Continue { current: true }
        );
    }
}

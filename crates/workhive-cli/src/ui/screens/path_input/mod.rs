//! Path input screen with live search and Tab completion.
//!
//! Differs from `text_input` in three important ways:
//!
//! 1. **Raw key handling.** `text_input` routes events through
//!    [`crate::ui::events::read_action`], which globally intercepts
//!    `j`, `k`, `q`, `y`, `n`, and `Tab` as UI actions — fine for
//!    select/confirm screens, fatal for path entry. `path_input`
//!    reads `crossterm` events directly so the user can type any
//!    legal path character.
//!
//! 2. **Live search.** Every printable keystroke and Backspace re-runs the same pure use case
//!    ([`workhive_core::usecases::path_completion::compute_path_completions`])
//!    against the current buffer. The candidate list refreshes
//!    immediately, scoped to one parent directory at a time. The
//!    user does NOT have to press Tab for the list to appear.
//!
//! 3. **Tab completion.** `Tab` is still supported as an explicit
//!    accelerator: a single match is filled in, multiple matches are
//!    highlighted at index 0 so arrows + Enter can pick one.
//!
//! `Esc` is *modal*: when a candidate list is visible Esc dismisses
//! the list (and any "no match" hint) without leaving the prompt.
//! With no list visible Esc returns
//! [`PathInputOutcome::Cancelled`] so the wizard can re-prompt.
//!
//! The screen is intentionally narrow — no history, no multiline,
//! no fancy editing. It exists to satisfy the trusted-root prompt
//! requirements for global-init trusted-root path completion, empty
//! submission handling, and live search.

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use camino::Utf8Path;
use workhive_core::ports::PathCompletionFsPort;
use workhive_core::usecases::path_completion::{
    PathCompletionResult, compute_path_completions, longest_common_prefix,
};
use workhive_core::usecases::path_strategy::PathStrategy;

use crate::error::CliError;
use crate::ui::session::{TerminalSession, modal_content_area};
use crate::ui::severity::MessageSeverity;

mod candidate_view;
mod path_rules;
mod render;
mod state;

use candidate_view::*;
use path_rules::*;
use state::State;

const MAX_VISIBLE_CANDIDATES: u16 = 8;

/// Maximum number of authored trusted-root paths the added-roots
/// summary block renders verbatim. Anything beyond this collapses
/// into the localized `and N more` overflow line so the prompt
/// stays readable on narrow terminals and with many accepted roots.
const MAX_VISIBLE_ADDED_ROOTS: usize = 3;

/// Compact baseline rows the inline viewport always reserves,
/// regardless of which optional sections (summary, validation
/// error, candidates, no-match hint, scope, preview) are visible:
///
///   * 2 rows for the outer block border,
///   * 1 row for the prompt (wrap-aware extra rows are added below),
///   * 3 rows for the boxed input frame (top border + content + bottom border),
///   * 1 row for the footer hint.
///
/// Every other section is content-driven: it contributes rows
/// only when it actually has something to show, so the prompt
/// stays compact.
const COMPACT_BASE_ROWS: u16 = 7;

/// Upper bound on viewport rows reserved for a blocking
/// validation error threaded in by the caller. The slot renders
/// the localized error with `MessageSeverity::Danger` styling so
/// the user sees the failure inside the current UI instead of
/// relying on terminal scrollback.
///
/// The slot height is bounded so the boxed input and the footer
/// hint stay visible even when the caller surfaces a long localized
/// error or the terminal is very narrow. The error wraps inside this
/// bounded slot rather than being truncated at the right edge.
const MAX_VALIDATION_ERROR_HEIGHT: u16 = 4;

/// Fallback content width assumed when `terminal_size` cannot
/// report the current terminal dimensions (e.g. piped stderr). The
/// value matches the conventional 80-column assumption used by the
/// other inline screens (`text_input`, `confirm`).
const FALLBACK_CONTENT_WIDTH: u16 = 80;

/// Minimum content width considered when wrapping the validation
/// error. Anything narrower would produce so many wrap rows that
/// the input box would lose its remaining vertical real estate, so
/// we clamp the width here and let the bounded
/// `MAX_VALIDATION_ERROR_HEIGHT` cap absorb the rest.
const MIN_CONTENT_WIDTH: u16 = 20;

/// Outcome of a single `path_input::run` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathInputOutcome {
    /// User submitted a non-empty value with Enter.
    Submitted(String),
    /// User submitted an empty value with Enter (the caller decides
    /// whether that means "done adding entries" or "validation
    /// failure").
    EmptyEnter,
    /// User cancelled the current path entry with Esc. This cancels
    /// only the current field; Ctrl+C remains the global abort path.
    Cancelled,
}

/// Display options for the path input prompt.
///
/// The prompt must visually read as a real input field (dedicated
/// label + boxed input area + cursor), surface the current
/// browse-scope (`Current location: ~/`), and preview the
/// would-be-committed path (`Will add: ~/Downloads`). The wizard
/// supplies pre-localized strings for every label so this widget
/// stays purely presentational.
pub struct PathInputOptions<'a> {
    /// Prompt text shown above the input. Continues to drive the
    /// per-call wizard heading (e.g. "Add a trusted root").
    pub prompt: &'a str,
    /// Initial buffer contents. Must default to "" so the field reads
    /// as a real empty input box backed by the `placeholder`, not as
    /// a pre-filled `~/`.
    pub initial: &'a str,
    /// Localized footer hint shown at the bottom of the prompt
    /// (typically: "Tab to complete, ↑/↓ for candidates, Enter to confirm").
    pub footer_hint: &'a str,
    /// Localized message shown briefly when Tab finds no candidates.
    pub no_match_hint: &'a str,
    /// Localized hint rendered in the candidate panel after Tab
    /// completes the buffer to an existing directory path (ending in
    /// the platform separator) whose child directories returned an
    /// empty live-search result. Gives the user explicit browse
    /// feedback so the post-completion state never reads as a stalled
    /// blank panel. Empty string suppresses the slot entirely.
    pub dir_no_children_hint: &'a str,
    /// User's home directory, used by completion to expand `~/`.
    pub home: Option<&'a Utf8Path>,
    /// Visible label rendered above the boxed input area, e.g.
    /// `Trusted root path` (already localized by the caller).
    pub field_label: &'a str,
    /// Placeholder shown inside the boxed input when the buffer is
    /// empty so the user can tell the prompt is in "browse home"
    /// mode without having to type `~/` first.
    pub placeholder: &'a str,
    /// Template for the scope line. The literal token `{path}` is
    /// replaced by the resolved browse-parent (`~/`, `~/dev/`, `/srv/`).
    pub scope_label_template: &'a str,
    /// Template for the commit-preview line. The literal token
    /// `{path}` is replaced by the user-visible full path that
    /// pressing Enter would submit.
    pub preview_label_template: &'a str,
    /// Warning shown in place of a preview when the buffer is parked
    /// at a bare scope (`~`, `~/`, `/`) — those would either be
    /// rejected (relative-path-shaped) or commit a too-broad root.
    pub bare_scope_warning: &'a str,
    /// Warning shown when the buffer is non-empty but neither
    /// absolute nor `~/`-rooted. Trusted roots must be absolute.
    pub relative_warning: &'a str,
    /// Warning shown when the buffer is a *scoped filter* (typed
    /// bare-word under the visible home browse scope) but no
    /// candidate directory matches. Required behavior:
    /// 1. NO "will add: ~/<buffer>" preview is fabricated;
    /// 2. this warning replaces the preview line;
    /// 3. pressing Enter MUST NOT submit the value (the input screen
    ///    stays open until the user enters a path that actually
    ///    matches a candidate, or an explicit `~/...` / absolute path
    ///    that the caller's own validation accepts).
    ///
    /// The string may contain a single `{0}` placeholder that the
    /// caller has pre-rendered with the bare buffer (e.g. `ddd`).
    pub scope_no_match_warning: &'a str,
    /// Pre-localized blocking validation failure threaded in by
    /// the caller. Rendered with `MessageSeverity::Danger`
    /// styling above the input area so the user sees the failure
    /// inside the current UI instead of only in scrollback.
    /// `None` (or empty) hides the slot.
    ///
    /// Validation errors use the shared `error` / `danger` style.
    pub validation_error: Option<&'a str>,
    /// Summary of trusted roots already accepted in the current
    /// wizard flow. Rendered inside the path-input viewport so the
    /// user can see the running list without having to scroll the
    /// terminal history.
    ///
    /// Always passed (with an empty list, the summary collapses to
    /// the localized empty-state label) so the prompt geometry
    /// stays stable across iterations.
    ///
    /// Trusted-root wizard state is shown in the active input screen.
    pub added_summary: AddedRootsSummary<'a>,
}

/// Caller-supplied data the path-input renders inside its
/// added-roots summary block.
///
/// The wizard owns the data; this struct is purely a presentation
/// contract. All localizable copy is pre-resolved by the wizard so
/// the path-input stays language-agnostic.
#[derive(Debug, Clone)]
pub struct AddedRootsSummary<'a> {
    /// Localized template for the summary header. The token `{0}`
    /// is replaced with `paths.len()` (e.g.
    /// `"Trusted roots added (3):"`). When `paths` is empty the
    /// header is suppressed and `empty_label` is rendered instead.
    pub header_template: &'a str,
    /// Localized empty-state label shown when no trusted root has
    /// been accepted yet (e.g. `"No trusted roots added yet."`).
    pub empty_label: &'a str,
    /// Localized template for the compact overflow line that
    /// replaces hidden entries when the list is longer than
    /// `MAX_VISIBLE_ADDED_ROOTS`. The token `{0}` is replaced with
    /// the remainder count (e.g. `"… and 2 more"`).
    pub overflow_template: &'a str,
    /// Authored-form trusted-root paths in insertion order (oldest
    /// first). The renderer preserves the slice's order so users
    /// can see what they typed (e.g. `~/development`) instead of
    /// the canonicalized absolute form.
    pub paths: &'a [String],
    /// Index into `paths` flagging the most recently accepted root
    /// for `MessageSeverity::Preview` highlighting. `None` skips
    /// the highlight (initial entry, or after a rejection that did
    /// not extend the list).
    pub most_recent: Option<usize>,
}

impl AddedRootsSummary<'_> {
    /// Empty/no-op summary used in tests and call sites that have
    /// no localized copy yet. Renders nothing visible.
    ///
    /// Production callers always materialize a real summary (the
    /// wizard owns the localized templates). The const is kept on
    /// the public API so tests and any future non-wizard entry
    /// point can opt in without re-encoding the empty contract.
    #[cfg_attr(not(test), allow(dead_code))]
    pub const EMPTY: AddedRootsSummary<'static> = AddedRootsSummary {
        header_template: "",
        empty_label: "",
        overflow_template: "",
        paths: &[],
        most_recent: None,
    };
}

/// Run the path input prompt.
///
/// Blocks until the user submits with Enter, cancels with Esc (when
/// no candidate list is open), or aborts globally with Ctrl+C.
///
/// `strategy` selects the path-syntax model the prompt applies to
/// scope/preview/relative-warning calculations. Production callers
/// pass [`workhive_core::usecases::path_strategy::current_target_strategy`]
/// so macOS / Linux share `UnixPathStrategy` and Windows uses
/// `WindowsPathStrategy`.
pub fn run(
    options: PathInputOptions<'_>,
    strategy: &dyn PathStrategy,
    fs: &dyn PathCompletionFsPort,
) -> Result<PathInputOutcome, CliError> {
    let mut state = State::from_options(&options);
    // Live search runs immediately on entry so the user sees
    // relevant candidates without first pressing Tab. The
    // candidate-slot reservation is sized from the candidate
    // count the live search returned at entry: zero candidates
    // reserve zero rows, one candidate reserves one row, and
    // larger results saturate at `MAX_VISIBLE_CANDIDATES`. The
    // same value is reused as the renderer's candidate-list cap
    // (via `state.reserved_candidate_rows`), so live-search
    // growth during typing or after candidate acceptance scrolls
    // inside the slot via `candidate_window` instead of exceeding
    // the reserved candidate slot.
    state.refresh_live_search(options.home, strategy, fs);
    // Entry width only: no frame has been drawn yet, so the initial
    // spec is sized from the queried terminal width. Every later
    // layout decision uses the live render/resize width tracked in
    // `content_width` below, so a resize re-wraps and re-heights
    // against the new terminal width rather than this entry snapshot.
    let mut content_width = path_input_content_width();
    let spec = resolve_inline_viewport_spec(&options, state.candidates.len(), content_width);
    state.reserved_candidate_rows = spec.reserved_candidate_rows;
    state.viewport_height = spec.viewport_height;
    // Complex path entry — a live candidate list, a validation slot,
    // and a running added-roots summary — renders in a modal
    // alternate screen. There a terminal resize is handled by an
    // ordinary full-frame redraw of a clean canvas, instead of the
    // inline-viewport rebuild that left stale frames, repeated
    // borders, and stacked prompt fragments behind. The content is
    // anchored at the top of the screen via `modal_content_area` so
    // the prompt keeps its compact look rather than stretching to
    // fill the terminal.
    let mut session = TerminalSession::modal()?;

    loop {
        let snapshot = state.render_snapshot(&options, strategy);
        let content_height = state.viewport_height;
        let mut drawn_content_width = content_width;
        session
            .terminal()
            .draw(|frame| {
                let area = modal_content_area(frame.area(), content_height);
                // The width the modal actually drew into is the source
                // of truth for the next layout decision. Deriving it
                // from `area.width` keeps it consistent with the
                // renderer (which wraps against `outer.inner(area)`)
                // and lets a later Tab browse grow against the live
                // frame instead of the entry-time terminal.
                drawn_content_width = content_width_for_area(area.width);
                render::render(frame, area, &snapshot);
            })
            .map_err(|e| CliError::Runtime(e.to_string()))?;
        content_width = drawn_content_width;

        let ev = event::read().map_err(|e| CliError::Runtime(e.to_string()))?;
        if let Event::Resize(new_cols, _) = ev {
            // A terminal resize is a layout event, not input. The
            // resize reports the new terminal column count directly, so
            // derive the live content width from it (matching the outer
            // block the renderer draws) and recompute the
            // content-driven height for that width — the prompt,
            // validation error, and candidate layout all wrap against
            // it. The next loop iteration repaints the full alternate
            // screen and re-anchors the top content box at the new
            // height, so no stale frames, ghost borders, or jagged
            // candidate rows survive. Every editing field (buffer,
            // cursor, candidate list, selected candidate, validation,
            // summary) is preserved; the resize never submits, cancels,
            // or mutates the input.
            content_width = content_width_for_area(new_cols);
            state.on_terminal_resize(&options, content_width);
            continue;
        }
        let Event::Key(key) = ev else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Err(CliError::UserAborted);
        }

        match key.code {
            // Esc is intentionally MODAL: when a candidate list (or
            // no-match hint) is visible, Esc dismisses the popup so
            // the user can keep editing. Only Esc with the input area
            // empty of candidates returns Cancelled to the caller.
            KeyCode::Esc => {
                if state.dismiss_candidates_if_visible() {
                    continue;
                }
                tracing::debug!("path_input: Esc with no candidates open; cancelling field");
                return Ok(PathInputOutcome::Cancelled);
            }
            KeyCode::Enter => {
                // Enter is shell-like: it submits the input buffer,
                // never a non-applied highlighted candidate. Tab is
                // the only key that writes a candidate into the
                // buffer; this keeps the input field as the
                // authoritative submission state and matches the
                // footer copy `Enter add current path`.
                let submitted: String = state.buffer.iter().collect();
                if submitted.trim().is_empty() {
                    return Ok(PathInputOutcome::EmptyEnter);
                }
                // A scoped filter under the visible browse scope with
                // zero matching candidates is a UI-level no-match
                // state. Enter must NOT escape the picker; the
                // localized warning rendered in `render_snapshot`
                // stays visible until the user types a path that
                // either matches a candidate or is an explicit
                // `~/...` / absolute path.
                if state.scoped_filter_no_match {
                    tracing::debug!(
                        input = %submitted,
                        "path_input: Enter blocked — scoped filter has no match"
                    );
                    continue;
                }
                return Ok(PathInputOutcome::Submitted(submitted));
            }
            KeyCode::Tab => {
                state.run_completion(options.home, strategy, fs);
                // A Tab that browses into a directory refreshes the
                // candidate list to that directory's children. Grow
                // the candidate reservation so multiple children are
                // visible at once instead of being frozen at the
                // compact entry reservation. `content_width` carries
                // the width the last frame drew into, so a Tab after a
                // resize sizes the growth against the resized terminal,
                // not the entry-time width. The modal owns the full
                // screen, so the next full-frame redraw reflects the
                // new content height — no viewport rebuild is needed.
                let _ = state.grow_candidate_reservation_for_browsing(&options, content_width);
            }
            KeyCode::Backspace if state.cursor > 0 => {
                state.cursor -= 1;
                state.buffer.remove(state.cursor);
                state.refresh_live_search(options.home, strategy, fs);
            }
            KeyCode::Delete if state.cursor < state.buffer.len() => {
                state.buffer.remove(state.cursor);
                state.refresh_live_search(options.home, strategy, fs);
            }
            KeyCode::Left => {
                state.cursor = state.cursor.saturating_sub(1);
            }
            KeyCode::Right if state.cursor < state.buffer.len() => {
                state.cursor += 1;
            }
            KeyCode::Up => {
                state.move_candidate_up();
            }
            KeyCode::Down => {
                state.move_candidate_down();
            }
            KeyCode::Char(c) => {
                state.buffer.insert(state.cursor, c);
                state.cursor += 1;
                state.refresh_live_search(options.home, strategy, fs);
            }
            _ => {}
        }
    }
}

/// Compute the content-driven inline viewport height required to
/// host the prompt for the supplied options at `content_width`
/// (outer terminal width, in cells).
///
/// Optional sections only contribute rows when they have visible
/// content. The total is:
///
/// ```text
/// COMPACT_BASE_ROWS                        // borders + prompt + input(3) + footer
///   + extra_prompt_wrap_rows               // long localized prompts
///   + added_summary_rows(opts)             // 0 unless the summary actually has content
///   + validation_error_wrapped_rows        // 0 unless an error is threaded in
///   + scope_row                            // 0 unless a scope can be derived
///   + preview_row                          // 0 unless a preview / warning line is present
///   + reserved_candidate_rows              // candidate slot reservation (bounded by MAX_VISIBLE_CANDIDATES)
/// ```
///
/// `reserved_candidate_rows` is the stable upper bound on rows
/// the candidate slot may consume. The renderer must cap its
/// candidate list at this same value so live-search growth during
/// typing or candidate acceptance never exceeds the inline
/// viewport reservation.
pub(crate) fn compute_viewport_height(
    opts: &PathInputOptions<'_>,
    reserved_candidate_rows: u16,
    content_width: u16,
) -> u16 {
    let summary_rows = added_summary_visible_rows(&opts.added_summary);
    let validation_rows = validation_error_wrapped_height(opts.validation_error, content_width);
    let scope_rows: u16 = if scope_is_visible(opts) { 1 } else { 0 };
    let preview_rows: u16 = if preview_is_visible(opts) { 1 } else { 0 };
    let prompt_extra = prompt_extra_wrap_rows(opts.prompt, content_width);
    let candidate_rows: u16 = reserved_candidate_rows.min(MAX_VISIBLE_CANDIDATES);

    COMPACT_BASE_ROWS
        .saturating_add(prompt_extra)
        .saturating_add(summary_rows)
        .saturating_add(validation_rows)
        .saturating_add(scope_rows)
        .saturating_add(preview_rows)
        .saturating_add(candidate_rows)
}

/// Candidate-slot reservation derived from the initial live-search
/// candidate count.
///
/// The same value is used both as the inline-viewport reservation
/// passed to [`compute_viewport_height`] AND as the cap the
/// renderer applies to the candidate list it draws. Sharing one
/// source of truth keeps render-time candidate rows from ever
/// exceeding the slot the inline session reserved at construction
/// time, since the inline viewport cannot grow afterwards.
///
/// The reservation is content-aware: it tracks the candidate
/// count seen at session entry, capped at
/// [`MAX_VISIBLE_CANDIDATES`]. Specifically:
///
///   * `0` when the initial live search returned no candidates,
///     so a prompt that has nothing to surface adds no candidate
///     rows to the inline viewport at all;
///   * `n` for `1 <= n <= MAX_VISIBLE_CANDIDATES`, so the slot
///     fits exactly the candidates the user can see;
///   * [`MAX_VISIBLE_CANDIDATES`] otherwise.
///
/// Live-search growth during typing or after candidate acceptance
/// is handled inside the same reservation: [`candidate_window`]
/// scrolls within the slot with the highlighted candidate kept
/// visible, never expanding past the reservation.
pub(crate) fn reservation_from_initial_candidates(initial_candidate_count: usize) -> u16 {
    initial_candidate_count.min(MAX_VISIBLE_CANDIDATES as usize) as u16
}

/// Content sizing the production [`run`] path locks in after the
/// initial live search returns. The renderer caps its candidate
/// list at `reserved_candidate_rows`, and `viewport_height` is the
/// total content height the modal screen anchors at the top of the
/// frame, so the two values together pin the rendered content size
/// for the lifetime of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InlineViewportSpec {
    pub reserved_candidate_rows: u16,
    pub viewport_height: u16,
}

/// Resolve the inline viewport sizing for the supplied options
/// after the live search returned `initial_candidate_count`
/// candidates.
///
/// This is the helper the production [`run`] path calls to lock
/// in `state.reserved_candidate_rows` and `state.viewport_height`
/// after the initial `refresh_live_search`. Tests drive the same
/// helper (with a `FakeFs`-backed live search) so the runtime
/// reservation and the renderer cap are exercised end-to-end,
/// not only as pure-helper return values.
pub(crate) fn resolve_inline_viewport_spec(
    opts: &PathInputOptions<'_>,
    initial_candidate_count: usize,
    content_width: u16,
) -> InlineViewportSpec {
    let reserved_candidate_rows = reservation_from_initial_candidates(initial_candidate_count);
    let viewport_height = compute_viewport_height(opts, reserved_candidate_rows, content_width);
    InlineViewportSpec {
        reserved_candidate_rows,
        viewport_height,
    }
}

/// Whether the rendered scope row needs a reserved slot in the
/// locked inline viewport for the given options.
///
/// The inline viewport is sized once at session construction and
/// cannot grow afterwards: any slot that may light up later as the
/// user types must be reserved up-front. The trailing
/// `Constraint::Min(0)` in `render` absorbs the reserved slot when
/// the live snapshot has nothing to show there, so reserving
/// unconditionally is safe.
///
/// The scope row lights up whenever the prompt has a home directory
/// (the home browse scope can be derived immediately) OR can light
/// up later once the user types an absolute or `~/`-rooted buffer.
/// Either way the row must be reserved at construction time.
fn scope_is_visible(_opts: &PathInputOptions<'_>) -> bool {
    true
}

/// Whether the rendered preview/warning row needs a reserved slot
/// in the locked inline viewport for the given options.
///
/// The preview/warning row echoes the commit preview, scoped-filter
/// warning, relative-path warning, or bare-scope warning derived
/// from the live buffer. Even when `initial` is empty at
/// construction time, the user can light the slot up by typing a
/// single character. The inline viewport cannot grow afterwards, so
/// the slot must be reserved up-front; the trailing
/// `Constraint::Min(0)` in `render` absorbs the reserved slot when
/// the snapshot has nothing to show.
fn preview_is_visible(_opts: &PathInputOptions<'_>) -> bool {
    true
}

/// Number of additional rows the prompt line needs beyond the base
/// 1 row when it wraps. Used by [`compute_viewport_height`] so long
/// localized prompts do not push the boxed input out of the
/// viewport on narrow terminals.
fn prompt_extra_wrap_rows(prompt: &str, content_width: u16) -> u16 {
    let inner = content_width.saturating_sub(2).max(1) as usize;
    let lines = wrapped_line_count(prompt, inner) as u16;
    lines.saturating_sub(1)
}

/// Actual number of rows the rendered added-roots summary will
/// occupy for the supplied caller data. Always `0` when the summary
/// has no visible content (no paths AND no empty-state label).
///
/// The shape mirrors `render_added_summary`: at most one header
/// row, up to [`MAX_VISIBLE_ADDED_ROOTS`] path rows OR one empty-
/// state label row, plus one optional overflow row when the path
/// list was truncated.
pub(crate) fn added_summary_visible_rows(summary: &AddedRootsSummary<'_>) -> u16 {
    let total = summary.paths.len();
    if total == 0 {
        if summary.empty_label.is_empty() {
            return 0;
        }
        // Empty-state label only.
        return 1;
    }
    let visible = total.min(MAX_VISIBLE_ADDED_ROOTS);
    let header = if summary.header_template.is_empty() {
        0
    } else {
        1
    };
    let overflow = if total > visible { 1 } else { 0 };
    (header + visible as u16 + overflow).min(MAX_VISIBLE_ADDED_ROOTS as u16 + 2)
}

/// Derive the wrap-relevant path-input content width from the width
/// of the area the prompt is drawn into.
///
/// The prompt renders inside an outer block that borrows one cell of
/// border on each side, so the wrap-relevant width is
/// `area_width - 2`, floored at [`MIN_CONTENT_WIDTH`] so a narrow
/// terminal still leaves a usable input row. This is the single
/// formula every layout decision shares: the entry-time terminal
/// query, the live frame width captured on each draw, and the new
/// column count a resize reports all flow through here, so
/// [`compute_viewport_height`] always agrees with the renderer, which
/// wraps against `outer.inner(area).width`.
pub(crate) fn content_width_for_area(area_width: u16) -> u16 {
    area_width.saturating_sub(2).max(MIN_CONTENT_WIDTH)
}

/// Read the current terminal content width for the initial layout.
///
/// Used once at entry, before any frame has been drawn: there is no
/// render area to measure yet, so the width is taken from
/// `terminal_size`. When `terminal_size` cannot report the dimensions
/// (e.g. stderr is piped, common in headless CI) we fall back to a
/// conventional 80-column assumption. Every later layout decision
/// derives its width from the live render area via
/// [`content_width_for_area`] instead.
pub(crate) fn path_input_content_width() -> u16 {
    let raw = terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(FALLBACK_CONTENT_WIDTH);
    content_width_for_area(raw)
}

/// Number of viewport rows reserved for the wrapped validation
/// error at `content_width`. Zero when there is nothing to show.
/// Capped at `MAX_VALIDATION_ERROR_HEIGHT` so the input box and
/// footer remain usable on narrow terminals.
pub(crate) fn validation_error_wrapped_height(error: Option<&str>, content_width: u16) -> u16 {
    let Some(msg) = error.filter(|m| !m.is_empty()) else {
        return 0;
    };
    // The rendered error is formatted as `" {msg} "` so the wrap
    // budget is two cells smaller than the slot width.
    let wrap_width = content_width.saturating_sub(2).max(1) as usize;
    let lines = wrapped_line_count(msg, wrap_width) as u16;
    lines.clamp(1, MAX_VALIDATION_ERROR_HEIGHT)
}

/// Greedy display-width line counter mirroring the wrapping
/// strategy used by `text_input` and `confirm`. Counts grapheme
/// cells (CJK fullwidth aware) and treats explicit `\n` as a hard
/// break. Always returns at least 1 so a non-empty string never
/// reserves zero rows.
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
mod tests;

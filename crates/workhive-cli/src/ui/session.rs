//! Single terminal lifecycle owner.
//!
//! `TerminalSession` is the ONLY place that enters/exits raw mode,
//! creates ratatui terminals, hides/shows the cursor, and installs
//! panic-safe restore logic.
//!
//! No other code may call `ratatui::init_*()`, `ratatui::restore()`,
//! `enable_raw_mode()`, or `disable_raw_mode()`.

use crossterm::{
    cursor::{self, MoveTo, MoveToNextLine},
    execute,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use std::io::{self, Stderr};
use std::panic;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::CliError;

type RatatuiTerminal = Terminal<CrosstermBackend<Stderr>>;

/// Tracks whether the single terminal session currently owns the
/// alternate screen.
///
/// The panic hook and [`TerminalSession::drop`] consult this so the
/// main screen is restored exactly once — on panic, error, abort, or
/// normal completion — without any screen component ever issuing
/// `EnterAlternateScreen` / `LeaveAlternateScreen` itself. The
/// terminal is a single global resource, so a single flag is
/// sufficient: at most one session is live at a time.
static ALTERNATE_SCREEN_ACTIVE: AtomicBool = AtomicBool::new(false);

/// The single owner of terminal state during an interactive session.
///
/// Created via [`TerminalSession::inline`] for inline viewport
/// rendering, or [`TerminalSession::modal`] for a full alternate-
/// screen drawing surface used by complex selectors and path input.
/// On drop, the terminal is unconditionally restored to normal mode,
/// leaving the alternate screen when one was entered.
pub struct TerminalSession {
    terminal: RatatuiTerminal,
    use_alternate_screen: bool,
    viewport_height: u16,
}

/// Captured layout of an inline ratatui viewport — top row and total
/// height in terminal cells — used by [`TerminalSession::drop`] to
/// position the cursor at a deterministic location relative to the
/// rendered viewport before disabling raw mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InlineLayout {
    top_row: u16,
    height: u16,
}

impl InlineLayout {
    /// Bottom-most terminal row occupied by the inline viewport.
    ///
    /// Teardown moves the cursor here and then emits a single line
    /// feed. A line feed advances to the row directly below the
    /// viewport when space remains, and scrolls the screen up by one
    /// row when this bottom row is the terminal's last line — so the
    /// next byte of CLI output always lands on a fresh line below the
    /// viewport instead of clamping onto and overwriting it.
    fn last_row(&self) -> u16 {
        self.top_row.saturating_add(self.height.saturating_sub(1))
    }
}

/// A single terminal operation in the inline-viewport teardown
/// sequence.
///
/// Modelling teardown as a typed plan keeps its bottom-edge behavior
/// (viewport flush against the terminal bottom vs. space remaining
/// below) unit-testable without a live terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TeardownStep {
    /// Move the cursor to column 0 of the given absolute row. An
    /// absolute move clamps at the terminal's bottom edge and never
    /// scrolls.
    MoveToRow(u16),
    /// Emit one line feed. This advances the cursor one row, or
    /// scrolls the screen up by one row when the cursor already sits
    /// on the terminal's last line.
    LineFeed,
}

/// Build the inline-viewport teardown plan: move to the viewport's
/// bottom row, then emit a line feed so the cursor ends on a fresh
/// line directly below the viewport — scrolling the screen when the
/// viewport is flush against the terminal's bottom edge.
fn inline_teardown_plan(layout: InlineLayout) -> [TeardownStep; 2] {
    [
        TeardownStep::MoveToRow(layout.last_row()),
        TeardownStep::LineFeed,
    ]
}

/// A single terminal operation in the inline-viewport clear
/// sequence used by [`TerminalSession::reset_inline_viewport`] when
/// it rebuilds the viewport after a resize.
///
/// Modelling the clear as a typed plan keeps its coverage — every
/// occupied row, including the footer/help last row — unit-testable
/// without a live terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum InlineClearStep {
    /// Move the cursor to column 0 of the given absolute row.
    MoveToRow(u16),
    /// Erase the entire row the cursor currently sits on.
    ClearRow,
}

/// Build the inline-viewport clear plan: erase every row the
/// viewport occupies from its top row through its footer/help last
/// row, then park the cursor at the top row.
///
/// Parking at the top row (rather than below the viewport) lets a
/// prompt that is immediately re-entered in the same flow reuse the
/// freed region: the next inline viewport ratatui reserves relative
/// to the cursor opens over the cleared rows instead of stacking a
/// second frame below them.
#[allow(dead_code)]
fn inline_clear_plan(layout: InlineLayout) -> Vec<InlineClearStep> {
    let top = layout.top_row;
    let last = layout.last_row();
    let mut steps = Vec::with_capacity(usize::from(layout.height) * 2 + 1);
    let mut row = top;
    loop {
        steps.push(InlineClearStep::MoveToRow(row));
        steps.push(InlineClearStep::ClearRow);
        if row == last {
            break;
        }
        row = row.saturating_add(1);
    }
    steps.push(InlineClearStep::MoveToRow(top));
    steps
}

/// Run an [`inline_clear_plan`] for `layout` against `stderr`.
///
/// Returns `false` when the backend rejected an operation, so the
/// caller can fall back instead of assuming the region was cleared.
#[allow(dead_code)]
fn execute_inline_clear(stderr: &mut Stderr, layout: InlineLayout) -> bool {
    for step in inline_clear_plan(layout) {
        let result = match step {
            InlineClearStep::MoveToRow(row) => execute!(stderr, MoveTo(0, row)),
            InlineClearStep::ClearRow => execute!(stderr, Clear(ClearType::CurrentLine)),
        };
        if result.is_err() {
            return false;
        }
    }
    true
}

impl TerminalSession {
    /// Create an inline viewport session occupying `height` rows below the cursor.
    ///
    /// Kept for narrow prompts that intentionally need to remain in
    /// shell scrollback.
    #[allow(dead_code)]
    pub fn inline(height: u16) -> Result<Self, CliError> {
        install_panic_hook();

        terminal::enable_raw_mode().map_err(|e| CliError::Runtime(e.to_string()))?;
        let mut init_guard = InitGuard::new();

        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(height),
            },
        )
        .map_err(|e| CliError::Runtime(e.to_string()))?;
        init_guard.disarm();

        Ok(Self {
            terminal,
            use_alternate_screen: false,
            viewport_height: height,
        })
    }

    /// Create a modal session that owns the full terminal as an
    /// isolated alternate-screen drawing surface.
    ///
    /// Complex interactive surfaces — bordered selectors and the
    /// path-entry prompt with its live candidate list — render here.
    /// The alternate screen is a clean canvas the renderer fully
    /// repaints every frame, so a terminal resize is handled by an
    /// ordinary full-frame redraw rather than by erasing and
    /// rebuilding an inline viewport in scrollback. That removes the
    /// stale frames, repeated borders, and stacked prompt fragments
    /// that inline rendering left behind on resize.
    ///
    /// The full terminal is the canvas, but screens keep their
    /// compact look by anchoring content at the top via
    /// [`modal_content_area`] instead of stretching to fill the
    /// screen. Raw mode, the hidden cursor, and the panic hook are
    /// installed exactly as for an inline session; on drop the
    /// alternate screen is left and the main shell is restored.
    pub fn modal() -> Result<Self, CliError> {
        install_panic_hook();

        terminal::enable_raw_mode().map_err(|e| CliError::Runtime(e.to_string()))?;
        let mut init_guard = InitGuard::new();

        let mut stderr = io::stderr();
        execute!(stderr, EnterAlternateScreen).map_err(|e| CliError::Runtime(e.to_string()))?;
        // Record ownership before any fallible step that follows, so a
        // failure here is unwound through the same single-leave restore
        // path as a panic or normal drop.
        ALTERNATE_SCREEN_ACTIVE.store(true, Ordering::SeqCst);

        let backend = CrosstermBackend::new(io::stderr());
        let mut terminal = Terminal::new(backend).map_err(|e| CliError::Runtime(e.to_string()))?;
        // Force a clean first frame even on terminals that do not blank
        // the alternate buffer on entry.
        terminal
            .clear()
            .map_err(|e| CliError::Runtime(e.to_string()))?;
        init_guard.disarm();

        Ok(Self {
            terminal,
            use_alternate_screen: true,
            viewport_height: 0,
        })
    }

    /// Access the underlying ratatui terminal for drawing.
    pub fn terminal(&mut self) -> &mut RatatuiTerminal {
        &mut self.terminal
    }

    /// Clear the current inline viewport region and re-reserve it at
    /// `new_height`, anchored at the current top row — even when
    /// `new_height` equals the current height.
    ///
    /// This is the entry point for handling a terminal resize event.
    /// A width-only resize leaves the row count unchanged but reflows
    /// the backend, so the viewport must still be erased and the
    /// ratatui terminal rebuilt: rebuilding re-queries the backend for
    /// its new dimensions and the next draw fully repaints the region,
    /// leaving no ghost borders, stacked frames, jagged table lines, or
    /// stale footer/help rows from the pre-resize layout. Callers pass
    /// the height recomputed for the new terminal width so a resize
    /// that changes how the prompt wraps re-reserves the correct row
    /// count in the same step. A no-op only in alternate-screen mode.
    #[allow(dead_code)]
    pub fn reset_inline_viewport(&mut self, new_height: u16) -> Result<(), CliError> {
        if self.use_alternate_screen {
            return Ok(());
        }
        self.rebuild_inline_viewport(new_height)
    }

    /// Erase the current inline viewport region and rebuild the
    /// ratatui terminal at `new_height`, anchored at the current top
    /// row.
    ///
    /// Backing implementation of [`Self::reset_inline_viewport`].
    /// ratatui fixes an inline viewport's height when the terminal is
    /// constructed, so changing the row count — or forcing a clean
    /// repaint after a backend reflow — requires rebuilding the
    /// terminal. Raw mode and the panic hook stay installed; only the
    /// ratatui terminal and the tracked row count change.
    #[allow(dead_code)]
    fn rebuild_inline_viewport(&mut self, new_height: u16) -> Result<(), CliError> {
        // Erase every row the current inline viewport occupies —
        // including its footer/help last row — before re-reserving at
        // the new height, so changing the row count leaves no ghost
        // rows from the prior viewport. The clear plan parks the
        // cursor at the viewport's top row; ratatui reserves an inline
        // viewport relative to the cursor, so the rebuilt region
        // anchors over the same rows instead of opening below them.
        let top_row = self.terminal.get_frame().area().y;
        let old_layout = InlineLayout {
            top_row,
            height: self.viewport_height,
        };
        let mut stderr = io::stderr();
        if !execute_inline_clear(&mut stderr, old_layout) {
            // A degraded backend rejected a clear op. Re-anchor at the
            // top row so the rebuilt viewport still opens over the old
            // region; the next draw repaints it.
            execute!(stderr, MoveTo(0, top_row)).map_err(|e| CliError::Runtime(e.to_string()))?;
        }

        let backend = CrosstermBackend::new(io::stderr());
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(new_height),
            },
        )
        .map_err(|e| CliError::Runtime(e.to_string()))?;
        self.terminal = terminal;
        self.viewport_height = new_height;
        Ok(())
    }

    /// Explicitly finish the session and restore the terminal.
    ///
    /// Prefer letting `Drop` handle this, but this method allows explicit
    /// control when you need to print after restore.
    #[allow(dead_code)]
    pub fn finish(self) {
        // Drop handles cleanup
    }

    /// Read the current inline viewport layout from ratatui's
    /// tracked area. The viewport top row reflects the row where
    /// the renderer reserved screen space at construction time
    /// (after any backend-side scrolling that ratatui performed to
    /// fit the viewport), and the height is the row count reserved
    /// at construction time. Returns `None` when this session is
    /// not in inline mode or has zero rows.
    fn inline_layout(&mut self) -> Option<InlineLayout> {
        if self.use_alternate_screen || self.viewport_height == 0 {
            return None;
        }
        // `get_frame` only reads the tracked viewport area; it does
        // not paint and has no observable side effect on the
        // backend, so calling it from `Drop` is safe.
        let area = self.terminal.get_frame().area();
        Some(InlineLayout {
            top_row: area.y,
            height: self.viewport_height,
        })
    }
}

/// Compute the top-anchored content rectangle a modal screen draws
/// into.
///
/// A [`TerminalSession::modal`] session owns the full alternate
/// screen, but complex prompts keep their compact CLI look by
/// anchoring content at the top instead of stretching to fill the
/// screen. The returned rectangle:
///
///   * starts at the frame's top-left origin;
///   * spans the full frame width (safe on narrow terminals — it is
///     never wider than the frame);
///   * is `desired_height` rows tall, clamped to the frame height so
///     a content block taller than the terminal is capped at the
///     visible rows. The caller scrolls long content within that
///     area rather than overflowing the screen.
///
/// A `desired_height` of zero is treated as one row so a screen
/// always has at least a single drawable row. The unused region
/// below the returned rectangle is cleared by the modal's
/// full-frame redraw, so a resize never leaves stale rows behind.
pub fn modal_content_area(full: Rect, desired_height: u16) -> Rect {
    let max_height = full.height.max(1);
    let height = desired_height.clamp(1, max_height);
    Rect {
        x: full.x,
        y: full.y,
        width: full.width,
        height,
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let layout = self.inline_layout();
        let mut stderr = io::stderr();

        // Inline-viewport teardown: leave the last rendered frame in
        // place and move to a fresh line directly below it. After the
        // last `Terminal::draw`, ratatui leaves the terminal cursor
        // wherever the most recent cell write landed — often somewhere
        // in the middle or at the bottom of the viewport, not at the
        // top. Subsequent CLI output must begin on a fresh line
        // directly below the viewport with no blank gap, and must
        // never overwrite the viewport, including when the viewport is
        // flush against the terminal's bottom edge. Move to the
        // viewport's bottom row and emit a single line feed. A line
        // feed advances exactly one row when space remains below the
        // viewport, and scrolls the screen up by one row when the
        // cursor is already on the terminal's last line. An absolute
        // or relative cursor move (e.g. `MoveTo`/`MoveToNextLine`)
        // clamps at the bottom edge and would land back on the
        // viewport's last line, overwriting it. The line feed is also
        // immediate, so it adds no teardown delay after a raw-mode
        // confirmation prompt — unlike `Terminal::insert_before`,
        // which is intentionally avoided here.
        if let Some(layout) = layout {
            for step in inline_teardown_plan(layout) {
                let result = match step {
                    TeardownStep::MoveToRow(row) => execute!(stderr, MoveTo(0, row)),
                    TeardownStep::LineFeed => execute!(stderr, Print("\n")),
                };
                if result.is_err() {
                    // Degraded backend rejected the operation (very
                    // unusual on tmux/iTerm/wezterm/macOS Terminal).
                    // Fall back to a relative move so the cursor is
                    // still pushed past the inline area instead of
                    // colliding with the rendered viewport.
                    execute!(stderr, MoveToNextLine(layout.height)).ok();
                    break;
                }
            }
        }

        // Leave the alternate screen exactly once. `swap` makes this
        // a no-op if the panic hook already restored the main screen
        // during unwinding, so a panic that drops this session does
        // not emit a second `LeaveAlternateScreen`.
        if self.use_alternate_screen && ALTERNATE_SCREEN_ACTIVE.swap(false, Ordering::SeqCst) {
            execute!(stderr, LeaveAlternateScreen).ok();
        }
        execute!(stderr, cursor::Show).ok();
        terminal::disable_raw_mode().ok();
    }
}

/// Install a panic hook that restores the terminal before printing the panic.
///
/// This prevents the user from being left with a broken terminal if a panic
/// occurs during an interactive session.
fn install_panic_hook() {
    static PANIC_HOOK_ONCE: Once = Once::new();
    PANIC_HOOK_ONCE.call_once(|| {
        let original_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic_info| {
            restore_terminal_state();
            original_hook(panic_info);
        }));
    });
}

fn restore_terminal_state() {
    let mut stderr = io::stderr();
    // Leave the alternate screen only when a modal session owns it,
    // and only once: `swap` clears the flag so a subsequent `Drop`
    // during unwinding does not emit a second `LeaveAlternateScreen`.
    if ALTERNATE_SCREEN_ACTIVE.swap(false, Ordering::SeqCst) {
        execute!(stderr, LeaveAlternateScreen).ok();
    }
    execute!(stderr, cursor::Show).ok();
    terminal::disable_raw_mode().ok();
}

struct InitGuard {
    active: bool,
}

impl InitGuard {
    fn new() -> Self {
        let mut stderr = io::stderr();
        execute!(stderr, cursor::Hide).ok();
        Self { active: true }
    }

    fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for InitGuard {
    fn drop(&mut self) {
        if self.active {
            restore_terminal_state();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InlineClearStep, InlineLayout, TeardownStep, inline_clear_plan, inline_teardown_plan,
        modal_content_area,
    };
    use ratatui::layout::Rect;

    /// Minimal model of how a VT-style terminal responds to the
    /// teardown steps, so the bottom-edge behavior can be verified
    /// without a live terminal.
    ///
    /// Faithful to the two semantics the teardown relies on:
    ///
    /// * an absolute cursor move clamps at the last row and never
    ///   scrolls;
    /// * a line feed advances one row, or — when the cursor is
    ///   already on the last row — scrolls the screen up by one row
    ///   and keeps the cursor on the (now fresh) last row.
    struct TerminalModel {
        rows: u16,
        cursor_row: u16,
        scrolled_rows: u16,
    }

    impl TerminalModel {
        fn new(rows: u16, cursor_row: u16) -> Self {
            Self {
                rows,
                cursor_row,
                scrolled_rows: 0,
            }
        }

        fn apply(&mut self, step: TeardownStep) {
            match step {
                TeardownStep::MoveToRow(row) => {
                    self.cursor_row = row.min(self.rows.saturating_sub(1));
                }
                TeardownStep::LineFeed => {
                    if self.cursor_row.saturating_add(1) >= self.rows {
                        self.scrolled_rows = self.scrolled_rows.saturating_add(1);
                        self.cursor_row = self.rows.saturating_sub(1);
                    } else {
                        self.cursor_row += 1;
                    }
                }
            }
        }

        fn run(&mut self, steps: impl IntoIterator<Item = TeardownStep>) {
            for step in steps {
                self.apply(step);
            }
        }
    }

    /// When the inline viewport has rows beneath it, teardown leaves
    /// the cursor exactly one row below the viewport with no scroll,
    /// so subsequent output is flush against the viewport and leaves
    /// no blank gap.
    #[test]
    fn teardown_lands_one_row_below_viewport_when_space_remains() {
        // 24-row terminal; viewport occupies rows 10..=14.
        let layout = InlineLayout {
            top_row: 10,
            height: 5,
        };
        // Cursor left somewhere inside the viewport after the last draw.
        let mut term = TerminalModel::new(24, 12);
        term.run(inline_teardown_plan(layout));
        assert_eq!(
            term.cursor_row, 15,
            "cursor must rest directly below the viewport (row 15)"
        );
        assert_eq!(
            term.scrolled_rows, 0,
            "no scroll is needed when rows remain below the viewport"
        );
    }

    /// When the inline viewport is flush against the terminal's
    /// bottom edge, teardown must scroll the screen up exactly one
    /// row and leave the cursor on the freshly opened last line, so
    /// later output never overwrites the viewport's last line.
    #[test]
    fn teardown_scrolls_one_row_when_viewport_is_flush_with_bottom() {
        // 24-row terminal; viewport occupies the bottom rows 19..=23.
        let layout = InlineLayout {
            top_row: 19,
            height: 5,
        };
        let mut term = TerminalModel::new(24, 22);
        term.run(inline_teardown_plan(layout));
        assert_eq!(
            term.scrolled_rows, 1,
            "a bottom-flush viewport must scroll up exactly one line to open a fresh line"
        );
        assert_eq!(
            term.cursor_row, 23,
            "cursor must rest on the freshly scrolled last line"
        );
    }

    /// A single-row inline viewport sitting on the terminal's last
    /// line still scrolls once on teardown, proving the line feed —
    /// not the cursor move — is what opens the fresh line.
    #[test]
    fn teardown_scrolls_single_row_viewport_at_bottom() {
        let layout = InlineLayout {
            top_row: 23,
            height: 1,
        };
        let mut term = TerminalModel::new(24, 23);
        term.run(inline_teardown_plan(layout));
        assert_eq!(term.scrolled_rows, 1);
        assert_eq!(term.cursor_row, 23);
    }

    /// Boundary contrast: an absolute move to the row below the
    /// viewport (the prior teardown) cannot scroll and clamps onto
    /// the viewport's last line — which is exactly the overwrite the
    /// line-feed teardown fixes.
    #[test]
    fn absolute_move_alone_clamps_onto_viewport_and_cannot_scroll() {
        // Same bottom-flush viewport as the scroll test.
        let layout = InlineLayout {
            top_row: 19,
            height: 5,
        };
        let mut term = TerminalModel::new(24, 22);
        // The row directly below the viewport is 24, off-screen, so
        // an absolute move clamps to the last row (23).
        term.apply(TeardownStep::MoveToRow(
            layout.top_row.saturating_add(layout.height),
        ));
        assert_eq!(
            term.scrolled_rows, 0,
            "an absolute move never scrolls the screen"
        );
        assert_eq!(
            term.cursor_row, 23,
            "the move clamps onto the viewport's last line, demonstrating the overwrite the line feed avoids"
        );
    }

    /// Replay an [`inline_clear_plan`] and record which rows were
    /// erased and where the cursor finished, so the clear coverage is
    /// verifiable without a live terminal.
    fn replay_clear(steps: impl IntoIterator<Item = InlineClearStep>) -> (Vec<u16>, Option<u16>) {
        let mut cursor: Option<u16> = None;
        let mut cleared = Vec::new();
        for step in steps {
            match step {
                InlineClearStep::MoveToRow(row) => cursor = Some(row),
                InlineClearStep::ClearRow => {
                    let row = cursor.expect("ClearRow must follow a MoveToRow");
                    cleared.push(row);
                }
            }
        }
        (cleared, cursor)
    }

    /// The clear plan must erase every row the viewport occupies —
    /// from its top row through its footer/help last row, with none
    /// skipped — and then park the cursor at the viewport's top row so
    /// a re-entered prompt reuses the freed region.
    #[test]
    fn clear_plan_erases_every_viewport_row_including_footer_and_parks_at_top() {
        // Viewport occupies rows 6..=12 (height 7); row 12 is the
        // footer/help last row.
        let layout = InlineLayout {
            top_row: 6,
            height: 7,
        };
        let (cleared, cursor) = replay_clear(inline_clear_plan(layout));
        assert_eq!(
            cleared,
            vec![6, 7, 8, 9, 10, 11, 12],
            "every viewport row from top through the footer/help last row must be cleared, none skipped"
        );
        assert_eq!(
            *cleared.last().unwrap(),
            layout.last_row(),
            "the footer/help last row must be cleared"
        );
        assert_eq!(
            cursor,
            Some(layout.top_row),
            "the cursor must be parked at the viewport's top row after clearing"
        );
    }

    /// A single-row inline viewport clears exactly its one row and
    /// parks the cursor on it.
    #[test]
    fn clear_plan_handles_single_row_viewport() {
        let layout = InlineLayout {
            top_row: 4,
            height: 1,
        };
        let (cleared, cursor) = replay_clear(inline_clear_plan(layout));
        assert_eq!(cleared, vec![4]);
        assert_eq!(cursor, Some(4));
    }

    /// Re-entry coverage: clearing a tall viewport then re-clearing a
    /// shorter one anchored at the same top row leaves no row of the
    /// tall viewport un-erased across the two passes — the footer/help
    /// row of the first frame is always within the first clear's
    /// range, so it cannot survive as a ghost.
    #[test]
    fn clear_plan_first_pass_covers_rows_a_shorter_reentry_would_leave() {
        let tall = InlineLayout {
            top_row: 6,
            height: 7,
        };
        let short = InlineLayout {
            top_row: 6,
            height: 3,
        };
        let (tall_cleared, _) = replay_clear(inline_clear_plan(tall));
        let (short_cleared, _) = replay_clear(inline_clear_plan(short));
        for row in tall.top_row..=tall.last_row() {
            let covered = tall_cleared.contains(&row) || short_cleared.contains(&row);
            assert!(
                covered,
                "row {row} of the first frame must be cleared by the first pass so no footer/help ghost survives"
            );
        }
    }

    /// A content block that fits inside the frame is anchored at the
    /// top-left origin, spans the full frame width, and keeps its
    /// requested height — so a compact prompt stays compact instead
    /// of stretching to fill the alternate screen.
    #[test]
    fn modal_content_area_anchors_top_and_keeps_requested_height_when_it_fits() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let area = modal_content_area(full, 8);
        assert_eq!(
            area.x, full.x,
            "content must start at the frame's left edge"
        );
        assert_eq!(
            area.y, full.y,
            "content must be anchored at the frame's top"
        );
        assert_eq!(area.width, full.width, "content must span the full width");
        assert_eq!(area.height, 8, "a fitting block keeps its requested height");
    }

    /// A content block taller than the frame is capped at the visible
    /// rows, so a long list never overflows the screen — the caller
    /// scrolls within the returned area instead.
    #[test]
    fn modal_content_area_caps_height_at_frame_height() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 10,
        };
        let area = modal_content_area(full, 100);
        assert_eq!(
            area.height, 10,
            "content taller than the frame is capped at the visible rows"
        );
        assert_eq!(area.width, 40, "the width still spans the narrow frame");
    }

    /// A zero requested height still yields one drawable row, so a
    /// screen always has somewhere to render.
    #[test]
    fn modal_content_area_zero_height_yields_one_row() {
        let full = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 24,
        };
        let area = modal_content_area(full, 0);
        assert_eq!(area.height, 1, "a zero request still yields one row");
    }

    /// A nonzero frame offset is preserved: content anchors at the
    /// frame's own origin, not at absolute (0, 0).
    #[test]
    fn modal_content_area_preserves_frame_origin() {
        let full = Rect {
            x: 3,
            y: 2,
            width: 50,
            height: 20,
        };
        let area = modal_content_area(full, 5);
        assert_eq!(area.x, 3);
        assert_eq!(area.y, 2);
        assert_eq!(area.width, 50);
        assert_eq!(area.height, 5);
    }
}

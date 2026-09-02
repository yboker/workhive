//! Interactive state for the trusted-path input screen.
//!
//! Owns the editable buffer, live-search candidate set, and the
//! `RenderSnapshot` projection consumed by the renderer.

use super::render::{AddedRootsSnapshot, RenderSnapshot};
use super::*;

pub(super) struct State {
    pub(super) buffer: Vec<char>,
    pub(super) cursor: usize,
    pub(super) candidates: Vec<String>,
    pub(super) candidate_state: ListState,
    pub(super) no_match: bool,
    /// `true` when the current buffer is a *scoped filter* under the
    /// visible home browse scope (e.g. bare `ddd` while scope is `~/`)
    /// AND no candidate matches it. This flag gates two distinct UX
    /// behaviors:
    ///   1. the preview line surfaces a typed warning instead of a
    ///      fabricated `~/<buffer>` "will add" string, and
    ///   2. the Enter handler refuses to submit the value, keeping
    ///      the user in the input screen.
    ///
    /// Explicit `~/...` or absolute buffers are NEVER guarded by this
    /// flag; their submission still flows into the caller's own
    /// trusted-root validation.
    pub(super) scoped_filter_no_match: bool,
    /// `true` after a Tab completion lands the buffer on an existing
    /// directory path that has zero browsable child directories. The
    /// renderer surfaces a compact localized hint in the candidate
    /// slot instead of a blank panel, so the post-completion state
    /// still carries explicit browse feedback. Reset by typing
    /// (live search), by Esc dismissal, and by any later Tab.
    pub(super) dir_no_children: bool,
    /// Total content height for the modal screen. The modal anchors
    /// this many rows at the top of the frame via
    /// [`crate::ui::session::modal_content_area`]. Computed from the
    /// supplied options (whether the caller threads in a validation
    /// error / added roots summary), recomputed on a terminal resize
    /// for the new width, and grown once when Tab opens a candidate
    /// panel.
    pub(super) viewport_height: u16,
    /// Stable cap on candidate-slot rows. The same number is used
    /// both as the candidate row reservation in
    /// [`compute_viewport_height`] AND as the upper bound the
    /// renderer applies when drawing the candidate list, so
    /// live-search growth during typing never exceeds the reserved
    /// candidate slot.
    pub(super) reserved_candidate_rows: u16,
}

impl State {
    pub(super) fn from_options(opts: &PathInputOptions<'_>) -> Self {
        Self::from_options_with_width(opts, path_input_content_width())
    }

    /// Construct a fresh state for the supplied options.
    ///
    /// The candidate-slot reservation defaults to `0`: at this
    /// point the live search has not yet run, so there are no
    /// candidates to size the slot for. The production [`run`]
    /// path overwrites both `reserved_candidate_rows` and
    /// `viewport_height` once the initial live search returns.
    /// Tests that drive the state directly can either keep the
    /// zero reservation (no candidate slot) or simulate the
    /// post-live-search state by setting `candidates` plus
    /// `reserved_candidate_rows` explicitly.
    pub(super) fn from_options_with_width(opts: &PathInputOptions<'_>, content_width: u16) -> Self {
        let buffer: Vec<char> = opts.initial.chars().collect();
        let cursor = buffer.len();
        Self {
            buffer,
            cursor,
            candidates: Vec::new(),
            candidate_state: ListState::default(),
            no_match: false,
            scoped_filter_no_match: false,
            dir_no_children: false,
            viewport_height: compute_viewport_height(opts, 0, content_width),
            reserved_candidate_rows: 0,
        }
    }

    /// Recompute the inline viewport height after a terminal resize
    /// and return the new value.
    ///
    /// The candidate-slot reservation is locked at session entry (the
    /// inline viewport cannot grow its candidate slot beyond the count
    /// seen by the first live search), so a resize only re-derives the
    /// height from the new `content_width`: the prompt and a threaded
    /// validation error wrap to a different number of rows as the
    /// terminal narrows or widens. Every editing field — `buffer`,
    /// `cursor`, `candidates`, the selected candidate, the no-match /
    /// scoped-filter flags, and the caller-owned summary / validation —
    /// is intentionally left untouched so a resize preserves the user's
    /// in-progress input and selection.
    pub(super) fn on_terminal_resize(
        &mut self,
        opts: &PathInputOptions<'_>,
        content_width: u16,
    ) -> u16 {
        self.viewport_height =
            compute_viewport_height(opts, self.reserved_candidate_rows, content_width);
        self.viewport_height
    }

    pub(super) fn move_candidate_up(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len();
        let new = match self.candidate_state.selected() {
            Some(i) if i > 0 => i - 1,
            _ => len - 1,
        };
        self.candidate_state.select(Some(new));
    }

    pub(super) fn move_candidate_down(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        let len = self.candidates.len();
        let new = match self.candidate_state.selected() {
            Some(i) if i + 1 < len => i + 1,
            _ => 0,
        };
        self.candidate_state.select(Some(new));
    }

    fn clear_candidates(&mut self) {
        self.candidates.clear();
        self.candidate_state.select(None);
        self.no_match = false;
        self.scoped_filter_no_match = false;
        self.dir_no_children = false;
    }

    /// Re-run the completion use case after every keystroke. Unlike
    /// [`Self::run_completion`] (Tab), this NEVER snaps the input to
    /// a single match — it only refreshes the candidate panel so the
    /// user can keep typing or pick with arrows.
    ///
    /// `no_match` is intentionally NOT set on a non-empty buffer
    /// because the user is mid-typing and yelling "no matches" at
    /// every character would be hostile. The hint is reserved for
    /// explicit Tab / Enter actions in [`Self::run_completion`].
    pub(super) fn refresh_live_search(
        &mut self,
        home: Option<&Utf8Path>,
        strategy: &dyn PathStrategy,
        fs: &dyn PathCompletionFsPort,
    ) {
        // Live typing always invalidates any prior post-Tab
        // dir-with-no-children hint: the user has moved on, so the
        // candidate slot should reflect the latest input only.
        self.dir_no_children = false;
        let input: String = self.buffer.iter().collect();
        let is_scoped_filter = buffer_is_scoped_filter(&input, home, strategy);
        match compute_path_completions(&input, home, strategy, fs) {
            PathCompletionResult::Single { completion } => {
                tracing::debug!(
                    input = %input,
                    candidate_count = 1,
                    first_candidate = %completion,
                    "path_input: live search produced single candidate"
                );
                self.candidates = vec![completion];
                // The visible active suggestion is the Tab target.
                // Enter still submits only the input buffer.
                self.candidate_state.select(Some(0));
                self.no_match = false;
                self.scoped_filter_no_match = false;
            }
            PathCompletionResult::Many { candidates } => {
                tracing::debug!(
                    input = %input,
                    candidate_count = candidates.len(),
                    first_candidate = candidates.first().map(String::as_str).unwrap_or(""),
                    "path_input: live search produced candidate list"
                );
                self.candidates = candidates;
                // The visible active suggestion is the Tab target.
                // Enter still submits only the input buffer.
                self.candidate_state.select(Some(0));
                self.no_match = false;
                self.scoped_filter_no_match = false;
            }
            PathCompletionResult::None => {
                tracing::debug!(
                    input = %input,
                    is_scoped_filter,
                    "path_input: live search produced no candidates"
                );
                self.candidates.clear();
                self.candidate_state.select(None);
                self.no_match = false;
                // When the buffer is a scoped filter under the visible
                // browse scope and no candidate matches, the preview
                // line must surface a warning AND Enter must be
                // blocked. Explicit `~/...` and absolute paths flow
                // through their own validation in the caller and are
                // never guarded here.
                self.scoped_filter_no_match = is_scoped_filter;
            }
        }
    }

    /// Refresh the candidate slot after a Tab completion has written
    /// a directory-shaped path into the buffer.
    ///
    /// Shell-style browsing: completing `~/de` to `~/development/`
    /// must immediately list `~/development`'s child directories so
    /// the user can keep navigating without re-typing or pressing
    /// Tab again. When the completed directory exposes no browsable
    /// child directories, the live search returns an empty result;
    /// in that case we set [`Self::dir_no_children`] so the renderer
    /// fills the candidate slot with a localized hint instead of a
    /// blank panel.
    ///
    /// No-op when the buffer does not end with a strategy
    /// separator (e.g. the completion landed on a non-directory
    /// fragment).
    fn refresh_after_directory_tab(
        &mut self,
        home: Option<&Utf8Path>,
        strategy: &dyn PathStrategy,
        fs: &dyn PathCompletionFsPort,
    ) {
        if !buffer_ends_with_separator(&self.buffer, strategy) {
            return;
        }
        // `refresh_live_search` resets `dir_no_children` to false at
        // entry, so checking its post-condition tells us whether the
        // newly-completed directory has browsable children.
        self.refresh_live_search(home, strategy, fs);
        if self.candidates.is_empty() {
            self.dir_no_children = true;
        }
    }

    /// Grow the candidate-slot reservation (and the inline viewport
    /// height) so a directory the user just browsed into can show
    /// multiple child candidates at once, bounded by
    /// [`MAX_VISIBLE_CANDIDATES`].
    ///
    /// The initial reservation is sized from the candidate count seen
    /// at session entry, which keeps the prompt compact when it opens
    /// with zero or one candidate. A `Tab` completion into a directory
    /// then refreshes the candidate list to that directory's children;
    /// without this growth the slot would stay frozen at the small
    /// entry reservation and display only one child even when several
    /// exist.
    ///
    /// Returns the new viewport height when the reservation grew, so
    /// the caller can re-reserve the inline viewport. The reservation
    /// only ever grows during a session: once the user is browsing a
    /// directory, narrowing the filter must not collapse the slot back
    /// to one row, which would reintroduce candidate-row flicker.
    /// Counts above [`MAX_VISIBLE_CANDIDATES`] keep scrolling inside
    /// the slot via [`candidate_window`] rather than growing further.
    pub(super) fn grow_candidate_reservation_for_browsing(
        &mut self,
        opts: &PathInputOptions<'_>,
        content_width: u16,
    ) -> Option<u16> {
        let desired = reservation_from_initial_candidates(self.candidates.len());
        if desired <= self.reserved_candidate_rows {
            return None;
        }
        self.reserved_candidate_rows = desired;
        self.viewport_height = compute_viewport_height(opts, desired, content_width);
        Some(self.viewport_height)
    }

    /// Dismiss the candidate panel and any "no match" hint if either
    /// is currently visible. Returns `true` when there was something
    /// to dismiss so the caller can suppress the global "cancel"
    /// effect of Esc when a popup is on screen.
    pub(super) fn dismiss_candidates_if_visible(&mut self) -> bool {
        let had_panel = !self.candidates.is_empty() || self.no_match;
        if had_panel {
            tracing::debug!(
                candidate_count = self.candidates.len(),
                no_match = self.no_match,
                "path_input: Esc dismissing candidate panel"
            );
            self.clear_candidates();
        }
        had_panel
    }

    /// Tab handler — shell-like completion, never submits.
    ///
    /// Behavior, in priority order:
    ///
    /// 1. Single candidate → apply it (replace buffer with the
    ///    fully-qualified completion).
    /// 2. Many candidates whose longest common prefix STRICTLY
    ///    EXTENDS the current input → write that prefix into the
    ///    buffer (zsh-style "first Tab fills the common prefix") and
    ///    keep the candidate panel open with row 0 highlighted.
    /// 3. Many candidates with no further common prefix → apply the
    ///    currently highlighted candidate.
    /// 4. No candidates → arm the localized `no_match` hint.
    pub(super) fn run_completion(
        &mut self,
        home: Option<&Utf8Path>,
        strategy: &dyn PathStrategy,
        fs: &dyn PathCompletionFsPort,
    ) {
        let input: String = self.buffer.iter().collect();
        match compute_path_completions(&input, home, strategy, fs) {
            PathCompletionResult::Single { completion } => {
                tracing::debug!(
                    input = %input,
                    completion = %completion,
                    "path input completion produced single candidate"
                );
                self.buffer = completion.chars().collect();
                self.cursor = self.buffer.len();
                self.candidates.clear();
                self.candidate_state.select(None);
                self.no_match = false;
                self.refresh_after_directory_tab(home, strategy, fs);
            }
            PathCompletionResult::Many { candidates } => {
                tracing::debug!(
                    input = %input,
                    candidate_count = candidates.len(),
                    first_candidate = candidates.first().map(String::as_str).unwrap_or(""),
                    "path input completion produced candidate list"
                );
                let prior_highlight = self.candidate_state.selected();
                let common = longest_common_prefix(&candidates);
                self.candidates = candidates;
                self.no_match = false;

                // Step 2: shell-style common-prefix completion. Only
                // overwrite the buffer when the prefix STRICTLY extends
                // what the user already typed — otherwise we would
                // either no-op or shrink the buffer, both of which are
                // surprising on a Tab press.
                if common.chars().count() > input.chars().count() && common.starts_with(&input) {
                    self.buffer = common.chars().collect();
                    self.cursor = self.buffer.len();
                    self.candidate_state.select(Some(0));
                    return;
                }

                // Step 3: no further common prefix. The visible
                // highlighted row is the Tab target.
                match prior_highlight.or(Some(0)) {
                    Some(idx) if idx < self.candidates.len() => {
                        let pick = self.candidates[idx].clone();
                        self.buffer = pick.chars().collect();
                        self.cursor = self.buffer.len();
                        self.candidates.clear();
                        self.candidate_state.select(None);
                        self.refresh_after_directory_tab(home, strategy, fs);
                    }
                    _ => {
                        self.candidate_state.select(Some(0));
                    }
                }
            }
            PathCompletionResult::None => {
                tracing::debug!(
                    input = %input,
                    "path input completion produced no candidates"
                );
                self.candidates.clear();
                self.candidate_state.select(None);
                self.no_match = true;
            }
        }
    }

    pub(super) fn render_snapshot(
        &self,
        opts: &PathInputOptions<'_>,
        strategy: &dyn PathStrategy,
    ) -> RenderSnapshot {
        let buffer: String = self.buffer.iter().collect();
        let highlighted = self.candidate_state.selected();
        let bare_scope = buffer_is_bare_scope(&buffer, strategy);
        // Bare text typed within an active home browse scope is a
        // scoped filter, NOT a persisted relative path. Suppress the
        // relative-path warning for that case so the picker stops
        // contradicting its own UI.
        let scoped_filter = buffer_is_scoped_filter(&buffer, opts.home, strategy);
        let relative = buffer_is_relative(&buffer, strategy) && !scoped_filter;
        let scope = compute_scope(&buffer, &self.candidates, opts.home, strategy);
        // Never fabricate a "will add: ~/<buffer>" preview when the
        // current scoped filter has zero candidates; instead surface
        // the localized no-match warning and block Enter.
        let preview = if self.scoped_filter_no_match {
            None
        } else {
            compute_preview(&buffer, &self.candidates, highlighted, opts.home, strategy)
        };
        let child_labels: Vec<String> = self
            .candidates
            .iter()
            .map(|c| compute_child_label(c, scope.as_deref().unwrap_or("")))
            .collect();
        let scope_line = scope
            .as_deref()
            .map(|s| format_label(opts.scope_label_template, s))
            .unwrap_or_default();
        let preview_line = if let Some(p) = preview.as_deref() {
            format_label(opts.preview_label_template, p)
        } else if self.scoped_filter_no_match {
            format_scope_no_match(opts.scope_no_match_warning, buffer.trim())
        } else if bare_scope {
            opts.bare_scope_warning.to_owned()
        } else if relative {
            opts.relative_warning.to_owned()
        } else {
            String::new()
        };

        let preview_severity = if preview.is_some() {
            // Highlighted-candidate / typed commit-preview paths are
            // positive: the user is one keystroke away from a clean
            // submission, so the line uses the shared positive
            // palette.
            MessageSeverity::Preview
        } else if self.scoped_filter_no_match {
            // A scoped filter under the visible browse scope with
            // zero matching candidates is a BLOCKING validation
            // failure — `run()` refuses to submit the buffer (see
            // the `Enter` branch that early-returns on
            // `scoped_filter_no_match`). Per the severity contract,
            // blocking failures must render with `Danger` (red +
            // bold), NOT the softer `Warning` reserved for
            // non-blocking mid-typing guidance.
            MessageSeverity::Danger
        } else if bare_scope || relative {
            // Non-blocking mid-typing guidance: bare scope (`~`,
            // `~/`, `/`) and relative-path-shaped buffers do NOT
            // block Enter on their own. Pressing Enter on these
            // states flows into the caller's own trusted-root
            // validation, which may accept the buffer or surface a
            // separate `Danger` validation-error row above the
            // input. The preview line itself stays a softer
            // `Warning` (yellow, no bold) so the user can tell
            // these mid-typing states apart from the actual
            // blocking `Danger` states.
            MessageSeverity::Warning
        } else {
            // Empty slot — pick the neutral baseline so callers that
            // inspect the severity get a stable, non-`None` value.
            MessageSeverity::Normal
        };

        // Tab-no-match hint shown in the candidates slot when
        // `run_completion` (Tab) returned zero candidates.
        //
        // This slot ALWAYS describes a blocking-submission state
        // and therefore ALWAYS renders as `MessageSeverity::Danger`
        // (red + bold). The slot only exists once the user has
        // pressed Tab on a buffer that produced zero candidates,
        // and in every such case the submission is blocked end-to-
        // end:
        //
        //   - `scoped_filter_no_match` => the picker refuses
        //     Enter directly inside `run()`'s `KeyCode::Enter`
        //     branch, which early-returns on this flag. Picker-
        //     level block.
        //   - Explicit `~/...` / absolute path with no candidate
        //     match => the picker forwards Enter, but the
        //     caller's `classify_trusted_root_input_with_strategy`
        //     rejects the submission (`DoesNotExist` /
        //     `NotADirectory` / `TooBroad`) and the next iteration
        //     surfaces that rejection in the in-frame
        //     `validation_error` slot. Caller-level block.
        //
        // The candidates-slot Tab-no-match hint is never a
        // transient mid-typing state — it only appears after a
        // deliberate Tab on a no-match buffer — so the slot uses
        // the unified blocking-failure palette regardless of
        // whether the block is picker-level or caller-level.
        //
        // The mid-typing severity distinction (Warning for bare
        // scope / relative path) still lives in `preview_severity`
        // above; this field is intentionally separate so the
        // candidates-slot hint and the preview-line classify
        // their own slots independently and the render path has
        // a single typed source of truth for each.
        let no_match_hint_severity = MessageSeverity::Danger;

        let validation_error_text = opts
            .validation_error
            .filter(|msg| !msg.is_empty())
            .map(str::to_owned);

        let added_summary = AddedRootsSnapshot::from(&opts.added_summary);

        RenderSnapshot {
            prompt: opts.prompt.to_owned(),
            field_label: opts.field_label.to_owned(),
            placeholder: opts.placeholder.to_owned(),
            scope_line,
            preview_line,
            preview_severity,
            validation_error: validation_error_text,
            added_summary,
            footer_hint: opts.footer_hint.to_owned(),
            no_match_hint: opts.no_match_hint.to_owned(),
            no_match_hint_severity,
            buffer,
            cursor: self.cursor,
            child_labels,
            highlighted,
            no_match: self.no_match,
            buffer_is_bare_scope: bare_scope,
            buffer_is_relative: relative,
            buffer_is_scope_no_match: self.scoped_filter_no_match,
            dir_no_children: self.dir_no_children,
            dir_no_children_hint: opts.dir_no_children_hint.to_owned(),
            reserved_candidate_rows: self.reserved_candidate_rows,
        }
    }
}

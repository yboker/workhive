//! Render-side projection and inline-frame rendering for the
//! trusted-path input screen.

use super::*;

pub(super) struct RenderSnapshot {
    pub(super) prompt: String,
    pub(super) field_label: String,
    pub(super) placeholder: String,
    /// Pre-formatted scope line (e.g. `Current location: ~/`). Empty
    /// when no scope can be derived from the buffer or candidates.
    pub(super) scope_line: String,
    /// Pre-formatted preview line (e.g. `Will add: ~/Downloads`),
    /// or the bare-scope / relative-path warning when the buffer is
    /// not in a committable shape.
    pub(super) preview_line: String,
    /// Severity that the renderer must apply to the preview line.
    /// Derived from the buffer-classification flags (bare scope /
    /// relative path / scope no-match) and whether a commit preview
    /// is present, so the style decision is centralized instead of
    /// re-derived in `render()` by re-parsing the text.
    pub(super) preview_severity: MessageSeverity,
    /// Pre-localized blocking validation error threaded in by the
    /// caller. `Some` value is rendered above the input box with
    /// `MessageSeverity::Danger` styling so callers can keep the
    /// failure visible inside the current UI screen instead of
    /// relying on terminal scrollback. `None` (or empty input)
    /// hides the slot.
    pub(super) validation_error: Option<String>,
    /// Owned snapshot of the trusted-roots summary block: header,
    /// per-row text, optional overflow line, plus the optional
    /// "most-recently-added" row index for `Preview` highlighting.
    /// The renderer iterates over this without re-reading the input
    /// `PathInputOptions`.
    pub(super) added_summary: AddedRootsSnapshot,
    pub(super) footer_hint: String,
    pub(super) no_match_hint: String,
    /// Severity that the renderer must apply to the Tab-no-match
    /// hint line shown in the candidates slot. This slot is only
    /// reached after the user has explicitly pressed Tab and the
    /// picker returned zero candidates.
    ///
    /// In this product flow every Tab no-match outcome represents a
    /// blocking-submission state, so the hint always uses
    /// `MessageSeverity::Danger` (red + bold):
    ///
    ///   - `scoped_filter_no_match`: the picker refuses Enter
    ///     directly inside `run()`'s `KeyCode::Enter` arm
    ///     (picker-level block).
    ///   - Otherwise (explicit `~/...` / absolute paths with no
    ///     candidate match): the picker forwards Enter to the
    ///     caller, the caller's trusted-root validator rejects
    ///     the submission, and the next frame surfaces that
    ///     rejection in the `validation_error` row (caller-level
    ///     block).
    ///
    /// The unified `Danger` styling keeps the candidates-slot hint
    /// visually consistent with the eventual blocking outcome. The
    /// mid-typing severity distinction (`Warning` for bare scope /
    /// relative-path guidance) lives separately in
    /// `preview_severity`, so each slot has its own typed source of
    /// truth.
    pub(super) no_match_hint_severity: MessageSeverity,
    pub(super) buffer: String,
    pub(super) cursor: usize,
    /// Per-candidate display label stripped to the immediate child
    /// segment (e.g. `Applications/`). Drives the candidate list
    /// rendering; the full would-be-committed path lives in
    /// `preview_line` so users always see the absolute commit form.
    pub(super) child_labels: Vec<String>,
    pub(super) highlighted: Option<usize>,
    pub(super) no_match: bool,
    /// `true` when the buffer is exactly `~`, `~/`, or `/` — the
    /// preview line carries the bare-scope warning instead of a
    /// commit path.
    ///
    /// Read by snapshot-level tests to assert the buffer
    /// classification. Renderer-side styling now reads
    /// `preview_severity` (precomputed by `render_snapshot`) so
    /// production code paths no longer touch this flag — hence the
    /// `allow(dead_code)` outside `cfg(test)`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) buffer_is_bare_scope: bool,
    /// `true` when the buffer is non-empty but neither absolute nor
    /// `~/`-rooted — the preview line carries the relative-path
    /// warning. See `buffer_is_bare_scope` for the dead-code note.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) buffer_is_relative: bool,
    /// `true` when the buffer is a *scoped filter* under the visible
    /// home browse scope with zero candidate matches. The preview
    /// line carries the localized `scope_no_match_warning` text and
    /// the renderer styles it as `Danger` (red + bold) because this
    /// state Enter-blocks submission (`run()` refuses to leave the
    /// picker until the buffer matches a candidate or becomes an
    /// explicit `~/...` / absolute path).
    /// See `buffer_is_bare_scope` for the dead-code note.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) buffer_is_scope_no_match: bool,
    /// `true` when the most recent Tab completion landed the buffer
    /// on an existing directory path that has zero browsable child
    /// directories. The renderer surfaces
    /// [`Self::dir_no_children_hint`] in the candidate slot in this
    /// state so the post-completion screen never reads as a stalled
    /// blank panel. Reset by live typing (next keystroke clears it
    /// through [`State::refresh_live_search`]).
    pub(super) dir_no_children: bool,
    /// Pre-localized hint text rendered in the candidate slot when
    /// [`Self::dir_no_children`] is set. Empty string suppresses
    /// the slot entirely so callers that have no localized copy yet
    /// degrade gracefully to a blank slot rather than a panic.
    pub(super) dir_no_children_hint: String,
    /// Stable cap on the candidate slot height (in rows). The
    /// renderer caps the candidate list at this exact number so
    /// the visible candidate-row count is always a single source
    /// of truth shared with the inline-viewport reservation in
    /// [`compute_viewport_height`].
    pub(super) reserved_candidate_rows: u16,
}

/// Render-side snapshot of `AddedRootsSummary`.
///
/// `RenderSnapshot` is owned (no borrowed `&str` lifetimes) so that
/// tests can hold it across awaits / borrows of `PathInputOptions`.
/// This mirrors that contract: the renderer iterates `paths` and
/// reads `header_line`, `overflow_line`, and `most_recent` without
/// re-touching the original `PathInputOptions::added_summary`.
#[derive(Debug, Clone)]
pub(super) struct AddedRootsSnapshot {
    /// Pre-formatted header line, e.g. `"Trusted roots added (2):"`.
    /// Empty when both header and paths are empty (the slot is
    /// hidden entirely in that case).
    pub(super) header_line: String,
    /// Pre-formatted empty-state label, e.g.
    /// `"No trusted roots added yet."`. Rendered in place of
    /// `paths` whenever `paths.is_empty()` and `empty_label` is
    /// non-empty. The renderer skips it when both fields are empty.
    pub(super) empty_line: String,
    /// Visible path rows. Already truncated to
    /// `MAX_VISIBLE_ADDED_ROOTS` by `AddedRootsSnapshot::from` so the
    /// renderer never overflows the reserved
    /// `ADDED_SUMMARY_HEIGHT` rows.
    pub(super) paths: Vec<String>,
    /// Optional overflow line when the supplied path list was
    /// truncated. Empty when no overflow occurred.
    pub(super) overflow_line: String,
    /// Index (into `paths`) of the row that should render with
    /// `MessageSeverity::Preview` styling because it was just
    /// accepted. `None` when no row should be highlighted (e.g.
    /// after a rejection that did not add a new root).
    pub(super) most_recent: Option<usize>,
}

impl AddedRootsSnapshot {
    pub(super) fn from(summary: &AddedRootsSummary<'_>) -> Self {
        let total = summary.paths.len();
        let visible_count = total.min(MAX_VISIBLE_ADDED_ROOTS);
        let paths: Vec<String> = summary.paths.iter().take(visible_count).cloned().collect();

        let header_line = if total == 0 {
            String::new()
        } else {
            summary
                .header_template
                .replacen("{0}", &total.to_string(), 1)
        };

        let overflow_line = if total > visible_count {
            let remaining = total - visible_count;
            summary
                .overflow_template
                .replacen("{0}", &remaining.to_string(), 1)
        } else {
            String::new()
        };

        let empty_line = if total == 0 {
            summary.empty_label.to_owned()
        } else {
            String::new()
        };

        // Translate the absolute most-recent index supplied by the
        // caller into an index inside the truncated `paths` slice.
        // Recently added items that fell off the top of the window
        // (truncation keeps the *first* N entries) become `None` so
        // the renderer does not point Preview styling at the wrong
        // row.
        let most_recent = summary
            .most_recent
            .filter(|&idx| idx < visible_count && idx < total);

        Self {
            header_line,
            empty_line,
            paths,
            overflow_line,
            most_recent,
        }
    }

    /// `true` when there is something to render (header + paths or
    /// the empty-state label).
    pub(super) fn is_renderable(&self) -> bool {
        !self.paths.is_empty() || !self.empty_line.is_empty()
    }
}

pub(super) fn render(frame: &mut Frame, area: Rect, snap: &RenderSnapshot) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Candidate slot height comes from the stable reservation
    // computed at construction time, capped by the number of
    // candidates the live search is currently surfacing. Sharing
    // `reserved_candidate_rows` with the inline-viewport
    // reservation in `compute_viewport_height` keeps render-time
    // candidate rows from ever exceeding what the inline session
    // reserved, even when typing or candidate acceptance grows
    // the live candidate count. Live counts above the reservation
    // still scroll inside [`candidate_window`] with the selected
    // candidate kept visible.
    let candidate_height = (snap.child_labels.len() as u16).min(snap.reserved_candidate_rows);
    let scope_height: u16 = if snap.scope_line.is_empty() { 0 } else { 1 };
    let preview_height: u16 = if snap.preview_line.is_empty() { 0 } else { 1 };
    let no_match_height: u16 = if candidate_height == 0 && snap.no_match {
        1
    } else {
        0
    };
    // Dir-with-no-children hint: appears after a Tab completion
    // that landed the buffer on an existing directory path that
    // has zero browsable children. The slot only renders when no
    // real candidates and no `no_match` hint are competing for the
    // candidate area, and only when the caller supplied non-empty
    // localized copy.
    let dir_no_children_height: u16 = if candidate_height == 0
        && no_match_height == 0
        && snap.dir_no_children
        && !snap.dir_no_children_hint.is_empty()
    {
        1
    } else {
        0
    };
    // Prompt slot uses `Length(prompt_height)` derived from the
    // wrap-aware line count so spare rows are NOT absorbed into
    // the prompt block. A wrapped prompt grows the slot exactly
    // by the number of additional wrapped rows; an unwrapped
    // prompt always reserves one row.
    let prompt_inner = inner.width.saturating_sub(2).max(1) as usize;
    let prompt_height = (wrapped_line_count(&snap.prompt, prompt_inner) as u16).max(1);
    // Validation-error slot height is derived from how the
    // localized error wraps inside the current `inner.width` and
    // is clamped to `MAX_VALIDATION_ERROR_HEIGHT` so the message
    // stays readable on narrow terminals without consuming so
    // much headroom that the input row or footer disappear. The
    // same clamp lives in `validation_error_wrapped_height`,
    // which keeps the slot bounded even when a stale upstream
    // width reports more cells than the renderer is given.
    let validation_error_height: u16 =
        validation_error_wrapped_height(snap.validation_error.as_deref(), inner.width);
    let added_summary_height: u16 = compute_added_summary_height(&snap.added_summary);

    // Layout: prompt / added-roots summary / validation-error /
    // scope / boxed-input (3 rows) / preview / candidates (or
    // no-match) / footer.
    //
    // The added-roots summary lives directly under the prompt so the
    // user always sees the running set of accepted roots without
    // scrolling terminal scrollback. The validation-error line sits
    // immediately above the boxed input so the rejection context
    // stays visually anchored to the field the user is about to
    // retry.
    //
    // The 3-row boxed input is what gives the field its "real input
    // box" affordance — top border, content row, bottom border.
    let mut constraints: Vec<Constraint> = vec![
        Constraint::Length(prompt_height), // prompt sized to its wrap height
    ];
    if added_summary_height > 0 {
        constraints.push(Constraint::Length(added_summary_height));
    }
    if validation_error_height > 0 {
        constraints.push(Constraint::Length(validation_error_height));
    }
    constraints.extend_from_slice(&[
        Constraint::Length(scope_height),
        Constraint::Length(3), // boxed input area
        Constraint::Length(preview_height),
    ]);
    if candidate_height > 0 {
        constraints.push(Constraint::Length(candidate_height));
    } else if no_match_height > 0 {
        constraints.push(Constraint::Length(no_match_height));
    } else if dir_no_children_height > 0 {
        constraints.push(Constraint::Length(dir_no_children_height));
    }
    constraints.push(Constraint::Length(1)); // footer hint
    // Trailing flexible filler so any spare rows inside the
    // inline viewport (rare, but possible when the user types
    // prefixes that shrink the candidate list below its initial
    // count) collapse to a single blank slot at the bottom rather
    // than getting absorbed into the prompt or the input box.
    constraints.push(Constraint::Min(0));

    let layout = Layout::vertical(constraints).split(inner);

    let prompt_idx = 0;
    let mut next_idx = 1;
    let added_summary_idx = if added_summary_height > 0 {
        let i = next_idx;
        next_idx += 1;
        Some(i)
    } else {
        None
    };
    let validation_error_idx = if validation_error_height > 0 {
        let i = next_idx;
        next_idx += 1;
        Some(i)
    } else {
        None
    };
    let scope_idx = next_idx;
    let input_idx = next_idx + 1;
    let preview_idx = next_idx + 2;
    next_idx += 3;
    let candidates_idx =
        if candidate_height > 0 || no_match_height > 0 || dir_no_children_height > 0 {
            let i = next_idx;
            next_idx += 1;
            Some(i)
        } else {
            None
        };
    let footer_idx = next_idx;

    // Prompt
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::raw(format!(" {} ", snap.prompt))]))
            .wrap(Wrap { trim: true }),
        layout[prompt_idx],
    );

    // Added trusted roots summary block — pinned directly under the
    // prompt so the user sees the running list without scrolling
    // terminal scrollback.
    if let Some(idx) = added_summary_idx {
        render_added_summary(frame, layout[idx], &snap.added_summary);
    }

    // Blocking validation error threaded in by the caller — sits
    // immediately above the boxed input so the rejection stays
    // anchored to the field being retried. Styled `Danger` so it
    // is visually distinct from warnings / preview hints / help
    // text.
    //
    // We enable `Wrap { trim: true }` so long localized errors flow
    // onto additional rows inside the bounded slot reserved by
    // `validation_error_height`. Without wrapping, narrow terminals
    // would clip the message at the right border.
    if let Some(idx) = validation_error_idx
        && let Some(msg) = snap.validation_error.as_deref()
    {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", msg),
                MessageSeverity::Danger.style(),
            )))
            .wrap(Wrap { trim: true }),
            layout[idx],
        );
    }

    // Scope ("Current location: ~/")
    if scope_height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", snap.scope_line),
                Style::default().fg(Color::Cyan),
            ))),
            layout[scope_idx],
        );
    }

    // Boxed input area with field label + cursor / placeholder.
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" {} ", snap.field_label),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    let input_inner = input_block.inner(layout[input_idx]);
    frame.render_widget(input_block, layout[input_idx]);

    let input_width = input_inner.width.saturating_sub(1) as usize;
    let input_line = if snap.buffer.is_empty() {
        // Render the placeholder grayed-out plus a visible cursor
        // block at column 0 so the field still reads as a real
        // editable input.
        Line::from(vec![
            Span::styled(
                " ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                snap.placeholder.clone(),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
        ])
    } else {
        let (visible, visible_cursor) =
            window_around_cursor(&snap.buffer, snap.cursor, input_width);
        let (before, cursor_char, after) = split_for_cursor(&visible, visible_cursor);
        Line::from(vec![
            Span::styled(before, Style::default().fg(Color::White)),
            Span::styled(
                cursor_char.to_string(),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(after, Style::default().fg(Color::White)),
        ])
    };
    frame.render_widget(Paragraph::new(input_line), input_inner);

    // Preview ("Will add: ~/Downloads") OR a state-driven message
    // rendered in the same slot. The severity decision is
    // centralized in `RenderSnapshot::preview_severity` (derived
    // once from buffer-classification flags); the renderer just
    // looks the style up so no Color::* literals leak into the
    // render branch. Per the severity contract:
    //   - commit-preview          → `Preview` (green + bold);
    //   - scoped-filter no-match  → `Danger`  (red + bold,
    //                                blocking — `run()` refuses
    //                                Enter on this state);
    //   - bare-scope / relative   → `Warning` (yellow,
    //                                non-blocking mid-typing
    //                                guidance);
    //   - otherwise empty         → `Normal`.
    if preview_height > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {} ", snap.preview_line),
                snap.preview_severity.style(),
            ))),
            layout[preview_idx],
        );
    }

    // Candidate list (or no-match hint) when we reserved a slot.
    if let Some(idx) = candidates_idx {
        if candidate_height > 0 {
            let mut list_state = ListState::default();
            list_state.select(snap.highlighted);
            let window = candidate_window(
                snap.child_labels.len(),
                snap.highlighted,
                snap.reserved_candidate_rows as usize,
            );
            list_state.select(window.local_highlighted);
            let items: Vec<ListItem> = snap
                .child_labels
                .iter()
                .enumerate()
                .skip(window.start)
                .take(window.len)
                .enumerate()
                .map(|(local_idx, (global_idx, label))| {
                    let is_sel = window.local_highlighted == Some(local_idx);
                    let prefix = if is_sel { " > " } else { "   " };
                    let suffix =
                        candidate_scroll_suffix(global_idx, snap.child_labels.len(), window);
                    let style = if is_sel {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!("{prefix}{label}{suffix}"),
                        style,
                    )))
                })
                .collect();
            frame.render_stateful_widget(List::new(items), layout[idx], &mut list_state);
        } else if no_match_height > 0 {
            // No-match hint slot. Severity is precomputed in
            // `render_snapshot` (`no_match_hint_severity`) so the
            // render path stays free of severity literals. The
            // slot ALWAYS resolves to `Danger` (red + bold)
            // because it only appears after a deliberate Tab on a
            // no-match buffer and submission is blocked end-to-end
            // in every branch — picker-level via
            // `scoped_filter_no_match`, or caller-level via the
            // next frame's `validation_error` row when the
            // caller's trusted-root validator rejects the buffer.
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} ", snap.no_match_hint),
                    snap.no_match_hint_severity.style(),
                ))),
                layout[idx],
            );
        } else {
            // Dir-with-no-children hint slot. Rendered after a Tab
            // completion lands the buffer on an existing directory
            // path whose child-directory live search returned no
            // candidates. The slot uses neutral help styling so it
            // reads as a passive browse hint, not as a validation
            // error — the buffer is itself acceptable and `Enter`
            // will submit it as a trusted root.
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {} ", snap.dir_no_children_hint),
                    MessageSeverity::Help.style(),
                ))),
                layout[idx],
            );
        }
    }

    // Footer hint — neutral help text.
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {} ", snap.footer_hint),
            MessageSeverity::Help.style(),
        ))),
        layout[footer_idx],
    );
}

/// Render the added-trusted-roots summary block: header, per-path
/// rows (with the most-recent row highlighted in `Preview` style)
/// and an optional overflow line. Empty state is rendered in place
/// of the path rows when no roots have been added yet.
fn render_added_summary(frame: &mut Frame, area: Rect, summary: &AddedRootsSnapshot) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    if !summary.header_line.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" {} ", summary.header_line),
            MessageSeverity::Help.style(),
        )));
    }

    if summary.paths.is_empty() {
        if !summary.empty_line.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" {} ", summary.empty_line),
                MessageSeverity::Help.style(),
            )));
        }
    } else {
        // Every accepted trusted-root row carries the same valid
        // success styling, regardless of insertion order. All
        // accepted roots are equally trusted, so consistent styling
        // keeps the list readable as a single committed set rather
        // than implying a hierarchy between rows.
        let _ = summary.most_recent;
        let row_style = MessageSeverity::Success.style();
        for path in summary.paths.iter() {
            lines.push(Line::from(Span::styled(format!(" • {} ", path), row_style)));
        }
        if !summary.overflow_line.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" {} ", summary.overflow_line),
                MessageSeverity::Help.style(),
            )));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}

/// Layout height the added-roots summary block needs.
///
/// Returns the actual number of rows that
/// [`render_added_summary`] will paint for the supplied snapshot:
///
///   * 0 when the block has no visible content,
///   * 1 when only the empty-state label is shown,
///   * `header? + visible_paths + overflow?` otherwise, capped at
///     [`MAX_VISIBLE_ADDED_ROOTS`] + 2 so the slot stays bounded.
///
/// Returning the actual rendered height (instead of a fixed
/// constant) keeps the inline viewport compact: a single accepted
/// root no longer reserves the rows that would only be required if
/// the user accumulates many roots.
pub(super) fn compute_added_summary_height(summary: &AddedRootsSnapshot) -> u16 {
    if !summary.is_renderable() {
        return 0;
    }
    let header_rows: u16 = if summary.header_line.is_empty() { 0 } else { 1 };
    if summary.paths.is_empty() {
        // Empty-state branch: only the empty-state label is shown.
        return if summary.empty_line.is_empty() {
            0
        } else {
            (header_rows + 1).max(1)
        };
    }
    let path_rows: u16 = summary.paths.len() as u16;
    let overflow_rows: u16 = if summary.overflow_line.is_empty() {
        0
    } else {
        1
    };
    let total = header_rows
        .saturating_add(path_rows)
        .saturating_add(overflow_rows);
    total.min(MAX_VISIBLE_ADDED_ROOTS as u16 + 2)
}

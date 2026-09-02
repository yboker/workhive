use super::*;

/// Empty initial buffer with a known home and a zero-row
/// candidate reservation keeps the rendered content compact.
/// Specifically the empty trusted-root summary and the absent
/// validation error must NOT reserve rows. The scope and
/// preview/warning rows are reserved unconditionally because
/// the locked candidate reservation cannot grow once the user starts
/// typing; the trailing filler absorbs the reservation when the
/// snapshot has nothing to show in those slots.
#[test]
fn empty_options_yield_compact_viewport_with_no_candidate_reservation() {
    let opts = options("");
    let h = compute_viewport_height(&opts, 0, 120);
    // 7 (compact base = borders + prompt + input(3) + footer)
    // + 1 (scope row, always reserved so typing `~/...` cannot
    //      push the input box off the locked viewport)
    // + 1 (preview/warning row, always reserved so typing
    //      lights up the commit-preview slot without growing
    //      the locked viewport)
    // + 0 (candidates, reservation = 0)
    // + 0 (summary, empty)
    // + 0 (validation error, none)
    assert_eq!(h, COMPACT_BASE_ROWS + 2);
}

/// The trusted-root path input keeps its editable input row visible
/// after the user types: the locked content sizing must not depend on
/// whether `initial` is empty.
#[test]
fn typed_buffer_does_not_inflate_locked_inline_viewport_beyond_empty_baseline() {
    let width: u16 = 120;
    let empty_baseline = compute_viewport_height(&options(""), 0, width);

    for typed in ["d", "~/", "~/d", "~/Downloads", "/abs/path", "relative/dir"] {
        let h = compute_viewport_height(&options(typed), 0, width);
        assert_eq!(
            h, empty_baseline,
            "viewport height for typed buffer `{typed}` must equal the empty baseline so the locked content area cannot clip the input row when the user types"
        );
    }
}

/// `compute_viewport_height` reserves the candidate slot at
/// the requested reservation, bounded by
/// [`MAX_VISIBLE_CANDIDATES`]. Reservations above the cap are
/// clamped so the prompt stays usable on tall and short
/// terminals alike.
#[test]
fn viewport_reserves_candidate_slot_up_to_the_max_visible_cap() {
    let opts = options("");
    let h_one = compute_viewport_height(&opts, 1, 120);
    let h_three = compute_viewport_height(&opts, 3, 120);
    let h_eight = compute_viewport_height(&opts, 8, 120);
    let h_overflow = compute_viewport_height(&opts, 30, 120);

    let baseline = compute_viewport_height(&opts, 0, 120);
    assert_eq!(h_one, baseline + 1);
    assert_eq!(h_three, baseline + 3);
    assert_eq!(h_eight, baseline + MAX_VISIBLE_CANDIDATES);
    assert_eq!(
        h_overflow,
        baseline + MAX_VISIBLE_CANDIDATES,
        "candidate slot is bounded by MAX_VISIBLE_CANDIDATES even when more rows are requested"
    );
}

/// `reservation_from_initial_candidates` is content-aware:
/// the slot exactly fits the candidates seen at session
/// entry, capped at [`MAX_VISIBLE_CANDIDATES`]. Zero
/// candidates reserve zero rows so a prompt that has nothing
/// to surface adds no candidate rows to the inline viewport.
#[test]
fn reservation_matches_initial_candidate_count_up_to_the_max_visible_cap() {
    assert_eq!(super::reservation_from_initial_candidates(0), 0);
    assert_eq!(super::reservation_from_initial_candidates(1), 1);
    assert_eq!(super::reservation_from_initial_candidates(3), 3);
    assert_eq!(
        super::reservation_from_initial_candidates(MAX_VISIBLE_CANDIDATES as usize),
        MAX_VISIBLE_CANDIDATES
    );
    assert_eq!(
        super::reservation_from_initial_candidates(50),
        MAX_VISIBLE_CANDIDATES
    );
}

/// Runtime path coverage: a trusted-root prompt with `home`
/// set whose live search returns ZERO candidates must
/// reserve zero candidate rows AND size the inline viewport
/// to the no-candidate baseline. This is the post-acceptance
/// "add another root?" geometry: no hidden full-slot
/// reservation may inflate the next prompt.
#[test]
fn runtime_reservation_is_zero_when_initial_live_search_returns_no_candidates() {
    let opts = options("");
    // FakeFs has no children registered for `/Users/me`, so
    // `refresh_live_search` returns no candidates.
    let fs = FakeFs::new();
    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(state.candidates.is_empty());

    let spec = super::resolve_inline_viewport_spec(&opts, state.candidates.len(), 120);
    let baseline = compute_viewport_height(&opts, 0, 120);

    assert_eq!(
        spec.reserved_candidate_rows, 0,
        "zero candidates must reserve zero rows"
    );
    assert_eq!(
        spec.viewport_height, baseline,
        "viewport must collapse to the no-candidate baseline"
    );
}

/// Runtime path coverage: when the initial live search
/// surfaces exactly one candidate, the candidate slot
/// reserves exactly one row and does not allocate unused
/// candidate rows.
#[test]
fn runtime_reservation_is_exactly_one_when_initial_live_search_returns_one_candidate() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["development"]);
    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert_eq!(state.candidates.len(), 1);

    let spec = super::resolve_inline_viewport_spec(&opts, state.candidates.len(), 120);
    let baseline = compute_viewport_height(&opts, 0, 120);

    assert_eq!(
        spec.reserved_candidate_rows, 1,
        "one candidate must reserve exactly one row"
    );
    assert_eq!(
        spec.viewport_height,
        baseline + 1,
        "viewport must grow by exactly one candidate row, not the full slot"
    );
}

/// Runtime path coverage: when the initial live search
/// surfaces many siblings the slot saturates at
/// `MAX_VISIBLE_CANDIDATES`. Anything beyond the cap is
/// scrolled inside the slot by `candidate_window`, never by
/// growing the inline viewport.
#[test]
fn runtime_reservation_caps_at_max_visible_when_live_search_returns_more() {
    let opts = options("");
    let names: Vec<String> = (0..20).map(|i| format!("dir{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let fs = FakeFs::new().dirs("/Users/me", &name_refs);
    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(state.candidates.len() > MAX_VISIBLE_CANDIDATES as usize);

    let spec = super::resolve_inline_viewport_spec(&opts, state.candidates.len(), 120);
    let baseline = compute_viewport_height(&opts, 0, 120);

    assert_eq!(
        spec.reserved_candidate_rows, MAX_VISIBLE_CANDIDATES,
        "slot reservation must saturate at MAX_VISIBLE_CANDIDATES"
    );
    assert_eq!(
        spec.viewport_height,
        baseline + MAX_VISIBLE_CANDIDATES,
        "viewport grows by exactly the bounded slot, not the live candidate count"
    );
}

/// Lock the entry candidate-slot reservation exactly as the
/// `run` loop does on session construction: size it from the
/// candidate count produced by the first live search.
fn lock_entry_reservation(state: &mut State, opts: &PathInputOptions<'_>, content_width: u16) {
    let spec = super::resolve_inline_viewport_spec(opts, state.candidates.len(), content_width);
    state.reserved_candidate_rows = spec.reserved_candidate_rows;
    state.viewport_height = spec.viewport_height;
}

/// A prompt that opens with a single scoped match reserves one
/// candidate row. After the user Tab-completes into that directory
/// the candidate list becomes the directory's children, and the
/// slot reservation grows so several children are visible at once
/// instead of staying frozen at the one-row entry reservation.
#[test]
fn post_tab_directory_browse_grows_candidate_slot_beyond_entry_reservation() {
    let opts = options("~/de");
    let strategy = unix_strategy();
    let fs = FakeFs::new().dirs("/Users/me", &["development"]).dirs(
        "/Users/me/development",
        &["alpha", "beta", "gamma", "delta"],
    );

    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &strategy, &fs);
    lock_entry_reservation(&mut state, &opts, 120);
    assert_eq!(
        state.reserved_candidate_rows, 1,
        "entry filter matches exactly one directory"
    );

    // Tab completes the single match and browses into it, so the
    // candidate list is now the directory's children.
    state.run_completion(opts.home, &strategy, &fs);
    assert_eq!(
        state.candidates.len(),
        4,
        "completing into the directory surfaces its children"
    );

    let grown = state.grow_candidate_reservation_for_browsing(&opts, 120);
    let baseline = compute_viewport_height(&opts, 0, 120);
    assert_eq!(
        grown,
        Some(state.viewport_height),
        "the slot must grow and report the new inline viewport height"
    );
    assert_eq!(
        state.reserved_candidate_rows, 4,
        "the slot must expand to show every child, not stay at one row"
    );
    assert_eq!(
        state.viewport_height,
        baseline + 4,
        "the inline viewport grows by exactly the expanded child rows"
    );
}

/// When the browsed directory holds more children than the cap,
/// the slot saturates at [`MAX_VISIBLE_CANDIDATES`]; the surplus
/// scrolls inside the slot via `candidate_window` rather than
/// growing the inline viewport without bound.
#[test]
fn post_tab_directory_browse_grows_candidate_slot_caps_at_max_visible() {
    let opts = options("~/de");
    let strategy = unix_strategy();
    let names: Vec<String> = (0..20).map(|i| format!("child{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let fs = FakeFs::new()
        .dirs("/Users/me", &["development"])
        .dirs("/Users/me/development", &name_refs);

    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &strategy, &fs);
    lock_entry_reservation(&mut state, &opts, 120);
    assert_eq!(state.reserved_candidate_rows, 1);

    state.run_completion(opts.home, &strategy, &fs);
    assert!(state.candidates.len() > MAX_VISIBLE_CANDIDATES as usize);

    state.grow_candidate_reservation_for_browsing(&opts, 120);
    let baseline = compute_viewport_height(&opts, 0, 120);
    assert_eq!(
        state.reserved_candidate_rows, MAX_VISIBLE_CANDIDATES,
        "the slot must saturate at the visible cap"
    );
    assert_eq!(
        state.viewport_height,
        baseline + MAX_VISIBLE_CANDIDATES,
        "the inline viewport grows by exactly the bounded slot"
    );

    // The surplus stays inside the slot: the scrolling window
    // never exceeds the bounded reservation.
    let window = candidate_window(
        state.candidates.len(),
        state.candidate_state.selected(),
        state.reserved_candidate_rows as usize,
    );
    assert!((window.len as u16) <= state.reserved_candidate_rows);
}

/// The reservation only ever grows within a session. After
/// browsing into a directory expands the slot, narrowing the
/// filter back down to one match must not collapse the slot to a
/// single row, which would reintroduce candidate-row flicker.
#[test]
fn post_tab_growth_never_shrinks_when_filter_narrows_within_directory() {
    let opts = options("~/de");
    let strategy = unix_strategy();
    let fs = FakeFs::new().dirs("/Users/me", &["development"]).dirs(
        "/Users/me/development",
        &["alpha", "beta", "gamma", "delta"],
    );

    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &strategy, &fs);
    lock_entry_reservation(&mut state, &opts, 120);

    state.run_completion(opts.home, &strategy, &fs);
    state.grow_candidate_reservation_for_browsing(&opts, 120);
    assert_eq!(state.reserved_candidate_rows, 4);

    // Simulate the user narrowing the filter to a single match.
    state.candidates = vec!["~/development/alpha/".to_owned()];
    let regrow = state.grow_candidate_reservation_for_browsing(&opts, 120);

    assert_eq!(
        regrow, None,
        "narrowing the filter must not request a viewport resize"
    );
    assert_eq!(
        state.reserved_candidate_rows, 4,
        "the expanded slot must persist so the candidate area does not flicker"
    );
}

/// Once an inline session is constructed the candidate-slot
/// reservation is frozen. Live-search growth during typing
/// or candidate acceptance is cropped to the slot reservation
/// by the renderer; the inline viewport cannot grow after
/// construction. The render-time cap and the
/// `candidate_window` length both read from the same
/// `reserved_candidate_rows` value, so neither path may
/// exceed the reservation.
#[test]
fn render_caps_candidate_rows_at_session_reservation_after_typing_growth() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["development", "projects"]);
    let strategy = UnixPathStrategy;

    // Replay the runtime path: refresh the live search, then
    // lock in the reservation via the same helper `run` calls.
    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &strategy, &fs);
    let session_spec = super::resolve_inline_viewport_spec(&opts, state.candidates.len(), 120);
    state.reserved_candidate_rows = session_spec.reserved_candidate_rows;
    state.viewport_height = session_spec.viewport_height;
    assert_eq!(state.reserved_candidate_rows, 2);

    // Simulate the user typing or accepting a candidate that
    // expands the live result set far beyond the frozen slot.
    state.candidates = (0..50).map(|i| format!("/abs/p/{i}")).collect();
    state.candidate_state.select(Some(20));

    let snap = state.render_snapshot(&opts, &strategy);

    // The slot reservation does NOT grow with the live
    // candidate list — it stays anchored to the value the
    // inline viewport was sized for.
    assert_eq!(
        snap.reserved_candidate_rows,
        session_spec.reserved_candidate_rows
    );

    // The renderer's candidate-row count is capped by the
    // slot reservation, regardless of how many live
    // candidates exist.
    let visible_rows = (snap.child_labels.len() as u16).min(snap.reserved_candidate_rows);
    assert_eq!(
        visible_rows, session_spec.reserved_candidate_rows,
        "render-time candidate rows must not exceed the session reservation"
    );

    // `candidate_window` consumes the same reservation, so
    // the scrolling window also stays bounded by it.
    let window = candidate_window(
        snap.child_labels.len(),
        snap.highlighted,
        snap.reserved_candidate_rows as usize,
    );
    assert!(
        (window.len as u16) <= snap.reserved_candidate_rows,
        "candidate_window length must respect the session reservation"
    );
}

/// A prompt whose initial live search returns one candidate
/// reserves exactly the rows it can render. `viewport_height`
/// equals the no-candidate baseline plus the visible
/// candidate rows, so the `MoveToNextLine(viewport_height)`
/// teardown advances the cursor by exactly the rows the user
/// saw and does not leave hidden empty rows in scrollback
/// above the next prompt.
#[test]
fn add_more_prompt_after_acceptance_keeps_compact_reservation() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["development"]);
    let mut state = State::from_options_with_width(&opts, 120);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let spec = super::resolve_inline_viewport_spec(&opts, state.candidates.len(), 120);
    let no_candidate_baseline = compute_viewport_height(&opts, 0, 120);
    let visible_candidate_rows = state.candidates.len() as u16;

    assert_eq!(spec.reserved_candidate_rows, visible_candidate_rows);
    assert_eq!(
        spec.viewport_height,
        no_candidate_baseline + visible_candidate_rows,
        "viewport_height must equal the no-candidate baseline plus the visible \
             candidate rows; any extra rows would surface as a stretch of blank \
             scrollback above the next prompt"
    );
    // The MoveToNextLine teardown advances the cursor by
    // exactly `viewport_height` rows, so the assertion above
    // is the contract that prevents the next prompt from
    // being separated from the accepted-root summary by a
    // hidden empty band.
    let unused_rows_in_session = spec
        .viewport_height
        .saturating_sub(no_candidate_baseline + visible_candidate_rows);
    assert_eq!(unused_rows_in_session, 0);
}

/// Tab-grow / no footer ghost: when Tab opens candidates the
/// content height grows. The modal screen anchors content at the
/// same top origin and fully repaints every frame, so a footer/help
/// last row only stays clear if the grown content covers every row
/// the old content occupied. This proves the grow direction never
/// leaves a row behind: the grown height is strictly larger and the
/// old footer row falls inside the new content span, so it is
/// repainted, not ghosted.
#[test]
fn tab_grow_reserves_a_viewport_that_covers_the_old_footer_row() {
    let opts = options("");
    let content_width = 120u16;

    // Compact (pre-Tab) viewport: no candidate rows reserved.
    let old_height = compute_viewport_height(&opts, 0, content_width);

    // Simulate Tab opening a candidate panel, then growing the
    // reservation the way `run()` does on the first Tab browse.
    let mut state = State::from_options_with_width(&opts, content_width);
    state.candidates = vec![
        "/Users/me/dev".to_owned(),
        "/Users/me/docs".to_owned(),
        "/Users/me/downloads".to_owned(),
    ];
    let new_height = state
        .grow_candidate_reservation_for_browsing(&opts, content_width)
        .expect("opening candidates must grow the reserved viewport height");

    assert!(
        new_height > old_height,
        "Tab opening candidates must grow the viewport (old={old_height}, new={new_height})"
    );
    // The content is anchored at the same top origin across the
    // grow. With a shared top row, every old row index is
    // 0..old_height and every new row index is 0..new_height.
    // new_height > old_height means the old footer row
    // (old_height - 1) is inside the new content span, so the
    // grown frame repaints it rather than leaving a ghost.
    let old_footer_row = old_height - 1;
    assert!(
        old_footer_row < new_height,
        "the old footer/help row ({old_footer_row}) must fall inside the grown viewport (height {new_height}) so it is repainted, not ghosted"
    );

    // Confirm the reserved candidate rows match the live candidate
    // count, i.e. the growth is driven by the candidate panel that
    // pushed the footer down, not an unrelated slot.
    assert_eq!(
        state.reserved_candidate_rows,
        reservation_from_initial_candidates(state.candidates.len()),
        "the growth must be sized to the opened candidate panel"
    );
}

/// The production event loop derives its layout width from the live
/// render area, not a width cached at entry. `content_width_for_area`
/// is that single source: it subtracts the outer block's two border
/// cells from the drawn area width and floors the result at
/// [`MIN_CONTENT_WIDTH`] so a narrow terminal stays usable. The
/// renderer wraps against the same `area.width - 2`, so the height
/// computed from this width always matches what is drawn.
#[test]
fn content_width_for_area_subtracts_borders_and_floors_to_minimum() {
    // A full 120-col frame leaves 118 cells inside the border.
    assert_eq!(content_width_for_area(120), 118);
    // After a resize down to 30 cols the live content width is 28.
    assert_eq!(content_width_for_area(30), 28);
    // Degenerate widths floor at MIN_CONTENT_WIDTH so wrapping never
    // collapses the input row off the screen.
    assert_eq!(content_width_for_area(10), MIN_CONTENT_WIDTH);
    assert_eq!(content_width_for_area(0), MIN_CONTENT_WIDTH);
}

/// The loop must size every post-entry layout decision from the live
/// frame: on resize it derives the width from the new column count the
/// resize event reports, and a subsequent Tab browse grows the
/// candidate reservation against that resized width — never the
/// entry-time width. This drives the exact width source the loop uses
/// (`content_width_for_area` of the reported columns), not the
/// already-covered `on_terminal_resize(&opts, 30)` pure function.
#[test]
fn tab_growth_after_resize_uses_resized_width_not_entry_width() {
    let long_prompt = "Add a trusted root directory you want workhive to be allowed to open later";
    let opts = PathInputOptions {
        prompt: long_prompt,
        ..options("")
    };

    // Entry: the loop derives content width from the drawn area of a
    // 120-col frame, matching what the renderer wraps against.
    let entry_width = content_width_for_area(120);
    assert_eq!(entry_width, 118);
    let mut content_width = entry_width;

    let mut state = State::from_options_with_width(&opts, content_width);
    state.candidates = vec![
        "/Users/me/dev".to_owned(),
        "/Users/me/docs".to_owned(),
        "/Users/me/downloads".to_owned(),
    ];

    // A resize event reports 30 columns. The loop derives the live
    // width from the event the same way `run()` does.
    content_width = content_width_for_area(30);
    assert_eq!(content_width, 28);
    state.on_terminal_resize(&opts, content_width);

    // The Tab browse grows the reservation against the width the loop
    // now carries (28) — the resized terminal, never the entry width.
    let grown_at_resized_width = state
        .grow_candidate_reservation_for_browsing(&opts, content_width)
        .expect("opening candidates must grow the reserved viewport height");

    // Independent control that never resized and grows at the entry
    // width. The long prompt wraps onto more rows at 28 cols than at
    // 118, so the resized-width growth must be strictly taller —
    // proving the loop used 28, not 118.
    let mut entry_width_state = State::from_options_with_width(&opts, entry_width);
    entry_width_state.candidates = state.candidates.clone();
    let grown_at_entry_width = entry_width_state
        .grow_candidate_reservation_for_browsing(&opts, entry_width)
        .expect("opening candidates must grow the reserved viewport height");

    assert!(
        grown_at_resized_width > grown_at_entry_width,
        "Tab growth after a resize to 30 cols must size against the resized width \
         (28 -> height {grown_at_resized_width}), not the entry width \
         (118 -> height {grown_at_entry_width})"
    );

    // The grown height must equal a direct compute at the resized
    // width and the grown reservation, confirming the loop's width
    // source feeds the same layout the renderer draws.
    let expected_at_resized_width =
        compute_viewport_height(&opts, state.reserved_candidate_rows, content_width);
    assert_eq!(
        grown_at_resized_width, expected_at_resized_width,
        "the loop's resized width (28) must drive the same height the renderer computes"
    );
}

/// A terminal resize recomputes the inline viewport height for the
/// new width while preserving every editing field. Narrowing the
/// terminal wraps a long prompt onto more rows (so the height grows),
/// and widening it back returns to the original height. The buffer,
/// cursor, candidate list, and selected candidate must survive the
/// resize untouched, and the locked candidate reservation must not
/// change.
#[test]
fn resize_recomputes_height_for_new_width_and_preserves_editing_state() {
    let long_prompt = "Add a trusted root directory you want workhive to be allowed to open later";
    let opts = PathInputOptions {
        prompt: long_prompt,
        ..options("")
    };

    let mut state = State::from_options_with_width(&opts, 120);
    // Simulate a post-live-search state with candidates and an active
    // selection so the preservation guarantee is meaningful.
    state.buffer = "~/dev".chars().collect();
    state.cursor = state.buffer.len();
    state.candidates = vec!["~/development/".to_owned(), "~/docs/".to_owned()];
    state.candidate_state.select(Some(1));
    state.reserved_candidate_rows = 2;
    state.viewport_height = compute_viewport_height(&opts, 2, 120);
    let wide_height = state.viewport_height;

    // Narrow the terminal: the long prompt wraps onto extra rows.
    let narrow_height = state.on_terminal_resize(&opts, 30);
    assert!(
        narrow_height > wide_height,
        "narrowing the terminal must grow the prompt wrap rows (wide={wide_height}, narrow={narrow_height})"
    );
    assert_eq!(state.viewport_height, narrow_height);

    // The locked candidate reservation is never touched by a resize.
    assert_eq!(state.reserved_candidate_rows, 2);

    // Every editing field is preserved across the resize.
    assert_eq!(state.buffer.iter().collect::<String>(), "~/dev");
    assert_eq!(state.cursor, 5);
    assert_eq!(
        state.candidates,
        vec!["~/development/".to_owned(), "~/docs/".to_owned()]
    );
    assert_eq!(state.candidate_state.selected(), Some(1));

    // Widening back to the original width returns to the original
    // height, proving the recompute is driven purely by the new width.
    let back = state.on_terminal_resize(&opts, 120);
    assert_eq!(back, wide_height);
    assert_eq!(state.viewport_height, wide_height);
}

/// A resize recomputes the bounded validation-error slot for the new
/// width: a long threaded error wraps onto more rows on a narrow
/// terminal (capped at `MAX_VALIDATION_ERROR_HEIGHT`) and collapses
/// back on a wide one, without disturbing the candidate reservation.
#[test]
fn resize_recomputes_validation_error_rows_for_new_width() {
    let long_msg = "Error: Path '/Users/dev/projects/very-long-fixture-path-that-must-wrap-on-narrow-terminals' does not exist or is not a trusted git root.";
    let opts = PathInputOptions {
        validation_error: Some(long_msg),
        ..options("")
    };

    let mut state = State::from_options_with_width(&opts, 200);
    state.reserved_candidate_rows = 1;
    state.viewport_height = compute_viewport_height(&opts, 1, 200);
    let wide_height = state.viewport_height;

    let narrow_height = state.on_terminal_resize(&opts, 30);
    assert!(
        narrow_height > wide_height,
        "a long validation error wraps onto more rows on a narrow terminal"
    );
    assert_eq!(
        state.reserved_candidate_rows, 1,
        "resize must not touch the candidate reservation"
    );

    let back = state.on_terminal_resize(&opts, 200);
    assert_eq!(back, wide_height);
}

/// Adding a single trusted root reserves only the header row,
/// the single path row, and any overflow row — not the
/// worst-case `MAX_VISIBLE_ADDED_ROOTS` rows. This is the core
/// of the trusted-root density fix: a one-root summary must
/// not pad the prompt by 5 rows.
#[test]
fn one_added_root_reserves_only_visible_summary_rows() {
    let width: u16 = 120;
    let baseline_h = compute_viewport_height(&options(""), 0, width);

    let paths = vec!["~/development/".to_owned()];
    let with_one_root = PathInputOptions {
        added_summary: AddedRootsSummary {
            header_template: "Trusted roots added ({0}):",
            empty_label: "",
            overflow_template: "… and {0} more",
            paths: &paths,
            most_recent: Some(0),
        },
        ..options("")
    };
    let h = compute_viewport_height(&with_one_root, 0, width);
    // Header (1) + visible path (1) = +2 rows over baseline.
    assert_eq!(
        h,
        baseline_h + 2,
        "one-root summary reserves header + 1 path row, not the worst-case max"
    );
}

/// Long summaries cap at the bounded slot height
/// (`MAX_VISIBLE_ADDED_ROOTS + 2`) so the prompt stays usable
/// when the user has accepted many roots. Overflow paths
/// collapse into the localized `… and N more` line.
#[test]
fn long_added_summary_caps_at_bounded_height_with_overflow_row() {
    let width: u16 = 120;
    let baseline_h = compute_viewport_height(&options(""), 0, width);

    let paths: Vec<String> = (0..10).map(|i| format!("~/p/{i}")).collect();
    let with_many = PathInputOptions {
        added_summary: AddedRootsSummary {
            header_template: "Trusted roots added ({0}):",
            empty_label: "",
            overflow_template: "… and {0} more",
            paths: &paths,
            most_recent: Some(0),
        },
        ..options("")
    };
    let h = compute_viewport_height(&with_many, 0, width);
    // Header (1) + MAX_VISIBLE_ADDED_ROOTS visible paths (3) + overflow (1) = +5.
    assert_eq!(h, baseline_h + (MAX_VISIBLE_ADDED_ROOTS as u16) + 2);
}

/// Validation errors drive their own bounded slot. A short
/// localized error on a wide terminal occupies exactly one row;
/// the empty-error case reserves zero rows so unrelated
/// iterations of the prompt do not pad the layout.
#[test]
fn validation_error_reserves_only_when_present() {
    let width: u16 = 120;
    let baseline_h = compute_viewport_height(&options(""), 0, width);

    let with_error = PathInputOptions {
        validation_error: Some("Error: Path '/x' does not exist."),
        ..options("")
    };
    assert_eq!(
        compute_viewport_height(&with_error, 0, width),
        baseline_h + 1
    );

    let no_error = PathInputOptions {
        validation_error: None,
        ..options("")
    };
    assert_eq!(compute_viewport_height(&no_error, 0, width), baseline_h);
}

/// All optional slots compound: candidates + summary + error
/// each add their own rows on top of the compact base.
#[test]
fn all_optional_slots_compound_on_top_of_compact_base() {
    let width: u16 = 120;
    let paths = vec!["~/a".to_owned()];
    let opts = PathInputOptions {
        validation_error: Some("Error: Path '/x' does not exist."),
        added_summary: AddedRootsSummary {
            header_template: "Trusted roots added ({0}):",
            empty_label: "",
            overflow_template: "… and {0} more",
            paths: &paths,
            most_recent: Some(0),
        },
        ..options("")
    };
    let h = compute_viewport_height(&opts, 5, width);
    let baseline = compute_viewport_height(&options(""), 0, width);
    // +5 candidates +2 (header + 1 path) +1 (validation error).
    assert_eq!(h, baseline + 5 + 2 + 1);
}

/// On a narrow terminal a long validation error must wrap into multiple
/// rows, but the reservation is capped at `MAX_VALIDATION_ERROR_HEIGHT` so
/// the input box and footer remain visible. The error remains readable
/// above the input box on narrow terminals without truncation.
#[test]
fn viewport_height_caps_wrapped_validation_error_on_narrow_terminal() {
    // ~144 cells of localized error text, representative of a
    // path-not-found message produced by the trusted-root
    // validator.
    let long_msg = "Error: Path '/Users/dev/projects/very-long-fixture-path-that-must-wrap-on-narrow-terminals' does not exist or is not a trusted git root.";
    let opts = PathInputOptions {
        validation_error: Some(long_msg),
        ..options("")
    };

    // Wide terminal: even with wrap, the long error must fit
    // inside the bounded slot — strictly greater than zero,
    // strictly less than or equal to the cap.
    let baseline = compute_viewport_height(&options(""), 0, 200);
    let extra_wide = compute_viewport_height(&opts, 0, 200) - baseline;
    assert!(
        (1..=MAX_VALIDATION_ERROR_HEIGHT).contains(&extra_wide),
        "wide terminal: long error reserves a bounded number of rows, got {extra_wide}"
    );

    // Narrow terminal: outer width 30 -> content width 28 ->
    // slot width 26. The ~144-cell message wraps into ~6 rows,
    // which exceeds the cap and must clamp to
    // `MAX_VALIDATION_ERROR_HEIGHT`.
    let baseline_narrow = compute_viewport_height(&options(""), 0, 30);
    let h_narrow = compute_viewport_height(&opts, 0, 30);
    assert_eq!(
        h_narrow,
        baseline_narrow + MAX_VALIDATION_ERROR_HEIGHT,
        "narrow terminal: wrapped error is capped at the bounded slot height"
    );

    // Sanity: cap stays within an inline budget that leaves
    // headroom for the input box (3 rows) + footer (1 row).
    const _: () = assert!(
        MAX_VALIDATION_ERROR_HEIGHT >= 2 && MAX_VALIDATION_ERROR_HEIGHT <= 6,
        "cap must stay within a reasonable inline budget so input box / footer remain visible"
    );
}

/// Wrapped-row count must produce at least one row for any
/// non-empty error message, including very narrow widths where
/// the wrap budget could mathematically round down.
#[test]
fn validation_error_wrapped_height_clamps_to_bounded_range() {
    let short = "Error: Path '/x' does not exist.";
    assert_eq!(
        validation_error_wrapped_height(Some(short), 120),
        1,
        "wide width: short error occupies one row"
    );
    assert_eq!(
        validation_error_wrapped_height(None, 120),
        0,
        "no error: slot consumes zero rows"
    );
    assert_eq!(
        validation_error_wrapped_height(Some(""), 120),
        0,
        "empty error string: slot consumes zero rows"
    );

    let long_msg = "Error: this is a deliberately long localized validation message that needs to wrap onto several rows for bounded-height coverage.";
    let narrow = validation_error_wrapped_height(Some(long_msg), 24);
    assert!(
        (1..=MAX_VALIDATION_ERROR_HEIGHT).contains(&narrow),
        "narrow width: row count must stay within bounded [1, cap] range, got {narrow}"
    );
}

/// Layout-side guard: empty empty-state label + no paths must
/// not reserve any height — otherwise the prompt would jump up
/// for callers that opted out of the summary block entirely.
#[test]
fn added_summary_height_is_zero_when_block_has_no_content() {
    let snap = AddedRootsSnapshot::from(&AddedRootsSummary::EMPTY);
    assert!(!snap.is_renderable());
    assert_eq!(compute_added_summary_height(&snap), 0);
}

/// Layout-side guard: a one-root summary renders header + 1
/// path row, not the worst-case bounded height. This pins the
/// content-driven layout invariant at the renderer level so a
/// future change cannot reintroduce a fixed-height summary
/// reservation.
#[test]
fn added_summary_height_matches_visible_content_for_one_root() {
    let paths = vec!["~/development/".to_owned()];
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: Some(0),
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert!(snap.is_renderable());
    assert_eq!(compute_added_summary_height(&snap), 2);
}

/// Layout-side guard: long summaries collapse to header +
/// `MAX_VISIBLE_ADDED_ROOTS` rows + overflow row, but never
/// more.
#[test]
fn added_summary_height_caps_for_overflowing_lists() {
    let paths: Vec<String> = (0..7).map(|i| format!("~/p/{i}")).collect();
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: Some(0),
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert_eq!(
        compute_added_summary_height(&snap),
        (MAX_VISIBLE_ADDED_ROOTS as u16) + 2
    );
}

/// Pure helper: visible-rows accounting for the summary slot
/// matches the renderer-side height. This pins the contract so
/// `compute_viewport_height` and the runtime renderer cannot
/// drift apart.
#[test]
fn added_summary_visible_rows_matches_compute_added_summary_height() {
    let paths = vec!["~/a".to_owned(), "~/b".to_owned()];
    let summary = AddedRootsSummary {
        header_template: "Trusted roots added ({0}):",
        empty_label: "",
        overflow_template: "… and {0} more",
        paths: &paths,
        most_recent: None,
    };
    let snap = AddedRootsSnapshot::from(&summary);
    assert_eq!(
        added_summary_visible_rows(&summary),
        compute_added_summary_height(&snap)
    );
}

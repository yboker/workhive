use super::*;

// ── Input affordance, ~/ scope, hierarchy preview, child
//    labels, bare-scope / relative warnings. Snapshot-level
//    tests so we cover the data the ratatui render reads,
//    independent of a live terminal.

/// Empty buffer + known home: the snapshot must announce the
/// browse scope (`~/`) and show child labels stripped to the
/// immediate basename. The placeholder text MUST be carried over
/// so the boxed input still reads as a real field. There is no
/// commit preview because the user has not picked anything yet.
#[test]
fn snapshot_empty_buffer_with_home_shows_tilde_scope_and_no_preview() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Applications", "Desktop", "Downloads"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap.scope_line, "Current location: ~/");
    assert_eq!(snap.placeholder, "Browsing under ~/");
    assert!(
        !snap.field_label.is_empty(),
        "boxed input must carry a localized field label"
    );
    assert_eq!(
        snap.child_labels,
        vec![
            "Applications/".to_owned(),
            "Desktop/".to_owned(),
            "Downloads/".to_owned()
        ],
        "candidate list must render relative child names, not full ~/x/"
    );
    assert!(
        snap.preview_line.is_empty(),
        "empty buffer + nothing highlighted must produce no commit preview"
    );
}

/// Typed prefix `~/de` keeps the scope at `~/`, surfaces the
/// matching children with relative names, and renders the
/// commit-preview as the buffer (the would-be-committed path
/// when Enter is pressed without picking a candidate).
#[test]
fn snapshot_typed_partial_under_home_keeps_tilde_scope_and_shows_buffer_preview() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development", "Music"]);
    let mut state = State::from_options(&opts);
    for c in "~/de".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap.scope_line, "Current location: ~/");
    assert_eq!(
        snap.child_labels,
        vec!["dev/".to_owned(), "development/".to_owned()],
    );
    assert_eq!(
        snap.preview_line, "Will add: ~/de",
        "buffer-as-preview must keep the user's authored form"
    );
}

/// Nested prefix `~/development/wo` re-anchors the scope at
/// `~/development/` and renders the children below it.
#[test]
fn snapshot_nested_prefix_uses_immediate_parent_scope() {
    let opts = options("");
    let fs = FakeFs::new().dirs(
        "/Users/me/development",
        &["workhive", "worktree-experiments", "other"],
    );
    let mut state = State::from_options(&opts);
    for c in "~/development/wo".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap.scope_line, "Current location: ~/development/");
    assert_eq!(
        snap.child_labels,
        vec!["workhive/".to_owned(), "worktree-experiments/".to_owned()],
    );
    assert_eq!(snap.preview_line, "Will add: ~/development/wo");
}

/// Highlighted candidate is suggestion-only. The preview line
/// ALWAYS reflects what `Enter` will commit,
/// which is the input buffer — independent of which candidate
/// row is currently highlighted. `Tab` is the only key that
/// promotes a candidate into the buffer.
#[test]
fn snapshot_preview_reflects_buffer_independent_of_highlighted_candidate() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    for c in "~/de".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    // Navigate to the second candidate. Even with `~/development/`
    // highlighted, the preview must keep showing what `Enter`
    // submits today, which is the buffer text `~/de`.
    state.move_candidate_down();
    state.move_candidate_down();

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(
        snap.preview_line, "Will add: ~/de",
        "highlighted candidate must NOT leak into the preview; \
             preview shows what Enter commits, which is the buffer"
    );
}

/// Bare scope buffers (`~`, `~/`, `/`) must NOT be promoted to a
/// commit preview — they would either be rejected (`~` is
/// relative-shaped, `/` and `~/` are too broad). Instead the
/// snapshot carries the localized warning copy so the field
/// tells the user to drill into a child directory.
#[test]
fn snapshot_bare_home_scope_renders_warning_instead_of_preview() {
    for bare in ["~", "~/", "/"] {
        let opts = options("");
        let fs = FakeFs::new().dirs("/Users/me", &["Documents"]);
        let mut state = State::from_options(&opts);
        for c in bare.chars() {
            state.buffer.insert(state.cursor, c);
            state.cursor += 1;
        }
        state.refresh_live_search(opts.home, &unix_strategy(), &fs);

        let snap = state.render_snapshot(&opts, &unix_strategy());
        assert!(
            snap.buffer_is_bare_scope,
            "buffer {bare:?} must be tagged as bare scope"
        );
        assert!(
            !snap.preview_line.starts_with("Will add:"),
            "bare scope {bare:?} must NOT advertise a commit preview, got {:?}",
            snap.preview_line
        );
        assert_eq!(
            snap.preview_line, "Pick a child directory",
            "bare scope must surface the localized warning copy"
        );
    }
}

/// Relative-path buffers WITHOUT an active home browse scope
/// must still carry the relative-path warning. The warning is
/// reserved for code paths that truly receive a relative path
/// with no scope to anchor to.
#[test]
fn snapshot_relative_buffer_renders_relative_warning_when_no_home() {
    let opts = PathInputOptions {
        home: None,
        ..options("")
    };
    let fs = FakeFs::new();
    let mut state = State::from_options(&opts);
    for c in "foo".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.buffer_is_relative,
        "non-empty non-absolute non-tilde buffer with no home must be tagged relative"
    );
    assert_eq!(snap.preview_line, "Trusted roots must be absolute");
    // Without a home we have no scope to anchor to.
    assert_eq!(snap.scope_line, "");
}

/// Bare typed text under an active home browse scope is a scoped
/// filter, NOT a relative path. The relative-path warning must
/// NOT appear, the scope stays at `~/`, and the preview shows
/// the implicit `~/<buffer>` form.
#[test]
fn snapshot_bare_buffer_under_home_is_scoped_filter_not_relative_warning() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Applications", "Documents"]);
    let mut state = State::from_options(&opts);
    for c in "a".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        !snap.buffer_is_relative,
        "scoped filter under ~/ must NOT be tagged as relative"
    );
    assert_ne!(
        snap.preview_line, "Trusted roots must be absolute",
        "scoped filter must not surface the relative-path warning"
    );
    assert_eq!(
        snap.scope_line, "Current location: ~/",
        "scoped filter must keep the visible browse scope at ~/"
    );
    assert_eq!(
        snap.preview_line, "Will add: ~/a",
        "scoped filter preview must show the implicit ~/<buffer> form"
    );
    assert_eq!(
        snap.child_labels,
        vec!["Applications/".to_owned()],
        "scoped filter must surface fuzzy-matched home children"
    );
}

/// Even when a scoped filter has zero matches, the relative-path
/// warning MUST stay suppressed: the visible scope is still `~/`
/// and the user is mid-typing.
///
/// The renderer must surface the localized no-match warning in
/// the preview slot instead of fabricating a `Will add: ~/zzz`
/// preview when no candidate matches.
#[test]
fn snapshot_bare_buffer_under_home_with_no_matches_still_suppresses_relative_warning() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Documents"]);
    let mut state = State::from_options(&opts);
    for c in "zzz".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(state.candidates.is_empty());
    assert!(
        !snap.buffer_is_relative,
        "scoped filter must stay non-relative even with zero matches"
    );
    assert_eq!(snap.scope_line, "Current location: ~/");
    assert!(
        !snap.preview_line.contains("Will add"),
        "no-match scoped filter must NOT echo `Will add: ~/<buffer>`; \
             got: {preview:?}",
        preview = snap.preview_line,
    );
    assert!(
        snap.buffer_is_scope_no_match,
        "no-match scoped filter must arm the Enter-blocking no-match flag"
    );
    assert!(
        snap.preview_line.contains("zzz"),
        "warning line must include the typed buffer for user clarity, \
             got: {preview:?}",
        preview = snap.preview_line,
    );
}

/// Absolute-path buffers behave symmetrically to `~/`-rooted
/// buffers: scope = `/srv/`, child labels stripped to the
/// immediate basename, preview = the typed buffer.
#[test]
fn snapshot_absolute_buffer_uses_absolute_scope() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/srv", &["projects"]);
    let mut state = State::from_options(&opts);
    for c in "/srv/proj".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap.scope_line, "Current location: /srv/");
    assert_eq!(snap.child_labels, vec!["projects/".to_owned()]);
    assert_eq!(snap.preview_line, "Will add: /srv/proj");
}

/// The placeholder is only carried into the snapshot — the boxed
/// input renderer is what actually paints it. Tests assert it
/// stays out of the way once the user has typed anything.
#[test]
fn snapshot_placeholder_only_useful_when_buffer_empty() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Documents"]);
    let mut state = State::from_options(&opts);
    let snap_empty = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap_empty.placeholder, "Browsing under ~/");
    assert!(snap_empty.buffer.is_empty());

    for c in "~/Doc".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    let snap_typed = state.render_snapshot(&opts, &unix_strategy());
    // Placeholder text is still propagated (the renderer ignores
    // it when buffer is non-empty) but the preview kicks in.
    assert!(!snap_typed.buffer.is_empty());
    assert_eq!(snap_typed.preview_line, "Will add: ~/Doc");
}

/// When the buffer is a bare scoped filter (`ddd`) under home and
/// live-search returns zero candidates, `refresh_live_search`
/// must flip `scoped_filter_no_match` so:
///
/// 1. the renderer suppresses the fabricated `~/ddd` preview and
///    surfaces the localized no-match string in its place, and
/// 2. that string is classified as a BLOCKING validation failure
///    — `run()` refuses Enter on this state — so its severity is
///    `Danger` (red + bold), NOT `Warning`.
#[test]
fn snapshot_scoped_filter_no_match_replaces_preview_with_danger_and_blocks_submission() {
    let opts = options("");
    // An empty home directory: scoped filter `ddd` has zero
    // candidates under `~/`.
    let fs = FakeFs::new().dirs("/Users/me", &[]);
    let mut state = State::from_options(&opts);
    for c in "ddd".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(
        state.scoped_filter_no_match,
        "scoped filter under home with zero candidates must arm \
             the Enter-blocking no-match guard"
    );

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        !snap.preview_line.contains("Will add"),
        "no-match scoped filter must NOT surface a `Will add: ~/<buffer>` \
             preview, got: {preview:?}",
        preview = snap.preview_line,
    );
    assert!(
        !snap.preview_line.contains("~/ddd"),
        "no-match scoped filter must NOT echo the buffer as an implicit \
             `~/<buffer>` commit, got: {preview:?}",
        preview = snap.preview_line,
    );
    assert!(
        snap.preview_line.contains("ddd"),
        "the blocking no-match line must include the typed buffer for user \
             clarity, got: {preview:?}",
        preview = snap.preview_line,
    );
    assert!(
        snap.buffer_is_scope_no_match,
        "RenderSnapshot must expose the Enter-blocking no-match flag"
    );
    // The preview slot for an Enter-blocking scoped-filter-no-match MUST be
    // styled as `Danger`, not as `Warning` (which is reserved
    // for non-blocking mid-typing guidance).
    assert_eq!(
        snap.preview_severity,
        MessageSeverity::Danger,
        "scoped-filter no-match blocks Enter; per the severity \
             contract it MUST render as Danger (red+bold), not as the \
             softer Warning used for non-blocking guidance"
    );
}

/// A scoped filter `Doc` whose candidate exists (`Documents/`)
/// must NOT trip the no-match guard. Enter is still allowed and
/// the implicit `~/Doc` preview remains.
#[test]
fn snapshot_scoped_filter_with_match_keeps_preview_and_allows_submission() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Documents"]);
    let mut state = State::from_options(&opts);
    for c in "Doc".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(
        !state.scoped_filter_no_match,
        "scoped filter that matches at least one candidate must NOT \
             arm the no-match guard"
    );
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.preview_line.contains("~/Doc"),
        "matching scoped filter must keep the implicit `~/<buffer>` \
             preview, got: {preview:?}",
        preview = snap.preview_line,
    );
    assert!(
        !snap.buffer_is_scope_no_match,
        "matching scoped filter must NOT raise the Enter-blocking no-match flag"
    );
}

/// Even when live-search returns no candidates, an explicit
/// `~/...` buffer is NOT a scoped filter under the visible scope:
/// the caller's trusted-root validation still decides whether to
/// accept it, so the picker MUST stay submittable and continue to
/// surface the `Will add: ~/...` preview.
#[test]
fn snapshot_explicit_tilde_path_with_no_candidates_remains_submittable() {
    let opts = options("");
    let fs = FakeFs::new(); // empty: zero candidates anywhere
    let mut state = State::from_options(&opts);
    for c in "~/code/never-exists".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(
        !state.scoped_filter_no_match,
        "explicit `~/...` paths are owned by trusted-root validation, \
             not by the picker's UI no-match guard"
    );
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.preview_line.contains("Will add"),
        "explicit `~/...` input keeps the commit preview so the user can \
             still submit and let the validator decide; got: {preview:?}",
        preview = snap.preview_line,
    );
}

// -----------------------------------------------------------------
// Severity / added-roots summary / validation-error tests
//
// Coverage anchor: severity-styling and added-summary invariants.
// -----------------------------------------------------------------

/// Snapshot must carry a blocking validation-error string when
/// the caller threads one in. This is the seam that drives the
/// in-frame `Danger`-styled rejection line in the path-input
/// screen.
#[test]
fn snapshot_threads_caller_validation_error_through() {
    let opts = PathInputOptions {
        validation_error: Some("Error: Path '/missing' does not exist."),
        ..options("")
    };
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(
        snap.validation_error.as_deref(),
        Some("Error: Path '/missing' does not exist."),
        "snapshot must surface the caller-supplied validation \
             error verbatim so the renderer can style it with Danger"
    );
}

/// Empty / missing caller validation error must produce `None`
/// — the renderer reserves no row in that case.
#[test]
fn snapshot_omits_empty_validation_error() {
    let opts = PathInputOptions {
        validation_error: Some(""),
        ..options("")
    };
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.validation_error.is_none(),
        "empty validation_error must collapse to None so the \
             renderer hides the row instead of drawing a blank \
             Danger-styled gap"
    );

    let opts2 = PathInputOptions {
        validation_error: None,
        ..options("")
    };
    let snap2 = State::from_options(&opts2).render_snapshot(&opts2, &unix_strategy());
    assert!(snap2.validation_error.is_none());
}

/// Re-entry seam: after a validation rejection the trusted-root
/// loop re-opens this prompt with the rejected text as the initial
/// buffer and the localized feedback as the in-frame validation
/// error. Both must survive into the re-entered frame so the user
/// can edit or Tab-complete the rejected value instead of retyping
/// it, and sees why it was rejected. This drives the real
/// `ValidatedInputRetry` seam the wizard threads into
/// `PathInputOptions { initial, validation_error }`.
#[test]
fn validation_rejection_preserves_rejected_input_and_feedback_on_reentry() {
    let mut retry = ValidatedInputRetry::new();
    retry.reject("./develop", "Trusted roots must be absolute");

    // Exactly the options the wizard rebuilds for the next prompt
    // after a rejection.
    let opts = PathInputOptions {
        initial: retry.initial(),
        validation_error: retry.feedback(),
        ..options("")
    };
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());

    assert_eq!(
        snap.buffer, "./develop",
        "the rejected text must be preserved as the re-entered buffer so the user can edit or complete it"
    );
    assert_eq!(
        snap.validation_error.as_deref(),
        Some("Trusted roots must be absolute"),
        "the localized rejection feedback must surface in-frame on re-entry"
    );
}

/// Severity decision must come from the buffer-classification
/// flags, not from text matching. Empty buffer + no candidates
/// has no preview line, so severity falls back to `Normal`.
#[test]
fn snapshot_preview_severity_is_normal_when_preview_line_is_empty() {
    let opts = PathInputOptions {
        home: None,
        ..options("")
    };
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(snap.preview_line, "");
    assert_eq!(snap.preview_severity, MessageSeverity::Normal);
}

/// Commit-preview lines (highlighted candidate / typed
/// absolute) must render with `Preview` severity — the shared
/// positive palette.
#[test]
fn snapshot_preview_severity_is_preview_for_committable_buffer() {
    let opts = options("/Users/me/Downloads");
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.preview_line.starts_with("Will add:"),
        "committable buffer must surface a Will add preview"
    );
    assert_eq!(snap.preview_severity, MessageSeverity::Preview);
}

/// Bare-scope and relative-path buffers are mid-typing,
/// CONTINUOUSLY-RENDERED states: every keystroke triggers
/// `refresh_live_search`, so the renderer paints them on the
/// way to a committable form (`~/` becoming `~/Documents`, or
/// `foo` becoming `~/foo`). Surfacing these transient typing
/// states with the bold-red `Danger` palette would drown out
/// the actual blocking signals (the in-frame
/// `validation_error` row and the scoped-filter no-match
/// preview); per the severity contract they stay `Warning`
/// (yellow, no bold) as non-blocking mid-typing guidance.
///
/// Slots that ARE classified as `Danger`:
///   - the scoped-filter no-match preview (covered by
///     `snapshot_scoped_filter_no_match_replaces_preview_with_danger_and_blocks_submission`),
///     because `run()` refuses Enter on that state;
///   - the Tab-no-match hint in the candidates slot, in
///     EVERY branch (covered by
///     `snapshot_no_match_hint_severity_is_danger_for_every_tab_no_match`).
///     The slot only appears after a deliberate Tab on a
///     no-match buffer, and submission is blocked end-to-end
///     in both the picker-level (`scoped_filter_no_match`)
///     and caller-level (explicit `~/...` / absolute path
///     rejected by trusted-root validation on the next frame)
///     branches, so the slot uses the unified blocking-
///     failure palette in both;
///   - the `validation_error` slot above the input box
///     (covered by `snapshot_threads_caller_validation_error_through`),
///     for blocking validation failures forwarded by the
///     caller.
#[test]
fn snapshot_preview_severity_is_warning_for_non_blocking_states() {
    // bare-scope: typing only `~` is a transient mid-typing
    // state (the user is on the way to `~/Downloads` etc.);
    // continuously painting it Danger on every keystroke
    // would be hostile, so the preview slot stays `Warning`.
    let opts = options("~");
    let state = State::from_options(&opts);
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(snap.buffer_is_bare_scope);
    assert_eq!(snap.preview_severity, MessageSeverity::Warning);

    // relative-path-shaped buffer without home: same mid-typing
    // logic — the user may still prepend `~/` or `/`, so the
    // preview slot stays `Warning` as soft guidance.
    let opts2 = PathInputOptions {
        home: None,
        ..options("")
    };
    let mut state2 = State::from_options(&opts2);
    for c in "foo".chars() {
        state2.buffer.insert(state2.cursor, c);
        state2.cursor += 1;
    }
    let snap2 = state2.render_snapshot(&opts2, &unix_strategy());
    assert!(snap2.buffer_is_relative);
    assert_eq!(snap2.preview_severity, MessageSeverity::Warning);
}

/// The Tab-no-match hint slot is always a blocking-submission preview,
/// because the slot only appears after the user has explicitly pressed Tab
/// and the picker returned zero candidates. In product terms submission is
/// blocked in either branch:
///
/// 1. **Scoped-filter no-match** — the picker refuses Enter
///    directly inside `run()`'s `KeyCode::Enter` arm. Picker-
///    level block.
/// 2. **Explicit `~/...` / absolute path with no candidate
///    match** — the picker still forwards Enter, but the
///    caller's `classify_trusted_root_input_with_strategy`
///    rejects the submission (`DoesNotExist` /
///    `NotADirectory` / `TooBroad`) and the next iteration
///    surfaces that rejection in the in-frame
///    `validation_error` slot, which is itself styled
///    `Danger`. Caller-level block.
///
/// Both branches MUST therefore render the Tab-no-match hint with
/// `MessageSeverity::Danger` (red + bold). A blocking-submission state
/// cannot use the softer non-blocking palette.
#[test]
fn snapshot_no_match_hint_severity_is_danger_for_every_tab_no_match() {
    // (1) Scoped-filter no-match — Enter is picker-level
    // blocked inside `run()`.
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &[]);
    let mut state = State::from_options(&opts);
    for c in "ddd".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    // Simulate the user pressing Tab on the no-match buffer:
    // `run_completion` is what arms `no_match` so the candidates
    // slot actually surfaces the localized `no_match_hint`.
    state.run_completion(opts.home, &unix_strategy(), &fs);
    assert!(
        state.no_match,
        "Tab on a no-match buffer must arm the Tab-no-match hint"
    );
    assert!(
        state.scoped_filter_no_match,
        "the underlying scoped-filter no-match flag must remain \
             armed across Tab so the picker-level block stays visible"
    );
    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert_eq!(
        snap.no_match_hint_severity,
        MessageSeverity::Danger,
        "scoped-filter no-match blocks Enter at the picker \
             level, so the Tab-no-match hint MUST render as Danger"
    );

    // (2) Explicit `~/...` path with no candidates. The picker
    // forwards Enter, but the caller's trusted-root validator
    // will reject the submission with `DoesNotExist`, so the
    // submission is still blocked end-to-end. The hint MUST also render as
    // Danger here, because the
    // slot describes a blocking-submission state and cannot
    // inherit the softer Warning palette reserved for
    // non-blocking mid-typing guidance.
    let opts2 = options("");
    let fs2 = FakeFs::new(); // no entries anywhere
    let mut state2 = State::from_options(&opts2);
    for c in "~/never-exists/x".chars() {
        state2.buffer.insert(state2.cursor, c);
        state2.cursor += 1;
    }
    state2.refresh_live_search(opts2.home, &unix_strategy(), &fs2);
    state2.run_completion(opts2.home, &unix_strategy(), &fs2);
    assert!(state2.no_match);
    assert!(
        !state2.scoped_filter_no_match,
        "explicit `~/...` inputs are not picker-level blocked, \
             but caller-level trusted-root validation still rejects \
             a non-existent path"
    );
    let snap2 = state2.render_snapshot(&opts2, &unix_strategy());
    assert_eq!(
        snap2.no_match_hint_severity,
        MessageSeverity::Danger,
        "Tab-no-match on an explicit `~/...` path with no \
             candidate match MUST render as Danger: caller-level \
             validation will block submission on the next iteration, \
             so the slot describes a blocking-submission state and \
             cannot inherit the softer Warning palette reserved for \
             non-blocking mid-typing guidance"
    );
}

/// `Warning` and `Preview` MUST NOT inherit the Red+Bold styling
/// reserved for `Danger`. This is the structural anti-regression
/// that prevents non-blocking hints from looking like blocking
/// validation failures.
#[test]
fn warning_and_preview_do_not_inherit_danger_style() {
    let danger = MessageSeverity::Danger.style();
    let warning = MessageSeverity::Warning.style();
    let preview = MessageSeverity::Preview.style();

    assert_ne!(warning.fg, danger.fg, "warning must NOT use Danger's color");
    assert_ne!(preview.fg, danger.fg, "preview must NOT use Danger's color");

    assert!(
        !warning
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD),
        "warning must NOT inherit Danger's bold treatment"
    );
}

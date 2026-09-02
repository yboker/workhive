use super::*;

#[test]
fn candidate_window_keeps_highlighted_candidate_visible() {
    assert_eq!(
        candidate_window(20, Some(0), 8),
        CandidateWindow {
            start: 0,
            len: 8,
            local_highlighted: Some(0),
        }
    );
    assert_eq!(
        candidate_window(20, Some(8), 8),
        CandidateWindow {
            start: 1,
            len: 8,
            local_highlighted: Some(7),
        }
    );
    assert_eq!(
        candidate_window(20, Some(19), 8),
        CandidateWindow {
            start: 12,
            len: 8,
            local_highlighted: Some(7),
        }
    );
}

/// On entry the prompt should immediately surface the home child
/// directories (without `~/` itself) so the user can navigate
/// without first pressing Tab. Whenever the candidate panel has at
/// least one row, the active suggestion is on row 0. Tab applies the
/// active suggestion; Enter still submits only the input buffer.
#[test]
fn entering_empty_prompt_shows_home_children_without_tilde_root() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["development", "projects", ".cache"]);
    let mut state = State::from_options(&opts);

    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    assert_eq!(state.buffer, Vec::<char>::new());
    assert_eq!(
        state.candidate_state.selected(),
        Some(0),
        "candidate panels with at least one row must surface a \
             highlighted active suggestion on row 0"
    );
    assert!(
        !state.candidates.contains(&"~/".to_owned()),
        "`~/` itself must NOT appear as an empty-input candidate"
    );
    assert_eq!(
        state.candidates,
        vec!["~/development/".to_owned(), "~/projects/".to_owned()]
    );
}

/// Live search: typing `~/de` immediately surfaces `~/dev/` and
/// `~/development/` without a Tab press.
#[test]
fn typing_refreshes_candidates_for_live_prefix() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development", "projects", "Music"]);
    let mut state = State::from_options(&opts);

    for c in "~/de".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
        state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    }

    assert_eq!(state.buffer.iter().collect::<String>(), "~/de");
    assert_eq!(
        state.candidates,
        vec!["~/dev/".to_owned(), "~/development/".to_owned()]
    );
}

/// Backspace must also re-run live search, not leave stale
/// candidates frozen on screen.
#[test]
fn backspace_refreshes_candidates() {
    let opts = options("~/dex");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(state.candidates.is_empty(), "no `dex*` should match");

    // Simulate Backspace dropping the trailing `x`.
    state.cursor -= 1;
    state.buffer.remove(state.cursor);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.candidates,
        vec!["~/dev/".to_owned(), "~/development/".to_owned()]
    );
}

/// Live search against a no-match prefix MUST NOT raise the
/// `no_match` hint mid-typing; it is reserved for explicit
/// Tab/Enter actions so users do not get yelled at on every key.
#[test]
fn live_search_no_match_does_not_raise_no_match_hint() {
    let opts = options("~/zzz");
    let fs = FakeFs::new().dirs("/Users/me", &["development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(state.candidates.is_empty());
    assert!(!state.no_match);
}

/// Tab on a single match must still snap the buffer to the
/// completion (use case behaviour) so the user can keep typing.
#[test]
fn tab_completion_still_snaps_to_single_match() {
    let opts = options("~/de");
    let fs = FakeFs::new().dirs("/Users/me", &["development"]);
    let mut state = State::from_options(&opts);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/development/",
        "Tab on a unique match should snap the buffer to the completion"
    );
}

/// Shell-style browsing: Tab completion that lands the buffer
/// on an existing directory path ending in `/` must refresh
/// the candidate slot with that directory's child directories
/// so the user can keep navigating without re-typing or
/// pressing Tab again. Verifies the post-completion candidate
/// list mirrors the children of the completed directory.
#[test]
fn tab_completion_to_existing_directory_refreshes_child_candidates() {
    let opts = options("~/de");
    let fs = FakeFs::new()
        .dirs("/Users/me", &["development"])
        .dirs("/Users/me/development", &["alpha", "beta"]);
    let mut state = State::from_options(&opts);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/development/",
        "Tab on a unique match must snap the buffer to the directory"
    );
    assert!(
        !state.candidates.is_empty(),
        "Tab onto an existing directory must repopulate the candidate slot with children"
    );
    assert_eq!(
        state.candidates,
        vec![
            "~/development/alpha/".to_owned(),
            "~/development/beta/".to_owned(),
        ],
        "post-completion candidates must list child directories of the completed path"
    );
    assert!(
        !state.dir_no_children,
        "directory with browsable children must NOT set the no-children hint"
    );
}

/// When Tab completion lands the buffer on an existing
/// directory path that has no browsable child directories,
/// the renderer must surface a localized hint in the candidate
/// slot instead of a blank panel. The state-level signal is
/// the `dir_no_children` flag, and the candidate list itself
/// is empty (no fabricated rows).
#[test]
fn tab_completion_to_empty_directory_arms_dir_no_children_hint() {
    let opts = options("~/de");
    // Directory exists, but has no child directories.
    let fs = FakeFs::new()
        .dirs("/Users/me", &["development"])
        .dirs("/Users/me/development", &[]);
    let mut state = State::from_options(&opts);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/development/",
        "Tab on a unique match must still snap the buffer to the directory"
    );
    assert!(
        state.candidates.is_empty(),
        "completed directory with no children must not fabricate candidate rows"
    );
    assert!(
        state.dir_no_children,
        "completed directory with no children must arm the dir-no-children hint"
    );
}

/// Typing after a Tab completion that armed the dir-no-children
/// hint must dismiss the hint: the user has moved on and the
/// candidate slot must reflect the current buffer, not stale
/// post-completion feedback.
#[test]
fn live_typing_after_dir_no_children_hint_dismisses_it() {
    let opts = options("~/de");
    let fs = FakeFs::new()
        .dirs("/Users/me", &["development"])
        .dirs("/Users/me/development", &[]);
    let mut state = State::from_options(&opts);

    state.run_completion(opts.home, &unix_strategy(), &fs);
    assert!(state.dir_no_children, "precondition: hint is armed");

    // Simulate the next keystroke clearing the buffer.
    state.buffer.clear();
    state.cursor = 0;
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    assert!(
        !state.dir_no_children,
        "live typing after a dir-no-children completion must clear the hint"
    );
}

/// Render-snapshot integration: when `dir_no_children` is set
/// and the caller supplied a localized hint, the snapshot
/// carries the hint text so the renderer can fill the
/// candidate slot without depending on the runtime state.
#[test]
fn snapshot_carries_dir_no_children_hint_after_completion_to_empty_dir() {
    let opts = options("~/de");
    let fs = FakeFs::new()
        .dirs("/Users/me", &["development"])
        .dirs("/Users/me/development", &[]);
    let mut state = State::from_options_with_width(&opts, 120);
    state.run_completion(opts.home, &unix_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &unix_strategy());
    assert!(
        snap.dir_no_children,
        "snapshot must propagate dir_no_children when the directory has no children"
    );
    assert_eq!(
        snap.dir_no_children_hint, "no child directories",
        "snapshot must carry the caller-provided dir-no-children hint copy"
    );
    assert!(
        !snap.no_match,
        "completing to an existing directory must not arm the no-match hint"
    );
}

/// Tab on multiple matches highlights the first candidate so
/// arrow keys + Enter can pick a row.
#[test]
fn tab_on_multiple_matches_highlights_first_candidate() {
    let opts = options("~/de");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(state.candidate_state.selected(), Some(0));
    assert_eq!(
        state.candidates,
        vec!["~/dev/".to_owned(), "~/development/".to_owned()]
    );
}

/// Esc with a candidate panel open must dismiss the panel
/// only and never bubble out to the wizard.
#[test]
fn esc_dismisses_candidate_panel_without_cancelling_field() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["development", "projects"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(!state.candidates.is_empty());

    let dismissed = state.dismiss_candidates_if_visible();

    assert!(
        dismissed,
        "Esc with visible panel must report it dismissed something"
    );
    assert!(state.candidates.is_empty());
    assert!(!state.no_match);
}

/// Esc with NO panel visible reports nothing-to-dismiss, so the
/// caller (the event loop in `run`) returns Cancelled.
#[test]
fn esc_with_no_candidates_reports_nothing_dismissed() {
    let opts = options("~/dex");
    let fs = FakeFs::new().dirs("/Users/me", &["development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert!(state.candidates.is_empty());

    let dismissed = state.dismiss_candidates_if_visible();
    assert!(!dismissed);
}

/// Arrows move the active suggestion only; `Tab` is the key that
/// writes the highlighted candidate into the buffer. Together they
/// reproduce zsh menu-select / fish
/// autosuggestion semantics on top of an inline path input.
#[test]
fn arrow_navigation_then_tab_applies_highlighted_candidate() {
    // `~/dev` is the shared common prefix of the two candidates,
    // so the first Tab cannot extend the buffer further. That
    // means the prior_highlight branch in `run_completion` is
    // exercised: Tab applies the row arrows landed on.
    let opts = options("~/dev");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    // Default-on active suggestion: the candidate panel must
    // surface a highlighted row as soon as candidates exist, so
    // operators can immediately see what `Tab` will apply.
    assert_eq!(
        state.candidate_state.selected(),
        Some(0),
        "live-search must highlight row 0 by default so the \
             active suggestion is always visible"
    );

    state.move_candidate_down();
    assert_eq!(state.candidate_state.selected(), Some(1));

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(state.buffer.iter().collect::<String>(), "~/development/");
    assert!(
        state.candidates.is_empty(),
        "applying the highlighted candidate must clear the panel"
    );
}

/// When multiple candidates share a longer common prefix than the
/// current input, `Tab` first extends the
/// buffer to that common prefix (zsh-style "first Tab fills the
/// common prefix") and keeps the candidate panel open so a
/// following Tab can disambiguate.
#[test]
fn tab_extends_buffer_to_longest_common_prefix_when_it_strictly_extends_input() {
    let opts = options("~/de");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/dev",
        "Tab must extend `~/de` to the longest common prefix `~/dev`"
    );
    assert!(
        !state.candidates.is_empty(),
        "panel must stay open after a common-prefix extension so a \
             following Tab can disambiguate"
    );
}

/// Many candidates with no further common prefix and a visible
/// highlight: Tab writes the highlighted candidate into the buffer.
#[test]
fn tab_commits_visible_default_highlight_without_common_prefix() {
    let opts = options("~/dev");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);
    assert_eq!(
        state.candidate_state.selected(),
        Some(0),
        "live search must surface a default highlighted candidate"
    );

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/dev/",
        "Tab must commit the visible highlighted candidate"
    );
    assert!(
        state.candidates.is_empty(),
        "committing the candidate must close the panel"
    );
}

/// A visible default highlight is immediately actionable with Tab.
#[test]
fn first_tab_commits_visible_default_highlight() {
    let opts = options("~/dev");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/dev/",
        "first Tab must commit the highlighted candidate"
    );
    assert!(
        state.candidates.is_empty(),
        "committing the candidate must close the panel"
    );
}

/// After a common-prefix extension, the next Tab applies the visible
/// highlighted candidate when there is no further common prefix.
#[test]
fn tab_after_common_prefix_extension_commits_visible_highlight() {
    let opts = options("~/de");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    // First Tab extends `~/de` -> `~/dev` (common prefix).
    state.run_completion(opts.home, &unix_strategy(), &fs);
    assert_eq!(state.buffer.iter().collect::<String>(), "~/dev");

    state.run_completion(opts.home, &unix_strategy(), &fs);
    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/dev/",
        "the Tab after a common-prefix extension must commit the visible highlighted candidate"
    );
    assert!(state.candidates.is_empty());
}

/// Arrow navigation changes the highlighted candidate, and Tab
/// applies the visible highlighted row.
#[test]
fn arrow_navigation_then_tab_commits_visible_highlight() {
    let opts = options("~/dev");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    state.move_candidate_down();
    assert_eq!(state.candidate_state.selected(), Some(1));

    state.run_completion(opts.home, &unix_strategy(), &fs);

    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/development/",
        "an explicit ↑/↓ selection must let the first Tab commit it"
    );
    assert!(state.candidates.is_empty());
}

/// Enter submits the buffer, never a highlighted candidate: even
/// with a highlight on a different row, the value Enter
/// would commit (the buffer) is unaffected by the highlight.
#[test]
fn enter_submits_buffer_not_highlighted_candidate() {
    let opts = options("~/dev");
    let fs = FakeFs::new().dirs("/Users/me", &["dev", "development"]);
    let mut state = State::from_options(&opts);
    state.refresh_live_search(opts.home, &unix_strategy(), &fs);

    // Explicitly highlight a different candidate with the arrows.
    state.move_candidate_down();
    assert_eq!(state.candidate_state.selected(), Some(1));

    // The Enter handler submits `state.buffer`; the highlight must
    // not have leaked into it.
    assert_eq!(
        state.buffer.iter().collect::<String>(),
        "~/dev",
        "Enter must submit the typed buffer, never the highlighted \
             candidate"
    );
}

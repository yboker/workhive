use super::*;

// ── Pure helpers — small, focused unit coverage ──────────────

#[test]
fn compute_scope_uses_first_candidate_prefix_when_available() {
    let candidates = vec!["~/dev/projects/".to_owned(), "~/dev/notes/".to_owned()];
    assert_eq!(
        compute_scope(
            "~/dev/p",
            &candidates,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        Some("~/dev/".to_owned())
    );
}

#[test]
fn compute_scope_empty_buffer_falls_back_to_home() {
    assert_eq!(
        compute_scope("", &[], Some(Utf8Path::new("/Users/me")), &unix_strategy()),
        Some("~/".to_owned())
    );
}

#[test]
fn compute_scope_relative_buffer_yields_none() {
    assert_eq!(
        compute_scope(
            "foo/bar",
            &[],
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        None
    );
}

#[test]
fn compute_child_label_strips_scope_prefix() {
    assert_eq!(
        compute_child_label("~/Applications/", "~/"),
        "Applications/"
    );
    assert_eq!(compute_child_label("/srv/projects/", "/srv/"), "projects/");
    assert_eq!(
        compute_child_label("~/Applications/", ""),
        "~/Applications/",
        "empty scope must NOT panic and must fall through unchanged"
    );
}

/// The preview ALWAYS reflects what `Enter` will submit, which is
/// the input buffer. A highlighted
/// candidate is visual-only — only `Tab` writes a candidate into
/// the buffer.
#[test]
fn compute_preview_ignores_highlighted_candidate_and_reflects_buffer() {
    let cands = vec!["~/dev/".to_owned(), "~/development/".to_owned()];
    assert_eq!(
        compute_preview(
            "~/Downloads",
            &cands,
            Some(1),
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy(),
        ),
        Some("~/Downloads".to_owned()),
        "highlighted candidate must not leak into the preview; \
             Enter is shell-like and submits the buffer text"
    );
}

#[test]
fn compute_preview_buffer_when_no_highlight() {
    assert_eq!(
        compute_preview(
            "~/Downloads",
            &[],
            None,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        Some("~/Downloads".to_owned())
    );
    assert_eq!(
        compute_preview(
            "/srv/proj",
            &[],
            None,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        Some("/srv/proj".to_owned())
    );
}

#[test]
fn compute_preview_bare_scope_returns_none() {
    for bare in ["~", "~/", "/"] {
        assert_eq!(
            compute_preview(
                bare,
                &[],
                None,
                Some(Utf8Path::new("/Users/me")),
                &unix_strategy()
            ),
            None,
            "bare scope {bare:?} must not advertise a commit preview"
        );
    }
}

/// Bare single-segment buffer under an active home scope must
/// preview as the implicit `~/<buffer>` form so the picker UI
/// agrees with the visible scope.
#[test]
fn compute_preview_bare_filter_under_home_uses_implicit_tilde_prefix() {
    assert_eq!(
        compute_preview(
            "a",
            &[],
            None,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        Some("~/a".to_owned())
    );
    assert_eq!(
        compute_preview(
            "Doc",
            &[],
            None,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        Some("~/Doc".to_owned())
    );
}

#[test]
fn compute_preview_relative_buffer_without_home_returns_none() {
    assert_eq!(
        compute_preview("foo", &[], None, None, &unix_strategy()),
        None
    );
    assert_eq!(
        compute_preview("foo/bar", &[], None, None, &unix_strategy()),
        None
    );
    // Multi-segment relative still returns None even with home —
    // it has no anchor without an explicit absolute or `~/...`
    // root.
    assert_eq!(
        compute_preview(
            "foo/bar",
            &[],
            None,
            Some(Utf8Path::new("/Users/me")),
            &unix_strategy()
        ),
        None
    );
}

/// Pure-helper coverage: `compute_scope` must accept Windows
/// roots through the strategy, not only the legacy Unix forms.
#[test]
fn compute_scope_handles_windows_drive_root_inputs() {
    assert_eq!(
        compute_scope(r"C:\Users\me\dev", &[], None, &windows_strategy()),
        Some(r"C:\Users\me\".to_owned())
    );
    assert_eq!(
        compute_scope("C:/Users/me/dev", &[], None, &windows_strategy()),
        Some("C:/Users/me/".to_owned())
    );
    assert_eq!(
        compute_scope(r"\\server\share\projects", &[], None, &windows_strategy()),
        Some(r"\\server\share\".to_owned())
    );
}

/// Pure-helper coverage: `compute_preview` must accept Windows
/// drive-rooted and UNC inputs as committable previews and NOT
/// fall through to the relative-path warning.
#[test]
fn compute_preview_handles_windows_drive_root_inputs() {
    assert_eq!(
        compute_preview(
            r"C:\Users\me\Downloads",
            &[],
            None,
            None,
            &windows_strategy(),
        ),
        Some(r"C:\Users\me\Downloads".to_owned()),
    );
    assert_eq!(
        compute_preview(
            r"\\server\share\projects",
            &[],
            None,
            None,
            &windows_strategy(),
        ),
        Some(r"\\server\share\projects".to_owned()),
    );
}

#[test]
fn format_label_replaces_path_token() {
    assert_eq!(
        format_label("Current location: {path}", "~/dev/"),
        "Current location: ~/dev/"
    );
    assert_eq!(
        format_label("当前位置: {path}", "~/development/"),
        "当前位置: ~/development/"
    );
    assert_eq!(
        format_label("plain text without token", "ignored"),
        "plain text without token"
    );
}

// Windows production-path coverage.
//
// Windows drive-rooted and UNC inputs must flow through the real
// `path_input` scope/preview logic, not only `path_strategy` unit tests.
// The cases below run snapshot-level assertions through
// `WindowsPathStrategy` so any regression is caught at the production
// boundary.

/// Drive-rooted Windows input: scope and preview keep the user's
/// authored backslash style and do not surface the relative-path
/// warning. Candidate child labels strip the resolved scope.
#[test]
fn snapshot_windows_drive_rooted_input_renders_drive_scope_and_preview() {
    let opts = options("");
    let fs = FakeFs::new().dirs(r"C:\Users\me", &["Documents", "Downloads"]);
    let mut state = State::from_options(&opts);
    for c in r"C:\Users\me\D".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &windows_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &windows_strategy());
    assert_eq!(snap.scope_line, r"Current location: C:\Users\me\");
    assert_eq!(
        snap.child_labels,
        vec![r"Documents\".to_owned(), r"Downloads\".to_owned()],
        "scope strip must work and candidates must keep the user's \
             backslash style"
    );
    assert_eq!(snap.preview_line, r"Will add: C:\Users\me\D");
    assert!(
        !snap.buffer_is_relative,
        "drive-rooted input must NOT trigger the relative-path warning"
    );
}

/// Drive-rooted Windows input typed with forward slashes must
/// render in forward-slash form.
#[test]
fn snapshot_windows_drive_rooted_forward_slash_preserves_authoring() {
    let opts = options("");
    let fs = FakeFs::new().dirs("C:/Users/me", &["Documents"]);
    let mut state = State::from_options(&opts);
    for c in "C:/Users/me/D".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &windows_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &windows_strategy());
    assert_eq!(snap.scope_line, "Current location: C:/Users/me/");
    assert_eq!(snap.child_labels, vec!["Documents/".to_owned()]);
    assert_eq!(snap.preview_line, "Will add: C:/Users/me/D");
}

/// UNC input: scope and preview must keep the UNC root and never
/// degrade to the relative-path warning.
#[test]
fn snapshot_windows_unc_input_renders_unc_scope_and_preview() {
    let opts = options("");
    let fs = FakeFs::new().dirs(r"\\server\share\projects", &["alpha", "beta"]);
    let mut state = State::from_options(&opts);
    for c in r"\\server\share\projects\al".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &windows_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &windows_strategy());
    assert_eq!(
        snap.scope_line,
        r"Current location: \\server\share\projects\"
    );
    assert_eq!(snap.child_labels, vec![r"alpha\".to_owned()]);
    assert_eq!(snap.preview_line, r"Will add: \\server\share\projects\al");
    assert!(
        !snap.buffer_is_relative,
        "UNC input must NOT trigger the relative-path warning"
    );
}

/// Windows-bare scopes (`C:\`, `C:/`, `\\server\share\`) must
/// surface the bare-scope warning instead of a commit preview.
#[test]
fn snapshot_windows_bare_drive_or_unc_root_renders_bare_scope_warning() {
    for bare in [r"C:\", "C:/", r"\\server\share\"] {
        let opts = options("");
        let fs = FakeFs::new();
        let mut state = State::from_options(&opts);
        for c in bare.chars() {
            state.buffer.insert(state.cursor, c);
            state.cursor += 1;
        }
        state.refresh_live_search(opts.home, &windows_strategy(), &fs);

        let snap = state.render_snapshot(&opts, &windows_strategy());
        assert!(
            snap.buffer_is_bare_scope,
            "Windows bare input {bare:?} must be tagged as bare scope"
        );
        assert_eq!(
            snap.preview_line, "Pick a child directory",
            "Windows bare input {bare:?} must surface the bare-scope warning"
        );
    }
}

/// Posix-shaped absolute input under `WindowsPathStrategy` is NOT
/// a Windows-absolute trusted-root candidate. The picker MUST NOT
/// render a `Will add: /Users/me` preview — that would silently
/// promote a non-portable path.
#[test]
fn snapshot_windows_strategy_rejects_posix_absolute_preview() {
    let opts = PathInputOptions {
        home: None,
        ..options("")
    };
    let fs = FakeFs::new();
    let mut state = State::from_options(&opts);
    for c in "/Users/me/dev".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &windows_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &windows_strategy());
    assert!(
        snap.buffer_is_relative,
        "posix-shaped absolute input must be classified as relative \
             under WindowsPathStrategy so the wizard rejects it"
    );
    assert_eq!(snap.preview_line, "Trusted roots must be absolute");
}

/// Bare typed text under `WindowsPathStrategy` with a known home
/// keeps the scoped-filter behaviour: the user is filtering home
/// children, NOT entering an invalid relative path.
#[test]
fn snapshot_windows_bare_input_under_home_is_scoped_filter() {
    let opts = options("");
    let fs = FakeFs::new().dirs("/Users/me", &["Applications", "Documents"]);
    let mut state = State::from_options(&opts);
    for c in "Doc".chars() {
        state.buffer.insert(state.cursor, c);
        state.cursor += 1;
    }
    state.refresh_live_search(opts.home, &windows_strategy(), &fs);

    let snap = state.render_snapshot(&opts, &windows_strategy());
    assert!(!snap.buffer_is_relative);
    assert_eq!(snap.scope_line, "Current location: ~/");
    assert_eq!(snap.preview_line, "Will add: ~/Doc");
    assert_eq!(snap.child_labels, vec!["Documents/".to_owned()]);
}

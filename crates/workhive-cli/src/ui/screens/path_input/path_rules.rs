//! Pure path/scope decision rules for the path-input screen.
//!
//! These functions decide browse scope, child labels, commit
//! preview, and buffer classification from the authored buffer and
//! the active [`PathStrategy`]. They hold no UI state so the screen
//! and its tests can exercise the decisions in isolation.

use camino::Utf8Path;
use workhive_core::usecases::path_strategy::PathStrategy;

/// Compute the user-visible browse scope.
///
/// Rules:
/// 1. Candidates always share a parent (the live-search use case
///    only ever lists siblings), so when present we derive the scope
///    from the first candidate's prefix. This keeps the displayed
///    scope perfectly aligned with the visible candidate list.
/// 2. Otherwise an empty buffer with a known home maps to `~/` so
///    the user can tell the field defaults to home browsing.
/// 3. Otherwise an absolute or tilde-rooted buffer maps to its
///    immediate parent (the part up to and including the last
///    separator the active strategy understands).
/// 4. A bare single-segment buffer with a known home is interpreted
///    as a scoped filter under `~/`, so the scope stays at `~/`
///    while the user is filtering.
/// 5. Anything else yields `None` — the buffer is either an
///    incomplete tilde token (`~`) or a multi-segment relative path
///    with no scope to anchor to; the warning line will explain.
pub(super) fn compute_scope(
    buffer: &str,
    candidates: &[String],
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
) -> Option<String> {
    if let Some(first) = candidates.first() {
        // Candidates inherit the user's authored separator style, so
        // we ask the strategy which characters to look for.
        let separators = strategy.path_separators();
        // Trim the trailing candidate separator before searching for
        // the parent's tail separator: candidates always end with one
        // of the strategy's separators.
        let trimmed = trim_trailing_separators(first, strategy);
        if let Some(idx) = trimmed.rfind(separators) {
            return Some(first[..=idx].to_owned());
        }
    }
    let trimmed = buffer.trim();
    if trimmed.is_empty() && home.is_some() {
        return Some("~/".to_owned());
    }
    if trimmed == "~" {
        return Some("~/".to_owned());
    }
    if trimmed.starts_with("~/") || strategy.is_absolute(trimmed) {
        let separators = strategy.path_separators();
        if let Some(idx) = trimmed.rfind(separators) {
            return Some(trimmed[..=idx].to_owned());
        }
        if let Some((root, _)) = strategy.split_absolute_root(trimmed) {
            return Some(root.to_owned());
        }
    }
    if buffer_is_scoped_filter(buffer, home, strategy) {
        return Some("~/".to_owned());
    }
    None
}

/// Trim trailing separators that match `strategy.path_separators`.
/// We avoid `trim_end_matches('/')` because Windows candidates may
/// terminate with `\` and we need to drop the right one.
pub(super) fn trim_trailing_separators<'a>(s: &'a str, strategy: &dyn PathStrategy) -> &'a str {
    let separators = strategy.path_separators();
    s.trim_end_matches(|c| separators.contains(&c))
}

/// Strip the resolved scope prefix from a candidate so the candidate
/// list can render the relative child name only — full paths stay
/// available in `RenderSnapshot::candidates` for the preview line.
pub(super) fn compute_child_label(candidate: &str, scope: &str) -> String {
    if scope.is_empty() {
        return candidate.to_owned();
    }
    candidate
        .strip_prefix(scope)
        .map(str::to_owned)
        .unwrap_or_else(|| candidate.to_owned())
}

/// Compute the preview text shown on the commit-preview line.
///
/// * The preview ALWAYS reflects what `Enter` will submit, which is
///   the input buffer (shell-like contract). Highlighted candidates
///   are visual-only; only `Tab` writes a candidate into the buffer.
/// * An absolute or `~/`-rooted non-bare buffer is the
///   would-be-committed path.
/// * A bare single-segment buffer with a known home renders as the
///   implicit `~/<buffer>` form, so the preview stays consistent
///   with the visible browse scope.
/// * Bare scopes (`~`, `~/`, `/`, `C:\`, `\\server\share\`, …) and
///   other relative buffers return `None`; the caller renders a
///   localized warning instead.
pub(super) fn compute_preview(
    buffer: &str,
    _candidates: &[String],
    _highlighted: Option<usize>,
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
) -> Option<String> {
    let trimmed = buffer.trim();
    if trimmed.is_empty() {
        return None;
    }
    if buffer_is_bare_scope(trimmed, strategy) {
        return None;
    }
    if trimmed.starts_with("~/") || strategy.is_absolute(trimmed) {
        return Some(trimmed.to_owned());
    }
    if buffer_is_scoped_filter(buffer, home, strategy) {
        return Some(format!("~/{trimmed}"));
    }
    None
}

/// `true` when the buffer is exactly one of the bare scope tokens
/// understood by the active strategy (`~`, `~/`, `/`, `C:\`, `C:/`,
/// `\\server\share\`, …). These are intentionally NOT promoted to a
/// commit-preview because they would either be rejected as too
/// broad or are not even valid trusted-root inputs under the
/// configured platform.
pub(super) fn buffer_is_bare_scope(buffer: &str, strategy: &dyn PathStrategy) -> bool {
    strategy.is_bare_scope(buffer)
}

/// `true` when the buffer is non-empty but neither absolute under
/// the active strategy nor `~/`-rooted. Trusted-root validation
/// rejects relative paths, so the field surfaces a warning before
/// the user submits.
pub(super) fn buffer_is_relative(buffer: &str, strategy: &dyn PathStrategy) -> bool {
    let t = buffer.trim();
    !t.is_empty() && !strategy.is_home_alias(t) && !strategy.is_absolute(t)
}

/// `true` when the buffer is a *scoped filter* under the visible
/// home browse scope: it is non-empty, contains no separator the
/// active strategy understands, is not the `~` token, and a home
/// directory is available to anchor the search. This case must be
/// presented as filtering inside `~/`, not as an invalid relative
/// path.
pub(super) fn buffer_is_scoped_filter(
    buffer: &str,
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
) -> bool {
    if home.is_none() {
        return false;
    }
    let t = buffer.trim();
    !t.is_empty()
        && !t.starts_with('~')
        && !t.contains(strategy.path_separators())
        && !strategy.is_absolute(t)
}

/// Buffer is a `Vec<char>`, but the active strategy's separators
/// are reported as `char`s too: convert the trailing element on
/// the fly and compare. Returns `false` for empty buffers.
pub(super) fn buffer_ends_with_separator(buffer: &[char], strategy: &dyn PathStrategy) -> bool {
    let Some(&last) = buffer.last() else {
        return false;
    };
    strategy.path_separators().contains(&last)
}

/// Replace the literal `{path}` token in `template` with `path`.
///
/// We deliberately use a typed token rather than positional `{}`:
/// translators reading `path_input.scope_label` should see "{path}"
/// in the source string so the localized translation stays
/// readable. Falls through unchanged when the token is absent.
pub(super) fn format_label(template: &str, path: &str) -> String {
    template.replacen("{path}", path, 1)
}

/// Substitute the typed bare-buffer into the `path_input.scope_no_match_warning`
/// template. The catalog uses a single `{0}` placeholder so we render with
/// `replacen` instead of synthesising an implicit `~/<buffer>` form, which
/// would contradict the localized warning by suggesting a path that does
/// not exist.
pub(super) fn format_scope_no_match(template: &str, bare_buffer: &str) -> String {
    template.replacen("{0}", bare_buffer, 1)
}

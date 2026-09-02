//! Pure Tab-completion logic for interactive directory prompts.
//!
//! The CLI feeds the user's current input string and a [`PathCompletionFsPort`]
//! into [`compute_path_completions`]. The result tells the interactive
//! prompt how to react to the Tab key:
//!
//! - [`PathCompletionResult::Single`] — replace the input with the
//!   single matching directory (with a trailing `/`) so the user can
//!   keep typing.
//! - [`PathCompletionResult::Many`] — show a candidate list and let
//!   the user pick.
//! - [`PathCompletionResult::None`] — leave the input unchanged and
//!   render a localized "no matches" hint.
//!
//! Behaviour rules pinned by the doc:
//!
//! - Only directories are surfaced as candidates — files are filtered.
//! - Hidden entries (those starting with `.`) are surfaced only when
//!   the partial filename also starts with `.`.
//! - Empty input returns the **direct child directories** of `home`,
//!   sorted alphabetically. `~/` itself is intentionally omitted as a
//!   selectable candidate: a bare home
//!   directory is rejected by trusted-root validation as too broad,
//!   so offering it as a first-run candidate is misleading.
//! - `~` and `~/` resolve to `home`. The returned `completion` strings
//!   preserve the user's authored form: typing `~/dev` returns
//!   completions that still start with `~/`, while typing absolute
//!   `/Users/me/dev` returns absolute candidates.
//! - Candidates are sorted by match quality (prefix > substring >
//!   subsequence, all case-insensitive) with stable alphabetical
//!   tie-breaking.
//! - Bare text without `/` or `~/` is treated as a SCOPED FILTER
//!   under the visible home scope when `home` is provided. Typing
//!   `a` while the wizard browses `~/` searches `~/a*` directly
//!   instead of returning [`PathCompletionResult::None`]. Multi-segment
//!   relative paths (e.g. `relative/sub`) are still rejected — they
//!   would be ambiguous without an authored absolute root.
//! - The use case is also the live-search source for the trusted-root
//!   prompt — every keystroke calls back into it with the current
//!   buffer, so the same scoping rules apply to typed-as-you-search.

use camino::{Utf8Path, Utf8PathBuf};

use crate::ports::PathCompletionFsPort;
use crate::usecases::path_match::{MatchKind, score_match};
use crate::usecases::path_strategy::PathStrategy;

/// Outcome of running [`compute_path_completions`] against a partial path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathCompletionResult {
    /// Exactly one directory matches the partial input. The CLI should
    /// replace the input with `completion`. The string already includes
    /// a trailing `/` so the user can keep typing without re-pressing Tab.
    Single { completion: String },
    /// Multiple directories match the partial input. The CLI shows them
    /// as a candidate list and lets the user pick.
    Many { candidates: Vec<String> },
    /// No directory matched the partial input. The CLI shows a localized
    /// "no candidates" hint and leaves the input untouched.
    None,
}

/// Compute Tab-completion candidates for the user's partial path input.
///
/// `home` is used to expand `~` / `~/foo` when present. When `home` is
/// `None`, tilde-prefixed input is rejected as if no candidates exist.
///
/// `strategy` injects the platform-specific path syntax. macOS and
/// Linux share [`crate::usecases::path_strategy::UnixPathStrategy`];
/// Windows uses [`crate::usecases::path_strategy::WindowsPathStrategy`].
/// The default selector is
/// [`crate::usecases::path_strategy::current_target_strategy`], used
/// by production CLI paths; tests inject a concrete strategy directly.
pub fn compute_path_completions(
    input: &str,
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
    fs: &dyn PathCompletionFsPort,
) -> PathCompletionResult {
    if input.is_empty() {
        return empty_input_completions(home, fs);
    }

    let split = match split_input_for_completion(input, home, strategy) {
        Some(parts) => parts,
        None => {
            tracing::debug!(
                input = %input,
                strategy = ?strategy.kind(),
                "path_completion: input is not absolute or tilde-rooted; no candidates"
            );
            return PathCompletionResult::None;
        }
    };

    let entries = match fs.list_children(&split.parent_to_list) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::debug!(
                input = %input,
                parent = %split.parent_to_list,
                error = %err,
                "path_completion: parent directory unreadable; no candidates"
            );
            return PathCompletionResult::None;
        }
    };

    let show_hidden = split.partial_name.starts_with('.');
    // Reuse the user's authored separator style when constructing
    // candidate strings: typing `~/dev/p` keeps producing `~/dev/...`,
    // typing `C:\Users\` keeps producing `C:\Users\...`. Falling back
    // to the strategy's primary separator handles the empty-prefix
    // case (e.g. typing `C:\` gives `\` on Windows).
    let candidate_separator = split.candidate_separator;
    let mut scored: Vec<(MatchKind, String, String)> = entries
        .into_iter()
        .filter(|entry| entry.is_directory)
        .filter(|entry| show_hidden || !entry.name.starts_with('.'))
        .filter_map(|entry| {
            score_match(&split.partial_name, &entry.name).map(|m| {
                let path = format!(
                    "{}{}{}",
                    split.user_visible_prefix, entry.name, candidate_separator
                );
                (m.kind, entry.name.to_ascii_lowercase(), path)
            })
        })
        .collect();
    // Sort by match tier (better first), then alphabetically by
    // lowercase candidate name for stable, locale-free output. Path
    // completion requires deterministic ordering with prefix priority;
    // this is exactly that contract, applied to the existing prefix branch.
    scored.sort_by(|a, b| match a.0.cmp(&b.0) {
        std::cmp::Ordering::Equal => a.1.cmp(&b.1),
        ord => ord,
    });
    let mut matched: Vec<String> = scored.into_iter().map(|(_, _, path)| path).collect();
    matched.dedup();

    tracing::debug!(
        input = %input,
        parent = %split.parent_to_list,
        partial = %split.partial_name,
        candidate_count = matched.len(),
        show_hidden,
        "path_completion: produced candidates"
    );

    match matched.len() {
        0 => PathCompletionResult::None,
        1 => PathCompletionResult::Single {
            completion: matched.into_iter().next().unwrap_or_default(),
        },
        _ => PathCompletionResult::Many {
            candidates: matched,
        },
    }
}

fn empty_input_completions(
    home: Option<&Utf8Path>,
    fs: &dyn PathCompletionFsPort,
) -> PathCompletionResult {
    // Empty input is first-run setup's most important case. Live search
    // must show the user's home-directory child directories, but NOT `~/`
    // itself — a bare home directory is later rejected by trusted-root
    // validation as too broad, so offering it as a first-run candidate
    // is misleading.
    let Some(home) = home else {
        tracing::debug!("path_completion: empty input but no home; returning no candidates");
        return PathCompletionResult::None;
    };

    let mut candidates: Vec<String> = match fs.list_children(home) {
        Ok(entries) => entries
            .into_iter()
            // Trusted roots are filesystem locations under which we
            // create `.workhive/`, so files are never selectable.
            .filter(|entry| entry.is_directory)
            // Hidden directories are intentionally omitted on empty
            // input; users who want `~/.foo` can type the leading `.`.
            .filter(|entry| !entry.name.starts_with('.'))
            .map(|entry| format!("~/{}/", entry.name))
            .collect(),
        Err(err) => {
            tracing::debug!(
                home = %home,
                error = %err,
                "path_completion: failed to list home directory; returning no candidates"
            );
            return PathCompletionResult::None;
        }
    };
    candidates.sort();
    candidates.dedup();

    tracing::debug!(
        home = %home,
        candidate_count = candidates.len(),
        "path_completion: produced empty-input candidates from home"
    );

    match candidates.len() {
        0 => PathCompletionResult::None,
        1 => PathCompletionResult::Single {
            completion: candidates.into_iter().next().unwrap_or_default(),
        },
        _ => PathCompletionResult::Many { candidates },
    }
}

/// Compute the longest common prefix shared by every candidate in
/// `candidates`. Returns an empty string when `candidates` is empty
/// or when no character is common across all entries.
///
/// Used by the Tab handler to perform shell-like prefix completion:
/// when the user types `~/d` against `[~/dev/, ~/docs/]`, the common
/// prefix `~/d` is unchanged, but typing `~/de` still has only one
/// candidate so completion routes through `Single`. The interesting
/// case is when the user types a prefix shorter than the maximal
/// shared prefix — Tab should extend the buffer to that prefix
/// before forcing the user to disambiguate by arrow-key navigation.
pub fn longest_common_prefix(candidates: &[String]) -> String {
    let mut iter = candidates.iter();
    let first = match iter.next() {
        Some(s) => s.as_str(),
        None => return String::new(),
    };
    let mut prefix_len_chars: usize = first.chars().count();
    for candidate in iter {
        let common: usize = first
            .chars()
            .zip(candidate.chars())
            .take_while(|(a, b)| a == b)
            .count();
        if common < prefix_len_chars {
            prefix_len_chars = common;
        }
        if prefix_len_chars == 0 {
            return String::new();
        }
    }
    first.chars().take(prefix_len_chars).collect()
}

/// Internal split of the user's input into "directory to scan" and
/// "filename prefix to match". Returns `None` for input that should
/// produce no completions (relative paths, or `~` without a home).
struct CompletionSplit {
    /// Filesystem path the [`PathCompletionFsPort`] should read.
    /// Always absolute by the time we hand it to the port.
    parent_to_list: Utf8PathBuf,
    /// Filename prefix that the completion must `starts_with`.
    partial_name: String,
    /// Form of the prefix that should be preserved in the returned
    /// completion strings, so a user typing `~/dev/` keeps seeing
    /// candidates that begin with `~/dev/`.
    user_visible_prefix: String,
    /// Trailing separator appended to each candidate entry. Mirrors
    /// the user's authored separator style under the active strategy
    /// (`/` on Unix; `\` or `/` on Windows depending on what the user
    /// typed) so completion never silently swaps separators.
    candidate_separator: char,
}

fn split_input_for_completion(
    input: &str,
    home: Option<&Utf8Path>,
    strategy: &dyn PathStrategy,
) -> Option<CompletionSplit> {
    if input.is_empty() {
        return None;
    }

    if input == "~" {
        // Treat `~` like `~/` for completion purposes.
        return split_input_for_completion("~/", home, strategy);
    }

    let separators = strategy.path_separators();

    // Bare text without any path separator and without a `~` prefix
    // is interpreted as a scoped filter under the visible home scope.
    // The wizard's default browse scope is `~/`; treating `a` as
    // `~/a` matches the visible scope and lets the user pick a
    // candidate. Multi-segment relative inputs (`relative/sub`,
    // `foo\bar`) are still rejected — disambiguating them would
    // require guessing the user's intent. Inputs that look absolute
    // under the active strategy (e.g. `C:\Users\me` on Windows) take
    // the absolute branch below.
    if !strategy.is_absolute(input) && !strategy.is_home_alias(input) && !input.contains(separators)
    {
        let home = home?;
        return Some(CompletionSplit {
            parent_to_list: home.to_owned(),
            partial_name: input.to_owned(),
            user_visible_prefix: "~/".to_owned(),
            candidate_separator: '/',
        });
    }

    // Resolve the absolute filesystem prefix that backs `input`, while
    // keeping the user's authored form (`~/...`, `/...`, `C:\...`,
    // `C:/...`, `\\server\share\...`) for display.
    let absolute_prefix: Utf8PathBuf = if let Some(rest) = input.strip_prefix("~/") {
        let home = home?;
        home.join(rest)
    } else if strategy.is_absolute(input) {
        Utf8PathBuf::from(input)
    } else {
        // Anything else (multi-segment relative path, environment
        // variable, etc.) is intentionally NOT supported — without
        // an active scope it would be ambiguous, and the trusted-root
        // commit gate still enforces absolute or `~/...`.
        return None;
    };

    // Pick the separator to append to candidate names. We prefer the
    // last separator the user typed so `C:\Users\` keeps producing
    // `C:\Users\foo\` and `C:/Users/` keeps producing `C:/Users/foo/`.
    // Falling back to `/` matches the prior Unix-only behaviour.
    let candidate_separator = input
        .chars()
        .rev()
        .find(|c| strategy.is_path_separator(*c))
        .unwrap_or('/');

    // If the user already ended their input with a separator, the
    // entire string is a directory and the completion is "list
    // everything inside". Strip any trailing separators before
    // handing the path to the FS port: on Unix
    // `Utf8Path::eq` already ignores trailing slashes, but Windows
    // backslashes survive on a Unix host (CI / dev) and would miss
    // the FS port's lookup keys. We keep at least the strategy's
    // root prefix so a bare `C:\` or `\\server\share\` still
    // produces a usable parent path.
    if strategy.ends_with_separator(input) {
        let trimmed = absolute_prefix
            .as_str()
            .trim_end_matches(|c: char| strategy.is_path_separator(c));
        let parent_to_list = if trimmed.is_empty() {
            absolute_prefix.clone()
        } else {
            Utf8PathBuf::from(trimmed)
        };
        return Some(CompletionSplit {
            parent_to_list,
            partial_name: String::new(),
            user_visible_prefix: input.to_owned(),
            candidate_separator,
        });
    }

    // Split on the last separator of the input string itself — NOT
    // via `Utf8Path::file_name()`, which strips trailing `.` / `..`
    // components and would silently drop the user's intent to
    // complete hidden entries (e.g. typing `/Users/me/.`).
    let last_sep = input
        .rfind(separators)
        .expect("input was confirmed to contain a separator (absolute, ~/-rooted, or scoped)");
    let partial_name = input[last_sep + 1..].to_owned();
    let visible_dir = input[..=last_sep].to_owned();

    // Resolve the directory to scan from the absolute prefix, dropping
    // the final segment we just put into `partial_name`. We re-derive
    // via string slicing on the *expanded* absolute path so behaviour
    // is identical for `~/dev/.` and `/Users/me/dev/.` — and so we
    // never have to trust `Utf8Path::parent()` to keep `.` as a child
    // name. We use the strategy's separators so Windows backslashes
    // are recognised when the wizard runs under `WindowsPathStrategy`.
    let abs_str = absolute_prefix.as_str();
    let parent_to_list = match abs_str.rfind(separators) {
        Some(0) => Utf8PathBuf::from(&abs_str[..1]),
        Some(idx) => Utf8PathBuf::from(&abs_str[..idx]),
        None => {
            // Absolute path with no separator (e.g. `C:` after a
            // hypothetical drive root) — fall back to the strategy's
            // root prefix.
            match strategy.split_absolute_root(abs_str) {
                Some((root, _)) => Utf8PathBuf::from(root),
                None => Utf8PathBuf::from(abs_str),
            }
        }
    };

    Some(CompletionSplit {
        parent_to_list,
        partial_name,
        user_visible_prefix: visible_dir,
        candidate_separator,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::PathCompletionEntry;
    use crate::usecases::path_strategy::{UnixPathStrategy, WindowsPathStrategy};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Default Unix strategy used by the bulk of these tests.
    fn unix() -> UnixPathStrategy {
        UnixPathStrategy
    }

    /// Windows strategy used by the production-path tests below; the
    /// review explicitly required Windows drive-rooted and UNC inputs
    /// to flow through the real completion logic, not just the
    /// strategy unit tests.
    fn windows() -> WindowsPathStrategy {
        WindowsPathStrategy
    }

    /// In-memory fake of [`PathCompletionFsPort`] for unit tests.
    /// Keeps things deterministic by storing entries per parent path.
    struct FakeFs {
        // parent path -> child entries (already sorted in insertion order)
        children: RefCell<HashMap<Utf8PathBuf, Vec<PathCompletionEntry>>>,
        // parent paths that should error out instead of returning entries.
        broken: RefCell<Vec<Utf8PathBuf>>,
    }

    impl FakeFs {
        fn new() -> Self {
            Self {
                children: RefCell::new(HashMap::new()),
                broken: RefCell::new(Vec::new()),
            }
        }

        fn dirs(mut self, parent: &str, names: &[&str]) -> Self {
            self.children.get_mut().insert(
                Utf8PathBuf::from(parent),
                names
                    .iter()
                    .map(|n| PathCompletionEntry {
                        name: (*n).to_owned(),
                        is_directory: true,
                    })
                    .collect(),
            );
            self
        }

        fn mixed(
            mut self,
            parent: &str,
            entries: &[(&str, bool)], // (name, is_directory)
        ) -> Self {
            self.children.get_mut().insert(
                Utf8PathBuf::from(parent),
                entries
                    .iter()
                    .map(|(name, is_dir)| PathCompletionEntry {
                        name: (*name).to_owned(),
                        is_directory: *is_dir,
                    })
                    .collect(),
            );
            self
        }

        fn break_path(mut self, parent: &str) -> Self {
            self.broken.get_mut().push(Utf8PathBuf::from(parent));
            self
        }
    }

    impl PathCompletionFsPort for FakeFs {
        fn list_children(
            &self,
            parent: &Utf8Path,
        ) -> Result<Vec<PathCompletionEntry>, std::io::Error> {
            if self.broken.borrow().iter().any(|p| p == parent) {
                return Err(std::io::Error::other("simulated read failure"));
            }
            Ok(self
                .children
                .borrow()
                .get(parent)
                .cloned()
                .unwrap_or_default())
        }

        fn classify_path(&self, _path: &Utf8Path) -> crate::ports::PathKind {
            // Not exercised by the completion tests in this module.
            crate::ports::PathKind::Missing
        }
    }

    fn home() -> Utf8PathBuf {
        Utf8PathBuf::from("/Users/me")
    }

    /// Tab on `/Users/me/dev/proj` with a single matching directory must
    /// surface `Single` so the prompt can fill the input with the full
    /// completion plus a trailing `/`.
    #[test]
    fn single_directory_match_returns_single_with_trailing_slash() {
        let fs = FakeFs::new().dirs("/Users/me/dev", &["projects"]);
        let result =
            compute_path_completions("/Users/me/dev/proj", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "/Users/me/dev/projects/".to_owned()
            }
        );
    }

    /// Multiple directory matches must surface `Many` with deterministic
    /// (lexicographic) ordering — we never want flaky candidate lists.
    #[test]
    fn many_matches_are_sorted_lexicographically() {
        let fs = FakeFs::new().dirs("/Users/me/dev", &["zeta", "alpha", "beta"]);
        let result =
            compute_path_completions("/Users/me/dev/", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "/Users/me/dev/alpha/".to_owned(),
                    "/Users/me/dev/beta/".to_owned(),
                    "/Users/me/dev/zeta/".to_owned(),
                ]
            }
        );
    }

    /// Files must NOT appear as completions for trusted-root input.
    /// Trusted roots are filesystem locations under which Workhive
    /// will create `.workhive/`, so suggesting a file would be
    /// actively misleading.
    #[test]
    fn only_directories_are_offered_as_completions() {
        let fs = FakeFs::new().mixed(
            "/srv",
            &[
                ("notes.md", false),
                ("scripts.tar.gz", false),
                ("projects", true),
            ],
        );
        let result = compute_path_completions("/srv/", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "/srv/projects/".to_owned()
            }
        );
    }

    /// `~/...` must resolve under the user's home directory but the
    /// returned completion MUST preserve the `~/` prefix the user typed,
    /// so the saved trusted-root can keep its portable form.
    #[test]
    fn tilde_input_preserves_authored_form_in_completion() {
        // Use neighbours that share zero characters with the partial
        // so the test exercises authored-form preservation, not the
        // fuzzy matcher tie-breaking already covered elsewhere.
        let fs = FakeFs::new().dirs("/Users/me/dev", &["alpha", "zulu"]);
        let result = compute_path_completions("~/dev/a", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "~/dev/alpha/".to_owned()
            }
        );
    }

    /// Empty input lists the user's home-directory child directories
    /// in alphabetical order, but NEVER includes `~/` itself — a bare
    /// home directory is rejected by trusted-root validation as too
    /// broad and would be a misleading first-run candidate. Files and
    /// hidden directories are filtered out.
    #[test]
    fn empty_input_lists_home_child_directories_without_home_itself() {
        let fs = FakeFs::new().mixed(
            "/Users/me",
            &[
                ("dev", true),
                ("development", true),
                ("Music", true),
                ("projects", true),
                (".cache", true),
                ("plan.md", false),
            ],
        );
        let result = compute_path_completions("", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                // `~/` itself MUST NOT appear. Files and hidden dirs
                // are filtered. Sort is alphabetic — `Music` (capital
                // `M`, ASCII 0x4D) sorts before `dev` (lowercase `d`,
                // ASCII 0x64).
                candidates: vec![
                    "~/Music/".to_owned(),
                    "~/dev/".to_owned(),
                    "~/development/".to_owned(),
                    "~/projects/".to_owned(),
                ]
            }
        );
    }

    /// Single home child directory should still degrade to `Single`
    /// rather than `Many` so the prompt UI can fill the input on
    /// first keystroke.
    #[test]
    fn empty_input_with_single_home_child_returns_single() {
        let fs = FakeFs::new().mixed(
            "/Users/me",
            &[("Music", true), (".cache", true), ("plan.md", false)],
        );
        let result = compute_path_completions("", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "~/Music/".to_owned()
            }
        );
    }

    /// Empty input with no usable home child directories must NOT
    /// fall back to `~/` itself.
    #[test]
    fn empty_input_with_no_visible_home_dirs_returns_none() {
        let fs = FakeFs::new().mixed("/Users/me", &[(".cache", true), ("plan.md", false)]);
        let result = compute_path_completions("", Some(home().as_path()), &unix(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    /// Live-search reproduces the doc's example: typing `~/de` while
    /// the home directory contains `dev/` and `development/` returns
    /// both as candidates without requiring a Tab press.
    #[test]
    fn live_search_for_tilde_de_returns_matching_home_children() {
        let fs = FakeFs::new().dirs("/Users/me", &["dev", "development", "Music", "projects"]);
        let result = compute_path_completions("~/de", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec!["~/dev/".to_owned(), "~/development/".to_owned(),]
            }
        );
    }

    /// Live-search reproduces the second doc example: typing
    /// `~/development/wo` scans `~/development/` directly and surfaces
    /// matching child directories such as `~/development/workhive/`.
    #[test]
    fn live_search_for_nested_prefix_scopes_to_immediate_parent() {
        let fs = FakeFs::new().dirs(
            "/Users/me/development",
            &["workhive", "worktree-experiments", "other"],
        );
        let result =
            compute_path_completions("~/development/wo", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "~/development/workhive/".to_owned(),
                    "~/development/worktree-experiments/".to_owned(),
                ]
            }
        );
    }

    /// Hidden entries are returned only when the user is explicitly
    /// typing a leading `.`. Otherwise we hide them so trusted-root
    /// completion does not start surfacing `.git`, `.workhive`, etc.
    #[test]
    fn hidden_entries_are_surfaced_only_when_prefix_starts_with_dot() {
        let fs = FakeFs::new().dirs("/Users/me", &[".cache", ".config", "dev"]);
        let result = compute_path_completions("/Users/me/.", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "/Users/me/.cache/".to_owned(),
                    "/Users/me/.config/".to_owned(),
                ]
            }
        );
    }

    /// Multi-segment relative paths without `~/` or `/` are NOT
    /// candidates — they are ambiguous without an authored absolute
    /// root, and the trusted-root commit gate still rejects them.
    /// Returning `None` keeps the prompt honest.
    #[test]
    fn multi_segment_relative_input_returns_none_so_no_false_hits() {
        let fs = FakeFs::new().dirs("relative/sub", &["unexpected"]);
        let result = compute_path_completions("relative/sub", Some(home().as_path()), &unix(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    /// Bare single-segment input under an active home scope MUST be
    /// treated as a filter against home children.
    /// Typing `a` while the wizard browses `~/` searches `~/a*` and
    /// must surface matching home children — NOT `PathCompletionResult::None`.
    #[test]
    fn bare_input_under_home_filters_home_children_as_scoped_search() {
        let fs = FakeFs::new().dirs(
            "/Users/me",
            &["Applications", "Documents", "Music", "apex", "data"],
        );
        let result = compute_path_completions("a", Some(home().as_path()), &unix(), &fs);
        // Prefix-tier matches (case-insensitive) come first:
        //   "Applications" (case-insensitive prefix match for "a")
        //   "apex"         (case-sensitive prefix match for "a")
        // Substring-tier matches come second:
        //   "data"         (contains "a" but does not start with it)
        // "Music" and "Documents" do not match at all.
        // Within the substring tier "data" wins alphabetically over
        // anything else; within the prefix tier the case-sensitive
        // hit ("apex") outranks the case-insensitive hit ("Applications").
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "~/apex/".to_owned(),
                    "~/Applications/".to_owned(),
                    "~/data/".to_owned(),
                ]
            }
        );
    }

    /// Bare-filter mode MUST be case-insensitive: typing uppercase
    /// `DOC` under the home scope still matches `Documents`.
    #[test]
    fn bare_input_under_home_is_case_insensitive() {
        let fs = FakeFs::new().dirs("/Users/me", &["Documents", "Downloads", "Music"]);
        let result = compute_path_completions("DOC", Some(home().as_path()), &unix(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "~/Documents/".to_owned()
            }
        );
    }

    /// Bare-filter mode MUST rank prefix matches ahead of weaker
    /// substring matches.
    #[test]
    fn bare_input_under_home_ranks_prefix_above_substring() {
        let fs = FakeFs::new().dirs(
            "/Users/me",
            &["docking", "indocs", "Documents", "no-doc-here"],
        );
        let result = compute_path_completions("doc", Some(home().as_path()), &unix(), &fs);
        // Prefix matches: "docking" (case-sensitive prefix), "Documents"
        // (case-insensitive prefix). Substring: "indocs", "no-doc-here".
        match result {
            PathCompletionResult::Many { candidates } => {
                assert_eq!(
                    candidates,
                    vec![
                        "~/docking/".to_owned(),
                        "~/Documents/".to_owned(),
                        "~/indocs/".to_owned(),
                        "~/no-doc-here/".to_owned(),
                    ]
                );
            }
            other => panic!("expected Many, got {other:?}"),
        }
    }

    /// Existing `~/de` live-search must keep working under the new
    /// fuzzy matcher: prefix-tier matches alphabetically tie-break
    /// to `~/dev/` then `~/development/`.
    #[test]
    fn live_search_for_tilde_de_still_uses_prefix_priority() {
        let fs = FakeFs::new().dirs("/Users/me", &["Music", "dev", "development", "indev"]);
        let result = compute_path_completions("~/de", Some(home().as_path()), &unix(), &fs);
        // Prefix matches sorted alphabetically by lowercase name.
        // Substring match "indev" follows.
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "~/dev/".to_owned(),
                    "~/development/".to_owned(),
                    "~/indev/".to_owned(),
                ]
            }
        );
    }

    /// Bare input without a usable home produces no candidates — the
    /// scoped-filter mode is only valid when a browse scope exists.
    #[test]
    fn bare_input_without_home_returns_none() {
        let fs = FakeFs::new().dirs("/Users/me", &["Applications"]);
        let result = compute_path_completions("a", None, &unix(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    /// Tilde input with no `home` available cannot be expanded, so the
    /// prompt MUST fall through to `None` rather than guess.
    #[test]
    fn tilde_input_without_home_returns_none() {
        let fs = FakeFs::new().dirs("/", &["root_dir"]);
        let result = compute_path_completions("~/dev", None, &unix(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    /// Filesystem read failures degrade gracefully into "no candidates"
    /// so the wizard does not crash mid-prompt — UI surfaces a localized
    /// hint instead.
    #[test]
    fn unreadable_directory_returns_none_so_prompt_can_show_localized_hint() {
        let fs = FakeFs::new().break_path("/locked");
        let result = compute_path_completions("/locked/", Some(home().as_path()), &unix(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    // ── Production-path Windows coverage ──
    //
    // These tests run the real `compute_path_completions` flow under
    // `WindowsPathStrategy` so the strategy is not a test-only patch.
    // They cover drive-rooted paths with both separators, UNC inputs,
    // and the bare scoped-filter shortcut.

    /// Trailing-separator drive-rooted input must list every child
    /// of that directory with the user's authored backslash style.
    #[test]
    fn windows_drive_rooted_completion_lists_children_with_backslash_style() {
        let fs = FakeFs::new().dirs(r"C:\Users\me", &["Documents", "Downloads", "Music"]);
        let result =
            compute_path_completions(r"C:\Users\me\", Some(home().as_path()), &windows(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    r"C:\Users\me\Documents\".to_owned(),
                    r"C:\Users\me\Downloads\".to_owned(),
                    r"C:\Users\me\Music\".to_owned(),
                ]
            }
        );
    }

    /// Drive-rooted input typed with forward slashes must keep using
    /// forward slashes for completed candidates so the returned
    /// strings match what the user typed.
    #[test]
    fn windows_drive_rooted_completion_preserves_forward_slash_authoring() {
        let fs = FakeFs::new().dirs("C:/Users/me", &["Documents", "Downloads"]);
        let result =
            compute_path_completions("C:/Users/me/D", Some(home().as_path()), &windows(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    "C:/Users/me/Documents/".to_owned(),
                    "C:/Users/me/Downloads/".to_owned(),
                ]
            }
        );
    }

    /// Partial drive-rooted prefix must filter siblings under the
    /// resolved parent, not list everything.
    #[test]
    fn windows_drive_rooted_partial_prefix_filters_siblings() {
        let fs = FakeFs::new().dirs(r"C:\Users\me", &["dev", "development", "Music"]);
        let result =
            compute_path_completions(r"C:\Users\me\de", Some(home().as_path()), &windows(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    r"C:\Users\me\dev\".to_owned(),
                    r"C:\Users\me\development\".to_owned(),
                ]
            }
        );
    }

    /// UNC inputs (`\\server\share\...`) must run through the same
    /// completion flow as drive-rooted ones.
    #[test]
    fn windows_unc_completion_lists_children_under_share() {
        let fs = FakeFs::new().dirs(r"\\server\share\projects", &["alpha", "beta"]);
        let result = compute_path_completions(
            r"\\server\share\projects\",
            Some(home().as_path()),
            &windows(),
            &fs,
        );
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    r"\\server\share\projects\alpha\".to_owned(),
                    r"\\server\share\projects\beta\".to_owned(),
                ]
            }
        );
    }

    /// UNC partial prefix must filter siblings under the share root.
    #[test]
    fn windows_unc_partial_prefix_filters_siblings_under_share() {
        let fs = FakeFs::new().dirs(r"\\server\share", &["projects", "promo"]);
        let result = compute_path_completions(
            r"\\server\share\pr",
            Some(home().as_path()),
            &windows(),
            &fs,
        );
        assert_eq!(
            result,
            PathCompletionResult::Many {
                candidates: vec![
                    r"\\server\share\projects\".to_owned(),
                    r"\\server\share\promo\".to_owned(),
                ]
            }
        );
    }

    /// Posix-style absolute input MUST be rejected by the Windows
    /// strategy: the requirement says only drive-rooted/UNC forms
    /// round-trip, and we never want to silently treat `/Users/me`
    /// as a Windows trusted-root candidate.
    #[test]
    fn windows_strategy_rejects_posix_absolute_input() {
        let fs = FakeFs::new().dirs("/Users/me", &["dev"]);
        let result =
            compute_path_completions("/Users/me/dev", Some(home().as_path()), &windows(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }

    /// Bare single-segment input still resolves under the home scope
    /// when the wizard runs under `WindowsPathStrategy`.
    #[test]
    fn windows_bare_input_under_home_still_filters_home_children() {
        let fs = FakeFs::new().dirs("/Users/me", &["Applications", "Documents", "Music"]);
        let result = compute_path_completions("D", Some(home().as_path()), &windows(), &fs);
        assert_eq!(
            result,
            PathCompletionResult::Single {
                completion: "~/Documents/".to_owned()
            }
        );
    }

    /// Multi-segment relative input under `WindowsPathStrategy` must
    /// still be rejected as ambiguous, regardless of the separator
    /// style.
    #[test]
    fn windows_multi_segment_relative_input_returns_none() {
        let fs = FakeFs::new().dirs(r"relative\sub", &["unexpected"]);
        let result =
            compute_path_completions(r"relative\sub", Some(home().as_path()), &windows(), &fs);
        assert_eq!(result, PathCompletionResult::None);
    }
}

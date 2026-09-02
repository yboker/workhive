//! Narrow path-syntax strategy abstraction for the trusted-root picker.
//!
//! ## Why this exists
//!
//! Workhive ships per-platform binaries and the trusted-root picker
//! must understand path syntax for *both* the platform it is running
//! on and the platform it might be invoked under in tests:
//!
//!   * **macOS / Linux** — share the Unix path syntax model.
//!     Differences are only in the *value* of the home directory
//!     (`/Users/...` vs `/home/...`), which the wizard injects as
//!     `home: Option<&Utf8Path>`. They MUST NOT be split into two
//!     separate strategies — the requirement explicitly forbids that.
//!   * **Windows** — drive-rooted paths (`C:\Users\...`,
//!     `C:/Users/...`), UNC paths (`\\server\share\...`), and
//!     POSIX-on-Windows shells. `~/...` is supported as a portable
//!     alias when a home is provided, but Workhive never persists
//!     `~/...` as a Windows trusted root.
//!
//! ## Boundary
//!
//! This module is intentionally narrow:
//!
//!   * absolute-path detection;
//!   * separator normalization for display;
//!   * platform-aware "current target" selector.
//!
//! It does NOT replace the trusted-root validation pipeline, the
//! filesystem port, or the path-completion split logic. The wizard,
//! filesystem port, and `path_completion` continue to own those
//! responsibilities; they merely consult this strategy when they need
//! to ask "is this string an absolute path under the current target's
//! syntax?".

use camino::Utf8Path;

/// Identity of the path-syntax model. Useful for diagnostic logs and
/// tests that want to assert "we ran under the Windows model".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStrategyKind {
    Unix,
    Windows,
}

/// Narrow strategy trait for path syntax.
///
/// Implementations MUST be pure: no I/O, no caching, no `std::env`
/// reads — everything they need is supplied by the caller. This is
/// what lets tests inject any home path and exercise the Windows
/// strategy on a non-Windows host.
pub trait PathStrategy {
    /// Identity tag for diagnostics and test assertions.
    fn kind(&self) -> PathStrategyKind;

    /// Returns `true` when `input` is an absolute path under this
    /// strategy's syntax. Both `~/...` and the Unix-on-Windows form
    /// `/foo` are intentionally NOT classified as absolute by the
    /// Windows strategy: the requirement says "explicit Windows
    /// absolute forms" must round-trip through `WindowsPathStrategy`.
    fn is_absolute(&self, input: &str) -> bool;

    /// Returns `true` when `input` starts with the home alias `~/`
    /// or is exactly `~`. Both Unix and Windows strategies accept
    /// the home alias when the wizard injects a home value; the
    /// alias is the wizard's portable display form for "the user's
    /// home directory" regardless of platform.
    fn is_home_alias(&self, input: &str) -> bool {
        input == "~" || input.starts_with("~/")
    }

    /// Path separators understood by this strategy.
    ///
    /// * Unix returns `&['/']`.
    /// * Windows returns `&['/', '\\']` because the OS, the standard
    ///   library, and most user input accept both forms.
    ///
    /// Production code uses this list every time it splits a path
    /// into "directory to scan" and "filename prefix" so the path
    /// completion module never has to special-case backslashes.
    fn path_separators(&self) -> &'static [char];

    /// `true` when `c` is one of the separators returned by
    /// [`PathStrategy::path_separators`].
    fn is_path_separator(&self, c: char) -> bool {
        self.path_separators().contains(&c)
    }

    /// `true` when `input` ends with any of the strategy's path
    /// separators. Used by `path_completion` to short-circuit the
    /// "list everything inside this directory" branch.
    fn ends_with_separator(&self, input: &str) -> bool {
        input
            .chars()
            .next_back()
            .map(|c| self.is_path_separator(c))
            .unwrap_or(false)
    }

    /// `true` when `input` is *exactly* the home alias `~` / `~/`,
    /// the Unix root `/`, a Windows drive root such as `C:\` or
    /// `C:/`, or a UNC root with no extra path component
    /// (`\\server\share\`). These shapes are visible "scopes" in
    /// the picker but MUST NOT be promoted to a commit preview.
    fn is_bare_scope(&self, input: &str) -> bool {
        let trimmed = input.trim();
        if matches!(trimmed, "~" | "~/") {
            return true;
        }
        if !self.is_absolute(trimmed) {
            return false;
        }
        // Strip the absolute root (drive letter, UNC root, or `/`)
        // and check that nothing meaningful follows.
        match self.split_absolute_root(trimmed) {
            Some((root, rest)) => rest.is_empty() && self.ends_with_separator(root),
            None => false,
        }
    }

    /// Split an absolute input into `(root_prefix, rest)` where
    /// `root_prefix` is the platform-specific anchor (`/`, `C:\`,
    /// `C:/`, `\\server\share\`) and `rest` is whatever the user
    /// typed after it. Returns `None` for inputs that are not
    /// absolute under this strategy.
    fn split_absolute_root<'a>(&self, input: &'a str) -> Option<(&'a str, &'a str)>;
}

/// Path strategy for macOS and Linux. Shared by both, per the
/// requirement: home location differences are injected, not modeled
/// here.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnixPathStrategy;

impl PathStrategy for UnixPathStrategy {
    fn kind(&self) -> PathStrategyKind {
        PathStrategyKind::Unix
    }

    fn is_absolute(&self, input: &str) -> bool {
        input.starts_with('/')
    }

    fn path_separators(&self) -> &'static [char] {
        &['/']
    }

    fn split_absolute_root<'a>(&self, input: &'a str) -> Option<(&'a str, &'a str)> {
        if !self.is_absolute(input) {
            return None;
        }
        Some((&input[..1], &input[1..]))
    }
}

/// Path strategy for Windows builds. Recognizes drive-rooted and UNC
/// absolute paths; does NOT classify `/foo` as absolute (that is a
/// Unix-style path that should be rejected on a Windows trusted-root
/// commit).
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsPathStrategy;

impl PathStrategy for WindowsPathStrategy {
    fn kind(&self) -> PathStrategyKind {
        PathStrategyKind::Windows
    }

    fn is_absolute(&self, input: &str) -> bool {
        is_windows_absolute(input)
    }

    fn path_separators(&self) -> &'static [char] {
        &['/', '\\']
    }

    fn split_absolute_root<'a>(&self, input: &'a str) -> Option<(&'a str, &'a str)> {
        if !self.is_absolute(input) {
            return None;
        }
        // UNC: `\\server\share\...` — the absolute root is up to and
        // including the separator after `share` (or the end of the
        // string when only the share is present).
        if let Some(rest) = input.strip_prefix(r"\\") {
            // Skip the server segment.
            let server_end = rest.find(['/', '\\']).unwrap_or(rest.len());
            if server_end == 0 {
                return None;
            }
            let after_server = &rest[server_end..];
            if after_server.is_empty() {
                // `\\server` with no share is not absolute enough;
                // is_windows_absolute already rejects this, but be
                // defensive.
                return None;
            }
            // `after_server` starts with a separator. Skip it, then
            // skip the share segment.
            let after_sep = &after_server[1..];
            let share_end = after_sep.find(['/', '\\']).unwrap_or(after_sep.len());
            // Length of `\\` + server + sep + share segment.
            let mut root_len = 2 + server_end + 1 + share_end;
            // Include the trailing separator after `share` if present.
            if share_end < after_sep.len() {
                root_len += 1;
            }
            let (root, rest) = input.split_at(root_len);
            return Some((root, rest));
        }
        // Drive-rooted: `C:\...` or `C:/...` — the root is `C:\` or
        // `C:/` (3 bytes since drive letters and the colon are ASCII).
        if input.len() >= 3 {
            let (root, rest) = input.split_at(3);
            return Some((root, rest));
        }
        None
    }
}

/// `true` when `input` is a Windows-absolute form: drive-rooted
/// (`C:\...`, `C:/...`) or UNC (`\\server\share\...`).
fn is_windows_absolute(input: &str) -> bool {
    if let Some(rest) = input.strip_prefix(r"\\") {
        let Some(server_end) = rest.find(['/', '\\']) else {
            return false;
        };
        if server_end == 0 {
            return false;
        }
        let after_server = &rest[server_end + 1..];
        let share_end = after_server.find(['/', '\\']).unwrap_or(after_server.len());
        return share_end > 0;
    }
    let bytes = input.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let sep = bytes[2];
        return sep == b'\\' || sep == b'/';
    }
    false
}

/// Convenience: pick the path strategy that matches the current
/// build target. Tests SHOULD inject one of the concrete strategies
/// directly; production code paths ask for this helper.
#[cfg(windows)]
pub fn current_target_strategy() -> Box<dyn PathStrategy + Send + Sync> {
    Box::new(WindowsPathStrategy)
}

/// Non-Windows builds always use the Unix strategy. macOS and Linux
/// share the same syntax model; the actual home directory comes from
/// the wizard and is injected when needed.
#[cfg(not(windows))]
pub fn current_target_strategy() -> Box<dyn PathStrategy + Send + Sync> {
    Box::new(UnixPathStrategy)
}

/// Resolve the visible browse-scope (`~/`, `~/dev/`, `/srv/`,
/// `C:\projects\`, `\\server\share\`) for `input`, given a strategy
/// and an optional home path.
///
/// Returns `None` for inputs that have no committable scope yet
/// (empty + no home, bare relative on a host that has no home, etc.).
pub fn visible_scope_for(
    strategy: &dyn PathStrategy,
    input: &str,
    home: Option<&Utf8Path>,
) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return home.map(|_| "~/".to_owned());
    }
    if trimmed == "~" || trimmed == "~/" {
        return home.map(|_| "~/".to_owned());
    }
    if strategy.is_home_alias(trimmed)
        && let Some(idx) = trimmed.rfind('/')
    {
        return Some(trimmed[..=idx].to_owned());
    }
    if strategy.is_absolute(trimmed) {
        let separators = strategy.path_separators();
        if let Some(last) = trimmed.rfind(separators) {
            // Length of "..separator" inclusive — keep the trailing
            // separator so the returned scope still reads as a
            // directory.
            return Some(trimmed[..=last].to_owned());
        }
        // No separator found — that can only happen for inputs whose
        // entire body is the absolute root (e.g. `/`). The strategy's
        // own root split tells us what to return.
        if let Some((root, _)) = strategy.split_absolute_root(trimmed) {
            return Some(root.to_owned());
        }
        return Some(trimmed.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_strategy_identifies_slash_rooted_input_as_absolute() {
        let s = UnixPathStrategy;
        assert!(s.is_absolute("/Users/me"));
        assert!(s.is_absolute("/"));
        assert!(!s.is_absolute("Users/me"));
        assert!(!s.is_absolute("~/dev"));
        assert!(!s.is_absolute(""));
        assert_eq!(s.kind(), PathStrategyKind::Unix);
    }

    #[test]
    fn unix_strategy_recognizes_home_alias() {
        let s = UnixPathStrategy;
        assert!(s.is_home_alias("~"));
        assert!(s.is_home_alias("~/"));
        assert!(s.is_home_alias("~/dev"));
        assert!(!s.is_home_alias("/dev"));
        assert!(!s.is_home_alias(""));
    }

    /// Windows strategy MUST recognize drive-rooted absolute forms
    /// for both `\` and `/` separator styles, plus UNC paths.
    #[test]
    fn windows_strategy_identifies_drive_rooted_and_unc_absolute_inputs() {
        let s = WindowsPathStrategy;
        assert!(s.is_absolute(r"C:\Users\me"));
        assert!(s.is_absolute("C:/Users/me"));
        assert!(s.is_absolute(r"D:\projects"));
        assert!(s.is_absolute(r"\\server\share\path"));
        assert!(s.is_absolute(r"\\server\share"));
        // UNC must have at least a server segment after `\\`.
        assert!(!s.is_absolute(r"\\"));
        assert!(!s.is_absolute(r"\\server"));
        assert!(!s.is_absolute(r"\\server\"));
        // Drive letter without a separator is NOT a path root.
        assert!(!s.is_absolute("C:"));
        // POSIX-style absolute is intentionally not classified as a
        // Windows-absolute trusted-root candidate; commits must be
        // explicit drive-rooted.
        assert!(!s.is_absolute("/Users/me"));
        assert_eq!(s.kind(), PathStrategyKind::Windows);
    }

    /// macOS and Linux MUST share `UnixPathStrategy`: when the home
    /// path is injected (e.g. `/home/me` for Linux), classification
    /// stays identical. This is the regression guard against ever
    /// splitting them into two strategies for the picker.
    #[test]
    fn unix_strategy_is_shared_across_macos_and_linux_via_injected_home() {
        let s = UnixPathStrategy;
        let macos_home = Utf8Path::new("/Users/me");
        let linux_home = Utf8Path::new("/home/me");

        assert_eq!(
            visible_scope_for(&s, "", Some(macos_home)),
            Some("~/".to_owned()),
        );
        assert_eq!(
            visible_scope_for(&s, "", Some(linux_home)),
            Some("~/".to_owned()),
        );
        assert_eq!(
            visible_scope_for(&s, "/usr/local", Some(macos_home)),
            Some("/usr/".to_owned()),
        );
        assert_eq!(
            visible_scope_for(&s, "/etc/nginx", Some(linux_home)),
            Some("/etc/".to_owned()),
        );
    }

    #[test]
    fn visible_scope_handles_bare_inputs_under_unix() {
        let s = UnixPathStrategy;
        let home = Utf8Path::new("/Users/me");
        assert_eq!(visible_scope_for(&s, "", Some(home)), Some("~/".to_owned()));
        assert_eq!(
            visible_scope_for(&s, "~", Some(home)),
            Some("~/".to_owned())
        );
        assert_eq!(
            visible_scope_for(&s, "~/dev/", Some(home)),
            Some("~/dev/".to_owned())
        );
        assert_eq!(
            visible_scope_for(&s, "~/dev/proj", Some(home)),
            Some("~/dev/".to_owned())
        );
        // Bare typed text without a known scope returns None — the
        // path_completion module then layers home-scope filtering on
        // top using its own logic.
        assert_eq!(visible_scope_for(&s, "a", Some(home)), None);
        assert_eq!(visible_scope_for(&s, "", None), None);
    }

    #[test]
    fn visible_scope_handles_windows_drive_and_unc_inputs() {
        let s = WindowsPathStrategy;
        assert_eq!(
            visible_scope_for(&s, r"C:\Users\me\dev", None),
            Some(r"C:\Users\me\".to_owned()),
        );
        assert_eq!(
            visible_scope_for(&s, "C:/Users/me/dev", None),
            Some("C:/Users/me/".to_owned()),
        );
        assert_eq!(
            visible_scope_for(&s, r"\\server\share\projects", None),
            Some(r"\\server\share\".to_owned()),
        );
    }

    #[test]
    fn unix_path_separators_only_include_forward_slash() {
        let s = UnixPathStrategy;
        assert_eq!(s.path_separators(), &['/']);
        assert!(s.is_path_separator('/'));
        assert!(!s.is_path_separator('\\'));
        assert!(s.ends_with_separator("/Users/me/"));
        assert!(!s.ends_with_separator("/Users/me"));
    }

    #[test]
    fn windows_path_separators_include_both_slash_and_backslash() {
        let s = WindowsPathStrategy;
        assert_eq!(s.path_separators(), &['/', '\\']);
        assert!(s.is_path_separator('/'));
        assert!(s.is_path_separator('\\'));
        assert!(s.ends_with_separator(r"C:\Users\"));
        assert!(s.ends_with_separator("C:/Users/"));
        assert!(!s.ends_with_separator(r"C:\Users\me"));
    }

    #[test]
    fn unix_split_absolute_root_returns_root_and_rest() {
        let s = UnixPathStrategy;
        assert_eq!(s.split_absolute_root("/Users/me"), Some(("/", "Users/me")));
        assert_eq!(s.split_absolute_root("/"), Some(("/", "")));
        assert_eq!(s.split_absolute_root("Users/me"), None);
        assert_eq!(s.split_absolute_root("~/dev"), None);
    }

    #[test]
    fn windows_split_absolute_root_handles_drive_and_unc_forms() {
        let s = WindowsPathStrategy;
        assert_eq!(
            s.split_absolute_root(r"C:\Users\me"),
            Some((r"C:\", r"Users\me"))
        );
        assert_eq!(
            s.split_absolute_root("C:/Users/me"),
            Some(("C:/", "Users/me"))
        );
        assert_eq!(s.split_absolute_root(r"D:\"), Some((r"D:\", "")));
        assert_eq!(
            s.split_absolute_root(r"\\server\share\path\to\thing"),
            Some((r"\\server\share\", r"path\to\thing")),
        );
        assert_eq!(
            s.split_absolute_root(r"\\server\share\"),
            Some((r"\\server\share\", "")),
        );
        // A bare share with no trailing separator is still classified
        // as a UNC root, with the entire input as the root and an
        // empty `rest`.
        assert_eq!(
            s.split_absolute_root(r"\\server\share"),
            Some((r"\\server\share", "")),
        );
        assert_eq!(s.split_absolute_root("/Users/me"), None);
        assert_eq!(s.split_absolute_root("~/dev"), None);
    }

    #[test]
    fn unix_is_bare_scope_matches_only_root_and_home_aliases() {
        let s = UnixPathStrategy;
        assert!(s.is_bare_scope("~"));
        assert!(s.is_bare_scope("~/"));
        assert!(s.is_bare_scope("/"));
        assert!(!s.is_bare_scope("/Users"));
        assert!(!s.is_bare_scope("~/dev"));
    }

    #[test]
    fn windows_is_bare_scope_matches_drive_and_unc_roots_only() {
        let s = WindowsPathStrategy;
        assert!(s.is_bare_scope("~"));
        assert!(s.is_bare_scope("~/"));
        assert!(s.is_bare_scope(r"C:\"));
        assert!(s.is_bare_scope("C:/"));
        assert!(s.is_bare_scope(r"\\server\share\"));
        assert!(!s.is_bare_scope(r"C:\Users"));
        assert!(!s.is_bare_scope(r"\\server\share\path"));
        // Naked `/` is NOT a Windows-absolute scope.
        assert!(!s.is_bare_scope("/"));
    }
}

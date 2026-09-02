//! Human-friendly path rendering for CLI output.
//!
//! The CLI surfaces filesystem paths in many places (config overview,
//! `project list`, `workspace list`, `session list`, doctor, completion,
//! reindex previews, …). When those paths live under the user's home
//! directory we want to abbreviate the prefix to `~/` so the output
//! stays readable in narrow terminals and matches what users typically
//! type by hand.
//!
//! Rules:
//! - Abbreviation is *human-only*. JSON, `--raw`, and any other
//!   machine-readable surface MUST keep the absolute path.
//! - We only abbreviate the user's *current* home directory. We do not
//!   try to detect other users' homes (e.g. `/Users/alice/foo` while
//!   running as `bob`). That avoids surprising rewrites of paths the
//!   user explicitly typed.
//! - User-configured paths that already use `~/` are passed through
//!   unchanged — this helper is for system-derived absolute paths.

use camino::{Utf8Path, Utf8PathBuf};

/// Abbreviate `path` to `~/...` when it is under the current user's
/// home directory. Otherwise return the absolute path verbatim.
///
/// This is the single helper every human-facing CLI path render should
/// flow through. Pass machine-readable rendering paths (JSON, `--raw`)
/// the absolute path directly instead of calling this helper.
pub fn display_path_for_humans(path: &Utf8Path) -> String {
    display_path_for_humans_with_home(path, current_home().as_deref())
}

/// Owned variant for [`Utf8PathBuf`] callers.
#[allow(dead_code)]
pub fn display_path_buf_for_humans(path: &Utf8PathBuf) -> String {
    display_path_for_humans(path.as_path())
}

/// Inner core that takes an explicit `home` for testing.
///
/// Returned strings:
/// - `~` when `path == home`
/// - `~/<rel>` when `path` is under `home`
/// - the original path string otherwise
pub(crate) fn display_path_for_humans_with_home(
    path: &Utf8Path,
    home: Option<&Utf8Path>,
) -> String {
    let raw = path.as_str();
    let Some(home) = home else {
        return raw.to_owned();
    };
    if home.as_str().is_empty() {
        return raw.to_owned();
    }
    if path == home {
        return "~".to_owned();
    }
    if let Ok(rel) = path.strip_prefix(home) {
        // `Path::strip_prefix` returns an empty path when stripped exactly,
        // already handled by the equality check above. For non-empty
        // remainders, glue back with a forward slash so the rendering is
        // stable across platforms (CLI output is ASCII).
        let rel_str = rel.as_str();
        if rel_str.is_empty() {
            "~".to_owned()
        } else {
            format!("~/{rel_str}")
        }
    } else {
        raw.to_owned()
    }
}

fn current_home() -> Option<Utf8PathBuf> {
    // We deliberately read `$HOME` directly rather than depending on the
    // `dirs` crate. The CLI uses the same convention in `wizard.rs`, so
    // the abbreviation tracks whatever home the user actually typed at
    // login (a re-export of platform behavior on Unix). On Windows
    // `%USERPROFILE%` would need a different probe, but the CLI is not
    // packaged for Windows today; treating that case as "no home" is
    // safe and simply skips abbreviation.
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(Utf8PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbreviates_under_home() {
        let home = Utf8PathBuf::from("/Users/alice");
        let path = Utf8PathBuf::from("/Users/alice/projects/foo");
        let rendered = display_path_for_humans_with_home(path.as_path(), Some(home.as_path()));
        assert_eq!(rendered, "~/projects/foo");
    }

    #[test]
    fn home_root_renders_as_tilde() {
        let home = Utf8PathBuf::from("/Users/alice");
        let rendered = display_path_for_humans_with_home(home.as_path(), Some(home.as_path()));
        assert_eq!(rendered, "~");
    }

    #[test]
    fn non_home_path_passes_through() {
        let home = Utf8PathBuf::from("/Users/alice");
        let path = Utf8PathBuf::from("/var/log/system.log");
        let rendered = display_path_for_humans_with_home(path.as_path(), Some(home.as_path()));
        assert_eq!(rendered, "/var/log/system.log");
    }

    #[test]
    fn missing_home_passes_through() {
        let path = Utf8PathBuf::from("/Users/alice/projects/foo");
        let rendered = display_path_for_humans_with_home(path.as_path(), None);
        assert_eq!(rendered, "/Users/alice/projects/foo");
    }

    #[test]
    fn does_not_abbreviate_other_users_home() {
        let home = Utf8PathBuf::from("/Users/alice");
        let path = Utf8PathBuf::from("/Users/bob/work");
        let rendered = display_path_for_humans_with_home(path.as_path(), Some(home.as_path()));
        assert_eq!(rendered, "/Users/bob/work");
    }
}

//! Real filesystem path canonicalization adapter.
//!
//! Implements `PathCanonicalizationPort` by delegating to `std::fs::canonicalize`.

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::ports::PathCanonicalizationPort;

/// Production implementation of `PathCanonicalizationPort`.
///
/// Uses `std::fs::canonicalize` to resolve paths against the live filesystem,
/// falling back to lexical normalization when the path does not exist.
pub struct RealPathCanonicalization;

impl RealPathCanonicalization {
    /// Create a new instance.
    pub fn new() -> Self {
        Self
    }
}

impl Default for RealPathCanonicalization {
    fn default() -> Self {
        Self::new()
    }
}

impl PathCanonicalizationPort for RealPathCanonicalization {
    fn canonicalize(&self, path: &Utf8Path) -> Utf8PathBuf {
        // Try std::fs::canonicalize first (resolves symlinks)
        if let Ok(real) = std::fs::canonicalize(path.as_std_path())
            && let Ok(utf8) = Utf8PathBuf::try_from(real)
        {
            return utf8;
        }
        // Fall back to lexical normalization for non-existent paths
        workhive_core::domain::normalize_lexical(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8Path;
    use std::fs;
    use tempfile::TempDir;

    /// `RealPathCanonicalization::new()` must resolve symlinks when the path exists.
    /// This is the primary contract: filesystem-aware canonicalization.
    #[test]
    fn real_canonicalization_resolves_symlinks_for_existing_paths() {
        let temp = TempDir::new().expect("failed to create temp dir");
        let real_dir = temp.path().join("real");
        let symlink_dir = temp.path().join("link");

        fs::create_dir(&real_dir).expect("failed to create real dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &symlink_dir).expect("failed to create symlink");
        #[cfg(windows)]
        std::os::windows::fs::create_dir_symlink(&real_dir, &symlink_dir)
            .expect("failed to create symlink");

        let canonicalizer = RealPathCanonicalization::new();
        let symlink_utf8 = Utf8PathBuf::try_from(symlink_dir.clone()).expect("non-UTF8 temp path");
        let real_utf8 = Utf8PathBuf::try_from(real_dir.clone()).expect("non-UTF8 temp path");

        let result = canonicalizer.canonicalize(&symlink_utf8);

        assert_eq!(
            result, real_utf8,
            "canonicalize must resolve the symlink to the real path"
        );
    }

    /// Non-existent paths must fall back to lexical normalization, not error.
    /// This is the contract: the method never fails.
    #[test]
    fn real_canonicalization_falls_back_to_lexical_for_nonexistent_paths() {
        let canonicalizer = RealPathCanonicalization::new();
        let nonexistent = Utf8Path::new("/this/path/definitely/does/not/exist/anywhere");

        let result = canonicalizer.canonicalize(nonexistent);

        assert_eq!(
            result,
            Utf8PathBuf::from("/this/path/definitely/does/not/exist/anywhere"),
            "non-existent paths must fall back to lexical normalization"
        );
    }

    /// Lexical normalization must collapse `..` and `.` segments even for
    /// non-existent paths. This verifies the fallback behavior.
    #[test]
    fn real_canonicalization_collapses_dot_segments_for_nonexistent_paths() {
        let canonicalizer = RealPathCanonicalization::new();
        let path_with_dots = Utf8Path::new("/dev/../work/./proj");

        let result = canonicalizer.canonicalize(path_with_dots);

        assert_eq!(
            result,
            Utf8PathBuf::from("/work/proj"),
            "lexical fallback must collapse `..` and `.` segments"
        );
    }

    /// Existing paths without symlinks must still be normalized.
    /// This verifies the happy path.
    #[test]
    fn real_canonicalization_normalizes_existing_paths_without_symlinks() {
        let temp = TempDir::new().expect("failed to create temp dir");
        let nested = temp.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).expect("failed to create nested dirs");

        let canonicalizer = RealPathCanonicalization::new();
        let nested_utf8 = Utf8PathBuf::try_from(nested.clone()).expect("non-UTF8 temp path");

        let result = canonicalizer.canonicalize(&nested_utf8);

        assert_eq!(
            result, nested_utf8,
            "existing paths without symlinks must return the normalized path"
        );
    }

    /// The method must handle the root path `/` correctly.
    /// Edge case: root is always normalized to `/`.
    #[test]
    fn real_canonicalization_handles_root_path() {
        let canonicalizer = RealPathCanonicalization::new();
        let root = Utf8Path::new("/");

        let result = canonicalizer.canonicalize(root);

        assert_eq!(
            result,
            Utf8PathBuf::from("/"),
            "root path must normalize to `/`"
        );
    }

    /// The method must handle empty paths by returning the lexically normalized form.
    /// Edge case: empty string normalizes to `.` per lexical normalization rules.
    #[test]
    fn real_canonicalization_handles_empty_path() {
        let canonicalizer = RealPathCanonicalization::new();
        let empty = Utf8Path::new("");

        let result = canonicalizer.canonicalize(empty);

        assert_eq!(
            result,
            Utf8PathBuf::from("."),
            "empty path must fall back to lexical normalization (`.`)"
        );
    }

    /// Relative paths must be normalized lexically when they don't exist.
    /// Edge case: relative paths are not resolved against cwd by the lexical fallback.
    #[test]
    fn real_canonicalization_handles_relative_paths() {
        let canonicalizer = RealPathCanonicalization::new();
        let relative = Utf8Path::new("relative/path/../to/./file");

        let result = canonicalizer.canonicalize(relative);

        assert_eq!(
            result,
            Utf8PathBuf::from("relative/to/file"),
            "relative paths must be normalized lexically"
        );
    }

    /// The method must handle paths with multiple consecutive separators.
    /// Edge case: lexical normalization collapses them.
    #[test]
    fn real_canonicalization_handles_multiple_separators() {
        let canonicalizer = RealPathCanonicalization::new();
        let path_with_multiple = Utf8Path::new("/dev///work////proj");

        let result = canonicalizer.canonicalize(path_with_multiple);

        assert_eq!(
            result,
            Utf8PathBuf::from("/dev/work/proj"),
            "multiple separators must be collapsed by lexical normalization"
        );
    }

    /// The method must handle paths ending with `/.` or `/..`.
    /// Edge case: trailing dot segments must be resolved.
    #[test]
    fn real_canonicalization_handles_trailing_dots() {
        let canonicalizer = RealPathCanonicalization::new();
        let path_trailing_dot = Utf8Path::new("/dev/work/.");
        let path_trailing_dotdot = Utf8Path::new("/dev/work/..");

        let result_dot = canonicalizer.canonicalize(path_trailing_dot);
        let result_dotdot = canonicalizer.canonicalize(path_trailing_dotdot);

        assert_eq!(
            result_dot,
            Utf8PathBuf::from("/dev/work"),
            "trailing `.` must be collapsed"
        );
        assert_eq!(
            result_dotdot,
            Utf8PathBuf::from("/dev"),
            "trailing `..` must pop the parent"
        );
    }
}

//! Fake path canonicalization for testing.
//!
//! Provides a test double for `PathCanonicalizationPort` that returns
//! predictable results without touching the filesystem.

use camino::{Utf8Path, Utf8PathBuf};
use std::cell::RefCell;
use std::collections::HashMap;
use workhive_core::ports::PathCanonicalizationPort;

/// A fake path canonicalizer for testing.
///
/// Returns pre-configured results for specific paths, falling back to
/// lexical normalization for unknown paths.
pub struct FakePathCanonicalization {
    /// Map from input path to canonicalized result.
    results: RefCell<HashMap<Utf8PathBuf, Utf8PathBuf>>,
    /// Optional default result for unmatched paths.
    default: RefCell<Option<Utf8PathBuf>>,
}

impl FakePathCanonicalization {
    /// Create a new fake canonicalizer with no pre-configured results.
    pub fn new() -> Self {
        Self {
            results: RefCell::new(HashMap::new()),
            default: RefCell::new(None),
        }
    }

    /// Create a fake canonicalizer with a single result.
    /// Create a fake canonicalizer with a single result.
    pub fn with_result(input: Utf8PathBuf, output: Utf8PathBuf) -> Self {
        let fake = Self::new();
        fake.set_result(input, output);
        fake
    }

    /// Set the result for a specific input path.
    pub fn set_result(&self, input: Utf8PathBuf, output: Utf8PathBuf) {
        self.results.borrow_mut().insert(input, output);
    }

    /// Set the default result for unmatched paths.
    pub fn set_default(&self, output: Utf8PathBuf) {
        *self.default.borrow_mut() = Some(output);
    }

    /// Clear all pre-configured results.
    pub fn clear(&self) {
        self.results.borrow_mut().clear();
        *self.default.borrow_mut() = None;
    }

    /// Get the number of pre-configured results.
    pub fn len(&self) -> usize {
        self.results.borrow().len()
    }

    /// Check if the fake is empty (no pre-configured results).
    pub fn is_empty(&self) -> bool {
        self.results.borrow().is_empty()
    }
}

impl Default for FakePathCanonicalization {
    fn default() -> Self {
        Self::new()
    }
}

impl PathCanonicalizationPort for FakePathCanonicalization {
    fn canonicalize(&self, path: &Utf8Path) -> Utf8PathBuf {
        // Check pre-configured results first
        if let Some(result) = self.results.borrow().get(path) {
            return result.clone();
        }
        // Check default result
        if let Some(ref default) = *self.default.borrow() {
            return default.clone();
        }
        // Fall back to lexical normalization (no filesystem access)
        workhive_core::domain::normalize_lexical(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    /// `FakePathCanonicalization::new()` starts empty; every path must
    /// fall back to `normalize_lexical` so test authors can rely on
    /// deterministic behavior without seeding.
    #[test]
    fn fake_canonicalization_falls_back_to_lexical_when_empty() {
        let fake = FakePathCanonicalization::new();
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/dev/../work/proj")),
            Utf8PathBuf::from("/work/proj"),
            "an unseeded fake must collapse `..` segments just like the domain's lexical normalizer"
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/dev/./proj")),
            Utf8PathBuf::from("/dev/proj"),
            "an unseeded fake must collapse `.` segments"
        );
    }

    /// Seeding a result for a specific path returns that result verbatim.
    #[test]
    fn fake_canonicalization_returns_seeded_result_for_matching_input() {
        let fake = FakePathCanonicalization::with_result(
            Utf8PathBuf::from("/var/run"),
            Utf8PathBuf::from("/private/var/run"),
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/var/run")),
            Utf8PathBuf::from("/private/var/run"),
            "seeded result must override the lexical fallback (e.g., simulating macOS symlink resolution)"
        );
    }

    /// Seeding a result for one path does not affect other paths.
    #[test]
    fn fake_canonicalization_does_not_cross_pollinate_between_paths() {
        let fake = FakePathCanonicalization::with_result(
            Utf8PathBuf::from("/var/run"),
            Utf8PathBuf::from("/private/var/run"),
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/dev")),
            Utf8PathBuf::from("/dev"),
            "an unseeded path must still fall back to lexical normalization"
        );
    }

    /// `set_default` provides a catch-all result for any path that
    /// hasn't been individually seeded. Useful for simulating
    /// "everything canonicalizes to X" test scenarios.
    #[test]
    fn fake_canonicalization_set_default_applies_to_unseeded_paths() {
        let fake = FakePathCanonicalization::new();
        fake.set_default(Utf8PathBuf::from("/default"));
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/any/path")),
            Utf8PathBuf::from("/default"),
            "set_default must apply to paths that weren't individually seeded"
        );
    }

    /// Individually seeded results take precedence over the default.
    #[test]
    fn fake_canonicalization_seeded_result_takes_precedence_over_default() {
        let fake = FakePathCanonicalization::new();
        fake.set_default(Utf8PathBuf::from("/default"));
        fake.set_result(
            Utf8PathBuf::from("/specific"),
            Utf8PathBuf::from("/specific-resolved"),
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/specific")),
            Utf8PathBuf::from("/specific-resolved"),
            "seeded result must override the default"
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/other")),
            Utf8PathBuf::from("/default"),
            "unseeded paths must still use the default"
        );
    }

    /// `clear()` wipes all seeded results and the default.
    #[test]
    fn fake_canonicalization_clear_resets_all_state() {
        let fake = FakePathCanonicalization::new();
        fake.set_result(Utf8PathBuf::from("/a"), Utf8PathBuf::from("/a-resolved"));
        fake.set_default(Utf8PathBuf::from("/default"));
        assert_eq!(fake.len(), 1);
        assert!(!fake.is_empty());

        fake.clear();

        assert_eq!(fake.len(), 0);
        assert!(fake.is_empty());
        // After clear, all paths must fall back to lexical normalization.
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/a")),
            Utf8PathBuf::from("/a"),
            "after clear, seeded result for /a must be gone"
        );
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/other")),
            Utf8PathBuf::from("/other"),
            "after clear, default must be gone"
        );
    }

    /// `set_result` overwrites any previous seed for the same input.
    #[test]
    fn fake_canonicalization_set_result_overwrites_previous_seed() {
        let fake = FakePathCanonicalization::new();
        fake.set_result(Utf8PathBuf::from("/a"), Utf8PathBuf::from("/first"));
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/a")),
            Utf8PathBuf::from("/first")
        );
        fake.set_result(Utf8PathBuf::from("/a"), Utf8PathBuf::from("/second"));
        assert_eq!(
            fake.canonicalize(Utf8Path::new("/a")),
            Utf8PathBuf::from("/second"),
            "set_result must overwrite any previous seed for the same input"
        );
        assert_eq!(fake.len(), 1, "duplicate seeds must not grow the map");
    }
}

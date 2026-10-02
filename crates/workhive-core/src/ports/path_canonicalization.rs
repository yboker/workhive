//! Path canonicalization port for filesystem path resolution.
//!
//! All code that needs to resolve a path against the live filesystem
//! MUST call `PathCanonicalizationPort::canonicalize()` instead of
//! calling `std::fs::canonicalize()` directly. This keeps domain
//! code free of direct filesystem dependencies and enables deterministic
//! testing without touching the real filesystem.
//!
//! ## Implementations
//!
//! - [`PathCanonicalizationPort`] — the trait (here).
//! - `workhive_infra::path_canonicalization::RealPathCanonicalization`
//!   — production implementation that delegates to `std::fs::canonicalize`.
//! - `workhive_testkit::fakes::FakePathCanonicalization` — test double
//!   that returns user-controlled results.

use camino::{Utf8Path, Utf8PathBuf};

/// A path canonicalization service that resolves paths against the
/// live filesystem.
///
/// Implementations must handle non-existent paths gracefully (typically
/// by returning the input unchanged or applying lexical normalization).
/// They must not panic on invalid UTF-8 input.
pub trait PathCanonicalizationPort {
    /// Canonicalize a path by resolving symlinks and normalizing
    /// `.` / `..` components. Returns the canonicalized path on success,
    /// or falls back to lexical normalization if the path does not exist.
    ///
    /// This method must never fail — if the path cannot be canonicalized
    /// (e.g., does not exist), implementations should return a
    /// lexically normalized version instead.
    fn canonicalize(&self, path: &Utf8Path) -> Utf8PathBuf;
}

//! Filesystem port that powers Tab completion in interactive path prompts.
//!
//! This port is intentionally narrow: callers ask for the immediate
//! child entries of a directory and get back `(name, is_directory)` tuples.
//! It exists so the trusted-root wizard can offer real directory completion
//! without leaking `std::fs` calls into business logic, and so the pure
//! completion logic in [`crate::usecases::path_completion`] can be unit
//! tested with an in-memory fake.
//!
//! The API returns a typed `Result` so callers can decide how to surface
//! filesystem failures — completion-driving callers (interactive UI)
//! generally render a localized "no candidates" hint and continue, while
//! audit-style callers may bubble the error up.

use camino::Utf8Path;

/// Error type for path completion operations.
///
/// Wraps I/O errors to prevent `std::io::Error` from leaking into
/// the domain layer, maintaining the hexagonal architecture boundary.
#[derive(Debug, thiserror::Error)]
pub enum PathCompletionError {
    /// I/O error during filesystem operations.
    #[error("path completion I/O error: {detail}")]
    Io { detail: String },
}

/// One filesystem entry returned by [`PathCompletionFsPort::list_children`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathCompletionEntry {
    /// File name (last path segment) without the parent directory.
    pub name: String,
    /// Whether the entry is itself a directory. Only directory entries
    /// are surfaced as completions for trusted-root input — see
    /// `compute_path_completions`.
    pub is_directory: bool,
}

/// Classification of a single filesystem path. Returned by
/// [`PathCompletionFsPort::classify_path`] so the trusted-root wizard
/// can validate `existence + is_directory` without leaking `std::fs`
/// into business logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Path resolves to a directory on disk.
    Directory,
    /// Path resolves to a regular file (or symlink to a non-directory).
    /// Trusted-root input rejects this — see
    /// `usecases::trusted_roots::classify_trusted_root_input`.
    File,
    /// Path does not exist or cannot be inspected.
    /// Adapters MUST collapse "permission denied", "broken symlink",
    /// and "not found" all into `Missing` — the wizard has nothing
    /// useful to do with the distinction.
    Missing,
}

/// Read immediate directory children, and classify single paths, for
/// Tab-completion prompts and trusted-root validation.
///
/// Errors carry context so callers can log at the appropriate level.
/// The interactive completion path treats all errors as "no candidates
/// available" and renders a localized hint, rather than aborting the
/// wizard, but the typed error remains available for debug-level tracing
/// and for non-interactive callers.
pub trait PathCompletionFsPort {
    /// Return the immediate child entries of `parent`, or a typed error
    /// if the directory cannot be read.
    fn list_children(
        &self,
        parent: &Utf8Path,
    ) -> Result<Vec<PathCompletionEntry>, PathCompletionError>;

    /// Classify a single path so trusted-root validation can answer
    /// "does this exist? is it a directory?" without business logic
    /// touching `std::fs::metadata` directly.
    fn classify_path(&self, path: &Utf8Path) -> PathKind;
}

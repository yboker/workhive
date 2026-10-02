//! Shared "did we change anything?" return type used by the
//! provider-specific install helpers.
//!
//! The automatic preparation lifecycle (`prepare_provider`) must be
//! silent and idempotent: when nothing on disk needs updating, no
//! third-party file is rewritten and no backup is taken. Each
//! provider's `install` helper returns one of these variants so the
//! lifecycle wrapper can roll the outcomes up into a single
//! [`super::PreparationOutcome`] for its caller.

use camino::Utf8PathBuf;

/// Outcome of a single provider-config edit step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InstallChange {
    /// The on-disk file already encoded exactly the desired Workhive
    /// state. No write was performed and no backup was created.
    AlreadyCurrent,
    /// The file was rewritten (created or updated). `backup` is the
    /// pre-mutation backup path the helper produced, or `None` for
    /// workhive-owned files (e.g. OpenCode plugin) where no user
    /// content needed preserving.
    Updated { backup: Option<Utf8PathBuf> },
}

impl InstallChange {
    /// Whether the helper actually performed a write.
    pub(super) fn was_written(&self) -> bool {
        matches!(self, InstallChange::Updated { .. })
    }
}

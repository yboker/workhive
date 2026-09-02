//! Durable branch-provenance store boundary for managed worktrees.
//!
//! ## Why this port exists
//!
//! When `workspace open` provisions a managed worktree it either creates
//! a brand-new local branch or attaches an already-existing one. Removal
//! and destroy must clean up Workhive-created branches as a unit with
//! their worktree, while user-authored branches must be protected and
//! require explicit consent before deletion.
//!
//! The only safe way to know which case applies later is to record the
//! decision authoritatively at open time. This port persists that typed
//! provenance, scoped per managed worktree, so removal never has to
//! guess branch origin from Git state, reflog, timestamps, or naming.
//!
//! ## Boundary rules
//!
//! - The trait stays in `workhive-core`; concrete adapters live in
//!   `workhive-infra` (a JSON-backed file under `.workhive/`).
//! - Provenance is Workhive-owned state, not user configuration.
//! - Missing, unreadable, or legacy provenance MUST resolve to
//!   [`BranchProvenance::Unknown`] so removal stays conservative; this is
//!   encoded in the type by making [`WorktreeProvenancePort::lookup`]
//!   infallible.

use camino::Utf8PathBuf;

/// Whether the local branch backing a Workhive-managed worktree was
/// created by Workhive or attached from a pre-existing branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchProvenance {
    /// Workhive created the backing branch because it did not exist when
    /// the workspace was opened. Removal may delete it automatically.
    WorkhiveCreated,
    /// The user explicitly attached an already-existing local branch.
    /// Removal must keep it by default and require explicit consent.
    PreExistingAttached,
    /// Provenance is missing, unreadable, or predates provenance
    /// tracking. Treated conservatively as user-owned (pre-existing).
    Unknown,
}

impl BranchProvenance {
    /// Whether removal/destroy may safely delete the backing branch
    /// automatically by default. Only Workhive-created branches qualify.
    pub fn auto_deletes_branch(self) -> bool {
        matches!(self, Self::WorkhiveCreated)
    }

    /// Whether the backing branch is user-owned and must be kept by
    /// default — deletion requires explicit confirmation or an explicit
    /// non-interactive authorization flag.
    pub fn is_user_owned(self) -> bool {
        matches!(self, Self::PreExistingAttached | Self::Unknown)
    }
}

/// Errors surfaced by [`WorktreeProvenancePort`] write operations.
///
/// Read operations never error: [`WorktreeProvenancePort::lookup`] is
/// infallible and resolves any missing/unreadable state to
/// [`BranchProvenance::Unknown`].
#[derive(Debug, thiserror::Error)]
pub enum WorktreeProvenanceError {
    /// The on-disk store could not be written.
    #[error("worktree provenance I/O at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The store contents could not be serialized for writing.
    #[error("worktree provenance store at {path} failed to serialize: {detail}")]
    Serialization { path: Utf8PathBuf, detail: String },
}

/// Durable, Workhive-owned source of managed-worktree branch provenance.
///
/// Implementations must be idempotent and torn-write-safe (atomic
/// rename). Provenance is keyed by the managed worktree's workspace
/// name.
pub trait WorktreeProvenancePort {
    /// Record (insert or update) provenance for a managed worktree.
    ///
    /// Recording [`BranchProvenance::Unknown`] clears any stored entry so
    /// the worktree resolves back to the conservative default.
    fn record(
        &self,
        workspace: &str,
        provenance: BranchProvenance,
    ) -> Result<(), WorktreeProvenanceError>;

    /// Resolve the stored provenance for a managed worktree.
    ///
    /// Infallible by contract: a missing, unreadable, or legacy store
    /// resolves to [`BranchProvenance::Unknown`] so removal never
    /// proceeds on an optimistic assumption.
    fn lookup(&self, workspace: &str) -> BranchProvenance;

    /// Forget provenance for a managed worktree. Idempotent — forgetting
    /// an absent entry is not an error.
    fn forget(&self, workspace: &str) -> Result<(), WorktreeProvenanceError>;
}

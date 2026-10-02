//! Durable pane launch-identity store boundary.
//!
//! ## Why this port exists
//!
//! The pane control-plane supervisor runs as a **detached child process**.
//! It cannot share the in-memory `PaneRegistryPort` that `workspace open`
//! populated in the parent CLI process. Without a durable, cross-process
//! source of authoritative launch identity, the supervisor's `QueryPane`
//! response would always have `display_title == None`, even though the
//! parent process knew that the pane was launched as `codex` or `claude`.
//!
//! This port provides exactly that durable bridge:
//!
//! - The `workspace open` path **records** authoritative launch identity
//!   (the canonical [`PaneLaunchOrigin`]) into the store right after it
//!   resolves the runtime `UnitId`.
//! - The detached supervisor **hydrates** an empty in-memory registry
//!   from the store at startup, before serving its first request.
//! - `close --kill` and project `destroy` **forget** identity entries to
//!   keep the store in step with the underlying tmux session.
//!
//! Without this durable store, the control-plane display title would
//! rely on a process-local registry that the detached supervisor cannot
//! see; this port is the production-path durable source.
//!
//! ## Boundary rules
//!
//! - The trait stays in `workhive-core`. Concrete adapters live in
//!   `workhive-infra` (e.g. JSON-backed file-system implementation).
//! - The store is **not** a substitute for the in-memory
//!   [`crate::ports::PaneRegistryPort`]. It only holds Workhive-managed
//!   launch identity; runtime-observed fields (cwd, foreground command,
//!   live state) belong in the in-memory registry, populated from
//!   `runtime.list_units` during `QueryPane`.
//! - All errors must be typed; the trait must not leak `Box<dyn Error>`.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

use crate::domain::{PaneLaunchOrigin, UnitId};

/// Single durable entry recorded by the open path and replayed by the
/// supervisor at startup.
///
/// The keying tuple is `(group, workspace, unit_id)`. `group` is implicit
/// in the store the entry lives in (one store file per group on disk),
/// so the entry only carries `workspace` and `unit_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneLaunchIdentityEntry {
    /// Workspace name (tmux session name in the current backend).
    pub workspace: String,
    /// Runtime-neutral unit identifier within the workspace.
    pub unit_id: UnitId,
    /// Workhive-managed authoritative launch identity. This is the
    /// **only** legitimate source for `QueryPane::display_title`.
    pub launch_origin: PaneLaunchOrigin,
}

/// Errors surfaced by [`PaneLaunchIdentityPort`] implementations.
///
/// The variants are intentionally narrow: callers handle each one
/// distinctly because the store sits on a fail-open boundary (the open
/// path must not block on a corrupt store, and the supervisor must not
/// refuse to serve when the store is missing).
#[derive(Debug, thiserror::Error)]
pub enum PaneLaunchIdentityError {
    /// The on-disk store directory or file could not be read or written.
    #[error("pane launch identity I/O at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The store file was reachable but its contents could not be parsed
    /// against the current schema. Implementations may treat this as
    /// "no entries" and continue, but the typed variant gives callers
    /// the option to log/repair.
    #[error("pane launch identity store at {path} failed to parse: {detail}")]
    Serialization { path: Utf8PathBuf, detail: String },

    /// The supplied group name was rejected by the implementation's
    /// path-safety policy. Implementations MUST sanitize groups
    /// defensively; this variant exists so unit tests can assert the
    /// rejection happened, not as a normal user-facing path.
    #[error("pane launch identity store rejected group {group:?}: {reason}")]
    Group { group: String, reason: String },
}

/// Durable, cross-process source of pane launch identity.
///
/// Implementations MUST be safe to call from multiple processes serially:
/// the typical sequence is "open writes → supervisor reads → close
/// forgets". Concurrent writers within a single project are already
/// serialized at a higher layer by `ProjectInitLockPort`, so the
/// adapter only needs to be torn-write-safe (atomic rename suffices).
pub trait PaneLaunchIdentityPort {
    /// Record (insert or update) the authoritative launch identity for
    /// a `(group, workspace, unit_id)` triple.
    ///
    /// The implementation MUST be idempotent — calling `record` twice
    /// with the same triple replaces the previous entry.
    fn record(
        &self,
        group: &str,
        entry: PaneLaunchIdentityEntry,
    ) -> Result<(), PaneLaunchIdentityError>;

    /// Return every entry recorded under `group`. Used by the
    /// supervisor at startup to hydrate its in-memory registry.
    ///
    /// Returns `Ok(Vec::new())` (not an error) when the store has no
    /// records for the group — the supervisor must not refuse to
    /// serve simply because no identities were ever recorded (the
    /// project may legitimately have no Workhive-launched panes).
    fn list_for_group(
        &self,
        group: &str,
    ) -> Result<Vec<PaneLaunchIdentityEntry>, PaneLaunchIdentityError>;

    /// Forget every entry that belongs to `(group, workspace)`. Used
    /// by `workspace close --kill` to keep the store in step with the
    /// underlying runtime.
    fn forget_workspace(&self, group: &str, workspace: &str)
    -> Result<(), PaneLaunchIdentityError>;

    /// Forget every entry under `group`, including its on-disk
    /// representation. Used by project `destroy` so a re-created
    /// project does not inherit stale identity.
    fn forget_group(&self, group: &str) -> Result<(), PaneLaunchIdentityError>;
}

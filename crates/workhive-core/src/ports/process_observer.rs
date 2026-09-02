//! Process observer port — high-confidence OS-level signal.
//!
//! The OS process observer answers: "given a tmux pane's shell PID,
//! what does the kernel say is *actually* running right now?".
//!
//! It is the second-tier truth source after the Workhive supervisor.
//! It MUST NOT short-circuit higher-confidence supervisor data, and
//! it MUST NOT silently replace tmux fallback when the OS query fails
//! — failures must surface as `Ok(None)` so the merge policy keeps
//! looking, or as an explicit error.
//!
//! ## PID-first contract
//!
//! Workhive owns the shell PID for every pane it spawns (tmux's
//! `pane_pid`), and the kernel's process group / controlling terminal
//! semantics are anchored on PIDs, not on tty path strings. Adapters
//! MUST therefore treat `shell_pid` as the primary, authoritative
//! input. An adapter MAY use `tty` as a sanity check (e.g., to verify
//! the foreground process group still belongs to the same controlling
//! terminal as the shell), but MUST NOT use the tty as the sole
//! lookup key.
//!
//! Concretely: if `shell_pid` is `None`, the adapter MUST return
//! `Ok(None)` for that lookup, even when `tty` is present. tty-only
//! observation would silently degrade to "best-effort guessing" in
//! environments where multiple shells share a tty (`tmux attach -t`,
//! ssh sessions sharing a controlling terminal, etc.) and is exactly
//! the kind of fragility this port is meant to remove.
//!
//! ## Outputs
//!
//! On success the adapter returns a [`PaneObservation`] with
//! [`crate::domain::PaneObservationConfidence::High`]. The
//! `process` field is required; `state_hint` may be omitted when the
//! observer cannot derive a [`crate::domain::PaneState`] purely from
//! the OS view.
//!
//! ## Truthful unknowns
//!
//! Returning `Ok(None)` means "I looked and could not find a
//! foreground process" (e.g., the shell PID is gone, or its
//! controlling terminal has no foreground process group). The merge
//! policy then falls through to tmux fallback. Adapters MUST NOT
//! fabricate a process to avoid `unknown`.

use crate::domain::PaneObservation;

/// Inputs an OS process observer needs to inspect a tmux pane.
///
/// `shell_pid` is the primary input (PID-first contract; see module
/// docs). `tty` is advisory — adapters may use it for sanity checks
/// but MUST NOT use it as the sole lookup key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneProcessLookup {
    /// Path to the pane's controlling terminal (e.g.,
    /// `/dev/ttys003`). Empty when tmux did not report one. Advisory
    /// only: used for cross-checks, not as the lookup key.
    pub tty: Option<String>,

    /// PID tmux reports for the pane's shell (`pane_pid`). The
    /// authoritative root of the foreground process group lookup.
    /// When `None`, the adapter MUST report `Ok(None)`.
    pub shell_pid: Option<u32>,
}

impl PaneProcessLookup {
    /// True when there is no usable identifier to look up.
    ///
    /// Under the PID-first contract `shell_pid` IS the lookup key, so
    /// an absent `shell_pid` always means "empty" regardless of `tty`.
    pub fn is_empty(&self) -> bool {
        self.shell_pid.is_none()
    }
}

/// Errors produced by [`ProcessObserverPort`].
#[derive(Debug, thiserror::Error)]
pub enum ProcessObserverError {
    /// The host platform is not supported by any wired-in adapter.
    #[error("os process observation is not supported on this platform")]
    Unsupported,

    /// I/O failure while talking to the OS (e.g., `ps` failed,
    /// `/proc` could not be read).
    #[error("os process observation io failure: {detail}")]
    Io { detail: String },

    /// Output from the OS could not be parsed.
    #[error("os process observation parse failure: {detail}")]
    Parse { detail: String },
}

/// Port: read-only OS-level inspection of a pane's process tree.
pub trait ProcessObserverPort {
    /// Inspect the OS state for the given tmux pane.
    ///
    /// `Ok(Some(...))` — observer found a foreground process and
    /// returns a [`PaneObservation`] with `High` confidence.
    ///
    /// `Ok(None)` — observer ran successfully but could not
    /// determine a foreground process (e.g., empty pane).
    ///
    /// `Err(...)` — adapter failed and the merge policy should treat
    /// the OS layer as silent (and log the error).
    fn observe(
        &self,
        lookup: &PaneProcessLookup,
    ) -> Result<Option<PaneObservation>, ProcessObserverError>;

    /// Batched variant: inspect many panes in a single OS call where
    /// possible.
    ///
    /// The default implementation simply loops [`observe`], which is
    /// O(panes) external process spawns. Adapters that can do better
    /// (e.g., a single `ps` invocation followed by a per-pane lookup
    /// in memory) MUST override this method so `pane list` does not
    /// pay O(panes) `ps` cost on every invocation.
    ///
    /// The returned vector is parallel to `lookups` (same length, same
    /// order). Per-pane errors collapse to `Ok(None)` for that index;
    /// adapter-wide errors (e.g., `Unsupported`) propagate as `Err`.
    fn observe_many(
        &self,
        lookups: &[PaneProcessLookup],
    ) -> Result<Vec<Option<PaneObservation>>, ProcessObserverError> {
        let mut out = Vec::with_capacity(lookups.len());
        for lookup in lookups {
            match self.observe(lookup) {
                Ok(opt) => out.push(opt),
                Err(ProcessObserverError::Unsupported) => {
                    return Err(ProcessObserverError::Unsupported);
                }
                Err(other) => {
                    tracing::warn!(error = %other, "observe_many: per-pane failure; collapsing to None");
                    out.push(None);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_empty_when_shell_pid_missing() {
        assert!(
            PaneProcessLookup {
                tty: None,
                shell_pid: None,
            }
            .is_empty()
        );

        assert!(
            PaneProcessLookup {
                tty: Some("   ".into()),
                shell_pid: None,
            }
            .is_empty(),
            "PID-first: tty alone is not enough"
        );

        assert!(
            !PaneProcessLookup {
                tty: None,
                shell_pid: Some(42),
            }
            .is_empty(),
            "PID-first: shell_pid alone is sufficient"
        );

        assert!(
            PaneProcessLookup {
                tty: Some("/dev/ttys003".into()),
                shell_pid: None,
            }
            .is_empty(),
            "PID-first: tty without shell_pid is unusable"
        );
    }
}

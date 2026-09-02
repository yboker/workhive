//! Pane supervisor port — Workhive's authoritative metadata source
//! for processes it launched.
//!
//! The supervisor port provides the highest-confidence observation
//! source: Workhive itself launched the process, so the binary, pid,
//! and exit status are known facts rather than heuristics.
//!
//! ## Boundary
//!
//! This port intentionally exposes only an *observation* surface.
//! Spawning, waiting on, and reaping supervised children belongs in
//! `workhive-infra` (the supervisor binary). Use cases consume this
//! port as a read-only signal to feed
//! [`crate::usecases::pane_observation`].
//!
//! ## Truthful states
//!
//! Adapters MUST:
//!
//! * Return `Ok(None)` when no supervisor metadata exists for the
//!   given (group, workspace, unit_id) tuple — the merge policy then
//!   falls through to lower-confidence observers.
//! * Never invent a supervised process when none exists.
//! * Never coerce `state_hint` to `Running` for an unknown pid.

use crate::domain::{PaneObservation, UnitId};

/// Errors produced by [`PaneSupervisorPort`].
#[derive(Debug, thiserror::Error)]
pub enum PaneSupervisorError {
    /// I/O error while reading supervisor state.
    #[error("supervisor metadata io failure: {detail}")]
    Io { detail: String },

    /// On-disk metadata could not be parsed; treat as "no observation"
    /// and surface a warning so users can recover.
    #[error("supervisor metadata is corrupt: {detail}")]
    Corrupt { detail: String },
}

/// Port: read-only access to Workhive's supervisor metadata.
///
/// One adapter implementation per host (today: a JSON file under
/// `<project>/.workhive/runtime/.../pane-supervisor/<unit>.json`).
pub trait PaneSupervisorPort {
    /// Look up the latest authoritative observation for a single
    /// pane unit.
    ///
    /// Returns `Ok(None)` when no supervisor metadata exists. The
    /// merge policy interprets this as "the supervisor layer has no
    /// information; defer to lower-confidence sources".
    fn observe(
        &self,
        group: &str,
        workspace: &str,
        unit_id: &UnitId,
    ) -> Result<Option<PaneObservation>, PaneSupervisorError>;
}

//! Append-only event store port for the activity control plane.
//!
//! This port is **distinct** from [`PaneActivityStorePort`] (the
//! latest-fact store): the latest-fact store records the most
//! recent fact per pane and overwrites prior facts; this store
//! appends each normalized [`ActivityEvent`] and never rewrites
//! history. The two stores are kept side by side because they
//! serve different consumers:
//!
//! - the latest-fact store backs today's `pane list` rendering
//!   (read-model-shaped, fast point lookup);
//! - this event store is the durable source the
//!   [`crate::usecases::activity_control_plane::StateProjector`]
//!   folds into the new
//!   [`crate::domain::activity::ReadModel`].
//!
//! The append contract:
//!
//! - **Per-pane order ([`ActivityEventStore::events_for_pane`])**: events
//!   for a given pane MUST be returned in append order. The store
//!   may use any internal mechanism (per-pane log, single global log
//!   + filter, ...) but the public iterator order is fixed.
//! - **Replay-all order ([`ActivityEventStore::replay_all`])**: events
//!   across all panes MUST be returned in deterministic
//!   `(observed_at_unix_ms, id)` order so the
//!   [`crate::usecases::activity_control_plane::StateProjector`]
//!   produces the same read model from any append history. This
//!   contract is binding for Phase 1: do not substitute append
//!   sequence or per-pane append order at the global level. Both
//!   the in-memory testkit fake and the file-backed infra adapter
//!   MUST honor this ordering so their projected read models are
//!   bit-for-bit identical.
//! - **No overwrite**: once an event is appended, the store may
//!   never silently rewrite or delete it. A separate compaction
//!   policy (out of scope for Phase 1) would surface that as a
//!   distinct typed operation.
//! - **Typed errors**: I/O failures must surface as
//!   [`ActivityEventStoreError::Io`]; corrupt entries surface as
//!   [`ActivityEventStoreError::CorruptEntry`] so callers can decide
//!   to skip vs degrade. `unwrap()`/panic is forbidden.
//!
//! [`PaneActivityStorePort`]: super::PaneActivityStorePort
//! [`ActivityEvent`]: crate::domain::activity::ActivityEvent

use thiserror::Error;

use crate::domain::activity::{ActivityEvent, PaneCorrelationKey};

/// Errors surfaced by the append-only activity event store.
#[derive(Debug, Error)]
pub enum ActivityEventStoreError {
    /// Filesystem / backing-store I/O failed.
    #[error("activity event store I/O failed: {0}")]
    Io(String),
    /// One or more entries on the wire could not be deserialized.
    /// Carries a human-readable detail; callers MUST surface this
    /// as a typed degraded outcome instead of panicking.
    #[error("activity event store entry corrupt: {0}")]
    CorruptEntry(String),
    /// The entire store contents are missing or unreachable in a
    /// way the projector cannot recover from. Distinct from
    /// `Io` to let callers render `Unavailable` rather than
    /// "I/O error".
    #[error("activity event store unavailable: {0}")]
    Unavailable(String),
}

/// Append-only port for normalized activity events.
pub trait ActivityEventStore {
    /// Append `event` to the store. The event's `id` is preserved
    /// (the projector relies on it for traceability).
    fn append(&self, event: &ActivityEvent) -> Result<(), ActivityEventStoreError>;

    /// Return all events for a single pane in append order.
    ///
    /// Returning `Ok(empty)` means "store is healthy and the pane
    /// has no events yet". Returning
    /// [`ActivityEventStoreError::Unavailable`] means "store is
    /// unreachable for this pane in a way that is not just
    /// missing data". Corrupt-entry errors must NOT silently
    /// downgrade to an empty list; callers must see the typed
    /// error.
    fn events_for_pane(
        &self,
        pane: &PaneCorrelationKey,
    ) -> Result<Vec<ActivityEvent>, ActivityEventStoreError>;

    /// Return all events across every pane in deterministic
    /// `(observed_at_unix_ms, id)` order. Used by the projector to
    /// rebuild the entire read model from scratch.
    ///
    /// Implementations MUST sort the global stream by
    /// `(observed_at_unix_ms, id)` so that
    /// [`crate::usecases::activity_control_plane::project_read_model`]
    /// produces the same per-pane fold regardless of which adapter
    /// served the events. The in-memory fake and the file-backed
    /// adapter share this contract; any new adapter inherits it.
    fn replay_all(&self) -> Result<Vec<ActivityEvent>, ActivityEventStoreError>;
}

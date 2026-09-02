//! Pane registry, observer, and messaging ports.
//!
//! These ports define the interfaces for pane lifecycle management,
//! state observation, and inter-pane communication.

use crate::domain::{
    ObservedPaneState, PaneCapability, PaneDescriptor, PaneEvent, PaneLifecycleEvent, PaneMessage,
    PaneRuntimeId, PaneStateChange,
};

/// Errors from pane registry and messaging operations.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("pane not found in registry: {id}")]
    PaneNotFound { id: String },

    #[error("payload type not supported by current transport: {payload_type}")]
    PayloadNotSupported { payload_type: String },

    #[error("registry error: {message}")]
    Internal { message: String },
}

/// Result of an upsert operation on the registry.
#[derive(Debug, Clone)]
pub struct RegistryDelta {
    /// Internal reconciliation diffs produced by the upsert.
    pub changes: Vec<PaneStateChange>,
}

/// Maintains a unified runtime view of all panes.
///
/// The registry is the single source of truth for pane identity, lifecycle
/// status, and capability declarations. Capabilities are **derived** from
/// observation facts during reconciliation — there are no manual
/// grant/revoke operations.
///
/// ## Phase 6 contract
///
/// The primary mutation path is [`upsert`](Self::upsert), which accepts a
/// [`PaneDescriptor`] and either inserts or updates the pane in the registry.
/// The runtime observation path is [`update_from_observation`](Self::update_from_observation),
/// which accepts an [`ObservedPaneState`] and reconciles it into the registry.
///
/// The [`drain_events`](Self::drain_events) method provides the lifecycle
/// event stream for external consumers.
pub trait PaneRegistryPort {
    /// Insert or update a pane descriptor in the registry.
    ///
    /// If the pane is new, it is registered and a `Registered` lifecycle event
    /// is emitted. If it already exists, observation-derived fields are updated.
    fn upsert(&self, descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError>;

    /// Remove a pane from the registry by its runtime ID.
    fn remove(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError>;

    /// Look up a pane by its runtime ID.
    fn get(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError>;

    /// List all registered panes for a specific workspace.
    fn list_workspace(&self, workspace: &str) -> Result<Vec<PaneDescriptor>, RegistryError>;

    /// List all registered panes, optionally filtered by workspace.
    fn list(&self, workspace: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError>;

    /// Drain all pending lifecycle events since the last drain.
    ///
    /// This is destructive — events are removed from the buffer once returned.
    fn drain_events(&self) -> Result<Vec<PaneLifecycleEvent>, RegistryError>;

    /// Reconcile a runtime observation into the registry.
    ///
    /// If the pane identified by the observation is new, it is registered.
    /// If it already exists, observation-derived fields (cwd, command, kind,
    /// state) are updated and capabilities are re-derived.
    ///
    /// Returns internal [`PaneStateChange`] diffs. The caller (sync logic)
    /// translates these into public [`PaneEvent`] values.
    fn update_from_observation(
        &self,
        observed: ObservedPaneState,
    ) -> Result<Vec<PaneStateChange>, RegistryError>;

    /// Find panes that have a specific capability.
    fn find_by_capability(
        &self,
        capability: PaneCapability,
    ) -> Result<Vec<PaneDescriptor>, RegistryError>;
}

/// Passive event buffer for pane lifecycle events.
///
/// The observer accumulates [`PaneEvent`] values pushed by sync logic.
/// External consumers (monitors, schedulers, CLI debug tools) drain the
/// buffer via [`poll_events`](Self::poll_events).
///
/// ## Sync contract
///
/// - Sync logic calls [`push_event`](Self::push_event) to mirror each
///   event into the observer. This is a **write-only** operation from
///   sync's perspective.
/// - Sync must **never** call `poll_events`. The sync function returns
///   its own `Vec<PaneEvent>` as the authoritative output.
/// - `poll_events` is reserved for external consumers and is destructive
///   (drains the buffer).
pub trait PaneObserverPort {
    /// Push a single event into the observer buffer.
    fn push_event(&self, event: PaneEvent);

    /// Drain all pending events since the last poll.
    ///
    /// This is destructive — events are removed from the buffer once
    /// returned. Reserved for external consumers only.
    fn poll_events(&self) -> Vec<PaneEvent>;
}

/// Send messages between panes or from the control plane to a pane.
///
/// Message routing resolves the target pane's routing context (group,
/// workspace, unit_id) via the registry, then delegates actual delivery
/// to the runtime adapter.
pub trait PaneMessagePort {
    /// Send a message to a target pane.
    fn send(&self, message: PaneMessage) -> Result<(), RegistryError>;
}

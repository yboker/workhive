//! Pane domain model.
//!
//! Identity newtypes live here; classification/state types live in
//! [`state`]; the runtime registry/eventing skeleton lives in
//! [`runtime`]. Everything is re-exported flat so callers keep using
//! `crate::domain::pane::*` paths.

use std::fmt;

use serde::{Deserialize, Serialize};

mod runtime;
mod state;

pub use runtime::*;
pub use state::*;

/// Backend-specific pane identifier (e.g., tmux `%0`, `%1`).
///
/// Lives only in the adapter/infra layer. Use [`UnitId`] for
/// runtime-neutral references across port boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneId(String);

impl PaneId {
    /// Create a new pane ID from a raw backend-specific identifier.
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Runtime-neutral identifier for a unit (pane/shell) within a workspace.
///
/// Unlike [`PaneId`] (which is backend-specific), `UnitId` is the primary
/// key used by runtime port interfaces. Adapters map between `UnitId` and
/// their backend-specific identifiers internally.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnitId(String);

impl UnitId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<PaneId> for UnitId {
    fn from(pane_id: PaneId) -> Self {
        Self(pane_id.0)
    }
}

impl From<&PaneId> for UnitId {
    fn from(pane_id: &PaneId) -> Self {
        Self(pane_id.0.clone())
    }
}

#[cfg(test)]
mod tests;

//! Activity control plane Phase-1 skeleton.
//!
//! This module is the durable architecture skeleton for Workhive's
//! event-sourced AI activity intelligence layer. It is **separate**
//! from the older `domain::ai_activity` (latest-fact overwrite store)
//! and `domain::pane::PaneActivityState` (process-level enum) modules
//! by design:
//!
//! - `domain::ai_activity` records the most recent provider fact per
//!   pane and is read by today's `pane list` rendering. It is a
//!   read-model-shaped store, not an event log.
//! - `domain::pane::PaneActivityState` is the process-level
//!   classification (`ShellIdle`, `AgentForeground`, ...) used by
//!   the supervisor / OS observer / tmux fallback stack.
//! - This `domain::activity` module is the **Phase 1 control plane
//!   vocabulary** the requirement requires: provider-neutral signal
//!   channels, raw observations, normalized append-only activity
//!   events, projected per-pane facts, and the derived current
//!   `PaneActivityState` read model with the full required
//!   vocabulary (`Working`, `Completed`, `Idle`, `Stale`, `Disabled`,
//!   `Unsupported`, `Degraded`, ...).
//!
//! The Phase-1 type with the same identifier (`PaneActivityState`)
//! intentionally lives behind the qualified `domain::activity::`
//! path so it does not collide with `domain::PaneActivityState`
//! (the process-level enum) at the bare re-export level. Consumers
//! that need the new control-plane vocabulary import it as
//! `workhive_core::domain::activity::PaneActivityState`.

mod confidence;
mod event;
mod fact;
mod kinds;
mod observation;
mod read_model;
mod state;

pub use confidence::{ObservationConfidence, SignalFreshness};
pub use event::{ActivityEvent, ActivityEventId, ActivityEventKind, ActivityOutcome};
pub use fact::PaneActivityFact;
pub use kinds::{PaneCorrelationKey, ProviderKind, ProviderSignalSource, SignalChannel};
pub use observation::{Observation, ObservationPayload, ProviderEvent, ProviderEventKind};
pub use read_model::{PaneActivityReadModel, ReadModel};
pub use state::PaneActivityState;

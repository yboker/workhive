//! Activity control plane use cases.
//!
//! This module wires together the Phase-1 building blocks:
//!
//! - [`provider_policy`]: per-provider capability matrix and event
//!   interpretation policy (Claude Code, Codex, OpenCode).
//! - [`normalizer`]: pure function that turns a raw provider
//!   [`Observation`] into a normalized [`ActivityEvent`].
//! - [`projector`]: folds appended events into the
//!   [`ReadModel`].
//! - [`decision_policy`]: gates orchestration / notification /
//!   metrics actions on confidence, freshness, and disabled state.
//!
//! All four are pure-logic where feasible and depend only on the
//! `domain::activity` types and the
//! [`crate::ports::ActivityEventStore`] port for storage.
//!
//! [`Observation`]: crate::domain::activity::Observation
//! [`ActivityEvent`]: crate::domain::activity::ActivityEvent
//! [`ReadModel`]: crate::domain::activity::ReadModel

pub mod decision_policy;
pub mod metrics;
pub mod normalizer;
pub mod overlay_bridge;
pub mod projector;
pub mod provider_payload_bridge;
pub mod provider_policy;

pub use decision_policy::{
    DecisionPolicy, DefaultDecisionPolicy, FeatureGate, OrchestrationDecision, OrchestrationRefusal,
};
pub use metrics::{
    FreshnessThresholds, MetricValue, MetricsError, PaneActivityMetrics, PartialReason,
    UnavailableReason, compute_pane_metrics,
};
pub use normalizer::{NormalizationError, normalize};
pub use overlay_bridge::{
    PaneActivityOverlayProjection, project_pane_fact_to_overlay, should_acp_drive,
};
pub use projector::{StateProjector, project_pane_events, project_read_model};
pub use provider_payload_bridge::{
    channel_for, confidence_for, event_kind_for, observation_from_legacy_input, provider_kind_for,
    source_for,
};
pub use provider_policy::{
    ClaudeCodePolicy, CodexPolicy, OpenCodePolicy, ProviderActivityCapability,
    ProviderActivityPolicy, ProviderCapabilityMatrix, policy_for,
};

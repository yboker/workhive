//! Normalized Workhive activity event vocabulary.
//!
//! `ActivityEvent` is the **append-only** event the
//! [`crate::ports::ActivityEventStore`] persists. It is what the
//! [`crate::usecases::activity_control_plane::StateProjector`]
//! folds to derive the current pane / session read model.
//!
//! Compare to [`crate::domain::activity::ProviderEvent`]:
//!
//! - `ProviderEvent`: provider-shaped vocabulary; what the provider
//!   said in its own terms.
//! - `ActivityEvent`: Workhive-shaped vocabulary; what we recorded
//!   as the canonical interpretation, after normalization. Two
//!   different providers may map to the same `ActivityEventKind`,
//!   and the same provider event may map to different
//!   `ActivityEventKind`s depending on signal channel and detail.
//!
//! `ActivityEvent` is **never** rewritten in place. The store
//! appends; older events are kept until a separate compaction policy
//! decides otherwise.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::confidence::ObservationConfidence;
use super::kinds::{PaneCorrelationKey, ProviderKind, ProviderSignalSource, SignalChannel};

/// Globally unique identifier for an [`ActivityEvent`]. Generated
/// by the store at append time and returned to callers so they can
/// correlate later projection results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ActivityEventId(pub Uuid);

impl ActivityEventId {
    pub fn new_v4() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Normalized event vocabulary.
///
/// Variant set covers the requirement-required normalized lifecycle
/// vocabulary plus telemetry / lifecycle outcome arms used by the
/// projector to derive `Stale`, `Disabled`, `Unsupported`, and
/// `Degraded` states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityEventKind {
    /// User submitted a prompt.
    PromptSubmitted,
    /// Provider started executing a tool on the user's behalf.
    ToolStarted,
    /// Provider finished executing a tool.
    ToolFinished,
    /// Permission is required from the user.
    PermissionRequested,
    /// User resolved (approved/denied) a permission request.
    PermissionResolved,
    /// Provider reported turn completion at lifecycle confidence
    /// (hook-attested). Drives orchestration.
    TurnCompleted,
    /// Provider reported turn completion at notify-channel
    /// (medium) confidence. Workhive accepts the completion fact
    /// but refuses to derive working/tool_running from earlier
    /// points in the turn.
    TurnNotifiedComplete,
    /// Provider reported a turn failure.
    TurnFailed,
    /// Provider reports session became idle.
    SessionIdle,
    /// Provider reports session was started.
    SessionStarted,
    /// Provider telemetry has gone stale (plugin disconnect, hook
    /// not firing, ...).
    TelemetryStale,
    /// Provider integration is disabled by feature gate. Recorded
    /// once per pane on the disabled boundary; the projector uses
    /// this to derive `PaneActivityState::Disabled` without
    /// reading any other channel.
    IntegrationDisabled,
    /// Provider integration is unsupported on this platform/version.
    /// Recorded once when the adapter detects unsupported state;
    /// drives `PaneActivityState::Unsupported`.
    IntegrationUnsupported,
    /// Provider integration could not prepare/verify/correlate.
    /// Drives `PaneActivityState::Degraded`.
    IntegrationDegraded,
}

/// Outcome lens used by some events that carry a final state
/// (succeeded vs failed). Stored alongside the event so the
/// projector does not have to parse `detail` strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityOutcome {
    Success,
    Failure,
    /// Outcome is unknown or not applicable for this event kind.
    /// Used by `ToolStarted`, `PromptSubmitted`, and friends.
    NotApplicable,
}

/// A normalized activity event ready for the append-only store.
///
/// Equality and hashing intentionally include the event id, so
/// adjacency tests and ordering tests can treat each appended event
/// as distinct.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Unique id assigned at append time.
    pub id: ActivityEventId,
    /// Pane the event belongs to.
    pub pane: PaneCorrelationKey,
    /// Provider that produced the underlying signal.
    pub provider: ProviderKind,
    /// Delivery channel used for the underlying signal.
    pub channel: SignalChannel,
    /// OS / runtime source of the underlying signal.
    pub source: ProviderSignalSource,
    /// Event kind in Workhive normalized vocabulary.
    pub kind: ActivityEventKind,
    /// Outcome lens (success / failure / N/A).
    pub outcome: ActivityOutcome,
    /// Per-event observation confidence as graded at normalization
    /// time.
    pub confidence: ObservationConfidence,
    /// Wall-clock observation time in Unix milliseconds.
    pub observed_at_unix_ms: i64,
    /// Optional display-safe detail (tool name, exit code, error
    /// message). Must contain no secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ActivityEvent {
    /// Construct an event with a fresh id.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pane: PaneCorrelationKey,
        provider: ProviderKind,
        channel: SignalChannel,
        source: ProviderSignalSource,
        kind: ActivityEventKind,
        outcome: ActivityOutcome,
        confidence: ObservationConfidence,
        observed_at_unix_ms: i64,
        detail: Option<String>,
    ) -> Self {
        Self {
            id: ActivityEventId::new_v4(),
            pane,
            provider,
            channel,
            source,
            kind,
            outcome,
            confidence,
            observed_at_unix_ms,
            detail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    #[test]
    fn event_round_trips_through_serde_with_optional_detail() {
        let mut e = ActivityEvent::new(
            pane(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            1_700_000_000_000,
            None,
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: ActivityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);

        e.detail = Some("bash".to_owned());
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"detail\":\"bash\""));
        let back: ActivityEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }

    /// Pinning test: notify completion has its own normalized event
    /// kind. The store and projector must distinguish hook-attested
    /// completion from notify-attested completion.
    #[test]
    fn notify_and_hook_completions_are_distinct_normalized_kinds() {
        assert_ne!(
            ActivityEventKind::TurnNotifiedComplete,
            ActivityEventKind::TurnCompleted,
        );
        assert_ne!(
            ActivityEventKind::TurnNotifiedComplete,
            ActivityEventKind::ToolFinished,
        );
    }

    #[test]
    fn integration_lifecycle_event_kinds_exist_and_serialise() {
        for k in [
            ActivityEventKind::IntegrationDisabled,
            ActivityEventKind::IntegrationUnsupported,
            ActivityEventKind::IntegrationDegraded,
        ] {
            let json = serde_json::to_string(&k).unwrap();
            let back: ActivityEventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, k);
        }
    }

    #[test]
    fn event_id_is_unique_per_construction() {
        let a = ActivityEventId::new_v4();
        let b = ActivityEventId::new_v4();
        assert_ne!(a, b, "two fresh ids must be distinct");
    }
}

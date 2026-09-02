//! Raw provider observations and typed provider events.
//!
//! These types sit **between** the provider adapter (which owns the
//! wire format) and the normalizer (which produces normalized
//! `ActivityEvent`s). They are intentionally provider-agnostic in
//! shape but provider-tagged at the value level so the normalizer
//! can route on `ProviderKind` without re-parsing strings.

use serde::{Deserialize, Serialize};

use super::ObservationConfidence;
use super::kinds::{PaneCorrelationKey, ProviderKind, ProviderSignalSource, SignalChannel};

/// A raw observation as Workhive received it from a provider
/// channel. The payload is intentionally opaque at this layer:
/// provider adapters convert their wire format into one of the
/// [`ProviderEventKind`] variants below before reaching the
/// normalizer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// Which provider produced the observation.
    pub provider: ProviderKind,
    /// Through which delivery channel Workhive received it.
    pub channel: SignalChannel,
    /// Coarse OS / runtime source — paired with [`Self::channel`]
    /// so diagnostics can render an honest "we read X via Y" line.
    pub source: ProviderSignalSource,
    /// Pane this observation belongs to. The correlation key is
    /// supplied by the adapter that received the signal; the
    /// normalizer trusts it.
    pub pane: PaneCorrelationKey,
    /// Wall-clock time at which Workhive received the observation,
    /// in Unix milliseconds.
    pub observed_at_unix_ms: i64,
    /// Provider-attributed confidence at the moment of observation.
    /// The normalizer/projector layers may downgrade this further,
    /// but never upgrade it.
    pub confidence: ObservationConfidence,
    /// The typed provider event payload.
    pub payload: ObservationPayload,
}

/// Either a typed provider event (well-formed payload the adapter
/// understood) or an opaque telemetry record (e.g. for diagnostics).
///
/// The two-arm shape lets adapters retain raw breadcrumbs without
/// forcing them through normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "payload", rename_all = "snake_case")]
pub enum ObservationPayload {
    /// A recognised provider event the normalizer can map to a
    /// normalized [`super::ActivityEvent`].
    ProviderEvent(ProviderEvent),
    /// An opaque telemetry record. The normalizer ignores these for
    /// projection purposes; diagnostics may still render them.
    Telemetry { description: String },
}

/// A typed provider event ready for normalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderEvent {
    /// The provider event kind. Provider-shaped vocabulary, NOT
    /// the Workhive activity vocabulary.
    pub kind: ProviderEventKind,
    /// Optional adapter-provided detail (tool name, exit code,
    /// error message, ...). Display-safe, no secrets.
    pub detail: Option<String>,
}

/// Provider-shaped event vocabulary.
///
/// Variants are deliberately **provider-agnostic**: every supported
/// provider produces these named events through its own adapter.
/// This is the boundary between "what the provider said" and
/// "what Workhive considers happened" (modeled by the normalized
/// [`super::ActivityEventKind`]).
///
/// Critical invariants pinned by the requirement:
///
/// - `NotifyCompletion` is the **only** provider-event vocabulary
///   that maps from Codex `notify` callbacks. Codex `notify` MUST
///   NOT produce `PromptSubmitted`, `ToolStarted`, or any other
///   "agent is working" event variant; it only signals turn
///   completion.
/// - `SessionIdle` and `SessionStarted` are session-scope events
///   distinct from per-turn lifecycle events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderEventKind {
    /// User submitted a new prompt.
    PromptSubmitted,
    /// Provider started executing a tool on the user's behalf.
    ToolStarted,
    /// Provider finished executing a tool.
    ToolFinished,
    /// Provider needs user approval before proceeding.
    PermissionRequested,
    /// User answered the permission request (approve/deny).
    PermissionResolved,
    /// The latest turn finished successfully (e.g. lifecycle
    /// `Stop` hook).
    TurnCompleted,
    /// The latest turn finished with an error.
    TurnFailed,
    /// Notify-channel: a turn completed. Does NOT imply working
    /// state for any earlier point in the turn — it is a coarse
    /// completion signal only.
    NotifyCompletion,
    /// Session was started by the provider (initial handshake,
    /// new chat, ...).
    SessionStarted,
    /// Provider reports the session went idle.
    SessionIdle,
    /// Provider reports the session telemetry has gone stale or
    /// degraded (e.g. plugin lost connection).
    TelemetryStale,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    fn observation(
        provider: ProviderKind,
        channel: SignalChannel,
        source: ProviderSignalSource,
        kind: ProviderEventKind,
        confidence: ObservationConfidence,
    ) -> Observation {
        Observation {
            provider,
            channel,
            source,
            pane: pane(),
            observed_at_unix_ms: 1_700_000_000_000,
            confidence,
            payload: ObservationPayload::ProviderEvent(ProviderEvent { kind, detail: None }),
        }
    }

    #[test]
    fn observation_round_trips_through_serde() {
        let obs = observation(
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::ToolStarted,
            ObservationConfidence::High,
        );
        let json = serde_json::to_string(&obs).unwrap();
        let back: Observation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, obs);
    }

    #[test]
    fn telemetry_payload_round_trips() {
        let obs = Observation {
            provider: ProviderKind::OpenCode,
            channel: SignalChannel::PluginEvent,
            source: ProviderSignalSource::Plugin,
            pane: pane(),
            observed_at_unix_ms: 0,
            confidence: ObservationConfidence::Low,
            payload: ObservationPayload::Telemetry {
                description: "plugin disconnect".to_owned(),
            },
        };
        let json = serde_json::to_string(&obs).unwrap();
        let back: Observation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, obs);
    }

    /// Pinning test for the Phase-1 invariant:
    /// `NotifyCompletion` is its own provider-event variant — it
    /// must not be aliased into `TurnCompleted` (which models a
    /// hook-attested completion) or `ToolFinished`.
    #[test]
    fn notify_completion_is_a_distinct_provider_event_kind() {
        let notify = ProviderEventKind::NotifyCompletion;
        assert_ne!(notify, ProviderEventKind::TurnCompleted);
        assert_ne!(notify, ProviderEventKind::ToolFinished);
        assert_ne!(notify, ProviderEventKind::PromptSubmitted);
        assert_ne!(notify, ProviderEventKind::ToolStarted);
        assert_ne!(notify, ProviderEventKind::SessionIdle);
    }

    #[test]
    fn provider_event_kinds_round_trip_through_serde() {
        for k in [
            ProviderEventKind::PromptSubmitted,
            ProviderEventKind::ToolStarted,
            ProviderEventKind::ToolFinished,
            ProviderEventKind::PermissionRequested,
            ProviderEventKind::PermissionResolved,
            ProviderEventKind::TurnCompleted,
            ProviderEventKind::TurnFailed,
            ProviderEventKind::NotifyCompletion,
            ProviderEventKind::SessionStarted,
            ProviderEventKind::SessionIdle,
            ProviderEventKind::TelemetryStale,
        ] {
            let json = serde_json::to_string(&k).unwrap();
            let back: ProviderEventKind = serde_json::from_str(&json).unwrap();
            assert_eq!(back, k);
        }
    }
}

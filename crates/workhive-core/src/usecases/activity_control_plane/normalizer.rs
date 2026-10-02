//! Raw observation -> normalized [`ActivityEvent`] normalizer.
//!
//! The normalizer is the **only** place where provider-shaped event
//! vocabulary is translated into the Workhive activity vocabulary.
//! It enforces the Phase-1 invariants:
//!
//! - Every provider event passes through provider policy
//!   ([`crate::usecases::activity_control_plane::provider_policy`]).
//!   Unsupported (provider, channel, kind) tuples are rejected with
//!   [`NormalizationError::ChannelDoesNotCarryEvent`] — the caller
//!   surfaces this as `Degraded`/`Unsupported` integration state,
//!   not as silent drop.
//! - Codex `notify` callbacks may only produce
//!   [`ActivityEventKind::TurnNotifiedComplete`]; they are never
//!   normalized into `Working` / `ToolRunning`.
//! - The recorded confidence is the **min** of the policy's max
//!   for the (provider, channel) pair and the adapter-supplied
//!   confidence — adapters can downgrade but never upgrade trust.
//!
//! The normalizer is a pure function: it takes an [`Observation`]
//! and returns a `Result<ActivityEvent, NormalizationError>`. No
//! I/O, no clock reads — `observed_at_unix_ms` flows straight from
//! the observation.

use thiserror::Error;

use crate::domain::activity::{
    ActivityEvent, ActivityEventKind, ActivityOutcome, Observation, ObservationPayload,
    ProviderEventKind,
};

use super::provider_policy::{ProviderActivityCapability, ProviderActivityPolicy, policy_for};

/// Errors returned by [`normalize`]. Distinct variants so callers
/// can render typed degraded outcomes instead of swallowing
/// failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NormalizationError {
    /// Provider has no implemented adapter (e.g. `ProviderKind::Future`).
    #[error("provider {0:?} has no implemented activity policy")]
    UnsupportedProvider(crate::domain::activity::ProviderKind),
    /// The (provider, channel) pair is unsupported. Caller should
    /// degrade rather than store the event.
    #[error("provider {provider:?} does not support channel {channel:?}")]
    UnsupportedChannel {
        provider: crate::domain::activity::ProviderKind,
        channel: crate::domain::activity::SignalChannel,
    },
    /// Provider policy does not allow this event kind on this
    /// channel (e.g. Codex notify carrying ToolStarted). Strongest
    /// signal we have that the adapter sent an out-of-contract
    /// payload; surfaces as a separate variant so tests can assert
    /// the Phase-1 invariant explicitly.
    #[error(
        "provider {provider:?} channel {channel:?} does not carry {kind:?} (Phase-1 invariant)"
    )]
    ChannelDoesNotCarryEvent {
        provider: crate::domain::activity::ProviderKind,
        channel: crate::domain::activity::SignalChannel,
        kind: ProviderEventKind,
    },
    /// Observation carried a telemetry payload, not a provider
    /// event. Telemetry is intentionally not normalized into the
    /// event log; surfacing this as an error lets callers route
    /// the breadcrumb to diagnostics instead of silently dropping
    /// it.
    #[error("telemetry payload is not normalizable into ActivityEvent")]
    TelemetryNotNormalizable,
}

/// Translate a raw [`Observation`] into a normalized
/// [`ActivityEvent`].
///
/// On success the returned event has a fresh
/// [`crate::domain::activity::ActivityEventId`] (the event store
/// preserves it on append).
pub fn normalize(observation: &Observation) -> Result<ActivityEvent, NormalizationError> {
    let policy = policy_for(observation.provider);
    // Reject if the provider has no implementation at all (Future).
    if matches!(policy, super::provider_policy::AnyProviderPolicy::Future(_)) {
        return Err(NormalizationError::UnsupportedProvider(
            observation.provider,
        ));
    }

    // Reject unsupported channels for the provider.
    let cap = policy.capability_matrix().capability(observation.channel);
    let max_conf = match cap {
        ProviderActivityCapability::Supported(c) => c,
        ProviderActivityCapability::Unsupported => {
            return Err(NormalizationError::UnsupportedChannel {
                provider: observation.provider,
                channel: observation.channel,
            });
        }
    };

    // Telemetry payloads do not normalize into events; route them
    // to diagnostics instead.
    let provider_event = match &observation.payload {
        ObservationPayload::ProviderEvent(ev) => ev,
        ObservationPayload::Telemetry { .. } => {
            return Err(NormalizationError::TelemetryNotNormalizable);
        }
    };

    // Enforce the per-policy "this channel can't carry that kind"
    // contract. This is where the Codex-notify-completion-only
    // invariant lands.
    if !policy.allows_event(observation.channel, provider_event.kind) {
        return Err(NormalizationError::ChannelDoesNotCarryEvent {
            provider: observation.provider,
            channel: observation.channel,
            kind: provider_event.kind,
        });
    }

    // Map provider event kind -> normalized event kind + outcome.
    let (kind, outcome) = map_kind(observation.channel, provider_event.kind);

    // Confidence is the minimum of policy max and adapter-supplied
    // confidence. Adapters can downgrade, never upgrade.
    let confidence = observation.confidence.min(max_conf);

    Ok(ActivityEvent::new(
        observation.pane.clone(),
        observation.provider,
        observation.channel,
        observation.source,
        kind,
        outcome,
        confidence,
        observation.observed_at_unix_ms,
        provider_event.detail.clone(),
    ))
}

/// Map a provider event kind to its normalized event kind and
/// outcome lens.
///
/// Special cases:
///
/// - `NotifyCompletion` always maps to `TurnNotifiedComplete`.
///   By Phase-1 invariant (enforced in
///   [`super::provider_policy::ProviderActivityPolicy::allows_event`])
///   it is channel-exclusive to `SignalChannel::NotifyCallback`, so
///   the mapping is unambiguous: a non-notify channel that delivered
///   `NotifyCompletion` would already have been rejected with
///   `NormalizationError::ChannelDoesNotCarryEvent`.
/// - Lifecycle/plugin completion uses `ProviderEventKind::TurnCompleted`,
///   which maps to `ActivityEventKind::TurnCompleted`.
/// - `TurnFailed` carries `ActivityOutcome::Failure`.
/// - Hook-attested `TurnCompleted` carries `Success`.
fn map_kind(
    channel: crate::domain::activity::SignalChannel,
    kind: ProviderEventKind,
) -> (ActivityEventKind, ActivityOutcome) {
    use ActivityOutcome::*;
    use ProviderEventKind as P;
    match (channel, kind) {
        (_, P::PromptSubmitted) => (ActivityEventKind::PromptSubmitted, NotApplicable),
        (_, P::ToolStarted) => (ActivityEventKind::ToolStarted, NotApplicable),
        (_, P::ToolFinished) => (ActivityEventKind::ToolFinished, Success),
        (_, P::PermissionRequested) => (ActivityEventKind::PermissionRequested, NotApplicable),
        (_, P::PermissionResolved) => (ActivityEventKind::PermissionResolved, NotApplicable),
        (_, P::TurnCompleted) => (ActivityEventKind::TurnCompleted, Success),
        (_, P::TurnFailed) => (ActivityEventKind::TurnFailed, Failure),
        (_, P::NotifyCompletion) => (ActivityEventKind::TurnNotifiedComplete, Success),
        (_, P::SessionStarted) => (ActivityEventKind::SessionStarted, NotApplicable),
        (_, P::SessionIdle) => (ActivityEventKind::SessionIdle, NotApplicable),
        (_, P::TelemetryStale) => (ActivityEventKind::TelemetryStale, NotApplicable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ObservationConfidence, PaneCorrelationKey, ProviderEvent, ProviderKind,
        ProviderSignalSource, SignalChannel,
    };

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    fn obs(
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

    /// Phase-1 invariant: Codex notify completion normalizes to
    /// `TurnNotifiedComplete`, *not* `Working` / `ToolRunning`.
    #[test]
    fn codex_notify_completion_maps_to_turn_notified_complete_not_working() {
        let o = obs(
            ProviderKind::Codex,
            SignalChannel::NotifyCallback,
            ProviderSignalSource::Notify,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::Medium,
        );
        let e = normalize(&o).expect("notify completion must normalize");
        assert_eq!(
            e.kind,
            ActivityEventKind::TurnNotifiedComplete,
            "Codex notify completion must map to TurnNotifiedComplete (Phase-1 invariant)"
        );
        assert_ne!(
            e.kind,
            ActivityEventKind::ToolStarted,
            "Codex notify completion must NEVER imply ToolStarted"
        );
        assert_eq!(e.confidence, ObservationConfidence::Medium);
        assert_eq!(e.outcome, ActivityOutcome::Success);
    }

    /// Phase-1 invariant: Codex notify channel rejects every event
    /// kind that is not `NotifyCompletion`.
    #[test]
    fn codex_notify_channel_rejects_non_completion_kinds() {
        let o = obs(
            ProviderKind::Codex,
            SignalChannel::NotifyCallback,
            ProviderSignalSource::Notify,
            ProviderEventKind::ToolStarted,
            ObservationConfidence::Medium,
        );
        let err = normalize(&o).expect_err("must reject");
        assert!(
            matches!(
                err,
                NormalizationError::ChannelDoesNotCarryEvent {
                    provider: ProviderKind::Codex,
                    channel: SignalChannel::NotifyCallback,
                    kind: ProviderEventKind::ToolStarted,
                }
            ),
            "expected ChannelDoesNotCarryEvent, got {err:?}"
        );
    }

    /// Lifecycle hook events from Claude Code map to high-confidence
    /// normalized events.
    #[test]
    fn claude_code_lifecycle_hook_tool_started_normalizes_to_high_confidence_tool_started() {
        let o = obs(
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::ToolStarted,
            ObservationConfidence::High,
        );
        let e = normalize(&o).expect("lifecycle hook must normalize");
        assert_eq!(e.kind, ActivityEventKind::ToolStarted);
        assert_eq!(e.confidence, ObservationConfidence::High);
        assert_eq!(e.outcome, ActivityOutcome::NotApplicable);
    }

    /// Plugin events from OpenCode map to high-confidence normalized
    /// events.
    #[test]
    fn opencode_plugin_event_turn_completed_normalizes_to_high_confidence_turn_completed() {
        let o = obs(
            ProviderKind::OpenCode,
            SignalChannel::PluginEvent,
            ProviderSignalSource::Plugin,
            ProviderEventKind::TurnCompleted,
            ObservationConfidence::High,
        );
        let e = normalize(&o).expect("plugin event must normalize");
        assert_eq!(e.kind, ActivityEventKind::TurnCompleted);
        assert_eq!(e.confidence, ObservationConfidence::High);
        assert_eq!(e.outcome, ActivityOutcome::Success);
    }

    /// Adapter-supplied confidence above the policy max is clamped
    /// to the policy max.
    #[test]
    fn adapter_confidence_above_policy_max_is_clamped_down() {
        let o = obs(
            ProviderKind::Codex,
            SignalChannel::NotifyCallback,
            ProviderSignalSource::Notify,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::High,
        );
        let e = normalize(&o).expect("notify completion must normalize");
        assert_eq!(
            e.confidence,
            ObservationConfidence::Medium,
            "Codex notify caps at Medium; adapter cannot upgrade to High"
        );
    }

    /// Adapter-supplied confidence below the policy max is preserved.
    /// Adapters can downgrade trust (e.g. corrupt detail field).
    #[test]
    fn adapter_confidence_below_policy_max_is_preserved() {
        let o = obs(
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::ToolStarted,
            ObservationConfidence::Low,
        );
        let e = normalize(&o).expect("normalize");
        assert_eq!(
            e.confidence,
            ObservationConfidence::Low,
            "adapter is allowed to downgrade confidence below the policy max"
        );
    }

    /// Unsupported (provider, channel) pair returns the typed
    /// error so callers can degrade.
    #[test]
    fn unsupported_channel_returns_typed_unsupported_channel_error() {
        let o = obs(
            ProviderKind::ClaudeCode,
            SignalChannel::NotifyCallback,
            ProviderSignalSource::Notify,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::Medium,
        );
        let err = normalize(&o).expect_err("must be unsupported");
        assert!(
            matches!(
                err,
                NormalizationError::UnsupportedChannel {
                    provider: ProviderKind::ClaudeCode,
                    channel: SignalChannel::NotifyCallback,
                }
            ),
            "expected UnsupportedChannel, got {err:?}"
        );
    }

    /// Future provider rejects everything.
    #[test]
    fn future_provider_returns_typed_unsupported_provider_error() {
        let o = obs(
            ProviderKind::Future,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::ToolStarted,
            ObservationConfidence::High,
        );
        let err = normalize(&o).expect_err("future has no policy");
        assert!(
            matches!(
                err,
                NormalizationError::UnsupportedProvider(ProviderKind::Future)
            ),
            "expected UnsupportedProvider(Future), got {err:?}"
        );
    }

    /// Telemetry payloads are not normalized; they are surfaced as
    /// a typed error so callers can route them to diagnostics.
    #[test]
    fn telemetry_payload_is_not_normalized() {
        let o = Observation {
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
        let err = normalize(&o).expect_err("telemetry must not normalize");
        assert!(matches!(err, NormalizationError::TelemetryNotNormalizable));
    }

    /// Channel-exclusivity invariant: `NotifyCompletion`
    /// is channel-exclusive to `SignalChannel::NotifyCallback`.
    /// Codex lifecycle hook delivering `NotifyCompletion` must be
    /// rejected with the typed `ChannelDoesNotCarryEvent` so the
    /// projector cannot derive notify-shaped state from a hook
    /// channel. Lifecycle/plugin completion must use
    /// `ProviderEventKind::TurnCompleted` instead.
    #[test]
    fn codex_lifecycle_hook_notify_completion_is_rejected_as_channel_mismatch() {
        let o = obs(
            ProviderKind::Codex,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::High,
        );
        let err = normalize(&o).expect_err("hook channel must NOT carry NotifyCompletion");
        assert!(
            matches!(
                err,
                NormalizationError::ChannelDoesNotCarryEvent {
                    provider: ProviderKind::Codex,
                    channel: SignalChannel::LifecycleHook,
                    kind: ProviderEventKind::NotifyCompletion,
                }
            ),
            "expected ChannelDoesNotCarryEvent for Codex lifecycle hook + NotifyCompletion, got {err:?}"
        );
    }

    /// Channel-exclusivity invariant: Claude Code lifecycle
    /// hook delivering `NotifyCompletion` must be rejected — Claude
    /// has no notify-equivalent and lifecycle completion must use
    /// `ProviderEventKind::TurnCompleted`.
    #[test]
    fn claude_code_lifecycle_hook_notify_completion_is_rejected_as_channel_mismatch() {
        let o = obs(
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::High,
        );
        let err = normalize(&o).expect_err("Claude hook must NOT carry NotifyCompletion");
        assert!(
            matches!(
                err,
                NormalizationError::ChannelDoesNotCarryEvent {
                    provider: ProviderKind::ClaudeCode,
                    channel: SignalChannel::LifecycleHook,
                    kind: ProviderEventKind::NotifyCompletion,
                }
            ),
            "expected ChannelDoesNotCarryEvent for Claude lifecycle hook + NotifyCompletion, got {err:?}"
        );
    }

    /// Channel-exclusivity invariant: OpenCode plugin
    /// channel delivering `NotifyCompletion` must be rejected.
    /// OpenCode's primary high-confidence channel is plugin events,
    /// which must use `ProviderEventKind::TurnCompleted` for
    /// completion semantics — never the notify-shaped variant.
    #[test]
    fn opencode_plugin_event_notify_completion_is_rejected_as_channel_mismatch() {
        let o = obs(
            ProviderKind::OpenCode,
            SignalChannel::PluginEvent,
            ProviderSignalSource::Plugin,
            ProviderEventKind::NotifyCompletion,
            ObservationConfidence::High,
        );
        let err = normalize(&o).expect_err("plugin channel must NOT carry NotifyCompletion");
        assert!(
            matches!(
                err,
                NormalizationError::ChannelDoesNotCarryEvent {
                    provider: ProviderKind::OpenCode,
                    channel: SignalChannel::PluginEvent,
                    kind: ProviderEventKind::NotifyCompletion,
                }
            ),
            "expected ChannelDoesNotCarryEvent for OpenCode plugin + NotifyCompletion, got {err:?}"
        );
    }

    /// Detail string round-trips through normalization.
    #[test]
    fn detail_string_is_preserved_in_normalized_event() {
        let o = Observation {
            provider: ProviderKind::ClaudeCode,
            channel: SignalChannel::LifecycleHook,
            source: ProviderSignalSource::LifecycleHook,
            pane: pane(),
            observed_at_unix_ms: 1,
            confidence: ObservationConfidence::High,
            payload: ObservationPayload::ProviderEvent(ProviderEvent {
                kind: ProviderEventKind::ToolStarted,
                detail: Some("bash".to_owned()),
            }),
        };
        let e = normalize(&o).unwrap();
        assert_eq!(e.detail.as_deref(), Some("bash"));
    }
}

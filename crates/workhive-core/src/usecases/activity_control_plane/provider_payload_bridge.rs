//! Legacy `ActivityRecordInput` -> ACP `Observation` bridge.
//!
//! This module is the **only** place that translates the legacy
//! [`crate::usecases::ai_activity::ActivityRecordInput`] payload
//! shape into a typed [`Observation`] for the activity control
//! plane. It exists as a sibling helper to keep
//! [`super::normalizer`] free of provider-specific event-name
//! decoding: the normalizer continues to consume `Observation`
//! values produced here.
//!
//! Boundaries:
//!
//! - The bridge is **pure**. No I/O, no clock reads, no config.
//! - Unknown / unsupported event names map to `None` so the caller
//!   silently skips ACP append (matching the legacy hook contract
//!   which already treats unmapped event names as silent no-ops).
//! - Provider identity is taken from the trusted env tuple via the
//!   `provider` argument; the input's `provider` wire string is
//!   *not* trusted to drive routing — callers run the legacy
//!   [`crate::usecases::ai_activity::record_activity`] validator
//!   first which rejects identity drift.
//! - The mapping table is the frozen provider-payload table,
//!   not the legacy `(state, confidence)` table. The two tables
//!   intentionally diverge (e.g. `PostToolUse` maps to
//!   `ToolFinished` here but `Processing` in legacy) because ACP
//!   captures discrete lifecycle events while legacy captures
//!   the running session state.

use crate::domain::AiActivityProvider;
use crate::domain::activity::{
    Observation, ObservationConfidence, ObservationPayload, PaneCorrelationKey, ProviderEvent,
    ProviderEventKind, ProviderKind, ProviderSignalSource, SignalChannel,
};
use crate::usecases::ai_activity::ActivityRecordInput;

/// Translate a legacy `ActivityRecordInput` into a typed
/// [`Observation`].
///
/// Returns `None` when:
///
/// - the supplied `provider` is `AiActivityProvider::None` (no
///   provider is active for the pane);
/// - the payload's wire provider disagrees with the trusted
///   `provider` argument (caller should already have rejected via
///   [`crate::usecases::ai_activity::record_activity`], but we
///   defend in depth so a future caller cannot bypass the check);
/// - the event name is not in the Phase 3 mapping table for the
///   trusted provider (the hook script silently dropped these
///   already; ACP must mirror that contract instead of introducing
///   a new error variant).
///
/// On `Some`, the returned `Observation` is ready for
/// [`super::normalize`]. The detail field is propagated verbatim
/// from `input.reason` (already display-safe per legacy adapter
/// rules).
pub fn observation_from_legacy_input(
    input: &ActivityRecordInput,
    provider: AiActivityProvider,
) -> Option<Observation> {
    let provider_kind = provider_kind_for(provider)?;
    // Defense in depth: refuse if the payload's wire provider
    // disagrees with the trusted argument. The CLI hook handler
    // already validates this through `record_activity`, but the
    // pure bridge must not assume that.
    let claimed = AiActivityProvider::from_wire(&input.provider)?;
    if claimed != provider {
        return None;
    }
    let event_kind = event_kind_for(provider, &input.event)?;
    let channel = channel_for(provider_kind);
    let source = source_for(provider_kind);
    let confidence = confidence_for(provider, &input.event);
    let pane = PaneCorrelationKey {
        group: input.group.clone(),
        workspace: input.workspace.clone(),
        unit_id: input.unit_id.clone(),
    };
    Some(Observation {
        provider: provider_kind,
        channel,
        source,
        pane,
        observed_at_unix_ms: input.observed_at_unix_ms,
        confidence,
        payload: ObservationPayload::ProviderEvent(ProviderEvent {
            kind: event_kind,
            detail: input.reason.clone(),
        }),
    })
}

/// Project the legacy provider enum (which carries a `None`
/// sentinel for "no integration") onto the strict control-plane
/// [`ProviderKind`]. Returns `None` for `AiActivityProvider::None`.
pub fn provider_kind_for(provider: AiActivityProvider) -> Option<ProviderKind> {
    ProviderKind::try_from(provider).ok()
}

/// Determine the high-confidence [`SignalChannel`] each integrated
/// provider uses to deliver real-time lifecycle/plugin signals.
///
/// `ProviderKind::ClaudeCode` and `ProviderKind::Codex` deliver
/// signals via lifecycle hooks; `ProviderKind::OpenCode` via
/// in-process plugin events. `ProviderKind::Future` has no
/// implemented adapter and is rejected later by the normalizer;
/// we tag it as `LifecycleHook` here so this function stays
/// total and the normalizer remains the single rejection point.
pub fn channel_for(provider: ProviderKind) -> SignalChannel {
    match provider {
        ProviderKind::ClaudeCode | ProviderKind::Codex => SignalChannel::LifecycleHook,
        ProviderKind::OpenCode => SignalChannel::PluginEvent,
        ProviderKind::Future => SignalChannel::LifecycleHook,
    }
}

/// Determine the OS / runtime [`ProviderSignalSource`] paired
/// with [`channel_for`]. Diagnostics renders both together.
pub fn source_for(provider: ProviderKind) -> ProviderSignalSource {
    match provider {
        ProviderKind::ClaudeCode | ProviderKind::Codex => ProviderSignalSource::LifecycleHook,
        ProviderKind::OpenCode => ProviderSignalSource::Plugin,
        ProviderKind::Future => ProviderSignalSource::LifecycleHook,
    }
}

/// Map a provider-specific event name to a control-plane
/// [`ProviderEventKind`].
///
/// Returns `None` for unknown event names. The mapping table is
/// frozen:
///
/// | Provider   | Event                | Kind                  |
/// |------------|----------------------|-----------------------|
/// | ClaudeCode | UserPromptSubmit     | PromptSubmitted       |
/// | ClaudeCode | PreToolUse           | ToolStarted           |
/// | ClaudeCode | PostToolUse          | ToolFinished          |
/// | ClaudeCode | PermissionRequest    | PermissionRequested   |
/// | ClaudeCode | Stop                 | TurnCompleted         |
/// | ClaudeCode | StopFailure          | TurnFailed            |
/// | Codex      | SessionStart         | SessionStarted        |
/// | Codex      | UserPromptSubmit     | PromptSubmitted       |
/// | Codex      | PreToolUse           | ToolStarted           |
/// | Codex      | PostToolUse          | ToolFinished          |
/// | Codex      | PermissionRequest    | PermissionRequested   |
/// | Codex      | Stop                 | TurnCompleted         |
/// | OpenCode   | session.created      | SessionStarted        |
/// | OpenCode   | session.idle         | SessionIdle           |
/// | OpenCode   | session.error        | TurnFailed            |
/// | OpenCode   | permission.asked     | PermissionRequested   |
/// | OpenCode   | permission.replied   | PermissionResolved    |
/// | OpenCode   | tool.execute.before  | ToolStarted           |
/// | OpenCode   | tool.execute.after   | ToolFinished          |
///
/// Notes:
///
/// - `Stop` from both Claude and Codex maps to
///   `ProviderEventKind::TurnCompleted`, **never**
///   `NotifyCompletion`. The notify-channel variant is reserved
///   for `SignalChannel::NotifyCallback` (Codex notify) and is
///   not produced by this bridge.
/// - The table is intentionally different from the legacy
///   `map_*_event` tables which decide `(state, confidence)` for
///   the legacy latest-fact store.
pub fn event_kind_for(provider: AiActivityProvider, event: &str) -> Option<ProviderEventKind> {
    use ProviderEventKind as K;
    match provider {
        AiActivityProvider::ClaudeCode => match event {
            "UserPromptSubmit" => Some(K::PromptSubmitted),
            "PreToolUse" => Some(K::ToolStarted),
            "PostToolUse" => Some(K::ToolFinished),
            "PermissionRequest" => Some(K::PermissionRequested),
            "Stop" => Some(K::TurnCompleted),
            "StopFailure" => Some(K::TurnFailed),
            _ => None,
        },
        AiActivityProvider::Codex => match event {
            "SessionStart" => Some(K::SessionStarted),
            "UserPromptSubmit" => Some(K::PromptSubmitted),
            "PreToolUse" => Some(K::ToolStarted),
            "PostToolUse" => Some(K::ToolFinished),
            "PermissionRequest" => Some(K::PermissionRequested),
            "Stop" => Some(K::TurnCompleted),
            _ => None,
        },
        AiActivityProvider::Opencode => match event {
            "session.created" => Some(K::SessionStarted),
            "session.idle" => Some(K::SessionIdle),
            "session.error" => Some(K::TurnFailed),
            "permission.asked" => Some(K::PermissionRequested),
            "permission.replied" => Some(K::PermissionResolved),
            "tool.execute.before" => Some(K::ToolStarted),
            "tool.execute.after" => Some(K::ToolFinished),
            _ => None,
        },
        AiActivityProvider::None => None,
    }
}

/// Adapter-supplied [`ObservationConfidence`] for the bridge.
///
/// Lifecycle hook / plugin events use `High` — the normalizer
/// will clamp per the provider policy cap. Codex `SessionStart`
/// is a best-effort session-scope signal whose schema can evolve
/// across tool versions; we tag it `MediumLow` so the provider
/// policy can downgrade further without ever upgrading.
pub fn confidence_for(provider: AiActivityProvider, event: &str) -> ObservationConfidence {
    match (provider, event) {
        (AiActivityProvider::Codex, "SessionStart") => ObservationConfidence::MediumLow,
        _ => ObservationConfidence::High,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AI_ACTIVITY_SCHEMA_VERSION, AiActivityConfidence, AiActivityState};

    fn input(
        provider: AiActivityProvider,
        event: &str,
        reason: Option<&str>,
    ) -> ActivityRecordInput {
        ActivityRecordInput {
            schema_version: AI_ACTIVITY_SCHEMA_VERSION,
            provider: provider.wire_name().to_owned(),
            event: event.to_owned(),
            tool_name: "tool".to_owned(),
            group: "g".to_owned(),
            workspace: "w".to_owned(),
            unit_id: "u".to_owned(),
            state: AiActivityState::Processing,
            confidence: AiActivityConfidence::Hook,
            observed_at_unix_ms: 1_700_000_000_000,
            reason: reason.map(str::to_owned),
        }
    }

    /// Mapping row 1: Claude `UserPromptSubmit` -> lifecycle-hook
    /// `PromptSubmitted` at High confidence.
    #[test]
    fn claude_user_prompt_submit_maps_to_lifecycle_prompt_submitted() {
        let inp = input(AiActivityProvider::ClaudeCode, "UserPromptSubmit", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        assert_eq!(obs.provider, ProviderKind::ClaudeCode);
        assert_eq!(obs.channel, SignalChannel::LifecycleHook);
        assert_eq!(obs.source, ProviderSignalSource::LifecycleHook);
        assert_eq!(obs.confidence, ObservationConfidence::High);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::PromptSubmitted);
                assert_eq!(ev.detail, None);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Mapping row 2: Claude `PreToolUse` with tool name -> ToolStarted
    /// carrying the tool name as detail.
    #[test]
    fn claude_pre_tool_use_carries_tool_name_as_detail() {
        let inp = input(AiActivityProvider::ClaudeCode, "PreToolUse", Some("Bash"));
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::ToolStarted);
                assert_eq!(ev.detail.as_deref(), Some("Bash"));
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Mapping row 3: Claude `PostToolUse` -> ToolFinished. The
    /// ACP table intentionally diverges from the legacy latest-fact
    /// table (which uses `Processing`).
    #[test]
    fn claude_post_tool_use_maps_to_tool_finished() {
        let inp = input(AiActivityProvider::ClaudeCode, "PostToolUse", Some("Bash"));
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::ToolFinished);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Mapping row 4: Claude `PermissionRequest` -> PermissionRequested.
    #[test]
    fn claude_permission_request_maps_to_permission_requested() {
        let inp = input(AiActivityProvider::ClaudeCode, "PermissionRequest", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::PermissionRequested);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Phase-1 invariant guard: Claude `Stop` MUST produce
    /// `TurnCompleted`, NEVER `NotifyCompletion`. The bridge is
    /// the closest place a regression could leak in.
    #[test]
    fn claude_stop_produces_turn_completed_never_notify_completion() {
        let inp = input(AiActivityProvider::ClaudeCode, "Stop", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        assert_eq!(obs.channel, SignalChannel::LifecycleHook);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::TurnCompleted);
                assert_ne!(
                    ev.kind,
                    ProviderEventKind::NotifyCompletion,
                    "lifecycle Stop must not produce NotifyCompletion"
                );
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Mapping row 6: Claude `StopFailure` -> TurnFailed.
    #[test]
    fn claude_stop_failure_maps_to_turn_failed() {
        let inp = input(AiActivityProvider::ClaudeCode, "StopFailure", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::TurnFailed);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Codex `SessionStart` lands at `MediumLow` confidence so
    /// provider policy can downgrade further without ever upgrading.
    #[test]
    fn codex_session_start_uses_medium_low_confidence() {
        let inp = input(AiActivityProvider::Codex, "SessionStart", None);
        let obs = observation_from_legacy_input(&inp, AiActivityProvider::Codex).expect("must map");
        assert_eq!(obs.confidence, ObservationConfidence::MediumLow);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::SessionStarted);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Codex `PreToolUse` lifecycle-channel event.
    #[test]
    fn codex_pre_tool_use_maps_to_lifecycle_tool_started() {
        let inp = input(AiActivityProvider::Codex, "PreToolUse", Some("git_status"));
        let obs = observation_from_legacy_input(&inp, AiActivityProvider::Codex).expect("must map");
        assert_eq!(obs.provider, ProviderKind::Codex);
        assert_eq!(obs.channel, SignalChannel::LifecycleHook);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::ToolStarted);
                assert_eq!(ev.detail.as_deref(), Some("git_status"));
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Codex `Stop` MUST land on the lifecycle channel as
    /// `TurnCompleted`, NEVER on the notify channel as
    /// `NotifyCompletion`. This is the bridge-side guard for the
    /// same Phase-1 invariant the normalizer rejects at its end.
    #[test]
    fn codex_stop_via_lifecycle_hook_is_turn_completed_not_notify() {
        let inp = input(AiActivityProvider::Codex, "Stop", None);
        let obs = observation_from_legacy_input(&inp, AiActivityProvider::Codex).expect("must map");
        assert_eq!(obs.channel, SignalChannel::LifecycleHook);
        assert_ne!(obs.channel, SignalChannel::NotifyCallback);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::TurnCompleted);
                assert_ne!(ev.kind, ProviderEventKind::NotifyCompletion);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// OpenCode plugin event landing on the plugin channel/source.
    #[test]
    fn opencode_tool_execute_before_maps_to_plugin_tool_started() {
        let inp = input(
            AiActivityProvider::Opencode,
            "tool.execute.before",
            Some("edit"),
        );
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::Opencode).expect("must map");
        assert_eq!(obs.provider, ProviderKind::OpenCode);
        assert_eq!(obs.channel, SignalChannel::PluginEvent);
        assert_eq!(obs.source, ProviderSignalSource::Plugin);
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::ToolStarted);
                assert_eq!(ev.detail.as_deref(), Some("edit"));
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// OpenCode `session.idle` -> SessionIdle.
    #[test]
    fn opencode_session_idle_maps_to_session_idle() {
        let inp = input(AiActivityProvider::Opencode, "session.idle", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::Opencode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::SessionIdle);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// OpenCode `session.error` -> TurnFailed.
    #[test]
    fn opencode_session_error_maps_to_turn_failed() {
        let inp = input(AiActivityProvider::Opencode, "session.error", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::Opencode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::TurnFailed);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// OpenCode `permission.replied` -> PermissionResolved.
    #[test]
    fn opencode_permission_replied_maps_to_permission_resolved() {
        let inp = input(AiActivityProvider::Opencode, "permission.replied", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::Opencode).expect("must map");
        match obs.payload {
            ObservationPayload::ProviderEvent(ev) => {
                assert_eq!(ev.kind, ProviderEventKind::PermissionResolved);
            }
            _ => panic!("expected ProviderEvent"),
        }
    }

    /// Provider mismatch between the trusted argument and the
    /// payload's wire provider returns `None` — the caller will
    /// already reject through `record_activity`, but the bridge
    /// must defend in depth.
    #[test]
    fn provider_mismatch_between_trust_and_payload_returns_none() {
        let mut inp = input(AiActivityProvider::ClaudeCode, "UserPromptSubmit", None);
        inp.provider = AiActivityProvider::Codex.wire_name().to_owned();
        assert!(observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).is_none());
    }

    /// `AiActivityProvider::None` cannot produce an observation.
    #[test]
    fn none_provider_returns_none() {
        let inp = input(AiActivityProvider::ClaudeCode, "UserPromptSubmit", None);
        assert!(observation_from_legacy_input(&inp, AiActivityProvider::None).is_none());
    }

    /// Unknown event names silently return `None` so the caller
    /// can skip the ACP append without introducing a new error
    /// variant.
    #[test]
    fn unknown_event_name_returns_none() {
        let inp = input(AiActivityProvider::ClaudeCode, "TotallyFakeEvent", None);
        assert!(observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).is_none());
    }

    /// Unknown wire provider string in the payload returns `None`
    /// rather than panicking — the bridge stays total.
    #[test]
    fn unknown_payload_wire_provider_returns_none() {
        let mut inp = input(AiActivityProvider::ClaudeCode, "UserPromptSubmit", None);
        inp.provider = "definitely-not-real".to_owned();
        assert!(observation_from_legacy_input(&inp, AiActivityProvider::ClaudeCode).is_none());
    }

    /// Pane correlation key is taken verbatim from the input
    /// tuple (which the CLI handler already pinned to the trusted
    /// env tuple before calling the bridge).
    #[test]
    fn pane_correlation_key_is_taken_from_input_tuple() {
        let inp = input(AiActivityProvider::Opencode, "session.idle", None);
        let obs =
            observation_from_legacy_input(&inp, AiActivityProvider::Opencode).expect("must map");
        assert_eq!(
            obs.pane,
            PaneCorrelationKey {
                group: "g".to_owned(),
                workspace: "w".to_owned(),
                unit_id: "u".to_owned(),
            }
        );
    }

    /// `observed_at_unix_ms` flows through unchanged.
    #[test]
    fn observed_at_flows_through_unchanged() {
        let inp = input(AiActivityProvider::Codex, "UserPromptSubmit", None);
        let obs = observation_from_legacy_input(&inp, AiActivityProvider::Codex).expect("must map");
        assert_eq!(obs.observed_at_unix_ms, 1_700_000_000_000);
    }
}

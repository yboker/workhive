//! Pure bridge between the ACP read model and the legacy
//! `AiActivityOverlayEntry` consumed by `pane list` / `pane show`.
//!
//! The CLI renderer still uses the legacy
//! [`crate::domain::AiActivityFact`] / [`crate::domain::AiActivityState`]
//! vocabulary because every i18n key, every column header, and every
//! source/accuracy/guidance line is anchored on that vocabulary.
//! The activity control plane projects events into a richer
//! [`PaneActivityFact`] (provider + state + confidence + channel +
//! freshness). This module maps a high-confidence ACP fact onto the
//! legacy vocabulary the renderer already speaks, so the CLI can opt
//! into the ACP read path without inventing new i18n keys or changing
//! the public column layout.
//!
//! The mapping is intentionally narrow:
//!
//! - Only `Fresh` facts are mapped. Stale / expired / unavailable
//!   facts fall through to the legacy latest-fact overlay so the
//!   existing stale-hint logic keeps working.
//! - Only `High` confidence in combination with one of the
//!   orchestration-eligible states drives the overlay. The single
//!   exception is the Codex `notify`-completion path
//!   (`channel = NotifyCallback`, `state = Completed`,
//!   `confidence = Medium`), which is allowed to render a Completed
//!   overlay but MUST NEVER render Working / ToolRunning.
//! - Provider mapping uses the legacy [`AiActivityProvider`] enum;
//!   [`ProviderKind::Future`] returns `None` so unrecognised
//!   providers degrade silently to the legacy overlay.
//! - Source attribution uses the legacy [`AiActivitySource`] enum.
//!   The medium-tier notify completion deliberately attributes to
//!   `WorkhiveSupervisor` rather than fabricating a "notify" source
//!   that no legacy i18n key exists for.
//!
//! Everything in this module is pure (no I/O, no globals, no clock
//! lookup) so the bridge is unit-testable from the CLI tests *and*
//! from core-level tests using the in-memory event store fake.

use crate::domain::activity::{
    PaneActivityFact, PaneActivityState, ProviderKind, SignalChannel, SignalFreshness,
};
use crate::domain::{AiActivityConfidence, AiActivityProvider, AiActivitySource, AiActivityState};

/// Resolved overlay attributes for an ACP-driven row.
///
/// The CLI assembles the user-facing `AiActivityOverlayEntry` by
/// localizing [`state_label_key`](Self::state_label_key) with the
/// active locale + i18n port, then attaching the typed
/// [`provider`](Self::provider), [`source`](Self::source), and
/// [`confidence`](Self::confidence) fields. Keeping the bridge in
/// the core crate (rather than carrying a localized string here)
/// lets us unit-test the mapping without instantiating a
/// `dyn I18nPort` and without coupling tests to specific localized
/// glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneActivityOverlayProjection {
    /// Legacy provider the renderer already knows how to label.
    pub provider: AiActivityProvider,
    /// Detection source rendered next to the AI activity overlay.
    pub source: AiActivitySource,
    /// Legacy confidence bucket used by the renderer to pick the
    /// "AI accuracy" line and the source-tag column.
    pub confidence: AiActivityConfidence,
    /// Pre-resolved i18n key for the legacy activity state label.
    pub state_label_key: &'static str,
    /// Typed AI activity state behind `state_label_key`. Carried so
    /// downstream consumers reasoning in the pane state-model
    /// vocabulary (e.g. concise `pane list` rendering) can resolve a
    /// concise label without parsing the localized string back into
    /// an enum.
    pub state: AiActivityState,
    /// `true` if the rendered overlay should display the stale hint.
    /// The bridge currently only fires for `Fresh` facts, so this is
    /// always `false`; the field exists so future callers can read
    /// `stale` without recomputing it.
    pub stale: bool,
}

/// Decide whether the projected fact is allowed to drive the
/// rendered overlay.
///
/// Returns `true` only when the fact is `Fresh` **and** one of:
///
/// 1. `confidence == High` and `state` is orchestration-eligible
///    (Working, ToolRunning, WaitingForPermission, Completed, Idle,
///    Failed); or
/// 2. `state == Completed && channel == NotifyCallback && confidence == Medium` —
///    the Codex `notify`-completion exception. This branch never
///    selects `Working` or `ToolRunning` because the projector pins
///    notify callbacks to `Completed`.
///
/// All other combinations return `false` so the caller falls back
/// to the legacy latest-fact overlay (which has its own stale-hint
/// logic).
pub fn should_acp_drive(fact: &PaneActivityFact) -> bool {
    if fact.freshness != SignalFreshness::Fresh {
        return false;
    }
    matches!(
        (fact.confidence, fact.state, fact.channel),
        (
            crate::domain::activity::ObservationConfidence::High,
            PaneActivityState::Working
                | PaneActivityState::ToolRunning
                | PaneActivityState::WaitingForPermission
                | PaneActivityState::Completed
                | PaneActivityState::Idle
                | PaneActivityState::Failed,
            _,
        ) | (
            crate::domain::activity::ObservationConfidence::Medium,
            PaneActivityState::Completed,
            SignalChannel::NotifyCallback,
        )
    )
}

/// Project an ACP [`PaneActivityFact`] onto the legacy overlay
/// vocabulary, or return `None` to let the caller use the legacy
/// latest-fact overlay path.
///
/// The mapping is pure and does no I/O.
///
/// Returns `None` for:
///
/// - facts that do not satisfy [`should_acp_drive`];
/// - facts whose `provider` is `None` or [`ProviderKind::Future`]
///   (we refuse to fabricate provider attribution);
/// - channel / confidence / provider combinations that lack a stable
///   legacy [`AiActivitySource`] mapping (e.g. `LifecycleHook` for
///   OpenCode, which only emits `PluginEvent` in production today,
///   or `PluginEvent` for Claude Code / Codex);
/// - states that the legacy [`AiActivityState`] vocabulary does not
///   model (`Stale`, `Disabled`, `Unsupported`, `Degraded`,
///   `Unknown`, `OpenUnverified`).
pub fn project_pane_fact_to_overlay(
    fact: &PaneActivityFact,
) -> Option<PaneActivityOverlayProjection> {
    if !should_acp_drive(fact) {
        return None;
    }
    let provider_kind = fact.provider?;
    let provider = match provider_kind {
        ProviderKind::ClaudeCode => AiActivityProvider::ClaudeCode,
        ProviderKind::Codex => AiActivityProvider::Codex,
        ProviderKind::OpenCode => AiActivityProvider::Opencode,
        // `Future` is a sentinel for "supported in principle, no
        // adapter wired up"; we never invent a label for it.
        ProviderKind::Future => return None,
    };
    let (source, confidence) = match (fact.channel, fact.confidence) {
        (SignalChannel::LifecycleHook, crate::domain::activity::ObservationConfidence::High) => {
            match provider {
                AiActivityProvider::ClaudeCode => {
                    (AiActivitySource::ClaudeCodeHook, AiActivityConfidence::Hook)
                }
                AiActivityProvider::Codex => {
                    (AiActivitySource::CodexHook, AiActivityConfidence::Hook)
                }
                // OpenCode does not emit lifecycle hooks in production
                // — events arrive via the plugin channel instead. Drop
                // to the legacy overlay rather than attributing the
                // wrong source.
                AiActivityProvider::Opencode | AiActivityProvider::None => return None,
            }
        }
        (SignalChannel::PluginEvent, crate::domain::activity::ObservationConfidence::High) => {
            match provider {
                AiActivityProvider::Opencode => (
                    AiActivitySource::OpencodePlugin,
                    AiActivityConfidence::Plugin,
                ),
                // Claude Code / Codex don't emit plugin events
                // today; refuse to attribute a plugin source for
                // them.
                _ => return None,
            }
        }
        (SignalChannel::NotifyCallback, crate::domain::activity::ObservationConfidence::Medium) => {
            // The medium-tier notify completion deliberately
            // surfaces as `WorkhiveSupervisor` rather than a
            // "notify" source. The legacy renderer already has
            // localized strings for `WorkhiveSupervisor`, and
            // attributing a notify completion to the supervisor is
            // faithful — the supervisor is what receives and
            // forwards the notify callback.
            (
                AiActivitySource::WorkhiveSupervisor,
                AiActivityConfidence::Fallback,
            )
        }
        // Other channel/confidence combinations (e.g. ObserverPoll,
        // SupervisorRecord) are not strong enough to drive the
        // overlay — let the legacy latest-fact path handle them.
        _ => return None,
    };
    let legacy_state = legacy_state_for(fact.state)?;
    Some(PaneActivityOverlayProjection {
        provider,
        source,
        confidence,
        state_label_key: legacy_state.i18n_key(),
        state: legacy_state,
        stale: false,
    })
}

/// Map an ACP [`PaneActivityState`] to a legacy
/// [`AiActivityState`].
///
/// Returns `None` for states the legacy vocabulary does not model
/// (Stale, Disabled, Unsupported, Degraded, Unknown, OpenUnverified)
/// so the caller falls back to the legacy overlay path. ACP must not
/// silently coerce non-busy states into busy ones; falling back keeps
/// the displayed activity faithful to the underlying signal.
fn legacy_state_for(state: PaneActivityState) -> Option<AiActivityState> {
    Some(match state {
        PaneActivityState::Working => AiActivityState::Processing,
        PaneActivityState::ToolRunning => AiActivityState::ToolRunning,
        PaneActivityState::WaitingForPermission => AiActivityState::WaitingForPermission,
        // Completed and Idle both surface as "waiting for the user
        // to respond" in the legacy vocabulary, which has no
        // dedicated `Completed` / `Idle` variants.
        PaneActivityState::Completed | PaneActivityState::Idle => AiActivityState::WaitingForUser,
        PaneActivityState::Failed => AiActivityState::Error,
        // States not modeled by the legacy vocabulary fall through.
        PaneActivityState::Unknown
        | PaneActivityState::OpenUnverified
        | PaneActivityState::Stale
        | PaneActivityState::Disabled
        | PaneActivityState::Unsupported
        | PaneActivityState::Degraded => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::activity::{
        ActivityOutcome, ObservationConfidence, PaneCorrelationKey, ProviderKind, SignalChannel,
        SignalFreshness,
    };
    use crate::i18n_keys;

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u")
    }

    fn base_fact(
        provider: ProviderKind,
        state: PaneActivityState,
        channel: SignalChannel,
        confidence: ObservationConfidence,
        freshness: SignalFreshness,
    ) -> PaneActivityFact {
        PaneActivityFact {
            pane: pane(),
            provider: Some(provider),
            state,
            outcome: ActivityOutcome::NotApplicable,
            confidence,
            channel,
            observed_at_unix_ms: 0,
            freshness,
            source_event_id: None,
        }
    }

    #[test]
    fn acp_high_confidence_lifecycle_hook_drives_overlay_state_and_source_for_claude_code() {
        let fact = base_fact(
            ProviderKind::ClaudeCode,
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let projection =
            project_pane_fact_to_overlay(&fact).expect("high-confidence lifecycle must project");
        assert_eq!(projection.provider, AiActivityProvider::ClaudeCode);
        assert_eq!(projection.source, AiActivitySource::ClaudeCodeHook);
        assert_eq!(projection.confidence, AiActivityConfidence::Hook);
        assert_eq!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_PROCESSING
        );
        assert!(!projection.stale, "Fresh facts must not render as stale");
    }

    #[test]
    fn acp_high_confidence_lifecycle_hook_drives_overlay_for_codex() {
        let fact = base_fact(
            ProviderKind::Codex,
            PaneActivityState::ToolRunning,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let projection = project_pane_fact_to_overlay(&fact).expect("Codex hook must project");
        assert_eq!(projection.provider, AiActivityProvider::Codex);
        assert_eq!(projection.source, AiActivitySource::CodexHook);
        assert_eq!(projection.confidence, AiActivityConfidence::Hook);
        assert_eq!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_TOOL_RUNNING
        );
    }

    #[test]
    fn acp_high_confidence_plugin_event_drives_overlay_for_opencode() {
        let fact = base_fact(
            ProviderKind::OpenCode,
            PaneActivityState::WaitingForPermission,
            SignalChannel::PluginEvent,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        let projection = project_pane_fact_to_overlay(&fact).expect("OpenCode plugin must project");
        assert_eq!(projection.provider, AiActivityProvider::Opencode);
        assert_eq!(projection.source, AiActivitySource::OpencodePlugin);
        assert_eq!(projection.confidence, AiActivityConfidence::Plugin);
        assert_eq!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_PERMISSION
        );
    }

    /// Locks the Codex `notify`-completion exception: medium-tier
    /// notify + Completed renders as a Completed-ish overlay
    /// (mapped to the legacy `WaitingForUser` key) and never as
    /// Working / ToolRunning. The source is `WorkhiveSupervisor`
    /// because the legacy vocabulary has no "notify" source.
    #[test]
    fn acp_notify_completion_renders_completed_never_working() {
        let fact = base_fact(
            ProviderKind::Codex,
            PaneActivityState::Completed,
            SignalChannel::NotifyCallback,
            ObservationConfidence::Medium,
            SignalFreshness::Fresh,
        );
        let projection =
            project_pane_fact_to_overlay(&fact).expect("notify completion must project");
        assert_eq!(projection.provider, AiActivityProvider::Codex);
        assert_eq!(projection.source, AiActivitySource::WorkhiveSupervisor);
        assert_eq!(projection.confidence, AiActivityConfidence::Fallback);
        assert_eq!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_USER,
            "notify completion must localize as Completed-ish (legacy WaitingForUser)"
        );
        assert_ne!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_PROCESSING,
            "notify completion must NEVER render as Processing/Working"
        );
        assert_ne!(
            projection.state_label_key,
            i18n_keys::PANE_AI_ACTIVITY_TOOL_RUNNING,
            "notify completion must NEVER render as ToolRunning"
        );
    }

    /// Codex notify-callback Working / ToolRunning is impossible
    /// per the projector — but lock the bridge as the second line
    /// of defense so any future projector regression is caught
    /// here too.
    #[test]
    fn acp_notify_callback_working_or_tool_running_is_dropped() {
        for busy in [PaneActivityState::Working, PaneActivityState::ToolRunning] {
            let fact = base_fact(
                ProviderKind::Codex,
                busy,
                SignalChannel::NotifyCallback,
                ObservationConfidence::Medium,
                SignalFreshness::Fresh,
            );
            assert!(
                project_pane_fact_to_overlay(&fact).is_none(),
                "{busy:?} on NotifyCallback must NEVER drive the overlay"
            );
        }
    }

    #[test]
    fn acp_stale_fact_falls_back_to_legacy_overlay() {
        let fact = base_fact(
            ProviderKind::ClaudeCode,
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Stale,
        );
        assert!(
            project_pane_fact_to_overlay(&fact).is_none(),
            "stale facts must fall back to the legacy stale-hint path",
        );
        assert!(!should_acp_drive(&fact));
    }

    #[test]
    fn acp_expired_or_unavailable_fact_falls_back_to_legacy() {
        for freshness in [SignalFreshness::Expired, SignalFreshness::Unavailable] {
            let fact = base_fact(
                ProviderKind::ClaudeCode,
                PaneActivityState::Working,
                SignalChannel::LifecycleHook,
                ObservationConfidence::High,
                freshness,
            );
            assert!(project_pane_fact_to_overlay(&fact).is_none());
            assert!(!should_acp_drive(&fact));
        }
    }

    #[test]
    fn acp_low_and_medium_low_confidence_fall_back_to_legacy() {
        for confidence in [
            ObservationConfidence::Low,
            ObservationConfidence::MediumLow,
            ObservationConfidence::None,
        ] {
            let fact = base_fact(
                ProviderKind::ClaudeCode,
                PaneActivityState::Working,
                SignalChannel::LifecycleHook,
                confidence,
                SignalFreshness::Fresh,
            );
            assert!(project_pane_fact_to_overlay(&fact).is_none());
            assert!(!should_acp_drive(&fact));
        }
    }

    #[test]
    fn acp_inactive_states_do_not_drive_overlay() {
        for state in [
            PaneActivityState::Unknown,
            PaneActivityState::OpenUnverified,
            PaneActivityState::Stale,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
        ] {
            let fact = base_fact(
                ProviderKind::ClaudeCode,
                state,
                SignalChannel::LifecycleHook,
                ObservationConfidence::High,
                SignalFreshness::Fresh,
            );
            assert!(
                project_pane_fact_to_overlay(&fact).is_none(),
                "state {state:?} must NOT drive ACP overlay"
            );
        }
    }

    #[test]
    fn acp_future_provider_falls_back_to_legacy() {
        let fact = base_fact(
            ProviderKind::Future,
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        assert!(
            project_pane_fact_to_overlay(&fact).is_none(),
            "Future is a sentinel — bridge must fall through to legacy",
        );
    }

    #[test]
    fn acp_missing_provider_falls_back_to_legacy() {
        let mut fact = base_fact(
            ProviderKind::ClaudeCode,
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        fact.provider = None;
        assert!(project_pane_fact_to_overlay(&fact).is_none());
    }

    #[test]
    fn acp_lifecycle_hook_for_opencode_does_not_attribute_to_plugin_or_hook() {
        // OpenCode emits PluginEvent in production. A LifecycleHook
        // event for OpenCode would be a wiring bug; the bridge
        // refuses to invent a source attribution for it.
        let fact = base_fact(
            ProviderKind::OpenCode,
            PaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        assert!(project_pane_fact_to_overlay(&fact).is_none());
    }

    #[test]
    fn acp_plugin_event_for_claude_code_or_codex_is_dropped() {
        for provider in [ProviderKind::ClaudeCode, ProviderKind::Codex] {
            let fact = base_fact(
                provider,
                PaneActivityState::Working,
                SignalChannel::PluginEvent,
                ObservationConfidence::High,
                SignalFreshness::Fresh,
            );
            assert!(
                project_pane_fact_to_overlay(&fact).is_none(),
                "PluginEvent must not attribute to hook providers",
            );
        }
    }

    #[test]
    fn acp_observer_poll_channel_never_drives_overlay() {
        let fact = base_fact(
            ProviderKind::ClaudeCode,
            PaneActivityState::Working,
            SignalChannel::ObserverPoll,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        assert!(
            project_pane_fact_to_overlay(&fact).is_none(),
            "ObserverPoll is not strong enough to drive the overlay",
        );
    }

    #[test]
    fn acp_supervisor_record_channel_never_drives_overlay() {
        let fact = base_fact(
            ProviderKind::ClaudeCode,
            PaneActivityState::Working,
            SignalChannel::SupervisorRecord,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        );
        assert!(project_pane_fact_to_overlay(&fact).is_none());
    }

    #[test]
    fn acp_completed_idle_failed_map_to_legacy_keys() {
        let cases = [
            (
                PaneActivityState::Completed,
                i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_USER,
            ),
            (
                PaneActivityState::Idle,
                i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_USER,
            ),
            (PaneActivityState::Failed, i18n_keys::PANE_AI_ACTIVITY_ERROR),
        ];
        for (state, expected_key) in cases {
            let fact = base_fact(
                ProviderKind::ClaudeCode,
                state,
                SignalChannel::LifecycleHook,
                ObservationConfidence::High,
                SignalFreshness::Fresh,
            );
            let projection = project_pane_fact_to_overlay(&fact)
                .unwrap_or_else(|| panic!("state {state:?} must project"));
            assert_eq!(projection.state_label_key, expected_key);
        }
    }
}

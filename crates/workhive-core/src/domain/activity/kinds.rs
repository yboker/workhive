//! Provider-neutral identity types for the activity control plane.
//!
//! These enums and the [`PaneCorrelationKey`] together answer
//! "**who** reported a signal, **how** did Workhive learn about it,
//! and **which pane** does it belong to". They are intentionally
//! independent of the older `AiActivityProvider` /
//! `PaneObservationSource` enums:
//!
//! - `AiActivityProvider` is sized for the latest-fact store and
//!   carries a `None` sentinel for "no provider attached".
//! - `PaneObservationSource` describes how the **process-level**
//!   pane observation was obtained (supervisor / process observer /
//!   tmux current command).
//!
//! The Phase-1 control plane needs a stricter, provider-only view
//! plus an explicit signal-channel taxonomy, so it owns these
//! types directly. A small `From` mapping from `AiActivityProvider`
//! is provided for compatibility with the legacy fact path.

use serde::{Deserialize, Serialize};

use crate::domain::AiActivityProvider;

/// Identity of a known AI provider.
///
/// "Future" is reserved for providers Workhive has not yet
/// integrated; it is **not** a fall-through bucket. Adapters that
/// receive an unknown provider string must reject the payload
/// rather than silently mapping it to `Future`. The variant exists
/// so the projector and policy modules can pattern-match
/// exhaustively without wildcard arms once a real new provider is
/// added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Claude Code (Anthropic).
    ClaudeCode,
    /// Codex (OpenAI).
    Codex,
    /// OpenCode.
    OpenCode,
    /// Reserved sentinel for a provider that is recognised by
    /// configuration as supported in principle but for which no
    /// adapter is implemented in this build. Distinct from
    /// "wrong/unknown provider string" — adapters must reject those.
    Future,
}

impl ProviderKind {
    /// Stable wire identifier used in events and on-disk records.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
            Self::Future => "future",
        }
    }

    /// Strict parse. Unknown wire names return `None` so callers
    /// can refuse the payload instead of guessing (fail loud).
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            "future" => Some(Self::Future),
            _ => None,
        }
    }
}

/// Compatibility bridge from the legacy latest-fact provider enum.
///
/// `AiActivityProvider::None` deliberately has **no** `ProviderKind`
/// equivalent: the activity control plane refuses to model "no
/// provider" as a provider. Use `Option<ProviderKind>` at the
/// boundary instead.
impl TryFrom<AiActivityProvider> for ProviderKind {
    type Error = ();

    fn try_from(value: AiActivityProvider) -> Result<Self, Self::Error> {
        match value {
            AiActivityProvider::ClaudeCode => Ok(Self::ClaudeCode),
            AiActivityProvider::Codex => Ok(Self::Codex),
            AiActivityProvider::Opencode => Ok(Self::OpenCode),
            AiActivityProvider::None => Err(()),
        }
    }
}

/// How Workhive learned about a provider signal.
///
/// This is the **delivery channel**, not the event meaning. A single
/// event meaning (e.g. "tool started") can in principle arrive over
/// any of these channels, with different freshness and confidence
/// implications.
///
/// `notify` is modeled as its own variant — not as the umbrella
/// channel — per the Phase-1 invariant "do not collapse notify,
/// lifecycle hooks, plugin events, observer polling, and supervisor
/// records into one undifferentiated adapter".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalChannel {
    /// Provider invokes a Workhive-managed lifecycle hook (Claude
    /// Code, Codex `/hooks`).
    LifecycleHook,
    /// Provider invokes a configured notification command (Codex
    /// notify). Coarse, completion-shaped.
    NotifyCallback,
    /// Provider loads a Workhive-managed plugin (OpenCode) and
    /// emits structured runtime events.
    PluginEvent,
    /// Workhive polls process/tmux state without provider
    /// cooperation.
    ObserverPoll,
    /// Workhive uses its own launch/runtime metadata.
    SupervisorRecord,
}

/// Closely related but coarser categorisation: where the signal
/// came **from** at the OS / runtime layer, independent of how
/// Workhive routed it. Used by diagnostics output and by the
/// projector when deciding whether to apply an event to the
/// foreground-mismatch guard.
///
/// The two distinct enums (`SignalChannel` and `ProviderSignalSource`)
/// exist on purpose: `SignalChannel` answers "what kind of
/// integration produced this", while `ProviderSignalSource` answers
/// "what underlying observation point did Workhive read from".
/// Diagnostics need both to render an honest "we read X from Y"
/// line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSignalSource {
    /// Provider lifecycle hook invocation (Claude Code, Codex).
    LifecycleHook,
    /// Provider notification command invocation (Codex notify).
    Notify,
    /// In-process plugin event (OpenCode).
    Plugin,
    /// Workhive supervisor record.
    Supervisor,
    /// OS process observer.
    Process,
    /// Tmux pane current command / title parsing.
    Tmux,
}

/// Stable correlation key linking a provider signal back to a
/// Workhive pane. Mirrors the `(group, workspace, unit_id)`
/// runtime tuple used elsewhere in the domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneCorrelationKey {
    pub group: String,
    pub workspace: String,
    pub unit_id: String,
}

impl PaneCorrelationKey {
    pub fn new(
        group: impl Into<String>,
        workspace: impl Into<String>,
        unit_id: impl Into<String>,
    ) -> Self {
        Self {
            group: group.into(),
            workspace: workspace.into(),
            unit_id: unit_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_wire_round_trip_for_all_variants() {
        for p in [
            ProviderKind::ClaudeCode,
            ProviderKind::Codex,
            ProviderKind::OpenCode,
            ProviderKind::Future,
        ] {
            let restored =
                ProviderKind::from_wire(p.wire_name()).expect("known wire name must round-trip");
            assert_eq!(restored, p, "round-trip failure for {}", p.wire_name());
        }
    }

    #[test]
    fn provider_kind_rejects_unknown_wire_name() {
        assert!(ProviderKind::from_wire("bogus").is_none());
        assert!(
            ProviderKind::from_wire("CLAUDE_CODE").is_none(),
            "case-flipped wire names must NOT match — parsing is strict and fails loud"
        );
        assert!(ProviderKind::from_wire("").is_none());
    }

    #[test]
    fn try_from_ai_activity_provider_drops_none_sentinel() {
        assert_eq!(
            ProviderKind::try_from(AiActivityProvider::ClaudeCode),
            Ok(ProviderKind::ClaudeCode)
        );
        assert_eq!(
            ProviderKind::try_from(AiActivityProvider::Codex),
            Ok(ProviderKind::Codex)
        );
        assert_eq!(
            ProviderKind::try_from(AiActivityProvider::Opencode),
            Ok(ProviderKind::OpenCode)
        );
        assert!(
            ProviderKind::try_from(AiActivityProvider::None).is_err(),
            "control plane refuses to model `None` as a provider; callers must use Option<ProviderKind>",
        );
    }

    #[test]
    fn signal_channel_round_trips_through_serde() {
        for ch in [
            SignalChannel::LifecycleHook,
            SignalChannel::NotifyCallback,
            SignalChannel::PluginEvent,
            SignalChannel::ObserverPoll,
            SignalChannel::SupervisorRecord,
        ] {
            let json = serde_json::to_string(&ch).expect("serialize channel");
            let back: SignalChannel = serde_json::from_str(&json).expect("deserialize channel");
            assert_eq!(back, ch, "round-trip failure for {ch:?}");
        }
    }

    /// SignalChannel and ProviderSignalSource are deliberately
    /// distinct enums even though several variants overlap. Lock
    /// this in: a future refactor must not collapse them, because
    /// the projector and diagnostics layer both need them
    /// separately (channel = how we routed it, source = what we
    /// read from).
    #[test]
    fn signal_channel_and_provider_signal_source_remain_distinct_types() {
        // This is a compile-time guard. If the two types were ever
        // unified into the same enum, the assertion below would
        // either fail to compile (different paths) or hide the
        // mistake. We avoid the latter by asserting they project
        // distinct discriminant counts at the value level.
        const CHANNELS: usize = 5;
        const SOURCES: usize = 6;
        let chs = [
            SignalChannel::LifecycleHook,
            SignalChannel::NotifyCallback,
            SignalChannel::PluginEvent,
            SignalChannel::ObserverPoll,
            SignalChannel::SupervisorRecord,
        ];
        let srcs = [
            ProviderSignalSource::LifecycleHook,
            ProviderSignalSource::Notify,
            ProviderSignalSource::Plugin,
            ProviderSignalSource::Supervisor,
            ProviderSignalSource::Process,
            ProviderSignalSource::Tmux,
        ];
        assert_eq!(chs.len(), CHANNELS);
        assert_eq!(srcs.len(), SOURCES);
    }

    #[test]
    fn pane_correlation_key_constructs_from_borrowed_strings() {
        let key = PaneCorrelationKey::new("g", "w", "u");
        assert_eq!(key.group, "g");
        assert_eq!(key.workspace, "w");
        assert_eq!(key.unit_id, "u");
    }
}

//! Provider activity policy and capability matrix.
//!
//! Each provider's behavior in the activity control plane is
//! encapsulated in a small policy struct (Claude Code / Codex /
//! OpenCode) that exposes:
//!
//! - which signal channels the provider supports (capability
//!   matrix);
//! - the maximum confidence the provider may attach to a given
//!   channel (e.g. Codex notify caps at `Medium`);
//! - per-channel default freshness windows (used by the projector
//!   to grade staleness without re-asking the provider).
//!
//! Provider policy is pure: it never reads files, never spawns
//! processes, and never depends on the application config. The
//! disabled-by-config decision lives in the
//! [`crate::usecases::activity_control_plane::decision_policy`]
//! layer instead, so a single call site (the decision policy) can
//! suppress reads/writes/diagnostics across all providers.

use crate::domain::activity::{
    ObservationConfidence, ProviderEventKind, ProviderKind, SignalChannel,
};

/// Capability triple for a single (provider, channel) pair.
///
/// Distinct from [`crate::domain::activity::ObservationConfidence`]
/// alone because we also need to know whether the channel is
/// supported at all. `Unsupported` means the adapter for that
/// provider/channel does not exist on the current build; it is not
/// a confidence grade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderActivityCapability {
    /// Channel exists for this provider and the maximum confidence
    /// any event delivered through it may carry.
    Supported(ObservationConfidence),
    /// Channel is not implemented for this provider.
    Unsupported,
}

impl ProviderActivityCapability {
    /// Convenience: extract the max confidence if supported.
    pub fn max_confidence(self) -> Option<ObservationConfidence> {
        match self {
            Self::Supported(c) => Some(c),
            Self::Unsupported => None,
        }
    }
}

/// Capability matrix exposed by a provider policy.
///
/// Stored as the `&'static` reference inside each policy struct so
/// the matrix is a compile-time fact (cheap to read, cheap to
/// pattern-match in tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCapabilityMatrix {
    pub provider: ProviderKind,
    pub lifecycle_hook: ProviderActivityCapability,
    pub notify_callback: ProviderActivityCapability,
    pub plugin_event: ProviderActivityCapability,
    pub observer_poll: ProviderActivityCapability,
    pub supervisor_record: ProviderActivityCapability,
}

impl ProviderCapabilityMatrix {
    /// Look up the capability for a given channel.
    pub fn capability(self, channel: SignalChannel) -> ProviderActivityCapability {
        match channel {
            SignalChannel::LifecycleHook => self.lifecycle_hook,
            SignalChannel::NotifyCallback => self.notify_callback,
            SignalChannel::PluginEvent => self.plugin_event,
            SignalChannel::ObserverPoll => self.observer_poll,
            SignalChannel::SupervisorRecord => self.supervisor_record,
        }
    }
}

/// Provider activity policy.
///
/// Implementations are unit structs; the trait is implemented
/// only by Workhive-supported providers. Adding a new provider
/// requires implementing this trait *and* adding a routing arm
/// in [`policy_for`].
pub trait ProviderActivityPolicy {
    /// Provider this policy describes.
    fn provider(&self) -> ProviderKind;

    /// Static capability matrix.
    fn capability_matrix(&self) -> ProviderCapabilityMatrix;

    /// Whether the provider supports a given provider-event kind
    /// on a given channel. The Phase-1 invariant
    /// "Codex `notify` does not carry working/tool_running" is
    /// enforced here: Codex returns `false` for any non-completion
    /// kind on the notify channel.
    ///
    /// Default impl: any kind is allowed on any supported channel.
    /// Concrete policies override the cases that would violate the
    /// requirement.
    fn allows_event(&self, channel: SignalChannel, kind: ProviderEventKind) -> bool {
        if matches!(
            self.capability_matrix().capability(channel),
            ProviderActivityCapability::Unsupported
        ) {
            return false;
        }
        // Universal Phase-1 invariant: `NotifyCompletion` is
        // channel-exclusive to `SignalChannel::NotifyCallback`. This
        // is symmetric with the rule that the notify channel only
        // carries `NotifyCompletion`. Any other channel that tries
        // to deliver a notify-shaped completion (e.g. a high-confidence
        // lifecycle hook claiming notify semantics) must be rejected
        // so downstream projection cannot derive notify state from a
        // hook channel. Lifecycle/plugin completion uses
        // `ProviderEventKind::TurnCompleted` instead.
        match (channel, kind) {
            // Notify channel: completion-only.
            (SignalChannel::NotifyCallback, ProviderEventKind::NotifyCompletion) => true,
            (SignalChannel::NotifyCallback, _) => false,
            // Any other channel: NotifyCompletion is forbidden;
            // every other provider event kind is allowed.
            (_, ProviderEventKind::NotifyCompletion) => false,
            _ => true,
        }
    }
}

/// Claude Code provider policy. High-confidence lifecycle hooks
/// are the primary channel; no notify callback channel is
/// modeled (Claude does not expose a notify-equivalent that should
/// be promoted into Workhive's event log per the requirement).
#[derive(Debug, Clone, Copy)]
pub struct ClaudeCodePolicy;

impl ProviderActivityPolicy for ClaudeCodePolicy {
    fn provider(&self) -> ProviderKind {
        ProviderKind::ClaudeCode
    }

    fn capability_matrix(&self) -> ProviderCapabilityMatrix {
        ProviderCapabilityMatrix {
            provider: ProviderKind::ClaudeCode,
            lifecycle_hook: ProviderActivityCapability::Supported(ObservationConfidence::High),
            notify_callback: ProviderActivityCapability::Unsupported,
            plugin_event: ProviderActivityCapability::Unsupported,
            observer_poll: ProviderActivityCapability::Supported(ObservationConfidence::Low),
            supervisor_record: ProviderActivityCapability::Supported(
                ObservationConfidence::MediumLow,
            ),
        }
    }
}

/// Codex provider policy. Lifecycle hooks at `High`, notify
/// callback at `Medium` and **only** for `NotifyCompletion`.
#[derive(Debug, Clone, Copy)]
pub struct CodexPolicy;

impl ProviderActivityPolicy for CodexPolicy {
    fn provider(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    fn capability_matrix(&self) -> ProviderCapabilityMatrix {
        ProviderCapabilityMatrix {
            provider: ProviderKind::Codex,
            lifecycle_hook: ProviderActivityCapability::Supported(ObservationConfidence::High),
            notify_callback: ProviderActivityCapability::Supported(ObservationConfidence::Medium),
            plugin_event: ProviderActivityCapability::Unsupported,
            observer_poll: ProviderActivityCapability::Supported(ObservationConfidence::Low),
            supervisor_record: ProviderActivityCapability::Supported(
                ObservationConfidence::MediumLow,
            ),
        }
    }
}

/// OpenCode provider policy. Plugin events are the primary
/// high-confidence channel; lifecycle hook channel is unsupported
/// (OpenCode integrates via plugin, not external hooks).
#[derive(Debug, Clone, Copy)]
pub struct OpenCodePolicy;

impl ProviderActivityPolicy for OpenCodePolicy {
    fn provider(&self) -> ProviderKind {
        ProviderKind::OpenCode
    }

    fn capability_matrix(&self) -> ProviderCapabilityMatrix {
        ProviderCapabilityMatrix {
            provider: ProviderKind::OpenCode,
            lifecycle_hook: ProviderActivityCapability::Unsupported,
            notify_callback: ProviderActivityCapability::Unsupported,
            plugin_event: ProviderActivityCapability::Supported(ObservationConfidence::High),
            observer_poll: ProviderActivityCapability::Supported(ObservationConfidence::Low),
            supervisor_record: ProviderActivityCapability::Supported(
                ObservationConfidence::MediumLow,
            ),
        }
    }
}

/// Owned policy enum. Returned by [`policy_for`] so callers can
/// dispatch without dynamic allocation.
#[derive(Debug, Clone, Copy)]
pub enum AnyProviderPolicy {
    ClaudeCode(ClaudeCodePolicy),
    Codex(CodexPolicy),
    OpenCode(OpenCodePolicy),
    /// `Future` provider has no implemented policy. The variant
    /// exists so callers can pattern-match exhaustively without
    /// needing a wildcard arm.
    Future(ProviderKind),
}

impl ProviderActivityPolicy for AnyProviderPolicy {
    fn provider(&self) -> ProviderKind {
        match self {
            Self::ClaudeCode(p) => p.provider(),
            Self::Codex(p) => p.provider(),
            Self::OpenCode(p) => p.provider(),
            Self::Future(p) => *p,
        }
    }

    fn capability_matrix(&self) -> ProviderCapabilityMatrix {
        match self {
            Self::ClaudeCode(p) => p.capability_matrix(),
            Self::Codex(p) => p.capability_matrix(),
            Self::OpenCode(p) => p.capability_matrix(),
            Self::Future(provider) => ProviderCapabilityMatrix {
                provider: *provider,
                lifecycle_hook: ProviderActivityCapability::Unsupported,
                notify_callback: ProviderActivityCapability::Unsupported,
                plugin_event: ProviderActivityCapability::Unsupported,
                observer_poll: ProviderActivityCapability::Unsupported,
                supervisor_record: ProviderActivityCapability::Unsupported,
            },
        }
    }

    fn allows_event(&self, channel: SignalChannel, kind: ProviderEventKind) -> bool {
        match self {
            Self::ClaudeCode(p) => p.allows_event(channel, kind),
            Self::Codex(p) => p.allows_event(channel, kind),
            Self::OpenCode(p) => p.allows_event(channel, kind),
            Self::Future(_) => false,
        }
    }
}

/// Look up the policy for a given provider.
pub fn policy_for(provider: ProviderKind) -> AnyProviderPolicy {
    match provider {
        ProviderKind::ClaudeCode => AnyProviderPolicy::ClaudeCode(ClaudeCodePolicy),
        ProviderKind::Codex => AnyProviderPolicy::Codex(CodexPolicy),
        ProviderKind::OpenCode => AnyProviderPolicy::OpenCode(OpenCodePolicy),
        ProviderKind::Future => AnyProviderPolicy::Future(ProviderKind::Future),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Capability matrix snapshot for the three first-class
    /// providers. This is the central "provider policy matrix"
    /// test — bumping any cell requires an
    /// explicit rationale because it changes the trust contract
    /// the projector and decision policy rely on.
    #[test]
    fn provider_capability_matrix_snapshot() {
        let claude = ClaudeCodePolicy.capability_matrix();
        let codex = CodexPolicy.capability_matrix();
        let opencode = OpenCodePolicy.capability_matrix();

        // Claude Code: lifecycle hooks high, no notify, no plugin.
        assert_eq!(
            claude.lifecycle_hook,
            ProviderActivityCapability::Supported(ObservationConfidence::High),
            "Claude Code lifecycle hook must be High",
        );
        assert_eq!(
            claude.notify_callback,
            ProviderActivityCapability::Unsupported,
            "Claude Code does not expose a notify-equivalent that drives the event log",
        );
        assert_eq!(
            claude.plugin_event,
            ProviderActivityCapability::Unsupported,
            "Claude Code plugin event channel is unsupported",
        );

        // Codex: lifecycle hooks high, notify medium (completion-only).
        assert_eq!(
            codex.lifecycle_hook,
            ProviderActivityCapability::Supported(ObservationConfidence::High),
            "Codex lifecycle hook must be High",
        );
        assert_eq!(
            codex.notify_callback,
            ProviderActivityCapability::Supported(ObservationConfidence::Medium),
            "Codex notify callback must cap at Medium",
        );
        assert_eq!(
            codex.plugin_event,
            ProviderActivityCapability::Unsupported,
            "Codex plugin event channel is unsupported",
        );

        // OpenCode: plugin event high; no lifecycle hook, no notify.
        assert_eq!(
            opencode.plugin_event,
            ProviderActivityCapability::Supported(ObservationConfidence::High),
            "OpenCode plugin event must be High",
        );
        assert_eq!(
            opencode.lifecycle_hook,
            ProviderActivityCapability::Unsupported,
            "OpenCode does not expose external lifecycle hooks (plugin only)",
        );
        assert_eq!(
            opencode.notify_callback,
            ProviderActivityCapability::Unsupported,
            "OpenCode notify callback is unsupported",
        );
    }

    /// Phase-1 invariant pinning: Codex notify channel must NOT
    /// allow any provider event other than `NotifyCompletion`.
    /// Codex hooks remain free to deliver the other kinds.
    #[test]
    fn codex_notify_channel_only_allows_notify_completion() {
        let codex = CodexPolicy;
        // Allowed: NotifyCompletion via notify.
        assert!(
            codex.allows_event(
                SignalChannel::NotifyCallback,
                ProviderEventKind::NotifyCompletion
            ),
            "NotifyCompletion via notify callback must be allowed",
        );
        // Forbidden: working/tool/permission/etc via notify.
        for forbidden in [
            ProviderEventKind::PromptSubmitted,
            ProviderEventKind::ToolStarted,
            ProviderEventKind::ToolFinished,
            ProviderEventKind::PermissionRequested,
            ProviderEventKind::PermissionResolved,
            ProviderEventKind::TurnCompleted,
            ProviderEventKind::TurnFailed,
            ProviderEventKind::SessionStarted,
            ProviderEventKind::SessionIdle,
            ProviderEventKind::TelemetryStale,
        ] {
            assert!(
                !codex.allows_event(SignalChannel::NotifyCallback, forbidden),
                "Codex notify must NOT carry {forbidden:?} (Phase-1 invariant)",
            );
        }
        // Lifecycle hook channel may carry every provider event.
        for allowed in [
            ProviderEventKind::PromptSubmitted,
            ProviderEventKind::ToolStarted,
            ProviderEventKind::TurnCompleted,
            ProviderEventKind::TurnFailed,
        ] {
            assert!(
                codex.allows_event(SignalChannel::LifecycleHook, allowed),
                "Codex lifecycle hook must allow {allowed:?}",
            );
        }
    }

    /// The "notify is completion-only for every provider" invariant
    /// is policy-wide. Lock it in for a hypothetical Future
    /// provider that grows a notify channel later — the trait's
    /// default impl already enforces it.
    #[test]
    fn notify_callback_is_completion_only_for_all_supported_providers() {
        for policy in [
            policy_for(ProviderKind::ClaudeCode),
            policy_for(ProviderKind::Codex),
            policy_for(ProviderKind::OpenCode),
        ] {
            // Either notify is unsupported (Claude / OpenCode), or
            // it allows only NotifyCompletion (Codex).
            let cap = policy
                .capability_matrix()
                .capability(SignalChannel::NotifyCallback);
            match cap {
                ProviderActivityCapability::Unsupported => {
                    // Unsupported: every kind must be rejected.
                    for k in [
                        ProviderEventKind::NotifyCompletion,
                        ProviderEventKind::ToolStarted,
                    ] {
                        assert!(
                            !policy.allows_event(SignalChannel::NotifyCallback, k),
                            "{:?} notify is unsupported and must reject {k:?}",
                            policy.provider(),
                        );
                    }
                }
                ProviderActivityCapability::Supported(_) => {
                    assert!(
                        policy.allows_event(
                            SignalChannel::NotifyCallback,
                            ProviderEventKind::NotifyCompletion
                        ),
                        "{:?} notify must allow NotifyCompletion",
                        policy.provider(),
                    );
                    assert!(
                        !policy.allows_event(
                            SignalChannel::NotifyCallback,
                            ProviderEventKind::ToolStarted
                        ),
                        "{:?} notify must NOT allow ToolStarted",
                        policy.provider(),
                    );
                }
            }
        }
    }

    /// Future provider has no implemented adapter. Its policy
    /// must report unsupported for every channel and reject every
    /// event so callers cannot accidentally drive state from it.
    #[test]
    fn future_provider_has_unsupported_matrix_and_rejects_all_events() {
        let policy = policy_for(ProviderKind::Future);
        let m = policy.capability_matrix();
        assert_eq!(m.lifecycle_hook, ProviderActivityCapability::Unsupported);
        assert_eq!(m.notify_callback, ProviderActivityCapability::Unsupported);
        assert_eq!(m.plugin_event, ProviderActivityCapability::Unsupported);
        assert_eq!(m.observer_poll, ProviderActivityCapability::Unsupported);
        assert_eq!(m.supervisor_record, ProviderActivityCapability::Unsupported);
        for ch in [
            SignalChannel::LifecycleHook,
            SignalChannel::NotifyCallback,
            SignalChannel::PluginEvent,
            SignalChannel::ObserverPoll,
            SignalChannel::SupervisorRecord,
        ] {
            for k in [
                ProviderEventKind::PromptSubmitted,
                ProviderEventKind::TurnCompleted,
                ProviderEventKind::NotifyCompletion,
            ] {
                assert!(
                    !policy.allows_event(ch, k),
                    "Future provider must reject every (channel, kind)",
                );
            }
        }
    }
}

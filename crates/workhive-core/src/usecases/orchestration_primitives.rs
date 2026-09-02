//! Conservative orchestration primitives.
//!
//! Pure decision functions consumed by the pane control plane to
//! determine whether it is safe to send a prompt, wait for a
//! completion, or route permission-needed events.
//!
//! ## Design decisions
//!
//! * All functions are **pure**: they take typed inputs and return
//!   typed decisions. No I/O, no logging, no side effects.
//! * Inputs are [`PaneActivityMetrics`] (Agent B's contract) so the
//!   orchestrator never parses raw provider payloads.
//! * `DecisionPolicy` / `DefaultDecisionPolicy` gates every action
//!   through the shared confidence + freshness + disabled checks.
//! * `force` in [`guarded_send`] must be an explicit caller opt-in.
//!   `guarded_send` never silently auto-bypasses its guards.
//!
//! ## File split note
//!
//! `usecases/orchestration.rs` is ~950 lines (above the 800-line
//! target). Converting it to a `mod orchestration/` directory was
//! considered but left to the integration lead to avoid a large
//! rename-and-verify step in a multi-agent batch. This sibling file
//! holds only Agent D's new primitive functions. Future work should
//! convert `orchestration.rs` → `orchestration/mod.rs` and move the
//! legacy functions into sibling files inside the directory.

use crate::domain::activity::{
    ObservationConfidence, PaneActivityState, PaneCorrelationKey, ProviderKind, SignalChannel,
    SignalFreshness,
};
use crate::usecases::activity_control_plane::metrics::PaneActivityMetrics;

// ── Shared guard helpers ──────────────────────────────────────────────────────

/// Whether `metrics` represents a fresh, high-confidence (or
/// medium-notify-completion), orchestration-eligible state.
///
/// This is the shared gate used by notification and orchestration
/// primitives. Individual primitives may add additional state-specific
/// checks on top of this baseline.
#[allow(dead_code)]
fn is_orchestration_ready(metrics: &PaneActivityMetrics) -> bool {
    // 1. Must be from the ACP event path.
    if !metrics.acp_source {
        return false;
    }
    // 2. State must be orchestration-eligible (not Unknown / Stale /
    //    Disabled / Unsupported / Degraded / OpenUnverified).
    if !metrics.state.is_orchestration_eligible() {
        return false;
    }
    // 3. Signal must be fresh.
    if !matches!(metrics.freshness, SignalFreshness::Fresh) {
        return false;
    }
    // 4. Confidence floor: High is always allowed. Medium is allowed
    //    only for the notify-callback completion flow (mirrors
    //    DefaultDecisionPolicy).
    if matches!(metrics.confidence, ObservationConfidence::High) {
        return true;
    }
    matches!(metrics.confidence, ObservationConfidence::Medium)
        && matches!(metrics.state, PaneActivityState::Completed)
        && matches!(metrics.channel, SignalChannel::NotifyCallback)
}

/// Whether `metrics` is in a completion-like terminal state.
fn is_completion_like(metrics: &PaneActivityMetrics) -> bool {
    matches!(
        metrics.state,
        PaneActivityState::Completed | PaneActivityState::Idle
    )
}

// ── WaitPaneDecision ──────────────────────────────────────────────────────────

/// Why a [`WaitPaneDecision::NotReady`] or
/// [`WaitPaneDecision::Refused`] was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPaneRefusalReason {
    /// `acp_source = false` (legacy path) — cannot make a reliable
    /// wait decision.
    LegacySource,
    /// State is not orchestration-eligible.
    NoUsableState,
    /// Signal is stale or expired.
    StaleOrExpired,
    /// Confidence is below the orchestration floor.
    LowConfidence,
}

/// Why a [`WaitPaneDecision::NotReady`] was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPaneNotReadyReason {
    /// Pane is actively working or running a tool.
    InProgress,
    /// Pane is waiting for user permission.
    WaitingForPermission,
    /// Pane turn failed — not a completion.
    Failed,
}

/// Outcome of [`wait_pane`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitPaneDecision {
    /// The pane is in a fresh, high-confidence completion-like state.
    Ready,
    /// The pane is running or waiting, but the state is still usable.
    NotReady { reason: WaitPaneNotReadyReason },
    /// The pane data is unusable for an orchestration wait decision.
    Refused { reason: WaitPaneRefusalReason },
}

/// Pure wait-for-completion decision for a single pane.
///
/// `Ready` only when `metrics` is fresh + high-confidence +
/// completion-like (`Completed` or `Idle`).
pub fn wait_pane(metrics: &PaneActivityMetrics) -> WaitPaneDecision {
    if !metrics.acp_source {
        return WaitPaneDecision::Refused {
            reason: WaitPaneRefusalReason::LegacySource,
        };
    }

    if !metrics.state.is_orchestration_eligible() {
        return WaitPaneDecision::Refused {
            reason: WaitPaneRefusalReason::NoUsableState,
        };
    }

    if !matches!(metrics.freshness, SignalFreshness::Fresh) {
        return WaitPaneDecision::Refused {
            reason: WaitPaneRefusalReason::StaleOrExpired,
        };
    }

    // Confidence floor.
    let confidence_ok = matches!(metrics.confidence, ObservationConfidence::High)
        || (matches!(metrics.confidence, ObservationConfidence::Medium)
            && matches!(metrics.state, PaneActivityState::Completed)
            && matches!(metrics.channel, SignalChannel::NotifyCallback));

    if !confidence_ok {
        return WaitPaneDecision::Refused {
            reason: WaitPaneRefusalReason::LowConfidence,
        };
    }

    if is_completion_like(metrics) {
        return WaitPaneDecision::Ready;
    }

    // Orchestration-eligible but not yet complete.
    let not_ready_reason = match metrics.state {
        PaneActivityState::Working | PaneActivityState::ToolRunning => {
            WaitPaneNotReadyReason::InProgress
        }
        PaneActivityState::WaitingForPermission => WaitPaneNotReadyReason::WaitingForPermission,
        PaneActivityState::Failed => WaitPaneNotReadyReason::Failed,
        // Idle / Completed covered by `is_completion_like`; all other
        // non-eligible states refused above. Default arm satisfies the
        // exhaustiveness requirement.
        _ => WaitPaneNotReadyReason::InProgress,
    };

    WaitPaneDecision::NotReady {
        reason: not_ready_reason,
    }
}

// ── WaitAllDecision ───────────────────────────────────────────────────────────

/// Outcome of [`wait_all`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitAllDecision {
    /// All relevant AI panes are in a fresh, high-confidence
    /// completion state.
    Ready,
    /// At least one relevant pane is not yet done.
    NotReady {
        /// Pane keys still in-progress (not ready or refused).
        remaining: Vec<PaneCorrelationKey>,
    },
    /// The `metrics_list` was empty (no ACP panes to wait on).
    NoPanesRegistered,
    /// Every pane was refused (legacy source / stale / disabled).
    Refused,
}

/// Pure wait-for-all-completion decision.
///
/// `Ready` only when every pane in `metrics_list` with
/// `acp_source = true` is fresh + completion-like + high-confidence.
pub fn wait_all(metrics_list: &[PaneActivityMetrics]) -> WaitAllDecision {
    let relevant: Vec<&PaneActivityMetrics> =
        metrics_list.iter().filter(|m| m.acp_source).collect();

    if relevant.is_empty() {
        return WaitAllDecision::NoPanesRegistered;
    }

    let mut remaining = Vec::new();
    let mut all_refused = true;

    for m in &relevant {
        match wait_pane(m) {
            WaitPaneDecision::Ready => {
                all_refused = false;
            }
            WaitPaneDecision::NotReady { .. } => {
                all_refused = false;
                remaining.push(m.pane.clone());
            }
            WaitPaneDecision::Refused { .. } => {
                // Keep all_refused = true only if every pane is refused.
                remaining.push(m.pane.clone());
            }
        }
    }

    if all_refused {
        return WaitAllDecision::Refused;
    }

    if remaining.is_empty() {
        WaitAllDecision::Ready
    } else {
        WaitAllDecision::NotReady { remaining }
    }
}

// ── PaneNeedingPermission ─────────────────────────────────────────────────────

/// A pane currently blocking on a user permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneNeedingPermission {
    /// Pane identity.
    pub pane: PaneCorrelationKey,
    /// Provider, if known.
    pub provider: Option<ProviderKind>,
}

/// Pure permission-attention detector.
///
/// Returns only panes with fresh, high-confidence
/// `WaitingForPermission` state. Stale or low-confidence permission
/// events are excluded.
pub fn permission_attention(metrics_list: &[PaneActivityMetrics]) -> Vec<PaneNeedingPermission> {
    metrics_list
        .iter()
        .filter(|m| {
            m.acp_source
                && matches!(m.state, PaneActivityState::WaitingForPermission)
                && matches!(m.freshness, SignalFreshness::Fresh)
                && matches!(m.confidence, ObservationConfidence::High)
        })
        .map(|m| PaneNeedingPermission {
            pane: m.pane.clone(),
            provider: m.provider,
        })
        .collect()
}

// ── GuardedSendDecision ───────────────────────────────────────────────────────

/// Why [`GuardedSendDecision::Block`] was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedSendBlockReason {
    /// Target pane is actively working.
    PaneBusy,
    /// Target pane is running a tool.
    ToolRunning,
    /// Target pane is waiting for user permission.
    WaitingForPermission,
    /// Signal is stale or expired.
    StaleOrExpired,
    /// Confidence is below the safe-to-send floor.
    LowConfidence,
    /// State is unknown, unsupported, disabled, or degraded.
    NoUsableState,
    /// Source is the legacy path — cannot make a reliable decision.
    LegacySource,
    /// The activity event store is unreachable or its contents are
    /// corrupt at decision time. Conservative: when the ACP path
    /// is the production source for the target pane and the read
    /// fails (`Io` / `Unavailable` / `CorruptEntry`), the guard
    /// MUST refuse rather than falling back to a "no events
    /// observed" interpretation that would silently bypass the
    /// safety check.
    AcpUnavailable,
}

/// Outcome of [`guarded_send`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardedSendDecision {
    /// Target is safe to receive a new prompt.
    Allow,
    /// Target should not receive a new prompt; the caller must refuse.
    Block { reason: GuardedSendBlockReason },
    /// `force = true` was set by the caller. The guard would have
    /// blocked but the caller explicitly overrode it.
    /// The caller MUST log this at `warn` before proceeding.
    AllowWithForce { reason: GuardedSendBlockReason },
}

impl GuardedSendDecision {
    /// Whether the decision ultimately allows the send (with or without
    /// force).
    pub fn is_allow(&self) -> bool {
        matches!(self, Self::Allow | Self::AllowWithForce { .. })
    }
}

/// Pure guarded-send decision.
///
/// `Allow` only when the target is idle/safe/fresh/high-confidence.
/// `Block` for busy / tool-running / waiting-permission / stale /
/// low-confidence / unknown / unsupported / disabled / corrupt states
/// unless `force == true`, in which case `AllowWithForce { reason }`
/// is returned so the caller can log/refuse at the application layer.
///
/// `force` requires explicit caller opt-in — it MUST NOT be the
/// default for any public API.
pub fn guarded_send(target_metrics: &PaneActivityMetrics, force: bool) -> GuardedSendDecision {
    // 1. Legacy source: cannot assess safety.
    if !target_metrics.acp_source {
        let reason = GuardedSendBlockReason::LegacySource;
        return if force {
            GuardedSendDecision::AllowWithForce { reason }
        } else {
            GuardedSendDecision::Block { reason }
        };
    }

    // 2. State eligibility.
    if !target_metrics.state.is_orchestration_eligible() {
        let reason = GuardedSendBlockReason::NoUsableState;
        return if force {
            GuardedSendDecision::AllowWithForce { reason }
        } else {
            GuardedSendDecision::Block { reason }
        };
    }

    // 3. Freshness.
    if !matches!(target_metrics.freshness, SignalFreshness::Fresh) {
        let reason = GuardedSendBlockReason::StaleOrExpired;
        return if force {
            GuardedSendDecision::AllowWithForce { reason }
        } else {
            GuardedSendDecision::Block { reason }
        };
    }

    // 4. Confidence floor.
    if !matches!(target_metrics.confidence, ObservationConfidence::High) {
        let reason = GuardedSendBlockReason::LowConfidence;
        return if force {
            GuardedSendDecision::AllowWithForce { reason }
        } else {
            GuardedSendDecision::Block { reason }
        };
    }

    // 5. State-specific safety: block on active busy states.
    let block_reason = match target_metrics.state {
        PaneActivityState::Working => Some(GuardedSendBlockReason::PaneBusy),
        PaneActivityState::ToolRunning => Some(GuardedSendBlockReason::ToolRunning),
        PaneActivityState::WaitingForPermission => {
            Some(GuardedSendBlockReason::WaitingForPermission)
        }
        // Completed / Idle / Failed are safe to send to.
        PaneActivityState::Completed | PaneActivityState::Idle | PaneActivityState::Failed => None,
        // Fallback — any remaining orchestration-eligible state that is
        // not explicitly safe defaults to block.
        _ => Some(GuardedSendBlockReason::NoUsableState),
    };

    match block_reason {
        Some(reason) => {
            if force {
                GuardedSendDecision::AllowWithForce { reason }
            } else {
                GuardedSendDecision::Block { reason }
            }
        }
        None => GuardedSendDecision::Allow,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecases::activity_control_plane::metrics::{
        MetricValue, PaneActivityMetrics, UnavailableReason,
    };

    fn pane() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u1")
    }

    fn pane2() -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", "u2")
    }

    fn metrics(
        state: PaneActivityState,
        confidence: ObservationConfidence,
        freshness: SignalFreshness,
        acp_source: bool,
    ) -> PaneActivityMetrics {
        PaneActivityMetrics {
            pane: pane(),
            provider: Some(ProviderKind::Codex),
            state,
            channel: SignalChannel::LifecycleHook,
            confidence,
            freshness,
            elapsed: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            current_turn_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            last_completed_turn_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            last_tool_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            aggregate_tool_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            permission_wait_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            idle_duration: MetricValue::Unavailable {
                reason: UnavailableReason::NoHistory,
            },
            provider_freshness_age: MetricValue::Known(std::time::Duration::from_millis(1_000)),
            acp_source,
        }
    }

    fn acp(state: PaneActivityState) -> PaneActivityMetrics {
        metrics(
            state,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
            true,
        )
    }

    fn acp_with_confidence(
        state: PaneActivityState,
        confidence: ObservationConfidence,
    ) -> PaneActivityMetrics {
        metrics(state, confidence, SignalFreshness::Fresh, true)
    }

    fn acp_stale(state: PaneActivityState) -> PaneActivityMetrics {
        metrics(
            state,
            ObservationConfidence::High,
            SignalFreshness::Stale,
            true,
        )
    }

    fn legacy(state: PaneActivityState) -> PaneActivityMetrics {
        metrics(
            state,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
            false,
        )
    }

    // ── wait_pane ──────────────────────────────────────────────────

    #[test]
    fn wait_pane_ready_only_for_fresh_high_confidence_completion() {
        assert_eq!(
            wait_pane(&acp(PaneActivityState::Completed)),
            WaitPaneDecision::Ready
        );
        assert_eq!(
            wait_pane(&acp(PaneActivityState::Idle)),
            WaitPaneDecision::Ready
        );
    }

    #[test]
    fn wait_pane_refuses_stale_or_low_confidence() {
        assert!(matches!(
            wait_pane(&acp_stale(PaneActivityState::Completed)),
            WaitPaneDecision::Refused {
                reason: WaitPaneRefusalReason::StaleOrExpired
            }
        ));
        assert!(matches!(
            wait_pane(&acp_with_confidence(
                PaneActivityState::Completed,
                ObservationConfidence::Low
            )),
            WaitPaneDecision::Refused {
                reason: WaitPaneRefusalReason::LowConfidence
            }
        ));
    }

    #[test]
    fn wait_pane_refuses_disabled_unsupported_corrupt_unknown() {
        for state in [
            PaneActivityState::Unknown,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
            PaneActivityState::Stale,
        ] {
            assert!(
                matches!(
                    wait_pane(&acp(state)),
                    WaitPaneDecision::Refused {
                        reason: WaitPaneRefusalReason::NoUsableState
                    }
                ),
                "{state:?} must be refused as NoUsableState"
            );
        }
    }

    #[test]
    fn wait_pane_not_ready_for_working_pane() {
        assert!(matches!(
            wait_pane(&acp(PaneActivityState::Working)),
            WaitPaneDecision::NotReady {
                reason: WaitPaneNotReadyReason::InProgress
            }
        ));
    }

    #[test]
    fn wait_pane_not_ready_for_permission_needed() {
        assert!(matches!(
            wait_pane(&acp(PaneActivityState::WaitingForPermission)),
            WaitPaneDecision::NotReady {
                reason: WaitPaneNotReadyReason::WaitingForPermission
            }
        ));
    }

    // ── wait_all ───────────────────────────────────────────────────

    #[test]
    fn wait_all_ready_only_when_all_panes_fresh_completion() {
        let m1 = acp(PaneActivityState::Completed);
        let mut m2 = acp(PaneActivityState::Idle);
        m2.pane = pane2();
        assert_eq!(wait_all(&[m1, m2]), WaitAllDecision::Ready);
    }

    #[test]
    fn wait_all_returns_remaining_when_some_in_progress() {
        let m1 = acp(PaneActivityState::Completed);
        let mut m2 = acp(PaneActivityState::Working);
        m2.pane = pane2();
        match wait_all(&[m1, m2]) {
            WaitAllDecision::NotReady { remaining } => {
                assert_eq!(remaining.len(), 1);
                assert_eq!(remaining[0], pane2());
            }
            other => panic!("expected NotReady, got {other:?}"),
        }
    }

    #[test]
    fn wait_all_no_panes_registered_when_list_empty() {
        assert_eq!(wait_all(&[]), WaitAllDecision::NoPanesRegistered);
    }

    #[test]
    fn wait_all_no_panes_registered_when_all_are_legacy() {
        let m1 = legacy(PaneActivityState::Completed);
        // Legacy panes are skipped; list of relevant becomes empty.
        assert_eq!(wait_all(&[m1]), WaitAllDecision::NoPanesRegistered);
    }

    // ── permission_attention ───────────────────────────────────────

    #[test]
    fn permission_attention_lists_only_high_confidence_permission_needed() {
        let m = acp(PaneActivityState::WaitingForPermission);
        let result = permission_attention(&[m]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].pane, pane());
    }

    #[test]
    fn permission_attention_excludes_stale_or_low_confidence() {
        let stale = acp_stale(PaneActivityState::WaitingForPermission);
        let low = acp_with_confidence(
            PaneActivityState::WaitingForPermission,
            ObservationConfidence::Low,
        );
        assert!(
            permission_attention(&[stale]).is_empty(),
            "stale must be excluded"
        );
        assert!(
            permission_attention(&[low]).is_empty(),
            "low confidence must be excluded"
        );
    }

    // ── guarded_send ───────────────────────────────────────────────

    #[test]
    fn guarded_send_allow_for_idle_safe_fresh_target() {
        assert_eq!(
            guarded_send(&acp(PaneActivityState::Idle), false),
            GuardedSendDecision::Allow
        );
        assert_eq!(
            guarded_send(&acp(PaneActivityState::Completed), false),
            GuardedSendDecision::Allow
        );
        assert_eq!(
            guarded_send(&acp(PaneActivityState::Failed), false),
            GuardedSendDecision::Allow
        );
    }

    #[test]
    fn guarded_send_blocks_for_busy_tool_running_target() {
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::Working), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::PaneBusy
            }
        ));
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::ToolRunning), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::ToolRunning
            }
        ));
    }

    #[test]
    fn guarded_send_blocks_for_waiting_permission_target() {
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::WaitingForPermission), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::WaitingForPermission
            }
        ));
    }

    #[test]
    fn guarded_send_blocks_for_stale_low_confidence_unknown_unsupported_disabled_corrupt() {
        // Stale.
        assert!(matches!(
            guarded_send(&acp_stale(PaneActivityState::Idle), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::StaleOrExpired
            }
        ));
        // Low confidence.
        assert!(matches!(
            guarded_send(
                &acp_with_confidence(PaneActivityState::Idle, ObservationConfidence::Low),
                false,
            ),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::LowConfidence
            }
        ));
        // Unknown state.
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::Unknown), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::NoUsableState
            }
        ));
        // Disabled.
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::Disabled), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::NoUsableState
            }
        ));
        // Legacy source.
        assert!(matches!(
            guarded_send(&legacy(PaneActivityState::Completed), false),
            GuardedSendDecision::Block {
                reason: GuardedSendBlockReason::LegacySource
            }
        ));
    }

    #[test]
    fn guarded_send_force_returns_allow_with_force_reason() {
        // Busy pane + force = AllowWithForce.
        assert!(matches!(
            guarded_send(&acp(PaneActivityState::Working), true),
            GuardedSendDecision::AllowWithForce {
                reason: GuardedSendBlockReason::PaneBusy
            }
        ));
        // Stale + force = AllowWithForce.
        assert!(matches!(
            guarded_send(&acp_stale(PaneActivityState::Idle), true),
            GuardedSendDecision::AllowWithForce {
                reason: GuardedSendBlockReason::StaleOrExpired
            }
        ));
        // Legacy + force = AllowWithForce.
        assert!(matches!(
            guarded_send(&legacy(PaneActivityState::Completed), true),
            GuardedSendDecision::AllowWithForce {
                reason: GuardedSendBlockReason::LegacySource
            }
        ));
    }
}

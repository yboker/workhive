//! Typed `pane send` action plan and centralized capability resolver.
//!
//! ## Why this module exists
//!
//! `workhive pane send` expresses the user's intent as a
//! structured input action plan instead of treating every target as
//! "paste text and maybe press Enter":
//!
//!   * AI chat panes (`Codex`, `Claude Code`) MUST submit by default;
//!   * `--no-enter` MUST remain available as text-only insertion;
//!   * shell/TUI/unknown panes MUST NOT silently receive normal chat
//!     text — they refuse, warn, or require explicit raw mode;
//!   * the decision MUST live in core logic, not in scattered
//!     `if command == "codex"` checks at the CLI boundary.
//!
//! This module implements the minimum viable typed send plan that
//! satisfies acceptance criteria 1, 2, 3, 5, 6, 7, 8, 10, 11 of the
//! requirement. Tool-specific submit profiles
//! (`paste_newline` vs. `text_then_enter`) and catalog `[messaging]`
//! schema extensions live under follow-up work; the plan introduced
//! here is the stable abstraction those follow-ups plug into.
//!
//! ## Stability boundary
//!
//! Core code consumes [`PaneMessageSafety`] (already produced by the
//! observation merge use case) plus the user's flags and emits a
//! [`PaneSendDecision`]. The CLI is responsible for:
//!
//!   1. resolving the target pane;
//!   2. fetching its [`PaneRuntimeTruth`] via the existing observation
//!      pipeline;
//!   3. asking [`resolve_pane_send_plan`] for the typed decision;
//!   4. surfacing the localized refusal/warning text and dispatching
//!      the eventual [`PaneSendPlan`] through the control plane.
//!
//! `workhive-core` deliberately does NOT own tmux key syntax such as
//! `Enter` or `C-m`. The send plan only states "insert this text" and
//! "submit yes/no"; the tmux adapter is still the single owner of the
//! actual key sequence.

use serde::{Deserialize, Serialize};

use crate::domain::PaneMessageSafety;
use crate::usecases::activity_control_plane::metrics::PaneActivityMetrics;
use crate::usecases::orchestration_primitives::{
    GuardedSendBlockReason, GuardedSendDecision, guarded_send,
};

/// Reason a `pane send` decision was selected.
///
/// Carrying the reason lets tests assert *why* the resolver picked a
/// specific decision without re-implementing the rule table inline.
/// It also lets the CLI render localized warnings/refusals keyed by a
/// stable enum instead of stringly-typed reason strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneSendReason {
    /// Target is a supervisor-confirmed AI message receiver
    /// (`PaneMessageSafety::SafeStructured`). Default flow:
    /// insert + submit.
    SafeStructuredAgent,
    /// Target is a recognised shell. Normal `pane send <target> "msg"`
    /// MUST refuse — silently delivering chat text would invoke
    /// arbitrary shell commands. The CLI surfaces a localized refusal
    /// and points users at `--force` for the explicit raw-keystroke
    /// override.
    ShellNotMessageReceiver,
    /// Target's foreground process looks like an AI agent but the
    /// supervisor handshake is not confirmed
    /// (`PaneMessageSafety::UnverifiedAgent`). The send proceeds with
    /// a localized warning rather than silently routing structured
    /// input to a possibly-stale pane.
    UnverifiedAgent,
    /// `--no-enter` was honoured for a receiver that accepts text.
    /// The CLI dispatches with `submit = false`.
    InsertOnlyByFlag,
    /// Target runs a TUI such as `lazygit`. Normal `pane send "msg"`
    /// must NOT silently inject keystrokes; refuse without
    /// `--force`.
    TuiNotMessageReceiver,
    /// No reliable observation classifies the target. Refuse without
    /// `--force` so unknown panes never get chat text by accident.
    UnknownTarget,
    /// `--force` was used to override a refusal. The dispatch still
    /// happens but the decision is tagged so the CLI can warn loudly
    /// that this was an explicit, lower-level override.
    ForcedOverride,
    /// `guarded_send` returned Block for the ACP-derived metrics.
    /// The send is refused by the activity guard; `--force` is the
    /// explicit opt-in override documented in the safety contract.
    ActivityGuardBlocked,
    /// `guarded_send` returned `AllowWithForce` because the caller
    /// passed `force = true`. The send proceeds but is tagged so the
    /// CLI can warn before dispatching.
    ActivityGuardForcedOverride,
}

/// Concrete dispatch plan a CLI/runtime must execute when a
/// `pane send` is allowed.
///
/// `submit = true` means "after inserting the text, instruct the
/// runtime to submit the input" — the tmux adapter is still the only
/// place that knows how to translate this into a `send-keys Enter`
/// (or any future per-tool sequence). Core never speaks tmux keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSendPlan {
    /// Literal text to insert into the target pane.
    pub text: String,
    /// Whether the runtime should submit the inserted text.
    pub submit: bool,
}

/// Typed outcome of [`resolve_pane_send_plan`].
///
/// The CLI MUST exhaustively match this enum — every variant is a
/// real production outcome and adding a new case is a deliberate
/// extension point, not a default fallthrough.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneSendDecision {
    /// Dispatch the plan unchanged.
    Allow {
        plan: PaneSendPlan,
        reason: PaneSendReason,
    },
    /// Dispatch the plan but render a localized warning before doing
    /// so. Used for `UnverifiedAgent` and `ForcedOverride`, where
    /// the send is plausibly intentional but the user benefits from
    /// knowing the target is not a supervisor-confirmed message
    /// receiver.
    AllowWithWarning {
        plan: PaneSendPlan,
        reason: PaneSendReason,
        warning_key: &'static str,
    },
    /// Refuse the send. The CLI MUST surface a localized error keyed
    /// by `reason_key` and NOT dispatch any keystrokes.
    Refuse {
        reason: PaneSendReason,
        reason_key: &'static str,
        hint_key: &'static str,
    },
    /// The activity guard (`guarded_send`) blocked the send based on
    /// ACP-derived target state. This is a separate variant from
    /// `Refuse` so the CLI can surface activity-specific guidance
    /// (e.g. "target is busy with a tool call") separately from the
    /// existing safety refusals.
    GuardedSendRefused {
        reason: PaneSendReason,
        block_reason: GuardedSendBlockReason,
    },
}

/// Centralized resolver: from typed safety + user flags to a typed
/// send plan.
///
/// Inputs:
///
/// * `safety` — already-derived `PaneMessageSafety` from the
///   observation merge stack. Capturing this through a typed enum
///   instead of "pane kind + observation source + confidence" keeps
///   the rule table small and matches what
///   [`crate::domain::derive_message_safety`] already publishes.
/// * `no_enter` — user passed `--no-enter`; the CLI wants text-only
///   insertion.
/// * `force` — user passed `--force`; the CLI explicitly overrides
///   conservative refusals (TUI / unknown).
/// * `text` — literal user text. The resolver does NOT interpret
///   embedded `\n` / `/n` as a submit marker; that is a deliberate
///   product decision (see requirement, section "Product Decisions").
///
/// Output: typed [`PaneSendDecision`].
///
/// Notes for implementers:
///
///   * The resolver never logs / never touches I/O. It is a pure
///     function so the CLI can unit-test the rule table without a
///     tmux/control-plane fixture.
///   * `--force` overrides `Refuse` outcomes by promoting them to a
///     forced [`PaneSendDecision::AllowWithWarning`]. It does NOT
///     change the default `submit` for safe receivers; `--no-enter`
///     is still the only way to suppress submit.
pub fn resolve_pane_send_plan(
    safety: PaneMessageSafety,
    no_enter: bool,
    force: bool,
    text: &str,
) -> PaneSendDecision {
    use crate::i18n_keys as k;

    let plan_with_submit = PaneSendPlan {
        text: text.to_owned(),
        submit: !no_enter,
    };

    match safety {
        PaneMessageSafety::SafeStructured => {
            if no_enter {
                PaneSendDecision::Allow {
                    plan: plan_with_submit,
                    reason: PaneSendReason::InsertOnlyByFlag,
                }
            } else {
                PaneSendDecision::Allow {
                    plan: plan_with_submit,
                    reason: PaneSendReason::SafeStructuredAgent,
                }
            }
        }
        PaneMessageSafety::ShellInput => {
            // Shell panes are
            // NOT silent message receivers. Even with `--no-enter`,
            // a plain `pane send <shell> "text"` would inject typed
            // characters into a shell prompt — which the user did
            // not opt into when they ran a chat-shaped command.
            // Refuse by default; `--force` is the explicit raw-key
            // override path for advanced users.
            if force {
                PaneSendDecision::AllowWithWarning {
                    plan: plan_with_submit,
                    reason: PaneSendReason::ForcedOverride,
                    warning_key: k::PANE_SEND_WARN_FORCED_SHELL,
                }
            } else {
                PaneSendDecision::Refuse {
                    reason: PaneSendReason::ShellNotMessageReceiver,
                    reason_key: k::PANE_SEND_REFUSE_SHELL,
                    hint_key: k::PANE_SEND_REFUSE_HINT_FORCE,
                }
            }
        }
        PaneMessageSafety::UnverifiedAgent => {
            if no_enter {
                PaneSendDecision::Allow {
                    plan: plan_with_submit,
                    reason: PaneSendReason::InsertOnlyByFlag,
                }
            } else {
                PaneSendDecision::AllowWithWarning {
                    plan: plan_with_submit,
                    reason: PaneSendReason::UnverifiedAgent,
                    warning_key: k::PANE_SEND_WARN_UNVERIFIED_AGENT,
                }
            }
        }
        PaneMessageSafety::NotApplicable => {
            if force {
                PaneSendDecision::AllowWithWarning {
                    plan: plan_with_submit,
                    reason: PaneSendReason::ForcedOverride,
                    warning_key: k::PANE_SEND_WARN_FORCED_TUI,
                }
            } else {
                PaneSendDecision::Refuse {
                    reason: PaneSendReason::TuiNotMessageReceiver,
                    reason_key: k::PANE_SEND_REFUSE_TUI,
                    hint_key: k::PANE_SEND_REFUSE_HINT_FORCE,
                }
            }
        }
        PaneMessageSafety::Unknown => {
            if force {
                PaneSendDecision::AllowWithWarning {
                    plan: plan_with_submit,
                    reason: PaneSendReason::ForcedOverride,
                    warning_key: k::PANE_SEND_WARN_FORCED_UNKNOWN,
                }
            } else {
                PaneSendDecision::Refuse {
                    reason: PaneSendReason::UnknownTarget,
                    reason_key: k::PANE_SEND_REFUSE_UNKNOWN,
                    hint_key: k::PANE_SEND_REFUSE_HINT_FORCE,
                }
            }
        }
    }
}

/// Activity-guarded resolver: extends [`resolve_pane_send_plan`] with
/// an additive [`guarded_send`] check from the ACP metrics layer.
///
/// ## Additive safety guarantee
///
/// This function NEVER weakens the existing pane-send safety rules:
///
/// 1. If [`resolve_pane_send_plan`] refuses, the result is **always
///    a refusal** regardless of the guarded-send outcome. The
///    activity guard cannot promote a safety refusal to an allow.
/// 2. If [`guarded_send`] blocks, the result is
///    [`PaneSendDecision::GuardedSendRefused`] regardless of the
///    existing safety rule's allow/warn outcome.
/// 3. If both allow, the base safety outcome is returned unchanged.
/// 4. `force = true` must be explicit and must propagate to both
///    the base resolver and the guard — it is never set automatically.
///
/// ## Parameters
///
/// * `target_metrics` — optional ACP-derived metrics for the target
///   pane. When `None` (legacy pane / ACP not available), the
///   guarded check is skipped and only the base safety rules apply.
/// * `guard_force` — explicit caller opt-in to override the
///   activity guard. Setting this to `true` MUST be a deliberate
///   product choice; callers must NOT default to `true`.
///
/// All other parameters have the same semantics as
/// [`resolve_pane_send_plan`].
pub fn resolve_pane_send_plan_guarded(
    safety: PaneMessageSafety,
    no_enter: bool,
    send_force: bool,
    guard_force: bool,
    text: &str,
    target_metrics: Option<&PaneActivityMetrics>,
) -> PaneSendDecision {
    use crate::i18n_keys as k;

    // Step 1: Run the base safety resolver.
    let base = resolve_pane_send_plan(safety, no_enter, send_force, text);

    // Step 2: If the base already refuses, return it unchanged.
    // The activity guard must NOT promote a safety refusal to allow.
    if matches!(base, PaneSendDecision::Refuse { .. }) {
        return base;
    }

    // Step 3: If target_metrics is absent (legacy path), skip the
    // activity guard.
    let Some(metrics) = target_metrics else {
        return base;
    };

    // Step 4: Run the activity guard.
    match guarded_send(metrics, guard_force) {
        GuardedSendDecision::Allow => base,
        GuardedSendDecision::AllowWithForce { .. } => {
            // guard_force = true was set: allow but tag the decision
            // so the CLI can warn the caller.
            match base {
                PaneSendDecision::Allow { plan, .. } => PaneSendDecision::AllowWithWarning {
                    plan,
                    reason: PaneSendReason::ActivityGuardForcedOverride,
                    warning_key: k::PANE_SEND_WARN_ACTIVITY_GUARD_FORCED,
                },
                PaneSendDecision::AllowWithWarning { plan, .. } => {
                    PaneSendDecision::AllowWithWarning {
                        plan,
                        reason: PaneSendReason::ActivityGuardForcedOverride,
                        warning_key: k::PANE_SEND_WARN_ACTIVITY_GUARD_FORCED,
                    }
                }
                // GuardedSendRefused cannot be an input here (we haven't
                // produced it yet).
                other => other,
            }
        }
        GuardedSendDecision::Block {
            reason: block_reason,
        } => PaneSendDecision::GuardedSendRefused {
            reason: PaneSendReason::ActivityGuardBlocked,
            block_reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_allow_plain(decision: &PaneSendDecision, submit: bool, reason: PaneSendReason) {
        match decision {
            PaneSendDecision::Allow { plan, reason: r } => {
                assert_eq!(plan.submit, submit, "submit mismatch: {:?}", decision);
                assert_eq!(*r, reason, "reason mismatch: {:?}", decision);
            }
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    fn assert_allow_warn(
        decision: &PaneSendDecision,
        submit: bool,
        reason: PaneSendReason,
        warning_key: &'static str,
    ) {
        match decision {
            PaneSendDecision::AllowWithWarning {
                plan,
                reason: r,
                warning_key: wk,
            } => {
                assert_eq!(plan.submit, submit);
                assert_eq!(*r, reason);
                assert_eq!(*wk, warning_key);
            }
            other => panic!("expected AllowWithWarning, got {other:?}"),
        }
    }

    fn assert_refuse(decision: &PaneSendDecision, reason: PaneSendReason) {
        match decision {
            PaneSendDecision::Refuse { reason: r, .. } => {
                assert_eq!(*r, reason);
            }
            other => panic!("expected Refuse, got {other:?}"),
        }
    }

    /// Verified AI chat pane (Codex/Claude Code under supervisor
    /// authoritative observation) defaults to insert + submit. This is
    /// the headline acceptance case from the requirement (criterion 3).
    #[test]
    fn pane_send_safe_structured_submits_by_default() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::SafeStructured,
            /*no_enter=*/ false,
            /*force=*/ false,
            "hello",
        );
        assert_allow_plain(&decision, true, PaneSendReason::SafeStructuredAgent);
    }

    /// `--no-enter` MUST stay available as text-only behaviour
    /// (criterion 8). Even for safe message receivers, the CLI must
    /// not invent a submit when the user opts out.
    #[test]
    fn pane_send_safe_structured_no_enter_inserts_text_only() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::SafeStructured,
            /*no_enter=*/ true,
            false,
            "draft only",
        );
        assert_allow_plain(&decision, false, PaneSendReason::InsertOnlyByFlag);
    }

    /// Shell panes are NOT
    /// silent message receivers. A plain `pane send <shell> "text"`
    /// must refuse so we never inject chat-shaped text as a shell
    /// command. `--force` is the documented escape hatch.
    #[test]
    fn pane_send_shell_default_is_refused_without_force() {
        let decision =
            resolve_pane_send_plan(PaneMessageSafety::ShellInput, false, false, "ls -la");
        assert_refuse(&decision, PaneSendReason::ShellNotMessageReceiver);
        match decision {
            PaneSendDecision::Refuse {
                reason_key,
                hint_key,
                ..
            } => {
                assert_eq!(reason_key, crate::i18n_keys::PANE_SEND_REFUSE_SHELL);
                assert_eq!(hint_key, crate::i18n_keys::PANE_SEND_REFUSE_HINT_FORCE);
            }
            other => panic!("unexpected decision shape: {other:?}"),
        }
    }

    /// `--no-enter` does NOT promote a shell pane to a safe message
    /// receiver: text-only insertion still types the message into
    /// the shell prompt, which is the same hazard. The shell case
    /// still refuses without `--force`.
    #[test]
    fn pane_send_shell_with_no_enter_is_still_refused_without_force() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::ShellInput,
            /*no_enter=*/ true,
            /*force=*/ false,
            "ls -la",
        );
        assert_refuse(&decision, PaneSendReason::ShellNotMessageReceiver);
    }

    /// `--force` is the documented advanced override: shell panes
    /// receive the raw keystrokes plus a localized warning that the
    /// user explicitly bypassed the conservative refusal.
    #[test]
    fn pane_send_shell_with_force_yields_forced_override_warning() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::ShellInput,
            /*no_enter=*/ false,
            /*force=*/ true,
            "ls -la",
        );
        assert_allow_warn(
            &decision,
            true,
            PaneSendReason::ForcedOverride,
            crate::i18n_keys::PANE_SEND_WARN_FORCED_SHELL,
        );
    }

    /// Unverified agent: foreground looks like an AI agent but the
    /// supervisor handshake was not confirmed. The send is allowed
    /// (legitimate when the user manually started the agent) but
    /// MUST be flagged as unverified (criteria 7 + 11).
    #[test]
    fn pane_send_unverified_agent_warns_then_submits() {
        let decision =
            resolve_pane_send_plan(PaneMessageSafety::UnverifiedAgent, false, false, "你好");
        assert_allow_warn(
            &decision,
            true,
            PaneSendReason::UnverifiedAgent,
            crate::i18n_keys::PANE_SEND_WARN_UNVERIFIED_AGENT,
        );
    }

    /// `lazygit` / `GitUi` / `Tui` pane: chat text MUST NOT be
    /// silently injected as keystrokes (criterion 6). Without
    /// `--force`, refuse with a localized reason and a hint.
    #[test]
    fn pane_send_tui_is_refused_without_force() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::NotApplicable,
            false,
            false,
            "hello world",
        );
        assert_refuse(&decision, PaneSendReason::TuiNotMessageReceiver);
        match decision {
            PaneSendDecision::Refuse {
                reason_key,
                hint_key,
                ..
            } => {
                assert_eq!(reason_key, crate::i18n_keys::PANE_SEND_REFUSE_TUI);
                assert_eq!(hint_key, crate::i18n_keys::PANE_SEND_REFUSE_HINT_FORCE);
            }
            other => panic!("unexpected decision shape: {other:?}"),
        }
    }

    /// `--force` is the documented escape hatch for raw key sending.
    /// It promotes a TUI refusal into an explicit warning rather
    /// than silently dropping the user's input.
    #[test]
    fn pane_send_tui_with_force_yields_forced_override_warning() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::NotApplicable,
            false,
            /*force=*/ true,
            "raw keys",
        );
        assert_allow_warn(
            &decision,
            true,
            PaneSendReason::ForcedOverride,
            crate::i18n_keys::PANE_SEND_WARN_FORCED_TUI,
        );
    }

    /// Unknown safety: no reliable signal. Refuse by default; keep
    /// `--force` as the explicit override path.
    #[test]
    fn pane_send_unknown_is_refused_without_force() {
        let decision = resolve_pane_send_plan(PaneMessageSafety::Unknown, false, false, "hi");
        assert_refuse(&decision, PaneSendReason::UnknownTarget);
    }

    #[test]
    fn pane_send_unknown_with_force_yields_forced_override_warning() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::Unknown,
            false,
            /*force=*/ true,
            "hi",
        );
        assert_allow_warn(
            &decision,
            true,
            PaneSendReason::ForcedOverride,
            crate::i18n_keys::PANE_SEND_WARN_FORCED_UNKNOWN,
        );
    }

    /// `--no-enter` on a TUI is still refused: text-only insert into
    /// `lazygit` is still keystroke injection. The flag does not
    /// promote unsafe receivers to safe ones.
    #[test]
    fn pane_send_tui_with_no_enter_is_still_refused_without_force() {
        let decision = resolve_pane_send_plan(
            PaneMessageSafety::NotApplicable,
            /*no_enter=*/ true,
            /*force=*/ false,
            "no submit",
        );
        assert_refuse(&decision, PaneSendReason::TuiNotMessageReceiver);
    }

    /// Literal `\n` and `/n` MUST NOT be interpreted as a submit
    /// marker (requirement: "Product Decisions" — literal `\n`/`/n`
    /// is user text). The resolver passes the text through; submit
    /// is decided by the typed `no_enter` flag, not by content.
    #[test]
    fn pane_send_does_not_treat_literal_backslash_n_as_submit() {
        let decision_default =
            resolve_pane_send_plan(PaneMessageSafety::SafeStructured, false, false, "hi\\n");
        match decision_default {
            PaneSendDecision::Allow { plan, .. } => {
                assert_eq!(plan.text, "hi\\n", "literal backslash-n must round-trip");
                assert!(plan.submit, "submit is independent of literal text");
            }
            other => panic!("unexpected decision: {other:?}"),
        }

        let decision_no_enter =
            resolve_pane_send_plan(PaneMessageSafety::SafeStructured, true, false, "hi/n");
        match decision_no_enter {
            PaneSendDecision::Allow { plan, .. } => {
                assert_eq!(plan.text, "hi/n", "literal slash-n must round-trip");
                assert!(!plan.submit, "no_enter must suppress submit");
            }
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    // ── guarded_send integration tests ────────────────────────────

    use crate::domain::activity::{
        ObservationConfidence, PaneActivityState, PaneCorrelationKey, ProviderKind, SignalChannel,
        SignalFreshness,
    };
    use crate::usecases::activity_control_plane::metrics::{
        MetricValue, PaneActivityMetrics, UnavailableReason,
    };

    fn acp_metrics(state: PaneActivityState) -> PaneActivityMetrics {
        PaneActivityMetrics {
            pane: PaneCorrelationKey::new("g", "w", "u"),
            provider: Some(ProviderKind::Codex),
            state,
            channel: SignalChannel::LifecycleHook,
            confidence: ObservationConfidence::High,
            freshness: SignalFreshness::Fresh,
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
            acp_source: true,
        }
    }

    #[allow(dead_code)]
    fn acp_metrics_stale(state: PaneActivityState) -> PaneActivityMetrics {
        let mut m = acp_metrics(state);
        m.freshness = SignalFreshness::Stale;
        m
    }

    /// Existing pane_send safety rules still apply when guarded send
    /// is wired in.
    #[test]
    fn pane_send_existing_safety_rules_still_apply_after_guarded_send_integration() {
        // Shell pane + idle ACP target: shell refusal must win.
        let m = acp_metrics(PaneActivityState::Idle);
        let decision = resolve_pane_send_plan_guarded(
            PaneMessageSafety::ShellInput,
            false, // no_enter
            false, // send_force
            false, // guard_force
            "ls",
            Some(&m),
        );
        // Must still be a safety refuse (not GuardedSendRefused).
        assert!(
            matches!(
                decision,
                PaneSendDecision::Refuse {
                    reason: PaneSendReason::ShellNotMessageReceiver,
                    ..
                }
            ),
            "shell refusal must not be overridden by an idle ACP guard: {decision:?}"
        );
    }

    /// Guarded send refuses when the target is busy.
    #[test]
    fn pane_send_refuses_when_guarded_send_blocks() {
        let m = acp_metrics(PaneActivityState::Working);
        let decision = resolve_pane_send_plan_guarded(
            PaneMessageSafety::SafeStructured,
            false, // no_enter
            false, // send_force
            false, // guard_force
            "hello",
            Some(&m),
        );
        assert!(
            matches!(
                decision,
                PaneSendDecision::GuardedSendRefused {
                    reason: PaneSendReason::ActivityGuardBlocked,
                    block_reason: GuardedSendBlockReason::PaneBusy,
                }
            ),
            "busy target must produce GuardedSendRefused: {decision:?}"
        );
    }

    /// The resolver must NOT silently bypass the activity guard when
    /// guard_force is false — even if send_force is true.
    #[test]
    fn pane_send_does_not_silently_bypass_guarded_send_without_explicit_force() {
        let m = acp_metrics(PaneActivityState::Working);
        // send_force=true only overrides the message-safety check, not
        // the activity guard.
        let decision = resolve_pane_send_plan_guarded(
            PaneMessageSafety::SafeStructured,
            false, // no_enter
            true,  // send_force (overrides message-safety)
            false, // guard_force = false (guard NOT overridden)
            "hello",
            Some(&m),
        );
        assert!(
            matches!(
                decision,
                PaneSendDecision::GuardedSendRefused {
                    reason: PaneSendReason::ActivityGuardBlocked,
                    ..
                }
            ),
            "busy target must still be refused even with send_force=true and guard_force=false: {decision:?}"
        );
    }

    /// guard_force=true overrides the activity guard with an explicit
    /// warning, and the caller can see the force reason.
    #[test]
    fn pane_send_with_explicit_force_logs_force_reason_and_proceeds_with_existing_confirmations() {
        let m = acp_metrics(PaneActivityState::Working);
        let decision = resolve_pane_send_plan_guarded(
            PaneMessageSafety::SafeStructured,
            false, // no_enter
            false, // send_force
            true,  // guard_force = true (explicit override)
            "hello",
            Some(&m),
        );
        // Must be AllowWithWarning tagged as ActivityGuardForcedOverride.
        assert!(
            matches!(
                decision,
                PaneSendDecision::AllowWithWarning {
                    reason: PaneSendReason::ActivityGuardForcedOverride,
                    ..
                }
            ),
            "explicit guard_force must produce AllowWithWarning(ActivityGuardForcedOverride): {decision:?}"
        );
        // Verify warning_key is the activity guard specific key.
        if let PaneSendDecision::AllowWithWarning { warning_key, .. } = decision {
            assert_eq!(
                warning_key,
                crate::i18n_keys::PANE_SEND_WARN_ACTIVITY_GUARD_FORCED
            );
        }
    }
}

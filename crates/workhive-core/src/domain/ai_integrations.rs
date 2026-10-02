//! AI integration lifecycle: typed states + pure resolver + per-provider policy.
//!
//! Design notes:
//! - The lifecycle is modeled as an enum, not a hierarchy of `State` classes.
//! - `ProviderPolicy` is a compact, copyable struct (a small data table), not a
//!   trait hierarchy. Provider-specific differences (Codex `/hooks` review,
//!   OpenCode plugin model) are expressed as plain fields.
//! - `resolve_lifecycle_state` is a pure function over an `LifecycleObservation`
//!   value, so it is trivial to test exhaustively without IO.
//! - `Disabled` is NOT a lifecycle variant — feature gate decisions live in
//!   [`crate::usecases::ai_integrations`] and short-circuit *before* lifecycle
//!   resolution ever runs.

use crate::domain::ai_activity::AiActivityProvider;

/// First-class lifecycle states for a single AI provider integration.
///
/// Variants are observation-driven: each one corresponds to something the
/// system can *prove* about the provider at the call site, not to an internal
/// flag we have to remember to flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiIntegrationLifecycleState {
    /// Provider is not a recognized AI integration (e.g. `AiActivityProvider::None`).
    /// Never displayed; emitted defensively so call sites can match exhaustively.
    Unsupported,
    /// The provider's CLI is not present on `PATH` / not discoverable. Nothing
    /// to prepare and no guidance is appropriate beyond install hints.
    ProviderUnavailable,
    /// The provider is available but Workhive hooks/plugins have not been
    /// installed yet.
    NotInstalled,
    /// Last prepare attempt failed (permissions, malformed user config, etc.).
    /// Carries a stable reason key so it survives across renders.
    PrepareFailed,
    /// Workhive hook/plugin artifacts are installed and current, and the
    /// observation does not yet require pane-level event evidence to
    /// progress. Reachable when:
    /// * `hooks_installed = true`,
    /// * the provider does **not** require user-side trust review
    ///   (or `has_accurate_event = true` proves a prior review-gated event),
    /// * and `observes_pane_event = false` (provider-scoped observation,
    ///   e.g. a provider-scope diagnostic, post-prepare verification,
    ///   or pre-spawn handshake path that is not tied to one pane).
    ///
    /// Pane-scoped renders (where the caller tracks per-pane events)
    /// skip this state entirely and progress to [`PendingFirstEvent`],
    /// [`Active`], or [`Stale`] based on real event evidence — see
    /// [`resolve_lifecycle_state`] for the full transition matrix and
    /// `crates/workhive-cli/src/commands/pane.rs::resolve_pane_lifecycle_state`
    /// for the documented projection rationale.
    Installed,
    /// Hooks/plugins are installed but the provider requires user-side trust
    /// review before they will fire. Currently only Codex `/hooks` review.
    ReviewRequired,
    /// Hooks/plugins are installed and (where applicable) reviewed, but no
    /// event has been received yet — we cannot yet promote to `Active`.
    PendingFirstEvent,
    /// Hooks/plugins are installed and at least one accurate event has been
    /// received within the recency window.
    Active,
    /// Hooks/plugins were active previously but the last event is older than
    /// the staleness threshold.
    Stale,
}

impl AiIntegrationLifecycleState {
    /// True for states where it is appropriate to render guidance lines.
    /// Hidden when the provider is not a real AI integration.
    pub fn renders_guidance(self) -> bool {
        !matches!(self, Self::Unsupported)
    }

    /// True for states that should *not* upgrade fact accuracy beyond
    /// `Approximate` (no proof the provider can actually emit hook events
    /// for the current pane). `Installed` is included because, although
    /// the artifact is installed, the caller deliberately observed at
    /// provider scope and therefore has no pane-event proof of emission.
    pub fn is_pre_hook(self) -> bool {
        matches!(
            self,
            Self::ProviderUnavailable
                | Self::NotInstalled
                | Self::PrepareFailed
                | Self::Installed
                | Self::ReviewRequired
                | Self::PendingFirstEvent
        )
    }
}

/// Inputs to the pure lifecycle resolver. Built from observations — never
/// from configuration.
#[derive(Debug, Clone, Copy)]
pub struct LifecycleObservation {
    /// Which provider this observation describes.
    pub provider: AiActivityProvider,
    /// `true` iff the provider executable is discoverable.
    pub provider_available: bool,
    /// `true` iff Workhive hook/plugin artifacts are installed and current.
    pub hooks_installed: bool,
    /// `true` iff the most recent prepare attempt failed.
    pub prepare_failed: bool,
    /// `true` iff we have ever seen an accurate event from this provider for
    /// the pane under consideration.
    pub has_accurate_event: bool,
    /// `true` iff the most recent accurate event has aged beyond the
    /// staleness threshold.
    pub last_event_is_stale: bool,
    /// Scope flag distinguishing pane-level observations from
    /// provider-level (global) observations.
    ///
    /// * `true` — the caller is observing this provider on behalf of a
    ///   specific pane and has populated `has_accurate_event` /
    ///   `last_event_is_stale` from that pane's overlay. The resolver
    ///   will progress past install through
    ///   [`AiIntegrationLifecycleState::PendingFirstEvent`] ↔
    ///   [`AiIntegrationLifecycleState::Active`] ↔
    ///   [`AiIntegrationLifecycleState::Stale`].
    /// * `false` — the caller is observing at provider scope only
    ///   (e.g. a provider-scope diagnostic, post-prepare verification,
    ///   or pre-spawn handshake path that is not tied to one pane).
    ///   The resolver stays at
    ///   [`AiIntegrationLifecycleState::Installed`] once hooks are
    ///   installed and review is not blocking, rather than fabricating
    ///   pane-event transitions it has no evidence for.
    pub observes_pane_event: bool,
}

/// Pure transition: maps a single observation snapshot to a lifecycle state.
///
/// Resolution order matches reality:
///
/// 1. `Unsupported` — provider has no Workhive integration model.
/// 2. `ProviderUnavailable` — executable not discoverable.
/// 3. `PrepareFailed` — last prepare attempt failed.
/// 4. `NotInstalled` — artifacts not yet present.
/// 5. `ReviewRequired` — artifacts installed, provider requires trust
///    review (Codex `/hooks`), no event evidence proves the review has
///    been completed. Dominates over scope because review is a
///    user-facing gate independent of where we observed.
/// 6. `Installed` — artifacts installed, review (if any) satisfied or
///    not required, **and** observation is at provider scope
///    (`observes_pane_event = false`). Pane-scope observations skip
///    this branch.
/// 7. `PendingFirstEvent` / `Active` / `Stale` — pane-scope progression
///    based on event evidence and freshness.
///
/// This function intentionally has no side effects and no IO.
pub fn resolve_lifecycle_state(
    obs: LifecycleObservation,
    policy: ProviderPolicy,
) -> AiIntegrationLifecycleState {
    if obs.provider == AiActivityProvider::None {
        return AiIntegrationLifecycleState::Unsupported;
    }
    if !obs.provider_available {
        return AiIntegrationLifecycleState::ProviderUnavailable;
    }
    if obs.prepare_failed {
        return AiIntegrationLifecycleState::PrepareFailed;
    }
    if !obs.hooks_installed {
        return AiIntegrationLifecycleState::NotInstalled;
    }
    // Hooks installed.
    if policy.requires_user_review && !obs.has_accurate_event {
        // Providers that require user-side trust review (Codex) stay in
        // `ReviewRequired` until proof of activation arrives (which can
        // only happen post-review). This avoids fabricating `Installed`
        // or `Active` states we cannot verify, and applies regardless
        // of observation scope.
        return AiIntegrationLifecycleState::ReviewRequired;
    }
    if !obs.observes_pane_event {
        // Provider-scope observation: artifacts are installed and
        // review (if any) is satisfied. The caller is not tracking
        // pane-level events, so the most we can truthfully report is
        // `Installed`. Promoting to `PendingFirstEvent` here would
        // pretend we are watching a pane we are not.
        return AiIntegrationLifecycleState::Installed;
    }
    if !obs.has_accurate_event {
        return AiIntegrationLifecycleState::PendingFirstEvent;
    }
    if obs.last_event_is_stale {
        AiIntegrationLifecycleState::Stale
    } else {
        AiIntegrationLifecycleState::Active
    }
}

/// Per-provider policy. Plain data — no trait objects, no inheritance.
///
/// Adding a provider means adding one row to [`policy_for`] and one variant
/// to [`AiActivityProvider`]; everything else stays generic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPolicy {
    /// Stable identifier used in TOML keys, log fields and tests.
    pub id: &'static str,
    /// Display name suitable for guidance text (in English; localizers may
    /// translate guidance strings around it).
    pub display_name: &'static str,
    /// `true` for providers that require the user to approve hooks in a
    /// dedicated trust UI before they fire (currently only Codex).
    pub requires_user_review: bool,
    /// `true` when the integration uses plugin files instead of shell hook
    /// scripts (currently only OpenCode).
    pub uses_plugin_model: bool,
}

/// Lookup policy by provider. Returns a defensive policy for
/// `AiActivityProvider::None` so callers can match exhaustively without
/// special-casing the `None` branch.
pub fn policy_for(provider: AiActivityProvider) -> ProviderPolicy {
    match provider {
        AiActivityProvider::ClaudeCode => ProviderPolicy {
            id: "claude_code",
            display_name: "Claude Code",
            requires_user_review: false,
            uses_plugin_model: false,
        },
        AiActivityProvider::Codex => ProviderPolicy {
            id: "codex",
            display_name: "Codex",
            requires_user_review: true,
            uses_plugin_model: false,
        },
        AiActivityProvider::Opencode => ProviderPolicy {
            id: "opencode",
            display_name: "OpenCode",
            requires_user_review: false,
            uses_plugin_model: true,
        },
        AiActivityProvider::None => ProviderPolicy {
            id: "none",
            display_name: "—",
            requires_user_review: false,
            uses_plugin_model: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default observation: pane-scoped (`observes_pane_event = true`) so
    /// existing tests that assert pane-event transitions continue to work
    /// unchanged. Provider-scope tests opt out by setting the field to
    /// `false` explicitly, which keeps the rare scope intentional and
    /// reviewable rather than implicit.
    fn obs_for(provider: AiActivityProvider) -> LifecycleObservation {
        LifecycleObservation {
            provider,
            provider_available: true,
            hooks_installed: false,
            prepare_failed: false,
            has_accurate_event: false,
            last_event_is_stale: false,
            observes_pane_event: true,
        }
    }

    #[test]
    fn none_provider_is_unsupported() {
        let state = resolve_lifecycle_state(
            obs_for(AiActivityProvider::None),
            policy_for(AiActivityProvider::None),
        );
        assert_eq!(state, AiIntegrationLifecycleState::Unsupported);
    }

    #[test]
    fn missing_executable_is_provider_unavailable() {
        let mut o = obs_for(AiActivityProvider::ClaudeCode);
        o.provider_available = false;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::ClaudeCode));
        assert_eq!(state, AiIntegrationLifecycleState::ProviderUnavailable);
    }

    #[test]
    fn prepare_failed_dominates_not_installed() {
        let mut o = obs_for(AiActivityProvider::Codex);
        o.prepare_failed = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Codex));
        assert_eq!(state, AiIntegrationLifecycleState::PrepareFailed);
    }

    #[test]
    fn claude_code_jumps_to_pending_first_event_after_install() {
        let mut o = obs_for(AiActivityProvider::ClaudeCode);
        o.hooks_installed = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::ClaudeCode));
        assert_eq!(state, AiIntegrationLifecycleState::PendingFirstEvent);
    }

    #[test]
    fn codex_requires_review_before_event() {
        let mut o = obs_for(AiActivityProvider::Codex);
        o.hooks_installed = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Codex));
        assert_eq!(state, AiIntegrationLifecycleState::ReviewRequired);
    }

    #[test]
    fn codex_promotes_past_review_after_first_event() {
        let mut o = obs_for(AiActivityProvider::Codex);
        o.hooks_installed = true;
        o.has_accurate_event = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Codex));
        assert_eq!(state, AiIntegrationLifecycleState::Active);
    }

    #[test]
    fn active_decays_to_stale_when_last_event_is_old() {
        let mut o = obs_for(AiActivityProvider::ClaudeCode);
        o.hooks_installed = true;
        o.has_accurate_event = true;
        o.last_event_is_stale = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::ClaudeCode));
        assert_eq!(state, AiIntegrationLifecycleState::Stale);
    }

    #[test]
    fn opencode_uses_plugin_model_in_policy() {
        let p = policy_for(AiActivityProvider::Opencode);
        assert!(p.uses_plugin_model);
        assert!(!p.requires_user_review);
    }

    #[test]
    fn policy_ids_are_stable() {
        assert_eq!(policy_for(AiActivityProvider::ClaudeCode).id, "claude_code");
        assert_eq!(policy_for(AiActivityProvider::Codex).id, "codex");
        assert_eq!(policy_for(AiActivityProvider::Opencode).id, "opencode");
        assert_eq!(policy_for(AiActivityProvider::None).id, "none");
    }

    #[test]
    fn renders_guidance_is_false_only_for_unsupported() {
        assert!(!AiIntegrationLifecycleState::Unsupported.renders_guidance());
        for s in [
            AiIntegrationLifecycleState::ProviderUnavailable,
            AiIntegrationLifecycleState::NotInstalled,
            AiIntegrationLifecycleState::PrepareFailed,
            AiIntegrationLifecycleState::Installed,
            AiIntegrationLifecycleState::ReviewRequired,
            AiIntegrationLifecycleState::PendingFirstEvent,
            AiIntegrationLifecycleState::Active,
            AiIntegrationLifecycleState::Stale,
        ] {
            assert!(s.renders_guidance());
        }
    }

    #[test]
    fn is_pre_hook_excludes_active_and_stale() {
        assert!(!AiIntegrationLifecycleState::Active.is_pre_hook());
        assert!(!AiIntegrationLifecycleState::Stale.is_pre_hook());
        assert!(AiIntegrationLifecycleState::ReviewRequired.is_pre_hook());
        assert!(AiIntegrationLifecycleState::PendingFirstEvent.is_pre_hook());
        assert!(AiIntegrationLifecycleState::Installed.is_pre_hook());
    }

    // ── `Installed` is a real, reachable state ──
    //
    // The product contract advertises `Installed` as a first-class
    // lifecycle vocabulary member. These tests pin the reachability and
    // the boundaries against `ReviewRequired` (Codex) and
    // `PendingFirstEvent` (pane-scope progression) so the variant cannot
    // silently regress to dead code.

    /// Claude Code: post-install verification at provider scope must
    /// resolve to `Installed`. Claude does not require user review, so
    /// once hooks are present and we are not in a pane-event tracking
    /// context the resolver intentionally stops at `Installed`.
    #[test]
    fn claude_code_resolves_to_installed_at_provider_scope() {
        let mut o = obs_for(AiActivityProvider::ClaudeCode);
        o.hooks_installed = true;
        o.observes_pane_event = false;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::ClaudeCode));
        assert_eq!(state, AiIntegrationLifecycleState::Installed);
    }

    /// OpenCode: same provider-scope semantics — Workhive plugin
    /// artifact installed, no review needed, no pane events being
    /// tracked → `Installed`.
    #[test]
    fn opencode_resolves_to_installed_at_provider_scope() {
        let mut o = obs_for(AiActivityProvider::Opencode);
        o.hooks_installed = true;
        o.observes_pane_event = false;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Opencode));
        assert_eq!(state, AiIntegrationLifecycleState::Installed);
    }

    /// Codex: review gate dominates scope. Even at provider scope,
    /// hooks installed without any accepted event evidence resolves to
    /// `ReviewRequired`, not `Installed`. This preserves the
    /// Codex `/hooks` UX guarantee.
    #[test]
    fn codex_at_provider_scope_stays_review_required_without_event() {
        let mut o = obs_for(AiActivityProvider::Codex);
        o.hooks_installed = true;
        o.observes_pane_event = false;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Codex));
        assert_eq!(state, AiIntegrationLifecycleState::ReviewRequired);
    }

    /// Codex after a fresh review-gated event, observed at provider
    /// scope: review is satisfied (event proves trust), but no pane
    /// scope means we report `Installed` rather than fabricating a
    /// pane-level `Active`.
    #[test]
    fn codex_at_provider_scope_resolves_to_installed_after_review_event() {
        let mut o = obs_for(AiActivityProvider::Codex);
        o.hooks_installed = true;
        o.has_accurate_event = true;
        o.observes_pane_event = false;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Codex));
        assert_eq!(state, AiIntegrationLifecycleState::Installed);
    }

    /// Pane-scope observation must skip the `Installed` branch
    /// entirely: the same Claude observation flipped to
    /// `observes_pane_event = true` returns `PendingFirstEvent`. This
    /// pins the contract that `Installed` and `PendingFirstEvent` are
    /// distinguished by observation scope, not by provider identity.
    #[test]
    fn pane_scope_skips_installed_for_claude_with_no_event() {
        let mut o = obs_for(AiActivityProvider::ClaudeCode);
        o.hooks_installed = true;
        o.observes_pane_event = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::ClaudeCode));
        assert_eq!(state, AiIntegrationLifecycleState::PendingFirstEvent);
    }

    /// Pane-scope observation with a fresh event continues to promote
    /// past `Installed` to `Active`, proving the post-install branches
    /// are not collapsed by the new variant.
    #[test]
    fn pane_scope_with_fresh_event_promotes_to_active_not_installed() {
        let mut o = obs_for(AiActivityProvider::Opencode);
        o.hooks_installed = true;
        o.has_accurate_event = true;
        o.observes_pane_event = true;
        let state = resolve_lifecycle_state(o, policy_for(AiActivityProvider::Opencode));
        assert_eq!(state, AiIntegrationLifecycleState::Active);
    }
}

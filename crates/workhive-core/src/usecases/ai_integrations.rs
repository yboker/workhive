//! AI integration feature gate.
//!
//! This module is a *pure* resolver: feeding it config produces a gate
//! value that callers query during prepare, supervisor spawn, and pane
//! render. There is no IO and no logging here.
//!
//! Built-in defaults:
//! - `features.ai_integrations.enabled` = `true`
//! - `features.ai_integrations.providers.<id>.enabled` = `true`
//!
//! Merge semantics for the gate match `AppConfig::merge` — project values
//! override global values, but resolution does not need to know which side
//! authored a key once `merge_configs` has been applied.

use crate::domain::{AiActivityProvider, policy_for};
use crate::ports::{
    AiIntegrationProviderConfig, AiIntegrationsConfig, AppConfig, CodexHookDetail, FeaturesConfig,
};

const AI_INTEGRATIONS_ENABLED_DEFAULT: bool = true;
const AI_PROVIDER_ENABLED_DEFAULT: bool = true;
const CODEX_HOOK_DETAIL_DEFAULT: CodexHookDetail = CodexHookDetail::Full;
const CODEX_NOTIFY_ENABLED_DEFAULT: bool = false;
const TERMINAL_STATUS_ENABLED_DEFAULT: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedAiIntegrationProviderConfig {
    enabled: bool,
    hook_detail: CodexHookDetail,
    notify_enabled: bool,
}

impl ResolvedAiIntegrationProviderConfig {
    const fn builtin_default() -> Self {
        Self {
            enabled: AI_PROVIDER_ENABLED_DEFAULT,
            hook_detail: CODEX_HOOK_DETAIL_DEFAULT,
            notify_enabled: CODEX_NOTIFY_ENABLED_DEFAULT,
        }
    }

    fn from_config(config: Option<&AiIntegrationProviderConfig>) -> Self {
        let defaults = Self::builtin_default();
        Self {
            enabled: config.and_then(|c| c.enabled).unwrap_or(defaults.enabled),
            hook_detail: config
                .and_then(|c| c.hook_detail)
                .unwrap_or(defaults.hook_detail),
            notify_enabled: config
                .and_then(|c| c.notify.as_ref())
                .and_then(|n| n.enabled)
                .unwrap_or(defaults.notify_enabled),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedAiIntegrationsConfig {
    enabled: bool,
    claude_code: ResolvedAiIntegrationProviderConfig,
    codex: ResolvedAiIntegrationProviderConfig,
    opencode: ResolvedAiIntegrationProviderConfig,
}

impl ResolvedAiIntegrationsConfig {
    const fn builtin_default() -> Self {
        Self {
            enabled: AI_INTEGRATIONS_ENABLED_DEFAULT,
            claude_code: ResolvedAiIntegrationProviderConfig::builtin_default(),
            codex: ResolvedAiIntegrationProviderConfig::builtin_default(),
            opencode: ResolvedAiIntegrationProviderConfig::builtin_default(),
        }
    }

    fn from_app_config(merged: Option<&AppConfig>) -> Self {
        Self::from_features(merged.and_then(|c| c.features.as_ref()))
    }

    fn from_features(features: Option<&FeaturesConfig>) -> Self {
        Self::from_config(features.and_then(|f| f.ai_integrations.as_ref()))
    }

    fn from_config(config: Option<&AiIntegrationsConfig>) -> Self {
        let defaults = Self::builtin_default();
        let providers = config.map(|c| &c.providers);
        Self {
            enabled: config.and_then(|c| c.enabled).unwrap_or(defaults.enabled),
            claude_code: ResolvedAiIntegrationProviderConfig::from_config(
                providers.and_then(|p| p.claude_code.as_ref()),
            ),
            codex: ResolvedAiIntegrationProviderConfig::from_config(
                providers.and_then(|p| p.codex.as_ref()),
            ),
            opencode: ResolvedAiIntegrationProviderConfig::from_config(
                providers.and_then(|p| p.opencode.as_ref()),
            ),
        }
    }

    const fn feature_gate(self) -> FeatureGate {
        FeatureGate {
            globally_enabled: self.enabled,
            claude_code_enabled: self.claude_code.enabled,
            codex_enabled: self.codex.enabled,
            opencode_enabled: self.opencode.enabled,
        }
    }

    const fn codex_feature_config(self) -> CodexFeatureConfig {
        CodexFeatureConfig {
            hook_detail: self.codex.hook_detail,
            notify_enabled: self.codex.notify_enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedPaneObservationConfig {
    terminal_status_enabled: bool,
}

impl ResolvedPaneObservationConfig {
    const fn builtin_default() -> Self {
        Self {
            terminal_status_enabled: TERMINAL_STATUS_ENABLED_DEFAULT,
        }
    }

    fn from_app_config(merged: Option<&AppConfig>) -> Self {
        let defaults = Self::builtin_default();
        Self {
            terminal_status_enabled: merged
                .and_then(|c| c.features.as_ref())
                .and_then(|f| f.pane_observation.as_ref())
                .and_then(|p| p.terminal_status.as_ref())
                .and_then(|t| t.enabled)
                .unwrap_or(defaults.terminal_status_enabled),
        }
    }

    const fn pane_observation_gate(self) -> PaneObservationGate {
        PaneObservationGate {
            terminal_status_enabled: self.terminal_status_enabled,
        }
    }
}

/// Resolved feature gate for the AI integration lifecycle.
///
/// Cheap to clone, cheap to query. Construct via [`FeatureGate::from_config`]
/// (preferred) or [`FeatureGate::builtin_default`] when no `AppConfig` is
/// available in a code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureGate {
    globally_enabled: bool,
    claude_code_enabled: bool,
    codex_enabled: bool,
    opencode_enabled: bool,
}

impl FeatureGate {
    /// Built-in default: everything enabled.
    ///
    /// Use this only on code paths that legitimately have no `AppConfig`
    /// (e.g. early bootstrap or tests). Production call sites should resolve
    /// from the merged config.
    pub const fn builtin_default() -> Self {
        Self {
            globally_enabled: true,
            claude_code_enabled: true,
            codex_enabled: true,
            opencode_enabled: true,
        }
    }

    /// Resolve from a *merged* `AppConfig` (project-over-global already
    /// applied). Treat absent tables and absent keys as "enabled".
    pub fn from_config(merged: Option<&AppConfig>) -> Self {
        ResolvedAiIntegrationsConfig::from_app_config(merged).feature_gate()
    }

    /// Resolve from a `FeaturesConfig` directly. Convenient for tests
    /// and for callers that already extracted the section.
    pub fn from_features(features: Option<&FeaturesConfig>) -> Self {
        ResolvedAiIntegrationsConfig::from_features(features).feature_gate()
    }

    /// True iff the global switch is on.
    pub fn globally_enabled(&self) -> bool {
        self.globally_enabled
    }

    /// True iff this provider is currently allowed to participate in the
    /// AI integration lifecycle: prepare, hook activity ingestion, guidance
    /// rendering. Per-provider override is AND-ed with the global switch.
    ///
    /// `AiActivityProvider::None` is never gated by this function — it does
    /// not represent a real provider and the global switch should not block
    /// the degenerate "no integration" path.
    pub fn is_provider_enabled(&self, provider: AiActivityProvider) -> bool {
        if matches!(provider, AiActivityProvider::None) {
            return true;
        }
        if !self.globally_enabled {
            return false;
        }
        match provider {
            AiActivityProvider::ClaudeCode => self.claude_code_enabled,
            AiActivityProvider::Codex => self.codex_enabled,
            AiActivityProvider::Opencode => self.opencode_enabled,
            AiActivityProvider::None => true,
        }
    }

    /// Construct a gate directly from individual switches.
    ///
    /// Intended for tests and a small number of programmatic callers
    /// (CLI surfaces that want to render a hypothetical "what if the
    /// user disabled X" view). Production code should resolve through
    /// [`FeatureGate::from_config`] or [`FeatureGate::builtin_default`]
    /// so config inheritance stays consistent with everywhere else.
    pub const fn from_switches(
        globally_enabled: bool,
        claude_code_enabled: bool,
        codex_enabled: bool,
        opencode_enabled: bool,
    ) -> Self {
        Self {
            globally_enabled,
            claude_code_enabled,
            codex_enabled,
            opencode_enabled,
        }
    }

    /// Iterate over the known AI providers the gate currently permits.
    /// Useful for `init --global` opportunistic prepare.
    pub fn enabled_providers(&self) -> impl Iterator<Item = AiActivityProvider> + '_ {
        [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ]
        .into_iter()
        .filter(move |p| self.is_provider_enabled(*p))
    }
}

/// Resolved Codex-specific feature configuration.
///
/// Constructed from the merged `AppConfig` by
/// [`CodexFeatureConfig::from_config`]. Carries the resolved (non-optional)
/// decisions that business logic consumes, not `Option<T>` fields.
///
/// Both `hook_detail` and `notify_enabled` default to the safe built-in
/// defaults when the config table is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexFeatureConfig {
    /// Resolved hook detail level. Default: `Full`.
    pub hook_detail: CodexHookDetail,
    /// Whether the Codex notify fallback is enabled. Default: `false`.
    pub notify_enabled: bool,
}

impl CodexFeatureConfig {
    /// Built-in defaults: full hook set, notify disabled.
    pub const fn builtin_default() -> Self {
        Self {
            hook_detail: CODEX_HOOK_DETAIL_DEFAULT,
            notify_enabled: CODEX_NOTIFY_ENABLED_DEFAULT,
        }
    }

    /// Resolve from a *merged* `AppConfig`. Uses built-in defaults when
    /// the provider config is absent or fields are unset.
    pub fn from_config(merged: Option<&AppConfig>) -> Self {
        ResolvedAiIntegrationsConfig::from_app_config(merged).codex_feature_config()
    }
}

impl Default for CodexFeatureConfig {
    fn default() -> Self {
        Self::builtin_default()
    }
}

impl Default for FeatureGate {
    fn default() -> Self {
        Self::builtin_default()
    }
}

/// Typed outcome of resolving the AI integration feature gate from one or
/// more `AppConfig` sources.
///
/// The AI integration contract treats an *unknown* gate as **not enabled**:
/// when global or project config cannot be read, parsed, or merged, we
/// cannot prove the user intended AI integrations to be on. Write paths
/// (init/spawn prepare) MUST skip; display paths MUST suppress hook/plugin
/// guidance and `/hooks` prompts. Only resolution success carries an
/// authoritative [`FeatureGate`].
///
/// This is *not* a lifecycle state — disabled-by-config and
/// unavailable-gate are gate decisions, not provider states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureGateResolution {
    /// Config was successfully read (or genuinely absent, falling back
    /// to documented built-in defaults) and parsed into a gate.
    Resolved(FeatureGate),
    /// Config could not be loaded/parsed and the gate cannot be trusted.
    /// `reason` is a short, English diagnostic string suitable for
    /// `tracing::debug!`; user-facing surfaces must NOT print it.
    Unavailable { reason: String },
}

impl FeatureGateResolution {
    /// Borrow the resolved gate, if any.
    pub fn resolved_gate(&self) -> Option<&FeatureGate> {
        match self {
            Self::Resolved(g) => Some(g),
            Self::Unavailable { .. } => None,
        }
    }

    /// Whether resolution succeeded.
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Resolved(_))
    }

    /// Diagnostic reason for unavailable resolution, for logs only.
    pub fn unavailable_reason(&self) -> Option<&str> {
        match self {
            Self::Unavailable { reason } => Some(reason.as_str()),
            Self::Resolved(_) => None,
        }
    }

    /// Fail-closed gate view suitable for *display* paths.
    ///
    /// Returns the resolved gate when available. When unavailable, returns
    /// a gate with the global switch and every provider switch set to
    /// `false`. This makes downstream rendering (which already honors
    /// `is_provider_enabled`) silently suppress hook/plugin labels and
    /// hints without us having to thread `Option<FeatureGate>` through
    /// every render helper.
    ///
    /// Write paths must NOT use this — they must check
    /// [`Self::is_available`] explicitly and skip prepare on
    /// `Unavailable`.
    pub fn effective_display_gate(&self) -> FeatureGate {
        match self {
            Self::Resolved(g) => *g,
            Self::Unavailable { .. } => FeatureGate::from_switches(false, false, false, false),
        }
    }
}

/// Convenience: stable provider id (`claude_code`, `codex`, `opencode`).
/// Mirrors `policy_for(provider).id` so feature-gate callers don't need to
/// import `policy_for` just to format a log field.
pub fn provider_id(provider: AiActivityProvider) -> &'static str {
    policy_for(provider).id
}

/// Resolved pane-observation feature gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneObservationGate {
    terminal_status_enabled: bool,
}

impl PaneObservationGate {
    pub const fn builtin_default() -> Self {
        Self {
            terminal_status_enabled: TERMINAL_STATUS_ENABLED_DEFAULT,
        }
    }

    pub fn from_config(merged: Option<&AppConfig>) -> Self {
        ResolvedPaneObservationConfig::from_app_config(merged).pane_observation_gate()
    }

    pub const fn from_switches(terminal_status_enabled: bool) -> Self {
        Self {
            terminal_status_enabled,
        }
    }

    pub fn terminal_status_enabled(&self) -> bool {
        self.terminal_status_enabled
    }
}

impl Default for PaneObservationGate {
    fn default() -> Self {
        Self::builtin_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };

    fn features_with(
        global_enabled: Option<bool>,
        claude: Option<bool>,
        codex: Option<bool>,
        opencode: Option<bool>,
    ) -> FeaturesConfig {
        FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: global_enabled,
                providers: AiIntegrationProvidersConfig {
                    claude_code: claude.map(|e| AiIntegrationProviderConfig {
                        enabled: Some(e),
                        ..Default::default()
                    }),
                    codex: codex.map(|e| AiIntegrationProviderConfig {
                        enabled: Some(e),
                        ..Default::default()
                    }),
                    opencode: opencode.map(|e| AiIntegrationProviderConfig {
                        enabled: Some(e),
                        ..Default::default()
                    }),
                },
            }),
            pane_observation: None,
            notifications: None,
        }
    }

    #[test]
    fn builtin_default_enables_everything() {
        let gate = FeatureGate::builtin_default();
        assert!(gate.globally_enabled());
        for p in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ] {
            assert!(
                gate.is_provider_enabled(p),
                "{p:?} should be enabled by default"
            );
        }
    }

    #[test]
    fn empty_features_table_treated_as_default() {
        let features = FeaturesConfig::default();
        let gate = FeatureGate::from_features(Some(&features));
        assert!(gate.globally_enabled());
        assert!(gate.is_provider_enabled(AiActivityProvider::Codex));
    }

    #[test]
    fn global_disable_suppresses_all_providers() {
        let features = features_with(Some(false), None, None, None);
        let gate = FeatureGate::from_features(Some(&features));
        assert!(!gate.globally_enabled());
        for p in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ] {
            assert!(!gate.is_provider_enabled(p));
        }
        assert_eq!(gate.enabled_providers().count(), 0);
    }

    #[test]
    fn provider_disable_only_affects_that_provider() {
        let features = features_with(None, None, Some(false), None);
        let gate = FeatureGate::from_features(Some(&features));
        assert!(gate.globally_enabled());
        assert!(gate.is_provider_enabled(AiActivityProvider::ClaudeCode));
        assert!(!gate.is_provider_enabled(AiActivityProvider::Codex));
        assert!(gate.is_provider_enabled(AiActivityProvider::Opencode));
    }

    #[test]
    fn explicit_global_false_overrides_provider_true() {
        let features = features_with(Some(false), Some(true), Some(true), Some(true));
        let gate = FeatureGate::from_features(Some(&features));
        for p in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ] {
            assert!(!gate.is_provider_enabled(p));
        }
    }

    #[test]
    fn none_provider_is_not_gated() {
        let features = features_with(Some(false), None, None, None);
        let gate = FeatureGate::from_features(Some(&features));
        // `None` is not a real integration; the gate never blocks it.
        assert!(gate.is_provider_enabled(AiActivityProvider::None));
    }

    #[test]
    fn from_config_uses_merged_appconfig() {
        let merged = AppConfig {
            features: Some(features_with(None, Some(false), None, None)),
            ..Default::default()
        };
        let gate = FeatureGate::from_config(Some(&merged));
        assert!(!gate.is_provider_enabled(AiActivityProvider::ClaudeCode));
        assert!(gate.is_provider_enabled(AiActivityProvider::Codex));
    }

    #[test]
    fn provider_id_is_stable() {
        assert_eq!(provider_id(AiActivityProvider::ClaudeCode), "claude_code");
        assert_eq!(provider_id(AiActivityProvider::Codex), "codex");
        assert_eq!(provider_id(AiActivityProvider::Opencode), "opencode");
    }

    #[test]
    fn resolution_resolved_exposes_gate() {
        let resolution = FeatureGateResolution::Resolved(FeatureGate::builtin_default());
        assert!(resolution.is_available());
        assert!(resolution.unavailable_reason().is_none());
        assert_eq!(
            resolution.resolved_gate().copied(),
            Some(FeatureGate::builtin_default()),
        );
    }

    #[test]
    fn resolution_unavailable_carries_reason() {
        let resolution = FeatureGateResolution::Unavailable {
            reason: "config parse error: bad TOML at line 3".to_owned(),
        };
        assert!(!resolution.is_available());
        assert!(resolution.resolved_gate().is_none());
        assert_eq!(
            resolution.unavailable_reason(),
            Some("config parse error: bad TOML at line 3"),
        );
    }

    #[test]
    fn unavailable_effective_display_gate_disables_everything() {
        let resolution = FeatureGateResolution::Unavailable {
            reason: "unreadable global config".to_owned(),
        };
        let gate = resolution.effective_display_gate();
        assert!(!gate.globally_enabled());
        for p in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ] {
            assert!(
                !gate.is_provider_enabled(p),
                "Unavailable display gate must fail closed for {p:?}",
            );
        }
        // The unsupported variant must still not be blocked — the
        // existing `is_provider_enabled` contract for the `None`
        // provider is unchanged.
        assert!(gate.is_provider_enabled(AiActivityProvider::None));
    }

    #[test]
    fn resolved_effective_display_gate_preserves_per_provider_switches() {
        let features = features_with(None, Some(true), Some(false), Some(true));
        let inner = FeatureGate::from_features(Some(&features));
        let resolution = FeatureGateResolution::Resolved(inner);
        let gate = resolution.effective_display_gate();
        assert!(gate.is_provider_enabled(AiActivityProvider::ClaudeCode));
        assert!(!gate.is_provider_enabled(AiActivityProvider::Codex));
        assert!(gate.is_provider_enabled(AiActivityProvider::Opencode));
    }
}

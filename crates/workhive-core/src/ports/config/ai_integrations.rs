#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// `[features.ai_integrations]` table.
///
/// The integration lifecycle is gated by a global switch plus an
/// optional per-provider override. Built-in default is "enabled".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiIntegrationsConfig {
    /// Global switch. Built-in default when `None`: `true`.
    pub enabled: Option<bool>,

    /// Per-provider overrides keyed by stable provider id
    /// (`claude_code`, `codex`, `opencode`). Unknown keys are
    /// preserved by serde and ignored at runtime.
    #[serde(default)]
    pub providers: AiIntegrationProvidersConfig,
}

impl AiIntegrationsConfig {
    /// Validate this section. Validates all provider-specific fields
    /// (e.g. `hook_detail`) so invalid values fail closed at load time.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        // Note: hook_detail is validated at deserialization time by CodexHookDetail's
        // Deserialize impl. This validate() is a secondary defense for any runtime
        // construction that bypasses serde. Currently a no-op structural pass.
        Ok(())
    }

    /// Merge with project overrides. Project `enabled` wins when set;
    /// per-provider entries are merged key-by-key with project precedence.
    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                enabled: p.enabled.or(g.enabled),
                providers: AiIntegrationProvidersConfig::merge(&g.providers, &p.providers),
            }),
        }
    }
}

/// Per-provider gate map: `[features.ai_integrations.providers.<id>]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiIntegrationProvidersConfig {
    pub claude_code: Option<AiIntegrationProviderConfig>,
    pub codex: Option<AiIntegrationProviderConfig>,
    pub opencode: Option<AiIntegrationProviderConfig>,
}

impl AiIntegrationProvidersConfig {
    fn merge(global: &Self, project: &Self) -> Self {
        Self {
            claude_code: merge_provider(global.claude_code.as_ref(), project.claude_code.as_ref()),
            codex: merge_provider(global.codex.as_ref(), project.codex.as_ref()),
            opencode: merge_provider(global.opencode.as_ref(), project.opencode.as_ref()),
        }
    }
}

fn merge_provider(
    global: Option<&AiIntegrationProviderConfig>,
    project: Option<&AiIntegrationProviderConfig>,
) -> Option<AiIntegrationProviderConfig> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => Some(AiIntegrationProviderConfig {
            enabled: p.enabled.or(g.enabled),
            hook_detail: p.hook_detail.or(g.hook_detail),
            notify: merge_codex_notify(g.notify.as_ref(), p.notify.as_ref()),
        }),
    }
}

/// `[features.ai_integrations.providers.codex]` hook detail level.
///
/// Controls the set of Codex lifecycle events Workhive registers hooks for.
///
/// - `Full` (default): registers the full current event set —
///   `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
///   `PermissionRequest`, `Stop`. This preserves backward-compatible
///   activity coverage and must remain the default until tests prove
///   that the minimal set causes no activity regression.
/// - `Minimal`: registers a reduced set that still supports hook-activation
///   detection and `pane list/show` state projection, but omits
///   tool-level events that are not required for the basic
///   working/idle/completed state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodexHookDetail {
    /// Full event set (default): all six Codex hook events registered.
    /// Preserved as default because no activity-regression test has been
    /// run to prove that removing `PreToolUse`/`PostToolUse` is safe.
    /// Flip the default only after tests confirm no regression.
    #[default]
    Full,
    /// Reduced event set: `SessionStart`, `UserPromptSubmit`,
    /// `PermissionRequest`, `Stop`. Omits `PreToolUse`/`PostToolUse`.
    /// Only use after verifying hook-activation detection and pane
    /// state projection still work with this reduced set.
    Minimal,
}

impl CodexHookDetail {
    /// Parse from a TOML string value. Returns `None` for unrecognized values
    /// so callers can emit a typed `ConfigError::InvalidValue` with context.
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s.trim() {
            "full" => Some(Self::Full),
            "minimal" => Some(Self::Minimal),
            _ => None,
        }
    }

    /// Canonical string representation (matches the TOML enum values).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Minimal => "minimal",
        }
    }
}

impl std::fmt::Display for CodexHookDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Serde helper for `CodexHookDetail` — stored/loaded as a string in TOML.
///
/// Invalid string values fail deserialization with a structured error so
/// `AppConfig::validate` can surface a typed `ConfigError::InvalidValue`.
impl Serialize for CodexHookDetail {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CodexHookDetail {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str_value(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid hook_detail '{s}': expected \"full\" or \"minimal\""
            ))
        })
    }
}

/// `[features.ai_integrations.providers.<id>]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiIntegrationProviderConfig {
    /// Built-in default when `None`: `true`.
    pub enabled: Option<bool>,

    /// Codex-specific hook detail level. Only meaningful for the `codex`
    /// provider entry. Default (`None`) resolves to `CodexHookDetail::Full`.
    ///
    /// `minimal` must only become the default after tests prove no activity
    /// regression. The current default is `full` (backward-compatible).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hook_detail: Option<CodexHookDetail>,

    /// Codex notify fallback configuration. Only meaningful for the `codex`
    /// provider entry. Default (`None`) means "not configured".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<CodexNotifyConfig>,
}

impl AiIntegrationProviderConfig {
    /// Resolve the effective `CodexHookDetail` for this provider entry.
    /// Returns `CodexHookDetail::Full` when the field is absent (safe default).
    pub fn resolved_hook_detail(&self) -> CodexHookDetail {
        self.hook_detail.unwrap_or(CodexHookDetail::Full)
    }
}

/// `[features.ai_integrations.providers.codex.notify]` sub-table.
///
/// Controls whether Workhive installs/manages a Codex `notify` command as a
/// completion-only medium-confidence fallback signal channel.
///
/// Contract:
/// - Disabled by default (`enabled = None` → built-in default `false`).
/// - When enabled, Workhive installs a sentinel-owned wrapper notify command
///   that forwards to the user's pre-existing notify command (if any) and
///   records a `NotifyCompletion` observation in the ACP.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexNotifyConfig {
    /// Whether the Codex notify fallback is enabled. Built-in default: `false`.
    ///
    /// Disabled means completely silent: no writes, no repairs, no diagnostics,
    /// no guidance unless the user explicitly inspects this feature.
    pub enabled: Option<bool>,
}

impl CodexNotifyConfig {
    /// Whether the notify fallback is effectively enabled (defaults to `false`).
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

fn merge_codex_notify(
    global: Option<&CodexNotifyConfig>,
    project: Option<&CodexNotifyConfig>,
) -> Option<CodexNotifyConfig> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => Some(CodexNotifyConfig {
            enabled: p.enabled.or(g.enabled),
        }),
    }
}

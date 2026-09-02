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

/// Top-level `[pane]` runtime behavior table.
///
/// This is a runtime-policy table (how managed panes behave), not a
/// feature gate (which lives under `[features.*]`). Distinct from the
/// layout-scoped `PaneConfig`.
///
/// Shape:
///
/// ```toml
/// [pane.exit]
/// tool_exit_behavior = "respawn_shell"
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRuntimeConfig {
    /// `[pane.exit]` — managed tool exit policy.
    ///
    /// `None` means the sub-table was absent and built-in defaults
    /// apply (`tool_exit_behavior = respawn_shell`).
    pub exit: Option<PaneExitConfig>,
}

impl PaneRuntimeConfig {
    /// Validate nested tables. Currently a no-op surface (the enum
    /// serde already rejects unknown values at load time), kept so
    /// future additions can fail closed at load time.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref exit) = self.exit {
            exit.validate()?;
        }
        Ok(())
    }

    /// Merge two `PaneRuntimeConfig` instances (project overrides
    /// global). Returns `None` only when neither side authored the
    /// table.
    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                exit: PaneExitConfig::merge(g.exit.as_ref(), p.exit.as_ref()),
            }),
        }
    }
}

/// `[pane.exit]` — managed tool exit policy.
///
/// When a Workhive-managed supervised tool process exits (normal exit,
/// interrupt-driven exit, or runtime failure after the supervisor
/// reached a terminal state), Workhive resolves this policy to decide
/// whether the pane slot is preserved as a shell or allowed to close.
///
/// Built-in default when absent: [`ManagedToolExitBehavior::RespawnShell`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneExitConfig {
    /// Behavior to apply when a managed tool's supervised child
    /// process exits. `None` means built-in default applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_exit_behavior: Option<ManagedToolExitBehavior>,
}

impl PaneExitConfig {
    /// Validate. Invalid string values for `tool_exit_behavior` are
    /// rejected at deserialization time by
    /// [`ManagedToolExitBehavior::deserialize`]; this method exists
    /// as a structural pass for any future runtime construction
    /// path that bypasses serde.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        Ok(())
    }

    /// Project value wins when set.
    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                tool_exit_behavior: p.tool_exit_behavior.or(g.tool_exit_behavior),
            }),
        }
    }

    /// Resolve the effective behavior, applying the built-in default
    /// when the field is absent. Pure: no I/O. Callers receive a
    /// non-`Option` typed enum so command/runtime code never reads
    /// `Option<ManagedToolExitBehavior>`.
    pub fn resolved_tool_exit_behavior(&self) -> ManagedToolExitBehavior {
        self.tool_exit_behavior.unwrap_or_default()
    }
}

/// What Workhive does when a managed tool's supervised child process
/// exits.
///
/// This is a **lifecycle policy**, not a keyboard-shortcut policy. It
/// applies uniformly to every reason a supervised child can exit
/// (normal exit, signal-driven exit, runtime failure), so the user's
/// configured behavior does not depend on which key they pressed or
/// how the tool was stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ManagedToolExitBehavior {
    /// Preserve the tmux pane slot and start a fresh shell in the
    /// same pane after the supervised child exits. **Default.**
    ///
    /// Layout continuity is the priority: the user picks the next
    /// action (rerun the tool, leave it as a shell, or close the
    /// pane manually) instead of losing the pane on every tool exit.
    #[default]
    RespawnShell,
    /// Allow the pane to close when the supervised child exits — the
    /// supervisor returns the child's exit code and tmux removes the
    /// pane from the layout. Matches the pre-policy behavior; users
    /// who prefer "tool exit == layout slot gone" can opt in via
    /// config.
    ClosePane,
}

impl ManagedToolExitBehavior {
    /// Parse from a TOML string value. Returns `None` for unrecognized
    /// values so callers can emit a typed
    /// `ConfigError::InvalidValue` with context.
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s.trim() {
            "respawn_shell" => Some(Self::RespawnShell),
            "close_pane" => Some(Self::ClosePane),
            _ => None,
        }
    }

    /// Canonical string representation (matches the TOML enum values).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RespawnShell => "respawn_shell",
            Self::ClosePane => "close_pane",
        }
    }
}

impl std::fmt::Display for ManagedToolExitBehavior {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ManagedToolExitBehavior {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ManagedToolExitBehavior {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str_value(&s).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "invalid tool_exit_behavior '{s}': expected \"respawn_shell\" or \"close_pane\""
            ))
        })
    }
}

/// Top-level `[features]` table.
///
/// Each first-class capability is exposed as a dedicated, strongly-typed
/// child table to avoid `string -> bool` lookups inside the core. Today
/// only `ai_integrations` is modeled; future gated capabilities should
/// be added here rather than introducing ad-hoc top-level keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeaturesConfig {
    /// AI integration lifecycle: opportunistic hook/plugin prepare,
    /// hook/plugin activity ingestion, and Codex review guidance.
    ///
    /// `None` means the table is absent → all built-in defaults apply.
    pub ai_integrations: Option<AiIntegrationsConfig>,
    /// Pane observation feature gates. `None` means built-in defaults
    /// apply.
    pub pane_observation: Option<PaneObservationConfig>,
    /// Local notification gate. `None` means built-in defaults apply
    /// (enabled, with conservative per-category noise controls).
    ///
    /// Shape: `[features.notifications]` with `enabled = true` per
    /// the external-command policy. Individual categories are controlled by nested
    /// `[features.notifications.<category>]` tables.
    pub notifications: Option<NotificationsConfig>,
}

impl FeaturesConfig {
    /// Validate nested tables. Currently a no-op surface that exists
    /// so future additions can fail closed at load time.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref ai) = self.ai_integrations {
            ai.validate()?;
        }
        if let Some(ref pane) = self.pane_observation {
            pane.validate()?;
        }
        if let Some(ref notif) = self.notifications {
            notif.validate()?;
        }
        Ok(())
    }

    /// Merge two `FeaturesConfig` instances (project overrides global).
    ///
    /// Returns `None` only when neither side authored the table; the
    /// callers (`merge_configs`) preserve that signal so downstream
    /// resolvers can distinguish "absent" from "explicitly empty".
    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                ai_integrations: AiIntegrationsConfig::merge(
                    g.ai_integrations.as_ref(),
                    p.ai_integrations.as_ref(),
                ),
                pane_observation: PaneObservationConfig::merge(
                    g.pane_observation.as_ref(),
                    p.pane_observation.as_ref(),
                ),
                notifications: NotificationsConfig::merge(
                    g.notifications.as_ref(),
                    p.notifications.as_ref(),
                ),
            }),
        }
    }
}

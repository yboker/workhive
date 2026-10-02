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

/// `[features.notifications]` table.
///
/// Global switch for local notifications plus per-category noise
/// controls. Built-in defaults are conservative: permission-needed
/// and failed are on; completed is low-noise but configurable.
///
/// Shape per the external-command policy:
/// ```toml
/// [features.notifications]
/// enabled = true
///
/// [features.notifications.completed]
/// enabled = false
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationsConfig {
    /// Master switch. Built-in default: `true`.
    pub enabled: Option<bool>,

    /// `[features.notifications.permission_needed]`.
    /// Built-in default: enabled when notifications are on.
    pub permission_needed: Option<NotificationCategoryConfig>,

    /// `[features.notifications.failed]`.
    /// Built-in default: enabled when notifications are on.
    pub failed: Option<NotificationCategoryConfig>,

    /// `[features.notifications.completed]`.
    /// Built-in default: disabled (low-noise); user may enable.
    pub completed: Option<NotificationCategoryConfig>,

    /// `[features.notifications.all_panes_complete]`.
    /// Built-in default: disabled to avoid spam when panes finish in
    /// rapid succession.
    pub all_panes_complete: Option<NotificationCategoryConfig>,
}

impl NotificationsConfig {
    /// Whether the global notifications feature is on.
    ///
    /// Absent config (`None`) defaults to `true` (enabled).
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// Whether the `permission_needed` category is enabled.
    pub fn permission_needed_enabled(&self) -> bool {
        self.is_enabled()
            && self
                .permission_needed
                .as_ref()
                .and_then(|c| c.enabled)
                .unwrap_or(true)
    }

    /// Whether the `failed` category is enabled.
    pub fn failed_enabled(&self) -> bool {
        self.is_enabled() && self.failed.as_ref().and_then(|c| c.enabled).unwrap_or(true)
    }

    /// Whether the `completed` category is enabled.
    /// Default: disabled (low-noise).
    pub fn completed_enabled(&self) -> bool {
        self.is_enabled()
            && self
                .completed
                .as_ref()
                .and_then(|c| c.enabled)
                .unwrap_or(false)
    }

    /// Whether the `all_panes_complete` category is enabled.
    /// Default: disabled to avoid spam.
    pub fn all_panes_complete_enabled(&self) -> bool {
        self.is_enabled()
            && self
                .all_panes_complete
                .as_ref()
                .and_then(|c| c.enabled)
                .unwrap_or(false)
    }

    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        Ok(())
    }

    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                enabled: p.enabled.or(g.enabled),
                permission_needed: merge_notification_category(
                    g.permission_needed.as_ref(),
                    p.permission_needed.as_ref(),
                ),
                failed: merge_notification_category(g.failed.as_ref(), p.failed.as_ref()),
                completed: merge_notification_category(g.completed.as_ref(), p.completed.as_ref()),
                all_panes_complete: merge_notification_category(
                    g.all_panes_complete.as_ref(),
                    p.all_panes_complete.as_ref(),
                ),
            }),
        }
    }
}

/// A per-category notification configuration entry.
///
/// Shape: `[features.notifications.<category>]` with `enabled = true`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationCategoryConfig {
    /// Whether this category is enabled.
    pub enabled: Option<bool>,
}

fn merge_notification_category(
    global: Option<&NotificationCategoryConfig>,
    project: Option<&NotificationCategoryConfig>,
) -> Option<NotificationCategoryConfig> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => Some(NotificationCategoryConfig {
            enabled: p.enabled.or(g.enabled),
        }),
    }
}

/// `[features.pane_observation]` table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneObservationConfig {
    pub terminal_status: Option<TerminalStatusProbeConfig>,
}

impl PaneObservationConfig {
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        Ok(())
    }

    pub fn merge(global: Option<&Self>, project: Option<&Self>) -> Option<Self> {
        match (global, project) {
            (None, None) => None,
            (Some(g), None) => Some(g.clone()),
            (None, Some(p)) => Some(p.clone()),
            (Some(g), Some(p)) => Some(Self {
                terminal_status: merge_terminal_status_probe(
                    g.terminal_status.as_ref(),
                    p.terminal_status.as_ref(),
                ),
            }),
        }
    }
}

fn merge_terminal_status_probe(
    global: Option<&TerminalStatusProbeConfig>,
    project: Option<&TerminalStatusProbeConfig>,
) -> Option<TerminalStatusProbeConfig> {
    match (global, project) {
        (None, None) => None,
        (Some(g), None) => Some(g.clone()),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), Some(p)) => Some(TerminalStatusProbeConfig {
            enabled: p.enabled.or(g.enabled),
        }),
    }
}

/// `[features.pane_observation.terminal_status]` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalStatusProbeConfig {
    /// Built-in default when `None`: `true`.
    pub enabled: Option<bool>,
}

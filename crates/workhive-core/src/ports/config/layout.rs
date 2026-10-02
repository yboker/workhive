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

/// Preset layout template identifier.
///
/// Templates are presets that expand into `ColumnLayout`. The stored
/// configuration always uses `columns` — templates are a UI shortcut.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutTemplate {
    #[default]
    AiCodingLeft,
    /// Dual AI layout — AI agents on both left and right sides.
    ///
    /// Backward-compatible: `ai_coding_right` in existing TOML configs
    /// deserializes to this variant via the serde alias.
    #[serde(alias = "ai_coding_right")]
    AiCodingDual,
    HalfSplit,
    Quad,
    Single,
    Custom,
}

impl std::fmt::Display for LayoutTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AiCodingLeft => write!(f, "ai_coding_left"),
            Self::AiCodingDual => write!(f, "ai_coding_dual"),
            Self::HalfSplit => write!(f, "half_split"),
            Self::Quad => write!(f, "quad"),
            Self::Single => write!(f, "single"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

/// A single pane within a column.
///
/// The `launch` field is the canonical description of how this pane starts.
/// It replaces the legacy `command: String` field and must never be reduced
/// back to a raw string or an empty-string hack for `EmptyShell`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PaneConfig {
    /// Height percentage within the column (all panes in a column sum to 100).
    #[serde(default = "default_100")]
    pub height: u8,
    /// Structured launch descriptor for this pane.
    ///
    /// Defaults to `EmptyShell` when omitted from TOML — explicit empty-shell
    /// intent, not a sentinel / error marker.
    #[serde(default = "PaneConfig::default_launch")]
    pub launch: PaneLaunchRef,
}

impl PaneConfig {
    fn default_launch() -> PaneLaunchRef {
        PaneLaunchRef::EmptyShell
    }
}

fn default_100() -> u8 {
    100
}

/// A column in the layout. Contains one or more panes stacked vertically.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnConfig {
    /// Width percentage of this column (all columns sum to 100).
    pub width: u8,
    /// Panes within this column, top to bottom.
    pub panes: Vec<PaneConfig>,
}

/// Maximum number of total panes across all columns.
pub const MAX_TOTAL_PANES: usize = 10;

/// Pane layout configuration — Column-based DSL.
///
/// The canonical layout representation. Templates expand into columns
/// during the init wizard; the stored config always uses this format.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LayoutConfig {
    /// Which template was used as the starting point (informational).
    #[serde(default)]
    pub template: LayoutTemplate,

    /// Column-based layout definition. This is the source of truth.
    #[serde(default)]
    pub columns: Vec<ColumnConfig>,
}

impl LayoutConfig {
    /// Total number of panes across all columns.
    pub fn total_panes(&self) -> usize {
        self.columns.iter().map(|c| c.panes.len()).sum()
    }

    /// Flat iterator over all pane launch refs in reading order (column by column, top to bottom).
    pub fn pane_launches(&self) -> Vec<&PaneLaunchRef> {
        self.columns
            .iter()
            .flat_map(|c| c.panes.iter().map(|p| &p.launch))
            .collect()
    }
}

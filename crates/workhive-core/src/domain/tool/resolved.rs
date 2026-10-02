//! Fully resolved pane-launch outcomes produced by evaluating a
//! `PaneLaunchRef` against the effective tool catalog and executable
//! availability checks.

use std::collections::BTreeMap;
use std::fmt;

use super::*;

// ── ResolvedPaneLaunch ───────────────────────────────────────────────────────

/// The fully resolved result of evaluating a `PaneLaunchRef` against the
/// effective tool catalog and executable availability checks.
///
/// This type replaces ad-hoc `Option<String>` resolution: every resolution
/// outcome is explicit, and degraded states carry enough context for
/// user-visible reporting.
///
/// ## Non-negotiable rules
///
/// - `CatalogMissing`, `DisabledTool`, `ExecutableNotFound`, and `InvalidConfig`
///   must be surfaced in the CLI open summary.
/// - Silent degradation to `EmptyShell` is forbidden.
/// - The `pane_position` field identifies which pane degraded (for diagnostics).
///
/// ## Typed error states (all distinct)
///
/// | Variant | Meaning |
/// |---------|---------|
/// | `CatalogMissing` | Tool name not in catalog at all |
/// | `DisabledTool` | In catalog but explicitly disabled |
/// | `ExecutableNotFound` | In catalog, enabled, but binary not on PATH |
/// | `InvalidConfig` | Malformed tool entry or catalog config |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedPaneLaunch {
    /// Pane was intentionally configured as an empty shell — not a degradation.
    EmptyShell,
    /// Tool or inline command resolved to an executable command.
    ///
    /// Carries typed argv (`program` + `args`) so downstream layers
    /// (the pane supervisor wrapper and runtime launch path) never
    /// have to re-parse a shell string. `command` is kept as the
    /// pre-formatted display text used by logs/snapshot summaries.
    Executable {
        /// Pre-formatted display string (`<program> <arg1> <arg2>`).
        /// Existing CLI/log/snapshot code that only renders the command
        /// can keep using this field; the supervisor instrumentation
        /// path uses the typed `program` / `args` instead.
        command: String,
        /// Executable program — typed argv[0] forwarded to the
        /// supervisor without shell parsing.
        program: String,
        /// Program arguments — typed argv[1..]; never join into a
        /// string until the very last shell-boundary moment.
        args: Vec<String>,
        /// Environment variables injected into the child process at
        /// spawn time via `Command::env()`. Keeps the process name
        /// equal to `program` for observation/provider identity.
        env: BTreeMap<String, String>,
        /// Optional resolved program path (catalog tools only).
        ///
        /// Surfaced so the supervisor metadata's `program_path`
        /// agrees with the catalog entry when one is configured;
        /// inline commands leave this `None`.
        program_path: Option<String>,
        /// Baseline pane kind derived from launch metadata.
        baseline_kind: crate::domain::pane::PaneKind,
    },
    /// A `Tool(name)` reference does not exist in the catalog at all.
    CatalogMissing { name: ToolName },
    /// A `Tool(name)` reference was found but disabled in the catalog.
    DisabledTool { name: ToolName },
    /// A `Tool(name)` exists and is enabled, but its binary is not on PATH.
    ExecutableNotFound { name: ToolName, program: String },
    /// A tool entry or catalog configuration is malformed.
    InvalidConfig { name: ToolName, reason: String },
}

impl ResolvedPaneLaunch {
    /// Whether the pane should receive a launch command.
    pub fn has_command(&self) -> bool {
        matches!(self, Self::Executable { .. })
    }

    /// Whether this resolution represents a degraded state that must be reported.
    pub fn is_degraded(&self) -> bool {
        matches!(
            self,
            Self::CatalogMissing { .. }
                | Self::DisabledTool { .. }
                | Self::ExecutableNotFound { .. }
                | Self::InvalidConfig { .. }
        )
    }

    /// Human-readable reason for degradation (for logs/summary).
    pub fn degradation_reason(&self) -> Option<String> {
        match self {
            Self::CatalogMissing { name } => Some(format!("tool '{}' not found in catalog", name)),
            Self::DisabledTool { name } => Some(format!("tool '{}' is disabled in catalog", name)),
            Self::ExecutableNotFound { name, program } => Some(format!(
                "tool '{}' binary '{}' not found on PATH",
                name, program
            )),
            Self::InvalidConfig { name, reason } => {
                Some(format!("tool '{}' has invalid config: {}", name, reason))
            }
            _ => None,
        }
    }

    /// Static capability baseline derived from launch metadata.
    ///
    /// This connects launch resolution to the pane capability model:
    /// `Executable` panes get capabilities based on their `baseline_kind`,
    /// `EmptyShell` gets `CommandExecutor`, and degraded states get nothing.
    /// Runtime observation may later refine these capabilities.
    pub fn baseline_capabilities(
        &self,
    ) -> std::collections::BTreeSet<crate::domain::pane::PaneCapability> {
        match self {
            Self::Executable { baseline_kind, .. } => {
                crate::domain::pane::derive_static_capabilities(*baseline_kind)
            }
            Self::EmptyShell => crate::domain::pane::derive_static_capabilities(
                crate::domain::pane::PaneKind::Terminal,
            ),
            Self::CatalogMissing { .. }
            | Self::DisabledTool { .. }
            | Self::ExecutableNotFound { .. }
            | Self::InvalidConfig { .. } => std::collections::BTreeSet::new(),
        }
    }
}

impl fmt::Display for ResolvedPaneLaunch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyShell => f.write_str("<empty shell>"),
            Self::Executable { command, .. } => write!(f, "exec:{command}"),
            Self::CatalogMissing { name } => write!(f, "CATALOG_MISSING:{name}"),
            Self::DisabledTool { name } => write!(f, "DISABLED:{name}"),
            Self::ExecutableNotFound { name, program } => {
                write!(f, "EXEC_NOT_FOUND:{name}({program})")
            }
            Self::InvalidConfig { name, reason } => {
                write!(f, "INVALID_CONFIG:{name}({reason})")
            }
        }
    }
}

//! Launch resolution use case — unified pipeline for resolving `PaneLaunchRef`
//! into executable `ResolvedPaneLaunch` results.
//!
//! Both `workhive workspace open` (apply layout) and `workhive init` (wizard preview)
//! must use this pipeline. CLI code must not call `ToolCatalog::builtin_defaults()`
//! directly for launch resolution.
//!
//! ## Pipeline
//!
//! ```text
//! effective tool catalog → launch resolution → availability check → result
//! ```
//!
//! Missing or disabled tools produce typed degraded states, never silent fallbacks.

use crate::domain::{
    CommandSpec, PaneKind, PaneLaunchRef, ResolvedPaneLaunch, ToolCatalog, ToolEntry, ToolName,
};
use crate::ports::ExecutableAvailabilityPort;

/// Resolve a single `PaneLaunchRef` against the effective catalog.
///
/// Availability checking is delegated to the `exec_checker` port, keeping
/// the resolver free of filesystem access and testable with fakes.
pub fn resolve_launch(
    launch: &PaneLaunchRef,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> ResolvedPaneLaunch {
    match launch {
        PaneLaunchRef::EmptyShell => ResolvedPaneLaunch::EmptyShell,

        PaneLaunchRef::Tool { name } => resolve_tool(name, catalog, exec_checker),

        PaneLaunchRef::InlineCommand { spec } => ResolvedPaneLaunch::Executable {
            command: spec.to_string(),
            program: spec.program().to_owned(),
            args: spec.args().to_vec(),
            env: spec.env().clone(),
            program_path: None,
            baseline_kind: baseline_kind_for_command(spec),
        },
    }
}

/// Resolve a batch of `PaneLaunchRef` items (e.g. from a layout plan).
///
/// Returns one `ResolvedPaneLaunch` per input, in the same order.
pub fn resolve_launches(
    launches: &[PaneLaunchRef],
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Vec<ResolvedPaneLaunch> {
    launches
        .iter()
        .map(|l| resolve_launch(l, catalog, exec_checker))
        .collect()
}

/// Collect all degraded (missing/disabled) resolutions for reporting.
pub fn collect_degraded(resolved: &[ResolvedPaneLaunch]) -> Vec<(usize, &ResolvedPaneLaunch)> {
    resolved
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_degraded())
        .collect()
}

/// Build an effective tool catalog from configuration.
///
/// Priority: project config > global config > builtin defaults.
/// This is the single entry point for obtaining the catalog used by both
/// `open` and `init`.
///
/// Config errors are propagated — never coerced to builtin defaults.
pub fn effective_catalog_from_config(
    config: &dyn crate::ports::ConfigPort,
    project_root: Option<&camino::Utf8Path>,
) -> Result<ToolCatalog, crate::errors::ConfigError> {
    if let Some(root) = project_root
        && let Some(ref cfg) = config.load_project(root)?
        && let Some(ref tools) = cfg.tools
    {
        return Ok(tools.catalog.clone());
    }

    if let Some(ref cfg) = config.load_global()?
        && let Some(ref tools) = cfg.tools
    {
        return Ok(tools.catalog.clone());
    }

    Ok(ToolCatalog::builtin_defaults())
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn resolve_tool(
    name: &ToolName,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> ResolvedPaneLaunch {
    match catalog.find(name) {
        None => ResolvedPaneLaunch::CatalogMissing { name: name.clone() },
        Some(entry) if !entry.enabled => ResolvedPaneLaunch::DisabledTool { name: name.clone() },
        Some(entry) => {
            if let Some(reason) = entry.validate() {
                tracing::warn!(
                    tool = name.as_str(),
                    reason = %reason,
                    "tool entry has invalid configuration"
                );
                return ResolvedPaneLaunch::InvalidConfig {
                    name: name.clone(),
                    reason,
                };
            }
            if !exec_checker.is_available(entry.command.program()) {
                tracing::warn!(
                    tool = name.as_str(),
                    program = entry.command.program(),
                    "tool binary not found on PATH"
                );
                return ResolvedPaneLaunch::ExecutableNotFound {
                    name: name.clone(),
                    program: entry.command.program().to_owned(),
                };
            }
            ResolvedPaneLaunch::Executable {
                command: entry.command.to_string(),
                program: entry.command.program().to_owned(),
                args: entry.command.args().to_vec(),
                env: entry.command.env().clone(),
                program_path: Some(entry.command.program().to_owned()),
                baseline_kind: baseline_kind_for_tool(entry),
            }
        }
    }
}

/// Derive baseline `PaneKind` from a tool entry's catalog metadata.
///
/// Uses `ToolEntry.kind` (the declared `ToolKind`) to determine the
/// baseline `PaneKind`. This replaces the previous tool-name string
/// matching approach, ensuring capability derivation depends on
/// structured metadata rather than hardcoded name lists.
fn baseline_kind_for_tool(entry: &ToolEntry) -> PaneKind {
    entry.effective_kind().to_pane_kind()
}

/// Derive baseline `PaneKind` from an inline command.
fn baseline_kind_for_command(_spec: &CommandSpec) -> PaneKind {
    PaneKind::Terminal
}

/// Pane identity for baseline registration, carrying launch-derived capabilities.
///
/// Produced by [`baseline_pane_entries`] and consumed by callers that need to
/// seed the pane registry before runtime observation refines the capabilities.
#[derive(Debug, Clone)]
pub struct BaselinePaneEntry {
    /// Column index (0-based).
    pub col: usize,
    /// Row index within the column (0-based).
    pub row: usize,
    /// Baseline capabilities derived from launch metadata.
    pub capabilities: std::collections::BTreeSet<crate::domain::PaneCapability>,
    /// The resolved launch state (for logging/display).
    pub resolved: ResolvedPaneLaunch,
    /// Original launch reference — records how this pane was configured.
    pub launch_origin: PaneLaunchRef,
}

/// Derive baseline pane entries with static capabilities from resolved launches.
///
/// Each element is `(col, row, resolved, original_launch_ref)`.
///
/// This bridges the launch resolution pipeline to the pane capability model:
/// each resolved launch produces a `BaselinePaneEntry` carrying capabilities
/// derived from `ResolvedPaneLaunch::baseline_capabilities()`, which calls
/// `derive_static_capabilities()` under the hood.
///
/// The caller (typically the `open` command) can then register these entries
/// in the `PaneRegistryPort` before runtime observation begins, ensuring
/// that capability truth exists from pane creation rather than only after
/// the first runtime sync cycle.
pub fn baseline_pane_entries(
    launches: &[(usize, usize, ResolvedPaneLaunch, PaneLaunchRef)],
) -> Vec<BaselinePaneEntry> {
    launches
        .iter()
        .map(|(col, row, resolved, origin)| BaselinePaneEntry {
            col: *col,
            row: *row,
            capabilities: resolved.baseline_capabilities(),
            resolved: resolved.clone(),
            launch_origin: origin.clone(),
        })
        .collect()
}

/// Classify a free-form `--cmd` value supplied to `workhive pane split`
/// into a typed [`PaneLaunchRef`] for durable identity recording.
///
/// The split path must record durable launch identity and the
/// pane title **immediately** after `pane split`, so that `pane list`
/// / `pane show` can identify the pane (e.g. as `codex`) without
/// waiting for another open/close cycle.
///
/// Common tool invocations with arguments — `codex
/// --model x`, `claude --dangerously-skip-permissions`, `lazygit
/// --all` — are recognized as the tool they invoke, not as generic
/// inline commands. Without that, the user-facing pane label drops
/// back to a generic program token even though the launch is plainly
/// the catalog tool.
///
/// Classification rules:
///
/// * `None` or whitespace-only → [`PaneLaunchRef::EmptyShell`].
/// * Trimmed string is shell-safe (no quotes, pipes, redirects, or
///   path separators in the first token) AND its first whitespace-
///   separated token is a valid `ToolName` AND that name is present
///   in the catalog → [`PaneLaunchRef::Tool`] using the catalog
///   tool's name. This covers both bare `codex` and `codex --flag`.
/// * Anything else (unknown program, path-qualified program, shell
///   metacharacters present anywhere, invalid tool name) →
///   [`PaneLaunchRef::InlineCommand`] with the first whitespace-
///   separated token as `program` and the remainder as `args`.
///   Unknown commands MUST NOT be misclassified as catalog tools.
///
/// Note: the durable record's role here is **identity only**. The
/// user's full original `--cmd` string was already executed verbatim
/// by the caller via `runtime.split_pane(... command: Some(raw) ...)`
/// before this classifier runs, so classifying `codex --flag` as
/// `Tool { name: codex }` does not strip the user's flags from
/// execution — they have already taken effect on the live pane.
/// The durable record identifies the already-running pane; it does not
/// participate in runtime reconstruction.
///
/// The function is pure and does not touch the executable
/// availability port: catalog membership alone determines the `Tool`
/// variant. Availability is re-checked later by `resolve_launch` if a
/// caller wants to inspect degraded states.
pub fn classify_split_command(cmd: Option<&str>, catalog: &ToolCatalog) -> PaneLaunchRef {
    let Some(raw) = cmd else {
        return PaneLaunchRef::EmptyShell;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return PaneLaunchRef::EmptyShell;
    }

    // First token of the trimmed command. We only treat the leading
    // whitespace-separated token as the candidate program. Anything
    // after the first whitespace is preserved as user-supplied args
    // (already executed by the runtime; recorded here purely for
    // typed-identity bookkeeping).
    let mut tokens = trimmed.split_whitespace();
    let head = tokens.next().unwrap_or("");
    let tail: Vec<String> = tokens.map(str::to_owned).collect();

    // Guard against shell metacharacters / path-qualified programs:
    // we conservatively only classify simple `<program> [args...]`
    // forms as catalog tools. Anything that hints at quoting,
    // pipelines, redirects, env-var prefixing, or absolute/relative
    // path invocation falls through to InlineCommand so the durable
    // record stays faithful to user input.
    let head_is_simple = !head.contains('/')
        && !head.contains('\\')
        && !head.contains('"')
        && !head.contains('\'')
        && !head.contains('|')
        && !head.contains('>')
        && !head.contains('<')
        && !head.contains('&')
        && !head.contains('=')
        && !head.contains('$')
        && !head.contains('`');

    if head_is_simple
        && let Ok(tool_name) = ToolName::new(head)
        && catalog.find(&tool_name).is_some()
    {
        return PaneLaunchRef::Tool { name: tool_name };
    }

    // Fall through: unknown program / metachar / path-qualified.
    PaneLaunchRef::InlineCommand {
        spec: CommandSpec::new(head, tail),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "launch_resolver_tests.rs"]
mod tests;

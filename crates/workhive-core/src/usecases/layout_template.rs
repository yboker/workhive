//! Layout template expansion and environment-aware launch degradation.
//!
//! Template expansion is product logic, not terminal UI logic. Keeping it in
//! core lets future frontends reuse the same template semantics.

use crate::domain::{PaneLaunchRef, ToolCatalog, ToolName};
use crate::ports::{
    ColumnConfig, ExecutableAvailabilityPort, LayoutTemplate, PaneConfig, ToolCatalogConfig,
};
use crate::usecases::launch_resolver;

/// Expand a built-in layout template into the canonical column DSL.
pub fn expand_layout_template(template: &LayoutTemplate) -> Vec<ColumnConfig> {
    match template {
        LayoutTemplate::AiCodingLeft => vec![
            ColumnConfig {
                width: 60,
                panes: vec![
                    PaneConfig {
                        height: 50,
                        launch: tool("claude"),
                    },
                    PaneConfig {
                        height: 50,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                ],
            },
            ColumnConfig {
                width: 40,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: tool("lazygit"),
                }],
            },
        ],
        LayoutTemplate::AiCodingDual => vec![
            ColumnConfig {
                width: 50,
                panes: vec![
                    PaneConfig {
                        height: 70,
                        launch: tool("claude"),
                    },
                    PaneConfig {
                        height: 30,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                ],
            },
            ColumnConfig {
                width: 50,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: tool("codex"),
                }],
            },
        ],
        LayoutTemplate::HalfSplit => vec![
            ColumnConfig {
                width: 50,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
            ColumnConfig {
                width: 50,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
        ],
        LayoutTemplate::Quad => vec![
            ColumnConfig {
                width: 50,
                panes: vec![
                    PaneConfig {
                        height: 50,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                    PaneConfig {
                        height: 50,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                ],
            },
            ColumnConfig {
                width: 50,
                panes: vec![
                    PaneConfig {
                        height: 50,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                    PaneConfig {
                        height: 50,
                        launch: PaneLaunchRef::EmptyShell,
                    },
                ],
            },
        ],
        LayoutTemplate::Single => vec![ColumnConfig {
            width: 100,
            panes: vec![PaneConfig {
                height: 100,
                launch: PaneLaunchRef::EmptyShell,
            }],
        }],
        LayoutTemplate::Custom => vec![],
    }
}

/// Replace unavailable tool launches with empty shells.
///
/// This is used during initialization only. Runtime `open` still reports
/// degraded explicit project references instead of mutating config.
pub fn degrade_unavailable_template_tools(
    columns: &[ColumnConfig],
    catalog: &crate::domain::ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Vec<ColumnConfig> {
    columns
        .iter()
        .map(|column| ColumnConfig {
            width: column.width,
            panes: column
                .panes
                .iter()
                .map(|pane| PaneConfig {
                    height: pane.height,
                    launch: degrade_launch(&pane.launch, catalog, exec_checker),
                })
                .collect(),
        })
        .collect()
}

/// Build the editable global tools catalog for executables available now.
///
/// This is used by `init --global`: unavailable built-in tools are not written
/// to the user's editable catalog, so generated project layouts cannot
/// accidentally reference commands that do not exist on the current PATH.
///
/// Launch-program fallback is intentionally narrow: only Cursor Agent has a
/// verified second executable name (`agent`) that is the same installed
/// binary as `cursor-agent`. All other built-in tools must have their
/// canonical `command.program` available on PATH or be omitted; their
/// `process_names` entries remain runtime observation / exact process-name
/// matching metadata and MUST NOT be repurposed as launch command aliases.
pub fn global_tool_catalog_config_for_environment(
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> ToolCatalogConfig {
    let mut catalog = ToolCatalog::empty();

    for entry in ToolCatalog::builtin_defaults().iter() {
        if let Some(resolved) = resolve_entry_for_environment(entry, exec_checker) {
            catalog
                .add(resolved)
                .expect("filtered builtin tool catalog has unique names");
        }
    }

    ToolCatalogConfig { catalog }
}

/// Cursor Agent's canonical tool name. Used to gate the narrow
/// canonical-to-alias launch fallback below.
const CURSOR_AGENT_TOOL_NAME: &str = "cursor-agent";

/// Cursor Agent's verified secondary executable name. Both `cursor-agent`
/// and `agent` ship as identical binaries in the supported install; this
/// alias is the ONLY launch-program fallback Workhive recognizes.
const CURSOR_AGENT_LAUNCH_ALIAS: &str = "agent";

/// Resolve a built-in tool entry against the current environment.
///
/// Default policy: return the entry unchanged when its primary
/// `command.program` is on PATH; otherwise return `None`. Built-in
/// `process_names` are runtime observation metadata (architecture-suffixed
/// binaries, exact basename matching) and are NEVER used as launch command
/// aliases here.
///
/// Narrow Cursor Agent exception: when the entry's canonical name is
/// `cursor-agent` and only the alias executable `agent` is available, the
/// returned clone has `command.program` rewritten to `agent` while
/// preserving the canonical `ToolName`. This is the verified two-executable
/// install pattern for Cursor Agent
/// and must not be generalized to other built-in tools.
fn resolve_entry_for_environment(
    entry: &crate::domain::ToolEntry,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> Option<crate::domain::ToolEntry> {
    if exec_checker.is_available(entry.command.program()) {
        return Some(entry.clone());
    }

    if entry.name.as_str() == CURSOR_AGENT_TOOL_NAME
        && entry.command.program() != CURSOR_AGENT_LAUNCH_ALIAS
        && exec_checker.is_available(CURSOR_AGENT_LAUNCH_ALIAS)
    {
        let mut rewritten = entry.clone();
        rewritten.command.program = CURSOR_AGENT_LAUNCH_ALIAS.to_owned();
        return Some(rewritten);
    }

    None
}

fn degrade_launch(
    launch: &PaneLaunchRef,
    catalog: &crate::domain::ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
) -> PaneLaunchRef {
    match launch {
        PaneLaunchRef::Tool { .. } => {
            let resolved = launch_resolver::resolve_launch(launch, catalog, exec_checker);
            if resolved.has_command() {
                launch.clone()
            } else {
                PaneLaunchRef::EmptyShell
            }
        }
        PaneLaunchRef::EmptyShell | PaneLaunchRef::InlineCommand { .. } => launch.clone(),
    }
}

fn tool(name: &str) -> PaneLaunchRef {
    PaneLaunchRef::Tool {
        name: ToolName::new(name).expect("builtin tool name is valid"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ToolName;

    struct AvailableTools(Vec<&'static str>);

    impl ExecutableAvailabilityPort for AvailableTools {
        fn is_available(&self, program: &str) -> bool {
            self.0.contains(&program)
        }
    }

    #[test]
    fn ai_template_uses_tool_references_before_environment_degradation() {
        let columns = expand_layout_template(&LayoutTemplate::AiCodingDual);

        assert!(matches!(
            columns[0].panes[0].launch,
            PaneLaunchRef::Tool { .. }
        ));
        assert!(matches!(
            columns[1].panes[0].launch,
            PaneLaunchRef::Tool { .. }
        ));
    }

    #[test]
    fn missing_template_tools_degrade_to_empty_shell() {
        let columns = expand_layout_template(&LayoutTemplate::AiCodingDual);
        let degraded = degrade_unavailable_template_tools(
            &columns,
            &ToolCatalog::builtin_defaults(),
            &AvailableTools(vec![]),
        );

        assert_eq!(degraded[0].panes[0].launch, PaneLaunchRef::EmptyShell);
        assert_eq!(degraded[1].panes[0].launch, PaneLaunchRef::EmptyShell);
    }

    #[test]
    fn available_template_tools_are_preserved() {
        let columns = expand_layout_template(&LayoutTemplate::AiCodingDual);
        let degraded = degrade_unavailable_template_tools(
            &columns,
            &ToolCatalog::builtin_defaults(),
            &AvailableTools(vec!["claude", "codex"]),
        );

        assert_eq!(
            degraded[0].panes[0].launch,
            PaneLaunchRef::Tool {
                name: ToolName::new("claude").unwrap()
            }
        );
        assert_eq!(
            degraded[1].panes[0].launch,
            PaneLaunchRef::Tool {
                name: ToolName::new("codex").unwrap()
            }
        );
    }

    #[test]
    fn global_tool_catalog_config_contains_builtin_tools() {
        let config = ToolCatalogConfig::default();

        assert!(
            config
                .catalog
                .find(&ToolName::new("claude").unwrap())
                .is_some()
        );
        assert!(
            config
                .catalog
                .find(&ToolName::new("codex").unwrap())
                .is_some()
        );
    }

    #[test]
    fn environment_catalog_contains_only_available_builtin_tools() {
        let config = global_tool_catalog_config_for_environment(&AvailableTools(vec!["codex"]));

        assert!(
            config
                .catalog
                .find(&ToolName::new("codex").unwrap())
                .is_some()
        );
        assert!(
            config
                .catalog
                .find(&ToolName::new("claude").unwrap())
                .is_none()
        );
    }

    /// Cursor Agent default selection: when `cursor-agent` is on PATH it
    /// must be preferred and the canonical command preserved.
    #[test]
    fn cursor_agent_is_preferred_when_canonical_executable_available() {
        let config =
            global_tool_catalog_config_for_environment(&AvailableTools(vec!["cursor-agent"]));
        let entry = config
            .catalog
            .find(&ToolName::new("cursor-agent").unwrap())
            .expect("cursor-agent must be present when `cursor-agent` is available");
        assert_eq!(entry.command.program(), "cursor-agent");
        assert_eq!(entry.command.args(), &["--yolo".to_owned()]);
    }

    /// Cursor Agent default selection: when only the `agent` alias is on
    /// PATH the entry must remain modeled as the canonical `cursor-agent`
    /// tool but the command program must be rewritten to invoke `agent`.
    #[test]
    fn cursor_agent_falls_back_to_agent_alias_when_canonical_executable_missing() {
        let config = global_tool_catalog_config_for_environment(&AvailableTools(vec!["agent"]));
        let entry = config
            .catalog
            .find(&ToolName::new("cursor-agent").unwrap())
            .expect("cursor-agent must be present when only `agent` is available");
        assert_eq!(entry.command.program(), "agent");
        assert_eq!(entry.command.args(), &["--yolo".to_owned()]);
    }

    /// Cursor Agent default selection: when neither executable is on PATH
    /// the entry must be omitted from the environment-filtered catalog
    /// so generated project layouts cannot reference a missing binary.
    #[test]
    fn cursor_agent_is_omitted_when_neither_executable_available() {
        let config = global_tool_catalog_config_for_environment(&AvailableTools(vec!["claude"]));
        assert!(
            config
                .catalog
                .find(&ToolName::new("cursor-agent").unwrap())
                .is_none(),
            "cursor-agent must be absent when neither `cursor-agent` nor `agent` is on PATH"
        );
    }

    /// Negative regression: built-in `process_names`
    /// are runtime observation metadata (architecture-suffixed binaries,
    /// exact basename matching). They MUST NOT be reused as launch
    /// command aliases for tools other than Cursor Agent. If only an
    /// observation-only process name such as `codex-aarch64-apple-darwin`
    /// is on PATH but the primary `codex` binary is not, Codex must be
    /// omitted from the environment-filtered catalog rather than written
    /// out with `command.program = "codex-aarch64-apple-darwin"`.
    #[test]
    fn codex_is_omitted_when_only_arch_suffixed_process_name_available() {
        let config = global_tool_catalog_config_for_environment(&AvailableTools(vec![
            "codex-aarch64-apple-darwin",
        ]));
        assert!(
            config
                .catalog
                .find(&ToolName::new("codex").unwrap())
                .is_none(),
            "codex must not be written into the catalog when only its observation-only \
             architecture-suffixed process name is on PATH; process_names is observation \
             metadata, not a launch alias"
        );
    }

    /// Negative regression: even though the
    /// `cursor-agent` entry now has a narrow `agent` launch fallback,
    /// the same narrow policy must not engage for any other built-in
    /// tool. Specifically, the canonical `agent` basename being on PATH
    /// must not pull non-Cursor-Agent entries into the catalog or
    /// rewrite their `command.program`.
    #[test]
    fn agent_alias_does_not_rewrite_non_cursor_agent_entries() {
        let config = global_tool_catalog_config_for_environment(&AvailableTools(vec!["agent"]));

        // Cursor Agent IS rewritten — that is the verified canonical exception.
        let cursor_agent_entry = config
            .catalog
            .find(&ToolName::new("cursor-agent").unwrap())
            .expect("cursor-agent must still resolve via the narrow alias policy");
        assert_eq!(cursor_agent_entry.command.program(), "agent");

        // No other built-in entry may piggy-back on the alias.
        for tool_name in ["claude", "codex", "opencode", "lazygit", "yazi"] {
            let entry = config.catalog.find(&ToolName::new(tool_name).unwrap());
            assert!(
                entry.is_none(),
                "{tool_name} must not be promoted into the catalog just because `agent` \
                 happens to be on PATH; the narrow alias policy is Cursor-Agent-only"
            );
        }
    }
}

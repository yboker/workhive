use super::*;
use crate::domain::ToolKind;
use crate::errors::ConfigError;
use crate::ports::{AppConfig, ConfigPort, ExecutableAvailabilityPort, ToolCatalogConfig};

struct AllAvailable;
impl ExecutableAvailabilityPort for AllAvailable {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

struct NoneAvailable;
impl ExecutableAvailabilityPort for NoneAvailable {
    fn is_available(&self, _program: &str) -> bool {
        false
    }
}

struct StubConfig {
    global: Option<AppConfig>,
    project: Option<AppConfig>,
}
impl StubConfig {
    fn empty() -> Self {
        Self {
            global: None,
            project: None,
        }
    }
    fn with_global(mut self, cfg: AppConfig) -> Self {
        self.global = Some(cfg);
        self
    }
    fn with_project(mut self, cfg: AppConfig) -> Self {
        self.project = Some(cfg);
        self
    }
}
impl ConfigPort for StubConfig {
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
        Ok(self.global.clone())
    }
    fn save_global(&self, _: &AppConfig) -> Result<(), ConfigError> {
        Ok(())
    }
    fn load_project(&self, _: &camino::Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        Ok(self.project.clone())
    }
    fn save_project(&self, _: &camino::Utf8Path, _: &AppConfig) -> Result<(), ConfigError> {
        Ok(())
    }
    fn global_exists(&self) -> Result<bool, ConfigError> {
        Ok(self.global.is_some())
    }
    fn project_exists(&self, _: &camino::Utf8Path) -> Result<bool, ConfigError> {
        Ok(self.project.is_some())
    }
    fn load_raw_toml(&self, _: Option<&camino::Utf8Path>, _: bool) -> Result<String, ConfigError> {
        Ok(String::new())
    }
    fn save_raw_toml(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
        _: &str,
    ) -> Result<(), ConfigError> {
        Ok(())
    }
    fn load_env(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
    ) -> Result<Vec<(String, String)>, ConfigError> {
        Ok(Vec::new())
    }
    fn save_env(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
        _: &[(String, String)],
    ) -> Result<(), ConfigError> {
        Ok(())
    }
    fn global_config_dir(&self) -> Result<camino::Utf8PathBuf, ConfigError> {
        Ok(camino::Utf8PathBuf::from("/tmp/stub-config"))
    }
}

#[test]
fn empty_shell_resolves_to_empty_shell() {
    let catalog = ToolCatalog::builtin_defaults();
    let result = resolve_launch(&PaneLaunchRef::EmptyShell, &catalog, &AllAvailable);
    assert_eq!(result, ResolvedPaneLaunch::EmptyShell);
}

/// A shell pane carries no provider requirement, so it must resolve
/// cleanly even when no tool executables are installed. This is the
/// shell-only first-run guarantee: absent AI providers never degrade a
/// plain shell workspace.
#[test]
fn empty_shell_resolves_without_any_executables() {
    let catalog = ToolCatalog::builtin_defaults();
    let result = resolve_launch(&PaneLaunchRef::EmptyShell, &catalog, &NoneAvailable);
    assert_eq!(result, ResolvedPaneLaunch::EmptyShell);
    assert!(
        !result.is_degraded(),
        "a shell pane must never be degraded by absent providers"
    );
}

/// A full shell-only layout (multiple shell panes) must resolve with
/// zero degraded panes even when no tool executables are available. This
/// proves the first-run shell-only flow is not blocked or degraded by
/// absent Claude/Codex/Opencode at the layout-resolution seam, not just
/// a single pane.
#[test]
fn shell_only_layout_has_no_degraded_panes_without_executables() {
    let catalog = ToolCatalog::builtin_defaults();
    let launches = vec![
        PaneLaunchRef::EmptyShell,
        PaneLaunchRef::EmptyShell,
        PaneLaunchRef::EmptyShell,
    ];
    let resolved = resolve_launches(&launches, &catalog, &NoneAvailable);
    assert_eq!(resolved.len(), 3);
    assert!(
        collect_degraded(&resolved).is_empty(),
        "a shell-only layout must not degrade when no providers are installed: {resolved:?}"
    );
    assert!(resolved.iter().all(|r| !r.is_degraded()));
}

/// Proves the global-init single-pane Claude Code selection path: when the
/// wizard persists a Single layout with `PaneLaunchRef::Tool { name: "claude" }`,
/// that config resolves to a Claude executable with `IS_SANDBOX=1` env —
/// not an empty shell.
#[test]
fn single_pane_claude_selection_resolves_to_claude_launch_not_empty_shell() {
    use crate::ports::{ColumnConfig, LayoutConfig, LayoutTemplate, PaneConfig};

    let layout = LayoutConfig {
        template: LayoutTemplate::Single,
        columns: vec![ColumnConfig {
            width: 100,
            panes: vec![PaneConfig {
                height: 100,
                launch: PaneLaunchRef::Tool {
                    name: ToolName::new("claude").unwrap(),
                },
            }],
        }],
    };
    let catalog = ToolCatalog::builtin_defaults();
    let launches: Vec<_> = layout
        .columns
        .iter()
        .flat_map(|c| c.panes.iter().map(|p| p.launch.clone()))
        .collect();
    let resolved = resolve_launches(&launches, &catalog, &AllAvailable);
    assert_eq!(resolved.len(), 1);
    match &resolved[0] {
        ResolvedPaneLaunch::Executable { program, env, .. } => {
            assert_eq!(program, "claude", "program identity must be 'claude'");
            assert_eq!(
                env.get("IS_SANDBOX").map(String::as_str),
                Some("1"),
                "Claude launch must carry IS_SANDBOX=1 from the catalog"
            );
        }
        other => panic!("single-pane Claude selection must resolve to Executable, not {other:?}"),
    }
    assert!(
        !resolved[0].is_degraded(),
        "single-pane Claude must not be degraded when claude binary is available"
    );
}

#[test]
fn known_enabled_tool_resolves_to_executable() {
    let catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    let result = resolve_launch(&PaneLaunchRef::Tool { name }, &catalog, &AllAvailable);
    match &result {
        ResolvedPaneLaunch::Executable {
            command,
            program,
            args,
            env,
            program_path,
            baseline_kind,
        } => {
            assert!(command.contains("claude"));
            assert_eq!(*baseline_kind, PaneKind::Agent);
            assert_eq!(program, "claude");
            assert!(!args.is_empty() || program == "claude");
            assert_eq!(program_path.as_deref(), Some("claude"));
            assert_eq!(
                env.get("IS_SANDBOX").map(String::as_str),
                Some("1"),
                "Claude tool resolution must propagate IS_SANDBOX=1 from the catalog"
            );
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

#[test]
fn unknown_tool_resolves_to_catalog_missing() {
    let catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("nonexistent").unwrap();
    let result = resolve_launch(
        &PaneLaunchRef::Tool { name: name.clone() },
        &catalog,
        &AllAvailable,
    );
    assert_eq!(result, ResolvedPaneLaunch::CatalogMissing { name });
}

#[test]
fn disabled_tool_resolves_to_disabled() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    catalog.disable(&name).unwrap();
    let result = resolve_launch(
        &PaneLaunchRef::Tool { name: name.clone() },
        &catalog,
        &AllAvailable,
    );
    assert_eq!(result, ResolvedPaneLaunch::DisabledTool { name });
}

#[test]
fn tool_with_unavailable_binary_resolves_to_executable_not_found() {
    let catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    let result = resolve_launch(
        &PaneLaunchRef::Tool { name: name.clone() },
        &catalog,
        &NoneAvailable,
    );
    match &result {
        ResolvedPaneLaunch::ExecutableNotFound {
            name: n,
            program: p,
        } => {
            assert_eq!(n, &name);
            assert_eq!(p, "claude");
        }
        other => panic!("expected ExecutableNotFound, got {other:?}"),
    }
}

#[test]
fn inline_command_resolves_to_executable() {
    let catalog = ToolCatalog::empty();
    let spec = CommandSpec::new("cargo", ["test", "--workspace"]);
    let result = resolve_launch(
        &PaneLaunchRef::InlineCommand { spec },
        &catalog,
        &AllAvailable,
    );
    match &result {
        ResolvedPaneLaunch::Executable {
            command,
            program,
            args,
            program_path,
            baseline_kind,
            ..
        } => {
            assert_eq!(command, "cargo test --workspace");
            assert_eq!(*baseline_kind, PaneKind::Terminal);
            assert_eq!(program, "cargo");
            assert_eq!(args, &vec!["test".to_string(), "--workspace".to_string()]);
            assert!(
                program_path.is_none(),
                "inline commands have no resolved path"
            );
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

#[test]
fn collect_degraded_finds_all_four_degraded_kinds() {
    let resolved = vec![
        ResolvedPaneLaunch::EmptyShell,
        ResolvedPaneLaunch::CatalogMissing {
            name: ToolName::new("ghost").unwrap(),
        },
        ResolvedPaneLaunch::Executable {
            command: "lazygit".to_owned(),
            program: "lazygit".to_owned(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
            program_path: Some("lazygit".to_owned()),
            baseline_kind: PaneKind::GitUi,
        },
        ResolvedPaneLaunch::DisabledTool {
            name: ToolName::new("codex").unwrap(),
        },
        ResolvedPaneLaunch::ExecutableNotFound {
            name: ToolName::new("aider").unwrap(),
            program: "aider".to_owned(),
        },
        ResolvedPaneLaunch::InvalidConfig {
            name: ToolName::new("broken").unwrap(),
            reason: "empty program".to_owned(),
        },
    ];
    let degraded = collect_degraded(&resolved);
    assert_eq!(degraded.len(), 4);
    assert_eq!(degraded[0].0, 1);
    assert_eq!(degraded[1].0, 3);
    assert_eq!(degraded[2].0, 4);
    assert_eq!(degraded[3].0, 5);
}

#[test]
fn effective_catalog_from_config_uses_global_tools() {
    let mut custom = ToolCatalog::empty();
    let name = ToolName::new("custom-tool").unwrap();
    custom
        .add(ToolEntry::new(
            name.clone(),
            "custom",
            CommandSpec::new("custom", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();

    let cfg = StubConfig::empty().with_global(AppConfig {
        tools: Some(ToolCatalogConfig {
            catalog: custom.clone(),
        }),
        ..Default::default()
    });

    let effective = effective_catalog_from_config(&cfg, None).unwrap();
    assert!(effective.resolve(&name).is_some());
    let claude = ToolName::new("claude").unwrap();
    assert!(effective.resolve(&claude).is_none());
}

#[test]
fn effective_catalog_from_config_falls_back_to_builtin() {
    let cfg = StubConfig::empty();
    let effective = effective_catalog_from_config(&cfg, None).unwrap();
    let claude = ToolName::new("claude").unwrap();
    assert!(effective.resolve(&claude).is_some());
}

#[test]
fn effective_catalog_from_config_project_overrides_global() {
    let mut proj_catalog = ToolCatalog::empty();
    let name = ToolName::new("proj-only").unwrap();
    proj_catalog
        .add(ToolEntry::new(
            name.clone(),
            "project-local tool",
            CommandSpec::new("proj-only", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();

    let cfg = StubConfig::empty()
        .with_global(AppConfig {
            tools: Some(ToolCatalogConfig::default()),
            ..Default::default()
        })
        .with_project(AppConfig {
            tools: Some(ToolCatalogConfig {
                catalog: proj_catalog,
            }),
            ..Default::default()
        });

    let root = camino::Utf8PathBuf::from("/tmp/proj");
    let effective = effective_catalog_from_config(&cfg, Some(&root)).unwrap();
    assert!(effective.resolve(&name).is_some());
    let claude = ToolName::new("claude").unwrap();
    assert!(
        effective.resolve(&claude).is_none(),
        "project catalog replaces global, not merges"
    );
}

#[test]
fn launch_resolution_uses_effective_catalog_not_builtin_defaults() {
    let mut custom = ToolCatalog::empty();
    let name = ToolName::new("claude").unwrap();
    custom
        .add(ToolEntry::new(
            name.clone(),
            "custom claude",
            CommandSpec::new("my-claude-wrapper", ["--custom"]),
            ToolKind::Agent,
        ))
        .unwrap();

    let cfg = StubConfig::empty().with_global(AppConfig {
        tools: Some(ToolCatalogConfig { catalog: custom }),
        ..Default::default()
    });
    let effective = effective_catalog_from_config(&cfg, None).unwrap();
    let result = resolve_launch(&PaneLaunchRef::Tool { name }, &effective, &AllAvailable);
    match &result {
        ResolvedPaneLaunch::Executable { command, .. } => {
            assert_eq!(command, "my-claude-wrapper --custom");
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

#[test]
fn open_reports_catalog_missing_tool_in_summary() {
    let catalog = ToolCatalog::builtin_defaults();
    let launches = vec![
        PaneLaunchRef::Tool {
            name: ToolName::new("ghost-tool").unwrap(),
        },
        PaneLaunchRef::EmptyShell,
    ];
    let resolved = resolve_launches(&launches, &catalog, &AllAvailable);
    let degraded = collect_degraded(&resolved);
    assert_eq!(degraded.len(), 1, "CatalogMissing must appear in degraded");
    assert!(resolved[0].is_degraded());
    assert!(
        resolved[0]
            .degradation_reason()
            .unwrap()
            .contains("not found in catalog"),
    );
}

#[test]
fn open_reports_executable_not_found_in_summary() {
    let catalog = ToolCatalog::builtin_defaults();
    let launches = vec![PaneLaunchRef::Tool {
        name: ToolName::new("claude").unwrap(),
    }];
    let resolved = resolve_launches(&launches, &catalog, &NoneAvailable);
    let degraded = collect_degraded(&resolved);
    assert_eq!(
        degraded.len(),
        1,
        "ExecutableNotFound must appear in degraded"
    );
    assert!(resolved[0].is_degraded());
    assert!(
        resolved[0]
            .degradation_reason()
            .unwrap()
            .contains("not found on PATH"),
    );
}

#[test]
fn open_reports_disabled_tool_in_summary() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    catalog.disable(&name).unwrap();
    let launches = vec![PaneLaunchRef::Tool { name: name.clone() }];
    let resolved = resolve_launches(&launches, &catalog, &AllAvailable);
    let degraded = collect_degraded(&resolved);
    assert_eq!(degraded.len(), 1, "DisabledTool must appear in degraded");
    assert!(resolved[0].is_degraded());
    assert!(
        resolved[0]
            .degradation_reason()
            .unwrap()
            .contains("disabled"),
    );
}

#[test]
fn wizard_shows_catalog_tools_and_marks_missing_without_hiding_them() {
    let catalog = ToolCatalog::builtin_defaults();
    let tool_names: Vec<_> = catalog.iter().filter(|e| e.enabled).collect();
    assert!(
        !tool_names.is_empty(),
        "builtin catalog must have enabled tools"
    );

    let launches: Vec<_> = tool_names
        .iter()
        .map(|e| PaneLaunchRef::Tool {
            name: e.name.clone(),
        })
        .collect();
    let resolved = resolve_launches(&launches, &catalog, &NoneAvailable);

    assert_eq!(
        resolved.len(),
        tool_names.len(),
        "all catalog tools must be present in resolved list (not hidden)"
    );
    for r in &resolved {
        assert!(
            r.is_degraded(),
            "all tools must be degraded when none are available: {r:?}"
        );
        assert!(
            matches!(r, ResolvedPaneLaunch::ExecutableNotFound { .. }),
            "tools in catalog but unavailable must be ExecutableNotFound, not CatalogMissing"
        );
    }
}

#[test]
fn static_capability_baseline_is_registered_before_runtime_sync() {
    use crate::domain::PaneCapability;

    let catalog = ToolCatalog::builtin_defaults();
    let launches = vec![
        PaneLaunchRef::Tool {
            name: ToolName::new("claude").unwrap(),
        },
        PaneLaunchRef::EmptyShell,
        PaneLaunchRef::Tool {
            name: ToolName::new("lazygit").unwrap(),
        },
    ];
    let resolved = resolve_launches(&launches, &catalog, &AllAvailable);

    let all_with_positions: Vec<_> = resolved
        .into_iter()
        .zip(launches.iter())
        .enumerate()
        .map(|(i, (r, origin))| (0, i, r, origin.clone()))
        .collect();
    let baseline = baseline_pane_entries(&all_with_positions);

    assert_eq!(baseline.len(), 3);

    let claude_caps = &baseline[0].capabilities;
    assert!(claude_caps.contains(&PaneCapability::MessageReceiver));
    assert!(claude_caps.contains(&PaneCapability::MessageResponder));
    assert!(claude_caps.contains(&PaneCapability::CommandExecutor));

    let shell_caps = &baseline[1].capabilities;
    let expected_shell: std::collections::BTreeSet<_> =
        [PaneCapability::CommandExecutor].into_iter().collect();
    assert_eq!(shell_caps, &expected_shell);

    let lazygit_caps = &baseline[2].capabilities;
    let expected_lazygit: std::collections::BTreeSet<_> =
        [PaneCapability::Tui].into_iter().collect();
    assert_eq!(lazygit_caps, &expected_lazygit);
}

/// Stub that always returns a config error on load — for testing error propagation.
struct FailingStubConfig {
    global_err: Option<ConfigError>,
    project_err: Option<ConfigError>,
}
impl ConfigPort for FailingStubConfig {
    fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
        match &self.global_err {
            Some(e) => Err(ConfigError::ParseFailed {
                path: camino::Utf8PathBuf::from(match e {
                    ConfigError::ParseFailed { path, .. } => path.as_str(),
                    _ => "/fake",
                }),
                message: match e {
                    ConfigError::ParseFailed { message, .. } => message.clone(),
                    _ => "injected error".to_owned(),
                },
            }),
            None => Ok(None),
        }
    }
    fn save_global(&self, _: &AppConfig) -> Result<(), ConfigError> {
        Ok(())
    }
    fn load_project(&self, _: &camino::Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
        match &self.project_err {
            Some(e) => Err(ConfigError::ParseFailed {
                path: camino::Utf8PathBuf::from(match e {
                    ConfigError::ParseFailed { path, .. } => path.as_str(),
                    _ => "/fake",
                }),
                message: match e {
                    ConfigError::ParseFailed { message, .. } => message.clone(),
                    _ => "injected error".to_owned(),
                },
            }),
            None => Ok(None),
        }
    }
    fn save_project(&self, _: &camino::Utf8Path, _: &AppConfig) -> Result<(), ConfigError> {
        Ok(())
    }
    fn global_exists(&self) -> Result<bool, ConfigError> {
        Ok(false)
    }
    fn project_exists(&self, _: &camino::Utf8Path) -> Result<bool, ConfigError> {
        Ok(false)
    }
    fn load_raw_toml(&self, _: Option<&camino::Utf8Path>, _: bool) -> Result<String, ConfigError> {
        Ok(String::new())
    }
    fn save_raw_toml(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
        _: &str,
    ) -> Result<(), ConfigError> {
        Ok(())
    }
    fn load_env(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
    ) -> Result<Vec<(String, String)>, ConfigError> {
        Ok(Vec::new())
    }
    fn save_env(
        &self,
        _: Option<&camino::Utf8Path>,
        _: bool,
        _: &[(String, String)],
    ) -> Result<(), ConfigError> {
        Ok(())
    }
    fn global_config_dir(&self) -> Result<camino::Utf8PathBuf, ConfigError> {
        Ok(camino::Utf8PathBuf::from("/tmp/failing-stub-config"))
    }
}

#[test]
fn invalid_tool_catalog_config_on_open_must_fail_not_fallback() {
    let config = FailingStubConfig {
        global_err: None,
        project_err: Some(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/project/config.toml"),
            message: "bad toml syntax".to_owned(),
        }),
    };
    let result =
        effective_catalog_from_config(&config, Some(camino::Utf8Path::new("/some/project")));
    assert!(
        result.is_err(),
        "must propagate config error, not silently fallback to builtin defaults"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, ConfigError::ParseFailed { .. }),
        "error type must be preserved: {err:?}"
    );
}

#[test]
fn invalid_tool_catalog_config_on_wizard_must_fail_not_fallback() {
    let config = FailingStubConfig {
        global_err: Some(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/fake/global.toml"),
            message: "unclosed table header".to_owned(),
        }),
        project_err: None,
    };
    let result = effective_catalog_from_config(&config, None);
    assert!(
        result.is_err(),
        "wizard path must also propagate config error, not fallback"
    );
}

// ── Capability derivation uses ToolKind metadata, not name matching ──

#[test]
fn custom_agent_tool_gets_agent_capabilities_via_kind_metadata() {
    use crate::domain::PaneCapability;

    let mut catalog = ToolCatalog::empty();
    catalog
        .add(ToolEntry::new(
            ToolName::new("my-unique-agent").unwrap(),
            "Custom agent with a unique name",
            CommandSpec::new("my-unique-agent-binary", ["--mode=ai"]),
            ToolKind::Agent,
        ))
        .unwrap();

    let result = resolve_launch(
        &PaneLaunchRef::Tool {
            name: ToolName::new("my-unique-agent").unwrap(),
        },
        &catalog,
        &AllAvailable,
    );

    match &result {
        ResolvedPaneLaunch::Executable { baseline_kind, .. } => {
            assert_eq!(*baseline_kind, PaneKind::Agent);
            let caps = result.baseline_capabilities();
            assert!(caps.contains(&PaneCapability::MessageReceiver));
            assert!(caps.contains(&PaneCapability::MessageResponder));
            assert!(caps.contains(&PaneCapability::CommandExecutor));
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

#[test]
fn custom_tui_tool_gets_tui_capability_via_kind_metadata() {
    use crate::domain::PaneCapability;

    let mut catalog = ToolCatalog::empty();
    catalog
        .add(ToolEntry::new(
            ToolName::new("my-fancy-tui").unwrap(),
            "A TUI tool",
            CommandSpec::new("fancy-tui", [] as [&str; 0]),
            ToolKind::Tui,
        ))
        .unwrap();

    let result = resolve_launch(
        &PaneLaunchRef::Tool {
            name: ToolName::new("my-fancy-tui").unwrap(),
        },
        &catalog,
        &AllAvailable,
    );

    match &result {
        ResolvedPaneLaunch::Executable { baseline_kind, .. } => {
            assert_eq!(*baseline_kind, PaneKind::GitUi);
            let caps = result.baseline_capabilities();
            assert!(caps.contains(&PaneCapability::Tui));
            assert!(!caps.contains(&PaneCapability::MessageReceiver));
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

#[test]
fn cli_tool_gets_command_executor_via_kind_metadata() {
    use crate::domain::PaneCapability;

    let mut catalog = ToolCatalog::empty();
    catalog
        .add(ToolEntry::new(
            ToolName::new("my-cli-tool").unwrap(),
            "A CLI tool",
            CommandSpec::new("my-cli", ["--verbose"]),
            ToolKind::Cli,
        ))
        .unwrap();

    let result = resolve_launch(
        &PaneLaunchRef::Tool {
            name: ToolName::new("my-cli-tool").unwrap(),
        },
        &catalog,
        &AllAvailable,
    );

    match &result {
        ResolvedPaneLaunch::Executable { baseline_kind, .. } => {
            assert_eq!(*baseline_kind, PaneKind::Terminal);
            let caps = result.baseline_capabilities();
            assert!(caps.contains(&PaneCapability::CommandExecutor));
            assert!(!caps.contains(&PaneCapability::MessageReceiver));
        }
        other => panic!("expected Executable, got {other:?}"),
    }
}

// ── InvalidConfig integration tests ──────────────────────────────────

#[test]
fn malformed_tool_resolves_to_invalid_config() {
    let mut catalog = ToolCatalog::empty();
    let name = ToolName::new("bad-tool").unwrap();
    catalog
        .add(ToolEntry::new(
            name.clone(),
            "tool with empty program",
            CommandSpec::new("", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();

    let result = resolve_launch(
        &PaneLaunchRef::Tool { name: name.clone() },
        &catalog,
        &AllAvailable,
    );

    match &result {
        ResolvedPaneLaunch::InvalidConfig { name: n, reason: r } => {
            assert_eq!(n, &name);
            assert!(r.contains("program must not be empty"));
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
}

#[test]
fn validate_runs_before_exec_check() {
    let mut catalog = ToolCatalog::empty();
    let name = ToolName::new("bad-tool").unwrap();
    catalog
        .add(ToolEntry::new(
            name.clone(),
            "tool with empty program",
            CommandSpec::new("", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();

    let result = resolve_launch(
        &PaneLaunchRef::Tool { name: name.clone() },
        &catalog,
        &NoneAvailable,
    );

    assert!(
        matches!(result, ResolvedPaneLaunch::InvalidConfig { .. }),
        "malformed entry must be InvalidConfig, not ExecutableNotFound: {result:?}"
    );
}

#[test]
fn invalid_config_and_tool_kind_coexist_in_resolve_tool() {
    let mut catalog = ToolCatalog::empty();

    let valid_name = ToolName::new("good-agent").unwrap();
    catalog
        .add(ToolEntry::new(
            valid_name.clone(),
            "valid agent",
            CommandSpec::new("good-agent-bin", [] as [&str; 0]),
            ToolKind::Agent,
        ))
        .unwrap();

    let invalid_name = ToolName::new("bad-agent").unwrap();
    catalog
        .add(ToolEntry::new(
            invalid_name.clone(),
            "invalid agent with empty program",
            CommandSpec::new("", [] as [&str; 0]),
            ToolKind::Agent,
        ))
        .unwrap();

    let valid_result = resolve_launch(
        &PaneLaunchRef::Tool {
            name: valid_name.clone(),
        },
        &catalog,
        &AllAvailable,
    );
    match &valid_result {
        ResolvedPaneLaunch::Executable { baseline_kind, .. } => {
            assert_eq!(*baseline_kind, PaneKind::Agent);
        }
        other => panic!("expected Executable for valid agent, got {other:?}"),
    }

    let invalid_result = resolve_launch(
        &PaneLaunchRef::Tool {
            name: invalid_name.clone(),
        },
        &catalog,
        &AllAvailable,
    );
    assert!(
        matches!(invalid_result, ResolvedPaneLaunch::InvalidConfig { .. }),
        "invalid agent must be InvalidConfig: {invalid_result:?}"
    );
}

#[test]
fn open_reports_invalid_config_in_summary() {
    let mut catalog = ToolCatalog::empty();
    let name = ToolName::new("broken").unwrap();
    catalog
        .add(ToolEntry::new(
            name.clone(),
            "broken tool",
            CommandSpec::new("", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();

    let launches = vec![PaneLaunchRef::Tool { name: name.clone() }];
    let resolved = resolve_launches(&launches, &catalog, &AllAvailable);
    let degraded = collect_degraded(&resolved);

    assert_eq!(
        degraded.len(),
        1,
        "InvalidConfig must appear in degraded list"
    );
    assert!(resolved[0].is_degraded());
    assert!(
        resolved[0]
            .degradation_reason()
            .unwrap()
            .contains("invalid config"),
    );
}

// ── classify_split_command ──────────────────────────────────────────
//
// The split path must immediately classify the user-supplied
// `--cmd` value into a typed `PaneLaunchRef` so the durable
// identity store and pane title can be populated before the user
// runs `pane list`. These tests pin down both happy-path tool
// recognition and the conservative misclassification guard for
// arbitrary shell snippets.

#[test]
fn classify_split_command_none_is_empty_shell() {
    let catalog = ToolCatalog::builtin_defaults();
    assert_eq!(
        classify_split_command(None, &catalog),
        PaneLaunchRef::EmptyShell
    );
}

#[test]
fn classify_split_command_blank_is_empty_shell() {
    let catalog = ToolCatalog::builtin_defaults();
    assert_eq!(
        classify_split_command(Some("   "), &catalog),
        PaneLaunchRef::EmptyShell
    );
}

#[test]
fn classify_split_command_exact_known_tool_is_tool() {
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("codex"), &catalog);
    match result {
        PaneLaunchRef::Tool { name } => {
            assert_eq!(name.as_str(), "codex");
        }
        other => panic!("expected Tool, got {other:?}"),
    }
}

#[test]
fn classify_split_command_known_tool_with_args_is_tool() {
    // When the first whitespace-separated token is a
    // catalog tool, the split must record durable identity as
    // that tool so `pane list` shows the user-recognizable label
    // (`codex`, `claude`, …) immediately. The user's full
    // `--cmd` was already executed verbatim by `runtime.split_pane`
    // before this classifier runs, so flagged invocations do not
    // lose their args at execution time.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("codex --model gpt-5.5"), &catalog);
    match result {
        PaneLaunchRef::Tool { name } => assert_eq!(name.as_str(), "codex"),
        other => panic!("expected Tool, got {other:?}"),
    }
}

#[test]
fn classify_split_command_known_tool_with_dangerously_flag_is_tool() {
    // Claude is commonly
    // launched as `claude --dangerously-skip-permissions`, which
    // must still record `claude` tool identity.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("claude --dangerously-skip-permissions"), &catalog);
    match result {
        PaneLaunchRef::Tool { name } => assert_eq!(name.as_str(), "claude"),
        other => panic!("expected Tool, got {other:?}"),
    }
}

#[test]
fn classify_split_command_known_tool_with_quoted_arg_is_tool() {
    // Quoted args after a known tool are still a tool launch:
    // the leading token is the catalog program, the runtime
    // already executed the full string, and only the leading
    // token determines durable identity.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("codex --prompt \"hello world\""), &catalog);
    match result {
        PaneLaunchRef::Tool { name } => assert_eq!(name.as_str(), "codex"),
        other => panic!("expected Tool, got {other:?}"),
    }
}

#[test]
fn classify_split_command_unknown_command_with_args_is_inline_not_tool() {
    // Even when the user
    // supplies a multi-token command, an unknown leading token
    // must NOT be promoted to a fake `Tool { name: ... }`.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("lazyit --foo"), &catalog);
    match result {
        PaneLaunchRef::InlineCommand { spec } => {
            assert_eq!(spec.program(), "lazyit");
            assert_eq!(spec.args(), &["--foo"]);
        }
        other => panic!("expected InlineCommand for unknown program, got {other:?}"),
    }
}

#[test]
fn classify_split_command_unknown_command_is_inline_not_tool() {
    // Conservative guard: a typed-by-mistake `lazyit` (or any
    // command outside the catalog) must NOT become a fake
    // `Tool { name: "lazyit" }`, which would corrupt the durable
    // identity store and mislead `pane list` rendering.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("lazyit"), &catalog);
    match result {
        PaneLaunchRef::InlineCommand { spec } => {
            assert_eq!(spec.program(), "lazyit");
            assert!(spec.args().is_empty());
        }
        other => panic!("expected InlineCommand, got {other:?}"),
    }
}

#[test]
fn classify_split_command_path_prefix_is_inline() {
    // Path-qualified invocations (`./tool`, `/usr/local/bin/codex`)
    // are user-managed binaries, not catalog tools. They must
    // survive as InlineCommand so display identity uses the raw
    // program string rather than fabricating a tool entry.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("/usr/local/bin/codex"), &catalog);
    match result {
        PaneLaunchRef::InlineCommand { spec } => {
            assert_eq!(spec.program(), "/usr/local/bin/codex");
            assert!(spec.args().is_empty());
        }
        other => panic!("expected InlineCommand, got {other:?}"),
    }
}

#[test]
fn classify_split_command_invalid_tool_name_is_inline() {
    // Tool names must be `[a-z0-9_-]`. An uppercase command must
    // not be silently downcased into a `Tool` — it stays inline.
    let catalog = ToolCatalog::builtin_defaults();
    let result = classify_split_command(Some("MyApp"), &catalog);
    match result {
        PaneLaunchRef::InlineCommand { spec } => {
            assert_eq!(spec.program(), "MyApp");
        }
        other => panic!("expected InlineCommand, got {other:?}"),
    }
}

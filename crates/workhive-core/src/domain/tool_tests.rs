use super::*;

// ── ToolName ──────────────────────────────────────────────────────────

#[test]
fn tool_name_valid_simple() {
    assert!(ToolName::new("claude").is_ok());
    assert!(ToolName::new("lazygit").is_ok());
    assert!(ToolName::new("my-tool").is_ok());
    assert!(ToolName::new("tool_2").is_ok());
}

#[test]
fn tool_name_rejects_empty() {
    assert_eq!(ToolName::new(""), Err(ToolNameError::Empty));
}

#[test]
fn tool_name_rejects_uppercase() {
    let err = ToolName::new("Claude").unwrap_err();
    assert!(matches!(err, ToolNameError::InvalidChars(_)));
}

#[test]
fn tool_name_rejects_spaces() {
    let err = ToolName::new("my tool").unwrap_err();
    assert!(matches!(err, ToolNameError::InvalidChars(_)));
}

#[test]
fn tool_name_display() {
    let name = ToolName::new("claude").unwrap();
    assert_eq!(name.to_string(), "claude");
}

// ── CommandSpec ───────────────────────────────────────────────────────

#[test]
fn command_spec_no_args() {
    let spec = CommandSpec::new("lazygit", [] as [&str; 0]);
    assert_eq!(spec.program(), "lazygit");
    assert!(spec.args().is_empty());
    assert_eq!(spec.to_string(), "lazygit");
}

#[test]
fn command_spec_with_args() {
    let spec = CommandSpec::new("claude", ["--dangerously-skip-permissions"]);
    assert_eq!(spec.program(), "claude");
    assert_eq!(spec.args(), &["--dangerously-skip-permissions"]);
    assert_eq!(spec.to_string(), "claude --dangerously-skip-permissions");
}

#[test]
fn command_spec_multi_arg_display() {
    let spec = CommandSpec::new("cargo", ["test", "--workspace"]);
    assert_eq!(spec.to_string(), "cargo test --workspace");
}

// ── PaneLaunchRef ─────────────────────────────────────────────────────

#[test]
fn launch_ref_empty_shell_display() {
    assert_eq!(PaneLaunchRef::EmptyShell.to_string(), "<empty shell>");
}

#[test]
fn launch_ref_tool_display() {
    let name = ToolName::new("lazygit").unwrap();
    assert_eq!(PaneLaunchRef::Tool { name }.to_string(), "tool:lazygit");
}

#[test]
fn launch_ref_inline_command_display() {
    let spec = CommandSpec::new("cargo", ["build"]);
    assert_eq!(
        PaneLaunchRef::InlineCommand { spec }.to_string(),
        "cmd:cargo build"
    );
}

#[test]
fn launch_ref_serde_round_trip() {
    let variants = vec![
        PaneLaunchRef::EmptyShell,
        PaneLaunchRef::Tool {
            name: ToolName::new("claude").unwrap(),
        },
        PaneLaunchRef::InlineCommand {
            spec: CommandSpec::new("cargo", ["test"]),
        },
    ];
    for variant in variants {
        let json = serde_json::to_string(&variant).unwrap();
        let restored: PaneLaunchRef = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, variant);
    }
}

// ── ToolCatalog ───────────────────────────────────────────────────────

#[test]
fn catalog_builtin_defaults_has_expected_tools() {
    let catalog = ToolCatalog::builtin_defaults();
    let names: Vec<&str> = catalog.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"claude"));
    assert!(names.contains(&"lazygit"));
    assert!(names.contains(&"yazi"));
    assert!(names.contains(&"codex"));
    assert!(names.contains(&"opencode"));
    assert!(
        names.contains(&"cursor-agent"),
        "Cursor Agent must ship as a built-in default so `pane ls` / `pane show` classify it without manual catalog edits"
    );
}

/// Cursor Agent must be modeled as a single `cursor-agent` tool
/// identity with `agent` as an exact process-name alias, not as
/// two separate providers. The default launch command must use
/// `--yolo` so a custom cursor-agent command is still recognized
/// as an AI pane.
#[test]
fn builtin_catalog_models_cursor_agent_as_agent_with_alias() {
    let catalog = ToolCatalog::builtin_defaults();
    let entry = catalog
        .find(&ToolName::new("cursor-agent").unwrap())
        .expect("cursor-agent must be present in the built-in default catalog");
    assert_eq!(entry.kind, Some(ToolKind::Agent));
    assert_eq!(entry.effective_kind(), ToolKind::Agent);
    assert_eq!(entry.command.program(), "cursor-agent");
    assert_eq!(
        entry.command.args(),
        &["--yolo"],
        "default Cursor Agent launch must pass --yolo per active-doc decision"
    );
    assert_eq!(
        entry.process_names,
        vec!["cursor-agent", "agent"],
        "alias `agent` must be an exact process-name match, not a separate provider"
    );
}

/// `cursor-agent` and its `agent` alias must both resolve to the
/// same canonical tool entry. Substring matches like
/// `cursor-agent-extra` or `my-cursor-agent` must NOT match per
/// the "exact basename" architecture rule.
#[test]
fn match_process_name_recognizes_cursor_agent_and_alias_only() {
    let catalog = ToolCatalog::builtin_defaults();

    let matched_canonical = catalog
        .match_process_name("cursor-agent")
        .expect("cursor-agent must match an enabled catalog entry");
    assert_eq!(matched_canonical.name.as_str(), "cursor-agent");

    let matched_alias = catalog
        .match_process_name("agent")
        .expect("agent alias must match the same Cursor Agent entry");
    assert_eq!(
        matched_alias.name.as_str(),
        "cursor-agent",
        "alias must map to the canonical Cursor Agent identity, not a new provider"
    );

    for lookalike in ["cursor-agent-extra", "my-cursor-agent", "agentx", "x-agent"] {
        assert!(
            catalog.match_process_name(lookalike).is_none(),
            "lookalike `{lookalike}` must not match the cursor-agent entry (exact-only policy)"
        );
    }
}

#[test]
fn catalog_resolve_finds_enabled_tool() {
    let catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    let entry = catalog.resolve(&name);
    assert!(entry.is_some());
    assert_eq!(entry.unwrap().name, name);
}

#[test]
fn catalog_resolve_returns_none_for_disabled() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    catalog.disable(&name).unwrap();
    assert!(catalog.resolve(&name).is_none());
}

#[test]
fn catalog_resolve_returns_none_for_unknown() {
    let catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("unknown-tool").unwrap();
    assert!(catalog.resolve(&name).is_none());
}

#[test]
fn catalog_add_and_resolve() {
    let mut catalog = ToolCatalog::empty();
    let name = ToolName::new("mytool").unwrap();
    catalog
        .add(ToolEntry::new(
            name.clone(),
            "My tool",
            CommandSpec::new("mytool", [] as [&str; 0]),
            ToolKind::Cli,
        ))
        .unwrap();
    assert!(catalog.resolve(&name).is_some());
}

#[test]
fn catalog_add_duplicate_returns_error() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    let err = catalog.add(ToolEntry::new(
        name.clone(),
        "dup",
        CommandSpec::new("claude", [] as [&str; 0]),
        ToolKind::Agent,
    ));
    assert_eq!(err, Err(CatalogError::AlreadyExists(name)));
}

#[test]
fn catalog_remove_existing_tool() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("yazi").unwrap();
    assert!(catalog.remove(&name).is_ok());
    assert!(catalog.find(&name).is_none());
}

#[test]
fn catalog_remove_nonexistent_returns_error() {
    let mut catalog = ToolCatalog::empty();
    let name = ToolName::new("ghost").unwrap();
    assert_eq!(catalog.remove(&name), Err(CatalogError::NotFound(name)));
}

#[test]
fn catalog_enable_disable_round_trip() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("lazygit").unwrap();
    catalog.disable(&name).unwrap();
    assert!(catalog.resolve(&name).is_none());
    catalog.enable(&name).unwrap();
    assert!(catalog.resolve(&name).is_some());
}

#[test]
fn catalog_enabled_iter_excludes_disabled() {
    let mut catalog = ToolCatalog::builtin_defaults();
    let name = ToolName::new("claude").unwrap();
    catalog.disable(&name).unwrap();
    let enabled_names: Vec<&str> = catalog.enabled_iter().map(|e| e.name.as_str()).collect();
    assert!(!enabled_names.contains(&"claude"));
    assert!(enabled_names.contains(&"lazygit"));
}

// ── ToolKind ──────────────────────────────────────────────────────────

#[test]
fn tool_kind_default_is_cli() {
    assert_eq!(ToolKind::default(), ToolKind::Cli);
}

#[test]
fn tool_kind_to_pane_kind_mapping() {
    use super::super::pane::PaneKind;
    assert_eq!(ToolKind::Agent.to_pane_kind(), PaneKind::Agent);
    assert_eq!(ToolKind::Tui.to_pane_kind(), PaneKind::GitUi);
    assert_eq!(ToolKind::Cli.to_pane_kind(), PaneKind::Terminal);
}

#[test]
fn tool_kind_serde_round_trip() {
    let kinds = [ToolKind::Agent, ToolKind::Tui, ToolKind::Cli];
    for kind in &kinds {
        let json = serde_json::to_string(kind).unwrap();
        let restored: ToolKind = serde_json::from_str(&json).unwrap();
        assert_eq!(*kind, restored);
    }
}

#[test]
fn tool_kind_deserialization_defaults_to_none_when_missing() {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        kind: Option<ToolKind>,
    }
    let json = "{}";
    let w: Wrapper = serde_json::from_str(json).unwrap();
    assert_eq!(w.kind, None);
}

#[test]
fn tool_kind_deserialization_preserves_explicit_value() {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        kind: Option<ToolKind>,
    }
    let json = r#"{"kind":"agent"}"#;
    let w: Wrapper = serde_json::from_str(json).unwrap();
    assert_eq!(w.kind, Some(ToolKind::Agent));
}

// ── Capability derivation from ToolKind metadata ──────────────────────

#[test]
fn capability_derivation_uses_tool_metadata_not_tool_name() {
    use super::super::pane::{PaneCapability, derive_static_capabilities};

    let agent_entry = ToolEntry::new(
        ToolName::new("my-custom-agent").unwrap(),
        "A custom AI agent",
        CommandSpec::new("my-agent", [] as [&str; 0]),
        ToolKind::Agent,
    );
    let caps = derive_static_capabilities(agent_entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));
    assert!(caps.contains(&PaneCapability::CommandExecutor));

    let tui_entry = ToolEntry::new(
        ToolName::new("my-tui").unwrap(),
        "A custom TUI",
        CommandSpec::new("my-tui", [] as [&str; 0]),
        ToolKind::Tui,
    );
    let caps = derive_static_capabilities(tui_entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::Tui));
    assert!(!caps.contains(&PaneCapability::MessageReceiver));

    let cli_entry = ToolEntry::new(
        ToolName::new("my-cli").unwrap(),
        "A CLI tool",
        CommandSpec::new("my-cli", [] as [&str; 0]),
        ToolKind::Cli,
    );
    let caps = derive_static_capabilities(cli_entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert!(!caps.contains(&PaneCapability::MessageReceiver));
}

#[test]
fn builtin_catalog_tools_have_correct_kinds() {
    let catalog = ToolCatalog::builtin_defaults();
    let claude = catalog.find(&ToolName::new("claude").unwrap()).unwrap();
    assert_eq!(claude.kind, Some(ToolKind::Agent));
    assert_eq!(claude.effective_kind(), ToolKind::Agent);
    assert_eq!(claude.command.program(), "claude");
    assert_eq!(claude.command.args(), &["--dangerously-skip-permissions"]);

    let codex = catalog.find(&ToolName::new("codex").unwrap()).unwrap();
    assert_eq!(codex.kind, Some(ToolKind::Agent));
    assert_eq!(codex.effective_kind(), ToolKind::Agent);
    assert_eq!(codex.command.program(), "codex");
    assert_eq!(
        codex.command.args(),
        &["--dangerously-bypass-approvals-and-sandbox"],
        "default Codex launch must include the bypass flag so Workhive-managed \
             panes do not stall on interactive approval/sandbox prompts"
    );
    assert_eq!(
        codex.process_names,
        vec![
            "codex",
            "codex-x86_64-apple-darwin",
            "codex-aarch64-apple-darwin",
            "codex-x86_64-unknown-linux-gnu",
            "codex-aarch64-unknown-linux-gnu",
        ],
        "Codex process/provider identity must remain based on process names, not launch args"
    );

    let opencode = catalog.find(&ToolName::new("opencode").unwrap()).unwrap();
    assert_eq!(opencode.kind, Some(ToolKind::Agent));
    assert_eq!(opencode.effective_kind(), ToolKind::Agent);

    let lazygit = catalog.find(&ToolName::new("lazygit").unwrap()).unwrap();
    assert_eq!(lazygit.kind, Some(ToolKind::Tui));
    assert_eq!(lazygit.effective_kind(), ToolKind::Tui);

    let yazi = catalog.find(&ToolName::new("yazi").unwrap()).unwrap();
    assert_eq!(yazi.kind, Some(ToolKind::Tui));
    assert_eq!(yazi.effective_kind(), ToolKind::Tui);

    let cursor_agent = catalog
        .find(&ToolName::new("cursor-agent").unwrap())
        .unwrap();
    assert_eq!(cursor_agent.kind, Some(ToolKind::Agent));
    assert_eq!(cursor_agent.effective_kind(), ToolKind::Agent);
    assert_eq!(cursor_agent.command.program(), "cursor-agent");
    assert_eq!(cursor_agent.command.args(), &["--yolo"]);
}

#[test]
fn resolved_launch_baseline_capabilities_derive_from_tool_kind() {
    use super::super::pane::PaneCapability;

    let agent_resolved = ResolvedPaneLaunch::Executable {
        command: "my-agent".to_owned(),
        program: "my-agent".to_owned(),
        args: Vec::new(),
        program_path: Some("my-agent".to_owned()),
        baseline_kind: ToolKind::Agent.to_pane_kind(),
    };
    let caps = agent_resolved.baseline_capabilities();
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));

    let tui_resolved = ResolvedPaneLaunch::Executable {
        command: "my-tui".to_owned(),
        program: "my-tui".to_owned(),
        args: Vec::new(),
        program_path: Some("my-tui".to_owned()),
        baseline_kind: ToolKind::Tui.to_pane_kind(),
    };
    let caps = tui_resolved.baseline_capabilities();
    assert!(caps.contains(&PaneCapability::Tui));
    assert!(!caps.contains(&PaneCapability::MessageReceiver));
}

// ── Legacy ToolKind inference tests ──────────────────────────────────

#[test]
fn legacy_config_without_kind_claude_gets_agent() {
    use super::super::pane::PaneCapability;

    let entry = ToolEntry {
        name: ToolName::new("claude").unwrap(),
        description: "Anthropic Claude".into(),
        command: CommandSpec::new("claude", ["--dangerously-skip-permissions"]),
        kind: None,
        process_names: Vec::new(),
        enabled: true,
    };
    assert_eq!(entry.effective_kind(), ToolKind::Agent);
    let caps =
        super::super::pane::derive_static_capabilities(entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn legacy_config_without_kind_lazygit_gets_tui() {
    use super::super::pane::PaneCapability;

    let entry = ToolEntry {
        name: ToolName::new("lazygit").unwrap(),
        description: "Terminal UI for git".into(),
        command: CommandSpec::new("lazygit", [] as [&str; 0]),
        kind: None,
        process_names: Vec::new(),
        enabled: true,
    };
    assert_eq!(entry.effective_kind(), ToolKind::Tui);
    let caps =
        super::super::pane::derive_static_capabilities(entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::Tui));
    assert!(!caps.contains(&PaneCapability::MessageReceiver));
}

#[test]
fn custom_unknown_tool_without_kind_gets_cli() {
    let entry = ToolEntry {
        name: ToolName::new("my-custom-script").unwrap(),
        description: "A custom tool".into(),
        command: CommandSpec::new("my-custom-script", [] as [&str; 0]),
        kind: None,
        process_names: Vec::new(),
        enabled: true,
    };
    assert_eq!(entry.effective_kind(), ToolKind::Cli);
}

#[test]
fn explicit_kind_cli_on_claude_overrides_inference() {
    let entry = ToolEntry {
        name: ToolName::new("claude").unwrap(),
        description: "Claude as CLI".into(),
        command: CommandSpec::new("claude", [] as [&str; 0]),
        kind: Some(ToolKind::Cli),
        process_names: Vec::new(),
        enabled: true,
    };
    assert_eq!(entry.effective_kind(), ToolKind::Cli);
}

#[test]
fn explicit_kind_agent_on_custom_tool_is_honored() {
    use super::super::pane::PaneCapability;

    let entry = ToolEntry {
        name: ToolName::new("my-private-agent").unwrap(),
        description: "My private AI agent".into(),
        command: CommandSpec::new("my-agent-bin", [] as [&str; 0]),
        kind: Some(ToolKind::Agent),
        process_names: Vec::new(),
        enabled: true,
    };
    assert_eq!(entry.effective_kind(), ToolKind::Agent);
    let caps =
        super::super::pane::derive_static_capabilities(entry.effective_kind().to_pane_kind());
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn legacy_toml_deserialization_without_kind_field() {
    let toml_str = r#"
            name = "claude"
            description = "Anthropic Claude"
            enabled = true

            [command]
            program = "claude"
            args = ["--dangerously-skip-permissions"]
        "#;
    let entry: ToolEntry = toml::from_str(toml_str).unwrap();
    assert_eq!(entry.kind, None, "kind must be None when absent from TOML");
    assert_eq!(
        entry.effective_kind(),
        ToolKind::Agent,
        "claude must infer Agent kind"
    );
}

#[test]
fn toml_deserialization_with_explicit_kind_field() {
    let toml_str = r#"
            name = "claude"
            description = "Claude as CLI"
            kind = "cli"
            enabled = true

            [command]
            program = "claude"
            args = []
        "#;
    let entry: ToolEntry = toml::from_str(toml_str).unwrap();
    assert_eq!(
        entry.kind,
        Some(ToolKind::Cli),
        "explicit kind must be preserved"
    );
    assert_eq!(
        entry.effective_kind(),
        ToolKind::Cli,
        "explicit kind overrides inference"
    );
}

#[test]
fn catalog_serializes_as_name_keyed_toml_tables() {
    let catalog = ToolCatalog::builtin_defaults();
    let toml = toml::to_string_pretty(&catalog).unwrap();

    assert!(toml.contains("[claude]"));
    assert!(toml.contains("[claude.command]"));
    assert!(toml.contains("program = \"claude\""));
    assert!(
        !toml.contains("[[entries]]"),
        "new catalog TOML must be keyed by tool name, not an entries array"
    );
}

#[test]
fn catalog_deserializes_legacy_entries_shape() {
    let toml_str = r#"
            [[entries]]
            name = "claude"
            description = "Anthropic Claude"
            kind = "agent"
            enabled = true

            [entries.command]
            program = "claude"
            args = ["--dangerously-skip-permissions"]
        "#;

    let catalog: ToolCatalog = toml::from_str(toml_str).unwrap();
    let claude = catalog.find(&ToolName::new("claude").unwrap()).unwrap();
    assert_eq!(claude.command.program(), "claude");
}

#[test]
fn catalog_deserializes_name_keyed_shape() {
    let toml_str = r#"
            [claude]
            description = "Anthropic Claude"
            kind = "agent"
            enabled = true

            [claude.command]
            program = "claude"
            args = ["--dangerously-skip-permissions"]
        "#;

    let catalog: ToolCatalog = toml::from_str(toml_str).unwrap();
    let claude = catalog.find(&ToolName::new("claude").unwrap()).unwrap();
    assert_eq!(claude.command.program(), "claude");
}

// ── ToolEntry::validate ──────────────────────────────────────────────

#[test]
fn tool_entry_validate_accepts_valid_entry() {
    let entry = ToolEntry::new(
        ToolName::new("claude").unwrap(),
        "valid tool",
        CommandSpec::new("claude", ["--flag"]),
        ToolKind::Agent,
    );
    assert!(entry.validate().is_none());
}

#[test]
fn tool_entry_validate_rejects_empty_program() {
    let entry = ToolEntry::new(
        ToolName::new("bad").unwrap(),
        "empty program",
        CommandSpec::new("", [] as [&str; 0]),
        ToolKind::Cli,
    );
    assert!(entry.validate().is_some());
    assert!(
        entry
            .validate()
            .unwrap()
            .contains("program must not be empty")
    );
}

#[test]
fn tool_entry_validate_rejects_null_in_program() {
    let entry = ToolEntry::new(
        ToolName::new("bad").unwrap(),
        "null in program",
        CommandSpec::new("pro\0gram", [] as [&str; 0]),
        ToolKind::Cli,
    );
    assert!(entry.validate().is_some());
    assert!(entry.validate().unwrap().contains("null bytes"));
}

#[test]
fn tool_entry_validate_rejects_null_in_args() {
    let entry = ToolEntry::new(
        ToolName::new("bad").unwrap(),
        "null in arg",
        CommandSpec::new("prog", ["--ok", "bad\0arg"]),
        ToolKind::Cli,
    );
    assert!(entry.validate().is_some());
    assert!(entry.validate().unwrap().contains("argument 1"));
}

// ── ResolvedPaneLaunch::InvalidConfig ────────────────────────────────

#[test]
fn resolved_invalid_config_is_degraded() {
    let r = ResolvedPaneLaunch::InvalidConfig {
        name: ToolName::new("bad").unwrap(),
        reason: "empty program".to_owned(),
    };
    assert!(r.is_degraded());
    assert!(!r.has_command());
}

#[test]
fn resolved_invalid_config_has_degradation_reason() {
    let r = ResolvedPaneLaunch::InvalidConfig {
        name: ToolName::new("bad").unwrap(),
        reason: "empty program".to_owned(),
    };
    let reason = r.degradation_reason().unwrap();
    assert!(reason.contains("bad"));
    assert!(reason.contains("invalid config"));
}

#[test]
fn resolved_invalid_config_display() {
    let r = ResolvedPaneLaunch::InvalidConfig {
        name: ToolName::new("broken").unwrap(),
        reason: "empty program".to_owned(),
    };
    let s = r.to_string();
    assert!(s.contains("INVALID_CONFIG"));
    assert!(s.contains("broken"));
}

#[test]
fn resolved_invalid_config_has_no_capabilities() {
    let r = ResolvedPaneLaunch::InvalidConfig {
        name: ToolName::new("bad").unwrap(),
        reason: "empty program".to_owned(),
    };
    assert!(r.baseline_capabilities().is_empty());
}

// ── All four degraded states are distinct ────────────────────────────

#[test]
fn all_four_degraded_states_are_distinct() {
    let name = ToolName::new("tool").unwrap();
    let missing = ResolvedPaneLaunch::CatalogMissing { name: name.clone() };
    let disabled = ResolvedPaneLaunch::DisabledTool { name: name.clone() };
    let exec_not_found = ResolvedPaneLaunch::ExecutableNotFound {
        name: name.clone(),
        program: "tool".to_owned(),
    };
    let invalid = ResolvedPaneLaunch::InvalidConfig {
        name: name.clone(),
        reason: "test".to_owned(),
    };

    assert_ne!(missing, disabled);
    assert_ne!(missing, exec_not_found);
    assert_ne!(missing, invalid);
    assert_ne!(disabled, exec_not_found);
    assert_ne!(disabled, invalid);
    assert_ne!(exec_not_found, invalid);

    assert!(missing.is_degraded());
    assert!(disabled.is_degraded());
    assert!(exec_not_found.is_degraded());
    assert!(invalid.is_degraded());

    assert!(!missing.has_command());
    assert!(!disabled.has_command());
    assert!(!exec_not_found.has_command());
    assert!(!invalid.has_command());

    assert!(
        missing
            .degradation_reason()
            .unwrap()
            .contains("not found in catalog")
    );
    assert!(disabled.degradation_reason().unwrap().contains("disabled"));
    assert!(
        exec_not_found
            .degradation_reason()
            .unwrap()
            .contains("not found on PATH")
    );
    assert!(
        invalid
            .degradation_reason()
            .unwrap()
            .contains("invalid config")
    );
}

#[test]
fn empty_shell_is_not_degraded() {
    let r = ResolvedPaneLaunch::EmptyShell;
    assert!(!r.is_degraded());
    assert!(r.degradation_reason().is_none());
}

// ── PaneLaunchRef::display_name ──────────────────────────────────────

#[test]
fn display_name_empty_shell_returns_shell() {
    assert_eq!(PaneLaunchRef::EmptyShell.display_name(), "shell");
}

#[test]
fn display_name_tool_returns_tool_name() {
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("claude").unwrap(),
    };
    assert_eq!(launch.display_name(), "claude");
}

#[test]
fn display_name_inline_command_returns_program() {
    let launch = PaneLaunchRef::InlineCommand {
        spec: CommandSpec::new("lazygit", [] as [&str; 0]),
    };
    assert_eq!(launch.display_name(), "lazygit");
}

#[test]
fn display_name_inline_command_ignores_args() {
    let launch = PaneLaunchRef::InlineCommand {
        spec: CommandSpec::new("cargo", ["test", "--workspace"]),
    };
    assert_eq!(launch.display_name(), "cargo");
}

use super::*;
use camino::{Utf8Path, Utf8PathBuf};
use cli::ListView;
use workhive_core::domain::{
    Locale, NamingContext, PaneLaunchRef, SessionNamingPolicy, ToolCatalog, ToolName,
};
use workhive_core::ports::{
    ColumnConfig, LayoutConfig, LayoutTemplate, PaneConfig, RuntimeError, WorkspaceFsPort,
    WorkspaceRuntimePort,
};
use workhive_testkit::fakes::{
    FakeI18n, FakeRuntime, FakeToolLaunchInstrumentation, RuntimeErrorInjection,
};

fn tool_launch(name: &str) -> PaneLaunchRef {
    PaneLaunchRef::Tool {
        name: ToolName::new(name).expect("valid tool name in test"),
    }
}

fn two_col_layout() -> LayoutConfig {
    LayoutConfig {
        template: LayoutTemplate::AiCodingLeft,
        columns: vec![
            ColumnConfig {
                width: 60,
                panes: vec![
                    PaneConfig {
                        height: 50,
                        launch: tool_launch("claude"),
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
                    launch: tool_launch("lazygit"),
                }],
            },
        ],
    }
}

fn setup_runtime_with_workspace() -> FakeRuntime {
    use workhive_core::ports::CreateWorkspaceRuntimeRequest;
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: camino::Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();
    runtime
}

fn test_catalog() -> ToolCatalog {
    ToolCatalog::builtin_defaults()
}

#[test]
fn apply_layout_succeeds_on_happy_path() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = two_col_layout();
    let catalog = test_catalog();
    let instrumentation = FakeToolLaunchInstrumentation::new();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        Some(&instrumentation),
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap().pane_count, 3);
}

#[test]
fn apply_layout_fails_when_list_units_fails() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    runtime.inject_errors(RuntimeErrorInjection {
        list_units: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::Other,
            detail: "backend not running".to_owned(),
        }),
        ..Default::default()
    });
    let layout = two_col_layout();
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("backend not available"),
        "expected backend error, got: {err}"
    );
}

#[test]
fn apply_layout_fails_when_respawn_command_fails() {
    // After P1, supervisor-wrapped tool launches install via
    // `respawn_unit_with_command`, not visible `send_input`. So the
    // error-propagation contract for the launch step is now expressed
    // against the typed respawn verb.
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    runtime.inject_errors(RuntimeErrorInjection {
        respawn_command: Some(RuntimeError::OperationFailed {
            message: "respawn denied".to_owned(),
        }),
        ..Default::default()
    });
    let layout = two_col_layout();
    let catalog = test_catalog();
    let instrumentation = FakeToolLaunchInstrumentation::new();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        Some(&instrumentation),
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("respawn denied"),
        "expected respawn error, got: {err}"
    );
}

#[test]
fn apply_layout_fails_when_spawn_unit_fails() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    runtime.inject_errors(RuntimeErrorInjection {
        spawn_unit: Some(RuntimeError::OperationFailed {
            message: "spawn rejected".to_owned(),
        }),
        ..Default::default()
    });
    let layout = two_col_layout();
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("spawn rejected"),
        "expected spawn error, got: {err}"
    );
}

#[test]
fn apply_layout_empty_layout_returns_error() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig::default();
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert!(result.is_err(), "empty columns should be rejected");
}

#[test]
fn apply_layout_three_equal_columns() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig {
        template: LayoutTemplate::Custom,
        columns: vec![
            ColumnConfig {
                width: 33,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
            ColumnConfig {
                width: 33,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
            ColumnConfig {
                width: 33,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
        ],
    };
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert_eq!(result.unwrap().pane_count, 3);
}

#[test]
fn apply_layout_three_rows_single_column() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig {
        template: LayoutTemplate::Custom,
        columns: vec![ColumnConfig {
            width: 100,
            panes: vec![
                PaneConfig {
                    height: 33,
                    launch: PaneLaunchRef::EmptyShell,
                },
                PaneConfig {
                    height: 33,
                    launch: PaneLaunchRef::EmptyShell,
                },
                PaneConfig {
                    height: 33,
                    launch: PaneLaunchRef::EmptyShell,
                },
            ],
        }],
    };
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert_eq!(result.unwrap().pane_count, 3);
}

#[test]
fn apply_layout_70_30_split() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig {
        template: LayoutTemplate::Custom,
        columns: vec![
            ColumnConfig {
                width: 70,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
            ColumnConfig {
                width: 30,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            },
        ],
    };
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert_eq!(result.unwrap().pane_count, 2);
}

#[test]
fn apply_layout_multicol_multirow_combo() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig {
        template: LayoutTemplate::Custom,
        columns: vec![
            ColumnConfig {
                width: 50,
                panes: vec![
                    PaneConfig {
                        height: 70,
                        launch: tool_launch("vim"),
                    },
                    PaneConfig {
                        height: 30,
                        launch: tool_launch("htop"),
                    },
                ],
            },
            ColumnConfig {
                width: 25,
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
                width: 25,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: tool_launch("lazygit"),
                }],
            },
        ],
    };
    let catalog = test_catalog();
    let instrumentation = FakeToolLaunchInstrumentation::new();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        Some(&instrumentation),
    );
    assert_eq!(result.unwrap().pane_count, 5);
}

/// Layout is applied by the post-create open phase for fresh runtimes.
#[test]
fn apply_layout_single_pane_returns_one() {
    let runtime = setup_runtime_with_workspace();
    let i18n = FakeI18n::new();
    let layout = LayoutConfig {
        template: LayoutTemplate::Single,
        columns: vec![ColumnConfig {
            width: 100,
            panes: vec![PaneConfig {
                height: 100,
                launch: PaneLaunchRef::EmptyShell,
            }],
        }],
    };
    let catalog = test_catalog();
    let result = apply_layout(
        &runtime,
        "srv",
        "sess",
        camino::Utf8Path::new("/tmp/worktree"),
        &layout,
        &catalog,
        &i18n,
        Locale::En,
        None,
    );
    assert_eq!(result.unwrap().pane_count, 1);
}

// -- build_main_workspace_entry error propagation tests --

use workhive_core::domain::NamingConfig;
use workhive_testkit::fakes::{FakeGit, FakeWorkspaceFs};

fn default_policy() -> SessionNamingPolicy {
    SessionNamingPolicy::new(NamingConfig::default())
}

#[test]
fn main_entry_propagates_current_branch_error() {
    let git = FakeGit::new("/tmp/repo");
    git.set_error("git permission denied");
    let runtime = FakeRuntime::new();
    let ws_fs = FakeWorkspaceFs::new();
    let i18n = FakeI18n::new();
    let root = Utf8PathBuf::from("/tmp/repo");
    let policy = default_policy();

    let result = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("git permission denied"),
        "expected git error, got: {err}"
    );
}

#[test]
fn main_entry_degrades_gracefully_on_has_workspace_backend_unavailable() {
    use workhive_core::domain::SessionAvailability;
    let git = FakeGit::new("/tmp/repo");
    let runtime = FakeRuntime::new();
    runtime.inject_errors(RuntimeErrorInjection {
        has_workspace: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                server: "srv".to_owned(),
            },
            detail: "server not running".to_owned(),
        }),
        ..Default::default()
    });
    let ws_fs = FakeWorkspaceFs::new();
    ws_fs
        .write_instance_id(Utf8Path::new("/tmp/repo"), "a1b2")
        .expect("set instance_id");
    let i18n = FakeI18n::new();
    let root = Utf8PathBuf::from("/tmp/repo");
    let policy = default_policy();

    let result = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
    );
    let entry = result.expect("BackendUnavailable should degrade gracefully, not propagate");
    assert_eq!(entry.session_state, SessionAvailability::ServerUnreachable);
    assert!(!entry.session_active);
}

#[test]
fn main_entry_propagates_on_list_workspaces_enrichment_error() {
    use workhive_core::ports::CreateWorkspaceRuntimeRequest;
    let git = FakeGit::new("/tmp/repo");
    let runtime = FakeRuntime::new();
    let ws_fs = FakeWorkspaceFs::new();
    ws_fs
        .write_instance_id(Utf8Path::new("/tmp/repo"), "a1b2")
        .expect("set instance_id");
    let i18n = FakeI18n::new();
    let root = Utf8PathBuf::from("/tmp/repo");
    let policy = default_policy();

    let session_name = policy
        .resolve_session(&build_naming_context("myproj", "main", &root, &git, &ws_fs))
        .expect("resolve session");
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: session_name,
            working_dir: Utf8PathBuf::from("/tmp/repo"),
            env: Vec::new(),
        })
        .unwrap();

    runtime.inject_errors(RuntimeErrorInjection {
        list_workspaces: Some(RuntimeError::PermissionDenied {
            detail: "enrichment permission denied".to_owned(),
        }),
        ..Default::default()
    });

    let result = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
    );
    assert!(matches!(
        result,
        Err(CliError::RuntimeOp(RuntimeError::PermissionDenied { .. }))
    ));
}

#[test]
fn main_entry_succeeds_with_valid_state() {
    let git = FakeGit::new("/tmp/repo");
    let runtime = FakeRuntime::new();
    let ws_fs = FakeWorkspaceFs::new();
    ws_fs
        .write_instance_id(Utf8Path::new("/tmp/repo"), "a1b2")
        .expect("set instance_id");
    let i18n = FakeI18n::new();
    let root = Utf8PathBuf::from("/tmp/repo");
    let policy = default_policy();

    let result = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
    );
    assert!(result.is_ok());
    let entry = result.unwrap();
    assert!(entry.name.contains("main"));
    assert!(!entry.worktree_exists);
    assert!(!entry.session_active);
}

// -- CommandPolicy semantic consistency tests --

use workhive_core::domain::{CommandKind, ConfirmationStage, ProjectInitBehavior};

fn workspace_cmd(action: WorkspaceAction) -> Commands {
    Commands::Workspace { action }
}

#[test]
fn policy_readonly_commands_are_consistent() {
    let readonly_commands = vec![
        workspace_cmd(WorkspaceAction::Attach { workspace: None }),
        workspace_cmd(WorkspaceAction::List {
            view: ListView::Auto,
        }),
        Commands::Project {
            action: ProjectAction::List {
                view: ListView::Auto,
            },
        },
        Commands::Session {
            action: SessionAction::List {
                view: ListView::Auto,
            },
        },
    ];
    for cmd in &readonly_commands {
        let pol = build_policy(cmd);
        assert!(
            policy::is_policy_consistent(&pol),
            "inconsistent policy for {cmd:?}: {pol:?}"
        );
        assert_eq!(
            pol.command_kind,
            CommandKind::ReadOnly,
            "{cmd:?} should be ReadOnly"
        );
        assert_eq!(
            pol.confirmation_stage,
            ConfirmationStage::None,
            "{cmd:?} should have no confirmation"
        );
    }
}

#[test]
fn policy_mutating_commands_are_consistent() {
    let confirming_commands = vec![
        workspace_cmd(WorkspaceAction::Close {
            kill: false,
            force: false,
        }),
        workspace_cmd(WorkspaceAction::Destroy {
            force: false,
            delete_branch: false,
            keep_branch: false,
        }),
        Commands::Init {
            global: true,
            backend: None,
            force: false,
        },
    ];
    for cmd in &confirming_commands {
        let pol = build_policy(cmd);
        assert_eq!(
            pol.command_kind,
            CommandKind::Mutating,
            "{cmd:?} should be Mutating"
        );
        assert_eq!(
            pol.confirmation_stage,
            ConfirmationStage::BeforeMutation,
            "{cmd:?} should confirm before mutation"
        );
    }

    // workspace open is mutating but non-confirming (no --force needed).
    let open_cmd = workspace_cmd(WorkspaceAction::Open {
        workspace: Some("test".into()),
        no_attach: false,
        force: false,
        mode: "manual".into(),
        init: false,
        attach_existing_branch: false,
    });
    let open_pol = build_policy(&open_cmd);
    assert_eq!(open_pol.command_kind, CommandKind::Mutating);
    assert_eq!(open_pol.confirmation_stage, ConfirmationStage::None);
    assert!(
        open_pol.check_deps,
        "workspace open creates/uses the current project's selected runtime backend"
    );
}

#[test]
fn policy_workspace_close_does_not_preflight_cwd_backend() {
    let cmd = workspace_cmd(WorkspaceAction::Close {
        kill: false,
        force: false,
    });
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::BeforeMutation);
    assert!(!pol.check_deps);
}

#[test]
fn policy_workspace_open_allows_auto_init_with_explicit_consent() {
    let cmd = workspace_cmd(WorkspaceAction::Open {
        workspace: Some("feat".into()),
        no_attach: false,
        force: false,
        mode: "manual".into(),
        init: false,
        attach_existing_branch: false,
    });
    let pol = build_policy(&cmd);
    assert_eq!(
        pol.project_init_behavior,
        ProjectInitBehavior::AllowAutoInitWithExplicitConsent
    );
}

#[test]
fn policy_workspace_attach_allows_global_fallback_when_uninitialized() {
    let cmd = workspace_cmd(WorkspaceAction::Attach { workspace: None });
    let pol = build_policy(&cmd);
    assert_eq!(
        pol.project_init_behavior,
        ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized
    );
    assert!(!pol.check_deps);
}

#[test]
fn policy_session_list_forbids_init() {
    let cmd = Commands::Session {
        action: SessionAction::List {
            view: ListView::Auto,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.project_init_behavior, ProjectInitBehavior::Forbidden);
}

#[test]
fn policy_session_attach_is_readonly_no_confirm() {
    // `session attach` does not mutate any state; it attaches to the
    // selected backend's existing runtime. Treat it like a read-only attach.
    let cmd = Commands::Session {
        action: SessionAction::Attach {
            target: Some("feature".into()),
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::None);
    assert!(matches!(pol.confirm, ConfirmPolicy::None));
    // Must also forbid project init (global surface).
    assert_eq!(pol.project_init_behavior, ProjectInitBehavior::Forbidden);
    assert!(!pol.check_deps);
}

#[test]
fn policy_worktree_list_is_readonly() {
    let cmd = workspace_cmd(WorkspaceAction::Worktree {
        action: Some(WorktreeAction::List),
    });
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::None);
    assert!(!pol.check_deps);
}

#[test]
fn policy_worktree_remove_is_mutating_handler_managed() {
    // `worktree remove` confirms its own destructive action via the
    // safety report + risky-confirm flow.
    let cmd = workspace_cmd(WorkspaceAction::Worktree {
        action: Some(WorktreeAction::Remove {
            worktree: Some("feature".into()),
            force: false,
            delete_branch: false,
            keep_branch: false,
        }),
    });
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::BeforeMutation);
    assert!(pol.check_deps);
    assert!(matches!(pol.confirm, ConfirmPolicy::HandlerManaged));
}

/// Regression: `workspace destroy` must use HandlerManaged confirm so
/// that project-state checks (including missing `.workhive/`) run
/// BEFORE the destructive warning + irreversible confirm. It must also
/// use `ProjectInitBehavior::NotApplicable` so the global preflight
/// never prompts the user to initialize a project mid-destroy.
#[test]
fn policy_workspace_destroy_is_handler_managed_and_not_applicable() {
    let cmd = workspace_cmd(WorkspaceAction::Destroy {
        force: false,
        delete_branch: false,
        keep_branch: false,
    });
    let pol = build_policy(&cmd);
    assert!(
        matches!(pol.confirm, ConfirmPolicy::HandlerManaged),
        "destroy confirmation must be handler-managed so that project-state \
             checks fire before the destructive warning"
    );
    assert!(
        matches!(
            pol.project_init_behavior,
            ProjectInitBehavior::NotApplicable
        ),
        "destroy must not run the global init-or-delegate preflight; it \
             inspects project state itself and short-circuits when `.workhive/` \
             is missing without ever offering to create it"
    );
}

#[test]
fn policy_workspace_remove_uses_handler_managed_confirm() {
    let cmd = workspace_cmd(WorkspaceAction::Remove {
        workspace: "feat".into(),
        delete_branch: false,
        keep_branch: false,
        force: false,
    });
    let pol = build_policy(&cmd);
    assert!(matches!(pol.confirm, ConfirmPolicy::HandlerManaged));
    assert!(pol.check_deps);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
}

#[test]
fn policy_pane_list_is_readonly() {
    let cmd = Commands::Pane {
        action: PaneAction::List {
            view: ListView::Auto,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::None);
}

#[test]
fn policy_pane_split_is_mutating() {
    let cmd = Commands::Pane {
        action: PaneAction::Split {
            direction: crate::cli::PaneSplitDirectionArg::Right,
            cmd: None,
            force: false,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::BeforeMutation);
}

#[test]
fn policy_config_show_is_readonly() {
    let cmd = Commands::Config {
        action: ConfigAction::Show {
            global: false,
            raw: false,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
}

#[test]
fn policy_config_set_is_mutating() {
    let cmd = Commands::Config {
        action: ConfigAction::Set {
            key: "language".into(),
            value: "en".into(),
            global: true,
            force: false,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::BeforeMutation);
}

#[test]
fn policy_reindex_dry_run_is_readonly() {
    let cmd = Commands::Project {
        action: ProjectAction::Reindex {
            root: vec![],
            config_roots: false,
            clear_first: false,
            dry_run: true,
            force: false,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::None);
}

#[test]
fn policy_reindex_real_is_mutating() {
    let cmd = Commands::Project {
        action: ProjectAction::Reindex {
            root: vec!["/tmp".into()],
            config_roots: false,
            clear_first: false,
            dry_run: false,
            force: false,
        },
    };
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::Mutating);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::BeforeMutation);
}

// ── resolve_runtime_server behavior under cwd drift ─────────────

fn test_naming_ctx(instance_id: &str, project: &str, workspace: &str) -> NamingContext {
    NamingContext {
        instance_id: Some(instance_id.to_owned()),
        project: project.to_owned(),
        workspace: workspace.to_owned(),
        branch: Some("main".to_owned()),
        dir_path: None,
        relative_project_path: None,
    }
}

#[test]
fn resolve_runtime_server_prefers_tmux_env_over_naming_policy() {
    let policy = default_policy();
    let ctx = test_naming_ctx("a1b2", "myproject", "main");
    let tmux_server = Some("whsrv-a1b2-myproject".to_owned());

    let server = resolve_runtime_server(&tmux_server, &policy, &ctx).unwrap();

    assert_eq!(server, "whsrv-a1b2-myproject");
}

#[test]
fn resolve_runtime_server_tmux_env_ignores_cwd_context() {
    let policy = default_policy();
    // naming_ctx simulates a "drifted" cwd pointing at a different project
    let drifted_ctx = test_naming_ctx("ffff", "other-project", "dev");
    let tmux_server = Some("whsrv-a1b2-myproject".to_owned());

    let server = resolve_runtime_server(&tmux_server, &policy, &drifted_ctx).unwrap();

    assert_eq!(
        server, "whsrv-a1b2-myproject",
        "cwd drift must not affect server when tmux env is available"
    );
}

#[test]
fn resolve_runtime_server_falls_back_to_naming_policy_without_tmux_env() {
    let policy = default_policy();
    let ctx = test_naming_ctx("a1b2", "myproject", "main");
    let tmux_server: Option<String> = None;

    let server = resolve_runtime_server(&tmux_server, &policy, &ctx).unwrap();

    assert_eq!(server, "default");
}

#[test]
fn resolve_runtime_server_fallback_is_shared_socket() {
    let policy = default_policy();
    let ctx_a = test_naming_ctx("a1b2", "project-a", "main");
    let ctx_b = test_naming_ctx("c3d4", "project-b", "dev");
    let no_tmux: Option<String> = None;

    let server_a = resolve_runtime_server(&no_tmux, &policy, &ctx_a).unwrap();
    let server_b = resolve_runtime_server(&no_tmux, &policy, &ctx_b).unwrap();

    assert_eq!(
        server_a, server_b,
        "shared socket: all projects resolve to the same server"
    );
    assert_eq!(server_a, "default");
}

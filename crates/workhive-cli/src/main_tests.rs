use super::*;
use camino::{Utf8Path, Utf8PathBuf};
use cli::ListView;
use workhive_core::domain::{
    Locale, NamingContext, PaneDescriptor, PaneKind, PaneLaunchRef, PaneRuntimeId,
    PaneRuntimeStatus, PaneState, ReattachOutcome, SessionNamingPolicy, SnapshotEventKind,
    SnapshotRevisionId, SnapshotSource, ToolCatalog, ToolName, UnitId, WorkspaceProvenance,
    WorkspaceStateKey,
};
use workhive_core::ports::{
    ColumnConfig, LayoutConfig, LayoutTemplate, PaneConfig, RuntimeError, SnapshotConfig,
    WorkspaceFsPort, WorkspaceRuntimePort,
};
use workhive_core::usecases;
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

fn sample_open_result() -> usecases::open::OpenResult {
    usecases::open::OpenResult {
        session_name: "sess".to_owned(),
        worktree_path: Utf8PathBuf::from("/tmp/repo"),
        restored_panes: 0,
        provenance: WorkspaceProvenance::ProjectRoot,
        recovery_path: usecases::open::OpenRecoveryPath::FellBackToLayout,
        snapshot_outcome: usecases::open::SnapshotOutcome::NotFound,
        outcome: usecases::open::OpenOutcome::SnapshotMissing,
        reattach_outcome: ReattachOutcome::Reattached,
        snapshot_supervisor_outcome: usecases::open::SnapshotSupervisorOutcome::NotRequested,
    }
}

fn state_key() -> WorkspaceStateKey {
    WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        String::new(),
        "main".to_owned(),
    )
}

fn sample_descriptor(unit_id: &str, launch_origin: PaneLaunchRef) -> PaneDescriptor {
    PaneDescriptor {
        runtime_id: PaneRuntimeId::derive("srv", "sess", unit_id),
        group: "srv".to_owned(),
        workspace: "sess".to_owned(),
        unit_id: UnitId::new(unit_id),
        cwd: String::new(),
        command: String::new(),
        kind: PaneKind::Terminal,
        state: PaneState::Running,
        observed_status: PaneRuntimeStatus::Running,
        role: None,
        launch_origin,
        capabilities: std::collections::BTreeSet::new(),
    }
}

#[test]
fn open_success_persists_launch_baseline_before_monitor_bootstrap() {
    use workhive_core::ports::{PaneRegistryPort, SnapshotStorePort};
    use workhive_infra::pane_registry::InMemoryPaneRegistry;
    use workhive_testkit::fakes::FakeSnapshotStore;

    let store = FakeSnapshotStore::new();
    let registry = InMemoryPaneRegistry::new();
    registry
        .upsert(sample_descriptor("unit-1", tool_launch("claude")))
        .expect("register launch metadata");

    let persisted = persist_open_launch_baseline_from_registry(
        &store,
        &registry,
        &state_key(),
        "srv",
        "sess",
        "123s",
    )
    .expect("persist baseline");

    assert_eq!(persisted, 1);
    let baseline = store
        .load_launch_baseline(&state_key())
        .expect("load baseline")
        .expect("baseline must exist before monitor starts");
    assert_eq!(baseline.entries.len(), 1);
    assert_eq!(baseline.entries[0].unit_id, "unit-1");
}

#[test]
fn first_monitor_bootstrap_requires_durable_launch_baseline() {
    use workhive_testkit::fakes::FakeSnapshotStore;

    let store = FakeSnapshotStore::new();
    let ctx = CommandContext::new();

    let err = hydrate_registry_from_baseline(&store, &state_key(), "srv", "sess", &ctx)
        .expect_err("missing launch baseline must block first monitor bootstrap");

    assert!(
        err.to_string().contains("launch baseline missing"),
        "unexpected error: {err}"
    );
}

#[test]
fn fresh_registry_hydrates_launch_metadata_from_durable_baseline() {
    use workhive_core::domain::{LaunchBaseline, LaunchBaselineEntry};
    use workhive_core::ports::{PaneRegistryPort, SnapshotStorePort};
    use workhive_testkit::fakes::FakeSnapshotStore;

    let store = FakeSnapshotStore::new();
    store
        .save_launch_baseline(&LaunchBaseline {
            state_key: state_key(),
            entries: vec![LaunchBaselineEntry {
                runtime_id: PaneRuntimeId::derive("srv", "sess", "unit-1")
                    .as_str()
                    .to_owned(),
                unit_id: "unit-1".to_owned(),
                launch_origin: tool_launch("claude"),
            }],
            updated_at: "123s".to_owned(),
        })
        .expect("save baseline");
    let ctx = CommandContext::new();

    let hydrated = hydrate_registry_from_baseline(&store, &state_key(), "srv", "sess", &ctx)
        .expect("hydrate baseline");

    assert_eq!(hydrated, 1);
    let runtime_id = PaneRuntimeId::derive("srv", "sess", "unit-1");
    let descriptor = ctx
        .pane_registry
        .get(&runtime_id)
        .expect("registry read")
        .expect("descriptor exists");
    assert_eq!(descriptor.launch_origin, tool_launch("claude"));
}

#[test]
fn reused_live_session_bootstraps_launch_baseline_when_missing() {
    use workhive_core::ports::{CreateWorkspaceRuntimeRequest, SnapshotStorePort};
    use workhive_testkit::fakes::FakeSnapshotStore;

    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/repo"),
            env: Vec::new(),
        })
        .expect("create live session");
    let store = FakeSnapshotStore::new();
    let key = state_key();

    ensure_or_bootstrap_reused_session_launch_baseline(
        &runtime, &store, &key, "srv", "sess", "123s",
    )
    .expect("bootstrap reused live session baseline");

    let baseline = store
        .load_launch_baseline(&key)
        .expect("load baseline")
        .expect("baseline exists");
    assert_eq!(baseline.entries.len(), 1);
    assert_eq!(baseline.entries[0].unit_id, "unit-0");
    assert_eq!(baseline.entries[0].launch_origin, PaneLaunchRef::EmptyShell);
}

#[test]
fn generic_snapshot_capture_does_not_write_launch_baseline() {
    use workhive_core::ports::CreateWorkspaceRuntimeRequest;
    use workhive_core::ports::SnapshotStorePort;
    use workhive_testkit::fakes::FakeSnapshotStore;

    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/repo"),
            env: Vec::new(),
        })
        .expect("create workspace");
    let store = FakeSnapshotStore::new();
    let config = SnapshotConfig::default();
    let key = state_key();
    let persist_ctx = SnapshotPersistContext {
        snapshot_runtime: &runtime,
        snapshot_store: &store,
        server: "srv",
        session: "sess",
        state_key: &key,
        snapshot_config: &config,
    };

    let revision = capture_and_persist_snapshot(
        &persist_ctx,
        SnapshotSource::ManualSave,
        SnapshotEventKind::ManualSave,
        None,
    )
    .expect("capture snapshot");

    assert_eq!(revision, Some(SnapshotRevisionId::first()));
    assert!(
        store
            .load_launch_baseline(&key)
            .expect("load baseline")
            .is_none(),
        "baseline must be written by the explicit open baseline path, not generic capture"
    );
}

#[test]
fn record_snapshot_supervisor_outcome_marks_started_on_success() {
    let mut result = sample_open_result();
    let warning = record_snapshot_supervisor_outcome(&mut result, Ok(()));
    assert!(warning.is_none());
    assert_eq!(
        result.snapshot_supervisor_outcome,
        usecases::open::SnapshotSupervisorOutcome::Started
    );
}

#[test]
fn record_snapshot_supervisor_outcome_marks_degraded_on_failure() {
    let mut result = sample_open_result();
    let warning = record_snapshot_supervisor_outcome(
        &mut result,
        Err(CliError::Runtime("spawn failed".to_owned())),
    );
    assert_eq!(warning.as_deref(), Some("spawn failed"));
    assert_eq!(
        result.snapshot_supervisor_outcome,
        usecases::open::SnapshotSupervisorOutcome::Degraded {
            reason: "spawn failed".to_owned()
        }
    );
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

/// Layout is only applied on fresh sessions. This test documents the
/// intentional design: `apply_layout` is only called when
/// `!result.reused_session && result.restored_panes == 0` in `cmd_open`.
/// Changing config and re-opening does NOT re-layout an existing session.
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
use workhive_testkit::fakes::{FakeGit, FakeSnapshot, FakeWorkspaceFs};

fn default_policy() -> SessionNamingPolicy {
    SessionNamingPolicy::new(NamingConfig::default())
}

#[test]
fn main_entry_propagates_current_branch_error() {
    let git = FakeGit::new("/tmp/repo");
    git.set_error("git permission denied");
    let runtime = FakeRuntime::new();
    let snapshot = FakeSnapshot::new();
    let ws_fs = FakeWorkspaceFs::new();
    let i18n = FakeI18n::new();
    let root = Utf8PathBuf::from("/tmp/repo");
    let policy = default_policy();

    let result = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &snapshot,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
        &|workspace| {
            Some(WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "myproj".to_owned(),
                String::new(),
                workspace.to_owned(),
            ))
        },
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
            reason: workhive_core::ports::BackendUnavailableReason::Other,
            detail: "backend not available".to_owned(),
        }),
        ..Default::default()
    });
    let snapshot = FakeSnapshot::new();
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
        &snapshot,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
        &|workspace| {
            Some(WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "myproj".to_owned(),
                String::new(),
                workspace.to_owned(),
            ))
        },
    );
    let entry = result.expect("BackendUnavailable should degrade gracefully, not propagate");
    assert_eq!(entry.session_state, SessionAvailability::ServerUnreachable);
    assert!(!entry.session_active);
}

#[test]
fn main_entry_degrades_gracefully_on_list_workspaces_enrichment_error() {
    use workhive_core::domain::SessionAvailability;
    use workhive_core::ports::CreateWorkspaceRuntimeRequest;
    let git = FakeGit::new("/tmp/repo");
    let runtime = FakeRuntime::new();
    let snapshot = FakeSnapshot::new();
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
        &snapshot,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
        &|workspace| {
            Some(WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "myproj".to_owned(),
                String::new(),
                workspace.to_owned(),
            ))
        },
    );
    let entry = result.expect("list_workspaces enrichment failure should degrade gracefully");
    assert_eq!(entry.session_state, SessionAvailability::Running);
    assert!(entry.session_active);
    assert_eq!(
        entry.pane_count, None,
        "pane_count should be None when enrichment fails"
    );
    assert_eq!(
        entry.pane_count_status,
        usecases::list::PaneCountStatus::Unknown,
        "pane_count_status must be Unknown, not NotApplicable"
    );
}

#[test]
fn main_entry_succeeds_with_valid_state() {
    let git = FakeGit::new("/tmp/repo");
    let runtime = FakeRuntime::new();
    let snapshot = FakeSnapshot::new();
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
        &snapshot,
        &root,
        "myproj",
        "srv",
        &policy,
        &i18n,
        Locale::En,
        &|workspace| {
            Some(WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "myproj".to_owned(),
                String::new(),
                workspace.to_owned(),
            ))
        },
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
    // `session attach` does not mutate any state; it just hands stdin to
    // tmux. Treat it like a read-only attach.
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
}

#[test]
fn policy_worktree_list_is_readonly() {
    let cmd = workspace_cmd(WorkspaceAction::Worktree {
        action: Some(WorktreeAction::List),
    });
    let pol = build_policy(&cmd);
    assert_eq!(pol.command_kind, CommandKind::ReadOnly);
    assert_eq!(pol.confirmation_stage, ConfirmationStage::None);
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
    assert_eq!(pol.command_kind, CommandKind::Mutating);
}

#[test]
fn policy_pane_list_is_readonly() {
    let cmd = Commands::Pane {
        action: PaneAction::List,
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
    }
}

#[test]
fn resolve_runtime_server_prefers_tmux_env_over_naming_policy() {
    let policy = default_policy();
    let ctx = test_naming_ctx("a1b2", "myproject", "main");
    let tmux_server = Some("whsrv-a1b2-myproject".to_owned());

    let (server, from_env) = resolve_runtime_server(&tmux_server, &policy, &ctx).unwrap();

    assert_eq!(server, "whsrv-a1b2-myproject");
    assert!(from_env, "server should come from tmux environment");
}

#[test]
fn resolve_runtime_server_tmux_env_ignores_cwd_context() {
    let policy = default_policy();
    // naming_ctx simulates a "drifted" cwd pointing at a different project
    let drifted_ctx = test_naming_ctx("ffff", "other-project", "dev");
    let tmux_server = Some("whsrv-a1b2-myproject".to_owned());

    let (server, from_env) = resolve_runtime_server(&tmux_server, &policy, &drifted_ctx).unwrap();

    assert_eq!(
        server, "whsrv-a1b2-myproject",
        "cwd drift must not affect server when tmux env is available"
    );
    assert!(from_env);
}

#[test]
fn resolve_runtime_server_falls_back_to_naming_policy_without_tmux_env() {
    let policy = default_policy();
    let ctx = test_naming_ctx("a1b2", "myproject", "main");
    let tmux_server: Option<String> = None;

    let (server, from_env) = resolve_runtime_server(&tmux_server, &policy, &ctx).unwrap();

    assert_eq!(server, "whsrv-a1b2-myproject");
    assert!(!from_env, "server should come from naming policy fallback");
}

#[test]
fn resolve_runtime_server_fallback_is_cwd_dependent() {
    let policy = default_policy();
    let ctx_a = test_naming_ctx("a1b2", "project-a", "main");
    let ctx_b = test_naming_ctx("c3d4", "project-b", "dev");
    let no_tmux: Option<String> = None;

    let (server_a, from_a) = resolve_runtime_server(&no_tmux, &policy, &ctx_a).unwrap();
    let (server_b, from_b) = resolve_runtime_server(&no_tmux, &policy, &ctx_b).unwrap();

    assert_ne!(
        server_a, server_b,
        "without tmux env, different cwds yield different servers"
    );
    assert!(!from_a);
    assert!(!from_b);
}

#[test]
fn policy_internal_snapshot_commands_are_readonly() {
    use cli::InternalSnapshotAction;
    let command = Commands::Internal {
        action: InternalAction::Snapshot {
            action: InternalSnapshotAction::Notify {
                project_root: "/tmp/proj".to_owned(),
                server: "srv".to_owned(),
                session: "sess".to_owned(),
                state_key: "a1@proj@main".to_owned(),
                snapshot_event: "split_window".to_owned(),
            },
        },
    };
    let policy = build_policy(&command);
    assert_eq!(policy.command_kind, CommandKind::ReadOnly);
    assert_eq!(policy.confirmation_stage, ConfirmationStage::None);
}

#[test]
fn parse_snapshot_event_kind_maps_expected_values() {
    assert_eq!(
        parse_snapshot_event_kind("new_window"),
        SnapshotEventKind::WindowAdded
    );
    assert_eq!(
        parse_snapshot_event_kind("split_window"),
        SnapshotEventKind::PaneAdded
    );
    assert_eq!(
        parse_snapshot_event_kind("kill_window"),
        SnapshotEventKind::WindowRemoved
    );
    assert_eq!(
        parse_snapshot_event_kind("kill_pane"),
        SnapshotEventKind::PaneRemoved
    );
    assert_eq!(
        parse_snapshot_event_kind("layout_changed"),
        SnapshotEventKind::LayoutChanged
    );
    assert_eq!(
        parse_snapshot_event_kind("pane_died"),
        SnapshotEventKind::PaneRemoved
    );
    assert_eq!(
        parse_snapshot_event_kind("pane_exited"),
        SnapshotEventKind::PaneRemoved
    );
    assert_eq!(
        parse_snapshot_event_kind("unknown_event"),
        SnapshotEventKind::ManualSave
    );
}

#[test]
fn should_debounce_respects_window() {
    let now_ms = 2_000u128;
    assert!(should_debounce("1s", now_ms, 1_500));
    assert!(!should_debounce("1s", now_ms, 500));
    assert!(!should_debounce("garbled", now_ms, 1_500));
    assert!(!should_debounce("1s", now_ms, 0));
}

//! MVP smoke test — full workspace lifecycle using fake adapters.
//!
//! Flow: init_global → init_project → open → list → close → destroy

use camino::Utf8PathBuf;
use workhive_core::domain::{
    LaunchCapture, Locale, NamingContext, PaneDirection, RuntimeBackendId, SessionNamingPolicy,
    ToolCatalog, UnitId, WorkspaceId, WorkspaceOpenIntent, WorkspaceStateKey,
};
use workhive_core::ports::*;
use workhive_core::ports::{
    LEGACY_SNAPSHOT_SCHEMA_VERSION as SNAPSHOT_SCHEMA_VERSION,
    LegacySnapshotDocument as SnapshotDocument, LegacySnapshotIdentity as SnapshotIdentity,
    LegacySnapshotMeta as SnapshotMeta, LegacySnapshotUnit as SnapshotUnit,
};
use workhive_core::usecases;
use workhive_core::usecases::open::{OpenMode, OpenRecoveryPath};
use workhive_testkit::fakes::*;

fn test_snap_ctx(server: &str) -> usecases::close::SnapshotContext {
    usecases::close::SnapshotContext {
        instance_id: "a1b2".to_owned(),
        project: "test-project".to_owned(),
        server_name: server.to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: usecases::close::current_platform().to_owned(),
        state_key: None,
    }
}

fn test_snap_ctx_with_state_key(server: &str, workspace: &str) -> usecases::close::SnapshotContext {
    let mut ctx = test_snap_ctx(server);
    ctx.state_key = Some(format!("a1b2@test-project@{workspace}"));
    ctx
}

struct StubExecChecker;
impl ExecutableAvailabilityPort for StubExecChecker {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

fn default_catalog() -> ToolCatalog {
    ToolCatalog::builtin_defaults()
}

fn test_snapshot_doc(
    workspace: &str,
    session_name: &str,
    panes: Vec<(u32, &str, &str, bool)>,
) -> SnapshotDocument {
    SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "test-instance".to_owned(),
            project: "test-project".to_owned(),
            workspace: workspace.to_owned(),
            server_name: "wh-test".to_owned(),
            session_name: session_name.to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "test".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: panes
            .into_iter()
            .map(|(index, cwd, command, active)| SnapshotUnit {
                index,
                active,
                title: None,
                cwd: Utf8PathBuf::from(cwd),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: command.to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            })
            .collect(),
    }
}

fn setup() -> (
    FakeConfig,
    FakeGit,
    FakeRuntime,
    FakeWorkspaceFs,
    FakeSnapshot,
    FakeProjectIndex,
) {
    (
        FakeConfig::new(),
        FakeGit::new("/tmp/my-project"),
        FakeRuntime::new(),
        FakeWorkspaceFs::new(),
        FakeSnapshot::new(),
        FakeProjectIndex::new(),
    )
}

fn test_naming_ctx(project: &str, workspace: &str) -> NamingContext {
    NamingContext {
        project: project.to_owned(),
        workspace: workspace.to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some(format!("/tmp/{project}")),
        instance_id: Some("test".to_owned()),
    }
}

#[test]
fn full_lifecycle_smoke() {
    let (config, git, runtime, ws_fs, snapshot, index) = setup();
    let project_root = Utf8PathBuf::from("/tmp/my-project");
    let project_name = "my-project";
    let policy = SessionNamingPolicy::default_policy();

    // 1. Init global
    let global_cfg = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    usecases::init::init_global(&config, global_cfg).unwrap();
    assert!(config.global_exists().unwrap());

    // 2. Init project
    usecases::init::init_project(&config, &ws_fs, &index, &project_root, project_name, None)
        .unwrap();
    assert!(ws_fs.workhive_dir_exists(&project_root));
    assert_eq!(index.list_projects().unwrap().projects().len(), 1);

    // 3. Open workspace
    let ws_id = WorkspaceId::new("feat-auth").unwrap();
    let ctx = test_naming_ctx(project_name, "feat-auth");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();
    let result = usecases::open::open_workspace(
        &git,
        &runtime,
        &runtime,
        &ws_fs,
        &snapshot,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(result.recovery_path, OpenRecoveryPath::ReusedLiveSession);
    assert!(result.session_name.contains("feat-auth"));

    ws_fs.register_workspace("feat-auth");

    // 4. List workspaces
    let session_fn = |ws: &str| {
        let c = test_naming_ctx(project_name, ws);
        policy.resolve_session(&c)
    };
    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &runtime,
        &snapshot,
        &project_root,
        &server,
        &session_fn,
        &|name| {
            Some(WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "test-project".to_owned(),
                String::new(),
                name.to_owned(),
            ))
        },
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "feat-auth");
    assert!(entries[0].session_active);

    let workspaces = runtime.list_workspaces(&server).unwrap();
    assert_eq!(workspaces.len(), 1);

    // 5. Split unit
    let unit = usecases::pane::split_pane(
        &runtime,
        &server,
        &result.session_name,
        PaneDirection::Right,
        Some("vim".to_owned()),
    )
    .unwrap();
    assert_eq!(unit.command, "vim");

    // 6. List units
    let units = usecases::pane::list_panes(&runtime, &server, &result.session_name).unwrap();
    assert_eq!(units.len(), 2);

    // 7. Send input to unit
    usecases::pane::send_keys(
        &runtime,
        &server,
        &UnitId::new("unit-0"),
        "echo hello",
        true,
    )
    .unwrap();
    assert_eq!(runtime.sent_inputs().len(), 1);

    // 8. Close (kill) session
    let action = usecases::close::close_session(
        &runtime,
        &runtime,
        &snapshot,
        &server,
        &result.session_name,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx_with_state_key(&server, ws_id.as_str()),
    )
    .unwrap();
    assert!(matches!(
        action,
        usecases::close::CloseAction::Killed {
            snapshot_status: usecases::close::SnapshotPersistStatus::Saved
        }
    ));
    assert!(runtime.list_workspaces(&server).unwrap().is_empty());

    // 9. Open again to test reuse path
    let result2 = usecases::open::open_workspace(
        &git,
        &runtime,
        &runtime,
        &ws_fs,
        &snapshot,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(result2.recovery_path, OpenRecoveryPath::ReusedLiveSession);

    // 10. Destroy project
    let worktree_base = project_root.join(".workhive").join("worktrees");
    let managed_paths: Vec<camino::Utf8PathBuf> = git
        .list_worktrees()
        .unwrap()
        .into_iter()
        .filter(|wt| !wt.is_main && wt.path.starts_with(&worktree_base))
        .map(|wt| wt.path)
        .collect();
    usecases::destroy::destroy_project(
        &git,
        &runtime,
        &ws_fs,
        &snapshot,
        &index,
        &project_root,
        project_name,
        &server,
        WorktreeRemovePolicy::Force,
        &managed_paths,
    )
    .unwrap();
    assert!(!ws_fs.workhive_dir_exists(&project_root));
    assert!(index.list_projects().unwrap().projects().is_empty());
}

#[test]
fn init_global_twice_without_force_errors() {
    let config = FakeConfig::new();
    let cfg = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    usecases::init::init_global(&config, cfg).unwrap();
    let cfg2 = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    let result = usecases::init::init_global(&config, cfg2);
    assert!(result.is_err());
}

#[test]
fn init_project_without_global_errors() {
    let config = FakeConfig::new();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let root = Utf8PathBuf::from("/tmp/proj");

    let result = usecases::init::init_project(&config, &ws_fs, &index, &root, "proj", None);
    assert!(result.is_err());
}

#[test]
fn open_reuses_existing_session() {
    let (_config, git, runtime, ws_fs, snapshot, _index) = setup();
    let project_root = Utf8PathBuf::from("/tmp/my-project");
    let policy = SessionNamingPolicy::default_policy();

    let ws_id = WorkspaceId::new("feat-reuse").unwrap();
    let ctx = test_naming_ctx("my-project", "feat-reuse");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    let r1 = usecases::open::open_workspace(
        &git,
        &runtime,
        &runtime,
        &ws_fs,
        &snapshot,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(r1.recovery_path, OpenRecoveryPath::ReusedLiveSession);
    assert!(runtime.has_workspace(&server, &r1.session_name).unwrap());

    let r2 = usecases::open::open_workspace(
        &git,
        &runtime,
        &runtime,
        &ws_fs,
        &snapshot,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_eq!(
        r2.recovery_path,
        OpenRecoveryPath::ReusedLiveSession,
        "second open should reuse existing session"
    );
    assert_eq!(r1.session_name, r2.session_name);

    let workspaces = runtime.list_workspaces(&server).unwrap();
    assert_eq!(workspaces.len(), 1);
}

#[test]
fn open_restores_snapshot_panes() {
    let (_config, git, runtime, ws_fs, snapshot, _index) = setup();
    let project_root = Utf8PathBuf::from("/tmp/my-project");
    let policy = SessionNamingPolicy::default_policy();
    let ws_id = WorkspaceId::new("feat-snap").unwrap();
    let ctx = test_naming_ctx("my-project", "feat-snap");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    let legacy_doc = test_snapshot_doc(
        "feat-snap",
        &session,
        vec![
            (0, "/tmp/pane0", "zsh", true),
            (1, "/tmp/pane1", "vim", false),
            (2, "/tmp/pane2", "cargo", false),
        ],
    );
    let legacy_json = serde_json::to_string(&legacy_doc).unwrap();
    let (snapshot_model, _) =
        usecases::snapshot_migration::migrate_to_current_schema(&legacy_json).unwrap();
    snapshot.save_revision(&snapshot_model).unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &runtime,
        &runtime,
        &ws_fs,
        &snapshot,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        true,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: snapshot_model.state_key.instance_id.clone(),
            project: snapshot_model.state_key.project_component.clone(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(snapshot_model.state_key.clone()),
        }),
    )
    .unwrap();

    assert_eq!(
        result.restored_panes, 3,
        "should restore all snapshot panes"
    );
    let units = usecases::pane::list_panes(&runtime, &server, &result.session_name).unwrap();
    assert_eq!(units.len(), 3);
    // Item 5: restore must NOT echo `cd -- /tmp/paneN` keystrokes. cwd is
    // carried through native backend capabilities:
    //   * session-level `new-session -c` for the first pane;
    //   * `split-window -c` (via `SplitPanePreparation.cwd`) for extra panes.
    // On the fake runtime these both land in the stored unit record.
    assert!(
        runtime.sent_inputs().is_empty(),
        "restore must not send any visible `cd -- ...` keystrokes, got {:?}",
        runtime.sent_inputs()
    );
    // Extra panes are split with the snapshot cwd carried through
    // `SplitPanePreparation.cwd`, so their stored cwd equals the snapshot.
    let mut extra_pane_cwds: Vec<String> = units
        .iter()
        .filter(|u| u.index != 0)
        .map(|u| u.cwd.as_str().to_owned())
        .collect();
    extra_pane_cwds.sort();
    assert_eq!(
        extra_pane_cwds,
        vec!["/tmp/pane1".to_owned(), "/tmp/pane2".to_owned()],
        "extra panes must carry the snapshot cwd via native split-window -c prep"
    );
}

#[test]
fn attach_existing_workspace_succeeds() {
    let runtime = FakeRuntime::new();
    let server = "whsrv-proj";
    let session = "wh--proj--feat-attach";

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: server.to_owned(),
            workspace_name: session.to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    assert!(runtime.has_workspace(server, session).unwrap());

    let result = runtime.attach_workspace(AttachWorkspaceRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
    });
    assert!(
        result.is_ok(),
        "attach to existing workspace should succeed"
    );
}

#[test]
fn attach_nonexistent_workspace_errors() {
    let runtime = FakeRuntime::new();
    let result = runtime.attach_workspace(AttachWorkspaceRequest {
        group: "whsrv-proj".to_owned(),
        workspace_name: "wh--proj--ghost".to_owned(),
    });
    assert!(
        result.is_err(),
        "attach to non-existent workspace should fail"
    );
}

#[test]
fn close_detach_does_not_kill_session() {
    let runtime = FakeRuntime::new();
    let snapshot = FakeSnapshot::new();
    let ws_id = WorkspaceId::new("feat-a").unwrap();
    let server = "whsrv-proj";
    let session = "wh--proj--feat-a";

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: server.to_owned(),
            workspace_name: session.to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    let action = usecases::close::close_session(
        &runtime,
        &runtime,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        false,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();
    assert!(matches!(action, usecases::close::CloseAction::Detached));
    assert!(runtime.has_workspace(server, session).unwrap());
}

// ── Naming priority: project > global > defaults ──

#[test]
fn naming_priority_defaults_when_no_config() {
    use workhive_core::domain::NamingConfig;
    let config = FakeConfig::new();
    // No global, no project → defaults
    let naming = resolve_naming(&config, None);
    let defaults = NamingConfig::default();
    assert_eq!(naming.server_template, defaults.server_template);
    assert_eq!(naming.session_template, defaults.session_template);
}

#[test]
fn naming_priority_global_overrides_defaults() {
    let config = FakeConfig::new();
    let global = AppConfig {
        tmux: Some(TmuxConfig {
            naming: Some(NamingSettings {
                server_template: Some("custom-global@{project}".to_owned()),
                session_template: None,
                sanitize: None,
                max_component_len: None,
            }),
            ..TmuxConfig::default()
        }),
        ..AppConfig::default()
    };
    config.save_global(&global).unwrap();

    let naming = resolve_naming(&config, None);
    assert_eq!(naming.server_template, "custom-global@{project}");
    // session_template falls through to default
    let defaults = workhive_core::domain::NamingConfig::default();
    assert_eq!(naming.session_template, defaults.session_template);
}

#[test]
fn naming_priority_project_overrides_global() {
    let config = FakeConfig::new();
    let root = Utf8PathBuf::from("/tmp/proj");

    let global = AppConfig {
        tmux: Some(TmuxConfig {
            naming: Some(NamingSettings {
                server_template: Some("global-srv".to_owned()),
                session_template: Some("global-sess".to_owned()),
                sanitize: None,
                max_component_len: None,
            }),
            ..TmuxConfig::default()
        }),
        ..AppConfig::default()
    };
    config.save_global(&global).unwrap();

    let project = AppConfig {
        tmux: Some(TmuxConfig {
            naming: Some(NamingSettings {
                server_template: Some("project-srv".to_owned()),
                session_template: Some("project-sess".to_owned()),
                sanitize: None,
                max_component_len: None,
            }),
            ..TmuxConfig::default()
        }),
        ..AppConfig::default()
    };
    config.save_project(&root, &project).unwrap();

    let naming = resolve_naming(&config, Some(root.as_ref()));
    assert_eq!(naming.server_template, "project-srv");
    assert_eq!(naming.session_template, "project-sess");
}

/// Helper: replicate the priority logic from `CommandContext::naming_policy_for`
/// using `ConfigPort` trait directly (testable without CLI infrastructure).
fn resolve_naming(
    config: &dyn ConfigPort,
    project_root: Option<&camino::Utf8Path>,
) -> workhive_core::domain::NamingConfig {
    let global_naming = config
        .load_global()
        .ok()
        .flatten()
        .and_then(|c| c.tmux)
        .map(|t| t.naming_config());

    let project_naming = project_root.and_then(|root| {
        config
            .load_project(root)
            .ok()
            .flatten()
            .and_then(|c| c.tmux)
            .map(|t| t.naming_config())
    });

    project_naming.or(global_naming).unwrap_or_default()
}

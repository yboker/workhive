//! MVP smoke test — full workspace lifecycle using fake adapters.
//!
//! Flow: init_global → init_project → open → list → close → destroy

use camino::Utf8PathBuf;
use workhive_core::domain::{
    Locale, NamingContext, PaneDirection, RuntimeBackendId, SessionNamingPolicy, ToolCatalog,
    UnitId, WorkspaceId, WorkspaceOpenIntent,
};
use workhive_core::ports::*;
use workhive_core::usecases;
use workhive_core::usecases::open::{OpenMode, OpenRecoveryPath};
use workhive_testkit::fakes::*;

struct StubExecChecker;
impl ExecutableAvailabilityPort for StubExecChecker {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

fn default_catalog() -> ToolCatalog {
    ToolCatalog::builtin_defaults()
}

fn setup() -> (
    FakeConfig,
    FakeGit,
    FakeRuntime,
    FakeWorkspaceFs,
    FakeProjectIndex,
) {
    let directories = workhive_testkit::fakes::FakeDirectoryRegistry::new();
    (
        FakeConfig::new(),
        FakeGit::with_directory_registry("/tmp/my-project", directories.clone()),
        FakeRuntime::new(),
        FakeWorkspaceFs::with_directory_registry(directories),
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
        relative_project_path: None,
    }
}

#[test]
fn full_lifecycle_smoke() {
    let (config, git, runtime, ws_fs, index) = setup();
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
        &ws_fs,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
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
    let entries =
        usecases::list::list_workspaces(&ws_fs, &runtime, &project_root, &server, &session_fn)
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
        &RuntimeBackendId::tmux(),
        &server,
        &result.session_name,
        true,
    )
    .unwrap();
    assert!(matches!(action, usecases::close::CloseAction::Killed));
    assert!(runtime.list_workspaces(&server).unwrap().is_empty());

    // 9. Open again to test reuse path
    let result2 = usecases::open::open_workspace(
        &git,
        &runtime,
        &ws_fs,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
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
    let binding = workhive_core::domain::RuntimeBinding::tmux(&server, &result2.session_name);
    usecases::destroy::destroy_project(
        &git,
        |binding| runtime.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &project_root,
        project_name,
        std::slice::from_ref(&binding),
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
    let (_config, git, runtime, ws_fs, _index) = setup();
    let project_root = Utf8PathBuf::from("/tmp/my-project");
    let policy = SessionNamingPolicy::default_policy();

    let ws_id = WorkspaceId::new("feat-reuse").unwrap();
    let ctx = test_naming_ctx("my-project", "feat-reuse");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    let r1 = usecases::open::open_workspace(
        &git,
        &runtime,
        &ws_fs,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
    )
    .unwrap();
    assert_ne!(r1.recovery_path, OpenRecoveryPath::ReusedLiveSession);
    assert!(runtime.has_workspace(&server, &r1.session_name).unwrap());

    let r2 = usecases::open::open_workspace(
        &git,
        &runtime,
        &ws_fs,
        &project_root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
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

    let binding = workhive_core::domain::RuntimeBinding::tmux(server, session);
    let result = runtime.attach_workspace_binding(
        &binding,
        workhive_core::ports::RuntimeInteractionPolicy::default(),
    );
    assert!(
        result.is_ok(),
        "attach to existing workspace should succeed"
    );
}

#[test]
fn attach_nonexistent_workspace_errors() {
    let runtime = FakeRuntime::new();
    let binding = workhive_core::domain::RuntimeBinding::tmux("whsrv-proj", "wh--proj--ghost");
    let result = runtime.attach_workspace_binding(
        &binding,
        workhive_core::ports::RuntimeInteractionPolicy::default(),
    );
    assert!(
        result.is_err(),
        "attach to non-existent workspace should fail"
    );
}

#[test]
fn close_detach_does_not_kill_session() {
    let runtime = FakeRuntime::new();
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

    let action =
        usecases::close::close_session(&runtime, &RuntimeBackendId::tmux(), server, session, false)
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

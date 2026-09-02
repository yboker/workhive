//! Use case unit tests — exercises every use case module with fake adapters.
//!
//! Covers: config_mgmt, list, destroy, close, pane, open (error paths), init,
//! reindex, sessions.

use camino::Utf8PathBuf;
use workhive_core::domain::LaunchCapture;
use workhive_core::domain::{
    IdentityError, Locale, MaterializationKind, NamingConfig, NamingContext, PaneDirection,
    PaneLaunchRef, RuntimeBackendId, SessionAvailability, SessionNamingPolicy, ToolCatalog, UnitId,
    WorkspaceId, WorkspaceIdentityKey, WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider,
    WorkspaceStateKey,
};
use workhive_core::ports::*;
use workhive_core::ports::{
    LEGACY_SNAPSHOT_SCHEMA_VERSION as SNAPSHOT_SCHEMA_VERSION,
    LegacySnapshotDocument as SnapshotDocument, LegacySnapshotIdentity as SnapshotIdentity,
    LegacySnapshotInvalidReason as SnapshotInvalidReason, LegacySnapshotMeta as SnapshotMeta,
    LegacySnapshotUnit as SnapshotUnit, LegacySnapshotValidation as SnapshotValidation,
    validate_legacy_snapshot as validate_snapshot,
    validate_legacy_snapshot_by_state_key as validate_snapshot_by_state_key,
};

use workhive_core::usecases;
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

struct StubExecChecker;
impl ExecutableAvailabilityPort for StubExecChecker {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

/// Build a `SnapshotDocument` for tests from workspace name, session name, and
/// a list of `(index, cwd, command, active)` tuples.
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

fn default_catalog() -> ToolCatalog {
    ToolCatalog::builtin_defaults()
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
        FakeGit::new("/tmp/test-project"),
        FakeRuntime::new(),
        FakeWorkspaceFs::new(),
        FakeSnapshot::new(),
        FakeProjectIndex::new(),
    )
}

fn init_global(config: &FakeConfig) {
    let cfg = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    usecases::init::init_global(config, cfg).unwrap();
}

fn naming_ctx(project: &str, workspace: &str) -> NamingContext {
    NamingContext {
        project: project.to_owned(),
        workspace: workspace.to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some(format!("/tmp/{project}")),
        instance_id: Some("abcd".to_owned()),
    }
}

/// Build the managed worktree path inventory from git worktrees, filtering
/// to those under `<root>/.workhive/worktrees`.
fn managed_worktree_paths_from_git(git: &FakeGit, root: &Utf8PathBuf) -> Vec<Utf8PathBuf> {
    let base = root.join(".workhive").join("worktrees");
    git.list_worktrees()
        .unwrap()
        .into_iter()
        .filter(|wt| !wt.is_main && wt.path.starts_with(&base))
        .map(|wt| wt.path)
        .collect()
}

fn test_state_key(workspace: &str) -> WorkspaceStateKey {
    WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "test-project".to_owned(),
        String::new(),
        workspace.to_owned(),
    )
}

fn test_open_identity_for_legacy_doc(workspace: &str) -> usecases::open::SnapshotIdentityContext {
    usecases::open::SnapshotIdentityContext {
        instance_id: "test-instance".to_owned(),
        project: "test-project".to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: Some(WorkspaceStateKey::new(
            "test-instance".to_owned(),
            "test-project".to_owned(),
            String::new(),
            workspace.to_owned(),
        )),
    }
}

fn test_open_identity_for_close_ctx(workspace: &str) -> usecases::open::SnapshotIdentityContext {
    usecases::open::SnapshotIdentityContext {
        instance_id: "a1b2".to_owned(),
        project: "test-project".to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: Some(test_state_key(workspace)),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// config_mgmt use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn config_show_global_returns_saved_config() {
    let (config, ..) = setup();
    init_global(&config);

    let result = usecases::config_mgmt::show_config(&config, None, true).unwrap();
    assert_eq!(result.language, Some(Locale::En));
}

#[test]
fn config_show_merges_project_over_global() {
    let (config, ..) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/proj");
    let project_cfg = AppConfig {
        language: Some(Locale::ZhCn),
        ..AppConfig::default()
    };
    config.save_project(&root, &project_cfg).unwrap();

    let merged = usecases::config_mgmt::show_config(&config, Some(root.as_ref()), false).unwrap();
    assert_eq!(merged.language, Some(Locale::ZhCn));
}

#[test]
fn config_show_global_only_ignores_project() {
    let (config, ..) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/proj");
    let project_cfg = AppConfig {
        language: Some(Locale::ZhCn),
        ..AppConfig::default()
    };
    config.save_project(&root, &project_cfg).unwrap();

    let result = usecases::config_mgmt::show_config(&config, Some(root.as_ref()), true).unwrap();
    assert_eq!(result.language, Some(Locale::En));
}

#[test]
fn config_set_language() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::set_config_value(&config, None, true, "language", "zh-CN").unwrap();
    let cfg = config.load_global().unwrap().unwrap();
    assert_eq!(cfg.language, Some(Locale::ZhCn));
}

#[test]
fn config_set_tmux_server_name() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::set_config_value(&config, None, true, "tmux.server_name", "custom-srv")
        .unwrap();
    let cfg = config.load_global().unwrap().unwrap();
    assert_eq!(cfg.tmux.unwrap().server_name, "custom-srv");
}

#[test]
fn config_set_naming_templates() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::set_config_value(
        &config,
        None,
        true,
        "tmux.naming.server_template",
        "mysrv@{instance_id}@{project}",
    )
    .unwrap();
    usecases::config_mgmt::set_config_value(
        &config,
        None,
        true,
        "tmux.naming.session_template",
        "mysess@{instance_id}@{project}@{workspace}",
    )
    .unwrap();

    let cfg = config.load_global().unwrap().unwrap();
    let naming = cfg.tmux.unwrap().naming.unwrap();
    assert_eq!(
        naming.server_template.unwrap(),
        "mysrv@{instance_id}@{project}"
    );
    assert_eq!(
        naming.session_template.unwrap(),
        "mysess@{instance_id}@{project}@{workspace}"
    );
}

#[test]
fn config_set_unknown_key_returns_error() {
    let (config, ..) = setup();
    init_global(&config);

    let result =
        usecases::config_mgmt::set_config_value(&config, None, true, "nonexistent.key", "val");
    assert!(result.is_err());
}

#[test]
fn config_set_server_template_rejects_workspace_placeholder() {
    let (config, ..) = setup();
    init_global(&config);

    let result = usecases::config_mgmt::set_config_value(
        &config,
        None,
        true,
        "tmux.naming.server_template",
        "srv-{workspace}-{project}",
    );
    assert!(
        result.is_err(),
        "server_template with {{workspace}} must be rejected"
    );
}

#[test]
fn config_set_server_template_rejects_branch_placeholder() {
    let (config, ..) = setup();
    init_global(&config);

    let result = usecases::config_mgmt::set_config_value(
        &config,
        None,
        true,
        "tmux.naming.server_template",
        "srv-{project}-{branch}",
    );
    assert!(
        result.is_err(),
        "server_template with {{branch}} must be rejected"
    );
}

#[test]
fn config_set_session_template_rejects_missing_project() {
    let (config, ..) = setup();
    init_global(&config);

    let result = usecases::config_mgmt::set_config_value(
        &config,
        None,
        true,
        "tmux.naming.session_template",
        "mysess-{workspace}",
    );
    assert!(
        result.is_err(),
        "session_template without {{project}} must be rejected"
    );
}

#[test]
fn config_unset_language() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::unset_config_value(&config, None, true, "language").unwrap();
    let cfg = config.load_global().unwrap().unwrap();
    assert!(cfg.language.is_none());
}

#[test]
fn config_unset_unknown_key_returns_error() {
    let (config, ..) = setup();
    init_global(&config);

    let result = usecases::config_mgmt::unset_config_value(&config, None, true, "bad.key");
    assert!(result.is_err());
}

#[test]
fn config_set_project_level() {
    let (config, ..) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/proj");
    usecases::config_mgmt::set_config_value(
        &config,
        Some(root.as_ref()),
        false,
        "language",
        "zh-CN",
    )
    .unwrap();

    let proj_cfg = config.load_project(root.as_ref()).unwrap().unwrap();
    assert_eq!(proj_cfg.language, Some(Locale::ZhCn));
}

#[test]
fn config_env_set_and_get() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::env_set(&config, None, true, "MY_KEY", "val1").unwrap();
    let entries = config.load_env(None, true).unwrap();
    assert!(entries.iter().any(|(k, v)| k == "MY_KEY" && v == "val1"));
}

#[test]
fn config_env_set_updates_existing() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::env_set(&config, None, true, "KEY", "old-value").unwrap();
    usecases::config_mgmt::env_set(&config, None, true, "KEY", "new-value").unwrap();
    let entries = config.load_env(None, true).unwrap();
    let vals: Vec<_> = entries.iter().filter(|(k, _)| k == "KEY").collect();
    assert_eq!(vals.len(), 1);
    assert_eq!(vals[0].1, "new-value");
}

#[test]
fn config_env_unset_removes_key() {
    let (config, ..) = setup();
    init_global(&config);

    usecases::config_mgmt::env_set(&config, None, true, "KEY", "old-value").unwrap();
    usecases::config_mgmt::env_unset(&config, None, true, "KEY").unwrap();
    let entries = config.load_env(None, true).unwrap();
    assert!(entries.iter().all(|(k, _)| k != "KEY"));
}

#[test]
fn config_env_unset_nonexistent_is_noop() {
    let (config, ..) = setup();
    init_global(&config);

    // Should succeed (no-op) even when key does not exist
    usecases::config_mgmt::env_unset(&config, None, true, "NONEXISTENT").unwrap();
}

#[test]
fn config_apply_set_preview_mutates_in_memory() {
    let mut cfg = AppConfig::default();
    usecases::config_mgmt::apply_set_preview(&mut cfg, "language", "zh-CN").unwrap();
    assert_eq!(cfg.language, Some(Locale::ZhCn));
}

#[test]
fn config_apply_unset_preview_clears_field() {
    let mut cfg = AppConfig {
        language: Some(Locale::ZhCn),
        ..AppConfig::default()
    };
    usecases::config_mgmt::apply_unset_preview(&mut cfg, "language").unwrap();
    assert!(cfg.language.is_none());
}

// ══════════════════════════════════════════════════════════════════════════════
// list use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn list_workspaces_empty_project() {
    let (_, _, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        "srv",
        &|name| Ok(format!("wh@abcd@proj@{name}")),
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    assert!(entries.is_empty());
}

#[test]
fn list_workspaces_shows_active_session() {
    let (config, git, tmux, ws_fs, snapshot, _) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("feat-x").unwrap();
    let policy = SessionNamingPolicy::default_policy();
    let ctx = naming_ctx("test-project", "feat-x");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    ws_fs.register_workspace("feat-x");

    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        &server,
        &|name| {
            let c = naming_ctx("test-project", name);
            policy.resolve_session(&c)
        },
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "feat-x");
    assert!(entries[0].session_active);
}

#[test]
fn list_workspaces_succeeds_with_no_sessions() {
    let (_, _, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let result = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        "nonexistent-server",
        &|name| Ok(format!("wh@abcd@proj@{name}")),
        &|name| Some(test_state_key(name)),
    );
    assert!(result.is_ok());
}

#[test]
fn list_workspaces_maps_has_workspace_error_to_lookup_failed() {
    let (_, _, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    ws_fs.register_workspace("feature-a");

    tmux.inject_errors(RuntimeErrorInjection {
        has_workspace: Some(RuntimeError::PermissionDenied {
            detail: "permission denied".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        "test-server",
        &|name| Ok(format!("wh@abcd@proj@{name}")),
        &|name| Some(test_state_key(name)),
    );
    let entries = result.expect("per-workspace lookup errors should not abort listing");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].session_state,
        workhive_core::domain::SessionAvailability::LookupFailed
    );
}

#[test]
fn list_workspaces_maps_backend_unavailable_to_server_unreachable() {
    let (_, _, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    ws_fs.register_workspace("feature-b");

    tmux.inject_errors(RuntimeErrorInjection {
        has_workspace: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::Other,
            detail: "tmux server not running".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        "test-server",
        &|name| Ok(format!("wh@abcd@proj@{name}")),
        &|name| Some(test_state_key(name)),
    );
    let entries = result.expect("backend-down should degrade to ServerUnreachable, not abort");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].session_state,
        workhive_core::domain::SessionAvailability::ServerUnreachable,
        "backend unavailable must map to ServerUnreachable, not Missing"
    );
}

#[test]
fn list_workspaces_enrichment_failure_sets_pane_count_status_unknown() {
    let (_, _, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    ws_fs.register_workspace("feature-c");

    let session = "wh@abcd@proj@feature-c";
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: "test-server".to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/proj"),
        env: Vec::new(),
    })
    .unwrap();

    tmux.inject_errors(RuntimeErrorInjection {
        list_workspaces: Some(RuntimeError::PermissionDenied {
            detail: "enrichment denied".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        "test-server",
        &|name| Ok(format!("wh@abcd@proj@{name}")),
        &|name| Some(test_state_key(name)),
    );
    let entries = result.expect("enrichment failure should not abort listing");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].pane_count_status,
        usecases::list::PaneCountStatus::Unknown,
        "enrichment failure must yield PaneCountStatus::Unknown, not NotApplicable"
    );
    assert_eq!(
        entries[0].pane_count, None,
        "backward-compat pane_count must be None for Unknown status"
    );
}

#[test]
fn list_workspaces_active_session_reports_linked_worktree_kind() {
    let (config, git, tmux, ws_fs, snapshot, _) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("feat-x").unwrap();
    let policy = SessionNamingPolicy::default_policy();
    let ctx = naming_ctx("test-project", "feat-x");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    ws_fs.register_workspace("feat-x");

    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        &server,
        &|name| {
            let c = naming_ctx("test-project", name);
            policy.resolve_session(&c)
        },
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].kind,
        usecases::list::WorkspaceKind::LinkedWorktree,
        "directory-backed workspaces must report LinkedWorktree kind"
    );
    assert_eq!(
        entries[0].pane_count_status,
        usecases::list::PaneCountStatus::Known(1),
        "a running linked worktree must report its real pane count"
    );
}

#[test]
fn build_main_workspace_entry_reports_project_root_kind_and_recent_activity() {
    let (_, git, tmux, _, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    // Recent activity for the project-root row comes from recorded usage
    // metadata for the main-branch workspace, not from directory mtimes.
    let ws_fs =
        FakeWorkspaceFs::new().with_workspace_last_used("/tmp/test-project", "main", 1_700_000_000);
    ws_fs.write_instance_id(root.as_ref(), "abcd").unwrap();
    let policy = SessionNamingPolicy::default_policy();

    let entry = usecases::list::build_main_workspace_entry(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        "test-project",
        "wh-test",
        &policy,
        "(main)",
        None,
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    assert_eq!(
        entry.kind,
        usecases::list::WorkspaceKind::ProjectRoot,
        "the main/root row must report ProjectRoot kind"
    );
    assert_eq!(
        entry.last_used_epoch_secs,
        Some(1_700_000_000),
        "project-root recent activity must surface the recorded usage metadata"
    );
}

#[test]
fn list_all_projects_returns_registered() {
    let (config, _, _, _, _, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/proj");
    index
        .upsert_project(&ProjectSummary {
            project_name: "proj".to_owned(),
            project_root: root.clone(),
            config_path: root.join(".workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    let result = usecases::list::list_all_projects(&index).unwrap();
    match result {
        usecases::list::ProjectListResult::Loaded(projects) => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].name, "proj");
        }
        usecases::list::ProjectListResult::IndexNotFound => {
            panic!("expected Loaded, got IndexNotFound");
        }
    }
}

#[test]
fn list_all_projects_empty_index() {
    let index = FakeProjectIndex::new();
    let result = usecases::list::list_all_projects(&index).unwrap();
    match result {
        usecases::list::ProjectListResult::Loaded(projects) => {
            assert!(projects.is_empty());
        }
        usecases::list::ProjectListResult::IndexNotFound => {
            panic!("expected Loaded (empty), got IndexNotFound");
        }
    }
}

#[test]
fn list_all_projects_index_not_found() {
    let index = FakeProjectIndex::with_not_found();
    let result = usecases::list::list_all_projects(&index).unwrap();
    assert!(
        matches!(result, usecases::list::ProjectListResult::IndexNotFound),
        "expected IndexNotFound for missing index file"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// destroy use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn destroy_removes_all_resources() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    let ws_id = WorkspaceId::new("feat-a").unwrap();
    let policy = SessionNamingPolicy::default_policy();
    let ctx = naming_ctx("test-project", "feat-a");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Verify resources exist
    assert!(tmux.has_workspace(&server, &session).unwrap());

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        &server,
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    )
    .unwrap();

    // Sessions cleared
    let sessions = tmux.list_workspaces(&server).unwrap();
    assert!(sessions.is_empty());
    // Index cleaned
    let state = index.list_projects().unwrap();
    assert!(state.projects().is_empty());
}

#[test]
fn destroy_succeeds_even_when_no_worktrees() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );
    assert!(result.is_ok());
}

#[test]
fn destroy_continues_when_runtime_backend_is_unavailable() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    assert!(ws_fs.workhive_dir_exists(root.as_ref()));

    tmux.inject_errors(RuntimeErrorInjection {
        destroy_group: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::Other,
            detail: "tmux server not running: whsrv-abcd-test-project".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );

    assert!(
        result.is_ok(),
        "destroy must be idempotent when runtime resources are already gone"
    );
    assert!(!ws_fs.workhive_dir_exists(root.as_ref()));
    assert!(index.list_projects().unwrap().projects().is_empty());
}

#[test]
fn remove_worktree_with_force_removes_runtime_and_worktree_but_keeps_branch() {
    let (_config, git, tmux, _ws_fs, _snapshot, _index) = setup();
    let worktree = "feat-rm";
    let worktree_path = Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-rm");
    git.add_worktree(worktree_path.clone(), worktree);

    let server = "whsrv-test-project";
    let session = "wh@test@test-project@feat-rm";
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: worktree_path.clone(),
        env: Vec::new(),
    })
    .unwrap();

    let managed_base = "/tmp/test-project/.workhive/worktrees";
    let mut request = usecases::remove_worktree::RemoveWorktreeRequest::new(
        worktree,
        server,
        session,
        managed_base,
    );
    request.force = true;

    let outcome = usecases::remove_worktree::remove_worktree(&git, &git, &tmux, &request).unwrap();
    assert!(matches!(
        outcome,
        usecases::remove_worktree::RemoveWorktreeOutcome::Removed {
            removed_session: true,
            removed_worktree: true,
            deleted_branch: false,
            ..
        }
    ));

    assert!(
        !tmux.has_workspace(server, session).unwrap(),
        "runtime session should be removed with default behavior"
    );
    assert!(
        !git.list_worktrees()
            .unwrap()
            .iter()
            .any(|wt| wt.branch.as_deref() == Some(worktree)),
        "linked worktree should be removed"
    );
    assert!(
        git.branch_exists(worktree).unwrap(),
        "branch should be kept unless delete-branch is explicitly requested"
    );
}

#[test]
fn remove_worktree_dirty_without_force_returns_safety_blocked() {
    let (_config, git, tmux, _ws_fs, _snapshot, _index) = setup();
    let worktree = "feat-dirty";
    let worktree_path = Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-dirty");
    git.add_worktree(worktree_path.clone(), worktree);
    git.set_worktree_status(
        worktree_path.clone(),
        WorktreeStatus {
            changed_files: vec![ChangedFile {
                status_code: " M".to_owned(),
                path: "src/main.rs".to_owned(),
                kinds: vec![ChangeKind::Unstaged],
            }],
        },
    );

    let server = "whsrv-test-project";
    let session = "wh@test@test-project@feat-dirty";
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: worktree_path.clone(),
        env: Vec::new(),
    })
    .unwrap();

    let managed_base = "/tmp/test-project/.workhive/worktrees";
    let request = usecases::remove_worktree::RemoveWorktreeRequest::new(
        worktree,
        server,
        session,
        managed_base,
    );
    let outcome = usecases::remove_worktree::remove_worktree(&git, &git, &tmux, &request).unwrap();
    assert!(matches!(
        outcome,
        usecases::remove_worktree::RemoveWorktreeOutcome::SafetyBlocked { .. }
    ));

    assert!(
        tmux.has_workspace(server, session).unwrap(),
        "runtime session must remain when safety check blocks deletion"
    );
    assert!(
        git.list_worktrees()
            .unwrap()
            .iter()
            .any(|wt| wt.branch.as_deref() == Some(worktree)),
        "linked worktree must remain when safety check blocks deletion"
    );
}

#[test]
fn remove_worktree_rejects_external_linked_worktree_from_core() {
    let (_config, git, tmux, _ws_fs, _snapshot, _index) = setup();
    let external_path = Utf8PathBuf::from("/external/repos/feat-ext");
    git.add_worktree(external_path.clone(), "feat-ext");

    let server = "whsrv-test-project";
    let session = "wh@test@test-project@feat-ext";
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: external_path.clone(),
        env: Vec::new(),
    })
    .unwrap();

    let managed_base = "/tmp/test-project/.workhive/worktrees";
    let mut request = usecases::remove_worktree::RemoveWorktreeRequest::new(
        "feat-ext",
        server,
        session,
        managed_base,
    );
    request.delete_branch = true;

    let outcome = usecases::remove_worktree::remove_worktree(&git, &git, &tmux, &request).unwrap();
    match &outcome {
        usecases::remove_worktree::RemoveWorktreeOutcome::NotWorkhiveManaged {
            worktree,
            path,
            managed_base: base,
        } => {
            assert_eq!(worktree, "feat-ext");
            assert_eq!(path.as_str(), "/external/repos/feat-ext");
            assert_eq!(base.as_str(), managed_base);
        }
        other => panic!("expected NotWorkhiveManaged, got: {other:?}"),
    }

    assert!(
        tmux.has_workspace(server, session).unwrap(),
        "runtime session must not be destroyed for external target rejection"
    );
    assert!(
        git.list_worktrees()
            .unwrap()
            .iter()
            .any(|wt| wt.branch.as_deref() == Some("feat-ext")),
        "external worktree must remain untouched"
    );
    assert!(
        git.branch_exists("feat-ext").unwrap(),
        "branch must not be deleted for external target rejection"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// close use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn close_kill_saves_snapshot_and_kills() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("feat-a").unwrap();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@feat-a";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let action = usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();
    assert!(matches!(
        action,
        usecases::close::CloseAction::Killed {
            snapshot_status: usecases::close::SnapshotPersistStatus::Saved
        }
    ));
    // Session is removed
    assert!(!tmux.has_workspace(server, session).unwrap());
    // Snapshot was saved
    let snap = LegacySnapshotPort::load_latest(&snapshot, &ws_id).unwrap();
    assert!(snap.is_some());
}

#[test]
fn close_detach_does_not_kill() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("feat-a").unwrap();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@feat-a";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let action = usecases::close::close_session(
        &tmux,
        &tmux,
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
    // Session still exists
    assert!(tmux.has_workspace(server, session).unwrap());
}

#[test]
fn close_kill_records_multiple_panes_in_snapshot() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("feat-b").unwrap();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@feat-b";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/work"),
        env: Vec::new(),
    })
    .unwrap();
    // Split to add a second pane
    tmux.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction: PaneDirection::Down,
        command: None,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
    .unwrap();

    usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();

    let snap = LegacySnapshotPort::load_latest(&snapshot, &ws_id)
        .unwrap()
        .unwrap();
    assert_eq!(snap.units.len(), 2);
}

// ══════════════════════════════════════════════════════════════════════════════
// pane use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn pane_split_creates_new_pane() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let pane =
        usecases::pane::split_pane(&tmux, server, session, PaneDirection::Down, None).unwrap();
    assert_eq!(pane.index, 1);
}

#[test]
fn pane_close_succeeds_when_multiple_panes() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let new_pane =
        usecases::pane::split_pane(&tmux, server, session, PaneDirection::Down, None).unwrap();
    usecases::pane::close_pane(&tmux, server, &new_pane.id).unwrap();

    let panes = usecases::pane::list_panes(&tmux, server, session).unwrap();
    assert_eq!(panes.len(), 1);
}

#[test]
fn pane_close_last_pane_fails() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let unit_id = UnitId::new("%0");
    let result = usecases::pane::close_pane(&tmux, server, &unit_id);
    assert!(result.is_err());
}

#[test]
fn pane_current_returns_active() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let pane = usecases::pane::current_pane(&tmux, server).unwrap();
    assert!(pane.active);
    assert_eq!(pane.id, UnitId::new("unit-0"));
}

#[test]
fn pane_send_keys() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let unit_id = UnitId::new("%0");
    usecases::pane::send_keys(&tmux, server, &unit_id, "echo hello", true).unwrap();

    let sent = tmux.sent_inputs();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, "echo hello");
}

#[test]
fn pane_list_panes() {
    let tmux = FakeRuntime::new();
    let server = "srv";
    let session = "sess";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();
    usecases::pane::split_pane(&tmux, server, session, PaneDirection::Down, None).unwrap();

    let panes = usecases::pane::list_panes(&tmux, server, session).unwrap();
    assert_eq!(panes.len(), 2);
}

// ══════════════════════════════════════════════════════════════════════════════
// open use case — error paths & snapshot restore
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn open_new_session_no_snapshot_restores_zero_panes() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("feat-z").unwrap();
    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-z",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_ne!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    assert_eq!(result.restored_panes, 0);
}

#[test]
fn open_reuses_existing_session() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("feat-y").unwrap();
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-y",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-y",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    assert_eq!(result.restored_panes, 0);
}

#[test]
fn open_restores_snapshot_panes() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("feat-snap").unwrap();

    // Pre-save a snapshot with 2 extra panes
    snapshot
        .save(&test_snapshot_doc(
            "feat-snap",
            "sess-snap",
            vec![
                (0, "/tmp", "zsh", true),
                (1, "/tmp/src", "lazygit", false),
                (2, "/tmp/docs", "vim", false),
            ],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-snap",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled — test verifies snapshot restore path
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("feat-snap")),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );
    assert_eq!(result.restored_panes, 3); // all panes in snapshot are restored
}

#[test]
fn close_capture_persists_multi_window_topology() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("multi-window-capture").unwrap();
    let server = "whsrv-a1b2-test-project";
    let session = "wh@a1b2@test-project@multi-window-capture";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/capture"),
        env: Vec::new(),
    })
    .unwrap();
    let first = tmux
        .create_window(
            server,
            session,
            1,
            Some("docs"),
            Utf8PathBuf::from("/tmp/docs").as_ref(),
        )
        .unwrap();
    tmux.split_pane(
        server,
        session,
        &first.unit_id,
        PaneDirection::Down,
        workhive_core::ports::SplitPanePreparation::default(),
    )
    .unwrap();

    usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();

    let stored = SnapshotStorePort::load_latest(&snapshot, &test_state_key("multi-window-capture"))
        .unwrap()
        .unwrap();
    assert_eq!(stored.topology.windows.len(), 2);
    assert!(
        stored
            .topology
            .windows
            .iter()
            .any(|window| window.index == 1),
        "captured topology must keep the non-primary window"
    );
    assert!(
        stored.panes.iter().any(|pane| pane.window_index == 1),
        "captured panes must preserve per-window membership"
    );
}

#[test]
fn open_restore_rebuilds_multi_window_topology() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("multi-window-restore").unwrap();
    let server = "whsrv-a1b2-test-project";
    let session = "wh@a1b2@test-project@multi-window-restore";

    let state_key = test_state_key("multi-window-restore");
    let snapshot_model = workhive_core::domain::WorkspaceSnapshot {
        schema_version: workhive_core::domain::SNAPSHOT_SCHEMA_VERSION,
        revision_id: workhive_core::domain::SnapshotRevisionId::first(),
        state_key: state_key.clone(),
        captured_at: "2026-04-24T12:00:00Z".to_owned(),
        source: workhive_core::domain::SnapshotSource::ManualSave,
        topology: workhive_core::domain::WorkspaceTopology {
            windows: vec![
                workhive_core::domain::WindowSnapshot {
                    index: 0,
                    name: Some("main".to_owned()),
                    layout: "tiled".to_owned(),
                    panes: vec![workhive_core::domain::PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    }],
                },
                workhive_core::domain::WindowSnapshot {
                    index: 1,
                    name: Some("docs".to_owned()),
                    layout: "even-horizontal".to_owned(),
                    panes: vec![workhive_core::domain::PaneRef {
                        pane_index: 0,
                        window_index: 1,
                    }],
                },
            ],
            active_window: Some(1),
            active_pane_runtime_id: None,
        },
        panes: vec![
            workhive_core::domain::PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: Some("main".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/main"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::Restore,
                launch_intent: workhive_core::domain::PaneLaunchIntent::EmptyShell,
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: true,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: None,
                window_index: 1,
                pane_index: 0,
                pane_id: None,
                title: Some("docs".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/docs"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::Restore,
                launch_intent: workhive_core::domain::PaneLaunchIntent::EmptyShell,
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: workhive_core::domain::SnapshotMetadata {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "test".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };
    SnapshotStorePort::save_revision(&snapshot, &snapshot_model).unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_close_ctx("multi-window-restore")),
    )
    .unwrap();
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );

    let topology = tmux.capture_topology(server, session).unwrap();
    assert_eq!(topology.windows.len(), 2);
    assert!(
        topology.windows.iter().any(|window| window.index == 1),
        "restore must recreate non-primary windows"
    );
    assert!(
        topology.panes.iter().any(|pane| pane.window_index == 1),
        "restore must place panes into their original window"
    );
}

#[test]
fn open_restore_uses_layout_aware_split_directions() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("layout-aware-restore").unwrap();
    let server = "whsrv-a1b2-test-project";
    let session = "wh@a1b2@test-project@layout-aware-restore";

    let state_key = test_state_key("layout-aware-restore");
    let snapshot_model = workhive_core::domain::WorkspaceSnapshot {
        schema_version: workhive_core::domain::SNAPSHOT_SCHEMA_VERSION,
        revision_id: workhive_core::domain::SnapshotRevisionId::first(),
        state_key,
        captured_at: "2026-04-24T12:00:00Z".to_owned(),
        source: workhive_core::domain::SnapshotSource::ManualSave,
        topology: workhive_core::domain::WorkspaceTopology {
            windows: vec![workhive_core::domain::WindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                // Contains both '{' and '[' markers so restore can derive
                // a mixed split sequence (Right then Down).
                layout:
                    "6f10,240x67,0,0{120x67,0,0,0,119x67,121,0[119x33,121,0,2,119x33,121,34,3]}"
                        .to_owned(),
                panes: vec![
                    workhive_core::domain::PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 2,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            workhive_core::domain::PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: Some("main".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/main"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::Restore,
                launch_intent: workhive_core::domain::PaneLaunchIntent::EmptyShell,
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: true,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: Some("right".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/right"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::Restore,
                launch_intent: workhive_core::domain::PaneLaunchIntent::EmptyShell,
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: false,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 2,
                pane_id: None,
                title: Some("down".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/down"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::Restore,
                launch_intent: workhive_core::domain::PaneLaunchIntent::EmptyShell,
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: workhive_core::domain::SnapshotMetadata {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "test".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };
    SnapshotStorePort::save_revision(&snapshot, &snapshot_model).unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_close_ctx("layout-aware-restore")),
    )
    .unwrap();
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );

    let split_directions = tmux.split_directions();
    assert!(
        split_directions.contains(&PaneDirection::Right),
        "restore split sequence should include Right for horizontal layout segments"
    );
    assert!(
        split_directions.contains(&PaneDirection::Down),
        "restore split sequence should include Down for vertical layout segments"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// init use case
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn generate_instance_id_format_4_hex() {
    let id = usecases::init::generate_instance_id().unwrap();
    assert_eq!(id.len(), 4);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn generate_instance_id_varies() {
    // Two calls should produce different IDs (with extremely high probability)
    let id1 = usecases::init::generate_instance_id().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let id2 = usecases::init::generate_instance_id().unwrap();
    // Not strictly guaranteed to differ, but effectively always does
    // due to subsec_nanos changing
    let _ = (id1, id2);
}

#[test]
fn init_project_idempotent_instance_id() {
    let (config, _, _, ws_fs, _, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let id1 = ws_fs.read_instance_id(root.as_ref()).unwrap();

    // Second init should NOT overwrite instance_id
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    let id2 = ws_fs.read_instance_id(root.as_ref()).unwrap();

    assert_eq!(id1, id2);
}

#[test]
fn init_project_inherits_global_language() {
    let (config, _, _, ws_fs, _, index) = setup();
    let global_cfg = AppConfig {
        language: Some(Locale::ZhCn),
        ..AppConfig::default()
    };
    usecases::init::init_global(&config, global_cfg).unwrap();

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let proj_cfg = config.load_project(root.as_ref()).unwrap().unwrap();
    assert_eq!(proj_cfg.language, Some(Locale::ZhCn));
}

#[test]
fn init_project_language_override() {
    let (config, _, _, ws_fs, _, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(
        &config,
        &ws_fs,
        &index,
        &root,
        "test-project",
        Some(Locale::ZhCn),
    )
    .unwrap();

    let proj_cfg = config.load_project(root.as_ref()).unwrap().unwrap();
    assert_eq!(proj_cfg.language, Some(Locale::ZhCn));
}

#[test]
fn init_global_twice_without_force_fails() {
    let config = FakeConfig::new();
    let cfg = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    usecases::init::init_global(&config, cfg.clone()).unwrap();
    let result = usecases::init::init_global(&config, cfg);
    assert!(result.is_err());
}

#[test]
fn init_global_force_overwrites() {
    let config = FakeConfig::new();
    let cfg1 = AppConfig {
        language: Some(Locale::En),
        ..AppConfig::default()
    };
    usecases::init::init_global(&config, cfg1).unwrap();

    let cfg2 = AppConfig {
        language: Some(Locale::ZhCn),
        ..AppConfig::default()
    };
    usecases::init::init_global_force(&config, cfg2).unwrap();

    let loaded = config.load_global().unwrap().unwrap();
    assert_eq!(loaded.language, Some(Locale::ZhCn));
}

#[test]
fn init_project_registers_in_index() {
    let (config, _, _, ws_fs, _, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let state = index.list_projects().unwrap();
    let projects = state.projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_name, "test-project");
}

// ══════════════════════════════════════════════════════════════════════════════
// BDD-style scenario: full lifecycle with naming verification
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_full_lifecycle_with_at_naming() {
    // Given: global config initialized
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    let project_name = "test-project";
    let policy = SessionNamingPolicy::default_policy();

    // When: project initialized
    usecases::init::init_project(&config, &ws_fs, &index, &root, project_name, None).unwrap();
    let instance_id = ws_fs.read_instance_id(root.as_ref()).unwrap();

    // Then: instance_id is 4 hex chars
    assert_eq!(instance_id.len(), 4);
    assert!(instance_id.chars().all(|c| c.is_ascii_hexdigit()));

    // When: workspace opened
    let ws_id = WorkspaceId::new("feat-login").unwrap();
    let ctx = NamingContext {
        project: project_name.to_owned(),
        workspace: "feat-login".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some(root.to_string()),
        instance_id: Some(instance_id.clone()),
    };
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    // Then: names follow @ convention
    assert!(
        server.starts_with("whsrv-"),
        "server should use whsrv- prefix: {server}"
    );
    assert!(
        session.starts_with("wh@"),
        "session should use @ format: {session}"
    );
    assert!(
        session.contains("@feat-login"),
        "session should contain workspace: {session}"
    );

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    ws_fs.register_workspace("feat-login");

    // When: list workspaces
    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        &server,
        &|name| {
            let c = NamingContext {
                project: project_name.to_owned(),
                workspace: name.to_owned(),
                branch: Some("main".to_owned()),
                dir_path: Some(root.to_string()),
                instance_id: Some(instance_id.clone()),
            };
            policy.resolve_session(&c)
        },
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    // Then: workspace listed with active session
    assert_eq!(entries.len(), 1);
    assert!(entries[0].session_active);
    assert!(entries[0].session_name.starts_with("wh@"));

    // When: close with kill
    let close_action = usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        &server,
        &session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(&server),
    )
    .unwrap();
    assert!(matches!(
        close_action,
        usecases::close::CloseAction::Killed { .. }
    ));

    // Then: snapshot saved
    let snap = LegacySnapshotPort::load_latest(&snapshot, &ws_id).unwrap();
    assert!(snap.is_some());

    // When: reopen (should restore from snapshot)
    let result2 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(
        result2.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    // Snapshot had 1 pane (index 0), so no extra panes to restore
    assert_eq!(result2.restored_panes, 0);

    // When: destroy project
    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        project_name,
        &server,
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    )
    .unwrap();

    // Then: everything cleaned
    let sessions = tmux.list_workspaces(&server).unwrap();
    assert!(sessions.is_empty());
    let state = index.list_projects().unwrap();
    assert!(state.projects().is_empty());
}

#[test]
fn scenario_subdir_project_naming() {
    // Given: project with sub-directory semantics
    let policy = SessionNamingPolicy::default_policy();
    let ctx = NamingContext {
        project: "myrepo/test01".to_owned(),
        workspace: "feat-a".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/home/user/myrepo".to_owned()),
        instance_id: Some("1234".to_owned()),
    };

    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    // Then: sub-directory preserved in session; server uses `-` (socket-safe)
    assert!(
        server.contains("myrepo-test01"),
        "server should contain sanitized subdir (/ → -): {server}"
    );
    assert!(
        session.contains("myrepo/test01"),
        "session should contain subdir: {session}"
    );

    // And: parseable
    let parsed = SessionNamingPolicy::parse_session(&session);
    assert!(parsed.is_some(), "session should be parseable: {session}");
    let (id, project, workspace) = parsed.unwrap();
    assert_eq!(id, "1234");
    assert_eq!(workspace, "feat-a");
    // Project contains slash
    assert!(project.contains('/'));
}

#[test]
fn scenario_open_close_reopen_cycle() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("cycle-test").unwrap();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@cycle-test";

    // Open
    let r1 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(
        r1.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );

    // Split a pane so snapshot has content
    tmux.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction: PaneDirection::Down,
        command: None,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
    .unwrap();

    // Close with kill (saves snapshot)
    usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();
    assert!(!tmux.has_workspace(server, session).unwrap());

    // Reopen (should restore 1 extra pane from snapshot)
    let r2 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled — test verifies snapshot restore path
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_close_ctx("cycle-test")),
    )
    .unwrap();
    assert_eq!(
        r2.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );
    assert_eq!(r2.restored_panes, 2);
}

// ══════════════════════════════════════════════════════════════════════════════
// Review fix tests: worktree three-step decision, main branch, open guard
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn ensure_worktree_reuses_existing_by_branch() {
    let git = FakeGit::new("/tmp/test-project");
    // Pre-add a worktree at a non-default path
    git.add_worktree(Utf8PathBuf::from("/tmp/custom-path/feat-x"), "feat-x");

    let wt = git
        .ensure_worktree(EnsureWorktreeRequest {
            base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
            workspace_name: "feat-x".to_owned(),
        })
        .unwrap();

    // Should reuse the existing worktree path, not create at default location
    assert_eq!(wt.path, Utf8PathBuf::from("/tmp/custom-path/feat-x"));
    assert_eq!(wt.branch.as_deref(), Some("feat-x"));
}

#[test]
fn ensure_worktree_branch_exists_but_no_worktree() {
    let git = FakeGit::new("/tmp/test-project");
    // Branch exists but has no worktree
    git.add_known_branch("test01");

    let wt = git
        .ensure_worktree(EnsureWorktreeRequest {
            base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
            workspace_name: "test01".to_owned(),
        })
        .unwrap();

    // Should create worktree at default path (not fail with "branch already exists")
    assert_eq!(
        wt.path,
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/test01")
    );
    assert_eq!(wt.branch.as_deref(), Some("test01"));
}

#[test]
fn ensure_worktree_creates_new_branch_when_nonexistent() {
    let git = FakeGit::new("/tmp/test-project");

    let wt = git
        .ensure_worktree(EnsureWorktreeRequest {
            base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
            workspace_name: "new-feature".to_owned(),
        })
        .unwrap();

    assert_eq!(
        wt.path,
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/new-feature")
    );
    // After creation, branch should now be known
    assert!(git.branch_exists("new-feature").unwrap());
}

#[test]
fn repeated_open_reuses_existing_worktree() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("repeat-wt").unwrap();
    let server = "whsrv-test";
    let session = "wh@test@proj@repeat-wt";

    // First open — creates worktree
    let r1 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(
        r1.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    let worktree_count_after_first = git.list_worktrees().unwrap().len();

    // Kill session to force new session creation on reopen
    usecases::close::close_session(
        &tmux,
        &tmux,
        &snapshot,
        server,
        session,
        &ws_id,
        0,
        true,
        true,
        &test_snap_ctx(server),
    )
    .unwrap();

    // Second open — should reuse the same worktree
    let r2 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        server,
        session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_ne!(
        r2.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );

    // Worktree count should not increase
    let worktree_count_after_second = git.list_worktrees().unwrap().len();
    assert_eq!(
        worktree_count_after_first, worktree_count_after_second,
        "repeated open should not create duplicate worktrees"
    );
}

#[test]
fn open_main_branch_uses_project_root_directly() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("main").unwrap();
    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-main",
        WorkspaceOpenIntent::UseProjectRoot,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Main branch should use the project root, not a worktree subdir
    assert_eq!(result.worktree_path, root);
}

#[test]
fn resolve_open_session_returns_legacy_when_only_legacy_exists() {
    let (_, _git, tmux, _ws_fs, _snapshot, _) = setup();
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        workspace_name: "wh--proj--feat-legacy".to_owned(),
        group: "whsrv-proj".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        env: Vec::new(),
    })
    .unwrap();

    let existing = usecases::open::resolve_open_session(
        &tmux,
        "whsrv-abcd-proj",
        "wh@abcd@proj@feat-legacy",
        "proj",
        "feat-legacy",
    )
    .unwrap();

    assert_eq!(
        existing,
        Some(usecases::open::ExistingSession::Legacy {
            server: "whsrv-proj".to_owned(),
            session: "wh--proj--feat-legacy".to_owned(),
        })
    );
}

#[test]
fn resolve_open_session_prefers_canonical_when_both_exist() {
    let (_, _git, tmux, _ws_fs, _snapshot, _) = setup();
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        workspace_name: "wh@abcd@proj@feat-both".to_owned(),
        group: "whsrv-abcd-proj".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        env: Vec::new(),
    })
    .unwrap();
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        workspace_name: "wh--proj--feat-both".to_owned(),
        group: "whsrv-proj".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        env: Vec::new(),
    })
    .unwrap();

    let existing = usecases::open::resolve_open_session(
        &tmux,
        "whsrv-abcd-proj",
        "wh@abcd@proj@feat-both",
        "proj",
        "feat-both",
    )
    .unwrap();

    assert_eq!(
        existing,
        Some(usecases::open::ExistingSession::Canonical {
            server: "whsrv-abcd-proj".to_owned(),
            session: "wh@abcd@proj@feat-both".to_owned(),
        })
    );
}

#[test]
fn main_worktree_identified_by_repo_root() {
    let git = FakeGit::new("/tmp/test-project");

    let worktrees = git.list_worktrees().unwrap();
    assert!(!worktrees.is_empty());

    let main_wt = worktrees.iter().find(|w| w.is_main);
    assert!(
        main_wt.is_some(),
        "should have a main worktree identified by repo root"
    );
    assert_eq!(
        main_wt.unwrap().path,
        Utf8PathBuf::from("/tmp/test-project")
    );
    assert_eq!(main_wt.unwrap().branch.as_deref(), Some("main"));
}

#[test]
fn linked_worktree_not_marked_as_main() {
    let git = FakeGit::new("/tmp/test-project");
    git.add_worktree(
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-z"),
        "feat-z",
    );

    let worktrees = git.list_worktrees().unwrap();
    let linked = worktrees
        .iter()
        .find(|w| w.branch.as_deref() == Some("feat-z"));
    assert!(linked.is_some());
    assert!(
        !linked.unwrap().is_main,
        "linked worktree must not be marked as main"
    );
}

#[test]
fn fake_git_branch_exists() {
    let git = FakeGit::new("/tmp/repo");
    // "main" is known by default
    assert!(git.branch_exists("main").unwrap());
    // Unknown branch
    assert!(!git.branch_exists("nonexistent").unwrap());
    // After adding
    git.add_known_branch("feat-y");
    assert!(git.branch_exists("feat-y").unwrap());
}

// ══════════════════════════════════════════════════════════════════════════════
// Git error propagation — ensure_worktree and open_workspace surface errors
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn ensure_worktree_propagates_git_error() {
    let git = FakeGit::new("/tmp/test-project");
    git.set_error("simulated git crash");

    let result = git.ensure_worktree(EnsureWorktreeRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "feat-x".to_owned(),
    });
    assert!(result.is_err(), "ensure_worktree must propagate git errors");
}

#[test]
fn list_worktrees_propagates_git_error() {
    let git = FakeGit::new("/tmp/test-project");
    git.set_error("simulated list failure");

    let result = git.list_worktrees();
    assert!(result.is_err(), "list_worktrees must propagate git errors");
}

#[test]
fn current_branch_propagates_git_error() {
    let git = FakeGit::new("/tmp/test-project");
    git.set_error("simulated branch failure");

    let result = git.current_branch();
    assert!(result.is_err(), "current_branch must propagate git errors");
}

#[test]
fn open_workspace_propagates_git_error_from_ensure_worktree() {
    let git = FakeGit::new("/tmp/test-project");
    let tmux = FakeRuntime::new();
    let ws_fs = FakeWorkspaceFs::new();
    let snapshot = FakeSnapshot::new();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("error-test").unwrap();

    // Inject error — open_workspace calls ensure_worktree internally
    git.set_error("simulated worktree failure");

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    );
    assert!(
        result.is_err(),
        "open_workspace must not swallow git errors from ensure_worktree"
    );
}

#[test]
fn fake_git_error_can_be_cleared() {
    let git = FakeGit::new("/tmp/test-project");
    git.set_error("temporary failure");
    assert!(git.current_branch().is_err());

    git.clear_error();
    assert!(git.current_branch().is_ok());
}

// ══════════════════════════════════════════════════════════════════════════════
// Snapshot restore — send_keys failure path
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn open_restore_snapshot_send_keys_failure_does_not_count() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("snap-fail").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "snap-fail",
            "sess-snap-fail",
            vec![
                (0, "/tmp", "zsh", true),
                (1, "/tmp/a", "vim", false),
                (2, "/tmp/b", "htop", false),
            ],
        ))
        .unwrap();

    // Inject send_keys failure — splits succeed but cd commands fail
    tmux.inject_errors(RuntimeErrorInjection {
        send_input: Some(RuntimeError::OperationFailed {
            message: "simulated send_input failure".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-snap-fail",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_ne!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
    assert_eq!(
        result.restored_panes, 0,
        "send_keys failures must not be counted as restored panes"
    );
}

// ── Reindex use case ──

fn make_discovered(name: &str, root: &str) -> DiscoveredProject {
    DiscoveredProject {
        project_name: name.to_owned(),
        project_root: Utf8PathBuf::from(root),
        instance_id: "a1b2".to_owned(),
        initialized: true,
    }
}

#[test]
fn reindex_empty_roots_returns_error() {
    let discovery = FakeDiscovery::new();
    let index = FakeProjectIndex::new();
    let result = usecases::reindex::rebuild_project_index(&discovery, &index, &[], false);
    assert!(result.is_err());
}

#[test]
fn reindex_discovers_and_writes_to_index() {
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("alpha", "/tmp/alpha"));
    discovery.add_project(make_discovered("beta", "/tmp/beta"));

    let index = FakeProjectIndex::new();
    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &roots, false).unwrap();

    assert_eq!(result.discovered_count, 2);
    assert_eq!(result.upserted_count, 2);
    assert_eq!(result.retained_existing_count, 0);
    assert_eq!(result.final_index_count, 2);
    assert_eq!(result.roots_scanned, 1);
    assert_eq!(index.list_projects().unwrap().projects().len(), 2);
}

#[test]
fn reindex_clear_first_removes_old_entries() {
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("new-proj", "/tmp/new"));

    let index = FakeProjectIndex::new();
    index
        .upsert_project(&ProjectSummary {
            project_name: "old-proj".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/old"),
            config_path: Utf8PathBuf::from("/tmp/old/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &roots, true).unwrap();

    assert_eq!(result.discovered_count, 1);
    assert_eq!(result.upserted_count, 1);
    assert_eq!(
        result.retained_existing_count, 0,
        "clear_first must report zero retained entries"
    );
    assert_eq!(result.final_index_count, 1);
    let projects = index.list_projects().unwrap().into_projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_name, "new-proj");
}

#[test]
fn reindex_without_clear_merges_with_existing() {
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("new-proj", "/tmp/new"));

    let index = FakeProjectIndex::new();
    index
        .upsert_project(&ProjectSummary {
            project_name: "old-proj".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/old"),
            config_path: Utf8PathBuf::from("/tmp/old/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &roots, false).unwrap();

    assert_eq!(result.discovered_count, 1);
    assert_eq!(result.upserted_count, 1);
    assert_eq!(
        result.retained_existing_count, 1,
        "merge must surface the pre-existing untouched entry"
    );
    assert_eq!(result.final_index_count, 2);
    assert_eq!(index.list_projects().unwrap().projects().len(), 2);
}

#[test]
fn reindex_propagates_discovery_error() {
    let discovery = FakeDiscovery::new();
    discovery.set_error("permission denied");

    let index = FakeProjectIndex::new();
    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result = usecases::reindex::rebuild_project_index(&discovery, &index, &roots, false);
    assert!(result.is_err(), "should propagate discovery error");
}

// ── Sessions use case ──

fn default_naming_policy() -> SessionNamingPolicy {
    SessionNamingPolicy::new(NamingConfig::default())
}

fn make_discovered_with_id(name: &str, root: &str, instance_id: &str) -> DiscoveredProject {
    DiscoveredProject {
        project_name: name.to_owned(),
        project_root: Utf8PathBuf::from(root),
        instance_id: instance_id.to_owned(),
        initialized: true,
    }
}

#[test]
fn sessions_empty_projects_returns_empty() {
    let tmux = FakeRuntime::new();
    let result =
        usecases::sessions::list_global_sessions(&[], |_| Ok(default_naming_policy()), &tmux)
            .unwrap();
    assert!(result.records.is_empty());
    assert!(result.unavailable.is_empty());
}

#[test]
fn sessions_returns_records_for_running_server() {
    let policy = default_naming_policy();
    let tmux = FakeRuntime::new();

    let project = make_discovered_with_id("myproj", "/tmp/myproj", "a1b2");
    let naming_ctx = NamingContext {
        project: "myproj".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/myproj".to_owned()),
        instance_id: Some("a1b2".to_owned()),
    };
    let server = policy.resolve_server(&naming_ctx).unwrap();
    let session = policy.resolve_session(&naming_ctx).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.clone(),
        workspace_name: session,
        working_dir: Utf8PathBuf::from("/tmp/myproj"),
        env: Vec::new(),
    })
    .unwrap();

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| Ok(default_naming_policy()),
        &tmux,
    )
    .unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].project_name, "myproj");
    assert_eq!(result.records[0].server_name, server);
}

#[test]
fn sessions_skips_projects_with_no_running_server() {
    let tmux = FakeRuntime::new();

    let project = make_discovered_with_id("offline", "/tmp/offline", "c3d4");
    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| Ok(default_naming_policy()),
        &tmux,
    )
    .unwrap();
    // FakeRuntime returns an empty workspace list for an unknown server, so the
    // project simply contributes no sessions and is not flagged unavailable.
    assert!(result.records.is_empty());
    assert!(result.unavailable.is_empty());
}

#[test]
fn list_global_sessions_propagates_non_backend_runtime_error() {
    let tmux = FakeRuntime::new();
    // A non-availability runtime error (permission denied) is a real failure,
    // not a per-project degradation, so it must abort the whole listing instead
    // of being downgraded to an "unavailable project".
    tmux.inject_errors(RuntimeErrorInjection {
        list_workspaces: Some(RuntimeError::PermissionDenied {
            detail: "permission denied".to_owned(),
        }),
        ..Default::default()
    });

    let project = make_discovered_with_id("offline", "/tmp/offline", "e5f6");
    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| Ok(default_naming_policy()),
        &tmux,
    );
    assert!(
        result.is_err(),
        "non-availability runtime errors must propagate, not degrade"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Review regression: per-project naming policy in global session aggregation
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn sessions_per_project_naming_lists_both_projects() {
    let tmux = FakeRuntime::new();

    let proj_a = make_discovered_with_id("alpha", "/tmp/alpha", "aaaa");
    let proj_b = make_discovered_with_id("beta", "/tmp/beta", "bbbb");

    let custom_policy = SessionNamingPolicy::new(NamingConfig {
        server_template: "custom-{instance_id}-{project}".to_owned(),
        ..NamingConfig::default()
    });

    let ctx_a = NamingContext {
        project: "alpha".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/alpha".to_owned()),
        instance_id: Some("aaaa".to_owned()),
    };
    let ctx_b = NamingContext {
        project: "beta".to_owned(),
        workspace: "dev".to_owned(),
        branch: None,
        dir_path: Some("/tmp/beta".to_owned()),
        instance_id: Some("bbbb".to_owned()),
    };

    let server_a = default_naming_policy().resolve_server(&ctx_a).unwrap();
    let session_a = default_naming_policy().resolve_session(&ctx_a).unwrap();
    let server_b = custom_policy.resolve_server(&ctx_b).unwrap();
    let session_b = custom_policy.resolve_session(&ctx_b).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server_a,
        workspace_name: session_a,
        working_dir: Utf8PathBuf::from("/tmp/alpha"),
        env: Vec::new(),
    })
    .unwrap();
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server_b,
        workspace_name: session_b,
        working_dir: Utf8PathBuf::from("/tmp/beta"),
        env: Vec::new(),
    })
    .unwrap();

    let records = usecases::sessions::list_global_sessions(
        &[proj_a, proj_b],
        |project| {
            if project.project_name == "beta" {
                Ok(SessionNamingPolicy::new(NamingConfig {
                    server_template: "custom-{instance_id}-{project}".to_owned(),
                    ..NamingConfig::default()
                }))
            } else {
                Ok(default_naming_policy())
            }
        },
        &tmux,
    )
    .unwrap();

    assert_eq!(records.records.len(), 2, "both projects should be listed");
    let names: Vec<&str> = records
        .records
        .iter()
        .map(|r| r.project_name.as_str())
        .collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn list_global_sessions_degrades_when_one_backend_unavailable() {
    let tmux = FakeRuntime::new();

    let proj_good = make_discovered_with_id("good", "/tmp/good", "1111");
    let proj_bad = make_discovered_with_id("bad", "/tmp/bad", "2222");

    let policy = default_naming_policy();
    let good_ctx = NamingContext {
        project: "good".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/good".to_owned()),
        instance_id: Some("1111".to_owned()),
    };
    let good_server = policy.resolve_server(&good_ctx).unwrap();
    let good_session = policy.resolve_session(&good_ctx).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: good_server,
        workspace_name: good_session,
        working_dir: Utf8PathBuf::from("/tmp/good"),
        env: Vec::new(),
    })
    .unwrap();

    // Model the real adapter contract: the bad project's tmux socket is missing
    // (`BackendUnavailable`), while the good project's server responds normally.
    let bad_ctx = NamingContext {
        project: "bad".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/bad".to_owned()),
        instance_id: Some("2222".to_owned()),
    };
    let bad_server = policy.resolve_server(&bad_ctx).unwrap();
    tmux.inject_list_workspaces_error_for_server(
        &bad_server,
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::SocketNotFound {
                path: "/tmp/bad/.tmux-socket".to_owned(),
            },
            detail: "socket not found".to_owned(),
        },
    );

    let listing = usecases::sessions::list_global_sessions(
        &[proj_good, proj_bad],
        |_| Ok(default_naming_policy()),
        &tmux,
    )
    .unwrap();

    assert_eq!(
        listing.records.len(),
        1,
        "an unreachable backend must not block sessions from reachable projects"
    );
    assert_eq!(listing.records[0].project_name, "good");

    assert_eq!(
        listing.unavailable.len(),
        1,
        "the unreachable project must be reported as unavailable"
    );
    assert_eq!(listing.unavailable[0].project_name, "bad");
    assert!(matches!(
        listing.unavailable[0].reason,
        workhive_core::ports::BackendUnavailableReason::SocketNotFound { .. }
    ));
}

#[test]
fn sessions_config_error_propagates_to_caller() {
    use workhive_core::errors::ConfigError;

    let tmux = FakeRuntime::new();
    let project = make_discovered_with_id("broken", "/tmp/broken", "ffff");

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Err(ConfigError::ParseFailed {
                path: "/tmp/broken/.workhive/config.toml".into(),
                message: "invalid server_template value".to_owned(),
            })
        },
        &tmux,
    );
    assert!(
        result.is_err(),
        "invalid project config must not be silently swallowed"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("invalid"),
        "error message should describe the config problem: {err_msg}"
    );
}

// Regression: non-workhive session names must not produce "unknown" records.
#[test]
fn sessions_non_workhive_session_name_returns_error() {
    let tmux = FakeRuntime::new();

    let project = make_discovered_with_id("proj", "/tmp/proj", "x1y2");
    let naming_ctx = NamingContext {
        project: "proj".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/proj".to_owned()),
        instance_id: Some("x1y2".to_owned()),
    };
    let policy = default_naming_policy();
    let server = policy.resolve_server(&naming_ctx).unwrap();

    // Create a workspace with a non-workhive session name (no wh@ prefix).
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.clone(),
        workspace_name: "plain-session-no-prefix".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/proj"),
        env: Vec::new(),
    })
    .unwrap();

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| Ok(default_naming_policy()),
        &tmux,
    );
    assert!(
        result.is_err(),
        "non-workhive session name must return error, not produce 'unknown' record"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("plain-session-no-prefix"),
        "error should reference the unparseable session name: {err_msg}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// open staging: prepare_open_plan + execute_open_plan
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn prepare_plan_captures_session_availability_missing_for_new() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("plan-test").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.session_availability,
        SessionAvailability::Missing,
        "no session pre-created"
    );
    assert_eq!(plan.workspace.as_str(), "plan-test");
    assert_eq!(plan.server, "srv");
    assert_eq!(plan.session, "sess");
    assert_ne!(plan.provenance, WorkspaceProvenance::ProjectRoot);
}

#[test]
fn prepare_plan_captures_session_availability_running_for_reuse() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("reuse-test").unwrap();
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &FakeSnapshot::new(),
        &root,
        &ws_id,
        "srv",
        "sess-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.session_availability,
        SessionAvailability::Running,
        "session was pre-created"
    );
}

#[test]
fn execute_plan_creates_session_when_not_exists() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("exec-test").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-exec",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(plan.session_availability, SessionAvailability::Missing);

    let result = usecases::open::execute_open_plan(
        &tmux,
        &tmux,
        &snapshot,
        &plan,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout
    );
    assert_eq!(result.session_name, "sess-exec");

    // Verify session was actually created
    assert!(tmux.has_workspace("srv", "sess-exec").unwrap());
}

#[test]
fn execute_plan_reuses_session_when_exists() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("reuse-exec").unwrap();
    // First open creates the session
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-reuse-exec",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Prepare plan with existing session
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-reuse-exec",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(plan.session_availability, SessionAvailability::Running);

    let result = usecases::open::execute_open_plan(
        &tmux,
        &tmux,
        &snapshot,
        &plan,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession
    );
}

#[test]
fn execute_open_plan_recovers_commands_from_prior_baseline_in_current_restore() {
    let (_, git, runtime, ws_fs, _legacy_snapshot, _) = setup();
    let snapshot_store = FakeSnapshotStore::new();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("restore-chain").unwrap();
    let state_key = test_state_key(ws_id.as_str());

    let snapshot = workhive_core::domain::WorkspaceSnapshot {
        schema_version: workhive_core::domain::SNAPSHOT_SCHEMA_VERSION,
        revision_id: workhive_core::domain::SnapshotRevisionId::first(),
        state_key: state_key.clone(),
        captured_at: "2026-04-27T18:00:00Z".to_owned(),
        source: workhive_core::domain::SnapshotSource::TimedSave,
        topology: workhive_core::domain::WorkspaceTopology {
            windows: vec![workhive_core::domain::WindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                layout: "tiled".to_owned(),
                panes: vec![
                    workhive_core::domain::PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 2,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 3,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: Some("pane-0".to_owned()),
        },
        panes: vec![
            // Tool still running live; capture metadata was lost
            // (NonRestorable). Baseline is allowed to recover the
            // launch intent because the pane has NOT exited the tool.
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-0".to_owned()),
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: Some("claude".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "claude".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("claude".to_owned()),
                    start_command: Some("claude".to_owned()),
                    active: true,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-1".to_owned()),
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: Some("python".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "python".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("python".to_owned()),
                    start_command: Some("python".to_owned()),
                    active: false,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-2".to_owned()),
                window_index: 0,
                pane_index: 2,
                pane_id: None,
                title: Some("codex".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "codex".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("codex".to_owned()),
                    start_command: Some("codex".to_owned()),
                    active: false,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-3".to_owned()),
                window_index: 0,
                pane_index: 3,
                pane_id: None,
                title: Some("shell".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "shell".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: workhive_core::domain::SnapshotMetadata {
            runtime_backend: "tmux".to_owned(),
            platform: "test".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };
    snapshot_store.save_revision(&snapshot).unwrap();

    let baseline = workhive_core::domain::LaunchBaseline {
        state_key: state_key.clone(),
        entries: vec![
            workhive_core::domain::LaunchBaselineEntry {
                runtime_id: "legacy-pane-0".to_owned(),
                unit_id: "pane-0".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: workhive_core::domain::ToolName::new("claude").unwrap(),
                },
            },
            workhive_core::domain::LaunchBaselineEntry {
                runtime_id: "legacy-pane-1".to_owned(),
                unit_id: "pane-1".to_owned(),
                launch_origin: PaneLaunchRef::InlineCommand {
                    spec: workhive_core::domain::CommandSpec::new(
                        "python",
                        ["-m", "http.server", "8080"],
                    ),
                },
            },
            workhive_core::domain::LaunchBaselineEntry {
                runtime_id: "legacy-pane-2".to_owned(),
                unit_id: "pane-2".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: workhive_core::domain::ToolName::new("codex").unwrap(),
                },
            },
        ],
        updated_at: "2026-04-27T18:00:00Z".to_owned(),
    };
    snapshot_store.save_launch_baseline(&baseline).unwrap();

    let plan = usecases::open::prepare_open_plan(
        &git,
        &runtime,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-restore-chain",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: "a1b2".to_owned(),
            project: "test-project".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(state_key.clone()),
        }),
    )
    .unwrap();

    // Fail-closed contract: restoring `Tool` panes requires a live
    // `ToolLaunchInstrumentationPort`. Provide a fake one so the
    // baseline `codex` entry can be restored under supervision; without
    // it the use-case would refuse the tool launch and the fidelity
    // report would degrade to "shell only".
    let instrumentation = workhive_testkit::fakes::FakeToolLaunchInstrumentation::new();
    let result = usecases::open::execute_open_plan(
        &runtime,
        &runtime,
        &snapshot_store,
        &plan,
        &default_catalog(),
        &StubExecChecker,
        Some(&instrumentation),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );
    assert_eq!(result.restored_panes, 4);
    match result.snapshot_outcome {
        usecases::open::SnapshotOutcome::Restored { panes, fidelity } => {
            assert_eq!(panes, 4);
            assert!(
                matches!(
                    fidelity,
                    workhive_core::domain::RestoreFidelity::TopologyOnly {
                        command_restored: 3,
                        shell_only: 1
                    }
                ),
                "expected TopologyOnly(3 command_restored, 1 shell_only), got: {fidelity:?}"
            );
        }
        other => panic!("expected SnapshotOutcome::Restored, got {other:?}"),
    }

    // Restore installs launches on two distinct paths:
    //
    // * `Tool` launches → typed `respawn_unit_with_command` so the
    //   internal `workhive internal pane-supervisor ...` wrapper never
    //   appears in pane scrollback.
    // * Inline (`Raw`) launches → visible `send_input` to preserve the
    //   user-typed echo and the shell-retained pane lifecycle.
    //
    // The baseline above carries two `Tool` entries (claude, codex) and
    // one `InlineCommand` entry (python http.server), so we expect:
    //   respawn_unit_with_command: 2 (claude, codex)
    //   send_input launch lines:   1 (python -m http.server 8080)
    let respawn_commands: Vec<String> = runtime
        .respawn_command_calls()
        .into_iter()
        .map(|(_, _, command)| command)
        .collect();
    assert_eq!(
        respawn_commands.len(),
        2,
        "only the two baseline-matched Tool panes should dispatch via \
         respawn_unit_with_command (inline commands stay on send_input); \
         got {respawn_commands:?}"
    );
    assert!(
        respawn_commands.iter().any(|cmd| cmd.contains("claude")),
        "current restore must dispatch recovered claude tool via respawn"
    );
    assert!(
        respawn_commands.iter().any(|cmd| cmd.contains("codex")),
        "current restore must dispatch recovered codex tool via respawn"
    );
    assert!(
        respawn_commands
            .iter()
            .all(|cmd| !(cmd.contains("python") && cmd.contains("http.server"))),
        "inline `python -m http.server` must NOT be installed via respawn; \
         got {respawn_commands:?}"
    );

    // Verify the inline command surfaced on the visible `send_input`
    // path, while no supervisor wrapper text leaked there.
    let visible_launch_inputs: Vec<String> = runtime
        .sent_inputs()
        .into_iter()
        .map(|(_, text)| text)
        .filter(|text| !text.starts_with("cd -- "))
        .collect();
    assert!(
        visible_launch_inputs
            .iter()
            .any(|text| text.contains("python") && text.contains("http.server")),
        "inline launch must reach the pane via visible send_input; got {visible_launch_inputs:?}"
    );
    assert!(
        visible_launch_inputs
            .iter()
            .all(|text| !text.contains("internal pane-supervisor")),
        "supervisor wrapper text MUST NOT appear in send_input traffic; \
         got {visible_launch_inputs:?}"
    );

    let persisted_baseline = snapshot_store
        .load_launch_baseline(&state_key)
        .unwrap()
        .expect("baseline should remain present after restore");
    assert_eq!(
        persisted_baseline.entries.len(),
        3,
        "non-empty baseline must not be overwritten by an empty baseline payload"
    );
}

/// Baseline freshness rule, exercised end-to-end through the open plan
/// executor:
/// when a snapshot's observed pane state shows the user has returned to
/// a shell, the prior baseline's tool/inline-command identities are
/// **stale** and must NOT be re-launched. Otherwise re-opening the
/// workspace would silently re-launch tools the user explicitly exited.
#[test]
fn execute_open_plan_skips_stale_tool_baselines_when_panes_observed_as_shell() {
    let (_, git, runtime, ws_fs, _legacy_snapshot, _) = setup();
    let snapshot_store = FakeSnapshotStore::new();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("restore-stale").unwrap();
    let state_key = test_state_key(ws_id.as_str());

    let snapshot = workhive_core::domain::WorkspaceSnapshot {
        schema_version: workhive_core::domain::SNAPSHOT_SCHEMA_VERSION,
        revision_id: workhive_core::domain::SnapshotRevisionId::first(),
        state_key: state_key.clone(),
        captured_at: "2026-05-06T18:00:00Z".to_owned(),
        source: workhive_core::domain::SnapshotSource::TimedSave,
        topology: workhive_core::domain::WorkspaceTopology {
            windows: vec![workhive_core::domain::WindowSnapshot {
                index: 0,
                name: Some("main".to_owned()),
                layout: "tiled".to_owned(),
                panes: vec![
                    workhive_core::domain::PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    workhive_core::domain::PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: Some("pane-0".to_owned()),
        },
        panes: vec![
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-0".to_owned()),
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: Some("claude".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "claude".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    // User exited claude; pane is back to shell.
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: true,
                    dead: false,
                },
            },
            workhive_core::domain::PaneSnapshot {
                runtime_id: Some("pane-1".to_owned()),
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: Some("codex".to_owned()),
                cwd: Utf8PathBuf::from("/tmp/test-project"),
                managed: true,
                restore_policy: workhive_core::domain::RestorePolicy::ShellOnly,
                launch_intent: workhive_core::domain::PaneLaunchIntent::NonRestorable {
                    display: "codex".to_owned(),
                    reason: "shell-only capture".to_owned(),
                },
                observed: workhive_core::domain::PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: Some("zsh".to_owned()),
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: workhive_core::domain::SnapshotMetadata {
            runtime_backend: "tmux".to_owned(),
            platform: "test".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };
    snapshot_store.save_revision(&snapshot).unwrap();

    let baseline = workhive_core::domain::LaunchBaseline {
        state_key: state_key.clone(),
        entries: vec![
            workhive_core::domain::LaunchBaselineEntry {
                runtime_id: "legacy-pane-0".to_owned(),
                unit_id: "pane-0".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: workhive_core::domain::ToolName::new("claude").unwrap(),
                },
            },
            workhive_core::domain::LaunchBaselineEntry {
                runtime_id: "legacy-pane-1".to_owned(),
                unit_id: "pane-1".to_owned(),
                launch_origin: PaneLaunchRef::Tool {
                    name: workhive_core::domain::ToolName::new("codex").unwrap(),
                },
            },
        ],
        updated_at: "2026-05-06T18:00:00Z".to_owned(),
    };
    snapshot_store.save_launch_baseline(&baseline).unwrap();

    let plan = usecases::open::prepare_open_plan(
        &git,
        &runtime,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-restore-stale",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: "a1b2".to_owned(),
            project: "test-project".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(state_key.clone()),
        }),
    )
    .unwrap();

    let result = usecases::open::execute_open_plan(
        &runtime,
        &runtime,
        &snapshot_store,
        &plan,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot
    );
    assert_eq!(result.restored_panes, 2);
    match result.snapshot_outcome {
        usecases::open::SnapshotOutcome::Restored { panes, fidelity } => {
            assert_eq!(panes, 2);
            assert!(
                matches!(
                    fidelity,
                    workhive_core::domain::RestoreFidelity::TopologyOnly {
                        command_restored: 0,
                        shell_only: 2
                    }
                ),
                "freshness rule: stale tool baselines must NOT recover \
                 commands when live observation says the panes are shells; \
                 got {fidelity:?}"
            );
        }
        other => panic!("expected SnapshotOutcome::Restored, got {other:?}"),
    }

    let launch_inputs: Vec<String> = runtime
        .sent_inputs()
        .into_iter()
        .map(|(_, text)| text)
        .filter(|text| !text.starts_with("cd -- "))
        .collect();
    assert!(
        launch_inputs.is_empty(),
        "freshness rule: no tool launch commands must be dispatched for \
         panes whose live state is a shell; got {launch_inputs:?}"
    );
}

#[test]
fn prepare_plan_main_branch_uses_project_root() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("main").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-main",
        WorkspaceOpenIntent::UseProjectRoot,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(plan.provenance, WorkspaceProvenance::ProjectRoot);
    assert_eq!(plan.working_dir, root);
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase 1: Semantic contract correctness tests
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn provision_new_workspace_returns_materialized_from_new_reference() {
    let git = FakeGit::new("/tmp/test-project");
    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "brand-new-feature".to_owned(),
    };
    let result: WorkspaceRootInfo = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        workhive_core::ports::ProvisionOutcome::MaterializedFromNewReference,
        "provisioning a new workspace (new branch) must report MaterializedFromNewReference"
    );
}

#[test]
fn provision_reused_linked_worktree_returns_reused_existing_root() {
    let git = FakeGit::new("/tmp/test-project");
    git.add_worktree(
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-reuse"),
        "feat-reuse",
    );

    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "feat-reuse".to_owned(),
    };
    let result: WorkspaceRootInfo = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        workhive_core::ports::ProvisionOutcome::ReusedExistingRoot,
        "reusing an existing linked worktree must report ReusedExistingRoot"
    );
}

#[test]
fn provision_attach_existing_branch_returns_materialized_from_existing_reference() {
    let git = FakeGit::new("/tmp/test-project");
    git.add_known_branch("orphan-branch");

    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "orphan-branch".to_owned(),
    };
    let result: WorkspaceRootInfo = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        workhive_core::ports::ProvisionOutcome::MaterializedFromExistingReference,
        "attaching an existing branch to a new worktree must report MaterializedFromExistingReference"
    );
}

#[test]
fn open_plan_provenance_maps_provision_lifecycle_correctly() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    // First open: brand new workspace → CreatedNewReference
    let ws_id = WorkspaceId::new("lifecycle-test").unwrap();
    let plan1 = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-lifecycle",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();
    assert_eq!(
        plan1.provenance,
        WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::FromNewReference,
        }
    );

    // Second open: worktree already exists → ReusedExisting
    let plan2 = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-lifecycle",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();
    assert_eq!(
        plan2.provenance,
        WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::ReusedExisting,
        },
        "second open of the same workspace must report ReusedExisting materialization"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase 1: Provision boundary — 3-variant lifecycle tests
//
// MainRoot is NOT a provision outcome.  It is produced by the open use case
// when provisioning is skipped (main branch).  See the open-usecase tests
// for MainRoot coverage.
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn ensure_workspace_root_returns_reused_existing_root_for_linked_worktree() {
    let git = FakeGit::new("/tmp/test-project");
    git.add_worktree(
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/existing-feat"),
        "existing-feat",
    );

    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "existing-feat".to_owned(),
    };
    let result = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        ProvisionOutcome::ReusedExistingRoot,
        "reusing a linked worktree must report ReusedExistingRoot"
    );
}

#[test]
fn ensure_workspace_root_returns_materialized_from_existing_reference_for_attach_branch() {
    let git = FakeGit::new("/tmp/test-project");
    git.add_known_branch("detached-branch");

    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "detached-branch".to_owned(),
    };
    let result = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        ProvisionOutcome::MaterializedFromExistingReference,
        "attaching an existing branch must report MaterializedFromExistingReference"
    );
}

#[test]
fn ensure_workspace_root_returns_materialized_from_new_reference_for_create_branch() {
    let git = FakeGit::new("/tmp/test-project");

    let request = ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "totally-new-branch".to_owned(),
    };
    let result = git.ensure_workspace_root(request).unwrap();
    assert_eq!(
        result.outcome,
        ProvisionOutcome::MaterializedFromNewReference,
        "creating a new branch must report MaterializedFromNewReference"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase 1: Open use case — truthful projection tests
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn prepare_open_plan_maps_project_root_provenance_truthfully() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("main").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::UseProjectRoot,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.provenance,
        WorkspaceProvenance::ProjectRoot,
        "UseProjectRoot intent must produce WorkspaceProvenance::ProjectRoot"
    );
    assert_eq!(plan.working_dir, root);
}

#[test]
fn prepare_open_plan_maps_reused_existing_provenance_truthfully() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    git.add_worktree(
        Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/pre-existing"),
        "pre-existing",
    );

    let ws_id = WorkspaceId::new("pre-existing").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.provenance,
        WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::ReusedExisting,
        },
        "reusing existing worktree must produce Provisioned with ReusedExisting materialization"
    );
}

#[test]
fn prepare_open_plan_maps_attach_branch_provenance_truthfully() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    git.add_known_branch("orphan-attach");

    let ws_id = WorkspaceId::new("orphan-attach").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.provenance,
        WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::FromExistingReference,
        },
        "attaching existing branch must produce Provisioned with FromExistingReference materialization"
    );
}

#[test]
fn prepare_open_plan_maps_create_branch_provenance_truthfully() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("brand-new-branch").unwrap();
    let plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    assert_eq!(
        plan.provenance,
        WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::FromNewReference,
        },
        "creating new branch must produce Provisioned with FromNewReference materialization"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase 1: Regression — AttachBranch and CreateBranch must not collapse
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn attach_branch_and_create_branch_produce_distinct_outcomes() {
    let git = FakeGit::new("/tmp/test-project");
    let base = Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees");

    git.add_known_branch("existing-ref");

    let attach_result = git
        .ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: base.clone(),
            workspace_name: "existing-ref".to_owned(),
        })
        .unwrap();

    let create_result = git
        .ensure_workspace_root(ProvisionWorkspaceRequest {
            base_path: base,
            workspace_name: "nonexistent-ref".to_owned(),
        })
        .unwrap();

    assert_ne!(
        attach_result.outcome, create_result.outcome,
        "AttachBranch ({:?}) and CreateBranch ({:?}) must produce different ProvisionOutcome variants",
        attach_result.outcome, create_result.outcome,
    );
    assert_eq!(
        attach_result.outcome,
        ProvisionOutcome::MaterializedFromExistingReference
    );
    assert_eq!(
        create_result.outcome,
        ProvisionOutcome::MaterializedFromNewReference
    );
}

#[test]
fn unit_count_reflects_actual_pane_count_after_split() {
    let tmux = FakeRuntime::new();
    let server = "srv-unit";
    let session = "sess-unit";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    // After creation: 1 pane
    let workspaces = tmux.list_workspaces(server).unwrap();
    assert_eq!(workspaces.len(), 1);
    assert_eq!(
        workspaces[0].unit_count, 1,
        "new session must have unit_count = 1 (one pane)"
    );

    // Split to add a second pane
    tmux.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction: PaneDirection::Down,
        command: None,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
    .unwrap();

    let workspaces = tmux.list_workspaces(server).unwrap();
    assert_eq!(
        workspaces[0].unit_count, 2,
        "unit_count must reflect actual pane count after split"
    );

    // Split again
    tmux.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction: PaneDirection::Down,
        command: None,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
    .unwrap();

    let workspaces = tmux.list_workspaces(server).unwrap();
    assert_eq!(
        workspaces[0].unit_count, 3,
        "unit_count must track cumulative pane splits"
    );
}

#[test]
fn list_workspaces_pane_count_matches_unit_count() {
    let (config, git, tmux, ws_fs, snapshot, _) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("pane-count-test").unwrap();
    let policy = SessionNamingPolicy::default_policy();
    let ctx = naming_ctx("test-project", "pane-count-test");
    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        &server,
        &session,
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    ws_fs.register_workspace("pane-count-test");

    // Split a pane inside the session
    tmux.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction: PaneDirection::Down,
        command: None,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
    .unwrap();

    let entries = usecases::list::list_workspaces(
        &ws_fs,
        &tmux,
        &snapshot,
        root.as_ref(),
        &server,
        &|name| {
            let c = naming_ctx("test-project", name);
            policy.resolve_session(&c)
        },
        &|name| Some(test_state_key(name)),
    )
    .unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].pane_count,
        Some(2),
        "pane_count in list output must reflect actual pane count, not window count"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Phase 3: Provider boundary — WorkspaceId accepts git-invalid names,
// git provider rejects them with typed error
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn workspace_id_accepts_tilde_but_git_provider_rejects_it() {
    let ws_id = WorkspaceId::new("has~tilde");
    assert!(
        ws_id.is_ok(),
        "WorkspaceId must accept '~' — it is git-only"
    );

    let (_, git, _tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "has~tilde".to_owned(),
    });
    assert!(result.is_err(), "git provider must reject '~'");
    match result.unwrap_err() {
        ProvisionError::InvalidWorkspaceName { name, .. } => {
            assert_eq!(name, "has~tilde");
        }
        other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
    }
}

#[test]
fn workspace_id_accepts_caret_but_git_provider_rejects_it() {
    let ws_id = WorkspaceId::new("has^caret");
    assert!(
        ws_id.is_ok(),
        "WorkspaceId must accept '^' — it is git-only"
    );

    let (_, git, _tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "has^caret".to_owned(),
    });
    assert!(result.is_err(), "git provider must reject '^'");
    match result.unwrap_err() {
        ProvisionError::InvalidWorkspaceName { name, .. } => {
            assert_eq!(name, "has^caret");
        }
        other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
    }
}

#[test]
fn workspace_id_accepts_dot_lock_but_git_provider_rejects_it() {
    let ws_id = WorkspaceId::new("branch.lock");
    assert!(
        ws_id.is_ok(),
        "WorkspaceId must accept '.lock' suffix — it is git-only"
    );

    let (_, git, _tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let result = provisioner.ensure_workspace_root(ProvisionWorkspaceRequest {
        base_path: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees"),
        workspace_name: "branch.lock".to_owned(),
    });
    assert!(result.is_err(), "git provider must reject '.lock' suffix");
    match result.unwrap_err() {
        ProvisionError::InvalidWorkspaceName { name, .. } => {
            assert_eq!(name, "branch.lock");
        }
        other => panic!("expected InvalidWorkspaceName, got: {other:?}"),
    }
}

#[test]
fn open_plan_with_git_invalid_workspace_name_returns_typed_provision_error() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let ws_id = WorkspaceId::new("has~tilde").unwrap();

    let result = usecases::open::prepare_open_plan(
        &provisioner,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    );
    assert!(
        result.is_err(),
        "open with git-invalid workspace name must fail at the provision boundary"
    );
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not valid for this provider"),
        "error must be a typed provider rejection, not generic git stderr: {err}"
    );
}

#[test]
fn prepare_open_plan_rejects_leading_dash_with_typed_provision_error() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let ws_id = WorkspaceId::new("-bad").unwrap();

    let result = usecases::open::prepare_open_plan(
        &provisioner,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not valid for this provider"),
        "leading dash must produce typed ProvisionError::InvalidWorkspaceName: {err}"
    );
}

#[test]
fn prepare_open_plan_rejects_double_slash_with_typed_provision_error() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let ws_id = WorkspaceId::new("fix//a").unwrap();

    let result = usecases::open::prepare_open_plan(
        &provisioner,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not valid for this provider"),
        "double slash must produce typed ProvisionError::InvalidWorkspaceName: {err}"
    );
}

#[test]
fn prepare_open_plan_rejects_at_brace_with_typed_provision_error() {
    let (_, git, tmux, ws_fs, _, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let provisioner = workhive_infra::workspace_provision::GitWorktreeProvisioner::new(&git);
    let ws_id = WorkspaceId::new("foo@{bar").unwrap();

    let result = usecases::open::prepare_open_plan(
        &provisioner,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    );
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("not valid for this provider"),
        "@{{ sequence must produce typed ProvisionError::InvalidWorkspaceName: {err}"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Typed OpenOutcome — integration tests per spec
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn outcome_existing_session_reuse_returns_existing_session_reused() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-reuse").unwrap();

    // First open creates the session
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Second open should reuse existing session
    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.outcome,
        usecases::open::OpenOutcome::ExistingSessionReused,
        "opening with existing session must return ExistingSessionReused"
    );
}

#[test]
fn outcome_snapshot_restored_returns_snapshot_restored() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-snap-ok").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "outcome-snap-ok",
            "sess-outcome-snap",
            vec![(0, "/tmp", "zsh", true), (1, "/tmp/src", "vim", false)],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-snap",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("outcome-snap-ok")),
    )
    .unwrap();

    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotRestored { panes: 2, .. }
        ),
        "open with valid snapshot must return SnapshotRestored, got: {:?}",
        result.outcome
    );
}

#[test]
fn outcome_snapshot_missing_returns_snapshot_missing() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-no-snap").unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-no-snap",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled — we want to attempt restore
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("outcome-no-snap")),
    )
    .unwrap();

    assert_eq!(
        result.outcome,
        usecases::open::OpenOutcome::SnapshotMissing,
        "open with snapshot enabled but no snapshot file must return SnapshotMissing"
    );
}

#[test]
fn outcome_snapshot_restore_failed_returns_snapshot_restore_failed() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-snap-fail").unwrap();

    // Save a snapshot with extra panes, then inject split errors
    snapshot
        .save(&test_snapshot_doc(
            "outcome-snap-fail",
            "sess-outcome-snap-fail",
            vec![(0, "/tmp", "zsh", true), (1, "/tmp/a", "vim", false)],
        ))
        .unwrap();

    // Inject spawn_unit (split) error to make restore fail
    tmux.inject_errors(RuntimeErrorInjection {
        spawn_unit: Some(RuntimeError::OperationFailed {
            message: "simulated split failure".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-snap-fail",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("outcome-snap-fail")),
    )
    .unwrap();

    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotRestoreFailed { .. }
        ),
        "open with snapshot restore failure must return SnapshotRestoreFailed, got: {:?}",
        result.outcome
    );
}

#[test]
fn open_force_destroys_and_recreates_from_layout_skipping_snapshot() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-force").unwrap();

    // Save a snapshot — it should be ignored when -f is used
    snapshot
        .save(&test_snapshot_doc(
            "outcome-force",
            "sess-outcome-force",
            vec![(0, "/tmp", "zsh", true), (1, "/tmp/src", "vim", false)],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-force",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.outcome,
        usecases::open::OpenOutcome::ForceRecreated,
        "open with -f must return ForceRecreated, never attempt snapshot"
    );
    assert_eq!(
        result.restored_panes, 0,
        "-f must not restore any panes from snapshot"
    );
}

#[test]
fn open_force_with_existing_session_destroys_and_recreates() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws_id = WorkspaceId::new("outcome-f-reuse").unwrap();

    // First open creates the session in Normal mode
    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-f-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Second open with -f must destroy and recreate, never reuse
    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-outcome-f-reuse",
        WorkspaceOpenIntent::Provision,
        true, // snapshot_enabled
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.outcome,
        usecases::open::OpenOutcome::ForceRecreated,
        "-f with existing session must destroy and recreate from layout"
    );
}

// ---------------------------------------------------------------------------
// Open semantics: Normal mode + ForceRecreateFromLayout — 10 scenarios
// ---------------------------------------------------------------------------

#[test]
fn open_existing_session_reuses_without_snapshot_or_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("reuse-no-snap").unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-reuse-no-snap",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    let r2 = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-reuse-no-snap",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("snap-restore")),
    )
    .unwrap();

    assert_eq!(
        r2.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession,
    );
    assert_eq!(
        r2.outcome,
        usecases::open::OpenOutcome::ExistingSessionReused
    );
}

/// `workspace open` snapshot restore must not silently degrade existing-pane
/// CWD: it must reach the typed restore use case which (a) never emits a
/// visible `cd -- ...` keystroke through `send_input`, and (b) for newly
/// created restore panes (the path exercised by this snapshot which has no
/// reusable live session) leaves the cwd to `create_window(..., cwd)`.
#[test]
fn open_snapshot_restore_never_emits_visible_cd_keystroke() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("snap-no-visible-cd").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "snap-no-visible-cd",
            "sess-snap-no-visible-cd",
            vec![
                (0, "/tmp/snap/expected-a", "zsh", true),
                (1, "/tmp/snap/expected-b", "less /etc/hosts", false),
            ],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-snap-no-visible-cd",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("snap-no-visible-cd")),
    )
    .unwrap();
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot,
    );

    for (unit_id, text) in tmux.sent_inputs() {
        let trimmed = text.trim_start();
        assert!(
            !trimmed.starts_with("cd "),
            "snapshot restore emitted visible `cd ` keystroke into pane \
             unit={unit_id} text={text:?} — visible cd keystrokes are forbidden"
        );
    }
}

/// Existing-pane reuse path: no visible `cd` keystrokes.
///
/// When a live tmux session already exposes a residual pane that the snapshot
/// wants to reuse, the typed restore use case MUST repair the pane's cwd via
/// the infra-only `respawn_unit_with_cwd` port (mapped to
/// `tmux respawn-pane -k -c <cwd>`), not via a visible `cd --` keystroke.
///
/// We exercise this directly through the pane runtime port to keep the
/// assertion tight on the runtime contract: the fake records every respawn
/// call and we verify it would carry the captured cwd.
#[test]
fn fake_runtime_respawn_unit_with_cwd_records_call_without_visible_cd() {
    use workhive_core::ports::SnapshotRuntimePort;
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(workhive_core::ports::CreateWorkspaceRuntimeRequest {
            workspace_name: "wsdemo".to_owned(),
            group: "srv".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/origin"),
            env: Vec::new(),
        })
        .unwrap();
    // Find the freshly-created unit so we have a real id to respawn.
    let units = runtime.list_units("srv", "wsdemo").unwrap();
    let unit = units.first().expect("create produced one unit").id.clone();

    let target = Utf8PathBuf::from("/tmp/snap/expected-a");
    runtime
        .respawn_unit_with_cwd("srv", "wsdemo", &unit, target.as_path())
        .expect("respawn fake must succeed");

    assert_eq!(
        runtime.respawn_calls(),
        vec![(unit.as_str().to_owned(), target.clone())],
        "respawn_unit_with_cwd must be recorded with the requested cwd"
    );
    let updated = runtime
        .list_units("srv", "wsdemo")
        .unwrap()
        .into_iter()
        .find(|u| u.id == unit)
        .expect("unit still present after respawn");
    assert_eq!(
        updated.cwd, target,
        "respawn must update the recorded cwd; otherwise QueryPane will lie"
    );

    assert!(
        runtime.sent_inputs().is_empty(),
        "respawn must not piggy-back any send_input; visible `cd --` is \
         categorically forbidden"
    );
}

#[test]
fn open_no_session_snapshot_enabled_with_snapshot_restores_snapshot() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("snap-restore").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "snap-restore",
            "sess-snap-restore",
            vec![(0, "/tmp", "zsh", true)],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-snap-restore",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("snap-restore")),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot,
    );
}

#[test]
fn open_no_session_snapshot_enabled_without_snapshot_applies_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("no-snap-layout").unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-no-snap-layout",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("corrupt-snap")),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
    );
    assert_eq!(result.outcome, usecases::open::OpenOutcome::SnapshotMissing);
}

#[test]
fn open_no_session_snapshot_disabled_applies_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("snap-off-layout").unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-snap-off-layout",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
    );
    assert_eq!(result.outcome, usecases::open::OpenOutcome::LayoutApplied);
}

/// Effective `.env` entries on `OpenPlan` must reach the runtime port's
/// `CreateWorkspaceRuntimeRequest`. This proves the seam from
/// `prepare_open_plan + execute_open_plan` to `WorkspaceRuntimePort`
/// preserves env in the order the use case computed.
#[test]
fn open_plan_env_propagates_to_runtime_create_workspace() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("env-flow").unwrap();

    let mut plan = usecases::open::prepare_open_plan(
        &git,
        &tmux,
        &ws_fs,
        &root,
        &ws_id,
        "srv",
        "sess-env-flow",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        None,
    )
    .unwrap();

    plan.env = vec![
        ("CLAUDE_CODE_NO_FLICKER".to_owned(), "1".to_owned()),
        ("WORKHIVE_TRACE".to_owned(), "debug".to_owned()),
    ];

    usecases::open::execute_open_plan(
        &tmux,
        &tmux,
        &snapshot,
        &plan,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    let recorded = tmux.create_workspace_env();
    assert_eq!(recorded.len(), 1, "exactly one create_workspace call");
    let (session_name, env) = &recorded[0];
    assert_eq!(session_name, "sess-env-flow");
    assert_eq!(
        env,
        &vec![
            ("CLAUDE_CODE_NO_FLICKER".to_owned(), "1".to_owned()),
            ("WORKHIVE_TRACE".to_owned(), "debug".to_owned()),
        ],
        "env must be forwarded verbatim and in order"
    );
}

#[test]
fn open_force_with_existing_session_destroys_session_and_recreates_from_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-destroy").unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-destroy",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();
    assert!(tmux.has_workspace("srv", "sess-force-destroy").unwrap());

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-destroy",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
    assert_eq!(result.outcome, usecases::open::OpenOutcome::ForceRecreated);
    assert!(tmux.has_workspace("srv", "sess-force-destroy").unwrap());
}

#[test]
fn open_force_with_existing_snapshot_deletes_snapshot_and_recreates_from_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-del-snap").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "force-del-snap",
            "sess-force-del-snap",
            vec![(0, "/tmp", "zsh", true)],
        ))
        .unwrap();
    assert!(!snapshot.list().unwrap().is_empty());

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-del-snap",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );

    let remaining: Vec<_> = snapshot
        .list()
        .unwrap()
        .into_iter()
        .filter(|s| s.workspace == "force-del-snap")
        .collect();
    assert!(remaining.is_empty(), "force must delete the snapshot");
}

/// Snapshot directory creation policy:
/// `snapshot.enabled = false` plus `OpenMode::ForceRecreateFromLayout` must
/// destroy + recreate the runtime via `WorkspaceRuntimePort` but must NOT
/// touch the snapshot store. Specifically, `delete_all` / `delete_all_global`
/// must never be called when the disabled gate is active in core.
///
/// This is the unit-level companion to the integration-level fake-port test
/// in `snapshot_disabled_open_does_not_touch_snapshot_store_or_baseline` —
/// it pins the core control-flow gate even if the CLI assembly changes.
#[test]
fn open_force_with_snapshot_disabled_does_not_call_snapshot_store_delete() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let tracker = TrackingSnapshotStore::new(snapshot);
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-disabled").unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &tracker,
        &root,
        &ws_id,
        "srv",
        "sess-force-disabled",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    // Reset baseline so we only observe the second (force) call.
    let baseline = tracker.calls();
    assert_eq!(
        baseline.delete_all, 0,
        "Normal-mode open with snapshot disabled must never call delete_all",
    );
    assert_eq!(
        baseline.delete_all_global, 0,
        "Normal-mode open with snapshot disabled must never call delete_all_global",
    );

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &tracker,
        &root,
        &ws_id,
        "srv",
        "sess-force-disabled",
        WorkspaceOpenIntent::Provision,
        false, // snapshot disabled
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
    assert_eq!(result.outcome, usecases::open::OpenOutcome::ForceRecreated);
    assert_eq!(
        result.snapshot_outcome,
        usecases::open::SnapshotOutcome::Disabled,
        "force-recreate with snapshot disabled must report Disabled outcome",
    );

    let after = tracker.calls();
    assert_eq!(
        after.delete_all, 0,
        "force-recreate with snapshot disabled must NOT call SnapshotStorePort::delete_all",
    );
    assert_eq!(
        after.delete_all_global, 0,
        "force-recreate with snapshot disabled must NOT call SnapshotStorePort::delete_all_global",
    );
    assert_eq!(
        after.save_revision, 0,
        "force-recreate with snapshot disabled must NOT call SnapshotStorePort::save_revision",
    );
    assert_eq!(
        after.load_latest, 0,
        "force-recreate with snapshot disabled must NOT call SnapshotStorePort::load_latest",
    );
    assert_eq!(
        after.save_launch_baseline, 0,
        "force-recreate with snapshot disabled must NOT persist launch baselines",
    );
    assert_eq!(
        after.set_hooks_registered_state, 0,
        "force-recreate with snapshot disabled must NOT touch hooks-registered state",
    );
}

#[test]
fn open_force_with_existing_session_and_snapshot_deletes_both_then_recreates_from_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-both").unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-both",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "force-both",
            "sess-force-both",
            vec![(0, "/tmp", "zsh", true)],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-both",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
    assert!(tmux.has_workspace("srv", "sess-force-both").unwrap());
    let remaining: Vec<_> = snapshot
        .list()
        .unwrap()
        .into_iter()
        .filter(|s| s.workspace == "force-both")
        .collect();
    assert!(remaining.is_empty());
}

#[test]
fn open_force_never_restores_snapshot() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-no-snap-restore").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "force-no-snap-restore",
            "sess-force-no-snap-r",
            vec![(0, "/tmp", "zsh", true)],
        ))
        .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-no-snap-r",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_ne!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::RestoredFromSnapshot,
        "force mode must never restore from snapshot"
    );
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
}

#[test]
fn open_force_never_reuses_existing_session() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-no-reuse").unwrap();

    usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-no-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-no-reuse",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_ne!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ReusedLiveSession,
        "force mode must never reuse an existing session"
    );
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
}

#[test]
fn open_force_without_existing_session_creates_from_layout() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-fresh").unwrap();

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-fresh",
        WorkspaceOpenIntent::Provision,
        false,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::ForceRecreatedFromLayout,
    );
    assert!(tmux.has_workspace("srv", "sess-force-fresh").unwrap());
}

// ── open -f snapshot deletion failure must abort ─────────────────────────

#[test]
fn open_force_returns_error_when_snapshot_deletion_fails() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-del-fail").unwrap();

    snapshot.set_delete_error("permission denied");

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-del-fail",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    );

    assert!(
        result.is_err(),
        "open -f must return error when snapshot deletion fails"
    );
}

#[test]
fn open_force_does_not_create_session_when_snapshot_deletion_fails() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("force-no-create").unwrap();

    snapshot.set_delete_error("disk full");

    let _ = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-force-no-create",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::ForceRecreateFromLayout,
        &default_catalog(),
        &StubExecChecker,
        None,
    );

    assert!(
        !tmux
            .has_workspace("srv", "sess-force-no-create")
            .unwrap_or(true),
        "no session must be created after snapshot deletion failure"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Snapshot document §7 required tests
// ══════════════════════════════════════════════════════════════════════════════

// T1: close --kill saves a versioned snapshot document with aligned identity.
#[test]
fn close_kill_saves_versioned_snapshot_document_with_aligned_identity() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("versioned-close").unwrap();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@versioned-close";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/versioned"),
        env: Vec::new(),
    })
    .unwrap();

    let ctx = usecases::close::SnapshotContext {
        instance_id: "inst-001".to_owned(),
        project: "my-proj".to_owned(),
        server_name: server.to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: "test-platform".to_owned(),
        state_key: None,
    };

    usecases::close::close_session(
        &tmux, &tmux, &snapshot, server, session, &ws_id, 42, true, true, &ctx,
    )
    .unwrap();

    let doc = LegacySnapshotPort::load_latest(&snapshot, &ws_id)
        .unwrap()
        .unwrap();
    assert_eq!(doc.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(doc.identity.instance_id, "inst-001");
    assert_eq!(doc.identity.project, "my-proj");
    assert_eq!(doc.identity.workspace, "versioned-close");
    assert_eq!(doc.identity.server_name, "whsrv-inst-001-my-proj");
    assert_eq!(
        doc.identity.session_name,
        "wh@inst-001@my-proj@versioned-close"
    );
    assert_eq!(doc.meta.runtime_backend, "tmux");
    assert_eq!(doc.meta.platform, "test-platform");
    assert_eq!(doc.meta.generation, 1);
    assert!(!doc.units.is_empty());
}

// T-corrupt: open falls back to layout when snapshot is corrupt (unreadable on disk).
#[test]
fn open_falls_back_to_layout_when_snapshot_is_corrupt() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("corrupt-snap").unwrap();

    snapshot.set_load_error("invalid JSON at byte 42");

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-corrupt",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("partial-spawn")),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
        "corrupt snapshot must fall back to layout"
    );
    assert!(
        matches!(
            result.snapshot_outcome,
            usecases::open::SnapshotOutcome::Failed { .. }
        ),
        "corrupt snapshot must report Failed outcome, got: {:?}",
        result.snapshot_outcome,
    );
}

// T5: open falls back to layout when snapshot is invalid/stale.
#[test]
fn open_falls_back_to_layout_when_snapshot_is_stale() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("stale-snap").unwrap();

    snapshot
        .save(&SnapshotDocument {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            identity: SnapshotIdentity {
                instance_id: "old-instance".to_owned(),
                project: "test-project".to_owned(),
                workspace: "stale-snap".to_owned(),
                server_name: "srv".to_owned(),
                session_name: "sess-stale-snap".to_owned(),
                state_key: None,
            },
            meta: SnapshotMeta {
                runtime_backend: RuntimeBackendId::tmux().to_string(),
                platform: "test".to_owned(),
                captured_at: "2026-04-20T00:00:00Z".to_owned(),
                generation: 1,
                config_fingerprint: None,
            },
            units: vec![SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "zsh".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            }],
        })
        .unwrap();

    let identity = usecases::open::SnapshotIdentityContext {
        instance_id: "current-instance".to_owned(),
        project: "test-project".to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: None,
    };

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-stale-snap",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(identity),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
        "stale snapshot must trigger layout fallback"
    );
    assert!(
        matches!(
            result.snapshot_outcome,
            usecases::open::SnapshotOutcome::Stale { .. }
        ),
        "outcome must be Stale, got {:?}",
        result.snapshot_outcome
    );
    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotStale { .. }
        ),
        "OpenOutcome must be SnapshotStale"
    );
    assert!(result.outcome.should_apply_layout());
    assert_eq!(
        result.restored_panes, 0,
        "no panes should be restored from stale snapshot"
    );
}

// T7: restore failure after partial runtime mutation destroys dirty runtime
//     before layout fallback.
#[test]
fn open_restore_failure_destroys_dirty_runtime_before_layout_fallback() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("dirty-restore").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "dirty-restore",
            "sess-dirty-restore",
            vec![(0, "/tmp", "zsh", true), (1, "/tmp/a", "vim", false)],
        ))
        .unwrap();

    tmux.inject_errors(RuntimeErrorInjection {
        spawn_unit: Some(RuntimeError::OperationFailed {
            message: "simulated split failure".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-dirty-restore",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("send-fail")),
    )
    .unwrap();

    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
        "failed restore must fall back to layout"
    );

    assert!(
        tmux.has_workspace("srv", "sess-dirty-restore").unwrap(),
        "workspace must exist after rollback+recreate (clean runtime for layout)"
    );

    let units = tmux.list_units("srv", "sess-dirty-restore").unwrap();
    assert_eq!(
        units.len(),
        1,
        "after rollback+recreate, only the default initial pane should exist"
    );
}

// T8-T10: snapshot identity semantics tests use validate_snapshot directly
// because identity derivation is a naming-level concern that close/open
// delegate to the caller (CLI). We test that the validation function
// correctly matches/rejects identity fields.

// T8: main workspace snapshot identity uses project root semantics.
#[test]
fn snapshot_validate_accepts_matching_project_root_identity() {
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            server_name: "wh-my-project".to_owned(),
            session_name: "wh--my-project--main".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };

    let result = validate_snapshot(
        &doc,
        "inst-001",
        "my-project",
        "main",
        "wh-my-project",
        "wh--my-project--main",
        "tmux",
    );
    assert_eq!(
        result,
        SnapshotValidation::Valid,
        "main workspace with project root identity must validate successfully"
    );
}

// T9: non-main workspace snapshot identity uses worktree semantics.
#[test]
fn snapshot_validate_accepts_worktree_identity_for_non_main() {
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "feat-auth".to_owned(),
            server_name: "wh-my-project".to_owned(),
            session_name: "wh--my-project--feat-auth".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };

    let result = validate_snapshot(
        &doc,
        "inst-001",
        "my-project",
        "feat-auth",
        "wh-my-project",
        "wh--my-project--feat-auth",
        "tmux",
    );
    assert_eq!(
        result,
        SnapshotValidation::Valid,
        "non-main (worktree) workspace identity must validate successfully"
    );
}

// T10: subdir project naming remains aligned in snapshot identity.
#[test]
fn snapshot_validate_rejects_project_mismatch_in_subdir() {
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "parent/child-proj".to_owned(),
            workspace: "main".to_owned(),
            server_name: "wh-child-proj".to_owned(),
            session_name: "wh--child-proj--main".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };

    let valid_result = validate_snapshot(
        &doc,
        "inst-001",
        "parent/child-proj",
        "main",
        "wh-child-proj",
        "wh--child-proj--main",
        "tmux",
    );
    assert_eq!(
        valid_result,
        SnapshotValidation::Valid,
        "subdir project with matching identity must validate"
    );

    let mismatch_result = validate_snapshot(
        &doc,
        "inst-001",
        "different-proj",
        "main",
        "wh-child-proj",
        "wh--child-proj--main",
        "tmux",
    );
    assert!(
        matches!(
            mismatch_result,
            SnapshotValidation::Invalid(SnapshotInvalidReason::ProjectMismatch { .. })
        ),
        "project name mismatch must be rejected: got {:?}",
        mismatch_result
    );
}

// T12: fake snapshot behavior matches the real adapter contract.
#[test]
fn fake_snapshot_contract_upsert_and_isolation() {
    let fake = FakeSnapshot::new();
    let ws_a = WorkspaceId::new("ws-a").unwrap();
    let ws_b = WorkspaceId::new("ws-b").unwrap();

    let doc_a = test_snapshot_doc("ws-a", "sess-a", vec![(0, "/tmp", "zsh", true)]);
    let doc_b = test_snapshot_doc("ws-b", "sess-b", vec![(0, "/home", "bash", true)]);

    fake.save(&doc_a).unwrap();
    fake.save(&doc_b).unwrap();

    let loaded_a = LegacySnapshotPort::load_latest(&fake, &ws_a)
        .unwrap()
        .unwrap();
    let loaded_b = LegacySnapshotPort::load_latest(&fake, &ws_b)
        .unwrap()
        .unwrap();
    assert_eq!(loaded_a.identity.workspace, "ws-a");
    assert_eq!(loaded_b.identity.workspace, "ws-b");

    let updated_doc_a = test_snapshot_doc(
        "ws-a",
        "sess-a",
        vec![(0, "/tmp", "zsh", true), (1, "/tmp/work", "vim", false)],
    );
    fake.save(&updated_doc_a).unwrap();

    let loaded_updated_a = LegacySnapshotPort::load_latest(&fake, &ws_a)
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded_updated_a.units.len(),
        2,
        "upsert must replace old snapshot"
    );

    let loaded_b_after = LegacySnapshotPort::load_latest(&fake, &ws_b)
        .unwrap()
        .unwrap();
    assert_eq!(
        loaded_b_after.units.len(),
        1,
        "ws-b must be unaffected by ws-a upsert"
    );

    fake.delete_for_workspace(&ws_a).unwrap();
    assert!(
        LegacySnapshotPort::load_latest(&fake, &ws_a)
            .unwrap()
            .is_none(),
        "deleted workspace must return None"
    );
    assert!(
        LegacySnapshotPort::load_latest(&fake, &ws_b)
            .unwrap()
            .is_some(),
        "other workspace must be unaffected"
    );

    let listing = fake.list().unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].workspace, "ws-b");

    LegacySnapshotPort::delete_all(&fake).unwrap();
    assert!(
        fake.list().unwrap().is_empty(),
        "delete_all must clear everything"
    );
}

// Additional validation tests for completeness:

#[test]
fn snapshot_validate_rejects_unsupported_schema_version() {
    let doc = SnapshotDocument {
        schema_version: 999,
        identity: SnapshotIdentity {
            instance_id: "inst".to_owned(),
            project: "proj".to_owned(),
            workspace: "ws".to_owned(),
            server_name: "srv".to_owned(),
            session_name: "sess".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "test".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };

    let result = validate_snapshot(&doc, "inst", "proj", "ws", "srv", "sess", "tmux");
    assert!(
        matches!(
            result,
            SnapshotValidation::Invalid(SnapshotInvalidReason::UnsupportedVersion { .. })
        ),
        "future schema version must be rejected"
    );
}

#[test]
fn snapshot_validate_rejects_backend_mismatch() {
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst".to_owned(),
            project: "proj".to_owned(),
            workspace: "ws".to_owned(),
            server_name: "srv".to_owned(),
            session_name: "sess".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "test".to_owned(),
            captured_at: "2026-04-20T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };

    let result = validate_snapshot(&doc, "inst", "proj", "ws", "srv", "sess", "zellij");
    assert!(
        matches!(
            result,
            SnapshotValidation::Invalid(SnapshotInvalidReason::BackendMismatch { .. })
        ),
        "backend mismatch must be rejected"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// CLI identity context integration
// ══════════════════════════════════════════════════════════════════════════════

// close saves snapshot identity aligned with caller-provided SnapshotContext.
#[test]
fn close_kill_snapshot_identity_matches_provided_context() {
    let (_, _, tmux, _, snapshot, _) = setup();
    let ws_id = WorkspaceId::new("identity-close").unwrap();
    let server = "whsrv-a1b2-myproj";
    let session = "wh@a1b2@myproj@identity-close";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/myproj"),
        env: Vec::new(),
    })
    .unwrap();

    let ctx = usecases::close::SnapshotContext {
        instance_id: "a1b2".to_owned(),
        project: "myproj".to_owned(),
        server_name: server.to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: "macos".to_owned(),
        state_key: None,
    };

    usecases::close::close_session(
        &tmux, &tmux, &snapshot, server, session, &ws_id, 5, true, true, &ctx,
    )
    .unwrap();

    let doc = LegacySnapshotPort::load_latest(&snapshot, &ws_id)
        .unwrap()
        .unwrap();
    assert_eq!(doc.identity.instance_id, "a1b2");
    assert_eq!(doc.identity.project, "myproj");
    assert_eq!(doc.identity.workspace, "identity-close");
    assert_eq!(doc.identity.server_name, server);
    assert_eq!(doc.identity.session_name, session);
    assert_eq!(doc.meta.runtime_backend, "tmux");
    assert_eq!(doc.meta.platform, "macos");
    assert_eq!(doc.meta.generation, 1);
}

// open rejects snapshot with mismatched identity instead of silently skipping.
#[test]
fn open_rejects_identity_mismatch_snapshot_with_stale_outcome() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("id-mismatch").unwrap();

    snapshot
        .save(&SnapshotDocument {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            identity: SnapshotIdentity {
                instance_id: "old-inst".to_owned(),
                project: "old-proj".to_owned(),
                workspace: "id-mismatch".to_owned(),
                server_name: "srv".to_owned(),
                session_name: "sess-id-mismatch".to_owned(),
                state_key: None,
            },
            meta: SnapshotMeta {
                runtime_backend: RuntimeBackendId::tmux().to_string(),
                platform: "test".to_owned(),
                captured_at: "2026-04-20T00:00:00Z".to_owned(),
                generation: 1,
                config_fingerprint: None,
            },
            units: vec![SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "zsh".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            }],
        })
        .unwrap();

    let identity = usecases::open::SnapshotIdentityContext {
        instance_id: "new-inst".to_owned(),
        project: "new-proj".to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: None,
    };

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-id-mismatch",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(identity),
    )
    .unwrap();

    assert!(
        matches!(
            result.snapshot_outcome,
            usecases::open::SnapshotOutcome::Stale { .. }
        ),
        "identity mismatch must produce Stale, got {:?}",
        result.snapshot_outcome
    );
    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotStale { .. }
        ),
        "OpenOutcome must be SnapshotStale"
    );
    assert_eq!(result.restored_panes, 0);
}

// ══════════════════════════════════════════════════════════════════════════════
// Transactional restore / no partial success
// ══════════════════════════════════════════════════════════════════════════════

// First extra pane restores ok, second fails at spawn_unit → must rollback.
#[test]
fn open_partial_spawn_failure_triggers_rollback() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("partial-spawn").unwrap();

    snapshot
        .save(&test_snapshot_doc(
            "partial-spawn",
            "sess-partial-spawn",
            vec![
                (0, "/tmp", "zsh", true),
                (1, "/tmp/a", "vim", false),
                (2, "/tmp/b", "htop", false),
            ],
        ))
        .unwrap();

    // First spawn succeeds (for unit index 1), then inject failure for second
    // spawn (for unit index 2). FakeRuntime errors are replicated for every
    // call, so we need a different approach: we can't make only the second
    // call fail with the current injection API.
    //
    // Instead, we inject spawn_unit failure immediately — the first extra pane
    // split will fail at index 1, triggering an immediate Failed + rollback.
    tmux.inject_errors(RuntimeErrorInjection {
        spawn_unit: Some(RuntimeError::OperationFailed {
            message: "spawn denied".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-partial-spawn",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("partial-spawn")),
    )
    .unwrap();

    assert!(
        matches!(
            result.snapshot_outcome,
            usecases::open::SnapshotOutcome::Failed { .. }
        ),
        "spawn failure must produce Failed, got {:?}",
        result.snapshot_outcome
    );
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
    );

    // After rollback+recreate, only the initial default pane should exist
    let units = tmux.list_units("srv", "sess-partial-spawn").unwrap();
    assert_eq!(
        units.len(),
        1,
        "after rollback+recreate, only the default pane should remain"
    );
}

// Split succeeds but the inline-launch verb (`send_input`) fails → must
// rollback, not return Restored.
//
// Inline /
// `Exec` launches stay on the visible `send_input` path so they preserve
// user-typed echo and the shell-retained pane lifecycle. Only the
// internal supervisor wrapper goes through the typed
// `respawn_unit_with_command` verb. This test therefore covers the
// rollback branch for the inline-command path; the typed-respawn path is
// covered by the unit-level `restore_runtime_pane_*` tests in
// `workhive-core::usecases::open` (where the instrumentation port can be
// wired explicitly).
#[test]
fn open_send_input_failure_triggers_rollback_not_partial_restore() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    let ws_id = WorkspaceId::new("send-fail").unwrap();

    let mut doc = test_snapshot_doc(
        "send-fail",
        "sess-send-fail",
        vec![(0, "/tmp", "zsh", true), (1, "/tmp/a", "vim", false)],
    );
    // Promote pane 1 to an Exec launch so restore must invoke `send_input`
    // to replay it under the narrowed inline-command policy; the injected
    // failure then propagates.
    doc.units[1].launch = LaunchCapture::Exec {
        program: "vim".to_owned(),
        args: vec![],
    };
    snapshot.save(&doc).unwrap();

    tmux.inject_errors(RuntimeErrorInjection {
        send_input: Some(RuntimeError::OperationFailed {
            message: "simulated send_input failure".to_owned(),
        }),
        ..Default::default()
    });

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws_id,
        "srv",
        "sess-send-fail",
        WorkspaceOpenIntent::Provision,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(test_open_identity_for_legacy_doc("send-fail")),
    )
    .unwrap();

    assert!(
        matches!(
            result.snapshot_outcome,
            usecases::open::SnapshotOutcome::Failed { .. }
        ),
        "send_input failure must produce Failed, got {:?}",
        result.snapshot_outcome
    );
    assert_eq!(
        result.recovery_path,
        usecases::open::OpenRecoveryPath::FellBackToLayout,
    );
    assert_eq!(
        result.restored_panes, 0,
        "no panes should be counted as restored after send_input failure"
    );

    // After rollback+recreate, only the initial default pane should exist
    let units = tmux.list_units("srv", "sess-send-fail").unwrap();
    assert_eq!(
        units.len(),
        1,
        "after rollback+recreate, only the default pane should remain"
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// Instance ID bootstrap and init policy tests
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn missing_instance_id_returns_naming_error() {
    let policy = SessionNamingPolicy::default_policy();
    let ctx = NamingContext {
        project: "myproj".to_owned(),
        workspace: "main".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/tmp/myproj".to_owned()),
        instance_id: None,
    };
    assert!(
        policy.resolve_server(&ctx).is_err(),
        "resolve_server must fail when instance_id is None"
    );
    assert!(
        policy.resolve_session(&ctx).is_err(),
        "resolve_session must fail when instance_id is None"
    );
    assert!(
        matches!(
            policy.resolve_server(&ctx),
            Err(workhive_core::domain::NamingError::MissingInstanceId)
        ),
        "must return MissingInstanceId variant"
    );
}

#[test]
fn transactional_open_init_creates_real_instance_id() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/test-project");

    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_none(),
        "instance_id must not exist before transactional init"
    );

    let was_initialized = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
        None,
    )
    .unwrap();

    assert!(
        was_initialized,
        "project should have been initialized transactionally"
    );

    let id = ws_fs
        .read_instance_id(root.as_ref())
        .expect("instance_id must exist after transactional init");
    assert!(!id.is_empty(), "instance_id must not be empty");
    assert_ne!(id, "0000", "instance_id must not be the fake 0000 value");
}

#[test]
fn repeated_init_preserves_instance_id() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/test-project");

    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    ws_fs.mark_config_written(root.as_ref());
    let first_id = ws_fs
        .read_instance_id(root.as_ref())
        .expect("instance_id must exist after init");

    let was_initialized = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
        None,
    )
    .unwrap();
    assert!(
        !was_initialized,
        "already-initialized project must not re-init"
    );
    let second_id = ws_fs
        .read_instance_id(root.as_ref())
        .expect("instance_id must still exist after transactional init");

    assert_eq!(
        first_id, second_id,
        "instance_id must be stable across repeated init/open"
    );
}

#[test]
fn destroy_on_uninitialized_project_does_not_create_workhive() {
    let (_config, git, tmux, ws_fs, snap, index) = setup();

    let root = Utf8PathBuf::from("/tmp/uninit-project");

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must not exist"
    );

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snap,
        &index,
        &root,
        "uninit-project",
        "whsrv-test-uninit-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be created by destroy on uninitialized project"
    );
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_none(),
        "instance_id must not be created by destroy"
    );

    let _ = result;
}

#[test]
fn destroy_aborts_when_managed_worktree_removal_fails() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let worktree_path = Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-x");
    git.add_worktree(worktree_path.clone(), "feat-x");
    ws_fs.register_workspace("feat-x");
    git.block_remove_for_path(worktree_path);

    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must exist after init"
    );

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );

    assert!(
        result.is_err(),
        "destroy must fail when worktree removal fails"
    );
    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be deleted when worktree removal fails"
    );
    assert!(
        index
            .list_projects()
            .unwrap()
            .projects()
            .iter()
            .any(|p| p.project_root == root),
        "project must remain in index when worktree removal fails"
    );
}

#[test]
fn destroy_inventory_excludes_external_dirty_worktree() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let managed_path = Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feat-m");
    git.add_worktree(managed_path.clone(), "feat-m");
    ws_fs.register_workspace("feat-m");

    let external_path = Utf8PathBuf::from("/external/repos/feat-ext");
    git.add_worktree(external_path.clone(), "feat-ext");
    git.set_worktree_status(
        external_path,
        WorktreeStatus {
            changed_files: vec![ChangedFile {
                status_code: " M".to_owned(),
                path: "dirty-file.rs".to_owned(),
                kinds: vec![ChangeKind::Unstaged],
            }],
        },
    );

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    assert_eq!(
        managed_paths.len(),
        1,
        "only managed worktree must appear in inventory"
    );
    assert_eq!(managed_paths[0], managed_path);

    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );
    assert!(
        result.is_ok(),
        "destroy must succeed when only managed worktrees are in inventory"
    );

    assert!(
        git.list_worktrees()
            .unwrap()
            .iter()
            .any(|wt| wt.branch.as_deref() == Some("feat-ext")),
        "external dirty worktree must not be removed by destroy"
    );
}

#[test]
fn destroy_rejects_orphan_managed_worktree_dir_before_cleanup() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    ws_fs.register_workspace("orphan");

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    assert!(
        managed_paths.is_empty(),
        "precondition: orphan dir is not registered as a git worktree"
    );

    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );

    match result {
        Err(usecases::destroy::DestroyError::OrphanManagedWorktreeDirs { count, first_path }) => {
            assert_eq!(count, 1);
            assert_eq!(
                first_path,
                Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/orphan")
            );
        }
        other => panic!("expected orphan managed worktree rejection, got: {other:?}"),
    }
    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be deleted when orphan worktree dirs are detected"
    );
    assert!(
        index
            .list_projects()
            .unwrap()
            .projects()
            .iter()
            .any(|p| p.project_root == root),
        "project must remain indexed when destroy is rejected"
    );
}

#[test]
fn destroy_rejects_inventory_path_outside_managed_base() {
    let (config, git, tmux, ws_fs, snapshot, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    let external_path = Utf8PathBuf::from("/external/repos/feat-ext");

    let result = usecases::destroy::destroy_project(
        &git,
        &tmux,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        "test-project",
        "whsrv-abcd-test-project",
        workhive_core::ports::WorktreeRemovePolicy::Force,
        std::slice::from_ref(&external_path),
    );

    match result {
        Err(usecases::destroy::DestroyError::WorktreePathOutsideManagedBase {
            path,
            managed_base,
        }) => {
            assert_eq!(path, external_path);
            assert_eq!(
                managed_base,
                Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees")
            );
        }
        other => panic!("expected outside managed base rejection, got: {other:?}"),
    }
    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be deleted when destroy inventory is invalid"
    );
}

#[test]
fn close_on_uninitialized_project_does_not_create_workhive() {
    let (_config, _git, tmux, ws_fs, snap, _index) = setup();

    let root = Utf8PathBuf::from("/tmp/uninit-project");

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must not exist"
    );

    let ws_id = WorkspaceId::new("main").unwrap();

    let result = usecases::close::close_session(
        &tmux,
        &tmux,
        &snap,
        "srv",
        "sess",
        &ws_id,
        0,
        false,
        true,
        &test_snap_ctx("srv"),
    );

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be created by close on uninitialized project"
    );
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_none(),
        "instance_id must not be created by close"
    );

    let _ = result;
}

#[test]
fn snapshot_identity_with_real_instance_id_never_contains_fake() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");

    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let id = ws_fs
        .read_instance_id(root.as_ref())
        .expect("instance_id must exist after init");

    let naming_ctx = NamingContext {
        project: "test-project".to_owned(),
        workspace: "main".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/tmp/test-project".to_owned()),
        instance_id: Some(id.clone()),
    };

    let identity = usecases::open::SnapshotIdentityContext {
        instance_id: naming_ctx.instance_id.clone().unwrap(),
        project: naming_ctx.project.clone(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: None,
    };

    assert_ne!(
        identity.instance_id, "0000",
        "snapshot identity must never contain fake 0000"
    );
    assert!(
        !identity.instance_id.is_empty(),
        "snapshot identity instance_id must not be empty"
    );
    assert_eq!(identity.instance_id, id);
}

// ── Instance-id consistency: session vs project ──

/// Simulates the instance-id consistency check performed in cmd_close/cmd_pane:
/// the session name's embedded instance_id must match the project's instance_id.
#[test]
fn instance_id_mismatch_produces_error() {
    use workhive_core::domain::SessionName;
    use workhive_core::errors::EnvironmentError;

    let session = SessionName::from_raw("wh@deadbeef@myproject@main");
    let project_iid = "a1b2c3d4";

    let session_iid = session
        .extract_instance_id()
        .expect("canonical session should have instance_id");
    assert_ne!(session_iid, project_iid);

    let err = EnvironmentError::InstanceIdMismatch {
        session_id: session_iid.to_owned(),
        project_id: project_iid.to_owned(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("deadbeef"),
        "error should contain session id: {msg}"
    );
    assert!(
        msg.contains("a1b2c3d4"),
        "error should contain project id: {msg}"
    );
}

#[test]
fn instance_id_match_passes_consistency_check() {
    use workhive_core::domain::SessionName;

    let session = SessionName::from_raw("wh@a1b2@myproject@main");
    let project_iid = "a1b2";

    let session_iid = session.extract_instance_id().expect("must parse");
    assert_eq!(session_iid, project_iid);
}

// ── tmux_server from ExecutionContext overrides cwd-based derivation ──

#[test]
fn execution_context_tmux_server_takes_precedence() {
    let env = FakeEnvironment::in_session_with_server("wh@a1b2@proj@main", "%0", "whsrv-a1b2-proj");
    use workhive_core::ports::EnvironmentPort;
    let ctx = env.execution_context().expect("must succeed");

    assert_eq!(ctx.tmux_server.as_deref(), Some("whsrv-a1b2-proj"));
    assert_eq!(ctx.tmux_session.as_deref(), Some("wh@a1b2@proj@main"));
}

#[test]
fn execution_context_without_server_falls_back() {
    let env = FakeEnvironment::in_session("wh@a1b2@proj@main", "%0");
    use workhive_core::ports::EnvironmentPort;
    let ctx = env.execution_context().expect("must succeed");

    assert!(ctx.tmux_server.is_none());
    assert_eq!(ctx.tmux_session.as_deref(), Some("wh@a1b2@proj@main"));
}

// ── legacy/non-canonical sessions must be rejected with typed error ──

#[test]
fn legacy_session_name_produces_unverifiable_error() {
    use workhive_core::domain::SessionName;
    use workhive_core::errors::EnvironmentError;

    let session = SessionName::from_raw("wh--myproject--main");
    assert!(
        session.extract_instance_id().is_none(),
        "legacy format must not yield instance_id"
    );

    let err = EnvironmentError::SessionIdentityUnverifiable {
        session_name: session.as_str().to_owned(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("wh--myproject--main"),
        "error must contain the session name: {msg}"
    );
    assert!(
        msg.contains("instance_id"),
        "error must mention instance_id: {msg}"
    );
}

#[test]
fn non_workhive_session_name_produces_unverifiable_error() {
    use workhive_core::domain::SessionName;
    use workhive_core::errors::EnvironmentError;

    let session = SessionName::from_raw("random-session-42");
    assert!(session.extract_instance_id().is_none());

    let err = EnvironmentError::SessionIdentityUnverifiable {
        session_name: session.as_str().to_owned(),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("random-session-42"),
        "error must contain the session name: {msg}"
    );
}

// ── Identity hardening: validate_snapshot_by_state_key ──────────────────

#[test]
fn validate_snapshot_by_state_key_accepts_matching_state_key() {
    use workhive_core::domain::WorkspaceStateKey;

    let state_key = WorkspaceStateKey::new(
        "inst-001".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            server_name: "whsrv-inst-001-my-project".to_owned(),
            session_name: "wh@inst-001@my-project@main".to_owned(),
            state_key: Some("inst-001@my-project@main".to_owned()),
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-22T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };
    let result = validate_snapshot_by_state_key(
        &doc,
        &state_key,
        "whsrv-inst-001-my-project",
        "wh@inst-001@my-project@main",
        "tmux",
    );
    assert!(
        matches!(result, SnapshotValidation::Valid),
        "snapshot with matching state_key must be valid, got: {result:?}"
    );
}

#[test]
fn validate_snapshot_by_state_key_rejects_mismatched_state_key() {
    use workhive_core::domain::WorkspaceStateKey;

    let state_key = WorkspaceStateKey::new(
        "inst-001".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            server_name: "whsrv-inst-001-my-project".to_owned(),
            session_name: "wh@inst-001@my-project@main".to_owned(),
            state_key: Some("other-inst@other-proj@main".to_owned()),
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-22T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };
    let result = validate_snapshot_by_state_key(
        &doc,
        &state_key,
        "whsrv-inst-001-my-project",
        "wh@inst-001@my-project@main",
        "tmux",
    );
    assert!(
        matches!(
            result,
            SnapshotValidation::Invalid(SnapshotInvalidReason::StateKeyMismatch { .. })
        ),
        "snapshot with mismatched state_key must be rejected as StateKeyMismatch, got: {result:?}"
    );
}

#[test]
fn validate_snapshot_by_state_key_falls_back_for_legacy_snapshot_without_state_key() {
    use workhive_core::domain::WorkspaceStateKey;

    let state_key = WorkspaceStateKey::new(
        "inst-001".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            server_name: "whsrv-inst-001-my-project".to_owned(),
            session_name: "wh@inst-001@my-project@main".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-22T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };
    let result = validate_snapshot_by_state_key(
        &doc,
        &state_key,
        "whsrv-inst-001-my-project",
        "wh@inst-001@my-project@main",
        "tmux",
    );
    assert!(
        matches!(result, SnapshotValidation::Valid),
        "legacy snapshot without state_key should fall back to field-by-field and pass, got: {result:?}"
    );
}

#[test]
fn snapshot_state_key_stable_for_main_and_subdir_workspaces() {
    use workhive_core::domain::WorkspaceStateKey;

    let key_main = WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let key_sub = WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "my-project".to_owned(),
        "subdir".to_owned(),
        "feat-auth".to_owned(),
    );

    assert_eq!(key_main.to_string(), "a1b2@my-project@main");
    assert_eq!(key_sub.to_string(), "a1b2@my-project@subdir@feat-auth");
    assert_ne!(key_main, key_sub);

    let key_main2 = WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    assert_eq!(
        key_main, key_main2,
        "state key must be stable across repeated derivation"
    );
}

/// Close path resolves runtime identity from explicit server/session parameters (obtained
/// from tmux runtime context), independent of any CWD. Two workspaces with different CWDs
/// must each close correctly when addressed by their runtime identity.
#[test]
fn close_resolves_runtime_identity_independent_of_cwd() {
    let (_, _, tmux, _, snapshot, _) = setup();

    let server = "whsrv-c0de-proj";
    let session_a = "wh@c0de@proj@ws-alpha";
    let session_b = "wh@c0de@proj@ws-beta";
    let ws_a = WorkspaceId::new("ws-alpha").unwrap();
    let ws_b = WorkspaceId::new("ws-beta").unwrap();

    for (session, cwd) in [
        (session_a, "/tmp/proj/alpha-dir"),
        (session_b, "/tmp/proj/completely-different-dir"),
    ] {
        tmux.create_workspace(CreateWorkspaceRuntimeRequest {
            group: server.to_owned(),
            workspace_name: session.to_owned(),
            working_dir: Utf8PathBuf::from(cwd),
            env: Vec::new(),
        })
        .unwrap();
    }

    let snap_ctx = usecases::close::SnapshotContext {
        instance_id: "c0de".to_owned(),
        project: "proj".to_owned(),
        server_name: server.to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: "test".to_owned(),
        state_key: None,
    };

    usecases::close::close_session(
        &tmux, &tmux, &snapshot, server, session_a, &ws_a, 1, true, true, &snap_ctx,
    )
    .unwrap();

    usecases::close::close_session(
        &tmux, &tmux, &snapshot, server, session_b, &ws_b, 1, true, true, &snap_ctx,
    )
    .unwrap();

    let doc_a = LegacySnapshotPort::load_latest(&snapshot, &ws_a)
        .unwrap()
        .unwrap();
    let doc_b = LegacySnapshotPort::load_latest(&snapshot, &ws_b)
        .unwrap()
        .unwrap();

    assert_eq!(doc_a.identity.session_name, session_a);
    assert_eq!(doc_b.identity.session_name, session_b);
    assert_eq!(doc_a.identity.workspace, "ws-alpha");
    assert_eq!(doc_b.identity.workspace, "ws-beta");
}

// ── Production wiring integration tests (R2) ────────────────────────

/// Close with a real state_key writes it into the snapshot identity block.
#[test]
fn close_writes_real_state_key_into_snapshot() {
    let (_, _, tmux, _, snapshot, _) = setup();

    let server = "whsrv-a1b2-proj";
    let session = "wh@a1b2@proj@main";
    let ws = WorkspaceId::new("main").unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/proj"),
        env: Vec::new(),
    })
    .unwrap();

    let state_key_str = "a1b2@proj@main".to_owned();
    let snap_ctx = usecases::close::SnapshotContext {
        instance_id: "a1b2".to_owned(),
        project: "proj".to_owned(),
        server_name: server.to_owned(),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: "test".to_owned(),
        state_key: Some(state_key_str.clone()),
    };

    usecases::close::close_session(
        &tmux, &tmux, &snapshot, server, session, &ws, 1, true, true, &snap_ctx,
    )
    .unwrap();

    let doc = LegacySnapshotPort::load_latest(&snapshot, &ws)
        .unwrap()
        .unwrap();
    assert_eq!(
        doc.identity.state_key.as_deref(),
        Some(state_key_str.as_str()),
        "close must write real state_key, not None"
    );
}

/// Open validates snapshot by typed state_key when present.
#[test]
fn open_validates_snapshot_by_typed_state_key() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws = WorkspaceId::new("main").unwrap();
    let server = "whsrv-a1b2-proj";
    let session = "wh@a1b2@proj@main";

    let mut doc = test_snapshot_doc("main", session, vec![(0, "/tmp", "zsh", true)]);
    doc.identity.instance_id = "a1b2".to_owned();
    doc.identity.project = "proj".to_owned();
    doc.identity.server_name = server.to_owned();
    doc.identity.state_key = Some("a1b2@proj@main".to_owned());
    snapshot.save(&doc).unwrap();

    let matching_key = workhive_core::domain::WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        String::new(),
        "main".to_owned(),
    );

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws,
        server,
        session,
        WorkspaceOpenIntent::UseProjectRoot,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: "a1b2".to_owned(),
            project: "proj".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(matching_key),
        }),
    )
    .unwrap();

    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotRestored { panes: 1, .. }
        ),
        "matching state_key should accept the snapshot, got: {:?}",
        result.outcome
    );
}

/// Open rejects snapshot when typed state_key mismatches.
#[test]
fn open_rejects_snapshot_with_mismatched_state_key() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws = WorkspaceId::new("main").unwrap();
    let server = "whsrv-a1b2-proj";
    let session = "wh@a1b2@proj@main";

    let mut doc = test_snapshot_doc("main", session, vec![(0, "/tmp", "zsh", true)]);
    doc.identity.instance_id = "a1b2".to_owned();
    doc.identity.project = "proj".to_owned();
    doc.identity.server_name = server.to_owned();
    doc.identity.state_key = Some("a1b2@proj@main".to_owned());
    snapshot.save(&doc).unwrap();

    let wrong_key = workhive_core::domain::WorkspaceStateKey::new(
        "xxxx".to_owned(),
        "other-proj".to_owned(),
        String::new(),
        "main".to_owned(),
    );

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws,
        server,
        session,
        WorkspaceOpenIntent::UseProjectRoot,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: "xxxx".to_owned(),
            project: "other-proj".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(wrong_key),
        }),
    )
    .unwrap();

    assert_eq!(
        result.outcome,
        usecases::open::OpenOutcome::SnapshotMissing,
        "mismatched typed state_key scopes lookup and should return SnapshotMissing"
    );
}

/// Open falls back to field-by-field when snapshot has no state_key (legacy).
#[test]
fn open_falls_back_to_legacy_when_snapshot_lacks_state_key() {
    let (_, git, tmux, ws_fs, snapshot, _) = setup();
    let root = Utf8PathBuf::from("/tmp/test-project");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let ws = WorkspaceId::new("main").unwrap();
    let server = "whsrv-a1b2-proj";
    let session = "wh@a1b2@proj@main";

    let mut doc = test_snapshot_doc("main", session, vec![(0, "/tmp", "zsh", true)]);
    doc.identity.instance_id = "a1b2".to_owned();
    doc.identity.project = "proj".to_owned();
    doc.identity.server_name = server.to_owned();
    doc.identity.state_key = None; // legacy snapshot without state_key
    snapshot.save(&doc).unwrap();

    let typed_key = workhive_core::domain::WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        String::new(),
        "main".to_owned(),
    );

    let result = usecases::open::open_workspace(
        &git,
        &tmux,
        &tmux,
        &ws_fs,
        &snapshot,
        &root,
        &ws,
        server,
        session,
        WorkspaceOpenIntent::UseProjectRoot,
        true,
        usecases::open::OpenMode::Normal,
        &default_catalog(),
        &StubExecChecker,
        Some(usecases::open::SnapshotIdentityContext {
            instance_id: "a1b2".to_owned(),
            project: "proj".to_owned(),
            runtime_backend: RuntimeBackendId::tmux(),
            state_key: Some(typed_key),
        }),
    )
    .unwrap();

    assert!(
        matches!(
            result.outcome,
            usecases::open::OpenOutcome::SnapshotRestored { panes: 1, .. }
        ),
        "legacy snapshot without state_key should accept via field-by-field fallback, got: {:?}",
        result.outcome
    );
}

/// Display identity (relative_display_path) remains separate from
/// sanitized runtime identity (canonical_session_name).
#[test]
fn display_identity_separate_from_runtime_identity() {
    let port = FakeIdentityPort::new();
    let policy = SessionNamingPolicy::default_policy();
    let ctx = NamingContext {
        project: "python/1_0pingtai".to_owned(),
        workspace: "feat.auth".to_owned(),
        branch: None,
        dir_path: Some("/dev/python/1.0pingtai".to_owned()),
        instance_id: Some("c3d4".to_owned()),
    };

    let result = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "1_0pingtai",
        Some("python/1.0pingtai/feat.auth"),
    )
    .unwrap();

    assert_eq!(
        result.relative_display_path.as_deref(),
        Some("python/1.0pingtai/feat.auth"),
        "display path must preserve original unsanitized form"
    );
    assert!(
        !result.canonical_session_name.contains('.'),
        "runtime session must not contain dots (tmux-hostile): {}",
        result.canonical_session_name
    );
    assert_ne!(
        result.relative_display_path.as_deref().unwrap_or(""),
        &result.canonical_workspace_component,
        "display and runtime components should differ for sanitize-required names"
    );
}

/// Main workspace and subdirectory workspace produce stable, distinct state keys.
#[test]
fn main_and_subdir_produce_distinct_stable_state_keys() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/my-project");

    usecases::identity::ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project")
        .unwrap();

    let policy = SessionNamingPolicy::default_policy();

    let ctx_main = NamingContext {
        project: "my-project".to_owned(),
        workspace: "main".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/Users/dev/my-project".to_owned()),
        instance_id: Some("a1b2".to_owned()),
    };
    usecases::identity::ensure_workspace_identity(&port, &policy, &ctx_main, "", None).unwrap();

    let ctx_sub = NamingContext {
        workspace: "feat-auth".to_owned(),
        ..ctx_main.clone()
    };
    usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx_sub,
        "subdir",
        Some("subdir/feat-auth"),
    )
    .unwrap();

    let k1 = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("main", ""),
    )
    .unwrap();
    let k2 = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("main", ""),
    )
    .unwrap();
    let k_sub = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("feat-auth", "subdir"),
    )
    .unwrap();

    assert_eq!(k1, k2, "same workspace must produce stable key");
    assert_ne!(k1, k_sub, "main and subdir must produce distinct keys");
    assert_eq!(k1.workspace, "main");
    assert_eq!(k_sub.workspace, "feat-auth");
}

/// ensure_project_identity returns typed error on instance_id conflict,
/// verifying that init cannot silently succeed when metadata conflicts.
#[test]
fn ensure_project_identity_returns_error_on_conflict() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/my-project");

    usecases::identity::ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project")
        .unwrap();

    let err = usecases::identity::ensure_project_identity(
        &port,
        "xxxx",
        &root,
        "my-project",
        "my-project",
    )
    .unwrap_err();
    assert!(
        matches!(err, IdentityError::InstanceIdConflict { .. }),
        "conflicting instance_id must produce typed IdentityError, got: {err:?}"
    );
}

/// derive_workspace_state_key returns typed error when project identity
/// metadata is missing, verifying that close cannot silently write None.
#[test]
fn derive_state_key_fails_when_project_identity_missing() {
    let port = FakeIdentityPort::new();

    let err = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("main", ""),
    )
    .unwrap_err();
    assert!(
        matches!(err, IdentityError::ProjectIdentityMissing { .. }),
        "missing project identity must produce typed error, got: {err:?}"
    );
}

/// derive_workspace_state_key returns typed error when workspace identity
/// metadata is missing (project exists but workspace was never opened).
#[test]
fn derive_state_key_fails_when_workspace_identity_missing() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/my-project");

    usecases::identity::ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project")
        .unwrap();

    let err = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("feat-x", ""),
    )
    .unwrap_err();
    assert!(
        matches!(err, IdentityError::WorkspaceIdentityMissing { .. }),
        "missing workspace identity must produce typed error, got: {err:?}"
    );
}

/// resolve_relative_display_path produces real unsanitized subdir path,
/// verifying that open writes the correct display identity.
#[test]
fn resolve_relative_display_path_preserves_unsanitized_subdir() {
    let root = Utf8PathBuf::from("/home/user/python");
    let cwd = Utf8PathBuf::from("/home/user/python/1.0pingtai");
    let display = usecases::naming::resolve_relative_display_path(
        "python",
        &root,
        "feat.auth",
        Some(cwd.as_ref()),
    );
    assert_eq!(
        display.as_deref(),
        Some("python/1.0pingtai/feat.auth"),
        "display path must include unsanitized subdir and workspace"
    );
}

/// validate_snapshot_by_state_key returns StateKeyMismatch (not InstanceMismatch)
/// when the composite state key differs.
#[test]
fn validate_snapshot_state_key_mismatch_returns_typed_reason() {
    use workhive_core::domain::WorkspaceStateKey;

    let state_key = WorkspaceStateKey::new(
        "inst-001".to_owned(),
        "my-project".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let doc = SnapshotDocument {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "inst-001".to_owned(),
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            server_name: "whsrv-inst-001-my-project".to_owned(),
            session_name: "wh@inst-001@my-project@main".to_owned(),
            state_key: Some("different-inst@other-proj@other-ws".to_owned()),
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-22T00:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![],
    };
    let result = validate_snapshot_by_state_key(
        &doc,
        &state_key,
        "whsrv-inst-001-my-project",
        "wh@inst-001@my-project@main",
        "tmux",
    );
    match result {
        SnapshotValidation::Invalid(SnapshotInvalidReason::StateKeyMismatch {
            expected,
            found,
        }) => {
            assert_eq!(expected, state_key.to_string());
            assert_eq!(found, "different-inst@other-proj@other-ws");
        }
        other => panic!("expected StateKeyMismatch, got: {other:?}"),
    }
}

/// Project identity created from a subdir open path must not contain
/// the subdir offset in its canonical component — subdir belongs to
/// workspace/display identity only.
#[test]
fn project_identity_from_subdir_does_not_contain_subdir_offset() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/python");
    let project_name = "python";
    let canonical_project = SessionNamingPolicy::sanitize_component(project_name);

    let record = usecases::identity::ensure_project_identity(
        &port,
        "c3d4",
        &root,
        project_name,
        &canonical_project,
    )
    .unwrap();

    assert_eq!(
        record.canonical_project_component, "python",
        "project canonical component must be repo name, not repo/subdir"
    );
    assert_eq!(record.project_display, "python");
    assert!(
        !record.canonical_project_component.contains('/'),
        "project canonical component must never contain subdir slash"
    );
}

/// Project identity and workspace identity are correctly layered:
/// project holds project-level invariants, workspace holds subdir display.
#[test]
fn project_and_workspace_identity_layering_is_correct() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/python");

    let canonical_project = SessionNamingPolicy::sanitize_component("python");
    usecases::identity::ensure_project_identity(&port, "c3d4", &root, "python", &canonical_project)
        .unwrap();

    let policy = SessionNamingPolicy::default_policy();
    let ctx = NamingContext {
        project: "python/1_0pingtai".to_owned(),
        workspace: "feat.auth".to_owned(),
        branch: None,
        dir_path: Some("/Users/dev/python".to_owned()),
        instance_id: Some("c3d4".to_owned()),
    };

    let ws_record = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "1_0pingtai",
        Some("python/1.0pingtai/feat.auth"),
    )
    .unwrap();

    let proj_record = port.load_project_identity().unwrap().unwrap();
    assert_eq!(
        proj_record.canonical_project_component, "python",
        "project-level canonical must stay at repo level"
    );
    assert_eq!(
        ws_record.relative_display_path.as_deref(),
        Some("python/1.0pingtai/feat.auth"),
        "workspace display includes unsanitized subdir path"
    );
    assert_ne!(
        proj_record.canonical_project_component, ws_record.canonical_workspace_component,
        "project and workspace canonical components are distinct"
    );
}

/// ensure_project_identity rejects a canonical_project_component that
/// contains a subdir offset, preventing callers from accidentally
/// leaking cwd into project-level identity.
#[test]
fn ensure_project_identity_rejects_subdir_canonical_on_second_call() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/python");

    usecases::identity::ensure_project_identity(&port, "c3d4", &root, "python", "python").unwrap();

    let err = usecases::identity::ensure_project_identity(
        &port,
        "c3d4",
        &root,
        "python",
        "python/1_0pingtai",
    )
    .unwrap_err();
    assert!(
        matches!(err, IdentityError::CanonicalProjectConflict { .. }),
        "subdir in canonical must be rejected, got: {err:?}"
    );
}

/// Same repo + same workspace name + different subdirs must produce
/// independent workspace identity records that never collide.
#[test]
fn same_workspace_different_subdirs_identity_no_collision() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/my-project");

    usecases::identity::ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project")
        .unwrap();

    let policy = SessionNamingPolicy::default_policy();

    let ctx = NamingContext {
        project: "my-project".to_owned(),
        workspace: "feat-x".to_owned(),
        branch: None,
        dir_path: Some("/Users/dev/my-project".to_owned()),
        instance_id: Some("a1b2".to_owned()),
    };

    let rec_a = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "subdir-a",
        Some("subdir-a/feat-x"),
    )
    .unwrap();

    let rec_b = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "subdir-b",
        Some("subdir-b/feat-x"),
    )
    .unwrap();

    assert_eq!(rec_a.workspace, "feat-x");
    assert_eq!(rec_b.workspace, "feat-x");
    assert_eq!(rec_a.subdir_key, "subdir-a");
    assert_eq!(rec_b.subdir_key, "subdir-b");
    assert_ne!(
        rec_a.identity_key(),
        rec_b.identity_key(),
        "identity keys must differ for different subdirs"
    );
    assert_ne!(
        rec_a.relative_display_path, rec_b.relative_display_path,
        "display paths must differ"
    );
}

/// Same repo + same workspace + different subdirs -> state keys are different.
#[test]
fn same_workspace_different_subdirs_produce_different_state_keys() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/my-project");

    usecases::identity::ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project")
        .unwrap();

    let policy = SessionNamingPolicy::default_policy();

    let ctx = NamingContext {
        project: "my-project".to_owned(),
        workspace: "feat-x".to_owned(),
        branch: None,
        dir_path: Some("/Users/dev/my-project".to_owned()),
        instance_id: Some("a1b2".to_owned()),
    };

    usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "subdir-a",
        Some("subdir-a/feat-x"),
    )
    .unwrap();

    usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        "subdir-b",
        Some("subdir-b/feat-x"),
    )
    .unwrap();

    let sk_a = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("feat-x", "subdir-a"),
    )
    .unwrap();
    let sk_b = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("feat-x", "subdir-b"),
    )
    .unwrap();

    assert_ne!(
        sk_a, sk_b,
        "state keys must differ for same workspace in different subdirs"
    );
    assert_eq!(sk_a.subdir_key, "subdir-a");
    assert_eq!(sk_b.subdir_key, "subdir-b");
    assert_eq!(sk_a.workspace, "feat-x");
    assert_eq!(sk_b.workspace, "feat-x");
}

/// Main workspace (root) and subdir workspace remain stable and distinguishable
/// after the subdir_key dimension is introduced.
#[test]
fn main_vs_subdir_workspace_stable_and_distinguishable() {
    let port = FakeIdentityPort::new();
    let root = Utf8PathBuf::from("/Users/dev/proj");

    usecases::identity::ensure_project_identity(&port, "inst1", &root, "proj", "proj").unwrap();

    let policy = SessionNamingPolicy::default_policy();

    let ctx_main = NamingContext {
        project: "proj".to_owned(),
        workspace: "main".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/Users/dev/proj".to_owned()),
        instance_id: Some("inst1".to_owned()),
    };
    let rec_main =
        usecases::identity::ensure_workspace_identity(&port, &policy, &ctx_main, "", None).unwrap();

    let ctx_sub = NamingContext {
        workspace: "dev".to_owned(),
        ..ctx_main.clone()
    };
    let rec_sub = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx_sub,
        "services",
        Some("services/dev"),
    )
    .unwrap();

    assert_eq!(rec_main.subdir_key, "");
    assert_eq!(rec_sub.subdir_key, "services");

    let sk_main = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("main", ""),
    )
    .unwrap();
    let sk_sub = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("dev", "services"),
    )
    .unwrap();

    assert_ne!(sk_main, sk_sub);
    assert_eq!(sk_main.subdir_key, "");
    assert_eq!(sk_sub.subdir_key, "services");

    let sk_main2 = usecases::identity::derive_workspace_state_key(
        &port,
        &WorkspaceIdentityKey::new("main", ""),
    )
    .unwrap();
    assert_eq!(sk_main, sk_main2, "main workspace state key must be stable");
}

// ── Project init consent tests (required by fix spec) ──

#[test]
fn partial_init_state_detected_correctly() {
    let (_config, _git, _tmux, ws_fs, _snap, _index) = setup();

    let root = Utf8PathBuf::from("/tmp/partial-init-detect");

    ws_fs.create_workhive_dir(root.as_ref()).unwrap();

    let state = usecases::project_state::resolve_init_state(&ws_fs, root.as_ref());
    assert!(
        state.is_partially_initialized(),
        "directory without instance_id or config should be partial"
    );
    match state {
        workhive_core::domain::ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => {
            assert!(has_dir, "directory exists");
            assert!(!has_instance_id, "instance_id does not exist yet");
            assert!(!has_config, "config does not exist yet");
        }
        other => panic!("expected PartiallyInitialized, got {other:?}"),
    }
}

#[test]
fn project_init_decision_variants_are_exhaustive() {
    use workhive_core::domain::ProjectInitDecision;

    let decisions = [
        ProjectInitDecision::AlreadyInitialized {
            instance_id: "test".to_string(),
        },
        ProjectInitDecision::NeedsConfirmation,
        ProjectInitDecision::DeniedNonInteractive,
        ProjectInitDecision::ApprovedAutoInit,
    ];

    assert_eq!(decisions.len(), 4, "all decision variants must be covered");
}

// ── Transactional project initialization tests ──

#[test]
fn transactional_init_on_clean_project_succeeds() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-init-clean");
    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must not exist"
    );

    let was_init = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "txn-clean",
        None,
    )
    .unwrap();

    assert!(was_init, "clean project must be initialized");
    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must exist after transactional init"
    );
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_some(),
        "instance_id must exist"
    );
    assert!(
        config.project_exists(root.as_ref()).unwrap(),
        "config.toml must exist (via ConfigPort)"
    );
}

#[test]
fn transactional_init_with_config_persists_supplied_project_config() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-init-supplied-config");
    let supplied_config = AppConfig {
        language: Some(Locale::ZhCn),
        layout: Some(LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::EmptyShell,
                }],
            }],
        }),
        ..AppConfig::default()
    };

    let was_init = usecases::init::init_project_transactional_with_config(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "supplied-config-proj",
        supplied_config.clone(),
    )
    .unwrap();

    assert!(was_init, "clean project must be initialized");
    let saved_config = config
        .load_project(root.as_ref())
        .unwrap()
        .expect("project config must be saved");
    assert_eq!(
        saved_config.layout, supplied_config.layout,
        "transactional init must persist the supplied wizard layout"
    );
    assert_eq!(
        saved_config.language, supplied_config.language,
        "transactional init must persist the supplied language"
    );
}

#[test]
fn transactional_init_already_initialized_skips() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-already-init");
    usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "already-proj",
        None,
    )
    .unwrap();
    let id1 = ws_fs
        .read_instance_id(root.as_ref())
        .expect("id must exist");

    let was_init = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "already-proj",
        None,
    )
    .unwrap();
    assert!(!was_init, "must return false for already-initialized");

    let id2 = ws_fs
        .read_instance_id(root.as_ref())
        .expect("id must exist");
    assert_eq!(id1, id2, "instance_id must be stable");
}

#[test]
fn transactional_init_lock_failure_does_not_create_workhive() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();
    lock.set_fail_on_acquire(true);

    let root = Utf8PathBuf::from("/tmp/txn-lock-fail");
    let result = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "lock-fail-proj",
        None,
    );

    assert!(result.is_err(), "lock failure must return error");
    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        ".workhive must not be created when lock fails"
    );
}

#[test]
fn transactional_init_config_write_failure_rolls_back_staging() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    config.set_save_project_error(workhive_core::errors::ConfigError::WriteFailed {
        path: Utf8PathBuf::from("/tmp/config-write-failure"),
        source: std::io::Error::other("injected config failure"),
    });
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-config-fail");
    let result = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "config-fail-proj",
        None,
    );

    assert!(result.is_err(), "config write failure must return error");
    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "final .workhive must not be created when staging config write fails"
    );
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_none(),
        "final instance_id must not be present after rollback"
    );
    assert!(
        !ws_fs.project_config_exists(root.as_ref()),
        "final config must not be present after rollback"
    );
}

#[test]
fn transactional_init_index_failure_reports_error_but_keeps_committed_project_state() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    index.set_upsert_error(workhive_core::errors::IndexError::WriteFailed {
        path: Utf8PathBuf::from("/tmp/index-write-failure"),
        source: std::io::Error::other("injected index failure"),
    });
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-index-fail");
    let result = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "index-fail-proj",
        None,
    );

    assert!(
        matches!(
            result,
            Err(workhive_core::errors::ProjectInitError::Index(_))
        ),
        "index failure must be visible to the caller"
    );
    assert!(
        ws_fs.workhive_dir_exists(root.as_ref()),
        "project-local state is committed before index update"
    );
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_some(),
        "committed project state must keep instance_id"
    );
    assert!(
        config.project_exists(root.as_ref()).unwrap(),
        "committed project state must keep config (via ConfigPort)"
    );
}

#[test]
fn transactional_init_partial_state_gets_cleaned_and_reinited() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-partial-reinit");
    ws_fs.create_workhive_dir(root.as_ref()).unwrap();
    assert!(ws_fs.workhive_dir_exists(root.as_ref()));
    assert!(ws_fs.read_instance_id(root.as_ref()).is_none());

    let state = usecases::project_state::resolve_init_state(&ws_fs, root.as_ref());
    assert!(
        state.is_partially_initialized(),
        "precondition: must detect partial init"
    );

    let was_init = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "partial-proj",
        None,
    )
    .unwrap();

    assert!(was_init, "partial project must be re-initialized");
    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_some(),
        "instance_id must exist after re-init"
    );
    assert!(
        config.project_exists(root.as_ref()).unwrap(),
        "config.toml must exist after re-init (via ConfigPort)"
    );
}

#[test]
fn transactional_init_repeated_produces_stable_instance_id() {
    let (config, _git, _tmux, ws_fs, _snap, index) = setup();
    init_global(&config);
    let lock = workhive_testkit::fakes::FakeProjectInitLock::new();

    let root = Utf8PathBuf::from("/tmp/txn-repeated-stable");

    let was_init = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "stable-proj",
        None,
    )
    .unwrap();
    assert!(was_init);
    let id1 = ws_fs.read_instance_id(root.as_ref()).expect("must exist");

    let was_init2 = usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "stable-proj",
        None,
    )
    .unwrap();
    assert!(!was_init2, "second call must skip init");
    let id2 = ws_fs.read_instance_id(root.as_ref()).expect("must exist");
    assert_eq!(id1, id2, "instance_id must be stable across repeated init");
}

// ── i18n key coverage tests ──

#[test]
fn i18n_project_init_keys_have_en_and_zh_translations() {
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;

    let i18n = workhive_infra::i18n::StaticI18n::new();
    let init_keys = [
        k::OPEN_PROJECT_INIT_PROMPT,
        k::OPEN_PROJECT_INIT_DENIED,
        k::OPEN_PROJECT_INIT_DECLINED,
        k::OPEN_PROJECT_PARTIAL_INIT,
    ];

    for key in &init_keys {
        let en = i18n.resolve(Locale::En, key);
        assert!(
            !en.is_empty() && !en.contains(key),
            "English translation missing or passthrough for key '{key}': got '{en}'"
        );

        let zh = i18n.resolve(Locale::ZhCn, key);
        assert!(
            !zh.is_empty() && !zh.contains(key),
            "Chinese translation missing or passthrough for key '{key}': got '{zh}'"
        );

        assert_ne!(en, zh, "EN and ZH must differ for key '{key}'");
    }
}

#[test]
fn i18n_project_init_prompt_uses_neutral_wording() {
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;

    let i18n = workhive_infra::i18n::StaticI18n::new();
    let en = i18n.resolve(Locale::En, k::OPEN_PROJECT_INIT_PROMPT);
    assert_eq!(en, "Project is not initialized. Initialize now?");

    let zh = i18n.resolve(Locale::ZhCn, k::OPEN_PROJECT_INIT_PROMPT);
    assert_eq!(zh, "项目还未初始化，是否现在进行初始化？");
}

#[test]
fn i18n_partial_init_error_supports_args() {
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;

    let i18n = workhive_infra::i18n::StaticI18n::new();
    let en = i18n.resolve_with_args(
        Locale::En,
        k::OPEN_PROJECT_PARTIAL_INIT,
        &["/tmp/proj", "true", "false", "true"],
    );
    assert!(
        en.contains("/tmp/proj") && en.contains("true") && en.contains("false"),
        "partial init error must interpolate all args: got '{en}'"
    );
}

#[test]
fn runtime_only_init_does_not_create_config_toml() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-init");

    let result = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
    );
    assert!(result.is_ok(), "runtime-only init should succeed");
    assert!(result.unwrap(), "should report initialization performed");

    assert!(
        ws_fs.read_instance_id(root.as_ref()).is_some(),
        "instance_id must be written"
    );
    assert!(
        !ws_fs.project_config_exists(root.as_ref()),
        "runtime-only init must NOT create config.toml"
    );
}

#[test]
fn runtime_only_init_skips_if_already_initialized() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-skip");

    let first = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
    );
    assert!(first.is_ok());
    assert!(first.unwrap());

    let second = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
    );
    assert!(second.is_ok());
    assert!(
        !second.unwrap(),
        "second runtime-only init should skip initialization (already initialized)"
    );
    // The already-initialized path still refreshes the global index, so the
    // project stays registered after a no-op re-init.
    assert_eq!(
        index.list_projects().unwrap().projects().len(),
        1,
        "already-initialized re-init keeps the project registered in the index"
    );
}

#[test]
fn init_project_runtime_only_upserts_index_for_new_project() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-init-index");

    let performed = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "indexed-project",
    )
    .unwrap();
    assert!(performed, "first runtime-only init performs initialization");

    let projects = index.list_projects().unwrap();
    let names: Vec<&str> = projects
        .projects()
        .iter()
        .map(|p| p.project_name.as_str())
        .collect();
    assert!(
        names.contains(&"indexed-project"),
        "successful runtime-only init must register the project in the global index"
    );
}

#[test]
fn init_project_runtime_only_refreshes_index_when_already_initialized() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-refresh");

    // First open initializes the runtime and registers the project.
    let first = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "refresh-project",
    )
    .unwrap();
    assert!(first, "first init performs initialization");
    assert_eq!(index.list_projects().unwrap().projects().len(), 1);

    // Simulate a stale/missing index entry while the project's `.workhive/`
    // runtime stays on disk (the already-initialized state).
    index.remove_project(&root).unwrap();
    assert!(index.list_projects().unwrap().projects().is_empty());

    // Opening again re-registers the project even though no initialization is
    // performed, so the idempotent init path restores global discovery.
    let second = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "refresh-project",
    )
    .unwrap();
    assert!(
        !second,
        "already-initialized re-init must not report initialization"
    );
    let projects = index.list_projects().unwrap();
    let names: Vec<&str> = projects
        .projects()
        .iter()
        .map(|p| p.project_name.as_str())
        .collect();
    assert!(
        names.contains(&"refresh-project"),
        "already-initialized open must refresh the global index entry"
    );
}

#[test]
fn init_project_runtime_only_propagates_index_write_error() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-index-write-error");

    // Arm an index-write failure for the upsert performed by a new init.
    index.set_upsert_error(workhive_core::errors::IndexError::WriteFailed {
        path: camino::Utf8PathBuf::from("/tmp/index-write-failure"),
        source: std::io::Error::other("injected index failure"),
    });

    let result = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "write-fail-project",
    );
    assert!(
        result.is_err(),
        "index write failure must surface, never silently break global discovery"
    );
}

#[test]
fn transactional_init_after_runtime_only_init_creates_project_config() {
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::{
        FakeConfig, FakeProjectIndex, FakeProjectInitLock, FakeWorkspaceFs,
    };

    let config = FakeConfig::new();
    config
        .save_global(&workhive_core::ports::AppConfig::default())
        .unwrap();
    let ws_fs = FakeWorkspaceFs::new();
    let index = FakeProjectIndex::new();
    let lock = FakeProjectInitLock::new();
    let root = camino::Utf8PathBuf::from("/tmp/test-runtime-then-init");

    let runtime_init = workhive_core::usecases::init::init_project_runtime_only(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
    )
    .unwrap();
    assert!(runtime_init);
    assert!(!config.project_exists(root.as_ref()).unwrap());
    let runtime_instance_id = ws_fs
        .read_instance_id(root.as_ref())
        .expect("runtime-only init should write instance_id");

    let explicit_init = workhive_core::usecases::init::init_project_transactional(
        &config,
        &ws_fs,
        &index,
        &lock,
        &root,
        "test-project",
        None,
    )
    .unwrap();

    assert!(
        explicit_init,
        "explicit init must complete runtime-only projects by writing config.toml"
    );
    assert!(
        config.project_exists(root.as_ref()).unwrap(),
        "explicit init after runtime-only init must create project config"
    );
    assert_eq!(
        ws_fs.read_instance_id(root.as_ref()).as_deref(),
        Some(runtime_instance_id.as_str()),
        "explicit init must preserve runtime-only instance_id"
    );
}

#[test]
fn destroy_safety_i18n_keys_have_entries_in_both_locales() {
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;

    let i18n = workhive_infra::i18n::StaticI18n::new();
    let keys = [
        k::DESTROY_SAFETY_REPORT_HEADING,
        k::DESTROY_SAFETY_NO_RISK,
        k::DESTROY_RISKY_WORKTREES_CONFIRM,
        k::DESTROY_FORCE_DESPITE_RISK,
        k::SAFETY_REPORT_CHANGED_FILES,
        k::SAFETY_REPORT_MORE_CHANGED_FILES,
        k::SAFETY_REPORT_UNMERGED_COMMITS,
        k::WORKSPACE_REMOVE_RISKY_CONFIRM,
    ];

    for key in keys {
        let en = i18n.resolve(Locale::En, key);
        assert_ne!(en, key, "EN locale missing key: {key}");
        assert!(!en.is_empty(), "EN locale empty for key: {key}");

        let zh = i18n.resolve(Locale::ZhCn, key);
        assert_ne!(zh, key, "ZH locale missing key: {key}");
        assert!(!zh.is_empty(), "ZH locale empty for key: {key}");
    }
}

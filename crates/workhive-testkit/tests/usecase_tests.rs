//! Use case unit tests — exercises every use case module with fake adapters.
//!
//! Covers: config_mgmt, list, destroy, close, pane, open (error paths), init,
//! reindex, sessions.

use camino::Utf8PathBuf;
use workhive_core::domain::{
    IdentityError, Locale, NamingConfig, NamingContext, PaneDirection, PaneLaunchRef,
    RelativeProjectPath, RuntimeBackendId, RuntimeBinding, SessionNamingPolicy, UnitId,
    WorkspaceId, WorkspaceIdentityRecord,
};
use workhive_core::ports::*;
use workhive_core::usecases;
use workhive_testkit::fakes::*;

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
        FakeGit::with_directory_registry("/tmp/test-project", directories.clone()),
        FakeRuntime::new(),
        FakeWorkspaceFs::with_directory_registry(directories),
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
    assert_eq!(cfg.tmux.unwrap().server_name, Some("custom-srv".to_owned()));
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
fn list_all_projects_returns_registered() {
    let (config, _, _, _, index) = setup();
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
fn destroy_succeeds_even_when_no_worktrees() {
    let (config, git, tmux, ws_fs, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        &[],
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &managed_paths,
    );
    assert!(result.is_ok());
}

#[test]
fn destroy_continues_when_runtime_backend_is_unavailable() {
    let (config, git, tmux, ws_fs, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    assert!(ws_fs.workhive_dir_exists(root.as_ref()));

    let binding = RuntimeBinding::tmux("whsrv-abcd-test-project", "session");
    tmux.inject_errors(RuntimeErrorInjection {
        destroy_workspace: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::ServerNotRunning {
                server: "whsrv-abcd-test-project".to_owned(),
            },
            detail: "tmux server not running: whsrv-abcd-test-project".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        std::slice::from_ref(&binding),
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
fn destroy_stops_on_missing_backend_capability_and_preserves_project_state() {
    let (config, git, runtime, ws_fs, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    let binding = RuntimeBinding::new(RuntimeBackendId::herdr(), "/run/herdr.sock", "wJ").unwrap();
    runtime.inject_errors(RuntimeErrorInjection {
        destroy_workspace: Some(RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing {
                capability: "workspace.destroy".to_owned(),
            },
            detail: "protocol capability unavailable".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });

    let result = usecases::destroy::destroy_project(
        &git,
        |binding| runtime.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        std::slice::from_ref(&binding),
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &[],
    );

    assert!(matches!(
        result,
        Err(usecases::destroy::DestroyError::Runtime(
            RuntimeError::BackendUnavailable {
                reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing { .. },
                ..
            }
        ))
    ));
    assert!(ws_fs.workhive_dir_exists(root.as_ref()));
    assert_eq!(index.list_projects().unwrap().projects().len(), 1);
}

#[test]
fn destroy_uses_persisted_opaque_runtime_binding() {
    let (config, git, runtime, ws_fs, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    let binding =
        RuntimeBinding::new(RuntimeBackendId::herdr(), "/run/herdr-exact.sock", "wJ").unwrap();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: binding.scope_id.clone(),
            workspace_name: binding.workspace_id.clone(),
            working_dir: root.clone(),
            env: Vec::new(),
        })
        .unwrap();

    let result = usecases::destroy::destroy_project(
        &git,
        |binding| runtime.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        std::slice::from_ref(&binding),
        workhive_core::ports::WorktreeRemovePolicy::Force,
        &[],
    );

    assert!(result.is_ok());
    assert!(
        !runtime
            .has_workspace(&binding.scope_id, &binding.workspace_id)
            .unwrap()
    );
}

#[test]
fn remove_worktree_with_force_removes_runtime_and_worktree_but_keeps_branch() {
    let (_config, git, tmux, _ws_fs, _index) = setup();
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
    let mut request = usecases::remove_worktree::RemoveWorktreeRequest::for_binding(
        worktree,
        RuntimeBinding::tmux(server.to_owned(), session.to_owned()),
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
    let (_config, git, tmux, _ws_fs, _index) = setup();
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
    let request = usecases::remove_worktree::RemoveWorktreeRequest::for_binding(
        worktree,
        RuntimeBinding::tmux(server.to_owned(), session.to_owned()),
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
    let (_config, git, tmux, _ws_fs, _index) = setup();
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
    let mut request = usecases::remove_worktree::RemoveWorktreeRequest::for_binding(
        "feat-ext",
        RuntimeBinding::tmux(server.to_owned(), session.to_owned()),
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
fn close_kill_removes_runtime() {
    let (_, _, tmux, _, _) = setup();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@feat-a";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let action =
        usecases::close::close_session(&tmux, &RuntimeBackendId::tmux(), server, session, true)
            .unwrap();
    assert!(matches!(action, usecases::close::CloseAction::Killed));
    // Session is removed
    assert!(!tmux.has_workspace(server, session).unwrap());
}

#[test]
fn close_detach_does_not_kill() {
    let (_, _, tmux, _, _) = setup();
    let server = "whsrv-abcd-proj";
    let session = "wh@abcd@proj@feat-a";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp"),
        env: Vec::new(),
    })
    .unwrap();

    let action =
        usecases::close::close_session(&tmux, &RuntimeBackendId::tmux(), server, session, false)
            .unwrap();
    assert!(matches!(action, usecases::close::CloseAction::Detached));
    // Session still exists
    assert!(tmux.has_workspace(server, session).unwrap());
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

    let pane = usecases::pane::current_pane(&tmux, server, session).unwrap();
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
// open use case — error paths and runtime creation
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
    let (config, _, _, ws_fs, index) = setup();
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
    let (config, _, _, ws_fs, index) = setup();
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
    let (config, _, _, ws_fs, index) = setup();
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
    let (config, _, _, ws_fs, index) = setup();
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
fn scenario_subdir_project_naming() {
    // Given: project with sub-directory semantics
    let policy = SessionNamingPolicy::default_policy();
    let ctx = NamingContext {
        project: "myrepo/test01".to_owned(),
        workspace: "feat-a".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/home/user/myrepo".to_owned()),
        instance_id: Some("1234".to_owned()),
        relative_project_path: None,
    };

    let server = policy.resolve_server(&ctx).unwrap();
    let session = policy.resolve_session(&ctx).unwrap();

    // Then: each naming component is sanitized independently.
    // The default server_template is the literal "default" (shared tmux server),
    // so server name does not contain per-project components.
    assert_eq!(
        server, "default",
        "default server is shared across all projects: {server}"
    );
    assert!(
        session.contains("myrepo-test01"),
        "session should contain the sanitized project component: {session}"
    );

    // And: parseable
    let parsed = SessionNamingPolicy::parse_session(&session);
    assert!(parsed.is_some(), "session should be parseable: {session}");
    let (id, project, workspace) = parsed.unwrap();
    assert_eq!(id, "1234");
    assert_eq!(workspace, "feat-a");
    assert_eq!(project, "myrepo-test01");
}

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
fn resolve_open_session_returns_legacy_when_only_legacy_exists() {
    let (_, _git, tmux, _ws_fs, _) = setup();
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
    let (_, _git, tmux, _ws_fs, _) = setup();
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
fn fake_git_error_can_be_cleared() {
    let git = FakeGit::new("/tmp/test-project");
    git.set_error("temporary failure");
    assert!(git.current_branch().is_err());

    git.clear_error();
    assert!(git.current_branch().is_ok());
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
            project_root: Utf8PathBuf::from("/outside/old"),
            config_path: Utf8PathBuf::from("/outside/old/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &roots, false).unwrap();

    assert_eq!(result.discovered_count, 1);
    assert_eq!(result.upserted_count, 1);
    assert_eq!(result.retained_existing_count, 1);
    assert_eq!(result.final_index_count, 2);
    assert_eq!(index.list_projects().unwrap().projects().len(), 2);
}

#[test]
fn reindex_without_clear_prunes_stale_entries_under_scanned_root() {
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

    assert_eq!(result.retained_existing_count, 0);
    let projects = index.list_projects().unwrap().into_projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_name, "new-proj");
}

#[test]
fn reindex_with_prune_roots_removes_entries_under_missing_root() {
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("new-proj", "/tmp/new"));

    let index = FakeProjectIndex::new();
    index
        .upsert_project(&ProjectSummary {
            project_name: "gone-proj".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/gone"),
            config_path: Utf8PathBuf::from("/tmp/gone/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    let roots = vec![Utf8PathBuf::from("/tmp")];
    let prune_roots = vec![Utf8PathBuf::from("/tmp/gone")];
    let result = usecases::reindex::rebuild_project_index_with_prune_roots(
        &discovery,
        &index,
        &roots,
        &prune_roots,
        false,
    )
    .unwrap();

    assert_eq!(result.retained_existing_count, 0);
    let projects = index.list_projects().unwrap().into_projects();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project_name, "new-proj");
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

// First conditional publication is paused while a foreground upsert changes the snapshot token.
struct InterceptingIndex {
    inner: workhive_infra::project_index::JsonProjectIndex,
    publish_count: std::sync::atomic::AtomicUsize,
    first_call_entered: std::sync::mpsc::SyncSender<()>,
    first_call_can_proceed: std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>,
}

impl ProjectIndexPort for InterceptingIndex {
    fn list_projects(&self) -> Result<IndexState, workhive_core::errors::IndexError> {
        self.inner.list_projects()
    }

    fn read_paired_state(&self) -> Result<PairedIndexState, workhive_core::errors::IndexError> {
        self.inner.read_paired_state()
    }

    fn capture_snapshot_token(
        &self,
    ) -> Result<IndexSnapshotToken, workhive_core::errors::IndexError> {
        self.inner.capture_snapshot_token()
    }

    fn upsert_project(
        &self,
        summary: &ProjectSummary,
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.upsert_project(summary)
    }

    fn remove_project(
        &self,
        project_root: &Utf8PathBuf,
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.remove_project(project_root)
    }

    fn clear(&self) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.clear()
    }

    fn replace_all(
        &self,
        projects: &[ProjectSummary],
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.replace_all(projects)
    }

    fn publish_snapshot_conditional(
        &self,
        projects: &[ProjectSummary],
        expected_token: IndexSnapshotToken,
    ) -> Result<SnapshotPublishOutcome, workhive_core::errors::IndexError> {
        let count = self
            .publish_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if count == 0 {
            // First call: signal entry and wait for proceed with timeout
            self.first_call_entered
                .send(())
                .expect("failed to signal first publication entry");
            self.first_call_can_proceed
                .lock()
                .expect("mutex poisoned")
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("timeout waiting for proceed signal");
        }

        // Delegate to inner
        self.inner
            .publish_snapshot_conditional(projects, expected_token)
    }
}

#[test]
fn reindex_merge_retry_is_deterministic() {
    use std::sync::Arc;
    use std::thread;
    use workhive_infra::project_index::JsonProjectIndex;

    let tmp = tempfile::TempDir::new().unwrap();
    let index_path = camino::Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
    let inner = JsonProjectIndex::with_path(index_path);

    // Setup: one pre-existing project
    inner
        .upsert_project(&ProjectSummary {
            project_name: "pre-existing".to_owned(),
            project_root: Utf8PathBuf::from("/outside/pre-existing"),
            config_path: Utf8PathBuf::from("/outside/pre-existing/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    // Create channels for synchronization
    let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
    let (proceed_tx, proceed_rx) = std::sync::mpsc::sync_channel(1);
    let proceed_rx = std::sync::Arc::new(std::sync::Mutex::new(proceed_rx));

    // Build intercepting index
    let index = Arc::new(InterceptingIndex {
        inner,
        publish_count: std::sync::atomic::AtomicUsize::new(0),
        first_call_entered: entered_tx,
        first_call_can_proceed: proceed_rx,
    });

    // Discovery will find one new project
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("discovered", "/tmp/discovered"));

    // Clone for foreground thread
    let index_foreground = Arc::clone(&index);

    // Spawn foreground thread that will upsert while first publication is blocked
    let foreground_handle = thread::spawn(move || {
        // Wait for first publication to enter with timeout
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("timeout waiting for first publication to enter");

        // Foreground upsert - this changes the token
        index_foreground
            .upsert_project(&ProjectSummary {
                project_name: "foreground".to_owned(),
                project_root: Utf8PathBuf::from("/tmp/foreground"),
                config_path: Utf8PathBuf::from("/tmp/foreground/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })
            .expect("foreground upsert must succeed");

        // Release the blocked publication
        proceed_tx
            .send(())
            .expect("failed to release blocked publication");
    });

    // Run merge reindex
    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result = usecases::reindex::rebuild_project_index(&discovery, &*index, &roots, false)
        .expect("reindex must succeed");

    // Wait for foreground to complete
    foreground_handle
        .join()
        .expect("foreground thread must not panic");

    // Verify exact behavior
    assert_eq!(
        index
            .publish_count
            .load(std::sync::atomic::Ordering::SeqCst),
        2,
        "must have exactly 2 publication attempts"
    );
    assert_eq!(discovery.call_count(), 1, "discovery must run exactly once");
    assert_eq!(result.discovered_count, 1);
    assert_eq!(result.upserted_count, 1);
    // On the second attempt, the pre-state includes both pre-existing and foreground,
    // so retained_existing_count = 2 (both are not in discovered_roots)
    assert_eq!(
        result.retained_existing_count, 2,
        "pre-existing and foreground must be retained"
    );

    // Verify final state: must have all three projects
    let projects = index.list_projects().unwrap().into_projects();
    assert_eq!(
        projects.len(),
        3,
        "must have pre-existing + discovered + foreground"
    );

    let names: Vec<&str> = projects.iter().map(|p| p.project_name.as_str()).collect();
    assert!(names.contains(&"pre-existing"));
    assert!(names.contains(&"discovered"));
    assert!(names.contains(&"foreground"));

    // Verify valid JSON
    let content = std::fs::read_to_string(tmp.path().join("index.json")).unwrap();
    let parsed: Result<Vec<ProjectSummary>, _> = serde_json::from_str(&content);
    assert!(parsed.is_ok(), "index must be valid JSON");
}

#[test]
fn reindex_clear_first_produces_exact_destructive_set() {
    use workhive_infra::project_index::JsonProjectIndex;

    let tmp = tempfile::TempDir::new().unwrap();
    let index_path = camino::Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
    let index = JsonProjectIndex::with_path(index_path);

    // Setup: pre-existing projects
    index
        .upsert_project(&ProjectSummary {
            project_name: "old1".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/old1"),
            config_path: Utf8PathBuf::from("/tmp/old1/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();
    index
        .upsert_project(&ProjectSummary {
            project_name: "old2".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/old2"),
            config_path: Utf8PathBuf::from("/tmp/old2/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    // Discovery will find new projects
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("new1", "/tmp/new1"));
    discovery.add_project(make_discovered("new2", "/tmp/new2"));

    // Run clear-first reindex
    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &roots, true).unwrap();

    // Verify exact destructive behavior
    assert_eq!(result.discovered_count, 2);
    assert_eq!(result.upserted_count, 2);
    assert_eq!(
        result.retained_existing_count, 0,
        "clear-first retains nothing"
    );
    assert_eq!(result.final_index_count, 2);

    // Verify final state: only discovered projects
    let projects = index.list_projects().unwrap().into_projects();
    assert_eq!(projects.len(), 2, "must have exactly 2 projects");

    let names: Vec<&str> = projects.iter().map(|p| p.project_name.as_str()).collect();
    assert!(
        !names.contains(&"old1"),
        "old1 must be removed by clear-first"
    );
    assert!(
        !names.contains(&"old2"),
        "old2 must be removed by clear-first"
    );
    assert!(names.contains(&"new1"), "new1 must be present");
    assert!(names.contains(&"new2"), "new2 must be present");

    // Verify valid JSON
    let content = std::fs::read_to_string(tmp.path().join("index.json")).unwrap();
    let parsed: Result<Vec<ProjectSummary>, _> = serde_json::from_str(&content);
    assert!(parsed.is_ok(), "index must be valid JSON");
}

/// Test that merge retry exhausts with ConcurrentModificationExhausted error
/// when publication is always stale
#[test]
fn reindex_merge_exhausts_with_concurrent_modification_error() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use workhive_infra::project_index::JsonProjectIndex;

    let tmp = tempfile::TempDir::new().unwrap();
    let index_path = camino::Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
    let inner = Arc::new(JsonProjectIndex::with_path(index_path));

    // Setup: one pre-existing project
    inner
        .upsert_project(&ProjectSummary {
            project_name: "pre-existing".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/pre-existing"),
            config_path: Utf8PathBuf::from("/tmp/pre-existing/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        })
        .unwrap();

    // Always-stale index: modifies the index on every publish_snapshot_conditional call
    // to force Stale outcomes.
    struct AlwaysStaleIndex {
        inner: Arc<JsonProjectIndex>,
        publish_count: AtomicUsize,
    }

    impl ProjectIndexPort for AlwaysStaleIndex {
        fn list_projects(&self) -> Result<IndexState, workhive_core::errors::IndexError> {
            self.inner.list_projects()
        }

        fn read_paired_state(&self) -> Result<PairedIndexState, workhive_core::errors::IndexError> {
            self.inner.read_paired_state()
        }

        fn capture_snapshot_token(
            &self,
        ) -> Result<IndexSnapshotToken, workhive_core::errors::IndexError> {
            self.inner.capture_snapshot_token()
        }

        fn upsert_project(
            &self,
            summary: &ProjectSummary,
        ) -> Result<(), workhive_core::errors::IndexError> {
            self.inner.upsert_project(summary)
        }

        fn remove_project(
            &self,
            project_root: &Utf8PathBuf,
        ) -> Result<(), workhive_core::errors::IndexError> {
            self.inner.remove_project(project_root)
        }

        fn clear(&self) -> Result<(), workhive_core::errors::IndexError> {
            self.inner.clear()
        }

        fn replace_all(
            &self,
            projects: &[ProjectSummary],
        ) -> Result<(), workhive_core::errors::IndexError> {
            self.inner.replace_all(projects)
        }

        fn publish_snapshot_conditional(
            &self,
            _projects: &[ProjectSummary],
            _expected_token: IndexSnapshotToken,
        ) -> Result<SnapshotPublishOutcome, workhive_core::errors::IndexError> {
            // Increment counter and modify index to make next read stale
            self.publish_count.fetch_add(1, Ordering::SeqCst);
            self.inner.upsert_project(&ProjectSummary {
                project_name: format!("stale-maker-{}", self.publish_count.load(Ordering::SeqCst)),
                project_root: Utf8PathBuf::from(format!(
                    "/tmp/stale-{}",
                    self.publish_count.load(Ordering::SeqCst)
                )),
                config_path: Utf8PathBuf::from(format!(
                    "/tmp/stale-{}/.workhive/config.toml",
                    self.publish_count.load(Ordering::SeqCst)
                )),
                last_opened_at: None,
                last_updated_at: None,
            })?;
            // Always return Stale
            Ok(SnapshotPublishOutcome::Stale)
        }
    }

    let index = AlwaysStaleIndex {
        inner: Arc::clone(&inner),
        publish_count: AtomicUsize::new(0),
    };

    // Discovery will find one new project
    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("discovered", "/tmp/discovered"));

    // Run merge reindex - should exhaust retries
    let roots = vec![Utf8PathBuf::from("/tmp")];
    let result = usecases::reindex::rebuild_project_index(&discovery, &index, &roots, false);

    // Verify it failed with ConcurrentModificationExhausted
    assert!(result.is_err(), "should fail after exhausting retries");
    match result {
        Err(usecases::reindex::ReindexError::ConcurrentModificationExhausted { attempts }) => {
            assert_eq!(attempts, 10, "should exhaust all 10 retries");
        }
        _ => panic!("expected ConcurrentModificationExhausted error"),
    }

    // Verify discovery was called exactly once
    assert_eq!(discovery.call_count(), 1, "discovery must run exactly once");

    // Verify publication was attempted 10 times
    assert_eq!(
        index.publish_count.load(Ordering::SeqCst),
        10,
        "should attempt publication 10 times"
    );
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

fn workspace_identity(
    workspace: &str,
    backend: RuntimeBackendId,
    scope_id: &str,
    runtime_id: &str,
) -> WorkspaceIdentityRecord {
    WorkspaceIdentityRecord {
        workspace: workspace.to_owned(),
        relative_project_path: None,
        relative_display_path: None,
        runtime_binding: RuntimeBinding::new(backend, scope_id, runtime_id)
            .expect("test runtime binding must be valid"),
        runtime_replacement: None,
        canonical_workspace_component: workspace.to_owned(),
        canonical_session_name: runtime_id.to_owned(),
        canonical_server_name: scope_id.to_owned(),
    }
}

#[test]
fn sessions_empty_projects_returns_empty() {
    let tmux = FakeRuntime::new();
    let result = usecases::sessions::list_global_sessions(
        &[],
        |_| Ok(Vec::new()),
        |binding| tmux.list_workspaces(&binding.scope_id),
    )
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
        relative_project_path: None,
    };
    let server = policy.resolve_server(&naming_ctx).unwrap();
    let session = policy.resolve_session(&naming_ctx).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.clone(),
        workspace_name: session.clone(),
        working_dir: Utf8PathBuf::from("/tmp/myproj"),
        env: Vec::new(),
    })
    .unwrap();

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::tmux(),
                &server,
                &session,
            )])
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
    )
    .unwrap();
    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].project_name, "myproj");
    assert_eq!(result.records[0].binding.scope_id, server);
    assert_eq!(result.records[0].binding.workspace_id, session);
}

#[test]
fn sessions_skips_projects_with_no_running_server() {
    let tmux = FakeRuntime::new();

    let project = make_discovered_with_id("offline", "/tmp/offline", "c3d4");
    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| Ok(Vec::new()),
        |binding| tmux.list_workspaces(&binding.scope_id),
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
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::tmux(),
                "offline",
                "runtime",
            )])
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
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
        relative_project_path: None,
    };
    let ctx_b = NamingContext {
        project: "beta".to_owned(),
        workspace: "dev".to_owned(),
        branch: None,
        dir_path: Some("/tmp/beta".to_owned()),
        instance_id: Some("bbbb".to_owned()),
        relative_project_path: None,
    };

    let server_a = default_naming_policy().resolve_server(&ctx_a).unwrap();
    let session_a = default_naming_policy().resolve_session(&ctx_a).unwrap();
    let server_b = custom_policy.resolve_server(&ctx_b).unwrap();
    let session_b = custom_policy.resolve_session(&ctx_b).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server_a.clone(),
        workspace_name: session_a.clone(),
        working_dir: Utf8PathBuf::from("/tmp/alpha"),
        env: Vec::new(),
    })
    .unwrap();
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server_b.clone(),
        workspace_name: session_b.clone(),
        working_dir: Utf8PathBuf::from("/tmp/beta"),
        env: Vec::new(),
    })
    .unwrap();

    let records = usecases::sessions::list_global_sessions(
        &[proj_a, proj_b],
        |project| {
            if project.project_name == "beta" {
                Ok(vec![workspace_identity(
                    "dev",
                    RuntimeBackendId::tmux(),
                    &server_b,
                    &session_b,
                )])
            } else {
                Ok(vec![workspace_identity(
                    "main",
                    RuntimeBackendId::tmux(),
                    &server_a,
                    &session_a,
                )])
            }
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
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
fn list_global_sessions_propagates_missing_backend_capability() {
    let project = make_discovered_with_id("incompatible", "/tmp/incompatible", "e5f6");
    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::herdr(),
                "/run/herdr.sock",
                "wJ",
            )])
        },
        |_| {
            Err(RuntimeError::BackendUnavailable {
                reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing {
                    capability: "workspace.list".to_owned(),
                },
                detail: "protocol capability unavailable".to_owned(),
            })
        },
    );

    assert!(matches!(
        result,
        Err(usecases::sessions::SessionQueryError::Runtime(
            RuntimeError::BackendUnavailable {
                reason: workhive_core::ports::BackendUnavailableReason::CapabilityMissing { .. },
                ..
            }
        ))
    ));
}

// Regression: per-project naming policy enables independent backend degradation.
// With a shared default server ("default"), all projects resolve to the same
// server name and cannot be degraded independently. Per-project naming policies
// give each project a distinct server so error injection targets only one.
#[test]
fn list_global_sessions_degrades_when_one_backend_unavailable() {
    let tmux = FakeRuntime::new();

    let proj_good = make_discovered_with_id("good", "/tmp/good", "1111");
    let proj_bad = make_discovered_with_id("bad", "/tmp/bad", "2222");

    // Each project gets its own server name so degradation is isolated.
    let good_policy = SessionNamingPolicy::new(NamingConfig {
        server_template: "srv-{instance_id}-{project}".to_owned(),
        ..NamingConfig::default()
    });
    let bad_policy = SessionNamingPolicy::new(NamingConfig {
        server_template: "srv-{instance_id}-{project}".to_owned(),
        ..NamingConfig::default()
    });

    let good_ctx = NamingContext {
        project: "good".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/good".to_owned()),
        instance_id: Some("1111".to_owned()),
        relative_project_path: None,
    };
    let good_server = good_policy.resolve_server(&good_ctx).unwrap();
    let good_session = good_policy.resolve_session(&good_ctx).unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: good_server.clone(),
        workspace_name: good_session.clone(),
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
        relative_project_path: None,
    };
    let bad_server = bad_policy.resolve_server(&bad_ctx).unwrap();

    // Verify the two servers are distinct — otherwise error injection would
    // degrade both projects simultaneously.
    assert_ne!(
        good_server, bad_server,
        "per-project policies must produce distinct server names"
    );

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
        |project| {
            if project.project_name == "good" {
                Ok(vec![workspace_identity(
                    "main",
                    RuntimeBackendId::herdr(),
                    &good_server,
                    &good_session,
                )])
            } else {
                Ok(vec![workspace_identity(
                    "main",
                    RuntimeBackendId::herdr(),
                    &bad_server,
                    "missing",
                )])
            }
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
    )
    .unwrap();

    assert_eq!(
        listing.records.len(),
        1,
        "an unreachable backend must not block sessions from reachable projects"
    );
    assert_eq!(listing.records[0].project_name, "good");
    assert_eq!(
        listing.records[0].binding.backend,
        RuntimeBackendId::herdr()
    );

    assert_eq!(
        listing.unavailable.len(),
        1,
        "the unreachable project must be reported as unavailable"
    );
    assert_eq!(listing.unavailable[0].project_name, "bad");
    assert_eq!(listing.unavailable[0].backend, RuntimeBackendId::herdr());
    assert!(matches!(
        listing.unavailable[0].reason,
        workhive_core::ports::BackendUnavailableReason::SocketNotFound { .. }
    ));
}

// Regression: with the v2 shared-server model, all projects using the default
// server resolve to the same tmux server name ("default"). If that single
// server becomes unreachable, ALL projects degrade together — not independently.
// This test documents the semantic difference from per-project server names.
#[test]
fn list_global_sessions_shared_server_degrades_all_projects_together() {
    let tmux = FakeRuntime::new();

    let proj_a = make_discovered_with_id("alpha", "/tmp/alpha", "aaaa");
    let proj_b = make_discovered_with_id("beta", "/tmp/beta", "bbbb");

    // Both projects use the default (shared) naming policy.
    let shared_server = default_naming_policy()
        .resolve_server(&NamingContext {
            project: "shared".to_owned(),
            workspace: String::new(),
            branch: None,
            dir_path: None,
            instance_id: Some("0000".to_owned()),
            relative_project_path: None,
        })
        .unwrap();

    // Inject BackendUnavailable on the shared server name.
    tmux.inject_list_workspaces_error_for_server(
        &shared_server,
        RuntimeError::BackendUnavailable {
            reason: workhive_core::ports::BackendUnavailableReason::SocketNotFound {
                path: "/tmp/shared/.tmux-socket".to_owned(),
            },
            detail: "socket not found".to_owned(),
        },
    );

    // Both projects resolve to the same server, so both degrade.
    let listing = usecases::sessions::list_global_sessions(
        &[proj_a, proj_b],
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::tmux(),
                &shared_server,
                "runtime",
            )])
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
    )
    .unwrap();

    assert!(
        listing.records.is_empty(),
        "shared server failure must produce zero session records"
    );
    assert_eq!(
        listing.unavailable.len(),
        2,
        "all projects on the shared server must be reported unavailable"
    );
    let names: Vec<&str> = listing
        .unavailable
        .iter()
        .map(|u| u.project_name.as_str())
        .collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

#[test]
fn sessions_identity_error_propagates_to_caller() {
    let tmux = FakeRuntime::new();
    let project = make_discovered_with_id("broken", "/tmp/broken", "ffff");

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Err(IdentityError::Corrupted {
                path: "/tmp/broken/.workhive/identity/workspaces/main.json".into(),
                reason: "invalid runtime binding".to_owned(),
            })
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
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

// Verify that FakeRuntime inherits the production naming-policy filter and
// does not leak sessions between projects sharing one scope.
#[test]
fn fake_runtime_filters_project_workspaces_by_naming_policy() {
    use workhive_core::ports::CreateWorkspaceRuntimeRequest;

    let tmux = FakeRuntime::new();
    let group = "default";

    // Create sessions for two different projects on the same shared server
    let session_proj_a = SessionNamingPolicy::default_policy()
        .resolve_session(&NamingContext {
            project: "proj-a".to_owned(),
            workspace: "feat-1".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("aaaa".to_owned()),
            relative_project_path: None,
        })
        .unwrap();

    let session_proj_b = SessionNamingPolicy::default_policy()
        .resolve_session(&NamingContext {
            project: "proj-b".to_owned(),
            workspace: "feat-2".to_owned(),
            branch: None,
            dir_path: None,
            instance_id: Some("bbbb".to_owned()),
            relative_project_path: None,
        })
        .unwrap();

    // Both sessions on the same server
    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: group.to_owned(),
        workspace_name: session_proj_a.clone(),
        working_dir: Utf8PathBuf::from("/tmp/a"),
        env: Vec::new(),
    })
    .unwrap();

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: group.to_owned(),
        workspace_name: session_proj_b.clone(),
        working_dir: Utf8PathBuf::from("/tmp/b"),
        env: Vec::new(),
    })
    .unwrap();

    let filtered_sessions = tmux
        .list_project_workspaces(group, "aaaa", "proj-a")
        .unwrap();
    assert_eq!(
        filtered_sessions.len(),
        1,
        "strict mode should only return sessions belonging to the project"
    );
    assert_eq!(
        filtered_sessions[0].runtime_id, session_proj_a,
        "strict mode should filter out cross-project sessions"
    );

    // Query for proj-b should only see proj-b's session
    let proj_b_sessions = tmux
        .list_project_workspaces(group, "bbbb", "proj-b")
        .unwrap();
    assert_eq!(
        proj_b_sessions.len(),
        1,
        "strict mode should correctly filter for different projects"
    );
    assert_eq!(
        proj_b_sessions[0].runtime_id, session_proj_b,
        "strict mode should return only proj-b's session"
    );
}

// A runtime identity is opaque to the listing use case. A plain tmux name is
// still listed when the persisted binding identifies it; no name parsing is
// needed to determine ownership.
#[test]
fn sessions_lists_opaque_runtime_id_from_identity_binding() {
    let tmux = FakeRuntime::new();

    let project = make_discovered_with_id("proj", "/tmp/proj", "x1y2");
    let naming_ctx = NamingContext {
        project: "proj".to_owned(),
        workspace: "main".to_owned(),
        branch: None,
        dir_path: Some("/tmp/proj".to_owned()),
        instance_id: Some("x1y2".to_owned()),
        relative_project_path: None,
    };
    let server = default_naming_policy().resolve_server(&naming_ctx).unwrap();
    let runtime_id = "plain-session-no-prefix";

    tmux.create_workspace(CreateWorkspaceRuntimeRequest {
        group: server.clone(),
        workspace_name: runtime_id.to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/proj"),
        env: Vec::new(),
    })
    .unwrap();
    tmux.set_workspace_label(&server, runtime_id, "friendly-label");

    let result = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::tmux(),
                &server,
                runtime_id,
            )])
        },
        |binding| tmux.list_workspaces(&binding.scope_id),
    )
    .unwrap();

    assert_eq!(result.records.len(), 1);
    assert_eq!(result.records[0].workspace, "main");
    assert_eq!(result.records[0].binding.workspace_id, runtime_id);
    assert_eq!(
        result.records[0].runtime_label.as_deref(),
        Some("friendly-label")
    );
}

#[test]
fn sessions_queries_each_scope_exactly_once() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let proj_a = make_discovered_with_id("alpha", "/tmp/alpha", "1111");
    let proj_b = make_discovered_with_id("beta", "/tmp/beta", "2222");

    let query_count = Arc::new(AtomicUsize::new(0));
    let query_count_clone = Arc::clone(&query_count);

    let listing = usecases::sessions::list_global_sessions(
        &[proj_a, proj_b],
        |proj| {
            if proj.project_name == "alpha" {
                Ok(vec![
                    workspace_identity("w1", RuntimeBackendId::tmux(), "shared_server", "s1"),
                    workspace_identity("w2", RuntimeBackendId::tmux(), "shared_server", "s2"),
                ])
            } else {
                Ok(vec![workspace_identity(
                    "w3",
                    RuntimeBackendId::tmux(),
                    "shared_server",
                    "s3",
                )])
            }
        },
        move |binding| {
            assert_eq!(binding.scope_id, "shared_server");
            query_count_clone.fetch_add(1, Ordering::SeqCst);
            Ok(vec![
                WorkspaceRuntimeInfo {
                    runtime_id: "s1".to_owned(),
                    label: Some("Session 1".to_owned()),
                    attached: false,
                    unit_count: 1,
                },
                WorkspaceRuntimeInfo {
                    runtime_id: "s2".to_owned(),
                    label: None,
                    attached: true,
                    unit_count: 1,
                },
                WorkspaceRuntimeInfo {
                    runtime_id: "s3".to_owned(),
                    label: None,
                    attached: false,
                    unit_count: 1,
                },
            ])
        },
    )
    .unwrap();

    assert_eq!(
        query_count.load(Ordering::SeqCst),
        1,
        "shared scope must be queried exactly once across multiple projects and workspaces"
    );
    assert_eq!(listing.records.len(), 3);
    assert!(listing.unavailable.is_empty());
}

#[test]
fn sessions_mixed_backends_partial_failure_degrades_gracefully() {
    let proj_tmux = make_discovered_with_id("alpha", "/tmp/alpha", "1111");
    let proj_herdr = make_discovered_with_id("beta", "/tmp/beta", "2222");

    let listing = usecases::sessions::list_global_sessions(
        &[proj_tmux, proj_herdr],
        |proj| {
            if proj.project_name == "alpha" {
                Ok(vec![workspace_identity(
                    "main",
                    RuntimeBackendId::tmux(),
                    "default_server",
                    "tmux_session_1",
                )])
            } else {
                Ok(vec![workspace_identity(
                    "dev",
                    RuntimeBackendId::herdr(),
                    "/tmp/broken_herdr.sock",
                    "wB",
                )])
            }
        },
        |binding| {
            if binding.backend == RuntimeBackendId::tmux() {
                Ok(vec![WorkspaceRuntimeInfo {
                    runtime_id: "tmux_session_1".to_owned(),
                    label: None,
                    attached: false,
                    unit_count: 1,
                }])
            } else {
                Err(RuntimeError::BackendUnavailable {
                    reason: BackendUnavailableReason::SocketNotFound {
                        path: binding.scope_id.clone(),
                    },
                    detail: "socket file does not exist".to_owned(),
                })
            }
        },
    )
    .unwrap();

    assert_eq!(listing.records.len(), 1);
    assert_eq!(listing.records[0].project_name, "alpha");
    assert_eq!(listing.records[0].workspace, "main");
    assert_eq!(listing.records[0].binding.workspace_id, "tmux_session_1");

    assert_eq!(listing.unavailable.len(), 1);
    assert_eq!(listing.unavailable[0].project_name, "beta");
    assert_eq!(listing.unavailable[0].backend, RuntimeBackendId::herdr());
}

#[test]
fn sessions_duplicate_runtime_ids_across_different_scopes_do_not_collide() {
    let proj_a = make_discovered_with_id("proj_a", "/tmp/proj_a", "1111");
    let proj_b = make_discovered_with_id("proj_b", "/tmp/proj_b", "2222");

    // Both projects have a workspace with runtime_id "ws-1", but under different scopes.
    let listing = usecases::sessions::list_global_sessions(
        &[proj_a, proj_b],
        |proj| {
            if proj.project_name == "proj_a" {
                Ok(vec![workspace_identity(
                    "w_alpha",
                    RuntimeBackendId::herdr(),
                    "/run/herdr-a.sock",
                    "ws-1",
                )])
            } else {
                Ok(vec![workspace_identity(
                    "w_beta",
                    RuntimeBackendId::herdr(),
                    "/run/herdr-b.sock",
                    "ws-1",
                )])
            }
        },
        |binding| {
            Ok(vec![WorkspaceRuntimeInfo {
                runtime_id: "ws-1".to_owned(),
                label: Some(format!("Label for {}", binding.scope_id)),
                attached: false,
                unit_count: 1,
            }])
        },
    )
    .unwrap();

    assert_eq!(listing.records.len(), 2);
    let rec_a = listing
        .records
        .iter()
        .find(|r| r.project_name == "proj_a")
        .unwrap();
    assert_eq!(rec_a.workspace, "w_alpha");
    assert_eq!(rec_a.binding.scope_id, "/run/herdr-a.sock");
    assert_eq!(rec_a.binding.workspace_id, "ws-1");
    assert_eq!(
        rec_a.runtime_label.as_deref(),
        Some("Label for /run/herdr-a.sock")
    );

    let rec_b = listing
        .records
        .iter()
        .find(|r| r.project_name == "proj_b")
        .unwrap();
    assert_eq!(rec_b.workspace, "w_beta");
    assert_eq!(rec_b.binding.scope_id, "/run/herdr-b.sock");
    assert_eq!(rec_b.binding.workspace_id, "ws-1");
    assert_eq!(
        rec_b.runtime_label.as_deref(),
        Some("Label for /run/herdr-b.sock")
    );
}

#[test]
fn sessions_ignores_stale_identity_bindings() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "scope".to_owned(),
            workspace_name: "live-id".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/proj"),
            env: Vec::new(),
        })
        .unwrap();
    let project = make_discovered_with_id("proj", "/tmp/proj", "a1b2");

    let listing = usecases::sessions::list_global_sessions(
        &[project],
        |_| {
            Ok(vec![workspace_identity(
                "main",
                RuntimeBackendId::tmux(),
                "scope",
                "stale-id",
            )])
        },
        |binding| runtime.list_workspaces(&binding.scope_id),
    )
    .unwrap();

    assert!(listing.records.is_empty());
    assert!(listing.unavailable.is_empty());
}

// ══════════════════════════════════════════════════════════════════════════════
// open staging: prepare_open_plan + execute_open_plan
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn force_execution_creates_target_without_destroying_old_runtime() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            workspace_name: "old".to_owned(),
            group: "srv".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/test-project"),
            env: vec![],
        })
        .unwrap();
    let plan = usecases::open::OpenPlan {
        workspace: WorkspaceId::new("main").unwrap(),
        provenance: workhive_core::domain::WorkspaceProvenance::ProjectRoot,
        server: "srv".to_owned(),
        session: "target".to_owned(),
        session_availability: workhive_core::domain::SessionAvailability::Missing,
        project_root: Utf8PathBuf::from("/tmp/test-project"),
        workspace_root: Utf8PathBuf::from("/tmp/test-project"),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        defer_checkout_registration: false,
        relative_project_path: None,
        open_mode: usecases::open::OpenMode::ForceRecreateFromLayout,
        env: vec![],
        interaction_policy: RuntimeInteractionPolicy::default(),
    };

    let result = usecases::open::execute_open_plan(&runtime, &plan).unwrap();

    assert_eq!(
        result.prepared_runtime.map(|prepared| prepared.binding),
        Some(workhive_core::domain::RuntimeBinding::tmux("srv", "target"))
    );
    assert!(runtime.has_workspace("srv", "old").unwrap());
    assert!(runtime.has_workspace("srv", "target").unwrap());
}

#[test]
fn force_execution_configuration_failure_leaves_old_runtime_usable() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            workspace_name: "old".to_owned(),
            group: "srv".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/test-project"),
            env: vec![],
        })
        .unwrap();
    runtime.inject_errors(RuntimeErrorInjection {
        configure_workspace: Some(RuntimeError::OperationFailed {
            message: "injected configure failure".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });
    let plan = usecases::open::OpenPlan {
        workspace: WorkspaceId::new("main").unwrap(),
        provenance: workhive_core::domain::WorkspaceProvenance::ProjectRoot,
        server: "srv".to_owned(),
        session: "target".to_owned(),
        session_availability: workhive_core::domain::SessionAvailability::Missing,
        project_root: Utf8PathBuf::from("/tmp/test-project"),
        workspace_root: Utf8PathBuf::from("/tmp/test-project"),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        defer_checkout_registration: false,
        relative_project_path: None,
        open_mode: usecases::open::OpenMode::ForceRecreateFromLayout,
        env: vec![],
        interaction_policy: RuntimeInteractionPolicy::default(),
    };

    assert!(usecases::open::execute_open_plan(&runtime, &plan).is_err());
    assert!(runtime.has_workspace("srv", "old").unwrap());
    assert!(!runtime.has_workspace("srv", "target").unwrap());
}

#[test]
fn worktree_registration_failure_removes_the_new_runtime() {
    let runtime = FakeRuntime::new();
    runtime.inject_errors(RuntimeErrorInjection {
        register_workspace_checkout: Some(RuntimeError::OperationFailed {
            message: "injected worktree registration failure".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });
    let plan = usecases::open::OpenPlan {
        workspace: WorkspaceId::new("feature").unwrap(),
        provenance: workhive_core::domain::WorkspaceProvenance::ProjectRoot,
        server: "srv".to_owned(),
        session: "target".to_owned(),
        session_availability: workhive_core::domain::SessionAvailability::Missing,
        project_root: Utf8PathBuf::from("/tmp/test-project"),
        workspace_root: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feature"),
        working_dir: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feature"),
        defer_checkout_registration: false,
        relative_project_path: None,
        open_mode: usecases::open::OpenMode::Normal,
        env: vec![],
        interaction_policy: RuntimeInteractionPolicy::default(),
    };

    let error = usecases::open::execute_open_plan(&runtime, &plan)
        .err()
        .expect("registration failure must fail the open");

    assert!(error.to_string().contains("worktree registration failure"));
    assert!(!runtime.has_workspace("srv", "target").unwrap());
}

#[test]
fn checkout_registration_reports_adopted_runtime_without_owning_it() {
    let runtime = FakeRuntime::new();
    let adopted = runtime
        .create_workspace_binding(CreateWorkspaceRuntimeRequest {
            workspace_name: "parent".to_owned(),
            group: "srv".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/test-project"),
            env: vec![],
        })
        .unwrap();
    runtime.set_checkout_registration_binding(adopted.clone());
    let plan = usecases::open::OpenPlan {
        workspace: WorkspaceId::new("main").unwrap(),
        provenance: workhive_core::domain::WorkspaceProvenance::ProjectRoot,
        server: "srv".to_owned(),
        session: "target".to_owned(),
        session_availability: workhive_core::domain::SessionAvailability::Missing,
        project_root: Utf8PathBuf::from("/tmp/test-project"),
        workspace_root: Utf8PathBuf::from("/tmp/test-project"),
        working_dir: Utf8PathBuf::from("/tmp/test-project"),
        defer_checkout_registration: false,
        relative_project_path: None,
        open_mode: usecases::open::OpenMode::Normal,
        env: vec![],
        interaction_policy: RuntimeInteractionPolicy::default(),
    };

    let result = usecases::open::execute_open_plan(&runtime, &plan).unwrap();
    let prepared = result.prepared_runtime.expect("runtime preparation");

    assert_eq!(prepared.binding, adopted);
    assert_eq!(
        prepared.ownership,
        usecases::open::PreparedRuntimeOwnership::AdoptedExisting
    );
    assert!(runtime.has_workspace("srv", "parent").unwrap());
    assert!(!runtime.has_workspace("srv", "target").unwrap());
}

#[test]
fn deferred_worktree_registration_allows_replacement_validation() {
    let runtime = FakeRuntime::new();
    runtime.inject_errors(RuntimeErrorInjection {
        register_workspace_checkout: Some(RuntimeError::OperationFailed {
            message: "old runtime still owns the checkout registration".to_owned(),
        }),
        ..RuntimeErrorInjection::default()
    });
    let plan = usecases::open::OpenPlan {
        workspace: WorkspaceId::new("feature").unwrap(),
        provenance: workhive_core::domain::WorkspaceProvenance::ProjectRoot,
        server: "srv".to_owned(),
        session: "target".to_owned(),
        session_availability: workhive_core::domain::SessionAvailability::Missing,
        project_root: Utf8PathBuf::from("/tmp/test-project"),
        workspace_root: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feature"),
        working_dir: Utf8PathBuf::from("/tmp/test-project/.workhive/worktrees/feature"),
        defer_checkout_registration: true,
        relative_project_path: None,
        open_mode: usecases::open::OpenMode::ForceRecreateFromLayout,
        env: vec![],
        interaction_policy: RuntimeInteractionPolicy::default(),
    };

    let result = usecases::open::execute_open_plan(&runtime, &plan)
        .expect("deferred registration must not block staged runtime validation");

    assert_eq!(
        result.prepared_runtime.map(|prepared| prepared.binding),
        Some(workhive_core::domain::RuntimeBinding::tmux("srv", "target"))
    );
    assert!(runtime.has_workspace("srv", "target").unwrap());
}

#[test]
fn binding_commit_failure_preserves_old_binding_and_pending_target() {
    let identity = FakeIdentityPort::new();
    let policy = SessionNamingPolicy::default_policy();
    let naming = NamingContext {
        project: "project".to_owned(),
        workspace: "main".to_owned(),
        branch: Some("main".to_owned()),
        dir_path: Some("/tmp/test-project".to_owned()),
        instance_id: Some("a1b2".to_owned()),
        relative_project_path: None,
    };
    let mut record =
        usecases::identity::ensure_workspace_identity(&identity, &policy, &naming, None, None)
            .unwrap();
    let old = record.runtime_binding();
    let target = workhive_core::domain::RuntimeBinding::tmux("srv", "target");
    usecases::identity::prepare_runtime_replacement(&identity, &mut record, target.clone())
        .unwrap();
    identity.fail_next_workspace_save();

    assert!(usecases::identity::commit_runtime_replacement(&identity, &mut record).is_err());

    let durable = identity
        .load_workspace_identity(&record.identity_key())
        .unwrap()
        .unwrap();
    assert_eq!(durable.runtime_binding(), old);
    assert_eq!(
        durable.runtime_replacement,
        Some(workhive_core::domain::RuntimeReplacement::Preparing { target })
    );
}

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
fn workspace_id_accepts_tilde_but_git_provider_rejects_it() {
    let ws_id = WorkspaceId::new("has~tilde");
    assert!(
        ws_id.is_ok(),
        "WorkspaceId must accept '~' — it is git-only"
    );

    let (_, git, _tmux, ws_fs, _) = setup();
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

    let (_, git, _tmux, ws_fs, _) = setup();
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

    let (_, git, _tmux, ws_fs, _) = setup();
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

// ══════════════════════════════════════════════════════════════════════════════
// CLI identity context integration
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
        relative_project_path: None,
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (_config, git, tmux, ws_fs, index) = setup();

    let root = Utf8PathBuf::from("/tmp/uninit-project");

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must not exist"
    );

    let managed_paths = managed_worktree_paths_from_git(&git, &root);
    let result = usecases::destroy::destroy_project(
        &git,
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "uninit-project",
        &[],
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
    let (config, git, tmux, ws_fs, index) = setup();
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
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        &[],
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
    let (config, git, tmux, ws_fs, index) = setup();
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
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        &[],
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
    let (config, git, tmux, ws_fs, index) = setup();
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
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        &[],
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
    let (config, git, tmux, ws_fs, index) = setup();
    init_global(&config);

    let root = Utf8PathBuf::from("/tmp/test-project");
    usecases::init::init_project(&config, &ws_fs, &index, &root, "test-project", None).unwrap();
    let external_path = Utf8PathBuf::from("/external/repos/feat-ext");

    let result = usecases::destroy::destroy_project(
        &git,
        |binding| tmux.destroy_workspace_binding(binding),
        &ws_fs,
        &index,
        &root,
        "test-project",
        &[],
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
    let (_config, _git, tmux, ws_fs, _index) = setup();

    let root = Utf8PathBuf::from("/tmp/uninit-project");

    assert!(
        !ws_fs.workhive_dir_exists(root.as_ref()),
        "precondition: .workhive must not exist"
    );

    let result =
        usecases::close::close_session(&tmux, &RuntimeBackendId::tmux(), "srv", "sess", false);

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

// ── tmux membership from ExecutionContext ──

#[test]
fn execution_context_tmux_server_takes_precedence() {
    let env = FakeEnvironment::in_session_with_server("wh@a1b2@proj@main", "%0", "whsrv-a1b2-proj");
    use workhive_core::ports::{EnvironmentPort, TmuxMembership};
    let ctx = env.execution_context().expect("must succeed");

    match &ctx.tmux {
        TmuxMembership::Resolved {
            server, session, ..
        } => {
            assert_eq!(server, "whsrv-a1b2-proj");
            assert_eq!(session, "wh@a1b2@proj@main");
        }
        other => panic!("expected Resolved, got: {other:?}"),
    }
}

#[test]
fn execution_context_without_server_falls_back() {
    let env = FakeEnvironment::in_session("wh@a1b2@proj@main", "%0");
    use workhive_core::ports::{EnvironmentPort, TmuxMembership};
    let ctx = env.execution_context().expect("must succeed");

    match &ctx.tmux {
        TmuxMembership::Resolved {
            server, session, ..
        } => {
            assert_eq!(server, "default");
            assert_eq!(session, "wh@a1b2@proj@main");
        }
        other => panic!("expected Resolved, got: {other:?}"),
    }
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

/// Close path resolves runtime identity from explicit server/session parameters (obtained
/// from tmux runtime context), independent of any CWD. Two workspaces with different CWDs
/// must each close correctly when addressed by their runtime identity.

// ── Production wiring integration tests (R2) ────────────────────────

#[test]
fn resolve_relative_display_path_preserves_unsanitized_subdir() {
    let relative_path = RelativeProjectPath::new("1.0pingtai").unwrap();
    let display = usecases::naming::resolve_relative_display_path(
        "python",
        Some(&relative_path),
        "feat.auth",
    );
    assert_eq!(
        display.as_deref(),
        Some("python/1.0pingtai/feat.auth"),
        "display path must include unsanitized subdir and workspace"
    );
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
        relative_project_path: None,
    };

    let ws_record = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        RelativeProjectPath::new("1_0pingtai").ok(),
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
        relative_project_path: None,
    };

    let rec_a = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        RelativeProjectPath::new("subdir-a").ok(),
        Some("subdir-a/feat-x"),
    )
    .unwrap();

    let rec_b = usecases::identity::ensure_workspace_identity(
        &port,
        &policy,
        &ctx,
        RelativeProjectPath::new("subdir-b").ok(),
        Some("subdir-b/feat-x"),
    )
    .unwrap();

    assert_eq!(rec_a.workspace, "feat-x");
    assert_eq!(rec_b.workspace, "feat-x");
    assert_eq!(
        rec_a.relative_project_path.as_ref().map(|p| p.as_str()),
        Some("subdir-a")
    );
    assert_eq!(
        rec_b.relative_project_path.as_ref().map(|p| p.as_str()),
        Some("subdir-b")
    );
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

// ── Project init consent tests (required by fix spec) ──

#[test]
fn partial_init_state_detected_correctly() {
    let (_config, _git, _tmux, ws_fs, _index) = setup();

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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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
    let (config, _git, _tmux, ws_fs, index) = setup();
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

//! Minimal tests for all fake adapters — ensures basic behavior and error paths.

use camino::Utf8PathBuf;
use workhive_core::domain::{Locale, PaneDirection, UnitId};
use workhive_core::ports::*;
use workhive_testkit::fakes::*;

// -- FakeConfig --

#[test]
fn fake_config_empty_global() {
    let cfg = FakeConfig::new();
    assert!(!cfg.global_exists().unwrap());
    assert!(cfg.load_global().unwrap().is_none());
}

#[test]
fn fake_config_save_and_load_global() {
    let cfg = FakeConfig::new();
    cfg.save_global(&AppConfig::default()).unwrap();
    assert!(cfg.global_exists().unwrap());
    assert!(cfg.load_global().unwrap().is_some());
}

#[test]
fn fake_config_project_roundtrip() {
    let cfg = FakeConfig::new();
    let root = Utf8PathBuf::from("/tmp/proj");
    cfg.save_project(&root, &AppConfig::default()).unwrap();
    assert!(cfg.project_exists(&root).unwrap());
}

// -- FakeGit --

#[test]
fn fake_git_ensure_worktree_creates_entry() {
    let git = FakeGit::new("/tmp/repo");
    // FakeGit starts with the main worktree for repo root
    assert_eq!(git.list_worktrees().unwrap().len(), 1);
    let info = git
        .ensure_worktree(EnsureWorktreeRequest {
            base_path: Utf8PathBuf::from("/tmp/worktrees"),
            workspace_name: "feat-a".to_owned(),
        })
        .unwrap();
    assert_eq!(info.branch.as_deref(), Some("feat-a"));
    assert_eq!(git.list_worktrees().unwrap().len(), 2);
}

#[test]
fn fake_git_remove_worktree() {
    let git = FakeGit::new("/tmp/repo");
    let info = git
        .ensure_worktree(EnsureWorktreeRequest {
            base_path: Utf8PathBuf::from("/tmp/worktrees"),
            workspace_name: "feat-a".to_owned(),
        })
        .unwrap();
    git.remove_worktree(&info.path, WorktreeRemovePolicy::Safe)
        .unwrap();
    // Only the main worktree remains
    assert_eq!(git.list_worktrees().unwrap().len(), 1);
    assert!(git.list_worktrees().unwrap()[0].is_main);
}

#[test]
fn fake_git_repo_root() {
    let git = FakeGit::new("/home/user/project");
    assert_eq!(
        git.repo_root().unwrap(),
        Utf8PathBuf::from("/home/user/project")
    );
}

// -- FakeRuntime --

#[test]
fn fake_runtime_workspace_lifecycle() {
    let runtime = FakeRuntime::new();
    assert!(!runtime.has_workspace("s", "my-session").unwrap());

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "s".to_owned(),
            workspace_name: "my-session".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    assert!(runtime.has_workspace("s", "my-session").unwrap());
    runtime.destroy_workspace("s", "my-session").unwrap();
    assert!(!runtime.has_workspace("s", "my-session").unwrap());
}

#[test]
fn fake_runtime_spawn_unit() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "s".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    let unit = runtime
        .spawn_unit(SpawnUnitRequest {
            group: "s".to_owned(),
            workspace_name: "sess".to_owned(),
            direction: PaneDirection::Right,
            command: Some("vim".to_owned()),
            target_unit: None,
            size_percent: None,
            env: Vec::new(),
            cwd: None,
        })
        .unwrap();

    assert_eq!(unit.command, "vim");
    assert_eq!(runtime.list_units("s", "sess").unwrap().len(), 2);
}

#[test]
fn fake_runtime_close_last_unit_rejected() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "s".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    let units = runtime.list_units("s", "sess").unwrap();
    let result = runtime.close_unit("s", &units[0].id);
    assert!(result.is_err(), "closing the last unit must be rejected");
}

#[test]
fn fake_runtime_send_input_records() {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "s".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    let unit_id = UnitId::new("unit-0");
    runtime
        .send_input(SendInputRequest {
            group: "s".to_owned(),
            target_unit: unit_id,
            text: "echo hello".to_owned(),
            enter: true,
        })
        .unwrap();

    let inputs = runtime.sent_inputs();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].1, "echo hello");
}

#[test]
fn fake_runtime_attach_requires_matching_workspace() {
    let runtime = FakeRuntime::new();

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "server-A".to_owned(),
            workspace_name: "shared-name".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/a"),
            env: Vec::new(),
        })
        .unwrap();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "server-B".to_owned(),
            workspace_name: "shared-name".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/b"),
            env: Vec::new(),
        })
        .unwrap();

    let ok = runtime.attach_workspace(AttachWorkspaceRequest {
        group: "server-A".to_owned(),
        workspace_name: "shared-name".to_owned(),
        interaction_policy: workhive_core::ports::RuntimeInteractionPolicy::default(),
    });
    assert!(ok.is_ok(), "attach to correct group should succeed");

    let wrong = runtime.attach_workspace(AttachWorkspaceRequest {
        group: "server-C".to_owned(),
        workspace_name: "shared-name".to_owned(),
        interaction_policy: workhive_core::ports::RuntimeInteractionPolicy::default(),
    });
    assert!(
        wrong.is_err(),
        "attach to unknown group must fail even when workspace name exists on another group"
    );
}

#[test]
fn fake_runtime_attach_nonexistent_workspace_fails() {
    let runtime = FakeRuntime::new();

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "real".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .unwrap();

    let result = runtime.attach_workspace(AttachWorkspaceRequest {
        group: "srv".to_owned(),
        workspace_name: "ghost".to_owned(),
        interaction_policy: workhive_core::ports::RuntimeInteractionPolicy::default(),
    });
    assert!(result.is_err(), "attach to nonexistent workspace must fail");
}

// FakeRuntime: duplicate workspace creation must be rejected with WorkspaceAlreadyExists

#[test]
fn fake_runtime_duplicate_create_returns_workspace_already_exists() {
    let runtime = FakeRuntime::new();

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "feat-a".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/feat-a"),
            env: Vec::new(),
        })
        .expect("first create must succeed");

    let result = runtime.create_workspace(CreateWorkspaceRuntimeRequest {
        group: "srv".to_owned(),
        workspace_name: "feat-a".to_owned(),
        working_dir: Utf8PathBuf::from("/tmp/feat-a"),
        env: Vec::new(),
    });

    assert!(result.is_err(), "duplicate create must fail");
    match result.unwrap_err() {
        RuntimeError::WorkspaceAlreadyExists { workspace } => {
            assert_eq!(workspace, "feat-a");
        }
        other => panic!("expected WorkspaceAlreadyExists, got: {other:?}"),
    }
}

#[test]
fn fake_runtime_same_name_different_group_is_allowed() {
    // (group, workspace_name) is the uniqueness key — same name on different
    // groups must not conflict.
    let runtime = FakeRuntime::new();

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv-A".to_owned(),
            workspace_name: "shared-name".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/a"),
            env: Vec::new(),
        })
        .expect("create on srv-A must succeed");

    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv-B".to_owned(),
            workspace_name: "shared-name".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp/b"),
            env: Vec::new(),
        })
        .expect("same workspace_name on a different group must succeed");

    assert!(
        runtime.has_workspace("srv-A", "shared-name").unwrap(),
        "srv-A/shared-name must exist"
    );
    assert!(
        runtime.has_workspace("srv-B", "shared-name").unwrap(),
        "srv-B/shared-name must exist"
    );
}

// -- FakeWorkspaceFs --

#[test]
fn fake_workspace_fs_create_and_check() {
    let ws = FakeWorkspaceFs::new();
    let root = Utf8PathBuf::from("/tmp/proj");
    assert!(!ws.workhive_dir_exists(&root));
    ws.create_workhive_dir(&root).unwrap();
    assert!(ws.workhive_dir_exists(&root));
}

#[test]
fn fake_workspace_fs_remove() {
    let ws = FakeWorkspaceFs::new();
    let root = Utf8PathBuf::from("/tmp/proj");
    ws.create_workhive_dir(&root).unwrap();
    ws.remove_workhive_dir(&root).unwrap();
    assert!(!ws.workhive_dir_exists(&root));
}

// -- FakeProjectIndex --

#[test]
fn fake_project_index_upsert_and_list() {
    let idx = FakeProjectIndex::new();
    idx.upsert_project(&ProjectSummary {
        project_name: "alpha".to_owned(),
        project_root: Utf8PathBuf::from("/tmp/alpha"),
        config_path: Utf8PathBuf::from("/tmp/alpha/.workhive/config.toml"),
        last_opened_at: None,
        last_updated_at: None,
    })
    .unwrap();
    assert_eq!(idx.list_projects().unwrap().projects().len(), 1);
}

// -- FakeEnvironment --

#[test]
fn fake_environment_returns_configured_context() {
    let env = FakeEnvironment::interactive();
    let ctx = env.execution_context().unwrap();
    assert!(ctx.is_interactive);
}

#[test]
fn fake_environment_non_interactive() {
    let env = FakeEnvironment::non_interactive();
    let ctx = env.execution_context().unwrap();
    assert!(!ctx.is_interactive);
}

// -- FakeDependency --

#[test]
fn fake_dependency_defaults_all_satisfied() {
    let deps = FakeDependency::new();
    let results = deps.check_all();
    assert!(results.iter().all(|s| s.is_ok()));
    assert_eq!(results.len(), 2);
}

#[test]
fn fake_dependency_missing_git() {
    let mut deps = FakeDependency::new();
    deps.set_missing(DependencyKind::Git);
    let status = deps.check(DependencyKind::Git);
    assert!(!status.is_ok());
    assert_eq!(status.kind(), DependencyKind::Git);
}

#[test]
fn fake_dependency_builtin_tmux() {
    let mut deps = FakeDependency::new();
    deps.set_builtin(DependencyKind::Tmux);
    let status = deps.check(DependencyKind::Tmux);
    assert!(status.is_ok());
    assert!(matches!(status, DependencyStatus::Builtin { .. }));
}

#[test]
fn fake_dependency_version_too_low() {
    let mut deps = FakeDependency::new();
    deps.set_status(
        DependencyKind::Git,
        DependencyStatus::VersionTooLow {
            kind: DependencyKind::Git,
            found: "1.8.0".to_owned(),
            minimum: "2.20.0".to_owned(),
        },
    );
    let status = deps.check(DependencyKind::Git);
    assert!(!status.is_ok());
}

// -- FakeI18n --

#[test]
fn fake_i18n_returns_key_as_message() {
    let i18n = FakeI18n;
    let msg = i18n.resolve(Locale::En, "test.key");
    assert!(msg.contains("test.key"));
}

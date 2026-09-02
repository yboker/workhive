use super::*;

fn try_parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(args.iter().copied())
}

// ── Resource grouping: workspace ──

#[test]
fn parse_workspace_open_minimal() {
    let cli = try_parse(&["workhive", "workspace", "open"]).expect("workspace open parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Open {
                    workspace,
                    no_attach,
                    force,
                    mode,
                    init,
                    attach_existing_branch,
                },
        } => {
            assert!(workspace.is_none());
            assert!(!no_attach);
            assert!(!force);
            assert_eq!(mode, "manual");
            assert!(!init);
            assert!(!attach_existing_branch);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_open_with_name_and_flags() {
    let cli = try_parse(&[
        "workhive",
        "workspace",
        "open",
        "feat-a",
        "--no-attach",
        "--force",
        "--mode",
        "primary-assisted",
        "--init",
    ])
    .expect("workspace open with flags parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Open {
                    workspace,
                    no_attach,
                    force,
                    mode,
                    init,
                    ..
                },
        } => {
            assert_eq!(workspace.as_deref(), Some("feat-a"));
            assert!(no_attach);
            assert!(force);
            assert_eq!(mode, "primary-assisted");
            assert!(init);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_list_view_json() {
    let cli = try_parse(&["workhive", "workspace", "list", "--view", "json"])
        .expect("workspace list parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::List { view },
        } => assert_eq!(view, ListView::Json),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_object_scoped_ls_aliases() {
    let cli = try_parse(&["workhive", "project", "ls"]).expect("project ls parses");
    assert!(matches!(
        cli.command,
        Commands::Project {
            action: ProjectAction::List { .. }
        }
    ));

    let cli = try_parse(&["workhive", "workspace", "ls"]).expect("workspace ls parses");
    assert!(matches!(
        cli.command,
        Commands::Workspace {
            action: WorkspaceAction::List { .. }
        }
    ));

    let cli = try_parse(&["workhive", "session", "ls"]).expect("session ls parses");
    assert!(matches!(
        cli.command,
        Commands::Session {
            action: SessionAction::List { .. }
        }
    ));

    let cli = try_parse(&["workhive", "pane", "ls"]).expect("pane ls parses");
    assert!(matches!(
        cli.command,
        Commands::Pane {
            action: PaneAction::List
        }
    ));
}

#[test]
fn top_level_ls_remains_rejected() {
    let err = try_parse(&["workhive", "ls"]).expect_err("top-level ls must not parse");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "top-level ls must remain outside the command surface"
    );
}

#[test]
fn parse_workspace_attach_optional_name() {
    let cli = try_parse(&["workhive", "workspace", "attach"]).expect("workspace attach parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Attach { workspace },
        } => assert!(workspace.is_none()),
        other => panic!("unexpected command: {other:?}"),
    }

    let cli = try_parse(&["workhive", "workspace", "attach", "feat-b"])
        .expect("workspace attach with name parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Attach { workspace },
        } => assert_eq!(workspace.as_deref(), Some("feat-b")),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_close() {
    let cli = try_parse(&["workhive", "workspace", "close", "--kill", "--force"])
        .expect("workspace close parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Close { kill, force },
        } => {
            assert!(kill);
            assert!(force);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_remove() {
    let cli = try_parse(&[
        "workhive",
        "workspace",
        "remove",
        "feat-c",
        "--delete-branch",
    ])
    .expect("workspace remove parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Remove {
                    workspace,
                    delete_branch,
                    keep_branch,
                    force,
                },
        } => {
            assert_eq!(workspace, "feat-c");
            assert!(delete_branch);
            assert!(!keep_branch);
            assert!(!force);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_remove_keep_branch() {
    let cli = try_parse(&["workhive", "workspace", "remove", "feat-c", "--keep-branch"])
        .expect("workspace remove --keep-branch parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Remove {
                    workspace,
                    delete_branch,
                    keep_branch,
                    ..
                },
        } => {
            assert_eq!(workspace, "feat-c");
            assert!(!delete_branch);
            assert!(keep_branch, "--keep-branch must parse into keep_branch");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_destroy() {
    let cli = try_parse(&["workhive", "workspace", "destroy", "--force"])
        .expect("workspace destroy parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Destroy {
                    force,
                    delete_branch,
                    keep_branch,
                },
        } => {
            assert!(force);
            assert!(!delete_branch);
            assert!(!keep_branch);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_open_attach_existing_branch() {
    let cli = try_parse(&[
        "workhive",
        "workspace",
        "open",
        "demo01",
        "--attach-existing-branch",
    ])
    .expect("workspace open --attach-existing-branch parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Open {
                    workspace,
                    attach_existing_branch,
                    ..
                },
        } => {
            assert_eq!(workspace.as_deref(), Some("demo01"));
            assert!(
                attach_existing_branch,
                "--attach-existing-branch must parse into attach_existing_branch"
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

// ── Resource grouping: project ──

#[test]
fn parse_project_list() {
    let cli = try_parse(&["workhive", "project", "list"]).expect("project list parses");
    match cli.command {
        Commands::Project {
            action: ProjectAction::List { view },
        } => assert_eq!(view, ListView::Auto),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_project_reindex() {
    let cli = try_parse(&[
        "workhive",
        "project",
        "reindex",
        "--root",
        "/tmp/a",
        "--root",
        "/tmp/b",
        "--clear-first",
        "--dry-run",
        "--force",
    ])
    .expect("project reindex parses");
    match cli.command {
        Commands::Project {
            action:
                ProjectAction::Reindex {
                    root,
                    config_roots,
                    clear_first,
                    dry_run,
                    force,
                },
        } => {
            assert_eq!(root, vec!["/tmp/a".to_owned(), "/tmp/b".to_owned()]);
            assert!(!config_roots);
            assert!(clear_first);
            assert!(dry_run);
            assert!(force);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `project roots edit` is the interactive trusted-root editor.
/// It takes no positional arguments and no flags;
/// the parser must accept the bare invocation.
#[test]
fn parse_project_roots_edit() {
    let cli =
        try_parse(&["workhive", "project", "roots", "edit"]).expect("project roots edit parses");
    match cli.command {
        Commands::Project {
            action:
                ProjectAction::Roots {
                    action: ProjectRootsAction::Edit,
                },
        } => {}
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `project roots remove --force` is required for non-interactive flows
/// where the trusted root has indexed projects beneath it.
/// The parser must surface the boolean.
#[test]
fn parse_project_roots_remove_with_force() {
    let cli = try_parse(&[
        "workhive", "project", "roots", "remove", "/tmp/foo", "--force",
    ])
    .expect("project roots remove --force parses");
    match cli.command {
        Commands::Project {
            action:
                ProjectAction::Roots {
                    action: ProjectRootsAction::Remove { path, force },
                },
        } => {
            assert_eq!(path, "/tmp/foo");
            assert!(force, "--force must parse to `true`");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `project roots remove` without `--force` parses with `force = false`
/// so the runtime can branch on it for the affected-projects guard.
#[test]
fn parse_project_roots_remove_default_force_is_false() {
    let cli = try_parse(&["workhive", "project", "roots", "remove", "/tmp/foo"])
        .expect("project roots remove parses without --force");
    match cli.command {
        Commands::Project {
            action:
                ProjectAction::Roots {
                    action: ProjectRootsAction::Remove { path, force },
                },
        } => {
            assert_eq!(path, "/tmp/foo");
            assert!(!force, "default --force must be `false`");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

// ── Resource grouping: session ──

#[test]
fn parse_session_list() {
    let cli = try_parse(&["workhive", "session", "list"]).expect("session list parses");
    assert!(matches!(
        cli.command,
        Commands::Session {
            action: SessionAction::List {
                view: ListView::Auto
            },
        }
    ));
}

#[test]
fn parse_session_list_view_json() {
    let cli = try_parse(&["workhive", "session", "list", "--view", "json"])
        .expect("session list --view json parses");
    match cli.command {
        Commands::Session {
            action: SessionAction::List { view },
        } => assert_eq!(view, ListView::Json),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_session_attach_no_target() {
    let cli = try_parse(&["workhive", "session", "attach"]).expect("session attach (bare) parses");
    match cli.command {
        Commands::Session {
            action: SessionAction::Attach { target },
        } => assert!(target.is_none(), "no target means interactive selection"),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_session_attach_with_target() {
    let cli = try_parse(&["workhive", "session", "attach", "myproj/feature-x"])
        .expect("session attach with target parses");
    match cli.command {
        Commands::Session {
            action: SessionAction::Attach { target },
        } => assert_eq!(target.as_deref(), Some("myproj/feature-x")),
        other => panic!("unexpected command: {other:?}"),
    }
}

// ── Resource grouping: worktree lives only under `workspace` ──

/// Worktree management is exposed only under `workhive workspace worktree`.
/// Top-level `worktree` is not a valid subcommand, so bare `worktree` is
/// rejected as an unknown subcommand and falls through to clap's standard
/// help guidance.
#[test]
fn top_level_worktree_bare_is_rejected() {
    let err = try_parse(&["workhive", "worktree"])
        .expect_err("top-level `worktree` must be rejected as an unknown subcommand");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "expected unknown subcommand for `worktree`, got: {err:?}"
    );
}

#[test]
fn top_level_worktree_list_is_rejected() {
    let err = try_parse(&["workhive", "worktree", "list"])
        .expect_err("top-level `worktree list` must be rejected as an unknown subcommand");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "expected unknown subcommand for `worktree list`, got: {err:?}"
    );
}

#[test]
fn top_level_worktree_remove_is_rejected() {
    let err = try_parse(&["workhive", "worktree", "remove", "demo"])
        .expect_err("top-level `worktree remove` must be rejected as an unknown subcommand");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "expected unknown subcommand for `worktree remove`, got: {err:?}"
    );
}

/// `wt` alias is intentionally NOT exposed (governance §12.1: avoid
/// inflating the command surface with redundant aliases).
#[test]
fn wt_alias_remains_rejected() {
    let err = try_parse(&["workhive", "wt"]).expect_err("`wt` alias must remain undefined");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "expected unknown subcommand for `wt`, got: {err:?}"
    );
}

// ── Resource grouping: workspace worktree (canonical path) ──

#[test]
fn parse_workspace_worktree_no_action_means_default() {
    let cli = try_parse(&["workhive", "workspace", "worktree"])
        .expect("bare `workhive workspace worktree` parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Worktree { action },
        } => assert!(action.is_none()),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_worktree_list() {
    let cli = try_parse(&["workhive", "workspace", "worktree", "list"])
        .expect("workspace worktree list parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Worktree { action },
        } => assert!(matches!(action, Some(WorktreeAction::List))),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_worktree_ls_alias() {
    let cli = try_parse(&["workhive", "workspace", "worktree", "ls"])
        .expect("workspace worktree ls parses");
    match cli.command {
        Commands::Workspace {
            action: WorkspaceAction::Worktree { action },
        } => assert!(matches!(action, Some(WorktreeAction::List))),
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_worktree_remove_with_name() {
    let cli = try_parse(&["workhive", "workspace", "worktree", "remove", "feat-x"])
        .expect("workspace worktree remove parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Worktree {
                    action:
                        Some(WorktreeAction::Remove {
                            worktree,
                            force,
                            delete_branch,
                            keep_branch,
                        }),
                },
        } => {
            assert_eq!(worktree.as_deref(), Some("feat-x"));
            assert!(!force);
            assert!(!delete_branch);
            assert!(!keep_branch);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_worktree_remove_with_force_and_delete_branch() {
    let cli = try_parse(&[
        "workhive",
        "workspace",
        "worktree",
        "remove",
        "feat-x",
        "--force",
        "--delete-branch",
    ])
    .expect("workspace worktree remove --force --delete-branch parses");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Worktree {
                    action:
                        Some(WorktreeAction::Remove {
                            worktree,
                            force,
                            delete_branch,
                            keep_branch,
                        }),
                },
        } => {
            assert_eq!(worktree.as_deref(), Some("feat-x"));
            assert!(force);
            assert!(delete_branch);
            assert!(!keep_branch);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

#[test]
fn parse_workspace_worktree_remove_without_name_parses_for_interactive() {
    let cli = try_parse(&["workhive", "workspace", "worktree", "remove"])
        .expect("workspace worktree remove without name parses (interactive in TTY)");
    match cli.command {
        Commands::Workspace {
            action:
                WorkspaceAction::Worktree {
                    action:
                        Some(WorktreeAction::Remove {
                            worktree,
                            force,
                            delete_branch,
                            keep_branch,
                        }),
                },
        } => {
            assert!(worktree.is_none());
            assert!(!force);
            assert!(!delete_branch);
            assert!(!keep_branch);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// Worktree management lives exclusively under the canonical
/// `workhive workspace worktree` path. No top-level `worktree` subcommand
/// exists in the command tree, so none appears in `--help`.
#[test]
fn top_level_worktree_absent_from_help() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    assert!(
        cmd.find_subcommand("worktree").is_none(),
        "top-level worktree must not exist in the command tree"
    );
}

#[test]
fn workspace_worktree_subcommand_is_visible_in_help() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let workspace = cmd
        .find_subcommand("workspace")
        .expect("workspace subcommand exists");
    let worktree = workspace
        .find_subcommand("worktree")
        .expect("workspace worktree subcommand exists");
    assert!(
        !worktree.is_hide_set(),
        "canonical `workspace worktree` must be visible in --help"
    );
}

// ── System commands stay top-level ──

#[test]
fn init_and_config_remain_top_level() {
    let cli = try_parse(&["workhive", "init"]).expect("init parses");
    assert!(matches!(cli.command, Commands::Init { .. }));

    let cli = try_parse(&["workhive", "config", "show"]).expect("config show parses");
    assert!(matches!(cli.command, Commands::Config { .. }));
}

#[test]
fn doctor_remains_top_level() {
    let cli = try_parse(&["workhive", "doctor"]).expect("doctor parses");
    assert!(matches!(cli.command, Commands::Doctor));
}

// ── Internal commands are hidden but still routable ──

#[test]
fn parse_internal_snapshot_notify() {
    let cli = try_parse(&[
        "workhive",
        "internal",
        "snapshot",
        "notify",
        "--project-root",
        "/tmp/p",
        "--server",
        "srv",
        "--session",
        "sess",
        "--state-key",
        "k",
        "--snapshot-event",
        "pane-exited",
    ])
    .expect("internal snapshot notify parses");
    match cli.command {
        Commands::Internal {
            action:
                InternalAction::Snapshot {
                    action:
                        InternalSnapshotAction::Notify {
                            project_root,
                            server,
                            session,
                            state_key,
                            snapshot_event,
                        },
                },
        } => {
            assert_eq!(project_root, "/tmp/p");
            assert_eq!(server, "srv");
            assert_eq!(session, "sess");
            assert_eq!(state_key, "k");
            assert_eq!(snapshot_event, "pane-exited");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `workhive internal pane-supervisor` is the hidden binding the
/// CLI uses to wrap a tracked tool. The whole instrumentation
/// pipeline depends on this clap shape parsing exactly once: a
/// stray flag or a swallowed `argv` entry would result in users
/// seeing the wrong command, or worse, the supervisor running
/// the wrong child. The contract:
///
/// 1. `--runtime-root`, `--group`, `--workspace`, `--unit` are
///    required and parsed as plain strings (no `value_enum`
///    coercion that could re-interpret values).
/// 2. `--tool` and `--program-path` are optional.
/// 3. Everything after `--` lands verbatim in `argv`, in order,
///    including hyphen-prefixed flags (so the wrapped tool can
///    receive `--flag value` without clap stealing it).
#[test]
fn parse_internal_pane_supervisor_full_argv() {
    let cli = try_parse(&[
        "workhive",
        "internal",
        "pane-supervisor",
        "--project-root",
        "/proj",
        "--runtime-root",
        "/tmp/r",
        "--group",
        "g",
        "--workspace",
        "ws",
        "--unit",
        "%2",
        "--tool",
        "codex",
        "--program-path",
        "/usr/bin/codex",
        "--",
        "codex",
        "--flag",
        "value",
        "--another",
        "with spaces",
    ])
    .expect("internal pane-supervisor parses with typed argv");
    match cli.command {
        Commands::Internal {
            action:
                InternalAction::PaneSupervisor {
                    project_root,
                    runtime_root,
                    group,
                    workspace,
                    unit,
                    tool,
                    program_path,
                    argv,
                },
        } => {
            assert_eq!(project_root.as_deref(), Some("/proj"));
            assert_eq!(runtime_root, "/tmp/r");
            assert_eq!(group, "g");
            assert_eq!(workspace, "ws");
            assert_eq!(unit, "%2");
            assert_eq!(tool.as_deref(), Some("codex"));
            assert_eq!(program_path.as_deref(), Some("/usr/bin/codex"));
            assert_eq!(
                argv,
                vec![
                    "codex".to_owned(),
                    "--flag".to_owned(),
                    "value".to_owned(),
                    "--another".to_owned(),
                    "with spaces".to_owned(),
                ],
                "argv must preserve order and hyphen-prefixed values verbatim"
            );
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `--project-root` is optional so legacy snapshot invocations
/// without the flag still parse. The handler treats absence as
/// "global-only gate resolution"; absence is not a `clap` error.
#[test]
fn parse_internal_pane_supervisor_project_root_defaults_to_none() {
    let cli = try_parse(&[
        "workhive",
        "internal",
        "pane-supervisor",
        "--runtime-root",
        "/tmp/r",
        "--group",
        "g",
        "--workspace",
        "ws",
        "--unit",
        "%2",
        "--",
        "codex",
    ])
    .expect("internal pane-supervisor parses without --project-root");
    match cli.command {
        Commands::Internal {
            action: InternalAction::PaneSupervisor { project_root, .. },
        } => assert!(
            project_root.is_none(),
            "project_root defaults to None when omitted"
        ),
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `--tool` and `--program-path` are optional. When they are
/// omitted the parser MUST still produce a valid command, with
/// `argv` carrying the wrapped program in slot 0.
#[test]
fn parse_internal_pane_supervisor_optional_flags_default_to_none() {
    let cli = try_parse(&[
        "workhive",
        "internal",
        "pane-supervisor",
        "--runtime-root",
        "/tmp/r",
        "--group",
        "g",
        "--workspace",
        "ws",
        "--unit",
        "%2",
        "--",
        "less",
        "README.md",
    ])
    .expect("internal pane-supervisor parses without optional flags");
    match cli.command {
        Commands::Internal {
            action:
                InternalAction::PaneSupervisor {
                    tool,
                    program_path,
                    argv,
                    ..
                },
        } => {
            assert!(tool.is_none(), "tool defaults to None when omitted");
            assert!(
                program_path.is_none(),
                "program_path defaults to None when omitted"
            );
            assert_eq!(argv, vec!["less".to_owned(), "README.md".to_owned()]);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// Empty `argv` is invalid: there is nothing for the supervisor
/// to wrap. clap MUST reject this at parse time so we never reach
/// the runner with no child to spawn.
#[test]
fn parse_internal_pane_supervisor_rejects_empty_argv() {
    let err = try_parse(&[
        "workhive",
        "internal",
        "pane-supervisor",
        "--runtime-root",
        "/tmp/r",
        "--group",
        "g",
        "--workspace",
        "ws",
        "--unit",
        "%2",
        "--",
    ])
    .expect_err("internal pane-supervisor must reject empty argv");
    assert!(
        matches!(
            err.kind(),
            clap::error::ErrorKind::TooFewValues
                | clap::error::ErrorKind::MissingRequiredArgument
                | clap::error::ErrorKind::InvalidValue
                | clap::error::ErrorKind::WrongNumberOfValues
        ),
        "empty argv should be a parse-time argument error: {err:?}",
    );
}

/// Missing required flags MUST surface as a parse error instead
/// of silently falling back to defaults.
#[test]
fn parse_internal_pane_supervisor_rejects_missing_required_flags() {
    let err = try_parse(&[
        "workhive",
        "internal",
        "pane-supervisor",
        "--group",
        "g",
        "--workspace",
        "ws",
        "--unit",
        "%2",
        "--",
        "codex",
    ])
    .expect_err("missing --runtime-root must not parse");
    assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
}

/// `internal` and its sub-actions are hidden but must be
/// reachable. They MUST NOT appear in the public help text so
/// they don't pollute completion / `--help`. We assert two
/// things:
///
/// 1. The `internal` subcommand is present in the command tree
///    (otherwise our parse tests above would never have run).
/// 2. It is marked `hide(true)` so it doesn't show up in the
///    user-facing help.
#[test]
fn internal_command_is_hidden_from_user_help() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let internal = cmd
        .find_subcommand("internal")
        .expect("internal subcommand must exist");
    assert!(
        internal.is_hide_set(),
        "internal subcommand must be hidden from --help / completions"
    );
}

/// `clap`'s `debug_assert()` walks the entire command tree at
/// build time and panics on any internal misconfiguration —
/// e.g. `trailing_var_arg` placed on a non-last positional, or a
/// flag that conflicts with itself. Calling it once in tests
/// guarantees that any future addition to the command surface
/// (especially the hidden `internal` subtree, which users will
/// not exercise interactively) cannot regress the parser
/// silently.
#[test]
fn command_tree_passes_clap_debug_assert() {
    use clap::CommandFactory;
    Cli::command().debug_assert();
}

#[test]
fn parse_internal_snapshot_monitor() {
    let cli = try_parse(&[
        "workhive",
        "internal",
        "snapshot",
        "monitor",
        "--project-root",
        "/tmp/p",
        "--server",
        "srv",
        "--session",
        "sess",
        "--state-key",
        "k",
    ])
    .expect("internal snapshot monitor parses");
    assert!(matches!(
        cli.command,
        Commands::Internal {
            action: InternalAction::Snapshot {
                action: InternalSnapshotAction::Monitor { .. }
            }
        }
    ));
}

// ── Pane split direction and send Enter semantics ──

/// `pane split` without `--direction` must default to `down`. This is a
/// typed `clap::ValueEnum`, so the default is validated at parse time.
#[test]
fn parse_pane_split_defaults_to_down() {
    let cli = try_parse(&["workhive", "pane", "split"]).expect("pane split parses");
    match cli.command {
        Commands::Pane {
            action:
                PaneAction::Split {
                    direction,
                    cmd,
                    force,
                },
        } => {
            assert_eq!(direction, PaneSplitDirectionArg::Down);
            assert!(cmd.is_none());
            assert!(!force);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// All four cardinal directions must parse to the matching enum variant.
/// This locks in the contract that `pane split` accepts exactly
/// right/down/left/up with no String fallback.
#[test]
fn parse_pane_split_accepts_all_four_directions() {
    let cases: &[(&str, PaneSplitDirectionArg)] = &[
        ("right", PaneSplitDirectionArg::Right),
        ("down", PaneSplitDirectionArg::Down),
        ("left", PaneSplitDirectionArg::Left),
        ("up", PaneSplitDirectionArg::Up),
    ];
    for (raw, expected) in cases {
        let cli = try_parse(&["workhive", "pane", "split", "--direction", raw])
            .unwrap_or_else(|err| panic!("direction {raw} must parse: {err}"));
        match cli.command {
            Commands::Pane {
                action: PaneAction::Split { direction, .. },
            } => assert_eq!(
                direction, *expected,
                "direction token {raw} must map to {expected:?}",
            ),
            other => panic!("unexpected command for {raw}: {other:?}"),
        }
    }
}

/// Invalid direction tokens must be rejected at parse time — no String
/// fallback to `right` or anything else.
#[test]
fn parse_pane_split_rejects_invalid_direction() {
    let err = try_parse(&["workhive", "pane", "split", "--direction", "diagonal"])
        .expect_err("invalid direction must not parse");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidValue,
        "invalid direction must be surfaced as InvalidValue: {err:?}",
    );
}

/// The clap→core enum adapter must preserve all four variants.
#[test]
fn pane_split_direction_arg_maps_to_core_pane_direction() {
    use workhive_core::domain::PaneDirection;
    assert_eq!(
        PaneDirection::from(PaneSplitDirectionArg::Right),
        PaneDirection::Right
    );
    assert_eq!(
        PaneDirection::from(PaneSplitDirectionArg::Down),
        PaneDirection::Down
    );
    assert_eq!(
        PaneDirection::from(PaneSplitDirectionArg::Left),
        PaneDirection::Left
    );
    assert_eq!(
        PaneDirection::from(PaneSplitDirectionArg::Up),
        PaneDirection::Up
    );
}

/// `pane send` without `--no-enter` must parse with `no_enter == false`,
/// so the handler submits a trailing `Enter` key event by default.
#[test]
fn parse_pane_send_defaults_to_submitting_enter() {
    let cli = try_parse(&["workhive", "pane", "send", "0", "hello"]).expect("pane send parses");
    match cli.command {
        Commands::Pane {
            action:
                PaneAction::Send {
                    pane,
                    text,
                    no_enter,
                    force,
                },
        } => {
            assert_eq!(pane, "0");
            assert_eq!(text, "hello");
            assert!(!no_enter, "default must submit Enter");
            assert!(!force);
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `pane show` with no argument keeps its original behaviour: the
/// optional target parses as `None` so the handler shows the current
/// pane.
#[test]
fn parse_pane_show_without_target_keeps_current_pane_behavior() {
    let cli = try_parse(&["workhive", "pane", "show"]).expect("pane show parses");
    match cli.command {
        Commands::Pane {
            action: PaneAction::Show { pane },
        } => assert_eq!(
            pane, None,
            "no-arg `pane show` must keep current-pane behavior"
        ),
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `pane show 0` must parse with the numeric target captured verbatim
/// so the handler can resolve it against the `pane list` snapshot.
#[test]
fn parse_pane_show_accepts_numeric_target() {
    let cli = try_parse(&["workhive", "pane", "show", "0"]).expect("pane show 0 parses");
    match cli.command {
        Commands::Pane {
            action: PaneAction::Show { pane },
        } => assert_eq!(pane.as_deref(), Some("0")),
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `pane show %2` must parse with the tmux target id captured verbatim
/// so the handler resolves it through the shared pane-target resolver.
#[test]
fn parse_pane_show_accepts_target_id() {
    let cli = try_parse(&["workhive", "pane", "show", "%2"]).expect("pane show %2 parses");
    match cli.command {
        Commands::Pane {
            action: PaneAction::Show { pane },
        } => assert_eq!(pane.as_deref(), Some("%2")),
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `pane approve-hooks <pane>` must still parse so any scripts
/// or test harnesses that invoke the provider-specific guide
/// command directly keep working. The subcommand is hidden from
/// `pane --help` (see `pane_approve_hooks_is_hidden_from_pane_help`
/// below) but invokable as a compatibility alias.
#[test]
fn parse_pane_approve_hooks_compat_alias_still_invokable() {
    let cli =
        try_parse(&["workhive", "pane", "approve-hooks", "%3"]).expect("alias must still parse");
    match cli.command {
        Commands::Pane {
            action: PaneAction::ApproveHooks { pane },
        } => assert_eq!(pane, "%3"),
        other => panic!("unexpected command: {other:?}"),
    }
}

/// `approve-hooks` is provider-specific and must not appear as a
/// regular command in `workhive pane --help`. The hidden clap
/// node keeps legacy invocations parseable while suppressing the
/// alias from the parent's `Commands:` block.
#[test]
fn pane_approve_hooks_is_hidden_from_pane_help() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let pane = cmd.find_subcommand("pane").expect("pane subcommand exists");
    let approve = pane
        .find_subcommand("approve-hooks")
        .expect("approve-hooks alias must remain defined for compatibility");
    assert!(
        approve.is_hide_set(),
        "approve-hooks must be hidden from `pane --help`",
    );
}

/// `pane send --no-enter` must flip the flag without swallowing the
/// literal text argument.
#[test]
fn parse_pane_send_no_enter_flag_is_honored() {
    let cli = try_parse(&["workhive", "pane", "send", "0", "cmd", "--no-enter"])
        .expect("pane send --no-enter parses");
    match cli.command {
        Commands::Pane {
            action:
                PaneAction::Send {
                    pane,
                    text,
                    no_enter,
                    ..
                },
        } => {
            assert_eq!(pane, "0");
            assert_eq!(text, "cmd");
            assert!(no_enter, "--no-enter must disable Enter submission");
        }
        other => panic!("unexpected command: {other:?}"),
    }
}

// ── Removed top-level commands must be rejected at parse time ──

fn assert_unknown_subcommand(args: &[&str]) {
    let err = try_parse(args)
        .map(|cli| format!("{cli:?}"))
        .expect_err("removed command must not parse");
    assert_eq!(
        err.kind(),
        clap::error::ErrorKind::InvalidSubcommand,
        "expected InvalidSubcommand for {args:?}, got: {err:?}"
    );
}

#[test]
fn removed_top_level_open_attach_close() {
    assert_unknown_subcommand(&["workhive", "open"]);
    assert_unknown_subcommand(&["workhive", "attach"]);
    assert_unknown_subcommand(&["workhive", "close"]);
}

#[test]
fn removed_top_level_list_and_destroy() {
    assert_unknown_subcommand(&["workhive", "list"]);
    assert_unknown_subcommand(&["workhive", "ls"]);
    assert_unknown_subcommand(&["workhive", "destroy"]);
    assert_unknown_subcommand(&["workhive", "worktree", "list"]);
    assert_unknown_subcommand(&["workhive", "wt", "list"]);
}

#[test]
fn removed_top_level_projects_sessions_reindex_snapshot() {
    assert_unknown_subcommand(&["workhive", "projects"]);
    assert_unknown_subcommand(&["workhive", "sessions"]);
    assert_unknown_subcommand(&["workhive", "reindex"]);
    assert_unknown_subcommand(&["workhive", "snapshot", "notify"]);
}

// ── Localized branch-cleanup flag help coverage ──
//
// These tests pin that the branch-provenance flags appear in the
// rendered help for both locales with the corrected semantics:
// `--delete-branch` authorizes deleting user-owned (pre-existing /
// unknown) branches, and `--keep-branch` retains Workhive-created
// branches and outranks `--delete-branch`. EN/zh-CN key parity is
// enforced separately by the infra i18n catalog parity test.

/// Collapse all runs of whitespace so substring assertions are not
/// broken by clap's line wrapping inside the rendered help block.
fn normalize_help_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn render_help_en(path: &[&str]) -> String {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    for segment in path {
        cmd = cmd
            .find_subcommand(segment)
            .cloned()
            .unwrap_or_else(|| panic!("missing subcommand {segment:?} for path {path:?}"));
    }
    normalize_help_ws(&cmd.render_long_help().to_string())
}

fn render_help_zh(path: &[&str]) -> String {
    use clap::CommandFactory;
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let mut cmd = crate::localize::localize_command(Cli::command(), Locale::ZhCn, &i18n);
    for segment in path {
        cmd = cmd
            .find_subcommand(segment)
            .cloned()
            .unwrap_or_else(|| panic!("missing subcommand {segment:?} for path {path:?}"));
    }
    normalize_help_ws(&cmd.render_long_help().to_string())
}

#[test]
fn help_workspace_open_documents_attach_existing_branch_en() {
    let help = render_help_en(&["workspace", "open"]);
    assert!(
        help.contains("--attach-existing-branch"),
        "open --help must list --attach-existing-branch:\n{help}"
    );
    assert!(
        help.contains("pre-existing local branch"),
        "open --help must explain attaching a pre-existing branch:\n{help}"
    );
}

#[test]
fn help_workspace_open_documents_attach_existing_branch_zh() {
    let help = render_help_zh(&["workspace", "open"]);
    assert!(
        help.contains("--attach-existing-branch"),
        "zh open --help must keep the flag token:\n{help}"
    );
    assert!(
        help.contains("挂载") && help.contains("已有"),
        "zh open --help must localize the attach-existing description:\n{help}"
    );
}

#[test]
fn help_workspace_remove_documents_branch_flags_en() {
    let help = render_help_en(&["workspace", "remove"]);
    assert!(
        help.contains("--delete-branch"),
        "remove --help must list --delete-branch:\n{help}"
    );
    assert!(
        help.contains("--keep-branch"),
        "remove --help must list --keep-branch:\n{help}"
    );
    assert!(
        help.contains("Authorize deleting") && help.contains("user-owned"),
        "remove --help --delete-branch must use the corrected semantics:\n{help}"
    );
    assert!(
        help.contains("Takes precedence over"),
        "remove --help --keep-branch must state precedence over --delete-branch:\n{help}"
    );
}

#[test]
fn help_workspace_remove_documents_branch_flags_zh() {
    let help = render_help_zh(&["workspace", "remove"]);
    assert!(
        help.contains("--delete-branch") && help.contains("--keep-branch"),
        "zh remove --help must keep both flag tokens:\n{help}"
    );
    assert!(
        help.contains("授权删除"),
        "zh remove --help --delete-branch must use corrected semantics:\n{help}"
    );
    assert!(
        help.contains("优先级高于"),
        "zh remove --help --keep-branch must state precedence:\n{help}"
    );
}

#[test]
fn help_workspace_destroy_documents_branch_flags_en() {
    let help = render_help_en(&["workspace", "destroy"]);
    assert!(
        help.contains("--delete-branch") && help.contains("--keep-branch"),
        "destroy --help must list both branch flags:\n{help}"
    );
    assert!(
        help.contains("Authorize deleting") && help.contains("user-owned"),
        "destroy --help --delete-branch must use the corrected semantics:\n{help}"
    );
    assert!(
        help.contains("Takes precedence over"),
        "destroy --help --keep-branch must state precedence:\n{help}"
    );
}

#[test]
fn help_workspace_destroy_documents_branch_flags_zh() {
    let help = render_help_zh(&["workspace", "destroy"]);
    assert!(
        help.contains("--delete-branch") && help.contains("--keep-branch"),
        "zh destroy --help must keep both flag tokens:\n{help}"
    );
    assert!(
        help.contains("授权删除"),
        "zh destroy --help --delete-branch must use corrected semantics:\n{help}"
    );
    assert!(
        help.contains("优先级高于"),
        "zh destroy --help --keep-branch must state precedence:\n{help}"
    );
}

#[test]
fn help_workspace_worktree_remove_documents_branch_flags_en() {
    let help = render_help_en(&["workspace", "worktree", "remove"]);
    assert!(
        help.contains("--delete-branch") && help.contains("--keep-branch"),
        "worktree remove --help must list both branch flags:\n{help}"
    );
    assert!(
        help.contains("Authorize deleting") && help.contains("user-owned"),
        "worktree remove --help --delete-branch must use corrected semantics:\n{help}"
    );
    assert!(
        help.contains("Takes precedence over"),
        "worktree remove --help --keep-branch must state precedence:\n{help}"
    );
}

#[test]
fn help_workspace_worktree_remove_documents_branch_flags_zh() {
    let help = render_help_zh(&["workspace", "worktree", "remove"]);
    assert!(
        help.contains("--delete-branch") && help.contains("--keep-branch"),
        "zh worktree remove --help must keep both flag tokens:\n{help}"
    );
    assert!(
        help.contains("授权删除"),
        "zh worktree remove --help --delete-branch must use corrected semantics:\n{help}"
    );
    assert!(
        help.contains("优先级高于"),
        "zh worktree remove --help --keep-branch must state precedence:\n{help}"
    );
}

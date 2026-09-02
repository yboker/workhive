use super::*;
use crate::domain::{
    PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy, SNAPSHOT_SCHEMA_VERSION,
    SnapshotMetadata, SnapshotRevisionId, SnapshotSource, WindowSnapshot, WorkspaceSnapshot,
    WorkspaceStateKey, WorkspaceTopology,
};
use camino::Utf8PathBuf;

fn sample_snapshot() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new(
            "a1b2".into(),
            "proj".into(),
            String::new(),
            "main".into(),
        ),
        captured_at: "2026-04-24T12:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: Some("main".into()),
                layout: "tiled".into(),
                panes: vec![
                    PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 2,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::EmptyShell,
                observed: PaneObservedState {
                    current_command: Some("zsh".into()),
                    start_command: None,
                    active: true,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: Some("editor".into()),
                cwd: Utf8PathBuf::from("/tmp/proj"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::ToolRef {
                    tool_name: "vim".into(),
                },
                observed: PaneObservedState {
                    current_command: Some("vim".into()),
                    start_command: Some("vim".into()),
                    active: false,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 2,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                managed: true,
                restore_policy: RestorePolicy::Skip,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "ssh".into(),
                    reason: "remote".into(),
                },
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: true,
                },
            },
        ],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    }
}

#[test]
fn build_plan_with_mixed_policies() {
    let snap = sample_snapshot();
    let plan = build_restore_plan(&snap, false).unwrap();

    assert_eq!(plan.window_actions.len(), 1);
    assert_eq!(plan.pane_count(), 2); // 2 Create, 1 Skip
    assert!(!plan.dry_run);
}

#[test]
fn build_plan_dry_run() {
    let snap = sample_snapshot();
    let plan = build_restore_plan(&snap, true).unwrap();

    assert!(plan.dry_run);
    let outcome = summarize_plan(&plan);
    assert!(matches!(
        outcome,
        RestoreOutcome::DryRun {
            panes_planned: 2,
            windows_planned: 1,
            ..
        }
    ));
}

#[test]
fn validate_empty_plan_errors() {
    let plan = RestorePlan {
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        revision_id: SnapshotRevisionId::first(),
        window_actions: vec![],
        active_window: None,
        dry_run: false,
    };
    let err = validate_restore_plan(&plan).unwrap_err();
    assert!(matches!(err, SnapshotError::RestoreFailed { .. }));
}

#[test]
fn non_restorable_pane_degrades_to_shell() {
    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        captured_at: "2026-04-24T12:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".into(),
                panes: vec![PaneRef {
                    pane_index: 0,
                    window_index: 0,
                }],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![PaneSnapshot {
            runtime_id: None,
            window_index: 0,
            pane_index: 0,
            pane_id: None,
            title: None,
            cwd: Utf8PathBuf::from("/tmp"),
            managed: true,
            restore_policy: RestorePolicy::Restore,
            launch_intent: PaneLaunchIntent::NonRestorable {
                display: "ssh".into(),
                reason: "remote".into(),
            },
            observed: PaneObservedState {
                current_command: None,
                start_command: None,
                active: false,
                dead: true,
            },
        }],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let plan = build_restore_plan(&snap, false).unwrap();
    let WindowRestoreAction::Create { pane_actions, .. } = &plan.window_actions[0];
    assert_eq!(pane_actions.len(), 1);
    match &pane_actions[0] {
        PaneRestoreAction::Create { launch_intent, .. } => {
            assert!(matches!(launch_intent, PaneLaunchIntent::EmptyShell));
        }
        _ => panic!("expected Create action"),
    }
}

#[test]
fn summarize_success_plan() {
    let snap = sample_snapshot();
    let plan = build_restore_plan(&snap, false).unwrap();
    let outcome = summarize_plan(&plan);
    assert!(matches!(
        outcome,
        RestoreOutcome::Success {
            panes_restored: 2,
            windows_restored: 1,
            ..
        }
    ));
}

#[test]
fn workspace_isolation_different_state_keys() {
    let snap1 = {
        let mut s = sample_snapshot();
        s.state_key =
            WorkspaceStateKey::new("a1b2".into(), "proj".into(), String::new(), "main".into());
        s
    };
    let snap2 = {
        let mut s = sample_snapshot();
        s.state_key = WorkspaceStateKey::new(
            "a1b2".into(),
            "proj".into(),
            "subdir-a".into(),
            "main".into(),
        );
        s
    };
    let snap3 = {
        let mut s = sample_snapshot();
        s.state_key =
            WorkspaceStateKey::new("a1b2".into(), "proj".into(), String::new(), "feat-x".into());
        s
    };

    let plan1 = build_restore_plan(&snap1, false).unwrap();
    let plan2 = build_restore_plan(&snap2, false).unwrap();
    let plan3 = build_restore_plan(&snap3, false).unwrap();

    assert_ne!(plan1.state_key, plan2.state_key);
    assert_ne!(plan1.state_key, plan3.state_key);
    assert_ne!(plan2.state_key, plan3.state_key);
}

#[test]
fn reconcile_recovers_tool_ref_from_baseline() {
    use crate::domain::{CommandSpec, PaneLaunchRef, ToolName};
    use std::collections::HashMap;

    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        captured_at: "2026-04-27T00:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".into(),
                panes: vec![
                    PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no start_command".into(),
                },
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no start_command".into(),
                },
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let mut plan = build_restore_plan(&snap, false).unwrap();

    let mut baseline = HashMap::new();
    baseline.insert(
        (0u32, 0u32),
        PaneLaunchRef::Tool {
            name: ToolName::new("claude").unwrap(),
        },
    );
    baseline.insert(
        (0u32, 1u32),
        PaneLaunchRef::InlineCommand {
            spec: CommandSpec::new("npm", ["run", "dev"]),
        },
    );

    let recovered =
        reconcile_plan_with_baseline(&mut plan, &baseline, &std::collections::HashSet::new());
    assert_eq!(recovered, 2);

    let WindowRestoreAction::Create { pane_actions, .. } = &plan.window_actions[0];
    match &pane_actions[0] {
        PaneRestoreAction::Create { launch_intent, .. } => {
            assert_eq!(
                *launch_intent,
                PaneLaunchIntent::ToolRef {
                    tool_name: "claude".into()
                }
            );
        }
        _ => panic!("expected Create"),
    }
    match &pane_actions[1] {
        PaneRestoreAction::Create { launch_intent, .. } => {
            assert_eq!(
                *launch_intent,
                PaneLaunchIntent::InlineCommand {
                    program: "npm".into(),
                    args: vec!["run".into(), "dev".into()],
                }
            );
        }
        _ => panic!("expected Create"),
    }

    let fidelity = compute_plan_fidelity(&plan);
    assert!(matches!(
        fidelity,
        RestoreFidelity::LaunchRestorable {
            command_restored: 2,
            shell_only: 0,
        }
    ));
}

#[test]
fn reconcile_skips_empty_shell_baseline() {
    use crate::domain::PaneLaunchRef;
    use std::collections::HashMap;

    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        captured_at: "2026-04-27T00:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".into(),
                panes: vec![PaneRef {
                    pane_index: 0,
                    window_index: 0,
                }],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![PaneSnapshot {
            runtime_id: None,
            window_index: 0,
            pane_index: 0,
            pane_id: None,
            title: None,
            cwd: Utf8PathBuf::from("/tmp"),
            managed: true,
            restore_policy: RestorePolicy::Restore,
            launch_intent: PaneLaunchIntent::NonRestorable {
                display: "zsh".into(),
                reason: "no start_command".into(),
            },
            observed: PaneObservedState {
                current_command: None,
                start_command: None,
                active: false,
                dead: false,
            },
        }],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let mut plan = build_restore_plan(&snap, false).unwrap();
    let mut baseline = HashMap::new();
    baseline.insert((0u32, 0u32), PaneLaunchRef::EmptyShell);

    let recovered =
        reconcile_plan_with_baseline(&mut plan, &baseline, &std::collections::HashSet::new());
    assert_eq!(
        recovered, 0,
        "EmptyShell baseline should not count as recovery"
    );
}

#[test]
fn fidelity_all_shell_only_snapshot_is_topology_only() {
    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        captured_at: "2026-04-27T00:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".into(),
                panes: vec![
                    PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::EmptyShell,
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::ShellOnly,
                launch_intent: PaneLaunchIntent::EmptyShell,
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: false,
                },
            },
        ],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let plan = build_restore_plan(&snap, false).unwrap();
    let fidelity = compute_plan_fidelity(&plan);
    assert!(
        matches!(
            fidelity,
            RestoreFidelity::TopologyOnly {
                command_restored: 0,
                shell_only: 2
            }
        ),
        "all shell-only snapshot restore must be TopologyOnly, got: {fidelity:?}"
    );
}

#[test]
fn fidelity_mixed_restore_is_topology_only() {
    let snap = sample_snapshot();
    let plan = build_restore_plan(&snap, false).unwrap();
    let fidelity = compute_plan_fidelity(&plan);
    assert!(
        matches!(
            fidelity,
            RestoreFidelity::TopologyOnly {
                command_restored: 1,
                shell_only: 1
            }
        ),
        "mixed restore must be TopologyOnly, got: {fidelity:?}"
    );
}

#[test]
fn fidelity_all_command_restored_is_launch_restorable() {
    let mut snap = sample_snapshot();
    snap.panes = vec![PaneSnapshot {
        runtime_id: None,
        window_index: 0,
        pane_index: 0,
        pane_id: None,
        title: None,
        cwd: Utf8PathBuf::from("/tmp"),
        managed: true,
        restore_policy: RestorePolicy::Restore,
        launch_intent: PaneLaunchIntent::ToolRef {
            tool_name: "vim".into(),
        },
        observed: PaneObservedState {
            current_command: Some("vim".into()),
            start_command: Some("vim".into()),
            active: true,
            dead: false,
        },
    }];
    snap.topology.windows[0].panes = vec![PaneRef {
        pane_index: 0,
        window_index: 0,
    }];

    let plan = build_restore_plan(&snap, false).unwrap();
    let fidelity = compute_plan_fidelity(&plan);
    assert!(
        matches!(
            fidelity,
            RestoreFidelity::LaunchRestorable {
                command_restored: 1,
                shell_only: 0
            }
        ),
        "all-command restore must be LaunchRestorable, got: {fidelity:?}"
    );
}

/// F7: End-to-end fault chain test.
///
/// Scenario:
/// - Prior baseline has 3 launch entries (tool/inline/tool at positions 0-2).
/// - Latest snapshot has 4 panes, ALL NonRestorable/ShellOnly.
/// - After reconciliation with baseline:
///   - 3 baseline-matched panes recover their commands.
///   - 1 new pane (position 3, no baseline match) stays EmptyShell.
/// - Fidelity must be TopologyOnly with command_restored=3, shell_only=1.
#[test]
fn fault_chain_baseline_reconciles_nonrestorable_snapshot() {
    use std::collections::HashMap;

    let non_restorable_obs = PaneObservedState {
        current_command: Some("zsh".into()),
        start_command: Some("zsh".into()),
        active: false,
        dead: false,
    };

    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(2),
        state_key: WorkspaceStateKey::new(
            "a1b2".into(),
            "proj".into(),
            String::new(),
            "main".into(),
        ),
        captured_at: "2026-04-27T00:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: Some("main".into()),
                layout: "tiled".into(),
                panes: vec![
                    PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 2,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 3,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::ShellOnly,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no durable metadata".into(),
                },
                observed: non_restorable_obs.clone(),
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::ShellOnly,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no durable metadata".into(),
                },
                observed: non_restorable_obs.clone(),
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 2,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::ShellOnly,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no durable metadata".into(),
                },
                observed: non_restorable_obs.clone(),
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 3,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::ShellOnly,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "zsh".into(),
                    reason: "no durable metadata".into(),
                },
                observed: non_restorable_obs,
            },
        ],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 2,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let mut plan = build_restore_plan(&snap, false).expect("plan should build");
    assert_eq!(plan.pane_count(), 4, "must plan 4 panes");

    let mut prior_baseline: HashMap<(u32, u32), crate::domain::PaneLaunchRef> = HashMap::new();
    prior_baseline.insert(
        (0, 0),
        crate::domain::PaneLaunchRef::Tool {
            name: crate::domain::ToolName::new("claude").expect("valid"),
        },
    );
    prior_baseline.insert(
        (0, 1),
        crate::domain::PaneLaunchRef::InlineCommand {
            spec: crate::domain::CommandSpec::new("python", ["server.py"].iter().copied()),
        },
    );
    prior_baseline.insert(
        (0, 2),
        crate::domain::PaneLaunchRef::Tool {
            name: crate::domain::ToolName::new("codex").expect("valid"),
        },
    );

    // The fault-chain test deliberately exercises the *non-stale*
    // reconciliation behavior: prior baseline must hydrate panes
    // whose snapshot lost the launch intent. Pass an empty
    // `stale_tool_positions` set so the freshness rule does not
    // suppress recovery.
    let recovered = reconcile_plan_with_baseline(
        &mut plan,
        &prior_baseline,
        &std::collections::HashSet::new(),
    );
    assert_eq!(recovered, 3, "must recover 3 of 4 panes from baseline");

    let fidelity = compute_plan_fidelity(&plan);
    assert!(
        matches!(
            fidelity,
            RestoreFidelity::TopologyOnly {
                command_restored: 3,
                shell_only: 1
            }
        ),
        "fidelity must be TopologyOnly(3 cmd, 1 shell), got: {fidelity:?}"
    );

    let WindowRestoreAction::Create { pane_actions, .. } = &plan.window_actions[0];
    let intents: Vec<&PaneLaunchIntent> = pane_actions
        .iter()
        .filter_map(|a| match a {
            PaneRestoreAction::Create { launch_intent, .. } => Some(launch_intent),
            _ => None,
        })
        .collect();

    assert!(
        matches!(intents[0], PaneLaunchIntent::ToolRef { tool_name } if tool_name == "claude"),
        "pane 0 must be claude ToolRef, got: {:?}",
        intents[0],
    );
    assert!(
        matches!(intents[1], PaneLaunchIntent::InlineCommand { program, .. } if program == "python"),
        "pane 1 must be python InlineCommand, got: {:?}",
        intents[1],
    );
    assert!(
        matches!(intents[2], PaneLaunchIntent::ToolRef { tool_name } if tool_name == "codex"),
        "pane 2 must be codex ToolRef, got: {:?}",
        intents[2],
    );
    assert!(
        matches!(intents[3], PaneLaunchIntent::EmptyShell),
        "pane 3 (no baseline) must stay EmptyShell, got: {:?}",
        intents[3],
    );
}

/// Baseline freshness rule:
/// when the user exits a tool and the pane snapshot observed a shell,
/// the prior tool baseline is stale and MUST NOT re-launch the tool.
#[test]
fn reconcile_skips_stale_tool_baseline_on_exited_pane() {
    use crate::domain::{PaneLaunchRef, ToolName};
    use std::collections::HashMap;

    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: WorkspaceStateKey::new("x".into(), "p".into(), String::new(), "w".into()),
        captured_at: "2026-05-06T00:00:00Z".into(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".into(),
                panes: vec![PaneRef {
                    pane_index: 0,
                    window_index: 0,
                }],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![PaneSnapshot {
            runtime_id: None,
            window_index: 0,
            pane_index: 0,
            pane_id: None,
            title: None,
            cwd: Utf8PathBuf::from("/tmp"),
            managed: true,
            restore_policy: RestorePolicy::ShellOnly,
            launch_intent: PaneLaunchIntent::EmptyShell,
            observed: PaneObservedState {
                current_command: Some("zsh".into()),
                start_command: Some("zsh".into()),
                active: true,
                dead: false,
            },
        }],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".into(),
            platform: "macos".into(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };

    let mut plan = build_restore_plan(&snap, false).unwrap();
    let mut baseline = HashMap::new();
    baseline.insert(
        (0u32, 0u32),
        PaneLaunchRef::Tool {
            name: ToolName::new("codex").unwrap(),
        },
    );

    let stale = stale_tool_positions_from_snapshot(&snap);
    assert_eq!(stale.len(), 1, "shell pane must be marked stale");
    assert!(stale.contains(&(0u32, 0u32)));

    let recovered = reconcile_plan_with_baseline(&mut plan, &baseline, &stale);
    assert_eq!(
        recovered, 0,
        "stale tool baseline must be skipped for exited pane"
    );

    let WindowRestoreAction::Create { pane_actions, .. } = &plan.window_actions[0];
    match &pane_actions[0] {
        PaneRestoreAction::Create { launch_intent, .. } => {
            assert!(
                matches!(launch_intent, PaneLaunchIntent::EmptyShell),
                "pane 0 must remain EmptyShell, got: {launch_intent:?}",
            );
        }
        _ => panic!("expected Create"),
    }
}

/// `stale_tool_positions_from_snapshot` must not treat non-shell
/// foreground commands as stale.
#[test]
fn stale_positions_does_not_flag_running_tool() {
    let mut snap = sample_snapshot();
    snap.panes[0].observed.current_command = Some("codex".into());
    snap.panes[1].observed.current_command = Some("python".into());

    let stale = stale_tool_positions_from_snapshot(&snap);
    assert!(
        stale.is_empty(),
        "non-shell observed commands must not be marked stale, got: {stale:?}"
    );
}

/// Absolute paths to canonical shells still count as shells.
#[test]
fn stale_positions_recognizes_absolute_path_shell() {
    let mut snap = sample_snapshot();
    snap.panes[0].observed.current_command = Some("/bin/bash".into());
    snap.panes[1].observed.current_command = Some("/usr/bin/fish".into());

    let stale = stale_tool_positions_from_snapshot(&snap);
    assert_eq!(stale.len(), 2);
    assert!(stale.contains(&(0u32, 0u32)));
    assert!(stale.contains(&(0u32, 1u32)));
}

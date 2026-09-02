//! Snapshot capture use case — builds canonical snapshots from live runtime state.
//!
//! Captures topology, pane state, and launch intent from the runtime ports
//! and pane registry, producing a `WorkspaceSnapshot` ready for persistence.

use camino::Utf8PathBuf;

#[cfg(test)]
use crate::domain::{PaneKind, PaneState};
use crate::domain::{
    PaneLaunchIntent, PaneLaunchOrigin, PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy,
    RuntimeBackendId, SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, SnapshotRevisionId,
    SnapshotSource, ToolCatalog, WindowSnapshot, WorkspaceSnapshot, WorkspaceStateKey,
    WorkspaceTopology,
};
use crate::errors::SnapshotError;
#[cfg(test)]
use crate::ports::RuntimeUnitSnapshot;
use crate::ports::{RuntimeTopologySnapshot, SnapshotConfig};

/// Context required to capture a snapshot.
pub struct CaptureContext {
    /// Typed workspace state key.
    pub state_key: WorkspaceStateKey,
    /// Runtime backend identifier.
    pub runtime_backend: RuntimeBackendId,
    /// Platform (e.g. "macos", "linux").
    pub platform: String,
    /// ISO-8601 timestamp of capture.
    pub captured_at: String,
    /// How this capture was triggered.
    pub source: SnapshotSource,
    /// Next revision ID to assign.
    pub next_revision_id: SnapshotRevisionId,
    /// Current snapshot configuration.
    pub config: SnapshotConfig,
}

/// Build a `WorkspaceSnapshot` from live runtime unit snapshots.
///
/// Test-only helper for constructing a snapshot from flat runtime units.
/// Production capture must use `build_snapshot_from_topology`.
#[cfg(test)]
pub fn build_snapshot_from_units(
    ctx: &CaptureContext,
    units: &[RuntimeUnitSnapshot],
) -> Result<WorkspaceSnapshot, SnapshotError> {
    if !ctx.config.enabled {
        return Err(SnapshotError::Disabled);
    }

    let panes: Vec<PaneSnapshot> = units
        .iter()
        .map(|unit| convert_unit_to_snapshot_pane(unit, &ctx.config))
        .collect();

    let pane_refs: Vec<PaneRef> = panes
        .iter()
        .map(|p| PaneRef {
            pane_index: p.pane_index,
            window_index: p.window_index,
        })
        .collect();

    let active_pane_id = panes
        .iter()
        .find(|p| p.observed.active)
        .and_then(|p| p.runtime_id.clone());

    let topology = WorkspaceTopology {
        windows: vec![WindowSnapshot {
            index: 0,
            name: None,
            layout: "tiled".to_owned(),
            panes: pane_refs,
        }],
        active_window: Some(0),
        active_pane_runtime_id: active_pane_id,
    };

    Ok(WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: ctx.next_revision_id,
        state_key: ctx.state_key.clone(),
        captured_at: ctx.captured_at.clone(),
        source: ctx.source.clone(),
        topology,
        panes,
        metadata: SnapshotMetadata {
            runtime_backend: ctx.runtime_backend.as_str().to_owned(),
            platform: ctx.platform.clone(),
            generation: ctx.next_revision_id.0,
            content_hash: None,
            config_fingerprint: None,
        },
    })
}

/// Build a `WorkspaceSnapshot` from full runtime topology.
pub fn build_snapshot_from_topology(
    ctx: &CaptureContext,
    topology: &RuntimeTopologySnapshot,
) -> Result<WorkspaceSnapshot, SnapshotError> {
    if !ctx.config.enabled {
        return Err(SnapshotError::Disabled);
    }

    let mut panes: Vec<PaneSnapshot> = topology
        .panes
        .iter()
        .filter(|pane| pane.managed || ctx.config.include_unmanaged_panes)
        .map(convert_runtime_pane_to_snapshot)
        .collect();
    panes.sort_by_key(|pane| (pane.window_index, pane.pane_index));

    if panes.is_empty() {
        return Err(SnapshotError::RestoreFailed {
            reason: "topology capture returned no panes".to_owned(),
        });
    }

    let mut windows: Vec<WindowSnapshot> = topology
        .windows
        .iter()
        .map(|window| {
            let pane_refs = panes
                .iter()
                .filter(|pane| pane.window_index == window.index)
                .map(|pane| PaneRef {
                    pane_index: pane.pane_index,
                    window_index: pane.window_index,
                })
                .collect::<Vec<_>>();
            WindowSnapshot {
                index: window.index,
                name: window.name.clone(),
                layout: window.layout.clone(),
                panes: pane_refs,
            }
        })
        .filter(|window| !window.panes.is_empty())
        .collect();
    windows.sort_by_key(|window| window.index);

    if windows.is_empty() {
        return Err(SnapshotError::RestoreFailed {
            reason: "topology capture returned no windows with panes".to_owned(),
        });
    }

    let active_window = topology
        .active_window
        .filter(|index| windows.iter().any(|window| window.index == *index));
    let active_pane_runtime_id = topology
        .active_pane_runtime_id
        .as_ref()
        .and_then(|runtime_id| {
            panes
                .iter()
                .find(|pane| pane.runtime_id.as_deref() == Some(runtime_id.as_str()))
                .and_then(|pane| pane.runtime_id.clone())
        });

    Ok(WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: ctx.next_revision_id,
        state_key: ctx.state_key.clone(),
        captured_at: ctx.captured_at.clone(),
        source: ctx.source.clone(),
        topology: WorkspaceTopology {
            windows,
            active_window,
            active_pane_runtime_id,
        },
        panes,
        metadata: SnapshotMetadata {
            runtime_backend: ctx.runtime_backend.as_str().to_owned(),
            platform: ctx.platform.clone(),
            generation: ctx.next_revision_id.0,
            content_hash: None,
            config_fingerprint: None,
        },
    })
}

/// Convert a runtime unit snapshot into a pane snapshot.
///
/// `RuntimeUnitSnapshot` fields: `id`, `index`, `cwd`, `command`,
/// `active`, `kind`, `state`.
#[cfg(test)]
fn convert_unit_to_snapshot_pane(
    unit: &RuntimeUnitSnapshot,
    _config: &SnapshotConfig,
) -> PaneSnapshot {
    let launch_intent = derive_launch_intent_from_kind(&unit.kind, &unit.command);

    let restore_policy = if !launch_intent.is_restorable() {
        RestorePolicy::ShellOnly
    } else {
        RestorePolicy::Restore
    };

    let dead = matches!(unit.state, PaneState::Unknown);

    PaneSnapshot {
        runtime_id: Some(unit.id.as_str().to_owned()),
        window_index: 0,
        pane_index: unit.index,
        pane_id: None,
        title: None,
        cwd: unit.cwd.clone(),
        managed: true,
        restore_policy,
        launch_intent,
        observed: PaneObservedState {
            current_command: Some(unit.command.clone()),
            start_command: None,
            active: unit.active,
            dead,
        },
    }
}

fn convert_runtime_pane_to_snapshot(pane: &crate::ports::RuntimePaneSnapshot) -> PaneSnapshot {
    let launch_intent = pane
        .launch_origin
        .as_ref()
        .map(convert_launch_origin_to_intent)
        .unwrap_or_else(|| derive_launch_intent_from_start_command(pane));

    let restore_policy = if !pane.managed || !launch_intent.is_restorable() {
        RestorePolicy::ShellOnly
    } else {
        RestorePolicy::Restore
    };

    PaneSnapshot {
        runtime_id: Some(pane.unit_id.as_str().to_owned()),
        window_index: pane.window_index,
        pane_index: pane.pane_index,
        pane_id: pane.pane_id.clone(),
        title: pane.title.clone(),
        cwd: pane.cwd.clone(),
        managed: pane.managed,
        restore_policy,
        launch_intent,
        observed: PaneObservedState {
            current_command: pane.current_command.clone(),
            start_command: pane.start_command.clone(),
            active: pane.active,
            dead: pane.dead,
        },
    }
}

fn convert_launch_origin_to_intent(origin: &PaneLaunchOrigin) -> PaneLaunchIntent {
    match origin {
        PaneLaunchOrigin::EmptyShell => PaneLaunchIntent::EmptyShell,
        PaneLaunchOrigin::Tool { name } => PaneLaunchIntent::ToolRef {
            tool_name: name.as_str().to_owned(),
        },
        PaneLaunchOrigin::InlineCommand { spec } => PaneLaunchIntent::InlineCommand {
            program: spec.program().to_owned(),
            args: spec.args().to_vec(),
        },
    }
}

/// Derive a snapshot-time `PaneLaunchIntent` from runtime pane data.
///
/// Routes the raw `pane_start_command` through the shared
/// [`runtime_start_command::classify_start_command`] classifier so the
/// snapshot path and the tmux runtime adapter share one typed
/// understanding of what tmux observed. Critically, this prevents
/// `workhive internal pane-supervisor ... --tool <name>` argv from
/// being recorded as a user `InlineCommand` (which `workspace open`
/// would later replay verbatim, surfacing the wrapper at the top of
/// the pane and skipping the real tool launch). Recoverable wrappers
/// round-trip back to `ToolRef`; malformed wrappers degrade to
/// `NonRestorable` so they cannot leak into a replayable record.
fn derive_launch_intent_from_start_command(
    pane: &crate::ports::RuntimePaneSnapshot,
) -> PaneLaunchIntent {
    use crate::usecases::runtime_start_command::{
        StartCommandClassification, classify_start_command,
    };

    let raw = pane.start_command.as_deref();
    match classify_start_command(raw) {
        StartCommandClassification::Empty => {
            let display = pane
                .current_command
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .or(pane.title.as_deref())
                .unwrap_or("unknown");
            PaneLaunchIntent::NonRestorable {
                display: display.to_owned(),
                reason: "missing durable launch metadata (start_command)".to_owned(),
            }
        }
        StartCommandClassification::Shell => PaneLaunchIntent::EmptyShell,
        StartCommandClassification::Tool { name } => {
            // Only echo back tools the catalog actually knows; otherwise
            // degrade to NonRestorable so we never persist a tool name
            // that the launch path cannot resolve.
            if ToolCatalog::builtin_defaults().find(&name).is_some() {
                PaneLaunchIntent::ToolRef {
                    tool_name: name.as_str().to_owned(),
                }
            } else {
                PaneLaunchIntent::NonRestorable {
                    display: name.as_str().to_owned(),
                    reason: "unrecognized tool identifier".to_owned(),
                }
            }
        }
        StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
            if ToolCatalog::builtin_defaults().find(&name).is_some() {
                PaneLaunchIntent::ToolRef {
                    tool_name: name.as_str().to_owned(),
                }
            } else {
                PaneLaunchIntent::NonRestorable {
                    display: name.as_str().to_owned(),
                    reason: "supervisor wrapper referenced an unknown tool".to_owned(),
                }
            }
        }
        StartCommandClassification::InternalSupervisorWrapper { tool: None } => {
            // Malformed supervisor wrapper: we cannot recover what tool
            // was launched. Persist as NonRestorable — never InlineCommand
            // — so a future open never replays the wrapper argv.
            PaneLaunchIntent::NonRestorable {
                display: "workhive internal pane-supervisor".to_owned(),
                reason: "malformed supervisor wrapper: --tool not recoverable".to_owned(),
            }
        }
        StartCommandClassification::InlineCommand { spec } => {
            let program = spec.program();
            if matches!(program, "ssh" | "mosh" | "scp" | "sftp") {
                let mut display = program.to_owned();
                for arg in spec.args() {
                    display.push(' ');
                    display.push_str(arg);
                }
                return PaneLaunchIntent::NonRestorable {
                    display,
                    reason: "interactive remote sessions are not restorable".to_owned(),
                };
            }
            PaneLaunchIntent::InlineCommand {
                program: program.to_owned(),
                args: spec.args().to_vec(),
            }
        }
    }
}

/// Derive launch intent from pane kind and current command.
///
/// Uses the `PaneKind` classification as the primary source:
/// - `Agent` / `GitUi` -> `ToolRef`
/// - `Terminal` -> `EmptyShell`
/// - `Unknown` -> `NonRestorable`
#[cfg(test)]
fn derive_launch_intent_from_kind(kind: &PaneKind, command: &str) -> PaneLaunchIntent {
    match kind {
        PaneKind::Agent => PaneLaunchIntent::ToolRef {
            tool_name: command.to_owned(),
        },
        PaneKind::GitUi => PaneLaunchIntent::ToolRef {
            tool_name: command.to_owned(),
        },
        PaneKind::Terminal => PaneLaunchIntent::EmptyShell,
        PaneKind::Unknown => {
            if is_likely_shell(command) {
                PaneLaunchIntent::EmptyShell
            } else {
                PaneLaunchIntent::NonRestorable {
                    display: command.to_owned(),
                    reason: "unknown pane kind".to_owned(),
                }
            }
        }
    }
}

/// Heuristic: is this command name likely a shell?
#[cfg(test)]
fn is_likely_shell(command: &str) -> bool {
    matches!(
        command,
        "zsh" | "bash" | "sh" | "fish" | "nushell" | "nu" | "dash" | "ksh" | "csh" | "tcsh"
    )
}

/// Compute a hash for deduplication.
pub fn compute_content_hash(snapshot: &WorkspaceSnapshot) -> Result<String, SnapshotError> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Exclude checksum field itself from the checksum input.
    let mut canonical = snapshot.clone();
    canonical.metadata.content_hash = None;
    let json = serde_json::to_string(&canonical).map_err(|e| SnapshotError::Corrupted {
        path: Utf8PathBuf::from("<hash-computation>"),
        reason: format!("serialization failed during hash: {e}"),
    })?;

    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::UnitId;
    use crate::ports::RuntimePaneSnapshot;

    fn sample_units() -> Vec<RuntimeUnitSnapshot> {
        vec![
            RuntimeUnitSnapshot {
                id: UnitId::new("u0"),
                index: 0,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                command: "zsh".to_owned(),
                active: true,
                kind: PaneKind::Terminal,
                state: PaneState::Idle,
                tty: None,
                process_pid: None,
            },
            RuntimeUnitSnapshot {
                id: UnitId::new("u1"),
                index: 1,
                cwd: Utf8PathBuf::from("/tmp/proj"),
                command: "lazygit".to_owned(),
                active: false,
                kind: PaneKind::GitUi,
                state: PaneState::Running,
                tty: None,
                process_pid: None,
            },
        ]
    }

    fn sample_ctx() -> CaptureContext {
        CaptureContext {
            state_key: WorkspaceStateKey::new(
                "a1b2".into(),
                "proj".into(),
                String::new(),
                "feat-x".into(),
            ),
            runtime_backend: RuntimeBackendId::tmux(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-24T12:00:00Z".to_owned(),
            source: SnapshotSource::ManualSave,
            next_revision_id: SnapshotRevisionId::first(),
            config: SnapshotConfig::default(),
        }
    }

    #[test]
    fn build_snapshot_success() {
        let ctx = sample_ctx();
        let units = sample_units();
        let snap = build_snapshot_from_units(&ctx, &units).expect("should succeed");

        assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snap.panes.len(), 2);
        assert_eq!(snap.topology.windows.len(), 1);
        assert!(matches!(
            snap.panes[0].launch_intent,
            PaneLaunchIntent::EmptyShell
        ));
        assert!(matches!(
            snap.panes[1].launch_intent,
            PaneLaunchIntent::ToolRef { .. }
        ));
        assert!(snap.panes[0].observed.active);
        assert!(!snap.panes[1].observed.active);
    }

    #[test]
    fn build_snapshot_disabled_errors() {
        let mut ctx = sample_ctx();
        ctx.config.enabled = false;
        let err = build_snapshot_from_units(&ctx, &sample_units()).unwrap_err();
        assert!(matches!(err, SnapshotError::Disabled));
    }

    #[test]
    fn content_hash_deterministic() {
        let ctx = sample_ctx();
        let snap = build_snapshot_from_units(&ctx, &sample_units()).expect("ok");
        let h1 = compute_content_hash(&snap).expect("ok");
        let h2 = compute_content_hash(&snap).expect("ok");
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_changes_on_mutation() {
        let ctx = sample_ctx();
        let snap1 = build_snapshot_from_units(&ctx, &sample_units()).expect("ok");
        let h1 = compute_content_hash(&snap1).expect("ok");

        let mut snap2 = snap1;
        snap2.panes[0].cwd = Utf8PathBuf::from("/different/path");
        let h2 = compute_content_hash(&snap2).expect("ok");
        assert_ne!(h1, h2);
    }

    #[test]
    fn unknown_shell_command_gets_empty_shell() {
        let unit = RuntimeUnitSnapshot {
            id: UnitId::new("u0"),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp"),
            command: "bash".to_owned(),
            active: false,
            kind: PaneKind::Unknown,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        };
        let config = SnapshotConfig::default();
        let pane = convert_unit_to_snapshot_pane(&unit, &config);
        assert!(matches!(pane.launch_intent, PaneLaunchIntent::EmptyShell));
        assert_eq!(pane.restore_policy, RestorePolicy::Restore);
    }

    /// Helper: build a runtime pane snapshot for a pane whose
    /// `pane_start_command` (as observed by tmux) is the literal
    /// `workhive internal pane-supervisor` argv we use to wrap tool
    /// launches.
    fn supervisor_wrapper_pane(start_command: &str, foreground: &str) -> RuntimePaneSnapshot {
        RuntimePaneSnapshot {
            unit_id: UnitId::new("%42"),
            pane_id: Some("%42".to_owned()),
            window_index: 0,
            pane_index: 0,
            cwd: Utf8PathBuf::from("/tmp/proj"),
            title: Some("codex".to_owned()),
            current_command: Some(foreground.to_owned()),
            start_command: Some(start_command.to_owned()),
            active: true,
            dead: false,
            launch_origin: None,
            managed: true,
            tty: None,
            process_pid: None,
        }
    }

    /// Close/open closed-loop regression:
    /// after `workspace open` runs a tool through the
    /// `workhive internal pane-supervisor ... --tool <tool> -- ...`
    /// wrapper, tmux reports that exact argv as `pane_start_command`.
    /// The previous snapshot capture path turned that wrapper argv
    /// into a user `PaneLaunchIntent::InlineCommand` and the next
    /// `workspace open` replayed it via `send_input`, surfacing the
    /// wrapper at the top of the pane and re-launching the wrapper
    /// instead of the real tool.
    ///
    /// Contract: a well-formed supervisor wrapper whose `--tool` we
    /// can recover MUST round-trip back to `ToolRef { tool_name }`
    /// every cycle. It must never persist as an `InlineCommand`,
    /// even after multiple close/open cycles.
    #[test]
    fn snapshot_capture_recovers_toolref_from_supervisor_wrapper_argv() {
        // The exact shape `tool_launch_instrumentation::wrap_tool_command`
        // produces (with `--workspace` injected by `open.rs`).
        let wrapper = "workhive internal pane-supervisor \
                       --workspace wh@iid@proj@main \
                       --tool codex \
                       -- codex";
        let pane = supervisor_wrapper_pane(wrapper, "codex");
        let snapshot_pane = convert_runtime_pane_to_snapshot(&pane);

        match &snapshot_pane.launch_intent {
            PaneLaunchIntent::ToolRef { tool_name } => {
                assert_eq!(tool_name, "codex");
            }
            other => panic!(
                "expected ToolRef recovered from supervisor wrapper, \
                 got {other:?}"
            ),
        }
        assert_eq!(snapshot_pane.restore_policy, RestorePolicy::Restore);
    }

    /// The `--tool` flag may also use the
    /// `--tool=<name>` form. Both spellings must round-trip back to
    /// `ToolRef`.
    #[test]
    fn snapshot_capture_recovers_toolref_from_supervisor_wrapper_with_equals_flag() {
        let wrapper = "workhive internal pane-supervisor \
                       --workspace wh@iid@proj@main \
                       --tool=claude \
                       -- claude --dangerously-skip-permissions";
        let pane = supervisor_wrapper_pane(wrapper, "claude");
        let snapshot_pane = convert_runtime_pane_to_snapshot(&pane);

        assert!(
            matches!(
                &snapshot_pane.launch_intent,
                PaneLaunchIntent::ToolRef { tool_name } if tool_name == "claude"
            ),
            "expected ToolRef(\"claude\"), got {:?}",
            snapshot_pane.launch_intent
        );
    }

    /// A malformed supervisor wrapper (no
    /// recoverable `--tool`) must NEVER be persisted as
    /// `InlineCommand` — the next `workspace open` would replay it
    /// verbatim. Demote to `NonRestorable` so the restore path skips
    /// it cleanly.
    #[test]
    fn snapshot_capture_demotes_malformed_supervisor_wrapper_to_non_restorable() {
        let wrapper = "workhive internal pane-supervisor --workspace wh@iid@proj@main";
        let pane = supervisor_wrapper_pane(wrapper, "workhive");
        let snapshot_pane = convert_runtime_pane_to_snapshot(&pane);

        match &snapshot_pane.launch_intent {
            PaneLaunchIntent::NonRestorable { reason, .. } => {
                assert!(
                    reason.contains("supervisor wrapper") || reason.contains("--tool"),
                    "expected reason to mention supervisor wrapper, got {reason}"
                );
            }
            other => panic!(
                "expected NonRestorable for malformed supervisor wrapper, \
                 got {other:?}"
            ),
        }
        assert_eq!(snapshot_pane.restore_policy, RestorePolicy::ShellOnly);
    }

    /// Simulate `open -> close(save snapshot) ->
    /// open -> close -> open` by capturing the same supervisor
    /// wrapper argv repeatedly. Each captured `PaneSnapshot` must
    /// contain `ToolRef`, never `InlineCommand`. This pins the
    /// idempotency contract: the captured launch_intent must not
    /// drift across cycles.
    #[test]
    fn snapshot_capture_close_open_loop_never_persists_supervisor_wrapper_as_inline_command() {
        let wrapper = "workhive internal pane-supervisor \
                       --workspace wh@iid@proj@main \
                       --tool codex \
                       -- codex";

        for cycle in 1..=4 {
            let pane = supervisor_wrapper_pane(wrapper, "codex");
            let snapshot_pane = convert_runtime_pane_to_snapshot(&pane);

            assert!(
                !matches!(
                    snapshot_pane.launch_intent,
                    PaneLaunchIntent::InlineCommand { .. }
                ),
                "cycle {cycle}: wrapper argv must never become InlineCommand; \
                 got {:?}",
                snapshot_pane.launch_intent
            );
            assert!(
                matches!(
                    &snapshot_pane.launch_intent,
                    PaneLaunchIntent::ToolRef { tool_name } if tool_name == "codex"
                ),
                "cycle {cycle}: expected ToolRef(\"codex\"), got {:?}",
                snapshot_pane.launch_intent
            );
        }
    }

    #[test]
    fn unknown_non_shell_gets_non_restorable() {
        let unit = RuntimeUnitSnapshot {
            id: UnitId::new("u0"),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp"),
            command: "ssh".to_owned(),
            active: false,
            kind: PaneKind::Unknown,
            state: PaneState::Running,
            tty: None,
            process_pid: None,
        };
        let config = SnapshotConfig::default();
        let pane = convert_unit_to_snapshot_pane(&unit, &config);
        assert!(matches!(
            pane.launch_intent,
            PaneLaunchIntent::NonRestorable { .. }
        ));
        assert_eq!(pane.restore_policy, RestorePolicy::ShellOnly);
    }
}

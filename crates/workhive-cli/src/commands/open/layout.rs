#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Validate and fix pane CWDs for a session.
///
/// This runs AFTER layout application or session reuse to ensure all panes
/// have the correct working directory. It only respawns panes that are in
/// a clean state (idle shell or exited tool), leaving running processes untouched.
///
/// Returns the number of panes that were respawned.
pub(crate) fn validate_and_fix_pane_cwds(
    pane_runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
    base_cwd: &camino::Utf8Path,
) -> Result<u32, CliError> {
    use workhive_core::domain::PaneState;

    // Get all current panes in the session
    let current_units = pane_runtime.list_units(server, session)?;

    let mut respawned = 0u32;

    for unit in &current_units {
        // Check if this pane has a running process.
        // PaneState::Running means some foreground process is active.
        // We must NOT respawn such panes — it would kill the running process.
        let has_running_process = unit.state == PaneState::Running;

        if has_running_process {
            // Running panes cannot be safely fixed here. This can also be a
            // short tmux startup observation before the shell inherits its CWD.
            if unit.cwd.as_path() != base_cwd {
                tracing::debug!(
                    unit_id = %unit.id,
                    actual_cwd = %unit.cwd,
                    expected_cwd = %base_cwd,
                    command = %unit.command,
                    "pane has running process with wrong CWD; preserving it without respawn"
                );
            }
            continue;
        }

        // Pane is idle (empty shell or exited tool), safe to respawn if CWD is wrong
        if unit.cwd.as_path() != base_cwd {
            tracing::info!(
                unit_id = %unit.id,
                actual_cwd = %unit.cwd,
                expected_cwd = %base_cwd,
                state = ?unit.state,
                "respawning idle pane with correct CWD"
            );
            pane_runtime.respawn_unit_with_cwd(server, session, &unit.id, base_cwd)?;
            respawned += 1;
        }
    }

    Ok(respawned)
}

/// Apply column-based layout by executing a pre-computed `SplitPlan`.
///
/// This function is a pure runtime executor: it receives a `SplitPlan`
/// produced by the layout planner and translates each step into runtime
/// operations. It does **not** interpret layout geometry or percentages —
/// all geometry reasoning is done by the planner layer.
///
/// # Execution model
///
/// 1. Column splits (horizontal) target the initial unit, executed
///    right-to-left so each split operates on the correct remaining space.
/// 2. Row splits (vertical) target the root pane of their respective column,
///    executed bottom-to-top within each column.
/// 3. Launch commands are sent after all splits are complete, using the
///    unit IDs captured during split execution.
///
/// Returns the total unit count on success. All runtime errors are propagated —
/// a partial layout failure means the caller must not claim success.
type LayoutResult = Result<LayoutOutput, CliError>;

#[derive(Debug)]
pub(crate) struct LayoutOutput {
    pub(crate) pane_count: u32,
    pub(crate) all_resolved: Vec<(
        usize,
        usize,
        ResolvedPaneLaunch,
        workhive_core::domain::PaneLaunchOrigin,
    )>,
    pub(crate) pane_units: std::collections::HashMap<(usize, usize), workhive_core::domain::UnitId>,
    /// Workhive-owned pane titles set during this `apply_layout` call,
    /// indexed by `UnitId`. Captured so `cmd_open` can re-assert them after
    /// the readiness wait but before `attach_workspace`, defending against
    /// runtime tools (e.g. Codex) that overwrite `#{pane_title}` while
    /// starting up.
    pub(crate) pane_titles: Vec<(workhive_core::domain::UnitId, String)>,
}

fn build_configured_layout_tree(
    layout: &LayoutConfig,
    resolved: &[(
        usize,
        usize,
        ResolvedPaneLaunch,
        workhive_core::domain::PaneLaunchOrigin,
    )],
    base_cwd: &camino::Utf8Path,
    commands: &std::collections::HashMap<(usize, usize), String>,
) -> Result<workhive_core::ports::ConfiguredLayoutNode, CliError> {
    use workhive_core::domain::PaneDirection;
    use workhive_core::ports::{ConfiguredLayoutNode, ConfiguredLayoutPane, LayoutPanePosition};

    fn combine(
        mut nodes: Vec<(u8, ConfiguredLayoutNode)>,
        direction: PaneDirection,
    ) -> ConfiguredLayoutNode {
        if nodes.len() == 1 {
            return nodes.pop().expect("one layout node").1;
        }
        let total: u32 = nodes.iter().map(|(weight, _)| u32::from(*weight)).sum();
        let (first_weight, first) = nodes.remove(0);
        let remaining = total - u32::from(first_weight);
        ConfiguredLayoutNode::Split {
            direction,
            ratio: f64::from(remaining) / f64::from(total),
            first: Box::new(first),
            second: Box::new(combine(nodes, direction)),
        }
    }

    let mut columns = Vec::with_capacity(layout.columns.len());
    for (column, config) in layout.columns.iter().enumerate() {
        let mut panes = Vec::with_capacity(config.panes.len());
        for (row, pane) in config.panes.iter().enumerate() {
            let (_, _, resolved, launch) = resolved
                .iter()
                .find(|(resolved_column, resolved_row, _, _)| {
                    *resolved_column == column && *resolved_row == row
                })
                .ok_or_else(|| {
                    CliError::Runtime(format!(
                        "resolved launch missing for pane ({column}, {row})"
                    ))
                })?;
            let env = match resolved {
                ResolvedPaneLaunch::Executable { env, .. } => env
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
                _ => Vec::new(),
            };
            panes.push((
                pane.height,
                ConfiguredLayoutNode::Pane(ConfiguredLayoutPane {
                    position: LayoutPanePosition { column, row },
                    cwd: base_cwd.to_path_buf(),
                    env,
                    title: pane_title_for_launch(launch, resolved),
                    command: commands.get(&(column, row)).cloned(),
                }),
            ));
        }
        columns.push((config.width, combine(panes, PaneDirection::Down)));
    }
    Ok(combine(columns, PaneDirection::Right))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_layout(
    pane_runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
    base_cwd: &camino::Utf8Path,
    layout: &LayoutConfig,
    catalog: &ToolCatalog,
    i18n: &dyn I18nPort,
    locale: Locale,
    instrumentation: Option<&dyn workhive_core::ports::ToolLaunchInstrumentationPort>,
) -> LayoutResult {
    use std::collections::HashMap;
    use workhive_core::domain::{PaneDirection, UnitId};
    use workhive_core::ports::{SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest};
    use workhive_core::usecases::layout_planner::{self, SplitDirection};

    let plan = layout_planner::plan_layout(layout)?;

    if plan.total_panes == 0 {
        return Ok(LayoutOutput {
            pane_count: 0,
            all_resolved: Vec::new(),
            pane_units: std::collections::HashMap::new(),
            pane_titles: Vec::new(),
        });
    }

    let composition = CliComposition::new();
    let exec_checker = composition.executable_checker();
    if pane_runtime.supports_configured_layout() {
        let all_resolved = plan
            .launches
            .iter()
            .map(|launch| {
                (
                    launch.col_index,
                    launch.pane_index,
                    usecases::launch_resolver::resolve_launch(
                        &launch.launch,
                        catalog,
                        &exec_checker,
                    ),
                    launch.launch.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut native_commands = HashMap::new();
        for (column, row, resolved, origin) in &all_resolved {
            if !matches!(origin, workhive_core::domain::PaneLaunchRef::Tool { .. }) {
                continue;
            }
            if let ResolvedPaneLaunch::Executable {
                command,
                program,
                args,
                env,
                program_path,
                ..
            } = resolved
            {
                native_commands.insert(
                    (*column, *row),
                    wrap_with_supervisor_or_passthrough(
                        instrumentation,
                        server,
                        session,
                        None,
                        origin,
                        program,
                        args,
                        env,
                        program_path.as_deref(),
                        command,
                    )?,
                );
            }
        }
        let request = workhive_core::ports::MaterializeConfiguredLayoutRequest {
            workspace_id: session.to_owned(),
            root: build_configured_layout_tree(layout, &all_resolved, base_cwd, &native_commands)?,
        };
        let materialized = pane_runtime.materialize_configured_layout(request)?;
        let pane_units = materialized
            .panes
            .iter()
            .map(|pane| {
                (
                    (pane.position.column, pane.position.row),
                    pane.unit.id.clone(),
                )
            })
            .collect::<HashMap<_, _>>();
        if pane_units.len() != plan.total_panes as usize {
            return Err(CliError::Runtime(format!(
                "native layout returned {} panes, expected {}",
                pane_units.len(),
                plan.total_panes
            )));
        }
        let mut pane_titles = Vec::with_capacity(all_resolved.len());
        for (column, row, resolved, origin) in &all_resolved {
            let unit_id = pane_units.get(&(*column, *row)).ok_or_else(|| {
                CliError::Runtime(format!("native layout omitted pane ({column}, {row})"))
            })?;
            if matches!(
                origin,
                workhive_core::domain::PaneLaunchRef::InlineCommand { .. }
            ) && let ResolvedPaneLaunch::Executable { command, .. } = resolved
            {
                pane_runtime.send_input(SendInputRequest {
                    group: server.to_owned(),
                    target_unit: unit_id.clone(),
                    text: command.clone(),
                    enter: true,
                })?;
            }
            pane_titles.push((unit_id.clone(), pane_title_for_launch(origin, resolved)));
        }
        return Ok(LayoutOutput {
            pane_count: plan.total_panes,
            all_resolved,
            pane_units,
            pane_titles,
        });
    }

    let initial_units = pane_runtime.list_units(server, session)?;
    let initial = initial_units.first().ok_or_else(|| {
        CliError::Runtime(i18n.resolve(
            locale,
            workhive_core::i18n_keys::OPEN_INITIAL_PANE_NOT_FOUND,
        ))
    })?;
    let initial_unit_id = initial.id.clone();
    let initial_cwd = base_cwd.to_path_buf();

    // Track which UnitId corresponds to each (col_index, pane_index=0) — column roots.
    // The initial pane is col 0's root.
    let mut col_roots: HashMap<usize, UnitId> = HashMap::new();
    col_roots.insert(0, initial_unit_id.clone());

    // Track all created panes by (col_index, pane_index) for launch routing.
    // Also used for post-layout CWD validation to ensure tools start in correct directory.
    let mut pane_units: HashMap<(usize, usize), UnitId> = HashMap::new();
    pane_units.insert((0, 0), initial_unit_id.clone());

    for step in &plan.steps {
        match step.direction {
            SplitDirection::Horizontal => {
                let new_unit = pane_runtime.spawn_unit(SpawnUnitRequest {
                    group: server.to_owned(),
                    workspace_name: session.to_owned(),
                    direction: PaneDirection::Right,
                    command: None,
                    target_unit: Some(initial_unit_id.clone()),
                    size_percent: Some(step.local_percent),
                    env: Vec::new(),
                    cwd: Some(initial_cwd.clone()),
                })?;
                col_roots.insert(step.col_index, new_unit.id.clone());
                pane_units.insert((step.col_index, 0), new_unit.id);
            }
            SplitDirection::Vertical => {
                let target = col_roots.get(&step.col_index).cloned().ok_or_else(|| {
                    CliError::Runtime(i18n.resolve_with_args(
                        locale,
                        workhive_core::i18n_keys::ERROR_LAYOUT_COLUMN_NOT_CREATED,
                        &[&step.col_index.to_string()],
                    ))
                })?;
                let new_unit = pane_runtime.spawn_unit(SpawnUnitRequest {
                    group: server.to_owned(),
                    workspace_name: session.to_owned(),
                    direction: PaneDirection::Down,
                    command: None,
                    target_unit: Some(target),
                    size_percent: Some(step.local_percent),
                    env: Vec::new(),
                    cwd: Some(initial_cwd.clone()),
                })?;
                pane_units.insert((step.col_index, step.pane_index), new_unit.id);
            }
        }
    }

    // Defensive fix for pane cwd correctness.
    //
    // tmux `new-session -c` sets the session's `default-path`, but when the
    // tmux server was already running from a different directory, any pane
    // created during the session (initial pane or even split panes in edge
    // cases) may inherit the server's CWD instead of the expected `base_cwd`.
    //
    // Fix: after all splits, check each pane's actual cwd against `base_cwd`
    // (the project root). If they don't match, respawn the pane with the
    // correct cwd. This is done BEFORE tool launches (which happen below),
    // so the respawned shell is still clean and the subsequent tool install
    // lands on the correct directory.
    //
    // Note: This runs INSIDE apply_layout, before tool launches. A similar
    // check (validate_and_fix_pane_cwds) runs later in entry.rs for reused
    // sessions where layout is not applied. Both are necessary because tool
    // launches must not start with wrong CWD.
    //
    // We iterate over `pane_units` (the panes this layout just created) rather
    // than all session panes, to avoid interfering with externally added panes.
    // Fetch pane state once to avoid O(n) IPC roundtrips.
    let current_units = pane_runtime.list_units(server, session)?;
    for (&(col, row), unit_id) in &pane_units {
        let actual_cwd = current_units
            .iter()
            .find(|u| u.id == *unit_id)
            .map(|u| &u.cwd);

        if let Some(actual) = actual_cwd
            && actual != base_cwd
        {
            tracing::info!(
                unit_id = %unit_id,
                col,
                row,
                actual_cwd = %actual,
                expected_cwd = %base_cwd,
                "respawning pane with wrong CWD after layout"
            );
            pane_runtime.respawn_unit_with_cwd(server, session, unit_id, base_cwd)?;
        }
    }

    // Resolve all launches through the unified pipeline.
    let mut all_resolved: Vec<(
        usize,
        usize,
        ResolvedPaneLaunch,
        workhive_core::domain::PaneLaunchOrigin,
    )> = Vec::new();
    let mut pane_titles: Vec<(workhive_core::domain::UnitId, String)> = Vec::new();

    for launch in &plan.launches {
        let resolved =
            usecases::launch_resolver::resolve_launch(&launch.launch, catalog, &exec_checker);

        all_resolved.push((
            launch.col_index,
            launch.pane_index,
            resolved.clone(),
            launch.launch.clone(),
        ));

        if let Some(unit_id) = pane_units.get(&(launch.col_index, launch.pane_index)) {
            if let ResolvedPaneLaunch::Executable {
                ref command,
                ref program,
                ref args,
                ref env,
                ref program_path,
                ..
            } = resolved
            {
                // Narrow visible-keystroke policy:
                //
                // * `Tool` launches are wrapped by the supervisor
                //   instrumentation port and installed via
                //   `respawn_unit_with_command` so the
                //   `workhive internal pane-supervisor ...` wrapper never
                //   appears in pane scrollback.
                //   Wrapped tools become the pane's primary process under
                //   the supervisor — that is the supervisor's
                //   fail-closed contract.
                //
                // * `InlineCommand` launches stay on the visible
                //   `send_input` path. The user typed the command into
                //   their layout config; echoing it is normal, and
                //   keeping it inside an interactive shell preserves the
                //   pane lifecycle (shell remains alive after the
                //   command exits) that the original implementation
                //   provided. Re-routing inline commands through
                //   `respawn-pane -k` would silently change pane
                //   lifecycle and is out of scope for this bug fix.
                use workhive_core::domain::PaneLaunchRef;
                match &launch.launch {
                    PaneLaunchRef::Tool { .. } => {
                        let wrapped = wrap_with_supervisor_or_passthrough(
                            instrumentation,
                            server,
                            session,
                            Some(unit_id),
                            &launch.launch,
                            program,
                            args,
                            env,
                            program_path.as_deref(),
                            command,
                        )?;
                        // Always use base_cwd (project root) for tool launches.
                        // This ensures tools start in the correct directory.
                        pane_runtime
                            .respawn_unit_with_command(server, session, unit_id, base_cwd, &wrapped)
                            .map_err(|e| {
                                CliError::Runtime(format!(
                                    "respawn_unit_with_command failed for pane \
                                     ({}, {}): {e}",
                                    launch.col_index, launch.pane_index
                                ))
                            })?;
                    }
                    PaneLaunchRef::InlineCommand { .. } => {
                        pane_runtime.send_input(SendInputRequest {
                            group: server.to_owned(),
                            target_unit: unit_id.clone(),
                            text: command.clone(),
                            enter: true,
                        })?;
                    }
                    PaneLaunchRef::EmptyShell => {
                        // `EmptyShell` should not produce an `Executable`
                        // launch in the resolver pipeline, but if it ever
                        // does we deliberately install nothing rather
                        // than typing a synthesized command.
                    }
                }
            }

            let title = pane_title_for_launch(&launch.launch, &resolved);
            pane_titles.push((unit_id.clone(), title.clone()));
            if let Err(e) = pane_runtime.set_unit_title(SetUnitTitleRequest {
                group: server.to_owned(),
                target_unit: unit_id.clone(),
                title,
            }) {
                tracing::warn!(
                    error = %e,
                    col = launch.col_index,
                    pane = launch.pane_index,
                    "failed to set pane title, continuing"
                );
            }
        }
    }

    Ok(LayoutOutput {
        pane_count: plan.total_panes,
        all_resolved,
        pane_units,
        pane_titles,
    })
}

/// Re-assert Workhive-owned pane titles for every known unit in `(server, session)`.
///
/// This defends against runtime programs (most notably Codex / shell prompt
/// integrations) that overwrite the terminal title — and therefore tmux's
/// `#{pane_title}` — while they boot. Workhive treats `#{pane_title}` as
/// authoritative for pane-border, status-right, and window-status display, so
/// any external write would mislabel the pane (e.g. show the project name
/// instead of `codex`). Re-applying the same titles after the readiness wait
/// and immediately before `attach_workspace` keeps the user-visible identity
/// stable.
///
/// `pane_titles` carries `(UnitId, title)` pairs that Workhive already wrote
/// during this layout application. We intentionally do **not** derive titles
/// by reading what the runtime currently shows: the assertion is a pure
/// write-back from Workhive's authoritative layout configuration.
///
/// Failures for individual panes are downgraded to `tracing::warn!` so we
/// never fail the whole `open` flow over a cosmetic title write; the user
/// still gets attached.
pub(crate) fn reassert_managed_pane_titles(
    pane_runtime: &dyn PaneRuntimePort,
    server: &str,
    pane_titles: &[(workhive_core::domain::UnitId, String)],
) {
    use workhive_core::ports::SetUnitTitleRequest;
    if pane_titles.is_empty() {
        return;
    }
    tracing::debug!(
        count = pane_titles.len(),
        server,
        "reasserting workhive-owned pane titles before attach"
    );
    for (unit_id, title) in pane_titles {
        if let Err(e) = pane_runtime.set_unit_title(SetUnitTitleRequest {
            group: server.to_owned(),
            target_unit: unit_id.clone(),
            title: title.clone(),
        }) {
            tracing::warn!(
                error = %e,
                unit_id = %unit_id,
                title = %title,
                "failed to reassert pane title before attach, continuing"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    #[test]
    fn apply_layout_uses_native_materialization_and_keeps_inline_commands_as_input() {
        use workhive_core::domain::UnitId;
        use workhive_core::ports::{
            ConfiguredLayoutNode, LayoutPanePosition, MaterializedConfiguredLayout,
            MaterializedLayoutPane, RuntimeUnitSnapshot,
        };

        let runtime = base_runtime();
        runtime.set_materialized_layout(MaterializedConfiguredLayout {
            panes: vec![
                MaterializedLayoutPane {
                    position: LayoutPanePosition { column: 0, row: 0 },
                    unit: RuntimeUnitSnapshot {
                        id: UnitId::new("opaque-a"),
                        index: 0,
                        cwd: Utf8PathBuf::from("/tmp/worktree"),
                        command: String::new(),
                        active: true,
                        kind: workhive_core::domain::PaneKind::Terminal,
                        state: workhive_core::domain::PaneState::Idle,
                        tty: None,
                        process_pid: None,
                    },
                },
                MaterializedLayoutPane {
                    position: LayoutPanePosition { column: 0, row: 1 },
                    unit: RuntimeUnitSnapshot {
                        id: UnitId::new("opaque-b"),
                        index: 1,
                        cwd: Utf8PathBuf::from("/tmp/worktree"),
                        command: String::new(),
                        active: false,
                        kind: workhive_core::domain::PaneKind::Terminal,
                        state: workhive_core::domain::PaneState::Idle,
                        tty: None,
                        process_pid: None,
                    },
                },
                MaterializedLayoutPane {
                    position: LayoutPanePosition { column: 1, row: 0 },
                    unit: RuntimeUnitSnapshot {
                        id: UnitId::new("opaque-c"),
                        index: 2,
                        cwd: Utf8PathBuf::from("/tmp/worktree"),
                        command: String::new(),
                        active: false,
                        kind: workhive_core::domain::PaneKind::Terminal,
                        state: workhive_core::domain::PaneState::Idle,
                        tty: None,
                        process_pid: None,
                    },
                },
            ],
        });
        let layout = LayoutConfig {
            template: LayoutTemplate::Custom,
            columns: vec![
                ColumnConfig {
                    width: 60,
                    panes: vec![
                        PaneConfig {
                            height: 70,
                            launch: PaneLaunchRef::EmptyShell,
                        },
                        PaneConfig {
                            height: 30,
                            launch: PaneLaunchRef::EmptyShell,
                        },
                    ],
                },
                ColumnConfig {
                    width: 40,
                    panes: vec![PaneConfig {
                        height: 100,
                        launch: PaneLaunchRef::InlineCommand {
                            spec: CommandSpec::new("echo", ["hello"]),
                        },
                    }],
                },
            ],
        };

        let output = apply_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &FakeI18n,
            Locale::En,
            None,
        )
        .unwrap();

        assert!(runtime.spawn_requests().is_empty());
        assert_eq!(output.pane_units[&(0, 0)].as_str(), "opaque-a");
        assert_eq!(output.pane_units[&(0, 1)].as_str(), "opaque-b");
        assert_eq!(output.pane_units[&(1, 0)].as_str(), "opaque-c");
        assert_eq!(
            runtime.sent_inputs(),
            vec![("opaque-c".to_owned(), "echo hello".to_owned())]
        );
        let requests = runtime.materialized_layout_requests();
        assert_eq!(requests.len(), 1);
        let ConfiguredLayoutNode::Split { ratio, first, .. } = &requests[0].root else {
            panic!("expected horizontal split")
        };
        assert!((ratio - 0.4).abs() < f64::EPSILON);
        let ConfiguredLayoutNode::Split { ratio, .. } = first.as_ref() else {
            panic!("expected vertical split")
        };
        assert!((ratio - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn native_layout_defers_supervised_tool_unit_identity_to_runtime() {
        use workhive_core::domain::UnitId;
        use workhive_core::ports::{
            ConfiguredLayoutNode, LayoutPanePosition, MaterializedConfiguredLayout,
            MaterializedLayoutPane, RuntimeUnitSnapshot,
        };
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        runtime.set_materialized_layout(MaterializedConfiguredLayout {
            panes: vec![MaterializedLayoutPane {
                position: LayoutPanePosition { column: 0, row: 0 },
                unit: RuntimeUnitSnapshot {
                    id: UnitId::new("opaque-tool"),
                    index: 0,
                    cwd: Utf8PathBuf::from("/tmp/worktree"),
                    command: "supervisor".to_owned(),
                    active: true,
                    kind: workhive_core::domain::PaneKind::Agent,
                    state: workhive_core::domain::PaneState::Running,
                    tty: None,
                    process_pid: None,
                },
            }],
        });
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = LayoutConfig {
            template: LayoutTemplate::Custom,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::Tool {
                        name: ToolName::new("claude").unwrap(),
                    },
                }],
            }],
        };

        apply_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &FakeI18n,
            Locale::En,
            Some(&instrumentation),
        )
        .unwrap();

        let captured = instrumentation.captured();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].unit_id.is_none());
        assert!(runtime.sent_inputs().is_empty());
        let requests = runtime.materialized_layout_requests();
        let ConfiguredLayoutNode::Pane(pane) = &requests[0].root else {
            panic!("expected one native pane")
        };
        let command = pane.command.as_deref().expect("supervised command");
        assert!(command.contains("pane-supervisor"));
        assert!(!command.contains("--unit"));
    }

    #[test]
    fn apply_layout_uses_expected_split_percentages_for_dual_layout() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::AiCodingDual,
            columns: vec![
                ColumnConfig {
                    width: 50,
                    panes: vec![
                        PaneConfig {
                            height: 70,
                            launch: PaneLaunchRef::EmptyShell,
                        },
                        PaneConfig {
                            height: 30,
                            launch: PaneLaunchRef::EmptyShell,
                        },
                    ],
                },
                ColumnConfig {
                    width: 50,
                    panes: vec![PaneConfig {
                        height: 100,
                        launch: PaneLaunchRef::EmptyShell,
                    }],
                },
            ],
        };

        apply_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("layout should apply");

        let requests = runtime.spawn_requests();
        assert_eq!(requests.len(), 2, "3 panes should need 2 splits");
        assert!(
            requests
                .iter()
                .all(|request| request.cwd.as_deref()
                    == Some(camino::Utf8Path::new("/tmp/worktree"))),
            "layout splits must use the open plan working_dir, got {requests:?}"
        );
        assert_eq!(
            requests[0].size_percent,
            Some(50),
            "column split should be 50/50"
        );
        assert_eq!(
            requests[1].size_percent,
            Some(30),
            "left row split should be 70/30"
        );
    }

    #[test]
    fn apply_layout_marks_degraded_tool_pane_title() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::HalfSplit,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: PaneLaunchRef::Tool {
                        name: ToolName::new("codex").expect("valid tool name"),
                    },
                }],
            }],
        };

        apply_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/worktree"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("layout should apply in degraded mode");

        let titles = runtime.title_updates();
        assert!(
            titles.iter().any(|(_, title)| title == "codex (degraded)"),
            "degraded tool panes should carry a degraded marker in display title"
        );
    }

    /// `reassert_managed_pane_titles` must call `set_unit_title` once per
    /// (unit_id, title) tuple, in order, so that runtime tools that
    /// rewrote the title (e.g. an LLM agent setting its own pane title)
    /// have the Workhive-owned title reasserted before the user sees the
    /// session.
    #[test]
    fn reassert_managed_pane_titles_calls_runtime_for_each_entry() {
        use workhive_core::domain::UnitId;
        let runtime = base_runtime();
        let titles = vec![
            (UnitId::new("u1"), "codex".to_owned()),
            (UnitId::new("u2"), "claude (degraded)".to_owned()),
        ];

        super::reassert_managed_pane_titles(&runtime, "srv", &titles);

        let updates = runtime.title_updates();
        assert_eq!(
            updates,
            vec![
                ("u1".to_owned(), "codex".to_owned()),
                ("u2".to_owned(), "claude (degraded)".to_owned()),
            ],
            "every Workhive-owned title must be re-applied before attach"
        );
    }

    /// Empty input is a no-op — we must not push spurious requests to the
    /// runtime.
    #[test]
    fn reassert_managed_pane_titles_is_noop_for_empty_list() {
        let runtime = base_runtime();
        super::reassert_managed_pane_titles(&runtime, "srv", &[]);
        assert!(
            runtime.title_updates().is_empty(),
            "no titles ⇒ no runtime calls"
        );
    }
}

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

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

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_layout(
    pane_runtime: &dyn PaneRuntimePort,
    snapshot_runtime: &dyn workhive_core::ports::SnapshotRuntimePort,
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

    // Track all created panes by (col_index, pane_index) plus their
    // current cwd so the typed launch verb can supply `-c <cwd>`.
    let mut pane_units: HashMap<(usize, usize), UnitId> = HashMap::new();
    let mut pane_cwds: HashMap<(usize, usize), camino::Utf8PathBuf> = HashMap::new();
    pane_units.insert((0, 0), initial_unit_id.clone());
    pane_cwds.insert((0, 0), initial_cwd.clone());

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
                pane_cwds.insert((step.col_index, 0), new_unit.cwd.clone());
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
                pane_cwds.insert((step.col_index, step.pane_index), new_unit.cwd.clone());
                pane_units.insert((step.col_index, step.pane_index), new_unit.id);
            }
        }
    }

    // Resolve all launches through the unified pipeline.
    let composition = CliComposition::new();
    let exec_checker = composition.executable_checker();
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
                            unit_id,
                            &launch.launch,
                            program,
                            args,
                            program_path.as_deref(),
                            command,
                        )?;
                        let cwd = pane_cwds
                            .get(&(launch.col_index, launch.pane_index))
                            .cloned()
                            .unwrap_or_else(|| initial_cwd.clone());
                        snapshot_runtime
                            .respawn_unit_with_command(
                                server,
                                session,
                                unit_id,
                                cwd.as_path(),
                                &wrapped,
                            )
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
/// during layout application or snapshot restore. We intentionally do **not**
/// derive titles by reading what tmux currently shows — the assertion is a
/// pure write-back from Workhive's authoritative state.
///
/// Failures for individual panes are downgraded to `tracing::warn!` so we
/// never fail the whole `open` flow over a cosmetic title write; the user
/// still gets attached.
/// Best-effort: append registry-derived `(unit_id, title)` pairs for the
/// given workspace into `out`. Used after snapshot restore to ensure each
/// restored pane has a Workhive-owned title to reassert.
///
/// Pulls the launch identity from each registered descriptor's
/// `launch_origin` and uses [`PaneLaunchRef::display_name`] — same source
/// the fresh-layout path uses. We deliberately do **not** read tmux's
/// captured `title` field, since a runtime tool may have polluted it
/// (the bug class this whole helper exists to fix).
///
/// Failures are logged at warn level and swallowed: title reassertion is
/// nice-to-have polish for the user-visible chrome and must never block
/// `open`.
pub(crate) fn extend_managed_titles_from_registry(
    pane_registry: &dyn PaneRegistryPort,
    _server: &str,
    workspace: &str,
    out: &mut Vec<(workhive_core::domain::UnitId, String)>,
) {
    match pane_registry.list_workspace(workspace) {
        Ok(descriptors) => {
            for d in descriptors {
                // Skip duplicates that the layout path already populated.
                if out.iter().any(|(u, _)| u == &d.unit_id) {
                    continue;
                }
                let title = d.launch_origin.display_name().to_owned();
                out.push((d.unit_id, title));
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                workspace,
                "could not list pane descriptors for title reassertion"
            );
        }
    }
}

/// After `try_restore_snapshot` returns `SnapshotRestored`, the in-memory
/// pane registry holds at most an `EmptyShell` default per pane (the
/// runtime sync sets `launch_origin = EmptyShell` because tmux exposes
/// no Workhive-owned launch metadata). Without this helper, a snapshot
/// reopen would surface the literal `shell` title for every restored
/// pane and `pane list` / `pane show` would classify everything as a
/// terminal — the very regression this path prevents.
///
/// We re-read the latest snapshot from disk (the same one
/// `try_restore_snapshot` just consumed) and, for each snapshot pane,
/// upsert a [`PaneDescriptor`] into the in-memory registry **plus**
/// mirror it into the durable launch-identity store. The descriptors
/// carry the snapshot's `launch_intent` mapped through
/// [`launch_intent_to_origin`], which already reflects the freshness
/// rule applied during `hydrate_snapshot_with_baseline`: panes the
/// user exited to a shell stay `EmptyShell`; running tools/inline
/// commands keep their identity.
///
/// Contract:
///
///   * Pane positions MUST be matched to live `UnitId`s by the full
///     `(window_index, pane_index)` tuple. Single-axis `pane_index`
///     matching aliases `(W0,P0)` with `(W1,P0)` and would clobber
///     identity across windows. We use
///     `SnapshotRuntimePort::capture_topology` because its
///     `RuntimePaneSnapshot` carries both indices natively (the
///     `PaneRuntimePort::list_units` summary intentionally does not).
///   * Descriptor `state` and `observed_status` MUST mirror the
///     snapshot's observed reality (already filtered by the freshness
///     rule). They MUST NOT be unconditionally set to `Running`.
///
/// If runtime topology capture fails the helper logs a warning and
/// returns silently — registry repopulation is best-effort and must
/// never block `open`.
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
    /// have the Workhive-owned title restored before the user sees the
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

    /// `extend_managed_titles_from_registry` must populate the title list
    /// from `PaneDescriptor::launch_origin` (the Workhive-owned launch
    /// intent) and skip entries whose `unit_id` is already covered by a
    /// fresh layout pass.
    #[test]
    fn extend_managed_titles_from_registry_populates_and_dedupes() {
        use workhive_core::domain::{
            PaneDescriptor, PaneKind, PaneLaunchOrigin, PaneRuntimeId, PaneRuntimeStatus,
            PaneState, UnitId, derive_capabilities,
        };
        use workhive_testkit::fakes::FakePaneRegistry;

        let registry = FakePaneRegistry::new();
        let workspace = "ws";
        for (unit, name) in [("u1", "codex"), ("u2", "claude")] {
            let runtime_id = PaneRuntimeId::derive("srv", workspace, unit);
            let descriptor = PaneDescriptor {
                runtime_id,
                group: "srv".to_owned(),
                workspace: workspace.to_owned(),
                unit_id: UnitId::new(unit),
                cwd: String::new(),
                command: String::new(),
                kind: PaneKind::Agent,
                state: PaneState::Idle,
                observed_status: PaneRuntimeStatus::Idle,
                role: None,
                launch_origin: PaneLaunchOrigin::Tool {
                    name: ToolName::new(name).unwrap(),
                },
                capabilities: derive_capabilities(PaneKind::Agent),
            };
            registry
                .upsert(descriptor)
                .expect("FakePaneRegistry upserts descriptors");
        }

        // Pre-seed `out` with a layout-derived entry so we can prove that
        // the registry helper does not duplicate already-known unit_ids.
        let mut out: Vec<(UnitId, String)> = vec![(UnitId::new("u1"), "codex".to_owned())];

        super::extend_managed_titles_from_registry(&registry, "srv", workspace, &mut out);

        assert_eq!(
            out.len(),
            2,
            "duplicate unit_id must not double-up the title list"
        );
        assert!(
            out.iter().any(|(u, t)| u.as_str() == "u1" && t == "codex"),
            "layout-seeded title must be preserved verbatim"
        );
        assert!(
            out.iter().any(|(u, t)| u.as_str() == "u2" && t == "claude"),
            "registry-only descriptor must be appended with its launch_origin display name"
        );
    }
}

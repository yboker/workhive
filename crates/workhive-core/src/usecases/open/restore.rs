#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};

#[allow(unused_imports)]
use camino::Utf8PathBuf;

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    CommandSpec, MaterializationKind, PaneDirection, PaneLaunchIntent, PaneLaunchRef,
    ReattachOutcome, RuntimeBackendId, SessionAvailability, ToolCatalog, ToolName, WorkspaceId,
    WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider, classify_reattach_outcome,
};
#[allow(unused_imports)]
use crate::errors::SnapshotError;
#[allow(unused_imports)]
use crate::ports::{
    CreateWorkspaceRuntimeRequest, ExecutableAvailabilityPort, PaneRuntimePort, ProvisionError,
    ProvisionOutcome, ProvisionWorkspaceRequest, RuntimeError, SendInputRequest,
    SnapshotPaneRuntimePort, SnapshotRuntimePort, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// Runtime context required by snapshot restore — groups the parameters that
/// `try_restore_snapshot` previously accepted as 8 individual arguments.
pub(crate) struct SnapshotRestoreContext<'a> {
    pub(crate) pane_runtime: &'a dyn PaneRuntimePort,
    pub(crate) snapshot_runtime: &'a dyn SnapshotRuntimePort,
    pub(crate) snapshot_store: &'a dyn SnapshotStorePort,
    pub(crate) server: &'a str,
    pub(crate) session: &'a str,
    pub(crate) workspace: &'a WorkspaceId,
    pub(crate) identity_ctx: Option<&'a SnapshotIdentityContext>,
    pub(crate) catalog: &'a ToolCatalog,
    pub(crate) exec_checker: &'a dyn ExecutableAvailabilityPort,
    pub(crate) instrumentation: Option<&'a dyn crate::ports::ToolLaunchInstrumentationPort>,
}

/// Attempt snapshot restore; always returns an explicit `SnapshotOutcome` — never silent.
///
/// Performs validation before any runtime mutation. If validation fails,
/// returns `Stale` without touching the runtime.
pub(crate) fn try_restore_snapshot(ctx: &SnapshotRestoreContext<'_>) -> SnapshotOutcome {
    let Some(state_key) = ctx.identity_ctx.and_then(|ic| ic.state_key.as_ref()) else {
        return SnapshotOutcome::Stale {
            reason: "missing state_key for snapshot restore".to_owned(),
        };
    };

    let snapshot = match ctx.snapshot_store.load_latest(state_key) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            tracing::info!(
                workspace = %ctx.workspace,
                "no snapshot found; layout fallback required"
            );
            return SnapshotOutcome::NotFound;
        }
        Err(e) => {
            let reason = e.to_string();
            tracing::warn!(
                workspace = %ctx.workspace,
                error = %e,
                "snapshot load failed; layout fallback required"
            );
            return SnapshotOutcome::Failed { reason };
        }
    };

    if snapshot.state_key != *state_key {
        return SnapshotOutcome::Stale {
            reason: "snapshot state_key mismatch".to_owned(),
        };
    }

    if let Some(identity) = ctx.identity_ctx
        && snapshot.metadata.runtime_backend != identity.runtime_backend.as_str()
    {
        return SnapshotOutcome::Stale {
            reason: format!(
                "runtime backend mismatch: snapshot='{}' current='{}'",
                snapshot.metadata.runtime_backend, identity.runtime_backend
            ),
        };
    }

    match ctx
        .snapshot_store
        .verify_revision(state_key, snapshot.revision_id)
    {
        Ok(true) => {}
        Ok(false) => {
            return SnapshotOutcome::Stale {
                reason: format!(
                    "revision checksum/identity verification failed for {}",
                    snapshot.revision_id
                ),
            };
        }
        Err(e) => {
            return SnapshotOutcome::Failed {
                reason: format!("verify_revision failed: {e}"),
            };
        }
    }

    let mut plan = match crate::usecases::snapshot_restore::build_restore_plan(&snapshot, false) {
        Ok(plan) => plan,
        Err(e) => {
            return SnapshotOutcome::Failed {
                reason: format!("failed to build restore plan: {e}"),
            };
        }
    };

    // F1: Reconcile plan with prior baseline BEFORE execution so that
    // NonRestorable/EmptyShell panes recover their launch commands now.
    let prior_baseline = match crate::usecases::snapshot_baseline::build_prior_baseline_map(
        ctx.snapshot_store,
        state_key,
        &snapshot,
    ) {
        Ok(prior_baseline) => prior_baseline,
        Err(e) => {
            return SnapshotOutcome::Failed {
                reason: format!("load launch baseline failed: {e}"),
            };
        }
    };
    // The baseline reconciliation must skip positions where the snapshot
    // observed a shell — that means the user explicitly exited the tool
    // and the prior baseline is stale.
    let stale_positions =
        crate::usecases::snapshot_restore::stale_tool_positions_from_snapshot(&snapshot);
    let recovered = crate::usecases::snapshot_restore::reconcile_plan_with_baseline(
        &mut plan,
        &prior_baseline,
        &stale_positions,
    );
    if recovered > 0 {
        tracing::info!(
            recovered,
            stale_positions = stale_positions.len(),
            "reconciled {recovered} pane launch intents from prior baseline"
        );
    }

    if let Err(e) = crate::usecases::snapshot_restore::validate_restore_plan(&plan) {
        return SnapshotOutcome::Stale {
            reason: format!("invalid restore plan: {e}"),
        };
    }

    let fidelity = crate::usecases::snapshot_restore::compute_plan_fidelity(&plan);
    match execute_restore_plan(ctx, &snapshot, &plan) {
        Ok(count) => SnapshotOutcome::Restored {
            panes: count,
            fidelity,
        },
        Err(reason) => SnapshotOutcome::Failed { reason },
    }
}

pub(crate) fn execute_restore_plan(
    ctx: &SnapshotRestoreContext<'_>,
    snapshot: &crate::domain::WorkspaceSnapshot,
    plan: &crate::domain::RestorePlan,
) -> Result<u32, String> {
    if plan.dry_run {
        return Ok(plan.pane_count() as u32);
    }

    let mut unit_by_position: HashMap<(u32, u32), crate::domain::UnitId> = HashMap::new();
    let mut current_topology = ctx
        .snapshot_runtime
        .capture_topology(ctx.server, ctx.session)
        .map_err(|e| format!("capture_topology failed: {e}"))?;
    for pane in &current_topology.panes {
        unit_by_position.insert((pane.window_index, pane.pane_index), pane.unit_id.clone());
    }

    let mut window_actions = plan.window_actions.clone();
    window_actions.sort_by_key(|action| match action {
        crate::domain::WindowRestoreAction::Create { index, .. } => *index,
    });

    for window_action in &window_actions {
        let crate::domain::WindowRestoreAction::Create {
            index,
            name,
            layout,
            pane_actions,
        } = window_action;

        let mut create_actions = pane_actions
            .iter()
            .filter_map(|action| match action {
                crate::domain::PaneRestoreAction::Create {
                    pane_index,
                    cwd,
                    launch_intent,
                    title,
                    ..
                } => Some((*pane_index, cwd, launch_intent, title)),
                crate::domain::PaneRestoreAction::Skip { .. } => None,
            })
            .collect::<Vec<_>>();
        create_actions.sort_by_key(|(pane_index, _, _, _)| *pane_index);

        if create_actions.is_empty() {
            continue;
        }

        let mut window_units = unit_by_position
            .iter()
            .filter(|((window_index, _), _)| *window_index == *index)
            .map(|((_, pane_index), unit_id)| (*pane_index, unit_id.clone()))
            .collect::<BTreeMap<_, _>>();

        let split_count = create_actions.len().saturating_sub(1);
        let split_instructions = if split_count > 0 {
            match parse_layout(layout) {
                Some(tree) if tree.leaf_count() > 1 => {
                    let instructions = derive_split_instructions(&tree);
                    if instructions.len() >= split_count {
                        instructions
                    } else {
                        fallback_split_instructions(split_count)
                    }
                }
                _ => fallback_split_instructions(split_count),
            }
        } else {
            Vec::new()
        };

        let (first_pane_index, first_cwd, first_launch, first_title) = create_actions[0];
        let first_pane_was_reused;
        let first_unit = if let Some(unit) = window_units.get(&first_pane_index) {
            first_pane_was_reused = true;
            unit.clone()
        } else {
            first_pane_was_reused = false;
            let created = ctx
                .snapshot_runtime
                .create_window(ctx.server, ctx.session, *index, name.as_deref(), first_cwd)
                .map_err(|e| format!("create_window({index}) failed: {e}"))?;
            current_topology.panes.push(created.clone());
            unit_by_position.insert((*index, created.pane_index), created.unit_id.clone());
            window_units.insert(created.pane_index, created.unit_id.clone());
            created.unit_id
        };
        window_units.insert(first_pane_index, first_unit.clone());
        unit_by_position.insert((*index, first_pane_index), first_unit.clone());
        let first_observed_cwd = if first_pane_was_reused {
            current_topology
                .panes
                .iter()
                .find(|p| p.unit_id == first_unit)
                .map(|p| p.cwd.clone())
        } else {
            None
        };
        let first_repair = if first_pane_was_reused {
            ReusedPaneCwdRepair::Repair {
                session: ctx.session,
                observed_cwd: first_observed_cwd.as_deref(),
            }
        } else {
            ReusedPaneCwdRepair::Skip
        };
        restore_runtime_pane(RestoreRuntimePaneRequest {
            pane_runtime: ctx.pane_runtime,
            snapshot_runtime: ctx.snapshot_runtime,
            server: ctx.server,
            session: ctx.session,
            unit_id: first_unit.clone(),
            pane_index: first_pane_index,
            cwd: first_cwd,
            launch_intent: first_launch,
            title: first_title.as_deref(),
            catalog: ctx.catalog,
            exec_checker: ctx.exec_checker,
            cwd_repair: first_repair,
            instrumentation: ctx.instrumentation,
        })?;

        // Track units in creation order so split_target_leaf_index can
        // resolve to the correct existing pane.
        let mut units_in_creation_order: Vec<crate::domain::UnitId> = vec![first_unit.clone()];

        for (split_index, (pane_index, cwd, launch_intent, title)) in
            create_actions.into_iter().skip(1).enumerate()
        {
            if let Some(existing_unit) = window_units.get(&pane_index).cloned() {
                unit_by_position.insert((*index, pane_index), existing_unit.clone());
                units_in_creation_order.push(existing_unit.clone());
                let observed_cwd = current_topology
                    .panes
                    .iter()
                    .find(|p| p.unit_id == existing_unit)
                    .map(|p| p.cwd.clone());
                restore_runtime_pane(RestoreRuntimePaneRequest {
                    pane_runtime: ctx.pane_runtime,
                    snapshot_runtime: ctx.snapshot_runtime,
                    server: ctx.server,
                    session: ctx.session,
                    unit_id: existing_unit,
                    pane_index,
                    cwd,
                    launch_intent,
                    title: title.as_deref(),
                    catalog: ctx.catalog,
                    exec_checker: ctx.exec_checker,
                    cwd_repair: ReusedPaneCwdRepair::Repair {
                        session: ctx.session,
                        observed_cwd: observed_cwd.as_deref(),
                    },
                    instrumentation: ctx.instrumentation,
                })?;
                continue;
            }

            let instruction =
                split_instructions
                    .get(split_index)
                    .cloned()
                    .unwrap_or(SplitInstruction {
                        split_target_leaf_index: units_in_creation_order.len().saturating_sub(1),
                        direction: PaneDirection::Down,
                    });
            let split_target = units_in_creation_order
                .get(instruction.split_target_leaf_index)
                .cloned()
                .unwrap_or_else(|| {
                    units_in_creation_order
                        .last()
                        .cloned()
                        .expect("at least one pane exists")
                });
            // Carry the captured cwd into the adapter-level `split-window -c`
            // so the new pane starts in the right directory without a visible
            // `cd --` keystroke. Only the `cwd` is considered internal prep
            // here; the launch command (tool wrapper or inline command) is
            // installed by `restore_runtime_pane` below: tool launches go
            // through the typed `respawn_unit_with_command` verb so the
            // internal `workhive internal pane-supervisor ...` wrapper never
            // appears in pane scrollback, while inline/raw commands remain
            // on the visible `send_input` path to preserve the user-typed
            // command echo and shell-retained pane lifecycle.
            let prep = crate::ports::SplitPanePreparation {
                cwd: Some(cwd.clone()),
                command: None,
            };
            let created = ctx
                .snapshot_runtime
                .split_pane(
                    ctx.server,
                    ctx.session,
                    &split_target,
                    instruction.direction,
                    prep,
                )
                .map_err(|e| {
                    format!(
                        "split_pane(window={index}, pane={pane_index}, direction={}) failed: {e}",
                        instruction.direction
                    )
                })?;
            window_units.insert(pane_index, created.unit_id.clone());
            unit_by_position.insert((*index, pane_index), created.unit_id.clone());
            units_in_creation_order.push(created.unit_id.clone());
            restore_runtime_pane(RestoreRuntimePaneRequest {
                pane_runtime: ctx.pane_runtime,
                snapshot_runtime: ctx.snapshot_runtime,
                server: ctx.server,
                session: ctx.session,
                unit_id: created.unit_id,
                pane_index,
                cwd,
                launch_intent,
                title: title.as_deref(),
                catalog: ctx.catalog,
                exec_checker: ctx.exec_checker,
                cwd_repair: ReusedPaneCwdRepair::Skip,
                instrumentation: ctx.instrumentation,
            })?;
        }

        ctx.snapshot_runtime
            .apply_window_layout(ctx.server, ctx.session, *index, layout)
            .map_err(|e| format!("apply_window_layout({index}) failed: {e}"))?;
    }

    if let Some(active_window) = plan.active_window {
        ctx.snapshot_runtime
            .select_window(ctx.server, ctx.session, active_window)
            .map_err(|e| format!("select_window({active_window}) failed: {e}"))?;
    }

    if let Some(active_runtime_id) = snapshot.topology.active_pane_runtime_id.as_deref()
        && let Some(active_snapshot_pane) = snapshot
            .panes
            .iter()
            .find(|pane| pane.runtime_id.as_deref() == Some(active_runtime_id))
        && let Some(unit_id) = unit_by_position.get(&(
            active_snapshot_pane.window_index,
            active_snapshot_pane.pane_index,
        ))
    {
        ctx.snapshot_runtime
            .select_pane(ctx.server, unit_id)
            .map_err(|e| format!("select_pane failed: {e}"))?;
    }

    if let Some(state_key) = ctx
        .identity_ctx
        .and_then(|identity| identity.state_key.as_ref())
    {
        let prior_baseline = crate::usecases::snapshot_baseline::build_prior_baseline_map(
            ctx.snapshot_store,
            state_key,
            snapshot,
        )
        .map_err(|e| format!("load launch baseline failed: {e}"))?;
        let entries = crate::usecases::snapshot_baseline::entries_from_restored_snapshot(
            snapshot,
            &unit_by_position,
            ctx.server,
            ctx.session,
            &prior_baseline,
        );
        crate::usecases::snapshot_baseline::persist_launch_baseline(
            ctx.snapshot_store,
            state_key,
            entries,
            crate::usecases::snapshot_baseline::launch_baseline_timestamp_now(),
        )
        .map_err(|e| format!("persist launch baseline failed: {e}"))?;
    }

    Ok(plan.pane_count() as u32)
}

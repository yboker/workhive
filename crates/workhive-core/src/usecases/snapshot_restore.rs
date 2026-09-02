//! Snapshot restore use case — declarative, idempotent, rollback-capable.
//!
//! Restore flow (from architecture doc):
//! 1. Load manifest
//! 2. Load latest full revision
//! 3. Migrate and normalize to latest internal model
//! 4. Validate state key and runtime compatibility
//! 5. Build restore plan
//! 6. Execute plan (create windows, rebuild pane topology, apply layout)
//! 7. Post-restore reconcile
//! 8. Rollback on failure

use crate::domain::{
    PaneLaunchIntent, PaneRestoreAction, PaneSnapshot, RestoreFidelity, RestoreOutcome,
    RestorePlan, WindowRestoreAction, WorkspaceSnapshot,
};
use crate::errors::SnapshotError;

/// Build a declarative restore plan from a workspace snapshot.
///
/// The plan describes what needs to happen without executing anything.
/// Use `dry_run = true` to preview without side effects.
pub fn build_restore_plan(
    snapshot: &WorkspaceSnapshot,
    dry_run: bool,
) -> Result<RestorePlan, SnapshotError> {
    let mut window_actions = Vec::new();

    for window in &snapshot.topology.windows {
        let window_panes: Vec<&PaneSnapshot> = snapshot
            .panes
            .iter()
            .filter(|p| p.window_index == window.index)
            .collect();

        let pane_actions: Vec<PaneRestoreAction> = window_panes
            .iter()
            .map(|pane| build_pane_action(pane))
            .collect();

        window_actions.push(WindowRestoreAction::Create {
            index: window.index,
            name: window.name.clone(),
            layout: window.layout.clone(),
            pane_actions,
        });
    }

    Ok(RestorePlan {
        state_key: snapshot.state_key.clone(),
        revision_id: snapshot.revision_id,
        window_actions,
        active_window: snapshot.topology.active_window,
        dry_run,
    })
}

/// Build a restore action for a single pane.
fn build_pane_action(pane: &PaneSnapshot) -> PaneRestoreAction {
    use crate::domain::RestorePolicy;

    match &pane.restore_policy {
        RestorePolicy::Skip => PaneRestoreAction::Skip {
            reason: "restore policy is Skip".to_owned(),
        },
        RestorePolicy::ShellOnly => PaneRestoreAction::Create {
            window_index: pane.window_index,
            pane_index: pane.pane_index,
            cwd: pane.cwd.clone(),
            launch_intent: PaneLaunchIntent::EmptyShell,
            title: pane.title.clone(),
        },
        RestorePolicy::Restore => {
            if !pane.launch_intent.is_restorable() {
                PaneRestoreAction::Create {
                    window_index: pane.window_index,
                    pane_index: pane.pane_index,
                    cwd: pane.cwd.clone(),
                    launch_intent: PaneLaunchIntent::EmptyShell,
                    title: pane.title.clone(),
                }
            } else {
                PaneRestoreAction::Create {
                    window_index: pane.window_index,
                    pane_index: pane.pane_index,
                    cwd: pane.cwd.clone(),
                    launch_intent: pane.launch_intent.clone(),
                    title: pane.title.clone(),
                }
            }
        }
    }
}

/// Validate a restore plan before execution.
///
/// Returns `Ok(())` if the plan is valid, or an error describing why not.
pub fn validate_restore_plan(plan: &RestorePlan) -> Result<(), SnapshotError> {
    if plan.is_empty() {
        return Err(SnapshotError::RestoreFailed {
            reason: "restore plan is empty — no windows or panes to restore".to_owned(),
        });
    }
    Ok(())
}

/// Summarize a restore plan as a `RestoreOutcome` with fidelity classification.
pub fn summarize_plan(plan: &RestorePlan) -> RestoreOutcome {
    let windows_count = plan.window_actions.len();
    let panes_count = plan.pane_count();
    let fidelity = compute_plan_fidelity(plan);

    if plan.dry_run {
        RestoreOutcome::DryRun {
            panes_planned: panes_count,
            windows_planned: windows_count,
            fidelity,
        }
    } else {
        RestoreOutcome::Success {
            panes_restored: panes_count,
            windows_restored: windows_count,
            fidelity,
        }
    }
}

/// Reconcile a restore plan with prior launch baseline data.
///
/// For any pane action that degrades to `EmptyShell` because the snapshot
/// had `NonRestorable` or `ShellOnly` intent, this function substitutes the
/// recovered launch intent from the prior baseline if a matching entry exists
/// at the same `(window_index, pane_index)` position.
///
/// This must be called **before** plan execution so the current open
/// restores recovered commands immediately.
///
/// **Freshness rule:**
/// for any pane position present in `stale_tool_positions`, we treat the
/// prior tool/inline baseline as **stale** and do NOT substitute it.
/// `stale_tool_positions` is derived from the snapshot's observed state:
/// a pane currently observed running a shell binary means the user
/// already exited the tool, so reusing the old baseline would silently
/// re-launch a tool the user intentionally left.
pub fn reconcile_plan_with_baseline(
    plan: &mut RestorePlan,
    prior_baseline: &std::collections::HashMap<(u32, u32), crate::domain::PaneLaunchRef>,
    stale_tool_positions: &std::collections::HashSet<(u32, u32)>,
) -> usize {
    let mut recovered = 0usize;
    let mut skipped_stale = 0usize;
    for window_action in &mut plan.window_actions {
        let WindowRestoreAction::Create { pane_actions, .. } = window_action;
        for action in pane_actions.iter_mut() {
            if let PaneRestoreAction::Create {
                window_index,
                pane_index,
                launch_intent,
                ..
            } = action
                && (!launch_intent.is_restorable()
                    || *launch_intent == PaneLaunchIntent::EmptyShell)
                && let Some(baseline_ref) = prior_baseline.get(&(*window_index, *pane_index))
                && let Some(intent) = launch_ref_to_intent(baseline_ref)
            {
                if stale_tool_positions.contains(&(*window_index, *pane_index)) {
                    tracing::info!(
                        window_index = *window_index,
                        pane_index = *pane_index,
                        stale_intent = ?intent,
                        "snapshot restore: ignored stale tool baseline; pane was a shell at capture"
                    );
                    skipped_stale += 1;
                    continue;
                }
                tracing::debug!(
                    window_index = *window_index,
                    pane_index = *pane_index,
                    new_intent = ?intent,
                    "snapshot restore: pane recovered from prior baseline"
                );
                *launch_intent = intent;
                recovered += 1;
            }
        }
    }
    if skipped_stale > 0 {
        tracing::info!(
            skipped_stale,
            recovered,
            "snapshot restore plan: skipped {skipped_stale} stale tool baselines"
        );
    }
    recovered
}

/// Compute the set of pane positions in `snapshot` whose observed state
/// is a shell binary. These positions are passed to
/// [`reconcile_plan_with_baseline`] to enforce the freshness rule.
pub fn stale_tool_positions_from_snapshot(
    snapshot: &crate::domain::WorkspaceSnapshot,
) -> std::collections::HashSet<(u32, u32)> {
    let mut out = std::collections::HashSet::new();
    for pane in &snapshot.panes {
        if observed_pane_is_shell(pane) {
            out.insert((pane.window_index, pane.pane_index));
        }
    }
    out
}

/// Heuristic: does the snapshot pane's observed `current_command` look
/// like a bare shell? Mirrors the helper in `snapshot_baseline.rs` so
/// the same rule applies on capture (`hydrate_snapshot_with_baseline`)
/// and restore (`reconcile_plan_with_baseline`).
fn observed_pane_is_shell(pane: &crate::domain::PaneSnapshot) -> bool {
    let Some(current) = pane.observed.current_command.as_deref() else {
        return false;
    };
    let basename = current
        .rsplit('/')
        .next()
        .unwrap_or(current)
        .to_ascii_lowercase();
    matches!(
        basename.as_str(),
        "zsh" | "bash" | "fish" | "sh" | "ksh" | "dash" | "ash"
    )
}

/// Convert a `PaneLaunchRef` (config/baseline model) to a `PaneLaunchIntent`
/// (snapshot/restore model). Returns `None` for `EmptyShell` since that offers
/// no recovery value.
fn launch_ref_to_intent(r: &crate::domain::PaneLaunchRef) -> Option<PaneLaunchIntent> {
    use crate::domain::PaneLaunchRef;
    match r {
        PaneLaunchRef::EmptyShell => None,
        PaneLaunchRef::Tool { name } => Some(PaneLaunchIntent::ToolRef {
            tool_name: name.as_str().to_owned(),
        }),
        PaneLaunchRef::InlineCommand { spec } => Some(PaneLaunchIntent::InlineCommand {
            program: spec.program().to_owned(),
            args: spec.args().to_vec(),
        }),
    }
}

/// Compute restore fidelity from the actions in a plan.
pub fn compute_plan_fidelity(plan: &RestorePlan) -> RestoreFidelity {
    let mut command_restored: usize = 0;
    let mut shell_only: usize = 0;

    for window_action in &plan.window_actions {
        let WindowRestoreAction::Create { pane_actions, .. } = window_action;
        for action in pane_actions {
            match action {
                PaneRestoreAction::Create { launch_intent, .. } => match launch_intent {
                    PaneLaunchIntent::EmptyShell => shell_only += 1,
                    PaneLaunchIntent::NonRestorable { .. } => shell_only += 1,
                    PaneLaunchIntent::ToolRef { .. } | PaneLaunchIntent::InlineCommand { .. } => {
                        command_restored += 1;
                    }
                },
                PaneRestoreAction::Skip { .. } => {}
            }
        }
    }

    if shell_only == 0 && command_restored > 0 {
        RestoreFidelity::LaunchRestorable {
            command_restored,
            shell_only: 0,
        }
    } else {
        // Snapshot restore with any shell-only panes is TopologyOnly.
        // LayoutFallback is reserved for the non-snapshot config-layout path.
        RestoreFidelity::TopologyOnly {
            command_restored,
            shell_only,
        }
    }
}

#[cfg(test)]
#[path = "snapshot_restore_tests.rs"]
mod tests;

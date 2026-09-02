//! Post-create phases of `workspace open`: layout application with
//! baseline pane registration, the post-layout registry sync, and the
//! interaction-mode summary line.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

use workhive_core::domain::{ResolvedPaneLaunch, UnitId, WorkspaceInteractionMode};
use workhive_core::i18n_keys as k;
use workhive_infra::runtime::TmuxRuntimeAdapter;

/// Outcome of applying the configured layout during `workspace open`.
#[derive(Default)]
pub(crate) struct LayoutApplication {
    /// Pane count produced by layout application, when a layout ran.
    pub(crate) layout_pane_count: Option<u32>,
    /// One line per degraded pane launch (position + reason).
    pub(crate) degraded_launches: Vec<String>,
    /// Workhive-owned pane titles captured this run, reasserted before
    /// attach so runtime tools cannot leave a stale terminal title.
    pub(crate) managed_pane_titles: Vec<(UnitId, String)>,
}

/// Apply the effective layout (when it defines more than one pane) and
/// register baseline pane descriptors plus durable launch identity.
pub(crate) fn apply_layout_and_register(
    ctx: &CommandContext,
    runtime: &TmuxRuntimeAdapter<'_>,
    plan: &usecases::open::OpenPlan,
    root: &camino::Utf8Path,
) -> Result<LayoutApplication, CliError> {
    let locale = ctx.locale();
    let layout = load_effective_layout(ctx, Some(root));
    if layout.total_panes() <= 1 {
        return Ok(LayoutApplication::default());
    }

    eprintln!("{}", ctx.i18n.resolve(locale, k::OPEN_APPLYING_LAYOUT));
    let catalog =
        usecases::launch_resolver::effective_catalog_from_config(&ctx.config, Some(root))?;
    let instrumentation = ctx
        .composition
        .tool_launch_instrumentation(root)
        .map_err(|e| {
            tracing::warn!(
                error = %e,
                "failed to construct tool launch instrumentation; tool panes that require layout wrapping will fail closed"
            );
            e
        })
        .ok();
    let layout_out = apply_layout(
        runtime,
        runtime,
        &plan.server,
        &plan.session,
        plan.working_dir.as_path(),
        &layout,
        &catalog,
        &ctx.i18n,
        locale,
        instrumentation
            .as_ref()
            .map(|i| i as &dyn workhive_core::ports::ToolLaunchInstrumentationPort),
    )?;

    let mut degraded_launches: Vec<String> = Vec::new();
    for (col, row, resolved, _origin) in &layout_out.all_resolved {
        if let Some(reason) = resolved.degradation_reason() {
            let position = format!("col={col} row={row}");
            degraded_launches.push(format!("{position}: {reason}"));
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_SUMMARY_TOOL_DEGRADED,
                    &[&position, &reason],
                )
            );
            tracing::warn!(
                col, row,
                reason = reason.as_str(),
                workspace = %plan.session,
                "pane launch degraded"
            );
        }
    }

    register_baseline_panes(ctx, plan, root, &layout_out);

    let count_str = layout_out.pane_count.to_string();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::OPEN_LAYOUT_APPLIED, &[&count_str])
    );

    Ok(LayoutApplication {
        layout_pane_count: Some(layout_out.pane_count),
        degraded_launches,
        managed_pane_titles: layout_out.pane_titles.clone(),
    })
}

/// Register baseline pane descriptors in the in-memory registry and mirror
/// launch identity into the durable store for the detached supervisor.
fn register_baseline_panes(
    ctx: &CommandContext,
    plan: &usecases::open::OpenPlan,
    root: &camino::Utf8Path,
    layout_out: &LayoutOutput,
) {
    use workhive_core::domain::{PaneDescriptor, PaneRuntimeId, PaneRuntimeStatus, PaneState};
    use workhive_core::ports::{PaneLaunchIdentityEntry, PaneLaunchIdentityPort, PaneRegistryPort};

    let baseline_entries =
        usecases::launch_resolver::baseline_pane_entries(&layout_out.all_resolved);
    // Durable launch-identity store: cross-process bridge for the
    // detached control-plane supervisor. The supervisor process runs in a
    // separate PID and cannot read the in-memory registry; instead it
    // hydrates from this durable store at startup.
    let identity_store = ctx.composition.pane_launch_identity_store(root);
    for entry in &baseline_entries {
        let real_unit_id = match layout_out.pane_units.get(&(entry.col, entry.row)) {
            Some(uid) => uid.clone(),
            None => {
                tracing::warn!(
                    col = entry.col,
                    row = entry.row,
                    "no runtime UnitId for pane position, skipping baseline registration"
                );
                continue;
            }
        };
        let descriptor = PaneDescriptor {
            runtime_id: PaneRuntimeId::derive(&plan.server, &plan.session, real_unit_id.as_str()),
            group: plan.server.clone(),
            workspace: plan.session.clone(),
            unit_id: real_unit_id.clone(),
            cwd: String::new(),
            command: match &entry.resolved {
                ResolvedPaneLaunch::Executable { command, .. } => command.clone(),
                _ => String::new(),
            },
            kind: match &entry.resolved {
                ResolvedPaneLaunch::Executable { baseline_kind, .. } => *baseline_kind,
                _ => workhive_core::domain::PaneKind::Terminal,
            },
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: entry.launch_origin.clone(),
            capabilities: entry.capabilities.clone(),
        };
        if let Err(e) = ctx.pane_registry.upsert(descriptor) {
            tracing::warn!(
                col = entry.col,
                row = entry.row,
                error = %e,
                "failed to register baseline pane descriptor"
            );
        } else {
            tracing::debug!(
                col = entry.col,
                row = entry.row,
                capabilities = ?entry.capabilities,
                "baseline capabilities registered into pane registry"
            );
        }

        // Mirror the launch identity into the durable store so the
        // detached supervisor can hydrate `display_title` for
        // `QueryPane`. We deliberately fail-open here: a write failure
        // becomes a degraded supervisor surface (no `display_title`)
        // rather than a blocked open path, per the fail-open principle.
        let identity_entry = PaneLaunchIdentityEntry {
            workspace: plan.session.clone(),
            unit_id: real_unit_id,
            launch_origin: entry.launch_origin.clone(),
        };
        if let Err(e) = identity_store.record(&plan.server, identity_entry) {
            tracing::warn!(
                col = entry.col,
                row = entry.row,
                server = %plan.server,
                workspace = %plan.session,
                error = %e,
                "failed to record durable launch identity; \
                 supervisor display_title may be missing for this pane"
            );
        }
    }
}

/// Reconcile baseline-registered panes with actual runtime state after
/// layout application. Sync failures degrade to warnings — the registry
/// may be stale but `open` continues.
pub(crate) fn sync_pane_registry_after_open(
    ctx: &CommandContext,
    runtime: &TmuxRuntimeAdapter<'_>,
    plan: &usecases::open::OpenPlan,
) {
    let locale = ctx.locale();
    match runtime.list_units(&plan.server, &plan.session) {
        Ok(snapshots) => {
            let observations: Vec<_> = snapshots
                .iter()
                .map(|s| {
                    usecases::pane_sync::snapshot_to_observation(s, &plan.server, &plan.session)
                })
                .collect();

            match usecases::pane_sync::sync_pane_registry(&ctx.pane_registry, observations, None) {
                Ok(result) => {
                    tracing::info!(
                        registered = result.registered,
                        updated = result.updated,
                        removed = result.removed,
                        events = result.events.len(),
                        workspace = %plan.session,
                        "runtime sync completed after layout"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        workspace = %plan.session,
                        "runtime sync failed — registry may be stale"
                    );
                    let detail = e.to_string();
                    eprintln!(
                        "{}",
                        ctx.i18n.resolve_with_args(
                            locale,
                            workhive_core::i18n_keys::OPEN_WARNING_RUNTIME_SYNC_FAILED,
                            &[&detail],
                        )
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                workspace = %plan.session,
                "could not list runtime units for post-layout sync"
            );
        }
    }
}

/// Render the localized interaction-mode summary line, building the
/// orchestration plan when primary-assisted mode is requested.
pub(crate) fn render_interaction_mode_message(
    ctx: &CommandContext,
    interaction_mode: WorkspaceInteractionMode,
    ws_id: &WorkspaceId,
) -> String {
    let locale = ctx.locale();
    let i18n = &ctx.i18n;
    match interaction_mode {
        WorkspaceInteractionMode::Manual => i18n.resolve(locale, k::OPEN_MODE_MANUAL),
        WorkspaceInteractionMode::PrimaryAssisted => {
            let orch_ws = ws_id.clone();
            match workhive_core::usecases::orchestration::build_orchestration_plan(
                &ctx.pane_registry,
                &orch_ws,
                interaction_mode,
            ) {
                Ok(plan) => {
                    if let Some(ref primary_id) = plan.primary {
                        tracing::info!(
                            primary = %primary_id.as_str(),
                            workers = plan.workers.len(),
                            "orchestration: primary-assisted mode active"
                        );
                        i18n.resolve_with_args(
                            locale,
                            k::OPEN_MODE_PRIMARY_ACTIVE,
                            &[primary_id.as_str(), &plan.workers.len().to_string()],
                        )
                    } else {
                        let reason = workhive_core::domain::DegradationReason::NoPrimaryPaneFound;
                        tracing::warn!(
                            reason = %reason,
                            "orchestration: degraded to manual mode"
                        );
                        i18n.resolve_with_args(
                            locale,
                            k::OPEN_MODE_PRIMARY_DEGRADED,
                            &[&reason.to_string()],
                        )
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to build orchestration plan");
                    i18n.resolve_with_args(locale, k::OPEN_MODE_PRIMARY_FAILED, &[&e.to_string()])
                }
            }
        }
    }
}

//! Post-create phases of `workspace open`: layout application with
//! baseline pane registration, the post-layout registry sync, and the
//! interaction-mode summary line.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

use crate::composition::RuntimeAdapter;
use workhive_core::domain::{ResolvedPaneLaunch, UnitId, WorkspaceInteractionMode};
use workhive_core::i18n_keys as k;

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

/// Apply the effective layout and register baseline pane descriptors plus
/// durable launch identity.
///
/// Delegates the pane-count gate and layout application to
/// [`orchestrate_layout`], then handles messaging, degraded-launch
/// reporting, and baseline registration.
pub(crate) fn apply_layout_and_register(
    ctx: &CommandContext,
    runtime: &dyn RuntimeAdapter,
    plan: &usecases::open::OpenPlan,
    root: &camino::Utf8Path,
) -> Result<LayoutApplication, CliError> {
    let locale = ctx.locale();
    let layout = load_effective_layout(ctx, Some(root));
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

    eprintln!("{}", ctx.i18n.resolve(locale, k::OPEN_APPLYING_LAYOUT));

    let layout_out = match orchestrate_layout(
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
    )? {
        Some(out) => out,
        None => return Ok(LayoutApplication::default()),
    };

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

/// Shared production helper: pane-count gate + layout application.
///
/// Single-pane layouts skip topology splits (the initial tmux pane is
/// reused) but still resolve and install the configured launch intent,
/// pane titles, and baseline identity — the same pipeline used by
/// multi-pane layouts.
///
/// Returns `None` when the layout has no panes (the early-return case).
#[allow(clippy::too_many_arguments)]
pub(crate) fn orchestrate_layout(
    pane_runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
    working_dir: &camino::Utf8Path,
    layout: &LayoutConfig,
    catalog: &ToolCatalog,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: workhive_core::domain::Locale,
    instrumentation: Option<&dyn workhive_core::ports::ToolLaunchInstrumentationPort>,
) -> Result<Option<super::layout::LayoutOutput>, CliError> {
    if layout.total_panes() == 0 {
        return Ok(None);
    }
    let out = super::layout::apply_layout(
        pane_runtime,
        server,
        session,
        working_dir,
        layout,
        catalog,
        i18n,
        locale,
        instrumentation,
    )?;
    Ok(Some(out))
}

/// A resolved registration entry mapping a baseline pane to its real
/// runtime UnitId.
#[derive(Debug)]
pub(crate) struct RegistrationPlanEntry {
    pub unit_id: UnitId,
    pub col: usize,
    pub row: usize,
    pub resolved: ResolvedPaneLaunch,
    pub launch_origin: workhive_core::domain::PaneLaunchRef,
    pub capabilities: std::collections::BTreeSet<workhive_core::domain::PaneCapability>,
}

/// Result of joining baseline pane entries with runtime UnitIds.
#[derive(Debug)]
pub(crate) struct RegistrationPlanResult {
    /// Entries that were successfully matched to a runtime UnitId.
    pub matched: Vec<RegistrationPlanEntry>,
    /// Baseline positions that had no corresponding runtime UnitId.
    pub missing: Vec<(usize, usize)>,
}

/// Build the registration plan by joining baseline entries with real
/// runtime UnitIds from the layout output.
///
/// Used by `register_baseline_panes` and testable independently.
pub(crate) fn build_registration_plan(layout_out: &LayoutOutput) -> RegistrationPlanResult {
    let baseline_entries =
        usecases::launch_resolver::baseline_pane_entries(&layout_out.all_resolved);
    let mut matched = Vec::new();
    let mut missing = Vec::new();
    for entry in baseline_entries {
        if let Some(uid) = layout_out.pane_units.get(&(entry.col, entry.row)) {
            matched.push(RegistrationPlanEntry {
                unit_id: uid.clone(),
                col: entry.col,
                row: entry.row,
                resolved: entry.resolved,
                launch_origin: entry.launch_origin,
                capabilities: entry.capabilities,
            });
        } else {
            missing.push((entry.col, entry.row));
        }
    }
    RegistrationPlanResult { matched, missing }
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

    let reg_result = build_registration_plan(layout_out);
    for &(col, row) in &reg_result.missing {
        tracing::warn!(
            col,
            row,
            "no runtime UnitId for pane position, skipping baseline registration"
        );
    }
    let identity_store = ctx.composition.pane_launch_identity_store(root);
    for entry in &reg_result.matched {
        let descriptor = PaneDescriptor {
            runtime_id: PaneRuntimeId::derive(&plan.server, &plan.session, entry.unit_id.as_str()),
            group: plan.server.clone(),
            workspace: plan.session.clone(),
            unit_id: entry.unit_id.clone(),
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
            unit_id: entry.unit_id.clone(),
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
    runtime: &dyn RuntimeAdapter,
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

#[cfg(test)]
mod orchestration_tests {
    use super::super::test_support::base_runtime;
    use super::*;
    use workhive_core::domain::{CommandSpec, Locale, ToolCatalog, ToolName};
    use workhive_core::ports::{ColumnConfig, LayoutConfig, LayoutTemplate, PaneConfig};

    struct FakeI18n;
    impl workhive_core::ports::I18nPort for FakeI18n {
        fn resolve(&self, _: Locale, key: &str) -> String {
            key.to_owned()
        }
        fn resolve_with_args(&self, _: Locale, key: &str, _: &[&str]) -> String {
            key.to_owned()
        }
    }

    fn single_tool_layout() -> LayoutConfig {
        LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: workhive_core::domain::PaneLaunchRef::Tool {
                        name: ToolName::new("claude").unwrap(),
                    },
                }],
            }],
        }
    }

    fn single_inline_layout() -> LayoutConfig {
        LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: workhive_core::domain::PaneLaunchRef::InlineCommand {
                        spec: CommandSpec::new("htop", Vec::<String>::new()),
                    },
                }],
            }],
        }
    }

    fn single_empty_layout() -> LayoutConfig {
        LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: workhive_core::domain::PaneLaunchRef::EmptyShell,
                }],
            }],
        }
    }

    #[test]
    fn single_pane_orchestration_tool_layout_proceeds() {
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = single_tool_layout();

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        )
        .expect("must succeed");

        let out = result.expect("single-pane tool layout must not be skipped");
        assert_eq!(out.pane_count, 1);

        let baseline =
            workhive_core::usecases::launch_resolver::baseline_pane_entries(&out.all_resolved);
        assert_eq!(
            baseline.len(),
            1,
            "baseline registration must have one entry"
        );
    }

    #[test]
    fn single_pane_orchestration_inline_command_proceeds() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = single_inline_layout();

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("must succeed");

        let out = result.expect("single-pane inline layout must not be skipped");
        assert_eq!(out.pane_count, 1);

        let baseline =
            workhive_core::usecases::launch_resolver::baseline_pane_entries(&out.all_resolved);
        assert_eq!(baseline.len(), 1);
    }

    #[test]
    fn single_pane_orchestration_empty_shell_proceeds() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = single_empty_layout();

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("must succeed");

        let out = result.expect("single-pane empty shell must not be skipped");
        assert_eq!(out.pane_count, 1);
    }

    #[test]
    fn single_pane_orchestration_zero_pane_layout_skips() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![],
        };

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("must succeed");

        assert!(result.is_none(), "zero-pane layout must be skipped");
    }

    #[test]
    fn single_pane_orchestration_tool_with_title() {
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = single_tool_layout();

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        )
        .expect("must succeed");

        let out = result.expect("single-pane must proceed");
        assert!(!out.pane_titles.is_empty(), "tool pane must have a title");
    }

    #[test]
    fn single_pane_orchestration_degraded_tool_launch() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = LayoutConfig {
            template: LayoutTemplate::Single,
            columns: vec![ColumnConfig {
                width: 100,
                panes: vec![PaneConfig {
                    height: 100,
                    launch: workhive_core::domain::PaneLaunchRef::Tool {
                        name: ToolName::new("nonexistent-tool-xyz").unwrap(),
                    },
                }],
            }],
        };

        let result = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("must succeed even with degraded tool");

        let out = result.expect("single-pane must proceed");
        assert_eq!(out.pane_count, 1);
        assert!(
            out.all_resolved
                .iter()
                .any(|(_, _, r, _)| r.degradation_reason().is_some()),
            "unknown tool must degrade"
        );
    }

    // ─── Registration plan tests ─────────────────────────────────────────

    #[test]
    fn single_pane_registration_tool_has_real_unit_id() {
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = single_tool_layout();

        let out = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        )
        .expect("must succeed")
        .expect("must produce output");

        let result = super::build_registration_plan(&out);
        assert!(result.missing.is_empty(), "no missing positions expected");
        assert_eq!(
            result.matched.len(),
            1,
            "single-pane tool must produce one entry"
        );
        assert!(
            !result.matched[0].unit_id.as_str().is_empty(),
            "unit_id must be a real runtime value"
        );
        assert_eq!(result.matched[0].col, 0);
        assert_eq!(result.matched[0].row, 0);
    }

    #[test]
    fn single_pane_registration_missing_unit_is_reported() {
        use workhive_testkit::fakes::FakeToolLaunchInstrumentation;

        let runtime = base_runtime();
        let i18n = FakeI18n;
        let instrumentation = FakeToolLaunchInstrumentation::new();
        let layout = single_tool_layout();

        let mut out = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::builtin_defaults(),
            &i18n,
            Locale::En,
            Some(&instrumentation),
        )
        .expect("must succeed")
        .expect("must produce output");

        out.pane_units.clear();

        let result = super::build_registration_plan(&out);
        assert!(
            result.matched.is_empty(),
            "no entries expected when all unit mappings are removed"
        );
        assert_eq!(
            result.missing.len(),
            1,
            "single-pane tool with no UnitId must report exactly one missing position"
        );
        assert_eq!(result.missing[0], (0, 0));
    }

    #[test]
    fn single_pane_registration_empty_shell_has_real_unit_id() {
        let runtime = base_runtime();
        let i18n = FakeI18n;
        let layout = single_empty_layout();

        let out = orchestrate_layout(
            &runtime,
            "srv",
            "sess",
            camino::Utf8Path::new("/tmp/wt"),
            &layout,
            &ToolCatalog::default(),
            &i18n,
            Locale::En,
            None,
        )
        .expect("must succeed")
        .expect("must produce output");

        let result = super::build_registration_plan(&out);
        assert!(result.missing.is_empty(), "no missing positions expected");
        assert_eq!(
            result.matched.len(),
            1,
            "single-pane empty shell must produce one entry"
        );
        assert!(
            !result.matched[0].unit_id.as_str().is_empty(),
            "unit_id must be a real runtime value"
        );
    }
}

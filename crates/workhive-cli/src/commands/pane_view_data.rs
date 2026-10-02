//! Shared, side-effect-free preparation of view-layer inputs for the
//! `workhive pane` command surface.
//!
//! `pane list`, `pane show`, and the guarded `pane send` planner all
//! need the same three pieces of "view truth" before they can render
//! or decide anything:
//!
//!   1. The layered tool catalog (global + project, with built-in
//!      defaults as a fail-open) used to classify pane processes.
//!   2. The durable [`PaneLaunchIdentityEntry`] entries scoped to the
//!      current workspace, used to recover a recognizable tool label
//!      even when the live process is the Workhive supervisor wrapper.
//!   3. Paired [`PaneObservedViewInput`] values built from the live
//!      runtime snapshots and a single batched
//!      [`ProcessObserverPort::observe_many`] sweep, so that view
//!      builders cannot be miscalled with mismatched slice lengths.
//!
//! Each helper is intentionally fail-open: catalog or durable-store
//! failures degrade to safe defaults and log a warning, but never
//! short-circuit `pane list` / `pane show`. Identification enrichment
//! is non-fatal — live observation data still renders even when the
//! durable launch-identity store cannot be read.
//!
//! The module uses explicit imports rather than a glob re-export from
//! its parent so the dependency surface stays visible at the top of
//! the file.

use camino::Utf8Path;
use workhive_core::domain::ToolCatalog;
use workhive_core::ports::{
    PaneLaunchIdentityEntry, PaneLaunchIdentityPort, PaneSupervisorPort, ProcessObserverPort,
    RuntimeUnitSnapshot,
};
use workhive_core::usecases;
use workhive_core::usecases::pane_observation::gather_pane_truths;

use crate::context::CommandContext;

/// Load the layered (global + project) tool catalog, falling back to
/// the built-in defaults when the project has no project config.
///
/// The merge policy in
/// [`workhive_core::usecases::pane_observation::gather_pane_truths`]
/// uses [`ToolCatalog::match_process_name`] for typed pane
/// classification, so the catalog must contain the configured
/// `process_names` for any third-party tools the user has installed.
/// If config loading fails for any reason we degrade to the built-in
/// defaults rather than refusing to render `pane list` / `pane show`.
pub(crate) fn load_tool_catalog(ctx: &CommandContext, project_root: &Utf8Path) -> ToolCatalog {
    match usecases::config_mgmt::show_config(&ctx.config, Some(project_root), false) {
        Ok(cfg) => cfg
            .tools
            .as_ref()
            .map(|tc| tc.catalog.clone())
            .unwrap_or_else(ToolCatalog::builtin_defaults),
        Err(error) => {
            tracing::warn!(
                %error,
                "pane view: failed to load layered tool catalog; \
                 falling back to built-in defaults"
            );
            ToolCatalog::builtin_defaults()
        }
    }
}

/// Load durable launch-identity entries for the current workspace.
///
/// Returns an empty vector when the durable store cannot be read so
/// that `pane list` / `pane show` still renders live observation data;
/// failure to enrich titles is non-fatal.
pub(crate) fn load_identity_entries(
    ctx: &CommandContext,
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
) -> Vec<PaneLaunchIdentityEntry> {
    let store = ctx.composition.pane_launch_identity_store(project_root);
    match store.list_for_group(server) {
        Ok(entries) => entries
            .into_iter()
            .filter(|entry| entry.workspace == workspace)
            .collect(),
        Err(error) => {
            tracing::warn!(
                %error,
                server,
                workspace,
                "pane view: failed to load durable launch identity; \
                 rendering live observations only"
            );
            Vec::new()
        }
    }
}

/// Build paired view inputs for the merged-observation view layer.
///
/// For each live snapshot, this gathers a `PaneRuntimeTruth` from the
/// supervisor / OS observer / tmux fallback stack in a single pass,
/// uses the batched [`ProcessObserverPort::observe_many`] API to keep
/// observation cost at O(1) external probes per `pane list`, and
/// pairs the result with the snapshot. The pairing exists at the type
/// level so view builders cannot be miscalled with mismatched slice
/// lengths (the error-handling policy: no production panics on caller bugs).
pub(crate) fn build_observed_inputs(
    snapshots: &[RuntimeUnitSnapshot],
    supervisor: &dyn PaneSupervisorPort,
    observer: &dyn ProcessObserverPort,
    server: &str,
    workspace: &str,
    tools: &ToolCatalog,
) -> Vec<usecases::pane::PaneObservedViewInput> {
    let truths = gather_pane_truths(
        snapshots,
        Some(supervisor),
        Some(observer),
        server,
        workspace,
        tools,
    );

    snapshots
        .iter()
        .zip(truths)
        .map(|(snap, truth)| usecases::pane::PaneObservedViewInput::new(snap.clone(), truth))
        .collect()
}

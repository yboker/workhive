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

/// Stage 3+4: Execute the plan — create session and restore snapshots.
///
/// All mutations (runtime creation, snapshot pane restore) happen here.
///
/// ## Normal mode
///
/// 1. If session exists → reuse (attach).
/// 2. Else if `snapshot_enabled` → try snapshot restore.
///    - Success → `RestoredFromSnapshot`.
///    - Failure/missing/stale → transactional rollback + `FellBackToLayout`.
/// 3. Else → `FellBackToLayout` (caller applies layout).
///
/// ## ForceRecreateFromLayout mode
///
/// 1. If session exists → destroy it.
/// 2. Delete snapshot for workspace (if any).
/// 3. Create fresh session → `ForceRecreatedFromLayout` (caller applies layout).
///
/// ## Transactional restore contract
///
/// If snapshot restore fails **after** runtime mutation has begun, the dirty
/// runtime is destroyed, a clean runtime is recreated, and the caller gets
/// `FellBackToLayout`. Partial restore state is never left alive.
#[allow(clippy::too_many_arguments)]
pub fn execute_open_plan(
    ws_runtime: &dyn WorkspaceRuntimePort,
    pane_runtime: &dyn SnapshotPaneRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    plan: &OpenPlan,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
    instrumentation: Option<&dyn crate::ports::ToolLaunchInstrumentationPort>,
) -> Result<OpenResult, OpenError> {
    let mut restored_panes = 0u32;
    let snapshot_outcome;
    let recovery_path;

    match plan.open_mode {
        OpenMode::ForceRecreateFromLayout => {
            if plan.session_availability.allows_reuse() {
                tracing::info!(
                    session = %plan.session,
                    server = %plan.server,
                    "open -f: destroying existing session"
                );
                ws_runtime.destroy_workspace(&plan.server, &plan.session)?;
            }

            // The snapshot runtime branch — including the snapshot store — must
            // be skipped entirely when `snapshot.enabled = false`. Force
            // recreate still destroys+recreates the runtime, but it must not
            // call `delete_all`/`delete_all_global` against `SnapshotStorePort`
            // because that would touch `.workhive/snapshots/` and surface
            // adapter errors users explicitly opted out of.
            if plan.snapshot_enabled {
                let delete_result = if let Some(state_key) = plan
                    .snapshot_identity
                    .as_ref()
                    .and_then(|identity| identity.state_key.as_ref())
                {
                    snapshot_store.delete_all(state_key)
                } else {
                    snapshot_store.delete_all_global()
                };

                match delete_result {
                    Ok(()) => {
                        tracing::debug!(
                            workspace = %plan.workspace,
                            "open -f: snapshot deleted (or did not exist)"
                        );
                        snapshot_outcome = SnapshotOutcome::DeletedByForceRecreate;
                    }
                    Err(e) => {
                        tracing::error!(
                            workspace = %plan.workspace,
                            error = %e,
                            "open -f: snapshot deletion failed — aborting force-recreate"
                        );
                        return Err(OpenError::Snapshot(e));
                    }
                }
            } else {
                tracing::debug!(
                    workspace = %plan.workspace,
                    "open -f: snapshot disabled — skipping snapshot store delete entirely"
                );
                snapshot_outcome = SnapshotOutcome::Disabled;
            }

            ws_runtime.create_workspace(CreateWorkspaceRuntimeRequest {
                workspace_name: plan.session.clone(),
                group: plan.server.clone(),
                working_dir: plan.working_dir.clone(),
                env: plan.env.clone(),
            })?;
            ws_runtime.configure_workspace(&plan.server, &plan.session)?;

            recovery_path = OpenRecoveryPath::ForceRecreatedFromLayout;
        }

        OpenMode::Normal => {
            if plan.session_availability.allows_reuse() {
                snapshot_outcome = SnapshotOutcome::Disabled;
                recovery_path = OpenRecoveryPath::ReusedLiveSession;
            } else {
                create_fresh_runtime(ws_runtime, plan)?;

                if plan.snapshot_enabled {
                    snapshot_outcome = try_restore_snapshot(&SnapshotRestoreContext {
                        pane_runtime,
                        snapshot_runtime: pane_runtime,
                        snapshot_store,
                        server: &plan.server,
                        session: &plan.session,
                        workspace: &plan.workspace,
                        identity_ctx: plan.snapshot_identity.as_ref(),
                        catalog,
                        exec_checker,
                        instrumentation,
                    });

                    match &snapshot_outcome {
                        SnapshotOutcome::Restored { panes, .. } => {
                            restored_panes = *panes;
                            recovery_path = OpenRecoveryPath::RestoredFromSnapshot;
                        }
                        SnapshotOutcome::Failed { .. } => {
                            tracing::warn!(
                                session = %plan.session,
                                outcome = ?snapshot_outcome,
                                "snapshot restore failed after runtime mutation; \
                                 destroying dirty runtime and recreating clean"
                            );
                            rollback_and_recreate(ws_runtime, plan)?;
                            recovery_path = OpenRecoveryPath::FellBackToLayout;
                        }
                        _ => {
                            tracing::info!(
                                session = %plan.session,
                                outcome = ?snapshot_outcome,
                                "snapshot restore did not complete; caller must apply layout"
                            );
                            recovery_path = OpenRecoveryPath::FellBackToLayout;
                        }
                    }
                } else {
                    tracing::debug!(
                        session = %plan.session,
                        "snapshot disabled; layout will be applied by caller"
                    );
                    snapshot_outcome = SnapshotOutcome::Disabled;
                    recovery_path = OpenRecoveryPath::FellBackToLayout;
                }
            }
        }
    }

    tracing::info!(
        session = %plan.session,
        server = %plan.server,
        availability = %plan.session_availability,
        recovery_path = %recovery_path,
        restored_panes,
        open_mode = ?plan.open_mode,
        snapshot_enabled = plan.snapshot_enabled,
        snapshot_outcome = ?snapshot_outcome,
        provenance = ?plan.provenance,
        "workspace opened"
    );

    let outcome = OpenOutcome::derive(&recovery_path, &snapshot_outcome);

    let reattach_outcome = match &recovery_path {
        OpenRecoveryPath::ReusedLiveSession => ReattachOutcome::Reattached,
        OpenRecoveryPath::RestoredFromSnapshot | OpenRecoveryPath::FellBackToLayout => {
            let availability = classify_session_availability(
                ws_runtime.has_workspace(&plan.server, &plan.session),
            );
            classify_reattach_outcome(&availability, true)
        }
        OpenRecoveryPath::ForceRecreatedFromLayout => ReattachOutcome::CreatedFresh,
    };

    Ok(OpenResult {
        session_name: plan.session.clone(),
        worktree_path: plan.working_dir.clone(),
        restored_panes,
        provenance: plan.provenance.clone(),
        recovery_path,
        snapshot_outcome,
        outcome,
        reattach_outcome,
        snapshot_supervisor_outcome: SnapshotSupervisorOutcome::NotRequested,
    })
}

/// Create a fresh workspace runtime from the plan.
pub(crate) fn create_fresh_runtime(
    ws_runtime: &dyn WorkspaceRuntimePort,
    plan: &OpenPlan,
) -> Result<(), OpenError> {
    ws_runtime.create_workspace(CreateWorkspaceRuntimeRequest {
        workspace_name: plan.session.clone(),
        group: plan.server.clone(),
        working_dir: plan.working_dir.clone(),
        env: plan.env.clone(),
    })?;
    ws_runtime.configure_workspace(&plan.server, &plan.session)?;
    Ok(())
}

/// Transactional rollback: destroy the dirty runtime and recreate a clean one.
///
/// This ensures partial restore state is never left alive for the caller
/// to accidentally build layout on top of.
pub(crate) fn rollback_and_recreate(
    ws_runtime: &dyn WorkspaceRuntimePort,
    plan: &OpenPlan,
) -> Result<(), OpenError> {
    ws_runtime.destroy_workspace(&plan.server, &plan.session)?;
    create_fresh_runtime(ws_runtime, plan)
}

/// Convenience wrapper: prepare + execute in one call.
///
/// `snapshot_enabled` must come from `project_config.snapshot.enabled`.
/// `open_mode` controls Normal vs ForceRecreateFromLayout behavior.
#[allow(clippy::too_many_arguments)]
pub fn open_workspace(
    provision: &dyn WorkspaceProvisionPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    pane_runtime: &dyn SnapshotPaneRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    snapshot_store: &dyn SnapshotStorePort,
    project_root: &Utf8PathBuf,
    workspace: &WorkspaceId,
    server_name: &str,
    session_name: &str,
    intent: WorkspaceOpenIntent,
    snapshot_enabled: bool,
    open_mode: OpenMode,
    catalog: &ToolCatalog,
    exec_checker: &dyn ExecutableAvailabilityPort,
    snapshot_identity: Option<SnapshotIdentityContext>,
) -> Result<OpenResult, OpenError> {
    let inferred_snapshot_identity = snapshot_identity
        .or_else(|| infer_snapshot_identity_context(session_name, server_name, workspace));

    let plan = prepare_open_plan(
        provision,
        ws_runtime,
        ws_fs,
        project_root,
        workspace,
        server_name,
        session_name,
        intent,
        snapshot_enabled,
        open_mode,
        inferred_snapshot_identity,
    )?;
    // Test/internal callers do not have a real `ToolLaunchInstrumentationPort`
    // wired up; they exercise the use case without supervisor wrapping. The
    // CLI uses `execute_open_plan` directly with the production
    // instrumentation adapter.
    execute_open_plan(
        ws_runtime,
        pane_runtime,
        snapshot_store,
        &plan,
        catalog,
        exec_checker,
        None,
    )
}

pub(crate) fn infer_snapshot_identity_context(
    session_name: &str,
    server_name: &str,
    workspace: &WorkspaceId,
) -> Option<SnapshotIdentityContext> {
    let (instance_id, project_component) = if let Some((instance_id, project_component, _)) =
        crate::domain::SessionNamingPolicy::parse_session(session_name)
    {
        (instance_id.to_owned(), project_component.to_owned())
    } else {
        let (instance_id, project_component) =
            crate::domain::SessionNamingPolicy::parse_server(server_name)?;
        (instance_id.to_owned(), project_component.to_owned())
    };
    Some(SnapshotIdentityContext {
        instance_id: instance_id.clone(),
        project: project_component.clone(),
        runtime_backend: RuntimeBackendId::tmux(),
        state_key: Some(crate::domain::WorkspaceStateKey::new(
            instance_id,
            project_component,
            String::new(),
            workspace.as_str().to_owned(),
        )),
    })
}

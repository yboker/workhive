use camino::Utf8PathBuf;

use super::*;
use crate::domain::{ToolCatalog, WorkspaceId, WorkspaceOpenIntent};
use crate::ports::{
    CreateWorkspaceRuntimeRequest, ExecutableAvailabilityPort, RegisterWorkspaceCheckoutRequest,
    RuntimeError, WorkspaceFsPort, WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// Execute an open plan against the live runtime.
///
/// Normal mode reuses a live runtime or creates one for the effective layout.
///
/// Force mode creates a fresh runtime for the supplied (staged) plan. The CLI
/// owns replacement: it prepares and validates the staged runtime before it
/// commits the new binding and retires the previous runtime.
pub fn execute_open_plan(
    ws_runtime: &dyn WorkspaceRuntimePort,
    plan: &OpenPlan,
) -> Result<OpenResult, OpenError> {
    let (recovery_path, prepared_runtime) = match plan.open_mode {
        OpenMode::ForceRecreateFromLayout => (
            OpenRecoveryPath::ForceRecreatedFromLayout,
            Some(create_fresh_runtime(ws_runtime, plan)?),
        ),
        OpenMode::Normal if plan.session_availability.allows_reuse() => {
            (OpenRecoveryPath::ReusedLiveSession, None)
        }
        OpenMode::Normal => (
            OpenRecoveryPath::CreatedFromLayout,
            Some(create_fresh_runtime(ws_runtime, plan)?),
        ),
    };

    let availability = if matches!(recovery_path, OpenRecoveryPath::CreatedFromLayout) {
        classify_session_availability_strict(
            ws_runtime.has_workspace_binding(
                prepared_runtime
                    .as_ref()
                    .map(|prepared| &prepared.binding)
                    .expect("created runtime returns a concrete binding"),
            ),
        )
        .map_err(OpenError::Runtime)?
    } else {
        plan.session_availability.clone()
    };
    let reattach_outcome = reattach_outcome_for_path(&recovery_path, availability);
    let outcome = match recovery_path {
        OpenRecoveryPath::ReusedLiveSession => OpenOutcome::ExistingSessionReused,
        OpenRecoveryPath::CreatedFromLayout => OpenOutcome::LayoutApplied,
        OpenRecoveryPath::ForceRecreatedFromLayout => OpenOutcome::ForceRecreated,
    };

    tracing::info!(
        session = %plan.session,
        server = %plan.server,
        recovery_path = %recovery_path,
        open_mode = ?plan.open_mode,
        provenance = ?plan.provenance,
        "workspace opened"
    );

    Ok(OpenResult {
        session_name: plan.session.clone(),
        prepared_runtime,
        worktree_path: plan.working_dir.clone(),
        provenance: plan.provenance.clone(),
        recovery_path,
        outcome,
        reattach_outcome,
    })
}

/// Create a fresh workspace runtime from the plan.
pub(crate) fn create_fresh_runtime(
    ws_runtime: &dyn WorkspaceRuntimePort,
    plan: &OpenPlan,
) -> Result<PreparedRuntimeBinding, OpenError> {
    let created = ws_runtime.create_workspace_binding_with_label(
        CreateWorkspaceRuntimeRequest {
            workspace_name: plan.session.clone(),
            group: plan.server.clone(),
            working_dir: plan.working_dir.clone(),
            env: plan.env.clone(),
        },
        plan.project_root
            .file_name()
            .filter(|name| !name.is_empty())
            .unwrap_or("project"),
    )?;
    let (binding, ownership) = if plan.defer_checkout_registration {
        (
            created.clone(),
            PreparedRuntimeOwnership::CreatedByOperation,
        )
    } else {
        let authoritative =
            match ws_runtime.register_workspace_checkout(RegisterWorkspaceCheckoutRequest {
                binding: created.clone(),
                project_root: plan.project_root.clone(),
                workspace_root: plan.workspace_root.clone(),
            }) {
                Ok(binding) => binding,
                Err(error) => return Err(compensate_created_runtime(ws_runtime, &created, error)),
            };
        if authoritative != created
            && let Err(cleanup_error) = ws_runtime.destroy_workspace_binding(&created)
            && !matches!(cleanup_error, RuntimeError::WorkspaceNotFound { .. })
        {
            return Err(OpenError::Runtime(RuntimeError::OperationFailed {
                message: format!(
                    "runtime checkout registration adopted '{}', but cleanup of superseded runtime '{}' failed ({cleanup_error})",
                    authoritative.workspace_id, created.workspace_id
                ),
            }));
        }
        let ownership = if authoritative == created {
            PreparedRuntimeOwnership::CreatedByOperation
        } else {
            PreparedRuntimeOwnership::AdoptedExisting
        };
        (authoritative, ownership)
    };
    if let Err(setup_error) =
        ws_runtime.configure_workspace_binding(&binding, &plan.interaction_policy)
    {
        if binding == created {
            return Err(compensate_created_runtime(
                ws_runtime,
                &created,
                setup_error,
            ));
        }
        return Err(OpenError::Runtime(setup_error));
    }
    Ok(PreparedRuntimeBinding { binding, ownership })
}

fn compensate_created_runtime(
    ws_runtime: &dyn WorkspaceRuntimePort,
    created: &crate::domain::RuntimeBinding,
    setup_error: RuntimeError,
) -> OpenError {
    match ws_runtime.destroy_workspace_binding(created) {
        Ok(()) | Err(RuntimeError::WorkspaceNotFound { .. }) => OpenError::Runtime(setup_error),
        Err(cleanup_error) => OpenError::Runtime(RuntimeError::OperationFailed {
            message: format!(
                "runtime setup failed ({setup_error}); compensating cleanup also failed ({cleanup_error})"
            ),
        }),
    }
}

/// Convenience wrapper: prepare + execute in one call.
#[allow(clippy::too_many_arguments)]
pub fn open_workspace(
    provision: &dyn WorkspaceProvisionPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    project_root: &Utf8PathBuf,
    workspace: &WorkspaceId,
    server_name: &str,
    session_name: &str,
    intent: WorkspaceOpenIntent,
    open_mode: OpenMode,
    _catalog: &ToolCatalog,
    _exec_checker: &dyn ExecutableAvailabilityPort,
) -> Result<OpenResult, OpenError> {
    let plan = prepare_open_plan(
        provision,
        ws_runtime,
        ws_fs,
        project_root,
        workspace,
        server_name,
        session_name,
        intent,
        open_mode,
    )?;
    execute_open_plan(ws_runtime, &plan)
}

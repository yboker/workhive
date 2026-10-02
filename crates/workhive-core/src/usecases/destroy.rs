//! `destroy` use case — remove all workhive resources for a project.

use std::collections::HashSet;

use camino::Utf8PathBuf;

use crate::domain::RuntimeBinding;
use crate::errors::{IndexError, WorkspaceFsError};
use crate::ports::{
    ProjectIndexPort, ProvisionError, RuntimeError, WorkspaceFsPort, WorkspaceProvisionPort,
    WorktreeRemovePolicy,
};

/// Errors from the `destroy` use case.
#[derive(Debug, thiserror::Error)]
pub enum DestroyError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Provision(#[from] ProvisionError),

    #[error(transparent)]
    WorkspaceFs(#[from] WorkspaceFsError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(
        "worktree removal failed for {failed_count} workspace(s), aborting destroy to protect .workhive"
    )]
    WorktreeRemovalFailed {
        failed_count: usize,
        first_error: ProvisionError,
    },

    #[error("destroy worktree path {path} is outside Workhive-managed base {managed_base}")]
    WorktreePathOutsideManagedBase {
        path: Utf8PathBuf,
        managed_base: Utf8PathBuf,
    },

    #[error(
        "found {count} unmanaged worktree directorie(s) under .workhive/worktrees; first: {first_path}"
    )]
    OrphanManagedWorktreeDirs {
        count: usize,
        first_path: Utf8PathBuf,
    },
}

/// Destroy all workhive-managed resources for a project.
///
/// `runtime_bindings` is the authoritative runtime inventory. The callback
/// must route each binding to its exact backend and scope.
/// `managed_worktree_paths` is the authoritative inventory — the same set
/// that was used for the safety scan. The destroy use case removes exactly
/// these paths and no others.
#[allow(clippy::too_many_arguments)]
pub fn destroy_project<D>(
    provision: &dyn WorkspaceProvisionPort,
    destroy_runtime: D,
    ws_fs: &dyn WorkspaceFsPort,
    index: &dyn ProjectIndexPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
    runtime_bindings: &[RuntimeBinding],
    remove_policy: WorktreeRemovePolicy,
    managed_worktree_paths: &[Utf8PathBuf],
) -> Result<(), DestroyError>
where
    D: Fn(&RuntimeBinding) -> Result<(), RuntimeError>,
{
    let worktree_base = ws_fs
        .workhive_dir_path(project_root.as_ref())
        .join("worktrees");

    let mut managed_names = HashSet::new();
    for path in managed_worktree_paths {
        if !path.starts_with(&worktree_base) {
            return Err(DestroyError::WorktreePathOutsideManagedBase {
                path: path.clone(),
                managed_base: worktree_base,
            });
        }
        if let Some(name) = path.file_name() {
            managed_names.insert(name.to_owned());
        }
    }

    let orphan_dirs = ws_fs
        .list_workspace_dirs(project_root.as_ref())?
        .into_iter()
        .filter(|name| !managed_names.contains(name))
        .map(|name| worktree_base.join(name))
        .collect::<Vec<_>>();

    if let Some(first_path) = orphan_dirs.first() {
        return Err(DestroyError::OrphanManagedWorktreeDirs {
            count: orphan_dirs.len(),
            first_path: first_path.clone(),
        });
    }

    // 1. Destroy exactly the persisted runtime bindings. Runtime IDs are
    // backend-owned and must never be parsed to infer project ownership.
    let mut destroyed = HashSet::new();
    for binding in runtime_bindings {
        let key = (
            binding.backend.clone(),
            binding.scope_id.clone(),
            binding.workspace_id.clone(),
        );
        if !destroyed.insert(key) {
            continue;
        }
        match destroy_runtime(binding) {
            Ok(()) => tracing::debug!(
                backend = %binding.backend,
                scope = %binding.scope_id,
                runtime_id = %binding.workspace_id,
                "destroyed project runtime workspace"
            ),
            Err(RuntimeError::WorkspaceNotFound { .. }) => {}
            Err(RuntimeError::BackendUnavailable { reason, .. }) if reason.is_runtime_absent() => {}
            Err(error) => return Err(DestroyError::Runtime(error)),
        }
    }

    // 2. Remove managed worktree roots from the caller-provided inventory.
    // Abort if any fails to protect .workhive and downstream state.
    let mut removal_errors: Vec<(Utf8PathBuf, ProvisionError)> = Vec::new();
    for ws_path in managed_worktree_paths {
        if let Err(e) = provision.remove_workspace_root(ws_path.as_ref(), remove_policy) {
            tracing::warn!(path = %ws_path, error = %e, "failed to remove workspace root");
            removal_errors.push((ws_path.clone(), e));
        }
    }
    let failed_count = removal_errors.len();
    if let Some((_, first_error)) = removal_errors.into_iter().next() {
        return Err(DestroyError::WorktreeRemovalFailed {
            failed_count,
            first_error,
        });
    }

    // 4. Remove .workhive/ directory
    ws_fs.remove_workhive_dir(project_root.as_ref())?;

    // 5. Remove from global index
    index.remove_project(project_root)?;

    tracing::info!(project = project_name, "project destroyed");
    Ok(())
}

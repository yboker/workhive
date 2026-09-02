//! `destroy` use case — remove all workhive resources for a project.

use std::collections::HashSet;

use camino::Utf8PathBuf;

use crate::errors::{IndexError, SnapshotError, WorkspaceFsError};
use crate::ports::{
    ProjectIndexPort, ProvisionError, RuntimeError, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceProvisionPort, WorkspaceRuntimePort, WorktreeRemovePolicy,
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
    Snapshot(#[from] SnapshotError),

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
/// `server_name` is resolved by the caller via `SessionNamingPolicy`.
/// `managed_worktree_paths` is the authoritative inventory — the same set
/// that was used for the safety scan. The destroy use case removes exactly
/// these paths and no others.
#[allow(clippy::too_many_arguments)]
pub fn destroy_project(
    provision: &dyn WorkspaceProvisionPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    snapshot_store: &dyn SnapshotStorePort,
    index: &dyn ProjectIndexPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
    server_name: &str,
    remove_policy: WorktreeRemovePolicy,
    managed_worktree_paths: &[Utf8PathBuf],
) -> Result<(), DestroyError> {
    let server = server_name;
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

    // 1. Destroy runtime group for this project. A missing backend means there
    // are no live runtime resources left to remove, so project cleanup can
    // continue idempotently.
    match ws_runtime.destroy_group(server) {
        Ok(()) => tracing::debug!(server = %server, "runtime group destroyed"),
        Err(RuntimeError::BackendUnavailable { detail, .. }) => {
            tracing::debug!(
                server = %server,
                detail = %detail,
                "runtime group already unavailable during destroy"
            );
        }
        Err(error) => return Err(DestroyError::Runtime(error)),
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

    // 3. Delete all snapshot state scoped under this project root.
    snapshot_store.delete_all_global()?;

    // 4. Remove .workhive/ directory
    ws_fs.remove_workhive_dir(project_root.as_ref())?;

    // 5. Remove from global index
    index.remove_project(project_root)?;

    tracing::info!(project = project_name, "project destroyed");
    Ok(())
}

//! Global runtime workspace listing across all discovered projects.

use std::collections::{HashMap, HashSet};

use crate::domain::{IdentityError, RuntimeBackendId, RuntimeBinding, WorkspaceIdentityRecord};
use crate::ports::{
    BackendUnavailableReason, DiscoveredProject, RuntimeError, WorkspaceRuntimeInfo,
};

/// A runtime workspace joined with its Workhive identity.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    /// Exact persisted runtime binding.
    pub binding: RuntimeBinding,
    /// Project display name.
    pub project_name: String,
    /// Absolute path to the project root.
    pub project_root: String,
    /// Workhive logical workspace name.
    pub workspace: String,
    /// Optional backend-owned human-readable label.
    pub runtime_label: Option<String>,
    /// Whether a client is currently attached.
    pub attached: bool,
}

/// A project whose runtime backend could not be queried during a global sweep.
#[derive(Debug, Clone)]
pub struct UnavailableProject {
    pub backend: RuntimeBackendId,
    pub project_name: String,
    pub project_root: String,
    pub reason: BackendUnavailableReason,
    pub detail: String,
}

/// Outcome of a global runtime workspace sweep.
#[derive(Debug, Clone, Default)]
pub struct GlobalSessionListing {
    pub records: Vec<SessionRecord>,
    pub unavailable: Vec<UnavailableProject>,
}

/// Errors that can occur when listing global runtime workspaces.
#[derive(Debug, thiserror::Error)]
pub enum SessionQueryError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Identity(#[from] IdentityError),
}

/// List live runtime workspaces belonging to persisted workspace identities.
///
/// Identity records are grouped by backend and scope. `list_scope` is called
/// exactly once for each group, and must resolve the adapter from the supplied
/// binding rather than from ambient backend configuration. Runtime names are
/// never parsed to infer logical ownership.
pub fn list_global_sessions<F, L>(
    projects: &[DiscoveredProject],
    resolve_identities: F,
    list_scope: L,
) -> Result<GlobalSessionListing, SessionQueryError>
where
    F: Fn(&DiscoveredProject) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError>,
    L: Fn(&RuntimeBinding) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>,
{
    type ScopeKey = (RuntimeBackendId, String);
    let mut scopes: HashMap<ScopeKey, Vec<(usize, WorkspaceIdentityRecord)>> = HashMap::new();
    for (project_index, project) in projects.iter().enumerate() {
        for identity in resolve_identities(project)? {
            let binding = identity.runtime_binding();
            scopes
                .entry((binding.backend, binding.scope_id))
                .or_default()
                .push((project_index, identity));
        }
    }

    let mut records = Vec::new();
    let mut unavailable = Vec::new();
    for ((backend, _scope_id), identities) in scopes {
        let query_binding = identities[0].1.runtime_binding();
        let workspaces = match list_scope(&query_binding) {
            Ok(workspaces) => workspaces,
            Err(RuntimeError::BackendUnavailable { reason, detail })
                if reason.is_runtime_absent() =>
            {
                let mut seen = HashSet::new();
                for (project_index, _) in identities.iter().filter(|(i, _)| seen.insert(*i)) {
                    let project = &projects[*project_index];
                    unavailable.push(UnavailableProject {
                        backend: backend.clone(),
                        project_name: project.project_name.clone(),
                        project_root: project.project_root.to_string(),
                        reason: reason.clone(),
                        detail: detail.clone(),
                    });
                }
                continue;
            }
            Err(other) => return Err(other.into()),
        };

        for (project_index, identity) in identities {
            let binding = identity.runtime_binding();
            let Some(runtime) = workspaces
                .iter()
                .find(|runtime| runtime.runtime_id == binding.workspace_id)
            else {
                continue;
            };
            let project = &projects[project_index];
            records.push(SessionRecord {
                binding,
                project_name: project.project_name.clone(),
                project_root: project.project_root.to_string(),
                workspace: identity.workspace,
                runtime_label: runtime.label.clone(),
                attached: runtime.attached,
            });
        }
    }

    Ok(GlobalSessionListing {
        records,
        unavailable,
    })
}

// Tests for this module live in workhive-testkit/tests/usecase_tests.rs.

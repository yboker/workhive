//! `sessions` use case — list active tmux sessions across all discovered projects.

use crate::domain::{NamingError, SessionNamingPolicy};
use crate::errors::ConfigError;
use crate::ports::{
    BackendUnavailableReason, DiscoveredProject, RuntimeError, WorkspaceRuntimePort,
};

/// A single tmux session record associated with a project.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    /// Project display name.
    pub project_name: String,
    /// Absolute path to the project root.
    pub project_root: String,
    /// Workspace name (extracted from session name).
    pub workspace: String,
    /// Tmux server name.
    pub server_name: String,
    /// Tmux session name.
    pub session_name: String,
    /// Whether a client is currently attached.
    pub attached: bool,
}

/// A project whose runtime backend could not be queried during a global
/// session sweep.
///
/// Capturing these separately keeps `session list` index-backed and resilient:
/// a single project whose runtime socket or server is unreachable is reported
/// per project instead of aborting the entire listing.
#[derive(Debug, Clone)]
pub struct UnavailableProject {
    /// Project display name.
    pub project_name: String,
    /// Absolute path to the project root.
    pub project_root: String,
    /// Typed reason the backend was unavailable (socket missing, server not
    /// running, backend binary absent, …).
    pub reason: BackendUnavailableReason,
    /// Diagnostic detail preserved from the runtime error for logs.
    pub detail: String,
}

/// Outcome of a global session sweep across every indexed project.
///
/// `records` carries sessions from every project whose runtime backend was
/// reachable; `unavailable` lists projects skipped because their backend could
/// not be queried. A single unavailable backend never blanks the listing.
#[derive(Debug, Clone, Default)]
pub struct GlobalSessionListing {
    /// Sessions discovered across all reachable projects.
    pub records: Vec<SessionRecord>,
    /// Projects skipped because their runtime backend was unreachable.
    pub unavailable: Vec<UnavailableProject>,
}

/// Errors that can occur when listing global sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionQueryError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Naming(#[from] NamingError),

    /// A session name on the server did not match the expected naming scheme.
    #[error("cannot extract workspace from session name '{session_name}' on server '{server}'")]
    SessionNameParseFailed {
        server: String,
        session_name: String,
    },
}

/// List active tmux sessions for all given projects.
///
/// For each project, derives the tmux server name via a per-project naming
/// policy resolver, queries its sessions, and extracts workspace info.
///
/// `resolve_policy` is called once per project to obtain the effective
/// `SessionNamingPolicy` (project config > global config > defaults).
/// Returns `Err` if the resolver fails (e.g., invalid project config).
///
/// Runtime backend availability is degraded per project: when a project's
/// backend is unreachable (`RuntimeError::BackendUnavailable`), that project is
/// recorded in `unavailable` and the sweep continues, so one missing socket or
/// stopped server never blanks sessions from other reachable projects. Other
/// runtime errors (permission, naming, unexpected) still propagate as `Err`.
pub fn list_global_sessions<F>(
    projects: &[DiscoveredProject],
    resolve_policy: F,
    ws_runtime: &dyn WorkspaceRuntimePort,
) -> Result<GlobalSessionListing, SessionQueryError>
where
    F: Fn(&DiscoveredProject) -> Result<SessionNamingPolicy, ConfigError>,
{
    let mut records = Vec::new();
    let mut unavailable = Vec::new();

    for project in projects {
        let policy = resolve_policy(project)?;

        let naming_ctx = crate::domain::NamingContext {
            project: project.project_name.clone(),
            workspace: String::new(),
            branch: None,
            dir_path: Some(project.project_root.to_string()),
            instance_id: Some(project.instance_id.clone()),
        };

        let server = policy.resolve_server_for_project(&naming_ctx)?;

        let workspaces = match ws_runtime.list_workspaces(&server) {
            Ok(workspaces) => workspaces,
            Err(RuntimeError::BackendUnavailable { reason, detail }) => {
                unavailable.push(UnavailableProject {
                    project_name: project.project_name.clone(),
                    project_root: project.project_root.to_string(),
                    reason,
                    detail,
                });
                continue;
            }
            Err(other) => return Err(other.into()),
        };

        for ws in &workspaces {
            let parsed = crate::domain::SessionName::from_raw(&ws.workspace_name);
            let workspace = parsed
                .extract_workspace()
                .ok_or_else(|| SessionQueryError::SessionNameParseFailed {
                    server: server.clone(),
                    session_name: ws.workspace_name.clone(),
                })?
                .to_owned();

            records.push(SessionRecord {
                project_name: project.project_name.clone(),
                project_root: project.project_root.to_string(),
                workspace,
                server_name: server.clone(),
                session_name: ws.workspace_name.clone(),
                attached: ws.attached,
            });
        }
    }

    Ok(GlobalSessionListing {
        records,
        unavailable,
    })
}

// Tests for this module live in `workhive-testkit/tests/usecase_tests.rs`
// because they require fake adapters that cannot be imported from `workhive-core`.

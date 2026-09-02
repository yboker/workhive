//! `list` use case — show workspaces for current project or all projects.

use camino::Utf8PathBuf;

use crate::domain::{NamingError, SessionAvailability, WorkspaceStateKey};
use crate::errors::{GitError, IndexError, WorkspaceFsError};
use crate::ports::{
    IndexState, ProjectIndexPort, RuntimeError, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceRuntimePort,
};
use crate::usecases::open::classify_session_availability;

/// Errors from the `list` use case.
#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Git(#[from] GitError),

    #[error(transparent)]
    WorkspaceFs(#[from] WorkspaceFsError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    Naming(#[from] NamingError),
}

/// Three-state snapshot probe result for display purposes.
///
/// Unlike a boolean, this distinguishes "no snapshot" from "probe failed",
/// preventing silent data loss in the display layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotStatus {
    /// A valid snapshot exists on disk.
    Available,
    /// No snapshot was found (workspace ID valid, storage accessible).
    Absent,
    /// The probe could not determine snapshot presence (I/O error, invalid workspace ID).
    Unknown,
}

impl SnapshotStatus {
    /// Backwards-compatible boolean: `Available` → `true`, everything else → `false`.
    pub fn is_available(self) -> bool {
        self == Self::Available
    }
}

/// Three-state pane count enrichment result for display purposes.
///
/// Unlike `Option<u32>`, this distinguishes "enrichment failed" from
/// "session not active", preventing silent data loss in the display layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCountStatus {
    /// Enrichment succeeded — this many panes are active.
    Known(u32),
    /// Session is not active, so pane count is not applicable.
    NotApplicable,
    /// Enrichment failed (runtime error) — pane count could not be determined.
    Unknown,
}

impl PaneCountStatus {
    /// Backwards-compatible `Option<u32>`: `Known(n)` → `Some(n)`, everything else → `None`.
    pub fn as_option(self) -> Option<u32> {
        match self {
            Self::Known(n) => Some(n),
            _ => None,
        }
    }
}

/// Stable structural classification of a workspace row.
///
/// This is the authoritative business identity for a workspace. Display
/// surfaces (`list`, `open`, `attach`) MUST branch on this enum rather than
/// parsing display names or matching on the `main` branch string, so the
/// project-root vs. linked-worktree distinction can never drift from a
/// localized label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceKind {
    /// The project root itself (the checkout `workhive init` was run in).
    ProjectRoot,
    /// A linked git worktree provisioned under `.workhive/worktrees/<name>`.
    LinkedWorktree,
}

impl WorkspaceKind {
    /// Stable machine token for JSON / scripting output.
    pub fn as_token(self) -> &'static str {
        match self {
            Self::ProjectRoot => "project_root",
            Self::LinkedWorktree => "linked_worktree",
        }
    }
}

/// Resolve the pane-count enrichment for an *active* session.
///
/// Shared by the project-root and linked-worktree row builders so the two
/// paths cannot drift. When the runtime confirms the session is running but
/// the workspace listing returns no matching record, the count is reported as
/// [`PaneCountStatus::Unknown`] (lookup failed) rather than a misleading
/// `Known(0)` — a running session always owns at least one pane, so zero is
/// never a truthful enrichment result here.
fn resolve_active_pane_count(
    ws_runtime: &dyn WorkspaceRuntimePort,
    server: &str,
    session_name: &str,
    workspace_label: &str,
) -> PaneCountStatus {
    match ws_runtime.list_workspaces(server) {
        Ok(list) => list
            .into_iter()
            .find(|w| w.workspace_name == session_name)
            .map(|w| PaneCountStatus::Known(w.unit_count))
            .unwrap_or_else(|| {
                tracing::warn!(
                    workspace = %workspace_label,
                    session = %session_name,
                    "session is active but has no matching runtime record; reporting unknown"
                );
                PaneCountStatus::Unknown
            }),
        Err(e) => {
            tracing::warn!(
                workspace = %workspace_label,
                error = %e,
                "pane count enrichment failed; degrading to unknown"
            );
            PaneCountStatus::Unknown
        }
    }
}

/// A workspace entry with full status for table display.
///
/// Columns align with development-plan.md Phase 6 requirements:
/// workspace, worktree_path, worktree_exists, session_name, session_status,
/// snapshot_status, last_used_epoch_secs.
#[derive(Debug)]
pub struct WorkspaceEntry {
    pub name: String,
    /// Stable structural identity (project root vs. linked worktree).
    pub kind: WorkspaceKind,
    pub worktree_path: Utf8PathBuf,
    pub worktree_exists: bool,
    pub session_name: String,
    pub session_active: bool,
    pub has_snapshot: bool,
    /// Unix epoch seconds of the last time Workhive successfully opened or
    /// attached this workspace, if available.
    ///
    /// The core boundary keeps this as a timezone-neutral raw epoch value;
    /// human-readable local-time formatting is the CLI rendering layer's
    /// responsibility so the same value can drive localized displays.
    pub last_used_epoch_secs: Option<u64>,
    /// Number of active units (panes) when the session is running.
    pub pane_count: Option<u32>,
    /// Concise layout summary: template name, pane count, or "-".
    pub layout_info: String,
    /// Session availability state for this workspace.
    pub session_state: SessionAvailability,
    /// Explicit snapshot probe result — never silently degraded.
    pub snapshot_status: SnapshotStatus,
    /// Explicit pane count enrichment result — never silently degraded.
    pub pane_count_status: PaneCountStatus,
}

/// List workspaces in the current project.
///
/// `server_name` and `session_name_fn` are resolved by the caller via
/// `SessionNamingPolicy`, keeping the use case config-free.
pub fn list_workspaces(
    ws_fs: &dyn WorkspaceFsPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    project_root: &camino::Utf8Path,
    server_name: &str,
    session_name_fn: &dyn Fn(&str) -> Result<String, NamingError>,
    state_key_fn: &dyn Fn(&str) -> Option<WorkspaceStateKey>,
) -> Result<Vec<WorkspaceEntry>, ListError> {
    let names = ws_fs.list_workspace_dirs(project_root)?;
    let server = server_name;

    let entries: Result<Vec<WorkspaceEntry>, ListError> = names
        .into_iter()
        .map(|name| {
            let session_name = session_name_fn(&name)?;

            let lookup_result = ws_runtime.has_workspace(server, &session_name);
            let session_state = classify_session_availability(lookup_result);
            let active = session_state == SessionAvailability::Running;

            let pane_count_status = if active {
                resolve_active_pane_count(ws_runtime, server, &session_name, &name)
            } else {
                PaneCountStatus::NotApplicable
            };
            let pane_count = pane_count_status.as_option();

            let wt_dir = ws_fs
                .workhive_dir_path(project_root)
                .join("worktrees")
                .join(&name);
            let worktree_exists = ws_fs.worktree_dir_exists(project_root, &name);

            let last_used_epoch_secs = ws_fs.workspace_last_used_epoch_secs(project_root, &name);

            let snapshot_status = match state_key_fn(&name) {
                Some(state_key) => match snapshot_store.load_latest(&state_key) {
                    Ok(Some(_)) => SnapshotStatus::Available,
                    Ok(None) => SnapshotStatus::Absent,
                    Err(e) => {
                        tracing::warn!(
                            workspace = %name,
                            state_key = %state_key,
                            error = %e,
                            "snapshot probe failed; marking as unknown"
                        );
                        SnapshotStatus::Unknown
                    }
                },
                None => {
                    // Expected degraded state: the workspace has no resolvable
                    // identity yet (no project info file or workspace not
                    // provisioned). This MUST NOT pollute normal `workspace
                    // list` output as a `WARN` line; keep it at `debug`.
                    tracing::debug!(
                        workspace = %name,
                        "state-key resolution failed during snapshot probe; marking as unknown"
                    );
                    SnapshotStatus::Unknown
                }
            };
            let has_snapshot = snapshot_status.is_available();

            let layout_info = match pane_count {
                Some(c) => format!("{c} panes"),
                None => "-".to_owned(),
            };

            Ok(WorkspaceEntry {
                session_name,
                name,
                kind: WorkspaceKind::LinkedWorktree,
                worktree_path: wt_dir,
                worktree_exists,
                session_active: active,
                has_snapshot,
                last_used_epoch_secs,
                pane_count,
                layout_info,
                session_state,
                snapshot_status,
                pane_count_status,
            })
        })
        .collect();

    entries
}

/// Summary of a project for `list --all-projects`.
#[derive(Debug)]
pub struct ProjectEntry {
    pub name: String,
    pub root: String,
}

/// Result of listing all projects, preserving index three-state semantics.
#[derive(Debug)]
pub enum ProjectListResult {
    /// The index file does not exist on disk.
    IndexNotFound,
    /// The index was loaded (may be empty).
    Loaded(Vec<ProjectEntry>),
}

/// Build a `WorkspaceEntry` representing the main branch (project root).
///
/// Queries git for the current branch, assembles the naming context,
/// resolves the session name via `SessionNamingPolicy`, checks runtime
/// for session existence, and probes snapshot storage.
///
/// The `main_label` parameter is a pre-resolved i18n string — this function
/// does not perform i18n resolution itself. The `cwd` parameter is the
/// current working directory passed by the CLI (no `std::env` reads here).
#[allow(clippy::too_many_arguments)]
pub fn build_main_workspace_entry(
    git: &dyn crate::ports::GitPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    snapshot_store: &dyn SnapshotStorePort,
    root: &Utf8PathBuf,
    project_name: &str,
    server: &str,
    policy: &crate::domain::SessionNamingPolicy,
    main_label: &str,
    cwd: Option<&camino::Utf8Path>,
    state_key_fn: &dyn Fn(&str) -> Option<WorkspaceStateKey>,
) -> Result<WorkspaceEntry, ListError> {
    let main_branch = git
        .list_worktrees()?
        .into_iter()
        .find(|worktree| worktree.is_main)
        .and_then(|worktree| worktree.branch)
        .map(Ok)
        .unwrap_or_else(|| git.current_branch())?;
    let instance_id = ws_fs.read_instance_id(root.as_ref());
    let naming_ctx = crate::usecases::naming::assemble_naming_context(
        project_name,
        &main_branch,
        root,
        Some(main_branch.clone()),
        instance_id,
        cwd,
    );
    let session_name = policy.resolve_session(&naming_ctx)?;
    // The project-root row's name is the bare branch identity (e.g. `main`).
    // Project-root semantics are conveyed by the human-output group header,
    // not by mixing an explanatory label into the identity field. The
    // `main_label` input is retained for signature stability across callers.
    let _ = main_label;
    let display_name = main_branch.clone();

    let (session_active, session_state, pane_count_status) = match ws_runtime
        .has_workspace(server, &session_name)
    {
        Ok(true) => {
            let pcs = resolve_active_pane_count(ws_runtime, server, &session_name, &main_branch);
            (true, SessionAvailability::Running, pcs)
        }
        Ok(false) => (
            false,
            SessionAvailability::Missing,
            PaneCountStatus::NotApplicable,
        ),
        Err(RuntimeError::BackendUnavailable { .. }) => (
            false,
            SessionAvailability::ServerUnreachable,
            PaneCountStatus::NotApplicable,
        ),
        Err(_) => (
            false,
            SessionAvailability::LookupFailed,
            PaneCountStatus::NotApplicable,
        ),
    };
    let pane_count = pane_count_status.as_option();

    let snapshot_status = match state_key_fn(&main_branch) {
        Some(state_key) => match snapshot_store.load_latest(&state_key) {
            Ok(Some(_)) => SnapshotStatus::Available,
            Ok(None) => SnapshotStatus::Absent,
            Err(e) => {
                tracing::warn!(
                    workspace = %main_branch,
                    state_key = %state_key,
                    error = %e,
                    "snapshot probe failed for main branch; marking as unknown"
                );
                SnapshotStatus::Unknown
            }
        },
        None => {
            // Expected degraded state for the main branch entry: the
            // project-info file is missing or the main branch has not yet
            // been provisioned. Keep this at `debug` level so default
            // `workspace list` output stays clean.
            tracing::debug!(
                workspace = %main_branch,
                "state-key resolution failed for main branch snapshot probe; marking as unknown"
            );
            SnapshotStatus::Unknown
        }
    };
    let has_snapshot = snapshot_status.is_available();

    let layout_info = match pane_count {
        Some(c) => format!("{c} panes"),
        None => "-".to_owned(),
    };

    let last_used_epoch_secs = ws_fs.workspace_last_used_epoch_secs(root.as_ref(), &main_branch);

    Ok(WorkspaceEntry {
        name: display_name,
        kind: WorkspaceKind::ProjectRoot,
        worktree_path: root.clone(),
        worktree_exists: false,
        session_name,
        session_active,
        has_snapshot,
        last_used_epoch_secs,
        pane_count,
        layout_info,
        session_state,
        snapshot_status,
        pane_count_status,
    })
}

/// List all registered projects with three-state index awareness.
pub fn list_all_projects(index: &dyn ProjectIndexPort) -> Result<ProjectListResult, ListError> {
    let state = index.list_projects()?;
    match state {
        IndexState::NotFound => Ok(ProjectListResult::IndexNotFound),
        IndexState::Loaded(projects) => Ok(ProjectListResult::Loaded(
            projects
                .into_iter()
                .map(|p| ProjectEntry {
                    name: p.project_name,
                    root: p.project_root.to_string(),
                })
                .collect(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        LaunchBaseline, SnapshotEvent, SnapshotHealthCheck, SnapshotHistoryEntry, SnapshotManifest,
        SnapshotRevisionId, SupervisorLock, WorkspaceSnapshot,
    };
    use crate::errors::SnapshotError;
    use crate::ports::{
        AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, EnsureWorktreeRequest, GitPort,
        WorkspaceRuntimeInfo, WorktreeInfo, WorktreeRemovePolicy, WorktreeStatus,
    };
    use camino::Utf8Path;

    /// Minimal runtime stub: only `list_workspaces` is meaningful; every
    /// other capability panics so the test cannot accidentally depend on it.
    struct StubRuntime {
        list_result: Result<Vec<WorkspaceRuntimeInfo>, RuntimeError>,
    }

    impl WorkspaceRuntimePort for StubRuntime {
        fn has_workspace(&self, _group: &str, _workspace_name: &str) -> Result<bool, RuntimeError> {
            unreachable!("resolve_active_pane_count must not call has_workspace")
        }
        fn create_workspace(
            &self,
            _request: CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn configure_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn detach_workspace(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn destroy_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            match &self.list_result {
                Ok(v) => Ok(v.clone()),
                Err(_) => Err(RuntimeError::PermissionDenied {
                    detail: "stub list failure".to_owned(),
                }),
            }
        }
        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    fn info(name: &str, units: u32) -> WorkspaceRuntimeInfo {
        WorkspaceRuntimeInfo {
            workspace_name: name.to_owned(),
            unit_count: units,
            attached: false,
        }
    }

    struct ListRuntime;

    impl WorkspaceRuntimePort for ListRuntime {
        fn has_workspace(&self, _group: &str, _workspace_name: &str) -> Result<bool, RuntimeError> {
            Ok(false)
        }
        fn create_workspace(
            &self,
            _request: CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn configure_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn detach_workspace(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn destroy_workspace(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn list_workspaces(&self, _group: &str) -> Result<Vec<WorkspaceRuntimeInfo>, RuntimeError> {
            Ok(Vec::new())
        }
        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    struct ListWorkspaceFs {
        root: Utf8PathBuf,
        workspaces: Vec<String>,
        last_used: std::collections::HashMap<(Utf8PathBuf, String), u64>,
    }

    impl ListWorkspaceFs {
        fn new(root: Utf8PathBuf, workspaces: Vec<&str>) -> Self {
            Self {
                root,
                workspaces: workspaces.into_iter().map(str::to_owned).collect(),
                last_used: std::collections::HashMap::new(),
            }
        }

        fn with_last_used(mut self, workspace: &str, epoch_secs: u64) -> Self {
            self.last_used
                .insert((self.root.clone(), workspace.to_owned()), epoch_secs);
            self
        }
    }

    impl WorkspaceFsPort for ListWorkspaceFs {
        fn project_root_exists(&self, _project_root: &Utf8Path) -> bool {
            true
        }
        fn create_workhive_dir(&self, _project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
        fn remove_workhive_dir(&self, _project_root: &Utf8Path) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
        fn workhive_dir_exists(&self, _project_root: &Utf8Path) -> bool {
            true
        }
        fn workhive_dir_path(&self, project_root: &Utf8Path) -> Utf8PathBuf {
            project_root.join(".workhive")
        }
        fn list_workspace_dirs(
            &self,
            _project_root: &Utf8Path,
        ) -> Result<Vec<String>, WorkspaceFsError> {
            Ok(self.workspaces.clone())
        }
        fn read_instance_id(&self, _project_root: &Utf8Path) -> Option<String> {
            Some("inst".to_owned())
        }
        fn write_instance_id(
            &self,
            _project_root: &Utf8Path,
            _id: &str,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
        fn project_config_exists(&self, _project_root: &Utf8Path) -> bool {
            true
        }
        fn worktree_dir_exists(&self, _project_root: &Utf8Path, worktree_name: &str) -> bool {
            self.workspaces.iter().any(|w| w == worktree_name)
        }
        fn workspace_last_used_epoch_secs(
            &self,
            project_root: &Utf8Path,
            workspace_name: &str,
        ) -> Option<u64> {
            self.last_used
                .get(&(project_root.to_owned(), workspace_name.to_owned()))
                .copied()
        }
        fn mark_workspace_used_epoch_secs(
            &self,
            _project_root: &Utf8Path,
            _workspace_name: &str,
            _epoch_secs: u64,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
        fn commit_staged_workhive(
            &self,
            _staging_root: &Utf8Path,
            _project_root: &Utf8Path,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
        fn cleanup_staging(&self, _staging_root: &Utf8Path) {}
        fn ensure_gitignore_workhive_entry(
            &self,
            _project_root: &Utf8Path,
        ) -> Result<(), WorkspaceFsError> {
            unreachable!()
        }
    }

    struct EmptySnapshotStore;

    impl SnapshotStorePort for EmptySnapshotStore {
        fn save_revision(
            &self,
            _snapshot: &WorkspaceSnapshot,
        ) -> Result<SnapshotRevisionId, SnapshotError> {
            unreachable!()
        }
        fn load_latest(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
            Ok(None)
        }
        fn load_revision(
            &self,
            _state_key: &WorkspaceStateKey,
            _revision_id: SnapshotRevisionId,
        ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
            unreachable!()
        }
        fn append_events(
            &self,
            _state_key: &WorkspaceStateKey,
            _events: &[SnapshotEvent],
        ) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn load_events_since(
            &self,
            _state_key: &WorkspaceStateKey,
            _since_revision: SnapshotRevisionId,
        ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
            unreachable!()
        }
        fn read_manifest(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<SnapshotManifest>, SnapshotError> {
            unreachable!()
        }
        fn write_manifest(&self, _manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn list_history(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
            unreachable!()
        }
        fn acquire_lock(&self, _lock: &SupervisorLock) -> Result<bool, SnapshotError> {
            unreachable!()
        }
        fn release_lock(&self, _state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn update_heartbeat(
            &self,
            _state_key: &WorkspaceStateKey,
            _heartbeat: &str,
        ) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn read_lock(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<SupervisorLock>, SnapshotError> {
            unreachable!()
        }
        fn health_check(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<SnapshotHealthCheck, SnapshotError> {
            unreachable!()
        }
        fn verify_revision(
            &self,
            _state_key: &WorkspaceStateKey,
            _revision_id: SnapshotRevisionId,
        ) -> Result<bool, SnapshotError> {
            unreachable!()
        }
        fn apply_retention(
            &self,
            _state_key: &WorkspaceStateKey,
            _retain_revisions: u32,
            _retain_event_days: u32,
        ) -> Result<u32, SnapshotError> {
            unreachable!()
        }
        fn delete_all(&self, _state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn delete_all_global(&self) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn save_launch_baseline(&self, _baseline: &LaunchBaseline) -> Result<(), SnapshotError> {
            unreachable!()
        }
        fn load_launch_baseline(
            &self,
            _state_key: &WorkspaceStateKey,
        ) -> Result<Option<LaunchBaseline>, SnapshotError> {
            unreachable!()
        }
    }

    struct MainBranchGit;

    impl GitPort for MainBranchGit {
        fn ensure_worktree(
            &self,
            _request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, GitError> {
            unreachable!()
        }
        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), GitError> {
            unreachable!()
        }
        fn worktree_status(&self, _path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
            unreachable!()
        }
        fn unmerged_commit_count(&self, _base: &str, _branch: &str) -> Result<u32, GitError> {
            unreachable!()
        }
        fn delete_branch(&self, _branch: &str) -> Result<(), GitError> {
            unreachable!()
        }
        fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
            unreachable!()
        }
        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
            Ok(vec![WorktreeInfo {
                path: Utf8PathBuf::from("/tmp/project"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            }])
        }
        fn current_branch(&self) -> Result<String, GitError> {
            Ok("main".to_owned())
        }
        fn branch_exists(&self, _branch: &str) -> Result<bool, GitError> {
            unreachable!()
        }
        fn has_commits(&self) -> Result<bool, GitError> {
            unreachable!()
        }
        fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
            unreachable!()
        }
        fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
            unreachable!()
        }
    }

    #[test]
    fn active_pane_count_known_when_record_present() {
        let runtime = StubRuntime {
            list_result: Ok(vec![info("wh@x@proj@main", 3)]),
        };
        assert_eq!(
            resolve_active_pane_count(&runtime, "srv", "wh@x@proj@main", "main"),
            PaneCountStatus::Known(3),
            "a matching runtime record must report its real pane count"
        );
    }

    #[test]
    fn active_pane_count_unknown_when_record_missing() {
        // Session is reported active by has_workspace, but the workspace
        // listing has no matching record (e.g. it died between probes).
        // Pane count must be Unknown rather than a fabricated Known(0).
        let runtime = StubRuntime {
            list_result: Ok(vec![info("wh@x@proj@other", 2)]),
        };
        assert_eq!(
            resolve_active_pane_count(&runtime, "srv", "wh@x@proj@main", "main"),
            PaneCountStatus::Unknown,
            "an active session with no matching record must be Unknown, never Known(0)"
        );
    }

    #[test]
    fn active_pane_count_unknown_when_listing_fails() {
        let runtime = StubRuntime {
            list_result: Err(RuntimeError::PermissionDenied {
                detail: "denied".to_owned(),
            }),
        };
        assert_eq!(
            resolve_active_pane_count(&runtime, "srv", "wh@x@proj@main", "main"),
            PaneCountStatus::Unknown,
            "a failed listing must degrade to Unknown"
        );
    }

    #[test]
    fn linked_worktree_last_used_comes_from_workspace_usage_metadata() {
        let root = Utf8PathBuf::from("/tmp/project");
        let fs = ListWorkspaceFs::new(root.clone(), vec!["feature-a"])
            .with_last_used("feature-a", 1_714_800_000);
        let runtime = ListRuntime;
        let snapshots = EmptySnapshotStore;

        let entries = list_workspaces(
            &fs,
            &runtime,
            &snapshots,
            root.as_ref(),
            "server",
            &|workspace| Ok(format!("session-{workspace}")),
            &|workspace| {
                Some(WorkspaceStateKey::new(
                    "inst".to_owned(),
                    "project".to_owned(),
                    String::new(),
                    workspace.to_owned(),
                ))
            },
        )
        .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "feature-a");
        // The core boundary returns the raw epoch seconds verbatim;
        // local-time formatting is the CLI rendering layer's job.
        assert_eq!(entries[0].last_used_epoch_secs, Some(1_714_800_000));
    }

    #[test]
    fn project_root_last_used_uses_current_branch_workspace_name() {
        let root = Utf8PathBuf::from("/tmp/project");
        let fs =
            ListWorkspaceFs::new(root.clone(), Vec::new()).with_last_used("main", 1_714_800_060);
        let runtime = ListRuntime;
        let snapshots = EmptySnapshotStore;

        let entry = build_main_workspace_entry(
            &MainBranchGit,
            &runtime,
            &fs,
            &snapshots,
            &root,
            "project",
            "server",
            &crate::domain::SessionNamingPolicy::default_policy(),
            "(main)",
            None,
            &|workspace| {
                Some(WorkspaceStateKey::new(
                    "inst".to_owned(),
                    "project".to_owned(),
                    String::new(),
                    workspace.to_owned(),
                ))
            },
        )
        .unwrap();

        assert_eq!(entry.kind, WorkspaceKind::ProjectRoot);
        assert_eq!(entry.name, "main");
        assert_eq!(entry.last_used_epoch_secs, Some(1_714_800_060));
    }

    struct LinkedCwdGit;

    impl GitPort for LinkedCwdGit {
        fn ensure_worktree(
            &self,
            _request: EnsureWorktreeRequest,
        ) -> Result<WorktreeInfo, GitError> {
            unreachable!()
        }
        fn remove_worktree(
            &self,
            _path: &Utf8Path,
            _policy: WorktreeRemovePolicy,
        ) -> Result<(), GitError> {
            unreachable!()
        }
        fn worktree_status(&self, _path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
            unreachable!()
        }
        fn unmerged_commit_count(&self, _base: &str, _branch: &str) -> Result<u32, GitError> {
            unreachable!()
        }
        fn delete_branch(&self, _branch: &str) -> Result<(), GitError> {
            unreachable!()
        }
        fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
            unreachable!()
        }
        fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
            Ok(vec![
                WorktreeInfo {
                    path: Utf8PathBuf::from("/tmp/project"),
                    branch: Some("main".to_owned()),
                    is_main: true,
                    resolved_strategy: None,
                },
                WorktreeInfo {
                    path: Utf8PathBuf::from("/tmp/project/.workhive/worktrees/feat-new"),
                    branch: Some("feat-new".to_owned()),
                    is_main: false,
                    resolved_strategy: None,
                },
            ])
        }
        fn current_branch(&self) -> Result<String, GitError> {
            Ok("feat-new".to_owned())
        }
        fn branch_exists(&self, _branch: &str) -> Result<bool, GitError> {
            unreachable!()
        }
        fn has_commits(&self) -> Result<bool, GitError> {
            unreachable!()
        }
        fn create_empty_initial_commit(&self, _message: &str) -> Result<(), GitError> {
            unreachable!()
        }
        fn init_repo(&self, _path: &Utf8Path) -> Result<(), GitError> {
            unreachable!()
        }
    }

    #[test]
    fn project_root_entry_uses_main_worktree_branch_when_cwd_is_linked_worktree() {
        let root = Utf8PathBuf::from("/tmp/project");
        let fs =
            ListWorkspaceFs::new(root.clone(), Vec::new()).with_last_used("main", 1_714_800_060);
        let runtime = ListRuntime;
        let snapshots = EmptySnapshotStore;

        let entry = build_main_workspace_entry(
            &LinkedCwdGit,
            &runtime,
            &fs,
            &snapshots,
            &root,
            "project",
            "server",
            &crate::domain::SessionNamingPolicy::default_policy(),
            "(main)",
            Some(Utf8Path::new("/tmp/project/.workhive/worktrees/feat-new")),
            &|workspace| {
                Some(WorkspaceStateKey::new(
                    "inst".to_owned(),
                    "project".to_owned(),
                    String::new(),
                    workspace.to_owned(),
                ))
            },
        )
        .unwrap();

        assert_eq!(entry.kind, WorkspaceKind::ProjectRoot);
        assert_eq!(entry.name, "main");
        assert_eq!(entry.last_used_epoch_secs, Some(1_714_800_060));
    }

    #[test]
    fn workspace_kind_tokens_are_stable() {
        assert_eq!(WorkspaceKind::ProjectRoot.as_token(), "project_root");
        assert_eq!(WorkspaceKind::LinkedWorktree.as_token(), "linked_worktree");
    }
}

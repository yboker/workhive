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

/// Map port-level `ProvisionOutcome` to domain `MaterializationKind`.
pub(crate) fn materialization_from_provision(outcome: ProvisionOutcome) -> MaterializationKind {
    match outcome {
        ProvisionOutcome::ReusedExistingRoot => MaterializationKind::ReusedExisting,
        ProvisionOutcome::MaterializedFromExistingReference => {
            MaterializationKind::FromExistingReference
        }
        ProvisionOutcome::MaterializedFromNewReference => MaterializationKind::FromNewReference,
    }
}

pub(crate) fn is_managed_worktree_cwd(project_root: &Utf8PathBuf, cwd: &camino::Utf8Path) -> bool {
    cwd.strip_prefix(project_root.as_path())
        .ok()
        .map(|suffix| {
            let suffix_str = suffix.as_str();
            suffix_str == ".workhive/worktrees" || suffix_str.starts_with(".workhive/worktrees/")
        })
        .unwrap_or(false)
}

/// Resolve the runtime cwd for fresh `workspace open` session/pane creation.
///
/// This pure helper keeps repository-root state authority separate from
/// the user-facing cwd used by interactive runtime panes. The
/// regression we are protecting is:
///
/// > `cd project/subdir && workhive workspace open` should leave the
/// > fresh runtime/pane in `project/subdir`, not snap back to the
/// > project root.
///
/// Rules (intentionally minimal):
///
/// 1. `Provision` intent → `provisioned_workspace_root` if provided,
///    otherwise `project_root`.
/// 2. No invocation cwd available → `project_root`.
/// 3. Invocation cwd equals project root → `project_root` (a no-op,
///    but kept explicit so subdir matching does not fire).
/// 4. Invocation cwd lives inside `.workhive/worktrees/...` →
///    `project_root` (those are managed worktrees, never user dirs).
/// 5. Invocation cwd is anywhere else inside the project →
///    invocation cwd (this is the regression-prevention).
/// 6. Anything outside the project → `project_root`.
///
/// No metadata struct, no enum tagging the decision, no public debug
/// surface: only the path the caller actually needs.
pub(crate) fn resolve_runtime_cwd(
    project_root: &Utf8PathBuf,
    invocation_cwd: Option<&camino::Utf8Path>,
    intent: WorkspaceOpenIntent,
    provisioned_workspace_root: Option<&camino::Utf8Path>,
) -> Utf8PathBuf {
    if let WorkspaceOpenIntent::Provision = intent
        && let Some(root) = provisioned_workspace_root
    {
        return root.to_path_buf();
    }

    let Some(cwd) = invocation_cwd else {
        return project_root.clone();
    };

    if cwd == project_root.as_path() {
        return project_root.clone();
    }

    if is_managed_worktree_cwd(project_root, cwd) {
        return project_root.clone();
    }

    if cwd.strip_prefix(project_root.as_path()).is_ok() {
        return cwd.to_path_buf();
    }

    project_root.clone()
}

/// Map a `has_workspace` result into `SessionAvailability`.
pub fn classify_session_availability(result: Result<bool, RuntimeError>) -> SessionAvailability {
    match result {
        Ok(true) => SessionAvailability::Running,
        Ok(false) => SessionAvailability::Missing,
        Err(RuntimeError::BackendUnavailable { .. }) => SessionAvailability::ServerUnreachable,
        Err(RuntimeError::WorkspaceNotFound { .. }) => SessionAvailability::Missing,
        Err(_) => SessionAvailability::LookupFailed,
    }
}

/// Discover an existing session for `open`.
///
/// Canonical `@` naming is always preferred. Legacy `--` naming is returned
/// only when canonical is absent and legacy compatibility lookup succeeds.
pub fn resolve_open_session(
    ws_runtime: &dyn WorkspaceRuntimePort,
    new_server: &str,
    new_session: &str,
    project_name: &str,
    workspace: &str,
) -> Result<Option<ExistingSession>, OpenError> {
    use crate::domain::SessionNamingPolicy;

    let canonical_exists = match ws_runtime.has_workspace(new_server, new_session) {
        Ok(exists) => exists,
        Err(RuntimeError::BackendUnavailable { .. }) => false,
        Err(err) => return Err(OpenError::Runtime(err)),
    };

    if canonical_exists {
        return Ok(Some(ExistingSession::Canonical {
            server: new_server.to_owned(),
            session: new_session.to_owned(),
        }));
    }

    let legacy_server = SessionNamingPolicy::legacy_server(project_name);
    let legacy_session = SessionNamingPolicy::legacy_session(project_name, workspace);
    let legacy_exists = if legacy_session != new_session {
        match ws_runtime.has_workspace(&legacy_server, &legacy_session) {
            Ok(exists) => exists,
            Err(RuntimeError::BackendUnavailable { .. }) => false,
            Err(err) => return Err(OpenError::Runtime(err)),
        }
    } else {
        false
    };

    if legacy_exists {
        tracing::debug!(
            legacy_server = %legacy_server,
            legacy_session = %legacy_session,
            new_server = %new_server,
            new_session = %new_session,
            "resolve_open_session: falling back to legacy session"
        );
        return Ok(Some(ExistingSession::Legacy {
            server: legacy_server,
            session: legacy_session,
        }));
    }

    Ok(None)
}

/// Stage 1+2: Validate inputs and prepare the execution plan.
///
/// This performs read-only queries (worktree existence, session existence)
/// but does NOT create anything. The returned `OpenPlan` is a pure data
/// description of what `execute_open_plan` will do.
///
/// `snapshot_enabled` must come from the project config (`snapshot.enabled`).
/// `open_mode` controls Normal vs ForceRecreateFromLayout behavior.
#[allow(clippy::too_many_arguments)]
pub fn prepare_open_plan(
    provision: &dyn WorkspaceProvisionPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    project_root: &Utf8PathBuf,
    workspace: &WorkspaceId,
    server_name: &str,
    session_name: &str,
    intent: WorkspaceOpenIntent,
    snapshot_enabled: bool,
    open_mode: OpenMode,
    snapshot_identity: Option<SnapshotIdentityContext>,
) -> Result<OpenPlan, OpenError> {
    prepare_open_plan_with_invocation_cwd(
        provision,
        ws_runtime,
        ws_fs,
        project_root,
        workspace,
        server_name,
        session_name,
        intent,
        None,
        snapshot_enabled,
        open_mode,
        snapshot_identity,
    )
}

/// Stage 1+2 variant that receives the caller's cwd explicitly.
#[allow(clippy::too_many_arguments)]
pub fn prepare_open_plan_with_invocation_cwd(
    provision: &dyn WorkspaceProvisionPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    project_root: &Utf8PathBuf,
    workspace: &WorkspaceId,
    server_name: &str,
    session_name: &str,
    intent: WorkspaceOpenIntent,
    invocation_cwd: Option<&camino::Utf8Path>,
    snapshot_enabled: bool,
    open_mode: OpenMode,
    snapshot_identity: Option<SnapshotIdentityContext>,
) -> Result<OpenPlan, OpenError> {
    let (working_dir, provenance) = match intent {
        WorkspaceOpenIntent::UseProjectRoot => {
            tracing::info!(
                workspace = %workspace.as_str(),
                intent = "UseProjectRoot",
                "open: using project root"
            );
            (project_root.clone(), WorkspaceProvenance::ProjectRoot)
        }
        WorkspaceOpenIntent::Provision => {
            let worktree_base = ws_fs
                .workhive_dir_path(project_root.as_ref())
                .join("worktrees");

            let root_info = provision.ensure_workspace_root(ProvisionWorkspaceRequest {
                base_path: worktree_base,
                workspace_name: workspace.as_str().to_owned(),
            })?;

            let materialization = materialization_from_provision(root_info.outcome);
            let provenance = WorkspaceProvenance::Provisioned {
                provider: WorkspaceProvider::Git,
                materialization,
            };

            tracing::info!(
                workspace = %workspace.as_str(),
                intent = "Provision",
                worktree_path = %root_info.path,
                materialization = ?materialization,
                "open: worktree decision context"
            );

            (root_info.path, provenance)
        }
    };

    let working_dir = resolve_runtime_cwd(
        project_root,
        invocation_cwd,
        intent,
        Some(working_dir.as_path()),
    );

    tracing::debug!(
        workspace = %workspace.as_str(),
        project_root = %project_root,
        invocation_cwd = ?invocation_cwd.map(|p| p.as_str()),
        runtime_cwd = %working_dir,
        "open: resolved runtime cwd"
    );

    let availability_result = ws_runtime.has_workspace(server_name, session_name);
    let session_availability = classify_session_availability(availability_result);

    if session_availability.is_error() {
        tracing::warn!(
            server = %server_name,
            session = %session_name,
            availability = %session_availability,
            "session lookup failed — cannot proceed with open"
        );
        return Err(OpenError::SessionLookupFailed {
            session: session_name.to_owned(),
            availability: session_availability,
        });
    }

    Ok(OpenPlan {
        workspace: workspace.clone(),
        provenance,
        server: server_name.to_owned(),
        session: session_name.to_owned(),
        session_availability,
        working_dir,
        snapshot_enabled,
        open_mode,
        snapshot_identity,
        env: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use crate::ports::{AttachWorkspaceRequest, SendInputRequest, WorkspaceRuntimeInfo};

    #[test]
    fn classify_running() {
        assert_eq!(
            classify_session_availability(Ok(true)),
            SessionAvailability::Running
        );
    }

    #[test]
    fn classify_missing() {
        assert_eq!(
            classify_session_availability(Ok(false)),
            SessionAvailability::Missing
        );
    }

    #[test]
    fn classify_server_unreachable() {
        assert_eq!(
            classify_session_availability(Err(RuntimeError::BackendUnavailable {
                reason: crate::ports::BackendUnavailableReason::Other,
                detail: "socket gone".to_owned()
            })),
            SessionAvailability::ServerUnreachable
        );
    }

    #[test]
    fn classify_lookup_failed() {
        assert_eq!(
            classify_session_availability(Err(RuntimeError::OperationFailed {
                message: "garbled".to_owned()
            })),
            SessionAvailability::LookupFailed
        );
    }

    #[test]
    fn classify_workspace_not_found_maps_to_missing() {
        assert_eq!(
            classify_session_availability(Err(RuntimeError::WorkspaceNotFound {
                workspace: "x".to_owned()
            })),
            SessionAvailability::Missing
        );
    }

    struct LookupRuntime {
        canonical: Result<bool, RuntimeError>,
        legacy: Result<bool, RuntimeError>,
    }

    fn cloned_lookup_result(result: &Result<bool, RuntimeError>) -> Result<bool, RuntimeError> {
        match result {
            Ok(value) => Ok(*value),
            Err(RuntimeError::BackendUnavailable { reason, detail }) => {
                Err(RuntimeError::BackendUnavailable {
                    reason: reason.clone(),
                    detail: detail.clone(),
                })
            }
            Err(RuntimeError::WorkspaceNotFound { workspace }) => {
                Err(RuntimeError::WorkspaceNotFound {
                    workspace: workspace.clone(),
                })
            }
            Err(RuntimeError::WorkspaceAlreadyExists { workspace }) => {
                Err(RuntimeError::WorkspaceAlreadyExists {
                    workspace: workspace.clone(),
                })
            }
            Err(RuntimeError::UnitNotFound { id }) => {
                Err(RuntimeError::UnitNotFound { id: id.clone() })
            }
            Err(RuntimeError::LastUnitCloseRejected) => Err(RuntimeError::LastUnitCloseRejected),
            Err(RuntimeError::OperationFailed { message }) => Err(RuntimeError::OperationFailed {
                message: message.clone(),
            }),
            Err(RuntimeError::PermissionDenied { detail }) => Err(RuntimeError::PermissionDenied {
                detail: detail.clone(),
            }),
        }
    }

    impl WorkspaceRuntimePort for LookupRuntime {
        fn has_workspace(&self, group: &str, workspace_name: &str) -> Result<bool, RuntimeError> {
            if group == "srv" && workspace_name == "sess" {
                return cloned_lookup_result(&self.canonical);
            }
            if group == "whsrv-workhive" && workspace_name == "wh--workhive--main" {
                return cloned_lookup_result(&self.legacy);
            }
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
            unreachable!()
        }

        fn destroy_group(&self, _group: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    #[test]
    fn resolve_open_session_treats_backend_unavailable_as_missing() {
        let runtime = LookupRuntime {
            canonical: Err(RuntimeError::BackendUnavailable {
                reason: crate::ports::BackendUnavailableReason::Other,
                detail: "socket missing".to_owned(),
            }),
            legacy: Err(RuntimeError::BackendUnavailable {
                reason: crate::ports::BackendUnavailableReason::Other,
                detail: "socket missing".to_owned(),
            }),
        };

        let session = resolve_open_session(&runtime, "srv", "sess", "workhive", "main")
            .expect("backend unavailable during probe should not abort open");

        assert_eq!(session, None);
    }

    // ── resolve_runtime_cwd: focused regression-prevention tests ──
    //
    // These four cases pin the cwd-resolution decision table.
    // They exercise only the path the helper actually returns,
    // not metadata. See the helper docstring for the full decision
    // table.

    #[test]
    fn resolve_runtime_cwd_uses_invocation_subdir_when_inside_project() {
        let root = Utf8PathBuf::from("/repo");
        let cwd = Utf8PathBuf::from("/repo/module");

        let resolved = resolve_runtime_cwd(
            &root,
            Some(cwd.as_path()),
            WorkspaceOpenIntent::UseProjectRoot,
            None,
        );

        assert_eq!(
            resolved, cwd,
            "fresh runtime/pane must start in invocation subdir"
        );
    }

    #[test]
    fn resolve_runtime_cwd_falls_back_to_project_root_for_root_invocation() {
        let root = Utf8PathBuf::from("/repo");

        let resolved = resolve_runtime_cwd(
            &root,
            Some(root.as_path()),
            WorkspaceOpenIntent::UseProjectRoot,
            None,
        );

        assert_eq!(resolved, root);
    }

    #[test]
    fn resolve_runtime_cwd_rejects_managed_worktree_invocation_dir() {
        let root = Utf8PathBuf::from("/repo");
        let cwd = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a");

        let resolved = resolve_runtime_cwd(
            &root,
            Some(cwd.as_path()),
            WorkspaceOpenIntent::UseProjectRoot,
            None,
        );

        assert_eq!(
            resolved, root,
            ".workhive/worktrees/* is a managed dir; never use as invocation cwd"
        );
    }

    #[test]
    fn resolve_runtime_cwd_uses_provisioned_workspace_root_for_provision_intent() {
        let root = Utf8PathBuf::from("/repo");
        let invocation_cwd = Utf8PathBuf::from("/repo/module");
        let provisioned = Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a");

        let resolved = resolve_runtime_cwd(
            &root,
            Some(invocation_cwd.as_path()),
            WorkspaceOpenIntent::Provision,
            Some(provisioned.as_path()),
        );

        assert_eq!(
            resolved, provisioned,
            "Provision intent must use provisioned workspace root, \
             not the invocation cwd"
        );
    }

    #[test]
    fn resolve_runtime_cwd_falls_back_to_project_root_for_outside_invocation() {
        let root = Utf8PathBuf::from("/repo");
        let outside = Utf8PathBuf::from("/elsewhere");

        let resolved = resolve_runtime_cwd(
            &root,
            Some(outside.as_path()),
            WorkspaceOpenIntent::UseProjectRoot,
            None,
        );

        assert_eq!(resolved, root);
    }

    #[test]
    fn resolve_runtime_cwd_falls_back_to_project_root_when_invocation_unavailable() {
        let root = Utf8PathBuf::from("/repo");

        let resolved = resolve_runtime_cwd(&root, None, WorkspaceOpenIntent::UseProjectRoot, None);

        assert_eq!(resolved, root);
    }

    #[test]
    fn prepare_open_plan_threads_invocation_subdir_into_working_dir() {
        struct NoProvision;
        impl WorkspaceProvisionPort for NoProvision {
            fn ensure_workspace_root(
                &self,
                _request: ProvisionWorkspaceRequest,
            ) -> Result<crate::ports::WorkspaceRootInfo, ProvisionError> {
                unreachable!("UseProjectRoot should not provision a workspace root")
            }

            fn remove_workspace_root(
                &self,
                _path: &camino::Utf8Path,
                _policy: crate::ports::WorktreeRemovePolicy,
            ) -> Result<(), ProvisionError> {
                unreachable!("UseProjectRoot should not remove a provisioned workspace root")
            }
        }

        struct NoFs;
        impl WorkspaceFsPort for NoFs {
            fn project_root_exists(&self, _project_root: &camino::Utf8Path) -> bool {
                true
            }
            fn create_workhive_dir(
                &self,
                _project_root: &camino::Utf8Path,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                unreachable!()
            }
            fn remove_workhive_dir(
                &self,
                _project_root: &camino::Utf8Path,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                unreachable!()
            }
            fn workhive_dir_exists(&self, _project_root: &camino::Utf8Path) -> bool {
                true
            }
            fn workhive_dir_path(&self, project_root: &camino::Utf8Path) -> Utf8PathBuf {
                project_root.join(".workhive")
            }
            fn list_workspace_dirs(
                &self,
                _project_root: &camino::Utf8Path,
            ) -> Result<Vec<String>, crate::errors::WorkspaceFsError> {
                Ok(Vec::new())
            }
            fn read_instance_id(&self, _project_root: &camino::Utf8Path) -> Option<String> {
                None
            }
            fn write_instance_id(
                &self,
                _project_root: &camino::Utf8Path,
                _id: &str,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                unreachable!()
            }
            fn project_config_exists(&self, _project_root: &camino::Utf8Path) -> bool {
                true
            }
            fn worktree_dir_exists(
                &self,
                _project_root: &camino::Utf8Path,
                _worktree_name: &str,
            ) -> bool {
                false
            }
            fn workspace_last_used_epoch_secs(
                &self,
                _project_root: &camino::Utf8Path,
                _workspace_name: &str,
            ) -> Option<u64> {
                None
            }
            fn mark_workspace_used_epoch_secs(
                &self,
                _project_root: &camino::Utf8Path,
                _workspace_name: &str,
                _epoch_secs: u64,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                Ok(())
            }
            fn commit_staged_workhive(
                &self,
                _staging_root: &camino::Utf8Path,
                _project_root: &camino::Utf8Path,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                unreachable!()
            }
            fn cleanup_staging(&self, _staging_root: &camino::Utf8Path) {}
            fn ensure_gitignore_workhive_entry(
                &self,
                _project_root: &camino::Utf8Path,
            ) -> Result<(), crate::errors::WorkspaceFsError> {
                unreachable!()
            }
        }

        let runtime = LookupRuntime {
            canonical: Ok(false),
            legacy: Ok(false),
        };
        let project_root = Utf8PathBuf::from("/repo");
        let invocation_cwd = Utf8PathBuf::from("/repo/module");
        let plan = prepare_open_plan_with_invocation_cwd(
            &NoProvision,
            &runtime,
            &NoFs,
            &project_root,
            &WorkspaceId::new("main").expect("valid workspace"),
            "srv",
            "sess",
            WorkspaceOpenIntent::UseProjectRoot,
            Some(invocation_cwd.as_path()),
            false,
            OpenMode::Normal,
            None,
        )
        .expect("plan should be prepared");

        assert_eq!(
            plan.working_dir, invocation_cwd,
            "open plan must propagate invocation subdir into working_dir for \
             fresh runtime panes"
        );
    }

    #[test]
    fn resolve_open_session_falls_back_to_legacy_when_canonical_missing() {
        let runtime = LookupRuntime {
            canonical: Ok(false),
            legacy: Ok(true),
        };

        let session = resolve_open_session(&runtime, "srv", "sess", "workhive", "main")
            .expect("legacy lookup should succeed");

        assert_eq!(
            session,
            Some(ExistingSession::Legacy {
                server: "whsrv-workhive".to_owned(),
                session: "wh--workhive--main".to_owned(),
            })
        );
    }
}

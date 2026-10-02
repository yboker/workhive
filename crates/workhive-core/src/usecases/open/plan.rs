#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};

#[allow(unused_imports)]
use camino::Utf8PathBuf;

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    MaterializationKind, SessionAvailability, WorkspaceId, WorkspaceOpenIntent,
    WorkspaceProvenance, WorkspaceProvider,
};
#[allow(unused_imports)]
use crate::ports::{
    DirectoryClassification, ProvisionOutcome, ProvisionWorkspaceRequest, RuntimeError,
    WorkspaceFsPort, WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// Resolve open intent from the authoritative Git worktree topology.
///
/// Only the branch attached to the canonical main worktree uses the project
/// root. Every other workspace uses the provision path, which may reuse an
/// existing linked worktree.
pub fn resolve_open_intent(
    worktrees: &[crate::ports::WorktreeInfo],
    workspace: &WorkspaceId,
) -> WorkspaceOpenIntent {
    let is_main_workspace = worktrees
        .iter()
        .any(|worktree| worktree.is_main && worktree.branch.as_deref() == Some(workspace.as_str()));

    if is_main_workspace {
        WorkspaceOpenIntent::UseProjectRoot
    } else {
        WorkspaceOpenIntent::Provision
    }
}

/// Resolve the open scope from Git worktree topology.
///
/// Uses `list_worktrees()` as topology authority. Finds the containing
/// worktree by longest root-prefix match. Computes `relative_project_path`
/// as the offset from the containing worktree root to `invocation_cwd`.
/// Derives the selected workspace root from intent and provision result.
/// Validates the final `working_dir` through `WorkspaceFsPort` before
/// returning.
///
/// Missing or non-directory mapped targets fail without root fallback.
pub fn resolve_open_scope(
    worktrees: &[crate::ports::WorktreeInfo],
    invocation_cwd: &camino::Utf8Path,
    selected_workspace_root: &Utf8PathBuf,
    provenance: WorkspaceProvenance,
    ws_fs: &dyn WorkspaceFsPort,
) -> Result<ResolvedOpenScope, OpenError> {
    use crate::ports::DirectoryClassification;

    let containing_root = worktrees
        .iter()
        .filter(|wt| invocation_cwd.starts_with(&wt.path))
        .max_by_key(|wt| wt.path.as_str().len())
        .map(|wt| &wt.path);

    let containing = containing_root.ok_or_else(|| OpenError::CwdOutsideTopology {
        cwd: invocation_cwd.to_path_buf(),
    })?;

    let relative_project_path = invocation_cwd
        .strip_prefix(containing)
        .ok()
        .filter(|suffix| !suffix.as_str().is_empty())
        .and_then(|suffix| crate::domain::RelativeProjectPath::new(suffix.as_str()).ok());

    let working_dir = match &relative_project_path {
        Some(rpp) => selected_workspace_root.join(rpp.as_str()),
        None => selected_workspace_root.clone(),
    };

    match ws_fs.classify_path(working_dir.as_ref()) {
        Ok(DirectoryClassification::Directory) => {}
        Ok(DirectoryClassification::Missing) => {
            return Err(OpenError::MappedTargetMissing { path: working_dir });
        }
        Ok(DirectoryClassification::NotDirectory) => {
            return Err(OpenError::MappedTargetNotDirectory { path: working_dir });
        }
        Err(e) => {
            return Err(OpenError::Runtime(
                crate::ports::RuntimeError::OperationFailed {
                    message: format!("filesystem inspection failed: {e}"),
                },
            ));
        }
    }

    Ok(ResolvedOpenScope {
        selected_workspace_root: selected_workspace_root.clone(),
        relative_project_path,
        working_dir,
        provenance,
    })
}

/// Resolve open topology scope with provisioning.
///
/// Production entry point called by `cmd_open` before naming, runtime
/// lookup, identity persistence, or runtime creation.
///
/// 1. Determines the selected workspace root from intent:
///    - `UseProjectRoot`: uses the canonical project root.
///    - `Provision`: provisions the worktree and uses the resulting path.
/// 2. Calls `resolve_open_scope` for topology-first scope resolution.
/// 3. Returns the resolved scope and the materialization outcome.
///
/// Missing or non-directory mapped targets fail before the caller can
/// perform naming, runtime, identity, or layout operations.
pub fn resolve_open_topology_scope(
    worktrees: &[crate::ports::WorktreeInfo],
    invocation_cwd: &camino::Utf8Path,
    project_root: &Utf8PathBuf,
    workspace: &WorkspaceId,
    intent: WorkspaceOpenIntent,
    provision: &dyn WorkspaceProvisionPort,
    ws_fs: &dyn WorkspaceFsPort,
) -> Result<(ResolvedOpenScope, Option<ProvisionOutcome>), OpenError> {
    let (selected_root, provenance, provision_outcome) = match intent {
        WorkspaceOpenIntent::UseProjectRoot => {
            (project_root.clone(), WorkspaceProvenance::ProjectRoot, None)
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
            (root_info.path, provenance, Some(root_info.outcome))
        }
    };

    let scope = resolve_open_scope(worktrees, invocation_cwd, &selected_root, provenance, ws_fs)?;
    Ok((scope, provision_outcome))
}

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

/// Map a `has_workspace` result into `SessionAvailability` for diagnostic
/// surfaces. Operational callers should use
/// [`classify_session_availability_strict`] so permission, capability, and
/// other runtime failures are not mistaken for an unreachable server.
pub fn classify_session_availability(result: Result<bool, RuntimeError>) -> SessionAvailability {
    match result {
        Ok(true) => SessionAvailability::Running,
        Ok(false) => SessionAvailability::Missing,
        Err(RuntimeError::BackendUnavailable { .. }) => SessionAvailability::ServerUnreachable,
        Err(RuntimeError::WorkspaceNotFound { .. }) => SessionAvailability::Missing,
        Err(_) => SessionAvailability::LookupFailed,
    }
}

/// Classify a session lookup without discarding non-absence failures.
///
/// A missing runtime is recoverable by open (the runtime can be created), and
/// a missing workspace is equivalent to `Ok(false)`. Every other failure is
/// returned to the caller so capability, permission, and protocol errors keep
/// their typed meaning.
pub fn classify_session_availability_strict(
    result: Result<bool, RuntimeError>,
) -> Result<SessionAvailability, RuntimeError> {
    match result {
        Ok(true) => Ok(SessionAvailability::Running),
        Ok(false) => Ok(SessionAvailability::Missing),
        Err(RuntimeError::BackendUnavailable { reason, .. }) if reason.is_runtime_absent() => {
            Ok(SessionAvailability::ServerUnreachable)
        }
        Err(RuntimeError::WorkspaceNotFound { .. }) => Ok(SessionAvailability::Missing),
        Err(error) => Err(error),
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

    let canonical_exists = match classify_session_availability_strict(
        ws_runtime.has_workspace(new_server, new_session),
    )
    .map_err(OpenError::Runtime)?
    {
        SessionAvailability::Running => true,
        SessionAvailability::Missing | SessionAvailability::ServerUnreachable => false,
        SessionAvailability::LookupFailed => {
            unreachable!("strict session availability classification never returns LookupFailed")
        }
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
        match classify_session_availability_strict(
            ws_runtime.has_workspace(&legacy_server, &legacy_session),
        )
        .map_err(OpenError::Runtime)?
        {
            SessionAvailability::Running => true,
            SessionAvailability::Missing | SessionAvailability::ServerUnreachable => false,
            SessionAvailability::LookupFailed => unreachable!(
                "strict session availability classification never returns LookupFailed"
            ),
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
    open_mode: OpenMode,
) -> Result<OpenPlan, OpenError> {
    let (selected_workspace_root, provenance) = match intent {
        WorkspaceOpenIntent::UseProjectRoot => {
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

            (root_info.path, provenance)
        }
    };

    match ws_fs.classify_path(selected_workspace_root.as_ref()) {
        Ok(DirectoryClassification::Directory) => {}
        Ok(DirectoryClassification::Missing) => {
            return Err(OpenError::MappedTargetMissing {
                path: selected_workspace_root,
            });
        }
        Ok(DirectoryClassification::NotDirectory) => {
            return Err(OpenError::MappedTargetNotDirectory {
                path: selected_workspace_root,
            });
        }
        Err(error) => {
            return Err(OpenError::Runtime(RuntimeError::OperationFailed {
                message: format!("filesystem inspection failed: {error}"),
            }));
        }
    }

    let scope = ResolvedOpenScope {
        selected_workspace_root: selected_workspace_root.clone(),
        relative_project_path: None,
        working_dir: selected_workspace_root,
        provenance,
    };
    prepare_open_plan_with_scope(
        ws_runtime,
        server_name,
        session_name,
        project_root,
        &scope,
        workspace,
        open_mode,
    )
}

/// Prepare an `OpenPlan` from an already-resolved `ResolvedOpenScope`.
///
/// Consumes the topology-first scope result produced by
/// `resolve_open_topology_scope`. Does not re-derive working directory
/// or relative project path from cwd.
pub fn prepare_open_plan_with_scope(
    ws_runtime: &dyn WorkspaceRuntimePort,
    server_name: &str,
    session_name: &str,
    project_root: &camino::Utf8Path,
    scope: &ResolvedOpenScope,
    workspace: &WorkspaceId,
    open_mode: OpenMode,
) -> Result<OpenPlan, OpenError> {
    let availability_result = ws_runtime.has_workspace(server_name, session_name);
    let session_availability =
        classify_session_availability_strict(availability_result).map_err(OpenError::Runtime)?;

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
        provenance: scope.provenance.clone(),
        server: server_name.to_owned(),
        session: session_name.to_owned(),
        session_availability,
        project_root: project_root.to_path_buf(),
        workspace_root: scope.selected_workspace_root.clone(),
        working_dir: scope.working_dir.clone(),
        defer_checkout_registration: false,
        relative_project_path: scope.relative_project_path.clone(),
        open_mode,
        env: Vec::new(),
        interaction_policy: crate::ports::RuntimeInteractionPolicy::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{
        AttachWorkspaceRequest, CreateWorkspaceRuntimeRequest, ProvisionError,
        ProvisionWorkspaceRequest, WorkspaceRuntimeInfo,
    };

    fn topology() -> Vec<crate::ports::WorktreeInfo> {
        vec![
            crate::ports::WorktreeInfo {
                path: Utf8PathBuf::from("/repo"),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            crate::ports::WorktreeInfo {
                path: Utf8PathBuf::from("/repo/.workhive/worktrees/feature-a"),
                branch: Some("feature-a".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ]
    }

    #[test]
    fn open_production_flow_main_topology_uses_project_root() {
        let intent = resolve_open_intent(&topology(), &WorkspaceId::new("main").unwrap());
        assert_eq!(intent, WorkspaceOpenIntent::UseProjectRoot);
    }

    #[test]
    fn open_production_flow_linked_topology_uses_provision_path() {
        let intent = resolve_open_intent(&topology(), &WorkspaceId::new("feature-a").unwrap());
        assert_eq!(intent, WorkspaceOpenIntent::Provision);
    }

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
    fn classify_session_availability_strict_propagates_capability_errors() {
        let error = RuntimeError::BackendUnavailable {
            reason: crate::ports::BackendUnavailableReason::CapabilityMissing {
                capability: "workspace.list".to_owned(),
            },
            detail: "unsupported".to_owned(),
        };
        assert_eq!(
            classify_session_availability_strict(Err(error.clone())),
            Err(error)
        );
        assert_eq!(
            classify_session_availability_strict(Err(RuntimeError::BackendUnavailable {
                reason: crate::ports::BackendUnavailableReason::ServerNotRunning {
                    server: "srv".to_owned(),
                },
                detail: "stopped".to_owned(),
            })),
            Ok(SessionAvailability::ServerUnreachable)
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
            Err(RuntimeError::WorkspaceGroupCloseRequired { workspace }) => {
                Err(RuntimeError::WorkspaceGroupCloseRequired {
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
            _policy: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }

        fn attach_workspace(&self, _request: AttachWorkspaceRequest) -> Result<(), RuntimeError> {
            unreachable!()
        }

        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
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
                reason: crate::ports::BackendUnavailableReason::ServerNotRunning {
                    server: "srv".to_owned(),
                },
                detail: "socket missing".to_owned(),
            }),
            legacy: Err(RuntimeError::BackendUnavailable {
                reason: crate::ports::BackendUnavailableReason::ServerNotRunning {
                    server: "srv".to_owned(),
                },
                detail: "socket missing".to_owned(),
            }),
        };

        let session = resolve_open_session(&runtime, "srv", "sess", "workhive", "main")
            .expect("backend unavailable during probe should not abort open");

        assert_eq!(session, None);
    }

    #[test]
    fn prepare_open_plan_builds_typed_root_scope() {
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
            fn classify_path(
                &self,
                _path: &camino::Utf8Path,
            ) -> Result<crate::ports::DirectoryClassification, crate::errors::WorkspaceFsError>
            {
                Ok(crate::ports::DirectoryClassification::Directory)
            }
        }

        let runtime = LookupRuntime {
            canonical: Ok(false),
            legacy: Ok(false),
        };
        let project_root = Utf8PathBuf::from("/repo");
        let plan = prepare_open_plan(
            &NoProvision,
            &runtime,
            &NoFs,
            &project_root,
            &WorkspaceId::new("main").expect("valid workspace"),
            "srv",
            "sess",
            WorkspaceOpenIntent::UseProjectRoot,
            OpenMode::Normal,
        )
        .expect("plan should be prepared");

        assert_eq!(
            plan.working_dir, project_root,
            "root-scope wrapper must keep the canonical project root"
        );
        assert!(plan.relative_project_path.is_none());
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

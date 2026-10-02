//! Identity metadata lifecycle — creation, validation, and consistency checks.
//!
//! These functions are called during `init` and `open` to ensure
//! project and workspace identity records exist and are consistent.

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::{
    IdentityError, NamingContext, ProjectIdentityRecord, RelativeProjectPath, RuntimeBackendId,
    RuntimeBinding, RuntimeReplacement, SessionNamingPolicy, WorkspaceIdentityKey,
    WorkspaceIdentityRecord,
};
use crate::ports::WorkspaceIdentityPort;

/// Canonicalize a path for comparison.
///
/// Returns the original path if canonicalization fails (e.g., the path
/// doesn't exist on disk). This is a best-effort normalization to handle
/// symlinks, `/private/tmp` vs `/tmp`, and other equivalent path forms.
pub fn canonicalize_best_effort(path: &Utf8Path) -> Utf8PathBuf {
    std::fs::canonicalize(path.as_std_path())
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok())
        .unwrap_or_else(|| path.to_owned())
}

/// Ensure project identity metadata exists and is consistent.
///
/// Called during `init` and `open` (auto-init path).
///
/// - If no record exists, creates one from the provided inputs.
/// - If a record exists and matches, returns it (idempotent).
/// - If a record exists but conflicts, returns a typed error.
pub fn ensure_project_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    instance_id: &str,
    project_root: &Utf8PathBuf,
    project_display: &str,
    canonical_project_component: &str,
) -> Result<ProjectIdentityRecord, IdentityError> {
    if let Some(existing) = identity_port.load_project_identity()? {
        if existing.instance_id != instance_id {
            return Err(IdentityError::InstanceIdConflict {
                stored: existing.instance_id,
                current: instance_id.to_owned(),
                project_root: project_root.clone(),
            });
        }

        // Path normalization: compare canonical forms to avoid false
        // mismatches from symlinks, /private/tmp vs /tmp, macOS case folding, etc.
        let canonical_stored = canonicalize_best_effort(&existing.project_root);
        let canonical_current = canonicalize_best_effort(project_root);
        if canonical_stored != canonical_current {
            return Err(IdentityError::ProjectRootConflict {
                stored: existing.project_root,
                current: project_root.clone(),
            });
        }

        if existing.project_display != project_display {
            return Err(IdentityError::ProjectDisplayConflict {
                stored: existing.project_display,
                current: project_display.to_owned(),
            });
        }
        if existing.canonical_project_component != canonical_project_component {
            return Err(IdentityError::CanonicalProjectConflict {
                stored: existing.canonical_project_component,
                current: canonical_project_component.to_owned(),
            });
        }
        return Ok(existing);
    }

    let record = ProjectIdentityRecord {
        instance_id: instance_id.to_owned(),
        project_root: project_root.clone(),
        project_display: project_display.to_owned(),
        canonical_project_component: canonical_project_component.to_owned(),
    };
    identity_port.save_project_identity(&record)?;
    Ok(record)
}

/// Explicitly repair the stored project root after the project directory was
/// moved or renamed. All workspace identities (and their runtime bindings)
/// remain untouched.
///
/// The instance-ID check only proves that the two files inside this one
/// directory are mutually consistent; because `instance_id` and
/// `project.json` move together, it cannot prove the directory is *the same
/// project*. Sameness is therefore the caller's burden: this function is only
/// reachable behind an explicit, confirmed repair path.
pub fn repair_project_identity_root(
    identity_port: &dyn WorkspaceIdentityPort,
    current_instance_id: &str,
    current_root: &Utf8Path,
) -> Result<ProjectIdentityRecord, IdentityError> {
    let existing = identity_port.load_project_identity()?.ok_or_else(|| {
        IdentityError::ProjectIdentityMissing {
            path: current_root
                .join(".workhive")
                .join("identity")
                .join("project.json"),
        }
    })?;
    if existing.instance_id != current_instance_id {
        return Err(IdentityError::InstanceIdConflict {
            stored: existing.instance_id,
            current: current_instance_id.to_owned(),
            project_root: current_root.to_owned(),
        });
    }

    let repaired = ProjectIdentityRecord {
        project_root: current_root.to_owned(),
        ..existing
    };
    identity_port.save_project_identity(&repaired)?;
    Ok(repaired)
}

/// Prepare workspace identity metadata without persisting a new binding.
///
/// A new binding becomes durable only after runtime creation succeeds. Existing
/// bindings are authoritative: normal opens must not rewrite them from naming
/// policy outputs.
pub fn prepare_workspace_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    policy: &SessionNamingPolicy,
    ctx: &NamingContext,
    backend: RuntimeBackendId,
    relative_project_path: Option<RelativeProjectPath>,
    relative_display_path: Option<&str>,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let ws_key = WorkspaceIdentityKey::new(&ctx.workspace, relative_project_path.clone());
    if let Some(existing) = identity_port.load_workspace_identity(&ws_key)? {
        return Ok(existing);
    }

    let scope_id = policy
        .resolve_server(ctx)
        .map_err(|e| IdentityError::Corrupted {
            path: Utf8PathBuf::from("<naming-policy>"),
            reason: e.to_string(),
        })?;
    let workspace_id = policy
        .resolve_session(ctx)
        .map_err(|e| IdentityError::Corrupted {
            path: Utf8PathBuf::from("<naming-policy>"),
            reason: e.to_string(),
        })?;

    Ok(WorkspaceIdentityRecord {
        workspace: ctx.workspace.clone(),
        relative_project_path,
        relative_display_path: relative_display_path.map(str::to_owned),
        canonical_workspace_component: SessionNamingPolicy::sanitize_component(&ctx.workspace),
        runtime_binding: RuntimeBinding::new(backend, scope_id.clone(), workspace_id.clone())
            .expect("runtime binding identifiers must not be empty"),
        runtime_replacement: None,
        canonical_session_name: workspace_id,
        canonical_server_name: scope_id,
    })
}

/// Persist a runtime binding after the runtime has been created and validated.
pub fn commit_workspace_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    identity_port.save_workspace_identity(record)
}

/// Replace every project runtime through one recoverable transaction.
pub fn replace_project_runtime_bindings<E, Prepare, CommitConfig, RollbackConfig, Cleanup>(
    identity_port: &dyn WorkspaceIdentityPort,
    records: &[WorkspaceIdentityRecord],
    mut prepare_target: Prepare,
    commit_config: CommitConfig,
    rollback_config: RollbackConfig,
    mut cleanup_runtime: Cleanup,
) -> Result<Vec<WorkspaceIdentityRecord>, E>
where
    E: From<IdentityError>,
    Prepare: FnMut(&WorkspaceIdentityRecord) -> Result<RuntimeBinding, E>,
    CommitConfig: FnOnce() -> Result<(), E>,
    RollbackConfig: FnOnce() -> Result<(), E>,
    Cleanup: FnMut(&RuntimeBinding) -> Result<(), E>,
{
    let mut targets = Vec::with_capacity(records.len());
    for record in records {
        match prepare_target(record) {
            Ok(binding) => {
                if binding != record.runtime_binding {
                    targets.push((record.identity_key(), binding));
                }
            }
            Err(error) => {
                for (_, binding) in &targets {
                    let _ = cleanup_runtime(binding);
                }
                return Err(error);
            }
        }
    }

    let prepared = match prepare_project_runtime_replacements(identity_port, records, &targets) {
        Ok(prepared) => prepared,
        Err(error) => {
            for (_, binding) in &targets {
                let _ = cleanup_runtime(binding);
            }
            return Err(error.into());
        }
    };
    if let Err(error) = commit_config() {
        rollback_project_runtime_replacements(identity_port, &prepared)?;
        for (_, binding) in &targets {
            cleanup_runtime(binding)?;
        }
        return Err(error);
    }
    let committed = match commit_project_runtime_replacements(identity_port, &prepared) {
        Ok(committed) => committed,
        Err(error) => {
            rollback_config()?;
            rollback_project_runtime_replacements(identity_port, &prepared)?;
            for (_, binding) in &targets {
                cleanup_runtime(binding)?;
            }
            return Err(error.into());
        }
    };

    for record in &committed {
        if let Some(RuntimeReplacement::Cleaning { previous }) = &record.runtime_replacement {
            cleanup_runtime(previous)?;
        }
    }
    finish_project_runtime_replacements(identity_port, &committed).map_err(Into::into)
}

/// Stage every workspace target while keeping all active bindings unchanged.
pub fn prepare_project_runtime_replacements(
    identity_port: &dyn WorkspaceIdentityPort,
    records: &[WorkspaceIdentityRecord],
    targets: &[(WorkspaceIdentityKey, RuntimeBinding)],
) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
    if records
        .iter()
        .any(|record| record.runtime_replacement.is_some())
        || targets.iter().enumerate().any(|(index, (key, _))| {
            !records.iter().any(|record| record.identity_key() == *key)
                || targets[..index]
                    .iter()
                    .any(|(previous_key, _)| previous_key == key)
        })
    {
        return Err(IdentityError::Corrupted {
            path: Utf8PathBuf::from("<project-runtime-replacement>"),
            reason: "workspace replacement set is invalid or already pending".to_owned(),
        });
    }
    let prepared = records
        .iter()
        .cloned()
        .map(|mut record| {
            if let Some(target) = targets.iter().find_map(|(candidate, binding)| {
                (candidate == &record.identity_key()).then(|| binding.clone())
            }) {
                record.runtime_replacement = Some(RuntimeReplacement::Preparing { target });
            }
            record
        })
        .collect::<Vec<_>>();
    identity_port.replace_workspace_identities(&prepared)?;
    Ok(prepared)
}

/// Atomically make all prepared project targets authoritative.
pub fn commit_project_runtime_replacements(
    identity_port: &dyn WorkspaceIdentityPort,
    prepared: &[WorkspaceIdentityRecord],
) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
    let committed = prepared
        .iter()
        .map(|record| {
            let Some(RuntimeReplacement::Preparing { target }) = record.runtime_replacement.clone()
            else {
                if record.runtime_replacement.is_none() {
                    return Ok(record.clone());
                }
                return Err(IdentityError::Corrupted {
                    path: Utf8PathBuf::from("<project-runtime-replacement>"),
                    reason: "workspace replacement is not prepared".to_owned(),
                });
            };
            let mut record = record.clone();
            let previous = activate_binding(&mut record, target);
            record.runtime_replacement = Some(RuntimeReplacement::Cleaning { previous });
            Ok(record)
        })
        .collect::<Result<Vec<_>, _>>()?;
    identity_port.replace_workspace_identities(&committed)?;
    Ok(committed)
}

/// Atomically clear replacement residue after every old runtime is gone.
pub fn finish_project_runtime_replacements(
    identity_port: &dyn WorkspaceIdentityPort,
    committed: &[WorkspaceIdentityRecord],
) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
    if committed.iter().any(|record| {
        !matches!(
            record.runtime_replacement,
            None | Some(RuntimeReplacement::Cleaning { .. })
        )
    }) {
        return Err(IdentityError::Corrupted {
            path: Utf8PathBuf::from("<project-runtime-replacement>"),
            reason: "workspace replacement cleanup is incomplete".to_owned(),
        });
    }
    let finished = committed
        .iter()
        .cloned()
        .map(|mut record| {
            record.runtime_replacement = None;
            record
        })
        .collect::<Vec<_>>();
    identity_port.replace_workspace_identities(&finished)?;
    Ok(finished)
}

/// Restore the original active bindings from either replacement phase.
pub fn rollback_project_runtime_replacements(
    identity_port: &dyn WorkspaceIdentityPort,
    records: &[WorkspaceIdentityRecord],
) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
    let restored = records
        .iter()
        .map(|record| {
            let mut record = record.clone();
            if let Some(previous) = record
                .runtime_replacement
                .take()
                .and_then(RuntimeReplacement::into_previous)
            {
                activate_binding(&mut record, previous);
            }
            record.runtime_replacement = None;
            record
        })
        .collect::<Vec<_>>();
    identity_port.replace_workspace_identities(&restored)?;
    Ok(restored)
}

/// Persist a staged target while leaving the active binding authoritative.
pub fn prepare_runtime_replacement(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
    target: RuntimeBinding,
) -> Result<(), IdentityError> {
    record.runtime_replacement = Some(RuntimeReplacement::Preparing { target });
    identity_port.save_workspace_identity(record)
}

/// Activate `binding` and keep the persisted canonical names in step with it,
/// returning the binding it replaced.
///
/// These three writes belong together: a phase that switches the binding but
/// forgets either canonical name leaves the record internally inconsistent.
/// Every phase below routes through here so a future phase cannot forget one.
fn activate_binding(
    record: &mut WorkspaceIdentityRecord,
    binding: RuntimeBinding,
) -> RuntimeBinding {
    let previous = std::mem::replace(&mut record.runtime_binding, binding);
    record.canonical_server_name = record.runtime_binding.scope_id.clone();
    record.canonical_session_name = record.runtime_binding.workspace_id.clone();
    previous
}

/// Atomically switch the active binding and retain the old binding for cleanup.
pub fn commit_runtime_replacement(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    let Some(RuntimeReplacement::Preparing { target }) = record.runtime_replacement.clone() else {
        return Ok(());
    };
    let previous = activate_binding(record, target);
    record.runtime_replacement =
        (previous != record.runtime_binding).then_some(RuntimeReplacement::Cleaning { previous });
    identity_port.save_workspace_identity(record)
}

/// Commit a validated target whose checkout association can only complete
/// after the previous runtime releases that checkout.
pub fn commit_runtime_checkout_registration(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    let Some(RuntimeReplacement::Preparing { target }) = record.runtime_replacement.clone() else {
        return Ok(());
    };
    let previous = activate_binding(record, target);
    record.runtime_replacement = Some(RuntimeReplacement::RegisteringCheckout { previous });
    identity_port.save_workspace_identity(record)
}

/// Clear a completed checkout-registration promotion.
pub fn finish_runtime_checkout_registration(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    if !matches!(
        record.runtime_replacement,
        Some(RuntimeReplacement::RegisteringCheckout { .. })
    ) {
        return Ok(());
    }
    record.runtime_replacement = None;
    identity_port.save_workspace_identity(record)
}

/// Commit a validated tmux replacement while keeping its staged runtime
/// authoritative until the backend rename succeeds.
pub fn commit_runtime_promotion(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
    final_binding: RuntimeBinding,
) -> Result<(), IdentityError> {
    let Some(RuntimeReplacement::Preparing { target: staged }) = record.runtime_replacement.clone()
    else {
        return Ok(());
    };
    let previous = activate_binding(record, staged.clone());
    record.runtime_replacement = Some(RuntimeReplacement::Promoting {
        staged,
        final_binding,
        previous,
    });
    identity_port.save_workspace_identity(record)
}

/// Make the promoted runtime's final backend identity authoritative.
pub fn finish_runtime_promotion(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    let Some(RuntimeReplacement::Promoting {
        final_binding,
        previous,
        ..
    }) = record.runtime_replacement.clone()
    else {
        return Ok(());
    };
    // `previous` must stay the binding carried by `Promoting`, NOT the value
    // `activate_binding` replaces here. Those differ: the binding being
    // replaced is the staged one (already renamed to `final_binding`, so
    // there is nothing left to retire), while cleanup still has to target the
    // original runtime recorded before the promotion began. Discarding the
    // return value is deliberate.
    activate_binding(record, final_binding);
    record.runtime_replacement = Some(RuntimeReplacement::Cleaning { previous });
    identity_port.save_workspace_identity(record)
}
/// Restore a workspace's original binding and clear its in-flight replacement.
pub fn rollback_runtime_replacement(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    if let Some(previous) = record
        .runtime_replacement
        .take()
        .and_then(RuntimeReplacement::into_previous)
    {
        activate_binding(record, previous);
    }
    record.runtime_replacement = None;
    identity_port.save_workspace_identity(record)
}

/// Clear replacement state after the staged target or old runtime is removed.
pub fn finish_runtime_replacement(
    identity_port: &dyn WorkspaceIdentityPort,
    record: &mut WorkspaceIdentityRecord,
) -> Result<(), IdentityError> {
    record.runtime_replacement = None;
    identity_port.save_workspace_identity(record)
}

/// Ensure workspace identity metadata exists and is consistent.
///
/// This compatibility helper is retained for callers that already own a live
/// runtime. New workspace creation must call [`prepare_workspace_identity`]
/// before runtime creation and [`commit_workspace_identity`] afterwards.
pub fn ensure_workspace_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    policy: &SessionNamingPolicy,
    ctx: &NamingContext,
    relative_project_path: Option<RelativeProjectPath>,
    relative_display_path: Option<&str>,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let record = prepare_workspace_identity(
        identity_port,
        policy,
        ctx,
        RuntimeBackendId::tmux(),
        relative_project_path,
        relative_display_path,
    )?;
    commit_workspace_identity(identity_port, &record)?;
    Ok(record)
}

/// Resolve a workspace identity record by matching its persisted runtime
/// scope and workspace identifiers. The backend is intentionally not
/// hard-coded: the caller resolves the selected backend separately.
pub fn resolve_workspace_identity_by_runtime(
    identity_port: &dyn WorkspaceIdentityPort,
    actual_backend: Option<&RuntimeBackendId>,
    actual_scope: &str,
    actual_workspace: &str,
    _project_identity: &ProjectIdentityRecord,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let all_records = identity_port.list_workspace_identities()?;
    let matches: Vec<_> = all_records
        .into_iter()
        .filter(|record| {
            let binding = record.runtime_binding();
            actual_backend.is_none_or(|backend| binding.backend == *backend)
                && binding.scope_id == actual_scope
                && binding.workspace_id == actual_workspace
        })
        .collect();

    resolve_unique_identity(matches, "", actual_workspace, actual_scope)
}

/// Resolve a workspace identity record by matching logical workspace and its
/// persisted runtime binding. Runtime identifiers remain opaque and are never
/// parsed.
pub fn resolve_workspace_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    workspace_id: &str,
    scope_id: &str,
    runtime_workspace_id: &str,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let matches: Vec<_> = identity_port
        .list_workspace_identities()?
        .into_iter()
        .filter(|record| {
            let binding = record.runtime_binding();
            record.workspace == workspace_id
                && binding.scope_id == scope_id
                && binding.workspace_id == runtime_workspace_id
        })
        .collect();

    resolve_unique_identity(matches, workspace_id, runtime_workspace_id, scope_id)
}

fn resolve_unique_identity(
    matches: Vec<WorkspaceIdentityRecord>,
    workspace: &str,
    runtime_workspace_id: &str,
    scope_id: &str,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let mut iter = matches.into_iter();
    match (iter.next(), iter.next()) {
        (None, _) => Err(IdentityError::IdentityNotFound {
            workspace: workspace.to_owned(),
            session: runtime_workspace_id.to_owned(),
            server: scope_id.to_owned(),
        }),
        (Some(record), None) => Ok(record),
        (Some(first), Some(_)) => Err(IdentityError::AmbiguousIdentity {
            workspace: first.workspace,
            session: runtime_workspace_id.to_owned(),
            server: scope_id.to_owned(),
            count: 2 + iter.count(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{NamingConfig, RelativeProjectPath};
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// In-memory fake for `WorkspaceIdentityPort`.
    struct FakeIdentityPort {
        project: RefCell<Option<ProjectIdentityRecord>>,
        workspaces: RefCell<HashMap<WorkspaceIdentityKey, WorkspaceIdentityRecord>>,
    }

    impl FakeIdentityPort {
        fn new() -> Self {
            Self {
                project: RefCell::new(None),
                workspaces: RefCell::new(HashMap::new()),
            }
        }
    }

    impl WorkspaceIdentityPort for FakeIdentityPort {
        fn load_project_identity(&self) -> Result<Option<ProjectIdentityRecord>, IdentityError> {
            Ok(self.project.borrow().clone())
        }

        fn save_project_identity(
            &self,
            record: &ProjectIdentityRecord,
        ) -> Result<(), IdentityError> {
            *self.project.borrow_mut() = Some(record.clone());
            Ok(())
        }

        fn load_workspace_identity(
            &self,
            key: &WorkspaceIdentityKey,
        ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError> {
            Ok(self.workspaces.borrow().get(key).cloned())
        }

        fn save_workspace_identity(
            &self,
            record: &WorkspaceIdentityRecord,
        ) -> Result<(), IdentityError> {
            self.workspaces
                .borrow_mut()
                .insert(record.identity_key(), record.clone());
            Ok(())
        }

        fn replace_workspace_identities(
            &self,
            records: &[WorkspaceIdentityRecord],
        ) -> Result<(), IdentityError> {
            *self.workspaces.borrow_mut() = records
                .iter()
                .cloned()
                .map(|record| (record.identity_key(), record))
                .collect();
            Ok(())
        }

        fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
            Ok(self.workspaces.borrow().values().cloned().collect())
        }
    }

    fn default_naming_ctx() -> NamingContext {
        NamingContext {
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/my-project".to_owned()),
            instance_id: Some("a1b2".to_owned()),
            relative_project_path: None,
        }
    }

    // ── ensure_project_identity ──

    #[test]
    fn creates_project_identity_when_missing() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        let result =
            ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();
        assert_eq!(result.instance_id, "a1b2");
        assert!(port.load_project_identity().unwrap().is_some());
    }

    #[test]
    fn returns_existing_project_identity_when_matching() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();
        let result =
            ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();
        assert_eq!(result.instance_id, "a1b2");
    }

    #[test]
    fn repairs_moved_project_root_without_touching_workspace_runtime_binding() {
        let port = FakeIdentityPort::new();
        let old_root = Utf8PathBuf::from("/Users/dev/my-project");
        let new_root = Utf8PathBuf::from("/Users/dev/renamed-project");
        ensure_project_identity(&port, "a1b2", &old_root, "my-project", "my-project").unwrap();

        let workspace = root_record();
        port.save_workspace_identity(&workspace).unwrap();

        let repaired = repair_project_identity_root(&port, "a1b2", &new_root).unwrap();
        assert_eq!(repaired.project_root, new_root);
        assert_eq!(repaired.instance_id, "a1b2");
        assert_eq!(
            port.load_workspace_identity(&workspace.identity_key())
                .unwrap()
                .unwrap()
                .runtime_binding,
            workspace.runtime_binding
        );
    }

    #[test]
    fn refuses_project_root_repair_when_instance_id_changed() {
        let port = FakeIdentityPort::new();
        let old_root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &old_root, "my-project", "my-project").unwrap();

        let err = repair_project_identity_root(
            &port,
            "cccc",
            Utf8Path::new("/Users/dev/renamed-project"),
        )
        .unwrap_err();
        assert!(matches!(err, IdentityError::InstanceIdConflict { .. }));
    }

    #[test]
    fn rejects_conflicting_instance_id() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let err =
            ensure_project_identity(&port, "cccc", &root, "my-project", "my-project").unwrap_err();
        assert!(matches!(err, IdentityError::InstanceIdConflict { .. }));
    }

    #[test]
    fn rejects_conflicting_project_root() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let other_root = Utf8PathBuf::from("/other/path");
        let err = ensure_project_identity(&port, "a1b2", &other_root, "my-project", "my-project")
            .unwrap_err();
        assert!(matches!(err, IdentityError::ProjectRootConflict { .. }));
    }

    #[test]
    fn equivalent_symlink_paths_do_not_conflict() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();

        let link_path = dir.path().join("link");
        symlink(&real_dir, &link_path).unwrap();

        let real = Utf8PathBuf::try_from(real_dir.canonicalize().unwrap()).unwrap();

        let port = FakeIdentityPort::new();
        ensure_project_identity(&port, "a1b2", &real, "my-project", "my-project").unwrap();

        // Access via the symlink path — should NOT conflict because
        // canonicalization resolves both to the same real path.
        let via_link = Utf8PathBuf::try_from(link_path).unwrap();
        let result = ensure_project_identity(&port, "a1b2", &via_link, "my-project", "my-project");
        assert!(
            result.is_ok(),
            "symlink-equivalent paths must not trigger ProjectRootConflict, got: {result:?}"
        );
    }

    #[test]
    fn rejects_conflicting_project_display() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let err =
            ensure_project_identity(&port, "a1b2", &root, "other-name", "my-project").unwrap_err();
        assert!(matches!(err, IdentityError::ProjectDisplayConflict { .. }));
    }

    #[test]
    fn rejects_canonical_project_component_with_subdir_suffix() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project/subdir").unwrap();

        let err =
            ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap_err();
        assert!(matches!(
            err,
            IdentityError::CanonicalProjectConflict { .. }
        ));
    }

    #[test]
    fn rejects_conflicting_canonical_project_component() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "other-project").unwrap();

        let err =
            ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap_err();
        assert!(matches!(
            err,
            IdentityError::CanonicalProjectConflict { .. }
        ));
    }

    // ── ensure_workspace_identity ──

    #[test]
    fn creates_workspace_identity_when_missing() {
        let port = FakeIdentityPort::new();
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();

        let result = ensure_workspace_identity(&port, &policy, &ctx, None, None).unwrap();
        assert_eq!(result.workspace, "main");
        assert!(result.relative_project_path.is_none());
        assert_eq!(
            result.runtime_binding().workspace_id,
            "wh@a1b2@my-project@main"
        );
        assert!(result.relative_display_path.is_none());
    }

    #[test]
    fn prepares_new_workspace_for_selected_backend() {
        let port = FakeIdentityPort::new();
        let record = prepare_workspace_identity(
            &port,
            &SessionNamingPolicy::default_policy(),
            &default_naming_ctx(),
            RuntimeBackendId::herdr(),
            None,
            None,
        )
        .unwrap();

        assert_eq!(record.runtime_binding().backend, RuntimeBackendId::herdr());
        assert!(
            port.load_workspace_identity(&WorkspaceIdentityKey::new("main", None))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn workspace_identity_is_idempotent() {
        let port = FakeIdentityPort::new();
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();

        let r1 = ensure_workspace_identity(&port, &policy, &ctx, None, None).unwrap();
        let r2 = ensure_workspace_identity(&port, &policy, &ctx, None, None).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn workspace_identity_updates_when_naming_changes() {
        let port = FakeIdentityPort::new();

        let policy1 = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();
        let r1 = ensure_workspace_identity(&port, &policy1, &ctx, None, None).unwrap();

        let policy2 = SessionNamingPolicy::new(NamingConfig {
            session_template: "wh2@{instance_id}@{project}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let r2 = ensure_workspace_identity(&port, &policy2, &ctx, None, None).unwrap();
        assert_eq!(
            r1.runtime_binding(),
            r2.runtime_binding(),
            "a persisted binding is stable across naming-policy changes"
        );
        assert_eq!(r2.workspace, "main");
    }

    // ── Sanitize-required subdir ──

    #[test]
    fn workspace_identity_preserves_display_for_sanitized_name() {
        let port = FakeIdentityPort::new();
        let policy = SessionNamingPolicy::default_policy();
        let ctx = NamingContext {
            project: "python/1_0pingtai".to_owned(),
            workspace: "main".to_owned(),
            branch: None,
            dir_path: Some("/dev/python/1.0pingtai".to_owned()),
            instance_id: Some("c3d4".to_owned()),
            relative_project_path: None,
        };
        let result = ensure_workspace_identity(
            &port,
            &policy,
            &ctx,
            Some(RelativeProjectPath::new("1_0pingtai").unwrap()),
            Some("python/1.0pingtai"),
        )
        .unwrap();

        assert_eq!(
            result.relative_display_path.as_deref(),
            Some("python/1.0pingtai")
        );
        assert_eq!(
            result.relative_project_path.as_ref().unwrap().as_str(),
            "1_0pingtai"
        );
        assert!(result.runtime_binding().workspace_id.contains("1_0pingtai"));
    }

    // ── resolve_workspace_identity ──

    fn root_record() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            runtime_binding: RuntimeBinding::tmux("whsrv-a1b2-proj", "wh@a1b2@proj@main"),
            runtime_replacement: None,
            canonical_session_name: "wh@a1b2@proj@main".to_owned(),
            canonical_server_name: "whsrv-a1b2-proj".to_owned(),
        }
    }

    fn scoped_record() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "feat-x".to_owned(),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
            relative_display_path: Some("proj/services/api/feat-x".to_owned()),
            canonical_workspace_component: "feat-x".to_owned(),
            runtime_binding: RuntimeBinding::tmux("whsrv-a1b2-proj", "wh@a1b2@proj@feat-x"),
            runtime_replacement: None,
            canonical_session_name: "wh@a1b2@proj@feat-x".to_owned(),
            canonical_server_name: "whsrv-a1b2-proj".to_owned(),
        }
    }

    fn replacement_error(reason: &str) -> IdentityError {
        IdentityError::Corrupted {
            path: Utf8PathBuf::from("<test-replacement>"),
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn project_runtime_transaction_commits_config_and_cleans_both_runtime_sets() {
        let port = FakeIdentityPort::new();
        let original = vec![root_record(), scoped_record()];
        port.replace_workspace_identities(&original).unwrap();
        let config_committed = std::cell::Cell::new(false);
        let cleaned = std::cell::RefCell::new(Vec::new());

        let finished = replace_project_runtime_bindings(
            &port,
            &original,
            |record| {
                RuntimeBinding::new(
                    RuntimeBackendId::herdr(),
                    "opaque-scope",
                    format!("opaque-{}", record.workspace),
                )
                .ok_or_else(|| replacement_error("invalid target binding"))
            },
            || {
                config_committed.set(true);
                Ok(())
            },
            || Ok(()),
            |binding| {
                cleaned.borrow_mut().push(binding.clone());
                Ok(())
            },
        )
        .unwrap();

        assert!(config_committed.get());
        assert!(finished.iter().all(|record| {
            record.runtime_binding.backend == RuntimeBackendId::herdr()
                && record.runtime_replacement.is_none()
        }));
        assert_eq!(cleaned.borrow().len(), original.len());
    }

    #[test]
    fn mixed_project_runtime_transaction_only_replaces_old_backend() {
        let port = FakeIdentityPort::new();
        let mut original = vec![root_record(), scoped_record()];
        original[1].runtime_binding =
            RuntimeBinding::new(RuntimeBackendId::herdr(), "scope", "already-herdr").unwrap();
        port.replace_workspace_identities(&original).unwrap();
        let cleaned = std::cell::RefCell::new(Vec::new());

        let finished = replace_project_runtime_bindings(
            &port,
            &original,
            |record| {
                if record.runtime_binding.backend == RuntimeBackendId::herdr() {
                    Ok(record.runtime_binding())
                } else {
                    RuntimeBinding::new(
                        RuntimeBackendId::herdr(),
                        "scope",
                        format!("opaque-{}", record.workspace),
                    )
                    .ok_or_else(|| replacement_error("invalid target binding"))
                }
            },
            || Ok(()),
            || Ok(()),
            |binding| {
                cleaned.borrow_mut().push(binding.clone());
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(finished[1], original[1]);
        assert_eq!(
            finished[0].runtime_binding.backend,
            RuntimeBackendId::herdr()
        );
        assert_eq!(cleaned.into_inner(), vec![original[0].runtime_binding()]);
    }

    #[test]
    fn project_runtime_transaction_config_failure_restores_old_bindings_and_cleans_targets() {
        let port = FakeIdentityPort::new();
        let original = vec![root_record(), scoped_record()];
        port.replace_workspace_identities(&original).unwrap();
        let cleaned = std::cell::RefCell::new(Vec::new());

        let result = replace_project_runtime_bindings(
            &port,
            &original,
            |record| {
                RuntimeBinding::new(
                    RuntimeBackendId::herdr(),
                    "opaque-scope",
                    format!("opaque-{}", record.workspace),
                )
                .ok_or_else(|| replacement_error("invalid target binding"))
            },
            || Err(replacement_error("config write failed")),
            || Ok(()),
            |binding| {
                cleaned.borrow_mut().push(binding.clone());
                Ok(())
            },
        );

        assert!(result.is_err());
        let mut durable = port.list_workspace_identities().unwrap();
        durable.sort_by(|left, right| left.workspace.cmp(&right.workspace));
        let mut expected = original;
        expected.sort_by(|left, right| left.workspace.cmp(&right.workspace));
        assert_eq!(durable, expected);
        assert_eq!(cleaned.borrow().len(), 2);
    }

    #[test]
    fn project_runtime_transaction_cleanup_failure_keeps_committed_bindings_recoverable() {
        let port = FakeIdentityPort::new();
        let original = vec![root_record(), scoped_record()];
        port.replace_workspace_identities(&original).unwrap();

        let result = replace_project_runtime_bindings(
            &port,
            &original,
            |record| {
                RuntimeBinding::new(
                    RuntimeBackendId::herdr(),
                    "opaque-scope",
                    format!("opaque-{}", record.workspace),
                )
                .ok_or_else(|| replacement_error("invalid target binding"))
            },
            || Ok(()),
            || Ok(()),
            |_| Err(replacement_error("old runtime cleanup failed")),
        );

        assert!(result.is_err());
        let durable = port.list_workspace_identities().unwrap();
        assert!(durable.iter().all(|record| {
            record.runtime_binding.backend == RuntimeBackendId::herdr()
                && matches!(
                    record.runtime_replacement,
                    Some(RuntimeReplacement::Cleaning { .. })
                )
        }));
    }

    #[test]
    fn project_runtime_replacement_switches_all_bindings_or_rolls_all_back() {
        let port = FakeIdentityPort::new();
        let original = vec![root_record(), scoped_record()];
        port.replace_workspace_identities(&original).unwrap();
        let targets = original
            .iter()
            .enumerate()
            .map(|(index, record)| {
                (
                    record.identity_key(),
                    RuntimeBinding::new(
                        RuntimeBackendId::herdr(),
                        "opaque-scope",
                        format!("opaque-w{index}"),
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();

        let prepared = prepare_project_runtime_replacements(&port, &original, &targets).unwrap();
        assert!(prepared.iter().all(|record| {
            record.runtime_binding().backend == RuntimeBackendId::tmux()
                && matches!(
                    record.runtime_replacement,
                    Some(RuntimeReplacement::Preparing { .. })
                )
        }));

        let committed = commit_project_runtime_replacements(&port, &prepared).unwrap();
        assert!(committed.iter().all(|record| {
            record.runtime_binding().backend == RuntimeBackendId::herdr()
                && matches!(
                    record.runtime_replacement,
                    Some(RuntimeReplacement::Cleaning { .. })
                )
        }));

        let finished = finish_project_runtime_replacements(&port, &committed).unwrap();
        assert!(finished.iter().all(|record| {
            record.runtime_binding().backend == RuntimeBackendId::herdr()
                && record.runtime_replacement.is_none()
        }));

        let restored = rollback_project_runtime_replacements(&port, &committed).unwrap();
        assert_eq!(restored, original);
        assert_eq!(port.list_workspace_identities().unwrap().len(), 2);
    }

    #[test]
    fn partial_project_replacement_preserves_unchanged_binding() {
        let port = FakeIdentityPort::new();
        let mut original = vec![root_record(), scoped_record()];
        original[1].runtime_binding =
            RuntimeBinding::new(RuntimeBackendId::herdr(), "scope", "already-herdr").unwrap();
        let targets = vec![(
            original[0].identity_key(),
            RuntimeBinding::new(RuntimeBackendId::herdr(), "scope", "opaque-w0").unwrap(),
        )];

        let prepared = prepare_project_runtime_replacements(&port, &original, &targets).unwrap();
        assert!(matches!(
            prepared[0].runtime_replacement,
            Some(RuntimeReplacement::Preparing { .. })
        ));
        assert!(prepared[1].runtime_replacement.is_none());

        let committed = commit_project_runtime_replacements(&port, &prepared).unwrap();
        assert_eq!(committed[1], original[1]);
        let finished = finish_project_runtime_replacements(&port, &committed).unwrap();
        assert_eq!(finished[1], original[1]);
        assert_eq!(
            finished[0].runtime_binding.backend,
            RuntimeBackendId::herdr()
        );
    }

    #[test]
    fn invalid_project_cleanup_keeps_durable_replacement_residue() {
        let port = FakeIdentityPort::new();
        let original = vec![root_record(), scoped_record()];
        port.replace_workspace_identities(&original).unwrap();
        let mut incomplete = original.clone();
        incomplete[0].runtime_replacement = Some(RuntimeReplacement::Preparing {
            target: RuntimeBinding::tmux("scope", "target"),
        });

        assert!(finish_project_runtime_replacements(&port, &incomplete).is_err());
        let durable = port.list_workspace_identities().unwrap();
        assert!(
            durable
                .iter()
                .all(|record| record.runtime_replacement.is_none())
        );
    }

    /// Characterization guard for the `activate_binding` refactor: every phase
    /// that activates a binding must leave both persisted canonical names in
    /// step with it. The pre-existing tests only pin
    /// `canonical_session_name` at two of the four transitions, so the sync
    /// invariant is otherwise unprotected.
    ///
    /// Observed through the module's public seam (the usecase functions +
    /// `WorkspaceIdentityPort`), so it survives the refactor.
    #[test]
    fn every_phase_keeps_canonical_names_in_step_with_the_active_binding() {
        fn assert_in_step(record: &WorkspaceIdentityRecord, phase: &str) {
            let active = record.runtime_binding();
            assert_eq!(
                record.canonical_server_name, active.scope_id,
                "{phase}: canonical_server_name must follow the active binding"
            );
            assert_eq!(
                record.canonical_session_name, active.workspace_id,
                "{phase}: canonical_session_name must follow the active binding"
            );
        }

        /// Every phase that records a runtime for cleanup must name the binding
        /// that existed *before* the replacement began. Two bindings are in
        /// play during a promotion (the staged one being swapped in, and the
        /// original one still running), and recording the staged binding there
        /// would leave the original runtime never retired.
        fn assert_cleanup_targets_original(
            record: &WorkspaceIdentityRecord,
            original: &RuntimeBinding,
            phase: &str,
        ) {
            let recorded = match &record.runtime_replacement {
                Some(
                    RuntimeReplacement::Cleaning { previous }
                    | RuntimeReplacement::RegisteringCheckout { previous }
                    | RuntimeReplacement::Promoting { previous, .. },
                ) => Some(previous),
                Some(RuntimeReplacement::Preparing { .. }) | None => None,
            };
            if let Some(recorded) = recorded {
                assert_eq!(
                    recorded, original,
                    "{phase}: cleanup must target the original binding, not a staged one"
                );
            }
        }

        let port = FakeIdentityPort::new();
        let staged = RuntimeBinding::tmux("whsrv-a1b2-proj", "staged-target");
        let final_binding = RuntimeBinding::tmux("whsrv-a1b2-proj", "canonical-target");

        // Phase 1-2: the tmux promotion path (Preparing -> Promoting -> promoted).
        let mut tmux_record = root_record();
        let tmux_original = tmux_record.runtime_binding();
        assert_in_step(&tmux_record, "initial");
        prepare_runtime_replacement(&port, &mut tmux_record, staged.clone()).unwrap();
        assert_in_step(&tmux_record, "prepared");
        commit_runtime_promotion(&port, &mut tmux_record, final_binding.clone()).unwrap();
        assert_in_step(&tmux_record, "promoting");
        assert_cleanup_targets_original(&tmux_record, &tmux_original, "promoting");
        finish_runtime_promotion(&port, &mut tmux_record).unwrap();
        assert_in_step(&tmux_record, "promoted");
        assert_cleanup_targets_original(&tmux_record, &tmux_original, "promoted");

        // Phase 3-4: the checkout-registration path (Preparing -> RegisteringCheckout).
        let mut herdr_record = root_record();
        let herdr_original = herdr_record.runtime_binding();
        let target = RuntimeBinding::new(RuntimeBackendId::herdr(), "scope", "opaque-w1").unwrap();
        prepare_runtime_replacement(&port, &mut herdr_record, target).unwrap();
        commit_runtime_checkout_registration(&port, &mut herdr_record).unwrap();
        assert_in_step(&herdr_record, "registering-checkout");
        assert_cleanup_targets_original(&herdr_record, &herdr_original, "registering-checkout");
        finish_runtime_checkout_registration(&port, &mut herdr_record).unwrap();
        assert_in_step(&herdr_record, "checkout-registered");

        // Phase 5: the plain replacement path (Preparing -> Cleaning).
        let mut replacement_record = root_record();
        let replacement_original = replacement_record.runtime_binding();
        prepare_runtime_replacement(&port, &mut replacement_record, staged).unwrap();
        commit_runtime_replacement(&port, &mut replacement_record).unwrap();
        assert_in_step(&replacement_record, "committed-replacement");
        assert_cleanup_targets_original(&replacement_record, &replacement_original, "committed");
        finish_runtime_replacement(&port, &mut replacement_record).unwrap();
        assert_in_step(&replacement_record, "replacement-finished");
    }

    #[test]
    fn runtime_replacement_keeps_old_binding_until_commit() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let old = record.runtime_binding();
        let target = RuntimeBinding::tmux("whsrv-a1b2-proj", "staged-target");

        prepare_runtime_replacement(&port, &mut record, target.clone()).unwrap();

        assert_eq!(record.runtime_binding(), old);
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::Preparing {
                target: target.clone()
            })
        );

        commit_runtime_replacement(&port, &mut record).unwrap();

        assert_eq!(record.runtime_binding(), target);
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::Cleaning { previous: old })
        );
        assert_eq!(record.canonical_session_name, "staged-target");
    }

    #[test]
    fn runtime_promotion_keeps_staged_binding_recoverable_until_rename() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let old = record.runtime_binding();
        let staged = RuntimeBinding::tmux("whsrv-a1b2-proj", "staged-target");
        let final_binding = RuntimeBinding::tmux("whsrv-a1b2-proj", "canonical-target");

        prepare_runtime_replacement(&port, &mut record, staged.clone()).unwrap();
        commit_runtime_promotion(&port, &mut record, final_binding.clone()).unwrap();

        assert_eq!(record.runtime_binding(), staged);
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::Promoting {
                staged: staged.clone(),
                final_binding: final_binding.clone(),
                previous: old.clone(),
            })
        );

        finish_runtime_promotion(&port, &mut record).unwrap();
        assert_eq!(record.runtime_binding(), final_binding);
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::Cleaning { previous: old })
        );
    }

    #[test]
    fn rollback_restores_binding_from_promoting_phase() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let old = record.runtime_binding();
        let staged = RuntimeBinding::tmux("whsrv-a1b2-proj", "staged-target");
        let final_binding = RuntimeBinding::tmux("whsrv-a1b2-proj", "canonical-target");

        prepare_runtime_replacement(&port, &mut record, staged).unwrap();
        commit_runtime_promotion(&port, &mut record, final_binding).unwrap();

        let restored = rollback_project_runtime_replacements(&port, &[record]).unwrap();
        assert_eq!(restored[0].runtime_binding(), old);
        assert!(restored[0].runtime_replacement.is_none());
    }

    #[test]
    fn checkout_registration_has_an_explicit_recoverable_phase() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let old = record.runtime_binding();
        let target =
            RuntimeBinding::new(crate::domain::RuntimeBackendId::herdr(), "scope", "target")
                .unwrap();

        prepare_runtime_replacement(&port, &mut record, target.clone()).unwrap();
        commit_runtime_checkout_registration(&port, &mut record).unwrap();

        assert_eq!(record.runtime_binding(), target);
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::RegisteringCheckout { previous: old })
        );

        finish_runtime_checkout_registration(&port, &mut record).unwrap();
        assert!(record.runtime_replacement.is_none());
    }

    #[test]
    fn runtime_replacement_commits_server_assigned_opaque_workspace_id() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let old = record.runtime_binding();
        let target = RuntimeBinding::new(
            RuntimeBackendId::herdr(),
            "/home/user/.config/herdr/herdr.sock",
            "w9",
        )
        .unwrap();

        prepare_runtime_replacement(&port, &mut record, target.clone()).unwrap();
        commit_runtime_replacement(&port, &mut record).unwrap();

        assert_eq!(record.runtime_binding(), target);
        assert_eq!(record.canonical_session_name, "w9");
        assert_eq!(
            port.load_workspace_identity(&record.identity_key())
                .unwrap()
                .unwrap()
                .runtime_binding(),
            target
        );
        assert_eq!(
            record.runtime_replacement,
            Some(RuntimeReplacement::Cleaning { previous: old })
        );
    }

    #[test]
    fn finishing_runtime_replacement_clears_durable_residue() {
        let port = FakeIdentityPort::new();
        let mut record = root_record();
        let target = RuntimeBinding::tmux("whsrv-a1b2-proj", "staged-target");
        prepare_runtime_replacement(&port, &mut record, target).unwrap();

        finish_runtime_replacement(&port, &mut record).unwrap();

        assert!(record.runtime_replacement.is_none());
        assert!(
            port.load_workspace_identity(&record.identity_key())
                .unwrap()
                .unwrap()
                .runtime_replacement
                .is_none()
        );
    }

    #[test]
    fn resolve_identity_finds_exact_root_match() {
        let port = FakeIdentityPort::new();
        let r = root_record();
        port.workspaces
            .borrow_mut()
            .insert(r.identity_key(), r.clone());

        let result =
            resolve_workspace_identity(&port, "main", "whsrv-a1b2-proj", "wh@a1b2@proj@main");
        assert_eq!(result.unwrap().workspace, "main");
    }

    #[test]
    fn resolve_identity_finds_scoped_match() {
        let port = FakeIdentityPort::new();
        let r = scoped_record();
        port.workspaces
            .borrow_mut()
            .insert(r.identity_key(), r.clone());

        let result =
            resolve_workspace_identity(&port, "feat-x", "whsrv-a1b2-proj", "wh@a1b2@proj@feat-x");
        let resolved = result.unwrap();
        assert_eq!(
            resolved.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("services/api")
        );
    }

    #[test]
    fn resolve_identity_zero_matches_returns_not_found() {
        let port = FakeIdentityPort::new();
        let result =
            resolve_workspace_identity(&port, "missing", "whsrv-a1b2-proj", "wh@a1b2@proj@missing");
        assert!(matches!(
            result,
            Err(IdentityError::IdentityNotFound { .. })
        ));
    }

    #[test]
    fn resolve_identity_ambiguous_returns_error() {
        let port = FakeIdentityPort::new();
        let r1 = root_record();
        let mut r2 = root_record();
        r2.relative_project_path = Some(RelativeProjectPath::new("sub").unwrap());
        port.workspaces.borrow_mut().insert(r1.identity_key(), r1);
        port.workspaces.borrow_mut().insert(r2.identity_key(), r2);

        let result =
            resolve_workspace_identity(&port, "main", "whsrv-a1b2-proj", "wh@a1b2@proj@main");
        assert!(matches!(
            result,
            Err(IdentityError::AmbiguousIdentity { count: 2, .. })
        ));
    }

    #[test]
    fn resolve_identity_mismatched_server_excludes() {
        let port = FakeIdentityPort::new();
        let r = root_record();
        port.workspaces.borrow_mut().insert(r.identity_key(), r);

        let result =
            resolve_workspace_identity(&port, "main", "wh@a1b2@proj@main", "whsrv-OTHER-proj");
        assert!(matches!(
            result,
            Err(IdentityError::IdentityNotFound { .. })
        ));
    }
}

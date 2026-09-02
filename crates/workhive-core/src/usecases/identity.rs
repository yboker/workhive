//! Identity metadata lifecycle — creation, validation, and consistency checks.
//!
//! These functions are called during `init` and `open` to ensure
//! project and workspace identity records exist and are consistent.

use camino::Utf8PathBuf;

use crate::domain::{
    IdentityError, NamingContext, ProjectIdentityRecord, SessionNamingPolicy, WorkspaceIdentityKey,
    WorkspaceIdentityRecord, WorkspaceStateKey,
};
use crate::ports::WorkspaceIdentityPort;

fn is_legacy_project_component_compatible(stored: &str, current: &str) -> bool {
    if stored == current {
        return true;
    }
    stored
        .strip_prefix(current)
        .map(|suffix| suffix.starts_with('/'))
        .unwrap_or(false)
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
        if existing.project_root != *project_root {
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
            if is_legacy_project_component_compatible(
                &existing.canonical_project_component,
                canonical_project_component,
            ) {
                let legacy_component = existing.canonical_project_component.clone();
                let updated = ProjectIdentityRecord {
                    canonical_project_component: canonical_project_component.to_owned(),
                    ..existing
                };
                tracing::warn!(
                    stored = %legacy_component,
                    current = %canonical_project_component,
                    "project identity uses legacy canonical project component with subdir suffix; migrating to normalized value"
                );
                identity_port.save_project_identity(&updated)?;
                return Ok(updated);
            }
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

/// Ensure workspace identity metadata exists and is consistent.
///
/// Called during `open` for each workspace.
///
/// - If no record exists, creates one from naming policy outputs.
/// - If a record exists and matches, returns it (idempotent).
/// - If a record exists, overwrites only if naming-derived values changed
///   (canonical fields updated, display fields preserved).
///
/// `subdir_key` is the sanitized subdirectory offset relative to project root
/// (empty string for repo-root workspaces). Together with `workspace`,
/// it forms the unique lookup key preventing collision between same-named
/// workspaces in different subdirectories.
pub fn ensure_workspace_identity(
    identity_port: &dyn WorkspaceIdentityPort,
    policy: &SessionNamingPolicy,
    ctx: &NamingContext,
    subdir_key: &str,
    relative_display_path: Option<&str>,
) -> Result<WorkspaceIdentityRecord, IdentityError> {
    let session_name = policy
        .resolve_session(ctx)
        .map_err(|e| IdentityError::Corrupted {
            path: Utf8PathBuf::from("<naming-policy>"),
            reason: e.to_string(),
        })?;
    let server_name = policy
        .resolve_server(ctx)
        .map_err(|e| IdentityError::Corrupted {
            path: Utf8PathBuf::from("<naming-policy>"),
            reason: e.to_string(),
        })?;

    let canonical_workspace = SessionNamingPolicy::sanitize_component(&ctx.workspace);

    let ws_key = WorkspaceIdentityKey::new(&ctx.workspace, subdir_key);

    if let Some(existing) = identity_port.load_workspace_identity(&ws_key)? {
        if existing.canonical_session_name == session_name
            && existing.canonical_server_name == server_name
        {
            return Ok(existing);
        }
        let updated = WorkspaceIdentityRecord {
            canonical_workspace_component: canonical_workspace,
            canonical_session_name: session_name,
            canonical_server_name: server_name,
            ..existing
        };
        identity_port.save_workspace_identity(&updated)?;
        return Ok(updated);
    }

    let record = WorkspaceIdentityRecord {
        workspace: ctx.workspace.clone(),
        subdir_key: subdir_key.to_owned(),
        relative_display_path: relative_display_path.map(|s| s.to_owned()),
        canonical_workspace_component: canonical_workspace,
        canonical_session_name: session_name,
        canonical_server_name: server_name,
    };
    identity_port.save_workspace_identity(&record)?;
    Ok(record)
}

/// Derive a `WorkspaceStateKey` from existing identity metadata.
///
/// This is the canonical way to get a snapshot state key — never from
/// reverse-parsing a sanitized tmux session name.
pub fn derive_workspace_state_key(
    identity_port: &dyn WorkspaceIdentityPort,
    key: &WorkspaceIdentityKey,
) -> Result<WorkspaceStateKey, IdentityError> {
    identity_port.derive_state_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{NamingConfig, WorkspaceStateKey};
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

        fn list_workspace_identities(&self) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
            Ok(self.workspaces.borrow().values().cloned().collect())
        }

        fn derive_state_key(
            &self,
            key: &WorkspaceIdentityKey,
        ) -> Result<WorkspaceStateKey, IdentityError> {
            let proj = self.project.borrow().clone().ok_or_else(|| {
                IdentityError::ProjectIdentityMissing {
                    path: Utf8PathBuf::from("<fake>"),
                }
            })?;
            let ws = self.workspaces.borrow().get(key).cloned().ok_or_else(|| {
                IdentityError::WorkspaceIdentityMissing {
                    workspace: key.to_string(),
                    path: Utf8PathBuf::from("<fake>"),
                }
            })?;
            Ok(WorkspaceStateKey::from_identity(&proj, &ws))
        }
    }

    fn default_naming_ctx() -> NamingContext {
        NamingContext {
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/my-project".to_owned()),
            instance_id: Some("a1b2".to_owned()),
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
    fn rejects_conflicting_project_display() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let err =
            ensure_project_identity(&port, "a1b2", &root, "other-name", "my-project").unwrap_err();
        assert!(matches!(err, IdentityError::ProjectDisplayConflict { .. }));
    }

    #[test]
    fn accepts_legacy_canonical_project_component_with_subdir_suffix() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project/subdir").unwrap();

        let result =
            ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();
        assert_eq!(result.canonical_project_component, "my-project");
        let stored = port
            .load_project_identity()
            .unwrap()
            .expect("project identity must remain present");
        assert_eq!(stored.canonical_project_component, "my-project");
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

        let result = ensure_workspace_identity(&port, &policy, &ctx, "", None).unwrap();
        assert_eq!(result.workspace, "main");
        assert_eq!(result.subdir_key, "");
        assert_eq!(result.canonical_session_name, "wh@a1b2@my-project@main");
        assert!(result.relative_display_path.is_none());
    }

    #[test]
    fn workspace_identity_is_idempotent() {
        let port = FakeIdentityPort::new();
        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();

        let r1 = ensure_workspace_identity(&port, &policy, &ctx, "", None).unwrap();
        let r2 = ensure_workspace_identity(&port, &policy, &ctx, "", None).unwrap();
        assert_eq!(r1, r2);
    }

    #[test]
    fn workspace_identity_updates_when_naming_changes() {
        let port = FakeIdentityPort::new();

        let policy1 = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();
        let r1 = ensure_workspace_identity(&port, &policy1, &ctx, "", None).unwrap();

        let policy2 = SessionNamingPolicy::new(NamingConfig {
            session_template: "wh2@{instance_id}@{project}@{workspace}".to_owned(),
            ..NamingConfig::default()
        });
        let r2 = ensure_workspace_identity(&port, &policy2, &ctx, "", None).unwrap();
        assert_ne!(r1.canonical_session_name, r2.canonical_session_name);
        assert_eq!(r2.workspace, "main");
    }

    // ── derive_workspace_state_key ──

    #[test]
    fn state_key_is_stable_across_repeated_open() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let policy = SessionNamingPolicy::default_policy();
        let ctx = default_naming_ctx();
        ensure_workspace_identity(&port, &policy, &ctx, "", None).unwrap();

        let ws_key = WorkspaceIdentityKey::new("main", "");
        let k1 = derive_workspace_state_key(&port, &ws_key).unwrap();
        let k2 = derive_workspace_state_key(&port, &ws_key).unwrap();
        assert_eq!(k1, k2);
    }

    #[test]
    fn state_key_differs_for_main_vs_subdir() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let policy = SessionNamingPolicy::default_policy();

        let ctx_main = default_naming_ctx();
        ensure_workspace_identity(&port, &policy, &ctx_main, "", None).unwrap();

        let ctx_sub = NamingContext {
            workspace: "feat-auth".to_owned(),
            ..default_naming_ctx()
        };
        ensure_workspace_identity(&port, &policy, &ctx_sub, "subdir", Some("subdir/feat-auth"))
            .unwrap();

        let k_main =
            derive_workspace_state_key(&port, &WorkspaceIdentityKey::new("main", "")).unwrap();
        let k_sub =
            derive_workspace_state_key(&port, &WorkspaceIdentityKey::new("feat-auth", "subdir"))
                .unwrap();
        assert_ne!(k_main, k_sub);
        assert_eq!(k_main.workspace, "main");
        assert_eq!(k_sub.workspace, "feat-auth");
    }

    #[test]
    fn same_workspace_different_subdirs_produce_different_state_keys() {
        let port = FakeIdentityPort::new();
        let root = Utf8PathBuf::from("/Users/dev/my-project");
        ensure_project_identity(&port, "a1b2", &root, "my-project", "my-project").unwrap();

        let policy = SessionNamingPolicy::default_policy();

        let ctx_a = NamingContext {
            project: "my-project/subdir-a".to_owned(),
            workspace: "feat-x".to_owned(),
            ..default_naming_ctx()
        };
        ensure_workspace_identity(
            &port,
            &policy,
            &ctx_a,
            "subdir-a",
            Some("my-project/subdir-a/feat-x"),
        )
        .unwrap();

        let ctx_b = NamingContext {
            project: "my-project/subdir-b".to_owned(),
            workspace: "feat-x".to_owned(),
            ..default_naming_ctx()
        };
        ensure_workspace_identity(
            &port,
            &policy,
            &ctx_b,
            "subdir-b",
            Some("my-project/subdir-b/feat-x"),
        )
        .unwrap();

        let sk_a =
            derive_workspace_state_key(&port, &WorkspaceIdentityKey::new("feat-x", "subdir-a"))
                .unwrap();
        let sk_b =
            derive_workspace_state_key(&port, &WorkspaceIdentityKey::new("feat-x", "subdir-b"))
                .unwrap();
        assert_ne!(sk_a, sk_b);
        assert_eq!(sk_a.subdir_key, "subdir-a");
        assert_eq!(sk_b.subdir_key, "subdir-b");
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
        };
        let result = ensure_workspace_identity(
            &port,
            &policy,
            &ctx,
            "1_0pingtai",
            Some("python/1.0pingtai"),
        )
        .unwrap();

        assert_eq!(
            result.relative_display_path.as_deref(),
            Some("python/1.0pingtai")
        );
        assert_eq!(result.subdir_key, "1_0pingtai");
        assert!(result.canonical_session_name.contains("1_0pingtai"));
    }
}

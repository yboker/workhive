#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;

fn resolve_close_snapshot_state_key(
    should_kill: bool,
    snapshot_config: &workhive_core::ports::SnapshotConfig,
    identity_port: &dyn workhive_core::ports::WorkspaceIdentityPort,
    naming_policy: &SessionNamingPolicy,
    naming_ctx: &NamingContext,
    workspace_identity_key: &workhive_core::domain::WorkspaceIdentityKey,
    relative_display_path: Option<&str>,
) -> Option<workhive_core::domain::WorkspaceStateKey> {
    use workhive_core::domain::IdentityError;

    if !should_kill || !snapshot_config.enabled {
        return None;
    }

    match usecases::identity::derive_workspace_state_key(identity_port, workspace_identity_key) {
        Ok(state_key) => return Some(state_key),
        Err(IdentityError::WorkspaceIdentityMissing { .. }) => {}
        Err(err) => {
            tracing::warn!(
                workspace = %workspace_identity_key,
                error = %err,
                "close snapshot identity unavailable"
            );
            return None;
        }
    }

    match usecases::identity::ensure_workspace_identity(
        identity_port,
        naming_policy,
        naming_ctx,
        &workspace_identity_key.subdir_key,
        relative_display_path,
    ) {
        Ok(_) => {
            match usecases::identity::derive_workspace_state_key(
                identity_port,
                workspace_identity_key,
            ) {
                Ok(state_key) => Some(state_key),
                Err(err) => {
                    tracing::warn!(
                        workspace = %workspace_identity_key,
                        error = %err,
                        "close snapshot identity repair did not produce a state key"
                    );
                    None
                }
            }
        }
        Err(err) => {
            tracing::warn!(
                workspace = %workspace_identity_key,
                error = %err,
                "close snapshot identity repair failed"
            );
            None
        }
    }
}

// ── Close (fixed: uses SessionName::extract_workspace) ──

pub(crate) fn cmd_close(
    kill: bool,
    force: bool,
    original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let env = ctx.composition.environment();
    let env_ctx = env.execution_context()?;

    let session_str = env_ctx.tmux_session.clone().ok_or(CliError::Environment(
        workhive_core::errors::EnvironmentError::NotInWorkspaceSession,
    ))?;

    let (root, name) = detect_project()?;
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let session_name = SessionName::from_raw(&session_str);
    let ws_str = session_name.extract_workspace().ok_or_else(|| {
        CliError::Runtime(
            "cannot extract workspace from current tmux session name; \
             this session may not follow workhive naming"
                .to_owned(),
        )
    })?;
    let _ws_id = WorkspaceId::new(ws_str)?;

    let verified_instance_id =
        enforce_or_delegate_project_policy(pol, root.as_ref(), original_command, &ws_fs, ctx)?
            .ok_or_else(|| {
                CliError::Runtime(
                    "instance_id must exist for an initialized project — close requires it"
                        .to_owned(),
                )
            })?;

    // Instance-id consistency check: session name vs project metadata.
    let session_iid = session_name.extract_instance_id().ok_or_else(|| {
        CliError::Environment(
            workhive_core::errors::EnvironmentError::SessionIdentityUnverifiable {
                session_name: session_str.clone(),
            },
        )
    })?;
    if session_iid != verified_instance_id {
        return Err(CliError::Environment(
            workhive_core::errors::EnvironmentError::InstanceIdMismatch {
                session_id: session_iid.to_owned(),
                project_id: verified_instance_id.clone(),
            },
        ));
    }

    let naming_policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context(&name, ws_str, &root, &git, &ws_fs);
    let (server, _from_env) =
        resolve_runtime_server(&env_ctx.tmux_server, &naming_policy, &naming_ctx)?;

    let should_kill = if kill {
        true
    } else if force {
        false
    } else if std::io::stdin().is_terminal() {
        let items = vec![
            ctx.i18n.resolve(locale, k::CLOSE_ACTION_DETACH),
            ctx.i18n.resolve(locale, k::CLOSE_ACTION_KILL),
        ];
        let title = ctx.i18n.resolve(locale, k::CLOSE_SELECT_ACTION);
        let selection = ui::screens::select::run(&title, &items, 0)?;
        if let Some(item) = items.get(selection) {
            crate::commands::print_choice_transcript(&title, item);
        }
        selection == 1
    } else {
        return Err(CliError::ConfirmationRequired(
            "this command requires --force in non-interactive mode".into(),
        ));
    };

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);
    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());
    let snapshot_config = resolve_effective_snapshot_config(ctx, root.as_ref())?;

    let close_identity_adapter = ctx.composition.workspace_identity(root.as_ref());
    let close_cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());
    let close_subdir_key = usecases::naming::resolve_subdir_key(&root, close_cwd.as_deref());
    let close_relative_display =
        usecases::naming::resolve_relative_display_path(&name, &root, ws_str, close_cwd.as_deref());
    let close_ws_identity_key =
        workhive_core::domain::WorkspaceIdentityKey::new(ws_str, &close_subdir_key);
    let close_state_key = resolve_close_snapshot_state_key(
        should_kill,
        &snapshot_config,
        &close_identity_adapter,
        &naming_policy,
        &naming_ctx,
        &close_ws_identity_key,
        close_relative_display.as_deref(),
    );

    let snap_ctx = usecases::close::SnapshotContext {
        instance_id: verified_instance_id,
        project: naming_ctx.project.clone(),
        server_name: server.clone(),
        runtime_backend: ctx.composition.runtime_backend(),
        platform: usecases::close::current_platform().to_owned(),
        state_key: close_state_key.as_ref().map(ToString::to_string),
    };

    let action = usecases::close::close_session_with_config(
        &runtime,
        &runtime,
        &snapshot_store,
        &server,
        &session_str,
        should_kill,
        &snapshot_config,
        &snap_ctx,
    )?;

    if should_kill
        && let Some(close_state_key) = close_state_key.as_ref()
        && let Err(e) = disable_snapshot_runtime_for_session(
            &runtime,
            &snapshot_store,
            &server,
            &session_str,
            close_state_key,
        )
    {
        tracing::warn!(
            session = %session_str,
            error = %e,
            "failed to clean up snapshot runtime after close --kill"
        );
    }

    // `close --kill` must also clean up the runtime control plane
    // supervisor. The supervisor's session-liveness probe will detect
    // the killed tmux session within ~1 second and stop on its own,
    // removing the bound socket on exit.
    // We additionally remove the socket file here as belt-and-suspenders
    // hygiene so a stale UDS never lingers in `.workhive/runtime/`.
    if should_kill {
        let socket =
            workhive_infra::control_plane::control_plane_socket_path(root.as_ref(), &server);
        if let Err(e) = workhive_infra::control_plane::remove_stale_socket(&socket) {
            tracing::debug!(
                socket = %socket,
                error = %e,
                "control-plane: best-effort socket cleanup after close --kill failed"
            );
        }
        // Keep the durable launch-identity store in step with the
        // killed workspace. Stale entries would otherwise survive a
        // re-create and feed misleading `display_title`s to the next
        // supervisor. Failures
        // here are best-effort; the worst case is one stale title
        // line, which the next `workspace open` will overwrite.
        use workhive_core::ports::PaneLaunchIdentityPort;
        let identity_store = ctx.composition.pane_launch_identity_store(root.as_ref());
        if let Err(e) = identity_store.forget_workspace(&server, &session_str) {
            tracing::debug!(
                server = %server,
                workspace = %session_str,
                error = %e,
                "control-plane: best-effort durable identity cleanup after close --kill failed"
            );
        }
    }

    match action {
        usecases::close::CloseAction::Detached => {
            eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_DETACHED));
        }
        usecases::close::CloseAction::Killed { snapshot_status } => {
            use workhive_core::usecases::close::SnapshotPersistStatus;

            match snapshot_status {
                SnapshotPersistStatus::Saved => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_KILLED));
                }
                SnapshotPersistStatus::Disabled => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_KILLED_NO_SNAPSHOT));
                }
                SnapshotPersistStatus::Failed { reason } => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_KILLED_NO_SNAPSHOT));
                    eprintln!(
                        "{}",
                        ctx.i18n
                            .resolve_with_args(locale, k::CLOSE_SNAPSHOT_FAILED, &[&reason])
                    );
                }
                SnapshotPersistStatus::SkippedNoUnits => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_KILLED_NO_SNAPSHOT));
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_SNAPSHOT_SKIPPED));
                }
            }
        }
    }
    Ok(())
}

// ── Snapshot runtime commands ──

#[cfg(test)]
mod close_snapshot_state_key_tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use workhive_core::domain::{
        IdentityError, NamingContext, ProjectIdentityRecord, SessionNamingPolicy,
        WorkspaceIdentityKey, WorkspaceIdentityRecord, WorkspaceStateKey,
    };
    use workhive_core::ports::{SnapshotConfig, WorkspaceIdentityPort};

    struct FakeIdentityPort {
        project: RefCell<Option<ProjectIdentityRecord>>,
        workspaces: RefCell<HashMap<WorkspaceIdentityKey, WorkspaceIdentityRecord>>,
        derive_calls: Cell<usize>,
        save_workspace_calls: Cell<usize>,
    }

    impl FakeIdentityPort {
        fn new(project: Option<ProjectIdentityRecord>) -> Self {
            Self {
                project: RefCell::new(project),
                workspaces: RefCell::new(HashMap::new()),
                derive_calls: Cell::new(0),
                save_workspace_calls: Cell::new(0),
            }
        }

        fn with_workspace(self, record: WorkspaceIdentityRecord) -> Self {
            self.workspaces
                .borrow_mut()
                .insert(record.identity_key(), record);
            self
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
            self.save_workspace_calls
                .set(self.save_workspace_calls.get() + 1);
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
            self.derive_calls.set(self.derive_calls.get() + 1);
            let project = self.project.borrow().clone().ok_or_else(|| {
                IdentityError::ProjectIdentityMissing {
                    path: Utf8PathBuf::from("<fake>/project.json"),
                }
            })?;
            let workspace = self.workspaces.borrow().get(key).cloned().ok_or_else(|| {
                IdentityError::WorkspaceIdentityMissing {
                    workspace: key.to_string(),
                    path: Utf8PathBuf::from("<fake>/workspaces"),
                }
            })?;
            Ok(WorkspaceStateKey::from_identity(&project, &workspace))
        }
    }

    fn project_record() -> ProjectIdentityRecord {
        ProjectIdentityRecord {
            instance_id: "a1b2".to_owned(),
            project_root: Utf8PathBuf::from("/Users/dev/my-project"),
            project_display: "my-project".to_owned(),
            canonical_project_component: "my-project".to_owned(),
        }
    }

    fn naming_ctx() -> NamingContext {
        NamingContext {
            project: "my-project".to_owned(),
            workspace: "main".to_owned(),
            branch: Some("main".to_owned()),
            dir_path: Some("/Users/dev/my-project".to_owned()),
            instance_id: Some("a1b2".to_owned()),
        }
    }

    fn workspace_record() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            subdir_key: String::new(),
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            canonical_session_name: "wh@a1b2@my-project@main".to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    fn workspace_key() -> WorkspaceIdentityKey {
        WorkspaceIdentityKey::new("main", "")
    }

    #[test]
    fn close_detach_does_not_require_workspace_identity() {
        let port = FakeIdentityPort::new(Some(project_record()));
        let config = SnapshotConfig::default();
        let state_key = resolve_close_snapshot_state_key(
            false,
            &config,
            &port,
            &SessionNamingPolicy::default_policy(),
            &naming_ctx(),
            &workspace_key(),
            None,
        );

        assert!(state_key.is_none());
        assert_eq!(port.derive_calls.get(), 0);
        assert_eq!(port.save_workspace_calls.get(), 0);
    }

    #[test]
    fn close_kill_with_snapshot_disabled_does_not_require_workspace_identity() {
        let port = FakeIdentityPort::new(Some(project_record()));
        let config = SnapshotConfig {
            enabled: false,
            ..SnapshotConfig::default()
        };
        let state_key = resolve_close_snapshot_state_key(
            true,
            &config,
            &port,
            &SessionNamingPolicy::default_policy(),
            &naming_ctx(),
            &workspace_key(),
            None,
        );

        assert!(state_key.is_none());
        assert_eq!(port.derive_calls.get(), 0);
        assert_eq!(port.save_workspace_calls.get(), 0);
    }

    #[test]
    fn close_kill_uses_existing_workspace_identity_state_key() {
        let port = FakeIdentityPort::new(Some(project_record())).with_workspace(workspace_record());
        let config = SnapshotConfig::default();
        let state_key = resolve_close_snapshot_state_key(
            true,
            &config,
            &port,
            &SessionNamingPolicy::default_policy(),
            &naming_ctx(),
            &workspace_key(),
            None,
        )
        .expect("state key should resolve from existing metadata");

        assert_eq!(state_key.to_string(), "a1b2@my-project@main");
        assert_eq!(port.derive_calls.get(), 1);
        assert_eq!(port.save_workspace_calls.get(), 0);
    }

    #[test]
    fn close_kill_repairs_missing_workspace_identity_before_snapshot() {
        let port = FakeIdentityPort::new(Some(project_record()));
        let config = SnapshotConfig::default();
        let state_key = resolve_close_snapshot_state_key(
            true,
            &config,
            &port,
            &SessionNamingPolicy::default_policy(),
            &naming_ctx(),
            &workspace_key(),
            None,
        )
        .expect("state key should resolve after metadata repair");

        assert_eq!(state_key.to_string(), "a1b2@my-project@main");
        assert_eq!(port.derive_calls.get(), 2);
        assert_eq!(port.save_workspace_calls.get(), 1);
    }

    #[test]
    fn close_kill_degrades_when_identity_cannot_produce_state_key() {
        let port = FakeIdentityPort::new(None);
        let config = SnapshotConfig::default();
        let state_key = resolve_close_snapshot_state_key(
            true,
            &config,
            &port,
            &SessionNamingPolicy::default_policy(),
            &naming_ctx(),
            &workspace_key(),
            None,
        );

        assert!(state_key.is_none());
        assert_eq!(port.derive_calls.get(), 1);
        assert_eq!(port.save_workspace_calls.get(), 0);
    }
}

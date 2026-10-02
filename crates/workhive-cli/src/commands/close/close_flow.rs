#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;
use workhive_core::ports::WorkspaceIdentityPort;

/// Resolved close context produced by `resolve_close_project`.
#[derive(Debug)]
pub(crate) struct CloseResolution {
    pub root: Utf8PathBuf,
    pub name: String,
    pub binding: workhive_core::domain::RuntimeBinding,
}

/// Candidate project root with its identity adapter, used during close resolution.
pub(crate) struct CloseCandidate<'a> {
    pub root: Utf8PathBuf,
    pub name: String,
    pub identity: &'a dyn WorkspaceIdentityPort,
}

/// Resolve the project and identity record for a close operation.
///
/// Examines each candidate project root through its persisted identity
/// records. The actual runtime server and session from `EnvironmentPort`
/// are authoritative.
///
/// Canonical matches are selected first. Zero or ambiguous matches fail
/// before any side effects.
pub(crate) fn resolve_close_identity(
    actual_server: &str,
    actual_session: &str,
    candidates: &[CloseCandidate<'_>],
) -> Result<CloseResolution, CliError> {
    let mut matches: Vec<(
        Utf8PathBuf,
        String,
        workhive_core::domain::WorkspaceIdentityRecord,
    )> = Vec::new();

    for candidate in candidates {
        let project_identity = match candidate.identity.load_project_identity() {
            Ok(Some(pi)) => pi,
            Ok(None) => continue,
            Err(error) => return Err(CliError::Identity(error)),
        };

        match usecases::identity::resolve_workspace_identity_by_runtime(
            candidate.identity,
            Some(&workhive_core::domain::RuntimeBackendId::tmux()),
            actual_server,
            actual_session,
            &project_identity,
        ) {
            Ok(record) => {
                matches.push((candidate.root.clone(), candidate.name.clone(), record));
            }
            Err(workhive_core::domain::IdentityError::IdentityNotFound { .. }) => continue,
            Err(error) => return Err(CliError::Identity(error)),
        }
    }

    let mut iter = matches.into_iter();
    match (iter.next(), iter.next()) {
        (Some((root, name, record)), None) => Ok(CloseResolution {
            root,
            name,
            binding: record.runtime_binding().clone(),
        }),
        (None, _) => Err(CliError::Identity(
            workhive_core::domain::IdentityError::IdentityNotFound {
                workspace: String::new(),
                session: actual_session.to_owned(),
                server: actual_server.to_owned(),
            },
        )),
        (Some((_, _, first_record)), Some(_)) => {
            let count = 2 + iter.count();
            Err(CliError::Identity(
                workhive_core::domain::IdentityError::AmbiguousIdentity {
                    workspace: first_record.workspace,
                    session: actual_session.to_owned(),
                    server: actual_server.to_owned(),
                    count,
                },
            ))
        }
    }
}

/// Build `CloseCandidate`s from the project index and resolve close identity.
pub(crate) fn resolve_close_project(
    actual_server: &str,
    actual_session: &str,
    ctx: &CommandContext,
) -> Result<CloseResolution, CliError> {
    // Use the shared trusted-project resolver for consistent state handling
    let projects = match crate::commands::resolve_trusted_project_index(ctx)? {
        crate::commands::TrustedProjectIndexState::NotFound { .. }
        | crate::commands::TrustedProjectIndexState::Empty { .. } => {
            // Empty candidates will fail resolution below with appropriate error
            Vec::new()
        }
        crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => projects,
    };

    let adapters: Vec<workhive_infra::workspace_identity::FileIdentityAdapter> = projects
        .iter()
        .map(|p| ctx.composition.workspace_identity(p.project_root.as_ref()))
        .collect();

    let candidates: Vec<CloseCandidate<'_>> = projects
        .iter()
        .zip(adapters.iter())
        .map(|(p, adapter)| CloseCandidate {
            root: p.project_root.clone(),
            name: p.project_name.clone(),
            identity: adapter as &dyn WorkspaceIdentityPort,
        })
        .collect();

    resolve_close_identity(actual_server, actual_session, &candidates)
}

fn run_after_close_resolution<T>(
    resolution: Result<CloseResolution, CliError>,
    action: impl FnOnce(CloseResolution) -> Result<T, CliError>,
) -> Result<T, CliError> {
    action(resolution?)
}

// ── Close ──

pub(crate) fn cmd_close(
    kill: bool,
    force: bool,
    _original_command: &str,
    _pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let env = ctx.composition.environment();
    let env_ctx = env.execution_context()?;

    let (server, session_str) = match &env_ctx.tmux {
        workhive_core::ports::TmuxMembership::Resolved {
            server, session, ..
        } => (server.clone(), session.clone()),
        _ => {
            return Err(CliError::Environment(
                workhive_core::errors::EnvironmentError::NotInWorkspaceSession,
            ));
        }
    };

    run_after_close_resolution(
        resolve_close_project(&server, &session_str, ctx),
        |resolution| {
            let root = resolution.root;
            let _name = resolution.name;

            crate::commands::project::enforce_trusted_root_or_error(
                root.as_ref(),
                ctx,
                crate::commands::project::AdmissionCaller::WorkspaceOpen,
            )?;

            // Determine action: kill or detach. Interactive mode shows a picker.
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

            let runtime = ctx.composition.runtime_for_binding(&resolution.binding)?;
            let action = usecases::close::close_session_binding(
                runtime.as_ref(),
                &resolution.binding,
                should_kill,
            )?;

            // Control-plane cleanup on kill.
            if should_kill {
                let socket = workhive_infra::control_plane::control_plane_socket_path(
                    root.as_ref(),
                    &server,
                );
                if let Err(e) = workhive_infra::control_plane::remove_stale_socket(&socket) {
                    tracing::debug!(
                        socket = %socket,
                        error = %e,
                        "control-plane: best-effort socket cleanup after close --kill failed"
                    );
                }
                // Clean up durable launch-identity store.
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
                usecases::close::CloseAction::Killed => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::CLOSE_KILLED));
                }
            }
            Ok(())
        },
    )
}

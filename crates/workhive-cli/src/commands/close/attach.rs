#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;

pub(crate) fn cmd_attach(
    workspace: Option<&str>,
    original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    match detect_project() {
        Ok((root, name)) => cmd_attach_project(workspace, &root, &name, original_command, pol, ctx),
        Err(_) => cmd_attach_global(workspace, ctx),
    }
}

/// Translate a `RuntimeError::BackendUnavailable` raised by attach-path
/// probes into the friendly localized no-attachable-session message.
///
/// The attach command is a user workflow ("connect to something
/// running"). When the underlying tmux server is not running or its
/// socket cannot be found, the product-meaningful translation is "there
/// is no active session to attach to here — run `workhive workspace
/// open` first". Raw socket paths are useful for adapter diagnostics
/// but they are not actionable for the user at this seam, so the raw
/// detail is preserved at debug level and the surface error is the
/// shared `attach.no_sessions` copy that already exists for the
/// "selector found zero workspaces" case.
///
/// Non-backend-unavailable runtime errors (workspace not found,
/// permission denied, operation failed, etc.) propagate unchanged so a
/// real failure is never silently re-labeled.
///
/// The translation seam takes `i18n` and `locale` directly rather than
/// the whole `CommandContext` so the mapping is a pure function of the
/// error variant and the localization catalog, which keeps unit tests
/// independent of the user's on-disk config (which would otherwise be
/// read by `CommandContext::locale()`).
pub(crate) fn map_attach_backend_unavailable_with(
    err: CliError,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> CliError {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::RuntimeError;
    if let CliError::RuntimeOp(RuntimeError::BackendUnavailable { reason, detail }) = &err {
        tracing::debug!(
            attach_seam = true,
            reason = ?reason,
            detail = %detail,
            "attach: backend unavailable mapped to no-attachable-session message"
        );
        return CliError::Runtime(i18n.resolve(locale, k::ATTACH_NO_SESSIONS));
    }
    err
}

/// Convenience wrapper around [`map_attach_backend_unavailable_with`]
/// that resolves `i18n` and `locale` from a [`CommandContext`].
///
/// Used by command handlers; tests prefer the `_with` variant.
pub(crate) fn map_attach_backend_unavailable(err: CliError, ctx: &CommandContext) -> CliError {
    let locale = ctx.locale();
    map_attach_backend_unavailable_with(err, &ctx.i18n, locale)
}

/// Attach within a known project context.
///
/// Attach never initializes the project and never creates, removes, or
/// reshapes workspaces, worktrees, or sessions. The only state it writes
/// is the workspace's last-used usage metadata, persisted before the
/// blocking attach call so the timestamp reflects when the user attached
/// rather than when they eventually detached; a failed metadata write is
/// non-fatal. When the repo is not initialized, policy may route the
/// command into the global attach flow instead.
pub(crate) fn cmd_attach_project(
    workspace: Option<&str>,
    root: &Utf8PathBuf,
    name: &str,
    original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let project_init =
        enforce_or_delegate_project_policy(pol, root.as_ref(), original_command, &ws_fs, ctx)?;
    if project_init.is_none() {
        return cmd_attach_global(workspace, ctx);
    }

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);
    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;

    let workspace_name = match workspace {
        Some(ws) => ws.to_owned(),
        None => select_attach_session(&runtime, name, root, &git, &ws_fs, &policy, ctx)?,
    };

    let _ws_id = WorkspaceId::new(&workspace_name)?;
    let naming_ctx = build_naming_context(name, &workspace_name, root, &git, &ws_fs);
    let server = policy.resolve_server(&naming_ctx)?;
    let session = policy.resolve_session(&naming_ctx)?;

    // Dual-path lookup: new naming first, then legacy (server+session paired).
    // A backend-unavailable on either lookup means there is no
    // running session to attach to in this project; surface the same
    // user-facing message instead of a raw socket path.
    let has_workspace_new = runtime
        .has_workspace(&server, &session)
        .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;
    let (actual_server, actual_session) = if has_workspace_new {
        (server, session.clone())
    } else {
        let legacy_server = SessionNamingPolicy::legacy_server(name);
        let legacy_session = SessionNamingPolicy::legacy_session(name, &workspace_name);
        let has_workspace_legacy = legacy_session != session
            && runtime
                .has_workspace(&legacy_server, &legacy_session)
                .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;
        if has_workspace_legacy {
            tracing::debug!(
                legacy_server = %legacy_server,
                legacy_session = %legacy_session,
                "attach: falling back to legacy session"
            );
            (legacy_server, legacy_session)
        } else {
            return Err(CliError::RuntimeOp(
                workhive_core::ports::RuntimeError::WorkspaceNotFound { workspace: session },
            ));
        }
    };

    crate::commands::attach_usage::mark_used_then_attach(
        &runtime,
        workhive_core::ports::AttachWorkspaceRequest {
            group: actual_server,
            workspace_name: actual_session.clone(),
        },
        || crate::commands::list::mark_workspace_used_now(root, &workspace_name, ctx),
    )
    .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;

    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::ATTACH_SUCCESS, &[&actual_session])
    );
    Ok(())
}

/// Global session selector when no project context is available.
///
/// Loads the project index, queries tmux sessions across all projects,
/// and presents an interactive selector for the user to pick a session.
pub(crate) fn cmd_attach_global(
    workspace: Option<&str>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::IndexState;
    let locale = ctx.locale();

    let index = ctx.composition.project_index()?;
    let state = index.list_projects()?;

    let summaries = match state {
        IndexState::NotFound => {
            return Err(CliError::Runtime(
                ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_INDEX),
            ));
        }
        IndexState::Loaded(v) if v.is_empty() => {
            return Err(CliError::Runtime(
                ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_SESSIONS),
            ));
        }
        IndexState::Loaded(v) => v,
    };

    let ws_fs = ctx.composition.workspace_fs();
    let resolution = usecases::resolve_projects::resolve_discovered_projects(&summaries, &ws_fs);
    render_stale_entries(&resolution.stale, ctx, locale);
    let discovered = resolution.valid;

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);

    let listing = usecases::sessions::list_global_sessions(
        &discovered,
        |project| ctx.naming_policy_for(Some(project.project_root.as_ref())),
        &runtime,
    )?;
    // A single project with an unreachable backend must not blank the global
    // close listing; it simply carries no closable sessions. Record it for
    // diagnostics and keep operating on the reachable sessions.
    for project in &listing.unavailable {
        tracing::debug!(
            project = %project.project_name,
            detail = %project.detail,
            "skipped unreachable runtime backend while collecting sessions for close"
        );
    }
    let records = listing.records;

    if records.is_empty() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_SESSIONS),
        ));
    }

    if let Some(workspace) = workspace {
        let chosen = records
            .iter()
            .find(|r| r.workspace == workspace || r.session_name == workspace)
            .ok_or_else(|| {
                CliError::RuntimeOp(workhive_core::ports::RuntimeError::WorkspaceNotFound {
                    workspace: workspace.to_owned(),
                })
            })?;

        crate::commands::attach_usage::mark_used_then_attach(
            &runtime,
            workhive_core::ports::AttachWorkspaceRequest {
                group: chosen.server_name.clone(),
                workspace_name: chosen.session_name.clone(),
            },
            || {
                crate::commands::list::mark_workspace_used_now(
                    &Utf8PathBuf::from(chosen.project_root.as_str()),
                    &chosen.workspace,
                    ctx,
                );
            },
        )
        .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;

        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::ATTACH_SUCCESS, &[&chosen.session_name])
        );
        return Ok(());
    }

    let plan = build_global_attach_plan(&records, &ctx.i18n, locale);
    let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    let (labels, disabled) = render_global_attach_lines(&plan, term_width, &ctx.i18n, locale);

    let title = ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_SELECT);
    let default_idx = disabled.iter().position(|d| !d).unwrap_or(0);
    let selection =
        ui::screens::select::run_with_disabled(&title, &labels, &disabled, default_idx)?;
    let row_index = selection - disabled[..selection].iter().filter(|d| **d).count();
    let chosen = &records[row_index];
    crate::commands::attach_usage::mark_used_then_attach(
        &runtime,
        workhive_core::ports::AttachWorkspaceRequest {
            group: chosen.server_name.clone(),
            workspace_name: chosen.session_name.clone(),
        },
        || {
            crate::commands::list::mark_workspace_used_now(
                &Utf8PathBuf::from(chosen.project_root.as_str()),
                &chosen.workspace,
                ctx,
            );
        },
    )
    .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;

    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::ATTACH_SUCCESS, &[&chosen.session_name])
    );
    Ok(())
}

/// Interactive session selector for `attach` when workspace arg is omitted.
///
/// Lists active tmux sessions on the project's server and lets the user pick one.
pub(crate) fn select_attach_session(
    ws_runtime: &dyn WorkspaceRuntimePort,
    project_name: &str,
    project_root: &Utf8PathBuf,
    git: &dyn GitPort,
    ws_fs: &dyn WorkspaceFsPort,
    policy: &SessionNamingPolicy,
    ctx: &CommandContext,
) -> Result<String, CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let naming_ctx = build_naming_context(project_name, "", project_root, git, ws_fs);
    let server = policy.resolve_server_for_project(&naming_ctx)?;
    // A missing tmux server / socket here is the product state
    // "nothing to attach to in this project"; translate it to the same
    // user-facing message as the empty-list branch so the picker entry
    // point never surfaces a raw socket path to the user.
    let workspaces = ws_runtime
        .list_workspaces(&server)
        .map_err(|e| map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx))?;

    if workspaces.is_empty() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::ATTACH_NO_SESSIONS),
        ));
    }

    // Probe the project's main branch so the selector can label each
    // row's workspace kind (Project root vs Worktree).
    let main_branch = git.current_branch()?;
    let mut plan = build_attach_selector_plan(&workspaces, &main_branch, &ctx.i18n, locale);

    // Fold in the same path + layout/recent context the `workspace list`
    // table renders, keyed by the parsed workspace name. Best-effort: if
    // the listing cannot be collected the selector keeps its bare
    // session-first identity rather than failing the attach flow.
    let enrichment = build_attach_enrichment_map(project_root, project_name, &main_branch, ctx);
    if !enrichment.is_empty() {
        decorate_attach_selector_plan(&mut plan, &|workspace| enrichment.get(workspace).cloned());
    }

    let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    let (labels, disabled) = render_attach_selector_lines(&plan, term_width, &ctx.i18n, locale);

    let title = ctx.i18n.resolve(locale, k::ATTACH_SELECT_SESSION);
    // Default to the first selectable row (the data row immediately
    // after the optional header) so Enter never lands on the disabled
    // column-header line.
    let default_idx = disabled
        .iter()
        .position(|d| !d)
        .ok_or_else(|| CliError::Runtime(ctx.i18n.resolve(locale, k::ATTACH_NO_SESSIONS)))?;
    let selection =
        ui::screens::select::run_with_disabled(&title, &labels, &disabled, default_idx)?;

    // Map the selected display index back to the matching selectable
    // entry in `plan.rows` by counting the non-selectable display rows
    // (section labels, column headers, and blank separators) that come
    // before the selection and subtracting that count.
    let row_index = selection - disabled[..selection].iter().filter(|d| **d).count();
    let session_name = &plan.rows[row_index].session_id;
    let parsed = SessionName::from_raw(session_name);
    parsed
        .extract_workspace()
        .map(|ws| ws.to_owned())
        .ok_or_else(|| {
            CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::ATTACH_CANNOT_EXTRACT_WORKSPACE,
                &[session_name],
            ))
        })
}

/// Build the per-workspace attach enrichment map for a project.
///
/// Reuses `collect_workspace_entries` — the same source the
/// `workspace list` table renders — so the attach selector and the list
/// view describe a workspace's path, layout, and recency identically.
/// Best-effort: returns an empty map when the project is not initialized
/// or any probe fails, so the caller falls back to bare session-first
/// rows instead of fabricating context. Keyed by parsed workspace name:
/// the project root maps under its main branch, linked worktrees under
/// their own name.
fn build_attach_enrichment_map(
    project_root: &Utf8PathBuf,
    project_name: &str,
    main_branch: &str,
    ctx: &CommandContext,
) -> std::collections::HashMap<String, AttachRowEnrichment> {
    let mut map: std::collections::HashMap<String, AttachRowEnrichment> =
        std::collections::HashMap::new();

    let ws_fs = ctx.composition.workspace_fs();
    if !usecases::project_state::resolve_init_state(&ws_fs, project_root.as_ref()).is_initialized()
    {
        return map;
    }

    let mut entries = match collect_workspace_entries(project_root, project_name, ctx) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!(error = %e, "attach selector enrichment unavailable; using bare rows");
            return map;
        }
    };
    apply_inactive_layout_fallback(&mut entries, ctx, project_root.as_ref());

    let locale = ctx.locale();
    for entry in &entries {
        let key = match entry.kind {
            workhive_core::usecases::list::WorkspaceKind::ProjectRoot => main_branch.to_owned(),
            workhive_core::usecases::list::WorkspaceKind::LinkedWorktree => entry.name.clone(),
        };
        let path = display_path_for_humans(entry.worktree_path.as_path());
        let parts = crate::commands::list::render_attach_enrichment_parts(entry, ctx, locale);
        map.insert(
            key,
            AttachRowEnrichment {
                path: Some(path),
                metadata: Some(AttachRowMetadata {
                    panes_layout: parts.layout,
                    recent: parts.recent,
                }),
            },
        );
    }

    map
}

/// Resolve the tmux server name from runtime context, preferring the real tmux
/// server (from `$TMUX` env) over cwd-derived naming policy.
///
/// Returns `(server_name, from_tmux_env)` where `from_tmux_env` indicates
/// whether the server was obtained from the tmux environment (true) or
/// from the naming policy fallback (false).
pub(crate) fn resolve_runtime_server(
    tmux_server: &Option<String>,
    naming_policy: &SessionNamingPolicy,
    naming_ctx: &NamingContext,
) -> Result<(String, bool), CliError> {
    if let Some(srv) = tmux_server {
        Ok((srv.clone(), true))
    } else {
        tracing::warn!(
            "tmux server not available from environment; \
             falling back to naming policy derivation (cwd-dependent)"
        );
        Ok((naming_policy.resolve_server(naming_ctx)?, false))
    }
}

/// Attach friendly no-session error mapping.
///
/// These tests pin the contract that `RuntimeError::BackendUnavailable`
/// raised inside an attach probe is translated into the localized
/// "no attachable session" copy (with the `workhive workspace open`
/// suggestion), that the raw tmux socket path is suppressed from the
/// default human-readable output, and that non-backend-unavailable
/// runtime errors continue to propagate as real errors instead of
/// being silently relabeled.
#[cfg(test)]
mod attach_friendly_error_tests {
    use super::*;
    use crate::error::CliError;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::{BackendUnavailableReason, I18nPort, RuntimeError};
    use workhive_infra::i18n::StaticI18n;

    /// Raw socket path representative of what the tmux adapter
    /// produces when the server is not running. The friendly mapper
    /// must drop this from the user-facing message.
    fn raw_socket_detail() -> String {
        "/private/tmp/tmux-501/whsrv-instance-demo".to_owned()
    }

    #[test]
    fn server_not_running_maps_to_no_attachable_session_message_en() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: "whsrv-instance-demo".to_owned(),
            },
            detail: raw_socket_detail(),
        });
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);
        let CliError::Runtime(msg) = mapped else {
            panic!("expected friendly Runtime message, got a different variant");
        };
        let expected = i18n.resolve(Locale::En, k::ATTACH_NO_SESSIONS);
        assert_eq!(
            msg, expected,
            "friendly message must come from i18n catalog"
        );
        assert!(
            !msg.contains("/private/tmp/tmux-"),
            "friendly message must not leak the raw tmux socket path: {msg}"
        );
        assert!(
            msg.contains("workhive workspace open"),
            "friendly message must suggest 'workhive workspace open': {msg}"
        );
    }

    #[test]
    fn socket_not_found_maps_to_no_attachable_session_message_en() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound {
                path: raw_socket_detail(),
            },
            detail: raw_socket_detail(),
        });
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);
        let CliError::Runtime(msg) = mapped else {
            panic!("expected friendly Runtime message, got a different variant");
        };
        assert!(
            !msg.contains("/private/tmp/tmux-"),
            "raw socket path must not appear in the default human output: {msg}"
        );
        assert!(
            msg.contains("workhive workspace open"),
            "must suggest 'workhive workspace open': {msg}"
        );
    }

    #[test]
    fn backend_unavailable_uses_chinese_friendly_copy_when_locale_is_zh() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: "whsrv-instance-demo".to_owned(),
            },
            detail: raw_socket_detail(),
        });
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::ZhCn);
        let CliError::Runtime(msg) = mapped else {
            panic!("expected friendly Runtime message in zh-CN locale");
        };
        let expected = i18n.resolve(Locale::ZhCn, k::ATTACH_NO_SESSIONS);
        assert_eq!(msg, expected);
        assert!(
            msg.contains("workhive workspace open"),
            "zh-CN copy must keep the actionable command verbatim: {msg}"
        );
        assert!(
            !msg.contains("/private/tmp/tmux-"),
            "zh-CN copy must not leak raw socket paths: {msg}"
        );
    }

    #[test]
    fn non_backend_unavailable_runtime_errors_propagate_unchanged() {
        let i18n = StaticI18n::new();
        // Workspace-not-found is a real failure (the user named a
        // workspace that does not exist); it must surface as a real
        // error, not get rewritten as "no session".
        let err = CliError::RuntimeOp(RuntimeError::WorkspaceNotFound {
            workspace: "wh@instance@proj@missing".to_owned(),
        });
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);
        match mapped {
            CliError::RuntimeOp(RuntimeError::WorkspaceNotFound { workspace }) => {
                assert_eq!(workspace, "wh@instance@proj@missing");
            }
            other => panic!("workspace-not-found must propagate unchanged, got {other:?}"),
        }
    }

    #[test]
    fn operation_failed_runtime_errors_propagate_unchanged() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::OperationFailed {
            message: "attach failed: permission denied".to_owned(),
        });
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);
        match mapped {
            CliError::RuntimeOp(RuntimeError::OperationFailed { message }) => {
                assert_eq!(message, "attach failed: permission denied");
            }
            other => panic!("operation-failed must propagate unchanged, got {other:?}"),
        }
    }

    #[test]
    fn non_runtime_errors_are_not_touched_by_the_mapper() {
        let i18n = StaticI18n::new();
        let err = CliError::Runtime("preexisting friendly error".to_owned());
        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);
        let CliError::Runtime(msg) = mapped else {
            panic!("Runtime variant must round-trip unchanged");
        };
        assert_eq!(msg, "preexisting friendly error");
    }
}

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
    if let CliError::RuntimeOp(RuntimeError::BackendUnavailable { reason, detail }) = &err
        && reason.is_runtime_absent()
    {
        tracing::debug!(
            attach_seam = true,
            reason = ?reason,
            detail = %detail,
            "attach: backend unavailable mapped to no-attachable-session message"
        );
        return CliError::ResourceNotFound(i18n.resolve(locale, k::ATTACH_NO_SESSIONS));
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

fn choose_project_attach_record(
    records: &[workhive_core::usecases::sessions::SessionRecord],
    ctx: &CommandContext,
) -> Result<workhive_core::usecases::sessions::SessionRecord, CliError> {
    let locale = ctx.locale();
    let main_branch = ctx.composition.git().current_branch()?;
    let mut plan = build_attach_selector_plan(records, &main_branch, &ctx.i18n, locale);
    if let Some(first) = records.first() {
        let project_root = Utf8PathBuf::from(&first.project_root);
        let enrichment =
            build_attach_enrichment_map(&project_root, &first.project_name, &main_branch, ctx);
        decorate_attach_selector_plan(&mut plan, &|workspace| enrichment.get(workspace).cloned());
    }
    let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    let (labels, disabled) = render_attach_selector_lines(&plan, term_width, &ctx.i18n, locale);
    let title = ctx
        .i18n
        .resolve(locale, workhive_core::i18n_keys::ATTACH_SELECT_SESSION);
    let default_idx = disabled
        .iter()
        .position(|disabled| !disabled)
        .ok_or_else(|| {
            CliError::ResourceNotFound(
                ctx.i18n
                    .resolve(locale, workhive_core::i18n_keys::ATTACH_NO_SESSIONS),
            )
        })?;
    let row_index =
        ui::screens::select::run_with_disabled_data_index(&title, &labels, &disabled, default_idx)?;
    Ok(records[plan.rows[row_index].source_index].clone())
}

fn choose_attach_record(
    query: Option<&str>,
    records: &[workhive_core::usecases::sessions::SessionRecord],
    title_key: &str,
    ctx: &CommandContext,
) -> Result<workhive_core::usecases::sessions::SessionRecord, CliError> {
    if let Some(query) = query {
        return crate::commands::session::resolve_session_target(query, records, ctx);
    }

    let locale = ctx.locale();
    let plan = build_global_attach_plan(records, &ctx.i18n, locale);
    let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    let (labels, disabled) = render_global_attach_lines(&plan, term_width, &ctx.i18n, locale);
    let title = ctx.i18n.resolve(locale, title_key);
    let default_idx = disabled.iter().position(|disabled| !disabled).unwrap_or(0);
    let row_index =
        ui::screens::select::run_with_disabled_data_index(&title, &labels, &disabled, default_idx)?;
    Ok(records[row_index].clone())
}

fn attach_record_with_usage(
    record: &workhive_core::usecases::sessions::SessionRecord,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let binding = &record.binding;
    let runtime = ctx.composition.runtime_for_binding(binding)?;
    let request = crate::commands::build_attach_request(&ctx.config, record)?;
    let exec_ctx = ctx.composition.environment().execution_context()?;
    let project_root = Utf8PathBuf::from(record.project_root.as_str());
    let locale = ctx.locale();

    crate::commands::attach_usage::mark_used_then_attach_binding(
        runtime.as_ref(),
        binding,
        request,
        &exec_ctx,
        crate::commands::attach_usage::AttachLifecycleCallbacks {
            mark_used: || {
                crate::commands::list::mark_workspace_used_now(
                    &project_root,
                    &record.workspace,
                    ctx,
                );
            },
            emit_ready_output: || {},
            emit_guidance: || crate::commands::session::print_attach_guidance(ctx),
            emit_already_current: || crate::commands::session::print_already_current(ctx),
        },
    )
    .map_err(|error| {
        let error = crate::commands::attach_usage::map_navigation_error(error, &ctx.i18n, locale);
        map_attach_backend_unavailable(error, ctx)
    })?;
    Ok(())
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
    let ws_fs = ctx.composition.workspace_fs();

    let Some(instance_id) =
        enforce_or_delegate_project_policy(pol, root.as_ref(), original_command, &ws_fs, ctx)?
    else {
        return cmd_attach_global(workspace, ctx);
    };
    if let Some(workspace) = workspace {
        WorkspaceId::new(workspace)?;
    }

    let project = workhive_core::ports::DiscoveredProject {
        project_name: name.to_owned(),
        project_root: root.clone(),
        instance_id,
        initialized: true,
    };

    let listing = crate::commands::session::list_global_sessions_by_backend(&[project], ctx)?;
    let records = listing.records;
    if records.is_empty() {
        return Err(CliError::ResourceNotFound(ctx.i18n.resolve(
            ctx.locale(),
            workhive_core::i18n_keys::ATTACH_NO_SESSIONS,
        )));
    }

    let chosen = match workspace {
        Some(query) => crate::commands::session::resolve_session_target(query, &records, ctx)?,
        None => choose_project_attach_record(&records, ctx)?,
    };
    attach_record_with_usage(&chosen, ctx)
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
    let locale = ctx.locale();

    // Use the shared trusted-project resolver to get typed state distinction
    let summaries = match crate::commands::resolve_trusted_project_index(ctx)? {
        crate::commands::TrustedProjectIndexState::NotFound { .. } => {
            return Err(CliError::ResourceNotFound(
                ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_INDEX),
            ));
        }
        crate::commands::TrustedProjectIndexState::Empty { .. } => {
            return Err(CliError::ResourceNotFound(
                ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_SESSIONS),
            ));
        }
        crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => projects,
    };

    let ws_fs = ctx.composition.workspace_fs();
    let resolution = usecases::resolve_projects::resolve_discovered_projects(&summaries, &ws_fs);
    render_stale_entries(&resolution.stale, ctx, locale);
    let discovered = resolution.valid;

    let listing = crate::commands::session::list_global_sessions_by_backend(&discovered, ctx)?;
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
        return Err(CliError::ResourceNotFound(
            ctx.i18n.resolve(locale, k::ATTACH_GLOBAL_NO_SESSIONS),
        ));
    }

    let chosen = choose_attach_record(
        workspace,
        &records,
        workhive_core::i18n_keys::ATTACH_GLOBAL_SELECT,
        ctx,
    )?;
    attach_record_with_usage(&chosen, ctx)
}

fn build_attach_enrichment_map(
    project_root: &Utf8PathBuf,
    project_name: &str,
    main_branch: &str,
    ctx: &CommandContext,
) -> std::collections::HashMap<String, AttachRowEnrichment> {
    let ws_fs = ctx.composition.workspace_fs();
    if !usecases::project_state::resolve_init_state(&ws_fs, project_root.as_ref()).is_initialized()
    {
        return std::collections::HashMap::new();
    }

    let mut entries = match collect_workspace_entries(project_root, project_name, ctx) {
        Ok(entries) => entries,
        Err(error) => {
            tracing::debug!(%error, "attach selector enrichment unavailable");
            return std::collections::HashMap::new();
        }
    };
    let layout = crate::commands::open::load_effective_layout(ctx, Some(project_root.as_ref()));
    apply_inactive_layout_fallback(&mut entries, &layout);

    let locale = ctx.locale();
    entries
        .into_iter()
        .map(|entry| {
            let workspace = match entry.kind {
                workhive_core::usecases::list::WorkspaceKind::ProjectRoot => main_branch.to_owned(),
                workhive_core::usecases::list::WorkspaceKind::LinkedWorktree => entry.name.clone(),
            };
            let path = display_path_for_humans(entry.worktree_path.as_path());
            (
                workspace,
                AttachRowEnrichment {
                    path: Some(path),
                    metadata: Some(AttachRowMetadata {
                        panes_layout: layout_display(&entry, ctx, locale),
                        recent: recent_display(&entry, ctx, locale),
                    }),
                },
            )
        })
        .collect()
}

/// Resolve the tmux server name from runtime context, preferring the real tmux
/// server (from `$TMUX` env) over cwd-derived naming policy.
///
/// Returns the server name, using the naming policy only when the process is
/// not already inside a tmux server.
pub(crate) fn resolve_runtime_server(
    tmux_server: &Option<String>,
    naming_policy: &SessionNamingPolicy,
    naming_ctx: &NamingContext,
) -> Result<String, CliError> {
    if let Some(srv) = tmux_server {
        Ok(srv.clone())
    } else {
        tracing::warn!(
            "tmux server not available from environment; \
             falling back to naming policy derivation (cwd-dependent)"
        );
        Ok(naming_policy.resolve_server(naming_ctx)?)
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
        let CliError::ResourceNotFound(msg) = mapped else {
            panic!("expected friendly ResourceNotFound message, got a different variant");
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
        let CliError::ResourceNotFound(msg) = mapped else {
            panic!("expected friendly ResourceNotFound message, got a different variant");
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
        let CliError::ResourceNotFound(msg) = mapped else {
            panic!("expected friendly ResourceNotFound message in zh-CN locale");
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
    fn missing_backend_binary_maps_to_no_attachable_session() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::BackendBinaryNotFound {
                binary: "herdr".to_owned(),
            },
            detail: "herdr not found".to_owned(),
        });

        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);

        let CliError::ResourceNotFound(message) = mapped else {
            panic!("expected friendly ResourceNotFound message");
        };
        assert_eq!(message, i18n.resolve(Locale::En, k::ATTACH_NO_SESSIONS));
    }

    #[test]
    fn capability_missing_propagates_unchanged() {
        let i18n = StaticI18n::new();
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::CapabilityMissing {
                capability: "workspace.list".to_owned(),
            },
            detail: "Herdr protocol is incompatible".to_owned(),
        });

        let mapped = map_attach_backend_unavailable_with(err, &i18n, Locale::En);

        assert!(matches!(
            mapped,
            CliError::RuntimeOp(RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::CapabilityMissing { .. },
                ..
            })
        ));
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

use super::*;
use crate::commands::pane_control_plane::dispatch_pane_send;
use crate::commands::pane_list::print_pane_list;
use crate::commands::pane_send::{
    enforce_not_self_target, plan_pane_send_guarded, resolve_pane_target,
};
use crate::commands::pane_show::print_pane_show;
use crate::commands::pane_view_data::{
    build_observed_inputs, load_identity_entries, load_tool_catalog,
};

pub(crate) fn cmd_pane(
    action: PaneAction,
    original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let env = ctx.composition.environment();
    let env_ctx = env.execution_context()?;
    let session_owned = env_ctx.tmux_session.clone().ok_or(CliError::Environment(
        workhive_core::errors::EnvironmentError::NotInWorkspaceSession,
    ))?;
    let session: &str = &session_owned;

    let (root, name) = detect_project()?;
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let verified_instance_id =
        enforce_or_delegate_project_policy(pol, root.as_ref(), original_command, &ws_fs, ctx)?;

    let np = ctx.naming_policy_for(Some(root.as_ref()))?;

    let parsed_session = SessionName::from_raw(session);
    let ws_str = parsed_session.extract_workspace().ok_or_else(|| {
        CliError::Runtime(
            "cannot extract workspace from current tmux session name; \
             this session may not follow workhive naming"
                .to_owned(),
        )
    })?;

    // Instance-id consistency check: session name vs project metadata.
    let session_iid = parsed_session.extract_instance_id().ok_or_else(|| {
        CliError::Environment(
            workhive_core::errors::EnvironmentError::SessionIdentityUnverifiable {
                session_name: session.to_owned(),
            },
        )
    })?;
    if let Some(project_iid) = &verified_instance_id
        && session_iid != project_iid.as_str()
    {
        return Err(CliError::Environment(
            workhive_core::errors::EnvironmentError::InstanceIdMismatch {
                session_id: session_iid.to_owned(),
                project_id: project_iid.clone(),
            },
        ));
    }

    let naming_ctx = build_naming_context(&name, ws_str, &root, &git, &ws_fs);
    let (server, _from_env) = resolve_runtime_server(&env_ctx.tmux_server, &np, &naming_ctx)?;
    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);

    match action {
        PaneAction::Split {
            direction,
            cmd,
            force,
        } => {
            confirm::require_confirm(&ctx.i18n.resolve(locale, k::PANE_CONFIRM_SPLIT), force)?;
            let dir: PaneDirection = direction.into();
            // Capture the user's --cmd before we move it into the runtime
            // spawn request; we need it again to classify the durable
            // launch identity once the pane has been spawned.
            let raw_cmd = cmd.clone();
            let pane = usecases::pane::split_pane(&runtime, &server, session, dir, cmd)?;
            // Classify the user-supplied command against the layered
            // tool catalog so a known tool name (`codex`, `claude`,
            // ...) is recorded as a `Tool` while everything else
            // (unknown programs, path-qualified binaries, shell
            // metacharacter prefixes) is preserved verbatim as
            // `InlineCommand`. `Tool` recognition also accepts known
            // tool names followed by common flags (`codex --flag`,
            // `claude --dangerously-skip-permissions`, ...) so durable
            // identity matches the user's intent even when arguments
            // are present. The user's full `--cmd` string was already
            // passed verbatim to the runtime above, so flagged
            // invocations execute with their args intact; classification
            // only governs what we record for identity and title
            // display. Both branches populate the durable identity
            // store and reassert the pane title immediately, so
            // `pane list` / `pane show` recognize the new pane without
            // waiting for a `close`/`open` restore cycle.
            let tools = load_tool_catalog(ctx, &root);
            let launch =
                usecases::launch_resolver::classify_split_command(raw_cmd.as_deref(), &tools);
            let identity_store = ctx.composition.pane_launch_identity_store(&root);
            // Durable launch identity is keyed by the **canonical session
            // name** (e.g. `wh@{iid}@{project}@{workspace}`) — the exact
            // string the tmux runtime adapter, the supervisor
            // subprocess, and the `workspace open` writer all use. Using
            // only the workspace suffix (`ws_str`, e.g. `main`) would
            // make `pane list` / `pane show` and the supervisor miss the
            // entry, because the runtime adapter looks the registry up
            // by `PaneRuntimeId::derive(group, canonical_session, unit)`.
            usecases::pane::record_split_launch_metadata(
                &runtime,
                &identity_store,
                &server,
                session,
                &pane.id,
                &launch,
            );
            eprintln!(
                "{} ({})",
                ctx.i18n.resolve(locale, k::PANE_SPLIT_SUCCESS),
                pane.id
            );
        }
        PaneAction::Close { force } => {
            confirm::require_confirm(&ctx.i18n.resolve(locale, k::PANE_CONFIRM_CLOSE), force)?;
            let current = usecases::pane::current_pane(&runtime, &server)?;
            usecases::pane::close_pane(&runtime, &server, &current.id)?;
            eprintln!("{}", ctx.i18n.resolve(locale, k::PANE_CLOSE_SUCCESS));
        }
        PaneAction::List => {
            let panes = usecases::pane::list_panes(&runtime, &server, session)?;
            // Read identity entries and observe the supervisor under the
            // canonical session name, so the keys match what `open` and
            // `pane split` write and what the supervisor child writes
            // its metadata under.
            let identity = load_identity_entries(ctx, &root, &server, session);
            let supervisor = ctx.composition.pane_supervisor(&root);
            let observer = ctx.composition.process_observer();
            let tools = load_tool_catalog(ctx, &root);
            let inputs = build_observed_inputs(
                &panes,
                &supervisor,
                observer.as_ref(),
                &server,
                session,
                &tools,
            );
            let rows = usecases::pane::build_pane_list_view_from_inputs(&inputs, &identity);
            print_pane_list(ctx, locale, &rows, &root, &server, session, &tools);
        }
        PaneAction::Show { pane: target } => {
            // No-argument `pane show` keeps its original behaviour: show
            // the pane currently running the command. With a target
            // argument, resolve it against the same pane snapshot users
            // see in `pane list`, reusing the shared `pane send` target
            // resolver so pane numbers (`2`) and tmux target ids (`%2`)
            // map onto the same canonical `UnitId`. A target that
            // resolves but is absent from the current snapshot is
            // surfaced with the existing localized pane-target error that
            // points users back to `pane list`.
            let pane = match target {
                None => usecases::pane::current_pane(&runtime, &server)?,
                Some(raw) => {
                    let panes = usecases::pane::list_panes(&runtime, &server, session)?;
                    let resolved = resolve_pane_target(&raw, &panes, ctx)?;
                    panes
                        .into_iter()
                        .find(|candidate| candidate.id == resolved.unit_id)
                        .ok_or_else(|| {
                            CliError::Runtime(ctx.i18n.resolve_with_args(
                                locale,
                                k::PANE_TARGET_INDEX_NOT_FOUND,
                                &[resolved.input.as_str()],
                            ))
                        })?
                }
            };
            let identity = load_identity_entries(ctx, &root, &server, session);
            let supervisor = ctx.composition.pane_supervisor(&root);
            let observer = ctx.composition.process_observer();
            let tools = load_tool_catalog(ctx, &root);
            let inputs = build_observed_inputs(
                std::slice::from_ref(&pane),
                &supervisor,
                observer.as_ref(),
                &server,
                session,
                &tools,
            );
            let input = inputs.into_iter().next().unwrap_or_else(|| {
                usecases::pane::PaneObservedViewInput::new(
                    pane.clone(),
                    workhive_core::domain::PaneRuntimeTruth::from_layered(
                        pane.kind,
                        pane.state,
                        pane.command.clone(),
                        workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
                        workhive_core::domain::PaneObservationConfidence::Fallback,
                    ),
                )
            });
            let view = usecases::pane::build_pane_show_view_from_input(&input, &identity);
            print_pane_show(ctx, locale, &view.row, &root, &server, session, &tools);
        }
        PaneAction::ApproveHooks { pane: _ } => {
            // Deprecated compatibility alias. Workhive no longer installs
            // provider-side hooks, so this command performs no provider
            // input dispatch. The alias remains parseable so legacy
            // scripts keep exiting `0`; users get a single localized
            // notice explaining that approval is unnecessary.
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::PANE_APPROVE_HOOKS_DEPRECATED)
            );
        }
        PaneAction::Send {
            pane,
            text,
            no_enter,
            force,
        } => {
            let panes = usecases::pane::list_panes(&runtime, &server, session)?;
            let target = resolve_pane_target(&pane, &panes, ctx)?;
            // Reject sending to the current pane before any confirmation
            // or control-plane dispatch. Both numeric pane numbers and
            // tmux `%target_id` inputs resolve to the same canonical
            // `UnitId`, so both forms are rejected.
            let current = usecases::pane::current_pane(&runtime, &server)?;
            enforce_not_self_target(&target, &current.id, ctx, locale)?;

            // ── Structured pane-send safety boundary ─────────────────
            //
            // Before any user prompt or control-plane dispatch, observe
            // the target pane through the existing layered observation
            // stack (supervisor → OS observer → tmux fallback) and ask
            // the typed core resolver what we are allowed to do. The
            // resolver lives in `workhive-core::usecases::pane_send` so
            // the rule table (TUI refusal, unverified-agent warning,
            // `--no-enter` text-only insert, `--force` raw override) is
            // unit-tested without spinning up tmux or the control plane.
            //
            // The full guarded-send composition (snapshot lookup,
            // observation gathering, conservative ACP read, metrics
            // derivation, decision translation, warning emission) lives
            // in `pane_send::plan_pane_send_guarded` so the dispatcher
            // here stays focused on confirm + dispatch wiring.
            let plan = plan_pane_send_guarded(
                ctx, locale, &root, &server, ws_str, session, &panes, &target, &text, no_enter,
                force,
            )?;

            // `pane send` is the one explicit Yes-default: the user has
            // already typed both the target and the literal text, so
            // highlighting "No" actively wastes a keystroke for the
            // common case. The prompt itself is preserved.
            confirm::require_confirm_with_default(
                &target.confirm_message(ctx, locale),
                force,
                confirm::ConfirmDefault::Yes,
            )?;

            // Submit semantics now come from the typed plan, not from
            // a stringly-typed `!no_enter` decision scattered at the
            // CLI boundary. Tmux key syntax remains owned by the tmux
            // adapter; core only knows "submit yes/no". Self-heal must
            // spawn the supervisor with the same canonical session name
            // that `workspace open` originally used (see `open.rs`);
            // otherwise the respawned supervisor would write its
            // registry under a different `--workspace` value than the
            // metadata it tries to read, breaking pane control entirely.
            dispatch_pane_send(
                ctx,
                &runtime,
                &server,
                session,
                &target.unit_id,
                &plan.text,
                plan.submit,
            )?;
            eprintln!("{}", target.success_message(ctx, locale));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Only dispatcher / command-layer wiring tests remain in this
    // module. Rendering tests (list / show), AI activity overlay
    // tests, and guarded-send / ACP read-helper tests live in their
    // owning sibling modules:
    //   * `pane_list` — list rendering and post-table hint blocks
    //   * `pane_show` — show rendering and AI overlay lines
    //   * `pane_activity_overlay` — overlay computation, lifecycle
    //     projection, and the ACP overlay bridge
    //   * `pane_send` — guarded send planning and ACP read helper
    //
    // Keep this module focused on `dispatch_pane_send` /
    // `PaneAction::ApproveHooks` wiring so the dispatcher's contracts
    // stay reviewable independent of the rendering pipeline.
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;
    use workhive_infra::i18n::StaticI18n;

    // ───────────────────────────────────────────────────────────────
    // approve-hooks deprecated no-op alias
    // ───────────────────────────────────────────────────────────────
    //
    // `pane approve-hooks` remains parseable as a hidden compatibility
    // alias but the dispatcher performs no provider input. These tests
    // assert the localized deprecation notice resolves cleanly in both
    // locales and never guides the user back into a provider hook
    // approval flow.

    /// The deprecation notice key must resolve to a non-empty string
    /// in both locales, frame the command as deprecated, and never
    /// guide the user back into a provider hook approval flow (no
    /// `/hooks` slash command, no `workhive integrations` invocation).
    #[test]
    fn approve_hooks_deprecated_notice_resolves_without_hook_guidance() {
        let i18n = StaticI18n::new();
        let en = i18n.resolve(Locale::En, k::PANE_APPROVE_HOOKS_DEPRECATED);
        let zh = i18n.resolve(Locale::ZhCn, k::PANE_APPROVE_HOOKS_DEPRECATED);

        assert!(
            !en.trim().is_empty(),
            "EN deprecation notice must be non-empty"
        );
        assert!(
            !zh.trim().is_empty(),
            "ZH deprecation notice must be non-empty"
        );

        for text in [&en, &zh] {
            assert!(
                !text.contains("/hooks"),
                "deprecation notice must not reference `/hooks`: {text:?}",
            );
            assert!(
                !text.contains("workhive integrations"),
                "deprecation notice must not reference the rescinded `workhive integrations` command: {text:?}",
            );
        }

        assert!(
            en.to_lowercase().contains("deprecated"),
            "EN deprecation notice must frame the command as deprecated: {en:?}",
        );
        assert!(
            zh.contains("已废弃"),
            "ZH deprecation notice must frame the command as deprecated: {zh:?}",
        );
    }

    /// `pane` subcommands run a session preflight that fails with
    /// `EnvironmentError::NotInWorkspaceSession` when the shell is not in
    /// a workhive-managed session. That typed error must localize through
    /// the CLI boundary into actionable recovery guidance rather than
    /// leaking the raw English core `Display` as the only information.
    #[test]
    fn not_in_workspace_session_localizes_with_recovery_commands() {
        use crate::error::CliError;
        use crate::error_render::render_localized_error;
        use workhive_core::errors::EnvironmentError;

        let err = CliError::Environment(EnvironmentError::NotInWorkspaceSession);
        let i18n = StaticI18n::new();

        let zh = render_localized_error(&err, Locale::ZhCn, &i18n)
            .expect("pane session preflight error must localize at the CLI boundary");
        assert!(
            !zh.to_lowercase()
                .contains("not inside a workhive-managed tmux session"),
            "zh-CN pane preflight error must not leak raw English: {zh}"
        );
        assert!(
            zh.contains("workhive workspace open") && zh.contains("workhive workspace attach"),
            "zh-CN pane preflight error must guide recovery: {zh}"
        );

        let en = render_localized_error(&err, Locale::En, &i18n)
            .expect("pane session preflight error must localize at the CLI boundary");
        assert!(
            en.contains("workhive workspace open") && en.contains("workhive workspace attach"),
            "EN pane preflight error must guide recovery: {en}"
        );
    }
}

// ── Reindex ──

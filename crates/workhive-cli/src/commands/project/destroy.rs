#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

/// Pure decision: given a resolved project init state, what should
/// `workspace destroy` do?
///
/// Extracted so the preflight order is testable without spinning up a
/// full `CommandContext`. The decision is deliberately exhaustive: there
/// is no fallback that silently creates `.workhive/`, and there is no
/// path that runs the destructive confirm before the project-state
/// checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DestroyEligibility {
    /// Project is initialized — destroy may proceed to safety scan + confirm.
    Proceed,
    /// `.workhive/` is missing — fail fast with `DESTROY_NOT_INITIALIZED`.
    /// No prompt-to-init, no destructive warning.
    NoWorkhiveState,
    /// `.workhive/` exists but is incomplete — fail fast with
    /// `DESTROY_PARTIAL_INIT`. Includes raw flags so the localized message
    /// can describe what is missing.
    PartiallyInitialized {
        has_dir: bool,
        has_instance_id: bool,
        has_config: bool,
    },
}

pub(crate) fn destroy_eligibility(
    state: &workhive_core::domain::ProjectInitState,
) -> DestroyEligibility {
    use workhive_core::domain::ProjectInitState;
    match state {
        ProjectInitState::Initialized { .. } => DestroyEligibility::Proceed,
        ProjectInitState::Uninitialized => DestroyEligibility::NoWorkhiveState,
        ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => DestroyEligibility::PartiallyInitialized {
            has_dir: *has_dir,
            has_instance_id: *has_instance_id,
            has_config: *has_config,
        },
    }
}

pub(crate) fn cmd_destroy(
    delete_branch: bool,
    keep_branch: bool,
    _original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::WorktreeRemovePolicy;
    use workhive_core::usecases::worktree_safety::{
        DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, build_worktree_safety_report,
    };

    let locale = ctx.locale();
    let (root, name) = detect_project()?;
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    // Phase 1: project-state preflight (handler-managed).
    //
    // `workspace destroy` MUST NOT trigger project initialization or create
    // `.workhive/config.toml`, and the destructive warning MUST NOT fire
    // before we know whether destruction even applies. We therefore inspect
    // project state inline — never via `enforce_or_delegate_project_policy`,
    // which is allowed to prompt-to-init for other commands.
    let init_state = usecases::project_state::resolve_init_state(&ws_fs, root.as_ref());
    match destroy_eligibility(&init_state) {
        DestroyEligibility::NoWorkhiveState => {
            // Nothing to destroy and nothing to initialize. Fail fast with
            // a clear localized message, before any destructive prompt.
            return Err(CliError::Runtime(
                ctx.i18n.resolve(locale, k::DESTROY_NOT_INITIALIZED),
            ));
        }
        DestroyEligibility::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => {
            let detail =
                format!("dir={has_dir}, instance_id={has_instance_id}, config={has_config}");
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::DESTROY_PARTIAL_INIT,
                &[&detail],
            )));
        }
        DestroyEligibility::Proceed => {
            // OK to proceed — project state is fully present and we are
            // about to compute the safety report and ask for confirmation.
        }
    }

    // Phase 2: worktree safety analysis (read-only).
    let workhive_wt_base = ws_fs.workhive_dir_path(root.as_ref()).join("worktrees");
    let worktrees = git.list_worktrees()?;
    let base_branch = infer_base_branch(&worktrees);
    let scan_targets = linked_worktree_scan_targets(&worktrees, workhive_wt_base.as_ref());

    // Build the unified managed worktree inventory: paths from scan_targets
    // are the authoritative set for both safety analysis and deletion.
    let managed_worktree_paths: Vec<Utf8PathBuf> =
        scan_targets.iter().map(|t| t.path.clone()).collect();

    let safety_report = build_worktree_safety_report(
        &git,
        &scan_targets,
        base_branch.as_deref(),
        DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
    )?;
    render_worktree_safety_report(
        &safety_report,
        ctx,
        k::DESTROY_SAFETY_REPORT_HEADING,
        k::DESTROY_SAFETY_NO_RISK,
    );

    // Phase 3: destructive confirmation (handler-managed).
    //
    // The irreversible warning fires only AFTER the safety report has been
    // rendered. Users see what is at risk before they are asked to confirm.
    // When risks exist, the project-wide safety lines are also threaded
    // into the confirmation screen so the changed-file breakdown stays
    // visible even when the modal covers the stderr report above.
    let confirm_context = worktree_safety_confirm_context(
        &safety_report,
        &ctx.i18n,
        locale,
        k::DESTROY_SAFETY_REPORT_HEADING,
    );

    eprintln!("{}", ctx.i18n.resolve(locale, k::DESTROY_WARNING));
    let confirm_msg = ctx.i18n.resolve(locale, k::CONFIRM_DESTROY);
    match &confirm_context {
        Some(context) => confirm::require_confirm_with_context(&confirm_msg, context, pol.force)?,
        None => confirm::require_confirm(&confirm_msg, pol.force)?,
    }

    if safety_report.has_risks() {
        if pol.force {
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::DESTROY_FORCE_DESPITE_RISK)
            );
        } else {
            let risky_msg = ctx.i18n.resolve(locale, k::DESTROY_RISKY_WORKTREES_CONFIRM);
            match &confirm_context {
                Some(context) => confirm::require_confirm_with_context(&risky_msg, context, false)?,
                None => confirm::require_confirm(&risky_msg, false)?,
            }
        }
    }

    // Resolve which backing branches to delete BEFORE destruction, while
    // the provenance store under `.workhive/` is still readable. Workhive-
    // created branches are deleted by default; user-owned branches are kept
    // unless explicitly authorized or interactively confirmed (default No).
    // The actual `git branch -d` runs after the worktrees are removed.
    let branches_to_delete = resolve_destroy_branch_deletions(
        &scan_targets,
        base_branch.as_deref(),
        delete_branch,
        keep_branch,
        pol.force,
        ctx,
    )?;

    // Phase 4: actual destruction.
    let np = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context(&name, "", &root, &git, &ws_fs);
    let server = np.resolve_server_for_project(&naming_ctx)?;

    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);
    let provisioner = ctx.composition.worktree_provisioner(&git);
    let snapshot = ctx.composition.snapshot_store(root.as_ref());
    let index = ctx.composition.project_index()?;
    let remove_policy = if pol.force || safety_report.has_risks() {
        WorktreeRemovePolicy::Force
    } else {
        WorktreeRemovePolicy::Safe
    };

    usecases::destroy::destroy_project(
        &provisioner,
        &runtime,
        &ws_fs,
        &snapshot,
        &index,
        &root,
        &name,
        &server,
        remove_policy,
        &managed_worktree_paths,
    )?;

    // `workspace destroy` must clean up any control-plane supervisor socket
    // left behind by a running supervisor. The supervisor's session-liveness
    // probe will shut it down on its own once the tmux server / sessions are
    // gone, but we still remove the socket file explicitly so a stale UDS
    // never lingers in `.workhive/runtime/` — `destroy` is the user's
    // explicit "wipe everything" intent.
    let socket = workhive_infra::control_plane::control_plane_socket_path(root.as_ref(), &server);
    if let Err(e) = workhive_infra::control_plane::remove_stale_socket(&socket) {
        tracing::debug!(
            socket = %socket,
            error = %e,
            "control-plane: best-effort socket cleanup during destroy failed"
        );
    }
    // Wipe the durable launch-identity store for this group as well.
    // `destroy` is the explicit "tear it all down" path, and a stale
    // identity file would otherwise survive a re-init of the project.
    {
        use workhive_core::ports::PaneLaunchIdentityPort;
        let identity_store = ctx.composition.pane_launch_identity_store(root.as_ref());
        if let Err(e) = identity_store.forget_group(&server) {
            tracing::debug!(
                server = %server,
                error = %e,
                "control-plane: best-effort durable identity cleanup during destroy failed"
            );
        }
    }

    // Backing-branch cleanup runs after worktree removal (a branch cannot
    // be deleted while checked out in a worktree). Safe deletion only
    // (`git branch -d`); a failure keeps the branch and reports why.
    for branch in &branches_to_delete {
        match git.delete_branch(branch) {
            Ok(()) => {
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::DESTROY_BRANCH_DELETED, &[branch])
                );
            }
            Err(e) => {
                let reason = e.to_string();
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::DESTROY_BRANCH_KEPT, &[branch, &reason])
                );
            }
        }
    }

    eprintln!("{}", ctx.i18n.resolve(locale, k::DESTROY_SUCCESS));
    Ok(())
}

/// Resolve which managed-worktree backing branches `workspace destroy`
/// should delete, applying provenance policy and (when interactive)
/// per-branch confirmation prompts.
///
/// Reads the provenance store before destruction removes `.workhive/`.
/// Main/root branches are never offered: managed worktree branches are
/// non-main by construction, and the base branch is skipped defensively.
fn resolve_destroy_branch_deletions(
    scan_targets: &[workhive_core::usecases::worktree_safety::WorktreeScanTarget],
    base_branch: Option<&str>,
    delete_branch: bool,
    keep_branch: bool,
    assume_yes: bool,
    ctx: &CommandContext,
) -> Result<Vec<String>, CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::WorktreeProvenancePort;
    use workhive_core::usecases::branch_provenance_policy::{
        BranchCleanupAction, resolve_branch_cleanup_action,
    };

    let locale = ctx.locale();
    let (root, _name) = detect_project()?;
    let provenance_store = ctx.composition.worktree_provenance(root.as_ref());
    let interactive = std::io::stdin().is_terminal() && !assume_yes;

    let mut to_delete = Vec::new();
    for target in scan_targets {
        let branch = match target.branch.as_deref() {
            Some(b) => b,
            None => continue,
        };
        // Never delete the base/root branch.
        if base_branch == Some(branch) {
            continue;
        }
        let provenance = provenance_store.lookup(&target.workspace);
        let action =
            resolve_branch_cleanup_action(provenance, keep_branch, delete_branch, interactive);
        let should_delete = match action {
            BranchCleanupAction::Delete => true,
            BranchCleanupAction::Keep => false,
            BranchCleanupAction::PromptDelete => {
                let msg =
                    ctx.i18n
                        .resolve_with_args(locale, k::DESTROY_CONFIRM_DELETE_BRANCH, &[branch]);
                crate::commands::confirm_with_transcript(ctx, locale, &msg, false)?
            }
        };
        if should_delete {
            to_delete.push(branch.to_owned());
        }
    }
    Ok(to_delete)
}

/// Inputs for [`execute_remove_worktree_core`].
///
/// Both `workspace remove` and `worktree remove` must reach the same
/// destructive code path so that runtime/session cleanup, worktree
/// removal, and branch deletion happen with consistent semantics.
pub(crate) struct ExecuteRemoveWorktreeInput<'a> {
    /// Repository / project root.
    pub project_root: &'a camino::Utf8Path,
    /// Project name as resolved by [`detect_project`].
    pub project_name: &'a str,
    /// Worktree selector — branch or directory name. Also used as the
    /// provenance store key (managed worktree dir name == branch name by
    /// construction).
    pub workspace_name: &'a str,
    /// Backing branch name, when known, used for the branch-delete prompt.
    pub branch: Option<String>,
    /// Workhive-managed worktree base, e.g. `<root>/.workhive/worktrees`.
    pub managed_base: Utf8PathBuf,
    /// Optional base branch used for unmerged-commit analysis inside core.
    pub base_branch: Option<String>,
    /// Whether destructive removal should bypass the safety block.
    pub force: bool,
    /// User authorization to delete a user-owned backing branch
    /// (pre-existing/attached/unknown provenance). `--delete-branch`.
    pub delete_branch: bool,
    /// User request to keep the backing branch even when Workhive created
    /// it. `--keep-branch`. Takes precedence over `delete_branch`.
    pub keep_branch: bool,
    /// Suppress the interactive branch-delete prompt (e.g. `--force` /
    /// non-interactive). When set, user-owned branches are kept unless
    /// `delete_branch` explicitly authorizes deletion.
    pub assume_yes: bool,
}

/// Resolve naming + invoke the core `remove_worktree` use case.
///
/// All localized output and confirm flows stay in the calling command;
/// this helper only centralizes the destructive plumbing so behavior
/// stays identical across `workspace remove` and `worktree remove`.
pub(crate) fn execute_remove_worktree_core(
    input: &ExecuteRemoveWorktreeInput<'_>,
    ctx: &CommandContext,
) -> Result<workhive_core::usecases::remove_worktree::RemoveWorktreeOutcome, CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::WorktreeProvenancePort;
    use workhive_core::usecases::branch_provenance_policy::{
        BranchCleanupAction, resolve_branch_cleanup_action,
    };
    use workhive_core::usecases::remove_worktree::{
        RemoveWorktreeError, RemoveWorktreeOutcome, RemoveWorktreeRequest,
        remove_worktree as core_remove_worktree,
    };
    use workhive_core::usecases::worktree_safety::DEFAULT_CHANGED_FILE_DISPLAY_LIMIT;

    let locale = ctx.locale();
    let np = ctx.naming_policy_for(Some(input.project_root))?;
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();
    let provenance_store = ctx.composition.worktree_provenance(input.project_root);

    let project_root_buf = input.project_root.to_path_buf();
    let naming_ctx = build_naming_context(
        input.project_name,
        input.workspace_name,
        &project_root_buf,
        &git,
        &ws_fs,
    );
    let server = np.resolve_server_for_project(&naming_ctx)?;
    let session = np.resolve_session(&naming_ctx)?;

    // Resolve the branch-cleanup decision from stored provenance plus
    // explicit user intent. A user-owned branch in an interactive terminal
    // is prompted (default No); never deleted silently.
    let provenance = provenance_store.lookup(input.workspace_name);
    let interactive = std::io::stdin().is_terminal() && !input.assume_yes;
    let delete_branch = match resolve_branch_cleanup_action(
        provenance,
        input.keep_branch,
        input.delete_branch,
        interactive,
    ) {
        BranchCleanupAction::Delete => true,
        BranchCleanupAction::Keep => false,
        BranchCleanupAction::PromptDelete => match input.branch.as_deref() {
            Some(branch) => {
                let msg = ctx.i18n.resolve_with_args(
                    locale,
                    k::WORKSPACE_REMOVE_CONFIRM_DELETE_BRANCH,
                    &[branch],
                );
                crate::commands::confirm_with_transcript(ctx, locale, &msg, false)?
            }
            None => false,
        },
    };

    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);
    let provisioner = ctx.composition.worktree_provisioner(&git);

    let mut req = RemoveWorktreeRequest::new(
        input.workspace_name,
        &server,
        &session,
        input.managed_base.clone(),
    );
    req.delete_branch = delete_branch;
    req.base_branch = input.base_branch.clone();
    req.changed_file_display_limit = DEFAULT_CHANGED_FILE_DISPLAY_LIMIT;
    req.force = input.force;

    let outcome =
        core_remove_worktree(&git, &provisioner, &runtime, &req).map_err(|err| match err {
            RemoveWorktreeError::Runtime(e) => CliError::RuntimeOp(e),
            RemoveWorktreeError::Provision(e) => CliError::Provision(e),
            RemoveWorktreeError::Git(e) => CliError::Git(e),
        })?;

    // Once the managed worktree is gone (with or without branch deletion),
    // its provenance record is orphaned. Forget it so a future open of the
    // same name resolves conservatively. Failures here are non-fatal.
    if matches!(
        outcome,
        RemoveWorktreeOutcome::Removed { .. } | RemoveWorktreeOutcome::BranchDeleteFailed { .. }
    ) && let Err(e) = provenance_store.forget(input.workspace_name)
    {
        tracing::warn!(
            workspace = %input.workspace_name,
            error = %e,
            "failed to forget worktree provenance after removal"
        );
    }

    Ok(outcome)
}

pub(crate) fn cmd_workspace_remove(
    workspace: &str,
    delete_branch: bool,
    keep_branch: bool,
    original_command: &str,
    pol: &policy::CommandPolicy,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::ProvisionError;
    use workhive_core::usecases::remove_worktree::RemoveWorktreeOutcome;
    use workhive_core::usecases::worktree_safety::{
        DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, WorktreeScanTarget, build_worktree_safety_report,
    };

    let locale = ctx.locale();
    let (root, project_name) = detect_project()?;
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let _ = enforce_or_delegate_project_policy(pol, root.as_ref(), original_command, &ws_fs, ctx)?;

    let workhive_wt_base = ws_fs.workhive_dir_path(root.as_ref()).join("worktrees");
    let worktrees = git.list_worktrees()?;
    let target = find_worktree(&worktrees, workspace).ok_or_else(|| {
        CliError::Provision(ProvisionError::NotFound {
            path: workhive_wt_base.join(workspace),
        })
    })?;
    if target.is_main {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_MAIN_FORBIDDEN),
        ));
    }
    // Component-aware: rejects both lookalike paths (e.g.
    // `.workhive/worktrees-archive/`) and the managed base directory
    // itself.
    if !path_is_under_base(target.path.as_path(), workhive_wt_base.as_path()) {
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::WORKSPACE_REMOVE_NOT_MANAGED,
            &[workspace],
        )));
    }

    let base_branch = infer_base_branch(&worktrees);
    let preview_report = build_worktree_safety_report(
        &git,
        &[WorktreeScanTarget {
            workspace: workspace.to_owned(),
            branch: target.branch.clone(),
            path: target.path.clone(),
        }],
        base_branch.as_deref(),
        DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
    )?;
    render_worktree_safety_report(
        &preview_report,
        ctx,
        k::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
        k::DESTROY_SAFETY_NO_RISK,
    );

    // Keep the changed-file safety summary visible at the decision
    // point: the modal confirmation can cover the stderr report above,
    // so when risks exist the same localized lines are threaded into
    // the confirmation screen. A clean removal has `None` here and
    // keeps the original compact prompt.
    let confirm_context = worktree_safety_confirm_context(
        &preview_report,
        &ctx.i18n,
        locale,
        k::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
    );

    let confirm_msg = ctx
        .i18n
        .resolve_with_args(locale, k::WORKSPACE_REMOVE_CONFIRM, &[workspace]);
    match &confirm_context {
        Some(context) => confirm::require_confirm_with_context(&confirm_msg, context, pol.force)?,
        None => confirm::require_confirm(&confirm_msg, pol.force)?,
    }
    if preview_report.has_risks() {
        if pol.force {
            eprintln!(
                "{}",
                ctx.i18n.resolve(locale, k::DESTROY_FORCE_DESPITE_RISK)
            );
        } else {
            let risky_msg = ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_RISKY_CONFIRM);
            match &confirm_context {
                Some(context) => confirm::require_confirm_with_context(&risky_msg, context, false)?,
                None => confirm::require_confirm(&risky_msg, false)?,
            }
        }
    }

    let outcome = execute_remove_worktree_core(
        &ExecuteRemoveWorktreeInput {
            project_root: root.as_ref(),
            project_name: &project_name,
            workspace_name: workspace,
            branch: target.branch.clone(),
            managed_base: workhive_wt_base.clone(),
            base_branch,
            force: pol.force || preview_report.has_risks(),
            delete_branch,
            keep_branch,
            assume_yes: pol.force,
        },
        ctx,
    )?;
    match outcome {
        RemoveWorktreeOutcome::Removed {
            deleted_branch,
            removed_session,
            ..
        } => {
            let removed_session_str = removed_session.to_string();
            let deleted_branch_str = deleted_branch.to_string();
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::WORKSPACE_REMOVE_SUCCESS,
                    &[workspace, &removed_session_str, &deleted_branch_str],
                )
            );
            Ok(())
        }
        RemoveWorktreeOutcome::BranchDeleteFailed { branch, reason, .. } => {
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::WORKSPACE_REMOVE_BRANCH_KEPT,
                    &[workspace, &branch, &reason],
                )
            );
            Ok(())
        }
        RemoveWorktreeOutcome::NoWorktreeFound { worktree } => {
            Err(CliError::Provision(ProvisionError::NotFound {
                path: root.join(".workhive").join("worktrees").join(worktree),
            }))
        }
        RemoveWorktreeOutcome::MainWorktreeForbidden { .. } => Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_MAIN_FORBIDDEN),
        )),
        RemoveWorktreeOutcome::NotWorkhiveManaged { worktree, .. } => Err(CliError::Runtime(
            ctx.i18n
                .resolve_with_args(locale, k::WORKSPACE_REMOVE_NOT_MANAGED, &[&worktree]),
        )),
        RemoveWorktreeOutcome::SafetyBlocked { report } => {
            render_worktree_safety_report(
                &report,
                ctx,
                k::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
                k::DESTROY_SAFETY_NO_RISK,
            );
            Err(CliError::ConfirmationRequired(
                ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_SAFETY_BLOCKED),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeInfo};
    #[allow(unused_imports)]
    use workhive_core::usecases::worktree_safety::{
        UnmergedCommitSummary, WorktreeRisk, WorktreeSafetyReport,
    };

    // ── workspace destroy preflight order ──────────────────────────────
    //
    // The pure `destroy_eligibility` function decides whether destroy may
    // proceed. Anything that depends on it (the destructive warning, the
    // safety scan, the runtime cleanup) MUST observe these outcomes.

    #[test]
    fn destroy_eligibility_uninitialized_blocks_destruction() {
        let state = workhive_core::domain::ProjectInitState::Uninitialized;
        assert_eq!(
            destroy_eligibility(&state),
            DestroyEligibility::NoWorkhiveState,
            "missing `.workhive/` must short-circuit destroy with a localized \
             `DESTROY_NOT_INITIALIZED` message — never a destructive prompt, \
             never a project-init prompt"
        );
    }

    #[test]
    fn destroy_eligibility_partial_carries_diagnostic_flags() {
        let state = workhive_core::domain::ProjectInitState::PartiallyInitialized {
            has_dir: true,
            has_instance_id: false,
            has_config: true,
        };
        assert_eq!(
            destroy_eligibility(&state),
            DestroyEligibility::PartiallyInitialized {
                has_dir: true,
                has_instance_id: false,
                has_config: true,
            },
            "partial init must surface the missing pieces so the localized \
             `DESTROY_PARTIAL_INIT` message can describe the broken state"
        );
    }

    #[test]
    fn destroy_eligibility_initialized_proceeds() {
        let state = workhive_core::domain::ProjectInitState::Initialized {
            instance_id: "abcd1234".to_owned(),
        };
        assert_eq!(
            destroy_eligibility(&state),
            DestroyEligibility::Proceed,
            "an initialized project must reach the safety scan + destructive \
             confirm in `cmd_destroy`"
        );
    }

    /// Pins the runtime-only destroy contract: a project that has
    /// `.workhive/` plus `instance_id` but does NOT have
    /// `.workhive/config.toml` MUST be classified `Initialized` so
    /// `workspace destroy` can enter the destructive flow without ever
    /// prompting to create project config and without writing one.
    ///
    /// `workspace open` produces this runtime-only shape: it creates
    /// `.workhive/` + `instance_id` but never writes a project
    /// `config.toml`. The destroy preflight chain (`resolve_init_state`
    /// → `destroy_eligibility`) must accept that shape without demanding
    /// a config file. We assert the post-condition
    /// `!project_config_exists()` after the call so any refactor that
    /// silently "fixes up" the missing config inside the preflight
    /// fails this test.
    #[test]
    fn destroy_eligibility_runtime_only_init_proceeds_without_creating_config() {
        use workhive_core::domain::ProjectInitState;
        use workhive_core::usecases::project_state;
        use workhive_testkit::fakes::FakeWorkspaceFs;

        let ws_fs = FakeWorkspaceFs::new();
        let root = Utf8PathBuf::from("/tmp/destroy-runtime-only");

        // Set up the runtime-only state on the fake FS:
        //   .workhive/             -> exists (created via WorkspaceFsPort)
        //   .workhive/instance_id  -> present
        //   .workhive/config.toml  -> ABSENT (never call `mark_config_written`)
        ws_fs.create_workhive_dir(root.as_ref()).unwrap();
        ws_fs.write_instance_id(root.as_ref(), "abcd1234").unwrap();
        assert!(
            !ws_fs.project_config_exists(root.as_ref()),
            "precondition: project config.toml MUST be absent — this is \
             the runtime-only init shape produced by `workspace open`"
        );

        // Phase 1: project state must classify as `Initialized` (not
        // `PartiallyInitialized`). Otherwise `cmd_destroy` would error
        // with `DESTROY_PARTIAL_INIT` on a perfectly valid runtime-only
        // project.
        let state = project_state::resolve_init_state(&ws_fs, root.as_ref());
        match &state {
            ProjectInitState::Initialized { instance_id } => {
                assert_eq!(instance_id, "abcd1234");
            }
            other => panic!(
                "runtime-only init (.workhive + instance_id, no config.toml) \
                 must classify as Initialized; got {other:?}"
            ),
        }

        // Phase 2: destroy preflight must say `Proceed`, which is what
        // lets `cmd_destroy` advance to the safety scan + destructive
        // prompt. This is the exact decision point this test pins.
        assert_eq!(
            destroy_eligibility(&state),
            DestroyEligibility::Proceed,
            "runtime-only init must enter the destroy flow without \
             `DESTROY_PARTIAL_INIT` and without prompting to create config"
        );

        // Phase 3: neither `resolve_init_state` nor `destroy_eligibility`
        // is allowed to CREATE project config — writing a config from
        // inside the destroy preflight is banned.
        assert!(
            !ws_fs.project_config_exists(root.as_ref()),
            "destroy preflight must NOT create project config.toml"
        );
    }

    // ── workspace destroy dirty-worktree warning ───────────────────────
    //
    // `workspace destroy` must surface a dirty-worktree warning when a
    // Workhive-managed linked worktree has uncommitted or untracked
    // changes. The default-running tests below pin the full detection
    // chain that the destroy command depends on:
    //
    //   `git list_worktrees` → `linked_worktree_scan_targets`
    //     → `build_worktree_safety_report` → `WorktreeSafetyReport::has_risks()`
    //
    // Real `tmux` + real `git` end-to-end coverage stays in the gated
    // `#[ignore]` tests `e2e_destroy_dirty_noninteractive_refuses_without_force`
    // and `e2e_destroy_force_reports_risky_worktrees_and_completes`
    // (`crates/workhive-cli/tests/cli_integration.rs`), which can only run in
    // environments with a real tmux server and git binary; default unit and
    // integration tests must not depend on real tmux or git, so those stay
    // out of the default `cargo test` matrix.

    fn workhive_managed_dirty_worktree_setup() -> (
        Utf8PathBuf,
        Vec<WorktreeInfo>,
        workhive_testkit::fakes::FakeGit,
    ) {
        use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeStatus};
        use workhive_testkit::fakes::FakeGit;

        let project_root = Utf8PathBuf::from("/project");
        let workhive_base = project_root.join(".workhive").join("worktrees");
        let dirty_path = workhive_base.join("feat-dirty");

        let worktrees = vec![
            WorktreeInfo {
                path: project_root.clone(),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: dirty_path.clone(),
                branch: Some("feat-dirty".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];

        let git = FakeGit::new(project_root);
        // Single untracked file in the managed worktree — the canonical
        // dirty shape: `workhive workspace open` followed by a new local
        // file inside `.workhive/worktrees/<name>/`.
        git.set_worktree_status(
            dirty_path,
            WorktreeStatus {
                changed_files: vec![ChangedFile {
                    status_code: "??".to_owned(),
                    path: "dirty.txt".to_owned(),
                    kinds: vec![ChangeKind::Untracked],
                }],
            },
        );

        (workhive_base, worktrees, git)
    }

    #[test]
    fn destroy_safety_chain_flags_managed_dirty_worktree() {
        use workhive_core::usecases::worktree_safety::{
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, build_worktree_safety_report,
        };

        let (workhive_base, worktrees, git) = workhive_managed_dirty_worktree_setup();

        let targets = linked_worktree_scan_targets(&worktrees, workhive_base.as_ref());
        assert_eq!(
            targets.len(),
            1,
            "exactly the dirty managed worktree must be scanned (main repo \
             excluded, no external worktrees in this fixture)"
        );

        let report = build_worktree_safety_report(
            &git,
            &targets,
            Some("main"),
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
        )
        .expect("safety report must build for a single dirty target");

        assert!(
            report.has_risks(),
            "an untracked file in a Workhive-managed linked worktree MUST \
             flip `has_risks()` so `cmd_destroy` raises the dirty-worktree \
             warning instead of silently proceeding"
        );
        let risk = &report.risky_worktrees[0];
        assert_eq!(risk.workspace, "feat-dirty");
        assert_eq!(risk.untracked_count, 1);
        assert_eq!(risk.staged_count, 0);
        assert_eq!(risk.unstaged_count, 0);
        assert_eq!(risk.total_changed_files, 1);
    }

    #[test]
    fn destroy_safety_chain_excludes_external_worktrees_from_warning() {
        use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeStatus};
        use workhive_core::usecases::worktree_safety::{
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, build_worktree_safety_report,
        };
        use workhive_testkit::fakes::FakeGit;

        let project_root = Utf8PathBuf::from("/project");
        let workhive_base = project_root.join(".workhive").join("worktrees");

        // External worktree carries dirty state, but `destroy` MUST NOT
        // surface or remove it because it is not under
        // `.workhive/worktrees/`. This guards against accidentally
        // expanding destroy scope.
        let external_dirty = Utf8PathBuf::from("/external/repos/feat-ext");
        let worktrees = vec![
            WorktreeInfo {
                path: project_root.clone(),
                branch: Some("main".to_owned()),
                is_main: true,
                resolved_strategy: None,
            },
            WorktreeInfo {
                path: external_dirty.clone(),
                branch: Some("feat-ext".to_owned()),
                is_main: false,
                resolved_strategy: None,
            },
        ];

        let git = FakeGit::new(project_root);
        git.set_worktree_status(
            external_dirty,
            WorktreeStatus {
                changed_files: vec![ChangedFile {
                    status_code: "??".to_owned(),
                    path: "leaked.txt".to_owned(),
                    kinds: vec![ChangeKind::Untracked],
                }],
            },
        );

        let targets = linked_worktree_scan_targets(&worktrees, workhive_base.as_ref());
        assert!(
            targets.is_empty(),
            "external worktrees outside `.workhive/worktrees/` must NEVER \
             enter the destroy safety scan, even when dirty"
        );

        let report = build_worktree_safety_report(
            &git,
            &targets,
            Some("main"),
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
        )
        .expect("safety report must build for an empty target list");
        assert!(
            !report.has_risks(),
            "no managed-worktree risk → destroy must skip the dirty warning"
        );
    }

    #[test]
    fn destroy_safety_warning_copy_includes_dirty_worktree_in_report() {
        use workhive_core::usecases::worktree_safety::{
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, build_worktree_safety_report,
        };
        let i18n = StaticI18n::new();

        let (workhive_base, worktrees, git) = workhive_managed_dirty_worktree_setup();
        let targets = linked_worktree_scan_targets(&worktrees, workhive_base.as_ref());
        let report = build_worktree_safety_report(
            &git,
            &targets,
            Some("main"),
            DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
        )
        .expect("safety report must build");

        // Render exactly what `cmd_destroy` renders before the destructive
        // confirmation — proves that English and Chinese users both see a
        // visible warning that mentions the dirty workspace name.
        for (locale, expected_marker) in [
            (Locale::En, "Destroy safety report:"),
            (Locale::ZhCn, "销毁安全报告："),
        ] {
            let rendered = format_worktree_safety_report_lines(
                &report,
                &i18n,
                locale,
                workhive_core::i18n_keys::DESTROY_SAFETY_REPORT_HEADING,
                workhive_core::i18n_keys::DESTROY_SAFETY_NO_RISK,
                SafetyStyleMode::Plain,
            )
            .join("\n");

            assert!(
                rendered.contains(expected_marker),
                "destroy heading missing in {locale:?} render: {rendered}"
            );
            assert!(
                rendered.contains("feat-dirty"),
                "the dirty managed workspace name MUST appear in the {locale:?} \
                 destroy warning so the user can identify what is at risk: \
                 {rendered}"
            );
        }
    }

    /// The destroy preflight is a discriminated union with no fallthrough:
    /// every variant of `ProjectInitState` MUST be classified explicitly so
    /// future state additions cannot accidentally fall into "Proceed".
    #[test]
    fn destroy_eligibility_is_total_over_init_states() {
        // Add every existing variant. If a new ProjectInitState is added,
        // the match in `destroy_eligibility` must be updated and this test
        // must be extended in the same commit.
        let states = [
            workhive_core::domain::ProjectInitState::Initialized {
                instance_id: "x".to_owned(),
            },
            workhive_core::domain::ProjectInitState::Uninitialized,
            workhive_core::domain::ProjectInitState::PartiallyInitialized {
                has_dir: false,
                has_instance_id: false,
                has_config: false,
            },
        ];
        for state in &states {
            // Just classify — the per-variant assertions above pin the
            // exact outcomes; here we only prove totality.
            let _ = destroy_eligibility(state);
        }
    }
}

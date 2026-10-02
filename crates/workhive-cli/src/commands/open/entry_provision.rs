//! Pre-plan phases of `workspace open`: managed-branch provenance
//! resolution (including unborn-repo repair) and the project
//! init-decision gate.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

use workhive_core::domain::ProjectInitDecision;
use workhive_core::i18n_keys as k;
use workhive_core::ports::{BranchProvenance, GitPort, WorkspaceFsPort, WorktreeRemovePolicy};
use workhive_core::usecases::branch_provenance_policy::{
    OpenBranchAction, resolve_open_branch_action,
};

/// Resolve how this `open` acquires the workspace branch when the open
/// intent is `Provision`, repairing unborn-repo state first when needed.
///
/// Returns the branch provenance to record after provisioning succeeds;
/// `None` means "leave the stored provenance untouched" (managed-worktree
/// reuse path).
pub(crate) fn resolve_provision_branch_provenance(
    git: &impl GitPort,
    ws_fs: &impl WorkspaceFsPort,
    ctx: &CommandContext,
    root: &camino::Utf8Path,
    workspace_name: &str,
    attach_existing_branch: bool,
) -> Result<Option<BranchProvenance>, CliError> {
    use std::io::IsTerminal;
    let locale = ctx.locale();

    let managed_base = ws_fs.workhive_dir_path(root).join("worktrees");
    let mut worktrees = git.list_worktrees()?;
    let managed_worktree = worktrees.iter().find(|wt| {
        !wt.is_main
            && wt.branch.as_deref() == Some(workspace_name)
            && wt.path.starts_with(&managed_base)
    });
    let mut managed_worktree_exists = managed_worktree.is_some();
    let branch_exists = git.branch_exists(workspace_name)?;
    if !git.has_commits()? {
        let interactive = std::io::stdin().is_terminal();
        if interactive {
            if let Some(existing) = managed_worktree {
                let status = git.worktree_status(existing.path.as_ref())?;
                let changed_files = status.total_changed_files();
                if changed_files > 0 {
                    return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                        locale,
                        k::OPEN_UNBORN_REPO_MANAGED_WORKTREE_DIRTY,
                        &[workspace_name, &changed_files.to_string()],
                    )));
                }
                let msg = ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_UNBORN_REPO_REPAIR_MANAGED_WORKTREE_PROMPT,
                    &[workspace_name],
                );
                let confirmed = crate::commands::confirm_with_transcript(ctx, locale, &msg, true)?;
                if !confirmed {
                    return Err(CliError::UserAborted);
                }
                git.remove_worktree(existing.path.as_ref(), WorktreeRemovePolicy::Safe)?;
                worktrees = git.list_worktrees()?;
                managed_worktree_exists = worktrees.iter().any(|wt| {
                    !wt.is_main
                        && wt.branch.as_deref() == Some(workspace_name)
                        && wt.path.starts_with(&managed_base)
                });
            } else {
                let msg = ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_UNBORN_REPO_CREATE_INITIAL_COMMIT_PROMPT,
                    &[workspace_name],
                );
                let confirmed = crate::commands::confirm_with_transcript(ctx, locale, &msg, true)?;
                if !confirmed {
                    return Err(CliError::UserAborted);
                }
            }
            git.create_empty_initial_commit("Initial commit")?;
        } else {
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_UNBORN_REPO_CREATE_WORKTREE_BLOCKED,
                &[workspace_name],
            )));
        }
    }

    let interactive = std::io::stdin().is_terminal();
    match resolve_open_branch_action(
        managed_worktree_exists,
        branch_exists,
        attach_existing_branch,
        interactive,
    ) {
        OpenBranchAction::ReuseManagedWorktree => {
            // Reuse/open the managed worktree; provenance is left as-is.
            Ok(None)
        }
        OpenBranchAction::CreateNewBranch => {
            if interactive {
                let msg = ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_CONFIRM_CREATE_WORKTREE,
                    &[workspace_name],
                );
                let confirmed = crate::commands::confirm_with_transcript(ctx, locale, &msg, true)?;
                if !confirmed {
                    return Err(CliError::UserAborted);
                }
            }
            Ok(Some(BranchProvenance::WorkhiveCreated))
        }
        OpenBranchAction::AttachExistingAuthorized => {
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve_with_args(locale, k::OPEN_BRANCH_ATTACHED, &[workspace_name])
            );
            Ok(Some(BranchProvenance::PreExistingAttached))
        }
        OpenBranchAction::PromptAttachExisting => {
            let msg = ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_CONFIRM_ATTACH_EXISTING_BRANCH,
                &[workspace_name],
            );
            let confirmed = crate::commands::confirm_with_transcript(ctx, locale, &msg, false)?;
            if !confirmed {
                return Err(CliError::UserAborted);
            }
            Ok(Some(BranchProvenance::PreExistingAttached))
        }
        OpenBranchAction::BlockedNeedsAuthorization => Err(CliError::Runtime(
            ctx.i18n
                .resolve_with_args(locale, k::OPEN_ATTACH_EXISTING_BLOCKED, &[workspace_name]),
        )),
    }
}

/// Resolve the project init decision for `workspace open`, prompt for
/// confirmation when allowed, and run the idempotent runtime-only init.
///
/// Returns whether this open auto-initialized the project.
pub(crate) fn resolve_project_init_and_index(
    ctx: &CommandContext,
    ws_fs: &impl WorkspaceFsPort,
    root: &camino::Utf8PathBuf,
    name: &str,
    init_flag: bool,
) -> Result<bool, CliError> {
    use std::io::IsTerminal;
    let locale = ctx.locale();

    let is_interactive = std::io::stdin().is_terminal();
    let mut init_decision =
        policy::resolve_project_init_decision(root, ws_fs, is_interactive, init_flag).map_err(
            |e| match &e {
                workhive_core::errors::ProjectInitError::PartiallyInitialized {
                    root: r,
                    has_dir,
                    has_instance_id,
                    has_config,
                } => CliError::Runtime(ctx.i18n.resolve_with_args(
                    locale,
                    k::OPEN_PROJECT_PARTIAL_INIT,
                    &[
                        r.as_str(),
                        &has_dir.to_string(),
                        &has_instance_id.to_string(),
                        &has_config.to_string(),
                    ],
                )),
                _ => CliError::from(e),
            },
        )?;

    if init_decision == ProjectInitDecision::NeedsConfirmation {
        let msg = ctx.i18n.resolve(locale, k::OPEN_PROJECT_INIT_PROMPT);
        let confirmed = crate::commands::confirm_with_transcript(ctx, locale, &msg, true)?;
        if confirmed {
            init_decision = ProjectInitDecision::ApprovedAutoInit;
            tracing::info!(project = %name, "user confirmed project initialization");
        } else {
            tracing::info!(project = %name, "user declined project initialization");
            return Err(CliError::Runtime(
                ctx.i18n.resolve(locale, k::OPEN_PROJECT_INIT_DECLINED),
            ));
        }
    }

    let index = ctx.composition.project_index()?;
    let lock_adapter = ctx.composition.project_init_lock();
    let was_auto_initialized = match &init_decision {
        ProjectInitDecision::AlreadyInitialized { .. } => {
            // The project is already initialized, but its global index entry
            // can be missing or stale. The idempotent runtime-only init
            // refreshes the index on every successful open: it performs no
            // initialization for an already-initialized project and reports
            // `false`, so `was_auto_initialized` stays accurate while a
            // successful open keeps the project globally discoverable.
            usecases::init::init_project_runtime_only(
                &ctx.config,
                ws_fs,
                &index,
                &lock_adapter,
                root,
                name,
            )?
        }
        ProjectInitDecision::DeniedNonInteractive => {
            return Err(CliError::Runtime(
                ctx.i18n.resolve(locale, k::OPEN_PROJECT_INIT_DENIED),
            ));
        }
        ProjectInitDecision::ApprovedAutoInit => {
            // Trusted-root admission already gated at the top of `cmd_open`,
            // so all mutating sub-flows (auto-init, worktree provisioning,
            // session creation) reuse that single decision instead of
            // re-checking.
            usecases::init::init_project_runtime_only(
                &ctx.config,
                ws_fs,
                &index,
                &lock_adapter,
                root,
                name,
            )?
        }
        ProjectInitDecision::NeedsConfirmation => {
            unreachable!("NeedsConfirmation should have been resolved above")
        }
    };
    Ok(was_auto_initialized)
}

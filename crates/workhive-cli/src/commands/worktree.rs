//! `workhive workspace worktree ...` lists or removes Workhive-managed linked
//! worktrees.
//!
//! Scope is intentionally narrow: this surface only sees worktrees that
//! live under `<project>/.workhive/worktrees/`. External `git worktree
//! add` outputs and the main project root never appear in the list nor
//! in the interactive remove selector. That guarantees the user can
//! never delete a worktree that Workhive did not create.
//!
//! Heavier workspace lifecycle (snapshot/runtime cleanup, dependency
//! aware removal) still lives under `workhive workspace remove`.

use std::io::IsTerminal;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::i18n_keys as k;
use workhive_core::ports::{GitPort, I18nPort, WorktreeInfo};
use workhive_core::usecases::remove_worktree::RemoveWorktreeOutcome;
use workhive_core::usecases::worktree_safety::{
    DEFAULT_CHANGED_FILE_DISPLAY_LIMIT, WorktreeScanTarget, build_worktree_safety_report,
};

use crate::cli::WorktreeAction;
use crate::confirm;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::path_display::display_path_for_humans;
use crate::table::{Column, print_table};
use crate::ui;

use super::project::{
    ExecuteRemoveWorktreeInput, detect_project, execute_remove_worktree_core, find_worktree,
    infer_base_branch, path_is_under_base, render_worktree_safety_report,
    worktree_safety_confirm_context,
};

/// Dispatch entry point for the `worktree` command group.
///
/// `None` action is equivalent to `worktree list` so users can type
/// `workhive workspace worktree` and immediately see the topology Workhive owns.
pub(crate) fn cmd_worktree_dispatch(
    action: Option<WorktreeAction>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    match action {
        None | Some(WorktreeAction::List) => cmd_worktree_list(ctx),
        Some(WorktreeAction::Remove {
            worktree,
            force,
            delete_branch,
            keep_branch,
        }) => cmd_worktree_remove(worktree.as_deref(), force, delete_branch, keep_branch, ctx),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Managed-base resolution
// ─────────────────────────────────────────────────────────────────────────────

/// Compute `<project_root>/.workhive/worktrees`, the only base from which
/// we accept worktrees as Workhive-managed.
fn managed_worktree_base(project_root: &Utf8Path) -> Utf8PathBuf {
    project_root.join(".workhive").join("worktrees")
}

/// Returns true when `path` lies *strictly* under `base` (component-aware).
///
/// We deliberately go through `Utf8Path::strip_prefix` (component
/// matching) instead of plain string prefix comparison so that paths
/// like `/repo/.workhive/worktrees-archive/foo` are *not* treated as
/// managed.
///
/// The base directory itself is not a
/// managed worktree — only entries strictly *inside* it qualify. The
/// implementation is delegated to [`path_is_under_base`] so the CLI and
/// the shared `workspace remove` path stay consistent.
pub(crate) fn is_under_managed_base(path: &Utf8Path, base: &Utf8Path) -> bool {
    path_is_under_base(path, base)
}

/// Filter `worktrees` down to entries that are *both* non-main *and*
/// physically located under the managed base. This is the single source
/// of truth for "what may appear in `worktree list` / the interactive
/// remove selector".
pub(crate) fn filter_managed_worktrees<'a>(
    worktrees: &'a [WorktreeInfo],
    managed_base: &Utf8Path,
) -> Vec<&'a WorktreeInfo> {
    worktrees
        .iter()
        .filter(|w| !w.is_main && is_under_managed_base(w.path.as_path(), managed_base))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// list
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_worktree_list(ctx: &CommandContext) -> Result<(), CliError> {
    let locale = ctx.locale();
    let (project_root, _) = detect_project()?;
    let git = ctx.composition.git();
    let managed_base = managed_worktree_base(project_root.as_path());
    let worktrees = git.list_worktrees()?;
    let managed = filter_managed_worktrees(&worktrees, managed_base.as_path());

    if managed.is_empty() {
        eprintln!("{}", ctx.i18n.resolve(locale, k::WORKTREE_LIST_EMPTY));
        return Ok(());
    }

    let columns = vec![
        Column {
            header: ctx.i18n.resolve(locale, k::WORKTREE_COL_NAME),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::WORKTREE_COL_BRANCH),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::WORKTREE_COL_KIND),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::WORKTREE_COL_PATH),
            min_width: 0,
        },
    ];

    let kind_linked = ctx.i18n.resolve(locale, k::WORKTREE_KIND_LINKED);
    let rows: Vec<Vec<String>> = managed
        .iter()
        .map(|w| {
            let name = worktree_display_name(w);
            let branch = w.branch.clone().unwrap_or_else(|| "-".to_owned());
            let path = display_path_for_humans(w.path.as_path());
            vec![name, branch, kind_linked.clone(), path]
        })
        .collect();

    eprintln!("{}", ctx.i18n.resolve(locale, k::WORKTREE_LIST_HEADER));
    print_table(&columns, &rows);
    Ok(())
}

/// Best-effort label for a worktree row.
///
/// Prefers the directory name (it is what the user types when interacting
/// with `git worktree`), falling back to the branch and finally the path.
pub(crate) fn worktree_display_name(w: &WorktreeInfo) -> String {
    if let Some(name) = w.path.file_name() {
        return name.to_owned();
    }
    if let Some(branch) = &w.branch {
        return branch.clone();
    }
    w.path.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// remove
// ─────────────────────────────────────────────────────────────────────────────

fn cmd_worktree_remove(
    name: Option<&str>,
    force: bool,
    delete_branch: bool,
    keep_branch: bool,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();

    // Resolve the project root + name through the same path as
    // `cmd_workspace_remove`, so naming policy / instance-id /
    // server / session live on the same plumbing.
    let (project_root, project_name) = detect_project()?;
    let git = ctx.composition.git();
    let managed_base = managed_worktree_base(project_root.as_path());
    let worktrees = git.list_worktrees()?;
    let managed = filter_managed_worktrees(&worktrees, managed_base.as_path());

    let target_name = match name {
        Some(n) => n.to_owned(),
        None => {
            if !std::io::stdin().is_terminal() {
                return Err(CliError::Runtime(
                    ctx.i18n.resolve(locale, k::WORKTREE_REMOVE_NO_TTY),
                ));
            }
            if managed.is_empty() {
                return Err(CliError::Runtime(
                    ctx.i18n.resolve(locale, k::WORKTREE_REMOVE_NONE_AVAILABLE),
                ));
            }
            select_worktree_interactively(&managed, ctx)?
        }
    };

    // Resolve the full WorktreeInfo (so we can read branch + path).
    // The "find" helper accepts directory-name OR branch as selector,
    // matching the same UX as `git worktree remove`.
    let target = find_worktree(&worktrees, &target_name).ok_or_else(|| {
        CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::WORKTREE_REMOVE_NOT_FOUND,
            &[&target_name],
        ))
    })?;

    if target.is_main {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_MAIN_FORBIDDEN),
        ));
    }

    // Hard managed-base gate: even if a user typed an exact selector
    // matching an external worktree, refuse and explain. The shared
    // helper below also enforces this in core; we surface a richer
    // localized message at the CLI seam.
    if !is_under_managed_base(target.path.as_path(), managed_base.as_path()) {
        let target_path = display_path_for_humans(target.path.as_path());
        let base = display_path_for_humans(managed_base.as_path());
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::WORKTREE_REMOVE_NOT_MANAGED,
            &[&target_name, &target_path, &base],
        )));
    }

    let base_branch = infer_base_branch(&worktrees);
    let report = build_worktree_safety_report(
        &git as &dyn GitPort,
        &[WorktreeScanTarget {
            workspace: target_name.clone(),
            branch: target.branch.clone(),
            path: target.path.clone(),
        }],
        base_branch.as_deref(),
        DEFAULT_CHANGED_FILE_DISPLAY_LIMIT,
    )?;
    render_worktree_safety_report(
        &report,
        ctx,
        k::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
        k::DESTROY_SAFETY_NO_RISK,
    );

    // Risky worktrees: refuse outright unless `--force`. The two-step
    // confirm is reserved for the *forced* path so the safety wall is
    // always visible to the user.
    if report.has_risks() && !force {
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::WORKTREE_REMOVE_BLOCKED_RISKY,
            &[&target_name],
        )));
    }

    // The risky path only reaches confirmation under `--force` (the
    // non-forced risky case returned above). When risks exist, the
    // safety lines are threaded into the confirmation screen so the
    // changed-file breakdown stays visible at the decision point; a
    // clean removal has `None` and keeps the original compact prompt.
    // `--force` here overrides the risk block but does not skip the
    // prompt, so the confirmation count is unchanged.
    let confirm_context = worktree_safety_confirm_context(
        &report,
        &ctx.i18n,
        locale,
        k::WORKSPACE_REMOVE_SAFETY_REPORT_HEADING,
    );

    let target_path_display = display_path_for_humans(target.path.as_path());
    let confirm_msg = ctx.i18n.resolve_with_args(
        locale,
        k::WORKTREE_REMOVE_CONFIRM,
        &[&target_name, &target_path_display],
    );
    match &confirm_context {
        Some(context) => confirm::require_confirm_with_context(&confirm_msg, context, false)?,
        None => confirm::require_confirm(&confirm_msg, false)?,
    }
    if force && report.has_risks() {
        let risky_msg = ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_RISKY_CONFIRM);
        match &confirm_context {
            Some(context) => confirm::require_confirm_with_context(&risky_msg, context, false)?,
            None => confirm::require_confirm(&risky_msg, false)?,
        }
    }

    let outcome = execute_remove_worktree_core(
        &ExecuteRemoveWorktreeInput {
            project_root: project_root.as_ref(),
            project_name: &project_name,
            workspace_name: &target_name,
            branch: target.branch.clone(),
            managed_base: managed_base.clone(),
            base_branch,
            force: force || report.has_risks(),
            delete_branch,
            keep_branch,
            assume_yes: force,
        },
        ctx,
    )?;
    render_remove_outcome(&outcome, &target_name, &managed_base, ctx)
}

/// Map the shared core outcome to localized output, mirroring the
/// behavior of `cmd_workspace_remove`.
fn render_remove_outcome(
    outcome: &RemoveWorktreeOutcome,
    target_name: &str,
    managed_base: &Utf8Path,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();
    match outcome {
        RemoveWorktreeOutcome::Removed {
            removed_session,
            deleted_branch,
            ..
        } => {
            let removed_session_str = removed_session.to_string();
            let deleted_branch_str = deleted_branch.to_string();
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::WORKSPACE_REMOVE_SUCCESS,
                    &[target_name, &removed_session_str, &deleted_branch_str],
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
                    &[target_name, branch, reason],
                )
            );
            Ok(())
        }
        RemoveWorktreeOutcome::NoWorktreeFound { worktree } => Err(CliError::Runtime(
            ctx.i18n
                .resolve_with_args(locale, k::WORKTREE_REMOVE_NOT_FOUND, &[worktree]),
        )),
        RemoveWorktreeOutcome::MainWorktreeForbidden { .. } => Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::WORKSPACE_REMOVE_MAIN_FORBIDDEN),
        )),
        RemoveWorktreeOutcome::NotWorkhiveManaged { worktree, path, .. } => {
            // Defense in depth: the CLI seam already filters external
            // worktrees, but the core boundary is authoritative. If
            // we reach this branch, surface the same 3-arg message.
            let target_path = display_path_for_humans(path);
            let base = display_path_for_humans(managed_base);
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::WORKTREE_REMOVE_NOT_MANAGED,
                &[worktree, &target_path, &base],
            )))
        }
        RemoveWorktreeOutcome::SafetyBlocked { report } => {
            // The CLI prevents this branch by short-circuiting on
            // `report.has_risks() && !force`, but we map it for
            // completeness (e.g. if core's safety scan is stricter).
            render_worktree_safety_report(
                report,
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

fn select_worktree_interactively(
    candidates: &[&WorktreeInfo],
    ctx: &CommandContext,
) -> Result<String, CliError> {
    let locale = ctx.locale();
    let labels: Vec<String> = candidates
        .iter()
        .map(|w| {
            let name = worktree_display_name(w);
            let branch = w
                .branch
                .as_deref()
                .map(|b| format!(" [{b}]"))
                .unwrap_or_default();
            format!(
                "{name}{branch}  {}",
                display_path_for_humans(w.path.as_path())
            )
        })
        .collect();
    let title = ctx.i18n.resolve(locale, k::WORKTREE_REMOVE_SELECT);
    let selection = ui::screens::select::run(&title, &labels, 0)?;
    if let Some(label) = labels.get(selection) {
        super::print_choice_transcript(&title, label);
    }
    Ok(worktree_display_name(candidates[selection]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;

    fn wt(path: &str, branch: Option<&str>, is_main: bool) -> WorktreeInfo {
        WorktreeInfo {
            path: Utf8PathBuf::from(path),
            branch: branch.map(str::to_owned),
            is_main,
            resolved_strategy: None,
        }
    }

    #[test]
    fn display_name_prefers_directory_name() {
        let w = wt(
            "/repo/.workhive/worktrees/feature-x",
            Some("feature-x"),
            false,
        );
        assert_eq!(worktree_display_name(&w), "feature-x");
    }

    #[test]
    fn display_name_falls_back_to_branch_for_root() {
        let w = wt("/", Some("main"), true);
        assert_eq!(worktree_display_name(&w), "main");
    }

    #[test]
    fn filter_excludes_main_worktree() {
        let base = Utf8PathBuf::from("/repo/.workhive/worktrees");
        let worktrees = vec![
            wt("/repo", Some("main"), true),
            wt("/repo/.workhive/worktrees/feat-a", Some("feat-a"), false),
        ];
        let managed = filter_managed_worktrees(&worktrees, base.as_path());
        assert_eq!(managed.len(), 1, "main worktree must be excluded");
        assert_eq!(managed[0].path, "/repo/.workhive/worktrees/feat-a");
    }

    #[test]
    fn filter_excludes_external_linked_worktrees() {
        // External linked worktrees (e.g. `git worktree add ../sandbox`)
        // must never appear in our list/remove candidates.
        let base = Utf8PathBuf::from("/repo/.workhive/worktrees");
        let worktrees = vec![
            wt("/repo", Some("main"), true),
            wt("/repo/.workhive/worktrees/feat-a", Some("feat-a"), false),
            wt("/elsewhere/sandbox", Some("sandbox"), false),
            wt("/tmp/scratch", Some("scratch"), false),
        ];
        let managed = filter_managed_worktrees(&worktrees, base.as_path());
        assert_eq!(managed.len(), 1, "only managed feat-a should remain");
        assert_eq!(managed[0].path, "/repo/.workhive/worktrees/feat-a");
    }

    #[test]
    fn filter_treats_lookalike_managed_path_as_external() {
        // Component-aware check: `/repo/.workhive/worktrees-archive/x`
        // shares a string prefix with the managed base but is a
        // different directory — must NOT be treated as managed.
        let base = Utf8PathBuf::from("/repo/.workhive/worktrees");
        let worktrees = vec![wt("/repo/.workhive/worktrees-archive/x", Some("x"), false)];
        let managed = filter_managed_worktrees(&worktrees, base.as_path());
        assert!(
            managed.is_empty(),
            "lookalike base must not match — got {managed:?}"
        );
    }

    #[test]
    fn is_under_managed_base_handles_exact_and_nested() {
        let base = Utf8Path::new("/repo/.workhive/worktrees");
        assert!(is_under_managed_base(
            Utf8Path::new("/repo/.workhive/worktrees/feat"),
            base
        ));
        assert!(is_under_managed_base(
            Utf8Path::new("/repo/.workhive/worktrees/feat/sub"),
            base
        ));
        // The base directory itself is *not*
        // a managed worktree — only entries strictly inside it are.
        assert!(!is_under_managed_base(base, base));
        assert!(!is_under_managed_base(
            Utf8Path::new("/repo/.workhive/worktrees-archive/feat"),
            base
        ));
        assert!(!is_under_managed_base(
            Utf8Path::new("/elsewhere/sandbox"),
            base
        ));
    }

    #[test]
    fn filter_excludes_managed_base_directory_itself() {
        // A spurious entry whose path is exactly the managed base must
        // never appear as a removable worktree.
        let base = Utf8PathBuf::from("/repo/.workhive/worktrees");
        let worktrees = vec![
            wt(base.as_str(), Some("worktrees"), false),
            wt("/repo/.workhive/worktrees/feat-a", Some("feat-a"), false),
        ];
        let managed = filter_managed_worktrees(&worktrees, base.as_path());
        assert_eq!(
            managed.len(),
            1,
            "managed base must be excluded from list/remove candidates"
        );
        assert_eq!(managed[0].path, "/repo/.workhive/worktrees/feat-a");
    }
}

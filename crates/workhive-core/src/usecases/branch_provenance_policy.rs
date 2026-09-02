//! Pure decisions for branch provenance on open and branch cleanup on removal.
//!
//! These resolvers turn the typed [`BranchProvenance`] plus user intent
//! (flags, interactivity) into an explicit action the CLI must take. They
//! contain no I/O so the safety-critical control flow — never silently
//! attaching a pre-existing branch, never auto-deleting a user-owned
//! branch — is unit-testable in isolation.

use crate::ports::BranchProvenance;

/// What `workspace open` must do for the requested workspace, decided from
/// observable repository state plus explicit user intent.
///
/// The branch-origin signals are facts about the current request:
/// whether a Workhive-managed worktree already exists and whether the
/// local branch already exists. Provenance is never guessed from reflog,
/// timestamps, or naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenBranchAction {
    /// A Workhive-managed worktree already exists — reuse/open it.
    /// Provenance is left untouched.
    ReuseManagedWorktree,
    /// The local branch does not exist — create branch + worktree and
    /// record [`BranchProvenance::WorkhiveCreated`].
    CreateNewBranch,
    /// The branch exists with no managed worktree and the user explicitly
    /// authorized attaching it — attach and record
    /// [`BranchProvenance::PreExistingAttached`].
    AttachExistingAuthorized,
    /// The branch exists with no managed worktree in an interactive
    /// terminal — must prompt for confirmation (default No) before any
    /// attach.
    PromptAttachExisting,
    /// The branch exists with no managed worktree in non-interactive mode
    /// and no explicit authorization — must fail instead of silently
    /// attaching.
    BlockedNeedsAuthorization,
}

/// Resolve the open action without performing any side effects.
pub fn resolve_open_branch_action(
    managed_worktree_exists: bool,
    branch_exists: bool,
    attach_existing_authorized: bool,
    interactive: bool,
) -> OpenBranchAction {
    if managed_worktree_exists {
        return OpenBranchAction::ReuseManagedWorktree;
    }
    if !branch_exists {
        return OpenBranchAction::CreateNewBranch;
    }
    // Branch exists but Workhive does not manage a worktree for it.
    if attach_existing_authorized {
        OpenBranchAction::AttachExistingAuthorized
    } else if interactive {
        OpenBranchAction::PromptAttachExisting
    } else {
        OpenBranchAction::BlockedNeedsAuthorization
    }
}

/// What `remove`/`destroy`/`worktree remove` must do with the backing
/// local branch, decided from provenance plus explicit user intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchCleanupAction {
    /// Delete the backing branch with safe semantics (`git branch -d`).
    Delete,
    /// Keep the backing branch; no prompt is required.
    Keep,
    /// User-owned branch in an interactive terminal — prompt to delete
    /// (default No). Keep unless the user confirms.
    PromptDelete,
}

/// Resolve the branch-cleanup action without performing any side effects.
///
/// Precedence:
/// 1. `keep_branch` always wins — never delete when the user asked to keep.
/// 2. Workhive-created branches are deleted by default.
/// 3. User-owned (pre-existing/attached/unknown) branches are kept unless
///    the user explicitly authorizes deletion (`delete_branch`) or, in an
///    interactive terminal, confirms a prompt.
pub fn resolve_branch_cleanup_action(
    provenance: BranchProvenance,
    keep_branch: bool,
    delete_branch_authorized: bool,
    interactive: bool,
) -> BranchCleanupAction {
    if keep_branch {
        return BranchCleanupAction::Keep;
    }
    if provenance.auto_deletes_branch() {
        return BranchCleanupAction::Delete;
    }
    // User-owned branch: keep by default.
    if delete_branch_authorized {
        BranchCleanupAction::Delete
    } else if interactive {
        BranchCleanupAction::PromptDelete
    } else {
        BranchCleanupAction::Keep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reuses_managed_worktree_regardless_of_branch_state() {
        for branch_exists in [true, false] {
            assert_eq!(
                resolve_open_branch_action(true, branch_exists, false, true),
                OpenBranchAction::ReuseManagedWorktree
            );
        }
    }

    #[test]
    fn open_creates_new_branch_when_branch_absent() {
        assert_eq!(
            resolve_open_branch_action(false, false, false, true),
            OpenBranchAction::CreateNewBranch
        );
        // The create path does not depend on interactivity or authorization.
        assert_eq!(
            resolve_open_branch_action(false, false, true, false),
            OpenBranchAction::CreateNewBranch
        );
    }

    #[test]
    fn open_prompts_when_branch_exists_interactively() {
        assert_eq!(
            resolve_open_branch_action(false, true, false, true),
            OpenBranchAction::PromptAttachExisting
        );
    }

    #[test]
    fn open_blocks_non_interactive_collision_without_flag() {
        assert_eq!(
            resolve_open_branch_action(false, true, false, false),
            OpenBranchAction::BlockedNeedsAuthorization
        );
    }

    #[test]
    fn open_attaches_when_authorized() {
        // Explicit authorization works in both interactive and
        // non-interactive contexts.
        for interactive in [true, false] {
            assert_eq!(
                resolve_open_branch_action(false, true, true, interactive),
                OpenBranchAction::AttachExistingAuthorized
            );
        }
    }

    #[test]
    fn cleanup_keep_branch_always_wins() {
        for provenance in [
            BranchProvenance::WorkhiveCreated,
            BranchProvenance::PreExistingAttached,
            BranchProvenance::Unknown,
        ] {
            for authorized in [true, false] {
                for interactive in [true, false] {
                    assert_eq!(
                        resolve_branch_cleanup_action(provenance, true, authorized, interactive),
                        BranchCleanupAction::Keep,
                        "keep_branch must override every other input"
                    );
                }
            }
        }
    }

    #[test]
    fn cleanup_deletes_workhive_created_by_default() {
        assert_eq!(
            resolve_branch_cleanup_action(BranchProvenance::WorkhiveCreated, false, false, false),
            BranchCleanupAction::Delete
        );
    }

    #[test]
    fn cleanup_keeps_user_owned_non_interactive_without_authorization() {
        for provenance in [
            BranchProvenance::PreExistingAttached,
            BranchProvenance::Unknown,
        ] {
            assert_eq!(
                resolve_branch_cleanup_action(provenance, false, false, false),
                BranchCleanupAction::Keep,
                "missing/pre-existing provenance must default to keep when non-interactive"
            );
        }
    }

    #[test]
    fn cleanup_prompts_user_owned_interactive() {
        for provenance in [
            BranchProvenance::PreExistingAttached,
            BranchProvenance::Unknown,
        ] {
            assert_eq!(
                resolve_branch_cleanup_action(provenance, false, false, true),
                BranchCleanupAction::PromptDelete
            );
        }
    }

    #[test]
    fn cleanup_deletes_user_owned_when_explicitly_authorized() {
        for provenance in [
            BranchProvenance::PreExistingAttached,
            BranchProvenance::Unknown,
        ] {
            for interactive in [true, false] {
                assert_eq!(
                    resolve_branch_cleanup_action(provenance, false, true, interactive),
                    BranchCleanupAction::Delete,
                    "explicit authorization must delete user-owned branches"
                );
            }
        }
    }
}

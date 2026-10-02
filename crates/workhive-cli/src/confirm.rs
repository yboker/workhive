//! Confirmation policy for mutating commands.
//!
//! Per the confirmation policy:
//! - All mutating commands require confirmation by default
//! - `-f` / `--force` skips confirmation
//! - Default choice is cancel (`[y/N]`) for high-risk destructive operations
//! - A small, explicit subset of *low-risk* user-initiated mutations
//!   (currently only `workhive pane send`, where the user has already
//!   typed the target and the literal text) may opt in to a default of
//!   `Yes` via [`ConfirmDefault::Yes`] / [`require_confirm_with_default`].
//!   The global default for [`require_confirm`] remains `No`.
//! - Read-only commands skip confirmation
//!
//! ## Why a typed `ConfirmDefault` instead of a `bool`
//!
//! Default selection at the prompt is a UX decision, not a yes/no flag.
//! A typed enum:
//!
//! 1. Makes call sites self-documenting (`ConfirmDefault::Yes` reads
//!    correctly at the call site, while `true` does not).
//! 2. Forbids accidental `true`/`false` argument transposition (the
//!    confirm helper already takes a `force: bool`, and stacking two
//!    `bool`s of opposite meaning is exactly the bug class we want to
//!    avoid).
//! 3. Lets us extend the set of defaults later (e.g. a future
//!    `Configured(...)`) without rewriting every callsite.

use std::io::IsTerminal;

use crate::error::CliError;
pub(crate) use crate::ui::screens::confirm::ConfirmContext;

/// Default selection for a confirmation prompt.
///
/// `No` is the global policy for high-risk operations (close / remove /
/// destroy / config / init). `Yes` is opt-in for explicitly low-risk
/// user-initiated mutations such as `pane send`, where the user has
/// already supplied the literal payload. See module docs for the
/// rationale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmDefault {
    /// Default highlights "No"; matches the confirmation policy.
    No,
    /// Default highlights "Yes"; only allowed for low-risk
    /// user-initiated mutations.
    Yes,
}

impl ConfirmDefault {
    /// Whether this default selects "Yes" at prompt entry.
    pub fn is_yes(self) -> bool {
        matches!(self, Self::Yes)
    }
}

/// Ask the user for confirmation before a mutating operation, with
/// the policy default of `No` (matches the confirmation policy).
///
/// - If `force` is true, returns `Ok(())` immediately (no prompt).
/// - If stdin is not a terminal, returns an error (non-interactive
///   must use `--force`).
/// - Otherwise, prompts with the given message and default `No`.
///
/// Returns `Err(CliError::UserCancelled)` if the user declines.
pub fn require_confirm(message: &str, force: bool) -> Result<(), CliError> {
    require_confirm_with_default(message, force, ConfirmDefault::No)
}

/// Ask the user for confirmation, with an explicit default selection.
///
/// Use this when the prompt is for a low-risk, user-initiated
/// mutation (e.g. `workhive pane send`, where the user has typed the
/// pane number and literal text). High-risk destructive operations
/// must keep using [`require_confirm`] / [`ConfirmDefault::No`].
///
/// Behavior matches [`require_confirm`] except the prompt enters with
/// `default` highlighted.
pub fn require_confirm_with_default(
    message: &str,
    force: bool,
    default: ConfirmDefault,
) -> Result<(), CliError> {
    prompt_confirm(message, None, force, default)
}

/// Ask the user for confirmation while keeping a decision-local safety
/// `context` visible inside the confirmation screen.
///
/// This is the destructive path for `workspace remove`, `workspace
/// destroy`, and forced risky `worktree remove`: the changed-file
/// safety summary that was printed before the prompt can be covered by
/// the modal alternate screen, so the same lines are threaded into the
/// confirmation body. The default selection is always [`ConfirmDefault::No`]
/// — destructive operations never enter with `Yes` highlighted.
///
/// Behavior otherwise matches [`require_confirm`]: `force` short-circuits
/// without prompting, a non-interactive session demands `--force`, and a
/// declined prompt returns [`CliError::UserCancelled`].
pub fn require_confirm_with_context(
    message: &str,
    context: &ConfirmContext,
    force: bool,
) -> Result<(), CliError> {
    prompt_confirm(message, Some(context), force, ConfirmDefault::No)
}

/// Shared confirmation flow for the public helpers above.
///
/// Centralizing the `force` short-circuit, the non-interactive guard,
/// and the decline mapping keeps every entry point — with or without a
/// safety context — on exactly one policy path, so `--force` can never
/// drift into prompting and a declined prompt can never silently
/// succeed.
fn prompt_confirm(
    message: &str,
    context: Option<&ConfirmContext>,
    force: bool,
    default: ConfirmDefault,
) -> Result<(), CliError> {
    if force {
        tracing::debug!("confirmation skipped (--force)");
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        tracing::warn!("non-interactive session without --force");
        return Err(CliError::ConfirmationRequired(
            "this command requires --force in non-interactive mode".into(),
        ));
    }

    let confirmed =
        crate::ui::screens::confirm::run_with_context(message, context, default.is_yes())?;

    if confirmed {
        tracing::debug!("user confirmed");
        Ok(())
    } else {
        tracing::debug!("user declined");
        Err(CliError::UserCancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_default_no_is_not_yes() {
        assert!(!ConfirmDefault::No.is_yes());
    }

    #[test]
    fn confirm_default_yes_is_yes() {
        assert!(ConfirmDefault::Yes.is_yes());
    }

    /// `require_confirm_with_default(force=true, _)` must short-circuit
    /// regardless of default selection. This is the regression for the
    /// "Task 3: pane send confirm default Yes" change — `--force` still
    /// bypasses confirmation.
    #[test]
    fn force_bypasses_confirm_for_default_yes() {
        require_confirm_with_default("ignored", true, ConfirmDefault::Yes)
            .expect("force=true must bypass prompt with default=Yes");
    }

    #[test]
    fn force_bypasses_confirm_for_default_no() {
        require_confirm_with_default("ignored", true, ConfirmDefault::No)
            .expect("force=true must bypass prompt with default=No");
        require_confirm("ignored", true).expect("require_confirm force=true bypasses prompt");
    }

    /// Non-interactive sessions (no TTY) must still demand `--force`.
    /// We can rely on this because `cargo test` runs without a TTY,
    /// so stdin is not a terminal here.
    #[test]
    fn non_interactive_requires_force_even_with_default_yes() {
        let err = require_confirm_with_default("prompt", false, ConfirmDefault::Yes)
            .expect_err("non-interactive must error without --force");
        match err {
            CliError::ConfirmationRequired(_) => {}
            other => panic!("expected ConfirmationRequired, got {other:?}"),
        }
    }

    #[test]
    fn non_interactive_require_confirm_default_no_still_errors() {
        let err = require_confirm("prompt", false)
            .expect_err("non-interactive default=No must error without --force");
        match err {
            CliError::ConfirmationRequired(_) => {}
            other => panic!("expected ConfirmationRequired, got {other:?}"),
        }
    }

    fn sample_context() -> ConfirmContext {
        use crate::ui::screens::confirm::{
            ConfirmContextLine, ConfirmLinePriority, ConfirmLineSeverity,
        };
        ConfirmContext {
            lines: vec![
                ConfirmContextLine {
                    text: "Workspace remove safety report:".to_owned(),
                    priority: ConfirmLinePriority::Important,
                    severity: ConfirmLineSeverity::Normal,
                },
                ConfirmContextLine {
                    text: "changed files: 2 (staged 0, unstaged 2, untracked 0)".to_owned(),
                    priority: ConfirmLinePriority::Essential,
                    severity: ConfirmLineSeverity::RiskCount,
                },
            ],
            truncation_notice: "more safety lines hidden".to_owned(),
            too_small_notice: "terminal too small to show safety details".to_owned(),
        }
    }

    /// A context-bearing destructive confirmation must still honor
    /// `--force`: the safety context never introduces a new prompt when
    /// the user already opted into the non-interactive path.
    #[test]
    fn force_bypasses_confirm_with_context() {
        require_confirm_with_context("ignored", &sample_context(), true)
            .expect("force=true must bypass the context-bearing prompt");
    }

    /// Without `--force`, a context-bearing confirmation in a
    /// non-interactive session must still demand `--force` rather than
    /// silently proceeding with a destructive removal.
    #[test]
    fn non_interactive_with_context_requires_force() {
        let err = require_confirm_with_context("prompt", &sample_context(), false)
            .expect_err("non-interactive context confirm must error without --force");
        match err {
            CliError::ConfirmationRequired(_) => {}
            other => panic!("expected ConfirmationRequired, got {other:?}"),
        }
    }
}

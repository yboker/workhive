//! Typed navigation decision for tmux session attach/switch.
//!
//! Determines the correct runtime operation when the user invokes an
//! attach-capable command (`workspace open`, `workspace attach`, or
//! `session attach`) based on the current execution context and the
//! intended target workspace.

use crate::ports::{ExecutionContext, TmuxMembership};

/// Target workspace the user wants to navigate to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationTarget {
    /// Isolation group (tmux server name).
    pub group: String,
    /// Workspace session name within the group.
    pub workspace_name: String,
}

/// Exhaustive result of the navigation decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxNavigationDecision {
    /// Not inside tmux — use normal `attach-session`.
    Attach,
    /// Already inside the exact target server/session — no-op.
    AlreadyCurrent,
    /// Inside another session on the same tmux server — use
    /// `switch-client -t <target>`.
    SwitchWithinGroup,
    /// Inside a different tmux server — reject before any runtime
    /// mutation.
    RejectCrossGroup {
        current_server: String,
        target_server: String,
    },
}

/// Error when the execution context cannot authorize navigation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NavigationContextError {
    /// Tmux evidence exists but server/session could not be resolved.
    /// Must fail before guidance, metadata, configuration, or runtime calls.
    #[error("unresolved tmux context: {reason}")]
    UnresolvedContext { reason: String },
}

/// Pure decision function: given the execution context and a navigation
/// target, determine which runtime operation to perform.
///
/// This function does not perform I/O. It inspects the `TmuxMembership`
/// populated by the environment port at the CLI boundary.
pub fn decide_navigation(
    ctx: &ExecutionContext,
    target: &NavigationTarget,
) -> Result<TmuxNavigationDecision, NavigationContextError> {
    match &ctx.tmux {
        TmuxMembership::Outside => Ok(TmuxNavigationDecision::Attach),

        TmuxMembership::Resolved {
            server, session, ..
        } => {
            if server == &target.group && session == &target.workspace_name {
                Ok(TmuxNavigationDecision::AlreadyCurrent)
            } else if server == &target.group {
                Ok(TmuxNavigationDecision::SwitchWithinGroup)
            } else {
                Ok(TmuxNavigationDecision::RejectCrossGroup {
                    current_server: server.clone(),
                    target_server: target.group.clone(),
                })
            }
        }

        TmuxMembership::Unresolved { reason } => Err(NavigationContextError::UnresolvedContext {
            reason: reason.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outside_tmux_context() -> ExecutionContext {
        ExecutionContext {
            is_interactive: true,
            tmux: TmuxMembership::Outside,
        }
    }

    fn inside_tmux_context(server: &str, session: &str) -> ExecutionContext {
        ExecutionContext {
            is_interactive: true,
            tmux: TmuxMembership::Resolved {
                server: server.to_owned(),
                session: session.to_owned(),
                pane: Some("%0".to_owned()),
            },
        }
    }

    fn target(group: &str, workspace: &str) -> NavigationTarget {
        NavigationTarget {
            group: group.to_owned(),
            workspace_name: workspace.to_owned(),
        }
    }

    #[test]
    fn tmux_navigation_outside_tmux_attaches() {
        let ctx = outside_tmux_context();
        let t = target("whsrv-inst-proj", "wh@inst@proj@main");
        let decision = decide_navigation(&ctx, &t).unwrap();
        assert_eq!(decision, TmuxNavigationDecision::Attach);
    }

    #[test]
    fn tmux_navigation_exact_target_is_already_current() {
        let ctx = inside_tmux_context("whsrv-inst-proj", "wh@inst@proj@main");
        let t = target("whsrv-inst-proj", "wh@inst@proj@main");
        let decision = decide_navigation(&ctx, &t).unwrap();
        assert_eq!(decision, TmuxNavigationDecision::AlreadyCurrent);
    }

    #[test]
    fn tmux_navigation_same_group_switches() {
        let ctx = inside_tmux_context("whsrv-inst-proj", "wh@inst@proj@main");
        let t = target("whsrv-inst-proj", "wh@inst@proj@feature");
        let decision = decide_navigation(&ctx, &t).unwrap();
        assert_eq!(decision, TmuxNavigationDecision::SwitchWithinGroup);
    }

    #[test]
    fn tmux_navigation_cross_group_is_rejected() {
        let ctx = inside_tmux_context("whsrv-inst-projA", "wh@inst@projA@main");
        let t = target("whsrv-inst-projB", "wh@inst@projB@main");
        let decision = decide_navigation(&ctx, &t).unwrap();
        assert_eq!(
            decision,
            TmuxNavigationDecision::RejectCrossGroup {
                current_server: "whsrv-inst-projA".to_owned(),
                target_server: "whsrv-inst-projB".to_owned(),
            }
        );
    }

    #[test]
    fn tmux_navigation_partial_context_is_rejected() {
        let ctx = ExecutionContext {
            is_interactive: true,
            tmux: TmuxMembership::Unresolved {
                reason: "server present but session probe failed".to_owned(),
            },
        };
        let t = target("whsrv-inst-proj", "wh@inst@proj@main");
        let result = decide_navigation(&ctx, &t);
        assert!(result.is_err());
        match result.unwrap_err() {
            NavigationContextError::UnresolvedContext { reason } => {
                assert!(reason.contains("session probe failed"));
            }
        }
    }

    #[test]
    fn tmux_navigation_unresolved_context_is_rejected() {
        let ctx = ExecutionContext {
            is_interactive: true,
            tmux: TmuxMembership::Unresolved {
                reason: "malformed TMUX variable".to_owned(),
            },
        };
        let t = target("whsrv-inst-proj", "wh@inst@proj@main");
        let result = decide_navigation(&ctx, &t);
        assert!(result.is_err());
    }

    #[test]
    fn tmux_navigation_lone_pane_context_is_rejected() {
        let ctx = ExecutionContext {
            is_interactive: true,
            tmux: TmuxMembership::Unresolved {
                reason: "TMUX_PANE present without resolved server/session".to_owned(),
            },
        };
        let t = target("whsrv-inst-proj", "wh@inst@proj@main");
        let result = decide_navigation(&ctx, &t);
        assert!(result.is_err());
        match result.unwrap_err() {
            NavigationContextError::UnresolvedContext { reason } => {
                assert!(reason.contains("TMUX_PANE"));
            }
        }
    }
}

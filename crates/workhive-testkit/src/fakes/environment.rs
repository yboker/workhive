//! Fake environment adapter for testing.

use workhive_core::errors::EnvironmentError;
use workhive_core::ports::{EnvironmentPort, ExecutionContext, TmuxMembership};

/// Fake environment with configurable context.
pub struct FakeEnvironment {
    context: ExecutionContext,
}

impl FakeEnvironment {
    /// Create a fake simulating an interactive terminal outside tmux.
    pub fn interactive() -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: true,
                tmux: TmuxMembership::Outside,
            },
        }
    }

    /// Create a fake simulating being inside a workhive tmux session.
    pub fn in_session(session: &str, pane: &str) -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: true,
                tmux: TmuxMembership::Resolved {
                    server: "default".to_owned(),
                    session: session.to_owned(),
                    pane: Some(pane.to_owned()),
                },
            },
        }
    }

    /// Create a fake simulating being inside a workhive tmux session with server.
    pub fn in_session_with_server(session: &str, pane: &str, server: &str) -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: true,
                tmux: TmuxMembership::Resolved {
                    server: server.to_owned(),
                    session: session.to_owned(),
                    pane: Some(pane.to_owned()),
                },
            },
        }
    }

    /// Create a fake simulating a non-interactive (piped) context.
    pub fn non_interactive() -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: false,
                tmux: TmuxMembership::Outside,
            },
        }
    }
}

impl EnvironmentPort for FakeEnvironment {
    fn execution_context(&self) -> Result<ExecutionContext, EnvironmentError> {
        Ok(self.context.clone())
    }
}

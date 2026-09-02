//! Fake environment adapter for testing.

use workhive_core::errors::EnvironmentError;
use workhive_core::ports::{EnvironmentPort, ExecutionContext};

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
                tmux_session: None,
                tmux_pane: None,
                tmux_server: None,
            },
        }
    }

    /// Create a fake simulating being inside a workhive tmux session.
    pub fn in_session(session: &str, pane: &str) -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: true,
                tmux_session: Some(session.to_owned()),
                tmux_pane: Some(pane.to_owned()),
                tmux_server: None,
            },
        }
    }

    /// Create a fake simulating being inside a workhive tmux session with server.
    pub fn in_session_with_server(session: &str, pane: &str, server: &str) -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: true,
                tmux_session: Some(session.to_owned()),
                tmux_pane: Some(pane.to_owned()),
                tmux_server: Some(server.to_owned()),
            },
        }
    }

    /// Create a fake simulating a non-interactive (piped) context.
    pub fn non_interactive() -> Self {
        Self {
            context: ExecutionContext {
                is_interactive: false,
                tmux_session: None,
                tmux_pane: None,
                tmux_server: None,
            },
        }
    }
}

impl EnvironmentPort for FakeEnvironment {
    fn execution_context(&self) -> Result<ExecutionContext, EnvironmentError> {
        Ok(self.context.clone())
    }
}

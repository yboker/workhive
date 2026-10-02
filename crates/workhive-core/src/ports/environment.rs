//! Environment port — abstracts runtime environment detection.

use crate::errors::EnvironmentError;

/// Typed tmux membership representing the execution environment's
/// relationship with tmux. Prevents unresolved or partial tmux state
/// from being silently treated as "outside tmux".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxMembership {
    /// No TMUX or TMUX_PANE evidence — proven to be outside tmux.
    Outside,
    /// Server and session are authoritatively known.
    Resolved {
        server: String,
        session: String,
        pane: Option<String>,
    },
    /// Tmux evidence exists (TMUX, TMUX_PANE, or both) but server/session
    /// could not both be resolved. Must fail closed — never treat as Outside.
    Unresolved { reason: String },
}

/// Information about the current execution context.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Whether stdin is connected to a terminal (interactive mode).
    pub is_interactive: bool,
    /// Typed tmux membership state.
    pub tmux: TmuxMembership,
}

/// Abstraction over runtime environment detection.
///
/// Used to determine if we're inside a tmux session, whether the terminal
/// is interactive, etc.
pub trait EnvironmentPort {
    /// Detect the current execution context.
    fn execution_context(&self) -> Result<ExecutionContext, EnvironmentError>;
}

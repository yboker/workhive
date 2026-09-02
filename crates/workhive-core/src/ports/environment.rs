//! Environment port — abstracts runtime environment detection.

use crate::errors::EnvironmentError;

/// Information about the current execution context.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Whether stdin is connected to a terminal (interactive mode).
    pub is_interactive: bool,
    /// Current tmux session name, if running inside tmux.
    pub tmux_session: Option<String>,
    /// Current tmux pane ID, if running inside tmux.
    pub tmux_pane: Option<String>,
    /// Current tmux server (socket) name, if running inside tmux.
    ///
    /// Resolution order:
    /// 1. Parse from `$TMUX` env var (format: `/path/to/<server>,pid,window`).
    /// 2. Fallback: query `tmux display-message -p '#{socket_path}'` and
    ///    extract the file-name component.
    pub tmux_server: Option<String>,
}

/// Abstraction over runtime environment detection.
///
/// Used to determine if we're inside a tmux session, whether the terminal
/// is interactive, etc.
pub trait EnvironmentPort {
    /// Detect the current execution context.
    fn execution_context(&self) -> Result<ExecutionContext, EnvironmentError>;
}

//! Infra-layer error types.
//!
//! These errors are specific to the tmux backend and are intentionally
//! kept out of `workhive-core`. Business use cases see only `RuntimeError`
//! (defined in `workhive-core::ports::runtime`). The adapters in this crate
//! translate `TmuxError` → `RuntimeError` at the port boundary.

/// Errors from tmux operations (session/pane management).
///
/// This type belongs to the infra layer. `workhive-core` does not know about
/// it — use cases depend on `RuntimeError` instead.
#[derive(Debug, thiserror::Error)]
pub enum TmuxError {
    #[error("tmux executable not found")]
    TmuxNotFound,

    #[error("tmux server not running: {server}")]
    ServerNotRunning { server: String },

    #[error("tmux socket not found: {path}")]
    SocketNotFound { path: String },

    #[error("tmux permission denied: {detail}")]
    PermissionDenied { detail: String },

    #[error("session not found: {name}")]
    SessionNotFound { name: String },

    /// A session with the same name already exists on this server.
    ///
    /// tmux `new-session` exits with "duplicate session" when the target name
    /// is already in use. This maps to `RuntimeError::WorkspaceAlreadyExists`
    /// at the adapter boundary.
    #[error("session already exists: {name}")]
    SessionAlreadyExists { name: String },

    #[error("failed to create session '{name}'")]
    SessionCreateFailed {
        name: String,
        #[source]
        source: std::io::Error,
    },

    /// `new-session` returned exit 0 but the session does not actually exist.
    #[error(
        "session creation reported success but session '{name}' was not found on server '{server}'"
    )]
    SessionCreateNotVerified { server: String, name: String },

    #[error("pane not found: {id}")]
    PaneNotFound { id: String },

    #[error("cannot close last pane in session")]
    LastPaneCloseRejected,

    #[error("tmux command failed: {message}")]
    CommandFailed { message: String },
}

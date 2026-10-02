//! Infrastructure layer for the pane runtime control plane.
//!
//! This module implements the concrete Unix-domain-socket transport for the
//! typed protocol defined in `workhive_core::control_plane::protocol`. It
//! exposes:
//!
//!   * [`control_plane_socket_path`] — deterministic per-group socket path
//!     under a short runtime root (see `socket.rs`);
//!   * [`LocalPaneControlPlaneClient`] — blocking Unix-socket client;
//!   * [`ControlPlaneSupervisor`] — single-threaded blocking supervisor
//!     server that serves one connection at a time over a `UnixListener`.
//!
//! Core invariants:
//!
//!   * tmux details stay in `crate::tmux`;
//!   * the supervisor delivers sends via the existing `PaneRuntimePort`
//!     (which the tmux adapter already implements) so item 7's literal-text
//!     + separate-Enter semantics are preserved end-to-end;
//!   * socket paths are Workhive-owned and live under a short runtime root
//!     (`$XDG_RUNTIME_DIR/workhive/` preferred, user-scoped temp dir
//!     fallback) instead of the project's `.workhive/` tree, so deeply
//!     nested project paths cannot exceed `SUN_LEN` on macOS.

mod client;
mod server;
mod socket;

pub use client::LocalPaneControlPlaneClient;
pub use server::{
    ControlPlaneSupervisor, ControlPlaneSupervisorError, prepare_socket_parent, remove_stale_socket,
};
pub use socket::{MAX_UNIX_SOCKET_PATH_LEN, control_plane_runtime_root, control_plane_socket_path};

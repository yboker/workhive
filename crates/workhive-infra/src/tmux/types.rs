//! Tmux request and info DTOs shared by the port trait and the CLI adapter.

use camino::Utf8PathBuf;
use workhive_core::domain::{PaneDirection, PaneId, PaneKind, PaneState};

// ─── Tmux-specific DTOs ────────────────────────────────────────────────────

/// Request to create a new tmux session.
#[derive(Debug, Clone)]
pub struct CreateSessionRequest {
    /// Tmux server name (e.g., `whsrv-myproject`).
    pub server_name: String,
    /// Session name (e.g., `wh::myproject::feature-auth`).
    pub session_name: String,
    /// Working directory for the initial window.
    pub working_dir: Utf8PathBuf,
    /// Environment variables (`KEY`, `VALUE`) to set on the new session.
    /// These are passed to tmux via `-e KEY=VALUE` arguments and become
    /// available to all child processes spawned in the session.
    pub env: Vec<(String, String)>,
}

/// Request to attach to an existing session.
#[derive(Debug, Clone)]
pub struct AttachRequest {
    /// Tmux server name.
    pub server_name: String,
    /// Session name to attach to.
    pub session_name: String,
}

/// Request to split an existing pane.
#[derive(Debug, Clone)]
pub struct SplitPaneRequest {
    /// Tmux server name.
    pub server_name: String,
    /// Session name.
    pub session_name: String,
    /// Direction of the split.
    pub direction: PaneDirection,
    /// Optional command to run in the new pane.
    pub command: Option<String>,
    /// Target pane to split. When `None`, tmux splits the active pane.
    pub target_pane: Option<PaneId>,
    /// Size of the new pane as a percentage (passed as tmux `-p N`).
    /// When `None`, tmux uses its default (50%).
    pub size_percent: Option<u8>,
    /// Environment variables (`KEY`, `VALUE`) to set on the new pane.
    /// These are passed to tmux via `-e KEY=VALUE` arguments so that the
    /// process spawned in the new pane sees them.
    pub env: Vec<(String, String)>,
    /// Optional initial working directory for the new pane. When `Some`, the
    /// adapter passes it to tmux via `split-window -c <cwd>` so the pane
    /// starts in that directory without a visible `cd` keystroke.
    pub cwd: Option<Utf8PathBuf>,
}

/// Request to send keystrokes to a pane.
#[derive(Debug, Clone)]
pub struct SendKeysRequest {
    /// Tmux server name.
    pub server_name: String,
    /// Target pane identifier.
    pub target_pane: PaneId,
    /// Text to send.
    pub text: String,
    /// Whether to append Enter after the text.
    pub enter: bool,
}

/// Request to capture the visible text of a tmux pane via `capture-pane -p`.
///
/// `start_line` is passed straight through to `capture-pane -S <line>` and
/// follows tmux's convention: negative values count back from the bottom of
/// the visible buffer (e.g. `-20` captures the last 20 lines on top of the
/// current screen). Adapters MUST format it as a string at call time so the
/// payload remains a typed integer at the port boundary instead of leaking
/// tmux command syntax into core.
#[derive(Debug, Clone)]
pub struct CapturePaneRequest {
    /// Tmux server name.
    pub server_name: String,
    /// Target pane identifier (a tmux `%N` id when known).
    pub target_pane: PaneId,
    /// First line to include in the capture, in tmux `-S` semantics.
    /// Use a small negative number to capture the last N lines.
    pub start_line: i32,
}

/// Runtime information about a tmux session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Session name.
    pub name: String,
    /// Number of windows in the session.
    pub windows: u32,
    /// Total number of panes across all windows.
    pub panes: u32,
    /// Whether a client is currently attached.
    pub attached: bool,
}

/// Runtime information about a tmux pane.
#[derive(Debug, Clone)]
pub struct PaneInfo {
    /// Tmux pane ID (e.g., `%0`).
    pub id: PaneId,
    /// Zero-based pane index within the window.
    pub index: u32,
    /// Current working directory of the pane.
    pub cwd: Utf8PathBuf,
    /// Current foreground command name.
    pub command: String,
    /// Whether this pane is the active (focused) one.
    pub active: bool,
    /// Best-effort pane type classification.
    pub kind: PaneKind,
    /// Best-effort pane state classification.
    pub state: PaneState,
    /// Tmux `#{pane_tty}` (e.g. `/dev/ttys003`). Empty strings from
    /// tmux are normalized to `None` so consumers cannot accidentally
    /// treat "" as a valid tty path.
    pub tty: Option<String>,
    /// Tmux `#{pane_pid}` (the pane's primary shell pid). `None` when
    /// tmux did not provide a parsable u32.
    pub process_pid: Option<u32>,
}

/// Runtime information about a tmux window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Zero-based window index.
    pub index: u32,
    /// Window name.
    pub name: Option<String>,
    /// Layout string for faithful reconstruction.
    pub layout: String,
    /// Whether this window is active.
    pub active: bool,
}

/// Detailed pane metadata for runtime topology observation.
///
/// Carries the same `tty` / `process_pid` observation facts as
/// [`PaneInfo`] so topology-based code paths can feed the pane runtime
/// observer with the same inputs that fresh `pane list` uses.
#[derive(Debug, Clone)]
pub struct PaneTopologyInfo {
    /// Tmux pane ID (e.g., `%0`).
    pub id: PaneId,
    /// Zero-based window index.
    pub window_index: u32,
    /// Zero-based pane index in window.
    pub pane_index: u32,
    /// Current working directory.
    pub cwd: Utf8PathBuf,
    /// Foreground command.
    pub current_command: String,
    /// Start command.
    pub start_command: Option<String>,
    /// Pane title.
    pub title: Option<String>,
    /// Whether pane is active.
    pub active: bool,
    /// Whether pane process has exited.
    pub dead: bool,
    /// Best-effort pane classification.
    pub kind: PaneKind,
    /// Best-effort pane state.
    pub state: PaneState,
    /// Path to the pane's controlling terminal (e.g., `/dev/ttys003`).
    pub tty: Option<String>,
    /// PID of the pane's shell process (root of the pane's process tree).
    pub process_pid: Option<u32>,
}

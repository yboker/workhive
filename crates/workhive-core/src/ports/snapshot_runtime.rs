//! Snapshot runtime port — topology capture and restore primitives.
//!
//! This port keeps snapshot orchestration runtime-neutral while still exposing
//! the operations required for faithful multi-window restore.

use camino::{Utf8Path, Utf8PathBuf};

use crate::domain::{PaneDirection, PaneLaunchOrigin, UnitId};

use super::{PaneRuntimePort, RuntimeError};

/// Optional pane preparation request carried into `split_pane` so restore can
/// create a child pane that already has the correct working directory and a
/// pre-baked launch command — without ever sending a visible `cd -- ...` into
/// the shell.
#[derive(Debug, Clone, Default)]
pub struct SplitPanePreparation {
    /// Initial working directory for the new pane. When `Some`, the adapter
    /// MUST create the pane with this cwd through its own facility (tmux's
    /// `split-window -c`). When `None`, the adapter uses the backend default
    /// (typically "inherit from the split target").
    pub cwd: Option<Utf8PathBuf>,
    /// Optional command to launch directly inside the new pane. When `Some`,
    /// the adapter spawns the command as the pane's initial process so it is
    /// not visible as typed input in the pane's scrollback. When `None`, the
    /// adapter leaves the pane in its default shell.
    pub command: Option<String>,
}

/// Runtime observation for a single window.
#[derive(Debug, Clone)]
pub struct RuntimeWindowSnapshot {
    /// Window index in the runtime backend.
    pub index: u32,
    /// Window name if set.
    pub name: Option<String>,
    /// Backend layout string.
    pub layout: String,
    /// Whether this window is currently active.
    pub active: bool,
}

/// Runtime observation for a single pane.
///
/// Carries enough information for the layered pane runtime observer
/// (supervisor → OS process observer → tmux fallback) to inspect the
/// pane through snapshot/topology paths consistently with fresh
/// `pane list` queries. In particular, `tty` and `process_pid` MUST
/// be populated whenever the runtime adapter can read them so that
/// snapshot-driven flows (workspace open/restore, snapshot
/// capture/replay) do not silently downgrade to tmux fallback
/// confidence.
#[derive(Debug, Clone)]
pub struct RuntimePaneSnapshot {
    /// Runtime-neutral pane identifier.
    pub unit_id: UnitId,
    /// Backend pane ID if available (for logs / diagnostics).
    pub pane_id: Option<String>,
    /// Owning window index.
    pub window_index: u32,
    /// Pane index within the owning window.
    pub pane_index: u32,
    /// Current working directory.
    pub cwd: Utf8PathBuf,
    /// Pane title if set.
    pub title: Option<String>,
    /// Foreground command at observation time.
    pub current_command: Option<String>,
    /// Start command captured from runtime metadata when available.
    pub start_command: Option<String>,
    /// Whether this pane is active.
    pub active: bool,
    /// Whether this pane is dead/exited.
    pub dead: bool,
    /// Durable launch metadata captured by registry/runtime bridge when available.
    pub launch_origin: Option<PaneLaunchOrigin>,
    /// Whether this pane is managed by workhive.
    pub managed: bool,
    /// Pane controlling terminal (e.g., `/dev/ttys003`) used by the
    /// OS process observer.
    pub tty: Option<String>,
    /// PID of the pane's shell process, used by the OS process
    /// observer as the root of the process tree walk.
    pub process_pid: Option<u32>,
}

/// Full runtime topology observation.
#[derive(Debug, Clone)]
pub struct RuntimeTopologySnapshot {
    /// All windows in the workspace session.
    pub windows: Vec<RuntimeWindowSnapshot>,
    /// All panes across windows.
    pub panes: Vec<RuntimePaneSnapshot>,
    /// Active window index if known.
    pub active_window: Option<u32>,
    /// Active pane runtime ID if known.
    pub active_pane_runtime_id: Option<String>,
}

/// Hook registration mode for snapshot notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotHookMode {
    /// Install hooks.
    Install,
    /// Remove hooks.
    Remove,
}

/// Runtime operations required by snapshot capture/restore/supervision.
pub trait SnapshotRuntimePort {
    /// Capture full workspace topology including windows and panes.
    fn capture_topology(
        &self,
        group: &str,
        workspace_name: &str,
    ) -> Result<RuntimeTopologySnapshot, RuntimeError>;

    /// Create a new window and return its first pane snapshot.
    fn create_window(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
        name: Option<&str>,
        cwd: &Utf8Path,
    ) -> Result<RuntimePaneSnapshot, RuntimeError>;

    /// Split an existing pane and return the new pane snapshot.
    ///
    /// `prep.cwd` and `prep.command` are honored by the adapter through native
    /// backend facilities (e.g. tmux `split-window -c` / direct command
    /// invocation) and MUST NOT be routed through typed shell input like a
    /// manual `cd -- ...` keystroke. This is the primary hook used by the
    /// restore path to avoid polluting pane scrollback (item 5).
    fn split_pane(
        &self,
        group: &str,
        workspace_name: &str,
        target_unit: &UnitId,
        direction: PaneDirection,
        prep: SplitPanePreparation,
    ) -> Result<RuntimePaneSnapshot, RuntimeError>;

    /// Reset an existing pane so its working directory matches `cwd` without
    /// emitting a visible `cd -- ...` keystroke. Implementations MUST use a
    /// backend-native facility (e.g. tmux `respawn-pane -k -c <cwd>`) that
    /// preserves the pane's runtime identifier and tmux layout slot. This
    /// hook is intentionally restore-only: it intentionally kills any process
    /// currently running in the pane because the restore use case is about
    /// to relaunch the pane's launch intent through the typed runtime port.
    fn respawn_unit_with_cwd(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &Utf8Path,
    ) -> Result<(), RuntimeError>;

    /// Reset an existing pane so it runs `command` directly in `cwd`,
    /// without ever typing the command into a visible shell.
    ///
    /// This is the typed runtime launch verb used by restore / fresh-open
    /// flows that need to install the **internal pane supervisor wrapper**
    /// as the pane's initial process. Implementations MUST translate this
    /// to a backend-native command-launch facility (e.g. tmux
    /// `respawn-pane -k -c <cwd> -t <pane> <command>`) so the wrapper text
    /// never appears in the pane's scrollback.
    ///
    /// Scope:
    ///
    /// * Use this verb ONLY for the internal `workhive internal
    ///   pane-supervisor ... -- <tool>` wrapper. The wrapper is a Workhive
    ///   implementation detail and was never meant to be visible to the
    ///   user.
    /// * Do NOT route user-supplied inline / raw commands through this
    ///   verb. Those must remain on the visible
    ///   [`crate::ports::PaneRuntimePort::send_input`] path so the user
    ///   continues to see what they typed and so the pane shell stays
    ///   alive after the command exits — re-routing inline commands here
    ///   would silently change pane lifecycle (the command would become
    ///   the pane's primary process and exit would close the pane).
    ///
    /// Contract:
    ///
    /// * The pane's existing process is killed (`-k` semantics) and `command`
    ///   becomes the pane's new initial process.
    /// * `command` is a single shell-string already escaped by the caller
    ///   (e.g. via [`crate::ports::ToolLaunchInstrumentationPort::wrap_tool_command`]).
    ///   Adapters MUST NOT re-quote it; tmux passes the trailing argument
    ///   to `/bin/sh -c` internally.
    /// * No visible keystroke (`send-keys`, `cd -- ...`, etc.) is permitted.
    ///   This is the canonical fix for the supervisor wrapper visibility
    ///   regression.
    fn respawn_unit_with_command(
        &self,
        group: &str,
        workspace_name: &str,
        unit_id: &UnitId,
        cwd: &Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError>;

    /// Apply opaque backend layout data to a window.
    fn apply_window_layout(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
        layout: &str,
    ) -> Result<(), RuntimeError>;

    /// Select active window after restore.
    fn select_window(
        &self,
        group: &str,
        workspace_name: &str,
        window_index: u32,
    ) -> Result<(), RuntimeError>;

    /// Select active pane after restore.
    fn select_pane(&self, group: &str, unit_id: &UnitId) -> Result<(), RuntimeError>;

    /// Install or remove runtime hooks that emit lightweight snapshot notifications.
    fn configure_snapshot_hooks(
        &self,
        group: &str,
        workspace_name: &str,
        mode: SnapshotHookMode,
        notify_command_prefix: &str,
    ) -> Result<(), RuntimeError>;
}

/// Composite runtime trait used by open/close snapshot workflows.
pub trait SnapshotPaneRuntimePort: PaneRuntimePort + SnapshotRuntimePort {}

impl<T> SnapshotPaneRuntimePort for T where T: PaneRuntimePort + SnapshotRuntimePort + ?Sized {}

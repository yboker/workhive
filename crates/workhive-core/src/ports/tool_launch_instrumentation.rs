//! Tool launch instrumentation port — wraps the user-facing tool
//! command with the Workhive pane supervisor invocation so that the
//! supervisor (not the bare tool) becomes the pane's tracked process.
//!
//! ## Architecture role
//!
//! The pane runtime observation architecture treats Workhive-supervised
//! processes as the highest-confidence source of pane truth. To produce
//! supervisor metadata in normal `workspace open` / snapshot restore
//! flows, every Workhive-launched tool pane must execute the tool
//! through this lightweight supervisor:
//!
//! ```text
//! workhive internal pane-supervisor \
//!   --runtime-root <root> --group <g> --workspace <ws> --unit <id> \
//!   --tool-name <name> [--program-path <p>] -- <program> [args...]
//! ```
//!
//! The supervisor writes `Running` metadata before exec, waits for the
//! child, and writes `Exited` metadata on completion. Stale metadata is
//! recovered by the OS process observer / tmux fallback layers.
//!
//! ## Why this is a port, not a free function
//!
//! Wrapping requires knowing where the workhive binary lives on disk
//! (`current_exe()`), which is a host-level concern. The core use
//! cases stay platform-neutral by depending on this trait.
//!
//! ## Contract
//!
//! Adapters MUST:
//!
//! * Accept a typed `program` + `args` pair (never re-parse a shell
//!   string into argv).
//! * Build the supervisor invocation as typed argv internally; if the
//!   pane runtime requires a string keystroke (e.g. tmux `send-keys`),
//!   shell-quote at the adapter boundary using a tested helper.
//! * Never silently fall back to the unwrapped command. If wrapping is
//!   impossible (e.g. binary path not resolvable), surface a typed
//!   [`ToolLaunchInstrumentationError`] so the caller can decide
//!   whether to abort or surface the error to the user — the use case
//!   never invents supervisor coverage that does not exist.

use crate::domain::UnitId;

/// Typed inputs needed to compose a supervised tool launch.
///
/// The caller provides the user-facing `program` + `args` directly
/// rather than a pre-built shell string. This keeps argv typed all the
/// way to the supervisor process boundary; the adapter is the only
/// layer that may shell-quote, and only when the underlying runtime
/// cannot accept argv (today: tmux `send-keys`).
#[derive(Debug, Clone)]
pub struct WrapToolCommandRequest {
    /// Isolation group (tmux server name in the canonical adapter).
    pub group: String,
    /// Workspace name (tmux session name in the canonical adapter).
    pub workspace: String,
    /// Logical unit identifier (the same id observed by
    /// [`crate::ports::PaneSupervisorPort`]).
    pub unit_id: UnitId,
    /// Tool display name to record in the supervisor metadata's
    /// `command_basename`. `None` is allowed for inline-command
    /// launches; in that case the adapter derives a basename from
    /// `program` so the metadata is still usable to the merge policy.
    pub tool_name: Option<String>,
    /// Optional resolved program path (e.g. `/usr/local/bin/claude`).
    /// Surfaced verbatim in supervisor metadata; not strictly required
    /// to launch the child because `program` already names the
    /// executable Workhive should exec.
    pub program_path: Option<String>,
    /// The user-facing program — the executable Workhive intends to
    /// run inside the pane (NOT the supervisor binary). Must be
    /// non-empty.
    pub program: String,
    /// The user-facing program arguments, in order.
    pub args: Vec<String>,
}

/// Errors produced by [`ToolLaunchInstrumentationPort`].
#[derive(Debug, thiserror::Error)]
pub enum ToolLaunchInstrumentationError {
    /// Adapter could not locate or describe its supervisor binary.
    /// Callers MUST fail closed: an unsupervised tool launch would
    /// silently degrade the highest-confidence observation source.
    #[error("supervisor binary unavailable: {detail}")]
    SupervisorBinaryUnavailable { detail: String },
}

/// Port that knows how to wrap a tool command in a `workhive
/// internal pane-supervisor` invocation.
///
/// The returned string is the exact text the runtime adapter should
/// hand to its keystroke API (e.g. tmux `send-keys`); it is already
/// shell-safe. The adapter, not the use case, owns this transformation.
pub trait ToolLaunchInstrumentationPort {
    /// Build the shell command string the pane shell should execute
    /// to launch the supervised tool. The returned string MUST be
    /// safe to pass to a user shell verbatim (already quoted).
    fn wrap_tool_command(
        &self,
        request: WrapToolCommandRequest,
    ) -> Result<String, ToolLaunchInstrumentationError>;
}

//! Hidden `workhive internal` runtime command definitions (hooks,
//! supervisors, completion candidates, and activity runtime).

use clap::{Subcommand, ValueEnum};

use super::IntegrationProviderArg;

/// Internal runtime subcommands (hidden from default help).
#[derive(Subcommand, Debug)]
pub enum InternalAction {
    /// AI activity recording entrypoint invoked by installed hook scripts
    /// and OpenCode plugins. The hook/plugin writes a JSON
    /// [`workhive_core::usecases::ai_activity::ActivityRecordInput`]
    /// payload to stdin; this command validates correlation against
    /// `WORKHIVE_TOOL_PROVIDER` and persists the resulting fact.
    Activity {
        #[command(subcommand)]
        action: InternalActivityAction,
    },
    /// Read-only candidate query for shell tab-completion.
    ///
    /// Invoked by Workhive's generated completion scripts (bash/zsh/fish)
    /// to obtain dynamic candidates such as workspace names and pane
    /// targets. Output is one candidate per line on stdout. Errors are
    /// swallowed and surfaced as an empty candidate list to keep the
    /// shell's completion UI quiet.
    Completion {
        #[command(subcommand)]
        action: InternalCompletionAction,
    },
    /// Pane runtime control plane supervisor.
    ///
    /// Spawned by Workhive itself (e.g. `workspace open` or the lazy
    /// self-heal path inside `pane send`) to host the typed UDS
    /// supervisor for the (project-root, server, workspace) tuple it is
    /// invoked with. The child process owns one supervisor for that
    /// tuple and exits cleanly when its parent goes away.
    ControlPlane {
        #[command(subcommand)]
        action: InternalControlPlaneAction,
    },
    /// Bounded post-attach pane-title guard.
    ///
    /// Spawned by `workspace open` immediately before `tmux attach` so the
    /// child can reassert Workhive-owned pane titles for a short window
    /// while the user-facing tool (Codex, OpenCode, lazygit, …) settles.
    /// Stops automatically once the duration elapses or the session goes
    /// away — never runs unbounded.
    PaneTitleGuard {
        /// Runtime backend identifier (e.g. `tmux`).
        #[arg(long, default_value = "tmux")]
        backend: String,
        /// Runtime scope identifier (for example, a tmux server label).
        #[arg(long)]
        scope_id: String,
        /// Opaque runtime workspace identifier.
        #[arg(long)]
        workspace_id: String,
        /// `unit_id=title` reassertion entry. Repeat for each managed pane.
        /// Title may be empty to clear; missing `=` is a parse error.
        #[arg(long = "pane", value_name = "UNIT=TITLE")]
        pane: Vec<String>,
        /// Reassertion interval in milliseconds. Default 500.
        #[arg(long, default_value_t = 500)]
        interval_ms: u64,
        /// Maximum total duration the guard may run, in milliseconds.
        /// Hard upper bound is enforced by the handler.
        #[arg(long, default_value_t = 8000)]
        duration_ms: u64,
    },
    /// Pane runtime supervisor — wraps a user-facing tool process so that
    /// Workhive owns the highest-confidence observation source.
    ///
    /// Workhive itself spawns this command (it is not user-facing) when it
    /// needs to launch a tracked tool inside a pane: the supervisor process
    /// becomes the pane's PID, writes authoritative
    /// supervisor metadata under
    /// `<project>/.workhive/runtime/<group>/<workspace>/panes/<unit>.json`,
    /// then `exec`s the wrapped child. When the child exits, the supervisor
    /// updates metadata and propagates the child's exit code.
    PaneSupervisor {
        /// Absolute project root, supplied by the upstream
        /// `tool_launch_instrumentation` adapter so the supervisor
        /// can resolve the **project-over-global** AI integrations
        /// feature gate identical to the one used by
        /// `workhive pane list` / `workhive pane show`.
        ///
        /// Optional for backward compatibility with externally
        /// constructed pane-supervisor invocations. When omitted, the supervisor
        /// falls back to global-only gate resolution — and treats
        /// any global-config load failure as `Unavailable`
        /// (fail-closed), so a missing flag never causes the
        /// supervisor to silently fail open.
        #[arg(long)]
        project_root: Option<String>,
        /// Absolute project runtime root
        /// (`<project>/.workhive/runtime`).
        #[arg(long)]
        runtime_root: String,
        /// Isolation group (the tmux server label `-L`).
        #[arg(long)]
        group: String,
        /// Workspace name (the tmux session name).
        #[arg(long)]
        workspace: String,
        /// Logical unit identifier. Native runtimes that assign pane IDs at
        /// process creation may provide it through their process context.
        #[arg(long)]
        unit: Option<String>,
        /// Tool name (catalog key). When omitted, the supervisor derives a
        /// command basename from the wrapped program.
        #[arg(long)]
        tool: Option<String>,
        /// Resolved program path for the wrapped tool, when known.
        #[arg(long)]
        program_path: Option<String>,
        /// Environment variables to inject into the child process at
        /// spawn time (`KEY=VALUE` pairs). Repeat for each variable.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        env_vars: Vec<String>,
        /// Wrapped program (argv[0]) followed by its arguments. Everything
        /// after `--` is treated as typed argv for the child process. The
        /// supervisor refuses to run without at least the wrapped program,
        /// so we mark this as `required = true` to surface the error at
        /// `clap` parse time rather than letting an empty `argv` slip
        /// through to `build_supervisor_run`.
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 1..,
            required = true,
        )]
        argv: Vec<String>,
    },
    /// Hidden: background reconciliation worker.
    ///
    /// Spawned by the foreground CLI after a trusted-root mutation to rebuild
    /// the project index. Acquires an inter-process lock, scans configured
    /// roots, and atomically publishes a fresh index snapshot if the revision
    /// is still current. Exits silently on contention or stale revision.
    Reconcile {
        /// Global config directory path (used to locate state files and the
        /// project index).
        #[arg(long)]
        config_dir: String,
    },
}

/// Hidden control-plane supervisor lifecycle actions.
#[derive(Subcommand, Debug)]
pub enum InternalControlPlaneAction {
    /// Run the pane runtime control plane supervisor (UDS) until its
    /// listener is shut down.
    ///
    /// This handler is intentionally minimal: it constructs the supervisor
    /// from `project_root`, `server`, and `workspace`, binds the UDS, and
    /// blocks on `serve_blocking`. Lifecycle (parent watchdog, idle
    /// timeout, log file path) is supplied by the caller via the flags
    /// below.
    Serve {
        /// Absolute project root path.
        #[arg(long)]
        project_root: String,
        /// Runtime server name (tmux `-L`).
        #[arg(long)]
        server: String,
        /// Workspace name this supervisor coordinates.
        #[arg(long)]
        workspace: String,
        /// Optional parent process pid to watch; supervisor exits cleanly
        /// once the parent disappears. When `0` or absent, the supervisor
        /// runs until SIGINT / SIGTERM.
        #[arg(long, default_value_t = 0)]
        parent_pid: i32,
    },
}

/// Hidden completion-query actions.
#[derive(Subcommand, Debug)]
pub enum InternalCompletionAction {
    /// Print one candidate per line for the requested kind.
    Candidates {
        /// What candidates the shell is asking for.
        #[arg(value_enum)]
        kind: CompletionCandidateKind,
    },
}

/// Kinds of dynamic completion candidates Workhive can resolve.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum CompletionCandidateKind {
    /// Workspace names under the current project's `.workhive/worktrees/`.
    Workspace,
    /// Pane targets in the current workspace's runtime.
    Pane,
}

/// Internal AI activity recording subcommands.
///
/// These are not part of the public CLI surface. Installed hook scripts
/// and OpenCode plugins invoke them with a JSON payload on stdin
/// describing a single provider lifecycle event:
///
/// - [`Record`](InternalActivityAction::Record) accepts the Workhive
///   canonical [`workhive_core::usecases::ai_activity::ActivityRecordInput`]
///   shape and is used by internal tests and by the OpenCode plugin
///   after it has translated its event into the canonical schema.
/// - [`Hook`](InternalActivityAction::Hook) accepts the *official*
///   provider stdin payload (e.g. Claude Code hook JSON containing
///   `hook_event_name`, Codex hook JSON likewise, or OpenCode plugin
///   event JSON forwarded via stdin) and adapts the documented event
///   names into the canonical fact before persisting it. This is the
///   entry point our generated hook shell scripts pipe stdin into so
///   the scripts themselves never have to inspect undocumented
///   environment variables.
#[derive(Subcommand, Debug)]
pub enum InternalActivityAction {
    /// Record a single AI activity fact from JSON stdin using the
    /// Workhive canonical schema.
    Record,
    /// Parse a provider-native hook payload from stdin and persist
    /// the resulting activity fact. The `--provider` flag selects
    /// which official payload schema to interpret.
    Hook {
        /// Provider whose official stdin payload schema to apply.
        #[arg(long, value_enum)]
        provider: IntegrationProviderArg,
    },
    /// Record a Codex `notify` callback completion event into the
    /// activity control plane event store.
    ///
    /// This is the hidden production endpoint invoked by the
    /// Workhive-managed Codex `notify` shell wrapper installed under
    /// `~/.codex/config.toml`. It receives no stdin (Codex only
    /// passes positional arguments to its `notify` command) and
    /// resolves the trusted correlation tuple from the
    /// `WORKHIVE_*` environment variables that the pane supervisor
    /// injected when spawning Codex.
    ///
    /// The handler appends exactly one ACP event with
    /// [`SignalChannel::NotifyCallback`] +
    /// [`ProviderEventKind::NotifyCompletion`] (medium confidence)
    /// and never produces `started` / `working` / `tool-running` /
    /// `permission-needed` events: those would violate the Phase-1
    /// invariant pinned in the normalizer.
    ///
    /// Fail-open: any internal error (missing env, append failure,
    /// gate refusal) is logged at `debug`/`warn` and the process
    /// exits `0` so the upstream Codex turn is never blocked.
    ///
    /// [`SignalChannel::NotifyCallback`]: workhive_core::domain::activity::SignalChannel::NotifyCallback
    /// [`ProviderEventKind::NotifyCompletion`]: workhive_core::domain::activity::ProviderEventKind::NotifyCompletion
    CodexNotify {
        /// Trailing positional arguments forwarded by Codex's
        /// `notify` invocation. Captured for completeness but the
        /// handler does NOT inspect them: a notify callback is a
        /// completion-only signal regardless of the arguments Codex
        /// supplies.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

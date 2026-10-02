//! Tmux CLI adapter — all tmux operations via `Command::arg()`.
//!
//! This module owns:
//! - The `TmuxPort` trait (infra-level, not exported by `workhive-core`)
//! - All tmux-specific DTOs and request types
//! - `TmuxCliAdapter`: the real tmux CLI implementation
//! - `classify_tmux_error`: the single stderr parsing point
//!
//! `workhive-core` use cases depend on `WorkspaceRuntimePort` / `PaneRuntimePort`
//! from `workhive-core::ports::runtime`. `TmuxPort` is an infra implementation
//! detail wired together in `workhive-cli`.

use std::process::Command;

use camino::Utf8Path;
use workhive_core::domain::PaneId;

use crate::tmux_errors::TmuxError;

mod output;
mod types;

pub use types::{
    AttachRequest, CapturePaneRequest, CreateSessionRequest, PaneInfo, PaneTopologyInfo,
    SendKeysRequest, SessionInfo, SplitPaneRequest, WindowInfo,
};

#[allow(unused_imports)]
use output::*;

/// Session options applied regardless of interaction policy.
const BASE_SESSION_OPTIONS: &[(&str, &str)] = &[
    ("status-position", "bottom"),
    ("status-style", "bg=colour235,fg=colour250"),
    ("status-left-length", "60"),
    ("status-left", "[#{session_name}] "),
    ("status-right", "#{pane_title} | %H:%M %d-%b-%y"),
    (
        "window-status-current-format",
        "#[fg=colour51,bold]#{window_index}:#{pane_title}#[fg=default]*",
    ),
    ("window-status-format", "#{window_index}:#{pane_title}"),
    ("pane-border-style", "fg=colour238"),
    ("pane-active-border-style", "fg=colour51"),
    ("pane-border-format", " #{pane_index} #{pane_title} "),
    ("pane-border-status", "top"),
    ("mode-keys", "vi"),
    ("history-limit", "50000"),
    ("display-time", "2000"),
    // Use tmux-256color for better scrolling performance.
    // screen-256color causes visible flicker with rapid output (e.g., AI tools).
    ("default-terminal", "tmux-256color"),
    ("allow-rename", "off"),
    ("automatic-rename", "off"),
    ("set-titles", "off"),
];

/// Terminal features applied to declare terminal capabilities.
/// Always installs the superset (with clipboard) since `set-clipboard`
/// per-session controls actual clipboard behavior.
///
/// Note: `focus` is intentionally excluded from the feature list. Focus events
/// trigger additional redraw cycles that cause visible flicker during rapid
/// output (e.g., AI tool streaming). Users who need focus events can enable
/// them via `~/.tmux.conf` with `set -g focus-events on`.
const CLIPBOARD_TERMINAL_FEATURES: &[(&str, &str)] = &[
    (
        "terminal-features[0]",
        "xterm*:clipboard:ccolour:cstyle:title:RGB",
    ),
    ("terminal-features[1]", "screen*:clipboard:RGB"),
    ("terminal-features[2]", "tmux*:clipboard:RGB"),
];

/// Workhive-owned copy-mode binding keys. Used for both installation
/// (clipboard=true) and removal (clipboard=false).
const WORKHIVE_COPY_MODE_KEYS: &[&str] = &["y", "MouseDragEnd1Pane"];

struct CopyModeBinding {
    key: &'static str,
    command: [&'static str; 3],
}

const COPY_MODE_BINDINGS: &[CopyModeBinding] = &[
    CopyModeBinding {
        key: "y",
        command: ["send-keys", "-X", "copy-selection-and-cancel"],
    },
    CopyModeBinding {
        key: "MouseDragEnd1Pane",
        command: ["send-keys", "-X", "copy-selection-and-cancel"],
    },
];

// ─── Typed session-configuration planner ─────────────────────────────────

/// A single tmux operation to execute during session configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionConfigOp {
    /// `set-option -t <session> <key> <value>`
    SetSessionOption {
        key: &'static str,
        value: &'static str,
    },
    /// `set-option -s <key> <value>` (server-level)
    SetServerOption {
        key: &'static str,
        value: &'static str,
    },
    /// `bind-key -T copy-mode-vi <key> <cmd...>`
    BindCopyModeKey {
        key: &'static str,
        command: [&'static str; 3],
    },
    /// `unbind-key -T copy-mode-vi <key>`
    UnbindCopyModeKey { key: &'static str },
    /// `set-option -g @workhive_managed_<key> installed`
    SetWorkhiveMarker { key: &'static str },
    /// `set-option -gu @workhive_managed_<key>`
    ClearWorkhiveMarker { key: &'static str },
}

/// The complete plan for configuring a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionConfigPlan {
    pub ops: Vec<SessionConfigOp>,
}

/// Per-key binding ownership determined by marker + binding state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingOwnership {
    /// No binding and no marker exist for this key.
    Absent,
    /// Workhive marker is set AND binding command matches the managed
    /// command.
    WorkhiveOwned,
    /// Binding exists but no Workhive marker — user or tmux default.
    UserOwned,
    /// Workhive marker exists but the binding command was modified by the
    /// user (or the binding was removed). The marker is stale.
    StaleMarker,
}

/// Build a deterministic configuration plan from an interaction policy.
///
/// `binding_state` provides per-key ownership for each managed key,
/// used on both the enable and disable clipboard paths.
pub(crate) fn plan_session_config(
    policy: &workhive_core::ports::RuntimeInteractionPolicy,
    binding_state: &[(&'static str, BindingOwnership)],
    terminal_features_installed: bool,
) -> SessionConfigPlan {
    let mut ops = Vec::new();

    for &(key, value) in BASE_SESSION_OPTIONS {
        ops.push(SessionConfigOp::SetSessionOption { key, value });
    }

    ops.push(SessionConfigOp::SetSessionOption {
        key: "mouse",
        value: if policy.mouse_enabled { "on" } else { "off" },
    });

    // Terminal-features is a server-global option declaring terminal
    // capabilities. On a shared server, always install the superset
    // (clipboard-capable) regardless of this session's clipboard policy.
    // Per-session `set-clipboard` controls actual clipboard behavior.
    if !terminal_features_installed {
        for &(key, value) in CLIPBOARD_TERMINAL_FEATURES {
            ops.push(SessionConfigOp::SetServerOption { key, value });
        }
        ops.push(SessionConfigOp::SetWorkhiveMarker {
            key: "terminal_features",
        });
    }

    if policy.clipboard_enabled {
        ops.push(SessionConfigOp::SetSessionOption {
            key: "set-clipboard",
            value: "external",
        });
        for &(key, ownership) in binding_state {
            let binding = COPY_MODE_BINDINGS.iter().find(|b| b.key == key);
            match ownership {
                BindingOwnership::Absent => {
                    if let Some(b) = binding {
                        ops.push(SessionConfigOp::BindCopyModeKey {
                            key: b.key,
                            command: b.command,
                        });
                        ops.push(SessionConfigOp::SetWorkhiveMarker { key });
                    }
                }
                BindingOwnership::UserOwned => {
                    // Preserve user binding — do not overwrite.
                }
                BindingOwnership::WorkhiveOwned => {
                    if let Some(b) = binding {
                        ops.push(SessionConfigOp::BindCopyModeKey {
                            key: b.key,
                            command: b.command,
                        });
                    }
                }
                BindingOwnership::StaleMarker => {
                    ops.push(SessionConfigOp::ClearWorkhiveMarker { key });
                }
            }
        }
    } else {
        ops.push(SessionConfigOp::SetSessionOption {
            key: "set-clipboard",
            value: "off",
        });
        for &(key, ownership) in binding_state {
            match ownership {
                BindingOwnership::WorkhiveOwned => {
                    ops.push(SessionConfigOp::UnbindCopyModeKey { key });
                    ops.push(SessionConfigOp::ClearWorkhiveMarker { key });
                }
                BindingOwnership::StaleMarker => {
                    ops.push(SessionConfigOp::ClearWorkhiveMarker { key });
                }
                BindingOwnership::Absent | BindingOwnership::UserOwned => {}
            }
        }
    }

    SessionConfigPlan { ops }
}

/// The Workhive-managed copy-mode-vi command string used to verify
/// marker-plus-command consistency.
const WORKHIVE_MANAGED_COPY_COMMAND: &str = "send-keys -X copy-selection-and-cancel";

/// Build the tmux user-option name used to mark Workhive ownership of a
/// copy-mode-vi key binding.
fn workhive_marker_option(key: &str) -> String {
    format!("@workhive_managed_{key}")
}

fn is_missing_key_diagnostic(message: &str, key: &str) -> bool {
    let trimmed = message.trim();
    trimmed == format!("invalid key: {key}") || trimmed == format!("unknown key: {key}")
}

/// Determine per-key binding ownership using the marker + binding probe.
///
/// Uses `runner` for tmux commands so production and tests share the
/// same logic. Returns `Err` on real tmux failures; only a clear
/// "not found" response is mapped to `Absent`.
pub(crate) fn detect_binding_state(
    server: &str,
    runner: &dyn Fn(&[&str]) -> Result<String, TmuxError>,
) -> Result<Vec<(&'static str, BindingOwnership)>, TmuxError> {
    let mut result = Vec::new();
    for &key in WORKHIVE_COPY_MODE_KEYS {
        let marker_name = workhive_marker_option(key);
        // Quiet query: `-gqv` returns empty on missing option instead of error.
        let has_marker = !runner(&["-L", server, "show-options", "-gqv", &marker_name])?
            .trim()
            .is_empty();

        let binding_output = match runner(&["-L", server, "list-keys", "-T", "copy-mode-vi", key]) {
            Ok(output) => Some(output),
            Err(TmuxError::CommandFailed { ref message })
                if is_missing_key_diagnostic(message, key) =>
            {
                None
            }
            Err(e) => return Err(e),
        };

        let ownership = match (has_marker, &binding_output) {
            (true, Some(output)) if output.contains(WORKHIVE_MANAGED_COPY_COMMAND) => {
                BindingOwnership::WorkhiveOwned
            }
            (true, _) => BindingOwnership::StaleMarker,
            (false, Some(_)) => BindingOwnership::UserOwned,
            (false, None) => BindingOwnership::Absent,
        };
        result.push((key, ownership));
    }
    Ok(result)
}

// ─── TmuxPort trait ────────────────────────────────────────────────────────

/// Low-level abstraction over tmux operations.
///
/// This trait is **infra-only**. `workhive-core` use cases must not depend on
/// it directly — they use `WorkspaceRuntimePort` and `PaneRuntimePort` instead.
///
/// Implementations: `TmuxCliAdapter` (real), `FakeTmux` in `workhive-testkit`.
/// The bridge from `TmuxPort` → runtime-neutral ports is `TmuxRuntimeAdapter`.
pub trait TmuxPort {
    /// Check if a session exists on the given server.
    fn has_session(&self, server: &str, session: &str) -> Result<bool, TmuxError>;

    /// Create a new session.
    fn create_session(&self, request: CreateSessionRequest) -> Result<(), TmuxError>;

    /// Apply session options (mouse, clipboard, terminal features, etc.)
    /// driven by the resolved interaction policy.
    fn configure_session(
        &self,
        server: &str,
        session: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), TmuxError>;

    /// Attach the current terminal to a session.
    fn attach(&self, request: AttachRequest) -> Result<(), TmuxError>;

    /// Switch the current tmux client to a different session on the same server.
    fn switch_client(&self, server: &str, target_session: &str) -> Result<(), TmuxError>;

    /// Detach the current client from the specified session.
    fn detach_client(&self, server: &str, session: &str) -> Result<(), TmuxError>;

    /// Rename a session without changing its panes or attached clients.
    fn rename_session(&self, server: &str, session: &str, new_name: &str) -> Result<(), TmuxError>;

    /// Kill (destroy) a session.
    fn kill_session(&self, server: &str, session: &str) -> Result<(), TmuxError>;

    /// List all sessions on a server.
    fn list_sessions(&self, server: &str) -> Result<Vec<SessionInfo>, TmuxError>;

    /// Kill the entire tmux server.
    fn kill_server(&self, server: &str) -> Result<(), TmuxError>;

    /// Split a pane.
    fn split_pane(&self, request: SplitPaneRequest) -> Result<PaneInfo, TmuxError>;

    /// Close a specific pane.
    fn close_pane(&self, server: &str, pane_id: &PaneId) -> Result<(), TmuxError>;

    /// List all panes in a session.
    fn list_panes(&self, server: &str, session: &str) -> Result<Vec<PaneInfo>, TmuxError>;

    /// Get information about the current pane.
    fn current_pane(&self, server: &str, session: &str) -> Result<PaneInfo, TmuxError>;

    /// Send keystrokes to a pane.
    fn send_keys(&self, request: SendKeysRequest) -> Result<(), TmuxError>;

    /// Capture the visible text of a pane through `tmux capture-pane -p`.
    ///
    /// Returns the raw captured text (no trimming) so callers can decide
    /// whether to treat empty buffers as "no signal" in their own policy.
    /// Adapters MUST go through `Command::arg()` and reuse the same error
    /// classification path as the rest of `TmuxPort`.
    ///
    /// This is the typed boundary used by the terminal-status fallback in
    /// the AI activity observation ladder. New observers MUST go through
    /// this verb instead of constructing `Command::new("tmux")` themselves
    /// so all tmux interaction stays auditable behind one port. See
    /// the tmux adapter boundary rule.
    fn capture_pane_text(&self, request: CapturePaneRequest) -> Result<String, TmuxError>;

    /// Set the title of a specific pane (used for display in pane borders / status bar).
    fn set_pane_title(&self, server: &str, pane_id: &PaneId, title: &str) -> Result<(), TmuxError>;

    /// List all windows in a session with layout metadata.
    fn list_windows(&self, server: &str, session: &str) -> Result<Vec<WindowInfo>, TmuxError>;

    /// List detailed pane topology in a session.
    fn list_panes_topology(
        &self,
        server: &str,
        session: &str,
    ) -> Result<Vec<PaneTopologyInfo>, TmuxError>;

    /// Create a new window and return its initial pane.
    fn create_window(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
        name: Option<&str>,
        cwd: &Utf8Path,
    ) -> Result<PaneTopologyInfo, TmuxError>;

    /// Apply a layout string to a specific window.
    fn apply_layout(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
        layout: &str,
    ) -> Result<(), TmuxError>;

    /// Select active window.
    fn select_window(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
    ) -> Result<(), TmuxError>;

    /// Select active pane.
    fn select_pane(&self, server: &str, pane_id: &PaneId) -> Result<(), TmuxError>;

    /// Respawn an existing pane in place at a new working directory.
    ///
    /// Implementations MUST translate this to `tmux respawn-pane -k -c <cwd>
    /// -t <pane>` so the pane keeps its tmux pane id (`%N`) and layout slot
    /// while restarting in the requested directory. The currently running
    /// process inside the pane is killed (`-k`) only when the caller is about
    /// to launch a new command.
    /// No visible `cd -- ...` keystroke is permitted; this is the canonical
    /// "infra-only" path for fixing existing-pane cwd.
    fn respawn_pane(&self, server: &str, pane_id: &PaneId, cwd: &Utf8Path)
    -> Result<(), TmuxError>;

    /// Respawn a pane in place at `cwd` and start `command` directly as the
    /// pane's new process.
    ///
    /// Implementations MUST translate this to
    /// `tmux respawn-pane -k -c <cwd> -t <pane> <command>` so the pane keeps
    /// its tmux pane id (`%N`) and layout slot while replacing whatever
    /// process is running inside it with `command`. tmux runs `command`
    /// internally through `/bin/sh -c`, which means the wrapper text never
    /// reaches the pane shell as visible input — this is the canonical
    /// "infra-only" path for installing the pane supervisor wrapper during
    /// workspace open.
    fn respawn_pane_with_command(
        &self,
        server: &str,
        pane_id: &PaneId,
        cwd: &Utf8Path,
        command: &str,
    ) -> Result<(), TmuxError>;

    /// Set or clear a tmux hook.
    fn set_hook(
        &self,
        server: &str,
        session: &str,
        hook_name: &str,
        command: Option<&str>,
    ) -> Result<(), TmuxError>;
}

// ─── TmuxCliAdapter ───────────────────────────────────────────────────────

/// Tmux adapter using the system `tmux` binary.
pub struct TmuxCliAdapter;

impl TmuxCliAdapter {
    pub fn new() -> Self {
        Self
    }

    fn run_tmux(args: &[&str]) -> Result<String, TmuxError> {
        tracing::debug!(args = ?args, "running tmux command");
        let output = Command::new("tmux")
            .args(args)
            .output()
            .map_err(|_| TmuxError::TmuxNotFound)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let typed = classify_tmux_error(&stderr, args);
            match &typed {
                TmuxError::ServerNotRunning { .. } | TmuxError::SocketNotFound { .. } => {
                    tracing::debug!(stderr = %stderr, "tmux server not running (benign)");
                }
                TmuxError::SessionNotFound { .. } if is_has_session_probe(args) => {
                    tracing::debug!(
                        stderr = %stderr,
                        "tmux session not found during existence probe (benign)"
                    );
                }
                TmuxError::CommandFailed { message }
                    if args.windows(2).any(|pair| pair == ["list-keys", "-T"])
                        && args
                            .last()
                            .is_some_and(|key| is_missing_key_diagnostic(message, key)) =>
                {
                    tracing::debug!(
                        stderr = %stderr,
                        "tmux key binding not found during ownership probe (benign)"
                    );
                }
                TmuxError::PermissionDenied { .. } => {
                    tracing::warn!(stderr = %stderr, "tmux permission denied");
                }
                _ => {
                    tracing::warn!(stderr = %stderr, "tmux command failed");
                }
            }
            Err(typed)
        }
    }

    /// Execute a session configuration plan via the real tmux CLI.
    fn execute_session_config_plan(
        &self,
        server: &str,
        session: &str,
        plan: &SessionConfigPlan,
    ) -> Result<(), TmuxError> {
        Self::execute_plan_with_runner(server, session, plan, &|args| Self::run_tmux(args))
    }

    /// Switch client using the provided command runner. Shared between
    /// production (`Self::run_tmux`) and tests (fake runner).
    fn switch_client_with_runner(
        server: &str,
        target_session: &str,
        runner: &dyn Fn(&[&str]) -> Result<String, TmuxError>,
    ) -> Result<(), TmuxError> {
        runner(&["-L", server, "switch-client", "-t", target_session])?;
        Ok(())
    }

    /// List sessions and enrich them with pane counts using the provided
    /// command runner. Keeping the query orchestration injectable makes the
    /// tmux contract testable without starting a real server.
    fn list_sessions_with_runner(
        server: &str,
        runner: &dyn Fn(&[&str]) -> Result<String, TmuxError>,
    ) -> Result<Vec<SessionInfo>, TmuxError> {
        let sessions_output = runner(&[
            "-L",
            server,
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_windows}\t#{session_attached}",
        ])?;

        // `list-panes` reports no rows when the last session disappears
        // between the two probes. Returning the session metadata with a zero
        // count lets the core layer degrade that enrichment to `Unknown`.
        if sessions_output.trim().is_empty() {
            return Ok(Vec::new());
        }

        let panes_output = runner(&["-L", server, "list-panes", "-a", "-F", "#{session_name}"])?;

        Ok(parse_sessions_with_pane_counts(
            &sessions_output,
            &panes_output,
        ))
    }

    /// Execute a plan using the provided command runner. Shared between
    /// production (`Self::run_tmux`) and tests (fake runner).
    fn execute_plan_with_runner(
        server: &str,
        session: &str,
        plan: &SessionConfigPlan,
        runner: &dyn Fn(&[&str]) -> Result<String, TmuxError>,
    ) -> Result<(), TmuxError> {
        for op in &plan.ops {
            match op {
                SessionConfigOp::SetSessionOption { key, value } => {
                    runner(&["-L", server, "set-option", "-t", session, key, value])?;
                }
                SessionConfigOp::SetServerOption { key, value } => {
                    runner(&["-L", server, "set-option", "-s", key, value])?;
                }
                SessionConfigOp::BindCopyModeKey { key, command } => {
                    runner(&[
                        "-L",
                        server,
                        "bind-key",
                        "-T",
                        "copy-mode-vi",
                        key,
                        command[0],
                        command[1],
                        command[2],
                    ])?;
                }
                SessionConfigOp::UnbindCopyModeKey { key } => {
                    runner(&["-L", server, "unbind-key", "-T", "copy-mode-vi", key])?;
                }
                SessionConfigOp::SetWorkhiveMarker { key } => {
                    let marker = workhive_marker_option(key);
                    runner(&["-L", server, "set-option", "-g", &marker, "installed"])?;
                }
                SessionConfigOp::ClearWorkhiveMarker { key } => {
                    let marker = workhive_marker_option(key);
                    runner(&["-L", server, "set-option", "-gu", &marker])?;
                }
            }
        }
        tracing::debug!(
            server,
            session,
            ops = plan.ops.len(),
            "tmux session configured"
        );
        Ok(())
    }
}

fn build_attach_command(request: &AttachRequest) -> Command {
    let mut command = Command::new("tmux");
    command.args([
        "-L",
        &request.server_name,
        "attach-session",
        "-t",
        &request.session_name,
    ]);
    command
}

fn is_has_session_probe(args: &[&str]) -> bool {
    args.contains(&"has-session")
}

impl Default for TmuxCliAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl TmuxPort for TmuxCliAdapter {
    fn has_session(&self, server: &str, session: &str) -> Result<bool, TmuxError> {
        // tmux otherwise accepts a unique session-name prefix, which makes a
        // staged `<canonical>--rebuild-*` session look like the canonical
        // session already exists during transactional promotion.
        let exact_session = format!("={session}");
        let result = Self::run_tmux(&["-L", server, "has-session", "-t", &exact_session]);
        match result {
            Ok(_) => Ok(true),
            Err(TmuxError::SessionNotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }

    fn create_session(&self, request: CreateSessionRequest) -> Result<(), TmuxError> {
        // Pre-build owned strings for env arguments so the &str slice we hand
        // tmux remains valid for the entire call. Each pair becomes
        // `-e KEY=VALUE`. tmux propagates these to all child processes of
        // the new session, so every pane inherits the resolved `.env`
        // overlay without per-pane injection.
        let env_pairs: Vec<String> = request
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let mut args: Vec<&str> = vec![
            "-L",
            &request.server_name,
            "new-session",
            "-d",
            "-s",
            &request.session_name,
            "-c",
            request.working_dir.as_str(),
        ];
        for pair in &env_pairs {
            args.push("-e");
            args.push(pair.as_str());
        }

        Self::run_tmux(&args)?;

        // Verify the session actually exists — tmux may report exit 0 while
        // emitting errors on stderr in some environments.
        let exists = self.has_session(&request.server_name, &request.session_name)?;
        if !exists {
            return Err(TmuxError::SessionCreateNotVerified {
                server: request.server_name,
                name: request.session_name,
            });
        }
        Ok(())
    }

    /// Configure tmux session options including status bar and pane border display.
    ///
    /// `#{pane_title}` in tmux formats is **Workhive-owned**: Workhive explicitly
    /// sets each pane's title via `select-pane -T` after layout operations.
    /// The value comes from config/launch identity, never from volatile runtime
    /// state. Programs that change `pane_title` via ANSI escapes will be
    /// overwritten on the next Workhive operation.
    fn configure_session(
        &self,
        server: &str,
        session: &str,
        policy: &workhive_core::ports::RuntimeInteractionPolicy,
    ) -> Result<(), TmuxError> {
        let binding_state = detect_binding_state(server, &|args| Self::run_tmux(args))?;
        let tf_installed = Self::run_tmux(&[
            "-L",
            server,
            "show-options",
            "-gqv",
            "@workhive_managed_terminal_features",
        ])
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
        let plan = plan_session_config(policy, &binding_state, tf_installed);
        self.execute_session_config_plan(server, session, &plan)
    }

    fn attach(&self, request: AttachRequest) -> Result<(), TmuxError> {
        use std::process::Stdio;

        let status = build_attach_command(&request)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|_| TmuxError::TmuxNotFound)?;

        if status.success() {
            Ok(())
        } else {
            let check = Self::run_tmux(&[
                "-L",
                &request.server_name,
                "has-session",
                "-t",
                &request.session_name,
            ]);
            match check {
                Err(TmuxError::CommandFailed { .. }) => Err(TmuxError::SessionNotFound {
                    name: request.session_name,
                }),
                _ => Err(TmuxError::CommandFailed {
                    message: format!(
                        "attach failed with exit code {}",
                        status.code().unwrap_or(-1)
                    ),
                }),
            }
        }
    }

    fn switch_client(&self, server: &str, target_session: &str) -> Result<(), TmuxError> {
        Self::switch_client_with_runner(server, target_session, &|args| Self::run_tmux(args))
    }

    fn detach_client(&self, server: &str, session: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "detach-client", "-s", session])?;
        Ok(())
    }

    fn rename_session(&self, server: &str, session: &str, new_name: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "rename-session", "-t", session, new_name])?;
        Ok(())
    }

    fn kill_session(&self, server: &str, session: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "kill-session", "-t", session])?;
        Ok(())
    }

    fn list_sessions(&self, server: &str) -> Result<Vec<SessionInfo>, TmuxError> {
        Self::list_sessions_with_runner(server, &|args| Self::run_tmux(args))
    }

    fn kill_server(&self, server: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "kill-server"])?;
        Ok(())
    }

    fn split_pane(&self, request: SplitPaneRequest) -> Result<PaneInfo, TmuxError> {
        // Direction model: `Right`/`Down` add new pane after target, `Left`/`Up`
        // add new pane before target (tmux `-b`). Horizontal splits use `-h`,
        // vertical splits use `-v`.
        let dir_flag = if request.direction.is_vertical() {
            "-v"
        } else {
            "-h"
        };
        let before_flag = request.direction.is_before_target();

        let target_str = request
            .target_pane
            .as_ref()
            .map(|p| p.to_string())
            .unwrap_or_else(|| request.session_name.clone());

        let size_str = request.size_percent.map(|p| p.to_string());

        let env_pairs: Vec<String> = request
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        let cwd_str = request.cwd.as_ref().map(|p| p.as_str().to_owned());

        let mut args = vec![
            "-L",
            &request.server_name,
            "split-window",
            dir_flag,
            "-t",
            &target_str,
        ];

        if before_flag {
            args.push("-b");
        }

        if let Some(ref sz) = size_str {
            args.push("-p");
            args.push(sz);
        }

        // `split-window -c <cwd>` sets the initial working directory for the
        // new pane without requiring a visible `cd` keystroke in scrollback.
        // This is the preferred path for all Workhive-prepared panes.
        if let Some(ref c) = cwd_str {
            args.push("-c");
            args.push(c.as_str());
        }

        // Pass `-e KEY=VALUE` so the spawned pane process inherits the
        // configured environment. tmux's `split-window -e` was added in 3.0
        // and is supported on every macOS/Linux baseline this project targets.
        for pair in &env_pairs {
            args.push("-e");
            args.push(pair.as_str());
        }

        args.extend_from_slice(&[
            "-P",
            "-F",
            "#{pane_id}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_active}\t#{pane_tty}\t#{pane_pid}",
        ]);

        let cmd_owned;
        if let Some(ref cmd) = request.command {
            cmd_owned = cmd.clone();
            args.push(&cmd_owned);
        }

        let output = Self::run_tmux(&args)?;
        parse_single_pane(&output).ok_or(TmuxError::CommandFailed {
            message: "failed to parse pane info after split".to_owned(),
        })
    }

    fn close_pane(&self, server: &str, pane_id: &PaneId) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "kill-pane", "-t", pane_id.as_str()])?;
        Ok(())
    }

    fn list_panes(&self, server: &str, session: &str) -> Result<Vec<PaneInfo>, TmuxError> {
        let target = session.to_owned();
        let output = Self::run_tmux(&[
            "-L",
            server,
            "list-panes",
            "-t",
            &target,
            "-F",
            "#{pane_id}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_active}\t#{pane_tty}\t#{pane_pid}",
        ])?;
        Ok(output.lines().filter_map(parse_single_pane).collect())
    }

    fn current_pane(&self, server: &str, session: &str) -> Result<PaneInfo, TmuxError> {
        let output = Self::run_tmux(&[
            "-L",
            server,
            "display-message",
            "-t",
            session,
            "-p",
            "#{pane_id}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_active}\t#{pane_tty}\t#{pane_pid}",
        ])?;
        parse_single_pane(&output).ok_or(TmuxError::PaneNotFound {
            id: "current".to_owned(),
        })
    }

    fn send_keys(&self, request: SendKeysRequest) -> Result<(), TmuxError> {
        // Semantics:
        //   1. `text` is always delivered via `send-keys -l` so every byte —
        //      including leading hyphens, backslashes, `\n`, `\r`, `C-m` — is
        //      treated as a literal keystroke. Mixing literal text and tmux
        //      key names in one invocation would let tmux reinterpret
        //      substrings, breaking round-trip fidelity.
        //   2. When `enter` is requested, `Enter` is submitted as a separate
        //      `send-keys` call so tmux treats it as a key event rather than
        //      as the literal four bytes `E`, `n`, `t`, `e`, `r`.
        //   3. Empty text with `enter == false` is a no-op: no tmux
        //      invocation at all. Empty text with `enter == true` still
        //      submits a single `Enter` key event.
        if !request.text.is_empty() {
            // Terminate the literal-only invocation with `--` so tmux does
            // not attempt to parse leading hyphens in the payload as flags.
            let text_args: [&str; 7] = [
                "-L",
                &request.server_name,
                "send-keys",
                "-t",
                request.target_pane.as_str(),
                "-l",
                "--",
            ];
            let mut args_ref: Vec<&str> = text_args.to_vec();
            args_ref.push(request.text.as_str());
            Self::run_tmux(&args_ref)?;
        }

        if request.enter {
            Self::run_tmux(&[
                "-L",
                &request.server_name,
                "send-keys",
                "-t",
                request.target_pane.as_str(),
                "Enter",
            ])?;
        }

        Ok(())
    }

    fn set_pane_title(&self, server: &str, pane_id: &PaneId, title: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&[
            "-L",
            server,
            "select-pane",
            "-t",
            pane_id.as_str(),
            "-T",
            title,
        ])?;
        Ok(())
    }

    fn capture_pane_text(&self, request: CapturePaneRequest) -> Result<String, TmuxError> {
        // tmux `-p` writes the captured pane contents to stdout. We deliberately
        // do not pass `-J` (rejoin wrapped lines) — keeping the buffer aligned
        // with what the user sees on screen is exactly what the conservative
        // terminal-status policy expects. `-S <line>` lets callers ask for a
        // bounded recent window so we never read the entire scrollback.
        let start_line = request.start_line.to_string();
        let raw = Self::run_tmux(&[
            "-L",
            &request.server_name,
            "capture-pane",
            "-p",
            "-t",
            request.target_pane.as_str(),
            "-S",
            start_line.as_str(),
        ])?;
        Ok(raw)
    }

    fn list_windows(&self, server: &str, session: &str) -> Result<Vec<WindowInfo>, TmuxError> {
        let output = Self::run_tmux(&[
            "-L",
            server,
            "list-windows",
            "-t",
            session,
            "-F",
            "#{window_index}\t#{window_name}\t#{window_layout}\t#{window_active}",
        ])?;
        Ok(parse_windows(&output))
    }

    fn list_panes_topology(
        &self,
        server: &str,
        session: &str,
    ) -> Result<Vec<PaneTopologyInfo>, TmuxError> {
        let output = Self::run_tmux(&[
            "-L",
            server,
            "list-panes",
            "-t",
            session,
            "-F",
            TOPOLOGY_PANE_FORMAT,
        ])?;
        Ok(output.lines().filter_map(parse_topology_pane).collect())
    }

    fn create_window(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
        name: Option<&str>,
        cwd: &Utf8Path,
    ) -> Result<PaneTopologyInfo, TmuxError> {
        let target = format!("{session}:{window_index}");
        let mut args = vec![
            "-L".to_owned(),
            server.to_owned(),
            "new-window".to_owned(),
            "-d".to_owned(),
            "-t".to_owned(),
            target,
            "-c".to_owned(),
            cwd.as_str().to_owned(),
        ];
        if let Some(window_name) = name {
            args.push("-n".to_owned());
            args.push(window_name.to_owned());
        }
        args.push("-P".to_owned());
        args.push("-F".to_owned());
        args.push(TOPOLOGY_PANE_FORMAT.to_owned());

        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = Self::run_tmux(&arg_refs)?;
        parse_topology_pane(&output).ok_or(TmuxError::CommandFailed {
            message: "failed to parse pane info after new-window".to_owned(),
        })
    }

    fn apply_layout(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
        layout: &str,
    ) -> Result<(), TmuxError> {
        let target = format!("{session}:{window_index}");
        Self::run_tmux(&["-L", server, "select-layout", "-t", &target, layout])?;
        Ok(())
    }

    fn select_window(
        &self,
        server: &str,
        session: &str,
        window_index: u32,
    ) -> Result<(), TmuxError> {
        let target = format!("{session}:{window_index}");
        Self::run_tmux(&["-L", server, "select-window", "-t", &target])?;
        Ok(())
    }

    fn select_pane(&self, server: &str, pane_id: &PaneId) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "select-pane", "-t", pane_id.as_str()])?;
        Ok(())
    }

    fn respawn_pane(
        &self,
        server: &str,
        pane_id: &PaneId,
        cwd: &Utf8Path,
    ) -> Result<(), TmuxError> {
        // tmux `respawn-pane -k -c <cwd> -t <pane>` keeps the pane's runtime
        // identifier and layout slot while killing any running process and
        // restarting the default shell in `cwd`. Callers use this before
        // ahead of installing a `Raw` (user inline) launch via the visible
        // `send_input` path, so the inline command is typed into a shell
        // already pointing at the captured cwd. Tool launches do NOT use
        // this verb — they go through `respawn_pane_with_command` below
        // which installs the (already-wrapped) supervisor command directly
        // and carries its own `-c <cwd>`. Killing the residual process is
        // intentional in both cases.
        Self::run_tmux(&[
            "-L",
            server,
            "respawn-pane",
            "-k",
            "-c",
            cwd.as_str(),
            "-t",
            pane_id.as_str(),
        ])?;
        Ok(())
    }

    fn respawn_pane_with_command(
        &self,
        server: &str,
        pane_id: &PaneId,
        cwd: &Utf8Path,
        command: &str,
    ) -> Result<(), TmuxError> {
        // tmux `respawn-pane -k -c <cwd> -t <pane> <command>` replaces the
        // pane's current process with `command`, executed by tmux through
        // `/bin/sh -c` internally. The wrapper text never reaches the pane
        // shell as typed input, so supervisor wrapper commands are never
        // visible inside the opened workspace. The pane id and layout slot
        // are preserved across the replacement.
        Self::run_tmux(&[
            "-L",
            server,
            "respawn-pane",
            "-k",
            "-c",
            cwd.as_str(),
            "-t",
            pane_id.as_str(),
            command,
        ])?;
        Ok(())
    }

    fn set_hook(
        &self,
        server: &str,
        session: &str,
        hook_name: &str,
        command: Option<&str>,
    ) -> Result<(), TmuxError> {
        match command {
            Some(cmd) => {
                Self::run_tmux(&["-L", server, "set-hook", "-t", session, hook_name, cmd])?;
            }
            None => {
                Self::run_tmux(&["-L", server, "set-hook", "-u", "-t", session, hook_name])?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

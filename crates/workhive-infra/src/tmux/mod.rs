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

use crate::errors::TmuxError;

mod output;
mod types;

pub use types::{
    AttachRequest, CapturePaneRequest, CreateSessionRequest, PaneInfo, PaneTopologyInfo,
    SendKeysRequest, SessionInfo, SplitPaneRequest, WindowInfo,
};

#[allow(unused_imports)]
use output::*;

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

    /// Apply default session options (mouse, terminal features, etc.).
    fn configure_session(&self, server: &str, session: &str) -> Result<(), TmuxError>;

    /// Attach the current terminal to a session.
    fn attach(&self, request: AttachRequest) -> Result<(), TmuxError>;

    /// Detach the current client from its session.
    fn detach_client(&self, server: &str) -> Result<(), TmuxError>;

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
    fn current_pane(&self, server: &str) -> Result<PaneInfo, TmuxError>;

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
    /// process inside the pane is killed (`-k`) — restore use cases call this
    /// only when they are about to relaunch the pane's tool/launch intent.
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
    /// "infra-only" path for installing the pane supervisor wrapper at
    /// open / restore time.
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
        let result = Self::run_tmux(&["-L", server, "has-session", "-t", session]);
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
    /// sets each pane's title via `select-pane -T` after open, restore, and layout
    /// operations. The value comes from config/launch identity, never from volatile
    /// runtime state. Programs that change `pane_title` via ANSI escapes will be
    /// overwritten on the next Workhive operation.
    fn configure_session(&self, server: &str, session: &str) -> Result<(), TmuxError> {
        // Auxiliary tmux hardening: disable runtime title/window
        // auto-renaming so external programs cannot pollute the
        // Workhive-owned `#{pane_title}` between layout application and
        // user attach. This is *secondary* to the `set_unit_title`
        // reassertion the CLI performs in the open flow.
        //
        //   - `allow-rename off` blocks the `\033]2;…\007` escape from
        //     overwriting the window name.
        //   - `automatic-rename off` prevents tmux from inferring a
        //     window name from the foreground command.
        //   - `set-titles off` keeps tmux from emitting its own
        //     terminal-title escape that could confuse the host emulator.
        let options: &[(&str, &str)] = &[
            ("mouse", "on"),
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
            ("default-terminal", "screen-256color"),
            ("allow-rename", "off"),
            ("automatic-rename", "off"),
            ("set-titles", "off"),
        ];

        for (key, value) in options {
            if let Err(e) = Self::run_tmux(&["-L", server, "set-option", "-t", session, key, value])
            {
                tracing::warn!(
                    server,
                    session,
                    option = key,
                    value,
                    error = %e,
                    "failed to set tmux option, continuing"
                );
            }
        }

        // Set server-level option for terminal-features (needs set-option -s)
        if let Err(e) = Self::run_tmux(&[
            "-L",
            server,
            "set-option",
            "-s",
            "terminal-features[0]",
            "xterm*:clipboard:ccolour:cstyle:focus:title:RGB",
        ]) {
            tracing::debug!(
                server,
                error = %e,
                "failed to set terminal-features, continuing"
            );
        }

        tracing::debug!(
            server,
            session,
            options_count = options.len(),
            "tmux session configured"
        );
        Ok(())
    }

    fn attach(&self, request: AttachRequest) -> Result<(), TmuxError> {
        use std::process::Stdio;

        // `tmux attach-session` requires a real TTY, so we must inherit
        // stdin/stdout/stderr instead of capturing them via `.output()`.
        let status = Command::new("tmux")
            .args([
                "-L",
                &request.server_name,
                "attach-session",
                "-t",
                &request.session_name,
            ])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|_| TmuxError::TmuxNotFound)?;

        if status.success() {
            Ok(())
        } else {
            // Without captured stderr, check session existence to provide a
            // meaningful error. status() does not give us stderr content.
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

    fn detach_client(&self, server: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "detach-client"])?;
        Ok(())
    }

    fn kill_session(&self, server: &str, session: &str) -> Result<(), TmuxError> {
        Self::run_tmux(&["-L", server, "kill-session", "-t", session])?;
        Ok(())
    }

    fn list_sessions(&self, server: &str) -> Result<Vec<SessionInfo>, TmuxError> {
        let output = Self::run_tmux(&[
            "-L",
            server,
            "list-sessions",
            "-F",
            "#{session_name}\t#{session_windows}\t#{session_panes}\t#{session_attached}",
        ]);
        match output {
            Ok(text) => Ok(parse_sessions(&text)),
            Err(e) => Err(e),
        }
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

    fn current_pane(&self, server: &str) -> Result<PaneInfo, TmuxError> {
        let output = Self::run_tmux(&[
            "-L",
            server,
            "display-message",
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
        // restarting the default shell in `cwd`. Restore callers run this
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

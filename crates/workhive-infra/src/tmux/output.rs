//! tmux stdout/stderr parsing and typed error classification.

use camino::Utf8PathBuf;
use workhive_core::domain::{PaneId, PaneKind, PaneState};

use crate::errors::TmuxError;

use super::{PaneInfo, PaneTopologyInfo, SessionInfo, WindowInfo};

/// Classify a tmux stderr message into a typed error.
///
/// The adapter is the single place that reads stderr text and maps it
/// to domain-level error variants. Use cases never parse stderr directly.
pub(super) fn classify_tmux_error(stderr: &str, args: &[&str]) -> TmuxError {
    let lower = stderr.to_lowercase();

    let server_name = extract_server_arg(args);

    if lower.contains("no server running") || lower.contains("server not found") {
        return TmuxError::ServerNotRunning {
            server: server_name,
        };
    }

    if lower.contains("no sessions") {
        return TmuxError::ServerNotRunning {
            server: server_name,
        };
    }

    if lower.contains("error connecting") && lower.contains("no such file or directory") {
        return TmuxError::SocketNotFound {
            path: extract_socket_path(stderr).unwrap_or(server_name),
        };
    }

    if lower.contains("permission denied") {
        return TmuxError::PermissionDenied {
            detail: stderr.to_owned(),
        };
    }

    if lower.contains("session not found") || lower.contains("can't find session") {
        let session = extract_session_from_stderr(stderr);
        return TmuxError::SessionNotFound {
            name: session.unwrap_or_else(|| "unknown".to_owned()),
        };
    }

    // tmux `new-session` stderr when the session name is already taken.
    if lower.contains("duplicate session") || lower.contains("session already exists") {
        let session = extract_session_from_stderr(stderr);
        return TmuxError::SessionAlreadyExists {
            name: session.unwrap_or_else(|| "unknown".to_owned()),
        };
    }

    TmuxError::CommandFailed {
        message: stderr.to_owned(),
    }
}

/// Extract the `-L <server>` value from tmux args.
pub(super) fn extract_server_arg(args: &[&str]) -> String {
    args.windows(2)
        .find(|w| w[0] == "-L")
        .map(|w| w[1].to_owned())
        .unwrap_or_else(|| "default".to_owned())
}

/// Extract socket path from "error connecting to /path/to/socket (...)" messages.
pub(super) fn extract_socket_path(stderr: &str) -> Option<String> {
    let prefix = "error connecting to ";
    let lower = stderr.to_lowercase();
    if let Some(pos) = lower.find(prefix) {
        let after = &stderr[pos + prefix.len()..];
        let end = after.find(" (").unwrap_or(after.len());
        Some(after[..end].to_owned())
    } else {
        None
    }
}

/// Extract session name from stderr messages like "can't find session: NAME".
pub(super) fn extract_session_from_stderr(stderr: &str) -> Option<String> {
    for sep in &[": ", "session "] {
        if let Some(pos) = stderr.rfind(sep) {
            let name = stderr[pos + sep.len()..].trim();
            if !name.is_empty() {
                return Some(name.to_owned());
            }
        }
    }
    None
}

pub(super) fn parse_sessions(output: &str) -> Vec<SessionInfo> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                Some(SessionInfo {
                    name: parts[0].to_owned(),
                    windows: parts[1].parse().unwrap_or(0),
                    panes: parts[2].parse().unwrap_or(0),
                    attached: parts[3] != "0",
                })
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn parse_windows(output: &str) -> Vec<WindowInfo> {
    output
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(WindowInfo {
                index: parts[0].parse().unwrap_or(0),
                name: if parts[1].is_empty() {
                    None
                } else {
                    Some(parts[1].to_owned())
                },
                layout: parts[2].to_owned(),
                active: parts[3] == "1",
            })
        })
        .collect()
}

pub(super) fn parse_single_pane(line: &str) -> Option<PaneInfo> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 5 {
        return None;
    }
    let tty = parts.get(5).copied().and_then(non_empty_string);
    let process_pid = parts.get(6).copied().and_then(parse_optional_pid);
    Some(PaneInfo {
        id: PaneId::new(parts[0]),
        index: parts[1].parse().unwrap_or(0),
        cwd: Utf8PathBuf::from(parts[2]),
        command: parts[3].to_owned(),
        active: parts[4] == "1",
        kind: classify_pane_kind(parts[3]),
        state: classify_pane_state(parts[3]),
        tty,
        process_pid,
    })
}

/// Map an empty tmux field to `None` so callers cannot accidentally
/// treat the empty string as a meaningful tty path.
pub(super) fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Parse `#{pane_pid}` defensively. Tmux returns plain decimal digits;
/// we normalize empty / non-numeric / negative inputs to `None` so
/// the observer layer never has to distinguish tmux quirks from real
/// failures.
pub(super) fn parse_optional_pid(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u32>().ok()
}

/// tmux `-F` format string for topology pane queries.
///
/// Carries 11 fields:
///   0 pane_id
///   1 window_index
///   2 pane_index
///   3 pane_current_path
///   4 pane_current_command
///   5 pane_start_command
///   6 pane_title
///   7 pane_active
///   8 pane_dead
///   9 pane_tty            (added so topology paths feed the OS observer)
///  10 pane_pid            (added so topology paths feed the OS observer)
pub(super) const TOPOLOGY_PANE_FORMAT: &str = "#{pane_id}\t#{window_index}\t#{pane_index}\t#{pane_current_path}\t#{pane_current_command}\t#{pane_start_command}\t#{pane_title}\t#{pane_active}\t#{pane_dead}\t#{pane_tty}\t#{pane_pid}";

pub(super) fn parse_topology_pane(line: &str) -> Option<PaneTopologyInfo> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 9 {
        return None;
    }
    let current_command = parts[4].to_owned();
    let tty = parts.get(9).and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    });
    let process_pid = parts.get(10).and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse::<u32>().ok()
        }
    });
    Some(PaneTopologyInfo {
        id: PaneId::new(parts[0]),
        window_index: parts[1].parse().unwrap_or(0),
        pane_index: parts[2].parse().unwrap_or(0),
        cwd: Utf8PathBuf::from(parts[3]),
        current_command: current_command.clone(),
        start_command: if parts[5].is_empty() {
            None
        } else {
            Some(parts[5].to_owned())
        },
        title: if parts[6].is_empty() {
            None
        } else {
            Some(parts[6].to_owned())
        },
        active: parts[7] == "1",
        dead: parts[8] == "1",
        kind: classify_pane_kind(&current_command),
        state: classify_pane_state(&current_command),
        tty,
        process_pid,
    })
}

/// Best-effort pane kind classification.
///
/// Matching is normalised: the command string is lowercased and the
/// final path segment plus the basename without architecture suffixes
/// is examined. This is what lets us classify launcher binaries that
/// arrive as `codex-x86_64-apple-darwin` or `/usr/local/bin/codex`
/// while still being defensive about substring collisions inside
/// arbitrary user processes.
pub(super) fn classify_pane_kind(command: &str) -> PaneKind {
    let cmd = command.to_lowercase();
    let basename = command_basename(&cmd);
    let stem = command_stem(&basename);

    if matches_known_tool(&basename, &stem, AGENT_TOOLS) {
        return PaneKind::Agent;
    }
    if matches_known_tool(&basename, &stem, GIT_UI_TOOLS) {
        return PaneKind::GitUi;
    }
    if is_shell_basename(&basename) {
        return PaneKind::Terminal;
    }

    tracing::debug!(
        command = command,
        basename = %basename,
        stem = %stem,
        "tmux: pane_kind classification fell back to Unknown"
    );
    PaneKind::Unknown
}

pub(super) fn classify_pane_state(command: &str) -> PaneState {
    let cmd = command.to_lowercase();
    let basename = command_basename(&cmd);

    if cmd.is_empty() {
        return PaneState::Unknown;
    }
    if is_shell_basename(&basename) {
        return PaneState::Idle;
    }
    PaneState::Running
}

/// Return the trailing path segment of `command` (lowercased input).
pub(super) fn command_basename(command: &str) -> String {
    command.rsplit('/').next().unwrap_or(command).to_owned()
}

/// Strip an architecture/triple suffix from a basename so launcher
/// binaries like `codex-x86_64-apple-darwin` collapse to `codex`.
/// Conservatively: cut at the first `-` that immediately follows a
/// known prefix; otherwise keep the basename intact.
pub(super) fn command_stem(basename: &str) -> String {
    if let Some(idx) = basename.find('-') {
        basename[..idx].to_owned()
    } else {
        basename.to_owned()
    }
}

/// Known AI/agent tool names that should classify as
/// [`PaneKind::Agent`]. The list is intentionally narrow — adding a
/// new entry must be a deliberate product decision, not heuristic
/// guessing.
pub(super) const AGENT_TOOLS: &[&str] =
    &["cursor", "claude", "copilot", "codex", "aider", "opencode"];

/// Known git UI tool names.
pub(super) const GIT_UI_TOOLS: &[&str] = &["lazygit", "tig", "gitui"];

/// Known shell basenames. Substring matching on the full command would
/// misclassify any process whose name happens to contain `sh` (e.g.
/// `ssh`, `lsh`, `bash-completion`), so we only accept exact basename
/// matches against this canonical list.
pub(super) const SHELL_BASENAMES: &[&str] = &["zsh", "bash", "fish", "sh", "ksh", "dash", "ash"];

pub(super) fn is_shell_basename(basename: &str) -> bool {
    SHELL_BASENAMES.contains(&basename)
}

pub(super) fn matches_known_tool(basename: &str, stem: &str, set: &[&str]) -> bool {
    set.iter().any(|tool| basename == *tool || stem == *tool)
}

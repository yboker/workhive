//! Process environment adapter — detects runtime context from env vars and TTY.

use std::io::IsTerminal;
use std::process::Command;

use workhive_core::errors::EnvironmentError;
use workhive_core::ports::{EnvironmentPort, ExecutionContext, TmuxMembership};

/// Adapter that reads execution context from the real process environment.
pub struct ProcessEnvironment {
    tmux_bin: String,
}

impl ProcessEnvironment {
    pub fn new() -> Self {
        Self {
            tmux_bin: "tmux".to_owned(),
        }
    }

    #[cfg(test)]
    fn with_tmux_bin(tmux_bin: impl Into<String>) -> Self {
        Self {
            tmux_bin: tmux_bin.into(),
        }
    }

    fn normalize_session_name(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    }

    fn query_current_tmux_session(&self) -> Option<String> {
        let output = Command::new(&self.tmux_bin)
            .args(["display-message", "-p", "#S"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        Self::normalize_session_name(&stdout)
    }

    /// Extract the tmux server (socket) name from `$TMUX`.
    ///
    /// `$TMUX` has the format `/path/to/<server_name>,<pid>,<window>`.
    /// The server name is the file-name component of the socket path
    /// (everything before the first comma).
    fn server_name_from_tmux_env(tmux_var: &str) -> Option<String> {
        let socket_path = tmux_var.split(',').next()?;
        let file_name = std::path::Path::new(socket_path).file_name()?.to_str()?;
        if file_name.is_empty() {
            None
        } else {
            Some(file_name.to_owned())
        }
    }

    /// Extract the server name from a socket path string (file-name component).
    fn server_name_from_socket_path(path: &str) -> Option<String> {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return None;
        }
        let file_name = std::path::Path::new(trimmed).file_name()?.to_str()?;
        if file_name.is_empty() {
            None
        } else {
            Some(file_name.to_owned())
        }
    }

    /// Query tmux for the current socket path and derive the server name.
    ///
    /// Fallback for when `$TMUX` env var is absent or malformed.
    fn query_tmux_server(&self) -> Option<String> {
        let output = Command::new(&self.tmux_bin)
            .args(["display-message", "-p", "#{socket_path}"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8(output.stdout).ok()?;
        Self::server_name_from_socket_path(&stdout)
    }
}

impl Default for ProcessEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve tmux membership from raw environment observations.
///
/// This is a pure function over supplied observations so tests can exercise
/// all states without mutating the process environment.
pub fn resolve_tmux_membership(
    tmux_env: Option<&str>,
    tmux_pane: Option<&str>,
    resolved_server: Option<String>,
    resolved_session: Option<String>,
) -> TmuxMembership {
    let has_tmux_evidence = tmux_env.is_some() || tmux_pane.is_some();

    if !has_tmux_evidence {
        return TmuxMembership::Outside;
    }

    match (resolved_server, resolved_session) {
        (Some(server), Some(session)) => TmuxMembership::Resolved {
            server,
            session,
            pane: tmux_pane.map(|p| p.to_owned()),
        },
        (server, session) => TmuxMembership::Unresolved {
            reason: format!(
                "tmux evidence present but resolution incomplete: server={server:?}, session={session:?}"
            ),
        },
    }
}

impl EnvironmentPort for ProcessEnvironment {
    fn execution_context(&self) -> Result<ExecutionContext, EnvironmentError> {
        let tmux_pane_raw = std::env::var("TMUX_PANE").ok();
        let tmux_env = std::env::var("TMUX").ok();
        let in_tmux = tmux_env.is_some();

        let tmux_session = if in_tmux {
            std::env::var("TMUX_SESSION")
                .ok()
                .and_then(|v| Self::normalize_session_name(&v))
                .or_else(|| self.query_current_tmux_session())
        } else {
            None
        };

        let tmux_server = tmux_env
            .as_deref()
            .and_then(Self::server_name_from_tmux_env)
            .or_else(|| {
                if in_tmux {
                    self.query_tmux_server()
                } else {
                    None
                }
            });

        let tmux = resolve_tmux_membership(
            tmux_env.as_deref(),
            tmux_pane_raw.as_deref(),
            tmux_server,
            tmux_session,
        );

        let ctx = ExecutionContext {
            is_interactive: std::io::stdin().is_terminal(),
            tmux: tmux.clone(),
        };

        tracing::debug!(
            is_interactive = ctx.is_interactive,
            tmux = ?ctx.tmux,
            "detected execution context"
        );

        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use workhive_core::ports::TmuxMembership;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn fake_tmux_script(dir: &TempDir, body: &str) -> String {
        let script_path = dir.path().join("tmux");
        let script = format!("#!/bin/sh\n{body}\n");
        fs::write(&script_path, script).expect("write fake tmux script");
        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&script_path)
                .expect("read metadata")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&script_path, perms).expect("set executable bit");
        }
        script_path
            .to_str()
            .expect("script path must be utf-8")
            .to_owned()
    }

    #[test]
    fn normalize_session_name_rejects_blank() {
        assert_eq!(ProcessEnvironment::normalize_session_name(""), None);
        assert_eq!(ProcessEnvironment::normalize_session_name("   "), None);
    }

    #[test]
    fn normalize_session_name_trims_whitespace() {
        assert_eq!(
            ProcessEnvironment::normalize_session_name("  wh@a1@proj@main \n"),
            Some("wh@a1@proj@main".to_owned())
        );
    }

    #[test]
    fn query_current_tmux_session_reads_display_message_output() {
        let dir = TempDir::new().expect("create temp dir");
        let tmux_path = fake_tmux_script(
            &dir,
            r##"
if [ "$1" = "display-message" ] && [ "$2" = "-p" ] && [ "$3" = "#S" ]; then
  echo "wh@4da6@workhive@main"
  exit 0
fi
exit 1
"##,
        );

        let env = ProcessEnvironment::with_tmux_bin(tmux_path);
        let session = env.query_current_tmux_session();
        assert_eq!(session.as_deref(), Some("wh@4da6@workhive@main"));
    }

    #[test]
    fn query_current_tmux_session_returns_none_on_failure() {
        let dir = TempDir::new().expect("create temp dir");
        let tmux_path = fake_tmux_script(&dir, "exit 1");
        let env = ProcessEnvironment::with_tmux_bin(tmux_path);
        assert_eq!(env.query_current_tmux_session(), None);
    }

    // ── server_name_from_tmux_env ──

    #[test]
    fn server_name_from_tmux_env_extracts_socket_name() {
        let tmux_var = "/tmp/tmux-501/whsrv-a1b2-myproject,12345,0";
        assert_eq!(
            ProcessEnvironment::server_name_from_tmux_env(tmux_var),
            Some("whsrv-a1b2-myproject".to_owned())
        );
    }

    #[test]
    fn server_name_from_tmux_env_handles_default_server() {
        let tmux_var = "/tmp/tmux-501/default,99999,0";
        assert_eq!(
            ProcessEnvironment::server_name_from_tmux_env(tmux_var),
            Some("default".to_owned())
        );
    }

    #[test]
    fn server_name_from_tmux_env_returns_none_for_empty() {
        assert_eq!(ProcessEnvironment::server_name_from_tmux_env(""), None);
    }

    // ── query_tmux_server (fallback path) ──

    #[test]
    fn query_tmux_server_extracts_server_name_from_socket_path() {
        let dir = TempDir::new().expect("create temp dir");
        let tmux_path = fake_tmux_script(
            &dir,
            r##"
if [ "$1" = "display-message" ] && [ "$2" = "-p" ] && [ "$3" = "#{socket_path}" ]; then
  echo "/tmp/tmux-501/whsrv-a1b2-myproject"
  exit 0
fi
exit 1
"##,
        );

        let env = ProcessEnvironment::with_tmux_bin(tmux_path);
        let server = env.query_tmux_server();
        assert_eq!(server.as_deref(), Some("whsrv-a1b2-myproject"));
    }

    #[test]
    fn query_tmux_server_returns_none_on_failure() {
        let dir = TempDir::new().expect("create temp dir");
        let tmux_path = fake_tmux_script(&dir, "exit 1");
        let env = ProcessEnvironment::with_tmux_bin(tmux_path);
        assert_eq!(env.query_tmux_server(), None);
    }

    #[test]
    fn query_tmux_server_returns_none_for_empty_output() {
        let dir = TempDir::new().expect("create temp dir");
        let tmux_path = fake_tmux_script(
            &dir,
            r##"
if [ "$1" = "display-message" ]; then
  echo ""
  exit 0
fi
exit 1
"##,
        );

        let env = ProcessEnvironment::with_tmux_bin(tmux_path);
        assert_eq!(env.query_tmux_server(), None);
    }

    // ── resolve_tmux_membership tests ──

    #[test]
    fn execution_context_malformed_tmux_is_unresolved() {
        let result = resolve_tmux_membership(
            Some("malformed-no-commas"),
            None,
            None, // server resolution failed
            None, // session resolution failed
        );
        assert!(
            matches!(result, TmuxMembership::Unresolved { .. }),
            "malformed TMUX must produce Unresolved, got: {result:?}"
        );
    }

    #[test]
    fn execution_context_failed_identity_probe_is_unresolved() {
        let result = resolve_tmux_membership(
            Some("/tmp/tmux-501/whsrv-inst-proj,12345,0"),
            Some("%0"),
            Some("whsrv-inst-proj".to_owned()),
            None, // session probe failed
        );
        assert!(
            matches!(result, TmuxMembership::Unresolved { .. }),
            "failed session probe must produce Unresolved, got: {result:?}"
        );
    }

    #[test]
    fn execution_context_lone_tmux_pane_is_unresolved() {
        let result = resolve_tmux_membership(
            None,       // no TMUX env var
            Some("%3"), // lone TMUX_PANE
            None,
            None,
        );
        assert!(
            matches!(result, TmuxMembership::Unresolved { .. }),
            "lone TMUX_PANE must produce Unresolved, got: {result:?}"
        );
    }

    #[test]
    fn execution_context_no_evidence_is_outside() {
        let result = resolve_tmux_membership(None, None, None, None);
        assert_eq!(result, TmuxMembership::Outside);
    }

    #[test]
    fn execution_context_full_resolution_is_resolved() {
        let result = resolve_tmux_membership(
            Some("/tmp/tmux-501/whsrv-inst-proj,12345,0"),
            Some("%0"),
            Some("whsrv-inst-proj".to_owned()),
            Some("wh@inst@proj@main".to_owned()),
        );
        assert_eq!(
            result,
            TmuxMembership::Resolved {
                server: "whsrv-inst-proj".to_owned(),
                session: "wh@inst@proj@main".to_owned(),
                pane: Some("%0".to_owned()),
            }
        );
    }
}

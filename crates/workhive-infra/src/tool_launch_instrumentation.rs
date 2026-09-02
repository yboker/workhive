//! Adapter for [`workhive_core::ports::ToolLaunchInstrumentationPort`].
//!
//! The adapter knows the path to the running `workhive` binary and the
//! project runtime root, builds the supervisor invocation as typed
//! argv internally, and only at the very end uses [`shell_join`] to
//! emit a single shell-safe command string suitable for tmux
//! `send-keys`.
//!
//! Producing the wrapper is a pure transformation; we never spawn a
//! process from this adapter. The wrapped command is later installed
//! by the open / restore use cases through
//! [`workhive_core::ports::SnapshotRuntimePort::respawn_unit_with_command`],
//! which delegates to `tmux respawn-pane -k -c <cwd> <cmd>` so the
//! internal `workhive internal pane-supervisor ...` wrapper never
//! appears as a visible keystroke in pane scrollback. User inline /
//! raw commands stay on the visible `send_input` path; only the
//! supervisor wrapper goes through the typed launch verb.

use camino::Utf8PathBuf;
use workhive_core::ports::{
    ToolLaunchInstrumentationError, ToolLaunchInstrumentationPort, WrapToolCommandRequest,
};

/// Adapter wiring the workhive binary path + project / runtime root
/// into the instrumentation port.
///
/// Spawn-time prepare must read the project-over-global gate:
/// the supervisor's spawn-time AI integration prepare must be able to
/// resolve a project-scoped feature gate, identical to the one used
/// by `pane list` / `pane show`. The project root therefore travels
/// from the composition root through this adapter as **typed
/// context** — never reconstructed by truncating `runtime_root` at
/// runtime, which would be a low-confidence string heuristic this
/// codebase forbids.
pub struct WorkhiveToolLaunchInstrumentation {
    workhive_binary: Utf8PathBuf,
    project_root: Utf8PathBuf,
    runtime_root: Utf8PathBuf,
}

impl WorkhiveToolLaunchInstrumentation {
    /// Build an adapter for the given project + runtime root,
    /// locating the workhive binary via `std::env::current_exe()`.
    /// Fails closed (with a typed error) if the binary path cannot
    /// be determined, per the port contract.
    pub fn from_current_exe(
        project_root: Utf8PathBuf,
        runtime_root: Utf8PathBuf,
    ) -> Result<Self, ToolLaunchInstrumentationError> {
        let exe = std::env::current_exe().map_err(|err| {
            ToolLaunchInstrumentationError::SupervisorBinaryUnavailable {
                detail: format!("current_exe: {err}"),
            }
        })?;
        let exe_utf8 = Utf8PathBuf::from_path_buf(exe).map_err(|raw| {
            ToolLaunchInstrumentationError::SupervisorBinaryUnavailable {
                detail: format!("non-UTF8 binary path: {}", raw.display()),
            }
        })?;
        Ok(Self {
            workhive_binary: exe_utf8,
            project_root,
            runtime_root,
        })
    }

    /// Test seam: build with explicit binary + project root + runtime root.
    pub fn with_paths(
        workhive_binary: Utf8PathBuf,
        project_root: Utf8PathBuf,
        runtime_root: Utf8PathBuf,
    ) -> Self {
        Self {
            workhive_binary,
            project_root,
            runtime_root,
        }
    }
}

impl ToolLaunchInstrumentationPort for WorkhiveToolLaunchInstrumentation {
    fn wrap_tool_command(
        &self,
        request: WrapToolCommandRequest,
    ) -> Result<String, ToolLaunchInstrumentationError> {
        if request.program.is_empty() {
            return Err(
                ToolLaunchInstrumentationError::SupervisorBinaryUnavailable {
                    detail: "program is empty".to_owned(),
                },
            );
        }

        let argv = build_supervisor_argv(
            self.workhive_binary.as_str(),
            self.project_root.as_str(),
            self.runtime_root.as_str(),
            &request,
        );
        Ok(shell_join(&argv))
    }
}

/// Build the typed argv for the supervisor invocation.
///
/// The result is `[workhive, "internal", "pane-supervisor", flags...,
/// "--", program, args...]`. Tests pin the exact sequence so the CLI
/// dispatch (which parses these flags) and this adapter cannot drift
/// independently.
///
/// `project_root` is emitted as `--project-root <path>` so the
/// spawned supervisor can resolve the **project-over-global** AI
/// integrations feature gate exactly like `pane list` / `pane show`,
/// instead of falling back to global-only.
pub fn build_supervisor_argv(
    workhive_binary: &str,
    project_root: &str,
    runtime_root: &str,
    request: &WrapToolCommandRequest,
) -> Vec<String> {
    let basename = request
        .tool_name
        .as_deref()
        .map(str::to_owned)
        .unwrap_or_else(|| derive_basename(&request.program));

    let mut argv: Vec<String> = Vec::with_capacity(18 + request.args.len());
    argv.push(workhive_binary.to_owned());
    argv.push("internal".to_owned());
    argv.push("pane-supervisor".to_owned());
    argv.push("--project-root".to_owned());
    argv.push(project_root.to_owned());
    argv.push("--runtime-root".to_owned());
    argv.push(runtime_root.to_owned());
    argv.push("--group".to_owned());
    argv.push(request.group.clone());
    argv.push("--workspace".to_owned());
    argv.push(request.workspace.clone());
    argv.push("--unit".to_owned());
    argv.push(request.unit_id.as_str().to_owned());
    argv.push("--tool".to_owned());
    argv.push(basename);
    if let Some(program_path) = request.program_path.as_deref() {
        argv.push("--program-path".to_owned());
        argv.push(program_path.to_owned());
    }
    argv.push("--".to_owned());
    argv.push(request.program.clone());
    for arg in &request.args {
        argv.push(arg.clone());
    }
    argv
}

/// Derive a basename from a program path for cases where the caller
/// did not supply a tool name. Strips any directory prefix and falls
/// back to a stable placeholder when empty.
fn derive_basename(program: &str) -> String {
    let basename = program.rsplit('/').next().unwrap_or(program);
    if basename.is_empty() {
        "command".to_owned()
    } else {
        basename.to_owned()
    }
}

/// Join a typed argv list into a shell-safe command string.
///
/// Each token is wrapped in single quotes via [`shell_single_quote`].
/// This is the only place in the adapter that crosses from typed argv
/// into a shell string, so all escaping invariants are anchored here.
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|s| shell_single_quote(s))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Wrap an arbitrary string in single quotes so it can be safely
/// passed through a Bourne-compatible shell. The single-quote rule
/// is: end the open quote, escape literal `'`, reopen the quote.
pub fn shell_single_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use workhive_core::domain::UnitId;

    fn fixture() -> WorkhiveToolLaunchInstrumentation {
        WorkhiveToolLaunchInstrumentation::with_paths(
            Utf8PathBuf::from("/usr/local/bin/workhive"),
            Utf8PathBuf::from("/proj"),
            Utf8PathBuf::from("/proj/.workhive/runtime"),
        )
    }

    #[test]
    fn argv_layout_pins_supervisor_flag_order() {
        let argv = build_supervisor_argv(
            "/usr/local/bin/workhive",
            "/proj",
            "/runtime",
            &WrapToolCommandRequest {
                group: "wh-srv".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("%5"),
                tool_name: Some("claude".to_owned()),
                program_path: Some("/usr/local/bin/claude".to_owned()),
                program: "claude".to_owned(),
                args: vec!["--no-banner".to_owned()],
            },
        );
        assert_eq!(
            argv,
            vec![
                "/usr/local/bin/workhive".to_owned(),
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--project-root".to_owned(),
                "/proj".to_owned(),
                "--runtime-root".to_owned(),
                "/runtime".to_owned(),
                "--group".to_owned(),
                "wh-srv".to_owned(),
                "--workspace".to_owned(),
                "ws".to_owned(),
                "--unit".to_owned(),
                "%5".to_owned(),
                "--tool".to_owned(),
                "claude".to_owned(),
                "--program-path".to_owned(),
                "/usr/local/bin/claude".to_owned(),
                "--".to_owned(),
                "claude".to_owned(),
                "--no-banner".to_owned(),
            ]
        );
    }

    /// The wrapped supervisor invocation MUST
    /// carry `--project-root` so the spawn-time AI integration
    /// prepare resolves the same **project-over-global** feature
    /// gate that `pane list` / `pane show` use. Reconstructing the
    /// project root by truncating `runtime_root` at runtime would be
    /// a forbidden low-confidence string heuristic;
    /// this test pins the typed flag instead.
    #[test]
    fn argv_emits_project_root_flag_before_runtime_root() {
        let adapter = WorkhiveToolLaunchInstrumentation::with_paths(
            Utf8PathBuf::from("/usr/local/bin/workhive"),
            Utf8PathBuf::from("/Users/me/proj"),
            Utf8PathBuf::from("/Users/me/proj/.workhive/runtime"),
        );
        let cmd = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "wh-srv".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("%5"),
                tool_name: Some("codex".to_owned()),
                program_path: None,
                program: "codex".to_owned(),
                args: Vec::new(),
            })
            .unwrap();
        assert!(
            cmd.contains("'--project-root' '/Users/me/proj'"),
            "supervisor invocation must include --project-root for typed project context; got {cmd}",
        );
        let project_idx = cmd.find("'--project-root'").expect("project-root present");
        let runtime_idx = cmd.find("'--runtime-root'").expect("runtime-root present");
        assert!(
            project_idx < runtime_idx,
            "--project-root must precede --runtime-root for stable argv layout; got {cmd}",
        );
    }

    #[test]
    fn wraps_simple_command_with_quoted_arguments() {
        let adapter = fixture();
        let cmd = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "wh-srv".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("%5"),
                tool_name: Some("claude".to_owned()),
                program_path: Some("/usr/local/bin/claude".to_owned()),
                program: "claude".to_owned(),
                args: vec!["--no-banner".to_owned()],
            })
            .unwrap();

        assert!(cmd.contains("'/usr/local/bin/workhive'"));
        assert!(cmd.contains("'internal'"));
        assert!(cmd.contains("'pane-supervisor'"));
        assert!(cmd.contains("'--unit' '%5'"));
        assert!(cmd.contains("'--tool' 'claude'"));
        assert!(cmd.contains("'--' 'claude' '--no-banner'"));
    }

    #[test]
    fn quoting_is_safe_against_embedded_single_quotes() {
        let adapter = fixture();
        let cmd = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "wh-srv".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("%5"),
                tool_name: None,
                program_path: None,
                program: "echo".to_owned(),
                args: vec!["it's fine".to_owned()],
            })
            .unwrap();
        // Embedded apostrophes use the `'\''` idiom rather than ending
        // the quoted region. The argv field for "it's fine" must read
        // exactly `'it'\''s fine'`.
        assert!(cmd.contains("'it'\\''s fine'"));
    }

    #[test]
    fn derives_basename_when_tool_name_is_missing() {
        let adapter = fixture();
        let cmd = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "g".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("u"),
                tool_name: None,
                program_path: None,
                program: "/opt/bin/foo".to_owned(),
                args: vec!["--bar".to_owned()],
            })
            .unwrap();
        assert!(cmd.contains("'--tool' 'foo'"));
    }

    #[test]
    fn preserves_runtime_root_with_spaces() {
        let adapter = WorkhiveToolLaunchInstrumentation::with_paths(
            Utf8PathBuf::from("/usr/local/bin/workhive"),
            Utf8PathBuf::from("/Users/me/A B"),
            Utf8PathBuf::from("/Users/me/A B/.workhive/runtime"),
        );
        let cmd = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "g".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("u"),
                tool_name: Some("claude".to_owned()),
                program_path: None,
                program: "claude".to_owned(),
                args: Vec::new(),
            })
            .unwrap();
        assert!(cmd.contains("'--runtime-root' '/Users/me/A B/.workhive/runtime'"));
    }

    #[test]
    fn empty_program_is_rejected() {
        let adapter = fixture();
        let err = adapter
            .wrap_tool_command(WrapToolCommandRequest {
                group: "g".to_owned(),
                workspace: "ws".to_owned(),
                unit_id: UnitId::new("u"),
                tool_name: None,
                program_path: None,
                program: String::new(),
                args: Vec::new(),
            })
            .unwrap_err();
        let detail = format!("{err}");
        assert!(detail.contains("program is empty"));
    }

    #[test]
    fn shell_single_quote_handles_metacharacters() {
        assert_eq!(shell_single_quote("a b"), "'a b'");
        assert_eq!(shell_single_quote("$HOME"), "'$HOME'");
        assert_eq!(shell_single_quote("`cmd`"), "'`cmd`'");
        assert_eq!(shell_single_quote("a;b|c&d"), "'a;b|c&d'");
        assert_eq!(shell_single_quote("a\nb"), "'a\nb'");
        assert_eq!(shell_single_quote(""), "''");
    }
}

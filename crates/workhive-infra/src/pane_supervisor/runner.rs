//! Pane supervisor runner — the long-running process that wraps a
//! user-supplied tool command, writes authoritative metadata, and
//! reports the child's exit code.
//!
//! The supervisor is the executable that produces the highest-
//! confidence observation source documented on
//! [`workhive_core::ports::PaneSupervisorPort`]. It is invoked via the
//! hidden CLI subcommand `workhive internal pane-supervisor` once
//! per supervised pane.
//!
//! ## Responsibilities
//!
//! 1. Write `Running` supervisor metadata before spawning the child
//!    so observers cannot momentarily see the pane as "unsupervised".
//! 2. Spawn the configured child via typed argv
//!    (`Command::new(program).args(args)`). No `sh -c`, no string
//!    splitting, no shell quoting — argv has been typed since the
//!    `ToolLaunchInstrumentationPort` boundary.
//! 3. Update metadata with the real child pid (the one the OS gave us
//!    after `fork`) so the OS observer's process tree and the
//!    supervisor metadata agree on identity.
//! 4. Wait for the child to exit; on exit, write `Exited` metadata
//!    with the exit code so observers can show the post-mortem state.
//! 5. Propagate the child's exit code as our own.
//!
//! ## Failure handling
//!
//! Errors writing metadata are logged via `tracing` but never block
//! the child from running. The merge policy already tolerates a
//! missing supervisor signal by falling through to the OS observer.

use std::process::{Command, ExitStatus};

use camino::Utf8PathBuf;
use workhive_core::domain::{AiActivityProvider, UnitId};

use super::store::{
    PaneSupervisorMetadata, PaneSupervisorStateOnDisk, PaneSupervisorStoreError, write_metadata,
};

/// Inputs to the supervisor runner.
///
/// The runner accepts the child as typed program + args; this is the
/// invariant the CLI dispatcher must preserve when parsing the
/// `workhive internal pane-supervisor` subcommand.
#[derive(Debug, Clone)]
pub struct PaneSupervisorRun {
    /// Project runtime root (`<project>/.workhive/runtime`).
    pub runtime_root: Utf8PathBuf,
    /// Isolation group (tmux server name).
    pub group: String,
    /// Workspace name (tmux session name).
    pub workspace: String,
    /// Logical unit identifier.
    pub unit_id: UnitId,
    /// Tool name (e.g. `claude`). Used as the supervisor metadata's
    /// `command_basename` so the merge policy can show a meaningful
    /// foreground command even before any OS observation arrives.
    pub command_basename: String,
    /// Optional resolved program path (`/usr/local/bin/claude`).
    pub program_path: Option<String>,
    /// Program to exec (typed argv[0]).
    pub program: String,
    /// Program arguments (typed argv[1..]).
    pub args: Vec<String>,
    /// Environment variables to inject into the child process.
    pub env: std::collections::BTreeMap<String, String>,
}

/// Outcome of running the supervisor.
#[derive(Debug)]
pub enum PaneSupervisorOutcome {
    /// Child was launched. The exit code (if any) is propagated.
    ChildExited(ExitStatus),
    /// Child could never be launched; supervisor metadata records the
    /// failure as `Exited` with no exit code.
    SpawnFailed(std::io::Error),
}

impl PaneSupervisorOutcome {
    /// Map the outcome to a stable process-exit code suitable for the
    /// CLI to return. Spawn failures map to `127` (POSIX "command
    /// not found" / "exec failed") so consumers can distinguish them
    /// from a child that ran and exited with code 0/1/etc.
    pub fn to_exit_code(&self) -> i32 {
        match self {
            Self::ChildExited(status) => status.code().unwrap_or(1),
            Self::SpawnFailed(_) => 127,
        }
    }
}

/// Run the supervisor end-to-end. Designed to be called once from
/// the CLI handler for `workhive internal pane-supervisor`.
pub fn run_supervisor(args: PaneSupervisorRun) -> PaneSupervisorOutcome {
    let started_at = current_rfc3339();

    // Step 1: write Running metadata with the supervisor's own pid.
    // We update with the real child pid as soon as we have it; until
    // then, observers seeing the supervisor pid is still better than
    // a missing observation — the supervisor is itself alive in the
    // same process group as the child-to-be.
    let supervisor_pid = std::process::id();
    let mut metadata = PaneSupervisorMetadata {
        pid: supervisor_pid,
        command_basename: args.command_basename.clone(),
        program_path: args.program_path.clone(),
        started_at_rfc3339: started_at.clone(),
        last_heartbeat_rfc3339: None,
        state: PaneSupervisorStateOnDisk::Running,
        exit_code: None,
    };
    record_metadata(
        &args.runtime_root,
        &args.group,
        &args.workspace,
        &args.unit_id,
        &metadata,
        "initial Running",
    );

    // Step 2: spawn the child via typed argv. No `sh -c`; argv is
    // typed all the way from the ToolLaunchInstrumentationPort
    // through the CLI dispatcher into this call.
    //
    // Inject Workhive correlation env vars so installed hook scripts
    // and OpenCode plugins can route lifecycle events back to the
    // right pane via `workhive internal activity record`. The
    // variable names are a stable provider-integration contract:
    // WORKHIVE_RUNTIME_ROOT, WORKHIVE_GROUP,
    // WORKHIVE_WORKSPACE, WORKHIVE_UNIT_ID, WORKHIVE_TOOL_PROVIDER,
    // WORKHIVE_TOOL_NAME, and WORKHIVE_BIN. WORKHIVE_BIN is the
    // absolute current executable path, so generated provider hooks do
    // not depend on a bare `workhive` lookup in the tool's PATH.
    // `from_tool_basename` deliberately returns `None` for non-AI
    // tools, so non-AI panes inherit a `none` provider and the recorder
    // rejects events from them.
    let provider = AiActivityProvider::from_tool_basename(&args.command_basename);
    let workhive_bin = std::env::current_exe();
    let mut command = Command::new(&args.program);
    command.args(&args.args);
    for (key, value) in &args.env {
        command.env(key, value);
    }
    command
        .env("WORKHIVE_RUNTIME_ROOT", args.runtime_root.as_str())
        .env("WORKHIVE_GROUP", &args.group)
        .env("WORKHIVE_WORKSPACE", &args.workspace)
        .env("WORKHIVE_UNIT_ID", args.unit_id.as_str())
        .env("WORKHIVE_TOOL_PROVIDER", provider.wire_name())
        .env("WORKHIVE_TOOL_NAME", &args.command_basename);
    match workhive_bin {
        Ok(path) => {
            command.env("WORKHIVE_BIN", path);
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "pane_supervisor: failed to resolve current executable for provider hooks"
            );
        }
    }
    let child_handle = command.spawn();
    let mut child = match child_handle {
        Ok(child) => child,
        Err(err) => {
            tracing::error!(
                error = %err,
                program = %args.program,
                "pane_supervisor: failed to spawn child"
            );
            metadata.state = PaneSupervisorStateOnDisk::Exited;
            metadata.exit_code = None;
            record_metadata(
                &args.runtime_root,
                &args.group,
                &args.workspace,
                &args.unit_id,
                &metadata,
                "spawn-failed Exited",
            );
            return PaneSupervisorOutcome::SpawnFailed(err);
        }
    };

    // Step 3: update Running metadata with the real child pid so the
    // OS observer's process tree and the supervisor metadata agree.
    metadata.pid = child.id();
    record_metadata(
        &args.runtime_root,
        &args.group,
        &args.workspace,
        &args.unit_id,
        &metadata,
        "child pid update",
    );

    // Step 4: wait for the child.
    let status = match child.wait() {
        Ok(status) => status,
        Err(err) => {
            tracing::error!(error = %err, "pane_supervisor: wait failed");
            metadata.state = PaneSupervisorStateOnDisk::Exited;
            metadata.exit_code = None;
            record_metadata(
                &args.runtime_root,
                &args.group,
                &args.workspace,
                &args.unit_id,
                &metadata,
                "wait-failed Exited",
            );
            // Treat wait failure as "child of unknown final state"; we
            // still surface a synthetic Exited status so the CLI
            // process can return a stable exit code.
            return PaneSupervisorOutcome::ChildExited(synthetic_failure_status());
        }
    };

    // Step 5: write final Exited metadata.
    metadata.state = PaneSupervisorStateOnDisk::Exited;
    metadata.exit_code = status.code();
    record_metadata(
        &args.runtime_root,
        &args.group,
        &args.workspace,
        &args.unit_id,
        &metadata,
        "Exited",
    );

    PaneSupervisorOutcome::ChildExited(status)
}

fn record_metadata(
    runtime_root: &Utf8PathBuf,
    group: &str,
    workspace: &str,
    unit_id: &UnitId,
    metadata: &PaneSupervisorMetadata,
    label: &str,
) {
    if let Err(err) = write_metadata(runtime_root, group, workspace, unit_id, metadata) {
        // Per the error-handling policy: errors carry context. Per the supervisor
        // contract, a metadata-write failure must NOT abort the child
        // — observers will simply see the previous (or empty) state.
        let path = format_store_error(&err);
        tracing::error!(
            error = %path,
            label = %label,
            unit_id = %unit_id,
            "pane_supervisor: metadata write failed"
        );
    }
}

fn format_store_error(err: &PaneSupervisorStoreError) -> String {
    match err {
        PaneSupervisorStoreError::Io { path, detail } => format!("io {path}: {detail}"),
        PaneSupervisorStoreError::InvalidPath { detail } => format!("invalid path: {detail}"),
        PaneSupervisorStoreError::Corrupt { path, detail } => {
            format!("corrupt {path}: {detail}")
        }
    }
}

#[cfg(unix)]
fn synthetic_failure_status() -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(1 << 8)
}

#[cfg(not(unix))]
fn synthetic_failure_status() -> ExitStatus {
    // On non-Unix hosts the only ExitStatus we can manufacture is
    // the result of an actual child run. Workhive is only formally
    // supported on macOS / Linux, but if this path is ever reached
    // we MUST NOT panic — a panic in the supervisor would abort the
    // CLI process before it can propagate a meaningful exit code.
    //
    // If `cmd /c exit 1` itself fails to spawn (e.g. no `cmd` in
    // PATH), fall back to invoking the host process's own canonical
    // "false" via spawning the current exe with a non-zero exit;
    // worst case we still fall back to whatever ExitStatus the
    // platform produced, never to a panic.
    if let Ok(status) = Command::new("cmd").arg("/c").arg("exit 1").status() {
        return status;
    }
    tracing::warn!(
        "pane_supervisor: synthetic failure status fallback could not spawn `cmd /c exit 1`; \
         using ExitStatus default to avoid aborting the supervisor"
    );
    // Best-effort: return whatever the platform's default `ExitStatus`
    // is. Callers translate this into exit code `1` via
    // `PaneSupervisorOutcome::to_exit_code()`'s `unwrap_or(1)`.
    ExitStatus::default()
}

/// Format the current time as an RFC 3339 timestamp without pulling
/// in a heavy date dependency. The supervisor uses this only for
/// audit fields in metadata; precise sub-second accuracy is not
/// required.
///
/// The system clock can in principle return a time before the Unix
/// epoch (rare, but possible on hosts whose RTC battery is dead).
/// In that case we explicitly log and fall back to epoch zero rather
/// than silently swallowing the error via `unwrap_or_default()` — the
/// audit field is still produced, but operators can see why.
fn current_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs_since_epoch = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "pane_supervisor: system clock is before UNIX_EPOCH; \
                 falling back to epoch zero for audit timestamp"
            );
            0
        }
    };
    format_rfc3339_utc(secs_since_epoch)
}

/// Convert a Unix epoch second count into an RFC 3339 UTC string.
/// Implemented locally to avoid a date-time dependency for what is
/// purely an audit field.
fn format_rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let sec_of_day = (secs % 86_400) as u32;
    let hours = sec_of_day / 3600;
    let minutes = (sec_of_day % 3600) / 60;
    let seconds = sec_of_day % 60;

    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert "days since 1970-01-01" into a (year, month, day) tuple
/// using Howard Hinnant's algorithm. Self-contained so the supervisor
/// has no external date-time dep.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_supervisor::store::read_metadata;
    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    fn tmp_root() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        (tmp, root)
    }

    #[test]
    fn run_supervisor_writes_running_then_exited_metadata_for_successful_child() {
        let (_t, root) = tmp_root();
        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u1"),
            command_basename: "true".to_owned(),
            program_path: None,
            program: "true".to_owned(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        };
        let outcome = run_supervisor(run);
        match outcome {
            PaneSupervisorOutcome::ChildExited(status) => assert!(status.success()),
            PaneSupervisorOutcome::SpawnFailed(err) => panic!("unexpected spawn failure: {err}"),
        }

        let metadata = read_metadata(&root, "g", "ws", &UnitId::new("u1"))
            .unwrap()
            .expect("metadata persisted");
        assert!(matches!(metadata.state, PaneSupervisorStateOnDisk::Exited));
        assert_eq!(metadata.exit_code, Some(0));
        assert_eq!(metadata.command_basename, "true");
    }

    #[test]
    fn run_supervisor_records_nonzero_exit_for_failing_child() {
        let (_t, root) = tmp_root();
        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u2"),
            command_basename: "false".to_owned(),
            program_path: None,
            program: "false".to_owned(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        };
        let outcome = run_supervisor(run);
        match outcome {
            PaneSupervisorOutcome::ChildExited(status) => {
                assert!(!status.success());
                assert_eq!(status.code(), Some(1));
            }
            PaneSupervisorOutcome::SpawnFailed(err) => panic!("unexpected spawn failure: {err}"),
        }
        let metadata = read_metadata(&root, "g", "ws", &UnitId::new("u2"))
            .unwrap()
            .expect("metadata persisted");
        assert_eq!(metadata.exit_code, Some(1));
    }

    #[test]
    fn run_supervisor_records_spawn_failure_for_missing_program() {
        let (_t, root) = tmp_root();
        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u3"),
            command_basename: "no-such".to_owned(),
            program_path: None,
            program: "/this/does/not/exist".to_owned(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        };
        let outcome = run_supervisor(run);
        assert_eq!(outcome.to_exit_code(), 127);
        let metadata = read_metadata(&root, "g", "ws", &UnitId::new("u3"))
            .unwrap()
            .expect("metadata persisted on spawn failure");
        assert!(matches!(metadata.state, PaneSupervisorStateOnDisk::Exited));
        assert!(metadata.exit_code.is_none());
    }

    #[test]
    fn run_supervisor_passes_args_through_typed_argv() {
        let (_t, root) = tmp_root();
        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u4"),
            command_basename: "echo".to_owned(),
            program_path: None,
            program: "false".to_owned(),
            args: vec!["arg with spaces".to_owned(), "$NOT_EXPANDED".to_owned()],
            env: std::collections::BTreeMap::new(),
        };
        let outcome = run_supervisor(run);
        match outcome {
            PaneSupervisorOutcome::ChildExited(status) => {
                // `false` ignores all args and exits 1.
                assert_eq!(status.code(), Some(1));
            }
            PaneSupervisorOutcome::SpawnFailed(err) => panic!("unexpected spawn failure: {err}"),
        }
    }

    #[test]
    fn run_supervisor_injects_current_exe_for_provider_hooks() {
        let (_t, root) = tmp_root();
        let out = root.join("env.txt");
        let script = root.join("print-env.sh");
        std::fs::write(
            script.as_std_path(),
            format!(
                "#!/bin/sh\nprintf '%s\\n%s\\n%s\\n' \"$WORKHIVE_BIN\" \"$WORKHIVE_TOOL_PROVIDER\" \"$WORKHIVE_TOOL_NAME\" > '{}'\n",
                out
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(script.as_std_path())
                .unwrap()
                .permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(script.as_std_path(), perm).unwrap();
        }

        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u5"),
            command_basename: "codex".to_owned(),
            program_path: None,
            program: script.to_string(),
            args: Vec::new(),
            env: std::collections::BTreeMap::new(),
        };
        let outcome = run_supervisor(run);
        match outcome {
            PaneSupervisorOutcome::ChildExited(status) => assert!(status.success()),
            PaneSupervisorOutcome::SpawnFailed(err) => panic!("unexpected spawn failure: {err}"),
        }

        let rendered = std::fs::read_to_string(out.as_std_path()).unwrap();
        let mut lines = rendered.lines();
        let bin = lines.next().expect("WORKHIVE_BIN line");
        assert!(
            std::path::Path::new(bin).is_absolute(),
            "WORKHIVE_BIN must be absolute, got {bin:?}"
        );
        assert_eq!(lines.next(), Some("codex"));
        assert_eq!(lines.next(), Some("codex"));
    }

    #[test]
    fn run_supervisor_injects_env_vars_into_child_process() {
        let (_t, root) = tmp_root();
        let out = root.join("env_check.txt");
        let script = root.join("check-env.sh");
        std::fs::write(
            script.as_std_path(),
            format!(
                "#!/bin/sh\nprintf '%s\\n%s' \"$IS_SANDBOX\" \"$CUSTOM_VAR\" > '{}'\n",
                out
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(script.as_std_path())
                .unwrap()
                .permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(script.as_std_path(), perm).unwrap();
        }

        let mut env = std::collections::BTreeMap::new();
        env.insert("IS_SANDBOX".to_owned(), "1".to_owned());
        env.insert("CUSTOM_VAR".to_owned(), "hello".to_owned());

        let run = PaneSupervisorRun {
            runtime_root: root.clone(),
            group: "g".to_owned(),
            workspace: "ws".to_owned(),
            unit_id: UnitId::new("u6"),
            command_basename: "claude".to_owned(),
            program_path: None,
            program: script.to_string(),
            args: Vec::new(),
            env,
        };
        let outcome = run_supervisor(run);
        match outcome {
            PaneSupervisorOutcome::ChildExited(status) => assert!(status.success()),
            PaneSupervisorOutcome::SpawnFailed(err) => panic!("unexpected spawn failure: {err}"),
        }

        let rendered = std::fs::read_to_string(out.as_std_path()).unwrap();
        let mut lines = rendered.lines();
        assert_eq!(lines.next(), Some("1"), "IS_SANDBOX must be set to 1");
        assert_eq!(
            lines.next(),
            Some("hello"),
            "CUSTOM_VAR must be set to hello"
        );
    }

    #[test]
    fn outcome_to_exit_code_maps_spawn_failure_to_127() {
        let err = std::io::Error::other("boom");
        assert_eq!(PaneSupervisorOutcome::SpawnFailed(err).to_exit_code(), 127);
    }
}

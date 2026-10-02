//! System dependency checker — verifies git/tmux availability and versions.
//!
//! Uses `std::process::Command` to probe the system. Platform-aware install
//! hints are provided for macOS, Linux (apt/dnf/pacman), and Windows (winget/scoop).

use std::process::Command;

use serde_json::Value;
use workhive_core::ports::{
    BackendPreflight, BackendUnavailableReason, DependencyKind, DependencyPort, DependencyStatus,
    RuntimeError, WORKHIVE_HERDR_SESSION,
};

const GIT_MIN_VERSION: &str = "2.20.0";
const TMUX_MIN_VERSION: &str = "3.0";
const HERDR_MIN_VERSION: &str = "0.8.0";

#[cfg(unix)]
const MANAGED_HERDR_RECORD: &str = "herdr-managed.json";
#[cfg(unix)]
const HERDR_START_LOCK: &str = "herdr-start.lock";
#[cfg(unix)]
const WORKHIVE_MANAGED_TOKEN_ENV: &str = "WORKHIVE_MANAGED_TOKEN";

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ManagedHerdrServer {
    pid: u32,
    socket: std::path::PathBuf,
    session: String,
    token: String,
}

#[cfg(unix)]
struct HerdrStartLock {
    _lock: fslock::LockFile,
}

/// The `HERDR_SOCKET_PATH` override, honoured for spawning only in tests.
///
/// AGENTS.md restricts that variable to an intentional alternate-socket test
/// and forbids using it to reach a session Workhive does not own, so a shipped
/// CLI ignores it here: inheriting it from the user's own Herdr pane must not
/// redirect a spawn onto that user's socket. Isolated tests spawn an unnamed
/// `herdr server` on the override socket, which is what keeps the
/// named-session path out of their sandbox.
fn socket_override() -> Option<std::ffi::OsString> {
    if !cfg!(test) {
        return None;
    }
    std::env::var_os("HERDR_SOCKET_PATH").filter(|socket| !socket.is_empty())
}

pub(crate) fn herdr_program() -> std::path::PathBuf {
    #[cfg(test)]
    if let Some(path) = std::env::var_os("WORKHIVE_HERDR_BIN").filter(|path| !path.is_empty()) {
        return std::path::PathBuf::from(path);
    }
    std::path::PathBuf::from("herdr")
}

#[cfg(unix)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HerdrSessionInfo {
    pub name: String,
    pub running: bool,
    pub socket_path: std::path::PathBuf,
}

#[cfg(unix)]
fn parse_herdr_sessions(payload_bytes: &[u8]) -> Result<Vec<HerdrSessionInfo>, RuntimeError> {
    let payload: Value = serde_json::from_slice(payload_bytes).map_err(|error| {
        operation_failed(format!("Herdr session list returned invalid JSON: {error}"))
    })?;
    let sessions = payload
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or_else(|| operation_failed("Herdr session list omitted the sessions array"))?;
    sessions
        .iter()
        .map(|session| {
            let name = session
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| operation_failed("Herdr session list omitted a session name"))?;
            let socket_path = session
                .get("socket_path")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    operation_failed(format!("Herdr session '{name}' omitted its socket path"))
                })?;
            Ok(HerdrSessionInfo {
                name: name.to_owned(),
                running: session
                    .get("running")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                socket_path: socket_path.into(),
            })
        })
        .collect()
}

#[cfg(unix)]
pub fn list_herdr_sessions() -> Result<Vec<HerdrSessionInfo>, RuntimeError> {
    let output = Command::new(herdr_program())
        .args(["session", "list", "--json"])
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                RuntimeError::BackendUnavailable {
                    reason: BackendUnavailableReason::BackendBinaryNotFound {
                        binary: "herdr".to_owned(),
                    },
                    detail: error.to_string(),
                }
            } else {
                operation_failed(format!("failed to list Herdr sessions: {error}"))
            }
        })?;
    if !output.status.success() {
        return Err(operation_failed(format!(
            "Herdr session list exited with status {}: {}",
            output.status,
            command_output_text(&output).trim()
        )));
    }
    parse_herdr_sessions(&output.stdout)
}
fn operation_failed(message: impl Into<String>) -> RuntimeError {
    RuntimeError::OperationFailed {
        message: message.into(),
    }
}

fn unmanaged_herdr_stop_hint(session: &str) -> String {
    format!("`herdr session stop {session}`")
}

#[cfg(unix)]
fn managed_herdr_not_owned(session: &str) -> RuntimeError {
    operation_failed(format!(
        "the Herdr session is not managed by this Workhive installation; stop it with {}",
        unmanaged_herdr_stop_hint(session)
    ))
}

/// Ensure a configured Herdr session is available without taking ownership of
/// its workspaces. A healthy server is reused; only an absent server is started,
/// detached, through Herdr's documented named-session CLI.
///
/// `HERDR_SOCKET_PATH` is honored only by isolated tests. Named-session servers
/// ignore it and bind the named session's own socket.
#[cfg(unix)]
pub fn ensure_herdr_session_server(session: &str) -> Result<std::path::PathBuf, RuntimeError> {
    let state_dir = crate::config::resolve_global_config_dir().map_err(|error| {
        operation_failed(format!(
            "failed to resolve Workhive state directory: {error}"
        ))
    })?;
    ensure_herdr_session_server_in(state_dir.as_std_path(), session)
}

#[cfg(unix)]
#[cfg(test)]
fn ensure_workhive_herdr_server_in(
    state_dir: &std::path::Path,
) -> Result<std::path::PathBuf, RuntimeError> {
    ensure_herdr_session_server_in(state_dir, WORKHIVE_HERDR_SESSION)
}

#[cfg(unix)]
fn ensure_herdr_session_server_in(
    state_dir: &std::path::Path,
    session: &str,
) -> Result<std::path::PathBuf, RuntimeError> {
    if let Some(socket) = resolve_herdr_socket_path_for_session(session) {
        match probe_herdr_socket(&socket) {
            Ok(()) => return Ok(socket),
            Err(error) if herdr_probe_allows_start(&error) => {}
            Err(error) => return Err(error),
        }
    }

    std::fs::create_dir_all(state_dir).map_err(|error| {
        operation_failed(format!(
            "failed to create Workhive state directory: {error}"
        ))
    })?;
    let Some(_lock) = try_acquire_herdr_start_lock(&state_dir.join(HERDR_START_LOCK))? else {
        return wait_for_herdr_server(session);
    };

    if let Some(socket) = resolve_herdr_socket_path_for_session(session)
        && probe_herdr_socket(&socket).is_ok()
    {
        return Ok(socket);
    }

    // Named `herdr --session <name> server` ignores HERDR_SOCKET_PATH and
    // binds the session's own socket. Only start it after the second probe
    // that second probe still shows it is not running.
    //
    // `socket_override()` is test-only; see its doc comment.
    let override_socket = socket_override();

    if let Some(parent) = override_socket
        .as_ref()
        .map(std::path::Path::new)
        .and_then(std::path::Path::parent)
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            operation_failed(format!("failed to create Herdr socket directory: {error}"))
        })?;
    }

    let token = uuid::Uuid::new_v4().to_string();
    let mut command = Command::new(herdr_program());
    if override_socket.is_some() {
        command.args(["server"]);
    } else {
        command.args(["--session", session, "server"]);
    }
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env(WORKHIVE_MANAGED_TOKEN_ENV, &token);
    if let Some(socket) = override_socket.as_ref() {
        command.env("HERDR_SOCKET_PATH", socket);
    }
    let mut child = spawn_detached_herdr_server(&mut command).map_err(map_herdr_spawn_error)?;
    match wait_for_herdr_server(session) {
        Ok(socket) => {
            let owner = ManagedHerdrServer {
                pid: child.id(),
                socket: socket.clone(),
                session: session.to_owned(),
                token,
            };
            if let Err(error) = write_managed_herdr_record(state_dir, &owner) {
                // Recycle only the child we spawned. Do not send `herdr server
                // stop` to the endpoint: ownership was never recorded, so a
                // full stop could hit a live named session we do not own.
                // The server is detached but still a child until it exits, so
                // killing the pid we just spawned is enough here.
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Ok(socket)
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            let record_path = managed_herdr_record_path(state_dir, session);
            let _ = std::fs::remove_file(record_path);
            Err(error)
        }
    }
}

/// Spawn a configured Herdr server in a session of its own.
///
/// A hangup signal reaches the foreground process group, so the server must
/// run in its own detached session.
#[cfg(unix)]
fn spawn_detached_herdr_server(command: &mut Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;

    // `setsid` runs in the post-fork child because std exposes no detached
    // spawn: `Command::pre_exec` guarantees the command's argv and environment
    // were copied before the fork, and the closure itself only issues one
    // syscall.
    // SAFETY: per the `pre_exec` contract the closure must only make
    // async-signal-safe calls in the child. This closure issues `setsid` and
    // nothing else — no allocation, no locks, no Rust state. A failure is
    // ignored deliberately: the child is a fresh process, never a process
    // group leader, so the call cannot fail in any way the caller could act on,
    // and reporting it would require allocation the child must not do.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    spawn_retrying_etxtbsy(command)
}

/// Spawn, retrying while the target image is briefly not executable.
///
/// `execve` refuses an image with `ETXTBSY` ("Text file busy") while any
/// process still holds a write reference to it. There are two real ways to hit
/// that window:
///
/// * `herdr update` replaces the installed binary. A spawn racing that write
///   would otherwise surface to the user as "failed to start Herdr server".
/// * `fork()` copies the whole descriptor table, so a child keeps the parent's
///   write reference alive for any file another thread was writing at that
///   instant. A test that execs a script it just wrote can therefore lose a
///   race it did not create — hence the retry here instead of per test.
///
/// The window is short and bounded, so a few spaced retries settle it.
#[cfg(unix)]
fn spawn_retrying_etxtbsy(command: &mut Command) -> std::io::Result<std::process::Child> {
    const ATTEMPTS: u64 = 10;
    for attempt in 1..=ATTEMPTS {
        match command.spawn() {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && attempt < ATTEMPTS => {
                std::thread::sleep(std::time::Duration::from_millis(10 * attempt));
            }
            settled => return settled,
        }
    }
    unreachable!("the final attempt always returns")
}

#[cfg(unix)]
fn map_herdr_spawn_error(error: std::io::Error) -> RuntimeError {
    if error.kind() == std::io::ErrorKind::NotFound {
        RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::BackendBinaryNotFound {
                binary: "herdr".to_owned(),
            },
            detail: error.to_string(),
        }
    } else {
        operation_failed(format!("failed to start Herdr server: {error}"))
    }
}

#[cfg(unix)]
fn try_acquire_herdr_start_lock(
    lock_path: &std::path::Path,
) -> Result<Option<HerdrStartLock>, RuntimeError> {
    let mut lock = fslock::LockFile::open(lock_path)
        .map_err(|error| operation_failed(format!("failed to open Herdr start lock: {error}")))?;
    let acquired = lock.try_lock().map_err(|error| {
        operation_failed(format!("failed to acquire Herdr start lock: {error}"))
    })?;
    Ok(acquired.then_some(HerdrStartLock { _lock: lock }))
}

#[cfg(unix)]
fn probe_herdr_socket(socket: &std::path::Path) -> Result<(), RuntimeError> {
    crate::herdr::HerdrRuntimeAdapter::connect(socket).map(|_| ())
}

#[cfg(unix)]
fn herdr_probe_allows_start(error: &RuntimeError) -> bool {
    matches!(
        error,
        RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning { .. }
                | BackendUnavailableReason::SocketNotFound { .. },
            ..
        }
    )
}

#[cfg(unix)]
fn herdr_ready_timeout() -> std::time::Duration {
    std::env::var("WORKHIVE_HERDR_READY_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse().ok())
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(5))
}

#[cfg(unix)]
fn wait_for_herdr_server(session: &str) -> Result<std::path::PathBuf, RuntimeError> {
    let deadline = std::time::Instant::now() + herdr_ready_timeout();
    let expected = socket_override();
    while std::time::Instant::now() < deadline {
        if let Some(socket) = resolve_herdr_socket_path_for_session(session)
            && expected
                .as_ref()
                .is_none_or(|expected| std::path::Path::new(expected) == socket)
            && probe_herdr_socket(&socket).is_ok()
        {
            return Ok(socket);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Err(RuntimeError::BackendUnavailable {
        reason: BackendUnavailableReason::ServerNotRunning {
            server: expected
                .map(|socket| socket.to_string_lossy().into_owned())
                .unwrap_or_else(|| session.to_owned()),
        },
        detail: "Herdr server did not become ready before the startup timeout".to_owned(),
    })
}

#[cfg(unix)]
fn write_managed_herdr_record(
    state_dir: &std::path::Path,
    owner: &ManagedHerdrServer,
) -> Result<(), RuntimeError> {
    let encoded = serde_json::to_vec(owner).map_err(|error| {
        operation_failed(format!("failed to encode managed Herdr server: {error}"))
    })?;
    std::fs::write(
        managed_herdr_record_path(state_dir, &owner.session),
        encoded,
    )
    .map_err(|error| operation_failed(format!("failed to record managed Herdr server: {error}")))
}

#[cfg(unix)]
fn read_managed_herdr_record(
    state_dir: &std::path::Path,
    session: &str,
) -> Result<ManagedHerdrServer, RuntimeError> {
    let bytes = std::fs::read(managed_herdr_record_path(state_dir, session))
        .map_err(|_| managed_herdr_not_owned(session))?;
    let owner: ManagedHerdrServer = serde_json::from_slice(&bytes).map_err(|_| {
        operation_failed(format!(
            "the Herdr session ownership record is invalid; stop an unmanaged session with {}",
            unmanaged_herdr_stop_hint(session)
        ))
    })?;
    if owner.session != session {
        return Err(managed_herdr_not_owned(session));
    }
    Ok(owner)
}

#[cfg(unix)]
fn managed_herdr_record_path(state_dir: &std::path::Path, session: &str) -> std::path::PathBuf {
    if session == WORKHIVE_HERDR_SESSION {
        return state_dir.join(MANAGED_HERDR_RECORD);
    }
    let encoded: String = session
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    state_dir.join(format!("herdr-managed-{encoded}.json"))
}

fn command_output_text(output: &std::process::Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[cfg(unix)]
fn split_nul_strings(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|token| !token.is_empty())
        .map(|token| String::from_utf8_lossy(token).into_owned())
        .collect()
}

#[cfg(unix)]
fn parse_env_entries(entries: &[String]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(unix)]
type ProcessArgvEnv = (Vec<String>, Vec<(String, String)>);

/// Parse macOS `KERN_PROCARGS2` into argv and environment entries.
///
/// The buffer layout is `argc: i32`, the executable path, optional padding
/// NULs, `argc` argument strings, then `KEY=VALUE` environment strings.
#[cfg(all(unix, any(test, target_os = "macos")))]
fn parse_kern_procargs2(buf: &[u8]) -> Option<ProcessArgvEnv> {
    if buf.len() < 4 {
        return None;
    }
    let argc = i32::from_ne_bytes(buf[0..4].try_into().ok()?) as usize;
    let mut cursor = 4;
    while cursor < buf.len() && buf[cursor] != 0 {
        cursor += 1;
    }
    cursor += 1;
    while cursor < buf.len() && buf[cursor] == 0 {
        cursor += 1;
    }
    let mut tokens = Vec::new();
    for _ in 0..argc {
        if cursor >= buf.len() {
            return None;
        }
        let start = cursor;
        while cursor < buf.len() && buf[cursor] != 0 {
            cursor += 1;
        }
        if start == cursor {
            return None;
        }
        tokens.push(String::from_utf8_lossy(&buf[start..cursor]).into_owned());
        cursor += 1;
    }
    let env = parse_env_entries(&split_nul_strings(&buf[cursor.min(buf.len())..]));
    (!tokens.is_empty()).then_some((tokens, env))
}

#[cfg(target_os = "linux")]
fn process_command_and_env(pid: u32) -> Option<ProcessArgvEnv> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let tokens = split_nul_strings(&cmdline);
    if tokens.is_empty() {
        return None;
    }
    let env = std::fs::read(format!("/proc/{pid}/environ"))
        .ok()
        .map(|bytes| parse_env_entries(&split_nul_strings(&bytes)))
        .unwrap_or_default();
    Some((tokens, env))
}

#[cfg(target_os = "macos")]
fn process_command_and_env(pid: u32) -> Option<ProcessArgvEnv> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
    let mut size = 0usize;
    let probe = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if probe != 0 || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    let loaded = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if loaded != 0 {
        return None;
    }
    buf.truncate(size);
    parse_kern_procargs2(&buf)
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_command_and_env(_pid: u32) -> Option<ProcessArgvEnv> {
    None
}

#[cfg(unix)]
fn argv_contains_herdr(tokens: &[String]) -> bool {
    // Match `herdr` anywhere in argv so wrappers such as `env`/`nice` still
    // count. A `sh -c 'herdr ...'` one-liner keeps `herdr` inside a single
    // token and is rejected; that fail-closed outcome is intentional.
    tokens.iter().any(|token| {
        std::path::Path::new(token)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "herdr")
    })
}

#[cfg(unix)]
fn argv_matches_managed_herdr(
    tokens: &[String],
    env_socket: Option<&str>,
    owner: &ManagedHerdrServer,
) -> bool {
    let has_server = tokens.iter().any(|token| token == "server");
    let has_named_session = tokens
        .windows(2)
        .any(|window| window[0] == "--session" && window[1] == owner.session);
    if !argv_contains_herdr(tokens) || !has_server {
        return false;
    }
    match env_socket {
        Some(socket) => std::path::Path::new(socket) == owner.socket,
        None => has_named_session,
    }
}

#[cfg(unix)]
fn env_pair_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    env.iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

#[cfg(unix)]
fn managed_token_matches(owner: &ManagedHerdrServer, env: &[(String, String)]) -> bool {
    if owner.token.is_empty() {
        return false;
    }
    env_pair_value(env, WORKHIVE_MANAGED_TOKEN_ENV).is_some_and(|value| value == owner.token)
}

#[cfg(unix)]
fn managed_process_matches(owner: &ManagedHerdrServer) -> bool {
    let Some((tokens, env)) = process_command_and_env(owner.pid) else {
        return false;
    };
    argv_matches_managed_herdr(&tokens, env_pair_value(&env, "HERDR_SOCKET_PATH"), owner)
        && managed_token_matches(owner, &env)
}

#[cfg(unix)]
fn herdr_endpoint_socket(socket: &std::path::Path) -> Option<std::path::PathBuf> {
    let output = Command::new(herdr_program())
        .env("HERDR_SOCKET_PATH", socket)
        .args(["status", "server", "--json"])
        .output()
        .ok()?;
    parse_endpoint_socket_path(&command_output_text(&output)).filter(|reported| reported == socket)
}

#[cfg(unix)]
fn herdr_endpoint_usage(socket: &std::path::Path) -> Result<Option<String>, RuntimeError> {
    let output = Command::new(herdr_program())
        .env("HERDR_SOCKET_PATH", socket)
        .args(["api", "snapshot"])
        .output()
        .map_err(|error| operation_failed(format!("failed to read Herdr snapshot: {error}")))?;
    if !output.status.success() {
        return Err(operation_failed(
            "failed to read Herdr snapshot; refusing to stop the managed server",
        ));
    }
    parse_herdr_snapshot_usage(&command_output_text(&output))
}

fn parse_herdr_snapshot_usage(output: &str) -> Result<Option<String>, RuntimeError> {
    let value = serde_json::from_str::<Value>(output.trim())
        .ok()
        .or_else(|| {
            output.lines().find_map(|line| {
                let trimmed = line.trim();
                (!trimmed.is_empty())
                    .then(|| serde_json::from_str::<Value>(trimmed).ok())
                    .flatten()
            })
        })
        .ok_or_else(|| operation_failed("failed to parse Herdr snapshot; refusing to stop"))?;
    let root = value.get("result").unwrap_or(&value);
    let root = root.get("snapshot").unwrap_or(root);
    let count = |key: &str| -> Result<usize, RuntimeError> {
        root.get(key)
            .and_then(Value::as_array)
            .map(Vec::len)
            .ok_or_else(|| {
                operation_failed(format!(
                    "Herdr snapshot is missing the required '{key}' array; refusing to stop"
                ))
            })
    };
    let workspaces = count("workspaces")?;
    let panes = count("panes")?;
    let agents = count("agents")?;
    if workspaces == 0 && panes == 0 && agents == 0 {
        Ok(None)
    } else {
        Ok(Some(format!(
            "{workspaces} workspace(s), {panes} pane(s), {agents} agent(s)"
        )))
    }
}

/// Stop the configured default Herdr session only when this process was started by
/// Workhive. A missing ownership record is deliberately an error: users
/// must stop servers they started themselves with the Herdr CLI.
#[cfg(unix)]
pub fn stop_managed_workhive_herdr_server(force: bool) -> Result<(), RuntimeError> {
    let state_dir = crate::config::resolve_global_config_dir().map_err(|error| {
        operation_failed(format!(
            "failed to resolve Workhive state directory: {error}"
        ))
    })?;
    stop_managed_herdr_server_in(state_dir.as_std_path(), WORKHIVE_HERDR_SESSION, force)
}

#[cfg(unix)]
#[cfg(test)]
fn stop_managed_workhive_herdr_server_in(
    state_dir: &std::path::Path,
    force: bool,
) -> Result<(), RuntimeError> {
    stop_managed_herdr_server_in(state_dir, WORKHIVE_HERDR_SESSION, force)
}

#[cfg(unix)]
fn stop_managed_herdr_server_in(
    state_dir: &std::path::Path,
    session: &str,
    force: bool,
) -> Result<(), RuntimeError> {
    let owner = read_managed_herdr_record(state_dir, session)?;
    if !managed_process_matches(&owner) {
        // Ownership is proven by the spawn token in the child's environment.
        // A platform that cannot read that environment never observed a
        // mismatch, so the message covers both cases truthfully.
        return Err(operation_failed(format!(
            "the managed Herdr server could not be verified as owned by this installation (its ownership record does not match a live Workhive Herdr server, or this platform cannot inspect process argv/environment); stop an unmanaged server with {}",
            unmanaged_herdr_stop_hint(&owner.session)
        )));
    }
    let reported = herdr_endpoint_socket(&owner.socket).ok_or_else(|| {
        operation_failed("failed to resolve the managed Herdr endpoint before stopping it")
    })?;
    if reported != owner.socket {
        return Err(operation_failed(format!(
            "refusing to stop Herdr: endpoint '{}' does not match owned socket '{}'",
            reported.display(),
            owner.socket.display()
        )));
    }
    if let Some(summary) = herdr_endpoint_usage(&owner.socket)?
        && !force
    {
        return Err(operation_failed(format!(
            "refusing to stop the managed Herdr server while it is in use ({summary}); pass --force to stop it anyway"
        )));
    }
    let status = Command::new(herdr_program())
        .env("HERDR_SOCKET_PATH", &owner.socket)
        .args(["server", "stop"])
        .status()
        .map_err(|error| {
            operation_failed(format!("failed to stop managed Herdr server: {error}"))
        })?;
    if !status.success() {
        return Err(operation_failed(
            "Herdr rejected the managed server stop request",
        ));
    }
    std::fs::remove_file(managed_herdr_record_path(state_dir, &owner.session)).map_err(|error| {
        operation_failed(format!(
            "failed to clear managed Herdr ownership record: {error}"
        ))
    })
}

/// Resolve the socket used by Workhive's configured default Herdr session.
///
/// The named-session status command is authoritative. The environment
/// variable remains an explicit override for custom or test endpoints; we do
/// not fall back to Herdr's default (user) session.
pub fn resolve_herdr_socket_path() -> Option<std::path::PathBuf> {
    resolve_herdr_socket_path_for_session(WORKHIVE_HERDR_SESSION)
}

/// Resolve the socket for a named Herdr session. `HERDR_SOCKET_PATH` is kept
/// as an explicit alternate-endpoint override; ordinary operation discovers
/// the requested native session through Herdr's status command.
pub fn resolve_herdr_socket_path_for_session(session: &str) -> Option<std::path::PathBuf> {
    if let Some(socket) = socket_override() {
        return Some(socket.into());
    }
    discover_herdr_session_socket_path(session)
}

/// Discover Workhive's default named Herdr endpoint without consulting an override.
pub fn discover_workhive_herdr_socket_path() -> Option<std::path::PathBuf> {
    discover_herdr_session_socket_path(WORKHIVE_HERDR_SESSION)
}

/// Discover a named Herdr endpoint without consulting an environment override.
pub fn discover_herdr_session_socket_path(session: &str) -> Option<std::path::PathBuf> {
    if let Ok(output) = Command::new(herdr_program())
        .args(herdr_status_args(session))
        .output()
        && let Some(socket) =
            parse_named_session_socket_path(&command_output_text(&output), session)
    {
        return Some(socket);
    }

    default_herdr_socket_path_for_session(session)
}

fn herdr_status_args(session: &str) -> [String; 5] {
    [
        "--session".to_owned(),
        session.to_owned(),
        "status".to_owned(),
        "server".to_owned(),
        "--json".to_owned(),
    ]
}

#[cfg(test)]
fn parse_socket_path(output: &str) -> Option<std::path::PathBuf> {
    parse_named_session_socket_path(output, WORKHIVE_HERDR_SESSION)
}

fn parse_endpoint_socket_path(output: &str) -> Option<std::path::PathBuf> {
    if let Ok(value) = serde_json::from_str::<Value>(output.trim())
        && let Some(socket) = value.get("socket").and_then(Value::as_str)
        && !socket.is_empty()
    {
        return Some(std::path::PathBuf::from(socket));
    }

    output.lines().find_map(|line| {
        let path = line.trim().strip_prefix("socket:")?.trim();
        (!path.is_empty()).then(|| std::path::PathBuf::from(path))
    })
}

fn parse_named_session_socket_path(output: &str, session: &str) -> Option<std::path::PathBuf> {
    if let Ok(value) = serde_json::from_str::<Value>(output.trim())
        && value
            .get("session")
            .and_then(Value::as_str)
            .is_some_and(|reported| reported == session)
    {
        return parse_endpoint_socket_path(output);
    }

    let named_marker = format!("sessions/{session}/");
    output.lines().find_map(|line| {
        let path = line.trim().strip_prefix("socket:")?.trim();
        (path.contains(&named_marker) && !path.is_empty()).then(|| std::path::PathBuf::from(path))
    })
}

/// Return the most useful socket path to show when discovery finds no live
/// Herdr endpoint. This is only a diagnostic hint; callers must not assume
/// the path exists.
pub fn herdr_socket_path_hint() -> std::path::PathBuf {
    herdr_socket_path_hint_for_session(WORKHIVE_HERDR_SESSION)
}

pub fn herdr_socket_path_hint_for_session(session: &str) -> std::path::PathBuf {
    if let Some(socket) = socket_override() {
        return socket.into();
    }

    default_herdr_socket_candidates_for_session(session)
        .into_iter()
        .next()
        .unwrap_or_else(|| {
            std::path::PathBuf::from(format!("~/.config/herdr/sessions/{session}/herdr.sock"))
        })
}

fn default_herdr_socket_path_for_session(session: &str) -> Option<std::path::PathBuf> {
    default_herdr_socket_candidates_for_session(session)
        .into_iter()
        .find(|path| path.exists())
}

#[cfg(test)]
fn default_herdr_socket_candidates() -> Vec<std::path::PathBuf> {
    default_herdr_socket_candidates_for_session(WORKHIVE_HERDR_SESSION)
}

fn default_herdr_socket_candidates_for_session(session: &str) -> Vec<std::path::PathBuf> {
    let mut candidates = Vec::new();
    let session_suffix = std::path::Path::new("herdr")
        .join("sessions")
        .join(session)
        .join("herdr.sock");

    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        candidates.push(std::path::PathBuf::from(xdg).join(&session_suffix));
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        candidates.push(
            std::path::PathBuf::from(home)
                .join(".config")
                .join(&session_suffix),
        );
    }
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR")
        && !xdg.is_empty()
    {
        candidates.push(std::path::PathBuf::from(xdg).join(&session_suffix));
    }

    #[cfg(unix)]
    if let Ok(output) = Command::new("id").arg("-u").output()
        && let Ok(uid) = String::from_utf8(output.stdout)
    {
        candidates.push(std::path::PathBuf::from(format!(
            "/tmp/herdr-{}/herdr/sessions/{session}/herdr.sock",
            uid.trim(),
        )));
    }

    candidates
}

/// Real system dependency checker.
pub struct SystemDependencyChecker;

impl SystemDependencyChecker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SystemDependencyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyPort for SystemDependencyChecker {
    fn check_all(&self) -> Vec<DependencyStatus> {
        vec![
            self.check(DependencyKind::Git),
            self.check(DependencyKind::Tmux),
        ]
    }

    fn check(&self, kind: DependencyKind) -> DependencyStatus {
        match kind {
            DependencyKind::Git => {
                check_command("git", &["--version"], "git version ", GIT_MIN_VERSION, kind)
            }
            DependencyKind::Tmux => check_command("tmux", &["-V"], "tmux ", TMUX_MIN_VERSION, kind),
            DependencyKind::Herdr => check_command(
                herdr_program(),
                &["--version"],
                "herdr ",
                HERDR_MIN_VERSION,
                kind,
            ),
        }
    }

    fn preflight(&self, kind: DependencyKind) -> BackendPreflight {
        if kind != DependencyKind::Herdr {
            return BackendPreflight::from_dependency(self.check(kind));
        }

        let dependency = self.check(kind);
        if !dependency.is_ok() {
            return BackendPreflight::from_dependency(dependency);
        }

        // Try the explicit override, Herdr's reported endpoint, then legacy
        // default locations. A running server is required because Workhive
        // speaks directly to its private protocol socket.
        let Some(socket) = resolve_herdr_socket_path() else {
            let detail = format!(
                "Herdr is installed, but the configured session socket was not found; retry the Workhive command, which starts `{WORKHIVE_HERDR_SESSION}` detached, or start `herdr --session {WORKHIVE_HERDR_SESSION} server` under a service supervisor"
            );
            return BackendPreflight {
                kind,
                installed: true,
                startable: true,
                running: false,
                compatible: false,
                version: Some(HERDR_MIN_VERSION.to_owned()),
                detail: Some(detail.clone()),
                runtime_error: Some(RuntimeError::BackendUnavailable {
                    reason: BackendUnavailableReason::SocketNotFound {
                        path: herdr_socket_path_hint().to_string_lossy().into_owned(),
                    },
                    detail,
                }),
            };
        };

        match crate::herdr::HerdrRuntimeAdapter::connect(socket) {
            Ok(_) => BackendPreflight {
                kind,
                installed: true,
                startable: true,
                running: true,
                compatible: true,
                version: Some("protocol 22".to_owned()),
                detail: None,
                runtime_error: None,
            },
            Err(error) => {
                let detail = error.to_string();
                BackendPreflight {
                    kind,
                    installed: true,
                    startable: false,
                    running: false,
                    compatible: false,
                    version: None,
                    detail: Some(detail),
                    runtime_error: Some(error),
                }
            }
        }
    }
}

/// Probe a system command, parse its version, and compare against a minimum.
fn check_command(
    binary: impl AsRef<std::ffi::OsStr>,
    args: &[&str],
    version_prefix: &str,
    min_version: &str,
    kind: DependencyKind,
) -> DependencyStatus {
    let binary = binary.as_ref();
    let binary_name = binary.to_string_lossy();
    let output = Command::new(binary).args(args).output();

    let output = match output {
        Ok(o) => o,
        Err(_) => {
            tracing::debug!(binary = %binary_name, "dependency not found on PATH");
            return DependencyStatus::Missing { kind };
        }
    };

    if !output.status.success() {
        tracing::debug!(binary = %binary_name, code = ?output.status.code(), "version command failed");
        return DependencyStatus::Missing { kind };
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = extract_version(&stdout, version_prefix);

    match version {
        Some(v) => {
            if version_satisfies(&v, min_version) {
                tracing::debug!(binary = %binary_name, version = %v, "dependency satisfied");
                DependencyStatus::Satisfied { kind, version: v }
            } else {
                tracing::warn!(binary = %binary_name, found = %v, minimum = min_version, "dependency version too low");
                DependencyStatus::VersionTooLow {
                    kind,
                    found: v,
                    minimum: min_version.to_owned(),
                }
            }
        }
        None => {
            tracing::warn!(binary = %binary_name, raw_output = %stdout.trim(), "could not parse version");
            DependencyStatus::Missing { kind }
        }
    }
}

/// Extract a semver-like version string from command output.
///
/// Handles formats like:
/// - `git version 2.43.0` → `2.43.0`
/// - `tmux 3.4` → `3.4`
/// - `tmux next-3.5` → `3.5`
fn extract_version(output: &str, prefix: &str) -> Option<String> {
    let line = output.lines().next()?;
    let after = line.strip_prefix(prefix).unwrap_or(line);

    let version_part = after
        .trim()
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>();

    if version_part.is_empty() {
        None
    } else {
        Some(version_part)
    }
}

/// Simple numeric version comparison (major.minor.patch).
///
/// Compares segment-by-segment: `2.43.0 >= 2.20.0` → true.
fn version_satisfies(found: &str, minimum: &str) -> bool {
    let found_parts: Vec<u32> = found.split('.').filter_map(|s| s.parse().ok()).collect();
    let min_parts: Vec<u32> = minimum.split('.').filter_map(|s| s.parse().ok()).collect();

    let max_len = found_parts.len().max(min_parts.len());
    for i in 0..max_len {
        let f = found_parts.get(i).copied().unwrap_or(0);
        let m = min_parts.get(i).copied().unwrap_or(0);
        if f > m {
            return true;
        }
        if f < m {
            return false;
        }
    }
    true // equal
}

/// Detected OS platform for install hint routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    /// Detect the current platform at runtime.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Platform-specific install hint for a dependency.
///
/// Returns only the command relevant to the detected OS, not all platforms.
pub fn install_hint(kind: DependencyKind) -> String {
    install_hint_for(kind, Platform::detect())
}

/// Install hint for a specific platform (testable).
pub fn install_hint_for(kind: DependencyKind, platform: Platform) -> String {
    match (kind, platform) {
        (DependencyKind::Git, Platform::MacOs) => "    brew install git".into(),
        (DependencyKind::Git, Platform::Linux) => "    sudo apt install git  # Debian/Ubuntu\n    \
             sudo dnf install git  # Fedora\n    \
             sudo pacman -S git    # Arch"
            .into(),
        (DependencyKind::Git, Platform::Windows) => {
            "    winget install Git.Git  # or: scoop install git".into()
        }
        (DependencyKind::Tmux, Platform::MacOs) => "    brew install tmux".into(),
        (DependencyKind::Tmux, Platform::Linux) => {
            "    sudo apt install tmux  # Debian/Ubuntu\n    \
             sudo dnf install tmux  # Fedora\n    \
             sudo pacman -S tmux    # Arch"
                .into()
        }
        (DependencyKind::Tmux, Platform::Windows) => {
            "    WSL required — install tmux inside WSL:\n    \
             wsl --install && wsl sudo apt install tmux"
                .into()
        }
        (DependencyKind::Herdr, _) => {
            "    Install Herdr from https://github.com/herdrdev/herdr or via your package manager"
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn parses_native_session_list_with_running_state() {
        let sessions = parse_herdr_sessions(
            br#"{"sessions":[{"name":"shared","running":true,"socket_path":"/run/herdr/shared.sock"},{"name":"stopped","running":false,"socket_path":"/run/herdr/stopped.sock"}]}"#,
        )
        .unwrap();
        assert_eq!(
            sessions,
            vec![
                HerdrSessionInfo {
                    name: "shared".to_owned(),
                    running: true,
                    socket_path: "/run/herdr/shared.sock".into(),
                },
                HerdrSessionInfo {
                    name: "stopped".to_owned(),
                    running: false,
                    socket_path: "/run/herdr/stopped.sock".into(),
                },
            ]
        );
    }

    #[test]
    fn parses_socket_from_herdr_json_status_output() {
        let output = r#"{"status":"not_running","socket":"/home/user/.config/herdr/sessions/workhive/herdr.sock","session":"workhive"}"#;
        assert_eq!(
            parse_socket_path(output),
            Some(std::path::PathBuf::from(
                "/home/user/.config/herdr/sessions/workhive/herdr.sock"
            ))
        );
    }

    #[test]
    fn rejects_json_status_from_another_session() {
        let output = r#"{"socket":"/home/user/.config/herdr/herdr.sock","session":"default"}"#;
        assert_eq!(parse_socket_path(output), None);
    }

    #[test]
    fn rejects_default_text_status_socket() {
        assert_eq!(
            parse_socket_path("status: running\nsocket: /home/user/.config/herdr/herdr.sock\n"),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_does_not_look_like_a_startable_absence() {
        assert!(!herdr_probe_allows_start(&RuntimeError::PermissionDenied {
            detail: "permission denied".to_owned(),
        }));
    }

    #[test]
    fn endpoint_socket_parser_accepts_an_unnamed_alternate_server() {
        let output = r#"{"status":"running","socket":"/tmp/workhive-e2e.sock"}"#;
        assert_eq!(
            parse_endpoint_socket_path(output),
            Some(std::path::PathBuf::from("/tmp/workhive-e2e.sock"))
        );
        assert_eq!(
            parse_endpoint_socket_path("status: running\nsocket: /tmp/workhive-e2e.sock\n"),
            Some(std::path::PathBuf::from("/tmp/workhive-e2e.sock"))
        );
    }

    #[test]
    fn named_session_discovery_targets_and_validates_the_requested_session() {
        assert_eq!(
            herdr_status_args("private"),
            ["--session", "private", "status", "server", "--json",]
        );
        let output = r#"{"session":"private","socket":"/tmp/herdr/sessions/private/herdr.sock"}"#;
        assert_eq!(
            parse_named_session_socket_path(output, "private"),
            Some(std::path::PathBuf::from(
                "/tmp/herdr/sessions/private/herdr.sock"
            ))
        );
        assert_eq!(parse_named_session_socket_path(output, "other"), None);
    }

    #[test]
    fn requests_the_workhive_named_session() {
        assert_eq!(
            herdr_status_args(WORKHIVE_HERDR_SESSION),
            [
                "--session",
                WORKHIVE_HERDR_SESSION,
                "status",
                "server",
                "--json"
            ]
        );
    }

    #[cfg(unix)]
    fn sample_owner(socket: &str) -> ManagedHerdrServer {
        ManagedHerdrServer {
            pid: 1,
            socket: std::path::PathBuf::from(socket),
            session: WORKHIVE_HERDR_SESSION.to_owned(),
            token: "workhive-managed-token".to_owned(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn ownership_matches_wrapper_argv_that_still_execs_herdr() {
        let owner = sample_owner("/tmp/workhive.sock");
        let tokens = vec![
            "/usr/bin/env".to_owned(),
            "/opt/herdr".to_owned(),
            "--session".to_owned(),
            WORKHIVE_HERDR_SESSION.to_owned(),
            "server".to_owned(),
        ];
        assert!(argv_matches_managed_herdr(&tokens, None, &owner));
    }

    #[cfg(unix)]
    #[test]
    fn ownership_rejects_unrelated_pid_image() {
        let owner = sample_owner("/tmp/workhive.sock");
        let tokens = vec!["/bin/sleep".to_owned(), "30".to_owned()];
        assert!(!argv_matches_managed_herdr(&tokens, None, &owner));
    }

    #[cfg(unix)]
    #[test]
    fn ownership_requires_managed_token_even_when_argv_and_socket_match() {
        let owner = sample_owner("/tmp/workhive.sock");
        let matching_argv = [
            "herdr".to_owned(),
            "--session".to_owned(),
            WORKHIVE_HERDR_SESSION.to_owned(),
            "server".to_owned(),
        ];
        assert!(argv_matches_managed_herdr(
            &matching_argv,
            Some("/tmp/workhive.sock"),
            &owner
        ));
        assert!(!managed_token_matches(
            &owner,
            &[
                (
                    "HERDR_SOCKET_PATH".to_owned(),
                    "/tmp/workhive.sock".to_owned()
                ),
                (
                    WORKHIVE_MANAGED_TOKEN_ENV.to_owned(),
                    "reused-pid-token".to_owned()
                ),
            ]
        ));
        assert!(managed_token_matches(
            &owner,
            &[(
                WORKHIVE_MANAGED_TOKEN_ENV.to_owned(),
                "workhive-managed-token".to_owned()
            )]
        ));
        let mut empty_token = owner.clone();
        empty_token.token.clear();
        assert!(!managed_token_matches(
            &empty_token,
            &[(WORKHIVE_MANAGED_TOKEN_ENV.to_owned(), String::new())]
        ));
    }

    #[cfg(unix)]
    #[test]
    fn ownership_requires_named_session_when_socket_env_is_absent() {
        let owner = sample_owner("/tmp/workhive.sock");
        let tokens = vec!["herdr".to_owned(), "server".to_owned()];
        assert!(!argv_matches_managed_herdr(&tokens, None, &owner));
        assert!(argv_matches_managed_herdr(
            &tokens,
            Some("/tmp/workhive.sock"),
            &owner
        ));
    }

    #[cfg(unix)]
    #[test]
    fn kern_procargs2_parser_round_trips_argv_and_socket_env() {
        let mut buf = Vec::new();
        let argv = ["herdr", "--session", WORKHIVE_HERDR_SESSION, "server"];
        buf.extend_from_slice(&(argv.len() as i32).to_ne_bytes());
        buf.extend_from_slice(b"/usr/local/bin/herdr\0\0");
        for token in argv {
            buf.extend_from_slice(token.as_bytes());
            buf.push(0);
        }
        buf.extend_from_slice(b"HERDR_SOCKET_PATH=/tmp/workhive.sock\0HOME=/tmp\0");
        let (tokens, env) = parse_kern_procargs2(&buf).expect("parse KERN_PROCARGS2 fixture");
        assert_eq!(tokens, argv);
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == "HERDR_SOCKET_PATH")
                .map(|(_, value)| value.as_str()),
            Some("/tmp/workhive.sock")
        );
    }

    #[test]
    fn named_socket_candidates_never_use_the_default_session() {
        let candidates = default_herdr_socket_candidates();
        assert!(!candidates.is_empty());
        assert!(candidates.iter().all(|path| {
            path.to_string_lossy()
                .contains(&format!("sessions/{WORKHIVE_HERDR_SESSION}/herdr.sock"))
        }));
    }

    #[test]
    fn parses_socket_from_herdr_status_output() {
        let output = "status: running\nversion: 0.9.0\nsocket: /home/user/.config/herdr/sessions/workhive/herdr.sock\n";
        assert_eq!(
            parse_socket_path(output),
            Some(std::path::PathBuf::from(
                "/home/user/.config/herdr/sessions/workhive/herdr.sock"
            ))
        );
    }

    #[test]
    fn parses_socket_with_indentation() {
        assert_eq!(
            parse_socket_path("  socket: /tmp/herdr/sessions/workhive/herdr.sock\n"),
            Some(std::path::PathBuf::from(
                "/tmp/herdr/sessions/workhive/herdr.sock",
            )),
        );
    }

    #[test]
    fn snapshot_without_workspaces_panes_or_agents_is_idle() {
        let usage = parse_herdr_snapshot_usage(
            r#"{"workspaces":[],"tabs":[{}],"panes":[],"layouts":[{}],"agents":[]}"#,
        )
        .expect("idle snapshot parses");
        assert_eq!(usage, None);
    }

    #[test]
    fn snapshot_envelope_from_herdr_cli_is_supported() {
        let usage = parse_herdr_snapshot_usage(
            r#"{"id":"cli:api:snapshot","result":{"snapshot":{"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[]}}}"#,
        )
        .expect("Herdr CLI snapshot envelope parses");
        assert_eq!(usage, None);
    }

    #[test]
    fn snapshot_with_panes_is_in_use() {
        let usage = parse_herdr_snapshot_usage(
            r#"{"workspaces":[],"tabs":[],"panes":[{"pane_id":"p1"}],"layouts":[],"agents":[]}"#,
        )
        .expect("in-use snapshot parses");
        assert_eq!(
            usage.as_deref(),
            Some("0 workspace(s), 1 pane(s), 0 agent(s)")
        );
    }

    #[test]
    fn snapshot_missing_required_array_fails_closed() {
        let error = parse_herdr_snapshot_usage(r#"{"workspaces":[],"panes":[]}"#).unwrap_err();
        assert!(
            error.to_string().contains("agents"),
            "missing snapshot arrays must fail closed: {error}"
        );
    }
    #[test]
    fn extract_git_version() {
        assert_eq!(
            extract_version("git version 2.43.0", "git version "),
            Some("2.43.0".to_owned())
        );
    }

    #[test]
    fn extract_git_version_with_extra() {
        assert_eq!(
            extract_version("git version 2.39.5 (Apple Git-154)", "git version "),
            Some("2.39.5".to_owned())
        );
    }

    #[test]
    fn extract_tmux_version() {
        assert_eq!(extract_version("tmux 3.4", "tmux "), Some("3.4".to_owned()));
    }

    #[test]
    fn extract_tmux_next_version() {
        assert_eq!(
            extract_version("tmux next-3.5", "tmux "),
            Some("3.5".to_owned())
        );
    }

    #[test]
    fn version_comparison() {
        assert!(version_satisfies("2.43.0", "2.20.0"));
        assert!(version_satisfies("3.4", "3.0"));
        assert!(version_satisfies("2.20.0", "2.20.0"));
        assert!(!version_satisfies("2.19.0", "2.20.0"));
        assert!(!version_satisfies("1.8", "2.0"));
        assert!(version_satisfies("3.5", "3.0"));
    }

    #[test]
    fn system_check_git_exists() {
        crate::test_env::with_exclusive_env(&[], || {
            let checker = SystemDependencyChecker::new();
            let status = checker.check(DependencyKind::Git);
            assert!(status.is_ok(), "git not found on system: {status:?}");
        });
    }

    #[test]
    fn install_hint_macos_returns_brew() {
        let hint = install_hint_for(DependencyKind::Git, Platform::MacOs);
        assert!(hint.contains("brew install git"), "expected brew: {hint}");
        assert!(!hint.contains("apt"), "macos hint should not mention apt");
    }

    #[test]
    fn install_hint_linux_returns_apt_and_dnf() {
        let hint = install_hint_for(DependencyKind::Tmux, Platform::Linux);
        assert!(hint.contains("apt install tmux"), "expected apt: {hint}");
        assert!(hint.contains("dnf install tmux"), "expected dnf: {hint}");
    }

    #[test]
    fn install_hint_windows_tmux_mentions_wsl() {
        let hint = install_hint_for(DependencyKind::Tmux, Platform::Windows);
        assert!(hint.contains("WSL"), "expected WSL: {hint}");
    }

    #[cfg(unix)]
    fn spawn_exclusive_lock_holder(lock_path: &std::path::Path) -> std::process::Child {
        use std::io::Read;
        use std::process::Stdio;

        let path = lock_path.to_str().expect("utf8 lock path");
        let python = concat!(
            "import fcntl, os, sys, time\n",
            "fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT)\n",
            "fcntl.flock(fd, fcntl.LOCK_EX)\n",
            "sys.stdout.write('LOCKED\\n')\n",
            "sys.stdout.flush()\n",
            "time.sleep(5)\n",
        );
        let mut child = Command::new("python3")
            .args(["-c", python, path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        if child.is_err() {
            let perl = concat!(
                "use Fcntl qw(:flock);",
                "open(my $fh, '+>>', $ARGV[0]) or die $!;",
                "flock($fh, LOCK_EX) or die $!;",
                "print \"LOCKED\\n\";",
                "select()->flush();",
                "sleep 5;",
            );
            child = Command::new("perl")
                .args(["-e", perl, path])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
        }
        let mut child = child.expect("python3 or perl must hold an exclusive flock");
        let mut stdout = child.stdout.take().expect("lock holder stdout");
        let mut buf = [0u8; 7];
        stdout
            .read_exact(&mut buf)
            .expect("lock holder must report once the flock is held");
        assert_eq!(&buf, b"LOCKED\n", "lock holder handshake");
        child
    }

    #[cfg(unix)]
    fn write_fake_herdr(bin_dir: &std::path::Path, script: &str) {
        std::fs::create_dir_all(bin_dir).expect("create fake bin dir");
        let path = bin_dir.join("herdr");
        std::fs::write(&path, script).expect("write fake herdr");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path)
                .expect("stat fake herdr")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("chmod fake herdr");
        }
    }

    #[cfg(unix)]
    fn wait_for_matching_decoy(
        child: &mut std::process::Child,
        owner: &ManagedHerdrServer,
    ) -> (Vec<String>, Vec<(String, String)>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        let mut last = None;
        loop {
            if let Some((tokens, env)) = process_command_and_env(owner.pid) {
                if argv_matches_managed_herdr(
                    &tokens,
                    env_pair_value(&env, "HERDR_SOCKET_PATH"),
                    owner,
                ) {
                    return (tokens, env);
                }
                last = Some((tokens, env));
            }
            if child
                .try_wait()
                .expect("poll decoy before argv probe")
                .is_some()
            {
                panic!("argv-matching decoy exited before its command line could be read");
            }
            if std::time::Instant::now() >= deadline {
                panic!("timed out waiting for decoy argv/socket/session to match: last={last:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    fn live_workhive_socket() -> Option<std::path::PathBuf> {
        let output = Command::new("herdr")
            .env_remove("HERDR_SOCKET_PATH")
            .args(["--session", "workhive", "status", "server", "--json"])
            .output()
            .ok()?;
        parse_named_session_socket_path(&command_output_text(&output), "workhive")
    }

    #[cfg(unix)]
    fn isolated_endpoint_status(socket: &std::path::Path) -> String {
        let output = Command::new("herdr")
            .env("HERDR_SOCKET_PATH", socket)
            .args(["status", "server", "--json"])
            .output()
            .expect("query isolated Herdr status");
        command_output_text(&output)
    }

    #[cfg(unix)]
    fn isolated_status_socket(output: &str) -> Option<std::path::PathBuf> {
        output.lines().find_map(|line| {
            serde_json::from_str::<Value>(line.trim())
                .ok()
                .and_then(|value| {
                    value
                        .get("socket")
                        .and_then(Value::as_str)
                        .filter(|socket| !socket.is_empty())
                        .map(std::path::PathBuf::from)
                })
        })
    }

    #[cfg(unix)]
    #[test]
    fn stop_without_ownership_record_refuses_and_names_herdr_stop() {
        let state = tempfile::tempdir().expect("state dir");
        let error = stop_managed_workhive_herdr_server_in(state.path(), false).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("not managed"),
            "missing ownership must refuse: {message}"
        );
        assert!(
            message.contains(&format!("herdr session stop {WORKHIVE_HERDR_SESSION}")),
            "recovery must name the official Herdr stop command: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_rejects_ownership_record_for_this_process_pid() {
        let state = tempfile::tempdir().expect("state dir");
        let owner = ManagedHerdrServer {
            pid: std::process::id(),
            socket: state.path().join("herdr.sock"),
            session: WORKHIVE_HERDR_SESSION.to_owned(),
            token: "spoofed-token".to_owned(),
        };
        write_managed_herdr_record(state.path(), &owner).expect("write spoofed record");
        let error = stop_managed_workhive_herdr_server_in(state.path(), false).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("does not match a live Workhive Herdr server"),
            "current test PID must not be treated as a Herdr server: {message}"
        );
        assert!(
            message.contains(&format!("herdr session stop {WORKHIVE_HERDR_SESSION}")),
            "stale ownership must name the official Herdr stop command: {message}"
        );
    }

    /// The retry must stay narrow. A spawn failure that is not `ETXTBSY` is a
    /// real error and must be reported at once, not after ten attempts.
    #[cfg(unix)]
    #[test]
    fn spawn_retry_only_retries_text_file_busy() {
        let started = std::time::Instant::now();
        let mut missing = Command::new("/nonexistent/workhive-missing-herdr");
        let error = spawn_retrying_etxtbsy(&mut missing).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(50),
            "a non-ETXTBSY failure must not be retried; took {:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_rejects_ownership_record_for_unrelated_sleep_pid() {
        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn decoy process");
        let state = tempfile::tempdir().expect("state dir");
        let owner = ManagedHerdrServer {
            pid: child.id(),
            socket: state.path().join("herdr.sock"),
            session: WORKHIVE_HERDR_SESSION.to_owned(),
            token: "spoofed-token".to_owned(),
        };
        write_managed_herdr_record(state.path(), &owner).expect("write decoy record");
        let error = stop_managed_workhive_herdr_server_in(state.path(), false).unwrap_err();
        let _ = child.kill();
        let _ = child.wait();
        let message = error.to_string();
        assert!(
            message.contains("does not match a live Workhive Herdr server"),
            "sleep PID must not satisfy Herdr ownership: {message}"
        );
        assert!(
            message.contains(&format!("herdr session stop {WORKHIVE_HERDR_SESSION}")),
            "stale ownership must name the official Herdr stop command: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_rejects_pid_reuse_when_argv_socket_session_match_but_token_differs() {
        let state = tempfile::tempdir().expect("state dir");
        let bin_dir = state.path().join("bin");
        write_fake_herdr(&bin_dir, "#!/bin/sh\nsleep 30\n");
        let herdr = bin_dir.join("herdr");
        let socket = state.path().join("herdr.sock");
        let mut decoy = Command::new(&herdr);
        decoy
            .args(["--session", WORKHIVE_HERDR_SESSION, "server"])
            .env("HERDR_SOCKET_PATH", &socket)
            .env(WORKHIVE_MANAGED_TOKEN_ENV, "live-or-reused-token")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = spawn_retrying_etxtbsy(&mut decoy).expect("spawn argv-matching decoy");
        let owner = ManagedHerdrServer {
            pid: child.id(),
            socket: socket.clone(),
            session: WORKHIVE_HERDR_SESSION.to_owned(),
            token: "stale-workhive-token".to_owned(),
        };
        let (tokens, env) = wait_for_matching_decoy(&mut child, &owner);
        assert!(
            !managed_token_matches(&owner, &env),
            "stored token must differ from the live process environ: tokens={tokens:?} env={env:?}"
        );
        write_managed_herdr_record(state.path(), &owner).expect("write reused-pid record");
        let error = stop_managed_workhive_herdr_server_in(state.path(), false).unwrap_err();
        let still_running = child.try_wait().expect("poll decoy").is_none();
        let _ = child.kill();
        let _ = child.wait();
        let message = error.to_string();
        assert!(
            still_running,
            "token mismatch must refuse without stopping the decoy process"
        );
        assert!(
            message.contains("does not match a live Workhive Herdr server"),
            "identical argv/socket/session must still fail when the spawn token differs: {message}"
        );
        assert!(
            message.contains(&format!("herdr session stop {WORKHIVE_HERDR_SESSION}")),
            "stale ownership must name the official Herdr stop command: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_rejects_ownership_record_without_managed_token() {
        let state = tempfile::tempdir().expect("state dir");
        std::fs::write(
            state.path().join(MANAGED_HERDR_RECORD),
            r#"{"pid":1,"socket":"/tmp/herdr.sock","session":"workhive"}"#,
        )
        .expect("write tokenless record");
        let error = stop_managed_workhive_herdr_server_in(state.path(), false).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("invalid"),
            "records without a spawn token must fail closed: {message}"
        );
        assert!(
            message.contains(&format!("herdr session stop {WORKHIVE_HERDR_SESSION}")),
            "invalid ownership must name the official Herdr stop command: {message}"
        );
    }

    #[cfg(unix)]
    fn write_stop_cli_herdr(
        bin_dir: &std::path::Path,
        snapshot_path: &std::path::Path,
        stop_marker: &std::path::Path,
    ) {
        let script = format!(
            "#!/bin/sh\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"status\" ]; then\n    printf '%s\\n' '{{\"status\":\"running\",\"socket\":\"'$HERDR_SOCKET_PATH'\",\"session\":\"{session}\"}}'\n    exit 0\n  fi\n  if [ \"$arg\" = \"snapshot\" ]; then\n    cat \"{snapshot}\"\n    exit 0\n  fi\ndone\nif [ \"$1\" = \"server\" ] && [ \"$2\" = \"stop\" ]; then\n  echo stopped > \"{stop}\"\n  exit 0\nfi\nexit 1\n",
            session = WORKHIVE_HERDR_SESSION,
            snapshot = snapshot_path.display(),
            stop = stop_marker.display(),
        );
        write_fake_herdr(bin_dir, &script);
    }

    #[cfg(unix)]
    fn spawn_owned_stop_decoy(
        state: &std::path::Path,
    ) -> (std::process::Child, ManagedHerdrServer) {
        let decoy_dir = state.join("decoy-bin");
        write_fake_herdr(&decoy_dir, "#!/bin/sh\nsleep 30\n");
        let herdr = decoy_dir.join("herdr");
        let socket = state.join("owned.sock");
        let mut decoy = Command::new(&herdr);
        decoy
            .args(["--session", WORKHIVE_HERDR_SESSION, "server"])
            .env("HERDR_SOCKET_PATH", &socket)
            .env(WORKHIVE_MANAGED_TOKEN_ENV, "owned-stop-token")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = spawn_retrying_etxtbsy(&mut decoy).expect("spawn owned decoy");
        let owner = ManagedHerdrServer {
            pid: child.id(),
            socket,
            session: WORKHIVE_HERDR_SESSION.to_owned(),
            token: "owned-stop-token".to_owned(),
        };
        let _ = wait_for_matching_decoy(&mut child, &owner);
        write_managed_herdr_record(state, &owner).expect("write ownership");
        (child, owner)
    }

    #[cfg(unix)]
    fn stop_owned_with_snapshot(
        snapshot: &str,
        force: bool,
    ) -> (Result<(), RuntimeError>, bool, bool) {
        let state = tempfile::tempdir().expect("state dir");
        let (mut child, _owner) = spawn_owned_stop_decoy(state.path());
        let snapshot_path = state.path().join("snapshot.json");
        let stop_marker = state.path().join("stopped");
        std::fs::write(&snapshot_path, snapshot).expect("write snapshot");
        let cli_bin = state.path().join("cli-bin");
        write_stop_cli_herdr(&cli_bin, &snapshot_path, &stop_marker);
        let herdr = cli_bin.join("herdr");
        let result = crate::test_env::with_exclusive_env(
            &[(
                "WORKHIVE_HERDR_BIN",
                Some(herdr.to_str().expect("utf8 herdr")),
            )],
            || stop_managed_workhive_herdr_server_in(state.path(), force),
        );
        let still_running = child.try_wait().expect("poll decoy").is_none();
        let stopped = stop_marker.exists();
        let _ = child.kill();
        let _ = child.wait();
        (result, still_running, stopped)
    }

    #[cfg(unix)]
    #[test]
    fn stop_in_use_without_force_refuses_and_leaves_server_running() {
        let snapshot = r#"{"workspaces":[{"workspace_id":"ws-1"}],"tabs":[],"panes":[{"pane_id":"p-1"}],"layouts":[],"agents":[]}"#;
        let (result, still_running, stopped) = stop_owned_with_snapshot(snapshot, false);
        let error = result.expect_err("in-use stop without --force must refuse");
        let message = error.to_string();
        assert!(
            still_running,
            "in-use stop without --force must leave the owned process running"
        );
        assert!(
            !stopped,
            "in-use stop without --force must not send server stop"
        );
        assert!(
            message.contains("in use"),
            "refusal must explain the server is in use: {message}"
        );
        assert!(
            message.contains("--force"),
            "refusal must name --force: {message}"
        );
        assert!(
            message.contains("workspace") && message.contains("pane"),
            "refusal must say what would be affected: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_in_use_with_force_stops_owned_server() {
        let snapshot = r#"{"workspaces":[{"workspace_id":"ws-1"}],"tabs":[],"panes":[{"pane_id":"p-1"}],"layouts":[],"agents":[{"agent_id":"a-1"}]}"#;
        let (result, _still_running, stopped) = stop_owned_with_snapshot(snapshot, true);
        result.expect("in-use stop with --force must execute after ownership");
        assert!(stopped, "in-use stop with --force must send server stop");
    }

    #[cfg(unix)]
    #[test]
    fn stop_idle_without_force_stops_owned_server() {
        let snapshot = r#"{"workspaces":[],"tabs":[],"panes":[],"layouts":[],"agents":[]}"#;
        let (result, _still_running, stopped) = stop_owned_with_snapshot(snapshot, false);
        result.expect("idle owned server may stop without --force");
        assert!(stopped, "idle stop without --force must send server stop");
    }

    #[cfg(unix)]
    #[test]
    fn stop_force_still_requires_ownership() {
        let state = tempfile::tempdir().expect("state dir");
        let error = stop_managed_workhive_herdr_server_in(state.path(), true).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("not managed"),
            "--force must not skip ownership: {message}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_herdr_binary_is_typed_before_start() {
        let state = tempfile::tempdir().expect("state dir");
        let missing = state.path().join("empty-path").join("herdr");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(missing.to_str().expect("utf8 herdr")),
                ),
                (
                    "HERDR_SOCKET_PATH",
                    Some(
                        state
                            .path()
                            .join("herdr.sock")
                            .to_str()
                            .expect("utf8 socket"),
                    ),
                ),
                (
                    "XDG_CONFIG_HOME",
                    Some(state.path().to_str().expect("utf8 xdg")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("200")),
            ],
            || {
                let error = ensure_workhive_herdr_server_in(state.path()).unwrap_err();
                assert!(
                    matches!(
                        error,
                        RuntimeError::BackendUnavailable {
                            reason: BackendUnavailableReason::BackendBinaryNotFound { ref binary },
                            ..
                        } if binary == "herdr"
                    ),
                    "missing herdr must be BackendBinaryNotFound: {error:?}"
                );
                let checker = SystemDependencyChecker::new();
                assert!(matches!(
                    checker.check(DependencyKind::Herdr),
                    DependencyStatus::Missing {
                        kind: DependencyKind::Herdr
                    }
                ));
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn herdr_version_too_low_is_reported_with_minimum() {
        let state = tempfile::tempdir().expect("state dir");
        let bin = state.path().join("bin");
        write_fake_herdr(
            &bin,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'herdr 0.1.0'; exit 0; fi\nexit 1\n",
        );
        let herdr = bin.join("herdr");
        crate::test_env::with_exclusive_env(
            &[(
                "WORKHIVE_HERDR_BIN",
                Some(herdr.to_str().expect("utf8 herdr")),
            )],
            || {
                let status = SystemDependencyChecker::new().check(DependencyKind::Herdr);
                match status {
                    DependencyStatus::VersionTooLow {
                        kind,
                        found,
                        minimum,
                    } => {
                        assert_eq!(kind, DependencyKind::Herdr);
                        assert_eq!(found, "0.1.0");
                        assert_eq!(minimum, HERDR_MIN_VERSION);
                    }
                    other => panic!("expected VersionTooLow, got {other:?}"),
                }
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn starts_the_configured_named_herdr_session_when_missing() {
        let state = tempfile::tempdir().expect("state dir");
        let bin = state.path().join("bin");
        let socket = state.path().join("sessions/private/herdr.sock");
        let argv_file = state.path().join("server-argv");
        write_fake_herdr(
            &bin,
            "#!/bin/sh\n\
             session=\"\"\n\
             if [ \"$1\" = \"--session\" ]; then session=\"$2\"; shift 2; fi\n\
             if [ \"$1\" = \"status\" ]; then\n\
               printf '{\\\"status\\\":\\\"not_running\\\",\\\"session\\\":\\\"%s\\\",\\\"socket\\\":\\\"%s\\\"}\\n' \"$session\" \"$WORKHIVE_TEST_SOCKET\"\n\
               exit 1\n\
             fi\n\
             if [ \"$1\" = \"server\" ]; then printf '%s %s\\n' \"$session\" \"$1\" >> \"$WORKHIVE_ARGV_FILE\"; fi\n\
             exit 1\n",
        );
        let herdr = bin.join("herdr");
        let xdg = state.path().join("config");
        std::fs::create_dir_all(&xdg).expect("create config root");
        let error = crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(herdr.to_str().expect("herdr path")),
                ),
                (
                    "WORKHIVE_TEST_SOCKET",
                    Some(socket.to_str().expect("socket path")),
                ),
                (
                    "WORKHIVE_ARGV_FILE",
                    Some(argv_file.to_str().expect("argv path")),
                ),
                ("XDG_CONFIG_HOME", Some(xdg.to_str().expect("config path"))),
                ("HOME", Some(state.path().to_str().expect("home path"))),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("200")),
                ("HERDR_SOCKET_PATH", None),
            ],
            || ensure_herdr_session_server_in(state.path(), "private").unwrap_err(),
        );
        assert!(matches!(
            error,
            RuntimeError::BackendUnavailable {
                reason: BackendUnavailableReason::ServerNotRunning { .. },
                ..
            }
        ));
        let argv = std::fs::read_to_string(&argv_file).expect("named server invocation recorded");
        assert!(
            argv.contains("private server"),
            "session was not forwarded: {argv}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn override_socket_starts_herdr_without_named_session() {
        let state = tempfile::tempdir().expect("state dir");
        let bin = state.path().join("bin");
        let argv_file = state.path().join("argv");
        write_fake_herdr(
            &bin,
            "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" > \"$WORKHIVE_ARGV_FILE\"\nexit 1\n",
        );
        let herdr = bin.join("herdr");
        let socket = state.path().join("herdr.sock");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(herdr.to_str().expect("utf8 herdr")),
                ),
                (
                    "HERDR_SOCKET_PATH",
                    Some(socket.to_str().expect("utf8 socket")),
                ),
                (
                    "WORKHIVE_ARGV_FILE",
                    Some(argv_file.to_str().expect("utf8 argv")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("200")),
            ],
            || {
                let _ = ensure_workhive_herdr_server_in(state.path());
            },
        );
        let argv = std::fs::read_to_string(&argv_file).expect("fake herdr must record argv");
        assert!(
            argv.contains("server"),
            "override start must still launch the headless server: {argv}"
        );
        assert!(
            !argv.contains("--session"),
            "HERDR_SOCKET_PATH start must not bind the live named session: {argv}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detached_spawn_puts_the_server_in_its_own_session() {
        let state = tempfile::tempdir().expect("state dir");
        let bin = state.path().join("bin");
        write_fake_herdr(
            &bin,
            "#!/bin/sh\n\
             if [ \"$(ps -o sid= -p $$)\" = \"$(ps -o sid= -p $PPID)\" ]; then\n\
               echo same > \"$WORKHIVE_SESSION_PROBE\"\n\
             else\n\
               echo detached > \"$WORKHIVE_SESSION_PROBE\"\n\
             fi\n",
        );
        let probe = state.path().join("session-probe");
        let mut command = Command::new(bin.join("herdr"));
        command.env("WORKHIVE_SESSION_PROBE", &probe);

        let mut child = spawn_detached_herdr_server(&mut command).expect("detached spawn");
        let _ = child.wait();

        let recorded = std::fs::read_to_string(&probe).expect("fake herdr must record its session");
        assert_eq!(
            recorded.trim(),
            "detached",
            "the reserved Herdr server must not stay in the caller's terminal session; \
             a terminal hangup would then kill it and every pane it owns"
        );
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "real-herdr: starts an isolated server to prove terminal-detached start"]
    fn real_herdr_started_server_is_a_detached_session_leader() {
        let root = tempfile::Builder::new()
            .prefix("workhive-detach-")
            .tempdir()
            .expect("isolated endpoint dir");
        let home = root.path().join("home");
        let xdg = home.join(".config");
        std::fs::create_dir_all(&xdg).expect("create isolated HOME");
        let socket = root.path().join("herdr.sock");
        let state = root.path().join("workhive-state");
        std::fs::create_dir_all(&state).expect("create isolated Workhive state");

        let started = crate::test_env::with_exclusive_env(
            &[
                ("HOME", Some(home.to_str().expect("utf8 HOME"))),
                ("XDG_CONFIG_HOME", Some(xdg.to_str().expect("utf8 xdg"))),
                (
                    "HERDR_SOCKET_PATH",
                    Some(socket.to_str().expect("utf8 socket")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("8000")),
            ],
            || ensure_workhive_herdr_server_in(&state),
        );
        let socket = match started {
            Ok(socket) => socket,
            Err(error) => panic!("isolated Herdr server must start: {error}"),
        };
        // The isolated server is not the live reserved session: stop it directly
        // by socket, whether or not the assertions below pass.
        struct StopIsolated(std::path::PathBuf);
        impl Drop for StopIsolated {
            fn drop(&mut self) {
                let _ = Command::new(herdr_program())
                    .env("HERDR_SOCKET_PATH", &self.0)
                    .args(["server", "stop"])
                    .status();
            }
        }
        let _cleanup = StopIsolated(socket.clone());

        let owner = read_managed_herdr_record(&state, WORKHIVE_HERDR_SESSION)
            .expect("started server must be owned");
        assert!(
            managed_process_matches(&owner),
            "detaching must not break spawn-token ownership of the started server"
        );

        // SAFETY: `getsid` only reads session ids for the given pids.
        let server_session = unsafe { libc::getsid(owner.pid as libc::pid_t) };
        // SAFETY: pid 0 asks for the caller's own session id.
        let caller_session = unsafe { libc::getsid(0) };
        assert_eq!(
            server_session, owner.pid as libc::pid_t,
            "the started server must be its own session leader"
        );
        assert_ne!(
            server_session, caller_session,
            "the started server must not share the caller's terminal session"
        );
    }

    #[cfg(unix)]
    fn argv_line_spawns_herdr_server(line: &str) -> bool {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        tokens.contains(&"server") && !tokens.contains(&"status")
    }

    #[cfg(unix)]
    fn serve_protocol_22(socket: &std::path::Path) -> std::thread::JoinHandle<()> {
        let _ = std::fs::remove_file(socket);
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).expect("create ping socket dir");
        }
        let listener =
            std::os::unix::net::UnixListener::bind(socket).expect("bind protocol 22 socket");
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    continue;
                };
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let request: Value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
                let id = request.get("id").cloned().unwrap_or(Value::from("1"));
                let reply = serde_json::json!({ "id": id, "result": { "protocol": 22 } });
                let _ = writeln!(stream, "{reply}");
            }
        })
    }

    #[cfg(unix)]
    #[test]
    fn ensure_reuses_healthy_configured_session_without_spawning_server() {
        let state = tempfile::tempdir().expect("state dir");
        let socket = state
            .path()
            .join("sessions")
            .join("workhive")
            .join("herdr.sock");
        let _listener = serve_protocol_22(&socket);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let bin = state.path().join("bin");
        let argv_file = state.path().join("argv");
        let script = format!(
            "#!/bin/sh\nprintf '%s' \"$0\" >> \"$WORKHIVE_ARGV_FILE\"\nfor arg in \"$@\"; do printf ' %s' \"$arg\" >> \"$WORKHIVE_ARGV_FILE\"; done\nprintf '\\n' >> \"$WORKHIVE_ARGV_FILE\"\nfor arg in \"$@\"; do\n  if [ \"$arg\" = \"status\" ]; then\n    printf '%s\\n' '{{\"status\":\"running\",\"socket\":\"{socket}\",\"session\":\"{session}\"}}'\n    exit 0\n  fi\ndone\nexit 1\n",
            socket = socket.display(),
            session = WORKHIVE_HERDR_SESSION,
        );
        write_fake_herdr(&bin, &script);
        let herdr = bin.join("herdr");
        let home = state.path().join("home");
        std::fs::create_dir_all(home.join(".config")).expect("isolated HOME");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(herdr.to_str().expect("utf8 herdr")),
                ),
                ("HERDR_SOCKET_PATH", None),
                (
                    "WORKHIVE_ARGV_FILE",
                    Some(argv_file.to_str().expect("utf8 argv")),
                ),
                ("HOME", Some(home.to_str().expect("utf8 HOME"))),
                (
                    "XDG_CONFIG_HOME",
                    Some(home.join(".config").to_str().expect("utf8 xdg")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("200")),
            ],
            || {
                let reused = ensure_workhive_herdr_server_in(state.path())
                    .expect("healthy configured session must be reused");
                assert_eq!(reused, socket);
            },
        );
        let argv = std::fs::read_to_string(&argv_file).expect("fake herdr must record argv");
        assert!(
            argv.contains("status"),
            "ensure must probe configured session status: {argv}"
        );
        assert!(
            !argv.lines().any(argv_line_spawns_herdr_server),
            "healthy configured session must not spawn `herdr server`: {argv}"
        );
        assert!(
            !state.path().join(MANAGED_HERDR_RECORD).exists(),
            "reusing a healthy configured session must not claim ownership"
        );
    }

    #[cfg(unix)]
    #[test]
    fn start_timeout_kills_child_and_clears_ownership() {
        let state = tempfile::tempdir().expect("state dir");
        let bin = state.path().join("bin");
        write_fake_herdr(&bin, "#!/bin/sh\nexec /bin/sleep 30\n");
        let herdr = bin.join("herdr");
        let socket = state.path().join("herdr.sock");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(herdr.to_str().expect("utf8 herdr")),
                ),
                (
                    "HERDR_SOCKET_PATH",
                    Some(socket.to_str().expect("utf8 socket")),
                ),
                (
                    "XDG_CONFIG_HOME",
                    Some(state.path().to_str().expect("utf8 xdg")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("300")),
            ],
            || {
                let error = ensure_workhive_herdr_server_in(state.path()).unwrap_err();
                assert!(
                    matches!(
                        error,
                        RuntimeError::BackendUnavailable {
                            reason: BackendUnavailableReason::ServerNotRunning { .. },
                            ..
                        }
                    ),
                    "timeout must keep the typed ServerNotRunning failure: {error:?}"
                );
                assert!(
                    !state.path().join(MANAGED_HERDR_RECORD).exists(),
                    "timeout must not leave an ownership record"
                );
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn leftover_lock_file_without_holder_does_not_block_start() {
        let state = tempfile::tempdir().expect("state dir");
        let lock_path = state.path().join(HERDR_START_LOCK);
        std::fs::write(&lock_path, "stale").expect("write leftover lock file");
        let missing = state.path().join("empty-path").join("herdr");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "WORKHIVE_HERDR_BIN",
                    Some(missing.to_str().expect("utf8 herdr")),
                ),
                (
                    "HERDR_SOCKET_PATH",
                    Some(
                        state
                            .path()
                            .join("herdr.sock")
                            .to_str()
                            .expect("utf8 socket"),
                    ),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("200")),
            ],
            || {
                let error = ensure_workhive_herdr_server_in(state.path()).unwrap_err();
                assert!(
                    matches!(
                        error,
                        RuntimeError::BackendUnavailable {
                            reason: BackendUnavailableReason::BackendBinaryNotFound { .. },
                            ..
                        }
                    ),
                    "stale lock file must be lockable so start proceeds to binary lookup: {error:?}"
                );
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn held_start_lock_makes_concurrent_ensure_wait_instead_of_spawning() {
        let state = tempfile::tempdir().expect("state dir");
        std::fs::create_dir_all(state.path()).expect("state dir");
        let lock_path = state.path().join(HERDR_START_LOCK);
        std::fs::write(&lock_path, []).expect("create lock file");
        let mut holder = spawn_exclusive_lock_holder(&lock_path);
        let socket = state.path().join("herdr.sock");
        crate::test_env::with_exclusive_env(
            &[
                (
                    "HERDR_SOCKET_PATH",
                    Some(socket.to_str().expect("utf8 socket")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("250")),
            ],
            || {
                let error = ensure_workhive_herdr_server_in(state.path()).unwrap_err();
                assert!(
                    matches!(
                        error,
                        RuntimeError::BackendUnavailable {
                            reason: BackendUnavailableReason::ServerNotRunning { .. },
                            ..
                        }
                    ),
                    "a held start lock must wait rather than spawn a second server: {error:?}"
                );
                assert!(
                    !state.path().join(MANAGED_HERDR_RECORD).exists(),
                    "waiter must not record ownership"
                );
            },
        );
        let _ = holder.kill();
        let _ = holder.wait();
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "real-herdr: isolated endpoint lifecycle; must not touch the live workhive session"]
    fn real_herdr_isolated_stopped_server_and_stale_socket() {
        let live = live_workhive_socket().expect("discover live Workhive Herdr socket");
        let live_status_before = Command::new("herdr")
            .env_remove("HERDR_SOCKET_PATH")
            .args(["--session", "workhive", "status", "server", "--json"])
            .output()
            .expect("query live Herdr status");
        let live_before = String::from_utf8_lossy(&live_status_before.stdout).into_owned();
        assert!(
            live_before.contains("running") || live_before.contains("\"status\":\"running\""),
            "live session must stay running for this test: {live_before}"
        );

        let root = tempfile::Builder::new()
            .prefix("workhive-e2e-")
            .tempdir()
            .expect("isolated endpoint dir");
        let home = root.path().join("home");
        let xdg = home.join(".config");
        std::fs::create_dir_all(&xdg).expect("create isolated HOME");
        let socket = root.path().join("herdr.sock");
        let state = root.path().join("workhive-state");
        std::fs::create_dir_all(&state).expect("create isolated Workhive state");

        struct IsolatedCleanup {
            socket: std::path::PathBuf,
            live: std::path::PathBuf,
            home: std::path::PathBuf,
        }
        impl Drop for IsolatedCleanup {
            fn drop(&mut self) {
                if self.socket == self.live {
                    return;
                }
                let _ = Command::new("herdr")
                    .env("HOME", &self.home)
                    .env("HERDR_SOCKET_PATH", &self.socket)
                    .args(["server", "stop"])
                    .status();
            }
        }
        let _cleanup = IsolatedCleanup {
            socket: socket.clone(),
            live: live.clone(),
            home: home.clone(),
        };

        crate::test_env::with_exclusive_env(
            &[
                ("HOME", Some(home.to_str().expect("utf8 HOME"))),
                ("XDG_CONFIG_HOME", Some(xdg.to_str().expect("utf8 xdg"))),
                (
                    "HERDR_SOCKET_PATH",
                    Some(socket.to_str().expect("utf8 socket")),
                ),
                ("WORKHIVE_HERDR_READY_TIMEOUT_MS", Some("8000")),
            ],
            || {
                assert_eq!(
                    resolve_herdr_socket_path().as_deref(),
                    Some(socket.as_path())
                );
                let stopped = isolated_endpoint_status(&socket);
                assert!(
                    stopped.contains("not_running")
                        || stopped.contains("\"status\":\"not_running\""),
                    "isolated endpoint must start stopped: {stopped}"
                );

                let started =
                    ensure_workhive_herdr_server_in(&state).expect("start isolated Herdr");
                assert_eq!(
                    started, socket,
                    "managed start must stay on the isolated socket"
                );
                assert_ne!(
                    started, live,
                    "isolated start must not adopt the live socket"
                );
                let running = isolated_endpoint_status(&socket);
                assert!(
                    running.contains("running") && !running.contains("not_running"),
                    "isolated endpoint must become ready: {running}"
                );
                let reported = isolated_status_socket(&running).expect("isolated status socket");
                assert_eq!(reported, socket);

                stop_managed_workhive_herdr_server_in(&state, false)
                    .expect("stop the isolated server Workhive started");
                let after_stop = isolated_endpoint_status(&socket);
                assert!(
                    after_stop.contains("not_running")
                        || after_stop.contains("\"status\":\"not_running\""),
                    "isolated endpoint must stop: {after_stop}"
                );

                if let Ok(listener) = std::os::unix::net::UnixListener::bind(&socket) {
                    drop(listener);
                    let stale = isolated_endpoint_status(&socket);
                    assert!(
                        stale.contains("not_running") || probe_herdr_socket(&socket).is_err(),
                        "bound-but-dead socket is stale: {stale}"
                    );
                    let recovered = ensure_workhive_herdr_server_in(&state);
                    match recovered {
                        Ok(path) => {
                            assert_eq!(path, socket);
                            stop_managed_workhive_herdr_server_in(&state, false)
                                .expect("stop recovered isolated server");
                        }
                        Err(error) => {
                            assert!(
                                herdr_probe_allows_start(&error)
                                    || matches!(
                                        error,
                                        RuntimeError::BackendUnavailable {
                                            reason: BackendUnavailableReason::ServerNotRunning { .. },
                                            ..
                                        }
                                    ),
                                "stale socket must not fall back to the live session: {error:?}"
                            );
                        }
                    }
                }
            },
        );

        let live_status_after = Command::new("herdr")
            .env_remove("HERDR_SOCKET_PATH")
            .args(["--session", "workhive", "status", "server", "--json"])
            .output()
            .expect("query live Herdr status after isolated test");
        let live_after = String::from_utf8_lossy(&live_status_after.stdout);
        assert!(
            live_after.contains("running") || live_after.contains("\"status\":\"running\""),
            "isolated lifecycle must leave the live session running: {live_after}"
        );
        assert_eq!(live_workhive_socket().as_deref(), Some(live.as_path()));
    }
}

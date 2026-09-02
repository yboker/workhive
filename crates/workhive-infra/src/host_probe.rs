//! Host system probes.
//!
//! Small, typed wrappers around host process/system queries that the CLI
//! layer needs for supervision and diagnostics. These live in infra so
//! that `workhive-cli` never constructs external commands for system
//! tools directly (the external-command policy); the only raw `Command::new` allowed
//! in the CLI is spawning Workhive's own executable.
//!
//! Probes are intentionally fail-open: a probe failure is reported as
//! "not alive" / "unknown", never as a hard error, because callers use
//! them for best-effort supervision and diagnostics only.

/// Return whether a process with `pid` currently exists.
///
/// Delegates the existence check to `kill -0 <pid>`, which avoids a
/// direct `libc` dependency for this single probe. Any spawn or status
/// failure is reported as "not alive" (fail-open: the caller treats an
/// unobservable parent as gone).
pub fn process_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Query host PTY usage as `(used, max)`.
///
/// On macOS this reads `kern.tty.ptmx_count` / `kern.tty.ptmx_max` via
/// `sysctl -n`. Other platforms (and any probe/parse failure) return
/// `None`, which callers render as "unknown" rather than an error.
pub fn pty_usage() -> Option<(u64, u64)> {
    #[cfg(target_os = "macos")]
    {
        let max = sysctl_u64("kern.tty.ptmx_max")?;
        let used = sysctl_u64("kern.tty.ptmx_count")?;
        Some((used, max))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Read a single numeric sysctl value via `sysctl -n <key>`.
#[cfg(target_os = "macos")]
fn sysctl_u64(key: &str) -> Option<u64> {
    let output = std::process::Command::new("sysctl")
        .args(["-n", key])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_alive_reports_own_process_as_alive() {
        let own_pid = std::process::id() as i32;
        assert!(
            process_alive(own_pid),
            "the probing process itself must always be observed as alive"
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_alive_reports_exited_child_as_gone() {
        // Spawn a short-lived child and wait for it so the PID is reaped;
        // probing the reaped PID must report "not alive".
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn `true`");
        let pid = child.id() as i32;
        child.wait().expect("child must exit");
        assert!(
            !process_alive(pid),
            "a reaped child PID must be observed as gone"
        );
    }

    #[test]
    fn pty_usage_probe_is_fail_open() {
        // Smoke check: the probe never panics; on macOS it should produce
        // a sane (used <= max) pair, elsewhere it must be None.
        match pty_usage() {
            Some((used, max)) => {
                assert!(max > 0, "ptmx_max must be positive when reported");
                assert!(used <= max, "used PTYs cannot exceed the maximum");
            }
            None => {
                #[cfg(target_os = "macos")]
                eprintln!("pty_usage returned None on macOS (acceptable in sandboxes)");
            }
        }
    }
}

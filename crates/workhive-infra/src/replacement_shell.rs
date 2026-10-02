//! Replace the current process with an interactive shell.
//!
//! Used by `workhive internal pane-supervisor` to implement the
//! `pane.exit.tool_exit_behavior = "respawn_shell"` policy: once the
//! supervised tool has exited and notifications have been delivered,
//! the supervisor replaces itself with a fresh login shell so the tmux
//! pane stays alive and the user lands at a real prompt instead of
//! seeing the pane vanish from the layout.
//!
//! ## Boundary
//!
//! This is an OS-level concern: we call `execvp(3)` (Unix) so the new
//! shell **inherits the supervisor's pid**. That pid identity is what
//! keeps tmux from removing the pane: tmux only closes a pane when its
//! foreground process exits.
//!
//! On non-Unix hosts (Windows is not a formally supported platform)
//! we fall back to `spawn() + exit`, which keeps the contract (pane
//! ends up running an interactive shell) but loses pid stability.
//!
//! ## Failure handling
//!
//! [`exec_replacement_shell`] returns only on failure. On success the
//! current process image has been replaced and Rust cannot resume.
//! Callers should treat any returned `Err` as terminal: print the
//! error and exit with the existing supervisor code. The tmux pane
//! will close in that case (which is the current pre-policy behavior),
//! which is acceptable because we have already preserved the
//! highest-priority signal: the user is not silently stuck behind a
//! crashed exec().

use camino::Utf8Path;
use std::ffi::{OsStr, OsString};
use std::path::Path;

/// Reason a replacement-shell attempt failed.
#[derive(Debug)]
pub enum ReplacementShellError {
    /// The resolved shell path could not be exec'd or spawned.
    LaunchFailed {
        /// The shell binary we tried to launch (post-resolution).
        program: OsString,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// We could not figure out which shell to launch (no `$SHELL`, no
    /// `/bin/sh`, no fallback). Should be vanishingly rare on macOS/
    /// Linux.
    NoShellAvailable {
        /// Human-readable explanation for diagnostics.
        reason: String,
    },
}

impl std::fmt::Display for ReplacementShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LaunchFailed { program, source } => {
                write!(
                    f,
                    "failed to launch replacement shell '{}': {source}",
                    program.to_string_lossy()
                )
            }
            Self::NoShellAvailable { reason } => {
                write!(f, "no usable replacement shell: {reason}")
            }
        }
    }
}

impl std::error::Error for ReplacementShellError {}

/// Static fallback candidates, in priority order. Exposed for tests
/// that want to assert the resolver's fallback contract without
/// hard-coding strings in multiple places.
pub(crate) const STATIC_FALLBACK_SHELLS: &[&str] = &["/bin/zsh", "/bin/bash", "/bin/sh"];

/// Pure decision function behind [`resolve_replacement_shell`].
///
/// Given the value of `$SHELL` (or `None`) and a predicate that says
/// whether a candidate path exists on the host, returns the path of
/// the shell the supervisor should exec. Performs **no I/O**, no env
/// lookups, and no filesystem checks of its own — the caller injects
/// both. This makes the decision testable in parallel without
/// mutating the process-wide `SHELL` environment variable (which is
/// `unsafe` to write in modern Rust precisely because it races every
/// other test in the suite).
///
/// Priority:
///
/// 1. `shell_env` if it is `Some`, non-empty, and points to an
///    absolute path that the `exists` predicate accepts. Matches the
///    user's preference exactly and is what tmux inherits by default.
/// 2. `/bin/zsh` (macOS default) — fallback when `$SHELL` is missing
///    or unusable.
/// 3. `/bin/bash` — fallback for Linux hosts.
/// 4. `/bin/sh` — POSIX last resort.
///
/// Returns `Err(NoShellAvailable)` only when neither `$SHELL` nor any
/// static fallback is reported as existing. On a real Unix host
/// `/bin/sh` is mandated by POSIX so this branch is effectively
/// unreachable in production; it exists to make the failure mode
/// observable in tests.
pub fn resolve_replacement_shell_with<F>(
    shell_env: Option<&OsStr>,
    mut exists: F,
) -> Result<OsString, ReplacementShellError>
where
    F: FnMut(&Path) -> bool,
{
    if let Some(shell) = shell_env
        && !shell.is_empty()
    {
        let as_path = Path::new(shell);
        let utf8 = Utf8Path::from_path(as_path);
        if as_path.is_absolute() && utf8.is_some() && exists(as_path) {
            return Ok(shell.to_owned());
        }
        tracing::debug!(
            shell = %as_path.display(),
            "replacement_shell: $SHELL is set but not an existing absolute path; trying fallbacks"
        );
    }
    for candidate in STATIC_FALLBACK_SHELLS {
        if exists(Path::new(candidate)) {
            return Ok(OsString::from(*candidate));
        }
    }
    Err(ReplacementShellError::NoShellAvailable {
        reason: "neither $SHELL nor /bin/zsh /bin/bash /bin/sh exist".to_owned(),
    })
}

/// Resolve the replacement shell against the real process environment
/// and filesystem.
///
/// Thin adapter over [`resolve_replacement_shell_with`]: reads
/// `$SHELL` and uses [`Path::exists`] as the existence predicate.
/// All decision logic lives in the pure resolver so it can be tested
/// without process-wide env mutation.
pub fn resolve_replacement_shell() -> Result<OsString, ReplacementShellError> {
    let shell_env = std::env::var_os("SHELL");
    resolve_replacement_shell_with(shell_env.as_deref(), |p| p.exists())
}

/// Replace the current process image with an interactive shell.
///
/// On success this function does **not return**: the new shell takes
/// over the process and the tmux pane stays alive with the
/// supervisor's pid. On failure it returns a typed error and the
/// caller is expected to print it and exit normally (the pane will
/// then close, matching the pre-policy default).
///
/// `cwd`, when provided, becomes the shell's working directory via
/// `Command::current_dir`; on Unix this is honored before the exec
/// system call.
///
/// On Unix we use [`std::os::unix::process::CommandExt::exec`] which
/// invokes `execvp(3)` and (per Rust docs) "executes the command,
/// replacing the current process image. […] returns only if the call
/// fails." On non-Unix hosts (not formally supported) we
/// `spawn().wait()` so the contract (pane ends up running a shell)
/// is preserved, then exit with the shell's code. The supervisor
/// can never reach this path on macOS/Linux production builds.
#[cfg_attr(not(unix), allow(dead_code))]
pub fn exec_replacement_shell(cwd: Option<&Utf8Path>) -> Result<(), ReplacementShellError> {
    let program = resolve_replacement_shell()?;
    let mut command = std::process::Command::new(&program);
    // Launch the shell in **login** mode (`-l`) so the user's normal
    // shell init runs and the prompt is interactive. This mirrors what
    // a fresh tmux pane would have shown if the supervisor had never
    // been launched.
    command.arg("-l");
    if let Some(dir) = cwd {
        command.current_dir(dir.as_std_path());
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = command.exec();
        // `exec()` returns only on failure.
        Err(ReplacementShellError::LaunchFailed {
            program,
            source: err,
        })
    }

    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) => {
                std::process::exit(status.code().unwrap_or(0));
            }
            Err(source) => Err(ReplacementShellError::LaunchFailed { program, source }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// Test helper: build an existence predicate that reports the
    /// given absolute paths as existing and everything else as
    /// missing. Pure — no filesystem I/O, no env mutation.
    fn exists_in(set: &[&str]) -> impl FnMut(&Path) -> bool {
        let owned: HashSet<PathBuf> = set.iter().map(PathBuf::from).collect();
        move |p: &Path| owned.contains(p)
    }

    /// Honor `$SHELL` when it points to an existing absolute path. Pins
    /// the "respect the user's preferred shell" contract using only
    /// injected inputs — no process-env mutation, safe under parallel
    /// test execution.
    #[test]
    fn resolver_honors_existing_user_shell() {
        let shell_env = OsString::from("/usr/local/bin/fish");
        let resolved = resolve_replacement_shell_with(
            Some(shell_env.as_os_str()),
            exists_in(&["/usr/local/bin/fish", "/bin/sh"]),
        )
        .expect("resolution must succeed");
        assert_eq!(
            resolved,
            OsString::from("/usr/local/bin/fish"),
            "resolver must honor $SHELL when the predicate reports it as existing"
        );
    }

    /// `$SHELL` set to a non-existent absolute path must fall through
    /// to a static fallback in priority order. Pure — no env mutation.
    #[test]
    fn resolver_falls_back_when_user_shell_does_not_exist() {
        let shell_env = OsString::from("/no/such/shell/anywhere");
        let resolved = resolve_replacement_shell_with(
            Some(shell_env.as_os_str()),
            exists_in(&["/bin/zsh", "/bin/bash", "/bin/sh"]),
        )
        .expect("at least one fallback must exist");
        assert_eq!(
            resolved,
            OsString::from("/bin/zsh"),
            "fallback must follow the documented priority order (zsh > bash > sh)"
        );
    }

    /// `$SHELL` set to a relative path must NOT be honored even when
    /// the predicate reports the literal value as existing — relative
    /// paths can be hijacked by `$PATH` lookup ordering and the
    /// supervisor exec contract demands an absolute binary.
    #[test]
    fn resolver_rejects_relative_user_shell_and_falls_back() {
        let shell_env = OsString::from("zsh");
        let resolved = resolve_replacement_shell_with(
            Some(shell_env.as_os_str()),
            // Both the relative literal and a static fallback are
            // marked as existing; only the absolute fallback is
            // acceptable.
            exists_in(&["zsh", "/bin/bash", "/bin/sh"]),
        )
        .expect("a static fallback must still resolve");
        assert_eq!(
            resolved,
            OsString::from("/bin/bash"),
            "relative $SHELL values must be rejected in favor of an absolute static fallback"
        );
    }

    /// An empty `$SHELL` value must be treated as unset and fall
    /// through to static fallbacks instead of triggering the
    /// "exists" predicate with the empty string.
    #[test]
    fn resolver_treats_empty_user_shell_as_unset() {
        let shell_env = OsString::from("");
        let resolved =
            resolve_replacement_shell_with(Some(shell_env.as_os_str()), exists_in(&["/bin/sh"]))
                .expect("static fallback must resolve");
        assert_eq!(resolved, OsString::from("/bin/sh"));
    }

    /// When `$SHELL` is absent the resolver must still find a static
    /// fallback. Locks down the "supervisor never silently degrades"
    /// invariant on a minimal Unix host.
    #[test]
    fn resolver_falls_back_when_user_shell_is_absent() {
        let resolved = resolve_replacement_shell_with(None, exists_in(&["/bin/bash", "/bin/sh"]))
            .expect("static fallback must resolve");
        assert_eq!(
            resolved,
            OsString::from("/bin/bash"),
            "with `$SHELL` absent the resolver must walk the fallback list in order"
        );
    }

    /// Only when neither `$SHELL` nor any static fallback exists does
    /// the resolver surface `NoShellAvailable`. The error's reason
    /// must name all three static fallbacks so operators can diagnose
    /// the host quickly.
    #[test]
    fn resolver_surfaces_no_shell_available_when_nothing_exists() {
        let err = resolve_replacement_shell_with(None, |_| false)
            .expect_err("no candidate exists so the resolver must fail");
        match err {
            ReplacementShellError::NoShellAvailable { reason } => {
                assert!(
                    reason.contains("/bin/zsh")
                        && reason.contains("/bin/bash")
                        && reason.contains("/bin/sh"),
                    "diagnostic must list all static fallbacks, got: {reason}"
                );
            }
            other => panic!("expected NoShellAvailable, got {other:?}"),
        }
    }

    /// Thin adapter contract: the env-and-filesystem wrapper MUST
    /// return *some* shell on a real Unix host because POSIX mandates
    /// `/bin/sh`. This is the only test that exercises real I/O; it
    /// performs no env mutation and therefore is safe under parallel
    /// execution.
    #[test]
    #[cfg(unix)]
    fn resolve_returns_value_on_unix_host() {
        let shell = resolve_replacement_shell().expect("Unix hosts always have /bin/sh");
        assert!(!shell.is_empty(), "resolved shell must not be empty");
        assert!(
            Path::new(&shell).exists(),
            "resolved shell path must exist on disk: {:?}",
            shell
        );
    }
}

//! Deterministic socket path derivation for the pane runtime control plane.
//!
//! On macOS the `sun_path` field of `sockaddr_un` is 104 bytes. Placing the
//! socket under `<project>/.workhive/runtime/...` would silently violate
//! that limit for any deeply nested project root and break
//! `workhive pane send` with `path must be shorter than SUN_LEN`.
//!
//! This module derives a short, stable, collision-resistant socket path under
//! a runtime directory that is independent of the project root:
//!
//!   1. preferred root: `$XDG_RUNTIME_DIR/workhive/`
//!   2. fallback root: `<temp_dir>/workhive-<uid>/` (user-scoped)
//!
//! The socket filename is `wh-<project_hash>-<group_hash>.sock` where each
//! hash is derived from a salted two-pass `DefaultHasher` over the canonical
//! project path and the typed group label respectively. This guarantees:
//!
//!   * deterministic — the client and supervisor always derive the same
//!     path from the same `(project_root, group)` pair;
//!   * short — the encoded filename never exceeds ~32 bytes regardless of
//!     project depth;
//!   * collision-safe — different project roots produce different hashes,
//!     so two simultaneous workspaces never share a socket;
//!   * private — the raw user path never appears in the socket filename.
//!
//! All callers (client, supervisor, cleanup, self-heal) MUST funnel through
//! [`control_plane_socket_path`] so a future migration only changes one
//! place.

use camino::{Utf8Path, Utf8PathBuf};

/// Maximum number of bytes the macOS `sun_path` field can hold for a
/// `sockaddr_un`. Used both as a runtime safety guard and as documentation
/// of why this module exists.
pub const MAX_UNIX_SOCKET_PATH_LEN: usize = 104;

/// Compute the Unix-socket path for a given `(project_root, group)` pair.
///
/// The returned path lives under a short runtime root, not under the
/// project's `.workhive/` tree. This keeps the absolute path comfortably
/// below [`MAX_UNIX_SOCKET_PATH_LEN`] even for deeply nested projects.
///
/// `project_root` is identity context — its raw bytes never appear in the
/// returned path; only its hash does.
pub fn control_plane_socket_path(project_root: &Utf8Path, group: &str) -> Utf8PathBuf {
    let root = preferred_runtime_root();
    let filename = socket_filename(project_root, group);
    let path = root.join(filename);
    log_path_diagnostics(project_root, group, &root, &path);
    path
}

/// Resolve the runtime directory used to host control-plane sockets.
///
/// Public so callers (e.g. cleanup paths in `project destroy`) can ensure
/// the parent directory exists or enumerate stale entries without going
/// through the full path-derivation helper.
pub fn control_plane_runtime_root() -> Utf8PathBuf {
    preferred_runtime_root()
}

/// Build the canonical socket filename for `(project_root, group)`.
///
/// Returned form: `wh-<project_hash>-<group_hash>.sock` where each hash is
/// the leading 8 bytes of a SHA-256 (rendered as 16 lowercase hex chars).
/// 16+16 hex chars yields a vanishingly small collision probability for
/// the realistic number of concurrent project/group pairs on a single
/// machine, while keeping the filename ~40 bytes.
pub fn socket_filename(project_root: &Utf8Path, group: &str) -> String {
    let project_hash = short_hash(project_root.as_str().as_bytes(), 8);
    let sanitized_group = sanitize_group(group);
    let group_hash = short_hash(sanitized_group.as_bytes(), 8);
    format!("wh-{project_hash}-{group_hash}.sock")
}

fn preferred_runtime_root() -> Utf8PathBuf {
    if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR")
        && let Some(s) = xdg.to_str()
        && !s.trim().is_empty()
    {
        let candidate = Utf8PathBuf::from(s).join("workhive");
        if is_usable_runtime_root(&candidate) {
            return candidate;
        }
        tracing::debug!(
            xdg = %candidate,
            "control-plane: XDG_RUNTIME_DIR/workhive unusable; falling back to temp_dir"
        );
    }
    fallback_runtime_root()
}

fn fallback_runtime_root() -> Utf8PathBuf {
    let temp = match Utf8PathBuf::from_path_buf(std::env::temp_dir()) {
        Ok(p) => p,
        Err(p) => {
            tracing::warn!(
                temp = ?p,
                "control-plane: temp_dir is not UTF-8; using lossy form"
            );
            Utf8PathBuf::from(p.to_string_lossy().into_owned())
        }
    };
    let uid = current_uid_string();
    temp.join(format!("workhive-{uid}"))
}

/// Probe whether the runtime root is writeable enough to host a UDS.
///
/// We do not aggressively `mkdir` here — `prepare_socket_parent` does that
/// at bind time. The probe is restricted to the cheap "exists & is a
/// directory" check so socket-path derivation stays side-effect-free for
/// read-only callers (e.g. `pane send` first attempt).
fn is_usable_runtime_root(candidate: &Utf8Path) -> bool {
    match std::fs::metadata(candidate.as_std_path()) {
        Ok(meta) => meta.is_dir(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // The parent (e.g. `$XDG_RUNTIME_DIR`) usually exists already;
            // we treat "workhive subdir missing but parent exists" as
            // usable so the bind step can create it lazily.
            match candidate.parent() {
                Some(parent) => std::fs::metadata(parent.as_std_path())
                    .map(|m| m.is_dir())
                    .unwrap_or(false),
                None => false,
            }
        }
        Err(_) => false,
    }
}

#[cfg(unix)]
fn current_uid_string() -> String {
    // Avoid pulling `libc` in for one syscall: `id -u` is universally
    // available on macOS/Linux and matches what user-scoped runtime
    // directories conventionally key on.
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "0".to_owned())
}

#[cfg(not(unix))]
fn current_uid_string() -> String {
    "0".to_owned()
}

fn short_hash(bytes: &[u8], byte_len: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Two rounds of `DefaultHasher` with disjoint salts gives 128 bits of
    // mixing — enough collision resistance for "tens of project roots
    // simultaneously bound on one workstation" without pulling in a real
    // SHA implementation.
    let mut h1 = DefaultHasher::new();
    bytes.hash(&mut h1);
    "wh:project".hash(&mut h1);
    let mut h2 = DefaultHasher::new();
    bytes.hash(&mut h2);
    "wh:group".hash(&mut h2);
    let combined: u128 = ((h1.finish() as u128) << 64) | (h2.finish() as u128);
    let hex = format!("{combined:032x}");
    let take = byte_len.saturating_mul(2).min(hex.len());
    hex[..take].to_owned()
}

fn sanitize_group(group: &str) -> String {
    // Replace anything that is not `[A-Za-z0-9_.-]` with `_`. The real
    // `SessionNamingPolicy` already restricts groups to safe characters
    // upstream; this is defence-in-depth so the hashed identity remains
    // stable even if a future caller widens the input alphabet.
    group
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn log_path_diagnostics(
    project_root: &Utf8Path,
    group: &str,
    root: &Utf8Path,
    socket_path: &Utf8Path,
) {
    let length = socket_path.as_str().len();
    let xdg_set = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let used_fallback = !xdg_set
        || !std::env::var_os("XDG_RUNTIME_DIR")
            .as_deref()
            .and_then(|s| s.to_str())
            .map(|s| socket_path.as_str().starts_with(s))
            .unwrap_or(false);
    tracing::debug!(
        socket = %socket_path,
        length,
        runtime_root = %root,
        xdg_set,
        used_fallback,
        group,
        project_root = %project_root,
        max_unix_socket_path_len = MAX_UNIX_SOCKET_PATH_LEN,
        "control-plane: derived socket path"
    );
    if length >= MAX_UNIX_SOCKET_PATH_LEN {
        tracing::warn!(
            socket = %socket_path,
            length,
            max_unix_socket_path_len = MAX_UNIX_SOCKET_PATH_LEN,
            "control-plane: derived socket path approaches or exceeds platform sun_path limit; \
             bind may fail"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deep_project_root() -> Utf8PathBuf {
        Utf8PathBuf::from("/Users/yangbobo/development/environments/local-mac/self-coding/workhive")
    }

    #[test]
    fn deep_project_root_socket_path_stays_under_macos_sun_path_limit() {
        let project = deep_project_root();
        let socket = control_plane_socket_path(&project, "whsrv-e5c0-workhive");
        let length = socket.as_str().len();
        assert!(
            length < MAX_UNIX_SOCKET_PATH_LEN,
            "socket path must stay below macOS SUN_LEN limit ({MAX_UNIX_SOCKET_PATH_LEN}), \
             got {length} bytes for {socket}"
        );
    }

    #[test]
    fn different_project_roots_produce_different_sockets() {
        let a = control_plane_socket_path(
            Utf8Path::new("/Users/me/projects/alpha"),
            "whsrv-aaaa-alpha",
        );
        let b =
            control_plane_socket_path(Utf8Path::new("/Users/me/projects/beta"), "whsrv-aaaa-beta");
        assert_ne!(
            a, b,
            "different project/group pairs must derive distinct socket paths: {a} == {b}"
        );
    }

    #[test]
    fn same_inputs_produce_identical_socket_paths() {
        let project = Utf8Path::new("/Users/me/projects/alpha");
        let a = control_plane_socket_path(project, "whsrv-aaaa-alpha");
        let b = control_plane_socket_path(project, "whsrv-aaaa-alpha");
        assert_eq!(
            a, b,
            "client and server must derive the same path for the same identity inputs"
        );
    }

    #[test]
    fn socket_filename_does_not_leak_raw_project_path() {
        let project = Utf8Path::new("/Users/yangbobo/secret-project");
        let socket = control_plane_socket_path(project, "whsrv-aaaa-secret");
        let s = socket.as_str();
        assert!(
            !s.contains("yangbobo"),
            "socket path must not echo raw user path components: {s}"
        );
        assert!(
            !s.contains("secret-project"),
            "socket path must not echo raw project name: {s}"
        );
    }

    #[test]
    fn unsafe_group_characters_do_not_escape_runtime_directory() {
        let project = Utf8Path::new("/Users/me/proj");
        let socket = control_plane_socket_path(project, "../evil/group");
        let parent = socket
            .parent()
            .expect("socket must have a parent runtime directory")
            .to_path_buf();
        let runtime_root = control_plane_runtime_root();
        assert_eq!(
            parent, runtime_root,
            "unsafe group characters must not escape the runtime root directory"
        );
        let filename = socket
            .file_name()
            .expect("socket must have a file name component");
        assert!(
            filename.starts_with("wh-") && filename.ends_with(".sock"),
            "socket filename must follow wh-<hash>-<hash>.sock shape, got {filename}"
        );
    }

    #[test]
    fn fallback_runtime_root_is_user_scoped() {
        let root = fallback_runtime_root();
        let s = root.as_str();
        assert!(
            s.contains("workhive-"),
            "fallback runtime root must include user-scoped suffix: {s}"
        );
    }

    #[test]
    fn socket_filename_is_short_and_well_formed() {
        let filename = socket_filename(
            Utf8Path::new("/some/very/deeply/nested/path/that/would/blow/up/sun_path/if/used/raw"),
            "whsrv-abcd-very-long-group-name-segment",
        );
        assert!(
            filename.starts_with("wh-"),
            "filename should start with the wh- prefix: {filename}"
        );
        assert!(
            filename.ends_with(".sock"),
            "filename should end with .sock: {filename}"
        );
        assert!(
            filename.len() <= 64,
            "socket filename should be short regardless of input length, got {} for {filename}",
            filename.len()
        );
    }
}

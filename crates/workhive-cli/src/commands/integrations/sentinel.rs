//! File-level sentinel + atomic write + backup helpers shared by the
//! per-provider cleanup paths and the Codex notify wrapper install
//! path.
//!
//! Two distinct kinds of artifact are touched by the preparation
//! lifecycle:
//!
//! 1. **Workhive-owned legacy scripts/plugins** under
//!    `<global_config>/workhive/integrations/<provider>/` and at the
//!    OpenCode plugin slot. These were emitted by previous releases
//!    and carry [`WORKHIVE_SENTINEL`] in their first line so cleanup
//!    can delete them safely while leaving any user-authored sibling
//!    file at the same path untouched.
//! 2. **Edits to the user's official tool config files** (e.g.
//!    `~/.claude/settings.json`, `~/.codex/config.toml`). These are
//!    *not* Workhive-owned files — we are guests inside them. The
//!    sentinel is therefore embedded *per entry* by the provider
//!    modules (a marker field inside the JSON object / TOML table).
//!    Before any such edit we copy the original user file to
//!    `<path>.workhive-backup-<unix_ms>` via [`backup_file`]. The
//!    backup is only created when an actual rewrite is about to
//!    occur — a no-op cleanup pass leaves the user's config bit-for-bit
//!    untouched and creates no backup file.
//!
//! All filesystem operations route through [`IntegrationError`] (see
//! `error.rs`) so the lifecycle caller can classify failures and
//! decide between fail-closed and fail-open behaviour.

use std::fs;
use std::io::Write as _;

use camino::{Utf8Path, Utf8PathBuf};

use super::error::{IntegrationError, io_err};

/// Sentinel emitted as the first marker inside every Workhive-authored
/// file (hook scripts / plugin files). `remove` refuses to delete
/// files missing this sentinel so user-authored files in the same
/// directory can never be deleted by Workhive.
pub(super) const WORKHIVE_SENTINEL: &str = "WORKHIVE-GENERATED";

/// Per-entry sentinel field embedded inside structured config edits
/// (JSON object key / TOML table key). `status` and `remove` use it
/// to identify which entries inside an otherwise user-owned config
/// file Workhive is allowed to inspect/delete.
pub(super) const WORKHIVE_MANAGED_KEY: &str = "_workhive_managed";

/// Read the first 4 KiB of `path` and check whether it contains
/// [`WORKHIVE_SENTINEL`]. Files that do not exist return `false`.
pub(super) fn file_is_workhive_authored(path: &Utf8Path) -> Result<bool, IntegrationError> {
    if !path.exists() {
        return Ok(false);
    }
    let head = fs::read_to_string(path).map_err(|e| io_err("read", path.to_path_buf(), e))?;
    Ok(head.contains(WORKHIVE_SENTINEL))
}

/// Atomically write `body` to `path` via a sibling temp file +
/// fsync + rename. The temp filename appends a dotted suffix to the
/// final basename so files with multiple extensions
/// (e.g. `opencode.sample.json`) get a stable temp name.
pub(super) fn atomic_write(path: &Utf8Path, body: &str) -> Result<(), IntegrationError> {
    let parent = path
        .parent()
        .ok_or_else(|| io_err("no parent", path.to_path_buf(), no_parent_error()))?;
    fs::create_dir_all(parent).map_err(|e| io_err("create_dir_all", parent.to_path_buf(), e))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io_err("no filename", path.to_path_buf(), no_parent_error()))?;
    let tmp = parent.join(format!(".{file_name}.workhive.tmp"));
    let mut f = fs::File::create(&tmp).map_err(|e| io_err("open_tmp", tmp.clone(), e))?;
    f.write_all(body.as_bytes())
        .map_err(|e| io_err("write_tmp", tmp.clone(), e))?;
    f.sync_all()
        .map_err(|e| io_err("sync_tmp", tmp.clone(), e))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| io_err("rename_tmp", tmp.clone(), e))?;
    Ok(())
}

/// Copy `path` to `<path>.workhive-backup-<unix_ms>` before we
/// mutate the user's official config file. Returns the backup path
/// on success. Skipped silently if `path` does not yet exist (the
/// fresh-install case).
///
/// **Idempotency contract**: this helper is the only path that ever
/// creates a backup file. Callers must call it *only* when they have
/// already determined that a mutation will occur — never as a
/// "defensive" precaution before deciding whether to write. A no-op
/// cleanup pass must therefore leave no `*.workhive-backup-*` file on
/// disk so repeated prepares are observably idempotent from the
/// user's filesystem view.
pub(super) fn backup_file(path: &Utf8Path) -> Result<Option<Utf8PathBuf>, IntegrationError> {
    if !path.exists() {
        return Ok(None);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let parent = path
        .parent()
        .ok_or_else(|| io_err("no parent", path.to_path_buf(), no_parent_error()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| io_err("no filename", path.to_path_buf(), no_parent_error()))?;
    let backup = parent.join(format!("{file_name}.workhive-backup-{stamp}"));
    fs::copy(path, &backup).map_err(|e| io_err("backup_copy", backup.clone(), e))?;
    Ok(Some(backup))
}

/// Construct a synthetic `io::Error` to wrap structural path
/// failures (e.g. `path.parent()` returning `None`). Keeps every
/// error inside [`IntegrationError::Io`] so callers do not need a
/// separate "shape error" variant.
fn no_parent_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "path lacks parent or filename component",
    )
}

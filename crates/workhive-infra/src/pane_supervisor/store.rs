//! On-disk metadata store for the Workhive pane supervisor.
//!
//! Metadata layout:
//!
//! ```text
//! <runtime_root>/<group>/<workspace>/pane-supervisor/<unit_id>.json
//! ```
//!
//! `<runtime_root>` is project-scoped (e.g.
//! `<project>/.workhive/runtime`). The store does not invent paths;
//! callers pass in the resolved root directory.
//!
//! ## File contents
//!
//! Each JSON file describes one supervised child and is rewritten
//! atomically (`create_tmp + rename`) by the supervisor binary. The
//! `state` field is the truthful state from the supervisor's
//! perspective: `Running` while the wait loop is active, `Exited`
//! once the child exited, optionally with `exit_code`.
//!
//! ## Truthful states
//!
//! * The store NEVER writes a state it cannot prove. If the
//!   supervisor crashes mid-wait, the file is left untouched and the
//!   adapter's pid liveness probe drives the merge.

use std::fs;
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use workhive_core::domain::UnitId;

/// State the supervisor records on disk for a child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneSupervisorStateOnDisk {
    /// The supervisor still believes the child is running. Adapter
    /// must verify pid liveness before trusting this.
    Running,
    /// The supervisor saw the child exit. `exit_code` may be `None`
    /// when the OS did not report one (e.g., killed by signal).
    Exited,
}

/// Metadata describing one supervised child process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneSupervisorMetadata {
    /// pid of the supervised child.
    pub pid: u32,
    /// argv\[0\] basename of the child (e.g. `claude`, `codex`).
    pub command_basename: String,
    /// Full program path when known; supervisor records what it
    /// resolved at fork time.
    pub program_path: Option<String>,
    /// RFC 3339 timestamp the supervisor wrote when the child
    /// started.
    pub started_at_rfc3339: String,
    /// Last time the supervisor refreshed this file (heartbeat).
    pub last_heartbeat_rfc3339: Option<String>,
    /// Current state — `Running` until the wait loop sees the child
    /// exit.
    pub state: PaneSupervisorStateOnDisk,
    /// Set only when `state == Exited`.
    pub exit_code: Option<i32>,
}

/// One supervisor metadata record paired with its on-disk path.
///
/// Useful for the supervisor binary which mutates files in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSupervisorRecord {
    pub path: Utf8PathBuf,
    pub metadata: PaneSupervisorMetadata,
}

/// Errors produced by the metadata store.
#[derive(Debug, thiserror::Error)]
pub enum PaneSupervisorStoreError {
    #[error("io failure on supervisor metadata path '{path}': {detail}")]
    Io { path: Utf8PathBuf, detail: String },

    #[error("supervisor metadata at '{path}' is corrupt: {detail}")]
    Corrupt { path: Utf8PathBuf, detail: String },

    #[error("invalid utf-8 path: {detail}")]
    InvalidPath { detail: String },
}

/// Compute the metadata file path for a single unit.
pub fn metadata_path(
    runtime_root: &Utf8Path,
    group: &str,
    workspace: &str,
    unit_id: &UnitId,
) -> Result<Utf8PathBuf, PaneSupervisorStoreError> {
    if group.is_empty() || workspace.is_empty() {
        return Err(PaneSupervisorStoreError::InvalidPath {
            detail: "group and workspace must both be non-empty".to_owned(),
        });
    }
    let group_segment = sanitize_group_segment(group)?;
    let workspace_segment = sanitize_group_segment(workspace)?;
    let unit_segment = sanitize_unit_segment(unit_id.as_str())?;
    let mut path = runtime_root.to_owned();
    path.push(group_segment);
    path.push(workspace_segment);
    path.push("pane-supervisor");
    path.push(format!("{unit_segment}.json"));
    Ok(path)
}

/// Read metadata for a single unit. Returns `Ok(None)` when no file
/// exists, so the caller (typically the supervisor adapter) can
/// surface "no supervisor observation" without conflating "missing"
/// with "I/O error".
pub fn read_metadata(
    runtime_root: &Utf8Path,
    group: &str,
    workspace: &str,
    unit_id: &UnitId,
) -> Result<Option<PaneSupervisorMetadata>, PaneSupervisorStoreError> {
    let path = metadata_path(runtime_root, group, workspace, unit_id)?;
    read_metadata_at(&path)
}

/// Read metadata from an explicit path. Useful for tests and the
/// supervisor binary which already knows the path.
pub fn read_metadata_at(
    path: &Utf8Path,
) -> Result<Option<PaneSupervisorMetadata>, PaneSupervisorStoreError> {
    let bytes = match fs::read(path.as_std_path()) {
        Ok(b) => b,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(PaneSupervisorStoreError::Io {
                path: path.to_owned(),
                detail: err.to_string(),
            });
        }
    };
    let parsed: PaneSupervisorMetadata =
        serde_json::from_slice(&bytes).map_err(|err| PaneSupervisorStoreError::Corrupt {
            path: path.to_owned(),
            detail: err.to_string(),
        })?;
    Ok(Some(parsed))
}

/// Write metadata atomically. Creates parent directories as needed.
pub fn write_metadata(
    runtime_root: &Utf8Path,
    group: &str,
    workspace: &str,
    unit_id: &UnitId,
    metadata: &PaneSupervisorMetadata,
) -> Result<Utf8PathBuf, PaneSupervisorStoreError> {
    let path = metadata_path(runtime_root, group, workspace, unit_id)?;
    write_metadata_at(&path, metadata)?;
    Ok(path)
}

/// Write metadata to an explicit path atomically.
pub fn write_metadata_at(
    path: &Utf8Path,
    metadata: &PaneSupervisorMetadata,
) -> Result<(), PaneSupervisorStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent.as_std_path()).map_err(|err| PaneSupervisorStoreError::Io {
            path: parent.to_owned(),
            detail: err.to_string(),
        })?;
    }
    let json =
        serde_json::to_vec_pretty(metadata).map_err(|err| PaneSupervisorStoreError::Corrupt {
            path: path.to_owned(),
            detail: err.to_string(),
        })?;
    let tmp_path: PathBuf = {
        let mut p = path.as_std_path().to_owned();
        let mut name = p.file_name().map(|s| s.to_owned()).unwrap_or_default();
        name.push(".tmp");
        p.set_file_name(name);
        p
    };
    fs::write(&tmp_path, &json).map_err(|err| PaneSupervisorStoreError::Io {
        path: Utf8PathBuf::from_path_buf(tmp_path.clone())
            .unwrap_or_else(|_| Utf8PathBuf::from("<non-utf8 tmp>")),
        detail: err.to_string(),
    })?;
    fs::rename(&tmp_path, path.as_std_path()).map_err(|err| {
        // Best-effort cleanup of the tmp file; ignore secondary errors.
        let _ = fs::remove_file(&tmp_path);
        PaneSupervisorStoreError::Io {
            path: path.to_owned(),
            detail: err.to_string(),
        }
    })?;
    Ok(())
}

fn sanitize_group_segment(raw: &str) -> Result<String, PaneSupervisorStoreError> {
    if raw.is_empty() {
        return Err(PaneSupervisorStoreError::InvalidPath {
            detail: "path segment must not be empty".to_owned(),
        });
    }
    Ok(raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect())
}

/// Convert a `UnitId` (e.g. `%2`) into a filename-safe segment
/// (`unit-2`).
fn sanitize_unit_segment(raw: &str) -> Result<String, PaneSupervisorStoreError> {
    if raw.is_empty() {
        return Err(PaneSupervisorStoreError::InvalidPath {
            detail: "unit_id must not be empty".to_owned(),
        });
    }
    let mut out = String::with_capacity(raw.len() + 5);
    out.push_str("unit-");
    for ch in raw.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => out.push(ch),
            _ => out.push('_'),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use tempfile::TempDir;

    fn fixture(state: PaneSupervisorStateOnDisk) -> PaneSupervisorMetadata {
        PaneSupervisorMetadata {
            pid: 4242,
            command_basename: "claude".to_owned(),
            program_path: Some("/usr/local/bin/claude".to_owned()),
            started_at_rfc3339: "2026-05-07T12:00:00Z".to_owned(),
            last_heartbeat_rfc3339: Some("2026-05-07T12:00:30Z".to_owned()),
            state,
            exit_code: None,
        }
    }

    fn tmp_root() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        (tmp, root)
    }

    #[test]
    fn metadata_path_includes_group_workspace_and_unit_segment() {
        let (_t, root) = tmp_root();
        let path = metadata_path(&root, "g", "ws", &UnitId::new("%2")).unwrap();
        assert!(
            path.to_string()
                .ends_with("g/ws/pane-supervisor/unit-_2.json")
        );
    }

    #[test]
    fn metadata_path_sanitizes_absolute_runtime_group() {
        let (_t, root) = tmp_root();
        let path = metadata_path(
            &root,
            "/home/user/.config/herdr/herdr.sock",
            "w9",
            &UnitId::new("w9:p2"),
        )
        .unwrap();
        assert!(path.starts_with(root.as_path()));
        assert!(
            path.ends_with(
                "_home_user_.config_herdr_herdr.sock/w9/pane-supervisor/unit-w9_p2.json"
            )
        );
    }

    #[test]
    fn metadata_path_rejects_blank_components() {
        let (_t, root) = tmp_root();
        let err = metadata_path(&root, "", "ws", &UnitId::new("%2")).unwrap_err();
        assert!(matches!(err, PaneSupervisorStoreError::InvalidPath { .. }));
    }

    #[test]
    fn read_metadata_returns_none_for_missing_file() {
        let (_t, root) = tmp_root();
        let result = read_metadata(&root, "g", "ws", &UnitId::new("%2")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_then_read_round_trips_running_state() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%2");
        let meta = fixture(PaneSupervisorStateOnDisk::Running);
        let written = write_metadata(&root, "g", "ws", &unit, &meta).unwrap();
        assert!(written.exists());
        let read = read_metadata(&root, "g", "ws", &unit).unwrap().unwrap();
        assert_eq!(read, meta);
    }

    #[test]
    fn write_then_read_round_trips_exited_state_with_exit_code() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%3");
        let mut meta = fixture(PaneSupervisorStateOnDisk::Exited);
        meta.exit_code = Some(0);
        write_metadata(&root, "g", "ws", &unit, &meta).unwrap();
        let read = read_metadata(&root, "g", "ws", &unit).unwrap().unwrap();
        assert_eq!(read.state, PaneSupervisorStateOnDisk::Exited);
        assert_eq!(read.exit_code, Some(0));
    }

    #[test]
    fn corrupt_file_surfaces_typed_error() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%4");
        let path = metadata_path(&root, "g", "ws", &unit).unwrap();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path()).unwrap();
        }
        fs::write(path.as_std_path(), b"this is not json").unwrap();
        let err = read_metadata(&root, "g", "ws", &unit).unwrap_err();
        assert!(matches!(err, PaneSupervisorStoreError::Corrupt { .. }));
    }
}

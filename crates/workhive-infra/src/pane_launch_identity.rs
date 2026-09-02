//! JSON-backed durable [`PaneLaunchIdentityPort`] adapter.
//!
//! The on-disk layout mirrors the control-plane socket:
//!
//! ```text
//! <project_root>/
//!   .workhive/
//!     runtime/
//!       control-plane/
//!         <group>.sock
//!       pane-identity/
//!         <group>.json
//! ```
//!
//! One file per group keeps the adapter dead-simple: every record/list
//! call locks the same file, and the file is small enough that we can
//! always rewrite it whole on update. Atomic rename gives us
//! torn-write safety; the project-level init lock guarantees no two
//! Workhive CLI processes touch the same file concurrently.
//!
//! ## Why a durable bridge
//!
//! The control-plane display title must not rely on a
//! process-local registry. This adapter is the durable bridge the
//! reviewer demanded between `workspace open` and the detached
//! supervisor.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use workhive_core::domain::{
    PaneDescriptor, PaneKind, PaneRuntimeId, PaneRuntimeStatus, PaneState,
};
use workhive_core::ports::{
    PaneLaunchIdentityEntry, PaneLaunchIdentityError, PaneLaunchIdentityPort, PaneRegistryPort,
    RegistryError,
};

/// Schema version for the on-disk JSON file. Bump on incompatible
/// changes; keep parser tolerant of older versions where reasonable.
const SCHEMA_VERSION: u32 = 1;

/// Root directory name (relative to `<project_root>/.workhive/runtime/`)
/// where per-group identity files live. Kept in step with
/// `control_plane::socket::control_plane_socket_path` for cleanup
/// hygiene.
const PANE_IDENTITY_DIR: &str = "pane-identity";

/// JSON envelope persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreFile {
    /// Schema version. Required on read; emitted on write.
    schema_version: u32,
    /// Group label this file belongs to. Stored for forensic
    /// debugging — the canonical key is the file name on disk.
    group: String,
    /// Recorded entries. Order is not significant; readers should
    /// treat `(workspace, unit_id)` as the unique key.
    entries: Vec<PaneLaunchIdentityEntry>,
}

/// File-system backed implementation of [`PaneLaunchIdentityPort`].
///
/// Constructed once per project; `record` / `list_for_group` /
/// `forget_*` are stateless and safe to call from any thread.
#[derive(Debug, Clone)]
pub struct JsonPaneLaunchIdentityStore {
    project_root: Utf8PathBuf,
}

impl JsonPaneLaunchIdentityStore {
    /// Build a store rooted at `<project_root>/.workhive/runtime/pane-identity/`.
    ///
    /// The directory is created lazily on first write so a freshly
    /// initialized project doesn't pay for a directory we don't need
    /// until at least one pane has been launched.
    pub fn new(project_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
        }
    }

    /// Project root the store is anchored to. Exposed for diagnostics
    /// and so the supervisor can pass it back into other adapters.
    pub fn project_root(&self) -> &Utf8Path {
        self.project_root.as_path()
    }

    fn group_dir(&self) -> Utf8PathBuf {
        self.project_root
            .join(".workhive")
            .join("runtime")
            .join(PANE_IDENTITY_DIR)
    }

    fn group_file(&self, group: &str) -> Result<Utf8PathBuf, PaneLaunchIdentityError> {
        let sanitized = sanitize_group(group)?;
        Ok(self.group_dir().join(format!("{sanitized}.json")))
    }

    fn read_file(&self, group: &str) -> Result<StoreFile, PaneLaunchIdentityError> {
        let path = self.group_file(group)?;
        if !path.as_std_path().exists() {
            return Ok(StoreFile {
                schema_version: SCHEMA_VERSION,
                group: group.to_owned(),
                entries: Vec::new(),
            });
        }
        let bytes = fs::read(path.as_std_path()).map_err(|source| PaneLaunchIdentityError::Io {
            path: path.clone(),
            source,
        })?;
        let parsed: StoreFile = serde_json::from_slice(&bytes).map_err(|err| {
            PaneLaunchIdentityError::Serialization {
                path: path.clone(),
                detail: err.to_string(),
            }
        })?;
        Ok(parsed)
    }

    fn write_file(&self, group: &str, file: &StoreFile) -> Result<(), PaneLaunchIdentityError> {
        let dir = self.group_dir();
        fs::create_dir_all(dir.as_std_path()).map_err(|source| PaneLaunchIdentityError::Io {
            path: dir.clone(),
            source,
        })?;

        let target = self.group_file(group)?;
        let tmp = target.with_extension("json.tmp");
        let serialized = serde_json::to_vec_pretty(file).map_err(|err| {
            PaneLaunchIdentityError::Serialization {
                path: target.clone(),
                detail: err.to_string(),
            }
        })?;

        // Atomic write: write into tmp, fsync, rename. We want the
        // file's contents to never appear partially written to a
        // concurrent supervisor reader.
        let mut handle = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(tmp.as_std_path())
            .map_err(|source| PaneLaunchIdentityError::Io {
                path: tmp.clone(),
                source,
            })?;
        handle
            .write_all(&serialized)
            .map_err(|source| PaneLaunchIdentityError::Io {
                path: tmp.clone(),
                source,
            })?;
        handle
            .sync_all()
            .map_err(|source| PaneLaunchIdentityError::Io {
                path: tmp.clone(),
                source,
            })?;
        drop(handle);

        fs::rename(tmp.as_std_path(), target.as_std_path()).map_err(|source| {
            PaneLaunchIdentityError::Io {
                path: target.clone(),
                source,
            }
        })?;
        Ok(())
    }

    fn delete_file(&self, group: &str) -> Result<(), PaneLaunchIdentityError> {
        let path = self.group_file(group)?;
        match fs::remove_file(path.as_std_path()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(PaneLaunchIdentityError::Io { path, source }),
        }
    }
}

impl PaneLaunchIdentityPort for JsonPaneLaunchIdentityStore {
    fn record(
        &self,
        group: &str,
        entry: PaneLaunchIdentityEntry,
    ) -> Result<(), PaneLaunchIdentityError> {
        let mut file = self.read_file(group)?;
        // Upsert by (workspace, unit_id). We deduplicate via HashMap to
        // collapse historic duplicates if the file was hand-edited.
        let mut by_key: HashMap<(String, String), PaneLaunchIdentityEntry> = file
            .entries
            .drain(..)
            .map(|e| ((e.workspace.clone(), e.unit_id.as_str().to_owned()), e))
            .collect();
        by_key.insert(
            (entry.workspace.clone(), entry.unit_id.as_str().to_owned()),
            entry,
        );
        let mut next: Vec<PaneLaunchIdentityEntry> = by_key.into_values().collect();
        next.sort_by(|a, b| {
            a.workspace
                .cmp(&b.workspace)
                .then_with(|| a.unit_id.as_str().cmp(b.unit_id.as_str()))
        });
        file.entries = next;
        file.schema_version = SCHEMA_VERSION;
        file.group = group.to_owned();
        self.write_file(group, &file)
    }

    fn list_for_group(
        &self,
        group: &str,
    ) -> Result<Vec<PaneLaunchIdentityEntry>, PaneLaunchIdentityError> {
        let file = self.read_file(group)?;
        Ok(file.entries)
    }

    fn forget_workspace(
        &self,
        group: &str,
        workspace: &str,
    ) -> Result<(), PaneLaunchIdentityError> {
        let mut file = self.read_file(group)?;
        file.entries.retain(|entry| entry.workspace != workspace);
        if file.entries.is_empty() {
            self.delete_file(group)
        } else {
            file.schema_version = SCHEMA_VERSION;
            file.group = group.to_owned();
            self.write_file(group, &file)
        }
    }

    fn forget_group(&self, group: &str) -> Result<(), PaneLaunchIdentityError> {
        self.delete_file(group)
    }
}

/// Outcome of a single hydration call.
///
/// We surface counts (rather than only `Ok(())`) so the supervisor's
/// startup log can report how many launch identities were re-attached
/// across the process boundary. That is a directly observable contract
/// ("foreground command is
/// `shell`, but `display_title` is `codex`/`claude`").
///
/// Registry rejections are not counted because they are treated as
/// hard errors: a single failed upsert stops the hydration loop with
/// [`HydrationError::Registry`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HydrationReport {
    /// Entries discovered in the durable store.
    pub entries_loaded: usize,
    /// Entries successfully upserted into the registry.
    pub entries_applied: usize,
}

/// Errors surfaced by [`hydrate_pane_registry_from_identity_store`].
///
/// The variants are split so the supervisor can decide between
/// "continue serving with empty title data" (Identity error during
/// list) and "abort startup" (registry mutation failure, which would
/// indicate a deeper bug).
#[derive(Debug, thiserror::Error)]
pub enum HydrationError {
    #[error("durable identity store unavailable: {0}")]
    Identity(#[from] PaneLaunchIdentityError),

    #[error("pane registry rejected hydrated descriptor: {0}")]
    Registry(#[from] RegistryError),
}

/// Replay every durable launch identity entry under `group` into the
/// supplied in-memory `registry`, producing baseline `PaneDescriptor`s
/// with `launch_origin` populated authoritatively.
///
/// This is the cross-process bridge: `workspace open` writes entries
/// in the parent CLI process, and the detached supervisor calls this
/// function once at startup to seed its own `InMemoryPaneRegistry`
/// before serving its first request. After hydration, `QueryPane`'s
/// `display_title` derivation has the authoritative source it needs
/// even though the runtime's foreground command may be `shell`.
///
/// The hydrated descriptors carry only the launch identity and basic
/// scaffolding (`PaneState::Idle`, `PaneRuntimeStatus::Unknown`,
/// empty `cwd`/`command`, no capabilities). Live observation later
/// fills cwd / foreground command / state via
/// [`PaneRegistryPort::update_from_observation`] during request
/// dispatch — that is the existing contract and we do not duplicate
/// it here.
pub fn hydrate_pane_registry_from_identity_store(
    store: &dyn PaneLaunchIdentityPort,
    registry: &dyn PaneRegistryPort,
    group: &str,
) -> Result<HydrationReport, HydrationError> {
    let entries = store.list_for_group(group)?;
    let mut report = HydrationReport {
        entries_loaded: entries.len(),
        ..HydrationReport::default()
    };

    for entry in entries {
        let descriptor = PaneDescriptor {
            runtime_id: PaneRuntimeId::derive(group, &entry.workspace, entry.unit_id.as_str()),
            group: group.to_owned(),
            workspace: entry.workspace.clone(),
            unit_id: entry.unit_id,
            cwd: String::new(),
            command: String::new(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            observed_status: PaneRuntimeStatus::Unknown,
            role: None,
            launch_origin: entry.launch_origin,
            capabilities: BTreeSet::new(),
        };
        // A registry rejection is treated as a hard error: the supervisor
        // cannot serve correct `display_title` values if the durable
        // identity didn't make it into the in-memory state. We bail out
        // immediately so the caller surfaces the failure rather than
        // silently degrading.
        registry
            .upsert(descriptor)
            .map_err(HydrationError::Registry)?;
        report.entries_applied += 1;
    }

    Ok(report)
}

/// Group sanitizer that mirrors the control-plane socket sanitizer.
///
/// We intentionally duplicate the rule (rather than re-export the
/// socket helper) because the two adapters are independent on the
/// boundary diagram, and a future change to one MUST be reviewed
/// alongside the other.
fn sanitize_group(group: &str) -> Result<String, PaneLaunchIdentityError> {
    if group.is_empty() {
        return Err(PaneLaunchIdentityError::Group {
            group: group.to_owned(),
            reason: "group must not be empty".to_owned(),
        });
    }
    let sanitized: String = group
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use workhive_core::domain::{PaneLaunchOrigin, ToolName, UnitId};

    fn fixture_entry(workspace: &str, unit_id: &str, tool: &str) -> PaneLaunchIdentityEntry {
        PaneLaunchIdentityEntry {
            workspace: workspace.to_owned(),
            unit_id: UnitId::new(unit_id),
            launch_origin: PaneLaunchOrigin::Tool {
                name: ToolName::new(tool).expect("tool name fixture"),
            },
        }
    }

    fn store_under(temp: &tempfile::TempDir) -> JsonPaneLaunchIdentityStore {
        JsonPaneLaunchIdentityStore::new(
            Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("tempdir path is utf-8"),
        )
    }

    #[test]
    fn record_and_list_round_trip() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        store
            .record("whsrv-demo", fixture_entry("ws1", "%4", "codex"))
            .expect("record codex");
        store
            .record("whsrv-demo", fixture_entry("ws1", "%5", "claude"))
            .expect("record claude");

        let entries = store.list_for_group("whsrv-demo").expect("list");
        assert_eq!(entries.len(), 2, "two entries expected: {entries:?}");
        let codex = entries
            .iter()
            .find(|e| e.unit_id.as_str() == "%4")
            .expect("codex entry");
        match &codex.launch_origin {
            PaneLaunchOrigin::Tool { name } => assert_eq!(name.as_str(), "codex"),
            other => panic!("expected Tool, got {other:?}"),
        }
    }

    #[test]
    fn record_is_idempotent_for_same_key() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        store
            .record("g", fixture_entry("ws", "%1", "codex"))
            .expect("first");
        store
            .record("g", fixture_entry("ws", "%1", "claude"))
            .expect("second");

        let entries = store.list_for_group("g").expect("list");
        assert_eq!(entries.len(), 1, "duplicates must collapse: {entries:?}");
        match &entries[0].launch_origin {
            PaneLaunchOrigin::Tool { name } => assert_eq!(name.as_str(), "claude"),
            other => panic!("expected Tool=claude, got {other:?}"),
        }
    }

    #[test]
    fn forget_workspace_keeps_other_workspaces() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        store
            .record("g", fixture_entry("ws1", "%1", "codex"))
            .expect("ws1");
        store
            .record("g", fixture_entry("ws2", "%2", "claude"))
            .expect("ws2");

        store.forget_workspace("g", "ws1").expect("forget ws1");
        let entries = store.list_for_group("g").expect("list");
        assert_eq!(entries.len(), 1, "only ws2 remains: {entries:?}");
        assert_eq!(entries[0].workspace, "ws2");
    }

    #[test]
    fn forget_group_removes_file() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        store
            .record("g", fixture_entry("ws1", "%1", "codex"))
            .expect("seed");
        let path = store.group_file("g").expect("group file path");
        assert!(path.as_std_path().exists(), "file should be created");

        store.forget_group("g").expect("forget");
        assert!(!path.as_std_path().exists(), "file removed");
        let entries = store.list_for_group("g").expect("list still ok");
        assert!(entries.is_empty(), "no entries: {entries:?}");
    }

    #[test]
    fn list_returns_empty_when_no_file_exists() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        let entries = store.list_for_group("never-recorded").expect("ok");
        assert!(entries.is_empty(), "must be empty: {entries:?}");
    }

    #[test]
    fn unsafe_group_chars_are_sanitized_in_path() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        let path = store.group_file("../evil/group").expect("sanitize");
        assert!(
            path.as_str().ends_with("/pane-identity/.._evil_group.json"),
            "must stay inside identity dir: {path}"
        );
    }

    #[test]
    fn empty_group_is_rejected() {
        let temp = tempdir().expect("tempdir");
        let store = store_under(&temp);
        let err = store
            .record("", fixture_entry("ws", "%1", "codex"))
            .unwrap_err();
        assert!(
            matches!(err, PaneLaunchIdentityError::Group { .. }),
            "expected Group error, got {err:?}"
        );
    }
}

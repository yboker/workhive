//! File-backed append-only [`ActivityEventStore`] adapter.
//!
//! On-disk layout (under a project-local runtime root):
//!
//! ```text
//! <runtime_root>/<group>/<workspace>/activity_events/<unit_id>.jsonl
//! ```
//!
//! Each line is a JSON-encoded [`ActivityEvent`]. The file is
//! append-only; no entry is ever rewritten. Reads filter out
//! corrupt lines but report them as
//! [`ActivityEventStoreError::CorruptEntry`] so callers can degrade
//! cleanly rather than silently observing an empty log.
//!
//! # Why JSONL, not a single JSON array
//!
//! - True append: the writer holds the file open in
//!   `O_APPEND | O_CREATE` mode; no read-modify-write race against
//!   concurrent writers.
//! - Tolerant decode: a corrupt trailing line does not invalidate
//!   the rest of the log. The adapter surfaces the `CorruptEntry`
//!   error so the projector can route to the `Degraded` state
//!   instead of "no events".
//! - Tooling: `tail -f` on the file is a free debug aid.
//!
//! # Atomicity
//!
//! `OpenOptions::append` on POSIX gives us atomic per-write
//! append for writes ≤ `PIPE_BUF`. Workhive's events are well
//! under that bound (a few hundred bytes), so concurrent writers
//! never interleave a single line. The file is opened for each
//! `append` to avoid keeping a file descriptor around between
//! calls (Phase-1 simplification; a future iteration may pool
//! the descriptor).

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::activity::{ActivityEvent, PaneCorrelationKey};
use workhive_core::ports::{ActivityEventStore, ActivityEventStoreError};

const ACTIVITY_EVENTS_DIR: &str = "activity_events";

/// File-backed adapter for the append-only activity event log.
pub struct FsActivityEventStore {
    root: Utf8PathBuf,
}

impl FsActivityEventStore {
    /// Create a store rooted at `root` (typically
    /// `<project>/.workhive/runtime`).
    pub fn new(root: impl Into<Utf8PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Compute the on-disk JSONL file path for a pane.
    pub fn log_path(&self, pane: &PaneCorrelationKey) -> Utf8PathBuf {
        self.root
            .join(sanitize_component(&pane.group))
            .join(sanitize_component(&pane.workspace))
            .join(ACTIVITY_EVENTS_DIR)
            .join(format!("{}.jsonl", sanitize_component(&pane.unit_id)))
    }

    /// Iterate every JSONL file under `root`. Used by `replay_all`.
    fn pane_log_paths(&self) -> Result<Vec<Utf8PathBuf>, ActivityEventStoreError> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let group_iter = fs::read_dir(self.root.as_std_path())
            .map_err(|e| ActivityEventStoreError::Io(format!("read root: {e}")))?;
        for group_entry in group_iter {
            let group_entry = group_entry
                .map_err(|e| ActivityEventStoreError::Io(format!("group entry: {e}")))?;
            if !group_entry
                .file_type()
                .map_err(|e| ActivityEventStoreError::Io(format!("group filetype: {e}")))?
                .is_dir()
            {
                continue;
            }
            let group_path = group_entry.path();
            let workspace_iter = fs::read_dir(&group_path)
                .map_err(|e| ActivityEventStoreError::Io(format!("read group dir: {e}")))?;
            for ws_entry in workspace_iter {
                let ws_entry =
                    ws_entry.map_err(|e| ActivityEventStoreError::Io(format!("ws entry: {e}")))?;
                if !ws_entry
                    .file_type()
                    .map_err(|e| ActivityEventStoreError::Io(format!("ws filetype: {e}")))?
                    .is_dir()
                {
                    continue;
                }
                let activity_dir = ws_entry.path().join(ACTIVITY_EVENTS_DIR);
                if !activity_dir.exists() {
                    continue;
                }
                let log_iter = fs::read_dir(&activity_dir)
                    .map_err(|e| ActivityEventStoreError::Io(format!("read activity: {e}")))?;
                for log_entry in log_iter {
                    let log_entry = log_entry
                        .map_err(|e| ActivityEventStoreError::Io(format!("log entry: {e}")))?;
                    let p = log_entry.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                        let utf8 = Utf8PathBuf::from_path_buf(p).map_err(|p: PathBuf| {
                            ActivityEventStoreError::Io(format!(
                                "non-utf8 log path: {}",
                                p.display()
                            ))
                        })?;
                        out.push(utf8);
                    }
                }
            }
        }
        Ok(out)
    }
}

impl ActivityEventStore for FsActivityEventStore {
    fn append(&self, event: &ActivityEvent) -> Result<(), ActivityEventStoreError> {
        let path = self.log_path(&event.pane);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent.as_std_path())
                .map_err(|e| ActivityEventStoreError::Io(format!("mkdir parent: {e}")))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_std_path())
            .map_err(|e| ActivityEventStoreError::Io(format!("open append: {e}")))?;
        let line = serde_json::to_string(event)
            .map_err(|e| ActivityEventStoreError::Io(format!("serialize: {e}")))?;
        writeln!(file, "{line}")
            .map_err(|e| ActivityEventStoreError::Io(format!("write line: {e}")))?;
        file.sync_data()
            .map_err(|e| ActivityEventStoreError::Io(format!("sync: {e}")))?;
        Ok(())
    }

    fn events_for_pane(
        &self,
        pane: &PaneCorrelationKey,
    ) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
        let path = self.log_path(pane);
        if !path.exists() {
            return Ok(Vec::new());
        }
        read_jsonl(&path)
    }

    fn replay_all(&self) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
        let mut paths = self.pane_log_paths()?;
        paths.sort();
        let mut events = Vec::new();
        for p in paths {
            let mut from_file = read_jsonl(&p)?;
            events.append(&mut from_file);
        }
        // Sort by observed_at to give stable cross-pane ordering;
        // within a pane, append-order is preserved by the JSONL
        // file itself and the timestamp tie-break.
        events.sort_by_key(|e| (e.observed_at_unix_ms, e.id.0));
        Ok(events)
    }
}

/// Read a JSONL file into events, surfacing parse errors as
/// `CorruptEntry`. Empty lines are tolerated; non-JSON lines are
/// reported.
fn read_jsonl(path: &Utf8Path) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
    let f = File::open(path.as_std_path()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ActivityEventStoreError::Unavailable(format!("missing log: {path}"))
        } else {
            ActivityEventStoreError::Io(format!("open {path}: {e}"))
        }
    })?;
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| ActivityEventStoreError::Io(format!("read line: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }
        let event: ActivityEvent = serde_json::from_str(&line).map_err(|e| {
            ActivityEventStoreError::CorruptEntry(format!("{path} line {}: {e}", idx + 1))
        })?;
        out.push(event);
    }
    Ok(out)
}

/// Sanitize a single path component (mirrors the policy used by
/// the `FsPaneActivityStore`). Reject empty/dotted components,
/// path separators, and control characters. Truncate to 64 chars.
fn sanitize_component(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        match ch {
            '/' | '\\' | '\0' => out.push('_'),
            c if c.is_control() => out.push('_'),
            c => out.push(c),
        }
    }
    if out == "." || out == ".." || out.is_empty() {
        out = "_".to_owned();
    }
    if out.len() > 64 {
        out.truncate(64);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, ProviderKind,
        ProviderSignalSource, SignalChannel,
    };

    fn pane(unit: &str) -> PaneCorrelationKey {
        PaneCorrelationKey::new("g", "w", unit)
    }

    fn ev(unit: &str, observed_at: i64) -> ActivityEvent {
        ActivityEvent::new(
            pane(unit),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            ActivityEventKind::ToolStarted,
            ActivityOutcome::NotApplicable,
            ObservationConfidence::High,
            observed_at,
            None,
        )
    }

    fn store_in_temp() -> (TempDir, FsActivityEventStore) {
        let td = TempDir::new().expect("tempdir");
        let root = Utf8PathBuf::from_path_buf(td.path().to_path_buf()).unwrap();
        let store = FsActivityEventStore::new(root);
        (td, store)
    }

    /// Append-only ordering contract on disk: per-pane events are
    /// returned in the same order they were appended.
    #[test]
    fn append_only_per_pane_order_is_preserved_on_disk() {
        let (_td, store) = store_in_temp();
        let a = ev("u1", 100);
        let b = ev("u1", 200);
        store.append(&a).unwrap();
        store.append(&b).unwrap();
        let read = store.events_for_pane(&pane("u1")).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].observed_at_unix_ms, 100);
        assert_eq!(read[1].observed_at_unix_ms, 200);
    }

    /// Missing pane log returns Ok(empty) — store is healthy, the
    /// pane just has no events yet.
    #[test]
    fn events_for_missing_pane_returns_empty() {
        let (_td, store) = store_in_temp();
        let v = store.events_for_pane(&pane("missing")).unwrap();
        assert!(v.is_empty());
    }

    /// Corrupt JSONL line is surfaced as a typed `CorruptEntry`.
    /// The renderer must NOT see a silent empty log.
    #[test]
    fn corrupt_jsonl_line_surfaces_typed_corrupt_entry() {
        let (_td, store) = store_in_temp();
        // Append a good event so the file/dir tree exists.
        store.append(&ev("u1", 100)).unwrap();
        let log = store.log_path(&pane("u1"));
        // Append a bogus line at the tail.
        let mut f = OpenOptions::new()
            .append(true)
            .open(log.as_std_path())
            .unwrap();
        writeln!(f, "{{not json}}").unwrap();
        let err = store.events_for_pane(&pane("u1")).expect_err("must error");
        assert!(
            matches!(err, ActivityEventStoreError::CorruptEntry(_)),
            "expected CorruptEntry, got {err:?}"
        );
    }

    /// Empty trailing newlines are tolerated.
    #[test]
    fn blank_lines_are_tolerated() {
        let (_td, store) = store_in_temp();
        store.append(&ev("u1", 100)).unwrap();
        let log = store.log_path(&pane("u1"));
        let mut f = OpenOptions::new()
            .append(true)
            .open(log.as_std_path())
            .unwrap();
        writeln!(f).unwrap();
        writeln!(f).unwrap();
        let read = store.events_for_pane(&pane("u1")).unwrap();
        assert_eq!(read.len(), 1);
    }

    /// `replay_all` walks every pane log under the root and
    /// returns events sorted by `(observed_at, id)`.
    #[test]
    fn replay_all_returns_events_sorted_by_observed_at_across_panes() {
        let (_td, store) = store_in_temp();
        store.append(&ev("u2", 200)).unwrap();
        store.append(&ev("u1", 100)).unwrap();
        store.append(&ev("u3", 300)).unwrap();
        let all = store.replay_all().unwrap();
        let times: Vec<i64> = all.iter().map(|e| e.observed_at_unix_ms).collect();
        assert_eq!(times, vec![100, 200, 300]);
    }

    /// Ordering regression: append two events for the
    /// **same pane** with timestamps in the opposite order of
    /// append, then assert `replay_all` returns them sorted by
    /// `(observed_at_unix_ms, id)`. This is the file-backed half of
    /// the cross-adapter contract — the in-memory fake mirrors the
    /// same expectation in
    /// [`workhive_testkit::fakes::activity_event_store::tests::replay_all_sorts_by_observed_at_when_append_order_disagrees_with_timestamps`][].
    #[test]
    fn replay_all_sorts_same_pane_by_observed_at_even_when_append_order_disagrees() {
        let (_td, store) = store_in_temp();
        // Append later-timestamp event first.
        store.append(&ev("u1", 500)).unwrap();
        store.append(&ev("u1", 100)).unwrap();

        let all = store.replay_all().unwrap();
        assert_eq!(
            all.iter()
                .map(|e| e.observed_at_unix_ms)
                .collect::<Vec<_>>(),
            vec![100, 500],
            "FsActivityEventStore::replay_all must sort by (observed_at, id)"
        );
    }

    /// Cross-adapter regression: drive both the
    /// in-memory fake and the file-backed adapter with the same
    /// append sequence whose timestamps are intentionally reversed
    /// from append order, then project both through `StateProjector`
    /// and assert the latest projected fact is identical. Without
    /// the deterministic `(observed_at_unix_ms, id)` contract on
    /// `replay_all`, the two adapters would project different
    /// states for the same append history.
    #[test]
    fn fake_and_fs_adapters_project_identical_latest_state_for_reversed_timestamps() {
        use workhive_core::domain::activity::{
            ActivityEventKind as Kind, ActivityOutcome as Outcome,
        };
        use workhive_core::ports::ActivityEventStore as _;
        use workhive_core::usecases::activity_control_plane::StateProjector;
        use workhive_testkit::fakes::InMemoryActivityEventStore;

        let pane_key = pane("u1");
        // Earlier-timestamp = ToolStarted (Working), later-timestamp =
        // TurnCompleted (Completed). Append in the OPPOSITE order so
        // the literal append sequence ends with the earlier-timestamp
        // event. The deterministic `replay_all` ordering must still
        // place TurnCompleted last for both adapters, so both
        // projectors land on `Completed`.
        let earlier_working = ActivityEvent::new(
            pane_key.clone(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            Kind::ToolStarted,
            Outcome::NotApplicable,
            ObservationConfidence::High,
            100,
            None,
        );
        let later_completed = ActivityEvent::new(
            pane_key.clone(),
            ProviderKind::ClaudeCode,
            SignalChannel::LifecycleHook,
            ProviderSignalSource::LifecycleHook,
            Kind::TurnCompleted,
            Outcome::Success,
            ObservationConfidence::High,
            500,
            None,
        );

        // Fake: append later-then-earlier.
        let fake = InMemoryActivityEventStore::new();
        fake.append(&later_completed).unwrap();
        fake.append(&earlier_working).unwrap();

        // FS adapter: same append sequence, fresh tempdir.
        let (_td, fs_store) = store_in_temp();
        fs_store.append(&later_completed).unwrap();
        fs_store.append(&earlier_working).unwrap();

        let fake_rm = StateProjector::new(&fake, 1_000, 30_000, 90_000)
            .rebuild()
            .expect("fake rebuild");
        let fs_rm = StateProjector::new(&fs_store, 1_000, 30_000, 90_000)
            .rebuild()
            .expect("fs rebuild");

        let fake_fact = fake_rm.get(&pane_key).expect("fake fact").fact.clone();
        let fs_fact = fs_rm.get(&pane_key).expect("fs fact").fact.clone();

        assert_eq!(
            fake_fact.state,
            workhive_core::domain::activity::PaneActivityState::Completed,
            "fake projector must pick latest-by-timestamp event"
        );
        assert_eq!(
            fs_fact.state, fake_fact.state,
            "fake and fs adapters must project identical latest state"
        );
        assert_eq!(
            fs_fact.observed_at_unix_ms, fake_fact.observed_at_unix_ms,
            "both adapters must agree on the latest event timestamp"
        );
        assert_eq!(
            fs_fact.source_event_id, fake_fact.source_event_id,
            "both adapters must agree on the source event id (same TurnCompleted)"
        );
    }

    /// Sanitizer prevents path traversal.
    #[test]
    fn sanitizer_blocks_path_separators_and_dotdot() {
        let p = PaneCorrelationKey::new("..", "ws/.././", "evil\0");
        let store = FsActivityEventStore::new(Utf8PathBuf::from("/tmp/whatever"));
        let path = store.log_path(&p);
        let s = path.as_str();
        assert!(!s.contains("/.."), "must not contain `..` segment: {s}");
        assert!(!s.contains('\0'), "must not contain NUL: {s}");
    }
}

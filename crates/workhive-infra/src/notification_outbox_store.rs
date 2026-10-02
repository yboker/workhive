//! File-backed append-only [`NotificationOutboxStore`] adapter.
//!
//! ## Purpose
//!
//! The event-driven notification trigger
//! ([`workhive_cli::commands::notification_trigger::evaluate_after_acp_append`])
//! is called once per ACP event append. Within a single
//! invocation, [`workhive_core::ports::NotificationOutbox`]
//! prevents duplicate delivery via an in-memory
//! `HashSet<TransitionSignature>`. But the trigger constructs a
//! fresh outbox on every call, so a second event for the same
//! pane that re-projects into the same notification-worthy state
//! would re-fire the notification without a persistent dedupe
//! boundary.
//!
//! This adapter is the persistent boundary. It records every
//! `TransitionSignature` that was successfully delivered in any
//! prior invocation (this process or a previous one) so the
//! trigger can seed its fresh outbox at the start of each
//! invocation and not re-deliver.
//!
//! ## On-disk layout
//!
//! ```text
//! <runtime_root>/notifications/outbox.jsonl
//! ```
//!
//! Each line is a single JSON record:
//!
//! ```json
//! {"sig":"<TransitionSignature string>"}
//! ```
//!
//! The file is append-only; no entry is ever rewritten. Records
//! are flushed and `fsync`-style synced with
//! [`File::sync_data`] after every append so a crash between the
//! return of `record_delivered` and the next invocation cannot
//! lose a signature.
//!
//! ## Privacy
//!
//! [`TransitionSignature`] is a metadata-only string (see its
//! docs in `workhive-core`). It carries either an ACP event UUID
//! or `pane:category:observed_at_unix_ms`. There is no prompt or
//! message content in this file, ever. We log the on-disk path
//! at `debug` only.
//!
//! ## Atomicity
//!
//! `OpenOptions::append` on POSIX gives us atomic per-write
//! append for writes ≤ `PIPE_BUF`. Signature lines are well
//! under that bound (a few tens of bytes), so concurrent writers
//! never interleave. The file is opened for each `record_delivered`
//! call to avoid keeping a file descriptor around between calls.
//!
//! ## Fail-open
//!
//! Both `load_delivered_signatures` and `record_delivered` return
//! [`NotificationOutboxStoreError`] on I/O failure. The trigger
//! boundary MUST log the error at `warn` and continue with an
//! empty seeded set (the worst case is one duplicate notification
//! after a corrupted read). The store NEVER blocks the
//! supervisor or provider hook on a write failure.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use workhive_core::ports::{
    NotificationOutboxStore, NotificationOutboxStoreError, TransitionSignature,
};

/// Directory under the runtime root that holds the outbox.
const NOTIFICATIONS_DIR: &str = "notifications";
/// File name of the append-only signatures log.
const OUTBOX_FILE: &str = "outbox.jsonl";

/// On-disk record shape for a single delivered signature.
///
/// Kept private to the adapter so the wire format can evolve
/// without leaking through the [`NotificationOutboxStore`] trait.
#[derive(Debug, Serialize, Deserialize)]
struct DeliveredSignatureRecord {
    /// The opaque `TransitionSignature` string. See the type's
    /// doc in `workhive-core` for the format and invariants.
    sig: String,
}

/// File-backed adapter for the durable notification outbox store.
///
/// Rooted at the same `runtime_root` that
/// [`crate::activity_event_store::FsActivityEventStore`] uses,
/// so the dedupe boundary lives next to the event store it
/// dedupes for. The trigger boundary builds the path with
/// [`Self::log_path`] and the rest is hidden behind the
/// [`NotificationOutboxStore`] trait.
pub struct FsNotificationOutboxStore {
    root: Utf8PathBuf,
}

impl FsNotificationOutboxStore {
    /// Create a store rooted at `runtime_root` (typically
    /// `<project>/.workhive/runtime`).
    pub fn new(runtime_root: impl Into<Utf8PathBuf>) -> Self {
        Self {
            root: runtime_root.into(),
        }
    }

    /// On-disk path of the append-only outbox log.
    ///
    /// Exposed for diagnostics and tests. The directory is
    /// created on the first `record_delivered` call.
    pub fn log_path(&self) -> Utf8PathBuf {
        self.root.join(NOTIFICATIONS_DIR).join(OUTBOX_FILE)
    }

    /// Ensure the parent directory exists. Idempotent.
    fn ensure_parent(&self) -> Result<(), NotificationOutboxStoreError> {
        let parent = self.root.join(NOTIFICATIONS_DIR);
        fs::create_dir_all(parent.as_std_path()).map_err(|e| NotificationOutboxStoreError {
            reason: format!("create outbox dir {}: {e}", parent),
        })
    }
}

impl NotificationOutboxStore for FsNotificationOutboxStore {
    fn load_delivered_signatures(
        &self,
    ) -> Result<Vec<TransitionSignature>, NotificationOutboxStoreError> {
        let path = self.log_path();
        if !path.exists() {
            // First-run / fresh-install: nothing recorded yet.
            return Ok(Vec::new());
        }
        let f = File::open(path.as_std_path()).map_err(|e| NotificationOutboxStoreError {
            reason: format!("open outbox file {path}: {e}"),
        })?;
        let rdr = BufReader::new(f);
        let mut out = Vec::new();
        for (line_no, line) in rdr.lines().enumerate() {
            let line = line.map_err(|e| NotificationOutboxStoreError {
                reason: format!("read outbox line {} in {path}: {e}", line_no + 1),
            })?;
            // Tolerant decode: skip blank lines and tolerate
            // a corrupt trailing line (POSIX append-with-crash
            // can leave a partial record). Any other malformed
            // line is still tolerated — we log and skip, not
            // fail closed, per the fail-open contract.
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<DeliveredSignatureRecord>(trimmed) {
                Ok(rec) => out.push(TransitionSignature::from_event_id(&rec.sig)),
                Err(_e) => {
                    tracing::warn!(
                        target: "workhive.notification.outbox_store",
                        path = %path,
                        line = line_no + 1,
                        "skipping corrupt outbox entry (fail-open)"
                    );
                    continue;
                }
            }
        }
        Ok(out)
    }

    fn record_delivered(
        &self,
        sig: &TransitionSignature,
    ) -> Result<(), NotificationOutboxStoreError> {
        self.ensure_parent()?;
        let path = self.log_path();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_std_path())
            .map_err(|e| NotificationOutboxStoreError {
                reason: format!("open outbox for append {path}: {e}"),
            })?;
        let rec = DeliveredSignatureRecord { sig: sig.0.clone() };
        let mut line = serde_json::to_string(&rec).map_err(|e| NotificationOutboxStoreError {
            reason: format!("serialize outbox entry: {e}"),
        })?;
        line.push('\n');
        f.write_all(line.as_bytes())
            .map_err(|e| NotificationOutboxStoreError {
                reason: format!("write outbox entry to {path}: {e}"),
            })?;
        // sync_data is enough — we only need the file contents on
        // disk, not its inode metadata. This makes the next
        // `load_delivered_signatures` see the entry even after a
        // crash between this call and process exit.
        f.sync_data().map_err(|e| NotificationOutboxStoreError {
            reason: format!("fsync outbox file {path}: {e}"),
        })?;
        Ok(())
    }
}

impl FsNotificationOutboxStore {
    /// Read-only view of the on-disk records as a [`Utf8Path`]
    /// for diagnostic logging. The path may not exist yet.
    pub fn path(&self) -> &Utf8Path {
        self.root.as_path()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workhive_core::domain::activity::PaneCorrelationKey;
    use workhive_core::ports::{NotificationCategory, TransitionSignature};

    fn temp_root() -> (TempDir, Utf8PathBuf) {
        let td = TempDir::new().expect("create tempdir");
        let p =
            Utf8PathBuf::from_path_buf(td.path().to_path_buf()).expect("temp path must be utf-8");
        (td, p)
    }

    /// First-run round trip: empty store loads as empty, then a
    /// single recorded signature reloads correctly.
    #[test]
    fn empty_store_loads_empty_then_round_trips_a_signature() {
        let (_td, root) = temp_root();
        let store = FsNotificationOutboxStore::new(root);

        let loaded = store
            .load_delivered_signatures()
            .expect("first load on empty store must succeed");
        assert!(loaded.is_empty(), "fresh store must be empty");

        let sig = TransitionSignature::from_event_id("evt-fresh-1");
        store
            .record_delivered(&sig)
            .expect("record_delivered must succeed on a writable temp root");

        let loaded = store
            .load_delivered_signatures()
            .expect("second load must succeed");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], sig);
    }

    /// Recording the same signature twice must produce a load
    /// that contains it (duplicates are harmless; the trigger
    /// boundary collapses them via the `HashSet` in
    /// `NotificationOutbox::seed_delivered`).
    #[test]
    fn duplicate_record_is_harmless_and_load_collapses_via_hashset_at_caller() {
        let (_td, root) = temp_root();
        let store = FsNotificationOutboxStore::new(root);

        let sig = TransitionSignature::from_event_id("evt-dup");
        store.record_delivered(&sig).unwrap();
        store.record_delivered(&sig).unwrap();

        let loaded = store.load_delivered_signatures().unwrap();
        // The store itself returns both entries — collapse is
        // the caller's job. We just assert no IO panic.
        assert!(!loaded.is_empty());
        assert!(loaded.iter().all(|s| *s == sig));
    }

    /// Two signatures recorded in sequence must both reload, in
    /// any order (the caller does not depend on insertion order
    /// because the seeded set is a hash set).
    #[test]
    fn multiple_signatures_are_persisted_and_reloaded() {
        let (_td, root) = temp_root();
        let store = FsNotificationOutboxStore::new(root);

        let s1 = TransitionSignature::from_event_id("evt-A");
        let s2 = TransitionSignature::from_seq(
            &PaneCorrelationKey::new("g", "w", "u"),
            NotificationCategory::PermissionNeeded,
            42,
        );

        store.record_delivered(&s1).unwrap();
        store.record_delivered(&s2).unwrap();

        let loaded = store.load_delivered_signatures().unwrap();
        assert!(loaded.contains(&s1));
        assert!(loaded.contains(&s2));
    }

    /// A corrupt trailing line must not poison the entire load —
    /// the rest of the file must still be returned. This is the
    /// crash-safety property the fail-open contract requires.
    #[test]
    fn corrupt_trailing_line_does_not_poison_load() {
        let (_td, root) = temp_root();
        let store = FsNotificationOutboxStore::new(root.clone());

        // Write one good record, then append a partial line
        // (simulating crash mid-write).
        let sig = TransitionSignature::from_event_id("evt-survives-corruption");
        store.record_delivered(&sig).unwrap();
        // Append a corrupt trailing line by hand.
        let path = store.log_path();
        use std::io::Write;
        let mut f = OpenOptions::new()
            .append(true)
            .open(path.as_std_path())
            .unwrap();
        f.write_all(b"{this is not valid json\n").unwrap();

        let loaded = store.load_delivered_signatures().unwrap();
        assert!(
            loaded.contains(&sig),
            "good record must survive a corrupt trailing line"
        );
    }

    /// Append-only invariant: previous entries are never
    /// rewritten when a new entry arrives.
    #[test]
    fn append_does_not_rewrite_prior_entries() {
        let (_td, root) = temp_root();
        let store = FsNotificationOutboxStore::new(root);

        let s1 = TransitionSignature::from_event_id("evt-first");
        store.record_delivered(&s1).unwrap();

        // Read the raw bytes after the first write.
        let path = store.log_path();
        let before = std::fs::read(path.as_std_path()).unwrap();

        let s2 = TransitionSignature::from_event_id("evt-second");
        store.record_delivered(&s2).unwrap();

        let after = std::fs::read(path.as_std_path()).unwrap();
        // The prefix of the file MUST be identical to `before` —
        // i.e. the second write appended without rewriting.
        assert!(
            after.starts_with(&before),
            "append-only invariant violated: file was rewritten"
        );
        assert!(
            after.len() > before.len(),
            "second record must have appended new bytes"
        );
    }
}

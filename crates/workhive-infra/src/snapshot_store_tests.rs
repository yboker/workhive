use super::*;
use tempfile::TempDir;
use workhive_core::domain::{
    PaneLaunchIntent, PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy,
    SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, SnapshotRevisionId, SnapshotSource, WindowSnapshot,
    WorkspaceSnapshot, WorkspaceStateKey, WorkspaceTopology,
};

fn setup() -> (TempDir, FileSnapshotStoreAdapter) {
    let tmp = TempDir::new().expect("create temp dir");
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("utf8 path");
    let adapter = FileSnapshotStoreAdapter::new(&root);
    (tmp, adapter)
}

/// Construction must be inert — no `.workhive/snapshots/` directory should
/// appear on disk until the first actual snapshot write. This is the
/// runtime-side counterpart of the workspace_fs guarantee that init does
/// not eagerly create `.workhive/snapshots/`.
#[test]
fn construction_does_not_create_snapshots_dir() {
    let tmp = TempDir::new().expect("create temp dir");
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("utf8 path");
    let _adapter = FileSnapshotStoreAdapter::new(&root);
    assert!(
        !root.join(".workhive").join("snapshots").exists(),
        "FileSnapshotStoreAdapter::new must not create .workhive/snapshots/ eagerly",
    );
}

/// First-write must lazily materialize `.workhive/snapshots/` and the
/// scoped `revisions/` subtree. This pins the lazy-creation contract.
#[test]
fn first_write_creates_snapshots_dir_lazily() {
    let (_tmp, store) = setup();
    // Path is derived from the adapter root for assertion purposes.
    let snap = sample_snapshot(1);
    // Inspect parent root via a freshly created adapter on the same path.
    let root_dir = store.root.parent().expect("snapshots root has parent");
    let snapshots_dir = store.root.clone();
    assert!(
        !snapshots_dir.exists(),
        "snapshots dir must not exist before first write",
    );
    // Also `.workhive/` itself was never created by the adapter.
    // (It is created elsewhere by workspace_fs.)
    let _ = root_dir;

    store.save_revision(&snap).expect("save");
    assert!(
        snapshots_dir.exists(),
        "snapshots dir must be lazily created on first write",
    );
}

fn state_key() -> WorkspaceStateKey {
    WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        String::new(),
        "main".to_owned(),
    )
}

fn sample_snapshot(rev: u64) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(rev),
        state_key: state_key(),
        captured_at: format!("2026-04-24T12:{rev:02}:00Z"),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".to_owned(),
                panes: vec![PaneRef {
                    pane_index: 0,
                    window_index: 0,
                }],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![PaneSnapshot {
            runtime_id: None,
            window_index: 0,
            pane_index: 0,
            pane_id: None,
            title: None,
            cwd: Utf8PathBuf::from("/tmp/proj"),
            managed: true,
            restore_policy: RestorePolicy::Restore,
            launch_intent: PaneLaunchIntent::EmptyShell,
            observed: PaneObservedState {
                current_command: Some("zsh".to_owned()),
                start_command: None,
                active: true,
                dead: false,
            },
        }],
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".to_owned(),
            platform: "macos".to_owned(),
            generation: rev,
            content_hash: None,
            config_fingerprint: None,
        },
    }
}

#[test]
fn save_and_load_revision() {
    let (_tmp, store) = setup();
    let snap = sample_snapshot(1);
    let rev = store.save_revision(&snap).expect("save");
    assert_eq!(rev, SnapshotRevisionId(1));

    let loaded = store
        .load_latest(&state_key())
        .expect("load")
        .expect("should exist");
    assert_eq!(loaded.revision_id, SnapshotRevisionId(1));
    assert_eq!(loaded.panes.len(), 1);
}

#[test]
fn load_specific_revision() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save 1");
    store.save_revision(&sample_snapshot(2)).expect("save 2");

    let loaded = store
        .load_revision(&state_key(), SnapshotRevisionId(1))
        .expect("load")
        .expect("should exist");
    assert_eq!(loaded.revision_id, SnapshotRevisionId(1));
}

#[test]
fn load_latest_without_manifest_finds_highest_rev() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save 1");
    store.save_revision(&sample_snapshot(3)).expect("save 3");
    store.save_revision(&sample_snapshot(2)).expect("save 2");
    let manifest_path = store.manifest_path(&state_key());
    std::fs::remove_file(manifest_path.as_std_path()).expect("remove manifest");

    let loaded = store
        .load_latest(&state_key())
        .expect("load")
        .expect("should exist");
    assert_eq!(loaded.revision_id, SnapshotRevisionId(3));
}

#[test]
fn load_latest_with_manifest() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save 1");
    store.save_revision(&sample_snapshot(2)).expect("save 2");

    let manifest = SnapshotManifest {
        state_key: state_key(),
        head_revision: SnapshotRevisionId(1),
        total_revisions: 2,
        events_since_full: 0,
        last_saved_at: "2026-04-24T12:00:00Z".to_owned(),
        last_full_revision_at: "2026-04-24T12:00:00Z".to_owned(),
    };
    store.write_manifest(&manifest).expect("write manifest");

    let loaded = store
        .load_latest(&state_key())
        .expect("load")
        .expect("should exist");
    assert_eq!(
        loaded.revision_id,
        SnapshotRevisionId(1),
        "manifest should direct to rev 1"
    );
}

#[test]
fn load_missing_returns_none() {
    let (_tmp, store) = setup();
    let result = store.load_latest(&state_key()).expect("load");
    assert!(result.is_none());
}

#[test]
fn append_and_load_events() {
    let (_tmp, store) = setup();
    use workhive_core::domain::{SnapshotEvent, SnapshotEventKind};

    let events = vec![
        SnapshotEvent {
            kind: SnapshotEventKind::PaneAdded,
            timestamp: "2026-04-24T12:01:00Z".to_owned(),
            revision_id: Some(SnapshotRevisionId(1)),
            detail: None,
        },
        SnapshotEvent {
            kind: SnapshotEventKind::LayoutChanged,
            timestamp: "2026-04-24T12:02:00Z".to_owned(),
            revision_id: Some(SnapshotRevisionId(2)),
            detail: Some("new layout".to_owned()),
        },
    ];
    store.append_events(&state_key(), &events).expect("append");

    let loaded = store
        .load_events_since(&state_key(), SnapshotRevisionId(0))
        .expect("load events");
    assert_eq!(loaded.len(), 2);

    let loaded_after_rev1 = store
        .load_events_since(&state_key(), SnapshotRevisionId(1))
        .expect("load events");
    assert_eq!(loaded_after_rev1.len(), 1);
    assert!(matches!(
        loaded_after_rev1[0].kind,
        SnapshotEventKind::LayoutChanged
    ));
}

#[test]
fn save_revision_updates_manifest_head_and_counts() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save rev1");
    store.save_revision(&sample_snapshot(2)).expect("save rev2");

    let manifest = store
        .read_manifest(&state_key())
        .expect("read manifest")
        .expect("manifest should exist");
    assert_eq!(manifest.head_revision, SnapshotRevisionId(2));
    assert_eq!(manifest.total_revisions, 2);
    assert_eq!(manifest.events_since_full, 0);
}

#[test]
fn retention_prunes_old_epoch_events() {
    let (_tmp, store) = setup();
    use workhive_core::domain::{SnapshotEvent, SnapshotEventKind};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("unix time")
        .as_secs();
    let old = now.saturating_sub(3 * 24 * 60 * 60);
    let events = vec![
        SnapshotEvent {
            kind: SnapshotEventKind::PaneAdded,
            timestamp: format!("{old}s"),
            revision_id: Some(SnapshotRevisionId(1)),
            detail: None,
        },
        SnapshotEvent {
            kind: SnapshotEventKind::PaneCommandChanged,
            timestamp: format!("{now}s"),
            revision_id: Some(SnapshotRevisionId(2)),
            detail: Some("revision=000002".to_owned()),
        },
    ];
    store.append_events(&state_key(), &events).expect("append");
    store
        .apply_retention(&state_key(), 32, 1)
        .expect("apply retention");

    let loaded = store
        .load_events_since(&state_key(), SnapshotRevisionId(0))
        .expect("load events");
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].revision_id, Some(SnapshotRevisionId(2)));
}

#[test]
fn history_is_sorted() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(3)).expect("save 3");
    store.save_revision(&sample_snapshot(1)).expect("save 1");
    store.save_revision(&sample_snapshot(2)).expect("save 2");

    let history = store.list_history(&state_key()).expect("history");
    let ids: Vec<u64> = history.iter().map(|h| h.revision_id.0).collect();
    assert_eq!(ids, vec![1, 2, 3]);
}

#[test]
fn retention_prunes_old_revisions() {
    let (_tmp, store) = setup();
    for i in 1..=5 {
        store
            .save_revision(&sample_snapshot(i))
            .unwrap_or_else(|_| panic!("save {i}"));
    }

    let removed = store
        .apply_retention(&state_key(), 2, 30)
        .expect("retention");
    assert_eq!(removed, 3);

    let history = store.list_history(&state_key()).expect("history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].revision_id.0, 4);
    assert_eq!(history[1].revision_id.0, 5);
}

#[test]
fn delete_all_for_state_key() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save");
    store.delete_all(&state_key()).expect("delete");

    let result = store.load_latest(&state_key()).expect("load");
    assert!(result.is_none());
}

#[test]
fn delete_all_global() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save");
    store.delete_all_global().expect("delete");

    let result = store.load_latest(&state_key()).expect("load");
    assert!(result.is_none());
}

#[test]
fn verify_valid_revision() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save");
    assert!(
        store
            .verify_revision(&state_key(), SnapshotRevisionId(1))
            .expect("verify")
    );
}

#[test]
fn verify_missing_revision_is_false() {
    let (_tmp, store) = setup();
    assert!(
        !store
            .verify_revision(&state_key(), SnapshotRevisionId(99))
            .expect("verify")
    );
}

#[test]
fn health_check_basic() {
    let (_tmp, store) = setup();
    store.save_revision(&sample_snapshot(1)).expect("save");

    let health = store.health_check(&state_key()).expect("health");
    assert!(health.enabled);
    assert!(!health.supervisor_running);
    assert!(health.latest_revision.is_some());
    assert_eq!(health.checksum_valid, Some(true));
}

#[test]
fn health_check_reports_hooks_registered_when_state_file_true() {
    let (_tmp, store) = setup();
    let key = state_key();
    let lock = SupervisorLock {
        pid: std::process::id(),
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: key.clone(),
    };
    store.acquire_lock(&lock).expect("acquire lock");
    store
        .set_hooks_registered_state(&key, true)
        .expect("set hooks true");

    let health = store.health_check(&key).expect("health");
    assert!(health.supervisor_running);
    assert!(health.hooks_registered);
}

#[test]
fn health_check_reports_hooks_disabled_when_state_file_false() {
    let (_tmp, store) = setup();
    let key = state_key();
    let lock = SupervisorLock {
        pid: std::process::id(),
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: key.clone(),
    };
    store.acquire_lock(&lock).expect("acquire lock");
    store
        .set_hooks_registered_state(&key, false)
        .expect("set hooks false");

    let health = store.health_check(&key).expect("health");
    assert!(health.supervisor_running);
    assert!(!health.hooks_registered);
}

#[test]
fn lock_acquire_release() {
    let (_tmp, store) = setup();
    let lock = SupervisorLock {
        pid: std::process::id(),
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: state_key(),
    };

    assert!(store.acquire_lock(&lock).expect("acquire"));
    let read = store.read_lock(&state_key()).expect("read").expect("lock");
    assert_eq!(read.pid, std::process::id());

    store.release_lock(&state_key()).expect("release");
    assert!(store.read_lock(&state_key()).expect("read").is_none());
}

#[test]
fn lock_stale_process_is_overridden() {
    let (_tmp, store) = setup();
    let stale_lock = SupervisorLock {
        pid: 99999999,
        heartbeat: "2026-04-24T10:00:00Z".to_owned(),
        state_key: state_key(),
    };
    store.acquire_lock(&stale_lock).expect("acquire stale");

    let new_lock = SupervisorLock {
        pid: std::process::id(),
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: state_key(),
    };
    assert!(
        store.acquire_lock(&new_lock).expect("acquire new"),
        "should override stale lock"
    );
}

#[test]
fn different_state_keys_are_isolated() {
    let (_tmp, store) = setup();
    let key_a = WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        String::new(),
        "main".to_owned(),
    );
    let key_b = WorkspaceStateKey::new(
        "a1b2".to_owned(),
        "proj".to_owned(),
        "subdir".to_owned(),
        "main".to_owned(),
    );

    let mut snap_a = sample_snapshot(1);
    snap_a.state_key = key_a.clone();
    let mut snap_b = sample_snapshot(1);
    snap_b.state_key = key_b.clone();

    store.save_revision(&snap_a).expect("save a");
    store.save_revision(&snap_b).expect("save b");

    store.delete_all(&key_a).expect("delete a");

    assert!(store.load_latest(&key_a).expect("load a").is_none());
    assert!(store.load_latest(&key_b).expect("load b").is_some());
}

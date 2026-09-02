//! Comprehensive integration test matrix for snapshot behavior.
//!
//! Covers the snapshot runtime contract:
//!   - timed / event / command-change / manual save triggers
//!   - main / worktree / subdir snapshot isolation
//!   - disabled full short-circuit
//!   - migration (V1 -> V3, V2 -> V3, V3 roundtrip)
//!   - transactional restore + rollback
//!   - doctor / status / history / verify

use camino::Utf8PathBuf;
use std::cell::RefCell;
use std::collections::HashMap;

use workhive_core::domain::{
    LaunchCapture, PaneKind, PaneLaunchIntent, PaneLaunchOrigin, PaneObservedState, PaneRef,
    PaneSnapshot, PaneState, RestoreOutcome, RestorePolicy, RuntimeBackendId,
    SNAPSHOT_SCHEMA_VERSION, SchemaVersion, SnapshotEvent, SnapshotEventKind, SnapshotHealthCheck,
    SnapshotHistoryEntry, SnapshotManifest, SnapshotMetadata, SnapshotRevisionId, SnapshotSource,
    SupervisorLock, ToolName, UnitId, WindowSnapshot, WorkspaceSnapshot, WorkspaceStateKey,
    WorkspaceTopology,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::{
    LEGACY_SNAPSHOT_SCHEMA_VERSION, LegacySnapshotDocument as SnapshotDocument,
    LegacySnapshotIdentity as SnapshotIdentity, LegacySnapshotMeta as SnapshotMeta,
    LegacySnapshotUnit as SnapshotUnit, RuntimePaneSnapshot, RuntimeTopologySnapshot,
    RuntimeUnitSnapshot, RuntimeWindowSnapshot, SnapshotConfig, SnapshotStorePort,
};
use workhive_core::usecases::{
    snapshot_capture::{self, CaptureContext},
    snapshot_migration, snapshot_restore,
};

/// Minimal in-memory fake for integration tests (avoids circular crate dep).
struct InMemoryStore {
    revisions: RefCell<HashMap<String, Vec<WorkspaceSnapshot>>>,
    events: RefCell<HashMap<String, Vec<SnapshotEvent>>>,
    manifests: RefCell<HashMap<String, SnapshotManifest>>,
    locks: RefCell<HashMap<String, SupervisorLock>>,
}

impl InMemoryStore {
    fn new() -> Self {
        Self {
            revisions: RefCell::new(HashMap::new()),
            events: RefCell::new(HashMap::new()),
            manifests: RefCell::new(HashMap::new()),
            locks: RefCell::new(HashMap::new()),
        }
    }
    fn key(sk: &WorkspaceStateKey) -> String {
        sk.to_file_key()
    }
}

impl SnapshotStorePort for InMemoryStore {
    fn save_revision(&self, s: &WorkspaceSnapshot) -> Result<SnapshotRevisionId, SnapshotError> {
        self.revisions
            .borrow_mut()
            .entry(Self::key(&s.state_key))
            .or_default()
            .push(s.clone());
        Ok(s.revision_id)
    }
    fn load_latest(
        &self,
        sk: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        Ok(self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .and_then(|v| v.last().cloned()))
    }
    fn load_revision(
        &self,
        sk: &WorkspaceStateKey,
        rid: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        Ok(self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .and_then(|v| v.iter().find(|s| s.revision_id == rid).cloned()))
    }
    fn append_events(
        &self,
        sk: &WorkspaceStateKey,
        evts: &[SnapshotEvent],
    ) -> Result<(), SnapshotError> {
        self.events
            .borrow_mut()
            .entry(Self::key(sk))
            .or_default()
            .extend(evts.iter().cloned());
        Ok(())
    }
    fn load_events_since(
        &self,
        sk: &WorkspaceStateKey,
        _since: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
        Ok(self
            .events
            .borrow()
            .get(&Self::key(sk))
            .cloned()
            .unwrap_or_default())
    }
    fn read_manifest(
        &self,
        sk: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError> {
        Ok(self.manifests.borrow().get(&Self::key(sk)).cloned())
    }
    fn write_manifest(&self, m: &SnapshotManifest) -> Result<(), SnapshotError> {
        self.manifests
            .borrow_mut()
            .insert(Self::key(&m.state_key), m.clone());
        Ok(())
    }
    fn list_history(
        &self,
        sk: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
        Ok(self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .map(|v| {
                v.iter()
                    .map(|s| SnapshotHistoryEntry {
                        revision_id: s.revision_id,
                        captured_at: s.captured_at.clone(),
                        source: s.source.clone(),
                        pane_count: s.panes.len(),
                        window_count: s.topology.windows.len(),
                        content_hash: s.metadata.content_hash.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }
    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError> {
        self.locks
            .borrow_mut()
            .insert(Self::key(&lock.state_key), lock.clone());
        Ok(true)
    }
    fn release_lock(&self, sk: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        self.locks.borrow_mut().remove(&Self::key(sk));
        Ok(())
    }
    fn update_heartbeat(&self, sk: &WorkspaceStateKey, hb: &str) -> Result<(), SnapshotError> {
        if let Some(l) = self.locks.borrow_mut().get_mut(&Self::key(sk)) {
            l.heartbeat = hb.to_owned();
            Ok(())
        } else {
            Err(SnapshotError::LockConflict {
                state_key: sk.to_string(),
                holder_pid: 0,
            })
        }
    }
    fn read_lock(&self, sk: &WorkspaceStateKey) -> Result<Option<SupervisorLock>, SnapshotError> {
        Ok(self.locks.borrow().get(&Self::key(sk)).cloned())
    }
    fn health_check(&self, sk: &WorkspaceStateKey) -> Result<SnapshotHealthCheck, SnapshotError> {
        let has = self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .is_some_and(|r| !r.is_empty());
        let latest = self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .and_then(|r| r.last().map(|s| s.revision_id));
        Ok(SnapshotHealthCheck {
            enabled: true,
            supervisor_running: false,
            hooks_registered: false,
            latest_save_time: None,
            latest_revision: latest,
            manifest_readable: self.manifests.borrow().contains_key(&Self::key(sk)),
            checksum_valid: if has { Some(true) } else { None },
            state_key_match: true,
        })
    }
    fn verify_revision(
        &self,
        sk: &WorkspaceStateKey,
        rid: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError> {
        Ok(self
            .revisions
            .borrow()
            .get(&Self::key(sk))
            .is_some_and(|r| r.iter().any(|s| s.revision_id == rid)))
    }
    fn apply_retention(
        &self,
        sk: &WorkspaceStateKey,
        retain: u32,
        _days: u32,
    ) -> Result<u32, SnapshotError> {
        let k = Self::key(sk);
        let mut m = self.revisions.borrow_mut();
        let v = m.entry(k).or_default();
        let rm = v.len().saturating_sub(retain as usize);
        *v = v.split_off(rm);
        Ok(rm as u32)
    }
    fn delete_all(&self, sk: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        let k = Self::key(sk);
        self.revisions.borrow_mut().remove(&k);
        self.events.borrow_mut().remove(&k);
        self.manifests.borrow_mut().remove(&k);
        self.locks.borrow_mut().remove(&k);
        Ok(())
    }
    fn delete_all_global(&self) -> Result<(), SnapshotError> {
        self.revisions.borrow_mut().clear();
        self.events.borrow_mut().clear();
        self.manifests.borrow_mut().clear();
        self.locks.borrow_mut().clear();
        Ok(())
    }

    fn save_launch_baseline(
        &self,
        _baseline: &workhive_core::domain::LaunchBaseline,
    ) -> Result<(), SnapshotError> {
        Ok(())
    }

    fn load_launch_baseline(
        &self,
        _state_key: &WorkspaceStateKey,
    ) -> Result<Option<workhive_core::domain::LaunchBaseline>, SnapshotError> {
        Ok(None)
    }
}

// ───────────────────────────────────────────────────────────
//  Helpers
// ───────────────────────────────────────────────────────────

fn make_state_key(project: &str, subdir: &str, workspace: &str) -> WorkspaceStateKey {
    WorkspaceStateKey::new(
        "inst-001".to_owned(),
        project.to_owned(),
        subdir.to_owned(),
        workspace.to_owned(),
    )
}

fn make_units(count: usize) -> Vec<RuntimeUnitSnapshot> {
    (0..count)
        .map(|i| RuntimeUnitSnapshot {
            id: UnitId::new(format!("u{i}")),
            index: i as u32,
            cwd: Utf8PathBuf::from(format!("/tmp/proj/pane{i}")),
            command: if i == 0 { "zsh" } else { "lazygit" }.to_owned(),
            active: i == 0,
            kind: if i == 0 {
                PaneKind::Terminal
            } else {
                PaneKind::GitUi
            },
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        })
        .collect()
}

fn launch_origin_from_kind(kind: &PaneKind, command: &str) -> Option<PaneLaunchOrigin> {
    match kind {
        PaneKind::Terminal => Some(PaneLaunchOrigin::EmptyShell),
        PaneKind::Agent | PaneKind::GitUi => ToolName::new(command.to_lowercase())
            .ok()
            .map(|name| PaneLaunchOrigin::Tool { name }),
        PaneKind::Unknown => None,
    }
}

fn make_topology_from_units(units: &[RuntimeUnitSnapshot]) -> RuntimeTopologySnapshot {
    let panes = units
        .iter()
        .map(|unit| RuntimePaneSnapshot {
            unit_id: unit.id.clone(),
            pane_id: Some(format!("%{}", unit.id.as_str())),
            window_index: 0,
            pane_index: unit.index,
            cwd: unit.cwd.clone(),
            title: None,
            current_command: Some(unit.command.clone()),
            start_command: Some(unit.command.clone()),
            active: unit.active,
            dead: matches!(unit.state, PaneState::Unknown),
            launch_origin: launch_origin_from_kind(&unit.kind, &unit.command),
            managed: true,
            tty: unit.tty.clone(),
            process_pid: unit.process_pid,
        })
        .collect::<Vec<_>>();

    RuntimeTopologySnapshot {
        windows: vec![RuntimeWindowSnapshot {
            index: 0,
            name: None,
            layout: "tiled".to_owned(),
            active: true,
        }],
        panes,
        active_window: Some(0),
        active_pane_runtime_id: units
            .iter()
            .find(|unit| unit.active)
            .map(|u| u.id.to_string()),
    }
}

fn capture_snapshot_from_units(
    ctx: &CaptureContext,
    units: &[RuntimeUnitSnapshot],
) -> Result<WorkspaceSnapshot, SnapshotError> {
    let topology = make_topology_from_units(units);
    snapshot_capture::build_snapshot_from_topology(ctx, &topology)
}

fn make_capture_ctx(source: SnapshotSource) -> CaptureContext {
    CaptureContext {
        state_key: make_state_key("proj", "", "main"),
        runtime_backend: RuntimeBackendId::tmux(),
        platform: "macos".to_owned(),
        captured_at: "2026-04-24T12:00:00Z".to_owned(),
        source,
        next_revision_id: SnapshotRevisionId(1),
        config: SnapshotConfig::default(),
    }
}

fn make_snapshot(state_key: WorkspaceStateKey, rev: u64) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(rev),
        state_key,
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
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            generation: rev,
            content_hash: None,
            config_fingerprint: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════
//  1. SAVE TRIGGERS
// ═══════════════════════════════════════════════════════════

#[test]
fn timed_save_produces_valid_snapshot() {
    let ctx = make_capture_ctx(SnapshotSource::TimedSave);
    let snap = capture_snapshot_from_units(&ctx, &make_units(2)).unwrap();
    assert_eq!(snap.source, SnapshotSource::TimedSave);
    assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snap.panes.len(), 2);
}

#[test]
fn event_save_produces_valid_snapshot() {
    let ctx = make_capture_ctx(SnapshotSource::EventSave);
    let snap = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap();
    assert_eq!(snap.source, SnapshotSource::EventSave);
}

#[test]
fn command_change_save() {
    let ctx = make_capture_ctx(SnapshotSource::CommandChangeSave);
    let snap = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap();
    assert_eq!(snap.source, SnapshotSource::CommandChangeSave);
}

#[test]
fn manual_save() {
    let ctx = make_capture_ctx(SnapshotSource::ManualSave);
    let snap = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap();
    assert_eq!(snap.source, SnapshotSource::ManualSave);
}

#[test]
fn session_closing_save() {
    let ctx = make_capture_ctx(SnapshotSource::SessionClosing);
    let snap = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap();
    assert_eq!(snap.source, SnapshotSource::SessionClosing);
}

// ═══════════════════════════════════════════════════════════
//  2. SNAPSHOT ISOLATION (main / worktree / subdir)
// ═══════════════════════════════════════════════════════════

#[test]
fn main_branch_isolation() {
    let store = InMemoryStore::new();
    let key_main = make_state_key("proj", "", "main");
    let key_feat = make_state_key("proj", "", "feat-x");

    store
        .save_revision(&make_snapshot(key_main.clone(), 1))
        .unwrap();
    store
        .save_revision(&make_snapshot(key_feat.clone(), 2))
        .unwrap();

    let main_snap = store.load_latest(&key_main).unwrap().unwrap();
    let feat_snap = store.load_latest(&key_feat).unwrap().unwrap();

    assert_eq!(main_snap.revision_id, SnapshotRevisionId(1));
    assert_eq!(feat_snap.revision_id, SnapshotRevisionId(2));
    assert_ne!(main_snap.state_key, feat_snap.state_key);
}

#[test]
fn worktree_isolation() {
    let store = InMemoryStore::new();
    let key_main = make_state_key("proj", "", "main");
    let key_wt = make_state_key("proj", "worktree-1", "feat-y");

    store
        .save_revision(&make_snapshot(key_main.clone(), 1))
        .unwrap();
    store
        .save_revision(&make_snapshot(key_wt.clone(), 2))
        .unwrap();

    assert!(store.load_latest(&key_main).unwrap().is_some());
    assert!(store.load_latest(&key_wt).unwrap().is_some());
    assert_ne!(
        store.load_latest(&key_main).unwrap().unwrap().state_key,
        store.load_latest(&key_wt).unwrap().unwrap().state_key
    );
}

#[test]
fn subdir_isolation() {
    let store = InMemoryStore::new();
    let key_root = make_state_key("proj", "", "main");
    let key_sub = make_state_key("proj", "packages/api", "main");

    store
        .save_revision(&make_snapshot(key_root.clone(), 1))
        .unwrap();
    store
        .save_revision(&make_snapshot(key_sub.clone(), 2))
        .unwrap();

    let root_snap = store.load_latest(&key_root).unwrap().unwrap();
    let sub_snap = store.load_latest(&key_sub).unwrap().unwrap();

    assert_ne!(root_snap.state_key, sub_snap.state_key);
    assert_eq!(root_snap.revision_id, SnapshotRevisionId(1));
    assert_eq!(sub_snap.revision_id, SnapshotRevisionId(2));
}

// ═══════════════════════════════════════════════════════════
//  3. DISABLED SHORT-CIRCUIT
// ═══════════════════════════════════════════════════════════

#[test]
fn disabled_snapshot_short_circuits() {
    let mut ctx = make_capture_ctx(SnapshotSource::TimedSave);
    ctx.config.enabled = false;

    let err = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap_err();
    assert!(matches!(err, SnapshotError::Disabled));
}

#[test]
fn enabled_snapshot_succeeds() {
    let mut ctx = make_capture_ctx(SnapshotSource::TimedSave);
    ctx.config.enabled = true;

    let snap = capture_snapshot_from_units(&ctx, &make_units(1)).unwrap();
    assert_eq!(snap.panes.len(), 1);
}

// ═══════════════════════════════════════════════════════════
//  4. MIGRATION
// ═══════════════════════════════════════════════════════════

fn v2_json() -> String {
    let doc = SnapshotDocument {
        schema_version: LEGACY_SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "a230".to_owned(),
            project: "proj".to_owned(),
            workspace: "feat-a".to_owned(),
            server_name: "whsrv-a230-proj".to_owned(),
            session_name: "wh@a230@proj@feat-a".to_owned(),
            state_key: Some("a230@proj@feat-a".to_owned()),
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-20T11:30:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![
            SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: "/tmp/proj".into(),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "zsh".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            },
            SnapshotUnit {
                index: 1,
                active: false,
                title: Some("git".to_owned()),
                cwd: "/tmp/proj".into(),
                kind: None,
                role: None,
                launch: LaunchCapture::ToolRef {
                    tool_name: "lazygit".to_owned(),
                },
                state: Some("running".to_owned()),
                last_error: None,
                updated_at: None,
            },
        ],
    };
    serde_json::to_string_pretty(&doc).unwrap()
}

fn v1_json() -> String {
    let doc = SnapshotDocument {
        schema_version: 1,
        identity: SnapshotIdentity {
            instance_id: "b340".to_owned(),
            project: "old-proj".to_owned(),
            workspace: "main".to_owned(),
            server_name: "whsrv-b340-old-proj".to_owned(),
            session_name: "wh@b340@old-proj@main".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "linux".to_owned(),
            captured_at: "2026-01-01T00:00:00Z".to_owned(),
            generation: 0,
            config_fingerprint: None,
        },
        units: vec![SnapshotUnit {
            index: 0,
            active: true,
            title: None,
            cwd: "/home/user/proj".into(),
            kind: None,
            role: None,
            launch: LaunchCapture::Shell {
                executable: "bash".to_owned(),
                args: vec![],
            },
            state: None,
            last_error: None,
            updated_at: None,
        }],
    };
    serde_json::to_string_pretty(&doc).unwrap()
}

#[test]
fn migration_legacy_snapshot_to_current_schema() {
    let (snap, outcome) = snapshot_migration::migrate_to_current_schema(&v2_json()).unwrap();
    assert!(matches!(
        outcome,
        workhive_core::domain::MigrationOutcome::Migrated {
            from: SchemaVersion::V2,
            to: SchemaVersion::V3
        }
    ));
    assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snap.panes.len(), 2);
    assert!(matches!(
        snap.panes[0].launch_intent,
        PaneLaunchIntent::EmptyShell
    ));
    assert!(matches!(
        snap.panes[1].launch_intent,
        PaneLaunchIntent::ToolRef { .. }
    ));
}

#[test]
fn migration_initial_snapshot_to_current_schema() {
    let (snap, outcome) = snapshot_migration::migrate_to_current_schema(&v1_json()).unwrap();
    assert!(matches!(
        outcome,
        workhive_core::domain::MigrationOutcome::Migrated {
            from: SchemaVersion::V1,
            to: SchemaVersion::V3
        }
    ));
    assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snap.panes.len(), 1);
}

#[test]
fn migration_current_schema_roundtrip() {
    let (snap, _) = snapshot_migration::migrate_to_current_schema(&v2_json()).unwrap();
    let current_json = serde_json::to_string_pretty(&snap).unwrap();
    let (snap2, outcome) = snapshot_migration::migrate_to_current_schema(&current_json).unwrap();
    assert!(matches!(
        outcome,
        workhive_core::domain::MigrationOutcome::AlreadyCurrent
    ));
    assert_eq!(snap2.panes.len(), snap.panes.len());
}

#[test]
fn migration_unknown_version_errors() {
    let bad_json = r#"{"schema_version": 99}"#;
    let err = snapshot_migration::detect_schema_version(bad_json).unwrap_err();
    assert!(matches!(
        err,
        SnapshotError::IncompatibleVersion { version: 99, .. }
    ));
}

// ═══════════════════════════════════════════════════════════
//  5. TRANSACTIONAL RESTORE + ROLLBACK
// ═══════════════════════════════════════════════════════════

#[test]
fn restore_plan_with_mixed_policies() {
    let snap = make_snapshot_with_policies();
    let plan = snapshot_restore::build_restore_plan(&snap, false).unwrap();
    assert!(!plan.dry_run);
    assert_eq!(plan.window_actions.len(), 1);

    let outcome = snapshot_restore::summarize_plan(&plan);
    match outcome {
        RestoreOutcome::Success {
            panes_restored,
            windows_restored,
            ..
        } => {
            assert_eq!(windows_restored, 1);
            assert!(panes_restored >= 1);
        }
        _ => panic!("expected Success outcome"),
    }
}

#[test]
fn restore_dry_run_produces_no_side_effects() {
    let snap = make_snapshot_with_policies();
    let plan = snapshot_restore::build_restore_plan(&snap, true).unwrap();
    assert!(plan.dry_run);
    let outcome = snapshot_restore::summarize_plan(&plan);
    assert!(matches!(outcome, RestoreOutcome::DryRun { .. }));
}

#[test]
fn restore_empty_plan_rejected() {
    let snap = WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: make_state_key("proj", "", "main"),
        captured_at: "2026-04-24T12:00:00Z".to_owned(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![],
            active_window: None,
            active_pane_runtime_id: None,
        },
        panes: vec![],
        metadata: SnapshotMetadata {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    };
    let plan = snapshot_restore::build_restore_plan(&snap, false).unwrap();
    let err = snapshot_restore::validate_restore_plan(&plan).unwrap_err();
    assert!(matches!(err, SnapshotError::RestoreFailed { .. }));
}

#[test]
fn restore_non_restorable_pane_degrades_to_shell() {
    let snap = make_snapshot_with_policies();
    let plan = snapshot_restore::build_restore_plan(&snap, false).unwrap();

    let pane_actions = match &plan.window_actions[0] {
        workhive_core::domain::WindowRestoreAction::Create { pane_actions, .. } => pane_actions,
    };

    let shell_only = pane_actions
        .iter()
        .filter(|a| {
            matches!(
                a,
                workhive_core::domain::PaneRestoreAction::Create {
                    launch_intent: PaneLaunchIntent::EmptyShell,
                    ..
                }
            )
        })
        .count();

    assert!(
        shell_only >= 1,
        "at least one pane should degrade to EmptyShell"
    );
}

fn make_snapshot_with_policies() -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId(1),
        state_key: make_state_key("proj", "", "main"),
        captured_at: "2026-04-24T12:00:00Z".to_owned(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows: vec![WindowSnapshot {
                index: 0,
                name: None,
                layout: "tiled".to_owned(),
                panes: vec![
                    PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 1,
                        window_index: 0,
                    },
                    PaneRef {
                        pane_index: 2,
                        window_index: 0,
                    },
                ],
            }],
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes: vec![
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::EmptyShell,
                observed: PaneObservedState {
                    current_command: Some("zsh".to_owned()),
                    start_command: None,
                    active: true,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 1,
                pane_id: None,
                title: Some("git".to_owned()),
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::ToolRef {
                    tool_name: "lazygit".to_owned(),
                },
                observed: PaneObservedState {
                    current_command: Some("lazygit".to_owned()),
                    start_command: None,
                    active: false,
                    dead: false,
                },
            },
            PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 2,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Skip,
                launch_intent: PaneLaunchIntent::NonRestorable {
                    display: "ssh".to_owned(),
                    reason: "remote session".to_owned(),
                },
                observed: PaneObservedState {
                    current_command: None,
                    start_command: None,
                    active: false,
                    dead: true,
                },
            },
        ],
        metadata: SnapshotMetadata {
            runtime_backend: RuntimeBackendId::tmux().to_string(),
            platform: "macos".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════
//  6. STORE: HISTORY / VERIFY / HEALTH / RETENTION
// ═══════════════════════════════════════════════════════════

#[test]
fn store_history_returns_chronological_entries() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    for i in 1..=3 {
        store.save_revision(&make_snapshot(key.clone(), i)).unwrap();
    }

    let history = store.list_history(&key).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].revision_id, SnapshotRevisionId(1));
    assert_eq!(history[2].revision_id, SnapshotRevisionId(3));
}

#[test]
fn store_verify_existing_revision() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");
    store
        .save_revision(&make_snapshot(key.clone(), 42))
        .unwrap();

    assert!(store.verify_revision(&key, SnapshotRevisionId(42)).unwrap());
    assert!(!store.verify_revision(&key, SnapshotRevisionId(99)).unwrap());
}

#[test]
fn store_health_check() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    let health = store.health_check(&key).unwrap();
    assert!(!health.manifest_readable);
    assert!(health.checksum_valid.is_none());

    store.save_revision(&make_snapshot(key.clone(), 1)).unwrap();
    let manifest = SnapshotManifest {
        state_key: key.clone(),
        head_revision: SnapshotRevisionId(1),
        total_revisions: 1,
        events_since_full: 0,
        last_saved_at: "2026-04-24T12:01:00Z".to_owned(),
        last_full_revision_at: "2026-04-24T12:01:00Z".to_owned(),
    };
    store.write_manifest(&manifest).unwrap();

    let health = store.health_check(&key).unwrap();
    assert!(health.manifest_readable);
    assert_eq!(health.latest_revision, Some(SnapshotRevisionId(1)));
    assert_eq!(health.checksum_valid, Some(true));
}

#[test]
fn store_retention_prunes_old_revisions() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    for i in 1..=10 {
        store.save_revision(&make_snapshot(key.clone(), i)).unwrap();
    }

    let removed = store.apply_retention(&key, 3, 30).unwrap();
    assert_eq!(removed, 7);

    let history = store.list_history(&key).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].revision_id, SnapshotRevisionId(8));
}

#[test]
fn store_delete_all_removes_everything() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    store.save_revision(&make_snapshot(key.clone(), 1)).unwrap();
    store.delete_all(&key).unwrap();

    assert!(store.load_latest(&key).unwrap().is_none());
    assert!(store.list_history(&key).unwrap().is_empty());
}

// ═══════════════════════════════════════════════════════════
//  7. SUPERVISOR LOCK
// ═══════════════════════════════════════════════════════════

#[test]
fn supervisor_lock_acquire_release() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    let lock = SupervisorLock {
        pid: 12345,
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: key.clone(),
    };

    assert!(store.acquire_lock(&lock).unwrap());
    assert!(store.read_lock(&key).unwrap().is_some());

    store.release_lock(&key).unwrap();
    assert!(store.read_lock(&key).unwrap().is_none());
}

#[test]
fn supervisor_heartbeat_update() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    let lock = SupervisorLock {
        pid: 12345,
        heartbeat: "2026-04-24T12:00:00Z".to_owned(),
        state_key: key.clone(),
    };
    store.acquire_lock(&lock).unwrap();

    store
        .update_heartbeat(&key, "2026-04-24T12:05:00Z")
        .unwrap();
    let updated = store.read_lock(&key).unwrap().unwrap();
    assert_eq!(updated.heartbeat, "2026-04-24T12:05:00Z");
}

#[test]
fn heartbeat_without_lock_errors() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    let err = store
        .update_heartbeat(&key, "2026-04-24T12:05:00Z")
        .unwrap_err();
    assert!(matches!(err, SnapshotError::LockConflict { .. }));
}

// ═══════════════════════════════════════════════════════════
//  8. CONTENT HASH
// ═══════════════════════════════════════════════════════════

#[test]
fn content_hash_deterministic() {
    let snap = make_snapshot(make_state_key("proj", "", "main"), 1);
    let h1 = snapshot_capture::compute_content_hash(&snap).unwrap();
    let h2 = snapshot_capture::compute_content_hash(&snap).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn content_hash_changes_on_mutation() {
    let snap1 = make_snapshot(make_state_key("proj", "", "main"), 1);
    let h1 = snapshot_capture::compute_content_hash(&snap1).unwrap();

    let mut snap2 = snap1;
    snap2.panes[0].cwd = Utf8PathBuf::from("/different/path");
    let h2 = snapshot_capture::compute_content_hash(&snap2).unwrap();
    assert_ne!(h1, h2);
}

// ═══════════════════════════════════════════════════════════
//  9. LAUNCH INTENT DERIVATION
// ═══════════════════════════════════════════════════════════

#[test]
fn terminal_pane_gets_empty_shell_intent() {
    let ctx = make_capture_ctx(SnapshotSource::ManualSave);
    let units = vec![RuntimeUnitSnapshot {
        id: UnitId::new("u0"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "zsh".to_owned(),
        active: true,
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }];
    let snap = capture_snapshot_from_units(&ctx, &units).unwrap();
    assert!(matches!(
        snap.panes[0].launch_intent,
        PaneLaunchIntent::EmptyShell
    ));
}

#[test]
fn agent_pane_gets_tool_ref_intent() {
    let ctx = make_capture_ctx(SnapshotSource::ManualSave);
    let units = vec![RuntimeUnitSnapshot {
        id: UnitId::new("u0"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "claude".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Running,
        tty: None,
        process_pid: None,
    }];
    let snap = capture_snapshot_from_units(&ctx, &units).unwrap();
    assert!(matches!(
        snap.panes[0].launch_intent,
        PaneLaunchIntent::ToolRef { .. }
    ));
}

#[test]
fn unknown_pane_with_shell_command_gets_empty_shell() {
    let ctx = make_capture_ctx(SnapshotSource::ManualSave);
    let units = vec![RuntimeUnitSnapshot {
        id: UnitId::new("u0"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "fish".to_owned(),
        active: true,
        kind: PaneKind::Unknown,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }];
    let snap = capture_snapshot_from_units(&ctx, &units).unwrap();
    assert!(matches!(
        snap.panes[0].launch_intent,
        PaneLaunchIntent::EmptyShell
    ));
}

#[test]
fn unknown_pane_with_non_shell_gets_non_restorable() {
    let ctx = make_capture_ctx(SnapshotSource::ManualSave);
    let units = vec![RuntimeUnitSnapshot {
        id: UnitId::new("u0"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "ssh".to_owned(),
        active: false,
        kind: PaneKind::Unknown,
        state: PaneState::Running,
        tty: None,
        process_pid: None,
    }];
    let snap = capture_snapshot_from_units(&ctx, &units).unwrap();
    assert!(matches!(
        snap.panes[0].launch_intent,
        PaneLaunchIntent::NonRestorable { .. }
    ));
    assert_eq!(snap.panes[0].restore_policy, RestorePolicy::ShellOnly);
}

// ═══════════════════════════════════════════════════════════
//  10. EVENTS
// ═══════════════════════════════════════════════════════════

#[test]
fn event_append_and_load() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    let events = vec![
        SnapshotEvent {
            timestamp: "2026-04-24T12:00:00Z".to_owned(),
            revision_id: None,
            kind: SnapshotEventKind::PaneAdded,
            detail: Some("pane 0 split".to_owned()),
        },
        SnapshotEvent {
            timestamp: "2026-04-24T12:01:00Z".to_owned(),
            revision_id: None,
            kind: SnapshotEventKind::PaneCommandChanged,
            detail: None,
        },
    ];

    store.append_events(&key, &events).unwrap();
    let loaded = store
        .load_events_since(&key, SnapshotRevisionId(0))
        .unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].kind, SnapshotEventKind::PaneAdded);
}

// ═══════════════════════════════════════════════════════════
//  11. END-TO-END: CAPTURE -> STORE -> RESTORE
// ═══════════════════════════════════════════════════════════

#[test]
fn e2e_capture_store_restore() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    // Capture
    let mut ctx = make_capture_ctx(SnapshotSource::ManualSave);
    ctx.state_key = key.clone();
    let snap = capture_snapshot_from_units(&ctx, &make_units(3)).unwrap();

    // Store
    store.save_revision(&snap).unwrap();

    // Load
    let loaded = store.load_latest(&key).unwrap().unwrap();
    assert_eq!(loaded.panes.len(), 3);

    // Build restore plan
    let plan = snapshot_restore::build_restore_plan(&loaded, false).unwrap();
    snapshot_restore::validate_restore_plan(&plan).unwrap();

    let outcome = snapshot_restore::summarize_plan(&plan);
    assert!(matches!(outcome, RestoreOutcome::Success { .. }));
}

#[test]
fn e2e_multi_revision_history() {
    let store = InMemoryStore::new();
    let key = make_state_key("proj", "", "main");

    for i in 1..=5 {
        let mut ctx = make_capture_ctx(SnapshotSource::TimedSave);
        ctx.state_key = key.clone();
        ctx.next_revision_id = SnapshotRevisionId(i);
        ctx.captured_at = format!("2026-04-24T12:{i:02}:00Z");
        let snap = capture_snapshot_from_units(&ctx, &make_units(2)).unwrap();
        store.save_revision(&snap).unwrap();
    }

    let history = store.list_history(&key).unwrap();
    assert_eq!(history.len(), 5);
    assert_eq!(history[0].revision_id, SnapshotRevisionId(1));
    assert_eq!(history[4].revision_id, SnapshotRevisionId(5));

    let latest = store.load_latest(&key).unwrap().unwrap();
    assert_eq!(latest.revision_id, SnapshotRevisionId(5));

    let rev2 = store
        .load_revision(&key, SnapshotRevisionId(2))
        .unwrap()
        .unwrap();
    assert_eq!(rev2.revision_id, SnapshotRevisionId(2));
}

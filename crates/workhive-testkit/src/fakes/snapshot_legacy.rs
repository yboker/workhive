//! Fake snapshot adapter for testing.

use std::cell::RefCell;

use workhive_core::domain::{
    LaunchBaseline, LaunchCapture, PaneLaunchIntent, PaneObservedState, PaneRef, PaneSnapshot,
    RestorePolicy, SNAPSHOT_SCHEMA_VERSION, SnapshotEvent, SnapshotHealthCheck,
    SnapshotHistoryEntry, SnapshotManifest, SnapshotMetadata as DomainSnapshotMetadata,
    SnapshotRevisionId, SnapshotSource, SupervisorLock, WindowSnapshot, WorkspaceId,
    WorkspaceSnapshot, WorkspaceStateKey, WorkspaceTopology,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::{
    LEGACY_SNAPSHOT_SCHEMA_VERSION, LegacySnapshotDocument as SnapshotDocument,
    LegacySnapshotIdentity as SnapshotIdentity, LegacySnapshotMeta as SnapshotMeta,
    LegacySnapshotMetadata as SnapshotMetadata, LegacySnapshotPort as SnapshotPort,
    LegacySnapshotUnit as SnapshotUnit, SnapshotStorePort,
};

/// In-memory fake for `SnapshotPort`.
///
/// Supports injecting errors for `delete_for_workspace` via `set_delete_error`.
pub struct FakeSnapshot {
    snapshots: RefCell<Vec<SnapshotDocument>>,
    revisions: RefCell<Vec<WorkspaceSnapshot>>,
    manifests: RefCell<Vec<SnapshotManifest>>,
    events: RefCell<Vec<(String, SnapshotEvent)>>,
    locks: RefCell<Vec<SupervisorLock>>,
    delete_error: RefCell<Option<String>>,
    load_error: RefCell<Option<String>>,
}

impl FakeSnapshot {
    pub fn new() -> Self {
        Self {
            snapshots: RefCell::new(Vec::new()),
            revisions: RefCell::new(Vec::new()),
            manifests: RefCell::new(Vec::new()),
            events: RefCell::new(Vec::new()),
            locks: RefCell::new(Vec::new()),
            delete_error: RefCell::new(None),
            load_error: RefCell::new(None),
        }
    }

    /// Make the next (and all subsequent) `delete_for_workspace` calls return an error.
    pub fn set_delete_error(&self, message: impl Into<String>) {
        *self.delete_error.borrow_mut() = Some(message.into());
    }

    /// Make all load calls return an I/O error (simulates corrupt snapshot on disk).
    pub fn set_load_error(&self, message: impl Into<String>) {
        *self.load_error.borrow_mut() = Some(message.into());
    }

    /// Read-only access to stored snapshots for test assertions.
    pub fn stored_documents(&self) -> Vec<SnapshotDocument> {
        self.snapshots.borrow().clone()
    }

    fn upsert_legacy_snapshot(&self, snapshot: SnapshotDocument) {
        let mut snapshots = self.snapshots.borrow_mut();
        if let Some(state_key) = snapshot.identity.state_key.as_deref() {
            snapshots.retain(|existing| {
                existing.identity.state_key.as_deref() != Some(state_key)
                    && !(existing.identity.state_key.is_none()
                        && existing.identity.workspace == snapshot.identity.workspace)
            });
        } else {
            snapshots.retain(|existing| {
                existing.identity.state_key.is_some()
                    || existing.identity.workspace != snapshot.identity.workspace
            });
        }
        snapshots.push(snapshot);
    }

    fn next_revision_for_state_key(&self, state_key: &WorkspaceStateKey) -> SnapshotRevisionId {
        self.revisions
            .borrow()
            .iter()
            .filter(|snapshot| snapshot.state_key == *state_key)
            .map(|snapshot| snapshot.revision_id)
            .max()
            .map(|revision| revision.next())
            .unwrap_or_else(SnapshotRevisionId::first)
    }

    fn mirror_revision_to_legacy(&self, snapshot: &WorkspaceSnapshot) {
        let doc = SnapshotDocument {
            schema_version: LEGACY_SNAPSHOT_SCHEMA_VERSION,
            identity: SnapshotIdentity {
                instance_id: snapshot.state_key.instance_id.clone(),
                project: snapshot.state_key.project_component.clone(),
                workspace: snapshot.state_key.workspace.clone(),
                server_name: format!(
                    "whsrv-{}-{}",
                    snapshot.state_key.instance_id, snapshot.state_key.project_component
                ),
                session_name: format!(
                    "wh@{}@{}@{}",
                    snapshot.state_key.instance_id,
                    snapshot.state_key.project_component,
                    snapshot.state_key.workspace
                ),
                state_key: Some(snapshot.state_key.to_string()),
            },
            meta: SnapshotMeta {
                runtime_backend: snapshot.metadata.runtime_backend.clone(),
                platform: snapshot.metadata.platform.clone(),
                captured_at: snapshot.captured_at.clone(),
                generation: snapshot.metadata.generation,
                config_fingerprint: snapshot.metadata.config_fingerprint.clone(),
            },
            units: snapshot
                .panes
                .iter()
                .map(|pane| SnapshotUnit {
                    index: pane.pane_index,
                    active: pane.observed.active,
                    title: pane.title.clone(),
                    cwd: pane.cwd.clone(),
                    kind: None,
                    role: None,
                    launch: match &pane.launch_intent {
                        workhive_core::domain::PaneLaunchIntent::ToolRef { tool_name } => {
                            LaunchCapture::ToolRef {
                                tool_name: tool_name.clone(),
                            }
                        }
                        workhive_core::domain::PaneLaunchIntent::InlineCommand {
                            program,
                            args,
                        } => LaunchCapture::Exec {
                            program: program.clone(),
                            args: args.clone(),
                        },
                        workhive_core::domain::PaneLaunchIntent::EmptyShell => {
                            LaunchCapture::Shell {
                                executable: "shell".to_owned(),
                                args: Vec::new(),
                            }
                        }
                        workhive_core::domain::PaneLaunchIntent::NonRestorable {
                            display,
                            reason,
                        } => LaunchCapture::NonRestorable {
                            display: display.clone(),
                            reason: reason.clone(),
                        },
                    },
                    state: Some(if pane.observed.dead {
                        "exited".to_owned()
                    } else {
                        "running".to_owned()
                    }),
                    last_error: None,
                    updated_at: None,
                })
                .collect(),
        };
        self.upsert_legacy_snapshot(doc);
    }
}

impl Default for FakeSnapshot {
    fn default() -> Self {
        Self::new()
    }
}

impl SnapshotPort for FakeSnapshot {
    fn save(&self, snapshot: &SnapshotDocument) -> Result<(), SnapshotError> {
        self.upsert_legacy_snapshot(snapshot.clone());

        let state_key = snapshot
            .identity
            .state_key
            .as_deref()
            .and_then(WorkspaceStateKey::parse)
            .unwrap_or_else(|| {
                WorkspaceStateKey::new(
                    snapshot.identity.instance_id.clone(),
                    snapshot.identity.project.clone(),
                    String::new(),
                    snapshot.identity.workspace.clone(),
                )
            });
        let next_revision = self.next_revision_for_state_key(&state_key);

        let panes = snapshot
            .units
            .iter()
            .map(|unit| {
                let launch_intent = match &unit.launch {
                    LaunchCapture::Shell { .. } => PaneLaunchIntent::EmptyShell,
                    LaunchCapture::ToolRef { tool_name } => PaneLaunchIntent::ToolRef {
                        tool_name: tool_name.clone(),
                    },
                    LaunchCapture::Exec { program, args } => PaneLaunchIntent::InlineCommand {
                        program: program.clone(),
                        args: args.clone(),
                    },
                    LaunchCapture::NonRestorable { display, reason } => {
                        PaneLaunchIntent::NonRestorable {
                            display: display.clone(),
                            reason: reason.clone(),
                        }
                    }
                };
                let restore_policy = if launch_intent.is_restorable() {
                    RestorePolicy::Restore
                } else {
                    RestorePolicy::ShellOnly
                };
                PaneSnapshot {
                    runtime_id: None,
                    window_index: 0,
                    pane_index: unit.index,
                    pane_id: None,
                    title: unit.title.clone(),
                    cwd: unit.cwd.clone(),
                    managed: true,
                    restore_policy,
                    launch_intent,
                    observed: PaneObservedState {
                        current_command: Some(match &unit.launch {
                            LaunchCapture::Shell { executable, .. } => executable.clone(),
                            LaunchCapture::ToolRef { tool_name } => tool_name.clone(),
                            LaunchCapture::Exec { program, .. } => program.clone(),
                            LaunchCapture::NonRestorable { display, .. } => display.clone(),
                        }),
                        start_command: None,
                        active: unit.active,
                        dead: unit.state.as_deref() == Some("exited"),
                    },
                }
            })
            .collect::<Vec<_>>();

        let pane_refs = panes
            .iter()
            .map(|pane| PaneRef {
                pane_index: pane.pane_index,
                window_index: pane.window_index,
            })
            .collect::<Vec<_>>();

        let mirrored = WorkspaceSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            revision_id: next_revision,
            state_key: state_key.clone(),
            captured_at: snapshot.meta.captured_at.clone(),
            source: SnapshotSource::ManualSave,
            topology: WorkspaceTopology {
                windows: vec![WindowSnapshot {
                    index: 0,
                    name: None,
                    layout: "tiled".to_owned(),
                    panes: pane_refs,
                }],
                active_window: Some(0),
                active_pane_runtime_id: None,
            },
            panes,
            metadata: DomainSnapshotMetadata {
                runtime_backend: snapshot.meta.runtime_backend.clone(),
                platform: snapshot.meta.platform.clone(),
                generation: next_revision.0,
                content_hash: None,
                config_fingerprint: snapshot.meta.config_fingerprint.clone(),
            },
        };
        let _ = self.save_revision(&mirrored);
        Ok(())
    }

    fn load_latest_by_state_key(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotDocument>, SnapshotError> {
        if let Some(msg) = self.load_error.borrow().as_ref() {
            return Err(SnapshotError::ReadFailed {
                path: format!("fake/{}", state_key.to_file_key()).into(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, msg.as_str()),
            });
        }
        let expected = state_key.to_string();
        let snapshots = self.snapshots.borrow();
        let exact = snapshots
            .iter()
            .rev()
            .find(|s| {
                s.identity.state_key.as_deref() == Some(expected.as_str())
                    || (s.identity.state_key.is_none()
                        && s.identity.workspace == state_key.workspace)
            })
            .cloned();
        Ok(exact.or_else(|| {
            snapshots
                .iter()
                .rev()
                .find(|s| s.identity.workspace == state_key.workspace)
                .cloned()
        }))
    }

    fn load_latest(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<Option<SnapshotDocument>, SnapshotError> {
        if let Some(msg) = self.load_error.borrow().as_ref() {
            return Err(SnapshotError::ReadFailed {
                path: format!("fake/{}", workspace.as_str()).into(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, msg.as_str()),
            });
        }
        Ok(self
            .snapshots
            .borrow()
            .iter()
            .rev()
            .find(|s| s.identity.workspace == workspace.as_str())
            .cloned())
    }

    fn list(&self) -> Result<Vec<SnapshotMetadata>, SnapshotError> {
        Ok(self
            .snapshots
            .borrow()
            .iter()
            .map(|s| SnapshotMetadata {
                workspace: s.identity.workspace.clone(),
                created_at: "2026-01-01T00:00:00Z".to_owned(),
                path: format!(".workhive/snapshots/{}.json", s.identity.workspace).into(),
            })
            .collect())
    }

    fn delete_for_workspace(&self, workspace: &WorkspaceId) -> Result<(), SnapshotError> {
        if let Some(msg) = self.delete_error.borrow().as_ref() {
            return Err(SnapshotError::DeleteFailed {
                path: format!("fake/{}", workspace.as_str()).into(),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg.as_str()),
            });
        }
        self.snapshots
            .borrow_mut()
            .retain(|s| s.identity.workspace != workspace.as_str());
        Ok(())
    }

    fn delete_by_state_key(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        if let Some(msg) = self.delete_error.borrow().as_ref() {
            return Err(SnapshotError::DeleteFailed {
                path: format!("fake/{}", state_key.to_file_key()).into(),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg.as_str()),
            });
        }
        let expected = state_key.to_string();
        self.snapshots.borrow_mut().retain(|s| {
            s.identity.state_key.as_deref() != Some(expected.as_str())
                && !(s.identity.state_key.is_none() && s.identity.workspace == state_key.workspace)
        });
        Ok(())
    }

    fn delete_all(&self) -> Result<(), SnapshotError> {
        self.snapshots.borrow_mut().clear();
        Ok(())
    }
}

impl SnapshotStorePort for FakeSnapshot {
    fn save_revision(
        &self,
        snapshot: &WorkspaceSnapshot,
    ) -> Result<SnapshotRevisionId, SnapshotError> {
        let mut revisions = self.revisions.borrow_mut();
        revisions.retain(|existing| {
            !(existing.state_key == snapshot.state_key
                && existing.revision_id == snapshot.revision_id)
        });
        revisions.push(snapshot.clone());

        let total_revisions = revisions
            .iter()
            .filter(|existing| existing.state_key == snapshot.state_key)
            .count() as u64;
        drop(revisions);

        let manifest = SnapshotManifest {
            state_key: snapshot.state_key.clone(),
            head_revision: snapshot.revision_id,
            total_revisions,
            events_since_full: 0,
            last_saved_at: snapshot.captured_at.clone(),
            last_full_revision_at: snapshot.captured_at.clone(),
        };
        self.write_manifest(&manifest)?;
        self.mirror_revision_to_legacy(snapshot);
        Ok(snapshot.revision_id)
    }

    fn load_latest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        if let Some(msg) = self.load_error.borrow().as_ref() {
            return Err(SnapshotError::ReadFailed {
                path: format!("fake/{}", state_key.to_file_key()).into(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, msg.as_str()),
            });
        }
        if let Some(manifest) = self.read_manifest(state_key)? {
            return self.load_revision(state_key, manifest.head_revision);
        }
        Ok(self
            .revisions
            .borrow()
            .iter()
            .filter(|snapshot| snapshot.state_key == *state_key)
            .max_by_key(|snapshot| snapshot.revision_id)
            .cloned())
    }

    fn load_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<Option<WorkspaceSnapshot>, SnapshotError> {
        if let Some(msg) = self.load_error.borrow().as_ref() {
            return Err(SnapshotError::ReadFailed {
                path: format!("fake/{}/{}", state_key.to_file_key(), revision_id).into(),
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, msg.as_str()),
            });
        }
        Ok(self
            .revisions
            .borrow()
            .iter()
            .find(|snapshot| {
                snapshot.state_key == *state_key && snapshot.revision_id == revision_id
            })
            .cloned())
    }

    fn append_events(
        &self,
        state_key: &WorkspaceStateKey,
        events: &[SnapshotEvent],
    ) -> Result<(), SnapshotError> {
        let key = state_key.to_string();
        self.events
            .borrow_mut()
            .extend(events.iter().cloned().map(|event| (key.clone(), event)));
        Ok(())
    }

    fn load_events_since(
        &self,
        state_key: &WorkspaceStateKey,
        _since_revision: SnapshotRevisionId,
    ) -> Result<Vec<SnapshotEvent>, SnapshotError> {
        let key = state_key.to_string();
        Ok(self
            .events
            .borrow()
            .iter()
            .filter(|(event_key, _)| event_key == &key)
            .map(|(_, event)| event.clone())
            .collect())
    }

    fn read_manifest(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SnapshotManifest>, SnapshotError> {
        Ok(self
            .manifests
            .borrow()
            .iter()
            .find(|manifest| manifest.state_key == *state_key)
            .cloned())
    }

    fn write_manifest(&self, manifest: &SnapshotManifest) -> Result<(), SnapshotError> {
        let mut manifests = self.manifests.borrow_mut();
        manifests.retain(|existing| existing.state_key != manifest.state_key);
        manifests.push(manifest.clone());
        Ok(())
    }

    fn list_history(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
        let mut history = self
            .revisions
            .borrow()
            .iter()
            .filter(|snapshot| snapshot.state_key == *state_key)
            .map(|snapshot| SnapshotHistoryEntry {
                revision_id: snapshot.revision_id,
                captured_at: snapshot.captured_at.clone(),
                source: snapshot.source.clone(),
                pane_count: snapshot.panes.len(),
                window_count: snapshot.topology.windows.len(),
                content_hash: snapshot.metadata.content_hash.clone(),
            })
            .collect::<Vec<_>>();
        history.sort_by_key(|entry| entry.revision_id);
        Ok(history)
    }

    fn acquire_lock(&self, lock: &SupervisorLock) -> Result<bool, SnapshotError> {
        let mut locks = self.locks.borrow_mut();
        if locks
            .iter()
            .any(|existing| existing.state_key == lock.state_key && existing.pid != lock.pid)
        {
            return Ok(false);
        }
        locks.retain(|existing| existing.state_key != lock.state_key);
        locks.push(lock.clone());
        Ok(true)
    }

    fn release_lock(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        self.locks
            .borrow_mut()
            .retain(|lock| lock.state_key != *state_key);
        Ok(())
    }

    fn update_heartbeat(
        &self,
        state_key: &WorkspaceStateKey,
        heartbeat: &str,
    ) -> Result<(), SnapshotError> {
        if let Some(lock) = self
            .locks
            .borrow_mut()
            .iter_mut()
            .find(|lock| lock.state_key == *state_key)
        {
            lock.heartbeat = heartbeat.to_owned();
            return Ok(());
        }
        Err(SnapshotError::LockConflict {
            state_key: state_key.to_string(),
            holder_pid: 0,
        })
    }

    fn read_lock(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<Option<SupervisorLock>, SnapshotError> {
        Ok(self
            .locks
            .borrow()
            .iter()
            .find(|lock| lock.state_key == *state_key)
            .cloned())
    }

    fn health_check(
        &self,
        state_key: &WorkspaceStateKey,
    ) -> Result<SnapshotHealthCheck, SnapshotError> {
        let manifest = self.read_manifest(state_key)?;
        let latest = self
            .revisions
            .borrow()
            .iter()
            .filter(|snapshot| snapshot.state_key == *state_key)
            .max_by_key(|snapshot| snapshot.revision_id)
            .map(|snapshot| snapshot.revision_id);
        Ok(SnapshotHealthCheck {
            enabled: true,
            supervisor_running: self
                .locks
                .borrow()
                .iter()
                .any(|lock| lock.state_key == *state_key),
            hooks_registered: false,
            latest_save_time: manifest.as_ref().map(|m| m.last_saved_at.clone()),
            latest_revision: latest,
            manifest_readable: manifest.is_some(),
            checksum_valid: Some(true),
            state_key_match: true,
        })
    }

    fn verify_revision(
        &self,
        state_key: &WorkspaceStateKey,
        revision_id: SnapshotRevisionId,
    ) -> Result<bool, SnapshotError> {
        Ok(self.revisions.borrow().iter().any(|snapshot| {
            snapshot.state_key == *state_key && snapshot.revision_id == revision_id
        }))
    }

    fn apply_retention(
        &self,
        state_key: &WorkspaceStateKey,
        retain_revisions: u32,
        _retain_event_days: u32,
    ) -> Result<u32, SnapshotError> {
        let mut revisions = self
            .revisions
            .borrow()
            .iter()
            .filter(|snapshot| snapshot.state_key == *state_key)
            .map(|snapshot| snapshot.revision_id)
            .collect::<Vec<_>>();
        revisions.sort();
        let remove_count = revisions.len().saturating_sub(retain_revisions as usize);
        if remove_count == 0 {
            return Ok(0);
        }
        let to_remove = revisions.into_iter().take(remove_count).collect::<Vec<_>>();
        self.revisions.borrow_mut().retain(|snapshot| {
            !(snapshot.state_key == *state_key && to_remove.contains(&snapshot.revision_id))
        });
        Ok(remove_count as u32)
    }

    fn delete_all(&self, state_key: &WorkspaceStateKey) -> Result<(), SnapshotError> {
        if let Some(msg) = self.delete_error.borrow().as_ref() {
            return Err(SnapshotError::DeleteFailed {
                path: format!("fake/{}", state_key.to_file_key()).into(),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg.as_str()),
            });
        }
        self.revisions
            .borrow_mut()
            .retain(|snapshot| snapshot.state_key != *state_key);
        self.manifests
            .borrow_mut()
            .retain(|manifest| manifest.state_key != *state_key);
        let key = state_key.to_string();
        self.events
            .borrow_mut()
            .retain(|(event_key, _)| event_key != &key);
        self.locks
            .borrow_mut()
            .retain(|lock| lock.state_key != *state_key);
        let expected = state_key.to_string();
        self.snapshots.borrow_mut().retain(|snapshot| {
            snapshot.identity.state_key.as_deref() != Some(expected.as_str())
                && !(snapshot.identity.state_key.is_none()
                    && snapshot.identity.workspace == state_key.workspace)
        });
        Ok(())
    }

    fn delete_all_global(&self) -> Result<(), SnapshotError> {
        if let Some(msg) = self.delete_error.borrow().as_ref() {
            return Err(SnapshotError::DeleteFailed {
                path: "fake/global".into(),
                source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, msg.as_str()),
            });
        }
        self.revisions.borrow_mut().clear();
        self.manifests.borrow_mut().clear();
        self.events.borrow_mut().clear();
        self.locks.borrow_mut().clear();
        self.snapshots.borrow_mut().clear();
        Ok(())
    }

    fn save_launch_baseline(&self, _baseline: &LaunchBaseline) -> Result<(), SnapshotError> {
        Ok(())
    }

    fn load_launch_baseline(
        &self,
        _state_key: &WorkspaceStateKey,
    ) -> Result<Option<LaunchBaseline>, SnapshotError> {
        Ok(None)
    }

    fn set_hooks_registered_state(
        &self,
        _state_key: &WorkspaceStateKey,
        _hooks_registered: bool,
    ) -> Result<(), SnapshotError> {
        Ok(())
    }
}

//! Pane registry synchronization use case.
//!
//! This module defines the sync contract that bridges runtime observations
//! into the pane registry and produces public [`PaneEvent`] values.

use std::collections::HashSet;

use crate::domain::{
    ObservedPaneState, PaneEvent, PaneObservationConfidence, PaneObservationSource, PaneRuntimeId,
    PaneStateChange, UnitId,
};
use crate::ports::{PaneObserverPort, PaneRegistryPort, RegistryError, RuntimeUnitSnapshot};

/// Errors from pane sync operations.
#[derive(Debug, thiserror::Error)]
pub enum PaneSyncError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

/// Result of a pane registry sync cycle.
#[derive(Debug)]
pub struct RegistrySyncResult {
    /// Public pane events produced by this sync cycle.
    pub events: Vec<PaneEvent>,
    /// Number of panes registered during this cycle.
    pub registered: usize,
    /// Number of panes updated during this cycle.
    pub updated: usize,
    /// Number of panes removed during this cycle.
    pub removed: usize,
}

/// Convert a runtime unit snapshot into an observation for registry sync.
///
/// The `group` and `workspace` must be supplied by the caller because
/// [`RuntimeUnitSnapshot`] is scoped to a single workspace query and
/// does not carry routing context.
///
/// Source/confidence are pinned to
/// [`PaneObservationSource::TmuxCurrentCommand`] /
/// [`PaneObservationConfidence::Fallback`] because a
/// [`RuntimeUnitSnapshot`] only carries the tmux runtime layer's view
/// of the pane — there is no supervisor handshake or OS process
/// observer enrichment at this seam. Promoting these to a higher
/// confidence here would let the registry advertise `MessageReceiver`
/// for any pane whose foreground basename happens to be `codex` —
/// which would misclassify ordinary panes. Callers that have
/// supervisor or OS observer signal must build the
/// [`ObservedPaneState`] directly with their stronger source.
pub fn snapshot_to_observation(
    snapshot: &RuntimeUnitSnapshot,
    group: &str,
    workspace: &str,
) -> ObservedPaneState {
    ObservedPaneState {
        group: group.to_owned(),
        workspace: workspace.to_owned(),
        unit_id: UnitId::new(snapshot.id.as_str()),
        cwd: snapshot.cwd.as_str().to_owned(),
        command: snapshot.command.clone(),
        kind: snapshot.kind,
        state: snapshot.state,
        source: PaneObservationSource::TmuxCurrentCommand,
        confidence: PaneObservationConfidence::Fallback,
    }
}

/// Synchronize runtime observations into the pane registry.
///
/// ## Contract
///
/// 1. For each [`ObservedPaneState`], calls `registry.update_from_observation()`
///    to reconcile the observation into the registry, collecting internal
///    [`PaneStateChange`] diffs.
/// 2. Detects panes that are registered but not observed, and removes them.
/// 3. Translates internal diffs into public [`PaneEvent`] values.
/// 4. If an observer is provided, mirrors each event via
///    `observer.push_event()` — this is a **write-only** operation.
/// 5. Returns the produced `Vec<PaneEvent>` as the authoritative output
///    of this sync cycle.
///
/// ## Prohibited
///
/// - This function must **never** call `observer.poll_events()`.
/// - The observer buffer is not consumed by sync. It accumulates across
///   sync cycles until an external consumer drains it.
pub fn sync_pane_registry(
    registry: &dyn PaneRegistryPort,
    observations: Vec<ObservedPaneState>,
    observer: Option<&dyn PaneObserverPort>,
) -> Result<RegistrySyncResult, PaneSyncError> {
    let mut all_changes = Vec::new();
    let mut registered_count: usize = 0;
    let mut updated_count: usize = 0;
    let mut removed_count: usize = 0;

    let mut observed_ids = HashSet::new();
    for obs in observations {
        let id = PaneRuntimeId::derive(&obs.group, &obs.workspace, obs.unit_id.as_str());
        observed_ids.insert(id);

        let changes = registry.update_from_observation(obs)?;
        for change in &changes {
            match change {
                PaneStateChange::Registered { .. } => registered_count += 1,
                PaneStateChange::DescriptorUpdated { .. } => updated_count += 1,
                PaneStateChange::Removed { .. } => removed_count += 1,
            }
        }
        all_changes.extend(changes);
    }

    let current = registry.list(None)?;
    for desc in current {
        if !observed_ids.contains(&desc.runtime_id) {
            registry.remove(&desc.runtime_id)?;
            all_changes.push(PaneStateChange::Removed {
                runtime_id: desc.runtime_id,
            });
            removed_count += 1;
        }
    }

    let events = translate_changes_to_events(&all_changes, registry)?;

    if let Some(obs) = observer {
        for event in &events {
            obs.push_event(event.clone());
        }
    }

    Ok(RegistrySyncResult {
        events,
        registered: registered_count,
        updated: updated_count,
        removed: removed_count,
    })
}

/// Translate internal reconciliation diffs into public pane events.
///
/// This is the explicit mapping boundary between [`PaneStateChange`]
/// (internal, may change across phases) and [`PaneEvent`] (external,
/// stable contract for observers/schedulers).
///
/// Registry errors are propagated — a failed `registry.get()` must not
/// be silently swallowed, as that would produce an incomplete event stream.
fn translate_changes_to_events(
    changes: &[PaneStateChange],
    registry: &dyn PaneRegistryPort,
) -> Result<Vec<PaneEvent>, RegistryError> {
    let mut events = Vec::new();

    for change in changes {
        match change {
            PaneStateChange::Registered { runtime_id } => {
                let descriptor =
                    registry
                        .get(runtime_id)?
                        .ok_or_else(|| RegistryError::PaneNotFound {
                            id: runtime_id.as_str().to_owned(),
                        })?;
                events.push(PaneEvent::Discovered {
                    runtime_id: runtime_id.clone(),
                    descriptor,
                });
            }
            PaneStateChange::DescriptorUpdated {
                runtime_id,
                state_changed,
                capabilities_changed,
            } => {
                if let Some((from, to)) = state_changed {
                    events.push(PaneEvent::StateChanged {
                        runtime_id: runtime_id.clone(),
                        from: *from,
                        to: *to,
                    });
                }
                if let Some((added, removed)) = capabilities_changed
                    && (!added.is_empty() || !removed.is_empty())
                {
                    events.push(PaneEvent::CapabilityChanged {
                        runtime_id: runtime_id.clone(),
                        added: added.clone(),
                        removed: removed.clone(),
                    });
                }
            }
            PaneStateChange::Removed { runtime_id } => {
                events.push(PaneEvent::Removed {
                    runtime_id: runtime_id.clone(),
                });
            }
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PaneLifecycleEvent;
    use crate::domain::{
        PaneCapability, PaneDescriptor, PaneKind, PaneLaunchOrigin, PaneState, UnitId,
        derive_runtime_capabilities, runtime_status_from_state,
    };
    use crate::ports::RegistryDelta;
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    // ── Fake Registry ──

    struct FakeRegistry {
        panes: RefCell<Vec<PaneDescriptor>>,
        lifecycle_events: RefCell<Vec<PaneLifecycleEvent>>,
    }

    impl FakeRegistry {
        fn new() -> Self {
            Self {
                panes: RefCell::new(Vec::new()),
                lifecycle_events: RefCell::new(Vec::new()),
            }
        }
    }

    impl PaneRegistryPort for FakeRegistry {
        fn upsert(&self, descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
            let mut panes = self.panes.borrow_mut();
            if let Some(existing) = panes
                .iter_mut()
                .find(|p| p.runtime_id == descriptor.runtime_id)
            {
                *existing = descriptor;
            } else {
                panes.push(descriptor);
            }
            Ok(RegistryDelta {
                changes: Vec::new(),
            })
        }

        fn remove(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            let mut panes = self.panes.borrow_mut();
            let pos = panes.iter().position(|p| p.runtime_id == *id);
            match pos {
                Some(idx) => Ok(Some(panes.remove(idx))),
                None => Ok(None),
            }
        }

        fn get(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .find(|p| p.runtime_id == *id)
                .cloned())
        }

        fn list_workspace(&self, workspace: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .filter(|p| p.workspace == workspace)
                .cloned()
                .collect())
        }

        fn list(&self, workspace: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
            let panes = self.panes.borrow();
            match workspace {
                Some(ws) => Ok(panes
                    .iter()
                    .filter(|p| p.workspace == ws)
                    .cloned()
                    .collect()),
                None => Ok(panes.clone()),
            }
        }

        fn drain_events(&self) -> Result<Vec<PaneLifecycleEvent>, RegistryError> {
            Ok(self.lifecycle_events.borrow_mut().drain(..).collect())
        }

        fn update_from_observation(
            &self,
            observed: ObservedPaneState,
        ) -> Result<Vec<PaneStateChange>, RegistryError> {
            let id = PaneRuntimeId::derive(
                &observed.group,
                &observed.workspace,
                observed.unit_id.as_str(),
            );
            let new_status = runtime_status_from_state(observed.state);

            let mut panes = self.panes.borrow_mut();
            if let Some(existing) = panes.iter_mut().find(|p| p.runtime_id == id) {
                let state_changed = if existing.state != observed.state {
                    let from = existing.state;
                    existing.state = observed.state;
                    Some((from, observed.state))
                } else {
                    None
                };

                let status_changed = if existing.observed_status != new_status {
                    existing.observed_status = new_status;
                    true
                } else {
                    false
                };

                let new_caps = derive_runtime_capabilities(
                    observed.kind,
                    observed.state,
                    observed.source,
                    observed.confidence,
                );
                let caps_changed = if existing.capabilities != new_caps {
                    let removed: BTreeSet<_> = existing
                        .capabilities
                        .difference(&new_caps)
                        .cloned()
                        .collect();
                    let added: BTreeSet<_> = new_caps
                        .difference(&existing.capabilities)
                        .cloned()
                        .collect();
                    existing.capabilities = new_caps;
                    Some((added, removed))
                } else {
                    None
                };

                existing.cwd = observed.cwd;
                existing.command = observed.command;
                existing.kind = observed.kind;

                if state_changed.is_some() || caps_changed.is_some() || status_changed {
                    Ok(vec![PaneStateChange::DescriptorUpdated {
                        runtime_id: id,
                        state_changed,
                        capabilities_changed: caps_changed,
                    }])
                } else {
                    Ok(vec![])
                }
            } else {
                let caps = derive_runtime_capabilities(
                    observed.kind,
                    observed.state,
                    observed.source,
                    observed.confidence,
                );
                let descriptor = PaneDescriptor {
                    runtime_id: id.clone(),
                    group: observed.group,
                    workspace: observed.workspace,
                    unit_id: observed.unit_id,
                    cwd: observed.cwd,
                    command: observed.command,
                    kind: observed.kind,
                    state: observed.state,
                    observed_status: new_status,
                    role: None,
                    launch_origin: PaneLaunchOrigin::EmptyShell,
                    capabilities: caps,
                };
                panes.push(descriptor);
                Ok(vec![PaneStateChange::Registered { runtime_id: id }])
            }
        }

        fn find_by_capability(
            &self,
            capability: PaneCapability,
        ) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(self
                .panes
                .borrow()
                .iter()
                .filter(|p| p.capabilities.contains(&capability))
                .cloned()
                .collect())
        }
    }

    // ── Fake Observer ──

    struct FakeObserver {
        events: RefCell<Vec<PaneEvent>>,
    }

    impl FakeObserver {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
            }
        }
    }

    impl PaneObserverPort for FakeObserver {
        fn push_event(&self, event: PaneEvent) {
            self.events.borrow_mut().push(event);
        }

        fn poll_events(&self) -> Vec<PaneEvent> {
            self.events.borrow_mut().drain(..).collect()
        }
    }

    fn make_observation(
        group: &str,
        workspace: &str,
        unit_id: &str,
        state: PaneState,
        kind: PaneKind,
    ) -> ObservedPaneState {
        // Default test fixture mimics the tmux-only fallback path
        // produced by `snapshot_to_observation`: no supervisor or OS
        // observer enrichment.
        make_observation_layered(
            group,
            workspace,
            unit_id,
            state,
            kind,
            PaneObservationSource::TmuxCurrentCommand,
            PaneObservationConfidence::Fallback,
        )
    }

    fn make_observation_layered(
        group: &str,
        workspace: &str,
        unit_id: &str,
        state: PaneState,
        kind: PaneKind,
        source: PaneObservationSource,
        confidence: PaneObservationConfidence,
    ) -> ObservedPaneState {
        ObservedPaneState {
            group: group.to_owned(),
            workspace: workspace.to_owned(),
            unit_id: UnitId::new(unit_id),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind,
            state,
            source,
            confidence,
        }
    }

    // ── Behavioral tests ──

    #[test]
    fn sync_discovers_new_pane_and_returns_event() {
        let registry = FakeRegistry::new();
        let obs = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];

        let result = sync_pane_registry(&registry, obs, None).unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.registered, 1);
        match &result.events[0] {
            PaneEvent::Discovered {
                runtime_id,
                descriptor,
            } => {
                assert_eq!(*runtime_id, PaneRuntimeId::derive("g", "ws", "%0"));
                assert_eq!(descriptor.group, "g");
                assert_eq!(descriptor.workspace, "ws");
            }
            other => panic!("expected Discovered, got {:?}", other),
        }
    }

    #[test]
    fn sync_detects_state_change() {
        let registry = FakeRegistry::new();
        let obs1 = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        sync_pane_registry(&registry, obs1, None).unwrap();

        let obs2 = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Running,
            PaneKind::Terminal,
        )];
        let result = sync_pane_registry(&registry, obs2, None).unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(result.updated, 1);
        match &result.events[0] {
            PaneEvent::StateChanged { from, to, .. } => {
                assert_eq!(*from, PaneState::Idle);
                assert_eq!(*to, PaneState::Running);
            }
            other => panic!("expected StateChanged, got {:?}", other),
        }
    }

    #[test]
    fn sync_detects_removed_pane() {
        let registry = FakeRegistry::new();
        let obs1 = vec![
            make_observation("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
            make_observation("g", "ws", "%1", PaneState::Idle, PaneKind::Terminal),
        ];
        sync_pane_registry(&registry, obs1, None).unwrap();

        let obs2 = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        let result = sync_pane_registry(&registry, obs2, None).unwrap();

        assert_eq!(result.removed, 1);
        let removed: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, PaneEvent::Removed { .. }))
            .collect();
        assert_eq!(removed.len(), 1);
        match &removed[0] {
            PaneEvent::Removed { runtime_id } => {
                assert_eq!(*runtime_id, PaneRuntimeId::derive("g", "ws", "%1"));
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn sync_mirrors_events_to_observer_without_draining() {
        let registry = FakeRegistry::new();
        let observer = FakeObserver::new();
        let obs = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];

        let result = sync_pane_registry(&registry, obs, Some(&observer)).unwrap();

        assert_eq!(result.events.len(), 1, "sync must return events directly");

        let observer_events = observer.poll_events();
        assert_eq!(
            observer_events.len(),
            1,
            "observer must have accumulated the same events"
        );
    }

    #[test]
    fn observer_accumulates_across_sync_cycles() {
        let registry = FakeRegistry::new();
        let observer = FakeObserver::new();

        let obs1 = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        sync_pane_registry(&registry, obs1, Some(&observer)).unwrap();

        let obs2 = vec![
            make_observation("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
            make_observation("g", "ws", "%1", PaneState::Running, PaneKind::Agent),
        ];
        sync_pane_registry(&registry, obs2, Some(&observer)).unwrap();

        let all_events = observer.poll_events();
        assert!(
            all_events.len() >= 2,
            "observer must accumulate events across sync cycles, got {}",
            all_events.len()
        );
    }

    #[test]
    fn external_poll_events_still_works_after_sync() {
        let registry = FakeRegistry::new();
        let observer = FakeObserver::new();

        let obs = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        sync_pane_registry(&registry, obs, Some(&observer)).unwrap();

        let events = observer.poll_events();
        assert!(
            !events.is_empty(),
            "external consumer must be able to poll events"
        );

        let events_after = observer.poll_events();
        assert!(
            events_after.is_empty(),
            "poll_events is destructive — second poll must be empty"
        );
    }

    #[test]
    fn cwd_command_changes_do_not_emit_pane_event() {
        let registry = FakeRegistry::new();
        let obs1 = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        sync_pane_registry(&registry, obs1, None).unwrap();

        let mut obs2 = make_observation("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal);
        obs2.cwd = "/home/user".to_owned();
        obs2.command = "vim".to_owned();
        let result = sync_pane_registry(&registry, vec![obs2], None).unwrap();

        assert!(
            result.events.is_empty(),
            "cwd/command changes must not produce PaneEvent"
        );

        let desc = registry
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc.cwd, "/home/user",
            "cwd must still be updated in descriptor"
        );
        assert_eq!(
            desc.command, "vim",
            "command must still be updated in descriptor"
        );
    }

    #[test]
    fn sync_without_observer_still_returns_events() {
        let registry = FakeRegistry::new();
        let obs = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];

        let result = sync_pane_registry(&registry, obs, None).unwrap();
        assert_eq!(
            result.events.len(),
            1,
            "events must be returned even without observer"
        );
    }

    #[test]
    fn translate_changes_is_separate_function() {
        let events = translate_changes_to_events(&[], &FakeRegistry::new()).unwrap();
        assert!(events.is_empty());
    }

    // ── Registry error propagation ──

    struct FailingGetRegistry;

    impl PaneRegistryPort for FailingGetRegistry {
        fn upsert(&self, _: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
            Ok(RegistryDelta {
                changes: Vec::new(),
            })
        }

        fn remove(&self, _: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            Ok(None)
        }

        fn get(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
            Err(RegistryError::PayloadNotSupported {
                payload_type: format!("simulated failure for {}", id),
            })
        }

        fn list_workspace(&self, _: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(Vec::new())
        }

        fn list(&self, _: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(Vec::new())
        }

        fn drain_events(&self) -> Result<Vec<PaneLifecycleEvent>, RegistryError> {
            Ok(Vec::new())
        }

        fn update_from_observation(
            &self,
            observed: ObservedPaneState,
        ) -> Result<Vec<PaneStateChange>, RegistryError> {
            let id = PaneRuntimeId::derive(
                &observed.group,
                &observed.workspace,
                observed.unit_id.as_str(),
            );
            Ok(vec![PaneStateChange::Registered { runtime_id: id }])
        }

        fn find_by_capability(
            &self,
            _: PaneCapability,
        ) -> Result<Vec<PaneDescriptor>, RegistryError> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn registry_get_error_propagates_through_sync() {
        let registry = FailingGetRegistry;
        let obs = vec![make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];

        let result = sync_pane_registry(&registry, obs, None);
        assert!(
            result.is_err(),
            "sync must propagate registry.get() errors, not silently swallow them"
        );
    }
}

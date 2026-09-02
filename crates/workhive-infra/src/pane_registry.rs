//! Concrete in-memory pane registry implementation.
//!
//! `InMemoryPaneRegistry` is the production implementation of
//! [`PaneRegistryPort`]. It lives in `workhive-infra` because it is
//! a port implementation (adapter-layer), not a domain type.
//!
//! Capability derivation uses [`derive_runtime_capabilities`] from the
//! domain layer, ensuring the registry and all test fakes share the
//! same rule. Static [`derive_capabilities`] is intentionally NOT used
//! on the runtime observation path: it is reserved for plan-time /
//! static declaration only. Runtime capability derivation must stay
//! aligned with message safety, and only a running supervisor-
//! authoritative agent may advertise structured-message capabilities;
//! tmux fallback or OS/process observation is not a supervisor
//! handshake.
//!
//! ## Thread-safety
//!
//! The internal cell type is `Mutex` (not `RefCell`). The control-plane
//! supervisor child process needs to share the registry across the
//! listener thread and any background hydration / observer threads, so
//! the registry MUST be `Sync`. Mutex contention is not a concern in
//! practice because every method is a short critical section over an
//! in-memory `Vec`.
//!
//! A poisoned mutex (caller panicked while holding the lock) is mapped
//! to [`RegistryError::Internal`] rather than propagating the
//! `PoisonError` payload. That keeps the public boundary typed without
//! leaking implementation details, and a poisoned lock is itself a
//! programming bug we want a typed signal for.

use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

use workhive_core::domain::{
    ObservedPaneState, PaneCapability, PaneDescriptor, PaneLaunchOrigin, PaneLifecycleEvent,
    PaneRuntimeId, PaneStateChange, derive_runtime_capabilities, runtime_status_from_state,
};
use workhive_core::ports::{PaneRegistryPort, RegistryDelta, RegistryError};

/// Production in-memory pane registry.
///
/// Stores pane descriptors and buffered lifecycle events behind a
/// `Mutex` for interior mutability and `Sync`-by-default behaviour.
/// Lifecycle events are buffered internally and drained via
/// `drain_events`.
pub struct InMemoryPaneRegistry {
    panes: Mutex<Vec<PaneDescriptor>>,
    lifecycle_events: Mutex<Vec<PaneLifecycleEvent>>,
}

impl InMemoryPaneRegistry {
    pub fn new() -> Self {
        Self {
            panes: Mutex::new(Vec::new()),
            lifecycle_events: Mutex::new(Vec::new()),
        }
    }
}

fn poison_error<T>(err: PoisonError<MutexGuard<'_, T>>) -> RegistryError {
    RegistryError::Internal {
        message: format!("pane registry mutex poisoned: {err}"),
    }
}

impl Default for InMemoryPaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneRegistryPort for InMemoryPaneRegistry {
    fn upsert(&self, descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
        let mut panes = self.panes.lock().map_err(poison_error)?;
        let mut events = self.lifecycle_events.lock().map_err(poison_error)?;

        if let Some(existing) = panes
            .iter_mut()
            .find(|p| p.runtime_id == descriptor.runtime_id)
        {
            let status_changed = if existing.observed_status != descriptor.observed_status {
                let from = existing.observed_status;
                let to = descriptor.observed_status;
                events.push(PaneLifecycleEvent::StatusChanged {
                    pane: descriptor.runtime_id.clone(),
                    from,
                    to,
                });
                true
            } else {
                false
            };

            *existing = descriptor.clone();

            if !status_changed {
                events.push(PaneLifecycleEvent::Updated {
                    pane: descriptor.runtime_id.clone(),
                });
            }

            Ok(RegistryDelta {
                changes: Vec::new(),
            })
        } else {
            events.push(PaneLifecycleEvent::Registered {
                pane: descriptor.runtime_id.clone(),
            });
            panes.push(descriptor);
            Ok(RegistryDelta {
                changes: Vec::new(),
            })
        }
    }

    fn remove(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
        let mut panes = self.panes.lock().map_err(poison_error)?;
        let pos = panes.iter().position(|p| p.runtime_id == *id);
        match pos {
            Some(idx) => {
                let removed = panes.remove(idx);
                self.lifecycle_events
                    .lock()
                    .map_err(poison_error)?
                    .push(PaneLifecycleEvent::Unregistered { pane: id.clone() });
                Ok(Some(removed))
            }
            None => Ok(None),
        }
    }

    fn get(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
        let panes = self.panes.lock().map_err(poison_error)?;
        Ok(panes.iter().find(|p| p.runtime_id == *id).cloned())
    }

    fn list_workspace(&self, workspace: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
        let panes = self.panes.lock().map_err(poison_error)?;
        Ok(panes
            .iter()
            .filter(|p| p.workspace == workspace)
            .cloned()
            .collect())
    }

    fn list(&self, workspace: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
        let panes = self.panes.lock().map_err(poison_error)?;
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
        let mut events = self.lifecycle_events.lock().map_err(poison_error)?;
        Ok(events.drain(..).collect())
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

        let mut panes = self.panes.lock().map_err(poison_error)?;
        if let Some(existing) = panes.iter_mut().find(|p| p.runtime_id == id) {
            let state_changed = if existing.state != observed.state {
                let from = existing.state;
                existing.state = observed.state;
                Some((from, observed.state))
            } else {
                None
            };

            let status_changed = if existing.observed_status != new_status {
                let from = existing.observed_status;
                existing.observed_status = new_status;
                self.lifecycle_events.lock().map_err(poison_error)?.push(
                    PaneLifecycleEvent::StatusChanged {
                        pane: id.clone(),
                        from,
                        to: new_status,
                    },
                );
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
            self.lifecycle_events
                .lock()
                .map_err(poison_error)?
                .push(PaneLifecycleEvent::Registered { pane: id.clone() });
            Ok(vec![PaneStateChange::Registered { runtime_id: id }])
        }
    }

    fn find_by_capability(
        &self,
        capability: PaneCapability,
    ) -> Result<Vec<PaneDescriptor>, RegistryError> {
        let panes = self.panes.lock().map_err(poison_error)?;
        Ok(panes
            .iter()
            .filter(|p| p.capabilities.contains(&capability))
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::{
        PaneKind, PaneObservationConfidence, PaneObservationSource, PaneRuntimeStatus, PaneState,
        UnitId,
    };

    fn make_observation(
        group: &str,
        workspace: &str,
        unit_id: &str,
        state: PaneState,
        kind: PaneKind,
    ) -> ObservedPaneState {
        // Default test fixture mimics the tmux-only fallback path: no
        // supervisor or OS observer enrichment. Tests that need
        // supervisor handshake or OS observer signal must use
        // [`make_observation_layered`].
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

    fn make_descriptor(group: &str, workspace: &str, unit: &str) -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive(group, workspace, unit),
            group: group.to_owned(),
            workspace: workspace.to_owned(),
            unit_id: UnitId::new(unit),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            observed_status: PaneRuntimeStatus::Unknown,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
        }
    }

    #[test]
    fn upsert_new_pane() {
        let reg = InMemoryPaneRegistry::new();
        let desc = make_descriptor("g", "ws", "%0");
        let delta = reg.upsert(desc).unwrap();
        assert!(delta.changes.is_empty());

        let got = reg.get(&PaneRuntimeId::derive("g", "ws", "%0")).unwrap();
        assert!(got.is_some());
        assert_eq!(got.unwrap().group, "g");

        let events = reg.drain_events().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], PaneLifecycleEvent::Registered { .. }));
    }

    #[test]
    fn upsert_update_existing() {
        let reg = InMemoryPaneRegistry::new();
        let desc = make_descriptor("g", "ws", "%0");
        reg.upsert(desc).unwrap();

        let mut updated = make_descriptor("g", "ws", "%0");
        updated.cwd = "/home".to_owned();
        reg.upsert(updated).unwrap();

        let got = reg.get(&PaneRuntimeId::derive("g", "ws", "%0")).unwrap();
        assert_eq!(got.unwrap().cwd, "/home");

        let events = reg.drain_events().unwrap();
        assert_eq!(events.len(), 2); // Registered + Updated
    }

    #[test]
    fn upsert_status_change_emits_status_changed() {
        let reg = InMemoryPaneRegistry::new();
        let desc = make_descriptor("g", "ws", "%0");
        reg.upsert(desc).unwrap();

        let mut updated = make_descriptor("g", "ws", "%0");
        updated.observed_status = PaneRuntimeStatus::Running;
        reg.upsert(updated).unwrap();

        let events = reg.drain_events().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[1],
            PaneLifecycleEvent::StatusChanged { .. }
        ));
    }

    #[test]
    fn remove_existing_pane() {
        let reg = InMemoryPaneRegistry::new();
        reg.upsert(make_descriptor("g", "ws", "%0")).unwrap();

        let id = PaneRuntimeId::derive("g", "ws", "%0");
        let removed = reg.remove(&id).unwrap();
        assert!(removed.is_some());

        let got = reg.get(&id).unwrap();
        assert!(got.is_none());

        let events = reg.drain_events().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, PaneLifecycleEvent::Unregistered { .. }))
        );
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let reg = InMemoryPaneRegistry::new();
        let id = PaneRuntimeId::derive("g", "ws", "%0");
        let removed = reg.remove(&id).unwrap();
        assert!(removed.is_none());
    }

    #[test]
    fn list_workspace_filters_correctly() {
        let reg = InMemoryPaneRegistry::new();
        reg.upsert(make_descriptor("g", "ws-a", "%0")).unwrap();
        reg.upsert(make_descriptor("g", "ws-b", "%1")).unwrap();

        let all = reg.list(None).unwrap();
        assert_eq!(all.len(), 2);

        let filtered = reg.list_workspace("ws-a").unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].workspace, "ws-a");
    }

    #[test]
    fn update_from_observation_registers_new() {
        let reg = InMemoryPaneRegistry::new();
        let obs = make_observation("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal);
        let changes = reg.update_from_observation(obs).unwrap();

        assert_eq!(changes.len(), 1);
        assert!(matches!(&changes[0], PaneStateChange::Registered { .. }));

        let desc = reg.get(&PaneRuntimeId::derive("g", "ws", "%0")).unwrap();
        assert!(desc.is_some());
        assert!(
            desc.unwrap()
                .capabilities
                .contains(&PaneCapability::CommandExecutor)
        );
    }

    #[test]
    fn update_from_observation_detects_state_change() {
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();

        let changes = reg
            .update_from_observation(make_observation(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Terminal,
            ))
            .unwrap();

        assert_eq!(changes.len(), 1);
        match &changes[0] {
            PaneStateChange::DescriptorUpdated { state_changed, .. } => {
                let (from, to) = state_changed.as_ref().unwrap();
                assert_eq!(*from, PaneState::Idle);
                assert_eq!(*to, PaneState::Running);
            }
            other => panic!("expected DescriptorUpdated, got {:?}", other),
        }
    }

    #[test]
    fn update_from_observation_detects_capability_change_for_supervised_running_agent() {
        // The capability change is only triggered when the new
        // observation is a *live* supervised agent: kind=Agent +
        // state=Running + supervisor + authoritative. A tmux fallback
        // observation of an agent process must NOT add MessageReceiver.
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();

        let changes = reg
            .update_from_observation(make_observation_layered(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Agent,
                PaneObservationSource::Supervisor,
                PaneObservationConfidence::Authoritative,
            ))
            .unwrap();

        assert_eq!(changes.len(), 1);
        match &changes[0] {
            PaneStateChange::DescriptorUpdated {
                capabilities_changed,
                ..
            } => {
                let (added, removed) = capabilities_changed.as_ref().unwrap();
                assert!(added.contains(&PaneCapability::MessageResponder));
                assert!(added.contains(&PaneCapability::MessageReceiver));
                assert!(!removed.contains(&PaneCapability::CommandExecutor));
                assert!(
                    removed.is_empty(),
                    "no capabilities should be removed: {removed:?}"
                );
            }
            other => panic!("expected DescriptorUpdated, got {:?}", other),
        }
    }

    #[test]
    fn tmux_fallback_agent_observation_does_not_promote_message_receiver() {
        // Regression test: a manually-started
        // codex pane (no supervisor handshake, no OS observer
        // enrichment, just tmux current-command saying "codex") MUST
        // NOT gain MessageReceiver/Responder. Before the fix the
        // registry called `derive_capabilities(PaneKind::Agent)` and
        // happily granted full message capabilities, breaking the
        // "honest capability" contract.
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();

        let _ = reg
            .update_from_observation(make_observation(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Agent,
            ))
            .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(desc.kind, PaneKind::Agent);
        assert!(
            !desc.capabilities.contains(&PaneCapability::MessageReceiver),
            "tmux-fallback agent must not be promoted to MessageReceiver"
        );
        assert!(
            !desc
                .capabilities
                .contains(&PaneCapability::MessageResponder),
            "tmux-fallback agent must not be promoted to MessageResponder"
        );
        assert!(desc.capabilities.contains(&PaneCapability::CommandExecutor));
    }

    #[test]
    fn supervised_authoritative_idle_agent_keeps_only_command_executor() {
        // Even though the supervisor authoritatively knew the agent's
        // identity, an Idle/Exited state means the live structured
        // channel is gone. The registry must drop the structured-
        // message capabilities for routing decisions.
        let reg = InMemoryPaneRegistry::new();
        let _ = reg
            .update_from_observation(make_observation_layered(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Agent,
                PaneObservationSource::Supervisor,
                PaneObservationConfidence::Authoritative,
            ))
            .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(desc.kind, PaneKind::Agent);
        assert_eq!(
            desc.capabilities,
            [PaneCapability::CommandExecutor].into_iter().collect(),
            "supervised + Idle agent must collapse to CommandExecutor only"
        );
    }

    #[test]
    fn cwd_command_update_without_state_change_produces_no_diff() {
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();

        let mut obs = make_observation("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal);
        obs.cwd = "/home".to_owned();
        obs.command = "vim".to_owned();
        let changes = reg.update_from_observation(obs).unwrap();

        assert!(
            changes.is_empty(),
            "cwd/command-only change must not produce diff"
        );

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(desc.cwd, "/home");
        assert_eq!(desc.command, "vim");
    }

    #[test]
    fn find_by_capability_works() {
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();
        // Use a *live* supervised agent so the second pane actually
        // qualifies for MessageResponder. A tmux-fallback agent
        // observation here would not.
        reg.update_from_observation(make_observation_layered(
            "g",
            "ws",
            "%1",
            PaneState::Running,
            PaneKind::Agent,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        ))
        .unwrap();

        let executors = reg
            .find_by_capability(PaneCapability::CommandExecutor)
            .unwrap();
        assert_eq!(executors.len(), 2);

        let responders = reg
            .find_by_capability(PaneCapability::MessageResponder)
            .unwrap();
        assert_eq!(responders.len(), 1);
    }

    #[test]
    fn find_by_capability_excludes_unverified_agents() {
        // Two agent panes, neither is a live supervised agent. None
        // should appear in `find_by_capability(MessageReceiver)`.
        // This is the key invariant message-bus / orchestration
        // consumers rely on.
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Running,
            PaneKind::Agent,
        ))
        .unwrap(); // tmux fallback
        reg.update_from_observation(make_observation_layered(
            "g",
            "ws",
            "%1",
            PaneState::Idle,
            PaneKind::Agent,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        ))
        .unwrap(); // supervisor authoritative but Idle

        let receivers = reg
            .find_by_capability(PaneCapability::MessageReceiver)
            .unwrap();
        assert!(
            receivers.is_empty(),
            "registry consumers must never see unverified or exited \
             agents under MessageReceiver: got {receivers:?}"
        );

        let responders = reg
            .find_by_capability(PaneCapability::MessageResponder)
            .unwrap();
        assert!(responders.is_empty());
    }

    #[test]
    fn drain_events_is_destructive() {
        let reg = InMemoryPaneRegistry::new();
        reg.upsert(make_descriptor("g", "ws", "%0")).unwrap();

        let first = reg.drain_events().unwrap();
        assert_eq!(first.len(), 1);

        let second = reg.drain_events().unwrap();
        assert!(second.is_empty(), "drain_events must be destructive");
    }

    #[test]
    fn capability_derivation_uses_runtime_aware_function() {
        // The registry must derive capabilities through
        // `derive_runtime_capabilities`, so that an observation's
        // `(kind, state, source, confidence)` tuple controls the
        // outcome rather than `kind` alone. GitUi must remain TUI-only
        // — never structured-message capable — regardless of source.
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::GitUi,
        ))
        .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc.capabilities,
            derive_runtime_capabilities(
                PaneKind::GitUi,
                PaneState::Idle,
                PaneObservationSource::TmuxCurrentCommand,
                PaneObservationConfidence::Fallback,
            )
        );
        assert!(!desc.capabilities.contains(&PaneCapability::MessageReceiver));
    }

    #[test]
    fn observation_sets_correct_observed_status_on_new_pane() {
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        ))
        .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc.observed_status,
            PaneRuntimeStatus::Idle,
            "new pane observed as Idle must have observed_status = Idle, not Unknown"
        );
    }

    #[test]
    fn baseline_running_corrected_to_idle_by_observation() {
        let reg = InMemoryPaneRegistry::new();
        let mut desc = make_descriptor("g", "ws", "%0");
        desc.observed_status = PaneRuntimeStatus::Running;
        desc.state = PaneState::Running;
        reg.upsert(desc).unwrap();

        let changes = reg
            .update_from_observation(make_observation(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            ))
            .unwrap();

        assert!(
            !changes.is_empty(),
            "observation correcting Running -> Idle must produce a change"
        );

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc.observed_status,
            PaneRuntimeStatus::Idle,
            "observed_status must be corrected from Running to Idle"
        );

        let events = reg.drain_events().unwrap();
        assert!(
            events.iter().any(|e| matches!(
                e,
                PaneLifecycleEvent::StatusChanged {
                    from: PaneRuntimeStatus::Running,
                    to: PaneRuntimeStatus::Idle,
                    ..
                }
            )),
            "StatusChanged lifecycle event must be emitted"
        );
    }

    #[test]
    fn observation_registered_pane_does_not_stay_unknown() {
        let reg = InMemoryPaneRegistry::new();
        reg.update_from_observation(make_observation(
            "g",
            "ws",
            "%0",
            PaneState::Running,
            PaneKind::Agent,
        ))
        .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_ne!(
            desc.observed_status,
            PaneRuntimeStatus::Unknown,
            "pane registered via observation must not have Unknown status"
        );
        assert_eq!(desc.observed_status, PaneRuntimeStatus::Running);
    }
}

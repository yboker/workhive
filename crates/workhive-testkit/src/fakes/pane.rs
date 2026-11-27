//! Fake pane registry and observer for testing.
//!
//! These fakes implement [`PaneRegistryPort`] and [`PaneObserverPort`]
//! using the same runtime-aware capability derivation
//! ([`derive_runtime_capabilities`]) as the concrete implementations
//! in `workhive-infra`, ensuring behavioral parity. Static
//! [`derive_capabilities`] is intentionally not used on the runtime
//! observation path — it is reserved for plan-time / static
//! declaration only.

use std::cell::RefCell;
use std::collections::BTreeSet;

use workhive_core::domain::{
    ObservedPaneState, PaneCapability, PaneDescriptor, PaneEvent, PaneLaunchOrigin,
    PaneLifecycleEvent, PaneRuntimeId, PaneStateChange, derive_runtime_capabilities,
    runtime_status_from_state,
};
use workhive_core::ports::{PaneObserverPort, PaneRegistryPort, RegistryDelta, RegistryError};

/// In-memory fake pane registry for testing.
pub struct FakePaneRegistry {
    panes: RefCell<Vec<PaneDescriptor>>,
    lifecycle_events: RefCell<Vec<PaneLifecycleEvent>>,
}

impl FakePaneRegistry {
    pub fn new() -> Self {
        Self {
            panes: RefCell::new(Vec::new()),
            lifecycle_events: RefCell::new(Vec::new()),
        }
    }
}

impl Default for FakePaneRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneRegistryPort for FakePaneRegistry {
    fn upsert(&self, descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
        let mut panes = self.panes.borrow_mut();
        let mut events = self.lifecycle_events.borrow_mut();

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
        let mut panes = self.panes.borrow_mut();
        let pos = panes.iter().position(|p| p.runtime_id == *id);
        match pos {
            Some(idx) => {
                let removed = panes.remove(idx);
                self.lifecycle_events
                    .borrow_mut()
                    .push(PaneLifecycleEvent::Unregistered { pane: id.clone() });
                Ok(Some(removed))
            }
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
        Ok(std::mem::take(&mut *self.lifecycle_events.borrow_mut()))
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
                let from = existing.observed_status;
                existing.observed_status = new_status;
                self.lifecycle_events
                    .borrow_mut()
                    .push(PaneLifecycleEvent::StatusChanged {
                        pane: id.clone(),
                        from,
                        to: new_status,
                    });
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
                .borrow_mut()
                .push(PaneLifecycleEvent::Registered { pane: id.clone() });
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

/// In-memory fake pane observer for testing.
pub struct FakePaneObserver {
    events: RefCell<Vec<PaneEvent>>,
}

impl FakePaneObserver {
    pub fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.events.borrow().len()
    }
}

impl Default for FakePaneObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneObserverPort for FakePaneObserver {
    fn push_event(&self, event: PaneEvent) {
        self.events.borrow_mut().push(event);
    }

    fn poll_events(&self) -> Vec<PaneEvent> {
        std::mem::take(&mut *self.events.borrow_mut())
    }
}

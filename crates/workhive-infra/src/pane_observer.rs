//! Concrete in-memory pane observer implementation.
//!
//! `InMemoryPaneObserver` is the production implementation of
//! [`PaneObserverPort`]. It accumulates [`PaneEvent`] values pushed by
//! sync logic and provides destructive draining via `poll_events()` for
//! external consumers.

use std::cell::RefCell;

use workhive_core::domain::PaneEvent;
use workhive_core::ports::PaneObserverPort;

/// Production in-memory pane event buffer.
///
/// Events accumulate across sync cycles until an external consumer
/// drains them via `poll_events()`. Sync logic must only call
/// `push_event()` — never `poll_events()`.
pub struct InMemoryPaneObserver {
    events: RefCell<Vec<PaneEvent>>,
}

impl InMemoryPaneObserver {
    pub fn new() -> Self {
        Self {
            events: RefCell::new(Vec::new()),
        }
    }

    /// Number of buffered events (for diagnostics/testing).
    pub fn pending_count(&self) -> usize {
        self.events.borrow().len()
    }
}

impl Default for InMemoryPaneObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneObserverPort for InMemoryPaneObserver {
    fn push_event(&self, event: PaneEvent) {
        self.events.borrow_mut().push(event);
    }

    fn poll_events(&self) -> Vec<PaneEvent> {
        self.events.borrow_mut().drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::PaneRuntimeId;

    #[test]
    fn push_and_poll() {
        let obs = InMemoryPaneObserver::new();
        let event = PaneEvent::Removed {
            runtime_id: PaneRuntimeId::derive("g", "ws", "%0"),
        };

        obs.push_event(event);
        assert_eq!(obs.pending_count(), 1);

        let events = obs.poll_events();
        assert_eq!(events.len(), 1);
        assert_eq!(obs.pending_count(), 0);
    }

    #[test]
    fn poll_is_destructive() {
        let obs = InMemoryPaneObserver::new();
        obs.push_event(PaneEvent::Removed {
            runtime_id: PaneRuntimeId::derive("g", "ws", "%0"),
        });

        let first = obs.poll_events();
        assert_eq!(first.len(), 1);

        let second = obs.poll_events();
        assert!(second.is_empty(), "second poll must be empty");
    }

    #[test]
    fn accumulates_across_pushes() {
        let obs = InMemoryPaneObserver::new();
        for i in 0..5 {
            obs.push_event(PaneEvent::Removed {
                runtime_id: PaneRuntimeId::derive("g", "ws", &format!("%{i}")),
            });
        }

        assert_eq!(obs.pending_count(), 5);
        let events = obs.poll_events();
        assert_eq!(events.len(), 5);
    }

    #[test]
    fn default_is_empty() {
        let obs = InMemoryPaneObserver::default();
        assert_eq!(obs.pending_count(), 0);
        assert!(obs.poll_events().is_empty());
    }
}

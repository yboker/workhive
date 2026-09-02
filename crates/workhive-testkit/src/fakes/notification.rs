//! Fake [`NotificationPort`] for use in tests.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use workhive_testkit::fakes::FakeNotificationPort;
//!
//! let port = FakeNotificationPort::new();
//! // ... trigger delivery use case ...
//! assert_eq!(port.delivery_count(), 1);
//! assert!(port.delivered_categories().contains(&NotificationCategory::PermissionNeeded));
//! ```
//!
//! ### Failure simulation
//!
//! ```rust,ignore
//! let port = FakeNotificationPort::failing("disk full");
//! ```
//! Every call to `deliver` will return an `Err` so the fail-open
//! path is exercised.

use std::sync::{Arc, Mutex};

use workhive_core::ports::{
    NotificationCategory, NotificationDeliveryError, NotificationPort, PendingNotification,
};

// ── FakeNotificationPort ──────────────────────────────────────────────────────

/// In-memory notification port for tests.
///
/// Thread-safe via an internal `Arc<Mutex<...>>` so it can be shared
/// between the delivery use case and the test assertions.
#[derive(Clone, Debug, Default)]
pub struct FakeNotificationPort {
    inner: Arc<Mutex<FakePortState>>,
}

#[derive(Debug, Default)]
struct FakePortState {
    deliveries: Vec<PendingNotification>,
    failure_reason: Option<String>,
}

impl FakeNotificationPort {
    /// Create a port that records all deliveries successfully.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a port that simulates a permanent delivery failure with
    /// the given reason string.
    pub fn failing(reason: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakePortState {
                deliveries: Vec::new(),
                failure_reason: Some(reason.into()),
            })),
        }
    }

    /// Number of successfully recorded deliveries.
    pub fn delivery_count(&self) -> usize {
        self.inner.lock().unwrap().deliveries.len()
    }

    /// All [`NotificationCategory`] values that have been delivered.
    pub fn delivered_categories(&self) -> Vec<NotificationCategory> {
        self.inner
            .lock()
            .unwrap()
            .deliveries
            .iter()
            .map(|n| n.category)
            .collect()
    }

    /// All delivered notifications (cloned), newest last.
    pub fn deliveries(&self) -> Vec<PendingNotification> {
        self.inner.lock().unwrap().deliveries.clone()
    }

    /// Set or clear the failure reason. Useful for toggling failure
    /// mid-test.
    pub fn set_failure(&self, reason: Option<String>) {
        self.inner.lock().unwrap().failure_reason = reason;
    }

    /// Whether the port is currently configured to fail.
    pub fn is_failing(&self) -> bool {
        self.inner.lock().unwrap().failure_reason.is_some()
    }
}

impl NotificationPort for FakeNotificationPort {
    fn deliver(&self, notification: &PendingNotification) -> Result<(), NotificationDeliveryError> {
        let mut state = self.inner.lock().unwrap();
        if let Some(ref reason) = state.failure_reason {
            return Err(NotificationDeliveryError {
                reason: reason.clone(),
            });
        }
        state.deliveries.push(notification.clone());
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::activity::{PaneCorrelationKey, ProviderKind};
    use workhive_core::ports::{NotificationCategory, NotificationHint, TransitionSignature};

    fn make_notification(category: NotificationCategory) -> PendingNotification {
        PendingNotification {
            pane: PaneCorrelationKey::new("g", "w", "u"),
            provider: Some(ProviderKind::Codex),
            category,
            hint: Some(NotificationHint::LifecycleHook),
            transition_sig: TransitionSignature::from_event_id("test-evt-001"),
        }
    }

    /// A succeeding fake port records deliveries and can simulate
    /// failure.
    #[test]
    fn fake_notification_port_records_deliveries_and_can_simulate_failure() {
        let port = FakeNotificationPort::new();

        // Successful delivery.
        let n = make_notification(NotificationCategory::PermissionNeeded);
        port.deliver(&n).expect("should succeed");
        assert_eq!(port.delivery_count(), 1);
        assert!(
            port.delivered_categories()
                .contains(&NotificationCategory::PermissionNeeded)
        );

        // Enable failure.
        port.set_failure(Some("test disk full".to_owned()));
        let n2 = make_notification(NotificationCategory::Failed);
        assert!(port.deliver(&n2).is_err(), "failure mode must return Err");
        // Failed delivery is NOT recorded in the fake's list.
        assert_eq!(
            port.delivery_count(),
            1,
            "failed delivery not added to count"
        );

        // Disable failure again.
        port.set_failure(None);
        let n3 = make_notification(NotificationCategory::Completed);
        port.deliver(&n3).expect("should succeed again");
        assert_eq!(port.delivery_count(), 2);
    }
}

//! Shared scheduling lease retry policy for reconciliation operations.
//!
//! Provides a bounded retry loop for acquiring scheduling leases.

use workhive_core::errors::IndexError;
use workhive_core::ports::{
    IndexReconciliationPort, ReconciliationLease, ReconciliationLeaseOutcome,
};

/// Outcome of a bounded scheduling-lease acquisition.
#[derive(Debug)]
pub(super) enum PublicationLeaseError {
    /// The lease remained contended for the entire bounded budget.
    ContentionExhausted,
    /// An I/O failure occurred while opening or locking the lease file.
    Io(IndexError),
}

/// Bounded scheduling-lease acquisition with non-blocking retries.
///
/// Retries only the lease file operation; does not reload roots, rewrite
/// `Updating` state, or re-run discovery. Returns either the acquired lease
/// or a typed exhaustion/Io error.
///
/// # Arguments
/// * `reconciliation` - The reconciliation port to acquire the lease from
/// * `config_dir` - The configuration directory containing the lease file
/// * `wait` - A callback for delaying between retry attempts
pub(super) fn acquire_scheduling_lease_bounded(
    reconciliation: &dyn IndexReconciliationPort,
    config_dir: &camino::Utf8Path,
    wait: &dyn Fn(std::time::Duration),
) -> Result<Box<dyn ReconciliationLease>, PublicationLeaseError> {
    const MAX_ATTEMPTS: usize = 100;
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(5);
    for attempt in 0..MAX_ATTEMPTS {
        match reconciliation.try_acquire_scheduling_lease(config_dir) {
            Ok(ReconciliationLeaseOutcome::Acquired(lease)) => return Ok(lease),
            Ok(ReconciliationLeaseOutcome::Contended) => {
                if attempt + 1 < MAX_ATTEMPTS {
                    wait(RETRY_DELAY);
                }
            }
            Err(e) => return Err(PublicationLeaseError::Io(e)),
        }
    }
    Err(PublicationLeaseError::ContentionExhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;
    use workhive_core::ports::ReconciliationLeaseOutcome;

    struct MockLease;
    impl std::fmt::Debug for MockLease {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "MockLease")
        }
    }
    impl ReconciliationLease for MockLease {}

    struct ContendingPort {
        contention_count: usize,
        current_attempt: RefCell<usize>,
    }

    impl IndexReconciliationPort for ContendingPort {
        fn read_state(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<
            workhive_core::domain::index_reconciliation::ReconciliationState,
            workhive_core::errors::IndexError,
        > {
            unimplemented!()
        }

        fn write_state(
            &self,
            _config_dir: &camino::Utf8Path,
            _state: &workhive_core::domain::index_reconciliation::ReconciliationState,
        ) -> Result<(), workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn read_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<
            Option<workhive_core::domain::index_reconciliation::ReconciliationRevision>,
            workhive_core::errors::IndexError,
        > {
            unimplemented!()
        }

        fn write_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
            _revision: workhive_core::domain::index_reconciliation::ReconciliationRevision,
        ) -> Result<(), workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn try_acquire_worker_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn try_acquire_scheduling_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            let mut attempt = self.current_attempt.borrow_mut();
            *attempt += 1;
            if *attempt > self.contention_count {
                Ok(ReconciliationLeaseOutcome::Acquired(Box::new(MockLease)))
            } else {
                Ok(ReconciliationLeaseOutcome::Contended)
            }
        }
    }

    struct IoErrorPort {
        attempt_count: RefCell<usize>,
    }

    impl IoErrorPort {
        fn new() -> Self {
            Self {
                attempt_count: RefCell::new(0),
            }
        }

        fn attempt_count(&self) -> usize {
            *self.attempt_count.borrow()
        }
    }

    impl IndexReconciliationPort for IoErrorPort {
        fn read_state(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<
            workhive_core::domain::index_reconciliation::ReconciliationState,
            workhive_core::errors::IndexError,
        > {
            unimplemented!()
        }

        fn write_state(
            &self,
            _config_dir: &camino::Utf8Path,
            _state: &workhive_core::domain::index_reconciliation::ReconciliationState,
        ) -> Result<(), workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn read_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<
            Option<workhive_core::domain::index_reconciliation::ReconciliationRevision>,
            workhive_core::errors::IndexError,
        > {
            unimplemented!()
        }

        fn write_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
            _revision: workhive_core::domain::index_reconciliation::ReconciliationRevision,
        ) -> Result<(), workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn try_acquire_worker_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            unimplemented!()
        }

        fn try_acquire_scheduling_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            let mut attempt = self.attempt_count.borrow_mut();
            *attempt += 1;
            Err(workhive_core::errors::IndexError::ReadFailed {
                path: "test".into(),
                source: std::io::Error::other("test error"),
            })
        }
    }

    #[test]
    fn acquire_scheduling_lease_bounded_immediate_success() {
        let port = ContendingPort {
            contention_count: 0,
            current_attempt: RefCell::new(0),
        };
        let wait_calls = Rc::new(RefCell::new(Vec::new()));
        let wait_calls_clone = wait_calls.clone();

        let result = acquire_scheduling_lease_bounded(
            &port,
            camino::Utf8Path::new("/test"),
            &move |duration| {
                wait_calls_clone.borrow_mut().push(duration);
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            *port.current_attempt.borrow(),
            1,
            "Immediate success must complete exactly 1 attempt"
        );
        assert_eq!(
            wait_calls.borrow().len(),
            0,
            "Immediate success must have 0 waits"
        );
    }

    #[test]
    fn acquire_scheduling_lease_bounded_contention_then_success() {
        let port = ContendingPort {
            contention_count: 5,
            current_attempt: RefCell::new(0),
        };
        let wait_calls = Rc::new(RefCell::new(Vec::new()));
        let wait_calls_clone = wait_calls.clone();

        let result = acquire_scheduling_lease_bounded(
            &port,
            camino::Utf8Path::new("/test"),
            &move |duration| {
                wait_calls_clone.borrow_mut().push(duration);
            },
        );

        assert!(result.is_ok());
        assert_eq!(
            *port.current_attempt.borrow(),
            6,
            "5 contentions + 1 success = 6 total attempts"
        );
        assert_eq!(
            wait_calls.borrow().len(),
            5,
            "Should wait exactly 5 times for 5 contentions"
        );
        // Verify all waits are exactly 5ms
        for (idx, wait) in wait_calls.borrow().iter().enumerate() {
            assert_eq!(
                *wait,
                Duration::from_millis(5),
                "Wait {} must be exactly 5ms",
                idx
            );
        }
    }

    #[test]
    fn acquire_scheduling_lease_bounded_exhausts_contention() {
        let port = ContendingPort {
            contention_count: 1000, // More than MAX_ATTEMPTS
            current_attempt: RefCell::new(0),
        };
        let wait_calls = Rc::new(RefCell::new(Vec::new()));
        let wait_calls_clone = wait_calls.clone();

        let result = acquire_scheduling_lease_bounded(
            &port,
            camino::Utf8Path::new("/test"),
            &move |duration| {
                wait_calls_clone.borrow_mut().push(duration);
            },
        );

        assert!(matches!(
            result,
            Err(PublicationLeaseError::ContentionExhausted)
        ));
        assert_eq!(
            *port.current_attempt.borrow(),
            100,
            "Must exhaust exactly 100 attempts (MAX_ATTEMPTS)"
        );
        assert_eq!(
            wait_calls.borrow().len(),
            99,
            "Should wait exactly 99 times (no wait after final attempt)"
        );
        // Verify all waits are exactly 5ms
        for (idx, wait) in wait_calls.borrow().iter().enumerate() {
            assert_eq!(
                *wait,
                Duration::from_millis(5),
                "Wait {} must be exactly 5ms",
                idx
            );
        }
    }

    #[test]
    fn acquire_scheduling_lease_bounded_io_error_immediate() {
        let port = IoErrorPort::new();
        let wait_calls = Rc::new(RefCell::new(Vec::new()));
        let wait_calls_clone = wait_calls.clone();

        let result = acquire_scheduling_lease_bounded(
            &port,
            camino::Utf8Path::new("/test"),
            &move |duration| {
                wait_calls_clone.borrow_mut().push(duration);
            },
        );

        assert_eq!(
            port.attempt_count(),
            1,
            "I/O error must occur on exactly 1 attempt"
        );
        assert_eq!(wait_calls.borrow().len(), 0, "No waits on I/O error");
        // Verify exact error preservation
        match result {
            Err(PublicationLeaseError::Io(err)) => match err {
                workhive_core::errors::IndexError::ReadFailed { path, source } => {
                    assert_eq!(path, "test", "Error path must be preserved exactly");
                    assert_eq!(
                        source.kind(),
                        std::io::ErrorKind::Other,
                        "Error source kind must be preserved"
                    );
                    assert_eq!(
                        source.to_string(),
                        "test error",
                        "Error source message must be preserved exactly"
                    );
                }
                other => panic!("Expected ReadFailed, got {:?}", other),
            },
            other => panic!("Expected Io error, got {:?}", other),
        }
    }
}

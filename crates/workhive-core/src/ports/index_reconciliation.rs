//! Runtime-neutral persistence and lease boundary for index reconciliation.

use camino::Utf8Path;

use crate::domain::index_reconciliation::{ReconciliationRevision, ReconciliationState};
use crate::errors::IndexError;

/// RAII lease held while a reconciliation coordination boundary is active.
pub trait ReconciliationLease: std::fmt::Debug {}

/// Result of a non-blocking lease acquisition attempt.
#[derive(Debug)]
pub enum ReconciliationLeaseOutcome {
    /// The caller owns the lease until the guard is dropped.
    Acquired(Box<dyn ReconciliationLease>),
    /// Another process currently owns the lease.
    Contended,
}

/// Persistence and inter-process coordination required by index reconciliation.
pub trait IndexReconciliationPort {
    /// Read the durable lifecycle state.
    fn read_state(&self, config_dir: &Utf8Path) -> Result<ReconciliationState, IndexError>;

    /// Atomically persist the lifecycle state.
    fn write_state(
        &self,
        config_dir: &Utf8Path,
        state: &ReconciliationState,
    ) -> Result<(), IndexError>;

    /// Read the latest requested revision.
    fn read_latest_revision(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<Option<ReconciliationRevision>, IndexError>;

    /// Atomically persist the latest requested revision.
    fn write_latest_revision(
        &self,
        config_dir: &Utf8Path,
        revision: ReconciliationRevision,
    ) -> Result<(), IndexError>;

    /// Try to acquire the single-worker lease without blocking.
    fn try_acquire_worker_lease(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, IndexError>;

    /// Try to acquire the mutation/publication coordination lease without blocking.
    fn try_acquire_scheduling_lease(
        &self,
        config_dir: &Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, IndexError>;
}

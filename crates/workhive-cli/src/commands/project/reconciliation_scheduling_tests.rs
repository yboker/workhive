use super::*;
use workhive_core::ports::IndexReconciliationPort;

// ── reconciliation_scheduling_ — fingerprint-based scheduling decision ──

use tempfile::TempDir;

fn setup_recon_dir() -> (TempDir, camino::Utf8PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8 path");
    (tmp, dir)
}

#[test]
fn reconciliation_scheduling_first_mutation_schedules() {
    let (_tmp, dir) = setup_recon_dir();
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(
        try_schedule_reconciliation(dir.as_ref(), &roots).unwrap(),
        "first mutation must schedule"
    );
}

#[test]
fn reconciliation_scheduling_same_roots_noop() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let trusted_roots = workhive_core::domain::TrustedRootSet::from_raw(&roots, None);

    let fp = RootsFingerprint::from_trusted_roots(&trusted_roots);
    let rev = ReconciliationRevision::new(1);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 1,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        !try_schedule_reconciliation(dir.as_ref(), &roots).unwrap(),
        "same fingerprint must be NoOp"
    );
}

#[test]
fn reconciliation_scheduling_different_roots_schedules() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let old_roots = vec![camino::Utf8PathBuf::from("/dev/old")];
    let new_roots = vec![camino::Utf8PathBuf::from("/dev/new")];

    let fp = RootsFingerprint::from_trusted_roots(
        &workhive_core::domain::TrustedRootSet::from_raw(&old_roots, None),
    );
    let rev = ReconciliationRevision::new(1);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 1,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        try_schedule_reconciliation(dir.as_ref(), &new_roots).unwrap(),
        "changed roots must schedule"
    );
}

#[test]
fn reconciliation_scheduling_add_root_schedules() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let base = vec![camino::Utf8PathBuf::from("/dev/a")];
    let expanded = vec![
        camino::Utf8PathBuf::from("/dev/a"),
        camino::Utf8PathBuf::from("/dev/b"),
    ];

    let fp = RootsFingerprint::from_trusted_roots(
        &workhive_core::domain::TrustedRootSet::from_raw(&base, None),
    );
    let rev = ReconciliationRevision::new(1);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 1,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        try_schedule_reconciliation(dir.as_ref(), &expanded).unwrap(),
        "adding a root must schedule"
    );
}

#[test]
fn reconciliation_scheduling_remove_root_schedules() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let original = vec![
        camino::Utf8PathBuf::from("/dev/a"),
        camino::Utf8PathBuf::from("/dev/b"),
    ];
    let reduced = vec![camino::Utf8PathBuf::from("/dev/a")];

    let fp = RootsFingerprint::from_trusted_roots(
        &workhive_core::domain::TrustedRootSet::from_raw(&original, None),
    );
    let rev = ReconciliationRevision::new(2);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 2,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        try_schedule_reconciliation(dir.as_ref(), &reduced).unwrap(),
        "removing a root must schedule"
    );
}

#[test]
fn reconciliation_scheduling_empty_roots_schedules_on_first_call() {
    let (_tmp, dir) = setup_recon_dir();
    let roots: Vec<camino::Utf8PathBuf> = vec![];
    assert!(
        try_schedule_reconciliation(dir.as_ref(), &roots).unwrap(),
        "empty roots on first call schedules (fingerprint differs from no-prior)"
    );
}

#[test]
fn reconciliation_scheduling_empty_roots_noop_when_unchanged() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let roots: Vec<camino::Utf8PathBuf> = vec![];

    let fp = RootsFingerprint::from_trusted_roots(
        &workhive_core::domain::TrustedRootSet::from_raw(&roots, None),
    );
    let rev = ReconciliationRevision::new(1);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 0,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        !try_schedule_reconciliation(dir.as_ref(), &roots).unwrap(),
        "unchanged empty roots must be NoOp"
    );
}

#[test]
fn reconciliation_scheduling_increments_revision() {
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let reconciliation = FsIndexReconciliation::new();

    assert!(try_schedule_reconciliation(dir.as_ref(), &roots).unwrap());
    let rev1 = reconciliation
        .read_latest_revision(dir.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(rev1.value(), 1);

    let roots2 = vec![camino::Utf8PathBuf::from("/dev/other")];
    assert!(try_schedule_reconciliation(dir.as_ref(), &roots2).unwrap());
    let rev2 = reconciliation
        .read_latest_revision(dir.as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(rev2.value(), 2);
}

#[test]
fn reconciliation_scheduling_repeated_without_completion_schedules_again() {
    let (_tmp, dir) = setup_recon_dir();
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(try_schedule_reconciliation(dir.as_ref(), &roots).unwrap());

    let roots_same = roots.clone();
    assert!(
        try_schedule_reconciliation(dir.as_ref(), &roots_same).unwrap(),
        "without a Completed state, scheduling accepts again"
    );
}

#[test]
fn reconciliation_scheduling_reorder_does_not_schedule() {
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::ports::IndexReconciliationPort;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let roots_a = vec![
        camino::Utf8PathBuf::from("/dev/a"),
        camino::Utf8PathBuf::from("/dev/b"),
    ];
    let roots_b = vec![
        camino::Utf8PathBuf::from("/dev/b"),
        camino::Utf8PathBuf::from("/dev/a"),
    ];

    let fp = RootsFingerprint::from_trusted_roots(
        &workhive_core::domain::TrustedRootSet::from_raw(&roots_a, None),
    );
    let rev = ReconciliationRevision::new(3);
    let reconciliation = FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), rev)
        .unwrap();
    let state = ReconciliationState::Completed {
        revision: rev,
        fingerprint: fp,
        completed_at: std::time::SystemTime::now(),
        discovered_count: 2,
    };
    reconciliation.write_state(dir.as_ref(), &state).unwrap();

    assert!(
        !try_schedule_reconciliation(dir.as_ref(), &roots_b).unwrap(),
        "reordered roots must produce same fingerprint and be NoOp"
    );
}

// Two revision reads both complete before either write.
#[test]
fn reconciliation_scheduling_unlocked_control_records_duplicate_revision_deterministically() {
    use std::sync::{Arc, Barrier, Mutex};
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState,
    };
    use workhive_core::errors::IndexError;
    use workhive_core::ports::{
        IndexReconciliationPort, ReconciliationLease, ReconciliationLeaseOutcome,
    };

    struct BarrierPort {
        read_barrier: Arc<Barrier>,
        reads_completed: Arc<Mutex<usize>>,
        writes: Arc<Mutex<Vec<u64>>>,
    }

    struct NoopLease;
    impl std::fmt::Debug for NoopLease {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("NoopLease")
        }
    }
    impl ReconciliationLease for NoopLease {}

    impl IndexReconciliationPort for BarrierPort {
        fn read_state(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationState, IndexError> {
            Ok(ReconciliationState::Idle)
        }
        fn write_state(
            &self,
            _config_dir: &camino::Utf8Path,
            _state: &ReconciliationState,
        ) -> Result<(), IndexError> {
            Ok(())
        }
        fn read_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<Option<ReconciliationRevision>, IndexError> {
            // Both callers must finish their read before either proceeds to write.
            self.read_barrier.wait();
            *self.reads_completed.lock().unwrap() += 1;
            Ok(None)
        }
        fn write_latest_revision(
            &self,
            _config_dir: &camino::Utf8Path,
            revision: ReconciliationRevision,
        ) -> Result<(), IndexError> {
            self.writes.lock().unwrap().push(revision.value());
            Ok(())
        }
        fn try_acquire_worker_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, IndexError> {
            Ok(ReconciliationLeaseOutcome::Acquired(Box::new(NoopLease)))
        }
        fn try_acquire_scheduling_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, IndexError> {
            Ok(ReconciliationLeaseOutcome::Acquired(Box::new(NoopLease)))
        }
    }

    let read_barrier = Arc::new(Barrier::new(2));
    let reads_completed = Arc::new(Mutex::new(0usize));
    let writes = Arc::new(Mutex::new(Vec::<u64>::new()));

    let port = Arc::new(BarrierPort {
        read_barrier: read_barrier.clone(),
        reads_completed: reads_completed.clone(),
        writes: writes.clone(),
    });

    let handles: Vec<_> = ["/dev/a", "/dev/b"]
        .into_iter()
        .map(|root| {
            let port = port.clone();
            std::thread::spawn(move || {
                let roots = vec![camino::Utf8PathBuf::from(root)];
                let dummy_dir = camino::Utf8Path::new("/test");
                super::persist_reconciliation_request_locked(port.as_ref(), dummy_dir, &roots)
            })
        })
        .collect();

    let mut success_count = 0usize;
    for handle in handles {
        let result = handle.join().expect("thread must not panic");
        if result
            .expect("persist must succeed under barrier control")
            .is_some()
        {
            success_count += 1;
        }
    }
    assert_eq!(
        success_count, 2,
        "both barrier-synchronized calls must report success"
    );
    assert_eq!(
        *reads_completed.lock().unwrap(),
        2,
        "both reads must complete through the barrier"
    );

    let mut recorded = writes.lock().unwrap().clone();
    recorded.sort();
    assert_eq!(
        recorded,
        vec![1, 1],
        "without lease serialization both callers compute the same next revision \
             and both record it; the lost update is deterministic"
    );
}

// The one-attempt lease primitive rejects mutation while the scheduling
// lease is held; revisions increment sequentially after release.
#[test]
fn reconciliation_scheduling_production_lease_contention_then_serializes_to_revisions_one_two() {
    use workhive_core::ports::{IndexReconciliationPort, ReconciliationLeaseOutcome};
    use workhive_infra::reconciliation::FsIndexReconciliation;

    let (_tmp, dir) = setup_recon_dir();
    let reconciliation = FsIndexReconciliation::new();

    // Hold the scheduling lease for the entire contention phase.
    let held_lease = match reconciliation
        .try_acquire_scheduling_lease(dir.as_ref())
        .unwrap()
    {
        ReconciliationLeaseOutcome::Acquired(l) => l,
        ReconciliationLeaseOutcome::Contended => panic!("first acquisition must succeed"),
    };

    // While the lease is held, a production scheduling attempt must
    // return the typed contention error and must not write a revision.
    let contention_err =
        try_schedule_reconciliation(dir.as_ref(), &[camino::Utf8PathBuf::from("/dev/a")])
            .unwrap_err();
    match contention_err {
        CliError::Runtime(message) => {
            assert_eq!(
                message, "reconciliation scheduling is already in progress",
                "production call while lease held must return exact contention message"
            );
        }
        other => panic!("expected CliError::Runtime, got: {:?}", other),
    }
    assert!(
        reconciliation
            .read_latest_revision(dir.as_ref())
            .unwrap()
            .is_none(),
        "contention must not write a revision file"
    );

    // Release the lease so subsequent calls can proceed.
    drop(held_lease);

    // First production call after release allocates revision 1.
    let first_scheduled =
        try_schedule_reconciliation(dir.as_ref(), &[camino::Utf8PathBuf::from("/dev/a")]).unwrap();
    assert!(first_scheduled, "first post-release call must schedule");
    assert_eq!(
        reconciliation
            .read_latest_revision(dir.as_ref())
            .unwrap()
            .unwrap()
            .value(),
        1,
        "first post-release call must allocate revision 1"
    );

    // Second production call allocates revision 2.
    let second_scheduled =
        try_schedule_reconciliation(dir.as_ref(), &[camino::Utf8PathBuf::from("/dev/b")]).unwrap();
    assert!(second_scheduled, "second post-release call must schedule");
    assert_eq!(
        reconciliation
            .read_latest_revision(dir.as_ref())
            .unwrap()
            .unwrap()
            .value(),
        2,
        "second post-release call must allocate revision 2"
    );
}

#[test]
fn reconciliation_scheduling_contention_returns_without_blocking() {
    use workhive_core::ports::{IndexReconciliationPort, ReconciliationLeaseOutcome};

    let (_tmp, dir) = setup_recon_dir();
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    let lease = reconciliation
        .try_acquire_scheduling_lease(dir.as_ref())
        .unwrap();
    assert!(matches!(lease, ReconciliationLeaseOutcome::Acquired(_)));

    let started = std::time::Instant::now();
    let error =
        try_schedule_reconciliation(dir.as_ref(), &[camino::Utf8PathBuf::from("/dev/projects")])
            .unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_millis(100));
    assert!(error.to_string().contains("already in progress"));
}

/// Test port that delegates to `FsIndexReconciliation` except for
/// `try_acquire_scheduling_lease`, which returns queued responses.
struct ConfigurableLeasePort {
    inner: workhive_infra::reconciliation::FsIndexReconciliation,
    lease_responses: std::cell::RefCell<Vec<LeaseResponse>>,
    lease_attempts: std::cell::Cell<usize>,
    revision_writes: std::cell::Cell<usize>,
    tracker: std::rc::Rc<EventTracker>,
    fail_write_state: std::cell::Cell<bool>,
}

enum LeaseResponse {
    Contended,
    Acquired,
    IoError(String),
}

/// Shared event log for ordered operation evidence.
struct EventTracker {
    events: std::cell::RefCell<Vec<&'static str>>,
}

impl EventTracker {
    fn new() -> std::rc::Rc<Self> {
        std::rc::Rc::new(Self {
            events: std::cell::RefCell::new(Vec::new()),
        })
    }

    fn record(&self, event: &'static str) {
        self.events.borrow_mut().push(event);
    }

    fn snapshot(&self) -> Vec<&'static str> {
        self.events.borrow().clone()
    }
}

/// Lease guard that appends `"lease_dropped"` to the event log on Drop.
struct TrackedLease {
    tracker: std::rc::Rc<EventTracker>,
}

impl std::fmt::Debug for TrackedLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TrackedLease")
    }
}

impl workhive_core::ports::ReconciliationLease for TrackedLease {}

impl Drop for TrackedLease {
    fn drop(&mut self) {
        self.tracker.record("lease_dropped");
    }
}

impl ConfigurableLeasePort {
    fn new(
        responses: Vec<LeaseResponse>,
        tracker: std::rc::Rc<EventTracker>,
    ) -> (tempfile::TempDir, camino::Utf8PathBuf, std::rc::Rc<Self>) {
        let (tmp, dir) = setup_recon_dir();
        let port = std::rc::Rc::new(Self {
            inner: workhive_infra::reconciliation::FsIndexReconciliation::new(),
            lease_responses: std::cell::RefCell::new(responses),
            lease_attempts: std::cell::Cell::new(0),
            revision_writes: std::cell::Cell::new(0),
            tracker,
            fail_write_state: std::cell::Cell::new(false),
        });
        (tmp, dir, port)
    }
}

impl workhive_core::ports::IndexReconciliationPort for ConfigurableLeasePort {
    fn read_state(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<
        workhive_core::domain::index_reconciliation::ReconciliationState,
        workhive_core::errors::IndexError,
    > {
        self.inner.read_state(config_dir)
    }

    fn write_state(
        &self,
        config_dir: &camino::Utf8Path,
        state: &workhive_core::domain::index_reconciliation::ReconciliationState,
    ) -> Result<(), workhive_core::errors::IndexError> {
        if self.fail_write_state.get() {
            return Err(workhive_core::errors::IndexError::WriteFailed {
                path: config_dir.join("reconciliation_state.json"),
                source: std::io::Error::other("simulated state write failure"),
            });
        }
        self.inner.write_state(config_dir, state)
    }

    fn read_latest_revision(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<
        Option<workhive_core::domain::index_reconciliation::ReconciliationRevision>,
        workhive_core::errors::IndexError,
    > {
        self.inner.read_latest_revision(config_dir)
    }

    fn write_latest_revision(
        &self,
        config_dir: &camino::Utf8Path,
        revision: workhive_core::domain::index_reconciliation::ReconciliationRevision,
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.revision_writes.set(self.revision_writes.get() + 1);
        self.tracker.record("revision_written");
        self.inner.write_latest_revision(config_dir, revision)
    }

    fn try_acquire_worker_lease(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<workhive_core::ports::ReconciliationLeaseOutcome, workhive_core::errors::IndexError>
    {
        self.inner.try_acquire_worker_lease(config_dir)
    }

    fn try_acquire_scheduling_lease(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<workhive_core::ports::ReconciliationLeaseOutcome, workhive_core::errors::IndexError>
    {
        self.lease_attempts.set(self.lease_attempts.get() + 1);
        let mut responses = self.lease_responses.borrow_mut();
        if responses.is_empty() {
            // After queue exhaustion, always return Contended.
            return Ok(workhive_core::ports::ReconciliationLeaseOutcome::Contended);
        }
        match responses.remove(0) {
            LeaseResponse::Contended => {
                Ok(workhive_core::ports::ReconciliationLeaseOutcome::Contended)
            }
            LeaseResponse::Acquired => {
                self.tracker.record("lease_acquired");
                Ok(workhive_core::ports::ReconciliationLeaseOutcome::Acquired(
                    Box::new(TrackedLease {
                        tracker: self.tracker.clone(),
                    }),
                ))
            }
            LeaseResponse::IoError(msg) => Err(workhive_core::errors::IndexError::WriteFailed {
                path: camino::Utf8PathBuf::from("/fake/scheduling.lock"),
                source: std::io::Error::other(msg),
            }),
        }
    }
}

#[test]
fn reconciliation_scheduling_bounded_contention_then_acquisition_writes_once() {
    let tracker = EventTracker::new();
    let (_tmp, dir, port) = ConfigurableLeasePort::new(
        vec![
            LeaseResponse::Contended,
            LeaseResponse::Contended,
            LeaseResponse::Contended,
            LeaseResponse::Acquired,
        ],
        tracker.clone(),
    );

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let config_writes = std::cell::Cell::new(0usize);
    let launches = std::cell::Cell::new(0usize);
    let no_wait = |_: std::time::Duration| {};

    let result = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || {
            config_writes.set(config_writes.get() + 1);
            tracker.record("config_written");
            Ok(())
        },
        |_dir| {
            launches.set(launches.get() + 1);
            tracker.record("launched");
            Ok(())
        },
        || {
            tracker.record("rollback");
            Ok(())
        },
    );
    assert!(
        result.is_ok(),
        "bounded retry must succeed after contention: {result:?}"
    );

    assert_eq!(
        port.lease_attempts.get(),
        4,
        "must attempt lease exactly once per response (3 contended + 1 acquired)"
    );
    assert_eq!(
        config_writes.get(),
        1,
        "exactly one config write after lease acquisition"
    );
    assert_eq!(
        port.revision_writes.get(),
        1,
        "exactly one revision write after lease acquisition"
    );
    assert_eq!(
        launches.get(),
        1,
        "exactly one launch after scheduled revision"
    );

    let events = tracker.snapshot();
    assert_eq!(
        events,
        vec![
            "lease_acquired",
            "config_written",
            "revision_written",
            "lease_dropped",
            "launched",
        ],
        "success must prove ordered: lease → config → revision → drop → launch"
    );
}

#[test]
fn reconciliation_scheduling_bounded_exhaustion_writes_nothing() {
    let tracker = EventTracker::new();
    let (_tmp, dir, port) = ConfigurableLeasePort::new(vec![], tracker.clone());

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let config_writes = std::cell::Cell::new(0usize);
    let launches = std::cell::Cell::new(0usize);
    let no_wait = |_: std::time::Duration| {};

    let err = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || {
            config_writes.set(config_writes.get() + 1);
            tracker.record("config_written");
            Ok(())
        },
        |_dir| {
            launches.set(launches.get() + 1);
            tracker.record("launched");
            Ok(())
        },
        || Ok(()),
    )
    .unwrap_err();

    match err {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "reconciliation scheduling is already in progress",
                "exhaustion must return exact contention message"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    assert_eq!(
        port.lease_attempts.get(),
        100,
        "must exhaust all 100 bounded attempts"
    );
    assert_eq!(config_writes.get(), 0, "exhaustion must not write config");
    assert_eq!(
        port.revision_writes.get(),
        0,
        "exhaustion must not write revision"
    );
    assert_eq!(launches.get(), 0, "exhaustion must not launch");
    assert!(
        tracker.snapshot().is_empty(),
        "exhaustion must produce zero events"
    );
}

#[test]
fn reconciliation_scheduling_bounded_io_error_writes_nothing() {
    let tracker = EventTracker::new();
    let (_tmp, dir, port) = ConfigurableLeasePort::new(
        vec![LeaseResponse::IoError("lease-fd-error".to_owned())],
        tracker.clone(),
    );

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let config_writes = std::cell::Cell::new(0usize);
    let launches = std::cell::Cell::new(0usize);
    let no_wait = |_: std::time::Duration| {};

    let err = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || {
            config_writes.set(config_writes.get() + 1);
            tracker.record("config_written");
            Ok(())
        },
        |_dir| {
            launches.set(launches.get() + 1);
            tracker.record("launched");
            Ok(())
        },
        || Ok(()),
    )
    .unwrap_err();

    match err {
        CliError::Runtime(ref msg) => {
            assert!(
                msg.starts_with("failed to acquire scheduling lock:"),
                "I/O error must propagate with context, got: {msg}"
            );
            assert!(
                msg.contains("failed to write project index"),
                "I/O error must propagate with context, got: {msg}"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    assert_eq!(
        port.lease_attempts.get(),
        1,
        "I/O error must stop after first attempt (no retry on I/O)"
    );
    assert_eq!(config_writes.get(), 0, "I/O error must not write config");
    assert_eq!(
        port.revision_writes.get(),
        0,
        "I/O error must not write revision"
    );
    assert_eq!(launches.get(), 0, "I/O error must not launch");
    assert!(
        tracker.snapshot().is_empty(),
        "I/O error must produce zero events"
    );
}

#[test]
fn reconciliation_scheduling_bounded_config_write_failure_drops_lease_without_revision_or_launch() {
    let tracker = EventTracker::new();
    let (_tmp, dir, port) =
        ConfigurableLeasePort::new(vec![LeaseResponse::Acquired], tracker.clone());

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let launches = std::cell::Cell::new(0usize);
    let no_wait = |_: std::time::Duration| {};

    let err = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || {
            tracker.record("config_write_failed");
            Err(CliError::Runtime("config write exploded".to_owned()))
        },
        |_dir| {
            launches.set(launches.get() + 1);
            tracker.record("launched");
            Ok(())
        },
        || Ok(()),
    )
    .unwrap_err();

    match err {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "config write exploded",
                "config write failure must propagate exact error"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    assert_eq!(
        port.lease_attempts.get(),
        1,
        "lease acquired on first attempt"
    );
    assert_eq!(
        port.revision_writes.get(),
        0,
        "config write failure must not reach revision write"
    );
    assert_eq!(launches.get(), 0, "config write failure must not launch");

    let events = tracker.snapshot();
    assert_eq!(
        events,
        vec!["lease_acquired", "config_write_failed", "lease_dropped"],
        "config write failure must prove: lease acquired → config failed → lease dropped, \
             with no revision or launch"
    );
}

#[test]
fn reconciliation_schedule_revision_failure_restores_global_toml() {
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_core::ports::{AppConfig, ProjectsConfig};
    use workhive_testkit::fakes::FakeConfig;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed config with existing trusted roots.
    let existing_toml = r#"language = "en"

[projects]
trusted_roots = ["/dev/original"]
"#;
    let fake_config = FakeConfig::new()
        .with_config_dir(state_dir.clone())
        .with_global(AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: vec![camino::Utf8PathBuf::from("/dev/original")],
            }),
            ..Default::default()
        });
    fake_config
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Block revision writes by creating a directory where the revision
    // file should go.
    std::fs::create_dir(state_dir.join("reconciliation_revision").as_std_path()).unwrap();

    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext::with_test_boundaries(Box::new(fake_config), launcher.clone());

    let roots = vec![camino::Utf8PathBuf::from("/dev/newroot")];
    let result = super::persist_trusted_roots_and_schedule(&roots, &ctx);

    // Revision write should fail.
    assert!(result.is_err(), "revision write must fail");

    // Original TOML must be restored.
    let restored = ctx.config.load_raw_toml(None, true).unwrap();
    assert!(
        restored.contains("/dev/original"),
        "original trusted roots must be restored after revision failure"
    );
    assert!(
        !restored.contains("/dev/newroot"),
        "new roots must not survive revision failure"
    );
}

#[test]
fn reconciliation_schedule_toml_capture_failure_aborts_before_any_side_effect() {
    use crate::context::{CommandContext, FakeProcessLauncher};
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_core::errors::ConfigError;
    use workhive_testkit::fakes::FakeConfig;

    // Wrapper to implement ConfigPort for Rc<RefCell<FakeConfig>>
    struct FakeConfigWrapper(Rc<RefCell<FakeConfig>>);
    impl workhive_core::ports::ConfigPort for FakeConfigWrapper {
        fn load_global(
            &self,
        ) -> Result<Option<workhive_core::ports::AppConfig>, workhive_core::errors::ConfigError>
        {
            self.0.borrow().load_global()
        }
        fn save_global(
            &self,
            config: &workhive_core::ports::AppConfig,
        ) -> Result<(), workhive_core::errors::ConfigError> {
            self.0.borrow().save_global(config)
        }
        fn load_project(
            &self,
            project_root: &camino::Utf8Path,
        ) -> Result<Option<workhive_core::ports::AppConfig>, workhive_core::errors::ConfigError>
        {
            self.0.borrow().load_project(project_root)
        }
        fn save_project(
            &self,
            project_root: &camino::Utf8Path,
            config: &workhive_core::ports::AppConfig,
        ) -> Result<(), workhive_core::errors::ConfigError> {
            self.0.borrow().save_project(project_root, config)
        }
        fn global_exists(&self) -> Result<bool, workhive_core::errors::ConfigError> {
            self.0.borrow().global_exists()
        }
        fn project_exists(
            &self,
            project_root: &camino::Utf8Path,
        ) -> Result<bool, workhive_core::errors::ConfigError> {
            self.0.borrow().project_exists(project_root)
        }
        fn load_raw_toml(
            &self,
            project_root: Option<&camino::Utf8Path>,
            global: bool,
        ) -> Result<String, workhive_core::errors::ConfigError> {
            self.0.borrow().load_raw_toml(project_root, global)
        }
        fn save_raw_toml(
            &self,
            project_root: Option<&camino::Utf8Path>,
            global: bool,
            contents: &str,
        ) -> Result<(), workhive_core::errors::ConfigError> {
            self.0
                .borrow()
                .save_raw_toml(project_root, global, contents)
        }
        fn load_env(
            &self,
            project_root: Option<&camino::Utf8Path>,
            global: bool,
        ) -> Result<Vec<(String, String)>, workhive_core::errors::ConfigError> {
            self.0.borrow().load_env(project_root, global)
        }
        fn save_env(
            &self,
            project_root: Option<&camino::Utf8Path>,
            global: bool,
            entries: &[(String, String)],
        ) -> Result<(), workhive_core::errors::ConfigError> {
            self.0.borrow().save_env(project_root, global, entries)
        }
        fn global_config_dir(
            &self,
        ) -> Result<camino::Utf8PathBuf, workhive_core::errors::ConfigError> {
            self.0.borrow().global_config_dir()
        }
    }

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed config with existing trusted roots.
    let existing_toml = r#"language = "en"

[projects]
trusted_roots = ["/dev/original"]
"#;
    let fake_config = Rc::new(RefCell::new(
        FakeConfig::new().with_config_dir(state_dir.clone()),
    ));
    fake_config
        .borrow()
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Record the initial call counts before injecting the error.
    let initial_save_global_count = fake_config.borrow().save_global_call_count();
    let initial_save_raw_toml_count = fake_config.borrow().save_raw_toml_call_count();

    // Inject a read failure for load_raw_toml to simulate capture failure.
    fake_config
        .borrow()
        .set_load_raw_toml_error(ConfigError::ReadFailed {
            path: camino::Utf8PathBuf::from("<fake-global-config>"),
            source: std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated read failure",
            ),
        });

    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext::with_test_boundaries(
        Box::new(FakeConfigWrapper(Rc::clone(&fake_config))),
        launcher.clone(),
    );

    let roots = vec![camino::Utf8PathBuf::from("/dev/newroot")];
    let result = super::persist_trusted_roots_and_schedule(&roots, &ctx);

    // TOML capture should fail.
    assert!(result.is_err(), "TOML capture must fail");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("failed to capture original TOML for rollback"),
        "error must mention TOML capture failure"
    );

    // Zero side effects across all boundaries:
    // 1. No config writes (save_global)
    assert_eq!(
        fake_config.borrow().save_global_call_count(),
        initial_save_global_count,
        "save_global must not be called when TOML capture fails"
    );

    // 2. No raw TOML writes (save_raw_toml)
    assert_eq!(
        fake_config.borrow().save_raw_toml_call_count(),
        initial_save_raw_toml_count,
        "save_raw_toml must not be called when TOML capture fails"
    );

    // 3. No launch calls
    assert_eq!(
        launcher.launch_reconciliation_call_count(),
        0,
        "launcher must not be called when TOML capture fails"
    );

    // 4. No revision/state writes - check that no state file was created
    let state_file = state_dir.join("reconciliation_state.json");
    assert!(
        !state_file.exists(),
        "reconciliation state file must not be created when TOML capture fails"
    );

    // 5. No latest-revision artifact was written
    let revision_file = state_dir.join("reconciliation_revision");
    assert!(
        !revision_file.exists(),
        "reconciliation revision file must not be created when TOML capture fails"
    );

    // Original TOML must remain unchanged.
    let unchanged = ctx.config.load_raw_toml(None, true).unwrap();
    assert!(
        unchanged.contains("/dev/original"),
        "original trusted roots must remain after TOML capture failure"
    );
    assert!(
        !unchanged.contains("/dev/newroot"),
        "new roots must not be written when TOML capture fails"
    );
}

#[test]
fn reconciliation_schedule_launch_failure_writes_failed_state() {
    let tracker = EventTracker::new();
    // Two lease responses: initial acquisition + reacquisition after launch failure.
    let (_tmp, dir, port) = ConfigurableLeasePort::new(
        vec![LeaseResponse::Acquired, LeaseResponse::Acquired],
        tracker.clone(),
    );

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let config_writes = std::cell::Cell::new(0usize);
    let launches = std::cell::Cell::new(0usize);
    let no_wait = |_: std::time::Duration| {};

    let err = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || {
            config_writes.set(config_writes.get() + 1);
            tracker.record("config_written");
            Ok(())
        },
        |_dir| {
            launches.set(launches.get() + 1);
            tracker.record("launch_failed");
            Err(CliError::Runtime("worker process exploded".to_owned()))
        },
        || Ok(()),
    )
    .unwrap_err();

    match err {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "worker process exploded",
                "launch failure must propagate exact error"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    assert_eq!(launches.get(), 1, "launch must be attempted once");
    assert_eq!(
        config_writes.get(),
        1,
        "config write must succeed before launch"
    );
    assert_eq!(
        port.revision_writes.get(),
        1,
        "revision must be persisted before launch"
    );

    // Verify Failed state was persisted with exact fields.
    let state = port
        .read_state(dir.as_ref())
        .expect("read_state must succeed");
    match state {
        workhive_core::domain::index_reconciliation::ReconciliationState::Failed {
            revision,
            fingerprint,
            failed_at,
            error_detail,
        } => {
            assert_eq!(
                revision.value(),
                1,
                "Failed state revision must equal scheduled revision (1)"
            );
            // Fingerprint must match what was computed from the roots.
            let expected_fp =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(&roots, None),
                );
            assert_eq!(
                fingerprint, expected_fp,
                "Failed state fingerprint must match scheduled fingerprint"
            );
            assert_eq!(
                error_detail, "worker process exploded",
                "Failed state error_detail must be exact launch error message"
            );
            // Timestamp must be valid (not zero/uninitialized).
            assert!(
                failed_at.duration_since(std::time::UNIX_EPOCH).is_ok(),
                "Failed state timestamp must be valid"
            );
        }
        other => panic!("expected Failed state, got: {other:?}"),
    }

    let events = tracker.snapshot();
    assert_eq!(
        events,
        vec![
            "lease_acquired",
            "config_written",
            "revision_written",
            "lease_dropped",
            "launch_failed",
            "lease_acquired",
            "lease_dropped",
        ],
        "success must prove ordered: lease → config → revision → drop → launch_failed → reacquire → drop"
    );
}

#[test]
fn reconciliation_schedule_launch_failure_and_state_write_failure_preserves_both_errors() {
    let tracker = EventTracker::new();
    // Two lease responses: initial acquisition + reacquisition after launch failure.
    let (_tmp, dir, port) = ConfigurableLeasePort::new(
        vec![LeaseResponse::Acquired, LeaseResponse::Acquired],
        tracker.clone(),
    );

    // Enable write_state failure to simulate state persistence failure.
    port.fail_write_state.set(true);

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let no_wait = |_: std::time::Duration| {};

    let err = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || Ok(()),
        |_dir| Err(CliError::Runtime("launch exploded".to_owned())),
        || Ok(()),
    )
    .unwrap_err();

    match err {
        CliError::Runtime(ref msg) => {
            assert!(
                msg.contains("launch exploded"),
                "must preserve launch error, got: {msg}"
            );
            assert!(
                msg.contains("failed to persist Failed state"),
                "must preserve state write error, got: {msg}"
            );
            assert!(
                msg.contains("simulated state write failure"),
                "must preserve exact underlying state write error text, got: {msg}"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }
}

/// NoOp test: when roots haven't changed, launch must not be called and
/// no Failed state must be written.
#[test]
fn reconciliation_schedule_noop_does_not_launch_or_write_failed() {
    let tracker = EventTracker::new();
    // Two lease responses: one for first call (schedules), one for second call (NoOp).
    let (_tmp, dir, port) = ConfigurableLeasePort::new(
        vec![LeaseResponse::Acquired, LeaseResponse::Acquired],
        tracker.clone(),
    );

    // First call schedules revision 1.
    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let no_wait = |_: std::time::Duration| {};
    let port_clone = port.clone();
    let result = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || Ok(()),
        |config_dir| {
            // Simulate launcher writing Completed state.
            let fingerprint =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(&roots, None),
                );
            let completed_state =
                workhive_core::domain::index_reconciliation::ReconciliationState::Completed {
                    revision:
                        workhive_core::domain::index_reconciliation::ReconciliationRevision::new(1),
                    fingerprint,
                    completed_at: std::time::SystemTime::now(),
                    discovered_count: 0,
                };
            port_clone
                .write_state(config_dir, &completed_state)
                .map_err(|e| CliError::Runtime(format!("failed to write completed state: {e}")))?;
            Ok(())
        },
        || Ok(()),
    );
    assert!(result.is_ok(), "first call must succeed");

    // Second call with same roots should be NoOp.
    let launches = std::cell::Cell::new(0usize);
    let result = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        port.as_ref(),
        dir.as_ref(),
        &no_wait,
        || Ok(()),
        |_dir| {
            launches.set(launches.get() + 1);
            Ok(())
        },
        || Ok(()),
    );
    assert!(result.is_ok(), "NoOp call must succeed");
    assert_eq!(launches.get(), 0, "NoOp must not call launch");

    // Verify state is not Failed.
    let state = port
        .read_state(dir.as_ref())
        .expect("read_state must succeed");
    assert!(!state.is_failed(), "NoOp must not write Failed state");
}

// Launch failure for revision N cannot overwrite revision N+1 state:
// failing_scheduler enters launch, newer_scheduler writes Completed revision 2,
// failing_scheduler's launch fails, final state belongs to newer_scheduler.
// Uses explicit channels for deterministic ordering without timing guesses.
#[test]
fn reconciliation_schedule_launch_failure_does_not_overwrite_newer_revision() {
    use std::sync::mpsc;
    use std::time::Duration;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = camino::Utf8PathBuf::from(tmp.path().to_str().unwrap());
    let reconciliation = FsIndexReconciliation::new();

    let failed_launch_roots = vec![camino::Utf8PathBuf::from("/dev/failed")];
    let newer_roots = vec![camino::Utf8PathBuf::from("/dev/newer")];
    let no_wait = |_: std::time::Duration| {};

    // Channel for failing_scheduler to signal launch entry.
    let (launch_entered_tx, launch_entered_rx) = mpsc::channel::<()>();
    // Channel for main thread to signal failing_scheduler to proceed.
    let (proceed_tx, proceed_rx) = mpsc::channel::<()>();

    let dir_clone = dir.clone();
    let failing_scheduler = std::thread::spawn(move || {
        let launches = std::cell::Cell::new(0usize);
        let result = super::persist_trusted_roots_and_schedule_inner(
            &failed_launch_roots,
            &reconciliation,
            dir_clone.as_ref(),
            &no_wait,
            || Ok(()),
            |_dir| {
                launches.set(launches.get() + 1);
                // Signal that launch has been entered.
                launch_entered_tx.send(()).expect("signal launch entered");
                // Wait for explicit release from main thread.
                proceed_rx
                    .recv_timeout(TEST_TIMEOUT)
                    .expect("wait for proceed signal (timeout indicates deadlock)");
                Err(CliError::Runtime("launch exploded".to_owned()))
            },
            || Ok(()),
        );
        (result, launches.get())
    });

    // Wait for failing_scheduler to enter launch closure (explicit signal, no sleep).
    launch_entered_rx
        .recv_timeout(TEST_TIMEOUT)
        .expect("wait for failing_scheduler to enter launch (timeout indicates deadlock)");

    // newer_scheduler: schedule revision 2 while failing_scheduler is blocked in launch.
    let newer_roots_clone = newer_roots.clone();
    let result = super::persist_trusted_roots_and_schedule_inner(
        &newer_roots,
        &reconciliation,
        dir.as_ref(),
        &no_wait,
        || Ok(()),
        |config_dir| {
            // Simulate launcher writing Completed state for revision 2.
            let fingerprint =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(&newer_roots_clone, None),
                );
            let completed_state =
                workhive_core::domain::index_reconciliation::ReconciliationState::Completed {
                    revision:
                        workhive_core::domain::index_reconciliation::ReconciliationRevision::new(2),
                    fingerprint,
                    completed_at: std::time::SystemTime::now(),
                    discovered_count: 0,
                };
            reconciliation
                .write_state(config_dir, &completed_state)
                .map_err(|e| CliError::Runtime(format!("failed to write completed state: {e}")))?;
            Ok(())
        },
        || Ok(()),
    );
    assert!(result.is_ok(), "newer_scheduler must succeed");
    assert_eq!(
        reconciliation
            .read_latest_revision(dir.as_ref())
            .unwrap()
            .unwrap()
            .value(),
        2,
        "newer_scheduler must allocate revision 2"
    );

    // Explicitly release failing_scheduler to continue (launch will fail).
    proceed_tx
        .send(())
        .expect("signal failing_scheduler to proceed");

    // Wait for failing_scheduler to complete.
    let (result, launches) = failing_scheduler
        .join()
        .expect("failing_scheduler must not panic");

    // failing_scheduler's launch must have executed exactly once.
    assert_eq!(
        launches, 1,
        "failing_scheduler launch must execute exactly once"
    );

    // failing_scheduler must fail with exact launch error.
    assert!(result.is_err(), "failing_scheduler must fail");
    match result.unwrap_err() {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "launch exploded",
                "failing_scheduler must propagate exact launch error"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    // Final state must belong to newer_scheduler (revision 2), not Failed for revision 1.
    let state = reconciliation
        .read_state(dir.as_ref())
        .expect("read_state must succeed");
    match state {
        workhive_core::domain::index_reconciliation::ReconciliationState::Failed {
            revision,
            ..
        } => {
            panic!(
                "final state must not be Failed; got Failed for revision {}",
                revision.value()
            );
        }
        workhive_core::domain::index_reconciliation::ReconciliationState::Completed {
            revision,
            fingerprint,
            ..
        } => {
            assert_eq!(
                revision.value(),
                2,
                "final state must be Completed for revision 2"
            );
            let expected_fingerprint =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(&newer_roots, None),
                );
            assert_eq!(
                fingerprint, expected_fingerprint,
                "final state fingerprint must belong to newer_scheduler"
            );
        }
        _ => {
            panic!("final state must be Completed for revision 2, got: {state:?}");
        }
    }
}

// Rollback blocks lease release: while rollback is held, a contender
// gets exact contention error and writes nothing; after release, a subsequent
// scheduler succeeds. Uses explicit channels for deterministic ordering.
#[test]
fn reconciliation_schedule_rollback_blocks_lease_until_complete() {
    use std::sync::mpsc;
    use std::time::Duration;
    use workhive_infra::reconciliation::FsIndexReconciliation;

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = camino::Utf8PathBuf::from(tmp.path().to_str().unwrap());
    let reconciliation = FsIndexReconciliation::new();

    let roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    let no_wait = |_: std::time::Duration| {};

    // Channel for failing_scheduler to signal rollback entry.
    let (rollback_entered_tx, rollback_entered_rx) = mpsc::channel::<()>();
    // Channel for main thread to signal rollback release.
    let (rollback_release_tx, rollback_release_rx) = mpsc::channel::<()>();

    let roots_clone = roots.clone();
    let dir_clone = dir.clone();
    let failing_scheduler = std::thread::spawn(move || {
        super::persist_trusted_roots_and_schedule_inner(
            &roots_clone,
            &reconciliation,
            dir_clone.as_ref(),
            &no_wait,
            || Err(CliError::Runtime("config write failed".to_owned())),
            |_dir| Ok(()),
            || {
                // Signal that rollback has been entered.
                rollback_entered_tx
                    .send(())
                    .expect("signal rollback entered");
                // Wait for explicit release from main thread.
                rollback_release_rx
                    .recv_timeout(TEST_TIMEOUT)
                    .expect("wait for rollback release (timeout indicates deadlock)");
                Ok(())
            },
        )
    });

    // Wait for failing_scheduler to enter rollback callback (explicit signal, no sleep).
    rollback_entered_rx
        .recv_timeout(TEST_TIMEOUT)
        .expect("wait for failing_scheduler to enter rollback (timeout indicates deadlock)");

    // Contender: try to acquire lease while failing_scheduler is blocked in rollback.
    // This should fail with exact contention error.
    let contender_config_writes = std::cell::Cell::new(0usize);
    let contender_launch_attempts = std::cell::Cell::new(0usize);
    let contender_rollback_attempts = std::cell::Cell::new(0usize);
    let contender_result = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        &reconciliation,
        dir.as_ref(),
        &no_wait,
        || {
            contender_config_writes.set(contender_config_writes.get() + 1);
            Ok(())
        },
        |_dir| {
            contender_launch_attempts.set(contender_launch_attempts.get() + 1);
            Ok(())
        },
        || {
            contender_rollback_attempts.set(contender_rollback_attempts.get() + 1);
            Ok(())
        },
    );

    // Contender should fail with exact contention error.
    assert!(
        contender_result.is_err(),
        "contender must fail while failing_scheduler holds lease in rollback"
    );
    match contender_result.unwrap_err() {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "reconciliation scheduling is already in progress",
                "contender must get exact contention error, got: {msg}"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    // Before releasing rollback, assert contender performed zero callback operations.
    assert_eq!(
        contender_config_writes.get(),
        0,
        "contender must not invoke write_config while rollback is blocked"
    );
    assert_eq!(
        contender_launch_attempts.get(),
        0,
        "contender must not invoke launch while rollback is blocked"
    );
    assert_eq!(
        contender_rollback_attempts.get(),
        0,
        "contender must not invoke rollback while rollback is blocked"
    );

    // Production revision store was not written by contender.
    let latest_revision = reconciliation
        .read_latest_revision(dir.as_ref())
        .expect("read_latest_revision must succeed");
    assert!(
        latest_revision.is_none(),
        "production revision store must remain empty while rollback is blocked"
    );

    // Explicitly release failing_scheduler to complete rollback.
    rollback_release_tx
        .send(())
        .expect("signal failing_scheduler to release rollback");

    // Wait for failing_scheduler to complete.
    let result = failing_scheduler
        .join()
        .expect("failing_scheduler must not panic");

    // failing_scheduler should fail with config write error.
    assert!(result.is_err(), "failing_scheduler must fail");
    match result.unwrap_err() {
        CliError::Runtime(ref msg) => {
            assert_eq!(
                msg, "config write failed",
                "failing_scheduler must propagate config write error"
            );
        }
        other => panic!("expected CliError::Runtime, got: {other:?}"),
    }

    // After failing_scheduler completes rollback and releases lease, subsequent scheduler should succeed.
    let launches = std::cell::Cell::new(0usize);
    let result = super::persist_trusted_roots_and_schedule_inner(
        &roots,
        &reconciliation,
        dir.as_ref(),
        &no_wait,
        || Ok(()),
        |_dir| {
            launches.set(launches.get() + 1);
            Ok(())
        },
        || Ok(()),
    );
    assert!(
        result.is_ok(),
        "subsequent scheduler must succeed after failing_scheduler releases lease"
    );
    assert_eq!(
        launches.get(),
        1,
        "subsequent scheduler must call launch after lease is released"
    );
}

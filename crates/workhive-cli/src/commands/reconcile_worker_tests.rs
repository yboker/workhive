use super::*;
use crate::error::CliError;
use camino::Utf8PathBuf;
use tempfile::TempDir;
use workhive_core::domain::index_reconciliation::{ReconciliationRevision, ReconciliationState};
use workhive_core::ports::{
    DiscoveredProject, IndexReconciliationPort, IndexState, ProjectIndexPort, ProjectSummary,
    ReconciliationLease, ReconciliationLeaseOutcome,
};
use workhive_infra::reconciliation::FsIndexReconciliation;
use workhive_testkit::fakes::{FakeDiscovery, FakeProjectIndex};

fn setup_config_dir() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn config_path(tmp: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("utf8")
}

fn make_discovered(name: &str, root: &str) -> DiscoveredProject {
    DiscoveredProject {
        project_name: name.to_owned(),
        project_root: Utf8PathBuf::from(root),
        instance_id: "ABCD".to_owned(),
        initialized: true,
    }
}

#[test]
fn index_reconciliation_worker_exits_when_no_revision() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];
    let discovery = FakeDiscovery::new();
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_ok());
    let state = index.list_projects().unwrap();
    assert!(matches!(state, IndexState::Loaded(v) if v.is_empty()));
}

#[test]
fn index_reconciliation_worker_publishes_on_current_revision() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("alpha", "/dev/projects/alpha"));
    discovery.add_project(make_discovered("beta", "/dev/projects/beta"));

    let index = FakeProjectIndex::new();
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_ok());

    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 2);
            assert!(projects.iter().any(|p| p.project_name == "alpha"));
            assert!(projects.iter().any(|p| p.project_name == "beta"));
        }
        _ => panic!("expected published index"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_completed());
}

#[test]
fn index_reconciliation_worker_discovery_failure_returns_err_and_persists_failed() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    let discovery = FakeDiscovery::new();
    discovery.set_error("disk I/O timeout");

    let index = FakeProjectIndex::new();
    index
        .replace_all(&[ProjectSummary {
            project_name: "prior".to_owned(),
            project_root: Utf8PathBuf::from("/dev/projects/prior"),
            config_path: Utf8PathBuf::from("/dev/projects/prior/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        }])
        .unwrap();

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(2))
        .unwrap();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_err(), "discovery failure must propagate as Err");

    // Prior index retained.
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].project_name, "prior");
        }
        _ => panic!("prior index must be retained on failure"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_failed());
}

#[test]
fn index_reconciliation_worker_publication_failure_returns_err_and_records_failed() {
    use workhive_core::errors::IndexError;

    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("new", "/dev/projects/new"));

    let index = FakeProjectIndex::new();
    index
        .replace_all(&[ProjectSummary {
            project_name: "existing".to_owned(),
            project_root: Utf8PathBuf::from("/dev/projects/existing"),
            config_path: Utf8PathBuf::from("/dev/projects/existing/.workhive/config.toml"),
            last_opened_at: None,
            last_updated_at: None,
        }])
        .unwrap();

    index.set_list_error(IndexError::WriteFailed {
        path: Utf8PathBuf::from("/fake/index.json"),
        source: std::io::Error::other("disk full"),
    });

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(3))
        .unwrap();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_err(), "publication failure must propagate as Err");

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_failed());
}

#[test]
fn index_reconciliation_worker_stale_revision_triggers_coalesce() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("proj", "/dev/projects/proj"));

    let index = FakeProjectIndex::new();

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(2))
        .unwrap();

    let result = scan_and_publish(
        dir.as_ref(),
        &|| {
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &roots.clone(),
                None,
            ))
        },
        &discovery,
        &index,
        ReconciliationRevision::new(1),
    );
    assert!(result.is_ok());

    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].project_name, "proj");
        }
        _ => panic!("coalesce must publish when revision is current"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_completed());
}

#[test]
fn index_reconciliation_worker_revision_read_failure_returns_err() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("proj", "/dev/projects/proj"));

    let index = FakeProjectIndex::new();
    // Write a corrupt revision file; the initial read fails immediately.
    let rev_path = dir.join("reconciliation_revision");
    std::fs::write(rev_path.as_std_path(), "not_a_number").unwrap();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(
        result.is_err(),
        "revision read failure must propagate as Err"
    );

    // No publication happened (error returned before scan).
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => assert!(projects.is_empty()),
        _ => panic!("no publication expected"),
    }
}

#[test]
fn index_reconciliation_worker_initial_revision_read_failure_returns_err() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);

    // Write a corrupt revision file before the cycle starts.
    let rev_path = dir.join("reconciliation_revision");
    std::fs::write(rev_path.as_std_path(), "garbage").unwrap();

    let roots = vec![Utf8PathBuf::from("/dev/projects")];
    let discovery = FakeDiscovery::new();
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(
        result.is_err(),
        "initial revision parse failure must be Err"
    );
}

/// Test discovery that returns a sequence of revisions on each call.
struct SequentialRevisionDiscovery {
    config_dir: Utf8PathBuf,
    revisions: std::cell::RefCell<Vec<ReconciliationRevision>>,
    projects: Vec<DiscoveredProject>,
}

impl workhive_core::ports::ProjectDiscoveryPort for SequentialRevisionDiscovery {
    fn discover_projects_report(
        &self,
        _roots: &[Utf8PathBuf],
    ) -> Result<workhive_core::ports::DiscoveryReport, workhive_core::errors::DiscoveryError> {
        let mut revs = self.revisions.borrow_mut();
        if let Some(rev) = revs.first().copied() {
            revs.remove(0);
            FsIndexReconciliation::new()
                .write_latest_revision(self.config_dir.as_ref(), rev)
                .unwrap();
        }
        Ok(workhive_core::ports::DiscoveryReport {
            projects: self.projects.clone(),
            warnings: vec![],
        })
    }
}

#[test]
fn index_reconciliation_worker_during_scan_stale_triggers_coalesce_to_latest() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    // First scan bumps to 3; second scan does not bump (convergence).
    let discovery = SequentialRevisionDiscovery {
        config_dir: dir.clone(),
        revisions: std::cell::RefCell::new(vec![ReconciliationRevision::new(3)]),
        projects: vec![make_discovered("alpha", "/dev/projects/alpha")],
    };
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_ok());

    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 1, "coalesced scan must publish");
        }
        _ => panic!("expected published index"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(
        recon.is_completed(),
        "final state must be Completed after coalesce"
    );
}

#[test]
fn index_reconciliation_worker_multi_update_converges_to_latest() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    // First scan bumps to 2, second scan bumps to 3, third scan stable.
    let discovery = SequentialRevisionDiscovery {
        config_dir: dir.clone(),
        revisions: std::cell::RefCell::new(vec![
            ReconciliationRevision::new(2),
            ReconciliationRevision::new(3),
        ]),
        projects: vec![make_discovered("proj", "/dev/projects/proj")],
    };
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_ok());

    // Only the final (revision 3) scan publishes.
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 1);
        }
        _ => panic!("expected published index"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_completed());
}

#[test]
fn index_reconciliation_worker_reloads_roots_for_new_revision() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    let roots = std::cell::RefCell::new(vec![Utf8PathBuf::from("/old")]);
    let loads = std::cell::Cell::new(0usize);
    let load_roots = || {
        loads.set(loads.get() + 1);
        Ok(workhive_core::domain::TrustedRootSet::from_raw(
            &roots.borrow().clone(),
            None,
        ))
    };
    struct ChangingDiscovery<'a> {
        config_dir: Utf8PathBuf,
        roots: &'a std::cell::RefCell<Vec<Utf8PathBuf>>,
        calls: std::cell::Cell<usize>,
    }
    impl workhive_core::ports::ProjectDiscoveryPort for ChangingDiscovery<'_> {
        fn discover_projects_report(
            &self,
            roots: &[Utf8PathBuf],
        ) -> Result<workhive_core::ports::DiscoveryReport, workhive_core::errors::DiscoveryError>
        {
            let call = self.calls.get();
            self.calls.set(call + 1);
            if call == 0 {
                assert_eq!(roots, &[Utf8PathBuf::from("/old")]);
                *self.roots.borrow_mut() = vec![Utf8PathBuf::from("/new")];
                FsIndexReconciliation::new()
                    .write_latest_revision(self.config_dir.as_ref(), ReconciliationRevision::new(2))
                    .unwrap();
            } else {
                assert_eq!(roots, &[Utf8PathBuf::from("/new")]);
            }
            Ok(workhive_core::ports::DiscoveryReport {
                projects: vec![make_discovered("project", &format!("{}/project", roots[0]))],
                warnings: vec![],
            })
        }
    }
    let discovery = ChangingDiscovery {
        config_dir: dir.clone(),
        roots: &roots,
        calls: std::cell::Cell::new(0),
    };
    let index = FakeProjectIndex::new();

    execute_worker_cycle_with_roots_loader(dir.as_ref(), &load_roots, &discovery, &index).unwrap();
    // load_roots is called: once per scan start + once before publish.
    // Iteration 1: load(/old), scan bumps rev, pre-publish load(/new) → stale discard.
    // Iteration 2: load(/new), scan, pre-publish load(/new) → publish.
    assert!(
        loads.get() >= 3,
        "expected at least 3 root loads, got {}",
        loads.get()
    );
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects[0].project_root.as_str(), "/new/project")
        }
        _ => panic!("expected the latest roots snapshot"),
    }
}

#[test]
fn index_reconciliation_worker_missing_revision_records_failed() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();
    struct RemovingDiscovery(Utf8PathBuf);
    impl workhive_core::ports::ProjectDiscoveryPort for RemovingDiscovery {
        fn discover_projects_report(
            &self,
            _: &[Utf8PathBuf],
        ) -> Result<workhive_core::ports::DiscoveryReport, workhive_core::errors::DiscoveryError>
        {
            std::fs::remove_file(self.0.join("reconciliation_revision").as_std_path()).unwrap();
            Ok(workhive_core::ports::DiscoveryReport {
                projects: vec![],
                warnings: vec![],
            })
        }
    }
    let index = FakeProjectIndex::new();
    let result = execute_worker_cycle(
        dir.as_ref(),
        &[Utf8PathBuf::from("/roots")],
        &RemovingDiscovery(dir.clone()),
        &index,
    );
    assert!(result.is_err());
    let state = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(matches!(state, ReconciliationState::Failed { .. }));
}

#[test]
fn index_reconciliation_worker_roots_reload_failure_records_failed() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();
    let load_roots = || Err(CliError::Runtime("config unavailable".to_owned()));
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle_with_roots_loader(
        dir.as_ref(),
        &load_roots,
        &FakeDiscovery::new(),
        &index,
    );
    assert!(result.is_err());
    let state = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(matches!(
        state,
        ReconciliationState::Failed { error_detail, .. }
            if error_detail.contains("config unavailable")
    ));
}

#[test]
fn index_reconciliation_worker_final_roots_reload_failure_records_failed() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();
    let calls = std::cell::Cell::new(0usize);
    let load_roots = || {
        let call = calls.get();
        calls.set(call + 1);
        if call == 0 {
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/roots")],
                None,
            ))
        } else {
            Err(CliError::Runtime("config became unreadable".to_owned()))
        }
    };
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle_with_roots_loader(
        dir.as_ref(),
        &load_roots,
        &FakeDiscovery::new(),
        &index,
    );
    assert!(result.is_err());
    let state = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(matches!(
        state,
        ReconciliationState::Failed { error_detail, .. }
            if error_detail.contains("config became unreadable")
    ));
}

#[test]
fn index_reconciliation_worker_holds_scheduling_lease_during_publication() {
    use workhive_core::ports::IndexReconciliationPort;

    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();
    let observed_contention = std::rc::Rc::new(std::cell::Cell::new(false));
    let hook_observation = observed_contention.clone();
    let hook_dir = dir.clone();
    let index = FakeProjectIndex::new();
    index.set_publish_hook(move || {
        let adapter = workhive_infra::reconciliation::FsIndexReconciliation::new();
        let outcome = adapter
            .try_acquire_scheduling_lease(hook_dir.as_ref())
            .unwrap();
        hook_observation.set(matches!(outcome, ReconciliationLeaseOutcome::Contended));
    });

    run_reconciliation_worker_with_roots_loader(
        dir.as_ref(),
        &|| {
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/roots")],
                None,
            ))
        },
        &FakeDiscovery::new(),
        &index,
        &reconciliation,
    )
    .unwrap();

    assert!(
        observed_contention.get(),
        "trusted-root mutations must not enter while a snapshot is published"
    );
    assert!(matches!(
        reconciliation
            .try_acquire_worker_lease(dir.as_ref())
            .unwrap(),
        ReconciliationLeaseOutcome::Acquired(_)
    ));
}

#[test]
fn index_reconciliation_worker_during_scan_stale_read_failure_records_failed() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let roots = vec![Utf8PathBuf::from("/dev/projects")];

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    // This discovery corrupts the revision file during scan so the
    // post-scan re-read fails.
    struct CorruptingDiscovery {
        config_dir: Utf8PathBuf,
        projects: Vec<DiscoveredProject>,
    }
    impl workhive_core::ports::ProjectDiscoveryPort for CorruptingDiscovery {
        fn discover_projects_report(
            &self,
            _roots: &[Utf8PathBuf],
        ) -> Result<workhive_core::ports::DiscoveryReport, workhive_core::errors::DiscoveryError>
        {
            let rev_path = self.config_dir.join("reconciliation_revision");
            std::fs::write(rev_path.as_std_path(), "corrupted").unwrap();
            Ok(workhive_core::ports::DiscoveryReport {
                projects: self.projects.clone(),
                warnings: vec![],
            })
        }
    }

    let discovery = CorruptingDiscovery {
        config_dir: dir.clone(),
        projects: vec![make_discovered("beta", "/dev/projects/beta")],
    };
    let index = FakeProjectIndex::new();

    let result = execute_worker_cycle(dir.as_ref(), &roots, &discovery, &index);
    assert!(result.is_err(), "revision read failure must propagate");

    // No publication on failure.
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => assert!(projects.is_empty()),
        _ => panic!("no publication expected"),
    }

    let recon = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(recon.is_failed(), "must record Failed state");
}

#[test]
fn index_reconciliation_worker_roots_change_during_scan_discards_stale() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);

    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    let call_count = std::cell::Cell::new(0usize);
    let load_roots = || {
        let n = call_count.get();
        call_count.set(n + 1);
        if n == 0 {
            // First load: scan starts with old roots.
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/old")],
                None,
            ))
        } else {
            // Subsequent loads: roots changed.
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/new")],
                None,
            ))
        }
    };

    let discovery = FakeDiscovery::new();
    discovery.add_project(make_discovered("proj", "/new/proj"));

    let index = FakeProjectIndex::new();

    scan_and_publish(
        dir.as_ref(),
        &load_roots,
        &discovery,
        &index,
        ReconciliationRevision::new(1),
    )
    .unwrap();

    // The scan used /old roots, but at publish-time /new roots have
    // a different fingerprint, so the first result is discarded.
    // The second scan uses /new roots and publishes.
    match index.list_projects().unwrap() {
        IndexState::Loaded(projects) => {
            assert_eq!(projects.len(), 1);
            assert_eq!(projects[0].project_name, "proj");
        }
        _ => panic!("expected published index with new roots"),
    }
    assert!(call_count.get() >= 2, "must reload roots at least twice");
}

/// Thread-safe discovery counter for cross-thread contention tests.
struct CountingDiscovery {
    projects: Vec<DiscoveredProject>,
    calls: std::sync::atomic::AtomicUsize,
}

impl CountingDiscovery {
    fn new(projects: Vec<DiscoveredProject>) -> Self {
        Self {
            projects,
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl workhive_core::ports::ProjectDiscoveryPort for CountingDiscovery {
    fn discover_projects_report(
        &self,
        _roots: &[Utf8PathBuf],
    ) -> Result<workhive_core::ports::DiscoveryReport, workhive_core::errors::DiscoveryError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(workhive_core::ports::DiscoveryReport {
            projects: self.projects.clone(),
            warnings: vec![],
        })
    }
}

/// Thread-safe project index for cross-thread contention tests.
/// Records publish calls and final project list.
struct SyncProjectIndex {
    published: std::sync::atomic::AtomicUsize,
    projects: std::sync::Mutex<Vec<ProjectSummary>>,
}

impl SyncProjectIndex {
    fn new() -> Self {
        Self {
            published: std::sync::atomic::AtomicUsize::new(0),
            projects: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn publish_count(&self) -> usize {
        self.published.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl ProjectIndexPort for SyncProjectIndex {
    fn list_projects(&self) -> Result<IndexState, workhive_core::errors::IndexError> {
        Ok(IndexState::Loaded(self.projects.lock().unwrap().clone()))
    }
    fn read_paired_state(
        &self,
    ) -> Result<workhive_core::ports::PairedIndexState, workhive_core::errors::IndexError> {
        let state = self.list_projects()?;
        // Use a zeroed digest for test purposes
        let token = workhive_core::ports::IndexSnapshotToken::from_digest([0u8; 32]);
        Ok(workhive_core::ports::PairedIndexState { state, token })
    }
    fn capture_snapshot_token(
        &self,
    ) -> Result<workhive_core::ports::IndexSnapshotToken, workhive_core::errors::IndexError> {
        Ok(workhive_core::ports::IndexSnapshotToken::from_digest(
            [0u8; 32],
        ))
    }
    fn upsert_project(
        &self,
        _summary: &ProjectSummary,
    ) -> Result<(), workhive_core::errors::IndexError> {
        Ok(())
    }
    fn remove_project(
        &self,
        _project_root: &camino::Utf8PathBuf,
    ) -> Result<(), workhive_core::errors::IndexError> {
        Ok(())
    }
    fn clear(&self) -> Result<(), workhive_core::errors::IndexError> {
        Ok(())
    }
    fn replace_all(
        &self,
        projects: &[ProjectSummary],
    ) -> Result<(), workhive_core::errors::IndexError> {
        *self.projects.lock().unwrap() = projects.to_vec();
        Ok(())
    }
    fn publish_snapshot_conditional(
        &self,
        projects: &[ProjectSummary],
        _expected_token: workhive_core::ports::IndexSnapshotToken,
    ) -> Result<workhive_core::ports::SnapshotPublishOutcome, workhive_core::errors::IndexError>
    {
        self.published
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.projects.lock().unwrap() = projects.to_vec();
        Ok(workhive_core::ports::SnapshotPublishOutcome::Published)
    }
}

#[test]
fn index_reconciliation_worker_contention_does_not_repeat_discovery() {
    use workhive_core::ports::IndexReconciliationPort;

    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    // Test thread holds the scheduling lease BEFORE the worker starts.
    // The worker will run discovery once, then encounter contention on
    // the scheduling lease. Contention retries must NOT reload roots,
    // rewrite Updating, or re-run discovery while waiting.
    let holder = reconciliation
        .try_acquire_scheduling_lease(dir.as_ref())
        .unwrap();
    let holder_lease = match holder {
        workhive_core::ports::ReconciliationLeaseOutcome::Acquired(l) => l,
        _ => unreachable!(),
    };

    let discovery = std::sync::Arc::new(CountingDiscovery::new(vec![make_discovered(
        "proj",
        "/dev/proj",
    )]));
    let index = std::sync::Arc::new(SyncProjectIndex::new());
    let dir_clone = dir.clone();
    let discovery_for_worker = discovery.clone();
    let index_for_worker = index.clone();
    let worker_handle = std::thread::spawn(move || {
        run_reconciliation_worker_with_roots_loader(
            dir_clone.as_ref(),
            &|| {
                Ok(workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ))
            },
            discovery_for_worker.as_ref(),
            index_for_worker.as_ref(),
            &workhive_infra::reconciliation::FsIndexReconciliation::new(),
        )
    });

    // Wait until the worker has completed its single discovery pass.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while discovery.call_count() == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        discovery.call_count() >= 1,
        "worker must have run discovery before lease release"
    );

    // Release the scheduling lease so the worker's bounded retry loop
    // can acquire it and publish.
    drop(holder_lease);

    worker_handle.join().unwrap().unwrap();

    assert_eq!(
        discovery.call_count(),
        1,
        "contention retry must NOT re-run discovery; exactly one scan must survive"
    );
    assert_eq!(
        index.publish_count(),
        1,
        "must publish the single scan result exactly once"
    );
}

#[test]
fn index_reconciliation_worker_lease_exhaustion_persists_failed_and_publishes_nothing() {
    use workhive_core::ports::IndexReconciliationPort;

    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    let reconciliation = workhive_infra::reconciliation::FsIndexReconciliation::new();
    reconciliation
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    // Hold the scheduling lease for the entire worker lifetime.
    // The worker will run discovery once, then retry lease acquisition
    // until exhaustion. It must persist Failed, publish zero, and
    // return a typed error.
    let holder = reconciliation
        .try_acquire_scheduling_lease(dir.as_ref())
        .unwrap();
    let holder_lease = match holder {
        workhive_core::ports::ReconciliationLeaseOutcome::Acquired(l) => l,
        _ => unreachable!(),
    };

    let discovery = std::sync::Arc::new(CountingDiscovery::new(vec![make_discovered(
        "proj",
        "/dev/proj",
    )]));
    let index = std::sync::Arc::new(SyncProjectIndex::new());
    let dir_clone = dir.clone();
    let discovery_for_worker = discovery.clone();
    let index_for_worker = index.clone();
    let worker_handle = std::thread::spawn(move || {
        run_reconciliation_worker_with_roots_loader(
            dir_clone.as_ref(),
            &|| {
                Ok(workhive_core::domain::TrustedRootSet::from_raw(
                    &[Utf8PathBuf::from("/dev")],
                    None,
                ))
            },
            discovery_for_worker.as_ref(),
            index_for_worker.as_ref(),
            &workhive_infra::reconciliation::FsIndexReconciliation::new(),
        )
    });

    let result = worker_handle.join().unwrap();
    assert!(
        result.is_err(),
        "lease exhaustion must produce a typed error, not silent success"
    );

    // Discovery must have happened exactly once: the exhaustion retries
    // must not re-run the scan.
    assert_eq!(
        discovery.call_count(),
        1,
        "exhaustion retry must NOT re-run discovery"
    );

    // No publication: publish count must be zero.
    assert_eq!(
        index.publish_count(),
        0,
        "exhaustion must not publish any snapshot"
    );

    // Failed state persisted.
    let state = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(
        matches!(state, ReconciliationState::Failed { .. }),
        "exhaustion must persist Failed state"
    );

    drop(holder_lease);
}

/// Test port that delegates to `FsIndexReconciliation` except
/// `try_acquire_scheduling_lease`, which returns a fixed I/O error.
struct IoFailingSchedulingLeasePort {
    inner: workhive_infra::reconciliation::FsIndexReconciliation,
    error_message: &'static str,
}

impl IndexReconciliationPort for IoFailingSchedulingLeasePort {
    fn read_state(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationState, workhive_core::errors::IndexError> {
        self.inner.read_state(config_dir)
    }
    fn write_state(
        &self,
        config_dir: &camino::Utf8Path,
        state: &ReconciliationState,
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.write_state(config_dir, state)
    }
    fn read_latest_revision(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<Option<ReconciliationRevision>, workhive_core::errors::IndexError> {
        self.inner.read_latest_revision(config_dir)
    }
    fn write_latest_revision(
        &self,
        config_dir: &camino::Utf8Path,
        revision: ReconciliationRevision,
    ) -> Result<(), workhive_core::errors::IndexError> {
        self.inner.write_latest_revision(config_dir, revision)
    }
    fn try_acquire_worker_lease(
        &self,
        config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
        self.inner.try_acquire_worker_lease(config_dir)
    }
    fn try_acquire_scheduling_lease(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
        Err(workhive_core::errors::IndexError::WriteFailed {
            path: camino::Utf8PathBuf::from("/fake/scheduling.lock"),
            source: std::io::Error::other(self.error_message),
        })
    }
}

#[test]
fn index_reconciliation_worker_scheduling_lease_io_error_persists_failed_with_original_context() {
    let tmp = setup_config_dir();
    let dir = config_path(&tmp);
    FsIndexReconciliation::new()
        .write_latest_revision(dir.as_ref(), ReconciliationRevision::new(1))
        .unwrap();

    let discovery = CountingDiscovery::new(vec![make_discovered("proj", "/dev/proj")]);
    let index = SyncProjectIndex::new();
    let port = IoFailingSchedulingLeasePort {
        inner: workhive_infra::reconciliation::FsIndexReconciliation::new(),
        error_message: "simulated lockfile open failure",
    };

    let result = run_reconciliation_worker_with_roots_loader(
        dir.as_ref(),
        &|| {
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/dev")],
                None,
            ))
        },
        &discovery,
        &index,
        &port,
    );

    assert!(result.is_err(), "lease I/O failure must propagate as Err");
    let err_message = result.unwrap_err().to_string();
    assert!(
        err_message.contains("failed to acquire publication lock"),
        "error must contain the publication-lock I/O context, got: {err_message}"
    );

    // Discovery ran exactly once (before the lease attempt failed).
    assert_eq!(
        discovery.call_count(),
        1,
        "lease I/O failure must not trigger additional discovery"
    );

    // Zero publications.
    assert_eq!(
        index.publish_count(),
        0,
        "lease I/O failure must not publish"
    );

    // Failed state persisted with the original error context.
    let state = FsIndexReconciliation::new()
        .read_state(dir.as_ref())
        .unwrap();
    assert!(
        matches!(&state, ReconciliationState::Failed { error_detail, .. }
                if error_detail.contains("failed to acquire publication lock")),
        "persisted state must be Failed with the original I/O context, got: {state:?}"
    );
}

/// `IndexReconciliationPort` whose `try_acquire_scheduling_lease` and
/// `write_state` (only for the Failed state write after lease I/O)
/// both return I/O errors.
struct DoubleIoFailingPort {
    write_state_call_count: std::cell::RefCell<usize>,
}

impl IndexReconciliationPort for DoubleIoFailingPort {
    fn read_state(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationState, workhive_core::errors::IndexError> {
        Ok(ReconciliationState::Idle)
    }
    fn write_state(
        &self,
        _config_dir: &camino::Utf8Path,
        state: &ReconciliationState,
    ) -> Result<(), workhive_core::errors::IndexError> {
        // Only the second write_state call (the Failed persist after
        // lease I/O error) fails. The first (Updating) succeeds so the
        // worker reaches the scheduling-lease path.
        let mut count = self.write_state_call_count.borrow_mut();
        *count += 1;
        if matches!(state, ReconciliationState::Failed { .. }) {
            Err(workhive_core::errors::IndexError::WriteFailed {
                path: camino::Utf8PathBuf::from("/fake/state.json"),
                source: std::io::Error::other("state-write-error"),
            })
        } else {
            Ok(())
        }
    }
    fn read_latest_revision(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<Option<ReconciliationRevision>, workhive_core::errors::IndexError> {
        Ok(Some(ReconciliationRevision::new(1)))
    }
    fn write_latest_revision(
        &self,
        _config_dir: &camino::Utf8Path,
        _revision: ReconciliationRevision,
    ) -> Result<(), workhive_core::errors::IndexError> {
        Ok(())
    }
    fn try_acquire_worker_lease(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
        struct L;
        impl std::fmt::Debug for L {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("L")
            }
        }
        impl ReconciliationLease for L {}
        Ok(ReconciliationLeaseOutcome::Acquired(Box::new(L)))
    }
    fn try_acquire_scheduling_lease(
        &self,
        _config_dir: &camino::Utf8Path,
    ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
        Err(workhive_core::errors::IndexError::WriteFailed {
            path: camino::Utf8PathBuf::from("/fake/scheduling.lock"),
            source: std::io::Error::other("lease-fd-error"),
        })
    }
}

#[test]
fn index_reconciliation_worker_scheduling_lease_io_and_failed_write_both_preserved() {
    let discovery = CountingDiscovery::new(vec![make_discovered("proj", "/dev/proj")]);
    let index = SyncProjectIndex::new();
    let port = DoubleIoFailingPort {
        write_state_call_count: std::cell::RefCell::new(0),
    };

    let result = run_reconciliation_worker_with_roots_loader(
        camino::Utf8Path::new("/fake-config"),
        &|| {
            Ok(workhive_core::domain::TrustedRootSet::from_raw(
                &[Utf8PathBuf::from("/dev")],
                None,
            ))
        },
        &discovery,
        &index,
        &port,
    );

    assert!(result.is_err(), "double failure must propagate as Err");
    let err_message = result.unwrap_err().to_string();
    assert!(
        err_message.contains("failed to acquire publication lock"),
        "error must contain the original lease I/O context, got: {err_message}"
    );
    assert!(
        err_message.contains("failed to persist Failed state"),
        "error must also contain the Failed-write I/O context, got: {err_message}"
    );
    assert_eq!(discovery.call_count(), 1);
    assert_eq!(index.publish_count(), 0);
}

#[test]
fn cmd_internal_reconcile_config_dir_mismatch_aborts_before_any_side_effect() {
    use crate::context::CommandContext;
    use workhive_testkit::fakes::FakeConfig;

    let canonical_tmp = tempfile::tempdir().expect("tempdir");
    let canonical_dir =
        Utf8PathBuf::from_path_buf(canonical_tmp.path().to_path_buf()).expect("utf8");

    let fake_config = FakeConfig::new().with_config_dir(canonical_dir.clone());
    let ctx = CommandContext::with_test_boundaries(
        Box::new(fake_config),
        crate::context::FakeProcessLauncher::new(),
    );

    // Provide a different directory than the canonical one
    let worker_tmp = tempfile::tempdir().expect("tempdir");
    let worker_dir = Utf8PathBuf::from_path_buf(worker_tmp.path().to_path_buf()).expect("utf8");

    let result = cmd_internal_reconcile(worker_dir.as_str(), &ctx);

    // Must fail with mismatch error
    assert!(result.is_err(), "mismatched config_dir must return Err");
    let err_message = result.unwrap_err().to_string();
    assert!(
        err_message.contains("config directory mismatch"),
        "error must mention mismatch, got: {err_message}"
    );

    // Zero side effects on mismatch: no lock acquired, no state written
    let state = FsIndexReconciliation::new().read_state(worker_dir.as_ref());
    assert!(
        state.is_err() || matches!(state.unwrap(), ReconciliationState::Idle),
        "worker must not write any state on mismatch"
    );
}

#[test]
fn worker_kernel_records_exact_acquisitions_and_waits_without_sleeping() {
    use super::scan_and_publish_with_reconciliation_kernel;
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_core::ports::ReconciliationLeaseOutcome;

    let dir = tempfile::TempDir::new().unwrap();
    let config_dir = camino::Utf8Path::from_path(dir.path()).unwrap();

    let reconciliation = FsIndexReconciliation::new();
    let initial_revision = ReconciliationRevision::new(1);
    reconciliation
        .write_latest_revision(config_dir, initial_revision)
        .unwrap();

    struct AlwaysContendingPort {
        inner: FsIndexReconciliation,
        attempt_count: RefCell<usize>,
        state_write_count: RefCell<usize>,
        states_written: RefCell<Vec<ReconciliationState>>,
    }

    impl AlwaysContendingPort {
        fn new(inner: FsIndexReconciliation) -> Self {
            Self {
                inner,
                attempt_count: RefCell::new(0),
                state_write_count: RefCell::new(0),
                states_written: RefCell::new(Vec::new()),
            }
        }

        fn attempt_count(&self) -> usize {
            *self.attempt_count.borrow()
        }

        fn state_write_count(&self) -> usize {
            *self.state_write_count.borrow()
        }

        fn states_written(&self) -> Vec<ReconciliationState> {
            self.states_written.borrow().clone()
        }
    }

    impl IndexReconciliationPort for AlwaysContendingPort {
        fn read_state(
            &self,
            config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationState, workhive_core::errors::IndexError> {
            self.inner.read_state(config_dir)
        }
        fn write_state(
            &self,
            config_dir: &camino::Utf8Path,
            state: &ReconciliationState,
        ) -> Result<(), workhive_core::errors::IndexError> {
            *self.state_write_count.borrow_mut() += 1;
            self.states_written.borrow_mut().push(state.clone());
            self.inner.write_state(config_dir, state)
        }
        fn read_latest_revision(
            &self,
            config_dir: &camino::Utf8Path,
        ) -> Result<Option<ReconciliationRevision>, workhive_core::errors::IndexError> {
            self.inner.read_latest_revision(config_dir)
        }
        fn write_latest_revision(
            &self,
            config_dir: &camino::Utf8Path,
            revision: ReconciliationRevision,
        ) -> Result<(), workhive_core::errors::IndexError> {
            self.inner.write_latest_revision(config_dir, revision)
        }
        fn try_acquire_worker_lease(
            &self,
            config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            self.inner.try_acquire_worker_lease(config_dir)
        }
        fn try_acquire_scheduling_lease(
            &self,
            _config_dir: &camino::Utf8Path,
        ) -> Result<ReconciliationLeaseOutcome, workhive_core::errors::IndexError> {
            let mut attempt = self.attempt_count.borrow_mut();
            *attempt += 1;
            Ok(ReconciliationLeaseOutcome::Contended)
        }
    }

    let port = AlwaysContendingPort::new(reconciliation);
    let index = workhive_testkit::fakes::FakeProjectIndex::new();
    let discovery = workhive_testkit::fakes::FakeDiscovery::new();
    discovery.add_project(make_discovered("test-project", "/test/project"));

    let load_roots_count = Rc::new(RefCell::new(0));
    let load_roots_count_clone = load_roots_count.clone();

    let wait_calls = Rc::new(RefCell::new(Vec::new()));
    let wait_calls_clone = wait_calls.clone();

    let result = scan_and_publish_with_reconciliation_kernel(
        ScanAndPublishInput {
            config_dir,
            load_roots: &move || {
                *load_roots_count_clone.borrow_mut() += 1;
                Ok(workhive_core::domain::TrustedRootSet::from_raw(
                    &[camino::Utf8PathBuf::from("/test")],
                    None,
                ))
            },
            discovery: &discovery,
            index: &index as &dyn ProjectIndexPort,
            target_revision: initial_revision,
            reconciliation: &port as &dyn IndexReconciliationPort,
            worker_lease: None,
        },
        &move |duration| {
            wait_calls_clone.borrow_mut().push(duration);
        },
    );

    assert!(result.is_err(), "kernel must fail on contention exhaustion");
    let err = result.unwrap_err();
    match &err {
        crate::error::CliError::Runtime(msg) => {
            assert_eq!(
                msg, "publication coordination lease remained contended",
                "error message must match exactly"
            );
        }
        other => panic!("Expected CliError::Runtime, got {:?}", other),
    }

    assert_eq!(
        port.attempt_count(),
        100,
        "kernel must attempt exactly 100 acquisitions"
    );

    assert_eq!(
        wait_calls.borrow().len(),
        99,
        "kernel must wait exactly 99 times"
    );

    for (idx, wait) in wait_calls.borrow().iter().enumerate() {
        assert_eq!(
            *wait,
            std::time::Duration::from_millis(5),
            "wait {} must be exactly 5ms",
            idx
        );
    }

    assert_eq!(
        *load_roots_count.borrow(),
        1,
        "kernel must load roots exactly once"
    );

    assert_eq!(
        port.state_write_count(),
        2,
        "kernel must write state exactly twice (Updating + Failed)"
    );

    assert_eq!(
        discovery.call_count(),
        1,
        "kernel must run discovery exactly once"
    );

    let states = port.states_written();
    assert_eq!(states.len(), 2, "must have exactly 2 state writes");

    // First state must be Updating
    match &states[0] {
        ReconciliationState::Updating {
            revision,
            fingerprint,
            ..
        } => {
            assert_eq!(
                revision, &initial_revision,
                "Updating state must carry target revision"
            );
            let expected_fingerprint =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(
                        &[camino::Utf8PathBuf::from("/test")],
                        None,
                    ),
                );
            assert_eq!(
                fingerprint, &expected_fingerprint,
                "Updating state must carry exact fingerprint from configured TrustedRootSet"
            );
        }
        other => panic!("First state must be Updating, got {:?}", other),
    }

    // Second state must be Failed
    match &states[1] {
        ReconciliationState::Failed {
            revision,
            fingerprint,
            error_detail,
            ..
        } => {
            assert_eq!(
                revision, &initial_revision,
                "Failed state must carry target revision"
            );
            let expected_fingerprint =
                workhive_core::domain::index_reconciliation::RootsFingerprint::from_trusted_roots(
                    &workhive_core::domain::TrustedRootSet::from_raw(
                        &[camino::Utf8PathBuf::from("/test")],
                        None,
                    ),
                );
            assert_eq!(
                fingerprint, &expected_fingerprint,
                "Failed state must carry exact fingerprint from configured TrustedRootSet"
            );
            assert_eq!(
                error_detail, "publication coordination lease remained contended",
                "Failed state must preserve exact error detail"
            );
        }
        other => panic!("Second state must be Failed, got {:?}", other),
    }
}

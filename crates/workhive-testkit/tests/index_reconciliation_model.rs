//! Tests for `workhive_core::domain::index_reconciliation` model.
//!
//! Exercises fingerprint computation, revision ordering, scheduling decisions,
//! and stale-publication prevention as pure logic without I/O.

use camino::Utf8PathBuf;
use workhive_core::domain::TrustedRootSet;
use workhive_core::domain::index_reconciliation::{
    PublicationDecision, ReconciliationDecision, ReconciliationRevision, ReconciliationState,
    RootsFingerprint, decide_publication, decide_reconciliation,
};

// ── Fingerprint ──

#[test]
fn index_reconciliation_model_fingerprint_same_roots_same_fingerprint() {
    let roots_a = TrustedRootSet::from_raw(
        &[Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")],
        None,
    );
    let roots_b = TrustedRootSet::from_raw(
        &[Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")],
        None,
    );
    assert_eq!(
        RootsFingerprint::from_trusted_roots(&roots_a),
        RootsFingerprint::from_trusted_roots(&roots_b),
    );
}

#[test]
fn index_reconciliation_model_fingerprint_reorder_same_fingerprint() {
    let roots_a = TrustedRootSet::from_raw(
        &[Utf8PathBuf::from("/dev/b"), Utf8PathBuf::from("/dev/a")],
        None,
    );
    let roots_b = TrustedRootSet::from_raw(
        &[Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")],
        None,
    );
    assert_eq!(
        RootsFingerprint::from_trusted_roots(&roots_a),
        RootsFingerprint::from_trusted_roots(&roots_b),
        "reordering must not change the fingerprint"
    );
}

#[test]
fn index_reconciliation_model_fingerprint_different_roots_different_fingerprint() {
    let roots_a = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/a")], None);
    let roots_b = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/b")], None);
    assert_ne!(
        RootsFingerprint::from_trusted_roots(&roots_a),
        RootsFingerprint::from_trusted_roots(&roots_b),
    );
}

// ── Revision ordering ──

#[test]
fn index_reconciliation_model_revision_ordering() {
    let r1 = ReconciliationRevision::new(1);
    let r2 = ReconciliationRevision::new(2);
    assert!(r2.is_newer_than(&r1));
    assert!(!r1.is_newer_than(&r2));
    assert!(!r1.is_newer_than(&r1));
}

// ── Scheduling decisions ──

#[test]
fn index_reconciliation_model_schedule_on_first_mutation() {
    let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/projects")], None);
    let decision = decide_reconciliation(&roots, None, None).unwrap();
    assert!(
        matches!(decision, ReconciliationDecision::Schedule { .. }),
        "first mutation must schedule reconciliation"
    );
    if let ReconciliationDecision::Schedule { revision, .. } = decision {
        assert_eq!(revision.value(), 1);
    }
}

#[test]
fn index_reconciliation_model_schedule_on_changed_fingerprint() {
    let old_roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/a")], None);
    let new_roots = TrustedRootSet::from_raw(
        &[Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("/dev/b")],
        None,
    );
    let old_fp = RootsFingerprint::from_trusted_roots(&old_roots);
    let last_rev = ReconciliationRevision::new(3);
    let decision = decide_reconciliation(&new_roots, Some(last_rev), Some(&old_fp)).unwrap();
    assert!(
        matches!(decision, ReconciliationDecision::Schedule { .. }),
        "changed fingerprint must schedule reconciliation"
    );
    if let ReconciliationDecision::Schedule { revision, .. } = decision {
        assert_eq!(revision.value(), 4, "revision must increment from last");
    }
}

#[test]
fn index_reconciliation_model_noop_on_unchanged_fingerprint() {
    let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/a")], None);
    let fp = RootsFingerprint::from_trusted_roots(&roots);
    let decision =
        decide_reconciliation(&roots, Some(ReconciliationRevision::new(5)), Some(&fp)).unwrap();
    assert!(
        matches!(decision, ReconciliationDecision::NoOp { .. }),
        "unchanged fingerprint must produce no-op"
    );
}

#[test]
fn index_reconciliation_model_rejects_invalid_persisted_revisions() {
    assert!(ReconciliationRevision::try_from_persisted(0).is_err());
    assert!(ReconciliationRevision::try_from_persisted(u64::MAX).is_err());
}

#[test]
fn index_reconciliation_model_exhausted_revision_returns_error() {
    let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev/projects")], None);
    let result = decide_reconciliation(
        &roots,
        Some(ReconciliationRevision::new(u64::MAX - 1)),
        None,
    );
    assert!(
        result.is_err(),
        "revision exhaustion must not panic or wrap"
    );
}

// ── Publication decisions ──

#[test]
fn index_reconciliation_model_publish_when_revision_current() {
    let rev = ReconciliationRevision::new(3);
    assert_eq!(decide_publication(rev, rev), PublicationDecision::Publish);
}

#[test]
fn index_reconciliation_model_discard_when_revision_stale() {
    let scan_rev = ReconciliationRevision::new(2);
    let current_rev = ReconciliationRevision::new(3);
    assert!(
        matches!(
            decide_publication(scan_rev, current_rev),
            PublicationDecision::Discard { .. }
        ),
        "stale scan must be discarded"
    );
}

// ── State queries ──

#[test]
fn index_reconciliation_model_state_queries() {
    use std::time::SystemTime;
    let idle = ReconciliationState::Idle;
    assert!(idle.is_idle());
    assert!(!idle.is_updating());
    assert!(idle.revision().is_none());

    let roots = TrustedRootSet::from_raw(&[Utf8PathBuf::from("/dev")], None);
    let updating = ReconciliationState::Updating {
        revision: ReconciliationRevision::new(1),
        fingerprint: RootsFingerprint::from_trusted_roots(&roots),
        started_at: SystemTime::now(),
    };
    assert!(updating.is_updating());
    assert_eq!(updating.revision(), Some(ReconciliationRevision::new(1)));

    let completed = ReconciliationState::Completed {
        revision: ReconciliationRevision::new(2),
        fingerprint: RootsFingerprint::from_trusted_roots(&roots),
        completed_at: SystemTime::now(),
        discovered_count: 5,
    };
    assert!(completed.is_completed());

    let failed = ReconciliationState::Failed {
        revision: ReconciliationRevision::new(3),
        fingerprint: RootsFingerprint::from_trusted_roots(&roots),
        failed_at: SystemTime::now(),
        error_detail: "scan timeout".to_owned(),
    };
    assert!(failed.is_failed());
    assert_eq!(failed.revision(), Some(ReconciliationRevision::new(3)));
}

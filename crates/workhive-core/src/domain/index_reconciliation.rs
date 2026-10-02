//! Index reconciliation lifecycle model.
//!
//! The project index is a recoverable projection of trusted-root configuration.
//! This module defines the typed lifecycle, fingerprint/revision tracking, and
//! stale-publication prevention rules that govern background reconciliation.

use std::time::SystemTime;

use super::trusted_roots::TrustedRootSet;

/// Fingerprint of a trusted-root configuration snapshot.
///
/// Computed from the sorted, deduplicated root paths using length-prefixed
/// encoding and SHA-256 so that the encoding is unambiguous (no newline
/// join collisions) and the digest is collision-resistant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RootsFingerprint(String);

impl RootsFingerprint {
    /// Compute a fingerprint from a normalized trusted root set.
    ///
    /// The TrustedRootSet provides already-normalized, deduplicated paths.
    /// This method sorts them and encodes with length-prefixed framing before
    /// hashing. This eliminates structural ambiguity (e.g. one root containing
    /// a newline encoding identically to two roots) and provides collision-resistant
    /// comparison.
    pub fn from_trusted_roots(trusted: &TrustedRootSet) -> Self {
        use sha2::{Digest, Sha256};
        let mut sorted: Vec<&str> = trusted.roots().iter().map(|p| p.as_str()).collect();
        sorted.sort();

        let mut hasher = Sha256::new();
        for root in &sorted {
            let len = root.len() as u32;
            hasher.update(len.to_le_bytes());
            hasher.update(root.as_bytes());
        }
        let result = hasher.finalize();
        Self(format!("{:x}", result))
    }

    /// Reconstruct from a persisted string representation.
    pub fn from_str_raw(raw: &str) -> Self {
        Self(raw.to_owned())
    }

    /// Borrow the canonical string form of this fingerprint.
    ///
    /// Two fingerprints with equal `as_str()` values represent the same
    /// root set. The string is an opaque hash; callers must not parse it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Monotonic revision counter for reconciliation requests.
///
/// Each successful trusted-root mutation increments the revision.
/// A worker must not publish results from an older revision after a newer
/// revision has been requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReconciliationRevision(u64);

/// Invalid or exhausted reconciliation revision sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReconciliationRevisionError {
    /// Persisted revisions must leave room for a subsequent request.
    #[error("invalid persisted reconciliation revision: {0}")]
    InvalidPersisted(u64),
    /// No later revision can be represented.
    #[error("reconciliation revision sequence exhausted at {0}")]
    Exhausted(u64),
}

impl ReconciliationRevision {
    /// Construct a revision from an in-memory value.
    ///
    /// This constructor does not validate the value. Use `try_from_persisted`
    /// when loading from durable storage.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Validate a revision loaded from durable storage.
    ///
    /// Returns `Err(InvalidPersisted)` for values outside the valid persisted
    /// range. Values `0` and `u64::MAX` are rejected: `0` is reserved as the
    /// sentinel for "no revision persisted", and `u64::MAX` leaves no room
    /// for a subsequent increment via `checked_next`.
    pub fn try_from_persisted(value: u64) -> Result<Self, ReconciliationRevisionError> {
        if value == 0 || value == u64::MAX {
            return Err(ReconciliationRevisionError::InvalidPersisted(value));
        }
        Ok(Self(value))
    }

    /// The underlying numeric value.
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Compute the next revision, failing at the exhaustion boundary.
    ///
    /// Returns `Err(Exhausted)` when the successor cannot be represented
    /// within the valid persisted range (i.e., when `self.value() >= u64::MAX - 1`).
    /// Never panics or wraps.
    pub fn checked_next(&self) -> Result<Self, ReconciliationRevisionError> {
        self.0
            .checked_add(1)
            .filter(|value| *value < u64::MAX)
            .map(Self)
            .ok_or(ReconciliationRevisionError::Exhausted(self.0))
    }

    /// Whether this revision is strictly newer than `other`.
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self.0 > other.0
    }
}

/// Lifecycle state of the index reconciliation process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationState {
    /// No reconciliation has ever been requested or completed.
    Idle,
    /// A worker is currently scanning and building the index.
    Updating {
        revision: ReconciliationRevision,
        fingerprint: RootsFingerprint,
        started_at: SystemTime,
    },
    /// The last reconciliation completed successfully.
    Completed {
        revision: ReconciliationRevision,
        fingerprint: RootsFingerprint,
        completed_at: SystemTime,
        discovered_count: usize,
    },
    /// The last reconciliation failed. The prior valid index is retained.
    Failed {
        revision: ReconciliationRevision,
        fingerprint: RootsFingerprint,
        failed_at: SystemTime,
        error_detail: String,
    },
}

impl ReconciliationState {
    /// The current revision, if any has been requested or completed.
    ///
    /// Returns `None` only in the `Idle` state (no reconciliation has ever
    /// been requested). All other states carry a revision.
    pub fn revision(&self) -> Option<ReconciliationRevision> {
        match self {
            Self::Idle => None,
            Self::Updating { revision, .. }
            | Self::Completed { revision, .. }
            | Self::Failed { revision, .. } => Some(*revision),
        }
    }

    /// Whether no reconciliation has ever been requested.
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    /// Whether a worker is currently scanning and building the index.
    pub fn is_updating(&self) -> bool {
        matches!(self, Self::Updating { .. })
    }

    /// Whether the most recent reconciliation completed successfully.
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    /// Whether the most recent reconciliation failed. The prior valid index
    /// is retained when this returns `true`.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

/// Decision about whether to schedule a reconciliation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconciliationDecision {
    /// Schedule a reconciliation worker with the given revision and fingerprint.
    Schedule {
        revision: ReconciliationRevision,
        fingerprint: RootsFingerprint,
    },
    /// No reconciliation needed: the roots did not change (same fingerprint).
    NoOp { reason: &'static str },
}

/// Whether a worker's scan result may be published given the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublicationDecision {
    /// The scan is current: publish the results atomically.
    Publish,
    /// The scan is stale: a newer revision was requested after the scan began.
    /// Discard the results without modifying the index.
    Discard { reason: &'static str },
}

/// Determine whether a reconciliation should be scheduled after a mutation.
///
/// Compares the fingerprint of the new roots against the last known
/// fingerprint. If they differ, a new revision is issued.
///
/// The new_roots parameter must be a normalized TrustedRootSet to ensure
/// scheduler and worker compute equivalent fingerprints for the same logical
/// root configuration.
pub fn decide_reconciliation(
    new_roots: &TrustedRootSet,
    last_revision: Option<ReconciliationRevision>,
    last_fingerprint: Option<&RootsFingerprint>,
) -> Result<ReconciliationDecision, ReconciliationRevisionError> {
    let new_fp = RootsFingerprint::from_trusted_roots(new_roots);
    if let Some(last_fp) = last_fingerprint
        && &new_fp == last_fp
    {
        return Ok(ReconciliationDecision::NoOp {
            reason: "roots fingerprint unchanged",
        });
    }
    let revision = match last_revision {
        Some(revision) => revision.checked_next()?,
        None => ReconciliationRevision::new(1),
    };
    Ok(ReconciliationDecision::Schedule {
        revision,
        fingerprint: new_fp,
    })
}

/// Determine whether a worker's scan result may be published.
///
/// A scan started at `scan_revision` may only publish if no newer revision
/// has been requested since. This prevents stale results from overwriting
/// fresher configuration. Publish only when the scan revision exactly
/// matches the current revision; any mismatch is discarded.
pub fn decide_publication(
    scan_revision: ReconciliationRevision,
    current_revision: ReconciliationRevision,
) -> PublicationDecision {
    if scan_revision == current_revision {
        PublicationDecision::Publish
    } else {
        PublicationDecision::Discard {
            reason: "scan revision does not match current revision",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};

    #[test]
    fn index_reconciliation_revision_try_from_persisted_rejects_zero() {
        let result = ReconciliationRevision::try_from_persisted(0);
        assert!(
            matches!(
                result,
                Err(ReconciliationRevisionError::InvalidPersisted(0))
            ),
            "persisted revision 0 must be rejected"
        );
    }

    #[test]
    fn index_reconciliation_revision_try_from_persisted_rejects_max() {
        let result = ReconciliationRevision::try_from_persisted(u64::MAX);
        assert!(
            matches!(
                result,
                Err(ReconciliationRevisionError::InvalidPersisted(v)) if v == u64::MAX
            ),
            "persisted revision u64::MAX must be rejected"
        );
    }

    #[test]
    fn index_reconciliation_revision_try_from_persisted_accepts_valid_range() {
        for value in [1, 2, 100, u64::MAX - 1] {
            let revision = ReconciliationRevision::try_from_persisted(value);
            assert!(
                revision.is_ok(),
                "persisted revision {value} must be accepted"
            );
            assert_eq!(revision.unwrap().value(), value);
        }
    }

    #[test]
    fn index_reconciliation_revision_checked_next_succeeds_for_normal_values() {
        let revision = ReconciliationRevision::new(5);
        let next = revision.checked_next().unwrap();
        assert_eq!(next.value(), 6);
    }

    #[test]
    fn index_reconciliation_revision_checked_next_exhausted_at_max_minus_one() {
        let revision = ReconciliationRevision::new(u64::MAX - 1);
        let result = revision.checked_next();
        assert!(
            matches!(
                result,
                Err(ReconciliationRevisionError::Exhausted(v)) if v == u64::MAX - 1
            ),
            "checked_next at u64::MAX - 1 must return Exhausted, not wrap"
        );
    }

    #[test]
    fn index_reconciliation_revision_checked_next_exhausted_at_max() {
        let revision = ReconciliationRevision::new(u64::MAX);
        let result = revision.checked_next();
        assert!(
            matches!(
                result,
                Err(ReconciliationRevisionError::Exhausted(v)) if v == u64::MAX
            ),
            "checked_next at u64::MAX must return Exhausted, not wrap"
        );
    }

    #[test]
    fn index_reconciliation_revision_is_newer_than_ordering() {
        let r1 = ReconciliationRevision::new(1);
        let r2 = ReconciliationRevision::new(2);
        assert!(r2.is_newer_than(&r1));
        assert!(!r1.is_newer_than(&r2));
        assert!(!r1.is_newer_than(&r1));
    }

    #[test]
    fn index_reconciliation_fingerprint_order_independent() {
        let roots_a = vec![Utf8PathBuf::from("/a"), Utf8PathBuf::from("/b")];
        let roots_b = vec![Utf8PathBuf::from("/b"), Utf8PathBuf::from("/a")];
        assert_eq!(
            RootsFingerprint::from_trusted_roots(&crate::domain::TrustedRootSet::from_raw(
                &roots_a, None
            )),
            RootsFingerprint::from_trusted_roots(&crate::domain::TrustedRootSet::from_raw(
                &roots_b, None
            )),
            "fingerprint must be independent of root ordering"
        );
    }

    #[test]
    fn index_reconciliation_fingerprint_differs_for_different_roots() {
        let roots_a = vec![Utf8PathBuf::from("/a")];
        let roots_b = vec![Utf8PathBuf::from("/b")];
        assert_ne!(
            RootsFingerprint::from_trusted_roots(&crate::domain::TrustedRootSet::from_raw(
                &roots_a, None
            )),
            RootsFingerprint::from_trusted_roots(&crate::domain::TrustedRootSet::from_raw(
                &roots_b, None
            ))
        );
    }

    #[test]
    fn roots_fingerprint_from_trusted_roots_home_expanded_equivalence() {
        use super::super::trusted_roots::TrustedRootSet;

        // Home-expanded paths must produce the same fingerprint as their expanded forms.
        let raw_with_home = vec![Utf8PathBuf::from("~/dev")];
        let raw_expanded = vec![Utf8PathBuf::from("/home/user/dev")];

        let trusted_home =
            TrustedRootSet::from_raw(&raw_with_home, Some(Utf8Path::new("/home/user")));
        let trusted_expanded = TrustedRootSet::from_raw(&raw_expanded, None);

        assert_eq!(
            RootsFingerprint::from_trusted_roots(&trusted_home),
            RootsFingerprint::from_trusted_roots(&trusted_expanded),
            "home-expanded paths must produce identical fingerprints"
        );
    }

    #[test]
    fn roots_fingerprint_from_trusted_roots_lexical_equivalence() {
        use super::super::trusted_roots::TrustedRootSet;

        // Lexically equivalent paths (after normalization) must produce the same fingerprint.
        let raw_with_dots = vec![Utf8PathBuf::from("/dev/./projects/../projects")];
        let raw_normalized = vec![Utf8PathBuf::from("/dev/projects")];

        let trusted_dots = TrustedRootSet::from_raw(&raw_with_dots, None);
        let trusted_normalized = TrustedRootSet::from_raw(&raw_normalized, None);

        assert_eq!(
            RootsFingerprint::from_trusted_roots(&trusted_dots),
            RootsFingerprint::from_trusted_roots(&trusted_normalized),
            "lexically equivalent paths must produce identical fingerprints"
        );
    }

    #[test]
    fn roots_fingerprint_from_trusted_roots_duplicate_elimination() {
        use super::super::trusted_roots::TrustedRootSet;

        // Duplicate roots must be eliminated before fingerprint computation.
        let raw_with_dups = vec![
            Utf8PathBuf::from("/dev"),
            Utf8PathBuf::from("/dev"),
            Utf8PathBuf::from("/dev"),
        ];
        let raw_without_dups = vec![Utf8PathBuf::from("/dev")];

        let trusted_dups = TrustedRootSet::from_raw(&raw_with_dups, None);
        let trusted_no_dups = TrustedRootSet::from_raw(&raw_without_dups, None);

        assert_eq!(
            RootsFingerprint::from_trusted_roots(&trusted_dups),
            RootsFingerprint::from_trusted_roots(&trusted_no_dups),
            "duplicate roots must be eliminated before fingerprint computation"
        );
    }

    #[test]
    fn roots_fingerprint_from_trusted_roots_order_independent() {
        use super::super::trusted_roots::TrustedRootSet;

        // Order of roots must not affect the fingerprint.
        let raw_order_a = vec![Utf8PathBuf::from("/a"), Utf8PathBuf::from("/b")];
        let raw_order_b = vec![Utf8PathBuf::from("/b"), Utf8PathBuf::from("/a")];

        let trusted_a = TrustedRootSet::from_raw(&raw_order_a, None);
        let trusted_b = TrustedRootSet::from_raw(&raw_order_b, None);

        assert_eq!(
            RootsFingerprint::from_trusted_roots(&trusted_a),
            RootsFingerprint::from_trusted_roots(&trusted_b),
            "fingerprint must be independent of root ordering"
        );
    }

    #[test]
    fn roots_fingerprint_from_trusted_roots_delimiter_framing() {
        use super::super::trusted_roots::TrustedRootSet;

        // Embedded newlines must not collide with multiple roots.
        let raw_single = vec![Utf8PathBuf::from("/dev/a\nb")];
        let raw_multiple = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("b")];

        let trusted_single = TrustedRootSet::from_raw(&raw_single, None);
        let trusted_multiple = TrustedRootSet::from_raw(&raw_multiple, None);

        assert_ne!(
            RootsFingerprint::from_trusted_roots(&trusted_single),
            RootsFingerprint::from_trusted_roots(&trusted_multiple),
            "embedded newline must not collide with multiple roots"
        );
    }

    #[test]
    fn roots_fingerprint_scheduler_worker_equality() {
        use super::super::trusted_roots::TrustedRootSet;

        // Scheduler and worker must compute identical fingerprints for the same logical roots.
        // This proves the typed digest contract ensures equivalence.
        let scheduler_raw = vec![Utf8PathBuf::from("~/dev")];
        let worker_raw = vec![Utf8PathBuf::from("/home/user/dev")];

        let scheduler_trusted =
            TrustedRootSet::from_raw(&scheduler_raw, Some(Utf8Path::new("/home/user")));
        let worker_trusted = TrustedRootSet::from_raw(&worker_raw, None);

        let scheduler_fp = RootsFingerprint::from_trusted_roots(&scheduler_trusted);
        let worker_fp = RootsFingerprint::from_trusted_roots(&worker_trusted);

        assert_eq!(
            scheduler_fp, worker_fp,
            "scheduler and worker must compute identical fingerprints for equivalent roots"
        );
    }

    #[test]
    fn index_reconciliation_decide_schedule_on_first_mutation() {
        use super::super::trusted_roots::TrustedRootSet;
        let roots = vec![Utf8PathBuf::from("/dev")];
        let trusted = TrustedRootSet::from_raw(&roots, None);
        let decision = decide_reconciliation(&trusted, None, None).unwrap();
        assert!(matches!(
            decision,
            ReconciliationDecision::Schedule { revision, .. } if revision.value() == 1
        ));
    }

    #[test]
    fn index_reconciliation_decide_noop_when_fingerprint_unchanged() {
        use super::super::trusted_roots::TrustedRootSet;
        let roots = vec![Utf8PathBuf::from("/dev")];
        let trusted = TrustedRootSet::from_raw(&roots, None);
        let fp = RootsFingerprint::from_trusted_roots(&trusted);
        let revision = ReconciliationRevision::new(3);
        let decision = decide_reconciliation(&trusted, Some(revision), Some(&fp)).unwrap();
        assert!(matches!(decision, ReconciliationDecision::NoOp { .. }));
    }

    #[test]
    fn index_reconciliation_decide_schedule_increments_revision() {
        use super::super::trusted_roots::TrustedRootSet;
        let old_roots = vec![Utf8PathBuf::from("/old")];
        let new_roots = vec![Utf8PathBuf::from("/new")];
        let old_trusted = TrustedRootSet::from_raw(&old_roots, None);
        let new_trusted = TrustedRootSet::from_raw(&new_roots, None);
        let fp = RootsFingerprint::from_trusted_roots(&old_trusted);
        let revision = ReconciliationRevision::new(5);
        let decision = decide_reconciliation(&new_trusted, Some(revision), Some(&fp)).unwrap();
        assert!(matches!(
            decision,
            ReconciliationDecision::Schedule { revision, .. } if revision.value() == 6
        ));
    }

    #[test]
    fn index_reconciliation_decide_exhausted_revision_returns_error() {
        use super::super::trusted_roots::TrustedRootSet;
        let old_roots = vec![Utf8PathBuf::from("/old")];
        let new_roots = vec![Utf8PathBuf::from("/new")];
        let old_trusted = TrustedRootSet::from_raw(&old_roots, None);
        let new_trusted = TrustedRootSet::from_raw(&new_roots, None);
        let fp = RootsFingerprint::from_trusted_roots(&old_trusted);
        let revision = ReconciliationRevision::new(u64::MAX - 1);
        let result = decide_reconciliation(&new_trusted, Some(revision), Some(&fp));
        assert!(
            matches!(result, Err(ReconciliationRevisionError::Exhausted(_))),
            "scheduling at exhaustion boundary must return typed error"
        );
    }

    #[test]
    fn index_reconciliation_publication_decision_current_is_publish() {
        let scan = ReconciliationRevision::new(3);
        let current = ReconciliationRevision::new(3);
        assert!(matches!(
            decide_publication(scan, current),
            PublicationDecision::Publish
        ));
    }

    #[test]
    fn index_reconciliation_publication_decision_stale_is_discard() {
        let scan = ReconciliationRevision::new(3);
        let current = ReconciliationRevision::new(5);
        assert!(matches!(
            decide_publication(scan, current),
            PublicationDecision::Discard { .. }
        ));
    }

    #[test]
    fn index_reconciliation_state_revision_accessor() {
        assert_eq!(ReconciliationState::Idle.revision(), None);
        let revision = ReconciliationRevision::new(7);
        let fp = RootsFingerprint::from_trusted_roots(&crate::domain::TrustedRootSet::from_raw(
            &[],
            None,
        ));
        let state = ReconciliationState::Completed {
            revision,
            fingerprint: fp.clone(),
            completed_at: SystemTime::now(),
            discovered_count: 0,
        };
        assert_eq!(state.revision(), Some(revision));
    }

    #[test]
    fn roots_fingerprint_distinguishes_embedded_delimiter_from_multiple_roots() {
        // One root containing a newline must NOT collide with two separate
        // roots whose concatenation would match under the old newline-join
        // encoding.
        let single_with_newline = vec![Utf8PathBuf::from("/dev/a\nb")];
        let two_roots = vec![Utf8PathBuf::from("/dev/a"), Utf8PathBuf::from("b")];
        let fp_single = RootsFingerprint::from_trusted_roots(
            &crate::domain::TrustedRootSet::from_raw(&single_with_newline, None),
        );
        let fp_two = RootsFingerprint::from_trusted_roots(
            &crate::domain::TrustedRootSet::from_raw(&two_roots, None),
        );
        assert_ne!(
            fp_single, fp_two,
            "length-prefixed encoding must distinguish embedded newline from multiple roots"
        );
    }

    #[test]
    fn publication_discards_scan_revision_newer_than_current() {
        // scan=5, current=3: scan is newer than current. Must discard.
        let scan = ReconciliationRevision::new(5);
        let current = ReconciliationRevision::new(3);
        assert!(
            matches!(
                decide_publication(scan, current),
                PublicationDecision::Discard { .. }
            ),
            "scan newer than current must be discarded (only exact equality publishes)"
        );
    }
}

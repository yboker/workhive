//! `reindex` use case — rebuild the global project index from discovery.

use std::collections::HashSet;

use camino::{Utf8Path, Utf8PathBuf};

use crate::errors::{DiscoveryError, IndexError};
use crate::ports::{
    DiscoveredProject, DiscoveryWarning, IndexSnapshotToken, IndexState, ProjectDiscoveryPort,
    ProjectIndexPort, ProjectSummary, SnapshotPublishOutcome,
};

/// Outcome of a reindex run.
///
/// The four counters capture distinct facts so CLI output (and JSON
/// consumers) can stop conflating "discovered this run" with "what the
/// index ultimately contains":
///
/// - [`Self::discovered_count`] — projects that the discovery scan
///   found under the supplied roots in this run.
/// - [`Self::upserted_count`] — entries that were freshly inserted or
///   updated in the index during this run. With `clear_first = true`
///   this typically equals `discovered_count`. With merge semantics it
///   only counts the discovered ones.
/// - [`Self::retained_existing_count`] — entries already in the index
///   before this run that are *outside* the scanned roots and therefore
///   left alone (relevant only for merge semantics).
/// - [`Self::final_index_count`] — total entries in the index after the
///   run completes. This is the authoritative size users care about
///   when comparing `workhive project list`.
/// - [`Self::roots_scanned`] — number of roots actually scanned this
///   run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexResult {
    /// Projects observed by discovery during this run.
    pub discovered_count: usize,
    /// Entries written or refreshed during this run.
    pub upserted_count: usize,
    /// Pre-existing entries that were untouched (merge mode).
    pub retained_existing_count: usize,
    /// Total entries in the index after the run.
    pub final_index_count: usize,
    /// Number of roots scanned this run.
    pub roots_scanned: usize,
    /// Non-fatal warnings surfaced by discovery. The CLI is expected
    /// to echo these to stderr so the user is aware that some descendant
    /// subtree was skipped, even though the reindex completed.
    pub warnings: Vec<DiscoveryWarning>,
}

/// Errors that can occur during reindex.
#[derive(Debug, thiserror::Error)]
pub enum ReindexError {
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error("concurrent modification exhausted after {attempts} attempts")]
    ConcurrentModificationExhausted { attempts: usize },
}

// Bundles discovery results for the merge retry loop.
// Discovery output is retained across conditional publication attempts.
struct MergeReindexInput {
    discovered: Vec<DiscoveredProject>,
    discovered_roots: HashSet<Utf8PathBuf>,
    /// Index state captured before discovery. Entries unchanged since this
    /// snapshot are safe to prune; concurrent additions/updates are kept.
    baseline_state: IndexState,
    baseline_token: IndexSnapshotToken,
    /// Roots whose stale index entries should be removed during the merge.
    /// This can include configured roots that no longer exist on disk: they
    /// cannot be discovered, but their old index entries are still stale.
    prune_roots: Vec<Utf8PathBuf>,
    roots_scanned: usize,
    warnings: Vec<DiscoveryWarning>,
}

impl MergeReindexInput {
    fn discovered_count(&self) -> usize {
        self.discovered.len()
    }
}

/// Rebuild the global project index by discovering projects under the given roots.
///
/// - `discovery`: scans the filesystem for workhive projects
/// - `index`: persists the discovered projects
/// - `roots`: directories to scan
/// - `clear_first`: if true, clears the index before writing
///
/// The default behavior is **merge**: existing entries outside the
/// scanned roots are preserved. Pass `clear_first = true` to perform a
/// destructive rebuild.
///
/// # Race-free merge
///
/// In merge mode, this function uses optimistic concurrency control:
/// 1. Runs discovery once
/// 2. Reads paired state+token
/// 3. Computes merge based on the paired state
/// 4. Attempts conditional publish with the token
/// 5. On `Stale`, re-reads paired state+token and recomputes merge without
///    re-running discovery
/// 6. Retries up to 10 times until success
///
/// This ensures merge never deletes a foreground mutation committed during
/// discovery. Clear-first mode is explicitly destructive and uses `replace_all`.
pub fn rebuild_project_index(
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    roots: &[Utf8PathBuf],
    clear_first: bool,
) -> Result<ReindexResult, ReindexError> {
    rebuild_project_index_with_prune_roots(discovery, index, roots, roots, clear_first)
}

/// Rebuild the global project index while allowing callers to prune stale
/// entries under roots that cannot be scanned (for example, a deleted
/// configured trusted root).
pub fn rebuild_project_index_with_prune_roots(
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    roots: &[Utf8PathBuf],
    prune_roots: &[Utf8PathBuf],
    clear_first: bool,
) -> Result<ReindexResult, ReindexError> {
    let roots_scanned = roots.len();

    let baseline = if clear_first {
        None
    } else {
        Some(index.read_paired_state()?)
    };

    let discovery_report = discovery.discover_projects_report(roots)?;
    let discovered = discovery_report.projects;
    let warnings = discovery_report.warnings;
    let discovered_count = discovered.len();
    let discovered_roots: HashSet<Utf8PathBuf> =
        discovered.iter().map(|p| p.project_root.clone()).collect();

    if clear_first {
        // Clear-first mode: explicitly destructive, use replace_all
        let final_projects: Vec<ProjectSummary> =
            discovered.iter().map(to_project_summary).collect();
        index.replace_all(&final_projects)?;

        // Use final_projects.len() to avoid reading a potentially newer index
        let final_index_count = final_projects.len();

        tracing::info!(
            roots_scanned,
            discovered_count,
            upserted_count = discovered_count,
            retained_existing_count = 0,
            final_index_count,
            warning_count = warnings.len(),
            clear_first,
            "project index rebuilt (clear-first)"
        );

        return Ok(ReindexResult {
            discovered_count,
            upserted_count: discovered_count,
            retained_existing_count: 0,
            final_index_count,
            roots_scanned,
            warnings,
        });
    }

    let baseline = baseline.expect("merge reindex captures a baseline");
    let input = MergeReindexInput {
        discovered,
        discovered_roots,
        baseline_state: baseline.state,
        baseline_token: baseline.token,
        prune_roots: prune_roots.to_vec(),
        roots_scanned,
        warnings,
    };
    merge_reindex(index, input, std::thread::sleep)
}

// Merge retry loop: each Stale result retries against a fresh paired index state.
fn merge_reindex(
    index: &dyn ProjectIndexPort,
    input: MergeReindexInput,
    wait: impl Fn(std::time::Duration),
) -> Result<ReindexResult, ReindexError> {
    const MAX_RETRIES: usize = 10;
    let discovered_count = input.discovered_count();

    for attempt in 0..MAX_RETRIES {
        // Read paired state+token
        let paired = index.read_paired_state()?;
        // Compute merge based on the paired state
        let merged: Vec<ProjectSummary> = compute_merge(
            &paired.state,
            &input.discovered,
            &input.discovered_roots,
            &input.prune_roots,
            &input.baseline_state,
        );
        let retained_existing_count = paired
            .state
            .projects()
            .iter()
            .filter(|project| {
                !input.discovered_roots.contains(&project.project_root)
                    && (!is_under_any_root(&project.project_root, &input.prune_roots)
                        || !is_unchanged_from_baseline(project, &input.baseline_state))
            })
            .count();
        let expected_token = if attempt == 0 {
            input.baseline_token.clone()
        } else {
            paired.token.clone()
        };

        // Attempt conditional publish
        match index.publish_snapshot_conditional(&merged, expected_token)? {
            SnapshotPublishOutcome::Published => {
                // Use merged.len() for final_index_count to avoid reading a potentially
                // newer index after releasing the lease
                let final_index_count = merged.len();

                tracing::info!(
                    roots_scanned = input.roots_scanned,
                    discovered_count,
                    upserted_count = discovered_count,
                    retained_existing_count,
                    final_index_count,
                    warning_count = input.warnings.len(),
                    clear_first = false,
                    attempt,
                    "project index rebuilt (merge)"
                );

                return Ok(ReindexResult {
                    discovered_count,
                    upserted_count: discovered_count,
                    retained_existing_count,
                    final_index_count,
                    roots_scanned: input.roots_scanned,
                    warnings: input.warnings,
                });
            }
            SnapshotPublishOutcome::Stale => {
                // Another mutation occurred; retry with fresh state
                tracing::debug!(attempt, "merge publish stale, retrying with fresh state");

                // Exponential backoff capped at 5ms, overflow-safe
                // No wait after final attempt
                if attempt < MAX_RETRIES - 1 {
                    let backoff_ms = (1_u64 << attempt.min(3)).min(5);
                    wait(std::time::Duration::from_millis(backoff_ms));
                }

                continue;
            }
        }
    }

    // All retries exhausted
    Err(ReindexError::ConcurrentModificationExhausted {
        attempts: MAX_RETRIES,
    })
}

/// Whether a path is equal to or nested beneath any root.
pub fn is_under_any_root(path: &Utf8Path, roots: &[Utf8PathBuf]) -> bool {
    roots
        .iter()
        .any(|root| path == root.as_path() || path.starts_with(root.as_path()))
}

/// Merge projects by root, retaining concurrent changes outside the scan baseline.
fn compute_merge(
    pre_state: &IndexState,
    discovered: &[DiscoveredProject],
    discovered_roots: &HashSet<Utf8PathBuf>,
    prune_roots: &[Utf8PathBuf],
    baseline_state: &IndexState,
) -> Vec<ProjectSummary> {
    let mut merged: Vec<ProjectSummary> = pre_state
        .projects()
        .iter()
        .filter(|existing| {
            !discovered_roots.contains(&existing.project_root)
                && (!is_under_any_root(&existing.project_root, prune_roots)
                    || !is_unchanged_from_baseline(existing, baseline_state))
        })
        .cloned()
        .collect();
    merged.extend(discovered.iter().map(to_project_summary));
    merged
}

// ponytail: O(n²) baseline scan; project indexes are small, use a root-keyed map if scale makes it measurable.
fn is_unchanged_from_baseline(project: &ProjectSummary, baseline: &IndexState) -> bool {
    baseline.projects().iter().any(|candidate| {
        candidate.project_root == project.project_root
            && candidate.project_name == project.project_name
            && candidate.config_path == project.config_path
            && candidate.last_opened_at == project.last_opened_at
            && candidate.last_updated_at == project.last_updated_at
    })
}

fn to_project_summary(project: &DiscoveredProject) -> ProjectSummary {
    ProjectSummary {
        project_name: project.project_name.clone(),
        project_root: project.project_root.clone(),
        config_path: project.project_root.join(".workhive/config.toml"),
        last_opened_at: None,
        last_updated_at: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // Returns Stale for every conditional publication attempt.
    struct AlwaysStaleIndex {
        publish_count: AtomicUsize,
    }

    impl ProjectIndexPort for AlwaysStaleIndex {
        fn list_projects(&self) -> Result<crate::ports::IndexState, IndexError> {
            Ok(crate::ports::IndexState::Loaded(vec![]))
        }

        fn read_paired_state(&self) -> Result<crate::ports::PairedIndexState, IndexError> {
            Ok(crate::ports::PairedIndexState {
                state: crate::ports::IndexState::Loaded(vec![ProjectSummary {
                    project_name: "pre-existing".to_owned(),
                    project_root: Utf8PathBuf::from("/tmp/pre-existing"),
                    config_path: Utf8PathBuf::from("/tmp/pre-existing/.workhive/config.toml"),
                    last_opened_at: None,
                    last_updated_at: None,
                }]),
                token: crate::ports::IndexSnapshotToken::from_digest([0u8; 32]),
            })
        }

        fn capture_snapshot_token(&self) -> Result<crate::ports::IndexSnapshotToken, IndexError> {
            Ok(crate::ports::IndexSnapshotToken::from_digest([0u8; 32]))
        }

        fn upsert_project(&self, _summary: &ProjectSummary) -> Result<(), IndexError> {
            Ok(())
        }

        fn remove_project(&self, _project_root: &Utf8PathBuf) -> Result<(), IndexError> {
            Ok(())
        }

        fn clear(&self) -> Result<(), IndexError> {
            Ok(())
        }

        fn replace_all(&self, _projects: &[ProjectSummary]) -> Result<(), IndexError> {
            Ok(())
        }

        fn publish_snapshot_conditional(
            &self,
            _projects: &[ProjectSummary],
            _expected_token: crate::ports::IndexSnapshotToken,
        ) -> Result<SnapshotPublishOutcome, IndexError> {
            self.publish_count.fetch_add(1, Ordering::SeqCst);
            Ok(SnapshotPublishOutcome::Stale)
        }
    }

    #[test]
    fn reindex_merge_backoff_sequence_is_exact() {
        let index = AlwaysStaleIndex {
            publish_count: AtomicUsize::new(0),
        };

        let wait_durations: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
        let wait_durations_clone = Arc::clone(&wait_durations);

        let discovered = vec![DiscoveredProject {
            project_name: "discovered".to_owned(),
            project_root: Utf8PathBuf::from("/tmp/discovered"),
            instance_id: "abcd".to_owned(),
            initialized: true,
        }];
        let discovered_roots: HashSet<Utf8PathBuf> =
            discovered.iter().map(|p| p.project_root.clone()).collect();

        let baseline = index.read_paired_state().unwrap();
        let input = MergeReindexInput {
            discovered,
            discovered_roots,
            baseline_state: baseline.state,
            baseline_token: baseline.token,
            prune_roots: vec![],
            roots_scanned: 1,
            warnings: vec![],
        };

        let result = merge_reindex(&index, input, move |duration| {
            wait_durations_clone.lock().unwrap().push(duration);
        });

        // Verify it failed with ConcurrentModificationExhausted
        assert!(result.is_err(), "should fail after exhausting retries");
        match result {
            Err(ReindexError::ConcurrentModificationExhausted { attempts }) => {
                assert_eq!(attempts, 10, "should exhaust all 10 retries");
            }
            _ => panic!("expected ConcurrentModificationExhausted error"),
        }

        // Verify publication was attempted 10 times
        assert_eq!(
            index.publish_count.load(Ordering::SeqCst),
            10,
            "should attempt publication 10 times"
        );

        // Verify exact wait sequence: 9 waits for 10 attempts
        let waits = wait_durations.lock().unwrap();
        assert_eq!(
            waits.len(),
            9,
            "should have exactly 9 waits (no wait after final attempt)"
        );

        // Verify exact durations: [1ms, 2ms, 4ms, 5ms, 5ms, 5ms, 5ms, 5ms, 5ms]
        let expected: Vec<Duration> = vec![
            Duration::from_millis(1),
            Duration::from_millis(2),
            Duration::from_millis(4),
            Duration::from_millis(5),
            Duration::from_millis(5),
            Duration::from_millis(5),
            Duration::from_millis(5),
            Duration::from_millis(5),
            Duration::from_millis(5),
        ];
        assert_eq!(
            *waits, expected,
            "wait sequence must match exponential backoff capped at 5ms"
        );
    }
}

// Tests for this module live in `workhive-testkit/tests/usecase_tests.rs`
// because they require fake adapters that cannot be imported from `workhive-core`.

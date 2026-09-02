//! `reindex` use case — rebuild the global project index from discovery.

use std::collections::HashSet;

use camino::Utf8PathBuf;

use crate::errors::{DiscoveryError, IndexError};
use crate::ports::{
    DiscoveredProject, DiscoveryWarning, ProjectDiscoveryPort, ProjectIndexPort, ProjectSummary,
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
pub fn rebuild_project_index(
    discovery: &dyn ProjectDiscoveryPort,
    index: &dyn ProjectIndexPort,
    roots: &[Utf8PathBuf],
    clear_first: bool,
) -> Result<ReindexResult, ReindexError> {
    let roots_scanned = roots.len();

    // Snapshot the pre-existing index so we can compute retained vs.
    // overwritten entries by **set difference on `project_root`**, not
    // by subtraction. We deliberately
    // avoid `final_index_count - upserted_count` because it conflates
    // overwrites with retentions.
    let pre_state = index.list_projects()?;
    let pre_roots: HashSet<Utf8PathBuf> = pre_state
        .projects()
        .iter()
        .map(|s| s.project_root.clone())
        .collect();

    let discovery_report = discovery.discover_projects_report(roots)?;
    let discovered = discovery_report.projects;
    let warnings = discovery_report.warnings;
    let discovered_count = discovered.len();
    let discovered_roots: HashSet<Utf8PathBuf> =
        discovered.iter().map(|p| p.project_root.clone()).collect();

    if clear_first {
        index.clear()?;
    }

    let mut upserted_count: usize = 0;
    for project in &discovered {
        let summary = to_project_summary(project);
        index.upsert_project(&summary)?;
        upserted_count += 1;
    }

    let post_state = index.list_projects()?;
    let final_index_count = post_state.projects().len();

    let retained_existing_count = if clear_first {
        // After a clear, the index is rebuilt from the discovered set;
        // by construction nothing pre-existing was retained.
        0
    } else {
        // Merge mode. An entry is "retained existing" when:
        //   - it was already in the index before this run, and
        //   - it was *not* re-discovered this run (i.e. the discovery
        //     scan did not touch its `project_root`).
        // This is a true set difference on the stable identity
        // (`project_root`) rather than a counter subtraction.
        pre_roots.difference(&discovered_roots).count()
    };

    tracing::info!(
        roots_scanned,
        discovered_count,
        upserted_count,
        retained_existing_count,
        final_index_count,
        warning_count = warnings.len(),
        clear_first,
        "project index rebuilt"
    );

    Ok(ReindexResult {
        discovered_count,
        upserted_count,
        retained_existing_count,
        final_index_count,
        roots_scanned,
        warnings,
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

// Tests for this module live in `workhive-testkit/tests/usecase_tests.rs`
// because they require fake adapters that cannot be imported from `workhive-core`.

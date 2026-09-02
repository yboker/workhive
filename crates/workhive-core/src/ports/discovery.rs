//! Project discovery port — abstracts filesystem scanning for workhive projects.

use camino::Utf8PathBuf;

use crate::errors::DiscoveryError;

/// A project found during filesystem discovery.
///
/// Represents a directory that contains `.workhive/config.toml` and
/// `.workhive/instance_id`, identifying it as a workhive-managed project.
#[derive(Debug, Clone)]
pub struct DiscoveredProject {
    /// Display name of the project (directory name).
    pub project_name: String,
    /// Absolute path to the project root.
    pub project_root: Utf8PathBuf,
    /// The 4-hex instance ID from `.workhive/instance_id`.
    pub instance_id: String,
    /// Whether the project has valid config and instance_id.
    pub initialized: bool,
}

/// A non-fatal observation surfaced from a discovery scan.
///
/// Discovery must remain resilient to unreadable descendant directories
/// nested inside a *valid* trusted root (e.g. macOS's
/// `~/Library/Application Support/MobileSync/Backup`, which is owned by
/// `tccd` and not readable by ordinary user processes). When that
/// happens we skip the unreadable subtree, keep scanning siblings, and
/// surface a typed warning so the CLI can echo it to stderr. Top-level
/// roots are *not* warnings — they remain hard `DiscoveryError`
/// failures so a misconfigured trusted-root list still gets caught.
///
/// Contract: unreadable subdirectories are skipped (and reported) rather
/// than failing the whole reindex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryWarning {
    /// A descendant directory beneath a valid trusted root could not be
    /// read. We skipped its subtree and continued the scan.
    ///
    /// `path` is the unreadable descendant, *not* the trusted root.
    /// `reason` captures the OS-level reason (e.g. "permission denied",
    /// "operation not permitted"). It is the localized-friendly short
    /// form of `std::io::Error::to_string`.
    UnreadableDescendant { path: Utf8PathBuf, reason: String },
}

/// Result of a discovery scan: discovered projects plus any non-fatal
/// warnings encountered while walking trusted roots.
///
/// This type lets callers display warnings (e.g. to stderr) while still
/// processing the discovered project set. Top-level fatal failures are
/// surfaced through `Result::Err(DiscoveryError)`, never through this
/// report.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryReport {
    /// Projects observed by the scan.
    pub projects: Vec<DiscoveredProject>,
    /// Non-fatal warnings (e.g. unreadable descendant directories).
    pub warnings: Vec<DiscoveryWarning>,
}

/// Abstraction over project discovery via filesystem scanning.
///
/// Implementations recursively scan given root directories to find
/// workhive-managed projects. Discovery is a read-only operation
/// that never modifies the filesystem or project state.
pub trait ProjectDiscoveryPort {
    /// Discover workhive projects under the given root directories.
    ///
    /// This is the legacy convenience form that discards warnings.
    /// New call sites should prefer [`Self::discover_projects_report`]
    /// so they can surface non-fatal scan warnings (e.g. unreadable
    /// descendant directories) to the user without aborting the scan.
    ///
    /// Each root is recursively scanned. A directory qualifies as a
    /// workhive project if it contains `.workhive/config.toml` and
    /// `.workhive/instance_id`.
    ///
    /// Skips `.git`, `.workhive/worktrees`, `node_modules`, and other
    /// known large directories.
    fn discover_projects(
        &self,
        roots: &[Utf8PathBuf],
    ) -> Result<Vec<DiscoveredProject>, DiscoveryError> {
        self.discover_projects_report(roots).map(|r| r.projects)
    }

    /// Discover workhive projects, returning both the discovered set
    /// and any non-fatal warnings observed during the walk.
    ///
    /// Implementations must hard-fail on top-level root errors
    /// (non-existent root, root that itself is unreadable) and must
    /// only emit warnings for unreadable *descendant* directories under
    /// an otherwise valid root. See [`DiscoveryWarning`].
    fn discover_projects_report(
        &self,
        roots: &[Utf8PathBuf],
    ) -> Result<DiscoveryReport, DiscoveryError>;
}

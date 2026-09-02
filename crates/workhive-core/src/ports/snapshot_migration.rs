//! Snapshot migration port — abstracts schema version upgrades.
//!
//! Migrations transform persisted snapshot data from older schema versions
//! to the current schema format. The migration pipeline is:
//! `V1 -> V2 -> V3 -> Current`

use crate::domain::{MigrationOutcome, SchemaVersion, WorkspaceSnapshot};
use crate::errors::SnapshotError;

/// Abstraction over snapshot schema migration.
///
/// Implementations handle the conversion of raw JSON data from older
/// schema versions into the current `WorkspaceSnapshot` model.
pub trait SnapshotMigrationPort {
    /// Detect the schema version of raw snapshot JSON.
    fn detect_version(&self, raw_json: &str) -> Result<SchemaVersion, SnapshotError>;

    /// Migrate raw snapshot JSON to the current schema format.
    ///
    /// Returns the migrated snapshot and the migration outcome.
    /// If the snapshot is already current, returns `MigrationOutcome::AlreadyCurrent`.
    /// If migration is impossible, returns `MigrationOutcome::Incompatible`.
    fn migrate_to_current(
        &self,
        raw_json: &str,
    ) -> Result<(WorkspaceSnapshot, MigrationOutcome), SnapshotError>;

    /// Check whether a raw snapshot can be migrated without performing the migration.
    fn can_migrate(&self, raw_json: &str) -> Result<bool, SnapshotError>;
}

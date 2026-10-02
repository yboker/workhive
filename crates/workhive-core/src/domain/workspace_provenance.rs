//! Workspace provenance — typed identity/source/materialization model.
//!
//! These types decouple workspace identity from git semantics:
//!
//! - [`WorkspaceOpenIntent`] — what the caller wants (typed, not boolean)
//! - [`WorkspaceProvider`]   — who materialized the workspace
//! - [`MaterializationKind`] — how the workspace root was produced
//! - [`WorkspaceProvenance`] — assembled by the use case, not by the caller

/// Typed intent for the `open` use case.
///
/// Replaces the old `is_main_branch: bool` parameter. The use case
/// decides how to interpret each intent variant; the caller only
/// declares *what* it wants, never *how* provenance is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceOpenIntent {
    /// Use the project root directory directly (no provisioning).
    UseProjectRoot,
    /// Provision a dedicated workspace root via the configured provider.
    Provision,
}

/// Which backend produced the workspace root.
///
/// Currently only `Git` exists, but the type boundary ensures
/// that adding a future provider (e.g. `PlainDirectory`, `Container`)
/// does not require redefining existing use-case logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceProvider {
    /// Workspace root was materialized via git worktree.
    Git,
}

/// How the workspace root was produced during this open call.
///
/// This is a domain-level projection of the port-level
/// `ProvisionOutcome`. The mapping lives inside the use case,
/// not in a `From` impl on the port type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterializationKind {
    /// Reused an already-existing workspace root directory.
    ReusedExisting,
    /// Created a new root from an already-existing backing reference.
    FromExistingReference,
    /// Created a new root and a brand-new backing reference.
    FromNewReference,
}

/// Complete provenance record assembled by the use case.
///
/// This is the **output** of provenance resolution, never an input.
/// The use case produces it after consulting the provision port
/// and deciding the materialization path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceProvenance {
    /// The workspace uses the project root directory directly.
    /// No provider was involved; provisioning was skipped.
    ProjectRoot,
    /// The workspace was materialized by a specific provider.
    Provisioned {
        provider: WorkspaceProvider,
        materialization: MaterializationKind,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_intent_is_not_boolean() {
        let intent_root = WorkspaceOpenIntent::UseProjectRoot;
        let intent_prov = WorkspaceOpenIntent::Provision;
        assert_ne!(intent_root, intent_prov);
    }

    #[test]
    fn provenance_project_root_carries_no_provider() {
        let prov = WorkspaceProvenance::ProjectRoot;
        assert_eq!(prov, WorkspaceProvenance::ProjectRoot);
    }

    #[test]
    fn provenance_provisioned_carries_explicit_provider_and_materialization() {
        let prov = WorkspaceProvenance::Provisioned {
            provider: WorkspaceProvider::Git,
            materialization: MaterializationKind::FromNewReference,
        };
        match &prov {
            WorkspaceProvenance::Provisioned {
                provider,
                materialization,
            } => {
                assert_eq!(*provider, WorkspaceProvider::Git);
                assert_eq!(*materialization, MaterializationKind::FromNewReference);
            }
            _ => panic!("expected Provisioned variant"),
        }
    }

    #[test]
    fn all_materialization_kinds_are_distinct() {
        let kinds = [
            MaterializationKind::ReusedExisting,
            MaterializationKind::FromExistingReference,
            MaterializationKind::FromNewReference,
        ];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i], kinds[j]);
            }
        }
    }
}

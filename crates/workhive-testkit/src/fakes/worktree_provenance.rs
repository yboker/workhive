//! In-memory fake for `WorktreeProvenancePort`.

use std::cell::RefCell;
use std::collections::HashMap;

use workhive_core::ports::{BranchProvenance, WorktreeProvenanceError, WorktreeProvenancePort};

/// In-memory provenance store for tests.
///
/// Mirrors the production contract: recording [`BranchProvenance::Unknown`]
/// clears the entry, and a missing entry resolves to `Unknown` so removal
/// stays conservative.
#[derive(Default)]
pub struct FakeWorktreeProvenance {
    entries: RefCell<HashMap<String, BranchProvenance>>,
}

impl FakeWorktreeProvenance {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WorktreeProvenancePort for FakeWorktreeProvenance {
    fn record(
        &self,
        workspace: &str,
        provenance: BranchProvenance,
    ) -> Result<(), WorktreeProvenanceError> {
        match provenance {
            BranchProvenance::Unknown => {
                self.entries.borrow_mut().remove(workspace);
            }
            decided => {
                self.entries
                    .borrow_mut()
                    .insert(workspace.to_owned(), decided);
            }
        }
        Ok(())
    }

    fn lookup(&self, workspace: &str) -> BranchProvenance {
        self.entries
            .borrow()
            .get(workspace)
            .copied()
            .unwrap_or(BranchProvenance::Unknown)
    }

    fn forget(&self, workspace: &str) -> Result<(), WorktreeProvenanceError> {
        self.entries.borrow_mut().remove(workspace);
        Ok(())
    }
}

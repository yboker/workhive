//! `open` use case — staged plan-then-execute for workspace creation.
//!
//! The flow is explicitly staged:
//!
//! 1. **validate** — verify inputs (workspace name, naming policy)
//! 2. **prepare** — produce an `OpenPlan` describing all side effects
//! 3. **execute** — run the plan (worktree, session, snapshot restore)
//!
//! Confirmation (stage 2 in the upgrade plan) is handled by the CLI
//! layer via `CommandPolicy`, not by the use case.
//! Attachment (stage 5) is handled by the CLI layer after execution.
//!
//! ## Recovery priority (product contract)
//!
//! `Normal` mode: live session > snapshot > layout.
//! `ForceRecreateFromLayout` mode: destroy session + delete snapshot → layout.

mod execute;
mod layout_parse;
mod plan;
mod restore;
mod restore_pane;
#[cfg(test)]
mod restore_pane_tests;
mod types;

pub use execute::*;
#[allow(unused_imports)]
pub(crate) use layout_parse::*;
pub use plan::*;
#[allow(unused_imports)]
pub(crate) use restore::*;
#[allow(unused_imports)]
pub(crate) use restore_pane::*;
pub use types::*;

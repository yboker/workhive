//! `open` use case — staged plan-then-execute for workspace creation.
//!
//! The flow is explicitly staged:
//!
//! 1. **validate** — verify inputs (workspace name, naming policy)
//! 2. **prepare** — produce an `OpenPlan` describing all side effects
//! 3. **execute** — run the plan (worktree, session, live runtime setup)
//!
//! Confirmation (stage 2 in the upgrade plan) is handled by the CLI
//! layer via `CommandPolicy`, not by the use case.
//! Attachment (stage 5) is handled by the CLI layer after execution.
//!
//! ## Runtime setup
//!
//! A live session is reused when available. Missing sessions are created and
//! configured from the effective project configuration.

mod execute;
mod plan;
mod types;

pub use execute::*;
pub use plan::*;
pub use types::*;

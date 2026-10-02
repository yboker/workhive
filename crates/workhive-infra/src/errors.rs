//! Backward-compatible re-export of tmux error types.
//!
//! This module provides compatibility for existing code that imports
//! `workhive_infra::errors::TmuxError`. New code should prefer importing
//! directly from `workhive_infra::tmux_errors`.

#[deprecated(
    since = "2.1.0",
    note = "Use `workhive_infra::tmux_errors` instead. This module will be removed in a future release."
)]
pub use crate::tmux_errors::*;

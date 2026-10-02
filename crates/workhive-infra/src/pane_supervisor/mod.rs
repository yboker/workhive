//! Workhive pane supervisor — authoritative metadata for processes
//! Workhive launched directly.
//!
//! ## Boundary
//!
//! * `store` owns the on-disk JSON metadata layout
//!   (`<runtime>/pane-supervisor/<unit_id>.json`). It is purely a
//!   read/write API; it does not look at OS state.
//! * `adapter` implements [`workhive_core::ports::PaneSupervisorPort`]
//!   by combining the metadata read with a liveness probe
//!   (`kill(pid, 0)` semantics, performed in-process via
//!   `rustix::process::test_kill_process`).
//!   A stale entry — pid no longer alive — is downgraded to a fallback
//!   observation so the merge policy falls through to OS observer or
//!   tmux fallback.
//! * `runner` owns the `workhive internal pane-supervisor` execution
//!   loop: it writes `Running` metadata, supervises the configured
//!   child command, and writes `Exited` metadata on exit.
//!
//! All three pieces compose into "Workhive-supervised processes are
//! the highest-confidence observation source" per the architecture
//! contract on `PaneSupervisorPort`.

pub mod adapter;
pub mod runner;
pub mod store;

pub use adapter::JsonFilePaneSupervisor;
pub use store::{
    PaneSupervisorMetadata, PaneSupervisorRecord, PaneSupervisorStateOnDisk,
    PaneSupervisorStoreError,
};

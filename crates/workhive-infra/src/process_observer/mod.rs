//! OS process observer adapters.
//!
//! ## Adapter selection
//!
//! * macOS / Linux → [`posix_ps::PsUnixProcessObserver`] (uses `ps`).
//! * everything else → [`unsupported::UnsupportedProcessObserver`].
//!
//! Use [`create_default_observer`] to obtain the right adapter for the
//! current host. Tests can construct adapters directly to inject
//! fake `ps` output.
//!
//! ## Design notes
//!
//! * Adapters MUST never panic on bad input. A parse failure surfaces
//!   as [`ProcessObserverError::Parse`] so the merge policy treats the
//!   OS layer as silent and falls through.
//! * Adapters MUST NOT shell out via `sh -c`; they invoke `ps`
//!   directly via `Command::arg()` per the external-command policy.
//! * Adapters implement `observe_many` to fold N pane lookups into
//!   at most two `ps -p <pids>` invocations (one for shell PIDs, one
//!   for foreground PG leaders) so commands like `workhive pane list`
//!   cost a constant number of external calls instead of O(panes).
//! * Adapters report the foreground process plus a best-effort
//!   process tree. Argv beyond `argv[0]` is intentionally not
//!   surfaced — only basenames are consumed by the merge policy and
//!   we avoid leaking secret-bearing argv tail.

use workhive_core::ports::ProcessObserverPort;

pub mod posix_ps;
pub mod unsupported;

#[cfg(test)]
mod ps_parse_tests;

/// Build the default adapter for the current platform.
///
/// On macOS and Linux this returns a [`posix_ps::PsUnixProcessObserver`].
/// Other platforms get [`unsupported::UnsupportedProcessObserver`] so
/// the merge policy still receives a typed observation (rather than
/// silently inventing data).
pub fn create_default_observer() -> Box<dyn ProcessObserverPort + Send + Sync> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        Box::new(posix_ps::PsUnixProcessObserver::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Box::new(unsupported::UnsupportedProcessObserver::new())
    }
}

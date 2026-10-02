//! Process launcher port for spawning background workers.
//!
//! The CLI orchestrates process spawning through this narrow port.
//! Infrastructure adapters implement the actual process creation.

use camino::Utf8Path;

use crate::errors::LaunchError;

/// Process-boundary abstraction for spawning background reconciliation workers.
///
/// Production launches a detached child process.
/// Tests use a fake that records launch calls without spawning processes.
pub trait ProcessLauncher {
    /// Launch a background reconciliation for the given config dir.
    ///
    /// Returns `Ok(())` on successful spawn, `Err` on executable resolution
    /// or process spawn failure.
    fn launch_reconciliation(&self, config_dir: &Utf8Path) -> Result<(), LaunchError>;
}

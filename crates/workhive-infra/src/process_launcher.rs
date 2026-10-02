//! Process launcher adapter for spawning background workers.
//!
//! Production implementation that spawns detached child processes.

use camino::Utf8Path;
use workhive_core::errors::LaunchError;
use workhive_core::ports::ProcessLauncher;

/// Production launcher: spawns the current executable with detached stdio.
pub struct OsProcessLauncher;

impl ProcessLauncher for OsProcessLauncher {
    fn launch_reconciliation(&self, config_dir: &Utf8Path) -> Result<(), LaunchError> {
        let exe = std::env::current_exe().map_err(LaunchError::ExecutableResolution)?;
        std::process::Command::new(exe)
            .arg("internal")
            .arg("reconcile")
            .arg("--config-dir")
            .arg(config_dir.as_str())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(LaunchError::SpawnFailed)?;
        tracing::debug!(config_dir = %config_dir, "reconciliation worker spawned");
        Ok(())
    }
}

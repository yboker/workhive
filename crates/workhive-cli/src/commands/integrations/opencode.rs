//! OpenCode legacy-plugin cleanup.
//!
//! Earlier Workhive releases dropped a sentinel-tagged plugin module
//! into OpenCode's auto-loaded plugin directory
//! (`$XDG_CONFIG_HOME/opencode/plugins/workhive.mjs`, see
//! <https://opencode.ai/docs/plugins/>). The current preparation
//! lifecycle does not install the plugin file; this module exists
//! solely to remove any legacy artifact still on disk.
//!
//! Cleanup is sentinel-gated: only files carrying
//! [`super::sentinel::WORKHIVE_SENTINEL`] are deletable. A
//! user-authored plugin module sitting at the Workhive slot path is
//! preserved verbatim — the cleanup step reports `Preserve` with
//! reason `"user_authored"` and the lifecycle's
//! [`super::PreparationOutcome`] reflects "nothing to write".

use std::fs;

use camino::Utf8Path;

use workhive_core::domain::AiActivityProvider;

use super::cleanup::CleanupStep;
use super::error::{IntegrationError, io_err};
use super::sentinel::file_is_workhive_authored;

/// Inspect-only: classify the OpenCode plugin slot at `plugin_path`
/// into a [`CleanupStep`] without writing to disk.
pub(super) fn plan_remove(plugin_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    if !plugin_path.exists() {
        return Ok(CleanupStep::missing(
            AiActivityProvider::Opencode,
            plugin_path.to_path_buf(),
        ));
    }
    if !file_is_workhive_authored(plugin_path)? {
        return Ok(CleanupStep::preserve(
            AiActivityProvider::Opencode,
            plugin_path.to_path_buf(),
            "user_authored",
        ));
    }
    Ok(CleanupStep::remove(
        AiActivityProvider::Opencode,
        plugin_path.to_path_buf(),
        "sentinel_match",
    ))
}

/// Plan + execute: remove the legacy Workhive-owned OpenCode plugin
/// file when present and sentinel-tagged. User-authored files at the
/// same path are preserved.
pub(super) fn remove(plugin_path: &Utf8Path) -> Result<CleanupStep, IntegrationError> {
    let step = plan_remove(plugin_path)?;
    if step.would_write {
        fs::remove_file(&step.target).map_err(|e| io_err("remove", step.target.clone(), e))?;
    }
    Ok(step)
}

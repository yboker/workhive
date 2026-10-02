//! Legacy-script cleanup helpers.
//!
//! Earlier Workhive releases installed sentinel-tagged shell hook
//! scripts under
//! `<global_config>/workhive/integrations/<provider>/workhive-<provider>-hook.sh`
//! and referenced them from the user's official tool config. The
//! current preparation lifecycle no longer materializes those scripts.
//! These helpers exist solely to delete the legacy artifacts that
//! prior releases may have left on disk.
//!
//! Each `remove_*_hook_script` returns a [`CleanupStep`] describing
//! the action taken at the target path. The `plan_remove_*` siblings
//! return the same shape without performing any filesystem write,
//! supporting the inspectable dry-run/plan contract described in
//! [`super::cleanup`].

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use workhive_core::domain::AiActivityProvider;

use super::cleanup::CleanupStep;
use super::error::{IntegrationError, io_err};
use super::sentinel::file_is_workhive_authored;

/// Absolute path to the legacy Claude hook shell script.
fn claude_hook_script_path(dir: &Utf8Path) -> Utf8PathBuf {
    dir.join("workhive-claude-hook.sh")
}

/// Absolute path to the legacy Codex hook shell script.
fn codex_hook_script_path(dir: &Utf8Path) -> Utf8PathBuf {
    dir.join("workhive-codex-hook.sh")
}

/// Resolve the absolute Claude hook script path, propagating any
/// global-config path resolution error.
pub(super) fn claude_hook_script_target() -> Result<Utf8PathBuf, IntegrationError> {
    let dir = super::paths::workhive_scripts_dir("claude_code")?;
    Ok(claude_hook_script_path(&dir))
}

/// Resolve the absolute Codex hook script path, propagating any
/// global-config path resolution error.
pub(super) fn codex_hook_script_target() -> Result<Utf8PathBuf, IntegrationError> {
    let dir = super::paths::workhive_scripts_dir("codex")?;
    Ok(codex_hook_script_path(&dir))
}

/// Inspect-only: report what cleanup *would* do at the legacy Claude
/// hook script path without performing any filesystem mutation.
///
/// Currently exercised exclusively by the in-process dry-run /
/// inspection path in tests; the production cleanup pass calls
/// [`remove_claude_hook_script`] which uses the same underlying
/// [`plan_remove_workhive_script_at`] helper before any destructive
/// write.
#[cfg(test)]
pub(super) fn plan_remove_claude_hook_script() -> Result<CleanupStep, IntegrationError> {
    let target = claude_hook_script_target()?;
    plan_remove_workhive_script_at(&target, AiActivityProvider::ClaudeCode)
}

/// Inspect-only: report what cleanup *would* do at the legacy Codex
/// hook script path without performing any filesystem mutation. See
/// [`plan_remove_claude_hook_script`] for the broader contract.
#[cfg(test)]
pub(super) fn plan_remove_codex_hook_script() -> Result<CleanupStep, IntegrationError> {
    let target = codex_hook_script_target()?;
    plan_remove_workhive_script_at(&target, AiActivityProvider::Codex)
}

/// Remove the legacy Workhive-owned Claude hook script if present.
/// User-authored files at the same path are preserved. Returns the
/// executed [`CleanupStep`].
pub(super) fn remove_claude_hook_script() -> Result<CleanupStep, IntegrationError> {
    let target = claude_hook_script_target()?;
    remove_workhive_script_at(&target, AiActivityProvider::ClaudeCode)
}

/// Remove the legacy Workhive-owned Codex hook script if present.
/// User-authored files at the same path are preserved.
pub(super) fn remove_codex_hook_script() -> Result<CleanupStep, IntegrationError> {
    let target = codex_hook_script_target()?;
    remove_workhive_script_at(&target, AiActivityProvider::Codex)
}

/// Shared inspection routine: classify the script at `target` into a
/// [`CleanupStep`] without writing to disk.
fn plan_remove_workhive_script_at(
    target: &Utf8Path,
    provider: AiActivityProvider,
) -> Result<CleanupStep, IntegrationError> {
    if !target.exists() {
        return Ok(CleanupStep::missing(provider, target.to_path_buf()));
    }
    if !file_is_workhive_authored(target)? {
        return Ok(CleanupStep::preserve(
            provider,
            target.to_path_buf(),
            "user_authored",
        ));
    }
    Ok(CleanupStep::remove(
        provider,
        target.to_path_buf(),
        "sentinel_match",
    ))
}

/// Shared destructive routine: plan + execute. The destructive
/// `fs::remove_file` is only invoked when the plan reports a write.
fn remove_workhive_script_at(
    target: &Utf8Path,
    provider: AiActivityProvider,
) -> Result<CleanupStep, IntegrationError> {
    let step = plan_remove_workhive_script_at(target, provider)?;
    if step.would_write {
        fs::remove_file(&step.target).map_err(|e| io_err("remove", step.target.clone(), e))?;
    }
    Ok(step)
}

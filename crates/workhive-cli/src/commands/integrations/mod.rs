//! Automatic AI-assistant integration preparation lifecycle.
//!
//! ## Current product contract
//!
//! Workhive does not install provider-side hooks or plugins.
//! [`prepare_provider`] is a cleanup-and-notify lifecycle:
//!
//! * Sweeps any Workhive-owned hook entries, hook scripts, and plugin
//!   files left behind by earlier releases. User-authored artifacts
//!   at the same paths are preserved verbatim.
//! * Installs/refreshes the command-native Codex `notify` wrapper
//!   (the only remaining write path). The wrapper records completion
//!   through Workhive's internal recorder and forwards to any
//!   user-configured `notify` command, fail-open.
//!
//! ## Per-provider closed loop
//!
//! Each provider helper computes an inspectable [`CleanupReport`]
//! ([`cleanup::CleanupReport`]) first, then performs filesystem
//! mutations only for steps that report `would_write = true`. Repeated
//! calls on an already-clean configuration are therefore observably
//! idempotent: no backup file is created, no third-party config is
//! rewritten, and no Workhive-owned script is touched.
//!
//! Edits to user-owned config files are preceded by a
//! `<config>.workhive-backup-<unix_ms>` copy via
//! [`sentinel::backup_file`] — and only when an actual rewrite is
//! about to occur.
//!
//! ## Error surface
//!
//! Every cleanup path returns `Result<_, IntegrationError>`. Missing
//! files map to `CleanupAction::Missing` (`would_write = false`), not
//! to errors; permission errors, parse errors, and atomic-write
//! failures propagate as typed [`IntegrationError`] so the supervisor
//! / `init --global` can route them through
//! [`IntegrationError::classify`] without ever masquerading a real
//! failure as "already current".

use workhive_core::domain::AiActivityProvider;
use workhive_core::ports::{AppConfig, ExecutableAvailabilityPort};
use workhive_core::usecases::ai_integrations::{CodexFeatureConfig, FeatureGate};

mod change;
mod claude;
mod cleanup;
mod codex;
pub(crate) mod error;
mod opencode;
mod paths;
mod scripts;
mod sentinel;

use change::InstallChange;
use cleanup::CleanupReport;
pub(crate) use error::{IntegrationError, PrepareErrorKind};

/// Aggregated outcome of one provider's preparation pass.
///
/// The lifecycle caller does not surface these on success — the happy
/// path is silent — but it uses the outcome internally for
/// `tracing::debug!` and for integration tests that need to assert
/// "no backup was created on the second prepare".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparationOutcome {
    /// Every cleanup step was a no-op (`Missing` / `AlreadyClean` /
    /// `Preserve`) and no notify-wrapper rewrite was needed. No
    /// third-party file was touched, no backup was taken.
    AlreadyCurrent,
    /// At least one step performed a write — a legacy artifact was
    /// removed, or the Codex notify wrapper was installed/refreshed.
    Updated,
}

/// Silent, provider-scoped preparation entry point.
///
/// Called by:
/// * [`crate::commands::pane_supervisor::cmd_internal_pane_supervisor`]
///   immediately before spawning a Claude/Codex/OpenCode child.
/// * [`crate::commands::project::execute_init`] in the `--global`
///   branch as an opportunistic maintenance pass over already
///   discoverable providers.
///
/// Returns [`PreparationOutcome::AlreadyCurrent`] when nothing on
/// disk needed updating, otherwise [`PreparationOutcome::Updated`].
///
/// Errors surface as [`IntegrationError`] so the caller may classify
/// them via [`IntegrationError::classify`].
pub(crate) fn prepare_provider(
    provider: AiActivityProvider,
) -> Result<PreparationOutcome, IntegrationError> {
    prepare_provider_with_config(provider, None)
}

/// Config-aware version of [`prepare_provider`]. Accepts an optional
/// merged `AppConfig` to resolve Codex-specific settings (notify).
/// Falls back to built-in defaults when `merged_config` is `None`.
pub(crate) fn prepare_provider_with_config(
    provider: AiActivityProvider,
    merged_config: Option<&AppConfig>,
) -> Result<PreparationOutcome, IntegrationError> {
    match provider {
        AiActivityProvider::ClaudeCode => prepare_claude(),
        AiActivityProvider::Codex => prepare_codex(CodexFeatureConfig::from_config(merged_config)),
        AiActivityProvider::Opencode => prepare_opencode(),
        // `None` is not a Workhive-managed provider; the preparation
        // lifecycle never writes anything in that case.
        AiActivityProvider::None => Ok(PreparationOutcome::AlreadyCurrent),
    }
}

/// Gate-aware wrapper around [`prepare_provider`].
///
/// Returns `Ok(None)` when the AI integrations feature gate currently
/// disables this provider (globally or per-provider). That
/// short-circuit is silent on every dimension: no cleanup is
/// performed, no third-party config is touched, no `tracing::warn!`
/// is emitted.
pub(crate) fn prepare_provider_if_enabled(
    provider: AiActivityProvider,
    gate: &FeatureGate,
) -> Result<Option<PreparationOutcome>, IntegrationError> {
    prepare_provider_if_enabled_with_config(provider, gate, None)
}

/// Config-aware version of [`prepare_provider_if_enabled`].
pub(crate) fn prepare_provider_if_enabled_with_config(
    provider: AiActivityProvider,
    gate: &FeatureGate,
    merged_config: Option<&AppConfig>,
) -> Result<Option<PreparationOutcome>, IntegrationError> {
    if !gate.is_provider_enabled(provider) {
        tracing::debug!(
            provider = provider.wire_name(),
            "AI integration prepare gated off; skipping silently",
        );
        return Ok(None);
    }
    Ok(Some(prepare_provider_with_config(provider, merged_config)?))
}

/// Claude Code preparation pass.
///
/// Sweeps any Workhive-owned hook entries in
/// `~/.claude/settings.json` and the legacy hook script at
/// `<global_config>/integrations/claude_code/workhive-claude-hook.sh`.
/// User-authored hook entries and user-authored files at the script
/// slot path are preserved.
fn prepare_claude() -> Result<PreparationOutcome, IntegrationError> {
    let mut report = CleanupReport::new();
    let settings_path = paths::claude_settings_path()?;
    report.push(claude::remove(&settings_path)?);
    report.push(scripts::remove_claude_hook_script()?);
    Ok(outcome_from_report(&report))
}

/// Codex preparation pass.
///
/// Two layers:
///
/// 1. Cleanup: sweeps any Workhive-owned `[[hooks.*]]` entries in
///    `~/.codex/config.toml` and the legacy hook script at
///    `<global_config>/integrations/codex/workhive-codex-hook.sh`.
/// 2. Notify: when the feature gate's
///    `features.ai_integrations.providers.codex.notify.enabled = true`,
///    installs/refreshes the sentinel-bracketed `notify` wrapper.
///    The wrapper records `NotifyCompletion` through the internal
///    `<bin> internal activity codex-notify` endpoint and forwards
///    to the user's pre-existing `notify` command (if any),
///    fail-open. When disabled, the notify layer is completely
///    silent.
fn prepare_codex(codex_cfg: CodexFeatureConfig) -> Result<PreparationOutcome, IntegrationError> {
    let cfg_path = paths::codex_config_path()?;
    let mut report = CleanupReport::new();
    report.push(codex::remove(&cfg_path)?);
    report.push(scripts::remove_codex_hook_script()?);

    let notify_change = if codex_cfg.notify_enabled {
        // The notify wrapper resolves the recording binary via
        // `${WORKHIVE_BIN}` at runtime (set by the pane supervisor on
        // every managed launch) and falls back to the install-time
        // `current_exe()` path for ad-hoc invocations. Resolution
        // failure here is non-fatal: dropping the fallback is strictly
        // better than embedding a malformed path.
        let install_time_fallback = current_workhive_bin_for_notify_fallback();
        codex::install_notify(&cfg_path, install_time_fallback.as_deref())?
    } else {
        InstallChange::AlreadyCurrent
    };

    let any_write = report.any_write() || notify_change.was_written();
    Ok(if any_write {
        PreparationOutcome::Updated
    } else {
        PreparationOutcome::AlreadyCurrent
    })
}

/// Resolve the install-time fallback path the Codex notify wrapper
/// will embed for the "outside Workhive-managed shell" case.
///
/// Returns `None` (logged at `warn`) when the OS cannot resolve a
/// canonical executable path or that path is not UTF-8. The notify
/// wrapper still installs in that case — the `${WORKHIVE_BIN:-}`
/// branch keeps recording on managed launches.
fn current_workhive_bin_for_notify_fallback() -> Option<camino::Utf8PathBuf> {
    match std::env::current_exe() {
        Ok(path) => match camino::Utf8PathBuf::try_from(path) {
            Ok(utf8) => Some(utf8),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "Codex notify install: current_exe path is not UTF-8; \
                     installing wrapper without fallback (managed launches \
                     still record via ${{WORKHIVE_BIN}})"
                );
                None
            }
        },
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Codex notify install: cannot resolve current_exe; \
                 installing wrapper without fallback (managed launches \
                 still record via ${{WORKHIVE_BIN}})"
            );
            None
        }
    }
}

/// OpenCode preparation pass.
///
/// Sweeps any Workhive-owned plugin file at the OpenCode plugin slot
/// (`$XDG_CONFIG_HOME/opencode/plugins/workhive.mjs` or the `$HOME`
/// fallback). User-authored files at the same path are preserved.
fn prepare_opencode() -> Result<PreparationOutcome, IntegrationError> {
    let plugin_path = paths::opencode_plugin_path()?;
    let mut report = CleanupReport::new();
    report.push(opencode::remove(&plugin_path)?);
    Ok(outcome_from_report(&report))
}

/// Aggregate a [`CleanupReport`] into the lifecycle's
/// [`PreparationOutcome`] surface. A report that produced any write
/// maps to `Updated`; otherwise `AlreadyCurrent`.
fn outcome_from_report(report: &CleanupReport) -> PreparationOutcome {
    if report.any_write() {
        PreparationOutcome::Updated
    } else {
        PreparationOutcome::AlreadyCurrent
    }
}

/// Inspect-only entry point: build a [`CleanupReport`] for `provider`
/// without performing any filesystem write. Used by the internal
/// dry-run / inspection contract and by tests asserting plan ↔
/// execute symmetry.
///
/// The report contains one [`cleanup::CleanupStep`] per surface the
/// destructive `prepare_provider` would visit (the provider config
/// file plus the legacy script path, where applicable). The Codex
/// notify wrapper is *not* part of the cleanup report — notify is a
/// write path managed separately through [`codex::install_notify`].
#[cfg(test)]
pub(crate) fn plan_cleanup(
    provider: AiActivityProvider,
) -> Result<CleanupReport, IntegrationError> {
    let mut report = CleanupReport::new();
    match provider {
        AiActivityProvider::ClaudeCode => {
            report.push(claude::plan_remove(&paths::claude_settings_path()?)?);
            report.push(scripts::plan_remove_claude_hook_script()?);
        }
        AiActivityProvider::Codex => {
            report.push(codex::plan_remove(&paths::codex_config_path()?)?);
            report.push(scripts::plan_remove_codex_hook_script()?);
        }
        AiActivityProvider::Opencode => {
            report.push(opencode::plan_remove(&paths::opencode_plugin_path()?)?);
        }
        AiActivityProvider::None => {}
    }
    Ok(report)
}

/// Opportunistic preparation pass invoked by `workhive init --global`.
///
/// Iterates over [`AiActivityProvider::ClaudeCode`],
/// [`AiActivityProvider::Codex`], and [`AiActivityProvider::Opencode`].
/// For each provider, if the corresponding executable
/// (`claude`/`codex`/`opencode`) is discoverable on `PATH`, calls
/// [`prepare_provider`] to bring the cleanup + notify state up to
/// date.
///
/// Providers that are not discoverable are skipped entirely — no
/// third-party config is materialized for a tool the user has not
/// installed.
///
/// `init --global` is the bootstrap-from-scratch entry point and
/// must never block on integration failures: per-provider errors are
/// logged at `warn` and the loop continues. The pane supervisor's
/// strong-fallback prepare retries on the next pane launch.
pub(crate) fn opportunistic_prepare_discoverable(
    checker: &dyn ExecutableAvailabilityPort,
    gate: &FeatureGate,
) -> PreparationOutcome {
    if !gate.globally_enabled() {
        tracing::debug!(
            "init --global: AI integrations feature gate disabled; skipping all providers",
        );
        return PreparationOutcome::AlreadyCurrent;
    }
    let mut any_updated = false;
    for (provider, executable) in [
        (AiActivityProvider::ClaudeCode, "claude"),
        (AiActivityProvider::Codex, "codex"),
        (AiActivityProvider::Opencode, "opencode"),
    ] {
        if !gate.is_provider_enabled(provider) {
            tracing::debug!(
                provider = provider.wire_name(),
                "init --global: provider gated off; skipping integration prepare",
            );
            continue;
        }
        if !checker.is_available(executable) {
            tracing::debug!(
                provider = provider.wire_name(),
                executable,
                "init --global: provider not discoverable on PATH; skipping integration prepare",
            );
            continue;
        }
        match prepare_provider(provider) {
            Ok(PreparationOutcome::AlreadyCurrent) => {
                tracing::debug!(
                    provider = provider.wire_name(),
                    "init --global: integration already up to date",
                );
            }
            Ok(PreparationOutcome::Updated) => {
                tracing::debug!(
                    provider = provider.wire_name(),
                    "init --global: integration prepared",
                );
                any_updated = true;
            }
            Err(err) => {
                let kind = err.classify();
                tracing::warn!(
                    provider = provider.wire_name(),
                    kind = ?kind,
                    error = %err,
                    "init --global: automatic integration prepare failed; pane supervisor will retry on next launch",
                );
            }
        }
    }
    if any_updated {
        PreparationOutcome::Updated
    } else {
        PreparationOutcome::AlreadyCurrent
    }
}

#[cfg(test)]
mod tests;

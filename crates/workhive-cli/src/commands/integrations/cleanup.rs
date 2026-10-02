//! Inspectable cleanup decision/report model.
//!
//! The integration preparation lifecycle visits one or more provider
//! artifacts per pass — Workhive-owned hook entries inside a user's
//! official tool config, Workhive-authored hook script files, and the
//! Workhive OpenCode plugin file. Each visit produces a typed
//! [`CleanupStep`] describing *what* the lifecycle would (or did) do
//! at *which* target path and *why*.
//!
//! Steps roll up into a [`CleanupReport`] which is the single source of
//! truth that callers consult to decide whether a write occurred. The
//! report is structured so an in-process inspector (or a future
//! diagnostic command surface) can render a dry-run plan without
//! mutating the user's config.
//!
//! The provider helpers expose `plan_*` entry points that produce the
//! same `CleanupStep` shape without performing any backup, atomic write,
//! or file deletion. `execute_*` (or `remove`) entry points first run
//! `plan_*` and then perform the destructive operation only when the
//! step indicates a write is required.

use camino::Utf8PathBuf;

use workhive_core::domain::AiActivityProvider;

/// Classification of what cleanup will (or did) do at a particular
/// target path.
///
/// The variants intentionally separate "file is absent" from "file is
/// present but contains no Workhive-owned material": both are
/// already-clean from the user's perspective, but only the second is
/// observable as on-disk content the inspector can show. Keeping them
/// distinct lets the inspector explain to the user *why* a step is a
/// no-op without conflating missing-file with already-clean-file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CleanupAction {
    /// The target file is present and contains Workhive-owned
    /// material. Cleanup will (or did) remove that material.
    Remove,
    /// The target file is present but contains only user-authored
    /// material. Cleanup leaves the file untouched.
    Preserve,
    /// The target file is present and Workhive-owned in shape (it
    /// passes the sentinel check or sits inside a Workhive-managed
    /// config region), but already contains no Workhive material that
    /// would need removing.
    AlreadyClean,
    /// The target file does not exist on disk.
    Missing,
}

/// One row in a [`CleanupReport`]: what `action` will happen at
/// `target` for `provider`, with a short stable `reason` keyword and
/// the boolean `would_write` signal callers use to short-circuit
/// backup/atomic-write paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CleanupStep {
    pub(crate) provider: AiActivityProvider,
    pub(crate) target: Utf8PathBuf,
    pub(crate) action: CleanupAction,
    /// Stable, untranslated keyword describing why this action was
    /// chosen (e.g. `"sentinel_match"`, `"user_authored"`,
    /// `"no_managed_entry"`, `"missing"`). Tests assert on this so it
    /// must remain stable; callers route translations through
    /// [`workhive_core::i18n_keys`] when surfacing to users.
    pub(crate) reason: &'static str,
    /// Whether executing this step would (or did) write to disk.
    /// `false` for `Preserve`, `AlreadyClean`, `Missing`; `true` for
    /// `Remove`. Backup + atomic write paths must consult this flag
    /// before creating any backup so idempotent reruns leave the user's
    /// config byte-for-byte unchanged.
    pub(crate) would_write: bool,
}

impl CleanupStep {
    pub(crate) fn missing(provider: AiActivityProvider, target: Utf8PathBuf) -> Self {
        Self {
            provider,
            target,
            action: CleanupAction::Missing,
            reason: "missing",
            would_write: false,
        }
    }

    pub(crate) fn preserve(
        provider: AiActivityProvider,
        target: Utf8PathBuf,
        reason: &'static str,
    ) -> Self {
        Self {
            provider,
            target,
            action: CleanupAction::Preserve,
            reason,
            would_write: false,
        }
    }

    pub(crate) fn already_clean(
        provider: AiActivityProvider,
        target: Utf8PathBuf,
        reason: &'static str,
    ) -> Self {
        Self {
            provider,
            target,
            action: CleanupAction::AlreadyClean,
            reason,
            would_write: false,
        }
    }

    pub(crate) fn remove(
        provider: AiActivityProvider,
        target: Utf8PathBuf,
        reason: &'static str,
    ) -> Self {
        Self {
            provider,
            target,
            action: CleanupAction::Remove,
            reason,
            would_write: true,
        }
    }
}

/// Aggregate of every [`CleanupStep`] produced by a single
/// `prepare_provider` pass.
///
/// The report is the structural input the lifecycle uses to roll up a
/// [`super::PreparationOutcome`]: if any step has `would_write == true`,
/// the pass reports `Updated`; otherwise it reports `AlreadyCurrent`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CleanupReport {
    pub(crate) steps: Vec<CleanupStep>,
}

impl CleanupReport {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, step: CleanupStep) {
        self.steps.push(step);
    }

    /// Whether any step in the report would write (or did write) to
    /// disk. Drives the [`super::PreparationOutcome::Updated`] vs
    /// [`super::PreparationOutcome::AlreadyCurrent`] decision and is
    /// what backup/atomic-write gates consult to avoid spurious
    /// backups on a no-op pass.
    pub(crate) fn any_write(&self) -> bool {
        self.steps.iter().any(|s| s.would_write)
    }
}

//! Snapshot side-effect routing for `workspace open`.
//!
//! `cmd_open` must drive four distinct kinds of snapshot-runtime side effects
//! after a successful open:
//!
//! 1. **Launch baseline persistence** (one of three flavors depending on
//!    [`OpenRecoveryPath`]).
//! 2. **Snapshot monitor process spawn**.
//! 3. **Snapshot hook installation/removal** (driven indirectly through the
//!    detached monitor process and the snapshot capture hook handshake).
//! 4. **Tmux compatibility warning surface**.
//!
//! Per the snapshot directory creation policy,
//! when [`SnapshotOpenPolicy::Disabled`] is in effect *none* of these side
//! effects may run — including the previously-existing "cleanup-on-open" call
//! that surfaced spurious tmux hook compatibility warnings on hosts where the
//! `.tmux.conf` integration is not supported.
//!
//! A default-running test proves the
//! disabled policy never persists a baseline, never spawns a monitor, and
//! never installs/removes hooks. Because the CLI composition is concrete
//! (no trait seam at the binary boundary), the tests extract a small
//! helper that takes fake ports/callbacks and exercise that helper
//! directly.
//!
//! The helper is [`dispatch_snapshot_open_side_effects`] — a single function
//! that consumes the snapshot-open policy together with the recovery path and
//! routes through the [`SnapshotOpenSideEffects`] trait. The CLI composition
//! provides one implementation; tests provide a counter-fake.

use super::open::OpenRecoveryPath;
use super::snapshot_open_policy::SnapshotOpenPolicy;

/// Side-effect endpoints driven by [`dispatch_snapshot_open_side_effects`]
/// when the snapshot-open policy is [`SnapshotOpenPolicy::Enabled`].
///
/// The trait is *behavioral*: each method represents an operation that must
/// not run when the policy is disabled. Keeping these as methods (rather than
/// raw closures) lets unit tests assert per-operation call counts and lets the
/// CLI implementation reuse a single struct that captures all the borrowed
/// state needed to perform the work.
pub trait SnapshotOpenSideEffects {
    /// Error type surfaced by baseline-persistence and monitor-spawn paths.
    type Error;

    /// Persist the launch baseline by ensuring it was already snapshotted by
    /// the restored session.
    ///
    /// Called only when [`OpenRecoveryPath::RestoredFromSnapshot`] applies.
    fn ensure_baseline_for_restored_snapshot(&mut self) -> Result<(), Self::Error>;

    /// Persist the launch baseline by bootstrapping it from the live tmux
    /// session being reused.
    ///
    /// Called only when [`OpenRecoveryPath::ReusedLiveSession`] applies.
    fn ensure_or_bootstrap_baseline_for_reused_session(&mut self) -> Result<(), Self::Error>;

    /// Persist the launch baseline from the freshly applied registry layout.
    ///
    /// Called only when [`OpenRecoveryPath::FellBackToLayout`] or
    /// [`OpenRecoveryPath::ForceRecreatedFromLayout`] applies.
    fn persist_baseline_from_registry(&mut self) -> Result<(), Self::Error>;

    /// Spawn the detached snapshot monitor supervisor process.
    ///
    /// The monitor in turn handles snapshot-hook installation and removal.
    /// If this method is never called, no snapshot hook ever gets installed
    /// or removed by the open path.
    fn spawn_snapshot_monitor(&mut self) -> Result<(), Self::Error>;

    /// Surface the localized "snapshot runtime skipped" notice to the user.
    ///
    /// Called only when the policy is [`SnapshotOpenPolicy::Disabled`].
    fn report_skipped(&mut self);
}

/// Outcome of the snapshot-open side-effect dispatch.
///
/// `Skipped` proves that the disabled policy short-circuited before any of the
/// `SnapshotOpenSideEffects` mutation methods were touched. `Active` carries
/// the result of the chained baseline-persist + monitor-spawn pipeline so the
/// CLI layer can record the open supervisor outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOpenSideEffectOutcome<E> {
    /// Snapshot runtime skipped — policy was [`SnapshotOpenPolicy::Disabled`].
    Skipped,
    /// Snapshot runtime active — baseline persistence and monitor spawn ran.
    ///
    /// The inner `Result` chains the baseline-persistence outcome with the
    /// monitor-spawn outcome via early return; on baseline failure the monitor
    /// is not spawned (matching the historical CLI semantics).
    Active(Result<(), E>),
}

/// Route `workspace open` to the correct snapshot side effects.
///
/// Single source of truth for the snapshot-runtime branch of `cmd_open`.
///
/// When the policy is [`SnapshotOpenPolicy::Disabled`] the function:
/// - calls [`SnapshotOpenSideEffects::report_skipped`] exactly once;
/// - never calls any baseline-persistence method;
/// - never calls [`SnapshotOpenSideEffects::spawn_snapshot_monitor`];
/// - returns [`SnapshotOpenSideEffectOutcome::Skipped`].
///
/// When the policy is [`SnapshotOpenPolicy::Enabled`] the function selects a
/// single baseline-persistence method based on `recovery`, then chains the
/// monitor-spawn call via `Result::and_then`. On baseline failure the monitor
/// is *not* spawned.
pub fn dispatch_snapshot_open_side_effects<S: SnapshotOpenSideEffects>(
    policy: SnapshotOpenPolicy,
    recovery: &OpenRecoveryPath,
    side_effects: &mut S,
) -> SnapshotOpenSideEffectOutcome<S::Error> {
    match policy {
        SnapshotOpenPolicy::Disabled => {
            side_effects.report_skipped();
            SnapshotOpenSideEffectOutcome::Skipped
        }
        SnapshotOpenPolicy::Enabled => {
            let baseline_result = match recovery {
                OpenRecoveryPath::RestoredFromSnapshot => {
                    side_effects.ensure_baseline_for_restored_snapshot()
                }
                OpenRecoveryPath::ReusedLiveSession => {
                    side_effects.ensure_or_bootstrap_baseline_for_reused_session()
                }
                OpenRecoveryPath::FellBackToLayout | OpenRecoveryPath::ForceRecreatedFromLayout => {
                    side_effects.persist_baseline_from_registry()
                }
            };
            let combined = baseline_result.and_then(|()| side_effects.spawn_snapshot_monitor());
            SnapshotOpenSideEffectOutcome::Active(combined)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counter-fake implementation tracking which side-effect endpoints were
    /// touched. Per-method counters let us assert precisely "method N was
    /// invoked exactly K times".
    #[derive(Debug, Default)]
    struct TrackingSideEffects {
        baseline_restored_calls: usize,
        baseline_reused_calls: usize,
        baseline_registry_calls: usize,
        monitor_spawn_calls: usize,
        skipped_reports: usize,
        baseline_should_fail: bool,
        monitor_should_fail: bool,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FakeError(&'static str);

    impl SnapshotOpenSideEffects for TrackingSideEffects {
        type Error = FakeError;

        fn ensure_baseline_for_restored_snapshot(&mut self) -> Result<(), Self::Error> {
            self.baseline_restored_calls += 1;
            if self.baseline_should_fail {
                Err(FakeError("baseline restored failed"))
            } else {
                Ok(())
            }
        }

        fn ensure_or_bootstrap_baseline_for_reused_session(&mut self) -> Result<(), Self::Error> {
            self.baseline_reused_calls += 1;
            if self.baseline_should_fail {
                Err(FakeError("baseline reused failed"))
            } else {
                Ok(())
            }
        }

        fn persist_baseline_from_registry(&mut self) -> Result<(), Self::Error> {
            self.baseline_registry_calls += 1;
            if self.baseline_should_fail {
                Err(FakeError("baseline registry failed"))
            } else {
                Ok(())
            }
        }

        fn spawn_snapshot_monitor(&mut self) -> Result<(), Self::Error> {
            self.monitor_spawn_calls += 1;
            if self.monitor_should_fail {
                Err(FakeError("monitor spawn failed"))
            } else {
                Ok(())
            }
        }

        fn report_skipped(&mut self) {
            self.skipped_reports += 1;
        }
    }

    impl TrackingSideEffects {
        fn baseline_call_total(&self) -> usize {
            self.baseline_restored_calls + self.baseline_reused_calls + self.baseline_registry_calls
        }
    }

    /// All four recovery paths under `Disabled` must short-circuit without
    /// touching any side-effect endpoint other than `report_skipped`.
    ///
    /// Default-running fake-port test:
    /// disabled `workspace open` must not write a baseline, must not spawn the
    /// monitor, and (because hook install/remove is driven through the
    /// monitor) must therefore not install or remove snapshot hooks.
    #[test]
    fn disabled_policy_skips_all_side_effects_for_every_recovery_path() {
        let recovery_paths = [
            OpenRecoveryPath::RestoredFromSnapshot,
            OpenRecoveryPath::ReusedLiveSession,
            OpenRecoveryPath::FellBackToLayout,
            OpenRecoveryPath::ForceRecreatedFromLayout,
        ];
        for recovery in &recovery_paths {
            let mut tracker = TrackingSideEffects::default();
            let outcome = dispatch_snapshot_open_side_effects(
                SnapshotOpenPolicy::Disabled,
                recovery,
                &mut tracker,
            );
            assert_eq!(
                outcome,
                SnapshotOpenSideEffectOutcome::Skipped,
                "recovery={recovery:?}: disabled policy must yield Skipped outcome",
            );
            assert_eq!(
                tracker.baseline_call_total(),
                0,
                "recovery={recovery:?}: disabled policy must not persist any baseline",
            );
            assert_eq!(
                tracker.monitor_spawn_calls, 0,
                "recovery={recovery:?}: disabled policy must not spawn the snapshot monitor",
            );
            assert_eq!(
                tracker.skipped_reports, 1,
                "recovery={recovery:?}: disabled policy must report skipped exactly once",
            );
        }
    }

    #[test]
    fn enabled_policy_with_restored_snapshot_calls_only_restored_baseline_then_monitor() {
        let mut tracker = TrackingSideEffects::default();
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::RestoredFromSnapshot,
            &mut tracker,
        );
        assert_eq!(outcome, SnapshotOpenSideEffectOutcome::Active(Ok(())));
        assert_eq!(tracker.baseline_restored_calls, 1);
        assert_eq!(tracker.baseline_reused_calls, 0);
        assert_eq!(tracker.baseline_registry_calls, 0);
        assert_eq!(tracker.monitor_spawn_calls, 1);
        assert_eq!(tracker.skipped_reports, 0);
    }

    #[test]
    fn enabled_policy_with_reused_live_session_calls_only_reused_baseline_then_monitor() {
        let mut tracker = TrackingSideEffects::default();
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::ReusedLiveSession,
            &mut tracker,
        );
        assert_eq!(outcome, SnapshotOpenSideEffectOutcome::Active(Ok(())));
        assert_eq!(tracker.baseline_restored_calls, 0);
        assert_eq!(tracker.baseline_reused_calls, 1);
        assert_eq!(tracker.baseline_registry_calls, 0);
        assert_eq!(tracker.monitor_spawn_calls, 1);
        assert_eq!(tracker.skipped_reports, 0);
    }

    #[test]
    fn enabled_policy_with_fell_back_to_layout_calls_only_registry_baseline_then_monitor() {
        let mut tracker = TrackingSideEffects::default();
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::FellBackToLayout,
            &mut tracker,
        );
        assert_eq!(outcome, SnapshotOpenSideEffectOutcome::Active(Ok(())));
        assert_eq!(tracker.baseline_restored_calls, 0);
        assert_eq!(tracker.baseline_reused_calls, 0);
        assert_eq!(tracker.baseline_registry_calls, 1);
        assert_eq!(tracker.monitor_spawn_calls, 1);
    }

    #[test]
    fn enabled_policy_with_force_recreated_from_layout_calls_only_registry_baseline_then_monitor() {
        let mut tracker = TrackingSideEffects::default();
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::ForceRecreatedFromLayout,
            &mut tracker,
        );
        assert_eq!(outcome, SnapshotOpenSideEffectOutcome::Active(Ok(())));
        assert_eq!(tracker.baseline_restored_calls, 0);
        assert_eq!(tracker.baseline_reused_calls, 0);
        assert_eq!(tracker.baseline_registry_calls, 1);
        assert_eq!(tracker.monitor_spawn_calls, 1);
    }

    /// On baseline persistence failure the monitor must not be spawned.
    /// This protects against the historical bug where a half-persisted
    /// baseline would still be supervised.
    #[test]
    fn enabled_policy_does_not_spawn_monitor_when_baseline_persist_fails() {
        let mut tracker = TrackingSideEffects {
            baseline_should_fail: true,
            ..TrackingSideEffects::default()
        };
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::FellBackToLayout,
            &mut tracker,
        );
        assert_eq!(
            outcome,
            SnapshotOpenSideEffectOutcome::Active(Err(FakeError("baseline registry failed"))),
        );
        assert_eq!(tracker.baseline_registry_calls, 1);
        assert_eq!(
            tracker.monitor_spawn_calls, 0,
            "monitor must not spawn when baseline persistence fails",
        );
        assert_eq!(tracker.skipped_reports, 0);
    }

    /// Monitor-spawn failure must surface through `Active(Err)` while still
    /// counting the spawn attempt — proves the chaining wiring is correct.
    #[test]
    fn enabled_policy_propagates_monitor_spawn_failure() {
        let mut tracker = TrackingSideEffects {
            monitor_should_fail: true,
            ..TrackingSideEffects::default()
        };
        let outcome = dispatch_snapshot_open_side_effects(
            SnapshotOpenPolicy::Enabled,
            &OpenRecoveryPath::ReusedLiveSession,
            &mut tracker,
        );
        assert_eq!(
            outcome,
            SnapshotOpenSideEffectOutcome::Active(Err(FakeError("monitor spawn failed"))),
        );
        assert_eq!(tracker.baseline_reused_calls, 1);
        assert_eq!(tracker.monitor_spawn_calls, 1);
    }
}

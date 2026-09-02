//! Bounded pane-readiness wait for `workhive workspace open`.
//!
//! When `open` finishes provisioning a fresh tmux session and applying
//! its layout, the panes inside the session may still be racing through
//! shell startup. Attaching too early lands the user at a half-painted
//! prompt or a partial recovery view.
//!
//! This module observes pane readiness through the existing
//! [`PaneRuntimePort::list_units`] surface — it never parses terminal
//! screen output and never spawns an async runtime. The wait has a hard
//! upper bound (default [`DEFAULT_BUDGET_MS`]); on timeout, the user
//! sees a degraded warning and `open` proceeds to attach anyway, never
//! hanging the process.
//!
//! Readiness is *observable*: a pane is considered ready only when its
//! [`RuntimeUnitSnapshot`] reports a non-default state (`Running` or
//! `Idle`) AND a non-empty foreground command. Early frames where tmux
//! has reported the pane id but its shell has not yet exec'd anything
//! correspond to `state = Unknown` and/or `command = ""`, which the
//! readiness check treats as "still warming up". Counting pane *ids*
//! alone is no longer sufficient — that races against the shell exec.
//!
//! The wait is skipped for sessions that were reused live (no fresh
//! pane lifecycle). For `--no-attach` runs the readiness wait is also
//! skipped (no attach to gate), but a deterministic progress line is
//! still emitted so the user always sees the same observable lifecycle
//! summary regardless of attach mode.

use std::time::Instant;

use workhive_core::domain::{Locale, PaneState};
use workhive_core::i18n_keys as keys;
use workhive_core::ports::{I18nPort, PaneRuntimePort, RuntimeUnitSnapshot};

/// Default upper bound on the readiness wait, in milliseconds.
///
/// Per the active workstream doc: "默认 8 秒". Chosen so that even slow
/// shell rc files (e.g., heavy zsh plugins) usually clear before
/// timeout, while never visibly hanging the user.
pub(crate) const DEFAULT_BUDGET_MS: u64 = 8_000;

/// Polling interval between `list_units` checks.
///
/// Short enough to feel responsive; long enough that the runtime adapter
/// is never hammered.
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u64 = 100;

/// Outcome of [`wait_for_panes_ready`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadinessOutcome {
    /// Observed `seen_count` panes within `elapsed_ms` ≤ budget.
    Ready { elapsed_ms: u64, seen_count: usize },
    /// Every expected pane is materially usable even though one or
    /// more panes has not reached the stricter "fully ready" shape.
    /// `workspace open` may attach on this outcome because the session
    /// exists, panes are observable, and the wait should not spend the
    /// full budget on cosmetic startup details.
    Usable { elapsed_ms: u64, seen_count: usize },
    /// Budget elapsed before observed pane count met `expected`.
    TimedOut {
        elapsed_ms: u64,
        seen_count: usize,
        expected: usize,
    },
    /// Runtime error prevented observation; caller should surface a
    /// degraded warning rather than fail open.
    Degraded { reason: String },
}

/// Sleeper trait so tests can drive the polling loop without
/// `thread::sleep` and without an async runtime.
pub(crate) trait Sleeper {
    fn sleep(&self, ms: u64);
}

/// Default real-clock sleeper used in production.
pub(crate) struct ThreadSleeper;
impl Sleeper for ThreadSleeper {
    fn sleep(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

/// Block until every expected pane has reached observable readiness, or
/// until `budget_ms` elapses.
///
/// Production callers should pass [`DEFAULT_BUDGET_MS`] and a real
/// [`ThreadSleeper`]. Tests inject a fake clock + sleeper.
///
/// Observable readiness criteria — applied per snapshot — see
/// [`is_unit_ready`]:
/// - state is not `Unknown` (we have a real Running/Idle observation), AND
/// - the unit has a non-empty foreground command name.
///
/// `expected` is the pane count we expect once startup finishes (e.g.
/// the layout pane count). When `expected == 0`, this returns
/// [`ReadinessOutcome::Ready`] immediately — there is nothing to observe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_for_panes_ready(
    runtime: &dyn PaneRuntimePort,
    group: &str,
    workspace: &str,
    expected: usize,
    budget_ms: u64,
    poll_interval_ms: u64,
    sleeper: &dyn Sleeper,
    started_at: Instant,
) -> ReadinessOutcome {
    if expected == 0 {
        return ReadinessOutcome::Ready {
            elapsed_ms: 0,
            seen_count: 0,
        };
    }

    let mut last_ready_count: usize = 0;
    let mut last_unit_count: usize = 0;
    // Track the most recent runtime error so a budget-exhausted, error-only
    // path can surface it via `Degraded` instead of swallowing it. Cleared
    // whenever we successfully observe the unit list so a transient error
    // does not stick on the final outcome.
    #[allow(unused_assignments)]
    let mut last_error: Option<String> = None;
    loop {
        match runtime.list_units(group, workspace) {
            Ok(units) => {
                last_error = None;
                last_unit_count = units.len();
                last_ready_count = units.iter().filter(|u| is_unit_ready(u)).count();
                let last_usable_count = units
                    .iter()
                    .filter(|u| is_unit_materially_usable(u))
                    .count();
                if last_ready_count >= expected {
                    return ReadinessOutcome::Ready {
                        elapsed_ms: u64::try_from(started_at.elapsed().as_millis())
                            .unwrap_or(budget_ms),
                        seen_count: last_ready_count,
                    };
                }
                if last_usable_count >= expected {
                    return ReadinessOutcome::Usable {
                        elapsed_ms: u64::try_from(started_at.elapsed().as_millis())
                            .unwrap_or(budget_ms),
                        seen_count: last_usable_count,
                    };
                }
            }
            Err(e) => {
                last_error = Some(e.to_string());
            }
        }

        let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(budget_ms);
        if elapsed_ms >= budget_ms {
            // Budget exhausted. Three distinct exit shapes:
            // - error-only (we never observed any unit list)
            // - we observed units but none reached observable readiness
            // - we observed some-but-not-all panes ready
            return match (last_unit_count, last_error) {
                (0, Some(reason)) => ReadinessOutcome::Degraded { reason },
                (_, _) => ReadinessOutcome::TimedOut {
                    elapsed_ms: budget_ms,
                    seen_count: last_ready_count,
                    expected,
                },
            };
        }
        sleeper.sleep(poll_interval_ms);
    }
}

/// Decide whether a single [`RuntimeUnitSnapshot`] is observably ready.
///
/// We rely only on fields the runtime adapter populates from its native
/// inspection API — never on terminal screen scraping. The criteria are:
///
/// - `state` is not `PaneState::Unknown` (a real Running/Idle reading), AND
/// - `command` is not empty (the shell has at least exec'd `argv[0]`).
///
/// Either of `state == Idle` or `state == Running` counts as "alive and
/// observed". An `Unknown` reading means the runtime adapter could not
/// determine state on this poll, which we conservatively treat as not
/// ready — letting the budget keep ticking instead of attaching too soon.
pub(crate) fn is_unit_ready(unit: &RuntimeUnitSnapshot) -> bool {
    if matches!(unit.state, PaneState::Unknown) {
        return false;
    }
    !unit.command.trim().is_empty()
}

/// Decide whether a pane is usable enough that `workspace open` can attach.
///
/// This is intentionally less strict than [`is_unit_ready`]: a shell or
/// tool pane can be safe to attach once the runtime can prove either a
/// non-default state or a foreground command. Fully unknown panes are
/// still not usable and remain bounded by the timeout/degraded path.
pub(crate) fn is_unit_materially_usable(unit: &RuntimeUnitSnapshot) -> bool {
    !matches!(unit.state, PaneState::Unknown) || !unit.command.trim().is_empty()
}

/// Render the user-facing line for a [`ReadinessOutcome`].
///
/// Reused by `open` so the rendering logic stays in one place and
/// remains test-friendly without driving `eprintln!` directly.
pub(crate) fn render_outcome(
    outcome: &ReadinessOutcome,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> Option<String> {
    match outcome {
        ReadinessOutcome::Ready {
            elapsed_ms,
            seen_count,
        }
        | ReadinessOutcome::Usable {
            elapsed_ms,
            seen_count,
        } => {
            if *seen_count == 0 {
                return None;
            }
            Some(i18n.resolve_with_args(
                locale,
                keys::OPEN_PROGRESS_PANES_READY,
                &[&seen_count.to_string(), &elapsed_ms.to_string()],
            ))
        }
        ReadinessOutcome::TimedOut {
            elapsed_ms,
            seen_count,
            expected,
        } => Some(i18n.resolve_with_args(
            locale,
            keys::OPEN_PROGRESS_READINESS_TIMEOUT,
            &[
                &elapsed_ms.to_string(),
                &seen_count.to_string(),
                &expected.to_string(),
            ],
        )),
        ReadinessOutcome::Degraded { reason } => {
            Some(i18n.resolve_with_args(locale, keys::OPEN_PROGRESS_READINESS_DEGRADED, &[reason]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use std::cell::RefCell;
    use workhive_core::domain::{PaneDirection, PaneKind, UnitId};
    use workhive_core::ports::{
        CreateWorkspaceRuntimeRequest, RuntimeError, SendInputRequest, SetUnitTitleRequest,
        SpawnUnitRequest, WorkspaceRuntimePort,
    };
    use workhive_infra::i18n::StaticI18n;
    use workhive_testkit::fakes::{FakeRuntime, RuntimeErrorInjection};

    struct ImmediateSleeper;
    impl Sleeper for ImmediateSleeper {
        fn sleep(&self, _ms: u64) {}
    }

    /// Inline runtime fake whose `list_units` returns a scripted sequence
    /// of `RuntimeUnitSnapshot` lists. Each call advances by one frame;
    /// the last frame is replayed if the loop polls past the script.
    /// Lets us simulate the realistic case where panes start with
    /// `state = Unknown` / empty command and only become observable a
    /// few polls later.
    struct ScriptedRuntime {
        frames: RefCell<std::vec::IntoIter<Vec<RuntimeUnitSnapshot>>>,
        last: RefCell<Vec<RuntimeUnitSnapshot>>,
    }
    impl ScriptedRuntime {
        fn new(frames: Vec<Vec<RuntimeUnitSnapshot>>) -> Self {
            assert!(!frames.is_empty(), "frames must not be empty");
            let last = frames.last().cloned().unwrap_or_default();
            Self {
                frames: RefCell::new(frames.into_iter()),
                last: RefCell::new(last),
            }
        }
    }
    impl PaneRuntimePort for ScriptedRuntime {
        fn spawn_unit(
            &self,
            _request: SpawnUnitRequest,
        ) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unreachable!("ScriptedRuntime is read-only for readiness tests")
        }
        fn close_unit(&self, _group: &str, _unit_id: &UnitId) -> Result<(), RuntimeError> {
            unreachable!("ScriptedRuntime is read-only for readiness tests")
        }
        fn list_units(
            &self,
            _group: &str,
            _workspace_name: &str,
        ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
            if let Some(next) = self.frames.borrow_mut().next() {
                *self.last.borrow_mut() = next.clone();
                Ok(next)
            } else {
                Ok(self.last.borrow().clone())
            }
        }
        fn current_unit(&self, _group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
            unreachable!("ScriptedRuntime is read-only for readiness tests")
        }
        fn send_input(&self, _request: SendInputRequest) -> Result<(), RuntimeError> {
            unreachable!("ScriptedRuntime is read-only for readiness tests")
        }
        fn set_unit_title(&self, _request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
            unreachable!("ScriptedRuntime is read-only for readiness tests")
        }
    }

    fn unit(id: &str, command: &str, state: PaneState) -> RuntimeUnitSnapshot {
        RuntimeUnitSnapshot {
            id: UnitId::new(id),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp"),
            command: command.to_owned(),
            active: false,
            kind: PaneKind::Terminal,
            state,
            tty: None,
            process_pid: None,
        }
    }

    fn make_runtime_with_units(units: usize) -> FakeRuntime {
        let runtime = FakeRuntime::new();
        runtime
            .create_workspace(CreateWorkspaceRuntimeRequest {
                group: "srv".to_owned(),
                workspace_name: "sess".to_owned(),
                working_dir: camino::Utf8PathBuf::from("/tmp"),
                env: Vec::new(),
            })
            .expect("create workspace");
        for _ in 1..units {
            runtime
                .spawn_unit(SpawnUnitRequest {
                    group: "srv".to_owned(),
                    workspace_name: "sess".to_owned(),
                    direction: PaneDirection::Right,
                    command: None,
                    target_unit: None,
                    size_percent: None,
                    env: Vec::new(),
                    cwd: None,
                })
                .expect("spawn unit");
        }
        runtime
    }

    #[test]
    fn ready_when_expected_is_zero() {
        let runtime = FakeRuntime::new();
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            0,
            DEFAULT_BUDGET_MS,
            DEFAULT_POLL_INTERVAL_MS,
            &ImmediateSleeper,
            started,
        );
        assert!(matches!(outcome, ReadinessOutcome::Ready { .. }));
    }

    #[test]
    fn ready_on_first_poll_when_units_already_present() {
        let runtime = make_runtime_with_units(3);
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            3,
            DEFAULT_BUDGET_MS,
            DEFAULT_POLL_INTERVAL_MS,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::Ready { seen_count, .. } => assert_eq!(seen_count, 3),
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn times_out_when_units_never_meet_expected() {
        // Runtime returns only 1 unit, but caller expects 3.
        let runtime = make_runtime_with_units(1);
        // Tiny budget + zero sleeper makes the real-clock loop exit fast.
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            3,
            50, // 50 ms budget
            10,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::TimedOut {
                seen_count,
                expected,
                ..
            } => {
                assert_eq!(seen_count, 1);
                assert_eq!(expected, 3);
            }
            other => panic!("expected TimedOut, got {other:?}"),
        }
    }

    #[test]
    fn degrades_when_list_units_keeps_failing() {
        let runtime = FakeRuntime::new();
        runtime
            .create_workspace(CreateWorkspaceRuntimeRequest {
                group: "srv".to_owned(),
                workspace_name: "sess".to_owned(),
                working_dir: camino::Utf8PathBuf::from("/tmp"),
                env: Vec::new(),
            })
            .expect("create workspace");
        runtime.inject_errors(RuntimeErrorInjection {
            list_units: Some(RuntimeError::BackendUnavailable {
                reason: workhive_core::ports::BackendUnavailableReason::Other,
                detail: "tmux server gone".to_owned(),
            }),
            ..Default::default()
        });
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            2,
            50,
            10,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::Degraded { reason } => {
                assert!(
                    reason.contains("tmux server gone"),
                    "degraded reason should propagate runtime error, got: {reason}",
                );
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn render_outcome_ready_renders_localized_message() {
        let i18n = StaticI18n::new();
        let outcome = ReadinessOutcome::Ready {
            elapsed_ms: 120,
            seen_count: 3,
        };
        let line = render_outcome(&outcome, &i18n, Locale::En).expect("rendered");
        assert!(line.contains('3'), "should mention pane count: {line}");
        assert!(line.contains("120"), "should mention elapsed ms: {line}");
    }

    #[test]
    fn render_outcome_zero_panes_returns_none() {
        let i18n = StaticI18n::new();
        let outcome = ReadinessOutcome::Ready {
            elapsed_ms: 0,
            seen_count: 0,
        };
        assert!(render_outcome(&outcome, &i18n, Locale::En).is_none());
    }

    #[test]
    fn render_outcome_timeout_includes_seen_and_expected() {
        let i18n = StaticI18n::new();
        let outcome = ReadinessOutcome::TimedOut {
            elapsed_ms: 8000,
            seen_count: 1,
            expected: 3,
        };
        let line = render_outcome(&outcome, &i18n, Locale::En).expect("rendered");
        assert!(line.contains("8000"));
        assert!(line.contains('1'));
        assert!(line.contains('3'));
    }

    #[test]
    fn render_outcome_zh_cn_uses_chinese_strings() {
        let i18n = StaticI18n::new();
        let outcome = ReadinessOutcome::TimedOut {
            elapsed_ms: 8000,
            seen_count: 0,
            expected: 1,
        };
        let line = render_outcome(&outcome, &i18n, Locale::ZhCn).expect("rendered");
        assert!(
            line.contains("超时"),
            "zh-CN timeout warning should be Chinese: {line}",
        );
    }

    // ─── Observable readiness tests (High 1) ──────────────────────────

    #[test]
    fn is_unit_ready_accepts_idle_with_command() {
        assert!(is_unit_ready(&unit("p1", "zsh", PaneState::Idle)));
    }

    #[test]
    fn is_unit_ready_accepts_running_with_command() {
        assert!(is_unit_ready(&unit("p1", "vim", PaneState::Running)));
    }

    #[test]
    fn is_unit_ready_rejects_unknown_state() {
        assert!(
            !is_unit_ready(&unit("p1", "zsh", PaneState::Unknown)),
            "Unknown state must not count as ready",
        );
    }

    #[test]
    fn is_unit_ready_rejects_empty_command() {
        assert!(
            !is_unit_ready(&unit("p1", "", PaneState::Idle)),
            "empty command must not count as ready",
        );
        assert!(
            !is_unit_ready(&unit("p1", "   ", PaneState::Idle)),
            "whitespace-only command must not count as ready",
        );
    }

    #[test]
    fn is_unit_materially_usable_accepts_state_without_command() {
        assert!(
            is_unit_materially_usable(&unit("p1", "", PaneState::Idle)),
            "a non-unknown runtime state is enough to attach without waiting the full budget",
        );
    }

    #[test]
    fn is_unit_materially_usable_accepts_command_without_state() {
        assert!(
            is_unit_materially_usable(&unit("p1", "zsh", PaneState::Unknown)),
            "a foreground command is enough to attach even if state classification lags",
        );
    }

    #[test]
    fn materially_usable_panes_do_not_wait_for_full_ready_budget() {
        let runtime = ScriptedRuntime::new(vec![vec![unit("p1", "", PaneState::Idle)]]);
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            1,
            DEFAULT_BUDGET_MS,
            DEFAULT_POLL_INTERVAL_MS,
            &ImmediateSleeper,
            started,
        );

        match outcome {
            ReadinessOutcome::Usable {
                seen_count,
                elapsed_ms,
            } => {
                assert_eq!(seen_count, 1);
                assert!(
                    elapsed_ms < DEFAULT_BUDGET_MS,
                    "usable panes must attach before the full readiness budget"
                );
            }
            other => panic!("expected Usable for state-observed pane, got {other:?}"),
        }
    }

    #[test]
    fn waits_until_panes_become_observably_ready() {
        // Frame 0: pane id allocated but state Unknown / no command.
        // Frame 1: still warming up.
        // Frame 2: real Idle + zsh — observable readiness reached.
        let runtime = ScriptedRuntime::new(vec![
            vec![unit("p1", "", PaneState::Unknown)],
            vec![unit("p1", "", PaneState::Unknown)],
            vec![unit("p1", "zsh", PaneState::Idle)],
        ]);
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            1,
            DEFAULT_BUDGET_MS,
            DEFAULT_POLL_INTERVAL_MS,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::Ready { seen_count, .. } => assert_eq!(seen_count, 1),
            other => panic!("expected Ready once observable, got {other:?}"),
        }
    }

    #[test]
    fn pane_count_alone_no_longer_satisfies_readiness() {
        // Three pane ids exist for the whole run, but their state stays
        // Unknown — the old "count >= expected" logic would have ruled
        // this Ready immediately. The new observable check must time out
        // instead, because no pane ever exposed a real runtime state.
        let unit_ids = vec![
            unit("p1", "", PaneState::Unknown),
            unit("p2", "", PaneState::Unknown),
            unit("p3", "", PaneState::Unknown),
        ];
        let runtime = ScriptedRuntime::new(vec![unit_ids.clone(), unit_ids.clone(), unit_ids]);
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            3,
            50, // tight budget keeps the test fast
            10,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::TimedOut {
                seen_count,
                expected,
                ..
            } => {
                assert_eq!(seen_count, 0, "no pane ever became observably ready");
                assert_eq!(expected, 3);
            }
            other => panic!("expected TimedOut, got {other:?}"),
        }
    }

    #[test]
    fn partial_readiness_is_not_enough() {
        // Two panes are spawned. One reaches Idle quickly, the other
        // stays Unknown forever. Readiness must NOT fire until both
        // panes are observable. With a tight budget we expect TimedOut
        // with seen_count == 1.
        let frames = vec![
            vec![
                unit("p1", "zsh", PaneState::Idle),
                unit("p2", "", PaneState::Unknown),
            ];
            5
        ];
        let runtime = ScriptedRuntime::new(frames);
        let started = Instant::now();
        let outcome = wait_for_panes_ready(
            &runtime,
            "srv",
            "sess",
            2,
            50,
            10,
            &ImmediateSleeper,
            started,
        );
        match outcome {
            ReadinessOutcome::TimedOut { seen_count, .. } => {
                assert_eq!(
                    seen_count, 1,
                    "should report only the observably-ready panes",
                );
            }
            other => panic!("expected TimedOut on partial readiness, got {other:?}"),
        }
    }

    #[test]
    fn never_parses_terminal_screen_output() {
        // Belt-and-suspenders check: this module must not depend on any
        // tmux capture / screen-scraping API. The fields used by
        // `is_unit_ready` are exactly the ones tmux already exposes via
        // `list_units` (state + command), nothing more. If new fields
        // ever leak in, the compiler forces this exhaustive destructure
        // to be revisited together with the readiness contract.
        let snap = unit("p1", "claude", PaneState::Running);
        let RuntimeUnitSnapshot {
            id: _,
            index: _,
            cwd: _,
            command: _,
            active: _,
            kind: _,
            state: _,
            tty: _,
            process_pid: _,
        } = snap.clone();
        assert!(is_unit_ready(&snap));
    }
}

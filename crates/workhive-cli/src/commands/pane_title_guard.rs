//! Bounded post-attach pane-title guard.
//!
//! Background process spawned by `workspace open` immediately before
//! `tmux attach` so the child can keep reasserting Workhive-owned pane
//! titles for a short window while interactive tools (Codex, OpenCode,
//! lazygit, …) settle in. Stops automatically once the configured
//! duration elapses.
//!
//! Architecture:
//!
//! * Pure [`parse_pane_title_args`] / [`schedule_tick`] helpers stay
//!   testable without I/O.
//! * `cmd_internal_pane_title_guard` is the thin runtime handler that
//!   wires the helpers to [`PaneRuntimePort::set_unit_title`].
//! * The CLI never depends on this guard for correctness — failure to
//!   reassert is logged at `warn` and swallowed (the testing policy
//!   "Fail-Open").

use std::time::{Duration, Instant};

use workhive_core::domain::UnitId;
use workhive_core::ports::{PaneRuntimePort, SetUnitTitleRequest};

use crate::context::CommandContext;
use crate::error::CliError;

/// Hard upper bound on total guard runtime, in milliseconds.
///
/// The guard is meant to defend against title pollution during the
/// first few seconds of a tool starting up. Anything past this is a
/// product/runtime hardening problem, not a polling problem — clamp
/// hard to keep this background process from leaking.
pub(crate) const PANE_TITLE_GUARD_MAX_DURATION_MS: u64 = 30_000;

/// Hard lower bound on the reassertion interval, in milliseconds.
///
/// Keeps the guard from busy-looping if a caller passes `--interval-ms 0`.
pub(crate) const PANE_TITLE_GUARD_MIN_INTERVAL_MS: u64 = 50;

/// One reassertion target parsed from the `--pane UNIT=TITLE` flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PaneTitleEntry {
    pub unit_id: UnitId,
    pub title: String,
}

/// Parse the repeated `--pane UNIT=TITLE` flag into structured entries.
///
/// Empty inputs return an empty Vec. A missing `=` is a parse error so
/// the caller (the spawning `workspace open` command) catches argv typos
/// at integration tests instead of in production logs.
pub(crate) fn parse_pane_title_args(raw: &[String]) -> Result<Vec<PaneTitleEntry>, CliError> {
    let mut out = Vec::with_capacity(raw.len());
    for entry in raw {
        let Some((unit, title)) = entry.split_once('=') else {
            return Err(CliError::Runtime(format!(
                "internal pane-title-guard: '--pane {entry}' is missing '=' separator"
            )));
        };
        let unit = unit.trim();
        if unit.is_empty() {
            return Err(CliError::Runtime(format!(
                "internal pane-title-guard: '--pane {entry}' has empty unit id"
            )));
        }
        out.push(PaneTitleEntry {
            unit_id: UnitId::new(unit.to_owned()),
            title: title.to_owned(),
        });
    }
    Ok(out)
}

/// Decide whether to keep ticking the reassertion loop.
///
/// Pure function so the bounded-runtime contract is unit-tested without
/// sleeping. `start` is the loop's first-tick instant; `max` is the
/// total budget; `now` is the current instant.
pub(crate) fn should_continue(start: Instant, max: Duration, now: Instant) -> bool {
    now.duration_since(start) < max
}

/// Compute the interval (in milliseconds) the loop should sleep,
/// clamped to the hard lower bound and the requested budget.
///
/// Pure: separated from the loop for testing and to make the clamping
/// behavior auditable.
pub(crate) fn schedule_tick(requested_ms: u64) -> u64 {
    requested_ms.max(PANE_TITLE_GUARD_MIN_INTERVAL_MS)
}

/// Clamp the requested total duration to the hard upper bound.
pub(crate) fn clamp_duration(requested_ms: u64) -> u64 {
    requested_ms.min(PANE_TITLE_GUARD_MAX_DURATION_MS)
}

/// `internal pane-title-guard` handler.
///
/// Runs entirely in the background after `workspace open` spawns it
/// detached. All output is `tracing`; nothing is written to stderr/stdout
/// because the parent's `attach` call has taken over the terminal.
pub(crate) fn cmd_internal_pane_title_guard(
    server: &str,
    session: &str,
    pane: Vec<String>,
    interval_ms: u64,
    duration_ms: u64,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let entries = parse_pane_title_args(&pane)?;
    if entries.is_empty() {
        tracing::debug!(
            server,
            session,
            "pane-title-guard: no entries to reassert; exiting immediately"
        );
        return Ok(());
    }

    let interval = Duration::from_millis(schedule_tick(interval_ms));
    let max = Duration::from_millis(clamp_duration(duration_ms));
    let start = Instant::now();

    tracing::debug!(
        server,
        session,
        count = entries.len(),
        interval_ms = interval.as_millis() as u64,
        duration_ms = max.as_millis() as u64,
        "pane-title-guard: starting bounded reassert loop"
    );

    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);

    loop {
        for entry in &entries {
            if let Err(e) = runtime.set_unit_title(SetUnitTitleRequest {
                group: server.to_owned(),
                target_unit: entry.unit_id.clone(),
                title: entry.title.clone(),
            }) {
                tracing::warn!(
                    server,
                    session,
                    unit_id = %entry.unit_id,
                    title = %entry.title,
                    error = %e,
                    "pane-title-guard: reassert failed; will retry next tick (or exit if pane gone)"
                );
            }
        }

        if !should_continue(start, max, Instant::now()) {
            break;
        }

        std::thread::sleep(interval);

        if !should_continue(start, max, Instant::now()) {
            break;
        }
    }

    tracing::debug!(
        server,
        session,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "pane-title-guard: bounded reassert loop finished"
    );

    Ok(())
}

/// Build the argv used to spawn the detached pane-title guard.
///
/// Returning an owned `Vec<String>` lets composition tests assert the
/// exact internal subcommand surface without spawning a child process.
/// Stable: changing this surface is a breaking change for any in-flight
/// guard processes a previous `workhive` build may have left running.
pub(crate) fn pane_title_guard_argv(
    server: &str,
    session: &str,
    entries: &[(UnitId, String)],
    interval_ms: u64,
    duration_ms: u64,
) -> Vec<String> {
    let mut argv = vec![
        "internal".to_owned(),
        "pane-title-guard".to_owned(),
        "--server".to_owned(),
        server.to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "--interval-ms".to_owned(),
        interval_ms.to_string(),
        "--duration-ms".to_owned(),
        duration_ms.to_string(),
    ];
    for (unit, title) in entries {
        argv.push("--pane".to_owned());
        argv.push(format!("{unit}={title}"));
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_simple_unit_title() {
        let parsed =
            parse_pane_title_args(&["abc=Codex".to_owned(), "def=Claude Code".to_owned()]).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].unit_id.as_str(), "abc");
        assert_eq!(parsed[0].title, "Codex");
        assert_eq!(parsed[1].title, "Claude Code");
    }

    #[test]
    fn parse_accepts_empty_title() {
        // Allowing empty titles lets callers explicitly clear pollution
        // without inventing a sentinel; the runtime still issues
        // `set_unit_title("")` which tmux accepts.
        let parsed = parse_pane_title_args(&["abc=".to_owned()]).unwrap();
        assert_eq!(parsed[0].title, "");
    }

    #[test]
    fn parse_rejects_missing_separator() {
        let err = parse_pane_title_args(&["bogus".to_owned()]).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("missing '=' separator"),
            "expected explicit parse error, got {msg}"
        );
    }

    #[test]
    fn parse_rejects_empty_unit_id() {
        let err = parse_pane_title_args(&["=Codex".to_owned()]).unwrap_err();
        let msg = format!("{err:?}");
        assert!(
            msg.contains("empty unit id"),
            "expected explicit parse error for empty unit id, got {msg}"
        );
    }

    #[test]
    fn schedule_tick_clamps_below_minimum() {
        assert_eq!(schedule_tick(0), PANE_TITLE_GUARD_MIN_INTERVAL_MS);
        assert_eq!(schedule_tick(10), PANE_TITLE_GUARD_MIN_INTERVAL_MS);
        assert_eq!(
            schedule_tick(PANE_TITLE_GUARD_MIN_INTERVAL_MS),
            PANE_TITLE_GUARD_MIN_INTERVAL_MS
        );
        assert_eq!(schedule_tick(750), 750);
    }

    #[test]
    fn clamp_duration_caps_overly_long_runs() {
        assert_eq!(
            clamp_duration(PANE_TITLE_GUARD_MAX_DURATION_MS + 1_000),
            PANE_TITLE_GUARD_MAX_DURATION_MS
        );
        assert_eq!(clamp_duration(5_000), 5_000);
        assert_eq!(clamp_duration(0), 0);
    }

    #[test]
    fn should_continue_stops_when_budget_elapsed() {
        let start = Instant::now();
        let max = Duration::from_millis(100);
        assert!(should_continue(start, max, start));
        assert!(should_continue(
            start,
            max,
            start + Duration::from_millis(50)
        ));
        assert!(!should_continue(
            start,
            max,
            start + Duration::from_millis(100)
        ));
        assert!(!should_continue(
            start,
            max,
            start + Duration::from_millis(500)
        ));
    }

    #[test]
    fn argv_includes_required_flags_and_repeats_pane_for_each_entry() {
        let argv = pane_title_guard_argv(
            "srv",
            "sess",
            &[
                (UnitId::new("u1".to_owned()), "Codex".to_owned()),
                (UnitId::new("u2".to_owned()), "Claude Code".to_owned()),
            ],
            500,
            8000,
        );
        let joined = argv.join(" ");
        assert!(argv.starts_with(&["internal".to_owned(), "pane-title-guard".to_owned()]));
        assert!(joined.contains("--server srv"));
        assert!(joined.contains("--session sess"));
        assert!(joined.contains("--interval-ms 500"));
        assert!(joined.contains("--duration-ms 8000"));
        assert!(joined.contains("--pane u1=Codex"));
        assert!(joined.contains("--pane u2=Claude Code"));
    }

    #[test]
    fn argv_omits_pane_flag_when_no_entries() {
        let argv = pane_title_guard_argv("srv", "sess", &[], 500, 8000);
        assert!(!argv.iter().any(|a| a == "--pane"));
    }

    /// Reassertion uses the launch-identity title supplied by the
    /// caller, not whatever the live `pane_title` may have drifted to.
    /// Callers MUST resolve titles from the launch baseline / pane
    /// registry — the guard process itself never reads `pane_title`.
    #[test]
    fn argv_titles_are_caller_supplied_not_introspected() {
        // Sentinel: title is "Codex" even though a polluting tool might
        // have set tmux's `pane_title` to "workhive". The argv builder
        // takes the caller's value verbatim.
        let argv = pane_title_guard_argv(
            "srv",
            "sess",
            &[(UnitId::new("u1".to_owned()), "Codex".to_owned())],
            500,
            8000,
        );
        assert!(argv.iter().any(|a| a == "u1=Codex"));
        assert!(!argv.iter().any(|a| a.contains("workhive")));
    }
}

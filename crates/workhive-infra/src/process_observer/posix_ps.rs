//! POSIX `ps`-based process observer (PID-first).
//!
//! ## Strategy
//!
//! The port contract pins the input as the shell PID Workhive owns
//! (tmux's `pane_pid`). The adapter walks the kernel-reported
//! controlling-terminal foreground process group from that PID:
//!
//! 1. Run `ps -p <shell_pids> -o pid=,tty=,ppid=,pgid=,tpgid=,comm=`
//!    once for every unique shell PID. This single invocation is the
//!    "first pass" — it tells us, for each shell PID, what tty it
//!    owns and which process group ID is currently foreground on
//!    that tty (`tpgid`).
//! 2. For shell PIDs whose `tpgid` points at some other process
//!    (i.e. a tool like `codex` is in front), collect those `tpgid`
//!    values that we do not yet have rows for and run a single
//!    second `ps -p <tpgids>` to fetch the foreground leader rows.
//!    For shells that ARE foreground (`tpgid == pgid == pid`) no
//!    second call is needed.
//! 3. Build a `pid -> row` map and, per lookup, surface the
//!    foreground row whose PID equals the shell's `tpgid`. The tty
//!    is used purely as a cross-check: if the resolved leader sits
//!    on a different tty than the shell, the result is dropped as
//!    untrustworthy and we fall through to tmux.
//!
//! ## Why PID-first and not tty-first
//!
//! tty paths are not stable identifiers — multiple shells routinely
//! share a controlling terminal in nested tmux/ssh setups, and tty
//! reuse after process exit is a real source of misidentification.
//! Using the shell PID is the same identity the supervisor and tmux
//! itself rely on, so all three layers of the merge policy stay
//! consistent.
//!
//! ## Why `ps` and not platform-specific syscalls
//!
//! * macOS does not expose `/proc`; `sysctl(KERN_PROC_PID)` exists
//!   but requires unsafe FFI and a per-platform binding.
//! * Linux `/proc/<pid>/stat` is parseable, but `ps` keeps the
//!   adapter uniform and matches the external-command policy ("`Command::arg()`
//!   directly … parse typed output").
//! * Tests inject a pluggable [`PsRunner`] so they run on any host.
//!
//! ## Batching contract
//!
//! `observe_many` issues at most TWO `ps` invocations regardless of
//! how many panes are passed in (one for shell PIDs, one optional
//! follow-up for foreground leaders). This holds the testing policy
//! perf bar that hot CLI paths like `workhive pane list` must not
//! pay O(panes) fork/exec cost.

use std::collections::{HashMap, HashSet};
use std::process::Command;

use workhive_core::domain::{PaneObservation, PaneObservedCommand, PaneObservedProcess};
use workhive_core::ports::{PaneProcessLookup, ProcessObserverError, ProcessObserverPort};

/// Test seam: run `ps -p <pids>` and return its stdout.
///
/// Production uses [`SystemPsRunner`]; tests inject canned strings.
pub trait PsRunner: Send + Sync {
    /// Invoke `ps -p <pids> -o pid=,tty=,ppid=,pgid=,tpgid=,comm=`
    /// and return the raw stdout.
    ///
    /// `pids` is one or more decimal PIDs. The adapter deduplicates
    /// before calling `run` so the production runner can pass them
    /// straight to `ps -p`. Implementations MUST NOT mutate the
    /// argument order — callers rely on it for assertion purposes
    /// in tests.
    fn run(&self, pids: &[u32]) -> Result<String, ProcessObserverError>;
}

/// `ps`-based observer wired to whatever [`PsRunner`] the caller injects.
pub struct PsUnixProcessObserver<R: PsRunner = SystemPsRunner> {
    runner: R,
}

impl PsUnixProcessObserver<SystemPsRunner> {
    /// Build an observer that shells out to the system `ps`.
    pub fn new() -> Self {
        Self {
            runner: SystemPsRunner,
        }
    }
}

impl Default for PsUnixProcessObserver<SystemPsRunner> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R: PsRunner> PsUnixProcessObserver<R> {
    /// Construct an observer with a custom runner (for tests).
    pub fn with_runner(runner: R) -> Self {
        Self { runner }
    }
}

impl<R: PsRunner> ProcessObserverPort for PsUnixProcessObserver<R> {
    fn observe(
        &self,
        lookup: &PaneProcessLookup,
    ) -> Result<Option<PaneObservation>, ProcessObserverError> {
        let observations = self.observe_many(std::slice::from_ref(lookup))?;
        Ok(observations.into_iter().next().unwrap_or(None))
    }

    fn observe_many(
        &self,
        lookups: &[PaneProcessLookup],
    ) -> Result<Vec<Option<PaneObservation>>, ProcessObserverError> {
        if lookups.is_empty() {
            return Ok(Vec::new());
        }

        // Collect unique shell PIDs to query ps for. Lookups missing a
        // shell_pid are placeholders and resolve to None at the end.
        let mut unique_shell_pids: Vec<u32> = Vec::new();
        for lookup in lookups {
            if let Some(pid) = lookup.shell_pid
                && !unique_shell_pids.contains(&pid)
            {
                unique_shell_pids.push(pid);
            }
        }

        if unique_shell_pids.is_empty() {
            return Ok(vec![None; lookups.len()]);
        }

        // First pass: query ps for the shell PIDs.
        let stdout = self.runner.run(&unique_shell_pids)?;
        let mut all_rows = parse_ps_rows(&stdout)?;

        // Identify foreground leader PIDs we still need rows for.
        let known_pids: HashSet<u32> = all_rows.iter().map(|r| r.pid).collect();
        let mut needed_tpgids: Vec<u32> = Vec::new();
        for row in &all_rows {
            if row.tpgid <= 0 {
                continue;
            }
            let tpgid = row.tpgid as u32;
            if !known_pids.contains(&tpgid) && !needed_tpgids.contains(&tpgid) {
                needed_tpgids.push(tpgid);
            }
        }

        // Second (optional) pass: query ps for the foreground leaders.
        if !needed_tpgids.is_empty() {
            let stdout2 = self.runner.run(&needed_tpgids)?;
            let rows2 = parse_ps_rows(&stdout2)?;
            all_rows.extend(rows2);
        }

        // Index all rows by PID for the per-lookup foreground walk.
        let by_pid: HashMap<u32, &PsRow> = all_rows.iter().map(|r| (r.pid, r)).collect();

        let mut out = Vec::with_capacity(lookups.len());
        for lookup in lookups {
            let observation = lookup.shell_pid.and_then(|pid| {
                by_pid
                    .get(&pid)
                    .and_then(|shell_row| build_observation_from_shell(shell_row, &by_pid, lookup))
            });
            out.push(observation);
        }
        Ok(out)
    }
}

/// Strip the `/dev/` prefix from a tty path so it matches `ps`-style
/// short tty names (e.g. `ttys003`) and tmux-reported full paths
/// (e.g. `/dev/ttys003`) interchangeably.
fn strip_dev_prefix(tty: &str) -> &str {
    tty.strip_prefix("/dev/").unwrap_or(tty)
}

/// One parsed `ps -p <pids> -o pid=,tty=,ppid=,pgid=,tpgid=,comm=` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PsRow {
    pub pid: u32,
    pub tty: String,
    pub ppid: u32,
    pub pgid: u32,
    pub tpgid: i32,
    pub comm: String,
}

/// Parse the stdout of `ps -p <pids> -o pid=,tty=,ppid=,pgid=,tpgid=,comm=`.
///
/// Each row has the shape `pid tty ppid pgid tpgid comm` separated by
/// whitespace. `comm` may contain spaces on macOS but not on Linux;
/// we accept everything after the five leading columns as the
/// command basename.
///
/// `tty` is normalized via [`strip_dev_prefix`] so callers can compare
/// it against the tmux-reported `/dev/ttys003` shape uniformly. A
/// literal `?` (some `ps` implementations report no controlling
/// terminal as `?`) is stored as the empty string.
pub(super) fn parse_ps_rows(stdout: &str) -> Result<Vec<PsRow>, ProcessObserverError> {
    let mut rows = Vec::new();
    for (line_no, raw) in stdout.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut iter = line.split_whitespace();
        let pid = parse_u32_field(iter.next(), line_no, "pid")?;
        let tty_raw = iter.next().ok_or_else(|| ProcessObserverError::Parse {
            detail: format!("line {line_no}: missing tty"),
        })?;
        let tty = if tty_raw == "?" {
            String::new()
        } else {
            strip_dev_prefix(tty_raw).to_owned()
        };
        let ppid = parse_u32_field(iter.next(), line_no, "ppid")?;
        let pgid = parse_u32_field(iter.next(), line_no, "pgid")?;
        let tpgid = parse_i32_field(iter.next(), line_no, "tpgid")?;
        let comm: String = iter.collect::<Vec<&str>>().join(" ");
        let comm = comm.trim().to_owned();
        if comm.is_empty() {
            // Without a command name we cannot classify; skip the row
            // rather than fabricate an empty observation.
            continue;
        }
        rows.push(PsRow {
            pid,
            tty,
            ppid,
            pgid,
            tpgid,
            comm,
        });
    }
    Ok(rows)
}

fn parse_u32_field(
    field: Option<&str>,
    line_no: usize,
    name: &'static str,
) -> Result<u32, ProcessObserverError> {
    let raw = field.ok_or_else(|| ProcessObserverError::Parse {
        detail: format!("line {line_no}: missing {name}"),
    })?;
    raw.trim()
        .parse::<u32>()
        .map_err(|err| ProcessObserverError::Parse {
            detail: format!("line {line_no}: bad {name} '{raw}': {err}"),
        })
}

fn parse_i32_field(
    field: Option<&str>,
    line_no: usize,
    name: &'static str,
) -> Result<i32, ProcessObserverError> {
    let raw = field.ok_or_else(|| ProcessObserverError::Parse {
        detail: format!("line {line_no}: missing {name}"),
    })?;
    raw.trim()
        .parse::<i32>()
        .map_err(|err| ProcessObserverError::Parse {
            detail: format!("line {line_no}: bad {name} '{raw}': {err}"),
        })
}

/// Walk from the shell row to the foreground process leader and
/// assemble a [`PaneObservation`].
///
/// The merge policy expects `Ok(None)` when the OS could not produce
/// a trustworthy answer, so we deliberately drop a few corner cases:
///
/// * `tpgid <= 0` — the shell's controlling terminal currently has no
///   foreground process group (the shell may already be backgrounded
///   or detached).
/// * The foreground leader (the row whose `pid == shell.tpgid`) is
///   missing from our rows. This is rare but possible if the leader
///   exited between the two `ps` calls.
/// * The foreground leader sits on a different tty than the shell.
///   This violates the kernel's controlling-terminal invariant; we
///   refuse to fabricate a cross-tty observation.
/// * The lookup carried a `tty` hint and it disagrees with the
///   shell's reported tty. Treated as untrustworthy: tty hints are
///   advisory but disagreement with the kernel signal is a strong
///   signal of stale tmux state, and silently overriding it would
///   defeat the whole point of PID-first observation.
pub(super) fn build_observation_from_shell(
    shell: &PsRow,
    by_pid: &HashMap<u32, &PsRow>,
    lookup: &PaneProcessLookup,
) -> Option<PaneObservation> {
    if shell.tpgid <= 0 {
        return None;
    }

    if let Some(hint) = lookup
        .tty
        .as_deref()
        .map(strip_dev_prefix)
        .filter(|t| !t.is_empty())
        && !shell.tty.is_empty()
        && hint != shell.tty
    {
        tracing::debug!(
            shell_pid = shell.pid,
            shell_tty = %shell.tty,
            hint_tty = %hint,
            "process_observer: tty hint disagrees with kernel; surfacing None"
        );
        return None;
    }

    let tpgid = shell.tpgid as u32;

    // Foreground leader lives in the same process group as `tpgid`.
    // Prefer the row whose pid == tpgid (the actual group leader).
    let leader = if shell.pid == tpgid {
        shell
    } else {
        match by_pid.get(&tpgid).copied() {
            Some(row) => row,
            None => {
                tracing::debug!(
                    shell_pid = shell.pid,
                    tpgid,
                    "process_observer: foreground leader row missing; surfacing None"
                );
                return None;
            }
        }
    };

    if !leader.tty.is_empty() && !shell.tty.is_empty() && leader.tty != shell.tty {
        tracing::debug!(
            shell_tty = %shell.tty,
            leader_tty = %leader.tty,
            "process_observer: foreground leader on different tty; surfacing None"
        );
        return None;
    }

    let process = PaneObservedProcess {
        foreground: Some(row_to_observed_command(leader)),
        process_tree: vec![row_to_observed_command(leader)],
    };
    Some(PaneObservation::process_observer(process))
}

fn row_to_observed_command(row: &PsRow) -> PaneObservedCommand {
    PaneObservedCommand {
        basename: row.comm.clone(),
        program_path: None,
        pid: Some(row.pid),
    }
}

/// Production [`PsRunner`] that shells out to the system `ps` binary.
pub struct SystemPsRunner;

impl PsRunner for SystemPsRunner {
    fn run(&self, pids: &[u32]) -> Result<String, ProcessObserverError> {
        if pids.is_empty() {
            return Ok(String::new());
        }
        let joined = pids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let output = Command::new("ps")
            .arg("-p")
            .arg(&joined)
            .arg("-o")
            .arg("pid=,tty=,ppid=,pgid=,tpgid=,comm=")
            .output()
            .map_err(|err| ProcessObserverError::Io {
                detail: format!("failed to spawn ps: {err}"),
            })?;
        if !output.status.success() {
            // `ps -p` returns non-zero when ALL the requested pids
            // are gone (e.g. the shell already exited). Surface that
            // truthfully so the merge policy can fall through to
            // tmux fallback rather than fabricating a stale answer.
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(ProcessObserverError::Io {
                detail: format!("ps -p {joined} failed: {}", stderr.trim()),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

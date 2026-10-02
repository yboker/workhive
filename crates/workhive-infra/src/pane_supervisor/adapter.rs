//! `JsonFilePaneSupervisor` — implements [`PaneSupervisorPort`]
//! against the JSON metadata store.
//!
//! The adapter combines two signals:
//!
//! 1. metadata read from disk via [`super::store`]
//! 2. live pid liveness probe via [`is_pid_alive`]
//!
//! Truth table:
//!
//! | metadata state | pid alive | observation                              |
//! | -------------- | --------- | ---------------------------------------- |
//! | `Running`      | yes       | `Authoritative` running with foreground  |
//! | `Running`      | no        | `Fallback` exited (stale metadata) — the |
//! |                |           | merge policy can still display the cmd   |
//! |                |           | basename but no longer at authoritative  |
//! |                |           | confidence                               |
//! | `Exited`       | n/a       | `Ok(None)` — historical audit only, the  |
//! |                |           | tool is no longer the pane's foreground  |
//! |                |           | so the supervisor must not pretend to    |
//! |                |           | own current truth (the user dropped back |
//! |                |           | to the shell). The merge policy then     |
//! |                |           | falls through to OS observer / tmux      |
//! |                |           | fallback so `pane list` shows the real   |
//! |                |           | shell instead of the historical tool.    |
//! | not present    | n/a       | `Ok(None)` — no supervisor signal        |
//!
//! Failures from the store surface as
//! [`PaneSupervisorError::Io`]/`Corrupt`. The adapter never coerces a
//! failure into `Ok(None)` silently per the error-handling policy.

use camino::Utf8PathBuf;
use workhive_core::domain::{
    PaneObservation, PaneObservationConfidence, PaneObservationSource, PaneObservedCommand,
    PaneObservedProcess, PaneState, UnitId,
};
use workhive_core::ports::{PaneSupervisorError, PaneSupervisorPort};

use super::store::{
    PaneSupervisorMetadata, PaneSupervisorStateOnDisk, PaneSupervisorStoreError, read_metadata,
};

/// Trait for probing pid liveness. Production wires up
/// [`NativeUnixPidProbe`]; tests pass a fake.
pub trait PidLivenessProbe: Send + Sync {
    fn is_alive(&self, pid: u32) -> bool;
}

/// Native pid liveness probe using `kill(pid, 0)` semantics directly
/// (no subprocess). On Unix this is implemented via `rustix::process::
/// test_kill_process`, which makes a single syscall. Calling this on
/// every pane during `pane list` is therefore O(panes) syscalls rather
/// than O(panes) `fork`+`exec` of `/bin/kill`. See the external-command policy —
/// adapter implementations parse typed results and avoid spawning
/// shells when an in-process API exists.
pub struct NativeUnixPidProbe;

impl PidLivenessProbe for NativeUnixPidProbe {
    fn is_alive(&self, pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        is_pid_alive_native(pid)
    }
}

#[cfg(unix)]
fn is_pid_alive_native(pid: u32) -> bool {
    use rustix::process::{Pid, test_kill_process};
    let raw = match i32::try_from(pid) {
        Ok(value) if value > 0 => value,
        _ => return false,
    };
    let Some(pid_handle) = Pid::from_raw(raw) else {
        return false;
    };
    match test_kill_process(pid_handle) {
        Ok(()) => true,
        Err(errno) => {
            // ESRCH ("No such process") is the explicit "dead" signal.
            // EPERM ("Operation not permitted") means the pid exists
            // but we cannot signal it; from the pane supervisor's
            // perspective the supervised child is still alive.
            const EPERM: i32 = rustix::io::Errno::PERM.raw_os_error();
            if errno.raw_os_error() == EPERM {
                true
            } else {
                tracing::debug!(
                    pid = pid,
                    errno = errno.raw_os_error(),
                    "pane_supervisor: native pid liveness probe reports dead"
                );
                false
            }
        }
    }
}

#[cfg(not(unix))]
fn is_pid_alive_native(_pid: u32) -> bool {
    // Workhive currently targets macOS primary / Linux secondary.
    // On non-Unix hosts we conservatively treat pids as dead so the
    // merge policy will fall through to the next observation source.
    false
}

/// Adapter implementing [`PaneSupervisorPort`] against a
/// project-scoped runtime root.
pub struct JsonFilePaneSupervisor<P: PidLivenessProbe = NativeUnixPidProbe> {
    runtime_root: Utf8PathBuf,
    probe: P,
}

impl JsonFilePaneSupervisor<NativeUnixPidProbe> {
    /// Build an adapter using the system pid liveness probe.
    pub fn new(runtime_root: Utf8PathBuf) -> Self {
        Self {
            runtime_root,
            probe: NativeUnixPidProbe,
        }
    }
}

impl<P: PidLivenessProbe> JsonFilePaneSupervisor<P> {
    /// Build an adapter with an injected probe (for tests).
    pub fn with_probe(runtime_root: Utf8PathBuf, probe: P) -> Self {
        Self {
            runtime_root,
            probe,
        }
    }
}

impl<P: PidLivenessProbe> PaneSupervisorPort for JsonFilePaneSupervisor<P> {
    fn observe(
        &self,
        group: &str,
        workspace: &str,
        unit_id: &UnitId,
    ) -> Result<Option<PaneObservation>, PaneSupervisorError> {
        let metadata = match read_metadata(&self.runtime_root, group, workspace, unit_id) {
            Ok(value) => value,
            Err(err) => return Err(translate_store_error(err)),
        };
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        Ok(observation_from_metadata(metadata, &self.probe))
    }
}

fn translate_store_error(err: PaneSupervisorStoreError) -> PaneSupervisorError {
    match err {
        PaneSupervisorStoreError::Io { path, detail } => PaneSupervisorError::Io {
            detail: format!("{path}: {detail}"),
        },
        PaneSupervisorStoreError::InvalidPath { detail } => PaneSupervisorError::Io { detail },
        PaneSupervisorStoreError::Corrupt { path, detail } => PaneSupervisorError::Corrupt {
            detail: format!("{path}: {detail}"),
        },
    }
}

/// Translate disk metadata + pid liveness into a `PaneObservation`.
///
/// Returns `None` when the supervisor metadata describes a historical
/// run that cannot speak for the pane's current foreground process —
/// today that means a child the supervisor already saw exit (e.g. the
/// user pressed `Ctrl+C` inside `claude` and dropped back to the
/// shell). The persisted `Exited` record is still kept on disk for
/// audit, but it is intentionally NOT surfaced to the merge policy as
/// a current truth, because doing so would mask the real shell with a
/// stale tool identity at `Authoritative` confidence (the merge
/// policy picks max confidence, so `Authoritative` always wins over
/// `High` OS observer / `Fallback` tmux signals).
///
/// `pub(super)` so the test module in `mod.rs` can exercise the
/// translation directly without faking the file system.
pub(super) fn observation_from_metadata(
    metadata: PaneSupervisorMetadata,
    probe: &dyn PidLivenessProbe,
) -> Option<PaneObservation> {
    let basename = metadata.command_basename.clone();
    let foreground = PaneObservedCommand {
        basename: basename.clone(),
        program_path: metadata.program_path.clone(),
        pid: Some(metadata.pid),
    };
    let process = PaneObservedProcess::from_foreground(foreground);

    match metadata.state {
        PaneSupervisorStateOnDisk::Running if probe.is_alive(metadata.pid) => {
            Some(PaneObservation {
                source: PaneObservationSource::Supervisor,
                confidence: PaneObservationConfidence::Authoritative,
                process: Some(process),
                state_hint: Some(PaneState::Running),
            })
        }
        PaneSupervisorStateOnDisk::Running => {
            // Stale metadata. Adapter intentionally downgrades to
            // Fallback so the merge policy lets a higher-confidence
            // OS observer (or tmux fallback) override the basename
            // and state. We still surface the basename so callers can
            // log "supervisor recorded this; pid is gone".
            tracing::debug!(
                pid = metadata.pid,
                command_basename = %basename,
                "pane_supervisor: pid no longer alive; downgrading observation"
            );
            Some(PaneObservation {
                source: PaneObservationSource::Supervisor,
                confidence: PaneObservationConfidence::Fallback,
                process: Some(process),
                state_hint: Some(PaneState::Idle),
            })
        }
        PaneSupervisorStateOnDisk::Exited => {
            // Historical audit only. The supervisor saw the wrapped
            // tool exit; the pane has now returned to a shell prompt
            // (or any other non-supervised process). Returning `None`
            // forces the merge policy to consult the OS observer and
            // tmux fallback so `pane list` / `pane show` display the
            // pane's real current foreground, never the historical
            // tool basename.
            tracing::debug!(
                pid = metadata.pid,
                command_basename = %basename,
                exit_code = ?metadata.exit_code,
                "pane_supervisor: exited metadata is audit only; not surfacing as current truth"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane_supervisor::store::write_metadata;
    use camino::Utf8PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::TempDir;

    struct StaticProbe(Arc<AtomicBool>);

    impl PidLivenessProbe for StaticProbe {
        fn is_alive(&self, _pid: u32) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn fixture(state: PaneSupervisorStateOnDisk, pid: u32) -> PaneSupervisorMetadata {
        PaneSupervisorMetadata {
            pid,
            command_basename: "claude".to_owned(),
            program_path: Some("/usr/local/bin/claude".to_owned()),
            started_at_rfc3339: "2026-05-07T12:00:00Z".to_owned(),
            last_heartbeat_rfc3339: None,
            state,
            exit_code: None,
        }
    }

    fn tmp_root() -> (TempDir, Utf8PathBuf) {
        let tmp = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).unwrap();
        (tmp, root)
    }

    #[test]
    fn observe_returns_none_when_no_metadata() {
        let (_t, root) = tmp_root();
        let probe = StaticProbe(Arc::new(AtomicBool::new(true)));
        let adapter = JsonFilePaneSupervisor::with_probe(root, probe);
        let result = adapter.observe("g", "ws", &UnitId::new("%2")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn live_running_metadata_yields_authoritative_running_observation() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%5");
        write_metadata(
            &root,
            "g",
            "ws",
            &unit,
            &fixture(PaneSupervisorStateOnDisk::Running, 4242),
        )
        .unwrap();
        let probe = StaticProbe(Arc::new(AtomicBool::new(true)));
        let adapter = JsonFilePaneSupervisor::with_probe(root, probe);
        let obs = adapter.observe("g", "ws", &unit).unwrap().unwrap();
        assert_eq!(obs.source, PaneObservationSource::Supervisor);
        assert_eq!(obs.confidence, PaneObservationConfidence::Authoritative);
        assert_eq!(obs.state_hint, Some(PaneState::Running));
        assert_eq!(obs.process.unwrap().foreground.unwrap().basename, "claude");
    }

    #[test]
    fn stale_running_metadata_is_downgraded_to_fallback() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%6");
        write_metadata(
            &root,
            "g",
            "ws",
            &unit,
            &fixture(PaneSupervisorStateOnDisk::Running, 9999),
        )
        .unwrap();
        let probe = StaticProbe(Arc::new(AtomicBool::new(false)));
        let adapter = JsonFilePaneSupervisor::with_probe(root, probe);
        let obs = adapter.observe("g", "ws", &unit).unwrap().unwrap();
        // Adapter must NOT keep the supervisor at Authoritative when
        // pid is gone; merge policy then falls through to OS observer.
        assert_eq!(obs.confidence, PaneObservationConfidence::Fallback);
        assert_eq!(obs.state_hint, Some(PaneState::Idle));
    }

    /// Once the supervised tool has exited, the persisted `Exited`
    /// metadata is purely a historical audit record. The adapter MUST
    /// stop surfacing it as the pane's current truth — otherwise the
    /// merge policy (max-confidence wins) would keep showing the
    /// historical tool basename at `Authoritative` confidence, masking
    /// the real shell that took over the pane after the user exited
    /// the tool.
    #[test]
    fn exited_state_returns_none_so_merge_falls_through_to_real_shell() {
        let (_t, root) = tmp_root();
        let unit = UnitId::new("%7");
        let mut meta = fixture(PaneSupervisorStateOnDisk::Exited, 1234);
        meta.exit_code = Some(0);
        write_metadata(&root, "g", "ws", &unit, &meta).unwrap();
        let probe = StaticProbe(Arc::new(AtomicBool::new(false)));
        let adapter = JsonFilePaneSupervisor::with_probe(root, probe);
        let result = adapter.observe("g", "ws", &unit).unwrap();
        assert!(
            result.is_none(),
            "Exited supervisor metadata must not be reported as a current observation: {result:?}"
        );
    }
}

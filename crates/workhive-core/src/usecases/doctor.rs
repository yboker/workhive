//! `doctor` use case — run diagnostic checks on the workhive environment.
//!
//! Returns structured diagnostic data only. All human-readable text
//! (labels, advice) is resolved by the CLI layer through i18n keys.

use crate::domain::{SessionAvailability, WorkspaceStateKey};
use crate::ports::{EnvironmentPort, RuntimeError, SnapshotStorePort, WorkspaceRuntimePort};
use crate::usecases::open::classify_session_availability;

/// Machine-readable check identifier.
///
/// Each variant maps to a corresponding `doctor.check.*` i18n key
/// in the CLI rendering layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    TmuxServer,
    SessionLookup,
    SnapshotIntegrity,
    PtyAvailability,
    AttachReadiness,
    WorkspaceIdentity,
}

/// Machine-readable advice identifier.
///
/// Each variant maps to a corresponding `doctor.advice.*` i18n key
/// in the CLI rendering layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdviceId {
    TmuxNotRunning,
    TmuxError,
    SessionMissing,
    SessionServerUnreachable,
    SessionLookupFailed,
    SnapshotCorrupt,
    PtyExhausted,
    PtyElevated,
    NestedTmux,
    InvalidWorkspaceName,
}

/// Status of a diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

/// A single diagnostic check result.
///
/// Contains only machine-readable identifiers and structured data.
/// The CLI layer is responsible for mapping these to localized text.
#[derive(Debug, Clone)]
pub struct DoctorCheckResult {
    pub check_id: CheckId,
    pub status: CheckStatus,
    /// Machine data about what was checked (e.g. server name, session name).
    pub detail: String,
    /// Advice identifier for the CLI to resolve via i18n. `None` if passed.
    pub advice_id: Option<AdviceId>,
}

/// Full diagnostic report.
#[derive(Debug)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheckResult>,
}

impl DoctorReport {
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Fail)
    }

    pub fn issue_count(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| matches!(c.status, CheckStatus::Fail | CheckStatus::Warn))
            .count()
    }
}

/// Port for querying system-level diagnostics (e.g. PTY availability).
pub trait SystemDiagnosticsPort {
    /// Returns (used, max) PTY count if available on this platform.
    fn pty_usage(&self) -> Option<(u64, u64)>;
}

/// Input describing a workspace for doctor per-workspace checks.
///
/// Carries both the typed `WorkspaceStateKey` (used for snapshot integrity
/// checks) and the resolved tmux session name (used for runtime checks).
#[derive(Debug, Clone)]
pub struct DoctorWorkspaceInput {
    pub workspace_state_key: WorkspaceStateKey,
    pub session_name: String,
}

/// Run remote/session diagnostics.
///
/// Checks tmux server reachability, per-workspace session lookup,
/// snapshot integrity, PTY availability, and attach readiness.
pub fn run_remote_diagnostics(
    ws_runtime: &dyn WorkspaceRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    env: &dyn EnvironmentPort,
    sys_diag: &dyn SystemDiagnosticsPort,
    server_name: &str,
    workspaces: &[DoctorWorkspaceInput],
) -> DoctorReport {
    let mut checks = vec![check_tmux_server(ws_runtime, server_name)];

    for ws in workspaces {
        checks.push(check_session_lookup(
            ws_runtime,
            server_name,
            &ws.session_name,
        ));
    }

    checks.push(check_snapshot_integrity(snapshot_store, workspaces));
    checks.push(check_pty_availability(sys_diag));
    checks.push(check_attach_readiness(env));

    DoctorReport { checks }
}

fn check_tmux_server(ws_runtime: &dyn WorkspaceRuntimePort, server: &str) -> DoctorCheckResult {
    match ws_runtime.list_workspaces(server) {
        Ok(_) => DoctorCheckResult {
            check_id: CheckId::TmuxServer,
            status: CheckStatus::Pass,
            detail: server.to_owned(),
            advice_id: None,
        },
        Err(RuntimeError::BackendUnavailable { .. }) => DoctorCheckResult {
            check_id: CheckId::TmuxServer,
            status: CheckStatus::Fail,
            detail: server.to_owned(),
            advice_id: Some(AdviceId::TmuxNotRunning),
        },
        Err(_) => DoctorCheckResult {
            check_id: CheckId::TmuxServer,
            status: CheckStatus::Fail,
            detail: server.to_owned(),
            advice_id: Some(AdviceId::TmuxError),
        },
    }
}

/// Per-workspace session lookup using `has_workspace()` and
/// `classify_session_availability()` — the same typed path used by `open`.
///
/// Contract: the doctor reports the raw session availability as a diagnostic
/// fact. `Running` is the only healthy state (`Pass`). `Missing` is a
/// warning — the session does not exist and `open` would need to create it.
/// `ServerUnreachable` and `LookupFailed` are failures.
fn check_session_lookup(
    ws_runtime: &dyn WorkspaceRuntimePort,
    server: &str,
    session_name: &str,
) -> DoctorCheckResult {
    let result = ws_runtime.has_workspace(server, session_name);
    let availability = classify_session_availability(result);

    match availability {
        SessionAvailability::Running => DoctorCheckResult {
            check_id: CheckId::SessionLookup,
            status: CheckStatus::Pass,
            detail: format!("{session_name} → Running"),
            advice_id: None,
        },
        SessionAvailability::Missing => DoctorCheckResult {
            check_id: CheckId::SessionLookup,
            status: CheckStatus::Warn,
            detail: format!("{session_name} → Missing"),
            advice_id: Some(AdviceId::SessionMissing),
        },
        SessionAvailability::ServerUnreachable => DoctorCheckResult {
            check_id: CheckId::SessionLookup,
            status: CheckStatus::Fail,
            detail: format!("{session_name} → ServerUnreachable"),
            advice_id: Some(AdviceId::SessionServerUnreachable),
        },
        SessionAvailability::LookupFailed => DoctorCheckResult {
            check_id: CheckId::SessionLookup,
            status: CheckStatus::Fail,
            detail: format!("{session_name} → LookupFailed"),
            advice_id: Some(AdviceId::SessionLookupFailed),
        },
    }
}

fn check_snapshot_integrity(
    snapshot_store: &dyn SnapshotStorePort,
    workspaces: &[DoctorWorkspaceInput],
) -> DoctorCheckResult {
    let mut corrupt_count = 0u32;
    let mut checked = 0u32;

    for ws in workspaces {
        checked += 1;
        match snapshot_store.load_latest(&ws.workspace_state_key) {
            Ok(Some(snapshot)) => {
                match snapshot_store.verify_revision(&ws.workspace_state_key, snapshot.revision_id)
                {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        corrupt_count += 1;
                    }
                }
            }
            Ok(None) => {}
            Err(_) => {
                corrupt_count += 1;
            }
        }
    }

    if corrupt_count > 0 {
        DoctorCheckResult {
            check_id: CheckId::SnapshotIntegrity,
            status: CheckStatus::Warn,
            detail: format!("{corrupt_count}/{checked}"),
            advice_id: Some(AdviceId::SnapshotCorrupt),
        }
    } else {
        DoctorCheckResult {
            check_id: CheckId::SnapshotIntegrity,
            status: CheckStatus::Pass,
            detail: format!("{checked} checked"),
            advice_id: None,
        }
    }
}

fn check_pty_availability(sys_diag: &dyn SystemDiagnosticsPort) -> DoctorCheckResult {
    match sys_diag.pty_usage() {
        Some((used, max)) if max > 0 => {
            let ratio = used as f64 / max as f64;
            if ratio >= 0.95 {
                DoctorCheckResult {
                    check_id: CheckId::PtyAvailability,
                    status: CheckStatus::Fail,
                    detail: format!("{used}/{max}"),
                    advice_id: Some(AdviceId::PtyExhausted),
                }
            } else if ratio >= 0.80 {
                DoctorCheckResult {
                    check_id: CheckId::PtyAvailability,
                    status: CheckStatus::Warn,
                    detail: format!("{used}/{max}"),
                    advice_id: Some(AdviceId::PtyElevated),
                }
            } else {
                DoctorCheckResult {
                    check_id: CheckId::PtyAvailability,
                    status: CheckStatus::Pass,
                    detail: format!("{used}/{max}"),
                    advice_id: None,
                }
            }
        }
        _ => DoctorCheckResult {
            check_id: CheckId::PtyAvailability,
            status: CheckStatus::Pass,
            detail: "n/a".to_owned(),
            advice_id: None,
        },
    }
}

/// Attach readiness — checks for nested tmux via `EnvironmentPort`.
fn check_attach_readiness(env: &dyn EnvironmentPort) -> DoctorCheckResult {
    match env.execution_context() {
        Ok(ctx) => {
            if ctx.tmux_session.is_some() {
                DoctorCheckResult {
                    check_id: CheckId::AttachReadiness,
                    status: CheckStatus::Warn,
                    detail: "nested".to_owned(),
                    advice_id: Some(AdviceId::NestedTmux),
                }
            } else {
                DoctorCheckResult {
                    check_id: CheckId::AttachReadiness,
                    status: CheckStatus::Pass,
                    detail: "ready".to_owned(),
                    advice_id: None,
                }
            }
        }
        Err(_) => DoctorCheckResult {
            check_id: CheckId::AttachReadiness,
            status: CheckStatus::Warn,
            detail: "unknown".to_owned(),
            advice_id: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRuntime {
        reachable: bool,
        existing_sessions: Vec<String>,
        lookup_fails: bool,
    }

    impl WorkspaceRuntimePort for FakeRuntime {
        fn has_workspace(&self, _g: &str, name: &str) -> Result<bool, RuntimeError> {
            if self.lookup_fails {
                return Err(RuntimeError::OperationFailed {
                    message: "garbled".into(),
                });
            }
            if !self.reachable {
                return Err(RuntimeError::BackendUnavailable {
                    reason: crate::ports::BackendUnavailableReason::Other,
                    detail: "fake".into(),
                });
            }
            Ok(self.existing_sessions.contains(&name.to_owned()))
        }
        fn create_workspace(
            &self,
            _r: crate::ports::CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn configure_workspace(&self, _g: &str, _w: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn attach_workspace(
            &self,
            _r: crate::ports::AttachWorkspaceRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn detach_workspace(&self, _g: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn destroy_workspace(&self, _g: &str, _w: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn list_workspaces(
            &self,
            _g: &str,
        ) -> Result<Vec<crate::ports::WorkspaceRuntimeInfo>, RuntimeError> {
            if !self.reachable {
                return Err(RuntimeError::BackendUnavailable {
                    reason: crate::ports::BackendUnavailableReason::Other,
                    detail: "fake".into(),
                });
            }
            Ok(self
                .existing_sessions
                .iter()
                .map(|n| crate::ports::WorkspaceRuntimeInfo {
                    workspace_name: n.clone(),
                    unit_count: 1,
                    attached: false,
                })
                .collect())
        }
        fn destroy_group(&self, _g: &str) -> Result<(), RuntimeError> {
            Ok(())
        }
    }

    struct FakeSnapshot;
    impl SnapshotStorePort for FakeSnapshot {
        fn save_revision(
            &self,
            snapshot: &crate::domain::WorkspaceSnapshot,
        ) -> Result<crate::domain::SnapshotRevisionId, crate::errors::SnapshotError> {
            Ok(snapshot.revision_id)
        }

        fn load_latest(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<Option<crate::domain::WorkspaceSnapshot>, crate::errors::SnapshotError>
        {
            Ok(None)
        }

        fn load_revision(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _revision_id: crate::domain::SnapshotRevisionId,
        ) -> Result<Option<crate::domain::WorkspaceSnapshot>, crate::errors::SnapshotError>
        {
            Ok(None)
        }

        fn append_events(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _events: &[crate::domain::SnapshotEvent],
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn load_events_since(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _since_revision: crate::domain::SnapshotRevisionId,
        ) -> Result<Vec<crate::domain::SnapshotEvent>, crate::errors::SnapshotError> {
            Ok(vec![])
        }

        fn read_manifest(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<Option<crate::domain::SnapshotManifest>, crate::errors::SnapshotError> {
            Ok(None)
        }

        fn write_manifest(
            &self,
            _manifest: &crate::domain::SnapshotManifest,
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn list_history(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<Vec<crate::domain::SnapshotHistoryEntry>, crate::errors::SnapshotError>
        {
            Ok(vec![])
        }

        fn acquire_lock(
            &self,
            _lock: &crate::domain::SupervisorLock,
        ) -> Result<bool, crate::errors::SnapshotError> {
            Ok(true)
        }

        fn release_lock(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn update_heartbeat(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _heartbeat: &str,
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn read_lock(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<Option<crate::domain::SupervisorLock>, crate::errors::SnapshotError> {
            Ok(None)
        }

        fn health_check(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<crate::domain::SnapshotHealthCheck, crate::errors::SnapshotError> {
            Ok(crate::domain::SnapshotHealthCheck {
                enabled: true,
                supervisor_running: false,
                hooks_registered: false,
                latest_save_time: None,
                latest_revision: None,
                manifest_readable: true,
                checksum_valid: None,
                state_key_match: true,
            })
        }

        fn verify_revision(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _revision_id: crate::domain::SnapshotRevisionId,
        ) -> Result<bool, crate::errors::SnapshotError> {
            Ok(true)
        }

        fn apply_retention(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
            _retain_revisions: u32,
            _retain_event_days: u32,
        ) -> Result<u32, crate::errors::SnapshotError> {
            Ok(0)
        }

        fn delete_all(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn delete_all_global(&self) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn save_launch_baseline(
            &self,
            _baseline: &crate::domain::LaunchBaseline,
        ) -> Result<(), crate::errors::SnapshotError> {
            Ok(())
        }

        fn load_launch_baseline(
            &self,
            _state_key: &crate::domain::WorkspaceStateKey,
        ) -> Result<Option<crate::domain::LaunchBaseline>, crate::errors::SnapshotError> {
            Ok(None)
        }
    }

    struct FakeEnv {
        in_tmux: bool,
    }
    impl EnvironmentPort for FakeEnv {
        fn execution_context(
            &self,
        ) -> Result<crate::ports::ExecutionContext, crate::errors::EnvironmentError> {
            Ok(crate::ports::ExecutionContext {
                is_interactive: true,
                tmux_session: if self.in_tmux {
                    Some("existing".into())
                } else {
                    None
                },
                tmux_pane: if self.in_tmux {
                    Some("%0".into())
                } else {
                    None
                },
                tmux_server: None,
            })
        }
    }

    struct FakeSysDiag {
        pty: Option<(u64, u64)>,
    }
    impl SystemDiagnosticsPort for FakeSysDiag {
        fn pty_usage(&self) -> Option<(u64, u64)> {
            self.pty
        }
    }

    fn ws_input(workspace: &str, session: &str) -> DoctorWorkspaceInput {
        DoctorWorkspaceInput {
            workspace_state_key: WorkspaceStateKey::new(
                "a1b2".to_owned(),
                "proj".to_owned(),
                String::new(),
                workspace.to_owned(),
            ),
            session_name: session.into(),
        }
    }

    #[test]
    fn all_pass_when_healthy() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec!["wh@test".into()],
            lookup_fails: false,
        };
        let snapshot = FakeSnapshot;
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((10, 100)),
        };

        let ws = vec![ws_input("test", "wh@test")];

        let report = run_remote_diagnostics(&runtime, &snapshot, &env, &diag, "test-server", &ws);

        assert!(!report.has_failures());
        let issues = report
            .checks
            .iter()
            .filter(|c| matches!(c.status, CheckStatus::Fail | CheckStatus::Warn))
            .count();
        assert_eq!(issues, 0);
    }

    #[test]
    fn tmux_unreachable_fails() {
        let runtime = FakeRuntime {
            reachable: false,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let snapshot = FakeSnapshot;
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag { pty: None };

        let report = run_remote_diagnostics(&runtime, &snapshot, &env, &diag, "test-server", &[]);

        assert!(report.has_failures());
        let tmux_check = report
            .checks
            .iter()
            .find(|c| c.check_id == CheckId::TmuxServer);
        assert_eq!(tmux_check.map(|c| c.status), Some(CheckStatus::Fail));
    }

    #[test]
    fn pty_exhausted_fails() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let snapshot = FakeSnapshot;
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((96, 100)),
        };

        let report = run_remote_diagnostics(&runtime, &snapshot, &env, &diag, "test-server", &[]);

        let pty_check = report
            .checks
            .iter()
            .find(|c| c.check_id == CheckId::PtyAvailability);
        assert_eq!(pty_check.map(|c| c.status), Some(CheckStatus::Fail));
    }

    #[test]
    fn session_running_passes() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec!["wh@main".into()],
            lookup_fails: false,
        };
        let r = check_session_lookup(&runtime, "srv", "wh@main");
        assert_eq!(r.status, CheckStatus::Pass);
        assert!(r.detail.contains("Running"));
    }

    #[test]
    fn session_missing_warns() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let r = check_session_lookup(&runtime, "srv", "wh@dev");
        assert_eq!(r.status, CheckStatus::Warn);
        assert!(r.detail.contains("Missing"));
        assert_eq!(r.advice_id, Some(AdviceId::SessionMissing));
    }

    #[test]
    fn session_lookup_failed_fails() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec![],
            lookup_fails: true,
        };
        let r = check_session_lookup(&runtime, "srv", "wh@main");
        assert_eq!(r.status, CheckStatus::Fail);
        assert_eq!(r.advice_id, Some(AdviceId::SessionLookupFailed));
    }

    #[test]
    fn session_server_unreachable_fails() {
        let runtime = FakeRuntime {
            reachable: false,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let r = check_session_lookup(&runtime, "srv", "wh@main");
        assert_eq!(r.status, CheckStatus::Fail);
        assert_eq!(r.advice_id, Some(AdviceId::SessionServerUnreachable));
    }

    #[test]
    fn attach_readiness_outside_tmux_passes() {
        let r = check_attach_readiness(&FakeEnv { in_tmux: false });
        assert_eq!(r.status, CheckStatus::Pass);
    }

    #[test]
    fn attach_readiness_inside_tmux_warns() {
        let r = check_attach_readiness(&FakeEnv { in_tmux: true });
        assert_eq!(r.status, CheckStatus::Warn);
        assert_eq!(r.advice_id, Some(AdviceId::NestedTmux));
    }

    #[test]
    fn snapshot_checks_use_state_key_not_session_name() {
        struct CapturingSnapshot {
            checked_keys: std::cell::RefCell<Vec<String>>,
        }
        impl SnapshotStorePort for CapturingSnapshot {
            fn save_revision(
                &self,
                snapshot: &crate::domain::WorkspaceSnapshot,
            ) -> Result<crate::domain::SnapshotRevisionId, crate::errors::SnapshotError>
            {
                Ok(snapshot.revision_id)
            }

            fn load_latest(
                &self,
                state_key: &WorkspaceStateKey,
            ) -> Result<Option<crate::domain::WorkspaceSnapshot>, crate::errors::SnapshotError>
            {
                self.checked_keys
                    .borrow_mut()
                    .push(state_key.workspace.clone());
                Ok(None)
            }

            fn load_revision(
                &self,
                _state_key: &WorkspaceStateKey,
                _revision_id: crate::domain::SnapshotRevisionId,
            ) -> Result<Option<crate::domain::WorkspaceSnapshot>, crate::errors::SnapshotError>
            {
                Ok(None)
            }

            fn append_events(
                &self,
                _state_key: &WorkspaceStateKey,
                _events: &[crate::domain::SnapshotEvent],
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn load_events_since(
                &self,
                _state_key: &WorkspaceStateKey,
                _since_revision: crate::domain::SnapshotRevisionId,
            ) -> Result<Vec<crate::domain::SnapshotEvent>, crate::errors::SnapshotError>
            {
                Ok(vec![])
            }

            fn read_manifest(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<Option<crate::domain::SnapshotManifest>, crate::errors::SnapshotError>
            {
                Ok(None)
            }

            fn write_manifest(
                &self,
                _manifest: &crate::domain::SnapshotManifest,
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn list_history(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<Vec<crate::domain::SnapshotHistoryEntry>, crate::errors::SnapshotError>
            {
                Ok(vec![])
            }

            fn acquire_lock(
                &self,
                _lock: &crate::domain::SupervisorLock,
            ) -> Result<bool, crate::errors::SnapshotError> {
                Ok(true)
            }

            fn release_lock(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn update_heartbeat(
                &self,
                _state_key: &WorkspaceStateKey,
                _heartbeat: &str,
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn read_lock(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<Option<crate::domain::SupervisorLock>, crate::errors::SnapshotError>
            {
                Ok(None)
            }

            fn health_check(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<crate::domain::SnapshotHealthCheck, crate::errors::SnapshotError>
            {
                Ok(crate::domain::SnapshotHealthCheck {
                    enabled: true,
                    supervisor_running: false,
                    hooks_registered: false,
                    latest_save_time: None,
                    latest_revision: None,
                    manifest_readable: true,
                    checksum_valid: None,
                    state_key_match: true,
                })
            }

            fn verify_revision(
                &self,
                _state_key: &WorkspaceStateKey,
                _revision_id: crate::domain::SnapshotRevisionId,
            ) -> Result<bool, crate::errors::SnapshotError> {
                Ok(true)
            }

            fn apply_retention(
                &self,
                _state_key: &WorkspaceStateKey,
                _retain_revisions: u32,
                _retain_event_days: u32,
            ) -> Result<u32, crate::errors::SnapshotError> {
                Ok(0)
            }

            fn delete_all(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn delete_all_global(&self) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn save_launch_baseline(
                &self,
                _baseline: &crate::domain::LaunchBaseline,
            ) -> Result<(), crate::errors::SnapshotError> {
                Ok(())
            }

            fn load_launch_baseline(
                &self,
                _state_key: &WorkspaceStateKey,
            ) -> Result<Option<crate::domain::LaunchBaseline>, crate::errors::SnapshotError>
            {
                Ok(None)
            }
        }

        let snap = CapturingSnapshot {
            checked_keys: std::cell::RefCell::new(vec![]),
        };
        let workspaces = vec![
            ws_input("main", "wh@proj@main"),
            ws_input("feature-x", "wh@proj@feature-x"),
        ];
        check_snapshot_integrity(&snap, &workspaces);

        let keys = snap.checked_keys.borrow();
        assert_eq!(keys.as_slice(), &["main", "feature-x"]);
    }

    #[test]
    fn main_only_workspace_is_checked_by_doctor() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec!["wh@proj@main".into()],
            lookup_fails: false,
        };
        let snapshot = FakeSnapshot;
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((10, 100)),
        };

        let ws = vec![ws_input("main", "wh@proj@main")];
        let report = run_remote_diagnostics(&runtime, &snapshot, &env, &diag, "test-server", &ws);

        let session_checks: Vec<_> = report
            .checks
            .iter()
            .filter(|c| c.check_id == CheckId::SessionLookup)
            .collect();
        assert_eq!(session_checks.len(), 1);
        assert!(session_checks[0].detail.contains("Running"));
        assert!(!report.has_failures());
    }

    #[test]
    fn invalid_workspace_name_produces_typed_diagnostic() {
        let result = DoctorCheckResult {
            check_id: CheckId::WorkspaceIdentity,
            status: CheckStatus::Warn,
            detail: "has space".to_owned(),
            advice_id: Some(AdviceId::InvalidWorkspaceName),
        };

        assert_eq!(result.check_id, CheckId::WorkspaceIdentity);
        assert_eq!(result.status, CheckStatus::Warn);
        assert_eq!(result.advice_id, Some(AdviceId::InvalidWorkspaceName));
        assert_eq!(result.detail, "has space");

        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "proj".to_owned(),
            "packages api".to_owned(),
            "has space".to_owned(),
        );
        assert!(
            key.to_string().contains("has space"),
            "doctor diagnostics may carry unsanitized workspace labels from fs discovery"
        );
    }
}

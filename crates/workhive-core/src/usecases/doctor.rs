//! `doctor` use case — run diagnostic checks on the workhive environment.
//!
//! Returns structured diagnostic data only. All human-readable text
//! (labels, advice) is resolved by the CLI layer through i18n keys.

use crate::domain::{RuntimeBackendId, SessionAvailability};
use crate::ports::{BackendUnavailableReason, EnvironmentPort, RuntimeError, WorkspaceRuntimePort};
use crate::usecases::open::classify_session_availability;

/// Machine-readable check identifier.
///
/// Each variant maps to a corresponding `doctor.check.*` i18n key
/// in the CLI rendering layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckId {
    RuntimeBackend,
    RuntimeScope,
    RuntimeBinding,
    SessionLookup,
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
    RuntimeUnavailable {
        reason: BackendUnavailableReason,
        detail: String,
    },
    RuntimeError,
    RuntimeBindingMismatch,
    SessionMissing,
    SessionServerUnreachable,
    SessionLookupFailed,
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
/// Each input carries its persisted backend, scope, and workspace identifier.
/// Runtime reachability and workspace lookup use those opaque identifiers.
#[derive(Debug, Clone)]
pub struct DoctorWorkspaceInput {
    pub session_name: String,
    pub backend: RuntimeBackendId,
    pub scope_id: String,
}

/// Run remote/session diagnostics.
///
/// Checks per-record runtime reachability, session lookup, PTY availability,
/// and attach readiness.
pub fn run_remote_diagnostics(
    ws_runtime: &dyn WorkspaceRuntimePort,
    env: &dyn EnvironmentPort,
    sys_diag: &dyn SystemDiagnosticsPort,
    workspaces: &[DoctorWorkspaceInput],
    configured_backend: &RuntimeBackendId,
    default_scope: &str,
) -> DoctorReport {
    let mut checked_scopes = vec![default_scope.to_owned()];
    let mut checks = vec![check_runtime_scope(
        ws_runtime,
        default_scope,
        CheckId::RuntimeBackend,
        configured_backend.to_string(),
    )];

    for ws in workspaces {
        if &ws.backend != configured_backend {
            checks.push(DoctorCheckResult {
                check_id: CheckId::RuntimeBinding,
                status: CheckStatus::Fail,
                detail: format!(
                    "{}: configured {}, bound {}",
                    ws.session_name, configured_backend, ws.backend
                ),
                advice_id: Some(AdviceId::RuntimeBindingMismatch),
            });
            continue;
        }

        if !checked_scopes.contains(&ws.scope_id) {
            checks.push(check_runtime_scope(
                ws_runtime,
                &ws.scope_id,
                CheckId::RuntimeScope,
                format!("{configured_backend}: {}", ws.scope_id),
            ));
            checked_scopes.push(ws.scope_id.clone());
        }

        checks.push(check_session_lookup(
            ws_runtime,
            &ws.scope_id,
            &ws.session_name,
        ));
    }

    checks.push(check_pty_availability(sys_diag));
    checks.push(check_attach_readiness(env));

    DoctorReport { checks }
}

/// Map a persisted workspace identity record to a `DoctorWorkspaceInput`.
///
/// This is the single production mapper called by `cmd_doctor` for every
/// persisted record. Tests must call this same function.
pub fn map_record_to_doctor_input(
    _project: &crate::domain::ProjectIdentityRecord,
    record: &crate::domain::WorkspaceIdentityRecord,
) -> DoctorWorkspaceInput {
    DoctorWorkspaceInput {
        session_name: record.runtime_binding().workspace_id.clone(),
        backend: record.runtime_binding().backend.clone(),
        scope_id: record.runtime_binding().scope_id.clone(),
    }
}

fn check_runtime_scope(
    ws_runtime: &dyn WorkspaceRuntimePort,
    scope: &str,
    check_id: CheckId,
    check_detail: String,
) -> DoctorCheckResult {
    match ws_runtime.list_workspaces(scope) {
        Ok(_) => DoctorCheckResult {
            check_id,
            status: CheckStatus::Pass,
            detail: check_detail,
            advice_id: None,
        },
        Err(RuntimeError::BackendUnavailable { reason, detail }) => DoctorCheckResult {
            check_id,
            status: CheckStatus::Fail,
            detail: check_detail,
            advice_id: Some(AdviceId::RuntimeUnavailable { reason, detail }),
        },
        Err(_) => DoctorCheckResult {
            check_id,
            status: CheckStatus::Fail,
            detail: check_detail,
            advice_id: Some(AdviceId::RuntimeError),
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
    use crate::ports::TmuxMembership;
    match env.execution_context() {
        Ok(ctx) => match &ctx.tmux {
            TmuxMembership::Outside => DoctorCheckResult {
                check_id: CheckId::AttachReadiness,
                status: CheckStatus::Pass,
                detail: "ready".to_owned(),
                advice_id: None,
            },
            TmuxMembership::Resolved { .. } | TmuxMembership::Unresolved { .. } => {
                DoctorCheckResult {
                    check_id: CheckId::AttachReadiness,
                    status: CheckStatus::Warn,
                    detail: "nested".to_owned(),
                    advice_id: Some(AdviceId::NestedTmux),
                }
            }
        },
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
                    reason: BackendUnavailableReason::BackendBinaryNotFound {
                        binary: "tmux".into(),
                    },
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
        fn configure_workspace(
            &self,
            _g: &str,
            _w: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn attach_workspace(
            &self,
            _r: crate::ports::AttachWorkspaceRequest,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &crate::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
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
                    reason: BackendUnavailableReason::BackendBinaryNotFound {
                        binary: "tmux".into(),
                    },
                    detail: "fake".into(),
                });
            }
            Ok(self
                .existing_sessions
                .iter()
                .map(|n| crate::ports::WorkspaceRuntimeInfo {
                    runtime_id: n.clone(),
                    label: None,
                    unit_count: 1,
                    attached: false,
                })
                .collect())
        }
        fn destroy_group(&self, _g: &str) -> Result<(), RuntimeError> {
            Ok(())
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
                tmux: if self.in_tmux {
                    crate::ports::TmuxMembership::Resolved {
                        server: "default".to_owned(),
                        session: "existing".to_owned(),
                        pane: Some("%0".to_owned()),
                    }
                } else {
                    crate::ports::TmuxMembership::Outside
                },
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

    fn ws_input(server: &str, session: &str) -> DoctorWorkspaceInput {
        ws_input_with_workspace(server, session, "main")
    }

    fn ws_input_with_workspace(
        server: &str,
        session: &str,
        _workspace: &str,
    ) -> DoctorWorkspaceInput {
        DoctorWorkspaceInput {
            session_name: session.into(),
            backend: RuntimeBackendId::tmux(),
            scope_id: server.to_owned(),
        }
    }

    #[test]
    fn all_pass_when_healthy() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec!["wh@test".into()],
            lookup_fails: false,
        };
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((10, 100)),
        };

        let ws = vec![ws_input("test", "wh@test")];

        let report = run_remote_diagnostics(
            &runtime,
            &env,
            &diag,
            &ws,
            &RuntimeBackendId::tmux(),
            "test",
        );

        assert!(!report.has_failures());
        let issues = report
            .checks
            .iter()
            .filter(|c| matches!(c.status, CheckStatus::Fail | CheckStatus::Warn))
            .count();
        assert_eq!(issues, 0);
    }

    #[test]
    fn runtime_unreachable_fails_with_typed_reason() {
        let runtime = FakeRuntime {
            reachable: false,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag { pty: None };
        let ws = vec![ws_input("test-server", "wh@test")];

        let report = run_remote_diagnostics(
            &runtime,
            &env,
            &diag,
            &ws,
            &RuntimeBackendId::tmux(),
            "test-server",
        );

        assert!(report.has_failures());
        let runtime_check = report
            .checks
            .iter()
            .find(|c| c.check_id == CheckId::RuntimeBackend);
        assert_eq!(runtime_check.map(|c| c.status), Some(CheckStatus::Fail));
        assert!(matches!(
            runtime_check.and_then(|c| c.advice_id.as_ref()),
            Some(AdviceId::RuntimeUnavailable {
                reason: BackendUnavailableReason::BackendBinaryNotFound { binary },
                ..
            }) if binary == "tmux"
        ));
    }

    #[test]
    fn mismatched_binding_is_reported_without_runtime_lookup() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec![],
            lookup_fails: true,
        };
        let ws = vec![ws_input("tmux-scope", "tmux-workspace")];

        let report = run_remote_diagnostics(
            &runtime,
            &FakeEnv { in_tmux: false },
            &FakeSysDiag { pty: None },
            &ws,
            &RuntimeBackendId::herdr(),
            "herdr-socket",
        );

        assert_eq!(report.checks[0].check_id, CheckId::RuntimeBackend);
        assert_eq!(report.checks[0].detail, "herdr");
        assert!(report.checks.iter().any(|check| {
            check.check_id == CheckId::RuntimeBinding
                && check.status == CheckStatus::Fail
                && check.advice_id == Some(AdviceId::RuntimeBindingMismatch)
        }));
        assert!(!report.checks.iter().any(|check| {
            matches!(
                check.check_id,
                CheckId::RuntimeScope | CheckId::SessionLookup
            )
        }));
    }

    #[test]
    fn configured_backend_is_checked_without_workspace_records() {
        let runtime = FakeRuntime {
            reachable: false,
            existing_sessions: vec![],
            lookup_fails: false,
        };

        let report = run_remote_diagnostics(
            &runtime,
            &FakeEnv { in_tmux: false },
            &FakeSysDiag { pty: None },
            &[],
            &RuntimeBackendId::tmux(),
            "default",
        );

        let backend = &report.checks[0];
        assert_eq!(backend.check_id, CheckId::RuntimeBackend);
        assert_eq!(backend.status, CheckStatus::Fail);
        assert!(matches!(
            backend.advice_id.as_ref(),
            Some(AdviceId::RuntimeUnavailable {
                reason: BackendUnavailableReason::BackendBinaryNotFound { binary },
                ..
            }) if binary == "tmux"
        ));
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.check_id == CheckId::PtyAvailability)
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.check_id == CheckId::AttachReadiness)
        );
    }

    #[test]
    fn pty_exhausted_fails() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec![],
            lookup_fails: false,
        };
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((96, 100)),
        };

        let report = run_remote_diagnostics(
            &runtime,
            &env,
            &diag,
            &[],
            &RuntimeBackendId::tmux(),
            "default",
        );

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
    fn main_only_workspace_is_checked_by_doctor() {
        let runtime = FakeRuntime {
            reachable: true,
            existing_sessions: vec!["wh@proj@main".into()],
            lookup_fails: false,
        };
        let env = FakeEnv { in_tmux: false };
        let diag = FakeSysDiag {
            pty: Some((10, 100)),
        };

        let ws = vec![ws_input("test-server", "wh@proj@main")];
        let report = run_remote_diagnostics(
            &runtime,
            &env,
            &diag,
            &ws,
            &RuntimeBackendId::tmux(),
            "test-server",
        );

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
    }
}

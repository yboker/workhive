use super::*;

/// Simple PTY diagnostics adapter.
///
/// On macOS, queries `sysctl kern.tty.ptmx_max` and
/// `kern.tty.ptmx_count`. Other platforms return None.
struct CliSystemDiagnostics<'a> {
    composition: &'a CliComposition,
}

impl workhive_core::usecases::doctor::SystemDiagnosticsPort for CliSystemDiagnostics<'_> {
    fn pty_usage(&self) -> Option<(u64, u64)> {
        self.composition.pty_usage()
    }
}

fn run_project_diagnostics(
    identity: &dyn workhive_core::ports::WorkspaceIdentityPort,
    runtime: &dyn workhive_core::ports::WorkspaceRuntimePort,
    environment: &dyn workhive_core::ports::EnvironmentPort,
    system_diagnostics: &dyn workhive_core::usecases::doctor::SystemDiagnosticsPort,
    backend: &workhive_core::domain::RuntimeBackendId,
    default_scope: &str,
) -> Result<workhive_core::usecases::doctor::DoctorReport, CliError> {
    use workhive_core::usecases::doctor;

    let records = identity
        .list_workspace_identities()
        .map_err(CliError::Identity)?;
    let project_identity = identity
        .load_project_identity()
        .map_err(CliError::Identity)?
        .ok_or_else(|| CliError::Runtime("project identity not found".to_owned()))?;
    let workspaces: Vec<doctor::DoctorWorkspaceInput> = records
        .iter()
        .map(|record| doctor::map_record_to_doctor_input(&project_identity, record))
        .collect();

    Ok(doctor::run_remote_diagnostics(
        runtime,
        environment,
        system_diagnostics,
        &workspaces,
        backend,
        default_scope,
    ))
}

pub(crate) fn cmd_doctor(fix: bool, ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::{WorkspaceFsPort, WorkspaceIdentityPort};
    use workhive_core::usecases::doctor::{AdviceId, CheckId, CheckStatus};
    use workhive_core::usecases::identity::canonicalize_best_effort;

    let locale = ctx.locale();

    // ── Global diagnostics gate ───────────────────────────────────────
    //
    // The trusted-roots / project-index panel is a *global* diagnostic
    // that doesn't depend on the local project's init state. We render
    // it even when there is no detectable project so that operators in
    // a fresh shell can still inspect the global state of their
    // installation. Project-scoped diagnostics (tmux/pty etc.)
    // run only for a fully initialized project.
    //
    // `doctor` only *inspects* project init state; it never creates,
    // repairs, or prompts to create project configuration. A detected but
    // uninitialized / partially initialized project, and the
    // no-project-detected case, are surfaced as warnings while the global
    // panel still renders.
    let project = detect_project().ok();

    let detected_init_state = project.as_ref().map(|(root, _)| {
        let ws_fs = ctx.composition.workspace_fs();
        usecases::project_state::resolve_init_state(&ws_fs, root.as_ref())
    });

    let init_state = match classify_doctor_project_scope(detected_init_state) {
        DoctorProjectScope::ProjectScoped => None,
        DoctorProjectScope::NotDetected => Some(CliError::Runtime(
            ctx.i18n
                .resolve(locale, k::DOCTOR_ERROR_PROJECT_NOT_DETECTED),
        )),
        DoctorProjectScope::Uninitialized => Some(CliError::Runtime(
            ctx.i18n
                .resolve(locale, k::ERROR_PROJECT_NOT_INITIALIZED_HINT),
        )),
        DoctorProjectScope::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        } => {
            let Some((root, _)) = project.as_ref() else {
                return Err(CliError::Runtime(
                    ctx.i18n
                        .resolve(locale, k::DOCTOR_ERROR_PROJECT_NOT_DETECTED),
                ));
            };
            let root_str = root.as_str();
            Some(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::OPEN_PROJECT_PARTIAL_INIT,
                &[
                    root_str,
                    &has_dir.to_string(),
                    &has_instance_id.to_string(),
                    &has_config.to_string(),
                ],
            )))
        }
    };

    if let Some(err) = init_state {
        // Project-scoped diagnostics cannot run, but we still owe the
        // operator a header and the global trusted-roots / index panel
        // so they can confirm whether the *installation* itself is sane.
        eprintln!("{}", ctx.i18n.resolve(locale, k::DOCTOR_HEADER));
        eprintln!();
        let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
        // Render the full localized condition inside the report (first
        // line anchored to the warn icon, continuation lines indented).
        // The returned error is a brief exit summary only — the same
        // sentence must never appear twice in one doctor run.
        let line = err.to_string();
        let mut lines = line.lines();
        let first = lines.next().unwrap_or("");
        eprintln!("  {warn_icon} {first}");
        for rest in lines {
            eprintln!("     {rest}");
        }
        print_trusted_roots_section(ctx, locale)?;
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::DOCTOR_HAS_ISSUES,
            &["1"],
        )));
    }

    let Some((root, name)) = project else {
        return Err(CliError::Runtime(
            ctx.i18n
                .resolve(locale, k::DOCTOR_ERROR_PROJECT_NOT_DETECTED),
        ));
    };

    // ── Identity drift detection & fix ──────────────────────────────────
    //
    // When the stored project_root in .workhive/identity/project.json
    // differs from the current directory, surface it as a diagnostic
    // issue. With `--fix`, prompt the user and update the stored root.
    let identity_adapter = ctx.composition.workspace_identity(root.as_ref());
    let stored_identity = identity_adapter
        .load_project_identity()
        .map_err(CliError::Identity)?;

    if let Some(ref stored) = stored_identity {
        // Canonicalize both paths to avoid false positives from symlinks
        // or equivalent path forms (e.g., /private/tmp vs /tmp on macOS).
        let canonical_stored = canonicalize_best_effort(&stored.project_root);
        let canonical_current = canonicalize_best_effort(&root);
        if canonical_stored != canonical_current {
            let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
            eprintln!(
                "{}  {}: {}",
                warn_icon,
                ctx.i18n.resolve(locale, k::DOCTOR_CHECK_WORKSPACE_IDENTITY),
                ctx.i18n
                    .resolve(locale, k::DOCTOR_FIX_ROOT_MISMATCH_DETECTED)
            );
            let stored_label = ctx.i18n.resolve(locale, k::DOCTOR_FIX_STORED_ROOT_LABEL);
            let current_label = ctx.i18n.resolve(locale, k::DOCTOR_FIX_CURRENT_ROOT_LABEL);
            eprintln!("      {stored_label} {}", stored.project_root);
            eprintln!("      {current_label} {root}");

            if fix {
                let prompt = ctx.i18n.resolve_with_args(
                    locale,
                    k::DOCTOR_FIX_ROOT_MISMATCH_PROMPT,
                    &[stored.project_root.as_str(), root.as_str()],
                );
                if confirm_with_transcript(ctx, locale, &prompt, true)? {
                    let instance_id = ctx
                        .composition
                        .workspace_fs()
                        .read_instance_id(root.as_ref())
                        .ok_or_else(|| {
                            CliError::Runtime(
                                "instance_id is missing; refusing to repair project identity"
                                    .to_owned(),
                            )
                        })?;
                    workhive_core::usecases::identity::repair_project_identity_root(
                        &identity_adapter,
                        &instance_id,
                        root.as_ref(),
                    )
                    .map_err(CliError::Identity)?;
                    let ok_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_PASS);
                    eprintln!(
                        "  {} {}",
                        ok_icon,
                        ctx.i18n.resolve_with_args(
                            locale,
                            k::DOCTOR_FIX_ROOT_UPDATED,
                            &[root.as_str()]
                        )
                    );
                } else {
                    let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
                    eprintln!(
                        "  {} {}",
                        warn_icon,
                        ctx.i18n.resolve(locale, k::DOCTOR_FIX_ROOT_SKIPPED)
                    );
                }
            } else {
                let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
                eprintln!(
                    "  {} {}",
                    warn_icon,
                    ctx.i18n.resolve(locale, k::DOCTOR_FIX_HINT)
                );
            }
        }
    }

    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();
    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context(&name, "", &root, &git, &ws_fs);
    let default_scope = match policy.resolve_server_for_project(&naming_ctx) {
        Ok(scope) => scope,
        Err(_) => {
            eprintln!("{}", ctx.i18n.resolve(locale, k::DOCTOR_HEADER));
            eprintln!();
            let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
            let msg = ctx.i18n.resolve(locale, k::DOCTOR_PROJECT_NOT_INITIALIZED);
            eprintln!("  {warn_icon} {msg}");
            return Ok(());
        }
    };

    let backend = ctx.runtime_backend_for(Some(root.as_ref()))?;
    let runtime = ctx
        .composition
        .runtime_for_project(root.as_ref(), &backend)?;
    let env = ctx.composition.environment();
    let sys_diag = CliSystemDiagnostics {
        composition: &ctx.composition,
    };
    let doctor_identity_adapter = ctx.composition.workspace_identity(root.as_ref());

    let report = run_project_diagnostics(
        &doctor_identity_adapter,
        runtime.as_ref(),
        &env,
        &sys_diag,
        &backend,
        &default_scope,
    )?;

    eprintln!("{}", ctx.i18n.resolve(locale, k::DOCTOR_HEADER));
    eprintln!();
    eprintln!(
        "  [{}]",
        ctx.i18n.resolve(locale, k::DOCTOR_CATEGORY_REMOTE)
    );

    for check in &report.checks {
        let status_icon = match check.status {
            CheckStatus::Pass => ctx.i18n.resolve(locale, k::DOCTOR_STATUS_PASS),
            CheckStatus::Warn => ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN),
            CheckStatus::Fail => ctx.i18n.resolve(locale, k::DOCTOR_STATUS_FAIL),
        };

        let label = match check.check_id {
            CheckId::RuntimeBackend => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_RUNTIME_BACKEND),
            CheckId::RuntimeScope => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_RUNTIME_SCOPE),
            CheckId::RuntimeBinding => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_RUNTIME_BINDING),
            CheckId::SessionLookup => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_SESSION_LOOKUP),
            CheckId::PtyAvailability => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_PTY_AVAILABILITY),
            CheckId::AttachReadiness => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_ATTACH_READINESS),
            CheckId::WorkspaceIdentity => {
                ctx.i18n.resolve(locale, k::DOCTOR_CHECK_WORKSPACE_IDENTITY)
            }
        };

        let detail_suffix = if check.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", check.detail)
        };

        eprintln!("    {status_icon} {label}{detail_suffix}");

        if let Some(advice_id) = &check.advice_id {
            if let AdviceId::RuntimeUnavailable { reason, detail } = advice_id {
                eprintln!(
                    "      → {}",
                    crate::error_render::render_backend_unavailable(
                        reason, detail, locale, &ctx.i18n
                    )
                );
                continue;
            }

            let advice_key = match advice_id {
                AdviceId::RuntimeUnavailable { .. } => unreachable!(),
                AdviceId::RuntimeError => k::DOCTOR_ADVICE_RUNTIME_ERROR,
                AdviceId::RuntimeBindingMismatch => k::DOCTOR_ADVICE_RUNTIME_BINDING_MISMATCH,
                AdviceId::SessionMissing => k::DOCTOR_ADVICE_SESSION_MISSING,
                AdviceId::SessionServerUnreachable => k::DOCTOR_ADVICE_SESSION_SERVER_UNREACHABLE,
                AdviceId::SessionLookupFailed => k::DOCTOR_ADVICE_SESSION_LOOKUP_FAILED,
                AdviceId::PtyExhausted => k::DOCTOR_ADVICE_PTY_EXHAUSTED,
                AdviceId::PtyElevated => k::DOCTOR_ADVICE_PTY_ELEVATED,
                AdviceId::NestedTmux => k::DOCTOR_ADVICE_NESTED_TMUX,
                AdviceId::InvalidWorkspaceName => k::DOCTOR_ADVICE_INVALID_WORKSPACE_NAME,
            };
            eprintln!("      → {}", ctx.i18n.resolve(locale, advice_key));
        }
    }

    print_trusted_roots_section(ctx, locale)?;

    eprintln!();
    let issue_count = report.issue_count();
    if issue_count == 0 {
        eprintln!("{}", ctx.i18n.resolve(locale, k::DOCTOR_ALL_PASSED));
    } else {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::DOCTOR_HAS_ISSUES, &[&issue_count.to_string()])
        );
    }

    if report.has_failures() {
        Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::DOCTOR_HAS_ISSUES,
            &[&issue_count.to_string()],
        )))
    } else {
        Ok(())
    }
}

/// Render an informational section showing the configured trusted project
/// roots and the current state of the global project index.
///
/// Doctor never *fixes* trusted-roots/index issues — fixing them belongs to
/// `workhive project roots add` and `workhive project reindex`. This is
/// purely diagnostic context.
fn print_trusted_roots_section(ctx: &CommandContext, locale: Locale) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::IndexState;
    use workhive_core::usecases::trusted_roots::trusted_roots_from_config;

    // Doctor's trusted-roots panel is read-only and
    // diagnostic. When the global config can't be parsed, emit an explicit
    // localized "config unreadable" line so the operator sees the truth
    // instead of "no trusted roots configured" — which
    // `trusted_roots_from_config(None)` would render whenever
    // `load_global()` errored.
    let global = super::project::load_global_for_hint(ctx, locale);
    let trusted =
        trusted_roots_from_config(global.as_ref(), super::project::home_path().as_deref());

    eprintln!();
    eprintln!(
        "  [{}]",
        ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_HEADER)
    );
    let count = trusted.len().to_string();
    eprintln!(
        "    {}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_CONFIGURED, &[&count])
    );
    if trusted.is_empty() {
        eprintln!(
            "    {}",
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_EMPTY)
        );
    } else {
        for root in trusted.roots() {
            eprintln!(
                "      - {}",
                crate::path_display::display_path_for_humans(camino::Utf8Path::new(root))
            );
        }
    }

    let index = ctx.composition.project_index()?;
    // `IndexState::Loaded` must surface a *project-index* state label
    // (`PROJECT_INDEX_STATE_PRESENT`) instead of the trusted-roots list
    // header, which produced confusing combined output. The three states
    // (`Loaded`, `NotFound`, `Err`) each have their own i18n key.
    let (state_msg, count_msg) = match index.list_projects() {
        Ok(IndexState::NotFound) => (
            ctx.i18n.resolve(locale, k::PROJECT_INDEX_STATE_MISSING),
            None,
        ),
        Ok(IndexState::Loaded(v)) => (
            ctx.i18n.resolve(locale, k::PROJECT_INDEX_STATE_PRESENT),
            Some(v.len()),
        ),
        Err(e) => {
            let detail = e.to_string();
            (
                ctx.i18n
                    .resolve_with_args(locale, k::PROJECT_INDEX_STATE_ERROR, &[&detail]),
                None,
            )
        }
    };
    eprintln!(
        "    {}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_INDEX, &[&state_msg])
    );
    if let Some(n) = count_msg {
        let n_str = n.to_string();
        eprintln!(
            "    {}",
            ctx.i18n
                .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_PROJECTS, &[&n_str])
        );
    }

    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    write_reconciliation_diagnostic(ctx, locale, &mut output)?;
    Ok(())
}

fn write_reconciliation_diagnostic(
    ctx: &CommandContext,
    locale: Locale,
    output: &mut dyn std::io::Write,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;

    let recon_status = match ctx.config.global_config_dir() {
        Ok(dir) => {
            let reconciliation = ctx.composition.index_reconciliation();
            let recon_state = workhive_core::ports::IndexReconciliationPort::read_state(
                &reconciliation,
                dir.as_ref(),
            );
            super::project::render_reconciliation_status(&recon_state, &ctx.i18n, locale)
        }
        Err(e) => {
            let detail = e.to_string();
            (
                ctx.i18n.resolve_with_args(
                    locale,
                    k::RECONCILIATION_STATUS_FAILED,
                    &["0", &detail],
                ),
                true,
            )
        }
    };
    writeln!(output, "    {}", recon_status.0).map_err(|e| CliError::Runtime(e.to_string()))?;
    if recon_status.1 {
        writeln!(
            output,
            "    {}",
            ctx.i18n.resolve(locale, k::RECONCILIATION_RECOVERY_HINT)
        )
        .map_err(|e| CliError::Runtime(e.to_string()))?;
    }
    Ok(())
}

/// Doctor's read-only classification of the detected project.
///
/// `doctor` is a diagnostic command: it inspects project init state but
/// never creates, repairs, or prompts to create project configuration.
/// Project-scoped diagnostics run only for a fully initialized project;
/// every other state is surfaced as a warning while global diagnostics
/// continue.
#[derive(Debug, PartialEq, Eq)]
enum DoctorProjectScope {
    /// Fully initialized project — project-scoped diagnostics may run.
    ProjectScoped,
    /// No project was detected from the current directory.
    NotDetected,
    /// Project detected but no `.workhive/` directory exists.
    Uninitialized,
    /// Project detected with a `.workhive/` directory but no instance id,
    /// indicating an interrupted initialization.
    PartiallyInitialized {
        has_dir: bool,
        has_instance_id: bool,
        has_config: bool,
    },
}

/// Map a detected project's init state onto doctor's diagnostic scope.
///
/// `None` means project detection itself failed (no repository), which is
/// distinct from a detected-but-uninitialized project. None of the
/// non-`ProjectScoped` outcomes carry an initialization side effect, which
/// is what keeps `doctor` purely diagnostic.
fn classify_doctor_project_scope(
    init_state: Option<workhive_core::domain::ProjectInitState>,
) -> DoctorProjectScope {
    use workhive_core::domain::ProjectInitState;

    match init_state {
        None => DoctorProjectScope::NotDetected,
        Some(ProjectInitState::Initialized { .. }) => DoctorProjectScope::ProjectScoped,
        Some(ProjectInitState::Uninitialized) => DoctorProjectScope::Uninitialized,
        Some(ProjectInitState::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        }) => DoctorProjectScope::PartiallyInitialized {
            has_dir,
            has_instance_id,
            has_config,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DoctorProjectScope, classify_doctor_project_scope, run_project_diagnostics,
        write_reconciliation_diagnostic,
    };
    use workhive_core::domain::ProjectInitState;
    use workhive_core::usecases::doctor;

    #[test]
    fn trusted_roots_status_surface_doctor_config_failure_shows_recovery() {
        use crate::context::{CommandContext, FakeProcessLauncher};
        use workhive_testkit::fakes::FakeConfig;
        let ctx = CommandContext::with_test_boundaries(
            Box::new(FakeConfig::new().with_global(Default::default())),
            FakeProcessLauncher::new(),
        );
        let mut output = Vec::new();
        write_reconciliation_diagnostic(&ctx, workhive_core::domain::Locale::En, &mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("<fake-config-dir>"), "{output}");
        assert!(output.contains("workhive project reindex"), "{output}");
    }

    #[test]
    fn initialized_project_runs_project_scoped_diagnostics() {
        let scope = classify_doctor_project_scope(Some(ProjectInitState::Initialized {
            instance_id: "abc123".to_owned(),
        }));
        assert_eq!(scope, DoctorProjectScope::ProjectScoped);
    }

    #[test]
    fn missing_repository_is_not_detected() {
        assert_eq!(
            classify_doctor_project_scope(None),
            DoctorProjectScope::NotDetected
        );
    }

    #[test]
    fn uninitialized_project_is_warned_without_init() {
        assert_eq!(
            classify_doctor_project_scope(Some(ProjectInitState::Uninitialized)),
            DoctorProjectScope::Uninitialized
        );
    }

    #[test]
    fn partial_project_carries_state_detail_for_diagnosis() {
        let scope = classify_doctor_project_scope(Some(ProjectInitState::PartiallyInitialized {
            has_dir: true,
            has_instance_id: false,
            has_config: true,
        }));
        assert_eq!(
            scope,
            DoctorProjectScope::PartiallyInitialized {
                has_dir: true,
                has_instance_id: false,
                has_config: true,
            }
        );
    }

    // ── doctor_scope_flow_ — production mapper and per-record checks ──

    #[test]
    fn doctor_scope_flow_propagates_identity_store_error() {
        use workhive_core::domain::{
            IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
        };
        use workhive_core::ports::WorkspaceIdentityPort;

        struct FailingIdentityPort;

        impl WorkspaceIdentityPort for FailingIdentityPort {
            fn load_project_identity(
                &self,
            ) -> Result<Option<ProjectIdentityRecord>, IdentityError> {
                Ok(None)
            }
            fn save_project_identity(
                &self,
                _: &ProjectIdentityRecord,
            ) -> Result<(), IdentityError> {
                Ok(())
            }
            fn load_workspace_identity(
                &self,
                _: &WorkspaceIdentityKey,
            ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError> {
                Ok(None)
            }
            fn save_workspace_identity(
                &self,
                _: &WorkspaceIdentityRecord,
            ) -> Result<(), IdentityError> {
                Ok(())
            }
            fn list_workspace_identities(
                &self,
            ) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
                Err(IdentityError::ReadFailed {
                    path: camino::Utf8PathBuf::from("/fake/workspaces"),
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "permission denied",
                    ),
                })
            }
        }

        let port = FailingIdentityPort;
        let result = port.list_workspace_identities();
        assert!(
            matches!(result, Err(IdentityError::ReadFailed { .. })),
            "identity store read failure must propagate as typed error"
        );
    }

    #[test]
    fn doctor_scope_flow_uses_each_records_server_session_and_state_key() {
        use workhive_core::domain::{
            ProjectIdentityRecord, RelativeProjectPath, RuntimeBinding, WorkspaceIdentityRecord,
        };
        let project = ProjectIdentityRecord {
            instance_id: "a1b2".to_owned(),
            project_root: camino::Utf8PathBuf::from("/proj"),
            project_display: "proj".to_owned(),
            canonical_project_component: "proj".to_owned(),
        };

        let root_record = WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            canonical_session_name: "wh@a1b2@proj@main".to_owned(),
            canonical_server_name: "whsrv-a1b2-proj".to_owned(),
            runtime_binding: RuntimeBinding::tmux("whsrv-a1b2-proj", "wh@a1b2@proj@main"),
            runtime_replacement: None,
        };

        let scoped_record = WorkspaceIdentityRecord {
            workspace: "feat-x".to_owned(),
            relative_project_path: Some(RelativeProjectPath::new("services/api").unwrap()),
            relative_display_path: Some("proj/services/api/feat-x".to_owned()),
            canonical_workspace_component: "feat-x".to_owned(),
            canonical_session_name: "wh@a1b2@proj@feat-x".to_owned(),
            canonical_server_name: "whsrv-other-proj".to_owned(),
            runtime_binding: RuntimeBinding::tmux("whsrv-other-proj", "wh@a1b2@proj@feat-x"),
            runtime_replacement: None,
        };

        let input_root = doctor::map_record_to_doctor_input(&project, &root_record);
        let input_scoped = doctor::map_record_to_doctor_input(&project, &scoped_record);

        assert_eq!(
            input_root.scope_id, "whsrv-a1b2-proj",
            "root record server must come from persisted record"
        );
        assert_eq!(
            input_root.session_name, "wh@a1b2@proj@main",
            "root record session must come from persisted record"
        );

        assert_eq!(
            input_scoped.scope_id, "whsrv-other-proj",
            "scoped record must use its own server"
        );
        assert_eq!(
            input_scoped.session_name, "wh@a1b2@proj@feat-x",
            "scoped record session must come from persisted record"
        );
    }

    // ── doctor_production_flow_ — production mapping + diagnostics ──

    use std::cell::RefCell;
    use workhive_core::ports::{EnvironmentPort, RuntimeError, WorkspaceRuntimePort};

    struct RecordingRuntime {
        server_checks: RefCell<Vec<String>>,
        session_queries: RefCell<Vec<(String, String)>>,
    }

    impl RecordingRuntime {
        fn new() -> Self {
            Self {
                server_checks: RefCell::new(Vec::new()),
                session_queries: RefCell::new(Vec::new()),
            }
        }
    }

    impl WorkspaceRuntimePort for RecordingRuntime {
        fn has_workspace(&self, server: &str, session: &str) -> Result<bool, RuntimeError> {
            self.session_queries
                .borrow_mut()
                .push((server.to_owned(), session.to_owned()));
            Ok(false)
        }
        fn create_workspace(
            &self,
            _: workhive_core::ports::CreateWorkspaceRuntimeRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn configure_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn attach_workspace(
            &self,
            _: workhive_core::ports::AttachWorkspaceRequest,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn switch_workspace(
            &self,
            _: &str,
            _: &str,
            _: &workhive_core::ports::RuntimeInteractionPolicy,
        ) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn destroy_workspace(&self, _: &str, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
        fn list_workspaces(
            &self,
            server: &str,
        ) -> Result<Vec<workhive_core::ports::WorkspaceRuntimeInfo>, RuntimeError> {
            self.server_checks.borrow_mut().push(server.to_owned());
            Ok(vec![])
        }
        fn destroy_group(&self, _: &str) -> Result<(), RuntimeError> {
            unreachable!()
        }
    }

    struct NoopEnv;
    impl EnvironmentPort for NoopEnv {
        fn execution_context(
            &self,
        ) -> Result<workhive_core::ports::ExecutionContext, workhive_core::errors::EnvironmentError>
        {
            Ok(workhive_core::ports::ExecutionContext {
                is_interactive: false,
                tmux: workhive_core::ports::TmuxMembership::Outside,
            })
        }
    }

    struct NoopSysDiag;
    impl workhive_core::usecases::doctor::SystemDiagnosticsPort for NoopSysDiag {
        fn pty_usage(&self) -> Option<(u64, u64)> {
            None
        }
    }

    struct StaticIdentityPort {
        project: workhive_core::domain::ProjectIdentityRecord,
        records: Vec<workhive_core::domain::WorkspaceIdentityRecord>,
    }

    impl workhive_core::ports::WorkspaceIdentityPort for StaticIdentityPort {
        fn load_project_identity(
            &self,
        ) -> Result<
            Option<workhive_core::domain::ProjectIdentityRecord>,
            workhive_core::domain::IdentityError,
        > {
            Ok(Some(self.project.clone()))
        }

        fn save_project_identity(
            &self,
            _: &workhive_core::domain::ProjectIdentityRecord,
        ) -> Result<(), workhive_core::domain::IdentityError> {
            unreachable!()
        }

        fn load_workspace_identity(
            &self,
            _: &workhive_core::domain::WorkspaceIdentityKey,
        ) -> Result<
            Option<workhive_core::domain::WorkspaceIdentityRecord>,
            workhive_core::domain::IdentityError,
        > {
            unreachable!()
        }

        fn save_workspace_identity(
            &self,
            _: &workhive_core::domain::WorkspaceIdentityRecord,
        ) -> Result<(), workhive_core::domain::IdentityError> {
            unreachable!()
        }

        fn list_workspace_identities(
            &self,
        ) -> Result<
            Vec<workhive_core::domain::WorkspaceIdentityRecord>,
            workhive_core::domain::IdentityError,
        > {
            Ok(self.records.clone())
        }
    }

    #[test]
    fn doctor_production_flow_propagates_identity_store_error() {
        use workhive_core::domain::{
            IdentityError, ProjectIdentityRecord, WorkspaceIdentityKey, WorkspaceIdentityRecord,
        };
        use workhive_core::ports::WorkspaceIdentityPort;

        struct FailingIdentityPort;

        impl WorkspaceIdentityPort for FailingIdentityPort {
            fn load_project_identity(
                &self,
            ) -> Result<Option<ProjectIdentityRecord>, IdentityError> {
                Ok(None)
            }
            fn save_project_identity(
                &self,
                _: &ProjectIdentityRecord,
            ) -> Result<(), IdentityError> {
                Ok(())
            }
            fn load_workspace_identity(
                &self,
                _: &WorkspaceIdentityKey,
            ) -> Result<Option<WorkspaceIdentityRecord>, IdentityError> {
                Ok(None)
            }
            fn save_workspace_identity(
                &self,
                _: &WorkspaceIdentityRecord,
            ) -> Result<(), IdentityError> {
                Ok(())
            }
            fn list_workspace_identities(
                &self,
            ) -> Result<Vec<WorkspaceIdentityRecord>, IdentityError> {
                Err(IdentityError::ReadFailed {
                    path: camino::Utf8PathBuf::from("/fake/workspaces"),
                    source: std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "permission denied",
                    ),
                })
            }
        }

        let port = FailingIdentityPort;
        let runtime = RecordingRuntime::new();
        let result = run_project_diagnostics(
            &port,
            &runtime,
            &NoopEnv,
            &NoopSysDiag,
            &workhive_core::domain::RuntimeBackendId::tmux(),
            "default",
        );

        assert!(matches!(
            result,
            Err(crate::error::CliError::Identity(
                IdentityError::ReadFailed { .. }
            ))
        ));
        assert!(runtime.server_checks.borrow().is_empty());
        assert!(runtime.session_queries.borrow().is_empty());
    }

    #[test]
    fn doctor_production_flow_uses_each_record_runtime_pair() {
        use workhive_core::domain::{
            ProjectIdentityRecord, RuntimeBinding, WorkspaceIdentityRecord,
        };
        let project = ProjectIdentityRecord {
            instance_id: "a1b2".to_owned(),
            project_root: camino::Utf8PathBuf::from("/proj"),
            project_display: "proj".to_owned(),
            canonical_project_component: "proj".to_owned(),
        };

        let root_record = WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            canonical_session_name: "wh@a1b2@proj@main".to_owned(),
            runtime_binding: RuntimeBinding::tmux("whsrv-a1b2-proj", "wh@a1b2@proj@main"),
            runtime_replacement: None,
            canonical_server_name: "whsrv-a1b2-proj".to_owned(),
        };

        let scoped_record = WorkspaceIdentityRecord {
            workspace: "feat-x".to_owned(),
            relative_project_path: Some(
                workhive_core::domain::RelativeProjectPath::new("services/api").unwrap(),
            ),
            relative_display_path: Some("proj/services/api/feat-x".to_owned()),
            canonical_workspace_component: "feat-x".to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-proj-scoped",
                "wh@a1b2@proj-scoped@feat-x",
            ),
            runtime_replacement: None,
            canonical_session_name: "wh@a1b2@proj-scoped@feat-x".to_owned(),
            canonical_server_name: "whsrv-a1b2-proj-scoped".to_owned(),
        };

        let identity = StaticIdentityPort {
            project,
            records: vec![root_record, scoped_record],
        };
        let runtime = RecordingRuntime::new();
        let env = NoopEnv;
        let diag = NoopSysDiag;

        let report = run_project_diagnostics(
            &identity,
            &runtime,
            &env,
            &diag,
            &workhive_core::domain::RuntimeBackendId::tmux(),
            "whsrv-a1b2-proj",
        );
        assert!(report.is_ok());

        let server_checks = runtime.server_checks.borrow();
        assert!(
            server_checks.contains(&"whsrv-a1b2-proj".to_owned()),
            "root server must be checked: {server_checks:?}"
        );
        assert!(
            server_checks.contains(&"whsrv-a1b2-proj-scoped".to_owned()),
            "scoped server must be checked: {server_checks:?}"
        );

        let session_queries = runtime.session_queries.borrow();
        assert!(
            session_queries
                .iter()
                .any(|(s, sess)| s == "whsrv-a1b2-proj" && sess == "wh@a1b2@proj@main"),
            "root session must be queried on its own server: {session_queries:?}"
        );
        assert!(
            session_queries
                .iter()
                .any(|(s, sess)| s == "whsrv-a1b2-proj-scoped"
                    && sess == "wh@a1b2@proj-scoped@feat-x"),
            "scoped session must be queried on its own server: {session_queries:?}"
        );
    }
}

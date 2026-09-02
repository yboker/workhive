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

pub(crate) fn cmd_doctor(ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::doctor::{
        self, AdviceId, CheckId, CheckStatus, DoctorWorkspaceInput,
    };

    let locale = ctx.locale();

    // ── Global diagnostics gate ───────────────────────────────────────
    //
    // The trusted-roots / project-index panel is a *global* diagnostic
    // that doesn't depend on the local project's init state. We render
    // it even when there is no detectable project so that operators in
    // a fresh shell can still inspect the global state of their
    // installation. Project-scoped diagnostics (tmux/snapshot/pty etc.)
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
            let root_str = project
                .as_ref()
                .map(|(root, _)| root.as_str())
                .unwrap_or_default();
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

    // SAFETY: when `init_state` is None we definitely entered the
    // `Some((root, name))` arm above, so `project` is always `Some`.
    let (root, name) = project.expect("project detection must have succeeded if init check passed");

    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();
    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context(&name, "", &root, &git, &ws_fs);
    let server = match policy.resolve_server_for_project(&naming_ctx) {
        Ok(s) => s,
        Err(_) => {
            eprintln!("{}", ctx.i18n.resolve(locale, k::DOCTOR_HEADER));
            eprintln!();
            let warn_icon = ctx.i18n.resolve(locale, k::DOCTOR_STATUS_WARN);
            let msg = ctx.i18n.resolve(locale, k::DOCTOR_PROJECT_NOT_INITIALIZED);
            eprintln!("  {warn_icon} {msg}");
            return Ok(());
        }
    };

    let ws_dirs = ws_fs.list_workspace_dirs(root.as_ref()).map_err(|e| {
        tracing::error!(error = %e, "workspace enumeration failed in doctor");
        CliError::Runtime(
            ctx.i18n
                .resolve(locale, k::DOCTOR_ERROR_WORKSPACE_ENUM_FAILED),
        )
    })?;

    let branch = git.current_branch().ok();
    let instance_id = ws_fs.read_instance_id(root.as_ref());

    let mut invalid_ws_diagnostics: Vec<doctor::DoctorCheckResult> = Vec::new();

    let doctor_identity_adapter = ctx.composition.workspace_identity(root.as_ref());
    let doctor_cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());
    let doctor_subdir_key = usecases::naming::resolve_subdir_key(&root, doctor_cwd.as_deref());

    let try_resolve_ws =
        |ws_name: &str| -> Result<DoctorWorkspaceInput, doctor::DoctorCheckResult> {
            let session_name = policy
                .resolve_session(&usecases::naming::assemble_naming_context(
                    &name,
                    ws_name,
                    &root,
                    branch.clone(),
                    instance_id.clone(),
                    None,
                ))
                .map_err(|_| doctor::DoctorCheckResult {
                    check_id: doctor::CheckId::WorkspaceIdentity,
                    status: doctor::CheckStatus::Warn,
                    detail: format!("naming resolution failed for workspace '{ws_name}'"),
                    advice_id: Some(doctor::AdviceId::InvalidWorkspaceName),
                })?;
            let _workspace_id =
                WorkspaceId::new(ws_name).map_err(|_| doctor::DoctorCheckResult {
                    check_id: doctor::CheckId::WorkspaceIdentity,
                    status: doctor::CheckStatus::Warn,
                    detail: ws_name.to_owned(),
                    advice_id: Some(doctor::AdviceId::InvalidWorkspaceName),
                })?;
            let identity_key =
                workhive_core::domain::WorkspaceIdentityKey::new(ws_name, &doctor_subdir_key);
            let workspace_state_key = usecases::identity::derive_workspace_state_key(
                &doctor_identity_adapter,
                &identity_key,
            )
            .map_err(|_| doctor::DoctorCheckResult {
                check_id: doctor::CheckId::WorkspaceIdentity,
                status: doctor::CheckStatus::Warn,
                detail: format!("missing workspace identity record for '{ws_name}'"),
                advice_id: Some(doctor::AdviceId::InvalidWorkspaceName),
            })?;
            Ok(DoctorWorkspaceInput {
                workspace_state_key,
                session_name,
            })
        };

    let main_ws_name = branch.as_deref().unwrap_or("main");
    let mut workspaces: Vec<DoctorWorkspaceInput> = Vec::new();

    match try_resolve_ws(main_ws_name) {
        Ok(ws) => workspaces.push(ws),
        Err(diag) => invalid_ws_diagnostics.push(diag),
    }
    for ws_name in &ws_dirs {
        match try_resolve_ws(ws_name) {
            Ok(ws) => workspaces.push(ws),
            Err(diag) => invalid_ws_diagnostics.push(diag),
        }
    }

    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);
    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());
    let env = ctx.composition.environment();
    let sys_diag = CliSystemDiagnostics {
        composition: &ctx.composition,
    };

    let mut report = doctor::run_remote_diagnostics(
        &runtime,
        &snapshot_store,
        &env,
        &sys_diag,
        &server,
        &workspaces,
    );

    report.checks.extend(invalid_ws_diagnostics);

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
            CheckId::TmuxServer => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_TMUX_SERVER),
            CheckId::SessionLookup => ctx.i18n.resolve(locale, k::DOCTOR_CHECK_SESSION_LOOKUP),
            CheckId::SnapshotIntegrity => {
                ctx.i18n.resolve(locale, k::DOCTOR_CHECK_SNAPSHOT_INTEGRITY)
            }
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
            let advice_key = match advice_id {
                AdviceId::TmuxNotRunning => k::DOCTOR_ADVICE_TMUX_NOT_RUNNING,
                AdviceId::TmuxError => k::DOCTOR_ADVICE_TMUX_ERROR,
                AdviceId::SessionMissing => k::DOCTOR_ADVICE_SESSION_MISSING,
                AdviceId::SessionServerUnreachable => k::DOCTOR_ADVICE_SESSION_SERVER_UNREACHABLE,
                AdviceId::SessionLookupFailed => k::DOCTOR_ADVICE_SESSION_LOOKUP_FAILED,
                AdviceId::SnapshotCorrupt => k::DOCTOR_ADVICE_SNAPSHOT_CORRUPT,
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
    use super::{DoctorProjectScope, classify_doctor_project_scope};
    use workhive_core::domain::ProjectInitState;

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
}

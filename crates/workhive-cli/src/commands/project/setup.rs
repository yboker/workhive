#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

pub(crate) fn detect_project() -> Result<(Utf8PathBuf, String), CliError> {
    let composition = CliComposition::new();
    let git = composition.git();
    match git.repo_root() {
        Ok(root) => {
            let project_root = resolve_main_worktree_root(root, &git)?;
            let name = project_root.file_name().unwrap_or("project").to_owned();
            Ok((project_root, name))
        }
        Err(workhive_core::errors::GitError::NotARepo) => guide_git_init(&git),
        Err(e) => Err(CliError::Git(e)),
    }
}

fn resolve_main_worktree_root(
    fallback_root: Utf8PathBuf,
    git: &dyn GitPort,
) -> Result<Utf8PathBuf, CliError> {
    let worktrees = git.list_worktrees()?;
    Ok(worktrees
        .into_iter()
        .find(|worktree| worktree.is_main)
        .map(|worktree| worktree.path)
        .or_else(|| infer_project_root_from_managed_worktree_path(fallback_root.as_ref()))
        .unwrap_or(fallback_root))
}

fn infer_project_root_from_managed_worktree_path(path: &Utf8Path) -> Option<Utf8PathBuf> {
    let marker = ".workhive/worktrees/";
    let raw = path.as_str();
    let marker_start = raw.find(marker)?;
    if marker_start == 0 {
        return None;
    }

    let project_root = raw[..marker_start].trim_end_matches('/');
    if project_root.is_empty() {
        return None;
    }

    Some(Utf8PathBuf::from(project_root))
}

pub(crate) fn guide_git_init(git: &dyn GitPort) -> Result<(Utf8PathBuf, String), CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;

    let ctx = CommandContext::new();
    let locale = ctx.locale();

    eprintln!("{}", ctx.i18n.resolve(locale, k::GIT_NOT_INITIALIZED));

    if !std::io::stdin().is_terminal() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::GIT_INIT_ABORT),
        ));
    }

    let git_init_prompt = ctx.i18n.resolve(locale, k::GIT_INIT_PROMPT);
    let do_init = crate::commands::confirm_with_transcript(&ctx, locale, &git_init_prompt, true)?;

    if !do_init {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::GIT_INIT_ABORT),
        ));
    }

    let cwd = std::env::current_dir()
        .map_err(|e| CliError::Runtime(format!("cannot determine cwd: {e}")))?;
    let cwd_utf8 = Utf8PathBuf::try_from(cwd)
        .map_err(|e| CliError::Runtime(format!("cwd is not valid UTF-8: {e}")))?;

    git.init_repo(&cwd_utf8)?;

    eprintln!("{}", ctx.i18n.resolve(locale, k::GIT_INIT_SUCCESS));

    let root = git.repo_root()?;
    let name = root.file_name().unwrap_or("project").to_owned();
    Ok((root, name))
}

/// Collect raw inputs from environment/ports and assemble a `NamingContext`.
///
/// Input collection (git branch, instance_id, cwd) stays in CLI.
/// Pure assembly is delegated to `usecases::naming::assemble_naming_context`.
pub(crate) fn build_naming_context(
    project_name: &str,
    workspace: &str,
    project_root: &Utf8PathBuf,
    git: &dyn GitPort,
    ws_fs: &dyn WorkspaceFsPort,
) -> NamingContext {
    let branch = git.current_branch().ok();
    let instance_id = ws_fs.read_instance_id(project_root.as_ref());
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());

    let relative_project_path = usecases::naming::resolve_subdir_key(project_root, cwd.as_deref());
    let ctx = usecases::naming::assemble_naming_context(
        project_name,
        workspace,
        project_root,
        branch,
        instance_id.clone(),
        relative_project_path,
    );
    tracing::debug!(
        project_name,
        project_with_subdir = %ctx.project,
        ?instance_id,
        "naming context built"
    );
    ctx
}

/// Interactive language selection, or default to English in non-interactive mode.
pub(crate) fn select_language(
    force: bool,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<Locale, CliError> {
    use workhive_core::i18n_keys as k;

    if force || !std::io::stdin().is_terminal() {
        tracing::debug!("language selection skipped (non-interactive), defaulting to English");
        return Ok(Locale::En);
    }

    let items = vec![
        i18n.resolve(locale, k::INIT_LANG_ENGLISH),
        i18n.resolve(locale, k::INIT_LANG_ZH_CN),
    ];
    let title = i18n.resolve(locale, k::INIT_SELECT_LANGUAGE);
    let selection = ui::screens::select::run(&title, &items, 0)?;

    let selected = match selection {
        1 => Locale::ZhCn,
        _ => Locale::En,
    };
    let echo_label = match selected {
        Locale::ZhCn => i18n.resolve(locale, k::INIT_LANG_ZH_CN),
        Locale::En => i18n.resolve(locale, k::INIT_LANG_ENGLISH),
    };
    crate::commands::print_choice_transcript(&title, &echo_label);
    tracing::debug!(locale = %selected, "language selected");
    Ok(selected)
}

/// Render stale project entries as user-facing warnings.
///
/// CLI is solely responsible for i18n key selection and rendering.
/// The `StaleProjectEntry` values are structured facts from core.
pub(crate) fn render_stale_entries(
    stale: &[usecases::resolve_projects::StaleProjectEntry],
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) {
    for entry in stale {
        let i18n_key = match entry.reason {
            usecases::resolve_projects::StaleReason::InstanceIdMissing => {
                workhive_core::i18n_keys::INDEX_STALE_ENTRY
            }
            usecases::resolve_projects::StaleReason::ProjectRootGone => {
                workhive_core::i18n_keys::INDEX_STALE_ENTRY_PROJECT_ROOT_GONE
            }
        };
        let msg = ctx
            .i18n
            .resolve_with_args(locale, i18n_key, &[&entry.summary.project_name]);
        tracing::debug!(
            project = %entry.summary.project_name,
            root = %entry.summary.project_root,
            reason = ?entry.reason,
            "stale index entry"
        );
        eprintln!("{msg}");
    }
}

// ── Admission action model ──────────────────────────────────────────────────

/// Admission action chosen by the user at an untrusted-project gate.
///
/// Replaces the binary Yes/No confirmation with a three-action menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionAction {
    /// Trust this project directly (adds the resolved project root).
    TrustProject,
    /// Open the shared trusted-directory editor.
    ManageTrustedDirectories,
    /// Cancel the operation.
    Cancel,
}

/// Which command is calling the admission gate (affects default selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionCaller {
    /// `workspace open` — defaults to TrustProject.
    WorkspaceOpen,
    /// `workhive init` (project-level) — defaults to ManageTrustedDirectories.
    ProjectInit,
}

/// Menu item indices for the admission action menu.
pub(crate) const ADMISSION_INDEX_TRUST_PROJECT: usize = 0;
pub(crate) const ADMISSION_INDEX_MANAGE_DIRECTORIES: usize = 1;
/// Index of the Cancel action. Used by tests to verify menu structure.
pub(crate) const ADMISSION_INDEX_CANCEL: usize = 2;

// Compile-time assertion that menu indices are sequential.
const _: () = assert!(ADMISSION_INDEX_CANCEL == 2);

/// Resolve the default-selected index for the admission menu.
pub(crate) fn admission_menu_default_index(caller: AdmissionCaller) -> usize {
    match caller {
        AdmissionCaller::WorkspaceOpen => ADMISSION_INDEX_TRUST_PROJECT,
        AdmissionCaller::ProjectInit => ADMISSION_INDEX_MANAGE_DIRECTORIES,
    }
}

/// Map a selection index to the corresponding admission action.
pub(crate) fn admission_action_from_index(index: usize) -> AdmissionAction {
    match index {
        ADMISSION_INDEX_TRUST_PROJECT => AdmissionAction::TrustProject,
        ADMISSION_INDEX_MANAGE_DIRECTORIES => AdmissionAction::ManageTrustedDirectories,
        _ => AdmissionAction::Cancel,
    }
}

/// Resolve the admission menu i18n keys for rendering. Returns the 3 labels
/// in fixed order: Trust this project, Manage trusted directories, Cancel.
pub(crate) fn admission_menu_labels(
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: workhive_core::domain::Locale,
) -> Vec<String> {
    use workhive_core::i18n_keys as k;
    vec![
        i18n.resolve(locale, k::ADMISSION_MENU_TRUST_PROJECT),
        i18n.resolve(locale, k::ADMISSION_MENU_MANAGE_DIRECTORIES),
        i18n.resolve(locale, k::ADMISSION_MENU_CANCEL),
    ]
}

/// Interactive input used by the untrusted-project admission gate.
pub(crate) trait AdmissionInput {
    fn select(&self, prompt: &str, items: &[String], default: usize) -> Result<usize, CliError>;
}

pub(crate) struct TtyAdmissionInput;

impl AdmissionInput for TtyAdmissionInput {
    fn select(&self, prompt: &str, items: &[String], default: usize) -> Result<usize, CliError> {
        crate::ui::screens::select::run(prompt, items, default)
    }
}

/// After the editor returns, check whether the project is now admitted.
///
/// Pure decision: given the current trusted-root set and the project root,
/// determines whether the project is covered. Does not perform I/O.
pub(crate) fn is_project_admitted_after_editor(
    project_root: &camino::Utf8Path,
    current_roots: &[camino::Utf8PathBuf],
    home: Option<&camino::Utf8Path>,
) -> bool {
    use workhive_core::domain::TrustedRootSet;
    let trusted = TrustedRootSet::from_raw(current_roots, home);
    let decision = workhive_core::usecases::trusted_roots::admit_mutation(&trusted, project_root);
    matches!(
        decision,
        workhive_core::domain::TrustedRootDecision::Admitted { .. }
    )
}

// ── Global-init trusted-root step decisions ─────────────────────────────────

/// What the global-init trusted-root step should show the user based on
/// the current state of configured roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GlobalInitTrustedRootPrompt {
    /// No roots configured: "Configure trusted directories now?" default Yes.
    OfferConfigure,
    /// Roots already exist: "N trusted directories configured. View or modify?" default No.
    OfferViewModify { count: usize },
}

/// Resolve the global-init final step prompt variant. Pure decision.
pub(crate) fn resolve_global_init_trusted_root_prompt(
    existing_root_count: usize,
) -> GlobalInitTrustedRootPrompt {
    if existing_root_count == 0 {
        GlobalInitTrustedRootPrompt::OfferConfigure
    } else {
        GlobalInitTrustedRootPrompt::OfferViewModify {
            count: existing_root_count,
        }
    }
}

/// Whether the user's answer to the global-init trusted-root prompt means
/// "open the editor". The default differs by prompt variant.
pub(crate) fn global_init_prompt_default_yes(prompt: GlobalInitTrustedRootPrompt) -> bool {
    matches!(prompt, GlobalInitTrustedRootPrompt::OfferConfigure)
}

// ── Trusted-roots / setup-state helpers ──

/// Resolve the user's home directory as a `Utf8PathBuf` if available.
pub(crate) fn home_path() -> Option<camino::Utf8PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(camino::Utf8PathBuf::from)
}

/// Map a reindex error to a localized CLI message. Currently specializes
/// `DiscoveryError::RootNotFound` so the user sees a zh-CN-friendly hint
/// tied to the *authored* root (not the expanded one).
pub(crate) fn localize_reindex_error(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    err: &workhive_core::usecases::reindex::ReindexError,
    authored: &[camino::Utf8PathBuf],
    expanded: &[camino::Utf8PathBuf],
) -> CliError {
    use workhive_core::errors::DiscoveryError;
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::reindex::ReindexError;
    if let ReindexError::Discovery(DiscoveryError::RootNotFound { path }) = err {
        // Find the authored entry whose expansion equals the missing
        // path so the error renders the shape the user actually typed.
        let display = expanded
            .iter()
            .zip(authored.iter())
            .find_map(|(exp, auth)| (exp == path).then(|| auth.clone()))
            .unwrap_or_else(|| path.clone());
        return CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::INIT_TRUSTED_ROOTS_DISCOVERY_ROOT_MISSING,
            &[display.as_str()],
        ));
    }
    CliError::Runtime(err.to_string())
}

/// What the trust gate should do next for a project root that asked for
/// admission. Pure projection of `(admission decision, session
/// interactivity, broad-root guard)` so the policy matrix is unit-testable
/// without a TTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrustGateAction {
    /// The root is already admitted; proceed without interaction.
    Proceed,
    /// The root is not admitted, the session is interactive, and the root
    /// passes the broad-root guard: offer the one-time trust prompt.
    OfferTrustPrompt,
    /// The root is not admitted and no prompt may be offered (non-interactive
    /// session, or the root itself is overly broad): surface the typed
    /// rejection error with its actionable guidance.
    Reject,
}

/// Pure trust-gate policy: admitted roots proceed; unadmitted roots may be
/// offered a one-time interactive trust prompt only when the session is
/// interactive and the root does not trip the broad-root guard.
pub(crate) fn resolve_trust_gate_action(
    decision: &workhive_core::domain::TrustedRootDecision,
    interactive: bool,
    overly_broad: bool,
) -> TrustGateAction {
    use workhive_core::domain::TrustedRootDecision;
    match decision {
        TrustedRootDecision::Admitted { .. } => TrustGateAction::Proceed,
        TrustedRootDecision::NoTrustedRootsConfigured
        | TrustedRootDecision::OutsideTrustedRoots { .. } => {
            if interactive && !overly_broad {
                TrustGateAction::OfferTrustPrompt
            } else {
                TrustGateAction::Reject
            }
        }
    }
}

/// Enforce the trusted-roots admission policy for a mutating project flow.
///
/// Returns `Ok(())` when the project root is admitted. When the root is not
/// admitted and the session is interactive, offers a three-action admission
/// menu (Trust this project / Manage directories / Cancel) with caller-specific
/// defaults. Non-interactive sessions and overly broad roots never prompt —
/// they fail explicitly with the localized guidance.
///
/// This is the single, typed gate every CLI handler that creates `.workhive/`,
/// materializes worktrees, or writes runtime state must funnel through.
pub(crate) fn enforce_trusted_root_or_error(
    project_root: &camino::Utf8Path,
    ctx: &CommandContext,
    caller: AdmissionCaller,
) -> Result<(), CliError> {
    enforce_trusted_root_with_input(
        project_root,
        ctx,
        caller,
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
        &TtyAdmissionInput,
        &super::roots_cmd::TtyRootsEditorInput,
    )
}

pub(crate) fn enforce_trusted_root_with_input(
    project_root: &camino::Utf8Path,
    ctx: &CommandContext,
    caller: AdmissionCaller,
    interactive: bool,
    admission_input: &dyn AdmissionInput,
    editor_input: &dyn super::roots_cmd::RootsEditorInput,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::{
        admit_mutation, classify_overly_broad_root, trusted_roots_from_config,
    };
    let locale = ctx.locale();
    let global_cfg = ctx.config.load_global()?;
    let home = home_path();
    let trusted = trusted_roots_from_config(global_cfg.as_ref(), home.as_deref());
    let decision = admit_mutation(&trusted, project_root);
    let overly_broad = classify_overly_broad_root(project_root, home.as_deref()).is_some();
    match resolve_trust_gate_action(&decision, interactive, overly_broad) {
        TrustGateAction::Proceed => Ok(()),
        TrustGateAction::Reject => Err(trusted_root_rejection_error(
            &decision,
            global_cfg.as_ref(),
            ctx,
        )),
        TrustGateAction::OfferTrustPrompt => {
            let prompt = ctx.i18n.resolve(locale, k::ADMISSION_MENU_PROMPT);
            let labels = admission_menu_labels(&ctx.i18n, locale);
            let default_idx = admission_menu_default_index(caller);

            loop {
                let selection = match admission_input.select(&prompt, &labels, default_idx) {
                    Ok(idx) => idx,
                    Err(CliError::UserAborted) => {
                        return Err(trusted_root_rejection_error(
                            &decision,
                            global_cfg.as_ref(),
                            ctx,
                        ));
                    }
                    Err(e) => return Err(e),
                };
                let action = admission_action_from_index(selection);
                match action {
                    AdmissionAction::TrustProject => {
                        persist_trusted_root(project_root, global_cfg.as_ref(), ctx)?;
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_CONFIRM_ADDED,
                                &[project_root.as_str()],
                            )
                        );
                        return Ok(());
                    }
                    AdmissionAction::ManageTrustedDirectories => {
                        // The editor returns a typed outcome; only Saved may emit post-save re-admission messaging.
                        let outcome =
                            super::roots_cmd::cmd_project_roots_edit_with_input(ctx, editor_input)?;
                        match outcome {
                            super::roots::EditorOutcome::Saved { .. } => {
                                let refreshed_cfg = ctx.config.load_global()?;
                                let refreshed_roots: Vec<camino::Utf8PathBuf> = refreshed_cfg
                                    .as_ref()
                                    .map(|c| c.trusted_roots())
                                    .unwrap_or_default();
                                if is_project_admitted_after_editor(
                                    project_root,
                                    &refreshed_roots,
                                    home.as_deref(),
                                ) {
                                    return Ok(());
                                }
                                let msg = ctx.i18n.resolve(locale, k::ADMISSION_STILL_OUTSIDE);
                                eprintln!("{msg}");
                                // Loop back to show the menu again.
                            }
                            super::roots::EditorOutcome::Unchanged
                            | super::roots::EditorOutcome::Cancelled => {
                                // No changes saved; loop back to show the menu again.
                            }
                        }
                    }
                    AdmissionAction::Cancel => {
                        return Err(trusted_root_rejection_error(
                            &decision,
                            global_cfg.as_ref(),
                            ctx,
                        ));
                    }
                }
            }
        }
    }
}

/// Localized rejection error for an unadmitted project root. Shared by the
/// non-interactive path and the declined-prompt path so both surface the
/// same actionable guidance.
fn trusted_root_rejection_error(
    decision: &workhive_core::domain::TrustedRootDecision,
    global_cfg: Option<&workhive_core::ports::AppConfig>,
    ctx: &CommandContext,
) -> CliError {
    use workhive_core::domain::TrustedRootDecision;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::AppConfig;
    let locale = ctx.locale();
    match decision {
        TrustedRootDecision::Admitted { .. } => {
            // Unreachable by construction: callers only render rejections for
            // unadmitted decisions. Kept total to avoid a panic path.
            CliError::Runtime(ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_NONE_CONFIGURED))
        }
        TrustedRootDecision::NoTrustedRootsConfigured => {
            CliError::Runtime(ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_NONE_CONFIGURED))
        }
        TrustedRootDecision::OutsideTrustedRoots { project_root } => {
            // Authored trusted roots (e.g. `~/Jts/`) are surfaced verbatim so
            // the user sees the same shape they configured. The list is
            // rendered through a dedicated i18n bullet item so the bullet
            // style is centralized.
            let authored: Vec<camino::Utf8PathBuf> =
                global_cfg.map(AppConfig::trusted_roots).unwrap_or_default();
            let rendered_list = render_trusted_roots_list(ctx, locale, &authored);
            CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_OUTSIDE,
                &[project_root.as_str(), &rendered_list],
            ))
        }
    }
}

/// Render the configured trusted roots as a multi-line bullet list for
/// inclusion inside the `TRUSTED_ROOTS_OUTSIDE` rejection copy. When the
/// list is empty the helper falls back to the localized "(none configured)"
/// placeholder so the rejection bullet never collapses to a confusing blank
/// line.
fn render_trusted_roots_list(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    authored: &[camino::Utf8PathBuf],
) -> String {
    use workhive_core::i18n_keys as k;
    if authored.is_empty() {
        return ctx
            .i18n
            .resolve(locale, k::TRUSTED_ROOTS_OUTSIDE_EMPTY_LIST);
    }
    authored
        .iter()
        .map(|p| {
            ctx.i18n
                .resolve_with_args(locale, k::TRUSTED_ROOTS_OUTSIDE_LINE_ITEM, &[p.as_str()])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the contextual hint for a discovery setup state.
///
/// Centralized so `session list`, `project list`, and the wizard speak the
/// same language about the trusted-roots → index → projects funnel.
pub(crate) fn print_setup_state_hint(
    state: &workhive_core::usecases::trusted_roots::DiscoverySetupState,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) {
    if let Some(message) = setup_state_hint(state, ctx, locale) {
        eprintln!("{message}");
    }
}

pub(crate) fn setup_state_hint(
    state: &workhive_core::usecases::trusted_roots::DiscoverySetupState,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> Option<String> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;
    let key = match state {
        DiscoverySetupState::NoTrustedRootsConfigured => k::TRUSTED_ROOTS_NONE_CONFIGURED,
        DiscoverySetupState::TrustedRootsConfiguredIndexMissing => k::TRUSTED_ROOTS_INDEX_MISSING,
        DiscoverySetupState::TrustedRootsConfiguredIndexEmpty => k::TRUSTED_ROOTS_INDEX_EMPTY,
        DiscoverySetupState::Ready => return None,
    };
    Some(ctx.i18n.resolve(locale, key))
}

/// Load the global config for read-only setup-hint surfaces.
///
/// Returns `Some(cfg)` on success, `None` when no config exists, and
/// on a real load/parse error surfaces a localized diagnostic line
/// rather than returning `None` (which would mislabel the state).
///
/// This helper returns:
/// - `Ok(Some(cfg))` when the file parses successfully;
/// - `Ok(None)` when no global config exists yet (first-time install);
/// - and on a real load/parse error, surfaces a localized
///   `TRUSTED_ROOTS_CONFIG_UNREADABLE` line, emits a tracing breadcrumb, and
///   returns `Ok(None)` so the rendering path can degrade explicitly without
///   pretending the config was empty.
///
/// Mutating/admission gates must NOT use this helper — they must propagate
/// the error directly via `ctx.config.load_global()?`.
pub(crate) fn load_global_for_hint(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> Option<workhive_core::ports::AppConfig> {
    use workhive_core::i18n_keys as k;
    match ctx.config.load_global() {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "Global config unreadable; trusted-roots hint surface degraded.",
            );
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::TRUSTED_ROOTS_CONFIG_UNREADABLE,
                    &[&err.to_string()],
                )
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

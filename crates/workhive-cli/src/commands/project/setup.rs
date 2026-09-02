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

    let ctx = usecases::naming::assemble_naming_context(
        project_name,
        workspace,
        project_root,
        branch,
        instance_id.clone(),
        cwd.as_deref(),
    );
    tracing::debug!(
        project_name,
        project_with_subdir = %ctx.project,
        ?instance_id,
        "naming context built"
    );
    ctx
}

pub(crate) fn build_naming_context_with_cwd(
    project_name: &str,
    workspace: &str,
    project_root: &Utf8PathBuf,
    git: &dyn GitPort,
    ws_fs: &dyn WorkspaceFsPort,
    cwd: Option<&Utf8Path>,
) -> NamingContext {
    let branch = git.current_branch().ok();
    let instance_id = ws_fs.read_instance_id(project_root.as_ref());

    let ctx = usecases::naming::assemble_naming_context(
        project_name,
        workspace,
        project_root,
        branch,
        instance_id.clone(),
        cwd,
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
    use workhive_core::i18n_keys as k;
    for entry in stale {
        let i18n_key = match entry.reason {
            usecases::resolve_projects::StaleReason::InstanceIdMissing => k::INDEX_STALE_ENTRY,
            usecases::resolve_projects::StaleReason::ProjectRootGone => k::INDEX_STALE_ENTRY,
        };
        let msg = ctx
            .i18n
            .resolve_with_args(locale, i18n_key, &[&entry.summary.project_name]);
        tracing::warn!(
            project = %entry.summary.project_name,
            root = %entry.summary.project_root,
            reason = ?entry.reason,
            "stale index entry"
        );
        eprintln!("{msg}");
    }
}

// ── Trusted-roots / setup-state helpers ──

/// Resolve the user's home directory as a `Utf8PathBuf` if available.
pub(crate) fn home_path() -> Option<camino::Utf8PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(camino::Utf8PathBuf::from)
}

/// Offer to build the initial project index right after a successful global
/// `workhive init` that configured trusted roots.
///
/// Honors interactive vs. non-interactive stdin: in non-interactive mode the
/// reindex is performed automatically (so scripted setups stay closed-loop),
/// in interactive mode the user is prompted with a Y/N confirm.
pub(crate) fn offer_initial_reindex(
    roots: &[camino::Utf8PathBuf],
    ctx: &CommandContext,
    locale_override: Option<workhive_core::domain::Locale>,
) -> Result<(), CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;
    // After interactive global init chooses a locale, downstream
    // messages in the same flow use that effective locale rather than
    // the pre-wizard default from `ctx.locale()`.
    let locale = locale_override.unwrap_or_else(|| ctx.locale());

    let do_index = if std::io::stdin().is_terminal() {
        let prompt = ctx
            .i18n
            .resolve(locale, k::INIT_TRUSTED_ROOTS_REINDEX_PROMPT);
        crate::commands::confirm_with_transcript(ctx, locale, &prompt, true)?
    } else {
        true
    };

    if !do_index {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve(locale, k::INIT_TRUSTED_ROOTS_REINDEX_SKIPPED)
        );
        return Ok(());
    }

    // Configured trusted roots may legitimately be authored as `~/...`
    // (portable across machines). Discovery checks filesystem existence
    // and therefore MUST receive expanded paths. Expansion is lexical
    // only; the authored value in global TOML is never rewritten.
    let home = home_path();
    let expanded_roots = workhive_core::usecases::trusted_roots::expand_trusted_roots_for_discovery(
        roots,
        home.as_deref(),
    );

    let index = ctx.composition.project_index()?;
    let discovery = ctx.composition.project_discovery();
    let result = workhive_core::usecases::reindex::rebuild_project_index(
        &discovery,
        &index,
        &expanded_roots,
        true,
    )
    .map_err(|e| localize_reindex_error(ctx, locale, &e, roots, &expanded_roots))?;
    let count = result.discovered_count.to_string();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::INIT_TRUSTED_ROOTS_REINDEX_DONE, &[&count])
    );
    emit_reindex_warnings(ctx, locale, &result.warnings);
    Ok(())
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

/// Enforce the trusted-roots admission policy for a mutating project flow.
///
/// Returns `Ok(())` when the project root is admitted (or trusted roots are
/// configured but the root is admitted). Returns a localized `CliError::Runtime`
/// when the root is outside the trusted set, or when no trusted roots are
/// configured at all.
///
/// This is the single, typed gate every CLI handler that creates `.workhive/`,
/// materializes worktrees, or writes runtime state must funnel through.
pub(crate) fn enforce_trusted_root_or_error(
    project_root: &camino::Utf8Path,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::domain::TrustedRootDecision;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::AppConfig;
    use workhive_core::usecases::trusted_roots::{admit_mutation, trusted_roots_from_config};
    let locale = ctx.locale();
    // This is a mutation-admission gate. Silently
    // converting a config read/parse error into "no trusted roots
    // configured" would weaken the safety boundary and silently fall
    // back on a config read path. Propagate the error
    // so the user sees the underlying failure instead of a misleading
    // "outside trusted roots" / "no trusted roots configured" message.
    let global_cfg = ctx.config.load_global()?;
    let trusted = trusted_roots_from_config(global_cfg.as_ref(), home_path().as_deref());
    match admit_mutation(&trusted, project_root) {
        TrustedRootDecision::Admitted { .. } => Ok(()),
        TrustedRootDecision::NoTrustedRootsConfigured => Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_NONE_CONFIGURED),
        )),
        TrustedRootDecision::OutsideTrustedRoots { project_root } => {
            // Authored trusted roots (e.g. `~/Jts/`) are surfaced verbatim so
            // the user sees the same shape they configured. The list is
            // rendered through a dedicated i18n bullet item so the bullet
            // style is centralized.
            let authored: Vec<camino::Utf8PathBuf> = global_cfg
                .as_ref()
                .map(AppConfig::trusted_roots)
                .unwrap_or_default();
            let rendered_list = render_trusted_roots_list(ctx, locale, &authored);
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_OUTSIDE,
                &[project_root.as_str(), &rendered_list],
            )))
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
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;
    let key = match state {
        DiscoverySetupState::NoTrustedRootsConfigured => k::TRUSTED_ROOTS_NONE_CONFIGURED,
        DiscoverySetupState::TrustedRootsConfiguredIndexMissing => k::TRUSTED_ROOTS_INDEX_MISSING,
        DiscoverySetupState::TrustedRootsConfiguredIndexEmpty => k::TRUSTED_ROOTS_INDEX_EMPTY,
        DiscoverySetupState::Ready => return,
    };
    eprintln!("{}", ctx.i18n.resolve(locale, key));
}

/// Load the global config for read-only setup-hint surfaces.
///
/// Read-only commands (`session list`, `project list`, `doctor`) must not
/// call `ctx.config.load_global().ok().flatten()`, which silently mislabels
/// a malformed/unreadable global config as "no trusted roots configured".
/// That violates the rule "do not lie to the user about the configured
/// state".
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
mod tests {
    use super::*;

    #[test]
    fn infers_project_root_from_managed_worktree_root_path() {
        let inferred = infer_project_root_from_managed_worktree_path(Utf8Path::new(
            "/repo/.workhive/worktrees/feat-new",
        ));

        assert_eq!(inferred, Some(Utf8PathBuf::from("/repo")));
    }

    #[test]
    fn infers_project_root_from_managed_worktree_subdir_path() {
        let inferred = infer_project_root_from_managed_worktree_path(Utf8Path::new(
            "/repo/.workhive/worktrees/feat-new/src/module",
        ));

        assert_eq!(inferred, Some(Utf8PathBuf::from("/repo")));
    }

    #[test]
    fn does_not_infer_project_root_from_unmanaged_path() {
        let inferred = infer_project_root_from_managed_worktree_path(Utf8Path::new(
            "/repo/worktrees/feat-new",
        ));

        assert_eq!(inferred, None);
    }
}

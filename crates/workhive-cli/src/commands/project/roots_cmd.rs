#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

/// Injectable input boundary for the roots editor interactive loop.
/// Production delegates to TUI helpers; tests supply deterministic sequences.
pub(crate) trait RootsEditorInput {
    fn select(&self, prompt: &str, items: &[String], default: usize) -> Result<usize, CliError>;
    fn text_input(&self, prompt: &str, initial: &str) -> Result<String, CliError>;
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool, CliError>;
}

/// Production implementation using real TUI interactions.
pub(crate) struct TtyRootsEditorInput;

impl RootsEditorInput for TtyRootsEditorInput {
    fn select(&self, prompt: &str, items: &[String], default: usize) -> Result<usize, CliError> {
        crate::ui::screens::select::run(prompt, items, default)
    }

    fn text_input(&self, prompt: &str, initial: &str) -> Result<String, CliError> {
        crate::ui::screens::text_input::run(prompt, initial)
    }

    fn confirm(&self, prompt: &str, default: bool) -> Result<bool, CliError> {
        crate::ui::screens::confirm::run_with_context(prompt, None, default)
    }
}

pub(crate) fn cmd_project_roots_list(ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();
    let cfg = load_global_or_default(ctx)?;
    let roots = cfg.trusted_roots();
    if roots.is_empty() {
        eprintln!("{}", ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_EMPTY));
        return Ok(());
    }
    eprintln!("{}", ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_HEADER));
    // User-configured
    // path values must round-trip verbatim. We never funnel them through
    // `display_path_for_humans` here — `~/dev` stays `~/dev`, and an explicit
    // `/Users/<name>/dev` stays absolute. Home abbreviation is reserved for
    // *system-derived* paths (config file location, log files, project list
    // entries, etc.).
    for entry in &roots {
        println!("  - {entry}");
    }
    Ok(())
}

pub(crate) fn cmd_project_roots_add(
    path: &str,
    force: bool,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::{TrustedRootMutation, add_trusted_root};
    let locale = ctx.locale();
    let candidate = camino::Utf8PathBuf::from(path);
    let home = home_path();

    if !force && let Some(broad) = classify_overly_broad_root(candidate.as_ref(), home.as_deref()) {
        return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::TRUSTED_ROOTS_REJECTED_TOO_BROAD,
            &[broad.as_str()],
        )));
    }

    // Validate existence + directory before any
    // mutation. `validate_trusted_root_path` expands `~` first, then uses
    // `std::fs` checks — neither i18n keys nor heuristics cover this gap.
    validate_trusted_root_path(candidate.as_ref(), force, ctx)?;

    let cfg = load_global_or_default(ctx)?;
    let existing = cfg.trusted_roots();
    let (next, mutation) = add_trusted_root(&existing, candidate.as_ref(), home.as_deref());

    match mutation {
        TrustedRootMutation::Added { canonical } => {
            persist_trusted_roots_and_schedule(&next, ctx)?;
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve_with_args(locale, k::TRUSTED_ROOTS_ADDED, &[canonical.as_str()])
            );
        }
        TrustedRootMutation::AlreadyPresent { canonical } => {
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::TRUSTED_ROOTS_ALREADY_PRESENT,
                    &[canonical.as_str()],
                )
            );
        }
        TrustedRootMutation::CoveredByExistingRoot {
            canonical,
            covering_root,
        } => {
            return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::TRUSTED_ROOTS_COVERED_BY_EXISTING_ROOT,
                &[canonical.as_str(), covering_root.as_str()],
            )));
        }
        TrustedRootMutation::Removed { .. } | TrustedRootMutation::NotPresent { .. } => {
            unreachable!("add_trusted_root never produces removal mutations")
        }
    }
    Ok(())
}

pub(crate) fn cmd_project_roots_remove(
    path: &str,
    force: bool,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let index = ctx.composition.project_index()?;
    cmd_project_roots_remove_with_index(path, force, ctx, &index)
}

pub(crate) fn cmd_project_roots_remove_with_index(
    path: &str,
    force: bool,
    ctx: &CommandContext,
    index: &dyn workhive_core::ports::ProjectIndexPort,
) -> Result<(), CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::{TrustedRootMutation, remove_trusted_root};
    let locale = ctx.locale();
    let candidate = camino::Utf8PathBuf::from(path);
    let home = home_path();

    let cfg = load_global_or_default(ctx)?;
    let existing = cfg.trusted_roots();
    let (next, mutation) = remove_trusted_root(&existing, candidate.as_ref(), home.as_deref());

    match mutation {
        TrustedRootMutation::Removed { canonical } => {
            let projects = load_indexed_projects_for_root_removal(index)?;
            let affected = count_affected_projects_under_root(canonical.as_path(), &projects);
            if affected > 0 && !force {
                let count_str = affected.to_string();
                if std::io::stdin().is_terminal() {
                    let prompt = ctx.i18n.resolve_with_args(
                        locale,
                        k::TRUSTED_ROOTS_REMOVE_AFFECTED_PROMPT,
                        &[canonical.as_str(), &count_str],
                    );
                    let confirmed =
                        crate::commands::confirm_with_transcript(ctx, locale, &prompt, false)?;
                    if !confirmed {
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_REMOVE_ABORTED)
                        );
                        return Ok(());
                    }
                } else {
                    return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                        locale,
                        k::TRUSTED_ROOTS_REMOVE_AFFECTED_REQUIRES_FORCE,
                        &[canonical.as_str(), &count_str],
                    )));
                }
            }

            persist_trusted_roots_and_schedule(&next, ctx)?;
            eprintln!(
                "{}",
                ctx.i18n
                    .resolve_with_args(locale, k::TRUSTED_ROOTS_REMOVED, &[canonical.as_str()])
            );
        }
        TrustedRootMutation::NotPresent { canonical } => {
            eprintln!(
                "{}",
                ctx.i18n.resolve_with_args(
                    locale,
                    k::TRUSTED_ROOTS_NOT_PRESENT,
                    &[canonical.as_str()],
                )
            );
        }
        TrustedRootMutation::Added { .. }
        | TrustedRootMutation::AlreadyPresent { .. }
        | TrustedRootMutation::CoveredByExistingRoot { .. } => {
            unreachable!("remove_trusted_root never produces add mutations")
        }
    }
    Ok(())
}

/// Interactive `workhive project roots edit`.
///
/// The command must support: viewing the current
/// trusted roots, adding entries, removing entries, and saving / cancelling.
/// All mutations are staged in memory; only on `Save` do we write the
/// document back via `toml_edit` (preserving comments). On `Cancel` (or
/// Ctrl+C) the file is left untouched.
pub(crate) fn cmd_project_roots_edit(
    ctx: &CommandContext,
) -> Result<super::roots::EditorOutcome, CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;

    if !std::io::stdin().is_terminal() {
        let locale = ctx.locale();
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_NO_TTY),
        ));
    }

    cmd_project_roots_edit_with_input(ctx, &TtyRootsEditorInput)
}

/// Core editor logic using an injectable input boundary.
pub(crate) fn cmd_project_roots_edit_with_input(
    ctx: &CommandContext,
    input: &dyn RootsEditorInput,
) -> Result<super::roots::EditorOutcome, CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::{
        TrustedRootMutation, add_trusted_root, remove_trusted_root,
    };
    let locale = ctx.locale();

    let cfg = load_global_or_default(ctx)?;
    // Snapshot the trusted-root list at the start of the edit session so
    // Save can compute all removals from the staged working list and run
    // one consolidated affected-project guard.
    let original: Vec<camino::Utf8PathBuf> = cfg.trusted_roots();
    let mut working: Vec<camino::Utf8PathBuf> = original.clone();
    let mut dirty = false;
    let home = home_path();

    eprintln!("{}", ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_HEADER));

    // Retry state for the Add path prompt. A rejected path is preserved
    // as the next add prompt's initial buffer so the user can edit or
    // Tab-complete it instead of retyping; it is cleared once a path is
    // accepted, the field is cancelled, or the entry is left empty.
    let mut add_retry = ui::validated_input::ValidatedInputRetry::new();

    loop {
        // Show the current staged list each iteration so the user always
        // knows exactly what Save would persist.
        if working.is_empty() {
            eprintln!(
                "  ({})",
                ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_EMPTY)
            );
        } else {
            eprintln!("{}", ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_LIST_HEADER));
            for entry in &working {
                eprintln!("  - {entry}");
            }
        }

        let menu_items = vec![
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_ADD),
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_REMOVE),
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_SAVE),
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_CANCEL),
        ];
        let selection = match input.select(
            &ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_PROMPT),
            &menu_items,
            0,
        ) {
            Ok(idx) => idx,
            Err(CliError::UserAborted) => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_CANCELLED)
                );
                return Ok(super::roots::EditorOutcome::Cancelled);
            }
            Err(e) => return Err(e),
        };
        if let Some(item) = menu_items.get(selection) {
            crate::commands::print_choice_transcript(
                &ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_MENU_PROMPT),
                item,
            );
        }

        match selection {
            0 => {
                // Add: ask for path, validate, run trusted-root mutation,
                // surface the appropriate user-facing message but stay in
                // the loop until the user explicitly Saves or Cancels.
                let raw = match input.text_input(
                    &ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_ADD_PROMPT),
                    add_retry.initial(),
                ) {
                    Ok(s) => s,
                    Err(CliError::UserAborted) => {
                        add_retry.clear();
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    add_retry.clear();
                    continue;
                }
                let candidate = camino::Utf8PathBuf::from(trimmed);
                if let Some(broad) = classify_overly_broad_root(candidate.as_ref(), home.as_deref())
                {
                    let msg = ctx.i18n.resolve_with_args(
                        locale,
                        k::TRUSTED_ROOTS_REJECTED_TOO_BROAD,
                        &[broad.as_str()],
                    );
                    eprintln!("{msg}");
                    add_retry.reject(trimmed);
                    continue;
                }
                if let Err(e) = validate_trusted_root_path(candidate.as_ref(), false, ctx) {
                    let msg = e.to_string();
                    eprintln!("{msg}");
                    add_retry.reject(trimmed);
                    continue;
                }
                let (next, mutation) =
                    add_trusted_root(&working, candidate.as_ref(), home.as_deref());
                match mutation {
                    TrustedRootMutation::Added { canonical } => {
                        working = next;
                        dirty = true;
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_ADDED,
                                &[canonical.as_str()],
                            )
                        );
                    }
                    TrustedRootMutation::AlreadyPresent { canonical } => {
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_ALREADY_PRESENT,
                                &[canonical.as_str()],
                            )
                        );
                    }
                    TrustedRootMutation::CoveredByExistingRoot {
                        canonical,
                        covering_root,
                    } => {
                        // The candidate is a strict descendant of an
                        // already-staged trusted root. Stay in the edit
                        // loop without mutating `working`; the user can
                        // remove the covering parent first if they
                        // really want to trust only the child.
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_COVERED_BY_EXISTING_ROOT,
                                &[canonical.as_str(), covering_root.as_str()],
                            )
                        );
                    }
                    _ => unreachable!("add_trusted_root never produces removal mutations"),
                }
                // The candidate passed shape/scope validation, so any
                // preserved rejected value is stale: clear it so the next
                // add prompt starts empty.
                add_retry.clear();
            }
            1 => {
                // Remove: pick from the current staged list. The
                // affected-project guard is applied at Save time on the
                // aggregated removal set, so the user can stage several
                // removals and see one consolidated warning.
                if working.is_empty() {
                    eprintln!(
                        "{}",
                        ctx.i18n
                            .resolve(locale, k::TRUSTED_ROOTS_EDIT_NOTHING_TO_REMOVE)
                    );
                    continue;
                }
                let items: Vec<String> = working.iter().map(|p| p.as_str().to_owned()).collect();
                let idx = match input.select(
                    &ctx.i18n
                        .resolve(locale, k::TRUSTED_ROOTS_EDIT_REMOVE_PROMPT),
                    &items,
                    0,
                ) {
                    Ok(i) => i,
                    Err(CliError::UserAborted) => continue,
                    Err(e) => return Err(e),
                };
                if let Some(item) = items.get(idx) {
                    crate::commands::print_choice_transcript(
                        &ctx.i18n
                            .resolve(locale, k::TRUSTED_ROOTS_EDIT_REMOVE_PROMPT),
                        item,
                    );
                }
                let target = working[idx].clone();
                let (next, mutation) =
                    remove_trusted_root(&working, target.as_ref(), home.as_deref());
                match mutation {
                    TrustedRootMutation::Removed { canonical } => {
                        working = next;
                        dirty = true;
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_REMOVED,
                                &[canonical.as_str()],
                            )
                        );
                    }
                    TrustedRootMutation::NotPresent { canonical } => {
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_NOT_PRESENT,
                                &[canonical.as_str()],
                            )
                        );
                    }
                    _ => unreachable!("remove_trusted_root never produces add mutations"),
                }
            }
            2 => {
                // Resolve the typed editor outcome from the loop state.
                let outcome =
                    resolve_editor_outcome(&original, &working, dirty, EditorUserAction::Save);
                if !outcome.is_changed_save() {
                    eprintln!(
                        "{}",
                        ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_NO_CHANGES)
                    );
                    return Ok(super::roots::EditorOutcome::Unchanged);
                }

                let index = ctx.composition.project_index()?;
                let summaries = validate_roots_edit_save(&original, &working, &index)?;
                if !summaries.is_empty() {
                    let total: usize = summaries.iter().map(|s| s.count).sum();
                    let total_str = total.to_string();
                    eprintln!(
                        "{}",
                        ctx.i18n.resolve_with_args(
                            locale,
                            k::TRUSTED_ROOTS_EDIT_AFFECTED_HEADER,
                            &[&total_str],
                        )
                    );
                    for s in &summaries {
                        let cnt = s.count.to_string();
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve_with_args(
                                locale,
                                k::TRUSTED_ROOTS_EDIT_AFFECTED_LINE,
                                &[s.canonical_root.as_str(), &cnt],
                            )
                        );
                    }

                    let prompt = ctx
                        .i18n
                        .resolve(locale, k::TRUSTED_ROOTS_EDIT_AFFECTED_PROMPT);
                    let confirmed = input.confirm(&prompt, false)?;
                    if !confirmed {
                        eprintln!(
                            "{}",
                            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_CANCELLED)
                        );
                        return Ok(super::roots::EditorOutcome::Cancelled);
                    }
                }

                persist_trusted_roots_and_schedule(&working, ctx)?;
                let count = working.len().to_string();
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::TRUSTED_ROOTS_EDIT_SAVED, &[&count])
                );
                return Ok(super::roots::EditorOutcome::Saved {
                    before: original,
                    after: working,
                });
            }
            _ => {
                // Cancel action via the typed outcome.
                let _outcome =
                    resolve_editor_outcome(&original, &working, dirty, EditorUserAction::Cancel);
                eprintln!(
                    "{}",
                    ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_CANCELLED)
                );
                return Ok(super::roots::EditorOutcome::Cancelled);
            }
        }
    }
}

pub(crate) fn cmd_project_roots_status(ctx: &CommandContext) -> Result<(), CliError> {
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    cmd_project_roots_status_with_writer(ctx, &mut output)
}

pub(crate) fn cmd_project_roots_status_with_writer(
    ctx: &CommandContext,
    output: &mut dyn std::io::Write,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::IndexState;
    use workhive_core::usecases::trusted_roots::{
        SetupStateProbe, resolve_setup_state, trusted_roots_from_config,
    };
    let locale = ctx.locale();
    let cfg = load_global_or_default(ctx)?;
    // The status block is labeled "configured trusted roots" — render the
    // raw configured values verbatim. The resolved
    // `TrustedRootSet` is still used below for setup-state classification
    // (admission semantics), but it must not leak normalized/expanded
    // paths back into the user-configured listing.
    let configured_roots = cfg.trusted_roots();
    let trusted_set = trusted_roots_from_config(Some(&cfg), home_path().as_deref());
    let count = configured_roots.len().to_string();
    writeln!(
        output,
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_CONFIGURED, &[&count])
    )
    .map_err(|e| CliError::Runtime(e.to_string()))?;
    if !configured_roots.is_empty() {
        for entry in &configured_roots {
            writeln!(output, "  - {entry}").map_err(|e| CliError::Runtime(e.to_string()))?;
        }
    }

    let index = ctx.composition.project_index()?;
    let state = index.list_projects()?;
    let (index_present, project_count) = match &state {
        IndexState::NotFound => (false, 0usize),
        IndexState::Loaded(v) => (true, v.len()),
    };
    let index_label = if index_present {
        ctx.i18n.resolve(locale, k::PROJECT_INDEX_STATE_PRESENT)
    } else {
        ctx.i18n.resolve(locale, k::PROJECT_INDEX_STATE_MISSING)
    };
    writeln!(
        output,
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_INDEX, &[&index_label])
    )
    .map_err(|e| CliError::Runtime(e.to_string()))?;
    let project_count_str = project_count.to_string();
    writeln!(
        output,
        "{}",
        ctx.i18n.resolve_with_args(
            locale,
            k::TRUSTED_ROOTS_STATUS_PROJECTS,
            &[&project_count_str],
        )
    )
    .map_err(|e| CliError::Runtime(e.to_string()))?;

    let setup = resolve_setup_state(SetupStateProbe {
        trusted_roots_configured: !trusted_set.is_empty(),
        index_present,
        indexed_project_count: project_count,
    });
    if let Some(setup_hint) = setup_state_hint(&setup, ctx, locale) {
        writeln!(output, "{setup_hint}").map_err(|e| CliError::Runtime(e.to_string()))?;
    }

    // Reconciliation lifecycle status.
    let recon_status = match ctx.config.global_config_dir() {
        Ok(dir) => {
            let reconciliation = ctx.composition.index_reconciliation();
            let recon_state = workhive_core::ports::IndexReconciliationPort::read_state(
                &reconciliation,
                dir.as_ref(),
            );
            render_reconciliation_status(&recon_state, &ctx.i18n, locale)
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
    writeln!(output, "  {}", recon_status.0).map_err(|e| CliError::Runtime(e.to_string()))?;
    if recon_status.1 {
        writeln!(
            output,
            "  {}",
            ctx.i18n.resolve(locale, k::RECONCILIATION_RECOVERY_HINT)
        )
        .map_err(|e| CliError::Runtime(e.to_string()))?;
    }

    Ok(())
}

/// Render reconciliation lifecycle status from a state result.
///
/// Returns (rendered message, whether recovery hint should be shown).
/// Absence (idle) is only for a valid empty state file or first run.
/// State-read errors render as failures with a recovery hint.
pub(crate) fn render_reconciliation_status(
    state_result: &Result<
        workhive_core::domain::index_reconciliation::ReconciliationState,
        workhive_core::errors::IndexError,
    >,
    i18n: &workhive_infra::i18n::StaticI18n,
    locale: workhive_core::domain::Locale,
) -> (String, bool) {
    use workhive_core::domain::index_reconciliation::ReconciliationState;
    use workhive_core::i18n_keys as k;

    let (msg, is_failed) = match state_result {
        Ok(ReconciliationState::Idle) => {
            (i18n.resolve(locale, k::RECONCILIATION_STATUS_IDLE), false)
        }
        Ok(ReconciliationState::Updating { revision, .. }) => {
            let rev = revision.value().to_string();
            (
                i18n.resolve_with_args(locale, k::RECONCILIATION_STATUS_UPDATING, &[&rev]),
                false,
            )
        }
        Ok(ReconciliationState::Completed {
            revision,
            discovered_count,
            ..
        }) => {
            let rev = revision.value().to_string();
            let cnt = discovered_count.to_string();
            (
                i18n.resolve_with_args(locale, k::RECONCILIATION_STATUS_COMPLETED, &[&rev, &cnt]),
                false,
            )
        }
        Ok(ReconciliationState::Failed {
            revision,
            error_detail,
            ..
        }) => {
            let rev = revision.value().to_string();
            (
                i18n.resolve_with_args(
                    locale,
                    k::RECONCILIATION_STATUS_FAILED,
                    &[&rev, error_detail.as_str()],
                ),
                true,
            )
        }
        Err(e) => {
            let detail = e.to_string();
            (
                i18n.resolve_with_args(locale, k::RECONCILIATION_STATUS_FAILED, &["0", &detail]),
                true,
            )
        }
    };
    (msg, is_failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::Locale;
    use workhive_core::domain::index_reconciliation::{
        ReconciliationRevision, ReconciliationState, RootsFingerprint,
    };
    use workhive_core::errors::IndexError;
    use workhive_infra::i18n::StaticI18n;

    fn make_i18n() -> StaticI18n {
        StaticI18n::new()
    }

    fn sample_fingerprint() -> RootsFingerprint {
        RootsFingerprint::from_trusted_roots(&workhive_core::domain::TrustedRootSet::from_raw(
            &[camino::Utf8PathBuf::from("/dev/projects")],
            None,
        ))
    }

    // ── trusted_roots_status_surface_ — lifecycle and recovery UX ─────

    #[test]
    fn trusted_roots_status_surface_idle_en() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Idle);
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::En);
        assert!(
            !msg.is_empty(),
            "idle status must produce non-empty message"
        );
        assert!(!show_recovery, "idle must not show recovery hint");
    }

    #[test]
    fn trusted_roots_status_surface_idle_zh_cn() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Idle);
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::ZhCn);
        assert!(
            !msg.is_empty(),
            "idle status must produce non-empty message in zh-CN"
        );
        assert!(!show_recovery);
    }

    #[test]
    fn trusted_roots_status_surface_updating_en() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Updating {
            revision: ReconciliationRevision::new(5),
            fingerprint: sample_fingerprint(),
            started_at: std::time::SystemTime::now(),
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::En);
        assert!(
            msg.contains("5"),
            "updating status must contain revision number"
        );
        assert!(!show_recovery, "updating must not show recovery hint");
    }

    #[test]
    fn trusted_roots_status_surface_completed_en() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Completed {
            revision: ReconciliationRevision::new(3),
            fingerprint: sample_fingerprint(),
            completed_at: std::time::SystemTime::now(),
            discovered_count: 7,
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::En);
        assert!(msg.contains("3"), "completed must contain revision");
        assert!(msg.contains("7"), "completed must contain discovered count");
        assert!(!show_recovery);
    }

    #[test]
    fn trusted_roots_status_surface_failed_en_shows_recovery() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Failed {
            revision: ReconciliationRevision::new(2),
            fingerprint: sample_fingerprint(),
            failed_at: std::time::SystemTime::now(),
            error_detail: "scan timeout".to_owned(),
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::En);
        assert!(msg.contains("2"), "failed must contain revision");
        assert!(
            msg.contains("scan timeout"),
            "failed must contain error detail"
        );
        assert!(show_recovery, "failed state MUST show recovery hint");
    }

    #[test]
    fn trusted_roots_status_surface_failed_zh_cn_shows_recovery() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Ok(ReconciliationState::Failed {
            revision: ReconciliationRevision::new(4),
            fingerprint: sample_fingerprint(),
            failed_at: std::time::SystemTime::now(),
            error_detail: "permission denied".to_owned(),
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::ZhCn);
        assert!(!msg.is_empty());
        assert!(msg.contains("4"), "zh-CN failed must contain revision");
        assert!(show_recovery, "zh-CN failed state must show recovery hint");
    }

    #[test]
    fn trusted_roots_status_surface_state_read_error_shows_failure_and_recovery() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Err(IndexError::ReadFailed {
            path: camino::Utf8PathBuf::from("/tmp/state.json"),
            source: std::io::Error::other("gone"),
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::En);
        assert!(!msg.is_empty(), "state read error must produce output");
        assert!(show_recovery, "state read error must show recovery hint");
        assert!(
            msg.contains("/tmp/state.json"),
            "state read error must include path detail: {msg}"
        );
    }

    #[test]
    fn trusted_roots_status_surface_state_read_error_zh_cn_shows_recovery() {
        let i18n = make_i18n();
        let state: Result<ReconciliationState, IndexError> = Err(IndexError::ReadFailed {
            path: camino::Utf8PathBuf::from("/tmp/state.json"),
            source: std::io::Error::other("disk error"),
        });
        let (msg, show_recovery) = render_reconciliation_status(&state, &i18n, Locale::ZhCn);
        assert!(!msg.is_empty());
        assert!(
            show_recovery,
            "zh-CN state read error must show recovery hint"
        );
    }

    #[test]
    fn trusted_roots_status_surface_recovery_hint_is_localized() {
        use workhive_core::i18n_keys as k;
        let i18n = make_i18n();
        let hint_en = i18n.resolve(Locale::En, k::RECONCILIATION_RECOVERY_HINT);
        let hint_zh = i18n.resolve(Locale::ZhCn, k::RECONCILIATION_RECOVERY_HINT);
        assert!(!hint_en.is_empty(), "English recovery hint must exist");
        assert!(!hint_zh.is_empty(), "Chinese recovery hint must exist");
        assert_ne!(
            hint_en, hint_zh,
            "recovery hint must be localized differently"
        );
    }

    // ── trusted_roots_status_surface_ — command-surface config-dir failure ──

    #[test]
    fn trusted_roots_status_surface_config_dir_failure_renders_error_en() {
        use crate::context::{CommandContext, FakeProcessLauncher};
        use workhive_testkit::fakes::FakeConfig;

        let fake_config = FakeConfig::new().with_global(Default::default());
        // No config_dir → global_config_dir() returns Err.
        let launcher = FakeProcessLauncher::new();
        let ctx = CommandContext::with_test_boundaries(Box::new(fake_config), launcher);
        let mut output = Vec::new();
        let result = super::cmd_project_roots_status_with_writer(&ctx, &mut output);
        assert!(result.is_ok(), "{result:?}");
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("<fake-config-dir>"), "{output}");
        assert!(output.contains("workhive project reindex"), "{output}");
    }

    #[test]
    fn trusted_roots_status_surface_config_dir_failure_renders_error_zh_cn() {
        use crate::context::{CommandContext, FakeProcessLauncher};
        use workhive_testkit::fakes::FakeConfig;

        let fake_config = FakeConfig::new().with_global(workhive_core::ports::AppConfig {
            language: Some(workhive_core::domain::Locale::ZhCn),
            ..Default::default()
        });
        // No config_dir → global_config_dir() returns Err.
        let launcher = FakeProcessLauncher::new();
        let ctx = CommandContext::with_test_boundaries(Box::new(fake_config), launcher);
        let mut output = Vec::new();
        let result = super::cmd_project_roots_status_with_writer(&ctx, &mut output);
        assert!(result.is_ok(), "{result:?}");
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("<fake-config-dir>"), "{output}");
        assert!(output.contains("workhive project reindex"), "{output}");
    }

    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_core::ports::ProjectsConfig;
    use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

    fn entry_ctx(
        config_dir: &camino::Utf8Path,
        existing_roots: Vec<camino::Utf8PathBuf>,
    ) -> (CommandContext, FakeProcessLauncher) {
        let cfg = workhive_core::ports::AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: existing_roots,
            }),
            ..Default::default()
        };
        let fake_config = FakeConfig::new()
            .with_config_dir(config_dir.to_owned())
            .with_global(cfg);
        let launcher = FakeProcessLauncher::new();
        let ctx = CommandContext::with_test_boundaries(Box::new(fake_config), launcher.clone());
        (ctx, launcher)
    }

    #[test]
    fn trusted_roots_reconciliation_entry_add_launches_once() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        let result = super::cmd_project_roots_add(root_dir.as_str(), true, &ctx);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_add_already_present_skips_launch() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_dir.clone()]);
        let result = super::cmd_project_roots_add(root_dir.as_str(), true, &ctx);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_remove_launches_once() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_dir.clone()]);
        let index = FakeProjectIndex::new();
        let result =
            super::cmd_project_roots_remove_with_index(root_dir.as_str(), true, &ctx, &index);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_remove_not_present_skips_launch() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        let index = FakeProjectIndex::new();
        let result =
            super::cmd_project_roots_remove_with_index(root_dir.as_str(), true, &ctx, &index);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_add_second_root_launches_once() {
        let root_a = tempfile::tempdir().unwrap();
        let root_a_dir = camino::Utf8PathBuf::from_path_buf(root_a.path().to_path_buf()).unwrap();
        let root_b = tempfile::tempdir().unwrap();
        let root_b_dir = camino::Utf8PathBuf::from_path_buf(root_b.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_a_dir]);
        let result = super::cmd_project_roots_add(root_b_dir.as_str(), true, &ctx);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_direct_trust_launches() {
        use super::super::setup::{
            AdmissionCaller, AdmissionInput, enforce_trusted_root_with_input,
        };

        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        struct TrustInput;
        impl AdmissionInput for TrustInput {
            fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
                Ok(0)
            }
        }
        let unused_editor = FakeEditorInput::new(vec![]);
        enforce_trusted_root_with_input(
            root_dir.as_ref(),
            &ctx,
            AdmissionCaller::WorkspaceOpen,
            true,
            &TrustInput,
            &unused_editor,
        )
        .unwrap();
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_noop_fingerprint_skips_launch() {
        use workhive_core::domain::index_reconciliation::{
            ReconciliationRevision, ReconciliationState, RootsFingerprint,
        };
        use workhive_core::ports::IndexReconciliationPort;
        use workhive_infra::reconciliation::FsIndexReconciliation;

        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let roots = vec![root_dir.clone()];
        let fp = RootsFingerprint::from_trusted_roots(
            &workhive_core::domain::TrustedRootSet::from_raw(&roots, None),
        );
        let rev = ReconciliationRevision::new(1);
        let reconciliation = FsIndexReconciliation::new();
        reconciliation
            .write_latest_revision(state_dir.as_ref(), rev)
            .unwrap();
        reconciliation
            .write_state(
                state_dir.as_ref(),
                &ReconciliationState::Completed {
                    revision: rev,
                    fingerprint: fp,
                    completed_at: std::time::SystemTime::now(),
                    discovered_count: 1,
                },
            )
            .unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_dir.clone()]);
        let result = super::cmd_project_roots_add(root_dir.as_str(), true, &ctx);
        assert!(result.is_ok());
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_config_error_propagates() {
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();
        let launcher = FakeProcessLauncher::new();
        let fake_config = FakeConfig::new()
            .with_config_dir(state_dir)
            .with_global_error(workhive_core::errors::ConfigError::ReadFailed {
                path: camino::Utf8PathBuf::from("<test>"),
                source: std::io::Error::other("disk failure"),
            });
        let ctx = CommandContext::with_test_boundaries(Box::new(fake_config), launcher.clone());

        let result = super::cmd_project_roots_add("/tmp/x", true, &ctx);
        assert!(result.is_err());
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_state_write_failure_propagates() {
        use std::os::unix::fs::PermissionsExt;

        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);

        std::fs::set_permissions(state_tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = super::cmd_project_roots_add(root_dir.as_str(), true, &ctx);

        std::fs::set_permissions(state_tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.is_err());
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_remove_with_affected_force_still_launches() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_dir.clone()]);

        let index = FakeProjectIndex::new();
        let proj_root = root_dir.join("myproj");
        index
            .replace_all(&[workhive_core::ports::ProjectSummary {
                project_name: "myproj".to_owned(),
                project_root: proj_root.clone(),
                config_path: proj_root.join(".workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            }])
            .unwrap();

        let result =
            super::cmd_project_roots_remove_with_index(root_dir.as_str(), true, &ctx, &index);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_launch_failure_propagates() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        launcher.set_fail_next(true);

        let result = super::cmd_project_roots_add(root_dir.as_str(), true, &ctx);
        assert!(result.is_err(), "launch failure must propagate");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    // ── Fake editor input for cmd_project_roots_edit_with_input ──

    /// Deterministic input sequence for editor tests.
    struct FakeEditorInput {
        actions: std::cell::RefCell<Vec<FakeAction>>,
    }

    #[allow(dead_code)]
    enum FakeAction {
        /// Return menu selection index.
        Select(usize),
        /// Return text input string.
        TextInput(String),
        /// Return confirm result.
        Confirm(bool),
    }

    impl FakeEditorInput {
        fn new(actions: Vec<FakeAction>) -> Self {
            Self {
                actions: std::cell::RefCell::new(actions),
            }
        }
    }

    impl super::RootsEditorInput for FakeEditorInput {
        fn select(
            &self,
            _prompt: &str,
            _items: &[String],
            _default: usize,
        ) -> Result<usize, CliError> {
            match self.actions.borrow_mut().remove(0) {
                FakeAction::Select(idx) => Ok(idx),
                _ => panic!("expected Select action"),
            }
        }

        fn text_input(&self, _prompt: &str, _initial: &str) -> Result<String, CliError> {
            match self.actions.borrow_mut().remove(0) {
                FakeAction::TextInput(s) => Ok(s),
                _ => panic!("expected TextInput action"),
            }
        }

        fn confirm(&self, _prompt: &str, _default: bool) -> Result<bool, CliError> {
            match self.actions.borrow_mut().remove(0) {
                FakeAction::Confirm(b) => Ok(b),
                _ => panic!("expected Confirm action"),
            }
        }
    }

    // ── Entry: edit save ──

    #[test]
    fn trusted_roots_reconciliation_entry_edit_save_launches_once() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        // Sequence: Add(0) → path text → Save(2)
        let input = FakeEditorInput::new(vec![
            FakeAction::Select(0),
            FakeAction::TextInput(root_dir.to_string()),
            FakeAction::Select(2),
        ]);
        let result = super::cmd_project_roots_edit_with_input(&ctx, &input);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_edit_cancel_skips_launch() {
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        // Sequence: Cancel(3)
        let input = FakeEditorInput::new(vec![FakeAction::Select(3)]);
        let result = super::cmd_project_roots_edit_with_input(&ctx, &input);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_edit_no_changes_skips_launch() {
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        // Start with a root already present, Save immediately without changes.
        let (ctx, launcher) = entry_ctx(&state_dir, vec![root_dir]);
        let input = FakeEditorInput::new(vec![FakeAction::Select(2)]);
        let result = super::cmd_project_roots_edit_with_input(&ctx, &input);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    // ── Entry: admission-editor save ──

    #[test]
    fn trusted_roots_reconciliation_entry_admission_editor_save_launches() {
        use super::super::setup::{
            AdmissionCaller, AdmissionInput, enforce_trusted_root_with_input,
        };
        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        // Simulates the admission "Manage trusted directories" path.
        let input = FakeEditorInput::new(vec![
            FakeAction::Select(0),
            FakeAction::TextInput(root_dir.to_string()),
            FakeAction::Select(2),
        ]);
        struct ManageInput;
        impl AdmissionInput for ManageInput {
            fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
                Ok(1)
            }
        }
        let result = enforce_trusted_root_with_input(
            root_dir.as_ref(),
            &ctx,
            AdmissionCaller::WorkspaceOpen,
            true,
            &ManageInput,
            &input,
        );
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_admission_editor_cancel_skips_launch() {
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        let input = FakeEditorInput::new(vec![FakeAction::Select(3)]);
        let result = super::cmd_project_roots_edit_with_input(&ctx, &input);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_edit_save_failure_propagates() {
        use std::os::unix::fs::PermissionsExt;

        let root_tmp = tempfile::tempdir().unwrap();
        let root_dir = camino::Utf8PathBuf::from_path_buf(root_tmp.path().to_path_buf()).unwrap();
        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);

        // Make config dir read-only so write_trusted_roots_into_global_toml fails.
        std::fs::set_permissions(state_tmp.path(), std::fs::Permissions::from_mode(0o555)).unwrap();

        let input = FakeEditorInput::new(vec![
            FakeAction::Select(0),
            FakeAction::TextInput(root_dir.to_string()),
            FakeAction::Select(2),
        ]);
        let result = super::cmd_project_roots_edit_with_input(&ctx, &input);

        std::fs::set_permissions(state_tmp.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(result.is_err(), "save failure must propagate");
        assert_eq!(launcher.launch_reconciliation_call_count(), 0);
    }

    #[test]
    fn trusted_roots_reconciliation_entry_admission_loops_after_editor_still_outside() {
        use super::super::setup::{AdmissionCaller, enforce_trusted_root_with_input};
        use crate::commands::project::setup::AdmissionInput;

        let state_tmp = tempfile::tempdir().unwrap();
        let state_dir = camino::Utf8PathBuf::from_path_buf(state_tmp.path().to_path_buf()).unwrap();

        // No trusted roots configured → project not admitted.
        let (ctx, launcher) = entry_ctx(&state_dir, vec![]);
        let project_dir = camino::Utf8PathBuf::from("/some/other/project");

        // Editor input: add a root that does NOT cover the project (/unrelated).
        let input = FakeEditorInput::new(vec![
            FakeAction::Select(0),
            FakeAction::TextInput("/unrelated".to_string()),
            FakeAction::Select(2),
        ]);

        let select_count = std::cell::Cell::new(0usize);
        struct LoopingAdmissionInput<'a> {
            count: &'a std::cell::Cell<usize>,
        }
        impl AdmissionInput for LoopingAdmissionInput<'_> {
            fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
                let n = self.count.get();
                self.count.set(n + 1);
                if n == 0 {
                    // First time: choose "Manage trusted directories..."
                    Ok(1)
                } else {
                    // Second time: cancel
                    Ok(2)
                }
            }
        }
        let admission_input = LoopingAdmissionInput {
            count: &select_count,
        };

        let result = enforce_trusted_root_with_input(
            project_dir.as_ref(),
            &ctx,
            AdmissionCaller::WorkspaceOpen,
            true,
            &admission_input,
            &input,
        );
        // The flow should have looped: editor didn't cover the project,
        // so menu was shown again, then user cancelled.
        assert!(result.is_err(), "cancel after loop must return error");
        assert_eq!(select_count.get(), 2, "menu must have been shown twice");
        assert_eq!(
            launcher.launch_reconciliation_call_count(),
            0,
            "no launch since editor wrote an unrelated root"
        );
    }
}

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

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
            // Write via toml_edit rather than
            // round-tripping AppConfig, so user comments and unrelated
            // formatting survive the mutation.
            write_trusted_roots_into_global_toml(&next, ctx)?;
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
            // Redundant child under an already-trusted parent. We do
            // NOT write the global TOML — the parent already trusts
            // the entire subtree. Surface the rejection to the user
            // so they understand why the entry was not added.
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
            // Read the global project index, count
            // projects affected by this removal using component-level path
            // containment (no string prefix), and either prompt (interactive)
            // or require --force (non-interactive). Removing a root never
            // deletes the underlying project or its `.workhive/` state — it
            // only revokes safety-policy admission.
            //
            // Route through
            // `load_indexed_projects_for_root_removal` so an unreadable or
            // corrupt index aborts the mutation before any TOML write,
            // instead of being silently relabeled as "zero affected
            // projects". `IndexState::NotFound` still resolves to an empty
            // vector because that is a known setup state, not a parse fault.
            let index = ctx.composition.project_index()?;
            let projects = load_indexed_projects_for_root_removal(&index)?;
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

            write_trusted_roots_into_global_toml(&next, ctx)?;
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
pub(crate) fn cmd_project_roots_edit(ctx: &CommandContext) -> Result<(), CliError> {
    use std::io::IsTerminal;
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::{
        TrustedRootMutation, add_trusted_root, remove_trusted_root,
    };
    let locale = ctx.locale();

    if !std::io::stdin().is_terminal() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_NO_TTY),
        ));
    }

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
        let selection = match ui::screens::select::run(
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
                return Ok(());
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
                let raw = match ui::screens::text_input::run(
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
                    add_retry.reject(trimmed, msg);
                    continue;
                }
                if let Err(e) = validate_trusted_root_path(candidate.as_ref(), false, ctx) {
                    let msg = e.to_string();
                    eprintln!("{msg}");
                    add_retry.reject(trimmed, msg);
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
                let idx = match ui::screens::select::run(
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
                // Save applies the same affected-project guard that
                // per-root removal enforces. It computes the removed set
                // with canonical-aware comparison and reports one
                // consolidated warning for affected indexed projects.
                if !dirty {
                    eprintln!(
                        "{}",
                        ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_NO_CHANGES)
                    );
                    return Ok(());
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

                    // `cmd_project_roots_edit` already requires a TTY
                    // (TRUSTED_ROOTS_EDIT_NO_TTY guard at entry), so we
                    // confirm interactively here. The non-interactive
                    // arm exists for defense-in-depth in case the TTY
                    // gate is ever loosened — keep it, fail fast, and
                    // point users at the per-root `--force` workflow.
                    if std::io::stdin().is_terminal() {
                        let prompt = ctx
                            .i18n
                            .resolve(locale, k::TRUSTED_ROOTS_EDIT_AFFECTED_PROMPT);
                        let confirmed =
                            crate::commands::confirm_with_transcript(ctx, locale, &prompt, false)?;
                        if !confirmed {
                            eprintln!(
                                "{}",
                                ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_CANCELLED)
                            );
                            return Ok(());
                        }
                    } else {
                        return Err(CliError::Runtime(
                            ctx.i18n
                                .resolve(locale, k::TRUSTED_ROOTS_EDIT_AFFECTED_REQUIRES_FORCE),
                        ));
                    }
                }

                write_trusted_roots_into_global_toml(&working, ctx)?;
                let count = working.len().to_string();
                eprintln!(
                    "{}",
                    ctx.i18n
                        .resolve_with_args(locale, k::TRUSTED_ROOTS_EDIT_SAVED, &[&count])
                );
                return Ok(());
            }
            _ => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve(locale, k::TRUSTED_ROOTS_EDIT_CANCELLED)
                );
                return Ok(());
            }
        }
    }
}

pub(crate) fn cmd_project_roots_status(ctx: &CommandContext) -> Result<(), CliError> {
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
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_CONFIGURED, &[&count])
    );
    if !configured_roots.is_empty() {
        for entry in &configured_roots {
            println!("  - {entry}");
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
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::TRUSTED_ROOTS_STATUS_INDEX, &[&index_label])
    );
    let project_count_str = project_count.to_string();
    eprintln!(
        "{}",
        ctx.i18n.resolve_with_args(
            locale,
            k::TRUSTED_ROOTS_STATUS_PROJECTS,
            &[&project_count_str],
        )
    );

    let setup = resolve_setup_state(SetupStateProbe {
        trusted_roots_configured: !trusted_set.is_empty(),
        index_present,
        indexed_project_count: project_count,
    });
    print_setup_state_hint(&setup, ctx, locale);
    Ok(())
}

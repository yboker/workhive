#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use workhive_core::ports::ExecutableAvailabilityPort;

pub(crate) fn cmd_reindex(
    roots_args: Vec<String>,
    config_roots: bool,
    clear_first: bool,
    dry_run: bool,
    force: bool,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let mut roots: Vec<Utf8PathBuf> = roots_args.into_iter().map(Utf8PathBuf::from).collect();
    let explicit_root_count = roots.len();

    // When the user did not pass explicit `--root` arguments (or asked for
    // `--config-roots`), pull the canonical trusted-root list from the global
    // config. The new `[projects].trusted_roots` takes precedence over the
    // legacy `[discovery].project_roots` via `AppConfig::trusted_roots()`.
    if config_roots || roots.is_empty() {
        // Reindex is a mutating operation (writes the
        // global project index). A malformed/unreadable global config must
        // surface the parse error, not silently degrade to "no trusted
        // roots configured" → the misleading `REINDEX_NO_ROOTS` branch.
        let global_cfg = ctx.config.load_global()?;
        let configured = global_cfg
            .as_ref()
            .map(|g| g.trusted_roots())
            .unwrap_or_default();
        for r in configured {
            if !roots.contains(&r) {
                roots.push(r);
            }
        }
    }

    if roots.is_empty() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::REINDEX_NO_ROOTS),
        ));
    }

    // Every explicit
    // `--root <path>` must lie within a configured trusted root. Implicit
    // roots loaded from `[projects].trusted_roots` are admitted by definition,
    // so we only validate the leading `explicit_root_count` entries.
    if explicit_root_count > 0 {
        use workhive_core::usecases::trusted_roots::trusted_roots_from_config;
        // Explicit-root validation is a safety
        // admission gate; an unreadable global config must not silently
        // degrade the trusted-root set to "empty" (which would either
        // accept everything or reject everything depending on policy).
        // Propagate the error.
        let global_cfg = ctx.config.load_global()?;
        let trusted = trusted_roots_from_config(global_cfg.as_ref(), home_path().as_deref());
        for explicit in roots.iter().take(explicit_root_count) {
            let expanded =
                workhive_core::domain::expand_home(explicit.as_path(), home_path().as_deref());
            if !trusted.admits(expanded.as_path()) {
                return Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                    locale,
                    k::REINDEX_ROOT_NOT_TRUSTED,
                    &[expanded.as_str()],
                )));
            }
        }
    }

    if !dry_run {
        confirm::require_confirm(&ctx.i18n.resolve(locale, k::REINDEX_CONFIRM), force)?;
    }

    let roots_count = roots.len().to_string();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::REINDEX_SCANNING, &[&roots_count])
    );

    let discovery = ctx.composition.project_discovery();
    let index = ctx.composition.project_index()?;

    // The filesystem must
    // see the expanded form even though the authored config retains `~`.
    // Expansion is lexical only — `trusted_roots_from_config` already
    // expanded on the admission gate above; here we expand the actual
    // discovery list for `DiscoveryPort` consumption.
    let home = home_path();
    let expanded_roots = workhive_core::usecases::trusted_roots::expand_trusted_roots_for_discovery(
        &roots,
        home.as_deref(),
    );

    if dry_run {
        let report = discovery
            .discover_projects_report(&expanded_roots)
            .map_err(|e| localize_discovery_error(ctx, locale, &e, &roots, &expanded_roots))?;
        let discovered = report.projects;
        let pre_state = index.list_projects()?;

        // Compute what the merge/clear semantics WOULD produce without
        // mutating the index. This mirrors `rebuild_project_index` so the
        // dry-run preview lines up with the real run.
        let mut would_upsert = 0usize;
        let mut discovered_roots: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for p in &discovered {
            discovered_roots.insert(p.project_root.as_str());
            would_upsert += 1;
        }
        let retained_existing = if clear_first {
            0
        } else {
            pre_state
                .projects()
                .iter()
                .filter(|p| !discovered_roots.contains(p.project_root.as_str()))
                .count()
        };
        let final_count = if clear_first {
            would_upsert
        } else {
            // Existing entries that match a discovered root are upserted (count once);
            // non-discovered existing entries are retained as-is.
            retained_existing + would_upsert
        };

        eprintln!("{}", ctx.i18n.resolve(locale, k::REINDEX_DRY_RUN_HEADING));
        emit_reindex_summary(
            ctx,
            locale,
            discovered.len(),
            roots.len(),
            would_upsert,
            retained_existing,
            final_count,
        );
        emit_reindex_warnings(ctx, locale, &report.warnings);
        for p in &discovered {
            println!(
                "  {} -> {}",
                p.project_name,
                crate::path_display::display_path_for_humans(p.project_root.as_path())
            );
        }
        return Ok(());
    }

    let result =
        usecases::reindex::rebuild_project_index(&discovery, &index, &expanded_roots, clear_first)
            .map_err(|e| localize_reindex_error(ctx, locale, &e, &roots, &expanded_roots))?;

    emit_reindex_summary(
        ctx,
        locale,
        result.discovered_count,
        result.roots_scanned,
        result.upserted_count,
        result.retained_existing_count,
        result.final_index_count,
    );
    emit_reindex_warnings(ctx, locale, &result.warnings);

    Ok(())
}

/// Print a typed list of non-fatal discovery warnings to stderr.
///
/// Currently only emits [`DiscoveryWarning::UnreadableDescendant`].
/// Top-level fatal failures are not warnings — they are surfaced via
/// `DiscoveryError` and never reach this function.
pub(crate) fn emit_reindex_warnings(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    warnings: &[workhive_core::ports::DiscoveryWarning],
) {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::DiscoveryWarning;
    if warnings.is_empty() {
        return;
    }
    eprintln!("{}", ctx.i18n.resolve(locale, k::REINDEX_WARNINGS_HEADER));
    for w in warnings {
        match w {
            DiscoveryWarning::UnreadableDescendant { path, reason } => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve_with_args(
                        locale,
                        k::REINDEX_WARNING_UNREADABLE_DESCENDANT,
                        &[path.as_str(), reason],
                    )
                );
            }
        }
    }
}

/// Render a `DiscoveryError` using the authored trusted-root shape when
/// the error references a missing root.
pub(crate) fn localize_discovery_error(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    err: &workhive_core::errors::DiscoveryError,
    authored: &[camino::Utf8PathBuf],
    expanded: &[camino::Utf8PathBuf],
) -> CliError {
    use workhive_core::errors::DiscoveryError;
    use workhive_core::i18n_keys as k;
    if let DiscoveryError::RootNotFound { path } = err {
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

/// Print the four-counter reindex summary in the user's locale.
///
/// Keeping this helper local to `cmd_reindex` ensures the dry-run path
/// and the real run emit identical line ordering, so the wording stays
/// in sync between previews and applied changes.
pub(crate) fn emit_reindex_summary(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    discovered: usize,
    roots_scanned: usize,
    upserted: usize,
    retained_existing: usize,
    final_index_count: usize,
) {
    use workhive_core::i18n_keys as k;

    let discovered_s = discovered.to_string();
    let roots_s = roots_scanned.to_string();
    let upserted_s = upserted.to_string();
    let retained_s = retained_existing.to_string();
    let final_s = final_index_count.to_string();
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::REINDEX_DISCOVERED, &[&discovered_s, &roots_s])
    );
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::REINDEX_UPSERTED, &[&upserted_s])
    );
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::REINDEX_RETAINED_EXISTING, &[&retained_s])
    );
    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::REINDEX_FINAL_INDEX_TOTAL, &[&final_s])
    );
}

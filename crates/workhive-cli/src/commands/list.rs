#[allow(unused_imports)]
use super::list_render::*;
use super::*;
use workhive_core::ports::WorkspaceIdentityPort;

/// `workhive workspace list` — list workspaces in the current project.
///
/// This is the project-scoped list; cross-project listing lives under
/// `workhive project list` and is implemented in `cmd_project_list`.
pub(crate) fn cmd_workspace_list(view: ListView, ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let (root, name) = detect_project()?;
    let scope = super::project::ConfigScope::Project {
        root: root.as_ref(),
    };
    let source = resolve_config_source(scope, ctx)?;
    if view != ListView::Json {
        let source_line = resolve_config_source_line(scope, ctx, locale)?;
        eprintln!("{source_line}");
    }
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let is_initialized =
        usecases::project_state::resolve_init_state(&ws_fs, root.as_ref()).is_initialized();

    let backend = ctx.runtime_backend_for(Some(root.as_ref()))?;
    let mut entries = if is_initialized {
        collect_workspace_entries(&root, &name, ctx)?
    } else {
        let main_branch = git.current_branch()?;
        vec![usecases::list::WorkspaceEntry {
            name: main_branch,
            kind: usecases::list::WorkspaceKind::ProjectRoot,
            worktree_path: root.clone(),
            worktree_exists: false,
            session_name: String::new(),
            session_active: false,
            session_state: workhive_core::domain::SessionAvailability::Missing,
            pane_count: None,
            layout_info: "-".to_owned(),
            last_used_epoch_secs: None,
            pane_count_status: usecases::list::PaneCountStatus::NotApplicable,
        }]
    };

    let layout = load_effective_layout(ctx, Some(root.as_ref()));
    apply_inactive_layout_fallback(&mut entries, &layout);

    if view != ListView::Json {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::LIST_BACKEND, &[backend.as_str()])
        );
    }

    match view {
        ListView::Auto => print_workspace_auto(&entries, &layout, ctx, locale, is_initialized),
        ListView::Table => print_workspace_table(&entries, &layout, ctx, locale, is_initialized),
        ListView::Compact => {
            print_workspace_compact_list(&entries, &layout, ctx, locale, is_initialized)
        }
        ListView::Json => print_workspace_json(&entries, &source, &backend),
    }

    if !is_initialized && view != ListView::Json {
        eprintln!("{}", ctx.i18n.resolve(locale, k::LIST_UNINITIALIZED_HINT));
    }
    Ok(())
}

/// `workhive project list` — list all registered projects from the global
/// index. Honors `--view auto|table|compact|json`.
///
/// All views share the same usecase-level data path (`list_all_projects`).
/// The JSON view emits the unified `ProjectListResult` shape; table /
/// compact / auto views render the `Loaded` variant from that same
/// query result. CLI rendering only formats `ProjectEntry { name, root }`
/// — it does not enrich projects with side reads from the workspace
/// filesystem.
pub(crate) fn cmd_project_list(view: ListView, ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;

    let locale = ctx.locale();

    // Use the shared trusted-project resolver for consistent state handling.
    let state = crate::commands::resolve_trusted_project_index(ctx)?;

    if view == ListView::Json {
        let source = resolve_config_source(super::project::ConfigScope::Global, ctx)?;
        let filtered_result = match state {
            crate::commands::TrustedProjectIndexState::NotFound { .. } => {
                // Empty result for JSON when index doesn't exist
                usecases::list::ProjectListResult::IndexNotFound
            }
            crate::commands::TrustedProjectIndexState::Empty { .. } => {
                // Empty result for JSON when no projects visible
                usecases::list::ProjectListResult::Loaded(Vec::new())
            }
            crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => {
                // Convert ProjectSummary to ProjectEntry for JSON output
                let entries: Vec<_> = projects
                    .into_iter()
                    .map(|p| usecases::list::ProjectEntry {
                        name: p.project_name,
                        root: p.project_root.to_string(),
                    })
                    .collect();
                usecases::list::ProjectListResult::Loaded(entries)
            }
        };
        print_projects_json(&filtered_result, &source);
        return Ok(());
    }

    // Handle non-JSON views with setup state hints
    let _setup = match state {
        crate::commands::TrustedProjectIndexState::NotFound { setup } => {
            print_setup_state_hint(&setup, ctx, locale);
            return Ok(());
        }
        crate::commands::TrustedProjectIndexState::Empty { setup } => {
            match setup {
                DiscoverySetupState::Ready => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::PROJECTS_EMPTY));
                }
                _ => print_setup_state_hint(&setup, ctx, locale),
            }
            return Ok(());
        }
        crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => {
            // Convert ProjectSummary to ProjectEntry for display
            let entries: Vec<_> = projects
                .into_iter()
                .map(|p| usecases::list::ProjectEntry {
                    name: p.project_name,
                    root: p.project_root.to_string(),
                })
                .collect();
            eprintln!("{}", ctx.i18n.resolve(locale, k::PROJECTS_HEADER));

            match view {
                ListView::Json => unreachable!("handled above"),
                ListView::Table => print_projects_table(&entries, ctx, locale),
                ListView::Compact => print_projects_compact_list(&entries, ctx, locale),
                ListView::Auto => print_projects_auto(&entries, ctx, locale),
            }

            return Ok(());
        }
    };
}

fn print_projects_table(
    rows: &[usecases::list::ProjectEntry],
    ctx: &CommandContext,
    locale: Locale,
) {
    use table::{Column, print_table};
    use workhive_core::i18n_keys as k;

    let columns = vec![
        Column {
            header: ctx.i18n.resolve(locale, k::PROJECTS_COL_NAME),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::PROJECTS_COL_ROOT),
            min_width: 0,
        },
    ];
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            vec![
                r.name.clone(),
                crate::path_display::display_path_for_humans(camino::Utf8Path::new(&r.root)),
            ]
        })
        .collect();
    print_table(&columns, &table_rows);
}

fn print_projects_compact_list(
    rows: &[usecases::list::ProjectEntry],
    ctx: &CommandContext,
    locale: Locale,
) {
    use workhive_core::i18n_keys as k;

    let root_label = ctx.i18n.resolve(locale, k::PROJECTS_COL_ROOT);
    let last_idx = rows.len().saturating_sub(1);
    for (idx, r) in rows.iter().enumerate() {
        println!("{}", r.name);
        println!(
            "  {}: {}",
            root_label,
            crate::path_display::display_path_for_humans(camino::Utf8Path::new(&r.root))
        );
        if idx != last_idx {
            println!();
        }
    }
}

fn print_projects_auto(
    rows: &[usecases::list::ProjectEntry],
    ctx: &CommandContext,
    locale: Locale,
) {
    if let Some((width, _)) = terminal_size::terminal_size()
        && width.0 <= 96
    {
        print_projects_compact_list(rows, ctx, locale);
        return;
    }
    print_projects_table(rows, ctx, locale);
}

/// Collect the enriched workspace entries for an initialized project.
///
/// Returns the project-root row followed by every linked-worktree row,
/// each carrying session availability, pane-count, layout, and
/// recent-activity enrichment. This is the single source of truth shared
/// by the `workspace list` table and the interactive `workspace open`
/// selector, so the two surfaces can never drift in how they describe a
/// workspace. Callers that run before initialization must guard with
/// their own init probe — this function assumes the project is
/// initialized and propagates probe errors rather than masking them.
pub(crate) fn collect_workspace_entries(
    root: &Utf8PathBuf,
    name: &str,
    ctx: &CommandContext,
) -> Result<Vec<usecases::list::WorkspaceEntry>, CliError> {
    let locale = ctx.locale();
    let git = ctx.composition.git();
    let ws_fs = ctx.composition.workspace_fs();

    let policy = ctx.naming_policy_for(Some(root.as_ref()))?;
    let naming_ctx = build_naming_context(name, "", root, &git, &ws_fs);
    let identity = ctx.composition.workspace_identity(root.as_ref());
    let identity_records = identity.list_workspace_identities()?;
    let configured_backend = ctx.runtime_backend_for(Some(root.as_ref()))?;
    for record in &identity_records {
        let backend = &record.runtime_binding().backend;
        if backend != &configured_backend {
            return Err(CliError::Runtime(format!(
                "configured runtime backend '{configured_backend}' does not match persisted binding '{backend}'",
            )));
        }
    }
    let (server, runtime) = if let Some(record) = identity_records.first() {
        let binding = record.runtime_binding();
        (
            binding.scope_id.clone(),
            ctx.composition.runtime_for_binding(&binding)?,
        )
    } else {
        (
            policy.resolve_server_for_project(&naming_ctx)?,
            ctx.composition.runtime_for_backend(&configured_backend)?,
        )
    };
    let session_fn = |ws: &str| {
        let c = build_naming_context(name, ws, root, &git, &ws_fs);
        policy.resolve_session(&c)
    };
    let mut entries = usecases::list::list_workspaces(
        &ws_fs,
        runtime.as_ref(),
        root.as_ref(),
        &server,
        &session_fn,
    )?;

    let main_entry = build_main_workspace_entry(
        &git,
        runtime.as_ref(),
        &ws_fs,
        root,
        name,
        &server,
        &policy,
        &ctx.i18n,
        locale,
    )?;
    entries.insert(0, main_entry);
    Ok(entries)
}

/// Fill the layout column for inactive workspaces from the effective
/// layout config.
///
/// A running session reports its real pane count, so this only applies to
/// rows with no live session and no resolved layout: it substitutes the
/// configured template + pane total so an operator sees what *would* be
/// launched rather than a bare placeholder.
pub(crate) fn apply_inactive_layout_fallback(
    entries: &mut [usecases::list::WorkspaceEntry],
    layout: &workhive_core::ports::LayoutConfig,
) {
    let configured_total = layout.total_panes();
    if configured_total == 0 {
        return;
    }
    for entry in entries.iter_mut() {
        if !entry.session_active && entry.layout_info == "-" {
            entry.layout_info = format!("{} ({})", layout.template, configured_total);
        }
    }
}

/// Pre-localized per-column metadata for the open selector.
///
/// each part is the final display string for the matching column in the
/// `workspace open` selector: session availability, layout, and last-used time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectorEnrichmentParts {
    pub session: String,
    pub layout: String,
    pub recent: String,
}

/// Compose the per-column enrichment parts used by interactive selectors.
///
/// Each part is produced by the same localized display helper that drives
/// the `workspace list` table, so the open selector and the list view
/// describe a workspace identically column-by-column.
pub(crate) fn render_selector_enrichment_parts(
    entry: &usecases::list::WorkspaceEntry,
    ctx: &CommandContext,
    locale: Locale,
) -> SelectorEnrichmentParts {
    SelectorEnrichmentParts {
        session: workspace_status_display(entry, ctx, locale),
        layout: layout_display(entry, ctx, locale),
        recent: recent_display(entry, ctx, locale),
    }
}

pub(crate) fn mark_workspace_used_now(
    root: &Utf8PathBuf,
    workspace_name: &str,
    ctx: &CommandContext,
) {
    let Some(epoch_secs) = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
    else {
        tracing::warn!(
            workspace = workspace_name,
            "system clock is before UNIX epoch; skipping workspace last-used update"
        );
        return;
    };
    let ws_fs = ctx.composition.workspace_fs();
    if let Err(e) = ws_fs.mark_workspace_used_epoch_secs(root.as_ref(), workspace_name, epoch_secs)
    {
        tracing::warn!(
            workspace = workspace_name,
            error = %e,
            "failed to persist workspace last-used timestamp"
        );
    }
}

/// Build a `WorkspaceEntry` for the main branch — CLI-side thin wrapper.
///
/// Resolves the i18n label (CLI concern) and cwd (environment read),
/// then delegates all business orchestration to core.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_main_workspace_entry(
    git: &dyn GitPort,
    ws_runtime: &dyn WorkspaceRuntimePort,
    ws_fs: &dyn WorkspaceFsPort,
    root: &Utf8PathBuf,
    project_name: &str,
    server: &str,
    policy: &SessionNamingPolicy,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> Result<usecases::list::WorkspaceEntry, CliError> {
    let main_label = i18n.resolve(locale, workhive_core::i18n_keys::LIST_MAIN_LABEL);
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());

    let entry = usecases::list::build_main_workspace_entry(
        git,
        ws_runtime,
        ws_fs,
        root,
        project_name,
        server,
        policy,
        &main_label,
        cwd.as_deref(),
    )?;
    Ok(entry)
}

pub(crate) fn print_projects_json(
    result: &usecases::list::ProjectListResult,
    source: &(String, String),
) {
    let projects = match result {
        usecases::list::ProjectListResult::IndexNotFound => Vec::<serde_json::Value>::new(),
        usecases::list::ProjectListResult::Loaded(projects) => projects
            .iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "root": p.root,
                })
            })
            .collect(),
    };

    let payload = serde_json::json!({
        "config_source": {
            "scope": source.0,
            "path": source.1,
        },
        "projects": projects,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).expect("JSON serialization must succeed")
    );
}

pub(crate) fn print_workspace_json(
    entries: &[usecases::list::WorkspaceEntry],
    source: &(String, String),
    backend: &workhive_core::domain::RuntimeBackendId,
) {
    let rows: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| workspace_json_row(entry, backend))
        .collect();

    let payload = serde_json::json!({
        "config_source": {
            "scope": source.0,
            "path": source.1,
        },
        "workspaces": rows,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).expect("JSON serialization must succeed")
    );
}

/// Render one `workspace list --view json` row.
///
/// The machine-readable surface carries the last-used time on two keys:
/// - `last_used_at`: a UTC `YYYY-MM-DD HH:mm` string, the long-standing
///   shape consumers already parse; `null` when there is no record;
/// - `last_used_epoch_secs`: the raw Unix epoch seconds as a number, for
///   consumers that want a timezone-neutral integer; `null` when there
///   is no record.
///
/// JSON stays locale- and timezone-neutral. Human surfaces (`workspace
/// list` table, `workspace open`/`attach` selectors) render local time
/// via [`format_last_used_epoch`].
fn workspace_json_row(
    e: &usecases::list::WorkspaceEntry,
    backend: &workhive_core::domain::RuntimeBackendId,
) -> serde_json::Value {
    use usecases::list::PaneCountStatus;
    use workhive_core::domain::SessionAvailability;

    let session_state = match e.session_state {
        SessionAvailability::Running => "running",
        SessionAvailability::Missing => "missing",
        SessionAvailability::ServerUnreachable => "server_unreachable",
        SessionAvailability::LookupFailed => "lookup_failed",
    };
    let pane_count_status_str = match e.pane_count_status {
        PaneCountStatus::Known(_) => "known",
        PaneCountStatus::NotApplicable => "not_applicable",
        PaneCountStatus::Unknown => "unknown",
    };
    serde_json::json!({
        "name": e.name,
        "backend": backend.as_str(),
        "kind": e.kind.as_token(),
        "status": if e.session_active { "active" } else { "inactive" },
        "session_state": session_state,
        "pane_count": e.pane_count,
        "pane_count_status": pane_count_status_str,
        "layout": if e.session_active && e.pane_count.is_some() { "-" } else { &e.layout_info },
        "worktree_exists": e.worktree_exists,
        "session": e.session_name,
        "last_used_at": format_last_used_at_utc(e.last_used_epoch_secs),
        "last_used_epoch_secs": e.last_used_epoch_secs,
        "path": e.worktree_path,
    })
}

/// Render a raw epoch as the JSON-stable UTC `YYYY-MM-DD HH:mm` string,
/// or `None` (serialized as JSON null) when there is no record or the
/// value is not representable.
fn format_last_used_at_utc(epoch_secs: Option<u64>) -> Option<String> {
    let secs = i64::try_from(epoch_secs?).ok()?;
    chrono::DateTime::from_timestamp(secs, 0).map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;

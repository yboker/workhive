#[allow(unused_imports)]
use super::list_render::*;
use super::*;

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
            has_snapshot: false,
            last_used_epoch_secs: None,
            snapshot_status: usecases::list::SnapshotStatus::Absent,
            pane_count_status: usecases::list::PaneCountStatus::NotApplicable,
        }]
    };

    apply_inactive_layout_fallback(&mut entries, ctx, root.as_ref());

    match view {
        ListView::Auto => print_workspace_auto(&entries, ctx, locale, is_initialized),
        ListView::Table => print_workspace_table(&entries, ctx, locale, is_initialized),
        ListView::Compact => print_workspace_compact_list(&entries, ctx, locale, is_initialized),
        ListView::Json => print_workspace_json(&entries, &source),
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
    use workhive_core::usecases::trusted_roots::{
        DiscoverySetupState, SetupStateProbe, resolve_setup_state, trusted_roots_from_config,
    };

    let locale = ctx.locale();
    let index = ctx.composition.project_index()?;
    let result = usecases::list::list_all_projects(&index)?;

    if view == ListView::Json {
        let source = resolve_config_source(super::project::ConfigScope::Global, ctx)?;
        print_projects_json(&result, &source);
        return Ok(());
    }

    // Read-only hint surface. Use the dedicated
    // `load_global_for_hint` helper so an unreadable global config surfaces
    // an explicit "config unreadable" line instead of silently mislabeling
    // the situation as "no trusted roots configured".
    let global_cfg = super::project::load_global_for_hint(ctx, locale);
    let trusted_set = trusted_roots_from_config(global_cfg.as_ref(), home_path().as_deref());

    let projects = match &result {
        usecases::list::ProjectListResult::IndexNotFound => {
            let setup = resolve_setup_state(SetupStateProbe {
                trusted_roots_configured: !trusted_set.is_empty(),
                index_present: false,
                indexed_project_count: 0,
            });
            print_setup_state_hint(&setup, ctx, locale);
            return Ok(());
        }
        usecases::list::ProjectListResult::Loaded(v) if v.is_empty() => {
            let setup = resolve_setup_state(SetupStateProbe {
                trusted_roots_configured: !trusted_set.is_empty(),
                index_present: true,
                indexed_project_count: 0,
            });
            match setup {
                DiscoverySetupState::Ready => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::PROJECTS_EMPTY));
                }
                _ => print_setup_state_hint(&setup, ctx, locale),
            }
            return Ok(());
        }
        usecases::list::ProjectListResult::Loaded(v) => v.as_slice(),
    };

    eprintln!("{}", ctx.i18n.resolve(locale, k::PROJECTS_HEADER));

    match view {
        ListView::Json => unreachable!("handled above"),
        ListView::Table => print_projects_table(projects, ctx, locale),
        ListView::Compact => print_projects_compact_list(projects, ctx, locale),
        ListView::Auto => print_projects_auto(projects, ctx, locale),
    }

    Ok(())
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
/// each carrying session availability, pane-count, snapshot, and
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
    let server = policy.resolve_server_for_project(&naming_ctx)?;
    let session_fn = |ws: &str| {
        let c = build_naming_context(name, ws, root, &git, &ws_fs);
        policy.resolve_session(&c)
    };
    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);
    let snapshot_store = ctx.composition.snapshot_store(root.as_ref());
    let identity = ctx.composition.workspace_identity(root.as_ref());
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());
    let subdir_key = usecases::naming::resolve_subdir_key(root, cwd.as_deref());
    let state_key_fn = |ws: &str| {
        let ws_identity_key = workhive_core::domain::WorkspaceIdentityKey::new(ws, &subdir_key);
        usecases::identity::derive_workspace_state_key(&identity, &ws_identity_key).ok()
    };

    let mut entries = usecases::list::list_workspaces(
        &ws_fs,
        &runtime,
        &snapshot_store,
        root.as_ref(),
        &server,
        &session_fn,
        &state_key_fn,
    )?;

    let main_entry = build_main_workspace_entry(
        &git,
        &runtime,
        &ws_fs,
        &snapshot_store,
        root,
        name,
        &server,
        &policy,
        &ctx.i18n,
        locale,
        &state_key_fn,
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
    ctx: &CommandContext,
    root: &camino::Utf8Path,
) {
    let layout = load_effective_layout(ctx, Some(root));
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
/// Each field is the final display string for the matching column in the
/// `workspace open` selector: session availability, layout/pane summary,
/// snapshot, and last-used time. Returning the parts separately (instead
/// of a pre-joined string) lets the selector renderer measure column
/// widths in display columns, align rows under shared headers, and drop
/// the lowest-priority columns to fit narrow terminals — without ever
/// re-parsing rendered text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectorEnrichmentParts {
    pub session: String,
    pub layout: String,
    pub snapshot: String,
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
        snapshot: snapshot_display(entry, ctx, locale),
        recent: recent_display(entry, ctx, locale),
    }
}

/// Pre-localized per-column attach enrichment for one workspace row.
///
/// Each field is the final display string the attach selector renders
/// in its corresponding column. Session availability is omitted because
/// attach rows are, by construction, live sessions; what an operator
/// lacks at the picker is the layout shape and how recently the
/// workspace changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct AttachEnrichmentParts {
    /// Layout / pane-count summary aligned with the `workspace list`
    /// `Layout` column.
    pub layout: String,
    /// Last-used timestamp aligned with the `workspace list` `Last used`
    /// column.
    pub recent: String,
}

/// Compose the per-column attach enrichment using the same localized
/// display helpers that drive the `workspace list` table.
///
/// The attach selector consumes these structured parts directly so the
/// renderer can compute display-width-aware column padding and
/// progressively drop the lowest-priority columns on narrow terminals
/// without re-parsing a joined string.
pub(crate) fn render_attach_enrichment_parts(
    entry: &usecases::list::WorkspaceEntry,
    ctx: &CommandContext,
    locale: Locale,
) -> AttachEnrichmentParts {
    AttachEnrichmentParts {
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
    snapshot_store: &dyn SnapshotStorePort,
    root: &Utf8PathBuf,
    project_name: &str,
    server: &str,
    policy: &SessionNamingPolicy,
    i18n: &dyn I18nPort,
    locale: Locale,
    state_key_fn: &dyn Fn(&str) -> Option<WorkspaceStateKey>,
) -> Result<usecases::list::WorkspaceEntry, CliError> {
    let main_label = i18n.resolve(locale, workhive_core::i18n_keys::LIST_MAIN_LABEL);
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| Utf8PathBuf::try_from(p).ok());

    let entry = usecases::list::build_main_workspace_entry(
        git,
        ws_runtime,
        ws_fs,
        snapshot_store,
        root,
        project_name,
        server,
        policy,
        &main_label,
        cwd.as_deref(),
        state_key_fn,
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
) {
    let rows: Vec<serde_json::Value> = entries.iter().map(workspace_json_row).collect();

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
fn workspace_json_row(e: &usecases::list::WorkspaceEntry) -> serde_json::Value {
    use usecases::list::{PaneCountStatus, SnapshotStatus};
    use workhive_core::domain::SessionAvailability;

    let session_state = match e.session_state {
        SessionAvailability::Running => "running",
        SessionAvailability::Missing => "missing",
        SessionAvailability::ServerUnreachable => "server_unreachable",
        SessionAvailability::LookupFailed => "lookup_failed",
    };
    let snapshot_status_str = match e.snapshot_status {
        SnapshotStatus::Available => "available",
        SnapshotStatus::Absent => "absent",
        SnapshotStatus::Unknown => "unknown",
    };
    let pane_count_status_str = match e.pane_count_status {
        PaneCountStatus::Known(_) => "known",
        PaneCountStatus::NotApplicable => "not_applicable",
        PaneCountStatus::Unknown => "unknown",
    };
    serde_json::json!({
        "name": e.name,
        "kind": e.kind.as_token(),
        "status": if e.session_active { "active" } else { "inactive" },
        "session_state": session_state,
        "pane_count": e.pane_count,
        "pane_count_status": pane_count_status_str,
        "layout": if e.session_active && e.pane_count.is_some() { "-" } else { &e.layout_info },
        "worktree_exists": e.worktree_exists,
        "has_snapshot": e.has_snapshot,
        "snapshot_status": snapshot_status_str,
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

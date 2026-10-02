//! `workhive session …` — global, backend-neutral runtime surfaces.
//!
//! `session list` shows the native Herdr hierarchy (including resources that
//! have no Workhive binding) and keeps tmux's existing flat presentation.
//! `session attach` resolves a persisted runtime binding across all projects
//! or hands an explicit Herdr session to Herdr's native attach command.
//!
//! These commands intentionally live in their own module (separate from the
//! large `commands/project.rs`) so the global session surface can evolve
//! independently from project-scoped logic.

#[cfg(unix)]
use std::collections::HashMap;
use std::io::IsTerminal;

use workhive_core::i18n_keys as k;
use workhive_core::ports::{EnvironmentPort, I18nPort, WorkspaceIdentityPort};
use workhive_core::usecases;
use workhive_core::usecases::sessions::{SessionRecord, UnavailableProject};

use crate::cli::ListView;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::path_display::display_path_for_humans;
#[cfg(unix)]
use crate::table::format_table_lines;
use crate::table::{Column, print_table};
use crate::ui;

use super::project::print_setup_state_hint;
use super::render_stale_entries;
use super::{build_global_attach_plan, render_global_attach_lines};

pub(crate) fn list_global_sessions_by_backend(
    projects: &[workhive_core::ports::DiscoveredProject],
    ctx: &CommandContext,
) -> Result<workhive_core::usecases::sessions::GlobalSessionListing, CliError> {
    let listing = usecases::sessions::list_global_sessions(
        projects,
        |project| {
            let identity = ctx
                .composition
                .workspace_identity(project.project_root.as_ref());
            identity.list_workspace_identities()
        },
        |binding| {
            let runtime = ctx
                .composition
                .runtime_for_binding(binding)
                .map_err(runtime_resolution_error)?;
            runtime.list_workspaces(&binding.scope_id)
        },
    )?;
    Ok(listing)
}

pub(crate) fn runtime_resolution_error(error: CliError) -> workhive_core::ports::RuntimeError {
    match error {
        CliError::RuntimeOp(error) => error,
        other => workhive_core::ports::RuntimeError::OperationFailed {
            message: other.to_string(),
        },
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrNativeListing {
    sessions: Vec<HerdrNativeSession>,
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrNativeSession {
    name: String,
    socket: String,
    running: bool,
    workspaces: Vec<HerdrNativeWorkspace>,
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrNativeWorkspace {
    id: String,
    number: u32,
    label: String,
    focused: bool,
    worktree: Option<workhive_infra::herdr::HerdrWorktreeSnapshot>,
    owner: Option<HerdrWorkspaceOwner>,
    tabs: Vec<HerdrNativeTab>,
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrNativeTab {
    id: String,
    number: u32,
    label: String,
    focused: bool,
    panes: Vec<HerdrNativePane>,
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrNativePane {
    id: String,
    label: Option<String>,
    agent: Option<String>,
    status: String,
    cwd: Option<String>,
    focused: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone, serde::Serialize)]
struct HerdrWorkspaceOwner {
    project: String,
    workspace: String,
    project_root: String,
}

#[cfg(unix)]
type HerdrOwnership = HashMap<(String, String), Vec<HerdrWorkspaceOwner>>;

#[cfg(unix)]
fn herdr_ownership(
    projects: &[workhive_core::ports::DiscoveredProject],
    ctx: &CommandContext,
) -> Result<HerdrOwnership, CliError> {
    let mut ownership: HerdrOwnership = HashMap::new();
    for project in projects {
        let identity = ctx
            .composition
            .workspace_identity(project.project_root.as_ref());
        for record in identity.list_workspace_identities()? {
            let binding = record.runtime_binding();
            if binding.backend != workhive_core::domain::RuntimeBackendId::herdr() {
                continue;
            }
            ownership
                .entry((binding.scope_id, binding.workspace_id))
                .or_default()
                .push(HerdrWorkspaceOwner {
                    project: project.project_name.clone(),
                    workspace: record.workspace,
                    project_root: project.project_root.to_string(),
                });
        }
    }
    Ok(ownership)
}

#[cfg(unix)]
fn project_herdr_session(
    name: String,
    socket_path: &std::path::Path,
    snapshot: workhive_infra::herdr::HerdrSessionSnapshot,
    ownership: &HerdrOwnership,
) -> HerdrNativeSession {
    let tabs = snapshot.tabs;
    let panes = snapshot.panes;
    let workspaces = snapshot
        .workspaces
        .into_iter()
        .map(|workspace| {
            let owner = ownership
                .get(&(
                    socket_path.to_string_lossy().into_owned(),
                    workspace.workspace_id.clone(),
                ))
                .filter(|owners| owners.len() == 1)
                .and_then(|owners| owners.first().cloned());
            let tabs = tabs
                .iter()
                .filter(|tab| tab.workspace_id == workspace.workspace_id)
                .map(|tab| HerdrNativeTab {
                    id: tab.tab_id.clone(),
                    number: tab.number,
                    label: tab.label.clone(),
                    focused: tab.focused,
                    panes: panes
                        .iter()
                        .filter(|pane| pane.tab_id == tab.tab_id)
                        .map(|pane| HerdrNativePane {
                            id: pane.pane_id.clone(),
                            label: pane
                                .label
                                .clone()
                                .or_else(|| pane.terminal_title_stripped.clone()),
                            agent: pane.agent.clone(),
                            status: pane
                                .agent_status
                                .clone()
                                .unwrap_or_else(|| "unknown".to_owned()),
                            cwd: pane.cwd.clone(),
                            focused: pane.focused,
                        })
                        .collect(),
                })
                .collect();
            HerdrNativeWorkspace {
                id: workspace.workspace_id,
                number: workspace.number,
                label: workspace.label,
                focused: workspace.focused,
                worktree: workspace.worktree,
                owner,
                tabs,
            }
        })
        .collect();
    HerdrNativeSession {
        name,
        socket: socket_path.to_string_lossy().into_owned(),
        running: true,
        workspaces,
    }
}

#[cfg(unix)]
fn collect_herdr_native_listing_with_ownership(
    ownership: &HerdrOwnership,
) -> Result<HerdrNativeListing, CliError> {
    let mut sessions = Vec::new();
    for session in workhive_infra::dependency::list_herdr_sessions()? {
        if !session.running {
            sessions.push(HerdrNativeSession {
                name: session.name,
                socket: session.socket_path.to_string_lossy().into_owned(),
                running: false,
                workspaces: Vec::new(),
            });
            continue;
        }
        let adapter =
            workhive_infra::herdr::HerdrRuntimeAdapter::connect(session.socket_path.clone())
                .map_err(CliError::RuntimeOp)?;
        let snapshot = adapter.session_snapshot().map_err(CliError::RuntimeOp)?;
        sessions.push(project_herdr_session(
            session.name,
            &session.socket_path,
            snapshot,
            ownership,
        ));
    }
    Ok(HerdrNativeListing { sessions })
}

#[cfg(unix)]
fn tree_branch(index: usize, len: usize) -> &'static str {
    if index + 1 == len { "└─" } else { "├─" }
}

#[cfg(unix)]
fn render_herdr_native_tree(listing: &HerdrNativeListing, max_width: usize) -> String {
    let mut lines = Vec::new();
    for (session_index, session) in listing.sessions.iter().enumerate() {
        lines.push(format!(
            "Herdr session: {}{}",
            session.name,
            if session.running { "" } else { " (stopped)" }
        ));
        for (workspace_index, workspace) in session.workspaces.iter().enumerate() {
            let workspace_branch = tree_branch(workspace_index, session.workspaces.len());
            let owner = workspace
                .owner
                .as_ref()
                .map(|owner| format!(" [Workhive {}/{}]", owner.project, owner.workspace))
                .unwrap_or_default();
            lines.push(format!(
                "{workspace_branch} Workspace {}: {}{}",
                workspace.id, workspace.label, owner
            ));
            if let Some(worktree) = &workspace.worktree {
                let linked = if worktree.is_linked_worktree {
                    " (linked)"
                } else {
                    ""
                };
                let worktree_branch = if workspace.tabs.is_empty() {
                    "└─"
                } else {
                    "├─"
                };
                lines.push(format!(
                    "   {worktree_branch} Worktree: {}{}",
                    worktree.checkout_path, linked
                ));
            }
            for (tab_index, tab) in workspace.tabs.iter().enumerate() {
                let tab_branch = tree_branch(tab_index, workspace.tabs.len());
                lines.push(format!("   {tab_branch} Tab {}: {}", tab.id, tab.label));
                for (pane_index, pane) in tab.panes.iter().enumerate() {
                    let pane_branch = tree_branch(pane_index, tab.panes.len());
                    let kind = pane.agent.as_deref().unwrap_or("unknown");
                    let label = pane.label.as_deref().unwrap_or("(unlabeled)");
                    let cwd = pane.cwd.as_deref().unwrap_or("?");
                    lines.push(format!(
                        "      {pane_branch} Pane {}: {} [{kind}, {}] {}",
                        pane.id, label, pane.status, cwd
                    ));
                }
            }
        }
        if session_index + 1 != listing.sessions.len() {
            lines.push(String::new());
        }
    }
    lines
        .into_iter()
        .map(|line| crate::table::truncate_to_width(&line, max_width))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(unix)]
fn collect_herdr_native_listing_if_available(
    projects: &[workhive_core::ports::DiscoveredProject],
    ctx: &CommandContext,
) -> Result<Option<HerdrNativeListing>, CliError> {
    let ownership = herdr_ownership(projects, ctx)?;
    collect_herdr_native_listing_if_available_with_ownership(&ownership)
}

#[cfg(unix)]
fn collect_herdr_native_listing_if_available_with_ownership(
    ownership: &HerdrOwnership,
) -> Result<Option<HerdrNativeListing>, CliError> {
    match collect_herdr_native_listing_with_ownership(ownership) {
        Ok(listing) => Ok(Some(listing)),
        Err(CliError::RuntimeOp(workhive_core::ports::RuntimeError::BackendUnavailable {
            ..
        })) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn global_herdr_configured(ctx: &CommandContext) -> Result<bool, CliError> {
    use workhive_core::ports::ConfigPort;
    Ok(ctx.config.load_global()?.is_some_and(|config| {
        config.backend == Some(workhive_core::domain::RuntimeBackendId::herdr())
    }))
}

#[cfg(unix)]
fn configured_herdr_projects(
    projects: &[workhive_core::ports::DiscoveredProject],
    ctx: &CommandContext,
) -> Result<bool, CliError> {
    for project in projects {
        if ctx.runtime_backend_for(Some(project.project_root.as_ref()))?
            == workhive_core::domain::RuntimeBackendId::herdr()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
fn collect_configured_herdr_native_listing(
    ctx: &CommandContext,
) -> Result<Option<HerdrNativeListing>, CliError> {
    let global_herdr = global_herdr_configured(ctx)?;
    let state = crate::commands::resolve_trusted_project_index(ctx)?;
    let projects = match state {
        crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => {
            let ws_fs = ctx.composition.workspace_fs();
            usecases::resolve_projects::resolve_discovered_projects(&projects, &ws_fs).valid
        }
        crate::commands::TrustedProjectIndexState::NotFound { .. }
        | crate::commands::TrustedProjectIndexState::Empty { .. } => Vec::new(),
    };
    if global_herdr || configured_herdr_projects(&projects, ctx)? {
        return collect_herdr_native_listing_if_available(&projects, ctx);
    }
    Ok(None)
}

#[cfg(unix)]
fn print_herdr_native_tree(listing: &HerdrNativeListing, ctx: &CommandContext) {
    eprintln!("{}", ctx.i18n.resolve(ctx.locale(), k::SESSIONS_HEADER));
    let width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    println!("{}", render_herdr_native_tree(listing, width));
}

#[cfg(unix)]
fn print_herdr_only_listing(listing: &HerdrNativeListing, view: ListView, ctx: &CommandContext) {
    if matches!(view, ListView::Json) {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "herdr_sessions": listing.sessions,
            }))
            .expect("Herdr session JSON must serialize")
        );
    } else if listing.sessions.is_empty() {
        eprintln!("{}", ctx.i18n.resolve(ctx.locale(), k::SESSIONS_EMPTY));
    } else {
        print_herdr_native_tree(listing, ctx);
    }
}

#[cfg(unix)]
#[derive(Debug, Clone)]
enum SessionAttachTarget {
    HerdrSession(String),
    WorkspaceBinding(SessionRecord),
}

#[cfg(unix)]
struct SessionAttachSelectorOptions {
    labels: Vec<String>,
    disabled: Vec<bool>,
    targets: Vec<SessionAttachTarget>,
}

#[cfg(unix)]
fn build_session_attach_selector(
    listing: Option<&HerdrNativeListing>,
    records: &[SessionRecord],
    i18n: &workhive_infra::i18n::StaticI18n,
    locale: workhive_core::domain::Locale,
) -> SessionAttachSelectorOptions {
    let mut rows = Vec::new();
    let mut targets = Vec::new();

    if let Some(listing) = listing {
        let mut herdr_sessions: Vec<_> = listing
            .sessions
            .iter()
            .filter(|session| session.running)
            .collect();
        herdr_sessions.sort_by(|left, right| left.name.cmp(&right.name));
        for session in herdr_sessions {
            let count = session.workspaces.len().to_string();
            rows.push(vec![
                "Herdr".to_owned(),
                session.name.clone(),
                i18n.resolve_with_args(locale, k::SESSION_ATTACH_HERDR_DETAILS, &[&count]),
            ]);
            targets.push(SessionAttachTarget::HerdrSession(session.name.clone()));
        }
    }

    let mut records: Vec<_> = records
        .iter()
        .filter(|record| {
            listing.is_none()
                || record.binding.backend == workhive_core::domain::RuntimeBackendId::tmux()
        })
        .collect();
    records.sort_by(|left, right| {
        (
            &left.project_name,
            &left.workspace,
            &left.binding.workspace_id,
        )
            .cmp(&(
                &right.project_name,
                &right.workspace,
                &right.binding.workspace_id,
            ))
    });
    for record in records {
        let backend = if record.binding.backend == workhive_core::domain::RuntimeBackendId::herdr()
        {
            "Herdr".to_owned()
        } else {
            record.binding.backend.to_string()
        };
        let session = record
            .runtime_label
            .as_deref()
            .unwrap_or(&record.binding.workspace_id);
        let path = display_path_for_humans(camino::Utf8Path::new(&record.project_root));
        rows.push(vec![
            backend,
            format!("{}/{}", record.project_name, record.workspace),
            format!("{session} · {path}"),
        ]);
        targets.push(SessionAttachTarget::WorkspaceBinding(record.clone()));
    }

    let columns = [
        Column {
            header: i18n.resolve(locale, k::SESSIONS_COL_BACKEND),
            min_width: 0,
        },
        Column {
            header: i18n.resolve(locale, k::SESSION_ATTACH_COL_TARGET),
            min_width: 0,
        },
        Column {
            header: i18n.resolve(locale, k::SESSION_ATTACH_COL_DETAILS),
            min_width: 0,
        },
    ];
    let labels = format_table_lines(&columns, &rows);
    let mut disabled = vec![true; labels.len() - targets.len()];
    disabled.extend(std::iter::repeat_n(false, targets.len()));

    SessionAttachSelectorOptions {
        labels,
        disabled,
        targets,
    }
}

#[cfg(unix)]
fn attach_session_selector(
    listing: Option<&HerdrNativeListing>,
    records: &[SessionRecord],
    ctx: &CommandContext,
) -> Result<bool, CliError> {
    let options = build_session_attach_selector(listing, records, &ctx.i18n, ctx.locale());
    if options.targets.is_empty() {
        return Ok(false);
    }

    let title = ctx.i18n.resolve(ctx.locale(), k::SESSION_ATTACH_SELECT);
    let index = ui::screens::select::run_with_disabled_data_index(
        &title,
        &options.labels,
        &options.disabled,
        0,
    )?;
    match &options.targets[index] {
        SessionAttachTarget::HerdrSession(name) => {
            workhive_infra::herdr::attach_session(name).map_err(CliError::RuntimeOp)?;
        }
        SessionAttachTarget::WorkspaceBinding(record) => perform_session_attach(record, ctx)?,
    }
    Ok(true)
}

#[cfg(unix)]
fn attach_native_herdr_session(
    target: Option<&str>,
    listing: &HerdrNativeListing,
) -> Result<bool, CliError> {
    let Some(target) = target else {
        return Ok(false);
    };
    let Some(session) = listing
        .sessions
        .iter()
        .find(|session| session.running && session.name == target)
    else {
        return Ok(false);
    };
    workhive_infra::herdr::attach_session(&session.name).map_err(CliError::RuntimeOp)?;
    Ok(true)
}

/// `workhive session list [--view auto|table|compact|json]` — global runtime
/// workspace listing across every project in the index.
///
/// Behaviour rules:
/// - Tmux keeps its existing flat project/workspace presentation.
/// - Herdr uses native session → workspace → tab → pane hierarchy.
/// - The backend is always explicit; no backend fallback is attempted.
/// - JSON keeps absolute paths; human views abbreviate `$HOME` to `~/...`.
pub(crate) fn cmd_sessions(view: ListView, ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;
    let locale = ctx.locale();

    let project_summaries = match crate::commands::resolve_trusted_project_index(ctx)? {
        state @ (crate::commands::TrustedProjectIndexState::NotFound { .. }
        | crate::commands::TrustedProjectIndexState::Empty { .. }) => {
            #[cfg(unix)]
            if global_herdr_configured(ctx)?
                && let Some(native) = collect_herdr_native_listing_if_available(&[], ctx)?
            {
                print_herdr_only_listing(&native, view, ctx);
                return Ok(());
            }
            if matches!(view, ListView::Json) {
                emit_sessions_json(&[]);
                return Ok(());
            }
            match state {
                crate::commands::TrustedProjectIndexState::NotFound { setup } => {
                    print_setup_state_hint(&setup, ctx, locale);
                }
                crate::commands::TrustedProjectIndexState::Empty { setup } => match setup {
                    DiscoverySetupState::Ready => {
                        eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_EMPTY));
                    }
                    other => print_setup_state_hint(&other, ctx, locale),
                },
                _ => unreachable!(),
            }
            return Ok(());
        }
        crate::commands::TrustedProjectIndexState::Loaded { projects, .. } => projects,
    };

    let ws_fs = ctx.composition.workspace_fs();
    let resolution =
        usecases::resolve_projects::resolve_discovered_projects(&project_summaries, &ws_fs);

    if !matches!(view, ListView::Json) {
        render_stale_entries(&resolution.stale, ctx, locale);
    }

    #[cfg(unix)]
    {
        let ownership = herdr_ownership(&resolution.valid, ctx)?;
        if (global_herdr_configured(ctx)?
            || configured_herdr_projects(&resolution.valid, ctx)?
            || !ownership.is_empty())
            && let Some(native) =
                collect_herdr_native_listing_if_available_with_ownership(&ownership)?
        {
            let listing = list_global_sessions_by_backend(&resolution.valid, ctx)?;
            let tmux_records: Vec<_> = listing
                .records
                .iter()
                .filter(|record| {
                    record.binding.backend != workhive_core::domain::RuntimeBackendId::herdr()
                })
                .cloned()
                .collect();
            if matches!(view, ListView::Json) {
                let sessions = render_sessions_value(&tmux_records);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "herdr_sessions": native.sessions,
                        "sessions": sessions["sessions"],
                    }))
                    .expect("mixed session JSON must serialize")
                );
                return Ok(());
            }
            log_unavailable_projects(&listing.unavailable, ctx, locale);
            if native.sessions.is_empty() && tmux_records.is_empty() {
                eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_EMPTY));
                return Ok(());
            }
            let mut printed_header = false;
            if !native.sessions.is_empty() {
                print_herdr_native_tree(&native, ctx);
                printed_header = true;
            }
            if !tmux_records.is_empty() {
                if !printed_header {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_HEADER));
                }
                match resolve_view(view) {
                    ResolvedView::Table => print_sessions_table(&tmux_records, ctx, locale, true),
                    ResolvedView::Compact => print_sessions_compact(&tmux_records, ctx, locale),
                }
            }
            return Ok(());
        }
    }

    let listing = list_global_sessions_by_backend(&resolution.valid, ctx)?;

    if matches!(view, ListView::Json) {
        // JSON output stays backward compatible: it carries only reachable
        // session records. Projects whose backend was unreachable are omitted
        // rather than failing the command or altering the schema.
        emit_sessions_json(&listing.records);
        return Ok(());
    }

    // Unreachable per-project tmux backends are non-actionable for normal
    // session discovery. Log details at debug for diagnostics only.
    log_unavailable_projects(&listing.unavailable, ctx, locale);

    if listing.records.is_empty() {
        eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_EMPTY));
        return Ok(());
    }

    let resolved = resolve_view(view);
    eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_HEADER));
    match resolved {
        ResolvedView::Table => print_sessions_table(&listing.records, ctx, locale, true),
        ResolvedView::Compact => print_sessions_compact(&listing.records, ctx, locale),
    }

    Ok(())
}

/// Resolved auto/table/compact mode.
///
/// The Json branch is handled before this is reached.
enum ResolvedView {
    Table,
    Compact,
}

fn resolve_view(view: ListView) -> ResolvedView {
    match view {
        ListView::Json => ResolvedView::Compact, // unreachable from caller
        ListView::Table => ResolvedView::Table,
        ListView::Compact => ResolvedView::Compact,
        ListView::Auto => match terminal_size::terminal_size() {
            Some((w, _)) if w.0 >= 100 => ResolvedView::Table,
            _ => ResolvedView::Compact,
        },
    }
}

fn print_sessions_table(
    records: &[SessionRecord],
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    include_client: bool,
) {
    let mut columns = vec![
        Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_PROJECT),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_BACKEND),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_WORKSPACE),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_PATH),
            min_width: 0,
        },
        Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_SESSION),
            min_width: 0,
        },
    ];
    if include_client {
        columns.push(Column {
            header: ctx.i18n.resolve(locale, k::SESSIONS_COL_CLIENT),
            min_width: 0,
        });
    }

    let rows: Vec<Vec<String>> = records
        .iter()
        .map(|r| {
            let mut row = vec![
                r.project_name.clone(),
                r.binding.backend.to_string(),
                r.workspace.clone(),
                display_path_for_humans(camino::Utf8Path::new(&r.project_root)),
                r.runtime_label
                    .as_deref()
                    .unwrap_or(&r.binding.workspace_id)
                    .to_owned(),
            ];
            if include_client {
                row.push(client_label(r.attached, ctx, locale));
            }
            row
        })
        .collect();

    print_table(&columns, &rows);
}

fn print_sessions_compact(
    records: &[SessionRecord],
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) {
    // Compact view trades the explicit "client" column for a star marker,
    // matching how `workspace list --view compact` annotates active sessions.
    let last_idx = records.len().saturating_sub(1);
    for (idx, r) in records.iter().enumerate() {
        let attached_marker = if r.attached { " *" } else { "" };
        println!("{} / {}{}", r.project_name, r.workspace, attached_marker);
        println!(
            "  {}: {}",
            ctx.i18n.resolve(locale, k::SESSIONS_COL_BACKEND),
            r.binding.backend
        );
        println!(
            "  {}: {}",
            ctx.i18n.resolve(locale, k::SESSIONS_COL_PATH),
            display_path_for_humans(camino::Utf8Path::new(&r.project_root))
        );
        println!(
            "  {}: {}",
            ctx.i18n.resolve(locale, k::SESSIONS_COL_SESSION),
            r.runtime_label
                .as_deref()
                .unwrap_or(&r.binding.workspace_id)
        );
        if idx != last_idx {
            println!();
        }
    }
}

fn client_label(
    attached: bool,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> String {
    let key = if attached {
        k::SESSIONS_ATTACHED_YES
    } else {
        k::SESSIONS_ATTACHED_NO
    };
    ctx.i18n.resolve(locale, key)
}

/// Log per-project diagnostic details at debug level for projects whose
/// runtime backend was unreachable during the global sweep.
///
/// These details are not emitted as normal stderr warnings. They are
/// available only through debug-level tracing subscribers or log files.
/// The localized reason is resolved from the typed
/// [`workhive_core::ports::BackendUnavailableReason`] so the diagnostic
/// explains why each project was skipped (missing socket, stopped server, …).
fn log_unavailable_projects(
    unavailable: &[UnavailableProject],
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) {
    for project in unavailable {
        let reason_args = project.reason.i18n_args();
        let reason_refs: Vec<&str> = reason_args.iter().map(String::as_str).collect();
        let reason = ctx
            .i18n
            .resolve_with_args(locale, project.reason.i18n_key(), &reason_refs);
        tracing::debug!(
            project = project.project_name.as_str(),
            backend = project.backend.as_str(),
            reason = reason.as_str(),
            "session list: skipping unavailable project"
        );
    }
}

/// Render the `session list --view json` document.
///
/// JSON schema:
///   { sessions: [{ project, backend, workspace, project_root, scope_id,
///                 runtime_id, runtime_label, attached, status }] }
/// `status` is the lifecycle state of the underlying runtime workspace as
/// observed by Workhive. Today every workspace that surfaces in the listing
/// was returned by `list_workspaces`, so it is always "running"; the field
/// exists so future expansion does not break the payload contract.
fn render_sessions_value(records: &[SessionRecord]) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "project": r.project_name,
                "backend": r.binding.backend.as_str(),
                "workspace": r.workspace,
                "project_root": r.project_root,
                "scope_id": r.binding.scope_id,
                "runtime_id": r.binding.workspace_id,
                "runtime_label": r.runtime_label,
                "attached": r.attached,
                "status": "running",
            })
        })
        .collect();

    serde_json::json!({
        "sessions": rows,
    })
}

fn render_sessions_json(records: &[SessionRecord]) -> String {
    serde_json::to_string_pretty(&render_sessions_value(records))
        .expect("JSON serialization must succeed")
}

fn emit_sessions_json(records: &[SessionRecord]) {
    println!("{}", render_sessions_json(records));
}

// ─────────────────────────────────────────────────────────────────────────────
// `workhive session attach`
// ─────────────────────────────────────────────────────────────────────────────

/// `workhive session attach [TARGET]` — global session attach.
///
/// This command intentionally does **not** detect or initialize a project. A
/// Herdr target is handed to Herdr's native whole-session attach command;
/// project-scoped targets still resolve through persisted Workhive bindings.
///
/// Resolution rules:
/// - Herdr session names are selected from native session discovery.
/// - No target and stdin is a TTY → one selector lists available Herdr sessions
///   and persisted workspace bindings with an explicit backend column.
/// - No target and stdin is non-interactive → persisted binding fallback.
/// - `project/workspace` and workspace-only targets remain binding-based.
pub(crate) fn cmd_session_attach(
    target: Option<&str>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();

    #[cfg(unix)]
    {
        let native = collect_configured_herdr_native_listing(ctx)?;
        if target.is_none() && std::io::stdin().is_terminal() {
            let records = if native.is_some() {
                collect_global_sessions_if_indexed(ctx)?
            } else {
                collect_global_sessions(ctx)?
            };
            if attach_session_selector(native.as_ref(), &records, ctx)? {
                return Ok(());
            }
        }
        if let Some(native) = native
            && attach_native_herdr_session(target, &native)?
        {
            return Ok(());
        }
    }

    let records = collect_global_sessions(ctx)?;
    if records.is_empty() {
        return Err(CliError::Runtime(
            ctx.i18n.resolve(locale, k::SESSION_ATTACH_NO_SESSIONS),
        ));
    }

    let chosen: SessionRecord = match target {
        None => {
            if !std::io::stdin().is_terminal() {
                return Err(CliError::Runtime(
                    ctx.i18n.resolve(locale, k::SESSION_ATTACH_NO_TTY),
                ));
            }
            // Session-first responsive selector shared with `workspace
            // attach`'s global fallback: session id leads every row,
            // followed by project / workspace / path / attached marker
            // columns that drop right-to-left on narrow terminals.
            let plan = build_global_attach_plan(&records, &ctx.i18n, locale);
            let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
            let (labels, disabled) =
                render_global_attach_lines(&plan, term_width, &ctx.i18n, locale);
            let title = ctx.i18n.resolve(locale, k::SESSION_ATTACH_SELECT);
            let default_idx = disabled.iter().position(|d| !d).unwrap_or(0);
            let row_index = ui::screens::select::run_with_disabled_data_index(
                &title,
                &labels,
                &disabled,
                default_idx,
            )?;
            records[row_index].clone()
        }
        Some(query) => resolve_session_target(query, &records, ctx)?,
    };

    perform_session_attach(&chosen, ctx)?;
    Ok(())
}

/// Load the project index and surface running tmux sessions across projects.
///
/// Routes the missing-index / empty-index cases through the same
/// setup-state guidance model that `session list` uses: instead of a
/// terse "no index built" line, the user sees the same trusted-roots /
/// `init` / `reindex` hint resolved from
/// [`workhive_core::usecases::trusted_roots::DiscoverySetupState`]. The
/// resolved hint message is returned as `CliError::Runtime` so the CLI
/// exits non-zero — attach cannot proceed when there are no projects.
fn collect_global_sessions(ctx: &CommandContext) -> Result<Vec<SessionRecord>, CliError> {
    let locale = ctx.locale();

    let summaries = match crate::commands::resolve_trusted_project_index(ctx)? {
        crate::commands::TrustedProjectIndexState::NotFound { setup }
        | crate::commands::TrustedProjectIndexState::Empty { setup } => {
            return Err(CliError::Runtime(setup_state_to_message(
                &setup, ctx, locale,
            )));
        }
        crate::commands::TrustedProjectIndexState::Loaded { projects } => projects,
    };

    collect_global_sessions_for_projects(&summaries, ctx)
}

#[cfg(unix)]
fn collect_global_sessions_if_indexed(
    ctx: &CommandContext,
) -> Result<Vec<SessionRecord>, CliError> {
    match crate::commands::resolve_trusted_project_index(ctx)? {
        crate::commands::TrustedProjectIndexState::NotFound { .. }
        | crate::commands::TrustedProjectIndexState::Empty { .. } => Ok(Vec::new()),
        crate::commands::TrustedProjectIndexState::Loaded { projects } => {
            collect_global_sessions_for_projects(&projects, ctx)
        }
    }
}

fn collect_global_sessions_for_projects(
    summaries: &[workhive_core::ports::ProjectSummary],
    ctx: &CommandContext,
) -> Result<Vec<SessionRecord>, CliError> {
    let locale = ctx.locale();
    let ws_fs = ctx.composition.workspace_fs();
    let resolution = usecases::resolve_projects::resolve_discovered_projects(summaries, &ws_fs);
    render_stale_entries(&resolution.stale, ctx, locale);

    let listing = list_global_sessions_by_backend(&resolution.valid, ctx)?;
    // Attach selects among existing sessions, so unreachable backends carry no
    // attachable targets. Record them for diagnostics without aborting attach.
    for project in &listing.unavailable {
        tracing::debug!(
            project = %project.project_name,
            detail = %project.detail,
            "skipped unreachable runtime backend while collecting sessions for attach"
        );
    }
    Ok(listing.records)
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared setup-state helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Resolve a [`DiscoverySetupState`] to a localized message string for use
/// as a `CliError::Runtime` payload.
///
/// This mirrors the keys printed by `print_setup_state_hint` so that
/// `session attach`'s failure copy is byte-equivalent to the hint
/// `session list` prints; the "Ready" branch (used when an index is
/// loaded with zero projects) maps to `SESSIONS_EMPTY` because no
/// trusted-roots / init / reindex action would help.
fn setup_state_to_message(
    setup: &workhive_core::usecases::trusted_roots::DiscoverySetupState,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> String {
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;

    let key = match setup {
        DiscoverySetupState::NoTrustedRootsConfigured => k::TRUSTED_ROOTS_NONE_CONFIGURED,
        DiscoverySetupState::TrustedRootsConfiguredIndexMissing => k::TRUSTED_ROOTS_INDEX_MISSING,
        DiscoverySetupState::TrustedRootsConfiguredIndexEmpty => k::TRUSTED_ROOTS_INDEX_EMPTY,
        DiscoverySetupState::Ready => k::SESSIONS_EMPTY,
    };
    ctx.i18n.resolve(locale, key)
}

/// Pure resolution result — exposed for unit testing.
///
/// The CLI wrapper maps `Ambiguous` and `NotFound` onto localized
/// `CliError::Runtime` messages; `Resolved` is the happy path.
#[derive(Debug, Clone)]
pub(crate) enum SessionTargetResolution {
    Resolved(SessionRecord),
    /// Multiple records matched a workspace-only query; carries the
    /// `project/workspace` candidate strings so the CLI can render them in a
    /// user-facing disambiguation hint.
    Ambiguous {
        candidates: Vec<String>,
    },
    NotFound,
}

/// Resolve the user's `target` against `records` without consulting i18n —
/// this lets us unit-test the matching rules without standing up a full
/// `CommandContext`.
pub(crate) fn resolve_session_target_pure(
    query: &str,
    records: &[SessionRecord],
) -> SessionTargetResolution {
    let match_candidates = |matches: Vec<&SessionRecord>| match matches.as_slice() {
        [] => None,
        [rec] => Some(SessionTargetResolution::Resolved((*rec).clone())),
        many => Some(SessionTargetResolution::Ambiguous {
            candidates: many
                .iter()
                .map(|r| format!("{}/{}", r.project_name, r.workspace))
                .collect(),
        }),
    };

    if let Some(res) = match_candidates(
        records
            .iter()
            .filter(|r| r.binding.workspace_id == query)
            .collect(),
    ) {
        return res;
    }

    if let Some((proj, ws)) = query.split_once('/') {
        let trimmed_proj = proj.trim();
        let trimmed_ws = ws.trim();
        if !trimmed_proj.is_empty()
            && !trimmed_ws.is_empty()
            && let Some(res) = match_candidates(
                records
                    .iter()
                    .filter(|r| r.project_name == trimmed_proj && r.workspace == trimmed_ws)
                    .collect(),
            )
        {
            return res;
        }
    }

    match_candidates(records.iter().filter(|r| r.workspace == query).collect())
        .unwrap_or(SessionTargetResolution::NotFound)
}

pub(crate) fn resolve_session_target(
    query: &str,
    records: &[SessionRecord],
    ctx: &CommandContext,
) -> Result<SessionRecord, CliError> {
    let locale = ctx.locale();
    match resolve_session_target_pure(query, records) {
        SessionTargetResolution::Resolved(rec) => Ok(rec),
        SessionTargetResolution::NotFound => Err(CliError::ResourceNotFound(
            ctx.i18n
                .resolve_with_args(locale, k::SESSION_ATTACH_NOT_FOUND, &[query]),
        )),
        SessionTargetResolution::Ambiguous { candidates } => {
            let joined = candidates.join(", ");
            Err(CliError::Runtime(ctx.i18n.resolve_with_args(
                locale,
                k::SESSION_ATTACH_AMBIGUOUS,
                &[query, &joined],
            )))
        }
    }
}

/// Print concise tmux attach guidance before handing terminal control.
///
/// Shared across `session attach`, `workspace attach`, and attach-capable
/// `workspace open` so users always see how to detach/return and close/exit.
pub(crate) fn print_attach_guidance(ctx: &CommandContext) {
    let locale = ctx.locale();
    eprintln!("{}", ctx.i18n.resolve(locale, k::ATTACH_GUIDANCE));
}

/// Print localized informational message when already in the target workspace.
pub(crate) fn print_already_current(ctx: &CommandContext) {
    let locale = ctx.locale();
    eprintln!("{}", ctx.i18n.resolve(locale, k::ATTACH_ALREADY_CURRENT));
}

/// Underlying attach helper — calls into the runtime port.
///
/// Kept private to this module so `workspace attach` retains its own,
/// project-aware, attach pipeline (with legacy fallback). This helper assumes
/// the caller has already validated the (server, session) pair against the
/// global session listing, so no fallback rewrite is needed here.
fn perform_session_attach(record: &SessionRecord, ctx: &CommandContext) -> Result<(), CliError> {
    // Session validation is backend-specific and no longer needed here

    let binding = record.binding.clone();
    let runtime = ctx.composition.runtime_for_binding(&binding)?;

    let env = ctx.composition.environment();
    let exec_ctx = env.execution_context()?;
    let locale = ctx.locale();

    let request = crate::commands::build_attach_request(&ctx.config, record)?;
    crate::commands::attach_usage::mark_used_then_attach_binding(
        runtime.as_ref(),
        &binding,
        request,
        &exec_ctx,
        crate::commands::attach_usage::AttachLifecycleCallbacks {
            mark_used: || {},         // No usage metadata for session attach
            emit_ready_output: || {}, // No ready output for session attach (guidance is the ready signal)
            emit_guidance: || print_attach_guidance(ctx),
            emit_already_current: || print_already_current(ctx),
        },
    )
    .map_err(|e| {
        let cli_err = crate::commands::attach_usage::map_navigation_error(e, &ctx.i18n, locale);
        crate::commands::close::map_attach_backend_unavailable(cli_err, ctx)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::RuntimeBinding;

    fn rec(project: &str, ws: &str, root: &str, session: &str, server: &str) -> SessionRecord {
        SessionRecord {
            binding: RuntimeBinding::tmux(server, session),
            project_name: project.to_owned(),
            project_root: root.to_owned(),
            workspace: ws.to_owned(),
            runtime_label: None,
            attached: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn mixed_attach_selector_shows_backend_and_keeps_each_target() {
        use workhive_core::domain::Locale;
        use workhive_infra::i18n::StaticI18n;

        let listing = HerdrNativeListing {
            sessions: vec![HerdrNativeSession {
                name: "workhive".to_owned(),
                socket: "/run/herdr/workhive.sock".to_owned(),
                running: true,
                workspaces: vec![HerdrNativeWorkspace {
                    id: "w1".to_owned(),
                    number: 1,
                    label: "main".to_owned(),
                    focused: true,
                    worktree: None,
                    owner: None,
                    tabs: Vec::new(),
                }],
            }],
        };
        let mut tmux = rec("demo", "main", "/demo", "wh-demo-main", "default");
        tmux.runtime_label = Some("wh@demo".to_owned());
        let i18n = StaticI18n::new();
        let options =
            build_session_attach_selector(Some(&listing), &[tmux.clone()], &i18n, Locale::En);

        assert_eq!(options.targets.len(), 2);
        assert_eq!(options.disabled, vec![true, true, false, false]);
        assert!(options.labels[0].contains("Backend"));
        assert!(options.labels[0].contains("Session"));
        assert!(options.labels[2].contains("Herdr"));
        assert!(options.labels[2].contains("workhive"));
        assert!(options.labels[2].contains("1 workspace(s)"));
        assert!(options.labels[3].contains("tmux"));
        assert!(options.labels[3].contains("demo/main"));
        assert!(options.labels[3].contains("wh@demo"));
        assert!(matches!(
            &options.targets[0],
            SessionAttachTarget::HerdrSession(name) if name == "workhive"
        ));
        assert!(matches!(
            &options.targets[1],
            SessionAttachTarget::WorkspaceBinding(record)
                if record.binding == RuntimeBinding::tmux("default", "wh-demo-main")
        ));
        let zh_options =
            build_session_attach_selector(Some(&listing), &[tmux], &i18n, Locale::ZhCn);
        assert!(zh_options.labels[0].contains("后端"));
        assert!(zh_options.labels[0].contains("会话"));
    }

    /// Pins the `session list --view json` schema: agents and scripts
    /// depend on the exact key set of the envelope and each row.
    #[test]
    fn sessions_json_schema_is_stable() {
        let records = vec![rec(
            "alpha",
            "feature",
            "/Users/u/alpha",
            "wh@xyz@alpha@feature",
            "whsrv-xyz-alpha",
        )];
        let rendered = render_sessions_json(&records);
        let value: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered sessions JSON must parse");

        let sessions = value
            .get("sessions")
            .and_then(|v| v.as_array())
            .expect("payload must carry a `sessions` array");
        assert_eq!(sessions.len(), 1);

        let row = sessions[0].as_object().expect("row must be an object");
        let mut keys: Vec<&str> = row.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "attached",
                "backend",
                "project",
                "project_root",
                "runtime_id",
                "runtime_label",
                "scope_id",
                "status",
                "workspace",
            ],
            "session list --view json row keys are a stable machine contract"
        );
        assert_eq!(row["project"], "alpha");
        assert_eq!(row["backend"], "tmux");
        assert_eq!(row["workspace"], "feature");
        assert_eq!(row["attached"], false);
        assert_eq!(row["status"], "running");
    }

    #[cfg(unix)]
    #[test]
    fn herdr_projection_requires_exact_scope_and_workspace_binding() {
        let snapshot: workhive_infra::herdr::HerdrSessionSnapshot = serde_json::from_value(
            serde_json::json!({
                "version": "0.9.1",
                "protocol": 22,
                "workspaces": [
                    {
                        "workspace_id": "owned",
                        "number": 0,
                        "label": "owned",
                        "focused": false,
                        "pane_count": 1,
                        "tab_count": 1,
                        "active_tab_id": "owned:t1",
                        "agent_status": "unknown"
                    },
                    {
                        "workspace_id": "unowned",
                        "number": 1,
                        "label": "unowned",
                        "focused": false,
                        "pane_count": 1,
                        "tab_count": 1,
                        "active_tab_id": "unowned:t1",
                        "agent_status": "unknown"
                    }
                ],
                "tabs": [
                    {"tab_id":"owned:t1","workspace_id":"owned","number":0,"label":"main","focused":true,"pane_count":1,"agent_status":"unknown"},
                    {"tab_id":"unowned:t1","workspace_id":"unowned","number":0,"label":"main","focused":false,"pane_count":1,"agent_status":"unknown"}
                ],
                "panes": [
                    {"pane_id":"owned:p1","terminal_id":"term-1","workspace_id":"owned","tab_id":"owned:t1","focused":true,"revision":1,"cwd":"/repo"},
                    {"pane_id":"unowned:p1","terminal_id":"term-2","workspace_id":"unowned","tab_id":"unowned:t1","focused":false,"revision":1}
                ]
            }),
        )
        .unwrap();
        let socket = std::path::Path::new("/run/herdr/shared.sock");
        let mut ownership = HashMap::new();
        ownership.insert(
            (socket.to_string_lossy().into_owned(), "owned".to_owned()),
            vec![HerdrWorkspaceOwner {
                project: "repo".to_owned(),
                workspace: "feature".to_owned(),
                project_root: "/repo".to_owned(),
            }],
        );

        let projected = project_herdr_session("shared".to_owned(), socket, snapshot, &ownership);
        assert!(projected.workspaces[0].owner.is_some());
        assert!(projected.workspaces[1].owner.is_none());
        assert_eq!(projected.workspaces[1].tabs[0].panes[0].status, "unknown");
    }

    #[cfg(unix)]
    #[test]
    fn herdr_tree_keeps_native_hierarchy_and_unknown_observations() {
        let listing = HerdrNativeListing {
            sessions: vec![
                HerdrNativeSession {
                    name: "shared".to_owned(),
                    socket: "/run/herdr/shared.sock".to_owned(),
                    running: true,
                    workspaces: vec![
                        HerdrNativeWorkspace {
                            id: "w1".to_owned(),
                            number: 1,
                            label: "repo".to_owned(),
                            focused: true,
                            worktree: Some(workhive_infra::herdr::HerdrWorktreeSnapshot {
                                repo_key: "repo-key".to_owned(),
                                repo_name: "repo".to_owned(),
                                repo_root: "/src/repo".to_owned(),
                                checkout_path: "/src/repo/.worktree/feature".to_owned(),
                                is_linked_worktree: true,
                            }),
                            owner: Some(HerdrWorkspaceOwner {
                                project: "repo".to_owned(),
                                workspace: "feature".to_owned(),
                                project_root: "/src/repo".to_owned(),
                            }),
                            tabs: vec![HerdrNativeTab {
                                id: "w1:t1".to_owned(),
                                number: 1,
                                label: "coding".to_owned(),
                                focused: true,
                                panes: vec![HerdrNativePane {
                                    id: "w1:p1".to_owned(),
                                    label: Some("shell".to_owned()),
                                    agent: None,
                                    status: "unknown".to_owned(),
                                    cwd: None,
                                    focused: false,
                                }],
                            }],
                        },
                        HerdrNativeWorkspace {
                            id: "w2".to_owned(),
                            number: 2,
                            label: "ordinary".to_owned(),
                            focused: false,
                            worktree: None,
                            owner: None,
                            tabs: vec![HerdrNativeTab {
                                id: "w2:t1".to_owned(),
                                number: 1,
                                label: "main".to_owned(),
                                focused: false,
                                panes: vec![],
                            }],
                        },
                    ],
                },
                HerdrNativeSession {
                    name: "other".to_owned(),
                    socket: "/run/herdr/other.sock".to_owned(),
                    running: false,
                    workspaces: Vec::new(),
                },
            ],
        };

        let rendered = render_herdr_native_tree(&listing, 200);
        assert!(rendered.contains("Herdr session: shared"));
        assert!(rendered.contains("Workspace w1: repo [Workhive repo/feature]"));
        assert!(rendered.contains("Worktree: /src/repo/.worktree/feature (linked)"));
        assert!(rendered.contains("Pane w1:p1: shell [unknown, unknown] ?"));
        assert!(rendered.contains("Workspace w2: ordinary"));
        assert!(rendered.contains("Herdr session: other (stopped)"));
        assert!(!rendered.contains("Worktree: ordinary"));

        for line in render_herdr_native_tree(&listing, 32).lines() {
            assert!(
                crate::table::display_width(line) <= 32,
                "tree line exceeds narrow width: {line:?}"
            );
        }
        let json = serde_json::to_value(&listing).unwrap();
        assert_eq!(
            json["sessions"][0]["workspaces"][1]["owner"],
            serde_json::Value::Null
        );
        assert_eq!(json["sessions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn full_session_name_matches() {
        let records = vec![rec(
            "alpha",
            "feature",
            "/Users/u/alpha",
            "wh@xyz@alpha@feature",
            "whsrv-xyz-alpha",
        )];
        match resolve_session_target_pure("wh@xyz@alpha@feature", &records) {
            SessionTargetResolution::Resolved(rec) => {
                assert_eq!(rec.binding.workspace_id, "wh@xyz@alpha@feature")
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn project_workspace_matches() {
        let records = vec![rec(
            "alpha",
            "feature",
            "/Users/u/alpha",
            "wh@xyz@alpha@feature",
            "whsrv-xyz-alpha",
        )];
        match resolve_session_target_pure("alpha/feature", &records) {
            SessionTargetResolution::Resolved(rec) => {
                assert_eq!(rec.project_name, "alpha");
                assert_eq!(rec.workspace, "feature");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn workspace_only_unique_match() {
        let records = vec![rec(
            "alpha",
            "feature",
            "/Users/u/alpha",
            "wh@xyz@alpha@feature",
            "whsrv-xyz-alpha",
        )];
        match resolve_session_target_pure("feature", &records) {
            SessionTargetResolution::Resolved(rec) => assert_eq!(rec.workspace, "feature"),
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn workspace_only_ambiguous_returns_candidates() {
        let records = vec![
            rec(
                "alpha",
                "feature",
                "/Users/u/alpha",
                "wh@xyz@alpha@feature",
                "whsrv-xyz-alpha",
            ),
            rec(
                "beta",
                "feature",
                "/Users/u/beta",
                "wh@xyz@beta@feature",
                "whsrv-xyz-beta",
            ),
        ];
        match resolve_session_target_pure("feature", &records) {
            SessionTargetResolution::Ambiguous { candidates } => {
                assert!(candidates.contains(&"alpha/feature".to_string()));
                assert!(candidates.contains(&"beta/feature".to_string()));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn runtime_label_is_display_only() {
        let mut record = rec("alpha", "main", "/Users/u/alpha", "wA", "/tmp/herdr.sock");
        record.runtime_label = Some("friendly-label".to_string());

        assert!(matches!(
            resolve_session_target_pure("friendly-label", &[record]),
            SessionTargetResolution::NotFound
        ));
    }

    #[test]
    fn duplicate_runtime_id_is_ambiguous_across_scopes() {
        let records = vec![
            rec("alpha", "main", "/a", "w1", "/run/a.sock"),
            rec("beta", "dev", "/b", "w1", "/run/b.sock"),
        ];

        match resolve_session_target_pure("w1", &records) {
            SessionTargetResolution::Ambiguous { candidates } => {
                assert!(candidates.contains(&"alpha/main".to_owned()));
                assert!(candidates.contains(&"beta/dev".to_owned()));
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_backend_resolution_is_not_downgraded_to_unavailable() {
        let error = runtime_resolution_error(CliError::Runtime(
            "runtime backend 'unknown' is unavailable".to_owned(),
        ));

        assert!(matches!(
            error,
            workhive_core::ports::RuntimeError::OperationFailed { .. }
        ));
    }

    #[test]
    fn unknown_target_returns_not_found() {
        let records = vec![rec(
            "alpha",
            "feature",
            "/Users/u/alpha",
            "wh@xyz@alpha@feature",
            "whsrv-xyz-alpha",
        )];
        assert!(matches!(
            resolve_session_target_pure("missing", &records),
            SessionTargetResolution::NotFound
        ));
    }

    /// Attach guidance i18n entries must describe multi-pane semantics:
    /// `exit` closes the current pane, the session ends when all panes
    /// exit. Both locales must mention `Ctrl-b` and `exit` and
    /// `workhive workspace close`.
    #[test]
    fn attach_guidance_describes_multi_pane_semantics() {
        use workhive_core::domain::Locale;
        use workhive_core::i18n_keys as ik;
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        let en = i18n.resolve(Locale::En, ik::ATTACH_GUIDANCE);
        let zh = i18n.resolve(Locale::ZhCn, ik::ATTACH_GUIDANCE);

        for (locale_name, text) in [("EN", &en), ("ZH", &zh)] {
            assert!(
                text.contains("Ctrl-b"),
                "{locale_name} guidance must mention tmux detach shortcut: {text}"
            );
            assert!(
                text.contains("Ctrl-b d"),
                "{locale_name} guidance must explain tmux detach: {text}"
            );
            assert!(
                text.contains("exit"),
                "{locale_name} guidance must mention exit for current pane: {text}"
            );
            assert!(
                text.contains("workhive workspace close"),
                "{locale_name} guidance must mention the Workhive close command: {text}"
            );
        }

        assert!(
            en.contains("current pane"),
            "EN guidance must clarify exit closes the current pane, not the session: {en}"
        );
    }

    /// The localized empty-session message must not expose raw tmux
    /// socket paths in either locale.
    #[test]
    fn empty_session_message_hides_socket_paths() {
        use workhive_core::domain::Locale;
        use workhive_core::i18n_keys as ik;
        use workhive_infra::i18n::StaticI18n;

        let i18n = StaticI18n::new();
        for locale in [Locale::En, Locale::ZhCn] {
            let msg = i18n.resolve(locale, ik::SESSIONS_EMPTY);
            assert!(!msg.is_empty());
            assert!(
                !msg.contains("/tmp/tmux") && !msg.contains("whsrv-"),
                "empty-session message must not contain raw socket paths: {msg}"
            );
        }
    }
}

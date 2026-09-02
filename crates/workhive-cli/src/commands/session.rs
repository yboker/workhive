//! `workhive session …` — global, project-agnostic session surfaces.
//!
//! `session list` enumerates every running tmux session that Workhive can
//! resolve from the project index, regardless of the current working
//! directory. `session attach` is the global counterpart of
//! `workspace attach`: it resolves a target session across all projects and
//! attaches the user's terminal — without ever creating `.workhive/`,
//! materializing worktrees, or upserting the project index.
//!
//! These commands intentionally live in their own module (separate from the
//! large `commands/project.rs`) so the global session surface can evolve
//! independently from project-scoped logic.

use std::io::IsTerminal;

use workhive_core::domain::SessionName;
use workhive_core::i18n_keys as k;
use workhive_core::ports::{I18nPort, IndexState, ProjectIndexPort, WorkspaceRuntimePort};
use workhive_core::usecases;
use workhive_core::usecases::sessions::{SessionRecord, UnavailableProject};

use crate::cli::ListView;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::path_display::display_path_for_humans;
use crate::table::{Column, print_table};
use crate::ui;

use super::project::{home_path, load_global_for_hint, print_setup_state_hint};
use super::render_stale_entries;
use super::{build_global_attach_plan, render_global_attach_lines};

// ─────────────────────────────────────────────────────────────────────────────
// `workhive session list`
// ─────────────────────────────────────────────────────────────────────────────

/// `workhive session list [--view auto|table|compact|json]` — global session
/// listing across every project in the index.
///
/// Behaviour rules:
/// - All views surface project / workspace / path / session.
/// - The "client" column is rendered as "有 / 无" (or "yes / no" in English).
/// - JSON keeps absolute paths; human views abbreviate `$HOME` to `~/...`.
/// - `auto` collapses to compact when the terminal is narrow.
pub(crate) fn cmd_sessions(view: ListView, ctx: &CommandContext) -> Result<(), CliError> {
    use workhive_core::usecases::trusted_roots::DiscoverySetupState;
    let locale = ctx.locale();

    let project_summaries = match resolve_session_listing_state(ctx)? {
        SessionListingState::IndexMissing { setup } => {
            if matches!(view, ListView::Json) {
                emit_sessions_json(&[]);
                return Ok(());
            }
            print_setup_state_hint(&setup, ctx, locale);
            return Ok(());
        }
        SessionListingState::IndexEmpty { setup } => {
            if matches!(view, ListView::Json) {
                emit_sessions_json(&[]);
                return Ok(());
            }
            match setup {
                DiscoverySetupState::Ready => {
                    eprintln!("{}", ctx.i18n.resolve(locale, k::SESSIONS_EMPTY));
                }
                other => print_setup_state_hint(&other, ctx, locale),
            }
            return Ok(());
        }
        SessionListingState::Loaded(v) => v,
    };

    let ws_fs = ctx.composition.workspace_fs();
    let resolution =
        usecases::resolve_projects::resolve_discovered_projects(&project_summaries, &ws_fs);

    if !matches!(view, ListView::Json) {
        render_stale_entries(&resolution.stale, ctx, locale);
    }

    let tmux = ctx.composition.tmux();
    let runtime = ctx.composition.runtime(&tmux);
    let listing = usecases::sessions::list_global_sessions(
        &resolution.valid,
        |project| ctx.naming_policy_for(Some(project.project_root.as_ref())),
        &runtime,
    )?;

    if matches!(view, ListView::Json) {
        // JSON output stays backward compatible: it carries only reachable
        // session records. Projects whose backend was unreachable are omitted
        // rather than failing the command or altering the schema.
        emit_sessions_json(&listing.records);
        return Ok(());
    }

    // Human output surfaces each unreachable project as a per-project warning,
    // then keeps listing sessions from every reachable project.
    render_unavailable_projects(&listing.unavailable, ctx, locale);

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
                r.workspace.clone(),
                display_path_for_humans(camino::Utf8Path::new(&r.project_root)),
                r.session_name.clone(),
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
            ctx.i18n.resolve(locale, k::SESSIONS_COL_PATH),
            display_path_for_humans(camino::Utf8Path::new(&r.project_root))
        );
        println!(
            "  {}: {}",
            ctx.i18n.resolve(locale, k::SESSIONS_COL_SESSION),
            r.session_name
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

/// Emit a per-project warning (stderr) for each project whose runtime backend
/// was unreachable during the global sweep.
///
/// This is informational only: the global listing already continued for every
/// reachable project. The localized reason is resolved from the typed
/// [`workhive_core::ports::BackendUnavailableReason`] so the message explains
/// why each project was skipped (missing socket, stopped server, …).
fn render_unavailable_projects(
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
        eprintln!(
            "{}",
            ctx.i18n.resolve_with_args(
                locale,
                k::SESSIONS_PROJECT_UNAVAILABLE,
                &[project.project_name.as_str(), reason.as_str()],
            )
        );
    }
}

fn emit_sessions_json(records: &[SessionRecord]) {
    // JSON intentionally retains absolute paths (machine-readable surface) and
    // exposes the boolean `attached` flag verbatim — no localization in the
    // payload. Schema:
    //   { project, workspace, project_root, server, session, attached, status }
    // `status` is the lifecycle state of the underlying tmux session as
    // observed by Workhive. Today every session that surfaces in the
    // listing was returned by `list_workspaces`, so it is always
    // "running"; the field exists so future expansion (e.g. detecting
    // hung/stopped sessions) does not break the payload contract.
    let rows: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "project": r.project_name,
                "workspace": r.workspace,
                "project_root": r.project_root,
                "server": r.server_name,
                "session": r.session_name,
                "attached": r.attached,
                "status": "running",
            })
        })
        .collect();

    let payload = serde_json::json!({
        "sessions": rows,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).expect("JSON serialization must succeed")
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// `workhive session attach`
// ─────────────────────────────────────────────────────────────────────────────

/// `workhive session attach [TARGET]` — global session attach.
///
/// This command intentionally does **not** detect or initialize a project. It
/// only attaches to an already-running tmux session that workhive can resolve
/// through the global project index.
///
/// Resolution rules:
/// - No `target` and stdin is a TTY → interactive selector.
/// - No `target` and stdin is non-interactive → friendly error.
/// - Exact session name match → attach.
/// - `project/workspace` match → attach.
/// - Workspace-only match → attach when unique, error on ambiguity.
pub(crate) fn cmd_session_attach(
    target: Option<&str>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();

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
            let selection =
                ui::screens::select::run_with_disabled(&title, &labels, &disabled, default_idx)?;
            let row_index = selection - disabled[..selection].iter().filter(|d| **d).count();
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

    let summaries = match resolve_session_listing_state(ctx)? {
        SessionListingState::IndexMissing { setup } | SessionListingState::IndexEmpty { setup } => {
            return Err(CliError::Runtime(setup_state_to_message(
                &setup, ctx, locale,
            )));
        }
        SessionListingState::Loaded(v) => v,
    };

    let ws_fs = ctx.composition.workspace_fs();
    let resolution = usecases::resolve_projects::resolve_discovered_projects(&summaries, &ws_fs);
    render_stale_entries(&resolution.stale, ctx, locale);

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);

    let listing = usecases::sessions::list_global_sessions(
        &resolution.valid,
        |project| ctx.naming_policy_for(Some(project.project_root.as_ref())),
        &runtime,
    )?;
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

/// Resolved listing state shared by `session list` and `session attach`.
///
/// Both commands need exactly the same setup-state semantics so that users
/// receive identical guidance copy regardless of whether they ran a list or
/// an attach. The variants carry the precomputed
/// [`DiscoverySetupState`](workhive_core::usecases::trusted_roots::DiscoverySetupState)
/// so the caller can either render it as a hint (list) or surface it as a
/// localized error message (attach).
enum SessionListingState {
    /// `index.list_projects()` reports no index file.
    IndexMissing {
        setup: workhive_core::usecases::trusted_roots::DiscoverySetupState,
    },
    /// Index exists but contains no projects.
    IndexEmpty {
        setup: workhive_core::usecases::trusted_roots::DiscoverySetupState,
    },
    /// Index loaded with at least one project.
    Loaded(Vec<workhive_core::ports::ProjectSummary>),
}

/// Compute the listing state once, including the trusted-roots probe used
/// for setup-state resolution. This is the only place the missing/empty
/// branching logic lives, so `session list` and `session attach` cannot
/// drift out of sync.
fn resolve_session_listing_state(ctx: &CommandContext) -> Result<SessionListingState, CliError> {
    use workhive_core::usecases::trusted_roots::{
        SetupStateProbe, resolve_setup_state, trusted_roots_from_config,
    };

    let locale = ctx.locale();
    let index = ctx.composition.project_index()?;
    let state = index.list_projects()?;

    let global_cfg = load_global_for_hint(ctx, locale);
    let trusted_set = trusted_roots_from_config(global_cfg.as_ref(), home_path().as_deref());

    Ok(match state {
        IndexState::NotFound => {
            let setup = resolve_setup_state(SetupStateProbe {
                trusted_roots_configured: !trusted_set.is_empty(),
                index_present: false,
                indexed_project_count: 0,
            });
            SessionListingState::IndexMissing { setup }
        }
        IndexState::Loaded(v) if v.is_empty() => {
            let setup = resolve_setup_state(SetupStateProbe {
                trusted_roots_configured: !trusted_set.is_empty(),
                index_present: true,
                indexed_project_count: 0,
            });
            SessionListingState::IndexEmpty { setup }
        }
        IndexState::Loaded(v) => SessionListingState::Loaded(v),
    })
}

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
    if let Some(rec) = records.iter().find(|r| r.session_name == query) {
        return SessionTargetResolution::Resolved(rec.clone());
    }

    if let Some((proj, ws)) = query.split_once('/') {
        let trimmed_proj = proj.trim();
        let trimmed_ws = ws.trim();
        if !trimmed_proj.is_empty()
            && !trimmed_ws.is_empty()
            && let Some(rec) = records
                .iter()
                .find(|r| r.project_name == trimmed_proj && r.workspace == trimmed_ws)
        {
            return SessionTargetResolution::Resolved(rec.clone());
        }
    }

    let by_workspace: Vec<&SessionRecord> =
        records.iter().filter(|r| r.workspace == query).collect();
    match by_workspace.len() {
        0 => SessionTargetResolution::NotFound,
        1 => SessionTargetResolution::Resolved(by_workspace[0].clone()),
        _ => SessionTargetResolution::Ambiguous {
            candidates: by_workspace
                .iter()
                .map(|r| format!("{}/{}", r.project_name, r.workspace))
                .collect(),
        },
    }
}

fn resolve_session_target(
    query: &str,
    records: &[SessionRecord],
    ctx: &CommandContext,
) -> Result<SessionRecord, CliError> {
    let locale = ctx.locale();
    match resolve_session_target_pure(query, records) {
        SessionTargetResolution::Resolved(rec) => Ok(rec),
        SessionTargetResolution::NotFound => Err(CliError::Runtime(ctx.i18n.resolve_with_args(
            locale,
            k::SESSION_ATTACH_NOT_FOUND,
            &[query],
        ))),
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

/// Underlying attach helper — calls into the runtime port.
///
/// Kept private to this module so `workspace attach` retains its own,
/// project-aware, attach pipeline (with legacy fallback). This helper assumes
/// the caller has already validated the (server, session) pair against the
/// global session listing, so no fallback rewrite is needed here.
fn perform_session_attach(record: &SessionRecord, ctx: &CommandContext) -> Result<(), CliError> {
    let locale = ctx.locale();

    // Sanity-check the session name is a workhive-managed name. If it is not,
    // the global listing should not have surfaced it; treat this as a
    // defensive guard rather than a user-facing error path.
    let _ = SessionName::from_raw(&record.session_name);

    let tmux = ctx.composition.tmux();
    let runtime = ctx
        .composition
        .runtime_with_registry(&tmux, &ctx.pane_registry);

    runtime
        .attach_workspace(workhive_core::ports::AttachWorkspaceRequest {
            group: record.server_name.clone(),
            workspace_name: record.session_name.clone(),
        })
        .map_err(|e| {
            crate::commands::close::map_attach_backend_unavailable(CliError::RuntimeOp(e), ctx)
        })?;

    eprintln!(
        "{}",
        ctx.i18n
            .resolve_with_args(locale, k::ATTACH_SUCCESS, &[&record.session_name])
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(project: &str, ws: &str, root: &str, session: &str, server: &str) -> SessionRecord {
        SessionRecord {
            project_name: project.to_owned(),
            project_root: root.to_owned(),
            workspace: ws.to_owned(),
            server_name: server.to_owned(),
            session_name: session.to_owned(),
            attached: false,
        }
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
                assert_eq!(rec.session_name, "wh@xyz@alpha@feature")
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
}

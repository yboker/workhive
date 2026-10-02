//! Human-readable renderers for `workspace list`: auto/table/compact
//! views and their formatting helpers.

use super::*;

pub(crate) fn print_workspace_auto(
    entries: &[usecases::list::WorkspaceEntry],
    layout: &workhive_core::ports::LayoutConfig,
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) {
    if let Some((width, _)) = terminal_size::terminal_size()
        && width.0 <= 96
    {
        print_workspace_compact_list(entries, layout, ctx, locale, initialized);
        return;
    }
    print_workspace_table(entries, layout, ctx, locale, initialized);
}

pub(crate) fn print_workspace_table(
    entries: &[usecases::list::WorkspaceEntry],
    layout: &workhive_core::ports::LayoutConfig,
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) {
    use workhive_core::i18n_keys as k;

    eprintln!("{}", ctx.i18n.resolve(locale, k::LIST_HEADER));
    eprintln!(
        "{}",
        workspace_overview_summary(entries, ctx, locale, initialized)
    );

    print_configured_layout(layout, ctx, locale, initialized);

    for line in format_workspace_table_lines(entries, ctx, locale) {
        println!("{}", line);
    }
}

/// Build the grouped, peer-section table rendering for
/// `workhive workspace ls` as a vector of lines. Returning the
/// rendered output as plain text lets tests assert section
/// ordering, headers, the blank-line break between project-root
/// and Worktree, and the empty-state hint without relying on
/// stdout capture.
///
/// Rendering contract:
/// - Section labels render on their own lines as plain text so
///   they read as peer hierarchy markers, not as table rows.
/// - Each non-empty section emits a fresh column header (driven
///   by [`table::format_table_lines`]), so a worktree section
///   that follows a project-root section never inherits or hides
///   the column context above it.
/// - Exactly one blank line separates project-root and Worktree,
///   giving the visual break the source spec calls for without
///   inflating vertical whitespace.
/// - The Worktree section always renders, even when empty; the
///   empty case emits the localized hint indented under the
///   section label so it reads as the section's content.
pub(crate) fn format_workspace_table_lines(
    entries: &[usecases::list::WorkspaceEntry],
    ctx: &CommandContext,
    locale: Locale,
) -> Vec<String> {
    use table::{Column, format_table_lines};
    use workhive_core::i18n_keys as k;

    // Shared optional columns for both sections. Only the identity
    // (first) column differs between sections: the project-root section
    // shows the repository's main branch, so it is labelled "Branch",
    // while the Worktree section shows workspace names ("Workspace").
    let trailing_columns = || {
        vec![
            Column {
                header: ctx.i18n.resolve(locale, k::LIST_COL_SESSION),
                min_width: 8,
            },
            Column {
                header: ctx.i18n.resolve(locale, k::LIST_COL_LAYOUT),
                min_width: 0,
            },
            Column {
                header: ctx.i18n.resolve(locale, k::LIST_COL_RECENT),
                min_width: 16,
            },
        ]
    };

    let mut root_columns = vec![Column {
        header: ctx.i18n.resolve(locale, k::WORKTREE_COL_BRANCH),
        min_width: 0,
    }];
    root_columns.extend(trailing_columns());

    let mut worktree_columns = vec![Column {
        header: ctx.i18n.resolve(locale, k::LIST_COL_WORKSPACE),
        min_width: 0,
    }];
    worktree_columns.extend(trailing_columns());

    let render_rows = |group: &[&usecases::list::WorkspaceEntry]| -> Vec<Vec<String>> {
        group
            .iter()
            .map(|e| {
                vec![
                    e.name.clone(),
                    workspace_status_display(e, ctx, locale),
                    layout_display(e, ctx, locale),
                    recent_display(e, ctx, locale),
                ]
            })
            .collect()
    };

    let (roots, worktrees) = partition_workspaces_by_kind(entries);

    let mut lines: Vec<String> = Vec::new();
    lines.push(ctx.i18n.resolve(locale, k::LIST_GROUP_PROJECT_ROOT));
    lines.extend(format_table_lines(&root_columns, &render_rows(&roots)));

    // Peer-section break between project-root and Worktree.
    lines.push(String::new());
    lines.push(ctx.i18n.resolve(locale, k::LIST_GROUP_WORKTREE));
    if worktrees.is_empty() {
        lines.push(format!(
            "  {}",
            ctx.i18n.resolve(locale, k::LIST_GROUP_WORKTREE_EMPTY)
        ));
    } else {
        lines.extend(format_table_lines(
            &worktree_columns,
            &render_rows(&worktrees),
        ));
    }
    lines
}

/// Split the workspace entries into the project-root group and the
/// linked-worktree group using the typed `WorkspaceKind` discriminant.
///
/// Grouping is driven by `kind`, never by parsing the display name or a
/// `(main)` suffix, so renderers stay decoupled from naming cosmetics.
pub(crate) fn partition_workspaces_by_kind(
    entries: &[usecases::list::WorkspaceEntry],
) -> (
    Vec<&usecases::list::WorkspaceEntry>,
    Vec<&usecases::list::WorkspaceEntry>,
) {
    use usecases::list::WorkspaceKind;
    entries
        .iter()
        .partition(|e| matches!(e.kind, WorkspaceKind::ProjectRoot))
}

pub(crate) fn print_workspace_compact_list(
    entries: &[usecases::list::WorkspaceEntry],
    layout: &workhive_core::ports::LayoutConfig,
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) {
    use workhive_core::i18n_keys as k;

    eprintln!("{}", ctx.i18n.resolve(locale, k::LIST_HEADER));
    eprintln!(
        "{}",
        workspace_overview_summary(entries, ctx, locale, initialized)
    );

    print_configured_layout(layout, ctx, locale, initialized);

    for line in format_workspace_compact_lines(entries, ctx, locale) {
        println!("{}", line);
    }
}

/// Render the project-level layout that will be applied when a workspace is
/// created. The same diagram renderer powers `workhive init` previews, so the
/// list and configuration surfaces use identical geometry and pane labels.
fn print_configured_layout(
    layout: &workhive_core::ports::LayoutConfig,
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) {
    for line in format_configured_layout_lines(layout, ctx, locale, initialized) {
        println!("{line}");
    }
}

pub(crate) fn format_configured_layout_lines(
    layout: &workhive_core::ports::LayoutConfig,
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) -> Vec<String> {
    use workhive_core::i18n_keys as k;

    if !initialized || layout.columns.is_empty() {
        return Vec::new();
    }

    let mut lines = vec![
        String::new(),
        ctx.i18n.resolve_with_args(
            locale,
            k::LIST_CONFIGURED_LAYOUT,
            &[&layout.template.to_string()],
        ),
    ];
    lines.extend(
        crate::wizard::render_diagram(&layout.columns, &ctx.i18n, locale)
            .lines()
            .map(str::to_owned),
    );
    lines.push(String::new());
    lines
}

/// Build the grouped compact (narrow-terminal) rendering for
/// `workhive workspace ls` as a vector of lines. Each workspace
/// entry expands to a leading identity line and indented
/// `label: value` rows so a screen reader or 80-column terminal
/// reads them as a single record. Sections and the peer break
/// mirror [`format_workspace_table_lines`] so the compact and
/// wide variants stay visually parallel.
pub(crate) fn format_workspace_compact_lines(
    entries: &[usecases::list::WorkspaceEntry],
    ctx: &CommandContext,
    locale: Locale,
) -> Vec<String> {
    use workhive_core::i18n_keys as k;

    let render_entry = |e: &usecases::list::WorkspaceEntry| -> Vec<String> {
        vec![
            e.name.clone(),
            format!(
                "  {}: {}",
                ctx.i18n.resolve(locale, k::LIST_COL_SESSION),
                workspace_status_display(e, ctx, locale),
            ),
            format!(
                "  {}: {}",
                ctx.i18n.resolve(locale, k::LIST_COL_LAYOUT),
                layout_display(e, ctx, locale),
            ),
            format!(
                "  {}: {}",
                ctx.i18n.resolve(locale, k::LIST_COL_RECENT),
                recent_display(e, ctx, locale),
            ),
        ]
    };

    let (roots, worktrees) = partition_workspaces_by_kind(entries);

    let mut lines: Vec<String> = Vec::new();
    lines.push(ctx.i18n.resolve(locale, k::LIST_GROUP_PROJECT_ROOT));
    let root_last = roots.len().saturating_sub(1);
    for (idx, e) in roots.iter().enumerate() {
        lines.extend(render_entry(e));
        if idx != root_last {
            lines.push(String::new());
        }
    }

    lines.push(String::new());
    lines.push(ctx.i18n.resolve(locale, k::LIST_GROUP_WORKTREE));
    if worktrees.is_empty() {
        lines.push(format!(
            "  {}",
            ctx.i18n.resolve(locale, k::LIST_GROUP_WORKTREE_EMPTY)
        ));
    } else {
        let wt_last = worktrees.len().saturating_sub(1);
        for (idx, e) in worktrees.iter().enumerate() {
            lines.extend(render_entry(e));
            if idx != wt_last {
                lines.push(String::new());
            }
        }
    }
    lines
}

/// Render the localized layout column for a workspace row.
///
/// `pane_count_status` is authoritative for the active session case
/// and feeds the localized "{N} panes" / "lookup failed" copy. When
/// no session is running we fall back to the `layout_info` string,
/// where the bare `-` placeholder is also localized to a
/// self-explanatory "Not started" / "未启动" instead of leaking the
/// raw glyph that the JSON view keeps for backward compatibility.
pub(crate) fn layout_display(
    entry: &usecases::list::WorkspaceEntry,
    ctx: &CommandContext,
    locale: Locale,
) -> String {
    use usecases::list::PaneCountStatus;
    use workhive_core::i18n_keys as k;

    match entry.pane_count_status {
        PaneCountStatus::Known(c) => {
            ctx.i18n
                .resolve_with_args(locale, k::LIST_LAYOUT_PANE_COUNT, &[&c.to_string()])
        }
        PaneCountStatus::Unknown => ctx.i18n.resolve(locale, k::LIST_LAYOUT_LOOKUP_FAILED),
        PaneCountStatus::NotApplicable => resolve_layout_display(&entry.layout_info, ctx, locale),
    }
}

/// Compute the single-line operational overview rendered after the
/// `Workspaces:` header.
///
/// When the project is initialized the summary lists totals and running
/// sessions. When the project is not initialized we surface a localized
/// hint pointing at `workhive init` so the human view never falls back to
/// a bare table without context.
pub(crate) fn workspace_overview_summary(
    entries: &[usecases::list::WorkspaceEntry],
    ctx: &CommandContext,
    locale: Locale,
    initialized: bool,
) -> String {
    use workhive_core::domain::SessionAvailability;
    use workhive_core::i18n_keys as k;

    if !initialized {
        return ctx.i18n.resolve_with_args(
            locale,
            k::LIST_OVERVIEW_UNINITIALIZED,
            &[&entries.len().to_string()],
        );
    }

    let total = entries.len();
    let running = entries
        .iter()
        .filter(|e| e.session_state == SessionAvailability::Running)
        .count();
    ctx.i18n.resolve_with_args(
        locale,
        k::LIST_OVERVIEW_SUMMARY,
        &[&total.to_string(), &running.to_string()],
    )
}

pub(crate) fn workspace_status_display(
    entry: &usecases::list::WorkspaceEntry,
    ctx: &CommandContext,
    locale: Locale,
) -> String {
    use workhive_core::domain::SessionAvailability;
    use workhive_core::i18n_keys as k;

    match entry.session_state {
        SessionAvailability::Running => ctx.i18n.resolve(locale, k::LIST_SESSION_RUNNING),
        SessionAvailability::LookupFailed => {
            ctx.i18n.resolve(locale, k::LIST_SESSION_LOOKUP_FAILED)
        }
        SessionAvailability::ServerUnreachable => {
            ctx.i18n.resolve(locale, k::LIST_SESSION_UNREACHABLE)
        }
        SessionAvailability::Missing => ctx.i18n.resolve(locale, k::LIST_SESSION_MISSING),
    }
}

pub(crate) fn recent_display(
    entry: &usecases::list::WorkspaceEntry,
    ctx: &CommandContext,
    locale: Locale,
) -> String {
    format_last_used_epoch(entry.last_used_epoch_secs, ctx, locale)
}

/// Shared localized "last used" display helper for every workspace surface
/// (`workspace ls`, `workspace open` selector, `workspace attach` selector,
/// and any enriched selector rows reusing the recent-activity field).
///
/// The core boundary hands the CLI a timezone-neutral raw epoch; this helper
/// renders it in the user's local timezone using a 24-hour clock and
/// locale-specific relative labels:
/// - same calendar day  -> localized "Today HH:mm" / "今天 HH:mm";
/// - previous calendar day -> localized "Yesterday HH:mm" / "昨天 HH:mm";
/// - older dates -> `YYYY-MM-DD HH:mm` in local time.
pub(crate) fn format_last_used_epoch(
    epoch_secs: Option<u64>,
    ctx: &CommandContext,
    locale: Locale,
) -> String {
    use workhive_core::i18n_keys as k;
    match epoch_secs {
        None => ctx.i18n.resolve(locale, k::LIST_NO_RECORD),
        Some(secs) => format_last_used_relative(secs, chrono::Local::now(), &ctx.i18n, locale),
    }
}

/// Pure relative-time renderer shared by [`format_last_used_epoch`].
///
/// `now` carries the timezone used to convert `epoch_secs`, so both the
/// wall-clock time and the same-day / previous-day comparison are performed
/// in a single, caller-supplied zone. This keeps the function deterministic
/// under test regardless of the host machine's timezone.
pub(crate) fn format_last_used_relative<Tz: chrono::TimeZone>(
    epoch_secs: u64,
    now: chrono::DateTime<Tz>,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> String {
    use chrono::{Datelike, Timelike};
    use workhive_core::i18n_keys as k;

    let tz = now.timezone();
    let Some(target) = i64::try_from(epoch_secs)
        .ok()
        .and_then(|s| tz.timestamp_opt(s, 0).single())
    else {
        // Unrepresentable timestamp (clock skew / overflow): degrade to the
        // same "no record" copy rather than rendering a misleading date.
        return i18n.resolve(locale, k::LIST_NO_RECORD);
    };

    let hhmm = format!("{:02}:{:02}", target.hour(), target.minute());
    let today = now.date_naive();
    let target_date = target.date_naive();

    if target_date == today {
        i18n.resolve_with_args(locale, k::LIST_LAST_USED_TODAY, &[&hhmm])
    } else if today.pred_opt() == Some(target_date) {
        i18n.resolve_with_args(locale, k::LIST_LAST_USED_YESTERDAY, &[&hhmm])
    } else {
        format!(
            "{:04}-{:02}-{:02} {hhmm}",
            target_date.year(),
            target_date.month(),
            target_date.day()
        )
    }
}

pub(crate) fn resolve_layout_display(
    layout_info: &str,
    ctx: &CommandContext,
    locale: Locale,
) -> String {
    use workhive_core::i18n_keys as k;
    match layout_info {
        "ai_coding_left" | "ai_coding_left (3)" => {
            ctx.i18n.resolve(locale, k::LAYOUT_AI_CODING_LEFT)
        }
        "ai_coding_dual" | "ai_coding_dual (3)" => {
            ctx.i18n.resolve(locale, k::LAYOUT_AI_CODING_DUAL)
        }
        "half_split" | "half_split (2)" => ctx.i18n.resolve(locale, k::LAYOUT_HALF_SPLIT),
        "quad" | "quad (4)" => ctx.i18n.resolve(locale, k::LAYOUT_QUAD),
        "single" | "single (1)" => ctx.i18n.resolve(locale, k::LAYOUT_SINGLE),
        "custom" => ctx.i18n.resolve(locale, k::LAYOUT_CUSTOM),
        // The bare `-` placeholder must not leak into the human view.
        // JSON keeps the glyph for backward compatibility (see
        // `print_workspace_json`), but operators see a localized,
        // self-explanatory label here.
        "-" | "" => ctx.i18n.resolve(locale, k::LIST_LAYOUT_NOT_CONFIGURED),
        other => other.to_owned(),
    }
}

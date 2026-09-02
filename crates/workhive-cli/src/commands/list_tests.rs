//! Unit tests for the `workspace list` human renderer. The test
//! surface intentionally exercises the small render helpers
//! (`layout_display`, `workspace_overview_summary`) so the
//! assertions are deterministic and locale-aware without forking
//! the CLI binary.

use super::*;
use camino::Utf8PathBuf;
use usecases::list::{PaneCountStatus, SnapshotStatus, WorkspaceEntry, WorkspaceKind};
use workhive_core::domain::{Locale, SessionAvailability};

fn entry_active(name: &str, panes: u32, snapshot: SnapshotStatus) -> WorkspaceEntry {
    WorkspaceEntry {
        name: name.to_owned(),
        kind: WorkspaceKind::LinkedWorktree,
        worktree_path: Utf8PathBuf::from(format!("/tmp/{name}")),
        worktree_exists: true,
        session_name: format!("wh@x@proj@{name}"),
        session_active: true,
        session_state: SessionAvailability::Running,
        pane_count: Some(panes),
        layout_info: format!("{panes} panes"),
        has_snapshot: matches!(snapshot, SnapshotStatus::Available),
        last_used_epoch_secs: Some(1_715_335_200),
        snapshot_status: snapshot,
        pane_count_status: PaneCountStatus::Known(panes),
    }
}

fn entry_inactive(name: &str, layout_info: &str, snapshot: SnapshotStatus) -> WorkspaceEntry {
    WorkspaceEntry {
        name: name.to_owned(),
        kind: WorkspaceKind::LinkedWorktree,
        worktree_path: Utf8PathBuf::from(format!("/tmp/{name}")),
        worktree_exists: true,
        session_name: String::new(),
        session_active: false,
        session_state: SessionAvailability::Missing,
        pane_count: None,
        layout_info: layout_info.to_owned(),
        has_snapshot: matches!(snapshot, SnapshotStatus::Available),
        last_used_epoch_secs: None,
        snapshot_status: snapshot,
        pane_count_status: PaneCountStatus::NotApplicable,
    }
}

fn entry_lookup_failed(name: &str) -> WorkspaceEntry {
    WorkspaceEntry {
        name: name.to_owned(),
        kind: WorkspaceKind::LinkedWorktree,
        worktree_path: Utf8PathBuf::from(format!("/tmp/{name}")),
        worktree_exists: true,
        session_name: format!("wh@x@proj@{name}"),
        session_active: true,
        session_state: SessionAvailability::Running,
        pane_count: None,
        layout_info: "-".to_owned(),
        has_snapshot: false,
        last_used_epoch_secs: None,
        snapshot_status: SnapshotStatus::Unknown,
        pane_count_status: PaneCountStatus::Unknown,
    }
}

/// Active session with a known pane count must render as the
/// localized "{N} panes" string in English, never as the raw
/// English fragment leaked from `layout_info`.
#[test]
fn layout_display_active_known_renders_localized_pane_count_en() {
    let ctx = CommandContext::new();
    let entry = entry_active("feat-x", 3, SnapshotStatus::Absent);
    let out = layout_display(&entry, &ctx, Locale::En);
    assert_eq!(out, "3 panes");
}

/// Active session with a known pane count must render in
/// Simplified Chinese using the localized template, ensuring the
/// `format!("{c} panes")` core fragment never leaks into zh-CN.
#[test]
fn layout_display_active_known_renders_localized_pane_count_zh_cn() {
    let ctx = CommandContext::new();
    let entry = entry_active("feat-x", 3, SnapshotStatus::Absent);
    let out = layout_display(&entry, &ctx, Locale::ZhCn);
    assert_eq!(out, "3 个窗格");
}

/// Inactive workspace whose layout has not been configured must
/// render as a localized "Not started" instead of leaking the raw
/// `-` glyph the JSON view keeps for backward compatibility.
#[test]
fn layout_display_inactive_no_config_returns_localized_not_started_en() {
    let ctx = CommandContext::new();
    let entry = entry_inactive("feat-y", "-", SnapshotStatus::Absent);
    let out = layout_display(&entry, &ctx, Locale::En);
    assert_eq!(out, "not started");
    assert_ne!(out, "-");
}

#[test]
fn layout_display_inactive_no_config_returns_localized_not_started_zh_cn() {
    let ctx = CommandContext::new();
    let entry = entry_inactive("feat-y", "-", SnapshotStatus::Absent);
    let out = layout_display(&entry, &ctx, Locale::ZhCn);
    assert_eq!(out, "未启动");
    assert_ne!(out, "-");
}

/// Inactive workspace with a configured layout (e.g. the
/// `ai_coding_left (3)` fallback applied by `cmd_workspace_list`)
/// must still resolve to the existing localized template name.
#[test]
fn layout_display_inactive_with_template_returns_localized_template_en() {
    let ctx = CommandContext::new();
    let entry = entry_inactive("feat-z", "ai_coding_left (3)", SnapshotStatus::Absent);
    let out = layout_display(&entry, &ctx, Locale::En);
    assert!(out.contains("AI Coding Left"), "got: {out}");
}

/// Active session with a failed pane-count probe must render the
/// distinct "lookup failed" copy, never the bare `-` and never
/// the unexplained `?` glyph.
#[test]
fn layout_display_lookup_failed_returns_localized_failure_en() {
    let ctx = CommandContext::new();
    let entry = entry_lookup_failed("feat-fail");
    let out = layout_display(&entry, &ctx, Locale::En);
    assert_eq!(out, "lookup failed");
    assert_ne!(out, "-");
    assert_ne!(out, "?");
}

#[test]
fn layout_display_lookup_failed_returns_localized_failure_zh_cn() {
    let ctx = CommandContext::new();
    let entry = entry_lookup_failed("feat-fail");
    let out = layout_display(&entry, &ctx, Locale::ZhCn);
    assert_eq!(out, "查询失败");
}

/// The operational overview line must summarize totals in
/// English using the localized template, with no hard-coded glyphs
/// or English-only fragments leaking into a zh-CN run.
#[test]
fn workspace_overview_summary_initialized_en() {
    let ctx = CommandContext::new();
    let entries = vec![
        entry_active("a", 3, SnapshotStatus::Available),
        entry_active("b", 2, SnapshotStatus::Absent),
        entry_inactive("c", "-", SnapshotStatus::Absent),
    ];
    let line = workspace_overview_summary(&entries, &ctx, Locale::En, true);
    assert_eq!(
        line, "Total: 3 workspaces \u{b7} 2 running \u{b7} 1 with snapshot",
        "got: {line}"
    );
}

#[test]
fn workspace_overview_summary_initialized_zh_cn() {
    let ctx = CommandContext::new();
    let entries = vec![
        entry_active("a", 3, SnapshotStatus::Available),
        entry_active("b", 2, SnapshotStatus::Absent),
        entry_inactive("c", "-", SnapshotStatus::Absent),
    ];
    let line = workspace_overview_summary(&entries, &ctx, Locale::ZhCn, true);
    assert_eq!(
        line, "总计：3 个工作区 \u{b7} 2 个运行中 \u{b7} 1 个含快照",
        "got: {line}"
    );
}

/// Uninitialized projects must surface the "run `workhive init`"
/// hint inline so the human view never displays just a placeholder
/// row without context.
#[test]
fn workspace_overview_summary_uninitialized_en_points_at_init() {
    let ctx = CommandContext::new();
    let entries = vec![entry_inactive("main", "-", SnapshotStatus::Absent)];
    let line = workspace_overview_summary(&entries, &ctx, Locale::En, false);
    assert!(
        line.contains("workhive init") && line.contains("not initialized"),
        "got: {line}"
    );
}

#[test]
fn workspace_overview_summary_uninitialized_zh_cn_points_at_init() {
    let ctx = CommandContext::new();
    let entries = vec![entry_inactive("main", "-", SnapshotStatus::Absent)];
    let line = workspace_overview_summary(&entries, &ctx, Locale::ZhCn, false);
    assert!(
        line.contains("workhive init") && line.contains("项目未初始化"),
        "got: {line}"
    );
}

/// Narrow-width compact rendering uses the same helpers, so the
/// summary line and the layout label must remain readable for an
/// 80-column-or-narrower terminal target. The render path of
/// `print_workspace_compact_list` writes via `eprintln!`/`println!`
/// which is not directly capturable in unit tests, so we instead
/// exercise the underlying helpers and verify their output never
/// exceeds the narrow-screen budget.
#[test]
fn narrow_width_compact_helpers_produce_readable_lines() {
    let ctx = CommandContext::new();
    let entries = vec![
        entry_active("alpha", 3, SnapshotStatus::Available),
        entry_lookup_failed("beta"),
        entry_inactive("gamma", "-", SnapshotStatus::Absent),
    ];

    let summary_en = workspace_overview_summary(&entries, &ctx, Locale::En, true);
    assert!(
        summary_en.chars().count() < 80,
        "summary should fit narrow terminals: {summary_en}"
    );

    for entry in &entries {
        let label_en = layout_display(entry, &ctx, Locale::En);
        let label_zh = layout_display(entry, &ctx, Locale::ZhCn);
        assert!(!label_en.is_empty());
        assert!(!label_zh.is_empty());
        // The human renderer never emits the bare `-` or the
        // unexplained `?` glyph.
        assert_ne!(label_en, "-");
        assert_ne!(label_en, "?");
        assert_ne!(label_zh, "-");
        assert_ne!(label_zh, "?");
    }
}

fn entry_with_state(name: &str, state: SessionAvailability) -> WorkspaceEntry {
    WorkspaceEntry {
        name: name.to_owned(),
        kind: WorkspaceKind::LinkedWorktree,
        worktree_path: Utf8PathBuf::from(format!("/tmp/{name}")),
        worktree_exists: true,
        session_name: String::new(),
        session_active: matches!(state, SessionAvailability::Running),
        session_state: state,
        pane_count: None,
        layout_info: "-".to_owned(),
        has_snapshot: false,
        last_used_epoch_secs: None,
        snapshot_status: SnapshotStatus::Absent,
        pane_count_status: PaneCountStatus::NotApplicable,
    }
}

/// The `Session` column must surface four distinct English values —
/// running / no session / unreachable / lookup failed — so an
/// operator can tell a never-created session apart from an
/// unreachable server or a probe error.
#[test]
fn session_display_distinguishes_all_four_states_en() {
    let ctx = CommandContext::new();
    let running = workspace_status_display(
        &entry_with_state("a", SessionAvailability::Running),
        &ctx,
        Locale::En,
    );
    let missing = workspace_status_display(
        &entry_with_state("b", SessionAvailability::Missing),
        &ctx,
        Locale::En,
    );
    let unreachable = workspace_status_display(
        &entry_with_state("c", SessionAvailability::ServerUnreachable),
        &ctx,
        Locale::En,
    );
    let lookup = workspace_status_display(
        &entry_with_state("d", SessionAvailability::LookupFailed),
        &ctx,
        Locale::En,
    );

    assert_eq!(running, "running");
    assert_eq!(missing, "no session");
    assert_eq!(unreachable, "unreachable");
    assert_eq!(lookup, "lookup failed");
    // Missing must never collapse into the unreachable or lookup copy.
    assert_ne!(missing, unreachable);
    assert_ne!(missing, lookup);
}

#[test]
fn session_display_distinguishes_all_four_states_zh_cn() {
    let ctx = CommandContext::new();
    assert_eq!(
        workspace_status_display(
            &entry_with_state("a", SessionAvailability::Running),
            &ctx,
            Locale::ZhCn
        ),
        "运行中"
    );
    assert_eq!(
        workspace_status_display(
            &entry_with_state("b", SessionAvailability::Missing),
            &ctx,
            Locale::ZhCn
        ),
        "无会话"
    );
    assert_eq!(
        workspace_status_display(
            &entry_with_state("c", SessionAvailability::ServerUnreachable),
            &ctx,
            Locale::ZhCn
        ),
        "不可达"
    );
    assert_eq!(
        workspace_status_display(
            &entry_with_state("d", SessionAvailability::LookupFailed),
            &ctx,
            Locale::ZhCn
        ),
        "查询失败"
    );
}

/// Grouping is driven by the typed `WorkspaceKind`, not by parsing the
/// display name. A `ProjectRoot` row whose name happens to be `main`
/// must land in the project-root group, and every `LinkedWorktree`
/// row in the worktree group.
#[test]
fn partition_workspaces_by_kind_splits_root_and_worktree() {
    let mut root = entry_inactive("main", "-", SnapshotStatus::Absent);
    root.kind = WorkspaceKind::ProjectRoot;
    let entries = vec![
        root,
        entry_active("feat-a", 2, SnapshotStatus::Available),
        entry_active("feat-b", 1, SnapshotStatus::Absent),
    ];

    let (roots, worktrees) = partition_workspaces_by_kind(&entries);
    assert_eq!(roots.len(), 1, "exactly one project-root row");
    assert_eq!(roots[0].name, "main");
    assert_eq!(worktrees.len(), 2, "both linked worktrees grouped together");
    assert!(
        worktrees
            .iter()
            .all(|e| matches!(e.kind, WorkspaceKind::LinkedWorktree))
    );
}

fn make_project_root(name: &str) -> WorkspaceEntry {
    let mut e = entry_inactive(name, "ai_coding_left (3)", SnapshotStatus::Available);
    e.kind = WorkspaceKind::ProjectRoot;
    e
}

/// `workspace ls` table output must render the project-root and
/// Worktree section labels in document order, each followed by
/// its own column-header row so the second group never inherits
/// the first group's column context.
#[test]
fn table_view_emits_peer_section_labels_in_order_zh_cn() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];
    let lines = format_workspace_table_lines(&entries, &ctx, Locale::ZhCn);
    let root_idx = lines
        .iter()
        .position(|l| l == "项目根目录")
        .expect("project-root section label present");
    let wt_idx = lines
        .iter()
        .position(|l| l == "Worktree 工作区")
        .expect("worktree section label present");
    assert!(
        root_idx < wt_idx,
        "project-root section must precede worktree section: {lines:?}"
    );
}

/// A peer-section break (an empty line) must sit between the
/// project-root section's last data row and the Worktree
/// section label, so the eye reads them as siblings instead of
/// one contiguous table.
#[test]
fn table_view_inserts_blank_line_between_root_and_worktree_section() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];
    let lines = format_workspace_table_lines(&entries, &ctx, Locale::En);
    let wt_idx = lines
        .iter()
        .position(|l| l == "Worktree workspaces")
        .expect("English worktree section label present");
    assert!(
        wt_idx >= 1 && lines[wt_idx - 1].is_empty(),
        "blank line must precede the worktree section label: {lines:?}"
    );
}

/// When no worktrees exist, the empty-state hint must render
/// *under* the Worktree section label (indented with two
/// spaces), not detached or mixed into the project-root table.
#[test]
fn table_view_empty_worktrees_renders_under_worktree_section() {
    let ctx = CommandContext::new();
    let entries = vec![make_project_root("main")];
    let lines = format_workspace_table_lines(&entries, &ctx, Locale::En);
    let wt_idx = lines
        .iter()
        .position(|l| l == "Worktree workspaces")
        .expect("English worktree section label present");
    let next = lines
        .get(wt_idx + 1)
        .expect("empty-state hint follows worktree section label");
    assert!(
        next.starts_with("  ") && next.contains("no worktree workspaces"),
        "empty-state hint must sit under the worktree section: {next:?}"
    );
}

/// When worktrees exist, the Worktree section emits its own
/// column-header row (driven by the localized `工作区` /
/// `Workspace` label) directly under the section label, so the
/// second group reads as a self-contained table.
#[test]
fn table_view_worktree_section_emits_its_own_column_header_when_populated() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];
    let lines = format_workspace_table_lines(&entries, &ctx, Locale::ZhCn);
    let wt_idx = lines
        .iter()
        .position(|l| l == "Worktree 工作区")
        .expect("worktree section label present");
    let header = lines
        .get(wt_idx + 1)
        .expect("column header follows worktree section label");
    // The header row begins with the localized workspace column
    // name and contains the session column heading too.
    assert!(
        header.starts_with("工作区") && header.contains("会话"),
        "worktree column header must include localized column names: {header:?}"
    );
}

/// The project-root section's identity column shows the
/// repository's main branch, so its column header must read
/// `Branch` / `分支` rather than the worktree section's
/// `Workspace` / `工作区`. This keeps the root section honest
/// about what its leading column means.
#[test]
fn table_view_root_section_identity_column_is_branch_not_workspace() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];

    let lines_en = format_workspace_table_lines(&entries, &ctx, Locale::En);
    let root_idx = lines_en
        .iter()
        .position(|l| l == "Project root")
        .expect("English project-root section label present");
    let header_en = lines_en
        .get(root_idx + 1)
        .expect("column header follows project-root section label");
    assert!(
        header_en.starts_with("Branch"),
        "English project-root header must lead with Branch: {header_en:?}"
    );
    assert!(
        !header_en.starts_with("Workspace"),
        "project-root header must not mislabel the branch column as Workspace: {header_en:?}"
    );

    let lines_zh = format_workspace_table_lines(&entries, &ctx, Locale::ZhCn);
    let root_idx_zh = lines_zh
        .iter()
        .position(|l| l == "项目根目录")
        .expect("zh-CN project-root section label present");
    let header_zh = lines_zh
        .get(root_idx_zh + 1)
        .expect("column header follows project-root section label");
    assert!(
        header_zh.starts_with("分支"),
        "zh-CN project-root header must lead with 分支: {header_zh:?}"
    );
    assert!(
        !header_zh.starts_with("工作区"),
        "zh-CN project-root header must not mislabel the branch column as 工作区: {header_zh:?}"
    );
}

/// Compact (narrow-terminal) rendering must keep the same
/// peer-section ordering as the table renderer so the two
/// presentations remain visually parallel.
#[test]
fn compact_view_preserves_peer_section_hierarchy() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];
    let lines = format_workspace_compact_lines(&entries, &ctx, Locale::ZhCn);
    let root_idx = lines
        .iter()
        .position(|l| l == "项目根目录")
        .expect("project-root label present in compact view");
    let wt_idx = lines
        .iter()
        .position(|l| l == "Worktree 工作区")
        .expect("worktree label present in compact view");
    assert!(root_idx < wt_idx);
    assert!(
        wt_idx >= 1 && lines[wt_idx - 1].is_empty(),
        "blank line precedes worktree section in compact view: {lines:?}"
    );
}

/// In the compact view, every workspace record begins with its
/// identity line and is followed by indented `label: value`
/// rows. The session column terminology must survive the
/// peer-section refactor.
#[test]
fn compact_view_renders_localized_session_label() {
    let ctx = CommandContext::new();
    let entries = vec![
        make_project_root("main"),
        entry_active("feat-a", 2, SnapshotStatus::Available),
    ];
    let lines_en = format_workspace_compact_lines(&entries, &ctx, Locale::En);
    let has_session_label = lines_en.iter().any(|l| l.contains("Session:"));
    assert!(
        has_session_label,
        "English compact view exposes a Session: label: {lines_en:?}"
    );

    let lines_zh = format_workspace_compact_lines(&entries, &ctx, Locale::ZhCn);
    let has_session_label_zh = lines_zh.iter().any(|l| l.contains("会话:"));
    assert!(
        has_session_label_zh,
        "Chinese compact view exposes a 会话: label: {lines_zh:?}"
    );
}

// ── Last-used localized local-time formatter ──────────────────────
//
// These exercise the pure `format_last_used_relative` helper with a
// caller-supplied timezone and `now`, so the same-day / previous-day
// decision and the wall-clock rendering are deterministic regardless
// of the host machine's timezone.

use chrono::{FixedOffset, TimeZone};

/// UTC+8 reference zone and a fixed "now" of 2026-06-07 22:00 local.
fn zone_and_now() -> (FixedOffset, chrono::DateTime<FixedOffset>) {
    let tz = FixedOffset::east_opt(8 * 3600).expect("valid offset");
    let now = tz
        .with_ymd_and_hms(2026, 6, 7, 22, 0, 0)
        .single()
        .expect("valid now");
    (tz, now)
}

fn epoch_at(tz: &FixedOffset, y: i32, m: u32, d: u32, hh: u32, mm: u32) -> u64 {
    tz.with_ymd_and_hms(y, m, d, hh, mm, 0)
        .single()
        .expect("valid local datetime")
        .timestamp() as u64
}

#[test]
fn last_used_same_day_renders_localized_today_en_and_zh() {
    let ctx = CommandContext::new();
    let (tz, now) = zone_and_now();
    let epoch = epoch_at(&tz, 2026, 6, 7, 9, 30);

    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::En),
        "Today 09:30"
    );
    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::ZhCn),
        "今天 09:30"
    );
}

#[test]
fn last_used_previous_day_renders_localized_yesterday_en_and_zh() {
    let ctx = CommandContext::new();
    let (tz, now) = zone_and_now();
    let epoch = epoch_at(&tz, 2026, 6, 6, 23, 15);

    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::En),
        "Yesterday 23:15"
    );
    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::ZhCn),
        "昨天 23:15"
    );
}

#[test]
fn last_used_older_date_renders_absolute_local_date_both_locales() {
    let ctx = CommandContext::new();
    let (tz, now) = zone_and_now();
    let epoch = epoch_at(&tz, 2026, 5, 1, 8, 5);

    // Older dates use a locale-neutral absolute `YYYY-MM-DD HH:mm`.
    let expected = "2026-05-01 08:05";
    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::En),
        expected
    );
    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::ZhCn),
        expected
    );
}

/// A 24-hour clock must be used: an afternoon time renders as `14:05`,
/// never `02:05 PM`.
#[test]
fn last_used_uses_24_hour_clock() {
    let ctx = CommandContext::new();
    let (tz, now) = zone_and_now();
    let epoch = epoch_at(&tz, 2026, 6, 7, 14, 5);

    let out = format_last_used_relative(epoch, now, &ctx.i18n, Locale::En);
    assert_eq!(out, "Today 14:05");
    assert!(!out.contains("PM") && !out.contains("AM"));
}

/// Conversion must use the supplied (local) timezone, not UTC. An
/// instant that is the previous calendar day in UTC but the current
/// day in UTC+8 must render as "Today" in local time.
#[test]
fn last_used_uses_local_timezone_not_utc() {
    let ctx = CommandContext::new();
    let tz = FixedOffset::east_opt(8 * 3600).expect("valid offset");
    // now = 2026-06-08 07:00 local (which is 2026-06-07 23:00 UTC).
    let now = tz
        .with_ymd_and_hms(2026, 6, 8, 7, 0, 0)
        .single()
        .expect("valid now");
    // target = 2026-06-08 00:30 local == 2026-06-07 16:30 UTC.
    let epoch = epoch_at(&tz, 2026, 6, 8, 0, 30);

    // Local view: same day -> "Today 00:30". A UTC formatter would have
    // shown the previous day (2026-06-07 16:30), so this proves the
    // conversion is local-timezone aware.
    assert_eq!(
        format_last_used_relative(epoch, now, &ctx.i18n, Locale::En),
        "Today 00:30"
    );
}

/// A missing timestamp renders the localized "no record" copy on every
/// surface, never an empty or fabricated value.
#[test]
fn last_used_missing_renders_localized_no_record() {
    let ctx = CommandContext::new();
    assert_eq!(format_last_used_epoch(None, &ctx, Locale::En), "no record");
    assert_eq!(format_last_used_epoch(None, &ctx, Locale::ZhCn), "无记录");
}

/// The JSON `last_used_at` field stays a UTC `YYYY-MM-DD HH:mm`
/// string so existing consumers that parse it keep working, while a
/// missing record maps to JSON null.
#[test]
fn json_last_used_at_is_utc_string_or_null() {
    assert_eq!(
        format_last_used_at_utc(Some(1_715_335_200)).as_deref(),
        Some("2024-05-10 10:00")
    );
    assert_eq!(format_last_used_at_utc(None), None);
}

/// The JSON row carries the last-used time on two keys: `last_used_at`
/// as a string and `last_used_epoch_secs` as a number, so a consumer
/// can read either the human-parseable UTC string or the raw integer.
#[test]
fn json_row_exposes_string_last_used_at_and_numeric_epoch() {
    let entry = entry_active("feat-x", 3, SnapshotStatus::Absent);
    let row = workspace_json_row(&entry);

    let last_used_at = &row["last_used_at"];
    assert!(
        last_used_at.is_string(),
        "last_used_at must remain a string for backward compatibility: {last_used_at:?}"
    );
    assert_eq!(last_used_at.as_str(), Some("2024-05-10 10:00"));

    let epoch = &row["last_used_epoch_secs"];
    assert!(
        epoch.is_number(),
        "last_used_epoch_secs must be a number: {epoch:?}"
    );
    assert_eq!(epoch.as_u64(), Some(1_715_335_200));
}

/// When a workspace has no recorded last-used time, both JSON keys are
/// present and null — never absent and never a fabricated value.
#[test]
fn json_row_renders_null_for_missing_last_used() {
    let entry = entry_inactive("feat-y", "-", SnapshotStatus::Absent);
    let row = workspace_json_row(&entry);

    assert!(
        row.get("last_used_at").is_some(),
        "last_used_at key must always be present"
    );
    assert!(
        row.get("last_used_epoch_secs").is_some(),
        "last_used_epoch_secs key must always be present"
    );
    assert!(
        row["last_used_at"].is_null(),
        "missing last_used_at must serialize as JSON null"
    );
    assert!(
        row["last_used_epoch_secs"].is_null(),
        "missing last_used_epoch_secs must serialize as JSON null"
    );
}

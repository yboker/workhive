#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
#[allow(unused_imports)]
use crate::path_display::display_path_for_humans;

use workhive_core::usecases::sessions::SessionRecord;
use workhive_infra::i18n::StaticI18n;

fn ws_info(name: &str, attached: bool) -> SessionRecord {
    let workspace = SessionName::from_raw(name)
        .extract_workspace()
        .unwrap_or("")
        .to_owned();
    SessionRecord {
        binding: workhive_core::domain::RuntimeBinding::tmux("default", name),
        project_name: "test".to_owned(),
        project_root: "/tmp/test".to_owned(),
        workspace,
        runtime_label: None,
        attached,
    }
}

fn session_record(
    project_name: &str,
    workspace: &str,
    session_name: &str,
    attached: bool,
) -> SessionRecord {
    SessionRecord {
        binding: workhive_core::domain::RuntimeBinding::tmux(
            format!("whsrv-test-{project_name}"),
            session_name,
        ),
        project_name: project_name.to_owned(),
        project_root: format!("/tmp/{project_name}"),
        workspace: workspace.to_owned(),
        runtime_label: None,
        attached,
    }
}

/// Helper to render an attach plan at a fixed wide width and skip
/// the disabled header so per-row assertions stay focused on
/// product-visible data rows.
fn render_data_rows(
    plan: &AttachSelectorPlan,
    term_width: usize,
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: Locale,
) -> Vec<String> {
    let (labels, disabled) = render_attach_selector_lines(plan, term_width, i18n, locale);
    labels
        .into_iter()
        .zip(disabled)
        .filter_map(|(l, d)| if d { None } else { Some(l) })
        .collect()
}

#[test]
fn attach_plan_classifies_main_branch_as_project_root_and_others_as_worktree() {
    let i18n = StaticI18n::new();
    let workspaces = vec![
        ws_info("wh@instance@proj@main", false),
        ws_info("wh@instance@proj@feature-x", true),
        ws_info("legacy-session-noschema", false),
    ];

    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);

    assert_eq!(plan.rows[0].kind, AttachWorkspaceKind::ProjectRoot);
    assert_eq!(plan.rows[0].workspace, "main");
    assert!(!plan.rows[0].attached);

    assert_eq!(plan.rows[1].kind, AttachWorkspaceKind::Worktree);
    assert_eq!(plan.rows[1].workspace, "feature-x");
    assert!(plan.rows[1].attached);

    // legacy/non-conforming session names degrade to Unknown rather
    // than masquerading as a Worktree row.
    assert_eq!(plan.rows[2].kind, AttachWorkspaceKind::Unknown);
}

#[test]
fn attach_plan_keeps_source_identity_after_sorting() {
    let i18n = StaticI18n::new();
    let mut feature = ws_info("shared-runtime", false);
    feature.workspace = "feature".to_owned();
    let mut main = ws_info("shared-runtime", false);
    main.workspace = "main".to_owned();
    let records = vec![feature, main];

    let plan = build_attach_selector_plan(&records, "main", &i18n, Locale::En);

    assert_eq!(plan.rows[0].workspace, "main");
    assert_eq!(plan.rows[0].source_index, 1);
    assert_eq!(plan.rows[1].workspace, "feature");
    assert_eq!(plan.rows[1].source_index, 0);
}

#[test]
fn attach_label_starts_with_session_id_so_tmux_users_can_scan_it_first() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@feature-x", false)];
    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    let data = render_data_rows(&plan, 120, &i18n, Locale::En);

    assert!(
        data[0].starts_with("wh@instance@proj@feature-x"),
        "session id must be the first visible field, got: {}",
        data[0]
    );
}

#[test]
fn attach_label_includes_kind_and_attached_marker_for_attached_main_session() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@main", true)];

    let plan_en = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    let data_en = render_data_rows(&plan_en, 200, &i18n, Locale::En);
    assert!(
        data_en[0].contains("Project root"),
        "English row should advertise Project root kind: {}",
        data_en[0]
    );
    assert!(
        data_en[0].contains("(attached)"),
        "English row should include attached marker: {}",
        data_en[0]
    );

    let plan_zh = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::ZhCn);
    let data_zh = render_data_rows(&plan_zh, 200, &i18n, Locale::ZhCn);
    assert!(
        data_zh[0].contains("项目根目录"),
        "zh-CN row should advertise 项目根目录 kind: {}",
        data_zh[0]
    );
    assert!(
        data_zh[0].contains("（已连接）"),
        "zh-CN row should include attached marker: {}",
        data_zh[0]
    );
}

#[test]
fn global_attach_label_leads_with_session_name_then_project_and_workspace() {
    let i18n = StaticI18n::new();
    let records = vec![session_record(
        "demo-app",
        "feature-x",
        "wh@instance@demo-app@feature-x",
        false,
    )];
    let plan = build_global_attach_plan(&records, &i18n, Locale::En);
    let (labels, disabled) = render_global_attach_lines(&plan, 200, &i18n, Locale::En);
    let data: Vec<&String> = labels
        .iter()
        .zip(disabled.iter())
        .filter_map(|(l, d)| if *d { None } else { Some(l) })
        .collect();

    assert!(
        data[0].starts_with("wh@instance@demo-app@feature-x"),
        "global attach selector must put session id first: {}",
        data[0]
    );
    assert!(
        data[0].contains("demo-app"),
        "row must show project: {}",
        data[0]
    );
    assert!(
        data[0].contains("feature-x"),
        "row must show workspace: {}",
        data[0]
    );
}

#[test]
fn global_attach_label_localizes_attached_marker_in_english() {
    let i18n = StaticI18n::new();
    let records = vec![
        session_record("demo-app", "main", "wh@instance@demo-app@main", true),
        session_record("demo-app", "main", "wh@instance@demo-app@main", false),
    ];
    let plan = build_global_attach_plan(&records, &i18n, Locale::En);
    let (labels, disabled) = render_global_attach_lines(&plan, 200, &i18n, Locale::En);
    let data: Vec<&String> = labels
        .iter()
        .zip(disabled.iter())
        .filter_map(|(l, d)| if *d { None } else { Some(l) })
        .collect();
    let attached_label = data[0];
    let detached_label = data[1];

    assert!(
        attached_label.contains("(attached)"),
        "EN attached marker must be localized, not a bare glyph: {attached_label}"
    );
    assert!(
        !attached_label.contains('*'),
        "EN attached marker must not use a bare `*`: {attached_label}"
    );
    assert!(
        !detached_label.contains("(attached)"),
        "detached row must not advertise the attached marker: {detached_label}"
    );
}

#[test]
fn global_attach_label_localizes_attached_marker_in_zh_cn() {
    let i18n = StaticI18n::new();
    let records = vec![
        session_record("demo-app", "main", "wh@instance@demo-app@main", true),
        session_record("demo-app", "main", "wh@instance@demo-app@main", false),
    ];
    let plan = build_global_attach_plan(&records, &i18n, Locale::ZhCn);
    let (labels, disabled) = render_global_attach_lines(&plan, 200, &i18n, Locale::ZhCn);
    let data: Vec<&String> = labels
        .iter()
        .zip(disabled.iter())
        .filter_map(|(l, d)| if *d { None } else { Some(l) })
        .collect();
    let attached_label = data[0];
    let detached_label = data[1];

    assert!(
        attached_label.contains("（已连接）"),
        "zh-CN attached marker must be localized: {attached_label}"
    );
    assert!(
        !attached_label.contains('*'),
        "zh-CN attached marker must not use a bare `*`: {attached_label}"
    );
    assert!(
        !detached_label.contains("（已连接）"),
        "detached row must not advertise the attached marker: {detached_label}"
    );
}

#[test]
fn decorate_attach_rows_adds_path_and_layout_recent_keeping_session_first() {
    let i18n = StaticI18n::new();
    let workspaces = vec![
        ws_info("wh@instance@proj@main", false),
        ws_info("wh@instance@proj@feature-x", true),
    ];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);

    let lookup = |workspace: &str| match workspace {
        "main" => Some(AttachRowEnrichment {
            path: Some("~/code/proj".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "3 panes".to_owned(),
                recent: "2026-05-10 10:00".to_owned(),
            }),
        }),
        "feature-x" => Some(AttachRowEnrichment {
            path: Some("~/code/proj-feature-x".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "2 panes".to_owned(),
                recent: "2026-05-11 09:30".to_owned(),
            }),
        }),
        _ => None,
    };
    decorate_attach_selector_plan(&mut plan, &lookup);
    let data = render_data_rows(&plan, 200, &i18n, Locale::En);

    assert!(
        data[0].starts_with("wh@instance@proj@main"),
        "session id must stay first after enrichment: {}",
        data[0]
    );
    assert!(
        data[0].contains("~/code/proj"),
        "row must show path: {}",
        data[0]
    );
    assert!(
        data[0].contains("3 panes"),
        "row must show layout/pane summary: {}",
        data[0]
    );
    assert!(data[1].starts_with("wh@instance@proj@feature-x"));
    assert!(data[1].contains("~/code/proj-feature-x"));
    assert!(
        data[1].contains("2026-05-11 09:30"),
        "row must show last-used timestamp: {}",
        data[1]
    );
}

#[test]
fn decorate_attach_rows_without_match_keep_bare_session_first_label() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@feature-x", false)];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    decorate_attach_selector_plan(&mut plan, &|_| None);
    // No metadata or path was attached, so the row stays in its
    // bare session-first identity. The renderer fills empty
    // optional cells with blanks rather than dropping the row.
    assert!(plan.rows[0].path.is_none());
    assert!(plan.rows[0].metadata.is_none());
    let data = render_data_rows(&plan, 200, &i18n, Locale::En);
    assert!(data[0].starts_with("wh@instance@proj@feature-x"));
}

#[test]
fn global_attach_label_includes_project_root_path() {
    let i18n = StaticI18n::new();
    let records = vec![session_record(
        "demo-app",
        "main",
        "wh@instance@demo-app@main",
        false,
    )];
    let plan = build_global_attach_plan(&records, &i18n, Locale::En);
    let (labels, disabled) = render_global_attach_lines(&plan, 200, &i18n, Locale::En);
    let data: Vec<&String> = labels
        .iter()
        .zip(disabled.iter())
        .filter_map(|(l, d)| if *d { None } else { Some(l) })
        .collect();
    assert!(
        data[0].contains("/tmp/demo-app"),
        "global attach row must surface the project root path: {}",
        data[0]
    );
}

#[test]
fn attach_plan_groups_root_and_worktree_into_peer_sections() {
    // The project-scoped attach selector presents project-root and
    // worktree sessions as distinct peer sections (mirroring
    // `workspace open` / `workspace ls`), while each selectable row
    // still leads with its session id.
    let i18n = StaticI18n::new();
    let workspaces = vec![
        ws_info("wh@instance@proj@main", false),
        ws_info("wh@instance@proj@feature-x", false),
        ws_info("wh@instance@proj@bugfix", true),
    ];
    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    assert_eq!(plan.rows.len(), 3);

    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::En);

    let root_idx = labels
        .iter()
        .position(|l| l == "Project root")
        .expect("project-root section label present");
    let wt_idx = labels
        .iter()
        .position(|l| l == "Worktree workspaces")
        .expect("worktree section label present");
    assert!(
        root_idx < wt_idx,
        "project-root section must precede worktree section: {labels:?}"
    );
    assert!(
        disabled[root_idx] && disabled[wt_idx],
        "section labels must be non-selectable: {disabled:?}"
    );
    assert!(
        wt_idx >= 1 && labels[wt_idx - 1].is_empty(),
        "a blank line must separate the two peer sections: {labels:?}"
    );

    // Selectable data rows still lead with the session id and stay
    // grouped root-first.
    let data: Vec<&String> = labels
        .iter()
        .zip(disabled.iter())
        .filter_map(|(l, d)| if *d { None } else { Some(l) })
        .collect();
    assert_eq!(data.len(), 3);
    assert!(data[0].starts_with("wh@instance@proj@main"));
    assert!(data[1].starts_with("wh@instance@proj@feature-x"));
    assert!(data[2].starts_with("wh@instance@proj@bugfix"));
}

#[test]
fn attach_selectable_display_indices_map_to_expected_plan_rows() {
    // Every selectable display row must map back — via the same
    // "subtract preceding non-selectable rows" rule the selection
    // dispatch uses — to the matching `plan.rows` entry, not merely
    // render in the right label order.
    let i18n = StaticI18n::new();
    let workspaces = vec![
        ws_info("wh@instance@proj@main", false),
        ws_info("wh@instance@proj@feature-x", false),
        ws_info("wh@instance@proj@bugfix", true),
    ];
    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::En);

    let mut mapped = Vec::new();
    for (display_idx, is_disabled) in disabled.iter().enumerate() {
        if *is_disabled {
            continue;
        }
        // Reproduce the production display-index → plan.rows mapping.
        let row_index = display_idx - disabled[..display_idx].iter().filter(|d| **d).count();
        let row = &plan.rows[row_index];
        assert!(
            labels[display_idx].starts_with(&row.session_id),
            "display row {display_idx} must map to plan.rows[{row_index}] ({}), got {:?}",
            row.session_id,
            labels[display_idx]
        );
        mapped.push(row_index);
    }

    // Each plan row is reached exactly once, in plan order.
    assert_eq!(mapped, (0..plan.rows.len()).collect::<Vec<_>>());
}

#[test]
fn unknown_attach_session_renders_in_other_section_not_worktree() {
    // A session whose name does not parse into a project workspace
    // is an "Other sessions" fallback row — never folded under the
    // worktree section — and stays selectable and correctly mapped.
    let i18n = StaticI18n::new();
    let workspaces = vec![
        ws_info("wh@instance@proj@main", false),
        ws_info("wh@instance@proj@feature-x", false),
        ws_info("legacy-session-noschema", false),
    ];
    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::En);

    let wt_idx = labels
        .iter()
        .position(|l| l == "Worktree workspaces")
        .expect("worktree section label present");
    let other_idx = labels
        .iter()
        .position(|l| l == "Other sessions")
        .expect("other-sessions section label present");
    assert!(
        disabled[wt_idx] && disabled[other_idx],
        "section labels must be non-selectable"
    );
    assert!(
        wt_idx < other_idx,
        "worktree section must precede the other-sessions section: {labels:?}"
    );

    // The unknown row sits under the other-sessions label, not the
    // worktree label, and stays selectable.
    let unknown_display = labels
        .iter()
        .position(|l| l.starts_with("legacy-session-noschema"))
        .expect("unknown row rendered");
    assert!(
        unknown_display > other_idx,
        "unknown row must sit under the other-sessions label: {labels:?}"
    );
    assert!(
        !disabled[unknown_display],
        "unknown row must stay selectable"
    );

    // The selection mapping resolves to the unknown plan row with
    // the right session id.
    let row_index = unknown_display - disabled[..unknown_display].iter().filter(|d| **d).count();
    assert_eq!(plan.rows[row_index].kind, AttachWorkspaceKind::Unknown);
    assert_eq!(plan.rows[row_index].session_id, "legacy-session-noschema");
}

// ── Readable responsive layout ──

#[test]
fn render_attach_selector_inserts_disabled_header_row_when_columns_visible() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@main", false)];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    decorate_attach_selector_plan(&mut plan, &|workspace| match workspace {
        "main" => Some(AttachRowEnrichment {
            path: Some("~/code/proj".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "3 panes".to_owned(),
                recent: "2026-05-10".to_owned(),
            }),
        }),
        _ => None,
    });
    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::En);
    // Sequence: [root section label (disabled), column header
    // (disabled), data row (selectable)].
    assert_eq!(labels.len(), 3, "section label + header + 1 data row");
    assert_eq!(disabled, vec![true, true, false]);
    assert_eq!(labels[0], "Project root", "first row is the section label");
    // Column-header row carries the localized session column label.
    assert!(
        labels[1].contains("Session"),
        "header row must surface the session column label: {}",
        labels[1]
    );
    // Data row still leads with the session id.
    assert!(
        labels[2].starts_with("wh@instance@proj@main"),
        "data row must lead with the session id: {}",
        labels[2]
    );
}

/// Narrow layout: when the aligned table can no longer keep the
/// core decision columns, the attach selector renders a compact
/// summary that still carries the session id together with the
/// workspace and its kind — never a bare session id. Optional
/// panes/recent/path are shed first.
#[test]
fn render_attach_selector_narrow_keeps_session_id_workspace_and_kind() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@i@p@wk", true)];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    decorate_attach_selector_plan(&mut plan, &|_| {
        Some(AttachRowEnrichment {
            path: Some("/very/long/path/that/will/force/columns/to/drop".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "5 panes".to_owned(),
                recent: "2026-05-11 09:30".to_owned(),
            }),
        })
    });
    // 28 columns is too narrow for the aligned table to keep the
    // workspace + kind columns, so the renderer switches to a
    // compact summary; the budget still holds the session id,
    // workspace name and kind while the optional panes/recent/path
    // are shed first.
    let (labels, disabled) = render_attach_selector_lines(&plan, 28, &i18n, Locale::En);
    // Compact mode emits no disabled column-header row.
    assert!(
        !disabled.iter().any(|d| *d),
        "compact attach mode must not emit a column-header row: {disabled:?}"
    );
    let data_row = labels.last().expect("at least one row rendered");
    assert!(
        data_row.contains("wh@i@p@wk"),
        "narrow layout must keep the session id visible: {data_row}"
    );
    assert!(
        data_row.contains("wk"),
        "narrow layout must keep the workspace name: {data_row}"
    );
    assert!(
        data_row.contains("Worktree"),
        "narrow layout must keep the workspace kind, not degrade to a bare id: {data_row}"
    );
}

/// Compact mode has no column-header row, so the kind must be visually
/// tagged (bracketed) to stay distinguishable from the workspace name.
#[test]
fn render_attach_selector_compact_brackets_kind_to_distinguish_from_workspace() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@i@p@wk", false)];
    let plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    // Narrow enough to force the compact summary path.
    let (labels, disabled) = render_attach_selector_lines(&plan, 28, &i18n, Locale::En);
    assert!(
        !disabled.iter().any(|d| *d),
        "fixture must exercise the compact path: {disabled:?}"
    );
    let data_row = labels.last().expect("at least one row rendered");
    assert!(
        data_row.contains("[Worktree]"),
        "compact row must bracket the kind so it reads as a category tag: {data_row}"
    );
    assert!(
        data_row.contains("wk"),
        "compact row must keep the bare workspace name: {data_row}"
    );
}

/// The selector titles must say this is a Workhive workspace-session
/// attach flow, in both locales and for both the project-scoped and the
/// global entry points.
#[test]
fn attach_selector_titles_identify_workhive_workspace_session_flow() {
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;

    let i18n = StaticI18n::new();

    let en = i18n.resolve(Locale::En, k::ATTACH_SELECT_SESSION);
    assert!(
        en.contains("Workhive") && en.contains("workspace session"),
        "EN project-scoped title must name the Workhive workspace session flow: {en}"
    );
    let zh = i18n.resolve(Locale::ZhCn, k::ATTACH_SELECT_SESSION);
    assert!(
        zh.contains("Workhive") && zh.contains("工作区会话"),
        "zh-CN project-scoped title must name the Workhive workspace session flow: {zh}"
    );

    let en_global = i18n.resolve(Locale::En, k::ATTACH_GLOBAL_SELECT);
    assert!(
        en_global.contains("Workhive") && en_global.contains("workspace session"),
        "EN global title must name the Workhive workspace session flow: {en_global}"
    );
    let zh_global = i18n.resolve(Locale::ZhCn, k::ATTACH_GLOBAL_SELECT);
    assert!(
        zh_global.contains("Workhive") && zh_global.contains("工作区会话"),
        "zh-CN global title must name the Workhive workspace session flow: {zh_global}"
    );
}

/// Column headers must clarify what each cell holds: the workspace
/// column is the workspace *name* (explicit in zh-CN), and the kind
/// labels are the typed Project root / Worktree / Unknown values.
#[test]
fn attach_selector_clarifies_workspace_name_column_and_kind_labels() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@main", false)];

    let plan_zh = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::ZhCn);
    assert_eq!(
        plan_zh.column_headers.workspace, "工作区名",
        "zh-CN workspace column header must read as workspace name"
    );

    let kind_labels_en: Vec<String> = [
        AttachWorkspaceKind::ProjectRoot,
        AttachWorkspaceKind::Worktree,
        AttachWorkspaceKind::Unknown,
    ]
    .into_iter()
    .map(|kind| kind.label(&i18n, Locale::En))
    .collect();
    assert_eq!(
        kind_labels_en,
        vec!["Project root", "Worktree", "Unknown"],
        "EN kind labels must be the typed Project root / Worktree / Unknown values"
    );
}

/// Wide layout: the kind column must fit localized kind labels
/// wider than the column header, and the following path column must
/// remain aligned between the header and data rows.
#[test]
fn attach_selector_kind_column_fits_localized_label_wider_than_header() {
    use crate::table::display_width;

    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@main", false)];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::En);
    decorate_attach_selector_plan(&mut plan, &|_| {
        Some(AttachRowEnrichment {
            path: Some("~/code/proj".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "3 panes".to_owned(),
                recent: "2026-05-10".to_owned(),
            }),
        })
    });

    // The row is the wide-kind ProjectRoot variant whose label
    // ("Project root") is strictly wider than the "Kind" header.
    assert_eq!(plan.rows[0].kind, AttachWorkspaceKind::ProjectRoot);
    let kind_label = plan.rows[0].kind.label(&i18n, Locale::En);
    let kind_label_w = display_width(&kind_label);
    assert!(
        kind_label_w > display_width("Kind"),
        "test fixture must use a kind label wider than the header: {kind_label:?}"
    );

    // The computed kind column must hold the full localized label.
    let visible = AttachSelectorVisibleColumns::all();
    let widths = compute_attach_selector_column_widths(&plan, &visible, &i18n, Locale::En);
    // widths order: [session, workspace, kind, path, attached, panes, recent]
    let kind_w = widths[2];
    assert!(
        kind_w >= kind_label_w,
        "kind column width ({kind_w}) must be at least the rendered label width ({kind_label_w})"
    );

    // Wide render keeps the aligned table. The column AFTER kind
    // (path) must begin at the same display offset in both the header
    // and the data row, proving the wider kind cell did not shove the
    // trailing columns off their shared boundary.
    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::En);
    // Sequence: [section label, column header, data row]; the leading
    // section + column-header rows are non-selectable.
    assert!(disabled[0], "section label row is non-selectable");
    assert!(disabled[1], "wide layout keeps the column-header row");
    let header = &labels[1];
    let data_row = &labels[2];
    // session + workspace + kind columns, plus the two-space gaps
    // between the four leading columns (3 gaps before the path column).
    let path_offset = widths[0] + widths[1] + widths[2] + 2 * 3;
    // The leading cells are ASCII here, so a display-column offset is a
    // byte offset; slice defensively in case padding is shorter.
    let header_tail = header.get(path_offset..).unwrap_or("");
    let data_tail = data_row.get(path_offset..).unwrap_or("");
    assert!(
        header_tail.starts_with("Path"),
        "path header must start at the shared boundary {path_offset}: {header:?}"
    );
    assert!(
        data_tail.starts_with("~/code/proj"),
        "path value must start at the same boundary {path_offset}: {data_row:?}"
    );
}

#[test]
fn render_attach_selector_renders_localized_headers_in_zh_cn() {
    let i18n = StaticI18n::new();
    let workspaces = vec![ws_info("wh@instance@proj@main", false)];
    let mut plan = build_attach_selector_plan(&workspaces, "main", &i18n, Locale::ZhCn);
    decorate_attach_selector_plan(&mut plan, &|_| {
        Some(AttachRowEnrichment {
            path: Some("~/code/proj".to_owned()),
            metadata: Some(AttachRowMetadata {
                panes_layout: "3".to_owned(),
                recent: "2026-05-10".to_owned(),
            }),
        })
    });
    let (labels, disabled) = render_attach_selector_lines(&plan, 200, &i18n, Locale::ZhCn);
    // labels[0] is the localized section label; labels[1] is the
    // column-header row carrying the session/workspace labels.
    assert_eq!(
        labels[0], "项目根目录",
        "first row is the zh-CN section label"
    );
    assert!(disabled[0]);
    assert!(disabled[1]);
    let header = &labels[1];
    assert!(
        header.contains("会话"),
        "zh-CN header row must include the session label: {header}"
    );
    assert!(
        header.contains("工作区"),
        "zh-CN header row must include the workspace label: {header}"
    );
}

#[test]
fn render_global_attach_inserts_disabled_header_row_when_columns_visible() {
    let i18n = StaticI18n::new();
    let records = vec![session_record(
        "demo-app",
        "main",
        "wh@instance@demo-app@main",
        false,
    )];
    let plan = build_global_attach_plan(&records, &i18n, Locale::En);
    let (labels, disabled) = render_global_attach_lines(&plan, 200, &i18n, Locale::En);
    assert_eq!(labels.len(), 2, "header + 1 data row");
    assert_eq!(disabled, vec![true, false]);
    assert!(
        labels[0].contains("Session"),
        "global attach header row must surface the session label: {}",
        labels[0]
    );
    assert!(labels[1].starts_with("wh@instance@demo-app@main"));
}

#[test]
fn render_global_attach_narrow_keeps_session_id_project_and_workspace() {
    let i18n = StaticI18n::new();
    let records = vec![session_record("demo", "feat-x", "wh@i@demo@feat-x", false)];
    let plan = build_global_attach_plan(&records, &i18n, Locale::En);
    // Width chosen so the aligned table cannot keep both project and
    // workspace columns, forcing the compact summary path.
    let (labels, disabled) = render_global_attach_lines(&plan, 38, &i18n, Locale::En);
    assert!(
        !disabled.iter().any(|d| *d),
        "compact global attach mode must not emit a column-header row: {disabled:?}"
    );
    let data_row = labels.last().expect("at least one row rendered");
    assert!(
        data_row.contains("wh@i@demo@feat-x"),
        "narrow global attach layout must keep the session id visible: {data_row}"
    );
    assert!(
        data_row.contains("demo"),
        "narrow global attach layout must keep the project: {data_row}"
    );
    assert!(
        data_row.contains("feat-x"),
        "narrow global attach layout must keep the workspace: {data_row}"
    );
}

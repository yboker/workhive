#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// One interactive-selector action surface for `workspace open`.
///
/// `Root` and `Worktree` rows each carry the workspace branch they
/// will dispatch to; `CreateNew` triggers the new-worktree text-input
/// flow without binding to any existing branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OpenSelectorAction {
    Root { branch: String },
    Worktree { branch: String },
    CreateNew,
}

/// Pure plan describing the rows the open selector will render.
///
/// The plan is split into three structural buckets so layout/grouping
/// logic stays testable without spinning up the TUI:
///
/// - `roots` — the project root row (always exactly one entry today,
///   typed as a Vec so future multi-root projects do not require a
///   selector rewrite).
/// - `worktrees` — every linked worktree workspace (excluding main).
/// - `create_new` — the standalone "+ New worktree…" action that
///   must remain visually separate from the worktree group.
///
/// Each row carries its display `label` plus the `OpenSelectorAction`
/// it dispatches when chosen, so the renderer can collapse the plan
/// into a flat label vector + parallel disabled mask + an action
/// lookup table without re-deriving the structure.
#[derive(Debug, Clone)]
pub(crate) struct OpenSelectorPlan {
    pub roots: Vec<OpenSelectorRow>,
    pub worktrees: Vec<OpenSelectorRow>,
    pub create_new: OpenSelectorRow,
    pub group_root_header: String,
    pub group_worktree_header: String,
    pub group_worktree_empty: String,
    /// Localized column header labels used when an existing-workspace
    /// row carries operational metadata. Kept on the plan so the
    /// selector renderer never re-resolves i18n strings and so tests
    /// can assert the header copy without instantiating the live
    /// catalog at every site.
    pub column_headers: OpenSelectorColumnHeaders,
}

#[derive(Debug, Clone)]
pub(crate) struct OpenSelectorRow {
    pub label: String,
    pub action: OpenSelectorAction,
    /// Per-column operational context (session availability, layout /
    /// pane summary, last-used timestamp) for an existing
    /// workspace row. `None` when no enrichment is available (project
    /// is not initialized, or a probe failed) — the row degrades to
    /// just its workspace name rather than fabricating status. The
    /// fields are kept as separate strings (not pre-joined) so the
    /// renderer can compute display-width-aware column padding and
    /// progressively drop the lowest-priority columns to fit narrow
    /// terminals.
    pub enrichment: Option<OpenSelectorRowMetadata>,
}

/// Pre-localized values for the per-row metadata that the selector
/// renders alongside the workspace identity. Each field is the
/// final display string for the corresponding column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenSelectorRowMetadata {
    pub session: String,
    pub layout: String,
    pub recent: String,
}

/// Pre-localized column header labels shown above existing-workspace
/// rows. The selector renderer pulls these straight onto the
/// disabled header row so the column labels match the data
/// values rendered on each row.
#[derive(Debug, Clone)]
pub(crate) struct OpenSelectorColumnHeaders {
    /// Identity column for the worktree section: the workspace name.
    pub workspace: String,
    /// Identity column for the project-root section: the row there is
    /// the repository's main branch, so it reads as "Branch", not
    /// "Workspace".
    pub branch: String,
    pub session: String,
    pub layout: String,
    pub recent: String,
}

/// Build the selector plan from raw git data + i18n strings.
///
/// Pure: no IO, no runtime probes. Every row starts with no enrichment;
/// richer per-row context (session availability, layout, recent activity)
/// is layered on afterwards via [`decorate_open_selector_plan`]
/// so the structural/grouping logic stays testable in isolation from any
/// runtime lookup.
pub(crate) fn build_open_selector_plan(
    main_branch: &str,
    worktrees: &[workhive_core::ports::WorktreeInfo],
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: workhive_core::domain::Locale,
) -> OpenSelectorPlan {
    use workhive_core::i18n_keys as k;

    let root_row = OpenSelectorRow {
        label: i18n.resolve_with_args(locale, k::OPEN_MAIN_BRANCH, &[main_branch]),
        action: OpenSelectorAction::Root {
            branch: main_branch.to_owned(),
        },
        enrichment: None,
    };

    let mut worktree_rows: Vec<OpenSelectorRow> = Vec::new();
    for wt in worktrees {
        if wt.is_main {
            continue;
        }
        if let Some(branch) = wt.branch.as_ref() {
            worktree_rows.push(OpenSelectorRow {
                label: branch.clone(),
                action: OpenSelectorAction::Worktree {
                    branch: branch.clone(),
                },
                enrichment: None,
            });
        }
    }

    let create_new = OpenSelectorRow {
        label: i18n.resolve(locale, k::OPEN_CREATE_NEW_WT),
        action: OpenSelectorAction::CreateNew,
        enrichment: None,
    };

    OpenSelectorPlan {
        roots: vec![root_row],
        worktrees: worktree_rows,
        create_new,
        group_root_header: i18n.resolve(locale, k::OPEN_GROUP_PROJECT_ROOT),
        group_worktree_header: i18n.resolve(locale, k::OPEN_GROUP_WORKTREE),
        group_worktree_empty: i18n.resolve(locale, k::OPEN_GROUP_WORKTREE_EMPTY),
        column_headers: OpenSelectorColumnHeaders {
            workspace: i18n.resolve(locale, k::LIST_COL_WORKSPACE),
            branch: i18n.resolve(locale, k::WORKTREE_COL_BRANCH),
            session: i18n.resolve(locale, k::LIST_COL_SESSION),
            layout: i18n.resolve(locale, k::LIST_COL_LAYOUT),
            recent: i18n.resolve(locale, k::LIST_COL_RECENT),
        },
    }
}

/// Decorate the selectable rows (project root + worktrees) with their
/// per-column operational metadata.
///
/// `lookup` returns the per-column metadata for the row, or `None` when
/// no status is available (project not initialized, probe failure, …) —
/// in which case the row degrades to its bare workspace name rather
/// than fabricating status. The standalone "+ New worktree…" action is
/// never enriched because it does not bind to an existing workspace.
/// Kept pure so the wiring can be unit-tested with a stub lookup.
pub(crate) fn decorate_open_selector_plan(
    plan: &mut OpenSelectorPlan,
    lookup: &dyn Fn(&OpenSelectorAction) -> Option<OpenSelectorRowMetadata>,
) {
    for row in plan.roots.iter_mut().chain(plan.worktrees.iter_mut()) {
        row.enrichment = lookup(&row.action);
    }
}

/// Interactive branch/worktree selector when `workspace` arg is omitted.
///
/// Menu structure (grouped, with non-selectable headers):
/// 1. ── Project root ──
///    main workspace
/// 2. ── Worktree workspaces ──
///    each linked worktree (or empty-state hint)
/// 3. + New worktree...
///
/// Returns (workspace_name, WorkspaceOpenIntent).
pub(crate) fn select_workspace_interactive(
    git: &dyn GitPort,
    ctx: &CommandContext,
) -> Result<(String, workhive_core::domain::WorkspaceOpenIntent), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let main_branch = git.current_branch()?;
    let worktrees = git.list_worktrees()?;

    let mut plan = build_open_selector_plan(&main_branch, &worktrees, &ctx.i18n, locale);

    // Decorate each selectable row with the same session/layout/recent
    // metadata the `workspace list` table renders. Metadata is
    // best-effort: when the project is not initialized or any probe fails,
    // rows fall back to their bare workspace name rather than inventing
    // status. Workspaces are keyed by their stable kind + name so the
    // lookup never relies on parsing the rendered label.
    let enrichment = build_open_selector_enrichment(ctx);
    decorate_open_selector_plan(&mut plan, &|action| match action {
        OpenSelectorAction::Root { .. } => enrichment.root.clone(),
        OpenSelectorAction::Worktree { branch } => enrichment.worktrees.get(branch).cloned(),
        OpenSelectorAction::CreateNew => None,
    });

    // Render with column headers + aligned columns when the terminal
    // can fit them. When stdout is not connected to a TTY (CI, pipe),
    // fall back to a wide assumption so deterministic output stays
    // wide-mode column-aligned.
    let term_width = crate::table::detect_terminal_width_columns().unwrap_or(120);
    let (labels, disabled, actions) = render_open_selector_lines(&plan, term_width);

    let title = ctx.i18n.resolve(locale, k::OPEN_SELECT_BRANCH);
    // Default to the first selectable row (the project-root entry).
    let default = disabled.iter().position(|d| !d).unwrap_or(0);
    let selection = ui::screens::select::run_with_disabled(&title, &labels, &disabled, default)?;
    if let Some(label) = labels.get(selection) {
        crate::commands::print_choice_transcript(&title, label);
    }

    let action = actions
        .get(selection)
        .and_then(Option::as_ref)
        .ok_or_else(|| CliError::Runtime(ctx.i18n.resolve(locale, k::OPEN_INVALID_SELECTION)))?;

    match action {
        OpenSelectorAction::Root { branch } => {
            eprintln!("{}", ctx.i18n.resolve(locale, k::OPEN_MAIN_DIR_MODE));
            Ok((
                branch.clone(),
                workhive_core::domain::WorkspaceOpenIntent::UseProjectRoot,
            ))
        }
        OpenSelectorAction::Worktree { branch } => Ok((
            branch.clone(),
            workhive_core::domain::WorkspaceOpenIntent::Provision,
        )),
        OpenSelectorAction::CreateNew => {
            let prompt = ctx.i18n.resolve(locale, k::OPEN_ENTER_BRANCH);
            let new_branch = ui::screens::text_input::run(&prompt, "")?;
            if new_branch.is_empty() {
                return Err(CliError::UserAborted);
            }
            crate::commands::print_choice_transcript(&prompt, &new_branch);
            Ok((
                new_branch,
                workhive_core::domain::WorkspaceOpenIntent::Provision,
            ))
        }
    }
}

/// Per-row metadata for the open selector, keyed by stable identity.
struct OpenSelectorEnrichment {
    /// Metadata for the project-root row, if available.
    root: Option<OpenSelectorRowMetadata>,
    /// Metadata for each linked worktree, keyed by workspace name.
    worktrees: std::collections::HashMap<String, OpenSelectorRowMetadata>,
}

/// Gather session/layout/recent metadata for the open selector.
///
/// Best-effort: returns empty metadata when the project is not
/// initialized or any probe fails, so the selector degrades to bare
/// workspace names rather than fabricating status. Reuses
/// `collect_workspace_entries` — the same source the `workspace list`
/// table renders — so the two surfaces describe a workspace identically
/// column-by-column.
fn build_open_selector_enrichment(ctx: &CommandContext) -> OpenSelectorEnrichment {
    let mut enrichment = OpenSelectorEnrichment {
        root: None,
        worktrees: std::collections::HashMap::new(),
    };

    let Ok((root, name)) = detect_project() else {
        return enrichment;
    };
    let ws_fs = ctx.composition.workspace_fs();
    if !usecases::project_state::resolve_init_state(&ws_fs, root.as_ref()).is_initialized() {
        return enrichment;
    }

    let mut entries = match collect_workspace_entries(&root, &name, ctx) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::debug!(error = %e, "open selector metadata unavailable; using bare names");
            return enrichment;
        }
    };
    let layout = super::env_config::load_effective_layout(ctx, Some(root.as_ref()));
    apply_inactive_layout_fallback(&mut entries, &layout);

    let locale = ctx.locale();
    for entry in &entries {
        let parts = crate::commands::list::render_selector_enrichment_parts(entry, ctx, locale);
        let meta = OpenSelectorRowMetadata {
            session: parts.session,
            layout: parts.layout,
            recent: parts.recent,
        };
        match entry.kind {
            workhive_core::usecases::list::WorkspaceKind::ProjectRoot => {
                enrichment.root = Some(meta);
            }
            workhive_core::usecases::list::WorkspaceKind::LinkedWorktree => {
                enrichment.worktrees.insert(entry.name.clone(), meta);
            }
        }
    }

    enrichment
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    /// The open selector groups rows into Project root + Worktree
    /// workspaces + the standalone "+ New worktree…" action. The
    /// rendered output must surface those groups in that exact order
    /// so the operator's eye scans the project root first, the
    /// existing worktrees next, and the new-worktree creator last.
    /// Without per-row metadata, the layout collapses to bare section
    /// labels + data rows, with no column-header rows.
    #[test]
    fn open_selector_plan_groups_root_worktrees_and_create_new() {
        let i18n = FakeI18n;
        let worktrees = vec![
            worktree_info("main", true),
            worktree_info("feat-a", false),
            worktree_info("feat-b", false),
        ];

        let plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        assert_eq!(plan.roots.len(), 1, "exactly one project-root row");
        assert!(matches!(
            plan.roots[0].action,
            OpenSelectorAction::Root { .. }
        ));
        assert_eq!(plan.worktrees.len(), 2);
        assert!(
            plan.worktrees
                .iter()
                .all(|row| matches!(row.action, OpenSelectorAction::Worktree { .. }))
        );
        assert_eq!(plan.create_new.action, OpenSelectorAction::CreateNew);

        let (labels, disabled, actions) = render_open_selector_lines(&plan, 200);
        assert_eq!(labels.len(), disabled.len());
        assert_eq!(labels.len(), actions.len());

        // Layout without metadata: [root header (disabled), root row,
        // blank peer-break (disabled), wt header (disabled), wt-a row,
        // wt-b row, create-new row].
        assert_eq!(disabled, vec![true, false, true, true, false, false, false]);
        assert_eq!(labels[0], plan.group_root_header);
        assert!(
            labels[2].is_empty(),
            "a blank line must separate the two peer sections: {:?}",
            labels[2]
        );
        assert_eq!(labels[3], plan.group_worktree_header);
        let create_action = actions.last().and_then(Option::as_ref).cloned();
        assert_eq!(create_action, Some(OpenSelectorAction::CreateNew));
    }

    /// When no linked worktrees exist, the worktree group must still
    /// render with a localized empty-state hint so operators see the
    /// section is intentionally empty rather than missing entirely.
    /// The hint row is non-selectable (no action) so Up/Down/Enter
    /// flows past it cleanly.
    #[test]
    fn open_selector_plan_renders_empty_worktree_group_with_hint() {
        let i18n = FakeI18n;
        let worktrees = vec![worktree_info("main", true)];

        let plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        let (labels, disabled, actions) = render_open_selector_lines(&plan, 200);

        // Layout: [root header, root row, blank peer-break, wt header,
        // empty hint, create-new].
        assert_eq!(disabled, vec![true, false, true, true, true, false]);
        assert_eq!(labels[4], plan.group_worktree_empty);
        assert!(
            actions[4].is_none(),
            "the empty-state hint row must not dispatch an action"
        );
        assert_eq!(
            actions.last().and_then(Option::as_ref).cloned(),
            Some(OpenSelectorAction::CreateNew)
        );
    }

    /// Group terminology must not drift between `workspace ls` and
    /// `workspace open`. The open selector reuses the same group
    /// terminology so operators see consistent wording across both
    /// surfaces. We assert the group headers against the real i18n
    /// catalog so a future terminology drift fails this test rather
    /// than silently shipping mismatched copy.
    #[test]
    fn open_selector_plan_uses_consistent_group_terminology() {
        use workhive_infra::i18n::StaticI18n;
        let i18n = StaticI18n::new();
        let worktrees = vec![worktree_info("main", true)];

        let plan_en = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        assert_eq!(plan_en.group_root_header, "Project root");
        assert_eq!(plan_en.group_worktree_header, "Worktree workspaces");

        let plan_zh = build_open_selector_plan("main", &worktrees, &i18n, Locale::ZhCn);
        assert_eq!(plan_zh.group_root_header, "项目根目录");
        assert_eq!(plan_zh.group_worktree_header, "Worktree 工作区");
    }

    /// Decoration must attach session/layout/recent metadata
    /// to the root and worktree rows while leaving the bare workspace
    /// name as the leading column. Rows with no metadata stay bare,
    /// and the `+ new worktree` action never carries metadata.
    #[test]
    fn decorate_open_selector_plan_appends_enrichment_after_name() {
        let i18n = FakeI18n;
        let worktrees = vec![worktree_info("main", true), worktree_info("feat-a", false)];

        let mut plan = build_open_selector_plan("main", &worktrees, &i18n, Locale::En);
        decorate_open_selector_plan(&mut plan, &|action| match action {
            OpenSelectorAction::Root { .. } => Some(OpenSelectorRowMetadata {
                session: "running".to_owned(),
                layout: "3 panes".to_owned(),
                recent: "5m".to_owned(),
            }),
            OpenSelectorAction::Worktree { branch } if branch == "feat-a" => {
                Some(OpenSelectorRowMetadata {
                    session: "no session".to_owned(),
                    layout: "half_split (2)".to_owned(),
                    recent: "-".to_owned(),
                })
            }
            _ => None,
        });

        let root_meta = plan.roots[0]
            .enrichment
            .as_ref()
            .expect("root row must carry metadata after decoration");
        assert_eq!(root_meta.session, "running");
        assert_eq!(root_meta.layout, "3 panes");

        let feat = plan
            .worktrees
            .iter()
            .find(
                |r| matches!(&r.action, OpenSelectorAction::Worktree { branch } if branch == "feat-a"),
            )
            .expect("feat-a worktree row");
        assert_eq!(feat.label, "feat-a", "workspace name leads the row");
        let feat_meta = feat
            .enrichment
            .as_ref()
            .expect("feat-a row must carry metadata after decoration");
        assert_eq!(feat_meta.session, "no session");
        assert_eq!(feat_meta.layout, "half_split (2)");

        // `+ new worktree` is a creation action, never an existing
        // workspace, so it must stay bare.
        assert!(plan.create_new.enrichment.is_none());
    }
}

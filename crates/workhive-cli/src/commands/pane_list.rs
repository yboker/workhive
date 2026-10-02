//! `workhive pane list` rendering.
//!
//! Contains the pure view-model builders, the production renderer, and
//! the post-table tips / legend / hook-event-hint composers extracted
//! from `pane.rs`. Behaviour is preserved: this module owns display
//! composition only, while pane discovery, observation, and command
//! dispatch stay behind their existing command and port boundaries.
//!
//! The focused tests in this file exercise the renderer and post-table
//! composition directly so future layout changes stay local to this
//! module.

use camino::Utf8Path;
use workhive_core::domain::{
    AiActivityProvider, AiActivitySource, AiIntegrationLifecycleState, Locale, PaneKind,
    ToolCatalog, derive_concise_activity,
};
use workhive_core::i18n_keys as k;
use workhive_core::ports::I18nPort;
use workhive_core::usecases::ai_integrations::FeatureGateResolution;
use workhive_core::usecases::pane::{PaneListRow, pane_selection_label};

use crate::commands::pane_activity_overlay::{
    AiActivityOverlayEntry, now_unix_ms_from, read_pane_ai_activity_overlay,
    read_terminal_status_observations, resolve_ai_integration_gate, resolve_pane_lifecycle_state,
    resolve_pane_observation_gate,
};
use crate::commands::pane_metrics_view;
use crate::context::CommandContext;

/// Width cap for the per-row ACP metrics summary line in `pane list`.
///
/// Each metric line is rendered as `  <target_id>: <render_list_metrics_line>`;
/// the prefix is small and the body is capped here so narrow
/// terminals (≈80 columns) keep the summary on one visible row.
const PANE_LIST_METRICS_MAX_WIDTH: usize = 80;

/// Build the render plan (column headers + table rows) for default
/// `pane list` output. Pure so it can be unit-tested without
/// exercising tmux or capturing stdout.
pub(crate) fn build_pane_list_render(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    ai_overlay: &[AiActivityOverlayEntry],
) -> (Vec<String>, Vec<Vec<String>>) {
    let i18n = &ctx.i18n;

    let headers = vec![
        i18n.resolve(locale, k::PANE_LIST_HEADER_ACTIVE),
        i18n.resolve(locale, k::PANE_LIST_HEADER_NUMBER),
        i18n.resolve(locale, k::PANE_LIST_HEADER_TARGET_ID),
        i18n.resolve(locale, k::PANE_LIST_HEADER_LABEL),
        i18n.resolve(locale, k::PANE_LIST_HEADER_KIND),
        // The visible status column renders the typed activity state,
        // not the raw `PaneState`. AI provider hook facts are layered
        // on top: when a non-stale `AiActivityFact` exists, its
        // localized state replaces the supervisor-derived activity
        // for that row only. The header itself does not change because
        // the AI overlay is purely a per-row substitution.
        i18n.resolve(locale, k::PANE_LIST_HEADER_ACTIVITY),
        // Detection-source is a separate column so the
        // activity column stays concise on narrow terminals. The
        // tokens come straight from
        // `AiActivitySource::tag_i18n_key()` (`hook` / `plugin` /
        // `supervisor` / `process` / `tmux` / `none`) and never
        // mix with the activity label.
        i18n.resolve(locale, k::PANE_LIST_HEADER_SOURCE),
        i18n.resolve(locale, k::PANE_LIST_HEADER_CONFIDENCE),
        i18n.resolve(locale, k::PANE_LIST_HEADER_CWD),
    ];

    let label_none = i18n.resolve(locale, k::PANE_LIST_LABEL_NONE);
    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let active_marker = if row.active { "*" } else { "" };
            let kind = i18n.resolve(locale, row.kind.i18n_key());
            let overlay = ai_overlay.get(idx);
            // `pane list` always renders one of the five concise labels
            // (waiting-input / running / completed / exited / unknown).
            // Verbose detail (raw activity / overlay reason / hook source
            // ordering) belongs to `pane show`. AI overlay state, when
            // present, overrides the supervisor-derived activity so an
            // AI pane reads as "running" while the model is working and
            // "completed" once it hands control back to the user.
            let concise = derive_concise_activity(row.activity, overlay.and_then(|e| e.state));
            let activity = i18n.resolve(locale, concise.i18n_key());
            // Source column. Resolves to the product-facing token
            // (`hook` / `plugin` / `supervisor` / `process` / `tmux`)
            // for AI rows, or the conservative `none` token when no
            // typed source is available — making the absence of a
            // signal explicit instead of leaving the cell blank.
            let source = i18n.resolve(
                locale,
                overlay
                    .and_then(|e| e.source)
                    .unwrap_or(AiActivitySource::None)
                    .tag_i18n_key(),
            );
            let confidence = i18n.resolve(
                locale,
                overlay
                    .and_then(|e| e.source)
                    .unwrap_or(AiActivitySource::None)
                    .accuracy()
                    .i18n_key(),
            );
            let label = pane_selection_label(row).unwrap_or_else(|| label_none.clone());
            vec![
                active_marker.to_owned(),
                row.index.to_string(),
                row.target_id.as_str().to_owned(),
                truncate_for_column(&label, 18),
                kind,
                activity,
                source,
                confidence,
                display_pane_cwd(&row.cwd),
            ]
        })
        .collect();
    (headers, table_rows)
}

/// Build the structured hint block emitted directly under the
/// `pane list` table.
///
/// Layout contract:
///
///   * Each example command (`workhive pane send 2 "..."`,
///     `workhive pane send %2 "..."`) renders on its own indented
///     line, never passed through a width-aware wrapper. In Chinese
///     locale a generic wrapper used to split the verbatim example
///     mid-command at arbitrary CJK character boundaries, so users
///     could not copy/paste the example as a single command.
///   * The `active` column's `*` glyph gets its own explanation line
///     so new users do not have to guess what the marker means.
///
/// The structured block produced here:
///
///   1. `Tips:` heading line (no indent — visually announces the block).
///   2. Indented `* in the active column marks ...` line — explains
///      the marker without renaming the column.
///   3. Indented `Send to a pane by its number ... e.g.:` lead-in.
///   4. Two further-indented verbatim example commands. Because they
///      are rendered as their own lines they cannot be split by
///      width-aware wrapping.
///
/// Pure: returns a `Vec<String>` of fully-rendered lines so the CLI
/// just `println!`s each one. View-model unit tests cover both
/// locales without going through stdout.
///
/// This helper is called by the production pane-list renderer. Keep
/// it outside `#[cfg(test)]` so the explanation of the `*` marker
/// and the `pane send` examples reach the terminal.
pub(crate) fn build_pane_list_hint_block(ctx: &CommandContext, locale: Locale) -> Vec<String> {
    let i18n = &ctx.i18n;

    vec![
        i18n.resolve(locale, k::PANE_LIST_HINT_HEADING),
        format!(
            "  {}",
            i18n.resolve(locale, k::PANE_LIST_HINT_ACTIVE_MARKER)
        ),
        format!("  {}", i18n.resolve(locale, k::PANE_LIST_HINT_SEND_INTRO)),
        "    workhive pane send 2 \"...\"".to_owned(),
        "    workhive pane send %2 \"...\"".to_owned(),
    ]
}

/// Build the post-table "column legend" sub-block.
///
/// The `pane list` table exposes three pieces of vocabulary that
/// need inline explanation:
///
///   1. The `open (activity unverified)` activity label, which
///      historically read as a hard "not working" assertion when
///      it really means "AI tool open, no hook/plugin event yet".
///   2. The `source` column, which carried short tokens (`hook`,
///      `tmux`, …) with no inline accuracy ordering.
///   3. The `confidence` column, which read as an opaque ranking.
///
/// This legend block produces one heading line plus three indented
/// explanation lines. Hook approval guidance lives in the separate
/// [`build_pane_list_no_hook_event_hint`] block above so
/// high-confidence rows do not surface redundant approval prompts.
///
/// Pure: returns a `Vec<String>` of fully-rendered lines.
pub(crate) fn build_pane_list_legend_block(ctx: &CommandContext, locale: Locale) -> Vec<String> {
    let i18n = &ctx.i18n;

    vec![
        i18n.resolve(locale, k::PANE_LIST_LEGEND_HEADING),
        i18n.resolve(locale, k::PANE_LIST_LEGEND_ACTIVITY_UNVERIFIED),
        i18n.resolve(locale, k::PANE_LIST_LEGEND_SOURCE),
        i18n.resolve(locale, k::PANE_LIST_LEGEND_CONFIDENCE),
    ]
}

/// Build the post-table "no hook/plugin event yet" sub-block.
///
/// Workhive cannot prove from outside an AI pane that
/// hooks are uninstalled — the user might have approved them but simply
/// not produced an event yet (e.g. Codex `/hooks` pending review). The
/// block states **only the observable fact** ("no hook/plugin activity
/// event has been observed yet for some AI panes") and points the user
/// at the `source` column to identify which rows are affected.
///
/// The block is empty when:
///
///   * the feature-gate resolution is `FeatureGateResolution::Unavailable`,
///   * AI integrations are globally disabled,
///   * there are no AI panes at all, OR
///   * every AI pane has reached `AiIntegrationLifecycleState::Active`.
///
/// The block intentionally never references `workhive integrations`
/// (rescinded command).
pub(crate) fn build_pane_list_no_hook_event_hint(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    ai_overlay: &[AiActivityOverlayEntry],
    catalog: &ToolCatalog,
    resolution: &FeatureGateResolution,
) -> Vec<String> {
    let i18n = &ctx.i18n;

    // Unavailable gate fails closed for display: emit no AI integration
    // guidance at all. The pane table itself still renders because it
    // does not depend on this hint block.
    let Some(gate) = resolution.resolved_gate() else {
        return Vec::new();
    };
    // Globally disabled = fully silent on AI integration guidance.
    if !gate.globally_enabled() {
        return Vec::new();
    }

    // Walk every row and resolve a typed lifecycle state through the
    // shared resolver. The lifecycle decision drives a single neutral
    // "waiting for a completion signal" hint; Workhive no longer
    // installs provider-side hooks/plugins, so the hint never asks
    // the user to approve anything in the provider UI.
    let mut any_pre_active = false;
    for (idx, row) in rows.iter().enumerate() {
        let overlay = ai_overlay.get(idx);
        let is_ai = matches!(row.kind, PaneKind::Agent)
            || overlay
                .and_then(|e| e.provider)
                .is_some_and(|p| !matches!(p, AiActivityProvider::None));
        if !is_ai {
            continue;
        }
        // Per-provider gate: skip rows whose resolved provider is
        // gated off so a per-provider disable hides every line.
        let provider_hint = overlay
            .and_then(|e| e.provider)
            .unwrap_or(AiActivityProvider::None);
        let resolved =
            AiActivityProvider::resolve_for_pane(provider_hint, &row.current_command, catalog);
        if matches!(resolved, AiActivityProvider::None) {
            continue;
        }
        if !gate.is_provider_enabled(resolved) {
            continue;
        }
        let lifecycle = resolve_pane_lifecycle_state(resolved, overlay);
        if matches!(
            lifecycle,
            AiIntegrationLifecycleState::Active | AiIntegrationLifecycleState::Unsupported,
        ) {
            continue;
        }
        any_pre_active = true;
    }

    if !any_pre_active {
        return Vec::new();
    }
    vec![
        i18n.resolve(locale, k::PANE_LIST_HINT_NO_HOOK_EVENT_HEADING),
        i18n.resolve(locale, k::PANE_LIST_HINT_NO_HOOK_EVENT_BODY),
    ]
}

/// Render `workhive pane list` using the typed view produced by the
/// core use case.
///
/// Default-mode contract:
///
///   * No top-level `pane list` title — the table itself is the
///     output. Removing the title also removes a redundant CJK-wide
///     line that broke alignment in narrow terminals.
///   * The diagnostic `launch identity` column is hidden by default;
///     its information is still available via `pane show` (verbose
///     diagnostic surface).
///   * The user-recognizable selection label replaces the raw
///     `current command` column. The label prefers Workhive's durable
///     launch identity (e.g. `codex`, `claude`) and falls back to the
///     live foreground command, but never surfaces the internal
///     `workhive` supervisor wrapper as a recognizable tool.
pub(crate) fn print_pane_list(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
    catalog: &ToolCatalog,
) {
    let i18n = &ctx.i18n;

    if rows.is_empty() {
        println!("{}", i18n.resolve(locale, k::PANE_LIST_EMPTY));
        return;
    }

    let resolution = resolve_ai_integration_gate(ctx, project_root);
    let gate = resolution.effective_display_gate();
    let pane_gate = resolve_pane_observation_gate(ctx, project_root);
    let terminal_status_probe = ctx.composition.terminal_status_probe();
    let terminal_status = read_terminal_status_observations(
        rows,
        server,
        catalog,
        &gate,
        &pane_gate,
        &terminal_status_probe,
    );
    let clock = workhive_infra::clock::SystemClock;
    let now_ms = now_unix_ms_from(&clock);
    let overlay = read_pane_ai_activity_overlay(
        &ctx.composition,
        project_root,
        server,
        workspace,
        rows,
        locale,
        &ctx.i18n,
        catalog,
        &gate,
        &terminal_status,
        now_ms,
    );
    // Activity Intelligence v1: per-row ACP metrics. Feeds both the
    // human "AI metrics:" block and the JSON entries below. Loader
    // lives in `pane_metrics_view` so it stays unit-testable with a
    // fake `ActivityEventStore`.
    let event_store = ctx.composition.activity_event_store(project_root);
    let (metrics_per_row, has_legacy_per_row) =
        pane_metrics_view::load_pane_list_metrics(&event_store, server, workspace, rows, now_ms);
    let (headers, table_rows) = build_pane_list_render(ctx, locale, rows, &overlay);
    // 9 columns now: active, number, target_id, label, kind,
    // activity, source, confidence, cwd. The dedicated `source` column carries
    // a short product-facing token so its min width can stay tight.
    let min_widths: [usize; 9] = [2, 4, 6, 10, 6, 6, 4, 4, 10];
    let columns: Vec<crate::table::Column> = headers
        .into_iter()
        .zip(min_widths)
        .map(|(header, min_width)| crate::table::Column { header, min_width })
        .collect();
    // Drop priority on narrow terminals: `cwd` (index 7) drops
    // first because pane selection still works without it; the
    // `source` column (index 6) drops second so the activity
    // column itself never disappears. Activity stays visible at
    // every width because the activity-vs-source split is the
    // whole point of this layout.
    crate::table::print_table_compact(&columns, &table_rows, &[8, 7]);

    // ACP metrics block — additive secondary lines per pane that
    // surface provider/state/freshness/hint without breaking the
    // table's column widths on narrow terminals. The block prints
    // only when at least one row produced a non-empty summary.
    let metrics_block = pane_metrics_view::compose_pane_list_metrics_block(
        rows,
        &metrics_per_row,
        &has_legacy_per_row,
        &ctx.i18n as &dyn workhive_core::ports::I18nPort,
        locale,
        PANE_LIST_METRICS_MAX_WIDTH,
    );
    if !metrics_block.is_empty() {
        println!();
        for line in metrics_block {
            println!("{line}");
        }
    }

    // Delegate post-table composition to a pure helper so the exact
    // ordering and conditional rendering of tips, legend, and
    // hook/plugin guidance is unit-testable.
    for line in
        compose_pane_list_post_table_lines(ctx, locale, rows, &overlay, catalog, &resolution)
    {
        println!("{line}");
    }
}

/// Render the `pane list --view json` envelope.
///
/// Kept pure (entries in, string out) so the envelope shape
/// (`{"panes": [...]}`) is unit-testable without a live runtime.
/// Per-entry field schema checks live in `pane_metrics_view` tests;
/// this function only owns the top-level wrapper.
pub(crate) fn render_pane_list_json(entries: &[pane_metrics_view::PaneListJsonEntry]) -> String {
    let payload = serde_json::json!({ "panes": entries });
    serde_json::to_string_pretty(&payload).expect("JSON serialization must succeed")
}

/// Print the machine-readable `pane list --view json` document to
/// stdout. Uses the same observed rows and ACP metrics loader as the
/// human table so both views describe the same pane snapshot; an
/// empty session yields `{"panes": []}` rather than localized prose.
pub(crate) fn print_pane_list_json(
    ctx: &CommandContext,
    rows: &[PaneListRow],
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
) {
    let clock = workhive_infra::clock::SystemClock;
    let now_ms = now_unix_ms_from(&clock);
    let event_store = ctx.composition.activity_event_store(project_root);
    let (metrics_per_row, has_legacy_per_row) =
        pane_metrics_view::load_pane_list_metrics(&event_store, server, workspace, rows, now_ms);
    let entries = pane_metrics_view::build_pane_list_json_entries(
        rows,
        &metrics_per_row,
        &has_legacy_per_row,
    );
    println!("{}", render_pane_list_json(&entries));
}

/// Production-path composer for the lines `print_pane_list` emits
/// AFTER the table. Returns a flat `Vec<String>` where blank-line
/// separators are encoded as empty strings, so the caller only has
/// to `println!` each entry.
pub(crate) fn compose_pane_list_post_table_lines(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    overlay: &[AiActivityOverlayEntry],
    catalog: &ToolCatalog,
    resolution: &FeatureGateResolution,
) -> Vec<String> {
    // Default `pane list` is a status overview, not reference
    // documentation. We only emit the full Tips / Column legend /
    // no-hook-event blocks when the user explicitly opts in via
    // `workhive --verbose pane list`. Default mode emits one
    // concise hint plus at most one short degraded line.
    if ctx.verbose {
        compose_pane_list_post_table_lines_verbose(ctx, locale, rows, overlay, catalog, resolution)
    } else {
        compose_pane_list_post_table_lines_concise(ctx, locale, rows, overlay, catalog, resolution)
    }
}

/// Verbose post-table composer (`workhive --verbose pane list`). Emits
/// the full Tips block, column legend, and hook-event hint block.
fn compose_pane_list_post_table_lines_verbose(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    overlay: &[AiActivityOverlayEntry],
    catalog: &ToolCatalog,
    resolution: &FeatureGateResolution,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    out.push(String::new());
    out.extend(build_pane_list_hint_block(ctx, locale));

    out.push(String::new());
    out.extend(build_pane_list_legend_block(ctx, locale));

    let no_event_lines =
        build_pane_list_no_hook_event_hint(ctx, locale, rows, overlay, catalog, resolution);
    if !no_event_lines.is_empty() {
        out.push(String::new());
        out.extend(no_event_lines);
    }

    out
}

/// Concise default post-table composer. Emits one short hint line and,
/// when an AI pane is in a pre-active lifecycle, one short actionable
/// degraded line. Codex `/hooks` review takes precedence over the
/// generic pending-event hint because the user has a specific next
/// action.
fn compose_pane_list_post_table_lines_concise(
    ctx: &CommandContext,
    locale: Locale,
    rows: &[PaneListRow],
    overlay: &[AiActivityOverlayEntry],
    catalog: &ToolCatalog,
    resolution: &FeatureGateResolution,
) -> Vec<String> {
    let i18n = &ctx.i18n;

    let mut out: Vec<String> = Vec::new();
    out.push(String::new());
    out.push(i18n.resolve(locale, k::PANE_LIST_DEFAULT_HINT));

    let degraded = compute_pane_list_degraded_summary(rows, overlay, catalog, resolution);
    let degraded_key = match degraded {
        PaneListDegradedSummary::PendingEvent => Some(k::PANE_LIST_DEFAULT_HINT_PENDING_EVENT),
        PaneListDegradedSummary::None => None,
    };
    if let Some(key) = degraded_key {
        out.push(i18n.resolve(locale, key));
    }

    out
}

/// Classification of the highest-priority degraded AI-integration
/// state visible across the rendered `pane list` rows. Used to decide
/// which (if any) concise default degraded hint to print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneListDegradedSummary {
    PendingEvent,
    None,
}

/// Reuse the same lifecycle scan as
/// [`build_pane_list_no_hook_event_hint`] (the verbose path) so the
/// concise default hint never disagrees with the verbose hint about
/// whether *any* AI pane is degraded. Disabled gates and gated-off
/// providers stay silent exactly as in the verbose path.
fn compute_pane_list_degraded_summary(
    rows: &[PaneListRow],
    ai_overlay: &[AiActivityOverlayEntry],
    catalog: &ToolCatalog,
    resolution: &FeatureGateResolution,
) -> PaneListDegradedSummary {
    let Some(gate) = resolution.resolved_gate() else {
        return PaneListDegradedSummary::None;
    };
    if !gate.globally_enabled() {
        return PaneListDegradedSummary::None;
    }

    let mut any_pre_active = false;
    for (idx, row) in rows.iter().enumerate() {
        let overlay = ai_overlay.get(idx);
        let is_ai = matches!(row.kind, PaneKind::Agent)
            || overlay
                .and_then(|e| e.provider)
                .is_some_and(|p| !matches!(p, AiActivityProvider::None));
        if !is_ai {
            continue;
        }
        let provider_hint = overlay
            .and_then(|e| e.provider)
            .unwrap_or(AiActivityProvider::None);
        let resolved =
            AiActivityProvider::resolve_for_pane(provider_hint, &row.current_command, catalog);
        if matches!(resolved, AiActivityProvider::None) {
            continue;
        }
        if !gate.is_provider_enabled(resolved) {
            continue;
        }
        let lifecycle = resolve_pane_lifecycle_state(resolved, overlay);
        if matches!(
            lifecycle,
            AiIntegrationLifecycleState::Active | AiIntegrationLifecycleState::Unsupported,
        ) {
            continue;
        }
        any_pre_active = true;
    }

    if any_pre_active {
        PaneListDegradedSummary::PendingEvent
    } else {
        PaneListDegradedSummary::None
    }
}

/// Display helper for the pane `cwd` column shared by `pane list`
/// and `pane show`. Kept here because it originated as a list
/// helper; the show renderer imports it explicitly.
pub(crate) fn display_pane_cwd(cwd: &str) -> String {
    crate::path_display::display_path_for_humans(Utf8Path::new(cwd))
}

/// Truncate a string for fixed-width column rendering, falling back
/// to a placeholder when the original would otherwise overrun.
pub(crate) fn truncate_for_column(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        value.to_owned()
    } else if width <= 1 {
        "…".to_owned()
    } else {
        let mut acc = String::with_capacity(width);
        for ch in value.chars().take(width.saturating_sub(1)) {
            acc.push(ch);
        }
        acc.push('…');
        acc
    }
}

#[cfg(test)]
#[path = "pane_list_tests.rs"]
mod tests;

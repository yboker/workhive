//! ACP read path + AI activity overlay for the `workhive pane`
//! command surface.
//!
//! `pane list` and `pane show` both render a per-row "AI activity"
//! overlay that fuses three independent signals into one display
//! decision:
//!
//!   1. The Activity Control Plane (ACP) projected read model, built
//!      from the append-only event log when the AI integrations gate
//!      is globally enabled.
//!   2. Workhive's legacy latest-fact `PaneActivityStorePort` (a
//!      `ClaudeCodeHook` / `CodexHook` / `OpencodePlugin` fact written
//!      by hook scripts or plugins).
//!   3. The pane-side runtime observation ladder
//!      (`plugin > terminal status > supervisor > process > tmux >
//!      none`) derived from the live pane snapshot.
//!
//! The merge policy is owned by
//! [`workhive_core::domain::resolve_ai_activity_display`]; this module
//! is only the command-layer wiring that loads the gate, gathers the
//! signals, and converts them into [`AiActivityOverlayEntry`] values
//! the renderers already speak.
//!
//! All public symbols are `pub(crate)` so the sibling pane modules
//! (`pane`, `pane_list`, `pane_show`, `pane_send`) can wire them
//! through their dispatchers and tests without leaking the surface to
//! other crates. The module uses explicit imports rather than a glob
//! re-export from its parent.

use camino::Utf8Path;
use workhive_core::domain::activity::{PaneCorrelationKey, ReadModel};
use workhive_core::domain::{
    AiActivityConfidence, AiActivityFact, AiActivityProvider, AiActivitySource, AiActivityState,
    AiIntegrationLifecycleState, ForegroundAgentSignal, LifecycleObservation, Locale,
    PaneActivityState, TerminalStatusObservation, ToolCatalog, policy_for,
    resolve_ai_activity_display, resolve_lifecycle_state,
};
use workhive_core::ports::{
    ActivityEventStore, ClockPort, ConfigPort, I18nPort, PaneActivityKey, PaneActivityStorePort,
    TerminalStatusProbePort, TerminalStatusProbeRequest,
};
use workhive_core::usecases::activity_control_plane::{
    PaneActivityOverlayProjection, StateProjector, project_pane_fact_to_overlay,
};
use workhive_core::usecases::ai_integrations::{
    FeatureGate, FeatureGateResolution, PaneObservationGate,
};
use workhive_core::usecases::pane::PaneListRow;
use workhive_core::usecases::terminal_status::interpret_terminal_status;
use workhive_infra::pane_activity::FsPaneActivityStore;

use crate::composition::CliComposition;
use crate::context::CommandContext;

/// Per-row AI activity overlay used by `pane list` / `pane show`.
///
/// The visible activity column can reflect Workhive-recorded
/// provider hook events when they exist and are non-stale, rather than
/// the supervisor-derived `activity` field alone. Each entry carries:
///
///   * `label` — the localized AI activity state (e.g. "处理中" /
///     "Processing"), or `None` if no usable fact is on disk.
///   * `reason` — the optional reason string from the recorded fact,
///     suitable for display under `pane show` (never raw user data;
///     hooks are responsible for redaction).
///   * `provider` — the originating provider, useful for `pane show`
///     when callers want to attribute the event.
///   * `confidence` — the per-fact confidence on the legacy `Hook` /
///     `Plugin` / `Fallback` / `Inferred` ladder used by the
///     supervisor-side merge policy.
///   * `stale` — `true` once the fact's `stale_after_ms` window has
///     elapsed; rendered with a discreet hint instead of dropping the
///     fact entirely.
///   * `source` — productized "activity source" rendered in `pane show`
///     next to the AI activity overlay. When the row is for an AI tool
///     but no hook/plugin event landed, the overlay still renders so
///     the user sees `Workhive supervisor` / `tmux fallback` instead
///     of silently degrading to the supervisor row's confidence
///     column. `None` means the row is for a non-AI process (shell,
///     TUI) and the overlay is suppressed entirely.
#[derive(Debug, Default, Clone)]
pub(crate) struct AiActivityOverlayEntry {
    pub(crate) label: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) provider: Option<AiActivityProvider>,
    pub(crate) confidence: Option<AiActivityConfidence>,
    pub(crate) stale: bool,
    pub(crate) source: Option<AiActivitySource>,
    /// Typed AI activity state behind `label`. Carried so renderers
    /// reasoning in the typed pane state-model vocabulary can collapse
    /// the AI overlay to a concise activity / lifecycle decision
    /// without parsing the localized string back into an enum.
    /// `None` when the row has no AI overlay (non-AI pane, gated-off
    /// provider, or no resolvable foreground).
    pub(crate) state: Option<AiActivityState>,
}

/// Stale window for ACP read model rebuild (milliseconds).
///
/// Matches the projector skeleton's default. Pinned as a `const`
/// here so the value is one place to grep when the cross-team
/// freshness contract changes; the projector's own tests already lock
/// this number in `confidence::tests`.
const ACP_STALE_AFTER_MS: i64 = 30_000;

/// Expired window for ACP read model rebuild (milliseconds).
///
/// Matches the projector skeleton's default.
const ACP_EXPIRED_AFTER_MS: i64 = 90_000;

/// Resolve the AI integrations feature gate for `pane list` / `pane show`.
///
/// Loads global and project configs and runs them through the same
/// project-over-global precedence the rest of the CLI uses. The return
/// type is a typed [`FeatureGateResolution`]:
///
/// * [`FeatureGateResolution::Resolved`] — config was read successfully
///   (or both files are genuinely absent and built-in defaults apply).
/// * [`FeatureGateResolution::Unavailable`] — we could not trust either
///   the global or the project config. Pane render paths interpret this
///   as **fail-closed**: no hook/plugin overlay, no `/hooks` review
///   hint. Ordinary supervisor/process/tmux information continues to
///   render because it does not depend on the AI integration gate.
///
/// This is the same gate semantics used by spawn-time prepare; the only
/// difference is that supervisor write paths must skip prepare on
/// `Unavailable`, while display paths simply suppress AI integration
/// guidance.
pub(crate) fn resolve_ai_integration_gate(
    ctx: &CommandContext,
    project_root: &Utf8Path,
) -> FeatureGateResolution {
    let global = match ctx.config.load_global() {
        Ok(opt) => opt,
        Err(err) => {
            let reason = format!("failed to load global config: {err}");
            tracing::debug!(
                error = %err,
                "pane render: AI integrations feature gate unavailable (global config load error); suppressing hook/plugin guidance"
            );
            return FeatureGateResolution::Unavailable { reason };
        }
    };
    let project = match ctx.config.load_project(project_root) {
        Ok(opt) => opt,
        Err(err) => {
            let reason = format!("failed to load project config: {err}");
            tracing::debug!(
                error = %err,
                "pane render: AI integrations feature gate unavailable (project config load error); suppressing hook/plugin guidance"
            );
            return FeatureGateResolution::Unavailable { reason };
        }
    };
    let merged = match (global, project) {
        (Some(g), Some(p)) => {
            Some(workhive_core::usecases::config_mgmt::resolve_effective_config(&g, &p))
        }
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };
    FeatureGateResolution::Resolved(FeatureGate::from_config(merged.as_ref()))
}

/// Resolve the pane-observation feature gate used to drive the
/// terminal-status probe.
///
/// Like [`resolve_ai_integration_gate`], this consults global and
/// project configs and falls back to a closed gate when config cannot
/// be trusted, so a broken config file never escalates into a runtime
/// probe error.
pub(crate) fn resolve_pane_observation_gate(
    ctx: &CommandContext,
    project_root: &Utf8Path,
) -> PaneObservationGate {
    let global = match ctx.config.load_global() {
        Ok(opt) => opt,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "pane render: pane observation gate unavailable; disabling terminal-status"
            );
            return PaneObservationGate::from_switches(false);
        }
    };
    let project = match ctx.config.load_project(project_root) {
        Ok(opt) => opt,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "pane render: pane observation gate unavailable; disabling terminal-status"
            );
            return PaneObservationGate::from_switches(false);
        }
    };
    let merged = match (global, project) {
        (Some(g), Some(p)) => {
            Some(workhive_core::usecases::config_mgmt::resolve_effective_config(&g, &p))
        }
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };
    PaneObservationGate::from_config(merged.as_ref())
}

/// Read AI activity facts for each row and project them through the
/// merge policy.
///
/// The lookup key is `(group=server, workspace=workspace_session,
/// unit_id=row.target_id)` — exactly the tuple the pane supervisor
/// injects into hook scripts via `WORKHIVE_GROUP`,
/// `WORKHIVE_WORKSPACE`, `WORKHIVE_UNIT_ID`. Missing facts and store
/// I/O errors are tolerated and produce empty overlays so a broken
/// activity store can never block `pane list`.
///
/// `catalog` is the layered tool catalog already loaded by the
/// caller. The per-row helper consults
/// [`AiActivityProvider::resolve_for_pane`] so architecture-suffixed
/// Codex binaries modeled in `ToolCatalog::builtin_defaults` (e.g.
/// `codex-x86_64-apple-darwin`) keep their provider attribution even
/// when no hook fact is present.
#[allow(clippy::too_many_arguments)]
pub(crate) fn read_pane_ai_activity_overlay(
    composition: &CliComposition,
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
    rows: &[PaneListRow],
    locale: Locale,
    i18n: &dyn I18nPort,
    catalog: &ToolCatalog,
    gate: &FeatureGate,
    terminal_status: &[Option<TerminalStatusObservation>],
    now_ms: i64,
) -> Vec<AiActivityOverlayEntry> {
    let runtime_root = project_root.join(".workhive").join("runtime");
    let legacy_store = FsPaneActivityStore::new(runtime_root.clone());

    // ACP read path: when the AI integrations gate is globally
    // enabled, rebuild the projected read model from the append-only
    // event log and use it to drive overlays for rows whose latest
    // fact is `Fresh` + `High` confidence (or the Codex
    // notify-completion exception). Per-row provider gates and any
    // state the bridge cannot map cleanly fall through to the legacy
    // latest-fact overlay so the visible behavior is strictly
    // additive.
    //
    // Failure handling: any `ActivityEventStoreError` (Io,
    // Unavailable, CorruptEntry) is logged at `debug` and the
    // function falls back to the legacy overlay for every row. We
    // never panic and never let an empty / broken event log silence
    // the legacy overlay.
    let acp_overlays = build_acp_overlay_map(
        composition,
        gate,
        project_root,
        now_ms,
        server,
        workspace,
        rows,
        locale,
        i18n,
    );

    rows.iter()
        .enumerate()
        .map(|(idx, row)| {
            if let Some(map) = acp_overlays.as_ref() {
                let key = PaneCorrelationKey::new(
                    server.to_owned(),
                    workspace.to_owned(),
                    row.target_id.as_str(),
                );
                if let Some(entry) = map.get(&key) {
                    return entry.clone();
                }
            }
            let key = PaneActivityKey {
                group: server.to_owned(),
                workspace: workspace.to_owned(),
                unit_id: row.target_id.as_str().to_owned(),
            };
            // Tolerate any kind of store failure — fail-open per
            // the error-handling policy for an auxiliary observability surface.
            // Errors are logged at `debug` (no user-facing leak) so we
            // do not match the `unwrap_or_default` anti-pattern called
            // out in the error-handling policy (the failure is observed, not
            // silenced).
            let fact = match legacy_store.read(&key) {
                Ok(opt) => opt,
                Err(err) => {
                    tracing::debug!(
                        unit_id = %row.target_id.as_str(),
                        error = %err,
                        "pane activity store read failed; falling back to no fact"
                    );
                    None
                }
            };
            compute_pane_ai_overlay_entry(
                row,
                fact.as_ref(),
                terminal_status.get(idx).and_then(|s| s.as_ref()),
                now_ms,
                locale,
                i18n,
                catalog,
                gate,
            )
        })
        .collect()
}

/// Build the per-pane ACP overlay map for `rows`.
///
/// Returns `None` when the ACP read path must be skipped entirely:
///
/// - the AI integrations gate is **globally disabled**;
/// - the event store rebuild failed (Io / Unavailable / CorruptEntry —
///   logged at `debug` and treated as "no ACP data today").
///
/// On `Some(map)`, each entry is a row whose latest projected fact
/// passed [`project_pane_fact_to_overlay`] (Fresh + orchestration-
/// eligible state + per-provider-enabled gate). Rows not in the map
/// fall through to the legacy latest-fact overlay.
///
/// This helper is pure given the gate, composition, project root, and
/// i18n port: it routes event-store construction through
/// [`CliComposition::activity_event_store`] so command modules never
/// call `FsActivityEventStore::new(...)` directly and never re-derive
/// the `<project>/.workhive/runtime` layout. It owns the projector
/// call but does not consult the legacy latest-fact store and does not
/// touch the terminal-status probe.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_acp_overlay_map(
    composition: &CliComposition,
    gate: &FeatureGate,
    project_root: &Utf8Path,
    now_ms: i64,
    server: &str,
    workspace: &str,
    rows: &[PaneListRow],
    locale: Locale,
    i18n: &dyn I18nPort,
) -> Option<std::collections::HashMap<PaneCorrelationKey, AiActivityOverlayEntry>> {
    if !gate.globally_enabled() {
        return None;
    }
    let store = composition.activity_event_store(project_root);
    build_acp_overlay_map_from_store(gate, &store, now_ms, server, workspace, rows, locale, i18n)
}

/// Test-injectable variant of [`build_acp_overlay_map`] that accepts
/// any [`ActivityEventStore`] implementation.
///
/// The production caller always reaches this through
/// [`build_acp_overlay_map`], which obtains its
/// [`ActivityEventStore`] from
/// [`CliComposition::activity_event_store`]. Tests drive this helper
/// directly with an `InMemoryActivityEventStore` from
/// `workhive-testkit` so the error-injection path can be exercised
/// without disk I/O.
///
/// Returns `None` if the projector rebuild fails (logged at `debug`).
/// Returns `Some(map)` (possibly empty) otherwise. The caller treats
/// `None` exactly the same as "no ACP overlays today" and falls back
/// to the legacy latest-fact path for every row.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_acp_overlay_map_from_store(
    gate: &FeatureGate,
    store: &dyn ActivityEventStore,
    now_ms: i64,
    server: &str,
    workspace: &str,
    rows: &[PaneListRow],
    locale: Locale,
    i18n: &dyn I18nPort,
) -> Option<std::collections::HashMap<PaneCorrelationKey, AiActivityOverlayEntry>> {
    if !gate.globally_enabled() {
        return None;
    }
    let projector = StateProjector::new(store, now_ms, ACP_STALE_AFTER_MS, ACP_EXPIRED_AFTER_MS);
    let read_model = match projector.rebuild() {
        Ok(rm) => rm,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "ACP read model rebuild failed; falling back to legacy latest-fact overlay"
            );
            return None;
        }
    };
    Some(build_acp_overlay_map_from_read_model(
        &read_model,
        rows,
        server,
        workspace,
        locale,
        i18n,
        gate,
    ))
}

/// Pure projection of a [`ReadModel`] onto per-row
/// [`AiActivityOverlayEntry`] values.
///
/// Extracted from [`build_acp_overlay_map`] so the projection logic
/// can be unit-tested without disk I/O. Rows whose projected fact
/// does not satisfy [`project_pane_fact_to_overlay`], or whose
/// provider is currently disabled by the per-provider gate, are
/// simply absent from the returned map and fall through to the
/// legacy overlay.
pub(crate) fn build_acp_overlay_map_from_read_model(
    read_model: &ReadModel,
    rows: &[PaneListRow],
    server: &str,
    workspace: &str,
    locale: Locale,
    i18n: &dyn I18nPort,
    gate: &FeatureGate,
) -> std::collections::HashMap<PaneCorrelationKey, AiActivityOverlayEntry> {
    use std::collections::HashMap;
    let mut out: HashMap<PaneCorrelationKey, AiActivityOverlayEntry> = HashMap::new();
    for row in rows {
        let key = PaneCorrelationKey::new(
            server.to_owned(),
            workspace.to_owned(),
            row.target_id.as_str(),
        );
        let Some(row_model) = read_model.get(&key) else {
            continue;
        };
        let Some(projection) = project_pane_fact_to_overlay(&row_model.fact) else {
            continue;
        };
        // Per-provider gate: a globally-on but provider-disabled row
        // must render silently. The legacy renderer enforces the same
        // rule via [`compute_pane_ai_overlay_entry`], but here we drop
        // the row from the ACP map so the legacy path handles it
        // consistently.
        if !gate.is_provider_enabled(projection.provider) {
            continue;
        }
        out.insert(key, render_acp_overlay_entry(projection, locale, i18n));
    }
    out
}

/// Localize an ACP overlay projection into the legacy
/// [`AiActivityOverlayEntry`] the renderer already speaks.
///
/// Pure: the only input is the projection + the i18n port. We
/// deliberately leave `reason` empty because the ACP fact carries no
/// localized reason today; the legacy "reason" line shows the tool
/// name from the pane row, which is computed downstream by the
/// renderer regardless of the overlay source.
pub(crate) fn render_acp_overlay_entry(
    projection: PaneActivityOverlayProjection,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> AiActivityOverlayEntry {
    AiActivityOverlayEntry {
        label: Some(i18n.resolve(locale, projection.state_label_key)),
        reason: None,
        provider: Some(projection.provider),
        confidence: Some(projection.confidence),
        stale: projection.stale,
        source: Some(projection.source),
        state: Some(projection.state),
    }
}

/// Drive the terminal-status probe for every AI row whose provider is
/// enabled by both the AI integrations gate and the pane observation
/// gate, returning one entry per row (aligned by index with `rows`).
///
/// Probe failures (mismatched result count, unavailable backend, etc.)
/// degrade silently to `None` for every row so the AI overlay never
/// blocks pane rendering on a flaky external probe.
pub(crate) fn read_terminal_status_observations(
    rows: &[PaneListRow],
    server: &str,
    catalog: &ToolCatalog,
    ai_gate: &FeatureGate,
    pane_gate: &PaneObservationGate,
    probe: &dyn TerminalStatusProbePort,
) -> Vec<Option<TerminalStatusObservation>> {
    if !pane_gate.terminal_status_enabled() || !ai_gate.globally_enabled() {
        return vec![None; rows.len()];
    }

    let mut request_rows: Vec<(usize, AiActivityProvider)> = Vec::new();
    let mut requests: Vec<TerminalStatusProbeRequest> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let provider = AiActivityProvider::resolve_for_pane(
            AiActivityProvider::None,
            &row.current_command,
            catalog,
        );
        if matches!(provider, AiActivityProvider::None) || !ai_gate.is_provider_enabled(provider) {
            continue;
        }
        request_rows.push((idx, provider));
        requests.push(TerminalStatusProbeRequest {
            group: server.to_owned(),
            target_unit: row.target_id.clone(),
        });
    }

    let mut out = vec![None; rows.len()];
    if requests.is_empty() {
        return out;
    }

    let captures = match probe.capture_many(&requests) {
        Ok(captures) if captures.len() == requests.len() => captures,
        Ok(captures) => {
            tracing::warn!(
                expected = requests.len(),
                got = captures.len(),
                "terminal-status probe returned mismatched result count; ignoring"
            );
            return out;
        }
        Err(error) => {
            tracing::debug!(%error, "terminal-status probe failed; falling through");
            return out;
        }
    };

    for ((row_idx, provider), capture) in request_rows.into_iter().zip(captures) {
        let Some(capture) = capture else { continue };
        out[row_idx] = interpret_terminal_status(provider, &capture.text);
    }
    out
}

/// Pure per-row projection of the activity overlay.
///
/// Extracted from [`read_pane_ai_activity_overlay`] so the
/// foreground-provider derivation can be unit-tested without hitting
/// the activity store. Architecture-suffixed Codex binaries (modeled
/// in [`ToolCatalog`]) keep their provider identity even when no hook
/// fact has landed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_pane_ai_overlay_entry(
    row: &PaneListRow,
    fact: Option<&AiActivityFact>,
    terminal_status: Option<&TerminalStatusObservation>,
    now_ms: i64,
    locale: Locale,
    i18n: &dyn I18nPort,
    catalog: &ToolCatalog,
    gate: &FeatureGate,
) -> AiActivityOverlayEntry {
    // Map the supervisor-observed `activity` to a
    // [`ForegroundAgentSignal`]. We must compute the signal BEFORE
    // consulting the fact so panes with no fact still get a typed
    // answer for `merge_displayed_activity`.
    let provider_hint = fact.map(|f| f.provider).unwrap_or(AiActivityProvider::None);
    // Feature gate short-circuits the hook/plugin overlay path
    // entirely when the row's provider (or its hint) is disabled —
    // no label, no source. The pane still renders via the supervisor
    // observation column; we just stop displaying the AI integration
    // story for this row. This is the "completely silent when
    // disabled" contract.
    let foreground_provider_for_gate =
        AiActivityProvider::resolve_for_pane(provider_hint, &row.current_command, catalog);
    if !gate.is_provider_enabled(foreground_provider_for_gate) {
        return AiActivityOverlayEntry::default();
    }
    let fg = match row.activity {
        PaneActivityState::AgentForeground | PaneActivityState::AgentBusy => {
            // Shared exact-match resolver: prefers the fact's
            // provider, then strict canonical basenames, then the
            // tool catalog's modeled process names (covers
            // architecture-suffixed Codex binaries without
            // hardcoding OS-specific names here).
            let foreground_provider =
                AiActivityProvider::resolve_for_pane(provider_hint, &row.current_command, catalog);
            if matches!(foreground_provider, AiActivityProvider::None) {
                ForegroundAgentSignal::Unknown
            } else {
                ForegroundAgentSignal::Matches(foreground_provider)
            }
        }
        // `ForegroundProcess` is the state the supervisor assigns
        // when a non-shell foreground command is running but the
        // supervisor has not yet had a chance to classify the pane
        // as an agent (e.g. `pane split --cmd opencode` before the
        // OpenCode plugin emits any event). If the live
        // `current_command` resolves to a known AI tool through the
        // typed catalog/canonical-basename ladder, treat the pane as
        // an `open (waiting for event)` agent so the fallback
        // source/confidence columns render truthfully
        // (`process`/`tmux` + `Fallback`) instead of collapsing to
        // `none`. Trust-boundary: this never sets a `Hook` / `Plugin`
        // confidence; it only widens the fallback window, exactly
        // matching the source ladder `plugin > terminal status >
        // supervisor > process > tmux > none`.
        PaneActivityState::ForegroundProcess => {
            let foreground_provider =
                AiActivityProvider::resolve_for_pane(provider_hint, &row.current_command, catalog);
            if matches!(foreground_provider, AiActivityProvider::None) {
                ForegroundAgentSignal::NotAnAgent
            } else {
                ForegroundAgentSignal::Matches(foreground_provider)
            }
        }
        PaneActivityState::ShellIdle | PaneActivityState::Exited => {
            ForegroundAgentSignal::NotAnAgent
        }
        _ => ForegroundAgentSignal::Unknown,
    };
    // Non-agent foreground: the overlay must stay empty so shell rows
    // never sprout an "AI" annotation.
    if matches!(fg, ForegroundAgentSignal::NotAnAgent) {
        return AiActivityOverlayEntry::default();
    }
    let resolved = resolve_ai_activity_display(fact, terminal_status, now_ms, fg, row.source);
    let displayed = resolved.displayed;
    // No fact AND foreground unknown collapses to `unknown_none()`;
    // for a shell-or-similar pane we suppress the overlay entirely.
    // Non-AI rows must render no AI overlay.
    if matches!(fg, ForegroundAgentSignal::Unknown) && fact.is_none() {
        return AiActivityOverlayEntry::default();
    }
    let label = i18n.resolve(locale, displayed.state.i18n_key());
    AiActivityOverlayEntry {
        label: Some(label),
        reason: displayed.reason.clone(),
        provider: Some(displayed.provider),
        confidence: Some(displayed.confidence),
        stale: displayed.stale,
        source: Some(resolved.source),
        state: Some(displayed.state),
    }
}

/// Current wall-clock time in Unix milliseconds from the supplied
/// [`ClockPort`].
///
/// All callers must pass a concrete clock (production: `SystemClock`;
/// tests: `FakeClock`) instead of calling `SystemTime::now()`
/// directly. This is the single call site in this module that converts
/// a `ClockPort` reference to a plain `i64`, making time injection
/// transparent throughout the rest of the render path.
pub(crate) fn now_unix_ms_from(clock: &dyn ClockPort) -> i64 {
    clock.now_unix_ms()
}

/// Project the per-pane AI integration lifecycle state given the
/// row's resolved provider and the rendered overlay entry, if any.
///
/// Pane scope is intentionally narrow: `provider_available` and
/// `hooks_installed` are always `true` here because by the time the
/// row exists the pane is alive and Workhive considers the integration
/// scaffold present at the project level. Whether the pane has ever
/// seen a hook/plugin event, and whether that event is stale, comes
/// from the overlay entry itself.
///
/// Without an overlay (e.g. supervisor-only rows), the function
/// returns the policy's "no event observed yet" state — typically
/// [`AiIntegrationLifecycleState::ReviewRequired`] for Codex. The
/// `Installed` variant is reserved for provider-scope observations
/// (e.g. provider-scope diagnostics, post-prepare verification, or
/// pre-spawn handshakes that are not tied to one pane) where there is
/// no pane to attach events to — see
/// [`AiIntegrationLifecycleState::Installed`] for the full contract.
pub(crate) fn resolve_pane_lifecycle_state(
    provider: AiActivityProvider,
    overlay: Option<&AiActivityOverlayEntry>,
) -> AiIntegrationLifecycleState {
    let has_accurate_event = overlay.is_some_and(|e| {
        matches!(
            e.source,
            Some(
                AiActivitySource::ClaudeCodeHook
                    | AiActivitySource::CodexHook
                    | AiActivitySource::OpencodePlugin
            )
        )
    });
    let last_event_is_stale = overlay.is_some_and(|e| e.stale);
    let policy = policy_for(provider);
    let observation = LifecycleObservation {
        provider,
        provider_available: true,
        hooks_installed: true,
        prepare_failed: false,
        has_accurate_event,
        last_event_is_stale,
        // Pane-scope projection: see the projection note above. This
        // is the one fact this function knows that the core resolver
        // cannot infer on its own.
        observes_pane_event: true,
    };
    resolve_lifecycle_state(observation, policy)
}

#[cfg(test)]
#[path = "pane_activity_overlay_tests.rs"]
mod tests;

//! `pane` use cases — split, close, list, show, send.
//!
//! Display-side responsibilities (`pane list`, `pane show`) are
//! implemented by the typed view helpers near the bottom of this
//! module. They merge live runtime observations with Workhive's
//! durable launch identity to produce a single typed structure that
//! the CLI renders without any classification logic of its own.
//!
//! ## Display contract
//!
//! - the live runtime observation (kind / state / current_command)
//!   is authoritative;
//! - Workhive's historical launch identity is rendered as a separate
//!   auxiliary field and must NEVER override the live values;
//! - all kind/state labels go through `PaneKind::i18n_key` /
//!   `PaneState::i18n_key` so locales render the right glyph.
//!
//! View helpers accept a single slice of [`PaneObservedViewInput`]
//! pairs so callers cannot accidentally feed unmatched live-runtime
//! observations and truth entries — that earlier `assert_eq!`-based
//! shape was a production panic risk per the error-handling policy.

use crate::domain::{
    PaneActivityState, PaneDirection, PaneKind, PaneLaunchOrigin, PaneLaunchRef, PaneMessageSafety,
    PaneObservationConfidence, PaneObservationSource, PaneRuntimeTruth, PaneState, UnitId,
};
use crate::ports::{
    PaneLaunchIdentityEntry, PaneLaunchIdentityPort, PaneRuntimePort, RuntimeError,
    RuntimeUnitSnapshot, SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest,
};

/// Split the current pane.
pub fn split_pane(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
    direction: PaneDirection,
    command: Option<String>,
) -> Result<RuntimeUnitSnapshot, RuntimeError> {
    runtime.spawn_unit(SpawnUnitRequest {
        group: server.to_owned(),
        workspace_name: session.to_owned(),
        direction,
        command,
        target_unit: None,
        size_percent: None,
        env: Vec::new(),
        cwd: None,
    })
}

/// Record durable launch identity and reassert the pane title for a
/// pane that was just produced by `pane split --cmd ...`.
///
/// Without these writes the durable identity store would stay empty
/// until the next live-runtime refresh, so `pane list` / `pane show`
/// would render the live foreground command (often `zsh` while a tool
/// boots) instead of the user-recognizable label like `codex`. This
/// helper composes the two writes — durable identity record and
/// runtime pane title — that are required to make the split pane
/// identifiable immediately.
///
/// Both writes are **fail-open**: if either backend fails (a corrupt
/// store, a tmux race, etc.) the helper logs at `warn` and returns
/// `Ok(())`. The split itself already succeeded; refusing to surface
/// the new pane just because metadata failed would be hostile.
///
/// `EmptyShell` launches deliberately skip both writes: there is no
/// useful title for a bare shell (rendering the literal `shell`
/// next to a live `zsh` command would only repeat what the user
/// already sees) and the durable store treats `EmptyShell` entries
/// as no-ops on the read side.
pub fn record_split_launch_metadata(
    runtime: &dyn PaneRuntimePort,
    identity_store: &dyn PaneLaunchIdentityPort,
    server: &str,
    workspace: &str,
    unit_id: &UnitId,
    launch: &PaneLaunchRef,
) {
    if matches!(launch, PaneLaunchRef::EmptyShell) {
        tracing::debug!(
            unit_id = %unit_id,
            "split: empty shell launch — skipping durable identity / title writes"
        );
        return;
    }

    let entry = PaneLaunchIdentityEntry {
        workspace: workspace.to_owned(),
        unit_id: unit_id.clone(),
        launch_origin: launch.clone(),
    };
    if let Err(error) = identity_store.record(server, entry) {
        tracing::warn!(
            %error,
            unit_id = %unit_id,
            "split: failed to record durable launch identity; \
             pane list will fall back to live foreground command"
        );
    }

    let title = launch.display_name().to_owned();
    if title.is_empty() {
        return;
    }
    if let Err(error) = runtime.set_unit_title(SetUnitTitleRequest {
        group: server.to_owned(),
        target_unit: unit_id.clone(),
        title,
    }) {
        tracing::warn!(
            %error,
            unit_id = %unit_id,
            "split: failed to set pane title; \
             pane border will keep tmux's default title"
        );
    }
}

/// Close a specific unit.
pub fn close_pane(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    unit_id: &UnitId,
) -> Result<(), RuntimeError> {
    runtime.close_unit(server, unit_id)
}

/// List all units in a session.
pub fn list_panes(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
    runtime.list_units(server, session)
}

/// Show the current unit.
pub fn current_pane(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    session: &str,
) -> Result<RuntimeUnitSnapshot, RuntimeError> {
    runtime.current_unit(server, session)
}

/// Send text to a target unit.
pub fn send_keys(
    runtime: &dyn PaneRuntimePort,
    server: &str,
    unit_id: &UnitId,
    text: &str,
    enter: bool,
) -> Result<(), RuntimeError> {
    runtime.send_input(SendInputRequest {
        group: server.to_owned(),
        target_unit: unit_id.clone(),
        text: text.to_owned(),
        enter,
    })
}

// ── Pane list/show view layer ────────────────────────────────────────
//
// The CLI must:
//   * surface the Workhive-owned launch identity (the title, e.g.
//     `codex` or `claude`) as a SEPARATE auxiliary column, not as the
//     primary command/type/state — the live merged observation always
//     wins;
//   * still report the live `RuntimeUnitSnapshot` foreground command
//     and observed state so users can see what tmux currently sees;
//   * never hardcode strings like `"Unknown"` or `"Running"` — every
//     enum is funneled through `PaneKind::i18n_key` /
//     `PaneState::i18n_key` so locales render the right glyph.
//
// All merging logic lives here so `pane list` and `pane show` share a
// single source of truth.

/// One row of a `workhive pane list` output, fully typed and ready to
/// render.
///
/// Contract: launch identity is
/// strictly auxiliary — it MUST NOT override the live runtime
/// observation's `kind`, `state`, or current command. The view model
/// therefore exposes the live observation as the primary fields, and
/// surfaces Workhive's historical launch identity as a separate
/// optional field (`launch_identity_title`) that the CLI renders as a
/// distinct column / label.
///
/// All localizable enums are kept as `PaneKind` / `PaneState` so the
/// caller decides the language; raw strings (e.g. `current_command`)
/// are already display-ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneListRow {
    /// Whether this is the currently focused pane.
    pub active: bool,
    /// Workspace-local index assigned by tmux (e.g. `0`, `1`, `2`).
    pub index: u32,
    /// Backend-specific target id usable by `workhive pane send`
    /// (e.g. tmux `%2`).
    pub target_id: UnitId,
    /// Live foreground command currently observed by the runtime
    /// (e.g. `zsh`, `codex`, `lazygit`). This is the user-facing
    /// "current command" / observed title — never rewritten by
    /// historical launch identity.
    pub current_command: String,
    /// Best-effort runtime classification taken directly from the live
    /// runtime observation. Never rewritten by launch identity.
    pub kind: PaneKind,
    /// Best-effort observed runtime state taken directly from the live
    /// runtime observation. Never fabricated as `Running` based on
    /// historical identity.
    ///
    /// **Caveat — do NOT render `state` as the user-facing activity
    /// column.** `state` only tells you "the foreground process
    /// is/isn't a recognised shell"; it does NOT tell you whether
    /// an AI agent is busy, whether the pane is safe to message, or
    /// whether the pane is "available". For the user-facing activity
    /// column use [`PaneListRow::activity`]; for routing decisions
    /// use [`PaneListRow::message_safety`].
    pub state: PaneState,
    /// Typed user-facing activity classification computed from the
    /// merged observation `(kind, state, source, confidence)`. CLI
    /// rendering MUST consume this for the visible "状态/Activity"
    /// column instead of `state` so a foreground `codex` is not
    /// silently rendered as "running" / "运行中" while the agent
    /// might actually be idle waiting for input.
    pub activity: PaneActivityState,
    /// Typed pane-message safety classification used by the CLI
    /// (`pane show`) and by future inter-pane scheduling. Only
    /// `SafeStructured` permits structured-message routing.
    pub message_safety: PaneMessageSafety,
    /// Workhive-owned launch identity carried forward from the
    /// durable identity store, when present and non-empty. Display
    /// is strictly auxiliary: e.g. when the live foreground command is
    /// `zsh` but Workhive originally launched the pane as `codex`,
    /// CLI surfaces this as a separate "launch identity" column
    /// without changing `kind` / `state` / `current_command`.
    pub launch_identity_title: Option<String>,
    /// Which layered observer produced `kind` / `state` /
    /// `current_command`. Carried for verbose CLI output and
    /// diagnostic logging only — never used to gate behavior.
    pub source: PaneObservationSource,
    /// Confidence of the observer that produced this row.
    pub confidence: PaneObservationConfidence,
    /// Working directory currently observed by the runtime.
    pub cwd: String,
}

/// A single typed pane view used by `workhive pane show`.
///
/// `PaneShowView` is just a one-row variant of [`PaneListRow`] but is
/// kept as a distinct type so future extensions do not retroactively widen
/// the list row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneShowView {
    pub row: PaneListRow,
}

/// Pair of inputs for a single pane view row.
///
/// Carries the live runtime snapshot for the pane and the merged
/// runtime truth produced by
/// [`crate::usecases::pane_observation::merge_pane_observations`] /
/// [`crate::usecases::pane_observation::gather_pane_truth`].
///
/// This structure exists so view builders cannot be miscalled with
/// two parallel slices of unequal length: the snapshot and its truth
/// always travel together. Per the error-handling policy user-facing CLI paths
/// MUST NOT panic on caller mistakes; pairing the two inputs makes
/// the bug impossible at the type level.
#[derive(Debug, Clone)]
pub struct PaneObservedViewInput {
    pub snapshot: RuntimeUnitSnapshot,
    pub truth: PaneRuntimeTruth,
}

impl PaneObservedViewInput {
    /// Construct a view input from a live snapshot and its merged
    /// runtime truth.
    pub fn new(snapshot: RuntimeUnitSnapshot, truth: PaneRuntimeTruth) -> Self {
        Self { snapshot, truth }
    }
}

impl PaneKind {
    /// i18n key for the user-facing label of this kind.
    ///
    /// Defined here (next to the domain type) so callers cannot drift
    /// into hardcoded `"unknown"` / `"agent"` strings that bypass the
    /// localized catalog.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Agent => crate::i18n_keys::PANE_KIND_AGENT,
            Self::Terminal => crate::i18n_keys::PANE_KIND_TERMINAL,
            Self::GitUi => crate::i18n_keys::PANE_KIND_GIT_UI,
            Self::Unknown => crate::i18n_keys::PANE_KIND_UNKNOWN,
        }
    }
}

impl PaneState {
    /// i18n key for the user-facing label of this state.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Running => crate::i18n_keys::PANE_STATE_RUNNING,
            Self::Idle => crate::i18n_keys::PANE_STATE_IDLE,
            Self::Unknown => crate::i18n_keys::PANE_STATE_UNKNOWN,
        }
    }
}

/// Build a typed `pane list` view from one live runtime observation
/// per pane and Workhive's durable launch-identity entries.
///
/// Each runtime observation is wrapped in a [`PaneObservedViewInput`] whose
/// `truth` is synthesized as fallback-confidence tmux data. Use
/// [`build_pane_list_view_from_inputs`] when richer observations have
/// already been gathered from the supervisor / OS observer / tmux
/// stack.
///
/// View contract:
///
///   * `kind` and `state` are taken directly from the live
///     `RuntimeUnitSnapshot` and MUST NOT be rewritten based on
///     historical launch identity. If the runtime currently observes
///     a shell, the row reports a terminal pane in idle state — even
///     when Workhive launched the pane as `codex`.
///   * `current_command` is the live foreground command observed by
///     the runtime adapter, exactly as reported.
///   * Workhive's historical launch identity, when present and not
///     `EmptyShell`, is exposed as `launch_identity_title` so CLIs can
///     surface it as a distinct "launch identity" column / label
///     without polluting the live observation.
pub fn build_pane_list_view(
    live: &[RuntimeUnitSnapshot],
    identity: &[PaneLaunchIdentityEntry],
) -> Vec<PaneListRow> {
    let inputs: Vec<PaneObservedViewInput> = live
        .iter()
        .map(|snap| PaneObservedViewInput::new(snap.clone(), snapshot_as_tmux_truth(snap)))
        .collect();
    build_pane_list_view_from_inputs(&inputs, identity)
}

/// Build a `pane list` view from pre-paired observation inputs.
///
/// Each [`PaneObservedViewInput`] couples a live runtime snapshot
/// with the merged runtime truth that the supervisor / OS observer /
/// tmux fallback stack produced. The merged truth's
/// `kind` / `state` / `current_command` win over the runtime observation's
/// tmux-classified values; the observation is consulted only for fields
/// that are not part of the observation model (target id, index,
/// cwd, focused flag).
///
/// Pairing inputs at the type level makes a previous "two parallel
/// slices of unequal length" bug impossible, so user-visible CLI
/// commands cannot panic on internal caller mistakes (the error-handling policy).
pub fn build_pane_list_view_from_inputs(
    inputs: &[PaneObservedViewInput],
    identity: &[PaneLaunchIdentityEntry],
) -> Vec<PaneListRow> {
    let identity_map = build_identity_map(identity);

    inputs
        .iter()
        .map(|input| {
            let snapshot = &input.snapshot;
            let truth = &input.truth;

            let key = snapshot.id.as_str();
            let entry = identity_map.get(key).copied();

            let launch_identity_title = entry.and_then(|e| launch_identity_title(&e.launch_origin));

            if entry.is_none() {
                tracing::debug!(
                    unit_id = %snapshot.id,
                    "pane_view: no durable launch identity for unit; \
                     surfacing live observation only"
                );
            }

            tracing::debug!(
                unit_id = %snapshot.id,
                source = ?truth.source,
                confidence = ?truth.confidence,
                command = %truth.current_command,
                kind = ?truth.kind,
                state = ?truth.state,
                "pane_view: rendering merged observation"
            );

            PaneListRow {
                active: snapshot.active,
                index: snapshot.index,
                target_id: snapshot.id.clone(),
                // The merged observation is authoritative: launch
                // identity must NEVER override these and the
                // runtime observation's tmux-classified values are only one
                // the merge inputs, never the final truth.
                current_command: truth.current_command.clone(),
                kind: truth.kind,
                state: truth.state,
                activity: truth.activity,
                message_safety: truth.message_safety,
                source: truth.source,
                confidence: truth.confidence,
                launch_identity_title,
                cwd: snapshot.cwd.to_string(),
            }
        })
        .collect()
}

/// Build a `pane show` view from a single live runtime observation
/// and identity entries. Mirrors [`build_pane_list_view`] using fallback
/// tmux truth when richer observations are unavailable.
pub fn build_pane_show_view(
    live: &RuntimeUnitSnapshot,
    identity: &[PaneLaunchIdentityEntry],
) -> PaneShowView {
    let input = PaneObservedViewInput::new(live.clone(), snapshot_as_tmux_truth(live));
    build_pane_show_view_from_input(&input, identity)
}

/// Build a `pane show` view from a pre-paired observation input.
///
/// Mirrors [`build_pane_list_view_from_inputs`] for a single pane so
/// `pane list` and `pane show` cannot drift from each other.
pub fn build_pane_show_view_from_input(
    input: &PaneObservedViewInput,
    identity: &[PaneLaunchIdentityEntry],
) -> PaneShowView {
    let rows = build_pane_list_view_from_inputs(std::slice::from_ref(input), identity);
    let row = rows.into_iter().next().unwrap_or_else(|| {
        // build_pane_list_view_from_inputs always yields one row per
        // input. The fallback exists so the function is panic-free
        // even if its body changes; surface a typed Unknown row
        // instead of falling over the user-facing command.
        let snapshot = &input.snapshot;
        PaneListRow {
            active: snapshot.active,
            index: snapshot.index,
            target_id: snapshot.id.clone(),
            current_command: input.truth.current_command.clone(),
            kind: input.truth.kind,
            state: input.truth.state,
            activity: input.truth.activity,
            message_safety: input.truth.message_safety,
            source: input.truth.source,
            confidence: input.truth.confidence,
            launch_identity_title: None,
            cwd: snapshot.cwd.to_string(),
        }
    });
    PaneShowView { row }
}

/// Synthesize fallback runtime truth from a live runtime observation.
/// The observation already carries the tmux adapter's classification of
/// `kind` / `state` / `command`; surface it with fallback confidence so view
/// rows retain coherent source and confidence labels.
fn snapshot_as_tmux_truth(snapshot: &RuntimeUnitSnapshot) -> PaneRuntimeTruth {
    PaneRuntimeTruth::from_layered(
        snapshot.kind,
        snapshot.state,
        snapshot.command.clone(),
        PaneObservationSource::TmuxCurrentCommand,
        PaneObservationConfidence::Fallback,
    )
}

fn build_identity_map(
    identity: &[PaneLaunchIdentityEntry],
) -> std::collections::HashMap<&str, &PaneLaunchIdentityEntry> {
    identity
        .iter()
        .map(|entry| (entry.unit_id.as_str(), entry))
        .collect()
}

/// Render the user-recognizable selection label for a pane row.
///
/// The default `pane list` output should help the user pick a pane,
/// not surface diagnostic columns. The label prefers Workhive's
/// durable launch identity (e.g. `codex`, `claude`) when present and
/// otherwise falls back to the live foreground command — but never
/// surfaces the internal `workhive` wrapper that the supervisor / CLI
/// uses behind the scenes.
///
/// Returns `None` when no user-recognizable label is available; the
/// CLI renders that as a localized placeholder (`pane.list.label.none`).
pub fn pane_selection_label(row: &PaneListRow) -> Option<String> {
    if let Some(identity) = row.launch_identity_title.as_deref() {
        let trimmed = identity.trim();
        if !trimmed.is_empty() && !is_internal_workhive_command(trimmed) {
            return Some(sanitize_label_for_display(trimmed));
        }
    }
    let cmd = row.current_command.trim();
    if cmd.is_empty() || is_internal_workhive_command(cmd) {
        None
    } else {
        Some(sanitize_label_for_display(cmd))
    }
}

/// tmux exposes the foreground command's `argv[0]` verbatim in
/// `pane_current_command`, which for login shells starts with a
/// hyphen (`-zsh`, `-bash`, ...). The leading dash is the POSIX
/// convention for "this shell was started as a login shell"; it is
/// not a typo. Surfacing it as a label looks like a formatting bug
/// to most users, so the simplified `pane list` label strips the
/// dash when the rest is a recognized shell name. The full,
/// truthful argv stays available through `pane show`'s
/// `current_command` field.
fn sanitize_label_for_display(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix('-') {
        let head = rest.split_whitespace().next().unwrap_or("");
        let basename = head.rsplit('/').next().unwrap_or(head);
        if matches!(
            basename,
            "zsh" | "bash" | "sh" | "fish" | "nushell" | "nu" | "dash" | "ksh" | "csh" | "tcsh"
        ) {
            return rest.to_owned();
        }
    }
    trimmed.to_owned()
}

/// Returns `true` when `value` looks like an internal Workhive
/// supervisor / CLI command rather than a user-launched tool. Used to
/// suppress the noise from default `pane list` output.
fn is_internal_workhive_command(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    let head = trimmed.split_whitespace().next().unwrap_or("");
    let basename = head.rsplit('/').next().unwrap_or(head);
    matches!(basename, "workhive" | "wh")
}

fn launch_identity_title(origin: &PaneLaunchOrigin) -> Option<String> {
    match origin {
        PaneLaunchOrigin::EmptyShell => None,
        PaneLaunchOrigin::Tool { name } => Some(name.as_str().to_owned()),
        PaneLaunchOrigin::InlineCommand { spec } => {
            let program = spec.program();
            if program.is_empty() {
                None
            } else {
                Some(program.to_owned())
            }
        }
    }
}

#[cfg(test)]
#[path = "pane_tests.rs"]
mod tests;

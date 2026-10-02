//! Pane observation merge use case — single source of truth for
//! combining supervisor / OS observer / tmux observations into one
//! [`PaneRuntimeTruth`].
//!
//! ## Architecture
//!
//! Adapters in `workhive-infra` produce one [`PaneObservation`] each:
//!
//! 1. supervisor adapter (`PaneSupervisorPort`) → `Authoritative`
//! 2. OS process observer (`ProcessObserverPort`) → `High`
//! 3. tmux fallback                              → `Fallback`
//! 4. anything else → `None` (unknown)
//!
//! This use case picks the highest-confidence observation that has
//! signal, classifies its foreground basename via
//! [`ToolCatalog::match_process_name`], and produces the final
//! [`PaneRuntimeTruth`] consumed by `pane list` / `pane show`.
//!
//! ## Hard rules
//!
//! * `kind` and `state` are derived **only** from the chosen
//!   observation. Historical launch identity (e.g. "Workhive launched
//!   this pane as `codex`") is rendered separately in the CLI as an
//!   auxiliary field and is not visible to this merge.
//! * If no observation has signal, return [`PaneRuntimeTruth::unknown`]
//!   — never invent a state.
//! * Tool recognition uses the configurable
//!   [`ToolEntry::process_names`] list. The fallback (matching against
//!   `command.program` basename) is exact only — no substring matches.

use crate::domain::{
    PaneKind, PaneObservation, PaneObservationConfidence, PaneObservationSource, PaneRuntimeTruth,
    PaneState, ToolCatalog, UnitId, derive_activity, derive_message_safety,
};
use crate::ports::{
    PaneProcessLookup, PaneSupervisorPort, ProcessObserverPort, RuntimeUnitSnapshot,
};

/// Merge a stack of pane observations into one final runtime truth.
///
/// The caller (typically `workhive-cli`) collects observations from
/// every available adapter and feeds them to this function. The
/// adapters are responsible for downgrading or omitting their
/// observation when the underlying signal is stale (e.g. supervisor
/// metadata exists but the recorded pid is no longer alive).
///
/// Selection rule: highest [`PaneObservationConfidence`] wins among
/// observations that have signal. Two observations of equal
/// confidence are not expected in practice (each source produces at
/// most one); ties resolve to the first seen so call sites can pin
/// down ordering when needed.
pub fn merge_pane_observations(
    observations: &[PaneObservation],
    tools: &ToolCatalog,
) -> PaneRuntimeTruth {
    let Some(chosen) = pick_observation(observations) else {
        tracing::debug!(
            observation_count = observations.len(),
            "merge_pane_observations: no observation has signal; returning unknown"
        );
        return PaneRuntimeTruth::unknown();
    };

    let foreground_raw = chosen
        .process
        .as_ref()
        .and_then(|p| p.foreground.as_ref())
        .map(|c| c.basename.clone())
        .unwrap_or_default();
    let command_basename = basename_of(&foreground_raw).to_owned();

    let mut kind = classify_kind(&command_basename, tools);
    let state = chosen
        .state_hint
        .unwrap_or_else(|| infer_state(&command_basename, kind));

    // Empty-shell intent: when a higher-confidence source (typically the
    // supervisor) authoritatively asserts the pane is idle but provides
    // no foreground command, that is by construction a shell prompt with
    // no tool running. Render it as Terminal/Idle rather than Unknown so
    // intentional empty shells stop looking like broken/unknown panes.
    // We do not promote kind for state hints that are not Idle (Running
    // without foreground is genuinely ambiguous).
    if matches!(kind, PaneKind::Unknown)
        && command_basename.is_empty()
        && matches!(state, PaneState::Idle)
    {
        kind = PaneKind::Terminal;
    }

    let activity = derive_activity(kind, state, chosen.source, chosen.confidence);
    let message_safety = derive_message_safety(kind, state, chosen.source, chosen.confidence);

    tracing::debug!(
        observation_source = ?chosen.source,
        observation_confidence = ?chosen.confidence,
        command_basename = %command_basename,
        kind = ?kind,
        state = ?state,
        activity = ?activity,
        message_safety = ?message_safety,
        "merge_pane_observations: chose primary observation"
    );

    PaneRuntimeTruth {
        kind,
        state,
        current_command: command_basename,
        source: chosen.source,
        confidence: chosen.confidence,
        activity,
        message_safety,
    }
}

/// Pick the highest-confidence observation that has signal.
fn pick_observation(observations: &[PaneObservation]) -> Option<&PaneObservation> {
    observations
        .iter()
        .filter(|o| o.has_signal() && o.confidence != PaneObservationConfidence::None)
        .max_by_key(|o| o.confidence)
}

/// Classify an observed basename into a [`PaneKind`] using only the
/// configured tool catalog and the canonical shell allow-list.
///
/// This is the only kind-classification path in workhive-core. The
/// historical (pre-2026-05-07) tmux adapter contained a hardcoded
/// `AGENT_TOOLS` / `GIT_UI_TOOLS` substring matcher; that path is now
/// replaced by exact basename matching against
/// [`ToolEntry::effective_process_names`].
fn classify_kind(basename: &str, tools: &ToolCatalog) -> PaneKind {
    if basename.is_empty() {
        return PaneKind::Unknown;
    }
    if let Some(entry) = tools.match_process_name(basename) {
        return entry.effective_kind().to_pane_kind();
    }
    if is_shell_basename(basename) {
        return PaneKind::Terminal;
    }
    PaneKind::Unknown
}

/// Infer state when the chosen observation source did not provide a
/// `state_hint`.
///
/// * Empty basename → `Unknown` (we never invent a state).
/// * Shell basename → `Idle` (the canonical "no foreground tool").
/// * Anything else → `Running` (some user process owns the foreground).
fn infer_state(basename: &str, kind: PaneKind) -> PaneState {
    if basename.is_empty() {
        return PaneState::Unknown;
    }
    if matches!(kind, PaneKind::Terminal) {
        return PaneState::Idle;
    }
    PaneState::Running
}

/// Canonical shell basenames. We deliberately avoid substring matches
/// (which would mis-classify e.g. `ssh`, `lsh`, `bash-completion`).
const SHELL_BASENAMES: &[&str] = &["zsh", "bash", "fish", "sh", "ksh", "dash", "ash"];

fn is_shell_basename(basename: &str) -> bool {
    SHELL_BASENAMES.contains(&basename)
}

/// Strip path separators so callers can pass raw program identifiers
/// (e.g. tmux's `pane_current_command`, supervisor stored `program_path`,
/// or OS observer `argv[0]`).
fn basename_of(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

/// Build the layered observation stack for a single pane, then merge.
///
/// This is the single-pane entry point used by `workhive pane show`.
/// `pane list` should prefer the batched [`gather_pane_truths`] to
/// keep observation cost at O(1) external probes per command.
///
/// Layering:
///
/// 1. ask the supervisor port (`Authoritative` when Workhive launched
///    the pane through its supervisor),
/// 2. ask the OS process observer (`High` confidence),
/// 3. fall back to tmux's `pane_current_command` (`Fallback`),
/// 4. let the merge policy pick the highest-confidence signal.
///
/// Errors from supervisor / observer adapters are logged and treated
/// as silent layers — the merge policy then falls through. The OS
/// observer's `Unsupported` variant is logged at `debug` because
/// "no observer for this platform" is a known fallback path; every
/// other error is logged at `warn` to surface unexpected adapter
/// failures per the error-handling policy.
pub fn gather_pane_truth(
    snapshot: &RuntimeUnitSnapshot,
    supervisor: Option<&dyn PaneSupervisorPort>,
    observer: Option<&dyn ProcessObserverPort>,
    group: &str,
    workspace: &str,
    tools: &ToolCatalog,
) -> PaneRuntimeTruth {
    let mut observations: Vec<PaneObservation> = Vec::with_capacity(3);

    if let Some(supervisor) = supervisor
        && let Some(obs) = observe_supervisor(supervisor, group, workspace, &snapshot.id)
    {
        observations.push(obs);
    }

    if let Some(observer) = observer
        && let Some(obs) = observe_process_single(observer, snapshot)
    {
        observations.push(obs);
    }

    if !snapshot.command.trim().is_empty() {
        observations.push(PaneObservation::tmux_fallback(snapshot.command.clone()));
    } else {
        tracing::debug!(
            unit_id = %snapshot.id,
            "gather_pane_truth: tmux command is empty; no fallback observation"
        );
    }

    let truth = merge_pane_observations(&observations, tools);
    backend_asserted_truth(snapshot, truth)
}

/// Last-resort fallback: the runtime backend classified this pane itself.
///
/// Some backends report a structured classification for a pane without
/// exposing a foreground command — Herdr maps its `agent_status` onto
/// `kind`/`state` and leaves `command` empty, while also exposing no tty or
/// pid for the OS observer to inspect. Such a pane has no observation with
/// signal at all, so the merge returns a fully unknown truth and the pane
/// renders as unknown even though the backend that owns it knows exactly
/// what is running in it.
///
/// This is deliberately a *fallback*, not a layer: it only applies when no
/// observation produced signal at all (`merged.source` is still [`Unknown`]),
/// so it can never override a supervisor, OS-process, or tmux-command
/// observation. The backend must also have declared a classification of its
/// own — a backend that observed nothing asserts nothing.
///
/// [`Unknown`]: PaneObservationSource::Unknown
fn backend_asserted_truth(
    snapshot: &RuntimeUnitSnapshot,
    merged: PaneRuntimeTruth,
) -> PaneRuntimeTruth {
    if !matches!(merged.source, PaneObservationSource::Unknown)
        || (matches!(snapshot.kind, PaneKind::Unknown)
            && matches!(snapshot.state, PaneState::Unknown))
    {
        return merged;
    }

    let truth = PaneRuntimeTruth::from_layered(
        snapshot.kind,
        snapshot.state,
        String::new(),
        PaneObservationSource::RuntimeBackend,
        PaneObservationSource::RuntimeBackend.baseline_confidence(),
    );
    tracing::debug!(
        unit_id = %snapshot.id,
        kind = ?truth.kind,
        state = ?truth.state,
        "backend_asserted_truth: no observation had signal; using the backend's own classification"
    );
    truth
}

/// Batched variant of [`gather_pane_truth`].
///
/// Used by `workhive pane list` to amortise expensive OS probes
/// across all panes in a workspace: the OS process observer is asked
/// once via [`ProcessObserverPort::observe_many`], the supervisor
/// metadata is read once per pane (cheap, file-local), and the
/// per-snapshot observation stacks are merged.
///
/// Result is parallel to `snapshots` (same length, same order).
pub fn gather_pane_truths(
    snapshots: &[RuntimeUnitSnapshot],
    supervisor: Option<&dyn PaneSupervisorPort>,
    observer: Option<&dyn ProcessObserverPort>,
    group: &str,
    workspace: &str,
    tools: &ToolCatalog,
) -> Vec<PaneRuntimeTruth> {
    if snapshots.is_empty() {
        return Vec::new();
    }

    // Batch the OS observer query so `pane list` only spawns at most
    // one external process for the whole list (not O(panes)).
    let observer_results: Vec<Option<PaneObservation>> = match observer {
        Some(observer) => {
            let lookups: Vec<PaneProcessLookup> = snapshots
                .iter()
                .map(|s| PaneProcessLookup {
                    tty: s.tty.clone(),
                    shell_pid: s.process_pid,
                })
                .collect();
            match observer.observe_many(&lookups) {
                Ok(many) if many.len() == lookups.len() => many,
                Ok(many) => {
                    tracing::warn!(
                        expected = lookups.len(),
                        got = many.len(),
                        "gather_pane_truths: observe_many returned mismatched length; ignoring"
                    );
                    vec![None; snapshots.len()]
                }
                Err(crate::ports::ProcessObserverError::Unsupported) => {
                    tracing::debug!(
                        "gather_pane_truths: OS process observer unsupported on this platform"
                    );
                    vec![None; snapshots.len()]
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "gather_pane_truths: OS process observer failed; ignoring layer"
                    );
                    vec![None; snapshots.len()]
                }
            }
        }
        None => vec![None; snapshots.len()],
    };

    snapshots
        .iter()
        .zip(observer_results)
        .map(|(snapshot, observer_obs)| {
            let mut observations: Vec<PaneObservation> = Vec::with_capacity(3);

            if let Some(supervisor) = supervisor
                && let Some(obs) = observe_supervisor(supervisor, group, workspace, &snapshot.id)
            {
                observations.push(obs);
            }

            if let Some(obs) = observer_obs {
                observations.push(obs);
            }

            if !snapshot.command.trim().is_empty() {
                observations.push(PaneObservation::tmux_fallback(snapshot.command.clone()));
            }

            let truth = merge_pane_observations(&observations, tools);
            backend_asserted_truth(snapshot, truth)
        })
        .collect()
}

fn observe_supervisor(
    supervisor: &dyn PaneSupervisorPort,
    group: &str,
    workspace: &str,
    unit_id: &UnitId,
) -> Option<PaneObservation> {
    match supervisor.observe(group, workspace, unit_id) {
        Ok(obs) => obs,
        Err(error) => {
            tracing::warn!(
                %error,
                unit_id = %unit_id,
                group,
                workspace,
                "gather_pane_truth: supervisor adapter failed; ignoring layer"
            );
            None
        }
    }
}

fn observe_process_single(
    observer: &dyn ProcessObserverPort,
    snapshot: &RuntimeUnitSnapshot,
) -> Option<PaneObservation> {
    let lookup = PaneProcessLookup {
        tty: snapshot.tty.clone(),
        shell_pid: snapshot.process_pid,
    };
    if lookup.is_empty() {
        tracing::debug!(
            unit_id = %snapshot.id,
            "gather_pane_truth: snapshot has no tty/pid; skipping OS observer layer"
        );
        return None;
    }
    match observer.observe(&lookup) {
        Ok(obs) => obs,
        Err(crate::ports::ProcessObserverError::Unsupported) => {
            tracing::debug!(
                unit_id = %snapshot.id,
                "gather_pane_truth: OS process observer unsupported on this platform"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                %error,
                unit_id = %snapshot.id,
                "gather_pane_truth: OS process observer failed; ignoring layer"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        PaneObservationSource, PaneObservedCommand, PaneObservedProcess, PaneState, ToolCatalog,
        ToolName,
    };

    fn supervisor_running(basename: &str) -> PaneObservation {
        PaneObservation::supervisor(
            PaneObservedCommand::from_basename(basename),
            PaneState::Running,
        )
    }

    fn os_observer(basename: &str) -> PaneObservation {
        PaneObservation::process_observer(PaneObservedProcess::from_foreground(
            PaneObservedCommand::from_basename(basename),
        ))
    }

    fn tmux_fallback(basename: &str) -> PaneObservation {
        PaneObservation::tmux_fallback(basename)
    }

    fn empty_obs(source: PaneObservationSource) -> PaneObservation {
        PaneObservation {
            source,
            confidence: source.baseline_confidence(),
            process: None,
            state_hint: None,
        }
    }

    fn catalog() -> ToolCatalog {
        ToolCatalog::builtin_defaults()
    }

    #[test]
    fn returns_unknown_when_no_observations() {
        let truth = merge_pane_observations(&[], &catalog());
        assert_eq!(truth, PaneRuntimeTruth::unknown());
    }

    #[test]
    fn returns_unknown_when_all_observations_lack_signal() {
        let observations = [
            empty_obs(PaneObservationSource::Supervisor),
            empty_obs(PaneObservationSource::ProcessObserver),
            empty_obs(PaneObservationSource::Unknown),
        ];
        let truth = merge_pane_observations(&observations, &catalog());
        assert_eq!(truth.kind, PaneKind::Unknown);
        assert_eq!(truth.state, PaneState::Unknown);
        assert!(truth.current_command.is_empty());
    }

    #[test]
    fn supervisor_live_beats_os_observer() {
        let truth = merge_pane_observations(
            &[supervisor_running("claude"), os_observer("zsh")],
            &catalog(),
        );
        assert_eq!(truth.source, PaneObservationSource::Supervisor);
        assert_eq!(truth.confidence, PaneObservationConfidence::Authoritative);
        assert_eq!(truth.current_command, "claude");
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.state, PaneState::Running);
    }

    #[test]
    fn os_observer_beats_tmux_fallback() {
        let truth =
            merge_pane_observations(&[os_observer("codex"), tmux_fallback("zsh")], &catalog());
        assert_eq!(truth.source, PaneObservationSource::ProcessObserver);
        assert_eq!(truth.confidence, PaneObservationConfidence::High);
        assert_eq!(truth.current_command, "codex");
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn tmux_fallback_used_when_stronger_sources_silent() {
        let truth = merge_pane_observations(&[tmux_fallback("zsh")], &catalog());
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.confidence, PaneObservationConfidence::Fallback);
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.state, PaneState::Idle);
    }

    /// "Stale supervisor metadata does not override current
    /// observation" — the adapter signals staleness by either
    /// returning `Ok(None)` (so no supervisor obs is fed in) or by
    /// downgrading the observation's confidence below
    /// `Authoritative`. The merge policy then trusts the higher
    /// confidence source.
    #[test]
    fn stale_supervisor_observation_is_overridden_by_high_confidence_os_observer() {
        let stale = PaneObservation {
            source: PaneObservationSource::Supervisor,
            confidence: PaneObservationConfidence::Fallback, // adapter downgraded
            process: Some(PaneObservedProcess::from_foreground(
                PaneObservedCommand::from_basename("claude"),
            )),
            state_hint: Some(PaneState::Running),
        };
        let live_os = os_observer("zsh");
        let truth = merge_pane_observations(&[stale, live_os], &catalog());
        assert_eq!(truth.source, PaneObservationSource::ProcessObserver);
        assert_eq!(truth.current_command, "zsh");
        assert_eq!(truth.kind, PaneKind::Terminal);
    }

    #[test]
    fn supervisor_state_hint_is_trusted_over_inferred_state() {
        let exited = PaneObservation::supervisor(
            PaneObservedCommand::from_basename("claude"),
            PaneState::Idle,
        );
        let truth = merge_pane_observations(&[exited], &catalog());
        // Even though `claude` is an agent, supervisor said it's
        // idle; we trust the source-provided hint.
        assert_eq!(truth.state, PaneState::Idle);
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn classify_kind_recognizes_codex_with_arch_suffix_via_catalog() {
        let truth =
            merge_pane_observations(&[os_observer("codex-x86_64-apple-darwin")], &catalog());
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.current_command, "codex-x86_64-apple-darwin");
    }

    #[test]
    fn classify_kind_handles_path_prefixed_basename() {
        let truth = merge_pane_observations(&[os_observer("/usr/local/bin/codex")], &catalog());
        // Path-prefixed input is normalised before classification.
        assert_eq!(truth.current_command, "codex");
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn classify_kind_unknown_for_unrecognised_basename() {
        let truth = merge_pane_observations(&[os_observer("totally-random-bin")], &catalog());
        assert_eq!(truth.kind, PaneKind::Unknown);
        assert_eq!(truth.state, PaneState::Running);
    }

    #[test]
    fn shell_basename_classifies_as_terminal_idle() {
        let truth = merge_pane_observations(&[os_observer("zsh")], &catalog());
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.state, PaneState::Idle);
    }

    /// Substring-matching used to misclassify processes whose names
    /// contained `sh` (e.g. `ssh`). The new merge policy must use
    /// exact basename matching only.
    #[test]
    fn shell_substring_does_not_promote_ssh_to_terminal() {
        let truth = merge_pane_observations(&[os_observer("ssh")], &catalog());
        assert_eq!(truth.kind, PaneKind::Unknown);
    }

    /// User-configured tools may add explicit `process_names` so the
    /// matcher recognises a custom binary as the user's tool kind.
    #[test]
    fn user_configured_process_names_extend_classification() {
        use crate::domain::{CommandSpec, ToolEntry, ToolKind};
        let mut tools = ToolCatalog::builtin_defaults();
        let mut entry = ToolEntry::new(
            ToolName::new("my-private-agent").unwrap(),
            "my private AI agent",
            CommandSpec::new("my-agent-bin", [] as [&str; 0]),
            ToolKind::Agent,
        );
        entry.process_names = vec!["mybin".to_owned(), "mybin-arm64".to_owned()];
        tools.add(entry).expect("custom tool name must be unique");

        let truth = merge_pane_observations(&[os_observer("mybin-arm64")], &tools);
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.current_command, "mybin-arm64");
    }

    #[test]
    fn empty_basename_yields_unknown_state() {
        let observation = PaneObservation::process_observer(PaneObservedProcess::from_foreground(
            PaneObservedCommand::from_basename(""),
        ));
        let truth = merge_pane_observations(&[observation], &catalog());
        assert_eq!(truth.current_command, "");
        assert_eq!(truth.kind, PaneKind::Unknown);
        assert_eq!(truth.state, PaneState::Unknown);
    }

    /// Bug 2026-05-17: empty shell / empty command panes must render as
    /// terminal/shell idle, not unknown, when Workhive has launch-intent
    /// evidence (modeled here as a supervisor observation that
    /// authoritatively reports state = Idle with no foreground).
    #[test]
    fn supervisor_idle_with_empty_foreground_renders_as_terminal_idle() {
        let empty_idle = PaneObservation {
            source: PaneObservationSource::Supervisor,
            confidence: PaneObservationConfidence::Authoritative,
            process: None,
            state_hint: Some(PaneState::Idle),
        };
        let truth = merge_pane_observations(&[empty_idle], &catalog());
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.state, PaneState::Idle);
        assert_eq!(truth.source, PaneObservationSource::Supervisor);
        assert!(truth.current_command.is_empty());
    }

    /// Negative guard: a Running state with no foreground is genuinely
    /// ambiguous and must remain Unknown — we only promote empty
    /// foregrounds to Terminal when the asserted state is Idle.
    #[test]
    fn supervisor_running_with_empty_foreground_stays_unknown() {
        let empty_running = PaneObservation {
            source: PaneObservationSource::Supervisor,
            confidence: PaneObservationConfidence::Authoritative,
            process: None,
            state_hint: Some(PaneState::Running),
        };
        let truth = merge_pane_observations(&[empty_running], &catalog());
        assert_eq!(truth.kind, PaneKind::Unknown);
        assert_eq!(truth.state, PaneState::Running);
    }

    /// Cursor Agent classification: catalog-modeled `cursor-agent`
    /// observed via the OS process observer must classify as the
    /// canonical Agent kind.
    #[test]
    fn cursor_agent_observed_basename_classifies_as_agent() {
        let truth = merge_pane_observations(&[os_observer("cursor-agent")], &catalog());
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.current_command, "cursor-agent");
        assert_eq!(truth.state, PaneState::Running);
    }

    /// Cursor Agent `agent` alias classification: an observed
    /// foreground process named `agent` must map to the same canonical
    /// Cursor Agent tool entry via `process_names` alias matching.
    #[test]
    fn cursor_agent_alias_agent_classifies_as_agent() {
        let truth = merge_pane_observations(&[os_observer("agent")], &catalog());
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.current_command, "agent");
    }

    /// Negative guard: lookalike basenames must not be classified as
    /// Cursor Agent — exact basename matching only, no substring.
    #[test]
    fn cursor_agent_lookalike_basenames_do_not_classify_as_agent() {
        for name in ["cursor-agent-extra", "my-cursor-agent", "agent-helper"] {
            let truth = merge_pane_observations(&[os_observer(name)], &catalog());
            assert_eq!(
                truth.kind,
                PaneKind::Unknown,
                "lookalike `{}` must not match Cursor Agent",
                name
            );
        }
    }

    // ── gather_pane_truth integration tests ─────────────────────────

    use crate::ports::{
        PaneProcessLookup, PaneSupervisorError, PaneSupervisorPort, ProcessObserverError,
        ProcessObserverPort, RuntimeUnitSnapshot,
    };
    use camino::Utf8PathBuf;

    /// Test fake for [`PaneSupervisorPort`] — returns whatever the
    /// caller stashed into it. Lets tests assert that `gather_pane_truth`
    /// (a) calls the port, (b) honors errors as silent, (c) prefers
    /// authoritative metadata.
    struct FakeSupervisor {
        result: Result<Option<PaneObservation>, PaneSupervisorError>,
    }

    impl PaneSupervisorPort for FakeSupervisor {
        fn observe(
            &self,
            _group: &str,
            _workspace: &str,
            _unit_id: &UnitId,
        ) -> Result<Option<PaneObservation>, PaneSupervisorError> {
            match &self.result {
                Ok(opt) => Ok(opt.clone()),
                Err(PaneSupervisorError::Io { detail }) => Err(PaneSupervisorError::Io {
                    detail: detail.clone(),
                }),
                Err(PaneSupervisorError::Corrupt { detail }) => Err(PaneSupervisorError::Corrupt {
                    detail: detail.clone(),
                }),
            }
        }
    }

    /// Test fake for [`ProcessObserverPort`].
    struct FakeObserver {
        result: Result<Option<PaneObservation>, ProcessObserverError>,
    }

    impl ProcessObserverPort for FakeObserver {
        fn observe(
            &self,
            _lookup: &PaneProcessLookup,
        ) -> Result<Option<PaneObservation>, ProcessObserverError> {
            match &self.result {
                Ok(opt) => Ok(opt.clone()),
                Err(ProcessObserverError::Unsupported) => Err(ProcessObserverError::Unsupported),
                Err(ProcessObserverError::Io { detail }) => Err(ProcessObserverError::Io {
                    detail: detail.clone(),
                }),
                Err(ProcessObserverError::Parse { detail }) => Err(ProcessObserverError::Parse {
                    detail: detail.clone(),
                }),
            }
        }
    }

    fn snapshot_with(command: &str, tty: Option<&str>, pid: Option<u32>) -> RuntimeUnitSnapshot {
        RuntimeUnitSnapshot {
            id: UnitId::new("%2"),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp/work"),
            command: command.to_owned(),
            active: false,
            kind: PaneKind::Unknown,
            state: PaneState::Unknown,
            tty: tty.map(str::to_owned),
            process_pid: pid,
        }
    }

    /// Backend-asserted snapshot: the runtime itself classified the pane, but
    /// exposes no foreground command and no tty/pid, so no other layer can add
    /// signal. This is the shape a Herdr pane produces (`agent_status` maps to
    /// `kind`/`state` while `command` stays empty).
    fn snapshot_backend_asserted(kind: PaneKind, state: PaneState) -> RuntimeUnitSnapshot {
        RuntimeUnitSnapshot {
            id: UnitId::new("w6:p2"),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp/work"),
            command: String::new(),
            active: true,
            kind,
            state,
            tty: None,
            process_pid: None,
        }
    }

    #[test]
    fn gather_keeps_backend_asserted_agent_without_foreground_command() {
        let snapshot = snapshot_backend_asserted(PaneKind::Agent, PaneState::Idle);
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver { result: Ok(None) };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(
            truth.kind,
            PaneKind::Agent,
            "a backend that classifies its own pane as an agent must not degrade to unknown"
        );
        assert_eq!(truth.state, PaneState::Idle);
        assert_eq!(truth.source, PaneObservationSource::RuntimeBackend);
        assert_eq!(
            truth.activity,
            PaneActivityState::AgentForeground,
            "an agent the backend still reports is open, not a shell idling"
        );
    }

    /// The backend fallback is a last resort: a live observation from the
    /// supervisor or the OS observer must win, so this can never mask a real
    /// foreground process.
    #[test]
    fn backend_assertion_never_overrides_a_live_observation() {
        let snapshot = snapshot_backend_asserted(PaneKind::Terminal, PaneState::Idle);
        let supervisor = FakeSupervisor {
            result: Ok(Some(supervisor_running("claude"))),
        };
        let truth = gather_pane_truth(&snapshot, Some(&supervisor), None, "g", "ws", &catalog());
        assert_eq!(truth.source, PaneObservationSource::Supervisor);
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.current_command, "claude");
    }

    /// A backend that reports no classification of its own behaves exactly as
    /// before: nothing to fall back to, so the pane stays unknown.
    #[test]
    fn unclassified_backend_snapshot_still_renders_unknown() {
        let snapshot = snapshot_backend_asserted(PaneKind::Unknown, PaneState::Unknown);
        let truth = gather_pane_truth(&snapshot, None, None, "g", "ws", &catalog());
        assert_eq!(truth, PaneRuntimeTruth::unknown());
    }

    /// Tmux-shaped snapshots derive `kind`/`state` from `command`, so the
    /// backend fallback must be unreachable for them: a reported command keeps
    /// the tmux-fallback provenance, and an empty command asserts nothing.
    #[test]
    fn tmux_shaped_snapshots_keep_their_original_provenance() {
        let reported = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let truth = gather_pane_truth(&reported, None, None, "g", "ws", &catalog());
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.kind, PaneKind::Terminal);

        let silent = snapshot_with("", None, None);
        let truth = gather_pane_truth(&silent, None, None, "g", "ws", &catalog());
        assert_eq!(truth, PaneRuntimeTruth::unknown());
    }

    #[test]
    fn gather_uses_supervisor_when_authoritative() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor {
            result: Ok(Some(supervisor_running("claude"))),
        };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.source, PaneObservationSource::Supervisor);
        assert_eq!(truth.current_command, "claude");
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn gather_uses_os_observer_when_supervisor_silent() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("codex"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.source, PaneObservationSource::ProcessObserver);
        assert_eq!(truth.current_command, "codex");
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn gather_falls_back_to_tmux_when_higher_layers_silent() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver { result: Ok(None) };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.current_command, "zsh");
        assert_eq!(truth.kind, PaneKind::Terminal);
    }

    #[test]
    fn gather_handles_missing_adapters_gracefully() {
        let snapshot = snapshot_with("codex", None, None);
        let truth = gather_pane_truth(&snapshot, None, None, "g", "ws", &catalog());
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.kind, PaneKind::Agent);
    }

    #[test]
    fn gather_returns_unknown_when_all_layers_silent() {
        let snapshot = snapshot_with("", None, None);
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Err(ProcessObserverError::Unsupported),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth, PaneRuntimeTruth::unknown());
    }

    #[test]
    fn gather_skips_observer_when_lookup_is_empty() {
        // No tty + no pid => observer cannot do anything useful.
        // The merge still falls through to tmux fallback.
        let snapshot = snapshot_with("lazygit", None, None);
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))), // would win if called
        };
        let truth = gather_pane_truth(&snapshot, None, Some(&observer), "g", "ws", &catalog());
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.kind, PaneKind::GitUi);
    }

    #[test]
    fn gather_treats_supervisor_error_as_silent_layer() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor {
            result: Err(PaneSupervisorError::Io {
                detail: "permission denied".to_owned(),
            }),
        };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("codex"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        // Supervisor error must NOT block the merge. OS observer wins.
        assert_eq!(truth.source, PaneObservationSource::ProcessObserver);
        assert_eq!(truth.current_command, "codex");
    }

    #[test]
    fn gather_treats_observer_unsupported_as_silent_layer() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Err(ProcessObserverError::Unsupported),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
        assert_eq!(truth.kind, PaneKind::Terminal);
    }

    // ── Exited supervisor must not pretend to own current truth ───
    //
    // Once the supervised tool has exited (e.g. user pressed Ctrl+C
    // inside `claude` and dropped back to the shell), the JSON-on-disk
    // adapter returns `Ok(None)` for the supervisor layer. These tests
    // assert that the OS observer / tmux fallback then describe the
    // pane truthfully as the real shell, rather than the merge policy
    // keeping the historical tool basename at any confidence.

    #[test]
    fn exited_supervisor_with_os_observer_zsh_yields_terminal_idle_via_process_observer() {
        let snapshot = snapshot_with("claude", Some("/dev/ttys001"), Some(4242));
        // Exited supervisor signals "no current observation" via
        // Ok(None) — the JSON adapter is responsible for that mapping.
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.current_command, "zsh");
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.state, PaneState::Idle);
        assert_eq!(truth.source, PaneObservationSource::ProcessObserver);
    }

    #[test]
    fn exited_supervisor_with_tmux_fallback_zsh_yields_terminal_idle_via_tmux() {
        // Snapshot's tmux command reflects the current foreground
        // (the user is back at the shell). OS observer is silent.
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver { result: Ok(None) };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.current_command, "zsh");
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.state, PaneState::Idle);
        assert_eq!(truth.source, PaneObservationSource::TmuxCurrentCommand);
    }

    // ── Activity / message-safety semantics ─────────────────────
    //
    // The
    // user-facing activity column must not equate "foreground command
    // is `codex`/`claude`" with "AI agent is busy". Only an
    // Authoritative supervisor handshake unlocks `AgentBusy` /
    // `SafeStructured`.

    use crate::domain::{PaneActivityState, PaneMessageSafety};

    #[test]
    fn os_observer_codex_is_foreground_process_not_agent_busy() {
        // Foreground process recognised as `codex` via the OS-level
        // observer. There is no supervisor handshake, so the merge
        // policy must NOT promote this row to AgentBusy / SafeStructured.
        let truth = merge_pane_observations(&[os_observer("codex")], &catalog());
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(
            truth.activity,
            PaneActivityState::ForegroundProcess,
            "OS-observed agent without supervisor handshake must surface \
             as ForegroundProcess, never AgentBusy"
        );
        assert!(
            !truth.activity.implies_agent_work(),
            "non-supervised agent observation must not imply AI work"
        );
        assert_eq!(truth.message_safety, PaneMessageSafety::UnverifiedAgent);
        assert!(
            !truth.message_safety.permits_structured_message(),
            "unverified agent must refuse structured-message routing"
        );
    }

    #[test]
    fn tmux_fallback_codex_is_foreground_process_not_agent_busy() {
        let truth = merge_pane_observations(&[tmux_fallback("codex")], &catalog());
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.activity, PaneActivityState::ForegroundProcess);
        assert_eq!(truth.message_safety, PaneMessageSafety::UnverifiedAgent);
    }

    /// The supervisor adapter signals "Authoritative + Running" when
    /// the recorded pid is alive, but for interactive AI assistants pid
    /// liveness is NOT proof that the agent is actively processing
    /// user work — assistants spend most of their time idle waiting
    /// for input. Until Workhive plumbs a real busy/processing
    /// protocol signal, the conservative `AgentForeground` state must
    /// be produced instead of `AgentBusy`. Message safety, which is
    /// concerned with whether the supervisor handshake exists at all,
    /// remains `SafeStructured`.
    #[test]
    fn supervisor_authoritative_running_agent_is_agent_foreground_not_agent_busy() {
        let truth = merge_pane_observations(&[supervisor_running("claude")], &catalog());
        assert_eq!(
            truth.activity,
            PaneActivityState::AgentForeground,
            "supervised live agent must NOT promote to AgentBusy from pid \
             liveness alone; AgentForeground is the \
             conservative truthful state"
        );
        assert!(
            !truth.activity.implies_agent_work(),
            "AgentForeground must NOT imply AI work: pid liveness is not a \
             busy/processing signal"
        );
        assert_ne!(
            truth.activity,
            PaneActivityState::AgentBusy,
            "AgentBusy is reserved for a future explicit authoritative busy \
             protocol handshake — it must be unreachable from supervisor \
             liveness alone"
        );
        // Message safety still flows from the supervisor handshake,
        // independent of the activity column. The handshake exists, so
        // structured-message routing is still permitted.
        assert_eq!(truth.message_safety, PaneMessageSafety::SafeStructured);
        assert!(truth.message_safety.permits_structured_message());
    }

    /// Domain rule:
    /// `AgentBusy` may not be derived from supervisor liveness today,
    /// regardless of which kind/state pair feeds the deriver. Either an
    /// explicit busy protocol signal exists or the activity must be
    /// conservative.
    #[test]
    fn agent_busy_is_unreachable_from_current_observation_layers() {
        use crate::domain::derive_activity;
        for source in [
            PaneObservationSource::Supervisor,
            PaneObservationSource::ProcessObserver,
            PaneObservationSource::TmuxCurrentCommand,
            PaneObservationSource::Unknown,
        ] {
            for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
                let confidence = source.baseline_confidence();
                let activity = derive_activity(PaneKind::Agent, state, source, confidence);
                assert_ne!(
                    activity,
                    PaneActivityState::AgentBusy,
                    "AgentBusy must not be reachable today from any observation \
                     (state={state:?}, source={source:?}, confidence={confidence:?})"
                );
            }
        }
    }

    #[test]
    fn supervisor_authoritative_idle_agent_is_exited_and_unverified() {
        // Supervisor knows about the agent and reports state Idle:
        // the agent has left the foreground. We surface this as
        // Exited so the row tells the user the *previous* tool
        // exited, distinct from a clean ShellIdle.
        //
        // Domain rule: this configuration MUST NOT remain
        // `SafeStructured`. The structured channel does not exist
        // once the supervised agent has exited, even though we
        // remember its identity. Only a running supervisor-
        // authoritative agent may advertise structured-message
        // capabilities. Reusing `UnverifiedAgent` here keeps the
        // registry/sync runtime capability aligned with message
        // safety so router and CLI agree on safety semantics.
        let exited = PaneObservation::supervisor(
            PaneObservedCommand::from_basename("claude"),
            PaneState::Idle,
        );
        let truth = merge_pane_observations(&[exited], &catalog());
        assert_eq!(truth.activity, PaneActivityState::Exited);
        assert!(!truth.activity.implies_agent_work());
        assert_eq!(
            truth.message_safety,
            PaneMessageSafety::UnverifiedAgent,
            "supervisor authoritative + Idle agent must collapse to \
             UnverifiedAgent: the live structured channel is gone"
        );
        assert!(
            !truth.message_safety.permits_structured_message(),
            "router must refuse structured-message routing for an \
             exited supervised agent"
        );
    }

    /// User Ctrl+C exits `codex`, returning to the shell. The
    /// supervisor adapter signals "no current observation" via
    /// Ok(None); the OS observer reports the live `zsh`. The merge
    /// policy must surface a Terminal pane in ShellIdle activity, not
    /// continue to render an active agent based on stale launch
    /// identity.
    #[test]
    fn ctrl_c_back_to_shell_yields_terminal_shell_idle_activity() {
        let snapshot = snapshot_with("claude", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.activity, PaneActivityState::ShellIdle);
        assert!(
            !truth.activity.implies_agent_work(),
            "post-Ctrl+C shell must not advertise agent work"
        );
        assert_eq!(truth.message_safety, PaneMessageSafety::ShellInput);
    }

    /// User manually launches `codex` in a Workhive-managed pane: the
    /// OS observer can detect the foreground process, but no
    /// supervisor handshake exists. The activity must be the
    /// conservative ForegroundProcess — Workhive cannot prove the
    /// pane is safe for structured messaging just because the
    /// process is recognised.
    #[test]
    fn user_manual_codex_launch_does_not_promote_to_agent_busy_or_safe_structured() {
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("codex"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.activity, PaneActivityState::ForegroundProcess);
        assert_eq!(truth.message_safety, PaneMessageSafety::UnverifiedAgent);
    }

    /// `lazygit` always classifies as `GitUi` and surfaces
    /// `NotApplicable` for message safety, regardless of which
    /// observation layer chose. TUIs
    /// must never be MessageReceivers.
    #[test]
    fn lazygit_pane_is_not_applicable_for_messaging() {
        for obs in [os_observer("lazygit"), tmux_fallback("lazygit")] {
            let truth = merge_pane_observations(std::slice::from_ref(&obs), &catalog());
            assert_eq!(truth.kind, PaneKind::GitUi);
            assert_eq!(truth.message_safety, PaneMessageSafety::NotApplicable);
            assert!(
                !truth.message_safety.permits_structured_message(),
                "GitUi pane must not permit structured-message routing \
                 (observation layer: {:?})",
                truth.source
            );
        }
    }

    /// Live observation must override stale launch metadata.
    /// `pane split --cmd "codex"` declares the launch as `codex`,
    /// but if the user replaces the foreground process with a shell,
    /// the merge policy (which only sees observations, not launch
    /// identity) must return Terminal/ShellIdle.
    #[test]
    fn live_shell_observation_wins_over_implicit_launch_history() {
        // Simulate "user split --cmd codex, then replaced the
        // process". The supervisor doesn't have a record (Ok(None));
        // the OS observer sees zsh; the snapshot's tmux fallback also
        // sees zsh. The truth is a Terminal pane.
        let snapshot = snapshot_with("zsh", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor { result: Ok(None) };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.kind, PaneKind::Terminal);
        assert_eq!(truth.activity, PaneActivityState::ShellIdle);
        assert_eq!(truth.message_safety, PaneMessageSafety::ShellInput);
    }

    #[test]
    fn unknown_observation_yields_conservative_activity_and_safety() {
        let truth = merge_pane_observations(&[], &catalog());
        assert_eq!(truth.activity, PaneActivityState::Unknown);
        assert_eq!(truth.message_safety, PaneMessageSafety::Unknown);
        assert!(!truth.activity.implies_agent_work());
        assert!(!truth.message_safety.permits_structured_message());
    }

    #[test]
    fn running_supervisor_with_live_pid_is_authoritative_running() {
        let snapshot = snapshot_with("claude", Some("/dev/ttys001"), Some(4242));
        let supervisor = FakeSupervisor {
            result: Ok(Some(supervisor_running("claude"))),
        };
        let observer = FakeObserver {
            result: Ok(Some(os_observer("zsh"))),
        };
        let truth = gather_pane_truth(
            &snapshot,
            Some(&supervisor),
            Some(&observer),
            "g",
            "ws",
            &catalog(),
        );
        assert_eq!(truth.current_command, "claude");
        assert_eq!(truth.kind, PaneKind::Agent);
        assert_eq!(truth.state, PaneState::Running);
        assert_eq!(truth.source, PaneObservationSource::Supervisor);
        assert_eq!(truth.confidence, PaneObservationConfidence::Authoritative);
    }
}

//! Pane runtime observation domain types.
//!
//! These types model the layered runtime-observation architecture that
//! `workhive pane list` / `pane show` (and any future pane-status
//! consumer) depend on. The architecture explicitly avoids letting the
//! historical launch identity rewrite live observation, and explicitly
//! avoids having tmux's `pane_current_command` string heuristic remain
//! the highest-confidence runtime classifier.
//!
//! ## Confidence ordering (highest first)
//!
//! 1. Workhive-supervised process metadata (`Source::Supervisor`) —
//!    Workhive launched the process, so the current binary, pid, and
//!    exit status are known facts.
//! 2. OS process observation (`Source::ProcessObserver`) — querying the
//!    foreground process / process tree of the pane. Reliable because
//!    it returns the actual executable name.
//! 3. tmux fallback (`Source::TmuxCurrentCommand`) — purely a string
//!    field surfaced by tmux. Fine for a baseline display when neither
//!    of the higher-confidence sources can answer, but it must never be
//!    the authoritative classifier.
//! 4. `Source::Unknown` — no observation could be obtained at all. The
//!    truthful representation is `PaneKind::Unknown` /
//!    `PaneState::Unknown`; we never invent a state.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::pane::{PaneActivityState, PaneKind, PaneMessageSafety, PaneState};

/// Confidence level of a single pane observation.
///
/// Higher confidence variants override lower confidence variants in
/// the merge policy. Two observations of the same confidence are not
/// inherently comparable; the merge policy is documented in
/// `crate::usecases::pane_observation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneObservationConfidence {
    /// No information could be gathered.
    None,
    /// tmux's `pane_current_command` string. Heuristic, not authoritative.
    Fallback,
    /// OS-level process observation (foreground command / process tree).
    High,
    /// Workhive-supervised process metadata; the binary identity is known.
    Authoritative,
}

impl PaneObservationConfidence {
    /// i18n key for the user-facing confidence label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::None => crate::i18n_keys::PANE_OBSERVATION_CONFIDENCE_NONE,
            Self::Fallback => crate::i18n_keys::PANE_OBSERVATION_CONFIDENCE_FALLBACK,
            Self::High => crate::i18n_keys::PANE_OBSERVATION_CONFIDENCE_HIGH,
            Self::Authoritative => crate::i18n_keys::PANE_OBSERVATION_CONFIDENCE_AUTHORITATIVE,
        }
    }
}

/// Where an individual pane observation came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneObservationSource {
    /// Workhive's own supervisor metadata for processes it launched.
    Supervisor,
    /// OS-level foreground / process-tree observer (e.g. `ps` on macOS,
    /// `/proc` on Linux).
    ProcessObserver,
    /// tmux `pane_current_command` string field, used as a last-resort
    /// fallback only.
    TmuxCurrentCommand,
    /// The runtime backend's own classification of a pane it hosts.
    ///
    /// Backends that track their own panes can report a structured
    /// classification without exposing a foreground command — Herdr
    /// reports `agent_status` and leaves `command` empty, and exposes no
    /// tty/pid for the OS observer to inspect. This source carries that
    /// claim as a genuine last resort: it is consulted only when no
    /// supervisor / OS-process / command observation produced signal, so
    /// it can never override a live process observation.
    RuntimeBackend,
    /// No observation could be obtained from any source.
    Unknown,
}

impl PaneObservationSource {
    /// Default confidence associated with this source. Adapters may
    /// degrade an individual observation below this (e.g. supervisor
    /// said "process exited but exit status unknown") via
    /// `PaneObservation::confidence`.
    pub fn baseline_confidence(self) -> PaneObservationConfidence {
        match self {
            Self::Supervisor => PaneObservationConfidence::Authoritative,
            Self::ProcessObserver => PaneObservationConfidence::High,
            Self::TmuxCurrentCommand => PaneObservationConfidence::Fallback,
            // The backend owns the pane and derives this from the process
            // it supervises, so it outranks the tmux string heuristic.
            // Ranked with the OS-process observer rather than above it: it
            // is the backend's view of the same foreground process, not a
            // Workhive launch record. Never used to override another
            // source — see the variant docs.
            Self::RuntimeBackend => PaneObservationConfidence::High,
            Self::Unknown => PaneObservationConfidence::None,
        }
    }

    /// i18n key for the user-facing source label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Supervisor => crate::i18n_keys::PANE_OBSERVATION_SOURCE_SUPERVISOR,
            Self::ProcessObserver => crate::i18n_keys::PANE_OBSERVATION_SOURCE_PROCESS_OBSERVER,
            Self::TmuxCurrentCommand => crate::i18n_keys::PANE_OBSERVATION_SOURCE_TMUX,
            Self::RuntimeBackend => crate::i18n_keys::PANE_OBSERVATION_SOURCE_RUNTIME_BACKEND,
            Self::Unknown => crate::i18n_keys::PANE_OBSERVATION_SOURCE_UNKNOWN,
        }
    }
}

/// A single observed command running inside a pane.
///
/// Observers MUST populate at least `basename`; `program_path` is
/// optional and only set when the source produces a real path (e.g.
/// supervisor remembers the exact binary it spawned).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneObservedCommand {
    /// Argv[0] basename of the foreground process. May still be empty
    /// when the pane is genuinely unknown — the merge policy then
    /// produces `PaneKind::Unknown`.
    pub basename: String,
    /// Full path to the program when known. Reserved for supervisor
    /// metadata; OS observers may leave it empty if they cannot
    /// resolve it cheaply.
    pub program_path: Option<String>,
    /// Optional pid that produced this command. Unset when the source
    /// cannot determine pid (e.g. tmux fallback).
    pub pid: Option<u32>,
}

impl PaneObservedCommand {
    /// Build an observation containing only a basename — convenience
    /// for tmux fallback / synthetic test fixtures.
    pub fn from_basename(basename: impl Into<String>) -> Self {
        Self {
            basename: basename.into(),
            program_path: None,
            pid: None,
        }
    }
}

impl fmt::Display for PaneObservedCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.basename)
    }
}

/// Observed process tree depth and basenames.
///
/// `direct_child_basenames` lists, in order, the basenames of the
/// foreground process and any descendants the observer tracked. This
/// helps the merge policy spot cases where a shell launched the agent
/// (e.g. `["zsh", "node", "claude"]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PaneObservedProcess {
    /// Foreground command at the time of observation. `None` means the
    /// observer could not see any foreground command at all, which is
    /// distinct from "the foreground command is `zsh`".
    pub foreground: Option<PaneObservedCommand>,
    /// Best-effort process tree, ordered foreground-first.
    pub process_tree: Vec<PaneObservedCommand>,
}

impl PaneObservedProcess {
    /// Convenience: build a process observation from a single
    /// foreground command.
    pub fn from_foreground(command: PaneObservedCommand) -> Self {
        Self {
            process_tree: vec![command.clone()],
            foreground: Some(command),
        }
    }

    /// `true` iff the observation contains no usable signal.
    pub fn is_empty(&self) -> bool {
        self.foreground.is_none() && self.process_tree.is_empty()
    }
}

/// One observation of a pane from a specific source.
///
/// A `PaneObservation` is intentionally a small leaf type so each
/// adapter (supervisor / OS observer / tmux) can produce one
/// independently and feed it into the merge policy. The merge policy
/// in `usecases::pane_observation` is responsible for deciding the
/// final `PaneRuntimeTruth`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneObservation {
    /// Where the observation came from.
    pub source: PaneObservationSource,
    /// Confidence the source ascribes to this specific observation.
    /// Adapters typically use `source.baseline_confidence()` but may
    /// downgrade per-observation (e.g. supervisor recorded the
    /// process but its OS pid is missing).
    pub confidence: PaneObservationConfidence,
    /// Process information observed for the pane, when available.
    pub process: Option<PaneObservedProcess>,
    /// Optional process state hint provided directly by the source
    /// (e.g. supervisor knows whether the child has exited).
    pub state_hint: Option<PaneState>,
}

impl PaneObservation {
    /// Build an `Unknown` observation — used when an adapter is
    /// available but explicitly produces no information for a pane.
    pub fn unknown() -> Self {
        Self {
            source: PaneObservationSource::Unknown,
            confidence: PaneObservationConfidence::None,
            process: None,
            state_hint: None,
        }
    }

    /// Build a supervisor observation containing a known foreground
    /// command and a state hint.
    pub fn supervisor(command: PaneObservedCommand, state: PaneState) -> Self {
        Self {
            source: PaneObservationSource::Supervisor,
            confidence: PaneObservationConfidence::Authoritative,
            process: Some(PaneObservedProcess::from_foreground(command)),
            state_hint: Some(state),
        }
    }

    /// Build an OS-process observation from a process tree.
    pub fn process_observer(process: PaneObservedProcess) -> Self {
        Self {
            source: PaneObservationSource::ProcessObserver,
            confidence: PaneObservationConfidence::High,
            process: Some(process),
            state_hint: None,
        }
    }

    /// Build a tmux fallback observation from `pane_current_command`.
    pub fn tmux_fallback(basename: impl Into<String>) -> Self {
        let command = PaneObservedCommand::from_basename(basename);
        Self {
            source: PaneObservationSource::TmuxCurrentCommand,
            confidence: PaneObservationConfidence::Fallback,
            process: Some(PaneObservedProcess::from_foreground(command)),
            state_hint: None,
        }
    }

    /// Whether this observation contributed any usable signal.
    pub fn has_signal(&self) -> bool {
        match (&self.process, &self.state_hint) {
            (Some(p), _) => !p.is_empty(),
            (None, Some(_)) => true,
            (None, None) => false,
        }
    }
}

/// The merged runtime truth for a single pane.
///
/// `PaneRuntimeTruth` is what the rest of the system consumes after
/// the merge policy has chosen between supervisor / OS observer /
/// tmux fallback. It is the only place where `kind`/`state` carry the
/// *final* meaning surfaced to the user; all upstream `PaneObservation`
/// values are intermediate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRuntimeTruth {
    /// Final classification surfaced to the user.
    pub kind: PaneKind,
    /// Final state surfaced to the user. `Unknown` when no source
    /// could determine state — never invented.
    pub state: PaneState,
    /// Final foreground command basename, or empty string if no source
    /// could obtain one.
    pub current_command: String,
    /// The source that won the merge for `kind` (used for logs and
    /// diagnostics, never to gate behavior).
    pub source: PaneObservationSource,
    /// Confidence of the chosen source for `kind`.
    pub confidence: PaneObservationConfidence,
    /// Typed user-facing activity classification. Computed by the
    /// merge use case from `(kind, state, source, confidence)`. CLI
    /// rendering MUST consume this rather than re-deriving from
    /// `PaneState`.
    ///
    /// `#[serde(default)]` keeps backwards compatibility: snapshots /
    /// IPC payloads written before this field existed deserialize as
    /// [`PaneActivityState::Unknown`], which is the conservative
    /// default.
    #[serde(default)]
    pub activity: PaneActivityState,
    /// Typed message-safety classification for inter-pane messaging.
    /// `Unknown` when no observation could be obtained — conservative.
    #[serde(default)]
    pub message_safety: PaneMessageSafety,
}

impl PaneRuntimeTruth {
    /// Build an explicit "no information at all" truth value.
    pub fn unknown() -> Self {
        Self {
            kind: PaneKind::Unknown,
            state: PaneState::Unknown,
            current_command: String::new(),
            source: PaneObservationSource::Unknown,
            confidence: PaneObservationConfidence::None,
            activity: PaneActivityState::Unknown,
            message_safety: PaneMessageSafety::Unknown,
        }
    }

    /// Build a truth value from explicit `(kind, state, current_command,
    /// source, confidence)` fields, automatically deriving the typed
    /// `activity` and `message_safety` columns.
    ///
    /// This is the single constructor that callers below the merge
    /// use case (e.g. CLI snapshot-only fallback paths in
    /// `usecases::pane`) should prefer over the struct literal so
    /// activity / safety semantics stay coherent everywhere.
    pub fn from_layered(
        kind: PaneKind,
        state: PaneState,
        current_command: String,
        source: PaneObservationSource,
        confidence: PaneObservationConfidence,
    ) -> Self {
        let activity = derive_activity(kind, state, source, confidence);
        let message_safety = derive_message_safety(kind, state, source, confidence);
        Self {
            kind,
            state,
            current_command,
            source,
            confidence,
            activity,
            message_safety,
        }
    }
}

/// Derive [`PaneActivityState`] from the chosen observation layer.
///
/// Hard rules:
///
/// * A non-shell foreground process is reported as
///   [`PaneActivityState::ForegroundProcess`] — NOT `AgentBusy`. The
///   user must not see a row that suggests the AI is working unless an
///   explicit busy/processing handshake says so.
/// * A supervised AI agent whose recorded pid is alive renders as
///   [`PaneActivityState::AgentForeground`] — never `AgentBusy`. The
///   adapter's "Authoritative + Running" signal only proves the
///   wrapped process is alive; for interactive AI assistants that does
///   not equal "actively processing user work". Until Workhive plumbs
///   a real busy/processing protocol signal, the conservative
///   `AgentForeground` state is the most truthful label.
/// * `AgentBusy` is reserved for a future authoritative busy/processing
///   protocol handshake and is intentionally NOT reachable from
///   process liveness alone.
/// * Supervisor-reported `Idle` for an agent pane is interpreted as
///   `Exited` so the user can tell apart "supervisor used to track
///   this agent and it has now exited" from "the foreground really
///   is a shell".
/// * Shell foregrounds always render as `ShellIdle`.
/// * Empty / unknown signal stays `Unknown`.
pub fn derive_activity(
    kind: PaneKind,
    state: PaneState,
    source: PaneObservationSource,
    confidence: PaneObservationConfidence,
) -> PaneActivityState {
    let supervisor_authoritative = matches!(source, PaneObservationSource::Supervisor)
        && matches!(confidence, PaneObservationConfidence::Authoritative);
    // A runtime backend that names the agent it hosts knows the pane's
    // identity without a busy/processing handshake — the same evidentiary
    // position as the supervisor's live pid. It earns the agent-specific
    // wording instead of degrading to shell-idle text, which would read as
    // "nothing is running here" for a pane that is demonstrably hosting an
    // agent. It still never reaches `AgentBusy`: identity is not a busy
    // signal, exactly as for the supervisor.
    let backend_reports_agent = matches!(source, PaneObservationSource::RuntimeBackend);

    match (kind, state) {
        (PaneKind::Unknown, _) => PaneActivityState::Unknown,
        (PaneKind::Terminal, PaneState::Idle) => PaneActivityState::ShellIdle,
        (PaneKind::Terminal, PaneState::Running) => PaneActivityState::ForegroundProcess,
        (PaneKind::Terminal, PaneState::Unknown) => PaneActivityState::Unknown,
        (PaneKind::GitUi, PaneState::Idle) => PaneActivityState::ShellIdle,
        (PaneKind::GitUi, _) => PaneActivityState::ForegroundProcess,
        (PaneKind::Agent, PaneState::Running) => {
            if supervisor_authoritative || backend_reports_agent {
                // Workhive launched and tracks this agent (live pid), or the
                // backend reports it running, but no busy/processing signal
                // is available today, so we MUST NOT promote to AgentBusy:
                // liveness is not a busy signal. AgentForeground is the
                // conservative, truthful label.
                PaneActivityState::AgentForeground
            } else {
                PaneActivityState::ForegroundProcess
            }
        }
        (PaneKind::Agent, PaneState::Idle) => {
            if supervisor_authoritative {
                // The supervisor's recorded process is gone: the pane fell
                // back to a shell.
                PaneActivityState::Exited
            } else if backend_reports_agent {
                // The backend still reports the agent as present and idle —
                // it is open and waiting for input, not gone.
                PaneActivityState::AgentForeground
            } else {
                PaneActivityState::ShellIdle
            }
        }
        (PaneKind::Agent, PaneState::Unknown) => PaneActivityState::Unknown,
    }
}

/// Derive [`PaneMessageSafety`] from `(kind, state, source, confidence)`.
///
/// Mirrors [`crate::domain::derive_runtime_capabilities`]'s rule: a
/// pane only earns `SafeStructured` when **all four** of the following
/// hold simultaneously:
///
/// 1. Final classification is [`PaneKind::Agent`].
/// 2. Final state is [`PaneState::Running`] — a supervised agent that
///    is `Idle`, `Exited`, or `Unknown` no longer has a live
///    structured-message channel, even if the supervisor previously
///    knew its identity.
/// 3. Source is [`PaneObservationSource::Supervisor`].
/// 4. Confidence is [`PaneObservationConfidence::Authoritative`].
///
/// All other agent observations (supervisor saw the agent leave the
/// foreground, OS observer saw `codex` but no supervisor handshake,
/// tmux fallback) collapse to [`PaneMessageSafety::UnverifiedAgent`]
/// — the CLI/router warns callers and refuses structured delivery.
pub fn derive_message_safety(
    kind: PaneKind,
    state: PaneState,
    source: PaneObservationSource,
    confidence: PaneObservationConfidence,
) -> PaneMessageSafety {
    let supervisor_authoritative = matches!(source, PaneObservationSource::Supervisor)
        && matches!(confidence, PaneObservationConfidence::Authoritative);
    match kind {
        PaneKind::Terminal => PaneMessageSafety::ShellInput,
        PaneKind::GitUi => PaneMessageSafety::NotApplicable,
        PaneKind::Agent => {
            if supervisor_authoritative && matches!(state, PaneState::Running) {
                PaneMessageSafety::SafeStructured
            } else {
                PaneMessageSafety::UnverifiedAgent
            }
        }
        PaneKind::Unknown => PaneMessageSafety::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_ordering_is_highest_to_lowest_when_compared() {
        assert!(PaneObservationConfidence::Authoritative > PaneObservationConfidence::High);
        assert!(PaneObservationConfidence::High > PaneObservationConfidence::Fallback);
        assert!(PaneObservationConfidence::Fallback > PaneObservationConfidence::None);
    }

    #[test]
    fn source_baseline_confidence_matches_architecture_layers() {
        assert_eq!(
            PaneObservationSource::Supervisor.baseline_confidence(),
            PaneObservationConfidence::Authoritative
        );
        assert_eq!(
            PaneObservationSource::ProcessObserver.baseline_confidence(),
            PaneObservationConfidence::High
        );
        assert_eq!(
            PaneObservationSource::TmuxCurrentCommand.baseline_confidence(),
            PaneObservationConfidence::Fallback
        );
        assert_eq!(
            PaneObservationSource::Unknown.baseline_confidence(),
            PaneObservationConfidence::None
        );
    }

    #[test]
    fn observation_unknown_has_no_signal() {
        let obs = PaneObservation::unknown();
        assert!(!obs.has_signal());
        assert_eq!(obs.confidence, PaneObservationConfidence::None);
    }

    #[test]
    fn observation_supervisor_has_signal_and_authoritative_confidence() {
        let obs = PaneObservation::supervisor(
            PaneObservedCommand::from_basename("codex"),
            PaneState::Running,
        );
        assert!(obs.has_signal());
        assert_eq!(obs.confidence, PaneObservationConfidence::Authoritative);
        assert_eq!(obs.state_hint, Some(PaneState::Running));
    }

    #[test]
    fn observation_tmux_fallback_uses_fallback_confidence() {
        let obs = PaneObservation::tmux_fallback("zsh");
        assert_eq!(obs.confidence, PaneObservationConfidence::Fallback);
        assert_eq!(obs.source, PaneObservationSource::TmuxCurrentCommand);
        let process = obs.process.expect("tmux observation has process info");
        assert_eq!(process.foreground.unwrap().basename, "zsh");
    }

    #[test]
    fn observed_process_from_foreground_populates_tree() {
        let proc = PaneObservedProcess::from_foreground(PaneObservedCommand::from_basename("node"));
        assert_eq!(proc.process_tree.len(), 1);
        assert_eq!(proc.foreground.as_ref().unwrap().basename, "node");
    }

    #[test]
    fn runtime_truth_unknown_is_truthful_unknown() {
        let truth = PaneRuntimeTruth::unknown();
        assert_eq!(truth.kind, PaneKind::Unknown);
        assert_eq!(truth.state, PaneState::Unknown);
        assert!(truth.current_command.is_empty());
        assert_eq!(truth.source, PaneObservationSource::Unknown);
    }

    // ── derive_message_safety — state must gate SafeStructured ─────
    //
    // Domain rule:
    // `SafeStructured` is reachable only when an agent pane is
    // simultaneously supervisor-authoritative AND `PaneState::Running`.
    // Idle/Exited/Unknown supervised agents collapse to
    // `UnverifiedAgent`; there is no separate `Exited` variant.
    // Tmux fallback or OS/process observation is not a supervisor
    // handshake and therefore can never satisfy this rule.

    #[test]
    fn message_safety_supervised_running_agent_is_safe_structured() {
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(safety, PaneMessageSafety::SafeStructured);
        assert!(safety.permits_structured_message());
    }

    #[test]
    fn message_safety_supervised_idle_agent_is_unverified_not_safe_structured() {
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Idle,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(
            safety,
            PaneMessageSafety::UnverifiedAgent,
            "supervisor authoritative + Idle agent must not be SafeStructured"
        );
        assert!(!safety.permits_structured_message());
    }

    #[test]
    fn message_safety_supervised_unknown_state_agent_is_unverified() {
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Unknown,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(safety, PaneMessageSafety::UnverifiedAgent);
        assert!(!safety.permits_structured_message());
    }

    #[test]
    fn message_safety_os_observed_running_agent_is_unverified() {
        // OS observer recognised `codex`, but no supervisor handshake.
        // Even though the foreground process is technically running,
        // the control plane has no way to address it safely.
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::ProcessObserver,
            PaneObservationConfidence::High,
        );
        assert_eq!(safety, PaneMessageSafety::UnverifiedAgent);
        assert!(!safety.permits_structured_message());
    }

    #[test]
    fn message_safety_tmux_fallback_running_agent_is_unverified() {
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::TmuxCurrentCommand,
            PaneObservationConfidence::Fallback,
        );
        assert_eq!(safety, PaneMessageSafety::UnverifiedAgent);
        assert!(!safety.permits_structured_message());
    }

    #[test]
    fn message_safety_supervisor_below_authoritative_is_unverified() {
        // Even with Source::Supervisor, a downgraded confidence must
        // not unlock structured messaging.
        let safety = derive_message_safety(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Fallback,
        );
        assert_eq!(safety, PaneMessageSafety::UnverifiedAgent);
    }

    #[test]
    fn message_safety_terminal_is_shell_input_regardless_of_state_or_source() {
        for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
            for source in [
                PaneObservationSource::Supervisor,
                PaneObservationSource::ProcessObserver,
                PaneObservationSource::TmuxCurrentCommand,
                PaneObservationSource::Unknown,
            ] {
                let confidence = source.baseline_confidence();
                assert_eq!(
                    derive_message_safety(PaneKind::Terminal, state, source, confidence),
                    PaneMessageSafety::ShellInput,
                    "terminal pane must always be ShellInput \
                     (state={state:?}, source={source:?})"
                );
            }
        }
    }

    #[test]
    fn message_safety_git_ui_is_not_applicable_regardless_of_state() {
        for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
            assert_eq!(
                derive_message_safety(
                    PaneKind::GitUi,
                    state,
                    PaneObservationSource::ProcessObserver,
                    PaneObservationConfidence::High,
                ),
                PaneMessageSafety::NotApplicable
            );
        }
    }

    #[test]
    fn message_safety_unknown_kind_stays_unknown() {
        assert_eq!(
            derive_message_safety(
                PaneKind::Unknown,
                PaneState::Unknown,
                PaneObservationSource::Unknown,
                PaneObservationConfidence::None,
            ),
            PaneMessageSafety::Unknown
        );
    }

    // ── derive_activity: AgentForeground vs AgentBusy invariants ──
    //
    // Pid liveness alone is NOT a busy/processing signal for
    // interactive AI assistants. Process-name `codex`/`claude` and
    // supervisor "Authoritative + Running" only prove the wrapped
    // process is alive; assistants spend most of their time idle
    // waiting for input. The activity column must therefore stay
    // conservative (AgentForeground / ForegroundProcess) instead of
    // promoting to AgentBusy.

    #[test]
    fn derive_activity_supervisor_authoritative_running_agent_is_agent_foreground() {
        let activity = derive_activity(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(
            activity,
            PaneActivityState::AgentForeground,
            "supervisor authoritative + Running agent must NOT yield \
             AgentBusy; pid liveness is not a busy signal"
        );
        assert!(
            !activity.implies_agent_work(),
            "AgentForeground must not imply AI work"
        );
    }

    #[test]
    fn derive_activity_supervisor_below_authoritative_running_agent_is_foreground_process() {
        let activity = derive_activity(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Fallback,
        );
        assert_eq!(
            activity,
            PaneActivityState::ForegroundProcess,
            "downgraded supervisor confidence (e.g. stale pid) must NOT \
             promote to AgentForeground; the supervisor handshake is no \
             longer authoritative for this pane"
        );
    }

    #[test]
    fn derive_activity_os_observed_running_agent_is_foreground_process() {
        let activity = derive_activity(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::ProcessObserver,
            PaneObservationConfidence::High,
        );
        assert_eq!(activity, PaneActivityState::ForegroundProcess);
    }

    #[test]
    fn derive_activity_tmux_fallback_running_agent_is_foreground_process() {
        let activity = derive_activity(
            PaneKind::Agent,
            PaneState::Running,
            PaneObservationSource::TmuxCurrentCommand,
            PaneObservationConfidence::Fallback,
        );
        assert_eq!(activity, PaneActivityState::ForegroundProcess);
    }

    #[test]
    fn derive_activity_agent_busy_is_unreachable_from_pid_liveness() {
        // Exhaustive search over (state, source) at the source's
        // baseline confidence. AgentBusy must NEVER be emitted today
        // because we have no busy/processing protocol signal.
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
                    "AgentBusy must not be reachable today \
                     (state={state:?}, source={source:?}, confidence={confidence:?})"
                );
            }
        }
    }

    #[test]
    fn from_layered_supervised_running_agent_advertises_agent_foreground_activity() {
        // Regression (pane list previously misreported an idle agent
        // as running):
        // construct the same supervised live agent configuration that
        // previously rendered as AgentBusy and confirm the typed
        // truth column now reports AgentForeground while keeping the
        // (independent) message safety still SafeStructured.
        let truth = PaneRuntimeTruth::from_layered(
            PaneKind::Agent,
            PaneState::Running,
            "claude".to_owned(),
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(truth.activity, PaneActivityState::AgentForeground);
        assert!(!truth.activity.implies_agent_work());
        assert_eq!(truth.message_safety, PaneMessageSafety::SafeStructured);
    }

    #[test]
    fn from_layered_supervised_idle_agent_does_not_advertise_safe_structured() {
        // Regression: this exact configuration
        // (supervisor authoritative observation but pane state already
        // back to Idle) was previously misclassified as SafeStructured.
        let truth = PaneRuntimeTruth::from_layered(
            PaneKind::Agent,
            PaneState::Idle,
            "claude".to_owned(),
            PaneObservationSource::Supervisor,
            PaneObservationConfidence::Authoritative,
        );
        assert_eq!(
            truth.message_safety,
            PaneMessageSafety::UnverifiedAgent,
            "from_layered must propagate state into derive_message_safety"
        );
    }

    #[test]
    fn observation_serde_round_trip_preserves_all_fields() {
        let obs = PaneObservation::supervisor(
            PaneObservedCommand {
                basename: "claude".to_owned(),
                program_path: Some("/usr/local/bin/claude".to_owned()),
                pid: Some(4242),
            },
            PaneState::Running,
        );
        let json = serde_json::to_string(&obs).unwrap();
        let restored: PaneObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(obs, restored);
    }
}

//! Pane classification and lifecycle-state domain model: split direction,
//! role, kind, state, activity, message safety, and launch capture.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Direction for splitting a pane.
///
/// The runtime-neutral direction model. Four orientations are supported so the
/// CLI can map one-to-one onto a typed clap `ValueEnum`.
/// Adapters translate these into their backend-specific flags (e.g. tmux's
/// `-h` / `-v` with optional `-b` for "before target").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneDirection {
    /// New pane is created to the right of the target (horizontal split,
    /// after target).
    Right,
    /// New pane is created below the target (vertical split, after target).
    Down,
    /// New pane is created to the left of the target (horizontal split,
    /// before target).
    Left,
    /// New pane is created above the target (vertical split, before target).
    Up,
}

impl fmt::Display for PaneDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Right => f.write_str("right"),
            Self::Down => f.write_str("down"),
            Self::Left => f.write_str("left"),
            Self::Up => f.write_str("up"),
        }
    }
}

impl PaneDirection {
    /// Whether this direction places the new pane before the target in tmux's
    /// orientation terms (i.e. requires `-b` in `split-window`).
    ///
    /// This predicate stays in core because the "before-or-after" semantics
    /// are part of the neutral direction model; only the tmux-specific flag
    /// name lives in the tmux adapter.
    pub fn is_before_target(self) -> bool {
        matches!(self, Self::Left | Self::Up)
    }

    /// Whether this direction yields a vertical split in tmux terms, i.e. the
    /// new pane occupies a slice above/below the target.
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
}

/// Workspace interaction mode.
///
/// Controls whether panes operate independently (manual) or collaborate
/// through a primary pane that coordinates work across the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceInteractionMode {
    /// Each pane operates independently; no automated coordination.
    #[default]
    Manual,
    /// A primary pane coordinates work across the workspace.
    PrimaryAssisted,
}

/// Role of a pane within a workspace.
///
/// Roles describe the intended function of a pane in the workspace,
/// independent of what tool is running in it. A workspace may have
/// zero or one `Primary` pane, and any number of the other roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PaneRole {
    /// Coordinates work across the workspace in `PrimaryAssisted` mode.
    Primary,
    /// Executes delegated tasks (code generation, testing, etc.).
    Worker,
    /// Provides supporting functionality (file browsing, monitoring, etc.).
    Utility,
    /// Read-only observation (logs, dashboards, etc.).
    Viewer,
}

/// Best-effort classification of what is running in a pane.
///
/// Determined lazily by inspecting `pane_current_command` from tmux.
/// Business logic MUST NOT hard-depend on this classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    /// An AI coding agent (e.g., cursor, claude).
    Agent,
    /// A regular terminal shell.
    Terminal,
    /// A git UI tool (e.g., lazygit, tig).
    GitUi,
    /// Could not determine the pane type.
    #[default]
    Unknown,
}

/// Observable state of a pane, read on demand from tmux.
///
/// **Caveat — do NOT consume `PaneState::Running` as "AI is busy".**
/// `Running` only means the runtime adapter saw *some* foreground
/// process that wasn't a recognised shell. It does NOT mean an AI
/// agent is processing a request, that the pane is safe to receive a
/// structured `pane send`, or that the pane is "available". For
/// user-facing activity / safety semantics, prefer
/// [`PaneActivityState`] and [`PaneMessageSafety`] derived in
/// `usecases::pane_observation::merge_pane_observations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneState {
    /// Pane shell is idle (no foreground process).
    Idle,
    /// A process is actively running.
    Running,
    /// Could not determine the pane state.
    #[default]
    Unknown,
}

/// User-facing pane activity classification.
///
/// `PaneActivityState` is the typed value `pane list` / `pane show`
/// surface to the human user. It is computed at observation-merge time
/// from the layered observation stack (supervisor → OS observer →
/// tmux fallback) and explicitly separates four operationally
/// different situations:
///
/// * "the pane has a foreground process that isn't a shell"
///   ([`Self::ForegroundProcess`]),
/// * "the foreground process is recognised as an AI agent and Workhive's
///   supervisor identifies it, but we have no live busy/processing
///   signal" ([`Self::AgentForeground`]),
/// * "Workhive's supervisor confirms an AI agent is actively
///   processing a request via an explicit busy/processing protocol
///   signal" ([`Self::AgentBusy`]).
///
/// The motivation is operational: rendering `Running` for every pane
/// whose foreground command happens to be `codex` made users believe
/// the agent was actively working when in fact it had just been
/// launched and was already idle waiting on input. Worse, Workhive's
/// supervisor metadata only proves "the wrapped agent process pid is
/// alive" — that says nothing about whether the agent is currently
/// processing work. `pane list` must never imply AI activity unless
/// an explicit, reliable busy/processing handshake says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneActivityState {
    /// No observation could be obtained, or the observation has no
    /// signal. Conservative; never rendered as "running" / "active".
    #[default]
    Unknown,
    /// A recognised shell is the foreground process. The pane is at
    /// the prompt and waiting for input.
    ShellIdle,
    /// A non-shell foreground process is observed (e.g. a build, an
    /// editor, a TUI). Conservative phrasing on display: the pane is
    /// *not* idle, but we cannot prove it's actively processing
    /// anything — only that *something* is in the foreground.
    ForegroundProcess,
    /// Workhive's supervisor identified an AI agent in the pane's
    /// foreground (Workhive launched it, the recorded pid is alive),
    /// but no authoritative busy/processing signal exists. Process
    /// liveness alone is NOT proof that the agent is actively working
    /// — interactive AI assistants spend most of their time idle
    /// waiting for input. Display copy must avoid wording such as
    /// "running" / "busy" / "active"; instead it should communicate
    /// "AI assistant is open in this pane, busy state unverified".
    AgentForeground,
    /// Workhive's supervisor has confirmed an AI agent is currently
    /// processing a request through an explicit, authoritative
    /// busy/processing protocol handshake. This variant is reserved
    /// for that future protocol signal and is intentionally NOT
    /// reachable from process liveness alone. Until such a signal is
    /// implemented, supervised live agents render as
    /// [`Self::AgentForeground`].
    AgentBusy,
    /// Workhive's supervisor reported that the previously-tracked
    /// process exited and the pane has fallen back to the shell. We
    /// surface this distinctly from [`Self::ShellIdle`] only when the
    /// supervisor is the chosen layer; OS observer / tmux fallback
    /// cannot tell us whether the previous process exited cleanly.
    Exited,
}

impl PaneActivityState {
    /// i18n key for the user-facing activity label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Unknown => crate::i18n_keys::PANE_ACTIVITY_UNKNOWN,
            Self::ShellIdle => crate::i18n_keys::PANE_ACTIVITY_SHELL_IDLE,
            Self::ForegroundProcess => crate::i18n_keys::PANE_ACTIVITY_FOREGROUND_PROCESS,
            Self::AgentForeground => crate::i18n_keys::PANE_ACTIVITY_AGENT_FOREGROUND,
            Self::AgentBusy => crate::i18n_keys::PANE_ACTIVITY_AGENT_BUSY,
            Self::Exited => crate::i18n_keys::PANE_ACTIVITY_EXITED,
        }
    }

    /// Whether this activity state indicates AI / agent work in
    /// progress. Only `AgentBusy` qualifies — `AgentForeground`
    /// deliberately does NOT, because process liveness is not proof of
    /// active work. Every other variant is also false.
    pub fn implies_agent_work(self) -> bool {
        matches!(self, Self::AgentBusy)
    }
}

/// Typed safety classification for `pane send` delivery.
///
/// `PaneMessageSafety` distinguishes the four operationally-different
/// situations a user can encounter when picking a pane to send input
/// to. It is the typed enum that downstream code (CLI render, future
/// scheduler) consumes instead of guessing from `PaneKind` alone.
///
/// Layering rules:
///
/// * `NotApplicable` — the pane runs a TUI (`PaneKind::GitUi`), so
///   sending structured text would corrupt the TUI's screen state.
///   `pane send` may still deliver a literal keystroke string, but
///   inter-pane structured messaging must refuse to route here.
/// * `ShellInput` — the pane is a recognised shell. Free-form command
///   text is fine; structured agent messages are NOT, since a shell
///   is not an agent message receiver.
/// * `UnverifiedAgent` — the foreground command basename matches an
///   AI agent (e.g. `codex`/`claude`) but the chosen observation
///   layer was NOT the Workhive supervisor. No protocol/handshake is
///   confirmed, so the pane cannot be promoted to a safe structured
///   receiver yet — the user could have manually launched the agent
///   in a Workhive-managed pane, or the supervisor could have lost
///   track of it. The CLI surfaces this distinctly so callers know
///   *why* the pane isn't a `SafeStructured` receiver.
/// * `SafeStructured` — supervisor `Authoritative` observation
///   confirms the pane is owned by Workhive and runs an AI agent
///   that completed the supervisor handshake. This is the only state
///   from which inter-pane scheduling can route structured messages.
/// * `Unknown` — no signal at all; conservative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneMessageSafety {
    /// No observation could be obtained — must default to "do not
    /// route". Conservative.
    #[default]
    Unknown,
    /// Pane runs a full-screen TUI; structured messaging is not a
    /// supported delivery target.
    NotApplicable,
    /// Pane is a shell. `pane send` of free-form text is fine, but
    /// structured agent messaging is not.
    ShellInput,
    /// Foreground looks like an AI agent but supervisor handshake is
    /// not confirmed. CLI displays this as a warning.
    UnverifiedAgent,
    /// Supervisor handshake confirmed; pane can safely receive
    /// structured messages.
    SafeStructured,
}

impl PaneMessageSafety {
    /// i18n key for the user-facing safety label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Unknown => crate::i18n_keys::PANE_MESSAGE_SAFETY_UNKNOWN,
            Self::NotApplicable => crate::i18n_keys::PANE_MESSAGE_SAFETY_NOT_APPLICABLE,
            Self::ShellInput => crate::i18n_keys::PANE_MESSAGE_SAFETY_SHELL_INPUT,
            Self::UnverifiedAgent => crate::i18n_keys::PANE_MESSAGE_SAFETY_UNVERIFIED_AGENT,
            Self::SafeStructured => crate::i18n_keys::PANE_MESSAGE_SAFETY_SAFE_STRUCTURED,
        }
    }

    /// Whether structured inter-pane messaging may target this pane.
    /// Only `SafeStructured` qualifies.
    pub fn permits_structured_message(self) -> bool {
        matches!(self, Self::SafeStructured)
    }
}

/// Coarse pane lifecycle: does the pane currently exist, is it gone,
/// or has its previously-tracked foreground process exited?
///
/// `PaneLifecycle` is a **typed pane-level lifecycle indicator**
/// orthogonal to [`ActivityState`] and [`InputCapability`].
/// `ActivityState` describes "what kind of foreground is running",
/// `InputCapability` describes "who may send input to this pane",
/// while `PaneLifecycle` answers "does this pane still exist at all,
/// and if not, why".
///
/// Variants:
///
/// * [`Self::Open`] — the pane is currently registered with the
///   runtime backend.
/// * [`Self::Exited`] — Workhive's supervisor previously tracked the
///   pane's foreground process and observed it exit. The pane shell
///   may still be present but the tracked program has stopped.
/// * [`Self::Missing`] — the pane was expected but cannot be found
///   in the runtime backend (e.g. session was killed externally).
/// * [`Self::Unknown`] — no observation could be obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneLifecycle {
    /// No observation could be obtained.
    #[default]
    Unknown,
    /// Pane is present and registered with the runtime backend.
    Open,
    /// Tracked foreground program exited.
    Exited,
    /// Pane is no longer registered with the runtime backend.
    Missing,
}

impl PaneLifecycle {
    /// Derive lifecycle from the typed observation columns.
    ///
    /// The supervisor-authoritative `Exited` activity collapses to
    /// [`Self::Exited`]. An unknown observation collapses to
    /// [`Self::Unknown`]. Every other observation is treated as
    /// [`Self::Open`] because the runtime backend confirmed the pane
    /// is currently present.
    pub fn from_observation(activity: PaneActivityState) -> Self {
        match activity {
            PaneActivityState::Exited => Self::Exited,
            PaneActivityState::Unknown => Self::Unknown,
            _ => Self::Open,
        }
    }
}

/// Type alias surfacing the actor-kind concept under the typed pane
/// state-model vocabulary.
///
/// `ActorKind` describes "who is running inside this pane" — a shell,
/// an AI agent, a TUI, or unknown — and is the typed input to every
/// downstream classifier (activity, lifecycle, input capability).
/// Aliasing [`PaneKind`] keeps the existing observation pipeline as
/// the single source of truth and avoids duplicating runtime
/// classification logic.
pub type ActorKind = PaneKind;

/// Type alias for the user-facing activity classification used by the
/// pane state-model vocabulary.
///
/// Carries the same semantic content as [`PaneActivityState`]; the
/// alias exists so callers reasoning in the typed state-model
/// vocabulary (`PaneLifecycle` / `ActivityState` / `InputCapability`)
/// can reference a single coherent set of names.
pub type ActivityState = PaneActivityState;

/// Type alias for the typed input-routing classification used by the
/// pane state-model vocabulary.
///
/// `InputCapability` is the typed answer to "may this pane receive
/// input, and of what kind?". It aliases [`PaneMessageSafety`] so the
/// existing routing logic (`derive_message_safety` in
/// [`crate::domain::observation`], `resolve_pane_send_plan_guarded`
/// in [`crate::usecases::pane_send`]) stays the single source of
/// truth. `pane send` planners consume this typed value directly and
/// MUST NOT make routing decisions from rendered display strings.
pub type InputCapability = PaneMessageSafety;

impl PaneMessageSafety {
    /// Whether this capability accepts inter-pane / structured
    /// messages routed by Workhive's scheduler. Mirrors
    /// [`Self::permits_structured_message`] under the typed
    /// `InputCapability` vocabulary so policy callers can read the
    /// intent without re-deriving from variant names.
    pub fn accepts_structured_input(self) -> bool {
        self.permits_structured_message()
    }

    /// Whether `pane send` is permitted to deliver free-form text to
    /// this pane. Shell input panes always accept; verified agents
    /// also accept (the scheduler still chooses the wire format).
    /// TUI / unknown panes do not.
    pub fn accepts_free_form_input(self) -> bool {
        matches!(self, Self::ShellInput | Self::SafeStructured)
    }
}

/// Concise pane activity surfaced by `pane list`.
///
/// `pane list` MUST render exactly one of the five labels below per
/// row. Keeping this projection concise prevents long
/// parenthesized activity labels (e.g. `已打开（等待信号）`,
/// `AI 助手已打开（活动状态未确认）`). `pane show` keeps the full
/// detailed activity, source, confidence, and AI overlay so the
/// concise label never erases diagnostic information.
///
/// Variants intentionally read as user-facing status, not as raw
/// process state:
///
/// * [`Self::WaitingInput`] — the pane is ready to receive user
///   input. Covers shells at the prompt, supervised AI assistants
///   sitting open (PID alive, no busy signal), and interactive TUIs
///   such as `lazygit` that wait for keystrokes. A non-shell
///   foreground process is never permanently rendered as
///   "running": without an explicit busy/processing handshake the
///   concise activity collapses to [`Self::WaitingInput`].
/// * [`Self::Running`] — an authoritative busy/processing signal is
///   in effect (AI provider hook/plugin event of state `processing`
///   or `tool_running`, or a typed `AgentBusy` from the supervisor
///   stack). The pane is actively doing work on the user's behalf.
/// * [`Self::Completed`] — an authoritative event reports the
///   foreground turn completed and is waiting for user follow-up
///   (e.g. AI provider `waiting_for_user`). The pane is reachable
///   for further input.
/// * [`Self::Exited`] — the previously-tracked foreground program
///   exited. The pane may still be visible but is no longer working.
/// * [`Self::Unknown`] — no usable signal. Conservative default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneConciseActivity {
    /// No usable signal.
    #[default]
    Unknown,
    /// Pane is ready to receive input.
    WaitingInput,
    /// Pane is actively doing work.
    Running,
    /// Authoritative completion signal; pane awaits follow-up.
    Completed,
    /// Foreground program exited.
    Exited,
}

impl PaneConciseActivity {
    /// i18n key for the user-facing concise activity label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Unknown => crate::i18n_keys::PANE_CONCISE_ACTIVITY_UNKNOWN,
            Self::WaitingInput => crate::i18n_keys::PANE_CONCISE_ACTIVITY_WAITING_INPUT,
            Self::Running => crate::i18n_keys::PANE_CONCISE_ACTIVITY_RUNNING,
            Self::Completed => crate::i18n_keys::PANE_CONCISE_ACTIVITY_COMPLETED,
            Self::Exited => crate::i18n_keys::PANE_CONCISE_ACTIVITY_EXITED,
        }
    }

    /// Whether this concise activity indicates the pane can receive
    /// new user / scheduler input right now.
    ///
    /// Both [`Self::WaitingInput`] and [`Self::Completed`] qualify:
    /// the pane is open, the foreground is reachable, and a new
    /// prompt / command can be delivered.
    pub fn can_receive_input(self) -> bool {
        matches!(self, Self::WaitingInput | Self::Completed)
    }
}

/// Collapse the typed pane observation + optional AI activity state
/// into the five concise activity labels rendered by `pane list`.
///
/// Rules:
///
/// * If an AI activity state is present (i.e. the row's provider is
///   recognized and the AI feature gate allows reading it), the AI
///   state is authoritative:
///   - `Processing` / `ToolRunning` / `WaitingForPermission` map to
///     [`PaneConciseActivity::Running`] because the agent is mid-turn.
///   - `WaitingForUser` / `Error` map to
///     [`PaneConciseActivity::Completed`] — the turn is closed and a
///     new prompt can be delivered.
///   - `OpenUnverified` and `Unknown` map to
///     [`PaneConciseActivity::WaitingInput`] (pane reachable, no
///     active-work evidence).
///
/// * Without an AI state, the supervisor observation is used:
///   - `ShellIdle` and `ForegroundProcess` map to
///     [`PaneConciseActivity::WaitingInput`]. Interactive helpers
///     (`lazygit`, `htop`, …) are never permanently "Running" just
///     because a foreground process exists.
///   - `AgentForeground` maps to
///     [`PaneConciseActivity::WaitingInput`] (no fresh AI evidence).
///   - `AgentBusy` maps to [`PaneConciseActivity::Running`].
///   - `Exited` maps to [`PaneConciseActivity::Exited`].
///   - `Unknown` and any other underspecified state map to
///     [`PaneConciseActivity::Unknown`].
///
/// The detailed signal (source, confidence, reason, AI provider) is
/// preserved for `pane show`; this resolver only drives the concise
/// label.
pub fn derive_concise_activity(
    activity: PaneActivityState,
    ai_state: Option<crate::domain::ai_activity::AiActivityState>,
) -> PaneConciseActivity {
    use crate::domain::ai_activity::AiActivityState as Ai;
    if let Some(ai) = ai_state {
        return match ai {
            Ai::Processing | Ai::ToolRunning | Ai::WaitingForPermission => {
                PaneConciseActivity::Running
            }
            Ai::WaitingForUser | Ai::Error => PaneConciseActivity::Completed,
            // `OpenUnverified` means a provider process is open and we
            // have no contradictory busy signal — the pane is reachable
            // and idle, so it accepts input.
            Ai::OpenUnverified => PaneConciseActivity::WaitingInput,
            // `Unknown` means the provider channel produced no usable
            // state at all. Surfacing it as the explicit unknown bucket
            // keeps the row honest instead of pretending it is idle.
            Ai::Unknown => PaneConciseActivity::Unknown,
        };
    }
    match activity {
        PaneActivityState::ShellIdle
        | PaneActivityState::ForegroundProcess
        | PaneActivityState::AgentForeground => PaneConciseActivity::WaitingInput,
        PaneActivityState::AgentBusy => PaneConciseActivity::Running,
        PaneActivityState::Exited => PaneConciseActivity::Exited,
        PaneActivityState::Unknown => PaneConciseActivity::Unknown,
    }
}

/// How a pane was launched.
///
/// This is a type alias for [`crate::domain::tool::PaneLaunchRef`], providing
/// the name required by the Phase 6 pane registry specification.
/// The semantic identity is the same: it describes whether a pane was
/// created as an empty shell, launched via a tool catalog entry, or
/// launched with an inline command.
pub type PaneLaunchOrigin = crate::domain::tool::PaneLaunchRef;

/// Captures how a pane/unit was originally launched for snapshot persistence.
///
/// Used by the snapshot system to record exactly what was running in each
/// pane, enabling faithful restoration. Variants cover tool-catalog
/// references, direct executables, interactive shells, and non-restorable
/// processes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LaunchCapture {
    /// Launched via a tool catalog entry (e.g., `lazygit`, `claude`).
    ToolRef { tool_name: String },
    /// Launched as a direct executable with arguments.
    Exec { program: String, args: Vec<String> },
    /// An interactive shell session.
    Shell {
        executable: String,
        args: Vec<String>,
    },
    /// A process that cannot be faithfully restored (e.g., SSH sessions).
    NonRestorable { display: String, reason: String },
}

impl LaunchCapture {
    /// The working directory to restore for this launch capture.
    pub fn default_cwd(&self) -> Option<&str> {
        None
    }

    /// Whether this capture represents a restorable process.
    pub fn is_restorable(&self) -> bool {
        !matches!(self, Self::NonRestorable { .. })
    }
}

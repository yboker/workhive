//! AI assistant activity domain model.
//!
//! ## Why this module is separate from `domain::pane`
//!
//! `domain::pane::PaneActivityState` already exists for the *process-level*
//! activity classification used by the supervisor / OS observer / tmux fallback
//! stack (e.g. `ShellIdle`, `AgentForeground`, `AgentBusy`). Those values
//! describe what we can prove from process liveness alone.
//!
//! Workhive keeps a *separate* AI-assistant activity model sourced from
//! real provider lifecycle events (Claude Code hooks, Codex hooks, OpenCode
//! plugin events). The two models intentionally do not collapse:
//!
//! - process facts answer "is something foreground here?";
//! - AI activity facts answer "what did the tool's last lifecycle event say?".
//!
//! Mixing them into one enum would make the merge policy untestable. The
//! merge rules below combine them at the rendering boundary instead.

use serde::{Deserialize, Serialize};

use crate::i18n_keys;

/// Stable schema version for serialized [`AiActivityFact`] records.
///
/// Stored on disk under
/// `<project>/.workhive/runtime/<group>/<workspace>/activity/<unit_id>.json`.
/// Bump only when on-disk shape changes in a way the reader cannot
/// transparently tolerate.
pub const AI_ACTIVITY_SCHEMA_VERSION: u32 = 1;

/// Default TTL (in milliseconds) applied to `processing`, `tool_running`,
/// and `waiting_for_permission` facts when the provider did not supply a
/// custom `expires_at_unix_ms`.
pub const AI_ACTIVITY_DEFAULT_BUSY_TTL_MS: i64 = 90_000;

/// AI activity provider known to Workhive.
///
/// The enum is the authoritative routing key. Hook scripts and plugins
/// MUST identify themselves via [`AiActivityProvider::wire_name`] so the
/// runtime store can refuse to attribute events to the wrong provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActivityProvider {
    /// Claude Code (Anthropic). Source: <https://code.claude.com/docs/en/hooks>.
    ClaudeCode,
    /// Codex (OpenAI). Source: <https://developers.openai.com/codex/hooks>.
    Codex,
    /// OpenCode. Source: <https://opencode.ai/docs/plugins/>.
    Opencode,
    /// Sentinel for "no provider attached". Used when the user has not
    /// installed any integration, the foreground tool is not one of the
    /// supported AI assistants, or the integration was explicitly removed.
    None,
}

impl AiActivityProvider {
    /// Stable wire name used in hook scripts, environment variables
    /// (`WORKHIVE_TOOL_PROVIDER`), and on-disk JSON. The catalog file
    /// must use exactly these strings.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::None => "none",
        }
    }

    /// Parse a provider wire name. Returns `None` for an unrecognised
    /// value rather than silently mapping it to a provider — the
    /// internal recorder uses the `None` branch to refuse hostile or
    /// malformed payloads.
    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "claude_code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::Opencode),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// i18n key for the localized provider label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::ClaudeCode => i18n_keys::PANE_AI_PROVIDER_CLAUDE_CODE,
            Self::Codex => i18n_keys::PANE_AI_PROVIDER_CODEX,
            Self::Opencode => i18n_keys::PANE_AI_PROVIDER_OPENCODE,
            Self::None => i18n_keys::PANE_AI_PROVIDER_NONE,
        }
    }

    /// Map a tool command basename (e.g. `claude`, `codex`, `opencode`) to
    /// its activity provider. Returns [`AiActivityProvider::None`] for any
    /// other basename — process-only panes remain activity-unverified
    /// until a provider event arrives, per the active requirement's
    /// "process facts and activity facts are separate" rule.
    ///
    /// The mapping is intentionally strict (exact basename match):
    /// alternative entrypoints like wrapper scripts must still pass the
    /// canonical tool name through `WORKHIVE_TOOL_NAME` if they expect
    /// activity-provider attribution.
    pub fn from_tool_basename(basename: &str) -> Self {
        match basename {
            "claude" => Self::ClaudeCode,
            "codex" => Self::Codex,
            "opencode" => Self::Opencode,
            _ => Self::None,
        }
    }

    /// Pure, exact-match provider resolver shared by `pane list`
    /// rendering and the no-hook-event hint.
    ///
    /// Resolution order:
    ///   1. If `hint` is non-`None`, return it (the activity store's
    ///      typed attribution wins — it came from a real provider
    ///      event).
    ///   2. Otherwise, try the strict canonical basename mapping
    ///      ([`Self::from_tool_basename`]) for `claude` / `codex` /
    ///      `opencode`.
    ///   3. Otherwise, consult the tool catalog by exact process-name
    ///      match. Architecture-suffixed Codex binaries (e.g.
    ///      `codex-x86_64-apple-darwin`) are modeled there as
    ///      `ToolEntry.process_names` for the canonical `codex`
    ///      entry, so this branch attributes them to
    ///      [`AiActivityProvider::Codex`] without leaking
    ///      OS-specific binary names into renderers.
    ///   4. Otherwise, return `None`.
    ///
    /// Matching is **exact** at every step. No substring or
    /// `contains()` heuristic is used; lookalike names such as
    /// `codex-extra`, `codex-x86_64-apple-darwin-extra`, or `mycodex`
    /// resolve to `None`.
    pub fn resolve_for_pane(
        hint: Self,
        command_basename: &str,
        catalog: &crate::domain::ToolCatalog,
    ) -> Self {
        if !matches!(hint, Self::None) {
            return hint;
        }
        let strict = Self::from_tool_basename(command_basename);
        if !matches!(strict, Self::None) {
            return strict;
        }
        match catalog.match_process_name(command_basename) {
            Some(entry) => Self::from_tool_basename(entry.name.as_str()),
            None => Self::None,
        }
    }
}

/// Strength of the signal that produced an activity fact.
///
/// Confidence is independent of the activity state itself: a `processing`
/// fact from a provider hook is strictly stronger than the same state
/// guessed from a heuristic, even when the rendered label is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActivityConfidence {
    /// Backed by an official tool lifecycle event delivered through the
    /// installed hook or plugin. Strongest signal.
    Hook,
    /// Plugin-emitted event for in-process plugin runtimes (OpenCode).
    /// Treated as equivalent to a hook for merge purposes.
    Plugin,
    /// Best-effort signal derived from incomplete information (e.g. a
    /// session created event without a follow-up). Conservative.
    Heuristic,
    /// Last-resort signal; never used to claim `processing` /
    /// `tool_running`. Present only so adapters can be explicit when
    /// they have nothing better.
    Fallback,
}

impl AiActivityConfidence {
    /// i18n key for the localized confidence label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Hook => i18n_keys::PANE_AI_ACTIVITY_CONFIDENCE_HOOK,
            Self::Plugin => i18n_keys::PANE_AI_ACTIVITY_CONFIDENCE_PLUGIN,
            Self::Heuristic => i18n_keys::PANE_AI_ACTIVITY_CONFIDENCE_HEURISTIC,
            Self::Fallback => i18n_keys::PANE_AI_ACTIVITY_CONFIDENCE_FALLBACK,
        }
    }
}

/// AI activity state machine.
///
/// The seven variants are pinned by the active requirement
/// (`Product Decisions / The binding activity state model is`) and
/// MUST NOT silently merge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AiActivityState {
    /// No trustworthy process or activity signal — the conservative
    /// default. `pane list` never claims AI activity here.
    #[default]
    Unknown,
    /// An AI process is open in the foreground but no fresh provider
    /// event proves what it is currently doing.
    OpenUnverified,
    /// The tool accepted a user prompt and is working on it.
    Processing,
    /// The tool is executing a sub-tool or shell command on the user's
    /// behalf.
    ToolRunning,
    /// The tool is blocked on the user approving a permission/consent
    /// prompt (e.g. file write, command execution).
    WaitingForPermission,
    /// The tool has stopped/finished and is waiting for the user to
    /// respond.
    WaitingForUser,
    /// The provider reported a session/turn/tool failure.
    Error,
}

impl AiActivityState {
    /// i18n key for the localized activity label.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Unknown => i18n_keys::PANE_AI_ACTIVITY_UNKNOWN,
            Self::OpenUnverified => i18n_keys::PANE_AI_ACTIVITY_OPEN_UNVERIFIED,
            Self::Processing => i18n_keys::PANE_AI_ACTIVITY_PROCESSING,
            Self::ToolRunning => i18n_keys::PANE_AI_ACTIVITY_TOOL_RUNNING,
            Self::WaitingForPermission => i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_PERMISSION,
            Self::WaitingForUser => i18n_keys::PANE_AI_ACTIVITY_WAITING_FOR_USER,
            Self::Error => i18n_keys::PANE_AI_ACTIVITY_ERROR,
        }
    }

    /// Whether this state is one of the time-limited "busy" variants
    /// that must downgrade to `OpenUnverified` after the default TTL
    /// when no newer event arrives.
    pub fn is_time_limited_busy(self) -> bool {
        matches!(
            self,
            Self::Processing | Self::ToolRunning | Self::WaitingForPermission
        )
    }

    /// Whether this state implies the AI tool is currently doing work.
    /// Used by callers that want a single "busy" predicate without
    /// caring which busy variant it is.
    pub fn implies_work(self) -> bool {
        matches!(self, Self::Processing | Self::ToolRunning)
    }
}

/// On-disk activity fact recorded by a provider hook/plugin.
///
/// The fact is the durable record of "the last meaningful thing the
/// provider said about this pane". Every field is required so the
/// reader can validate without consulting external context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiActivityFact {
    /// Schema version of the on-disk encoding. Must equal
    /// [`AI_ACTIVITY_SCHEMA_VERSION`] for the reader to accept the fact.
    pub schema_version: u32,
    /// Provider that produced this fact.
    pub provider: AiActivityProvider,
    /// Foreground tool name as the provider knows it (e.g. `bash`,
    /// `edit_file`). May differ from `provider` because a single
    /// provider can drive multiple sub-tools.
    pub tool_name: String,
    /// Runtime isolation group (the tmux server label).
    pub group: String,
    /// Workspace name.
    pub workspace: String,
    /// Logical unit identifier within the workspace.
    pub unit_id: String,
    /// Current activity state.
    pub state: AiActivityState,
    /// Confidence assigned by the provider adapter.
    pub confidence: AiActivityConfidence,
    /// Wall-clock timestamp at which the provider observed the event,
    /// in Unix milliseconds.
    pub observed_at_unix_ms: i64,
    /// Wall-clock timestamp after which the fact MUST be downgraded to
    /// `OpenUnverified`. For non-time-limited states this is the
    /// observation time plus a long horizon, not a hard expiry —
    /// `merge_displayed_activity` decides the actual policy.
    pub expires_at_unix_ms: i64,
    /// Optional, display-safe reason ("`bash` (sub-tool)", "edit file
    /// approval required"). MUST NOT contain raw user data or
    /// credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl AiActivityFact {
    /// Construct a fact from individual fields, defaulting the schema
    /// version and TTL. `now_ms` is the wall-clock observation time
    /// in Unix milliseconds.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: AiActivityProvider,
        tool_name: impl Into<String>,
        group: impl Into<String>,
        workspace: impl Into<String>,
        unit_id: impl Into<String>,
        state: AiActivityState,
        confidence: AiActivityConfidence,
        now_ms: i64,
        reason: Option<String>,
    ) -> Self {
        let expires_at_unix_ms = if state.is_time_limited_busy() {
            now_ms.saturating_add(AI_ACTIVITY_DEFAULT_BUSY_TTL_MS)
        } else {
            // Non-time-limited states: pick a sentinel far enough in
            // the future that `merge_displayed_activity` decides what
            // to do, not a wall-clock cutoff.
            i64::MAX
        };
        Self {
            schema_version: AI_ACTIVITY_SCHEMA_VERSION,
            provider,
            tool_name: tool_name.into(),
            group: group.into(),
            workspace: workspace.into(),
            unit_id: unit_id.into(),
            state,
            confidence,
            observed_at_unix_ms: now_ms,
            expires_at_unix_ms,
            reason,
        }
    }
}

/// Display-time projection of an [`AiActivityFact`] after applying the
/// TTL / staleness / foreground-mismatch policy.
///
/// Returned by [`merge_displayed_activity`]. Rendered by `pane list` and
/// `pane show`. `stale` is `true` when the underlying fact was time-limited
/// but the busy TTL has elapsed (so the display falls back to
/// `OpenUnverified` and the renderer can grey out the line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayedAiActivity {
    pub provider: AiActivityProvider,
    pub state: AiActivityState,
    pub reason: Option<String>,
    pub stale: bool,
    pub confidence: AiActivityConfidence,
}

impl DisplayedAiActivity {
    /// Conservative "no provider attached" display used when the
    /// foreground process is not a recognised AI assistant. Renders as
    /// `unknown / none / no fact`.
    pub fn unknown_none() -> Self {
        Self {
            provider: AiActivityProvider::None,
            state: AiActivityState::Unknown,
            reason: None,
            stale: false,
            confidence: AiActivityConfidence::Fallback,
        }
    }

    /// Conservative "process is open but we have no fresh fact" display
    /// used when the foreground process is a recognised AI assistant
    /// but no provider fact exists for the pane (or it expired and was
    /// downgraded).
    pub fn open_unverified(provider: AiActivityProvider) -> Self {
        Self {
            provider,
            state: AiActivityState::OpenUnverified,
            reason: None,
            stale: false,
            confidence: AiActivityConfidence::Fallback,
        }
    }
}

/// Productized "activity source" surfaced to users in `pane show`
/// and as a legend under `pane list`.
///
/// Fallback / tmux / process / supervisor states must never be
/// displayed as hook-confirmed AI activity. This enum is the
/// *display* projection of the underlying provider fact + observation
/// tuple, and is independent of
/// [`AiActivityConfidence`] (which is the recorder's own assessment
/// of strength).
///
/// The variants are ordered from strongest to weakest accuracy. The
/// mapping is purely typed — no string substring matching — and is
/// stable across `pane show` and `pane list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiActivitySource {
    /// A fresh Claude Code hook event drove the display. Highest
    /// accuracy; the tool reported this state itself via the
    /// official hooks API.
    ClaudeCodeHook,
    /// A fresh Codex hook event drove the display. Highest accuracy.
    CodexHook,
    /// A fresh OpenCode plugin event drove the display. Highest
    /// accuracy.
    OpencodePlugin,
    /// A conservative status parsed from the visible terminal UI.
    /// Medium accuracy: useful when hooks/plugins are missing, pending
    /// review, unavailable, or stale, but never equivalent to provider
    /// lifecycle events.
    TerminalStatus,
    /// Workhive's pane supervisor confirmed the foreground binary
    /// (e.g. `claude` / `codex` / `opencode`) but no hook/plugin
    /// event is in the store. Medium accuracy: we know an AI tool
    /// is open, we do not know what it is currently doing.
    WorkhiveSupervisor,
    /// The OS process observer reported the foreground binary in
    /// the absence of a supervisor. Medium accuracy.
    ProcessObserver,
    /// Tmux's own `pane_current_command` was the only signal
    /// available — supervisor and process observer both unavailable.
    /// Low accuracy.
    TmuxFallback,
    /// The runtime backend reported the agent it hosts (`Herdr`'s
    /// `agent` / `agent_status`) with no hook, plugin, or supervisor
    /// signal. Medium accuracy: the backend owns the pane and names the
    /// agent, so we know an AI tool is open, but it carries no
    /// busy/processing handshake for what that tool is doing.
    RuntimeBackend,
    /// No usable signal. Rendered as the conservative "none".
    None,
}

impl AiActivitySource {
    /// i18n key for the localized source label.
    pub fn i18n_key(self) -> &'static str {
        use crate::i18n_keys as k;
        match self {
            Self::ClaudeCodeHook => k::PANE_AI_ACTIVITY_SOURCE_CLAUDE_CODE_HOOK,
            Self::CodexHook => k::PANE_AI_ACTIVITY_SOURCE_CODEX_HOOK,
            Self::OpencodePlugin => k::PANE_AI_ACTIVITY_SOURCE_OPENCODE_PLUGIN,
            Self::TerminalStatus => k::PANE_AI_ACTIVITY_SOURCE_TERMINAL_STATUS,
            Self::WorkhiveSupervisor => k::PANE_AI_ACTIVITY_SOURCE_SUPERVISOR,
            Self::ProcessObserver => k::PANE_AI_ACTIVITY_SOURCE_PROCESS_OBSERVER,
            Self::TmuxFallback => k::PANE_AI_ACTIVITY_SOURCE_TMUX_FALLBACK,
            Self::RuntimeBackend => k::PANE_AI_ACTIVITY_SOURCE_RUNTIME_BACKEND,
            Self::None => k::PANE_AI_ACTIVITY_SOURCE_NONE,
        }
    }

    /// Accuracy tier the source projects. Hook/plugin sources are
    /// `High`; supervisor/process observations are `Medium`; tmux
    /// fallback and the "no signal" sentinel are `Low`.
    pub fn accuracy(self) -> AiActivityAccuracy {
        match self {
            Self::ClaudeCodeHook | Self::CodexHook | Self::OpencodePlugin => {
                AiActivityAccuracy::High
            }
            Self::TerminalStatus | Self::WorkhiveSupervisor => AiActivityAccuracy::Medium,
            Self::ProcessObserver => AiActivityAccuracy::MediumLow,
            // Same tier as the supervisor: the backend names the agent, so
            // identity is known even though activity facts are not.
            Self::RuntimeBackend => AiActivityAccuracy::Medium,
            Self::TmuxFallback => AiActivityAccuracy::Low,
            Self::None => AiActivityAccuracy::None,
        }
    }

    /// i18n key for the **product-facing** per-row source tag
    /// rendered in the dedicated `source` column of
    /// `pane list`. Distinct from
    /// [`AiActivitySource::i18n_key`] (which renders the full
    /// localized source line in `pane show`).
    ///
    /// `ClaudeCodeHook` and `CodexHook` share the `tag.hook` key
    /// because the user-visible distinction (provider name) is
    /// already carried by the `kind` / `label` columns; the
    /// `source` column only needs to convey
    /// hook-vs-plugin-vs-supervisor-vs-process-vs-tmux-vs-none.
    pub fn tag_i18n_key(self) -> &'static str {
        use crate::i18n_keys as k;
        match self {
            Self::ClaudeCodeHook | Self::CodexHook => k::PANE_AI_ACTIVITY_SOURCE_TAG_HOOK,
            Self::OpencodePlugin => k::PANE_AI_ACTIVITY_SOURCE_TAG_PLUGIN,
            Self::TerminalStatus => k::PANE_AI_ACTIVITY_SOURCE_TAG_TERMINAL_STATUS,
            Self::WorkhiveSupervisor => k::PANE_AI_ACTIVITY_SOURCE_TAG_SUPERVISOR,
            Self::ProcessObserver => k::PANE_AI_ACTIVITY_SOURCE_TAG_PROCESS,
            Self::TmuxFallback => k::PANE_AI_ACTIVITY_SOURCE_TAG_TMUX,
            Self::RuntimeBackend => k::PANE_AI_ACTIVITY_SOURCE_TAG_BACKEND,
            Self::None => k::PANE_AI_ACTIVITY_SOURCE_TAG_NONE,
        }
    }

    /// Which localized guidance block (if any) should accompany the
    /// rendered row. Returns `None` only for fresh hook/plugin
    /// sources, which already convey high accuracy without
    /// additional copy.
    pub fn guidance(self, stale: bool) -> Option<&'static str> {
        use crate::i18n_keys as k;
        match (self, stale) {
            (Self::ClaudeCodeHook | Self::CodexHook | Self::OpencodePlugin, false) => None,
            (Self::ClaudeCodeHook | Self::CodexHook | Self::OpencodePlugin, true) => {
                Some(k::PANE_AI_ACTIVITY_GUIDANCE_STALE)
            }
            (Self::TerminalStatus, _) => Some(k::PANE_AI_ACTIVITY_GUIDANCE_UNVERIFIED),
            (Self::WorkhiveSupervisor | Self::ProcessObserver | Self::RuntimeBackend, _) => {
                Some(k::PANE_AI_ACTIVITY_GUIDANCE_UNVERIFIED)
            }
            (Self::TmuxFallback | Self::None, _) => Some(k::PANE_AI_ACTIVITY_GUIDANCE_FALLBACK),
        }
    }
}

/// Accuracy tier rendered next to the activity source. Independent
/// of [`AiActivityConfidence`] (which is internal to the recorder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiActivityAccuracy {
    /// Backed by a real, fresh tool lifecycle event.
    High,
    /// Backed by foreground process observation; we know who is
    /// open but not what they are doing.
    Medium,
    /// Backed by OS process observation; stronger than tmux, weaker
    /// than supervisor or terminal UI status.
    MediumLow,
    /// Last-resort fallback; the rendered state may not reflect
    /// reality at all.
    Low,
    /// No signal.
    None,
}

impl AiActivityAccuracy {
    /// i18n key for the localized accuracy label.
    pub fn i18n_key(self) -> &'static str {
        use crate::i18n_keys as k;
        match self {
            Self::High => k::PANE_AI_ACTIVITY_ACCURACY_HIGH,
            Self::Medium => k::PANE_AI_ACTIVITY_ACCURACY_MEDIUM,
            Self::MediumLow => k::PANE_AI_ACTIVITY_ACCURACY_MEDIUM_LOW,
            Self::Low => k::PANE_AI_ACTIVITY_ACCURACY_LOW,
            Self::None => k::PANE_AI_ACTIVITY_ACCURACY_NONE,
        }
    }
}

/// Conservative observation parsed from an AI tool's visible terminal UI.
///
/// This is a fallback signal only. It must never be written to the hook
/// activity store or rendered as hook-confirmed activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalStatusObservation {
    pub provider: AiActivityProvider,
    pub state: AiActivityState,
    pub reason: Option<String>,
}

impl TerminalStatusObservation {
    pub fn working(provider: AiActivityProvider) -> Self {
        Self {
            provider,
            state: AiActivityState::Processing,
            reason: None,
        }
    }
}

/// Display projection selected by the full AI status source ladder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAiActivityDisplay {
    pub displayed: DisplayedAiActivity,
    pub source: AiActivitySource,
}

/// Resolve the display row using the product source ladder:
/// hook/plugin → terminal-status → supervisor → process → tmux → none.
pub fn resolve_ai_activity_display(
    fact: Option<&AiActivityFact>,
    terminal_status: Option<&TerminalStatusObservation>,
    now_ms: i64,
    foreground: ForegroundAgentSignal,
    observation: crate::domain::PaneObservationSource,
) -> ResolvedAiActivityDisplay {
    let displayed = merge_displayed_activity(fact, now_ms, foreground);
    let projected = project_ai_activity_source(&displayed, observation);
    if matches!(
        projected,
        AiActivitySource::ClaudeCodeHook
            | AiActivitySource::CodexHook
            | AiActivitySource::OpencodePlugin
    ) {
        return ResolvedAiActivityDisplay {
            displayed,
            source: projected,
        };
    }

    if let Some(status) = terminal_status
        && !matches!(status.provider, AiActivityProvider::None)
    {
        return ResolvedAiActivityDisplay {
            displayed: DisplayedAiActivity {
                provider: status.provider,
                state: status.state,
                reason: status.reason.clone(),
                stale: false,
                confidence: AiActivityConfidence::Fallback,
            },
            source: AiActivitySource::TerminalStatus,
        };
    }

    ResolvedAiActivityDisplay {
        displayed,
        source: projected,
    }
}

/// Project a typed [`AiActivitySource`] from the displayed activity
/// + the underlying [`crate::domain::PaneObservationSource`].
///
/// Rules:
/// * If the displayed activity carries a `Hook` / `Plugin` confidence
///   AND the provider is not `None`, the source is the matching
///   provider variant.
/// * Otherwise we fall back to the pane observation source:
///   * `Supervisor` → `WorkhiveSupervisor`
///   * `ProcessObserver` → `ProcessObserver`
///   * `TmuxCurrentCommand` / `Unknown` → `TmuxFallback`
/// * If the displayed activity is `Unknown` with provider `None`
///   and the observation source is also `Unknown`, the source
///   collapses to `None`.
pub fn project_ai_activity_source(
    displayed: &DisplayedAiActivity,
    observation: crate::domain::PaneObservationSource,
) -> AiActivitySource {
    use crate::domain::PaneObservationSource;

    let hook_or_plugin = matches!(
        displayed.confidence,
        AiActivityConfidence::Hook | AiActivityConfidence::Plugin
    );
    if hook_or_plugin && displayed.provider != AiActivityProvider::None && !displayed.stale {
        return match displayed.provider {
            AiActivityProvider::ClaudeCode => AiActivitySource::ClaudeCodeHook,
            AiActivityProvider::Codex => AiActivitySource::CodexHook,
            AiActivityProvider::Opencode => AiActivitySource::OpencodePlugin,
            AiActivityProvider::None => AiActivitySource::None,
        };
    }

    // No fresh hook/plugin fact: project the underlying observation
    // source. The display remains honest about which signal is
    // driving the rendered row.
    if displayed.provider == AiActivityProvider::None
        && displayed.state == AiActivityState::Unknown
        && matches!(observation, PaneObservationSource::Unknown)
    {
        return AiActivitySource::None;
    }
    match observation {
        PaneObservationSource::Supervisor => AiActivitySource::WorkhiveSupervisor,
        PaneObservationSource::ProcessObserver => AiActivitySource::ProcessObserver,
        PaneObservationSource::RuntimeBackend => AiActivitySource::RuntimeBackend,
        PaneObservationSource::TmuxCurrentCommand | PaneObservationSource::Unknown => {
            AiActivitySource::TmuxFallback
        }
    }
}

/// Foreground signal forwarded to the merge policy by callers.
///
/// `pane list` already classifies each pane's foreground (shell vs
/// agent). The activity merge consults that classification through
/// this typed input rather than re-running the heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundAgentSignal {
    /// No recognised AI assistant is in the foreground (e.g. a shell
    /// or a TUI). Activity facts for this pane MUST be ignored so we
    /// never display stale provider data over a different foreground.
    NotAnAgent,
    /// The given provider matches the foreground process. Activity
    /// facts for this provider are eligible for display.
    Matches(AiActivityProvider),
    /// We could not classify the foreground (e.g. the supervisor is
    /// missing). Provider facts are accepted but `stale` semantics
    /// apply as usual.
    Unknown,
}

/// Compute the user-facing activity display for a pane.
///
/// Inputs:
/// - `fact`: the most recent on-disk fact for the pane, if any.
/// - `now_ms`: wall-clock time when the display is being rendered.
/// - `foreground`: the foreground classification computed by the rest
///   of `pane list` (separate signal — process facts are independent
///   from provider events).
///
/// Output: a [`DisplayedAiActivity`] that obeys the TTL rules and the
/// foreground-mismatch rule.
pub fn merge_displayed_activity(
    fact: Option<&AiActivityFact>,
    now_ms: i64,
    foreground: ForegroundAgentSignal,
) -> DisplayedAiActivity {
    // Foreground says this pane is not an agent at all → ignore any
    // stale provider data; display the conservative "none / unknown"
    // line so we never claim AI activity for shells/TUIs.
    if matches!(foreground, ForegroundAgentSignal::NotAnAgent) {
        return DisplayedAiActivity::unknown_none();
    }

    let Some(fact) = fact else {
        // No on-disk fact. If the foreground recognises an agent we
        // know who is open; otherwise stay fully unknown.
        return match foreground {
            ForegroundAgentSignal::Matches(provider) => {
                DisplayedAiActivity::open_unverified(provider)
            }
            ForegroundAgentSignal::Unknown | ForegroundAgentSignal::NotAnAgent => {
                DisplayedAiActivity::unknown_none()
            }
        };
    };

    // Foreground mismatch: e.g. a Claude fact landed in the store but
    // the foreground process is `codex`. Ignore such facts and fall
    // back to "process is open, activity unverified" for the
    // actually-foreground provider.
    if let ForegroundAgentSignal::Matches(fg_provider) = foreground
        && fg_provider != fact.provider
    {
        return DisplayedAiActivity::open_unverified(fg_provider);
    }

    // Apply TTL rules. `processing` / `tool_running` /
    // `waiting_for_permission` expire after their `expires_at`;
    // `waiting_for_user` and `error` remain visible until superseded.
    let expired = fact.state.is_time_limited_busy() && now_ms >= fact.expires_at_unix_ms;
    if expired {
        return DisplayedAiActivity {
            provider: fact.provider,
            state: AiActivityState::OpenUnverified,
            reason: None,
            stale: true,
            confidence: AiActivityConfidence::Fallback,
        };
    }

    DisplayedAiActivity {
        provider: fact.provider,
        state: fact.state,
        reason: fact.reason.clone(),
        stale: false,
        confidence: fact.confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000;

    fn fact(
        provider: AiActivityProvider,
        state: AiActivityState,
        observed_ms: i64,
    ) -> AiActivityFact {
        AiActivityFact::new(
            provider,
            "claude",
            "wh-srv",
            "ws-1",
            "%0",
            state,
            AiActivityConfidence::Hook,
            observed_ms,
            None,
        )
    }

    fn terminal(provider: AiActivityProvider) -> TerminalStatusObservation {
        TerminalStatusObservation::working(provider)
    }

    #[test]
    fn provider_wire_round_trip_for_all_variants() {
        for p in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
            AiActivityProvider::None,
        ] {
            let wire = p.wire_name();
            let restored = AiActivityProvider::from_wire(wire).expect("known wire name");
            assert_eq!(p, restored, "round-trip failure for {wire}");
        }
        assert!(AiActivityProvider::from_wire("bogus").is_none());
        assert!(AiActivityProvider::from_wire("CLAUDE_CODE").is_none());
    }

    #[test]
    fn time_limited_busy_only_for_busy_triplet() {
        for s in [
            AiActivityState::Processing,
            AiActivityState::ToolRunning,
            AiActivityState::WaitingForPermission,
        ] {
            assert!(s.is_time_limited_busy(), "{s:?} must be time-limited busy");
        }
        for s in [
            AiActivityState::Unknown,
            AiActivityState::OpenUnverified,
            AiActivityState::WaitingForUser,
            AiActivityState::Error,
        ] {
            assert!(
                !s.is_time_limited_busy(),
                "{s:?} must NOT be time-limited busy"
            );
        }
    }

    #[test]
    fn implies_work_only_for_processing_and_tool_running() {
        assert!(AiActivityState::Processing.implies_work());
        assert!(AiActivityState::ToolRunning.implies_work());
        assert!(!AiActivityState::WaitingForPermission.implies_work());
        assert!(!AiActivityState::WaitingForUser.implies_work());
        assert!(!AiActivityState::Error.implies_work());
        assert!(!AiActivityState::OpenUnverified.implies_work());
        assert!(!AiActivityState::Unknown.implies_work());
    }

    #[test]
    fn new_fact_sets_busy_ttl_for_busy_states() {
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        assert_eq!(
            f.expires_at_unix_ms,
            NOW + AI_ACTIVITY_DEFAULT_BUSY_TTL_MS,
            "processing fact must carry the default 90s TTL"
        );
        let idle = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::WaitingForUser,
            NOW,
        );
        assert_eq!(
            idle.expires_at_unix_ms,
            i64::MAX,
            "non-time-limited states do not carry a wall-clock TTL"
        );
    }

    #[test]
    fn merge_no_fact_with_no_foreground_is_unknown() {
        let d = merge_displayed_activity(None, NOW, ForegroundAgentSignal::Unknown);
        assert_eq!(d.state, AiActivityState::Unknown);
        assert_eq!(d.provider, AiActivityProvider::None);
        assert!(!d.stale);
    }

    #[test]
    fn merge_no_fact_with_matching_foreground_is_open_unverified() {
        let d = merge_displayed_activity(
            None,
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
        );
        assert_eq!(d.state, AiActivityState::OpenUnverified);
        assert_eq!(d.provider, AiActivityProvider::Codex);
    }

    #[test]
    fn merge_no_fact_with_non_agent_foreground_is_unknown_none() {
        let d = merge_displayed_activity(None, NOW, ForegroundAgentSignal::NotAnAgent);
        assert_eq!(d.state, AiActivityState::Unknown);
        assert_eq!(d.provider, AiActivityProvider::None);
    }

    #[test]
    fn merge_processing_fact_within_ttl_shows_processing() {
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        let d = merge_displayed_activity(
            Some(&f),
            NOW + 10_000, // 10s in, well under 90s
            ForegroundAgentSignal::Matches(AiActivityProvider::ClaudeCode),
        );
        assert_eq!(d.state, AiActivityState::Processing);
        assert!(!d.stale, "fresh processing fact must NOT be stale");
        assert_eq!(d.confidence, AiActivityConfidence::Hook);
    }

    #[test]
    fn merge_processing_fact_after_ttl_downgrades_to_open_unverified_and_marks_stale() {
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        let d = merge_displayed_activity(
            Some(&f),
            NOW + 120_000, // 120s in, past 90s TTL
            ForegroundAgentSignal::Matches(AiActivityProvider::ClaudeCode),
        );
        assert_eq!(d.state, AiActivityState::OpenUnverified);
        assert!(d.stale, "expired busy fact must mark stale");
        assert_eq!(
            d.confidence,
            AiActivityConfidence::Fallback,
            "stale facts must downgrade confidence"
        );
    }

    #[test]
    fn merge_waiting_for_user_remains_visible_indefinitely() {
        let f = fact(
            AiActivityProvider::Codex,
            AiActivityState::WaitingForUser,
            NOW,
        );
        let d = merge_displayed_activity(
            Some(&f),
            NOW + 600_000, // 10 minutes later
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
        );
        assert_eq!(
            d.state,
            AiActivityState::WaitingForUser,
            "waiting_for_user must remain valid past the busy TTL"
        );
        assert!(!d.stale);
    }

    #[test]
    fn merge_error_remains_visible_until_superseded() {
        let f = fact(AiActivityProvider::Opencode, AiActivityState::Error, NOW);
        let d = merge_displayed_activity(
            Some(&f),
            NOW + 3_600_000, // 1 hour later
            ForegroundAgentSignal::Matches(AiActivityProvider::Opencode),
        );
        assert_eq!(d.state, AiActivityState::Error);
    }

    #[test]
    fn merge_foreground_mismatch_ignores_fact_and_returns_open_unverified_for_foreground() {
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        let d = merge_displayed_activity(
            Some(&f),
            NOW + 10_000,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
        );
        assert_eq!(d.state, AiActivityState::OpenUnverified);
        assert_eq!(
            d.provider,
            AiActivityProvider::Codex,
            "foreground mismatch must display the foreground provider, not the fact provider"
        );
    }

    #[test]
    fn merge_processing_fact_with_non_agent_foreground_is_ignored() {
        // The user switched the pane to a shell — provider facts are
        // stale by construction. Display must NOT claim AI activity.
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        let d = merge_displayed_activity(Some(&f), NOW + 10_000, ForegroundAgentSignal::NotAnAgent);
        assert_eq!(d.state, AiActivityState::Unknown);
        assert_eq!(d.provider, AiActivityProvider::None);
    }

    #[test]
    fn fact_round_trips_through_json_with_schema_version() {
        let f = fact(
            AiActivityProvider::ClaudeCode,
            AiActivityState::Processing,
            NOW,
        );
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"provider\":\"claude_code\""));
        let restored: AiActivityFact = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, f);
    }

    #[test]
    fn fact_with_reason_round_trips() {
        let mut f = fact(AiActivityProvider::Codex, AiActivityState::ToolRunning, NOW);
        f.reason = Some("bash".to_owned());
        let json = serde_json::to_string(&f).unwrap();
        let restored: AiActivityFact = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.reason, Some("bash".to_owned()));
    }

    /// `resolve_for_pane` must honour an
    /// existing typed `hint` (the activity store's provider for the
    /// pane) without consulting the catalog. The catalog is a
    /// fallback, not an override — if the recorder has already
    /// attributed an event to a specific provider, that attribution
    /// is the ground truth.
    #[test]
    fn resolve_for_pane_returns_hint_when_provider_is_known() {
        let catalog = crate::domain::ToolCatalog::builtin_defaults();
        for hint in [
            AiActivityProvider::ClaudeCode,
            AiActivityProvider::Codex,
            AiActivityProvider::Opencode,
        ] {
            let resolved = AiActivityProvider::resolve_for_pane(hint, "irrelevant-cmd", &catalog);
            assert_eq!(
                resolved, hint,
                "resolve_for_pane must echo a non-None hint regardless of basename"
            );
        }
    }

    /// Canonical basenames (`claude`, `codex`, `opencode`) keep the
    /// strict mapping. The catalog fallback only kicks in when the
    /// strict mapping returns `None`.
    #[test]
    fn resolve_for_pane_maps_canonical_basenames_without_consulting_catalog() {
        let empty_catalog = crate::domain::ToolCatalog::empty();
        assert_eq!(
            AiActivityProvider::resolve_for_pane(
                AiActivityProvider::None,
                "claude",
                &empty_catalog,
            ),
            AiActivityProvider::ClaudeCode,
            "exact `claude` basename must map to ClaudeCode even with an empty catalog",
        );
        assert_eq!(
            AiActivityProvider::resolve_for_pane(AiActivityProvider::None, "codex", &empty_catalog,),
            AiActivityProvider::Codex,
        );
        assert_eq!(
            AiActivityProvider::resolve_for_pane(
                AiActivityProvider::None,
                "opencode",
                &empty_catalog,
            ),
            AiActivityProvider::Opencode,
        );
    }

    /// Architecture-
    /// suffixed Codex binaries already modeled by the tool catalog
    /// (`codex-x86_64-apple-darwin`, `codex-aarch64-apple-darwin`,
    /// `codex-x86_64-unknown-linux-gnu`, `codex-aarch64-unknown-linux-gnu`)
    /// MUST resolve to `Codex` when no hook fact is available so
    /// `pane list` shows a truthful fallback source and the Codex
    /// `/hooks` hint can fire.
    #[test]
    fn resolve_for_pane_maps_architecture_suffixed_codex_via_tool_catalog() {
        let catalog = crate::domain::ToolCatalog::builtin_defaults();
        for basename in [
            "codex-x86_64-apple-darwin",
            "codex-aarch64-apple-darwin",
            "codex-x86_64-unknown-linux-gnu",
            "codex-aarch64-unknown-linux-gnu",
        ] {
            let resolved =
                AiActivityProvider::resolve_for_pane(AiActivityProvider::None, basename, &catalog);
            assert_eq!(
                resolved,
                AiActivityProvider::Codex,
                "architecture-suffixed Codex binary `{basename}` must resolve to Codex via the tool catalog",
            );
        }
    }

    /// Strict-match guard: commands that merely *look* like one of
    /// the known providers (e.g. `codex-extra`, `mycodex`, or an
    /// extra-suffixed architecture name not modeled by the catalog)
    /// must NOT be attributed to that provider. This proves we are
    /// not doing `contains("codex")` or any other fuzzy substring
    /// check anywhere in the resolver path.
    #[test]
    fn resolve_for_pane_returns_none_for_unmodeled_codex_lookalike_commands() {
        let catalog = crate::domain::ToolCatalog::builtin_defaults();
        for basename in [
            "codex-extra",
            "codex-x86_64-apple-darwin-extra",
            "mycodex",
            "not-codex",
            "claude-fork",
            "opencode-internal",
            // Empty basename — the matcher must short-circuit.
            "",
        ] {
            let resolved =
                AiActivityProvider::resolve_for_pane(AiActivityProvider::None, basename, &catalog);
            assert_eq!(
                resolved,
                AiActivityProvider::None,
                "unmodeled lookalike `{basename}` must resolve to None (no substring matching)",
            );
        }
    }

    #[test]
    fn source_ladder_fresh_hook_beats_terminal_status() {
        let f = fact(AiActivityProvider::Codex, AiActivityState::Processing, NOW);
        let resolved = resolve_ai_activity_display(
            Some(&f),
            Some(&terminal(AiActivityProvider::Codex)),
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::Supervisor,
        );
        assert_eq!(resolved.source, AiActivitySource::CodexHook);
        assert_eq!(resolved.displayed.state, AiActivityState::Processing);
    }

    #[test]
    fn source_ladder_terminal_status_beats_supervisor_process_and_tmux_when_hook_missing() {
        for observation in [
            crate::domain::PaneObservationSource::Supervisor,
            crate::domain::PaneObservationSource::ProcessObserver,
            crate::domain::PaneObservationSource::TmuxCurrentCommand,
        ] {
            let resolved = resolve_ai_activity_display(
                None,
                Some(&terminal(AiActivityProvider::ClaudeCode)),
                NOW,
                ForegroundAgentSignal::Matches(AiActivityProvider::ClaudeCode),
                observation,
            );
            assert_eq!(resolved.source, AiActivitySource::TerminalStatus);
            assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::Medium);
        }
    }

    #[test]
    fn source_ladder_stale_hook_downgrades_to_terminal_status() {
        let f = fact(AiActivityProvider::Codex, AiActivityState::Processing, NOW);
        let resolved = resolve_ai_activity_display(
            Some(&f),
            Some(&terminal(AiActivityProvider::Codex)),
            NOW + 120_000,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::Supervisor,
        );
        assert_eq!(resolved.source, AiActivitySource::TerminalStatus);
    }

    #[test]
    fn source_ladder_supervisor_beats_process_and_tmux_when_terminal_missing() {
        let resolved = resolve_ai_activity_display(
            None,
            None,
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::Supervisor,
        );
        assert_eq!(resolved.source, AiActivitySource::WorkhiveSupervisor);
        assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::Medium);
    }

    #[test]
    fn source_ladder_process_beats_tmux_when_higher_sources_missing() {
        let resolved = resolve_ai_activity_display(
            None,
            None,
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::ProcessObserver,
        );
        assert_eq!(resolved.source, AiActivitySource::ProcessObserver);
        assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::MediumLow);
    }

    #[test]
    fn source_ladder_tmux_is_last_resort() {
        let resolved = resolve_ai_activity_display(
            None,
            None,
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::TmuxCurrentCommand,
        );
        assert_eq!(resolved.source, AiActivitySource::TmuxFallback);
        assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::Low);
    }

    /// A runtime backend that named the agent is its own rung: it must not be
    /// reported as a tmux fallback (a different backend's heuristic) nor
    /// collapse to "none" as if nothing had been observed at all.
    #[test]
    fn source_ladder_runtime_backend_is_not_a_tmux_fallback() {
        let resolved = resolve_ai_activity_display(
            None,
            None,
            NOW,
            ForegroundAgentSignal::Matches(AiActivityProvider::Codex),
            crate::domain::PaneObservationSource::RuntimeBackend,
        );
        assert_eq!(resolved.source, AiActivitySource::RuntimeBackend);
        assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::Medium);
        assert_ne!(resolved.source, AiActivitySource::TmuxFallback);
        assert_ne!(resolved.source, AiActivitySource::None);
    }

    #[test]
    fn source_ladder_none_when_no_signal() {
        let resolved = resolve_ai_activity_display(
            None,
            None,
            NOW,
            ForegroundAgentSignal::Unknown,
            crate::domain::PaneObservationSource::Unknown,
        );
        assert_eq!(resolved.source, AiActivitySource::None);
        assert_eq!(resolved.source.accuracy(), AiActivityAccuracy::None);
    }
}

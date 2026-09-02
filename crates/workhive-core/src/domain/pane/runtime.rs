//! Pane runtime skeleton: runtime identity, capabilities, descriptors,
//! events, and messaging payloads for pane registry and orchestration.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::PaneLaunchOrigin;
use super::{PaneKind, PaneRole, PaneState, UnitId};

// ── Pane Runtime Skeleton ──
//
// The types below form the long-term skeletal structure for pane registry,
// events, capabilities, and messaging. They are defined now to establish
// stable interfaces that future runtime features (monitoring, scheduling,
// inter-pane communication) can build on without rewriting existing APIs.
//
// Current MVP adapters may return `NotSupported` for capabilities they
// cannot yet implement.

/// Runtime-neutral identifier for a pane within the current observed epoch.
///
/// `PaneRuntimeId` is derived from `(group, workspace, unit_id)` using a
/// **length-prefixed encoding** that is injective (collision-free) by
/// construction. It does **not** survive backend restarts or migrations —
/// after a restart, the same logical pane may receive a different
/// `PaneRuntimeId`.
///
/// The derivation includes `group` to prevent aliasing when the same
/// workspace name exists in multiple runtime groups.
///
/// ## Encoding scheme
///
/// The internal representation is:
/// ```text
/// prt:<len_group>:<group>:<len_workspace>:<workspace>:<len_unit_id>:<unit_id>
/// ```
/// where each `len_*` is the byte length of the following component encoded
/// as a decimal integer. Length prefixes make the field boundaries
/// unambiguous, so different `(group, workspace, unit_id)` triples always
/// produce different strings — no hash, no information loss.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaneRuntimeId(String);

impl PaneRuntimeId {
    /// Derive a `PaneRuntimeId` from the full runtime routing scope.
    ///
    /// All three components are required to guarantee uniqueness within a
    /// single registry instance that may hold panes from multiple groups.
    ///
    /// The encoding is injective: distinct inputs always produce distinct IDs.
    pub fn derive(group: &str, workspace: &str, unit_id: &str) -> Self {
        Self(format!(
            "prt:{}:{}:{}:{}:{}:{}",
            group.len(),
            group,
            workspace.len(),
            workspace,
            unit_id.len(),
            unit_id,
        ))
    }

    /// Reconstruct from a previously serialized value.
    ///
    /// This is intentionally **not** `new(arbitrary_string)`. Callers that
    /// need a `PaneRuntimeId` should use [`Self::derive`] in production or
    /// this method only when deserializing a known-good value.
    pub fn from_raw(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneRuntimeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Declared capability of a pane unit.
///
/// Capabilities describe *what a pane can do*, not what it is currently doing.
/// This enables scheduling and routing decisions without inspecting process state.
///
/// ## Design rules
///
/// - **No `is_ai: bool`**: AI-ness is encoded via `MessageReceiver` + `MessageResponder`.
/// - **No hardcoded tool names**: routing logic must check capabilities, never match
///   against strings like `"claude"` or `"codex"`.
/// - **Static declaration + runtime correction**: capabilities are first derived from
///   the launch declaration (`PaneLaunchRef`) at plan time, then optionally corrected
///   by runtime observation — never guessed solely from terminal output.
///
/// ## Variant semantics
///
/// | Variant | Meaning |
/// |---------|---------|
/// | `MessageReceiver` | Pane can accept structured messages sent from other panes |
/// | `MessageResponder` | Pane can produce structured replies to messages |
/// | `CommandExecutor` | Pane can execute arbitrary shell / inline commands |
/// | `Tui` | Pane runs a full-screen TUI (interactive, not a message target) |
/// | `ObserverOnly` | Pane only monitors; cannot receive messages or execute commands |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PaneCapability {
    /// Can receive structured messages from other panes or the control plane.
    MessageReceiver,
    /// Can produce structured replies to received messages.
    MessageResponder,
    /// Can execute arbitrary shell or inline commands.
    CommandExecutor,
    /// Runs a full-screen TUI application (lazygit, yazi, …).
    Tui,
    /// Read-only observer; cannot receive messages or run commands.
    ObserverOnly,
}

/// Runtime lifecycle status of a pane as observed by the registry.
///
/// While `PaneState` captures the observable process state (idle/running),
/// `PaneRuntimeStatus` captures the registry-level lifecycle status
/// that a scheduler or monitor would track.
///
/// Phase 6 aligned: variants represent observable runtime truth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneRuntimeStatus {
    /// Status has not been determined yet.
    #[default]
    Unknown,
    /// Pane shell is idle (no foreground process).
    Idle,
    /// A process is actively running.
    Running,
    /// Pane is blocked waiting on input or a resource.
    Blocked,
    /// Process has exited (pane may still be visible).
    Exited,
}

/// Complete descriptor for a pane in the runtime registry.
///
/// Combines identity, runtime routing context, current snapshot data,
/// capabilities, and lifecycle status. All fields required for message
/// routing (`group`, `workspace`, `unit_id`) are carried here so that
/// `PaneMessagePort` can resolve delivery without external lookups.
///
/// `backend_id` (e.g., tmux `PaneId`) is intentionally excluded — it
/// belongs in the adapter layer, not at the core registry boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneDescriptor {
    /// Epoch-scoped runtime identifier, derived from (group, workspace, unit_id).
    pub runtime_id: PaneRuntimeId,
    /// Runtime isolation group (e.g., tmux server name).
    pub group: String,
    /// Workspace this pane belongs to.
    pub workspace: String,
    /// Runtime-neutral unit identifier within the workspace.
    pub unit_id: UnitId,
    /// Current working directory.
    pub cwd: String,
    /// Current foreground command.
    pub command: String,
    /// Best-effort classification.
    pub kind: PaneKind,
    /// Observable process state.
    pub state: PaneState,
    /// Registry-level lifecycle status.
    pub observed_status: PaneRuntimeStatus,
    /// Role of this pane within the workspace (if assigned).
    pub role: Option<PaneRole>,
    /// How the pane was launched.
    pub launch_origin: PaneLaunchOrigin,
    /// Declared capabilities — deterministic ordering via `BTreeSet`.
    pub capabilities: BTreeSet<PaneCapability>,
}

/// External public event stream for pane lifecycle transitions.
///
/// `PaneEvent` is the stable, long-lived contract for external observers,
/// schedulers, and future automation. Variants represent meaningful pane
/// lifecycle or state transitions — not raw internal diffs.
///
/// **Architectural boundary:** `PaneEvent` is produced by sync logic after
/// interpreting internal [`PaneStateChange`] values. Internal reconciliation
/// details (e.g., cwd/command field diffs) are intentionally excluded from
/// this enum in Phase 5. They remain queryable via descriptor access.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaneEvent {
    /// A previously unknown pane was observed and registered.
    Discovered {
        runtime_id: PaneRuntimeId,
        descriptor: PaneDescriptor,
    },
    /// The observable process state of a pane changed.
    StateChanged {
        runtime_id: PaneRuntimeId,
        from: PaneState,
        to: PaneState,
    },
    /// The derived capability set of a pane changed.
    CapabilityChanged {
        runtime_id: PaneRuntimeId,
        added: BTreeSet<PaneCapability>,
        removed: BTreeSet<PaneCapability>,
    },
    /// A previously registered pane is no longer observed.
    Removed { runtime_id: PaneRuntimeId },
}

/// Runtime observation snapshot fed into registry reconciliation.
///
/// Adapters produce this from backend-specific inspection (e.g., tmux
/// `list-panes`) or from a layered [`crate::domain::PaneRuntimeTruth`]
/// merge. The registry uses it to derive or update a
/// [`PaneDescriptor`] and emit [`PaneStateChange`] diffs.
///
/// The `source` and `confidence` columns must mirror the chosen
/// observation layer so the registry/sync path can call
/// [`derive_runtime_capabilities`] with the same `(kind, state,
/// source, confidence)` tuple the CLI uses to render
/// [`PaneMessageSafety`]. Tmux-only snapshots (no supervisor or OS
/// observer enrichment) MUST report
/// [`crate::domain::observation::PaneObservationSource::TmuxCurrentCommand`]
/// at [`crate::domain::observation::PaneObservationConfidence::Fallback`] —
/// never `Authoritative`. This prevents the registry from issuing
/// `MessageReceiver` capability to manually-started agents whose
/// identity Workhive never confirmed.
///
/// Domain rule: tmux fallback or OS/process observation is not a
/// supervisor handshake; only a running supervisor-authoritative agent
/// may advertise structured-message capabilities.
#[derive(Debug, Clone)]
pub struct ObservedPaneState {
    /// Runtime isolation group.
    pub group: String,
    /// Workspace name.
    pub workspace: String,
    /// Runtime-neutral unit identifier.
    pub unit_id: UnitId,
    /// Current working directory.
    pub cwd: String,
    /// Current foreground command.
    pub command: String,
    /// Best-effort classification.
    pub kind: PaneKind,
    /// Observable process state.
    pub state: PaneState,
    /// Where this observation came from. Adapters that only have a
    /// tmux pane snapshot must use
    /// [`crate::domain::observation::PaneObservationSource::TmuxCurrentCommand`].
    pub source: crate::domain::observation::PaneObservationSource,
    /// Confidence the source ascribes to this observation. Adapters
    /// without a real handshake must use
    /// [`crate::domain::observation::PaneObservationConfidence::Fallback`] or
    /// lower; never `Authoritative`.
    pub confidence: crate::domain::observation::PaneObservationConfidence,
}

/// Internal reconciliation artifact produced by registry update logic.
///
/// `PaneStateChange` captures what changed at the data field level during
/// a reconciliation operation. It is **not** part of the public event
/// stream — it is consumed internally by sync logic to produce
/// [`PaneEvent`] values.
///
/// This type may change shape across phases without breaking external
/// consumers.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneStateChange {
    /// A new pane was registered from observation.
    Registered { runtime_id: PaneRuntimeId },
    /// Observation-derived descriptor fields were updated.
    DescriptorUpdated {
        runtime_id: PaneRuntimeId,
        state_changed: Option<(PaneState, PaneState)>,
        capabilities_changed: Option<(BTreeSet<PaneCapability>, BTreeSet<PaneCapability>)>,
    },
    /// A previously registered pane was not observed and has been removed.
    Removed { runtime_id: PaneRuntimeId },
}

/// Map an observed [`PaneState`] to its [`PaneRuntimeStatus`] equivalent.
///
/// This is the single, authoritative mapping from runtime-observed process
/// state to registry-level lifecycle status. All registry implementations
/// must use this function in `update_from_observation` to ensure the
/// `observed_status` field reflects runtime truth.
///
/// ## Mapping
///
/// | `PaneState` | `PaneRuntimeStatus` |
/// |-------------|---------------------|
/// | `Idle`      | `Idle`              |
/// | `Running`   | `Running`           |
/// | `Unknown`   | `Unknown`           |
pub fn runtime_status_from_state(state: PaneState) -> PaneRuntimeStatus {
    match state {
        PaneState::Idle => PaneRuntimeStatus::Idle,
        PaneState::Running => PaneRuntimeStatus::Running,
        PaneState::Unknown => PaneRuntimeStatus::Unknown,
    }
}

/// Derive pane capabilities from a `PaneKind` runtime observation.
///
/// This is the **plan-time / static declaration** path. It is used by
/// the layout planner and registry reconciliation when *only* a kind
/// is known (e.g. a snapshot restore) and the layered observation
/// stack has not yet produced a [`PaneRuntimeTruth`]. It assumes the
/// observation is correct — i.e. an `Agent` kind here is a *declared*
/// agent, the same way a `--cmd codex` launch declares the kind.
///
/// **Do not** call this from `pane list` / `pane show` rendering or
/// from inter-pane message routing. Those paths must consult
/// [`derive_runtime_capabilities`] which honours observation
/// source/confidence and refuses to claim `MessageReceiver` for an
/// agent pane that has no supervisor handshake.
///
/// ## Rules
///
/// - `Terminal` → `CommandExecutor` (can run arbitrary commands)
/// - `Agent` → `CommandExecutor + MessageReceiver + MessageResponder`
///   (declared agents are assumed to accept structured messages)
/// - `GitUi` → `Tui` (full-screen TUI, not a message target)
/// - `Unknown` → empty (no capability claimed until identity is established)
///
/// **Do not** add tool-name checks here. Routing must never depend on which
/// specific tool is running — only on the capability set.
pub fn derive_capabilities(kind: PaneKind) -> BTreeSet<PaneCapability> {
    match kind {
        PaneKind::Terminal => [PaneCapability::CommandExecutor].into_iter().collect(),
        PaneKind::Agent => [
            PaneCapability::CommandExecutor,
            PaneCapability::MessageReceiver,
            PaneCapability::MessageResponder,
        ]
        .into_iter()
        .collect(),
        PaneKind::GitUi => [PaneCapability::Tui].into_iter().collect(),
        PaneKind::Unknown => BTreeSet::new(),
    }
}

/// Derive pane capabilities from the *layered* runtime observation.
///
/// This is the runtime-correction path used by `pane list` /
/// `pane show`, the registry/sync reconciliation, and any
/// inter-pane message-routing scheduler. Unlike
/// [`derive_capabilities`], it consults
/// `(kind, state, source, confidence)` so an agent pane **only**
/// claims structured-message capabilities when **all four** of the
/// following hold simultaneously:
///
/// 1. `kind` is [`PaneKind::Agent`].
/// 2. `state` is [`PaneState::Running`] — a supervised agent that
///    is `Idle`, `Exited`, or `Unknown` no longer has a live channel.
/// 3. `source` is [`PaneObservationSource::Supervisor`].
/// 4. `confidence` is [`PaneObservationConfidence::Authoritative`].
///
/// Runtime capability derivation MUST stay aligned with
/// [`crate::domain::derive_message_safety`]: the registry/sync code
/// derives capabilities here, and the CLI/router checks
/// [`PaneMessageSafety::permits_structured_message`] — disagreement
/// between the two would let one path believe the pane is reachable
/// while the other refuses delivery. Tmux fallback or OS/process
/// observation is not a supervisor handshake, so only a running
/// supervisor-authoritative agent may advertise structured-message
/// capabilities.
///
/// ## Rules
///
/// | Kind     | State    | Supervisor authoritative? | Capabilities |
/// |----------|----------|---------------------------|--------------|
/// | Terminal | any      | n/a                       | `CommandExecutor` |
/// | Agent    | Running  | yes                       | `CommandExecutor + MessageReceiver + MessageResponder` |
/// | Agent    | Idle/Unknown | yes                   | `CommandExecutor` (no live handshake) |
/// | Agent    | any      | no (OS / tmux / unknown)  | `CommandExecutor` (unverified) |
/// | GitUi    | any      | n/a                       | `Tui` (never `MessageReceiver`) |
/// | Unknown  | any      | n/a                       | empty (conservative) |
pub fn derive_runtime_capabilities(
    kind: PaneKind,
    state: PaneState,
    source: crate::domain::observation::PaneObservationSource,
    confidence: crate::domain::observation::PaneObservationConfidence,
) -> BTreeSet<PaneCapability> {
    use crate::domain::observation::{PaneObservationConfidence, PaneObservationSource};
    match kind {
        PaneKind::Terminal => [PaneCapability::CommandExecutor].into_iter().collect(),
        PaneKind::Agent => {
            let has_supervisor_handshake = matches!(source, PaneObservationSource::Supervisor)
                && matches!(confidence, PaneObservationConfidence::Authoritative);
            let agent_running = matches!(state, PaneState::Running);
            if has_supervisor_handshake && agent_running {
                [
                    PaneCapability::CommandExecutor,
                    PaneCapability::MessageReceiver,
                    PaneCapability::MessageResponder,
                ]
                .into_iter()
                .collect()
            } else {
                // Either:
                //   - foreground process looks like an agent but no
                //     supervisor handshake, OR
                //   - supervisor previously handshook but the agent
                //     has now exited / gone idle, so the structured
                //     channel is gone.
                // Treat like a shell: free-form command input is
                // still allowed, structured messaging is not.
                [PaneCapability::CommandExecutor].into_iter().collect()
            }
        }
        PaneKind::GitUi => [PaneCapability::Tui].into_iter().collect(),
        PaneKind::Unknown => BTreeSet::new(),
    }
}

/// Derive static pane capabilities from a `PaneLaunchRef` at plan time.
///
/// This is the **static declaration** path in the "static declaration + runtime
/// correction" design. It runs during layout planning, before any pane is
/// actually launched, giving the scheduler an early capability estimate.
///
/// Runtime observation via [`derive_capabilities`] may later refine these.
///
/// ## Rules
///
/// - `EmptyShell` → `CommandExecutor` (user will type commands manually)
/// - `Tool(name)` → resolved via the catalog; AI tools get full message
///   capabilities, TUI tools get `Tui`, others get `CommandExecutor`.
///   Resolution is done by the caller (catalog lookup) and passed as `tool_kind`.
/// - `InlineCommand` → `CommandExecutor` (runs a specific command, non-interactive)
pub fn derive_static_capabilities(kind: PaneKind) -> BTreeSet<PaneCapability> {
    derive_capabilities(kind)
}

/// Registry-level lifecycle event for pane transitions.
///
/// `PaneLifecycleEvent` represents meaningful state transitions within the
/// pane registry. It is the output contract of
/// [`PaneRegistryPort::drain_events`] and is distinct from the
/// observation-derived [`PaneEvent`] produced by sync logic.
///
/// Phase 6: this is the stable lifecycle event stream for registry consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaneLifecycleEvent {
    /// A new pane was registered in the registry.
    Registered { pane: PaneRuntimeId },
    /// An existing pane descriptor was updated.
    Updated { pane: PaneRuntimeId },
    /// The observed runtime status of a pane changed.
    StatusChanged {
        pane: PaneRuntimeId,
        from: PaneRuntimeStatus,
        to: PaneRuntimeStatus,
    },
    /// A pane was removed from the registry.
    Unregistered { pane: PaneRuntimeId },
}

/// A message sent between panes or from the control plane to a pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneMessage {
    /// Sender pane, or `None` for control-plane messages.
    pub from: Option<PaneRuntimeId>,
    /// Target pane.
    pub to: PaneRuntimeId,
    /// Message payload (opaque to the transport layer).
    pub payload: PaneMessagePayload,
}

/// Payload variants for inter-pane messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneMessagePayload {
    /// Plain text to be sent as input.
    Text { content: String },
    /// A structured command for the pane to interpret.
    Command { name: String, args: Vec<String> },
    /// A signal (e.g., interrupt, terminate).
    Signal { signal: String },
}

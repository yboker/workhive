//! AI assistant activity use cases — provider event mapping and recording.
//!
//! The provider mapping functions in this module are pure: they take the
//! raw event name from a Claude Code hook, a Codex hook, or an OpenCode
//! plugin event, plus a `now_ms`, and return a typed
//! [`crate::domain::AiActivityFact`]. The on-disk write is owned by the
//! adapter side ([`crate::ports::PaneActivityStorePort`]) so use-case logic
//! stays I/O-free.

use thiserror::Error;
use tracing::{debug, warn};

use crate::domain::{
    AI_ACTIVITY_SCHEMA_VERSION, AiActivityConfidence, AiActivityFact, AiActivityProvider,
    AiActivityState,
};
use crate::ports::{ActivityEventStore, PaneActivityStorePort};
use crate::usecases::activity_control_plane::{normalize, observation_from_legacy_input};

/// Strongly-typed errors for the internal `activity record` entrypoint.
#[derive(Debug, Error, PartialEq)]
pub enum ActivityRecordError {
    /// The hook payload could not be parsed as JSON.
    #[error("invalid activity record payload (not JSON): {0}")]
    InvalidJson(String),
    /// The payload parsed as JSON but did not validate against the
    /// expected shape (missing required field, wrong type, unknown
    /// provider, etc.).
    #[error("invalid activity record fields: {0}")]
    InvalidFields(String),
    /// The payload's `provider` does not match the provider declared
    /// via `WORKHIVE_TOOL_PROVIDER` for the pane. Hook scripts are
    /// expected to use the env var as the source of truth; a mismatch
    /// indicates a misconfigured installation and must be rejected.
    #[error("provider mismatch: expected {expected}, received {actual}")]
    ProviderMismatch { expected: String, actual: String },
    /// The store rejected the write (I/O failure, disk full, etc.).
    #[error("failed to write activity fact: {0}")]
    WriteFailed(String),
}

/// Map a Claude Code hook event name (per
/// <https://code.claude.com/docs/en/hooks>) to a typed activity state.
///
/// Returns `None` for events that should not change the surfaced
/// activity state — the recorder must then keep the previously
/// observed state intact.
pub fn map_claude_event(hook_name: &str) -> Option<(AiActivityState, AiActivityConfidence)> {
    Some(match hook_name {
        // A user submitted a prompt → the model is now working.
        "UserPromptSubmit" => (AiActivityState::Processing, AiActivityConfidence::Hook),
        // Pre/post tool use → a sub-tool is running.
        "PreToolUse" => (AiActivityState::ToolRunning, AiActivityConfidence::Hook),
        // PostToolUse means the sub-tool finished. The model is still
        // working on the user's request until `Stop` fires, so we
        // explicitly classify this as `Processing` rather than idle.
        "PostToolUse" => (AiActivityState::Processing, AiActivityConfidence::Hook),
        // Claude Code surfaces explicit permission requests.
        "PermissionRequest" => (
            AiActivityState::WaitingForPermission,
            AiActivityConfidence::Hook,
        ),
        // Stop = turn complete. Waiting for the user to type the next prompt.
        "Stop" => (AiActivityState::WaitingForUser, AiActivityConfidence::Hook),
        // StopFailure = turn failed → error.
        "StopFailure" => (AiActivityState::Error, AiActivityConfidence::Hook),
        _ => return None,
    })
}

/// Map a Codex hook event name (per
/// <https://developers.openai.com/codex/hooks>) to a typed activity
/// state. Returns `None` for events outside our activity vocabulary.
pub fn map_codex_event(hook_name: &str) -> Option<(AiActivityState, AiActivityConfidence)> {
    Some(match hook_name {
        "SessionStart" => (
            AiActivityState::OpenUnverified,
            AiActivityConfidence::Heuristic,
        ),
        "UserPromptSubmit" => (AiActivityState::Processing, AiActivityConfidence::Hook),
        "PreToolUse" => (AiActivityState::ToolRunning, AiActivityConfidence::Hook),
        "PostToolUse" => (AiActivityState::Processing, AiActivityConfidence::Hook),
        "PermissionRequest" => (
            AiActivityState::WaitingForPermission,
            AiActivityConfidence::Hook,
        ),
        "Stop" => (AiActivityState::WaitingForUser, AiActivityConfidence::Hook),
        _ => return None,
    })
}

/// Map an OpenCode plugin event name (per
/// <https://opencode.ai/docs/plugins/>) to a typed activity state.
///
/// OpenCode is the only provider that exposes a richly typed event
/// vocabulary (session vs permission vs tool events), so we cover the
/// full set the requirement explicitly enumerates.
pub fn map_opencode_event(event: &str) -> Option<(AiActivityState, AiActivityConfidence)> {
    Some(match event {
        "session.created" => (
            AiActivityState::OpenUnverified,
            AiActivityConfidence::Plugin,
        ),
        "session.updated" | "session.status" => {
            (AiActivityState::Processing, AiActivityConfidence::Plugin)
        }
        "session.idle" => (
            AiActivityState::WaitingForUser,
            AiActivityConfidence::Plugin,
        ),
        "session.error" => (AiActivityState::Error, AiActivityConfidence::Plugin),
        "permission.asked" => (
            AiActivityState::WaitingForPermission,
            AiActivityConfidence::Plugin,
        ),
        "permission.replied" => (AiActivityState::Processing, AiActivityConfidence::Plugin),
        "tool.execute.before" => (AiActivityState::ToolRunning, AiActivityConfidence::Plugin),
        "tool.execute.after" => (AiActivityState::Processing, AiActivityConfidence::Plugin),
        _ => return None,
    })
}

/// Parsed payload received by the internal `activity record` entrypoint.
///
/// Lives in core because both the CLI (parses JSON from stdin) and
/// tests need to validate the same shape. The recorder converts an
/// `ActivityRecordInput` into an [`AiActivityFact`] after correlating
/// it with the expected provider from the environment.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct ActivityRecordInput {
    /// Schema version; must equal [`AI_ACTIVITY_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Provider wire name. The recorder rejects values that do not
    /// match the expected provider from `WORKHIVE_TOOL_PROVIDER`.
    pub provider: String,
    /// Specific event name (e.g. `UserPromptSubmit`,
    /// `tool.execute.before`). Used for diagnostics; the *state* is
    /// what determines the rendered label.
    pub event: String,
    /// Tool name supplied by the hook payload (foreground tool that
    /// produced the event).
    pub tool_name: String,
    /// Runtime correlation: tmux server / runtime group.
    pub group: String,
    pub workspace: String,
    pub unit_id: String,
    /// Resolved state derived from the hook event. The hook script is
    /// responsible for translating event names to one of the seven
    /// known states; the recorder validates the resulting enum.
    pub state: AiActivityState,
    /// Confidence assigned by the hook script.
    pub confidence: AiActivityConfidence,
    /// Observation time in Unix milliseconds.
    pub observed_at_unix_ms: i64,
    /// Optional display-safe reason.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Record an activity fact for a pane.
///
/// `expected_provider` is the provider Workhive launched in this pane,
/// taken from `WORKHIVE_TOOL_PROVIDER` at hook spawn time. The recorder
/// refuses to attribute an event to a different provider.
pub fn record_activity<S: PaneActivityStorePort + ?Sized>(
    input: &ActivityRecordInput,
    expected_provider: AiActivityProvider,
    store: &S,
) -> Result<AiActivityFact, ActivityRecordError> {
    if input.schema_version != AI_ACTIVITY_SCHEMA_VERSION {
        return Err(ActivityRecordError::InvalidFields(format!(
            "unsupported schema_version: {}",
            input.schema_version
        )));
    }
    let claimed = AiActivityProvider::from_wire(&input.provider).ok_or_else(|| {
        ActivityRecordError::InvalidFields(format!("unknown provider: {}", input.provider))
    })?;
    if claimed != expected_provider {
        return Err(ActivityRecordError::ProviderMismatch {
            expected: expected_provider.wire_name().to_owned(),
            actual: claimed.wire_name().to_owned(),
        });
    }
    let fact = AiActivityFact::new(
        claimed,
        input.tool_name.clone(),
        input.group.clone(),
        input.workspace.clone(),
        input.unit_id.clone(),
        input.state,
        input.confidence,
        input.observed_at_unix_ms,
        input.reason.clone(),
    );
    store
        .record(&fact)
        .map_err(|e| ActivityRecordError::WriteFailed(e.to_string()))?;
    Ok(fact)
}

/// Activity-control-plane feature gate, isolated from the CLI-side
/// `ai_integrations::FeatureGate` so the core does not depend on
/// the wider AI integrations machinery for a single decision.
///
/// Conversion from the CLI's richer feature gate happens at the
/// CLI boundary; the core only sees the bool tuple it cares about.
///
/// Semantics:
///
/// - `globally_enabled = false` => ACP append is skipped for every
///   provider. The legacy latest-fact path continues to run.
/// - `provider_enabled = false` (with `globally_enabled = true`)
///   => ACP append is skipped for this specific provider. The
///   legacy latest-fact path continues to run.
/// - Both `true` => ACP append is attempted; failures fail open.
///
/// "Fail-open" means an ACP append failure is logged and dropped,
/// never propagated to the legacy path or the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityControlPlaneGate {
    /// AI integrations master switch (covers all providers).
    pub globally_enabled: bool,
    /// Per-trusted-provider switch for the pane this record is for.
    pub provider_enabled: bool,
}

impl ActivityControlPlaneGate {
    /// All-disabled gate — the ACP path is silenced entirely.
    /// Used by the CLI hook handler when it cannot derive a usable
    /// project root from the runtime root (fail-closed for ACP only).
    pub const fn all_disabled() -> Self {
        Self {
            globally_enabled: false,
            provider_enabled: false,
        }
    }

    /// All-enabled gate — the ACP path runs end-to-end. Used by
    /// core-level tests and by the CLI when both the global switch
    /// and the provider switch resolve to enabled.
    pub const fn all_enabled() -> Self {
        Self {
            globally_enabled: true,
            provider_enabled: true,
        }
    }

    /// Whether the ACP append should be attempted for this record.
    pub const fn allows_acp_append(self) -> bool {
        self.globally_enabled && self.provider_enabled
    }
}

/// Dual-write the legacy latest-fact AND the ACP event in one pass.
///
/// Legacy contract is **honored unchanged**:
///
/// - The legacy write happens first; if it fails (schema mismatch,
///   provider mismatch, unknown wire provider, store I/O error,
///   ...) the function returns the legacy error and the ACP path
///   is **not** touched.
/// - On legacy success, the function then best-effort writes the
///   ACP event. Any failure on the ACP path (gate-disabled,
///   unknown event name, normalizer rejection, append I/O error)
///   is logged via [`tracing`] and dropped; the returned
///   [`AiActivityFact`] is whatever the legacy write produced.
///
/// This shape lets the CLI hook handler call a single function and
/// never have to think about wiring the two paths — the legacy
/// behavior the user-facing `pane list` depends on stays
/// deterministic, and the new ACP append rides alongside it.
pub fn record_activity_dual_write<L, E>(
    input: &ActivityRecordInput,
    expected_provider: AiActivityProvider,
    legacy_store: &L,
    event_store: &E,
    gate: ActivityControlPlaneGate,
) -> Result<AiActivityFact, ActivityRecordError>
where
    L: PaneActivityStorePort + ?Sized,
    E: ActivityEventStore + ?Sized,
{
    // Legacy path runs first and is the only path that can affect
    // the caller's error result. Provider mismatch / schema /
    // unknown provider all bail here, leaving the event store
    // untouched — matching the orchestrator's contract that a
    // mismatch must reject both writes.
    let fact = record_activity(input, expected_provider, legacy_store)?;

    // ACP path is best-effort from here on. Disabled gate, unknown
    // event names, normalizer rejection, and store I/O errors all
    // resolve to "log and continue".
    if !gate.allows_acp_append() {
        debug!(
            provider = expected_provider.wire_name(),
            "ACP append skipped: feature gate disabled"
        );
        return Ok(fact);
    }

    let observation = match observation_from_legacy_input(input, expected_provider) {
        Some(obs) => obs,
        None => {
            debug!(
                provider = expected_provider.wire_name(),
                event = %input.event,
                "ACP append skipped: payload not mappable to control-plane observation"
            );
            return Ok(fact);
        }
    };

    let event = match normalize(&observation) {
        Ok(event) => event,
        Err(err) => {
            debug!(
                provider = expected_provider.wire_name(),
                event = %input.event,
                error = ?err,
                "ACP append skipped: normalizer rejected observation"
            );
            return Ok(fact);
        }
    };

    if let Err(err) = event_store.append(&event) {
        warn!(
            provider = expected_provider.wire_name(),
            event = %input.event,
            error = %err,
            "ACP activity event append failed; legacy path preserved (fail-open)"
        );
    }

    Ok(fact)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    use crate::domain::AiActivityProvider;
    use crate::ports::PaneActivityKey;

    struct InMemoryStore {
        facts: RefCell<HashMap<PaneActivityKey, AiActivityFact>>,
    }

    impl InMemoryStore {
        fn new() -> Self {
            Self {
                facts: RefCell::new(HashMap::new()),
            }
        }
    }

    impl PaneActivityStorePort for InMemoryStore {
        fn record(&self, fact: &AiActivityFact) -> Result<(), crate::ports::PaneActivityIoError> {
            self.facts.borrow_mut().insert(
                PaneActivityKey {
                    group: fact.group.clone(),
                    workspace: fact.workspace.clone(),
                    unit_id: fact.unit_id.clone(),
                },
                fact.clone(),
            );
            Ok(())
        }

        fn read(
            &self,
            key: &PaneActivityKey,
        ) -> Result<Option<AiActivityFact>, crate::ports::PaneActivityIoError> {
            Ok(self.facts.borrow().get(key).cloned())
        }

        fn remove(&self, key: &PaneActivityKey) -> Result<(), crate::ports::PaneActivityIoError> {
            self.facts.borrow_mut().remove(key);
            Ok(())
        }
    }

    #[test]
    fn claude_event_mapping_covers_required_lifecycle_events() {
        assert_eq!(
            map_claude_event("UserPromptSubmit"),
            Some((AiActivityState::Processing, AiActivityConfidence::Hook))
        );
        assert_eq!(
            map_claude_event("PreToolUse"),
            Some((AiActivityState::ToolRunning, AiActivityConfidence::Hook))
        );
        assert_eq!(
            map_claude_event("PermissionRequest"),
            Some((
                AiActivityState::WaitingForPermission,
                AiActivityConfidence::Hook
            ))
        );
        assert_eq!(
            map_claude_event("Stop"),
            Some((AiActivityState::WaitingForUser, AiActivityConfidence::Hook))
        );
        assert_eq!(
            map_claude_event("StopFailure"),
            Some((AiActivityState::Error, AiActivityConfidence::Hook))
        );
        assert!(map_claude_event("CompletelyUnknownEvent").is_none());
    }

    #[test]
    fn codex_event_mapping_covers_required_lifecycle_events() {
        assert_eq!(
            map_codex_event("UserPromptSubmit"),
            Some((AiActivityState::Processing, AiActivityConfidence::Hook))
        );
        assert_eq!(
            map_codex_event("PreToolUse"),
            Some((AiActivityState::ToolRunning, AiActivityConfidence::Hook))
        );
        assert_eq!(
            map_codex_event("PermissionRequest"),
            Some((
                AiActivityState::WaitingForPermission,
                AiActivityConfidence::Hook
            ))
        );
        assert_eq!(
            map_codex_event("Stop"),
            Some((AiActivityState::WaitingForUser, AiActivityConfidence::Hook))
        );
        // SessionStart is a best-effort heuristic because Codex hook
        // event shapes can evolve across tool versions.
        assert_eq!(
            map_codex_event("SessionStart"),
            Some((
                AiActivityState::OpenUnverified,
                AiActivityConfidence::Heuristic
            ))
        );
        assert!(map_codex_event("FutureUnknown").is_none());
    }

    #[test]
    fn opencode_event_mapping_covers_session_permission_and_tool_events() {
        assert_eq!(
            map_opencode_event("session.created"),
            Some((
                AiActivityState::OpenUnverified,
                AiActivityConfidence::Plugin
            ))
        );
        assert_eq!(
            map_opencode_event("session.idle"),
            Some((
                AiActivityState::WaitingForUser,
                AiActivityConfidence::Plugin
            ))
        );
        assert_eq!(
            map_opencode_event("session.error"),
            Some((AiActivityState::Error, AiActivityConfidence::Plugin))
        );
        assert_eq!(
            map_opencode_event("permission.asked"),
            Some((
                AiActivityState::WaitingForPermission,
                AiActivityConfidence::Plugin
            ))
        );
        assert_eq!(
            map_opencode_event("permission.replied"),
            Some((AiActivityState::Processing, AiActivityConfidence::Plugin))
        );
        assert_eq!(
            map_opencode_event("tool.execute.before"),
            Some((AiActivityState::ToolRunning, AiActivityConfidence::Plugin))
        );
        assert_eq!(
            map_opencode_event("tool.execute.after"),
            Some((AiActivityState::Processing, AiActivityConfidence::Plugin))
        );
        assert!(map_opencode_event("session.unknown.future").is_none());
    }

    fn valid_input() -> ActivityRecordInput {
        ActivityRecordInput {
            schema_version: AI_ACTIVITY_SCHEMA_VERSION,
            provider: "claude_code".to_owned(),
            event: "UserPromptSubmit".to_owned(),
            tool_name: "claude".to_owned(),
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: "%0".to_owned(),
            state: AiActivityState::Processing,
            confidence: AiActivityConfidence::Hook,
            observed_at_unix_ms: 1_700_000_000_000,
            reason: None,
        }
    }

    #[test]
    fn record_activity_persists_a_fact_and_returns_it() {
        let store = InMemoryStore::new();
        let input = valid_input();
        let fact =
            record_activity(&input, AiActivityProvider::ClaudeCode, &store).expect("must record");
        assert_eq!(fact.state, AiActivityState::Processing);
        assert_eq!(fact.provider, AiActivityProvider::ClaudeCode);
        let key = PaneActivityKey {
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: "%0".to_owned(),
        };
        let restored = store
            .read(&key)
            .expect("read ok")
            .expect("fact must be in store");
        assert_eq!(restored, fact);
    }

    #[test]
    fn record_activity_rejects_mismatched_provider() {
        let store = InMemoryStore::new();
        let mut input = valid_input();
        input.provider = "codex".to_owned();
        let err =
            record_activity(&input, AiActivityProvider::ClaudeCode, &store).expect_err("reject");
        match err {
            ActivityRecordError::ProviderMismatch { expected, actual } => {
                assert_eq!(expected, "claude_code");
                assert_eq!(actual, "codex");
            }
            other => panic!("expected ProviderMismatch, got {other:?}"),
        }
        // Store must remain empty.
        let key = PaneActivityKey {
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: "%0".to_owned(),
        };
        assert!(store.read(&key).unwrap().is_none());
    }

    #[test]
    fn record_activity_rejects_unknown_provider_wire_name() {
        let store = InMemoryStore::new();
        let mut input = valid_input();
        input.provider = "totally-not-a-provider".to_owned();
        let err =
            record_activity(&input, AiActivityProvider::ClaudeCode, &store).expect_err("reject");
        assert!(matches!(err, ActivityRecordError::InvalidFields(_)));
    }

    #[test]
    fn record_activity_rejects_wrong_schema_version() {
        let store = InMemoryStore::new();
        let mut input = valid_input();
        input.schema_version = 99;
        let err =
            record_activity(&input, AiActivityProvider::ClaudeCode, &store).expect_err("reject");
        assert!(matches!(err, ActivityRecordError::InvalidFields(_)));
    }

    // ----------------------------------------------------------------
    // record_activity_dual_write tests
    // ----------------------------------------------------------------
    //
    // The core crate cannot depend on `workhive-testkit` (crate
    // dependency direction rule), so we use a tiny local
    // in-memory `ActivityEventStore` fake with optional error
    // injection that mirrors the testkit one's contract for the
    // single behavior these tests exercise: append failures must
    // surface a typed error so the dual-write helper can swallow it.

    use crate::domain::activity::{
        ActivityEvent, ActivityEventKind, PaneCorrelationKey, ProviderKind, SignalChannel,
    };
    use crate::ports::{ActivityEventStore, ActivityEventStoreError};

    #[derive(Default)]
    struct InMemoryEventStore {
        appended: RefCell<Vec<ActivityEvent>>,
        force_append_io_error: RefCell<bool>,
    }

    impl InMemoryEventStore {
        fn new() -> Self {
            Self::default()
        }

        fn inject_append_io_error(&self) {
            *self.force_append_io_error.borrow_mut() = true;
        }

        fn len(&self) -> usize {
            self.appended.borrow().len()
        }

        fn last(&self) -> Option<ActivityEvent> {
            self.appended.borrow().last().cloned()
        }
    }

    impl ActivityEventStore for InMemoryEventStore {
        fn append(&self, event: &ActivityEvent) -> Result<(), ActivityEventStoreError> {
            if *self.force_append_io_error.borrow() {
                return Err(ActivityEventStoreError::Io("injected".to_owned()));
            }
            self.appended.borrow_mut().push(event.clone());
            Ok(())
        }

        fn events_for_pane(
            &self,
            pane: &PaneCorrelationKey,
        ) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
            Ok(self
                .appended
                .borrow()
                .iter()
                .filter(|e| &e.pane == pane)
                .cloned()
                .collect())
        }

        fn replay_all(&self) -> Result<Vec<ActivityEvent>, ActivityEventStoreError> {
            Ok(self.appended.borrow().clone())
        }
    }

    fn pane_key() -> PaneActivityKey {
        PaneActivityKey {
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: "%0".to_owned(),
        }
    }

    fn provider_input(
        provider: AiActivityProvider,
        event: &str,
        state: AiActivityState,
        confidence: AiActivityConfidence,
        reason: Option<&str>,
    ) -> ActivityRecordInput {
        ActivityRecordInput {
            schema_version: AI_ACTIVITY_SCHEMA_VERSION,
            provider: provider.wire_name().to_owned(),
            event: event.to_owned(),
            tool_name: "tool".to_owned(),
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: "%0".to_owned(),
            state,
            confidence,
            observed_at_unix_ms: 1_700_000_000_000,
            reason: reason.map(str::to_owned),
        }
    }

    /// Case 1: Claude `UserPromptSubmit` -> ACP `PromptSubmitted`.
    #[test]
    fn claude_user_prompt_submit_dual_writes_prompt_submitted_event() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        let fact = record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        assert_eq!(fact.state, AiActivityState::Processing);
        assert_eq!(events.len(), 1);
        let appended = events.last().unwrap();
        assert_eq!(appended.provider, ProviderKind::ClaudeCode);
        assert_eq!(appended.channel, SignalChannel::LifecycleHook);
        assert_eq!(appended.kind, ActivityEventKind::PromptSubmitted);
    }

    /// Case 2: Claude `PreToolUse` with tool name -> ToolStarted +
    /// `detail` carrying the tool name.
    #[test]
    fn claude_pre_tool_use_dual_writes_tool_started_with_detail() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "PreToolUse",
            AiActivityState::ToolRunning,
            AiActivityConfidence::Hook,
            Some("Bash"),
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        assert_eq!(events.len(), 1);
        let appended = events.last().unwrap();
        assert_eq!(appended.kind, ActivityEventKind::ToolStarted);
        assert_eq!(appended.detail.as_deref(), Some("Bash"));
    }

    /// Case 3 — invariant guard: Claude `Stop` MUST normalize to
    /// `TurnCompleted` and the event store MUST receive a
    /// lifecycle-channel event, NEVER `NotifyCompletion` or
    /// `TurnNotifiedComplete`.
    #[test]
    fn claude_stop_appends_turn_completed_not_notify_completion() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "Stop",
            AiActivityState::WaitingForUser,
            AiActivityConfidence::Hook,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.channel, SignalChannel::LifecycleHook);
        assert_eq!(appended.kind, ActivityEventKind::TurnCompleted);
        assert_ne!(
            appended.kind,
            ActivityEventKind::TurnNotifiedComplete,
            "Stop must never normalize to a notify-shaped completion"
        );
    }

    /// Case 4: Claude `StopFailure` -> ACP `TurnFailed`.
    #[test]
    fn claude_stop_failure_appends_turn_failed() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "StopFailure",
            AiActivityState::Error,
            AiActivityConfidence::Hook,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.kind, ActivityEventKind::TurnFailed);
    }

    /// Case 5: Codex `PreToolUse` -> ACP ToolStarted on the
    /// lifecycle channel (not the notify channel).
    #[test]
    fn codex_pre_tool_use_appends_tool_started_lifecycle_channel() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Codex,
            "PreToolUse",
            AiActivityState::ToolRunning,
            AiActivityConfidence::Hook,
            Some("git_status"),
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Codex,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.provider, ProviderKind::Codex);
        assert_eq!(appended.channel, SignalChannel::LifecycleHook);
        assert_eq!(appended.kind, ActivityEventKind::ToolStarted);
    }

    /// Case 6 — invariant guard: Codex `Stop` via lifecycle hook
    /// MUST normalize to `TurnCompleted` on
    /// `SignalChannel::LifecycleHook`. The bridge MUST NEVER route
    /// a lifecycle Stop through the notify channel.
    #[test]
    fn codex_stop_appends_turn_completed_via_lifecycle_hook_not_notify() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Codex,
            "Stop",
            AiActivityState::WaitingForUser,
            AiActivityConfidence::Hook,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Codex,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.channel, SignalChannel::LifecycleHook);
        assert_ne!(appended.channel, SignalChannel::NotifyCallback);
        assert_eq!(appended.kind, ActivityEventKind::TurnCompleted);
        assert_ne!(
            appended.kind,
            ActivityEventKind::TurnNotifiedComplete,
            "lifecycle Stop must never produce a notify-shaped completion"
        );
    }

    /// Case 7: OpenCode `tool.execute.before` -> ACP ToolStarted on
    /// the plugin channel.
    #[test]
    fn opencode_tool_execute_before_appends_tool_started_plugin_channel() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Opencode,
            "tool.execute.before",
            AiActivityState::ToolRunning,
            AiActivityConfidence::Plugin,
            Some("edit"),
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Opencode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.provider, ProviderKind::OpenCode);
        assert_eq!(appended.channel, SignalChannel::PluginEvent);
        assert_eq!(
            appended.source,
            crate::domain::activity::ProviderSignalSource::Plugin
        );
        assert_eq!(appended.kind, ActivityEventKind::ToolStarted);
    }

    /// Case 8: OpenCode `session.idle` -> ACP SessionIdle.
    #[test]
    fn opencode_session_idle_appends_session_idle() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Opencode,
            "session.idle",
            AiActivityState::WaitingForUser,
            AiActivityConfidence::Plugin,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Opencode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.kind, ActivityEventKind::SessionIdle);
    }

    /// Case 9: OpenCode `session.error` -> ACP TurnFailed.
    #[test]
    fn opencode_session_error_appends_turn_failed() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Opencode,
            "session.error",
            AiActivityState::Error,
            AiActivityConfidence::Plugin,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Opencode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("must record");
        let appended = events.last().expect("must append");
        assert_eq!(appended.kind, ActivityEventKind::TurnFailed);
    }

    /// Case 10: a provider mismatch between trusted env and the
    /// payload MUST be rejected by the legacy path AND leave the
    /// event store untouched.
    #[test]
    fn provider_mismatch_rejects_both_legacy_and_acp() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let mut input = provider_input(
            AiActivityProvider::ClaudeCode,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        input.provider = AiActivityProvider::Codex.wire_name().to_owned();
        let err = record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect_err("must reject");
        assert!(matches!(err, ActivityRecordError::ProviderMismatch { .. }));
        // Legacy store stayed empty.
        assert!(legacy.read(&pane_key()).unwrap().is_none());
        // ACP store stayed empty.
        assert_eq!(events.len(), 0);
    }

    /// Case 11: a disabled global gate skips ACP append, but the
    /// legacy latest-fact write is still recorded.
    #[test]
    fn disabled_global_gate_skips_acp_append_keeps_legacy_write() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        let fact = record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate {
                globally_enabled: false,
                provider_enabled: true,
            },
        )
        .expect("legacy must record");
        assert_eq!(fact.state, AiActivityState::Processing);
        assert!(legacy.read(&pane_key()).unwrap().is_some());
        assert_eq!(events.len(), 0, "ACP must be silent when globally disabled");
    }

    /// Case 12: a disabled per-provider gate skips ACP append for
    /// that provider only.
    #[test]
    fn disabled_provider_gate_skips_acp_for_that_provider_only() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::Codex,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::Codex,
            &legacy,
            &events,
            ActivityControlPlaneGate {
                globally_enabled: true,
                provider_enabled: false,
            },
        )
        .expect("legacy must record");
        assert!(legacy.read(&pane_key()).unwrap().is_some());
        assert_eq!(
            events.len(),
            0,
            "ACP must skip when the trusted provider is disabled"
        );
    }

    /// Case 13: when the ACP event store rejects the append with a
    /// typed I/O error, the legacy path must still succeed and the
    /// caller must receive an `Ok(fact)`.
    #[test]
    fn acp_append_failure_does_not_break_legacy_write() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        events.inject_append_io_error();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        let fact = record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("legacy must still record under ACP failure");
        assert_eq!(fact.state, AiActivityState::Processing);
        assert!(legacy.read(&pane_key()).unwrap().is_some());
        assert_eq!(
            events.len(),
            0,
            "no event must be observed when append injected an error"
        );
    }

    /// Case 14 — regression: when the dual-write helper succeeds end
    /// to end, the legacy latest-fact path produces the same fact
    /// the legacy-only call would have produced.
    #[test]
    fn legacy_latest_fact_write_path_unchanged_on_success() {
        let legacy_only = InMemoryStore::new();
        let legacy_dual = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "UserPromptSubmit",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );

        let legacy_only_fact =
            record_activity(&input, AiActivityProvider::ClaudeCode, &legacy_only)
                .expect("legacy-only must record");
        let dual_fact = record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy_dual,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("dual write must record");

        assert_eq!(legacy_only_fact, dual_fact);
        assert_eq!(
            legacy_only.read(&pane_key()).unwrap(),
            legacy_dual.read(&pane_key()).unwrap()
        );
    }

    /// Bonus: an unknown event name (e.g. `Stop` with an obvious
    /// typo) silently skips the ACP append without surfacing an
    /// error. The legacy path will already have rejected — or
    /// accepted — based on its own mapping; the dual-write helper
    /// only governs the ACP path here.
    #[test]
    fn unknown_event_name_silently_skips_acp_append() {
        let legacy = InMemoryStore::new();
        let events = InMemoryEventStore::new();
        // Use an event the legacy path does not validate ("event"
        // is informational; the legacy store ingests the state
        // directly).
        let input = provider_input(
            AiActivityProvider::ClaudeCode,
            "TotallyFakeEvent",
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            None,
        );
        record_activity_dual_write(
            &input,
            AiActivityProvider::ClaudeCode,
            &legacy,
            &events,
            ActivityControlPlaneGate::all_enabled(),
        )
        .expect("legacy must record (event name is informational)");
        assert_eq!(events.len(), 0);
    }

    /// Bonus: the `ActivityControlPlaneGate::allows_acp_append`
    /// accessor correctly composes the two switches. Pinning the
    /// truth table prevents future refactors from accidentally
    /// loosening fail-closed semantics.
    #[test]
    fn activity_control_plane_gate_allows_acp_append_truth_table() {
        assert!(
            !ActivityControlPlaneGate {
                globally_enabled: false,
                provider_enabled: false
            }
            .allows_acp_append()
        );
        assert!(
            !ActivityControlPlaneGate {
                globally_enabled: false,
                provider_enabled: true
            }
            .allows_acp_append()
        );
        assert!(
            !ActivityControlPlaneGate {
                globally_enabled: true,
                provider_enabled: false
            }
            .allows_acp_append()
        );
        assert!(
            ActivityControlPlaneGate {
                globally_enabled: true,
                provider_enabled: true
            }
            .allows_acp_append()
        );
        assert!(!ActivityControlPlaneGate::all_disabled().allows_acp_append());
        assert!(ActivityControlPlaneGate::all_enabled().allows_acp_append());
    }
}

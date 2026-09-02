use super::{AiActivityOverlayEntry, compute_pane_ai_overlay_entry, resolve_pane_lifecycle_state};
use workhive_core::domain::{
    AiActivityProvider, AiActivitySource, AiIntegrationLifecycleState, PaneKind, PaneState, UnitId,
};
use workhive_core::usecases::ai_integrations::FeatureGate;
use workhive_core::usecases::pane::PaneListRow;

fn list_row(
    active: bool,
    index: u32,
    target_id: &str,
    current_command: &str,
    launch_identity_title: Option<&str>,
) -> PaneListRow {
    PaneListRow {
        active,
        index,
        target_id: UnitId::new(target_id),
        current_command: current_command.to_owned(),
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        activity: workhive_core::domain::PaneActivityState::ShellIdle,
        message_safety: workhive_core::domain::PaneMessageSafety::ShellInput,
        launch_identity_title: launch_identity_title.map(|s| s.to_owned()),
        source: workhive_core::domain::PaneObservationSource::Supervisor,
        confidence: workhive_core::domain::PaneObservationConfidence::Authoritative,
        cwd: "/Users/test/project".to_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn list_row_with_activity(
    active: bool,
    index: u32,
    target_id: &str,
    current_command: &str,
    kind: PaneKind,
    state: PaneState,
    activity: workhive_core::domain::PaneActivityState,
    message_safety: workhive_core::domain::PaneMessageSafety,
    source: workhive_core::domain::PaneObservationSource,
) -> PaneListRow {
    PaneListRow {
        active,
        index,
        target_id: UnitId::new(target_id),
        current_command: current_command.to_owned(),
        kind,
        state,
        activity,
        message_safety,
        launch_identity_title: None,
        source,
        confidence: source.baseline_confidence(),
        cwd: "/Users/test/project".to_owned(),
    }
}

/// Regression contract: the pure per-row overlay helper must
/// attribute architecture-suffixed Codex binaries to
/// [`AiActivityProvider::Codex`] and project a truthful fallback
/// source (`WorkhiveSupervisor`), never `source = None`. The
/// activity label must remain conservative — `open (waiting for
/// event)` for a foreground Codex pane without a fresh hook
/// fact — and not silently claim AI activity.
#[test]
fn compute_pane_ai_overlay_entry_resolves_architecture_suffixed_codex_via_tool_catalog() {
    use workhive_core::domain::Locale;
    use workhive_core::i18n_keys as k;
    use workhive_core::ports::I18nPort;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    for arch_suffixed in [
        "codex-x86_64-apple-darwin",
        "codex-aarch64-apple-darwin",
        "codex-x86_64-unknown-linux-gnu",
        "codex-aarch64-unknown-linux-gnu",
    ] {
        let row = list_row_with_activity(
            true,
            0,
            "%0",
            arch_suffixed,
            PaneKind::Agent,
            PaneState::Running,
            workhive_core::domain::PaneActivityState::AgentForeground,
            workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
            workhive_core::domain::PaneObservationSource::Supervisor,
        );
        let overlay = compute_pane_ai_overlay_entry(
            &row,
            None,
            None,
            0,
            Locale::En,
            &i18n,
            &catalog,
            &FeatureGate::builtin_default(),
        );
        assert_eq!(
            overlay.provider,
            Some(AiActivityProvider::Codex),
            "architecture-suffixed Codex command `{arch_suffixed}` must resolve to provider=Codex"
        );
        assert_eq!(
            overlay.source,
            Some(AiActivitySource::WorkhiveSupervisor),
            "supervisor-observed Codex pane must project source=WorkhiveSupervisor, never None"
        );
        let expected_label = i18n.resolve(Locale::En, k::PANE_AI_ACTIVITY_OPEN_UNVERIFIED);
        assert_eq!(
            overlay.label.as_deref(),
            Some(expected_label.as_str()),
            "no-fact Codex pane must render the conservative open-unverified label"
        );
    }
}

/// Strict-match negative guard at the overlay layer: a
/// catalog-unmodeled lookalike command must collapse to no AI
/// overlay, never sneaking into `AiActivityProvider::Codex`.
#[test]
fn compute_pane_ai_overlay_entry_returns_empty_for_unmodeled_codex_lookalike_command() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    for lookalike in [
        "codex-extra",
        "codex-x86_64-apple-darwin-extra",
        "mycodex",
        "not-codex",
    ] {
        let row = list_row_with_activity(
            true,
            0,
            "%0",
            lookalike,
            PaneKind::Agent,
            PaneState::Running,
            workhive_core::domain::PaneActivityState::AgentForeground,
            workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
            workhive_core::domain::PaneObservationSource::Supervisor,
        );
        let overlay = compute_pane_ai_overlay_entry(
            &row,
            None,
            None,
            0,
            Locale::En,
            &i18n,
            &catalog,
            &FeatureGate::builtin_default(),
        );
        assert!(
            overlay.provider.is_none(),
            "lookalike `{lookalike}` must produce no provider attribution, got {:?}",
            overlay.provider
        );
        assert!(
            overlay.source.is_none(),
            "lookalike `{lookalike}` must produce no source attribution, got {:?}",
            overlay.source
        );
        assert!(
            overlay.label.is_none(),
            "lookalike `{lookalike}` must produce no AI activity label, got {:?}",
            overlay.label
        );
    }
}

/// `pane split --cmd "opencode"` produces a pane whose
/// `current_command == "opencode"` and whose
/// `PaneActivityState::ForegroundProcess` reflects the
/// supervisor's "non-shell foreground but not yet classified as
/// an agent" state before any OpenCode plugin event has arrived.
/// The overlay must:
///
///   * identify the OpenCode provider via the tool catalog,
///   * report a non-`None` source (process/tmux/supervisor — any
///     of the fallback sources is acceptable),
///   * NOT promote source to `OpencodePlugin` (no plugin event).
#[test]
fn compute_pane_ai_overlay_entry_split_opencode_foreground_promotes_source_to_non_none() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();

    let row = list_row_with_activity(
        true,
        0,
        "%0",
        "opencode",
        PaneKind::Terminal,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::ForegroundProcess,
        workhive_core::domain::PaneMessageSafety::ShellInput,
        workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
    );

    let overlay = compute_pane_ai_overlay_entry(
        &row,
        None,
        None,
        0,
        Locale::En,
        &i18n,
        &catalog,
        &FeatureGate::builtin_default(),
    );

    assert_eq!(
        overlay.provider,
        Some(AiActivityProvider::Opencode),
        "split-launched OpenCode must resolve to provider=Opencode via the tool catalog: {overlay:?}"
    );
    let source = overlay.source.expect(
        "split-launched OpenCode without plugin event must report a non-None fallback source",
    );
    assert_ne!(
        source,
        AiActivitySource::OpencodePlugin,
        "no plugin event yet — overlay must NOT promote source to OpencodePlugin: {overlay:?}"
    );
    assert_ne!(
        source,
        AiActivitySource::ClaudeCodeHook,
        "no Claude hook fact — overlay must NOT promote source to ClaudeCodeHook: {overlay:?}"
    );
    assert_ne!(
        source,
        AiActivitySource::CodexHook,
        "no Codex hook fact — overlay must NOT promote source to CodexHook: {overlay:?}"
    );
}

/// Trust-boundary regression: a fresh OpenCode plugin event must
/// still win over the fallback foreground path. The overlay's
/// source must be `OpencodePlugin` so high-confidence telemetry
/// is never demoted by the new fallback rule.
#[test]
fn compute_pane_ai_overlay_entry_fresh_opencode_plugin_event_still_overrides_fallback() {
    use workhive_core::domain::{AiActivityConfidence, AiActivityFact, AiActivityState, Locale};
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();

    let row = list_row_with_activity(
        true,
        0,
        "%0",
        "opencode",
        PaneKind::Agent,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::AgentBusy,
        workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
        workhive_core::domain::PaneObservationSource::Supervisor,
    );

    let fact = AiActivityFact::new(
        AiActivityProvider::Opencode,
        "opencode",
        "test-server",
        "test-workspace",
        "%0",
        AiActivityState::Processing,
        AiActivityConfidence::Plugin,
        0,
        None,
    );

    let overlay = compute_pane_ai_overlay_entry(
        &row,
        Some(&fact),
        None,
        0,
        Locale::En,
        &i18n,
        &catalog,
        &FeatureGate::builtin_default(),
    );

    assert_eq!(
        overlay.provider,
        Some(AiActivityProvider::Opencode),
        "fresh OpenCode plugin event must keep provider=Opencode: {overlay:?}"
    );
    assert_eq!(
        overlay.source,
        Some(AiActivitySource::OpencodePlugin),
        "fresh OpenCode plugin event must elevate source to OpencodePlugin: {overlay:?}"
    );
}

/// Negative regression: a shell pane (no AI tool in the
/// foreground) must still render `source=None` / `confidence=None`.
/// The fallback widening for `ForegroundProcess` must not turn
/// non-AI panes into AI rows.
#[test]
fn compute_pane_ai_overlay_entry_shell_pane_in_foreground_process_stays_none() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();

    let row = list_row_with_activity(
        true,
        0,
        "%0",
        "cargo",
        PaneKind::Terminal,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::ForegroundProcess,
        workhive_core::domain::PaneMessageSafety::ShellInput,
        workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
    );

    let overlay = compute_pane_ai_overlay_entry(
        &row,
        None,
        None,
        0,
        Locale::En,
        &i18n,
        &catalog,
        &FeatureGate::builtin_default(),
    );

    assert!(
        overlay.provider.is_none(),
        "non-AI foreground command must keep provider=None: {overlay:?}"
    );
    assert!(
        overlay.source.is_none(),
        "non-AI foreground command must keep source=None: {overlay:?}"
    );
}

/// Feature-gate regression: when OpenCode is disabled via the
/// per-provider gate, the overlay must stay silent even when a
/// split-launched OpenCode pane is in the foreground. This proves
/// the fallback widening does not bypass the feature gate.
#[test]
fn compute_pane_ai_overlay_entry_split_opencode_stays_silent_when_provider_gate_disabled() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();

    let row = list_row_with_activity(
        true,
        0,
        "%0",
        "opencode",
        PaneKind::Terminal,
        PaneState::Running,
        workhive_core::domain::PaneActivityState::ForegroundProcess,
        workhive_core::domain::PaneMessageSafety::ShellInput,
        workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
    );

    // Disable OpenCode only; keep the global switch and other
    // providers enabled so the test isolates the per-provider gate.
    let gate_without_opencode = FeatureGate::from_switches(
        true,  // globally enabled
        true,  // ClaudeCode enabled
        true,  // Codex enabled
        false, // OpenCode disabled
    );

    let overlay = compute_pane_ai_overlay_entry(
        &row,
        None,
        None,
        0,
        Locale::En,
        &i18n,
        &catalog,
        &gate_without_opencode,
    );

    assert!(
        overlay.provider.is_none(),
        "OpenCode gated off must keep provider=None even on the fallback path: {overlay:?}"
    );
    assert!(
        overlay.source.is_none(),
        "OpenCode gated off must keep source=None even on the fallback path: {overlay:?}"
    );
}

/// Lifecycle resolver direct contract test: Codex with no
/// accurate event → `ReviewRequired`; Claude with no accurate
/// event → `PendingFirstEvent`; Codex with hook event →
/// `Active`. Locks the wiring between pane overlay shape and
/// the typed lifecycle vocabulary.
#[test]
fn resolve_pane_lifecycle_state_drives_expected_states() {
    let codex_overlay = AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    };
    assert_eq!(
        resolve_pane_lifecycle_state(AiActivityProvider::Codex, Some(&codex_overlay)),
        AiIntegrationLifecycleState::ReviewRequired,
    );
    let claude_overlay = AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::ClaudeCode),
        source: Some(AiActivitySource::WorkhiveSupervisor),
        ..AiActivityOverlayEntry::default()
    };
    assert_eq!(
        resolve_pane_lifecycle_state(AiActivityProvider::ClaudeCode, Some(&claude_overlay)),
        AiIntegrationLifecycleState::PendingFirstEvent,
    );
    let codex_active = AiActivityOverlayEntry {
        provider: Some(AiActivityProvider::Codex),
        source: Some(AiActivitySource::CodexHook),
        stale: false,
        ..AiActivityOverlayEntry::default()
    };
    assert_eq!(
        resolve_pane_lifecycle_state(AiActivityProvider::Codex, Some(&codex_active)),
        AiIntegrationLifecycleState::Active,
    );
}

/// Projection-contract test: the pane-scope projection must never
/// report `Installed` for any provider/overlay shape it can
/// produce. `Installed` is reserved for provider-scope observations
/// (handled by the core resolver). If this module ever silently
/// drops `observes_pane_event = true` this assertion catches the
/// regression.
#[test]
fn pane_scope_projection_never_returns_installed() {
    for provider in [
        AiActivityProvider::ClaudeCode,
        AiActivityProvider::Codex,
        AiActivityProvider::Opencode,
    ] {
        let no_overlay = resolve_pane_lifecycle_state(provider, None);
        assert_ne!(
            no_overlay,
            AiIntegrationLifecycleState::Installed,
            "pane-scope (no overlay) must skip Installed for {provider:?}",
        );

        let supervisor_only = AiActivityOverlayEntry {
            provider: Some(provider),
            source: Some(AiActivitySource::WorkhiveSupervisor),
            ..AiActivityOverlayEntry::default()
        };
        let supervisor_state = resolve_pane_lifecycle_state(provider, Some(&supervisor_only));
        assert_ne!(
            supervisor_state,
            AiIntegrationLifecycleState::Installed,
            "pane-scope (supervisor-only overlay) must skip Installed for {provider:?}",
        );

        let hook_source = match provider {
            AiActivityProvider::ClaudeCode => Some(AiActivitySource::ClaudeCodeHook),
            AiActivityProvider::Codex => Some(AiActivitySource::CodexHook),
            AiActivityProvider::Opencode => Some(AiActivitySource::OpencodePlugin),
            AiActivityProvider::None => None,
        };
        let with_event = AiActivityOverlayEntry {
            provider: Some(provider),
            source: hook_source,
            stale: false,
            ..AiActivityOverlayEntry::default()
        };
        let active_state = resolve_pane_lifecycle_state(provider, Some(&with_event));
        assert_ne!(
            active_state,
            AiIntegrationLifecycleState::Installed,
            "pane-scope (hook/plugin event overlay) must skip Installed for {provider:?}",
        );
    }
}

/// When the feature gate disables a provider, the activity
/// overlay for matching panes must be empty so neither the
/// `source` column nor the `pane show` AI block surfaces any
/// hook/plugin guidance. This is the "no read/no display" half
/// of the silent-when-disabled contract.
#[test]
fn compute_pane_ai_overlay_entry_returns_empty_for_gated_off_provider() {
    use workhive_core::domain::Locale;
    use workhive_infra::i18n::StaticI18n;
    let i18n = StaticI18n::new();
    let catalog = workhive_core::domain::ToolCatalog::builtin_defaults();
    let row = list_row(true, 0, "%0", "codex", Some("codex"));
    let gate = FeatureGate::from_switches(true, true, false, true);
    let overlay =
        compute_pane_ai_overlay_entry(&row, None, None, 0, Locale::En, &i18n, &catalog, &gate);
    assert!(
        overlay.provider.is_none() && overlay.source.is_none() && overlay.label.is_none(),
        "gated-off provider must surface a fully empty overlay entry, got {overlay:?}"
    );
}

mod acp_overlay_bridge_tests {
    use crate::commands::pane_activity_overlay::{
        build_acp_overlay_map, build_acp_overlay_map_from_read_model,
        build_acp_overlay_map_from_store,
    };
    use camino::Utf8PathBuf;
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventId, ActivityOutcome, ObservationConfidence, PaneActivityFact,
        PaneActivityState as AcpPaneActivityState, PaneCorrelationKey, ProviderKind,
        ProviderSignalSource, ReadModel, SignalChannel, SignalFreshness,
    };
    use workhive_core::domain::{
        AiActivityProvider, AiActivitySource, Locale, PaneKind, PaneState, UnitId,
    };
    use workhive_core::ports::{ActivityEventStore, ActivityEventStoreError};
    use workhive_core::usecases::ai_integrations::FeatureGate;
    use workhive_core::usecases::pane::PaneListRow;
    use workhive_infra::i18n::StaticI18n;
    use workhive_testkit::fakes::{ActivityEventStoreErrorInjection, InMemoryActivityEventStore};

    const SERVER: &str = "whsrv-test";
    const WORKSPACE: &str = "ws-main";

    fn acp_row(target_id: &str) -> PaneListRow {
        PaneListRow {
            active: true,
            index: 0,
            target_id: UnitId::new(target_id),
            current_command: "codex".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            activity: workhive_core::domain::PaneActivityState::AgentForeground,
            message_safety: workhive_core::domain::PaneMessageSafety::UnverifiedAgent,
            launch_identity_title: None,
            source: workhive_core::domain::PaneObservationSource::TmuxCurrentCommand,
            confidence: workhive_core::domain::PaneObservationConfidence::Fallback,
            cwd: "/Users/test/project".to_owned(),
        }
    }

    fn key(target_id: &str) -> PaneCorrelationKey {
        PaneCorrelationKey::new(SERVER.to_owned(), WORKSPACE.to_owned(), target_id)
    }

    fn fact(
        target_id: &str,
        provider: ProviderKind,
        state: AcpPaneActivityState,
        channel: SignalChannel,
        confidence: ObservationConfidence,
        freshness: SignalFreshness,
    ) -> PaneActivityFact {
        PaneActivityFact {
            pane: key(target_id),
            provider: Some(provider),
            state,
            outcome: ActivityOutcome::NotApplicable,
            confidence,
            channel,
            observed_at_unix_ms: 0,
            freshness,
            source_event_id: None,
        }
    }

    fn read_model_with_facts(facts: Vec<PaneActivityFact>) -> ReadModel {
        let mut rm = ReadModel::empty(0);
        for f in facts {
            rm.upsert(f);
        }
        rm
    }

    /// Phase-2 happy path. A `Fresh` + `High`-confidence lifecycle
    /// hook fact for Claude Code drives the overlay directly from
    /// the ACP projection. Confirms label localisation, source,
    /// provider, confidence, and `stale = false` are all wired
    /// through from the bridge, not from the legacy latest-fact
    /// path.
    #[test]
    fn acp_high_confidence_lifecycle_fact_drives_overlay_state_and_source() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rm = read_model_with_facts(vec![fact(
            "%0",
            ProviderKind::ClaudeCode,
            AcpPaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        )]);
        let map = build_acp_overlay_map_from_read_model(
            &rm,
            std::slice::from_ref(&row),
            SERVER,
            WORKSPACE,
            Locale::En,
            &i18n,
            &FeatureGate::builtin_default(),
        );
        let entry = map
            .get(&key("%0"))
            .expect("ACP overlay must drive this row");
        assert_eq!(
            entry.label.as_deref(),
            Some("processing"),
            "Working must localize to the legacy `processing` label"
        );
        assert_eq!(entry.provider, Some(AiActivityProvider::ClaudeCode));
        assert_eq!(entry.source, Some(AiActivitySource::ClaudeCodeHook));
        assert_eq!(
            entry.confidence,
            Some(workhive_core::domain::AiActivityConfidence::Hook)
        );
        assert!(!entry.stale, "Fresh facts must not render as stale");
    }

    /// Codex `notify`-completion (Medium + NotifyCallback +
    /// Completed) MUST render the Completed overlay (legacy
    /// `WaitingForUser`) and never Working / ToolRunning.
    #[test]
    fn acp_notify_completion_renders_completed_never_working() {
        let i18n = StaticI18n::new();
        let row = acp_row("%1");
        let rm = read_model_with_facts(vec![fact(
            "%1",
            ProviderKind::Codex,
            AcpPaneActivityState::Completed,
            SignalChannel::NotifyCallback,
            ObservationConfidence::Medium,
            SignalFreshness::Fresh,
        )]);
        let map = build_acp_overlay_map_from_read_model(
            &rm,
            std::slice::from_ref(&row),
            SERVER,
            WORKSPACE,
            Locale::En,
            &i18n,
            &FeatureGate::builtin_default(),
        );
        let entry = map
            .get(&key("%1"))
            .expect("notify-completion must drive overlay");
        assert_eq!(
            entry.label.as_deref(),
            Some("waiting for user"),
            "Completed must localize to legacy `waiting for user`"
        );
        assert_ne!(entry.label.as_deref(), Some("processing"));
        assert_ne!(entry.label.as_deref(), Some("tool running"));
        assert_eq!(entry.provider, Some(AiActivityProvider::Codex));
        assert_eq!(entry.source, Some(AiActivitySource::WorkhiveSupervisor));
        assert_eq!(
            entry.confidence,
            Some(workhive_core::domain::AiActivityConfidence::Fallback)
        );
    }

    /// Stale facts and low/medium-low confidence facts MUST be
    /// absent from the ACP overlay map so the caller falls back
    /// to the legacy latest-fact overlay (which has its own
    /// stale-hint logic).
    #[test]
    fn acp_low_confidence_or_stale_falls_back_to_legacy_overlay() {
        let i18n = StaticI18n::new();
        let stale_row = acp_row("%stale");
        let low_row = acp_row("%low");
        let rm = read_model_with_facts(vec![
            fact(
                "%stale",
                ProviderKind::ClaudeCode,
                AcpPaneActivityState::Working,
                SignalChannel::LifecycleHook,
                ObservationConfidence::High,
                SignalFreshness::Stale,
            ),
            fact(
                "%low",
                ProviderKind::ClaudeCode,
                AcpPaneActivityState::Working,
                SignalChannel::LifecycleHook,
                ObservationConfidence::Low,
                SignalFreshness::Fresh,
            ),
        ]);
        let rows = vec![stale_row, low_row];
        let map = build_acp_overlay_map_from_read_model(
            &rm,
            &rows,
            SERVER,
            WORKSPACE,
            Locale::En,
            &i18n,
            &FeatureGate::builtin_default(),
        );
        assert!(
            !map.contains_key(&key("%stale")),
            "stale facts must fall back to legacy overlay path"
        );
        assert!(
            !map.contains_key(&key("%low")),
            "low-confidence facts must fall back to legacy overlay path"
        );
    }

    /// When the projector rebuild fails for ANY of the typed
    /// store errors (Io / CorruptEntry / Unavailable), the CLI
    /// helper must log at `debug` and return `None` so every
    /// row falls back to the legacy latest-fact path. This is
    /// the "broken event log never blocks `pane list`" guarantee
    /// from the error-handling policy.
    #[test]
    fn acp_corrupt_or_unavailable_store_falls_back_to_legacy_display() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rows = std::slice::from_ref(&row);
        let gate = FeatureGate::builtin_default();
        for error in [
            ActivityEventStoreError::Io("simulated".to_owned()),
            ActivityEventStoreError::CorruptEntry("simulated".to_owned()),
            ActivityEventStoreError::Unavailable("simulated".to_owned()),
        ] {
            let store = InMemoryActivityEventStore::new();
            store.set_error_injection(ActivityEventStoreErrorInjection {
                replay_all_error: Some(error),
                ..Default::default()
            });
            let result = build_acp_overlay_map_from_store(
                &gate,
                &store,
                1_000,
                SERVER,
                WORKSPACE,
                rows,
                Locale::En,
                &i18n,
            );
            assert!(
                result.is_none(),
                "rebuild error must cause the helper to return None (legacy fallback)"
            );
        }
    }

    /// When the AI integrations gate is globally disabled the
    /// helper must skip the ACP read path entirely — no store
    /// I/O, no projection. Lock this by injecting a typed
    /// `Unavailable` error: if the helper consulted the store
    /// the error would still produce `None`, but the gate path
    /// runs *before* construction so we cannot tell apart at
    /// the return value alone. We instead exercise the
    /// behavioral contract: an enabled gate with the same
    /// empty in-memory store returns `Some(empty map)`; a
    /// disabled gate returns `None`.
    #[test]
    fn globally_disabled_gate_skips_acp_reads() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rows = std::slice::from_ref(&row);
        let store = InMemoryActivityEventStore::new();
        let with_gate = build_acp_overlay_map_from_store(
            &FeatureGate::builtin_default(),
            &store,
            1_000,
            SERVER,
            WORKSPACE,
            rows,
            Locale::En,
            &i18n,
        );
        assert!(
            with_gate.is_some(),
            "gate-enabled empty store must return Some(map), got None"
        );
        let disabled = FeatureGate::from_switches(false, true, true, true);
        let without_gate = build_acp_overlay_map_from_store(
            &disabled,
            &store,
            1_000,
            SERVER,
            WORKSPACE,
            rows,
            Locale::En,
            &i18n,
        );
        assert!(
            without_gate.is_none(),
            "globally-disabled gate must skip ACP reads entirely"
        );
    }

    /// Per-provider disable: when the global gate is ON but the
    /// row's provider is disabled, the ACP overlay map must NOT
    /// contain that row so the legacy `compute_pane_ai_overlay_entry`
    /// path is exercised (which already enforces the silent
    /// behavior). This preserves the same "completely silent
    /// when provider-disabled" contract on both code paths.
    #[test]
    fn provider_disabled_gate_falls_back_to_legacy_for_that_provider() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rm = read_model_with_facts(vec![fact(
            "%0",
            ProviderKind::ClaudeCode,
            AcpPaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        )]);
        let gate = FeatureGate::from_switches(true, false, true, true);
        let map = build_acp_overlay_map_from_read_model(
            &rm,
            std::slice::from_ref(&row),
            SERVER,
            WORKSPACE,
            Locale::En,
            &i18n,
            &gate,
        );
        assert!(
            !map.contains_key(&key("%0")),
            "provider-disabled row must NOT appear in the ACP overlay map",
        );
    }

    /// Smoke check on the production wrapper itself: an empty
    /// runtime directory must not cause the helper to error,
    /// and the result is `Some(empty map)` so the renderer
    /// proceeds row-by-row to the legacy path. Exercises the
    /// production composition path: the wrapper routes store
    /// construction through
    /// [`crate::composition::CliComposition::activity_event_store`],
    /// which derives `<project>/.workhive/runtime` and hands
    /// back a real `FsActivityEventStore`.
    #[test]
    fn build_acp_overlay_map_returns_some_empty_for_pristine_runtime_root() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rows = std::slice::from_ref(&row);
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("tempdir is utf8");
        let composition = crate::composition::CliComposition::new();
        let result = build_acp_overlay_map(
            &composition,
            &FeatureGate::builtin_default(),
            &project_root,
            1_000,
            SERVER,
            WORKSPACE,
            rows,
            Locale::En,
            &i18n,
        );
        let map = result.expect("pristine project root must yield Some(empty)");
        assert!(map.is_empty(), "no events yet → no ACP overlay entries");
    }

    /// Belt-and-suspenders: the bridge maps `ProviderKind::Future`
    /// to a fall-through, so even a `Fresh` + `High` event for a
    /// future provider must NOT enter the ACP overlay map.
    #[test]
    fn future_provider_does_not_appear_in_acp_overlay_map() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rm = read_model_with_facts(vec![fact(
            "%0",
            ProviderKind::Future,
            AcpPaneActivityState::Working,
            SignalChannel::LifecycleHook,
            ObservationConfidence::High,
            SignalFreshness::Fresh,
        )]);
        let map = build_acp_overlay_map_from_read_model(
            &rm,
            std::slice::from_ref(&row),
            SERVER,
            WORKSPACE,
            Locale::En,
            &i18n,
            &FeatureGate::builtin_default(),
        );
        assert!(map.is_empty(), "Future provider must not drive overlay");
    }

    /// Lifecycle smoke test: write a normalized `ActivityEvent`
    /// into the in-memory store and assert the bridge yields the
    /// corresponding overlay. This is the closest the CLI test
    /// layer gets to an end-to-end check without standing up the
    /// JSONL adapter or tmux.
    #[test]
    fn store_with_normalized_lifecycle_event_drives_acp_overlay() {
        let i18n = StaticI18n::new();
        let row = acp_row("%0");
        let rows = std::slice::from_ref(&row);
        let store = InMemoryActivityEventStore::new();
        let event = ActivityEvent {
            id: ActivityEventId::new_v4(),
            pane: key("%0"),
            provider: ProviderKind::ClaudeCode,
            channel: SignalChannel::LifecycleHook,
            source: ProviderSignalSource::LifecycleHook,
            kind: workhive_core::domain::activity::ActivityEventKind::ToolStarted,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::High,
            observed_at_unix_ms: 1_000,
            detail: None,
        };
        store.append(&event).expect("append");
        let map = build_acp_overlay_map_from_store(
            &FeatureGate::builtin_default(),
            &store,
            2_000,
            SERVER,
            WORKSPACE,
            rows,
            Locale::En,
            &i18n,
        )
        .expect("rebuild must succeed");
        let entry = map
            .get(&key("%0"))
            .expect("event must produce ACP overlay row");
        let label = entry.label.as_deref().unwrap_or("");
        assert!(
            label == "processing" || label == "tool running",
            "expected a busy label, got {label:?}"
        );
        assert_eq!(entry.source, Some(AiActivitySource::ClaudeCodeHook));
    }
}

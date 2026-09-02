use std::collections::BTreeSet;

use super::*;

#[test]
fn pane_direction_display() {
    assert_eq!(PaneDirection::Right.to_string(), "right");
    assert_eq!(PaneDirection::Down.to_string(), "down");
    assert_eq!(PaneDirection::Left.to_string(), "left");
    assert_eq!(PaneDirection::Up.to_string(), "up");
}

#[test]
fn pane_direction_serde_round_trip_covers_all_four_variants() {
    for dir in [
        PaneDirection::Right,
        PaneDirection::Down,
        PaneDirection::Left,
        PaneDirection::Up,
    ] {
        let json = serde_json::to_string(&dir).expect("serialize");
        let restored: PaneDirection = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(dir, restored);
    }
}

#[test]
fn pane_direction_before_target_flag_matches_semantics() {
    assert!(!PaneDirection::Right.is_before_target());
    assert!(!PaneDirection::Down.is_before_target());
    assert!(PaneDirection::Left.is_before_target());
    assert!(PaneDirection::Up.is_before_target());
}

#[test]
fn pane_direction_vertical_predicate_matches_semantics() {
    assert!(!PaneDirection::Right.is_vertical());
    assert!(!PaneDirection::Left.is_vertical());
    assert!(PaneDirection::Down.is_vertical());
    assert!(PaneDirection::Up.is_vertical());
}

#[test]
fn pane_kind_default_is_unknown() {
    assert_eq!(PaneKind::default(), PaneKind::Unknown);
}

#[test]
fn pane_state_default_is_unknown() {
    assert_eq!(PaneState::default(), PaneState::Unknown);
}

#[test]
fn pane_runtime_id_derive_is_deterministic() {
    let a = PaneRuntimeId::derive("server-1", "feat-auth", "%0");
    let b = PaneRuntimeId::derive("server-1", "feat-auth", "%0");
    assert_eq!(a, b);
    assert!(a.as_str().starts_with("prt:"));
}

#[test]
fn pane_runtime_id_different_groups_do_not_alias() {
    let a = PaneRuntimeId::derive("group-a", "workspace", "%1");
    let b = PaneRuntimeId::derive("group-b", "workspace", "%1");
    assert_ne!(
        a, b,
        "same workspace+unit_id in different groups must not alias"
    );
}

#[test]
fn pane_runtime_id_different_workspaces_do_not_alias() {
    let a = PaneRuntimeId::derive("srv", "ws-alpha", "%0");
    let b = PaneRuntimeId::derive("srv", "ws-beta", "%0");
    assert_ne!(
        a, b,
        "same group+unit_id in different workspaces must not alias"
    );
}

#[test]
fn pane_runtime_id_different_units_do_not_alias() {
    let a = PaneRuntimeId::derive("server", "ws", "%0");
    let b = PaneRuntimeId::derive("server", "ws", "%1");
    assert_ne!(a, b);
}

#[test]
fn pane_runtime_id_boundary_shift_does_not_alias() {
    // "ab" + "c" vs "a" + "bc" — length-prefix encoding prevents
    // boundary ambiguity that naive concatenation would allow.
    let a = PaneRuntimeId::derive("ab", "c", "x");
    let b = PaneRuntimeId::derive("a", "bc", "x");
    assert_ne!(
        a, b,
        "shifting characters across field boundaries must not alias"
    );
}

#[test]
fn pane_runtime_id_empty_components_do_not_alias() {
    let a = PaneRuntimeId::derive("", "ws", "%0");
    let b = PaneRuntimeId::derive("ws", "", "%0");
    assert_ne!(a, b, "empty group vs empty workspace must not alias");

    let c = PaneRuntimeId::derive("", "", "%0");
    assert_ne!(a, c);
    assert_ne!(b, c);
}

#[test]
fn pane_runtime_id_colons_in_values_do_not_alias() {
    let a = PaneRuntimeId::derive("g:1", "ws", "%0");
    let b = PaneRuntimeId::derive("g", "1:ws", "%0");
    assert_ne!(a, b, "colons inside values must not cause aliasing");
}

#[test]
fn pane_runtime_id_from_raw_round_trip() {
    let original = PaneRuntimeId::derive("g", "w", "u");
    let raw = original.as_str().to_owned();
    let restored = PaneRuntimeId::from_raw(raw);
    assert_eq!(original, restored);
}

#[test]
fn pane_runtime_id_serialization_round_trip() {
    let id = PaneRuntimeId::derive("srv", "main", "%0");
    let json = serde_json::to_string(&id).unwrap();
    let restored: PaneRuntimeId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, restored);
    assert!(json.contains("prt:"));
}

#[test]
fn pane_runtime_status_default_is_unknown() {
    assert_eq!(PaneRuntimeStatus::default(), PaneRuntimeStatus::Unknown);
}

fn make_test_descriptor(group: &str, workspace: &str, unit: &str) -> PaneDescriptor {
    PaneDescriptor {
        runtime_id: PaneRuntimeId::derive(group, workspace, unit),
        group: group.to_owned(),
        workspace: workspace.to_owned(),
        unit_id: UnitId::new(unit),
        cwd: "/tmp".to_owned(),
        command: "zsh".to_owned(),
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        observed_status: PaneRuntimeStatus::Unknown,
        role: None,
        launch_origin: PaneLaunchOrigin::EmptyShell,
        capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
    }
}

#[test]
fn pane_descriptor_round_trip() {
    let desc = make_test_descriptor("srv", "feat-auth", "%0");
    let json = serde_json::to_string(&desc).unwrap();
    let restored: PaneDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.runtime_id, desc.runtime_id);
    assert_eq!(restored.group, "srv");
    assert_eq!(restored.unit_id, UnitId::new("%0"));
    assert_eq!(restored.capabilities.len(), 1);
    assert!(
        !json.contains("backend_id"),
        "backend_id must not leak into descriptor"
    );
}

#[test]
fn pane_descriptor_carries_routing_context() {
    let desc = make_test_descriptor("grp", "ws", "%2");
    assert_eq!(desc.group, "grp");
    assert_eq!(desc.workspace, "ws");
    assert_eq!(desc.unit_id, UnitId::new("%2"));
}

#[test]
fn pane_descriptor_role_is_optional() {
    let desc = make_test_descriptor("g", "ws", "%0");
    assert_eq!(desc.role, None);
}

#[test]
fn pane_descriptor_with_role() {
    let mut desc = make_test_descriptor("g", "ws", "%0");
    desc.role = Some(PaneRole::Primary);
    let json = serde_json::to_string(&desc).unwrap();
    let restored: PaneDescriptor = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.role, Some(PaneRole::Primary));
}

#[test]
fn pane_descriptor_capabilities_deterministic() {
    let desc = make_test_descriptor("g", "ws", "%0");
    let caps_vec: Vec<_> = desc.capabilities.iter().cloned().collect();
    let caps_vec2: Vec<_> = desc.capabilities.iter().cloned().collect();
    assert_eq!(
        caps_vec, caps_vec2,
        "BTreeSet must yield deterministic order"
    );
}

#[test]
fn pane_event_serialization() {
    let event = PaneEvent::Discovered {
        runtime_id: PaneRuntimeId::derive("s", "main", "%0"),
        descriptor: make_test_descriptor("s", "main", "%0"),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"discovered\""));
}

#[test]
fn workspace_interaction_mode_default_is_manual() {
    assert_eq!(
        WorkspaceInteractionMode::default(),
        WorkspaceInteractionMode::Manual
    );
}

#[test]
fn pane_role_serde_round_trip() {
    let roles = [
        PaneRole::Primary,
        PaneRole::Worker,
        PaneRole::Utility,
        PaneRole::Viewer,
    ];
    for role in &roles {
        let json = serde_json::to_string(role).unwrap();
        let restored: PaneRole = serde_json::from_str(&json).unwrap();
        assert_eq!(*role, restored);
    }
}

#[test]
fn pane_lifecycle_event_serde_round_trip() {
    let events = [
        PaneLifecycleEvent::Registered {
            pane: PaneRuntimeId::derive("g", "ws", "%0"),
        },
        PaneLifecycleEvent::Updated {
            pane: PaneRuntimeId::derive("g", "ws", "%0"),
        },
        PaneLifecycleEvent::StatusChanged {
            pane: PaneRuntimeId::derive("g", "ws", "%0"),
            from: PaneRuntimeStatus::Unknown,
            to: PaneRuntimeStatus::Running,
        },
        PaneLifecycleEvent::Unregistered {
            pane: PaneRuntimeId::derive("g", "ws", "%0"),
        },
    ];
    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        let restored: PaneLifecycleEvent = serde_json::from_str(&json).unwrap();
        // verify round-trip by re-serializing
        assert_eq!(json, serde_json::to_string(&restored).unwrap());
    }
}

// ── derive_capabilities ───────────────────────────────────────────────

#[test]
fn derive_capabilities_terminal_gets_command_executor() {
    let caps = derive_capabilities(PaneKind::Terminal);
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert_eq!(caps.len(), 1);
}

#[test]
fn derive_capabilities_agent_gets_full_message_set() {
    let caps = derive_capabilities(PaneKind::Agent);
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn derive_capabilities_git_ui_gets_tui() {
    let caps = derive_capabilities(PaneKind::GitUi);
    assert!(caps.contains(&PaneCapability::Tui));
    assert_eq!(caps.len(), 1);
}

#[test]
fn derive_capabilities_unknown_is_empty() {
    let caps = derive_capabilities(PaneKind::Unknown);
    assert!(caps.is_empty());
}

#[test]
fn derive_capabilities_is_deterministic() {
    let a = derive_capabilities(PaneKind::Terminal);
    let b = derive_capabilities(PaneKind::Terminal);
    assert_eq!(a, b, "same kind must always produce same capabilities");
}

#[test]
fn capability_no_is_ai_variant_present() {
    // Static assertion: verify the variants compile but do not include
    // is_ai, Agent, Shell, or Custom (all forbidden by spec).
    let caps = [
        PaneCapability::MessageReceiver,
        PaneCapability::MessageResponder,
        PaneCapability::CommandExecutor,
        PaneCapability::Tui,
        PaneCapability::ObserverOnly,
    ];
    // All five canonical variants must be serializable to snake_case.
    for cap in &caps {
        let json = serde_json::to_string(cap).unwrap();
        assert!(!json.contains("is_ai"));
        assert!(!json.contains("agent"));
        assert!(!json.contains("shell"));
        assert!(!json.contains("custom"));
    }
}

#[test]
fn capability_routing_uses_capabilities_not_names() {
    // Demonstrate that routing logic should check capabilities, not names.
    // A terminal and an agent pane should be differentiated solely by caps.
    let terminal_caps = derive_capabilities(PaneKind::Terminal);
    let agent_caps = derive_capabilities(PaneKind::Agent);

    // Terminal cannot respond to messages
    assert!(!terminal_caps.contains(&PaneCapability::MessageResponder));
    // Agent can
    assert!(agent_caps.contains(&PaneCapability::MessageResponder));

    // git UI pane must not be routed messages
    let gitui_caps = derive_capabilities(PaneKind::GitUi);
    assert!(!gitui_caps.contains(&PaneCapability::MessageReceiver));
    assert!(!gitui_caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn pane_message_payload_variants() {
    let text = PaneMessagePayload::Text {
        content: "ls -la".to_owned(),
    };
    let json = serde_json::to_string(&text).unwrap();
    assert!(json.contains("\"kind\":\"text\""));

    let cmd = PaneMessagePayload::Command {
        name: "run".to_owned(),
        args: vec!["test".to_owned()],
    };
    let json = serde_json::to_string(&cmd).unwrap();
    assert!(json.contains("\"kind\":\"command\""));
}

// ── Structural: no is_ai field in pane/orchestration types ────────

#[test]
fn no_is_ai_field_in_pane_descriptor() {
    let desc = make_test_descriptor("g", "ws", "%0");
    let json = serde_json::to_string(&desc).unwrap();
    assert!(
        !json.contains("is_ai"),
        "PaneDescriptor must not contain is_ai field"
    );
}

// ── Structural: traditional vs primary-assisted type distinction ──

#[test]
fn traditional_and_primary_assisted_are_distinct_enum_variants() {
    let manual = WorkspaceInteractionMode::Manual;
    let primary = WorkspaceInteractionMode::PrimaryAssisted;
    assert_ne!(manual, primary);
    let manual_json = serde_json::to_string(&manual).unwrap();
    let primary_json = serde_json::to_string(&primary).unwrap();
    assert_ne!(manual_json, primary_json);
}

#[test]
fn default_mode_is_traditional_not_primary_assisted() {
    let mode = WorkspaceInteractionMode::default();
    assert_eq!(mode, WorkspaceInteractionMode::Manual);
    assert_ne!(mode, WorkspaceInteractionMode::PrimaryAssisted);
}

// ── Structural: types support async bus evolution ─────────────────

#[test]
fn pane_message_has_serializable_payload() {
    let msg = PaneMessage {
        from: Some(PaneRuntimeId::derive("g", "ws", "%0")),
        to: PaneRuntimeId::derive("g", "ws", "%1"),
        payload: PaneMessagePayload::Text {
            content: "hello".to_owned(),
        },
    };
    let json = serde_json::to_string(&msg).unwrap();
    let restored: PaneMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.to, msg.to);
}

#[test]
fn pane_message_payload_supports_multiple_delivery_patterns() {
    let payloads = [
        PaneMessagePayload::Text {
            content: "hi".to_owned(),
        },
        PaneMessagePayload::Command {
            name: "build".to_owned(),
            args: vec!["--release".to_owned()],
        },
        PaneMessagePayload::Signal {
            signal: "SIGINT".to_owned(),
        },
    ];
    for p in &payloads {
        let json = serde_json::to_string(p).unwrap();
        let restored: PaneMessagePayload = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&restored).unwrap();
        assert_eq!(json, json2);
    }
}

#[test]
fn pane_event_supports_capability_change_tracking() {
    let event = PaneEvent::CapabilityChanged {
        runtime_id: PaneRuntimeId::derive("g", "ws", "%0"),
        added: [PaneCapability::MessageReceiver].into_iter().collect(),
        removed: BTreeSet::new(),
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("capability_changed"));
}

#[test]
fn pane_runtime_status_supports_blocked_and_exited_for_async() {
    let statuses = [PaneRuntimeStatus::Blocked, PaneRuntimeStatus::Exited];
    for status in &statuses {
        let json = serde_json::to_string(status).unwrap();
        let restored: PaneRuntimeStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(*status, restored);
    }
}

// ── derive_runtime_capabilities — capability honest to observation source ──
//
// These tests pin the honest-capability contract: an agent kind
// claims structured-message capabilities ONLY when Workhive's
// supervisor authoritatively observed it. Every other source
// (OS observer / tmux fallback / unknown) is treated as
// unverified — `MessageReceiver` is withheld.

use crate::domain::observation::{PaneObservationConfidence, PaneObservationSource};

#[test]
fn runtime_capabilities_terminal_is_command_executor_regardless_of_source() {
    for source in [
        PaneObservationSource::Supervisor,
        PaneObservationSource::ProcessObserver,
        PaneObservationSource::TmuxCurrentCommand,
        PaneObservationSource::Unknown,
    ] {
        for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
            let confidence = source.baseline_confidence();
            let caps = derive_runtime_capabilities(PaneKind::Terminal, state, source, confidence);
            assert_eq!(
                caps,
                [PaneCapability::CommandExecutor].into_iter().collect(),
                "terminal pane caps must be CommandExecutor only \
                     (state={state:?}, source={source:?})"
            );
            assert!(!caps.contains(&PaneCapability::MessageReceiver));
        }
    }
}

#[test]
fn runtime_capabilities_supervised_running_agent_gets_full_message_set() {
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Running,
        PaneObservationSource::Supervisor,
        PaneObservationConfidence::Authoritative,
    );
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert!(caps.contains(&PaneCapability::MessageReceiver));
    assert!(caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn runtime_capabilities_supervised_idle_agent_drops_message_receiver() {
    // Regression: the agent process is
    // gone, even though we authoritatively knew its identity
    // moments ago. The structured channel is no longer live, so
    // the registry must not advertise MessageReceiver/Responder.
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Idle,
        PaneObservationSource::Supervisor,
        PaneObservationConfidence::Authoritative,
    );
    assert_eq!(
        caps,
        [PaneCapability::CommandExecutor].into_iter().collect(),
        "supervisor authoritative + Idle agent must drop \
             structured-message capabilities"
    );
    assert!(!caps.contains(&PaneCapability::MessageReceiver));
    assert!(!caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn runtime_capabilities_supervised_unknown_state_agent_drops_message_receiver() {
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Unknown,
        PaneObservationSource::Supervisor,
        PaneObservationConfidence::Authoritative,
    );
    assert!(!caps.contains(&PaneCapability::MessageReceiver));
}

#[test]
fn runtime_capabilities_unverified_agent_via_os_observer_drops_message_receiver() {
    // Foreground basename is `codex` / `claude` and the OS
    // process observer recognised it, but Workhive's supervisor
    // never confirmed a handshake. We must NOT auto-promote the
    // pane to a structured-message receiver: the user might have
    // started codex manually in a Workhive-managed pane, and the
    // pane control plane has no way to address it safely.
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Running,
        PaneObservationSource::ProcessObserver,
        PaneObservationConfidence::High,
    );
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert!(
        !caps.contains(&PaneCapability::MessageReceiver),
        "unverified agent pane (no supervisor handshake) must NOT \
             advertise MessageReceiver capability"
    );
    assert!(!caps.contains(&PaneCapability::MessageResponder));
}

#[test]
fn runtime_capabilities_unverified_agent_via_tmux_fallback_drops_message_receiver() {
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Running,
        PaneObservationSource::TmuxCurrentCommand,
        PaneObservationConfidence::Fallback,
    );
    assert!(caps.contains(&PaneCapability::CommandExecutor));
    assert!(
        !caps.contains(&PaneCapability::MessageReceiver),
        "tmux-fallback agent observation must not promote a pane \
             to MessageReceiver"
    );
}

#[test]
fn runtime_capabilities_supervisor_below_authoritative_does_not_promote_agent() {
    // Defensive: an adapter that downgraded a supervisor
    // observation below Authoritative (e.g. metadata exists but
    // pid is gone) MUST NOT count as a handshake. Code that
    // claims `MessageReceiver` here is exactly the bug this
    // test prevents.
    let caps = derive_runtime_capabilities(
        PaneKind::Agent,
        PaneState::Running,
        PaneObservationSource::Supervisor,
        PaneObservationConfidence::Fallback,
    );
    assert!(
        !caps.contains(&PaneCapability::MessageReceiver),
        "supervisor below Authoritative must not be treated as a handshake"
    );
}

/// `lazygit` / `tig` / any TUI must NEVER be a structured-message
/// receiver — even if the user is technically able to send key
/// events to the pane via tmux. This is a hard product rule.
#[test]
fn runtime_capabilities_git_ui_is_never_message_receiver() {
    for source in [
        PaneObservationSource::Supervisor,
        PaneObservationSource::ProcessObserver,
        PaneObservationSource::TmuxCurrentCommand,
    ] {
        for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
            let confidence = source.baseline_confidence();
            let caps = derive_runtime_capabilities(PaneKind::GitUi, state, source, confidence);
            assert_eq!(
                caps,
                [PaneCapability::Tui].into_iter().collect(),
                "GitUi caps must be exactly {{Tui}} (state={state:?}, source={source:?})"
            );
            assert!(
                !caps.contains(&PaneCapability::MessageReceiver),
                "GitUi must never expose MessageReceiver"
            );
            assert!(
                !caps.contains(&PaneCapability::MessageResponder),
                "GitUi must never expose MessageResponder"
            );
        }
    }
}

#[test]
fn runtime_capabilities_unknown_is_empty_and_conservative() {
    for source in [
        PaneObservationSource::Supervisor,
        PaneObservationSource::ProcessObserver,
        PaneObservationSource::TmuxCurrentCommand,
        PaneObservationSource::Unknown,
    ] {
        for state in [PaneState::Idle, PaneState::Running, PaneState::Unknown] {
            let confidence = source.baseline_confidence();
            let caps = derive_runtime_capabilities(PaneKind::Unknown, state, source, confidence);
            assert!(caps.is_empty(), "Unknown caps must always be empty");
        }
    }
}

// ── PaneActivityState / PaneMessageSafety ─────────────────────

#[test]
fn activity_state_default_is_unknown() {
    assert_eq!(PaneActivityState::default(), PaneActivityState::Unknown);
}

#[test]
fn activity_state_only_agent_busy_implies_agent_work() {
    // AgentForeground is the conservative state for "AI assistant is
    // open, busy state unverified". It MUST NOT imply agent work; the
    // domain rule is that only an explicit authoritative busy
    // handshake (currently unreached) yields AgentBusy.
    for s in [
        PaneActivityState::Unknown,
        PaneActivityState::ShellIdle,
        PaneActivityState::ForegroundProcess,
        PaneActivityState::AgentForeground,
        PaneActivityState::Exited,
    ] {
        assert!(!s.implies_agent_work(), "{s:?} must not imply agent work");
    }
    assert!(PaneActivityState::AgentBusy.implies_agent_work());
}

#[test]
fn activity_state_agent_foreground_uses_dedicated_i18n_key() {
    assert_eq!(
        PaneActivityState::AgentForeground.i18n_key(),
        crate::i18n_keys::PANE_ACTIVITY_AGENT_FOREGROUND,
        "AgentForeground must route through its own i18n key so the \
             label cannot be conflated with AgentBusy or ForegroundProcess"
    );
    assert_ne!(
        PaneActivityState::AgentForeground.i18n_key(),
        PaneActivityState::AgentBusy.i18n_key(),
    );
    assert_ne!(
        PaneActivityState::AgentForeground.i18n_key(),
        PaneActivityState::ForegroundProcess.i18n_key(),
    );
}

#[test]
fn message_safety_default_is_unknown() {
    assert_eq!(PaneMessageSafety::default(), PaneMessageSafety::Unknown);
}

#[test]
fn message_safety_only_safe_structured_permits_structured_message() {
    for s in [
        PaneMessageSafety::Unknown,
        PaneMessageSafety::NotApplicable,
        PaneMessageSafety::ShellInput,
        PaneMessageSafety::UnverifiedAgent,
    ] {
        assert!(
            !s.permits_structured_message(),
            "{s:?} must not permit structured message routing"
        );
    }
    assert!(PaneMessageSafety::SafeStructured.permits_structured_message());
}

#[test]
fn input_capability_accepts_structured_input_matches_safe_structured_only() {
    for s in [
        InputCapability::Unknown,
        InputCapability::NotApplicable,
        InputCapability::ShellInput,
        InputCapability::UnverifiedAgent,
    ] {
        assert!(
            !s.accepts_structured_input(),
            "{s:?} must not accept structured input"
        );
    }
    assert!(InputCapability::SafeStructured.accepts_structured_input());
}

#[test]
fn input_capability_accepts_free_form_input_only_for_addressable_panes() {
    assert!(InputCapability::ShellInput.accepts_free_form_input());
    assert!(InputCapability::SafeStructured.accepts_free_form_input());
    for s in [
        InputCapability::Unknown,
        InputCapability::NotApplicable,
        InputCapability::UnverifiedAgent,
    ] {
        assert!(
            !s.accepts_free_form_input(),
            "{s:?} must not accept free-form input"
        );
    }
}

#[test]
fn pane_lifecycle_from_observation_collapses_correctly() {
    assert_eq!(
        PaneLifecycle::from_observation(PaneActivityState::Exited),
        PaneLifecycle::Exited
    );
    assert_eq!(
        PaneLifecycle::from_observation(PaneActivityState::Unknown),
        PaneLifecycle::Unknown
    );
    for s in [
        PaneActivityState::ShellIdle,
        PaneActivityState::ForegroundProcess,
        PaneActivityState::AgentForeground,
        PaneActivityState::AgentBusy,
    ] {
        assert_eq!(
            PaneLifecycle::from_observation(s),
            PaneLifecycle::Open,
            "{s:?} must map to PaneLifecycle::Open"
        );
    }
}

#[test]
fn pane_lifecycle_default_is_unknown() {
    assert_eq!(PaneLifecycle::default(), PaneLifecycle::Unknown);
}

#[test]
fn pane_concise_activity_default_is_unknown() {
    assert_eq!(PaneConciseActivity::default(), PaneConciseActivity::Unknown);
}

#[test]
fn pane_concise_activity_can_receive_input_only_when_addressable() {
    assert!(PaneConciseActivity::WaitingInput.can_receive_input());
    assert!(PaneConciseActivity::Completed.can_receive_input());
    for s in [
        PaneConciseActivity::Unknown,
        PaneConciseActivity::Running,
        PaneConciseActivity::Exited,
    ] {
        assert!(
            !s.can_receive_input(),
            "{s:?} must not be addressable for new input"
        );
    }
}

#[test]
fn derive_concise_activity_uses_supervisor_when_no_ai_state() {
    use super::derive_concise_activity;
    assert_eq!(
        derive_concise_activity(PaneActivityState::ShellIdle, None),
        PaneConciseActivity::WaitingInput
    );
    // Interactive helpers (lazygit, htop, …) show through as
    // ForegroundProcess. They must not be permanently "Running"
    // just because a foreground process exists.
    assert_eq!(
        derive_concise_activity(PaneActivityState::ForegroundProcess, None),
        PaneConciseActivity::WaitingInput
    );
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentForeground, None),
        PaneConciseActivity::WaitingInput
    );
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentBusy, None),
        PaneConciseActivity::Running
    );
    assert_eq!(
        derive_concise_activity(PaneActivityState::Exited, None),
        PaneConciseActivity::Exited
    );
    assert_eq!(
        derive_concise_activity(PaneActivityState::Unknown, None),
        PaneConciseActivity::Unknown
    );
}

#[test]
fn derive_concise_activity_uses_ai_state_when_present() {
    use super::derive_concise_activity;
    use crate::domain::ai_activity::AiActivityState as Ai;
    // Mid-turn AI work collapses to Running.
    for state in [Ai::Processing, Ai::ToolRunning, Ai::WaitingForPermission] {
        assert_eq!(
            derive_concise_activity(PaneActivityState::AgentBusy, Some(state)),
            PaneConciseActivity::Running,
            "AI state {state:?} must collapse to Running"
        );
    }
    // Turn-closed AI signals collapse to Completed and can
    // therefore still receive new prompts.
    for state in [Ai::WaitingForUser, Ai::Error] {
        assert_eq!(
            derive_concise_activity(PaneActivityState::AgentForeground, Some(state)),
            PaneConciseActivity::Completed,
            "AI state {state:?} must collapse to Completed"
        );
        assert!(
            derive_concise_activity(PaneActivityState::AgentForeground, Some(state))
                .can_receive_input()
        );
    }
    // Pane reachable but no provider evidence: waiting for input.
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::OpenUnverified)),
        PaneConciseActivity::WaitingInput,
        "Ai::OpenUnverified must collapse to WaitingInput"
    );
    // No usable provider signal at all: surface unknown rather
    // than fabricating a waiting/ready claim.
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::Unknown)),
        PaneConciseActivity::Unknown,
        "Ai::Unknown must collapse to Unknown"
    );
    // OpenUnverified is reachable+idle so it accepts input.
    // Unknown intentionally does not — claiming "can receive input"
    // without any provider evidence would mask the missing signal.
    assert!(
        derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::OpenUnverified))
            .can_receive_input()
    );
    assert!(
        !derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::Unknown))
            .can_receive_input()
    );
    // Place a guard against future drift: a WaitingForUser AI fact
    // must remain free-form ready (collapses to Completed, which
    // accepts free-form input).
    assert!(
        derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::WaitingForUser))
            .can_receive_input()
    );
}

#[test]
fn derive_concise_activity_ai_state_overrides_supervisor() {
    use super::derive_concise_activity;
    use crate::domain::ai_activity::AiActivityState as Ai;
    // Even if the supervisor still reports AgentForeground (open
    // but unverified), a fresh AI fact saying "Processing" pulls
    // the concise label to Running. This protects against the
    // "pane just reports a process exists" case being shown as
    // active work.
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentForeground, Some(Ai::Processing)),
        PaneConciseActivity::Running
    );
    // And the inverse: AgentBusy with a stale WaitingForUser fact
    // still collapses to Completed because the typed AI state is
    // authoritative once present.
    assert_eq!(
        derive_concise_activity(PaneActivityState::AgentBusy, Some(Ai::WaitingForUser)),
        PaneConciseActivity::Completed
    );
}

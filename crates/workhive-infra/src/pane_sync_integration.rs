//! Integration tests: sync_pane_registry against concrete substrate.
//!
//! These tests prove the end-to-end sync path using the real
//! [`InMemoryPaneRegistry`] and [`InMemoryPaneObserver`] — not inline
//! fakes. This satisfies the Phase 5 requirement that the sync path
//! must be exercised against production implementations.

#[cfg(test)]
mod tests {
    use crate::pane_observer::InMemoryPaneObserver;
    use crate::pane_registry::InMemoryPaneRegistry;
    use workhive_core::domain::{
        ObservedPaneState, PaneCapability, PaneDescriptor, PaneEvent, PaneKind, PaneLaunchOrigin,
        PaneObservationConfidence, PaneObservationSource, PaneRuntimeId, PaneRuntimeStatus,
        PaneState, UnitId,
    };
    use workhive_core::ports::{PaneObserverPort, PaneRegistryPort, RuntimeUnitSnapshot};
    use workhive_core::usecases::pane_sync::{snapshot_to_observation, sync_pane_registry};

    fn make_obs(
        group: &str,
        workspace: &str,
        unit_id: &str,
        state: PaneState,
        kind: PaneKind,
    ) -> ObservedPaneState {
        // Default test fixture mimics the tmux-only fallback path: no
        // supervisor or OS observer enrichment.
        make_obs_layered(
            group,
            workspace,
            unit_id,
            state,
            kind,
            PaneObservationSource::TmuxCurrentCommand,
            PaneObservationConfidence::Fallback,
        )
    }

    fn make_obs_layered(
        group: &str,
        workspace: &str,
        unit_id: &str,
        state: PaneState,
        kind: PaneKind,
        source: PaneObservationSource,
        confidence: PaneObservationConfidence,
    ) -> ObservedPaneState {
        ObservedPaneState {
            group: group.to_owned(),
            workspace: workspace.to_owned(),
            unit_id: UnitId::new(unit_id),
            cwd: "/tmp".to_owned(),
            command: "zsh".to_owned(),
            kind,
            state,
            source,
            confidence,
        }
    }

    // ── Discovery ──

    #[test]
    fn concrete_sync_discovers_new_pane() {
        let reg = InMemoryPaneRegistry::new();
        let obs = vec![make_obs(
            "g",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];

        let result = sync_pane_registry(&reg, obs, None).unwrap();

        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            PaneEvent::Discovered {
                runtime_id,
                descriptor,
            } => {
                assert_eq!(*runtime_id, PaneRuntimeId::derive("g", "ws", "%0"));
                assert_eq!(descriptor.group, "g");
                assert_eq!(descriptor.workspace, "ws");
                assert!(
                    descriptor
                        .capabilities
                        .contains(&PaneCapability::CommandExecutor)
                );
            }
            other => panic!("expected Discovered, got {:?}", other),
        }
    }

    // ── State change ──

    #[test]
    fn concrete_sync_detects_state_change() {
        let reg = InMemoryPaneRegistry::new();
        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        let result = sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        assert_eq!(result.events.len(), 1);
        match &result.events[0] {
            PaneEvent::StateChanged { from, to, .. } => {
                assert_eq!(*from, PaneState::Idle);
                assert_eq!(*to, PaneState::Running);
            }
            other => panic!("expected StateChanged, got {:?}", other),
        }
    }

    // ── Capability change ──

    #[test]
    fn concrete_sync_detects_capability_change_for_supervised_running_agent() {
        // The capability change must be triggered by a *live*
        // supervised agent observation. Tmux fallback alone must
        // never produce a Terminal→Agent capability promotion to
        // MessageReceiver.
        let reg = InMemoryPaneRegistry::new();
        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        let result = sync_pane_registry(
            &reg,
            vec![make_obs_layered(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Agent,
                PaneObservationSource::Supervisor,
                PaneObservationConfidence::Authoritative,
            )],
            None,
        )
        .unwrap();

        // The transition Idle→Running on a kind change also triggers
        // a `StateChanged` event; we only assert the capability part.
        let cap_event = result
            .events
            .iter()
            .find(|e| matches!(e, PaneEvent::CapabilityChanged { .. }))
            .expect("expected at least one CapabilityChanged event");
        match cap_event {
            PaneEvent::CapabilityChanged { added, removed, .. } => {
                assert!(added.contains(&PaneCapability::MessageResponder));
                assert!(added.contains(&PaneCapability::MessageReceiver));
                assert!(!removed.contains(&PaneCapability::CommandExecutor));
                assert!(
                    removed.is_empty(),
                    "no capabilities should be removed: {removed:?}"
                );
            }
            other => panic!("expected CapabilityChanged, got {:?}", other),
        }
    }

    #[test]
    fn concrete_sync_does_not_promote_tmux_fallback_agent_to_message_receiver() {
        // A workspace where a user
        // manually started `codex` in pane %0. Tmux gives us
        // `current_command = codex`, classification yields
        // `PaneKind::Agent`, but there is no supervisor handshake.
        // The descriptor must keep CommandExecutor only — never
        // promote a tmux-fallback agent to MessageReceiver.
        let reg = InMemoryPaneRegistry::new();
        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Running,
                PaneKind::Agent,
            )],
            None,
        )
        .unwrap();

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .expect("pane should be registered");
        assert!(
            !desc.capabilities.contains(&PaneCapability::MessageReceiver),
            "tmux-fallback agent must not be promoted to MessageReceiver"
        );
        assert!(
            !desc
                .capabilities
                .contains(&PaneCapability::MessageResponder),
            "tmux-fallback agent must not be promoted to MessageResponder"
        );
    }

    // ── Removal ──

    #[test]
    fn concrete_sync_detects_removed_pane() {
        let reg = InMemoryPaneRegistry::new();
        sync_pane_registry(
            &reg,
            vec![
                make_obs("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
                make_obs("g", "ws", "%1", PaneState::Idle, PaneKind::Terminal),
            ],
            None,
        )
        .unwrap();

        let result = sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        let removed: Vec<_> = result
            .events
            .iter()
            .filter(|e| matches!(e, PaneEvent::Removed { .. }))
            .collect();
        assert_eq!(removed.len(), 1);
        match &removed[0] {
            PaneEvent::Removed { runtime_id } => {
                assert_eq!(*runtime_id, PaneRuntimeId::derive("g", "ws", "%1"));
            }
            _ => unreachable!(),
        }
    }

    // ── Observer integration ──

    #[test]
    fn concrete_sync_mirrors_events_to_observer() {
        let reg = InMemoryPaneRegistry::new();
        let obs = InMemoryPaneObserver::new();

        let result = sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            Some(&obs),
        )
        .unwrap();

        assert_eq!(result.events.len(), 1);
        assert_eq!(obs.pending_count(), 1);

        let observer_events = obs.poll_events();
        assert_eq!(observer_events.len(), 1);
    }

    #[test]
    fn concrete_observer_accumulates_across_sync_cycles() {
        let reg = InMemoryPaneRegistry::new();
        let obs = InMemoryPaneObserver::new();

        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            Some(&obs),
        )
        .unwrap();

        sync_pane_registry(
            &reg,
            vec![
                make_obs("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
                make_obs("g", "ws", "%1", PaneState::Running, PaneKind::Agent),
            ],
            Some(&obs),
        )
        .unwrap();

        let all_events = obs.poll_events();
        assert!(
            all_events.len() >= 2,
            "observer must accumulate across cycles, got {}",
            all_events.len()
        );
    }

    #[test]
    fn concrete_external_poll_after_sync() {
        let reg = InMemoryPaneRegistry::new();
        let obs = InMemoryPaneObserver::new();

        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            Some(&obs),
        )
        .unwrap();

        let first = obs.poll_events();
        assert!(!first.is_empty());

        let second = obs.poll_events();
        assert!(second.is_empty(), "poll_events is destructive");
    }

    // ── cwd/command rule ──

    #[test]
    fn concrete_cwd_command_changes_do_not_emit_event() {
        let reg = InMemoryPaneRegistry::new();
        sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        let mut changed = make_obs("g", "ws", "%0", PaneState::Idle, PaneKind::Terminal);
        changed.cwd = "/home/user".to_owned();
        changed.command = "vim".to_owned();

        let result = sync_pane_registry(&reg, vec![changed], None).unwrap();
        assert!(result.events.is_empty());

        let desc = reg
            .get(&PaneRuntimeId::derive("g", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(desc.cwd, "/home/user");
        assert_eq!(desc.command, "vim");
    }

    // ── No observer ──

    #[test]
    fn concrete_sync_without_observer_returns_events() {
        let reg = InMemoryPaneRegistry::new();
        let result = sync_pane_registry(
            &reg,
            vec![make_obs(
                "g",
                "ws",
                "%0",
                PaneState::Idle,
                PaneKind::Terminal,
            )],
            None,
        )
        .unwrap();

        assert_eq!(result.events.len(), 1);
    }

    // ── Multi-group isolation ──

    #[test]
    fn concrete_sync_handles_multi_group_panes() {
        let reg = InMemoryPaneRegistry::new();
        let result = sync_pane_registry(
            &reg,
            vec![
                make_obs("group-a", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
                make_obs("group-b", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
            ],
            None,
        )
        .unwrap();

        assert_eq!(
            result.events.len(),
            2,
            "different groups must produce separate panes"
        );

        let all = reg.list(None).unwrap();
        assert_eq!(all.len(), 2);
        assert_ne!(all[0].runtime_id, all[1].runtime_id);
    }

    // ── snapshot_to_observation conversion ──

    #[test]
    fn snapshot_to_observation_maps_all_fields() {
        let snapshot = RuntimeUnitSnapshot {
            id: UnitId::new("%5"),
            index: 0,
            cwd: camino::Utf8PathBuf::from("/home/dev"),
            command: "vim".to_owned(),
            active: true,
            kind: PaneKind::Terminal,
            state: PaneState::Running,
            tty: None,
            process_pid: None,
        };

        let obs = snapshot_to_observation(&snapshot, "srv1", "my-ws");
        assert_eq!(obs.group, "srv1");
        assert_eq!(obs.workspace, "my-ws");
        assert_eq!(obs.unit_id, UnitId::new("%5"));
        assert_eq!(obs.cwd, "/home/dev");
        assert_eq!(obs.command, "vim");
        assert_eq!(obs.kind, PaneKind::Terminal);
        assert_eq!(obs.state, PaneState::Running);
    }

    // ── Baseline + runtime sync: observed_status correction ──

    fn make_baseline_descriptor(group: &str, workspace: &str, unit: &str) -> PaneDescriptor {
        PaneDescriptor {
            runtime_id: PaneRuntimeId::derive(group, workspace, unit),
            group: group.to_owned(),
            workspace: workspace.to_owned(),
            unit_id: UnitId::new(unit),
            cwd: String::new(),
            command: "zsh".to_owned(),
            kind: PaneKind::Terminal,
            state: PaneState::Running,
            observed_status: PaneRuntimeStatus::Running,
            role: None,
            launch_origin: PaneLaunchOrigin::EmptyShell,
            capabilities: [PaneCapability::CommandExecutor].into_iter().collect(),
        }
    }

    #[test]
    fn baseline_running_corrected_by_runtime_sync_to_idle() {
        let reg = InMemoryPaneRegistry::new();

        // Simulate baseline registration (what cmd_open does)
        reg.upsert(make_baseline_descriptor("srv", "ws", "%0"))
            .unwrap();

        let desc_before = reg
            .get(&PaneRuntimeId::derive("srv", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc_before.observed_status,
            PaneRuntimeStatus::Running,
            "baseline must set Running"
        );

        // Simulate runtime sync with Idle observation
        let observations = vec![make_obs(
            "srv",
            "ws",
            "%0",
            PaneState::Idle,
            PaneKind::Terminal,
        )];
        let result = sync_pane_registry(&reg, observations, None).unwrap();

        assert!(
            result.updated > 0,
            "runtime sync must detect status correction as an update"
        );

        let desc_after = reg
            .get(&PaneRuntimeId::derive("srv", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc_after.observed_status,
            PaneRuntimeStatus::Idle,
            "runtime sync must correct observed_status from Running to Idle"
        );
    }

    #[test]
    fn runtime_sync_new_pane_gets_correct_observed_status() {
        let reg = InMemoryPaneRegistry::new();

        let observations = vec![make_obs(
            "srv",
            "ws",
            "%0",
            PaneState::Running,
            PaneKind::Agent,
        )];
        let result = sync_pane_registry(&reg, observations, None).unwrap();

        assert_eq!(result.registered, 1);

        let desc = reg
            .get(&PaneRuntimeId::derive("srv", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(
            desc.observed_status,
            PaneRuntimeStatus::Running,
            "pane registered via observation must not stay Unknown"
        );
    }

    #[test]
    fn production_baseline_then_sync_flow() {
        let reg = InMemoryPaneRegistry::new();
        let obs_port = InMemoryPaneObserver::new();

        // Phase 1: Baseline registration (simulates cmd_open upsert)
        reg.upsert(make_baseline_descriptor("srv", "ws", "%0"))
            .unwrap();
        reg.upsert(make_baseline_descriptor("srv", "ws", "%1"))
            .unwrap();

        // Phase 2: Runtime sync (simulates list_units -> sync_pane_registry).
        // Pane %1 is observed as a *live supervised agent*, the only
        // configuration that grants MessageResponder under the
        // capability rules.
        let runtime_observations = vec![
            make_obs("srv", "ws", "%0", PaneState::Idle, PaneKind::Terminal),
            make_obs_layered(
                "srv",
                "ws",
                "%1",
                PaneState::Running,
                PaneKind::Agent,
                PaneObservationSource::Supervisor,
                PaneObservationConfidence::Authoritative,
            ),
        ];

        let result = sync_pane_registry(&reg, runtime_observations, Some(&obs_port)).unwrap();

        // Both panes were updated (state or status changed from baseline)
        assert!(
            result.updated > 0,
            "runtime sync must produce updates after baseline"
        );

        // Verify pane %0: baseline Running -> observed Idle
        let d0 = reg
            .get(&PaneRuntimeId::derive("srv", "ws", "%0"))
            .unwrap()
            .unwrap();
        assert_eq!(d0.observed_status, PaneRuntimeStatus::Idle);
        assert_eq!(d0.state, PaneState::Idle);

        // Verify pane %1: baseline Running -> still Running (no state change)
        // but kind changed Terminal -> Agent with a supervisor handshake,
        // so capabilities differ.
        let d1 = reg
            .get(&PaneRuntimeId::derive("srv", "ws", "%1"))
            .unwrap()
            .unwrap();
        assert_eq!(d1.observed_status, PaneRuntimeStatus::Running);
        assert!(d1.capabilities.contains(&PaneCapability::MessageResponder));

        // Observer received events
        let events = obs_port.poll_events();
        assert!(
            !events.is_empty(),
            "observer must receive events from production sync path"
        );
    }
}

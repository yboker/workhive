#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

pub(crate) fn register_restored_panes_from_snapshot(
    ctx: &CommandContext,
    runtime: &dyn workhive_core::ports::SnapshotPaneRuntimePort,
    snapshot_store: &dyn SnapshotStorePort,
    state_key: &workhive_core::domain::WorkspaceStateKey,
    server: &str,
    workspace: &str,
    project_root: &camino::Utf8Path,
) {
    use workhive_core::ports::{PaneLaunchIdentityEntry, PaneLaunchIdentityPort, PaneRegistryPort};

    let snapshot = match snapshot_store.load_latest(state_key) {
        Ok(Some(s)) => s,
        Ok(None) => {
            tracing::warn!(
                workspace,
                "snapshot restore reported success but load_latest now returns None; skipping registry repopulation"
            );
            return;
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                workspace,
                "could not re-load snapshot for registry repopulation; pane titles may show shell"
            );
            return;
        }
    };

    // Use the multi-window
    // topology so we can key by `(window_index, pane_index)`.
    let topology = match runtime.capture_topology(server, workspace) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                error = %e,
                workspace,
                "could not capture runtime topology after snapshot restore; pane registry not repopulated"
            );
            return;
        }
    };

    // (window_index, pane_index) → UnitId. Multi-window safe.
    let mut unit_by_position: std::collections::HashMap<(u32, u32), workhive_core::domain::UnitId> =
        std::collections::HashMap::new();
    for pane in &topology.panes {
        unit_by_position.insert((pane.window_index, pane.pane_index), pane.unit_id.clone());
    }

    let identity_store = ctx.composition.pane_launch_identity_store(project_root);
    let mut registered = 0usize;
    let mut skipped = 0usize;

    let entries =
        build_restored_pane_registry_entries(&snapshot, &unit_by_position, server, workspace);
    for entry in &entries {
        if entry.unit_id.is_none() {
            tracing::debug!(
                window_index = entry.window_index,
                pane_index = entry.pane_index,
                "no live runtime UnitId matches snapshot pane (window,pane) position; skipping registry entry"
            );
            skipped += 1;
            continue;
        }
        let descriptor = entry
            .descriptor
            .clone()
            .expect("entry.descriptor present when unit_id is some");
        let unit_id = descriptor.unit_id.clone();
        let launch_origin = descriptor.launch_origin.clone();
        let observed_kind = descriptor.kind;
        let observed_state = descriptor.state;
        match ctx.pane_registry.upsert(descriptor) {
            Ok(_) => {
                registered += 1;
                tracing::debug!(
                    window_index = entry.window_index,
                    pane_index = entry.pane_index,
                    unit_id = %unit_id,
                    launch_origin = ?launch_origin,
                    observed_kind = ?observed_kind,
                    observed_state = ?observed_state,
                    "registered restored pane descriptor"
                );
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    window_index = entry.window_index,
                    pane_index = entry.pane_index,
                    "failed to register restored pane descriptor"
                );
                continue;
            }
        }

        // Mirror into the durable identity store so the detached
        // control-plane supervisor can hydrate `display_title` for
        // `QueryPane` after this open.
        let identity_entry = PaneLaunchIdentityEntry {
            workspace: workspace.to_owned(),
            unit_id: unit_id.clone(),
            launch_origin,
        };
        if let Err(e) = identity_store.record(server, identity_entry) {
            tracing::warn!(
                error = %e,
                window_index = entry.window_index,
                pane_index = entry.pane_index,
                "failed to mirror restored launch identity to durable store"
            );
        }
    }

    tracing::info!(
        registered,
        skipped,
        workspace,
        "snapshot restore: pane registry repopulated from snapshot launch_intent"
    );
}

/// Pure projection used by `register_restored_panes_from_snapshot`
/// and unit tests. Carries the resolved `UnitId` (or `None` when no
/// live position matched) alongside the descriptor / identity that
/// would be persisted, plus the original `(window_index,pane_index)`
/// for diagnostics.
#[derive(Debug, Clone)]
struct RestoredPaneRegistryEntry {
    window_index: u32,
    pane_index: u32,
    /// `Some` when a live runtime position exists for this snapshot
    /// pane. `None` means the snapshot pane was not present in the
    /// captured topology and registry repopulation is skipped.
    unit_id: Option<workhive_core::domain::UnitId>,
    descriptor: Option<workhive_core::domain::PaneDescriptor>,
}

/// Build the typed registry entries for every managed snapshot pane.
///
/// Contract:
///
///   * Live `UnitId`s are looked up by the full
///     `(window_index, pane_index)` tuple — single-axis matching is
///     forbidden.
///   * Descriptor `kind` / `state` / `observed_status` are derived
///     from the snapshot's observed reality, never fabricated as
///     `Running`.
fn build_restored_pane_registry_entries(
    snapshot: &workhive_core::domain::WorkspaceSnapshot,
    unit_by_position: &std::collections::HashMap<(u32, u32), workhive_core::domain::UnitId>,
    server: &str,
    workspace: &str,
) -> Vec<RestoredPaneRegistryEntry> {
    use workhive_core::domain::{
        PaneDescriptor, PaneObservationConfidence, PaneObservationSource, PaneRuntimeId,
        derive_runtime_capabilities,
    };
    // Snapshot restore is not a supervisor handshake. The most we
    // can claim about a restored pane's foreground process is what
    // tmux's `pane_current_command` reports at capture time, which
    // is the canonical fallback observation. We therefore tag every
    // restored seed as `TmuxCurrentCommand` + `Fallback` so the
    // runtime-aware capability derivation refuses to grant
    // `MessageReceiver`/`MessageResponder` until a live
    // supervisor-authoritative observation upgrades the pane. This
    // keeps registry capabilities aligned with `derive_message_safety`,
    // which is required because `message_bus` (direct/role/capability/
    // broadcast routing) and `orchestration` worker selection both
    // route on `PaneCapability::MessageReceiver`.
    let restore_seed_source = PaneObservationSource::TmuxCurrentCommand;
    let restore_seed_confidence = PaneObservationConfidence::Fallback;

    snapshot
        .panes
        .iter()
        .filter(|pane| pane.managed)
        .map(|pane| {
            let position = (pane.window_index, pane.pane_index);
            let unit_id = unit_by_position.get(&position).cloned();
            let Some(unit_id) = unit_id else {
                return RestoredPaneRegistryEntry {
                    window_index: pane.window_index,
                    pane_index: pane.pane_index,
                    unit_id: None,
                    descriptor: None,
                };
            };

            let launch_origin = launch_intent_to_origin(&pane.launch_intent);
            let observed_kind = pane_kind_from_observed_command(&pane.observed.current_command);
            let observed_state = pane_state_from_observed(&pane.observed);
            let observed_status = pane_runtime_status_from_state(observed_state);

            let descriptor = PaneDescriptor {
                runtime_id: PaneRuntimeId::derive(server, workspace, unit_id.as_str()),
                group: server.to_owned(),
                workspace: workspace.to_owned(),
                unit_id: unit_id.clone(),
                cwd: pane.cwd.as_str().to_owned(),
                command: pane.observed.current_command.clone().unwrap_or_default(),
                kind: observed_kind,
                state: observed_state,
                observed_status,
                role: None,
                launch_origin,
                capabilities: derive_runtime_capabilities(
                    observed_kind,
                    observed_state,
                    restore_seed_source,
                    restore_seed_confidence,
                ),
            };

            RestoredPaneRegistryEntry {
                window_index: pane.window_index,
                pane_index: pane.pane_index,
                unit_id: Some(unit_id),
                descriptor: Some(descriptor),
            }
        })
        .collect()
}

/// Map a snapshot's observed foreground command to a `PaneKind` using
/// only domain-level knowledge.
///
/// This deliberately does not consult the historical launch identity —
/// the descriptor must
/// reflect the snapshot's observed reality. The shell / agent name
/// lists mirror the tmux adapter's classification so registry seeds
/// match what `pane list` / `pane show` will report once the live
/// runtime sync runs.
fn pane_kind_from_observed_command(command: &Option<String>) -> workhive_core::domain::PaneKind {
    use workhive_core::domain::PaneKind;
    let Some(raw) = command.as_deref() else {
        return PaneKind::Unknown;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return PaneKind::Unknown;
    }
    let basename = trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_lowercase();
    const SHELLS: &[&str] = &["bash", "zsh", "fish", "sh", "dash", "ksh"];
    const AGENTS: &[&str] = &["claude", "cursor", "copilot", "codex", "aider", "opencode"];
    const GIT_UIS: &[&str] = &["lazygit", "tig", "gitui"];
    if SHELLS.contains(&basename.as_str()) {
        PaneKind::Terminal
    } else if AGENTS.contains(&basename.as_str()) {
        PaneKind::Agent
    } else if GIT_UIS.contains(&basename.as_str()) {
        PaneKind::GitUi
    } else {
        PaneKind::Unknown
    }
}

/// Map a snapshot's observed pane state to a domain `PaneState`.
///
/// Registry descriptors MUST NOT
/// fabricate `Running`. A dead pane reports `Unknown` (we do not have
/// a domain `Stopped` variant); a live shell reports `Idle`; a live
/// foreground command other than a shell reports `Running`.
fn pane_state_from_observed(
    observed: &workhive_core::domain::PaneObservedState,
) -> workhive_core::domain::PaneState {
    use workhive_core::domain::PaneState;
    if observed.dead {
        return PaneState::Unknown;
    }
    let Some(raw) = observed.current_command.as_deref() else {
        return PaneState::Unknown;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return PaneState::Unknown;
    }
    let basename = trimmed
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_lowercase();
    const SHELLS: &[&str] = &["bash", "zsh", "fish", "sh", "dash", "ksh"];
    if SHELLS.contains(&basename.as_str()) {
        PaneState::Idle
    } else {
        PaneState::Running
    }
}

/// Map a domain `PaneState` to the registry's richer
/// `PaneRuntimeStatus`. We only ever produce the subset we have
/// evidence for at registry seed time.
fn pane_runtime_status_from_state(
    state: workhive_core::domain::PaneState,
) -> workhive_core::domain::PaneRuntimeStatus {
    use workhive_core::domain::{PaneRuntimeStatus, PaneState};
    match state {
        PaneState::Idle => PaneRuntimeStatus::Idle,
        PaneState::Running => PaneRuntimeStatus::Running,
        PaneState::Unknown => PaneRuntimeStatus::Unknown,
    }
}

/// Convert a snapshot `PaneLaunchIntent` into a registry `PaneLaunchOrigin`.
///
/// `EmptyShell` and `NonRestorable` both surface as `EmptyShell`
/// because neither carries Workhive-owned tool identity that would
/// drive a reasserted pane title.
fn launch_intent_to_origin(
    intent: &workhive_core::domain::PaneLaunchIntent,
) -> workhive_core::domain::PaneLaunchOrigin {
    use workhive_core::domain::{PaneLaunchIntent, PaneLaunchOrigin, ToolName};
    match intent {
        PaneLaunchIntent::EmptyShell | PaneLaunchIntent::NonRestorable { .. } => {
            PaneLaunchOrigin::EmptyShell
        }
        PaneLaunchIntent::ToolRef { tool_name } => match ToolName::new(tool_name) {
            Ok(name) => PaneLaunchOrigin::Tool { name },
            Err(_) => PaneLaunchOrigin::EmptyShell,
        },
        PaneLaunchIntent::InlineCommand { program, args } => PaneLaunchOrigin::InlineCommand {
            spec: workhive_core::domain::CommandSpec::new(program.clone(), args.iter().cloned()),
        },
    }
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    #[test]
    fn restored_registry_entries_match_panes_by_window_and_pane_tuple() {
        // Multi-window snapshot: W0/P0 was launched as `claude`,
        // W1/P0 was launched as `codex`. Single-axis `pane_index`
        // matching would alias both at index 0 and clobber identity.
        // Our resolver must use the full tuple so each window's pane
        // gets its own correct `UnitId` + launch identity.
        use workhive_core::domain::{PaneLaunchIntent, ToolName};
        let snapshot = make_snapshot(vec![
            make_pane(
                0,
                0,
                PaneLaunchIntent::ToolRef {
                    tool_name: ToolName::new("claude").expect("valid tool").to_string(),
                },
                Some("claude"),
                false,
            ),
            make_pane(
                1,
                0,
                PaneLaunchIntent::ToolRef {
                    tool_name: ToolName::new("codex").expect("valid tool").to_string(),
                },
                Some("codex"),
                false,
            ),
        ]);
        let positions = position_map(&[(0, 0, "%w0p0"), (1, 0, "%w1p0")]);

        let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");

        assert_eq!(entries.len(), 2);
        let by_pos: std::collections::HashMap<(u32, u32), &RestoredPaneRegistryEntry> = entries
            .iter()
            .map(|e| ((e.window_index, e.pane_index), e))
            .collect();

        let w0p0 = by_pos.get(&(0, 0)).expect("W0P0 entry");
        let w1p0 = by_pos.get(&(1, 0)).expect("W1P0 entry");

        let unit_w0 = w0p0.unit_id.as_ref().expect("W0P0 unit_id");
        let unit_w1 = w1p0.unit_id.as_ref().expect("W1P0 unit_id");
        assert_eq!(unit_w0.as_str(), "%w0p0");
        assert_eq!(unit_w1.as_str(), "%w1p0");
        assert_ne!(
            unit_w0, unit_w1,
            "multi-window panes at pane_index=0 must NOT share UnitId"
        );

        // Descriptor launch_origin / kind /
        // state must reflect the snapshot's own observation, and
        // must be different per window (claude vs codex).
        let desc_w0 = w0p0.descriptor.as_ref().expect("W0P0 descriptor");
        let desc_w1 = w1p0.descriptor.as_ref().expect("W1P0 descriptor");
        assert_eq!(desc_w0.command, "claude");
        assert_eq!(desc_w1.command, "codex");
        assert_eq!(desc_w0.kind, workhive_core::domain::PaneKind::Agent);
        assert_eq!(desc_w1.kind, workhive_core::domain::PaneKind::Agent);
        assert_eq!(desc_w0.state, workhive_core::domain::PaneState::Running);
        assert_eq!(desc_w1.state, workhive_core::domain::PaneState::Running);
    }

    #[test]
    fn restored_registry_skips_panes_without_matching_live_position() {
        // Snapshot says W2/P3 had a tool, but the live topology does
        // not contain W2/P3. The entry must mark `unit_id = None`
        // and the caller must skip it instead of inventing a UnitId.
        use workhive_core::domain::PaneLaunchIntent;
        let snapshot = make_snapshot(vec![make_pane(
            2,
            3,
            PaneLaunchIntent::EmptyShell,
            Some("zsh"),
            false,
        )]);
        let positions = position_map(&[(0, 0, "%only")]);
        let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].unit_id.is_none());
        assert!(entries[0].descriptor.is_none());
        assert_eq!(entries[0].window_index, 2);
        assert_eq!(entries[0].pane_index, 3);
    }

    #[test]
    fn restored_registry_does_not_register_running_agent_for_observed_shell() {
        // Snapshot observed shell / live
        // shell MUST NOT be registered as Running Agent. Verify both
        // halves: when the snapshot's launch_intent is EmptyShell
        // AND the observed command is a shell, the descriptor is
        // Terminal+Idle.
        use workhive_core::domain::PaneLaunchIntent;
        let snapshot = make_snapshot(vec![make_pane(
            0,
            0,
            PaneLaunchIntent::EmptyShell,
            Some("zsh"),
            false,
        )]);
        let positions = position_map(&[(0, 0, "%shell")]);
        let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");
        let desc = entries[0]
            .descriptor
            .as_ref()
            .expect("entry has descriptor");
        assert_eq!(desc.kind, workhive_core::domain::PaneKind::Terminal);
        assert_eq!(desc.state, workhive_core::domain::PaneState::Idle);
        assert_eq!(
            desc.observed_status,
            workhive_core::domain::PaneRuntimeStatus::Idle
        );
        assert!(matches!(
            desc.launch_origin,
            workhive_core::domain::PaneLaunchOrigin::EmptyShell
        ));
    }

    #[test]
    fn restored_registry_keeps_running_agent_only_when_observed_tool_is_running() {
        // Snapshot observed tool (current_command = "claude") + live
        // tool launch_intent → descriptor MUST be registered as
        // Running Agent. The freshness rule already filtered
        // exited-to-shell cases out at hydrate time; this is the
        // legitimate "still running" path.
        use workhive_core::domain::PaneLaunchIntent;
        let snapshot = make_snapshot(vec![make_pane(
            0,
            0,
            PaneLaunchIntent::ToolRef {
                tool_name: "claude".to_owned(),
            },
            Some("claude"),
            false,
        )]);
        let positions = position_map(&[(0, 0, "%agent")]);
        let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");
        let desc = entries[0]
            .descriptor
            .as_ref()
            .expect("entry has descriptor");
        assert_eq!(desc.kind, workhive_core::domain::PaneKind::Agent);
        assert_eq!(desc.state, workhive_core::domain::PaneState::Running);
        assert_eq!(
            desc.observed_status,
            workhive_core::domain::PaneRuntimeStatus::Running
        );
        assert!(matches!(
            desc.launch_origin,
            workhive_core::domain::PaneLaunchOrigin::Tool { .. }
        ));
    }

    #[test]
    fn restored_registry_marks_dead_pane_as_unknown_state() {
        // A snapshot pane marked `dead=true` must NOT be registered
        // as Running. We do not have a `Stopped` state; `Unknown`
        // is the honest classification.
        use workhive_core::domain::PaneLaunchIntent;
        let snapshot = make_snapshot(vec![make_pane(
            0,
            0,
            PaneLaunchIntent::EmptyShell,
            Some("zsh"),
            true,
        )]);
        let positions = position_map(&[(0, 0, "%dead")]);
        let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");
        let desc = entries[0]
            .descriptor
            .as_ref()
            .expect("entry has descriptor");
        assert_eq!(desc.state, workhive_core::domain::PaneState::Unknown);
        assert_eq!(
            desc.observed_status,
            workhive_core::domain::PaneRuntimeStatus::Unknown
        );
    }

    /// Snapshot restore registry seeding must NOT grant
    /// `MessageReceiver` (or `MessageResponder`) from a process-name
    /// classification alone.
    ///
    /// Domain rule:
    ///   - Runtime capability derivation MUST stay aligned with
    ///     `derive_message_safety`. Only a running supervisor-
    ///     authoritative agent may advertise structured-message
    ///     capabilities.
    ///   - A snapshot's `observed.current_command` is at best a
    ///     restored tmux-fallback observation. It is not a
    ///     supervisor handshake. Trusting it would let stale or
    ///     manually-started agents (`claude`, `codex`, `opencode`)
    ///     route structured messages until a live supervisor
    ///     observation corrects the registry.
    ///
    /// The registry feeds `message_bus` (direct/role/capability/
    /// broadcast routing) and `orchestration` worker selection, both
    /// of which trust `PaneCapability::MessageReceiver`. The seed
    /// must therefore be conservative.
    #[test]
    fn restored_registry_does_not_grant_message_receiver_for_observed_agent_command() {
        use workhive_core::domain::{PaneCapability, PaneLaunchIntent, ToolName};

        // Cover all three observed agent process names that the
        // existing classifier maps to `PaneKind::Agent`. Every one
        // of them must seed the registry without
        // `MessageReceiver`/`MessageResponder` because snapshot
        // restore is not a supervisor handshake.
        for observed_command in ["claude", "codex", "opencode"] {
            let snapshot = make_snapshot(vec![make_pane(
                0,
                0,
                PaneLaunchIntent::ToolRef {
                    tool_name: ToolName::new(observed_command)
                        .expect("valid tool name")
                        .to_string(),
                },
                Some(observed_command),
                false,
            )]);
            let positions = position_map(&[(0, 0, "%agent")]);
            let entries = build_restored_pane_registry_entries(&snapshot, &positions, "srv", "ws");
            let desc = entries[0]
                .descriptor
                .as_ref()
                .expect("entry has descriptor");

            assert_eq!(
                desc.kind,
                workhive_core::domain::PaneKind::Agent,
                "{observed_command}: descriptor.kind must still reflect the observed agent classification"
            );
            assert_eq!(
                desc.state,
                workhive_core::domain::PaneState::Running,
                "{observed_command}: observed live agent command keeps Running state"
            );
            assert!(
                desc.capabilities.contains(&PaneCapability::CommandExecutor),
                "{observed_command}: free-form command input is still allowed for an unverified agent"
            );
            assert!(
                !desc.capabilities.contains(&PaneCapability::MessageReceiver),
                "{observed_command}: snapshot-restore seed is not a supervisor handshake; \
                 MessageReceiver must NOT be granted from observed process name. \
                 message_bus.rs and orchestration.rs route by this capability."
            );
            assert!(
                !desc
                    .capabilities
                    .contains(&PaneCapability::MessageResponder),
                "{observed_command}: MessageResponder must follow the same conservative gate as MessageReceiver"
            );
        }
    }
}

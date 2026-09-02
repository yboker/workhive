//! Durable launch-baseline use case.
//!
//! The launch baseline is the cross-process bridge between `open` and later
//! monitor/notify invocations. It is written from launch metadata that `open`
//! already used to create or restore panes, not inferred from a later topology
//! capture.

use std::collections::HashMap;

use crate::domain::{
    LaunchBaseline, LaunchBaselineEntry, PaneDescriptor, PaneLaunchIntent, PaneRuntimeId, UnitId,
    WorkspaceSnapshot, WorkspaceStateKey,
};
use crate::errors::SnapshotError;
use crate::ports::{RuntimeTopologySnapshot, SnapshotStorePort};

/// Current timestamp for launch baseline documents.
pub fn launch_baseline_timestamp_now() -> String {
    match std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH) {
        Ok(duration) => format!("{}s", duration.as_secs()),
        Err(_) => "unknown".to_owned(),
    }
}

/// Persist durable launch metadata for later monitor/notify hydration.
///
/// When `new_entries` is empty but the on-disk baseline already contains
/// entries, the existing file is preserved to avoid data loss. Returns the
/// persisted entry count (zero when the write was skipped).
pub fn persist_launch_baseline(
    snapshot_store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
    new_entries: Vec<LaunchBaselineEntry>,
    updated_at: String,
) -> Result<usize, SnapshotError> {
    if new_entries.is_empty() {
        let existing = snapshot_store.load_launch_baseline(state_key)?;
        if existing.is_some_and(|b| !b.entries.is_empty()) {
            tracing::info!(
                "skipping baseline persist: new entries are empty but existing baseline has data"
            );
            return Ok(0);
        }
    }
    let count = new_entries.len();
    let baseline = LaunchBaseline {
        state_key: state_key.clone(),
        entries: new_entries,
        updated_at,
    };
    snapshot_store.save_launch_baseline(&baseline)?;
    Ok(count)
}

/// Convert pane registry descriptors into baseline entries.
pub fn entries_from_descriptors(descriptors: &[PaneDescriptor]) -> Vec<LaunchBaselineEntry> {
    descriptors
        .iter()
        .map(|descriptor| LaunchBaselineEntry {
            runtime_id: descriptor.runtime_id.as_str().to_owned(),
            unit_id: descriptor.unit_id.as_str().to_owned(),
            launch_origin: descriptor.launch_origin.clone(),
        })
        .collect()
}

/// Build baseline entries after a snapshot restore has mapped saved pane
/// positions to newly created runtime unit IDs.
///
/// When a pane's `launch_intent` is `NonRestorable`, the function checks
/// `prior_baseline` (keyed by `(window_index, pane_index)`) and reuses
/// that metadata instead of silently discarding the pane.
pub fn entries_from_restored_snapshot(
    snapshot: &WorkspaceSnapshot,
    unit_by_position: &HashMap<(u32, u32), UnitId>,
    group: &str,
    workspace: &str,
    prior_baseline: &HashMap<(u32, u32), crate::domain::PaneLaunchRef>,
) -> Vec<LaunchBaselineEntry> {
    snapshot
        .panes
        .iter()
        .filter_map(|pane| {
            let pos = (pane.window_index, pane.pane_index);
            let unit_id = unit_by_position.get(&pos)?;
            let launch_origin = launch_intent_to_baseline_ref(&pane.launch_intent)
                .or_else(|| prior_baseline.get(&pos).cloned());
            let launch_origin = launch_origin?;
            let runtime_id = PaneRuntimeId::derive(group, workspace, unit_id.as_str());
            Some(LaunchBaselineEntry {
                runtime_id: runtime_id.as_str().to_owned(),
                unit_id: unit_id.as_str().to_owned(),
                launch_origin,
            })
        })
        .collect()
}

/// Build baseline entries from a live topology capture.
///
/// This is intended for legacy or reused live sessions where `open` did not
/// create the panes in the current process and therefore has no launch
/// resolution result to persist.
pub fn entries_from_topology(
    topology: &RuntimeTopologySnapshot,
    group: &str,
    workspace: &str,
) -> Vec<LaunchBaselineEntry> {
    topology
        .panes
        .iter()
        .filter_map(|pane| {
            let launch_origin = pane.launch_origin.clone()?;
            let runtime_id = PaneRuntimeId::derive(group, workspace, pane.unit_id.as_str());
            Some(LaunchBaselineEntry {
                runtime_id: runtime_id.as_str().to_owned(),
                unit_id: pane.unit_id.as_str().to_owned(),
                launch_origin,
            })
        })
        .collect()
}

/// Build a position-keyed map of launch refs from the prior baseline,
/// cross-referenced against the snapshot's pane positions.
///
/// This is shared by `open` (pre-restore reconciliation) and `close`
/// (close-time snapshot hydration).
pub fn build_prior_baseline_map(
    store: &dyn SnapshotStorePort,
    state_key: &WorkspaceStateKey,
    snapshot: &WorkspaceSnapshot,
) -> Result<HashMap<(u32, u32), crate::domain::PaneLaunchRef>, SnapshotError> {
    let Some(baseline) = store.load_launch_baseline(state_key)? else {
        return Ok(HashMap::new());
    };

    let mut by_unit: HashMap<&str, &crate::domain::LaunchBaselineEntry> = HashMap::new();
    for entry in &baseline.entries {
        by_unit.insert(entry.unit_id.as_str(), entry);
    }

    let mut result = HashMap::new();
    for pane in &snapshot.panes {
        if let Some(runtime_id) = pane.runtime_id.as_deref()
            && let Some(entry) = by_unit.get(runtime_id)
        {
            result.insert(
                (pane.window_index, pane.pane_index),
                entry.launch_origin.clone(),
            );
        }
    }
    Ok(result)
}

/// Hydrate snapshot panes with launch metadata from a prior baseline.
///
/// For each pane in the snapshot whose `launch_intent` is `NonRestorable`,
/// if a matching entry exists in the baseline at the same position, its
/// `launch_intent` is upgraded to the corresponding restorable intent.
///
/// **Freshness rule:**
/// when the live pane is observed running a shell (the user exited the
/// tool with `Ctrl+C`), the prior tool baseline is treated as **stale**
/// and is NOT used to hydrate the pane. The pane stays
/// `EmptyShell` so the next restore truthfully reopens it as a shell.
///
/// We never override an already-restorable `ToolRef` / `InlineCommand`
/// intent — that means the live capture (or `convert_launch_origin_to_intent`)
/// already saw an authoritative `launch_origin` for the pane.
///
/// Returns the number of panes hydrated.
pub fn hydrate_snapshot_with_baseline(
    snapshot: &mut WorkspaceSnapshot,
    prior_baseline: &HashMap<(u32, u32), crate::domain::PaneLaunchRef>,
) -> usize {
    let mut hydrated = 0usize;
    let mut skipped_stale = 0usize;
    let mut neutralised_wrappers = 0usize;
    let mut healed_quote_split = 0usize;
    for pane in &mut snapshot.panes {
        // Heal a quote-polluted known-tool inline command back to its
        // typed tool launch. A configured tool persisted as
        // `InlineCommand { program: "\"cursor-agent", args: ["--yolo\""] }`
        // (matching outer double quotes scattered across the slots) is
        // tokenization pollution; the shared classifier recovers the
        // canonical tool with high confidence. Healing here cleans the
        // in-memory snapshot before re-persistence so the polluted shape
        // does not propagate into the next baseline.
        if let PaneLaunchIntent::InlineCommand { program, args } = &pane.launch_intent
            && let crate::usecases::runtime_start_command::StartCommandClassification::Tool { name } =
                crate::usecases::runtime_start_command::classify_inline_command(program, args)
        {
            tracing::info!(
                window_index = pane.window_index,
                pane_index = pane.pane_index,
                tool = name.as_str(),
                "healed quote-polluted inline command back to typed tool launch"
            );
            pane.launch_intent = PaneLaunchIntent::ToolRef {
                tool_name: name.as_str().to_owned(),
            };
            pane.restore_policy = crate::domain::RestorePolicy::Restore;
            healed_quote_split += 1;
            continue;
        }
        // Defensive hydration: legacy on-disk snapshots from
        // before the shared start-command classifier could persist a
        // `workhive internal pane-supervisor ...` argv as a user
        // `InlineCommand`. Treat that shape as dominated so a prior
        // good baseline can override it. If no baseline is available,
        // demote it to `NonRestorable` rather than replaying the
        // wrapper argv on the next `workspace open`.
        let is_legacy_supervisor_inline =
            launch_intent_looks_like_supervisor_wrapper(&pane.launch_intent);
        let dominated = matches!(
            pane.launch_intent,
            PaneLaunchIntent::NonRestorable { .. } | PaneLaunchIntent::EmptyShell
        ) || is_legacy_supervisor_inline;
        if !dominated {
            continue;
        }
        let Some(baseline_ref) = prior_baseline.get(&(pane.window_index, pane.pane_index)) else {
            if is_legacy_supervisor_inline {
                tracing::warn!(
                    window_index = pane.window_index,
                    pane_index = pane.pane_index,
                    "neutralising legacy supervisor-wrapper InlineCommand without baseline; \
                     marking pane as NonRestorable to prevent wrapper replay"
                );
                pane.launch_intent = PaneLaunchIntent::NonRestorable {
                    display: "workhive internal pane-supervisor".to_owned(),
                    reason: "legacy snapshot recorded supervisor wrapper as inline command"
                        .to_owned(),
                };
                pane.restore_policy = crate::domain::RestorePolicy::Skip;
                neutralised_wrappers += 1;
            }
            continue;
        };
        let Some(intent) = baseline_ref_to_launch_intent(baseline_ref) else {
            continue;
        };

        // Freshness check: if the baseline wants to restore a tool but
        // the live pane is currently a shell, that means the user
        // explicitly exited the tool before snapshot capture. Restoring
        // the old tool would silently re-launch a tool the user
        // intentionally left.
        let baseline_is_tool_or_command = matches!(
            intent,
            PaneLaunchIntent::ToolRef { .. } | PaneLaunchIntent::InlineCommand { .. }
        );
        if baseline_is_tool_or_command && observed_pane_is_shell(pane) {
            tracing::info!(
                window_index = pane.window_index,
                pane_index = pane.pane_index,
                stale_intent = ?intent,
                "snapshot baseline ignored as stale: live pane has returned to shell"
            );
            skipped_stale += 1;
            continue;
        }

        tracing::debug!(
            window_index = pane.window_index,
            pane_index = pane.pane_index,
            new_intent = ?intent,
            "snapshot pane hydrated from prior baseline"
        );
        pane.launch_intent = intent;
        pane.restore_policy = crate::domain::RestorePolicy::Restore;
        hydrated += 1;
    }
    if skipped_stale > 0 {
        tracing::info!(
            skipped_stale,
            hydrated,
            "snapshot baseline hydration skipped {skipped_stale} stale tool entries"
        );
    }
    if neutralised_wrappers > 0 {
        tracing::info!(
            neutralised_wrappers,
            hydrated,
            "snapshot baseline hydration neutralised {neutralised_wrappers} \
             legacy supervisor-wrapper inline commands"
        );
    }
    if healed_quote_split > 0 {
        tracing::info!(
            healed_quote_split,
            hydrated,
            "snapshot baseline hydration healed {healed_quote_split} \
             quote-polluted tool inline commands"
        );
    }
    hydrated
}

/// Inspect a [`PaneLaunchIntent`] to see whether it carries the
/// shape of an internal supervisor wrapper recorded as a user
/// `InlineCommand`. Some on-disk snapshots persist a supervised
/// tool launch (`workhive internal pane-supervisor ...`) as a
/// verbatim `InlineCommand` argv, so this shape must still be
/// recognized when hydrating such snapshots.
///
/// The persisted shape varies in `program` formatting:
/// `"workhive"`, `"/Users/.../workhive"`, `"'/Users/.../workhive'"`,
/// or even `"\"'/Users/.../workhive'\""` from naive whitespace-split
/// capture paths. Detection delegates to the shared classifier in
/// [`runtime_start_command`] so the `program` token is matched by
/// **basename after stripping outer shell quotes**, exactly as the
/// live `pane_start_command` classifier does. This keeps wrapper
/// identification anchored to a single semantic rule across capture
/// paths and avoids ad hoc substring checks.
fn launch_intent_looks_like_supervisor_wrapper(intent: &PaneLaunchIntent) -> bool {
    let PaneLaunchIntent::InlineCommand { program, args } = intent else {
        return false;
    };
    matches!(
        crate::usecases::runtime_start_command::classify_inline_command(program, args),
        crate::usecases::runtime_start_command::StartCommandClassification::InternalSupervisorWrapper { .. }
    )
}

/// Heuristic: does the snapshot pane's observed state look like a
/// bare shell prompt right now?
///
/// Used by [`hydrate_snapshot_with_baseline`] to detect the
/// "user exited Codex with Ctrl+C" case where stale tool identity
/// must NOT be used to re-launch the tool on restore.
///
/// We deliberately keep this conservative: only well-known shell
/// basenames count as "shell". Any other foreground command (tool,
/// inline binary, unknown) leaves the freshness check inactive so we
/// keep the prior baseline's restorability.
fn observed_pane_is_shell(pane: &crate::domain::PaneSnapshot) -> bool {
    let Some(current) = pane.observed.current_command.as_deref() else {
        return false;
    };
    let basename = current
        .rsplit('/')
        .next()
        .unwrap_or(current)
        .to_ascii_lowercase();
    matches!(
        basename.as_str(),
        "zsh" | "bash" | "fish" | "sh" | "ksh" | "dash" | "ash"
    )
}

fn baseline_ref_to_launch_intent(r: &crate::domain::PaneLaunchRef) -> Option<PaneLaunchIntent> {
    use crate::domain::PaneLaunchRef;
    match r {
        PaneLaunchRef::EmptyShell => None,
        PaneLaunchRef::Tool { name } => Some(PaneLaunchIntent::ToolRef {
            tool_name: name.as_str().to_owned(),
        }),
        PaneLaunchRef::InlineCommand { spec } => Some(PaneLaunchIntent::InlineCommand {
            program: spec.program().to_owned(),
            args: spec.args().to_vec(),
        }),
    }
}

fn launch_intent_to_baseline_ref(
    launch_intent: &PaneLaunchIntent,
) -> Option<crate::domain::PaneLaunchRef> {
    match launch_intent {
        PaneLaunchIntent::ToolRef { tool_name } => crate::domain::ToolName::new(tool_name)
            .ok()
            .map(|name| crate::domain::PaneLaunchRef::Tool { name }),
        PaneLaunchIntent::InlineCommand { program, args } => {
            Some(crate::domain::PaneLaunchRef::InlineCommand {
                spec: crate::domain::CommandSpec::new(program, args.iter().map(String::as_str)),
            })
        }
        PaneLaunchIntent::EmptyShell => Some(crate::domain::PaneLaunchRef::EmptyShell),
        PaneLaunchIntent::NonRestorable { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy, SNAPSHOT_SCHEMA_VERSION,
        SnapshotMetadata, SnapshotRevisionId, SnapshotSource, WindowSnapshot, WorkspaceTopology,
    };
    use camino::Utf8PathBuf;

    fn state_key() -> WorkspaceStateKey {
        WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "proj".to_owned(),
            String::new(),
            "main".to_owned(),
        )
    }

    fn snapshot_with_tool_launch() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            revision_id: SnapshotRevisionId::first(),
            state_key: state_key(),
            captured_at: "123s".to_owned(),
            source: SnapshotSource::ManualSave,
            topology: WorkspaceTopology {
                windows: vec![WindowSnapshot {
                    index: 0,
                    name: None,
                    layout: "tiled".to_owned(),
                    panes: vec![PaneRef {
                        pane_index: 0,
                        window_index: 0,
                    }],
                }],
                active_window: Some(0),
                active_pane_runtime_id: None,
            },
            panes: vec![PaneSnapshot {
                runtime_id: None,
                window_index: 0,
                pane_index: 0,
                pane_id: None,
                title: None,
                cwd: Utf8PathBuf::from("/tmp"),
                managed: true,
                restore_policy: RestorePolicy::Restore,
                launch_intent: PaneLaunchIntent::ToolRef {
                    tool_name: "claude".to_owned(),
                },
                observed: PaneObservedState {
                    current_command: Some("claude".to_owned()),
                    start_command: Some("claude".to_owned()),
                    active: true,
                    dead: false,
                },
            }],
            metadata: SnapshotMetadata {
                runtime_backend: "tmux".to_owned(),
                platform: "macos".to_owned(),
                generation: 1,
                content_hash: None,
                config_fingerprint: None,
            },
        }
    }

    #[test]
    fn restored_snapshot_entries_preserve_launch_intent() {
        let mut unit_by_position = HashMap::new();
        unit_by_position.insert((0, 0), UnitId::new("%1"));

        let entries = entries_from_restored_snapshot(
            &snapshot_with_tool_launch(),
            &unit_by_position,
            "srv",
            "sess",
            &HashMap::new(),
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].unit_id, "%1");
        assert!(matches!(
            entries[0].launch_origin,
            crate::domain::PaneLaunchRef::Tool { .. }
        ));
    }

    #[test]
    fn persist_empty_baseline_does_not_overwrite_existing() {
        use crate::domain::{
            LaunchBaseline, LaunchBaselineEntry, SnapshotHealthCheck, SnapshotHistoryEntry,
            SnapshotManifest, SupervisorLock,
        };
        use crate::errors::SnapshotError;
        use crate::ports::SnapshotStorePort;
        use std::cell::RefCell;

        struct StubStore {
            existing: Option<LaunchBaseline>,
            saves: RefCell<Vec<LaunchBaseline>>,
        }

        impl SnapshotStorePort for StubStore {
            fn save_revision(
                &self,
                _: &crate::domain::WorkspaceSnapshot,
            ) -> Result<crate::domain::SnapshotRevisionId, SnapshotError> {
                unreachable!()
            }
            fn load_latest(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<Option<crate::domain::WorkspaceSnapshot>, SnapshotError> {
                Ok(None)
            }
            fn load_revision(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: crate::domain::SnapshotRevisionId,
            ) -> Result<Option<crate::domain::WorkspaceSnapshot>, SnapshotError> {
                Ok(None)
            }
            fn append_events(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: &[crate::domain::SnapshotEvent],
            ) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn load_events_since(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: crate::domain::SnapshotRevisionId,
            ) -> Result<Vec<crate::domain::SnapshotEvent>, SnapshotError> {
                Ok(vec![])
            }
            fn read_manifest(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<Option<SnapshotManifest>, SnapshotError> {
                Ok(None)
            }
            fn write_manifest(&self, _: &SnapshotManifest) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn list_history(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<Vec<SnapshotHistoryEntry>, SnapshotError> {
                Ok(vec![])
            }
            fn acquire_lock(&self, _: &SupervisorLock) -> Result<bool, SnapshotError> {
                Ok(true)
            }
            fn release_lock(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn update_heartbeat(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: &str,
            ) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn read_lock(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<Option<SupervisorLock>, SnapshotError> {
                Ok(None)
            }
            fn health_check(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<SnapshotHealthCheck, SnapshotError> {
                unreachable!()
            }
            fn verify_revision(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: crate::domain::SnapshotRevisionId,
            ) -> Result<bool, SnapshotError> {
                Ok(true)
            }
            fn apply_retention(
                &self,
                _: &crate::domain::WorkspaceStateKey,
                _: u32,
                _: u32,
            ) -> Result<u32, SnapshotError> {
                Ok(0)
            }
            fn delete_all(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn delete_all_global(&self) -> Result<(), SnapshotError> {
                Ok(())
            }
            fn save_launch_baseline(&self, b: &LaunchBaseline) -> Result<(), SnapshotError> {
                self.saves.borrow_mut().push(b.clone());
                Ok(())
            }
            fn load_launch_baseline(
                &self,
                _: &crate::domain::WorkspaceStateKey,
            ) -> Result<Option<LaunchBaseline>, SnapshotError> {
                Ok(self.existing.clone())
            }
        }

        let existing = LaunchBaseline {
            state_key: state_key(),
            entries: vec![LaunchBaselineEntry {
                runtime_id: "rt-1".to_owned(),
                unit_id: "%1".to_owned(),
                launch_origin: crate::domain::PaneLaunchRef::Tool {
                    name: crate::domain::ToolName::new("claude").expect("valid"),
                },
            }],
            updated_at: "100s".to_owned(),
        };

        let store = StubStore {
            existing: Some(existing),
            saves: RefCell::new(Vec::new()),
        };

        let count = persist_launch_baseline(&store, &state_key(), vec![], "200s".to_owned())
            .expect("persist should succeed");
        assert_eq!(count, 0, "must return 0 when skipping empty write");
        assert!(
            store.saves.borrow().is_empty(),
            "must NOT overwrite non-empty baseline with empty entries"
        );
    }

    /// Baseline freshness rule:
    /// when the live pane has returned to shell, stale tool baseline
    /// must NOT be reused to restore as the old tool.
    #[test]
    fn hydrate_skips_stale_tool_baseline_when_pane_now_shell() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::EmptyShell;
        snapshot.panes[0].observed.current_command = Some("zsh".to_owned());
        snapshot.panes[0].observed.start_command = Some("zsh".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("codex").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(
            hydrated, 0,
            "stale tool baseline must be skipped when pane is shell"
        );
        assert!(matches!(
            snapshot.panes[0].launch_intent,
            PaneLaunchIntent::EmptyShell
        ));
    }

    /// Same scenario but with an absolute path shell — basename
    /// detection must still flag this as a shell.
    #[test]
    fn hydrate_skips_stale_tool_baseline_for_absolute_path_shell() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::EmptyShell;
        snapshot.panes[0].observed.current_command = Some("/bin/bash".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("claude").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        assert!(matches!(
            snapshot.panes[0].launch_intent,
            PaneLaunchIntent::EmptyShell
        ));
    }

    /// When the snapshot pane is `NonRestorable` and the live foreground
    /// is the same tool that the baseline records, hydration must
    /// proceed: the tool is still alive, the snapshot just lost its
    /// launch metadata.
    #[test]
    fn hydrate_uses_baseline_when_pane_still_running_tool() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::NonRestorable {
            display: "claude".to_owned(),
            reason: "missing durable launch metadata".to_owned(),
        };
        snapshot.panes[0].observed.current_command = Some("claude".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("claude").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 1);
        assert!(matches!(
            snapshot.panes[0].launch_intent,
            PaneLaunchIntent::ToolRef { .. }
        ));
    }

    /// `EmptyShell` baselines (the user explicitly chose a shell) are
    /// never stale — they may always hydrate.
    #[test]
    fn hydrate_does_not_treat_empty_shell_baseline_as_stale() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::NonRestorable {
            display: "shell".to_owned(),
            reason: "missing durable launch metadata".to_owned(),
        };
        snapshot.panes[0].observed.current_command = Some("zsh".to_owned());

        let mut prior = HashMap::new();
        prior.insert((0u32, 0u32), crate::domain::PaneLaunchRef::EmptyShell);

        // EmptyShell baseline yields no PaneLaunchIntent (see
        // baseline_ref_to_launch_intent), so hydration is a no-op
        // — the freshness rule does not need to engage here.
        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        assert!(matches!(
            snapshot.panes[0].launch_intent,
            PaneLaunchIntent::NonRestorable { .. }
        ));
    }

    /// Regression guard: a legacy snapshot whose pane was
    /// captured (under the old code path) as
    /// `InlineCommand { program: "workhive", args: ["internal", "pane-supervisor", ...] }`
    /// must NOT be replayed verbatim by `workspace open`. When a prior
    /// good baseline exists, hydration must override the wrapper with
    /// the real tool identity.
    #[test]
    fn hydrate_overrides_legacy_supervisor_wrapper_inline_command_with_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "workhive".to_owned(),
            args: vec![
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--runtime-id".to_owned(),
                "wh@i@p@w/%1".to_owned(),
                "--tool".to_owned(),
                "codex".to_owned(),
                "--shell".to_owned(),
                "zsh".to_owned(),
            ],
        };
        snapshot.panes[0].observed.current_command = Some("codex".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("codex").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 1);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::ToolRef { tool_name } => assert_eq!(tool_name, "codex"),
            other => panic!("expected ToolRef after hydration, got {other:?}"),
        }
    }

    /// Regression guard: a legacy supervisor-wrapper
    /// `InlineCommand` with no available baseline must collapse to
    /// `NonRestorable` (skip on restore) so `workspace open` cannot
    /// rebroadcast the wrapper argv via visible `send_input`.
    #[test]
    fn hydrate_neutralises_legacy_supervisor_wrapper_when_no_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "workhive".to_owned(),
            args: vec![
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--runtime-id".to_owned(),
                "wh@i@p@w/%1".to_owned(),
            ],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::NonRestorable { .. } => {}
            other => panic!("expected NonRestorable after neutralisation, got {other:?}"),
        }
        assert!(matches!(
            snapshot.panes[0].restore_policy,
            crate::domain::RestorePolicy::Skip
        ));
    }

    /// Legacy snapshots may persist the supervisor wrapper with a
    /// path-qualified `program` token rather than the bare `workhive`
    /// string. The classifier-backed detector must strip the directory
    /// prefix and identify the wrapper by executable basename.
    #[test]
    fn hydrate_overrides_path_qualified_supervisor_wrapper_with_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "/Users/me/projects/workhive/target/release/workhive".to_owned(),
            args: vec![
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--tool".to_owned(),
                "codex".to_owned(),
                "--".to_owned(),
                "codex".to_owned(),
            ],
        };
        snapshot.panes[0].observed.current_command = Some("codex".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("codex").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 1);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::ToolRef { tool_name } => assert_eq!(tool_name, "codex"),
            other => {
                panic!("expected ToolRef after hydrating path-qualified wrapper, got {other:?}",)
            }
        }
    }

    /// Legacy capture paths can persist the program token with
    /// surrounding apostrophes. Quote-stripped basename detection must
    /// still flag this as a wrapper and demote it when no baseline
    /// exists.
    #[test]
    fn hydrate_neutralises_single_quoted_supervisor_wrapper_when_no_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "'/Users/me/target/release/workhive'".to_owned(),
            args: vec![
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--tool".to_owned(),
                "claude".to_owned(),
                "--".to_owned(),
                "claude".to_owned(),
            ],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::NonRestorable { .. } => {}
            other => panic!(
                "expected NonRestorable after neutralising single-quoted wrapper, got {other:?}",
            ),
        }
        assert!(matches!(
            snapshot.panes[0].restore_policy,
            crate::domain::RestorePolicy::Skip
        ));
    }

    /// Even when nested quoting is preserved in the persisted program
    /// token, the wrapper must be recognized. With baseline available,
    /// hydration restores the recorded tool identity.
    #[test]
    fn hydrate_overrides_nested_quoted_supervisor_wrapper_with_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "\"'/Users/me/target/release/workhive'\"".to_owned(),
            args: vec![
                "internal".to_owned(),
                "pane-supervisor".to_owned(),
                "--tool".to_owned(),
                "claude".to_owned(),
                "--".to_owned(),
                "claude".to_owned(),
            ],
        };
        snapshot.panes[0].observed.current_command = Some("claude".to_owned());

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("claude").expect("valid"),
            },
        );

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 1);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::ToolRef { tool_name } => assert_eq!(tool_name, "claude"),
            other => {
                panic!("expected ToolRef after hydrating nested-quoted wrapper, got {other:?}",)
            }
        }
    }

    /// Persisted wrapper records can store both the program and
    /// individual args with surrounding single quotes. The classifier
    /// strips per-arg quotes so the wrapper signature and `--tool`
    /// resolution still succeed.
    #[test]
    fn hydrate_neutralises_supervisor_wrapper_with_quoted_args_when_no_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "'/Users/me/target/release/workhive'".to_owned(),
            args: vec![
                "'internal'".to_owned(),
                "'pane-supervisor'".to_owned(),
                "'--tool'".to_owned(),
                "'codex'".to_owned(),
                "'--'".to_owned(),
                "'codex'".to_owned(),
            ],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::NonRestorable { .. } => {}
            other => panic!(
                "expected NonRestorable after neutralising quoted-args wrapper, got {other:?}",
            ),
        }
        assert!(matches!(
            snapshot.panes[0].restore_policy,
            crate::domain::RestorePolicy::Skip
        ));
    }

    /// `workhive status` is a real user CLI invocation, not the
    /// supervisor wrapper. Both bare and path-qualified program tokens
    /// with non-supervisor args must round-trip unchanged through
    /// hydration so user-authored inline commands are preserved.
    #[test]
    fn hydrate_preserves_real_workhive_status_user_command_path_qualified() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "/usr/local/bin/workhive".to_owned(),
            args: vec!["status".to_owned(), "--json".to_owned()],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::InlineCommand { program, args } => {
                assert_eq!(program, "/usr/local/bin/workhive");
                assert_eq!(
                    args,
                    &vec!["status".to_owned(), "--json".to_owned()],
                    "real `workhive status` user command must NOT be neutralised",
                );
            }
            other => panic!(
                "expected user `workhive status` to survive hydration unchanged, got {other:?}",
            ),
        }
    }

    #[test]
    fn hydrate_preserves_real_workhive_status_user_command_bare_program() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "workhive".to_owned(),
            args: vec!["status".to_owned()],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::InlineCommand { program, args } => {
                assert_eq!(program, "workhive");
                assert_eq!(args, &vec!["status".to_owned()]);
            }
            other => panic!(
                "expected bare `workhive status` to survive hydration unchanged, got {other:?}",
            ),
        }
    }

    /// Defensive helper sanity: a real user `InlineCommand` (e.g. a
    /// Python HTTP server) must NOT be misclassified as a supervisor
    /// wrapper.
    #[test]
    fn hydrate_does_not_neutralise_real_user_inline_command() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "python".to_owned(),
            args: vec!["-m".to_owned(), "http.server".to_owned()],
        };

        let prior = HashMap::new();

        let hydrated = hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        assert_eq!(hydrated, 0);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::InlineCommand { program, args } => {
                assert_eq!(program, "python");
                assert_eq!(args, &vec!["-m".to_owned(), "http.server".to_owned()]);
            }
            other => panic!("user inline command must survive hydration, got {other:?}"),
        }
    }

    /// A quote-split known-tool inline command
    /// (`InlineCommand { program: "\"cursor-agent", args: ["--yolo\""] }`)
    /// is normalized during hydration back to a typed `ToolRef` — the
    /// classifier recovers it without a baseline — so the quote-split
    /// shape never reaches restore or the next persisted baseline.
    #[test]
    fn hydrate_heals_quote_polluted_tool_inline_command_without_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "\"cursor-agent".to_owned(),
            args: vec!["--yolo\"".to_owned()],
        };
        snapshot.panes[0].observed.current_command = Some("node".to_owned());

        let prior = HashMap::new();

        hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::ToolRef { tool_name } => assert_eq!(tool_name, "cursor-agent"),
            other => panic!("expected ToolRef after healing quote pollution, got {other:?}"),
        }
        assert!(matches!(
            snapshot.panes[0].restore_policy,
            crate::domain::RestorePolicy::Restore
        ));
    }

    /// A genuine user inline command with a quoted argument must NOT be
    /// rewritten by the quote-split healer.
    #[test]
    fn hydrate_preserves_inline_command_with_quoted_argument() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::InlineCommand {
            program: "python".to_owned(),
            args: vec!["-c".to_owned(), "print('hi')".to_owned()],
        };

        let prior = HashMap::new();

        hydrate_snapshot_with_baseline(&mut snapshot, &prior);
        match &snapshot.panes[0].launch_intent {
            PaneLaunchIntent::InlineCommand { program, args } => {
                assert_eq!(program, "python");
                assert_eq!(args, &vec!["-c".to_owned(), "print('hi')".to_owned()]);
            }
            other => panic!("expected inline command to survive hydration, got {other:?}"),
        }
    }

    #[test]
    fn restored_snapshot_entries_recover_from_prior_baseline() {
        let mut snapshot = snapshot_with_tool_launch();
        snapshot.panes[0].launch_intent = PaneLaunchIntent::NonRestorable {
            display: "claude".to_owned(),
            reason: "missing durable launch metadata".to_owned(),
        };

        let mut unit_by_position = HashMap::new();
        unit_by_position.insert((0, 0), UnitId::new("%1"));

        let mut prior = HashMap::new();
        prior.insert(
            (0u32, 0u32),
            crate::domain::PaneLaunchRef::Tool {
                name: crate::domain::ToolName::new("claude").expect("valid"),
            },
        );

        let entries =
            entries_from_restored_snapshot(&snapshot, &unit_by_position, "srv", "sess", &prior);

        assert_eq!(entries.len(), 1);
        assert!(matches!(
            entries[0].launch_origin,
            crate::domain::PaneLaunchRef::Tool { .. }
        ));
    }
}

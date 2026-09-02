//! Shared fixtures for `open` command tests.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
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
use workhive_core::usecases::env::{EffectiveEnvIssue, EnvSource};
#[allow(unused_imports)]
use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

pub(crate) fn base_runtime() -> FakeRuntime {
    let runtime = FakeRuntime::new();
    runtime
        .create_workspace(CreateWorkspaceRuntimeRequest {
            group: "srv".to_owned(),
            workspace_name: "sess".to_owned(),
            working_dir: Utf8PathBuf::from("/tmp"),
            env: Vec::new(),
        })
        .expect("workspace should be created for layout test");
    runtime
}

pub(crate) fn yaml_issue(path: &str, line: u32) -> EffectiveEnvIssue {
    EffectiveEnvIssue {
        source: Utf8PathBuf::from(path),
        issue: workhive_core::ports::EnvLineIssue {
            line,
            kind: workhive_core::ports::EnvLineIssueKind::LooksLikeYaml,
        },
    }
}

/// Render-time guarantee: the English warning for a YAML-style
/// malformed line includes the source path, the 1-based line
/// number, and the localized YAML-syntax reason — so a user
/// looking at stderr can find the offending file and line without
/// the CLI having to echo the raw line itself.
pub(crate) fn make_snapshot(
    panes: Vec<workhive_core::domain::PaneSnapshot>,
) -> workhive_core::domain::WorkspaceSnapshot {
    use std::collections::BTreeMap;
    use workhive_core::domain::{
        PaneRef, SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, SnapshotRevisionId, SnapshotSource,
        WindowSnapshot, WorkspaceSnapshot, WorkspaceStateKey, WorkspaceTopology,
    };
    // Group panes by window so each `WindowSnapshot` carries its
    // own `Vec<PaneRef>`. BTreeMap keeps window order stable.
    let mut by_window: BTreeMap<u32, Vec<PaneRef>> = BTreeMap::new();
    for p in &panes {
        by_window.entry(p.window_index).or_default().push(PaneRef {
            pane_index: p.pane_index,
            window_index: p.window_index,
        });
    }
    let windows = by_window
        .into_iter()
        .map(|(idx, refs)| WindowSnapshot {
            index: idx,
            name: Some(format!("window-{idx}")),
            layout: "tiled".to_owned(),
            panes: refs,
        })
        .collect();
    WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId::first(),
        state_key: WorkspaceStateKey::new(
            "inst".to_owned(),
            "proj".to_owned(),
            "sub".to_owned(),
            "ws".to_owned(),
        ),
        captured_at: "2026-05-06T00:00:00Z".to_owned(),
        source: SnapshotSource::ManualSave,
        topology: WorkspaceTopology {
            windows,
            active_window: Some(0),
            active_pane_runtime_id: None,
        },
        panes,
        metadata: SnapshotMetadata {
            runtime_backend: "tmux".to_owned(),
            platform: "macos".to_owned(),
            generation: 1,
            content_hash: None,
            config_fingerprint: None,
        },
    }
}

pub(crate) fn make_pane(
    window_index: u32,
    pane_index: u32,
    intent: workhive_core::domain::PaneLaunchIntent,
    observed_command: Option<&str>,
    observed_dead: bool,
) -> workhive_core::domain::PaneSnapshot {
    use workhive_core::domain::{PaneObservedState, PaneSnapshot, RestorePolicy};
    PaneSnapshot {
        runtime_id: None,
        window_index,
        pane_index,
        pane_id: None,
        title: None,
        cwd: camino::Utf8PathBuf::from("/tmp/proj"),
        managed: true,
        restore_policy: RestorePolicy::Restore,
        launch_intent: intent,
        observed: PaneObservedState {
            current_command: observed_command.map(|s| s.to_owned()),
            start_command: observed_command.map(|s| s.to_owned()),
            active: pane_index == 0 && window_index == 0,
            dead: observed_dead,
        },
    }
}

pub(crate) fn position_map(
    items: &[(u32, u32, &str)],
) -> std::collections::HashMap<(u32, u32), workhive_core::domain::UnitId> {
    items
        .iter()
        .map(|(w, p, id)| {
            (
                (*w, *p),
                workhive_core::domain::UnitId::new((*id).to_owned()),
            )
        })
        .collect()
}

pub(crate) fn worktree_info(branch: &str, is_main: bool) -> workhive_core::ports::WorktreeInfo {
    workhive_core::ports::WorktreeInfo {
        path: Utf8PathBuf::from(format!("/tmp/{branch}")),
        branch: Some(branch.to_owned()),
        is_main,
        resolved_strategy: None,
    }
}

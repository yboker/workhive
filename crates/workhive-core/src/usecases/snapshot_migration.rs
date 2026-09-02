//! Snapshot migration pipeline that converts older schema versions to current schema.
//!
//! Migration path: legacy document -> current workspace snapshot.
//!
//! Each step is explicit and testable. If a snapshot cannot be safely
//! migrated, it is rejected with `MigrationOutcome::Incompatible`.

use camino::Utf8PathBuf;

use crate::domain::{
    MigrationOutcome, PaneLaunchIntent, PaneObservedState, PaneRef, PaneSnapshot, RestorePolicy,
    SNAPSHOT_SCHEMA_VERSION, SchemaVersion, SnapshotMetadata, SnapshotRevisionId, SnapshotSource,
    WindowSnapshot, WorkspaceSnapshot, WorkspaceTopology,
};
use crate::errors::SnapshotError;
use crate::ports::{
    LegacySnapshotDocument as SnapshotDocument, LegacySnapshotUnit as SnapshotUnit,
};

/// Detect the schema version from raw JSON without fully deserializing.
pub fn detect_schema_version(raw_json: &str) -> Result<SchemaVersion, SnapshotError> {
    #[derive(serde::Deserialize)]
    struct VersionProbe {
        schema_version: u32,
    }

    let probe: VersionProbe =
        serde_json::from_str(raw_json).map_err(|e| SnapshotError::Corrupted {
            path: Utf8PathBuf::from("<raw>"),
            reason: format!("cannot read schema_version: {e}"),
        })?;

    SchemaVersion::from_version_number(probe.schema_version).ok_or_else(|| {
        SnapshotError::IncompatibleVersion {
            version: probe.schema_version,
            reason: format!(
                "unknown schema version {}, supported: 1, 2, 3",
                probe.schema_version
            ),
        }
    })
}

/// Migrate raw JSON to the current schema `WorkspaceSnapshot`.
///
/// Returns `(snapshot, outcome)`. If already at the current schema, the outcome is
/// `AlreadyCurrent`. If migration is not possible, returns an error.
pub fn migrate_to_current_schema(
    raw_json: &str,
) -> Result<(WorkspaceSnapshot, MigrationOutcome), SnapshotError> {
    let version = detect_schema_version(raw_json)?;

    match version {
        SchemaVersion::V3 => {
            let snapshot: WorkspaceSnapshot =
                serde_json::from_str(raw_json).map_err(|e| SnapshotError::Corrupted {
                    path: Utf8PathBuf::from("<raw>"),
                    reason: format!("V3 parse failed: {e}"),
                })?;
            Ok((snapshot, MigrationOutcome::AlreadyCurrent))
        }
        SchemaVersion::V2 => {
            let v2_doc: SnapshotDocument =
                serde_json::from_str(raw_json).map_err(|e| SnapshotError::Corrupted {
                    path: Utf8PathBuf::from("<raw>"),
                    reason: format!("V2 parse failed: {e}"),
                })?;
            let snapshot = migrate_legacy_document_to_current_snapshot(&v2_doc)?;
            Ok((
                snapshot,
                MigrationOutcome::Migrated {
                    from: SchemaVersion::V2,
                    to: SchemaVersion::V3,
                },
            ))
        }
        SchemaVersion::V1 => {
            let v2_doc = parse_initial_snapshot_as_legacy_document(raw_json)?;
            let snapshot = migrate_legacy_document_to_current_snapshot(&v2_doc)?;
            Ok((
                snapshot,
                MigrationOutcome::Migrated {
                    from: SchemaVersion::V1,
                    to: SchemaVersion::V3,
                },
            ))
        }
    }
}

/// Migrate a legacy `SnapshotDocument` to the current `WorkspaceSnapshot`.
fn migrate_legacy_document_to_current_snapshot(
    doc: &SnapshotDocument,
) -> Result<WorkspaceSnapshot, SnapshotError> {
    let state_key = crate::domain::WorkspaceStateKey::new(
        doc.identity.instance_id.clone(),
        doc.identity.project.clone(),
        String::new(),
        doc.identity.workspace.clone(),
    );

    let panes: Vec<PaneSnapshot> = doc
        .units
        .iter()
        .enumerate()
        .map(|(idx, unit)| convert_legacy_unit_to_current_pane(unit, idx as u32))
        .collect();

    let pane_refs: Vec<PaneRef> = panes
        .iter()
        .map(|p| PaneRef {
            pane_index: p.pane_index,
            window_index: 0,
        })
        .collect();

    let topology = WorkspaceTopology {
        windows: vec![WindowSnapshot {
            index: 0,
            name: None,
            layout: "tiled".to_owned(),
            panes: pane_refs,
        }],
        active_window: Some(0),
        active_pane_runtime_id: None,
    };

    Ok(WorkspaceSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        revision_id: SnapshotRevisionId::first(),
        state_key,
        captured_at: doc.meta.captured_at.clone(),
        source: SnapshotSource::ManualSave,
        topology,
        panes,
        metadata: SnapshotMetadata {
            runtime_backend: doc.meta.runtime_backend.clone(),
            platform: doc.meta.platform.clone(),
            generation: doc.meta.generation,
            content_hash: None,
            config_fingerprint: doc.meta.config_fingerprint.clone(),
        },
    })
}

/// Convert a legacy `SnapshotUnit` to a current `PaneSnapshot`.
fn convert_legacy_unit_to_current_pane(unit: &SnapshotUnit, default_index: u32) -> PaneSnapshot {
    let launch_intent = convert_launch_capture_to_intent(&unit.launch);

    PaneSnapshot {
        runtime_id: None,
        window_index: 0,
        pane_index: unit.index.max(default_index),
        pane_id: None,
        title: unit.title.clone(),
        cwd: unit.cwd.clone(),
        managed: true,
        restore_policy: RestorePolicy::Restore,
        launch_intent,
        observed: PaneObservedState {
            current_command: unit.state.clone(),
            start_command: None,
            active: unit.active,
            dead: false,
        },
    }
}

/// Convert a legacy `LaunchCapture` to a current `PaneLaunchIntent`.
fn convert_launch_capture_to_intent(capture: &crate::domain::LaunchCapture) -> PaneLaunchIntent {
    match capture {
        crate::domain::LaunchCapture::ToolRef { tool_name } => PaneLaunchIntent::ToolRef {
            tool_name: tool_name.clone(),
        },
        crate::domain::LaunchCapture::Exec { program, args } => PaneLaunchIntent::InlineCommand {
            program: program.clone(),
            args: args.clone(),
        },
        crate::domain::LaunchCapture::Shell { .. } => PaneLaunchIntent::EmptyShell,
        crate::domain::LaunchCapture::NonRestorable { display, reason } => {
            PaneLaunchIntent::NonRestorable {
                display: display.clone(),
                reason: reason.clone(),
            }
        }
    }
}

/// Attempt to parse an initial-schema raw JSON payload into a legacy `SnapshotDocument`.
///
/// Initial snapshots had `schema_version: 1` and a similar structure but
/// lacked `state_key` in their identity. We parse them into the legacy
/// document shape by keeping `state_key` absent and upgrading the stored
/// version number before the current migration step.
fn parse_initial_snapshot_as_legacy_document(
    raw_json: &str,
) -> Result<SnapshotDocument, SnapshotError> {
    let mut doc: SnapshotDocument =
        serde_json::from_str(raw_json).map_err(|e| SnapshotError::MigrationFailed {
            from: 1,
            to: 2,
            reason: format!("V1 parse failed: {e}"),
        })?;

    doc.schema_version = 2;
    // Initial snapshots did not have state_key; keep it absent for compatibility.
    if doc.identity.state_key.is_none() {
        doc.identity.state_key = None;
    }

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::LaunchCapture;
    use crate::ports::{
        LEGACY_SNAPSHOT_SCHEMA_VERSION, LegacySnapshotIdentity as SnapshotIdentity,
        LegacySnapshotMeta as SnapshotMeta,
    };

    fn sample_legacy_json() -> String {
        let doc = SnapshotDocument {
            schema_version: LEGACY_SNAPSHOT_SCHEMA_VERSION,
            identity: SnapshotIdentity {
                instance_id: "a230".to_owned(),
                project: "proj".to_owned(),
                workspace: "feat-a".to_owned(),
                server_name: "whsrv-a230-proj".to_owned(),
                session_name: "wh@a230@proj@feat-a".to_owned(),
                state_key: Some("a230@proj@feat-a".to_owned()),
            },
            meta: SnapshotMeta {
                runtime_backend: "tmux".to_owned(),
                platform: "macos".to_owned(),
                captured_at: "2026-04-20T11:30:00Z".to_owned(),
                generation: 1,
                config_fingerprint: None,
            },
            units: vec![
                SnapshotUnit {
                    index: 0,
                    active: true,
                    title: Some("shell".to_owned()),
                    cwd: "/tmp/proj".into(),
                    kind: None,
                    role: None,
                    launch: LaunchCapture::Shell {
                        executable: "zsh".to_owned(),
                        args: vec![],
                    },
                    state: Some("running".to_owned()),
                    last_error: None,
                    updated_at: None,
                },
                SnapshotUnit {
                    index: 1,
                    active: false,
                    title: None,
                    cwd: "/tmp/proj".into(),
                    kind: None,
                    role: None,
                    launch: LaunchCapture::ToolRef {
                        tool_name: "lazygit".to_owned(),
                    },
                    state: Some("running".to_owned()),
                    last_error: None,
                    updated_at: None,
                },
            ],
        };
        serde_json::to_string_pretty(&doc).expect("serialize legacy snapshot")
    }

    fn sample_initial_json() -> String {
        let doc = SnapshotDocument {
            schema_version: 1,
            identity: SnapshotIdentity {
                instance_id: "b340".to_owned(),
                project: "old-proj".to_owned(),
                workspace: "main".to_owned(),
                server_name: "whsrv-b340-old-proj".to_owned(),
                session_name: "wh@b340@old-proj@main".to_owned(),
                state_key: None,
            },
            meta: SnapshotMeta {
                runtime_backend: "tmux".to_owned(),
                platform: "linux".to_owned(),
                captured_at: "2026-01-01T00:00:00Z".to_owned(),
                generation: 0,
                config_fingerprint: None,
            },
            units: vec![SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: "/home/user/proj".into(),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "bash".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            }],
        };
        serde_json::to_string_pretty(&doc).expect("serialize initial snapshot")
    }

    #[test]
    fn detect_legacy_schema_version() {
        let json = sample_legacy_json();
        let version = detect_schema_version(&json).unwrap();
        assert_eq!(version, SchemaVersion::V2);
    }

    #[test]
    fn detect_initial_schema_version() {
        let json = sample_initial_json();
        let version = detect_schema_version(&json).unwrap();
        assert_eq!(version, SchemaVersion::V1);
    }

    #[test]
    fn detect_unknown_version_errors() {
        let json = r#"{"schema_version": 99}"#;
        let err = detect_schema_version(json).unwrap_err();
        assert!(matches!(
            err,
            SnapshotError::IncompatibleVersion { version: 99, .. }
        ));
    }

    #[test]
    fn migrate_legacy_snapshot_to_current_success() {
        let json = sample_legacy_json();
        let (snap, outcome) = migrate_to_current_schema(&json).unwrap();
        assert!(matches!(
            outcome,
            MigrationOutcome::Migrated {
                from: SchemaVersion::V2,
                to: SchemaVersion::V3
            }
        ));
        assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snap.panes.len(), 2);
        assert_eq!(snap.topology.windows.len(), 1);
        assert_eq!(snap.state_key.workspace, "feat-a");

        // First pane: shell -> EmptyShell
        assert!(matches!(
            &snap.panes[0].launch_intent,
            PaneLaunchIntent::EmptyShell
        ));
        // Second pane: tool ref preserved
        assert!(matches!(
            &snap.panes[1].launch_intent,
            PaneLaunchIntent::ToolRef { tool_name } if tool_name == "lazygit"
        ));
    }

    #[test]
    fn migrate_initial_snapshot_to_current_success() {
        let json = sample_initial_json();
        let (snap, outcome) = migrate_to_current_schema(&json).unwrap();
        assert!(matches!(
            outcome,
            MigrationOutcome::Migrated {
                from: SchemaVersion::V1,
                to: SchemaVersion::V3
            }
        ));
        assert_eq!(snap.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snap.panes.len(), 1);
        assert_eq!(snap.state_key.workspace, "main");
    }

    #[test]
    fn current_schema_roundtrip_is_already_current() {
        let legacy_json = sample_legacy_json();
        let (snap, _) = migrate_to_current_schema(&legacy_json).unwrap();
        let current_json = serde_json::to_string_pretty(&snap).unwrap();
        let (snap2, outcome) = migrate_to_current_schema(&current_json).unwrap();
        assert!(matches!(outcome, MigrationOutcome::AlreadyCurrent));
        assert_eq!(snap2.schema_version, SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snap2.panes.len(), snap.panes.len());
    }

    #[test]
    fn corrupted_json_errors() {
        let err = detect_schema_version("not json").unwrap_err();
        assert!(matches!(err, SnapshotError::Corrupted { .. }));
    }

    #[test]
    fn launch_capture_to_intent_mapping() {
        assert!(matches!(
            convert_launch_capture_to_intent(&LaunchCapture::Shell {
                executable: "zsh".into(),
                args: vec![]
            }),
            PaneLaunchIntent::EmptyShell
        ));

        assert!(matches!(
            convert_launch_capture_to_intent(&LaunchCapture::ToolRef {
                tool_name: "vim".into()
            }),
            PaneLaunchIntent::ToolRef { tool_name } if tool_name == "vim"
        ));

        assert!(matches!(
            convert_launch_capture_to_intent(&LaunchCapture::Exec {
                program: "python".into(),
                args: vec!["app.py".into()]
            }),
            PaneLaunchIntent::InlineCommand { program, args }
                if program == "python" && args == vec!["app.py"]
        ));

        assert!(matches!(
            convert_launch_capture_to_intent(&LaunchCapture::NonRestorable {
                display: "ssh".into(),
                reason: "remote".into()
            }),
            PaneLaunchIntent::NonRestorable { .. }
        ));
    }
}

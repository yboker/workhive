//! Insta snapshot tests for domain types and port DTOs.
//!
//! These tests lock down the serialization format of key structures.

use camino::Utf8PathBuf;
use workhive_core::domain::LaunchCapture;
use workhive_core::domain::{Locale, PaneKind, PaneState, WorkspaceId};
use workhive_core::ports::{
    AppConfig, LEGACY_SNAPSHOT_SCHEMA_VERSION, LegacySnapshotDocument as SnapshotDocument,
    LegacySnapshotIdentity as SnapshotIdentity, LegacySnapshotMeta as SnapshotMeta,
    LegacySnapshotUnit as SnapshotUnit, ProjectSummary, TmuxConfig,
};

#[test]
fn snapshot_app_config_default() {
    let config = AppConfig::default();
    insta::assert_json_snapshot!("app_config_default", config);
}

#[test]
fn snapshot_app_config_full() {
    let config = AppConfig {
        language: Some(Locale::ZhCn),
        tmux: Some(TmuxConfig {
            server_name: "custom-server".to_owned(),
            naming: None,
        }),
        layout: None,
        snapshot: None,
        logging: None,
        projects: None,
        discovery: None,
        tools: None,
        worktree_root: Some(Utf8PathBuf::from("/custom/worktrees")),
        snapshot_root: Some(Utf8PathBuf::from("/custom/snapshots")),
        runtime_root: Some(Utf8PathBuf::from("/custom/runtime")),
        features: None,
        pane: None,
    };
    insta::assert_json_snapshot!("app_config_full", config);
}

#[test]
fn snapshot_project_summary() {
    let summary = ProjectSummary {
        project_name: "my-project".to_owned(),
        project_root: Utf8PathBuf::from("/Users/dev/my-project"),
        config_path: Utf8PathBuf::from("/Users/dev/my-project/.workhive/config.toml"),
        last_opened_at: Some("2026-04-14T00:00:00Z".to_owned()),
        last_updated_at: None,
    };
    insta::assert_json_snapshot!("project_summary", summary);
}

#[test]
fn snapshot_snapshot_document() {
    let doc = SnapshotDocument {
        schema_version: LEGACY_SNAPSHOT_SCHEMA_VERSION,
        identity: SnapshotIdentity {
            instance_id: "test-instance-001".to_owned(),
            project: "proj".to_owned(),
            workspace: "feat-auth".to_owned(),
            server_name: "wh-proj".to_owned(),
            session_name: "wh--proj--feat-auth".to_owned(),
            state_key: None,
        },
        meta: SnapshotMeta {
            runtime_backend: "tmux".to_owned(),
            platform: "macos".to_owned(),
            captured_at: "2026-04-20T12:00:00Z".to_owned(),
            generation: 1,
            config_fingerprint: None,
        },
        units: vec![
            SnapshotUnit {
                index: 0,
                active: true,
                title: None,
                cwd: Utf8PathBuf::from("/Users/dev/proj"),
                kind: None,
                role: None,
                launch: LaunchCapture::Shell {
                    executable: "zsh".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            },
            SnapshotUnit {
                index: 1,
                active: false,
                title: Some("editor".to_owned()),
                cwd: Utf8PathBuf::from("/Users/dev/proj"),
                kind: None,
                role: None,
                launch: LaunchCapture::Exec {
                    program: "vim".to_owned(),
                    args: vec![],
                },
                state: None,
                last_error: None,
                updated_at: None,
            },
        ],
    };
    insta::assert_json_snapshot!("snapshot_document", doc);
}

#[test]
fn snapshot_workspace_id_display() {
    let id = WorkspaceId::new("feat-auth-next").unwrap();
    insta::assert_snapshot!("workspace_id_display", id);
}

#[test]
fn snapshot_pane_kind_variants() {
    let kinds = vec![
        PaneKind::Agent,
        PaneKind::Terminal,
        PaneKind::GitUi,
        PaneKind::Unknown,
    ];
    insta::assert_json_snapshot!("pane_kind_variants", kinds);
}

#[test]
fn snapshot_pane_state_variants() {
    let states = vec![PaneState::Idle, PaneState::Running, PaneState::Unknown];
    insta::assert_json_snapshot!("pane_state_variants", states);
}

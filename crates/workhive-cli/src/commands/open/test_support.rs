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

pub(crate) fn worktree_info(branch: &str, is_main: bool) -> workhive_core::ports::WorktreeInfo {
    workhive_core::ports::WorktreeInfo {
        path: Utf8PathBuf::from(format!("/tmp/{branch}")),
        branch: Some(branch.to_owned()),
        is_main,
        resolved_strategy: None,
    }
}

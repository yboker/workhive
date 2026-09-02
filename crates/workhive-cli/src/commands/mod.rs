use std::io::IsTerminal;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::{
    Locale, NamingContext, PaneDirection, ResolvedPaneLaunch, RuntimeBackendId, SessionName,
    SessionNamingPolicy, SnapshotEvent, SnapshotEventKind, SnapshotRevisionId, SnapshotSource,
    SupervisorLock, ToolCatalog, WorkspaceId, WorkspaceStateKey,
};
use workhive_core::errors::SnapshotError;
use workhive_core::ports::{
    ConfigPort, EnvironmentPort, GitPort, I18nPort, LayoutConfig, PaneRegistryPort,
    PaneRuntimePort, ProjectDiscoveryPort, ProjectIndexPort, RuntimeTopologySnapshot,
    SnapshotConfig, SnapshotHookMode, SnapshotRuntimePort, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceRuntimePort,
};
use workhive_core::usecases;
use workhive_infra::i18n::StaticI18n;

use crate::cli::{ConfigAction, InternalSnapshotAction, ListView, PaneAction};
use crate::composition::CliComposition;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::{confirm, policy, table, ui, wizard};

pub(crate) mod activity;
pub(crate) mod attach_usage;
pub(crate) mod close;
pub(crate) mod completions;
pub(crate) mod control_plane;
pub(crate) mod doctor;
pub(crate) mod integrations;
pub(crate) mod internal_completion;
pub(crate) mod list;
pub(crate) mod list_render;
pub(crate) mod notification_trigger;
pub(crate) mod open;
pub(crate) mod open_readiness;
pub(crate) mod pane;
pub(crate) mod pane_activity_overlay;
pub(crate) mod pane_control_plane;
pub(crate) mod pane_list;
pub(crate) mod pane_metrics_human;
pub(crate) mod pane_metrics_view;
pub(crate) mod pane_send;
pub(crate) mod pane_show;
pub(crate) mod pane_supervisor;
pub(crate) mod pane_title_guard;
pub(crate) mod pane_view_data;
pub(crate) mod project;
pub(crate) mod session;
pub(crate) mod snapshot;
pub(crate) mod worktree;

pub(crate) use activity::*;
pub(crate) use close::*;
pub(crate) use completions::*;
pub(crate) use control_plane::*;
pub(crate) use doctor::*;
// `integrations` is an *internal* automatic lifecycle module, not a
// user CLI handler. It is imported directly by call sites
// (pane_supervisor, project::execute_init) via
// `crate::commands::integrations::prepare_provider` and intentionally
// not wildcard-re-exported.
pub(crate) use internal_completion::*;
pub(crate) use list::*;
#[allow(unused_imports)]
pub(crate) use list_render::*;
pub(crate) use open::*;
pub(crate) use pane::*;
pub(crate) use pane_supervisor::*;
pub(crate) use pane_title_guard::*;
pub(crate) use project::*;
pub(crate) use session::*;
pub(crate) use snapshot::*;
pub(crate) use worktree::*;

pub(crate) fn localized_bool(ctx: &CommandContext, locale: Locale, value: bool) -> String {
    use workhive_core::i18n_keys as k;

    if value {
        ctx.i18n.resolve(locale, k::BOOL_YES)
    } else {
        ctx.i18n.resolve(locale, k::BOOL_NO)
    }
}

pub(crate) fn print_choice_transcript(prompt: &str, choice: &str) {
    eprintln!("  {prompt} → {choice}");
}

pub(crate) fn confirm_with_transcript(
    ctx: &CommandContext,
    locale: Locale,
    prompt: &str,
    default: bool,
) -> Result<bool, CliError> {
    let value = crate::ui::screens::confirm::run(prompt, default)?;
    print_choice_transcript(prompt, &localized_bool(ctx, locale, value));
    Ok(value)
}

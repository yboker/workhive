//! `workhive project` command family: setup/init, config show/edit/set,
//! trusted-roots management, reindex, worktree safety, and destroy/remove
//! flows.

mod config;
mod config_overview;
mod destroy;
mod init;
mod reindex;
mod roots;
mod roots_cmd;
mod setup;
mod worktree_safety;

pub(crate) use config::*;
pub(crate) use config_overview::*;
pub(crate) use destroy::*;
pub(crate) use init::*;
pub(crate) use reindex::*;
pub(crate) use roots::*;
pub(crate) use roots_cmd::*;
pub(crate) use setup::*;
pub(crate) use worktree_safety::*;

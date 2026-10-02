//! Workspace and session lifecycle: open/attach/close/destroy, list, worktree, layout, and recovery.

use std::collections::HashMap;

mod list_worktree_layout;
mod open_attach_close;
mod recovery;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    open_attach_close::register_en(m);
    list_worktree_layout::register_en(m);
    recovery::register_en(m);
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    open_attach_close::register_zh_cn(m);
    list_worktree_layout::register_zh_cn(m);
    recovery::register_zh_cn(m);
}

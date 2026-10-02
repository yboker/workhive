//! `workhive workspace attach` / `close` command family: attach entry
//! points, project and global attach selectors, and the close flow.

mod attach;
mod attach_selector;
mod close_flow;
mod global_attach;
#[cfg(test)]
mod selector_tests;

fn last_visible_column(flags: &[bool]) -> usize {
    flags
        .iter()
        .rposition(|visible| *visible)
        .map_or(0, |index| index + 1)
}

pub(crate) use attach::*;
pub(crate) use attach_selector::*;
pub(crate) use close_flow::*;
pub(crate) use global_attach::*;

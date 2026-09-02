//! Pane surface: pane list/show, activity/state labels, supervisor diagnostics, and AI-integration prepare copy.

use std::collections::HashMap;

mod activity_status;
mod pane_list_show;
mod provider_integration;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    pane_list_show::register_en(m);
    activity_status::register_en(m);
    provider_integration::register_en(m);
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    pane_list_show::register_zh_cn(m);
    activity_status::register_zh_cn(m);
    provider_integration::register_zh_cn(m);
}

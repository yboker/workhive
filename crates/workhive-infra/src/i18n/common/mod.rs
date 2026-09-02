//! Cross-cutting copy: CLI help/clap templates, generic errors, confirmations, validation, and shared input-widget labels.

use std::collections::HashMap;

mod cli_help;
mod confirm_input;
mod errors_validation;

pub(super) fn register_en(m: &mut HashMap<&'static str, &'static str>) {
    confirm_input::register_en(m);
    errors_validation::register_en(m);
    cli_help::register_en(m);
}

pub(super) fn register_zh_cn(m: &mut HashMap<&'static str, &'static str>) {
    confirm_input::register_zh_cn(m);
    errors_validation::register_zh_cn(m);
    cli_help::register_zh_cn(m);
}

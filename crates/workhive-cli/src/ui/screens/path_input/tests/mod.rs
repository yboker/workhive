//! Behavioral tests for the trusted-path input screen.
//!
//! Tests are grouped into workflow-focused submodules. This module owns
//! the shared fakes, option builders, and strategy helpers that every
//! submodule reuses through `use super::*;`.

use super::render::{AddedRootsSnapshot, compute_added_summary_height};
use super::state::State;
use super::{
    AddedRootsSummary, COMPACT_BASE_ROWS, CandidateWindow, MAX_VALIDATION_ERROR_HEIGHT,
    MAX_VISIBLE_ADDED_ROOTS, MAX_VISIBLE_CANDIDATES, MIN_CONTENT_WIDTH, PathInputOptions,
    added_summary_visible_rows, candidate_window, compute_child_label, compute_preview,
    compute_scope, compute_viewport_height, content_width_for_area, format_label,
    reservation_from_initial_candidates, resolve_inline_viewport_spec,
    validation_error_wrapped_height,
};
use crate::ui::severity::MessageSeverity;
use crate::ui::validated_input::ValidatedInputRetry;
use camino::Utf8Path;
use workhive_core::ports::{PathCompletionEntry, PathCompletionFsPort, PathKind};
use workhive_core::usecases::path_strategy::{UnixPathStrategy, WindowsPathStrategy};

mod added_summary;
mod candidates;
mod path_rules;
mod snapshot;
mod viewport;

/// macOS / Linux build-target strategy (the requirement explicitly
/// forbids splitting them into two different strategies).
fn unix_strategy() -> UnixPathStrategy {
    UnixPathStrategy
}

/// Windows build-target strategy used by the production-path
/// scope/preview tests below.
fn windows_strategy() -> WindowsPathStrategy {
    WindowsPathStrategy
}

/// Test fake that lets the test author wire `parent -> children`
/// per case so we can exercise live-search across multiple
/// directories without spinning up a real filesystem.
struct FakeFs {
    children: std::collections::HashMap<camino::Utf8PathBuf, Vec<PathCompletionEntry>>,
}

impl FakeFs {
    fn new() -> Self {
        Self {
            children: std::collections::HashMap::new(),
        }
    }

    fn dirs(mut self, parent: &str, names: &[&str]) -> Self {
        self.children.insert(
            camino::Utf8PathBuf::from(parent),
            names
                .iter()
                .map(|name| PathCompletionEntry {
                    name: (*name).to_owned(),
                    is_directory: true,
                })
                .collect(),
        );
        self
    }
}

impl PathCompletionFsPort for FakeFs {
    fn list_children(&self, parent: &Utf8Path) -> Result<Vec<PathCompletionEntry>, std::io::Error> {
        Ok(self.children.get(parent).cloned().unwrap_or_default())
    }

    fn classify_path(&self, _path: &Utf8Path) -> PathKind {
        PathKind::Missing
    }
}

fn options(initial: &'static str) -> PathInputOptions<'static> {
    PathInputOptions {
        prompt: "path:",
        initial,
        footer_hint: "Tab",
        no_match_hint: "none",
        dir_no_children_hint: "no child directories",
        home: Some(Utf8Path::new("/Users/me")),
        field_label: "Trusted root path",
        placeholder: "Browsing under ~/",
        scope_label_template: "Current location: {path}",
        preview_label_template: "Will add: {path}",
        bare_scope_warning: "Pick a child directory",
        relative_warning: "Trusted roots must be absolute",
        scope_no_match_warning: "no match — `{0}` does not exist under the current browse scope",
        validation_error: None,
        added_summary: AddedRootsSummary::EMPTY,
    }
}

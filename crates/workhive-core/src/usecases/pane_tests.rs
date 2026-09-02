use super::*;
use crate::domain::{PaneKind, PaneLaunchOrigin, PaneState, ToolName, UnitId};
use camino::Utf8PathBuf;

fn snapshot(
    id: &str,
    index: u32,
    command: &str,
    kind: PaneKind,
    state: PaneState,
    active: bool,
) -> RuntimeUnitSnapshot {
    RuntimeUnitSnapshot {
        id: UnitId::new(id),
        index,
        cwd: Utf8PathBuf::from("/tmp/work"),
        command: command.to_owned(),
        active,
        kind,
        state,
        tty: None,
        process_pid: None,
    }
}

fn identity(workspace: &str, unit_id: &str, tool: &str) -> PaneLaunchIdentityEntry {
    PaneLaunchIdentityEntry {
        workspace: workspace.to_owned(),
        unit_id: UnitId::new(unit_id),
        launch_origin: PaneLaunchOrigin::Tool {
            name: ToolName::new(tool).expect("tool name fixture"),
        },
    }
}

#[test]
fn list_view_keeps_live_kind_state_and_command_when_identity_is_stale_tool() {
    // Fixture:
    //   * runtime command = "zsh"
    //   * durable identity = "codex"
    // The live pane has returned to a shell. Launch identity must
    // be auxiliary only; it must NOT override the observed
    // `kind` (Terminal), the observed `state` (Idle), or the
    // observed `current_command` (zsh).
    let live = vec![snapshot(
        "%2",
        1,
        "zsh",
        PaneKind::Terminal,
        PaneState::Idle,
        false,
    )];
    let identity = vec![identity("ws", "%2", "codex")];

    let rows = build_pane_list_view(&live, &identity);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].current_command, "zsh");
    assert_eq!(rows[0].kind, PaneKind::Terminal);
    assert_eq!(rows[0].state, PaneState::Idle);
    assert_eq!(rows[0].launch_identity_title, Some("codex".to_owned()));
    assert!(!rows[0].active);
}

#[test]
fn list_view_falls_back_to_runtime_command_when_no_identity() {
    let live = vec![snapshot(
        "%0",
        0,
        "lazygit",
        PaneKind::GitUi,
        PaneState::Running,
        true,
    )];
    let rows = build_pane_list_view(&live, &[]);
    assert_eq!(rows[0].current_command, "lazygit");
    assert_eq!(rows[0].kind, PaneKind::GitUi);
    assert_eq!(rows[0].launch_identity_title, None);
    assert!(rows[0].active);
}

#[test]
fn list_view_recognizes_live_codex_as_agent() {
    // Live runtime really is running codex — adapter classifies
    // as Agent. Launch identity matches. View must surface kind
    // Agent (from runtime, not invented), and the launch identity
    // column is also "codex".
    let live = vec![snapshot(
        "%3",
        2,
        "codex",
        PaneKind::Agent,
        PaneState::Running,
        false,
    )];
    let identity = vec![identity("ws", "%3", "codex")];
    let rows = build_pane_list_view(&live, &identity);
    assert_eq!(rows[0].current_command, "codex");
    assert_eq!(rows[0].kind, PaneKind::Agent);
    assert_eq!(rows[0].state, PaneState::Running);
    assert_eq!(rows[0].launch_identity_title, Some("codex".to_owned()));
}

#[test]
fn list_view_preserves_unknown_state_when_runtime_cannot_tell() {
    // Never invent "Running" when the
    // runtime cannot determine state.
    let live = vec![snapshot(
        "%4",
        3,
        "",
        PaneKind::Unknown,
        PaneState::Unknown,
        false,
    )];
    let rows = build_pane_list_view(&live, &[]);
    assert_eq!(rows[0].state, PaneState::Unknown);
    assert_eq!(rows[0].kind, PaneKind::Unknown);
}

#[test]
fn show_view_keeps_live_kind_state_and_command_when_identity_is_stale_tool() {
    // `pane show` must
    // produce the SAME view model as `pane list` for the same
    // observation/identity inputs. Mirrors
    // `list_view_keeps_live_kind_state_and_command_when_identity_is_stale_tool`.
    let live = snapshot("%5", 4, "zsh", PaneKind::Terminal, PaneState::Idle, true);
    let identity = vec![identity("ws", "%5", "claude")];
    let view = build_pane_show_view(&live, &identity);
    assert_eq!(view.row.current_command, "zsh");
    assert_eq!(view.row.kind, PaneKind::Terminal);
    assert_eq!(view.row.state, PaneState::Idle);
    assert_eq!(view.row.launch_identity_title, Some("claude".to_owned()));
    assert!(view.row.active);
}

#[test]
fn show_view_recognizes_live_claude_as_agent() {
    // Companion to `list_view_recognizes_live_codex_as_agent`:
    // when the live runtime is the tool, kind stays Agent
    // because the runtime adapter said so — not because of
    // identity rewrite.
    let live = snapshot("%6", 5, "claude", PaneKind::Agent, PaneState::Running, true);
    let identity = vec![identity("ws", "%6", "claude")];
    let view = build_pane_show_view(&live, &identity);
    assert_eq!(view.row.current_command, "claude");
    assert_eq!(view.row.kind, PaneKind::Agent);
    assert_eq!(view.row.state, PaneState::Running);
    assert_eq!(view.row.launch_identity_title, Some("claude".to_owned()));
}

#[test]
fn pane_kind_i18n_key_maps_to_catalog_keys() {
    assert_eq!(
        PaneKind::Agent.i18n_key(),
        crate::i18n_keys::PANE_KIND_AGENT
    );
    assert_eq!(
        PaneKind::Terminal.i18n_key(),
        crate::i18n_keys::PANE_KIND_TERMINAL
    );
    assert_eq!(
        PaneKind::GitUi.i18n_key(),
        crate::i18n_keys::PANE_KIND_GIT_UI
    );
    assert_eq!(
        PaneKind::Unknown.i18n_key(),
        crate::i18n_keys::PANE_KIND_UNKNOWN
    );
}

#[test]
fn pane_state_i18n_key_maps_to_catalog_keys() {
    assert_eq!(
        PaneState::Running.i18n_key(),
        crate::i18n_keys::PANE_STATE_RUNNING
    );
    assert_eq!(
        PaneState::Idle.i18n_key(),
        crate::i18n_keys::PANE_STATE_IDLE
    );
    assert_eq!(
        PaneState::Unknown.i18n_key(),
        crate::i18n_keys::PANE_STATE_UNKNOWN
    );
}

#[test]
fn empty_shell_identity_does_not_override_runtime_observation() {
    // Launch identity is
    // strictly auxiliary. An `EmptyShell` identity must not
    // promote a runtime-observed `Unknown` kind to `Terminal`,
    // and an `EmptyShell` identity has no useful display title
    // (`launch_identity_title` returns `None` so the CLI does
    // not echo the literal `shell` next to a live `zsh`
    // command).
    let live = vec![snapshot(
        "%9",
        5,
        "zsh",
        PaneKind::Unknown,
        PaneState::Idle,
        false,
    )];
    let identity = vec![PaneLaunchIdentityEntry {
        workspace: "ws".to_owned(),
        unit_id: UnitId::new("%9"),
        launch_origin: PaneLaunchOrigin::EmptyShell,
    }];
    let rows = build_pane_list_view(&live, &identity);
    // Runtime said Unknown — view must preserve that.
    assert_eq!(rows[0].kind, PaneKind::Unknown);
    assert_eq!(rows[0].current_command, "zsh");
    assert_eq!(rows[0].launch_identity_title, None);
}

fn list_row_with(launch_identity_title: Option<&str>, current_command: &str) -> PaneListRow {
    PaneListRow {
        active: false,
        index: 1,
        target_id: UnitId::new("%1"),
        current_command: current_command.to_owned(),
        kind: PaneKind::Terminal,
        state: PaneState::Idle,
        activity: PaneActivityState::ShellIdle,
        message_safety: PaneMessageSafety::ShellInput,
        launch_identity_title: launch_identity_title.map(str::to_owned),
        source: PaneObservationSource::TmuxCurrentCommand,
        confidence: PaneObservationConfidence::Fallback,
        cwd: "/tmp/work".to_owned(),
    }
}

#[test]
fn pane_selection_label_prefers_launch_identity() {
    let row = list_row_with(Some("codex"), "zsh");
    assert_eq!(pane_selection_label(&row), Some("codex".to_owned()));
}

#[test]
fn pane_selection_label_falls_back_to_current_command() {
    let row = list_row_with(None, "lazygit");
    assert_eq!(pane_selection_label(&row), Some("lazygit".to_owned()));
}

#[test]
fn pane_selection_label_suppresses_internal_workhive_wrapper() {
    // Until the supervisor wrapper is fully hidden by the runtime
    // launch path, the live foreground command may still report
    // an internal `workhive` invocation. The default selection
    // label must not surface it as a user-recognizable tool.
    let row = list_row_with(None, "workhive internal pane-supervisor --unit %1");
    assert_eq!(pane_selection_label(&row), None);
}

#[test]
fn pane_selection_label_skips_internal_workhive_identity() {
    // Defensive: even if a stale identity entry stored `workhive`
    // as the launch identity, the label must still suppress it
    // and fall through to a non-internal command.
    let row = list_row_with(Some("workhive"), "zsh");
    assert_eq!(pane_selection_label(&row), Some("zsh".to_owned()));
}

/// tmux's `pane_current_command` carries the foreground process's
/// raw `argv[0]`. Login shells set that to `-zsh` / `-bash` so the
/// shell knows it's a login session. The simplified `pane list`
/// label should drop the leading dash so the column reads `zsh`,
/// not `-zsh` (which looks like a formatting bug). The full
/// argv stays available via `pane show`'s truthful command field.
#[test]
fn pane_selection_label_strips_login_shell_dash() {
    for raw in ["-zsh", "-bash", "-fish"] {
        let row = list_row_with(None, raw);
        assert_eq!(
            pane_selection_label(&row),
            Some(raw.trim_start_matches('-').to_owned()),
            "label for {raw} should be the bare shell name"
        );
    }
}

/// The dash-strip MUST only kick in for
/// recognized shell names. A user-launched binary that legitimately
/// starts with `-` (vanishingly rare, but possible for path
/// arguments) must round-trip unchanged so the user can still
/// recognize what they ran.
#[test]
fn pane_selection_label_preserves_dash_for_non_shell_commands() {
    let row = list_row_with(None, "-customtool");
    assert_eq!(
        pane_selection_label(&row),
        Some("-customtool".to_owned()),
        "non-shell labels must not be silently mutated"
    );
}

/// When the durable launch identity itself
/// somehow recorded `-zsh` (e.g. legacy snapshot data), the same
/// sanitization must apply on the identity branch.
#[test]
fn pane_selection_label_strips_login_shell_dash_from_launch_identity() {
    let row = list_row_with(Some("-bash"), "zsh");
    assert_eq!(pane_selection_label(&row), Some("bash".to_owned()));
}

// ── record_split_launch_metadata ─────────────────────────────────────
//
// After `pane split --cmd "codex"` the durable identity store
// and pane title MUST be populated immediately so that the next
// `pane list` / `pane show` recognizes the new pane without
// waiting for a `close`/`open` restore cycle.

use crate::domain::{CommandSpec, PaneLaunchRef};
use crate::ports::{
    PaneLaunchIdentityError, PaneLaunchIdentityPort, RuntimeError, RuntimeUnitSnapshot,
    SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest,
};
use std::cell::RefCell;

/// Minimal in-test fake for [`PaneLaunchIdentityPort`]. Records
/// every `record(...)` call so tests can assert the durable
/// identity write happened with the expected origin.
struct InlineIdentityStore {
    recorded: RefCell<Vec<(String, PaneLaunchIdentityEntry)>>,
    force_record_error: bool,
}
impl InlineIdentityStore {
    fn new() -> Self {
        Self {
            recorded: RefCell::new(Vec::new()),
            force_record_error: false,
        }
    }
    fn failing() -> Self {
        Self {
            recorded: RefCell::new(Vec::new()),
            force_record_error: true,
        }
    }
    fn recorded(&self) -> Vec<(String, PaneLaunchIdentityEntry)> {
        self.recorded.borrow().clone()
    }
}
impl PaneLaunchIdentityPort for InlineIdentityStore {
    fn record(
        &self,
        group: &str,
        entry: PaneLaunchIdentityEntry,
    ) -> Result<(), PaneLaunchIdentityError> {
        if self.force_record_error {
            return Err(PaneLaunchIdentityError::Group {
                group: group.to_owned(),
                reason: "synthetic test error".to_owned(),
            });
        }
        self.recorded.borrow_mut().push((group.to_owned(), entry));
        Ok(())
    }
    fn list_for_group(
        &self,
        _group: &str,
    ) -> Result<Vec<PaneLaunchIdentityEntry>, PaneLaunchIdentityError> {
        Ok(self
            .recorded
            .borrow()
            .iter()
            .map(|(_, e)| e.clone())
            .collect())
    }
    fn forget_workspace(
        &self,
        _group: &str,
        _workspace: &str,
    ) -> Result<(), PaneLaunchIdentityError> {
        Ok(())
    }
    fn forget_group(&self, _group: &str) -> Result<(), PaneLaunchIdentityError> {
        Ok(())
    }
}

/// Minimal in-test fake for [`PaneRuntimePort`]. Records title
/// updates so we can assert the pane border is reasserted on
/// split.
struct InlineRuntime {
    title_updates: RefCell<Vec<(String, String)>>,
    force_title_error: bool,
}
impl InlineRuntime {
    fn new() -> Self {
        Self {
            title_updates: RefCell::new(Vec::new()),
            force_title_error: false,
        }
    }
    fn failing() -> Self {
        Self {
            title_updates: RefCell::new(Vec::new()),
            force_title_error: true,
        }
    }
    fn title_updates(&self) -> Vec<(String, String)> {
        self.title_updates.borrow().clone()
    }
}
impl PaneRuntimePort for InlineRuntime {
    fn spawn_unit(&self, _request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        unreachable!("split metadata helper must not respawn units")
    }
    fn close_unit(&self, _group: &str, _unit_id: &UnitId) -> Result<(), RuntimeError> {
        unreachable!("split metadata helper must not close units")
    }
    fn list_units(
        &self,
        _group: &str,
        _workspace: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        Ok(Vec::new())
    }
    fn current_unit(&self, _group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        unreachable!("split metadata helper must not query current unit")
    }
    fn send_input(&self, _request: SendInputRequest) -> Result<(), RuntimeError> {
        unreachable!("split metadata helper must not send input")
    }
    fn set_unit_title(&self, request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
        if self.force_title_error {
            return Err(RuntimeError::OperationFailed {
                message: "synthetic test error".to_owned(),
            });
        }
        self.title_updates
            .borrow_mut()
            .push((request.target_unit.as_str().to_owned(), request.title));
        Ok(())
    }
}

#[test]
fn record_split_launch_metadata_writes_identity_and_title_for_tool() {
    let runtime = InlineRuntime::new();
    let store = InlineIdentityStore::new();
    let unit = UnitId::new("%4");
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("codex").unwrap(),
    };
    record_split_launch_metadata(&runtime, &store, "wh-srv", "alpha", &unit, &launch);

    // durable identity stored under (group, workspace, unit)
    let recorded = store.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "wh-srv");
    assert_eq!(recorded[0].1.workspace, "alpha");
    assert_eq!(recorded[0].1.unit_id.as_str(), "%4");
    assert!(matches!(
        &recorded[0].1.launch_origin,
        PaneLaunchOrigin::Tool { name } if name.as_str() == "codex"
    ));

    // pane title set to the tool's display name
    let titles = runtime.title_updates();
    assert_eq!(titles, vec![("%4".to_owned(), "codex".to_owned())]);
}

#[test]
fn record_split_launch_metadata_writes_inline_program_as_title() {
    let runtime = InlineRuntime::new();
    let store = InlineIdentityStore::new();
    let unit = UnitId::new("%5");
    let launch = PaneLaunchRef::InlineCommand {
        spec: CommandSpec::new("lazygit", ["--all"]),
    };
    record_split_launch_metadata(&runtime, &store, "wh-srv", "alpha", &unit, &launch);

    let recorded = store.recorded();
    assert_eq!(recorded.len(), 1);
    match &recorded[0].1.launch_origin {
        PaneLaunchOrigin::InlineCommand { spec } => {
            assert_eq!(spec.program(), "lazygit");
            assert_eq!(spec.args(), &["--all"]);
        }
        other => panic!("expected InlineCommand identity, got {other:?}"),
    }

    let titles = runtime.title_updates();
    // For inline commands the program token becomes the visible
    // title (mirrors `PaneLaunchRef::display_name`).
    assert_eq!(titles, vec![("%5".to_owned(), "lazygit".to_owned())]);
}

#[test]
fn record_split_launch_metadata_skips_writes_for_empty_shell() {
    let runtime = InlineRuntime::new();
    let store = InlineIdentityStore::new();
    let unit = UnitId::new("%6");
    record_split_launch_metadata(
        &runtime,
        &store,
        "wh-srv",
        "alpha",
        &unit,
        &PaneLaunchRef::EmptyShell,
    );

    assert!(
        store.recorded().is_empty(),
        "EmptyShell must not record identity"
    );
    assert!(
        runtime.title_updates().is_empty(),
        "EmptyShell must not set a pane title"
    );
}

#[test]
fn record_split_launch_metadata_is_fail_open_on_identity_error() {
    // Identity store fails — split metadata must still attempt
    // the title update so the user-visible pane border is
    // reasserted, and must not propagate the error.
    let runtime = InlineRuntime::new();
    let store = InlineIdentityStore::failing();
    let unit = UnitId::new("%7");
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("claude").unwrap(),
    };
    record_split_launch_metadata(&runtime, &store, "wh-srv", "alpha", &unit, &launch);

    // identity store rejected the write; nothing recorded
    assert!(store.recorded().is_empty());
    // title update still happened
    assert_eq!(
        runtime.title_updates(),
        vec![("%7".to_owned(), "claude".to_owned())]
    );
}

/// `pane split --cmd codex` must be visible as `codex` in `pane
/// list` immediately, without requiring a `close`/`open` cycle.
///
/// The previous bug stored the identity entry under the workspace
/// suffix (`main`) while `pane list` filtered by the canonical
/// session name (`wh@iid@proj@main`), so the lookup missed and
/// the row fell back to whatever tmux reported as the foreground
/// command. This test pins the contract: the workspace key used
/// by `record_split_launch_metadata` MUST be identical to the key
/// `pane list` filters by, and the resulting view-model row MUST
/// surface the tool's launch identity title.
#[test]
fn record_split_then_pane_list_view_uses_same_canonical_workspace_key() {
    use crate::domain::{
        PaneKind, PaneObservationConfidence, PaneObservationSource, PaneRuntimeTruth, PaneState,
    };
    use camino::Utf8PathBuf;

    let runtime = InlineRuntime::new();
    let store = InlineIdentityStore::new();
    let unit = UnitId::new("%9");
    let canonical_session = "wh@abcdef@proj@main";
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("codex").unwrap(),
    };

    // Write durable identity with the canonical session as the
    // workspace key (the unified contract).
    record_split_launch_metadata(
        &runtime,
        &store,
        "wh-srv",
        canonical_session,
        &unit,
        &launch,
    );

    // Re-read everything the group has, then filter by the same
    // canonical session — exactly mirroring `pane list`'s read
    // path in `crates/workhive-cli/src/commands/pane.rs`.
    let all_entries = store.list_for_group("wh-srv").expect("list_for_group");
    let identity_entries: Vec<PaneLaunchIdentityEntry> = all_entries
        .into_iter()
        .filter(|entry| entry.workspace == canonical_session)
        .collect();

    assert_eq!(
        identity_entries.len(),
        1,
        "writer/reader workspace keys must match: \
             the entry written under {canonical_session} \
             must be visible under {canonical_session}"
    );
    assert_eq!(identity_entries[0].unit_id.as_str(), "%9");
    assert!(matches!(
        &identity_entries[0].launch_origin,
        PaneLaunchOrigin::Tool { name } if name.as_str() == "codex"
    ));

    // Critically: filtering by the *legacy* workspace suffix
    // ("main") must yield NOTHING — this is the exact regression
    // we are guarding against.
    let suffix_view: Vec<PaneLaunchIdentityEntry> = store
        .list_for_group("wh-srv")
        .expect("list_for_group")
        .into_iter()
        .filter(|entry| entry.workspace == "main")
        .collect();
    assert!(
        suffix_view.is_empty(),
        "workspace suffix (\"main\") must no longer be the storage key; \
             only the canonical session ({canonical_session}) is durable"
    );

    // Round-trip through the actual pane-list view-model so the
    // unit-id keyed identity_map lookup is exercised end-to-end.
    let snapshot = RuntimeUnitSnapshot {
        id: unit.clone(),
        index: 0,
        kind: PaneKind::Terminal,
        state: PaneState::Running,
        command: "zsh".to_owned(),
        cwd: Utf8PathBuf::from("/tmp"),
        active: false,
        tty: None,
        process_pid: None,
    };
    let truth = PaneRuntimeTruth::from_layered(
        PaneKind::Terminal,
        PaneState::Running,
        "zsh".to_owned(),
        PaneObservationSource::TmuxCurrentCommand,
        PaneObservationConfidence::Fallback,
    );
    let inputs = vec![PaneObservedViewInput::new(snapshot, truth)];
    let rows = build_pane_list_view_from_inputs(&inputs, &identity_entries);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].launch_identity_title.as_deref(),
        Some("codex"),
        "with matching workspace keys, the durable identity title \
             must hydrate the row immediately"
    );
}

#[test]
fn record_split_launch_metadata_is_fail_open_on_title_error() {
    // Runtime title update fails — durable identity must still
    // be recorded so the next `pane list` can hydrate the label
    // even without an immediately reasserted title.
    let runtime = InlineRuntime::failing();
    let store = InlineIdentityStore::new();
    let unit = UnitId::new("%8");
    let launch = PaneLaunchRef::Tool {
        name: ToolName::new("codex").unwrap(),
    };
    record_split_launch_metadata(&runtime, &store, "wh-srv", "alpha", &unit, &launch);

    assert_eq!(store.recorded().len(), 1);
    assert!(
        runtime.title_updates().is_empty(),
        "title update was attempted but failed; nothing should be recorded"
    );
}

//! Behavioral matrix tests for `restore_runtime_pane`.

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    CommandSpec, PaneDirection, PaneLaunchIntent, PaneLaunchRef, ToolCatalog, ToolName,
};
#[allow(unused_imports)]
use crate::ports::{PaneRuntimePort, RuntimeError};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::ports::{AttachWorkspaceRequest, SendInputRequest, WorkspaceRuntimeInfo};

#[test]
fn restore_title_prefers_configured_tool_identity_over_snapshot_title() {
    let launch = PaneLaunchIntent::ToolRef {
        tool_name: "codex".to_owned(),
    };
    let title = preferred_restore_title(Some("workhive"), &launch);
    assert_eq!(
        title.as_deref(),
        Some("codex"),
        "config identity must override snapshot-captured runtime title"
    );
    assert!(
        !title.as_deref().unwrap_or("").contains("#{"),
        "Workhive-owned title must never contain tmux format variables"
    );
}

#[test]
fn restore_title_keeps_launch_display_for_non_restorable_pane() {
    let launch = PaneLaunchIntent::NonRestorable {
        display: "claude (degraded)".to_owned(),
        reason: "missing".to_owned(),
    };
    let title = preferred_restore_title(Some("runtime-title"), &launch);
    assert_eq!(title.as_deref(), Some("claude (degraded)"));
}

// ── Keystroke-free cwd repair for reused panes ──
//
// Existing-pane CWD correction must go through the infra-only respawn
// path. The unit tests below exercise `restore_runtime_pane` with both
// repair policies and assert two invariants:
//
// 1. `Skip` → no `respawn_unit_with_cwd` call (newly created panes start
//    in the right cwd through `create_window` / `split_pane`).
// 2. `Repair` with mismatched observed cwd → exactly one
//    `respawn_unit_with_cwd(unit, cwd)` call BEFORE any `send_input`.
//
// Neither path is allowed to issue a visible `cd -- ...` keystroke; the
// stub `send_input` channel records the verbatim input text and the
// assertions check it never starts with `cd `.

#[derive(Default)]
struct CwdRepairStub {
    respawn_calls: std::cell::RefCell<Vec<(String, camino::Utf8PathBuf)>>,
    respawn_command_calls: std::cell::RefCell<Vec<(String, camino::Utf8PathBuf, String)>>,
    send_input_calls: std::cell::RefCell<Vec<(String, String)>>,
    title_calls: std::cell::RefCell<Vec<(String, String)>>,
}

impl PaneRuntimePort for CwdRepairStub {
    fn spawn_unit(
        &self,
        _request: crate::ports::SpawnUnitRequest,
    ) -> Result<crate::ports::RuntimeUnitSnapshot, RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn close_unit(
        &self,
        _group: &str,
        _unit_id: &crate::domain::UnitId,
    ) -> Result<(), RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn list_units(
        &self,
        _group: &str,
        _workspace_name: &str,
    ) -> Result<Vec<crate::ports::RuntimeUnitSnapshot>, RuntimeError> {
        Ok(Vec::new())
    }
    fn current_unit(
        &self,
        _group: &str,
    ) -> Result<crate::ports::RuntimeUnitSnapshot, RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        self.send_input_calls
            .borrow_mut()
            .push((request.target_unit.as_str().to_owned(), request.text));
        Ok(())
    }
    fn set_unit_title(
        &self,
        request: crate::ports::SetUnitTitleRequest,
    ) -> Result<(), RuntimeError> {
        self.title_calls
            .borrow_mut()
            .push((request.target_unit.as_str().to_owned(), request.title));
        Ok(())
    }
}

impl crate::ports::SnapshotRuntimePort for CwdRepairStub {
    fn capture_topology(
        &self,
        _group: &str,
        _workspace_name: &str,
    ) -> Result<crate::ports::RuntimeTopologySnapshot, RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn create_window(
        &self,
        _group: &str,
        _workspace_name: &str,
        _window_index: u32,
        _name: Option<&str>,
        _cwd: &camino::Utf8Path,
    ) -> Result<crate::ports::RuntimePaneSnapshot, RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn split_pane(
        &self,
        _group: &str,
        _workspace_name: &str,
        _target_unit: &crate::domain::UnitId,
        _direction: PaneDirection,
        _prep: crate::ports::SplitPanePreparation,
    ) -> Result<crate::ports::RuntimePaneSnapshot, RuntimeError> {
        unreachable!("not used by restore tests")
    }
    fn respawn_unit_with_cwd(
        &self,
        _group: &str,
        _workspace_name: &str,
        unit_id: &crate::domain::UnitId,
        cwd: &camino::Utf8Path,
    ) -> Result<(), RuntimeError> {
        self.respawn_calls
            .borrow_mut()
            .push((unit_id.as_str().to_owned(), cwd.to_path_buf()));
        Ok(())
    }
    fn respawn_unit_with_command(
        &self,
        _group: &str,
        _workspace_name: &str,
        unit_id: &crate::domain::UnitId,
        cwd: &camino::Utf8Path,
        command: &str,
    ) -> Result<(), RuntimeError> {
        self.respawn_command_calls.borrow_mut().push((
            unit_id.as_str().to_owned(),
            cwd.to_path_buf(),
            command.to_owned(),
        ));
        Ok(())
    }
    fn apply_window_layout(
        &self,
        _group: &str,
        _workspace_name: &str,
        _window_index: u32,
        _layout: &str,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn select_window(
        &self,
        _group: &str,
        _workspace_name: &str,
        _window_index: u32,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn select_pane(
        &self,
        _group: &str,
        _unit_id: &crate::domain::UnitId,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn configure_snapshot_hooks(
        &self,
        _group: &str,
        _workspace_name: &str,
        _mode: crate::ports::SnapshotHookMode,
        _notify_command_prefix: &str,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
}

struct DummyExecChecker;
impl crate::ports::ExecutableAvailabilityPort for DummyExecChecker {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

#[test]
fn restore_runtime_pane_skip_does_not_respawn_or_emit_visible_cd() {
    // Newly created panes pass `Skip` because the adapter created them
    // with `create_window(..., cwd)` / `split_pane(..., prep.cwd)`.
    let stub = CwdRepairStub::default();
    let unit = crate::domain::UnitId::new("%1");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::EmptyShell,
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: None,
    })
    .expect("Skip-policy restore must succeed");

    assert!(
        stub.respawn_calls.borrow().is_empty(),
        "Skip policy MUST NOT call respawn_unit_with_cwd"
    );
    for (_, text) in stub.send_input_calls.borrow().iter() {
        assert!(
            !text.trim_start().starts_with("cd "),
            "restore must not emit visible `cd ` keystroke; saw {text:?}"
        );
    }
}

#[test]
fn restore_runtime_pane_repair_calls_respawn_when_cwd_differs() {
    // Reused panes carrying a user inline command preserve original
    // pane lifecycle: the cwd is realigned via the keystroke-free
    // `respawn_unit_with_cwd` first, then the inline command is
    // typed via `send_input` so the pane shell remains alive after
    // the command exits.
    let stub = CwdRepairStub::default();
    let unit = crate::domain::UnitId::new("%7");
    let captured_cwd = camino::Utf8PathBuf::from("/tmp/restore/expected");
    let observed = camino::Utf8PathBuf::from("/tmp/restore/wrong");
    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &captured_cwd,
        launch_intent: &PaneLaunchIntent::InlineCommand {
            program: "less".to_owned(),
            args: vec!["README.md".to_owned()],
        },
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Repair {
            session: "wsdemo",
            observed_cwd: Some(observed.as_path()),
        },
        instrumentation: None,
    })
    .expect("Repair-policy restore must succeed");

    // Inline commands MUST NOT use the supervisor-only typed launch
    // verb — respawn-pane -k would replace the pane's primary process
    // and close it on exit.
    assert!(
        stub.respawn_command_calls.borrow().is_empty(),
        "inline command must NOT be installed via respawn_unit_with_command; \
         saw {:?}",
        stub.respawn_command_calls.borrow()
    );
    let repair_calls = stub.respawn_calls.borrow().clone();
    assert_eq!(
        repair_calls.len(),
        1,
        "reused pane with cwd drift must realign via respawn_unit_with_cwd"
    );
    assert_eq!(repair_calls[0].0, "%7");
    assert_eq!(&repair_calls[0].1, &captured_cwd);
    let send_calls = stub.send_input_calls.borrow().clone();
    assert_eq!(send_calls.len(), 1, "inline command must be typed once");
    assert_eq!(send_calls[0].0, "%7");
    assert!(
        send_calls[0].1.contains("less"),
        "send_input payload must echo the user's inline command, got {:?}",
        send_calls[0].1
    );
    assert!(
        !send_calls[0].1.trim_start().starts_with("cd "),
        "restore must not emit visible `cd ` keystroke; saw {:?}",
        send_calls[0].1
    );
}

#[test]
fn restore_runtime_pane_repair_skips_respawn_when_cwd_matches() {
    // If the live pane already lives in the captured cwd, respawn would
    // be wasted process churn. The use case must compare and skip.
    let stub = CwdRepairStub::default();
    let unit = crate::domain::UnitId::new("%3");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/match");
    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 1,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::EmptyShell,
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Repair {
            session: "wsdemo",
            observed_cwd: Some(cwd.as_path()),
        },
        instrumentation: None,
    })
    .expect("Repair-policy restore must succeed");

    assert!(
        stub.respawn_calls.borrow().is_empty(),
        "Repair policy must skip respawn when observed cwd matches captured"
    );
    assert!(
        stub.respawn_command_calls.borrow().is_empty(),
        "EmptyShell restore must not invoke the typed launch verb"
    );
}

/// Capturing fake for `ToolLaunchInstrumentationPort` used by the
/// in-module restore tests. Returns a deterministic supervisor-style
/// command string and records every wrap request.
#[derive(Default)]
struct CapturingInstrumentation {
    captured: std::cell::RefCell<Vec<crate::ports::WrapToolCommandRequest>>,
    force_error: std::cell::RefCell<Option<crate::ports::ToolLaunchInstrumentationError>>,
}

impl crate::ports::ToolLaunchInstrumentationPort for CapturingInstrumentation {
    fn wrap_tool_command(
        &self,
        request: crate::ports::WrapToolCommandRequest,
    ) -> Result<String, crate::ports::ToolLaunchInstrumentationError> {
        if let Some(err) = self.force_error.borrow_mut().take() {
            return Err(err);
        }
        let unit = request.unit_id.as_str().to_owned();
        let tool = request.tool_name.clone().unwrap_or_default();
        let program = request.program.clone();
        self.captured.borrow_mut().push(request);
        Ok(format!(
            "/usr/local/bin/workhive internal pane-supervisor --unit {unit} --tool {tool} -- {program}"
        ))
    }
}

fn test_tool_catalog_with(name: &str, program: &str, args: &[&str]) -> ToolCatalog {
    let mut catalog = ToolCatalog::empty();
    let entry = crate::domain::ToolEntry::new(
        crate::domain::ToolName::new(name.to_owned()).expect("tool name"),
        "test entry",
        crate::domain::CommandSpec::new(program, args.iter().copied()),
        crate::domain::ToolKind::Cli,
    );
    catalog.add(entry).expect("add tool");
    catalog
}

/// P1 regression: the supervisor wrapper MUST be installed via the
/// snapshot runtime's typed launch verb (`respawn_unit_with_command`),
/// never typed into the pane shell. If the wrapper text reaches
/// `send_input`, it appears at the top of the pane's scrollback —
/// the visible-wrapper regression this test prevents.
#[test]
fn restore_runtime_pane_installs_tool_wrapper_via_typed_launch_not_send_input() {
    let stub = CwdRepairStub::default();
    let instrumentation = CapturingInstrumentation::default();
    let unit = crate::domain::UnitId::new("%42");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    let catalog = test_tool_catalog_with("less", "less", &["README.md"]);

    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::ToolRef {
            tool_name: "less".to_owned(),
        },
        title: None,
        catalog: &catalog,
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect("typed-launch restore must succeed");

    let respawn_command_calls = stub.respawn_command_calls.borrow();
    assert_eq!(
        respawn_command_calls.len(),
        1,
        "tool restore must install wrapper via respawn_unit_with_command exactly once",
    );
    let (recorded_unit, recorded_cwd, recorded_command) = &respawn_command_calls[0];
    assert_eq!(recorded_unit, "%42");
    assert_eq!(recorded_cwd, &cwd);
    assert!(
        recorded_command.contains("internal pane-supervisor"),
        "respawn command must carry the supervisor wrapper, got {recorded_command:?}"
    );

    assert!(
        stub.send_input_calls.borrow().is_empty(),
        "tool restore MUST NOT send any visible keystroke to the pane; \
         saw {:?}",
        stub.send_input_calls.borrow()
    );
}

/// Regression guard: user inline/raw commands MUST
/// NOT be silently re-routed through the typed `respawn_unit_with_command`
/// verb. Doing so would change pane lifecycle — `respawn-pane -k`
/// makes the inline command the pane's primary process, and the
/// pane closes on exit instead of returning to a shell. The bug
/// report only required hiding the internal supervisor wrapper, not
/// changing user-visible inline command semantics.
#[test]
fn restore_runtime_pane_installs_inline_command_via_send_input_preserving_user_visibility() {
    let stub = CwdRepairStub::default();
    let unit = crate::domain::UnitId::new("%43");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");

    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::InlineCommand {
            program: "less".to_owned(),
            args: vec!["README.md".to_owned()],
        },
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: None,
    })
    .expect("inline command restore must succeed");

    // Inline command lifecycle MUST be preserved: typed into the
    // pane shell, no respawn-pane primary-process replacement.
    assert!(
        stub.respawn_command_calls.borrow().is_empty(),
        "inline command MUST NOT install via respawn_unit_with_command; \
         saw {:?}",
        stub.respawn_command_calls.borrow()
    );
    let send_calls = stub.send_input_calls.borrow().clone();
    assert_eq!(
        send_calls.len(),
        1,
        "inline command must use send_input once"
    );
    assert_eq!(send_calls[0].0, "%43");
    assert!(
        send_calls[0].1.contains("less"),
        "send_input payload must carry the inline command, got {:?}",
        send_calls[0].1
    );
}

/// A quote-split known-tool inline command
/// (`InlineCommand { program: "\"cursor-agent", args: ["--yolo\""] }`)
/// resolves to the typed tool launch path on restore: it runs wrapped
/// through the supervisor via `respawn_unit_with_command` and is not
/// replayed as raw `send_input` (which would make the shell look for
/// an executable named `"cursor-agent`).
#[test]
fn restore_runtime_pane_heals_quote_polluted_tool_inline_command_via_typed_launch() {
    let stub = CwdRepairStub::default();
    let instrumentation = CapturingInstrumentation::default();
    let unit = crate::domain::UnitId::new("%77");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    let catalog = test_tool_catalog_with("cursor-agent", "cursor-agent", &["--yolo"]);

    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::InlineCommand {
            program: "\"cursor-agent".to_owned(),
            args: vec!["--yolo\"".to_owned()],
        },
        title: None,
        catalog: &catalog,
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect("healed tool restore must succeed");

    let captured = instrumentation.captured.borrow();
    assert_eq!(captured.len(), 1, "healed tool must be wrapped once");
    assert_eq!(captured[0].tool_name.as_deref(), Some("cursor-agent"));

    let cmd_calls = stub.respawn_command_calls.borrow();
    assert_eq!(
        cmd_calls.len(),
        1,
        "healed tool must install via respawn_unit_with_command"
    );
    assert!(
        cmd_calls[0].2.contains("internal pane-supervisor"),
        "respawn command must carry the supervisor wrapper, got {:?}",
        cmd_calls[0].2
    );
    assert!(
        stub.send_input_calls.borrow().is_empty(),
        "quote-polluted tool MUST NOT be replayed via send_input; saw {:?}",
        stub.send_input_calls.borrow()
    );
}

/// Restore of a tool pane must wrap the command via the instrumentation
/// port and forward the wrapped command to the typed launch verb
/// (`respawn_unit_with_command`), keeping `send_input` empty.
#[test]
fn restore_runtime_pane_wraps_tool_command_via_instrumentation_when_available() {
    let stub = CwdRepairStub::default();
    let instrumentation = CapturingInstrumentation::default();
    let unit = crate::domain::UnitId::new("%9");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    let catalog = test_tool_catalog_with("less", "less", &["README.md"]);

    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::ToolRef {
            tool_name: "less".to_owned(),
        },
        title: None,
        catalog: &catalog,
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect("instrumented restore must succeed");

    let captured = instrumentation.captured.borrow();
    assert_eq!(captured.len(), 1, "instrumentation must be invoked once");
    let req = &captured[0];
    assert_eq!(req.unit_id, unit);
    assert_eq!(req.tool_name.as_deref(), Some("less"));
    assert_eq!(req.program, "less");
    assert_eq!(req.args, vec!["README.md".to_owned()]);

    let cmd_calls = stub.respawn_command_calls.borrow();
    assert_eq!(
        cmd_calls.len(),
        1,
        "tool restore must install wrapper via respawn_unit_with_command"
    );
    assert_eq!(cmd_calls[0].0, "%9");
    assert!(
        cmd_calls[0].2.contains("internal pane-supervisor"),
        "respawn command must carry the supervisor wrapper, got {:?}",
        cmd_calls[0].2
    );
    assert!(
        stub.send_input_calls.borrow().is_empty(),
        "tool restore MUST NOT use send_input"
    );
}

/// Restore of a `ToolRef` pane must fail closed when instrumentation
/// returns an error: the use case must surface the error and MUST
/// NOT call `send_input`. Silently sending the unwrapped tool would
/// break the supervisor observation contract — `Source::Supervisor`
/// would never appear for that pane and `pane list` would lose
/// authoritative truth.
#[test]
fn restore_runtime_pane_fails_closed_when_tool_instrumentation_errors() {
    let stub = CwdRepairStub::default();
    let instrumentation = CapturingInstrumentation::default();
    instrumentation.force_error.replace(Some(
        crate::ports::ToolLaunchInstrumentationError::SupervisorBinaryUnavailable {
            detail: "test failure".to_owned(),
        },
    ));
    let unit = crate::domain::UnitId::new("%10");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    let catalog = test_tool_catalog_with("less", "less", &["README.md"]);

    let err = restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::ToolRef {
            tool_name: "less".to_owned(),
        },
        title: None,
        catalog: &catalog,
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect_err("restore must fail closed when instrumentation fails for a tool");
    assert!(
        err.contains("less") && err.contains("instrumentation"),
        "error must mention the tool and the instrumentation failure: {err}"
    );

    assert!(
        stub.send_input_calls.borrow().is_empty(),
        "fail-closed restore must NOT send any input to the pane"
    );
}

/// Restore of a `ToolRef` pane MUST also fail closed when no
/// instrumentation port is wired in. A `Tool` launch without a
/// supervisor would silently lose the highest-confidence
/// observation source, so the use case rejects it. Inline / shell
/// launches are handled by a separate test below.
#[test]
fn restore_runtime_pane_fails_closed_when_tool_lacks_instrumentation_port() {
    let stub = CwdRepairStub::default();
    let unit = crate::domain::UnitId::new("%10b");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");
    let catalog = test_tool_catalog_with("less", "less", &["README.md"]);

    let err = restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::ToolRef {
            tool_name: "less".to_owned(),
        },
        title: None,
        catalog: &catalog,
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: None,
    })
    .expect_err("restore must fail closed when no instrumentation is provided for a tool");
    assert!(
        err.contains("instrumentation"),
        "error must mention instrumentation availability: {err}"
    );

    assert!(
        stub.send_input_calls.borrow().is_empty(),
        "fail-closed restore must NOT send any input to the pane"
    );
}

/// Inline command and empty-shell restore intents bypass instrumentation
/// because they are not first-class catalog tools — wrapping them would
/// label the supervisor metadata with no tool name.
#[test]
fn restore_runtime_pane_does_not_wrap_inline_commands_or_empty_shell() {
    let stub = CwdRepairStub::default();
    let instrumentation = CapturingInstrumentation::default();
    let unit = crate::domain::UnitId::new("%11");
    let cwd = camino::Utf8PathBuf::from("/tmp/restore/proj");

    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: unit.clone(),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::InlineCommand {
            program: "less".to_owned(),
            args: vec!["README.md".to_owned()],
        },
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect("inline command restore must succeed");

    assert!(
        instrumentation.captured.borrow().is_empty(),
        "inline command restore must not invoke instrumentation"
    );

    // Now empty shell: no command sent, no instrumentation called.
    restore_runtime_pane(RestoreRuntimePaneRequest {
        pane_runtime: &stub,
        snapshot_runtime: &stub,
        server: "whsrv-test",
        session: "wsdemo",
        unit_id: crate::domain::UnitId::new("%12"),
        pane_index: 0,
        cwd: &cwd,
        launch_intent: &PaneLaunchIntent::EmptyShell,
        title: None,
        catalog: &ToolCatalog::empty(),
        exec_checker: &DummyExecChecker,
        cwd_repair: ReusedPaneCwdRepair::Skip,
        instrumentation: Some(&instrumentation),
    })
    .expect("empty-shell restore must succeed");

    assert!(
        instrumentation.captured.borrow().is_empty(),
        "empty-shell restore must not invoke instrumentation"
    );
}

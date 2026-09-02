//! Integration tests for the pane runtime control plane transport.
//!
//! These tests drive the real Unix-socket supervisor and client together,
//! using `workhive-testkit`'s `FakeRuntime` as the backing `PaneRuntimePort`.
//! No actual tmux/git processes are started, so the tests are hermetic and
//! fast enough for the default `cargo test --workspace` gate.
//!
//! Coverage:
//!
//! - The supervisor must invoke `PaneRuntimePort::send_input`
//!   with `enter = true` when the CLI asks it to submit, and with `enter =
//!   false` when only literal text is requested. The text must pass
//!   through unchanged (no `\n`/`\r`/`C-m` concatenation).
//! - The client ↔ supervisor protocol supports `Ping`,
//!   version negotiation, and `SendInput` round-trips across a real UDS.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use camino::Utf8PathBuf;
use workhive_core::control_plane::{PaneControlPlaneClient, SendPaneInputOutcome};
use workhive_core::domain::{
    CommandSpec, ObservedPaneState, PaneCapability, PaneDescriptor, PaneEvent, PaneKind,
    PaneLaunchOrigin, PaneLifecycleEvent, PaneRole, PaneRuntimeId, PaneRuntimeStatus, PaneState,
    PaneStateChange, ToolName, UnitId,
};
use workhive_core::ports::{
    PaneLaunchIdentityEntry, PaneLaunchIdentityPort, PaneRegistryPort, PaneRuntimePort,
    RegistryDelta, RegistryError, RuntimeError, RuntimeUnitSnapshot, SendInputRequest,
    SetUnitTitleRequest, SpawnUnitRequest,
};
use workhive_infra::control_plane::{
    ControlPlaneSupervisor, LocalPaneControlPlaneClient, control_plane_socket_path,
    prepare_socket_parent, remove_stale_socket,
};
use workhive_infra::pane_launch_identity::{
    JsonPaneLaunchIdentityStore, hydrate_pane_registry_from_identity_store,
};
use workhive_infra::pane_registry::InMemoryPaneRegistry;

/// Minimal, thread-safe `PaneRuntimePort` used to drive the supervisor from
/// a background thread. The workhive-testkit `FakeRuntime` stores state in
/// `RefCell` which is `!Sync`; this local adapter uses `Mutex` instead so
/// the test can keep both the supervisor thread and the main-thread client
/// observing it concurrently.
struct ThreadSafeRuntime {
    inner: Mutex<ThreadSafeRuntimeState>,
}

#[derive(Default)]
struct ThreadSafeRuntimeState {
    send_input_calls: Vec<SendInputRequest>,
    units: Vec<RuntimeUnitSnapshot>,
}

impl ThreadSafeRuntime {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ThreadSafeRuntimeState::default()),
        }
    }

    fn send_input_calls(&self) -> Vec<SendInputRequest> {
        self.inner.lock().expect("mutex").send_input_calls.clone()
    }

    fn set_units(&self, units: Vec<RuntimeUnitSnapshot>) {
        self.inner.lock().expect("mutex").units = units;
    }
}

impl PaneRuntimePort for ThreadSafeRuntime {
    fn spawn_unit(&self, _request: SpawnUnitRequest) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "spawn_unit not exercised by control-plane tests".to_owned(),
        })
    }

    fn close_unit(&self, _group: &str, _unit_id: &UnitId) -> Result<(), RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "close_unit not exercised by control-plane tests".to_owned(),
        })
    }

    fn list_units(
        &self,
        _group: &str,
        _workspace_name: &str,
    ) -> Result<Vec<RuntimeUnitSnapshot>, RuntimeError> {
        Ok(self.inner.lock().expect("mutex").units.clone())
    }

    fn current_unit(&self, _group: &str) -> Result<RuntimeUnitSnapshot, RuntimeError> {
        Err(RuntimeError::OperationFailed {
            message: "current_unit not exercised by control-plane tests".to_owned(),
        })
    }

    fn send_input(&self, request: SendInputRequest) -> Result<(), RuntimeError> {
        self.inner
            .lock()
            .expect("mutex")
            .send_input_calls
            .push(request);
        Ok(())
    }

    fn set_unit_title(&self, _request: SetUnitTitleRequest) -> Result<(), RuntimeError> {
        Ok(())
    }
}

/// Minimal thread-safe `PaneRegistryPort` for the control-plane e2e tests.
/// The infra `InMemoryPaneRegistry` uses `RefCell`, which is `!Sync`, so it
/// cannot be shared with the supervisor thread spawned by the harness.
///
/// This adapter only implements the methods exercised by the supervisor's
/// `dispatch_query_pane` and `dispatch_send_input` paths
/// (`upsert`, `get`, `update_from_observation`); the rest fall back to a
/// minimal stub. It is **not** a general-purpose registry.
struct ThreadSafeRegistry {
    inner: Mutex<Vec<PaneDescriptor>>,
}

impl ThreadSafeRegistry {
    fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }
}

impl PaneRegistryPort for ThreadSafeRegistry {
    fn upsert(&self, descriptor: PaneDescriptor) -> Result<RegistryDelta, RegistryError> {
        let mut panes = self.inner.lock().expect("registry mutex");
        if let Some(slot) = panes
            .iter_mut()
            .find(|p| p.runtime_id == descriptor.runtime_id)
        {
            *slot = descriptor;
        } else {
            panes.push(descriptor);
        }
        Ok(RegistryDelta {
            changes: Vec::new(),
        })
    }

    fn remove(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
        let mut panes = self.inner.lock().expect("registry mutex");
        let pos = panes.iter().position(|p| p.runtime_id == *id);
        Ok(pos.map(|idx| panes.remove(idx)))
    }

    fn get(&self, id: &PaneRuntimeId) -> Result<Option<PaneDescriptor>, RegistryError> {
        Ok(self
            .inner
            .lock()
            .expect("registry mutex")
            .iter()
            .find(|p| p.runtime_id == *id)
            .cloned())
    }

    fn list_workspace(&self, workspace: &str) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(self
            .inner
            .lock()
            .expect("registry mutex")
            .iter()
            .filter(|p| p.workspace == workspace)
            .cloned()
            .collect())
    }

    fn list(&self, workspace: Option<&str>) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(self
            .inner
            .lock()
            .expect("registry mutex")
            .iter()
            .filter(|p| workspace.is_none_or(|w| p.workspace == w))
            .cloned()
            .collect())
    }

    fn drain_events(&self) -> Result<Vec<PaneLifecycleEvent>, RegistryError> {
        Ok(Vec::new())
    }

    fn update_from_observation(
        &self,
        observed: ObservedPaneState,
    ) -> Result<Vec<PaneStateChange>, RegistryError> {
        let mut panes = self.inner.lock().expect("registry mutex");
        let runtime_id = PaneRuntimeId::derive(
            &observed.group,
            &observed.workspace,
            observed.unit_id.as_str(),
        );
        if let Some(slot) = panes.iter_mut().find(|p| p.runtime_id == runtime_id) {
            slot.cwd = observed.cwd;
            slot.command = observed.command;
            slot.kind = observed.kind;
            slot.state = observed.state;
        }
        Ok(Vec::new())
    }

    fn find_by_capability(
        &self,
        _capability: PaneCapability,
    ) -> Result<Vec<PaneDescriptor>, RegistryError> {
        Ok(Vec::new())
    }
}

// Suppress dead-code warnings for `PaneEvent`: imported for trait
// completeness above but not used in current assertions.
#[allow(dead_code)]
const _PANE_EVENT_USED: Option<PaneEvent> = None;

/// Helper that spawns the supervisor on a background thread and hands back
/// the socket path plus a stop flag. The test is responsible for calling
/// the returned `stop` closure and joining the background handle so the
/// tempdir clean-up runs after the listener has exited.
struct SupervisorHarness {
    socket: Utf8PathBuf,
    stop: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
    _tempdir: tempfile::TempDir,
}

impl SupervisorHarness {
    fn launch(runtime: Arc<ThreadSafeRuntime>, group: &str) -> Self {
        Self::launch_with_workspace(runtime, group, "default")
    }

    fn launch_with_workspace(
        runtime: Arc<ThreadSafeRuntime>,
        group: &str,
        workspace: &str,
    ) -> Self {
        Self::launch_with_options(runtime, group, workspace, None)
    }

    /// Launch with an optional registry pre-populated by the caller. The
    /// supervisor uses [`ControlPlaneSupervisor::with_registry`] so
    /// `QueryPane` can source `display_title` from the authoritative
    /// `PaneLaunchOrigin`.
    fn launch_with_registry(
        runtime: Arc<ThreadSafeRuntime>,
        group: &str,
        workspace: &str,
        registry: Arc<ThreadSafeRegistry>,
    ) -> Self {
        Self::launch_with_options(runtime, group, workspace, Some(registry))
    }

    fn launch_with_options(
        runtime: Arc<ThreadSafeRuntime>,
        group: &str,
        workspace: &str,
        registry: Option<Arc<ThreadSafeRegistry>>,
    ) -> Self {
        // macOS `sockaddr_un::sun_path` caps at 104 bytes, which the default
        // `TempDir` location (e.g. `/var/folders/.../T/...`) can already
        // consume most of once we nest `.workhive/runtime/control-plane/...`
        // inside it. Anchor the tempdir under `/tmp` to keep the final
        // socket path comfortably under that limit.
        let tempdir = tempfile::Builder::new()
            .prefix("whcp-")
            .tempdir_in("/tmp")
            .expect("create tempdir under /tmp");
        let project_root =
            Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf-8 tempdir path");
        let socket = control_plane_socket_path(&project_root, group);
        prepare_socket_parent(&socket).expect("prepare socket parent");
        remove_stale_socket(&socket).expect("no stale socket");

        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_server = Arc::clone(&stop);
        let socket_for_server = socket.clone();
        let group = group.to_owned();
        let workspace = workspace.to_owned();
        let join = thread::spawn(move || {
            let supervisor = match registry.as_deref() {
                Some(registry) => ControlPlaneSupervisor::with_registry(
                    runtime.as_ref(),
                    registry,
                    group,
                    workspace,
                ),
                None => ControlPlaneSupervisor::new(runtime.as_ref(), group, workspace),
            };
            if let Err(e) = supervisor.serve_blocking(&socket_for_server, || {
                stop_for_server.load(Ordering::SeqCst)
            }) {
                eprintln!("supervisor exited with error: {e}");
            }
        });

        // Wait until the supervisor is *actually* accepting connections,
        // not merely until the socket file appears. Under parallel
        // `cargo test` execution we observed transient `ECONNREFUSED`
        // when the file is present but the kernel listen backlog has
        // not yet absorbed the first connect. Probing with a real
        // connect closes that race deterministically.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if socket.as_std_path().exists()
                && std::os::unix::net::UnixStream::connect(socket.as_std_path()).is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                panic!(
                    "supervisor did not bind+accept on socket within 5s: {}",
                    socket
                );
            }
            thread::sleep(Duration::from_millis(10));
        }

        Self {
            socket,
            stop,
            join: Some(join),
            _tempdir: tempdir,
        }
    }

    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A dummy connect wakes `listener.incoming()` so the loop observes
        // the stop flag and returns. We tolerate connection errors here —
        // the goal is strictly to unblock `accept`.
        let _ = std::os::unix::net::UnixStream::connect(self.socket.as_std_path());
        if let Some(handle) = self.join.take() {
            handle.join().expect("supervisor thread join");
        }
    }
}

#[test]
fn ping_round_trip_succeeds() {
    let runtime = Arc::new(ThreadSafeRuntime::new());
    let harness = SupervisorHarness::launch(Arc::clone(&runtime), "whsrv-ping");

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    client.ping().expect("ping must succeed");

    harness.shutdown();
}

#[test]
fn send_pane_input_delivers_enter_when_requested() {
    let runtime = Arc::new(ThreadSafeRuntime::new());
    // The supervisor validates pane existence via `list_units` before
    // delegating to the runtime, so the test must pre-seed the target
    // unit.
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new("%42"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "bash".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }]);
    let harness = SupervisorHarness::launch(Arc::clone(&runtime), "whsrv-send-enter");

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let outcome = client
        .send_pane_input(&UnitId::new("%42"), "hello world", true)
        .expect("send must succeed");

    assert_eq!(outcome, SendPaneInputOutcome::Delivered);
    let requests = runtime.send_input_calls();
    assert_eq!(
        requests.len(),
        1,
        "supervisor must issue exactly one runtime call"
    );
    assert_eq!(requests[0].target_unit.as_str(), "%42");
    assert_eq!(
        requests[0].text, "hello world",
        "literal text must not be mutated"
    );
    assert!(
        requests[0].enter,
        "control plane must forward submit=true as enter=true"
    );

    harness.shutdown();
}

#[test]
fn send_pane_input_suppresses_enter_when_no_enter_requested() {
    let runtime = Arc::new(ThreadSafeRuntime::new());
    // Pane existence is validated via `list_units` before delivery.
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new("%7"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "bash".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }]);
    let harness = SupervisorHarness::launch(Arc::clone(&runtime), "whsrv-send-no-enter");

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    client
        .send_pane_input(&UnitId::new("%7"), "/help", false)
        .expect("send must succeed");

    let requests = runtime.send_input_calls();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].text, "/help");
    assert!(
        !requests[0].enter,
        "--no-enter must NOT concatenate Enter into the runtime call"
    );

    harness.shutdown();
}

#[test]
fn send_pane_input_rejects_unknown_unit_with_typed_error() {
    // When the target unit is not in `list_units`, the supervisor MUST
    // return `UnknownPane` and MUST NOT call `runtime.send_input`.
    let runtime = Arc::new(ThreadSafeRuntime::new());
    // Intentionally leave the runtime with NO units listed.
    let harness = SupervisorHarness::launch(Arc::clone(&runtime), "whsrv-send-unknown");

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let err = client
        .send_pane_input(&UnitId::new("%nope"), "hello", true)
        .expect_err("send to unknown pane must fail");

    match err {
        workhive_core::control_plane::PaneControlPlaneError::UnknownPane { unit_id } => {
            assert_eq!(unit_id.as_str(), "%nope");
        }
        other => panic!("expected UnknownPane, got {other:?}"),
    }

    assert!(
        runtime.send_input_calls().is_empty(),
        "runtime.send_input MUST NOT be called for unknown panes"
    );

    harness.shutdown();
}

#[test]
fn query_pane_returns_real_descriptor_for_known_unit() {
    // QueryPane must surface the real (kind, status, cwd, command)
    // for a unit listed by the runtime.
    //
    // When no registry is wired,
    // `display_title` MUST be `None` (no Workhive-managed launch identity
    // available); the foreground command lives only in the dedicated
    // `command` field.
    let runtime = Arc::new(ThreadSafeRuntime::new());
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new("%99"),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp/some/where"),
        command: "claude".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Running,
        tty: None,
        process_pid: None,
    }]);
    let harness = SupervisorHarness::launch_with_workspace(
        Arc::clone(&runtime),
        "whsrv-query-known",
        "wsdemo",
    );

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let snapshot = client
        .query_pane(&UnitId::new("%99"))
        .expect("query must succeed");
    assert_eq!(snapshot.unit_id.as_str(), "%99");
    assert_eq!(snapshot.kind, PaneKind::Agent);
    assert_eq!(snapshot.cwd.as_deref(), Some("/tmp/some/where"));
    assert_eq!(snapshot.command.as_deref(), Some("claude"));
    assert_eq!(
        snapshot.display_title, None,
        "without a registry there is no authoritative launch title; the \
         foreground command must NOT be impersonated as a display title"
    );

    harness.shutdown();
}

/// Helper: build a `PaneDescriptor` for the given runtime id + launch origin.
fn descriptor_for(
    group: &str,
    workspace: &str,
    unit_id: &str,
    launch_origin: PaneLaunchOrigin,
) -> PaneDescriptor {
    let runtime_id = PaneRuntimeId::derive(group, workspace, unit_id);
    PaneDescriptor {
        runtime_id,
        group: group.to_owned(),
        workspace: workspace.to_owned(),
        unit_id: UnitId::new(unit_id),
        cwd: "/tmp".to_owned(),
        command: String::new(),
        kind: PaneKind::Agent,
        state: PaneState::Idle,
        observed_status: PaneRuntimeStatus::Idle,
        role: None as Option<PaneRole>,
        launch_origin,
        capabilities: std::collections::BTreeSet::<PaneCapability>::new(),
    }
}

#[test]
fn query_pane_display_title_comes_from_registry_launch_origin_for_tool() {
    // When the registry holds an
    // authoritative `Tool` launch origin, `display_title` MUST be the
    // tool name, NOT the foreground command observed by the runtime.
    //
    // Scenario: pane was launched as the `claude` tool but the user
    // dropped to a shell. `command` reflects reality (`bash`); the
    // Workhive title still says `claude`.
    let group = "whsrv-query-title-tool";
    let workspace = "wstool";
    let unit_id = "%111";

    let registry = Arc::new(ThreadSafeRegistry::new());
    registry
        .upsert(descriptor_for(
            group,
            workspace,
            unit_id,
            PaneLaunchOrigin::Tool {
                name: ToolName::new("claude").expect("tool name"),
            },
        ))
        .expect("seed registry");

    let runtime = Arc::new(ThreadSafeRuntime::new());
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new(unit_id),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp/work"),
        command: "bash".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Idle,
        tty: None,
        process_pid: None,
    }]);

    let harness = SupervisorHarness::launch_with_registry(
        Arc::clone(&runtime),
        group,
        workspace,
        Arc::clone(&registry),
    );

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let snapshot = client
        .query_pane(&UnitId::new(unit_id))
        .expect("query must succeed");

    assert_eq!(
        snapshot.display_title.as_deref(),
        Some("claude"),
        "display_title must come from the registry's PaneLaunchOrigin::Tool, \
         not the foreground command"
    );
    assert_eq!(
        snapshot.command.as_deref(),
        Some("bash"),
        "command field must surface the real foreground command"
    );

    harness.shutdown();
}

#[test]
fn query_pane_display_title_is_none_for_empty_shell_origin() {
    // `EmptyShell` is the
    // sentinel for "no Workhive title". Even when the foreground
    // command is something noisy (e.g. the user manually launched
    // `codex`), `display_title` must remain `None`. The command
    // belongs ONLY in the `command` field.
    let group = "whsrv-query-title-shell";
    let workspace = "wsempty";
    let unit_id = "%222";

    let registry = Arc::new(ThreadSafeRegistry::new());
    registry
        .upsert(descriptor_for(
            group,
            workspace,
            unit_id,
            PaneLaunchOrigin::EmptyShell,
        ))
        .expect("seed registry");

    let runtime = Arc::new(ThreadSafeRuntime::new());
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new(unit_id),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp"),
        command: "codex".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Running,
        tty: None,
        process_pid: None,
    }]);

    let harness = SupervisorHarness::launch_with_registry(
        Arc::clone(&runtime),
        group,
        workspace,
        Arc::clone(&registry),
    );

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let snapshot = client
        .query_pane(&UnitId::new(unit_id))
        .expect("query must succeed");

    assert_eq!(
        snapshot.display_title, None,
        "EmptyShell origin must NOT be turned into a display_title — the \
         foreground command must not be impersonated"
    );
    assert_eq!(snapshot.command.as_deref(), Some("codex"));

    harness.shutdown();
}

#[test]
fn query_pane_display_title_uses_inline_command_program_name() {
    // For `InlineCommand` launch
    // origins, the display_title MUST be the program name from the
    // launch identity, not the live foreground command.
    let group = "whsrv-query-title-inline";
    let workspace = "wsinline";
    let unit_id = "%333";

    let registry = Arc::new(ThreadSafeRegistry::new());
    let spec = CommandSpec::new("pytest", ["-x", "tests/"]);
    registry
        .upsert(descriptor_for(
            group,
            workspace,
            unit_id,
            PaneLaunchOrigin::InlineCommand { spec },
        ))
        .expect("seed registry");

    let runtime = Arc::new(ThreadSafeRuntime::new());
    runtime.set_units(vec![RuntimeUnitSnapshot {
        id: UnitId::new(unit_id),
        index: 0,
        cwd: Utf8PathBuf::from("/tmp/proj"),
        // Foreground command differs from the launch identity (e.g.
        // pytest spawned a python child). The display title must
        // still match the original launch program.
        command: "python".to_owned(),
        active: true,
        kind: PaneKind::Agent,
        state: PaneState::Running,
        tty: None,
        process_pid: None,
    }]);

    let harness = SupervisorHarness::launch_with_registry(
        Arc::clone(&runtime),
        group,
        workspace,
        Arc::clone(&registry),
    );

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let snapshot = client
        .query_pane(&UnitId::new(unit_id))
        .expect("query must succeed");

    assert_eq!(
        snapshot.display_title.as_deref(),
        Some("pytest"),
        "display_title must come from PaneLaunchOrigin::InlineCommand program name"
    );
    assert_eq!(snapshot.command.as_deref(), Some("python"));

    harness.shutdown();
}

#[test]
fn query_pane_returns_unknown_pane_for_missing_unit() {
    // Typed UnknownPane must surface to the client, not a
    // fabricated empty snapshot.
    let runtime = Arc::new(ThreadSafeRuntime::new());
    let harness = SupervisorHarness::launch_with_workspace(
        Arc::clone(&runtime),
        "whsrv-query-unknown",
        "wsmissing",
    );

    let client = LocalPaneControlPlaneClient::new(harness.socket.clone());
    let err = client
        .query_pane(&UnitId::new("%does-not-exist"))
        .expect_err("query for unknown unit must fail");
    match err {
        workhive_core::control_plane::PaneControlPlaneError::UnknownPane { unit_id } => {
            assert_eq!(unit_id.as_str(), "%does-not-exist");
        }
        other => panic!("expected UnknownPane, got {other:?}"),
    }

    harness.shutdown();
}

/// Simulates the production cross-process boundary:
///
/// 1. Phase A — `workspace open` (parent CLI process):
///    - Writes durable launch identity entries to the per-group JSON
///      file under `<project_root>/.workhive/runtime/pane-identity/`.
///    - In production these come from `baseline_pane_entries` after
///      the runtime returns the real `UnitId`s.
/// 2. Phase B — supervisor process (detached child):
///    - Builds a brand-new, empty `InMemoryPaneRegistry`.
///    - Hydrates from the durable store via
///      [`hydrate_pane_registry_from_identity_store`].
///    - Serves `QueryPane` requests with the runtime listing whatever
///      the foreground command happens to be (`shell` here, on
///      purpose).
/// 3. Phase C — assertion:
///    - `display_title` is the durable Workhive launch identity
///      (`codex` / `claude`) even though the live foreground command
///      is `shell`.
///    - `command` is the truthful foreground command (`shell`).
///
/// The test uses `InMemoryPaneRegistry` directly (the production
/// adapter), not the test-local `ThreadSafeRegistry`, because the
/// registry has been promoted to `Sync` (Mutex-backed) specifically
/// to make this cross-process replay viable.
#[test]
fn supervisor_hydrates_display_title_from_durable_store_across_process_boundary() {
    let group = "whsrv-cross-proc";
    let workspace = "ws-cross";
    let codex_unit = "%5";
    let claude_unit = "%6";
    let plain_unit = "%7";

    // ── Phase A: parent CLI process writes durable identity ──
    let tempdir = tempfile::Builder::new()
        .prefix("whcp-cross-")
        .tempdir_in("/tmp")
        .expect("tempdir");
    let project_root =
        Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf()).expect("utf-8 tempdir path");
    let store = JsonPaneLaunchIdentityStore::new(project_root.clone());

    store
        .record(
            group,
            PaneLaunchIdentityEntry {
                workspace: workspace.to_owned(),
                unit_id: UnitId::new(codex_unit),
                launch_origin: PaneLaunchOrigin::Tool {
                    name: ToolName::new("codex").expect("codex tool name"),
                },
            },
        )
        .expect("record codex");
    store
        .record(
            group,
            PaneLaunchIdentityEntry {
                workspace: workspace.to_owned(),
                unit_id: UnitId::new(claude_unit),
                launch_origin: PaneLaunchOrigin::Tool {
                    name: ToolName::new("claude").expect("claude tool name"),
                },
            },
        )
        .expect("record claude");
    // No durable identity recorded for plain_unit — that is the
    // "Workhive doesn't manage this pane" case, and `display_title`
    // must remain `None`.

    // ── Phase B: supervisor process boots fresh ──
    // Set up the runtime to list all three panes with the SAME
    // foreground command "shell". The display_title can ONLY come
    // from the durable store; the runtime cannot help.
    let runtime = Arc::new(ThreadSafeRuntime::new());
    runtime.set_units(vec![
        RuntimeUnitSnapshot {
            id: UnitId::new(codex_unit),
            index: 0,
            cwd: Utf8PathBuf::from("/tmp/work"),
            command: "shell".to_owned(),
            active: true,
            kind: PaneKind::Agent,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        },
        RuntimeUnitSnapshot {
            id: UnitId::new(claude_unit),
            index: 1,
            cwd: Utf8PathBuf::from("/tmp/work"),
            command: "shell".to_owned(),
            active: false,
            kind: PaneKind::Agent,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        },
        RuntimeUnitSnapshot {
            id: UnitId::new(plain_unit),
            index: 2,
            cwd: Utf8PathBuf::from("/tmp/work"),
            command: "shell".to_owned(),
            active: false,
            kind: PaneKind::Terminal,
            state: PaneState::Idle,
            tty: None,
            process_pid: None,
        },
    ]);

    // Build the supervisor's socket exactly the way the real
    // detached supervisor process does.
    let socket = control_plane_socket_path(&project_root, group);
    prepare_socket_parent(&socket).expect("prepare socket parent");
    remove_stale_socket(&socket).expect("no stale socket");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_server = Arc::clone(&stop);
    let socket_for_server = socket.clone();
    let group_owned = group.to_owned();
    let workspace_owned = workspace.to_owned();
    let project_root_for_server = project_root.clone();

    let join = thread::spawn(move || {
        // This closure simulates the detached supervisor process.
        // It does NOT receive the registry from the parent — it
        // builds its own and hydrates from the durable store.
        let store = JsonPaneLaunchIdentityStore::new(project_root_for_server);
        let registry = InMemoryPaneRegistry::new();

        let report = hydrate_pane_registry_from_identity_store(&store, &registry, &group_owned)
            .expect("hydration must succeed");
        assert_eq!(
            report.entries_loaded, 2,
            "exactly two durable entries (codex, claude) must be discovered"
        );
        assert_eq!(report.entries_applied, 2);

        let supervisor = ControlPlaneSupervisor::with_registry(
            runtime.as_ref(),
            &registry,
            group_owned,
            workspace_owned,
        );
        if let Err(e) = supervisor.serve_blocking(&socket_for_server, || {
            stop_for_server.load(Ordering::SeqCst)
        }) {
            eprintln!("cross-proc supervisor exited with error: {e}");
        }
    });

    // Wait for the supervisor to bind.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !socket.as_std_path().exists() {
        if Instant::now() >= deadline {
            panic!("cross-proc supervisor did not bind socket within 2s");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // ── Phase C: assertions ──
    let client = LocalPaneControlPlaneClient::new(socket.clone());

    let codex_snapshot = client
        .query_pane(&UnitId::new(codex_unit))
        .expect("query codex");
    assert_eq!(
        codex_snapshot.display_title.as_deref(),
        Some("codex"),
        "display_title MUST come from the durable launch identity store, \
         even when the live foreground command is `shell`. \
         got snapshot={codex_snapshot:?}"
    );
    assert_eq!(
        codex_snapshot.command.as_deref(),
        Some("shell"),
        "command field MUST surface the truthful foreground command"
    );

    let claude_snapshot = client
        .query_pane(&UnitId::new(claude_unit))
        .expect("query claude");
    assert_eq!(
        claude_snapshot.display_title.as_deref(),
        Some("claude"),
        "second durable entry must hydrate independently"
    );
    assert_eq!(claude_snapshot.command.as_deref(), Some("shell"));

    let plain_snapshot = client
        .query_pane(&UnitId::new(plain_unit))
        .expect("query plain");
    assert_eq!(
        plain_snapshot.display_title, None,
        "no durable identity recorded ⇒ no authoritative title; \
         the foreground command must NOT be impersonated as a title"
    );
    assert_eq!(plain_snapshot.command.as_deref(), Some("shell"));

    // ── Teardown ──
    stop.store(true, Ordering::SeqCst);
    let _ = std::os::unix::net::UnixStream::connect(socket.as_std_path());
    join.join().expect("supervisor thread join");
}

#[test]
fn unavailable_supervisor_reports_typed_error() {
    // Point the client at a socket that no one is serving so the CLI can
    // fall back to the direct PaneRuntimePort path. The error variant must
    // be `SupervisorUnavailable` so the dispatcher can distinguish "no
    // supervisor" from a genuine backend failure.
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let socket = Utf8PathBuf::from_path_buf(tempdir.path().join("missing.sock"))
        .expect("utf-8 tempdir path");
    let client = LocalPaneControlPlaneClient::new(socket);

    let err = client
        .send_pane_input(&UnitId::new("%1"), "noop", false)
        .expect_err("client must fail when nothing is listening");
    match err {
        workhive_core::control_plane::PaneControlPlaneError::SupervisorUnavailable { .. } => {}
        other => panic!("expected SupervisorUnavailable, got {other:?}"),
    }
}

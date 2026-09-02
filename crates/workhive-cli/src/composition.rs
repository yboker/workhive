//! Thin CLI composition root for real adapter construction.

use camino::Utf8Path;
use workhive_core::domain::{RuntimeBackendId, WorkspaceStateKey};
use workhive_core::errors::IndexError;
use workhive_core::ports::GitPort;
use workhive_infra::activity_event_store::FsActivityEventStore;
use workhive_infra::config::TomlConfigAdapter;
use workhive_infra::dependency::SystemDependencyChecker;
use workhive_infra::discovery::FsProjectDiscoveryAdapter;
use workhive_infra::environment::ProcessEnvironment;
use workhive_infra::executable::SystemExecutableChecker;
use workhive_infra::git::GitCliAdapter;
use workhive_infra::i18n::StaticI18n;
use workhive_infra::pane_launch_identity::JsonPaneLaunchIdentityStore;
use workhive_infra::pane_registry::InMemoryPaneRegistry;
use workhive_infra::pane_supervisor::JsonFilePaneSupervisor;
use workhive_infra::process_observer::create_default_observer;
use workhive_infra::project_index::JsonProjectIndex;
use workhive_infra::project_init_lock::FsProjectInitLockAdapter;
use workhive_infra::runtime::TmuxRuntimeAdapter;
use workhive_infra::snapshot_store::FileSnapshotStoreAdapter;
use workhive_infra::terminal_status_probe::TmuxTerminalStatusProbe;
use workhive_infra::tmux::TmuxCliAdapter;
use workhive_infra::tool_launch_instrumentation::WorkhiveToolLaunchInstrumentation;
use workhive_infra::workspace_fs::FsWorkspaceAdapter;
use workhive_infra::workspace_identity::FileIdentityAdapter;
use workhive_infra::workspace_provision::GitWorktreeProvisioner;
use workhive_infra::worktree_provenance::FileWorktreeProvenanceAdapter;

/// Builds concrete CLI adapters at the binary boundary.
pub struct CliComposition;

impl CliComposition {
    /// Create a composition root.
    pub fn new() -> Self {
        Self
    }

    /// Build the config adapter.
    pub fn config(&self) -> TomlConfigAdapter {
        TomlConfigAdapter::new()
    }

    /// Build the dependency checker.
    pub fn dependencies(&self) -> SystemDependencyChecker {
        SystemDependencyChecker::new()
    }

    /// Build the static i18n catalog.
    pub fn i18n(&self) -> StaticI18n {
        StaticI18n::new()
    }

    /// Build the in-memory pane registry used by CLI command flows.
    pub fn pane_registry(&self) -> InMemoryPaneRegistry {
        InMemoryPaneRegistry::new()
    }

    /// Build the durable pane launch-identity store rooted at a project.
    ///
    /// This is the cross-process bridge between `workspace open`, which
    /// writes authoritative launch identity here, and the detached
    /// control-plane supervisor, which hydrates from it at startup.
    pub fn pane_launch_identity_store(
        &self,
        project_root: &Utf8Path,
    ) -> JsonPaneLaunchIdentityStore {
        JsonPaneLaunchIdentityStore::new(project_root.to_path_buf())
    }

    /// Build the JSON-file pane supervisor adapter rooted at a
    /// project's runtime directory.
    ///
    /// The adapter exposes [`workhive_core::ports::PaneSupervisorPort`]
    /// for the layered observation merge — when no metadata exists
    /// for a unit it returns `Ok(None)` so the merge policy falls
    /// through to the OS observer / tmux fallback.
    pub fn pane_supervisor(&self, project_root: &Utf8Path) -> JsonFilePaneSupervisor {
        let runtime_root = project_root.join(".workhive").join("runtime");
        JsonFilePaneSupervisor::new(runtime_root)
    }

    /// Build the append-only Activity Control Plane event store
    /// rooted at an already-resolved runtime root.
    ///
    /// This is the centralized production constructor for
    /// [`FsActivityEventStore`]. Command handlers MUST NOT call
    /// `FsActivityEventStore::new(...)` directly; instead they
    /// route through this accessor (or
    /// [`Self::activity_event_store`] when only the project root is
    /// known). Centralizing construction here keeps runtime-root
    /// derivation, telemetry backends, and future Activity Control
    /// Plane wiring out of the CLI command modules.
    ///
    /// The hidden activity record/hook entry points already
    /// resolve the runtime root from `WORKHIVE_RUNTIME_ROOT` (the
    /// trusted env var Workhive itself injected), so they call
    /// this overload directly with the pinned tuple. The pane list
    /// / pane show read path derives the runtime root from the
    /// project root via the canonical `<project>/.workhive/runtime`
    /// layout and goes through the same accessor.
    pub fn activity_event_store_from_runtime_root(
        &self,
        runtime_root: &Utf8Path,
    ) -> FsActivityEventStore {
        FsActivityEventStore::new(runtime_root.to_path_buf())
    }

    /// Convenience accessor that derives the canonical runtime
    /// root `<project_root>/.workhive/runtime` and returns the
    /// resulting [`FsActivityEventStore`].
    ///
    /// Use this overload from production callers that hold only a
    /// project root (e.g. `pane list` / `pane show`). The
    /// derivation matches [`Self::pane_supervisor`] exactly so
    /// both ports observe the same on-disk layout.
    pub fn activity_event_store(&self, project_root: &Utf8Path) -> FsActivityEventStore {
        let runtime_root = project_root.join(".workhive").join("runtime");
        self.activity_event_store_from_runtime_root(&runtime_root)
    }

    /// Build the tool launch instrumentation adapter used by the
    /// `workspace open` and snapshot restore flows to wrap user-facing
    /// tool commands with a `workhive internal pane-supervisor`
    /// invocation. Returns an error when the running binary's path
    /// cannot be resolved — refusing to launch is the only safe
    /// behavior because an unsupervised launch would silently degrade
    /// the highest-confidence observation source.
    pub fn tool_launch_instrumentation(
        &self,
        project_root: &Utf8Path,
    ) -> Result<
        WorkhiveToolLaunchInstrumentation,
        workhive_core::ports::ToolLaunchInstrumentationError,
    > {
        let runtime_root = project_root.join(".workhive").join("runtime");
        WorkhiveToolLaunchInstrumentation::from_current_exe(
            project_root.to_path_buf(),
            runtime_root,
        )
    }

    /// Build the OS process observer adapter for the current
    /// platform. Returns an `Unsupported`-backed adapter on
    /// platforms without a ported observer so callers always have a
    /// real port instance.
    pub fn process_observer(
        &self,
    ) -> Box<dyn workhive_core::ports::ProcessObserverPort + Send + Sync> {
        create_default_observer()
    }

    /// Build the terminal status probe adapter.
    pub fn terminal_status_probe(&self) -> TmuxTerminalStatusProbe {
        TmuxTerminalStatusProbe::new()
    }

    /// Build the process environment adapter.
    pub fn environment(&self) -> ProcessEnvironment {
        ProcessEnvironment::new()
    }

    /// Runtime backend used by the current CLI composition.
    pub fn runtime_backend(&self) -> RuntimeBackendId {
        RuntimeBackendId::tmux()
    }

    /// Build the system git adapter.
    pub fn git(&self) -> GitCliAdapter {
        GitCliAdapter::new()
    }

    /// Build the workspace filesystem adapter.
    pub fn workspace_fs(&self) -> FsWorkspaceAdapter {
        FsWorkspaceAdapter::new()
    }

    /// Build the project index adapter.
    pub fn project_index(&self) -> Result<JsonProjectIndex, IndexError> {
        JsonProjectIndex::new()
    }

    /// Build the project initialization lock adapter.
    pub fn project_init_lock(&self) -> FsProjectInitLockAdapter {
        FsProjectInitLockAdapter::new()
    }

    /// Build the runtime control adapter.
    pub fn tmux(&self) -> TmuxCliAdapter {
        TmuxCliAdapter::new()
    }

    /// Adapt runtime control into runtime-neutral use-case ports.
    pub fn runtime<'a>(&self, tmux: &'a TmuxCliAdapter) -> TmuxRuntimeAdapter<'a> {
        TmuxRuntimeAdapter::new(tmux)
    }

    /// Adapt runtime control into runtime-neutral ports with registry hydration.
    pub fn runtime_with_registry<'a>(
        &self,
        tmux: &'a TmuxCliAdapter,
        pane_registry: &'a InMemoryPaneRegistry,
    ) -> TmuxRuntimeAdapter<'a> {
        TmuxRuntimeAdapter::with_pane_registry(tmux, pane_registry)
    }

    /// Build the executable availability checker.
    pub fn executable_checker(&self) -> SystemExecutableChecker {
        SystemExecutableChecker::new()
    }

    /// Build the project discovery adapter.
    pub fn project_discovery(&self) -> FsProjectDiscoveryAdapter {
        FsProjectDiscoveryAdapter::new()
    }

    /// Build the snapshot store adapter for a project.
    pub fn snapshot_store(&self, project_root: &Utf8Path) -> FileSnapshotStoreAdapter {
        FileSnapshotStoreAdapter::new(project_root)
    }

    /// Build the workspace identity adapter for a project.
    pub fn workspace_identity(&self, project_root: &Utf8Path) -> FileIdentityAdapter {
        FileIdentityAdapter::new(project_root)
    }

    /// Build the managed-worktree branch provenance store for a project.
    pub fn worktree_provenance(&self, project_root: &Utf8Path) -> FileWorktreeProvenanceAdapter {
        FileWorktreeProvenanceAdapter::new(project_root)
    }

    /// Build the git-backed workspace provisioner.
    pub fn worktree_provisioner<'a>(&self, git: &'a dyn GitPort) -> GitWorktreeProvisioner<'a> {
        GitWorktreeProvisioner::new(git)
    }

    /// Spawn the detached snapshot monitor process for a workspace.
    pub fn spawn_snapshot_monitor_process(
        &self,
        project_root: &Utf8Path,
        server: &str,
        session: &str,
        state_key: &WorkspaceStateKey,
    ) -> Result<(), std::io::Error> {
        let current_exe = std::env::current_exe()?;
        let argv = snapshot_monitor_argv(project_root, server, session, state_key);
        let mut command = std::process::Command::new(current_exe);
        command
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command.spawn()?;
        Ok(())
    }

    /// Spawn the detached pane runtime control plane supervisor.
    ///
    /// The child process owns one supervisor for the (project_root, server,
    /// workspace) tuple, binds the UDS at the canonical location returned by
    /// [`workhive_infra::control_plane::control_plane_socket_path`], and
    /// blocks on `serve_blocking`. The caller is expected to have probed
    /// the socket and detected a missing supervisor; this method is the
    /// only sanctioned way to put a supervisor on disk.
    ///
    /// Returns the spawned child's pid so the caller can wait for the
    /// socket to materialize and surface a typed error if it never does.
    /// Stdio is silenced — diagnostic output goes through the parent's
    /// file logger only, never to a user-facing tty.
    ///
    /// All production callers (`workspace open`, `pane send` self-heal)
    /// MUST use `SupervisorLifecycle::Session` so the supervisor's
    /// lifetime tracks the tmux workspace session and never the
    /// short-lived CLI process. The argv generated here therefore omits
    /// any `--parent-pid <current-cli-pid>` watchdog binding — binding to
    /// the CLI pid would tear the supervisor down as soon as the CLI
    /// exits. A future debugging entry point that wants
    /// the parent-pid watchdog must opt in explicitly via
    /// [`SupervisorLifecycle::ParentWatchdog`].
    ///
    /// Production code must always start the
    /// supervisor before the first `pane send`, and never silently fall
    /// back to direct tmux when it cannot.
    pub fn spawn_control_plane_supervisor(
        &self,
        project_root: &Utf8Path,
        server: &str,
        workspace: &str,
    ) -> Result<u32, std::io::Error> {
        self.spawn_control_plane_supervisor_with_lifecycle(
            project_root,
            server,
            workspace,
            SupervisorLifecycle::Session,
        )
    }

    /// Internal hook that lets non-production callers (e.g. one-off
    /// debug invocations) pick a different lifecycle. Production must
    /// keep going through [`Self::spawn_control_plane_supervisor`].
    pub fn spawn_control_plane_supervisor_with_lifecycle(
        &self,
        project_root: &Utf8Path,
        server: &str,
        workspace: &str,
        lifecycle: SupervisorLifecycle,
    ) -> Result<u32, std::io::Error> {
        let current_exe = std::env::current_exe()?;
        let argv = control_plane_supervisor_argv(project_root, server, workspace, lifecycle);
        let mut command = std::process::Command::new(current_exe);
        command
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let child = command.spawn()?;
        Ok(child.id())
    }

    /// Spawn the detached pane-title guard process.
    ///
    /// The guard reasserts Workhive-owned pane titles for a bounded
    /// window after `tmux attach` so runtime tools (Codex, OpenCode, …)
    /// cannot leave the user looking at a polluted `#{pane_title}`.
    /// Stdio is silenced because the parent's `attach` call has taken
    /// over the terminal — diagnostic output goes through the file
    /// logger only.
    pub fn spawn_pane_title_guard_process(
        &self,
        server: &str,
        session: &str,
        entries: &[(workhive_core::domain::UnitId, String)],
        interval_ms: u64,
        duration_ms: u64,
    ) -> Result<(), std::io::Error> {
        let current_exe = std::env::current_exe()?;
        let argv = crate::commands::pane_title_guard::pane_title_guard_argv(
            server,
            session,
            entries,
            interval_ms,
            duration_ms,
        );
        let mut command = std::process::Command::new(current_exe);
        command
            .args(&argv)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command.spawn()?;
        Ok(())
    }

    /// Query PTY usage through the infra host-probe helper
    /// (macOS `sysctl`; `None` on other platforms or probe failure).
    pub fn pty_usage(&self) -> Option<(u64, u64)> {
        workhive_infra::host_probe::pty_usage()
    }
}

impl Default for CliComposition {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the argv used to spawn the detached snapshot monitor supervisor.
///
/// The monitor lives under the hidden `internal snapshot monitor` parser
/// path (see [`crate::cli::InternalSnapshotAction::Monitor`]), so this
/// function MUST keep that path stable. Returning an owned `Vec<String>`
/// makes the argv trivially testable from CLI integration tests without
/// actually spawning a child process.
pub(crate) fn snapshot_monitor_argv(
    project_root: &Utf8Path,
    server: &str,
    session: &str,
    state_key: &WorkspaceStateKey,
) -> Vec<String> {
    vec![
        "internal".to_owned(),
        "snapshot".to_owned(),
        "monitor".to_owned(),
        "--project-root".to_owned(),
        project_root.as_str().to_owned(),
        "--server".to_owned(),
        server.to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "--state-key".to_owned(),
        state_key.to_string(),
    ]
}

/// Lifecycle policy for the spawned control plane supervisor.
///
/// Production paths (`workspace open`, `pane send` self-heal) MUST use
/// [`SupervisorLifecycle::Session`] — the supervisor outlives the
/// short-lived CLI process and stops once the tmux workspace session
/// disappears (probed by the supervisor itself). Binding the supervisor
/// to the CLI parent pid would tear it down on CLI exit and must not be
/// reintroduced for production callers.
///
/// [`SupervisorLifecycle::ParentWatchdog`] is only retained for ad-hoc
/// debugging or future tooling that wants the supervisor to die with a
/// known parent process; it is not used by `workspace open` /
/// `pane send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorLifecycle {
    /// Supervisor lifetime tracks the workspace/tmux session. The
    /// supervisor periodically probes `runtime.has_workspace(...)` and
    /// stops once the session is gone. `--parent-pid 0` is emitted so
    /// the receiving handler skips the parent watchdog entirely.
    Session,
    /// Bind supervisor lifetime to a specific parent pid via the
    /// `kill -0` watchdog. Reserved for non-production tooling.
    #[allow(dead_code)]
    ParentWatchdog { parent_pid: i32 },
}

/// Build the argv used to spawn the detached pane runtime control plane
/// supervisor. The argv lives under the hidden `internal control-plane
/// serve` path (see [`crate::cli::InternalControlPlaneAction::Serve`])
/// and is the only sanctioned way to start a supervisor — `pane send`
/// and `workspace open` both route through it via
/// [`CliComposition::spawn_control_plane_supervisor`].
///
/// The `lifecycle` argument is mapped to a `--parent-pid` value:
///
/// * [`SupervisorLifecycle::Session`] -> `--parent-pid 0`
///   (no parent watchdog; supervisor relies on the workspace probe)
/// * [`SupervisorLifecycle::ParentWatchdog`] -> the supplied pid.
///
/// This signature deliberately does NOT pull `std::process::id()` so
/// production callers cannot accidentally re-introduce the CLI-parent
/// lifecycle bug.
pub(crate) fn control_plane_supervisor_argv(
    project_root: &Utf8Path,
    server: &str,
    workspace: &str,
    lifecycle: SupervisorLifecycle,
) -> Vec<String> {
    let parent_pid: i32 = match lifecycle {
        SupervisorLifecycle::Session => 0,
        SupervisorLifecycle::ParentWatchdog { parent_pid } => parent_pid,
    };
    vec![
        "internal".to_owned(),
        "control-plane".to_owned(),
        "serve".to_owned(),
        "--project-root".to_owned(),
        project_root.as_str().to_owned(),
        "--server".to_owned(),
        server.to_owned(),
        "--workspace".to_owned(),
        workspace.to_owned(),
        "--parent-pid".to_owned(),
        parent_pid.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use workhive_core::domain::activity::{
        ActivityEvent, ActivityEventId, ActivityEventKind, ActivityOutcome, ObservationConfidence,
        PaneCorrelationKey, ProviderKind, ProviderSignalSource, SignalChannel,
    };
    use workhive_core::ports::ActivityEventStore;

    fn sample_state_key() -> WorkspaceStateKey {
        WorkspaceStateKey::new(
            "ABCD".to_owned(),
            "project-a".to_owned(),
            String::new(),
            "feat-a".to_owned(),
        )
    }

    #[test]
    fn snapshot_monitor_argv_uses_internal_path() {
        let argv = snapshot_monitor_argv(
            Utf8Path::new("/tmp/proj"),
            "srv",
            "sess",
            &sample_state_key(),
        );
        assert_eq!(argv[0], "internal");
        assert_eq!(argv[1], "snapshot");
        assert_eq!(argv[2], "monitor");
    }

    #[test]
    fn snapshot_monitor_argv_carries_required_flags() {
        let argv = snapshot_monitor_argv(
            Utf8Path::new("/tmp/proj"),
            "srv",
            "sess",
            &sample_state_key(),
        );
        let joined = argv.join(" ");
        assert!(joined.contains("--project-root /tmp/proj"));
        assert!(joined.contains("--server srv"));
        assert!(joined.contains("--session sess"));
        assert!(joined.contains("--state-key"));
    }

    #[test]
    fn snapshot_monitor_argv_parses_via_internal_clap_path() {
        // The argv must round-trip through the canonical `Cli` parser as
        // an `internal snapshot monitor` invocation. This is the contract
        // that prevents future drift back to the removed top-level path.
        let argv = snapshot_monitor_argv(
            Utf8Path::new("/tmp/proj"),
            "srv",
            "sess",
            &sample_state_key(),
        );
        let mut full: Vec<String> = vec!["workhive".to_owned()];
        full.extend(argv);
        let parsed =
            crate::cli::Cli::try_parse_from(full.iter()).expect("argv parses as internal command");
        match parsed.command {
            crate::cli::Commands::Internal {
                action: crate::cli::InternalAction::Snapshot { action },
            } => match action {
                crate::cli::InternalSnapshotAction::Monitor {
                    project_root,
                    server,
                    session,
                    state_key,
                } => {
                    assert_eq!(project_root, "/tmp/proj");
                    assert_eq!(server, "srv");
                    assert_eq!(session, "sess");
                    assert!(state_key.contains("project-a"));
                    assert!(state_key.contains("feat-a"));
                }
                other => panic!("expected Monitor, got {other:?}"),
            },
            other => panic!("expected Internal::Snapshot, got {other:?}"),
        }
    }

    #[test]
    fn control_plane_supervisor_argv_uses_internal_path() {
        let argv = control_plane_supervisor_argv(
            Utf8Path::new("/tmp/proj"),
            "whsrv-instance-proj",
            "ws-feat-a",
            SupervisorLifecycle::Session,
        );
        assert_eq!(argv[0], "internal");
        assert_eq!(argv[1], "control-plane");
        assert_eq!(argv[2], "serve");
    }

    #[test]
    fn control_plane_supervisor_argv_carries_required_flags() {
        let argv = control_plane_supervisor_argv(
            Utf8Path::new("/tmp/proj"),
            "whsrv-instance-proj",
            "ws-feat-a",
            SupervisorLifecycle::Session,
        );
        let joined = argv.join(" ");
        assert!(joined.contains("--project-root /tmp/proj"));
        assert!(joined.contains("--server whsrv-instance-proj"));
        assert!(joined.contains("--workspace ws-feat-a"));
        assert!(joined.contains("--parent-pid"));
    }

    /// Production lifecycle contract: the spawn argv MUST emit
    /// `--parent-pid 0` so the supervisor never binds its lifetime to
    /// the short-lived `workhive` CLI process. A contract that captured
    /// `std::process::id()` would make `pane send` repeatedly
    /// re-spawn the supervisor because the original process exited.
    #[test]
    fn control_plane_supervisor_argv_session_lifecycle_emits_parent_pid_zero() {
        let argv = control_plane_supervisor_argv(
            Utf8Path::new("/tmp/proj"),
            "whsrv-instance-proj",
            "ws-feat-a",
            SupervisorLifecycle::Session,
        );
        let pos = argv
            .iter()
            .position(|s| s == "--parent-pid")
            .expect("--parent-pid flag must be present");
        let value = argv
            .get(pos + 1)
            .expect("--parent-pid must carry a value")
            .clone();
        assert_eq!(
            value, "0",
            "production session lifecycle MUST pass `--parent-pid 0`; \
             binding to the CLI parent pid would tear the supervisor \
             down on CLI exit. got={value}"
        );
    }

    /// Sanity-check the explicit watchdog opt-in. This path is reserved
    /// for non-production tooling that intentionally wants the supervisor
    /// to die with a known parent.
    #[test]
    fn control_plane_supervisor_argv_parent_watchdog_passes_through_pid() {
        let argv = control_plane_supervisor_argv(
            Utf8Path::new("/tmp/proj"),
            "whsrv-instance-proj",
            "ws-feat-a",
            SupervisorLifecycle::ParentWatchdog { parent_pid: 4242 },
        );
        let joined = argv.join(" ");
        assert!(joined.contains("--parent-pid 4242"));
    }

    /// Production composition smoke: the activity event store
    /// produced from a runtime root MUST be a working append-only
    /// store at that exact path. This is the
    /// production-wrapper smoke test. It exercises the CLI composition path end-to-end
    /// against the real `FsActivityEventStore` implementation
    /// without going through any command handler.
    #[test]
    fn activity_event_store_from_runtime_root_yields_working_store_at_path() {
        let composition = CliComposition::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let runtime_root =
            camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("tempdir is utf8");
        let store = composition.activity_event_store_from_runtime_root(&runtime_root);
        let pane = PaneCorrelationKey::new("srv".to_owned(), "ws".to_owned(), "%0");
        let event = ActivityEvent {
            id: ActivityEventId::new_v4(),
            pane: pane.clone(),
            provider: ProviderKind::ClaudeCode,
            channel: SignalChannel::LifecycleHook,
            source: ProviderSignalSource::LifecycleHook,
            kind: ActivityEventKind::ToolStarted,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::High,
            observed_at_unix_ms: 1_000,
            detail: None,
        };
        store
            .append(&event)
            .expect("composition-built store accepts an append");
        let replayed = store
            .events_for_pane(&pane)
            .expect("composition-built store replays appended event");
        assert_eq!(
            replayed.len(),
            1,
            "composition.activity_event_store_from_runtime_root must produce a working store"
        );
        assert_eq!(replayed[0].id, event.id);
    }

    /// Production composition smoke: the project-root overload
    /// MUST derive `<project>/.workhive/runtime` (matching
    /// `pane_supervisor`) and produce a working store. The two
    /// overloads must observe the same on-disk layout for
    /// `pane_supervisor` / `activity_event_store` so the AI
    /// integrations gate, the legacy latest-fact store, and the
    /// append-only ACP store all agree on the runtime root.
    #[test]
    fn activity_event_store_derives_canonical_runtime_root_from_project_root() {
        let composition = CliComposition::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            camino::Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("tempdir is utf8");
        let derived_runtime_root = project_root.join(".workhive").join("runtime");
        let store = composition.activity_event_store(&project_root);
        let pane = PaneCorrelationKey::new("srv".to_owned(), "ws".to_owned(), "%0");
        let event = ActivityEvent {
            id: ActivityEventId::new_v4(),
            pane: pane.clone(),
            provider: ProviderKind::Codex,
            channel: SignalChannel::LifecycleHook,
            source: ProviderSignalSource::LifecycleHook,
            kind: ActivityEventKind::SessionStarted,
            outcome: ActivityOutcome::NotApplicable,
            confidence: ObservationConfidence::MediumLow,
            observed_at_unix_ms: 2_000,
            detail: None,
        };
        store
            .append(&event)
            .expect("project-root overload yields working store");
        // Cross-check: a second store hand-built from the
        // canonical derived path must see the same event the
        // overload-built store appended.
        let cross = composition.activity_event_store_from_runtime_root(&derived_runtime_root);
        let cross_replay = cross
            .events_for_pane(&pane)
            .expect("cross-store can replay the same canonical runtime root");
        assert_eq!(
            cross_replay.len(),
            1,
            "activity_event_store(project) must equal activity_event_store_from_runtime_root(\
             project/.workhive/runtime); on-disk derivation drift detected"
        );
        assert_eq!(cross_replay[0].id, event.id);
    }

    #[test]
    fn control_plane_supervisor_argv_parses_via_internal_clap_path() {
        // Round-trip the spawn argv through the production `Cli` parser as
        // proof that future changes to `internal control-plane serve` flags
        // do not silently break the supervisor lifecycle.
        let argv = control_plane_supervisor_argv(
            Utf8Path::new("/tmp/proj"),
            "whsrv-instance-proj",
            "ws-feat-a",
            SupervisorLifecycle::Session,
        );
        let mut full: Vec<String> = vec!["workhive".to_owned()];
        full.extend(argv);
        let parsed = crate::cli::Cli::try_parse_from(full.iter())
            .expect("control-plane argv parses as internal command");
        match parsed.command {
            crate::cli::Commands::Internal {
                action:
                    crate::cli::InternalAction::ControlPlane {
                        action:
                            crate::cli::InternalControlPlaneAction::Serve {
                                project_root,
                                server,
                                workspace,
                                parent_pid,
                            },
                    },
            } => {
                assert_eq!(project_root, "/tmp/proj");
                assert_eq!(server, "whsrv-instance-proj");
                assert_eq!(workspace, "ws-feat-a");
                assert_eq!(
                    parent_pid, 0,
                    "production spawn argv MUST NOT carry a real \
                     parent-pid; the supervisor lifecycle is bound to the \
                     workspace/tmux session, not to the short-lived \
                     `workhive` CLI process. got={parent_pid}"
                );
            }
            other => panic!("expected Internal::ControlPlane::Serve, got {other:?}"),
        }
    }
}

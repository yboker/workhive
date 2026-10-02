//! Port traits defining external system boundaries.
//!
//! Each port is a trait that abstracts an external dependency (git, runtime, filesystem, etc.).
//! Implementations (adapters) live in `workhive-infra`. Fake implementations for testing
//! live in `workhive-testkit`.
//!
//! This design enables:
//! - Unit testing with fakes (no real git/tmux needed)
//! - Future backend swaps (e.g., tmux -> wezterm) without touching business logic
//!
//! # Runtime boundary
//!
//! Business use cases in `workhive-core` depend on `WorkspaceRuntimePort` and
//! `PaneRuntimePort` — runtime-neutral abstractions. The `tmux` module is
//! **not** re-exported from this crate: it is an infra-level detail whose types
//! and trait live in `workhive-infra`.

mod activity_event_store;
mod clock;
mod config;
mod dependency;
mod discovery;
mod environment;
mod executable;
mod git;
mod i18n;
mod index_reconciliation;
mod message_bus;
mod notification;
mod orchestration;
mod pane_activity;
mod pane_launch_identity;
mod pane_registry;
mod pane_supervisor;
mod path_canonicalization;
mod path_completion;
mod process_launcher;
mod process_observer;
mod project_index;
mod project_init_lock;
mod runtime;
mod scheduler;
mod terminal_status_probe;
mod tool_launch_instrumentation;
mod workspace_fs;
mod workspace_identity;
mod workspace_provision;
mod worktree_provenance;

pub use activity_event_store::{ActivityEventStore, ActivityEventStoreError};
pub use clock::ClockPort;
pub use config::{
    AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig, AppConfig,
    CodexHookDetail, CodexNotifyConfig, ColumnConfig, ConfigPort, ConsoleLogConfig,
    DEFAULT_HERDR_SESSION, DiscoveryConfig, EnvLineIssue, EnvLineIssueKind, EnvLoadReport,
    FeaturesConfig, FileLogConfig, FileLogScope, HerdrConfig, LayoutConfig, LayoutTemplate,
    LogConfig, LogFormat, LogLevel, LogRetention, LogRotation, MAX_TOTAL_PANES,
    ManagedToolExitBehavior, NamingSettings, NotificationCategoryConfig, NotificationsConfig,
    PaneConfig, PaneExitConfig, PaneObservationConfig, PaneRuntimeConfig, ProjectsConfig,
    TMUX_CONFIG_SCHEMA_VERSION, TerminalStatusProbeConfig, TmuxConfig, ToolCatalogConfig,
};
pub use dependency::{BackendPreflight, DependencyKind, DependencyPort, DependencyStatus};
pub use discovery::{DiscoveredProject, DiscoveryReport, DiscoveryWarning, ProjectDiscoveryPort};
pub use environment::{EnvironmentPort, ExecutionContext, TmuxMembership};
pub use executable::ExecutableAvailabilityPort;
pub use git::{
    ChangeKind, ChangedFile, EnsureWorktreeRequest, GitPort, WorktreeInfo, WorktreeRemovePolicy,
    WorktreeStatus, WorktreeStrategy,
};
pub use i18n::I18nPort;
pub use index_reconciliation::{
    IndexReconciliationPort, ReconciliationLease, ReconciliationLeaseOutcome,
};
pub use message_bus::{
    DeliveryAttempt, PaneMessageBusError, PaneMessageBusPort, PaneMessageDeliveryError,
    PaneMessageDeliveryPort,
};
pub use notification::{
    NotificationCategory, NotificationDeliveryError, NotificationHint, NotificationOutbox,
    NotificationOutboxStore, NotificationOutboxStoreError, NotificationPort, NotificationRefusal,
    OutboxEntry, OutboxEntryStatus, PendingNotification, TransitionSignature,
};
pub use orchestration::OrchestrationPort;
pub use pane_activity::{PaneActivityIoError, PaneActivityKey, PaneActivityStorePort};
pub use pane_launch_identity::{
    PaneLaunchIdentityEntry, PaneLaunchIdentityError, PaneLaunchIdentityPort,
};
pub use pane_registry::{
    PaneMessagePort, PaneObserverPort, PaneRegistryPort, RegistryDelta, RegistryError,
};
pub use pane_supervisor::{PaneSupervisorError, PaneSupervisorPort};
pub use path_canonicalization::PathCanonicalizationPort;
pub use path_completion::{
    PathCompletionEntry, PathCompletionError, PathCompletionFsPort, PathKind,
};
pub use process_launcher::ProcessLauncher;
pub use process_observer::{PaneProcessLookup, ProcessObserverError, ProcessObserverPort};
pub use project_index::{
    IndexSnapshotToken, IndexState, PairedIndexState, ProjectIndexPort, ProjectSummary,
    SnapshotPublishOutcome,
};
pub use project_init_lock::{ProjectInitLockGuard, ProjectInitLockPort};
pub use runtime::{
    AttachWorkspaceRequest, BackendUnavailableReason, ConfiguredLayoutNode, ConfiguredLayoutPane,
    CreateWorkspaceRuntimeRequest, LayoutPanePosition, MaterializeConfiguredLayoutRequest,
    MaterializedConfiguredLayout, MaterializedLayoutPane, PaneRuntimePort,
    RegisterWorkspaceCheckoutRequest, RuntimeError, RuntimeInteractionPolicy, RuntimeUnitSnapshot,
    SendInputRequest, SetUnitTitleRequest, SpawnUnitRequest, WORKHIVE_HERDR_SESSION,
    WorkspaceRuntimeInfo, WorkspaceRuntimePort,
};
pub use scheduler::SchedulerPort;

pub use terminal_status_probe::{
    TerminalStatusCapture, TerminalStatusProbeError, TerminalStatusProbePort,
    TerminalStatusProbeRequest,
};
pub use tool_launch_instrumentation::{
    ToolLaunchInstrumentationError, ToolLaunchInstrumentationPort, WrapToolCommandRequest,
};
pub use workspace_fs::{DirectoryClassification, WorkspaceFsPort};
pub use workspace_identity::WorkspaceIdentityPort;
pub use workspace_provision::{
    ProvisionError, ProvisionOutcome, ProvisionWorkspaceRequest, WorkspaceProvisionPort,
    WorkspaceRootInfo,
};
pub use worktree_provenance::{BranchProvenance, WorktreeProvenanceError, WorktreeProvenancePort};

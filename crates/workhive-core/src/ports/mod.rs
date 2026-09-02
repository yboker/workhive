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
mod message_bus;
mod notification;
mod orchestration;
mod pane_activity;
mod pane_launch_identity;
mod pane_registry;
mod pane_supervisor;
mod path_completion;
mod process_observer;
mod project_index;
mod project_init_lock;
mod runtime;
mod scheduler;
mod snapshot_legacy;
mod snapshot_migration;
mod snapshot_runtime;
mod snapshot_store;
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
    DiscoveryConfig, EnvLineIssue, EnvLineIssueKind, EnvLoadReport, FeaturesConfig, FileLogConfig,
    FileLogScope, LayoutConfig, LayoutTemplate, LogConfig, LogFormat, LogLevel, LogRetention,
    LogRotation, MAX_TOTAL_PANES, ManagedToolExitBehavior, NamingSettings,
    NotificationCategoryConfig, NotificationsConfig, PaneConfig, PaneExitConfig,
    PaneObservationConfig, PaneRuntimeConfig, ProjectsConfig, SnapshotConfig,
    TerminalStatusProbeConfig, TmuxConfig, ToolCatalogConfig,
};
pub use dependency::{DependencyKind, DependencyPort, DependencyStatus};
pub use discovery::{DiscoveredProject, DiscoveryReport, DiscoveryWarning, ProjectDiscoveryPort};
pub use environment::{EnvironmentPort, ExecutionContext};
pub use executable::ExecutableAvailabilityPort;
pub use git::{
    ChangeKind, ChangedFile, EnsureWorktreeRequest, GitPort, WorktreeInfo, WorktreeRemovePolicy,
    WorktreeStatus, WorktreeStrategy,
};
pub use i18n::I18nPort;
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
pub use path_completion::{PathCompletionEntry, PathCompletionFsPort, PathKind};
pub use process_observer::{PaneProcessLookup, ProcessObserverError, ProcessObserverPort};
pub use project_index::{IndexState, ProjectIndexPort, ProjectSummary};
pub use project_init_lock::{ProjectInitLockGuard, ProjectInitLockPort};
pub use runtime::{
    AttachWorkspaceRequest, BackendUnavailableReason, CreateWorkspaceRuntimeRequest,
    PaneRuntimePort, RuntimeError, RuntimeUnitSnapshot, SendInputRequest, SetUnitTitleRequest,
    SpawnUnitRequest, WorkspaceRuntimeInfo, WorkspaceRuntimePort,
};
pub use scheduler::SchedulerPort;
pub use snapshot_legacy::{
    SNAPSHOT_SCHEMA_VERSION as LEGACY_SNAPSHOT_SCHEMA_VERSION,
    SnapshotDocument as LegacySnapshotDocument, SnapshotIdentity as LegacySnapshotIdentity,
    SnapshotInvalidReason as LegacySnapshotInvalidReason, SnapshotMeta as LegacySnapshotMeta,
    SnapshotMetadata as LegacySnapshotMetadata, SnapshotPort as LegacySnapshotPort,
    SnapshotUnit as LegacySnapshotUnit, SnapshotValidation as LegacySnapshotValidation,
    validate_snapshot as validate_legacy_snapshot,
    validate_snapshot_by_state_key as validate_legacy_snapshot_by_state_key,
};
pub use snapshot_migration::SnapshotMigrationPort;
pub use snapshot_runtime::{
    RuntimePaneSnapshot, RuntimeTopologySnapshot, RuntimeWindowSnapshot, SnapshotHookMode,
    SnapshotPaneRuntimePort, SnapshotRuntimePort, SplitPanePreparation,
};
pub use snapshot_store::SnapshotStorePort;
pub use terminal_status_probe::{
    TerminalStatusCapture, TerminalStatusProbeError, TerminalStatusProbePort,
    TerminalStatusProbeRequest,
};
pub use tool_launch_instrumentation::{
    ToolLaunchInstrumentationError, ToolLaunchInstrumentationPort, WrapToolCommandRequest,
};
pub use workspace_fs::WorkspaceFsPort;
pub use workspace_identity::WorkspaceIdentityPort;
pub use workspace_provision::{
    ProvisionError, ProvisionOutcome, ProvisionWorkspaceRequest, WorkspaceProvisionPort,
    WorkspaceRootInfo,
};
pub use worktree_provenance::{BranchProvenance, WorktreeProvenanceError, WorktreeProvenancePort};

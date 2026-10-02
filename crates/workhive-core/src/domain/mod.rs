//! Domain value objects and entities for workhive.
//!
//! These types enforce business invariants at construction time.
//! Invalid states are unrepresentable.

/// Activity control plane (Phase 1) skeleton.
///
/// Public sub-module instead of a flat re-export: the new
/// `PaneActivityState` here intentionally **shadows** the older
/// process-level `domain::pane::PaneActivityState` if flat-exported,
/// so consumers reach the new vocabulary via the qualified path
/// `workhive_core::domain::activity::PaneActivityState`. The legacy
/// process-level enum stays accessible via `domain::PaneActivityState`
/// for existing renderers.
pub mod activity;
mod ai_activity;
mod ai_integrations;
mod command_policy;
pub mod index_reconciliation;
mod locale;
mod message;
mod naming_policy;
mod observation;
mod orchestration;
mod pane;
mod project_state;
mod relative_project_path;
mod session_availability;
mod session_name;
mod task;
mod tool;
mod trusted_roots;
mod workspace_id;
mod workspace_identity;
mod workspace_provenance;

pub use ai_activity::{
    AI_ACTIVITY_DEFAULT_BUSY_TTL_MS, AI_ACTIVITY_SCHEMA_VERSION, AiActivityAccuracy,
    AiActivityConfidence, AiActivityFact, AiActivityProvider, AiActivitySource, AiActivityState,
    DisplayedAiActivity, ForegroundAgentSignal, ResolvedAiActivityDisplay,
    TerminalStatusObservation, merge_displayed_activity, project_ai_activity_source,
    resolve_ai_activity_display,
};
pub use ai_integrations::{
    AiIntegrationLifecycleState, LifecycleObservation, ProviderPolicy, policy_for,
    resolve_lifecycle_state,
};
pub use command_policy::{
    CommandKind, ConfirmationStage, ProjectInitBehavior, ProjectInitDecision,
};
pub use locale::Locale;
pub use message::{
    MessageAcceptance, MessageEnvelope, MessageEvent, MessageFailureReason, MessageId,
    MessageRejectReason, MessageStatus, MessageTarget,
};
pub use naming_policy::{NamingConfig, NamingContext, NamingError, SessionNamingPolicy};
pub use observation::{
    PaneObservation, PaneObservationConfidence, PaneObservationSource, PaneObservedCommand,
    PaneObservedProcess, PaneRuntimeTruth, derive_activity, derive_message_safety,
};
pub use orchestration::{
    DegradationReason, InvalidTransition, OrchestrationIntent, OrchestrationPlan,
    OrchestrationRequest, OrchestrationState, WorkerSelectionCriteria, validate_transition,
};
pub use pane::{
    ActivityState, ActorKind, InputCapability, ObservedPaneState, PaneActivityState,
    PaneCapability, PaneConciseActivity, PaneDescriptor, PaneDirection, PaneEvent, PaneId,
    PaneKind, PaneLaunchOrigin, PaneLifecycle, PaneLifecycleEvent, PaneMessage, PaneMessagePayload,
    PaneMessageSafety, PaneRole, PaneRuntimeId, PaneRuntimeStatus, PaneState, PaneStateChange,
    UnitId, WorkspaceInteractionMode, derive_capabilities, derive_concise_activity,
    derive_runtime_capabilities, derive_static_capabilities, runtime_status_from_state,
};
pub use project_state::{ProjectInitState, ProjectRuntimeState, RuntimeSessionState};
pub use relative_project_path::{RelativePathError, RelativeProjectPath};
pub use session_availability::{
    ReattachOutcome, SessionAvailability, SessionRecoveryAction, classify_reattach_outcome,
};
pub use session_name::SessionName;
pub use task::{
    InvalidTaskTransition, TaskAssignment, TaskId, TaskStatus, TaskValidationError,
    validate_task_transition,
};
pub use tool::{
    CatalogError, CommandSpec, PaneLaunchRef, ResolvedPaneLaunch, ToolCatalog, ToolEntry, ToolKind,
    ToolName, ToolNameError,
};
pub use trusted_roots::{
    TrustedRootDecision, TrustedRootSet, canonicalize_or_normalize, evaluate_project_root,
    expand_home, is_path_within, normalize_lexical,
};
pub use workspace_id::WorkspaceId;
pub use workspace_identity::{
    IdentityError, ProjectIdentityRecord, RUNTIME_BINDING_SCHEMA_VERSION, RuntimeBackendId,
    RuntimeBinding, RuntimeReplacement, WorkspaceIdentityKey, WorkspaceIdentityRecord,
};
pub use workspace_provenance::{
    MaterializationKind, WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider,
};

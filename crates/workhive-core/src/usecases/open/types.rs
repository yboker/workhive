#[allow(unused_imports)]
use std::collections::{BTreeMap, HashMap};

#[allow(unused_imports)]
use camino::Utf8PathBuf;

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{
    CommandSpec, MaterializationKind, PaneDirection, PaneLaunchIntent, PaneLaunchRef,
    ReattachOutcome, RuntimeBackendId, SessionAvailability, ToolCatalog, ToolName, WorkspaceId,
    WorkspaceOpenIntent, WorkspaceProvenance, WorkspaceProvider, classify_reattach_outcome,
};
#[allow(unused_imports)]
use crate::errors::SnapshotError;
#[allow(unused_imports)]
use crate::ports::{
    CreateWorkspaceRuntimeRequest, ExecutableAvailabilityPort, PaneRuntimePort, ProvisionError,
    ProvisionOutcome, ProvisionWorkspaceRequest, RuntimeError, SendInputRequest,
    SnapshotPaneRuntimePort, SnapshotRuntimePort, SnapshotStorePort, WorkspaceFsPort,
    WorkspaceProvisionPort, WorkspaceRuntimePort,
};

/// How `workhive workspace open` should behave.
///
/// Mutually exclusive modes for `workhive workspace open`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Default: reuse existing session if present, otherwise try
    /// snapshot restore (if enabled), otherwise apply config layout.
    Normal,

    /// `workhive workspace open -f`: force-recreate from config layout.
    ///
    /// 1. If the target session exists → destroy it.
    /// 2. If the target snapshot exists → delete it.
    /// 3. Create a fresh session from config layout.
    ///
    /// Never reuses existing sessions. Never restores snapshots.
    ForceRecreateFromLayout,
}

/// Errors from the `open` use case.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    Provision(#[from] ProvisionError),

    #[error(transparent)]
    Snapshot(#[from] SnapshotError),

    #[error("session lookup failed for '{session}': {availability}")]
    SessionLookupFailed {
        session: String,
        availability: SessionAvailability,
    },
}

/// Existing session discovered before `open` executes.
///
/// Canonical naming is preferred. Legacy naming is returned only for
/// compatibility lookup and transitional cleanup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExistingSession {
    Canonical { server: String, session: String },
    Legacy { server: String, session: String },
}

impl ExistingSession {
    /// Runtime group containing the discovered session.
    pub fn server(&self) -> &str {
        match self {
            Self::Canonical { server, .. } | Self::Legacy { server, .. } => server,
        }
    }

    /// Session name that currently exists in tmux.
    pub fn session(&self) -> &str {
        match self {
            Self::Canonical { session, .. } | Self::Legacy { session, .. } => session,
        }
    }

    /// Whether the discovered session uses legacy naming.
    pub fn is_legacy(&self) -> bool {
        matches!(self, Self::Legacy { .. })
    }
}

/// Declarative plan for opening a workspace.
///
/// Produced by `prepare_open_plan` (pure validation + queries, no mutations).
/// Consumed by `execute_open_plan` (all side effects happen here).
#[derive(Debug, Clone)]
pub struct OpenPlan {
    /// The workspace being opened.
    pub workspace: WorkspaceId,
    /// How the workspace root was resolved — typed provenance assembled by the use case.
    pub provenance: WorkspaceProvenance,
    /// Runtime group (resolved from naming policy).
    pub server: String,
    /// Workspace environment name (resolved from naming policy).
    pub session: String,
    /// Typed session availability — replaces the former `session_exists: bool`.
    ///
    /// `Running` means the session is live and can be reused (Normal) or
    /// must be destroyed (ForceRecreateFromLayout).
    /// `Missing` / `ServerUnreachable` mean a new session must be created.
    /// `LookupFailed` is an error state — no automated recovery.
    pub session_availability: SessionAvailability,
    /// Working directory for the workspace (project root or provisioned path).
    pub working_dir: Utf8PathBuf,
    /// Whether snapshot restore is enabled in project config.
    ///
    /// Only meaningful in `Normal` mode when creating a fresh session.
    pub snapshot_enabled: bool,
    /// How the open should behave.
    pub open_mode: OpenMode,
    /// Snapshot identity context for validation during restore.
    ///
    /// When `None`, snapshot validation is skipped (legacy compat).
    pub snapshot_identity: Option<SnapshotIdentityContext>,
    /// Effective environment variables to install in the workspace runtime.
    ///
    /// Already merged (global ∪ project) by the CLI before reaching the
    /// use case. Empty vec means no `.env` injection.
    pub env: Vec<(String, String)>,
}

/// Context needed to validate a snapshot's identity against the current
/// naming-derived values before restore.
#[derive(Debug, Clone)]
pub struct SnapshotIdentityContext {
    pub instance_id: String,
    pub project: String,
    pub runtime_backend: RuntimeBackendId,
    /// Typed state key derived from identity metadata.
    /// When present, `validate_snapshot_by_state_key` is used instead of
    /// field-by-field comparison.
    pub state_key: Option<crate::domain::WorkspaceStateKey>,
}

/// Describes what happened to the snapshot during `open`.
///
/// This is always visible in `OpenResult` — silent fallbacks are forbidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotOutcome {
    /// Snapshot restore was skipped because it is disabled in config.
    Disabled,
    /// No snapshot existed for the workspace; layout will be applied.
    NotFound,
    /// Snapshot restore succeeded; `panes` panes were created.
    Restored {
        panes: u32,
        fidelity: crate::domain::RestoreFidelity,
    },
    /// Snapshot restore failed (corrupted / I/O error); layout fallback applied.
    Failed { reason: String },
    /// Snapshot existed but was stale/incompatible; layout fallback applied.
    Stale { reason: String },
    /// Snapshot was deleted by ForceRecreateFromLayout before session creation.
    DeletedByForceRecreate,
}

impl SnapshotOutcome {
    /// Returns `true` when the caller should apply the effective layout.
    pub fn should_apply_layout(&self) -> bool {
        matches!(
            self,
            Self::Disabled
                | Self::NotFound
                | Self::Failed { .. }
                | Self::Stale { .. }
                | Self::DeletedByForceRecreate
        )
    }
}

/// Which recovery path was taken during `open`.
///
/// Priority order is absolute: live session > snapshot > layout.
/// `ForceRecreateFromLayout` always takes the layout path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRecoveryPath {
    /// An existing live session was reused — attach directly. (Normal mode only)
    ReusedLiveSession,
    /// Session was created and panes restored from a snapshot.
    RestoredFromSnapshot,
    /// Session was created using the effective layout (snapshot skipped/failed/absent).
    FellBackToLayout,
    /// Existing session was destroyed and recreated from layout. (ForceRecreateFromLayout only)
    ForceRecreatedFromLayout,
}

impl std::fmt::Display for OpenRecoveryPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReusedLiveSession => write!(f, "reused live session"),
            Self::RestoredFromSnapshot => write!(f, "restored from snapshot"),
            Self::FellBackToLayout => write!(f, "applied layout"),
            Self::ForceRecreatedFromLayout => write!(f, "force-recreated from layout"),
        }
    }
}

/// Consolidated user-facing outcome of an `open` operation.
///
/// Each variant represents a distinct, actionable outcome that the CLI
/// layer can render as a single-line summary.
///
/// Impossible state combinations are caught via `unreachable!()` rather
/// than silently coerced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenOutcome {
    /// The workspace already had a live session — no creation needed.
    ExistingSessionReused,
    /// A snapshot was restored successfully.
    SnapshotRestored {
        panes: u32,
        fidelity: crate::domain::RestoreFidelity,
    },
    /// No snapshot was found; layout was applied instead.
    SnapshotMissing,
    /// Snapshot restore failed; layout was applied as fallback.
    SnapshotRestoreFailed { reason: String },
    /// Snapshot was stale/incompatible; layout was applied as fallback.
    SnapshotStale { reason: String },
    /// Layout was applied (snapshot was disabled or force-recreate was used).
    LayoutApplied,
    /// Session was force-recreated from layout (existing session destroyed).
    ForceRecreated,
}

impl OpenOutcome {
    /// Derive the consolidated outcome from recovery path and snapshot state.
    ///
    /// # Panics
    ///
    /// Panics on impossible state combinations:
    /// - `RestoredFromSnapshot` with any snapshot outcome other than `Restored`
    /// - `FellBackToLayout` with `Restored` snapshot outcome
    pub fn derive(recovery_path: &OpenRecoveryPath, snapshot_outcome: &SnapshotOutcome) -> Self {
        match recovery_path {
            OpenRecoveryPath::ReusedLiveSession => Self::ExistingSessionReused,
            OpenRecoveryPath::ForceRecreatedFromLayout => Self::ForceRecreated,
            OpenRecoveryPath::RestoredFromSnapshot => match snapshot_outcome {
                SnapshotOutcome::Restored { panes, fidelity } => Self::SnapshotRestored {
                    panes: *panes,
                    fidelity: fidelity.clone(),
                },
                _ => unreachable!("RestoredFromSnapshot path requires Restored snapshot outcome"),
            },
            OpenRecoveryPath::FellBackToLayout => match snapshot_outcome {
                SnapshotOutcome::NotFound => Self::SnapshotMissing,
                SnapshotOutcome::Failed { reason } => Self::SnapshotRestoreFailed {
                    reason: reason.clone(),
                },
                SnapshotOutcome::Stale { reason } => Self::SnapshotStale {
                    reason: reason.clone(),
                },
                SnapshotOutcome::Disabled => Self::LayoutApplied,
                SnapshotOutcome::Restored { .. } => {
                    unreachable!("FellBackToLayout + Restored is an invalid state combination")
                }
                SnapshotOutcome::DeletedByForceRecreate => {
                    unreachable!(
                        "FellBackToLayout + DeletedByForceRecreate should use ForceRecreatedFromLayout path"
                    )
                }
            },
        }
    }

    /// Whether the caller should apply the effective layout after this outcome.
    pub fn should_apply_layout(&self) -> bool {
        matches!(
            self,
            Self::SnapshotMissing
                | Self::SnapshotRestoreFailed { .. }
                | Self::SnapshotStale { .. }
                | Self::LayoutApplied
                | Self::ForceRecreated
        )
    }

    /// Whether this outcome means an existing session was reused (no layout needed).
    pub fn is_session_reused(&self) -> bool {
        matches!(self, Self::ExistingSessionReused)
    }
}

/// Snapshot supervisor startup status attached to `OpenResult`.
///
/// The open use case always succeeds or fails independently from supervisor startup.
/// This typed outcome tracks whether snapshot supervision entered degraded mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotSupervisorOutcome {
    /// Snapshot supervision was not requested (snapshot disabled).
    NotRequested,
    /// Snapshot monitor process started successfully.
    Started,
    /// Snapshot monitor process failed to start.
    Degraded { reason: String },
}

/// Result of executing an `OpenPlan`.
pub struct OpenResult {
    pub session_name: String,
    pub worktree_path: Utf8PathBuf,
    pub restored_panes: u32,
    /// How the workspace root was resolved (propagated from the plan).
    pub provenance: WorkspaceProvenance,
    /// Which recovery path was taken — always set; never silent.
    pub recovery_path: OpenRecoveryPath,
    /// What happened with snapshot restore — always set; never silent.
    pub snapshot_outcome: SnapshotOutcome,
    /// Consolidated user-facing outcome — derived from recovery_path + snapshot_outcome.
    pub outcome: OpenOutcome,
    /// Typed reattach outcome — truthful representation of what happened
    /// to the remote session during open. CLI uses this for user-facing
    /// recoverable / unrecoverable / manual-intervention messaging.
    pub reattach_outcome: ReattachOutcome,
    /// Snapshot supervisor startup status (set by CLI orchestration layer).
    pub snapshot_supervisor_outcome: SnapshotSupervisorOutcome,
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use crate::ports::{AttachWorkspaceRequest, SendInputRequest, WorkspaceRuntimeInfo};

    #[test]
    fn snapshot_outcome_should_apply_layout_covers_all_non_restored() {
        assert!(SnapshotOutcome::Disabled.should_apply_layout());
        assert!(SnapshotOutcome::NotFound.should_apply_layout());
        assert!(
            SnapshotOutcome::Failed {
                reason: "test".to_owned()
            }
            .should_apply_layout()
        );
        assert!(
            SnapshotOutcome::Stale {
                reason: "old".to_owned()
            }
            .should_apply_layout()
        );
        assert!(SnapshotOutcome::DeletedByForceRecreate.should_apply_layout());
        assert!(
            !SnapshotOutcome::Restored {
                panes: 1,
                fidelity: crate::domain::RestoreFidelity::LaunchRestorable {
                    command_restored: 1,
                    shell_only: 0,
                },
            }
            .should_apply_layout()
        );
    }

    #[test]
    fn recovery_path_display_is_distinct() {
        let variants = [
            OpenRecoveryPath::ReusedLiveSession,
            OpenRecoveryPath::RestoredFromSnapshot,
            OpenRecoveryPath::FellBackToLayout,
            OpenRecoveryPath::ForceRecreatedFromLayout,
        ];
        let strings: Vec<String> = variants.iter().map(|v| v.to_string()).collect();
        for (i, a) in strings.iter().enumerate() {
            for (j, b) in strings.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

    // ── OpenOutcome::derive ──────────────────────────────────────────────

    #[test]
    fn outcome_reused_session() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::ReusedLiveSession,
            &SnapshotOutcome::Disabled,
        );
        assert_eq!(outcome, OpenOutcome::ExistingSessionReused);
        assert!(outcome.is_session_reused());
        assert!(!outcome.should_apply_layout());
    }

    #[test]
    fn outcome_snapshot_restored() {
        let fidelity = crate::domain::RestoreFidelity::LaunchRestorable {
            command_restored: 3,
            shell_only: 0,
        };
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::RestoredFromSnapshot,
            &SnapshotOutcome::Restored {
                panes: 3,
                fidelity: fidelity.clone(),
            },
        );
        assert_eq!(
            outcome,
            OpenOutcome::SnapshotRestored { panes: 3, fidelity }
        );
        assert!(!outcome.should_apply_layout());
        assert!(!outcome.is_session_reused());
    }

    #[test]
    fn outcome_snapshot_missing() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::FellBackToLayout,
            &SnapshotOutcome::NotFound,
        );
        assert_eq!(outcome, OpenOutcome::SnapshotMissing);
        assert!(outcome.should_apply_layout());
    }

    #[test]
    fn outcome_snapshot_failed() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::FellBackToLayout,
            &SnapshotOutcome::Failed {
                reason: "corrupt".to_owned(),
            },
        );
        assert_eq!(
            outcome,
            OpenOutcome::SnapshotRestoreFailed {
                reason: "corrupt".to_owned()
            }
        );
        assert!(outcome.should_apply_layout());
    }

    #[test]
    fn outcome_snapshot_stale() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::FellBackToLayout,
            &SnapshotOutcome::Stale {
                reason: "instance mismatch".to_owned(),
            },
        );
        assert_eq!(
            outcome,
            OpenOutcome::SnapshotStale {
                reason: "instance mismatch".to_owned()
            }
        );
        assert!(outcome.should_apply_layout());
    }

    #[test]
    fn outcome_layout_applied_disabled() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::FellBackToLayout,
            &SnapshotOutcome::Disabled,
        );
        assert_eq!(outcome, OpenOutcome::LayoutApplied);
        assert!(outcome.should_apply_layout());
    }

    #[test]
    fn outcome_force_recreated() {
        let outcome = OpenOutcome::derive(
            &OpenRecoveryPath::ForceRecreatedFromLayout,
            &SnapshotOutcome::DeletedByForceRecreate,
        );
        assert_eq!(outcome, OpenOutcome::ForceRecreated);
        assert!(outcome.should_apply_layout());
    }

    #[test]
    #[should_panic(expected = "RestoredFromSnapshot path requires Restored snapshot outcome")]
    fn outcome_restored_from_snapshot_with_not_found_panics() {
        OpenOutcome::derive(
            &OpenRecoveryPath::RestoredFromSnapshot,
            &SnapshotOutcome::NotFound,
        );
    }

    #[test]
    #[should_panic(expected = "RestoredFromSnapshot path requires Restored snapshot outcome")]
    fn outcome_restored_from_snapshot_with_disabled_panics() {
        OpenOutcome::derive(
            &OpenRecoveryPath::RestoredFromSnapshot,
            &SnapshotOutcome::Disabled,
        );
    }

    #[test]
    #[should_panic(expected = "RestoredFromSnapshot path requires Restored snapshot outcome")]
    fn outcome_restored_from_snapshot_with_failed_panics() {
        OpenOutcome::derive(
            &OpenRecoveryPath::RestoredFromSnapshot,
            &SnapshotOutcome::Failed {
                reason: "io error".to_owned(),
            },
        );
    }

    #[test]
    #[should_panic(expected = "FellBackToLayout + Restored is an invalid state combination")]
    fn outcome_fell_back_to_layout_with_restored_panics() {
        OpenOutcome::derive(
            &OpenRecoveryPath::FellBackToLayout,
            &SnapshotOutcome::Restored {
                panes: 2,
                fidelity: crate::domain::RestoreFidelity::LaunchRestorable {
                    command_restored: 2,
                    shell_only: 0,
                },
            },
        );
    }
}

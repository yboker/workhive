//! Pane activity state vocabulary required by the Phase-1 control
//! plane.
//!
//! This is the **product** state model — what `pane list`,
//! `pane show`, dashboards, notifications, and orchestration speak
//! about. It is intentionally larger than the older
//! [`crate::domain::AiActivityState`] (seven-state legacy
//! vocabulary): the requirement adds explicit `Completed`, `Idle`,
//! `Stale`, `Disabled`, `Unsupported`, and `Degraded` so that
//! downstream surfaces never have to alias these into
//! `WaitingForUser` or `Error`.
//!
//! The legacy enum is **not** removed — current pane rendering
//! still consumes it. This new enum is the target vocabulary the
//! projector / decision policy / read model speak; future
//! projection adapters bridge between the two when needed.

use serde::{Deserialize, Serialize};

/// Pane activity state for the activity control plane.
///
/// The variant set is pinned by the requirement and must not silently
/// alias.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum PaneActivityState {
    /// No reliable activity evidence at all.
    #[default]
    Unknown,
    /// AI pane / process exists, but no provider activity event has
    /// been received.
    OpenUnverified,
    /// Provider evidence says the agent is actively working.
    Working,
    /// Provider evidence says a tool is executing.
    ToolRunning,
    /// Provider evidence says user approval is needed.
    WaitingForPermission,
    /// Provider evidence says the latest turn completed.
    Completed,
    /// Provider evidence says the session is idle.
    Idle,
    /// Provider evidence says the latest turn failed or stopped
    /// with an error.
    Failed,
    /// Last provider event is too old to trust as current status.
    Stale,
    /// Provider integration is disabled by feature gate. Renderers
    /// must remain silent for disabled-by-config panes unless the
    /// user explicitly inspects integrations.
    Disabled,
    /// Provider has no supported event integration on this platform
    /// or version.
    Unsupported,
    /// Provider integration exists but could not prepare, verify,
    /// read, or correlate events fully.
    Degraded,
}

impl PaneActivityState {
    /// Whether the projector is allowed to drive orchestration
    /// decisions from this state. Per the requirement, only the
    /// "real lifecycle" states (and `Failed`, which is itself a
    /// terminal lifecycle outcome) qualify; `Stale`, `Disabled`,
    /// `Unsupported`, `Degraded`, `Unknown`, and `OpenUnverified`
    /// must always require additional confirmation.
    pub fn is_orchestration_eligible(self) -> bool {
        matches!(
            self,
            Self::Working
                | Self::ToolRunning
                | Self::WaitingForPermission
                | Self::Completed
                | Self::Idle
                | Self::Failed
        )
    }

    /// Whether the state expresses an explicit "no usable data"
    /// outcome (Disabled / Unsupported / Degraded / Unknown).
    /// Renderers use this to suppress noisy guidance when the
    /// pane is intentionally silent.
    pub fn is_inactive_outcome(self) -> bool {
        matches!(
            self,
            Self::Unknown | Self::Disabled | Self::Unsupported | Self::Degraded
        )
    }

    /// Whether the state is a busy/working state where new events
    /// MUST be allowed to overwrite older events even if the older
    /// one had higher confidence (e.g. a `Working` notify-medium
    /// followed by a hook-high `Completed` upgrades, not downgrades).
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Working | Self::ToolRunning | Self::WaitingForPermission
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Phase-1 vocabulary must include every state called out
    /// by the requirement. Lock the variant set so a future refactor
    /// cannot silently drop one of them.
    #[test]
    fn vocabulary_covers_every_required_state() {
        let all = [
            PaneActivityState::Unknown,
            PaneActivityState::OpenUnverified,
            PaneActivityState::Working,
            PaneActivityState::ToolRunning,
            PaneActivityState::WaitingForPermission,
            PaneActivityState::Completed,
            PaneActivityState::Idle,
            PaneActivityState::Failed,
            PaneActivityState::Stale,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
        ];
        // 12 distinct variants — pin the count too so silent merges
        // are caught by this test.
        assert_eq!(all.len(), 12);
        assert_eq!(
            all.iter().collect::<std::collections::BTreeSet<_>>().len(),
            12,
            "every required state must be a distinct variant",
        );
    }

    #[test]
    fn orchestration_eligibility_matches_requirement() {
        for s in [
            PaneActivityState::Working,
            PaneActivityState::ToolRunning,
            PaneActivityState::WaitingForPermission,
            PaneActivityState::Completed,
            PaneActivityState::Idle,
            PaneActivityState::Failed,
        ] {
            assert!(s.is_orchestration_eligible(), "{s:?} must be eligible");
        }
        for s in [
            PaneActivityState::Unknown,
            PaneActivityState::OpenUnverified,
            PaneActivityState::Stale,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
        ] {
            assert!(
                !s.is_orchestration_eligible(),
                "{s:?} must NOT be eligible (requirement: low/no confidence cannot drive orchestration)"
            );
        }
    }

    #[test]
    fn inactive_outcome_covers_disabled_unsupported_degraded_unknown() {
        for s in [
            PaneActivityState::Unknown,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
        ] {
            assert!(s.is_inactive_outcome());
        }
        for s in [
            PaneActivityState::Working,
            PaneActivityState::Idle,
            PaneActivityState::Stale,
            PaneActivityState::Completed,
        ] {
            assert!(!s.is_inactive_outcome());
        }
    }

    #[test]
    fn busy_states_are_only_working_tool_permission() {
        assert!(PaneActivityState::Working.is_busy());
        assert!(PaneActivityState::ToolRunning.is_busy());
        assert!(PaneActivityState::WaitingForPermission.is_busy());
        for non_busy in [
            PaneActivityState::Idle,
            PaneActivityState::Completed,
            PaneActivityState::Failed,
            PaneActivityState::Stale,
        ] {
            assert!(!non_busy.is_busy(), "{non_busy:?} must NOT be busy");
        }
    }

    #[test]
    fn state_round_trips_through_serde() {
        for s in [
            PaneActivityState::Working,
            PaneActivityState::WaitingForPermission,
            PaneActivityState::Completed,
            PaneActivityState::Disabled,
            PaneActivityState::Unsupported,
            PaneActivityState::Degraded,
        ] {
            let json = serde_json::to_string(&s).unwrap();
            let back: PaneActivityState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }
}

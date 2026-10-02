//! Session availability and reattach outcome types.
//!
//! These types carry NO user-facing prose. The `Display` impls produce
//! internal identifiers for tracing/logging only. The CLI layer maps each
//! variant to an i18n key for user-visible output.

use std::fmt;

/// The availability state of a workspace session.
///
/// Used by `ls` to show session state, by `open` to decide the recovery
/// path, and by error rendering to provide context on session lookup
/// failures.
///
/// Each variant maps to a specific runtime error class or success state
/// from `WorkspaceRuntimePort::has_workspace()`:
///
/// - `Ok(true)`  → `Running`
/// - `Ok(false)` → `Missing`
/// - `Err(BackendUnavailable)` → `ServerUnreachable`
/// - `Err(OperationFailed)` → `LookupFailed`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAvailability {
    /// tmux server responds AND the target session exists.
    Running,
    /// tmux server responds AND the target session does NOT exist.
    Missing,
    /// tmux server socket does not exist or refuses connection.
    ServerUnreachable,
    /// tmux command ran but returned an unexpected or malformed result.
    LookupFailed,
}

impl SessionAvailability {
    /// Whether this state allows reusing an existing session.
    pub fn allows_reuse(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Whether this state allows creating a new session.
    pub fn allows_creation(&self) -> bool {
        matches!(self, Self::Missing | Self::ServerUnreachable)
    }

    /// Whether this state is an error that blocks all automated recovery.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::LookupFailed)
    }
}

impl fmt::Display for SessionAvailability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Missing => write!(f, "Missing"),
            Self::ServerUnreachable => write!(f, "ServerUnreachable"),
            Self::LookupFailed => write!(f, "LookupFailed"),
        }
    }
}

/// Typed outcome of a session reattach attempt.
///
/// Each variant truthfully reports what happened when trying to reconnect
/// to an existing workspace session, including the exact recovery action
/// the user should take. No variant is silent; no backend failure is
/// disguised as "missing" or "manual".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReattachOutcome {
    /// Session is live and was reattached successfully.
    Reattached,
    /// Session was missing; a new one was created via the recovery chain.
    CreatedFresh,
    /// Backend (tmux server) was unreachable; recovery created a new server+session.
    BackendRecovered,
    /// Lookup returned an unrecoverable error; manual intervention is required.
    Unrecoverable { reason: String },
}

impl ReattachOutcome {
    /// The recovery action the CLI should communicate to the user.
    pub fn recovery_action(&self) -> SessionRecoveryAction {
        match self {
            Self::Reattached => SessionRecoveryAction::None,
            Self::CreatedFresh => SessionRecoveryAction::Recoverable {
                description: "session was missing; created from recovery chain".to_owned(),
            },
            Self::BackendRecovered => SessionRecoveryAction::Recoverable {
                description: "backend was unreachable; new server started".to_owned(),
            },
            Self::Unrecoverable { reason } => SessionRecoveryAction::ManualIntervention {
                reason: reason.clone(),
            },
        }
    }
}

impl fmt::Display for ReattachOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reattached => write!(f, "Reattached"),
            Self::CreatedFresh => write!(f, "CreatedFresh"),
            Self::BackendRecovered => write!(f, "BackendRecovered"),
            Self::Unrecoverable { reason } => write!(f, "Unrecoverable({reason})"),
        }
    }
}

/// What action the user/CLI should take in response to a session state.
///
/// This is the truthful, typed surface that the CLI renders directly.
/// Backend failures are NEVER silently downgraded to "missing" or "manual".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRecoveryAction {
    /// No action needed — session is healthy.
    None,
    /// The system recovered automatically; user should be informed.
    Recoverable { description: String },
    /// Automated recovery is not possible; user must intervene.
    ManualIntervention { reason: String },
}

impl fmt::Display for SessionRecoveryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "no action needed"),
            Self::Recoverable { description } => write!(f, "auto-recovered: {description}"),
            Self::ManualIntervention { reason } => {
                write!(f, "manual intervention required: {reason}")
            }
        }
    }
}

/// Classify a `SessionAvailability` into a `ReattachOutcome` for open/reattach flows.
///
/// This function translates the raw session availability into a typed
/// reattach outcome that truthfully represents what happened:
/// - `Running` → `Reattached`
/// - `Missing` → `CreatedFresh`
/// - `ServerUnreachable` → `BackendRecovered`
/// - `LookupFailed` → `Unrecoverable`
pub fn classify_reattach_outcome(
    availability: &SessionAvailability,
    created_new: bool,
) -> ReattachOutcome {
    match availability {
        SessionAvailability::Running => ReattachOutcome::Reattached,
        SessionAvailability::Missing if created_new => ReattachOutcome::CreatedFresh,
        SessionAvailability::Missing => ReattachOutcome::Unrecoverable {
            reason: "session missing and creation was not attempted".to_owned(),
        },
        SessionAvailability::ServerUnreachable if created_new => ReattachOutcome::BackendRecovered,
        SessionAvailability::ServerUnreachable => ReattachOutcome::Unrecoverable {
            reason: "backend server unreachable and creation was not attempted".to_owned(),
        },
        SessionAvailability::LookupFailed => ReattachOutcome::Unrecoverable {
            reason: "session lookup returned malformed result".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_allows_reuse_not_creation() {
        assert!(SessionAvailability::Running.allows_reuse());
        assert!(!SessionAvailability::Running.allows_creation());
        assert!(!SessionAvailability::Running.is_error());
    }

    #[test]
    fn missing_allows_creation_not_reuse() {
        assert!(!SessionAvailability::Missing.allows_reuse());
        assert!(SessionAvailability::Missing.allows_creation());
        assert!(!SessionAvailability::Missing.is_error());
    }

    #[test]
    fn server_unreachable_allows_creation() {
        assert!(!SessionAvailability::ServerUnreachable.allows_reuse());
        assert!(SessionAvailability::ServerUnreachable.allows_creation());
        assert!(!SessionAvailability::ServerUnreachable.is_error());
    }

    #[test]
    fn lookup_failed_blocks_everything() {
        assert!(!SessionAvailability::LookupFailed.allows_reuse());
        assert!(!SessionAvailability::LookupFailed.allows_creation());
        assert!(SessionAvailability::LookupFailed.is_error());
    }

    #[test]
    fn display_is_distinct() {
        let variants = [
            SessionAvailability::Running,
            SessionAvailability::Missing,
            SessionAvailability::ServerUnreachable,
            SessionAvailability::LookupFailed,
        ];
        let strings: Vec<String> = variants.iter().map(|v| v.to_string()).collect();
        for (i, a) in strings.iter().enumerate() {
            for (j, b) in strings.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "variants must have distinct Display output");
                }
            }
        }
    }

    // ── ReattachOutcome tests ────────────────────────────────────────

    #[test]
    fn reattach_running_is_reattached() {
        let outcome = classify_reattach_outcome(&SessionAvailability::Running, false);
        assert_eq!(outcome, ReattachOutcome::Reattached);
        assert_eq!(outcome.recovery_action(), SessionRecoveryAction::None);
    }

    #[test]
    fn reattach_missing_with_creation_is_created_fresh() {
        let outcome = classify_reattach_outcome(&SessionAvailability::Missing, true);
        assert_eq!(outcome, ReattachOutcome::CreatedFresh);
        assert!(matches!(
            outcome.recovery_action(),
            SessionRecoveryAction::Recoverable { .. }
        ));
    }

    #[test]
    fn reattach_missing_without_creation_is_unrecoverable() {
        let outcome = classify_reattach_outcome(&SessionAvailability::Missing, false);
        assert!(matches!(outcome, ReattachOutcome::Unrecoverable { .. }));
        assert!(matches!(
            outcome.recovery_action(),
            SessionRecoveryAction::ManualIntervention { .. }
        ));
    }

    #[test]
    fn reattach_server_unreachable_with_creation_is_backend_recovered() {
        let outcome = classify_reattach_outcome(&SessionAvailability::ServerUnreachable, true);
        assert_eq!(outcome, ReattachOutcome::BackendRecovered);
        assert!(matches!(
            outcome.recovery_action(),
            SessionRecoveryAction::Recoverable { .. }
        ));
    }

    #[test]
    fn reattach_server_unreachable_without_creation_is_unrecoverable() {
        let outcome = classify_reattach_outcome(&SessionAvailability::ServerUnreachable, false);
        assert!(matches!(outcome, ReattachOutcome::Unrecoverable { .. }));
        assert!(matches!(
            outcome.recovery_action(),
            SessionRecoveryAction::ManualIntervention { .. }
        ));
    }

    #[test]
    fn reattach_lookup_failed_is_always_unrecoverable() {
        let outcome = classify_reattach_outcome(&SessionAvailability::LookupFailed, true);
        assert!(matches!(outcome, ReattachOutcome::Unrecoverable { .. }));
        assert!(matches!(
            outcome.recovery_action(),
            SessionRecoveryAction::ManualIntervention { .. }
        ));
    }

    #[test]
    fn reattach_outcome_display_is_distinct() {
        let variants = [
            ReattachOutcome::Reattached,
            ReattachOutcome::CreatedFresh,
            ReattachOutcome::BackendRecovered,
            ReattachOutcome::Unrecoverable {
                reason: "test".to_owned(),
            },
        ];
        let strings: Vec<String> = variants.iter().map(|v| v.to_string()).collect();
        for (i, a) in strings.iter().enumerate() {
            for (j, b) in strings.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "ReattachOutcome variants must have distinct Display");
                }
            }
        }
    }

    #[test]
    fn session_recovery_action_display_is_distinct() {
        let variants = [
            SessionRecoveryAction::None,
            SessionRecoveryAction::Recoverable {
                description: "test".to_owned(),
            },
            SessionRecoveryAction::ManualIntervention {
                reason: "test".to_owned(),
            },
        ];
        let strings: Vec<String> = variants.iter().map(|v| v.to_string()).collect();
        for (i, a) in strings.iter().enumerate() {
            for (j, b) in strings.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        a, b,
                        "SessionRecoveryAction variants must have distinct Display"
                    );
                }
            }
        }
    }
}

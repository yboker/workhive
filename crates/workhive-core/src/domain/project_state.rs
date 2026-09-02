//! Project runtime state — unified state model for CLI commands.
//!
//! Instead of each handler independently checking `.workhive/` existence,
//! `instance_id` presence, and runtime session status, all commands consume
//! a pre-resolved `ProjectRuntimeState`.

/// Whether the project has been initialized with `workhive init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectInitState {
    /// `.workhive/` does not exist or has no `instance_id`.
    Uninitialized,
    /// `.workhive/` directory exists but is missing `instance_id`.
    /// This indicates a previous initialization was interrupted or corrupted.
    ///
    /// `config.toml` is optional because `workhive workspace open` can create runtime
    /// state before the user explicitly writes project configuration.
    PartiallyInitialized {
        has_dir: bool,
        has_instance_id: bool,
        has_config: bool,
    },
    /// `.workhive/` exists and contains a valid `instance_id`.
    Initialized { instance_id: String },
}

impl ProjectInitState {
    /// Returns `true` if the project is initialized.
    pub fn is_initialized(&self) -> bool {
        matches!(self, Self::Initialized { .. })
    }

    /// Returns `true` if the project is in a partially initialized state.
    pub fn is_partially_initialized(&self) -> bool {
        matches!(self, Self::PartiallyInitialized { .. })
    }

    /// Returns the instance_id if initialized, `None` otherwise.
    pub fn instance_id(&self) -> Option<&str> {
        match self {
            Self::Initialized { instance_id } => Some(instance_id),
            _ => None,
        }
    }
}

/// Runtime backend/session availability for a project.
///
/// Uses runtime-neutral vocabulary. The backing runtime (tmux, PTY, etc.)
/// is not named here; that detail belongs in the infra adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSessionState {
    /// The runtime backend is not available (no server/socket).
    BackendUnavailable,
    /// The backend is running but has no workspace sessions for this project.
    NoSession,
    /// One or more workspace sessions exist for this project.
    HasSessions,
}

/// Combined project runtime state resolved from multiple sources.
///
/// CLI handlers consume this instead of directly probing `.workhive/`,
/// `instance_id`, and the runtime backend independently.
#[derive(Debug, Clone)]
pub struct ProjectRuntimeState {
    /// Project initialization state.
    pub init: ProjectInitState,
    /// Runtime session availability.
    pub session: RuntimeSessionState,
}

impl ProjectRuntimeState {
    /// Whether the project is fully initialized.
    pub fn is_initialized(&self) -> bool {
        self.init.is_initialized()
    }

    /// Whether any runtime sessions are active for this project.
    pub fn has_active_sessions(&self) -> bool {
        self.session == RuntimeSessionState::HasSessions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uninitialized_state() {
        let state = ProjectRuntimeState {
            init: ProjectInitState::Uninitialized,
            session: RuntimeSessionState::BackendUnavailable,
        };
        assert!(!state.is_initialized());
        assert!(!state.has_active_sessions());
        assert!(state.init.instance_id().is_none());
        assert!(!state.init.is_partially_initialized());
    }

    #[test]
    fn partially_initialized_state() {
        let state = ProjectInitState::PartiallyInitialized {
            has_dir: true,
            has_instance_id: false,
            has_config: false,
        };
        assert!(!state.is_initialized());
        assert!(state.is_partially_initialized());
        assert!(state.instance_id().is_none());
    }

    #[test]
    fn initialized_no_sessions() {
        let state = ProjectRuntimeState {
            init: ProjectInitState::Initialized {
                instance_id: "a1b2".to_owned(),
            },
            session: RuntimeSessionState::NoSession,
        };
        assert!(state.is_initialized());
        assert!(!state.has_active_sessions());
        assert_eq!(state.init.instance_id(), Some("a1b2"));
    }

    #[test]
    fn initialized_with_sessions() {
        let state = ProjectRuntimeState {
            init: ProjectInitState::Initialized {
                instance_id: "c3d4".to_owned(),
            },
            session: RuntimeSessionState::HasSessions,
        };
        assert!(state.is_initialized());
        assert!(state.has_active_sessions());
    }

    #[test]
    fn backend_unavailable_state() {
        let state = ProjectRuntimeState {
            init: ProjectInitState::Initialized {
                instance_id: "e5f6".to_owned(),
            },
            session: RuntimeSessionState::BackendUnavailable,
        };
        assert!(state.is_initialized());
        assert!(!state.has_active_sessions());
    }
}

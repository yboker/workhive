//! CLI-layer error type with exit code mapping.
//!
//! Every domain error flows through [`CliError`] at the CLI boundary.
//! This provides:
//! - Uniform exit code assignment (see the exit-code contract)
//! - `miette::Diagnostic` support for rich terminal output
//! - `From` conversions for ergonomic `?` usage in command handlers

use std::process::ExitCode;

use miette::Diagnostic;
use workhive_core::domain::IdentityError;
use workhive_core::domain::NamingError;
use workhive_core::domain::SessionAvailability;
use workhive_core::errors::{
    ConfigError, DiscoveryError, EnvironmentError, GitError, I18nError, IndexError,
    ProjectInitError, SnapshotError, ValidationError, WorkspaceFsError,
};
use workhive_core::ports::ProvisionError;
use workhive_core::ports::RuntimeError;
use workhive_core::usecases::close::CloseError;
use workhive_core::usecases::destroy::DestroyError;
use workhive_core::usecases::init::InitError;
use workhive_core::usecases::layout_planner::LayoutPlanError;
use workhive_core::usecases::list::ListError;
use workhive_core::usecases::open::OpenError;
use workhive_core::usecases::reindex::ReindexError;
use workhive_core::usecases::sessions::SessionQueryError;
use workhive_infra::errors::TmuxError;

/// Unified error type at the CLI boundary.
///
/// Wraps all domain/infra errors and maps each to the appropriate
/// process exit code per the exit-code contract.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum CliError {
    #[error(transparent)]
    Validation(#[from] ValidationError),

    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Git(#[from] GitError),

    #[error(transparent)]
    Tmux(#[from] TmuxError),

    #[error(transparent)]
    Snapshot(#[from] SnapshotError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error(transparent)]
    I18n(#[from] I18nError),

    #[error(transparent)]
    WorkspaceFs(#[from] WorkspaceFsError),

    #[error(transparent)]
    Environment(#[from] EnvironmentError),

    #[error(transparent)]
    Discovery(#[from] DiscoveryError),

    #[error(transparent)]
    Naming(#[from] NamingError),

    #[error(transparent)]
    RuntimeOp(#[from] RuntimeError),

    #[error(transparent)]
    Provision(#[from] ProvisionError),

    #[error(transparent)]
    LayoutPlan(#[from] LayoutPlanError),

    #[error(transparent)]
    Identity(#[from] IdentityError),

    /// User cancelled a confirmation prompt.
    #[error("operation cancelled by user")]
    UserCancelled,

    /// User chose not to overwrite existing config.
    #[error("operation aborted by user")]
    UserAborted,

    /// A mutating command was run non-interactively without --force.
    #[error("{0}")]
    #[diagnostic(help("use --force to skip confirmation in non-interactive mode"))]
    ConfirmationRequired(String),

    /// Runtime/UI orchestration error represented as a user-facing string.
    #[error("{0}")]
    Runtime(String),

    /// Session lookup produced a non-actionable availability state.
    ///
    /// Structured data is preserved until a rendering site with i18n
    /// resolves it. The primary path is `render_open_error()` in `main.rs`;
    /// this variant is the fallback for any call site that converts via
    /// the generic `From<OpenError>`.
    #[error("session lookup failed for '{session}': {availability}")]
    SessionLookupFailed {
        session: String,
        availability: SessionAvailability,
    },

    /// Global config was just initialized via the intercept flow.
    /// The original command should NOT continue — user must re-run.
    #[error("{0}")]
    GlobalConfigJustInitialized(String),

    /// Project config was just initialized via the intercept flow.
    /// The original command should NOT continue — user must re-run.
    #[error("{0}")]
    ProjectJustInitialized(String),
}

impl From<SessionQueryError> for CliError {
    fn from(err: SessionQueryError) -> Self {
        match err {
            SessionQueryError::Runtime(e) => Self::RuntimeOp(e),
            SessionQueryError::Config(e) => Self::Config(e),
            SessionQueryError::Naming(e) => Self::Naming(e),
            SessionQueryError::SessionNameParseFailed {
                server,
                session_name,
            } => Self::Runtime(format!(
                "cannot extract workspace from session name '{session_name}' on server '{server}'"
            )),
        }
    }
}

impl From<OpenError> for CliError {
    fn from(err: OpenError) -> Self {
        match err {
            OpenError::Runtime(e) => Self::RuntimeOp(e),
            OpenError::Provision(e) => Self::Provision(e),
            OpenError::Snapshot(e) => Self::Snapshot(e),
            OpenError::SessionLookupFailed {
                session,
                availability,
            } => Self::SessionLookupFailed {
                session,
                availability,
            },
        }
    }
}

impl From<CloseError> for CliError {
    fn from(err: CloseError) -> Self {
        match err {
            CloseError::Runtime(e) => Self::RuntimeOp(e),
        }
    }
}

impl From<DestroyError> for CliError {
    fn from(err: DestroyError) -> Self {
        match err {
            DestroyError::Runtime(e) => Self::RuntimeOp(e),
            DestroyError::Provision(e) => Self::Provision(e),
            DestroyError::WorkspaceFs(e) => Self::WorkspaceFs(e),
            DestroyError::Snapshot(e) => Self::Snapshot(e),
            DestroyError::Index(e) => Self::Index(e),
            DestroyError::WorktreeRemovalFailed { .. } => Self::Runtime(err.to_string()),
            DestroyError::WorktreePathOutsideManagedBase { .. } => Self::Runtime(err.to_string()),
            DestroyError::OrphanManagedWorktreeDirs { .. } => Self::Runtime(err.to_string()),
        }
    }
}

impl From<ListError> for CliError {
    fn from(err: ListError) -> Self {
        match err {
            ListError::Runtime(e) => Self::RuntimeOp(e),
            ListError::Git(e) => Self::Git(e),
            ListError::WorkspaceFs(e) => Self::WorkspaceFs(e),
            ListError::Index(e) => Self::Index(e),
            ListError::Naming(e) => Self::Naming(e),
        }
    }
}

impl From<InitError> for CliError {
    fn from(err: InitError) -> Self {
        match err {
            InitError::Config(e) => Self::Config(e),
            InitError::WorkspaceFs(e) => Self::WorkspaceFs(e),
            InitError::Index(e) => Self::Index(e),
            InitError::SystemClock => Self::Runtime(err.to_string()),
        }
    }
}

impl From<ProjectInitError> for CliError {
    fn from(err: ProjectInitError) -> Self {
        match err {
            ProjectInitError::DeniedNonInteractive => Self::Runtime(err.to_string()),
            ProjectInitError::DeclinedByUser => Self::UserAborted,
            ProjectInitError::PartiallyInitialized { .. } => Self::Runtime(err.to_string()),
            ProjectInitError::LockFailed { .. } => Self::Runtime(err.to_string()),
            ProjectInitError::TransactionRolledBack { .. } => Self::Runtime(err.to_string()),
            ProjectInitError::StagingFailed { .. } => Self::Runtime(err.to_string()),
            ProjectInitError::CommitFailed { .. } => Self::Runtime(err.to_string()),
            ProjectInitError::Config(e) => Self::Config(e),
            ProjectInitError::WorkspaceFs(e) => Self::WorkspaceFs(e),
            ProjectInitError::Init(e) => Self::from(e),
            ProjectInitError::Index(e) => Self::Index(e),
        }
    }
}

impl From<ReindexError> for CliError {
    fn from(err: ReindexError) -> Self {
        match err {
            ReindexError::Discovery(e) => Self::Discovery(e),
            ReindexError::Index(e) => Self::Index(e),
        }
    }
}

impl CliError {
    /// Map this error to the appropriate process exit code.
    ///
    /// | Code | Meaning              |
    /// |------|----------------------|
    /// | 0    | Success              |
    /// | 1    | Runtime error        |
    /// | 2    | Invalid args / validation |
    /// | 3    | Resource not found   |
    /// | 130  | User interrupted     |
    pub fn exit_code(&self) -> ExitCode {
        let code: u8 = match self {
            Self::Validation(_) | Self::LayoutPlan(_) | Self::Identity(_) => 2,

            Self::Config(ConfigError::InvalidValue { .. }) => 2,

            Self::Config(ConfigError::NotFound { .. })
            | Self::Config(ConfigError::GlobalNotInitialized)
            | Self::Config(ConfigError::ProjectNotInitialized)
            | Self::Tmux(TmuxError::ServerNotRunning { .. })
            | Self::Tmux(TmuxError::SocketNotFound { .. })
            | Self::Tmux(TmuxError::SessionNotFound { .. })
            | Self::Tmux(TmuxError::PaneNotFound { .. })
            | Self::RuntimeOp(RuntimeError::WorkspaceNotFound { .. })
            | Self::RuntimeOp(RuntimeError::UnitNotFound { .. })
            | Self::RuntimeOp(RuntimeError::BackendUnavailable { .. })
            | Self::WorkspaceFs(WorkspaceFsError::PathNotFound { .. })
            | Self::Provision(ProvisionError::NotFound { .. }) => 3,

            Self::UserCancelled | Self::UserAborted => 130,

            Self::GlobalConfigJustInitialized(_) | Self::ProjectJustInitialized(_) => 0,

            _ => 1,
        };
        ExitCode::from(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_lookup_failed_preserves_structured_data() {
        let err = OpenError::SessionLookupFailed {
            session: "wh@test".into(),
            availability: SessionAvailability::LookupFailed,
        };
        let cli_err = CliError::from(err);

        match &cli_err {
            CliError::SessionLookupFailed {
                session,
                availability,
            } => {
                assert_eq!(session, "wh@test");
                assert_eq!(*availability, SessionAvailability::LookupFailed);
            }
            other => panic!("expected CliError::SessionLookupFailed, got {other:?}"),
        }
    }

    #[test]
    fn session_lookup_failed_fallback_does_not_emit_raw_i18n_key() {
        let err = OpenError::SessionLookupFailed {
            session: "wh@test".into(),
            availability: SessionAvailability::ServerUnreachable,
        };
        let cli_err = CliError::from(err);
        let display = format!("{cli_err}");

        assert!(
            !display.contains(workhive_core::i18n_keys::ERROR_SESSION_LOOKUP_FAILED_MSG),
            "fallback must not emit raw i18n key as user-facing text, got: {display}"
        );
    }

    #[test]
    fn project_just_initialized_maps_to_success_exit_code() {
        let err = CliError::ProjectJustInitialized("rerun".into());
        assert_eq!(err.exit_code(), ExitCode::from(0));
    }
}

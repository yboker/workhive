//! Command policy types — declarative semantics for CLI commands.
//!
//! These enums describe *what kind of command* is being run, rather than
//! leaving the CLI handler to self-enforce safety invariants.

/// Whether a command reads or writes state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Command only reads state. Must never create files, initialize projects,
    /// or modify tmux sessions.
    ReadOnly,
    /// Command may create files, tmux sessions, or modify configuration.
    Mutating,
}

/// How a command interacts with project initialization state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectInitBehavior {
    /// Command has no project context at all (e.g., `projects`, `sessions`).
    /// The policy preflight skips every project-state check.
    Forbidden,
    /// Command requires the project to already be initialized.
    /// Returns an error if `.workhive/` is missing.
    RequireInitialized,
    /// Command prefers current-project context, but may continue through a
    /// global read-only flow when the current repo is not initialized.
    AllowGlobalFallbackWhenUninitialized,
    /// Command may auto-initialize the project, but only with explicit consent.
    ///
    /// In interactive mode: the CLI must prompt for confirmation before any
    /// `.workhive/` writes. In non-interactive mode: the command must fail
    /// unless an explicit opt-in flag (`--init`) is provided.
    AllowAutoInitWithExplicitConsent,
    /// Command IS project-scoped but the global preflight must NOT decide
    /// initialization on its behalf. The command handler inspects the
    /// project state itself and either fails fast with a localized error
    /// (e.g. `workspace destroy` against an un-initialized project) or
    /// proceeds — but it must never silently create `.workhive/` on the
    /// user's behalf and must never prompt-to-init from inside a
    /// destructive flow.
    NotApplicable,
}

/// Typed decision from the project-init policy enforcement layer.
///
/// Produced by the CLI policy enforcer after inspecting project state,
/// interactivity, and explicit opt-in flags. Use cases consume this to
/// decide whether initialization should proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectInitDecision {
    /// Project is already initialized — no action needed.
    AlreadyInitialized { instance_id: String },
    /// Project is not initialized. Interactive mode should prompt.
    NeedsConfirmation,
    /// Non-interactive mode and no explicit opt-in — must fail.
    DeniedNonInteractive,
    /// User approved auto-init (interactive confirmation or `--init` flag).
    ApprovedAutoInit,
}

/// When the user must confirm before side effects begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationStage {
    /// No confirmation needed (read-only or force-default commands).
    None,
    /// Confirm before any mutation occurs.
    BeforeMutation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_kind_equality() {
        assert_eq!(CommandKind::ReadOnly, CommandKind::ReadOnly);
        assert_ne!(CommandKind::ReadOnly, CommandKind::Mutating);
    }

    #[test]
    fn project_init_behavior_equality() {
        assert_ne!(
            ProjectInitBehavior::Forbidden,
            ProjectInitBehavior::RequireInitialized
        );
        assert_ne!(
            ProjectInitBehavior::RequireInitialized,
            ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized
        );
        assert_ne!(
            ProjectInitBehavior::AllowGlobalFallbackWhenUninitialized,
            ProjectInitBehavior::AllowAutoInitWithExplicitConsent
        );
    }

    #[test]
    fn confirmation_stage_equality() {
        assert_eq!(ConfirmationStage::None, ConfirmationStage::None);
        assert_ne!(ConfirmationStage::None, ConfirmationStage::BeforeMutation);
    }

    #[test]
    fn project_init_decision_variants_are_distinct() {
        let already = ProjectInitDecision::AlreadyInitialized {
            instance_id: "a1b2".to_owned(),
        };
        let needs = ProjectInitDecision::NeedsConfirmation;
        let denied = ProjectInitDecision::DeniedNonInteractive;
        let approved = ProjectInitDecision::ApprovedAutoInit;
        assert_ne!(already, needs);
        assert_ne!(needs, denied);
        assert_ne!(denied, approved);
    }
}

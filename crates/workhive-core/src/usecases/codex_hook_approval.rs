//! Codex hook approval guidance decisions.

use crate::domain::{AiActivityProvider, AiActivitySource, AiIntegrationLifecycleState};
use crate::usecases::ai_integrations::FeatureGate;

/// Decision for `workhive pane approve-hooks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexHookApprovalDecision {
    Disabled,
    RejectNonCodex,
    RejectLowConfidence,
    AlreadyActive,
    SendHooksCommand { text: String },
}

/// Resolve the guided approval action without doing I/O.
pub fn resolve_codex_hook_approval(
    provider: AiActivityProvider,
    source: AiActivitySource,
    lifecycle: AiIntegrationLifecycleState,
    gate: &FeatureGate,
) -> CodexHookApprovalDecision {
    if !gate.is_provider_enabled(AiActivityProvider::Codex) {
        return CodexHookApprovalDecision::Disabled;
    }
    if provider != AiActivityProvider::Codex {
        return CodexHookApprovalDecision::RejectNonCodex;
    }
    if matches!(
        source,
        AiActivitySource::TmuxFallback | AiActivitySource::None
    ) {
        return CodexHookApprovalDecision::RejectLowConfidence;
    }
    if matches!(source, AiActivitySource::CodexHook)
        || matches!(lifecycle, AiIntegrationLifecycleState::Active)
    {
        return CodexHookApprovalDecision::AlreadyActive;
    }
    if matches!(lifecycle, AiIntegrationLifecycleState::ReviewRequired) {
        return CodexHookApprovalDecision::SendHooksCommand {
            text: "/hooks".to_owned(),
        };
    }
    CodexHookApprovalDecision::RejectLowConfidence
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecases::ai_integrations::FeatureGate;

    #[test]
    fn rejects_non_codex_without_injection() {
        let decision = resolve_codex_hook_approval(
            AiActivityProvider::ClaudeCode,
            AiActivitySource::WorkhiveSupervisor,
            AiIntegrationLifecycleState::ReviewRequired,
            &FeatureGate::builtin_default(),
        );
        assert_eq!(decision, CodexHookApprovalDecision::RejectNonCodex);
    }

    #[test]
    fn active_hook_is_noop() {
        let decision = resolve_codex_hook_approval(
            AiActivityProvider::Codex,
            AiActivitySource::CodexHook,
            AiIntegrationLifecycleState::Active,
            &FeatureGate::builtin_default(),
        );
        assert_eq!(decision, CodexHookApprovalDecision::AlreadyActive);
    }

    #[test]
    fn review_required_sends_exactly_hooks() {
        let decision = resolve_codex_hook_approval(
            AiActivityProvider::Codex,
            AiActivitySource::WorkhiveSupervisor,
            AiIntegrationLifecycleState::ReviewRequired,
            &FeatureGate::builtin_default(),
        );
        assert_eq!(
            decision,
            CodexHookApprovalDecision::SendHooksCommand {
                text: "/hooks".to_owned()
            }
        );
    }

    #[test]
    fn never_sends_trust_keystroke() {
        let decision = resolve_codex_hook_approval(
            AiActivityProvider::Codex,
            AiActivitySource::TerminalStatus,
            AiIntegrationLifecycleState::ReviewRequired,
            &FeatureGate::builtin_default(),
        );
        match decision {
            CodexHookApprovalDecision::SendHooksCommand { text } => {
                assert_eq!(text, "/hooks");
                assert!(!text.contains('t'));
            }
            other => panic!("expected send decision, got {other:?}"),
        }
    }

    #[test]
    fn respects_disabled_gate() {
        let gate = FeatureGate::from_switches(false, true, true, true);
        let decision = resolve_codex_hook_approval(
            AiActivityProvider::Codex,
            AiActivitySource::WorkhiveSupervisor,
            AiIntegrationLifecycleState::ReviewRequired,
            &gate,
        );
        assert_eq!(decision, CodexHookApprovalDecision::Disabled);
    }
}

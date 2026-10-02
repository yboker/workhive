//! Ordering seam between usage-metadata writes, attach guidance, and
//! blocking attach.
//!
//! Attaching transfers the terminal into the workspace runtime and only
//! returns when the user detaches — potentially hours later. Usage
//! metadata ("last used") must therefore be persisted *before* the
//! attach call, so it reflects when the workspace was entered rather
//! than when the session ended. Attach guidance must be emitted after
//! metadata but before the blocking attach so the user sees instructions
//! while they can still read stdout/stderr.

use workhive_core::ports::{
    AttachWorkspaceRequest, ExecutionContext, I18nPort, RuntimeError, WorkspaceRuntimePort,
};
use workhive_core::usecases::tmux_navigation::{
    NavigationContextError, NavigationTarget, TmuxNavigationDecision, decide_navigation,
};

use workhive_core::domain::{Locale, RuntimeBackendId, RuntimeBinding};

use crate::error::CliError;

/// Determines metadata write behavior for AlreadyCurrent navigation.
///
/// Explicit attach (e.g. `workspace attach`) treats AlreadyCurrent as a
/// pure no-op — no metadata mutation. Workspace-open treats it as a
/// successful open event that records one usage timestamp even though no
/// runtime attach/switch occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageMetadataPolicy {
    ExplicitAttach,
    WorkspaceOpen,
}

/// Result of the navigation-aware attach orchestration.
///
/// Returned by the shared entry points so callers can distinguish "we
/// attached/switched" from "we are already in the target workspace" or
/// "cross-group was rejected".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AttachOutcome {
    /// Terminal was attached to the target (outside tmux path).
    Attached,
    /// Client was switched to the target session (same-server path).
    Switched,
    /// Already in the target — no runtime operation performed.
    AlreadyCurrent,
}

/// Typed error for cross-group rejection or unresolved context at the CLI boundary.
#[derive(Debug)]
pub(crate) enum AttachNavigationError {
    /// Cross-server attach was requested.
    CrossGroup {
        current_server: String,
        target_server: String,
    },
    /// Tmux evidence exists but identity could not be resolved.
    #[allow(dead_code)]
    UnresolvedContext(NavigationContextError),
    /// Underlying runtime error from attach/switch.
    Runtime(RuntimeError),
}

impl From<RuntimeError> for AttachNavigationError {
    fn from(e: RuntimeError) -> Self {
        Self::Runtime(e)
    }
}

/// Lifecycle callbacks for attach orchestration.
///
/// Consolidates the four callback parameters that control side effects during
/// the attach flow: marking workspace usage, emitting ready output, emitting
/// guidance, and emitting already-current messages.
pub(crate) struct AttachLifecycleCallbacks<
    MarkUsed: FnOnce(),
    EmitReadyOutput: FnOnce(),
    EmitGuidance: FnOnce(),
    EmitAlreadyCurrent: FnOnce(),
> {
    /// Called after usage metadata is persisted but before any output.
    pub mark_used: MarkUsed,
    /// Called to emit ready/success output before blocking attach.
    pub emit_ready_output: EmitReadyOutput,
    /// Called to emit guidance before blocking attach.
    pub emit_guidance: EmitGuidance,
    /// Called when already in the target workspace.
    pub emit_already_current: EmitAlreadyCurrent,
}

pub(crate) fn mark_used_then_attach_binding<MU, ERO, EG, EAC>(
    runtime: &dyn WorkspaceRuntimePort,
    binding: &RuntimeBinding,
    request: AttachWorkspaceRequest,
    exec_ctx: &ExecutionContext,
    callbacks: AttachLifecycleCallbacks<MU, ERO, EG, EAC>,
) -> Result<AttachOutcome, AttachNavigationError>
where
    MU: FnOnce(),
    ERO: FnOnce(),
    EG: FnOnce(),
    EAC: FnOnce(),
{
    if binding.backend == RuntimeBackendId::tmux() {
        return mark_used_then_attach(runtime, request, exec_ctx, callbacks);
    }
    // Herdr focus is non-blocking; tmux-only detach guidance would be wrong.
    let AttachLifecycleCallbacks {
        mark_used,
        emit_ready_output,
        ..
    } = callbacks;
    mark_used();
    emit_ready_output();
    runtime.attach_workspace_binding(binding, request.interaction_policy)?;
    Ok(AttachOutcome::Attached)
}

pub(crate) fn mark_used_then_maybe_attach_binding<MU, ERO, EG, EAC>(
    runtime: &dyn WorkspaceRuntimePort,
    binding: &RuntimeBinding,
    request: AttachWorkspaceRequest,
    attach: bool,
    exec_ctx: &ExecutionContext,
    callbacks: AttachLifecycleCallbacks<MU, ERO, EG, EAC>,
) -> Result<AttachOutcome, AttachNavigationError>
where
    MU: FnOnce(),
    ERO: FnOnce(),
    EG: FnOnce(),
    EAC: FnOnce(),
{
    if binding.backend == RuntimeBackendId::tmux() {
        return mark_used_then_maybe_attach(runtime, request, attach, exec_ctx, callbacks);
    }
    // Herdr focus is non-blocking; tmux-only detach guidance would be wrong.
    let AttachLifecycleCallbacks {
        mark_used,
        emit_ready_output,
        ..
    } = callbacks;
    mark_used();
    emit_ready_output();
    if attach {
        runtime.attach_workspace_binding(binding, request.interaction_policy)?;
        Ok(AttachOutcome::Attached)
    } else {
        Ok(AttachOutcome::AlreadyCurrent)
    }
}

/// Persist usage metadata, emit ready output and guidance, then block on the runtime attach.
///
/// Stable ordering for Attach/Switch: `mark_used()` → `emit_ready_output()` → `emit_guidance()` → runtime op.
/// AlreadyCurrent: emit only already-current, no metadata.
/// Cross-group/Unresolved: error before any side effect.
pub(crate) fn mark_used_then_attach<MU, ERO, EG, EAC>(
    runtime: &dyn WorkspaceRuntimePort,
    request: AttachWorkspaceRequest,
    exec_ctx: &ExecutionContext,
    callbacks: AttachLifecycleCallbacks<MU, ERO, EG, EAC>,
) -> Result<AttachOutcome, AttachNavigationError>
where
    MU: FnOnce(),
    ERO: FnOnce(),
    EG: FnOnce(),
    EAC: FnOnce(),
{
    let target = NavigationTarget {
        group: request.group.clone(),
        workspace_name: request.workspace_name.clone(),
    };
    let decision =
        decide_navigation(exec_ctx, &target).map_err(AttachNavigationError::UnresolvedContext)?;

    execute_decision_unified(
        runtime,
        request,
        decision,
        UsageMetadataPolicy::ExplicitAttach,
        callbacks,
    )
}

/// Persist usage metadata, then optionally emit ready output, guidance, and attach.
///
/// `workspace open` uses this seam for both attach modes: the usage
/// write always runs after successful workspace preparation, and the
/// guidance + blocking attach are skipped entirely under `--no-attach`
/// while the timestamp still records the open event.
///
/// For `--no-attach`: marks used, emits ready output, and returns without resolving navigation.
/// For attach: resolves navigation; rejects cross-group/unresolved BEFORE
/// metadata, then marks used and executes the successful path.
pub(crate) fn mark_used_then_maybe_attach<MU, ERO, EG, EAC>(
    runtime: &dyn WorkspaceRuntimePort,
    request: AttachWorkspaceRequest,
    attach: bool,
    exec_ctx: &ExecutionContext,
    callbacks: AttachLifecycleCallbacks<MU, ERO, EG, EAC>,
) -> Result<AttachOutcome, AttachNavigationError>
where
    MU: FnOnce(),
    ERO: FnOnce(),
    EG: FnOnce(),
    EAC: FnOnce(),
{
    if !attach {
        (callbacks.mark_used)();
        (callbacks.emit_ready_output)();
        return Ok(AttachOutcome::AlreadyCurrent);
    }

    let target = NavigationTarget {
        group: request.group.clone(),
        workspace_name: request.workspace_name.clone(),
    };
    let decision =
        decide_navigation(exec_ctx, &target).map_err(AttachNavigationError::UnresolvedContext)?;

    execute_decision_unified(
        runtime,
        request,
        decision,
        UsageMetadataPolicy::WorkspaceOpen,
        callbacks,
    )
}

fn execute_decision_unified<MU, ERO, EG, EAC>(
    runtime: &dyn WorkspaceRuntimePort,
    request: AttachWorkspaceRequest,
    decision: TmuxNavigationDecision,
    metadata_policy: UsageMetadataPolicy,
    callbacks: AttachLifecycleCallbacks<MU, ERO, EG, EAC>,
) -> Result<AttachOutcome, AttachNavigationError>
where
    MU: FnOnce(),
    ERO: FnOnce(),
    EG: FnOnce(),
    EAC: FnOnce(),
{
    let AttachLifecycleCallbacks {
        mark_used,
        emit_ready_output,
        emit_guidance,
        emit_already_current,
    } = callbacks;

    match decision {
        TmuxNavigationDecision::Attach => {
            (mark_used)();
            (emit_ready_output)();
            (emit_guidance)();
            runtime.attach_workspace(request)?;
            Ok(AttachOutcome::Attached)
        }
        TmuxNavigationDecision::AlreadyCurrent => {
            if metadata_policy == UsageMetadataPolicy::WorkspaceOpen {
                (mark_used)();
                (emit_already_current)();
                (emit_ready_output)();
            } else {
                (emit_already_current)();
            }
            Ok(AttachOutcome::AlreadyCurrent)
        }
        TmuxNavigationDecision::SwitchWithinGroup => {
            (mark_used)();
            (emit_ready_output)();
            (emit_guidance)();
            runtime.switch_workspace(
                &request.group,
                &request.workspace_name,
                &request.interaction_policy,
            )?;
            Ok(AttachOutcome::Switched)
        }
        TmuxNavigationDecision::RejectCrossGroup {
            current_server,
            target_server,
        } => Err(AttachNavigationError::CrossGroup {
            current_server,
            target_server,
        }),
    }
}

/// Map an `AttachNavigationError` into a CLI-boundary error with localized messages.
pub(crate) fn map_navigation_error(
    err: AttachNavigationError,
    i18n: &dyn I18nPort,
    locale: Locale,
) -> CliError {
    use workhive_core::i18n_keys as k;
    match err {
        AttachNavigationError::CrossGroup {
            current_server,
            target_server,
        } => CliError::Runtime(i18n.resolve_with_args(
            locale,
            k::ATTACH_CROSS_GROUP_REJECTED,
            &[&target_server, &current_server],
        )),
        AttachNavigationError::UnresolvedContext(_) => {
            CliError::Runtime(i18n.resolve_with_args(locale, k::ATTACH_PARTIAL_CONTEXT_ERROR, &[]))
        }
        AttachNavigationError::Runtime(e) => CliError::RuntimeOp(e),
    }
}

#[cfg(test)]
#[path = "attach_usage_tests.rs"]
mod tests;

//! CLI-boundary localization for typed errors.
//!
//! The CLI must NEVER let the raw `Display` text of a typed runtime
//! error reach the user. The `RuntimeError` enum is engineered for
//! programmatic dispatch — its `#[error("...")]` strings are
//! diagnostic English, not human-facing copy. When the user runs
//! `workhive workspace attach` against an unreachable backend, the
//! message must come out of the i18n catalog in the user's
//! configured language.
//!
//! This module owns that translation. It accepts a [`CliError`] and
//! returns a fully-rendered, locale-respecting string for each
//! variant that has a typed reason. If no localization is available
//! the function returns `None` so the caller can fall back to
//! `miette`'s diagnostic renderer (which carries chained-source
//! breadcrumbs for unstructured errors).
//!
//! Keep this module side-effect-free so unit tests can exhaustively
//! exercise it without spinning up the runtime.

use workhive_core::domain::Locale;
use workhive_core::errors::EnvironmentError;
use workhive_core::i18n_keys as k;
use workhive_core::ports::{BackendUnavailableReason, I18nPort, RuntimeError};

use crate::error::CliError;

/// Render a [`CliError`] to a localized user-facing string when a
/// typed translation is available.
///
/// Returns `None` for variants that this module does not yet
/// localize (the caller falls back to `miette` for those). Returning
/// `None` is intentional — adding a typed error variant to the
/// renderer is an explicit, reviewable change.
pub fn render_localized_error(
    err: &CliError,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> Option<String> {
    match err {
        CliError::RuntimeOp(runtime_err) => Some(render_runtime_error(runtime_err, locale, i18n)),
        CliError::Environment(env_err) => render_environment_error(env_err, locale, i18n),
        _ => None,
    }
}

/// Render an [`EnvironmentError`] to localized, actionable copy for the
/// variants that carry curated recovery guidance.
///
/// Returns `None` for variants without a dedicated catalog entry so the
/// caller keeps the `miette` fallback. Today only
/// [`EnvironmentError::NotInWorkspaceSession`] is localized: it is the
/// state a user hits when running a workspace-scoped command (such as
/// `workhive pane ls`) from a shell that is not inside a workhive-managed
/// session, and the catalog entry names the commands that recover from it.
fn render_environment_error(
    err: &EnvironmentError,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> Option<String> {
    match err {
        EnvironmentError::NotInWorkspaceSession => {
            Some(i18n.resolve(locale, k::ERROR_NOT_IN_SESSION))
        }
        _ => None,
    }
}

/// Render a [`RuntimeError`] to a localized "errror prefix: detail"
/// line.
///
/// Pure helper so unit tests can pin the catalog text per locale
/// without going through the higher-level `CliError` wrapper.
pub fn render_runtime_error(err: &RuntimeError, locale: Locale, i18n: &dyn I18nPort) -> String {
    let prefix = i18n.resolve(locale, k::CLI_RUNTIME_ERROR_PREFIX);
    let body = render_runtime_error_body(err, locale, i18n);
    if body.is_empty() {
        prefix
    } else {
        format!("{prefix}: {body}")
    }
}

fn render_runtime_error_body(err: &RuntimeError, locale: Locale, i18n: &dyn I18nPort) -> String {
    match err {
        RuntimeError::BackendUnavailable { reason, detail } => {
            render_backend_unavailable(reason, detail, locale, i18n)
        }
        RuntimeError::WorkspaceNotFound { workspace } => {
            i18n.resolve_with_args(locale, k::CLI_RUNTIME_WORKSPACE_NOT_FOUND, &[workspace])
        }
        RuntimeError::WorkspaceAlreadyExists { workspace } => i18n.resolve_with_args(
            locale,
            k::CLI_RUNTIME_WORKSPACE_ALREADY_EXISTS,
            &[workspace],
        ),
        RuntimeError::UnitNotFound { id } => {
            i18n.resolve_with_args(locale, k::CLI_RUNTIME_UNIT_NOT_FOUND, &[id])
        }
        RuntimeError::LastUnitCloseRejected => {
            i18n.resolve(locale, k::CLI_RUNTIME_LAST_UNIT_CLOSE_REJECTED)
        }
        RuntimeError::OperationFailed { message } => {
            i18n.resolve_with_args(locale, k::CLI_RUNTIME_OPERATION_FAILED, &[message])
        }
        RuntimeError::PermissionDenied { detail } => {
            i18n.resolve_with_args(locale, k::CLI_RUNTIME_PERMISSION_DENIED, &[detail])
        }
    }
}

fn render_backend_unavailable(
    reason: &BackendUnavailableReason,
    detail: &str,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> String {
    match reason {
        BackendUnavailableReason::Other => {
            // No typed reason — log the raw detail at debug so we
            // can spot adapters that should be reporting a typed
            // reason, then surface the localized generic message.
            tracing::debug!(
                detail = %detail,
                "render_runtime_error: BackendUnavailable without typed reason; \
                 falling back to generic localized message"
            );
            i18n.resolve(locale, k::CLI_RUNTIME_BACKEND_UNAVAILABLE_GENERIC)
        }
        other => {
            let args: Vec<String> = other.i18n_args();
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            i18n.resolve_with_args(locale, other.i18n_key(), &arg_refs)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for the typed CLI error renderer.
    //!
    //! These tests pin two localization contracts:
    //!
    //! 1. Every `BackendUnavailable` variant the adapter mappers
    //!    produce gets translated through the i18n catalog. The
    //!    English diagnostic strings (`runtime backend not
    //!    available`, `tmux socket not found`) MUST NOT appear in
    //!    rendered zh-CN output.
    //! 2. The renderer carries the typed context through to the
    //!    user (server name, socket path, etc.) so the localized
    //!    text is still actionable.

    use super::*;
    use workhive_core::ports::BackendUnavailableReason;
    use workhive_infra::i18n::StaticI18n;

    fn i18n() -> StaticI18n {
        StaticI18n::new()
    }

    #[test]
    fn zh_cn_socket_not_found_does_not_leak_english_phrase() {
        let err = RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound {
                path: "/tmp/wh-srv".to_owned(),
            },
            detail: "tmux socket not found: /tmp/wh-srv".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::ZhCn, &i18n());
        assert!(
            !rendered
                .to_lowercase()
                .contains("runtime backend not available"),
            "zh-CN render must not leak English diagnostic phrase: {rendered}"
        );
        assert!(
            !rendered.to_lowercase().contains("tmux socket not found"),
            "zh-CN render must not leak English `tmux socket not found`: {rendered}"
        );
        assert!(
            rendered.contains("/tmp/wh-srv"),
            "zh-CN render must still surface the socket path: {rendered}"
        );
    }

    #[test]
    fn zh_cn_server_not_running_does_not_leak_english_phrase() {
        let err = RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::ServerNotRunning {
                server: "whsrv-test".to_owned(),
            },
            detail: "tmux server not running: whsrv-test".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::ZhCn, &i18n());
        assert!(
            !rendered
                .to_lowercase()
                .contains("runtime backend not available"),
            "zh-CN render must not leak English diagnostic phrase: {rendered}"
        );
        assert!(
            !rendered.to_lowercase().contains("tmux server"),
            "zh-CN render must not leak English `tmux server` phrase: {rendered}"
        );
        assert!(
            rendered.contains("whsrv-test"),
            "zh-CN render must surface the server name: {rendered}"
        );
    }

    #[test]
    fn zh_cn_binary_not_found_does_not_leak_english_phrase() {
        let err = RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::BackendBinaryNotFound {
                binary: "tmux".to_owned(),
            },
            detail: "tmux executable not found".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::ZhCn, &i18n());
        assert!(
            !rendered.to_lowercase().contains("not found"),
            "zh-CN render must not leak the English `not found` phrase: {rendered}"
        );
        assert!(
            !rendered.to_lowercase().contains("backend not available"),
            "zh-CN render must not leak English `backend not available`: {rendered}"
        );
        assert!(
            rendered.contains("tmux"),
            "zh-CN render must still name the missing binary: {rendered}"
        );
    }

    #[test]
    fn zh_cn_other_reason_falls_back_to_localized_generic_text() {
        let err = RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::Other,
            detail: "tmux socket not found: /tmp/whatever".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::ZhCn, &i18n());
        // Even when an adapter forgot to set a typed reason, the
        // CLI boundary refuses to print the raw English `detail`
        // and emits the localized generic line instead.
        assert!(
            !rendered.to_lowercase().contains("tmux socket not found"),
            "fallback must not echo raw English detail to the user: {rendered}"
        );
        assert!(
            !rendered
                .to_lowercase()
                .contains("runtime backend not available"),
            "fallback must not echo raw English Display: {rendered}"
        );
    }

    #[test]
    fn en_locale_renders_through_catalog_and_surfaces_arguments() {
        let err = RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound {
                path: "/tmp/wh-srv".to_owned(),
            },
            detail: "diagnostic".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::En, &i18n());
        assert!(rendered.contains("/tmp/wh-srv"));
        // English catalog says "Runtime backend socket `{0}` was
        // not found." We don't pin the exact wording (so the
        // catalog can be edited freely) — only that the path
        // round-trips.
        assert!(
            rendered.contains("Runtime"),
            "English render must use the catalog prefix: {rendered}"
        );
    }

    #[test]
    fn render_localized_error_returns_none_for_unsupported_variant() {
        let err = CliError::UserCancelled;
        assert!(render_localized_error(&err, Locale::ZhCn, &i18n()).is_none());
    }

    #[test]
    fn render_localized_error_translates_runtime_op() {
        let err = CliError::RuntimeOp(RuntimeError::BackendUnavailable {
            reason: BackendUnavailableReason::SocketNotFound {
                path: "/tmp/wh-srv".to_owned(),
            },
            detail: "diagnostic".to_owned(),
        });
        let rendered = render_localized_error(&err, Locale::ZhCn, &i18n())
            .expect("typed RuntimeOp must be localized");
        assert!(
            !rendered.to_lowercase().contains("tmux socket not found"),
            "zh-CN render of CliError::RuntimeOp must not leak English: {rendered}"
        );
    }

    #[test]
    fn workspace_not_found_renders_workspace_argument_in_zh_cn() {
        let err = RuntimeError::WorkspaceNotFound {
            workspace: "feature-x".to_owned(),
        };
        let rendered = render_runtime_error(&err, Locale::ZhCn, &i18n());
        assert!(
            rendered.contains("feature-x"),
            "workspace name must round-trip into the localized message: {rendered}"
        );
        assert!(
            !rendered.to_lowercase().contains("session not found"),
            "raw English Display must not leak: {rendered}"
        );
    }

    #[test]
    fn render_localized_error_translates_not_in_workspace_session_zh_cn() {
        let err = CliError::Environment(EnvironmentError::NotInWorkspaceSession);
        let rendered = render_localized_error(&err, Locale::ZhCn, &i18n())
            .expect("NotInWorkspaceSession must be localized at the CLI boundary");
        assert!(
            !rendered
                .to_lowercase()
                .contains("not inside a workhive-managed tmux session"),
            "zh-CN render must not echo the raw core English Display: {rendered}"
        );
        assert!(
            rendered.contains("工作区"),
            "zh-CN render must be Chinese and name the workspace concept: {rendered}"
        );
        assert!(
            rendered.contains("workhive workspace open")
                && rendered.contains("workhive workspace attach"),
            "zh-CN render must surface the recovery commands: {rendered}"
        );
    }

    #[test]
    fn render_localized_error_not_in_workspace_session_en_is_actionable() {
        let err = CliError::Environment(EnvironmentError::NotInWorkspaceSession);
        let rendered = render_localized_error(&err, Locale::En, &i18n())
            .expect("NotInWorkspaceSession must be localized at the CLI boundary");
        assert!(
            rendered.contains("workhive workspace open")
                && rendered.contains("workhive workspace attach"),
            "EN render must surface the recovery commands: {rendered}"
        );
    }

    #[test]
    fn render_localized_error_returns_none_for_other_environment_variants() {
        // Only NotInWorkspaceSession has curated localized copy; the
        // identity-verification family keeps the miette fallback so this
        // boundary does not silently widen its localization surface.
        let err = CliError::Environment(EnvironmentError::SessionIdentityUnverifiable {
            session_name: "wh@x".to_owned(),
        });
        assert!(render_localized_error(&err, Locale::ZhCn, &i18n()).is_none());
    }
}

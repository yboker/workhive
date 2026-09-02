//! Typed error model for the automatic AI-assistant integration
//! preparation lifecycle.
//!
//! Workhive's automatic preparation pass walks each enabled
//! provider's official tool config and applies legacy cleanup +
//! (Codex-only) command-native notify wrapper management. The pass
//! must never silently swallow a real failure: a malformed user
//! `settings.json`, a permission error reading `~/.codex/config.toml`,
//! or a failed atomic write must all surface as a typed
//! [`IntegrationError`] so the caller can decide how to react.
//!
//! ## Categories
//!
//! [`IntegrationError`] currently models only *transient / retryable*
//! failures. All variants classify as
//! [`PrepareErrorKind::Transient`]: the pane supervisor logs
//! `tracing::warn!`, emits a localized "activity tracking may be
//! degraded" message on stderr, and continues spawning the child.
//! Activity rendering later degrades to lower-accuracy sources.
//!
//! Variants:
//!
//! * **`Io`** — A filesystem operation (`read`, `write`, `rename`,
//!   `create_dir_all`, `remove_file`, `metadata`, `copy`, …) failed.
//! * **`ParseJson`** — A JSON config Workhive must inspect (today
//!   that's only Claude's `~/.claude/settings.json` during legacy
//!   cleanup) failed to parse. The cleanup helper fails open so the
//!   user's malformed file is preserved byte-for-byte instead of
//!   being silently overwritten.
//! * **`ParseToml`** — A TOML config (`~/.codex/config.toml`) failed
//!   to parse. Same fail-open guarantee.
//! * **`PathResolution`** — Resolving an OS-level path the lifecycle
//!   needs (`HOME`, `XDG_CONFIG_HOME`, Workhive's global config dir,
//!   …) returned an `io::Error`.
//!
//! [`IntegrationError::classify`] returns the boundary signal
//! [`PrepareErrorKind`] used by the pane supervisor and the
//! opportunistic `init --global` preparation pass.

use std::fmt;
use std::io;

use camino::Utf8PathBuf;
use serde_json::Error as JsonError;
use toml_edit::TomlError;

use workhive_core::domain::{AiActivityProvider, Locale};
use workhive_core::i18n_keys::AUTO_PREPARE_FAILED;
use workhive_core::ports::I18nPort;

/// Classification surface consumed by the lifecycle callers
/// (pane supervisor + `init --global`).
///
/// Today every [`IntegrationError`] is transient: the lifecycle no
/// longer installs provider hook tables, hook scripts, or plugin
/// files, so there is no path that overwrites user-authored material.
/// The enum is kept as a single-variant boundary type because callers
/// already match on it; future safety-refusal categories would re-enter
/// here as additional variants without changing the call site shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrepareErrorKind {
    /// Transient or retryable I/O / parse failure. Caller may fail
    /// open with a localized warning, but must not claim the
    /// integration is active.
    Transient,
}

/// Typed failure for one provider's integration preparation pass.
///
/// Constructors carry full structural context (`path`, `context`,
/// `source`) so the supervisor can render a localized actionable
/// message and `tracing::warn!` can record the underlying cause.
#[derive(Debug)]
pub(crate) enum IntegrationError {
    /// A filesystem operation failed (read/write/chmod/rename/copy
    /// / create_dir_all / metadata / remove_file). The `path` is the
    /// affected target and `context` is a stable, untranslated keyword
    /// used in traces (e.g. `"read"`, `"atomic_write"`, `"backup"`,
    /// `"remove"`).
    Io {
        context: &'static str,
        path: Utf8PathBuf,
        source: io::Error,
    },
    /// JSON config (e.g. `~/.claude/settings.json`) failed to parse.
    /// The cleanup helper preserves the user's bytes instead of
    /// overwriting them.
    ParseJson {
        path: Utf8PathBuf,
        source: JsonError,
    },
    /// TOML config (e.g. `~/.codex/config.toml`) failed to parse.
    /// Same byte-preserving guarantee as `ParseJson`.
    ParseToml {
        path: Utf8PathBuf,
        source: TomlError,
    },
    /// Resolving a Workhive path failed (e.g. global config dir
    /// unavailable). The resolver itself uses `std::env::var` and
    /// `std::fs::create_dir_all` under the hood.
    PathResolution {
        context: &'static str,
        source: io::Error,
    },
}

impl IntegrationError {
    /// Return the [`PrepareErrorKind`] that drives the caller's
    /// fail-open vs fail-closed decision.
    pub(crate) fn classify(&self) -> PrepareErrorKind {
        match self {
            IntegrationError::Io { .. }
            | IntegrationError::ParseJson { .. }
            | IntegrationError::ParseToml { .. }
            | IntegrationError::PathResolution { .. } => PrepareErrorKind::Transient,
        }
    }

    /// Render a localized user-facing message for this failure.
    ///
    /// All current variants resolve the generic `AUTO_PREPARE_FAILED`
    /// template with `{0}` = provider wire name and `{1}` = reason.
    /// The reason is the [`Self::user_reason`] string — paths + io
    /// error message, never an internal type name.
    pub(crate) fn localized_message(
        &self,
        i18n: &dyn I18nPort,
        locale: Locale,
        provider: AiActivityProvider,
    ) -> String {
        let reason = self.user_reason();
        i18n.resolve_with_args(
            locale,
            AUTO_PREPARE_FAILED,
            &[provider.wire_name(), reason.as_str()],
        )
    }

    /// Short human-readable reason for diagnostic rendering. The
    /// string is stable enough for tests to assert on, but not
    /// translated — translations are handled by
    /// [`Self::localized_message`].
    fn user_reason(&self) -> String {
        match self {
            IntegrationError::Io {
                context,
                path,
                source,
            } => format!("{context} {path}: {source}"),
            IntegrationError::ParseJson { path, source } => {
                format!("parse json at {path}: {source}")
            }
            IntegrationError::ParseToml { path, source } => {
                format!("parse toml at {path}: {source}")
            }
            IntegrationError::PathResolution { context, source } => {
                format!("{context}: {source}")
            }
        }
    }
}

impl fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.user_reason())
    }
}

impl std::error::Error for IntegrationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IntegrationError::Io { source, .. } => Some(source),
            IntegrationError::ParseJson { source, .. } => Some(source),
            IntegrationError::ParseToml { source, .. } => Some(source),
            IntegrationError::PathResolution { source, .. } => Some(source),
        }
    }
}

/// Construct an [`IntegrationError::Io`] error for a filesystem
/// operation against `path` with an untranslated `context` keyword.
pub(super) fn io_err(
    context: &'static str,
    path: impl Into<Utf8PathBuf>,
    source: io::Error,
) -> IntegrationError {
    IntegrationError::Io {
        context,
        path: path.into(),
        source,
    }
}

/// Construct an [`IntegrationError::PathResolution`] error.
pub(super) fn path_err(context: &'static str, source: io::Error) -> IntegrationError {
    IntegrationError::PathResolution { context, source }
}

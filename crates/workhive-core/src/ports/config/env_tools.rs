#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// Tool catalog configuration — serializable wrapper around [`ToolCatalog`].
///
/// Stored in `[tools]` section of config TOML.  The `catalog` field holds
/// the actual tool entries.  When `None` in a project config, the global
/// catalog (or builtin defaults) is used.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCatalogConfig {
    /// The tool entries managed by the user.
    #[serde(default)]
    pub catalog: ToolCatalog,
}

impl Default for ToolCatalogConfig {
    fn default() -> Self {
        Self {
            catalog: ToolCatalog::builtin_defaults(),
        }
    }
}

/// Why a single line in a `.env` overlay was rejected during parsing.
///
/// Mirrors `workhive_core::usecases::env::EnvParseReason` at the port
/// boundary so the CLI can render localized warnings without depending
/// on the `usecases` layer for boundary types. Each variant maps 1:1 to
/// a localized `OPEN_ENV_PARSE_REASON_*` key — see
/// `EnvLineIssueKind::i18n_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvLineIssueKind {
    /// Line uses YAML-style `KEY: VALUE`.
    LooksLikeYaml,
    /// Line has no `=` separator and is not a comment / blank.
    MissingEquals,
    /// Line has an empty key (e.g. `=foo`).
    EmptyKey,
    /// Key contains characters that are not valid in env var names.
    InvalidKeyChars,
}

impl EnvLineIssueKind {
    /// Localized i18n key the CLI should pass to its i18n resolver as
    /// the third argument to `OPEN_ENV_PARSE_WARNING`. The user-facing
    /// warning includes the source path, line
    /// number, and a typed reason rendered through this mapping —
    /// never the raw line and never the value.
    pub fn i18n_key(self) -> &'static str {
        use crate::i18n_keys as k;
        match self {
            Self::LooksLikeYaml => k::OPEN_ENV_PARSE_REASON_LOOKS_LIKE_YAML,
            Self::MissingEquals => k::OPEN_ENV_PARSE_REASON_MISSING_EQUALS,
            Self::EmptyKey => k::OPEN_ENV_PARSE_REASON_EMPTY_KEY,
            Self::InvalidKeyChars => k::OPEN_ENV_PARSE_REASON_INVALID_KEY_CHARS,
        }
    }
}

/// One invalid line from a `.env` overlay surfaced to user-visible
/// command output.
///
/// Carries the 1-based line number and a typed reason. Deliberately
/// does NOT carry the raw line text or the value side: echoing either
/// to stderr or logs is forbidden because
/// the right-hand side may include a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvLineIssue {
    /// 1-based line number in the source file.
    pub line: u32,
    /// Why the line was rejected.
    pub kind: EnvLineIssueKind,
}

/// Result of loading a `.env` overlay through [`ConfigPort::load_env_report`].
///
/// Adds the resolved file path and parser issues alongside the
/// accepted `KEY=VALUE` entries so the CLI can:
/// - display source path + line + typed reason for every malformed line;
/// - skip user-visible warnings entirely when the file is missing
///   (`path == None`) or syntactically clean (`issues.is_empty()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvLoadReport {
    /// Resolved on-disk path. `None` when the overlay file does not
    /// exist (`workspace open` always tolerates a missing `.env`).
    pub path: Option<Utf8PathBuf>,
    /// Successfully parsed entries (preserves source order, last-wins
    /// semantics on duplicates handled later by the merger).
    pub entries: Vec<(String, String)>,
    /// Lines that violated the `.env` syntax contract.
    pub issues: Vec<EnvLineIssue>,
}

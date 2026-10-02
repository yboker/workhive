//! Workspace identity — typed records separating runtime and display concerns.
//!
//! Runtime identity uses the concrete backend binding. Display identity preserves
//! human-readable project/workspace names.

use camino::Utf8PathBuf;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RelativeProjectPath;

/// Current persisted runtime-binding schema.
pub const RUNTIME_BINDING_SCHEMA_VERSION: u32 = 1;

/// Typed runtime backend identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeBackendId(String);

impl RuntimeBackendId {
    /// Built-in tmux runtime backend identifier.
    pub fn tmux() -> Self {
        Self("tmux".to_owned())
    }

    /// Built-in Herdr runtime backend identifier.
    pub fn herdr() -> Self {
        Self("herdr".to_owned())
    }

    /// Construct a runtime backend identifier from a non-empty string.
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        if value.trim().is_empty() {
            None
        } else {
            Some(Self(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuntimeBackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Persisted association between a logical workspace and its live runtime.
/// Backend identifiers are opaque to core and owned by the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeBinding {
    pub schema_version: u32,
    pub backend: RuntimeBackendId,
    pub scope_id: String,
    pub workspace_id: String,
}

impl RuntimeBinding {
    pub fn new(
        backend: RuntimeBackendId,
        scope_id: impl Into<String>,
        workspace_id: impl Into<String>,
    ) -> Option<Self> {
        let scope_id = scope_id.into();
        let workspace_id = workspace_id.into();
        if scope_id.trim().is_empty() || workspace_id.trim().is_empty() {
            return None;
        }
        Some(Self {
            schema_version: RUNTIME_BINDING_SCHEMA_VERSION,
            backend,
            scope_id,
            workspace_id,
        })
    }

    pub fn tmux(scope_id: impl Into<String>, workspace_id: impl Into<String>) -> Self {
        Self::new(RuntimeBackendId::tmux(), scope_id, workspace_id)
            .expect("tmux runtime binding identifiers must not be empty")
    }
}

impl<'de> serde::Deserialize<'de> for RuntimeBinding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            schema_version: u32,
            backend: RuntimeBackendId,
            scope_id: String,
            workspace_id: String,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.schema_version != RUNTIME_BINDING_SCHEMA_VERSION {
            return Err(serde::de::Error::custom(format!(
                "unsupported runtime binding schema version {}",
                raw.schema_version
            )));
        }
        Self::new(raw.backend, raw.scope_id, raw.workspace_id).ok_or_else(|| {
            serde::de::Error::custom("runtime binding identifiers must not be empty")
        })
    }
}

/// Project-level identity invariants.
///
/// One per initialized project. Stored at `.workhive/identity/project.json`.
/// Fields here do not change per-workspace; they are shared across all
/// workspaces within the same project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectIdentityRecord {
    /// 4-hex random instance ID generated at `init`.
    pub instance_id: String,
    /// Absolute path to the project root directory.
    pub project_root: Utf8PathBuf,
    /// Human-readable project display name (unsanitized original).
    pub project_display: String,
    /// Sanitized project component used in canonical naming.
    pub canonical_project_component: String,
}

/// Typed composite key for workspace identity lookup and persistence.
///
/// Uniquely identifies a workspace within a project by combining
/// the workspace name with its subdirectory context.
/// Two workspaces with the same name but in different subdirectories
/// produce different keys.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceIdentityKey {
    /// Logical workspace name (the `WorkspaceId` value).
    pub workspace: String,
    /// Validated relative path from project root.
    /// `None` for repo-root workspaces.
    pub relative_project_path: Option<RelativeProjectPath>,
}

impl WorkspaceIdentityKey {
    pub fn new(
        workspace: impl Into<String>,
        relative_project_path: Option<RelativeProjectPath>,
    ) -> Self {
        Self {
            workspace: workspace.into(),
            relative_project_path,
        }
    }

    /// File-safe string form for persistence path naming.
    ///
    /// Encoded as `[v2]-<readable-prefix>-<sha256-128-hex>` via the shared
    /// persistence codec. The readable prefix is derived from
    /// `{path}__{workspace}` where `path` is empty for project root.
    pub fn to_file_stem(&self) -> String {
        let path = self
            .relative_project_path
            .as_ref()
            .map(|p| p.as_str())
            .unwrap_or("");
        let readable_input = format!("{path}__{}", self.workspace);
        persistence_file_key_codec(&readable_input, &[path, &self.workspace], 250)
    }
}

impl std::fmt::Display for WorkspaceIdentityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.relative_project_path.as_ref() {
            None => write!(f, "{}", self.workspace),
            Some(path) => write!(f, "{}:{}", path.as_str(), self.workspace),
        }
    }
}

/// `Preparing` keeps the active binding authoritative while recording the
/// staged target. `Promoting` makes the staged tmux identity authoritative
/// while recording the final name and the previous runtime, so a crash between
/// retirement and rename can resume without guessing. `RegisteringCheckout`
/// records a target whose backend-native checkout association must complete
/// after retiring the previous runtime. `Cleaning` records only old-runtime
/// cleanup after the active binding has switched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum RuntimeReplacement {
    Preparing {
        target: RuntimeBinding,
    },
    Promoting {
        staged: RuntimeBinding,
        final_binding: RuntimeBinding,
        previous: RuntimeBinding,
    },
    RegisteringCheckout {
        previous: RuntimeBinding,
    },
    Cleaning {
        previous: RuntimeBinding,
    },
}

impl RuntimeReplacement {
    /// Binding to restore when rolling back an in-flight replacement.
    pub fn into_previous(self) -> Option<RuntimeBinding> {
        match self {
            Self::Preparing { .. } => None,
            Self::Promoting { previous, .. }
            | Self::RegisteringCheckout { previous }
            | Self::Cleaning { previous } => Some(previous),
        }
    }
}

/// Per-workspace identity record.
///
/// One per logical workspace within a project. Stored at
/// `.workhive/identity/workspaces/<file-stem>.json` where `<file-stem>` is a
/// canonical encoded component derived from the typed workspace identity.
///
/// Contains both display (human) and canonical (runtime) identities,
/// persisted so that no reverse-parsing of sanitized tmux names is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceIdentityRecord {
    /// Logical workspace name (the `WorkspaceId` value).
    pub workspace: String,
    /// Validated relative path from project root.
    /// `None` (explicit null) for repo-root workspaces.
    /// Missing field is a deserialization error.
    pub relative_project_path: Option<RelativeProjectPath>,
    /// Human-readable relative display path for subdirectory workspaces.
    /// `None` for project-root provenance (current checked-out branch).
    pub relative_display_path: Option<String>,
    /// Concrete backend and opaque runtime identifiers persisted on disk.
    pub runtime_binding: RuntimeBinding,
    /// In-progress replacement state used for crash-safe compensation.
    pub runtime_replacement: Option<RuntimeReplacement>,
    /// Sanitized workspace component used in canonical naming.
    pub canonical_workspace_component: String,
    /// Canonical tmux session name retained for in-memory compatibility.
    pub canonical_session_name: String,
    /// Canonical tmux server name retained for in-memory compatibility.
    pub canonical_server_name: String,
}

impl serde::Serialize for WorkspaceIdentityRecord {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(serde::Serialize)]
        struct Persisted<'a> {
            workspace: &'a str,
            relative_project_path: &'a Option<RelativeProjectPath>,
            #[serde(skip_serializing_if = "Option::is_none")]
            relative_display_path: &'a Option<String>,
            canonical_workspace_component: &'a str,
            runtime_binding: RuntimeBinding,
            #[serde(skip_serializing_if = "Option::is_none")]
            runtime_replacement: &'a Option<RuntimeReplacement>,
        }
        Persisted {
            workspace: &self.workspace,
            relative_project_path: &self.relative_project_path,
            relative_display_path: &self.relative_display_path,
            canonical_workspace_component: &self.canonical_workspace_component,
            runtime_binding: self.runtime_binding.clone(),
            runtime_replacement: &self.runtime_replacement,
        }
        .serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for WorkspaceIdentityRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            workspace: String,
            relative_project_path: serde_json::Value,
            #[serde(default)]
            relative_display_path: Option<String>,
            canonical_workspace_component: String,
            #[serde(default)]
            runtime_binding: Option<RuntimeBinding>,
            #[serde(default)]
            runtime_replacement: Option<RuntimeReplacement>,
            #[serde(default)]
            canonical_session_name: Option<String>,
            #[serde(default)]
            canonical_server_name: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let relative_project_path = if raw.relative_project_path.is_null() {
            None
        } else {
            Some(
                serde_json::from_value::<RelativeProjectPath>(raw.relative_project_path)
                    .map_err(serde::de::Error::custom)?,
            )
        };
        let runtime_binding = match raw.runtime_binding {
            Some(binding) => binding,
            None => {
                let session = raw.canonical_session_name.ok_or_else(|| {
                    serde::de::Error::custom("missing runtime_binding and canonical_session_name")
                })?;
                let server = raw.canonical_server_name.ok_or_else(|| {
                    serde::de::Error::custom("missing runtime_binding and canonical_server_name")
                })?;
                RuntimeBinding::tmux(server, session)
            }
        };
        let canonical_session_name = runtime_binding.workspace_id.clone();
        let canonical_server_name = runtime_binding.scope_id.clone();
        Ok(Self {
            workspace: raw.workspace,
            relative_project_path,
            relative_display_path: raw.relative_display_path,
            canonical_workspace_component: raw.canonical_workspace_component,
            runtime_binding,
            runtime_replacement: raw.runtime_replacement,
            canonical_session_name,
            canonical_server_name,
        })
    }
}
impl WorkspaceIdentityRecord {
    pub fn runtime_binding(&self) -> RuntimeBinding {
        self.runtime_binding.clone()
    }

    pub fn identity_key(&self) -> WorkspaceIdentityKey {
        WorkspaceIdentityKey {
            workspace: self.workspace.clone(),
            relative_project_path: self.relative_project_path.clone(),
        }
    }
}

// ── Shared persistence-key codec ──

const CODEC_DOMAIN_SEPARATOR: &[u8] = b"workhive:persistence-key:v2\0";
const CODEC_VERSION_PREFIX: &str = "[v2]-";
const CODEC_READABLE_PREFIX_MAX: usize = 48;

/// Deterministic bounded persistence-key codec shared by identity and
/// snapshot file keys.
///
/// Always returns the canonical versioned format:
/// `[v2]-<readable-prefix>-<sha256-128-hex>`.
///
/// The readable prefix is derived from `legacy` for human recognition.
/// The digest is computed from the framed `tuple` components.
fn persistence_file_key_codec(legacy: &str, tuple: &[&str], _max_len: usize) -> String {
    let readable = build_readable_prefix(legacy);
    let digest_hex = compute_tuple_digest_hex(tuple);
    format!("{CODEC_VERSION_PREFIX}{readable}-{digest_hex}")
}

fn build_readable_prefix(legacy: &str) -> String {
    let mut buf = String::with_capacity(legacy.len());
    for ch in legacy.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '~') {
            buf.push(ch);
        } else if ch == '/' || ch == '\\' {
            buf.push('~');
        } else {
            buf.push('_');
        }
    }

    // Collapse repeated '_'
    let mut collapsed = String::with_capacity(buf.len());
    let mut prev_underscore = false;
    for ch in buf.chars() {
        if ch == '_' {
            if !prev_underscore {
                collapsed.push('_');
            }
            prev_underscore = true;
        } else {
            collapsed.push(ch);
            prev_underscore = false;
        }
    }

    // Trim leading/trailing '_', '.', '-', '~'
    let trimmed = collapsed
        .trim_start_matches(['_', '.', '-', '~'])
        .trim_end_matches(['_', '.', '-', '~']);

    let result = if trimmed.is_empty() {
        "key".to_owned()
    } else {
        trimmed.to_owned()
    };

    truncate_utf8(&result, CODEC_READABLE_PREFIX_MAX)
}

/// Truncate a string to at most `max_bytes` without splitting a UTF-8
/// codepoint.
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn compute_tuple_digest_hex(tuple: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CODEC_DOMAIN_SEPARATOR);
    for component in tuple {
        let len_bytes = (component.len() as u64).to_le_bytes();
        hasher.update(len_bytes);
        hasher.update(component.as_bytes());
    }
    let full_digest = hasher.finalize();
    let first_16 = &full_digest[..16];
    let mut hex = String::with_capacity(32);
    for byte in first_16 {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Errors from workspace identity operations.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum IdentityError {
    #[error("project identity not found at {path} — project may not be initialized")]
    #[diagnostic(
        code(workhive::identity::project_identity_missing),
        help(
            "The project may not be initialized. Run `workhive init` from the project root directory."
        )
    )]
    ProjectIdentityMissing { path: Utf8PathBuf },

    #[error("workspace identity not found for '{workspace}' at {path}")]
    #[diagnostic(
        code(workhive::identity::workspace_identity_missing),
        help(
            "Workspace identity record is missing. Run `workhive doctor` to check project status."
        )
    )]
    WorkspaceIdentityMissing {
        workspace: String,
        path: Utf8PathBuf,
    },

    #[error(
        "project identity conflict: stored project_root '{stored}' \
         does not match current '{current}'"
    )]
    #[diagnostic(
        code(workhive::identity::project_root_mismatch),
        help(
            "The project directory may have been moved or renamed. Run `workhive doctor --fix` in the current directory to repair the stored root, then `workhive project reindex` to refresh the global project index, or cd back to the original directory {stored}."
        )
    )]
    ProjectRootConflict {
        stored: Utf8PathBuf,
        current: Utf8PathBuf,
    },

    #[error(
        "instance_id conflict: stored '{stored}' does not match current '{current}' \
         for project at {project_root}"
    )]
    #[diagnostic(
        code(workhive::identity::instance_id_conflict),
        help(
            "The identity file may be corrupted or manually edited. Run `workhive doctor` to diagnose and repair."
        )
    )]
    InstanceIdConflict {
        stored: String,
        current: String,
        project_root: Utf8PathBuf,
    },

    #[error("failed to read identity metadata at {path}")]
    #[diagnostic(code(workhive::identity::read_failed))]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write identity metadata to {path}")]
    #[diagnostic(code(workhive::identity::write_failed))]
    WriteFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("corrupted identity metadata at {path}: {reason}")]
    #[diagnostic(
        code(workhive::identity::corrupted),
        help(
            "The identity file is corrupted. Run `workhive doctor` to diagnose, or restore .workhive/identity/project.json from backup."
        )
    )]
    Corrupted { path: Utf8PathBuf, reason: String },

    #[error("failed to create identity directory at {path}")]
    #[diagnostic(code(workhive::identity::dir_create_failed))]
    DirCreateFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "project display name conflict: stored '{stored}' \
         does not match current '{current}'"
    )]
    #[diagnostic(
        code(workhive::identity::project_display_conflict),
        help("The project display name has changed. Run `workhive init` to update.")
    )]
    ProjectDisplayConflict { stored: String, current: String },

    #[error(
        "canonical project component conflict: stored '{stored}' \
         does not match current '{current}'"
    )]
    #[diagnostic(
        code(workhive::identity::canonical_project_conflict),
        help("The canonical project name has changed. Run `workhive init` to update.")
    )]
    CanonicalProjectConflict { stored: String, current: String },

    #[error(
        "no identity record matches workspace '{workspace}', \
         session '{session}', server '{server}'"
    )]
    #[diagnostic(
        code(workhive::identity::not_found),
        help(
            "No matching identity record was found. Verify the workspace, session, and server names are correct."
        )
    )]
    IdentityNotFound {
        workspace: String,
        session: String,
        server: String,
    },

    #[error(
        "ambiguous identity: {count} records match workspace '{workspace}', \
         session '{session}', server '{server}'"
    )]
    #[diagnostic(
        code(workhive::identity::ambiguous),
        help(
            "Multiple identity records match the query. Run `workhive doctor` to inspect and deduplicate."
        )
    )]
    AmbiguousIdentity {
        workspace: String,
        session: String,
        server: String,
        count: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_project() -> ProjectIdentityRecord {
        ProjectIdentityRecord {
            instance_id: "a1b2".to_owned(),
            project_root: Utf8PathBuf::from("/Users/dev/my-project"),
            project_display: "my-project".to_owned(),
            canonical_project_component: "my-project".to_owned(),
        }
    }

    fn sample_workspace_main() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                "wh@a1b2@my-project@main",
            ),
            runtime_replacement: None,
            canonical_session_name: "wh@a1b2@my-project@main".to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    fn sample_workspace_subdir() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "feature-auth".to_owned(),
            relative_project_path: RelativeProjectPath::new("subdir").ok(),
            relative_display_path: Some("subdir/feature-auth".to_owned()),
            canonical_workspace_component: "feature-auth".to_owned(),
            runtime_binding: RuntimeBinding::tmux(
                "whsrv-a1b2-my-project",
                "wh@a1b2@my-project@feature-auth",
            ),
            runtime_replacement: None,
            canonical_session_name: "wh@a1b2@my-project@feature-auth".to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    // ── ProjectIdentityRecord ──

    #[test]
    fn project_identity_roundtrip_serde() {
        let record = sample_project();
        let json = serde_json::to_string(&record).expect("serialize");
        let parsed: ProjectIdentityRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, record);
    }

    // ── WorkspaceIdentityRecord ──

    #[test]
    fn workspace_identity_roundtrip_serde() {
        let record = sample_workspace_main();
        let json = serde_json::to_string(&record).expect("serialize");
        let parsed: WorkspaceIdentityRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, record);
    }

    #[test]
    fn runtime_replacement_roundtrips_all_durable_phases() {
        let target = RuntimeBinding::tmux("srv", "target");
        let final_binding = RuntimeBinding::tmux("srv", "final");
        let previous = RuntimeBinding::tmux("srv", "previous");
        for replacement in [
            RuntimeReplacement::Preparing {
                target: target.clone(),
            },
            RuntimeReplacement::Promoting {
                staged: target.clone(),
                final_binding: final_binding.clone(),
                previous: previous.clone(),
            },
            RuntimeReplacement::RegisteringCheckout {
                previous: previous.clone(),
            },
            RuntimeReplacement::Cleaning {
                previous: previous.clone(),
            },
        ] {
            let mut record = sample_workspace_main();
            record.runtime_replacement = Some(replacement);
            let json = serde_json::to_string(&record).expect("serialize");
            let parsed: WorkspaceIdentityRecord = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, record);
        }
    }

    #[test]
    fn workspace_identity_main_has_no_relative_path() {
        let ws = sample_workspace_main();
        assert!(ws.relative_display_path.is_none());
        assert!(ws.relative_project_path.is_none());
    }

    #[test]
    fn workspace_identity_subdir_has_relative_path() {
        let ws = sample_workspace_subdir();
        assert_eq!(
            ws.relative_display_path.as_deref(),
            Some("subdir/feature-auth")
        );
        assert_eq!(
            ws.relative_project_path.as_ref().map(|p| p.as_str()),
            Some("subdir")
        );
    }

    // ── Sanitized project with special chars ──

    #[test]
    fn identity_preserves_display_while_canonical_is_sanitized() {
        let proj = ProjectIdentityRecord {
            instance_id: "c3d4".to_owned(),
            project_root: Utf8PathBuf::from("/Users/dev/python/1.0pingtai-with-laopingta"),
            project_display: "python/1.0pingtai-with-laopingta".to_owned(),
            canonical_project_component: "python/1_0pingtai-with-laopingta".to_owned(),
        };
        assert_ne!(proj.project_display, proj.canonical_project_component);
        assert!(proj.project_display.contains('.'));
        assert!(!proj.canonical_project_component.contains('.'));
    }

    // ── IdentityError ──

    #[test]
    fn identity_error_variants_carry_context() {
        let err = IdentityError::ProjectRootConflict {
            stored: Utf8PathBuf::from("/old/path"),
            current: Utf8PathBuf::from("/new/path"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("/old/path"));
        assert!(msg.contains("/new/path"));

        let err2 = IdentityError::InstanceIdConflict {
            stored: "aaaa".to_owned(),
            current: "bbbb".to_owned(),
            project_root: Utf8PathBuf::from("/project"),
        };
        let msg2 = format!("{err2}");
        assert!(msg2.contains("aaaa"));
        assert!(msg2.contains("bbbb"));
    }

    // ── Persistence file-key codec ──

    #[test]
    fn persistence_file_key_golden_vector_slash_workspace() {
        let key = WorkspaceIdentityKey::new("chore/ignore-resources-and-commit", None);
        let stem = key.to_file_stem();
        assert!(
            !stem.contains('/'),
            "file stem must not contain path separator: {stem}"
        );
        assert_eq!(
            stem,
            "[v2]-chore~ignore-resources-and-commit-ca97552a9554be54214a63d8147089c1",
        );
    }

    #[test]
    fn persistence_file_key_slash_differs_from_tilde() {
        let slash = WorkspaceIdentityKey::new("chore/a", None);
        let tilde = WorkspaceIdentityKey::new("chore~a", None);
        assert_ne!(
            slash.to_file_stem(),
            tilde.to_file_stem(),
            "chore/a and chore~a must produce distinct file stems"
        );
    }

    #[test]
    fn persistence_file_key_no_path_separator() {
        for ws in ["fix/login-bug", "feature/auth/login", "a/b/c/d"] {
            let key = WorkspaceIdentityKey::new(ws, None);
            let stem = key.to_file_stem();
            assert!(
                !stem.contains('/') && !stem.contains('\\'),
                "file stem for '{ws}' must not contain path separator: {stem}"
            );
        }
    }

    #[test]
    fn persistence_file_key_reserved_prefix_not_aliased() {
        let key = WorkspaceIdentityKey::new("rest", RelativeProjectPath::new("[v2]-").ok());
        let stem = key.to_file_stem();
        assert!(
            stem.starts_with("[v2]-"),
            "input whose legacy form starts with [v2]- must be re-encoded: {stem}"
        );
        assert_ne!(stem, "[v2]-__rest");
    }

    #[test]
    fn persistence_file_key_unicode_deterministic() {
        let key = WorkspaceIdentityKey::new("feature/\u{65E5}\u{672C}\u{8A9E}", None);
        let stem1 = key.to_file_stem();
        let stem2 = key.to_file_stem();
        assert_eq!(stem1, stem2, "unicode file stems must be deterministic");
        assert!(
            !stem1.contains('/'),
            "unicode file stem must not contain path separator: {stem1}"
        );
    }

    #[test]
    fn persistence_file_key_oversized_within_limit() {
        let long_name = "a".repeat(300);
        let key = WorkspaceIdentityKey::new(&long_name, None);
        let stem = key.to_file_stem();
        assert!(
            stem.len() <= 250,
            "file stem must stay within 250-byte limit: len={}, stem={stem}",
            stem.len()
        );
    }

    // ── persistence_scope_key_ — canonical format for identity records ──

    // ── workspace_identity_scope_ — identity key model ──

    #[test]
    fn workspace_identity_scope_no_stem_depends_on_main() {
        let main_key = WorkspaceIdentityKey::new("main", None);
        let other_key = WorkspaceIdentityKey::new("develop", None);
        let main_stem = main_key.to_file_stem();
        let other_stem = other_key.to_file_stem();
        assert!(
            main_stem.starts_with("[v2]-"),
            "main stem must use canonical format: {main_stem}"
        );
        assert!(
            other_stem.starts_with("[v2]-"),
            "other stem must use canonical format: {other_stem}"
        );
    }

    #[test]
    fn workspace_identity_scope_slash_workspace_opaque() {
        let key = WorkspaceIdentityKey::new("feature/api/login", None);
        assert_eq!(key.workspace, "feature/api/login");
        let stem = key.to_file_stem();
        assert!(
            stem.starts_with("[v2]-"),
            "slash workspace stem must be canonical: {stem}"
        );
    }

    #[test]
    fn workspace_identity_scope_rejects_old_subdir_key_json() {
        let json = r#"{"workspace":"main","subdir_key":"services/api","canonical_workspace_component":"main","runtime_binding":{"schema_version":1,"backend":"tmux","scope_id":"whsrv-a1b2-proj","workspace_id":"wh@a1b2@proj@main"}}"#;
        let result: Result<WorkspaceIdentityRecord, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "old JSON with subdir_key must be rejected, not silently accepted as root scope"
        );
    }

    // ── workspace_identity_scope_missing_ — serialized field validation ──

    #[test]
    fn workspace_identity_scope_missing_relative_path_field_is_rejected() {
        let json_without_field = r#"{
            "workspace": "main",
            "canonical_workspace_component": "main",
            "runtime_binding": {"schema_version": 1, "backend": "tmux", "scope_id": "whsrv-a1b2-proj", "workspace_id": "wh@a1b2@proj@main"}
        }"#;
        let result: Result<WorkspaceIdentityRecord, _> = serde_json::from_str(json_without_field);
        assert!(
            result.is_err(),
            "missing relative_project_path field must be rejected, not silently accepted as root"
        );

        let json_with_null = r#"{
            "workspace": "main",
            "relative_project_path": null,
            "canonical_workspace_component": "main",
            "runtime_binding": {"schema_version": 1, "backend": "tmux", "scope_id": "whsrv-a1b2-proj", "workspace_id": "wh@a1b2@proj@main"}
        }"#;
        let result: Result<WorkspaceIdentityRecord, _> = serde_json::from_str(json_with_null);
        assert!(
            result.is_ok(),
            "explicit null relative_project_path must remain valid for root scope"
        );
        let record = result.unwrap();
        assert!(record.relative_project_path.is_none());
    }

    #[test]
    fn workspace_identity_scope_three_identities_all_distinct() {
        let root_feat = WorkspaceIdentityKey::new("feature/api/login", None);
        let subdir_main =
            WorkspaceIdentityKey::new("main", RelativeProjectPath::new("services/api").ok());
        let subdir_feat = WorkspaceIdentityKey::new(
            "feature/api/login",
            RelativeProjectPath::new("services/api").ok(),
        );

        assert_ne!(root_feat, subdir_main);
        assert_ne!(root_feat, subdir_feat);
        assert_ne!(subdir_main, subdir_feat);

        let stems: Vec<_> = [&root_feat, &subdir_main, &subdir_feat]
            .iter()
            .map(|k| k.to_file_stem())
            .collect();
        for i in 0..stems.len() {
            for j in (i + 1)..stems.len() {
                assert_ne!(stems[i], stems[j], "stems[{i}] vs stems[{j}]");
            }
        }
    }
}

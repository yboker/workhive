//! Workspace identity — typed records separating runtime, display, and state key concerns.
//!
//! Three distinct identity layers:
//!
//! 1. **Runtime identity**: tmux-safe canonical session/server names (lived in `naming_policy`)
//! 2. **Display identity**: human-readable project/workspace names preserved from original input
//! 3. **State key**: deterministic lookup key for snapshots, derived from typed identity
//!
//! These records are the source of truth for display and state lookup.
//! Runtime naming is always derived from `SessionNamingPolicy` and stored here
//! for consistency, not recomputed ad hoc.

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

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
    /// Sanitized subdirectory offset relative to project root.
    /// Empty string `""` for repo-root workspaces.
    pub subdir_key: String,
}

impl WorkspaceIdentityKey {
    pub fn new(workspace: impl Into<String>, subdir_key: impl Into<String>) -> Self {
        Self {
            workspace: workspace.into(),
            subdir_key: subdir_key.into(),
        }
    }

    /// File-safe string form for persistence path naming.
    /// Format: `{subdir_key}__{workspace}` (empty subdir_key → `__workspace`).
    pub fn to_file_stem(&self) -> String {
        format!("{}__{}", self.subdir_key, self.workspace)
    }
}

impl std::fmt::Display for WorkspaceIdentityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.subdir_key.is_empty() {
            write!(f, "{}", self.workspace)
        } else {
            write!(f, "{}:{}", self.subdir_key, self.workspace)
        }
    }
}

/// Per-workspace identity record.
///
/// One per logical workspace within a project. Stored at
/// `.workhive/identity/workspaces/<subdir_key>__<workspace>.json`.
///
/// Contains both display (human) and canonical (runtime) identities,
/// persisted so that no reverse-parsing of sanitized tmux names is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceIdentityRecord {
    /// Logical workspace name (the `WorkspaceId` value).
    pub workspace: String,
    /// Sanitized subdirectory offset relative to project root.
    /// Empty string for repo-root workspaces.
    #[serde(default)]
    pub subdir_key: String,
    /// Human-readable relative display path for subdirectory workspaces.
    /// `None` for the `main` workspace (project root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_display_path: Option<String>,
    /// Sanitized workspace component used in canonical naming.
    pub canonical_workspace_component: String,
    /// Full canonical tmux session name (e.g. `wh@a1b2@proj@main`).
    pub canonical_session_name: String,
    /// Full canonical tmux server name (e.g. `whsrv-a1b2-proj`).
    pub canonical_server_name: String,
}

impl WorkspaceIdentityRecord {
    /// Derive the typed identity key from this record.
    pub fn identity_key(&self) -> WorkspaceIdentityKey {
        WorkspaceIdentityKey {
            workspace: self.workspace.clone(),
            subdir_key: self.subdir_key.clone(),
        }
    }
}

/// Deterministic lookup key for snapshot state.
///
/// Derived from typed identity, not from string parsing.
/// Stable across reopen/reattach flows.
/// Includes `subdir_key` to distinguish workspaces with the same name
/// in different subdirectories.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceStateKey {
    pub instance_id: String,
    pub project_component: String,
    pub subdir_key: String,
    pub workspace: String,
}

impl WorkspaceStateKey {
    /// Derive from a project identity and workspace identity pair.
    pub fn from_identity(
        project: &ProjectIdentityRecord,
        workspace: &WorkspaceIdentityRecord,
    ) -> Self {
        Self {
            instance_id: project.instance_id.clone(),
            project_component: project.canonical_project_component.clone(),
            subdir_key: workspace.subdir_key.clone(),
            workspace: workspace.workspace.clone(),
        }
    }

    /// Construct directly when identity records are not yet available
    /// (e.g. during migration or legacy fallback).
    pub fn new(
        instance_id: String,
        project_component: String,
        subdir_key: String,
        workspace: String,
    ) -> Self {
        Self {
            instance_id,
            project_component,
            subdir_key,
            workspace,
        }
    }

    /// File-safe string form suitable for snapshot directory naming.
    ///
    /// Format: `{instance_id}_{project_component}_{subdir_key}_{workspace}`
    /// Uses `_` as separator since all components are already sanitized.
    /// Empty subdir_key produces `inst_proj__ws` (double underscore).
    pub fn to_file_key(&self) -> String {
        format!(
            "{}_{}_{}_{}",
            self.instance_id, self.project_component, self.subdir_key, self.workspace
        )
    }

    /// Parse the stable display form generated by [`std::fmt::Display`].
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('@');
        let instance_id = parts.next()?.to_owned();
        let project_component = parts.next()?.to_owned();
        let third = parts.next()?.to_owned();
        let fourth = parts.next().map(str::to_owned);
        if parts.next().is_some() || instance_id.is_empty() || project_component.is_empty() {
            return None;
        }

        match fourth {
            Some(workspace) if !third.is_empty() && !workspace.is_empty() => Some(Self {
                instance_id,
                project_component,
                subdir_key: third,
                workspace,
            }),
            None if !third.is_empty() => Some(Self {
                instance_id,
                project_component,
                subdir_key: String::new(),
                workspace: third,
            }),
            _ => None,
        }
    }
}

impl std::fmt::Display for WorkspaceStateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.subdir_key.is_empty() {
            write!(
                f,
                "{}@{}@{}",
                self.instance_id, self.project_component, self.workspace
            )
        } else {
            write!(
                f,
                "{}@{}@{}@{}",
                self.instance_id, self.project_component, self.subdir_key, self.workspace
            )
        }
    }
}

/// Errors from workspace identity operations.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("project identity not found at {path} — project may not be initialized")]
    ProjectIdentityMissing { path: Utf8PathBuf },

    #[error("workspace identity not found for '{workspace}' at {path}")]
    WorkspaceIdentityMissing {
        workspace: String,
        path: Utf8PathBuf,
    },

    #[error(
        "project identity conflict: stored project_root '{stored}' \
         does not match current '{current}'"
    )]
    ProjectRootConflict {
        stored: Utf8PathBuf,
        current: Utf8PathBuf,
    },

    #[error(
        "instance_id conflict: stored '{stored}' does not match current '{current}' \
         for project at {project_root}"
    )]
    InstanceIdConflict {
        stored: String,
        current: String,
        project_root: Utf8PathBuf,
    },

    #[error("failed to read identity metadata at {path}")]
    ReadFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write identity metadata to {path}")]
    WriteFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("corrupted identity metadata at {path}: {reason}")]
    Corrupted { path: Utf8PathBuf, reason: String },

    #[error("failed to create identity directory at {path}")]
    DirCreateFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "project display name conflict: stored '{stored}' \
         does not match current '{current}'"
    )]
    ProjectDisplayConflict { stored: String, current: String },

    #[error(
        "canonical project component conflict: stored '{stored}' \
         does not match current '{current}'"
    )]
    CanonicalProjectConflict { stored: String, current: String },
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
            subdir_key: String::new(),
            relative_display_path: None,
            canonical_workspace_component: "main".to_owned(),
            canonical_session_name: "wh@a1b2@my-project@main".to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    fn sample_workspace_subdir() -> WorkspaceIdentityRecord {
        WorkspaceIdentityRecord {
            workspace: "feature-auth".to_owned(),
            subdir_key: "subdir".to_owned(),
            relative_display_path: Some("subdir/feature-auth".to_owned()),
            canonical_workspace_component: "feature-auth".to_owned(),
            canonical_session_name: "wh@a1b2@my-project@feature-auth".to_owned(),
            canonical_server_name: "whsrv-a1b2-my-project".to_owned(),
        }
    }

    // ── WorkspaceStateKey ──

    #[test]
    fn state_key_from_identity_is_deterministic() {
        let proj = sample_project();
        let ws = sample_workspace_main();
        let k1 = WorkspaceStateKey::from_identity(&proj, &ws);
        let k2 = WorkspaceStateKey::from_identity(&proj, &ws);
        assert_eq!(k1, k2);
    }

    #[test]
    fn state_key_differs_by_workspace() {
        let proj = sample_project();
        let ws_main = sample_workspace_main();
        let ws_sub = sample_workspace_subdir();
        let k_main = WorkspaceStateKey::from_identity(&proj, &ws_main);
        let k_sub = WorkspaceStateKey::from_identity(&proj, &ws_sub);
        assert_ne!(k_main, k_sub);
    }

    #[test]
    fn state_key_to_file_key_is_filesystem_safe() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            String::new(),
            "main".to_owned(),
        );
        let file_key = key.to_file_key();
        assert_eq!(file_key, "a1b2_my-project__main");
        assert!(!file_key.contains('/'));
        assert!(!file_key.contains('@'));
    }

    #[test]
    fn state_key_to_file_key_with_subdir() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            "subdir-a".to_owned(),
            "feat-x".to_owned(),
        );
        let file_key = key.to_file_key();
        assert_eq!(file_key, "a1b2_my-project_subdir-a_feat-x");
    }

    #[test]
    fn state_key_display_format() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            String::new(),
            "main".to_owned(),
        );
        assert_eq!(format!("{key}"), "a1b2@my-project@main");
    }

    #[test]
    fn state_key_display_format_with_subdir() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            "subdir-a".to_owned(),
            "feat-x".to_owned(),
        );
        assert_eq!(format!("{key}"), "a1b2@my-project@subdir-a@feat-x");
    }

    #[test]
    fn state_key_parse_roundtrip_for_root_workspace() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            String::new(),
            "main".to_owned(),
        );
        let parsed = WorkspaceStateKey::parse(&key.to_string()).expect("parse");
        assert_eq!(parsed, key);
    }

    #[test]
    fn state_key_parse_roundtrip_for_subdir_workspace() {
        let key = WorkspaceStateKey::new(
            "a1b2".to_owned(),
            "my-project".to_owned(),
            "services~api".to_owned(),
            "feat-x".to_owned(),
        );
        let parsed = WorkspaceStateKey::parse(&key.to_string()).expect("parse");
        assert_eq!(parsed, key);
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
    fn workspace_identity_main_has_no_relative_path() {
        let ws = sample_workspace_main();
        assert!(ws.relative_display_path.is_none());
    }

    #[test]
    fn workspace_identity_subdir_has_relative_path() {
        let ws = sample_workspace_subdir();
        assert_eq!(
            ws.relative_display_path.as_deref(),
            Some("subdir/feature-auth")
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
}

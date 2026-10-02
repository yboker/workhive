use std::fmt;

use serde::{Deserialize, Serialize};

/// A tmux session name wrapper.
///
/// Canonical format (new `@` convention):
///   `wh@{instance_id}@{project}@{workspace}`
///
/// Legacy format (for backward compat):
///   `wh--{project}--{workspace}`
///
/// `SessionName` provides parsing helpers for both formats.
/// New session creation should use `SessionNamingPolicy`; this type is
/// primarily for parsing existing session strings from tmux environment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionName(String);

impl SessionName {
    /// Build a legacy session name from project and workspace identifiers.
    pub fn new(project: &str, workspace: &str) -> Self {
        Self(format!("wh--{project}--{workspace}"))
    }

    /// Wrap an existing session string (e.g., from `$TMUX_PANE` env) for parsing.
    pub fn from_raw(raw: &str) -> Self {
        Self(raw.to_owned())
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derive the legacy tmux server name for a given project.
    pub fn server_name(project: &str) -> String {
        format!("whsrv-{project}")
    }

    /// Extract the workspace portion from a session name.
    ///
    /// Supports both canonical `@` format and legacy `--` format.
    pub fn extract_workspace(&self) -> Option<&str> {
        // Try canonical format first: wh@{instance_id}@{project}@{workspace}
        if let Some(rest) = self.0.strip_prefix("wh@") {
            // Find the last `@` — everything after it is workspace
            if let Some(pos) = rest.rfind('@') {
                let ws = &rest[pos + 1..];
                if !ws.is_empty() {
                    return Some(ws);
                }
            }
        }
        // Fall back to legacy: wh--{project}--{workspace}
        let rest = self.0.strip_prefix("wh--")?;
        let (_project, workspace) = rest.split_once("--")?;
        Some(workspace)
    }

    /// Extract the project portion from a session name.
    ///
    /// Supports both canonical `@` format and legacy `--` format.
    pub fn extract_project(&self) -> Option<&str> {
        // Try canonical format: wh@{instance_id}@{project}@{workspace}
        if let Some(rest) = self.0.strip_prefix("wh@") {
            let (instance_id, remainder) = rest.split_once('@')?;
            if instance_id.is_empty() {
                return None;
            }
            let (project, _workspace) = remainder.rsplit_once('@')?;
            if !project.is_empty() {
                return Some(project);
            }
        }
        // Fall back to legacy: wh--{project}--{workspace}
        let rest = self.0.strip_prefix("wh--")?;
        let (project, _workspace) = rest.split_once("--")?;
        Some(project)
    }

    /// Extract the instance_id from a canonical `@` session name.
    pub fn extract_instance_id(&self) -> Option<&str> {
        let rest = self.0.strip_prefix("wh@")?;
        let (instance_id, _rest) = rest.split_once('@')?;
        if instance_id.is_empty() {
            return None;
        }
        Some(instance_id)
    }
}

impl fmt::Display for SessionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for SessionName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Legacy format ──

    #[test]
    fn legacy_session_name_format() {
        let name = SessionName::new("myproject", "feature-auth");
        assert_eq!(name.as_str(), "wh--myproject--feature-auth");
    }

    #[test]
    fn legacy_server_name_format() {
        assert_eq!(SessionName::server_name("myproject"), "whsrv-myproject");
    }

    #[test]
    fn legacy_extract_workspace() {
        let name = SessionName::new("proj", "feat-x");
        assert_eq!(name.extract_workspace(), Some("feat-x"));
    }

    #[test]
    fn legacy_extract_project() {
        let name = SessionName::new("proj", "feat-x");
        assert_eq!(name.extract_project(), Some("proj"));
    }

    // ── Canonical @ format ──

    #[test]
    fn canonical_extract_workspace() {
        let name = SessionName::from_raw("wh@a1b2@my-project@feature-auth");
        assert_eq!(name.extract_workspace(), Some("feature-auth"));
    }

    #[test]
    fn canonical_extract_project() {
        let name = SessionName::from_raw("wh@a1b2@my-project@feature-auth");
        assert_eq!(name.extract_project(), Some("my-project"));
    }

    #[test]
    fn canonical_extract_instance_id() {
        let name = SessionName::from_raw("wh@a1b2@my-project@feature-auth");
        assert_eq!(name.extract_instance_id(), Some("a1b2"));
    }

    #[test]
    fn canonical_subdir_extract_project() {
        let name = SessionName::from_raw("wh@a1b2@myrepo/test01@main");
        assert_eq!(name.extract_project(), Some("myrepo/test01"));
        assert_eq!(name.extract_workspace(), Some("main"));
    }

    // ── Invalid format ──

    #[test]
    fn extract_returns_none_for_invalid() {
        let name = SessionName(String::from("random-name"));
        assert_eq!(name.extract_workspace(), None);
        assert_eq!(name.extract_project(), None);
        assert_eq!(name.extract_instance_id(), None);
    }
}

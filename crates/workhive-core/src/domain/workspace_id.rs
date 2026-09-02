use std::fmt;

use serde::{Deserialize, Serialize};

use crate::errors::ValidationError;

/// A validated workspace identifier — logical identity for a workspace.
///
/// Validation enforces filesystem/safety constraints only:
/// - Must not be empty
/// - Must not contain whitespace, `..`, `:`, `?`, `*`, `[`, `\`
/// - Must not start or end with `/` or `.`
/// - Must not contain control characters or NUL
///
/// Provider-specific constraints (e.g. git branch naming rules like
/// rejecting `~`, `^`, or `.lock` suffixes) are NOT enforced here.
/// Those belong at the provider adapter boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// Create a new `WorkspaceId` after validation.
    pub fn new(raw: impl Into<String>) -> Result<Self, ValidationError> {
        let raw = raw.into();
        validate_workspace_id(&raw)?;
        Ok(Self(raw))
    }

    /// Return the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for WorkspaceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Validates a workspace identifier using filesystem/safety constraints only.
///
/// Rationale for each rule:
/// - empty: a workspace must have a name
/// - leading/trailing `/` or `.`: prevents hidden files and path traversal
/// - `..`: prevents directory traversal
/// - `:`, `?`, `*`, `[`, `\`: unsafe on Windows and/or shell-special
/// - whitespace: ambiguous in CLIs and path composition
/// - control characters / NUL: filesystem safety
///
/// NOT enforced here (git-only, belongs at provider boundary):
/// - `~`, `^`: git ref syntax characters, not filesystem-dangerous
/// - `.lock` suffix: git lock-file convention, not a workspace concern
fn validate_workspace_id(raw: &str) -> Result<(), ValidationError> {
    if raw.is_empty() {
        return Err(ValidationError::EmptyWorkspaceId);
    }

    if raw.starts_with('/') || raw.ends_with('/') {
        return Err(ValidationError::InvalidWorkspaceId {
            id: raw.to_owned(),
            reason: "must not start or end with '/'".to_owned(),
        });
    }

    if raw.starts_with('.') || raw.ends_with('.') {
        return Err(ValidationError::InvalidWorkspaceId {
            id: raw.to_owned(),
            reason: "must not start or end with '.'".to_owned(),
        });
    }

    // Filesystem/safety forbidden characters
    let forbidden = [':', '?', '*', '[', '\\', ' ', '\t'];
    for ch in forbidden {
        if raw.contains(ch) {
            return Err(ValidationError::InvalidWorkspaceId {
                id: raw.to_owned(),
                reason: format!("contains forbidden character '{ch}'"),
            });
        }
    }

    if raw.chars().any(|c| c.is_control()) {
        return Err(ValidationError::InvalidWorkspaceId {
            id: raw.to_owned(),
            reason: "must not contain control characters".to_owned(),
        });
    }

    if raw.contains("..") {
        return Err(ValidationError::InvalidWorkspaceId {
            id: raw.to_owned(),
            reason: "must not contain '..'".to_owned(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_workspace_ids() {
        assert!(WorkspaceId::new("feature-auth").is_ok());
        assert!(WorkspaceId::new("fix/login-bug").is_ok());
        assert!(WorkspaceId::new("release").is_ok());
        assert!(WorkspaceId::new("my-feature-123").is_ok());
    }

    #[test]
    fn accepts_tilde_and_caret_as_non_git_specific() {
        assert!(
            WorkspaceId::new("has~tilde").is_ok(),
            "~ is git-only; workspace identity must accept it"
        );
        assert!(
            WorkspaceId::new("has^caret").is_ok(),
            "^ is git-only; workspace identity must accept it"
        );
    }

    #[test]
    fn accepts_dot_lock_suffix_as_non_git_specific() {
        assert!(
            WorkspaceId::new("branch.lock").is_ok(),
            ".lock is git-only; workspace identity must accept it"
        );
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            WorkspaceId::new(""),
            Err(ValidationError::EmptyWorkspaceId)
        ));
    }

    #[test]
    fn rejects_filesystem_unsafe_chars() {
        assert!(WorkspaceId::new("has space").is_err());
        assert!(WorkspaceId::new("has:colon").is_err());
        assert!(WorkspaceId::new("has*star").is_err());
        assert!(WorkspaceId::new("has[bracket").is_err());
        assert!(WorkspaceId::new("has\\backslash").is_err());
        assert!(WorkspaceId::new("has?question").is_err());
    }

    #[test]
    fn rejects_control_characters() {
        assert!(WorkspaceId::new("has\x00null").is_err());
        assert!(WorkspaceId::new("has\x07bell").is_err());
    }

    #[test]
    fn rejects_dot_dot() {
        assert!(WorkspaceId::new("a..b").is_err());
    }

    #[test]
    fn rejects_leading_trailing_slash() {
        assert!(WorkspaceId::new("/leading").is_err());
        assert!(WorkspaceId::new("trailing/").is_err());
    }

    #[test]
    fn rejects_leading_trailing_dot() {
        assert!(WorkspaceId::new(".hidden").is_err());
        assert!(WorkspaceId::new("trailing.").is_err());
    }
}

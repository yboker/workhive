//! Validated relative project path — lossless, normalized, typed.
//!
//! Represents a caller's position within a project directory as a typed
//! domain value. Project root is represented by `None` at the call site,
//! never by an empty inner value.

use camino::{Utf8Component, Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};

/// Validated, normalized project-relative path backed by `Utf8PathBuf`.
///
/// Invariants enforced at construction and deserialization:
/// - Not absolute.
/// - No `..` parent references.
/// - Leading `./` stripped via component iteration.
/// - Trailing `/` stripped via component iteration.
/// - Never empty (use `Option<RelativeProjectPath>` for project root).
/// - Original path components preserved losslessly (dots, spaces, etc.).
///
/// Deserialization goes through the validating constructor; derived
/// tuple-struct deserialization is prohibited.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RelativeProjectPath(Utf8PathBuf);

impl<'de> Deserialize<'de> for RelativeProjectPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        RelativeProjectPath::new(&raw).map_err(serde::de::Error::custom)
    }
}

impl RelativeProjectPath {
    /// Construct from a raw string.
    ///
    /// Normalizes by iterating path components: accepts normal components,
    /// discards `CurDir` (`.`), and rejects root/prefix, parent (`..`),
    /// and empty normalized results.
    pub fn new(raw: &str) -> Result<Self, RelativePathError> {
        let input = Utf8Path::new(raw);

        let mut parts: Vec<&str> = Vec::new();
        for component in input.components() {
            match component {
                Utf8Component::RootDir | Utf8Component::Prefix(_) => {
                    return Err(RelativePathError::Absolute(raw.to_owned()));
                }
                Utf8Component::ParentDir => {
                    return Err(RelativePathError::ParentReference(raw.to_owned()));
                }
                Utf8Component::CurDir => {}
                Utf8Component::Normal(segment) => {
                    parts.push(segment);
                }
            }
        }

        if parts.is_empty() {
            return Err(RelativePathError::Empty);
        }

        let normalized = parts.join("/");
        Ok(Self(Utf8PathBuf::from(normalized)))
    }

    /// Construct from a raw string, returning `None` for empty/root paths.
    ///
    /// Convenience for call sites that represent project root as `None`.
    pub fn from_raw_or_root(raw: &str) -> Result<Option<Self>, RelativePathError> {
        let trimmed = raw.strip_prefix("./").unwrap_or(raw).trim_end_matches('/');

        if trimmed.is_empty() {
            return Ok(None);
        }

        Self::new(trimmed).map(Some)
    }

    /// Raw normalized path string.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Underlying `Utf8Path` reference.
    pub fn as_path(&self) -> &Utf8Path {
        self.0.as_path()
    }
}

impl std::fmt::Display for RelativeProjectPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Errors from `RelativeProjectPath` construction.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelativePathError {
    #[error("path must be relative, got absolute: {0}")]
    Absolute(String),
    #[error("path must not contain parent references: {0}")]
    ParentReference(String),
    #[error("path must not be empty (use None for project root)")]
    Empty,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_project_path_valid_simple() {
        let path = RelativeProjectPath::new("services/api").unwrap();
        assert_eq!(path.as_str(), "services/api");
    }

    #[test]
    fn relative_project_path_valid_single_component() {
        let path = RelativeProjectPath::new("backend").unwrap();
        assert_eq!(path.as_str(), "backend");
    }

    #[test]
    fn relative_project_path_rejects_absolute() {
        assert!(matches!(
            RelativeProjectPath::new("/usr/local"),
            Err(RelativePathError::Absolute(_))
        ));
    }

    #[test]
    fn relative_project_path_rejects_parent_ref() {
        assert!(matches!(
            RelativeProjectPath::new("../sibling"),
            Err(RelativePathError::ParentReference(_))
        ));
    }

    #[test]
    fn relative_project_path_rejects_embedded_parent_ref() {
        assert!(matches!(
            RelativeProjectPath::new("services/../etc"),
            Err(RelativePathError::ParentReference(_))
        ));
    }

    #[test]
    fn relative_project_path_rejects_empty() {
        assert!(matches!(
            RelativeProjectPath::new(""),
            Err(RelativePathError::Empty)
        ));
    }

    #[test]
    fn relative_project_path_strips_leading_dot_slash() {
        let path = RelativeProjectPath::new("./services/api").unwrap();
        assert_eq!(path.as_str(), "services/api");
    }

    #[test]
    fn relative_project_path_strips_trailing_slash() {
        let path = RelativeProjectPath::new("services/api/").unwrap();
        assert_eq!(path.as_str(), "services/api");
    }

    #[test]
    fn relative_project_path_preserves_dots_in_names() {
        let p1 = RelativeProjectPath::new("api.v1").unwrap();
        let p2 = RelativeProjectPath::new("api_v1").unwrap();
        assert_ne!(p1, p2, "dotted and underscored paths must be distinct");
        assert_eq!(p1.as_str(), "api.v1");
        assert_eq!(p2.as_str(), "api_v1");
    }

    #[test]
    fn relative_project_path_preserves_spaces() {
        let path = RelativeProjectPath::new("1.0 platform/api.next").unwrap();
        assert_eq!(path.as_str(), "1.0 platform/api.next");
    }

    #[test]
    fn relative_project_path_from_raw_or_root_empty_is_none() {
        assert_eq!(RelativeProjectPath::from_raw_or_root("").unwrap(), None);
    }

    #[test]
    fn relative_project_path_from_raw_or_root_dot_slash_is_none() {
        assert_eq!(RelativeProjectPath::from_raw_or_root("./").unwrap(), None);
    }

    #[test]
    fn relative_project_path_from_raw_or_root_some() {
        let result = RelativeProjectPath::from_raw_or_root("services/api").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), "services/api");
    }

    #[test]
    fn relative_project_path_display() {
        let path = RelativeProjectPath::new("services/api").unwrap();
        assert_eq!(format!("{path}"), "services/api");
    }

    #[test]
    fn relative_project_path_hash_equality() {
        let p1 = RelativeProjectPath::new("services/api").unwrap();
        let p2 = RelativeProjectPath::new("./services/api/").unwrap();
        assert_eq!(p1, p2, "normalized paths must be equal");
    }

    #[test]
    fn relative_project_path_serde_rejects_empty() {
        let json = r#""""#;
        let result: Result<RelativeProjectPath, _> = serde_json::from_str(json);
        assert!(result.is_err(), "empty string must fail deserialization");
    }

    #[test]
    fn relative_project_path_serde_rejects_absolute() {
        let json = r#""/usr/local""#;
        let result: Result<RelativeProjectPath, _> = serde_json::from_str(json);
        assert!(result.is_err(), "absolute path must fail deserialization");
    }

    #[test]
    fn relative_project_path_serde_rejects_parent_ref() {
        let json = r#""../sibling""#;
        let result: Result<RelativeProjectPath, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "parent reference must fail deserialization"
        );
    }

    #[test]
    fn relative_project_path_serde_roundtrip() {
        let original = RelativeProjectPath::new("services/api").unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let restored: RelativeProjectPath = serde_json::from_str(&json).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn relative_project_path_serde_normalizes_dot_slash() {
        let json = r#""./services/api/""#;
        let path: RelativeProjectPath =
            serde_json::from_str(json).expect("normalized relative path must deserialize");
        assert_eq!(path.as_str(), "services/api");
    }
}

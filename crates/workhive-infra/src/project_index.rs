//! JSON-based global project index adapter.
//!
//! Index file: `~/.config/workhive/index.json`

use camino::Utf8PathBuf;
use workhive_core::errors::IndexError;
use workhive_core::ports::{IndexState, ProjectIndexPort, ProjectSummary};

const INDEX_FILE: &str = "index.json";

/// File-based project index adapter.
pub struct JsonProjectIndex {
    index_path: Utf8PathBuf,
}

impl JsonProjectIndex {
    pub fn new() -> Result<Self, IndexError> {
        let config_dir =
            crate::config::resolve_global_config_dir().map_err(|_| IndexError::ReadFailed {
                path: Utf8PathBuf::from("<global-config-dir>/index.json"),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "failed to resolve global config directory (HOME not set)",
                ),
            })?;
        let index_path = config_dir.join(INDEX_FILE);
        Ok(Self { index_path })
    }

    /// Create with an explicit path (for testing).
    pub fn with_path(path: Utf8PathBuf) -> Self {
        Self { index_path: path }
    }

    fn read_all(&self) -> Result<IndexState, IndexError> {
        if !self.index_path.exists() {
            return Ok(IndexState::NotFound);
        }
        let content =
            std::fs::read_to_string(&self.index_path).map_err(|source| IndexError::ReadFailed {
                path: self.index_path.clone(),
                source,
            })?;
        let projects: Vec<ProjectSummary> =
            serde_json::from_str(&content).map_err(|_| IndexError::Corrupted {
                path: self.index_path.clone(),
            })?;
        Ok(IndexState::Loaded(projects))
    }

    /// Read the raw project list for internal mutation operations.
    ///
    /// Unlike `read_all`, this always returns a `Vec` — treating a missing
    /// index file as empty. Used by `upsert`, `remove`, and `rebuild`.
    fn read_all_vec(&self) -> Result<Vec<ProjectSummary>, IndexError> {
        Ok(self.read_all()?.into_projects())
    }

    fn write_all(&self, projects: &[ProjectSummary]) -> Result<(), IndexError> {
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| IndexError::WriteFailed {
                path: self.index_path.clone(),
                source,
            })?;
        }
        let content =
            serde_json::to_string_pretty(projects).map_err(|_| IndexError::Corrupted {
                path: self.index_path.clone(),
            })?;
        std::fs::write(&self.index_path, content).map_err(|source| IndexError::WriteFailed {
            path: self.index_path.clone(),
            source,
        })?;
        tracing::debug!(path = %self.index_path, count = projects.len(), "index written");
        Ok(())
    }
}

impl ProjectIndexPort for JsonProjectIndex {
    fn list_projects(&self) -> Result<IndexState, IndexError> {
        self.read_all()
    }

    fn upsert_project(&self, summary: &ProjectSummary) -> Result<(), IndexError> {
        let mut projects = self.read_all_vec()?;
        projects.retain(|p| p.project_root != summary.project_root);
        projects.push(summary.clone());
        self.write_all(&projects)
    }

    fn remove_project(&self, project_root: &Utf8PathBuf) -> Result<(), IndexError> {
        let mut projects = self.read_all_vec()?;
        projects.retain(|p| &p.project_root != project_root);
        self.write_all(&projects)
    }

    fn clear(&self) -> Result<(), IndexError> {
        self.write_all(&[])
    }

    fn rebuild(&self) -> Result<(), IndexError> {
        let projects = self.read_all_vec()?;
        let valid: Vec<_> = projects
            .into_iter()
            .filter(|p| p.project_root.exists())
            .collect();
        self.write_all(&valid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_index(tmp: &TempDir) -> JsonProjectIndex {
        let path = Utf8PathBuf::try_from(tmp.path().join("index.json")).unwrap();
        JsonProjectIndex::with_path(path)
    }

    fn sample_project(name: &str) -> ProjectSummary {
        ProjectSummary {
            project_name: name.to_owned(),
            project_root: Utf8PathBuf::from(format!("/tmp/{name}")),
            config_path: Utf8PathBuf::from(format!("/tmp/{name}/.workhive/config.toml")),
            last_opened_at: None,
            last_updated_at: None,
        }
    }

    #[test]
    fn missing_index_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);
        let state = idx.list_projects().unwrap();
        assert!(state.is_not_found());
    }

    #[test]
    fn empty_index_returns_loaded_empty() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);
        // Write an empty array to create the file
        idx.clear().unwrap();
        let state = idx.list_projects().unwrap();
        assert!(!state.is_not_found());
        assert!(state.projects().is_empty());
    }

    #[test]
    fn corrupted_index_returns_error() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("index.json");
        std::fs::write(&path, "not valid json!!!").unwrap();
        let idx = JsonProjectIndex::with_path(Utf8PathBuf::try_from(path).unwrap());
        let result = idx.list_projects();
        assert!(result.is_err());
    }

    #[test]
    fn upsert_and_list() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        idx.upsert_project(&sample_project("alpha")).unwrap();
        idx.upsert_project(&sample_project("beta")).unwrap();

        let state = idx.list_projects().unwrap();
        assert_eq!(state.projects().len(), 2);
    }

    #[test]
    fn upsert_replaces_existing() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        idx.upsert_project(&sample_project("alpha")).unwrap();
        idx.upsert_project(&sample_project("alpha")).unwrap();

        assert_eq!(idx.list_projects().unwrap().projects().len(), 1);
    }

    #[test]
    fn remove_project_works() {
        let tmp = TempDir::new().unwrap();
        let idx = test_index(&tmp);

        let p = sample_project("alpha");
        idx.upsert_project(&p).unwrap();
        idx.remove_project(&p.project_root).unwrap();

        assert!(idx.list_projects().unwrap().projects().is_empty());
    }

    #[test]
    fn new_uses_xdg_config_home_when_set() {
        let tmp = TempDir::new().unwrap();
        let xdg_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        crate::test_env::with_xdg_config_home(xdg_dir.as_str(), || {
            let idx = JsonProjectIndex::new().unwrap();
            let expected = xdg_dir.join("workhive").join("index.json");
            assert_eq!(idx.index_path, expected);
        });
    }

    #[test]
    fn new_and_config_dir_resolve_same_parent() {
        let tmp = TempDir::new().unwrap();
        let xdg_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        crate::test_env::with_xdg_config_home(xdg_dir.as_str(), || {
            let config_dir = crate::config::resolve_global_config_dir().unwrap();
            let idx = JsonProjectIndex::new().unwrap();

            assert_eq!(
                idx.index_path.parent().unwrap(),
                config_dir.as_path(),
                "index and config must share the same global directory"
            );
        });
    }
}

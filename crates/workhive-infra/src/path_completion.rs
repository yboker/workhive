//! `PathCompletionFsPort` adapter backed by `std::fs`.
//!
//! Used by the global init wizard to power Tab completion in the
//! trusted-root prompt. Filesystem errors here are surfaced as typed
//! `io::Error` so the caller can decide how to render them — the CLI
//! wizard turns them into a localized "no candidates" hint and
//! continues, which is acceptable per the testing policy since the wizard
//! never depends on Tab completion for correctness.

use camino::{Utf8Path, Utf8PathBuf};

use workhive_core::ports::{PathCompletionEntry, PathCompletionFsPort, PathKind};

/// Default real-filesystem implementation of [`PathCompletionFsPort`].
#[derive(Debug, Clone, Default)]
pub struct FsPathCompletion;

impl FsPathCompletion {
    /// Construct a fresh adapter. Stateless.
    pub fn new() -> Self {
        Self
    }
}

impl PathCompletionFsPort for FsPathCompletion {
    fn list_children(&self, parent: &Utf8Path) -> Result<Vec<PathCompletionEntry>, std::io::Error> {
        let read = std::fs::read_dir(parent.as_std_path())?;
        let mut out = Vec::new();
        for entry in read {
            let entry = match entry {
                Ok(e) => e,
                // One bad entry should not abort the whole completion —
                // just skip it. We log at debug so this is observable.
                Err(err) => {
                    tracing::debug!(
                        parent = parent.as_str(),
                        error = %err,
                        "skipping unreadable directory entry during path completion"
                    );
                    continue;
                }
            };

            let Ok(name_utf8) = Utf8PathBuf::try_from(std::path::PathBuf::from(entry.file_name()))
            else {
                // Non-UTF8 file names are simply skipped — we cannot
                // store them in `Utf8PathBuf` and the wizard cannot
                // safely write them to TOML anyway.
                continue;
            };

            // file_type() may itself fail (e.g. broken symlink). Treat
            // unknown as not-a-directory so it does not poison
            // completion candidates.
            let is_directory = entry.file_type().map(|f| f.is_dir()).unwrap_or(false);

            out.push(PathCompletionEntry {
                name: name_utf8.as_str().to_owned(),
                is_directory,
            });
        }
        Ok(out)
    }

    fn classify_path(&self, path: &Utf8Path) -> PathKind {
        match std::fs::metadata(path.as_std_path()) {
            Ok(meta) if meta.is_dir() => PathKind::Directory,
            Ok(_) => PathKind::File,
            Err(_) => PathKind::Missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_children_returns_directory_entries() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).expect("utf-8 tempdir");
        fs::create_dir(root.join("alpha")).unwrap();
        fs::create_dir(root.join("beta")).unwrap();
        fs::write(root.join("notes.md"), b"hello").unwrap();

        let port = FsPathCompletion::new();
        let mut entries = port.list_children(root.as_path()).expect("list ok");
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<(String, bool)> = entries
            .into_iter()
            .map(|e| (e.name, e.is_directory))
            .collect();
        assert_eq!(
            names,
            vec![
                ("alpha".to_owned(), true),
                ("beta".to_owned(), true),
                ("notes.md".to_owned(), false),
            ]
        );
    }

    #[test]
    fn classify_path_distinguishes_directory_file_and_missing() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let root = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).expect("utf-8 tempdir");
        let dir = root.join("dir");
        let file = root.join("note.md");
        fs::create_dir(&dir).unwrap();
        fs::write(&file, b"hi").unwrap();

        let port = FsPathCompletion::new();
        assert_eq!(port.classify_path(dir.as_path()), PathKind::Directory);
        assert_eq!(port.classify_path(file.as_path()), PathKind::File);
        assert_eq!(
            port.classify_path(root.join("missing").as_path()),
            PathKind::Missing
        );
    }

    /// End-to-end check that the production filesystem adapter feeds
    /// `compute_path_completions` correctly: empty input returns the
    /// alphabetically sorted home child directories, with `~/` itself
    /// intentionally excluded.
    #[test]
    fn real_fs_empty_prefix_completion_lists_home_children_alphabetically() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).expect("utf-8 tempdir");
        fs::create_dir(home.join("development")).unwrap();
        fs::create_dir(home.join("projects")).unwrap();
        fs::create_dir(home.join("Music")).unwrap();
        fs::create_dir(home.join(".hidden")).unwrap();
        fs::write(home.join("notes.md"), b"hi").unwrap();

        let strategy = workhive_core::usecases::path_strategy::current_target_strategy();
        let result = workhive_core::usecases::path_completion::compute_path_completions(
            "",
            Some(home.as_path()),
            strategy.as_ref(),
            &FsPathCompletion::new(),
        );

        assert_eq!(
            result,
            workhive_core::usecases::path_completion::PathCompletionResult::Many {
                candidates: vec![
                    "~/Music/".to_owned(),
                    "~/development/".to_owned(),
                    "~/projects/".to_owned(),
                ],
            },
            "empty input must list non-hidden home child directories alphabetically, \
             with `~/` itself excluded and files filtered out"
        );
    }

    /// Live-search end-to-end through the production adapter: typing
    /// `~/dev` while home contains `dev` and `development` returns
    /// both candidates without requiring a Tab press.
    #[test]
    fn real_fs_live_search_for_dev_prefix_returns_matching_children() {
        let tmp = tempfile::tempdir().expect("tempdir should be created");
        let home = Utf8PathBuf::from_path_buf(tmp.path().to_owned()).expect("utf-8 tempdir");
        fs::create_dir(home.join("dev")).unwrap();
        fs::create_dir(home.join("development")).unwrap();
        fs::create_dir(home.join("Music")).unwrap();

        let strategy = workhive_core::usecases::path_strategy::current_target_strategy();
        let result = workhive_core::usecases::path_completion::compute_path_completions(
            "~/de",
            Some(home.as_path()),
            strategy.as_ref(),
            &FsPathCompletion::new(),
        );

        assert_eq!(
            result,
            workhive_core::usecases::path_completion::PathCompletionResult::Many {
                candidates: vec!["~/dev/".to_owned(), "~/development/".to_owned()],
            }
        );
    }

    #[test]
    fn list_children_returns_io_error_for_missing_parent() {
        let port = FsPathCompletion::new();
        let result = port.list_children(Utf8Path::new("/this/path/should/not/exist/__wh__"));
        assert!(
            result.is_err(),
            "missing directory must surface as Err so callers can decide"
        );
    }
}

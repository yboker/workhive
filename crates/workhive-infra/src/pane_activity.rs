//! Filesystem implementation of [`PaneActivityStorePort`].
//!
//! On-disk layout (under a project-local runtime root):
//!
//! ```text
//! <runtime_root>/<group>/<workspace>/activity/<unit_id>.json
//! ```
//!
//! `<runtime_root>` is supplied by the caller — for project-scoped panes the
//! `<project>/.workhive/runtime` directory; for global tools the
//! `XDG_RUNTIME_DIR/workhive` equivalent. The adapter is intentionally
//! agnostic to which root it sits in so the same store backs both.
//!
//! Writes are atomic-replace via `tempfile::NamedTempFile`. Reads that
//! encounter corrupt JSON return `None` so the renderer can fall back to
//! `OpenUnverified` instead of crashing `pane list`.

use camino::Utf8PathBuf;
use std::io::Write as _;
use tempfile::NamedTempFile;
use workhive_core::domain::AiActivityFact;
use workhive_core::ports::{PaneActivityIoError, PaneActivityKey, PaneActivityStorePort};

/// Filename component used by hooks/plugins to discover the
/// per-unit activity file. Bumped only when the on-disk layout changes
/// in a non-tolerable way.
const ACTIVITY_DIR: &str = "activity";

/// Atomic-replace filesystem adapter for AI activity facts.
pub struct FsPaneActivityStore {
    /// Root directory under which the `<group>/<workspace>/activity/`
    /// hierarchy is materialized. Owned by the caller.
    root: Utf8PathBuf,
}

impl FsPaneActivityStore {
    /// Create a store rooted at `root` (typically `<project>/.workhive/runtime`).
    pub fn new(root: impl Into<Utf8PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Compute the on-disk file path for a given pane key.
    ///
    /// Public so callers (CLI status output, integration tests) can
    /// introspect it. Every component (`group`, `workspace`,
    /// `unit_id`) is funnelled through [`sanitize_path_component`]
    /// before joining so a hostile payload cannot escape `root`. The
    /// adapter purposefully accepts the unsanitized key shape on its
    /// boundary so callers can never bypass the sanitizer.
    pub fn fact_path(&self, key: &PaneActivityKey) -> Utf8PathBuf {
        self.root
            .join(sanitize_path_component(&key.group))
            .join(sanitize_path_component(&key.workspace))
            .join(ACTIVITY_DIR)
            .join(format!("{}.json", sanitize_path_component(&key.unit_id)))
    }
}

/// Sanitize a single path component used by [`FsPaneActivityStore`].
///
/// Rules (each one is required to keep the on-disk file under the
/// configured `root`; see review High 1):
///
/// 1. Path separators (`/`, `\`) and NUL are neutralised to `_`.
/// 2. The leading/trailing `.` and `..` components are neutralised
///    so the joined path can never traverse the parent directory.
///    We do this defensively even after rule 1 because a payload
///    that contains literally `".."` (no separators) would otherwise
///    join cleanly and let `..` act as a directory traversal.
/// 3. Empty components are replaced with `_` so the on-disk layout
///    always has the same number of segments regardless of payload.
///
/// The output is intentionally lossy: a hostile payload is never
/// reflected back into a usable path. Pure function for unit
/// testing.
fn sanitize_path_component(input: &str) -> String {
    if input.is_empty() {
        return "_".to_owned();
    }
    let mut neutralised: String = input
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Reject the special dot-only components (`.`, `..`) even though
    // they no longer contain separators after rule 1, because the
    // filesystem still interprets them as the current / parent
    // directory.
    if neutralised == "." || neutralised == ".." {
        neutralised = "_".to_owned();
    }
    neutralised
}

impl PaneActivityStorePort for FsPaneActivityStore {
    fn record(&self, fact: &AiActivityFact) -> Result<(), PaneActivityIoError> {
        let key = PaneActivityKey::from_fact(fact);
        let target = self.fact_path(&key);
        // Defence-in-depth: even with the per-component sanitizer
        // above, refuse to follow any non-trivial parent traversal
        // before doing real I/O. This guarantees the file we write
        // can never sit outside `self.root`.
        let canonical_root = self.root.as_std_path();
        if !target.as_std_path().starts_with(canonical_root) {
            return Err(PaneActivityIoError::Io(format!(
                "refusing to write outside runtime root: target={target}, root={}",
                self.root
            )));
        }
        let parent = target
            .parent()
            .ok_or_else(|| PaneActivityIoError::Io(format!("invalid path: {target}")))?;

        std::fs::create_dir_all(parent.as_std_path())
            .map_err(|e| PaneActivityIoError::Io(format!("create_dir_all {parent}: {e}")))?;

        let json = serde_json::to_string(fact)
            .map_err(|e| PaneActivityIoError::Io(format!("serialize fact: {e}")))?;

        let mut tmp = NamedTempFile::new_in(parent.as_std_path())
            .map_err(|e| PaneActivityIoError::Io(format!("tempfile in {parent}: {e}")))?;
        tmp.write_all(json.as_bytes())
            .and_then(|_| tmp.flush())
            .map_err(|e| PaneActivityIoError::Io(format!("write tempfile: {e}")))?;
        tmp.persist(target.as_std_path())
            .map_err(|e| PaneActivityIoError::Io(format!("persist {target}: {e}")))?;
        Ok(())
    }

    fn read(&self, key: &PaneActivityKey) -> Result<Option<AiActivityFact>, PaneActivityIoError> {
        let path = self.fact_path(key);
        match std::fs::read_to_string(path.as_std_path()) {
            Ok(content) => match serde_json::from_str::<AiActivityFact>(&content) {
                Ok(fact) => Ok(Some(fact)),
                Err(e) => {
                    tracing::warn!(
                        path = %path,
                        error = %e,
                        "pane activity store: corrupt fact JSON; treating as missing"
                    );
                    Ok(None)
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(PaneActivityIoError::Io(format!("read {path}: {err}"))),
        }
    }

    fn remove(&self, key: &PaneActivityKey) -> Result<(), PaneActivityIoError> {
        let path = self.fact_path(key);
        match std::fs::remove_file(path.as_std_path()) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(PaneActivityIoError::Io(format!("remove {path}: {err}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use workhive_core::domain::{AiActivityConfidence, AiActivityProvider, AiActivityState};

    fn sample_fact(unit_id: &str) -> AiActivityFact {
        AiActivityFact::new(
            AiActivityProvider::ClaudeCode,
            "claude",
            "wh-srv",
            "ws-1",
            unit_id,
            AiActivityState::Processing,
            AiActivityConfidence::Hook,
            1_700_000_000_000,
            None,
        )
    }

    fn key(unit_id: &str) -> PaneActivityKey {
        PaneActivityKey {
            group: "wh-srv".to_owned(),
            workspace: "ws-1".to_owned(),
            unit_id: unit_id.to_owned(),
        }
    }

    #[test]
    fn record_creates_directories_and_atomically_writes() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root.clone());
        let fact = sample_fact("%0");
        store.record(&fact).unwrap();

        let path = root.join("wh-srv/ws-1/activity/%0.json");
        assert!(path.exists(), "fact file must be created at {path}");

        let restored = store.read(&key("%0")).unwrap().expect("fact present");
        assert_eq!(restored, fact);
    }

    #[test]
    fn record_overwrites_previous_fact_for_same_unit() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root);

        let mut f1 = sample_fact("%0");
        f1.state = AiActivityState::Processing;
        store.record(&f1).unwrap();

        let mut f2 = sample_fact("%0");
        f2.state = AiActivityState::WaitingForUser;
        store.record(&f2).unwrap();

        let restored = store.read(&key("%0")).unwrap().unwrap();
        assert_eq!(restored.state, AiActivityState::WaitingForUser);
    }

    #[test]
    fn read_returns_none_when_no_fact_exists() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root);
        assert!(store.read(&key("%nope")).unwrap().is_none());
    }

    #[test]
    fn read_treats_corrupt_json_as_missing_so_pane_list_keeps_rendering() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root.clone());
        let path = root.join("wh-srv/ws-1/activity/%0.json");
        std::fs::create_dir_all(path.parent().unwrap().as_std_path()).unwrap();
        std::fs::write(path.as_std_path(), b"{not valid json").unwrap();

        // Corrupt payload must be reported as missing — the renderer
        // falls back to OpenUnverified instead of crashing.
        assert!(store.read(&key("%0")).unwrap().is_none());
    }

    #[test]
    fn remove_is_noop_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root);
        store.remove(&key("%nope")).unwrap();
    }

    #[test]
    fn remove_deletes_existing_file() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root);
        store.record(&sample_fact("%0")).unwrap();
        store.remove(&key("%0")).unwrap();
        assert!(store.read(&key("%0")).unwrap().is_none());
    }

    #[test]
    fn unit_id_with_slash_is_sanitized() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root.clone());
        let fact = sample_fact("%0/extra");
        store.record(&fact).unwrap();
        let path = store.fact_path(&PaneActivityKey {
            group: "wh-srv".into(),
            workspace: "ws-1".into(),
            unit_id: "%0/extra".into(),
        });
        let s = path.as_str();
        assert!(
            !s.ends_with("%0/extra.json"),
            "unit_id slash must be sanitized in path; got {s}"
        );
        assert!(path.exists());
    }

    /// Review High 1: a hostile hook payload that uses `..` for
    /// `group` or `workspace` must never produce a path that
    /// traverses outside the configured `root`. `record` must also
    /// refuse to write such a file (defence-in-depth).
    #[test]
    fn parent_traversal_in_group_or_workspace_stays_under_root() {
        let dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let store = FsPaneActivityStore::new(root.clone());

        // 1) Plain `..` for group: the sanitizer must collapse it.
        let key = PaneActivityKey {
            group: "..".into(),
            workspace: "ws-1".into(),
            unit_id: "%0".into(),
        };
        let path = store.fact_path(&key);
        assert!(
            path.as_std_path().starts_with(root.as_std_path()),
            "group=`..` must stay under root, got {path}",
        );

        // 2) Slash-bearing workspace: `..` plus a separator. The
        //    sanitizer neutralises the separator first, then the
        //    dotted-only fallback turns the remaining component into
        //    `_` so it cannot still be interpreted as `parent`.
        let key2 = PaneActivityKey {
            group: "wh-srv".into(),
            workspace: "../escape".into(),
            unit_id: "%0".into(),
        };
        let path2 = store.fact_path(&key2);
        assert!(
            path2.as_std_path().starts_with(root.as_std_path()),
            "workspace=`../escape` must stay under root, got {path2}",
        );

        // 3) Hostile unit_id with NUL.
        let key3 = PaneActivityKey {
            group: "wh-srv".into(),
            workspace: "ws-1".into(),
            unit_id: "..\0bad".into(),
        };
        let path3 = store.fact_path(&key3);
        assert!(
            path3.as_std_path().starts_with(root.as_std_path()),
            "unit_id with NUL must stay under root, got {path3}",
        );

        // 4) record() with hostile group must write under root and
        //    leave no artefact outside it.
        let mut hostile = sample_fact("%0");
        hostile.group = "../escape".into();
        store.record(&hostile).expect("record sanitises components");
        // Confirm nothing landed outside root.
        let parent_of_root = root.parent().unwrap();
        let stray = parent_of_root.join("escape");
        assert!(!stray.exists(), "no file may land at {stray}");
    }

    /// `sanitize_path_component` is a pure helper; lock its contract
    /// in case future callers reuse it.
    #[test]
    fn sanitize_path_component_neutralises_separators_and_dot_only() {
        assert_eq!(sanitize_path_component(".."), "_");
        assert_eq!(sanitize_path_component("."), "_");
        assert_eq!(sanitize_path_component(""), "_");
        assert_eq!(sanitize_path_component("a/b"), "a_b");
        assert_eq!(sanitize_path_component("a\\b"), "a_b");
        assert_eq!(sanitize_path_component("a\0b"), "a_b");
        assert_eq!(sanitize_path_component("safe"), "safe");
    }

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn store_is_send_sync_for_use_across_threads() {
        // FsPaneActivityStore is a tiny struct with no interior
        // mutability; assert it remains thread-safe.
        assert_send_sync::<FsPaneActivityStore>();
    }
}

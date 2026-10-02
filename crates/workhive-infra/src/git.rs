//! Git CLI adapter — all git operations via `Command::arg()`.
//!
//! Per the git/tmux adapter policy: NEVER build shell strings, NEVER use shlex.

use std::process::Command;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::errors::GitError;
use workhive_core::ports::{
    ChangeKind, ChangedFile, EnsureWorktreeRequest, GitPort, WorktreeInfo, WorktreeRemovePolicy,
    WorktreeStatus, WorktreeStrategy,
};

/// Git adapter using system `git` binary.
pub struct GitCliAdapter;

impl GitCliAdapter {
    pub fn new() -> Self {
        Self
    }

    fn run_git(args: &[&str]) -> Result<String, GitError> {
        tracing::debug!(args = ?args, "running git command");
        let output = Command::new("git")
            .args(args)
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            tracing::warn!(stderr = %stderr, "git command failed");
            Err(GitError::CommandFailed { message: stderr })
        }
    }

    fn classify_repo_discovery_error(stderr: &str) -> GitError {
        let lowered = stderr.to_ascii_lowercase();
        if lowered.contains("not a git repository") {
            GitError::NotARepo
        } else {
            GitError::CommandFailed {
                message: stderr.to_owned(),
            }
        }
    }

    fn classify_branch_probe_failure(stderr: &str) -> Result<bool, GitError> {
        if stderr.trim().is_empty() {
            return Ok(false);
        }
        Err(Self::classify_repo_discovery_error(stderr))
    }
}

impl Default for GitCliAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl GitPort for GitCliAdapter {
    fn ensure_worktree(&self, request: EnsureWorktreeRequest) -> Result<WorktreeInfo, GitError> {
        let branch = &request.workspace_name;
        let default_path = request.base_path.join(branch);

        let worktrees = self.list_worktrees()?;
        let existing_wt = worktrees
            .iter()
            .find(|w| w.branch.as_deref() == Some(branch));
        let branch_known = self.branch_exists(branch)?;

        let strategy = if existing_wt.is_some() {
            WorktreeStrategy::ReuseExisting
        } else if branch_known {
            WorktreeStrategy::AttachBranch
        } else {
            WorktreeStrategy::CreateBranch
        };

        tracing::info!(
            workspace = %branch,
            list_worktrees_count = worktrees.len(),
            matched_existing_worktree_path = ?existing_wt.map(|w| w.path.as_str()),
            branch_exists = branch_known,
            worktree_strategy = ?strategy,
            "ensure_worktree decision"
        );

        match strategy {
            WorktreeStrategy::ReuseExisting => {
                let wt = existing_wt.expect("ReuseExisting strategy requires existing worktree");
                tracing::debug!(path = %wt.path, branch = %branch, "reusing existing worktree");
                Ok(WorktreeInfo {
                    path: wt.path.clone(),
                    branch: Some(branch.to_owned()),
                    is_main: false,
                    resolved_strategy: Some(WorktreeStrategy::ReuseExisting),
                })
            }
            WorktreeStrategy::AttachBranch => {
                let path_str = default_path.as_str();
                tracing::debug!(path = %default_path, branch = %branch, "attaching existing branch to new worktree");
                Self::run_git(&["worktree", "add", path_str, branch])?;
                Ok(WorktreeInfo {
                    path: default_path,
                    branch: Some(branch.to_owned()),
                    is_main: false,
                    resolved_strategy: Some(WorktreeStrategy::AttachBranch),
                })
            }
            WorktreeStrategy::CreateBranch => {
                let path_str = default_path.as_str();
                tracing::debug!(path = %default_path, branch = %branch, "creating new branch and worktree");
                Self::run_git(&["worktree", "add", "-b", branch, path_str])?;
                Ok(WorktreeInfo {
                    path: default_path,
                    branch: Some(branch.to_owned()),
                    is_main: false,
                    resolved_strategy: Some(WorktreeStrategy::CreateBranch),
                })
            }
        }
    }

    fn remove_worktree(
        &self,
        path: &Utf8Path,
        policy: WorktreeRemovePolicy,
    ) -> Result<(), GitError> {
        tracing::debug!(path = %path, "removing worktree");
        let result = if policy.is_force() {
            Self::run_git(&["worktree", "remove", "--force", path.as_str()])
        } else {
            Self::run_git(&["worktree", "remove", path.as_str()])
        };
        result.map_err(|error| GitError::WorktreeRemoveFailed {
            path: path.to_owned(),
            source: std::io::Error::other(error.to_string()),
        })?;
        Ok(())
    }

    fn worktree_status(&self, path: &Utf8Path) -> Result<WorktreeStatus, GitError> {
        tracing::debug!(path = %path, "collecting worktree status");
        let output = Command::new("git")
            .arg("-C")
            .arg(path.as_str())
            .arg("status")
            .arg("--porcelain")
            .arg("--untracked-files=all")
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            tracing::warn!(path = %path, stderr = %stderr, "git status failed");
            return Err(GitError::CommandFailed { message: stderr });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(WorktreeStatus {
            changed_files: parse_porcelain_status(&stdout),
        })
    }

    fn unmerged_commit_count(&self, base: &str, branch: &str) -> Result<u32, GitError> {
        let range = format!("{base}..{branch}");
        let count = Self::run_git(&["rev-list", "--count", &range])?;
        count
            .parse::<u32>()
            .map_err(|error| GitError::CommandFailed {
                message: format!("failed to parse rev-list count '{count}': {error}"),
            })
    }

    fn delete_branch(&self, branch: &str) -> Result<(), GitError> {
        Self::run_git(&["branch", "-d", branch]).map(|_| ())
    }

    fn repo_root(&self) -> Result<Utf8PathBuf, GitError> {
        tracing::debug!("running git repo root discovery");
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--show-toplevel")
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            // An expected, user-recoverable probe outcome (e.g. "not a git
            // repository"); the caller renders the localized error, so this
            // detail belongs at debug verbosity only.
            tracing::debug!(stderr = %stderr, "git repo root discovery failed");
            return Err(Self::classify_repo_discovery_error(&stderr));
        }

        let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Utf8PathBuf::try_from(std::path::PathBuf::from(&root)).map_err(|_| {
            GitError::CommandFailed {
                message: format!("non-UTF8 repo root: {root}"),
            }
        })
    }

    fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
        let output = Self::run_git(&["worktree", "list", "--porcelain"])?;
        let worktrees = parse_worktree_list(&output);
        for wt in &worktrees {
            tracing::debug!(
                path = %wt.path,
                branch = ?wt.branch,
                is_main = wt.is_main,
                "parsed worktree entry"
            );
        }
        Ok(worktrees)
    }

    fn current_branch(&self) -> Result<String, GitError> {
        // `symbolic-ref --short HEAD` resolves the branch name even for an
        // unborn HEAD (a freshly initialized repository with no commits),
        // where `rev-parse --abbrev-ref HEAD` instead fails with an
        // ambiguous-HEAD error. A detached HEAD has no symbolic ref and
        // surfaces as an error, matching the `current_branch` contract.
        tracing::debug!("resolving current branch via symbolic-ref");
        let output = Command::new("git")
            .arg("symbolic-ref")
            .arg("--short")
            .arg("HEAD")
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            // Expected, user-recoverable probe outcomes (no symbolic ref,
            // not a repository); callers render localized guidance, so this
            // belongs at debug verbosity only.
            tracing::debug!(stderr = %stderr, "current branch resolution failed");
            Err(Self::classify_repo_discovery_error(&stderr))
        }
    }

    fn branch_exists(&self, branch: &str) -> Result<bool, GitError> {
        let ref_name = format!("refs/heads/{branch}");
        tracing::debug!(branch, "checking branch existence");
        // `--quiet` makes a missing ref an ordinary non-zero exit with no
        // stderr, so an absent branch (the expected first-run probe outcome,
        // including an unborn repository) resolves to `false` without emitting
        // a raw git-failure warning.
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--verify")
            .arg("--quiet")
            .arg(&ref_name)
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if output.status.success() {
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            match Self::classify_branch_probe_failure(&stderr) {
                Ok(false) => {
                    tracing::debug!(branch, "branch does not exist");
                    Ok(false)
                }
                Err(error) => {
                    tracing::debug!(branch, stderr = %stderr, "branch existence probe failed");
                    Err(error)
                }
                Ok(true) => unreachable!("branch probe failure cannot classify as existing"),
            }
        }
    }

    fn has_commits(&self) -> Result<bool, GitError> {
        tracing::debug!("checking whether repository has commits");
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--verify")
            .arg("--quiet")
            .arg("HEAD")
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if output.status.success() {
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            match Self::classify_branch_probe_failure(&stderr) {
                Ok(false) => {
                    tracing::debug!("repository has no commits");
                    Ok(false)
                }
                Err(error) => {
                    tracing::debug!(stderr = %stderr, "commit existence probe failed");
                    Err(error)
                }
                Ok(true) => unreachable!("commit probe failure cannot classify as committed"),
            }
        }
    }

    fn create_empty_initial_commit(&self, message: &str) -> Result<(), GitError> {
        tracing::debug!("creating empty initial commit");
        Self::run_git(&[
            "-c",
            "user.name=Workhive",
            "-c",
            "user.email=workhive@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            message,
        ])
        .map(|_| ())
    }

    fn init_repo(&self, path: &Utf8Path) -> Result<(), GitError> {
        tracing::debug!(path = %path, "initializing git repository");
        let output = Command::new("git")
            .arg("init")
            .current_dir(path.as_std_path())
            .output()
            .map_err(|_| GitError::GitNotFound)?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(GitError::CommandFailed {
                message: format!("git init failed: {stderr}"),
            })
        }
    }
}

/// Parse `git worktree list --porcelain` output.
///
/// `is_main` is determined positionally: the first worktree entry in
/// `git worktree list` output is always the main worktree. This is a
/// documented git invariant and does not depend on `repo_root()` or the
/// directory from which the command is executed — both of which can be
/// misleading when running inside a linked worktree.
fn parse_worktree_list(porcelain: &str) -> Vec<WorktreeInfo> {
    let mut results = Vec::new();
    let mut path: Option<Utf8PathBuf> = None;
    let mut branch: Option<String> = None;

    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(prev_path) = path.take() {
                let is_main = results.is_empty();
                results.push(WorktreeInfo {
                    path: prev_path,
                    branch: branch.take(),
                    is_main,
                    resolved_strategy: None,
                });
            }
            path = Utf8PathBuf::try_from(std::path::PathBuf::from(p)).ok();
            branch = None;
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            branch = Some(b.to_owned());
        }
    }

    if let Some(p) = path {
        let is_main = results.is_empty();
        results.push(WorktreeInfo {
            path: p,
            branch,
            is_main,
            resolved_strategy: None,
        });
    }

    results
}

fn parse_porcelain_status(output: &str) -> Vec<ChangedFile> {
    output.lines().filter_map(parse_status_line).collect()
}

fn parse_status_line(line: &str) -> Option<ChangedFile> {
    if line.len() < 3 {
        return None;
    }
    if line.starts_with("##") {
        return None;
    }

    let status_code = line[..2].to_owned();
    if status_code == "!!" {
        return None;
    }

    let mut kinds = Vec::new();
    if status_code == "??" {
        kinds.push(ChangeKind::Untracked);
    } else {
        let chars = status_code.chars().collect::<Vec<_>>();
        if chars[0] != ' ' {
            kinds.push(ChangeKind::Staged);
        }
        if chars[1] != ' ' {
            kinds.push(ChangeKind::Unstaged);
        }
    }

    if kinds.is_empty() {
        return None;
    }

    let path = line[3..].to_owned();
    Some(ChangedFile {
        status_code,
        path,
        kinds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_list_single() {
        let input = "worktree /Users/me/project\nHEAD abc123\nbranch refs/heads/main\n\n";
        let wts = parse_worktree_list(input);
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert!(wts[0].is_main, "first worktree should be main");
    }

    #[test]
    fn parse_worktree_list_multiple() {
        let input = "\
worktree /Users/me/project
HEAD abc123
branch refs/heads/main

worktree /Users/me/project/.workhive/worktrees/feat-a
HEAD def456
branch refs/heads/feat-a
";
        let wts = parse_worktree_list(input);
        assert_eq!(wts.len(), 2);
        assert!(wts[0].is_main, "first worktree should be main");
        assert!(!wts[1].is_main, "linked worktree should not be main");
        assert_eq!(wts[1].branch.as_deref(), Some("feat-a"));
    }

    #[test]
    fn parse_worktree_list_empty() {
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn parse_worktree_main_identified_positionally() {
        let input = "\
worktree /Users/me/project
HEAD abc123
branch refs/heads/main

worktree /Users/me/project/.workhive/worktrees/test01
HEAD def456
branch refs/heads/test01
";
        let wts = parse_worktree_list(input);
        assert_eq!(wts.len(), 2);
        assert!(wts[0].is_main);
        assert!(!wts[1].is_main);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[1].branch.as_deref(), Some("test01"));
    }

    #[test]
    fn parse_worktree_linked_at_nonstandard_path() {
        let input = "\
worktree /Users/me/project
HEAD abc123
branch refs/heads/main

worktree /tmp/custom-path/feat-b
HEAD def456
branch refs/heads/feat-b
";
        let wts = parse_worktree_list(input);
        assert_eq!(wts.len(), 2);
        assert!(wts[0].is_main);
        assert!(!wts[1].is_main);
        assert_eq!(wts[1].path.as_str(), "/tmp/custom-path/feat-b");
    }

    /// Regression: when executed from inside a linked worktree, the main
    /// worktree must still be correctly identified. The porcelain output
    /// is the same regardless of cwd — the first entry is always main.
    /// Previously, `repo_root()` was used as the comparator and would
    /// return the linked worktree's path, causing the linked worktree
    /// to be misidentified as main.
    #[test]
    fn parse_worktree_correct_when_cwd_is_linked_worktree() {
        // Simulate: user runs from /Users/me/project/.workhive/worktrees/feat-a
        // git worktree list --porcelain output is the same regardless of cwd.
        // Old code would call repo_root() which returns the linked worktree
        // path, making the comparison wrong. New code ignores repo_root entirely.
        let input = "\
worktree /Users/me/project
HEAD abc123
branch refs/heads/main

worktree /Users/me/project/.workhive/worktrees/feat-a
HEAD def456
branch refs/heads/feat-a

worktree /Users/me/project/.workhive/worktrees/feat-b
HEAD 789abc
branch refs/heads/feat-b
";
        let wts = parse_worktree_list(input);
        assert_eq!(wts.len(), 3);

        // First entry is always main, regardless of what repo_root() would return
        assert!(wts[0].is_main, "first entry must be main worktree");
        assert_eq!(wts[0].path.as_str(), "/Users/me/project");

        // Linked worktrees are never main
        assert!(!wts[1].is_main, "linked worktree feat-a must not be main");
        assert!(!wts[2].is_main, "linked worktree feat-b must not be main");
    }

    #[test]
    fn repo_discovery_error_maps_not_repo_to_typed_error() {
        let err = GitCliAdapter::classify_repo_discovery_error(
            "fatal: not a git repository (or any of the parent directories): .git",
        );
        assert!(matches!(err, GitError::NotARepo));
    }

    #[test]
    fn repo_discovery_error_preserves_other_git_failures() {
        let err = GitCliAdapter::classify_repo_discovery_error("fatal: ambiguous argument 'x'");
        match err {
            GitError::CommandFailed { message } => {
                assert!(message.contains("ambiguous argument"));
            }
            other => panic!("expected CommandFailed, got: {other:?}"),
        }
    }

    #[test]
    fn parse_status_line_tracks_staged_and_unstaged() {
        let changed = parse_status_line("MM src/lib.rs").expect("line should parse");
        assert_eq!(changed.status_code, "MM");
        assert!(changed.kinds.contains(&ChangeKind::Staged));
        assert!(changed.kinds.contains(&ChangeKind::Unstaged));
    }

    #[test]
    fn parse_status_line_tracks_untracked() {
        let changed = parse_status_line("?? notes/todo.md").expect("line should parse");
        assert_eq!(changed.path, "notes/todo.md");
        assert_eq!(changed.kinds, vec![ChangeKind::Untracked]);
    }

    /// A detached HEAD has no symbolic ref. Its failure must classify as a
    /// generic command failure, never as "not a git repository", so callers
    /// keep an accurate repository-state signal.
    #[test]
    fn branch_discovery_error_classifies_detached_head_as_command_failure() {
        let err =
            GitCliAdapter::classify_repo_discovery_error("fatal: ref HEAD is not a symbolic ref");
        match err {
            GitError::CommandFailed { message } => {
                assert!(message.contains("symbolic ref"));
            }
            other => panic!("expected CommandFailed, got: {other:?}"),
        }
    }

    #[test]
    fn branch_probe_failure_classifies_missing_ref_as_false() {
        assert!(
            !GitCliAdapter::classify_branch_probe_failure("").expect("empty stderr is missing ref")
        );
    }

    #[test]
    fn branch_probe_failure_classifies_not_repo_as_typed_error() {
        match GitCliAdapter::classify_branch_probe_failure(
            "fatal: not a git repository (or any of the parent directories): .git",
        ) {
            Err(GitError::NotARepo) => {}
            other => panic!("expected NotARepo, got: {other:?}"),
        }
    }

    #[test]
    fn branch_probe_failure_preserves_unexpected_git_error() {
        match GitCliAdapter::classify_branch_probe_failure("fatal: bad revision") {
            Err(GitError::CommandFailed { message }) => {
                assert!(message.contains("bad revision"));
            }
            other => panic!("expected CommandFailed, got: {other:?}"),
        }
    }

    #[test]
    fn branch_probe_failure_classifies_unborn_head_as_false() {
        assert!(
            !GitCliAdapter::classify_branch_probe_failure("").expect("empty stderr is no commit")
        );
    }

    /// Serializes process-wide working-directory mutation across the
    /// real-git tests below so concurrent `--ignored` runs cannot observe
    /// each other's `current_dir` changes.
    static CWD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Acquire the working-directory lock, tolerating poisoning so that a
    /// single failing test does not cascade into unrelated lock-poison
    /// panics across the other real-git tests.
    fn lock_cwd() -> std::sync::MutexGuard<'static, ()> {
        CWD_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct CwdGuard {
        previous: std::path::PathBuf,
    }

    impl CwdGuard {
        fn enter(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().expect("read cwd");
            std::env::set_current_dir(path).expect("enter test cwd");
            Self { previous }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    /// Real-git: a freshly initialized repository with no commits (unborn
    /// HEAD) must resolve to its initial branch name instead of failing
    /// with an ambiguous-HEAD error.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn current_branch_resolves_unborn_head() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        let branch = git
            .current_branch()
            .expect("unborn HEAD must resolve a branch name");
        assert!(!branch.is_empty(), "branch name must not be empty");
        assert_ne!(branch, "HEAD", "must not surface the raw HEAD token");
    }

    /// Real-git: a repository with at least one commit resolves its
    /// checked-out branch normally.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn current_branch_resolves_committed_branch() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        run_git_in(tmp.path(), &["config", "user.email", "t@example.com"]);
        run_git_in(tmp.path(), &["config", "user.name", "t"]);
        run_git_in(tmp.path(), &["commit", "--allow-empty", "-m", "init"]);

        let branch = git.current_branch().expect("committed branch resolves");
        assert!(!branch.is_empty(), "branch name must not be empty");
        assert_ne!(branch, "HEAD", "must not surface the raw HEAD token");
    }

    /// Real-git: a detached HEAD has no symbolic ref, so branch detection
    /// must return an error and never the literal `HEAD` token.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn current_branch_errors_on_detached_head() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        run_git_in(tmp.path(), &["config", "user.email", "t@example.com"]);
        run_git_in(tmp.path(), &["config", "user.name", "t"]);
        run_git_in(tmp.path(), &["commit", "--allow-empty", "-m", "c1"]);
        run_git_in(tmp.path(), &["commit", "--allow-empty", "-m", "c2"]);
        // Detach HEAD onto the first commit.
        run_git_in(tmp.path(), &["checkout", "--detach", "HEAD~1"]);

        if let Ok(branch) = git.current_branch() {
            panic!("detached HEAD must not resolve a branch, got: {branch:?}");
        }
    }

    /// Real-git: outside any repository, branch detection must fail with a
    /// typed not-a-repo error rather than an opaque command failure.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn current_branch_errors_outside_repository() {
        let tmp = tempfile::TempDir::new().expect("tempdir");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        match GitCliAdapter::new().current_branch() {
            Err(GitError::NotARepo) => {}
            other => panic!("expected NotARepo outside a repository, got: {other:?}"),
        }
    }

    /// Real-git: probing a non-existent branch in an unborn repository must
    /// resolve to `false` (not an error), so provisioning can proceed to
    /// create the branch without raw git failure noise.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn branch_exists_false_for_missing_branch_in_unborn_repo() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        assert!(
            !git.branch_exists("feat-x").expect("probe must not error"),
            "a missing branch must resolve to false"
        );
    }

    /// Real-git: a repository with an unborn HEAD has no commits, but this
    /// is a normal first-run state and must resolve to `false`.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn has_commits_false_for_unborn_repo() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        assert!(
            !git.has_commits()
                .expect("unborn repository probe must not error"),
            "a repository with no commits must resolve to false"
        );
    }

    /// Real-git: once a repository has at least one commit, HEAD resolves
    /// to an object and the commit probe returns `true`.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn has_commits_true_for_committed_repo() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        run_git_in(tmp.path(), &["config", "user.email", "t@example.com"]);
        run_git_in(tmp.path(), &["config", "user.name", "t"]);
        run_git_in(tmp.path(), &["commit", "--allow-empty", "-m", "init"]);

        assert!(
            git.has_commits()
                .expect("committed repository probe must not error"),
            "a repository with a commit must resolve to true"
        );
    }

    /// Real-git: outside any repository, the commit probe must preserve the
    /// repository discovery error instead of returning a false readiness
    /// signal.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn has_commits_errors_outside_repository() {
        let tmp = tempfile::TempDir::new().expect("tempdir");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        match GitCliAdapter::new().has_commits() {
            Err(GitError::NotARepo) => {}
            other => panic!("expected NotARepo outside a repository, got: {other:?}"),
        }
    }

    /// Real-git: outside any repository, branch existence probing must
    /// preserve the repository discovery error instead of silently treating
    /// it as a missing branch.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn branch_exists_errors_outside_repository() {
        let tmp = tempfile::TempDir::new().expect("tempdir");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        match GitCliAdapter::new().branch_exists("feat-x") {
            Err(GitError::NotARepo) => {}
            other => panic!("expected NotARepo outside a repository, got: {other:?}"),
        }
    }

    /// Real-git: an existing branch resolves to `true`; a missing one to
    /// `false`.
    #[test]
    #[ignore] // real-git: requires the system git binary
    fn branch_exists_true_for_existing_branch() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let root = Utf8Path::from_path(tmp.path()).expect("utf8 tempdir");
        let git = GitCliAdapter::new();
        git.init_repo(root).expect("git init");

        let _lock = lock_cwd();
        let _cwd = CwdGuard::enter(tmp.path());
        run_git_in(tmp.path(), &["config", "user.email", "t@example.com"]);
        run_git_in(tmp.path(), &["config", "user.name", "t"]);
        run_git_in(tmp.path(), &["commit", "--allow-empty", "-m", "init"]);
        let current = git.current_branch().expect("branch after commit");

        assert!(
            git.branch_exists(&current).expect("probe must not error"),
            "the committed branch must resolve to true"
        );
        assert!(
            !git.branch_exists("definitely-missing")
                .expect("probe must not error"),
            "a missing branch must resolve to false"
        );
    }

    fn run_git_in(dir: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("run git");
        assert!(
            status.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }
}

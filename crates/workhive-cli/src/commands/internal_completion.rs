//! Hidden completion-query handler.
//!
//! Wired up by Workhive's generated shell completion scripts. The shell
//! invokes `workhive internal completion candidates <kind>` whenever the
//! user requests dynamic candidates (workspace names, pane targets).
//! This command must obey three contracts:
//!
//! 1. **Read-only.** No filesystem writes, no tmux/git mutations, no
//!    config mutations.
//! 2. **Failure-tolerant.** Missing project, missing tmux server, IO
//!    errors, permission errors, locale resolution failures all degrade
//!    to a silent empty result. The completion UI must never see a
//!    Workhive error block.
//! 3. **Stdout-only.** Output is one candidate per line on stdout. No
//!    stderr noise, no headers, no trailing prompts.
//!
//! Architecture: this module owns only the IO/adapter wiring and policy
//! choices specific to "we are running inside a tab-completion query."
//! All candidate computation lives in
//! [`workhive_core::usecases::completion`].
use std::io::{self, Write};

use camino::Utf8PathBuf;
use workhive_core::ports::{EnvironmentPort, GitPort};
use workhive_core::usecases;

use crate::cli::CompletionCandidateKind;
use crate::composition::CliComposition;
use crate::error::CliError;

/// Handle `workhive internal completion candidates <kind>`.
///
/// Always returns `Ok(())`. Failures are intentionally swallowed and
/// turned into a silent empty stdout to keep the user's tab-completion
/// UI quiet.
pub(crate) fn cmd_internal_completion_candidates(
    kind: CompletionCandidateKind,
) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let candidates = resolve_candidates(kind);
    write_lines(&mut handle, &candidates);
    Ok(())
}

/// Dispatch table for the candidate kinds. Kept at top-level so unit
/// tests can compose alternate adapters.
fn resolve_candidates(kind: CompletionCandidateKind) -> Vec<String> {
    match kind {
        CompletionCandidateKind::Workspace => safe_resolve(resolve_workspace_candidates),
        CompletionCandidateKind::Pane => safe_resolve(resolve_pane_candidates),
    }
}

/// Run a candidate resolver with global panic safety. A panic during
/// candidate resolution must NOT take the user's shell down.
fn safe_resolve<F>(f: F) -> Vec<String>
where
    F: FnOnce() -> Vec<String> + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(f).unwrap_or_default()
}

/// Resolve workspace candidates by reading the current project's
/// `.workhive/worktrees/` listing.
///
/// Returns an empty vector when:
/// - cwd is not inside any registered project,
/// - the project is not initialized,
/// - the worktrees directory does not exist or is unreadable,
/// - any unexpected error occurs.
fn resolve_workspace_candidates() -> Vec<String> {
    let comp = CliComposition::new();
    let project_root = match find_project_root_quietly(&comp) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let ws_fs = comp.workspace_fs();
    usecases::completion::workspace_candidates(&ws_fs, &project_root)
}

/// Resolve pane candidates for the workspace currently visible in the
/// caller's tmux environment.
///
/// We require both `TMUX_*` env detection (to know we are inside a real
/// tmux session) AND a Workhive project root (to validate the session
/// belongs to Workhive). When either is missing we return empty rather
/// than guess. This keeps `pane send <TAB>` quiet outside live
/// workspaces.
fn resolve_pane_candidates() -> Vec<String> {
    let comp = CliComposition::new();
    let env = comp.environment();
    let exec = match env.execution_context() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let (server, session) = match exec.tmux {
        workhive_core::ports::TmuxMembership::Resolved {
            server, session, ..
        } => (server, session),
        _ => return Vec::new(),
    };
    let root = match find_project_root_quietly(&comp) {
        Some(root) => root,
        None => return Vec::new(),
    };
    let runtime = match comp.runtime_for_workspace(&root, &server, &session) {
        Ok(runtime) => runtime,
        Err(_) => return Vec::new(),
    };
    usecases::completion::pane_candidates(runtime.as_ref(), &server, &session)
}

/// Find the Workhive project root for the calling shell, without
/// initializing git, prompting the user, or printing diagnostics.
///
/// Strategy: ask `GitPort::repo_root()` directly. If we are not inside
/// a git repo or the call fails for any reason, return None. Workhive's
/// project model is anchored on git repos, so a missing repo means no
/// project and therefore no candidates.
fn find_project_root_quietly(comp: &CliComposition) -> Option<Utf8PathBuf> {
    let git = comp.git();
    git.repo_root().ok()
}

/// Write candidates one-per-line, swallowing writer errors. We never
/// surface IO errors to the shell; an interrupted pipe is part of
/// normal completion behavior.
fn write_lines<W: Write>(writer: &mut W, candidates: &[String]) {
    for line in candidates {
        let _ = writeln!(writer, "{line}");
    }
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_lines_emits_one_candidate_per_line() {
        let mut buf: Vec<u8> = Vec::new();
        write_lines(&mut buf, &["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        let text = String::from_utf8(buf).expect("utf-8");
        assert_eq!(text, "a\nb\nc\n");
    }

    #[test]
    fn write_lines_emits_nothing_for_empty_input() {
        let mut buf: Vec<u8> = Vec::new();
        write_lines(&mut buf, &[]);
        assert!(buf.is_empty());
    }

    #[test]
    fn safe_resolve_recovers_from_panic() {
        let candidates = safe_resolve(|| panic!("simulated resolver panic"));
        assert!(candidates.is_empty());
    }

    #[test]
    fn cmd_internal_completion_workspace_candidates_returns_ok_outside_project() {
        // When invoked from a directory that is not a Workhive project,
        // the handler must succeed silently with no output. We can't
        // easily redirect stdout from a unit test on stable Rust, but
        // we *can* assert that the entry point returns Ok and never
        // panics. The integration test exercises the actual binary.
        let result = cmd_internal_completion_candidates(CompletionCandidateKind::Workspace);
        assert!(
            result.is_ok(),
            "internal completion handler must not surface errors: {result:?}",
        );
    }

    #[test]
    fn cmd_internal_completion_pane_candidates_returns_ok_outside_project() {
        let result = cmd_internal_completion_candidates(CompletionCandidateKind::Pane);
        assert!(
            result.is_ok(),
            "internal completion handler must not surface errors: {result:?}",
        );
    }
}

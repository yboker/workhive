//! CLI integration tests using `assert_cmd`.
//!
//! These tests exercise the compiled binary to verify:
//! - Help output renders correctly
//! - Version flag works
//! - Exit codes match the exit-code contract
//! - Subcommand routing is correct
//!
//! ## Test gate strategy
//!
//! Tests that create or query real tmux sessions are marked `#[ignore]`.
//! They run only via `cargo test -p workhive-cli --test cli_integration -- --ignored`
//! (the "real-tmux gate"), keeping `cargo test --workspace` (the "default
//! gate") fast and deterministic without a running tmux server.
//!
//! Each `#[ignore]` test that creates a tmux session holds a [`TmuxTestGuard`]
//! that kills the per-test tmux server on drop, even if the test panics.
//! The guard must be created immediately after the session-creating command
//! succeeds and before any assertions that could panic.

use assert_cmd::Command;
use camino::Utf8PathBuf;
use predicates::prelude::*;
use serde_json::Value;
use workhive_core::domain::SessionNamingPolicy;
use workhive_core::ports::ProjectIndexPort;
use workhive_core::usecases::naming::assemble_naming_context;

/// RAII guard that kills a tmux server on drop, preventing PTY device leaks.
///
/// Every test that creates a tmux session via `open --no-attach` MUST hold
/// a `TmuxTestGuard`. The guard kills the server even if the test panics.
/// When created via `with_project_dir`, it also cleans linked git worktrees
/// under `.workhive/worktrees/*` and deletes their corresponding local
/// branches in that test repo.
struct TmuxTestGuard {
    tmpdir: tempfile::TempDir,
    project_dir: Option<std::path::PathBuf>,
}

impl TmuxTestGuard {
    fn new() -> Self {
        Self {
            tmpdir: tempfile::TempDir::new().unwrap(),
            project_dir: None,
        }
    }

    fn with_project_dir(project_dir: &std::path::Path) -> Self {
        Self {
            tmpdir: tempfile::TempDir::new().unwrap(),
            project_dir: Some(project_dir.to_path_buf()),
        }
    }

    fn path(&self) -> &std::path::Path {
        self.tmpdir.path()
    }
}

impl Drop for TmuxTestGuard {
    fn drop(&mut self) {
        let _ = std::process::Command::new("tmux")
            .env("TMUX_TMPDIR", self.tmpdir.path())
            .args(["kill-server"])
            .output();
        if let Some(project_dir) = &self.project_dir {
            cleanup_test_git_side_effects(project_dir);
        }
    }
}

fn cleanup_test_git_side_effects(project_dir: &std::path::Path) {
    let worktrees_root = project_dir.join(".workhive").join("worktrees");
    let mut branch_candidates: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&worktrees_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let branch_name = entry.file_name().to_string_lossy().to_string();
            branch_candidates.push(branch_name);

            let _ = std::process::Command::new("git")
                .arg("-C")
                .arg(project_dir)
                .arg("worktree")
                .arg("remove")
                .arg("--force")
                .arg(&path)
                .output();
        }
    }

    for branch in branch_candidates {
        let _ = std::process::Command::new("git")
            .arg("-C")
            .arg(project_dir)
            .arg("branch")
            .arg("-D")
            .arg(branch)
            .output();
    }

    let _ = std::fs::remove_dir_all(&worktrees_root);
}

fn tmux_has_session(guard: &TmuxTestGuard, session: &str) -> bool {
    std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", guard.path())
        .args(["has-session", "-t", session])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn tmux_has_session_on_server(guard: &TmuxTestGuard, server: &str, session: &str) -> bool {
    std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", guard.path())
        .args(["-L", server, "has-session", "-t", session])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn workhive() -> Command {
    Command::cargo_bin("workhive").expect("binary should exist")
}

fn workhive_with_home(home: &std::path::Path) -> Command {
    let mut cmd = workhive();
    cmd.env("HOME", home);
    cmd.env("XDG_CONFIG_HOME", home.join(".config"));
    cmd
}

fn fake_tool_script(tool: &str) -> &'static str {
    match tool {
        "git" => "#!/bin/sh\necho 'git version 2.50.0'\n",
        "tmux" => "#!/bin/sh\necho 'tmux 3.5a'\n",
        _ => "#!/bin/sh\nexit 0\n",
    }
}

fn add_fake_tools(cmd: &mut Command, root: &std::path::Path, tools: &[&str]) {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = root.join("fake-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    for tool in tools {
        let path = bin_dir.join(tool);
        std::fs::write(&path, fake_tool_script(tool)).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(bin_dir).chain(std::env::split_paths(&current_path));
    cmd.env("PATH", std::env::join_paths(paths).unwrap());
}

fn add_fake_tools_with_isolated_path(cmd: &mut Command, root: &std::path::Path, tools: &[&str]) {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = root.join("fake-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    for tool in tools {
        let path = bin_dir.join(tool);
        std::fs::write(&path, fake_tool_script(tool)).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }

    cmd.env("PATH", bin_dir);
}

/// Create a tempdir with global config initialized (hermetic HOME).
///
/// The trusted-roots admission policy gates project init / `workspace open
/// --init` / worktree provisioning. Test fixtures that drive those mutating
/// flows must register their temp root as a trusted root, so we centralize
/// that here. `--force` bypasses the overly-broad-path heuristic for the
/// canonicalized tempdir (which on macOS sits under `/private/var/...`).
fn setup_hermetic_home() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), tmp.path());
    tmp
}

/// Sibling of [`setup_hermetic_home`] that **does not** pre-trust the
/// temp HOME directory. Use this for tests that exercise
/// `project roots add`, `project roots remove`, or `project roots list`
/// against paths chosen by the test itself.
///
/// Why this exists: `setup_hermetic_home` auto-trusts `tmp.path()` so
/// that workspace operations inside `tmp.path()/<project>` succeed
/// without each test threading a project-specific `roots add` call.
/// That convenience is incompatible with the
/// `add_trusted_root_redundancy_check` rule,
/// which (correctly) rejects any descendant of an already-trusted root
/// — including the test fixtures' synthetic `dev-roots` children.
/// Tests that need to drive the trust insertion themselves must opt
/// in to this no-auto-trust variant.
fn setup_hermetic_home_without_auto_trust() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    tmp
}

/// Register `path` as a trusted project root in the hermetic global config
/// rooted at `home`. Uses `--force` so deeply nested tempdir parents (which
/// on macOS resolve under `/private/var/...`) are admitted past the heuristic
/// guard for overly broad roots.
fn register_trusted_root(home: &std::path::Path, path: &std::path::Path) {
    workhive_with_home(home)
        .args([
            "project",
            "roots",
            "add",
            path.to_str().expect("trusted root must be valid UTF-8"),
            "--force",
        ])
        .assert()
        .success();
}

/// Create a hermetic HOME with a git project initialized inside it.
fn setup_hermetic_project(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = setup_hermetic_home();
    let project_dir = tmp.path().join(name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    (tmp, project_dir)
}

fn setup_hermetic_project_with_subdir(
    repo_name: &str,
    subdir_name: &str,
) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let (tmp, project_dir) = setup_hermetic_project(repo_name);
    let subdir = project_dir.join(subdir_name);
    std::fs::create_dir_all(&subdir).expect("subdir should be created");
    (tmp, project_dir, subdir)
}

/// Create a hermetic HOME with a fully initialized workhive project so
/// `pane` subcommands pass project discovery and trust preflight and
/// reach their in-session check.
fn setup_initialized_project(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let (tmp, project_dir) = setup_hermetic_project(name);
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    (tmp, project_dir)
}

/// Run a `pane` subcommand from inside `project_dir` with the tmux
/// environment variables cleared, so the in-session preflight
/// deterministically observes that the shell is not inside a
/// workhive-managed session regardless of the host shell the test runs
/// under.
fn run_pane_outside_session(
    home: &std::path::Path,
    project_dir: &std::path::Path,
    tmux_tmpdir: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    workhive_with_home(home)
        .env_remove("TMUX")
        .env_remove("TMUX_PANE")
        .env_remove("TMUX_SESSION")
        .env("TMUX_TMPDIR", tmux_tmpdir)
        .args(args)
        .current_dir(project_dir)
        .output()
        .expect("pane command should run")
}

fn expected_runtime_names(
    project_dir: &std::path::Path,
    current_dir: &std::path::Path,
    workspace: &str,
) -> (String, String) {
    let project_root =
        Utf8PathBuf::from_path_buf(project_dir.to_path_buf()).expect("project dir must be utf-8");
    let cwd =
        Utf8PathBuf::from_path_buf(current_dir.to_path_buf()).expect("cwd must be valid utf-8");
    let project_name = project_root
        .file_name()
        .expect("project dir must have file name")
        .to_owned();
    let instance_id = read_instance_id(project_dir);
    let policy = SessionNamingPolicy::default_policy();
    let relative_path =
        workhive_core::usecases::naming::resolve_subdir_key(&project_root, Some(cwd.as_ref()));
    let naming_ctx = assemble_naming_context(
        &project_name,
        workspace,
        &project_root,
        Some("main".to_owned()),
        Some(instance_id),
        relative_path,
    );
    let server = policy
        .resolve_server(&naming_ctx)
        .expect("server name must resolve");
    let session = policy
        .resolve_session(&naming_ctx)
        .expect("session name must resolve");
    (server, session)
}

// -- Help output --

/// Parse subcommand names from clap-rendered `--help` output.
///
/// Reads stdout, locates command heading sections, and returns each command
/// line's leading token. Stops at `Options:`. This avoids false positives from
/// substring matching against description text such as
/// "Manage active workspace sessions".
fn parse_subcommands_from_help(stdout: &str) -> Vec<String> {
    let mut commands: Vec<String> = Vec::new();
    let mut in_section = false;
    for raw_line in stdout.lines() {
        let trimmed = raw_line.trim();
        if trimmed.ends_with("Commands:") || trimmed == "系统命令:" || trimmed == "资源命令:"
        {
            in_section = true;
            continue;
        }
        if trimmed == "Options:" || trimmed == "选项:" {
            break;
        }
        if !in_section || trimmed.is_empty() {
            continue;
        }
        if !raw_line.starts_with("  ") {
            in_section = false;
            continue;
        }
        let mut parts = raw_line.split_whitespace();
        let Some(first) = parts.next() else {
            continue;
        };
        let name = if first == "workhive" {
            parts.next().unwrap_or(first)
        } else {
            first
        };
        if name != "workhive" {
            commands.push(name.to_owned());
        }
    }
    commands
}

fn help_stdout_for(args: &[&str]) -> String {
    // Use a hermetic HOME so the developer's real `~/.config/workhive/config.toml`
    // (which may select a non-English locale) never leaks into help-rendering
    // assertions that expect canonical English output.
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME for help test");
    let output = workhive_with_home(tmp.path())
        .args(args)
        .output()
        .expect("workhive binary should run");
    assert!(
        output.status.success(),
        "expected success for {args:?}, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help stdout should be utf-8")
}

/// Top-level command names that **must remain rejected** by the parser.
///
/// Worktree management lives only under `workhive workspace worktree …`.
/// Top-level `worktree` is not a valid subcommand, so it is rejected as an
/// unknown subcommand here. `wt` is likewise rejected because that alias is
/// intentionally not exposed.
const REMOVED_TOP_LEVEL_COMMANDS: &[&str] = &[
    "open",
    "attach",
    "close",
    "list",
    "ls",
    "destroy",
    "worktree",
    "wt",
    "projects",
    "sessions",
    "reindex",
    "snapshot",
    // The `workhive integrations` CLI surface was removed entirely;
    // it must not appear in `--help` and any explicit invocation
    // must be rejected as an unknown subcommand.
    "integrations",
];

#[test]
fn top_level_help_shows_only_new_public_commands() {
    let stdout = help_stdout_for(&["--help"]);
    assert!(
        stdout.contains("Terminal workspace manager"),
        "top-level help should keep about line, got: {stdout}"
    );
    assert!(
        stdout.contains("System Commands:"),
        "top-level help should group system commands, got: {stdout}"
    );
    assert!(
        stdout.contains("Resource Commands:"),
        "top-level help should group resource commands, got: {stdout}"
    );

    let commands = parse_subcommands_from_help(&stdout);

    for expected in [
        "init",
        "config",
        "doctor",
        "project",
        "workspace",
        "session",
        "pane",
    ] {
        assert!(
            commands.iter().any(|c| c == expected),
            "top-level help missing public command `{expected}`; got {commands:?}"
        );
    }

    // `internal` is hidden — never advertised in normal help.
    assert!(
        !commands.iter().any(|c| c == "internal"),
        "top-level help must not list hidden `internal` command; got {commands:?}"
    );

    // `help` is not a supported subcommand: the canonical help entries are
    // `workhive --help` and `workhive <command> --help`. It must never be
    // advertised in the top-level command list.
    assert!(
        !commands.iter().any(|c| c == "help"),
        "top-level help must not advertise a `help` subcommand; got {commands:?}"
    );

    // None of the removed top-level commands may appear as subcommand
    // entries in either custom root-help section.
    for forbidden in REMOVED_TOP_LEVEL_COMMANDS {
        assert!(
            !commands.iter().any(|c| c == forbidden),
            "top-level help must not list removed command `{forbidden}`; got {commands:?}"
        );
    }
}

#[test]
fn zh_cn_top_level_help_shows_only_current_public_commands() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["--help"]);
    let commands = parse_subcommands_from_help(&stdout);

    for expected in [
        "init",
        "config",
        "doctor",
        "completion",
        "project",
        "workspace",
        "session",
        "pane",
    ] {
        assert!(
            commands.iter().any(|c| c == expected),
            "zh-CN top-level help missing public command `{expected}`; got {commands:?}"
        );
    }

    for forbidden in REMOVED_TOP_LEVEL_COMMANDS {
        assert!(
            !commands.iter().any(|c| c == forbidden),
            "zh-CN top-level help must not list removed command `{forbidden}`; got {commands:?}"
        );
    }
}

/// `workhive help` is intentionally not a subcommand. Invoking it must
/// fail as an unknown subcommand and guide the user to the canonical
/// `workhive --help` entry point, in English.
#[test]
fn workhive_help_is_rejected_and_points_to_help_flag() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    let output = workhive_with_home(tmp.path())
        .arg("help")
        .output()
        .expect("workhive binary should run");
    assert!(
        !output.status.success(),
        "`workhive help` must not succeed as a subcommand"
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("--help"),
        "rejecting `workhive help` must guide users to `--help`, got:\n{stderr}"
    );
}

/// zh-CN top-level help must document the `--help` flag path and must not
/// advertise a `workhive help` subcommand.
#[test]
fn zh_cn_top_level_help_uses_help_flag_not_help_subcommand() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["--help"]);
    assert!(
        !stdout.contains("workhive help"),
        "zh-CN top-level help must not advertise `workhive help`, got:\n{stdout}"
    );
    assert!(
        stdout.contains("--help"),
        "zh-CN top-level help must document the `--help` flag path, got:\n{stdout}"
    );
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    workhive()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("workhive"));
}

// -- Subcommand help --

#[test]
fn config_help_shows_subcommands() {
    workhive()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("unset"))
        .stdout(predicate::str::contains("env-set"))
        .stdout(predicate::str::contains("env-unset"));
}

#[test]
fn pane_help_shows_subcommands() {
    workhive()
        .args(["pane", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("split"))
        .stdout(predicate::str::contains("close"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("send"));
}

/// `pane show --help` must advertise the optional target argument so the
/// help surface matches the hints elsewhere that tell users to run
/// `workhive pane show <pane>`. The positional is optional (`[PANE]`) and
/// the help text documents both the pane-number and target-id forms.
#[test]
fn pane_show_help_documents_optional_target() {
    workhive()
        .args(["pane", "show", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[PANE]"))
        .stdout(predicate::str::contains("pane number"))
        .stdout(predicate::str::contains("target id"));
}

#[test]
fn workspace_help_shows_remove_command() {
    let stdout = help_stdout_for(&["workspace", "--help"]);
    let commands = parse_subcommands_from_help(&stdout);
    for expected in ["list", "open", "attach", "close", "remove", "destroy"] {
        assert!(
            commands.iter().any(|c| c == expected),
            "workspace help missing `{expected}`; got {commands:?}"
        );
    }
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    workhive_with_home(tmp.path())
        .args(["workspace", "remove", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Remove"));
}

#[test]
fn project_help_lists_list_and_reindex() {
    let stdout = help_stdout_for(&["project", "--help"]);
    let commands = parse_subcommands_from_help(&stdout);
    for expected in ["list", "reindex"] {
        assert!(
            commands.iter().any(|c| c == expected),
            "project help missing `{expected}`; got {commands:?}"
        );
    }
}

#[test]
fn session_help_lists_list_and_attach() {
    let stdout = help_stdout_for(&["session", "--help"]);
    let commands = parse_subcommands_from_help(&stdout);
    assert!(
        commands.iter().any(|c| c == "list"),
        "session help missing `list`; got {commands:?}"
    );
    // `session attach` is the global-scope attach command; it must be listed
    // alongside `session list` so users can discover it.
    assert!(
        commands.iter().any(|c| c == "attach"),
        "session help must list `attach`; got {commands:?}"
    );
}

#[test]
fn removed_top_level_commands_are_rejected_with_exit_code_2() {
    // Use a hermetic HOME so the parse-error locale defaults to English
    // and is not contaminated by the developer's local global config
    // (e.g. `language = "zh-CN"`). Locale-specific parse errors are
    // covered by dedicated zh_cn_* tests below.
    let tmp = tempfile::TempDir::new().unwrap();
    for cmd in REMOVED_TOP_LEVEL_COMMANDS {
        workhive_with_home(tmp.path())
            .arg(cmd)
            .assert()
            .code(2)
            .stderr(predicate::str::contains("unrecognized subcommand"));
    }
}

// -- Implemented commands run in a git repo --

#[test]
fn list_in_initialized_git_repo_succeeds() {
    let (tmp, project_dir) = setup_hermetic_project("list-repo");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success();
}

#[test]
fn config_show_prints_config_source_line() {
    let (tmp, project_dir) = setup_hermetic_project("config-show-source");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "show"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("Config source:"))
        .stderr(predicate::str::contains(".workhive/config.toml"));
}

#[test]
fn list_prints_config_source_line_and_workspace_overview_columns() {
    let (tmp, project_dir) = setup_hermetic_project("list-source-panes");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Current overview contract: the project-root section's identity
    // column is "Branch" (the repository's main branch); the worktree
    // section's "Workspace" column only renders when worktree workspaces
    // exist. The recency column is labelled "Last used".
    let output = workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("Config source:"))
        .stderr(predicate::str::contains(".workhive/config.toml"))
        .stdout(predicate::str::contains("Branch"))
        .stdout(predicate::str::contains("Session"))
        .stdout(predicate::str::contains("Layout"))
        .stdout(predicate::str::contains("Last used"))
        .stdout(predicate::str::contains("Project root"))
        .stdout(predicate::str::contains("Worktree workspaces"))
        .stdout(predicate::str::contains("Status").not())
        .stdout(predicate::str::contains("Panes").not())
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout).expect("stdout must be utf-8");
    // The main row's session column must surface one of the four
    // session-availability values introduced by the Session/Status
    // rename — never the legacy `Active`/`Inactive`/`Not created` copy.
    let session_values = ["running", "no session", "unreachable", "lookup failed"];
    assert!(
        session_values.iter().any(|value| stdout.contains(value)),
        "expected one of {session_values:?} in workspace list stdout but got: {stdout}"
    );
    for legacy in ["Active", "Inactive", "Not created"] {
        assert!(
            !stdout.contains(legacy),
            "legacy session label `{legacy}` must not appear in workspace list stdout: {stdout}"
        );
    }
}

#[test]
fn list_view_json_outputs_machine_readable_payload() {
    let (tmp, project_dir) = setup_hermetic_project("list-json-view");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("list --view json should execute");

    assert!(output.status.success(), "json view should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("valid json payload");
    assert!(payload.get("config_source").is_some());
    assert!(payload.get("workspaces").is_some());

    let workspaces = payload["workspaces"]
        .as_array()
        .expect("workspaces must be an array");
    assert!(
        !workspaces.is_empty(),
        "expected at least one workspace entry"
    );

    let first = &workspaces[0];
    assert!(
        first.get("pane_count_status").is_some(),
        "json output must expose pane_count_status explicitly"
    );
}

#[test]
fn list_view_compact_uses_key_value_style_output() {
    let (tmp, project_dir) = setup_hermetic_project("list-compact-view");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["workspace", "list", "--view", "compact"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Session:"))
        .stdout(predicate::str::contains("Layout:"))
        .stdout(predicate::str::contains("Last used:"))
        .stdout(predicate::str::contains("Project root"))
        .stdout(predicate::str::contains("Worktree workspaces"))
        .stdout(predicate::str::contains("Server unreachable").not())
        .stdout(predicate::str::contains("Status:").not())
        .stdout(predicate::str::contains("Workspace").not());
}

#[test]
fn list_view_table_uses_workspace_overview_columns() {
    let (tmp, project_dir) = setup_hermetic_project("list-table-overview");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["workspace", "list", "--view", "table"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Branch"))
        .stdout(predicate::str::contains("Session"))
        .stdout(predicate::str::contains("Layout"))
        .stdout(predicate::str::contains("Last used"))
        .stdout(predicate::str::contains("Project root"))
        .stdout(predicate::str::contains("Worktree workspaces"))
        .stdout(predicate::str::contains("Server unreachable").not())
        .stdout(predicate::str::contains("Panes").not())
        .stdout(predicate::str::contains("Status").not());
}

#[test]
fn list_in_uninitialized_repo_shows_main_branch() {
    let (tmp, project_dir) = setup_hermetic_project("list-uninit");
    workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("main"));
}

/// Regression:
/// default `workspace list` MUST NOT leak the raw
/// `WARN state-key resolution failed...` tracing line into normal user
/// output. State-key resolution returning `None` for a not-yet-provisioned
/// workspace is an *expected* degraded state and must be surfaced as the
/// localized "unknown" snapshot status, not as a warning log.
#[test]
fn list_does_not_leak_raw_state_key_warning_on_uninitialized_workspace() {
    let (tmp, project_dir) = setup_hermetic_project("list-no-warn-leak");
    let output = workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace list should run");
    assert!(
        output.status.success(),
        "workspace list should succeed in uninitialized repo"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stderr.contains("state-key resolution failed"),
        "default list output must not surface raw state-key resolution warnings; stderr: {stderr}"
    );
    assert!(
        !stdout.contains("state-key resolution failed"),
        "default list output must not surface raw state-key resolution warnings; stdout: {stdout}"
    );
}

/// Regression: default human output
/// MUST NOT show unexplained `?` or `-` for the snapshot column. Snapshot
/// status must render as localized text (available / no snapshot /
/// unknown).
#[test]
fn list_default_output_omits_snapshot_column() {
    let (tmp, project_dir) = setup_hermetic_project("list-snapshot-localized");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["workspace", "list", "--view", "table"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace list table should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The snapshot column MUST contain one of the localized status texts
    // for the main entry (which has no snapshot in a fresh init):
    // either "no snapshot" or "unknown" — never the raw `?` or `-`
    // glyphs that originally shipped.
    assert!(!stdout.contains("Snapshot"));
}

/// Regression: JSON output remains backward-compatible after the human
/// output UX rework. JSON is the contract for automation; the rename of
/// snapshot.yes / snapshot.none / snapshot.unknown applied to the
/// human-facing strings only and must not bleed into the JSON shape.
#[test]
fn list_json_output_omits_snapshot_status_field() {
    let (tmp, project_dir) = setup_hermetic_project("list-json-backcompat");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace list json should run");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("valid json payload");
    let workspaces = payload["workspaces"]
        .as_array()
        .expect("workspaces array required");
    assert!(!workspaces.is_empty(), "expected at least one workspace");
    let first = &workspaces[0];

    assert!(first.get("pane_count_status").is_some());
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn list_shows_table_after_open() {
    // Default `workspace list` is a workspace-centric human-readable table.
    // The contract is the column set: Workspace / Status / Layout / Snapshot / Recent.
    // Implementation details such as raw worktree paths, session identity,
    // or the `Updated` timestamp column belong in JSON output or in the
    // dedicated `session` command — not in this default table.
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("listproj{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-list", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .output()
        .expect("list should execute");

    let stdout = String::from_utf8_lossy(&output.stdout);

    for header in ["Workspace", "Session", "Layout", "Last used"] {
        assert!(
            stdout.contains(header),
            "missing required column header `{header}` in default workspace list output. stdout: {stdout}"
        );
    }

    for group in ["Project root", "Worktree workspaces"] {
        assert!(
            stdout.contains(group),
            "missing workspace group header `{group}` in default workspace list output. stdout: {stdout}"
        );
    }

    for legacy_header in ["Status", "Updated"] {
        assert!(
            !stdout.contains(legacy_header),
            "default workspace list must not expose legacy column `{legacy_header}`. stdout: {stdout}"
        );
    }

    assert!(
        stdout.contains("feat-list"),
        "missing workspace name in data row. stdout: {stdout}"
    );
    assert!(
        stdout.contains("running") || stdout.contains("no session"),
        "missing session availability in data row. stdout: {stdout}"
    );
}

#[test]
fn list_all_projects_shows_registered_project() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("listallproj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["project", "list"])
        .current_dir(&project_dir)
        .output()
        .expect("list --all-projects should execute");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify column headers
    assert!(stdout.contains("Project"), "missing Project column header");
    assert!(stdout.contains("Root"), "missing Root column header");

    // Verify data row
    assert!(
        stdout.contains("listallproj"),
        "list --all-projects should show project name"
    );
}

#[test]
fn list_all_projects_compact_view_uses_key_value_style_output() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("listallcompactproj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["project", "list", "--view", "compact"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("listallcompactproj"))
        .stdout(predicate::str::contains("Root:"))
        .stdout(predicate::str::contains("Project").not());
}

#[test]
fn project_list_is_index_backed_without_runtime() {
    // `project list` is a fast, index-backed global query: it reads the project
    // index only and never contacts the tmux runtime backend. `init` registers
    // the project in the index without creating any tmux session, so a
    // successful listing here proves the path does not require a reachable
    // runtime (no `workspace open` and no running tmux server involved).
    let (tmp, project_dir) = setup_initialized_project("projectlistindexbacked");

    workhive_with_home(tmp.path())
        .args(["project", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("projectlistindexbacked"));
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn project_list_reflects_open_after_index_entry_lost_without_reindex() {
    // Invariant: a successful `workspace open` on an already-initialized
    // project whose global index entry is missing/stale re-registers it, so
    // `project list` shows it again. Open alone restores global discovery;
    // `project reindex` is not required to make an opened project visible.
    let (tmp, project_dir) = setup_initialized_project("reopenindexproj");

    // Simulate a lost/stale global index entry while the project's `.workhive/`
    // runtime stays initialized on disk (the already-initialized state).
    let index_path = tmp.path().join(".config/workhive/index.json");
    std::fs::remove_file(&index_path).expect("index.json should exist after project init");

    // Guard before the mutating open so a panic still tears down the tmux server.
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-reindex", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Open alone restores discoverability; `project reindex` is not required.
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["project", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("reopenindexproj"));
}

#[test]
#[ignore] // real-tmux: opens one project's session; a second indexed project has no running server
fn session_list_degrades_when_one_indexed_project_backend_unavailable() {
    // Invariant (resilience): with two indexed projects where one has a running
    // tmux server and the other does not, `session list` still succeeds and
    // lists the reachable project's session while omitting the unavailable
    // project from normal output. Diagnostic details remain available through
    // debug logging.
    let (tmp, proj_available) = setup_initialized_project("availproj");

    // Second project: initialized + indexed, but never opened, so it has no
    // running tmux server (its runtime backend socket is absent).
    let proj_unavailable = tmp.path().join("downproj");
    std::fs::create_dir_all(&proj_unavailable).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&proj_unavailable)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&proj_unavailable)
        .output()
        .expect("git commit should succeed");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&proj_unavailable)
        .assert()
        .success();

    // Open a real session only for the available project.
    let guard = TmuxTestGuard::with_project_dir(&proj_available);
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-avail", "--no-attach"])
        .current_dir(&proj_available)
        .assert()
        .success();

    // Global session list must not abort just because `downproj` has no running
    // backend; it lists the reachable session and exits successfully.
    let output = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["session", "list"])
        .current_dir(&proj_available)
        .output()
        .expect("session list should execute");
    assert!(
        output.status.success(),
        "session list must succeed despite one project's backend being unavailable. stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("availproj"),
        "reachable project's session must still be listed. stdout: {stdout}, stderr: {stderr}"
    );

    assert!(
        !stderr.contains("downproj"),
        "unavailable project must not appear in normal stderr output: {stderr}"
    );
    assert!(
        !stderr.contains("runtime backend unavailable")
            && !stderr.contains("backend not available"),
        "backend-unavailable warning must not appear in normal output: {stderr}"
    );
    assert!(
        !stderr.contains("/private/tmp/tmux") && !stderr.contains("/tmp/tmux"),
        "raw tmux socket path must not appear in normal output: {stderr}"
    );
}

/// An unavailable persisted Herdr binding stays exact: listings omit it and
/// targeted attach fails without silently rebinding it to the configured session.
#[test]
fn session_list_preserves_unavailable_herdr_binding() {
    use workhive_core::domain::{RuntimeBackendId, RuntimeBinding, WorkspaceIdentityRecord};
    use workhive_core::ports::WorkspaceIdentityPort;

    let (tmp, project_dir) = setup_initialized_project("binding-preservation");

    // Persist an exact Herdr socket binding. A later config change must not
    // replace this scope just because the socket is unavailable.
    let project_root =
        Utf8PathBuf::from_path_buf(project_dir.clone()).expect("project dir must be valid utf-8");
    let original_scope = tmp
        .path()
        .join("herdr/sessions/original/herdr.sock")
        .to_string_lossy()
        .into_owned();
    std::fs::write(
        project_root.join(".workhive/config.toml"),
        r#"backend = "herdr"

[herdr]
session = "new-session"
"#,
    )
    .expect("the test project can select a new Herdr session");
    workhive_infra::workspace_identity::FileIdentityAdapter::new(&project_root)
        .save_workspace_identity(&WorkspaceIdentityRecord {
            workspace: "main".to_owned(),
            relative_project_path: None,
            relative_display_path: None,
            runtime_binding: RuntimeBinding::new(RuntimeBackendId::herdr(), &original_scope, "wA")
                .expect("binding must be valid"),
            runtime_replacement: None,
            canonical_workspace_component: "main".to_owned(),
            canonical_session_name: "wA".to_owned(),
            canonical_server_name: original_scope.clone(),
        })
        .expect("original workspace identity must persist");

    let output = workhive_with_home(tmp.path())
        .args(["session", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("session list should execute");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "an unavailable Herdr scope must not abort the global listing. stderr: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let payload: Value = serde_json::from_str(&stdout).expect("session list must emit JSON");
    assert_eq!(
        payload["sessions"].as_array().map(Vec::len),
        Some(0),
        "the unavailable Herdr binding must not appear as an active session: {stdout}"
    );

    // Project-scoped attach is targeted, so it must fail for the unavailable
    // exact binding instead of silently switching to another Herdr session.
    let attach = workhive_with_home(tmp.path())
        .args(["workspace", "attach", "main"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace attach should execute");
    let attach_stderr = String::from_utf8_lossy(&attach.stderr);
    assert!(
        !attach.status.success(),
        "attaching to an unavailable binding must fail closed: {attach_stderr}"
    );
    let identities = workhive_infra::workspace_identity::FileIdentityAdapter::new(&project_root)
        .list_workspace_identities()
        .expect("workspace identities should remain readable");
    let stored = identities
        .iter()
        .find(|record| record.workspace == "main")
        .expect("the original workspace identity should remain stored");
    assert_eq!(stored.runtime_binding.scope_id, original_scope);
    assert_eq!(stored.runtime_binding.workspace_id, "wA");
}

// -- Init global with force (idempotent) --

#[test]
fn init_global_with_force_succeeds() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
}

#[test]
fn init_global_creates_config_and_env_files() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = workhive_with_home(tmp.path());
    add_fake_tools(&mut cmd, tmp.path(), &["claude", "codex"]);
    cmd.args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let env_path = tmp.path().join(".config/workhive/.env");
    assert!(config_path.exists(), "config.toml should be created");
    assert!(env_path.exists(), ".env should be created");

    let config_content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        config_content.contains("language"),
        "config.toml should contain language key"
    );
    assert!(
        config_content.contains("[logging.console]"),
        "config.toml should contain explicit console logging config"
    );
    assert!(
        config_content.contains("[logging.file]"),
        "config.toml should contain explicit file logging config"
    );
    assert!(
        config_content.contains("base_name = \"workhive.log\""),
        "file log base name must be explicit and end with .log"
    );
    assert!(
        config_content.contains("format = \"compact\""),
        "default file log format should be compact"
    );
    assert!(
        config_content.contains("rotation = \"size_mb(10)\""),
        "default file rotation should be bounded"
    );
    assert!(
        config_content.contains("retention = \"max_files(7)\""),
        "default file retention should be bounded"
    );
    assert!(
        config_content.contains("[tools.catalog.claude]"),
        "global config should contain editable name-keyed tool catalog sections"
    );
    assert!(
        config_content.contains("[tools.catalog.claude.command]")
            && config_content.contains("program = \"claude\""),
        "global config should write the default claude tool command"
    );
    assert!(
        config_content.contains("--dangerously-skip-permissions"),
        "global config claude command must include --dangerously-skip-permissions"
    );
    assert!(
        config_content.contains("IS_SANDBOX") && config_content.contains("\"1\""),
        "global config claude command must include IS_SANDBOX = \"1\" env"
    );
    assert!(
        config_content.contains("[tools.catalog.codex]")
            && config_content.contains("[tools.catalog.codex.command]")
            && config_content.contains("program = \"codex\""),
        "global config should write the default codex tool command"
    );

    let env_content = std::fs::read_to_string(&env_path).unwrap();
    assert!(
        env_content.contains("workhive"),
        ".env should contain default header"
    );
}

#[test]
fn init_global_omits_unavailable_default_tools() {
    let tmp = tempfile::TempDir::new().unwrap();
    let mut cmd = workhive_with_home(tmp.path());
    add_fake_tools_with_isolated_path(&mut cmd, tmp.path(), &["codex", "git", "tmux"]);
    cmd.args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let config_content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        !config_content.contains("[tools.catalog.claude]"),
        "unavailable default tools must not be written to the editable global catalog"
    );
    assert!(
        config_content.contains("[tools.catalog.codex]"),
        "available default tools should be written to the editable global catalog"
    );
}

#[test]
fn init_global_force_is_idempotent() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    let config_path = tmp.path().join(".config/workhive/config.toml");
    assert!(config_path.exists(), "config.toml should survive re-init");
}

// -- Invalid input --

#[test]
fn unknown_command_exits_with_code_2() {
    // Hermetic HOME — see comment on removed_top_level_commands_are_rejected_with_exit_code_2.
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .arg("nonexistent")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

/// Without a workspace name and without a TTY on stdin, `workspace open`
/// must fail deterministically with actionable guidance instead of
/// surfacing a raw terminal-mode error from the interactive selector.
#[test]
fn open_without_workspace_arg_fails_deterministically_without_tty() {
    let (tmp, project_dir) = setup_hermetic_project("open-interactive");
    workhive_with_home(tmp.path())
        .args(["workspace", "open"])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        // miette wraps long lines, so assert on fragments that survive
        // the wrap: the reason and the tail of the suggested invocation.
        .stderr(predicate::str::contains("requires a terminal"))
        .stderr(predicate::str::contains("--no-attach"));
}

// -- Verbose flag --

#[test]
fn verbose_flag_enables_debug_logging() {
    let (tmp, project_dir) = setup_hermetic_project("verbose-repo");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args(["-v", "workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("DEBUG"));
}

// -- Non-interactive confirmation --

#[test]
fn mutating_command_without_force_in_pipe_fails() {
    workhive()
        .args(["init", "--global", "--backend", "tmux"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));
}

#[test]
fn destroy_without_force_in_pipe_fails() {
    // Verifies the legacy `--force` requirement. The destroy preflight
    // short-circuits with a localized "no workhive state" message when
    // `.workhive/` is missing, so this test must initialize the project
    // before reaching the `--force` confirmation gate.
    let (tmp, project_dir) = setup_hermetic_project("destroy-needs-force");

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["workspace", "destroy"])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));
}

/// `workspace destroy` MUST short-circuit with a non-destructive,
/// localized message when `.workhive/` does not exist. It must NOT
/// trigger any project-init prompt and must NOT print the generic
/// destructive `--force` warning.
#[test]
fn destroy_short_circuits_when_no_workhive_state() {
    let (tmp, project_dir) = setup_hermetic_project("destroy-fresh");

    // Note: `setup_hermetic_project` only initializes the global config and
    // a bare git repo — it does NOT run `workhive init` in the project, so
    // `.workhive/` is absent here. That is exactly the state this test pins.

    workhive_with_home(tmp.path())
        .args(["workspace", "destroy"])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("No workhive state"))
        .stderr(predicate::str::contains("workhive init"))
        .stderr(predicate::str::contains("--force").not());

    assert!(
        !project_dir.join(".workhive").exists(),
        "destroy must not create .workhive/ when none exists"
    );
}

#[test]
fn worktree_remove_without_existing_target_returns_not_found() {
    let (tmp, project_dir, _proj_name) = setup_git_project("worktree-remove-no-force");
    workhive_with_home(tmp.path())
        .args(["workspace", "remove", "feat-a"])
        .current_dir(&project_dir)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("workspace root not found"));
}

#[test]
fn workspace_remove_is_recognized_by_parser() {
    let (tmp, project_dir, _proj_name) = setup_git_project("workspace-remove-parse");

    workhive_with_home(tmp.path())
        .args(["workspace", "remove", "feat-a"])
        .current_dir(&project_dir)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("workspace root not found"));
}

#[test]
fn workspace_list_is_recognized_by_parser() {
    let (tmp, project_dir, _proj_name) = setup_git_project("workspace-list-parse");

    workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Project root"));
}

#[test]
fn init_project_creates_workhive_structure() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("my-project");
    std::fs::create_dir_all(&project_dir).unwrap();

    // Initialize a git repo in the project dir
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    // First, init global
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    // Then, init project
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Verify .workhive/ structure
    assert!(
        project_dir.join(".workhive/config.toml").exists(),
        ".workhive/config.toml should exist"
    );
    assert!(
        project_dir.join(".workhive/.env").exists(),
        ".workhive/.env should exist"
    );

    // Verify project config inherits global language
    let project_config =
        std::fs::read_to_string(project_dir.join(".workhive/config.toml")).unwrap();
    assert!(
        project_config.contains("language"),
        "project config should inherit language from global"
    );
    assert!(
        !project_config.contains("[tools.catalog."),
        "project config should not copy the global tool catalog"
    );
    assert!(
        !project_config.contains("program = \"claude\""),
        "project config should reference tools by name instead of copying command definitions"
    );

    // Verify global index updated
    let index_path = tmp.path().join(".config/workhive/index.json");
    assert!(index_path.exists(), "global index.json should exist");
    let index_content = std::fs::read_to_string(&index_path).unwrap();
    assert!(
        index_content.contains("my-project"),
        "global index should contain the project name"
    );
}

#[test]
fn init_project_without_global_in_pipe_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("test-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    // init project without global, non-interactive → should fail with global init guidance
    workhive_with_home(tmp.path())
        .args(["init"])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("init --global"));
}

// -- Config commands --

#[test]
fn config_show_global_displays_toml() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Default mode: visual overview
    workhive_with_home(tmp.path())
        .args(["config", "show", "--global"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Language"));

    // Raw mode: TOML output
    workhive_with_home(tmp.path())
        .args(["config", "show", "--global", "--raw"])
        .assert()
        .success()
        .stdout(predicate::str::contains("language"));
}

#[test]
fn init_global_writes_explanatory_logging_comments() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(config_path).expect("global config should be readable");

    assert!(content.contains("# Rotation values: never, hourly, daily, size_mb(N)."));
    assert!(content.contains("# Retention values: unlimited, max_files(N)."));
    assert!(content.contains("# Format values: compact, json."));
    assert!(content.contains("# User interface language."));
    assert!(content.contains("# Runtime backend settings for tmux."));
    assert!(
        content.contains("# Pane layout used when the workspace is created or force-recreated.")
    );
    assert!(content.contains("ignored when rotation = \"never\"."));
    assert!(
        content.contains("# Global tool catalog. Project layouts reference these tools by name.")
    );
    assert!(content.contains("# Edit command.program and command.args here"));
    assert!(content.contains("# kind values: agent, tui, cli."));
    assert!(content.contains("# enabled = false hides the tool from init-time command selection."));
    assert!(content.contains("rotation = \"size_mb(10)\""));
    assert!(content.contains("retention = \"max_files(7)\""));

    assert!(
        content.contains("compact"),
        "generated config comments must list compact as a valid format"
    );
    assert!(
        content.contains("json"),
        "generated config comments must list json as a valid format"
    );
    assert!(
        !content.contains("pretty"),
        "generated config comments must not list unsupported format 'pretty'"
    );
}

#[test]
fn config_set_and_show_reflects_change() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "show", "--global"])
        .assert()
        .success()
        .stdout(predicate::str::contains("zh-CN"));
}

#[test]
fn config_unset_removes_key() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "unset", "language", "--global", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(config_path).unwrap();
    assert!(
        !content.contains("\"en\""),
        "language value should be removed after unset"
    );
}

#[test]
fn config_set_and_unset_preserve_logging_comments_and_string_syntax() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "logging.file.retention",
            "max_files(9)",
            "--global",
            "--force",
        ])
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args(["config", "unset", "language", "--global", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(config_path).expect("global config should be readable");
    assert!(content.contains("retention = \"max_files(9)\""));
    assert!(content.contains("# Retention values: unlimited, max_files(N)."));
}

#[test]
fn config_set_shows_diff_preview() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success()
        .stderr(predicate::str::contains("---").or(predicate::str::contains("+++")));
}

#[test]
fn config_explain_logging_retention_shows_examples() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "explain", "logging.file.retention"])
        .assert()
        .success()
        .stdout(predicate::str::contains("max_files(N)"))
        .stdout(predicate::str::contains("unlimited"));
}

#[test]
fn config_explain_logging_rotation_shows_examples() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "explain", "logging.file.rotation"])
        .assert()
        .success()
        .stdout(predicate::str::contains("size_mb(N)"))
        .stdout(predicate::str::contains("hourly"))
        .stdout(predicate::str::contains("daily"));
}

#[test]
fn config_explain_logging_format_shows_examples() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "explain", "logging.file.format"])
        .assert()
        .success()
        .stdout(predicate::str::contains("compact"))
        .stdout(predicate::str::contains("json"));
}

#[test]
fn config_explain_layout_template_shows_examples() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "explain", "layout.template"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ai_coding_dual"))
        .stdout(predicate::str::contains("grid"));
}

#[test]
fn config_env_set_writes_to_dotenv() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args([
            "config", "env-set", "MY_VAR", "hello", "--global", "--force",
        ])
        .assert()
        .success();

    let env_path = tmp.path().join(".config/workhive/.env");
    let content = std::fs::read_to_string(env_path).unwrap();
    assert!(
        content.contains("MY_VAR=hello"),
        ".env should contain MY_VAR=hello"
    );
}

#[test]
fn config_env_unset_removes_from_dotenv() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Set first
    workhive_with_home(tmp.path())
        .args(["config", "env-set", "DEL_ME", "val", "--global", "--force"])
        .assert()
        .success();

    // Then unset
    workhive_with_home(tmp.path())
        .args(["config", "env-unset", "DEL_ME", "--global", "--force"])
        .assert()
        .success();

    let env_path = tmp.path().join(".config/workhive/.env");
    let content = std::fs::read_to_string(env_path).unwrap();
    assert!(
        !content.contains("DEL_ME"),
        ".env should not contain DEL_ME after unset"
    );
}

// -- Open command --

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn open_in_pipe_with_workspace_arg_requires_explicit_init_to_create_session() {
    let (tmp, project_dir) = setup_hermetic_project("open-no-force");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-test", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .success();
    assert!(
        project_dir.join(".workhive").exists(),
        "open with workspace arg and --init should initialize the project"
    );
}

#[test]
fn open_invalid_workspace_id_fails_with_zero_side_effects() {
    let (tmp, project_dir) = setup_hermetic_project("open-bad-id");

    // Capture index state before the command
    let index_path = tmp.path().join(".config/workhive/index.json");
    let index_before = std::fs::read_to_string(&index_path).unwrap_or_default();

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "..bad-name"])
        .current_dir(&project_dir)
        .assert()
        .code(2);

    // Invalid parameter MUST NOT create .workhive/ directory
    assert!(
        !project_dir.join(".workhive").exists(),
        "open with invalid workspace must not create .workhive/"
    );

    // Invalid parameter MUST NOT write instance_id
    assert!(
        !project_dir.join(".workhive/instance_id").exists(),
        "open with invalid workspace must not write instance_id"
    );

    // Invalid parameter MUST NOT modify the project index
    // Normalize empty states: both "" (file doesn't exist) and "[]" (empty array)
    // mean "no projects registered" and are equivalent for this test
    let index_after = std::fs::read_to_string(&index_path).unwrap_or_default();
    let normalize = |s: String| -> String {
        let trimmed = s.trim().to_owned();
        if trimmed.is_empty() || trimmed == "[]" {
            String::new()
        } else {
            trimmed
        }
    };
    assert_eq!(
        normalize(index_before),
        normalize(index_after),
        "open with invalid workspace must not modify index.json"
    );
}

#[test]
fn open_help_shows_usage() {
    workhive()
        .args(["workspace", "open", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace"))
        .stdout(predicate::str::contains("--no-attach"));
}

#[test]
fn open_force_flag_is_recognized() {
    // --force / -f is now a supported flag for force-recreate semantics.
    // Without global init the command fails with an init error — not an
    // "unexpected argument" parse error.  That confirms the flag is accepted.
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["workspace", "open", "--force", "test-ws"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument").not());
}

#[test]
fn open_force_short_flag_is_recognized() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["workspace", "open", "-f", "test-ws"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument").not());
}

#[test]
fn open_rejects_old_force_layout_flag() {
    // Hermetic HOME — keeps locale at default English so the
    // expected English clap token is asserted. zh-CN parse errors are
    // covered by dedicated zh_cn_* tests.
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["workspace", "open", "--force-layout", "test-ws"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected argument"));
}

#[test]
fn open_in_non_git_directory_returns_typed_not_a_repo_error() {
    let tmp = setup_hermetic_home();

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "test-ws"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Git repository is required"))
        // "Not a git repository" is an expected probe outcome: at default
        // verbosity the user must see only the localized error, never a
        // raw internal WARN line with git stderr.
        .stderr(predicate::str::contains("WARN").not());

    assert!(
        !tmp.path().join(".workhive").exists(),
        "open outside git repo must not create project state"
    );
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn open_with_no_attach_succeeds_in_git_repo() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("openproj{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-test", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success()
        // Deterministic --no-attach progress: the workspace name must
        // appear, AND the dedicated --no-attach summary line must be
        // emitted so users get the same lifecycle visibility regardless
        // of attach mode, including deterministic --no-attach execution.
        .stderr(predicate::str::contains("feat-test"))
        .stderr(predicate::str::contains("--no-attach"));
}

/// First-run readiness: opening the project-root workspace in a freshly
/// initialized repository with no commits (unborn HEAD) must not surface
/// the raw `ambiguous argument 'HEAD'` git failure. Branch detection runs
/// through the unborn-safe adapter path and the open completes.
#[test]
#[ignore] // real-tmux + real-git: opens a session in an unborn repository
fn open_in_unborn_repo_does_not_surface_ambiguous_head() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("unbornproj{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    // Initialize the repository but intentionally create no commit, so HEAD
    // points at an unborn branch.
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "main", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("ambiguous argument").not());
}

/// First-run readiness: opening a brand-new workspace in a freshly
/// initialized repository with no commits (unborn HEAD) must fail with
/// actionable product guidance instead of silently creating an orphan
/// worktree.
#[test]
#[ignore] // real-git: validates command flow in an unborn repository
fn open_new_workspace_in_unborn_repo_requires_initial_commit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("unbornnewproj{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    // Initialize the repository but intentionally create no commit.
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "feat-new", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("feat-new"))
        .stderr(predicate::str::contains("base commit"))
        .stderr(predicate::str::contains("ambiguous argument").not())
        .stderr(predicate::str::contains("WARN").not());

    assert!(
        !project_dir.join(".workhive/worktrees/feat-new").exists(),
        "non-root workspace provisioning must not create an orphan worktree"
    );
}

/// First-run readiness: if an earlier run already left behind a managed
/// linked worktree in an unborn repository, opening that non-root workspace
/// still must stop before tmux/session creation. Reusing the path would keep
/// the user in an orphan workspace with no base commit.
#[test]
#[ignore] // real-git: validates command flow with an existing linked worktree
fn open_existing_managed_workspace_in_unborn_repo_requires_initial_commit() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("unbornreuseproj{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    let worktree_dir = project_dir.join(".workhive/worktrees/feat-new");
    std::fs::create_dir_all(worktree_dir.parent().expect("worktree parent")).unwrap();
    std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "--orphan",
            "feat-new",
            worktree_dir.to_str().expect("utf8 worktree path"),
        ])
        .current_dir(&project_dir)
        .output()
        .expect("git worktree add should run");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "feat-new", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains("base commit"))
        .stderr(predicate::str::contains("Creating tmux session").not())
        .stderr(predicate::str::contains("ambiguous argument").not())
        .stderr(predicate::str::contains("WARN").not());
}

/// Hard-dependency preflight must run before the open handler mutates any
/// project or workspace state. With Git and tmux hidden from `PATH`,
/// `workspace open` must fail with the missing-dependency guidance and must
/// not create `.workhive` project state. This runs quickly because it fails
/// at preflight, before any tmux/worktree work.
#[test]
fn open_blocks_on_missing_dependencies_before_project_mutation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("depblock");
    std::fs::create_dir_all(&project_dir).unwrap();

    let mut cmd = workhive_with_home(tmp.path());
    // Isolate PATH to an empty fake-bin dir so neither git nor tmux resolves.
    add_fake_tools_with_isolated_path(&mut cmd, tmp.path(), &[]);
    cmd.args(["workspace", "open", "feat-new", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .failure()
        // The missing-dependency line ("✗ Git — not found" / tmux) proves the
        // dependency preflight, not a later handler stage, stopped the command.
        .stderr(predicate::str::contains("not found"));

    assert!(
        !project_dir.join(".workhive").exists(),
        "dependency preflight must block before any project state is created"
    );
}

// ── Phase 5: Attach ──

#[test]
fn attach_help_shows_usage() {
    workhive()
        .args(["workspace", "attach", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workspace"));
}

#[test]
fn attach_invalid_workspace_id_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("attach-invalid-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["workspace", "attach", "..bad-name"])
        .current_dir(&project_dir)
        .assert()
        .code(2);
}

/// When `language = "zh-CN"`, `workhive workspace attach` MUST NOT
/// emit raw English diagnostic strings for typed runtime errors.
///
/// We simulate a tmux backend that fails with `no server running on
/// <socket>`, which the adapter classifies as
/// `RuntimeError::BackendUnavailable { reason: ServerNotRunning, .. }`.
/// Under zh-CN that error must be rendered through the i18n catalog,
/// so the literal phrases `runtime backend not available` and
/// `tmux server not running` must NOT appear in stderr.
#[test]
fn attach_zh_cn_runtime_error_does_not_leak_english_diagnostic() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("attach-zh-cn-runtime");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), tmp.path());
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();

    // Stand up a deterministic tmux stub that always reports the
    // backend as unreachable. The adapter classifies this stderr as
    // `ServerNotRunning` → `BackendUnavailable { ServerNotRunning }`.
    let bin_dir = tmp.path().join("attach-zh-fake-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let tmux_path = bin_dir.join("tmux");
    std::fs::write(
        &tmux_path,
        "#!/bin/sh\necho 'no server running on /tmp/wh-srv' 1>&2\nexit 1\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&tmux_path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&tmux_path, permissions).unwrap();

    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let paths = std::iter::once(bin_dir).chain(std::env::split_paths(&current_path));

    let output = workhive_with_home(tmp.path())
        .env("PATH", std::env::join_paths(paths).unwrap())
        .args(["workspace", "attach", "feature-x"])
        .current_dir(&project_dir)
        .output()
        .expect("attach should run");

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stderr_lower = stderr.to_lowercase();

    assert!(
        !stderr_lower.contains("runtime backend not available"),
        "zh-CN attach leaked English Display from RuntimeError: {stderr}"
    );
    assert!(
        !stderr_lower.contains("tmux server not running"),
        "zh-CN attach leaked raw tmux English diagnostic: {stderr}"
    );
    assert!(
        !stderr_lower.contains("tmux socket not found"),
        "zh-CN attach leaked English `tmux socket not found`: {stderr}"
    );
}

/// Shared assertions for the zh-CN "not inside a workhive-managed
/// session" experience: the error must be Chinese, must name the
/// recovery commands, and must never surface the raw English core
/// `Display` as the only information.
fn assert_zh_cn_not_in_session(stderr: &str, command: &str) {
    assert!(
        stderr.contains("工作区") && stderr.contains("窗格"),
        "{command} zh-CN error must localize the not-in-session message: {stderr}"
    );
    assert!(
        stderr.contains("workhive workspace open") && stderr.contains("workhive workspace attach"),
        "{command} zh-CN error must guide recovery via workspace open/attach: {stderr}"
    );
    assert!(
        !stderr
            .to_lowercase()
            .contains("not inside a workhive-managed tmux session"),
        "{command} zh-CN error must not leak the raw English Display as the only info: {stderr}"
    );
}

/// `workhive pane ls` outside a workhive-managed session under zh-CN must
/// fail with a localized, actionable error rather than the raw English
/// core `Display`.
#[test]
fn pane_ls_outside_session_zh_cn_is_localized_and_actionable() {
    let (tmp, project_dir) = setup_initialized_project("pane-zh-ls");
    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();

    let output = run_pane_outside_session(tmp.path(), &project_dir, tmp.path(), &["pane", "ls"]);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(
        !output.status.success(),
        "pane ls outside a workhive session must fail: {stderr}"
    );
    assert_zh_cn_not_in_session(&stderr, "pane ls");
}

/// `workhive pane close` shares the same in-session preflight as the rest
/// of the `pane` surface, so it too must localize the not-in-session
/// error under zh-CN.
#[test]
fn pane_close_outside_session_zh_cn_is_localized_and_actionable() {
    let (tmp, project_dir) = setup_initialized_project("pane-zh-close");
    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();

    let output = run_pane_outside_session(tmp.path(), &project_dir, tmp.path(), &["pane", "close"]);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(
        !output.status.success(),
        "pane close outside a workhive session must fail: {stderr}"
    );
    assert_zh_cn_not_in_session(&stderr, "pane close");
}

/// `workhive pane send <target> <message>` must localize the
/// not-in-session error under zh-CN. The message payload is irrelevant to
/// the preflight (which runs before any target/message handling), so a
/// neutral ASCII payload is used here to keep the fixture language-clean.
#[test]
fn pane_send_outside_session_zh_cn_is_localized_and_actionable() {
    let (tmp, project_dir) = setup_initialized_project("pane-zh-send");
    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();

    let output = run_pane_outside_session(
        tmp.path(),
        &project_dir,
        tmp.path(),
        &["pane", "send", "2", "news-brief"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(
        !output.status.success(),
        "pane send outside a workhive session must fail: {stderr}"
    );
    assert_zh_cn_not_in_session(&stderr, "pane send");
}

/// Under the default English locale, the not-in-session error must still
/// be actionable: it names the canonical recovery commands.
#[test]
fn pane_ls_outside_session_en_surfaces_recovery_commands() {
    let (tmp, project_dir) = setup_initialized_project("pane-en-ls");

    let output = run_pane_outside_session(tmp.path(), &project_dir, tmp.path(), &["pane", "ls"]);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    assert!(
        !output.status.success(),
        "pane ls outside a workhive session must fail: {stderr}"
    );
    assert!(
        stderr.contains("workhive workspace open") && stderr.contains("workhive workspace attach"),
        "EN pane ls error must surface the recovery commands: {stderr}"
    );
}

#[test]
#[ignore] // real-tmux: queries tmux for session existence
fn attach_nonexistent_session_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("attach-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["workspace", "attach", "nonexistent"])
        .current_dir(&project_dir)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("No active sessions found"));
}

#[test]
#[ignore] // real-tmux: creates + attaches session
fn attach_existing_session_does_not_report_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("attachok{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");

    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Create session via open --no-attach
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-attach", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Attach to the existing session — in a pipe tmux attach itself may fail
    // (no terminal), but it must NOT fail with "session not found" (exit code 3).
    let result = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "attach", "feat-attach"])
        .current_dir(&project_dir)
        .output()
        .expect("command should execute");
    assert_ne!(
        result.status.code().unwrap_or(0),
        3,
        "attach should find the session (exit code 3 means session not found)"
    );
}

// -- E2E lifecycle: init → open → list → attach → destroy --

/// Helper: set up a git repo inside tmpdir, return (tmpdir, project_dir, proj_name)
fn setup_git_project(suffix: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("e2e{suffix}{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    (tmp, project_dir, proj_name)
}

#[cfg(unix)]
#[test]
#[ignore = "real-herdr: creates a disposable session under a temporary HOME"]
fn real_herdr_force_rebuild_refusal_preserves_project_and_linked_workspaces() {
    use std::os::unix::fs::PermissionsExt;

    let path = std::env::var_os("PATH").unwrap_or_default();
    let Some(real_herdr) = std::env::split_paths(&path)
        .map(|entry| entry.join("herdr"))
        .find(|candidate| candidate.is_file())
    else {
        eprintln!("SKIP: herdr executable is not available");
        return;
    };
    let tmp = tempfile::TempDir::new().unwrap();
    let herdr_config_dir = tmp.path().join(".config/herdr");
    std::fs::create_dir_all(&herdr_config_dir).unwrap();
    std::fs::write(
        herdr_config_dir.join("config.toml"),
        "[experimental]\nallow_nested = true\n",
    )
    .unwrap();
    let real_home = tmp.path().as_os_str().to_owned();
    let wrapper_dir = tmp.path().join("real-herdr-bin");
    std::fs::create_dir_all(&wrapper_dir).unwrap();
    let wrapper = wrapper_dir.join("herdr");
    std::fs::write(
        &wrapper,
        "#!/bin/sh\nunset XDG_CONFIG_HOME\nHOME=\"$REAL_HERDR_HOME\" exec \"$REAL_HERDR_BIN\" \"$@\"\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).unwrap();
    let command_path = std::env::join_paths(
        std::iter::once(wrapper_dir.clone()).chain(std::env::split_paths(&path)),
    )
    .unwrap();
    let command = || {
        let mut command = workhive_with_home(tmp.path());
        command
            .env("PATH", &command_path)
            .env("REAL_HERDR_BIN", &real_herdr)
            .env("REAL_HERDR_HOME", &real_home)
            .env_remove("HERDR_SOCKET_PATH")
            .env_remove("HERDR_CONFIG_PATH");
        command
    };

    let herdr_session = format!("workhive-issue19-{}", std::process::id());
    let migration_session = format!("workhive-issue19-migrated-{}", std::process::id());
    let project_name = format!("real-herdr-parent-{}", std::process::id());
    let project_dir = tmp.path().join(&project_name);
    let terminal_command = || {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_workhive"));
        command
            .env("HOME", tmp.path())
            .env("XDG_CONFIG_HOME", tmp.path().join(".config"))
            .env("PATH", &command_path)
            .env("REAL_HERDR_BIN", &real_herdr)
            .env("REAL_HERDR_HOME", &real_home)
            .env_remove("HERDR_SOCKET_PATH")
            .env_remove("HERDR_CONFIG_PATH")
            .current_dir(&project_dir);
        command
    };
    struct Cleanup {
        project_dir: std::path::PathBuf,
        home: std::path::PathBuf,
        command_path: std::ffi::OsString,
        real_herdr: std::path::PathBuf,
        real_home: std::ffi::OsString,
        herdr_session: String,
        migration_session: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new(env!("CARGO_BIN_EXE_workhive"))
                .env("HOME", &self.home)
                .env("XDG_CONFIG_HOME", self.home.join(".config"))
                .env("PATH", &self.command_path)
                .env("REAL_HERDR_BIN", &self.real_herdr)
                .env("REAL_HERDR_HOME", &self.real_home)
                .env_remove("HERDR_SOCKET_PATH")
                .args(["workspace", "destroy", "--force"])
                .current_dir(&self.project_dir)
                .output();
            for session in [&self.herdr_session, &self.migration_session] {
                let _ = std::process::Command::new(&self.real_herdr)
                    .env("HOME", &self.real_home)
                    .env_remove("XDG_CONFIG_HOME")
                    .env_remove("HERDR_SOCKET_PATH")
                    .env_remove("HERDR_CONFIG_PATH")
                    .args(["session", "stop", session])
                    .output();
            }
        }
    }
    let _cleanup = Cleanup {
        project_dir: project_dir.clone(),
        home: tmp.path().to_path_buf(),
        command_path: command_path.clone(),
        real_herdr: real_herdr.clone(),
        real_home: real_home.clone(),
        herdr_session: herdr_session.clone(),
        migration_session: migration_session.clone(),
    };
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    command()
        .args(["init", "--global", "--backend", "herdr", "--force"])
        .assert()
        .success();
    command()
        .args([
            "config",
            "set",
            "herdr.session",
            herdr_session.as_str(),
            "--global",
            "--force",
        ])
        .assert()
        .success();
    command()
        .args([
            "project",
            "roots",
            "add",
            tmp.path().to_str().expect("UTF-8 temp path"),
            "--force",
        ])
        .assert()
        .success();
    command()
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    command()
        .args(["workspace", "open", "feature-parent-e2e", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();
    command()
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let listed = command()
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("list Herdr workspaces");
    assert!(listed.status.success());
    let payload: Value = serde_json::from_slice(&listed.stdout).expect("workspace list JSON");
    let names = payload["workspaces"]
        .as_array()
        .expect("workspaces array")
        .iter()
        .filter_map(|workspace| workspace["name"].as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"feature-parent-e2e"));
    assert!(names.contains(&"main"));

    let before = herdr_snapshot(command(), &herdr_session);
    assert!(
        before["workspaces"]
            .as_array()
            .is_some_and(|workspaces| workspaces
                .iter()
                .any(|workspace| { workspace["worktree"]["is_linked_worktree"] == true })),
        "fixture must contain a linked worktree workspace: {before}"
    );
    let main_identity = read_workspace_identity(&project_dir, "main");
    let rebuild = command()
        .args(["workspace", "open", "main", "--force", "--no-attach"])
        .current_dir(&project_dir)
        .output()
        .expect("force rebuild should run");
    assert!(
        !rebuild.status.success(),
        "Herdr must reject closing a parent with linked children"
    );
    assert_eq!(herdr_snapshot(command(), &herdr_session), before);
    assert_eq!(read_workspace_identity(&project_dir, "main"), main_identity);
    assert!(
        String::from_utf8_lossy(&rebuild.stderr).contains("interactive terminal"),
        "non-interactive refusal must explain how to enter safely: {}",
        String::from_utf8_lossy(&rebuild.stderr)
    );

    let mut enter_existing = terminal_command();
    enter_existing.args(["workspace", "open", "main", "--force"]);
    let (entered, entered_output) = run_interactive_cli(
        enter_existing,
        "Enter the existing workspace without rebuilding",
        b"\r",
        &[
            (std::time::Duration::from_secs(1), b"\r"),
            (std::time::Duration::from_millis(1500), b"\x1b"),
            (std::time::Duration::from_secs(2), b"\x02"),
            (std::time::Duration::from_millis(2200), b"q"),
        ],
    );
    assert!(
        entered.success(),
        "direct entry should succeed without rebuilding: {entered_output}"
    );
    assert_eq!(herdr_snapshot(command(), &herdr_session), before);
    assert_eq!(read_workspace_identity(&project_dir, "main"), main_identity);

    let mut cancel = terminal_command();
    cancel.args(["workspace", "open", "main", "--force", "--no-attach"]);
    let (cancelled, cancel_output) = run_interactive_cli(
        cancel,
        "Keep the existing workspace without attaching",
        b"\x1b[B\r",
        &[],
    );
    assert!(
        !cancelled.success(),
        "selecting cancel should abort the rebuild: {cancel_output}"
    );
    assert_eq!(herdr_snapshot(command(), &herdr_session), before);
    assert_eq!(read_workspace_identity(&project_dir, "main"), main_identity);

    command()
        .args(["workspace", "remove", "feature-parent-e2e", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    let after_child_removal = herdr_snapshot(command(), &herdr_session);
    command()
        .args([
            "config",
            "set",
            "herdr.session",
            migration_session.as_str(),
            "--force",
        ])
        .current_dir(&project_dir)
        .assert()
        .success();
    command()
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();
    assert_eq!(
        herdr_snapshot(command(), &herdr_session),
        after_child_removal
    );
    assert_eq!(read_workspace_identity(&project_dir, "main"), main_identity);

    command()
        .args(["workspace", "open", "main", "--force", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();
    let migrated_identity = read_workspace_identity(&project_dir, "main");
    assert_ne!(
        migrated_identity["runtime_binding"]["scope_id"],
        main_identity["runtime_binding"]["scope_id"]
    );
    assert_ne!(
        migrated_identity["runtime_binding"]["workspace_id"],
        main_identity["runtime_binding"]["workspace_id"]
    );
    assert!(
        herdr_snapshot(command(), &herdr_session)["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let migrated = herdr_snapshot(command(), &migration_session);
    assert!(
        migrated["workspaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workspace| {
                workspace["id"] == migrated_identity["runtime_binding"]["workspace_id"]
                    && workspace["owner"]["workspace"] == "main"
            })
    );

    command()
        .args(["workspace", "destroy", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
}

fn run_interactive_cli(
    mut command: std::process::Command,
    prompt_item: &str,
    input: &[u8],
    follow_up: &[(std::time::Duration, &[u8])],
) -> (std::process::ExitStatus, String) {
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut master_fd = -1;
    let mut slave_fd = -1;
    let size = libc::winsize {
        ws_row: 40,
        ws_col: 120,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let opened = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null(),
            &size,
        )
    };
    assert_eq!(
        opened,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );

    let slave = unsafe { std::fs::File::from_raw_fd(slave_fd) };
    command.stdin(Stdio::from(slave.try_clone().unwrap()));
    command.stdout(Stdio::from(slave.try_clone().unwrap()));
    command.stderr(Stdio::from(slave));
    let mut master = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let flags = unsafe { libc::fcntl(master.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0, "fcntl(F_GETFL) failed");
    assert_eq!(
        unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0,
        "fcntl(F_SETFL) failed"
    );

    let mut child = command.spawn().expect("start interactive Workhive command");
    let expected = prompt_item.as_bytes();
    let mut output = Vec::new();
    let mut sent_input = false;
    let mut next_follow_up = 0;
    let mut input_sent_at = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let mut buffer = [0_u8; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("read interactive Workhive output: {error}"),
            }
        }
        if !sent_input
            && output
                .windows(expected.len())
                .any(|window| window == expected)
        {
            master.write_all(input).expect("send interactive selection");
            sent_input = true;
            input_sent_at = Some(Instant::now());
        }
        if let Some(sent_at) = input_sent_at {
            while let Some((delay, bytes)) = follow_up.get(next_follow_up)
                && sent_at.elapsed() >= *delay
            {
                master
                    .write_all(bytes)
                    .expect("send follow-up interactive input");
                next_follow_up += 1;
            }
        }
        if let Some(status) = child.try_wait().expect("check interactive command") {
            loop {
                match master.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => output.extend_from_slice(&buffer[..read]),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("read final Workhive output: {error}"),
                }
            }
            return (status, String::from_utf8_lossy(&output).into_owned());
        }
        if Instant::now() >= deadline {
            let output = String::from_utf8_lossy(&output).into_owned();
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "interactive prompt timed out; input_sent={sent_input}; follow_ups_sent={next_follow_up}; output={output}"
            );
        }
        let mut poll = libc::pollfd {
            fd: master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut poll, 1, 100) };
    }
}

fn herdr_snapshot(mut command: Command, session: &str) -> Value {
    let output = command
        .args(["session", "list", "--view", "json"])
        .output()
        .expect("list Herdr session snapshot");
    assert!(
        output.status.success(),
        "session list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let payload: Value = serde_json::from_slice(&output.stdout).expect("session list JSON");
    let mut snapshot = payload["herdr_sessions"]
        .as_array()
        .expect("Herdr sessions array")
        .iter()
        .find(|candidate| candidate["name"] == session)
        .unwrap_or_else(|| panic!("missing disposable Herdr session {session}: {payload}"))
        .clone();
    for workspace in snapshot["workspaces"].as_array_mut().unwrap() {
        workspace.as_object_mut().unwrap().remove("focused");
        for tab in workspace["tabs"].as_array_mut().unwrap() {
            tab.as_object_mut().unwrap().remove("focused");
            for pane in tab["panes"].as_array_mut().unwrap() {
                pane.as_object_mut().unwrap().remove("focused");
            }
        }
    }
    snapshot
}

fn read_workspace_identity(project_dir: &std::path::Path, workspace: &str) -> Value {
    std::fs::read_dir(project_dir.join(".workhive/identity/workspaces"))
        .unwrap()
        .flatten()
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .map(|contents| serde_json::from_str::<Value>(&contents).unwrap())
        .find(|identity| identity["workspace"] == workspace)
        .unwrap_or_else(|| panic!("missing workspace identity for {workspace}"))
}

fn read_instance_id(project_dir: &std::path::Path) -> String {
    std::fs::read_to_string(project_dir.join(".workhive/instance_id"))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn host_repo_root() -> std::path::PathBuf {
    let crate_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_dir
        .join("../..")
        .canonicalize()
        .expect("workspace root should be canonicalizable")
}

fn git_branch_exists(repo_root: &std::path::Path, branch: &str) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("--verify")
        .arg(format!("refs/heads/{branch}"))
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn git_has_worktree_for_workspace(repo_root: &std::path::Path, workspace: &str) -> bool {
    let target_path = repo_root
        .join(".workhive")
        .join("worktrees")
        .join(workspace);
    let output = match std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("worktree")
        .arg("list")
        .arg("--porcelain")
        .output()
    {
        Ok(out) => out,
        Err(_) => return false,
    };
    if !output.status.success() {
        return false;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().any(|line| {
        line.strip_prefix("worktree ").is_some_and(|p| {
            std::path::Path::new(p)
                .canonicalize()
                .ok()
                .zip(target_path.canonicalize().ok())
                .is_some_and(|(left, right)| left == right)
        })
    })
}

struct HostRepoCleanupGuard {
    repo_root: std::path::PathBuf,
    workspace: String,
    had_branch_before: bool,
    had_worktree_before: bool,
}

impl HostRepoCleanupGuard {
    fn new(repo_root: std::path::PathBuf, workspace: String) -> Self {
        let had_branch_before = git_branch_exists(&repo_root, &workspace);
        let had_worktree_before = git_has_worktree_for_workspace(&repo_root, &workspace);
        Self {
            repo_root,
            workspace,
            had_branch_before,
            had_worktree_before,
        }
    }
}

impl Drop for HostRepoCleanupGuard {
    fn drop(&mut self) {
        // Only clean side effects that were not present before this test.
        if !self.had_worktree_before {
            let worktree_path = self
                .repo_root
                .join(".workhive")
                .join("worktrees")
                .join(&self.workspace);
            let _ = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.repo_root)
                .arg("worktree")
                .arg("remove")
                .arg("--force")
                .arg(&worktree_path)
                .output();
            let _ = std::fs::remove_dir_all(&worktree_path);
        }

        if !self.had_branch_before {
            let _ = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.repo_root)
                .arg("branch")
                .arg("-D")
                .arg(&self.workspace)
                .output();
        }
    }
}

#[test]
#[ignore] // real-tmux: full lifecycle (open → list → attach → destroy)
fn e2e_open_list_attach_destroy_lifecycle() {
    let (tmp, project_dir, proj_name) = setup_git_project("lifecycle");
    let instance_id = read_instance_id(&project_dir);
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Open workspace (no-attach for piped environment)
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-e2e", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("feat-e2e"));

    // List: the human table shows the workspace row with its running
    // status; the canonical @-format session name is part of the
    // machine-readable surface.
    let list_out = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .output()
        .expect("list");
    let stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        stdout.contains("feat-e2e"),
        "list should show the workspace row, got: {stdout}"
    );
    let list_json_out = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("list json");
    let payload: Value = serde_json::from_str(&String::from_utf8_lossy(&list_json_out.stdout))
        .expect("workspace list must emit valid JSON");
    let expected_session = format!("wh@{}@{}@feat-e2e", instance_id, proj_name);
    let has_canonical_session = payload["workspaces"]
        .as_array()
        .expect("workspaces array")
        .iter()
        .any(|row| row["session"] == expected_session.as_str());
    assert!(
        has_canonical_session,
        "list JSON should carry session in @ format: expected {expected_session}, got: {payload}"
    );

    // Attach: should not report "session not found"
    let attach_result = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "attach", "feat-e2e"])
        .current_dir(&project_dir)
        .output()
        .expect("attach");
    assert_ne!(
        attach_result.status.code().unwrap_or(0),
        3,
        "attach should find the session"
    );

    // Destroy with force
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "destroy", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Verify .workhive dir removed
    assert!(
        !project_dir.join(".workhive").exists(),
        ".workhive should be removed after destroy"
    );
}

/// Agent closed loop over machine-readable surfaces: dispatch a
/// workspace without a TTY, observe it through `session list --view
/// json` and `workspace list --view json`, then remove it and observe
/// it disappear. This is the scripted path an AI agent drives from a
/// primary pane; no step may prompt.
#[test]
#[ignore] // real-tmux: agent dispatch → JSON query → remove closed loop
fn e2e_agent_dispatch_query_remove_closed_loop() {
    let (tmp, project_dir, proj_name) = setup_git_project("agentloop");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Dispatch: provision worktree + session without attaching.
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-agent", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Query 1: the global session listing must expose the new session.
    let sessions_out = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["session", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("session list json");
    assert!(sessions_out.status.success(), "session list must succeed");
    let sessions: Value = serde_json::from_str(&String::from_utf8_lossy(&sessions_out.stdout))
        .expect("session list must emit valid JSON");
    let found_session = sessions["sessions"]
        .as_array()
        .expect("sessions array")
        .iter()
        .any(|row| row["workspace"] == "feat-agent" && row["project"] == proj_name.as_str());
    assert!(
        found_session,
        "dispatched workspace must appear in session list JSON: {sessions}"
    );

    // Query 2: the project workspace listing must expose it too.
    let ws_out = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace list json");
    assert!(ws_out.status.success(), "workspace list must succeed");
    let workspaces: Value = serde_json::from_str(&String::from_utf8_lossy(&ws_out.stdout))
        .expect("workspace list must emit valid JSON");
    let found_ws = workspaces["workspaces"]
        .as_array()
        .expect("workspaces array")
        .iter()
        .any(|row| row["name"] == "feat-agent");
    assert!(
        found_ws,
        "dispatched workspace must appear in workspace list JSON: {workspaces}"
    );

    // Teardown: non-interactive removal, then the workspace must be gone
    // from the machine-readable surface.
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "remove", "feat-agent", "-f"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let ws_after = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "list", "--view", "json"])
        .current_dir(&project_dir)
        .output()
        .expect("workspace list json after remove");
    assert!(
        ws_after.status.success(),
        "workspace list after remove must succeed: {}",
        String::from_utf8_lossy(&ws_after.stderr)
    );
    let workspaces_after: Value = serde_json::from_str(&String::from_utf8_lossy(&ws_after.stdout))
        .expect("workspace list must emit valid JSON after remove");
    let still_there = workspaces_after["workspaces"]
        .as_array()
        .expect("workspaces array")
        .iter()
        .any(|row| row["name"] == "feat-agent");
    assert!(
        !still_there,
        "removed workspace must disappear from workspace list JSON: {workspaces_after}"
    );
}

#[test]
#[ignore] // real-tmux: open + destroy lifecycle
fn e2e_destroy_force_cleans_up() {
    let (tmp, project_dir, proj_name) = setup_git_project("destroy");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Open a workspace first
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "feat-del", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Destroy
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "destroy", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // .workhive should be gone
    assert!(!project_dir.join(".workhive").exists());

    // List --all-projects should no longer show this project
    let list_out = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["project", "list"])
        .output()
        .expect("list");
    let stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(
        !stdout.contains(&proj_name),
        "project should be removed from global index after destroy"
    );
}

#[test]
#[ignore] // real-tmux + real-git: dirty linked worktree must not be removed in non-interactive non-force destroy
fn e2e_destroy_dirty_noninteractive_refuses_without_force() {
    let (tmp, project_dir, _proj_name) = setup_git_project("destroy-dirty-refusal");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);
    let workspace = "feat-dirty-destroy";

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let dirty_file = project_dir
        .join(".workhive")
        .join("worktrees")
        .join(workspace)
        .join("dirty.txt");
    std::fs::write(&dirty_file, "dirty").expect("dirty marker should be written");
    assert!(git_has_worktree_for_workspace(&project_dir, workspace));

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "destroy"])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));

    assert!(
        project_dir.join(".workhive").exists(),
        "destroy refusal must keep project state on disk"
    );
    assert!(
        git_has_worktree_for_workspace(&project_dir, workspace),
        "dirty linked worktree must remain when destroy is refused"
    );
}

#[test]
#[ignore] // real-tmux + real-git: --force destroy should proceed while surfacing risky worktree report
fn e2e_destroy_force_reports_risky_worktrees_and_completes() {
    let (tmp, project_dir, _proj_name) = setup_git_project("destroy-force-risk-report");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);
    let workspace = "feat-dirty-force";

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let dirty_file = project_dir
        .join(".workhive")
        .join("worktrees")
        .join(workspace)
        .join("dirty.txt");
    std::fs::write(&dirty_file, "dirty").expect("dirty marker should be written");

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "destroy", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("Destroy safety report:"))
        .stderr(predicate::str::contains(
            "workspaces about to be removed contain unsaved or unmerged work",
        ));

    assert!(
        !project_dir.join(".workhive").exists(),
        "force destroy should still complete project cleanup"
    );
}

#[test]
fn workspace_remove_rejects_main_project_root_selector() {
    let (tmp, project_dir, proj_name) = setup_git_project("workspace-main-guard");

    let assert = workhive_with_home(tmp.path())
        .args(["workspace", "remove", &proj_name, "--force"])
        .current_dir(&project_dir)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Cannot remove the main project workspace.",
        ));

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.to_lowercase().contains("worktree"),
        "user-facing remove failure must not expose 'worktree' wording, got: {stderr}"
    );
}

#[test]
fn workspace_remove_unknown_workspace_uses_workspace_wording() {
    // Failing path: `workspace remove` for a name that does not resolve to a
    // managed workspace must surface a user-facing message phrased around the
    // `workspace` concept, not raw `worktree` wording.
    let (tmp, project_dir, _proj_name) = setup_git_project("workspace-remove-unknown");

    let assert = workhive_with_home(tmp.path())
        .args(["workspace", "remove", "does-not-exist", "--force"])
        .current_dir(&project_dir)
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.to_lowercase().contains("workspace"),
        "user-facing not-found error must mention 'workspace', got: {stderr}"
    );
    // The CLI handler must not emit primary user-facing sentences like
    // "Workhive-managed worktree" or "remove ... worktree". Path components
    // such as `.workhive/worktrees/` may legitimately appear.
    assert!(
        !stderr.contains("Workhive-managed worktree"),
        "user-facing not-found must not expose old 'Workhive-managed worktree' wording, got: {stderr}"
    );
}

#[test]
#[ignore] // real-tmux + real-git: --keep-branch preserves a linked worktree branch
fn e2e_worktree_remove_keep_branch_flag_preserves_branch() {
    let (tmp, project_dir, _proj_name) = setup_git_project("worktree-remove-keep-branch");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);
    let workspace = "feat-keep-branch";

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(git_branch_exists(&project_dir, workspace));
    assert!(git_has_worktree_for_workspace(&project_dir, workspace));

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "remove", workspace, "--force", "--keep-branch"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(
        git_branch_exists(&project_dir, workspace),
        "--keep-branch should preserve the local branch"
    );
    assert!(
        !git_has_worktree_for_workspace(&project_dir, workspace),
        "linked worktree should be removed"
    );
}

#[test]
#[ignore] // real-tmux + real-git: --delete-branch removes branch after worktree cleanup
fn e2e_worktree_remove_delete_branch_removes_local_branch() {
    let (tmp, project_dir, _proj_name) = setup_git_project("worktree-remove-delete-branch");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);
    let workspace = "feat-delete-branch";

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(git_branch_exists(&project_dir, workspace));
    assert!(git_has_worktree_for_workspace(&project_dir, workspace));

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args([
            "workspace",
            "remove",
            workspace,
            "--delete-branch",
            "--force",
        ])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(
        !git_branch_exists(&project_dir, workspace),
        "--delete-branch should remove the local branch"
    );
    assert!(
        !git_has_worktree_for_workspace(&project_dir, workspace),
        "linked worktree should be removed before branch deletion"
    );
}

#[test]
#[ignore] // real-tmux + real-git: non-force removal in non-interactive mode refuses dirty risk
fn e2e_worktree_remove_dirty_noninteractive_refuses_without_force() {
    let (tmp, project_dir, _proj_name) = setup_git_project("worktree-remove-dirty-refusal");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);
    let workspace = "feat-dirty-refusal";

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let dirty_file = project_dir
        .join(".workhive")
        .join("worktrees")
        .join(workspace)
        .join("dirty.txt");
    std::fs::write(&dirty_file, "dirty").expect("dirty marker should be written");
    assert!(git_has_worktree_for_workspace(&project_dir, workspace));

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "remove", workspace])
        .current_dir(&project_dir)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));

    assert!(
        git_has_worktree_for_workspace(&project_dir, workspace),
        "non-force, non-interactive execution must refuse destructive removal"
    );
}

#[test]
#[ignore] // real-tmux: creates session, verifies @ naming format
fn e2e_open_shows_session_in_at_format() {
    let (tmp, project_dir, proj_name) = setup_git_project("atformat");
    let instance_id = read_instance_id(&project_dir);
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    let result = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "my-ws", "--no-attach"])
        .current_dir(&project_dir)
        .output()
        .expect("open");

    let stderr = String::from_utf8_lossy(&result.stderr);
    let expected = format!("wh@{}@{}@my-ws", instance_id, proj_name);
    assert!(
        stderr.contains(&expected),
        "open should display session in @ format: expected '{expected}' in stderr: {stderr}"
    );
}

#[test]
#[ignore] // real-tmux: creates session, verifies reuse on second open
fn e2e_open_twice_reuses_session() {
    let (tmp, project_dir, _proj_name) = setup_git_project("reuse");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "reuse-ws", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Second open reuses the session
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "reuse-ws", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("reuse-ws"));
}

#[test]
#[ignore] // real-tmux: creates session + tests attach failure path
fn open_attach_failure_does_not_output_reusing_message() {
    let (tmp, project_dir, _proj_name) = setup_git_project("attach-fail");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "attach-ws", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Second open WITHOUT --no-attach: in a pipe tmux attach will fail
    // because there's no TTY. The "Reusing existing session" message
    // must NOT appear because it was moved AFTER attach_workspace().
    let result = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "attach-ws"])
        .current_dir(&project_dir)
        .output()
        .expect("command should execute");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !result.status.success() || !stderr.contains("Reusing existing session"),
        "when attach fails, 'Reusing existing session' must not appear in stderr: {stderr}"
    );
}

#[test]
fn e2e_instance_id_persists_across_inits() {
    let (tmp, project_dir, _proj_name) = setup_git_project("idem");
    let id1 = read_instance_id(&project_dir);

    // Re-init project
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let id2 = read_instance_id(&project_dir);
    assert_eq!(id1, id2, "instance_id should be stable across re-inits");
}

#[test]
fn e2e_force_init_creates_minimal_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();

    // Force (non-interactive) mode creates minimal config with language
    assert!(
        content.contains("language"),
        "config should contain language after force init"
    );
    // Naming templates are populated by the interactive wizard, not force mode.
    // In force mode, runtime defaults from NamingConfig::default() apply.
}

// -- Dependency check integration tests --

#[test]
fn init_global_force_shows_dependency_check() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success()
        .stderr(predicate::str::contains("git").or(predicate::str::contains("Git")))
        .stderr(predicate::str::contains("tmux"));
}

#[test]
fn list_with_global_config_runs_without_dep_output() {
    let tmp = tempfile::TempDir::new().unwrap();

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Non-init commands use quiet dependency checks — no dep output
    // when all dependencies are satisfied.
    workhive_with_home(tmp.path())
        .args(["project", "list"])
        .assert()
        .success();
}

// -- Command side-effect boundary tests --
//
// These tests verify that read-only commands (attach, list, sessions)
// never create `.workhive/` or write any persistent state, and that
// mutating commands (open) only write after confirmation.

#[test]
fn attach_on_uninitialized_project_does_not_create_workhive_dir() {
    let (tmp, project_dir) = setup_hermetic_project("attach-no-init");

    workhive_with_home(tmp.path())
        .args(["workspace", "attach", "main"])
        .current_dir(&project_dir)
        .assert()
        .code(3);

    assert!(
        !project_dir.join(".workhive").exists(),
        "attach must not create .workhive/ on an uninitialized project"
    );
}

#[test]
fn attach_on_uninitialized_project_does_not_return_init_hint() {
    let (tmp, project_dir) = setup_hermetic_project("attach-no-hint");

    workhive_with_home(tmp.path())
        .args(["workspace", "attach"])
        .current_dir(&project_dir)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("workhive init").not())
        .stderr(predicate::str::contains("project not initialized").not());
}

#[test]
fn config_edit_on_uninitialized_project_returns_init_hint() {
    let (tmp, project_dir) = setup_hermetic_project("config-edit-hint");

    workhive_with_home(tmp.path())
        .args(["config", "edit"])
        .current_dir(&project_dir)
        .assert()
        .code(3)
        .stderr(predicate::str::contains("workhive init"));
}

/// `config edit` previously only printed the effective merged source. When
/// the project hadn't shipped its own config yet, the effective source
/// pointed at the global file, which made users believe the wizard was
/// about to mutate global config. The fix renders a separate
/// "Edit target (Project): .workhive/config.toml" line so the user can
/// always tell what file they are editing.
#[test]
fn config_edit_default_project_shows_edit_target_distinct_from_effective_source() {
    let (tmp, project_dir) = setup_hermetic_project("config-edit-target-default");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["config", "edit"])
        .current_dir(&project_dir)
        .output()
        .expect("config edit should produce output before failing at the confirm prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Effective source"),
        "config edit must still display the effective merged source; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Edit target"),
        "config edit must display a distinct edit-target line; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Project"),
        "default project edit must label the target as Project scope; stdout was: {stdout}"
    );
    assert!(
        stdout.contains(".workhive/config.toml"),
        "default project edit must point at .workhive/config.toml; stdout was: {stdout}"
    );
}

/// Project edit must warn the user that continuing will create a new
/// project override when the file is missing — without that line the user
/// cannot tell the difference between "I'm editing the global config
/// shown in the effective view" and "I'm about to create a brand new
/// project override file".
#[test]
fn config_edit_warns_when_project_config_absent_and_will_be_created() {
    let (tmp, project_dir) = setup_hermetic_project("config-edit-target-create");
    // Simulate runtime-only init: `.workhive/instance_id` exists but
    // `.workhive/config.toml` was never written (mirrors what
    // `workhive workspace open` does without `--init`). `config edit`
    // must reach our new rendering and surface the create warning.
    let workhive_dir = project_dir.join(".workhive");
    std::fs::create_dir_all(&workhive_dir).expect("workhive dir must be creatable");
    std::fs::write(workhive_dir.join("instance_id"), "test-instance-id\n")
        .expect("instance_id must be writable");

    let output = workhive_with_home(tmp.path())
        .args(["config", "edit"])
        .current_dir(&project_dir)
        .output()
        .expect("config edit should produce output before failing at the confirm prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Edit target"),
        "edit-target line must still appear when project config is missing; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("create a new project override"),
        "missing project config must surface the create-override warning; stdout was: {stdout}"
    );
    assert!(
        stdout.contains(".workhive/config.toml"),
        "create-override warning must name the file path; stdout was: {stdout}"
    );
}

/// `config edit --global` targets the global file regardless of which
/// scope the effective merged view is currently sourced from. The
/// create-project-override warning must NOT appear for global edits.
#[test]
fn config_edit_global_targets_global_file_and_omits_project_create_warning() {
    let (tmp, project_dir) = setup_hermetic_project("config-edit-target-global");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let output = workhive_with_home(tmp.path())
        .args(["config", "edit", "--global"])
        .current_dir(&project_dir)
        .output()
        .expect("config edit --global should produce output before failing at the confirm prompt");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Edit target"),
        "global edit must display an edit-target line; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Global"),
        "global edit must label the target as Global scope; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("config/workhive/config.toml"),
        "global edit must point at the global config path; stdout was: {stdout}"
    );
    assert!(
        !stdout.contains("create a new project override"),
        "global edit must not show the project-override create warning; stdout was: {stdout}"
    );
}

/// `config show` must keep its existing overview behavior — it still
/// renders the effective merged source line and must NOT start showing
/// an edit-target line (only `config edit` cares about the write target).
#[test]
fn config_show_does_not_render_edit_target_line() {
    let (tmp, project_dir) = setup_hermetic_project("config-show-no-edit-target");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "show"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("Edit target").not())
        .stdout(predicate::str::contains("create a new project override").not());
}

#[test]
#[ignore] // real-tmux: queries tmux for session existence
fn attach_on_initialized_project_does_not_fail_with_init_error() {
    let (tmp, project_dir) = setup_hermetic_project("attach-init-ok");

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let result = workhive_with_home(tmp.path())
        .args(["workspace", "attach", "main"])
        .current_dir(&project_dir)
        .assert();

    let stderr_bytes = result.get_output().stderr.clone();
    let stderr_str = String::from_utf8_lossy(&stderr_bytes);
    assert!(
        !stderr_str.contains("not initialized"),
        "attach on initialized project must not report init error"
    );
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn open_with_workspace_arg_initializes_with_explicit_init_without_tty() {
    let (tmp, project_dir) = setup_hermetic_project("open-no-write");

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "test-ws", "--no-attach", "--init"])
        .current_dir(&project_dir)
        .assert()
        .success();
    assert!(
        project_dir.join(".workhive").exists(),
        "open with workspace arg and --init should initialize in pipe mode"
    );
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn open_with_valid_name_and_init_triggers_initialization() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("openautoinit{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "--no-attach", "--init", "test-ws"])
        .current_dir(&project_dir)
        .assert()
        .success();
    assert!(
        project_dir.join(".workhive").exists(),
        "open with valid name and --init must initialize the project"
    );
    assert!(
        project_dir.join(".workhive/instance_id").exists(),
        "open with valid name must create instance_id"
    );

    let index_path = tmp.path().join(".config/workhive/index.json");
    let index_content = std::fs::read_to_string(&index_path).unwrap_or_default();
    assert!(
        index_content.contains(&proj_name),
        "open with valid name must register project in index"
    );
}

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach`
fn open_in_temp_repo_does_not_pollute_host_repo_branch_or_worktree() {
    let host_repo = host_repo_root();
    let workspace = format!("test-ws-isolation-{}", std::process::id());
    let _host_cleanup_guard = HostRepoCleanupGuard::new(host_repo.clone(), workspace.clone());

    assert!(
        !git_branch_exists(&host_repo, &workspace),
        "host repo must not already have branch '{workspace}'"
    );
    assert!(
        !git_has_worktree_for_workspace(&host_repo, &workspace),
        "host repo must not already have worktree for '{workspace}'"
    );

    let (tmp, project_dir, _proj_name) = setup_git_project("isolation");

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", &workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(
        !git_branch_exists(&host_repo, &workspace),
        "open in temp repo must not create host branch '{workspace}'"
    );
    assert!(
        !git_has_worktree_for_workspace(&host_repo, &workspace),
        "open in temp repo must not create host worktree '{workspace}'"
    );
}

#[test]
fn list_on_uninitialized_project_does_not_create_workhive_dir() {
    let (tmp, project_dir) = setup_hermetic_project("list-no-init");

    workhive_with_home(tmp.path())
        .args(["workspace", "list"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(
        !project_dir.join(".workhive").exists(),
        "list must not create .workhive/ on an uninitialized project"
    );
}

// ── open main regression: must not create a linked worktree ──────────────

#[test]
#[ignore] // real-tmux: creates session for main branch
fn open_main_uses_project_root_without_creating_linked_worktree() {
    let tmp = tempfile::TempDir::new().unwrap();
    let proj_name = format!("openmain{}", std::process::id());
    let project_dir = tmp.path().join(&proj_name);
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    register_trusted_root(tmp.path(), tmp.path());

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    let output = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // ── Primary assertion: git worktree state ──────────────────────────────
    // `git worktree list --porcelain` is the ground truth. After `open main`,
    // there must be exactly one worktree and its path must equal project_dir.
    let wt_output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&project_dir)
        .output()
        .expect("git worktree list should succeed");
    let wt_stdout = String::from_utf8_lossy(&wt_output.stdout);

    let worktree_paths: Vec<&str> = wt_stdout
        .lines()
        .filter(|l| l.starts_with("worktree "))
        .map(|l| l.strip_prefix("worktree ").unwrap())
        .collect();

    assert_eq!(
        worktree_paths.len(),
        1,
        "open main must not create any linked worktree; expected 1 worktree, found {}: {:?}",
        worktree_paths.len(),
        worktree_paths
    );
    assert_eq!(
        std::path::Path::new(worktree_paths[0])
            .canonicalize()
            .unwrap(),
        project_dir.canonicalize().unwrap(),
        "the only worktree must be the project root itself"
    );

    // ── Secondary assertion: directory side-effect ───────────────────────
    let worktrees_dir = project_dir.join(".workhive").join("worktrees");
    if worktrees_dir.exists() {
        let entries: Vec<_> = std::fs::read_dir(&worktrees_dir)
            .expect("should be able to read worktrees dir")
            .filter_map(|e| e.ok())
            .collect();
        assert!(
            entries.is_empty(),
            "open main must not create any linked worktree dir, found: {:?}",
            entries.iter().map(|e| e.file_name()).collect::<Vec<_>>()
        );
    }

    // ── Tertiary assertion: stderr sanity ────────────────────────────────
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        !stderr.contains("created (new branch)"),
        "open main must not create a new branch worktree, got: {stderr}"
    );
}

#[test]
#[ignore] // real-tmux: creates legacy + canonical sessions, verifies migration
fn open_force_migrates_legacy_session_to_canonical_naming() {
    let (tmp, project_dir, proj_name) = setup_git_project("force-legacy-migrate");
    let workspace = "feat-legacy-migrate";
    let instance_id = read_instance_id(&project_dir);
    let canonical_session = format!("wh@{}@{}@{}", instance_id, proj_name, workspace);
    let legacy_session = format!("wh--{}--{}", proj_name, workspace);
    let legacy_server = format!("whsrv-{proj_name}");
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Create the legacy session on its historical per-project socket.
    std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", guard.path())
        .args([
            "-L",
            &legacy_server,
            "new-session",
            "-d",
            "-s",
            &legacy_session,
            "-c",
            project_dir
                .to_str()
                .expect("project dir must be valid utf-8"),
        ])
        .output()
        .expect("legacy tmux session creation should succeed");

    assert!(
        tmux_has_session_on_server(&guard, &legacy_server, &legacy_session),
        "legacy session must exist before force open"
    );
    assert!(
        !tmux_has_session(&guard, &canonical_session),
        "canonical session must not exist before force open"
    );

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--force", "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    assert!(
        !tmux_has_session_on_server(&guard, &legacy_server, &legacy_session),
        "open -f must destroy legacy session instead of recreating it"
    );
    assert!(
        tmux_has_session(&guard, &canonical_session),
        "open -f must recreate the session using canonical naming"
    );
}

// ── Regression: sub-directory names requiring sanitize inside a repo ────

#[test]
#[ignore] // real-tmux: creates session from subdir whose name requires sanitize
fn open_in_sanitized_subdir_succeeds_without_pane_or_session_error() {
    let repo_name = format!("python{}", std::process::id());
    let subdir_name = "1.0 pingtai@with#laopingta";
    let (tmp, project_dir, subdir) = setup_hermetic_project_with_subdir(&repo_name, subdir_name);
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let (_server, session) = expected_runtime_names(&project_dir, &subdir, "main");
    let guard = TmuxTestGuard::new();

    let result = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&subdir)
        .output()
        .expect("open should execute");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "open in sanitized subdir must succeed, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("can't find pane") && !stderr.contains("duplicate session"),
        "open in sanitized subdir must not produce pane/session errors, stderr: {stderr}"
    );
    assert!(
        tmux_has_session(&guard, &session),
        "expected session '{session}' to exist"
    );
}

#[test]
#[ignore] // real-tmux: repeated open from subdir whose name requires sanitize
fn open_twice_in_sanitized_subdir_reuses_session() {
    let repo_name = format!("python{}", std::process::id());
    let subdir_name = "release rc1@platform#alpha";
    let (tmp, project_dir, subdir) = setup_hermetic_project_with_subdir(&repo_name, subdir_name);
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let (_server, session) = expected_runtime_names(&project_dir, &subdir, "main");
    let guard = TmuxTestGuard::new();

    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&subdir)
        .assert()
        .success();

    let second = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", "main", "--no-attach"])
        .current_dir(&subdir)
        .output()
        .expect("second open should execute");

    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        second.status.success(),
        "second open in sanitized subdir must succeed, stderr: {stderr}"
    );
    assert!(
        !stderr.contains("can't find pane") && !stderr.contains("duplicate session"),
        "repeated open in sanitized subdir must not produce pane/session errors, stderr: {stderr}"
    );
    assert!(
        tmux_has_session(&guard, &session),
        "expected reused session '{session}' to exist"
    );
    assert!(
        stderr.contains("Session: reused"),
        "second open should report reused session summary, stderr: {stderr}"
    );
}

/// Helper for zh-CN locale tests: builds a hermetic HOME with `language = "zh-CN"`
/// in the global config, so `workhive_with_home(tmp.path())` invocations resolve
/// the locale to ZhCn before any clap parsing happens.
fn setup_zh_cn_home() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME for zh-CN test");
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args(["config", "set", "language", "zh-CN", "--global", "--force"])
        .assert()
        .success();
    tmp
}

fn zh_cn_help_stdout(tmp: &tempfile::TempDir, args: &[&str]) -> String {
    let out = workhive_with_home(tmp.path())
        .args(args)
        .output()
        .expect("workhive should run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{args:?} must exit success; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout.into_owned()
}

fn assert_zh_cn_help_has_no_english_clap_headings(stdout: &str) {
    for forbidden in [
        "Usage:",
        "Options:",
        "Commands:",
        "Arguments:",
        "Possible values:",
        "Print help",
        "Print version",
        "Print this message",
        "[aliases:",
    ] {
        assert!(
            !stdout.contains(forbidden),
            "zh-CN help leaked English clap text `{forbidden}`:\n{stdout}"
        );
    }
}

fn assert_zh_cn_list_view_help_mentions_all_values(stdout: &str) {
    for value in ["auto", "table", "compact", "json"] {
        assert!(
            stdout.contains(value),
            "zh-CN list --view help missing value `{value}`:\n{stdout}"
        );
    }
}

fn setup_home_with_broken_file_logging() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let not_a_dir = tmp.path().join("not-a-directory");
    std::fs::write(&not_a_dir, "this is a file, not a log directory")
        .expect("write broken logging path sentinel");

    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "logging.file.enabled",
            "true",
            "--global",
            "--force",
        ])
        .assert()
        .success();
    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "logging.file.directory",
            not_a_dir.to_str().expect("temp path should be utf-8"),
            "--global",
            "--force",
        ])
        .assert()
        .success();
    tmp
}

#[test]
fn completions_bash_is_stdout_only_even_when_file_logging_is_broken() {
    let tmp = setup_home_with_broken_file_logging();
    let out = workhive_with_home(tmp.path())
        .args(["completion", "script", "bash"])
        .output()
        .expect("workhive should run");
    assert!(
        out.status.success(),
        "completions bash must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "completion script generation must not write stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("_workhive"),
        "bash completion script should be written to stdout"
    );
}

#[test]
fn completions_install_bash_writes_user_completion_file() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    let out = workhive_with_home(tmp.path())
        .args(["completion", "install", "bash"])
        .output()
        .expect("workhive should run");

    assert!(
        out.status.success(),
        "install must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stdout.is_empty(),
        "install should not print the script to stdout"
    );

    let path = tmp
        .path()
        .join(".config/workhive/completions/bash/workhive");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("completion file should exist at {}: {e}", path.display()));
    assert!(
        script.contains("_workhive"),
        "installed bash completion should contain generated function"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    // The CLI now abbreviates home-prefixed paths in human output via
    // `path_display::display_path_for_humans`. Accept
    // either the full absolute path or the `~/...`-abbreviated form so
    // the test passes regardless of whether the hermetic HOME tempdir
    // is rooted under the user's real home.
    let abs_str = path.to_string_lossy().to_string();
    let home_str = tmp.path().to_string_lossy().to_string();
    let abbreviated = abs_str.replacen(&home_str, "~", 1);
    assert!(
        stderr.contains(&abs_str) || stderr.contains(&abbreviated),
        "install output should name the installed path (abs={abs_str}, abbreviated={abbreviated}): {stderr}"
    );
    assert!(
        stderr.contains("Updated ~/.bashrc") && stderr.contains("source ~/.bashrc"),
        "bash install output should name the updated rc file and activation command: {stderr}"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn completion_install_bash_activates_completion_in_a_new_shell() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    let out = workhive_with_home(tmp.path())
        .args(["completion", "install", "bash"])
        .output()
        .expect("workhive should run");

    assert!(
        out.status.success(),
        "install must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let bashrc = tmp.path().join(".bashrc");
    let shell = std::process::Command::new("bash")
        .args(["--noprofile", "--rcfile"])
        .arg(&bashrc)
        .args(["-ic", "complete -p workhive"])
        .env("HOME", tmp.path())
        .output()
        .expect("bash should run");

    assert!(
        shell.status.success(),
        "a new bash shell should register workhive completion; stdout: {}; stderr: {}",
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&shell.stderr)
    );
    assert!(
        String::from_utf8_lossy(&shell.stdout).contains("_workhive_dynamic"),
        "bash should use Workhive's generated completion function: {}",
        String::from_utf8_lossy(&shell.stdout)
    );
}

#[test]
fn completion_install_bash_is_idempotent_and_preserves_existing_bashrc() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    let bashrc_path = tmp.path().join(".bashrc");
    std::fs::write(&bashrc_path, "# user config\n").expect("write bashrc fixture");

    workhive_with_home(tmp.path())
        .args(["completion", "install", "bash"])
        .assert()
        .success();
    let first = std::fs::read_to_string(&bashrc_path).expect("read first bashrc");

    workhive_with_home(tmp.path())
        .args(["completion", "install", "bash"])
        .assert()
        .success();
    let second = std::fs::read_to_string(&bashrc_path).expect("read second bashrc");

    assert_eq!(first, second, "reinstalling must not change .bashrc");
    assert!(second.starts_with("# user config\n"));
    assert_eq!(
        second.matches("# >>> workhive completion >>>").count(),
        1,
        "managed completion block should be written once: {second}"
    );
}

#[test]
fn completion_install_fish_prints_shell_specific_activation_hint() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    let out = workhive_with_home(tmp.path())
        .args(["completion", "install", "fish"])
        .output()
        .expect("workhive should run");

    assert!(
        out.status.success(),
        "install must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("exec fish"),
        "fish install output should provide a fish-specific activation hint: {stderr}"
    );
}

/// Generated completion scripts must not advertise a top-level `worktree`
/// command. Worktree management lives only under `workspace worktree`;
/// top-level `worktree` is not a valid subcommand, so tab completion must
/// never surface it as a top-level subcommand.
///
/// We sample each shell's top-level command enumeration:
/// - bash: the `_workhive_commands` `if [[ ... ]]; then COMPREPLY=...`
///   alternation, where each visible top-level command is mentioned
///   verbatim with a leading space and no parent prefix.
/// - zsh: the `_workhive_commands()` function listing visible
///   top-level commands as `'<name>:<desc>'` entries.
/// - fish: the top-level `__fish_use_subcommand` lines that bind a
///   visible top-level command. Hidden subcommands of nested groups
///   share the literal name `worktree` with the canonical
///   `workspace worktree`, so we match the *top-level* condition only.
#[test]
fn completion_script_does_not_advertise_top_level_worktree() {
    for shell in ["bash", "zsh", "fish"] {
        let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
        let out = workhive_with_home(tmp.path())
            .args(["completion", "script", shell])
            .output()
            .expect("workhive should run");
        assert!(
            out.status.success(),
            "completion script must succeed for {shell}; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        // The canonical workspace worktree subcommand must still appear
        // because it is the user-visible entry point.
        assert!(
            stdout.contains("worktree"),
            "completion script for {shell} should still mention the canonical `workspace worktree`; got first 200 chars: {}",
            stdout.chars().take(200).collect::<String>()
        );

        match shell {
            "zsh" => {
                // Extract the body of `_workhive_commands()` (top-level
                // command enumeration) and pin the absence of
                // `'worktree:`.
                let block = extract_block_after(&stdout, "_workhive_commands() {")
                    .expect("zsh script must define _workhive_commands");
                assert!(
                    !block.contains("'worktree:"),
                    "zsh top-level enumeration must not advertise hidden `worktree`: {block}"
                );
            }
            "fish" => {
                // fish encodes top-level commands as
                // `complete -c workhive -n "__fish_use_subcommand" -f -a "<name>" ...`.
                let needle = "-n \"__fish_use_subcommand\" -f -a \"worktree\"";
                assert!(
                    !stdout.contains(needle),
                    "fish completion must not advertise hidden top-level `worktree` (matched {needle:?})"
                );
            }
            "bash" => {
                // bash encodes top-level commands inside the
                // `_workhive_commands` clause as a space-separated list
                // assigned to `cmd=workhive__commands`.
                let block =
                    extract_block_after(&stdout, "_workhive__commands)").unwrap_or_default();
                if !block.is_empty() {
                    assert!(
                        !block.contains("\"worktree\""),
                        "bash top-level enumeration must not advertise hidden `worktree`: {block}"
                    );
                }
            }
            _ => {}
        }
    }
}

/// Extract a small window starting at `marker` so we can scope shell
/// completion assertions to the region that actually enumerates the
/// top-level commands. Returns `None` if `marker` is not present.
fn extract_block_after(haystack: &str, marker: &str) -> Option<String> {
    let start = haystack.find(marker)?;
    let rest = &haystack[start..];
    // Next occurrence of `}` closes the function body. 4 KB is plenty
    // for the top-level enumeration we care about.
    let end = rest.find('}').unwrap_or(rest.len()).min(4096);
    Some(rest[..end].to_owned())
}

#[test]
fn old_completions_command_is_rejected() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    workhive_with_home(tmp.path())
        .args(["completions", "bash"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("completions"));
}

#[test]
fn old_completion_generate_command_is_rejected() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    workhive_with_home(tmp.path())
        .args(["completion", "generate", "bash"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("generate"));
}

#[test]
fn completion_install_detects_current_shell() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    std::fs::write(tmp.path().join(".zshrc"), "# user config\n").expect("write zshrc fixture");
    let out = workhive_with_home(tmp.path())
        .env("SHELL", "/bin/zsh")
        .args(["completion", "install"])
        .output()
        .expect("workhive should run");

    assert!(
        out.status.success(),
        "install must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let path = tmp
        .path()
        .join(".config/workhive/completions/zsh/_workhive");
    let script = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("completion file should exist at {}: {e}", path.display()));
    assert!(
        script.contains("_workhive"),
        "installed zsh completion should contain generated function"
    );
    let zshrc = std::fs::read_to_string(tmp.path().join(".zshrc")).expect("read zshrc");
    assert!(
        zshrc.contains("# >>> workhive completion >>>")
            && zshrc.contains(".config/workhive/completions/zsh")
            && zshrc.contains("compinit")
            && zshrc.contains("# <<< workhive completion <<<"),
        "zsh install should update .zshrc with managed completion block: {zshrc}"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("source") && stderr.contains(".zshrc"),
        "zsh install output should tell the user how to activate completion: {stderr}"
    );
}

#[test]
fn completion_install_zsh_is_idempotent() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME");
    for _ in 0..2 {
        workhive_with_home(tmp.path())
            .env("SHELL", "/bin/zsh")
            .args(["completion", "install"])
            .assert()
            .success();
    }

    let zshrc = std::fs::read_to_string(tmp.path().join(".zshrc")).expect("read zshrc");
    assert_eq!(
        zshrc.matches("# >>> workhive completion >>>").count(),
        1,
        "managed completion block should be written once: {zshrc}"
    );
}

#[test]
fn internal_completion_candidates_are_quiet_outside_project_with_broken_logging() {
    let tmp = setup_home_with_broken_file_logging();
    let out = workhive_with_home(tmp.path())
        .args(["internal", "completion", "candidates", "workspace"])
        .current_dir(tmp.path())
        .output()
        .expect("workhive should run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "internal completion must exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "internal completion must stay quiet on stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().all(|line| !line.trim().is_empty()),
        "internal completion stdout should be empty or one candidate per line: {stdout:?}"
    );
}

#[test]
fn zh_cn_root_help_is_fully_localized() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["--help"]);
    // Localized headings replace clap's English defaults.
    assert!(
        stdout.contains("用法:"),
        "missing 'Usage:' translation: {stdout}"
    );
    assert!(
        stdout.contains("选项:"),
        "missing 'Options:' translation: {stdout}"
    );
    // Auto-generated --help/--version descriptions are localized.
    assert!(
        stdout.contains("显示帮助"),
        "missing localized --help text: {stdout}"
    );
    assert!(
        stdout.contains("显示版本号"),
        "missing localized --version text: {stdout}"
    );
    // Top-level command descriptions stay localized.
    assert!(
        stdout.contains("管理工作区"),
        "workspace command should have localized about: {stdout}"
    );
    // No clap English boilerplate leaked through.
    assert!(
        !stdout.contains("Print help"),
        "English flag description leaked: {stdout}"
    );
    assert!(
        !stdout.contains("Print version"),
        "English flag description leaked: {stdout}"
    );
    assert_zh_cn_help_has_no_english_clap_headings(&stdout);
}

#[test]
fn zh_cn_subcommand_help_is_fully_localized() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["workspace", "remove", "--help"]);
    // Per-arg help is localized.
    assert!(
        stdout.contains("跳过交互式确认"),
        "--force help should be localized: {stdout}"
    );
    assert!(
        stdout.contains("授权删除用户自有的支撑分支"),
        "--delete-branch help should be localized: {stdout}"
    );
    // The auto --help description is localized at every command depth.
    assert!(
        stdout.contains("显示帮助"),
        "auto --help description should be localized at subcommand depth: {stdout}"
    );
    // The about line (not long_about) is localized — main description should be Chinese.
    assert!(
        stdout.contains("移除某个工作区"),
        "subcommand about should be localized: {stdout}"
    );
    // No English clap descriptions for built-in flags should leak.
    assert!(
        !stdout.contains("Print help"),
        "English flag description leaked: {stdout}"
    );
    assert_zh_cn_help_has_no_english_clap_headings(&stdout);
}

#[test]
fn zh_cn_nested_workspace_help_has_chinese_headings() {
    let tmp = setup_zh_cn_home();
    for args in [
        ["workspace", "--help"].as_slice(),
        ["workspace", "remove", "--help"].as_slice(),
        ["workspace", "ls", "--help"].as_slice(),
        ["project", "ls", "--help"].as_slice(),
    ] {
        let stdout = zh_cn_help_stdout(&tmp, args);
        assert!(
            stdout.contains("用法:"),
            "{args:?} should render a localized usage heading:\n{stdout}"
        );
        assert_zh_cn_help_has_no_english_clap_headings(&stdout);
    }
}

/// Walks every visible command path (top-level + project/workspace/session/pane/worktree
/// nested actions, including project roots/* and the new worktree group)
/// and asserts the zh-CN help output contains no English clap chrome.
///
/// New visible commands added later must be covered here so the regression
/// surface stays explicit.
#[test]
fn zh_cn_full_help_walk_has_no_english_clap_chrome() {
    let tmp = setup_zh_cn_home();
    let paths: &[&[&str]] = &[
        &["--help"],
        &["init", "--help"],
        &["doctor", "--help"],
        &["completion", "--help"],
        &["completion", "install", "--help"],
        &["completion", "script", "--help"],
        &["config", "--help"],
        &["config", "show", "--help"],
        &["config", "set", "--help"],
        &["config", "unset", "--help"],
        &["config", "edit", "--help"],
        &["config", "explain", "--help"],
        &["config", "env-set", "--help"],
        &["config", "env-unset", "--help"],
        &["project", "--help"],
        &["project", "list", "--help"],
        &["project", "reindex", "--help"],
        &["project", "roots", "--help"],
        &["project", "roots", "list", "--help"],
        &["project", "roots", "add", "--help"],
        &["project", "roots", "remove", "--help"],
        &["project", "roots", "edit", "--help"],
        &["project", "roots", "status", "--help"],
        &["workspace", "--help"],
        &["workspace", "open", "--help"],
        &["workspace", "list", "--help"],
        &["workspace", "attach", "--help"],
        &["workspace", "close", "--help"],
        &["workspace", "remove", "--help"],
        &["workspace", "destroy", "--help"],
        &["workspace", "worktree", "--help"],
        &["workspace", "worktree", "list", "--help"],
        &["workspace", "worktree", "remove", "--help"],
        &["session", "--help"],
        &["session", "list", "--help"],
        &["session", "attach", "--help"],
        &["pane", "--help"],
        &["pane", "split", "--help"],
        &["pane", "close", "--help"],
        &["pane", "list", "--help"],
        &["pane", "show", "--help"],
        &["pane", "send", "--help"],
    ];
    for args in paths {
        let stdout = zh_cn_help_stdout(&tmp, args);
        assert_zh_cn_help_has_no_english_clap_headings(&stdout);
    }
}

#[test]
fn zh_cn_list_view_help_mentions_all_supported_values() {
    let tmp = setup_zh_cn_home();
    for args in [
        ["workspace", "list", "--help"].as_slice(),
        ["project", "list", "--help"].as_slice(),
    ] {
        let stdout = zh_cn_help_stdout(&tmp, args);
        assert_zh_cn_help_has_no_english_clap_headings(&stdout);
        assert_zh_cn_list_view_help_mentions_all_values(&stdout);
    }
}

/// Leaf commands with neither subcommands nor positional arguments
/// must not render bare `命令:` and `参数:` headings in their zh-CN
/// help block.
///
/// The localization layer selects the nested help template from the
/// clap node shape, not by special-casing a command name.
#[test]
fn zh_cn_leaf_pane_list_help_does_not_render_empty_sections() {
    let tmp = setup_zh_cn_home();
    for args in [
        ["pane", "list", "--help"].as_slice(),
        ["pane", "ls", "--help"].as_slice(),
    ] {
        let stdout = zh_cn_help_stdout(&tmp, args);
        assert_zh_cn_help_has_no_english_clap_headings(&stdout);
        assert!(
            !stdout.contains("命令:\n选项"),
            "{args:?} must not render an empty 命令: heading:\n{stdout}",
        );
        assert!(
            !stdout.contains("命令:\n参数"),
            "{args:?} must not render an empty 命令: heading:\n{stdout}",
        );
        assert!(
            !stdout.contains("参数:\n选项"),
            "{args:?} must not render an empty 参数: heading:\n{stdout}",
        );
        assert!(
            stdout.contains("用法:"),
            "{args:?} must still render localized 用法: heading:\n{stdout}",
        );
        assert!(
            stdout.contains("选项:"),
            "{args:?} must still render localized 选项: heading:\n{stdout}",
        );
    }
}

/// Leaf commands with positional arguments but no subcommands must
/// keep the populated `参数:` section and drop the otherwise-empty
/// `命令:` section.
#[test]
fn zh_cn_leaf_pane_send_help_keeps_arguments_drops_commands() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["pane", "send", "--help"]);
    assert_zh_cn_help_has_no_english_clap_headings(&stdout);
    assert!(
        stdout.contains("参数:"),
        "pane send --help must keep the 参数: heading:\n{stdout}",
    );
    assert!(
        !stdout.contains("命令:\n参数"),
        "pane send --help must not render empty 命令: before 参数::\n{stdout}",
    );
    assert!(
        !stdout.contains("命令:\n选项"),
        "pane send --help must not render empty 命令: before 选项::\n{stdout}",
    );
}

/// Category commands with subcommands but no positionals must keep
/// the `命令:` section and drop the otherwise-empty `参数:` section.
#[test]
fn zh_cn_category_pane_help_keeps_commands_drops_arguments() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["pane", "--help"]);
    assert_zh_cn_help_has_no_english_clap_headings(&stdout);
    assert!(
        stdout.contains("命令:"),
        "pane --help must keep the 命令: heading:\n{stdout}",
    );
    assert!(
        !stdout.contains("参数:\n选项"),
        "pane --help must not render empty 参数: before 选项::\n{stdout}",
    );
}

/// `workhive pane --help` must NOT advertise the provider-specific
/// `approve-hooks` subcommand. Both English and Chinese listings
/// are checked because the hide flag affects help rendering at
/// clap level, independent of locale.
#[test]
fn pane_help_does_not_advertise_approve_hooks_in_either_locale() {
    let tmp = setup_zh_cn_home();
    let zh_stdout = zh_cn_help_stdout(&tmp, &["pane", "--help"]);
    assert!(
        !zh_stdout.contains("approve-hooks"),
        "zh `workhive pane --help` must not list approve-hooks:\n{zh_stdout}",
    );

    let en_out = workhive()
        .args(["pane", "--help"])
        .output()
        .expect("workhive should run");
    assert!(
        en_out.status.success(),
        "en pane --help must succeed; stderr: {}",
        String::from_utf8_lossy(&en_out.stderr)
    );
    let en_stdout = String::from_utf8_lossy(&en_out.stdout);
    assert!(
        !en_stdout.contains("approve-hooks"),
        "en `workhive pane --help` must not list approve-hooks:\n{en_stdout}",
    );
}

/// The hidden `approve-hooks` alias must still be invokable by legacy
/// scripts, but its direct `--help` must NOT leak any English clap
/// chrome or English derive defaults when the user is on zh-CN, and
/// must frame the command as a deprecated no-op without guiding the
/// user back into any provider hook approval flow.
#[test]
fn zh_cn_pane_approve_hooks_direct_help_is_localized_and_marks_deprecated() {
    let tmp = setup_zh_cn_home();
    let stdout = zh_cn_help_stdout(&tmp, &["pane", "approve-hooks", "--help"]);

    assert_zh_cn_help_has_no_english_clap_headings(&stdout);
    assert!(
        stdout.contains("已废弃"),
        "zh-CN direct help must frame `approve-hooks` as deprecated:\n{stdout}",
    );
    assert!(
        stdout.contains("目标窗格"),
        "zh-CN direct help must localize the <PANE> positional argument help:\n{stdout}",
    );
    assert!(
        !stdout.contains("/hooks"),
        "zh-CN direct help must NOT guide users into a `/hooks` approval flow:\n{stdout}",
    );
    // The deprecation copy may neutrally explain "no approval is
    // required", which legitimately contains "批准". What the help
    // text MUST NOT do is push the user into an active provider hook
    // approval flow (e.g. "请批准 ...", "approve the entries"). We
    // check for those active-imperative shapes instead of banning
    // the substring outright.
    for forbidden in ["请批准", "需要批准", "去批准", "批准条目", "批准 Workhive"] {
        assert!(
            !stdout.contains(forbidden),
            "zh-CN direct help must NOT push users into an approval flow ({forbidden:?}):\n{stdout}",
        );
    }
    assert!(
        !stdout.contains("Codex hook approval"),
        "zh-CN direct help must NOT leak the English derive default for the about line:\n{stdout}",
    );
    assert!(
        !stdout.contains("Target pane"),
        "zh-CN direct help must NOT leak the English derive default for the <PANE> arg:\n{stdout}",
    );
}

/// EN counterpart of the zh-CN direct-help test above. The English
/// help must frame the command as a deprecated no-op, must not push
/// users toward a provider hook approval flow, and must not leak
/// temporary implementation or project-management vocabulary.
///
/// Uses a hermetic HOME with no `language` override so the resolved
/// locale defaults to English independent of the developer's own
/// environment (where the global config may pin zh-CN).
#[test]
fn en_pane_approve_hooks_direct_help_marks_deprecated_and_points_at_hooks_review() {
    let tmp = tempfile::TempDir::new().expect("create hermetic HOME for en test");
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    let out = workhive_with_home(tmp.path())
        .args(["pane", "approve-hooks", "--help"])
        .output()
        .expect("workhive should run");
    assert!(
        out.status.success(),
        "en `pane approve-hooks --help` must succeed; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lower = stdout.to_lowercase();

    // Required user-visible semantics: deprecated no-op framing
    // without any provider hook approval guidance.
    assert!(
        lower.contains("deprecated"),
        "en direct help must frame `approve-hooks` as deprecated:\n{stdout}",
    );
    assert!(
        !stdout.contains("/hooks"),
        "en direct help must NOT guide users into a `/hooks` approval flow:\n{stdout}",
    );
    // The deprecation copy may neutrally explain that "no approval is
    // required", which legitimately contains "approval". What the
    // help text MUST NOT do is push the user into an active provider
    // hook approval flow (e.g. "approve the entries labeled ...",
    // "review and approve ..."). We check for those active-imperative
    // shapes instead of banning the substring outright.
    for forbidden in [
        "approve the entr",
        "approve the workhive",
        "approve the hook",
        "review and approve",
        "approve manually",
        "approve in codex",
    ] {
        assert!(
            !lower.contains(forbidden),
            "en direct help must NOT push users into an approval flow ({forbidden:?}):\n{stdout}",
        );
    }

    // Forbidden temporary implementation vocabulary. This text must
    // remain a product-facing help message, not a project-management
    // breadcrumb trail.
    for forbidden in [
        "active doc",
        ".md",
        ".dev-governor",
        "remediation",
        "2026-05-",
        "handoff",
        "pane-list-lacks-status",
        "empty-leaf-command-help",
    ] {
        // Case-insensitive containment check: governance tokens
        // are case-insensitive identifiers in our internal docs,
        // and user-visible CLI text shouldn't case-flip to dodge
        // this gate.
        assert!(
            !lower.contains(&forbidden.to_lowercase()),
            "en direct help must NOT leak internal governance vocabulary {forbidden:?}:\n{stdout}",
        );
    }
}

#[test]
fn zh_cn_unknown_subcommand_emits_localized_error_block() {
    let tmp = setup_zh_cn_home();
    let out = workhive_with_home(tmp.path())
        .args(["nonexistent-cmd"])
        .output()
        .expect("workhive should run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code must be 2 for parse error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("错误"),
        "missing localized error label: {stderr}"
    );
    assert!(
        stderr.contains("未知的子命令"),
        "missing localized 'unknown subcommand' phrase: {stderr}"
    );
    assert!(
        stderr.contains("用法："),
        "missing localized usage line: {stderr}"
    );
    // No English clap error block should leak through.
    assert!(
        !stderr.to_lowercase().contains("error:"),
        "English 'error:' label leaked: {stderr}"
    );
    assert!(
        !stderr.to_lowercase().contains("unrecognized subcommand"),
        "English clap message leaked: {stderr}"
    );
    assert!(
        !stderr.contains("Usage:"),
        "English Usage: heading leaked: {stderr}"
    );
}

#[test]
fn zh_cn_missing_required_argument_emits_localized_error_block() {
    let tmp = setup_zh_cn_home();
    let out = workhive_with_home(tmp.path())
        .args(["workspace", "remove"])
        .output()
        .expect("workhive should run");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("缺少必填参数"),
        "missing localized 'missing required arg' phrase: {stderr}"
    );
    assert!(
        !stderr.to_lowercase().contains("required"),
        "English clap 'required' message leaked: {stderr}"
    );
    assert!(
        !stderr.contains("Usage:"),
        "English Usage: heading leaked: {stderr}"
    );
}

#[test]
fn zh_cn_unknown_option_emits_localized_error_block() {
    let tmp = setup_zh_cn_home();
    let out = workhive_with_home(tmp.path())
        .args(["workspace", "remove", "foo", "--no-such-flag"])
        .output()
        .expect("workhive should run");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("未知的选项") || stderr.contains("非预期的参数"),
        "missing localized 'unknown option' phrase: {stderr}"
    );
    assert!(
        !stderr.to_lowercase().contains("unexpected argument"),
        "English clap message leaked: {stderr}"
    );
    assert!(
        !stderr.contains("Usage:"),
        "English Usage: heading leaked: {stderr}"
    );
}

// ── Trusted-roots admission policy ──────────────────────────────────────────
//
// These tests exercise the global trusted-roots gate that protects all
// mutating project operations (init, open --init, worktree provisioning).

/// `init --force` (project mode) outside any configured trusted root must be
/// blocked with a localized error and must NOT create `.workhive/` on disk.
/// This is the safety boundary that makes "no surprises" real: the user runs
/// the command, sees the error, and the workspace state is unchanged.
#[test]
fn project_init_outside_trusted_roots_is_blocked_without_writing_workhive() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("untrusted-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Deliberately DO NOT register the temp dir as a trusted root.
    let assertion = workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr).to_string();
    assert!(
        stderr.to_lowercase().contains("trusted") || stderr.contains("可信"),
        "trusted-roots admission failure must surface a localized policy message; got: {stderr}",
    );
    assert!(
        !project_dir.join(".workhive").exists(),
        ".workhive/ must NOT be created when trusted-roots admission rejects the project",
    );
}

/// `workspace open --init` outside trusted roots must be blocked BEFORE
/// `.workhive/` is created or any git/tmux side effect runs. The init
/// admission gate sits in front of `usecases::init::init_project_runtime_only`.
#[test]
fn workspace_open_init_outside_trusted_roots_is_blocked_before_workhive() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("untrusted-open-proj");
    std::fs::create_dir_all(&project_dir).unwrap();

    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init should succeed");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit should succeed");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Deliberately DO NOT register a trusted root.
    workhive_with_home(tmp.path())
        .args(["workspace", "open", "--no-attach", "--init", "test-ws"])
        .current_dir(&project_dir)
        .assert()
        .failure();

    assert!(
        !project_dir.join(".workhive").exists(),
        ".workhive/ must NOT be created when `workspace open --init` is gated by trusted-roots policy",
    );
}

/// Trusted roots configured but the project itself sits in a sibling-prefix
/// directory (e.g. trusted=`~/dev`, project=`~/dev2/foo`). The string-prefix
/// trap must be avoided — the admission policy requires path-component
/// matching, not raw substring containment.
#[test]
fn project_init_in_sibling_prefix_dir_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dev = tmp.path().join("dev");
    let dev2 = tmp.path().join("dev2");
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::create_dir_all(&dev2).unwrap();

    // `dev2` lives next to `dev`; only `dev` should be trusted.
    let project_dir = dev2.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), &dev);

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .failure();

    assert!(
        !project_dir.join(".workhive").exists(),
        "sibling-prefix path `dev2` must not be admitted as descendant of `dev`",
    );
}

/// Smoke test for the `project roots` command surface: list / add / remove /
/// status. Validates that the canonical CLI flow round-trips through the
/// global config without truncation, confirms each command is wired up, and
/// that `add --force` works for tempdir paths.
#[test]
fn project_roots_subcommands_round_trip_a_path_through_global_config() {
    // Use the no-auto-trust home so `roots add <parent>` is not rejected
    // by the parent-coverage redundancy guard (the auto-trusted
    // `tmp.path()` would otherwise dominate every child path).
    let tmp = setup_hermetic_home_without_auto_trust();
    let parent = tmp.path().join("dev");
    std::fs::create_dir_all(&parent).unwrap();

    // List on a fresh hermetic home — the heading is emitted even when
    // no roots are configured yet.
    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let listed_stderr = String::from_utf8_lossy(&listed.get_output().stderr).to_string();
    assert!(
        !listed_stderr.is_empty(),
        "`project roots list` must surface at least the heading",
    );

    // Add the dev directory.
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // Status must mention the configured count and project index state.
    let status = workhive_with_home(tmp.path())
        .args(["project", "roots", "status"])
        .assert()
        .success();
    let status_stderr = String::from_utf8_lossy(&status.get_output().stderr).to_string();
    assert!(
        !status_stderr.is_empty(),
        "`project roots status` must produce output",
    );

    // Remove the dev directory.
    workhive_with_home(tmp.path())
        .args(["project", "roots", "remove", parent.to_str().unwrap()])
        .assert()
        .success();
}

/// User-configured
/// trusted-root path values must round-trip *verbatim* through `project roots
/// list`. An absolute path inside `$HOME` must not be rewritten to `~/...`.
///
/// Configured roots must never be funneled through
/// `display_path_for_humans`, which would abbreviate `/Users/<name>/dev` to
/// `~/dev` even though the user explicitly stored the absolute form.
#[test]
fn project_roots_list_displays_absolute_configured_value_verbatim() {
    // No-auto-trust home: avoids the redundancy guard rejecting the
    // explicit `roots add tmp/dev` call below.
    let tmp = setup_hermetic_home_without_auto_trust();
    let dev = tmp.path().join("dev");
    std::fs::create_dir_all(&dev).unwrap();

    // Use the canonicalized form so this test is stable across macOS
    // `/private/var` symlink quirks.
    let absolute = std::fs::canonicalize(&dev).unwrap();
    let absolute_str = absolute.to_str().unwrap().to_owned();

    workhive_with_home(tmp.path())
        .args(["project", "roots", "add", &absolute_str, "--force"])
        .assert()
        .success();

    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&listed.get_output().stdout).to_string();

    let bullet_line = format!("  - {absolute_str}");
    assert!(
        stdout.contains(&bullet_line),
        "absolute configured root must round-trip verbatim — got stdout:\n{stdout}",
    );

    // Ensure no `~`-rewriting of the configured absolute path leaked through.
    assert!(
        !stdout.contains("- ~/"),
        "configured absolute path must not be displayed with `~/` rewriting — got:\n{stdout}",
    );
}

/// `~/<sub>` must round-trip verbatim too.
///
/// Users sometimes hand-edit `[projects] trusted_roots = ["~/dev"]`.
/// `project roots list` must show `~/dev` as configured, not the
/// expanded absolute form.
#[test]
fn project_roots_list_preserves_tilde_configured_value() {
    let tmp = setup_hermetic_home();
    let config_path = tmp.path().join(".config/workhive/config.toml");

    // Append an explicit `~/dev` entry. We rewrite the `[projects]` block to
    // make sure the stored value is the raw tilde form rather than something
    // re-canonicalized by `project roots add`.
    let original = std::fs::read_to_string(&config_path).unwrap();
    let mut without_projects = String::new();
    let mut in_projects_block = false;
    for line in original.lines() {
        if line.trim_start().starts_with("[projects]") {
            in_projects_block = true;
            continue;
        }
        if in_projects_block {
            if line.trim_start().starts_with('[') {
                in_projects_block = false;
            } else {
                continue;
            }
        }
        without_projects.push_str(line);
        without_projects.push('\n');
    }
    let appended = format!("{without_projects}\n[projects]\ntrusted_roots = [\"~/dev\"]\n");
    std::fs::write(&config_path, appended).unwrap();

    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&listed.get_output().stdout).to_string();
    assert!(
        stdout.contains("- ~/dev"),
        "`~/dev` configured root must round-trip verbatim — got stdout:\n{stdout}",
    );
}

/// `project roots add /` must be blocked by the overly-broad-root heuristic
/// without `--force`. This protects users from accidentally granting
/// filesystem-wide trust during an interactive session.
#[test]
fn project_roots_add_rejects_overly_broad_root_without_force() {
    let tmp = setup_hermetic_home();
    workhive_with_home(tmp.path())
        .args(["project", "roots", "add", "/"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("overly broad").or(predicate::str::contains("过宽")));
}

// ── trusted-root whitelist and project-index safety invariants ──

/// `project roots add` MUST validate path existence before mutating config.
/// A missing path must fail loudly with the localized "path does not exist"
/// message — no partial writes.
#[test]
fn project_roots_add_rejects_missing_path() {
    let tmp = setup_hermetic_home();
    let bogus = tmp.path().join("does-not-exist");

    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            bogus.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("does not exist").or(predicate::str::contains("不存在")));
}

/// `project roots add` MUST reject paths that exist but are not
/// directories (e.g. regular files). Trusted roots are a directory-only
/// concept by definition.
#[test]
fn project_roots_add_rejects_regular_file_path() {
    let tmp = setup_hermetic_home();
    let file_path = tmp.path().join("not-a-dir.txt");
    std::fs::write(&file_path, b"hi").unwrap();

    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            file_path.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("not a directory").or(predicate::str::contains("不是目录")),
        );
}

/// `project reindex --root <path>` must refuse paths that are
/// not contained within a configured trusted root. This is the core safety
/// guarantee — no escaping the whitelist via an explicit `--root` argument.
#[test]
fn project_reindex_rejects_root_outside_trusted_roots() {
    let tmp = setup_hermetic_home();
    let outsider = tempfile::TempDir::new().unwrap();

    workhive_with_home(tmp.path())
        .args([
            "project",
            "reindex",
            "--root",
            outsider.path().to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("not under any trusted project root")
                .or(predicate::str::contains("不属于任何已配置的可信项目目录")),
        );
}

/// A `--root` that lies *under* a configured trusted root must
/// reindex successfully. The whitelist is a containment check, not a strict
/// equality check.
#[test]
fn project_reindex_accepts_subdir_of_trusted_root() {
    let tmp = setup_hermetic_home();
    // The hermetic tmp.path() itself is registered as a trusted root by
    // `setup_hermetic_home`. A subdirectory inside it is therefore admitted.
    let subdir = tmp.path().join("dev-subdir");
    std::fs::create_dir_all(&subdir).unwrap();

    workhive_with_home(tmp.path())
        .args([
            "project",
            "reindex",
            "--root",
            subdir.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
}

/// `project roots add` must mutate `[projects].trusted_roots` via
/// `toml_edit`, preserving user comments and unrelated keys. A round-trip
/// through `toml::to_string_pretty` would silently delete this comment.
#[test]
fn project_roots_add_preserves_user_comments_in_global_toml() {
    // No-auto-trust home: the comment-preservation check
    // needs to actually exercise an insert/remove cycle on a child of
    // `tmp.path()`, which the redundancy guard would refuse if
    // `tmp.path()` were already trusted by setup.
    let tmp = setup_hermetic_home_without_auto_trust();
    let cfg_path = tmp.path().join(".config/workhive/config.toml");
    let original = std::fs::read_to_string(&cfg_path).unwrap();
    // Inject a free-floating comment plus a comment inside a non-trusted-roots
    // table. Both must survive the mutation.
    let marker_top = "# user-marker: top-level note that must survive";
    let marker_section = "# user-marker: tmux note that must survive";
    let mutated = format!(
        "{marker_top}\n{}",
        original.replace("[tmux]", &format!("{marker_section}\n[tmux]"))
    );
    std::fs::write(&cfg_path, &mutated).unwrap();

    let parent = tmp.path().join("dev-roots");
    std::fs::create_dir_all(&parent).unwrap();

    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    let after_add = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        after_add.contains(marker_top),
        "top-level user comment must survive `project roots add` toml_edit mutation, got:\n{after_add}",
    );
    assert!(
        after_add.contains(marker_section),
        "[tmux] user comment must survive `project roots add` toml_edit mutation, got:\n{after_add}",
    );
    assert!(
        after_add.contains("trusted_roots"),
        "trusted_roots key must be present after add, got:\n{after_add}",
    );

    workhive_with_home(tmp.path())
        .args(["project", "roots", "remove", parent.to_str().unwrap()])
        .assert()
        .success();

    let after_remove = std::fs::read_to_string(&cfg_path).unwrap();
    assert!(
        after_remove.contains(marker_top),
        "top-level user comment must survive `project roots remove` toml_edit mutation, got:\n{after_remove}",
    );
    assert!(
        after_remove.contains(marker_section),
        "[tmux] user comment must survive `project roots remove` toml_edit mutation, got:\n{after_remove}",
    );
}

/// Once a parent directory is trusted, adding a strict descendant must be
/// rejected with a localized message that names the covering root, and the
/// global config must NOT be rewritten with the redundant child entry.
///
/// The wizard, `project roots add`, and `project roots edit` all funnel
/// through `add_trusted_root` so this integration coverage on the
/// non-interactive `roots add` surface is the highest-leverage proof: it
/// exercises the full toml_edit path, the resolved i18n string, and the
/// fact that the on-disk config is not mutated.
#[test]
fn project_roots_add_rejects_redundant_child_under_existing_trusted_root() {
    // No-auto-trust home: we need `tmp.path()` itself to be UNtrusted
    // so we can deliberately install `parent` as the covering root and
    // then prove that `parent/child` is rejected by the redundancy
    // guard.
    let tmp = setup_hermetic_home_without_auto_trust();
    let cfg_path = tmp.path().join(".config/workhive/config.toml");

    // Set up a parent directory and trust it. This is the existing root
    // that should cover any descendants going forward.
    let parent = tmp.path().join("dev-redundant-cover");
    std::fs::create_dir_all(&parent).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    let after_parent_add = std::fs::read_to_string(&cfg_path).unwrap();

    // Now try to trust a child directory. The CLI must refuse and the
    // covering root must appear verbatim in the error so the user can
    // see *why* the entry was rejected.
    let child = parent.join("proj-a");
    std::fs::create_dir_all(&child).unwrap();
    let output = workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            child.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&output);
    // miette wraps long lines and prefixes each visual row with `│ `, so
    // the literal path can appear split across line breaks. Normalize
    // away whitespace and the diagnostic gutter characters before
    // substring matching, since the *content* of the message — not its
    // visual layout — is what carries the user-facing signal.
    let normalized_stderr: String = stderr
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '│' && *c != '×')
        .collect();
    let parent_str = parent.to_str().unwrap();
    let normalized_parent: String = parent_str.chars().filter(|c| !c.is_whitespace()).collect();
    // macOS canonicalizes `/var/folders/...` → `/private/var/folders/...`
    // when the engine resolves the configured trusted root for display.
    // Either spelling is acceptable as long as it identifies the
    // covering root that already trusts the child path.
    let normalized_parent_macos =
        normalized_parent.replace("/var/folders/", "/private/var/folders/");
    assert!(
        normalized_stderr.contains(&normalized_parent)
            || normalized_stderr.contains(&normalized_parent_macos),
        "covered-by-parent rejection must name the covering trusted root (`{parent_str}` or its canonical form), got: {stderr}",
    );
    // Anchor the message wording so future translations cannot silently
    // drop the "already covered" / redundancy intent.
    assert!(
        normalized_stderr.contains("alreadycovered")
            || normalized_stderr.contains("已被可信目录")
            || normalized_stderr.contains("已被")
            || normalized_stderr.contains("coveredbytrustedroot"),
        "rejection message must explain that the path is covered by an existing trusted root, got: {stderr}",
    );

    // Crucial: the global config must NOT have been mutated. The
    // redundant child entry would be noise at best and a misleading
    // trust signal at worst.
    let after_child_add = std::fs::read_to_string(&cfg_path).unwrap();
    assert_eq!(
        after_parent_add, after_child_add,
        "global config must not be rewritten when a redundant child trusted root is rejected",
    );
    assert!(
        !after_child_add.contains(child.to_str().unwrap()),
        "the redundant child path must NOT be persisted to disk, got config:\n{after_child_add}",
    );
}

/// When a removed trusted root has zero indexed projects beneath
/// it, removal proceeds without confirmation in non-interactive mode. This
/// is the baseline `--force`-not-needed path.
#[test]
fn project_roots_remove_succeeds_when_no_indexed_projects_affected() {
    // No-auto-trust home so the explicit `roots add parent` below is
    // not refused by the parent-coverage redundancy guard.
    let tmp = setup_hermetic_home_without_auto_trust();
    let parent = tmp.path().join("dev-empty");
    std::fs::create_dir_all(&parent).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    // No `workhive init` has been run inside `parent`, so the global index
    // is empty and removal needs no confirmation.
    workhive_with_home(tmp.path())
        .args(["project", "roots", "remove", parent.to_str().unwrap()])
        .assert()
        .success();
}

/// When a removed trusted root has indexed projects beneath it
/// and the call is non-interactive, the CLI MUST fail loudly and demand
/// `--force`. The trusted root must remain configured (no partial mutation).
#[test]
fn project_roots_remove_requires_force_when_projects_affected_non_interactive() {
    // No-auto-trust home so the explicit `roots add parent` below is
    // not refused by the parent-coverage redundancy guard.
    let tmp = setup_hermetic_home_without_auto_trust();
    let parent = tmp.path().join("dev-with-project");
    std::fs::create_dir_all(&parent).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // Initialize a project inside the trusted root so the global index
    // records at least one entry under `parent`.
    let project_dir = parent.join("proj-affected");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Non-interactive remove without --force: must fail and surface the
    // localized "requires --force" message.
    workhive_with_home(tmp.path())
        .args(["project", "roots", "remove", parent.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("--force")
                .or(predicate::str::contains("re-run"))
                .or(predicate::str::contains("--force")),
        );

    // The trusted root must still be present after the failed removal.
    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let listed_stderr = String::from_utf8_lossy(&listed.get_output().stderr).to_string();
    let listed_stdout = String::from_utf8_lossy(&listed.get_output().stdout).to_string();
    let combined = format!("{listed_stderr}\n{listed_stdout}");
    let parent_str = parent.to_string_lossy();
    assert!(
        combined.contains(parent_str.as_ref())
            // macOS canonicalizes `/var/folders/...` → `/private/var/...`,
            // so accept either orientation.
            || combined.contains(&parent_str.replace("/var/folders/", "/private/var/folders/")),
        "trusted root must remain configured after failed non-interactive remove, listed:\n{combined}",
    );

    // With `--force`, removal proceeds even though projects are affected.
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "remove",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
}

/// `workhive project roots edit` must be wired through the CLI
/// surface. We don't drive the interactive UI here, but we verify it returns
/// the expected "no TTY" failure path in non-interactive contexts so the
/// command is at minimum reachable and produces a localized error.
#[test]
fn project_roots_edit_requires_a_tty() {
    let tmp = setup_hermetic_home();
    workhive_with_home(tmp.path())
        .args(["project", "roots", "edit"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("requires a TTY").or(predicate::str::contains("需要 TTY")),
        );
}

/// `doctor` must surface a *project-index* state ("present" /
/// "missing" / "error: ..."), not the trusted-roots list header. Mixing the
/// two would produce confusing output like "Project index: Trusted project
/// roots:".
#[test]
fn doctor_reports_project_index_state_with_dedicated_label() {
    let tmp = setup_hermetic_home();
    // `doctor` may exit nonzero in CI/sandboxes where tmux/pty checks fail —
    // but we only care about the trusted-roots / project-index *labels* in
    // the rendered report, not the overall pass/fail status. Drive it via
    // `output()` and inspect stdout+stderr regardless of exit code.
    let output = workhive_with_home(tmp.path())
        .current_dir(tmp.path())
        .arg("doctor")
        .output()
        .expect("doctor command runs");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stderr}\n{stdout}");

    // The trusted-roots heading and the project-index state line must be
    // distinct. Specifically, the doctor output MUST NOT splice the
    // "Trusted project roots:" header into the project-index state slot.
    let project_index_marker_en = "Project index:";
    let project_index_marker_zh = "项目索引：";
    assert!(
        combined.contains(project_index_marker_en) || combined.contains(project_index_marker_zh),
        "doctor must emit a project-index state line, got:\n{combined}",
    );
    assert!(
        !combined.contains("Project index: Trusted project roots:"),
        "doctor must NOT reuse TRUSTED_ROOTS_LIST_HEADER for the project-index state slot, got:\n{combined}",
    );
    // Either localized state phrase must appear next to the index label.
    assert!(
        combined.contains("present")
            || combined.contains("missing")
            || combined.contains("已建立")
            || combined.contains("未建立"),
        "doctor must surface a localized index-state phrase, got:\n{combined}",
    );
}

/// `doctor` is diagnostic, not setup. Running it inside a git project that
/// has no `.workhive/` must not prompt for or perform project
/// initialization, while global diagnostics still render and the missing
/// project state is surfaced as an actionable warning.
#[test]
fn doctor_in_uninitialized_project_neither_prompts_nor_initializes() {
    let (tmp, project_dir) = setup_hermetic_project("diag-proj");

    let output = workhive_with_home(tmp.path())
        .current_dir(&project_dir)
        .arg("doctor")
        .output()
        .expect("doctor command runs");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stderr}\n{stdout}");

    // Must never offer to run init as part of a diagnostic command.
    assert!(
        !combined.contains("Run 'workhive init' now?"),
        "doctor must not prompt to initialize the project, got:\n{combined}"
    );
    // Must not create project state as a side effect.
    assert!(
        !project_dir.join(".workhive").exists(),
        "doctor must not create '.workhive/' in an uninitialized project"
    );
    // Global diagnostics still render.
    assert!(
        combined.contains("workhive doctor"),
        "doctor must still render its diagnostic header, got:\n{combined}"
    );
    assert!(
        combined.contains("Project index:"),
        "doctor must still render the global project-index panel, got:\n{combined}"
    );
    // The uninitialized project is reported as a warning, not intercepted.
    assert!(
        combined.contains("Project not initialized"),
        "doctor must surface an uninitialized-project warning, got:\n{combined}"
    );
}

/// Running `doctor` outside any project must report the condition exactly
/// once: the localized sentence lives in the report's warn line, and the
/// final error is only a brief exit summary. The same sentence rendered
/// twice in one run reads as a bug on this beginner-facing trust surface.
#[test]
fn doctor_outside_project_reports_condition_once_with_nonzero_exit() {
    let tmp = setup_hermetic_home();

    let output = workhive_with_home(tmp.path())
        .current_dir(tmp.path())
        .arg("doctor")
        .output()
        .expect("doctor command runs");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stderr}\n{stdout}");

    assert!(
        !output.status.success(),
        "doctor outside a project must keep its non-zero exit code"
    );
    let sentence = "Cannot detect project";
    let occurrences = combined.matches(sentence).count();
    assert_eq!(
        occurrences, 1,
        "the project-not-detected sentence must appear exactly once, got {occurrences}:\n{combined}"
    );
    // The global panel still renders so the operator can sanity-check the
    // installation itself.
    assert!(
        combined.contains("Project index:"),
        "doctor must still render the global project-index panel, got:\n{combined}"
    );
}

/// `project list` and `session list` must NOT advertise the legacy
/// `workhive project reindex --root <path>` hint anymore. The new hint
/// surface is driven by the typed `DiscoverySetupState`.
#[test]
fn list_commands_no_longer_print_root_path_reindex_hint() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // No trusted roots configured yet → hint should suggest adding one,
    // not running `reindex --root <path>`.
    let project_list = workhive_with_home(tmp.path())
        .args(["project", "list"])
        .assert();
    let project_stderr = String::from_utf8_lossy(&project_list.get_output().stderr).to_string();
    assert!(
        !project_stderr.contains("reindex --root"),
        "`project list` must not advertise legacy `reindex --root <path>` hint, got: {project_stderr}",
    );

    let session_list = workhive_with_home(tmp.path())
        .args(["session", "list"])
        .assert();
    let session_stderr = String::from_utf8_lossy(&session_list.get_output().stderr).to_string();
    assert!(
        !session_stderr.contains("reindex --root"),
        "`session list` must not advertise legacy `reindex --root <path>` hint, got: {session_stderr}",
    );
}

/// `session
/// attach` and `session list` MUST share the same setup-state guidance
/// when the project index is missing or empty. Specifically, when no
/// trusted roots are configured, attach must surface the
/// "no trusted project directories configured" hint — never the legacy
/// `SESSION_ATTACH_NO_INDEX` line that points at `reindex --root <path>`.
#[test]
fn session_attach_missing_index_uses_setup_state_guidance() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let attach = workhive_with_home(tmp.path())
        .args(["session", "attach"])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&attach.get_output().stderr).to_string();

    // Must NOT emit the legacy attach-specific copy that mentions
    // `reindex --root <path>` as the only path forward.
    assert!(
        !stderr.contains("reindex --root"),
        "`session attach` must not advertise legacy `reindex --root <path>` hint, got: {stderr}",
    );
    // Must emit the same guidance that `session list` would have printed:
    // either the localized "no trusted project directories configured"
    // hint, or its Chinese counterpart. We tolerate either locale because
    // the hermetic global config defaults are environment-derived.
    let mentions_no_trusted_roots = stderr.contains("No trusted project directories configured")
        || stderr.contains("未配置受信任的项目目录")
        || stderr.contains("受信任的项目目录尚未配置");
    assert!(
        mentions_no_trusted_roots,
        "`session attach` must surface the same trusted-roots guidance as `session list`, got: {stderr}",
    );
}

/// When the global config file is unreadable, `session
/// attach` must surface the same config-unreadable hint that `session
/// list` does, and still fail (because attach cannot proceed) without
/// regressing to the legacy `reindex --root` copy.
#[test]
fn session_attach_unreadable_global_config_emits_same_hint_as_list() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    // Corrupt the global config so `load_global` returns an error.
    let cfg_path = tmp.path().join(".config/workhive/config.toml");
    std::fs::write(&cfg_path, "= invalid toml ===\n").unwrap();

    let list_stderr = {
        let assert = workhive_with_home(tmp.path())
            .args(["session", "list"])
            .assert();
        String::from_utf8_lossy(&assert.get_output().stderr).to_string()
    };

    let attach_assert = workhive_with_home(tmp.path())
        .args(["session", "attach"])
        .assert()
        .failure();
    let attach_stderr = String::from_utf8_lossy(&attach_assert.get_output().stderr).to_string();

    // The "config unreadable" surface is shared via the
    // `load_global_for_hint` helper; both flows must emit a snippet
    // unique to that hint.
    let unreadable_marker = "config";
    assert!(
        list_stderr.to_lowercase().contains(unreadable_marker) || list_stderr.contains("配置"),
        "test setup invalid: `session list` did not surface a config-unreadable line, got: {list_stderr}",
    );
    assert!(
        attach_stderr.to_lowercase().contains(unreadable_marker) || attach_stderr.contains("配置"),
        "`session attach` must surface the same config-unreadable hint as `session list`, got: {attach_stderr}",
    );
    assert!(
        !attach_stderr.contains("reindex --root"),
        "`session attach` must not fall back to the legacy `reindex --root <path>` hint, got: {attach_stderr}",
    );
}

// ── Runtime reconstruction compatibility scenarios ───────────────────────
//
// These tests pin the post-snapshot-removal contract at the binary boundary.
#[test]
fn legacy_runtime_snapshot_files_are_ignored() {
    let (tmp, project_dir) = setup_hermetic_project("legacy-runtime-files");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let legacy_file = project_dir
        .join(".workhive")
        .join("snapshots")
        .join("legacy")
        .join("manifest.json");
    std::fs::create_dir_all(legacy_file.parent().unwrap()).unwrap();
    let legacy_contents = "not a current runtime artifact";
    std::fs::write(&legacy_file, legacy_contents).unwrap();

    workhive_with_home(tmp.path())
        .args(["config", "show", "--raw"])
        .current_dir(&project_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("snapshot").not());

    assert_eq!(
        std::fs::read_to_string(&legacy_file).unwrap(),
        legacy_contents,
        "commands must not read, migrate, or rewrite legacy runtime snapshots"
    );
}

// -- Trusted-root admission gates must not swallow
//    `load_global()` errors. These regression tests prove that a
//    malformed/unreadable global config surfaces a typed config error
//    instead of being silently relabeled as "no trusted roots configured"
//    or as a misleading trusted-root admission rejection. --

/// Corrupt the hermetic global `config.toml` in-place. The TOML parser must
/// reject the file with `ConfigError::InvalidValue`, which the CLI then
/// surfaces through `miette` rather than collapsing it into an empty
/// `TrustedRootSet`.
fn corrupt_global_config(home: &std::path::Path) {
    let cfg_path = home.join(".config/workhive/config.toml");
    std::fs::write(&cfg_path, "this is not [valid toml = \nbroken").unwrap();
}

/// Mutating admission gate (`enforce_trusted_root_or_error`) must propagate
/// the underlying config parse error. Running `workhive init` inside a
/// project directory while the global config is malformed must NOT print
/// "Outside trusted roots" or "no trusted roots configured" — those would
/// silently mislabel the actual fault.
#[test]
fn project_init_surfaces_config_error_when_global_config_is_unreadable() {
    let tmp = setup_hermetic_home();
    // Prepare a project directory under the hermetic home (so absent
    // corruption it would normally be admitted by the trusted-root set).
    let project_dir = tmp.path().join("crashy-proj");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    corrupt_global_config(tmp.path());

    let output = workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !stderr.contains("Outside trusted roots") && !stderr.contains("不在已配置的可信项目目录"),
        "config-parse failure must NOT be relabeled as 'outside trusted roots'; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("No trusted project roots configured")
            && !stderr.contains("尚未配置可信项目目录"),
        "config-parse failure must NOT be relabeled as 'no trusted roots configured'; got:\n{stderr}",
    );
    assert!(
        stderr.contains("Invalid value")
            || stderr.contains("InvalidValue")
            || stderr.contains("配置")
            || stderr.contains("config")
            || stderr.contains("toml"),
        "expected a typed config-error message, got:\n{stderr}",
    );
}

/// `project reindex --root <path>` must propagate global config parse
/// errors through the safety gate, not silently degrade the trusted-root
/// admission check to "empty set ⇒ rejected".
#[test]
fn project_reindex_explicit_root_surfaces_config_error_when_global_config_is_unreadable() {
    let tmp = setup_hermetic_home();
    let subdir = tmp.path().join("dev-subdir");
    std::fs::create_dir_all(&subdir).unwrap();

    corrupt_global_config(tmp.path());

    let output = workhive_with_home(tmp.path())
        .args([
            "project",
            "reindex",
            "--root",
            subdir.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !stderr.contains("not under any trusted project root")
            && !stderr.contains("不属于任何已配置的可信项目目录"),
        "explicit `--root` validation must NOT silently reject as 'untrusted' when the underlying config is unparseable; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("No trusted project roots configured")
            && !stderr.contains("尚未配置可信项目目录"),
        "explicit `--root` validation must NOT relabel a parse failure as 'no trusted roots configured'; got:\n{stderr}",
    );
    assert!(
        stderr.contains("Invalid value")
            || stderr.contains("InvalidValue")
            || stderr.contains("toml")
            || stderr.contains("配置"),
        "expected a typed config-error message, got:\n{stderr}",
    );
}

/// `session list` must return a non-zero exit code when the global config
/// is unreadable; config errors propagate rather than silently degrading to
/// an empty listing.
#[test]
fn session_list_unreadable_global_config_returns_nonzero_exit() {
    let tmp = setup_hermetic_home();
    corrupt_global_config(tmp.path());

    let output = workhive_with_home(tmp.path())
        .args(["session", "list", "--view", "json"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        stdout.trim().is_empty(),
        "session list --view json must emit empty stdout when config is unreadable, got: {stdout}"
    );
}

#[test]
fn project_list_json_unreadable_global_config_returns_nonzero_exit() {
    let tmp = setup_hermetic_home();
    corrupt_global_config(tmp.path());

    let output = workhive_with_home(tmp.path())
        .args(["project", "list", "--view", "json"])
        .assert()
        .failure()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(
        stdout.trim().is_empty(),
        "project list --view json must emit empty stdout when config is unreadable, got: {stdout}"
    );
}

#[test]
fn doctor_reports_physical_persisted_index_project_count() {
    let tmp = setup_hermetic_home_without_auto_trust();
    let trusted_project = tmp.path().join("trusted");
    let untrusted_project = tmp.path().join("untrusted");
    std::fs::create_dir_all(&trusted_project).unwrap();
    std::fs::create_dir_all(&untrusted_project).unwrap();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let raw_config = std::fs::read_to_string(&config_path).expect("read global config");
    let mut config: toml_edit::DocumentMut = raw_config.parse().expect("parse global config");
    if !config
        .as_table()
        .get("projects")
        .is_some_and(toml_edit::Item::is_table)
    {
        config["projects"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let mut trusted_roots = toml_edit::Array::new();
    trusted_roots.push(
        trusted_project
            .to_str()
            .expect("trusted project path must be UTF-8"),
    );
    config["projects"]["trusted_roots"] =
        toml_edit::Item::Value(toml_edit::Value::Array(trusted_roots));
    std::fs::write(&config_path, config.to_string()).expect("write global config");

    let index_path = tmp.path().join(".config/workhive/index.json");
    let index = workhive_infra::project_index::JsonProjectIndex::with_path(
        Utf8PathBuf::from_path_buf(index_path).expect("index path must be UTF-8"),
    );
    let projects = vec![
        workhive_core::ports::ProjectSummary {
            project_name: "trusted".to_owned(),
            project_root: Utf8PathBuf::from_path_buf(trusted_project.clone())
                .expect("trusted project path must be UTF-8"),
            config_path: Utf8PathBuf::from_path_buf(trusted_project.join(".workhive/config.toml"))
                .expect("trusted config path must be UTF-8"),
            last_opened_at: None,
            last_updated_at: None,
        },
        workhive_core::ports::ProjectSummary {
            project_name: "untrusted".to_owned(),
            project_root: Utf8PathBuf::from_path_buf(untrusted_project.clone())
                .expect("untrusted project path must be UTF-8"),
            config_path: Utf8PathBuf::from_path_buf(
                untrusted_project.join(".workhive/config.toml"),
            )
            .expect("untrusted config path must be UTF-8"),
            last_opened_at: None,
            last_updated_at: None,
        },
    ];
    index
        .replace_all(&projects)
        .expect("seed physical project index");
    match index.list_projects().expect("read seeded project index") {
        workhive_core::ports::IndexState::Loaded(entries) => assert_eq!(entries.len(), 2),
        other => panic!("seeded project index must be loaded, got {other:?}"),
    }

    let output = workhive_with_home(tmp.path())
        .current_dir(&trusted_project)
        .arg("doctor")
        .output()
        .expect("doctor command runs");

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let combined = format!("{stderr}\n{stdout}");

    let index_count_line = combined
        .lines()
        .find(|line| line.contains("Indexed projects:") || line.contains("索引中项目数："))
        .expect("doctor must emit an indexed-project count line");
    assert!(
        index_count_line.contains("Indexed projects: 2")
            || index_count_line.contains("索引中项目数：2"),
        "doctor must report exactly two physical projects, got: {index_count_line}"
    );

    let visible = workhive_with_home(tmp.path())
        .args(["project", "list", "--view", "json"])
        .output()
        .expect("project list command runs");
    assert!(visible.status.success(), "project list must succeed");
    let payload: Value = serde_json::from_slice(&visible.stdout).expect("project list JSON");
    let visible_projects = payload["projects"]
        .as_array()
        .expect("project list JSON must contain projects array");
    assert_eq!(
        visible_projects.len(),
        1,
        "trusted filtering must expose one project"
    );
    assert_eq!(visible_projects[0]["name"], "trusted");
}

// -- Trusted-root removal must FAIL CLOSED when the
//    global project index cannot be read or parsed. Logging a warning
//    and continuing with an empty affected-project list would silently
//    bypass the safety prompt exactly when the safety evidence was
//    untrustworthy. Both `roots remove` and `roots edit` Save route
//    through a shared helper that propagates
//    `IndexError::ReadFailed`/`Corrupted` as `CliError::Index`.
//
//    These tests cover:
//      1. `roots remove` aborts and does NOT rewrite `config.toml` when
//         `index.json` is corrupt JSON.
//      2. `roots remove` still succeeds when `index.json` is *missing*
//         (`IndexState::NotFound`), since that is a recognized setup
//         state, not a parse fault.

/// Overwrite the hermetic global `index.json` with non-JSON garbage so the
/// `JsonProjectIndex` adapter surfaces `IndexError::Corrupted` on the next
/// `list_projects()` call.
fn corrupt_global_project_index(home: &std::path::Path) {
    let index_path = home.join(".config/workhive/index.json");
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent).expect("global config dir must be creatable");
    }
    std::fs::write(&index_path, "{not valid json: ").expect("write malformed index.json");
}

/// `roots remove` MUST refuse to write `config.toml` when the global
/// project index is unreadable. Logging "proceeding without
/// affected-project warning" and continuing with an empty list is the
/// exact bypass this policy forbids — the safety evidence
/// is unknown, so the policy change must abort.
#[test]
fn roots_remove_fails_closed_when_project_index_is_corrupt() {
    // No-auto-trust home: the explicit `roots add parent` call must not
    // be rejected by the parent-coverage redundancy guard.
    let tmp = setup_hermetic_home_without_auto_trust();
    let parent = tmp.path().join("dev-corrupt-index");
    std::fs::create_dir_all(&parent).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // Initialize a project so the global index exists with at least one
    // entry, then deliberately corrupt the file. We need a *real* file on
    // disk to corrupt; `roots remove` must not silently treat the
    // resulting `IndexError::Corrupted` as "zero affected projects".
    let project_dir = parent.join("proj-corrupt");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");
    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();

    corrupt_global_project_index(tmp.path());

    // `--force` must not paper over an unreadable index either: the
    // safety evidence has to be loaded successfully *before* the prompt
    // policy is consulted. Even with `--force`, the helper must reject.
    let output = workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "remove",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        !stderr.contains("Removed trusted root") && !stderr.contains("已移除可信项目目录"),
        "removal must not be reported as successful when the index cannot be read; got:\n{stderr}",
    );
    assert!(
        stderr.contains("project index")
            || stderr.contains("项目索引")
            || stderr.contains("corrupted")
            || stderr.contains("index.json"),
        "expected a typed project-index failure message, got:\n{stderr}",
    );

    // The trusted root MUST still be configured. A silent fail-open would
    // have rewritten `config.toml` and stripped this entry.
    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let listed_stderr = String::from_utf8_lossy(&listed.get_output().stderr).to_string();
    let listed_stdout = String::from_utf8_lossy(&listed.get_output().stdout).to_string();
    let combined = format!("{listed_stderr}\n{listed_stdout}");
    let parent_str = parent.to_string_lossy();
    assert!(
        combined.contains(parent_str.as_ref())
            // macOS canonicalizes `/var/folders/...` → `/private/var/...`.
            || combined.contains(&parent_str.replace("/var/folders/", "/private/var/folders/")),
        "trusted root must remain configured after fail-closed removal, listed:\n{combined}",
    );
}

/// `roots remove` MUST still succeed when the global `index.json` simply
/// does not exist. A missing index is a known setup state — it cannot
/// imply unsafe affected projects, so the helper resolves it to an empty
/// project list and the command proceeds without the affected-project
/// warning. This guards the call site against an over-correction where a
/// stricter "fail on any non-Loaded state" rule would block valid setups.
#[test]
fn roots_remove_succeeds_when_project_index_is_not_found() {
    // No-auto-trust home: the explicit `roots add parent` call must not
    // be rejected by the parent-coverage redundancy guard.
    let tmp = setup_hermetic_home_without_auto_trust();
    let parent = tmp.path().join("dev-no-index");
    std::fs::create_dir_all(&parent).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "add",
            parent.to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();

    // `setup_hermetic_home` may or may not have created the index file
    // depending on prior init paths; explicitly remove it so we drive
    // the `IndexState::NotFound` branch deterministically.
    let index_path = tmp.path().join(".config/workhive/index.json");
    if index_path.exists() {
        std::fs::remove_file(&index_path).expect("delete global index.json");
    }
    assert!(
        !index_path.exists(),
        "test precondition: index.json must be absent to drive NotFound",
    );

    // No `--force` needed: with zero indexed projects the affected-count
    // is 0 and the prompt is skipped.
    workhive_with_home(tmp.path())
        .args(["project", "roots", "remove", parent.to_str().unwrap()])
        .assert()
        .success();

    // The removal should have rewritten `config.toml` and the entry
    // should now be gone.
    let listed = workhive_with_home(tmp.path())
        .args(["project", "roots", "list"])
        .assert()
        .success();
    let listed_stderr = String::from_utf8_lossy(&listed.get_output().stderr).to_string();
    let listed_stdout = String::from_utf8_lossy(&listed.get_output().stdout).to_string();
    let combined = format!("{listed_stderr}\n{listed_stdout}");
    let parent_str = parent.to_string_lossy();
    let canonical_parent_str = parent_str.replace("/var/folders/", "/private/var/folders/");
    assert!(
        !combined.contains(parent_str.as_ref())
            && !combined.contains(canonical_parent_str.as_str()),
        "trusted root must be removed after a successful NotFound-branch removal, listed:\n{combined}",
    );
}

// ── Real/gated tmux E2E for the FULL `.env`
// merge — global baseline + project overlay, project wins on conflict.
//
// Contract: `effective_env = global_env merged with project_env`; project
// keys override same-name global keys. The gated tmux E2E proves all
// three legs in a single run:
//     1. global-only key visible in the pane,
//     2. project-only key visible in the pane,
//     3. project key overrides global same-name key in the pane.
//
// We do this with a real tmux server (no fakes) by:
//
// 1. Writing the global `.env` (`<HOME>/.config/workhive/.env`) with:
//        CLAUDE_CODE_NO_FLICKER=1            # canonical user example
//        WORKHIVE_GLOBAL_ONLY=global-visible # global-only sentinel
//        WORKHIVE_ENV_SOURCE=global          # will be overridden
//    and the project `<root>/.workhive/.env` with:
//        WORKHIVE_PROJECT_ONLY=project-visible # project-only sentinel
//        WORKHIVE_ENV_SOURCE=project           # overrides global value
//    The values are unique strings so leakage from the host env or
//    from any cargo-test driver cannot satisfy the assertions by
//    accident.
// 2. Running `workhive workspace open <ws> --no-attach` to materialize
//    a real tmux session via `new-session -e KEY=VALUE`. This is the
//    very same surface a user would invoke; no internal API is poked.
// 3. Spawning a fresh tmux window inside that session whose initial
//    command is a one-shot shell that calls `printenv` once per key,
//    writes each captured value to its own file, then writes a done
//    marker. This bypasses pane-render timing flakiness and proves the
//    values flow into a real child-process environment, not just tmux's
//    session env table.
// 4. The parent test waits for the done marker with a hard deadline,
//    then reads the on-disk values and asserts on them.
//
// The test is gated behind `#[ignore]` because it spawns a real tmux
// server. Run command (also documented in the active log):
//
//   cargo test -p workhive-cli --test cli_integration \
//     -- --ignored e2e_open_injects_env_visible_via_printenv_in_real_pane
//
// `TmuxTestGuard` cleans the per-test tmux server on drop so the test
// will not leak PTY devices even on panic (the tmux test cleanup policy).

#[test]
#[ignore] // real-tmux: creates session via `open --no-attach` and probes pane env via `printenv`
fn e2e_open_injects_env_visible_via_printenv_in_real_pane() {
    let (tmp, project_dir, proj_name) = setup_git_project("env-e2e-printenv");
    let instance_id = read_instance_id(&project_dir);
    // RAII cleanup BEFORE the session-creating command and any
    // panic-prone assertion (the tmux test cleanup policy). If `open` fails before
    // creating a server, Drop's kill-server is harmless; if it creates
    // a partial server and then fails, the guard still cleans it up.
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Step 1a: write GLOBAL `.env` at the hermetic
    // `<HOME>/.config/workhive/.env` path that `workhive_with_home`
    // sets up via `HOME` + `XDG_CONFIG_HOME`. Three keys cover:
    //   * canonical user example  (CLAUDE_CODE_NO_FLICKER=1),
    //   * global-only baseline    (WORKHIVE_GLOBAL_ONLY=global-visible),
    //   * a key the project will override (WORKHIVE_ENV_SOURCE=global).
    let global_env_path = tmp.path().join(".config").join("workhive").join(".env");
    std::fs::create_dir_all(global_env_path.parent().expect("global env parent"))
        .expect("create global config dir");
    std::fs::write(
        &global_env_path,
        "# e2e fixture (global)\n\
         CLAUDE_CODE_NO_FLICKER=1\n\
         WORKHIVE_GLOBAL_ONLY=global-visible\n\
         WORKHIVE_ENV_SOURCE=global\n",
    )
    .expect("write global .env");

    // Step 1b: write PROJECT `.workhive/.env`. Two keys cover:
    //   * project-only key (WORKHIVE_PROJECT_ONLY=project-visible),
    //   * the same-name override (WORKHIVE_ENV_SOURCE=project).
    let project_env_path = project_dir.join(".workhive").join(".env");
    std::fs::write(
        &project_env_path,
        "# e2e fixture (project)\n\
         WORKHIVE_PROJECT_ONLY=project-visible\n\
         WORKHIVE_ENV_SOURCE=project\n",
    )
    .expect("write project .env");

    // Step 2: open the workspace (no attach so the test can keep
    // driving tmux). The session is created via `tmux new-session -e
    // KEY=VALUE` per the infra adapter contract.
    let workspace = "feat-env-e2e";
    workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    // Step 3: derive the canonical session name (matches
    // `SessionNamingPolicy::canonical`) and spawn a probe window
    // running a one-shot shell that records env values and signals.
    let session = format!("wh@{instance_id}@{proj_name}@{workspace}");
    let probe_dir = tmp.path().join("env-probe");
    std::fs::create_dir_all(&probe_dir).expect("create probe dir");
    let flicker_out = probe_dir.join("flicker.txt");
    let global_only_out = probe_dir.join("global_only.txt");
    let project_only_out = probe_dir.join("project_only.txt");
    let source_out = probe_dir.join("env_source.txt");
    let done_out = probe_dir.join("done");

    // The shell command MUST use `printenv` so the assertion reads the
    // process environment, not shell-expanded text.
    // We redirect to disk instead of `capture-pane`-ing because file
    // I/O is race-free vs terminal rendering. The final marker gives
    // the parent test a bounded completion condition.
    //
    // One `printenv KEY` per file proves every key flowed into the
    // pane process environment without any one of them masking
    // another at capture time.
    let probe_cmd = format!(
        "sh -c 'printenv CLAUDE_CODE_NO_FLICKER > {flicker} 2>/dev/null; \
         printenv WORKHIVE_GLOBAL_ONLY > {global_only} 2>/dev/null; \
         printenv WORKHIVE_PROJECT_ONLY > {project_only} 2>/dev/null; \
         printenv WORKHIVE_ENV_SOURCE > {source} 2>/dev/null; \
         : > {done}'",
        flicker = flicker_out.display(),
        global_only = global_only_out.display(),
        project_only = project_only_out.display(),
        source = source_out.display(),
        done = done_out.display(),
    );

    let new_window = std::process::Command::new("tmux")
        .env("TMUX_TMPDIR", guard.path())
        .args([
            "new-window",
            "-t",
            &format!("{session}:"),
            "-n",
            "env-probe",
            "-d",
            &probe_cmd,
        ])
        .output()
        .expect("spawn tmux probe window");
    assert!(
        new_window.status.success(),
        "tmux new-window failed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&new_window.stdout),
        String::from_utf8_lossy(&new_window.stderr),
    );

    // Step 4: wait for the probe shell to flush all four files.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done_out.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        done_out.exists(),
        "tmux env probe did not finish within 10s: flicker={:?} global_only={:?} project_only={:?} source={:?}",
        std::fs::read_to_string(&flicker_out).ok(),
        std::fs::read_to_string(&global_only_out).ok(),
        std::fs::read_to_string(&project_only_out).ok(),
        std::fs::read_to_string(&source_out).ok(),
    );

    // Step 5: read the captured `printenv` outputs and assert that
    // the merged effective env reaches the real pane process. We
    // deliberately compare the trimmed contents (printenv emits a
    // trailing newline) and we assert on every leg of the merge:
    //
    //   * canonical example   (global → pane);
    //   * global-only key     (global → pane);
    //   * project-only key    (project → pane, with no shadowing);
    //   * conflict resolution (project value wins over global).
    let read_trim = |path: &std::path::Path| -> String {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    let flicker = read_trim(&flicker_out);
    let global_only = read_trim(&global_only_out);
    let project_only = read_trim(&project_only_out);
    let source = read_trim(&source_out);

    assert_eq!(
        flicker, "1",
        "printenv CLAUDE_CODE_NO_FLICKER must reflect global .env (got {flicker:?})"
    );
    assert_eq!(
        global_only, "global-visible",
        "printenv WORKHIVE_GLOBAL_ONLY must reflect global-only .env baseline (got {global_only:?})"
    );
    assert_eq!(
        project_only, "project-visible",
        "printenv WORKHIVE_PROJECT_ONLY must reflect project-only overlay (got {project_only:?})"
    );
    assert_eq!(
        source, "project",
        "printenv WORKHIVE_ENV_SOURCE must reflect project value overriding the global same-name key (got {source:?})"
    );
}

// ── Real CLI test for `.env` user-visible parse warning.
//
// The `.env` parser plumbing is unit-tested, but the user-visible
// contract lives on the actual stderr surface of
// `workhive workspace open`. This test drives the real
// CLI binary, captures stderr, and asserts:
//
// 1. the localized `.env` parse warning appears on stderr with
//    source path + 1-based line number + localized YAML-syntax reason,
// 2. the warning never echoes the raw malformed line nor the
//    YAML-style value (`super-secret-value`),
// 3. the injection summary on stderr lists the valid `GOOD_ENV` key
//    but NEVER lists `CLAUDE_CODE_NO_FLICKER` (which was rejected by
//    the parser and so cannot be in the effective env).
//
// The test is gated behind `#[ignore]` because `workspace open`
// spawns a real tmux server (every other `--no-attach` open test in
// this file is gated for the same reason). The default-running
// renderer-level coverage of the same contract — including the
// Chinese locale — lives in
// `crates/workhive-cli/src/commands/open.rs::tests::format_env_parse_warning_*`.
//
// Run command (also documented in the active log):
//
//   cargo test -p workhive-cli --test cli_integration \
//     -- --ignored e2e_open_emits_localized_env_parse_warning_and_redacts_raw_line

#[test]
#[ignore] // real-tmux: drives `workspace open --no-attach` and asserts on its real stderr
fn e2e_open_emits_localized_env_parse_warning_and_redacts_raw_line() {
    let (tmp, project_dir, _proj_name) = setup_git_project("env-parse-warn");
    // RAII cleanup BEFORE the session-creating command and any
    // panic-prone assertion (the tmux test cleanup policy). If `open` fails before
    // creating a server, Drop's kill-server is harmless; if it creates
    // a partial server and then fails, the guard still cleans it up.
    let guard = TmuxTestGuard::with_project_dir(&project_dir);

    // Write a malformed GLOBAL `.env` using the canonical YAML-style
    // mistake fixture:
    //
    //   CLAUDE_CODE_NO_FLICKER: 1     ← YAML-style, line 1 (must warn,
    //                                    must NOT inject the key)
    //   API_KEY: super-secret-value   ← YAML-style, line 2 (must warn,
    //                                    must NOT leak the value)
    //   GOOD_ENV=ok                   ← valid, must be in summary
    let global_env_path = tmp.path().join(".config").join("workhive").join(".env");
    std::fs::create_dir_all(global_env_path.parent().expect("global env parent"))
        .expect("create global config dir");
    std::fs::write(
        &global_env_path,
        "CLAUDE_CODE_NO_FLICKER: 1\n\
         API_KEY: super-secret-value\n\
         GOOD_ENV=ok\n",
    )
    .expect("write malformed global .env");

    // Drive the real CLI surface that a user would invoke. We assert
    // on its stderr (where parse warnings + injection summary are
    // emitted) regardless of overall exit status — the warning surface
    // and the secret-redaction policy are user-visible properties of
    // the open flow.
    let workspace = "feat-warn";
    let assert = workhive_with_home(tmp.path())
        .env("TMUX_TMPDIR", guard.path())
        .args(["workspace", "open", workspace, "--no-attach"])
        .current_dir(&project_dir)
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();

    // ── (1) the warning surface includes path + line + localized reason. ──
    assert!(
        stderr.contains("warning:"),
        "stderr must include localized parse-warning prefix, got:\n{stderr}"
    );
    assert!(
        stderr.contains(global_env_path.to_string_lossy().as_ref()),
        "stderr must include the global .env path, got:\n{stderr}"
    );
    assert!(
        stderr.contains(" line 1") && stderr.contains(" line 2"),
        "stderr must include 1-based line numbers for BOTH malformed lines (1 and 2), got:\n{stderr}"
    );
    assert!(
        stderr.contains("KEY=VALUE") && stderr.contains("YAML"),
        "stderr must include the localized YAML-style reason text, got:\n{stderr}"
    );

    // ── (2) the warning surface NEVER leaks raw line text or values. ──
    for forbidden in [
        "CLAUDE_CODE_NO_FLICKER: 1",
        "API_KEY: super-secret-value",
        "super-secret-value",
        ": super-secret-value",
        ": 1\n",
    ] {
        assert!(
            !stderr.contains(forbidden),
            "stderr must NOT leak raw .env line/value fragment {forbidden:?}, got:\n{stderr}"
        );
    }

    // ── (3) the injection summary lists the VALID key only. ──
    //
    // The summary uses `open.env.injected_global_only` / `..._mixed`
    // for the header and `open.env.key_item` ("  {0}") for each entry.
    // We assert on the per-key list directly: GOOD_ENV must appear
    // there, and the malformed key must never appear in stderr at
    // all (it was rejected at parse time and so cannot reach
    // `EffectiveEnv.entries`).
    assert!(
        stderr.contains("  GOOD_ENV"),
        "injection summary must list the valid key GOOD_ENV, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("CLAUDE_CODE_NO_FLICKER"),
        "stderr must NOT mention CLAUDE_CODE_NO_FLICKER (parse-rejected, never injected), got:\n{stderr}"
    );
    assert!(
        !stderr.contains("API_KEY"),
        "stderr must NOT mention API_KEY (parse-rejected, never injected), got:\n{stderr}"
    );
}

// ── Trusted-root admission must consistently gate every mutating
//    `workspace open` flow. ──

/// Initialize a project under a trusted root (so `init` succeeds), then
/// remove that trust, then run `workspace open` against the same
/// already-initialized project. Even though `.workhive/` already exists,
/// the trusted-root admission gate MUST reject the open before any
/// further mutation (tmux session, runtime state, snapshot, identity
/// metadata, worktree) takes place.
#[test]
fn workspace_open_already_initialized_outside_trusted_roots_is_rejected() {
    let tmp = setup_hermetic_home();
    // Init under trusted root so `.workhive/` is real and represents an
    // already-initialized project. Use a subdir so we can later
    // narrow the trusted set to exclude it.
    let project_dir = tmp.path().join("init-then-distrust").join("repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--force"])
        .current_dir(&project_dir)
        .assert()
        .success();
    assert!(
        project_dir.join(".workhive").exists(),
        "precondition: project must be initialized before we revoke trust",
    );

    // Now revoke trust by replacing the trusted-root list with a
    // disjoint sibling directory. The previously initialized project
    // must NOT be grandfathered.
    let other_trusted = tmp.path().join("only-trusted-elsewhere");
    std::fs::create_dir_all(&other_trusted).unwrap();
    workhive_with_home(tmp.path())
        .args([
            "project",
            "roots",
            "remove",
            tmp.path().to_str().unwrap(),
            "--force",
        ])
        .assert()
        .success();
    register_trusted_root(tmp.path(), &other_trusted);

    let assert = workhive_with_home(tmp.path())
        .args(["workspace", "open", "--no-attach", "main"])
        .current_dir(&project_dir)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        stderr.contains("trusted") || stderr.contains("可信"),
        "rejection must mention trusted-root policy in localized form, got:\n{stderr}",
    );
}

/// `workspace open` against a project that has NEVER been initialized
/// and sits outside trusted roots. With `UseProjectRoot` intent (opening
/// the main branch), the admission gate must still fire BEFORE any
/// init/auto-init prompt or runtime mutation.
#[test]
fn workspace_open_use_project_root_outside_trusted_roots_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let project_dir = tmp.path().join("untrusted-main-open");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    // Deliberately NOT registering trusted root for project_dir.

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "--no-attach", "main"])
        .current_dir(&project_dir)
        .assert()
        .failure();

    assert!(
        !project_dir.join(".workhive").exists(),
        ".workhive/ must NOT be created when `workspace open` (UseProjectRoot) is gated by trusted-roots policy",
    );
}

/// The localized rejection from the trusted-root admission gate MUST
/// help the user understand the configured boundary: it must include
/// the configured trusted roots (in their authored form, e.g. `~/Jts/`)
/// or a direct command for inspecting/adding them.
#[test]
fn workspace_open_outside_trusted_roots_message_includes_configured_roots() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Create a dedicated trusted-root that the project does NOT live in.
    let trusted_dir = tmp.path().join("trusted-only");
    std::fs::create_dir_all(&trusted_dir).unwrap();
    let project_dir = tmp.path().join("not-in-trusted");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), &trusted_dir);

    // Pass `--init` so the open flow actually tries to mutate state
    // and reaches the trusted-root admission gate, instead of failing
    // earlier on "project not initialized".
    let assert = workhive_with_home(tmp.path())
        .args(["workspace", "open", "--no-attach", "--init", "main"])
        .current_dir(&project_dir)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();

    // Must mention the configured trusted root somewhere in the message
    // OR offer a direct command to inspect them. The inspect-command hint
    // MUST be the actual CLI surface, i.e. `workhive project roots list`,
    // never the stale `workhive project roots show` (which is not a real
    // subcommand and only confuses users).
    let trusted_str = trusted_dir.to_string_lossy().to_string();
    let mentions_configured = stderr.contains(&trusted_str);
    let mentions_inspect = stderr.contains("project roots list") || stderr.contains("查看可信目录");
    let mentions_stale_show_hint = stderr.contains("project roots show");
    assert!(
        !mentions_stale_show_hint,
        "rejection MUST NOT hint the non-existent `workhive project roots show` \
         command; it must hint the real `workhive project roots list` instead. \
         got:\n{stderr}",
    );
    assert!(
        mentions_configured || mentions_inspect,
        "rejection must include configured trusted roots {trusted_str:?} \
         or a localized inspect-command hint, got:\n{stderr}",
    );
}

/// Path-component matching: `~/Jts` must NOT admit `~/JtsTwins` even
/// though the latter is a string-prefix of the former. Regression for
/// the bug doc's "by path components, not raw string prefix" decision.
#[test]
fn workspace_open_string_prefix_sibling_outside_trusted_roots_is_rejected() {
    let tmp = tempfile::TempDir::new().unwrap();
    let trusted = tmp.path().join("Jts");
    let sibling = tmp.path().join("JtsTwins");
    std::fs::create_dir_all(&trusted).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    let project_dir = sibling.join("repo");
    std::fs::create_dir_all(&project_dir).unwrap();
    std::process::Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&project_dir)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&project_dir)
        .output()
        .expect("git commit");

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), &trusted);

    workhive_with_home(tmp.path())
        .args(["workspace", "open", "--no-attach", "main"])
        .current_dir(&project_dir)
        .assert()
        .failure();
    assert!(
        !project_dir.join(".workhive").exists(),
        "string-prefix sibling `JtsTwins` must NOT be admitted as descendant of `Jts`",
    );
}

/// `workhive project reindex` MUST be resilient to unreadable
/// descendant directories beneath a valid trusted root. A single
/// unreadable subtree must not abort the entire reindex; sibling
/// Workhive projects under the same trusted root MUST still be
/// discovered and surface in the index, and the skipped subtree MUST
/// be reported as a warning on stderr.
///
/// Top-level trusted roots that are themselves unreadable still
/// hard-fail; that case has its own dedicated regression test.
#[cfg(unix)]
#[test]
fn reindex_skips_unreadable_descendant_and_still_discovers_sibling_project() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::TempDir::new().unwrap();
    let trusted = tmp.path().join("trusted");
    std::fs::create_dir_all(&trusted).unwrap();

    // Sibling Workhive project under the trusted root.
    let good = trusted.join("good-project");
    let wh = good.join(".workhive");
    std::fs::create_dir_all(&wh).unwrap();
    std::fs::write(wh.join("config.toml"), "[default]\n").unwrap();
    std::fs::write(wh.join("instance_id"), "a1b2").unwrap();

    // Unreadable descendant — mimics `MobileSync` on macOS.
    let locked = trusted.join("locked-subtree");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();
    register_trusted_root(tmp.path(), &trusted);

    let assert = workhive_with_home(tmp.path())
        .args(["project", "reindex", "--force"])
        .assert();

    // Restore permissions so TempDir cleanup can succeed regardless of
    // assertion outcome.
    let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));

    let assert = assert.success();
    let output = assert.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The skipped subtree must appear in the warning list on stderr.
    assert!(
        stderr.contains("locked-subtree"),
        "reindex stderr must surface skipped subtree, got:\n{stderr}",
    );

    // The good sibling project must still appear in the index.
    let list_out = workhive_with_home(tmp.path())
        .args(["project", "list"])
        .assert()
        .success();
    let list_stdout = String::from_utf8_lossy(&list_out.get_output().stdout);
    let list_stderr = String::from_utf8_lossy(&list_out.get_output().stderr);
    let combined = format!("{list_stdout}{list_stderr}");
    assert!(
        combined.contains("good-project"),
        "sibling Workhive project must be discovered even when a peer \
         subtree was unreadable; combined output:\n{combined}",
    );
}

/// Regression: NO user-facing localized string may mention the stale
/// command `workhive project roots show`. The real subcommand is
/// `workhive project roots list`, and surfacing a non-existent command
/// in a rejection / hint string actively misleads users who try to run
/// the suggested command.
///
/// This test scans the source-of-truth i18n catalog module
/// (`workhive-infra/src/i18n/`) for the banned substring in any
/// user-facing string literal. The catalog is the single chokepoint for
/// all localized user-visible text in workhive, so scanning it gives
/// blanket coverage across both English and Chinese.
#[test]
fn no_user_facing_string_mentions_stale_project_roots_show_hint() {
    const BANNED: &str = "project roots show";
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let catalog_dir = manifest_dir
        .parent()
        .expect("workhive-cli has a parent crates dir")
        .join("workhive-infra")
        .join("src")
        .join("i18n");

    fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read dir {dir:?}: {e}")) {
            let path = entry.expect("dir entry is readable").path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    let mut catalog_files = Vec::new();
    collect_rs_files(&catalog_dir, &mut catalog_files);
    assert!(
        !catalog_files.is_empty(),
        "i18n catalog dir {catalog_dir:?} must contain Rust sources",
    );

    let mut banned_lines: Vec<String> = Vec::new();
    for path in &catalog_files {
        let catalog =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        for (n, line) in catalog.lines().enumerate() {
            if line.contains(BANNED) {
                banned_lines.push(format!("  {}:{}: {}", path.display(), n + 1, line));
            }
        }
    }
    assert!(
        banned_lines.is_empty(),
        "User-facing i18n catalog ({catalog_dir:?}) must not surface the \
         non-existent `workhive project roots show` subcommand. The real \
         subcommand is `workhive project roots list`. Offending lines:\n{}",
        banned_lines.join("\n"),
    );
}

// ── Automatic AI-integration lifecycle is hidden from normal users ──────
//
// `workhive integrations` is not a public user command. The tests below
// pin the CLI-surface contract — `workhive --help` (both EN and ZH)
// must not advertise `integrations`, and every
// `workhive integrations …` invocation must be rejected as an
// unknown subcommand with exit code 2.

/// English `--help` must not advertise `integrations`.
#[test]
fn english_top_level_help_does_not_advertise_integrations() {
    let stdout = help_stdout_for(&["--help"]);
    assert!(
        !stdout.contains("integrations"),
        "EN top-level help must not advertise the removed \
         `integrations` command, got:\n{stdout}",
    );
}

/// Chinese `--help` must not advertise `integrations` and must not
/// leak the English doc comment that previously described it.
#[test]
fn chinese_top_level_help_does_not_advertise_integrations() {
    let out = workhive()
        .env("LANG", "zh_CN.UTF-8")
        .env("LC_ALL", "zh_CN.UTF-8")
        .arg("--help")
        .output()
        .expect("clap renders help to stdout and exits 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("integrations"),
        "ZH top-level help must not advertise the removed \
         `integrations` command, got:\n{stdout}",
    );
}

/// `workhive integrations` must fail with the clap "unknown subcommand"
/// exit code (2 per the exit-code contract) — not a routing surprise.
#[test]
fn workhive_integrations_is_unknown_subcommand() {
    let out = workhive().arg("integrations").output().expect("spawn");
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        2,
        "`workhive integrations` must be rejected as an unknown subcommand (exit 2), \
         got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn workhive_integrations_status_is_unknown_subcommand() {
    let out = workhive()
        .args(["integrations", "status"])
        .output()
        .expect("spawn");
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        2,
        "`workhive integrations status` must be rejected (exit 2), \
         got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn workhive_integrations_install_claude_is_unknown_subcommand() {
    let out = workhive()
        .args(["integrations", "install", "claude"])
        .output()
        .expect("spawn");
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        2,
        "`workhive integrations install claude` must be rejected (exit 2), \
         got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn workhive_integrations_remove_codex_is_unknown_subcommand() {
    let out = workhive()
        .args(["integrations", "remove", "codex"])
        .output()
        .expect("spawn");
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        2,
        "`workhive integrations remove codex` must be rejected (exit 2), \
         got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// `workhive integrations` must NOT have been moved into the hidden
/// `internal` namespace either: the removal forbids
/// preserving the CLI under any namespace.
#[test]
fn workhive_internal_integrations_is_unknown_subcommand() {
    let out = workhive()
        .args(["internal", "integrations"])
        .output()
        .expect("spawn");
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        2,
        "`workhive internal integrations` must be rejected (exit 2), \
         got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// The non-AI / non-AI-tool path on the `internal pane-supervisor`
/// handler must not materialize any Claude / Codex / OpenCode config
/// as a side-effect. We exercise this by invoking the supervisor with
/// a non-existent program (so spawn fails fast, exit code 127), and
/// then assert the third-party config dirs remain empty under an
/// isolated `HOME`/`XDG_CONFIG_HOME`.
///
/// This is the load-bearing safety boundary: provider detection must
/// use typed metadata; we do not
/// write Claude/Codex/OpenCode config for non-AI panes.
#[test]
fn pane_supervisor_does_not_write_third_party_config_for_shell_launches() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path().join("home");
    let xdg = tmp.path().join("xdg");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&xdg).unwrap();
    let runtime = home.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();

    let out = workhive_with_home(&home)
        .env("XDG_CONFIG_HOME", &xdg)
        .args([
            "internal",
            "pane-supervisor",
            "--runtime-root",
            runtime.to_str().unwrap(),
            "--group",
            "g",
            "--workspace",
            "w",
            "--unit",
            "u",
            "--tool",
            "lazygit", // non-AI basename → AiActivityProvider::None
            "--",
            // intentionally non-existent program so spawn fails fast
            "/this/path/does/not/exist/please",
        ])
        .output()
        .expect("spawn");

    // 127 is the documented spawn-failure exit code from the
    // pane_supervisor handler.
    let code = out.status.code().unwrap_or(-1);
    assert_eq!(
        code,
        127,
        "non-AI supervisor launch with a missing program should surface \
         spawn-failed (127); got code={code}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    // No Claude / Codex / OpenCode third-party config files should
    // have been materialized as a side-effect of routing the
    // non-AI launch through the supervisor.
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "non-AI launch must not create ~/.claude/settings.json",
    );
    assert!(
        !home.join(".codex").join("config.toml").exists(),
        "non-AI launch must not create ~/.codex/config.toml",
    );
    assert!(
        !xdg.join("opencode")
            .join("plugins")
            .join("workhive.mjs")
            .exists(),
        "non-AI launch must not create the OpenCode workhive plugin",
    );
}

/// Truncating stdout with a closed pipe reader (the `| head` / quit-pager
/// scenario) must terminate the process the way standard Unix CLIs do —
/// killed by `SIGPIPE` — and never surface a Rust `BrokenPipe` panic
/// (exit 101).
#[cfg(unix)]
#[test]
fn truncated_stdout_pipe_terminates_via_sigpipe_not_panic() {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::ExitStatusExt;

    // Build a pipe and close the read end BEFORE the child writes, so the
    // child's first stdout write deterministically raises `EPIPE`.
    let mut fds = [0i32; 2];
    // SAFETY: plain pipe(2) on a stack array of two fds.
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    assert_eq!(rc, 0, "pipe(2) failed");
    let (read_fd, write_fd) = (fds[0], fds[1]);
    // SAFETY: write_fd is a freshly created, owned fd; Stdio takes ownership.
    let child_stdout = unsafe { std::process::Stdio::from_raw_fd(write_fd) };
    // SAFETY: read_fd is a freshly created, owned fd closed exactly once.
    unsafe { libc::close(read_fd) };

    // `completion script` is a stdout-only path that needs no global config.
    let status = std::process::Command::new(assert_cmd::cargo::cargo_bin("workhive"))
        .args(["completion", "script", "zsh"])
        .stdout(child_stdout)
        .stderr(std::process::Stdio::null())
        .status()
        .expect("spawn workhive");

    assert_ne!(
        status.code(),
        Some(101),
        "a truncated stdout pipe must never produce a Rust panic exit (101)"
    );
    assert_eq!(
        status.signal(),
        Some(libc::SIGPIPE),
        "process should be terminated by SIGPIPE like standard Unix CLIs; got {status:?}"
    );
}

#[test]
fn tmux_interaction_config_set_mouse_false() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "tmux.mouse",
            "false",
            "--global",
            "--force",
        ])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("mouse = false"),
        "config should contain 'mouse = false' after set"
    );
}

#[test]
fn tmux_interaction_config_set_clipboard_false() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "tmux.clipboard",
            "false",
            "--global",
            "--force",
        ])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("clipboard = false"),
        "config should contain 'clipboard = false' after set"
    );
}

#[test]
fn tmux_interaction_config_unset_mouse() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args([
            "config",
            "set",
            "tmux.mouse",
            "false",
            "--global",
            "--force",
        ])
        .assert()
        .success();

    workhive_with_home(tmp.path())
        .args(["config", "unset", "tmux.mouse", "--global", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        !content.contains("mouse = false"),
        "mouse = false should be removed after unset"
    );
}

#[test]
fn tmux_interaction_init_writes_defaults() {
    let tmp = tempfile::TempDir::new().unwrap();
    workhive_with_home(tmp.path())
        .args(["init", "--global", "--backend", "tmux", "--force"])
        .assert()
        .success();

    let config_path = tmp.path().join(".config/workhive/config.toml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("mouse = true"),
        "init should write 'mouse = true' by default"
    );
    assert!(
        content.contains("clipboard = true"),
        "init should write 'clipboard = true' by default"
    );
}

// ── workspace_scope_context_ — production topology/scope resolver tests ──

#[test]
fn workspace_scope_context_resolve_subdir_key_produces_rpp() {
    use camino::Utf8PathBuf;
    use workhive_core::usecases::naming::resolve_subdir_key;

    let root = Utf8PathBuf::from("/proj");
    let cwd_root = Utf8PathBuf::from("/proj");
    let cwd_sub = Utf8PathBuf::from("/proj/services/api");

    let rpp_root = resolve_subdir_key(&root, Some(cwd_root.as_ref()));
    let rpp_sub = resolve_subdir_key(&root, Some(cwd_sub.as_ref()));

    assert!(rpp_root.is_none(), "project root must produce no rpp");
    assert_eq!(
        rpp_sub.as_ref().map(|p| p.as_str()),
        Some("services/api"),
        "subdir must produce matching rpp"
    );
}

#[test]
fn workspace_scope_context_managed_worktree_subdir_preserved() {
    use camino::Utf8PathBuf;
    use workhive_core::usecases::naming::resolve_subdir_key;

    let root = Utf8PathBuf::from("/proj");
    let cwd_wt_root = Utf8PathBuf::from("/proj/.workhive/worktrees/feat-x");
    let cwd_wt_sub = Utf8PathBuf::from("/proj/.workhive/worktrees/feat-x/lib/core");

    let rpp_wt_root = resolve_subdir_key(&root, Some(cwd_wt_root.as_ref()));
    let rpp_wt_sub = resolve_subdir_key(&root, Some(cwd_wt_sub.as_ref()));

    assert!(rpp_wt_root.is_none(), "worktree root must produce no rpp");
    assert_eq!(
        rpp_wt_sub.as_ref().map(|p| p.as_str()),
        Some("lib/core"),
        "worktree subdirectory must be preserved as rpp"
    );
}

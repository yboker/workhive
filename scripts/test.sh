#!/usr/bin/env bash
#
# Project-local test runner and verification-tier entry point.
#
# This runner is purely additive: every mode orchestrates existing project
# commands, run serially with fail-fast semantics. Direct
# `cargo fmt`, `cargo clippy`, and `cargo test` commands remain valid and
# behave exactly as before. The runner does not implement any product
# assertions; it only selects and orders existing test suites.
#
# Verification tiers (run from cheapest/narrowest to broadest):
#
#   focused <area>...  Narrow tests for one or more changed
#                      command/module/workflow areas.
#   batch <name>       A named, durable union of focused areas plus a cheap
#                      deterministic check. Batch names describe a workflow
#                      domain (e.g. `cli-surface`), not a transient task.
#   full               Final quality gates (fmt, all-target clippy, all Rust
#                      targets and doctests, and dependency-boundary checks).
#   e2e                Real-dependency / ignored tests (tmux and Herdr). Opt-in
#                      only; never part of focused, batch, or full.
#
# A focused command that matches zero tests is treated as a failure: a
# green run with no executed tests is not behavioral evidence.
#
# Timeout controls:
#
#   WORKHIVE_COMMAND_TIMEOUT_SECS  Ordinary runner commands. Default: 300.
#   WORKHIVE_TEST_TIMEOUT_SECS     Focused/batch cargo test commands. Default: 600.
#   WORKHIVE_FULL_TIMEOUT_SECS     Full-tier clippy/test commands. Default: 600.
#   WORKHIVE_E2E_TIMEOUT_SECS      Real-dependency E2E tests. Default: 600.
#
# Set any value to 0 to disable that timeout class.

set -euo pipefail

# Resolve the workspace root from this script's location so the runner
# works regardless of the caller's current directory.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

# Focused areas are grouped by workflow area, not by transient task names,
# so the runner stays valid as individual work items come and go.
FOCUSED_AREAS="doctor help pane-show cli pane i18n integration \
input-widgets workspace-list workspace-open workspace-attach session-list \
project-list worktree-list"

# Named batches are durable workflow-domain groupings. They do not encode
# the currently in-flight work, so archiving a work item never makes a
# batch stale.
BATCHES="cli-surface trusted-root workspace-surfaces cli-responsive"

COMMAND_TIMEOUT_SECS="${WORKHIVE_COMMAND_TIMEOUT_SECS:-300}"
TEST_TIMEOUT_SECS="${WORKHIVE_TEST_TIMEOUT_SECS:-600}"
FULL_TIMEOUT_SECS="${WORKHIVE_FULL_TIMEOUT_SECS:-600}"
E2E_TIMEOUT_SECS="${WORKHIVE_E2E_TIMEOUT_SECS:-600}"

usage() {
    cat <<'EOF'
workhive test runner

Usage:
  scripts/test.sh list
  scripts/test.sh focused <area>...
  scripts/test.sh batch <name>
  scripts/test.sh full
  scripts/test.sh e2e

Timeout environment variables:
  WORKHIVE_COMMAND_TIMEOUT_SECS  Ordinary runner commands. Default: 300.
  WORKHIVE_TEST_TIMEOUT_SECS     Focused/batch cargo test commands. Default: 600.
  WORKHIVE_FULL_TIMEOUT_SECS     Full-tier clippy/test commands. Default: 600.
  WORKHIVE_E2E_TIMEOUT_SECS      Real-dependency E2E tests. Default: 600.
  Set a value to 0 to disable that timeout class.

Modes:
  list               Print supported modes, focused areas, and batches.
  focused <area>...  Run the focused tests for one or more areas (below).
  batch <name>       Run a named batch: a durable union of focused areas
                     plus a cheap deterministic format check.
  full               Run the final quality gates: fmt check, all-target
                     clippy, all Rust targets and doctests, and boundary lint.
  e2e                Run real-dependency / ignored tests (tmux and Herdr).
                     Opt-in only; excluded from focused, batch, and full.

Focused areas:
  doctor          doctor diagnostics behavior and command policy.
  help            top-level help command surface and localization.
  pane-show       pane show optional-target parsing and resolution.
  cli             CLI argument parsing.
  pane            pane command/render/resolution modules.
  i18n            localization rendering and parse-error text.
  integration     CLI integration tests (non-ignored only).
  input-widgets   validated-input retry state and plain text input.
  workspace-list  workspace list command rendering and selection.
  workspace-open  workspace open selector and readiness checks.
  workspace-attach
                  workspace/session attach selector and friendly errors.
  session-list    session list command behavior.
  project-list    project command surface incl. trusted-roots edit.
  worktree-list   workspace worktree list command behavior.

Batches:
  cli-surface     CLI command-surface regression: doctor, help, and
                  pane-show focused areas plus a fmt check.
  trusted-root    Trusted-root entry surface: input-widgets plus
                  localization plus a fmt check.
  workspace-surfaces
                  Workspace list/open/attach selectors plus select
                  widget and localization plus a fmt check.
  cli-responsive  List/open/attach/session/project/worktree command
                  render tests plus localization plus a fmt check.
EOF
}

run_with_timeout() {
    local timeout_secs="$1"
    shift

    case "${timeout_secs}" in
        "" | *[!0-9]*)
            echo "FAIL: timeout value must be a non-negative integer, got '${timeout_secs}'." >&2
            exit 2
            ;;
    esac

    if [ "${timeout_secs}" -eq 0 ]; then
        "$@"
        return
    fi

    python3 - "$timeout_secs" "$@" <<'PY'
import os
import signal
import subprocess
import sys

timeout_secs = int(sys.argv[1])
cmd = sys.argv[2:]

try:
    process = subprocess.Popen(cmd, preexec_fn=os.setsid)
except FileNotFoundError:
    print(f"FAIL: command not found: {cmd[0]}", file=sys.stderr)
    sys.exit(127)

try:
    exit_code = process.wait(timeout=timeout_secs)
except subprocess.TimeoutExpired:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
    print(
        f"FAIL: command timed out after {timeout_secs}s: {' '.join(cmd)}",
        file=sys.stderr,
    )
    sys.exit(124)

if exit_code < 0:
    sys.exit(128 + abs(exit_code))
sys.exit(exit_code)
PY
}

# Run an ordinary (non-test) command with visible invocation and
# fail-fast behavior inherited from `set -e`.
run_step() {
    run_step_with_timeout "${COMMAND_TIMEOUT_SECS}" "$@"
}

run_step_with_timeout() {
    local timeout_secs="$1"
    shift
    echo "+ timeout ${timeout_secs}s $*"
    run_with_timeout "${timeout_secs}" "$@"
}

# Run a `cargo test` invocation, stream its output, and reject a run that
# matched zero tests. Zero matched tests is not valid evidence even when
# the command exits successfully.
run_cargo_test() {
    run_cargo_test_with_timeout "${TEST_TIMEOUT_SECS}" "$@"
}

run_cargo_test_with_timeout() {
    local timeout_secs="$1"
    shift
    echo "+ timeout ${timeout_secs}s cargo test $*"
    local log
    log="$(mktemp)"
    # `pipefail` propagates a cargo failure through `tee`.
    if ! run_with_timeout "${timeout_secs}" cargo test "$@" 2>&1 | tee "${log}"; then
        rm -f "${log}"
        echo "FAIL: cargo test $*" >&2
        exit 1
    fi
    # Sum every "running N tests" header across the test binaries this
    # command executed. Tolerate the no-match case so the assignment
    # always yields a number.
    local matched
    matched="$(grep -Eo 'running [0-9]+ tests?' "${log}" \
        | awk '{ sum += $2 } END { print sum + 0 }')" || matched=0
    rm -f "${log}"
    if [ "${matched}" -eq 0 ]; then
        echo "FAIL: cargo test $* matched 0 tests; zero tests is not valid evidence." >&2
        exit 1
    fi
}

focused_doctor() {
    run_cargo_test -p workhive-cli --test cli_integration doctor
    run_cargo_test -p workhive-cli --bin workhive commands::doctor
    run_cargo_test -p workhive-cli --bin workhive policy
}

focused_help() {
    run_cargo_test -p workhive-cli --test cli_integration help
    run_cargo_test -p workhive-cli --bin workhive cli::tests
    run_cargo_test -p workhive-cli --bin workhive localize
}

focused_pane_show() {
    run_cargo_test -p workhive-cli --bin workhive cli::tests
    run_cargo_test -p workhive-cli --bin workhive commands::pane_show
    run_cargo_test -p workhive-cli --bin workhive commands::pane
    run_cargo_test -p workhive-cli --test cli_integration pane_show
}

focused_cli() {
    run_cargo_test -p workhive-cli --bin workhive cli::tests
}

focused_pane() {
    run_cargo_test -p workhive-cli --bin workhive commands::pane
}

focused_i18n() {
    run_cargo_test -p workhive-cli --bin workhive localize
}

focused_integration() {
    run_cargo_test -p workhive-cli --test cli_integration
}

focused_input_widgets() {
    run_cargo_test -p workhive-cli --bin workhive ui::validated_input
    run_cargo_test -p workhive-cli --bin workhive ui::screens::text_input
}

focused_workspace_list() {
    run_cargo_test -p workhive-cli --bin workhive commands::list
}

focused_workspace_open() {
    run_cargo_test -p workhive-cli --bin workhive commands::open
}

focused_workspace_attach() {
    run_cargo_test -p workhive-cli --bin workhive commands::close::attach
}

focused_session_list() {
    run_cargo_test -p workhive-cli --bin workhive commands::session
}

focused_project_list() {
    run_cargo_test -p workhive-cli --bin workhive commands::project
}

focused_worktree_list() {
    run_cargo_test -p workhive-cli --bin workhive commands::worktree
}

# Dispatch a single focused area. Callers validate area names first so a
# typo fails before any test runs.
run_focused_area() {
    case "$1" in
        doctor) focused_doctor ;;
        help) focused_help ;;
        pane-show) focused_pane_show ;;
        cli) focused_cli ;;
        pane) focused_pane ;;
        i18n) focused_i18n ;;
        integration) focused_integration ;;
        input-widgets) focused_input_widgets ;;
        workspace-list) focused_workspace_list ;;
        workspace-open) focused_workspace_open ;;
        workspace-attach) focused_workspace_attach ;;
        session-list) focused_session_list ;;
        project-list) focused_project_list ;;
        worktree-list) focused_worktree_list ;;
    esac
}

run_focused() {
    if [ "$#" -eq 0 ]; then
        echo "FAIL: focused requires at least one area. Supported areas: ${FOCUSED_AREAS}." >&2
        usage >&2
        exit 2
    fi
    # Validate every area before running anything so an unknown area fails
    # fast instead of part-way through a multi-area run.
    local area
    for area in "$@"; do
        case "${area}" in
            doctor | help | pane-show | cli | pane | i18n | integration | \
                input-widgets | workspace-list | workspace-open | \
                workspace-attach | session-list | project-list | \
                worktree-list) ;;
            *)
                echo "FAIL: unknown focused area '${area}'. Supported areas: ${FOCUSED_AREAS}." >&2
                usage >&2
                exit 2
                ;;
        esac
    done
    for area in "$@"; do
        run_focused_area "${area}"
    done
}

# Cheap deterministic format check runs first so a formatting regression
# fails the batch before the longer focused suites run.
batch_cli_surface() {
    run_step cargo fmt --all -- --check
    focused_doctor
    focused_help
    focused_pane_show
}

# Trusted-root entry surface: the validated-input retry state and text
# input widgets used by `project roots` flows, plus localization, guarded
# by a cheap fmt check first.
batch_trusted_root() {
    run_step cargo fmt --all -- --check
    focused_input_widgets
    focused_i18n
}

# Workspace selector surface: list/open/attach plus the shared select
# widget and localization.
batch_workspace_surfaces() {
    run_step cargo fmt --all -- --check
    focused_workspace_list
    focused_workspace_open
    focused_workspace_attach
    run_cargo_test -p workhive-cli --bin workhive ui::screens::select
    focused_i18n
}

# Broader CLI listing/selector responsiveness: every workflow list and
# selector command plus localization.
batch_cli_responsive() {
    run_step cargo fmt --all -- --check
    focused_workspace_list
    focused_workspace_open
    focused_workspace_attach
    focused_session_list
    focused_project_list
    focused_worktree_list
    focused_i18n
}

run_batch() {
    local name="${1:-}"
    case "${name}" in
        cli-surface) batch_cli_surface ;;
        trusted-root) batch_trusted_root ;;
        workspace-surfaces) batch_workspace_surfaces ;;
        cli-responsive) batch_cli_responsive ;;
        "")
            echo "FAIL: batch requires a name. Supported batches: ${BATCHES}." >&2
            usage >&2
            exit 2
            ;;
        *)
            echo "FAIL: unknown batch '${name}'. Supported batches: ${BATCHES}." >&2
            usage >&2
            exit 2
            ;;
    esac
}

run_full() {
    run_step cargo fmt --all -- --check
    run_step_with_timeout "${FULL_TIMEOUT_SECS}" cargo clippy --workspace --all-targets -- -D warnings
    run_step_with_timeout "${FULL_TIMEOUT_SECS}" cargo test --workspace --all-targets
    run_step_with_timeout "${FULL_TIMEOUT_SECS}" cargo test --workspace --doc
    run_step npm run lint:boundaries
}

run_e2e() {
    # Real-dependency tier: real tmux is required; Herdr is optional for the
    # tmux subset and is exercised when its binary is installed.
    if ! command -v tmux >/dev/null 2>&1; then
        echo "FAIL: real E2E requires tmux on PATH." >&2
        return 2
    fi

    local cli_test_args=(--ignored)
    if ! command -v herdr >/dev/null 2>&1; then
        # Keep real-tmux E2E useful on hosts without Herdr; the one CLI test
        # that needs the Herdr binary is skipped explicitly.
        cli_test_args+=(--skip real_herdr_force_rebuild_refusal_preserves_project_and_linked_workspaces)
    fi
    run_cargo_test_with_timeout "${E2E_TIMEOUT_SECS}" -p workhive-cli --test cli_integration -- "${cli_test_args[@]}"

    # `workhive-managed` is the built-in Workhive-owned Herdr session;
    # project overrides are possible, but the real Herdr smoke tier uses
    # this reserved session.
    local herdr_status=""
    if command -v herdr >/dev/null 2>&1; then
        herdr_status="$(herdr --session workhive-managed status server 2>/dev/null || true)"
    fi
    if [[ "${herdr_status}" == *"status: running"* ]]; then
        run_cargo_test_with_timeout "${E2E_TIMEOUT_SECS}" -p workhive-infra real_herdr_ -- --ignored
    else
        echo "SKIP: real Herdr E2E requires 'herdr --session workhive-managed status server' to report status: running."
    fi
}

main() {
    local mode="${1:-}"
    case "${mode}" in
        list | --list | -l | help | --help | -h)
            usage
            ;;
        focused)
            shift || true
            run_focused "$@"
            ;;
        batch)
            shift || true
            run_batch "${1:-}"
            ;;
        full)
            run_full
            ;;
        e2e)
            run_e2e
            ;;
        "")
            echo "FAIL: a mode is required." >&2
            usage >&2
            exit 2
            ;;
        *)
            echo "FAIL: unknown mode '${mode}'." >&2
            usage >&2
            exit 2
            ;;
    esac
}

main "$@"

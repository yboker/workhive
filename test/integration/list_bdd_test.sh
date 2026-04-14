#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Tests: workhive list
# Feature: Global Workspace Dashboard
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
BIN_DIR="$ROOT_DIR/bin"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

test_suite "BDD: workhive list (Global Dashboard)"

# ──────────────────────────────────────────────────
# Setup
# ──────────────────────────────────────────────────

TEST_DIR="/tmp/workhive-bdd-list-$$"
TEST_PREFIX="whbst$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
git init >/dev/null 2>&1
git config user.name "Test"
git config user.email "test@example.com"
echo "# BDD List Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1
mkdir -p .workhive
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${TEST_PREFIX}"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF
echo ".workhive/" > .gitignore
git add .gitignore
git commit -m "Add gitignore" >/dev/null 2>&1

cleanup() {
    tmux_cleanup_kill_sessions_in_scope "$TEST_DIR"
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ──────────────────────────────────────────────────
# Scenario 1: Empty dashboard
# ──────────────────────────────────────────────────

test_case "GIVEN only main worktree WHEN running list THEN shows dashboard with main"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" list 2>&1)
assert_contains "$output" "Dashboard" "should show Dashboard"
assert_contains "$output" "main" "should show main branch"

test_case "GIVEN only main worktree WHEN running list --format json THEN shows JSON"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" list --format json 2>&1)
assert_contains "$output" '"workspaces"' "should have workspaces"

test_case "GIVEN only main worktree WHEN running list --format table THEN shows main"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" list --format table 2>&1)
assert_contains "$output" "main" "should show main"

# ──────────────────────────────────────────────────
# Scenario 2: CLI argument validation
# ──────────────────────────────────────────────────

test_case "GIVEN invalid format WHEN running list THEN shows error"
output=$("$BIN_DIR/workhive" list --format yaml 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "GIVEN unknown option WHEN running list THEN shows error"
output=$("$BIN_DIR/workhive" list --verbose 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "GIVEN positional arg WHEN running list THEN shows error"
output=$("$BIN_DIR/workhive" list myarg 2>&1 || true)
assert_contains "$output" "does not accept positional"

# ──────────────────────────────────────────────────
# Scenario 3: Dashboard with active workspaces (requires tmux)
# ──────────────────────────────────────────────────

if tmux_usable; then
    # Create two workspaces
    cd "$TEST_DIR" || exit 1
    open_a_rc=0; "$BIN_DIR/workhive" open bdd-feat-a-$$ --no-attach >/dev/null 2>&1 || open_a_rc=$?
    open_b_rc=0; "$BIN_DIR/workhive" open bdd-feat-b-$$ --no-attach >/dev/null 2>&1 || open_b_rc=$?

    if [[ "$open_a_rc" -ne 0 || "$open_b_rc" -ne 0 ]]; then
        test_case "SKIP: workspace creation for list dashboard"
        skip_test "open --no-attach failed in current environment"
    else

        test_case "GIVEN 2 workspaces WHEN running list THEN shows both branches"
        output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
        assert_contains "$output" "bdd-feat-a-$$" "should show first branch"
        assert_contains "$output" "bdd-feat-b-$$" "should show second branch"

    test_case "GIVEN 2 workspaces WHEN running list THEN shows count"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "2" "should show total count"

    test_case "GIVEN 2 workspaces WHEN running list table THEN shows dashboard title"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "Dashboard" "should show Dashboard title"

    test_case "GIVEN 2 workspaces WHEN running list table THEN shows session status"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "detached" "sessions should be detached"

    test_case "GIVEN 2 workspaces WHEN running list table THEN shows legend"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "Legend" "should show legend"

    test_case "GIVEN 2 workspaces WHEN running list table THEN shows actions"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "workhive" "should show action commands"

    test_case "GIVEN 2 workspaces WHEN running list --format json THEN outputs valid JSON"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format json 2>&1)
    assert_contains "$output" '"summary"' "should have summary object"
    assert_contains "$output" '"workspaces"' "should have workspaces array"
    assert_contains "$output" '"branch": "bdd-feat-a-' "should have first branch"
    assert_contains "$output" '"branch": "bdd-feat-b-' "should have second branch"
    assert_contains "$output" '"git_status"' "should have git_status"
    assert_contains "$output" '"last_activity"' "should have last_activity"

    test_case "GIVEN 2 workspaces WHEN running list --format table THEN outputs branches"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format table 2>&1)
    assert_contains "$output" "bdd-feat-a-$$" "should have first branch"
    assert_contains "$output" "bdd-feat-b-$$" "should have second branch"

    test_case "GIVEN dirty worktree WHEN running list THEN shows modified indicator"
    wt_dir="$TEST_DIR/.workhive/worktrees/bdd-feat-a-$$"
    if [[ -d "$wt_dir" ]]; then
        echo "dirty" >> "$wt_dir/README.md" 2>/dev/null || true
        output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
        assert_contains "$output" "M" "should show modified indicator"
    else
        skip_test "worktree directory not found"
    fi

    test_case "GIVEN list alias WHEN running ls THEN includes dashboard markers"
    output_ls=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" ls 2>&1)
    output_list=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output_ls" "Dashboard" "ls should include dashboard"
    assert_contains "$output_list" "Dashboard" "list should include dashboard"
    assert_contains "$output_ls" "bdd-feat-a-$$" "ls should include first branch"
    assert_contains "$output_list" "bdd-feat-a-$$" "list should include first branch"
    fi

else
    test_case "SKIP: tmux not available"
    skip_test "tmux required for integration tests"
fi

test_report

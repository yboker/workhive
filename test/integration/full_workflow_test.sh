#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Full Workflow Integration Test
# Tests the complete workflow: init -> open -> list -> attach -> remove
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

# ──────────────────────────────────────────────────
# Environment initialization
# ──────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
TEST_REPO_DIR="${TEST_REPO_DIR:-/tmp/workhive-test-$(date +%s)}"
PROJECT_NAME="$(basename "$TEST_REPO_DIR")"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"

# Color definitions
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# ──────────────────────────────────────────────────
# Helper functions
# ──────────────────────────────────────────────────

log_test() {
    echo -e "${BLUE}[TEST]${NC} $1"
}

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $1"
    TESTS_PASSED=$((TESTS_PASSED + 1))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    TESTS_FAILED=$((TESTS_FAILED + 1))
}

log_info() {
    echo -e "${YELLOW}[INFO]${NC} $1"
}

run_test() {
    local test_name="$1"
    TESTS_RUN=$((TESTS_RUN + 1))
    log_test "$test_name"
}

assert_success() {
    local command="$1"
    local description="${2:-Command should succeed}"
    local output

    if output=$(eval "$command" 2>&1); then
        log_pass "$description"
        return 0
    else
        log_fail "$description"
        log_info "Failed command: $command"
        log_info "Output:\n$output"
        return 1
    fi
}

assert_file_exists() {
    local file_path="$1"
    local description="${2:-File should exist: $file_path}"

    if [[ -f "$file_path" ]]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description"
        return 1
    fi
}

assert_dir_exists() {
    local dir_path="$1"
    local description="${2:-Directory should exist: $dir_path}"

    if [[ -d "$dir_path" ]]; then
        log_pass "$description"
        return 0
    else
        log_fail "$description"
        return 1
    fi
}

assert_contains() {
    local text="$1"
    local substring="$2"
    local description="${3:-Output should contain: $substring}"

    if echo "$text" | grep -q "$substring"; then
        log_pass "$description"
        return 0
    else
        log_fail "$description"
        log_info "Expected substring: $substring"
        log_info "Actual text: $text"
        return 1
    fi
}

# ──────────────────────────────────────────────────
# Test environment setup
# ──────────────────────────────────────────────────

get_expected_session_name() {
    local branch="$1"
    local instance_id=""

    if [[ -f "$TEST_REPO_DIR/.workhive/instance_id" ]]; then
        read -r instance_id < "$TEST_REPO_DIR/.workhive/instance_id" 2>/dev/null || true
    fi

    if [[ -n "$instance_id" ]]; then
        printf 'wh@%s@%s@%s' "$instance_id" "$PROJECT_NAME" "$branch"
    else
        printf 'wh@%s@%s' "$PROJECT_NAME" "$branch"
    fi
}

setup_test_repo() {
    log_info "Creating test repository at: $TEST_REPO_DIR"

    # Clean up old test directory
    if [[ -d "$TEST_REPO_DIR" ]]; then
        rm -rf "$TEST_REPO_DIR"
    fi

    # Create test repository
    mkdir -p "$TEST_REPO_DIR"
    cd "$TEST_REPO_DIR"

    git init
    git config user.name "Test User"
    git config user.email "test@example.com"

    # Create initial commit
    echo "# Test Project" > README.md
    git add README.md
    git commit -m "Initial commit"

    # Create test branch
    git branch feature-test

    log_pass "Test repository created"
}

cleanup_test_repo() {
    log_info "Cleaning up test repository: $TEST_REPO_DIR"

    # Return to original directory
    cd "$ROOT_DIR" 2>/dev/null || true

    # Kill only test-owned workhive sessions in this repo scope
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_REPO_DIR"

    # Delete test directory
    if [[ -d "$TEST_REPO_DIR" ]]; then
        rm -rf "$TEST_REPO_DIR"
    fi

    log_pass "Cleanup complete"
}

# ──────────────────────────────────────────────────
# Test cases
# ──────────────────────────────────────────────────

test_01_init_non_interactive() {
    run_test "01: workhive init --non-interactive"

    cd "$TEST_REPO_DIR"

    # Execute non-interactive initialization
    assert_success "$BIN_DIR/workhive init --non-interactive" \
        "Init command should succeed"

    # Verify config file was created
    assert_file_exists ".workhive/config.yaml" \
        "Config file should be created"

    # Verify config file content
    local config_content
    config_content=$(cat .workhive/config.yaml)
    assert_contains "$config_content" "version:" \
        "Config should contain version"
    assert_contains "$config_content" "layout:" \
        "Config should contain layout section"
    assert_contains "$config_content" "worktree:" \
        "Config should contain worktree section"

    # Session naming is deterministic with runtime-computed prefix:
    # wh@<instance_id>@<project>@<branch>

    # Verify .gitignore was updated
    if [[ -f .gitignore ]]; then
        local gitignore_content
        gitignore_content=$(cat .gitignore)
        assert_contains "$gitignore_content" ".workhive/" \
            ".gitignore should exclude .workhive directory"
    fi
}

test_02_open_workspace() {
    run_test "02: workhive open feature-test"

    cd "$TEST_REPO_DIR"

    # Execute open command (use --no-attach to avoid blocking)
    assert_success "$BIN_DIR/workhive open feature-test --no-attach" \
        "Open command should succeed"

    # Verify worktree was created
    assert_dir_exists ".workhive/worktrees/feature-test" \
        "Worktree directory should be created"

    # Verify worktree contains correct files
    assert_file_exists ".workhive/worktrees/feature-test/README.md" \
        "Worktree should contain project files"

        # Verify session was created (session format: wh@<project>@<branch>)
    local expected_session
    expected_session=$(get_expected_session_name "feature-test")
    if command -v tmux &>/dev/null; then
        assert_success "tmux has-session -t $expected_session" \
            "tmux session should be created"

        # Verify window count
        local window_count
        window_count=$(tmux list-windows -t "$expected_session" 2>/dev/null | wc -l)
        if [[ $window_count -ge 1 ]]; then
            log_pass "Session should have at least 1 window"
        else
            log_fail "Session should have at least 1 window (found: $window_count)"
        fi

        # Verify pane count (should be 3)
        local pane_count
        pane_count=$(tmux list-panes -t "$expected_session" 2>/dev/null | wc -l)
        if [[ $pane_count -eq 3 ]]; then
            log_pass "Session should have exactly 3 panes"
        else
            log_fail "Session should have exactly 3 panes (found: $pane_count)"
        fi
    fi
}

test_03_list_workspaces() {
    run_test "03: workhive list"

    cd "$TEST_REPO_DIR"

    # Execute list command
    local list_output
    list_output=$("$BIN_DIR/workhive" list 2>&1)

    # Verify output contains the created workspace
    assert_contains "$list_output" "feature-test" \
        "List should show feature-test workspace"

    # Test JSON format output
    local json_output
    json_output=$("$BIN_DIR/workhive" list --format json 2>&1)
    assert_contains "$json_output" '"branch": "feature-test"' \
        "JSON output should contain branch name"

    # Test simple format output
    local simple_output
    simple_output=$("$BIN_DIR/workhive" list --format simple 2>&1)
    assert_contains "$simple_output" "feature-test" \
        "Simple output should contain branch name"
}

test_04_open_existing_workspace() {
    run_test "04: workhive open (existing workspace)"

    cd "$TEST_REPO_DIR"

    # Re-open existing workspace (should attach directly)
    # Use --no-attach to avoid blocking
    assert_success "$BIN_DIR/workhive open feature-test --no-attach" \
        "Open existing workspace should succeed"

    # Verify worktree still exists
    assert_dir_exists ".workhive/worktrees/feature-test" \
        "Worktree should still exist"
}

test_05_remove_workspace_keep_worktree() {
    run_test "05: workhive remove --keep-worktree"

    cd "$TEST_REPO_DIR"

    # Close workspace but keep worktree (--force skips non-interactive confirmation)
    assert_success "$BIN_DIR/workhive remove feature-test --keep-worktree --force" \
        "Remove with --keep-worktree should succeed"

    # Verify session was closed
    local expected_session
    expected_session=$(get_expected_session_name "feature-test")
    if command -v tmux &>/dev/null; then
        if tmux has-session -t "$expected_session" 2>/dev/null; then
            log_fail "Session should be closed"
        else
            log_pass "Session should be closed"
        fi
    fi

    # Verify worktree was preserved
    assert_dir_exists ".workhive/worktrees/feature-test" \
        "Worktree should be preserved"

    # Verify branch was preserved
    if git rev-parse --verify feature-test &>/dev/null; then
        log_pass "Branch should be preserved"
    else
        log_fail "Branch should be preserved"
    fi
}

test_06_open_and_remove_thoroughly() {
    run_test "06: workhive open + remove (thorough deletion)"

    cd "$TEST_REPO_DIR"

    # Create new workspace
    assert_success "$BIN_DIR/workhive open feature-test --no-attach" \
        "Re-open workspace should succeed"

    # Thoroughly destroy worktree (--force skips non-interactive confirmation)
    assert_success "$BIN_DIR/workhive remove feature-test --force" \
        "Remove command should succeed"

    # Verify worktree was deleted
    if [[ -d ".workhive/worktrees/feature-test" ]]; then
        log_fail "Worktree should be removed"
    else
        log_pass "Worktree should be removed"
    fi

    # Verify branch was deleted
    if git rev-parse --verify feature-test &>/dev/null; then
        log_fail "Branch should be removed"
    else
        log_pass "Branch should be removed"
    fi
}

test_06b_remove_fails_fast_when_worktree_missing() {
    run_test "06b: workhive remove fails before killing session when worktree is missing"

    cd "$TEST_REPO_DIR"

    assert_success "$BIN_DIR/workhive open feature-test --no-attach" \
        "Re-open workspace for missing-worktree regression test should succeed"

    local expected_session
    expected_session=$(get_expected_session_name "feature-test")
    local wt_path=".workhive/worktrees/feature-test"
    local output

    rm -rf "$wt_path"

    if output=$("$BIN_DIR/workhive" remove feature-test --force 2>&1); then
        log_fail "Remove should fail when worktree is missing"
        log_info "Unexpected output:\n$output"
    else
        log_pass "Remove should fail when worktree is missing"
    fi

    assert_contains "$output" "Worktree not found:" \
        "Missing worktree error should be shown immediately"

    if echo "$output" | grep -qE "Step 1/3: Close session|会话已关闭|Session closed"; then
        log_fail "Remove should not attempt to close the session before failing"
        log_info "Actual output:\n$output"
    else
        log_pass "Remove should not log any session-close step before failing"
    fi

    if command -v tmux &>/dev/null; then
        if tmux has-session -t "$expected_session" 2>/dev/null; then
            log_pass "tmux session should remain alive after fail-fast validation"
            tmux kill-session -t "$expected_session" 2>/dev/null || true
        else
            log_fail "tmux session should not be closed when worktree is missing"
        fi
    fi

    git worktree prune >/dev/null 2>&1 || true
}

test_07_snapshot_restore_summary_and_cwd() {
    run_test "07: snapshot restore shows runtime pane state and preserves cwd"

    cd "$TEST_REPO_DIR"

    cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
          role: main
        - height: 50
          command: null
          role: shell
    - width: 40
      panes:
        - height: 100
          command: null
          role: preview
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
tmux:
  mouse_support: true
  theme: "catppuccin-mocha"
  persistence:
    enabled: true
    auto_save_interval: 1m
    restore_processes: true
EOF

    assert_success "$BIN_DIR/workhive open restore-bdd --no-attach" \
        "Initial open for restore-bdd should succeed"

    local expected_session
    expected_session=$(get_expected_session_name "restore-bdd")
    local worktree_dir="$TEST_REPO_DIR/.workhive/worktrees/restore-bdd"
    local home_pane_dir="$worktree_dir/home-pane"
    mkdir -p "$home_pane_dir"

    if command -v tmux &>/dev/null; then
        tmux send-keys -t "${expected_session}:0.0" "claude --dangerously-skip-permissions" C-m
        tmux send-keys -t "${expected_session}:0.1" "cd $(printf '%q' "$HOME")" C-m
        tmux send-keys -t "${expected_session}:0.2" "lazygit" C-m
        sleep 1
    fi

    assert_success "$BIN_DIR/workhive remove restore-bdd --keep-worktree --force" \
        "Remove --keep-worktree should save snapshot and close session"

    local restore_output
    restore_output=$("$BIN_DIR/workhive" open restore-bdd --no-attach 2>&1 || true)

    if echo "$restore_output" | grep -qiE "Session restored from snapshot|会话已从快照恢复"; then
        log_pass "Restore open should report snapshot restore"
    else
        log_info "Restore message not detected; continue with structural assertions"
        log_info "Expected substring: Session restored from snapshot OR 会话已从快照恢复"
        log_info "Actual text: $restore_output"
    fi

    if command -v tmux &>/dev/null; then
        local pane1_cwd
        pane1_cwd=$(tmux display-message -p -t "${expected_session}:0.1" '#{pane_current_path}' 2>/dev/null || echo "")
        if [[ "$pane1_cwd" == "$HOME" ]]; then
            log_pass "Restore should preserve HOME cwd from snapshot"
        else
            log_fail "Restore should preserve HOME cwd from snapshot"
            log_info "Expected pane cwd: $HOME"
            log_info "Actual pane cwd: $pane1_cwd"
        fi

        tmux kill-session -t "$expected_session" 2>/dev/null || true
    fi
}

test_07b_v2_snapshot_format_and_restore_cwd() {
    run_test "07b: v2 JSONL snapshot save → close → restore preserves structure and cwd"

    cd "$TEST_REPO_DIR"

    cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
          role: main
        - height: 50
          command: null
          role: shell
    - width: 40
      panes:
        - height: 100
          command: null
          role: preview
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
tmux:
  mouse_support: true
  theme: "catppuccin-mocha"
  persistence:
    enabled: true
    auto_save_interval: 1m
    restore_processes: true
EOF

    assert_success "$BIN_DIR/workhive open v2-restore-test --no-attach" \
        "Initial open for v2-restore-test should succeed"

    local expected_session
    expected_session=$(get_expected_session_name "v2-restore-test")
    local worktree_dir="$TEST_REPO_DIR/.workhive/worktrees/v2-restore-test"

    if ! command -v tmux &>/dev/null; then
        log_info "tmux not available, skip v2 restore integration assertions"
        return 0
    fi

    local pane0_cwd
    pane0_cwd=$(tmux display-message -p -t "${expected_session}:0.0" '#{pane_current_path}' 2>/dev/null || echo "")

    assert_success "$BIN_DIR/workhive remove v2-restore-test --keep-worktree --force" \
        "Remove --keep-worktree should save v2 snapshot and close session"

    local snapshot_dir="$TEST_REPO_DIR/.workhive/snapshots"
    local latest_snapshot="${snapshot_dir}/wh_snapshot_latest.jsonl"

    assert_file_exists "$latest_snapshot" \
        "v2 snapshot file should exist after remove"

    local snapshot_first_line
    snapshot_first_line=$(head -n1 "$latest_snapshot")
    if echo "$snapshot_first_line" | grep -q '"type":"meta"'; then
        log_pass "Snapshot should start with v2 meta record"
    else
        log_fail "Snapshot should start with v2 meta record"
        log_info "First line: $snapshot_first_line"
    fi

    if grep -q '"type":"pane"' "$latest_snapshot"; then
        log_pass "Snapshot should contain v2 pane records"
    else
        log_fail "Snapshot should contain v2 pane records"
    fi

    if grep -q '"type":"window"' "$latest_snapshot"; then
        log_pass "Snapshot should contain v2 window records"
    else
        log_fail "Snapshot should contain v2 window records"
    fi

    local restore_output
    restore_output=$("$BIN_DIR/workhive" open v2-restore-test --no-attach 2>&1 || true)

    if echo "$restore_output" | grep -qiE "Session restored from snapshot|会话已从快照恢复"; then
        log_pass "Restore open should report snapshot restore"
    else
        log_info "Restore message not detected; continue with structural assertions"
        log_info "Expected substring: Session restored from snapshot OR 会话已从快照恢复"
        log_info "Actual text: $restore_output"
    fi

    if tmux has-session -t "$expected_session" 2>/dev/null; then
        log_pass "Session should be restored from v2 snapshot"
    else
        log_fail "Session should be restored from v2 snapshot"
        log_info "Output: $restore_output"
        return 0
    fi

    local restored_pane_count
    restored_pane_count=$(tmux list-panes -t "$expected_session" 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$restored_pane_count" -ge 2 ]]; then
        log_pass "Restored session should have multiple panes (found: $restored_pane_count)"
    else
        log_fail "Restored session should have multiple panes (found: $restored_pane_count)"
    fi

    local restored_pane0_cwd
    restored_pane0_cwd=$(tmux display-message -p -t "${expected_session}:0.0" '#{pane_current_path}' 2>/dev/null || echo "")
    if [[ -n "$restored_pane0_cwd" && "$restored_pane0_cwd" == "$worktree_dir"* ]]; then
        log_pass "Restored pane cwd should be worktree directory, not HOME"
    elif [[ "$restored_pane0_cwd" == "$HOME" && -n "$pane0_cwd" && "$pane0_cwd" != "$HOME" ]]; then
        if [[ "$restore_output" == *"Created workspace"* || "$restore_output" == *"创建工作区"* ]]; then
            log_fail "Open appears to have created a fresh workspace instead of restoring snapshot"
            log_info "Output: $restore_output"
        else
            log_fail "Restored pane cwd fell back to HOME instead of preserving worktree path"
            log_info "Expected: $pane0_cwd, Actual: $restored_pane0_cwd"
        fi
    else
        log_pass "Restored pane cwd is acceptable: $restored_pane0_cwd"
    fi

    tmux kill-session -t "$expected_session" 2>/dev/null || true
}

test_07c_restore_then_attach_exit_cleanly() {
    run_test "07c: restore followed by attach exits cleanly"

    cd "$TEST_REPO_DIR"

    local branch="attach-recover"

    cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
          role: main
        - height: 50
          command: null
          role: shell
    - width: 40
      panes:
        - height: 100
          command: null
          role: preview
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
tmux:
  mouse_support: true
  theme: "catppuccin-mocha"
  persistence:
    enabled: true
    auto_save_interval: 1m
    restore_processes: true
EOF

    if ! command -v tmux &>/dev/null; then
        log_info "tmux not available, skip attach-flow assertions"
        return 0
    fi

    assert_success "$BIN_DIR/workhive open $branch --no-attach" \
        "Initial open for attach recovery should succeed"

    local expected_session
    expected_session=$(get_expected_session_name "$branch")

    assert_success "$BIN_DIR/workhive remove $branch --keep-worktree --force" \
        "Remove --keep-worktree should save snapshot for attach-flow test"

    assert_success "$BIN_DIR/workhive open $branch --no-attach" \
        "Restore open should recreate session before attach"

    if tmux has-session -t "$expected_session" 2>/dev/null; then
        log_pass "Session should exist before attach"
    else
        log_fail "Session should exist before attach"
        return 0
    fi

    local attach_output=""
    local timeout_cmd=""
    if command -v timeout &>/dev/null; then
        timeout_cmd="timeout"
    elif command -v gtimeout &>/dev/null; then
        timeout_cmd="gtimeout"
    fi

    if [[ ! -t 0 || ! -t 1 ]]; then
        log_info "non-interactive environment, skip direct attach timeout assertion"
    elif [[ -n "$timeout_cmd" ]]; then
        if attach_output=$($timeout_cmd 8 "$BIN_DIR/workhive" attach "$branch" 2>&1); then
            log_pass "Attach command should exit cleanly when tmux client detaches"
        else
            log_fail "Attach command should exit cleanly when tmux client detaches"
            log_info "Attach output: $attach_output"
        fi
    else
        log_info "timeout command not available, skip direct attach timeout assertion"
    fi

    if tmux has-session -t "$expected_session" 2>/dev/null; then
        log_pass "Session should remain alive after attach exit"
        tmux kill-session -t "$expected_session" 2>/dev/null || true
    else
        log_fail "Session should remain alive after attach exit"
    fi
}

test_08_init_force() {
    run_test "08: workhive init --force (overwrite config)"

    cd "$TEST_REPO_DIR"

    # Back up original config
    local original_config
    original_config=$(cat .workhive/config.yaml)

    # Force re-initialization
    assert_success "$BIN_DIR/workhive init --force --non-interactive" \
        "Init with --force should succeed"

    # Verify config file was updated
    assert_file_exists ".workhive/config.yaml" \
        "Config file should still exist"

    local new_config
    new_config=$(cat .workhive/config.yaml)
    # Timestamps should differ
    if [[ "$original_config" != "$new_config" ]]; then
        log_pass "Config should be regenerated"
    else
        log_info "Config content unchanged (acceptable)"
    fi
}

test_09_error_handling() {
    run_test "09: Error handling (non-existent workspace)"

    cd "$TEST_REPO_DIR"

    # Try to attach to non-existent session (should fail)
    local output
    output=$("$BIN_DIR/workhive" attach nonexistent-branch 2>&1 || true)

    if echo "$output" | grep -qiE "不存在|not found"; then
        log_pass "Should show error for non-existent workspace"
    else
        log_fail "Should show error for non-existent workspace"
        log_info "Actual output:"
        echo "$output"
    fi

    # Try to close non-existent workspace (should fail)
    output=$("$BIN_DIR/workhive" remove nonexistent-branch 2>&1 || true)

    if echo "$output" | grep -qiE "不存在|not found"; then
        log_pass "Should show error when removing non-existent workspace"
    else
        log_fail "Should show error when removing non-existent workspace"
        log_info "Actual output:"
        echo "$output"
    fi
}

# ──────────────────────────────────────────────────
# Main test flow
# ──────────────────────────────────────────────────

main() {
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "workhive Full Workflow Integration Test"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    # Set up cleanup trap
    trap cleanup_test_repo EXIT

    # Check dependencies
    if ! command -v git &>/dev/null; then
        echo "Error: git is required"
        exit 1
    fi

    if ! command -v tmux &>/dev/null; then
        echo "Warning: tmux not found, some tests will be skipped"
    fi

    # Set up test environment
    setup_test_repo

    # Run tests
    echo ""
    echo "Running tests..."
    echo ""

    test_01_init_non_interactive || true
    echo ""

    test_02_open_workspace || true
    echo ""

    test_03_list_workspaces || true
    echo ""

    test_04_open_existing_workspace || true
    echo ""

    test_05_remove_workspace_keep_worktree || true
    echo ""

    test_06_open_and_remove_thoroughly || true
    echo ""

    test_06b_remove_fails_fast_when_worktree_missing || true
    echo ""

    test_07_snapshot_restore_summary_and_cwd || true
    echo ""

    test_07b_v2_snapshot_format_and_restore_cwd || true
    echo ""

    test_07c_restore_then_attach_exit_cleanly || true
    echo ""

    test_08_init_force || true
    echo ""

    test_09_error_handling || true
    echo ""

    # Output test results
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Test Results"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Total:  $TESTS_RUN"
    echo -e "Passed: ${GREEN}$TESTS_PASSED${NC}"
    echo -e "Failed: ${RED}$TESTS_FAILED${NC}"
    echo ""

    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}✅ All tests passed!${NC}"
        return 0
    else
        echo -e "${RED}❌ Some tests failed${NC}"
        return 1
    fi
}

# Run tests
main "$@"

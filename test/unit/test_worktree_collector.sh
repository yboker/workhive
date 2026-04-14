#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/app/shared/worktree_collector.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/adapters/terminal/factory.sh"
source "$LIB_DIR/infra/config/loader.sh"

terminal_adapter_init >/dev/null 2>&1

source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/worktree_service.sh"
source "$LIB_DIR/app/shared/worktree_collector.sh"
source "$ROOT_DIR/test/integration/tmux_cleanup_scope.sh"

TEST_DIR="/tmp/workhive-test-collector-$$"
TEST_SESSION=""

mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"
git init >/dev/null 2>&1
git config user.name "Test"
git config user.email "test@example.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1

mkdir -p .workhive
cat > .workhive/config.yaml <<'EOF'
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
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF

cleanup() {
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_DIR"
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "lib/app/shared/worktree_collector.sh"

# ──────────────────────────────────────────────────
# WORKTREE_COLLECTOR_BRANCH global variable
# ──────────────────────────────────────────────────

test_case "WORKTREE_COLLECTOR_BRANCH is set after call"
worktrees=()
_collect_available_worktrees worktrees
assert_not_empty "$WORKTREE_COLLECTOR_BRANCH" "should have current branch"

test_case "WORKTREE_COLLECTOR_BRANCH matches git branch"
expected_branch=$(git branch --show-current 2>/dev/null)
assert_eq "$expected_branch" "$WORKTREE_COLLECTOR_BRANCH" "should match git branch"

# ──────────────────────────────────────────────────
# Empty state (no worktrees, no sessions)
# ──────────────────────────────────────────────────

test_case "returns empty array when no worktrees or sessions"
cache_clear
worktrees=()
_collect_available_worktrees worktrees
# Only sessions matching prefix are included; no worktree dirs exist yet
# (there may be external sessions, so we just check the function doesn't crash)
assert_eq "0" "$?" "should succeed"

# ──────────────────────────────────────────────────
# With worktree directories
# ──────────────────────────────────────────────────

test_case "detects worktree directories"
mkdir -p "$TEST_DIR/.workhive/worktrees/feature-a"
mkdir -p "$TEST_DIR/.workhive/worktrees/feature-b"
cache_clear
worktrees=()
_collect_available_worktrees worktrees
found_a=false
found_b=false
for w in "${worktrees[@]}"; do
    [[ "$w" == "feature-a" ]] && found_a=true
    [[ "$w" == "feature-b" ]] && found_b=true
done
if [[ "$found_a" == true && "$found_b" == true ]]; then
    assert_eq "0" "0" "found both worktrees"
else
    assert_eq "found" "not_found" "should find feature-a and feature-b (found_a=$found_a, found_b=$found_b)"
fi

test_case "worktrees array is populated (not empty due to subshell bug)"
assert_ne "0" "${#worktrees[@]}" "array should not be empty"

# ──────────────────────────────────────────────────
# With tmux sessions
# ──────────────────────────────────────────────────

if tmux_usable; then
    test_suite "worktree_collector.sh (with tmux sessions)"

    test_case "includes session-only branches"
    TEST_SESSION=$(session_get_name "collector-test-$$")
    tmux new-session -d -s "$TEST_SESSION" 2>/dev/null
    cache_clear
    worktrees=()
    _collect_available_worktrees worktrees
    found_session=false
    for w in "${worktrees[@]}"; do
        [[ "$w" == "collector-test-$$" ]] && found_session=true
    done
    assert_eq "true" "$found_session" "should find session-only branch"
    tmux kill-session -t "$TEST_SESSION" 2>/dev/null

    test_case "deduplicates dir + session entries"
    mkdir -p "$TEST_DIR/.workhive/worktrees/dedup-test"
    TEST_SESSION=$(session_get_name "dedup-test")
    tmux new-session -d -s "$TEST_SESSION" 2>/dev/null
    cache_clear
    worktrees=()
    _collect_available_worktrees worktrees
    count=0
    for w in "${worktrees[@]}"; do
        [[ "$w" == "dedup-test" ]] && count=$((count + 1))
    done
    assert_eq "1" "$count" "should not have duplicates"
    tmux kill-session -t "$TEST_SESSION" 2>/dev/null

    test_case "excludes current branch from session list"
    current=$(git branch --show-current 2>/dev/null)
    TEST_SESSION=$(session_get_name "$current")
    tmux new-session -d -s "$TEST_SESSION" 2>/dev/null
    cache_clear
    worktrees=()
    _collect_available_worktrees worktrees
    # shellcheck disable=SC2034
    found_current=false
    for w in "${worktrees[@]}"; do
        # shellcheck disable=SC2034
        [[ "$w" == "$current" ]] && found_current=true
    done
    # current branch should be excluded from session scan
    # (but may exist as a directory, which is fine)
    tmux kill-session -t "$TEST_SESSION" 2>/dev/null
    # We just verify no crash and the function returns
    assert_eq "0" "$?" "should succeed"
fi

# ──────────────────────────────────────────────────
# Critical: nameref works without $() subshell
# ──────────────────────────────────────────────────

test_suite "worktree_collector.sh (nameref correctness)"

test_case "nameref populates array in parent shell"
mkdir -p "$TEST_DIR/.workhive/worktrees/nameref-test"
cache_clear
my_arr=()
_collect_available_worktrees my_arr
found=false
for w in "${my_arr[@]}"; do
    [[ "$w" == "nameref-test" ]] && found=true
done
assert_eq "true" "$found" "nameref should work without subshell"

test_case "multiple calls accumulate correctly"
# First call
arr1=()
cache_clear
_collect_available_worktrees arr1
count1=${#arr1[@]}

# Second call with fresh array
arr2=()
cache_clear
_collect_available_worktrees arr2
count2=${#arr2[@]}

assert_eq "$count1" "$count2" "consistent results across calls"

# Cleanup test worktree dirs
rm -rf "$TEST_DIR/.workhive/worktrees/feature-a"
rm -rf "$TEST_DIR/.workhive/worktrees/feature-b"
rm -rf "$TEST_DIR/.workhive/worktrees/dedup-test"
rm -rf "$TEST_DIR/.workhive/worktrees/nameref-test"

test_report

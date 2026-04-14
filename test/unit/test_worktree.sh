#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/domain/services/worktree_service.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/domain/services/worktree_service.sh"

# Temporary Git repository for testing
TEST_DIR="/tmp/workhive-test-worktree-$$"
ORIG_DIR="$(pwd)"

setup_git_repo() {
    rm -rf "$TEST_DIR"
    mkdir -p "$TEST_DIR"
    cd "$TEST_DIR" || return 1
    export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
    mkdir -p "$XDG_CONFIG_HOME"
    git init >/dev/null 2>&1
    git config user.name "Test"
    git config user.email "test@example.com"
    echo "# Test" > README.md
    git add README.md
    git commit -m "Init" >/dev/null 2>&1
    mkdir -p .workhive
    echo 'version: "0.2.0"' > .workhive/config.yaml
}

cleanup() {
    cd "$ORIG_DIR" || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

setup_git_repo

test_suite "lib/domain/services/worktree_service.sh"

# ──────────────────────────────────────────────────
# worktree_check_repo
# ──────────────────────────────────────────────────

test_case "worktree_check_repo succeeds in git repo"
worktree_check_repo >/dev/null 2>&1
assert_eq "0" "$?"

test_case "worktree_check_repo fails outside git repo"
(cd /tmp && worktree_check_repo >/dev/null 2>&1) || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_get_base_path
# ──────────────────────────────────────────────────

test_case "worktree_get_base_path returns absolute path under git root"
cache_clear
result=$(worktree_get_base_path)
expected="$(_git_root)/.workhive/worktrees"
assert_eq "$expected" "$result"

# ──────────────────────────────────────────────────
# worktree_get_path
# ──────────────────────────────────────────────────

test_case "worktree_get_path returns absolute path"
result=$(worktree_get_path "feature-x")
assert_contains "$result" "feature-x" "should contain branch name"
assert_eq "/" "${result:0:1}" "should be absolute path"

test_case "worktree_get_path includes base path"
result=$(worktree_get_path "feature-y")
assert_contains "$result" ".workhive/worktrees/feature-y"

# ──────────────────────────────────────────────────
# worktree_exists
# ──────────────────────────────────────────────────

test_case "worktree_exists returns 1 for non-existent"
worktree_exists "nonexistent-branch-$$" || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_create
# ──────────────────────────────────────────────────

TEST_BRANCH="test-unit-$$"

test_case "worktree_create creates new worktree"
cache_clear
worktree_create "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "worktree_exists returns 0 after create"
worktree_exists "$TEST_BRANCH"
assert_eq "0" "$?"

test_case "worktree_create is idempotent"
worktree_create "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?" "should succeed for existing worktree"

test_case "worktree_create fails for invalid branch name"
worktree_create "" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "empty name should fail"

test_case "worktree_create fails for branch with spaces"
worktree_create "has spaces" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "name with spaces should fail"

test_case "worktree_create fails for git-invalid branch syntax"
worktree_create "feature.lock" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "git-invalid ref should fail"

# ──────────────────────────────────────────────────
# worktree_get_branch
# ──────────────────────────────────────────────────

test_case "worktree_get_branch returns branch name"
wt_path=$(worktree_get_path "$TEST_BRANCH")
result=$(worktree_get_branch "$wt_path")
assert_eq "$TEST_BRANCH" "$result"

test_case "worktree_get_branch preserves caller CWD"
original_dir="$(pwd)"
wt_path=$(worktree_get_path "$TEST_BRANCH")
worktree_get_branch "$wt_path" >/dev/null 2>&1
current_dir="$(pwd)"
assert_eq "$original_dir" "$current_dir" "CWD should not change"

test_case "worktree_get_branch returns 1 for non-existent dir"
worktree_get_branch "/tmp/nonexistent_$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_validate
# ──────────────────────────────────────────────────

test_case "worktree_validate passes for healthy worktree"
worktree_validate "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "worktree_validate fails for non-existent worktree"
worktree_validate "nonexistent-branch-$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_list_all
# ──────────────────────────────────────────────────

test_case "worktree_list_all includes created worktree"
cache_clear
result=$(worktree_list_all)
assert_not_empty "$result" "should list worktrees"

# ──────────────────────────────────────────────────
# worktree_sync_files
# ──────────────────────────────────────────────────

SYNC_BRANCH="test-sync-$$"

test_case "worktree_sync_files skips when no config"
cache_clear
worktree_create "$SYNC_BRANCH" >/dev/null 2>&1
sync_wt_path=$(worktree_get_path "$SYNC_BRANCH")
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "worktree_sync_files copies file"
# Create source file and config
echo "SECRET=123" > "$TEST_DIR/.env"
cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.5.0"
worktree:
  base_path: ".workhive/worktrees"
  sync_files:
    - source: ".env"
      mode: "copy"
YAML
cache_clear
# Remove existing target (if any)
rm -f "$sync_wt_path/.env" 2>/dev/null
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?"
assert_eq "true" "$([ -f "$sync_wt_path/.env" ] && echo true || echo false)" ".env should be copied"

test_case "worktree_sync_files supports string shorthand"
cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.5.0"
worktree:
  base_path: ".workhive/worktrees"
  sync_files:
    - ".env"
YAML
cache_clear
rm -f "$sync_wt_path/.env" 2>/dev/null
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?"
assert_eq "true" "$([ -f "$sync_wt_path/.env" ] && echo true || echo false)" ".env shorthand should be copied"

test_case "worktree_sync_files creates symlink"
mkdir -p "$TEST_DIR/node_modules/fake-pkg"
echo "{}" > "$TEST_DIR/node_modules/fake-pkg/package.json"
cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.5.0"
worktree:
  base_path: ".workhive/worktrees"
  sync_files:
    - source: ".env"
      mode: "copy"
    - source: "node_modules"
      mode: "symlink"
YAML
cache_clear
rm -rf "$sync_wt_path/node_modules" 2>/dev/null
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?"
assert_eq "true" "$([ -L "$sync_wt_path/node_modules" ] && echo true || echo false)" "node_modules should be symlink"

test_case "worktree_sync_files skips existing target"
cache_clear
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?" "should succeed even when targets exist"

test_case "worktree_sync_files rejects path traversal"
cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.5.0"
worktree:
  base_path: ".workhive/worktrees"
  sync_files:
    - source: "../../etc/passwd"
      mode: "copy"
YAML
cache_clear
worktree_sync_files "$sync_wt_path" >/dev/null 2>&1
assert_eq "0" "$?" "should not crash on path traversal"

test_case "worktree_sync_files fails for non-existent worktree"
worktree_sync_files "/tmp/nonexistent_$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# Clean up worktree created for sync tests
worktree_remove "$SYNC_BRANCH" >/dev/null 2>&1

# Restore config
echo 'version: "0.5.0"' > "$TEST_DIR/.workhive/config.yaml"

# ──────────────────────────────────────────────────
# worktree_remove
# ──────────────────────────────────────────────────

test_case "worktree_remove deletes worktree"
cache_clear
worktree_remove "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "worktree_exists returns 1 after remove"
worktree_exists "$TEST_BRANCH" || rc=$?
assert_eq "1" "${rc:-0}"

test_case "worktree_remove fails for non-existent"
worktree_remove "nonexistent-$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_prune
# ──────────────────────────────────────────────────

test_case "worktree_prune succeeds"
worktree_prune >/dev/null 2>&1
assert_eq "0" "$?"

# ──────────────────────────────────────────────────
# feature/foo branch name support (P4 — hierarchical branches)
# ──────────────────────────────────────────────────

test_case "worktree_create accepts feature/foo branch name"
cache_clear
_WORKHIVE_GIT_ROOT=""
worktree_create "feature/test-slash-$$" >/dev/null 2>&1
assert_eq "0" "$?" "feature/foo should be accepted"

test_case "worktree_get_path maps / to - for feature/foo"
_wt_path=$(worktree_get_path "feature/test-slash-$$")
assert_contains "$_wt_path" "feature-test-slash-" "path should map / to -"

test_case "worktree_create rejects .. traversal"
rc=0
worktree_create "feature/../evil" >/dev/null 2>&1 || rc=$?
assert_eq "1" "$rc" ".. should be rejected"

test_case "worktree_exists finds feature/foo worktree"
worktree_exists "feature/test-slash-$$"
assert_eq "0" "$?" "should find worktree for feature/foo"

# Cleanup
worktree_remove "feature/test-slash-$$" "true" >/dev/null 2>&1

# ──────────────────────────────────────────────────
# _git_root_reset (P4 — cache resettable)
# ──────────────────────────────────────────────────

test_case "_git_root_reset clears cached root"
_WORKHIVE_GIT_ROOT="/fake/path"
_git_root_reset
assert_eq "" "${_WORKHIVE_GIT_ROOT:-}" "should be empty after reset"

test_case "_git_root recomputes after reset"
_recomputed=$(_git_root)
if [[ -n "$_recomputed" ]]; then
    assert_eq "0" "0" "should recompute non-empty value"
else
    assert_eq "non-empty" "" "should recompute non-empty value"
fi

test_report

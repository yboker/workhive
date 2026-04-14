#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/app/shared/worktree_safety.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/app/shared/worktree_safety.sh"

test_suite "Worktree Safety Tests"

TEST_DIR="${TMPDIR:-/tmp}/workhive-test-wt-safety-$$"
rm -rf "$TEST_DIR"

cleanup() {
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ──────────────────────────────────────────────────
# _worktree_is_dirty
# ──────────────────────────────────────────────────

test_case "_worktree_is_dirty returns 1 (clean) for non-existent path"
_worktree_is_dirty "/nonexistent/path" 2>/dev/null
assert_eq 1 $? "non-existent path should report clean"

test_case "_worktree_is_dirty returns 1 (clean) for clean worktree"
_wt_clean="$TEST_DIR/clean-wt"
mkdir -p "$_wt_clean"
(
    cd "$_wt_clean" || exit 1
    git init -q
    git commit --allow-empty -m "init" -q
)
_worktree_is_dirty "$_wt_clean" 2>/dev/null
assert_eq 1 $? "clean worktree should return 1"

test_case "_worktree_is_dirty returns 0 (dirty) for modified file"
_wt_dirty="$TEST_DIR/dirty-wt"
mkdir -p "$_wt_dirty"
(
    cd "$_wt_dirty" || exit 1
    git init -q
    echo "initial" > file.txt
    git add file.txt
    git commit -m "init" -q
    echo "modified" > file.txt
)
_worktree_is_dirty "$_wt_dirty" 2>/dev/null
assert_eq 0 $? "modified file should report dirty"

test_case "_worktree_is_dirty returns 0 (dirty) for staged changes"
_wt_staged="$TEST_DIR/staged-wt"
mkdir -p "$_wt_staged"
(
    cd "$_wt_staged" || exit 1
    git init -q
    echo "initial" > file.txt
    git add file.txt
    git commit -m "init" -q
    echo "staged" > file.txt
    git add file.txt
)
_worktree_is_dirty "$_wt_staged" 2>/dev/null
assert_eq 0 $? "staged changes should report dirty"

test_case "_worktree_is_dirty returns 0 (dirty) for untracked files"
_wt_untracked="$TEST_DIR/untracked-wt"
mkdir -p "$_wt_untracked"
(
    cd "$_wt_untracked" || exit 1
    git init -q
    git commit --allow-empty -m "init" -q
    echo "new file" > untracked.txt
)
_worktree_is_dirty "$_wt_untracked" 2>/dev/null
assert_eq 0 $? "untracked files should report dirty"

# ──────────────────────────────────────────────────
# _worktree_unmerged_count
# ──────────────────────────────────────────────────

_uc_repo="$TEST_DIR/uc-repo"
mkdir -p "$_uc_repo"
(
    cd "$_uc_repo" || exit 1
    git init -q
    echo "base" > file.txt
    git add file.txt
    git commit -m "initial commit" -q
    git checkout -b feature-branch -q
    echo "feature1" >> file.txt && git add file.txt && git commit -m "feature commit 1" -q
    echo "feature2" >> file.txt && git add file.txt && git commit -m "feature commit 2" -q
    echo "feature3" >> file.txt && git add file.txt && git commit -m "feature commit 3" -q
    git checkout master -q 2>/dev/null || git checkout main -q 2>/dev/null
)
_uc_main=$(cd "$_uc_repo" && git branch | grep -E '^\*' | sed 's/^\* //')

test_case "_worktree_unmerged_count returns 3 for branch with 3 unmerged commits"
count=$(_worktree_unmerged_count "feature-branch" "$_uc_repo" "$_uc_main")
assert_eq "3" "$count" "should be 3 unmerged"

test_case "_worktree_unmerged_count returns 0 for main branch"
count=$(_worktree_unmerged_count "$_uc_main" "$_uc_repo" "$_uc_main")
assert_eq "0" "$count" "main vs main should be 0"

test_case "_worktree_unmerged_count returns 0 when main_branch is empty"
count=$(_worktree_unmerged_count "feature-branch" "$_uc_repo" "")
assert_eq "0" "$count" "empty main_branch should return 0"

# ──────────────────────────────────────────────────
# _worktree_safety_scan
# ──────────────────────────────────────────────────

test_case "_worktree_safety_scan detects dirty branches"
_scan_repo="$TEST_DIR/scan-repo"
mkdir -p "$_scan_repo"
(
    cd "$_scan_repo" || exit 1
    git init -q
    echo "base" > file.txt
    git add file.txt
    git commit -m "init" -q
)
_scan_wt_base="$TEST_DIR/scan-wts"
mkdir -p "$_scan_wt_base/dirty-branch"
(
    cd "$_scan_wt_base/dirty-branch" || exit 1
    git init -q
    echo "x" > f.txt
    git add f.txt
    git commit -m "c" -q
    echo "modified" > f.txt
)
_worktree_safety_scan "$_scan_repo" "$_scan_wt_base" "dirty-branch"
assert_eq "true" "$_WT_SAFETY_HAS_WARNINGS" "should have warnings"
assert_eq "dirty-branch" "${_WT_SAFETY_DIRTY_BRANCHES[0]:-}" "should find dirty branch"

test_case "_worktree_safety_scan reports clean for clean branches"
_scan_wt_clean="$TEST_DIR/scan-wts-clean"
mkdir -p "$_scan_wt_clean/clean-branch"
(
    cd "$_scan_wt_clean/clean-branch" || exit 1
    git init -q
    git commit --allow-empty -m "c" -q
)
_worktree_safety_scan "$_scan_repo" "$_scan_wt_clean" "clean-branch"
assert_eq "false" "$_WT_SAFETY_HAS_WARNINGS" "should have no warnings"
assert_eq "0" "${#_WT_SAFETY_DIRTY_BRANCHES[@]}" "should have 0 dirty branches"

# ──────────────────────────────────────────────────
# _worktree_safety_get_marks
# ──────────────────────────────────────────────────

test_case "_worktree_safety_get_marks returns empty for clean branch"
_WT_SAFETY_DIRTY_BRANCHES=()
_WT_SAFETY_UNMERGED_BRANCHES=()
_WT_SAFETY_UNMERGED_COUNTS=()
marks=$(_worktree_safety_get_marks "clean")
assert_empty "$marks" "clean branch should have no marks"

test_case "_worktree_safety_get_marks returns mark for dirty branch"
_WT_SAFETY_DIRTY_BRANCHES=("my-dirty")
_WT_SAFETY_DIRTY_PATHS=("/tmp/x")
_WT_SAFETY_UNMERGED_BRANCHES=()
_WT_SAFETY_UNMERGED_COUNTS=()
marks=$(_worktree_safety_get_marks "my-dirty")
assert_not_empty "$marks" "dirty branch should have marks"

test_case "_worktree_safety_get_marks returns mark for unmerged branch"
_WT_SAFETY_DIRTY_BRANCHES=()
_WT_SAFETY_DIRTY_PATHS=()
_WT_SAFETY_UNMERGED_BRANCHES=("my-unmerged")
_WT_SAFETY_UNMERGED_COUNTS=("5")
marks=$(_worktree_safety_get_marks "my-unmerged")
assert_not_empty "$marks" "unmerged branch should have marks"
assert_contains "$marks" "5" "mark should contain unmerged count"

# ──────────────────────────────────────────────────
# _worktree_safety_print_warning_box
# ──────────────────────────────────────────────────

test_case "_worktree_safety_print_warning_box is no-op when no warnings"
_WT_SAFETY_HAS_WARNINGS=false
output=$(_worktree_safety_print_warning_box 2>/dev/null)
assert_empty "$output" "no warnings should produce no output"

test_case "_worktree_safety_print_warning_box prints box when warnings exist"
_WT_SAFETY_HAS_WARNINGS=true
_WT_SAFETY_DIRTY_BRANCHES=("feat-x")
_WT_SAFETY_DIRTY_PATHS=("$_scan_wt_base/dirty-branch")
_WT_SAFETY_UNMERGED_BRANCHES=()
_WT_SAFETY_UNMERGED_COUNTS=()
_WT_SAFETY_GIT_ROOT="$_scan_repo"
_WT_SAFETY_MAIN_BRANCH="main"
output=$(_worktree_safety_print_warning_box 2>/dev/null)
assert_not_empty "$output" "should produce warning box output"

test_report

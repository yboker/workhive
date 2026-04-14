#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: .gitignore management
# Covers _ensure_gitignore_workhive and _ensure_workhive_dir
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/test_framework.sh"

# shellcheck disable=SC2034
BIN_DIR="$(cd "$SCRIPT_DIR/../../bin" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/utils/i18n.sh"

TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

_git_root_reset() {
    unset _WORKHIVE_GIT_ROOT
}

_setup_git_repo() {
    local dir="$1"
    mkdir -p "$dir"
    (
        cd "$dir" || exit 1
        git init --initial-branch=main . >/dev/null 2>&1
        git commit --allow-empty -m "Initial commit" >/dev/null 2>&1
    )
}

# ══════════════════════════════════════════════════
test_suite "_ensure_gitignore_workhive Tests"
# ══════════════════════════════════════════════════

_repo=""

test_case "Creates .gitignore with .workhive/ when no .gitignore and no template"
_repo="$TEST_DIR/test_no_gitignore_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    rm -f .gitignore
    _OLD_ROOT_DIR="${ROOT_DIR:-}"
    ROOT_DIR="/nonexistent_for_test"
    _ensure_gitignore_workhive
    ROOT_DIR="$_OLD_ROOT_DIR"
    [[ -f ".gitignore" ]] && grep -q ".workhive/" ".gitignore"
)
assert_eq "0" "$?"

test_case "Creates .gitignore from template when template exists"
_repo="$TEST_DIR/test_template_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    rm -f .gitignore
    _ensure_gitignore_workhive
    [[ -f ".gitignore" ]] && grep -q ".workhive/" ".gitignore"
)
assert_eq "0" "$?"

test_case "Adds .workhive/ to existing .gitignore without it"
_repo="$TEST_DIR/test_add_entry_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    echo "node_modules/" > .gitignore
    _ensure_gitignore_workhive
    grep -q ".workhive/" ".gitignore" && grep -q "node_modules/" ".gitignore"
)
assert_eq "0" "$?"

test_case "Does not duplicate .workhive/ if already present"
_repo="$TEST_DIR/test_no_dup_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    printf 'node_modules/\n.workhive/\n' > .gitignore
    _ensure_gitignore_workhive
    _count=$(grep -c "\.workhive" ".gitignore")
    [[ "$_count" -eq 1 ]]
)
assert_eq "0" "$?"

test_case "Handles .workhive (without trailing slash) as already present"
_repo="$TEST_DIR/test_no_slash_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    printf '.workhive\n' > .gitignore
    _ensure_gitignore_workhive
    _count=$(grep -c "\.workhive" ".gitignore")
    [[ "$_count" -eq 1 ]]
)
assert_eq "0" "$?"

test_case "Handles /.workhive/ (with leading slash) as already present"
_repo="$TEST_DIR/test_leading_slash_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    printf '/.workhive/\n' > .gitignore
    _ensure_gitignore_workhive
    _count=$(grep -c "\.workhive" ".gitignore")
    [[ "$_count" -eq 1 ]]
)
assert_eq "0" "$?"

test_case "Appends newline before adding to file without trailing newline"
_repo="$TEST_DIR/test_no_newline_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    printf 'node_modules/' > .gitignore
    _ensure_gitignore_workhive
    grep -q ".workhive/" ".gitignore" && grep -q "node_modules/" ".gitignore"
)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
test_suite "_ensure_workhive_dir always ensures .gitignore"
# ══════════════════════════════════════════════════

test_case ".workhive dir does not exist -> creates dir + adds to .gitignore"
_repo="$TEST_DIR/test_wh_dir_new_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    echo "node_modules/" > .gitignore
    _ensure_workhive_dir
    [[ -d ".workhive" ]] && grep -q ".workhive/" ".gitignore"
)
assert_eq "0" "$?"

test_case ".workhive dir already exists -> still adds to .gitignore if missing"
_repo="$TEST_DIR/test_wh_dir_exists_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    mkdir -p .workhive
    echo "node_modules/" > .gitignore
    _ensure_workhive_dir
    grep -q ".workhive/" ".gitignore"
)
assert_eq "0" "$?"

test_case ".workhive dir and .gitignore both exist with entry -> idempotent"
_repo="$TEST_DIR/test_wh_idempotent_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    mkdir -p .workhive
    printf '.workhive/\n' > .gitignore
    _ensure_workhive_dir
    _count=$(grep -c "\.workhive" ".gitignore")
    [[ "$_count" -eq 1 ]]
)
assert_eq "0" "$?"

test_case "Creates instance_id when .workhive dir is new"
_repo="$TEST_DIR/test_wh_instance_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    _ensure_workhive_dir
    [[ -f ".workhive/instance_id" ]]
)
assert_eq "0" "$?"

test_case "Preserves existing instance_id"
_repo="$TEST_DIR/test_wh_preserve_id_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    mkdir -p .workhive
    echo -n "test1234" > .workhive/instance_id
    _ensure_workhive_dir
    [[ "$(cat .workhive/instance_id)" == "test1234" ]]
)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
test_suite "Simulated init+open workflow"
# ══════════════════════════════════════════════════

test_case "init creates .workhive and adds to .gitignore, open preserves"
_repo="$TEST_DIR/test_workflow_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    echo "*.log" > .gitignore
    mkdir -p .workhive
    _ensure_gitignore_workhive
    grep -q ".workhive/" ".gitignore" || exit 1
    _git_root_reset
    _ensure_workhive_dir
    _count=$(grep -c "\.workhive" ".gitignore")
    [[ "$_count" -eq 1 ]]
)
assert_eq "0" "$?"

test_case "init skips .gitignore, open compensates"
_repo="$TEST_DIR/test_compensate_$$"
_setup_git_repo "$_repo"
(
    cd "$_repo" || exit 1
    _git_root_reset
    echo "*.log" > .gitignore
    mkdir -p .workhive
    _git_root_reset
    _ensure_workhive_dir
    grep -q ".workhive/" ".gitignore"
)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
test_report

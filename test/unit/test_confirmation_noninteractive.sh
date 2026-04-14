#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"

TEST_DIR="/tmp/workhive-test-confirmation-noninteractive-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"
git init -q . 2>/dev/null || true
git commit --allow-empty -m "init" -q 2>/dev/null || true
mkdir -p .workhive/logs .workhive/state
printf 'version: "0.2.0"\n' > .workhive/config.yaml

cleanup() {
    cd / || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "remove_command.sh non-interactive"
unset _REMOVE_COMMAND_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/app/commands/remove_command.sh" 2>/dev/null || true

test_case "remove_command_execute returns failure in non-interactive mode without force"
remove_marker="$TEST_DIR/remove_noninteractive_called.txt"
branch_marker="$TEST_DIR/remove_noninteractive_branch.txt"
session_marker="$TEST_DIR/remove_noninteractive_session.txt"
rm -f "$remove_marker" "$branch_marker" "$session_marker"
session_get_name() { echo "wh-test03"; }
session_exists() { return 1; }
worktree_exists() { return 0; }
worktree_get_path() { echo "$TEST_DIR/test03"; }
config_get() { echo "true"; }
_git_root() { echo "$TEST_DIR"; }
_worktree_safety_scan() {
    _WT_SAFETY_HAS_WARNINGS=false
}
_worktree_safety_print_warning_box() { :; }
worktree_remove() {
    : > "$remove_marker"
    return 0
}
session_kill() {
    : > "$session_marker"
    return 0
}
git() {
    if [[ "$1" == "branch" && "$2" == "-D" ]]; then
        : > "$branch_marker"
    fi
    return 0
}
mkdir -p "$TEST_DIR/test03/.git"
output=$(printf '' | remove_command_execute test03 2>&1)
rc=$?
assert_failure "$rc"
assert_true "[[ ! -e \"$remove_marker\" ]]" "worktree_remove should not run in non-interactive mode"
assert_true "[[ ! -e \"$branch_marker\" ]]" "branch delete should not run in non-interactive mode"
assert_true "[[ ! -e \"$session_marker\" ]]" "session kill should not run in non-interactive mode"
assert_not_contains "$output" "$(_i18n MSG_REMOVE_WT)"
assert_contains "$output" "$(_i18n MSG_CANCELLED)"
unset -f session_get_name session_exists worktree_exists worktree_get_path config_get _git_root _worktree_safety_scan _worktree_safety_print_warning_box worktree_remove session_kill git

test_suite "rename_command.sh non-interactive"
unset _RENAME_COMMAND_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/app/commands/rename_command.sh" 2>/dev/null || true

test_case "rename_command_execute returns failure in non-interactive mode without force"
rename_marker="$TEST_DIR/rename_noninteractive_called.txt"
session_marker="$TEST_DIR/rename_noninteractive_session.txt"
rm -f "$rename_marker" "$session_marker"
_branch_needs_sanitize() { return 1; }
_git_main_branch() { echo "main"; }
worktree_check_repo() { return 0; }
worktree_exists() {
    [[ "$1" == "old-name" ]] && return 0
    return 1
}
session_get_name() { echo "wh-$1"; }
session_exists() { return 1; }
worktree_get_path() { echo "$TEST_DIR/$1"; }
terminal_adapter_rename_session() {
    : > "$session_marker"
    return 0
}
cache_invalidate() { :; }
_git_root() { echo "$TEST_DIR"; }
git() {
    if [[ "$1" == "rev-parse" && "$2" == "--verify" ]]; then
        [[ "$3" == "old-name" ]] && return 0
        return 1
    fi
    if [[ "$1" == "branch" && "$2" == "-m" ]]; then
        : > "$rename_marker"
    fi
    return 0
}
mkdir -p "$TEST_DIR/old-name"
output=$(printf '' | rename_command_execute old-name new-name 2>&1)
rc=$?
assert_failure "$rc"
assert_true "[[ ! -e \"$rename_marker\" ]]" "git rename should not run in non-interactive mode"
assert_true "[[ ! -e \"$session_marker\" ]]" "session rename should not run in non-interactive mode"
assert_contains "$output" "$(_i18n MSG_CANCELLED)"
unset -f _branch_needs_sanitize _git_main_branch worktree_check_repo worktree_exists session_get_name session_exists worktree_get_path terminal_adapter_rename_session cache_invalidate _git_root git

test_report

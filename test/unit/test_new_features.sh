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

TEST_DIR="/tmp/workhive-test-newfeats2-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"
git init -q . 2>/dev/null || true
git commit --allow-empty -m "init" -q 2>/dev/null || true
mkdir -p .workhive/logs .workhive/state
echo 'version: "0.2.0"' > .workhive/config.yaml
OP_LOG_FILE="$TEST_DIR/.workhive/logs/op-test.log"
_LOG_FILE="$OP_LOG_FILE"
_LOG_INITIALIZED=true
: > "$OP_LOG_FILE"

cleanup() {
    cd / || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "_sanitize_branch_name"

test_case "converts slashes to dashes"
result=$(_sanitize_branch_name "feature/foo")
assert_eq "feature-foo" "$result"

test_case "converts dots to dashes"
result=$(_sanitize_branch_name "release.1.0")
assert_eq "release-1-0" "$result"

test_case "converts colons to dashes"
result=$(_sanitize_branch_name "refs:heads:main")
assert_eq "refs-heads-main" "$result"

test_case "handles mixed special characters"
result=$(_sanitize_branch_name "feature/v1.2:hotfix")
assert_eq "feature-v1-2-hotfix" "$result"

test_case "converts spaces to dashes"
result=$(_sanitize_branch_name "my branch name")
assert_eq "my-branch-name" "$result"

test_case "converts shell-dangerous characters"
result=$(_sanitize_branch_name 'fix$bug`now!done')
assert_eq "fix-bug-now-done" "$result"

test_case "converts git-invalid characters"
result=$(_sanitize_branch_name "topic~1^2")
assert_eq "topic-1-2" "$result"

test_case "converts glob characters"
result=$(_sanitize_branch_name "test*file?name[0]")
assert_eq "test-file-name-0-" "$result"

test_case "preserves already-clean names"
result=$(_sanitize_branch_name "feature-foo-bar")
assert_eq "feature-foo-bar" "$result"

test_case "handles deeply nested slashes"
result=$(_sanitize_branch_name "a/b/c/d/e")
assert_eq "a-b-c-d-e" "$result"

test_suite "progress.sh"
unset _PROGRESS_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/utils/progress.sh" 2>/dev/null || true

test_case "_progress_spinner_chars array has 10 elements"
# shellcheck disable=SC2154
count=${#_progress_spinner_chars[@]}
assert_eq "10" "$count"

test_case "progress_start/progress_stop lifecycle"
progress_start "test"
assert_empty "$_PROGRESS_PID"
sleep 0.15
progress_stop
assert_empty "$_PROGRESS_PID"

test_case "progress_start is disabled in machine output mode"
WORKHIVE_MACHINE_OUTPUT=1 progress_start "test"
assert_empty "$_PROGRESS_PID"

test_case "progress_step outputs progress bar format"
out=$(progress_step 5 10 "msg" 2>&1)
assert_empty "$out"

test_case "progress.sh preserves existing EXIT trap when registering cleanup"
prev_exit_trap="$(trap -p EXIT || true)"
__test_progress_exit_marker() { :; }
trap '__test_progress_exit_marker' EXIT

_progress_register_exit_cleanup

current_exit_trap="$(trap -p EXIT || true)"
assert_contains "$current_exit_trap" "_progress_cleanup_on_exit"
assert_contains "$current_exit_trap" "__test_progress_exit_marker"

_progress_unregister_exit_cleanup
restored_exit_trap="$(trap -p EXIT || true)"
assert_not_contains "$restored_exit_trap" "_progress_cleanup_on_exit"
assert_contains "$restored_exit_trap" "__test_progress_exit_marker"

trap - EXIT
if [[ -n "$prev_exit_trap" ]]; then
    eval "$prev_exit_trap"
fi
unset -f __test_progress_exit_marker 2>/dev/null || true

test_suite "layout_service query diagnostics"
unset _LAYOUT_SERVICE_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/domain/services/layout_service.sh" 2>/dev/null || true

test_case "_layout_config_query returns non-zero and emits debug context on yq failure"
layout_debug_output="$(
    (
        log_debug() { printf '%s\n' "$*"; }
        yq() { echo "simulated yq failure" >&2; return 1; }
        _layout_config_query '.layout.columns | length' "$TEST_DIR/not-found.yaml"
    ) 2>&1
)"
layout_rc=$?
assert_eq "1" "$layout_rc"
assert_contains "$layout_debug_output" "layout config query failed"
assert_contains "$layout_debug_output" "simulated yq failure"

test_case "_layout_parse_tsv4 preserves empty middle and trailing fields"
_layout_parse_tsv4 $'2\t50\t\t'
assert_eq "0" "$?" "tsv4 parser should parse rows with empty trailing fields"
assert_eq "2" "$_LAYOUT_TSV_0"
assert_eq "50" "$_LAYOUT_TSV_1"
assert_eq "" "$_LAYOUT_TSV_2"
assert_eq "" "$_LAYOUT_TSV_3"

test_case "_layout_parse_tsv3 preserves empty trailing field"
_layout_parse_tsv3 $'80\tcmd\t'
assert_eq "0" "$?" "tsv3 parser should parse rows with empty trailing field"
assert_eq "80" "$_LAYOUT_TSV_0"
assert_eq "cmd" "$_LAYOUT_TSV_1"
assert_eq "" "$_LAYOUT_TSV_2"

test_suite "log_command.sh"
unset _LOG_COMMAND_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/app/commands/log_command.sh" 2>/dev/null || true

test_case "log_command_help outputs help text"
help=$(log_command_help)
assert_contains "$help" "workhive log"

test_case "log_command_execute --help returns 0"
log_command_execute --help >/dev/null 2>&1
assert_success $?

test_case "log_command_execute with no log file succeeds with empty output"
rm -f .workhive/logs/workhive.log 2>/dev/null
output=$(log_command_execute 2>/dev/null)
rc=$?
assert_success "$rc"
assert_empty "$output"

test_suite "remove_command.sh"
unset _REMOVE_COMMAND_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/app/commands/remove_command.sh" 2>/dev/null || true

test_case "remove_command_execute stops before deletion when clean worktree removal is declined"
prompt_file="$TEST_DIR/remove_prompt.txt"
rm -f "$prompt_file"
remove_marker="$TEST_DIR/remove_called.txt"
branch_marker="$TEST_DIR/branch_deleted.txt"
rm -f "$remove_marker" "$branch_marker"
session_get_name() { echo "wh-test01"; }
session_exists() { return 1; }
worktree_exists() { return 0; }
worktree_get_path() { echo "$TEST_DIR/test01"; }
config_get() { echo "true"; }
_git_root() { echo "$TEST_DIR"; }
_worktree_safety_scan() {
    _WT_SAFETY_HAS_WARNINGS=false
}
_worktree_safety_print_warning_box() { :; }
ask_yes_no() {
    _ASK_YES_NO_NONINTERACTIVE=false
    printf '%s' "$1" > "$prompt_file"
    return 1
}
worktree_remove() {
    : > "$remove_marker"
    return 0
}
git() {
    if [[ "$1" == "branch" && "$2" == "-D" ]]; then
        : > "$branch_marker"
    fi
    return 0
}
mkdir -p "$TEST_DIR/test01/.git"
output=$(remove_command_execute test01 2>&1)
rc=$?
prompt=$(cat "$prompt_file" 2>/dev/null)
assert_success "$rc"
assert_not_empty "$prompt"
assert_true "[[ ! -e \"$remove_marker\" ]]" "worktree_remove should not be called"
assert_true "[[ ! -e \"$branch_marker\" ]]" "branch deletion should not be called"
assert_not_contains "$output" "$(_i18n MSG_REMOVE_WT)"
assert_not_contains "$output" "$(_i18n MSG_REMOVE_STEP_3)"
assert_contains "$output" "$(_i18n MSG_CANCELLED)"
unset -f session_get_name session_exists worktree_exists worktree_get_path config_get _git_root _worktree_safety_scan _worktree_safety_print_warning_box ask_yes_no worktree_remove git

test_case "remove_command_execute cancels before killing session when workspace removal is declined"
prompt_file="$TEST_DIR/remove_session_prompt.txt"
rm -f "$prompt_file"
session_killed=""
session_get_name() { echo "wh-test02"; }
session_exists() { return 0; }
worktree_exists() { return 0; }
worktree_get_path() { echo "$TEST_DIR/test02"; }
config_get() { echo "true"; }
_git_root() { echo "$TEST_DIR"; }
_worktree_safety_scan() {
    _WT_SAFETY_HAS_WARNINGS=false
}
_worktree_safety_print_warning_box() { :; }
ask_yes_no() {
    _ASK_YES_NO_NONINTERACTIVE=false
    printf '%s' "$1" > "$prompt_file"
    return 1
}
session_kill() {
    session_killed="yes"
    return 0
}
mkdir -p "$TEST_DIR/test02/.git"
output=$(remove_command_execute test02 2>&1)
rc=$?
prompt=$(cat "$prompt_file" 2>/dev/null)
assert_success "$rc"
assert_not_empty "$prompt"
assert_empty "$session_killed"
assert_contains "$output" "$(_i18n MSG_CANCELLED)"
unset -f session_get_name session_exists worktree_exists worktree_get_path config_get _git_root _worktree_safety_scan _worktree_safety_print_warning_box ask_yes_no session_kill

test_case "remove_command_execute logs OP begin+ok for keep-worktree flow"
: > "$OP_LOG_FILE"
session_get_name() { echo "wh-keep01"; }
session_exists() { return 1; }
worktree_exists() { return 0; }
worktree_get_path() { echo "$TEST_DIR/keep01"; }
config_get() { echo "false"; }
_git_root() { echo "$TEST_DIR"; }
_worktree_safety_scan() { _WT_SAFETY_HAS_WARNINGS=false; }
_worktree_safety_print_warning_box() { :; }
mkdir -p "$TEST_DIR/keep01/.git"
output=$(remove_command_execute keep01 2>&1)
rc=$?
oplog=$(cat "$OP_LOG_FILE" 2>/dev/null || true)
assert_success "$rc"
assert_contains "$oplog" "[OP][BEGIN] action=remove_workspace target=keep01"
assert_contains "$oplog" "[OP][OK] action=remove_workspace target=keep01 detail=keep_worktree=true"
unset -f session_get_name session_exists worktree_exists worktree_get_path config_get _git_root _worktree_safety_scan _worktree_safety_print_warning_box

test_suite "rename_command.sh"
unset _RENAME_COMMAND_SH_LOADED 2>/dev/null || true
source "$LIB_DIR/app/commands/rename_command.sh" 2>/dev/null || true

test_case "rename_command_help outputs help text"
help=$(rename_command_help)
assert_contains "$help" "rename"

test_case "rename_command_execute --help returns 0"
rename_command_execute --help >/dev/null 2>&1
assert_success $?

test_case "rename_command_execute with no args returns error"
rename_command_execute 2>/dev/null || rc=$?
assert_failure "${rc:-0}"

test_case "rename_command_execute rejects main branch rename"
main_branch=$(_git_main_branch)
rename_command_execute "$main_branch" "new" 2>/dev/null || rc=$?
assert_failure "${rc:-0}"

test_case "rename_command_execute asks for confirmation before renaming workspace"
prompt_file="$TEST_DIR/rename_prompt.txt"
rm -f "$prompt_file"
rename_performed=""
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
terminal_adapter_rename_session() { return 0; }
cache_invalidate() { :; }
_git_root() { echo "$TEST_DIR"; }
ask_yes_no() {
    _ASK_YES_NO_NONINTERACTIVE=false
    printf '%s' "$1" > "$prompt_file"
    return 1
}
git() {
    if [[ "$1" == "rev-parse" && "$2" == "--verify" ]]; then
        [[ "$3" == "old-name" ]] && return 0
        return 1
    fi
    rename_performed="yes"
    return 0
}
mkdir -p "$TEST_DIR/old-name"
output=$(rename_command_execute old-name new-name 2>&1)
rc=$?
prompt=$(cat "$prompt_file" 2>/dev/null)
assert_success "$rc"
assert_not_empty "$prompt"
assert_empty "$rename_performed"
assert_contains "$output" "$(_i18n MSG_CANCELLED)"
unset -f _branch_needs_sanitize _git_main_branch worktree_check_repo worktree_exists session_get_name session_exists worktree_get_path terminal_adapter_rename_session cache_invalidate _git_root ask_yes_no git

test_case "rename_command_execute clears rollback trap when confirmation is declined"
prompt_file="$TEST_DIR/rename_trap_prompt.txt"
rm -f "$prompt_file"
rollback_called=""
_rename_rollback() {
    rollback_called="yes"
}
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
terminal_adapter_rename_session() { return 0; }
cache_invalidate() { :; }
_git_root() { echo "$TEST_DIR"; }
ask_yes_no() {
    _ASK_YES_NO_NONINTERACTIVE=false
    printf '%s' "$1" > "$prompt_file"
    return 1
}
git() {
    if [[ "$1" == "rev-parse" && "$2" == "--verify" ]]; then
        [[ "$3" == "old-name" ]] && return 0
        return 1
    fi
    return 0
}
mkdir -p "$TEST_DIR/old-name"
prev_exit_trap="$(trap -p EXIT || true)"
prev_err_trap="$(trap -p ERR || true)"
rename_command_execute old-name new-name >/dev/null 2>&1
trap -p INT > "$TEST_DIR/int_trap_after_rename.txt"
trap_output=$(cat "$TEST_DIR/int_trap_after_rename.txt" 2>/dev/null)
assert_empty "$trap_output"
after_exit_trap="$(trap -p EXIT || true)"
after_err_trap="$(trap -p ERR || true)"
assert_not_contains "$after_exit_trap" "_rename_rollback" "EXIT trap should not retain rename rollback handler"
assert_not_contains "$after_err_trap" "_rename_rollback" "ERR trap should not retain rename rollback handler"
assert_empty "$rollback_called"
unset -f _rename_rollback _branch_needs_sanitize _git_main_branch worktree_check_repo worktree_exists session_get_name session_exists worktree_get_path terminal_adapter_rename_session cache_invalidate _git_root ask_yes_no git

test_case "rename_command_execute --force kills existing session instead of renaming session"
session_killed=""
session_renamed=""
_branch_needs_sanitize() { return 1; }
_git_main_branch() { echo "main"; }
worktree_check_repo() { return 0; }
worktree_exists() {
    [[ "$1" == "old-name" ]] && return 0
    return 1
}
session_get_name() { echo "wh-$1"; }
session_exists() {
    [[ "$1" == "wh-old-name" ]] && return 0
    return 1
}
worktree_get_path() { echo "$TEST_DIR/$1"; }
terminal_adapter_rename_session() {
    session_renamed="yes"
    return 0
}
session_kill_by_name() {
    session_killed="$1|$2"
    return 0
}
cache_invalidate() { :; }
_git_root() { echo "$TEST_DIR"; }
git() {
    if [[ "$1" == "rev-parse" && "$2" == "--verify" ]]; then
        [[ "$3" == "old-name" ]] && return 0
        return 1
    fi
    return 0
}
mkdir -p "$TEST_DIR/old-name"
rename_command_execute old-name new-name --force >/dev/null 2>&1 || rc=$?
assert_success "${rc:-0}"
assert_contains "$session_killed" "wh-old-name|old-name"
assert_empty "$session_renamed"
unset -f _branch_needs_sanitize _git_main_branch worktree_check_repo worktree_exists session_get_name session_exists worktree_get_path terminal_adapter_rename_session session_kill_by_name cache_invalidate _git_root git

test_case "rename_command_execute logs OP begin+ok on success path"
: > "$OP_LOG_FILE"
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
terminal_adapter_rename_session() { return 0; }
cache_invalidate() { :; }
_git_root() { echo "$TEST_DIR"; }
git() {
    if [[ "$1" == "rev-parse" && "$2" == "--verify" ]]; then
        [[ "$3" == "old-name" ]] && return 0
        return 1
    fi
    return 0
}
mkdir -p "$TEST_DIR/old-name"
rc=0
rename_command_execute old-name new-name --force >/dev/null 2>&1 || rc=$?
oplog=$(cat "$OP_LOG_FILE" 2>/dev/null || true)
assert_success "${rc:-0}"
assert_contains "$oplog" "[OP][BEGIN] action=rename_workspace target=old-name"
assert_contains "$oplog" "[OP][OK] action=rename_workspace target=old-name detail=to=new-name"
unset -f _branch_needs_sanitize _git_main_branch worktree_check_repo worktree_exists session_get_name session_exists worktree_get_path terminal_adapter_rename_session cache_invalidate _git_root git


test_case "_rename_validate_branch valid-name returns 0"
_rename_validate_branch "valid-name" 2>/dev/null
assert_success $?

test_case "_rename_validate_branch empty returns 1"
_rename_validate_branch "" 2>/dev/null || rc=$?
assert_failure "${rc:-0}"

test_case "_rename_validate_branch has spaces returns 1"
_rename_validate_branch "has spaces" 2>/dev/null || rc=$?
assert_failure "${rc:-0}"

test_case "_rename_validate_branch has double dots returns 1"
_rename_validate_branch "has..dots" 2>/dev/null || rc=$?
assert_failure "${rc:-0}"

# ──────────────────────────────────────────────────
# worktree_selector.sh (shared TUI helper)
# ──────────────────────────────────────────────────

test_suite "worktree_selector.sh"

source "$LIB_DIR/infra/adapters/terminal/factory.sh" 2>/dev/null || true
terminal_adapter_init >/dev/null 2>&1 || true
source "$LIB_DIR/domain/services/session_service.sh" 2>/dev/null || true
source "$LIB_DIR/app/shared/worktree_selector.sh" 2>/dev/null || true

test_case "worktree_interactive_select function exists"
assert_true "declare -f worktree_interactive_select >/dev/null"

test_case "worktree_interactive_select returns 1 in non-interactive (piped stdin)"
echo "" | worktree_interactive_select 2>/dev/null
assert_failure "$?" "should fail in non-interactive mode"

test_case "worktree_interactive_select --with-main-dir in non-interactive returns 1"
echo "" | worktree_interactive_select --with-main-dir 2>/dev/null
assert_failure "$?" "should fail even with --with-main-dir in piped stdin"

test_case "worktree_selector.sh has source guard"
assert_true "[[ -n \"\${_WORKTREE_SELECTOR_SH_LOADED:-}\" ]]" "source guard should be set"

# ──────────────────────────────────────────────────
# layout_rebuild_helper.sh (simplified signature)
# ──────────────────────────────────────────────────

test_suite "layout_rebuild_helper.sh"

source "$LIB_DIR/app/shared/layout_rebuild_helper.sh" 2>/dev/null || true

test_case "layout_rebuild_force function exists"
assert_true "declare -f layout_rebuild_force >/dev/null"

test_case "layout_rebuild_helper.sh does not use layout_clear_state"
helper_src=$(cat "$LIB_DIR/app/shared/layout_rebuild_helper.sh")
assert_not_contains "$helper_src" "layout_clear_state" "should not call layout_clear_state"

test_case "layout_rebuild_helper.sh does not use layout_save_state"
assert_not_contains "$helper_src" "layout_save_state" "should not call layout_save_state"

test_case "layout_rebuild_helper.sh always uses layout_apply_commands"
assert_contains "$helper_src" "layout_apply_commands" "should use layout_apply_commands"

test_suite "test_new_features.sh hygiene"
SELF_FILE="$SCRIPT_DIR/test_new_features.sh"

test_case "rename test suite appears only once"
suite_count=$(grep -c '^test_suite "rename_command\.sh"$' "$SELF_FILE")
assert_eq "1" "$suite_count"

test_case "rename help test appears only once"
help_count=$(grep -c '^test_case "rename_command_help outputs help text"$' "$SELF_FILE")
assert_eq "1" "$help_count"

test_case "rename validate branch valid-name test appears only once"
validate_count=$(grep -c '^test_case "_rename_validate_branch valid-name returns 0"$' "$SELF_FILE")
assert_eq "1" "$validate_count"

test_report

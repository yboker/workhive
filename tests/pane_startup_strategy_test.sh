#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"

source "$LIB_DIR/domain/services/pane_startup_strategy_service.sh"

declare -A _TEST_META=()
_TEST_STARTUP_FAIL=false
_TEST_SHELL_INIT_FAIL=false
_TEST_STARTUP_CALLS=0
_TEST_SHELL_CALLS=0

pane_meta_transition_state() {
    local session_name="$1"
    local pane_index="$2"
    local state="$3"
    local err="${4:-}"
    _TEST_META["${session_name}:${pane_index}:state"]="$state"
    _TEST_META["${session_name}:${pane_index}:err"]="$err"
}

_pane_meta_classify_kind() {
    local cmd="${1:-}"
    case "$cmd" in
        codex|claude|hermes*) echo "agent" ;;
        lazygit|vim|nvim) echo "tool" ;;
        *) echo "shell" ;;
    esac
}

session_send_startup_command() {
    _TEST_STARTUP_CALLS=$((_TEST_STARTUP_CALLS + 1))
    [[ "$_TEST_STARTUP_FAIL" == true ]] && return 1
    return 0
}

session_prepare_shell_pane() {
    _TEST_SHELL_CALLS=$((_TEST_SHELL_CALLS + 1))
    [[ "$_TEST_SHELL_INIT_FAIL" == true ]] && return 1
    return 0
}

test_suite "pane startup strategy"

test_case "strategy selector: command"
strategy=$(pane_startup_select_strategy "codex" "/tmp")
assert_eq "command" "$strategy" "non-empty command should select command strategy"

test_case "strategy selector: shell_init"
strategy=$(pane_startup_select_strategy "" "/tmp")
assert_eq "shell_init" "$strategy" "empty command + dir should select shell_init"

test_case "strategy selector: noop"
strategy=$(pane_startup_select_strategy "" "")
assert_eq "noop" "$strategy" "empty command + empty dir should select noop"

test_case "apply command strategy sets running for agent"
_TEST_META=()
_TEST_STARTUP_FAIL=false
_TEST_SHELL_INIT_FAIL=false
_TEST_STARTUP_CALLS=0
_TEST_SHELL_CALLS=0
rc=0
pane_startup_apply "main" "wh@test" "main" "0" "codex" "/tmp" || rc=$?
assert_eq 0 "$rc" "startup should succeed"
assert_eq "running" "${_TEST_META["wh@test:0:state"]:-}" "agent command should end in running"
assert_eq 1 "$_TEST_STARTUP_CALLS" "command strategy should use startup command path"
assert_eq 0 "$_TEST_SHELL_CALLS" "command strategy should not use shell init path"

test_case "apply shell_init strategy sets idle"
_TEST_META=()
_TEST_STARTUP_FAIL=false
_TEST_SHELL_INIT_FAIL=false
_TEST_STARTUP_CALLS=0
_TEST_SHELL_CALLS=0
rc=0
pane_startup_apply "main" "wh@test" "shell" "1" "" "/tmp" || rc=$?
assert_eq 0 "$rc" "shell_init should succeed"
assert_eq "idle" "${_TEST_META["wh@test:1:state"]:-}" "shell_init should end in idle"
assert_eq 0 "$_TEST_STARTUP_CALLS" "shell_init should avoid generic startup command path"
assert_eq 1 "$_TEST_SHELL_CALLS" "shell_init should use dedicated shell path"

test_case "apply failure sets failed with error"
_TEST_META=()
_TEST_STARTUP_FAIL=true
_TEST_SHELL_INIT_FAIL=false
_TEST_STARTUP_CALLS=0
_TEST_SHELL_CALLS=0
rc=0
pane_startup_apply "main" "wh@test" "main" "2" "codex" "/tmp" || rc=$?
assert_ne 0 "$rc" "startup failure should return non-zero"
assert_eq "failed" "${_TEST_META["wh@test:2:state"]:-}" "failure should set failed state"
assert_contains "${_TEST_META["wh@test:2:err"]:-}" "startup command failed" "error should be explicit"

test_report

#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"

# ──────────────────────────────────────────────────
# In-memory pane option store (no tmux dependency)
# ──────────────────────────────────────────────────
declare -A _TEST_PANE_OPTIONS=()
terminal_adapter_set_pane_option() {
    _TEST_PANE_OPTIONS["${1}:${2}:${3}"]="$4"
}
terminal_adapter_get_pane_option() {
    printf '%s' "${_TEST_PANE_OPTIONS["${1}:${2}:${3}"]:-}"
}
terminal_adapter_unset_pane_option() {
    unset '_TEST_PANE_OPTIONS["${1}:${2}:${3}"]' 2>/dev/null || true
}

source "$LIB_DIR/domain/services/layout_service.sh"

test_suite "layout apply commands"

_orig_session_require_exists=$(declare -f session_require_exists)
_orig_layout_wait_for_geometry_stable=$(declare -f _layout_wait_for_geometry_stable)
_orig_pane_mapping_init=$(declare -f _pane_mapping_init)
_orig_pane_get_index=$(declare -f _pane_get_index)
_orig_layout_collect_pane_commands=$(declare -f layout_collect_pane_commands)
_orig_session_send_startup_command=$(declare -f session_send_startup_command)
_orig_session_prepare_shell_pane=$(declare -f session_prepare_shell_pane)
_orig_session_send_command=$(declare -f session_send_command)
_orig_terminal_adapter_list_panes_format=$(declare -f terminal_adapter_list_panes_format)
_orig_terminal_adapter_reflow_session_panes=$(declare -f terminal_adapter_reflow_session_panes)

session_require_exists() { SESSION_NAME="wh@test"; return 0; }
_layout_wait_for_geometry_stable() { return 0; }
_pane_mapping_init() { return 0; }
_pane_get_index() { echo "0"; return 0; }
terminal_adapter_list_panes_format() { printf '0\t1\t1\tready\n'; }
terminal_adapter_reflow_session_panes() { return 0; }
session_prepare_shell_pane() { return 0; }

# ──────────────────────────────────────────────────
# Basic failure/success propagation
# ──────────────────────────────────────────────────

test_case "layout_apply_commands returns failure when startup command fails"
_TEST_PANE_OPTIONS=()
layout_collect_pane_commands() {
    cat <<'ROWS'
main|codex
ROWS
}
session_send_startup_command() { return 1; }
session_send_command() { return 0; }
rc=0
layout_apply_commands "main" "/tmp" >/dev/null 2>&1 || rc=$?
assert_ne 0 "$rc" "should propagate startup command failure"

test_case "layout_apply_commands succeeds when all pane commands apply"
_TEST_PANE_OPTIONS=()
session_send_startup_command() { return 0; }
rc=0
layout_apply_commands "main" "/tmp" >/dev/null 2>&1 || rc=$?
assert_eq 0 "$rc" "should succeed when no pane command fails"

# ──────────────────────────────────────────────────
# Phase 2: State machine behavior
# ──────────────────────────────────────────────────

test_case "successful startup transitions pane to running (agent kind)"
_TEST_PANE_OPTIONS=()
layout_collect_pane_commands() { echo "main|codex"; }
session_send_startup_command() { return 0; }
layout_apply_commands "main" "/tmp" >/dev/null 2>&1
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "running" "$state" "agent pane should be in running state"

test_case "failed startup transitions pane to failed with error"
_TEST_PANE_OPTIONS=()
layout_collect_pane_commands() { echo "main|codex"; }
session_send_startup_command() { return 1; }
layout_apply_commands "main" "/tmp" >/dev/null 2>&1 || true
state=$(pane_meta_get "wh@test" "0" "state")
last_error=$(pane_meta_get "wh@test" "0" "last_error")
assert_eq "failed" "$state" "failed pane should have state=failed"
assert_contains "$last_error" "codex" "last_error should mention the failed command"

test_case "empty-command pane transitions to idle"
_TEST_PANE_OPTIONS=()
layout_collect_pane_commands() { echo "shell_pane|"; }
session_prepare_shell_pane() { return 0; }
session_send_command() { return 0; }
layout_apply_commands "main" "/tmp" >/dev/null 2>&1
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "idle" "$state" "shell pane without command should be idle"

test_case "failure message includes role name"
_TEST_PANE_OPTIONS=()
layout_collect_pane_commands() { echo "my_agent|claude"; }
session_send_startup_command() { return 1; }
output=$(layout_apply_commands "main" "/tmp" 2>&1) || true
assert_contains "$output" "my_agent" "error output should mention failing role"

# ──────────────────────────────────────────────────
# Multi-pane scenario
# ──────────────────────────────────────────────────

test_case "multi-pane: partial failure reports correct roles"
_TEST_PANE_OPTIONS=()
_test_pane_idx=0
_pane_get_index() {
    echo "$_test_pane_idx"
    _test_pane_idx=$((_test_pane_idx + 1))
    return 0
}
layout_collect_pane_commands() {
    printf 'agent|claude\nshell|\ntool|lazygit\n'
}
session_send_startup_command() {
    local pane_idx="$2"
    local cmd="$3"
    [[ "$cmd" == "lazygit" ]] && return 1
    return 0
}
session_prepare_shell_pane() { return 0; }
session_send_command() { return 0; }
_test_pane_idx=0
rc=0
output=$(layout_apply_commands "main" "/tmp" 2>&1) || rc=$?
assert_ne 0 "$rc" "should fail when any pane fails"
assert_contains "$output" "tool" "should name the failing role"

test_case "render wait delays success until command pane leaves 0,0"
_TEST_PANE_OPTIONS=()
_test_render_counter_file=$(mktemp)
printf '0' > "$_test_render_counter_file"
layout_collect_pane_commands() { echo "agent|codex"; }
session_send_startup_command() { return 0; }
pane_meta_write_identity "wh@test" "0" "agent" "agent" "codex"
pane_meta_transition_state "wh@test" "0" "launching"
terminal_adapter_list_panes_format() {
    local calls=0
    calls=$(cat "$_test_render_counter_file")
    calls=$((calls + 1))
    printf '%s' "$calls" > "$_test_render_counter_file"
    if [[ "$calls" -lt 3 ]]; then
        printf '0\t0\t0\tBOBO-MAC.local\n'
    else
        printf '0\t2\t8\tworkhive\n'
    fi
}
layout_apply_commands "main" "/tmp" >/dev/null 2>&1
assert_eq 3 "$(cat "$_test_render_counter_file")" "render wait should poll until pane leaves initial cursor position"
rm -f "$_test_render_counter_file"

test_case "render wait skips empty shell pane"
_TEST_PANE_OPTIONS=()
_test_render_counter_file=$(mktemp)
printf '0' > "$_test_render_counter_file"
layout_collect_pane_commands() { echo "shell_pane|"; }
session_prepare_shell_pane() { return 0; }
terminal_adapter_list_panes_format() {
    local calls=0
    calls=$(cat "$_test_render_counter_file")
    calls=$((calls + 1))
    printf '%s' "$calls" > "$_test_render_counter_file"
    printf '0\t0\t0\tBOBO-MAC.local\n'
}
layout_apply_commands "main" "/tmp" >/dev/null 2>&1
assert_eq 1 "$(cat "$_test_render_counter_file")" "empty shell pane should not block render wait"
rm -f "$_test_render_counter_file"

# ──────────────────────────────────────────────────
# Cleanup
# ──────────────────────────────────────────────────

unset -f session_require_exists _layout_wait_for_geometry_stable _pane_mapping_init _pane_get_index layout_collect_pane_commands session_send_startup_command session_prepare_shell_pane session_send_command terminal_adapter_list_panes_format terminal_adapter_reflow_session_panes
builtin eval "$_orig_session_require_exists"
builtin eval "$_orig_layout_wait_for_geometry_stable"
builtin eval "$_orig_pane_mapping_init"
builtin eval "$_orig_pane_get_index"
builtin eval "$_orig_layout_collect_pane_commands"
builtin eval "$_orig_session_send_startup_command"
builtin eval "$_orig_session_prepare_shell_pane"
builtin eval "$_orig_session_send_command"
builtin eval "$_orig_terminal_adapter_list_panes_format"
builtin eval "$_orig_terminal_adapter_reflow_session_panes"

test_report

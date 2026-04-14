#!/usr/bin/env bash
# Simple unit test example for `workhive pane list`

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/app/commands/pane_command.sh"

test_suite "pane list - basic function"

test_case "pane list --format json returns session and pane fields"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_agents=$(declare -f pane_state_collect_session)

_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@test"; }
terminal_adapter_get_current_pane_index() { echo "0"; }
terminal_adapter_list_panes_format() {
    cat <<'ROWS'
0|%10|bash|/tmp/workhive|120x30|0
ROWS
}
pane_state_collect_session() {
    cat <<'ROWS'
0|%10|101||idle|probe|high|NO_AGENT
ROWS
}

rc=0
output=$(_pane_cmd_list --format json 2>&1) || rc=$?

unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format pane_state_collect_session
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_agents"

assert_eq 0 "$rc" "pane list should succeed"
assert_contains "$output" '"session":"wh@test"' "should include session"
assert_contains "$output" '"pane_index":0' "should include pane index"
assert_contains "$output" '"current_command":"bash"' "should include command"
assert_contains "$output" '"is_current":true' "should mark current pane"

test_report

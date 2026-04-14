#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"

test_suite "list command dedicated runtime isolation"

test_case "tmux_runtime_server_name returns dedicated server name"
source "$LIB_DIR/infra/adapters/terminal/tmux_runtime.sh"
WORKHIVE_TMUX_SERVER_NAME="workhive-test-abc123"
name=$(tmux_runtime_server_name)
assert_eq "workhive-test-abc123" "$name" "should use WORKHIVE_TMUX_SERVER_NAME when set"

test_case "tmux_runtime_cli_prefix includes -L flag"
WORKHIVE_TMUX_SERVER_NAME="workhive-test-abc123"
cli=$(tmux_runtime_cli_prefix)
assert_contains "$cli" "-L" "should include -L flag for dedicated server"
assert_contains "$cli" "workhive-test-abc123" "should include server name"

test_case "tmux wrapper prepends -L server flag"
WORKHIVE_TMUX_SERVER_NAME="workhive-test-isolate"
_TEST_TMUX_ARGS=""
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        _TEST_TMUX_ARGS="$*"
        return 0
    fi
    builtin command "$@"
}
tmux list-sessions -F '#{session_name}'
assert_contains "$_TEST_TMUX_ARGS" "-L workhive-test-isolate" "tmux wrapper should prepend -L <server>"
unset -f command

test_case "terminal_adapter_list_sessions goes through tmux wrapper"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"
WORKHIVE_TMUX_SERVER_NAME="workhive-test-list"
_LIST_CALLS=""
tmux() {
    _LIST_CALLS+="$*"$'\n'
    return 0
}
terminal_adapter_list_sessions >/dev/null 2>&1
assert_contains "$_LIST_CALLS" "list-sessions" "adapter should call list-sessions through wrapper"

unset WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true

test_report

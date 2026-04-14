#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/persistence/restore.sh"

_RESTORE_TMUX_CALLS=""

# Override the tmux wrapper from tmux_runtime.sh
tmux() {
    case "$1" in
        switch-client)
            _RESTORE_TMUX_CALLS+="switch-client $*"$'\n'
            return 0
            ;;
        has-session)
            return 0
            ;;
    esac
    command tmux "$@" 2>/dev/null || return 0
}

test_suite "restore active-state runtime safety"

snapshot_get_state_line() { echo '{"type":"state","active_session":"wh@test@main","last_session":""}'; }

test_case "restore active-state skips switch-client when outside tmux"
_RESTORE_TMUX_CALLS=""
unset TMUX 2>/dev/null || true
_restore_active_state "/dev/null"
assert_not_contains "$_RESTORE_TMUX_CALLS" "switch-client" "should not attempt switch-client outside tmux"

test_case "restore active-state uses switch-client on same server"
_RESTORE_TMUX_CALLS=""
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
_terminal_tmux_select_attach_strategy() { printf 'same-server\n'; }
_restore_active_state "/dev/null"
assert_contains "$_RESTORE_TMUX_CALLS" "switch-client" "should use switch-client on same server"

test_case "restore active-state skips switch-client on cross-server"
_RESTORE_TMUX_CALLS=""
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
_terminal_tmux_select_attach_strategy() { printf 'cross-server\n'; }
_restore_active_state "/dev/null"
assert_not_contains "$_RESTORE_TMUX_CALLS" "switch-client" "should skip switch-client across servers"

unset TMUX WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true
unset -f _terminal_tmux_select_attach_strategy 2>/dev/null || true

test_report

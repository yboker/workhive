#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"

test_suite "tmux attach state"

_TMUX_ATTACH_CALLS=""
tmux() {
    case "$1" in
        attach)
            _TMUX_ATTACH_CALLS+=$'attach '"$*\n"
            return 0
            ;;
        switch-client)
            _TMUX_ATTACH_CALLS+=$'switch-client '"$*\n"
            return 0
            ;;
        set-option)
            _TMUX_ATTACH_CALLS+=$'set-option '"$*\n"
            return 0
            ;;
        list-panes)
            printf '%%1|1\n%%2|0\n'
            return 0
            ;;
        send-keys)
            _TMUX_ATTACH_CALLS+=$'send-keys '"$*\n"
            return 0
            ;;
        list-clients)
            return 0
            ;;
        display-message)
            printf '120x40|120x40\n'
            return 0
            ;;
    esac
    return 0
}

test_case "normalize interactive state resets key table and exits pane modes"
_TMUX_ATTACH_CALLS=""
_terminal_tmux_normalize_interactive_state "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -t wh@test key-table root" "should normalize session key-table"
assert_contains "$_TMUX_ATTACH_CALLS" "send-keys send-keys -t %1 -X cancel" "should cancel copy-mode panes"

test_case "normalize interactive state skips panes not in mode"
_TMUX_ATTACH_CALLS=""
_terminal_tmux_normalize_interactive_state "wh@test"
assert_not_contains "$_TMUX_ATTACH_CALLS" "%2 -X cancel" "should not touch panes already in root mode"

test_case "attach uses switch-client when already inside same dedicated tmux server"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -g focus-events on" "should enforce focus-events on before same-server attach"
assert_contains "$_TMUX_ATTACH_CALLS" "switch-client switch-client -t wh@test" "should switch client on same server"
assert_not_contains "$_TMUX_ATTACH_CALLS" "attach attach -t wh@test" "should not spawn nested attach on same server"

test_case "attach uses fresh tmux attach when inside a different server"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -g focus-events on" "should enforce focus-events on before cross-server attach"
assert_contains "$_TMUX_ATTACH_CALLS" "attach attach -t wh@test" "should force attach path for cross-server switch"
assert_not_contains "$_TMUX_ATTACH_CALLS" "switch-client switch-client -t wh@test" "should avoid switch-client across servers"

test_case "readonly attach uses switch-client -r when inside same dedicated tmux server"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session_readonly "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -g focus-events on" "should enforce focus-events on before same-server readonly attach"
assert_contains "$_TMUX_ATTACH_CALLS" "switch-client switch-client -t wh@test -r" "should switch client read-only on same server"
assert_not_contains "$_TMUX_ATTACH_CALLS" "attach attach -t wh@test" "should not spawn nested attach on same server for readonly"

test_case "readonly attach uses fresh tmux attach -r when inside a different server"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session_readonly "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -g focus-events on" "should enforce focus-events on before cross-server readonly attach"
assert_contains "$_TMUX_ATTACH_CALLS" "attach attach -t wh@test -r" "should force readonly attach path for cross-server"
assert_not_contains "$_TMUX_ATTACH_CALLS" "switch-client switch-client -t wh@test" "should avoid switch-client across servers for readonly"

test_case "readonly attach uses fresh tmux attach -r when outside tmux"
_TMUX_ATTACH_CALLS=""
unset TMUX
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session_readonly "wh@test"
assert_contains "$_TMUX_ATTACH_CALLS" "set-option set-option -g focus-events on" "should enforce focus-events on before outside readonly attach"
assert_contains "$_TMUX_ATTACH_CALLS" "attach attach -t wh@test -r" "should use direct attach readonly outside tmux"
assert_not_contains "$_TMUX_ATTACH_CALLS" "switch-client" "should not use switch-client outside tmux"

test_case "attach status tracks attached on successful same-server attach"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session "wh@test"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "attached" "$_status" "should report attached on successful switch-client"

test_case "attach status tracks detached on successful cross-server attach"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
terminal_adapter_attach_session "wh@test"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "detached" "$_status" "should report detached on successful outside/cross attach"

# ══════════════════════════════════════════════════
# Readonly attach failure propagation regression
# ══════════════════════════════════════════════════

test_case "readonly attach propagates error on same-server switch-client failure"
_TMUX_ATTACH_CALLS=""
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
tmux() {
    case "$1" in
        switch-client) return 1 ;;
        display-message) printf '120x40|120x40\n' ;;
        list-panes) printf '%%1|0\n' ;;
        list-clients) return 0 ;;
    esac
    return 0
}
_ro_rc=0
terminal_adapter_attach_session_readonly "wh@test" || _ro_rc=$?
assert_eq "1" "$_ro_rc" "readonly same-server failure must return non-zero"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "error" "$_status" "readonly same-server failure must report error status"

test_case "readonly attach propagates error on cross-server attach failure with live session"
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
tmux() {
    case "$1" in
        attach) return 1 ;;
        has-session) return 0 ;;
        display-message) printf '120x40|120x40\n' ;;
        list-panes) printf '%%1|0\n' ;;
        list-clients) return 0 ;;
    esac
    return 0
}
_ro_rc=0
terminal_adapter_attach_session_readonly "wh@test" || _ro_rc=$?
assert_eq "1" "$_ro_rc" "readonly cross-server failure with live session must return non-zero"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "error" "$_status" "readonly cross-server failure with live session must report error status"

test_case "readonly attach returns success with ended status when session disappeared"
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
tmux() {
    case "$1" in
        attach) return 1 ;;
        has-session) return 1 ;;
        display-message) printf '120x40|120x40\n' ;;
        list-panes) printf '%%1|0\n' ;;
        list-clients) return 0 ;;
    esac
    return 0
}
_ro_rc=0
terminal_adapter_attach_session_readonly "wh@test" || _ro_rc=$?
assert_eq "0" "$_ro_rc" "readonly attach should succeed when session ended naturally"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "ended" "$_status" "readonly attach should report ended when session disappeared"

test_case "readonly attach reports detached on successful cross-server attach"
TMUX="/tmp/tmux-501/default,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
tmux() {
    case "$1" in
        attach) return 0 ;;
        display-message) printf '120x40|120x40\n' ;;
        list-panes) printf '%%1|0\n' ;;
        list-clients) return 0 ;;
    esac
    return 0
}
_ro_rc=0
terminal_adapter_attach_session_readonly "wh@test" || _ro_rc=$?
assert_eq "0" "$_ro_rc" "readonly successful attach should return zero"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "detached" "$_status" "readonly successful attach should report detached"

test_case "readonly attach reports attached on successful same-server switch"
TMUX="/tmp/tmux-501/workhive-workhive-a230,123,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-workhive-a230"
tmux() {
    case "$1" in
        switch-client) return 0 ;;
        display-message) printf '120x40|120x40\n' ;;
        list-panes) printf '%%1|0\n' ;;
        list-clients) return 0 ;;
    esac
    return 0
}
_ro_rc=0
terminal_adapter_attach_session_readonly "wh@test" || _ro_rc=$?
assert_eq "0" "$_ro_rc" "readonly successful switch should return zero"
_status=$(terminal_adapter_get_last_attach_status)
assert_eq "attached" "$_status" "readonly successful switch should report attached"

# ══════════════════════════════════════════════════
# focus-events invariant: dedicated runtime must enable focus-events
# ══════════════════════════════════════════════════

test_case "apply_base_options sets focus-events on as server-global option"
_FOCUS_EVENTS_CALLS=""
tmux() {
    case "$1" in
        set-option)
            _FOCUS_EVENTS_CALLS+="$*"$'\n'
            return 0
            ;;
        display-message) printf '120x40|120x40\n'; return 0 ;;
        list-panes) printf '%%1|0\n'; return 0 ;;
        list-clients) return 0 ;;
        bind-key) return 0 ;;
    esac
    return 0
}
_tmux_apply_base_options "wh@test@main" >/dev/null 2>&1
assert_contains "$_FOCUS_EVENTS_CALLS" "-g focus-events on" "must set focus-events on globally for dedicated runtime"

test_case "apply_base_options sets focus-events before mouse/status options"
_FOCUS_EVENTS_ORDER=""
_FOCUS_EVENTS_SEQ=0
tmux() {
    case "$1" in
        set-option)
            _FOCUS_EVENTS_SEQ=$((_FOCUS_EVENTS_SEQ + 1))
            if [[ "$*" == *"focus-events"* ]]; then
                _FOCUS_EVENTS_ORDER="focus:${_FOCUS_EVENTS_SEQ}"
            elif [[ "$*" == *"mouse"* && -z "${_MOUSE_ORDER:-}" ]]; then
                _MOUSE_ORDER="mouse:${_FOCUS_EVENTS_SEQ}"
            fi
            return 0
            ;;
        display-message) printf '120x40|120x40\n'; return 0 ;;
        list-panes) printf '%%1|0\n'; return 0 ;;
        list-clients) return 0 ;;
        bind-key) return 0 ;;
    esac
    return 0
}
_MOUSE_ORDER=""
_tmux_apply_base_options "wh@test@main" >/dev/null 2>&1
_focus_seq="${_FOCUS_EVENTS_ORDER##*:}"
_mouse_seq="${_MOUSE_ORDER##*:}"
_ordering_ok="false"
if [[ -n "$_focus_seq" && -n "$_mouse_seq" ]] && (( _focus_seq < _mouse_seq )); then
    _ordering_ok="true"
fi
assert_eq "true" "$_ordering_ok" "focus-events must be set before mouse option"
assert_not_empty "$_FOCUS_EVENTS_ORDER" "focus-events set-option call must be recorded"

test_case "normalize interactive state resets pane modes after attach (copy-mode defense)"
_NORMALIZE_CALLS=""
tmux() {
    case "$1" in
        set-option)
            _NORMALIZE_CALLS+="$*"$'\n'
            return 0
            ;;
        list-panes)
            printf '%%1|1\n%%2|0\n'
            return 0
            ;;
        send-keys)
            _NORMALIZE_CALLS+="$*"$'\n'
            return 0
            ;;
        list-clients) return 0 ;;
        display-message) printf '120x40|120x40\n'; return 0 ;;
    esac
    return 0
}
_terminal_tmux_normalize_interactive_state "wh@test@main"
assert_contains "$_NORMALIZE_CALLS" "key-table root" "must reset key-table to root"
assert_contains "$_NORMALIZE_CALLS" "-t %1 -X cancel" "must cancel copy-mode on pane in mode"
assert_not_contains "$_NORMALIZE_CALLS" "-t %2 -X cancel" "must not cancel pane not in mode"

unset TMUX
unset WORKHIVE_TMUX_SERVER_NAME

test_report

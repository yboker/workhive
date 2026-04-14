#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# User-visible list command runtime isolation regression
# Proves workhive list data collection reflects dedicated-runtime
# truth even when same-name sessions exist on another tmux server
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux_runtime.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/domain/services/session_service.sh"

test_suite "list command user-visible runtime isolation"

# ──────────────────────────────────────────────────
# Save tmux_runtime.sh wrapper for restoration between tests
# ──────────────────────────────────────────────────
_SAVED_TMUX_WRAPPER="$(declare -f tmux 2>/dev/null || true)"
_LIST_ISO_DEDICATED_SERVER="workhive-myproject-abc123"
WORKHIVE_TMUX_SERVER_NAME="$_LIST_ISO_DEDICATED_SERVER"

_restore_tmux_wrapper() {
    [[ -n "$_SAVED_TMUX_WRAPPER" ]] && eval "$_SAVED_TMUX_WRAPPER"
}

# ══════════════════════════════════════════════════
# Scenario 1: tmux wrapper always routes through dedicated server
# The tmux_runtime.sh wrapper calls `command tmux -L <server> ...`,
# so we intercept at the `command` level.
# ══════════════════════════════════════════════════

test_case "tmux wrapper sends -L dedicated-server for list-sessions"
_restore_tmux_wrapper
_LIST_ISO_CMD_ARGS=""
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        _LIST_ISO_CMD_ARGS+="$*"$'\n'
        return 0
    fi
    builtin command "$@"
}
tmux list-sessions -F '#{session_name}|#{session_attached}' 2>/dev/null || true
assert_contains "$_LIST_ISO_CMD_ARGS" "-L ${_LIST_ISO_DEDICATED_SERVER}" \
    "list-sessions must route through dedicated runtime server"
unset -f command

test_case "tmux wrapper sends -L dedicated-server for list-panes"
_restore_tmux_wrapper
_LIST_ISO_CMD_ARGS=""
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        _LIST_ISO_CMD_ARGS+="$*"$'\n'
        return 0
    fi
    builtin command "$@"
}
tmux list-panes -a -F '#{session_name}|#{pane_current_path}' 2>/dev/null || true
assert_contains "$_LIST_ISO_CMD_ARGS" "-L ${_LIST_ISO_DEDICATED_SERVER}" \
    "list-panes must route through dedicated runtime server"
unset -f command

# ══════════════════════════════════════════════════
# Scenario 2: session_list_all filters sessions by prefix via dedicated server
# ══════════════════════════════════════════════════

test_case "session_list_all only returns sessions from dedicated server"
_restore_tmux_wrapper

_saved_session_get_prefix="$(declare -f session_get_prefix 2>/dev/null || true)"
_saved_terminal_adapter_init="$(declare -f terminal_adapter_init 2>/dev/null || true)"
_saved_ensure_adapter="$(declare -f _session_ensure_terminal_adapter 2>/dev/null || true)"

session_get_prefix() { echo "wh@abc123@myproject"; }
terminal_adapter_init() { return 0; }
_session_ensure_terminal_adapter() { return 0; }
cache_get() { return 1; }
cache_set() { return 0; }

# Override the tmux wrapper for this test to return controlled data
tmux() {
    case "$1" in
        list-sessions)
            printf 'wh@abc123@myproject@feat1\n'
            printf 'wh@abc123@myproject@main\n'
            printf 'other-session\n'
            return 0
            ;;
    esac
    return 0
}

_result=$(session_list_all)
assert_contains "$_result" "wh@abc123@myproject@feat1" "should include matching session feat1"
assert_contains "$_result" "wh@abc123@myproject@main" "should include matching session main"
assert_not_contains "$_result" "other-session" "should exclude sessions not matching prefix"

[[ -n "$_saved_session_get_prefix" ]] && eval "$_saved_session_get_prefix"
[[ -n "$_saved_terminal_adapter_init" ]] && eval "$_saved_terminal_adapter_init"
[[ -n "$_saved_ensure_adapter" ]] && eval "$_saved_ensure_adapter"

# ══════════════════════════════════════════════════
# Scenario 3: terminal_adapter_list_sessions routes through runtime wrapper
# ══════════════════════════════════════════════════

test_case "terminal_adapter_list_sessions uses tmux wrapper (dedicated server)"
_restore_tmux_wrapper
_LIST_ISO_CMD_FILE=$(mktemp)
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        printf '%s\n' "$*" >> "$_LIST_ISO_CMD_FILE"
        printf 'wh@proj@main\n'
        return 0
    fi
    builtin command "$@"
}

_result=$(terminal_adapter_list_sessions)
_captured=$(cat "$_LIST_ISO_CMD_FILE" 2>/dev/null || true)
rm -f "$_LIST_ISO_CMD_FILE"
assert_contains "$_captured" "-L ${_LIST_ISO_DEDICATED_SERVER}" \
    "adapter list_sessions must go through dedicated runtime tmux wrapper"
assert_contains "$_result" "wh@proj@main" "should return sessions from dedicated server"
unset -f command

# ══════════════════════════════════════════════════
# Scenario 4: has_session queries only dedicated server
# ══════════════════════════════════════════════════

test_case "terminal_adapter_has_session queries only dedicated server"
_restore_tmux_wrapper
_LIST_ISO_CMD_ARGS=""
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        _LIST_ISO_CMD_ARGS+="$*"$'\n'
        return 0
    fi
    builtin command "$@"
}

terminal_adapter_has_session "wh@proj@main" 2>/dev/null || true
assert_contains "$_LIST_ISO_CMD_ARGS" "-L ${_LIST_ISO_DEDICATED_SERVER}" \
    "has_session must query dedicated runtime server"
assert_contains "$_LIST_ISO_CMD_ARGS" "has-session -t wh@proj@main" \
    "has_session must check correct session name"
unset -f command

# ══════════════════════════════════════════════════
# Scenario 5: simulated cross-server same-name isolation
# When `command tmux -L <dedicated>` is called, only return
# dedicated-server sessions. Prove default server sessions
# are structurally invisible.
# ══════════════════════════════════════════════════

test_case "same-name session on default server is invisible to dedicated runtime queries"
_restore_tmux_wrapper
_dedicated_server_sessions="wh@proj@feat1"

command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        local server_flag=""
        local args=("$@")
        local i
        for ((i = 0; i < ${#args[@]}; i++)); do
            if [[ "${args[$i]}" == "-L" && $((i + 1)) -lt ${#args[@]} ]]; then
                server_flag="${args[$((i + 1))]}"
                break
            fi
        done
        if [[ "$server_flag" == "$_LIST_ISO_DEDICATED_SERVER" ]]; then
            local subcmd="${args[2]:-}"
            if [[ "$subcmd" == "list-sessions" ]]; then
                printf '%s\n' "$_dedicated_server_sessions"
                return 0
            fi
        fi
        return 0
    fi
    builtin command "$@"
}

_result=$(terminal_adapter_list_sessions)
assert_contains "$_result" "wh@proj@feat1" "dedicated server session should be visible"
assert_not_contains "$_result" "wh@proj@main" "default server session must not leak into dedicated runtime"
unset -f command

# ══════════════════════════════════════════════════
# Scenario 6: Both bulk tmux queries in list data collection use wrapper
# ══════════════════════════════════════════════════

test_case "bulk tmux queries for list data collection use dedicated server"
_restore_tmux_wrapper
_LIST_ISO_CMD_ARGS=""
command() {
    if [[ "$1" == "tmux" ]]; then
        shift
        _LIST_ISO_CMD_ARGS+="$*"$'\n'
        return 0
    fi
    builtin command "$@"
}

tmux list-sessions -F "#{session_name}|#{session_attached}" 2>/dev/null || true
tmux list-panes -a -F "#{session_name}|#{pane_current_path}" 2>/dev/null || true

_server_flags=$(echo "$_LIST_ISO_CMD_ARGS" | grep -c "\-L ${_LIST_ISO_DEDICATED_SERVER}" || true)
assert_eq "2" "$_server_flags" "both list-sessions and list-panes must use dedicated server"
unset -f command

# ──────────────────────────────────────────────────
# Cleanup
# ──────────────────────────────────────────────────
_restore_tmux_wrapper
unset WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true

test_report

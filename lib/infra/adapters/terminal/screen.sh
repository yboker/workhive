#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Screen Adapter Implementation  [EXPERIMENTAL]
# GNU Screen adapter implementation (experimental, limited features)
#
# Status: EXPERIMENTAL — tmux is the recommended and fully-supported adapter.
# Screen lacks pane management, layout export/restore, and many tmux features.
# This adapter uses graceful degradation (error codes + logs) for unsupported ops.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_TERMINAL_SCREEN_SH_LOADED:-}" ]] && return 0
readonly _TERMINAL_SCREEN_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/base.sh"
[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

_TERMINAL_SCREEN_LAST_ATTACH_STATUS="unknown"

terminal_adapter_name() {
    echo "screen"
}

terminal_adapter_check_available() {
    command -v screen &>/dev/null
}

terminal_adapter_create_session() {
    local session_name="$1"
    local workdir="$2"
    # Use printf %q to safely escape the path
    local safe_dir
    safe_dir=$(printf '%q' "$workdir")
    screen -dmS "$session_name" bash -c "cd ${safe_dir} && exec bash"
}

terminal_adapter_has_session() {
    local session_name="$1"
    screen -list 2>/dev/null | grep -q "\\.${session_name}[[:space:]]"
}

terminal_adapter_attach_session() {
    local session_name="$1"
    _TERMINAL_SCREEN_LAST_ATTACH_STATUS="unknown"
    if screen -r "$session_name"; then
        _TERMINAL_SCREEN_LAST_ATTACH_STATUS="detached"
        return 0
    fi
    _TERMINAL_SCREEN_LAST_ATTACH_STATUS="error"
    return 1
}

terminal_adapter_get_last_attach_status() {
    printf '%s\n' "${_TERMINAL_SCREEN_LAST_ATTACH_STATUS:-unknown}"
}

terminal_adapter_kill_session() {
    local session_name="$1"
    screen -S "$session_name" -X quit 2>/dev/null
}

terminal_adapter_split_vertical() {
    local session_name="$1"
    screen -S "$session_name" -X split -v
}

terminal_adapter_split_horizontal() {
    local session_name="$1"
    screen -S "$session_name" -X split
}

terminal_adapter_send_keys() {
    local session_name="$1"
    local pane_index="$2"  # screen does not support multi-pane indexing, ignored
    local keys="$3"

    screen -S "$session_name" -X stuff "${keys}$(printf '\r')"
}

terminal_adapter_send_text() {
    local session_name="$1"
    local pane_index="$2"  # ignored in screen adapter
    local text="$3"
    local no_enter="${4:-false}"
    if [[ "$no_enter" == "true" ]]; then
        screen -S "$session_name" -X stuff "$text" 2>/dev/null || {
            log_debug "screen adapter: send_text failed for session=${session_name}"
            return 1
        }
        return 0
    fi
    screen -S "$session_name" -X stuff "${text}$(printf '\r')" 2>/dev/null || {
        log_debug "screen adapter: send_text+enter failed for session=${session_name}"
        return 1
    }
    return 0
}

terminal_adapter_get_last_send_meta() {
    echo "client=generic mode=plain commit=enter cache_hit=0"
}

terminal_adapter_list_sessions() {
    screen -list 2>/dev/null | grep -o '[0-9]*\.[^[:space:]]*' | cut -d'.' -f2 || echo ""
}

terminal_adapter_apply_config() {
    :
}

# -- Graceful degradation methods (Screen does not support, return safe defaults) --

terminal_adapter_count_panes() {
    # Screen cannot accurately count regions, return 1 (default single window)
    echo "1"
}

terminal_adapter_get_current_session() {
    # Screen has no direct method to get the current session name
    local _sty="${STY:-}"
    echo "${_sty#*.}"
}

terminal_adapter_get_current_pane_index() {
    echo "0"
}

terminal_adapter_get_pane_command() {
    echo "bash"
}

terminal_adapter_get_pane_cwd() {
    pwd
}

terminal_adapter_attach_session_readonly() {
    local session_name="$1"
    log_warn "screen adapter: read-only mode not supported, attaching normally"
    screen -r "$session_name"
}

terminal_adapter_select_pane() {
    local session_name="$1"
    # Screen region selection: use the focus command
    screen -S "$session_name" -X focus 2>/dev/null || true
}

terminal_adapter_resize_pane() {
    log_debug "screen adapter: precise pane resize not supported"
    return 1
}

terminal_adapter_get_session_info() {
    local session_name="$1"
    local safe_name
    safe_name=$(json_escape "$session_name")
    echo "{\"name\":\"$safe_name\",\"adapter\":\"screen\"}"
}

terminal_adapter_kill_pane() {
    # Close the current region in Screen
    screen -X remove 2>/dev/null || return 1
}

terminal_adapter_split_current_pane() {
    # shellcheck disable=SC2034
    local percentage="${1:-50}"
    local cmd="${2:-}"
    screen -X split 2>/dev/null || return 1
    screen -X focus 2>/dev/null || true
    screen -X screen 2>/dev/null || true
    if ! _is_null_or_empty "$cmd"; then
        screen -X stuff "${cmd}$(printf '\r')" 2>/dev/null || true
    fi
}

terminal_adapter_get_layout_string() {
    # Screen does not support layout string export
    echo ""
}


terminal_adapter_get_window_count() {
    local session_name="$1"
    echo "1"  # screen doesn't have multi-window concept like tmux
}

terminal_adapter_is_session_attached() {
    local session_name="$1"
    screen -list 2>/dev/null | grep -q "${session_name}.*Attached"
}

terminal_adapter_get_window_size() {
    local session_name="$1"
    echo "80x24"  # screen doesn't easily expose window size
}

terminal_adapter_resize_window() {
    # screen doesn't support programmatic window resize
    return 0
}

terminal_adapter_list_panes_format() {
    # screen doesn't support pane listing in tmux format
    log_debug "screen adapter: list_panes_format not supported"
    return 1
}

terminal_adapter_set_after_kill_pane_hook() {
    # screen doesn't support after-kill-pane hooks
    return 0
}

terminal_adapter_run_shell() {
    local cmd="$1"
    local rc=0
    bash -c "$cmd" 2>/dev/null || rc=$?
    if [[ $rc -ne 0 ]]; then
        log_debug "screen adapter: run_shell exited with code $rc: ${cmd:0:120}"
    fi
    return $rc
}

terminal_adapter_kill_pane_target() {
    # Screen does not support targeted pane kill
    log_debug "screen adapter: kill_pane_target not supported"
    return 1
}

terminal_adapter_resolve_pane_target() {
    # Screen does not support pane id resolution
    log_debug "screen adapter: resolve_pane_target not supported"
    return 1
}

terminal_adapter_display_message() {
    # Screen does not support display-message
    return 0
}

terminal_adapter_set_option() {
    # Screen options managed via ~/.screenrc
    return 0
}

terminal_adapter_rename_session() {
    local session_name="$1"
    local new_name="$2"
    screen -S "$session_name" -X sessionname "$new_name" 2>/dev/null || return 1
}

terminal_adapter_select_layout() {
    # Screen does not support layout selection
    return 0
}

terminal_adapter_detach_session() {
    screen -d 2>/dev/null || return 1
}

terminal_adapter_new_window() {
    local session_name="$1"
    local window_name="$2"
    local workdir="$3"
    screen -S "$session_name" -X screen -t "$window_name" 2>/dev/null || return 1
}

terminal_adapter_rename_window() {
    local session_name="$1"
    local window_index="$2"
    local new_name="$3"
    screen -S "$session_name" -X title "$new_name" 2>/dev/null || return 1
}

terminal_adapter_select_window() {
    local session_name="$1"
    local window_index="$2"
    screen -S "$session_name" -X select "$window_index" 2>/dev/null || return 1
}

terminal_adapter_send_keys_to_window() {
    local session_name="$1"
    local window_index="$2"
    # shellcheck disable=SC2034
    local pane_index="$3"
    local keys="$4"
    screen -S "$session_name" -X stuff "${keys}$(printf '\r')" 2>/dev/null || return 1
}

terminal_adapter_spawn_command_target() {
    local target="$1"
    local command="$2"
    # shellcheck disable=SC2034
    local workdir="${3:-}"
    # screen adapter has no robust non-echo pane-respawn equivalent.
    # Return failure so caller can fallback to send-keys strategy.
    return 1
}

terminal_adapter_reflow_session_panes() {
    # screen has no pane-level geometry API compatible with tmux reflow.
    return 0
}

terminal_adapter_list_windows() {
    local session_name="$1"
    # Screen doesn't support structured window listing
    echo ""
}

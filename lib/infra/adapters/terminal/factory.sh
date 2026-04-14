#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Terminal Adapter Factory
# Factory pattern - select adapter based on platform and configuration
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_TERMINAL_FACTORY_SH_LOADED:-}" ]] && return 0
readonly _TERMINAL_FACTORY_SH_LOADED=1

source "${LIB_DIR}/infra/platform/detector.sh"
[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

# Currently loaded adapter
CURRENT_TERMINAL_ADAPTER=""

# Initialize terminal adapter (must be called in the main shell)
terminal_adapter_init() {
    # If already loaded, return immediately
    if [[ -n "$CURRENT_TERMINAL_ADAPTER" ]]; then
        return 0
    fi

    local platform
    platform=$(platform_detect)
    local adapter_type="${WORKHIVE_TERMINAL_ADAPTER:-auto}"

    # Auto-detection mode
    if [[ "$adapter_type" == "auto" ]]; then
        if platform_supports_feature "tmux" "$platform"; then
            adapter_type="tmux"
        else
            adapter_type="screen"
        fi
    fi

    # Load the corresponding adapter
    local adapter_file="${LIB_DIR}/infra/adapters/terminal/${adapter_type}.sh"

    if [[ ! -f "$adapter_file" ]]; then
        printf '%s\n' "$(_i18n_fmt ERR_ADAPTER_UNSUPPORTED "$adapter_type")" >&2
        return 1
    fi

    # shellcheck source=/dev/null
    source "$adapter_file"

    # Check if the adapter is available
    if ! terminal_adapter_check_available; then
        printf '%s\n' "$(_i18n_fmt ERR_ADAPTER_NOT_AVAILABLE "$adapter_type")" >&2
        return 1
    fi

    # Verify the adapter implements all required interface methods
    if ! _terminal_adapter_verify_contract "$adapter_type"; then
        return 1
    fi

    if [[ "$adapter_type" == "screen" ]]; then
        log_warn "Screen adapter is EXPERIMENTAL. Many features (pane management, layout, etc.) are limited. tmux is strongly recommended."
    fi

    CURRENT_TERMINAL_ADAPTER="$adapter_type"
    return 0
}

# Verify the adapter implements all required interface methods
_terminal_adapter_verify_contract() {
    local adapter_type="$1"
    local -a required_methods=(
        "terminal_adapter_name"
        "terminal_adapter_check_available"
        "terminal_adapter_create_session"
        "terminal_adapter_has_session"
        "terminal_adapter_attach_session"
        "terminal_adapter_kill_session"
        "terminal_adapter_split_vertical"
        "terminal_adapter_split_horizontal"
        "terminal_adapter_send_keys"
        "terminal_adapter_apply_config"
        "terminal_adapter_list_sessions"
        "terminal_adapter_count_panes"
        "terminal_adapter_get_current_session"
        "terminal_adapter_rename_session"
    )

    local -a optional_methods=(
        "terminal_adapter_get_current_pane_index"
        "terminal_adapter_get_pane_command"
        "terminal_adapter_get_pane_cwd"
        "terminal_adapter_attach_session_readonly"
        "terminal_adapter_select_pane"
        "terminal_adapter_resize_pane"
        "terminal_adapter_get_session_info"
        "terminal_adapter_kill_pane"
        "terminal_adapter_split_current_pane"
        "terminal_adapter_get_layout_string"
        "terminal_adapter_get_window_count"
        "terminal_adapter_is_session_attached"
        "terminal_adapter_get_window_size"
        "terminal_adapter_resize_window"
        "terminal_adapter_list_panes_format"
        "terminal_adapter_set_after_kill_pane_hook"
        "terminal_adapter_run_shell"
        "terminal_adapter_display_message"
        "terminal_adapter_set_option"
        "terminal_adapter_get_version"
        "terminal_adapter_spawn_command_target"
    )

    local missing=()
    local method
    for method in "${required_methods[@]}"; do
        if ! declare -f "$method" &>/dev/null; then
            missing+=("$method")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        log_error "Adapter '$adapter_type' missing required methods: ${missing[*]}"
        return 1
    fi

    local missing_optional=()
    local method
    for method in "${optional_methods[@]}"; do
        if ! declare -f "$method" &>/dev/null; then
            missing_optional+=("$method")
        fi
    done

    if [[ ${#missing_optional[@]} -gt 0 ]]; then
        log_debug "Adapter '$adapter_type' missing optional methods (using defaults): ${missing_optional[*]}"
    fi

    return 0
}

# Get terminal adapter type
terminal_adapter_get() {
    # If not initialized, try to initialize (but calling this in a subshell won't affect the parent shell)
    if [[ -z "$CURRENT_TERMINAL_ADAPTER" ]]; then
        terminal_adapter_init >/dev/null
    fi
    echo "$CURRENT_TERMINAL_ADAPTER"
}

# Reset adapter (for testing)
terminal_adapter_reset() {
    CURRENT_TERMINAL_ADAPTER=""
}

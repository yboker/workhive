#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Interactive Utilities
# Shared helpers for interactive form input/echo patterns
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_INTERACTIVE_SH_LOADED:-}" ]] && return 0
readonly _INTERACTIVE_SH_LOADED=1

interactive_trim_label_suffix() {
    local label="${1:-}"
    # Trim trailing spaces and common prompt suffix chars (: / ：)
    label="${label%"${label##*[![:space:]]}"}"
    while [[ "$label" == *: || "$label" == *： ]]; do
        label="${label%?}"
        label="${label%"${label##*[![:space:]]}"}"
    done
    printf '%s' "$label"
}

interactive_read_line() {
    local prompt="$1"
    local __var_name="$2"
    local default_value="${3:-}"
    local input=""
    local rc=0

    if [[ -n "$default_value" ]]; then
        if ! read -r -e -i "$default_value" -p "$prompt " input < /dev/tty; then
            rc=$?
            echo ""
            (( rc >= 128 )) && return 130
            return 1
        fi
    else
        if ! read -r -p "$prompt " input < /dev/tty; then
            rc=$?
            echo ""
            (( rc >= 128 )) && return 130
            return 1
        fi
    fi

    printf -v "$__var_name" '%s' "$input"
    return 0
}

interactive_echo_field() {
    local label="$1"
    local value="$2"
    local clean_label
    clean_label=$(interactive_trim_label_suffix "$label")
    log_info "$clean_label: $value"
}

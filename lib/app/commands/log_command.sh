#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Log Command
# View workhive operation logs (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_LOG_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _LOG_COMMAND_SH_LOADED=1

source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"

log_command_help() {
    _help_dispatch "_log_command_help"
}

_log_level_pattern() {
    local level="$1"
    case "$level" in
        debug) echo '\[DEBUG\]' ;;
        info)  echo '\[INFO \]' ;;
        warn)  echo '\[WARN \]' ;;
        error) echo '\[ERROR\]' ;;
        *)     echo "" ;;
    esac
}

_log_stream_with_level_filter() {
    local log_file="$1"
    local lines="$2"
    local level="$3"
    local follow="$4"

    local pattern
    pattern=$(_log_level_pattern "$level")

    tail -n "$lines" "$log_file" | grep -E "$pattern" || true
    if [[ "$follow" == true ]]; then
        tail -n 0 -f "$log_file" 2>/dev/null | grep --line-buffered -E "$pattern" || true
    fi
}

log_command_execute() {
    local follow=false
    local lines=50
    local clear_mode=false
    local level=""
    local log_file

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                log_command_help
                return 0
                ;;
            --follow)
                follow=true
                shift
                ;;
            --lines)
                local lines_value=""
                lines_value=$(cli_require_option_value_nonflag "--lines" "${2:-}") || return 1
                if [[ ! "$lines_value" =~ ^[0-9]+$ ]] || [[ "$lines_value" -lt 1 ]]; then
                    log_error "$(_i18n_fmt LOG_ERR_INVALID_LINES_FMT "$lines_value")"
                    return 1
                fi
                lines="$lines_value"
                shift 2
                ;;
            --clear)
                clear_mode=true
                shift
                ;;
            --level)
                local level_value=""
                level_value=$(cli_require_option_value_nonflag "--level" "${2:-}") || return 1
                if [[ ! "$level_value" =~ ^(debug|info|warn|error)$ ]]; then
                    log_error "$(_i18n_fmt LOG_ERR_INVALID_LEVEL_FMT "$level_value")"
                    return 1
                fi
                level="$level_value"
                shift 2
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                log_command_help
                return 1
                ;;
            *)
                cli_log_no_positional "log"
                echo ""
                log_command_help
                return 1
                ;;
        esac
    done

    log_file="$(_workhive_dir 2>/dev/null)/logs/workhive.log"

    if [[ ! -f "$log_file" ]]; then
        return 0
    fi

    if [[ "$clear_mode" == true ]]; then
        : > "$log_file"
        log_success "$(_i18n LOG_FILE_CLEARED)"
        return 0
    fi

    if [[ -n "$level" ]]; then
        _log_stream_with_level_filter "$log_file" "$lines" "$level" "$follow"
        return 0
    fi

    if [[ "$follow" == true ]]; then
        tail -n "$lines" -f "$log_file"
        return 0
    fi

    tail -n "$lines" "$log_file"
}

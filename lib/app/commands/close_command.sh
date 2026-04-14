#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Close Command
# Close current workspace and return to terminal (App layer)
# Default: detach only (session preserved)
# Optional: terminate session (destroy all panes)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard
[[ -n "${_CLOSE_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _CLOSE_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/domain/services/session_service.sh"

# Help
close_command_help() {
    _help_dispatch "_close_command_help"
}

# Execute close command
close_command_execute() {
    local force=false
    local kill_session=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                close_command_help
                return 0
                ;;
            --force|-f)
                force=true
                shift
                ;;
            --kill)
                kill_session=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                close_command_help
                return 1
                ;;
            *)
                cli_log_no_positional "close"
                return 1
                ;;
        esac
    done

    # Must be inside tmux
    if [[ -z "${TMUX:-}" ]]; then
        log_error "$(_i18n CLOSE_NOT_IN_TMUX)"
        return 1
    fi

    # Get current session
    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null || true)

    if [[ -z "$current_session" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    local tag
    tag=$(config_get "tmux.session_prefix" 2>/dev/null || true)
    _is_null_or_empty "$tag" && tag="wh"
    tag=$(_sanitize_session_prefix_tag "$tag")
    if [[ "$current_session" != "${tag}@"* ]]; then
        log_error "$(_i18n CLOSE_NOT_WORKHIVE_SESSION)"
        return 1
    fi

    local branch
    branch=$(session_extract_branch "$current_session")

    # Determine action
    if [[ "$force" == true && "$kill_session" != true ]]; then
        # --force without --kill: detach immediately
        _close_do_detach "$branch"
        return $?
    fi

    if [[ "$kill_session" == true ]]; then
        # --kill mode: confirm then kill
        if [[ "$force" != true ]]; then
            local confirm_msg
            confirm_msg=$(_i18n_fmt CLOSE_CONFIRM "$branch")
            echo ""
            if ! ask_yes_no "$confirm_msg" "N"; then
                log_info "$(_i18n CLOSE_CANCELLED)"
                return 0
            fi
            echo ""
        fi
        _close_do_kill "$branch" "$current_session"
        return $?
    fi

    # Interactive mode: TUI select
    echo ""
    # shellcheck disable=SC2034
    local close_options=(
        "$(_i18n CLOSE_OPT_DETACH)"
        "$(_i18n CLOSE_OPT_KILL)"
    )
    tui_select_menu "$(_i18n CLOSE_SELECT_ACTION)" close_options 0
    echo ""

    case "$TUI_SELECTED_INDEX" in
        0)
            _close_do_detach "$branch"
            ;;
        1)
            _close_do_kill "$branch" "$current_session"
            ;;
        *)
            log_info "$(_i18n CLOSE_CANCELLED)"
            return 0
            ;;
    esac
}

_close_do_detach() {
    local branch="$1"

    log_info "$(_i18n CLOSE_DETACHING) $branch"

    session_detach

    log_success "$(_i18n CLOSE_DETACH_SUCCESS) $branch"
    log_info "$(_i18n_fmt CLOSE_DETACH_HINT "$branch")"
    return 0
}

_close_do_kill() {
    local branch="$1"
    local current_session="$2"

    log_info "$(_i18n CLOSE_CLOSING) $branch"

    # When close runs inside the target session, kill can interrupt the current
    # CLI process before follow-up steps complete. Detach first to make the kill
    # path deterministic for interactive "terminate session" mode.
    session_detach 2>/dev/null || true

    if ! session_kill_by_name "$current_session" "$branch"; then
        log_error "$(_i18n ERR_SESSION_KILL_FAILED)"
        return 1
    fi

    cache_invalidate "session:*"

    log_success "$(_i18n CLOSE_SUCCESS) $branch"
    return 0
}

#!/usr/bin/env bash
[[ -n "${_RENAME_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _RENAME_COMMAND_SH_LOADED=1

source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/infra/adapters/terminal/factory.sh"
source "${LIB_DIR}/infra/cache/manager.sh"

_RN_TX_BRANCH_RENAMED=false
_RN_TX_WORKTREE_MOVED=false
_RN_TX_SESSION_RENAMED=false
_RN_TX_COMPLETED=false
_RN_TX_OLD_BRANCH=""
_RN_TX_NEW_BRANCH=""
_RN_TX_OLD_PATH=""
_RN_TX_NEW_PATH=""
_RN_TX_OLD_SESSION=""
_RN_TX_NEW_SESSION=""

_rename_rollback() {
    if [[ "$_RN_TX_COMPLETED" == true ]]; then
        return 0
    fi
    log_warn "$(_i18n RENAME_ROLLBACK)"
    if [[ "$_RN_TX_SESSION_RENAMED" == true ]]; then
        terminal_adapter_rename_session "$_RN_TX_NEW_SESSION" "$_RN_TX_OLD_SESSION" 2>/dev/null || true
    fi
    if [[ "$_RN_TX_WORKTREE_MOVED" == true ]]; then
        local git_root
        git_root=$(_git_root)
        (cd "$git_root" && git worktree move "$_RN_TX_NEW_PATH" "$_RN_TX_OLD_PATH") 2>/dev/null || true
    fi
    if [[ "$_RN_TX_BRANCH_RENAMED" == true ]]; then
        local git_root
        git_root=$(_git_root)
        (cd "$git_root" && git branch -m "$_RN_TX_NEW_BRANCH" "$_RN_TX_OLD_BRANCH") 2>/dev/null || true
    fi
}

_rename_validate_branch() {
    local branch="$1"
    if [[ -z "$branch" ]]; then
        log_error "$(_i18n ERR_BRANCH_EMPTY)"
        return 1
    fi
    if ! git check-ref-format --branch "$branch" &>/dev/null; then
        log_error "$(_i18n RENAME_ERR_INVALID_REF) $branch"
        return 1
    fi
    return 0
}

rename_command_help() {
    _help_dispatch "_rename_command_help"
}

rename_command_execute() {
    local old_branch=""
    local new_branch=""
    local force=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                rename_command_help
                return 0
                ;;
            --force|-f)
                force=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                rename_command_help
                return 1
                ;;
            *)
                if [[ -z "$old_branch" ]]; then
                    old_branch="$1"
                elif [[ -z "$new_branch" ]]; then
                    new_branch="$1"
                else
                    log_error "$(_i18n RENAME_ERR_TOO_MANY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    if [[ -z "$old_branch" || -z "$new_branch" ]]; then
        log_error "$(_i18n RENAME_ERR_BOTH_REQUIRED)"
        echo ""
        rename_command_help
        return 1
    fi

    if ! _rename_validate_branch "$old_branch" || ! _rename_validate_branch "$new_branch"; then
        return 1
    fi

    # Branch name sanitization: warn and confirm if new name will be converted
    if _branch_needs_sanitize "$new_branch"; then
        if ! _branch_confirm_sanitize "$new_branch"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    if [[ "$old_branch" == "$new_branch" ]]; then
        log_error "$(_i18n RENAME_ERR_SAME)"
        return 1
    fi

    local main_branch
    main_branch=$(_git_main_branch)
    if [[ "$old_branch" == "$main_branch" ]]; then
        log_error "$(_i18n_fmt RENAME_ERR_MAIN_FMT "$main_branch")"
        return 1
    fi

    if ! worktree_check_repo; then
        return 1
    fi

    if ! git rev-parse --verify "$old_branch" &>/dev/null; then
        log_error "$(_i18n ERR_WT_NOT_FOUND) $old_branch (branch does not exist)"
        return 1
    fi

    if git rev-parse --verify "$new_branch" &>/dev/null; then
        log_error "$(_i18n_fmt RENAME_ERR_NEW_EXISTS_FMT "$new_branch")"
        return 1
    fi

    if worktree_exists "$new_branch"; then
        log_error "$(_i18n_fmt RENAME_ERR_WT_NEW_EXISTS_FMT "$new_branch")"
        return 1
    fi

    if ! worktree_exists "$old_branch"; then
        log_error "$(_i18n_fmt RENAME_ERR_WT_OLD_MISSING_FMT "$old_branch")"
        return 1
    fi

    local old_session new_session
    old_session=$(session_get_name "$old_branch")
    new_session=$(session_get_name "$new_branch")

    if session_exists "$old_session"; then
        if terminal_adapter_is_session_attached "$old_session" 2>/dev/null && [[ "$force" != true ]]; then
            log_error "$(_i18n_fmt RENAME_ERR_ATTACHED_FMT "$old_session")"
            return 1
        fi
    fi

    local old_path new_path
    old_path=$(worktree_get_path "$old_branch")
    new_path=$(worktree_get_path "$new_branch")
    op_begin "rename_workspace" "$old_branch" "to=$new_branch force=$force"

    _RN_TX_BRANCH_RENAMED=false
    _RN_TX_WORKTREE_MOVED=false
    _RN_TX_SESSION_RENAMED=false
    _RN_TX_STATE_RENAMED=false
    _RN_TX_COMPLETED=false
    _RN_TX_OLD_BRANCH="$old_branch"
    _RN_TX_NEW_BRANCH="$new_branch"
    _RN_TX_OLD_PATH="$old_path"
    _RN_TX_NEW_PATH="$new_path"
    _RN_TX_OLD_SESSION="$old_session"
    _RN_TX_NEW_SESSION="$new_session"

    # Keep rollback bound to EXIT/ERR, so Ctrl+C keeps default interruption
    # semantics and does not get swallowed by command-local traps.
    trap '_rename_rollback' EXIT ERR

    log_bold "$(_i18n_fmt RENAME_TITLE_FMT "$old_branch" "$new_branch")"
    echo ""

    if ! confirm_action "$(_i18n RENAME_CONFIRM)" "$force" "N" "1"; then
        trap - EXIT ERR
        if [[ "${_ASK_YES_NO_NONINTERACTIVE:-false}" == true ]]; then
            return 1
        fi
        return 0
    fi

    echo ""

    local git_root
    git_root=$(_git_root)

    log_info "$(_i18n RENAME_STEP_BRANCH)"
    op_begin "rename_branch" "$old_branch" "to=$new_branch"
    if ! (cd "$git_root" && git branch -m "$old_branch" "$new_branch"); then
        log_error "$(_i18n RENAME_ERR_BRANCH_FAIL)"
        op_fail "rename_branch" "$old_branch" 1 "to=$new_branch"
        op_fail "rename_workspace" "$old_branch" 1 "step=rename_branch to=$new_branch"
        trap - EXIT ERR
        return 1
    fi
    op_ok "rename_branch" "$old_branch" "to=$new_branch"
    _RN_TX_BRANCH_RENAMED=true

    log_info "$(_i18n RENAME_STEP_WORKTREE)"
    op_begin "rename_worktree_path" "$old_path" "to=$new_path"
    if ! (cd "$git_root" && git worktree move "$old_path" "$new_path"); then
        log_error "$(_i18n RENAME_ERR_WT_FAIL)"
        op_fail "rename_worktree_path" "$old_path" 1 "to=$new_path"
        op_fail "rename_workspace" "$old_branch" 1 "step=rename_worktree_path"
        _rename_rollback
        trap - EXIT ERR
        return 1
    fi
    op_ok "rename_worktree_path" "$old_path" "to=$new_path"
    _RN_TX_WORKTREE_MOVED=true

    log_info "$(_i18n RENAME_STEP_SESSION)"
    if session_exists "$old_session"; then
        if [[ "$force" == true ]]; then
            # Force rename must avoid carrying over stale long-running pane processes.
            log_warn "$(_i18n MSG_FORCE_DESTROY_SESSION)"
            op_begin "rename_session_force_kill" "$old_session" "branch=$old_branch"
            if ! session_kill_by_name "$old_session" "$old_branch" 2>/dev/null; then
                log_error "$(_i18n RENAME_ERR_SESSION_FAIL)"
                op_fail "rename_session_force_kill" "$old_session" 1
                op_fail "rename_workspace" "$old_branch" 1 "step=rename_session_force_kill"
                _rename_rollback
                trap - EXIT ERR
                return 1
            fi
            op_ok "rename_session_force_kill" "$old_session"
        else
            op_begin "rename_session" "$old_session" "to=$new_session"
            if ! terminal_adapter_rename_session "$old_session" "$new_session" 2>/dev/null; then
                log_error "$(_i18n RENAME_ERR_SESSION_FAIL)"
                op_fail "rename_session" "$old_session" 1 "to=$new_session"
                op_fail "rename_workspace" "$old_branch" 1 "step=rename_session"
                _rename_rollback
                trap - EXIT ERR
                return 1
            fi
            op_ok "rename_session" "$old_session" "to=$new_session"
            _RN_TX_SESSION_RENAMED=true
        fi
    else
        log_info "$(_i18n RENAME_NO_SESSION)"
    fi

    _RN_TX_COMPLETED=true
    trap - EXIT ERR

    cache_invalidate "worktree:*"
    cache_invalidate "session:*"

    echo ""
    log_success "$(_i18n_fmt RENAME_SUCCESS_FMT "$old_branch" "$new_branch")"
    op_ok "rename_workspace" "$old_branch" "to=$new_branch"
}

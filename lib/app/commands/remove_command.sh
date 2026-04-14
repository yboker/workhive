#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Remove Command
# Completely remove Worktree (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_REMOVE_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _REMOVE_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/app/shared/worktree_collector.sh"
source "${LIB_DIR}/app/shared/worktree_selector.sh"
source "${LIB_DIR}/app/shared/worktree_safety.sh"

# Display usage help
remove_command_help() {
    _help_dispatch "_remove_command_help"
}

# Execute remove command
# Arguments: $@ = command line arguments
remove_command_execute() {
    local branch=""
    local keep_worktree=""
    local force=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                remove_command_help
                return 0
                ;;
            --keep-worktree)
                keep_worktree=true
                shift
                ;;
            --remove-worktree)
                keep_worktree=false
                shift
                ;;
            --force|-f)
                force=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                remove_command_help
                return 1
                ;;
            *)
                if [[ -z "$branch" ]]; then
                    branch="$1"
                else
                    log_error "$(_i18n ERR_ONE_BRANCH_ONLY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    # No arguments: interactive selection
    if [[ -z "$branch" ]]; then
        local select_rc=0
        branch=$(_remove_command_interactive_select) || select_rc=$?
        if [[ $select_rc -ne 0 || -z "$branch" ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    # Branch name sanitization: warn and confirm if name will be converted
    if _branch_needs_sanitize "$branch"; then
        if ! _branch_confirm_sanitize "$branch"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    log_bold "🗑️  $(_i18n TITLE_REMOVE_WT) $branch"
    op_begin "remove_workspace" "$branch" "force=$force"
    echo ""

    # Determine whether to delete Worktree
    local should_delete=false

    if [[ -n "$keep_worktree" ]]; then
        # User explicitly specified
        should_delete=$([[ "$keep_worktree" == false ]] && echo true || echo false)
    else
        # Read default behavior from config
        local auto_delete
        auto_delete=$(config_get "worktree.auto_delete")
        [[ "$auto_delete" == "true" ]] && should_delete=true || should_delete=false
    fi

    # ──────────────────────────────────────────────────
    # Check status
    # ──────────────────────────────────────────────────
    local normalized_branch
    normalized_branch=$(_sanitize_branch_name "$branch")
    local branch_sessions
    branch_sessions=$(session_list_project_family_by_branch "$branch" 2>/dev/null || true)

    local has_session=false
    local has_worktree=false

    if [[ -n "$branch_sessions" ]]; then
        has_session=true
    fi

    if worktree_exists "$branch"; then
        has_worktree=true
    fi

    if [[ "$should_delete" == true && "$has_worktree" == false ]]; then
        log_warn "$(_i18n ERR_WT_NOT_FOUND) $branch"
        echo ""

        if [[ "$has_session" == false ]]; then
            log_info "$(_i18n MSG_AVAILABLE_SPACES)"

            local available_sessions
            available_sessions=$(session_list_project_family)

            if [[ -z "$available_sessions" ]]; then
                echo "  $(_i18n MSG_NO_ACTIVE_SESSIONS)"
            else
                echo "$available_sessions" | while read -r session; do
                    local branch_name
                    branch_name=$(session_extract_branch "$session")
                    echo "  • $branch_name"
                done
            fi
        fi

        op_fail "remove_workspace" "$branch" 1 "worktree_missing"
        return 1
    fi

    local remove_confirmed=false
    if [[ "$should_delete" == true && "$has_worktree" == true ]]; then
        if ! confirm_action "$(_i18n CONFIRM_REMOVE_WT)" "$force" "N" "1"; then
            if [[ "${_ASK_YES_NO_NONINTERACTIVE:-false}" == true ]]; then
                return 1
            fi
            return 0
        fi
        remove_confirmed=true
    fi

    # ──────────────────────────────────────────────────
    # Step 1: Close session
    # ──────────────────────────────────────────────────
    if [[ "$has_session" == true ]]; then
        log_info "$(_i18n MSG_REMOVE_STEP_1)"
        op_begin "remove_session" "$normalized_branch"

        # Confirm before killing session when this command is acting like close-only
        if [[ "$force" == false && "$remove_confirmed" == false ]]; then
            if ! ask_yes_no "$(_i18n CONFIRM_KILL_SESSION)" "N"; then
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
        fi

        if session_cleanup_project_family_by_branch "$branch" "false"; then
            op_ok "remove_session" "$normalized_branch" "killed=${SESSION_CLEANUP_CLOSED:-0}"
        else
            log_warn "$(_i18n MSG_SESSION_CLOSE_FAILED)"
            op_fail "remove_session" "$normalized_branch" 1
            log_error "$(_i18n REMOVE_ERR_SESSIONS_REMAIN)"
            echo "${SESSION_CLEANUP_REMAINING:-}" | while IFS= read -r _left; do
                [[ -z "$_left" ]] && continue
                echo "  • $_left"
            done
            log_info "$(_i18n REMOVE_INFO_RETRY_FORCE)"
            op_fail "remove_session" "$normalized_branch" 1 "sessions_remain=true"
            return 1
        fi
        echo ""
    else
        log_info "$(_i18n MSG_SESSION_NOT_EXIST)"
        echo ""
    fi

    # ──────────────────────────────────────────────────
    # Step 2: Remove Worktree (optional)
    # ──────────────────────────────────────────────────
    if [[ "$has_worktree" == false ]]; then
        log_info "$(_i18n MSG_WT_NOT_EXIST_SKIP)"
        log_success "$(_i18n MSG_DESTROY_DONE)"
        op_ok "remove_workspace" "$branch" "worktree_skipped=true"
        return 0
    fi

    if [[ "$should_delete" == false ]]; then
        log_info "$(_i18n MSG_KEEP_WT)"
        local wt_path
        wt_path=$(worktree_get_path "$branch")
        log_info "$(_i18n MSG_WT_PATH) $wt_path"
        log_success "$(_i18n MSG_DESTROY_DONE)"
        op_ok "remove_workspace" "$branch" "keep_worktree=true"
        return 0
    fi

    # Remove Worktree
    log_info "$(_i18n MSG_REMOVE_WT)"

    local wt_path
    wt_path=$(worktree_get_path "$branch")

    # Unified safety check: dirty state + unmerged commits (unless force mode)
    if [[ "$force" == false ]]; then
        if [[ ! -d "$wt_path" ]]; then
            log_error "$(_i18n ERR_CANNOT_ACCESS_WT) $wt_path"
            return 1
        fi

        local git_root
        git_root=$(_git_root)

        _worktree_safety_scan "$git_root" "" "$branch"

        if [[ "$_WT_SAFETY_HAS_WARNINGS" == true ]]; then
            echo ""
            _worktree_safety_print_warning_box
            if ! confirm_action "$(_i18n CONFIRM_DESTROY_WT)" "$force" "N"; then
                return 0
            fi
            force=true
        fi
    fi

    # Execute deletion
    if worktree_remove "$branch" "$force"; then
        op_ok "remove_worktree" "$branch" "force=$force"
        : # worktree_remove already logs success
    else
        log_error "$(_i18n ERR_WT_REMOVE_FAILED)"
        op_fail "remove_worktree" "$branch" 1 "force=$force"
        op_fail "remove_workspace" "$branch" 1 "step=remove_worktree"
        if [[ "$force" == false ]]; then
            log_info "$(_i18n MSG_TRY_FORCE)"
            echo "  workhive remove $branch --force"
        fi
        return 1
    fi

    # ──────────────────────────────────────────────────
    # Step 3: Delete branch (full cleanup)
    # ──────────────────────────────────────────────────
    echo ""
    log_info "$(_i18n MSG_REMOVE_STEP_3)"

    local _branch_delete_failed=false
    local git_root
    git_root=$(_git_root)

    if (cd "$git_root" && git branch -D "$branch") &>/dev/null; then
        log_success "$(_i18n MSG_BRANCH_DELETED) $branch"
        op_ok "remove_branch_ref" "$branch"
    else
        if ! (cd "$git_root" && git rev-parse --verify "$branch") &>/dev/null; then
            log_debug "$(_i18n DEBUG_BRANCH_NOT_EXISTS) $branch"
        else
            log_warn "$(_i18n WARN_BRANCH_DELETE_FAILED)"
            log_info "$(_i18n INFO_CHECK_BRANCH) $branch"
            op_fail "remove_branch_ref" "$branch" 1
            _branch_delete_failed=true
        fi
    fi

    echo ""
    if [[ "$_branch_delete_failed" == true ]]; then
        log_warn "$(_i18n MSG_DESTROY_DONE) (branch delete failed)"
        op_fail "remove_workspace" "$branch" 2 "branch_delete_failed"
        return 2
    fi
    log_success "$(_i18n MSG_DESTROY_DONE)"
    op_ok "remove_workspace" "$branch"
}

# Interactive worktree selection for remove (when no argument given)
_remove_command_interactive_select() {
    worktree_interactive_select \
        --title "$(_i18n MSG_SELECT_REMOVE)"
}

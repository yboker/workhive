#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Destroy Command
# Completely remove all workhive resources (App layer)
#
# Behavior: close all sessions -> delete all Worktrees -> delete .workhive/ directory
# This is an irreversible operation requiring explicit user confirmation.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_DESTROY_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _DESTROY_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/app/shared/worktree_safety.sh"

# Show usage help
destroy_command_help() {
    _help_dispatch "_destroy_command_help"
}

# Delete global configuration
_destroy_global_config() {
    local force="${1:-false}"
    local global_dir
    global_dir=$(config_get_global_dir)
    local global_file
    global_file=$(config_get_global_file)

    if [[ ! -d "$global_dir" ]] && [[ ! -f "$global_file" ]]; then
        log_info "$(_i18n INFO_NO_GLOBAL_CONFIG)"
        log_info "  Location: $global_file"
        return 0
    fi

    log_bold "💀 $(_i18n TITLE_DESTROY_GLOBAL)"
    op_begin "destroy_global_config" "$global_dir" "force=$force"
    echo ""

    log_warn "$(_i18n MSG_GLOBAL_RESOURCES_TO_DELETE)"
    echo ""
    echo "  $(_i18n MSG_GLOBAL_CONFIG_DIR)"
    echo "    • $global_dir"
    echo ""

    if [[ "$force" == false ]]; then
        if ! ask_yes_no "$(_i18n CONFIRM_DESTROY_GLOBAL)" "N"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    echo ""

    if safe_rm "$global_dir" 2>/dev/null; then
        log_success "$(_i18n MSG_DELETED) $global_dir"
        op_ok "destroy_global_config" "$global_dir"
    else
        log_error "$(_i18n ERR_DELETE_FAILED)"
        log_info "$(_i18n INFO_MANUAL_DELETE) $global_dir"
        op_fail "destroy_global_config" "$global_dir" 1
        return 1
    fi

    echo ""
    log_success "$(_i18n MSG_GLOBAL_DESTROY_COMPLETE)"
    log_info "$(_i18n MSG_USE_INIT_GLOBAL_REINIT)"
}

# Execute destroy command
destroy_command_execute() {
    local force=false
    local global_mode=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                destroy_command_help
                return 0
                ;;
            --force|-f)
                force=true
                shift
                ;;
            --global)
                global_mode=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                destroy_command_help
                return 1
                ;;
            *)
                cli_log_no_positional "destroy"
                echo ""
                destroy_command_help
                return 1
                ;;
        esac
    done

    # Global mode: delete global configuration only
    if [[ "$global_mode" == true ]]; then
        _destroy_global_config "$force"
        return $?
    fi
    op_begin "destroy_project" "$(_git_root)" "force=$force"

    # ── Pre-flight checks ──
    local project_root
    project_root=$(_git_root)
    local workhive_dir
    workhive_dir=$(_workhive_dir)
    local has_workhive_dir=true

    if [[ ! -d "$workhive_dir" ]]; then
        has_workhive_dir=false
        log_warn "$(_i18n ERR_NO_WORKHIVE_DIR)"
        log_info "$(_i18n DESTROY_INFO_CLEANUP_SESSIONS_ONLY)"
    fi

    # ── Safety check: cannot execute inside a worktree directory ──
    local current_dir
    current_dir=$(pwd)
    local worktree_base="${workhive_dir}/worktrees"

    if [[ "$has_workhive_dir" == true && "$current_dir" == "$worktree_base"* ]]; then
        log_error "$(_i18n ERR_CANNOT_DESTROY_IN_WT)"
        log_info "$(_i18n INFO_CD_TO_ROOT) $project_root"
        op_fail "destroy_project" "$project_root" 1 "inside_worktree=true"
        return 1
    fi

    log_bold "💀 $(_i18n TITLE_DESTROY)"
    echo ""

    # ── Collect information ──

    # Collect all active sessions
    local sessions=()
    local session_list
    session_list=$(session_list_project_family 2>/dev/null)
    while IFS= read -r session; do
        [[ -z "$session" ]] && continue
        sessions+=("$session")
    done <<< "$session_list"

    # Collect all Worktrees
    local worktrees=()
    if [[ "$has_workhive_dir" == true && -d "$worktree_base" ]]; then
        local dir
        for dir in "$worktree_base"/*/; do
            [[ -d "$dir" ]] || continue
            local _bn="${dir%/}"; _bn="${_bn##*/}"
            worktrees+=("$_bn")
        done
    fi

    # Check all worktrees for dirty state and unmerged commits
    if [[ "$has_workhive_dir" == true && ${#worktrees[@]} -gt 0 ]]; then
        _worktree_safety_scan "$project_root" "$worktree_base" "${worktrees[@]}"
    fi

    # ── Show resources to be deleted ──
    log_warn "$(_i18n MSG_RESOURCES_TO_DELETE)"
    echo ""

    if [[ ${#sessions[@]} -gt 0 ]]; then
        echo "  $(_i18n MSG_TMUX_SESSIONS) (${#sessions[@]}):"
        for s in "${sessions[@]}"; do
            local branch_name
            branch_name=$(session_extract_branch "$s")
            echo "    • $branch_name ($s)"
        done
        echo ""
    fi

    if [[ ${#worktrees[@]} -gt 0 ]]; then
        echo "  Worktree (${#worktrees[@]}):"
        for wt in "${worktrees[@]}"; do
            local marks
            marks=$(_worktree_safety_get_marks "$wt")
            echo -e "    • $wt${marks}"
        done
        echo ""
    fi

    if [[ "$has_workhive_dir" == true ]]; then
        echo "  $(_i18n MSG_CONFIG_DIR)"
        echo "    • $workhive_dir"
        echo ""
    fi

    _worktree_safety_print_warning_box

    # ── Confirmation ──
    if [[ "$force" == false ]]; then
        if ! ask_yes_no "$(_i18n CONFIRM_DESTROY)" "N"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi

        # Mandatory second confirmation when at-risk data exists
        if [[ "$_WT_SAFETY_HAS_WARNINGS" == true ]]; then
            echo ""
            log_error "$(_i18n CONFIRM_DESTROY_DIRTY)"
            if ! ask_yes_no "$(_i18n CONFIRM_DESTROY_DIRTY_TYPED)" "N"; then
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
        fi
    fi

    echo ""

    # ── Step 1: Close all sessions ──
    log_info "$(_i18n DESTROY_STEP_1) $(_i18n DESTROY_STEP_1_DESC)"

    session_cleanup_project_family "true" || true
    local session_closed="${SESSION_CLEANUP_CLOSED:-0}"

    if [[ $session_closed -gt 0 ]]; then
        log_success "$session_closed $(_i18n MSG_SESSIONS_CLOSED)"
    else
        log_info "$(_i18n MSG_NO_SESSIONS)"
    fi

    local remaining_family_sessions
    remaining_family_sessions="${SESSION_CLEANUP_REMAINING:-}"
    if [[ -n "$remaining_family_sessions" ]]; then
        log_warn "$(_i18n DESTROY_WARN_SESSIONS_REMAIN)"
        echo "$remaining_family_sessions" | while IFS= read -r _left; do
            [[ -z "$_left" ]] && continue
            echo "  • $_left"
        done
        echo ""
        log_info "$(_i18n DESTROY_INFO_MANUAL_SESSION_CLEANUP)"
        echo "$remaining_family_sessions" | while IFS= read -r _left; do
            [[ -z "$_left" ]] && continue
            echo "  tmux kill-session -t \"$_left\""
        done
    fi
    echo ""

    # ── Step 2: Delete all Worktrees ──
    log_info "$(_i18n DESTROY_STEP_2) $(_i18n DESTROY_STEP_2_DESC)"

    local wt_deleted=0
    for wt in "${worktrees[@]}"; do
        if worktree_remove "$wt" "true" 2>/dev/null; then
            wt_deleted=$((wt_deleted + 1))
            log_debug "$(_i18n DEBUG_DELETED_WORKTREE) $wt"

            # Delete the corresponding Git branch
            (cd "$project_root" && git branch -D "$wt") &>/dev/null || true
        else
            log_warn "$(_i18n WARN_WORKTREE_DELETE_FAILED) $wt"
            # Force cleanup: directly delete directory + prune
            local wt_path="${worktree_base}/${wt}"
            local _force_ok=true
            safe_rm "$wt_path" 2>/dev/null || _force_ok=false
            (cd "$project_root" && worktree_prune) 2>/dev/null || true
            (cd "$project_root" && git branch -D "$wt") &>/dev/null || true
            if [[ "$_force_ok" == true ]] || [[ ! -d "$wt_path" ]]; then
                wt_deleted=$((wt_deleted + 1))
            else
                log_warn "$(_i18n DESTROY_WARN_FORCE_DELETE_WT_FAILED) $wt"
            fi
        fi
    done

    if [[ $wt_deleted -gt 0 ]]; then
        log_success "$wt_deleted $(_i18n MSG_WT_DELETED)"
    else
        log_info "$(_i18n MSG_NO_WT)"
    fi

    # Final prune to ensure clean Git state
    (cd "$project_root" && worktree_prune) 2>/dev/null || true
    echo ""

    # ── Step 3: Delete .workhive/ directory ──
    log_info "$(_i18n DESTROY_STEP_3) $(_i18n DESTROY_STEP_3_DESC)"

    if [[ "$has_workhive_dir" == true ]]; then
        _log_disable_file
        if safe_rm "$workhive_dir" 2>/dev/null; then
            log_success "$(_i18n MSG_DELETED) $workhive_dir"
        else
            log_error "$(_i18n ERR_DELETE_FAILED)"
            log_info "$(_i18n INFO_MANUAL_DELETE) $workhive_dir"
            op_fail "destroy_project" "$project_root" 1 "remove_workhive_dir_failed"
            return 1
        fi
    else
        log_info "$(_i18n DESTROY_INFO_WORKHIVE_ALREADY_REMOVED)"
    fi

    echo ""
    log_success "$(_i18n MSG_DESTROY_COMPLETE)"
    op_ok "destroy_project" "$project_root" "sessions_closed=$session_closed worktrees_deleted=$wt_deleted sessions_remain=$(echo "$remaining_family_sessions" | sed '/^$/d' | wc -l | tr -d ' ')"
    echo ""
    log_info "$(_i18n MSG_PROJECT_RESET)"
    log_info "$(_i18n MSG_USE_INIT_REINIT)"
}

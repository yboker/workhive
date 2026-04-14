#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Attach Command
# Attach to existing session (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_ATTACH_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _ATTACH_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/app/shared/layout_rebuild_helper.sh"
source "${LIB_DIR}/utils/tui.sh"

# Delay constant (seconds)
readonly _ATTACH_DELAY_BEFORE_ATTACH=0.5

# Display usage help
attach_command_help() {
    _help_dispatch "_attach_command_help"
}

# Execute attach command
# Arguments: $@ = command line arguments
attach_command_execute() {
    local branch=""
    local read_only=false
    local force_layout=false
    local target=""
    local session_name=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                attach_command_help
                return 0
                ;;
            -r|--read-only)
                read_only=true
                shift
                ;;
            --force-layout)
                force_layout=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                attach_command_help
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

    # If no target specified: interactive selection
    if [[ -z "$branch" ]]; then
        local select_rc=0
        target=$(_attach_command_interactive_select) || select_rc=$?
        if [[ $select_rc -ne 0 || -z "$target" ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
        branch="$target"
    fi

    target="$branch"

    # Resolve attach target with session-first semantics:
    # 1) exact full session name
    # 2) unique branch alias among active family sessions
    local family_sessions
    family_sessions=$(session_list_project_family 2>/dev/null || true)
    if echo "$family_sessions" | grep -Fxq "$target"; then
        session_name="$target"
        branch=$(session_extract_branch "$session_name")
    else
        # Branch alias mode
        if _branch_needs_sanitize "$branch"; then
            if ! _branch_confirm_sanitize "$branch"; then
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
        fi

        local normalized_branch
        normalized_branch=$(_sanitize_branch_name "$branch")

        local matched_sessions=()
        while IFS= read -r _s; do
            [[ -z "$_s" ]] && continue
            local _b
            _b=$(session_extract_branch "$_s")
            [[ "$_b" == "$normalized_branch" ]] || continue
            matched_sessions+=("$_s")
        done <<< "$family_sessions"

        if [[ ${#matched_sessions[@]} -eq 1 ]]; then
            session_name="${matched_sessions[0]}"
            branch="$normalized_branch"
        elif [[ ${#matched_sessions[@]} -gt 1 ]]; then
            log_error "Ambiguous branch alias: $normalized_branch"
            log_info "$(_i18n MSG_AVAILABLE_SESSIONS)"
            local _s
            for _s in "${matched_sessions[@]}"; do
                echo "  • $_s"
            done
            log_info "Use full session name with 'workhive attach <session>'"
            return 1
        else
            session_name=$(session_get_name "$branch")
        fi
    fi

    log_bold "🔗 $(_i18n INFO_ATTACHING) $branch"
    echo ""

    # Attach is session-only: never auto-create via open.
    if ! session_exists "$session_name"; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $session_name"
        echo ""
        log_info "$(_i18n MSG_AVAILABLE_SESSIONS)"
        local available_sessions
        available_sessions=$(session_list_project_family)

        if [[ -z "$available_sessions" ]]; then
            echo "  $(_i18n MSG_NO_ACTIVE_SESSIONS)"
        else
            echo "$available_sessions" | while read -r session; do
                local branch_name
                branch_name=$(session_extract_branch "$session")
                echo "  • $branch_name ($(_i18n COL_SESSION): $session)"
            done
        fi

        echo ""
        log_info "$(_i18n INFO_CREATE_NEW_SESSION)"
        echo "  workhive open $branch"
        return 1
    fi

    # Validate session health
    if ! session_validate_by_name "$session_name"; then
        log_error "$(_i18n ERR_SESSION_CORRUPTED)"
        log_info "$(_i18n INFO_TRY_RECREATE)"
        echo "  workhive rm $branch"
        echo "  workhive open $branch"
        return 1
    fi

    # Display session info (human-readable)
    _attach_print_session_info "$session_name" "$branch"

    # Read-only mode hint
    if [[ "$read_only" == true ]]; then
        log_info "$(_i18n INFO_READONLY_MODE)"

        # Check tmux version
        local adapter
        adapter=$(terminal_adapter_get)

        if [[ "$adapter" == "tmux" ]]; then
            local tmux_version
            tmux_version=$(terminal_adapter_get_version)
            local major="${tmux_version%%.*}"

            if [[ $major -lt 3 ]]; then
                log_warn "$(_i18n WARN_READONLY_NEEDS_TMUX3) $tmux_version"
                log_info "$(_i18n INFO_ATTACH_NORMAL)"
                read_only=false
            fi
        else
            log_warn "$(_i18n WARN_READONLY_TMUX_ONLY)"
            read_only=false
        fi
    fi

    # ── Force layout rebuild (--force-layout) ──
    if [[ "$force_layout" == true ]]; then
        log_info "$(_i18n MSG_FORCE_LAYOUT_REBUILD)..."

        local project_root
        project_root=$(_git_root)

        local workdir
        workdir=$(worktree_get_path "$branch" 2>/dev/null || echo "$project_root")
        [[ ! -d "$workdir" ]] && workdir="$project_root"

        local _rebuild_rc=0
        layout_rebuild_force "$branch" "$workdir" || _rebuild_rc=$?
        if [[ $_rebuild_rc -eq 1 ]]; then
            log_error "$(_i18n ERR_SESSION_CREATE_FAILED)"
            return 1
        fi
        if [[ $_rebuild_rc -eq 0 ]]; then
            log_success "$(_i18n MSG_LAYOUT_CREATED)"
        elif [[ $_rebuild_rc -eq 3 ]]; then
            log_warn "$(_i18n WARN_LAYOUT_REBUILD_ROLLBACK)"
        else
            log_error "$(_i18n ERR_LAYOUT_CREATE_FAILED)"
            log_warn "$(_i18n WARN_LAYOUT_INCOMPLETE)"
        fi

        session_name=$(session_get_name "$branch")
    fi

    log_info "$(_i18n MSG_ATTACHING_TO) $session_name"
    log_info "$(_i18n HINT_DETACH)"
    sleep "$_ATTACH_DELAY_BEFORE_ATTACH"

    # Attach to session
    if [[ "$read_only" == true ]]; then
        terminal_adapter_attach_session_readonly "$session_name"
    else
        # Normal mode: attach by full session name to avoid branch ambiguity
        session_attach_by_name "$session_name"
    fi

}

_attach_print_session_info() {
    local session_name="$1"
    local branch="$2"

    local adapter windows attached attached_label activity_epoch recent_session
    adapter=$(terminal_adapter_get 2>/dev/null || echo "-")
    windows=$(terminal_adapter_get_window_count "$session_name" 2>/dev/null || echo "0")
    attached=false
    if terminal_adapter_is_session_attached "$session_name" 2>/dev/null; then
        attached=true
    fi
    if [[ "$attached" == true ]]; then
        attached_label="$(_i18n LABEL_YES)"
    else
        attached_label="$(_i18n LABEL_NO)"
    fi
    activity_epoch=$(session_activity_epoch "$session_name")
    recent_session=$(format_relative_time "$activity_epoch")

    log_info "$(_i18n INFO_SESSION_INFO)"
    echo "  $(_i18n COL_SESSION): $session_name"
    echo "  $(_i18n COL_BRANCH): $branch"
    echo "  $(_i18n INFO_SESSION_ADAPTER): $adapter"
    echo "  $(_i18n INFO_SESSION_WINDOWS): $windows"
    echo "  $(_i18n INFO_SESSION_ATTACHED): $attached_label"
    echo "  $(_i18n STATUS_DETAIL_COL_RECENT_SESSION): $recent_session"
    echo ""
}

# Interactive selection of active sessions
# Output (stdout): selected full session name
# Return: 0 = user selected a session, 1 = user cancelled or no sessions available
_attach_command_interactive_select() {
    # Non-interactive terminal: do not show menu
    [[ ! -t 0 ]] && return 1

    # Get active session list
    local sessions
    sessions=$(session_list_project_family 2>/dev/null)

    if [[ -z "$sessions" ]]; then
        log_info "$(_i18n MSG_NO_ACTIVE_SESSIONS)" > /dev/tty
        echo "" > /dev/tty
        log_info "$(_i18n MSG_ATTACH_PROMPT)" > /dev/tty
        echo "  workhive open <worktree>" > /dev/tty
        return 1
    fi

    # Build sortable rows: epoch|session|state|branch|human_activity
    local _activity_rows=()
    while IFS= read -r session; do
        [[ -z "$session" ]] && continue
        local branch_name activity_epoch activity_human state_key state_text
        branch_name=$(session_extract_branch "$session")
        activity_epoch=$(session_activity_epoch "$session")
        activity_human=$(format_relative_time "$activity_epoch")
        state_key="detached"
        if terminal_adapter_is_session_attached "$session" 2>/dev/null; then
            state_key="active"
        fi
        state_text=$(_attach_state_with_icon "$state_key")
        _activity_rows+=("${activity_epoch}|${session}|${state_text}|${branch_name}|${activity_human}")
    done <<< "$sessions"

    # Sort by last activity desc (recent first)
    local sorted_rows
    sorted_rows=$(printf '%s\n' "${_activity_rows[@]}" | sort -t'|' -k1,1nr)

    # Build menu arrays
    local session_names=()
    local _attach_menu_items=()
    while IFS='|' read -r _epoch session state_text branch_name activity_human; do
        [[ -z "$session" ]] && continue
        session_names+=("$session")
        _attach_menu_items+=("$(_attach_format_menu_row "$session" "$state_text" "$branch_name" "$activity_human")")
    done <<< "$sorted_rows"

    if [[ ${#_attach_menu_items[@]} -eq 0 ]]; then
        log_info "$(_i18n MSG_NO_ACTIVE_SESSIONS)" > /dev/tty
        return 1
    fi

    # Print one-time header above menu (avoid multiline prompt redraw artifacts)
    _attach_print_menu_header

    # TUI menu selection (ESC to cancel: TUI_SELECTED_INDEX=-1)
    tui_select_menu "$(_i18n MSG_SELECT_ATTACH)" _attach_menu_items 0

    # ESC to cancel
    [[ "$TUI_SELECTED_INDEX" -eq -1 ]] && return 1

    echo "${session_names[$TUI_SELECTED_INDEX]}"
    return 0
}

_attach_print_menu_header() {
    local s_h="$(_i18n COL_SESSION)"
    local st_h="$(_i18n STATUS_DETAIL_COL_STATE)"
    local b_h="$(_i18n COL_BRANCH)"
    local a_h="$(_i18n STATUS_DETAIL_COL_RECENT_SESSION)"
    local s_w=44 st_w=12 b_w=14 a_w=12
    local sep
    sep=$(printf '%*s' $((s_w + st_w + b_w + a_w + 6)) '' | tr ' ' '─')

    {
        printf '\n'
        # Align with menu item content start ("  ● " prefix takes 4 columns)
        printf '    %s  %s  %s  %s\n' \
            "$(_wpad "$s_w" "$s_h")" "$(_wpad "$st_w" "$st_h")" "$(_wpad "$b_w" "$b_h")" "$(_wpad "$a_w" "$a_h")"
        printf '    %s\n' "$sep"
    } > /dev/tty
}

_attach_format_menu_row() {
    local session_name="$1"
    local state_text="$2"
    local branch_name="$3"
    local last_activity="$4"
    local s_w=44 st_w=12 b_w=14 a_w=12
    printf '%s  %s  %s  %s' \
        "$(_wpad "$s_w" "$session_name")" "$(_wpad "$st_w" "$state_text")" "$(_wpad "$b_w" "$branch_name")" "$(_wpad "$a_w" "$last_activity")"
}

_attach_state_with_icon() {
    case "$1" in
        active) echo "● $(_i18n STATUS_ACTIVE)" ;;
        *)      echo "○ $(_i18n STATUS_DETACHED)" ;;
    esac
}

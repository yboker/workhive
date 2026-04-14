#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Shared Worktree Interactive Selector
# TUI menu for choosing a worktree branch.
# Used by open, pane, remove commands.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_WORKTREE_SELECTOR_SH_LOADED:-}" ]] && return 0
readonly _WORKTREE_SELECTOR_SH_LOADED=1

source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/app/shared/worktree_collector.sh"

# Build a TUI menu for worktree selection.
#
# Options (flags before positional args):
#   --with-main-dir     Include "Main directory" as first item
#   --with-create-new   Include "+ Create new worktree" item
#   --title <string>    Menu title (default: select worktree i18n)
#
# Output (stdout): selected branch name, or "__MAIN_DIR__" for main dir
# Return: 0 = selection made, 1 = cancelled / non-interactive / empty
#
# Usage:
#   branch=$(worktree_interactive_select --with-main-dir --with-create-new)
#   branch=$(worktree_interactive_select --title "Select to remove")
# Build menu items array for worktree selection.
# Sets _WT_SEL_MENU_ITEMS and _WT_SEL_OFFSET.
_wt_selector_build_menu() {
    local with_main_dir="$1" with_create_new="$2" current_branch="$3"
    shift 3
    local worktrees=("$@")

    _WT_SEL_MENU_ITEMS=()
    _WT_SEL_OFFSET=0

    if [[ "$with_main_dir" == true ]]; then
        local main_label
        main_label="$(_i18n MSG_MAIN_DIR)"
        [[ -n "$current_branch" ]] && main_label="$(_i18n MSG_MAIN_DIR) ($current_branch)"
        _WT_SEL_MENU_ITEMS+=("$main_label")
        _WT_SEL_OFFSET=$((_WT_SEL_OFFSET + 1))
    fi

    if [[ "$with_create_new" == true ]]; then
        _WT_SEL_MENU_ITEMS+=("+ $(_i18n MSG_CREATE_NEW_WT)...")
        _WT_SEL_OFFSET=$((_WT_SEL_OFFSET + 1))
    fi

    local _sel_prefix
    _sel_prefix=$(session_get_prefix)
    local w
    for w in "${worktrees[@]}"; do
        local session_name="${_sel_prefix}@${w}"
        local status_icon
        if terminal_adapter_has_session "$session_name" 2>/dev/null; then
            status_icon="● $(_i18n MSG_ACTIVE)"
        else
            status_icon="○ $(_i18n MSG_CREATED)"
        fi
        _WT_SEL_MENU_ITEMS+=("${w}  (${status_icon})")
    done
}

# Resolve menu selection index to a branch name or sentinel.
# Echoes the result; returns 1 on cancelled input.
_wt_selector_resolve() {
    local selected_index="$1" with_main_dir="$2" with_create_new="$3"
    shift 3
    local worktrees=("$@")

    local main_dir_index=-1 create_new_index=-1 item_offset=0
    if [[ "$with_main_dir" == true ]]; then
        main_dir_index=$item_offset; item_offset=$((item_offset + 1))
    fi
    if [[ "$with_create_new" == true ]]; then
        create_new_index=$item_offset; item_offset=$((item_offset + 1))
    fi

    if [[ "$selected_index" -eq "$main_dir_index" ]]; then
        echo "__MAIN_DIR__"
    elif [[ "$selected_index" -eq "$create_new_index" ]]; then
        local new_branch=""
        _tui_print "\n"
        read -p "  $(_i18n MSG_ENTER_BRANCH) " new_branch < /dev/tty
        [[ -z "$new_branch" ]] && return 1
        echo "$new_branch"
    else
        echo "${worktrees[$((selected_index - item_offset))]}"
    fi
}

worktree_interactive_select() {
    [[ ! -t 0 ]] && return 1

    local with_main_dir=false with_create_new=false title=""
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --with-main-dir)   with_main_dir=true; shift ;;
            --with-create-new) with_create_new=true; shift ;;
            --title)           title="$2"; shift 2 ;;
            *)                 shift ;;
        esac
    done
    [[ -z "$title" ]] && title=$(_i18n SELECT_WORKTREE)

    local worktrees=()
    _collect_available_worktrees worktrees
    local current_branch="$WORKTREE_COLLECTOR_BRANCH"

    if [[ "$with_main_dir" != true && "$with_create_new" != true && ${#worktrees[@]} -eq 0 ]]; then
        log_info "$(_i18n MSG_NO_WT_TO_REMOVE 2>/dev/null || echo 'No worktrees available')" > /dev/tty
        return 1
    fi

    _wt_selector_build_menu "$with_main_dir" "$with_create_new" "$current_branch" "${worktrees[@]}"
    tui_select_menu "$title" _WT_SEL_MENU_ITEMS 0

    [[ "$TUI_SELECTED_INDEX" -eq -1 ]] && return 1
    _wt_selector_resolve "$TUI_SELECTED_INDEX" "$with_main_dir" "$with_create_new" "${worktrees[@]}"
}

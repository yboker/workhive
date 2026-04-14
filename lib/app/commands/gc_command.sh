#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# GC Command
# Garbage collection: clean up Worktrees for merged branches (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_GC_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _GC_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/app/shared/worktree_safety.sh"
source "${LIB_DIR}/infra/persistence/snapshot.sh"

# Show usage help
gc_command_help() {
    _help_dispatch "_gc_command_help"
}

# ──────────────────────────────────────────────────
# Get branches merged into the main branch
# Returns (stdout): newline-separated branch names
# ──────────────────────────────────────────────────
_gc_get_merged_branches() {
    if ! git_is_inside_work_tree; then
        return 1
    fi

    local main_branch
    main_branch=$(_git_main_branch)

    # Get branches merged into main (excluding main branch itself and current branch)
    local current_branch
    current_branch=$(_git_current_branch)

    git branch --merged "$main_branch" 2>/dev/null | while IFS= read -r branch; do
        branch="${branch#"${branch%%[![:space:]*]*}"}"
        branch="${branch#\*}"
        branch="${branch#"${branch%%[![:space:]*]*}"}"
        branch="${branch// /}"
        [[ -z "$branch" ]] && continue
        [[ "$branch" == "$main_branch" ]] && continue
        [[ "$branch" == "$current_branch" ]] && continue
        echo "$branch"
    done
}

# ──────────────────────────────────────────────────
# Get unmodified worktree branches (identical to parent branch)
# Excludes merged branches (those are handled by _gc_get_merged_branches)
# ──────────────────────────────────────────────────
_gc_get_unmodified_branches() {
    local main_branch
    main_branch=$(_git_main_branch)

    local current_branch
    current_branch=$(_git_current_branch)

    local worktrees
    worktrees=$(worktree_list_all 2>/dev/null)
    [[ -z "$worktrees" ]] && return 0

    while IFS= read -r wt_path; do
        [[ -z "$wt_path" ]] && continue
        local branch
        branch=$(worktree_get_branch "$wt_path" 2>/dev/null) || true
        [[ -z "$branch" ]] && continue
        [[ "$branch" == "$main_branch" ]] && continue
        [[ "$branch" == "$current_branch" ]] && continue

        local has_diff=false
        if _worktree_is_dirty "$wt_path" || (cd "$wt_path" 2>/dev/null && {
            [[ -n "$(git log "${main_branch}..${branch}" --oneline 2>/dev/null)" ]];
        }); then
            has_diff=true
        fi

        if [[ "$has_diff" == false ]]; then
            echo "$branch"
        fi
    done <<< "$worktrees"
}

# ──────────────────────────────────────────────────
# Get orphan sessions (sessions without a corresponding worktree directory)
# ──────────────────────────────────────────────────
_gc_get_orphan_sessions() {
    local sessions
    sessions=$(session_list_project)

    while IFS= read -r session; do
        [[ -z "$session" ]] && continue
        local branch
        branch=$(session_extract_branch "$session")
        if ! worktree_exists "$branch"; then
            echo "$branch"
        fi
    done <<< "$sessions"
}

# ──────────────────────────────────────────────────
# Get stale state files
# ──────────────────────────────────────────────────
_gc_get_stale_state_files() {
    local project_root
    # shellcheck disable=SC2034
    project_root=$(_git_root)
    local state_dir
    state_dir="$(_workhive_dir)/state"

    [[ -d "$state_dir" ]] || return 0

    for state_file in "$state_dir"/*; do
        [[ -f "$state_file" ]] || continue
        local basename
        basename=$(basename "$state_file")

        # Extract branch name from filename (format: layout_<branch>, pane_commands_<branch>, window_size_<branch>)
        local branch=""
        case "$basename" in
            layout_*)         branch="${basename#layout_}" ;;
            pane_commands_*)  branch="${basename#pane_commands_}" ;;
            window_size_*)    branch="${basename#window_size_}" ;;
        esac

        [[ -z "$branch" ]] && continue

        # If neither worktree nor session exists for the branch, it's stale
        local session_name
        session_name=$(session_get_name "$branch")
        if ! worktree_exists "$branch" && ! session_exists "$session_name"; then
            echo "$state_file"
        fi
    done
}

_gc_get_snapshot_files() {
    local latest
    latest=$(snapshot_get_latest 2>/dev/null || echo "")
    [[ -n "$latest" && -e "$latest" ]] && echo "$latest"

    local file
    while IFS= read -r file; do
        [[ -n "$file" && -e "$file" ]] && echo "$file"
    done < <(snapshot_list_history_files)
}

# ──────────────────────────────────────────────────
# GC sub-phases (split for maintainability)
# ──────────────────────────────────────────────────

# Remove a single branch: kill session, remove worktree, delete branch.
# Returns: 0 on full success, 1 if any sub-step fails.
_gc_remove_branch() {
    local branch="$1"
    local delete_mode="${2:-safe}"
    local ok=true
    op_begin "gc_remove_branch" "$branch" "mode=$delete_mode"

    local session_name
    session_name=$(session_get_name "$branch")
    if session_exists "$session_name"; then
        session_kill "$branch" 2>/dev/null || { log_warn "$(_i18n GC_WARN_KILL_SESSION_FAILED) $branch"; ok=false; }
    fi

    if worktree_exists "$branch"; then
        worktree_remove "$branch" "true" 2>/dev/null || { log_warn "$(_i18n GC_WARN_REMOVE_WORKTREE_FAILED) $branch"; ok=false; }
    fi

    if [[ "$delete_mode" == "force-check" ]]; then
        local _main_br
        _main_br=$(_git_main_branch)
        if git merge-base --is-ancestor "$branch" "$_main_br" 2>/dev/null; then
            git branch -d "$branch" 2>/dev/null || git branch -D "$branch" 2>/dev/null || {
                log_warn "$(_i18n_fmt GC_WARN_BRANCH_DELETE_SKIPPED_FMT "$branch")"; ok=false
            }
        else
            log_warn "$(_i18n_fmt GC_WARN_BRANCH_NOT_MERGED_FMT "$branch" "$_main_br")"; ok=false
        fi
    else
        git branch -d "$branch" 2>/dev/null || { log_warn "$(_i18n GC_WARN_DELETE_BRANCH_FAILED) $branch"; ok=false; }
    fi

    if [[ "$ok" == true ]]; then
        op_ok "gc_remove_branch" "$branch" "mode=$delete_mode"
        return 0
    fi
    op_fail "gc_remove_branch" "$branch" 1 "mode=$delete_mode"
    return 1
}

# Ask a GC confirmation prompt and normalize return semantics.
# Return: 0=confirmed, 1=declined, 130=interrupted (Ctrl+C)
_gc_handle_prompt() {
    local prompt="$1"
    local skip_msg="$2"
    local default_answer="${3:-}"
    local prompt_rc=0

    if [[ -n "$default_answer" ]]; then
        ask_yes_no "$prompt" "$default_answer"
        prompt_rc=$?
    else
        ask_yes_no "$prompt"
        prompt_rc=$?
    fi

    if [[ $prompt_rc -eq 130 ]]; then
        return 130
    fi
    if [[ $prompt_rc -ne 0 ]]; then
        log_info "$skip_msg"
        return 1
    fi
    return 0
}

_gc_print_branch_bullets_array() {
    local -n _gc_branches_ref=$1
    local branch
    for branch in "${_gc_branches_ref[@]}"; do
        [[ -n "$branch" ]] && echo "    • $branch"
    done
}

_gc_print_branch_bullets_lines() {
    local lines="${1:-}"
    local branch
    while IFS= read -r branch; do
        [[ -n "$branch" ]] && echo "    • $branch"
    done <<< "$lines"
}

# Phase 1: Clean merged branches.
# $1=auto_mode, $2=dry_run, $3=cleaned_ref(nameref), $4=failed_ref(nameref)
_gc_phase_merged() {
    local auto_mode="$1" dry_run="$2"
    local -n _gc_c=$3 _gc_f=$4

    log_info "$(_i18n GC_SCANNING)"
    local merged
    merged=$(_gc_get_merged_branches)

    local -a merged_with_wt=()
    local branch
    while IFS= read -r branch; do
        [[ -z "$branch" ]] && continue
        worktree_exists "$branch" && merged_with_wt+=("$branch")
    done <<< "$merged"

    if [[ ${#merged_with_wt[@]} -eq 0 ]]; then
        log_info "$(_i18n GC_NO_MERGED)"
        echo ""
        return
    fi

    local -a dirty_merged=() clean_merged=()
    for branch in "${merged_with_wt[@]}"; do
        local _wt_path
        _wt_path=$(worktree_get_path "$branch" 2>/dev/null)
        if [[ -n "$_wt_path" ]] && _worktree_is_dirty "$_wt_path"; then
            dirty_merged+=("$branch")
        else
            clean_merged+=("$branch")
        fi
    done

    local _gc_git_root
    _gc_git_root=$(_git_root)
    _worktree_safety_scan "$_gc_git_root" "" "${merged_with_wt[@]}"

    log_info "$(_i18n GC_FOUND_MERGED)"
    for branch in "${merged_with_wt[@]}"; do
        local marks
        marks=$(_worktree_safety_get_marks "$branch")
        echo -e "    • ${branch}${marks}"
    done
    echo ""
    _worktree_safety_print_warning_box 3 GC_WARN_DIRTY_BRANCHES

    local do_clean=true
    if [[ "$dry_run" == true ]]; then
        do_clean=false; log_info "[dry-run] $(_i18n GC_SKIPPED)"
    elif [[ "$auto_mode" != true ]]; then
        local _prompt_rc
        if [[ ${#dirty_merged[@]} -gt 0 ]]; then
            _gc_handle_prompt "$(_i18n GC_CONFIRM_DIRTY)" "$(_i18n GC_SKIPPED_DIRTY)" "N"
            _prompt_rc=$?
        else
            _gc_handle_prompt "$(_i18n GC_CONFIRM)" "$(_i18n GC_SKIPPED)"
            _prompt_rc=$?
        fi
        if [[ $_prompt_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_prompt_rc -ne 0 ]]; then
            do_clean=false
        fi
    elif [[ "$auto_mode" == true && ${#dirty_merged[@]} -gt 0 ]]; then
        log_warn "$(_i18n GC_SKIPPED_DIRTY)"
        merged_with_wt=("${clean_merged[@]}")
        [[ ${#merged_with_wt[@]} -eq 0 ]] && do_clean=false
    fi

    if [[ "$do_clean" == true ]]; then
        for branch in "${merged_with_wt[@]}"; do
            log_info "$(_i18n GC_REMOVING) $branch"
            if _gc_remove_branch "$branch" "safe"; then
                ((_gc_c++)) || true
            else
                ((_gc_f++)) || true
            fi
        done
    fi
    echo ""
}

# Filter out protected branches from GC candidates.
# Args: branch names
# Stdout: filtered branch names (newline-separated)
_gc_filter_protected() {
    local main_branch current_branch
    main_branch=$(_git_main_branch)
    current_branch=$(_git_current_branch)

    local -A seen=()
    local branch
    for branch in "$@"; do
        [[ -z "$branch" ]] && continue
        [[ "$branch" == "$main_branch" ]] && continue
        [[ "$branch" == "$current_branch" ]] && continue
        [[ -n "${seen[$branch]:-}" ]] && continue
        seen["$branch"]=1
        echo "$branch"
    done
}

# Phase 2: Clean unmodified branches.
# $1=auto_mode, $2=dry_run, $3=cleaned_ref(nameref), $4=failed_ref(nameref)
_gc_phase_unmodified() {
    local auto_mode="$1" dry_run="$2"
    local -n _gc_c=$3 _gc_f=$4
    shift 2
    local -a skip_branches=("$@")

    log_info "$(_i18n GC_SCANNING_UNMODIFIED)"
    local unmodified
    unmodified=$(_gc_get_unmodified_branches)

    local -a candidates=()
    local branch
    while IFS= read -r branch; do
        [[ -z "$branch" ]] && continue
        local already=false
        local m
        for m in "${skip_branches[@]}"; do
            [[ "$m" == "$branch" ]] && already=true && break
        done
        [[ "$already" == true ]] && continue
        candidates+=("$branch")
    done <<< "$unmodified"

    local -a unmod_list=()
    while IFS= read -r branch; do
        [[ -n "$branch" ]] && unmod_list+=("$branch")
    done < <(_gc_filter_protected "${candidates[@]}")

    if [[ ${#unmod_list[@]} -eq 0 ]]; then
        log_info "$(_i18n GC_NO_UNMODIFIED)"
        echo ""
        return
    fi

    log_info "$(_i18n GC_FOUND_UNMODIFIED)"
    _gc_print_branch_bullets_array unmod_list
    echo ""

    local do_clean=true
    if [[ "$dry_run" == true ]]; then
        do_clean=false; log_info "[dry-run] $(_i18n GC_SKIPPED)"
    elif [[ "$auto_mode" != true ]]; then
        _gc_handle_prompt "$(_i18n GC_CONFIRM_UNMODIFIED)" "$(_i18n GC_SKIPPED)"
        local _prompt_rc=$?
        if [[ $_prompt_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_prompt_rc -ne 0 ]]; then
            do_clean=false
        fi
    fi

    if [[ "$do_clean" == true ]]; then
        for branch in "${unmod_list[@]}"; do
            if [[ "$auto_mode" == true ]] && worktree_exists "$branch"; then
                local _wt_path _dirty
                _wt_path=$(worktree_get_path "$branch")
                _dirty=$(git -C "$_wt_path" status --porcelain 2>/dev/null)
                if [[ -n "$_dirty" ]]; then
                    log_warn "$(_i18n GC_WARN_SKIP_DIRTY_AUTO) $branch"
                    continue
                fi
            fi
            log_info "$(_i18n GC_REMOVING) $branch"
            if _gc_remove_branch "$branch" "force-check"; then
                ((_gc_c++)) || true
            else
                ((_gc_f++)) || true
            fi
        done
    fi
    echo ""
}

# Phase 3: Clean orphan sessions.
# $1=auto_mode, $2=dry_run, $3=cleaned_ref(nameref), $4=failed_ref(nameref)
_gc_phase_orphan_sessions() {
    local auto_mode="$1" dry_run="$2"
    local -n _gc_c=$3 _gc_f=$4
    local orphans
    orphans=$(_gc_get_orphan_sessions)
    [[ -z "$orphans" ]] && return

    log_info "$(_i18n GC_ORPHAN_SESSIONS)"
    _gc_print_branch_bullets_lines "$orphans"
    echo ""

    local do_clean=true
    if [[ "$dry_run" == true ]]; then
        do_clean=false; log_info "[dry-run] $(_i18n GC_SKIPPED)"
    elif [[ "$auto_mode" != true ]]; then
        _gc_handle_prompt "$(_i18n GC_CONFIRM_ORPHAN)" "$(_i18n GC_SKIPPED)"
        local _prompt_rc=$?
        if [[ $_prompt_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_prompt_rc -ne 0 ]]; then
            do_clean=false
        fi
    fi

    if [[ "$do_clean" == true ]]; then
        while IFS= read -r branch; do
            [[ -z "$branch" ]] && continue
            local session_name
            session_name=$(session_get_name "$branch")
            log_info "$(_i18n INFO_SESSION_KILLING) $session_name"
            if session_exists "$session_name"; then
                if terminal_adapter_kill_session "$session_name" 2>/dev/null; then
                    cache_invalidate "session:*"
                    log_success "$(_i18n INFO_SESSION_CLOSED) $session_name"
                    ((_gc_c++)) || true
                else
                    log_warn "$(_i18n GC_WARN_KILL_ORPHAN_FAILED) $branch"
                    ((_gc_f++)) || true
                fi
            else
                log_info "$(_i18n GC_INFO_SESSION_NOT_FOUND) $branch"
            fi
        done <<< "$orphans"
    fi
    echo ""
}

# Phase 4: Clean stale state files.
# $1=auto_mode, $2=dry_run, $3=cleaned_ref(nameref), $4=failed_ref(nameref)
_gc_phase_stale_files() {
    local auto_mode="$1" dry_run="$2"
    local -n _gc_c=$3 _gc_f=$4
    local stale_files
    stale_files=$(_gc_get_stale_state_files)
    [[ -z "$stale_files" ]] && return

    log_info "$(_i18n GC_STALE_STATE)"
    local state_file
    while IFS= read -r state_file; do
        [[ -z "$state_file" ]] && continue
        echo "    • $(basename "$state_file")"
    done <<< "$stale_files"
    echo ""

    local do_clean=true
    if [[ "$dry_run" == true ]]; then
        do_clean=false; log_info "[dry-run] $(_i18n GC_SKIPPED)"
    elif [[ "$auto_mode" != true ]]; then
        _gc_handle_prompt "$(_i18n GC_CONFIRM_STALE)" "$(_i18n GC_SKIPPED)"
        local _prompt_rc=$?
        if [[ $_prompt_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_prompt_rc -ne 0 ]]; then
            do_clean=false
        fi
    fi

    if [[ "$do_clean" == true ]]; then
        while IFS= read -r state_file; do
            [[ -z "$state_file" ]] && continue
            if rm -f -- "$state_file" 2>/dev/null; then
                ((_gc_c++)) || true
            else
                log_warn "$(_i18n GC_WARN_REMOVE_STATE_FAILED) $(basename "$state_file")"
                ((_gc_f++)) || true
            fi
        done <<< "$stale_files"
    fi
    echo ""
}

# Phase: Clean snapshot files only.
# $1=auto_mode, $2=cleaned_ref(nameref), $3=failed_ref(nameref)
_gc_phase_snapshots() {
    local auto_mode="$1"
    local -n _gc_c=$2 _gc_f=$3

    log_info "$(_i18n GC_SCANNING_SNAPSHOTS)"

    local -a snapshot_files=()
    local file
    while IFS= read -r file; do
        [[ -n "$file" ]] && snapshot_files+=("$file")
    done < <(_gc_get_snapshot_files)

    if [[ ${#snapshot_files[@]} -eq 0 ]]; then
        log_info "$(_i18n GC_NO_SNAPSHOTS)"
        echo ""
        return 0
    fi

    log_info "$(_i18n GC_FOUND_SNAPSHOTS)"
    for file in "${snapshot_files[@]}"; do
        echo "    • $(basename "$file")"
    done
    echo ""

    local do_clean=true
    if [[ "$auto_mode" != true ]]; then
        _gc_handle_prompt "$(_i18n GC_CONFIRM_SNAPSHOTS)" "$(_i18n GC_SKIPPED)"
        local _prompt_rc=$?
        if [[ $_prompt_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_prompt_rc -ne 0 ]]; then
            do_clean=false
        fi
    fi

    if [[ "$do_clean" == true ]]; then
        for file in "${snapshot_files[@]}"; do
            if rm -f -- "$file" 2>/dev/null; then
                ((_gc_c++)) || true
            else
                log_warn "$(_i18n GC_WARN_REMOVE_SNAPSHOT_FAILED) $(basename "$file")"
                ((_gc_f++)) || true
            fi
        done
    fi
    echo ""
}

# ──────────────────────────────────────────────────
# Execute gc command
# ──────────────────────────────────────────────────
gc_command_execute() {
    local auto_mode=false
    local snapshots_only=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) gc_command_help; return 0 ;;
            --auto)    auto_mode=true; shift ;;
            --snapshots) snapshots_only=true; shift ;;
            -*)
                cli_log_unknown_opt "$1"; echo ""
                gc_command_help; return 1 ;;
            *)
                cli_log_no_positional "gc"; echo ""
                gc_command_help; return 1 ;;
        esac
    done
    op_begin "gc_execute" "$(_git_root)" "auto=$auto_mode snapshots_only=$snapshots_only"

    log_bold "🧹 $(_i18n GC_TITLE)"
    echo ""

    local gc_cleaned=0
    local gc_failed=0
    local _phase_rc=0

    if [[ "$snapshots_only" == true ]]; then
        _gc_phase_snapshots "$auto_mode" gc_cleaned gc_failed
        _phase_rc=$?
        if [[ $_phase_rc -eq 130 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            op_fail "gc_execute" "$(_git_root)" 130 "interrupted=true phase=snapshots"
            return 130
        fi
    else
        _gc_phase_merged "$auto_mode" "false" gc_cleaned gc_failed
        _phase_rc=$?
        if [[ $_phase_rc -eq 130 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            op_fail "gc_execute" "$(_git_root)" 130 "interrupted=true phase=merged"
            return 130
        fi
        _gc_phase_unmodified "$auto_mode" "false" gc_cleaned gc_failed
        _phase_rc=$?
        if [[ $_phase_rc -eq 130 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            op_fail "gc_execute" "$(_git_root)" 130 "interrupted=true phase=unmodified"
            return 130
        fi
        _gc_phase_orphan_sessions "$auto_mode" "false" gc_cleaned gc_failed
        _phase_rc=$?
        if [[ $_phase_rc -eq 130 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            op_fail "gc_execute" "$(_git_root)" 130 "interrupted=true phase=orphan_sessions"
            return 130
        fi
        _gc_phase_stale_files "$auto_mode" "false" gc_cleaned gc_failed
        _phase_rc=$?
        if [[ $_phase_rc -eq 130 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            op_fail "gc_execute" "$(_git_root)" 130 "interrupted=true phase=stale_files"
            return 130
        fi

        worktree_prune 2>/dev/null || true
    fi

    if [[ "$gc_failed" -gt 0 ]]; then
        log_warn "$(_i18n GC_COMPLETE): $gc_cleaned $(_i18n GC_CLEANED), $gc_failed failed"
        op_fail "gc_execute" "$(_git_root)" 2 "cleaned=$gc_cleaned failed=$gc_failed"
        return 2
    fi
    log_success "$(_i18n GC_COMPLETE): $gc_cleaned $(_i18n GC_CLEANED)"
    op_ok "gc_execute" "$(_git_root)" "cleaned=$gc_cleaned"
}

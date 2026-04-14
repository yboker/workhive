#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Shared Worktree Collector
# Collect available worktree list (shared logic)
#
# IMPORTANT: Uses global variables to return results, avoiding $() subshell which breaks nameref.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent duplicate loading
[[ -n "${_WORKTREE_COLLECTOR_SH_LOADED:-}" ]] && return 0
readonly _WORKTREE_COLLECTOR_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"

# Global output variables
# Read these after calling _collect_available_worktrees:
#   WORKTREE_COLLECTOR_BRANCH  - current branch name
#   caller-provided array ref  - populated list of worktree names
WORKTREE_COLLECTOR_BRANCH=""

# Collect all available worktrees (directories + active sessions)
#
# Args: $1 = output array variable name (nameref)
# Output: populates array via nameref, returns current branch via WORKTREE_COLLECTOR_BRANCH
# Returns: 0 = success
#
# CRITICAL: Do not capture this function's output with $()!
#   Correct: _collect_available_worktrees my_array
#   Wrong:   result=$(_collect_available_worktrees my_array)  # nameref breaks in subshell!
_collect_available_worktrees() {
    local -n _out_worktrees=$1

    local project_root
    # shellcheck disable=SC2034
    project_root=$(_git_root)

    WORKTREE_COLLECTOR_BRANCH=""
    if git_is_inside_work_tree; then
        WORKTREE_COLLECTOR_BRANCH=$(_git_current_branch)
    fi

    local full_base
    full_base=$(worktree_get_base_path)

    if [[ -d "$full_base" ]]; then
        local dir
        for dir in "$full_base"/*/; do
            [[ -d "$dir" ]] || continue
            dir="${dir%/}"
            _out_worktrees+=("${dir##*/}")
        done
    fi

    local sessions
    sessions=$(session_list_project 2>/dev/null)
    while IFS= read -r session; do
        [[ -z "$session" ]] && continue
        local branch_name
        branch_name=$(session_extract_branch "$session")
        [[ "$branch_name" == "$WORKTREE_COLLECTOR_BRANCH" ]] && continue
        local found=false
        local w
        for w in "${_out_worktrees[@]}"; do
            [[ "$w" == "$branch_name" ]] && found=true && break
        done
        [[ "$found" == false ]] && _out_worktrees+=("$branch_name")
    done <<< "$sessions"

    return 0
}

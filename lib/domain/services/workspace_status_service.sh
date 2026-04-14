#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Workspace Status Service (Domain Layer)
#
# Single-source batch collector for `workhive ls` / `workhive list`.
# All runtime data is collected in one pass; command layer only renders.
#
# Design:
#   - One bulk tmux query for sessions (attached + activity + pane_count).
#   - One `git worktree list --porcelain` parse for worktree mapping.
#   - Git status via TTL cache service (skippable in fast/watch mode).
#   - Output: associative arrays consumed by list_command rendering.
#
# Collection strategy:
#   full  — includes git dirty status (interactive table)
#   fast  — skips git dirty status (--watch, --format json)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_WORKSPACE_STATUS_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _WORKSPACE_STATUS_SERVICE_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/worktree_status_cache_service.sh"

# ──────────────────────────────────────────────────
# Bulk tmux session snapshot
#
# Single tmux query returns: session_name, attached, activity_epoch, pane_count
# Replaces per-session calls to session_activity_epoch and pane counting.
# ──────────────────────────────────────────────────
declare -gA _WSS_SESSION_ATTACHED=()
declare -gA _WSS_SESSION_ACTIVITY=()
declare -gA _WSS_SESSION_PANE_COUNT=()
declare -gA _WSS_SESSION_IN_PROJECT=()

# ──────────────────────────────────────────────────
# Bulk worktree mapping (single `git worktree list --porcelain` parse)
#
# Produces:
#   _WSS_WT_PATH_TO_BRANCH[path] = branch
#   _WSS_WT_BRANCH_TO_PATH[branch] = path
#   _WSS_WT_SANITIZED_TO_REAL[sanitized] = branch
# ──────────────────────────────────────────────────
declare -gA _WSS_WT_PATH_TO_BRANCH=()
declare -gA _WSS_WT_BRANCH_TO_PATH=()
declare -gA _WSS_WT_SANITIZED_TO_REAL=()
declare -ga _WSS_WT_ORDERED_BRANCHES=()

workspace_status_reset() {
    _WSS_SESSION_ATTACHED=()
    _WSS_SESSION_ACTIVITY=()
    _WSS_SESSION_PANE_COUNT=()
    _WSS_SESSION_IN_PROJECT=()
    _WSS_WT_PATH_TO_BRANCH=()
    _WSS_WT_BRANCH_TO_PATH=()
    _WSS_WT_SANITIZED_TO_REAL=()
    _WSS_WT_ORDERED_BRANCHES=()
}

# ──────────────────────────────────────────────────
# Phase 1: Bulk tmux session collection
# Single query returns all session metadata needed by `ls`.
# ──────────────────────────────────────────────────
workspace_status_collect_sessions() {
    local project_root="${1:-}"
    local t; t=$(perf_timer_start)

    local _sname _sattached _sactivity
    while IFS='|' read -r _sname _sattached _sactivity; do
        [[ -z "$_sname" ]] && continue
        _WSS_SESSION_ATTACHED["$_sname"]="${_sattached:-0}"
        _WSS_SESSION_ACTIVITY["$_sname"]="${_sactivity:-0}"
    done < <(tmux list-sessions -F '#{session_name}|#{session_attached}|#{session_activity}' 2>/dev/null || true)

    local _pane_sname _pane_path
    while IFS='|' read -r _pane_sname _pane_path; do
        [[ -z "$_pane_sname" ]] && continue
        _WSS_SESSION_PANE_COUNT["$_pane_sname"]=$(( ${_WSS_SESSION_PANE_COUNT["$_pane_sname"]:-0} + 1 ))
        if [[ -n "$project_root" && "$_pane_path" == "$project_root"* ]]; then
            _WSS_SESSION_IN_PROJECT["$_pane_sname"]=1
        fi
    done < <(tmux list-panes -a -F '#{session_name}|#{pane_current_path}' 2>/dev/null || true)

    perf_timer_log "ls.bulk_tmux" "$t" "sessions=${#_WSS_SESSION_ATTACHED[@]} panes_counted=${#_WSS_SESSION_PANE_COUNT[@]}"
}

# ──────────────────────────────────────────────────
# Phase 2: Bulk worktree mapping
# Single `git worktree list --porcelain` parse.
# ──────────────────────────────────────────────────
workspace_status_collect_worktrees() {
    local t; t=$(perf_timer_start)
    local base_path
    base_path=$(worktree_get_base_path 2>/dev/null) || true
    base_path="${base_path%/}"

    local current_wt="" current_branch=""
    while IFS= read -r line; do
        if [[ "$line" == "worktree "* ]]; then
            current_wt="${line#worktree }"
            current_branch=""
        elif [[ "$line" == "branch refs/heads/"* ]]; then
            current_branch="${line#branch refs/heads/}"
            if [[ -n "$current_wt" && -n "$current_branch" ]]; then
                _WSS_WT_PATH_TO_BRANCH["$current_wt"]="$current_branch"
                if [[ -n "$base_path" && "$current_wt" == "$base_path/"* ]]; then
                    _WSS_WT_BRANCH_TO_PATH["$current_branch"]="$current_wt"
                    _WSS_WT_ORDERED_BRANCHES+=("$current_branch")
                    local sanitized
                    sanitized=$(_sanitize_branch_name "$current_branch")
                    _WSS_WT_SANITIZED_TO_REAL["$sanitized"]="$current_branch"
                fi
            fi
        elif [[ -z "$line" ]]; then
            current_wt=""
            current_branch=""
        fi
    done < <(git worktree list --porcelain 2>/dev/null || true)

    perf_timer_log "ls.bulk_worktree" "$t" "mapped=${#_WSS_WT_BRANCH_TO_PATH[@]}"
}

# ──────────────────────────────────────────────────
# Accessor API for command layer
# ──────────────────────────────────────────────────

workspace_status_session_attached() {
    printf '%s' "${_WSS_SESSION_ATTACHED[${1:-}]:-0}"
}

workspace_status_session_activity() {
    printf '%s' "${_WSS_SESSION_ACTIVITY[${1:-}]:-0}"
}

workspace_status_session_pane_count() {
    printf '%s' "${_WSS_SESSION_PANE_COUNT[${1:-}]:-0}"
}

workspace_status_session_in_project() {
    [[ "${_WSS_SESSION_IN_PROJECT[${1:-}]:-}" == "1" ]]
}

workspace_status_worktree_path_for_branch() {
    printf '%s' "${_WSS_WT_BRANCH_TO_PATH[${1:-}]:-}"
}

workspace_status_branch_for_sanitized() {
    printf '%s' "${_WSS_WT_SANITIZED_TO_REAL[${1:-}]:-}"
}

workspace_status_managed_branches() {
    local branch
    for branch in "${_WSS_WT_ORDERED_BRANCHES[@]}"; do
        printf '%s\n' "$branch"
    done
}

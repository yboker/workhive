#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Worktree Safety Checker (shared)
# Shared worktree safety checks: dirty state + unmerged commits + prominent warning box
#
# Reused by the following commands: destroy, remove, gc
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard
[[ -n "${_WORKTREE_SAFETY_SH_LOADED:-}" ]] && return 0
readonly _WORKTREE_SAFETY_SH_LOADED=1

[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

# ── Module-level scan results ──
_WT_SAFETY_DIRTY_BRANCHES=()
_WT_SAFETY_DIRTY_PATHS=()       # parallel with DIRTY_BRANCHES
_WT_SAFETY_UNMERGED_BRANCHES=()
_WT_SAFETY_UNMERGED_COUNTS=()   # parallel with UNMERGED_BRANCHES
_WT_SAFETY_HAS_WARNINGS=false
_WT_SAFETY_MAIN_BRANCH=""
_WT_SAFETY_GIT_ROOT=""

# ──────────────────────────────────────────────────
# Atomic checks (can be used standalone)
# ──────────────────────────────────────────────────

# Check if a worktree directory has dirty state (staged + unstaged + untracked)
# Args: $1 = worktree path
# Returns: 0 = dirty, 1 = clean
_worktree_is_dirty() {
    local wt_path="$1"
    [[ -d "$wt_path" ]] || return 1
    (cd "$wt_path" && {
        ! git diff --quiet 2>/dev/null ||
        ! git diff --cached --quiet 2>/dev/null ||
        [[ -n "$(git ls-files --others --exclude-standard 2>/dev/null)" ]]
    })
}

# Get count of commits on branch not merged to main
# Args: $1 = branch, $2 = git_root, $3 = main_branch
# Stdout: integer count (0 if fully merged or error)
_worktree_unmerged_count() {
    local branch="$1"
    local git_root="$2"
    local main_branch="$3"
    [[ -n "$main_branch" ]] || { echo "0"; return; }
    local count=0
    count=$(cd "$git_root" && git log "${main_branch}..${branch}" --oneline 2>/dev/null | wc -l | tr -d ' ') || true
    echo "${count:-0}"
}

# ──────────────────────────────────────────────────
# Bulk scan: check multiple worktrees at once
# ──────────────────────────────────────────────────

# Scan worktrees for dirty state and unmerged commits.
# Results stored in _WT_SAFETY_* module variables.
#
# Args: $1 = git_root
#       $2 = worktree_base (if empty, uses worktree_get_path to resolve)
#       $3... = branch names
_worktree_safety_scan() {
    local git_root="$1"
    local worktree_base="$2"
    shift 2

    # Reset results
    _WT_SAFETY_DIRTY_BRANCHES=()
    _WT_SAFETY_DIRTY_PATHS=()
    _WT_SAFETY_UNMERGED_BRANCHES=()
    _WT_SAFETY_UNMERGED_COUNTS=()
    _WT_SAFETY_HAS_WARNINGS=false
    _WT_SAFETY_GIT_ROOT="$git_root"
    _WT_SAFETY_MAIN_BRANCH=""
    _WT_SAFETY_MAIN_BRANCH=$(cd "$git_root" && _git_main_branch 2>/dev/null) || true

    local branch
    for branch in "$@"; do
        local wt_path
        if [[ -n "$worktree_base" ]]; then
            wt_path="${worktree_base}/${branch}"
        else
            wt_path=$(worktree_get_path "$branch" 2>/dev/null)
        fi

        # Dirty check
        if _worktree_is_dirty "$wt_path"; then
            _WT_SAFETY_DIRTY_BRANCHES+=("$branch")
            _WT_SAFETY_DIRTY_PATHS+=("$wt_path")
        fi

        # Unmerged check
        local uc
        uc=$(_worktree_unmerged_count "$branch" "$git_root" "$_WT_SAFETY_MAIN_BRANCH")
        if [[ "$uc" -gt 0 ]]; then
            _WT_SAFETY_UNMERGED_BRANCHES+=("$branch")
            _WT_SAFETY_UNMERGED_COUNTS+=("$uc")
        fi
    done

    if [[ ${#_WT_SAFETY_DIRTY_BRANCHES[@]} -gt 0 || ${#_WT_SAFETY_UNMERGED_BRANCHES[@]} -gt 0 ]]; then
        _WT_SAFETY_HAS_WARNINGS=true
    fi
}

# ──────────────────────────────────────────────────
# Display helpers
# ──────────────────────────────────────────────────

# Get inline warning marks for a branch (for list display).
# Stdout: colored marker string (empty if no warnings).
# Args: $1 = branch name
_worktree_safety_get_marks() {
    local branch="$1"
    local marks=""

    local i
    for i in "${!_WT_SAFETY_DIRTY_BRANCHES[@]}"; do
        if [[ "${_WT_SAFETY_DIRTY_BRANCHES[$i]}" == "$branch" ]]; then
            marks+=" ${RED}⚠️ $(_i18n WARN_WT_HAS_UNCOMMITTED)${NC}"
            break
        fi
    done

    for i in "${!_WT_SAFETY_UNMERGED_BRANCHES[@]}"; do
        if [[ "${_WT_SAFETY_UNMERGED_BRANCHES[$i]}" == "$branch" ]]; then
            marks+=" ${RED}⚠️ ${_WT_SAFETY_UNMERGED_COUNTS[$i]} $(_i18n WARN_WT_HAS_UNMERGED)${NC}"
            break
        fi
    done

    echo "$marks"
}

# Print prominent red warning box from scan results.
# No-op if no warnings found.
#
# Args: $1 = max lines per section (default 5)
#       $2 = header i18n key (default WARN_UNCOMMITTED_OR_UNMERGED_ALERT)
_worktree_safety_print_warning_box() {
    [[ "$_WT_SAFETY_HAS_WARNINGS" == true ]] || return 0

    local max_lines="${1:-5}"
    local header_key="${2:-WARN_UNCOMMITTED_OR_UNMERGED_ALERT}"

    echo -e "${RED}┌──────────────────────────────────────────────────────────┐${NC}"
    echo -e "${RED}│  ⚠️  $(_i18n "$header_key")${NC}"
    echo -e "${RED}│${NC}"

    # Dirty worktrees: file-level changes
    local i
    for i in "${!_WT_SAFETY_DIRTY_BRANCHES[@]}"; do
        local branch="${_WT_SAFETY_DIRTY_BRANCHES[$i]}"
        local wt_path="${_WT_SAFETY_DIRTY_PATHS[$i]}"
        echo -e "${RED}│${NC}  • ${BOLD}${branch}${NC} (${wt_path})"

        local changes
        changes=$(cd "$wt_path" 2>/dev/null && git status --porcelain 2>/dev/null | head -"$max_lines") || true
        if [[ -n "$changes" ]]; then
            while IFS= read -r _line; do
                [[ -z "$_line" ]] && continue
                echo -e "${RED}│${NC}      ${YELLOW}${_line}${NC}"
            done <<< "$changes"
        fi

        local total
        total=$(cd "$wt_path" 2>/dev/null && git status --porcelain 2>/dev/null | wc -l | tr -d ' ') || true
        total="${total:-0}"
        if [[ "$total" -gt "$max_lines" ]]; then
            echo -e "${RED}│${NC}      ... (+$((total - max_lines)) more)"
        fi
    done

    # Unmerged commits
    for i in "${!_WT_SAFETY_UNMERGED_BRANCHES[@]}"; do
        local branch="${_WT_SAFETY_UNMERGED_BRANCHES[$i]}"
        local uc="${_WT_SAFETY_UNMERGED_COUNTS[$i]}"

        # If already shown in dirty section, use sub-label
        local already_shown=false
        local d
        for d in "${_WT_SAFETY_DIRTY_BRANCHES[@]}"; do
            [[ "$d" == "$branch" ]] && already_shown=true && break
        done

        if [[ "$already_shown" == true ]]; then
            echo -e "${RED}│${NC}    ↳ ${uc} $(_i18n WARN_UNMERGED_COMMITS)"
        else
            echo -e "${RED}│${NC}  • ${BOLD}${branch}${NC}: ${uc} $(_i18n WARN_UNMERGED_COMMITS)"
        fi

        local _unmerged_log
        _unmerged_log=$(cd "$_WT_SAFETY_GIT_ROOT" 2>/dev/null && git log "${_WT_SAFETY_MAIN_BRANCH}..${branch}" --oneline 2>/dev/null | head -"$max_lines") || true
        if [[ -n "$_unmerged_log" ]]; then
            while IFS= read -r _line; do
                [[ -z "$_line" ]] && continue
                echo -e "${RED}│${NC}      ${YELLOW}${_line}${NC}"
            done <<< "$_unmerged_log"
        fi
        if [[ "$uc" -gt "$max_lines" ]]; then
            echo -e "${RED}│${NC}      ... (+$((uc - max_lines)) more)"
        fi
    done

    echo -e "${RED}│${NC}"
    echo -e "${RED}│  $(_i18n WARN_UNCOMMITTED_LOST)${NC}"
    echo -e "${RED}└──────────────────────────────────────────────────────────┘${NC}"
    echo ""
}

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Worktree Status Cache Service (Domain Layer)
#
# Provides short-TTL caching for expensive Git status queries
# (`git status --porcelain`, `git log -1 --format=%ct`).
#
# Design:
#   - Cache is in-memory (associative arrays), scoped to process lifetime.
#   - TTL is wall-clock based (epoch seconds).
#   - Callers may force-refresh via wt_status_cache_invalidate().
#   - watch mode reuses cache within TTL; manual `--refresh` bypasses.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_WT_STATUS_CACHE_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _WT_STATUS_CACHE_SERVICE_SH_LOADED=1

readonly _WT_STATUS_CACHE_DEFAULT_TTL=5

declare -gA _WT_STATUS_GIT_STATUS=()
declare -gA _WT_STATUS_GIT_STATUS_TS=()
declare -gA _WT_STATUS_COMMIT_EPOCH=()
declare -gA _WT_STATUS_COMMIT_EPOCH_TS=()

_wt_status_cache_now() {
    date +%s 2>/dev/null || printf '0'
}

_wt_status_cache_is_valid() {
    local ts="${1:-0}"
    local ttl="${2:-$_WT_STATUS_CACHE_DEFAULT_TTL}"
    local now
    now=$(_wt_status_cache_now)
    (( now - ts < ttl ))
}

wt_status_cache_invalidate() {
    _WT_STATUS_GIT_STATUS=()
    _WT_STATUS_GIT_STATUS_TS=()
    _WT_STATUS_COMMIT_EPOCH=()
    _WT_STATUS_COMMIT_EPOCH_TS=()
}

# Get cached or fresh git status summary for a worktree path.
# Args: $1 = worktree_path, $2 = ttl (optional)
# Output: "clean" | "3M 1A 2?" | "-"
wt_status_cache_git_status() {
    local wt_path="$1"
    local ttl="${2:-$_WT_STATUS_CACHE_DEFAULT_TTL}"

    if [[ -n "${_WT_STATUS_GIT_STATUS[$wt_path]+_}" ]]; then
        if _wt_status_cache_is_valid "${_WT_STATUS_GIT_STATUS_TS[$wt_path]:-0}" "$ttl"; then
            printf '%s' "${_WT_STATUS_GIT_STATUS[$wt_path]}"
            return 0
        fi
    fi

    local result
    result=$(_wt_status_compute_git_status "$wt_path")
    _WT_STATUS_GIT_STATUS["$wt_path"]="$result"
    _WT_STATUS_GIT_STATUS_TS["$wt_path"]=$(_wt_status_cache_now)
    printf '%s' "$result"
}

# Get cached or fresh last-commit epoch for a worktree path or branch.
# Args: $1 = worktree_path (or "-" for non-worktree), $2 = branch, $3 = project_root, $4 = ttl
# Output: epoch seconds or "0"
wt_status_cache_commit_epoch() {
    local wt_path="$1"
    local branch="${2:-}"
    local project_root="${3:-}"
    local ttl="${4:-$_WT_STATUS_CACHE_DEFAULT_TTL}"

    local cache_key="${wt_path}:${branch}"
    if [[ -n "${_WT_STATUS_COMMIT_EPOCH[$cache_key]+_}" ]]; then
        if _wt_status_cache_is_valid "${_WT_STATUS_COMMIT_EPOCH_TS[$cache_key]:-0}" "$ttl"; then
            printf '%s' "${_WT_STATUS_COMMIT_EPOCH[$cache_key]}"
            return 0
        fi
    fi

    local result
    result=$(_wt_status_compute_commit_epoch "$wt_path" "$branch" "$project_root")
    _WT_STATUS_COMMIT_EPOCH["$cache_key"]="$result"
    _WT_STATUS_COMMIT_EPOCH_TS["$cache_key"]=$(_wt_status_cache_now)
    printf '%s' "$result"
}

# ──────────────────────────────────────────────────
# Computation helpers (no caching, pure I/O)
# ──────────────────────────────────────────────────

_wt_status_compute_git_status() {
    local wt_path="$1"
    [[ -d "$wt_path" ]] || { printf '-'; return 0; }

    local status_output
    status_output=$(git -C "$wt_path" status --porcelain 2>/dev/null) || { printf '-'; return 0; }
    [[ -z "$status_output" ]] && { printf 'clean'; return 0; }

    local modified=0 added=0 deleted=0 untracked=0
    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        local xy="${line:0:2}"
        case "$xy" in
            "??"*) untracked=$((untracked + 1)) ;;
            *M*|*m*) modified=$((modified + 1)) ;;
            *A*|*a*) added=$((added + 1)) ;;
            *D*|*d*) deleted=$((deleted + 1)) ;;
            *) modified=$((modified + 1)) ;;
        esac
    done <<< "$status_output"

    local parts=()
    [[ $modified -gt 0 ]] && parts+=("${modified}M")
    [[ $added -gt 0 ]] && parts+=("${added}A")
    [[ $deleted -gt 0 ]] && parts+=("${deleted}D")
    [[ $untracked -gt 0 ]] && parts+=("${untracked}?")

    if [[ ${#parts[@]} -eq 0 ]]; then
        printf 'clean'
    else
        local IFS=' '
        printf '%s' "${parts[*]}"
    fi
}

_wt_status_compute_commit_epoch() {
    local wt_path="$1"
    local branch="${2:-}"
    local project_root="${3:-}"

    local epoch=""
    if [[ -d "$wt_path" && "$wt_path" != "-" ]]; then
        epoch=$(git -C "$wt_path" log -1 --format=%ct 2>/dev/null || true)
    elif [[ -n "$branch" && -n "$project_root" ]]; then
        epoch=$(git -C "$project_root" log -1 --format=%ct "$branch" 2>/dev/null || true)
    fi

    if [[ -z "$epoch" || ! "$epoch" =~ ^[0-9]+$ || "$epoch" -le 0 ]]; then
        printf '0'
    else
        printf '%s' "$epoch"
    fi
}

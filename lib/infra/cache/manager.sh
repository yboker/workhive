#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Cache Manager
# High-performance in-memory cache layer
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_CACHE_MANAGER_SH_LOADED:-}" ]] && return 0
readonly _CACHE_MANAGER_SH_LOADED=1

# Standard TTL constants (seconds) — single source of truth (used by callers that source this)
# shellcheck disable=SC2034
readonly CACHE_TTL_SESSION_EXISTS=5
# shellcheck disable=SC2034
readonly CACHE_TTL_SESSION_LIST=10
# shellcheck disable=SC2034
readonly CACHE_TTL_WORKTREE_LIST=10
# shellcheck disable=SC2034
readonly CACHE_TTL_CONFIG_VALUE=60
# shellcheck disable=SC2034
readonly CACHE_TTL_CONFIG_FILE=300

# Global cache storage (associative arrays)
declare -gA WORKHIVE_CACHE
declare -gA WORKHIVE_CACHE_TTL

# Get from cache
# Returns: 0 = hit, 1 = miss
cache_get() {
    local key="$1"

    # Check if key exists
    if [[ -z "${WORKHIVE_CACHE[$key]+_}" ]]; then
        return 1
    fi

    # Check if expired
    local ttl="${WORKHIVE_CACHE_TTL[$key]:-0}"
    if [[ "$ttl" -gt 0 ]]; then
        local now
        printf -v now '%(%s)T' -1
        if [[ "$now" -gt "$ttl" ]]; then
            # Expired, delete and return miss
            unset "WORKHIVE_CACHE[$key]"
            unset "WORKHIVE_CACHE_TTL[$key]"
            return 1
        fi
    fi

    # Hit, output value
    printf '%s' "${WORKHIVE_CACHE[$key]}"
    return 0
}

# Set cache entry
cache_set() {
    local key="$1"
    local value="$2"
    local ttl_seconds="${3:-0}"  # 0 = never expires

    WORKHIVE_CACHE["$key"]="$value"

    if [[ "$ttl_seconds" -gt 0 ]]; then
        local now expire_at
        printf -v now '%(%s)T' -1
        expire_at=$(( now + ttl_seconds ))
        WORKHIVE_CACHE_TTL["$key"]="$expire_at"
    else
        WORKHIVE_CACHE_TTL["$key"]=0
    fi
}

# Delete cache entry
cache_delete() {
    local key="$1"
    unset "WORKHIVE_CACHE[$key]"
    unset "WORKHIVE_CACHE_TTL[$key]"
}

# Batch delete (supports wildcards)
cache_invalidate() {
    local pattern="$1"
    # Collect keys to delete (avoid modifying array during iteration)
    local keys_to_delete=()
    local key
    for key in "${!WORKHIVE_CACHE[@]}"; do
        # shellcheck disable=SC2053
        if [[ "$key" == $pattern ]]; then
            keys_to_delete+=("$key")
        fi
    done
    for key in "${keys_to_delete[@]}"; do
        cache_delete "$key"
    done
}

# Clear all cache entries
cache_clear() {
    WORKHIVE_CACHE=()
    WORKHIVE_CACHE_TTL=()
}

# Proactively clean up expired cache entries
cache_gc() {
    local now
    printf -v now '%(%s)T' -1
    local keys_to_delete=()
    local key
    for key in "${!WORKHIVE_CACHE_TTL[@]}"; do
        local ttl="${WORKHIVE_CACHE_TTL[$key]}"
        if [[ "$ttl" -gt 0 && "$now" -gt "$ttl" ]]; then
            keys_to_delete+=("$key")
        fi
    done
    for key in "${keys_to_delete[@]}"; do
        unset "WORKHIVE_CACHE[$key]"
        unset "WORKHIVE_CACHE_TTL[$key]"
    done
    echo "${#keys_to_delete[@]}"
    return 0
}

# Get cache statistics
cache_stats() {
    local total=${#WORKHIVE_CACHE[@]}
    echo "Cache entries: $total"

    if [[ "$total" -gt 0 ]]; then
        echo "Cache keys:"
        local key
        for key in "${!WORKHIVE_CACHE[@]}"; do
            echo "  - $key"
        done
    fi
}

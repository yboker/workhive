#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Platform Path Utilities
# Cross-platform path utilities
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_PLATFORM_PATH_SH_LOADED:-}" ]] && return 0
readonly _PLATFORM_PATH_SH_LOADED=1

source "${LIB_DIR}/infra/platform/detector.sh"

# Normalize path (handle cross-platform differences)
path_normalize() {
    local path="$1"
    local platform="${2:-$(platform_detect)}"

    case "$platform" in
        windows-git-bash)
            # Git Bash requires conversion to Windows path format
            if command -v cygpath &>/dev/null; then
                cygpath -w "$path" 2>/dev/null || echo "$path"
            else
                echo "$path"
            fi
            ;;
        *)
            # Unix-style path, return directly
            echo "$path"
            ;;
    esac
}

# Convert path to Unix-style (for internal processing)
path_to_unix() {
    local path="$1"
    local platform="${2:-$(platform_detect)}"

    case "$platform" in
        windows-git-bash)
            if command -v cygpath &>/dev/null; then
                cygpath -u "$path" 2>/dev/null || echo "$path"
            else
                # Simple replacement
                echo "${path//\\//}"
            fi
            ;;
        *)
            echo "$path"
            ;;
    esac
}

# Join paths (cross-platform safe)
path_join() {
    local result=""
    local part
    for part in "$@"; do
        if [[ -z "$result" ]]; then
            result="$part"
        else
            result="${result%/}/${part#/}"
        fi
    done
    echo "$result"
}

# Get absolute path
path_absolute() {
    local path="$1"

    if [[ "$path" = /* ]]; then
        echo "$path"
    else
        echo "$(pwd)/$path"
    fi
}

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Platform Detection Module
# Cross-platform detection and abstraction layer
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_PLATFORM_DETECTOR_SH_LOADED:-}" ]] && return 0
readonly _PLATFORM_DETECTOR_SH_LOADED=1

# Detect the current running platform
# Returns: macos | linux | wsl2 | windows-git-bash | unknown
platform_detect() {
    if [[ -n "${WORKHIVE_PLATFORM:-}" ]]; then
        echo "$WORKHIVE_PLATFORM"
        return 0
    fi

    case "$(uname -s)" in
        Darwin*)
            echo "macos"
            ;;
        Linux*)
            # Detect if running under WSL2
            if grep -qi microsoft /proc/version 2>/dev/null; then
                echo "wsl2"
            else
                echo "linux"
            fi
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "windows-git-bash"
            ;;
        *)
            echo "unknown"
            ;;
    esac
}

# Check if the platform is supported
platform_is_supported() {
    local platform="${1:-$(platform_detect)}"

    case "$platform" in
        macos|linux|wsl2|windows-git-bash)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

# Get platform-friendly display name
platform_get_name() {
    local platform="${1:-$(platform_detect)}"

    case "$platform" in
        macos)              echo "macOS" ;;
        linux)              echo "Linux" ;;
        wsl2)               echo "Windows Subsystem for Linux 2" ;;
        windows-git-bash)   echo "Windows (Git Bash)" ;;
        *)                  echo "Unknown Platform" ;;
    esac
}

# Detect platform feature support
platform_supports_feature() {
    local feature="$1"
    local platform="${2:-$(platform_detect)}"

    case "$feature" in
        tmux)
            [[ "$platform" == "macos" || "$platform" == "linux" || "$platform" == "wsl2" ]]
            ;;
        screen)
            true  # screen is supported on almost all platforms
            ;;
        native-symlinks)
            [[ "$platform" != "windows-git-bash" ]]
            ;;
        *)
            return 1
            ;;
    esac
}

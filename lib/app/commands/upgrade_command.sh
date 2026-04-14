#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Upgrade Command
# Fetch latest version from GitHub and update (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_UPGRADE_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _UPGRADE_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/tui.sh"

# ──────────────────────────────────────────────────
# Constants
# ──────────────────────────────────────────────────
readonly _UPGRADE_REPO_OWNER="yboker"
readonly _UPGRADE_REPO_NAME="workhive"
readonly _UPGRADE_REPO_URL="https://github.com/${_UPGRADE_REPO_OWNER}/${_UPGRADE_REPO_NAME}"

# Show usage help
upgrade_command_help() {
    _help_dispatch "_upgrade_command_help"
}

# ──────────────────────────────────────────────────
# Core logic
# ──────────────────────────────────────────────────

_upgrade_fetch_url() {
    local url="$1"
    check_cmd curl || return 1
    curl -fsSL --connect-timeout 5 "$url" 2>/dev/null
}

# Get remote latest version number
# Output: version number (e.g. "0.3.0") or empty string
_upgrade_get_remote_version() {
    local remote_version=""

    # Method 1: Get latest release tag via GitHub API
    remote_version=$(_upgrade_fetch_url \
        "https://api.github.com/repos/${_UPGRADE_REPO_OWNER}/${_UPGRADE_REPO_NAME}/releases/latest" \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"//;s/".*//' || echo "")

    # Method 2: If no release exists, get version from loader.sh on main branch
    if [[ -z "$remote_version" ]]; then
        remote_version=$(_upgrade_fetch_url \
            "https://raw.githubusercontent.com/${_UPGRADE_REPO_OWNER}/${_UPGRADE_REPO_NAME}/main/lib/infra/config/loader.sh" \
            | grep 'WORKHIVE_VERSION=' | head -1 | sed 's/.*WORKHIVE_VERSION="//;s/".*//' || echo "")
    fi

    # Strip 'v' prefix
    remote_version="${remote_version#v}"
    echo "$remote_version"
}

# Compare version numbers
# Returns: 0 = update needed, 1 = already up-to-date or higher
_upgrade_version_gt() {
    local remote="$1"
    local local_ver="$2"

    if [[ "$remote" == "$local_ver" ]]; then
        return 1
    fi

    # Use sort -V for semantic version comparison
    local highest
    highest=$(printf '%s\n%s\n' "$remote" "$local_ver" | sort -V | tail -1)
    [[ "$highest" == "$remote" ]]
}

_upgrade_is_valid_semver() {
    local version="$1"
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

_upgrade_require_valid_version() {
    local version="$1"
    local err_key="$2"
    if ! _upgrade_is_valid_semver "$version"; then
        log_error "$(_i18n_fmt "$err_key" "$version")"
        return 1
    fi
    return 0
}

_upgrade_resolve_workhive_bin() {
    resolve_workhive_bin
}

_upgrade_resolve_real_path() {
    local path="$1"
    resolve_realpath_fallback "$path"
}

_upgrade_resolve_install_prefix_from_bin() {
    local workhive_bin="$1"
    [[ -n "$workhive_bin" ]] || return 1

    local real_path
    real_path=$(_upgrade_resolve_real_path "$workhive_bin")
    local bin_dir
    bin_dir=$(dirname "$real_path")
    dirname "$bin_dir"
}

# Detect current installation type
# Output: "git" | "installed" | "development"
_upgrade_detect_install_type() {
    local workhive_bin
    workhive_bin=$(_upgrade_resolve_workhive_bin)

    if [[ -z "$workhive_bin" ]]; then
        echo "development"
        return
    fi

    # Resolve real path (follow symbolic links)
    local parent_dir
    parent_dir=$(_upgrade_resolve_install_prefix_from_bin "$workhive_bin")

    # Check if in a git repository (development environment)
    if [[ -d "${parent_dir}/.git" ]]; then
        echo "git"
        return
    fi

    # Check if installed via install.sh (in ~/.local or /usr/local, etc.)
    if [[ -d "${parent_dir}/lib/workhive" ]]; then
        echo "installed"
        return
    fi

    echo "development"
}

# Upgrade via git pull (development environment)
_upgrade_via_git() {
    local force="$1"
    local target_version="$2"

    local git_root
    git_root=$(cd "$ROOT_DIR" && git rev-parse --show-toplevel 2>/dev/null)

    if [[ -z "$git_root" ]]; then
        log_error "$(_i18n ERR_UPGRADE_NOT_GIT)"
        return 1
    fi

    log_info "$(_i18n MSG_UPGRADE_GIT_PULL)..."

    cd "$git_root" || return 1

    # Check for uncommitted changes
    if ! git diff --quiet 2>/dev/null || ! git diff --cached --quiet 2>/dev/null; then
        log_warn "$(_i18n WARN_UPGRADE_UNCOMMITTED)"
        if ! ask_yes_no "$(_i18n MSG_UPGRADE_CONTINUE_ANYWAY)" "N"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
        # Stash uncommitted changes
        git stash push -m "workhive-upgrade-$(date +%Y%m%d%H%M%S)" 2>/dev/null
        log_info "$(_i18n MSG_UPGRADE_STASHED)"
    fi

    # Pull latest code
    if [[ -n "$target_version" ]]; then
        git fetch --tags 2>/dev/null
        if ! git checkout "v${target_version}" 2>/dev/null && ! git checkout "${target_version}" 2>/dev/null; then
            log_error "$(_i18n ERR_UPGRADE_VERSION_NOT_FOUND) ${target_version}"
            return 1
        fi
    else
        if ! git pull --ff-only origin main 2>/dev/null; then
            log_warn "$(_i18n WARN_UPGRADE_FF_FAILED)"
            if ! git pull --rebase origin main 2>/dev/null; then
                log_error "$(_i18n ERR_UPGRADE_PULL_FAILED)"
                return 1
            fi
        fi
    fi

    return 0
}

# Upgrade via reinstall (install.sh installation method)
_upgrade_via_reinstall() {
    local force="$1"
    local target_version="$2"

    local workhive_bin
    workhive_bin=$(_upgrade_resolve_workhive_bin)
    local prefix
    prefix=$(_upgrade_resolve_install_prefix_from_bin "$workhive_bin")

    log_info "$(_i18n MSG_UPGRADE_REINSTALL) ${prefix}..."

    # Validate version format if specified
    if [[ -n "$target_version" ]]; then
        _upgrade_require_valid_version "$target_version" "UPGRADE_ERR_INVALID_VERSION_FMT" || return 1
    fi

    # Build install args as array (safe against word-splitting)
    local -a install_args=("--prefix" "$prefix")
    [[ -n "$target_version" ]] && install_args+=("--version" "v${target_version}")

    local install_script="${ROOT_DIR}/install.sh"
    if [[ -f "$install_script" ]]; then
        bash "$install_script" "${install_args[@]}"
    else
        local tmp_installer
        tmp_installer=$(mktemp "${TMPDIR:-/tmp}/workhive-upgrade.XXXXXX") || {
            log_error "$(_i18n ERR_UPGRADE_DOWNLOAD_FAILED)"
            return 1
        }
        chmod 0700 "$tmp_installer"

        local install_rc=0
        if curl -fsSL "${_UPGRADE_REPO_URL}/raw/main/install.sh" -o "$tmp_installer" 2>/dev/null; then
            bash "$tmp_installer" "${install_args[@]}" || install_rc=$?
        else
            log_error "$(_i18n ERR_UPGRADE_DOWNLOAD_FAILED)"
            install_rc=1
        fi
        rm -f -- "$tmp_installer"
        return "$install_rc"
    fi
}

# ──────────────────────────────────────────────────
# Execute upgrade command
# ──────────────────────────────────────────────────

upgrade_command_execute() {
    local check_only=false
    local target_version=""
    local force=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                upgrade_command_help
                return 0
                ;;
            --check)
                check_only=true
                shift
                ;;
            --version)
                shift
                target_version="${1:-}"
                [[ -z "$target_version" ]] && {
                    log_error "$(_i18n ERR_MISSING_VERSION)"
                    return 1
                }
                target_version="${target_version#v}"
                _upgrade_require_valid_version "$target_version" "UPGRADE_ERR_INVALID_VERSION_EXAMPLE_FMT" || return 1
                shift
                ;;
            --force|-f)
                force=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                upgrade_command_help
                return 1
                ;;
            *)
                cli_log_unknown_opt "$1"
                return 1
                ;;
        esac
    done

    local current_version="${WORKHIVE_VERSION}"
    log_info "$(_i18n MSG_UPGRADE_CURRENT) v${current_version}"

    # Get remote version
    log_info "$(_i18n MSG_UPGRADE_CHECKING)..."
    local remote_version
    if [[ -n "$target_version" ]]; then
        remote_version="$target_version"
    else
        remote_version=$(_upgrade_get_remote_version)
    fi

    if [[ -z "$remote_version" ]]; then
        log_warn "$(_i18n WARN_UPGRADE_CANNOT_CHECK)"
        log_info "$(_i18n MSG_UPGRADE_MANUAL)"
        echo "  ${_UPGRADE_REPO_URL}/releases"
        return 1
    fi

    log_info "$(_i18n MSG_UPGRADE_REMOTE) v${remote_version}"

    # Version comparison
    if [[ "$force" != true ]] && ! _upgrade_version_gt "$remote_version" "$current_version"; then
        log_success "$(_i18n MSG_UPGRADE_UP_TO_DATE) (v${current_version})"
        return 0
    fi

    if [[ "$check_only" == true ]]; then
        if _upgrade_version_gt "$remote_version" "$current_version"; then
            log_bold "🆕 $(_i18n MSG_UPGRADE_AVAILABLE): v${current_version} → v${remote_version}"
            echo ""
            log_info "$(_i18n MSG_UPGRADE_RUN_CMD)"
            echo "  workhive upgrade"
        else
            log_success "$(_i18n MSG_UPGRADE_UP_TO_DATE) (v${current_version})"
        fi
        return 0
    fi

    # Confirm upgrade
    log_bold "📦 $(_i18n MSG_UPGRADE_WILL_UPDATE): v${current_version} → v${remote_version}"
    if ! ask_yes_no "$(_i18n MSG_UPGRADE_CONFIRM)" "Y"; then
        log_info "$(_i18n MSG_CANCELLED)"
        return 0
    fi

    # Detect installation type and perform upgrade
    local install_type
    install_type=$(_upgrade_detect_install_type)
    log_debug "Install type: $install_type"

    case "$install_type" in
        git)
            _upgrade_via_git "$force" "$target_version"
            ;;
        installed)
            _upgrade_via_reinstall "$force" "$target_version"
            ;;
        development)
            log_warn "$(_i18n WARN_UPGRADE_DEV_MODE)"
            log_info "$(_i18n MSG_UPGRADE_DEV_HINT)"
            echo "  cd ${ROOT_DIR} && git pull"
            return 0
            ;;
    esac

    local rc=$?
    if [[ $rc -eq 0 ]]; then
        # Reload version number (from updated code)
        local new_version
        new_version=$(grep 'WORKHIVE_VERSION=' "${LIB_DIR}/infra/config/loader.sh" 2>/dev/null \
            | head -1 | sed 's/.*WORKHIVE_VERSION="//;s/".*//')
        [[ -z "$new_version" ]] && new_version="$remote_version"

        echo ""
        log_success "$(_i18n MSG_UPGRADE_COMPLETE): v${current_version} → v${new_version}"
    else
        log_error "$(_i18n ERR_UPGRADE_FAILED)"
        return 1
    fi
}

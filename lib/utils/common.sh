#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Common Utilities
# Core utility functions library
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_COMMON_SH_LOADED:-}" ]] && return 0
readonly _COMMON_SH_LOADED=1

# ──────────────────────────────────────────────────
# Color definitions
# ──────────────────────────────────────────────────
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BLUE='\033[0;34m'
readonly NC='\033[0m'
readonly BOLD='\033[1m'

# Centralized logging module
source "${LIB_DIR}/infra/logging/manager.sh"

# ──────────────────────────────────────────────────
# Error handling
# ──────────────────────────────────────────────────
die() {
    log_error "$1"
    exit 1
}

# ──────────────────────────────────────────────────
# User interaction
# ──────────────────────────────────────────────────
ask_yes_no() {
    local prompt="$1"
    local default="${2:-Y}"
    local response
    local _prev_int_trap
    local _ask_interrupted=false

    _ASK_YES_NO_NONINTERACTIVE=false

    # Non-interactive terminal: use default value
    if [[ ! -t 0 ]]; then
        _ASK_YES_NO_NONINTERACTIVE=true
        [[ "$default" =~ ^[Yy]$ ]]
        return $?
    fi

    if [[ "$default" == "Y" ]]; then
        prompt="$prompt [Y/n]: "
    else
        prompt="$prompt [y/N]: "
    fi

    _prev_int_trap="$(trap -p INT || true)"
    trap '_ask_interrupted=true' INT

    # Print prompt once, then poll read with timeout so Ctrl+C can be observed
    # immediately via the INT trap (without requiring an extra Enter keypress).
    printf "%b" "${BOLD}${prompt}${NC}"
    while true; do
        if read -r -t 0.1 response; then
            break
        else
            local read_rc=$?
            if [[ "$_ask_interrupted" == true ]]; then
                echo ""
                if [[ -n "$_prev_int_trap" ]]; then
                    eval "$_prev_int_trap"
                else
                    trap - INT
                fi
                return 130
            fi
            # EOF / closed stdin
            if [[ $read_rc -eq 1 ]]; then
                if [[ -n "$_prev_int_trap" ]]; then
                    eval "$_prev_int_trap"
                else
                    trap - INT
                fi
                return 1
            fi
        fi
    done

    if [[ "$_ask_interrupted" == true ]]; then
        echo ""
        if [[ -n "$_prev_int_trap" ]]; then
            eval "$_prev_int_trap"
        else
            trap - INT
        fi
        return 130
    fi

    if [[ -n "$_prev_int_trap" ]]; then
        eval "$_prev_int_trap"
    else
        trap - INT
    fi

    response="${response:-$default}"

    [[ "$response" =~ ^[Yy]$ ]]
}

confirm_action() {
    local prompt="$1"
    local force="${2:-false}"
    local default="${3:-N}"
    local cancel_rc="${4:-1}"

    _ASK_YES_NO_NONINTERACTIVE=false

    if [[ "$force" == true ]]; then
        return 0
    fi

    ask_yes_no "$prompt" "$default"
    local ask_rc=$?
    if [[ $ask_rc -eq 130 ]]; then
        return 130
    fi
    if [[ $ask_rc -ne 0 ]]; then
        log_info "$(_i18n MSG_CANCELLED)"
        return "$cancel_rc"
    fi

    return 0
}

cli_is_help_flag() {
    case "${1:-}" in
        -h|--help|help) return 0 ;;
        *) return 1 ;;
    esac
}

# Enforce no extra args for a subcommand and provide consistent help behavior.
# Args:
#   $1 usage text (printed on help/validation error)
#   $2 command label for positional-arg error message
#   $@ remaining args to validate
# Returns:
#   0: valid (no extra args)
#   1: invalid args
#   2: help printed
cli_expect_no_args() {
    local usage="$1"
    local cmd_label="$2"
    shift 2 || true

    if [[ $# -eq 0 ]]; then
        return 0
    fi

    local first_arg="${1:-}"
    if cli_is_help_flag "$first_arg"; then
        echo "$usage"
        return 2
    fi

    if [[ "$first_arg" == -* ]]; then
        log_error "$(_i18n ERR_UNKNOWN_OPT) $first_arg"
    else
        log_error "${cmd_label} $(_i18n ERR_NO_POSITIONAL)"
    fi
    echo "$usage"
    return 1
}

cli_log_unknown_opt() {
    local opt="${1:-}"
    log_error "$(_i18n ERR_UNKNOWN_OPT) $opt"
}

cli_log_no_positional() {
    local scope="${1:-}"
    if [[ -n "$scope" ]]; then
        log_error "$scope $(_i18n ERR_NO_POSITIONAL)"
    else
        log_error "$(_i18n ERR_NO_POSITIONAL)"
    fi
}

# Validate generic option value and return it via stdout.
# Args:
#   $1 option name (for error message)
#   $2 candidate value
# Returns: 0 valid, 1 invalid (with user-facing error)
cli_require_option_value() {
    local opt="$1"
    local value="${2:-}"
    if [[ -z "$value" ]]; then
        log_error "$(_i18n_fmt ERR_MISSING_VALUE_FOR_OPT_FMT "$opt")"
        return 1
    fi
    printf '%s\n' "$value"
    return 0
}

# Validate option value and reject option-like values (starting with '-').
# Args:
#   $1 option name (for error message)
#   $2 candidate value
# Returns: 0 valid, 1 invalid (with user-facing error)
cli_require_option_value_nonflag() {
    local opt="$1"
    local value="${2:-}"
    if [[ -z "$value" || "$value" == -* ]]; then
        log_error "$(_i18n_fmt ERR_MISSING_VALUE_FOR_OPT_FMT "$opt")"
        return 1
    fi
    printf '%s\n' "$value"
    return 0
}

# Validate enum-style --format value and return normalized value via stdout.
# Args:
#   $1 candidate value
#   $2 regex (e.g. '^(table|json)$')
#   $3 display values text (e.g. 'table, json')
# Returns: 0 valid, 1 invalid (with user-facing error)
cli_parse_format_value() {
    local value="${1:-}"
    local allowed_regex="$2"
    local allowed_display="$3"

    if [[ -z "$value" || "$value" == -* ]]; then
        log_error "$(_i18n ERR_INVALID_FORMAT) '' (${allowed_display})"
        return 1
    fi
    if [[ ! "$value" =~ $allowed_regex ]]; then
        log_error "$(_i18n ERR_INVALID_FORMAT) ${value} (${allowed_display})"
        return 1
    fi
    printf '%s\n' "$value"
    return 0
}

# Validate enum-style --format value and render error via i18n template key.
# Args:
#   $1 candidate value
#   $2 regex (e.g. '^(table|json)$')
#   $3 i18n key template that accepts one %s argument for value
# Returns: 0 valid, 1 invalid (with user-facing error)
cli_parse_format_value_i18n() {
    local value="${1:-}"
    local allowed_regex="$2"
    local err_key="$3"

    if [[ -z "$value" || "$value" == -* ]]; then
        log_error "$(_i18n_fmt "$err_key" "")"
        return 1
    fi
    if [[ ! "$value" =~ $allowed_regex ]]; then
        log_error "$(_i18n_fmt "$err_key" "$value")"
        return 1
    fi
    printf '%s\n' "$value"
    return 0
}


# ──────────────────────────────────────────────────
# File operations
# ──────────────────────────────────────────────────
cleanup_file_quiet() {
    local target="${1:-}"
    [[ -z "$target" ]] && return 0
    rm -f -- "$target" 2>/dev/null || true
}

cleanup_dir_quiet() {
    local target="${1:-}"
    [[ -z "$target" ]] && return 0
    rm -rf -- "$target" 2>/dev/null || true
}

cleanup_stale_files_by_name() {
    local target_dir="${1:-}"
    local stale_minutes="${2:-}"
    local name_pattern="${3:-}"

    [[ -d "$target_dir" ]] || return 0
    [[ "$stale_minutes" =~ ^[0-9]+$ ]] || return 0
    [[ -n "$name_pattern" ]] || return 0

    find "$target_dir" -maxdepth 1 -type f -name "$name_pattern" -mmin "+$stale_minutes" -exec rm -f -- {} + 2>/dev/null || true
}

ensure_dir() {
    [[ -z "$1" ]] && return 1
    if [[ -d "$1" ]]; then
        return 0
    fi
    if ! mkdir -p "$1" 2>/dev/null; then
        log_error "$(_i18n_fmt ERR_CANNOT_CREATE_DIR_FMT "$1")"
        return 1
    fi
}

# ──────────────────────────────────────────────────
# Safe rm -rf wrapper (rejects empty, root, home paths)
# ──────────────────────────────────────────────────
safe_rm() {
    local target="$1"
    if [[ -z "$target" ]]; then
        log_error "$(_i18n ERR_SAFE_RM_EMPTY)"
        return 1
    fi
    if [[ "$target" == "." || "$target" == "./" ]]; then
        log_error "$(_i18n_fmt ERR_SAFE_RM_DANGEROUS_FMT "$target" "$target")"
        return 1
    fi

    # Reject parent-traversal path segments defensively.
    # Even though callers should pass normalized paths, block lexical ".."
    # components to prevent accidental directory escape on malformed input.
    if [[ "$target" == ".." || "$target" == ../* || "$target" == */.. || "$target" == */../* ]]; then
        log_error "$(_i18n_fmt ERR_SAFE_RM_DANGEROUS_FMT "$target" "$target")"
        return 1
    fi

    # Resolve symlinks to protect against symlink traversal attacks
    local resolved_target="$target"
    if [[ -e "$target" ]]; then
        if command -v realpath &>/dev/null; then
            resolved_target=$(realpath "$target" 2>/dev/null) || resolved_target="$target"
        elif command -v readlink &>/dev/null; then
            resolved_target=$(readlink -f "$target" 2>/dev/null) || resolved_target="$target"
        fi
    fi

    if [[ "$resolved_target" == ".." || "$resolved_target" == ../* || "$resolved_target" == */.. || "$resolved_target" == */../* ]]; then
        log_error "$(_i18n_fmt ERR_SAFE_RM_DANGEROUS_FMT "$target" "$resolved_target")"
        return 1
    fi

    # Reject dangerous system paths
    local _dangerous_paths=("/" "$HOME" "$HOME/" "/usr" "/etc" "/var" "/tmp" "/bin" "/sbin" "/opt" "/lib" "/boot" "/dev" "/proc" "/sys" "/root")
    local _dpath
    for _dpath in "${_dangerous_paths[@]}"; do
        if [[ "$resolved_target" == "$_dpath" || "$target" == "$_dpath" ]]; then
            log_error "$(_i18n_fmt ERR_SAFE_RM_DANGEROUS_FMT "$target" "$resolved_target")"
            return 1
        fi
    done

    rm -rf -- "$target"
}

# ──────────────────────────────────────────────────
# Branch name sanitization
# Normalize branch names for use in session names, paths, etc.
# Converts / . : to - for tmux/filesystem compatibility
# ──────────────────────────────────────────────────
_sanitize_branch_name() {
    local branch="$1"
    branch="${branch//\//-}"
    branch="${branch//./-}"
    branch="${branch//:/-}"
    branch="${branch// /-}"
    branch="${branch//\$/-}"
    branch="${branch//\`/-}"
    branch="${branch//!/-}"
    branch="${branch//#/-}"
    branch="${branch//[~]/-}"
    branch="${branch//^/-}"
    branch="${branch//\*/-}"
    branch="${branch//\?/-}"
    branch="${branch//[/-}"
    branch="${branch//]/-}"
    printf '%s' "$branch"
}

# Sanitize project name for tmux session use.
# Same as _sanitize_branch_name but preserves / (path hierarchy).
_sanitize_project_name() {
    local name="$1"
    name="${name//./-}"
    name="${name//:/-}"
    name="${name// /-}"
    name="${name//\$/-}"
    name="${name//\`/-}"
    name="${name//!/-}"
    name="${name//#/-}"
    name="${name//[~]/-}"
    name="${name//^/-}"
    name="${name//\*/-}"
    name="${name//\?/-}"
    name="${name//[/-}"
    name="${name//]/-}"
    printf '%s' "$name"
}

# Check if branch name needs sanitization.
# Returns 0 if sanitized == original (no conversion needed), 1 if different.
_branch_needs_sanitize() {
    local branch="$1"
    local sanitized
    sanitized=$(_sanitize_branch_name "$branch")
    [[ "$branch" != "$sanitized" ]]
}

# Prompt user to confirm branch name conversion.
# Sets BRANCH_SANITIZED_NAME to the sanitized result.
# Returns 0 if user confirms, 1 if user cancels.
_branch_confirm_sanitize() {
    local branch="$1"
    local sanitized
    sanitized=$(_sanitize_branch_name "$branch")
    # shellcheck disable=SC2034
    BRANCH_SANITIZED_NAME="$sanitized"

    if [[ "$branch" == "$sanitized" ]]; then
        return 0
    fi

    local msg
    msg=$(_i18n_fmt WARN_BRANCH_NAME_SANITIZED "$branch" "$sanitized")
    log_warn "$msg"
    echo ""
    if ! ask_yes_no "$(_i18n ASK_BRANCH_NAME_CONFIRM)" "Y"; then
        return 1
    fi
    echo ""
    return 0
}

# ──────────────────────────────────────────────────
# Git helpers: current branch
# ──────────────────────────────────────────────────
_git_current_branch() {
    git branch --show-current 2>/dev/null || echo ""
}

# ──────────────────────────────────────────────────
# Dangerous directory detection
# Minimal hard guard for git init safety.
# Only root ("/") is hard-blocked here; other dangerous directories are
# configured via git_init.dangerous_paths_extra (auto-generated defaults).
# Returns: 0 = dangerous, 1 = safe
# ──────────────────────────────────────────────────
_is_dangerous_git_root() {
    local dir="$1"
    [[ -z "$dir" ]] && dir="$(pwd)"

    local resolved
    resolved=$(cd "$dir" 2>/dev/null && pwd -P) || resolved="$dir"

    case "$resolved" in
        /)                                  return 0 ;;
    esac

    return 1
}

# Generate a list of system-specific dangerous directories for git_init defaults.
# Outputs one path per line. Used during global config generation.
_get_system_dangerous_paths() {
    local home_resolved
    home_resolved=$(cd "$HOME" 2>/dev/null && pwd -P) || home_resolved="$HOME"

    # NOTE: "/" is hard-blocked by _is_dangerous_git_root() and therefore
    # intentionally omitted from dangerous_paths_extra defaults.
    # Other default dangerous directories (including "$HOME") are generated
    # here so users can review and customize them in global config.
    local paths=()
    paths+=("$home_resolved")
    paths+=("/tmp")
    paths+=("/var")
    paths+=("/usr")
    paths+=("/etc")
    paths+=("/opt")

    case "$(uname -s)" in
        Darwin)
            paths+=("/private/tmp")
            paths+=("/private/var")
            paths+=("/private/etc")
            paths+=("/System")
            paths+=("/Applications")
            paths+=("/Library")
            paths+=("$home_resolved/Desktop")
            paths+=("$home_resolved/Documents")
            paths+=("$home_resolved/Downloads")
            paths+=("$home_resolved/Pictures")
            paths+=("$home_resolved/Movies")
            paths+=("$home_resolved/Music")
            paths+=("$home_resolved/Library")
            paths+=("$home_resolved/Applications")
            ;;
        Linux)
            paths+=("/boot")
            paths+=("/sbin")
            paths+=("/bin")
            paths+=("/proc")
            paths+=("/sys")
            paths+=("/dev")
            paths+=("/run")
            paths+=("/srv")
            paths+=("$home_resolved/Desktop")
            paths+=("$home_resolved/Documents")
            paths+=("$home_resolved/Downloads")
            paths+=("$home_resolved/Pictures")
            paths+=("$home_resolved/Music")
            paths+=("$home_resolved/Videos")
            ;;
    esac

    local p
    for p in "${paths[@]}"; do
        [[ -d "$p" ]] && printf '%s\n' "$p"
    done
}

# Generate YAML array items for dangerous paths (indented for config file).
# Output format: lines of '  - "/path"'
_get_system_dangerous_paths_yaml() {
    local p
    while IFS= read -r p; do
        printf '  - "%s"\n' "$p"
    done < <(_get_system_dangerous_paths)
}

# ──────────────────────────────────────────────────
# Git init path guard (whitelist + dangerous path check)
# Unified entry point for both bin/workhive and init_command.
#
# Policy (evaluated in order):
#   1. Hardcoded hard guard (root "/") → always block (no prompt)
#   2. If whitelist_paths is non-empty:
#      - whitelisted dir (or subdir) → allow immediately
#      - otherwise block
#   3. User-added dangerous_paths_extra (exact match) → block
#   4. Otherwise fall through to confirmation
#
# $1 = directory to check
# Returns: 0 = allowed, 1 = blocked (caller should exit/return)
# Side effect: prints block reason to stderr when returning 1
# ──────────────────────────────────────────────────
_parse_json_array_lines() {
    local raw="$1"
    [[ -z "$raw" || "$raw" == "null" || "$raw" == "[]" ]] && return 0
    printf '%s\n' "$raw" | yq '.[]' 2>/dev/null
}

_is_git_init_allowed() {
    local dir="$1"
    [[ -z "$dir" ]] && dir="$(pwd)"

    local resolved
    resolved=$(cd "$dir" 2>/dev/null && pwd -P) || resolved="$dir"

    if _is_dangerous_git_root "$resolved"; then
        log_error "$(_i18n_fmt ERR_HARD_GUARD_ROOT "$resolved")"
        log_info "$(_i18n HINT_HARD_GUARD_ROOT)"
        return 1
    fi

    local whitelist
    whitelist=$(config_get "git_init.whitelist_paths" 2>/dev/null || echo "")
    if [[ -n "$whitelist" && "$whitelist" != "null" && "$whitelist" != "[]" ]]; then
        local allowed=false
        local wl_item
        while IFS= read -r wl_item; do
            [[ -z "$wl_item" ]] && continue
            local wl_resolved
            wl_resolved=$(cd "$wl_item" 2>/dev/null && pwd -P) || wl_resolved="$wl_item"
            if [[ "$resolved" == "$wl_resolved" || "$resolved" == "${wl_resolved}/"* ]]; then
                allowed=true
                break
            fi
        done < <(_parse_json_array_lines "$whitelist")
        if [[ "$allowed" == true ]]; then
            return 0
        fi
        log_error "$(_i18n_fmt ERR_NOT_IN_WHITELIST "$resolved")"
        log_info "$(_i18n HINT_ADD_WHITELIST)"
        return 1
    fi

    local extra_paths
    extra_paths=$(config_get "git_init.dangerous_paths_extra" 2>/dev/null || echo "")
    if [[ -n "$extra_paths" && "$extra_paths" != "null" && "$extra_paths" != "[]" ]]; then
        local path_item
        while IFS= read -r path_item; do
            [[ -z "$path_item" ]] && continue
            local extra_resolved
            extra_resolved=$(cd "$path_item" 2>/dev/null && pwd -P) || extra_resolved="$path_item"
            # Exact-match only for dangerous_paths_extra.
            if [[ "$resolved" == "$extra_resolved" ]]; then
                log_error "$(_i18n_fmt ERR_DANGEROUS_DIR "$resolved")"
                return 1
            fi
        done < <(_parse_json_array_lines "$extra_paths")
    fi

    return 0
}

# Execute git init with standard ceremony (init + .gitignore + initial commit).
# Shared between bin/workhive::check_git_repo and init_command_execute.
# $1 = interactive (true/false)
# Returns: 0 = success, 1 = user declined or error
_git_init_guarded() {
    local interactive="${1:-true}"

    log_warn "$(_i18n MSG_GIT_INIT_REQUIRED)"
    echo ""
    log_info "$(_i18n MSG_GIT_INIT_PREVIEW)"
    echo ""

    if [[ "$interactive" == true ]]; then
        if ask_yes_no "$(_i18n ASK_INIT_GIT_REPO)" "N"; then
            echo ""
            if ! git init; then
                log_error "$(_i18n ERR_GIT_INIT_FAILED)"
                return 1
            fi
            _apply_gitignore_template
            git add -A &>/dev/null || true
            if ! git commit -m "Initial commit" &>/dev/null; then
                log_error "$(_i18n ERR_GIT_COMMIT_FAILED)"
                return 1
            fi
            echo ""
            log_success "$(_i18n MSG_GIT_INIT_SUCCESS)"
            echo ""
            return 0
        else
            echo ""
            log_info "$(_i18n HINT_MANUAL_GIT_INIT)"
            return 1
        fi
    else
        log_error "$(_i18n ERR_NOT_GIT_REPO)"
        return 1
    fi
}

# Ensure initial commit exists in a git repo that has none.
# $1 = interactive (true/false)
# Returns: 0 = success, 1 = user declined or error
_git_ensure_initial_commit() {
    local interactive="${1:-true}"

    log_warn "$(_i18n MSG_GIT_NO_COMMITS)"
    echo ""

    if [[ "$interactive" == true ]]; then
        if ask_yes_no "$(_i18n ASK_CREATE_INITIAL_COMMIT)" "N"; then
            echo ""
            if ! git commit --allow-empty -m "Initial commit" &>/dev/null; then
                log_error "$(_i18n ERR_GIT_COMMIT_FAILED)"
                return 1
            fi
            log_success "$(_i18n MSG_GIT_INIT_SUCCESS)"
            echo ""
            return 0
        else
            echo ""
            log_info "$(_i18n HINT_MANUAL_GIT_INIT)"
            return 1
        fi
    else
        if ! git commit --allow-empty -m "Initial commit" &>/dev/null; then
            log_error "$(_i18n ERR_GIT_COMMIT_FAILED)"
            return 1
        fi
        return 0
    fi
}

# ──────────────────────────────────────────────────
# Git repository checks
# ──────────────────────────────────────────────────
git_is_inside_work_tree() {
    check_cmd git && git rev-parse --is-inside-work-tree &>/dev/null
}

# ──────────────────────────────────────────────────
# Git root directory (cached to avoid repeated forks)
# ──────────────────────────────────────────────────
# Cached git root directory (computed once, reused everywhere)
# Usage: local root; root=$(_git_root)
_git_root() {
    if [[ -n "${_WORKHIVE_GIT_ROOT:-}" ]]; then
        local cwd
        cwd=$(pwd -P 2>/dev/null || pwd)
        case "$cwd" in
            "$_WORKHIVE_GIT_ROOT"|"$_WORKHIVE_GIT_ROOT"/*)
                echo "$_WORKHIVE_GIT_ROOT"
                return 0
                ;;
        esac
    fi
    local toplevel
    toplevel=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

    # If inside a git worktree, resolve to the main repository root.
    # git-common-dir points to the shared .git of the main repo;
    # its parent is the true project root.
    local common_dir
    common_dir=$(git rev-parse --git-common-dir 2>/dev/null || echo "")
    if [[ -n "$common_dir" ]]; then
        # Resolve to absolute path
        if [[ "$common_dir" != /* ]]; then
            common_dir="$(cd "$common_dir" 2>/dev/null && pwd -P)" || common_dir=""
        fi
        # Only act if common_dir differs from toplevel's .git (= true worktree)
        if [[ -n "$common_dir" && "$common_dir" != "$toplevel/.git" ]]; then
            local main_root="${common_dir%/.git}"
            if [[ -d "$main_root" ]]; then
                toplevel="$main_root"
            fi
        fi
    fi

    _WORKHIVE_GIT_ROOT="$toplevel"
    export _WORKHIVE_GIT_ROOT
    echo "$_WORKHIVE_GIT_ROOT"
}

# Get the absolute path of the .workhive directory (always under git root)
_workhive_dir() {
    local root
    root=$(_git_root)

    if [[ -n "${_WORKHIVE_DIR_CACHED:-}" && "${_WORKHIVE_DIR_CACHED_ROOT:-}" == "$root" ]]; then
        echo "$_WORKHIVE_DIR_CACHED"
        return 0
    fi

    _WORKHIVE_DIR_CACHED_ROOT="$root"
    _WORKHIVE_DIR_CACHED="${root}/.workhive"
    echo "$_WORKHIVE_DIR_CACHED"
}

# Compute the subpath offset of cwd relative to git root
# e.g.: git root = /a, cwd = /a/b/c → returns "b/c"
# If cwd is git root → returns empty string
# If cwd is inside .workhive/worktrees/ → returns empty string (worktrees are not subpaths)
_git_subpath_offset() {
    local root
    root=$(_git_root)
    local cwd
    cwd=$(pwd)

    # Resolve symlinks to ensure path consistency (macOS: /tmp → /private/tmp)
    if command -v realpath &>/dev/null; then
        root=$(realpath "$root" 2>/dev/null) || root=$(_git_root)
        cwd=$(realpath "$cwd" 2>/dev/null) || cwd=$(pwd)
    elif [[ -d "$root" ]]; then
        root=$(cd "$root" && pwd -P)
        cwd=$(pwd -P)
    fi

    if [[ "$cwd" == "$root" ]]; then
        echo ""
    elif [[ "$cwd" == "$root/"* ]]; then
        local offset="${cwd#"$root"/}"
        # Worktree directories (.workhive/worktrees/*) are not subpath offsets
        if [[ "$offset" == ".workhive/worktrees"* ]]; then
            echo ""
        else
            echo "$offset"
        fi
    else
        echo ""
    fi
}

# Reset cached git root (for testing or after directory change)
_git_root_reset() {
    unset _WORKHIVE_GIT_ROOT
    unset _WORKHIVE_DIR_CACHED
    unset _WORKHIVE_DIR_CACHED_ROOT
}

# Copy .gitignore-template to .gitignore if target does not exist.
# Template lives at $ROOT_DIR/.gitignore-template (shipped with workhive).
_apply_gitignore_template() {
    [[ -f ".gitignore" ]] && return 0

    local template="${ROOT_DIR:-.}/.gitignore-template"
    if [[ -f "$template" ]]; then
        cp -- "$template" ".gitignore" 2>/dev/null || return 0
        log_debug "Applied .gitignore template from $template"
    fi
}

# Ensure .workhive is ignored by git.
# Idempotent: safe to call multiple times.
_ensure_gitignore_workhive() {
    local git_root
    git_root=$(_git_root)
    local ignore_file="${git_root}/.gitignore"
    local entry=".workhive/"
    local comment="# workhive directories"

    if [[ ! -f "$ignore_file" ]]; then
        _apply_gitignore_template
        if [[ ! -f "$ignore_file" ]]; then
            printf '%s\n%s\n' "$comment" "$entry" > "$ignore_file" 2>/dev/null || {
                log_warn "$(_i18n WARN_GITIGNORE_READONLY)"
                return 0
            }
            log_debug "Created .gitignore with .workhive/ entry"
            return 0
        fi
    fi

    if [[ ! -w "$ignore_file" ]]; then
        log_warn "$(_i18n WARN_GITIGNORE_READONLY)"
        return 0
    fi

    if ! grep -qE "^/?\.workhive/?$" "$ignore_file"; then
        [[ -n "$(tail -c1 "$ignore_file" 2>/dev/null)" ]] && echo "" >> "$ignore_file"
        echo "$comment" >> "$ignore_file"
        echo "$entry" >> "$ignore_file"
        log_debug "Added .workhive/ to .gitignore"
    fi
}

# Ensure .workhive directory and instance_id exist.
# Called lazily by open/attach when only global config is present.
_ensure_workhive_dir() {
    local wh_dir
    wh_dir=$(_workhive_dir)

    local was_missing=false
    if [[ ! -d "$wh_dir" ]]; then
        was_missing=true
        mkdir -p "$wh_dir" 2>/dev/null || return 0
        log_debug "Auto-created .workhive directory: $wh_dir"
    fi

    _ensure_gitignore_workhive

    local id_file="${wh_dir}/instance_id"
    if [[ ! -f "$id_file" ]]; then
        local new_id
        new_id=$(od -An -tx1 -N2 /dev/urandom 2>/dev/null | tr -d ' \n')
        printf '%s' "$new_id" > "$id_file" 2>/dev/null || return 0
        log_debug "Auto-generated instance_id: $new_id"
    fi

    # Re-initialize file logging now that .workhive/logs/ can be created.
    # This covers the case where only global config existed at startup
    # and _log_init skipped because .workhive/ did not exist yet.
    if [[ "$was_missing" == true || -z "$_LOG_FILE" ]]; then
        _LOG_INITIALIZED=false
        _log_init
    fi
}

# ──────────────────────────────────────────────────
# Git helpers
# ──────────────────────────────────────────────────

# Detect the main/default branch name for the current repo.
# Strategy: origin/HEAD → current branch → "main" fallback
_git_main_branch() {
    if [[ -n "${_GIT_MAIN_BRANCH_CACHED:-}" ]]; then
        echo "$_GIT_MAIN_BRANCH_CACHED"
        return 0
    fi
    local main_branch=""
    main_branch=$(git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@') || true
    if [[ -z "$main_branch" ]]; then
        local candidate
        for candidate in main master; do
            if git show-ref --verify --quiet "refs/heads/$candidate" 2>/dev/null; then
                main_branch="$candidate"
                break
            fi
        done
    fi
    [[ -z "$main_branch" ]] && main_branch="main"
    _GIT_MAIN_BRANCH_CACHED="$main_branch"
    echo "$main_branch"
}

# ──────────────────────────────────────────────────
# Command detection
# ──────────────────────────────────────────────────
check_cmd() {
    command -v "$1" &> /dev/null
}

resolve_realpath_fallback() {
    local path="$1"
    if [[ -z "$path" ]]; then
        echo ""
        return 1
    fi
    if command -v realpath &>/dev/null; then
        realpath "$path" 2>/dev/null || echo "$path"
        return 0
    fi
    if command -v readlink &>/dev/null; then
        readlink -f "$path" 2>/dev/null || echo "$path"
        return 0
    fi
    echo "$path"
}

resolve_realpath_allow_missing() {
    local path="$1"
    if [[ -z "$path" ]]; then
        echo ""
        return 1
    fi
    if command -v realpath &>/dev/null; then
        realpath -m "$path" 2>/dev/null || resolve_realpath_fallback "$path"
        return 0
    fi
    resolve_realpath_fallback "$path"
}

resolve_workhive_bin() {
    local bin
    bin=$(command -v workhive 2>/dev/null || echo "")
    if [[ -n "$bin" ]]; then
        echo "$bin"
        return 0
    fi
    if [[ -n "${ROOT_DIR:-}" ]]; then
        echo "${ROOT_DIR}/bin/workhive"
        return 0
    fi
    echo ""
    return 1
}

# Extract first version-like token from text by regex pattern.
# Args:
#   $1 raw text
#   $2 grep -E pattern
# Returns:
#   0 when a match is found, 1 otherwise
# Output:
#   matched token (stdout)
extract_version_by_pattern() {
    local raw="$1"
    local pattern="$2"
    local match=""

    [[ -z "$raw" || -z "$pattern" ]] && return 1

    match=$(printf '%s\n' "$raw" | grep -oE "$pattern" | head -n1 || true)
    [[ -z "$match" ]] && return 1

    printf '%s\n' "$match"
    return 0
}

# Run `<tool> <flag>` and extract version token by regex pattern.
# Args:
#   $1 tool command
#   $2 version flag (e.g. --version / -V)
#   $3 grep -E pattern
# Returns:
#   0 when extracted, 1 otherwise
# Output:
#   extracted version token (stdout)
get_tool_version() {
    local tool="$1"
    local version_flag="${2:---version}"
    local pattern="$3"
    local raw=""

    [[ -z "$tool" || -z "$pattern" ]] && return 1

    raw=$("$tool" "$version_flag" 2>&1 || true)
    extract_version_by_pattern "$raw" "$pattern"
}

# Resolve tool version detection rule by profile.
# Args:
#   $1 tool name
#   $2 profile name: strict | relaxed
# Output:
#   "<flag>|<pattern>"
# Returns:
#   0 when rule exists, 1 otherwise
tool_version_rule() {
    local tool="$1"
    local profile="${2:-strict}"

    case "$profile:$tool" in
        strict:git|strict:yq|strict:lazygit|strict:claude|strict:codex|strict:opencode|strict:yazi)
            printf '%s\n' '--version|[0-9]+\.[0-9]+\.[0-9]+'
            return 0
            ;;
        strict:tmux)
            printf '%s\n' '-V|[0-9]+\.[0-9]+'
            return 0
            ;;
        relaxed:git|relaxed:lazygit|relaxed:yazi)
            printf '%s\n' '--version|[0-9]+\.[0-9]+(\.[0-9]+)?'
            return 0
            ;;
        relaxed:tmux)
            printf '%s\n' '-V|[0-9]+\.[0-9]+(\.[0-9]+)?'
            return 0
            ;;
        relaxed:yq|relaxed:claude|relaxed:codex|relaxed:opencode)
            printf '%s\n' '--version|[0-9]+\.[0-9]+\.[0-9]+'
            return 0
            ;;
    esac

    return 1
}

# Get tool version using pre-defined profile rules.
# Args:
#   $1 tool name
#   $2 profile name: strict | relaxed
# Returns:
#   0 when extracted, 1 otherwise
# Output:
#   extracted version token (stdout)
get_tool_version_by_profile() {
    local tool="$1"
    local profile="${2:-strict}"
    local rule=""
    local flag=""
    local pattern=""

    rule=$(tool_version_rule "$tool" "$profile") || return 1
    flag="${rule%%|*}"
    pattern="${rule#*|}"

    get_tool_version "$tool" "$flag" "$pattern"
}

# Parse version into numeric major/minor fields.
# Accepts forms like: 3.4, 3.4a, 3.4.1
# Args:
#   $1 version string
# Output:
#   "<major>|<minor>"
# Returns:
#   0 when parsed, 1 on invalid input
parse_version_major_minor() {
    local raw="${1:-}"
    [[ -n "$raw" ]] || return 1

    local major_raw="${raw%%.*}"
    local rest="${raw#*.}"
    local minor_raw="${rest%%.*}"
    local major="${major_raw%%[^0-9]*}"
    local minor="${minor_raw%%[^0-9]*}"

    [[ -n "$major" && "$major" =~ ^[0-9]+$ ]] || return 1
    [[ -n "$minor" && "$minor" =~ ^[0-9]+$ ]] || return 1

    printf '%s|%s\n' "$major" "$minor"
    return 0
}

# ──────────────────────────────────────────────────
# Null/empty value detection (yq returns "null" for missing keys)
# ──────────────────────────────────────────────────
# Check if a value is null or empty (handles yq's "null" string)
# Usage: _is_null_or_empty "$value" && echo "empty"
#        _is_null_or_empty "$value" || echo "has value"
_is_null_or_empty() {
    local value="${1:-}"
    [[ -z "$value" || "$value" == "null" ]]
}

# Check if a command string contains shell metacharacters that could
# chain commands or redirect execution: ; && || | & > < $() `` and newlines.
# Returns: 0 = clean, 1 = contains shell metacharacters
_has_shell_metacharacters() {
    local cmd="$1"
    [[ "$cmd" == *'$('* || "$cmd" == *'`'* || \
       "$cmd" == *';'* || "$cmd" == *'&&'* || \
       "$cmd" == *'||'* || "$cmd" == *'|'* || \
       "$cmd" == *'>'* || "$cmd" == *'<'* || \
       "$cmd" == *'&'* || \
       "$cmd" == *$'\n'* ]] && return 1
    return 0
}

# ──────────────────────────────────────────────────
# JSON escaping (RFC 8259)
# ──────────────────────────────────────────────────
# Escape a string for safe JSON embedding (RFC 8259)
# Usage: local escaped; escaped=$(json_escape "$value")
json_escape() {
    local str="$1"
    str="${str//\\/\\\\}"         # backslash (must be first)
    str="${str//\"/\\\"}"         # double quote
    str="${str//$'\n'/\\n}"       # newline
    str="${str//$'\r'/\\r}"       # carriage return
    str="${str//$'\t'/\\t}"       # tab
    str="${str//$'\010'/\\b}"     # backspace
    str="${str//$'\014'/\\f}"     # form feed
    # Remaining control characters U+0000 to U+001F (ones above are already
    # multi-char escape sequences and will never match single-char < \x20)
    local i char hex
    local result=""
    for ((i=0; i<${#str}; i++)); do
        char="${str:i:1}"
        if [[ "$char" < $'\x20' ]]; then
            printf -v hex '\\u%04x' "'$char"
            result+="$hex"
        else
            result+="$char"
        fi
    done
    printf '%s' "$result"
}

# ──────────────────────────────────────────────────
# CJK-aware display width and string padding
# CJK characters occupy 2 terminal columns, ASCII occupies 1.
# ──────────────────────────────────────────────────

# Calculate terminal display width of a string (CJK = 2 cols, ASCII = 1 col)
# $1 = string
# Output: display width (integer)
_dw() {
    local str="$1"
    local display_w=0 char codepoint
    local i len=${#str}
    for (( i = 0; i < len; i++ )); do
        char="${str:$i:1}"
        printf -v codepoint '%d' "'$char" 2>/dev/null || codepoint=0
        if (( codepoint >= 0x2E80 && codepoint <= 0x9FFF )) || \
           (( codepoint >= 0xF900 && codepoint <= 0xFAFF )) || \
           (( codepoint >= 0xFE30 && codepoint <= 0xFE4F )) || \
           (( codepoint >= 0xFF00 && codepoint <= 0xFF60 )) || \
           (( codepoint >= 0xFFE0 && codepoint <= 0xFFE6 )) || \
           (( codepoint >= 0x20000 && codepoint <= 0x2FA1F )); then
            display_w=$((display_w + 2))
        else
            display_w=$((display_w + 1))
        fi
    done
    echo "$display_w"
}

# Pad a string to a given terminal display width
# $1 = target width, $2 = string
_wpad() {
    local width="$1" str="$2"
    local display_w=0 char codepoint
    local i len=${#str}
    for (( i = 0; i < len; i++ )); do
        char="${str:$i:1}"
        printf -v codepoint '%d' "'$char" 2>/dev/null || codepoint=0
        if (( codepoint >= 0x2E80 && codepoint <= 0x9FFF )) || \
           (( codepoint >= 0xF900 && codepoint <= 0xFAFF )) || \
           (( codepoint >= 0xFE30 && codepoint <= 0xFE4F )) || \
           (( codepoint >= 0xFF00 && codepoint <= 0xFF60 )) || \
           (( codepoint >= 0xFFE0 && codepoint <= 0xFFE6 )) || \
           (( codepoint >= 0x20000 && codepoint <= 0x2FA1F )); then
            display_w=$((display_w + 2))
        else
            display_w=$((display_w + 1))
        fi
    done
    local pad=$((width - display_w))
    (( pad < 0 )) && pad=0
    printf '%s%*s' "$str" "$pad" ""
}

# ──────────────────────────────────────────────────
# Cross-platform file modification time (human-readable)
# $1 = file path
# $2 = format: "display" (default, "YYYY-MM-DD HH:MM:SS")
#              or "iso" ("YYYY-MM-DDTHH:MM:SS")
# Returns: formatted timestamp string, or empty on error
# ──────────────────────────────────────────────────
_file_mtime() {
    local filepath="$1"
    local fmt="${2:-display}"
    [[ ! -f "$filepath" ]] && return 0

    local result=""
    if [[ "$(uname)" == "Darwin" ]]; then
        if [[ "$fmt" == "iso" ]]; then
            result=$(stat -f "%Sm" -t "%Y-%m-%dT%H:%M:%S" "$filepath" 2>/dev/null)
        else
            result=$(stat -f "%Sm" -t "%Y-%m-%d %H:%M:%S" "$filepath" 2>/dev/null)
        fi
    else
        local raw
        raw=$(stat -c "%y" "$filepath" 2>/dev/null || echo "")
        result="${raw%%.*}"
        if [[ "$fmt" == "iso" && -n "$result" ]]; then
            result="${result/ /T}"
        fi
    fi
    [[ -n "$result" ]] && printf '%s' "$result"
}

# ──────────────────────────────────────────────────
# Duration helpers
# v1 format: positive integer + unit suffix (s|m|h), e.g. 3s, 10m, 2h
# ──────────────────────────────────────────────────
is_duration_string() {
    local raw="${1:-}"
    [[ "$raw" =~ ^[1-9][0-9]*[smh]$ ]]
}

duration_to_seconds() {
    local raw="${1:-}"
    is_duration_string "$raw" || return 1
    local num="${raw%[smh]}"
    local unit="${raw:${#raw}-1:1}"
    case "$unit" in
        s) printf '%s' "$num" ;;
        m) printf '%s' "$((num * 60))" ;;
        h) printf '%s' "$((num * 3600))" ;;
        *) return 1 ;;
    esac
}

# ──────────────────────────────────────────────────
# Session activity helpers
# ──────────────────────────────────────────────────

# Get tmux session activity epoch.
# Args: $1=session name
# Output: epoch seconds or 0 when unavailable
session_activity_epoch() {
    local session_name="$1"
    local epoch
    epoch=$(tmux display-message -t "$session_name" -p '#{session_activity}' 2>/dev/null || true)
    if [[ -z "$epoch" || ! "$epoch" =~ ^[0-9]+$ ]]; then
        echo "0"
        return 0
    fi
    echo "$epoch"
}

# Format epoch seconds into localized relative time.
# Args: $1=epoch seconds
# Output: localized relative time or "-"
format_relative_time() {
    local epoch="$1"
    local now delta
    now=$(date +%s 2>/dev/null || echo "0")
    if [[ ! "$epoch" =~ ^[0-9]+$ || "$epoch" -le 0 ]]; then
        echo "-"
        return 0
    fi
    if [[ ! "$now" =~ ^[0-9]+$ || "$now" -lt "$epoch" ]]; then
        echo "-"
        return 0
    fi
    delta=$((now - epoch))

    if type -t _i18n >/dev/null 2>&1; then
        if [[ $delta -lt 60 ]]; then
            echo "$(_i18n STATUS_TIME_JUST_NOW)"
        elif [[ $delta -lt 3600 ]]; then
            _i18n_fmt STATUS_TIME_MIN_AGO_FMT "$((delta / 60))"
        elif [[ $delta -lt 86400 ]]; then
            _i18n_fmt STATUS_TIME_HOUR_AGO_FMT "$((delta / 3600))"
        else
            _i18n_fmt STATUS_TIME_DAY_AGO_FMT "$((delta / 86400))"
        fi
        return 0
    fi

    if [[ $delta -lt 60 ]]; then
        echo "just now"
    elif [[ $delta -lt 3600 ]]; then
        echo "$((delta / 60))m ago"
    elif [[ $delta -lt 86400 ]]; then
        echo "$((delta / 3600))h ago"
    else
        echo "$((delta / 86400))d ago"
    fi
}

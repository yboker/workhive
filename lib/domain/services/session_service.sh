#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Session Service
# tmux/screen session management service (Domain layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_SESSION_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _SESSION_SERVICE_SH_LOADED=1

source "${LIB_DIR}/utils/i18n.sh"

# Dependencies
source "${LIB_DIR}/infra/adapters/terminal/factory.sh"
source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/persistence/auto_save.sh"

# Lazy adapter bootstrap: called on demand by functions that need terminal ops.
# Pure naming helpers (session_get_prefix, session_get_name, session_extract_branch)
# never touch the adapter, keeping L1 query paths fast.
_session_ensure_terminal_adapter() {
    case "${COMMAND:-}" in
        _internal_save|_internal_save_runtime_debounced) return 0 ;;
    esac
    terminal_adapter_init
}

_session_kill_wait_max_attempts() {
    local raw="${WORKHIVE_SESSION_KILL_WAIT_MAX_ATTEMPTS:-20}"
    if [[ "$raw" =~ ^[0-9]+$ ]] && ((raw > 0)); then
        printf '%s\n' "$raw"
    else
        printf '20\n'
    fi
}

_session_kill_wait_interval_seconds() {
    local raw="${WORKHIVE_SESSION_KILL_WAIT_INTERVAL_SEC:-0.1}"
    if [[ "$raw" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        printf '%s\n' "$raw"
    else
        printf '0.1\n'
    fi
}

_session_wait_until_absent() {
    local session_name="$1"
    local max_attempts="$2"
    local wait_interval="$3"
    local attempt=0

    while terminal_adapter_has_session "$session_name" 2>/dev/null; do
        ((attempt += 1))
        if ((attempt >= max_attempts)); then
            return 1
        fi
        sleep "$wait_interval"
    done
    return 0
}

_session_pre_kill_save_timeout_seconds() {
    local raw="${WORKHIVE_PRE_KILL_SAVE_TIMEOUT_SEC:-1.0}"
    if [[ "$raw" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
        printf '%s\n' "$raw"
    else
        printf '1.0\n'
    fi
}

_session_pre_kill_snapshot_best_effort() {
    local branch="$1"
    [[ -n "$branch" ]] || return 0

    if ! declare -f persistence_save_before_kill >/dev/null 2>&1; then
        source "${LIB_DIR}/infra/persistence/manager.sh" 2>/dev/null || true
    fi
    declare -f persistence_save_before_kill >/dev/null 2>&1 || return 0

    local timeout_sec timeout_ticks waited_ticks
    timeout_sec=$(_session_pre_kill_save_timeout_seconds)
    timeout_ticks=$(awk "BEGIN { printf \"%d\", ($timeout_sec * 20) + 0.999999 }")
    (( timeout_ticks < 1 )) && timeout_ticks=1
    waited_ticks=0

    local env_prefix=""
    env_prefix=$(config_build_env_assignment_prefix_from_getter "config_get_persistence_pre_kill_env_entries")

    (
        config_unset_managed_env_vars
        if [[ -n "$env_prefix" ]]; then
            eval "${env_prefix}persistence_save_before_kill \"\$branch\" >/dev/null 2>&1 || true"
        else
            persistence_save_before_kill "$branch" >/dev/null 2>&1 || true
        fi
    ) &
    local save_pid=$!

    while kill -0 "$save_pid" 2>/dev/null; do
        if (( waited_ticks >= timeout_ticks )); then
            kill "$save_pid" 2>/dev/null || true
            sleep 0.02
            kill -9 "$save_pid" 2>/dev/null || true
            wait "$save_pid" 2>/dev/null || true
            log_debug "Pre-kill snapshot timed out (${timeout_sec}s), continue kill flow: branch=${branch}"
            return 0
        fi
        sleep 0.05
        waited_ticks=$((waited_ticks + 1))
    done

    wait "$save_pid" 2>/dev/null || true
    return 0
}

# Get project name (git root basename + CWD subpath)
# At git root:     python
# In subdirectory: python/chedui_excel_last
session_get_project_name() {
    local git_root
    git_root=$(_git_root)
    local project="${git_root##*/}"

    local subpath
    subpath=$(_git_subpath_offset)
    if [[ -n "$subpath" ]]; then
        echo "${project}/${subpath}"
    else
        echo "$project"
    fi
}

# Get the project's instance_id (4-digit hex, generated at init time)
# Returns (stdout): instance_id or empty string (not initialized)
session_get_instance_id() {
    local id_file
    id_file="$(_workhive_dir)/instance_id"
    if [[ -f "$id_file" ]]; then
        local _id=""
        read -r _id < "$id_file" 2>/dev/null || true
        echo "$_id"
    fi
}

# Get session name prefix
# Project-level (initialized): <tag>@<id>@<project>
# Global fallback (not initialized): <tag>@<project>
# tag defaults to "wh", customizable via tmux.session_prefix
# Used to list/filter all tmux sessions belonging to the current project
_sanitize_session_prefix_tag() {
    local tag="$1"
    tag="${tag//[^[:alnum:]_-]/}"
    _is_null_or_empty "$tag" && tag="wh"
    printf '%s' "$tag"
}

_SESSION_PREFIX_CACHED=""
_SESSION_TMUX_ENV_PREFIX_CACHED=""

session_get_prefix() {
    if [[ -n "$_SESSION_PREFIX_CACHED" ]]; then
        echo "$_SESSION_PREFIX_CACHED"
        return 0
    fi

    # Fast path: read tmux.session_prefix directly from config files
    # to avoid config_get's full search + yq fallback overhead (~200ms saved)
    local tag=""
    local _cfg_dir="${_WORKHIVE_GIT_ROOT:+$_WORKHIVE_GIT_ROOT/.workhive}"
    if [[ -n "$_cfg_dir" && -f "$_cfg_dir/config.yaml" ]]; then
        tag=$(_config_fast_get "tmux.session_prefix" "$_cfg_dir/config.yaml" 2>/dev/null) || true
    fi
    if _is_null_or_empty "$tag" && [[ -f ".workhive/config.yaml" ]]; then
        tag=$(_config_fast_get "tmux.session_prefix" ".workhive/config.yaml" 2>/dev/null) || true
    fi
    if _is_null_or_empty "$tag"; then
        local _user_cfg
        _user_cfg="$(config_get_global_file 2>/dev/null)"
        [[ -n "$_user_cfg" && -f "$_user_cfg" ]] && tag=$(_config_fast_get "tmux.session_prefix" "$_user_cfg" 2>/dev/null) || true
    fi
    _is_null_or_empty "$tag" && tag="wh"
    tag=$(_sanitize_session_prefix_tag "$tag")

    local project
    project=$(session_get_project_name)
    project=$(_sanitize_project_name "$project")
    local instance_id
    instance_id=$(session_get_instance_id)
    if [[ -n "$instance_id" ]]; then
        _SESSION_PREFIX_CACHED="${tag}@${instance_id}@${project}"
    else
        _SESSION_PREFIX_CACHED="${tag}@${project}"
    fi
    echo "$_SESSION_PREFIX_CACHED"
}

_session_get_tmux_env_prefix() {
    if [[ -n "$_SESSION_TMUX_ENV_PREFIX_CACHED" ]]; then
        printf '%s' "$_SESSION_TMUX_ENV_PREFIX_CACHED"
        return 0
    fi

    _SESSION_TMUX_ENV_PREFIX_CACHED=$(config_build_env_assignment_prefix_from_getter "config_get_tmux_env_entries")
    printf '%s' "$_SESSION_TMUX_ENV_PREFIX_CACHED"
}

# Generate session name from branch name
# Format: <prefix>@<branch>
# Args: $1 = branch name
session_get_name() {
    local branch="$1"
    local prefix
    prefix=$(session_get_prefix)

    local safe_branch
    safe_branch=$(_sanitize_branch_name "$branch")
    echo "${prefix}@${safe_branch}"
}

# Resolve a project-family session prefix.
# With instance_id: <tag>@<instance_id>@
# Without instance_id: <tag>@
_session_get_family_prefix() {
    local prefix
    prefix=$(session_get_prefix)

    local tag="${prefix%%@*}"
    local instance_id
    instance_id=$(session_get_instance_id)

    if [[ -n "$instance_id" ]]; then
        printf '%s@%s@' "$tag" "$instance_id"
    else
        printf '%s@' "$tag"
    fi
}

# Extract branch from a full session name (everything after the last @)
# Args: $1 = full session name
session_extract_branch() {
    local session_name="$1"
    echo "${session_name##*@}"
}

# Check if a session exists
# Args: $1 = session name
# Returns: 0 = exists, 1 = does not exist
session_exists() {
    local session_name="$1"

    # Use cache (5 seconds)
    local cache_key="session:exists:$session_name"
    local cached_value
    if cached_value=$(cache_get "$cache_key" 2>/dev/null); then
        [[ "$cached_value" == "true" ]]
        return $?
    fi

    _session_ensure_terminal_adapter

    # Check session
    local exists=false
    if terminal_adapter_has_session "$session_name"; then
        exists=true
    fi

    cache_set "$cache_key" "$exists" "$CACHE_TTL_SESSION_EXISTS"

    [[ "$exists" == "true" ]]
}

# Resolve session name from branch + assert it exists.
# Sets caller variable SESSION_NAME and returns 0 on success.
# On failure: logs error and returns 1.
# Usage: local SESSION_NAME; session_require_exists "$branch" || return 1
session_require_exists() {
    local branch="$1"
    SESSION_NAME=$(session_get_name "$branch")
    if ! session_exists "$SESSION_NAME"; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $SESSION_NAME"
        return 1
    fi
    return 0
}

# Create a session
# Args: $1 = branch name, $2 = working directory
# Returns: 0 = success, 1 = failure
session_create() {
    local branch="$1"
    local workdir="$2"

    if [[ -z "$branch" ]]; then
        log_error "$(_i18n ERR_BRANCH_EMPTY)"
        return 1
    fi

    if [[ "$branch" == "__MAIN_DIR__" ]]; then
        log_error "$(_i18n_fmt ERR_SESSION_SENTINEL_LEAK_FMT "$branch")"
        return 1
    fi

    if [[ ! -d "$workdir" ]]; then
        log_error "$(_i18n ERR_WORKDIR_NOT_FOUND) $workdir"
        return 1
    fi

    _session_ensure_terminal_adapter

    local session_name
    session_name=$(session_get_name "$branch")

    log_info "$(_i18n INFO_SESSION_CREATING) $session_name"
    op_begin "session_create" "$session_name" "branch=$branch workdir=$workdir"

    # Idempotent creation: try to create first, check if already exists on failure (avoid TOCTOU race)
    if ! terminal_adapter_create_session "$session_name" "$workdir" 2>/dev/null; then
        if session_exists "$session_name"; then
            log_info "$(_i18n INFO_SESSION_EXISTS) $session_name"
            op_ok "session_create" "$session_name" "already_exists=true"
            return 0
        fi
        log_error "$(_i18n ERR_SESSION_CREATE_FAILED)"
        op_fail "session_create" "$session_name" 1 "branch=$branch"
        return 1
    fi

    # Apply configuration (theme, mouse support, etc.)
    session_apply_config "$session_name"

    # Invalidate cache
    cache_invalidate "session:*"

    log_success "$(_i18n INFO_SESSION_CREATED) $session_name"
    op_ok "session_create" "$session_name" "branch=$branch"
    return 0
}

# Attach to a session
# Args: $1 = branch name
# Returns: 0 = success, 1 = failure
session_attach() {
    local branch="$1"
    _session_ensure_terminal_adapter
    local SESSION_NAME
    if ! session_require_exists "$branch"; then
        log_info "$(_i18n INFO_USE_OPEN_CREATE)"
        return 1
    fi
    local session_name="$SESSION_NAME"

    _session_attach_by_resolved_name "$session_name"
}

# Attach to a session by full session name.
# Args: $1 = full session name
# Returns: 0 = success, 1 = failure
session_attach_by_name() {
    local session_name="$1"
    _session_ensure_terminal_adapter

    if ! session_exists "$session_name"; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $session_name"
        return 1
    fi

    _session_attach_by_resolved_name "$session_name"
}

_session_attach_by_resolved_name() {
    local session_name="$1"

    log_info "$(_i18n INFO_ATTACHING_TO_SESSION) $session_name"
    terminal_adapter_attach_session "$session_name"
    local attach_rc=$?
    _session_log_attach_outcome "$session_name" "$attach_rc"
    return "$attach_rc"
}

_session_log_attach_outcome() {
    local session_name="$1"
    local attach_rc="$2"
    local attach_status="unknown"
    if declare -f terminal_adapter_get_last_attach_status >/dev/null 2>&1; then
        attach_status=$(terminal_adapter_get_last_attach_status 2>/dev/null || echo "unknown")
    fi
    if [[ "$attach_rc" -eq 0 && "$attach_status" == "ended" ]]; then
        log_warn "$(_i18n_fmt WARN_ATTACH_SESSION_ENDED_FMT "$session_name")"
    elif [[ "$attach_rc" -ne 0 ]]; then
        log_debug "$(_i18n_fmt DEBUG_ATTACH_FAILED_FMT "$session_name" "$attach_status" "$attach_rc")"
    fi
    return "$attach_rc"
}

# Detach from the current session (session stays alive)
# Returns: 0 = success, 1 = failure
session_detach() {
    _session_ensure_terminal_adapter
    terminal_adapter_detach_session
}

# Kill a session by exact session name
# Args: $1 = full session name, $2 = branch name (optional), $3 = skip_pre_kill_snapshot (optional, true/false)
# Returns: 0 = success, 1 = failure
session_kill_by_name() {
    local session_name="$1"
    local branch="${2:-}"
    local skip_pre_kill_snapshot="${3:-false}"

    _session_ensure_terminal_adapter

    if ! terminal_adapter_has_session "$session_name" 2>/dev/null; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $session_name"
        op_fail "session_kill" "$session_name" 1 "not_found"
        return 1
    fi

    if [[ -z "$branch" ]]; then
        branch=$(session_extract_branch "$session_name")
    fi

    # Safety check: warn if trying to kill the current session
    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null || true)
    if [[ -n "$current_session" && "$current_session" == "$session_name" ]]; then
        log_warn "$(_i18n_fmt WARN_KILLING_CURRENT_SESSION "$session_name")"
    fi

    log_info "$(_i18n INFO_SESSION_KILLING) $session_name"
    op_begin "session_kill" "$session_name" "branch=$branch"

    # Pre-kill snapshot is best-effort and must not block kill completion.
    # Strict rebuild flows can explicitly skip snapshot creation.
    if [[ "$skip_pre_kill_snapshot" != "true" ]]; then
        _session_pre_kill_snapshot_best_effort "$branch"
    fi

    # Kill the session
    if ! terminal_adapter_kill_session "$session_name"; then
        log_error "$(_i18n ERR_SESSION_KILL_FAILED)"
        op_fail "session_kill" "$session_name" 1 "branch=$branch"
        return 1
    fi

    local kill_wait_attempts kill_wait_interval
    kill_wait_attempts=$(_session_kill_wait_max_attempts)
    kill_wait_interval=$(_session_kill_wait_interval_seconds)
    if ! _session_wait_until_absent "$session_name" "$kill_wait_attempts" "$kill_wait_interval"; then
        log_error "$(_i18n_fmt ERR_SESSION_KILL_TIMEOUT_FMT "$session_name" "$kill_wait_attempts" "$kill_wait_interval")"
        op_fail "session_kill" "$session_name" 1 "branch=$branch timeout_wait=${kill_wait_attempts}x${kill_wait_interval}s"
        return 1
    fi

    # Invalidate cache
    cache_invalidate "session:*"

    log_success "$(_i18n INFO_SESSION_CLOSED) $session_name"
    op_ok "session_kill" "$session_name" "branch=$branch"
    return 0
}

# Kill a session
# Args: $1 = branch name, $2 = skip_pre_kill_snapshot (optional, true/false)
# Returns: 0 = success, 1 = failure
session_kill() {
    local branch="$1"
    local skip_pre_kill_snapshot="${2:-false}"
    _session_ensure_terminal_adapter
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    session_kill_by_name "$SESSION_NAME" "$branch" "$skip_pre_kill_snapshot"
}

# List all active sessions
# Returns (stdout): newline-separated list of session names
session_list_all() {
    # Use cache (10 seconds)
    local cache_key="session:list"
    local cached
    if cached=$(cache_get "$cache_key" 2>/dev/null); then
        printf '%s\n' "$cached"
        return 0
    fi

    _session_ensure_terminal_adapter

    local prefix
    prefix=$(session_get_prefix)

    # Get all sessions via adapter and filter by prefix
    local sessions
    sessions=$(terminal_adapter_list_sessions | grep "^${prefix}@" || true)

    # Cache the results
    if [[ -n "$sessions" ]]; then
        cache_set "$cache_key" "$sessions" "$CACHE_TTL_SESSION_LIST"
    fi

    printf '%s\n' "$sessions"
}

# List active sessions for the current project (filtered by first pane's working directory)
# Returns (stdout): newline-separated list of session names (only those belonging to the current project)
session_list_project() {
    _session_ensure_terminal_adapter
    local project_root
    project_root=$(_git_root)

    local all_sessions
    all_sessions=$(session_list_all)
    [[ -z "$all_sessions" ]] && return 0

    local result=()
    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        local pane_path
        pane_path=$(terminal_adapter_list_panes_format "$session_name" '#{pane_current_path}' 2>/dev/null | head -n1)
        if [[ -n "$pane_path" && "$pane_path" == "${project_root}"* ]]; then
            result+=("$session_name")
        fi
    done <<< "$all_sessions"

    if [[ ${#result[@]} -gt 0 ]]; then
        printf '%s\n' "${result[@]}"
    fi
}

# List active sessions for the current Git root across all project subpaths.
# This is used by attach flows so users can see all running sessions in the
# same repository, including sessions started from nested directories.
# Returns (stdout): newline-separated full session names
session_list_project_family() {
    _session_ensure_terminal_adapter

    local project_root
    project_root=$(_git_root)

    local family_prefix
    family_prefix=$(_session_get_family_prefix)

    local all_sessions
    all_sessions=$(terminal_adapter_list_sessions 2>/dev/null || true)
    [[ -z "$all_sessions" ]] && return 0

    local result=()
    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        [[ "$session_name" == "${family_prefix}"* ]] || continue

        local pane_path
        pane_path=$(terminal_adapter_list_panes_format "$session_name" '#{pane_current_path}' 2>/dev/null | head -n1)
        if [[ -n "$pane_path" && "$pane_path" == "${project_root}"* ]]; then
            result+=("$session_name")
        fi
    done <<< "$all_sessions"

    if [[ ${#result[@]} -gt 0 ]]; then
        printf '%s\n' "${result[@]}"
    fi
}

# List project-family sessions matching a specific branch.
# Args: $1 = branch name (will be sanitized for session matching)
# Returns (stdout): newline-separated full session names
session_list_project_family_by_branch() {
    local branch="$1"
    local normalized_branch
    normalized_branch=$(_sanitize_branch_name "$branch")

    local sessions
    sessions=$(session_list_project_family 2>/dev/null || true)
    [[ -z "$sessions" ]] && return 0

    local result=()
    local session_name session_branch
    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        session_branch=$(session_extract_branch "$session_name")
        [[ "$session_branch" == "$normalized_branch" ]] || continue
        result+=("$session_name")
    done <<< "$sessions"

    if [[ ${#result[@]} -gt 0 ]]; then
        printf '%s\n' "${result[@]}"
    fi
}

# Cleanup result globals
SESSION_CLEANUP_ATTEMPTED=0
SESSION_CLEANUP_CLOSED=0
SESSION_CLEANUP_REMAINING=""

# Internal cleanup helper for a concrete session list.
# Args:
#   $1 = newline-separated full session names
#   $2 = best_effort (true/false): when true, fallback to direct tmux kill
_session_cleanup_by_list() {
    local sessions="$1"
    local best_effort="${2:-false}"

    SESSION_CLEANUP_ATTEMPTED=0
    SESSION_CLEANUP_CLOSED=0
    SESSION_CLEANUP_REMAINING=""

    local session_name branch_name
    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        SESSION_CLEANUP_ATTEMPTED=$((SESSION_CLEANUP_ATTEMPTED + 1))
        branch_name=$(session_extract_branch "$session_name")
        if session_kill_by_name "$session_name" "$branch_name" 2>/dev/null; then
            SESSION_CLEANUP_CLOSED=$((SESSION_CLEANUP_CLOSED + 1))
            continue
        fi
        if [[ "$best_effort" == "true" ]]; then
            if terminal_adapter_kill_session "$session_name" 2>/dev/null; then
                SESSION_CLEANUP_CLOSED=$((SESSION_CLEANUP_CLOSED + 1))
            fi
        fi
    done <<< "$sessions"
}

# Cleanup all sessions in current project family.
# Args: $1 = best_effort (true/false)
# Return: 0 when no sessions remain, 1 otherwise
session_cleanup_project_family() {
    local best_effort="${1:-false}"
    local sessions
    sessions=$(session_list_project_family 2>/dev/null || true)
    _session_cleanup_by_list "$sessions" "$best_effort"
    SESSION_CLEANUP_REMAINING=$(session_list_project_family 2>/dev/null || true)
    [[ -z "$SESSION_CLEANUP_REMAINING" ]]
}

# Cleanup all sessions for one branch in current project family.
# Args:
#   $1 = branch name
#   $2 = best_effort (true/false)
# Return: 0 when no branch sessions remain, 1 otherwise
session_cleanup_project_family_by_branch() {
    local branch="$1"
    local best_effort="${2:-false}"
    local sessions
    sessions=$(session_list_project_family_by_branch "$branch" 2>/dev/null || true)
    _session_cleanup_by_list "$sessions" "$best_effort"
    SESSION_CLEANUP_REMAINING=$(session_list_project_family_by_branch "$branch" 2>/dev/null || true)
    [[ -z "$SESSION_CLEANUP_REMAINING" ]]
}

# Apply session configuration (theme, mouse, status bar, etc.)
# This is the required bootstrap phase: only tmux options that affect
# first-frame correctness. Autosave is deferred to session_apply_deferred.
# Args: $1 = session name
session_apply_config() {
    local session_name="$1"
    _session_ensure_terminal_adapter

    log_debug "$(_i18n DEBUG_APPLYING_SESSION_CONFIG) $session_name"

    local prefix
    prefix=$(session_get_prefix)

    terminal_adapter_apply_config "$session_name" "$prefix"

    log_debug "Session config applied via adapter for: $session_name"
    log_debug "$(_i18n DEBUG_CONFIG_APPLIED)"
}

# Deferred session bootstrap: non-critical initialization that does not
# affect first-frame rendering. Safe to run after attach or in background.
# Args: $1 = session name
session_apply_deferred() {
    local session_name="$1"
    auto_save_install_hooks "$session_name" 2>/dev/null || true
    auto_save_start_timer "$session_name" 2>/dev/null || true
}

# Async variant: runs deferred bootstrap in background so it does not
# block attach (first-frame critical path). Fire-and-forget.
# Args: $1 = session name
session_apply_deferred_async() {
    local session_name="$1"
    ( session_apply_deferred "$session_name" ) &
    disown 2>/dev/null || true
}

# Get session info
# Args: $1 = branch name
# Returns: JSON-formatted session information
session_get_info() {
    local branch="$1"
    _session_ensure_terminal_adapter
    local SESSION_NAME
    if ! session_require_exists "$branch"; then
        echo "{\"exists\": false}"
        return 1
    fi
    local session_name="$SESSION_NAME"

    # Get adapter
    local adapter
    adapter=$(terminal_adapter_get)

    local windows
    windows=$(terminal_adapter_get_window_count "$session_name" 2>/dev/null || echo "0")
    local attached=false
    if terminal_adapter_is_session_attached "$session_name" 2>/dev/null; then
        attached=true
    fi

    # Safely escape JSON strings (prevent special characters from breaking JSON)
    local safe_name safe_branch safe_adapter
    safe_name=$(json_escape "$session_name")
    safe_branch=$(json_escape "$branch")
    safe_adapter=$(json_escape "$adapter")

    cat <<EOF
{
  "exists": true,
  "name": "$safe_name",
  "branch": "$safe_branch",
  "adapter": "$safe_adapter",
  "windows": $windows,
  "attached": $attached
}
EOF
}

# Get session info by full session name.
# Args: $1 = full session name, $2 = branch hint (optional)
# Returns: JSON-formatted session information
session_get_info_by_name() {
    local session_name="$1"
    local branch_hint="${2:-$(session_extract_branch "$session_name")}"
    _session_ensure_terminal_adapter

    if ! session_exists "$session_name"; then
        echo "{\"exists\": false}"
        return 1
    fi

    local adapter
    adapter=$(terminal_adapter_get)

    local windows
    windows=$(terminal_adapter_get_window_count "$session_name" 2>/dev/null || echo "0")
    local attached=false
    if terminal_adapter_is_session_attached "$session_name" 2>/dev/null; then
        attached=true
    fi

    local safe_name safe_branch safe_adapter
    safe_name=$(json_escape "$session_name")
    safe_branch=$(json_escape "$branch_hint")
    safe_adapter=$(json_escape "$adapter")

    cat <<EOF
{
  "exists": true,
  "name": "$safe_name",
  "branch": "$safe_branch",
  "adapter": "$safe_adapter",
  "windows": $windows,
  "attached": $attached
}
EOF
}

# Validate session health
# Args: $1 = branch name
# Returns: 0 = healthy, 1 = has issues
session_validate() {
    local branch="$1"
    _session_ensure_terminal_adapter
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    # Check if session is responsive
    if ! terminal_adapter_get_window_count "$session_name" &>/dev/null; then
        log_error "$(_i18n ERR_SESSION_UNRESPONSIVE) $session_name"
        return 1
    fi

    log_debug "$(_i18n DEBUG_SESSION_HEALTHY) $session_name"
    return 0
}

# Validate session health by full session name.
# Args: $1 = full session name
# Returns: 0 = healthy, 1 = has issues
session_validate_by_name() {
    local session_name="$1"
    _session_ensure_terminal_adapter

    if ! session_exists "$session_name"; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $session_name"
        return 1
    fi

    if ! terminal_adapter_get_window_count "$session_name" &>/dev/null; then
        log_error "$(_i18n ERR_SESSION_UNRESPONSIVE) $session_name"
        return 1
    fi

    log_debug "$(_i18n DEBUG_SESSION_HEALTHY) $session_name"
    return 0
}

# Send a tmux notification message (display-message)
# Args: $1 = branch name, $2 = message content, $3 = duration (milliseconds, optional, default 3000)
session_notify() {
    local branch="$1"
    local message="$2"
    local duration="${3:-3000}"
    _session_ensure_terminal_adapter

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    terminal_adapter_display_message "$session_name" "$message" "$duration"

    if [[ "$(platform_detect)" == "macos" ]]; then
        if command -v terminal-notifier &>/dev/null; then
            (terminal-notifier -title "workhive" -message "$message" -group "workhive" &>/dev/null) & disown 2>/dev/null || true
        elif command -v osascript &>/dev/null; then
            (NOTIFY_MSG="$message" osascript -e 'display notification (system attribute "NOTIFY_MSG") with title "workhive"' &>/dev/null) & disown 2>/dev/null || true
        fi
    fi

    return 0
}

# Send a command to a specific pane in a session
# Args: $1 = branch name, $2 = pane index, $3 = command
session_send_command() {
    local branch="$1"
    local pane_index="$2"
    local command="$3"
    _session_ensure_terminal_adapter

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    log_debug "$(_i18n DEBUG_SEND_CMD_TO_PANE) $pane_index: $command"

    # Send command
    terminal_adapter_send_keys "$session_name" "$pane_index" "$command"
}

# Execute a startup command in pane with minimal visual echo.
# Intended for workspace/layout bootstrap only.
# Args: $1 = branch name, $2 = pane index, $3 = command, $4 = workdir(optional)
session_send_startup_command() {
    local branch="$1"
    local pane_index="$2"
    local command="$3"
    local workdir="${4:-}"
    _session_ensure_terminal_adapter

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"
    local startup_cmd="$command"
    local env_prefix
    env_prefix=$(_session_get_tmux_env_prefix)
    if [[ -n "$env_prefix" ]]; then
        startup_cmd="${env_prefix}${startup_cmd}"
    fi
    log_debug "session_send_startup_command: session=${session_name} pane=${pane_index} cmd='${command:-<empty>}' workdir='${workdir:-<empty>}'"

    # Prefer adapter-native non-typing execution when available.
    if declare -f terminal_adapter_spawn_command_target >/dev/null 2>&1; then
        if terminal_adapter_spawn_command_target "${session_name}:0.${pane_index}" "$startup_cmd" "$workdir"; then
            log_debug "session_send_startup_command: spawn_target ok session=${session_name} pane=${pane_index}"
            return 0
        fi
        log_warn "session_send_startup_command: spawn_target failed, fallback to send_keys session=${session_name} pane=${pane_index}"
    elif declare -f terminal_adapter_spawn_pane_command >/dev/null 2>&1; then
        # Backward-compatible fallback
        if terminal_adapter_spawn_pane_command "$session_name" "$pane_index" "$startup_cmd" "$workdir"; then
            log_debug "session_send_startup_command: spawn_pane ok session=${session_name} pane=${pane_index}"
            return 0
        fi
        log_warn "session_send_startup_command: spawn_pane failed, fallback to send_keys session=${session_name} pane=${pane_index}"
    fi

    # Fallback: emulate typing (legacy behavior)
    local fallback_cmd="$startup_cmd"
    if [[ -n "$workdir" ]]; then
        local safe_dir
        safe_dir=$(printf '%q' "$workdir")
        fallback_cmd="cd ${safe_dir}; clear; ${command}"
    fi
    if terminal_adapter_send_keys "$session_name" "$pane_index" "$fallback_cmd"; then
        log_debug "session_send_startup_command: send_keys fallback ok session=${session_name} pane=${pane_index}"
        return 0
    fi

    log_error "session_send_startup_command: all strategies failed session=${session_name} pane=${pane_index}"
    return 1
}

# Initialize an empty shell pane via adapter-native login-shell spawn when
# available. This keeps empty panes out of the generic "run a command" path.
# Args: $1 = branch, $2 = pane_index, $3 = workdir(optional)
session_prepare_shell_pane() {
    local branch="$1"
    local pane_index="$2"
    local workdir="${3:-}"
    _session_ensure_terminal_adapter

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    log_debug "session_prepare_shell_pane: session=${session_name} pane=${pane_index} dir='${workdir:-<empty>}'"

    local pane_snapshot
    pane_snapshot=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}|#{pane_current_command}|#{pane_current_path}|#{pane_dead}' 2>/dev/null | awk -F'|' -v idx="$pane_index" '$1 == idx {print; exit}')
    if [[ -n "$pane_snapshot" ]]; then
        local _idx pane_cmd pane_cwd pane_dead
        IFS='|' read -r _idx pane_cmd pane_cwd pane_dead <<< "$pane_snapshot"
        if [[ "${pane_dead:-0}" != "1" && "${pane_cmd:-}" =~ ^(zsh|bash|sh|fish|dash|ksh|tcsh|csh)$ ]]; then
            if [[ -z "$workdir" || "$pane_cwd" == "$workdir" ]]; then
                log_debug "session_prepare_shell_pane: keep existing pane shell session=${session_name} pane=${pane_index} cwd='${pane_cwd:-<empty>}'"
                return 0
            fi
        fi
    fi

    if declare -f terminal_adapter_spawn_login_shell_target >/dev/null 2>&1; then
        if terminal_adapter_spawn_login_shell_target "${session_name}:0.${pane_index}" "$workdir"; then
            log_debug "session_prepare_shell_pane: spawn_login_shell ok session=${session_name} pane=${pane_index}"
            return 0
        fi
        log_warn "session_prepare_shell_pane: spawn_login_shell failed, fallback to startup command session=${session_name} pane=${pane_index}"
    fi

    session_send_startup_command "$branch" "$pane_index" ":" "$workdir"
}

# ──────────────────────────────────────────────────
# Window operations (for multi-window sessions)
# ──────────────────────────────────────────────────

# Create a new window in an existing session
# Args: $1 = session name (full name), $2 = window name, $3 = working directory
# Returns: 0 = success, 1 = failure
session_create_window() {
    local session_name="$1"
    local window_name="$2"
    local workdir="$3"
    _session_ensure_terminal_adapter

    if ! terminal_adapter_has_session "$session_name" 2>/dev/null; then
        log_error "$(_i18n ERR_SESSION_NOT_FOUND) $session_name"
        return 1
    fi

    terminal_adapter_new_window "$session_name" "$window_name" "$workdir" 2>/dev/null || {
        log_error "$(_i18n_fmt ERR_SESSION_CREATE_WINDOW_FAILED_FMT "$window_name" "$session_name")"
        return 1
    }

    return 0
}

# Rename a window in a session
# Args: $1 = session name, $2 = window index, $3 = new name
session_rename_window() {
    local session_name="$1"
    local window_index="$2"
    local new_name="$3"
    _session_ensure_terminal_adapter

    terminal_adapter_rename_window "$session_name" "$window_index" "$new_name" 2>/dev/null || true
}

# Select (focus on) a specific window
# Args: $1 = session name, $2 = window index
session_select_window() {
    local session_name="$1"
    local window_index="$2"
    _session_ensure_terminal_adapter

    terminal_adapter_select_window "$session_name" "$window_index" 2>/dev/null || true
}

# Send a command to a pane in a specific window
# Args: $1 = session name, $2 = window index, $3 = pane index, $4 = command
session_send_command_to_window() {
    local session_name="$1"
    local window_index="$2"
    local pane_index="$3"
    local command="$4"
    _session_ensure_terminal_adapter

    terminal_adapter_send_keys_to_window "$session_name" "$window_index" "$pane_index" "$command" 2>/dev/null || true
}

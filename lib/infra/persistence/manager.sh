#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Persistence Manager
# Top-level API for the builtin persistence module.
# Coordinates save, restore, auto-save, and config queries.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_MANAGER_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_MANAGER_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/persistence/snapshot.sh"
source "${LIB_DIR}/infra/persistence/save.sh"
source "${LIB_DIR}/infra/persistence/restore.sh"
source "${LIB_DIR}/infra/persistence/auto_save.sh"

# ──────────────────────────────────────────────────
# Persistence — Default Values (Single Source of Truth)
# All defaults live here. Never hardcode these values elsewhere.
# ──────────────────────────────────────────────────
readonly _PERSIST_DEFAULT_ENABLED="false"
readonly _PERSIST_DEFAULT_AUTO_SAVE_INTERVAL="10m"
readonly _PERSIST_DEFAULT_RESTORE_PROCESSES="true"
readonly _PERSIST_DEFAULT_AUTO_SAVE_MODE="hybrid"
readonly _PERSIST_DEFAULT_AUTO_SAVE_DEBOUNCE="3s"
readonly _PERSIST_DEFAULT_GENERATION=0
readonly _PERSIST_DEFAULT_SNAPSHOT_HISTORY_LIMIT=8
readonly _PERSIST_DEFAULT_SNAPSHOT_MAX_SIZE_MB=8

# ──────────────────────────────────────────────────
# Typed config getters (use these instead of raw _persistence_config_get)
# ──────────────────────────────────────────────────
persistence_cfg_enabled()           { _persistence_config_get "enabled"           "$_PERSIST_DEFAULT_ENABLED"; }
persistence_cfg_auto_save_interval(){ _persistence_config_get "auto_save_interval" "$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL"; }
persistence_cfg_restore_processes() { _persistence_config_get "restore_processes"  "$_PERSIST_DEFAULT_RESTORE_PROCESSES"; }
persistence_cfg_auto_save_mode()    { _persistence_config_get "auto_save_mode" "$_PERSIST_DEFAULT_AUTO_SAVE_MODE"; }
persistence_cfg_auto_save_debounce(){ _persistence_config_get "auto_save_debounce" "$_PERSIST_DEFAULT_AUTO_SAVE_DEBOUNCE"; }
persistence_cfg_generation()        { _persistence_config_get "generation" "$_PERSIST_DEFAULT_GENERATION"; }
persistence_cfg_snapshot_history_limit(){ _persistence_config_get "snapshot_history_limit" "$_PERSIST_DEFAULT_SNAPSHOT_HISTORY_LIMIT"; }
persistence_cfg_snapshot_max_size_mb(){ _persistence_config_get "snapshot_max_size_mb" "$_PERSIST_DEFAULT_SNAPSHOT_MAX_SIZE_MB"; }

# ──────────────────────────────────────────────────
# Configuration queries (low-level)
# ──────────────────────────────────────────────────

# Per-process cache for persistence config values
declare -A _PERSISTENCE_CFG_CACHE=()

# Read a persistence config key from project -> global -> defaults.
# $1 = key suffix (e.g. "enabled", "auto_save_interval")
# $2 = default value (optional, prefer typed getters above)
_persistence_config_get() {
    local suffix="$1"
    local default_value="${2:-}"

    # Ensure cache stays associative even if polluted by caller/test overrides
    local _cache_decl
    _cache_decl=$(declare -p _PERSISTENCE_CFG_CACHE 2>/dev/null || true)
    if [[ "$_cache_decl" != "declare -A "* ]]; then
        unset _PERSISTENCE_CFG_CACHE 2>/dev/null || true
        declare -g -A _PERSISTENCE_CFG_CACHE=()
    fi

    # Cache hit — return immediately
    if [[ -n "${_PERSISTENCE_CFG_CACHE[$suffix]+set}" ]]; then
        local _cached="${_PERSISTENCE_CFG_CACHE[$suffix]}"
        [[ -n "$_cached" ]] && printf '%s' "$_cached"
        return 0
    fi

    local new_key="tmux.persistence.${suffix}"
    # Fast path: if yq is not available, use config_get chain
    if ! check_cmd yq; then
        local value
        value=$(config_get "$new_key" 2>/dev/null || echo "")
        _is_null_or_empty "$value" && value="$default_value"
        _PERSISTENCE_CFG_CACHE["$suffix"]="${value:-}"
        [[ -n "$value" ]] && printf '%s' "$value"
        return 0
    fi

    local value=""
    local _has_file=false

    # 1. Project config
    local _project_config
    _project_config="$(_workhive_dir)/config.yaml"
    if [[ -f "$_project_config" ]]; then
        _has_file=true
        value=$(yq ".${new_key}" "$_project_config" 2>/dev/null) || value=""
    fi

    # 2. Global config
    if _is_null_or_empty "$value"; then
        local user_config
        user_config=$(config_get_global_file)
        if [[ -f "$user_config" ]]; then
            _has_file=true
            value=$(yq ".${new_key}" "$user_config" 2>/dev/null) || value=""
        fi
    fi

    # 3. Built-in defaults (only new key exists in defaults)
    if _is_null_or_empty "$value"; then
        value=$(config_get_defaults | yq ".${new_key}" 2>/dev/null) || value=""
    fi

    # If no config files existed at all, fall back to config_get
    # (supports test environments with stubbed config_get)
    if [[ "$_has_file" == false ]] && _is_null_or_empty "$value"; then
        value=$(config_get "$new_key" 2>/dev/null || echo "")
    fi

    # Normalize
    [[ "$value" == "null" ]] && value=""
    # yq may preserve inline comments for some scalar/collection outputs.
    # Keep persistence config reads comment-safe and fail-open.
    value="${value%%#*}"
    value="${value%"${value##*[![:space:]]}"}"

    # Resolve final value
    local _final=""
    if [[ -n "$value" ]]; then
        _final="$value"
    elif [[ -n "$default_value" ]]; then
        _final="$default_value"
    fi

    # Cache and return
    _PERSISTENCE_CFG_CACHE["$suffix"]="$_final"
    [[ -n "$_final" ]] && printf '%s' "$_final"
}

# Check if builtin persistence is enabled and active
# Returns: 0 = active, 1 = not active
persistence_is_enabled() {
    local enabled
    enabled=$(persistence_cfg_enabled)
    [[ "$enabled" == "true" ]]
}

# Return persistence enabled status as JSON-friendly bool text.
# Returns (stdout): "true" | "false"
persistence_enabled_bool() {
    if persistence_is_enabled; then
        echo "true"
    else
        echo "false"
    fi
}

# Return latest snapshot file path if it exists, otherwise empty.
persistence_latest_snapshot_path() {
    local latest
    latest=$(snapshot_get_latest 2>/dev/null || echo "")
    if [[ -n "$latest" && -f "$latest" ]]; then
        printf '%s\n' "$latest"
    else
        printf '\n'
    fi
}

_persistence_file_backup_time() {
    local snapshot_path="$1"
    local mode="${2:-}"
    local backup_time
    if [[ -n "$mode" ]]; then
        backup_time=$(_file_mtime "$snapshot_path" "$mode")
    else
        backup_time=$(_file_mtime "$snapshot_path")
    fi
    printf '%s\n' "${backup_time:-}"
}

# Return latest snapshot save time, or "none" if no valid latest snapshot file exists.
# Args:
#   $1 = time mode passed to _file_mtime (optional): e.g. "iso"
# Returns (stdout):
#   timestamp string | "none"
persistence_last_backup_time() {
    local mode="${1:-}"
    persistence_latest_snapshot_time_or "$mode" "none"
}

# Return latest snapshot path and its save time in one query result.
# Args:
#   $1 = time mode passed to persistence_last_backup_time (optional), e.g. "iso"
# Returns (stdout):
#   "<path>|<time>" when snapshot exists
#   "|none" when snapshot does not exist
persistence_latest_snapshot_info() {
    local mode="${1:-}"
    local latest
    latest=$(persistence_latest_snapshot_path)
    if [[ -z "$latest" ]]; then
        printf '|none\n'
        return 0
    fi

    local save_time
    save_time=$(_persistence_file_backup_time "$latest" "$mode")
    printf '%s|%s\n' "$latest" "${save_time:-}"
}

# Return latest snapshot time with caller-defined fallback semantics.
# Args:
#   $1 = time mode passed to persistence_latest_snapshot_info (optional), e.g. "iso"
#   $2 = fallback value when no snapshot/time is available (optional, default: "none")
# Returns (stdout):
#   "<time>" or fallback
persistence_latest_snapshot_time_or() {
    local mode="${1:-}"
    local fallback="${2:-none}"
    local latest save_time
    _persistence_read_latest_snapshot_info "$mode" latest save_time
    if [[ -z "$latest" || -z "$save_time" || "$save_time" == "none" ]]; then
        printf '%s\n' "$fallback"
        return 0
    fi
    printf '%s\n' "$save_time"
}

_persistence_read_latest_snapshot_info() {
    local mode="${1:-}"
    local -n _plsi_latest_ref=$2
    local -n _plsi_time_ref=$3
    IFS='|' read -r _plsi_latest_ref _plsi_time_ref <<< "$(persistence_latest_snapshot_info "$mode")"
}

# ──────────────────────────────────────────────────
# High-level operations
# ──────────────────────────────────────────────────

# Try to restore a session from snapshot, or return 1 to signal
# the caller should use the default config.yaml-based creation.
# This is the integration point for open_command.sh / attach_command.sh.
#
# Logic:
#   persistence.enabled=false → return 1 (use default logic)
#   persistence.enabled=true:
#     has snapshot → restore from snapshot → return 0
#     no snapshot → return 1 (fall back to config.yaml)
#
# $1 = branch name
# Returns: 0 = restored from snapshot, 1 = use default logic
persistence_try_restore_session() {
    local branch="$1"
    log_debug "persistence_try_restore_session: branch=${branch:-<empty>}"

    # Gate: persistence must be enabled
    if ! persistence_is_enabled; then
        log_debug "persistence_try_restore_session: skip restore, reason=persistence_disabled"
        return 1
    fi

    # Gate: session must not already exist (idempotent)
    local session_name
    session_name=$(session_get_name "$branch" 2>/dev/null || echo "")
    if [[ -n "$session_name" ]] && tmux has-session -t "$session_name" 2>/dev/null; then
        log_debug "persistence_try_restore_session: skip restore, reason=session_exists, session=${session_name}"
        return 1
    fi

    # Gate: must have snapshot data for this session
    if ! persistence_has_snapshot "$branch"; then
        log_debug "persistence_try_restore_session: skip restore, reason=no_snapshot_data, branch=${branch}"
        return 1
    fi

    log_debug "persistence_try_restore_session: restore triggered, reason=snapshot_available"

    # Attempt restore (trap unexpected errors to ensure graceful degradation)
    local rc=0
    persistence_restore_session "$branch" || rc=$?

    if [[ $rc -eq 0 ]]; then
        cache_invalidate "session:*" 2>/dev/null || true
        log_debug "persistence_try_restore_session: restore succeeded, session=${session_name:-<unknown>}, skip layout create because snapshot already contains layout + pane commands"
        log_info "$(_i18n PERSIST_RESTORED_FROM_SNAPSHOT)"
        return 0
    fi

    # Ensure no partially created session remains
    if [[ -n "$session_name" ]] && tmux has-session -t "$session_name" 2>/dev/null; then
        log_debug "persistence_try_restore_session: killing partial session after failed restore"
        tmux kill-session -t "$session_name" 2>/dev/null || true
    fi

    log_debug "persistence_try_restore_session: restore failed, rc=${rc}, fallback=layout_create"

    return 1
}

# Save before destroying a session (so it can be recovered)
# $1 = branch name
persistence_save_before_kill() {
    local branch="$1"
    if ! persistence_is_enabled; then
        return 0
    fi
    persistence_save_session "$branch" "before-kill-session" "persistence_save_before_kill" 2>/dev/null || true
}

# ──────────────────────────────────────────────────
# Status & diagnostics
# ──────────────────────────────────────────────────

# Print persistence status information
persistence_status() {
    if ! persistence_is_enabled; then
        echo "disabled"
        return 0
    fi

    echo "enabled"

    local auto_save_interval
    auto_save_interval=$(persistence_cfg_auto_save_interval)
    echo "auto_save_interval=${auto_save_interval}"
    local auto_save_mode
    auto_save_mode=$(persistence_cfg_auto_save_mode)
    echo "auto_save_mode=${auto_save_mode}"
    local auto_save_debounce
    auto_save_debounce=$(persistence_cfg_auto_save_debounce)
    echo "auto_save_debounce=${auto_save_debounce}"
    local snapshot_history_limit
    snapshot_history_limit=$(persistence_cfg_snapshot_history_limit)
    echo "snapshot_history_limit=${snapshot_history_limit}"

    local snapshot_max_size_mb
    snapshot_max_size_mb=$(persistence_cfg_snapshot_max_size_mb)
    echo "snapshot_max_size_mb=${snapshot_max_size_mb}"

    local snap_dir
    snap_dir=$(snapshot_get_dir)
    echo "snapshot_dir=${snap_dir}"

    local latest save_time
    _persistence_read_latest_snapshot_info "" latest save_time
    if [[ -n "$latest" ]]; then
        [[ -n "$save_time" ]] && echo "last_save=${save_time}"

        local sessions_in_snapshot=0
        local _sline
        while IFS= read -r _sline; do
            [[ -n "$_sline" ]] && sessions_in_snapshot=$((sessions_in_snapshot + 1))
        done < <(snapshot_get_sessions "$latest")
        echo "sessions_in_snapshot=${sessions_in_snapshot}"
    else
        echo "last_save=none"
    fi
}

# Doctor: check persistence health
persistence_doctor() {
    local healthy=true

    if persistence_is_enabled; then
        echo "  ✓ Builtin persistence enabled"

        # Show auto-save interval
        local interval
        interval=$(persistence_cfg_auto_save_interval)
        echo "    Auto-save interval: ${interval}"

        # Show auto-restore status

        local snap_dir
        snap_dir=$(snapshot_get_dir)
        if [[ -d "$snap_dir" ]]; then
            echo "  ✓ Snapshot directory exists: $snap_dir"
        else
            echo "  ⚠ Snapshot directory not yet created (will be created on first save)"
        fi

        local latest save_time
        _persistence_read_latest_snapshot_info "" latest save_time
        if [[ -n "$latest" ]]; then
            [[ -n "$save_time" ]] && echo "  ✓ Last backup: $save_time"

            local instance_id
            instance_id=$(session_get_instance_id 2>/dev/null || echo "")
            if snapshot_validate "$latest" "$instance_id"; then
                echo "  ✓ Latest snapshot valid"
            else
                echo "  ✗ Latest snapshot validation failed"
                healthy=false
            fi
        else
            echo "  ○ No snapshots yet (auto-save will create them)"
        fi
    else
        echo "  ○ Builtin persistence is disabled"
        echo "    Enable: workhive config → Session Persistence"
    fi

    [[ "$healthy" == true ]]
}

_persistence_hash_text() {
    local text="$1"
    _snapshot_hash_text "$text"
}

_persistence_runtime_signature() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 1
    tmux has-session -t "$session_name" 2>/dev/null || return 1

    local payload=""
    payload+=$(tmux list-panes -t "$session_name" -F '#{session_name}|#{window_index}|#{pane_index}|#{pane_current_path}|#{pane_current_command}|#{pane_active}|#{pane_title}' 2>/dev/null | sort)
    payload+=$'\n'
    payload+=$(tmux list-windows -t "$session_name" -F '#{session_name}|#{window_index}|#{window_name}|#{window_layout}|#{window_active}' 2>/dev/null | sort)
    _persistence_hash_text "$payload"
}

persistence_save_if_runtime_changed() {
    local session_name="$1"
    local trigger="${2:-unknown}"
    [[ -n "$session_name" ]] || return 1
    if ! persistence_is_enabled; then
        return 1
    fi

    local signature
    signature=$(_persistence_runtime_signature "$session_name" 2>/dev/null || echo "")
    [[ -n "$signature" ]] || return 1

    local prev_signature
    prev_signature=$(tmux show-options -t "$session_name" -v "@workhive-runtime-signature" 2>/dev/null || echo "")
    if [[ -n "$prev_signature" && "$prev_signature" == "$signature" ]]; then
        log_debug "Auto-save runtime unchanged: session=${session_name}, trigger=${trigger}"
        return 2
    fi

    tmux set-option -t "$session_name" "@workhive-runtime-signature" "$signature" 2>/dev/null || true
    log_debug "Auto-save runtime changed: session=${session_name}, trigger=${trigger}"
    persistence_save "event" "runtime-changed:${session_name}:trigger=${trigger}" 2>/dev/null || true
    return 0
}

persistence_save_runtime_debounced() {
    local session_name="$1"
    local debounce="${2:-3}"
    local trigger="${3:-${WORKHIVE_AUTO_SAVE_TRIGGER:-unknown}}"
    [[ -n "$session_name" ]] || return 1
    if ! persistence_is_enabled; then
        return 1
    fi
    [[ "$debounce" =~ ^[0-9]+$ ]] || debounce=3
    (( debounce < 0 )) && debounce=0

    local now
    now=$(date +%s 2>/dev/null || echo "0")
    [[ "$now" =~ ^[0-9]+$ ]] || now=0

    local last_ts
    last_ts=$(tmux show-options -t "$session_name" -v "@workhive-save-last-ts" 2>/dev/null || echo "0")
    [[ "$last_ts" =~ ^[0-9]+$ ]] || last_ts=0

    if (( debounce > 0 && now > 0 && (now - last_ts) < debounce )); then
        log_debug "Auto-save debounced: session=${session_name}, trigger=${trigger}, debounce=${debounce}s"
        return 2
    fi

    tmux set-option -t "$session_name" "@workhive-save-last-ts" "$now" 2>/dev/null || true
    persistence_save_if_runtime_changed "$session_name" "$trigger"
}

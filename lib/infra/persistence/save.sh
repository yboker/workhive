#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Persistence Save
# Collects tmux session/window/pane data and writes a snapshot.
# Project-scoped: only saves sessions matching the current project prefix.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_SAVE_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_SAVE_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/persistence/snapshot.sh"
source "${LIB_DIR}/infra/persistence/process_capture.sh"

# ──────────────────────────────────────────────────
# Pre-flight: validate prefix + tmux availability
# Sets _SAVE_PREFIX and _SAVE_MATCHING on success
# Returns: 0 = ready, 1 = skip
# ──────────────────────────────────────────────────
_save_preflight() {
    _SAVE_PREFIX=$(session_get_prefix 2>/dev/null || echo "")
    if [[ -z "$_SAVE_PREFIX" ]]; then
        log_error "$(_i18n PERSIST_ERR_NO_PREFIX)"
        return 1
    fi

    if ! command -v tmux &>/dev/null || ! tmux list-sessions &>/dev/null 2>&1; then
        log_debug "tmux not running, skip save"
        return 1
    fi

    _SAVE_MATCHING=$(tmux list-sessions -F "#{session_name}" 2>/dev/null | grep "^${_SAVE_PREFIX}@" || true)
    if [[ -z "$_SAVE_MATCHING" ]]; then
        log_debug "No sessions matching prefix: $_SAVE_PREFIX"
        return 1
    fi

    local _save_runtime_server=""
    if declare -f tmux_runtime_server_name >/dev/null 2>&1; then
        _save_runtime_server=$(tmux_runtime_server_name 2>/dev/null || echo "unknown")
    fi
    log_debug "save preflight: server=${_save_runtime_server} prefix=${_SAVE_PREFIX} matching_sessions=$(echo "$_SAVE_MATCHING" | wc -l | tr -d ' ')"
    return 0
}

# Write snapshot content to file (runs in subshell with umask 077)
_save_write_snapshot() {
    local new_file="$1" prefix="$2" reason="$3" context="$4"
    local old_content="$5"
    local instance_id project_name wh_version generation
    instance_id=$(session_get_instance_id 2>/dev/null || echo "")
    project_name=$(session_get_project_name 2>/dev/null || echo "unknown")
    wh_version="${WORKHIVE_VERSION:-unknown}"
    generation=$(snapshot_current_generation 2>/dev/null || echo "0")

    (
        umask 077
        {
            snapshot_format_meta "$instance_id" "$project_name" "$prefix" "$wh_version" "$generation"
            _save_dump_panes "$prefix" "$reason" "$context"
            _save_dump_windows "$prefix"
            [[ -n "$old_content" ]] && echo "$old_content"
            _save_dump_state "$prefix"
        } > "$new_file"
    )
}

# Snapshot save lock (per project) to avoid concurrent temp-file finalize races.
_SAVE_LOCK_DIR=""
readonly _SAVE_LOCK_WAIT_MAX=50
readonly _SAVE_LOCK_WAIT_SEC="0.1"
readonly _SAVE_LOCK_STALE_SEC=600
readonly _SAVE_QUALITY_DEFAULT_MAX_RETRIES=3
readonly _SAVE_QUALITY_DEFAULT_RETRY_SLEEP_SEC="0.2"
readonly _SAVE_QUALITY_DEFAULT_MIN_SCORE=40

_save_lock_log_contention() {
    if [[ "${WORKHIVE_SAVE_LOCK_QUIET:-0}" == "1" ]]; then
        log_debug "$(_i18n PERSIST_WARN_SAVE_SKIPPED_LOCKED)"
    else
        log_warn "$(_i18n PERSIST_WARN_SAVE_SKIPPED_LOCKED)"
    fi
}

_save_quality_log_rejected_final() {
    local attempts="$1"
    local msg="Snapshot quality gate failed after ${attempts} attempts, keeping last known good snapshot"
    if [[ "${WORKHIVE_SAVE_QUALITY_QUIET:-0}" == "1" ]]; then
        log_debug "$msg"
    else
        log_warn "$msg"
    fi
}

_save_lock_age_seconds() {
    local lock_dir="$1"
    local mtime now
    if [[ "$(uname)" == "Darwin" ]]; then
        mtime=$(stat -f '%m' "$lock_dir" 2>/dev/null || echo "")
    else
        mtime=$(stat -c '%Y' "$lock_dir" 2>/dev/null || echo "")
    fi
    [[ "$mtime" =~ ^[0-9]+$ ]] || return 1
    now=$(date +%s 2>/dev/null || echo "")
    [[ "$now" =~ ^[0-9]+$ ]] || return 1
    echo $((now - mtime))
}

_save_quality_max_retries() {
    local raw="${WORKHIVE_SAVE_QUALITY_MAX_RETRIES:-$_SAVE_QUALITY_DEFAULT_MAX_RETRIES}"
    [[ "$raw" =~ ^[0-9]+$ ]] || raw="$_SAVE_QUALITY_DEFAULT_MAX_RETRIES"
    (( raw < 1 )) && raw=1
    echo "$raw"
}

_save_quality_retry_sleep_seconds() {
    local raw="${WORKHIVE_SAVE_QUALITY_RETRY_SLEEP_SEC:-$_SAVE_QUALITY_DEFAULT_RETRY_SLEEP_SEC}"
    [[ "$raw" =~ ^[0-9]+([.][0-9]+)?$ ]] || raw="$_SAVE_QUALITY_DEFAULT_RETRY_SLEEP_SEC"
    echo "$raw"
}

_save_quality_min_score() {
    local raw="${WORKHIVE_SNAPSHOT_MIN_HEALTH_SCORE:-$_SAVE_QUALITY_DEFAULT_MIN_SCORE}"
    [[ "$raw" =~ ^[0-9]+$ ]] || raw="$_SAVE_QUALITY_DEFAULT_MIN_SCORE"
    (( raw < 0 )) && raw=0
    (( raw > 100 )) && raw=100
    echo "$raw"
}

_save_snapshot_quality_ok() {
    local file="$1"
    local prefix="$2"
    [[ -f "$file" && -n "$prefix" ]] || return 1

    local min_score
    min_score=$(_save_quality_min_score)
    local found=0 healthy=0 unhealthy_names=""
    local session_name
    while IFS= read -r session_name; do
        [[ -n "$session_name" ]] || continue
        [[ "$session_name" == "${prefix}@"* ]] || continue
        found=$((found + 1))
        local score
        score=$(snapshot_session_health_score "$file" "$session_name" 2>/dev/null || echo "0")
        [[ "$score" =~ ^[0-9]+$ ]] || score=0
        if (( score >= min_score )); then
            healthy=$((healthy + 1))
        else
            unhealthy_names+=" ${session_name}(${score})"
        fi
    done < <(snapshot_get_sessions "$file" 2>/dev/null || true)

    if (( found == 0 )); then
        log_debug "Snapshot candidate rejected: no matching sessions for prefix=${prefix}, file=$(basename "$file")"
        return 1
    fi

    if (( healthy == 0 )); then
        log_debug "Snapshot candidate rejected: no healthy sessions (min=${min_score}), unhealthy:${unhealthy_names}, file=$(basename "$file")"
        return 1
    fi

    if [[ -n "$unhealthy_names" ]]; then
        log_debug "Snapshot accepted with degraded sessions: healthy=${healthy}/${found}, degraded:${unhealthy_names}"
    fi

    return 0
}

_save_lock_is_stale() {
    local lock_dir="$1"
    [[ -d "$lock_dir" ]] || return 0

    local pid=""
    if [[ -f "${lock_dir}/pid" ]]; then
        read -r pid < "${lock_dir}/pid" 2>/dev/null || pid=""
        pid="${pid//[^0-9]/}"
    fi

    if [[ -n "$pid" ]]; then
        kill -0 "$pid" 2>/dev/null && return 1
        return 0
    fi

    local age
    age=$(_save_lock_age_seconds "$lock_dir" 2>/dev/null || echo "")
    [[ "$age" =~ ^[0-9]+$ ]] || return 1
    (( age >= _SAVE_LOCK_STALE_SEC ))
}

_save_lock_acquire() {
    local snap_dir lock_dir
    snap_dir=$(snapshot_get_dir)
    [[ -d "$snap_dir" ]] || (umask 077 && mkdir -p "$snap_dir")

    lock_dir="${snap_dir}/.save.lock"
    local attempts=0
    while ! mkdir "$lock_dir" 2>/dev/null; do
        if _save_lock_is_stale "$lock_dir"; then
            log_debug "$(_i18n_fmt PERSIST_WARN_STALE_LOCK_REMOVING_FMT "$lock_dir")"
            cleanup_dir_quiet "$lock_dir"
            continue
        fi
        attempts=$((attempts + 1))
        if [[ "$attempts" -ge "$_SAVE_LOCK_WAIT_MAX" ]]; then
            _save_lock_log_contention
            return 1
        fi
        sleep "$_SAVE_LOCK_WAIT_SEC"
    done

    _SAVE_LOCK_DIR="$lock_dir"
    printf '%s\n' "$$" > "${lock_dir}/pid" 2>/dev/null || true
    return 0
}

_save_lock_release() {
    local lock_dir="${_SAVE_LOCK_DIR:-}"
    [[ -z "$lock_dir" ]] && return 0
    cleanup_dir_quiet "$lock_dir"
    _SAVE_LOCK_DIR=""
}

# ──────────────────────────────────────────────────
# Main save entry point
# Returns: 0 = success, 1 = failure, 2 = no change (identical to last)
# ──────────────────────────────────────────────────
persistence_save() {
    local reason="${1:-}" context="${2:-}"

    # Hard gate: never write snapshots when persistence is disabled.
    # Keep this check local so stale hooks / internal save commands are harmless.
    if declare -f persistence_is_enabled >/dev/null 2>&1; then
        if ! persistence_is_enabled; then
            return 0
        fi
    fi

    _save_preflight || return 1
    _save_lock_acquire || return 1

    local last_file old_sessions_content=""
    last_file=$(snapshot_get_latest 2>/dev/null || echo "")
    if [[ -n "$last_file" && -f "$last_file" ]]; then
        old_sessions_content=$(_save_collect_dormant_records "$last_file" "$_SAVE_MATCHING")
    fi

    local max_retries retry_sleep
    max_retries=$(_save_quality_max_retries)
    retry_sleep=$(_save_quality_retry_sleep_seconds)

    local attempt
    for ((attempt=1; attempt<=max_retries; attempt++)); do
        local new_file
        new_file=$(snapshot_new_file_path) || {
            _save_lock_release
            log_error "$(_i18n PERSIST_ERR_CREATE_TEMP_SNAPSHOT)"
            return 1
        }

        _save_write_snapshot "$new_file" "$_SAVE_PREFIX" "$reason" "$context" "$old_sessions_content"

        if [[ -n "$last_file" ]] && ! snapshot_has_changed "$new_file" "$last_file"; then
            cleanup_file_quiet "$new_file"
            _save_lock_release
            log_debug "Snapshot unchanged, skipped"
            return 2
        fi

        if ! _save_snapshot_quality_ok "$new_file" "$_SAVE_PREFIX"; then
            cleanup_file_quiet "$new_file"
            if (( attempt < max_retries )); then
                sleep "$retry_sleep"
                continue
            fi
            _save_lock_release
            _save_quality_log_rejected_final "$max_retries"
            return 1
        fi

        if ! snapshot_update_last "$new_file"; then
            cleanup_file_quiet "$new_file"
            _save_lock_release
            log_error "$(_i18n PERSIST_ERR_FINALIZE_SNAPSHOT)"
            return 1
        fi

        local snap_dir
        snap_dir=$(dirname "$new_file")
        _save_lock_release
        log_debug "Snapshot saved: ${snap_dir}/${_SNAPSHOT_LATEST_BASENAME}${_SNAPSHOT_EXT} (reason=${reason:-manual}, attempt=${attempt}/${max_retries})"
        snapshot_cleanup 2>/dev/null || true
        return 0
    done

    _save_lock_release
    return 1
}

# ──────────────────────────────────────────────────
# Dump all pane records for matching sessions
# ──────────────────────────────────────────────────
_save_dump_panes() {
    local prefix="$1"
    # $2/$3 reserved for callers passing reason/context (kept for API compat)
    # shellcheck disable=SC2034
    local save_reason="${2:-}" save_context="${3:-}"
    local d=$'\t'

    local pane_format
    pane_format="#{session_name}${d}#{window_index}${d}#{window_active}${d}#{window_flags}${d}#{pane_index}${d}#{pane_title}${d}#{pane_current_path}${d}#{pane_active}${d}#{pane_current_command}${d}#{pane_pid}"

    local pane_lines
    pane_lines=$(tmux list-panes -a -F "$pane_format" 2>/dev/null || true)
    [[ -n "$pane_lines" ]] || return 0

    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue

        local session_name win_idx win_active win_flags pane_idx pane_title pane_dir pane_active pane_cmd pane_pid
        IFS="$d" read -r session_name win_idx win_active win_flags pane_idx pane_title pane_dir pane_active pane_cmd pane_pid <<< "$line"

        # Only save sessions matching our prefix
        [[ "$session_name" == "${prefix}@"* ]] || continue

        local full_command=""
        if [[ -n "$pane_pid" && "$pane_pid" != "0" ]]; then
            full_command=$(process_capture_full_command "$pane_pid" 2>/dev/null || echo "")
        fi

        if [[ "${WORKHIVE_DEBUG:-}" == "1" ]]; then
            local _safe_cmd
            _safe_cmd=$(process_sanitize_for_log "${full_command:-}")
            log_debug "Pane ${pane_idx} capture: cmd=${pane_cmd}, full=${_safe_cmd:-<empty>}" >&2
        fi

        local pane_role="" pane_kind="" pane_desired_command="" pane_state="" pane_last_error="" pane_updated_at=""
        pane_role=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_role 2>/dev/null || true)
        pane_kind=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_kind 2>/dev/null || true)
        pane_desired_command=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_desired_command 2>/dev/null || true)
        pane_state=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_state 2>/dev/null || true)
        pane_last_error=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_last_error 2>/dev/null || true)
        pane_updated_at=$(tmux show-options -pt "${session_name}:0.${pane_idx}" -v @workhive_pane_updated_at 2>/dev/null || true)

        snapshot_format_pane \
            "$session_name" "$win_idx" "$win_active" "$win_flags" \
            "$pane_idx" "$pane_title" "$pane_dir" "$pane_active" \
            "$pane_cmd" "$full_command" \
            "$pane_role" "$pane_kind" "$pane_desired_command" "$pane_state" "$pane_last_error" "$pane_updated_at"

    done <<< "$pane_lines"
}

# ──────────────────────────────────────────────────
# Dump all window records for matching sessions
# ──────────────────────────────────────────────────
_save_dump_windows() {
    local prefix="$1"
    local d=$'\t'

    local win_format
    win_format="#{session_name}${d}#{window_index}${d}#{window_name}${d}#{window_active}${d}#{window_flags}${d}#{window_layout}${d}#{window_active}"

    local win_lines
    win_lines=$(tmux list-windows -a -F "$win_format" 2>/dev/null || true)
    [[ -n "$win_lines" ]] || return 0

    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue

        local session_name win_idx win_name win_active win_flags win_layout auto_rename
        IFS="$d" read -r session_name win_idx win_name win_active win_flags win_layout auto_rename <<< "$line"
        [[ "$session_name" == "${prefix}@"* ]] || continue

        snapshot_format_window \
            "$session_name" "$win_idx" "$win_name" "$win_active" \
            "$win_flags" "$win_layout" "$auto_rename"

    done <<< "$win_lines"
}

# ──────────────────────────────────────────────────
# Dump session state (active/last session)
# ──────────────────────────────────────────────────
_save_dump_state() {
    local prefix="$1"

    local active_session=""
    active_session=$(tmux display-message -p '#{client_session}' 2>/dev/null || echo "")

    # Only record state if it belongs to our project
    if [[ -n "$active_session" && "$active_session" == "${prefix}@"* ]]; then
        local last_session
        last_session=$(tmux display-message -p '#{client_last_session}' 2>/dev/null || echo "")
        [[ "$last_session" != "${prefix}@"* ]] && last_session=""
        snapshot_format_state "$active_session" "$last_session"
    fi
}

# ──────────────────────────────────────────────────
# Collect dormant (non-running) records from previous snapshot
# Keeps only pane/window records for sessions not currently running
# ──────────────────────────────────────────────────
_save_collect_dormant_records() {
    local snapshot_file="$1"
    local running_sessions="$2"
    [[ -f "$snapshot_file" ]] || return 0

    local running_pipe
    running_pipe=$(printf '%s\n' "$running_sessions" | tr '\n' '|')

    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue

        local line_type
        line_type=$(snapshot_parse_json_type "$line")
        if [[ "$line_type" == "pane" || "$line_type" == "window" ]]; then
            local session_name
            session_name=$(snapshot_parse_json_string_field "$line" "session")
            if _save_session_is_running "$session_name" "$running_pipe"; then
                continue
            fi
            echo "$line"
        fi
    done < "$snapshot_file"
}

_save_session_is_running() {
    local session_name="$1"
    local running_pipe="$2"

    [[ -n "$session_name" ]] || return 1
    [[ "$running_pipe" == *"|${session_name}|"* ]] && return 0
    [[ "$running_pipe" == "${session_name}|"* ]] && return 0
    [[ "$running_pipe" == *"|${session_name}" ]] && return 0
    [[ "$running_pipe" == "$session_name" ]] && return 0
    return 1
}

# ──────────────────────────────────────────────────
# Save a single session by branch name
# Useful for pre-kill-session snapshots
# $1 = branch name
# ──────────────────────────────────────────────────
persistence_save_session() {
    local branch="$1"
    local reason="${2:-}"
    local context="${3:-}"
    [[ -z "$branch" ]] && return 1

    local session_name
    session_name=$(session_get_name "$branch" 2>/dev/null || echo "")
    [[ -z "$session_name" ]] && return 1

    if ! tmux has-session -t "$session_name" 2>/dev/null; then
        return 1
    fi

    # Full project save (captures the target session along with siblings)
    persistence_save "$reason" "$context"
}

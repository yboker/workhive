#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Persistence Restore
# Rebuilds tmux sessions/windows/panes from a snapshot file.
# Idempotent: skips already-existing sessions.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_RESTORE_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_RESTORE_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/persistence/snapshot.sh"
source "${LIB_DIR}/infra/persistence/process_capture.sh"

declare -a _RESTORE_PANE_LINES=()
declare -a _RESTORE_WINDOW_LINES=()
declare -a _RESTORE_CREATED_WINDOWS=()

readonly _RESTORE_DEFAULT_MAX_ATTEMPTS=3

_restore_max_attempts() {
    local raw="${WORKHIVE_RESTORE_MAX_ATTEMPTS:-$_RESTORE_DEFAULT_MAX_ATTEMPTS}"
    [[ "$raw" =~ ^[0-9]+$ ]] || raw="$_RESTORE_DEFAULT_MAX_ATTEMPTS"
    # 0 means unlimited attempts (diagnostics/troubleshooting mode).
    echo "$raw"
}

_restore_is_shell_cmd() {
    local cmd="${1:-}"
    case "$cmd" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh) return 0 ;;
        *) return 1 ;;
    esac
}

_restore_pane_fix_shell_prompt_marker() {
    local target_pane="$1"
    local pane_idx="$2"

    local last_line
    last_line=$(tmux capture-pane -p -t "$target_pane" -S -1 -E -1 2>/dev/null | tail -n1)
    last_line="${last_line//$'\r'/}"
    last_line="${last_line#"${last_line%%[![:space:]]*}"}"
    last_line="${last_line%"${last_line##*[![:space:]]}"}"
    if [[ "$last_line" != "%" ]]; then
        log_debug "Pane ${pane_idx} compatibility: skip prompt normalization (marker_not_detected)"
        return 0
    fi

    # Empty shell panes can show zsh EOL marker (e.g. "%") after restore.
    # Send a harmless empty Enter to normalize prompt rendering.
    tmux send-keys -t "$target_pane" C-m 2>/dev/null || true
    log_debug "Pane ${pane_idx} compatibility: sent empty Enter for shell prompt normalization"
}

# Try to remap a snapshot subpath under the new project root.
# Handles cases like /old/proj/src → /new/proj/src by matching on
# the project basename and rebuilding the relative portion.
# $1 = snapshot dir, $2 = fallback dir, $3 = fallback basename
# stdout: remapped path or empty if no match
_restore_remap_subpath() {
    local snap_dir="$1"
    local fallback_dir="$2"
    local fallback_basename="$3"

    local snap_remaining="$snap_dir"
    while [[ "$snap_remaining" == *"/$fallback_basename/"* ]]; do
        local subpath="${snap_remaining#*"/$fallback_basename/"}"
        local candidate="${fallback_dir}/${subpath}"
        if [[ -d "$candidate" ]]; then
            log_debug "CWD relocated (subpath): ${snap_dir} → ${candidate}"
            echo "$candidate"
            return 0
        fi
        snap_remaining="${snap_remaining#*"/$fallback_basename/"}"
    done

    return 1
}

# Resolve a pane directory from snapshot, handling directory relocation.
# When the project directory has been moved (e.g. /a/proj → /b/proj),
# snapshot CWDs still point to the old location. This function detects
# the mismatch and rewrites the path using the current project root.
# $1 = snapshot dir, $2 = fallback dir (current project root)
# stdout: resolved directory path
_restore_resolve_dir() {
    local snap_dir="$1"
    local fallback_dir="${2:-}"

    if [[ -z "$snap_dir" || "$snap_dir" != /* ]]; then
        echo "${fallback_dir:-$HOME}"
        return 0
    fi

    if [[ -d "$snap_dir" ]]; then
        echo "$snap_dir"
        return 0
    fi

    if [[ -n "$fallback_dir" && -d "$fallback_dir" ]]; then
        local fallback_basename="${fallback_dir##*/}"
        local snap_basename="${snap_dir##*/}"

        if [[ "$snap_basename" == "$fallback_basename" ]]; then
            log_debug "CWD relocated: ${snap_dir} → ${fallback_dir}"
            echo "$fallback_dir"
            return 0
        fi

        local remapped
        if remapped=$(_restore_remap_subpath "$snap_dir" "$fallback_dir" "$fallback_basename"); then
            echo "$remapped"
            return 0
        fi

        log_debug "CWD not found, using fallback: ${snap_dir} → ${fallback_dir}"
        echo "$fallback_dir"
        return 0
    fi

    echo "$HOME"
}

# ──────────────────────────────────────────────────
# Main restore entry point
# Restores all sessions from the latest snapshot
# Returns: 0 = restored, 1 = no snapshot, 2 = validation failed
# ──────────────────────────────────────────────────
persistence_restore() {
    local snapshot_file=""
    local instance_id
    instance_id=$(session_get_instance_id 2>/dev/null || echo "")

    while IFS= read -r snapshot_file; do
        [[ -z "$snapshot_file" || ! -f "$snapshot_file" ]] && continue
        snapshot_validate "$snapshot_file" "$instance_id" || continue
        snapshot_matches_generation "$snapshot_file" || continue
        break
    done < <(snapshot_list_restore_candidates)

    if [[ -z "$snapshot_file" || ! -f "$snapshot_file" ]]; then
        log_debug "No snapshot available for restore"
        return 1
    fi

    local _restore_runtime_server=""
    if declare -f tmux_runtime_server_name >/dev/null 2>&1; then
        _restore_runtime_server=$(tmux_runtime_server_name 2>/dev/null || echo "unknown")
    fi
    log_debug "restore: begin server=${_restore_runtime_server} snapshot=${snapshot_file}"
    log_info "$(_i18n PERSIST_RESTORING)"

    _restore_all_sessions "$snapshot_file"
    _restore_active_state "$snapshot_file"

    log_debug "restore: complete server=${_restore_runtime_server}"
    log_success "$(_i18n PERSIST_RESTORED)"
    return 0
}

# ──────────────────────────────────────────────────
# Restore a single session by branch name
# $1 = branch name
# Returns: 0 = restored, 1 = no data in snapshot, 2 = session already exists
# ──────────────────────────────────────────────────
persistence_restore_session() {
    local branch="$1"
    [[ -z "$branch" ]] && return 1

    local session_name
    session_name=$(session_get_name "$branch" 2>/dev/null || echo "")
    [[ -z "$session_name" ]] && return 1

    # Already exists — nothing to do (idempotent)
    if tmux has-session -t "$session_name" 2>/dev/null; then
        log_debug "Session already exists, skip restore: $session_name"
        return 2
    fi

    local instance_id
    instance_id=$(session_get_instance_id 2>/dev/null || echo "")

    local attempted=0
    local max_attempts
    max_attempts=$(_restore_max_attempts)
    local snapshot_file=""
    while IFS= read -r snapshot_file; do
        [[ -z "$snapshot_file" || ! -f "$snapshot_file" ]] && continue
        snapshot_validate "$snapshot_file" "$instance_id" || continue
        snapshot_matches_generation "$snapshot_file" || continue
        snapshot_has_session_exact "$snapshot_file" "$session_name" || continue

        local health_score
        health_score=$(snapshot_session_health_score "$snapshot_file" "$session_name" 2>/dev/null || echo "0")
        if ! snapshot_is_session_healthy "$snapshot_file" "$session_name"; then
            log_warn "Skipping low-quality snapshot for ${session_name}: score=${health_score}, file=$(basename "$snapshot_file")"
            continue
        fi

        attempted=$((attempted + 1))
        log_info "$(_i18n_fmt PERSIST_RESTORING_SESSION "$session_name") (score=${health_score}, file=$(basename "$snapshot_file"))"

        if _restore_session_from_snapshot "$snapshot_file" "$session_name"; then
            log_success "$(_i18n_fmt PERSIST_SESSION_RESTORED "$session_name")"
            return 0
        fi

        log_warn "Failed to restore session from snapshot: $session_name ($(basename "$snapshot_file"))"
        if [[ "$max_attempts" -gt 0 && "$attempted" -ge "$max_attempts" ]]; then
            log_warn "Snapshot restore attempt limit reached (${attempted}/${max_attempts}) for ${session_name}, fallback to fresh session creation"
            break
        fi
    done < <(snapshot_list_restore_candidates)

    if [[ "$attempted" -eq 0 ]]; then
        log_debug "No healthy snapshot candidates for session: $session_name"
    fi
    return 1
}

# ──────────────────────────────────────────────────
# Check if a session has restorable data in snapshot history
# $1 = branch name
# Returns: 0 = has healthy snapshot data, 1 = no healthy data
# ──────────────────────────────────────────────────
persistence_has_snapshot() {
    local branch="$1"
    [[ -z "$branch" ]] && return 1

    local session_name
    session_name=$(session_get_name "$branch" 2>/dev/null || echo "")
    [[ -z "$session_name" ]] && return 1

    local instance_id
    instance_id=$(session_get_instance_id 2>/dev/null || echo "")

    local snapshot_file
    while IFS= read -r snapshot_file; do
        [[ -z "$snapshot_file" || ! -f "$snapshot_file" ]] && continue
        snapshot_validate "$snapshot_file" "$instance_id" || continue
        snapshot_matches_generation "$snapshot_file" || continue
        snapshot_has_session_exact "$snapshot_file" "$session_name" || continue
        snapshot_is_session_healthy "$snapshot_file" "$session_name" || continue
        return 0
    done < <(snapshot_list_restore_candidates)

    return 1
}

# ──────────────────────────────────────────────────
# Internal: restore all sessions from a snapshot
# ──────────────────────────────────────────────────
_restore_all_sessions() {
    local file="$1"

    local session_name
    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue

        if tmux has-session -t "$session_name" 2>/dev/null; then
            log_debug "Session exists, skipping: $session_name"
            continue
        fi

        if ! _restore_session_from_snapshot "$file" "$session_name"; then
            log_warn "Failed to restore session: $session_name"
        fi
    done < <(snapshot_get_sessions "$file")
}

# ──────────────────────────────────────────────────
# Internal: restore a single session from snapshot data
# $1 = snapshot file, $2 = session name to restore
# $3 = fallback_dir (optional, current project root for CWD relocation)
# ──────────────────────────────────────────────────
# Collect and validate snapshot data for a session.
# Populates _RESTORE_PANE_LINES / _RESTORE_WINDOW_LINES.
# $1 = snapshot file, $2 = session name, $3 = fallback dir
_restore_load_and_validate() {
    local file="$1"
    local target_session="$2"
    local fallback_dir="$3"

    _RESTORE_PANE_LINES=()
    _RESTORE_WINDOW_LINES=()
    _RESTORE_CREATED_WINDOWS=()

    if ! _restore_collect_session_lines "$file" "$target_session"; then
        return 1
    fi
    if ! _restore_validate_pane_lines "$fallback_dir"; then
        log_warn "Invalid pane data in snapshot for session: $target_session"
        return 1
    fi
    if ! _restore_validate_window_lines; then
        log_warn "Invalid window data in snapshot for session: $target_session"
        return 1
    fi
    return 0
}

# Execute all restore phases for a validated session.
# Must be called after _restore_load_and_validate.
# $1 = session name, $2 = fallback dir
_restore_execute_phases() {
    local target_session="$1"
    local fallback_dir="$2"

    if ! _restore_phase_create_session "$target_session" "${_RESTORE_PANE_LINES[0]}" "$fallback_dir"; then
        return 1
    fi
    _RESTORE_CREATED_WINDOWS+=("0")

    _restore_phase_build_topology "$target_session" "$fallback_dir"
    _restore_phase_apply_window_layouts "$target_session"
    _restore_phase_restore_pane_runtime "$target_session" "$fallback_dir"
    _restore_phase_restore_active_panes "$target_session"
    _restore_phase_restore_active_window "$target_session"
    session_apply_config "$target_session" 2>/dev/null || true
    session_apply_deferred "$target_session" 2>/dev/null || true
    _restore_phase_restore_pane_titles "$target_session"
}

_restore_session_from_snapshot() {
    local file="$1"
    local target_session="$2"
    local fallback_dir="${3:-}"

    if [[ -z "$fallback_dir" ]]; then
        fallback_dir=$(_git_root 2>/dev/null || echo "")
    fi

    if ! _restore_load_and_validate "$file" "$target_session" "$fallback_dir"; then
        return 1
    fi

    if ! _restore_execute_phases "$target_session" "$fallback_dir"; then
        log_warn "Snapshot restore partially failed for: $target_session, cleaning up"
        tmux kill-session -t "$target_session" 2>/dev/null || true
        return 1
    fi
}

_restore_collect_session_lines() {
    local file="$1"
    local target_session="$2"

    _RESTORE_PANE_LINES=()
    _RESTORE_WINDOW_LINES=()

    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        snapshot_parse_pane_line "$line" || continue
        [[ "$_SP_SESSION" == "$target_session" ]] || continue
        _RESTORE_PANE_LINES+=("$line")
    done < <(snapshot_get_pane_lines "$file")

    [[ ${#_RESTORE_PANE_LINES[@]} -gt 0 ]] || return 1

    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        snapshot_parse_window_line "$line" || continue
        [[ "$_SW_SESSION" == "$target_session" ]] || continue
        _RESTORE_WINDOW_LINES+=("$line")
    done < <(snapshot_get_window_lines "$file")

    return 0
}

_restore_validate_pane_lines() {
    local fallback_dir="${1:-}"
    local line
    for line in "${_RESTORE_PANE_LINES[@]}"; do
        snapshot_parse_pane_line "$line" || return 1
        [[ "$_SP_WIN_IDX" =~ ^[0-9]+$ ]] || return 1
        [[ "$_SP_PANE_IDX" =~ ^[0-9]+$ ]] || return 1
        if [[ -n "$_SP_DIR" && "$_SP_DIR" != /* ]]; then
            log_debug "Pane ${_SP_PANE_IDX} has non-absolute dir: '${_SP_DIR}', will use fallback"
        fi
        if [[ -n "$_SP_DIR" && "$_SP_DIR" == /* && ! -d "$_SP_DIR" ]]; then
            local resolved
            resolved=$(_restore_resolve_dir "$_SP_DIR" "$fallback_dir")
            if [[ ! -d "$resolved" ]]; then
                log_debug "Pane ${_SP_PANE_IDX} dir not found: '${_SP_DIR}', will use fallback"
            else
                log_debug "Pane ${_SP_PANE_IDX} dir relocated: '${_SP_DIR}' → '${resolved}'"
            fi
        fi
    done

    return 0
}

_restore_validate_window_lines() {
    local line
    for line in "${_RESTORE_WINDOW_LINES[@]}"; do
        snapshot_parse_window_line "$line" || return 1
        [[ "$_SW_WIN_IDX" =~ ^[0-9]+$ ]] || return 1
    done

    return 0
}

_restore_phase_create_session() {
    local target_session="$1"
    local first_pane_line="$2"
    local fallback_dir="${3:-}"

    snapshot_parse_pane_line "$first_pane_line" || return 1

    local first_dir
    first_dir=$(_restore_resolve_dir "$_SP_DIR" "$fallback_dir")

    tmux new-session -d -s "$target_session" -c "$first_dir" 2>/dev/null || return 1
    return 0
}

_restore_phase_build_topology() {
    local target_session="$1"
    local fallback_dir="${2:-}"

    local i
    for ((i = 1; i < ${#_RESTORE_PANE_LINES[@]}; i++)); do
        snapshot_parse_pane_line "${_RESTORE_PANE_LINES[$i]}" || continue

        local win_idx="$_SP_WIN_IDX"
        local pane_dir
        pane_dir=$(_restore_resolve_dir "$_SP_DIR" "$fallback_dir")

        local win_exists=false
        local cw
        for cw in "${_RESTORE_CREATED_WINDOWS[@]}"; do
            [[ "$cw" == "$win_idx" ]] && win_exists=true && break
        done

        if [[ "$win_exists" == false ]]; then
            tmux new-window -d -t "${target_session}:${win_idx}" -c "$pane_dir" 2>/dev/null || true
            _RESTORE_CREATED_WINDOWS+=("$win_idx")
        else
            tmux split-window -t "${target_session}:${win_idx}" -c "$pane_dir" 2>/dev/null || true
            tmux resize-pane -t "${target_session}:${win_idx}" -U 999 2>/dev/null || true
        fi
    done
}

_restore_phase_apply_window_layouts() {
    local target_session="$1"

    local line
    for line in "${_RESTORE_WINDOW_LINES[@]}"; do
        snapshot_parse_window_line "$line" || continue

        local win_idx="$_SW_WIN_IDX"
        local layout="$_SW_LAYOUT"
        local win_name="$_SW_WIN_NAME"

        if [[ -n "$layout" ]]; then
            tmux select-layout -t "${target_session}:${win_idx}" "$layout" 2>/dev/null || true
        fi
        if [[ -n "$win_name" ]]; then
            tmux rename-window -t "${target_session}:${win_idx}" "$win_name" 2>/dev/null || true
        fi
    done
}

# Restore CWD for a single pane.
# $1 = tmux target pane, $2 = resolved directory
# Sets _RESTORE_CD_SENT="true" or "false" as side effect (avoids subshell).
_restore_pane_cwd() {
    local target_pane="$1"
    local pane_dir="$2"
    _RESTORE_CD_SENT="false"

    if [[ -n "$pane_dir" && -d "$pane_dir" ]]; then
        local current_pane_dir
        current_pane_dir=$(tmux display-message -p -t "$target_pane" '#{pane_current_path}' 2>/dev/null || echo "")
        if [[ "$current_pane_dir" != "$pane_dir" ]]; then
            tmux send-keys -t "$target_pane" "cd $(printf '%q' "$pane_dir")" C-m 2>/dev/null || true
            _RESTORE_CD_SENT="true"
        fi
    fi
}

# Execute a validated restore command in a pane.
# $1 = target pane, $2 = full command, $3 = pane dir, $4 = cd_sent flag, $5 = pane index (for logging)
# Returns: 0 if command was sent, 1 otherwise.
_restore_pane_exec_command() {
    local target_pane="$1"
    local full_cmd="$2"
    local pane_dir="$3"
    local cd_sent="$4"
    local pane_idx="$5"

    full_cmd=$(process_unwrap_restore_command "$full_cmd")
    [[ -z "$full_cmd" ]] && return 1

    local cmd_name
    cmd_name="${full_cmd%% *}"
    cmd_name=$(basename "$cmd_name" 2>/dev/null || echo "$cmd_name")

    if ! process_should_restore "$cmd_name" "$full_cmd"; then
        log_debug "Pane ${pane_idx} skip restore: '${cmd_name}' not in process list"
        return 1
    fi

    if ! process_validate_restore_command "$full_cmd"; then
        log_warn "Pane ${pane_idx} REJECTED unsafe command from snapshot: cmd_name=${cmd_name}"
        return 1
    fi

    local resolved_cmd
    resolved_cmd=$(process_resolve_restore_command "$full_cmd" "$pane_dir")
    [[ -z "$resolved_cmd" ]] && return 1

    local strategy
    strategy=$(process_classify_restore_strategy "$cmd_name" "$full_cmd")
    log_debug "Pane ${pane_idx} restoring: ${cmd_name} (strategy: ${strategy})"

    if [[ "$cd_sent" == "true" ]]; then
        local cd_delay="${WORKHIVE_RESTORE_CD_DELAY:-0.1}"
        if [[ ! "$cd_delay" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
            cd_delay="0.1"
        fi
        sleep "$cd_delay"
    fi

    # Prefer unified no-echo adapter strategy to avoid visible bootstrap commands.
    if declare -f terminal_adapter_spawn_command_target >/dev/null 2>&1; then
        if terminal_adapter_spawn_command_target "$target_pane" "$resolved_cmd" "$pane_dir"; then
            return 0
        fi
    fi

    # Fallback to legacy send-keys if respawn is not available/fails.
    if tmux send-keys -t "$target_pane" "$resolved_cmd" C-m 2>/dev/null; then
        return 0
    fi

    log_debug "Pane ${pane_idx} restore command failed (respawn/send-keys)"
    return 1
}

_restore_phase_restore_pane_runtime() {
    local target_session="$1"
    local fallback_dir="${2:-}"

    local restore_processes
    if declare -f persistence_cfg_restore_processes >/dev/null 2>&1; then
        restore_processes=$(persistence_cfg_restore_processes 2>/dev/null || echo "")
    elif declare -f _persistence_config_get >/dev/null 2>&1; then
        restore_processes=$(_persistence_config_get "restore_processes" "${_PERSIST_DEFAULT_RESTORE_PROCESSES:-}")
    else
        restore_processes="${_PERSIST_DEFAULT_RESTORE_PROCESSES:-}"
    fi
    log_debug "Restore processes config: ${restore_processes}"

    local line
    for line in "${_RESTORE_PANE_LINES[@]}"; do
        snapshot_parse_pane_line "$line" || continue
        local target_pane="${target_session}:${_SP_WIN_IDX}.${_SP_PANE_IDX}"
        local pane_dir
        pane_dir=$(_restore_resolve_dir "$_SP_DIR" "$fallback_dir")

        local full_cmd_restored=false
        if [[ "$restore_processes" == "true" ]]; then
            local restore_cmd=""
            if [[ -n "${_SP_PANE_DESIRED_CMD:-}" ]]; then
                restore_cmd="$_SP_PANE_DESIRED_CMD"
            elif [[ -n "$_SP_FULL_CMD" ]]; then
                restore_cmd="$_SP_FULL_CMD"
            fi
            if [[ -n "$restore_cmd" ]]; then
                if _restore_pane_exec_command "$target_pane" "$restore_cmd" "$pane_dir" "false" "$_SP_PANE_IDX"; then
                    full_cmd_restored=true
                fi
            fi
        fi

        local cd_sent="false"
        if [[ "$full_cmd_restored" != "true" ]]; then
            _restore_pane_cwd "$target_pane" "$pane_dir"
            cd_sent="$_RESTORE_CD_SENT"
        fi

        log_debug "Pane ${_SP_PANE_IDX} restore: resolved_cwd=${pane_dir}, cwd_resolved=true, cd_sent=${cd_sent}, full_cmd_restored=${full_cmd_restored}"

        if [[ "$restore_processes" != "true" ]]; then
            log_debug "Pane ${_SP_PANE_IDX} skip: restore_processes=${restore_processes}"
        elif [[ -z "$_SP_FULL_CMD" ]]; then
            log_debug "Pane ${_SP_PANE_IDX} skip: no full_cmd in snapshot"
        fi

        if [[ "$full_cmd_restored" != "true" && -z "$_SP_FULL_CMD" && "$cd_sent" != "true" ]] && _restore_is_shell_cmd "$_SP_PANE_CMD"; then
            _restore_pane_fix_shell_prompt_marker "$target_pane" "$_SP_PANE_IDX"
        fi

        log_debug "Pane ${_SP_PANE_IDX} restore result: full_cmd_restored=${full_cmd_restored}"

        _restore_pane_write_metadata "$target_session" "$_SP_PANE_IDX" "$full_cmd_restored"
    done
}

_restore_pane_write_metadata() {
    local target_session="$1"
    local pane_idx="$2"
    local full_cmd_restored="$3"

    if ! declare -f pane_meta_write_identity >/dev/null 2>&1; then
        return 0
    fi

    if [[ -n "${_SP_PANE_ROLE:-}" ]]; then
        local restore_kind="${_SP_PANE_KIND:-shell}"
        if [[ -n "${_SP_PANE_DESIRED_CMD:-}" ]] && declare -f _pane_meta_classify_kind >/dev/null 2>&1; then
            local reclassified_kind
            reclassified_kind=$(_pane_meta_classify_kind "${_SP_PANE_DESIRED_CMD}")
            if [[ "$reclassified_kind" != "$restore_kind" ]]; then
                log_debug "Pane ${pane_idx} kind corrected: snapshot=${restore_kind} reclassified=${reclassified_kind} desired_cmd=${_SP_PANE_DESIRED_CMD}"
                restore_kind="$reclassified_kind"
            fi
        fi
        pane_meta_write_identity "$target_session" "$pane_idx" \
            "$_SP_PANE_ROLE" "$restore_kind" "${_SP_PANE_DESIRED_CMD:-}"
        local restore_state="${_SP_PANE_STATE:-}"
        case "$restore_state" in
            pending|launching|running|failed|idle) ;;
            *)
                restore_state="idle"
                ;;
        esac
        if [[ "$restore_state" == "idle" && "$full_cmd_restored" == "true" ]]; then
            [[ "$restore_kind" == "agent" || "$restore_kind" == "tool" ]] && restore_state="running"
        fi
        pane_meta_transition_state "$target_session" "$pane_idx" "$restore_state" 2>/dev/null || true
        if [[ -n "${_SP_PANE_LAST_ERROR:-}" ]] && declare -f pane_meta_set >/dev/null 2>&1; then
            pane_meta_set "$target_session" "$pane_idx" "last_error" "$_SP_PANE_LAST_ERROR"
        fi
        if [[ -n "${_SP_PANE_UPDATED_AT:-}" ]] && declare -f pane_meta_set >/dev/null 2>&1; then
            pane_meta_set "$target_session" "$pane_idx" "updated_at" "$_SP_PANE_UPDATED_AT"
        fi
        log_debug "Pane ${pane_idx} metadata restored from snapshot: role=${_SP_PANE_ROLE} state=${restore_state}"
    else
        local role="pane_${pane_idx}"
        local classify_cmd="${_SP_PANE_DESIRED_CMD:-}"
        if [[ -z "$classify_cmd" ]]; then
            classify_cmd="${_SP_FULL_CMD:-${_SP_PANE_CMD:-}}"
            classify_cmd=$(process_unwrap_restore_command "$classify_cmd" 2>/dev/null || echo "$classify_cmd")
        fi
        local kind
        if declare -f _pane_meta_classify_kind >/dev/null 2>&1; then
            kind=$(_pane_meta_classify_kind "$classify_cmd")
        else
            kind="shell"
        fi
        local state="idle"
        [[ "$full_cmd_restored" == "true" && ("$kind" == "agent" || "$kind" == "tool") ]] && state="running"
        pane_meta_write_identity "$target_session" "$pane_idx" "$role" "$kind" "$classify_cmd"
        pane_meta_transition_state "$target_session" "$pane_idx" "$state" 2>/dev/null || true
        log_debug "Pane ${pane_idx} metadata backfilled (legacy snapshot): role=${role} kind=${kind} classify_cmd=${classify_cmd:-(empty)} state=${state}"
    fi
}

_restore_phase_restore_active_panes() {
    local target_session="$1"

    local line
    for line in "${_RESTORE_PANE_LINES[@]}"; do
        snapshot_parse_pane_line "$line" || continue
        if [[ "$_SP_PANE_ACTIVE" == "1" ]]; then
            tmux select-pane -t "${target_session}:${_SP_WIN_IDX}.${_SP_PANE_IDX}" 2>/dev/null || true
        fi
    done
}

_restore_phase_restore_active_window() {
    local target_session="$1"

    local line
    for line in "${_RESTORE_WINDOW_LINES[@]}"; do
        snapshot_parse_window_line "$line" || continue
        if [[ "$_SW_WIN_ACTIVE" == "1" ]]; then
            tmux select-window -t "${target_session}:${_SW_WIN_IDX}" 2>/dev/null || true
            break
        fi
    done
}

_restore_phase_restore_pane_titles() {
    local target_session="$1"

    local line
    for line in "${_RESTORE_PANE_LINES[@]}"; do
        snapshot_parse_pane_line "$line" || continue
        if [[ -n "$_SP_PANE_TITLE" ]]; then
            tmux select-pane -t "${target_session}:${_SP_WIN_IDX}.${_SP_PANE_IDX}" -T "$_SP_PANE_TITLE" 2>/dev/null || true
        fi
    done
}

# ──────────────────────────────────────────────────
# Restore active/alternate session state
# ──────────────────────────────────────────────────
_restore_active_state() {
    local file="$1"
    local state_line
    state_line=$(snapshot_get_state_line "$file")
    [[ -z "$state_line" ]] && return 0

    snapshot_parse_state_line "$state_line" || return 0
    local active_session="$_SS_ACTIVE_SESSION"

    if [[ -n "$active_session" ]] && tmux has-session -t "$active_session" 2>/dev/null; then
        if [[ -z "${TMUX:-}" ]]; then
            log_debug "restore active-state: outside tmux, skip switch-client for ${active_session}"
            return 0
        fi

        local strategy="cross-server"
        if declare -f _terminal_tmux_select_attach_strategy >/dev/null 2>&1; then
            strategy=$(_terminal_tmux_select_attach_strategy)
        fi

        if [[ "$strategy" == "same-server" ]]; then
            log_debug "restore active-state: same-server switch-client to ${active_session}"
            tmux switch-client -t "$active_session" 2>/dev/null || true
        else
            log_debug "restore active-state: skip switch-client (strategy=${strategy}), invalid cross-server client switching"
        fi
    fi
}

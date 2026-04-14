#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot I/O
# Handles snapshot file format: read, write, parse, diff
# JSONL records with project-scoped metadata and strict schema validation.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_SNAPSHOT_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_SNAPSHOT_SH_LOADED=1

readonly _SNAPSHOT_VERSION="3"
readonly _SNAPSHOT_ARCHIVE_PREFIX="wh_snapshot_archive_"
readonly _SNAPSHOT_LEGACY_ARCHIVE_PREFIX="wh_snapshot_"
readonly _SNAPSHOT_LATEST_BASENAME="wh_snapshot_latest"
readonly _SNAPSHOT_EXT=".jsonl"

# ──────────────────────────────────────────────────
# Snapshot directory resolution
# ──────────────────────────────────────────────────

# Per-project snapshot directory
# Returns: path to .workhive/snapshots/
snapshot_get_project_dir() {
    local wh_dir
    wh_dir=$(_workhive_dir)
    echo "${wh_dir}/snapshots"
}

# Global snapshot directory (organized by instance_id)
# Returns: path to ~/.local/share/workhive/snapshots/<instance_id>/
snapshot_get_global_dir() {
    local instance_id="${1:-}"
    local data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/workhive/snapshots"
    if [[ -n "$instance_id" ]]; then
        echo "${data_dir}/${instance_id}"
    else
        echo "$data_dir"
    fi
}

# Resolve the effective snapshot directory (always project-scoped)
snapshot_get_dir() {
    snapshot_get_project_dir
}

# ──────────────────────────────────────────────────
# Snapshot file naming and listing
# ──────────────────────────────────────────────────

# Generate a temporary snapshot file path for safe writing.
# Uses mktemp to prevent symlink clobber attacks. We intentionally avoid
# suffix templates because BSD mktemp can keep trailing Xs literally.
snapshot_new_file_path() {
    local snap_dir
    snap_dir=$(snapshot_get_dir)
    [[ -d "$snap_dir" ]] || (umask 077 && mkdir -p "$snap_dir")

    local tmp_file
    tmp_file=$(mktemp "${snap_dir}/.tmp.${_SNAPSHOT_ARCHIVE_PREFIX}XXXXXXXXXX" 2>/dev/null) || {
        log_error "$(_i18n_fmt SNAPSHOT_ERR_MKTEMP_FAILED_FMT "$snap_dir")"
        return 1
    }

    # Ensure strict permissions even on platforms/filesystems with permissive defaults.
    chmod 600 "$tmp_file" 2>/dev/null || true

    local ts suffix attempt renamed
    ts=$(date "+%Y%m%dT%H%M%S")
    suffix="${RANDOM}${RANDOM}"
    for attempt in 1 2 3; do
        renamed="${snap_dir}/${_SNAPSHOT_ARCHIVE_PREFIX}${ts}_${suffix}_${attempt}${_SNAPSHOT_EXT}"
        if mv -n -- "$tmp_file" "$renamed" 2>/dev/null; then
            chmod 600 "$renamed" 2>/dev/null || true
            echo "$renamed"
            return 0
        fi
    done

    cleanup_file_quiet "$tmp_file"
    log_error "$(_i18n_fmt SNAPSHOT_ERR_RENAME_TEMP_FAILED_FMT "${snap_dir}/${_SNAPSHOT_ARCHIVE_PREFIX}${ts}_${suffix}_*${_SNAPSHOT_EXT}")"
    return 1
}

# Get the single latest snapshot file
# Returns: full path to latest snapshot, or empty string if none
snapshot_get_latest() {
    local snap_dir
    snap_dir=$(snapshot_get_dir)

    local full_path_jsonl="${snap_dir}/${_SNAPSHOT_LATEST_BASENAME}${_SNAPSHOT_EXT}"
    if [[ -f "$full_path_jsonl" ]]; then
        echo "$full_path_jsonl"
        return 0
    fi

    echo ""
    return 1
}

# List snapshot files (latest + history)
snapshot_list_files() {
    snapshot_list_restore_candidates
}

# Count snapshot files
snapshot_count() {
    local count=0
    while IFS= read -r _f; do
        [[ -n "$_f" ]] || continue
        count=$((count + 1))
    done < <(snapshot_list_files)
    echo "$count"
}

# ──────────────────────────────────────────────────
# Snapshot header generation and parsing
# ──────────────────────────────────────────────────

# JSONL meta record
snapshot_write_header() {
    local instance_id="$1"
    local project_name="$2"
    local session_prefix="$3"
    local wh_version="$4"

    local tmux_version
    tmux_version=$(tmux -V 2>/dev/null | head -n1 || echo "unknown")
    local created
    created=$(date "+%Y-%m-%dT%H:%M:%S%z")

    cat <<EOF
# workhive snapshot v${_SNAPSHOT_VERSION}
# instance_id: ${instance_id}
# project: ${project_name}
# session_prefix: ${session_prefix}
# created: ${created}
# workhive_version: ${wh_version}
# tmux_version: ${tmux_version}
EOF
}

# Parse a header value from snapshot file
# $1 = file path, $2 = header key (e.g. "instance_id")
snapshot_parse_header() {
    local file="$1"
    local key="$2"
    grep "^# ${key}: " "$file" 2>/dev/null | head -n1 | sed "s/^# ${key}: //"
}

# Detect snapshot version from file content
# Returns: version number, empty if unknown
snapshot_detect_version() {
    local file="$1"
    [[ -f "$file" ]] || { echo ""; return 1; }

    local first_non_empty
    first_non_empty=$(grep -v '^[[:space:]]*$' "$file" 2>/dev/null | head -n1)

    if [[ "$(snapshot_parse_json_type "$first_non_empty")" == "meta" ]]; then
        local ver
        ver=$(snapshot_parse_json_string_field "$first_non_empty" "version")
        if [[ -n "$ver" ]]; then
            echo "$ver"
            return 0
        fi
    fi

    echo ""
    return 1
}

# Escape plain string into JSON string content (without wrapping quotes)
snapshot_escape_json_string() {
    local val="$1"
    val="${val//\\/\\\\}"
    val="${val//\"/\\\"}"
    val="${val//$'\t'/\\t}"
    val="${val//$'\n'/\\n}"
    val="${val//$'\r'/\\r}"
    printf '%s' "$val"
}

snapshot_parse_json_string_literal() {
    local value="$1"
    local bs_placeholder=$'\x1f'

    if [[ "$value" == 'null' ]]; then
        REPLY=""
        return 0
    fi

    [[ "$value" == '"'*'"' ]] || return 1
    value="${value:1:${#value}-2}"

    # Decode in safe order so "\\n" remains literal "\n" (not newline)
    value="${value//\\\\/${bs_placeholder}}"
    value="${value//\\\"/\"}"
    value="${value//\\r/$'\r'}"
    value="${value//\\n/$'\n'}"
    value="${value//\\t/$'\t'}"
    value="${value//${bs_placeholder}/\\}"

    REPLY="$value"
    return 0
}

_SNAPSHOT_YQ_READY=0

_snapshot_ensure_yq_json_parser() {
    [[ "${_SNAPSHOT_YQ_READY:-0}" == "1" ]] && return 0

    check_cmd yq || return 1

    # If config layer exposes compatibility checks, enforce yq v4+.
    if declare -F config_check_yq_version >/dev/null 2>&1; then
        config_check_yq_version >/dev/null 2>&1 || return 1
    fi

    _SNAPSHOT_YQ_READY=1
    return 0
}

# Parse selected fields from a single-line JSON object into shell variables.
# Usage: snapshot_parse_json_fields "$line" type field1 field2 ...
snapshot_parse_json_fields() {
    local line="$1"
    shift
    _snapshot_ensure_yq_json_parser || return 1

    local expected_count=$#
    local clear_index=0
    while [[ "$clear_index" -lt 32 ]]; do
        printf -v "_SNAP_JSON_FIELD_${clear_index}" '%s' ""
        clear_index=$((clear_index + 1))
    done

    local expr='['
    local key
    for key in "$@"; do
        [[ "$expr" != "[" ]] && expr+=', '
        expr+=".${key} // \"\""
    done
    expr+='] | .[]'

    local values
    values=$(printf '%s\n' "$line" | yq -p=json -o=json "$expr" 2>/dev/null) || return 1

    local index=0
    local raw_value
    while IFS= read -r raw_value; do
        snapshot_parse_json_string_literal "$raw_value" || return 1
        printf -v "_SNAP_JSON_FIELD_${index}" '%s' "$REPLY"
        index=$((index + 1))
    done <<< "$values"

    if [[ "$index" -ne "$expected_count" ]]; then
        return 1
    fi

    _SNAP_JSON_FIELD_COUNT="$expected_count"
    return 0
}

# Parse JSON string field from a single-line JSON object
# $1 = line, $2 = key
snapshot_parse_json_string_field() {
    local line="$1"
    local key="$2"
    if snapshot_parse_json_fields "$line" "$key"; then
        printf '%s' "${_SNAP_JSON_FIELD_0:-}"
        return 0
    fi
    printf '%s' ""
}

# Parse JSON object type from a single-line JSON object
snapshot_parse_json_type() {
    local line="$1"
    snapshot_parse_json_string_field "$line" "type"
}

# Format JSONL records
snapshot_format_meta() {
    local instance_id="$1"
    local project_name="$2"
    local session_prefix="$3"
    local wh_version="$4"
    local generation="${5:-0}"
    local created
    created=$(date "+%Y-%m-%dT%H:%M:%S%z")
    local tmux_server=""
    if declare -f tmux_runtime_server_name >/dev/null 2>&1; then
        tmux_server=$(tmux_runtime_server_name 2>/dev/null || echo "")
    fi
    printf '{"type":"meta","version":"%s","schema":"workhive.snapshot","instance_id":"%s","project":"%s","session_prefix":"%s","generation":"%s","workhive_version":"%s","tmux_server":"%s","created":"%s"}\n' \
        "$(snapshot_escape_json_string "$_SNAPSHOT_VERSION")" \
        "$(snapshot_escape_json_string "$instance_id")" \
        "$(snapshot_escape_json_string "$project_name")" \
        "$(snapshot_escape_json_string "$session_prefix")" \
        "$(snapshot_escape_json_string "$generation")" \
        "$(snapshot_escape_json_string "$wh_version")" \
        "$(snapshot_escape_json_string "$tmux_server")" \
        "$(snapshot_escape_json_string "$created")"
}

snapshot_format_window() {
    local session_name="$1"
    local window_index="$2"
    local window_name="$3"
    local window_active="$4"
    local window_flags="$5"
    local window_layout="$6"
    local automatic_rename="${7:-}"

    printf '{"type":"window","session":"%s","window_index":"%s","window_name":"%s","window_active":"%s","window_flags":"%s","window_layout":"%s","automatic_rename":"%s"}\n' \
        "$(snapshot_escape_json_string "$session_name")" \
        "$(snapshot_escape_json_string "$window_index")" \
        "$(snapshot_escape_json_string "$window_name")" \
        "$(snapshot_escape_json_string "$window_active")" \
        "$(snapshot_escape_json_string "$window_flags")" \
        "$(snapshot_escape_json_string "$window_layout")" \
        "$(snapshot_escape_json_string "$automatic_rename")"
}

snapshot_format_pane() {
    local session_name="$1"
    local window_index="$2"
    local window_active="$3"
    local window_flags="$4"
    local pane_index="$5"
    local pane_title="$6"
    local dir="$7"
    local pane_active="$8"
    local pane_command="$9"
    local full_command="${10:-}"
    local pane_role="${11:-}"
    local pane_kind="${12:-}"
    local pane_desired_command="${13:-}"
    local pane_state="${14:-}"
    local pane_last_error="${15:-}"
    local pane_updated_at="${16:-}"

    local base
    base=$(printf '{"type":"pane","session":"%s","window_index":"%s","window_active":"%s","window_flags":"%s","pane_index":"%s","pane_title":"%s","dir":"%s","pane_active":"%s","pane_command":"%s","full_command":"%s"' \
        "$(snapshot_escape_json_string "$session_name")" \
        "$(snapshot_escape_json_string "$window_index")" \
        "$(snapshot_escape_json_string "$window_active")" \
        "$(snapshot_escape_json_string "$window_flags")" \
        "$(snapshot_escape_json_string "$pane_index")" \
        "$(snapshot_escape_json_string "$pane_title")" \
        "$(snapshot_escape_json_string "$dir")" \
        "$(snapshot_escape_json_string "$pane_active")" \
        "$(snapshot_escape_json_string "$pane_command")" \
        "$(snapshot_escape_json_string "$full_command")")

    if [[ -n "$pane_role" ]]; then
        base="${base}"$(printf ',"pane_role":"%s","pane_kind":"%s","pane_desired_command":"%s","pane_state":"%s","pane_last_error":"%s","pane_updated_at":"%s"' \
            "$(snapshot_escape_json_string "$pane_role")" \
            "$(snapshot_escape_json_string "$pane_kind")" \
            "$(snapshot_escape_json_string "$pane_desired_command")" \
            "$(snapshot_escape_json_string "$pane_state")" \
            "$(snapshot_escape_json_string "$pane_last_error")" \
            "$(snapshot_escape_json_string "$pane_updated_at")")
    fi

    printf '%s}\n' "$base"
}

snapshot_format_state() {
    local active_session="$1"
    local last_session="${2:-}"

    printf '{"type":"state","active_session":"%s","last_session":"%s"}\n' \
        "$(snapshot_escape_json_string "$active_session")" \
        "$(snapshot_escape_json_string "$last_session")"
}

# Validate snapshot file: check schema and instance_id match
# $1 = file path, $2 = expected instance_id
# Returns: 0 = valid, 1 = invalid
snapshot_validate() {
    local file="$1"
    local expected_id="$2"

    if [[ ! -f "$file" ]]; then
        log_debug "Snapshot file not found: $file"
        return 1
    fi

    local snap_version
    snap_version=$(snapshot_detect_version "$file" 2>/dev/null || echo "")
    if [[ -z "$snap_version" ]]; then
        log_debug "Invalid snapshot format: missing version header"
        return 1
    fi

    if [[ "$snap_version" != "$_SNAPSHOT_VERSION" ]]; then
        log_debug "Unsupported snapshot version: $snap_version (expected ${_SNAPSHOT_VERSION})"
        return 1
    fi

    local snap_schema=""
    local meta_line=""
    meta_line=$(_snapshot_get_lines_by_type "$file" "meta" "first")
    snap_schema=$(snapshot_parse_json_string_field "$meta_line" "schema")
    if [[ "$snap_schema" != "workhive.snapshot" ]]; then
        log_debug "Snapshot schema mismatch: expected=workhive.snapshot, got=${snap_schema:-<empty>}"
        return 1
    fi

    if [[ -n "$expected_id" ]]; then
        local snap_id=""
        snap_id=$(snapshot_parse_json_string_field "$meta_line" "instance_id")
        if [[ "$snap_id" != "$expected_id" ]]; then
            log_debug "Snapshot instance_id mismatch: expected=$expected_id, got=$snap_id"
            return 1
        fi
    fi

    return 0
}

# ──────────────────────────────────────────────────
# Snapshot record parsing
# ──────────────────────────────────────────────────

_snapshot_get_lines_by_type() {
    local file="$1"
    local target_type="$2"
    local mode="${3:-all}" # all | first
    [[ -f "$file" ]] || return 1

    local line
    while IFS= read -r line; do
        [[ "$(snapshot_parse_json_type "$line")" == "$target_type" ]] || continue
        printf '%s\n' "$line"
        [[ "$mode" == "first" ]] && return 0
    done < "$file"
    return 0
}

# Parse pane lines from snapshot file
# $1 = file path
# Outputs: raw JSON lines (one per line)
snapshot_get_pane_lines() {
    local file="$1"
    _snapshot_get_lines_by_type "$file" "pane" "all"
}

# Parse window lines from snapshot file
snapshot_get_window_lines() {
    local file="$1"
    _snapshot_get_lines_by_type "$file" "window" "all"
}

# Parse state line from snapshot file
snapshot_get_state_line() {
    local file="$1"
    _snapshot_get_lines_by_type "$file" "state" "first"
}

# Extract unique session names from snapshot
_snapshot_get_pane_sessions() {
    local file="$1"
    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        snapshot_parse_pane_line "$line" || continue
        printf '%s\n' "$_SP_SESSION"
    done < <(snapshot_get_pane_lines "$file")
}

snapshot_get_sessions() {
    local file="$1"
    local sessions
    sessions=$(_snapshot_get_pane_sessions "$file")
    if [[ -n "$sessions" ]]; then
        printf '%s' "$sessions" | sed '/^$/d' | sort -u
    fi
    return 0
}

# Check whether snapshot contains an exact session name match
# $1 = file path, $2 = target session name
# Returns: 0 = exact match found, 1 = not found/invalid
snapshot_has_session_exact() {
    local file="$1"
    local target_session="$2"
    [[ -f "$file" && -n "$target_session" ]] || return 1

    _snapshot_load_session_pane_metrics "$file" "$target_session" || return 1
    [[ "${_SNAPSHOT_SESSION_PANE_COUNT:-0}" -gt 0 ]]
}

# Parse a single pane line into variables
# Usage: snapshot_parse_pane_line "$line" and then read _SP_* vars
_SP_SESSION="" _SP_WIN_IDX="" _SP_WIN_ACTIVE="" _SP_WIN_FLAGS=""
_SP_PANE_IDX="" _SP_PANE_TITLE="" _SP_DIR="" _SP_PANE_ACTIVE=""
_SP_PANE_CMD="" _SP_FULL_CMD=""
_SP_PANE_ROLE="" _SP_PANE_KIND="" _SP_PANE_DESIRED_CMD="" _SP_PANE_STATE=""
_SP_PANE_LAST_ERROR="" _SP_PANE_UPDATED_AT=""

snapshot_parse_pane_line() {
    local line="$1"
    [[ "$(snapshot_parse_json_type "$line")" == "pane" ]] || return 1

    _SP_PANE_ROLE="" _SP_PANE_KIND="" _SP_PANE_DESIRED_CMD="" _SP_PANE_STATE=""
    _SP_PANE_LAST_ERROR="" _SP_PANE_UPDATED_AT=""

    if [[ "$line" == *'"pane_role"'* ]]; then
        snapshot_parse_json_fields "$line" session window_index window_active window_flags pane_index pane_title dir pane_active pane_command full_command pane_role pane_kind pane_desired_command pane_state pane_last_error pane_updated_at || return 1
        _SP_PANE_ROLE="${_SNAP_JSON_FIELD_10:-}"
        _SP_PANE_KIND="${_SNAP_JSON_FIELD_11:-}"
        _SP_PANE_DESIRED_CMD="${_SNAP_JSON_FIELD_12:-}"
        _SP_PANE_STATE="${_SNAP_JSON_FIELD_13:-}"
        _SP_PANE_LAST_ERROR="${_SNAP_JSON_FIELD_14:-}"
        _SP_PANE_UPDATED_AT="${_SNAP_JSON_FIELD_15:-}"
    else
        snapshot_parse_json_fields "$line" session window_index window_active window_flags pane_index pane_title dir pane_active pane_command full_command || return 1
    fi

    _SP_SESSION="${_SNAP_JSON_FIELD_0:-}"
    _SP_WIN_IDX="${_SNAP_JSON_FIELD_1:-}"
    _SP_WIN_ACTIVE="${_SNAP_JSON_FIELD_2:-}"
    _SP_WIN_FLAGS="${_SNAP_JSON_FIELD_3:-}"
    _SP_PANE_IDX="${_SNAP_JSON_FIELD_4:-}"
    _SP_PANE_TITLE="${_SNAP_JSON_FIELD_5:-}"
    _SP_DIR="${_SNAP_JSON_FIELD_6:-}"
    _SP_PANE_ACTIVE="${_SNAP_JSON_FIELD_7:-}"
    _SP_PANE_CMD="${_SNAP_JSON_FIELD_8:-}"
    _SP_FULL_CMD="${_SNAP_JSON_FIELD_9:-}"
    return 0
}

# Parse a single window line into variables
_SW_SESSION="" _SW_WIN_IDX="" _SW_WIN_NAME="" _SW_WIN_ACTIVE=""
_SW_WIN_FLAGS="" _SW_LAYOUT="" _SW_AUTO_RENAME=""

snapshot_parse_window_line() {
    local line="$1"
    [[ "$(snapshot_parse_json_type "$line")" == "window" ]] || return 1
    snapshot_parse_json_fields "$line" session window_index window_name window_active window_flags window_layout automatic_rename || return 1
    _SW_SESSION="${_SNAP_JSON_FIELD_0:-}"
    _SW_WIN_IDX="${_SNAP_JSON_FIELD_1:-}"
    _SW_WIN_NAME="${_SNAP_JSON_FIELD_2:-}"
    _SW_WIN_ACTIVE="${_SNAP_JSON_FIELD_3:-}"
    _SW_WIN_FLAGS="${_SNAP_JSON_FIELD_4:-}"
    _SW_LAYOUT="${_SNAP_JSON_FIELD_5:-}"
    _SW_AUTO_RENAME="${_SNAP_JSON_FIELD_6:-}"
    return 0
}

_SS_ACTIVE_SESSION="" _SS_LAST_SESSION=""
snapshot_parse_state_line() {
    local line="$1"
    [[ "$(snapshot_parse_json_type "$line")" == "state" ]] || return 1
    snapshot_parse_json_fields "$line" active_session last_session || return 1
    _SS_ACTIVE_SESSION="${_SNAP_JSON_FIELD_0:-}"
    _SS_LAST_SESSION="${_SNAP_JSON_FIELD_1:-}"
    return 0
}

# ──────────────────────────────────────────────────
# Snapshot lifecycle: update last symlink, cleanup
# ──────────────────────────────────────────────────

_snapshot_is_shell_cmd() {
    local cmd="${1:-}"
    case "$cmd" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh) return 0 ;;
        *) return 1 ;;
    esac
}

_snapshot_collect_session_pane_metrics() {
    local file="$1"
    local target_session="$2"
    [[ -f "$file" && -n "$target_session" ]] || return 1

    local pane_count=0
    local full_count=0
    local active_proc_count=0
    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        snapshot_parse_pane_line "$line" || continue
        [[ "$_SP_SESSION" == "$target_session" ]] || continue

        pane_count=$((pane_count + 1))
        if [[ -n "$_SP_FULL_CMD" || -n "${_SP_PANE_DESIRED_CMD:-}" ]]; then
            full_count=$((full_count + 1))
        fi
        if [[ -n "$_SP_PANE_CMD" ]] && ! _snapshot_is_shell_cmd "$_SP_PANE_CMD"; then
            active_proc_count=$((active_proc_count + 1))
        fi
    done < <(snapshot_get_pane_lines "$file")

    printf '%s|%s|%s\n' "$pane_count" "$full_count" "$active_proc_count"
    return 0
}

_SNAPSHOT_SESSION_PANE_COUNT="0"
_SNAPSHOT_SESSION_FULL_COUNT="0"
_SNAPSHOT_SESSION_ACTIVE_PROC_COUNT="0"

_snapshot_load_session_pane_metrics() {
    local file="$1"
    local target_session="$2"
    local metrics
    metrics=$(_snapshot_collect_session_pane_metrics "$file" "$target_session") || return 1
    IFS='|' read -r _SNAPSHOT_SESSION_PANE_COUNT _SNAPSHOT_SESSION_FULL_COUNT _SNAPSHOT_SESSION_ACTIVE_PROC_COUNT <<< "$metrics"
    return 0
}

snapshot_session_health_score() {
    local file="$1"
    local target_session="$2"
    [[ -f "$file" && -n "$target_session" ]] || { echo "0"; return 1; }

    _snapshot_load_session_pane_metrics "$file" "$target_session" || {
        echo "0"
        return 1
    }

    if [[ "${_SNAPSHOT_SESSION_PANE_COUNT}" -eq 0 ]]; then
        echo "0"
        return 1
    fi

    local score=0
    score=$(( (_SNAPSHOT_SESSION_FULL_COUNT * 70 + _SNAPSHOT_SESSION_ACTIVE_PROC_COUNT * 30) / _SNAPSHOT_SESSION_PANE_COUNT ))
    (( score > 100 )) && score=100
    echo "$score"
    return 0
}

snapshot_is_session_healthy() {
    local file="$1"
    local target_session="$2"
    local score
    score=$(snapshot_session_health_score "$file" "$target_session" 2>/dev/null || echo "0")
    [[ "$score" =~ ^[0-9]+$ ]] || score=0
    (( score >= 40 ))
}

snapshot_list_history_files() {
    local snap_dir
    snap_dir=$(snapshot_get_dir)
    [[ -d "$snap_dir" ]] || return 0

    local f
    for f in \
        "${snap_dir}/${_SNAPSHOT_ARCHIVE_PREFIX}"*"${_SNAPSHOT_EXT}" \
        "${snap_dir}/${_SNAPSHOT_LEGACY_ARCHIVE_PREFIX}"[0-9]*"${_SNAPSHOT_EXT}"; do
        [[ -f "$f" ]] || continue
        printf '%s\n' "$f"
    done | sort -r
}

snapshot_list_restore_candidates() {
    local latest
    latest=$(snapshot_get_latest 2>/dev/null || echo "")
    local latest_target=""
    if [[ -n "$latest" && -f "$latest" ]]; then
        if [[ -L "$latest" ]]; then
            local link_name
            link_name=$(readlink "$latest" 2>/dev/null || echo "")
            [[ -n "$link_name" ]] && latest_target="$(dirname "$latest")/${link_name}"
        fi
        printf '%s\n' "$latest"
    fi
    local f
    while IFS= read -r f; do
        [[ -n "$f" ]] || continue
        if [[ -n "$latest_target" && "$f" == "$latest_target" ]]; then
            continue
        fi
        printf '%s\n' "$f"
    done < <(snapshot_list_history_files)
}

snapshot_current_generation() {
    local generation="0"
    if declare -f persistence_cfg_generation >/dev/null 2>&1; then
        generation=$(persistence_cfg_generation 2>/dev/null || echo "0")
    fi
    [[ "$generation" =~ ^[0-9]+$ ]] || generation=0
    printf '%s' "$generation"
}

snapshot_meta_generation() {
    local file="$1"
    [[ -f "$file" ]] || { echo ""; return 1; }
    local meta_line=""
    while IFS= read -r line; do
        [[ "$(snapshot_parse_json_type "$line")" == "meta" ]] || continue
        meta_line="$line"
        break
    done < "$file"
    [[ -n "$meta_line" ]] || { echo ""; return 1; }

    local generation
    generation=$(snapshot_parse_json_string_field "$meta_line" "generation")
    [[ -n "$generation" ]] && printf '%s' "$generation"
}

snapshot_matches_generation() {
    local file="$1"
    [[ -f "$file" ]] || return 1

    local current_generation meta_generation
    current_generation=$(snapshot_current_generation)
    [[ "$current_generation" =~ ^[0-9]+$ ]] || current_generation=0

    # Backward compatibility: generation=0 accepts legacy snapshots.
    if (( current_generation == 0 )); then
        return 0
    fi

    meta_generation=$(snapshot_meta_generation "$file" 2>/dev/null || echo "")
    [[ "$meta_generation" =~ ^[0-9]+$ ]] || return 1
    [[ "$meta_generation" == "$current_generation" ]]
}

# Overwrite the latest snapshot with the new one.
# Returns non-zero on failure so callers can handle it.
# $1 = new snapshot file path
snapshot_update_last() {
    local new_file="$1"
    local snap_dir
    snap_dir=$(dirname "$new_file")
    local stable_file="${snap_dir}/${_SNAPSHOT_LATEST_BASENAME}${_SNAPSHOT_EXT}"
    local archive_file
    archive_file="${snap_dir}/${_SNAPSHOT_ARCHIVE_PREFIX}$(date '+%Y%m%dT%H%M%S%z')${_SNAPSHOT_EXT}"

    if ! mv -- "$new_file" "$archive_file" 2>/dev/null; then
        log_warn "$(_i18n_fmt SNAPSHOT_WARN_FINALIZE_MOVE_FAILED_FMT "$new_file" "$archive_file")"
        return 1
    fi

    local rel_target
    rel_target=$(basename "$archive_file")
    if ! ln -sfn "$rel_target" "$stable_file" 2>/dev/null; then
        if ! cp -- "$archive_file" "$stable_file" 2>/dev/null; then
            log_warn "$(_i18n_fmt SNAPSHOT_WARN_FINALIZE_MOVE_FAILED_FMT "$archive_file" "$stable_file")"
            return 1
        fi
    fi

    return 0
}

_snapshot_hash_text() {
    local text="$1"
    if command -v shasum >/dev/null 2>&1; then
        printf '%s' "$text" | shasum -a 256 | awk '{print $1}'
        return 0
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s' "$text" | sha256sum | awk '{print $1}'
        return 0
    fi
    if command -v md5 >/dev/null 2>&1; then
        printf '%s' "$text" | md5 | awk '{print $NF}'
        return 0
    fi
    # Fallback: return raw text length signature when hashing tools are unavailable.
    # This is weaker but still deterministic in constrained environments.
    printf 'len:%s:%s' "${#text}" "$text"
}

snapshot_semantic_payload() {
    local file="$1"
    [[ -f "$file" ]] || return 1

    local payload=""
    local line
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        local t
        t=$(snapshot_parse_json_type "$line")
        case "$t" in
            pane|window|state)
                payload+="${line}"$'\n'
                ;;
            *)
                ;;
        esac
    done < "$file"

    printf '%s' "$payload" | sort
}

snapshot_content_hash() {
    local file="$1"
    local payload
    payload=$(snapshot_semantic_payload "$file" 2>/dev/null || echo "")
    _snapshot_hash_text "$payload"
}

# Check if new snapshot differs from last
# $1 = new file, $2 = last file
# Returns: 0 = different (should keep), 1 = identical (should discard)
snapshot_has_changed() {
    local new_file="$1"
    local last_file="$2"

    [[ ! -f "$last_file" ]] && return 0

    # Compare semantic payload hash only (pane/window/state), ignore volatile meta.
    local new_hash last_hash
    new_hash=$(snapshot_content_hash "$new_file" 2>/dev/null || echo "")
    last_hash=$(snapshot_content_hash "$last_file" 2>/dev/null || echo "")

    if [[ -n "$new_hash" && "$new_hash" == "$last_hash" ]]; then
        return 1
    fi
    return 0
}

# Clean up orphan temp snapshot files (from interrupted saves).
# Removes old archived snapshots, excluding the latest stable target.
snapshot_cleanup() {
    local snap_dir
    snap_dir=$(snapshot_get_dir)
    [[ -d "$snap_dir" ]] || return 0

    local stable_file="${snap_dir}/${_SNAPSHOT_LATEST_BASENAME}${_SNAPSHOT_EXT}"
    local limit
    limit=$(snapshot_history_limit)
    local max_bytes
    max_bytes=$(snapshot_max_size_bytes)
    [[ "$max_bytes" =~ ^[0-9]+$ ]] || max_bytes=0

    local keep_target=""
    if [[ -L "$stable_file" ]]; then
        local link_name
        link_name=$(readlink "$stable_file" 2>/dev/null || echo "")
        if [[ -n "$link_name" ]]; then
            keep_target="${snap_dir}/${link_name}"
        fi
    fi

    local keep_count=0
    local current_bytes=0
    if [[ -n "$keep_target" && -f "$keep_target" ]]; then
        # keep_target is part of archive history and must count toward the
        # configured history limit to avoid keeping limit+1 archive files.
        keep_count=1
        current_bytes=$(_snapshot_file_size_bytes "$keep_target")
        [[ "$current_bytes" =~ ^[0-9]+$ ]] || current_bytes=0
    fi

    local f
    while IFS= read -r f; do
        [[ -f "$f" ]] || continue
        if [[ -n "$keep_target" && "$f" == "$keep_target" ]]; then
            continue
        fi

        local fsize
        fsize=$(_snapshot_file_size_bytes "$f")
        [[ "$fsize" =~ ^[0-9]+$ ]] || fsize=0

        local over_count=false
        local over_size=false
        if (( keep_count >= limit )); then
            over_count=true
        fi
        if (( max_bytes > 0 && (current_bytes + fsize) > max_bytes )); then
            over_size=true
        fi

        if [[ "$over_count" == true || "$over_size" == true ]]; then
            log_debug "Removing old snapshot: $f (over_count=${over_count}, over_size=${over_size})"
            cleanup_file_quiet "$f"
            continue
        fi

        keep_count=$((keep_count + 1))
        current_bytes=$((current_bytes + fsize))
    done < <(snapshot_list_history_files)
}

# Delete all snapshot files for current project.
# Returns: 0 on success (even if already empty), 1 on any remove failure.
snapshot_purge_all() {
    local snap_dir
    snap_dir=$(snapshot_get_dir)
    [[ -d "$snap_dir" ]] || return 0

    local failed=0
    local f
    for f in \
        "${snap_dir}/${_SNAPSHOT_LATEST_BASENAME}${_SNAPSHOT_EXT}" \
        "${snap_dir}/${_SNAPSHOT_ARCHIVE_PREFIX}"*"${_SNAPSHOT_EXT}" \
        "${snap_dir}/${_SNAPSHOT_LEGACY_ARCHIVE_PREFIX}"*"${_SNAPSHOT_EXT}"; do
        [[ -f "$f" || -L "$f" ]] || continue
        rm -f -- "$f" 2>/dev/null || failed=1
    done

    return "$failed"
}
snapshot_history_limit() {
    local raw=""
    if declare -f persistence_cfg_snapshot_history_limit >/dev/null 2>&1; then
        raw=$(persistence_cfg_snapshot_history_limit 2>/dev/null || echo "")
    fi
    if [[ -z "$raw" && -n "${WORKHIVE_SNAPSHOT_HISTORY_LIMIT:-}" ]]; then
        raw="${WORKHIVE_SNAPSHOT_HISTORY_LIMIT}"
    fi
    [[ "$raw" =~ ^[0-9]+$ ]] || raw="8"
    (( raw < 1 )) && raw=1
    printf '%s' "$raw"
}

snapshot_max_size_mb() {
    local raw=""
    if declare -f persistence_cfg_snapshot_max_size_mb >/dev/null 2>&1; then
        raw=$(persistence_cfg_snapshot_max_size_mb 2>/dev/null || echo "")
    fi
    if [[ -z "$raw" && -n "${WORKHIVE_SNAPSHOT_MAX_SIZE_MB:-}" ]]; then
        raw="${WORKHIVE_SNAPSHOT_MAX_SIZE_MB}"
    fi
    [[ "$raw" =~ ^[0-9]+$ ]] || raw="8"
    (( raw < 0 )) && raw=0
    printf '%s' "$raw"
}

snapshot_max_size_bytes() {
    local mb
    mb=$(snapshot_max_size_mb)
    [[ "$mb" =~ ^[0-9]+$ ]] || mb=8
    if (( mb == 0 )); then
        echo "0"
        return 0
    fi
    echo $((mb * 1024 * 1024))
}

_snapshot_file_size_bytes() {
    local file="$1"
    [[ -f "$file" ]] || { echo "0"; return 0; }
    if [[ "$(uname)" == "Darwin" ]]; then
        stat -f '%z' "$file" 2>/dev/null || echo "0"
    else
        stat -c '%s' "$file" 2>/dev/null || echo "0"
    fi
}

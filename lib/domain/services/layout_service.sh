#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Layout Service (Domain Layer)
#
# Unified Column-based DSL engine — the ONLY layout representation.
# All layouts are defined as an array of columns, each with independent
# pane count, width, heights, commands, and roles.
#
# config.yaml format:
#   layout:
#     columns:
#       - width: 60
#         panes:
#           - height: 50
#             command: "claude"
#             role: "agent"
#           - height: 50
#             command: null
#       - width: 40
#         panes:
#           - height: 100
#             command: "lazygit"
#             role: "git"
#
# Pane Role System: Each pane can have an optional role for cross-pane
# addressing via `pane send --role <name>`.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_LAYOUT_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _LAYOUT_SERVICE_SH_LOADED=1

source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/infra/adapters/terminal/factory.sh"
source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/tools/registry.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/pane_state_service.sh"
source "${LIB_DIR}/domain/services/pane_startup_strategy_service.sh"

# ──────────────────────────────────────────────────
# Constants
# ──────────────────────────────────────────────────

# Give pane shells enough time to become input-ready after split/create.
# Too-short delays can leave startup commands visible but not executed in some environments.
readonly _LAYOUT_STABILIZE_DELAY=0.35
readonly _LAYOUT_CMD_APPLY_SETTLE_DELAY=0.35
readonly _LAYOUT_GEOMETRY_STABLE_MAX_TRIES=20
readonly _LAYOUT_GEOMETRY_STABLE_INTERVAL=0.1
readonly _LAYOUT_RENDER_READY_MAX_TRIES=25
readonly _LAYOUT_RENDER_READY_INTERVAL=0.1
_layout_command_bin() {
    local cmd="${1:-}"
    cmd="${cmd#"${cmd%%[![:space:]]*}"}"
    printf '%s' "${cmd%% *}"
}

_layout_is_ai_command() {
    local cmd="${1:-}"
    tools_registry_is_ai_command "$cmd"
}

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Column-based DSL: Internal Representation
#
# After _layout_normalize(), these globals describe the layout:
#   _LCOL_COUNT          — number of columns
#   _LCOL_WIDTHS[]       — width percentage for each column
#   _LCOL_PANE_COUNTS[]  — number of panes in each column
#   _LPANE_HEIGHTS[]     — flat array: height of each pane (global index)
#   _LPANE_COMMANDS[]    — flat array: command for each pane
#   _LPANE_ROLES[]       — flat array: role for each pane ("" if unset)
#   _LPANE_TOTAL         — total pane count
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

_LCOL_COUNT=0
_LCOL_WIDTHS=()
_LCOL_PANE_COUNTS=()
_LPANE_HEIGHTS=()
_LPANE_COMMANDS=()
_LPANE_ROLES=()
_LPANE_TOTAL=0
_LAYOUT_NORMALIZE_CACHE_KEY=""

# ──────────────────────────────────────────────────
# Resolve effective layout config source/file
# Priority: project → global → defaults
# Returns config file path when project/global layout exists; empty string means defaults
# ──────────────────────────────────────────────────
_layout_resolve_config_file() {
    local cfg_file
    if cfg_file=$(config_find_file_with_key "layout.columns" 2>/dev/null) && [[ -n "$cfg_file" ]]; then
        printf '%s\n' "$cfg_file"
        return 0
    fi

    printf '\n'
    return 0
}

_layout_config_query() {
    local query="$1"
    local cfg_file="${2:-}"
    local result

    if [[ -n "$cfg_file" ]]; then
        result=$(yq "$query" "$cfg_file" 2>&1) || {
            log_debug "layout config query failed: query='$query' cfg='$cfg_file' err='$result'"
            return 1
        }
    else
        result=$(config_get_defaults | yq "$query" - 2>&1) || {
            log_debug "layout defaults query failed: query='$query' err='$result'"
            return 1
        }
    fi

    printf '%s\n' "$result"
}

_layout_required_query() {
    local query="$1"
    local cfg_file="${2:-}"
    local result
    result=$(_layout_config_query "$query" "$cfg_file") || {
        log_error "Failed to parse layout config (query: $query)"
        return 1
    }
    printf '%s\n' "$result"
}

_layout_config_stamp() {
    local cfg_file="${1:-}"
    if [[ -z "$cfg_file" ]]; then
        printf '%s\n' "__defaults__"
        return 0
    fi

    local file_state=""
    file_state=$(stat -f '%m|%c|%i|%z' "$cfg_file" 2>/dev/null || stat -c '%Y|%Z|%i|%s' "$cfg_file" 2>/dev/null || echo "")
    printf '%s|%s\n' "$cfg_file" "$file_state"
}

_LAYOUT_TSV_0=""
_LAYOUT_TSV_1=""
_LAYOUT_TSV_2=""
_LAYOUT_TSV_3=""

_layout_parse_tsv4() {
    local row="${1:-}"
    local rest

    _LAYOUT_TSV_0="${row%%$'\t'*}"
    rest="${row#*$'\t'}"
    [[ "$rest" == "$row" ]] && return 1

    _LAYOUT_TSV_1="${rest%%$'\t'*}"
    rest="${rest#*$'\t'}"
    [[ "$rest" == "$_LAYOUT_TSV_1" ]] && return 1

    _LAYOUT_TSV_2="${rest%%$'\t'*}"
    if [[ "$rest" == *$'\t'* ]]; then
        _LAYOUT_TSV_3="${rest#*$'\t'}"
    else
        _LAYOUT_TSV_3=""
    fi

    return 0
}

_layout_parse_tsv3() {
    local row="${1:-}"
    local rest

    _LAYOUT_TSV_0="${row%%$'\t'*}"
    rest="${row#*$'\t'}"
    [[ "$rest" == "$row" ]] && return 1

    _LAYOUT_TSV_1="${rest%%$'\t'*}"
    if [[ "$rest" == *$'\t'* ]]; then
        _LAYOUT_TSV_2="${rest#*$'\t'}"
    else
        _LAYOUT_TSV_2=""
    fi

    return 0
}

# ──────────────────────────────────────────────────
# Parse layout.columns from effective config into internal arrays
# ──────────────────────────────────────────────────
_layout_normalize() {
    local _cfg_file
    _cfg_file=$(_layout_resolve_config_file) || return 1

    local cache_key
    cache_key=$(_layout_config_stamp "$_cfg_file")
    if [[ -n "$cache_key" && "$cache_key" == "${_LAYOUT_NORMALIZE_CACHE_KEY:-}" && "${_LPANE_TOTAL:-0}" -gt 0 ]]; then
        return 0
    fi

    local col_count
    col_count=$(_layout_required_query '.layout.columns // [] | length' "$_cfg_file") || return 1
    [[ "$col_count" =~ ^[0-9]+$ ]] || {
        log_error "Invalid layout column count: ${col_count}"
        return 1
    }
    ((col_count < 1)) && col_count=1

    local -a new_col_widths=()
    local -a new_col_pane_counts=()
    local -a new_pane_heights=()
    local -a new_pane_commands=()
    local -a new_pane_roles=()
    local -a parsed_col_widths=()
    local -a parsed_col_pane_counts=()
    local -a parsed_col_pane_rows=()
    local new_pane_total=0

    # Batch-read column metadata once to reduce yq process overhead.
    local column_summaries=""
    column_summaries=$(_layout_required_query \
        ".layout.columns // [] | to_entries[]? | [(.key | tostring), ((.value.width // $((100 / col_count))) | tostring), (((.value.panes // []) | length) | tostring)] | @tsv" \
        "$_cfg_file") || return 1

    local summary_row
    while IFS= read -r summary_row; do
        [[ -z "$summary_row" ]] && continue
        local _summary_col_idx _summary_width _summary_panes
        IFS=$'\t' read -r _summary_col_idx _summary_width _summary_panes <<< "$summary_row"
        [[ "$_summary_col_idx" =~ ^[0-9]+$ ]] || continue
        parsed_col_widths["$_summary_col_idx"]="$_summary_width"
        parsed_col_pane_counts["$_summary_col_idx"]="$_summary_panes"
    done <<< "$column_summaries"

    # Batch-read all pane rows once to avoid one yq call per column.
    local pane_summaries=""
    pane_summaries=$(_layout_required_query \
        ".layout.columns // [] | to_entries[]? | .key as \$ci | (.value.panes // []) | .[]? | [(\$ci | tostring), (.height | tostring), ((.command // \"\") | tostring), ((.role // \"\") | tostring)] | @tsv" \
        "$_cfg_file") || return 1

    local pane_summary_row
    while IFS= read -r pane_summary_row; do
        [[ -z "$pane_summary_row" ]] && continue
        _layout_parse_tsv4 "$pane_summary_row" || continue
        local _pane_col_idx _pane_height _pane_cmd _pane_role
        _pane_col_idx="$_LAYOUT_TSV_0"
        _pane_height="$_LAYOUT_TSV_1"
        _pane_cmd="$_LAYOUT_TSV_2"
        _pane_role="$_LAYOUT_TSV_3"
        [[ "$_pane_col_idx" =~ ^[0-9]+$ ]] || continue
        parsed_col_pane_rows["$_pane_col_idx"]+="${_pane_height}"$'\t'"${_pane_cmd}"$'\t'"${_pane_role}"$'\n'
    done <<< "$pane_summaries"

    local c
    for ((c=0; c<col_count; c++)); do
        local col_width pane_count
        col_width="${parsed_col_widths[$c]:-$((100 / col_count))}"
        pane_count="${parsed_col_pane_counts[$c]:-0}"
        [[ "$col_width" =~ ^[0-9]+$ ]] || col_width=$((100 / col_count))
        new_col_widths+=("$col_width")

        [[ "$pane_count" =~ ^[0-9]+$ ]] || pane_count=0
        ((pane_count < 1)) && pane_count=1
        new_col_pane_counts+=("$pane_count")

        local pane_rows="${parsed_col_pane_rows[$c]:-}"

        local row_count=0
        local row
        while IFS= read -r row; do
            [[ -z "$row" ]] && continue
            local p_height p_cmd p_role
            _layout_parse_tsv3 "$row" || continue
            p_height="$_LAYOUT_TSV_0"
            p_cmd="$_LAYOUT_TSV_1"
            p_role="$_LAYOUT_TSV_2"
            [[ "$p_height" =~ ^[0-9]+$ ]] || p_height=$((100 / pane_count))
            [[ "$p_cmd" == "null" ]] && p_cmd=""
            [[ "$p_role" == "null" ]] && p_role=""
            new_pane_heights+=("$p_height")
            new_pane_commands+=("$p_cmd")
            new_pane_roles+=("${p_role:-}")
            new_pane_total=$((new_pane_total + 1))
            row_count=$((row_count + 1))
        done <<< "$pane_rows"

        # Keep behavior deterministic when panes are omitted in config.
        while ((row_count < pane_count)); do
            new_pane_heights+=("$((100 / pane_count))")
            new_pane_commands+=("")
            new_pane_roles+=("")
            new_pane_total=$((new_pane_total + 1))
            row_count=$((row_count + 1))
        done
    done

    _LCOL_COUNT=$col_count
    _LCOL_WIDTHS=("${new_col_widths[@]}")
    _LCOL_PANE_COUNTS=("${new_col_pane_counts[@]}")
    _LPANE_HEIGHTS=("${new_pane_heights[@]}")
    _LPANE_COMMANDS=("${new_pane_commands[@]}")
    _LPANE_ROLES=("${new_pane_roles[@]}")
    _LPANE_TOTAL=$new_pane_total
    _LAYOUT_NORMALIZE_CACHE_KEY="$cache_key"
}

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Pane Role Mapping
# Builds PANE_ROLE_TO_INDEX from normalized data
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

declare -A PANE_ROLE_TO_INDEX

_layout_emit_pane_index_keys() {
    _layout_normalize || return 1
    local idx=0 c p
    for ((c=0; c<_LCOL_COUNT; c++)); do
        local pc=${_LCOL_PANE_COUNTS[$c]}
        for ((p=0; p<pc; p++)); do
            local col_key="col_${c}_pane_${p}"
            local role_key="${_LPANE_ROLES[$idx]:-$col_key}"
            _is_null_or_empty "$role_key" && role_key="$col_key"
            printf '%s|%s|%s\n' "$idx" "$col_key" "$role_key"
            idx=$((idx + 1))
        done
    done
}

_pane_mapping_init() {
    PANE_ROLE_TO_INDEX=()
    _layout_normalize || return 1

    local idx col_key role_key
    while IFS='|' read -r idx col_key role_key; do
        [[ -z "${idx:-}" || -z "${col_key:-}" ]] && continue
        PANE_ROLE_TO_INDEX["$col_key"]="$idx"
        [[ -n "${role_key:-}" ]] && PANE_ROLE_TO_INDEX["$role_key"]="$idx"
    done < <(_layout_emit_pane_index_keys)
    PANE_ROLE_TO_INDEX[main]=0
    return 0
}

_pane_get_index() {
    local role="$1"
    if [[ -z "${PANE_ROLE_TO_INDEX[$role]+_}" ]]; then
        log_error "$(_i18n ERR_INVALID_PANE_ROLE) $role"
        echo "-1"
        return 1
    fi
    echo "${PANE_ROLE_TO_INDEX[$role]}"
}

_pane_get_main_index() {
    echo "${PANE_ROLE_TO_INDEX[main]}"
}

layout_resolve_role_index() {
    local role="$1"
    _pane_mapping_init

    if [[ -n "${PANE_ROLE_TO_INDEX[$role]+_}" ]]; then
        echo "${PANE_ROLE_TO_INDEX[$role]}"
        return 0
    fi
    return 1
}

layout_list_roles() {
    _pane_mapping_init

    local key
    for key in "${!PANE_ROLE_TO_INDEX[@]}"; do
        echo "${key}|${PANE_ROLE_TO_INDEX[$key]}"
    done
}

# ──────────────────────────────────────────────────
# Pane command collection (from normalized arrays)
# ──────────────────────────────────────────────────

layout_collect_pane_commands() {
    local idx _col_key role_key
    while IFS='|' read -r idx _col_key role_key; do
        [[ -z "${idx:-}" || -z "${role_key:-}" ]] && continue
        echo "${role_key}|${_LPANE_COMMANDS[$idx]}"
    done < <(_layout_emit_pane_index_keys)
}

layout_collect_runtime_pane_commands() {
    local branch="$1"
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    if declare -f pane_meta_has_metadata >/dev/null 2>&1 && \
       pane_meta_has_metadata "$session_name" "0" 2>/dev/null; then
        _layout_collect_runtime_from_metadata "$session_name"
        return $?
    fi

    _layout_collect_runtime_from_tmux "$session_name"
}

_layout_collect_runtime_from_metadata() {
    local session_name="$1"

    local pane_indices
    pane_indices=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}' 2>/dev/null || true)
    [[ -z "$pane_indices" ]] && return 1

    local emitted=0 _idx
    while IFS= read -r _idx; do
        [[ -z "${_idx:-}" || ! "$_idx" =~ ^[0-9]+$ ]] && continue
        local meta_all
        meta_all=$(pane_meta_read_all "$session_name" "$_idx" 2>/dev/null || true)
        local role="" desired_cmd=""
        if [[ -n "$meta_all" ]]; then
            IFS='|' read -r role _ desired_cmd _ _ _ <<< "$meta_all"
        fi
        role="${role:-pane_${_idx}}"
        echo "${role}|${desired_cmd:-}"
        emitted=$((emitted + 1))
    done <<< "$pane_indices"

    [[ $emitted -gt 0 ]]
}

_layout_collect_runtime_from_tmux() {
    local session_name="$1"

    # Diagnostic fallback: uses pane_current_command (NOT the source of truth).
    local runtime_lines
    runtime_lines=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}|#{pane_current_command}' 2>/dev/null || true)
    [[ -z "$runtime_lines" ]] && return 1

    local -A role_by_index=()
    local idx _col_key role_key
    while IFS='|' read -r idx _col_key role_key; do
        [[ -z "${idx:-}" || -z "${role_key:-}" ]] && continue
        role_by_index["$idx"]="$role_key"
    done < <(_layout_emit_pane_index_keys)

    local _idx _cmd _role
    local emitted=0
    while IFS='|' read -r _idx _cmd; do
        [[ -z "${_idx:-}" ]] && continue
        [[ "$_idx" =~ ^[0-9]+$ ]] || continue

        _role="${role_by_index[$_idx]:-pane_${_idx}}"
        echo "${_role}|${_cmd}"
        emitted=$((emitted + 1))
    done <<< "$runtime_lines"

    [[ $emitted -gt 0 ]]
}

layout_pane_role_label() {
    local role="$1"
    case "$role" in
        main)  _i18n PANE_L_MAIN ;;
        col_*_pane_*)
            local _col _pane
            _col="${role#col_}"; _col="${_col%%_pane_*}"
            _pane="${role##*pane_}"
            _i18n_fmt COLUMNS_PANE_LABEL "$((_col+1))" "$((_pane+1))"
            ;;
        *)  echo "$role" ;;
    esac
}

_layout_expected_pane_count() {
    _layout_normalize
    echo "$_LPANE_TOTAL"
}

_layout_fast_pane_count() {
    # Fast path: reuse normalized cache when available.
    if [[ "${_LPANE_TOTAL:-0}" -gt 0 ]]; then
        echo "$_LPANE_TOTAL"
        return 0
    fi

    local cfg_file fast_count
    cfg_file=$(_layout_resolve_config_file) || return 1
    if [[ -n "$cfg_file" ]]; then
        fast_count=$(_layout_required_query '.layout.columns // [] | map((((.panes // []) | length) | if . < 1 then 1 else . end)) | add // 0' "$cfg_file" 2>/dev/null) || true
        if [[ -n "$fast_count" && "$fast_count" =~ ^[0-9]+$ && "$fast_count" -gt 0 ]]; then
            echo "$fast_count"
            return 0
        fi
    fi

    # Fallback: compute through normalized layout path, else use default (3 = 2-column standard layout)
    local full_count
    full_count=$(_layout_expected_pane_count 2>/dev/null) || true
    if [[ -n "$full_count" && "$full_count" -gt 0 ]] 2>/dev/null; then
        echo "$full_count"
    else
        echo "3"
    fi
}

# ──────────────────────────────────────────────────
# Layout creation (unified column-based engine)
# ──────────────────────────────────────────────────

layout_create() {
    local branch="$1"
    local workdir="$2"
    local pane_workdir="${3:-$workdir}"

    if [[ -z "$branch" || -z "$workdir" ]]; then
        log_error "$(_i18n ERR_BRANCH_DIR_EMPTY)"
        return 1
    fi

    local session_name
    session_name=$(session_get_name "$branch")

    log_info "$(_i18n INFO_CREATING_LAYOUT) $session_name"

    _pane_mapping_init || {
        log_error "Failed to initialize pane mapping from layout config"
        return 1
    }

    _layout_create_from_columns "$session_name" "$pane_workdir" || return 1

    _layout_write_pane_metadata "$session_name"

    return 0
}

_layout_write_pane_metadata() {
    local session_name="$1"
    local idx=0 c p
    for ((c=0; c<_LCOL_COUNT; c++)); do
        local pc=${_LCOL_PANE_COUNTS[$c]}
        for ((p=0; p<pc; p++)); do
            local col_key="col_${c}_pane_${p}"
            local role_key="${_LPANE_ROLES[$idx]:-$col_key}"
            _is_null_or_empty "$role_key" && role_key="$col_key"
            local cmd="${_LPANE_COMMANDS[$idx]:-}"
            _is_null_or_empty "$cmd" && cmd=""
            local kind
            kind=$(_pane_meta_classify_kind "$cmd")
            pane_meta_write_identity "$session_name" "$idx" "$role_key" "$kind" "$cmd"
            log_debug "pane metadata written: idx=$idx role=$role_key kind=$kind cmd=${cmd:-(empty)}"
            idx=$((idx + 1))
        done
    done
}

# tmux pane creation strategy:
# 1. Create columns via vertical splits (each split creates a new rightmost pane)
# 2. Create rows within each column via horizontal splits
_layout_create_from_columns() {
    local session_name="$1"
    local pane_workdir="${2:-}"

    if ((_LCOL_COUNT < 1)); then
        log_error "$(_i18n ERR_UNKNOWN_LAYOUT) (empty columns)"
        return 1
    fi

    local c
    for ((c=1; c<_LCOL_COUNT; c++)); do
        local total_remaining_width=0
        local j
        for ((j=c; j<_LCOL_COUNT; j++)); do
            total_remaining_width=$((total_remaining_width + ${_LCOL_WIDTHS[$j]}))
        done
        local pane_current_width=0
        for ((j=c-1; j<_LCOL_COUNT; j++)); do
            pane_current_width=$((pane_current_width + ${_LCOL_WIDTHS[$j]}))
        done
        local pct=50
        if ((pane_current_width > 0)); then
            pct=$((total_remaining_width * 100 / pane_current_width))
        fi
        ((pct < 1)) && pct=1
        ((pct > 99)) && pct=99

        local target=$((c - 1))
        log_debug "Column split: target=$target, percentage=$pct"
        if ! terminal_adapter_split_vertical "$session_name" "$target" "$pct" "$pane_workdir"; then
            log_error "$(_i18n ERR_SPLIT_V_FAILED)"
            return 1
        fi
    done

    local pane_offset=0
    for ((c=0; c<_LCOL_COUNT; c++)); do
        local pc=${_LCOL_PANE_COUNTS[$c]}
        local col_base_pane=$((c + pane_offset))

        local p
        for ((p=1; p<pc; p++)); do
            local target=$((col_base_pane + p - 1))
            local remaining=$((pc - p))
            local pct=$((100 * remaining / (remaining + 1)))
            ((pct < 1)) && pct=1
            ((pct > 99)) && pct=99

            log_debug "Row split: col=$c, target=$target, percentage=$pct"
            if ! terminal_adapter_split_horizontal "$session_name" "$target" "$pct" "$pane_workdir"; then
                log_error "$(_i18n ERR_SPLIT_H_FAILED)"
                return 1
            fi
        done

        pane_offset=$((pane_offset + pc - 1))
    done

    return 0
}

# ──────────────────────────────────────────────────
# Layout command application
# ──────────────────────────────────────────────────

layout_apply_commands() {
    local branch="$1"
    local target_dir="${2:-}"
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    log_info "$(_i18n INFO_APPLYING_LAYOUT_CMDS)"

    sleep "$_LAYOUT_CMD_APPLY_SETTLE_DELAY"
    _layout_wait_for_geometry_stable "$session_name"

    _pane_mapping_init

    _apply_pane_cmd() {
        local p_role="$1"
        local p_cmd="$2"
        local p_dir="$3"

        local p_idx
        p_idx=$(_pane_get_index "$p_role")
        pane_startup_apply "$branch" "$session_name" "$p_role" "$p_idx" "$p_cmd" "$p_dir"
    }

    local _role _cmd
    local apply_failures=0
    local failed_roles=""
    while IFS='|' read -r _role _cmd; do
        [[ -z "$_role" ]] && continue
        if ! _apply_pane_cmd "$_role" "$_cmd" "$target_dir"; then
            apply_failures=$((apply_failures + 1))
            failed_roles="${failed_roles:+${failed_roles}, }${_role}"
        fi
    done < <(layout_collect_pane_commands)

    if ((apply_failures > 0)); then
        log_error "layout_apply_commands: ${apply_failures} pane(s) failed: [${failed_roles}]"
        return 1
    fi

    if declare -f terminal_adapter_reflow_session_panes >/dev/null 2>&1; then
        terminal_adapter_reflow_session_panes "$session_name" >/dev/null 2>&1 || true
    fi
    _layout_wait_for_render_ready "$session_name"

    log_success "$(_i18n INFO_LAYOUT_CMDS_APPLIED)"
    return 0
}

_layout_capture_geometry_signature() {
    local session_name="$1"
    terminal_adapter_list_panes_format "$session_name" '#{pane_index}|#{pane_width}|#{pane_height}' 2>/dev/null | sort || true
}

_layout_wait_for_geometry_stable() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local prev=""
    local curr=""
    local i=0
    for ((i=0; i<_LAYOUT_GEOMETRY_STABLE_MAX_TRIES; i++)); do
        curr=$(_layout_capture_geometry_signature "$session_name")
        if [[ -n "$curr" && "$curr" == "$prev" ]]; then
            log_debug "layout geometry stable: session=${session_name} attempts=$((i+1))"
            return 0
        fi
        prev="$curr"
        sleep "$_LAYOUT_GEOMETRY_STABLE_INTERVAL"
    done

    log_debug "layout geometry wait timeout: session=${session_name}"
    return 0
}

_layout_should_wait_for_pane_render() {
    local desired_command="${1:-}"
    local state="${2:-}"

    [[ -n "$desired_command" ]] || return 1
    [[ "$state" == "failed" ]] && return 1
    return 0
}

_layout_collect_pending_render_panes() {
    local session_name="$1"
    local line pane_index cursor_x cursor_y pane_title
    local pending=""

    while IFS=$'\t' read -r pane_index cursor_x cursor_y pane_title; do
        [[ -n "$pane_index" ]] || continue

        local meta role kind desired_command state last_error updated_at
        meta=$(pane_meta_read_all "$session_name" "$pane_index" 2>/dev/null || true)
        IFS='|' read -r role kind desired_command state last_error updated_at <<< "$meta"

        if ! _layout_should_wait_for_pane_render "$desired_command" "$state"; then
            continue
        fi

        [[ "$cursor_x" =~ ^[0-9]+$ ]] || cursor_x=0
        [[ "$cursor_y" =~ ^[0-9]+$ ]] || cursor_y=0

        if (( cursor_x == 0 && cursor_y == 0 )); then
            pending+="${pending:+;}${pane_index}:${role:-pane_${pane_index}}:${desired_command%% *}:${cursor_x},${cursor_y}:${pane_title:-<untitled>}"
        fi
    done < <(terminal_adapter_list_panes_format "$session_name" '#{pane_index}	#{cursor_x}	#{cursor_y}	#{pane_title}' 2>/dev/null || true)

    printf '%s\n' "$pending"
}

_layout_wait_for_render_ready() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local pending=""
    local attempt=0
    for ((attempt=1; attempt<=_LAYOUT_RENDER_READY_MAX_TRIES; attempt++)); do
        pending=$(_layout_collect_pending_render_panes "$session_name")
        if [[ -z "$pending" ]]; then
            _op_log "INFO" "pane_render_ready" "ok" "$session_name" "attempt=${attempt}"
            log_debug "layout render ready: session=${session_name} attempts=${attempt}"
            return 0
        fi
        if (( attempt == 5 || attempt == 10 || attempt == 15 || attempt == 20 )); then
            _op_log "INFO" "pane_render_ready" "waiting" "$session_name" "attempt=${attempt} pending=${pending}"
            log_debug "layout render waiting: session=${session_name} attempt=${attempt} pending=${pending}"
        fi
        sleep "$_LAYOUT_RENDER_READY_INTERVAL"
    done

    _op_log "WARN" "pane_render_ready" "timeout" "$session_name" "pending=${pending}"
    log_warn "layout render wait timeout: session=${session_name} pending=${pending}"
    return 0
}

# ──────────────────────────────────────────────────
# Workspace orchestration
# ──────────────────────────────────────────────────

layout_create_workspace() {
    local branch="$1"
    local workdir="$2"

    if ! session_create "$branch" "$workdir"; then
        log_error "$(_i18n ERR_SESSION_CREATE_FAILED)"
        return 1
    fi

    local session_name
    session_name=$(session_get_name "$branch")

    if ! layout_create "$branch" "$workdir"; then
        log_error "$(_i18n ERR_LAYOUT_CREATE_FAILED)"
        session_kill "$branch"
        return 1
    fi

    sleep "$_LAYOUT_STABILIZE_DELAY"

    if ! layout_apply_commands "$branch" "$workdir"; then
        log_warn "$(_i18n WARN_CMD_CONFIG_FAILED_WORKSPACE)"
        return 1
    fi

    _pane_mapping_init

    local main_index
    main_index=$(_pane_get_main_index)

    log_debug "$(_i18n DEBUG_FOCUS_MAIN_PANE) (index=$main_index)"
    layout_select_pane "$branch" "$main_index"

    log_success "$(_i18n INFO_WORKSPACE_READY) $session_name"
    return 0
}

layout_reload() {
    local branch="$1"
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    log_info "$(_i18n INFO_RELOADING_LAYOUT)"
    cache_invalidate "config:*"
    layout_apply_commands "$branch"
}

# ──────────────────────────────────────────────────
# Layout info (JSON output)
# ──────────────────────────────────────────────────

layout_get_info() {
    local branch="$1"

    _layout_normalize

    local panes_json=""
    local idx=0 c p

    for ((c=0; c<_LCOL_COUNT; c++)); do
        local pc=${_LCOL_PANE_COUNTS[$c]}
        for ((p=0; p<pc; p++)); do
            local cmd="${_LPANE_COMMANDS[$idx]}"
            _is_null_or_empty "$cmd" && cmd=""
            local safe_cmd
            safe_cmd=$(json_escape "$cmd")
            local role="${_LPANE_ROLES[$idx]:-}"
            _is_null_or_empty "$role" && role=""
            local safe_role
            safe_role=$(json_escape "$role")

            [[ -n "$panes_json" ]] && panes_json="${panes_json},"$'\n'
            panes_json="${panes_json}    {\"column\": $c, \"row\": $p, \"width\": ${_LCOL_WIDTHS[$c]}, \"height\": ${_LPANE_HEIGHTS[$idx]}, \"command\": \"${safe_cmd}\", \"role\": \"${safe_role}\", \"index\": ${idx}}"
            idx=$((idx + 1))
        done
    done

    cat <<EOF
{
  "columns": $_LCOL_COUNT,
  "total_panes": $_LPANE_TOTAL,
  "panes": [
$panes_json
  ]
}
EOF
}

# ──────────────────────────────────────────────────
# Pane operations
# ──────────────────────────────────────────────────

layout_select_pane() {
    local branch="$1"
    local pane_index="$2"

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    terminal_adapter_select_pane "$session_name" "$pane_index"
}

layout_resize_pane() {
    local branch="$1"
    local pane_index="$2"
    local direction="$3"
    local amount="$4"

    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    local adapter
    adapter=$(terminal_adapter_get)

    if [[ "$adapter" == "tmux" ]]; then
        case "$direction" in
            up|down|left|right) ;;
            *)
                log_error "$(_i18n ERR_INVALID_DIRECTION) $direction"
                return 1
                ;;
        esac
        terminal_adapter_resize_pane "$session_name" "$pane_index" "$direction" "$amount"
    else
        log_warn "$(_i18n_fmt WARN_ADAPTER_NO_RESIZE "$adapter")"
        return 1
    fi
}

layout_validate() {
    local branch="$1"
    local SESSION_NAME
    session_require_exists "$branch" || return 1
    local session_name="$SESSION_NAME"

    local expected_panes
    expected_panes=$(_layout_expected_pane_count)

    local adapter
    adapter=$(terminal_adapter_get)

    if [[ "$adapter" == "tmux" ]]; then
        local pane_count
        pane_count=$(terminal_adapter_count_panes "$session_name" 2>/dev/null || echo "0")

        if [[ "$pane_count" -lt "$expected_panes" ]]; then
            log_debug "$(_i18n_fmt DEBUG_LAYOUT_MISMATCH_FMT "$expected_panes" "$pane_count")"
            return 1
        fi

        if [[ "$pane_count" -gt "$expected_panes" ]]; then
            log_debug "$(_i18n_fmt DEBUG_LAYOUT_EXTRA_PANES_FMT "$expected_panes" "$pane_count")"
        fi
    fi

    log_debug "$(_i18n_fmt DEBUG_LAYOUT_COMPLETE_FMT "$session_name" "$expected_panes")"
    return 0
}

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Agent Status Detection Service
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

_layout_protocol_state_to_legacy() {
    local state="${1:-idle}"
    case "$state" in
        waiting_for_user|waiting) printf '%s' 'waiting' ;;
        running) printf '%s' 'running' ;;
        completed|failed) printf '%s' 'stopped' ;;
        *) printf '%s' 'none' ;;
    esac
}

_layout_infer_agent_legacy_status() {
    local agent_name="${1:-}"
    local pane_cmd="${2:-}"
    local protocol_state="${3:-idle}"

    [[ -n "$agent_name" ]] || { printf '%s' 'none'; return 0; }

    local legacy
    legacy=$(_layout_protocol_state_to_legacy "$protocol_state")
    if [[ "$legacy" != "none" ]]; then
        printf '%s' "$legacy"
        return 0
    fi

    # Fallback: manually launched AI tools usually don't publish protocol state.
    local cmd_bin
    cmd_bin=$(_layout_command_bin "$pane_cmd")
    if [[ -n "$cmd_bin" ]] && tools_registry_is_ai_command "$cmd_bin"; then
        printf '%s' 'running'
        return 0
    fi

    printf '%s' 'none'
}

_layout_detect_agent_name() {
    local pane_cmd="${1:-}"
    local source="${2:-}"

    local source_name="${source##*:}"
    if [[ -n "$source_name" && "$source_name" != "$source" ]]; then
        printf '%s' "$source_name"
        return 0
    fi

    local cmd_bin
    cmd_bin=$(_layout_command_bin "$pane_cmd")
    if tools_registry_is_ai_command "$cmd_bin"; then
        printf '%s' "$cmd_bin"
        return 0
    fi

    printf '%s' ''
}

_layout_read_pane_agent_state() {
    local pane_target="$1"
    local state
    state=$(tmux show-options -pt "$pane_target" -v @workhive_agent_state 2>/dev/null || printf '%s' '')
    case "$state" in
        waiting) state="waiting_for_user" ;;
        none) state="idle" ;;
    esac
    case "$state" in
        running|waiting_for_user|idle|unknown|completed|failed) printf '%s' "$state" ;;
        *) printf '%s' 'idle' ;;
    esac
}

agent_detect_in_pane() {
    local session_name="$1"
    local pane_index="$2"

    local idx pane_id pane_pid agent_name agent_state state_source confidence reason
    while IFS='|' read -r idx pane_id pane_pid agent_name agent_state state_source confidence reason; do
        [[ "$idx" == "$pane_index" ]] || continue
        printf '%s' "${agent_name:-}"
        return 0
    done < <(pane_state_collect_session "$session_name" 2>/dev/null)

    printf '%s' ''
}

agent_detect_status() {
    local session_name="$1"
    local pane_index="$2"

    local idx agent_name agent_state
    local _ignored_pane_id _ignored_pane_pid _ignored_state_source _ignored_confidence _ignored_reason
    while IFS='|' read -r idx _ignored_pane_id _ignored_pane_pid agent_name agent_state _ignored_state_source _ignored_confidence _ignored_reason; do
        [[ "$idx" == "$pane_index" ]] || continue
        if [[ -z "$agent_name" ]]; then
            printf '%s' 'none'
            return 0
        fi
        pane_state_to_legacy "$agent_state"
        return 0
    done < <(pane_state_collect_session "$session_name" 2>/dev/null)

    printf '%s' 'none'
}

agent_detect_all_panes() {
    local session_name="$1"

    local idx agent_name agent_state
    local _ignored_pane_id _ignored_pane_pid _ignored_state_source _ignored_confidence _ignored_reason
    while IFS='|' read -r idx _ignored_pane_id _ignored_pane_pid agent_name agent_state _ignored_state_source _ignored_confidence _ignored_reason; do
        [[ -z "$idx" ]] && continue

        local agent_status
        if [[ -z "$agent_name" ]]; then
            agent_status="none"
        else
            agent_status=$(pane_state_to_legacy "$agent_state")
        fi
        printf '%s|%s|%s\n' "$idx" "${agent_name:-}" "$agent_status"
    done < <(pane_state_collect_session "$session_name" 2>/dev/null)
}

agent_needs_attention() {
    local session_name="$1"
    local result
    result=$(agent_detect_all_panes "$session_name")
    echo "$result" | grep -q "|waiting$"
}

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Smart Notification System
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

_agent_notify_bell() {
    printf '\a' 2>/dev/null || true
}

_agent_notify_tmux() {
    local session_name="$1"
    local message="$2"
    tmux display-message -t "$session_name" "$message" 2>/dev/null || true
}

_agent_notify_os() {
    local title="$1"
    local message="$2"
    if [[ "$(uname -s)" == "Darwin" ]]; then
        osascript -e "display notification \"$message\" with title \"$title\"" 2>/dev/null || true
    fi
}

agent_notify() {
    local session_name="$1"
    local pane_index="$2"
    local agent_name="$3"
    local event="$4"

    local message
    if [[ "$event" == "finished" ]]; then
        message="Agent $agent_name (pane $pane_index) finished"
    elif [[ "$event" == "waiting" ]]; then
        message="Agent $agent_name (pane $pane_index) needs input"
    else
        message="Agent $agent_name (pane $pane_index): $event"
    fi

    _agent_notify_bell
    _agent_notify_tmux "$session_name" "workhive: $message"
}

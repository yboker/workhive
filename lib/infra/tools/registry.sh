#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Tools Registry
# Tools registry — reads available tool list from configuration (Infra layer)
#
# Responsibility: reads the tools config node, provides tool query interface
# Data source: global config.yaml tools.{default,list} (user-level) > built-in defaults
# Note: tools is an object, does not use config_get (which only supports simple paths)
#       reads config files directly with yq
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_TOOLS_REGISTRY_SH_LOADED:-}" ]] && return 0
readonly _TOOLS_REGISTRY_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/infra/config/loader.sh"

# ──────────────────────────────────────────────────
# Internal helpers
# ──────────────────────────────────────────────────

# Detect whether a config file contains a tools node.
# Args: $1 = config file path
# Returns (stdout): "true" or "false"
_tools_registry_has_tools_node() {
    local cfg_file="${1:-}"
    if [[ -z "$cfg_file" || ! -f "$cfg_file" ]]; then
        echo "false"
        return 0
    fi
    if ! check_cmd yq; then
        echo "false"
        return 0
    fi
    _tools_registry_yq_or '.tools != null' "$cfg_file" "false"
}

_tools_registry_write_fallback_config_into() {
    local out_var="$1"
    [[ -n "$out_var" ]] || return 1

    local tmp_file
    tmp_file=$(mktemp "${TMPDIR:-/tmp}/workhive-tools.XXXXXX") || {
        log_error "Failed to create temporary tools config file"
        return 1
    }
    {
        printf 'tools:\n'
        local _line
        while IFS= read -r _line; do
            printf '  %s\n' "$_line"
        done < <(config_get_default_tools_yaml)
    } > "$tmp_file" || {
        cleanup_file_quiet "$tmp_file"
        log_error "Failed to write temporary tools config file"
        return 1
    }
    printf -v "$out_var" '%s' "$tmp_file"
    return 0
}

# Find the first config file containing a tools node
# Priority: global-level > built-in defaults (temp file)
# Args: $1 = output variable name
_tools_registry_find_config_into() {
    local out_var="$1"
    [[ -n "$out_var" ]] || return 1

    _TOOLS_REGISTRY_LAST_CFG_IS_TMP=false
    local project_cfg="${_WORKHIVE_DIR_OVERRIDE:-$(_workhive_dir)}/config.yaml"
    local project_has_tools
    project_has_tools=$(_tools_registry_has_tools_node "$project_cfg")
    if [[ "$project_has_tools" == "true" ]] && [[ "${_TOOLS_REGISTRY_WARNED_PROJECT_SCOPE:-false}" != "true" ]]; then
        log_warn "Project-level 'tools' is ignored. Use global config tools only."
        _TOOLS_REGISTRY_WARNED_PROJECT_SCOPE=true
    fi

    local global_cfg
    global_cfg=$(config_get_global_file)
    local global_has_tools
    global_has_tools=$(_tools_registry_has_tools_node "$global_cfg")
    if [[ "$global_has_tools" == "true" ]]; then
        _TOOLS_REGISTRY_LAST_CFG_IS_TMP=false
        printf -v "$out_var" '%s' "$global_cfg"
        return 0
    fi

    # Built-in defaults: write to a temp file for yq to read.
    # Use XXXXXX at the end of template for BSD/GNU mktemp compatibility.
    local tmp=""
    _tools_registry_write_fallback_config_into tmp || return 1
    _TOOLS_REGISTRY_LAST_CFG_IS_TMP=true
    printf -v "$out_var" '%s' "$tmp"
    return 0
}

_tools_registry_print_cfg_from_into() {
    local resolver_fn="$1"
    local cfg_file
    "$resolver_fn" cfg_file || return 1
    printf '%s\n' "$cfg_file"
}

# Backward-compatible stdout wrapper.
_tools_registry_find_config() {
    _tools_registry_print_cfg_from_into "_tools_registry_find_config_into"
}

# Cached version: avoid repeated config file lookups + yq length checks
_TOOLS_REGISTRY_CFG_CACHE=""
_TOOLS_REGISTRY_CFG_CACHE_SET=false
_TOOLS_REGISTRY_CFG_IS_TMP=false
_TOOLS_REGISTRY_LAST_CFG_IS_TMP=false

_tools_registry_find_config_cached_into() {
    local out_var="$1"
    [[ -n "$out_var" ]] || return 1

    if [[ "$_TOOLS_REGISTRY_CFG_CACHE_SET" == true ]]; then
        printf -v "$out_var" '%s' "$_TOOLS_REGISTRY_CFG_CACHE"
        return 0
    fi
    local resolved_cfg=""
    _tools_registry_find_config_into resolved_cfg || resolved_cfg=""
    _TOOLS_REGISTRY_CFG_CACHE="$resolved_cfg"
    if [[ -z "$resolved_cfg" ]]; then
        _TOOLS_REGISTRY_CFG_CACHE=""
        _TOOLS_REGISTRY_CFG_CACHE_SET=false
        _TOOLS_REGISTRY_CFG_IS_TMP=false
        return 1
    fi
    _TOOLS_REGISTRY_CFG_CACHE_SET=true
    _TOOLS_REGISTRY_CFG_IS_TMP="${_TOOLS_REGISTRY_LAST_CFG_IS_TMP:-false}"
    printf -v "$out_var" '%s' "$_TOOLS_REGISTRY_CFG_CACHE"
    return 0
}

_tools_registry_find_config_cached() {
    _tools_registry_print_cfg_from_into "_tools_registry_find_config_cached_into"
}

# Higher-order helper: execute yq query with the found config file, auto-clean temp files
# Args: $1 = yq expression
# Returns (stdout): yq output
# Note: only for non-batch queries; for batch queries, use tools_registry_list() directly
_tools_registry_yq() {
    local expr="$1"
    local cfg_file
    _tools_registry_find_config_cached_into cfg_file || return 1
    if [[ -z "$cfg_file" || ! -f "$cfg_file" ]]; then
        _tools_registry_cleanup_cached_tmp
        return 1
    fi

    local result
    if ! result=$(yq "$expr" "$cfg_file" 2>/dev/null); then
        _tools_registry_cleanup_cached_tmp
        return 1
    fi
    _tools_registry_cleanup_cached_tmp
    echo "$result"
}

_tools_registry_cleanup_cached_tmp() {
    if [[ "$_TOOLS_REGISTRY_CFG_IS_TMP" == true ]]; then
        local cached_tmp="${_TOOLS_REGISTRY_CFG_CACHE:-}"
        cleanup_file_quiet "$cached_tmp"
        _TOOLS_REGISTRY_CFG_CACHE=""
        _TOOLS_REGISTRY_CFG_CACHE_SET=false
        _TOOLS_REGISTRY_CFG_IS_TMP=false
    fi
}

_tools_registry_get_existing_cfg_into() {
    local out_var="$1"
    [[ -n "$out_var" ]] || return 1

    local _cfg_path=""
    _tools_registry_find_config_cached_into _cfg_path || _cfg_path=""
    if [[ -z "$_cfg_path" || ! -f "$_cfg_path" ]]; then
        _tools_registry_cleanup_cached_tmp
        printf -v "$out_var" '%s' ""
        return 1
    fi
    printf -v "$out_var" '%s' "$_cfg_path"
    return 0
}

_tools_registry_require_yq() {
    local err_msg="${1:-yq is required for tools registry}"
    if ! check_cmd yq; then
        log_error "$err_msg"
        return 1
    fi
    return 0
}

_tools_registry_yq_exec_or() {
    local mode="$1"
    local expr="$2"
    local cfg_file="$3"
    local fallback="${4:-}"

    if [[ "$mode" == "read" ]]; then
        yq -r "$expr" "$cfg_file" 2>/dev/null || echo "$fallback"
        return 0
    fi
    yq "$expr" "$cfg_file" 2>/dev/null || echo "$fallback"
}

_tools_registry_yq_or() {
    local expr="$1"
    local cfg_file="$2"
    local fallback="${3:-}"
    _tools_registry_yq_exec_or "raw" "$expr" "$cfg_file" "$fallback"
}

_tools_registry_yq_read_or() {
    local expr="$1"
    local cfg_file="$2"
    local fallback="${3:-}"
    _tools_registry_yq_exec_or "read" "$expr" "$cfg_file" "$fallback"
}

_tools_registry_fail_with_cleanup() {
    local cfg_file="$1"
    local msg="$2"
    log_error "$msg"
    _tools_registry_cleanup_cached_tmp
    return 1
}

_tools_registry_invalid_config() {
    local cfg_file="$1"
    local detail="$2"
    _tools_registry_fail_with_cleanup "$cfg_file" "Invalid tools config: $detail"
}

_tools_registry_is_special_default_ref() {
    local value="${1:-}"
    [[ "$value" == "__empty__" || "$value" == "__custom__" ]]
}

_tools_registry_invalid_default_ref() {
    local cfg_file="$1"
    local default_ref="$2"
    _tools_registry_invalid_config "$cfg_file" "tools.default '$default_ref' not found in tools.list"
}

declare -A _TOOLS_REGISTRY_BIN_INSTALLED_CACHE=()

_tools_registry_reset_bin_installed_cache() {
    _TOOLS_REGISTRY_BIN_INSTALLED_CACHE=()
}

_tools_registry_is_bin_available_cached() {
    local cmd_bin="${1:-}"
    [[ -n "$cmd_bin" ]] || return 1

    if [[ -z "${_TOOLS_REGISTRY_BIN_INSTALLED_CACHE[$cmd_bin]+x}" ]]; then
        if command -v "$cmd_bin" &>/dev/null; then
            _TOOLS_REGISTRY_BIN_INSTALLED_CACHE["$cmd_bin"]=1
        else
            _TOOLS_REGISTRY_BIN_INSTALLED_CACHE["$cmd_bin"]=0
        fi
    fi
    [[ "${_TOOLS_REGISTRY_BIN_INSTALLED_CACHE[$cmd_bin]}" == "1" ]]
}

_tools_registry_resolve_command_bin() {
    local command="${1:-}"
    local require_installed="${2:-true}"
    local cmd_bin
    cmd_bin=$(_tools_registry_command_bin "$command")
    [[ -n "$cmd_bin" ]] || return 1

    if [[ "$require_installed" == "true" ]]; then
        _tools_registry_is_bin_available_cached "$cmd_bin" || return 1
    fi
    REPLY="$cmd_bin"
    return 0
}

_tools_registry_set_list_cache() {
    local value="${1:-}"
    _TOOLS_REGISTRY_LIST_CACHE="$value"
    _TOOLS_REGISTRY_LIST_CACHE_SET=true
}

_tools_registry_bool_is_true() {
    local value="${1:-}"
    [[ "$value" == "true" ]]
}

# ──────────────────────────────────────────────────
# Public API
# ──────────────────────────────────────────────────

# Result cache: query the registry only once per command lifecycle
_TOOLS_REGISTRY_LIST_CACHE=""
_TOOLS_REGISTRY_LIST_CACHE_SET=false
readonly _TOOLS_REGISTRY_AI_FALLBACK_BINS="claude codex opencode aider gemini goose cline hermes"

tools_registry_reset_cache() {
    if [[ "$_TOOLS_REGISTRY_CFG_IS_TMP" == true && -n "${_TOOLS_REGISTRY_CFG_CACHE:-}" ]]; then
        cleanup_file_quiet "$_TOOLS_REGISTRY_CFG_CACHE"
    fi
    _TOOLS_REGISTRY_CFG_CACHE=""
    _TOOLS_REGISTRY_CFG_CACHE_SET=false
    _TOOLS_REGISTRY_CFG_IS_TMP=false
    _TOOLS_REGISTRY_LIST_CACHE=""
    _TOOLS_REGISTRY_LIST_CACHE_SET=false
}

_tools_registry_internal_id_from_name() {
    local tool_name="$1"
    local normalized="${tool_name,,}"
    normalized="${normalized//[^a-z0-9]/-}"
    while [[ "$normalized" == *"--"* ]]; do
        normalized="${normalized//--/-}"
    done
    normalized="${normalized##-}"
    normalized="${normalized%%-}"
    [[ -z "$normalized" ]] && normalized="tool"
    printf '%s' "$normalized"
}

_tools_registry_load_rows() {
    _tools_registry_require_yq "yq is required for tools registry" || return 1

    local cfg_file
    _tools_registry_get_existing_cfg_into cfg_file || return 1

    local raw_query raw_rows
    raw_query='. as $cfg |
      ($cfg.tools.default // "") as $def |
      ($cfg.tools.list // [])[]? |
      [
        (.tool_name // ""),
        (.label // .tool_name // "tool"),
        (.command // ""),
        (.enabled),
        (.is_ai),
        (((.tool_name // "") == $def) | tostring)
      ] | @tsv'
    if ! raw_rows=$(yq -r "$raw_query" "$cfg_file" 2>/dev/null); then
        _tools_registry_cleanup_cached_tmp
        return 1
    fi

    local normalized_rows=""
    local line name label command enabled is_ai is_default internal_id
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        IFS=$'\t' read -r name label command enabled is_ai is_default <<< "$line"
        [[ -z "$enabled" || "$enabled" == "null" ]] && enabled="true"
        [[ -z "$is_ai" || "$is_ai" == "null" ]] && is_ai="false"
        internal_id=$(_tools_registry_internal_id_from_name "$name")
        if [[ -n "$normalized_rows" ]]; then
            normalized_rows+=$'\n'
        fi
        normalized_rows+="${name}"$'\t'"${internal_id}"$'\t'"${label}"$'\t'"${command}"$'\t'"${enabled}"$'\t'"${is_ai}"$'\t'"${is_default}"
    done <<< "$raw_rows"

    _tools_registry_cleanup_cached_tmp
    [[ -n "$normalized_rows" ]] && printf '%s\n' "$normalized_rows"
    return 0
}

tools_registry_load() {
    _tools_registry_load_rows
}

tools_registry_validate() {
    _tools_registry_require_yq "yq is required for tools registry" || return 1

    local cfg_file
    _tools_registry_get_existing_cfg_into cfg_file || return 0

    local raw
    if [[ "$(_tools_registry_yq_or '.tools | type' "$cfg_file" "")" != "!!map" ]]; then
        _tools_registry_invalid_config "$cfg_file" "expected 'tools' to be an object with keys {default,list}"
        return 1
    fi
    if [[ "$(_tools_registry_yq_or '.tools.list | type' "$cfg_file" "")" != "!!seq" ]]; then
        _tools_registry_invalid_config "$cfg_file" "expected 'tools.list' to be an array"
        return 1
    fi

    local tools_default
    tools_default=$(_tools_registry_yq_read_or '.tools.default // ""' "$cfg_file" "")
    local rows
    rows=$(_tools_registry_load_rows) || return 1
    [[ -z "$rows" ]] && {
        if [[ -n "$tools_default" ]] && ! _tools_registry_is_special_default_ref "$tools_default"; then
            _tools_registry_invalid_default_ref "$cfg_file" "$tools_default"
            return 1
        fi
        _tools_registry_cleanup_cached_tmp
        return 0
    }

    local line name internal_id label command enabled is_ai is_default
    declare -A _seen_tool_names=()
    declare -A _seen_internal_ids=()
    local has_default_name=false
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS=$'\t' read -r name internal_id label command enabled is_ai is_default <<< "$line"

        if [[ -z "$name" || "$name" == "null" ]]; then
            _tools_registry_invalid_config "$cfg_file" "tool_name is required"
            return 1
        fi
        if [[ -n "${_seen_tool_names[$name]:-}" ]]; then
            _tools_registry_invalid_config "$cfg_file" "duplicated tool_name '$name'"
            return 1
        fi
        _seen_tool_names["$name"]=1
        if [[ -n "${_seen_internal_ids[$internal_id]:-}" ]]; then
            _tools_registry_invalid_config "$cfg_file" "normalized tool_name conflict on '$internal_id' (rename '$name')"
            return 1
        fi
        _seen_internal_ids["$internal_id"]=1

        if [[ -z "$command" || "$command" == "null" ]]; then
            _tools_registry_invalid_config "$cfg_file" "command is required for tool '$name'"
            return 1
        fi
        if [[ "$name" == "$tools_default" ]]; then
            has_default_name=true
        fi
    done <<< "$rows"
    if [[ "$tools_default" == "null" ]]; then
        _tools_registry_invalid_config "$cfg_file" "tools.default must be string or omitted"
        return 1
    fi
    if [[ -n "$tools_default" ]] && ! _tools_registry_is_special_default_ref "$tools_default"; then
        if [[ "$has_default_name" != "true" ]]; then
            _tools_registry_invalid_default_ref "$cfg_file" "$tools_default"
            return 1
        fi
    fi

    _tools_registry_cleanup_cached_tmp
    return 0
}

# Validate that each enabled tool command is executable in current environment.
# - checks enabled!=false entries only
# - validates command binary (first token) via command -v
tools_registry_validate_commands_available() {
    _tools_registry_require_yq "$(_i18n ERR_NO_YQ)" || return 1

    local rows
    rows=$(_tools_registry_load_rows) || return 0
    [[ -z "$rows" ]] && return 0

    local has_error=false
    _tools_registry_reset_bin_installed_cache
    local line name internal_id label command enabled is_ai is_default cmd_bin
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS=$'\t' read -r name internal_id label command enabled is_ai is_default <<< "$line"
        [[ -z "$enabled" || "$enabled" == "null" ]] && enabled="true"
        if ! _tools_registry_bool_is_true "$enabled"; then
            continue
        fi

        if [[ -z "$command" || "$command" == "null" ]]; then
            log_error "$(_i18n_fmt TOOLS_ERR_COMMAND_EMPTY_FMT "$name")"
            has_error=true
            continue
        fi

        if ! _tools_registry_resolve_command_bin "$command" "false"; then
            log_error "$(_i18n_fmt TOOLS_ERR_COMMAND_EMPTY_FMT "$name")"
            has_error=true
            continue
        fi
        cmd_bin="$REPLY"
        if ! _tools_registry_is_bin_available_cached "$cmd_bin"; then
            log_error "$(_i18n_fmt TOOLS_ERR_COMMAND_NOT_FOUND_FMT "$name" "$cmd_bin" "$command")"
            has_error=true
        fi
    done <<< "$rows"

    [[ "$has_error" == true ]] && return 1
    return 0
}

# List all tools with enabled=true and command installed
# Output format (per line): id|label|command|is_ai|default
# Performance: single yq call to batch-extract all tool data + result caching
tools_registry_list() {
    # Return cached result
    if [[ "$_TOOLS_REGISTRY_LIST_CACHE_SET" == true ]]; then
        [[ -n "$_TOOLS_REGISTRY_LIST_CACHE" ]] && echo "$_TOOLS_REGISTRY_LIST_CACHE"
        return 0
    fi

    if ! tools_registry_validate; then
        _tools_registry_cleanup_cached_tmp
        return 1
    fi

    local raw_result
    raw_result=$(_tools_registry_load_rows) || {
        _tools_registry_cleanup_cached_tmp
        return 1
    }

    # Filter out disabled and uninstalled tools
    local result_filtered="" line
    _tools_registry_reset_bin_installed_cache
    local _name _label _cmd _is_ai _is_default _is_default_raw _enabled
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS=$'\t' read -r _name _id _label _cmd _enabled _is_ai _is_default_raw <<< "$line"
        [[ -z "$_enabled" || "$_enabled" == "null" ]] && _enabled="true"
        [[ -z "$_is_ai" || "$_is_ai" == "null" ]] && _is_ai="false"
        if ! _tools_registry_bool_is_true "${_enabled:-false}"; then
            continue
        fi
        _tools_registry_resolve_command_bin "$_cmd" "true" || continue
        if _tools_registry_bool_is_true "${_is_default_raw:-false}"; then
            _is_default="true"
        else
            _is_default="false"
        fi

        line="${_id}|${_label}|${_cmd}|${_is_ai}|${_is_default}"
        if [[ -n "$result_filtered" ]]; then
            result_filtered="${result_filtered}
${line}"
        else
            result_filtered="$line"
        fi
    done <<< "$raw_result"

    # Cleanup temporary files
    _tools_registry_cleanup_cached_tmp

    # Cache the result
    _tools_registry_set_list_cache "$result_filtered"

    [[ -n "$result_filtered" ]] && echo "$result_filtered"
    return 0
}

_tools_registry_filter_ai_rows() {
    local registry_rows="${1:-}"
    local line _id _label _cmd _is_ai _is_default
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS='|' read -r _id _label _cmd _is_ai _is_default <<< "$line"
        [[ "$_is_ai" == "true" ]] && printf '%s\n' "$line"
    done <<< "$registry_rows"
}

# List all enabled AI tools (enabled=true && is_ai=true) with commands installed
# Output format same as tools_registry_list
tools_registry_list_ai() {
    local registry_rows
    registry_rows=$(tools_registry_list) || return 1
    _tools_registry_filter_ai_rows "$registry_rows"
}

_tools_registry_get_default_idx_from_stream() {
    local i=0
    local line _id _label _cmd _is_ai _is_default
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS='|' read -r _id _label _cmd _is_ai _is_default <<< "$line"
        if _tools_registry_bool_is_true "${_is_default:-false}"; then
            echo "$i"
            return 0
        fi
        i=$((i + 1))
    done
    echo "0"
}

_tools_registry_first_id_from_stream() {
    local line row_id _label _cmd _is_ai _is_default
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        IFS='|' read -r row_id _label _cmd _is_ai _is_default <<< "$line"
        [[ -n "$row_id" ]] || continue
        printf '%s\n' "$row_id"
        return 0
    done
    return 1
}

_tools_registry_get_default_idx_for_scope() {
    local scope="${1:-all}"
    case "$scope" in
        all) _tools_registry_get_default_idx_from_stream < <(tools_registry_list) ;;
        ai)  _tools_registry_get_default_idx_from_stream < <(tools_registry_list_ai) ;;
        *)   echo "0" ;;
    esac
}

# Get the 0-based index of the default=true tool in the installed list
# Returns (stdout): numeric index, returns 0 if not found
tools_registry_get_default_idx() {
    _tools_registry_get_default_idx_for_scope "all"
}

# Get the 0-based index of the default=true AI tool in the AI tool list
# Returns (stdout): numeric index, returns 0 if not found
tools_registry_get_ai_default_idx() {
    _tools_registry_get_default_idx_for_scope "ai"
}

# Get raw default selector from tools config.
# Returns: tool_name | "__empty__" | "__custom__" | ""
tools_registry_get_default_ref() {
    _tools_registry_require_yq "yq is required for tools registry" >/dev/null 2>&1 || { echo ""; return 1; }
    local cfg_file
    if ! _tools_registry_get_existing_cfg_into cfg_file; then
        echo ""
        return 0
    fi
    local ref
    ref=$(_tools_registry_yq_read_or '.tools.default // ""' "$cfg_file" "")
    _tools_registry_cleanup_cached_tmp
    echo "$ref"
    return 0
}

_tools_registry_get_row_by_id() {
    local target_id="$1"
    [[ -n "$target_id" ]] || return 1

    local line row_id _label _cmd _is_ai _is_default
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS='|' read -r row_id _label _cmd _is_ai _is_default <<< "$line"
        if [[ "$row_id" == "$target_id" ]]; then
            printf '%s\n' "$line"
            return 0
        fi
    done < <(tools_registry_list)
    return 1
}

# Get command string by internal id
# Args: $1 = internal tool id
# Returns (stdout): command string, or empty if not found
tools_registry_get_command() {
    local target_id="$1"
    [[ -n "$target_id" ]] || return 1
    local line
    local _id _label _cmd _rest
    line=$(_tools_registry_get_row_by_id "$target_id") || return 1
    IFS='|' read -r _id _label _cmd _rest <<< "$line"
    echo "$_cmd"
    return 0
}

_tools_registry_scan_by_name() {
    local target_name="${1:-}"
    local output_mode="${2:-row}"
    [[ -n "$target_name" ]] || return 1

    local idx=0
    local row name internal_id label command enabled is_ai is_default
    while IFS= read -r row; do
        [[ -n "$row" ]] || continue
        IFS=$'\t' read -r name internal_id label command enabled is_ai is_default <<< "$row"
        if [[ "$name" == "$target_name" ]]; then
            case "$output_mode" in
                idx) printf '%s\n' "$idx" ;;
                with_idx) printf '%s\t%s\n' "$idx" "$row" ;;
                *) printf '%s\n' "$row" ;;
            esac
            return 0
        fi
        idx=$((idx + 1))
    done < <(tools_registry_load 2>/dev/null || true)
    return 1
}

# Get configured row by tool_name from canonical rows.
# Output format: name\tid\tlabel\tcommand\tenabled\tis_ai\tis_default
tools_registry_find_by_name() {
    _tools_registry_scan_by_name "${1:-}" "row"
}

tools_registry_index_by_name() {
    _tools_registry_scan_by_name "${1:-}" "idx"
}

# Get configured row and index by tool_name in one scan.
# Output format: idx\tname\tid\tlabel\tcommand\tenabled\tis_ai\tis_default
tools_registry_find_with_index_by_name() {
    _tools_registry_scan_by_name "${1:-}" "with_idx"
}

tools_registry_has_normalized_id_conflict() {
    local candidate_name="${1:-}"
    local exclude_name="${2:-}"
    tools_registry_find_normalized_id_conflict_name "$candidate_name" "$exclude_name" >/dev/null 2>&1
}

tools_registry_find_normalized_id_conflict_name() {
    local candidate_name="${1:-}"
    local exclude_name="${2:-}"
    [[ -n "$candidate_name" ]] || return 1

    local candidate_id
    candidate_id=$(_tools_registry_internal_id_from_name "$candidate_name")
    local row name internal_id label command enabled is_ai is_default
    while IFS= read -r row; do
        [[ -n "$row" ]] || continue
        IFS=$'\t' read -r name internal_id label command enabled is_ai is_default <<< "$row"
        [[ -z "$name" ]] && continue
        [[ "$name" == "$exclude_name" ]] && continue
        if [[ "$internal_id" == "$candidate_id" ]]; then
            printf '%s\n' "$name"
            return 0
        fi
    done < <(tools_registry_load 2>/dev/null || true)
    return 1
}

tools_registry_get_default() {
    local line _id _label _cmd _is_ai _is_default
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        IFS='|' read -r _id _label _cmd _is_ai _is_default <<< "$line"
        if [[ "$_is_default" == "true" ]]; then
            printf '%s\n' "$_id"
            return 0
        fi
    done < <(tools_registry_list)
    return 1
}

# Check if a tool id is installed
# Args: $1 = tool id
# Returns: 0 = installed, 1 = not installed or does not exist
tools_registry_is_installed() {
    local target_id="$1"
    local cmd
    cmd=$(tools_registry_get_command "$target_id") || return 1
    _tools_registry_resolve_command_bin "$cmd" "true"
}

# Detect the highest priority available AI tool id (first installed tool in config order)
# Returns (stdout): tool id, empty on failure with return 1
tools_registry_detect_ai() {
    local registry_rows
    registry_rows=$(tools_registry_list) || return 1
    _tools_registry_first_id_from_stream < <(_tools_registry_filter_ai_rows "$registry_rows")
}

_tools_registry_command_bin() {
    local cmd="${1:-}"
    cmd="${cmd#"${cmd%%[![:space:]]*}"}"
    printf '%s' "${cmd%% *}"
}

tools_registry_command_bin() {
    _tools_registry_command_bin "${1:-}"
}

_tools_registry_match_variant_bin() {
    local observed="$1"
    local canonical="$2"
    [[ -z "$observed" || -z "$canonical" ]] && return 1
    [[ "$observed" == "$canonical" ]] && return 0
    [[ "$observed" == "${canonical}-"* ]] && return 0
    [[ "$observed" == "${canonical}_"* ]] && return 0
    [[ "$observed" == "${canonical}."* ]] && return 0
    return 1
}

tools_registry_normalize_ai_command() {
    local cmd="${1:-}"
    local cmd_bin
    cmd_bin=$(_tools_registry_command_bin "$cmd")
    [[ -n "$cmd_bin" ]] || return 1
    local cmd_leaf="${cmd_bin##*/}"

    # Priority 1: registered AI tools (tools.list with is_ai=true)
    local line _id _label _cmd _is_ai _is_default configured_bin configured_leaf
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS='|' read -r _id _label _cmd _is_ai _is_default <<< "$line"
        configured_bin=$(_tools_registry_command_bin "$_cmd")
        [[ -n "$configured_bin" ]] || continue
        configured_leaf="${configured_bin##*/}"

        if _tools_registry_match_variant_bin "$cmd_leaf" "$configured_leaf"; then
            printf '%s' "$configured_leaf"
            return 0
        fi
    done < <(tools_registry_list_ai 2>/dev/null || true)

    # Priority 2: static fallback AI binaries
    local fallback
    for fallback in $_TOOLS_REGISTRY_AI_FALLBACK_BINS; do
        if _tools_registry_match_variant_bin "$cmd_leaf" "$fallback"; then
            printf '%s' "$fallback"
            return 0
        fi
    done

    return 1
}

tools_registry_is_ai_command() {
    local cmd="${1:-}"
    tools_registry_normalize_ai_command "$cmd" >/dev/null 2>&1
}

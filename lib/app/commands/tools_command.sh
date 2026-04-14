#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Tools Command
# Global tools registry management
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_TOOLS_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _TOOLS_COMMAND_SH_LOADED=1

source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/tools/registry.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/interactive.sh"

tools_command_help() {
    _help_dispatch "_tools_command_help"
}

_tools_command_help_en() {
    cat <<EOF
Usage: workhive tools <subcommand> [options]

Manage global tools registry (~/.config/workhive/config.yaml).

Subcommands:
  list                          List configured tools
  add --name <n> --cmd <c>      Add a tool
  edit <name> [--label <l>] [--cmd <c>] [--ai <true|false>] [--enable <true|false>]
                                Edit a tool
  remove <name>                 Remove a tool
  set-default <name>            Set default tool
  validate                      Validate tools config
  manage                        Interactive manager (list/add/edit/remove/set-default)
  -h, --help                    Show help

Notes:
  - tools are global-only; project-level tools are ignored
  - tool_name must be unique
EOF
}

_tools_command_help_zh() {
    cat <<EOF
用法: workhive tools <subcommand> [options]

管理全局工具注册表 (~/.config/workhive/config.yaml)。

子命令:
  list                          列出工具
  add --name <n> --cmd <c>      添加工具
  edit <name> [--label <l>] [--cmd <c>] [--ai <true|false>] [--enable <true|false>]
                                编辑工具
  remove <name>                 删除工具
  set-default <name>            设置默认工具
  validate                      校验工具配置
  manage                        交互式管理 (增删改查/设默认)
  -h, --help                    显示帮助

说明:
  - tools 仅支持全局配置；项目级 tools 会被忽略
  - tool_name 必须唯一
EOF
}

_tools_cfg_file() {
    config_get_global_file
}

_tools_require_global_file() {
    local cfg
    cfg=$(_tools_cfg_file)
    if [[ ! -f "$cfg" ]]; then
        log_error "$(_i18n INFO_NO_GLOBAL_CONFIG)"
        log_info "$(_i18n HINT_SETUP_GLOBAL)"
        return 1
    fi
    if ! config_check_yq_version; then
        return 1
    fi
    config_ensure_scope_fields "$cfg" "global" || return 1
    tools_registry_reset_cache
    tools_registry_validate || return 1
    return 0
}

_tools_exists_by_name() {
    local name="$1"
    tools_registry_find_by_name "$name" >/dev/null 2>&1
}

_tools_require_cfg_index_and_row_by_name_into() {
    local name="$1"
    local cfg_out_var="$2"
    local idx_out_var="$3"
    local row_out_var="${4:-}"

    local cfg_path result_line idx_value row_value
    _tools_cfg_path_into cfg_path
    result_line=$(tools_registry_find_with_index_by_name "$name" 2>/dev/null || true)
    if [[ -z "$result_line" ]]; then
        _tools_log_name_not_found "$name"
        return 1
    fi
    IFS=$'\t' read -r idx_value row_value <<< "$result_line"

    printf -v "$cfg_out_var" '%s' "$cfg_path"
    printf -v "$idx_out_var" '%s' "$idx_value"
    [[ -n "$row_out_var" ]] && printf -v "$row_out_var" '%s' "$row_value"
    return 0
}

_tools_cfg_path_into() {
    local out_var="$1"
    local resolved_cfg_path
    resolved_cfg_path=$(_tools_cfg_file)
    printf -v "$out_var" '%s' "$resolved_cfg_path"
    return 0
}

_tools_edit_set_bool_field() {
    local cfg="$1"
    local idx="$2"
    local field="$3"
    local opt="$4"
    local value="$5"
    local env_key="$6"
    _tools_validate_bool "$value" "$opt" || return 1
    config_atomic_yq_edit "$cfg" ".tools.list[$idx].$field = (strenv($env_key) == \"true\")" "$env_key=$value"
}

_tools_edit_set_string_field() {
    local cfg="$1"
    local idx="$2"
    local field="$3"
    local value="$4"
    local env_key="$5"
    config_atomic_yq_edit "$cfg" ".tools.list[$idx].$field = strenv($env_key)" "$env_key=$value"
}

_tools_finalize_mutation_success() {
    local msg_key="$1"
    local name="$2"
    _tools_invalidate_cache
    _tools_log_success_fmt "$msg_key" "$name"
}

_tools_edit_apply_default_flag() {
    local cfg="$1"
    local name="$2"
    local value="$3"
    _tools_validate_bool "$value" "--default" || return 1
    if [[ "$value" == "true" ]]; then
        _tools_set_default_name "$cfg" "$name"
    else
        _tools_clear_default_if_matches "$cfg" "$name"
    fi
}

_tools_validate_add_bool_options() {
    local is_ai="$1"
    local enabled="$2"
    local is_default="$3"
    _tools_validate_bool "$is_ai" "--ai" || return 1
    _tools_validate_bool "$enabled" "--enable" || return 1
    _tools_validate_bool "$is_default" "--default" || return 1
}

_tools_normalize_default_name_ref() {
    local name="${1:-}"
    case "$name" in
        empty|null|shell) echo "__empty__" ;;
        custom) echo "__custom__" ;;
        *) echo "$name" ;;
    esac
}

_tools_require_default_target_exists() {
    local name="$1"
    if ! _tools_registry_is_special_default_ref "$name" && ! _tools_exists_by_name "$name"; then
        _tools_log_name_not_found "$name"
        return 1
    fi
    return 0
}

_tools_validate_bool() {
    local value="$1"
    local opt="$2"
    if [[ "$value" != "true" && "$value" != "false" ]]; then
        log_error "$(_i18n_fmt TOOLS_ERR_INVALID_BOOL_FMT "$opt" "$value")"
        return 1
    fi
    return 0
}

_tools_validate_new_name() {
    local name="$1"
    [[ -z "$name" ]] && {
        log_error "$(_i18n TOOLS_ERR_MISSING_NAME)"
        return 1
    }
    if _tools_exists_by_name "$name"; then
        log_error "$(_i18n_fmt TOOLS_ERR_NAME_EXISTS_FMT "$name")"
        return 1
    fi
    local conflicting_name
    conflicting_name=$(tools_registry_find_normalized_id_conflict_name "$name" 2>/dev/null || true)
    if [[ -n "$conflicting_name" ]]; then
        local target_internal
        target_internal=$(_tools_registry_internal_id_from_name "$name")
        log_error "$(_i18n_fmt TOOLS_ERR_INTERNAL_ID_CONFLICT_FMT "$name" "$conflicting_name" "$target_internal")"
        return 1
    fi
    return 0
}

_tools_require_name_arg() {
    local name="${1:-}"
    [[ -z "$name" ]] && {
        log_error "$(_i18n TOOLS_ERR_MISSING_NAME)"
        return 1
    }
    return 0
}

_tools_log_name_not_found() {
    local name="$1"
    log_error "$(_i18n_fmt TOOLS_ERR_NAME_NOT_FOUND_FMT "$name")"
}

_tools_log_success_fmt() {
    local key="$1"
    shift
    log_success "$(_i18n_fmt "$key" "$@")"
}

_tools_fail_unknown_opt() {
    local opt="$1"
    cli_log_unknown_opt "$opt"
    return 1
}

_tools_prompt_bool_tui() {
    local prompt="$1"
    local default_value="${2:-false}"
    # shellcheck disable=SC2034
    local options=("$(_i18n TOOLS_BOOL_TRUE)" "$(_i18n TOOLS_BOOL_FALSE)")
    local default_idx
    default_idx=$(_tools_bool_default_index "$default_value" "false")
    tui_select_menu "$prompt" options "$default_idx"
    _tools_bool_from_index "${TUI_SELECTED_INDEX:-$default_idx}" "false"
}

_tools_prompt_bool_or_skip_tui() {
    local prompt="$1"
    local default_value="${2:-}"
    # shellcheck disable=SC2034
    local options=("$(_i18n TOOLS_BOOL_TRUE)" "$(_i18n TOOLS_BOOL_FALSE)" "$(_i18n TOOLS_BOOL_SKIP)")
    local default_idx
    default_idx=$(_tools_bool_default_index "$default_value" "true")
    tui_select_menu "$prompt" options "$default_idx"
    _tools_bool_from_index "${TUI_SELECTED_INDEX:-$default_idx}" "true"
}

_tools_bool_default_index() {
    local value="$1"
    local allow_skip="${2:-false}"
    if [[ "$allow_skip" == "true" ]]; then
        case "$value" in
            true) echo "0" ;;
            false) echo "1" ;;
            *) echo "2" ;;
        esac
        return 0
    fi
    [[ "$value" == "true" ]] && echo "0" || echo "1"
}

_tools_bool_from_index() {
    local idx="${1:-0}"
    local allow_skip="${2:-false}"
    case "$idx" in
        0) echo "true" ;;
        1) echo "false" ;;
        *)
            [[ "$allow_skip" == "true" ]] && echo "" || echo "false"
            ;;
    esac
}

_tools_dash_fill() {
    local width="${1:-0}"
    printf '%*s' "$width" '' | tr ' ' '-'
}

_tools_append_edit_arg_if_changed() {
    local -n args_ref="$1"
    local flag="$2"
    local new_value="${3:-}"
    local old_value="${4:-}"
    [[ -n "$new_value" && "$new_value" != "$old_value" ]] && args_ref+=("$flag" "$new_value")
}

_tools_build_edit_args() {
    local out_var="$1"
    local label_new="$2"
    local label_old="$3"
    local cmd_new="$4"
    local cmd_old="$5"
    local ai_new="$6"
    local ai_old="$7"
    local enabled_new="$8"
    local enabled_old="$9"
    local default_new="${10:-}"
    local default_old="${11:-}"

    local -n args_ref="$out_var"
    _tools_append_edit_arg_if_changed args_ref --label "$label_new" "$label_old"
    _tools_append_edit_arg_if_changed args_ref --cmd "$cmd_new" "$cmd_old"
    _tools_append_edit_arg_if_changed args_ref --ai "$ai_new" "$ai_old"
    _tools_append_edit_arg_if_changed args_ref --enable "$enabled_new" "$enabled_old"
    _tools_append_edit_arg_if_changed args_ref --default "$default_new" "$default_old"
}

_tools_manage_echo_field() {
    local field="$1"
    local value="$2"
    interactive_echo_field "$field" "$value"
}

_tools_manage_echo_optional_value() {
    local field="$1"
    local value="${2:-}"
    _tools_manage_echo_field "$field" "${value:-$(_i18n TOOLS_BOOL_SKIP)}"
}

_tools_manage_continue_or_interrupt() {
    local rc="${1:-1}"
    [[ $rc -eq 130 ]] && return 130
    echo ""
    return 0
}

_tools_manage_continue_after_prompt_failure() {
    local rc="${1:-1}"
    _tools_manage_continue_or_interrupt "$rc" || return 130
    return 0
}

_tools_manage_return_interrupt_or_error() {
    local rc="${1:-1}"
    [[ $rc -eq 130 ]] && return 130
    return 1
}

_tools_manage_resolve_existing_name_or_continue() {
    local prompt="$1"
    local out_var="$2"
    _tools_manage_prompt_existing_name "$prompt" "$out_var" && return 0
    local rc=$?
    _tools_manage_continue_or_interrupt "$rc" || return 130
    return 2
}

_tools_manage_run_existing_name_action() {
    local prompt="$1"
    local action_fn="$2"
    local selected_name
    if ! _tools_manage_resolve_existing_name_or_continue "$prompt" selected_name; then
        local rc=$?
        _tools_manage_continue_on_rc2_or_return "$rc"
        rc=$?
        return "$rc"
    fi
    "$action_fn" "$selected_name"
    echo ""
    return 0
}

_tools_manage_continue_on_rc2_or_return() {
    local rc="${1:-1}"
    [[ $rc -eq 2 ]] && return 2
    return "$rc"
}

_tools_manage_finalize_existing_name_case() {
    local rc="${1:-0}"
    # rc=2 means "user skipped this action"; treat as normal loop continuation.
    [[ $rc -eq 2 ]] && return 0
    [[ $rc -ne 0 ]] && return "$rc"
    return 0
}

_tools_manage_run_existing_name_menu_action() {
    local prompt="$1"
    local action_fn="$2"
    _tools_manage_run_existing_name_action "$prompt" "$action_fn"
    local rc=$?
    _tools_manage_continue_on_rc2_or_return "$rc"
    rc=$?
    _tools_manage_finalize_existing_name_case "$rc"
}

_tools_manage_read_current_tool_state() {
    local tool_name="$1"
    local label_out_var="$2"
    local cmd_out_var="$3"
    local ai_out_var="$4"
    local enabled_out_var="$5"
    local default_out_var="$6"
    local row="$7"

    local current_name current_id current_label current_cmd current_enabled current_ai current_default
    [[ -n "$row" ]] || return 1
    IFS=$'\t' read -r current_name current_id current_label current_cmd current_enabled current_ai current_default <<< "$row"
    [[ -z "$current_label" ]] && current_label="$tool_name"
    [[ -z "$current_ai" || "$current_ai" == "null" ]] && current_ai="false"
    [[ -z "$current_enabled" || "$current_enabled" == "null" ]] && current_enabled="true"
    [[ -z "$current_default" || "$current_default" == "null" ]] && current_default="false"

    printf -v "$label_out_var" '%s' "$current_label"
    printf -v "$cmd_out_var" '%s' "$current_cmd"
    printf -v "$ai_out_var" '%s' "$current_ai"
    printf -v "$enabled_out_var" '%s' "$current_enabled"
    printf -v "$default_out_var" '%s' "$current_default"
    return 0
}

_tools_manage_prompt_new_name() {
    local __var_name="$1"
    local val=""
    while true; do
        interactive_read_line "$(_i18n TOOLS_PROMPT_NAME)" val || {
            local rc=$?
            _tools_manage_return_interrupt_or_error "$rc"
            return $?
        }
        if _tools_validate_new_name "$val"; then
            printf -v "$__var_name" '%s' "$val"
            _tools_manage_echo_field "$(_i18n TOOLS_COL_TOOL_NAME)" "$val"
            return 0
        fi
    done
}

_tools_manage_prompt_existing_name() {
    local prompt="$1"
    local __var_name="$2"
    local val=""
    while true; do
        interactive_read_line "$prompt" val || {
            local rc=$?
            _tools_manage_return_interrupt_or_error "$rc"
            return $?
        }
        if [[ -z "$val" ]]; then
            log_error "$(_i18n TOOLS_ERR_MISSING_NAME)"
            continue
        fi
        if _tools_exists_by_name "$val"; then
            printf -v "$__var_name" '%s' "$val"
            _tools_manage_echo_field "$(_i18n TOOLS_COL_TOOL_NAME)" "$val"
            return 0
        fi
        _tools_log_name_not_found "$val"
    done
}

_tools_manage_prompt_required_command() {
    local prompt="$1"
    local __var_name="$2"
    local default_value="${3:-}"
    local val=""
    while true; do
        interactive_read_line "$prompt" val "$default_value" || {
            local rc=$?
            _tools_manage_return_interrupt_or_error "$rc"
            return $?
        }
        if [[ -n "${val//[[:space:]]/}" ]]; then
            printf -v "$__var_name" '%s' "$val"
            _tools_manage_echo_field "$(_i18n TOOLS_COL_COMMAND)" "$val"
            return 0
        fi
        log_error "$(_i18n TOOLS_ERR_ADD_REQUIRED)"
    done
}

_tools_manage_handle_add_action() {
    local n l c a d e
    _tools_manage_prompt_new_name n || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    interactive_read_line "$(_i18n_fmt TOOLS_PROMPT_LABEL_FMT "$n")" l || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    a=$(_tools_prompt_bool_tui "$(_i18n TOOLS_PROMPT_IS_AI_TUI)" "false")
    _tools_manage_echo_field "$(_i18n TOOLS_COL_AI)" "$a"
    d=$(_tools_prompt_bool_tui "$(_i18n TOOLS_PROMPT_DEFAULT_TUI)" "false")
    _tools_manage_echo_field "$(_i18n TOOLS_COL_DEFAULT)" "$d"
    e=$(_tools_prompt_bool_tui "$(_i18n TOOLS_PROMPT_ENABLED_TUI)" "true")
    _tools_manage_echo_field "$(_i18n TOOLS_PROMPT_ENABLED_TUI)" "$e"
    _tools_manage_prompt_required_command "$(_i18n TOOLS_PROMPT_COMMAND)" c || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    [[ -z "$l" ]] && l="$n"
    _tools_manage_echo_field "$(_i18n TOOLS_COL_LABEL)" "$l"
    tools_command_add --name "$n" --label "$l" --cmd "$c" --ai "$a" --default "$d" --enable "$e"
    echo ""
    return 0
}

_tools_manage_handle_edit_action() {
    local en
    if ! _tools_manage_resolve_existing_name_or_continue "$(_i18n TOOLS_PROMPT_EDIT_NAME)" en; then
        local rc=$?
        [[ $rc -eq 2 ]] && return 0
        return "$rc"
    fi

    local cfg idx row curr_label curr_cmd curr_ai curr_enabled curr_default
    _tools_require_cfg_index_and_row_by_name_into "$en" cfg idx row || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    _tools_manage_read_current_tool_state "$en" curr_label curr_cmd curr_ai curr_enabled curr_default "$row" || return 1

    local l2 c2 a2 e2 d2
    interactive_read_line "$(_i18n TOOLS_PROMPT_NEW_LABEL)" l2 "$curr_label" || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    _tools_manage_echo_field "$(_i18n TOOLS_COL_LABEL)" "$l2"
    _tools_manage_prompt_required_command "$(_i18n TOOLS_PROMPT_NEW_COMMAND)" c2 "$curr_cmd" || {
        local rc=$?
        _tools_manage_continue_after_prompt_failure "$rc" || return $?
        return 0
    }
    a2=$(_tools_prompt_bool_or_skip_tui "$(_i18n TOOLS_PROMPT_NEW_IS_AI_TUI)" "$curr_ai")
    _tools_manage_echo_optional_value "$(_i18n TOOLS_COL_AI)" "$a2"
    e2=$(_tools_prompt_bool_or_skip_tui "$(_i18n TOOLS_PROMPT_NEW_ENABLED_TUI)" "$curr_enabled")
    _tools_manage_echo_optional_value "$(_i18n TOOLS_PROMPT_ENABLED_TUI)" "$e2"
    d2=$(_tools_prompt_bool_or_skip_tui "$(_i18n TOOLS_PROMPT_SET_DEFAULT_TUI)" "$curr_default")
    _tools_manage_echo_optional_value "$(_i18n TOOLS_COL_DEFAULT)" "$d2"
    local args=()
    _tools_build_edit_args args "$l2" "$curr_label" "$c2" "$curr_cmd" "$a2" "$curr_ai" "$e2" "$curr_enabled" "$d2" "$curr_default"
    tools_command_edit "$en" "${args[@]}"
    echo ""
    return 0
}

_tools_expect_no_args_or_return() {
    local usage="$1"
    local command_name="$2"
    shift 2
    local validate_rc=0
    cli_expect_no_args "$usage" "$command_name" "$@" || validate_rc=$?
    if [[ $validate_rc -eq 2 ]]; then
        return 0
    fi
    [[ $validate_rc -ne 0 ]] && return "$validate_rc"
    return 0
}

_tools_invalidate_cache() {
    cache_invalidate "config:*"
    tools_registry_reset_cache
}

_tools_set_default_name() {
    local cfg="$1"
    local name="$2"
    config_atomic_yq_edit "$cfg" '.tools.default = strenv(TOOL_NAME)' "TOOL_NAME=$name"
}

_tools_clear_default_if_matches() {
    local cfg="$1"
    local name="$2"
    config_atomic_yq_edit "$cfg" 'if (.tools.default // "") == strenv(TOOL_NAME) then .tools.default = "" else . end' "TOOL_NAME=$name"
}

tools_command_list() {
    _tools_expect_no_args_or_return "$(_i18n TOOLS_USAGE_LIST)" "tools list" "$@" || return $?

    _tools_require_global_file || return 1

    local default_name default_display default_label
    default_name=$(tools_registry_get_default_ref 2>/dev/null || echo "")
    default_label="$(_i18n TOOLS_COL_DEFAULT)"
    case "$default_name" in
        "__empty__"|"") default_display="null ($(_i18n EMPTY_SHELL_OPT))" ;;
        "__custom__") default_display="custom ($(_i18n CUSTOM_INPUT))" ;;
        *) default_display="$default_name" ;;
    esac
    local rows
    rows=$(tools_registry_load 2>/dev/null || true)
    if [[ -z "$rows" ]]; then
        log_info "$(_i18n TOOLS_NO_CONFIGURED)"
        printf '%s: %s\n' "$default_label" "$default_display"
        return 0
    fi

    printf '%s: %s\n' "$default_label" "$default_display"

    local hdr_name hdr_label hdr_ai hdr_cmd
    hdr_name="$(_i18n TOOLS_COL_TOOL_NAME)"
    hdr_label="$(_i18n TOOLS_COL_LABEL)"
    hdr_ai="$(_i18n TOOLS_COL_AI)"
    hdr_cmd="$(_i18n TOOLS_COL_COMMAND)"

    local w_name w_label w_ai w_cmd
    w_name=$(_dw "$hdr_name")
    w_label=$(_dw "$hdr_label")
    w_ai=$(_dw "$hdr_ai")
    w_cmd=$(_dw "$hdr_cmd")

    local line name internal_id label cmd enabled is_ai is_default
    local d_name d_label d_ai
    local enabled_rows
    enabled_rows=""
    while IFS= read -r line; do
        [[ -n "$line" ]] || continue
        IFS=$'\t' read -r name internal_id label cmd enabled is_ai is_default <<< "$line"
        [[ "$enabled" == "false" ]] && continue
        if [[ -n "$enabled_rows" ]]; then
            enabled_rows+=$'\n'
        fi
        enabled_rows+="${name}|${label}|${cmd}|${is_ai}"
    done <<< "$rows"

    while IFS='|' read -r name label cmd is_ai; do
        d_name=$(_dw "$name")
        d_label=$(_dw "$label")
        d_ai=$(_dw "$is_ai")

        (( d_name > w_name )) && w_name=$d_name
        (( d_label > w_label )) && w_label=$d_label
        (( d_ai > w_ai )) && w_ai=$d_ai
    done <<< "$enabled_rows"

    printf '%s  %s  %s  %s\n' \
        "$(_wpad "$w_name" "$hdr_name")" \
        "$(_wpad "$w_label" "$hdr_label")" \
        "$(_wpad "$w_ai" "$hdr_ai")" \
        "$hdr_cmd"

    printf '%s  %s  %s  %s\n' \
        "$(_wpad "$w_name" "$(_tools_dash_fill "$w_name")")" \
        "$(_wpad "$w_label" "$(_tools_dash_fill "$w_label")")" \
        "$(_wpad "$w_ai" "$(_tools_dash_fill "$w_ai")")" \
        "$(_wpad "$w_cmd" "$(_tools_dash_fill "$w_cmd")")"

    while IFS='|' read -r name label cmd is_ai enabled; do
        printf '%s  %s  %s  %s\n' \
            "$(_wpad "$w_name" "$name")" \
            "$(_wpad "$w_label" "$label")" \
            "$(_wpad "$w_ai" "$is_ai")" \
            "$cmd"
    done <<< "$enabled_rows"
}

tools_command_add() {
    _tools_require_global_file || return 1
    local name="" label="" cmd="" is_ai="false" enabled="true" is_default="false"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --name) name="${2:-}"; shift 2 ;;
            --label) label="${2:-}"; shift 2 ;;
            --cmd) cmd="${2:-}"; shift 2 ;;
            --ai) is_ai="${2:-}"; shift 2 ;;
            --enable) enabled="${2:-}"; shift 2 ;;
            --default) is_default="${2:-}"; shift 2 ;;
            *) _tools_fail_unknown_opt "$1" || return 1 ;;
        esac
    done

    if [[ -z "$cmd" ]]; then
        log_error "$(_i18n TOOLS_ERR_ADD_REQUIRED)"
        return 1
    fi
    _tools_validate_new_name "$name" || return 1
    _tools_validate_add_bool_options "$is_ai" "$enabled" "$is_default" || return 1
    [[ -z "$label" ]] && label="$name"

    local cfg
    _tools_cfg_path_into cfg
    config_atomic_yq_edit "$cfg" '.tools.list += [{"tool_name": strenv(NAME), "label": strenv(LABEL), "command": strenv(CMD), "is_ai": (strenv(IS_AI) == "true"), "enabled": (strenv(ENABLED) == "true")} ]' \
        "NAME=$name" "LABEL=$label" "CMD=$cmd" "IS_AI=$is_ai" "ENABLED=$enabled" || return 1
    if [[ "$is_default" == "true" ]]; then
        _tools_set_default_name "$cfg" "$name" || return 1
    fi
    _tools_finalize_mutation_success TOOLS_MSG_ADDED_FMT "$name"
}

tools_command_edit() {
    _tools_require_global_file || return 1
    local name="${1:-}"
    shift || true
    _tools_require_name_arg "$name" || return 1

    local idx cfg
    _tools_require_cfg_index_and_row_by_name_into "$name" cfg idx || return 1
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --label) LABEL="${2:-}"; _tools_edit_set_string_field "$cfg" "$idx" "label" "$LABEL" "LABEL" || return 1; shift 2 ;;
            --cmd) CMD="${2:-}"; _tools_edit_set_string_field "$cfg" "$idx" "command" "$CMD" "CMD" || return 1; shift 2 ;;
            --ai)
                AI="${2:-false}"
                _tools_edit_set_bool_field "$cfg" "$idx" "is_ai" "--ai" "$AI" "AI" || return 1
                shift 2
                ;;
            --enable)
                EN="${2:-true}"
                _tools_edit_set_bool_field "$cfg" "$idx" "enabled" "--enable" "$EN" "EN" || return 1
                shift 2
                ;;
            --default)
                DF="${2:-false}"
                _tools_edit_apply_default_flag "$cfg" "$name" "$DF" || return 1
                shift 2
                ;;
            *) _tools_fail_unknown_opt "$1" || return 1 ;;
        esac
    done
    _tools_finalize_mutation_success TOOLS_MSG_UPDATED_FMT "$name"
}

tools_command_remove() {
    _tools_require_global_file || return 1
    local name="${1:-}"
    _tools_require_name_arg "$name" || return 1
    local cfg idx
    _tools_require_cfg_index_and_row_by_name_into "$name" cfg idx || return 1
    config_atomic_yq_edit "$cfg" "del(.tools.list[$idx])" || return 1
    _tools_clear_default_if_matches "$cfg" "$name" || return 1
    _tools_finalize_mutation_success TOOLS_MSG_REMOVED_FMT "$name"
}

tools_command_set_default() {
    _tools_require_global_file || return 1
    local name="${1:-}"
    _tools_require_name_arg "$name" || return 1
    name=$(_tools_normalize_default_name_ref "$name")
    _tools_require_default_target_exists "$name" || return 1
    local cfg
    _tools_cfg_path_into cfg
    _tools_set_default_name "$cfg" "$name" || return 1
    _tools_finalize_mutation_success TOOLS_MSG_DEFAULT_SET_FMT "$name"
}

tools_command_validate() {
    _tools_expect_no_args_or_return "$(_i18n TOOLS_USAGE_VALIDATE)" "tools validate" "$@" || return $?

    _tools_require_global_file || return 1
    tools_registry_validate || return 1
    tools_registry_validate_commands_available || return 1
    log_success "$(_i18n TOOLS_MSG_VALID)"
    return 0
}

tools_command_manage_help() {
    cat <<EOF
$(_i18n TOOLS_USAGE_MANAGE)

$(_i18n TOOLS_MANAGE_HELP_PURPOSE)

$(_i18n TOOLS_MANAGE_HELP_ACTIONS)
  - $(_i18n TOOLS_MENU_LIST)
  - $(_i18n TOOLS_MENU_ADD)
  - $(_i18n TOOLS_MENU_EDIT)
  - $(_i18n TOOLS_MENU_REMOVE)
  - $(_i18n TOOLS_MENU_SET_DEFAULT)
  - $(_i18n TOOLS_MENU_VALIDATE)
  - $(_i18n TOOLS_MENU_DONE)

$(_i18n TOOLS_MANAGE_HELP_NOTE)
EOF
}

tools_command_manage() {
    if cli_is_help_flag "${1:-}"; then
        tools_command_manage_help
        return 0
    fi

    _tools_expect_no_args_or_return "$(_i18n TOOLS_USAGE_MANAGE)" "tools manage" "$@" || return $?

    _tools_require_global_file || return 1
    if ! [ -t 0 ] || ! [ -t 1 ]; then
        log_error "$(_i18n TOOLS_ERR_TTY_REQUIRED)"
        log_info "$(_i18n TOOLS_HINT_NON_INTERACTIVE)"
        return 1
    fi
    while true; do
        # shellcheck disable=SC2034
        local menu=(
            "$(_i18n TOOLS_MENU_LIST)"
            "$(_i18n TOOLS_MENU_ADD)"
            "$(_i18n TOOLS_MENU_EDIT)"
            "$(_i18n TOOLS_MENU_REMOVE)"
            "$(_i18n TOOLS_MENU_SET_DEFAULT)"
            "$(_i18n TOOLS_MENU_VALIDATE)"
            "$(_i18n TOOLS_MENU_DONE)"
        )
        tui_select_menu "$(_i18n TOOLS_MENU_TITLE)" menu 0
        case "${TUI_SELECTED_INDEX:-0}" in
            0) tools_command_list; echo "" ;;
            1) _tools_manage_handle_add_action || return $? ;;
            2) _tools_manage_handle_edit_action || return $? ;;
            3)
                _tools_manage_run_existing_name_menu_action "$(_i18n TOOLS_PROMPT_REMOVE_NAME)" "tools_command_remove" || return $?
                ;;
            4)
                _tools_manage_run_existing_name_menu_action "$(_i18n TOOLS_PROMPT_DEFAULT_NAME)" "tools_command_set_default" || return $?
                ;;
            5) tools_command_validate; echo "" ;;
            *) return 0 ;;
        esac
    done
}

tools_command_execute() {
    local sub="${1:-help}"
    case "$sub" in
        -h|--help|help) tools_command_help ;;
        list) shift; tools_command_list "$@" ;;
        add) shift; tools_command_add "$@" ;;
        edit) shift; tools_command_edit "$@" ;;
        remove|rm) shift; tools_command_remove "$@" ;;
        set-default) shift; tools_command_set_default "$@" ;;
        validate) shift; tools_command_validate "$@" ;;
        manage) shift; tools_command_manage "$@" ;;
        *)
            log_error "$(_i18n TOOLS_ERR_UNKNOWN_SUBCMD) $sub"
            echo ""
            tools_command_help
            return 1
            ;;
    esac
}

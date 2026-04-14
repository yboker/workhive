#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Init Command
# Interactive initialization configuration (App layer)
#
# All templates generate unified Column-based DSL config.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_INIT_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _INIT_COMMAND_SH_LOADED=1

source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/platform/detector.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/app/shared/theme_helper.sh"
source "${LIB_DIR}/infra/persistence/manager.sh"
source "${LIB_DIR}/app/shared/command_builder.sh"
source "${LIB_DIR}/app/shared/layout_visual.sh"

# ──────────────────────────────────────────────────
# Template registry (data-driven, easy to extend)
#
# Each template defines a *preset* columns layout.
# Fields:
#   _TPL_<ID>_NAME       Display name
#   _TPL_<ID>_DESC       Short description
#   _TPL_<ID>_COLS       Number of columns
#   _TPL_<ID>_PANES      Pane counts per column (',' separated, e.g. "2,1")
#   _TPL_<ID>_WIDTHS     Column widths (',' separated, e.g. "60,40")
#   _TPL_<ID>_HEIGHTS    Pane heights per column ('|' separated groups, ',' within, e.g. "50,50|100")
#   _TPL_<ID>_CMDS       Default commands ('|' separated in reading order)
#   _TPL_<ID>_DIAGRAM    ASCII layout diagram
# ──────────────────────────────────────────────────

_TPL_IDS=("ai-coding-left" "ai-coding-right" "ai-coding-half" "quad" "custom")

# --- Template 1: AI Coding (Left) ---
_TPL_ai_coding_left_NAME="AI Coding (Left)"
_TPL_ai_coding_left_DESC="Claude Code on left + Lazygit + Shell"
_TPL_ai_coding_left_COLS=2
_TPL_ai_coding_left_PANES="2,1"
_TPL_ai_coding_left_WIDTHS="60,40"
_TPL_ai_coding_left_HEIGHTS="50,50|100"
_TPL_ai_coding_left_CMDS="__default__|null|lazygit"
_TPL_ai_coding_left_DIAGRAM='     ┌────────────────┬───────────┐
     │ Claude Code    │           │
     │ (60%, 50%)     │  Lazygit  │
     ├────────────────┤  (40%)    │
     │ Shell          │           │
     │ (60%, 50%)     │           │
     └────────────────┴───────────┘'

# --- Template 2: AI Coding (Right) ---
_TPL_ai_coding_right_NAME="AI Coding (Right)"
_TPL_ai_coding_right_DESC="Lazygit on left + Claude Code + Shell"
_TPL_ai_coding_right_COLS=2
_TPL_ai_coding_right_PANES="1,2"
_TPL_ai_coding_right_WIDTHS="40,60"
_TPL_ai_coding_right_HEIGHTS="100|50,50"
_TPL_ai_coding_right_CMDS="lazygit|__default__|null"
_TPL_ai_coding_right_DIAGRAM='     ┌──────────┬────────────────┐
     │          │ Claude Code    │
     │ Lazygit  │ (60%, 50%)     │
     │ (40%)    ├────────────────┤
     │          │ Shell          │
     │          │ (60%, 50%)     │
     └──────────┴────────────────┘'

# --- Template 3: AI Coding (Half) ---
_TPL_ai_coding_half_NAME="AI Coding (Half)"
_TPL_ai_coding_half_DESC="Claude Code + Lazygit — simple 50/50 split"
_TPL_ai_coding_half_COLS=2
_TPL_ai_coding_half_PANES="1,1"
_TPL_ai_coding_half_WIDTHS="50,50"
_TPL_ai_coding_half_HEIGHTS="100|100"
_TPL_ai_coding_half_CMDS="__default__|lazygit"
_TPL_ai_coding_half_DIAGRAM='     ┌────────────────┬───────────────┐
     │                │               │
     │  Claude Code   │   Lazygit     │
     │  (50%)         │   (50%)       │
     │                │               │
     │                │               │
     └────────────────┴───────────────┘'

# --- Template 4: Quad (four panes) ---
_TPL_quad_NAME="Quad"
_TPL_quad_DESC="Four panes — Claude Code + Lazygit + Shell + Yazi"
_TPL_quad_COLS=2
_TPL_quad_PANES="2,2"
_TPL_quad_WIDTHS="50,50"
_TPL_quad_HEIGHTS="50,50|50,50"
_TPL_quad_CMDS="__default__|null|lazygit|yazi"
_TPL_quad_DIAGRAM='     ┌──────────┬──────────┐
     │ Claude   │ Lazygit  │
     │ Code     │          │
     │ (50%)    │ (50%)    │
     ├──────────┼──────────┤
     │ Shell    │ Yazi     │
     │ (50%)    │ (50%)    │
     └──────────┴──────────┘'

# --- Template 5: Custom (user-defined columns layout) ---
_TPL_custom_NAME="Custom"
_TPL_custom_DESC="Custom columns — define columns and panes freely"
_TPL_custom_COLS=0
_TPL_custom_PANES=""
_TPL_custom_WIDTHS=""
_TPL_custom_HEIGHTS=""
_TPL_custom_CMDS=""
_TPL_custom_DIAGRAM=""

_tpl_custom_diagram() {
    cat <<'EODIAG'
     ┌──────┬──────┬──────┐
     │ C1P1 │ C2P1 │      │
     │      ├──────┤ C3P1 │
     │ C1P2 │ C2P2 │      │
     └──────┴──────┴──────┘
EODIAG
    echo "     $(_i18n CUSTOM_EACH_COL_INDEPENDENT)"
}

# ──────────────────────────────────────────────────
# Available command registry
# ──────────────────────────────────────────────────

_init_build_available_commands() {
    # Tools manager may have modified global tools config in the same init run.
    # Reset registry caches so latest default tool is reflected immediately.
    if declare -f tools_registry_reset_cache >/dev/null 2>&1; then
        tools_registry_reset_cache
    fi
    _build_pane_command_list
}

_init_is_template_command_available() {
    local target_cmd="$1"
    if ! declare -p _AVAILABLE_CMD_VALUES &>/dev/null; then
        return 2
    fi

    local available_count=${#_AVAILABLE_CMD_VALUES[@]}
    if (( available_count == 0 )); then
        return 2
    fi

    local candidate=""
    local has_registry_values=false
    local i
    for i in "${!_AVAILABLE_CMD_VALUES[@]}"; do
        candidate="${_AVAILABLE_CMD_VALUES[$i]}"
        [[ "$candidate" == "__empty__" || "$candidate" == "__custom__" ]] && continue
        has_registry_values=true
        if [[ "$candidate" == "$target_cmd" ]]; then
            return 0
        fi
    done

    [[ "$has_registry_values" == true ]] && return 1
    return 2
}

_init_resolve_template_commands() {
    local pane_cmds="$1"
    IFS='|' read -ra cmds <<< "$pane_cmds"
    local resolved=()

    local cmd
    for cmd in "${cmds[@]}"; do
        if [[ "$cmd" == "__default__" ]]; then
            if ! declare -p _AVAILABLE_CMD_VALUES &>/dev/null; then
                _init_build_available_commands
            fi
            resolved+=("$(_pane_command_get_default_command)")
            continue
        fi
        if _is_null_or_empty "$cmd"; then
            resolved+=("null")
            continue
        fi
        if _init_is_template_command_available "$cmd"; then
            resolved+=("$cmd")
        else
            local check_rc=$?
            if [[ "$check_rc" == "2" ]]; then
                local bin_name="${cmd%% *}"
                if command -v "$bin_name" &>/dev/null; then
                    resolved+=("$cmd")
                else
                    resolved+=("null")
                fi
            else
                resolved+=("null")
            fi
        fi
    done

    local IFS='|'
    echo "${resolved[*]}"
}

_init_reconcile_template_defaults_with_tools() {
    local idx
    local cmd

    tools_registry_reset_cache
    _init_build_available_commands

    for idx in "${!_init_pane_cmds[@]}"; do
        [[ "${_init_pane_cmd_sources[$idx]:-}" != "template" ]] && continue
        cmd="${_init_pane_cmds[$idx]:-null}"
        _is_null_or_empty "$cmd" && continue
        if ! _init_is_template_command_available "$cmd"; then
            _init_pane_cmds[$idx]="null"
        fi
    done
}

_init_apply_tools_default_to_template_commands() {
    local pane_cmds="$1"
    local default_ref=""
    default_ref=$(tools_registry_get_default_ref 2>/dev/null || echo "")
    if [[ "$default_ref" != "__empty__" ]]; then
        echo "$pane_cmds"
        return 0
    fi

    local -a cmds=()
    local -a resolved=()
    IFS='|' read -ra cmds <<< "$pane_cmds"

    local cmd
    for cmd in "${cmds[@]}"; do
        if _is_null_or_empty "$cmd"; then
            resolved+=("null")
        else
            resolved+=("null")
        fi
    done

    local IFS='|'
    echo "${resolved[*]}"
}

_init_write_layout_commands_to_file() {
    local cfg_file="$1"
    if ! check_cmd yq; then
        return 0
    fi

    local cmd_idx=0
    local ci pi
    for ((ci=0; ci<_init_col_count; ci++)); do
        local pc=${_init_pane_counts[$ci]}
        for ((pi=0; pi<pc; pi++)); do
            local p_cmd="${_init_pane_cmds[$cmd_idx]:-null}"
            if _is_null_or_empty "$p_cmd"; then
                config_atomic_yq_edit "$cfg_file" ".layout.columns[$ci].panes[$pi].command = null" || return 1
            else
                config_atomic_yq_edit "$cfg_file" ".layout.columns[$ci].panes[$pi].command = strenv(CMD)" "CMD=$p_cmd" || return 1
            fi
            cmd_idx=$((cmd_idx + 1))
        done
    done
}

_init_capture_global_tools_yaml() {
    local cfg_file="$1"
    if ! check_cmd yq; then
        return 1
    fi
    yq '.tools // {}' "$cfg_file" 2>/dev/null || return 1
    return 0
}

_init_restore_global_tools_yaml() {
    local cfg_file="$1"
    local tools_yaml="$2"
    if ! check_cmd yq; then
        return 1
    fi
    [[ -n "$tools_yaml" ]] || return 1
    config_atomic_yq_edit "$cfg_file" '.tools = (strenv(TOOLS_YAML) | from_yaml)' "TOOLS_YAML=$tools_yaml" 2>/dev/null || return 1
    return 0
}

_init_apply_persistence_settings() {
    local scope="${1:-project}" # project|global
    local cfg_file
    if [[ "$scope" == "global" ]]; then
        cfg_file=$(config_get_global_file)
    else
        cfg_file=$(config_get_project_file)
    fi

    config_atomic_yq_edit "$cfg_file" '
        .tmux.persistence.enabled = true |
        .tmux.persistence.generation = (.tmux.persistence.generation // 0) |
        .tmux.persistence.auto_save_mode = strenv(PERSIST_MODE) |
        .tmux.persistence.auto_save_debounce = strenv(PERSIST_DEBOUNCE) |
        .tmux.persistence.auto_save_interval = strenv(PERSIST_INTERVAL) |
        .tmux.persistence.restore_processes = (strenv(PERSIST_RESTORE_PROCESSES) == "true") |
        .tmux.persistence.snapshot_history_limit = (strenv(PERSIST_HISTORY_LIMIT) | tonumber) |
        .tmux.persistence.snapshot_max_size_mb = (strenv(PERSIST_MAX_SIZE_MB) | tonumber)
    ' \
        "PERSIST_MODE=$_PERSIST_DEFAULT_AUTO_SAVE_MODE" \
        "PERSIST_DEBOUNCE=$_PERSIST_DEFAULT_AUTO_SAVE_DEBOUNCE" \
        "PERSIST_INTERVAL=$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL" \
        "PERSIST_RESTORE_PROCESSES=$_PERSIST_DEFAULT_RESTORE_PROCESSES" \
        "PERSIST_HISTORY_LIMIT=$_PERSIST_DEFAULT_SNAPSHOT_HISTORY_LIMIT" \
        "PERSIST_MAX_SIZE_MB=$_PERSIST_DEFAULT_SNAPSHOT_MAX_SIZE_MB" || return 1
}

_init_columns_pane_label() {
    local col_index="$1"
    local pane_index="$2"
    _i18n_fmt COLUMNS_PANE_LABEL "$col_index" "$pane_index"
}

# ──────────────────────────────────────────────────
# Help information
# ──────────────────────────────────────────────────

init_command_help() {
    _help_dispatch "_init_command_help"
}

# ──────────────────────────────────────────────────
# Template helper functions
# ──────────────────────────────────────────────────

_tpl_get() {
    local id="$1"
    local prop="$2"
    local var_name="_TPL_${id//-/_}_${prop}"
    echo "${!var_name}"
}

# ──────────────────────────────────────────────────
# Global configuration initialization
# ──────────────────────────────────────────────────

_init_global_config() {
    local force="${1:-false}"
    local global_dir
    global_dir=$(config_get_global_dir)
    local global_file
    global_file=$(config_get_global_file)

    if [[ -f "$global_file" ]] && [[ "$force" != true ]]; then
        log_warn "$(_i18n INFO_CONFIG_EXISTS) $global_file"
        log_info "$(_i18n INFO_USE_FORCE_REINIT)"
        return 1
    fi

    mkdir -p "$global_dir" 2>/dev/null || true
    _init_config_tx_begin "$global_file" "global" "defaults" || return 1
    local stage_file
    stage_file=$(config_get_global_file)
    config_ensure_scope_fields "$stage_file" "global"
    if ! _init_config_tx_commit; then
        return 1
    fi

    log_success "$(_i18n INFO_CONFIG_CREATED) $global_file"
    log_info "$(_i18n INFO_GLOBAL_CONFIG_PRIORITY)"
    return 0
}

_INIT_CONFIG_TX_STAGE_FILE=""
_INIT_CONFIG_TX_FINAL_FILE=""
_INIT_CONFIG_TX_SCOPE=""
_INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE=""
_INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE=""
_INIT_CONFIG_TX_COMPLETED=false

_init_config_tx_begin() {
    local final_file="$1"
    local scope="${2:-project}" # project|global
    local seed_mode="${3:-existing}" # existing|defaults

    local tx_dir
    tx_dir=$(dirname "$final_file")
    mkdir -p "$tx_dir" 2>/dev/null || true
    config_cleanup_stale_temp_files "$tx_dir"

    local tx_stage
    tx_stage=$(mktemp "${tx_dir}/config.yaml.stage.XXXXXX" 2>/dev/null || true)
    if [[ -z "$tx_stage" ]]; then
        log_error "Failed to create config transaction stage file in: $tx_dir"
        return 1
    fi
    _INIT_CONFIG_TX_STAGE_FILE="$tx_stage"
    _INIT_CONFIG_TX_FINAL_FILE="$final_file"
    _INIT_CONFIG_TX_SCOPE="$scope"
    _INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE="${WORKHIVE_GLOBAL_CONFIG_FILE:-}"
    _INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE="${WORKHIVE_PROJECT_CONFIG_FILE:-}"
    _INIT_CONFIG_TX_COMPLETED=false
    # Keep cleanup on EXIT/ERR only: trapping INT/TERM here swallows Ctrl+C
    # during interactive prompts and lets init continue unexpectedly.
    trap '_init_config_tx_rollback' EXIT ERR

    if [[ "$seed_mode" == "defaults" ]]; then
        config_get_defaults > "$tx_stage"
    elif [[ -f "$final_file" ]]; then
        cp -f -- "$final_file" "$tx_stage"
    else
        config_get_defaults > "$tx_stage"
    fi
    config_ensure_scope_fields "$tx_stage" "$scope" || {
        cleanup_file_quiet "$tx_stage"
        return 1
    }

    if [[ "$scope" == "global" ]]; then
        if ! config_global_init_lock_acquire; then
            log_error "Global initialization is already in progress."
            cleanup_file_quiet "$tx_stage"
            return 1
        fi
        export WORKHIVE_GLOBAL_CONFIG_FILE="$tx_stage"
    else
        export WORKHIVE_PROJECT_CONFIG_FILE="$tx_stage"
    fi
}

_init_config_tx_rollback() {
    if [[ "$_INIT_CONFIG_TX_COMPLETED" == "true" ]]; then
        return 0
    fi

    [[ -n "$_INIT_CONFIG_TX_STAGE_FILE" && -f "$_INIT_CONFIG_TX_STAGE_FILE" ]] && cleanup_file_quiet "$_INIT_CONFIG_TX_STAGE_FILE"

    if [[ -n "$_INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE" ]]; then
        export WORKHIVE_GLOBAL_CONFIG_FILE="$_INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE"
    else
        unset WORKHIVE_GLOBAL_CONFIG_FILE
    fi

    if [[ -n "$_INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE" ]]; then
        export WORKHIVE_PROJECT_CONFIG_FILE="$_INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE"
    else
        unset WORKHIVE_PROJECT_CONFIG_FILE
    fi

    if [[ "$_INIT_CONFIG_TX_SCOPE" == "global" ]]; then
        config_global_init_lock_release
    fi
}

_init_config_tx_commit() {
    if ! mv -f -- "$_INIT_CONFIG_TX_STAGE_FILE" "$_INIT_CONFIG_TX_FINAL_FILE"; then
        trap - EXIT ERR
        _init_config_tx_rollback
        log_error "Failed to finalize config: $_INIT_CONFIG_TX_FINAL_FILE"
        return 1
    fi

    _INIT_CONFIG_TX_COMPLETED=true
    trap - EXIT ERR

    if [[ -n "$_INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE" ]]; then
        export WORKHIVE_GLOBAL_CONFIG_FILE="$_INIT_CONFIG_TX_PREV_GLOBAL_OVERRIDE"
    else
        unset WORKHIVE_GLOBAL_CONFIG_FILE
    fi
    if [[ -n "$_INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE" ]]; then
        export WORKHIVE_PROJECT_CONFIG_FILE="$_INIT_CONFIG_TX_PREV_PROJECT_OVERRIDE"
    else
        unset WORKHIVE_PROJECT_CONFIG_FILE
    fi
    if [[ "$_INIT_CONFIG_TX_SCOPE" == "global" ]]; then
        config_global_init_lock_release
    fi
    return 0
}

# ──────────────────────────────────────────────────
# Main execution function
# ──────────────────────────────────────────────────

init_command_execute() {
    local interactive=true
    local force=false
    local global_mode=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                init_command_help
                return 0
                ;;
            --non-interactive)
                interactive=false
                shift
                ;;
            --force|-f)
                force=true
                shift
                ;;
            --global)
                global_mode=true
                shift
                ;;
            --debug)
                export WORKHIVE_DEBUG=1
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                init_command_help
                return 1
                ;;
            *)
                cli_log_no_positional "init"
                echo ""
                init_command_help
                return 1
                ;;
        esac
    done

    # Language selection
    if [[ "$interactive" == true ]]; then
        local _has_existing_config=false
        if [[ "$global_mode" == true ]]; then
            local _gf; _gf=$(config_get_global_file)
            [[ -f "$_gf" ]] && _has_existing_config=true
        else
            config_project_exists 2>/dev/null && _has_existing_config=true
        fi

        if [[ "$force" == true ]] || [[ "$_has_existing_config" == false ]]; then
            i18n_ask_language --force
        else
            i18n_ask_language
        fi
        echo ""
    else
        [[ -z "$WORKHIVE_LANG" ]] && WORKHIVE_LANG="en"
    fi

    # Git init guard (project mode) — uses unified path guard and shared helpers
    if [[ "$global_mode" != true ]]; then
        local _check_dir
        _check_dir=$(git rev-parse --show-toplevel 2>/dev/null || pwd)

        if ! git_is_inside_work_tree; then
            if ! _is_git_init_allowed "$_check_dir"; then
                return 1
            fi
            if ! _git_init_guarded "$interactive"; then
                return 1
            fi
        elif ! git rev-parse HEAD &>/dev/null; then
            if ! _git_ensure_initial_commit "$interactive"; then
                return 1
            fi
        fi
    fi

    local _global_tools_preexisting=false
    local _global_final_file=""
    local _project_final_file=""
    # Global mode interactive flow
    if [[ "$global_mode" == true ]]; then
        if [[ "$interactive" == false ]]; then
            _init_global_config "$force"
            return $?
        fi

        _global_final_file=$(config_get_global_file)
        if [[ "$force" != true ]] && [[ -f "$_global_final_file" ]] && check_cmd yq; then
            local _tools_count
            _tools_count=$(yq '.tools | length' "$_global_final_file" 2>/dev/null || echo "0")
            [[ "$_tools_count" -gt 0 ]] && _global_tools_preexisting=true
        fi

        if [[ -f "$_global_final_file" ]] && [[ "$force" != true ]]; then
            log_warn "$(_i18n INFO_CONFIG_EXISTS) $_global_final_file"
            echo ""
            local _overwrite_rc=0
            if ask_yes_no "$(_i18n OVERWRITE_CONFIG)" "N"; then
                _overwrite_rc=0
            else
                _overwrite_rc=$?
            fi
            if [[ $_overwrite_rc -eq 130 ]]; then
                return 130
            fi
            if [[ $_overwrite_rc -ne 0 ]]; then
                log_info "$(_i18n INIT_CANCELLED)"
                return 0
            fi
            echo ""
        fi
    else
        _project_final_file="$(config_get_project_file)"
    fi

    log_bold "🚀 $(_i18n INIT_WIZARD)"
    echo ""

    # Check existing configuration (project mode)
    if [[ "$global_mode" != true ]] && config_project_exists && [[ "$force" == false ]]; then
        log_warn "$(_i18n CONFIG_EXISTS)"
        echo ""

        if [[ "$interactive" == true ]]; then
            local _overwrite_rc=0
            if ask_yes_no "$(_i18n OVERWRITE_CONFIG)" "N"; then
                _overwrite_rc=0
            else
                _overwrite_rc=$?
            fi
            if [[ $_overwrite_rc -eq 130 ]]; then
                return 130
            fi
            if [[ $_overwrite_rc -ne 0 ]]; then
                log_info "$(_i18n INIT_CANCELLED)"
                log_info "$(_i18n USE_CONFIG_CMD)"
                return 0
            fi
        else
            log_info "$(_i18n USE_FORCE)"
            return 1
        fi

        echo ""
    fi

    # Step 1: Dependency detection
    log_info "$(_i18n STEP_DEPS)"
    echo ""

    if ! init_command_check_dependencies; then
        echo ""
        log_error "$(_i18n DEPS_MISSING)"
        return 1
    fi

    echo ""

    # Step 2: Select workflow template
    # Columns layout definition (populated by template or custom wizard)
    local _init_col_count=0
    local _init_col_widths=()     # width per column
    local _init_pane_counts=()    # panes per column
    local _init_pane_heights=()   # flat: height per pane
    local _init_pane_cmds=()      # flat: command per pane
    local _init_pane_cmd_sources=() # flat: template|user per pane
    local _global_tools_snapshot_yaml=""
    if [[ "$global_mode" != true ]]; then
        _init_config_tx_begin "$_project_final_file" "project" || return 1
    fi

    # Global interactive init: run tools setup BEFORE template selection.
    if [[ "$interactive" == true ]] && [[ "$global_mode" == true ]]; then
        _init_config_tx_begin "$_global_final_file" "global" || return 1

        local _global_tools_file
        _global_tools_file=$(config_get_global_file)

        if [[ "$_global_tools_preexisting" != "true" ]]; then
            echo ""
            log_info "$(_i18n INFO_GLOBAL_TOOLS_TEMPLATE_INITIALIZED)"
            log_info "$(_i18n INFO_TOOLS_TEMPLATE_SCOPE)"
            local _tools_prompt_rc=0
            if ask_yes_no "$(_i18n ASK_USE_DEFAULT_TOOLS_TEMPLATE)" "Y"; then
                _tools_prompt_rc=0
            else
                _tools_prompt_rc=$?
            fi
            if [[ $_tools_prompt_rc -eq 130 ]]; then
                return 130
            fi
            if [[ $_tools_prompt_rc -ne 0 ]]; then
                echo ""
                log_info "$(_i18n INFO_OPENING_TOOLS_MANAGER)"
                source "${LIB_DIR}/app/commands/tools_command.sh"
                tools_command_execute manage || true
            fi
        fi

        if [[ "$force" != true ]] && check_cmd yq; then
            _global_tools_snapshot_yaml=$(_init_capture_global_tools_yaml "$_global_tools_file" 2>/dev/null || echo "")
        fi
    fi

    # Step 2: Select workflow template
    log_info "$(_i18n STEP_TEMPLATE)"
    echo ""

    if [[ "$interactive" == true ]]; then
        _init_build_available_commands

        local tpl_id
        tpl_id=$(_init_ask_template)
        local _init_defaults_cfg=""
        if [[ "$global_mode" == true ]]; then
            _init_defaults_cfg=$(config_get_global_file)
        else
            _init_defaults_cfg="$(_workhive_dir)/config.yaml"
        fi

        local tpl_cols
        tpl_cols=$(_tpl_get "$tpl_id" "COLS")

        if [[ "$tpl_cols" == "0" || -z "$tpl_cols" ]]; then
            # Custom mode: user defines columns interactively
            _init_ask_columns_layout "$_init_defaults_cfg" || return $?

            _init_col_count=$_COLUMNS_COL_COUNT

            local c
            for ((c=0; c<_init_col_count; c++)); do
                _init_col_widths+=($((100 / _init_col_count)))
                _init_pane_counts+=("${_COLUMNS_PANE_COUNTS[$c]}")
                local pc=${_COLUMNS_PANE_COUNTS[$c]}
                local p
                for ((p=0; p<pc; p++)); do
                    _init_pane_heights+=($((100 / pc)))
                done
            done

            {
                echo ""
                log_success "$(_i18n SELECTED): $(_tpl_get "$tpl_id" "NAME")"
                echo ""
                echo "  $(_i18n LAYOUT): $(_i18n_fmt INIT_COLUMNS_LAYOUT_SUMMARY_FMT "$_init_col_count")"
                echo ""
            } > /dev/tty

            # Ask commands for each pane
            local _custom_cmds
            _custom_cmds=$(_init_ask_columns_pane_commands "$_init_defaults_cfg")
            IFS='|' read -ra _init_pane_cmds <<< "$_custom_cmds"
            _init_pane_cmd_sources=()
            local _custom_idx
            for _custom_idx in "${!_init_pane_cmds[@]}"; do
                _init_pane_cmd_sources+=("user")
            done
        else
            # Preset template: parse template definition into column arrays
            _init_col_count=$tpl_cols

            IFS=',' read -ra _init_pane_counts <<< "$(_tpl_get "$tpl_id" "PANES")"
            IFS=',' read -ra _init_col_widths <<< "$(_tpl_get "$tpl_id" "WIDTHS")"

            local heights_str
            heights_str=$(_tpl_get "$tpl_id" "HEIGHTS")
            IFS='|' read -ra _height_groups <<< "$heights_str"
            local g
            for g in "${_height_groups[@]}"; do
                IFS=',' read -ra _h_vals <<< "$g"
                local h
                for h in "${_h_vals[@]}"; do
                    _init_pane_heights+=("$h")
                done
            done

            local tpl_cmds
            tpl_cmds=$(_tpl_get "$tpl_id" "CMDS")
            tpl_cmds=$(_init_template_resolved_commands "$tpl_id")
            IFS='|' read -ra _init_pane_cmds <<< "$tpl_cmds"
            _init_pane_cmd_sources=()
            local _tpl_idx
            for _tpl_idx in "${!_init_pane_cmds[@]}"; do
                _init_pane_cmd_sources+=("template")
            done

            {
                echo ""
                log_success "$(_i18n SELECTED): $(_tpl_get "$tpl_id" "NAME")"
                echo ""
                echo "$(_init_template_dynamic_diagram "$tpl_id")"
                echo ""

                local pane_idx=0
                local ci pi
                for ((ci=0; ci<_init_col_count; ci++)); do
                    local pc=${_init_pane_counts[$ci]}
                    for ((pi=0; pi<pc; pi++)); do
                        local label
                        label=$(_init_columns_pane_label "$((ci+1))" "$((pi+1))")
                        local display_cmd="${_init_pane_cmds[$pane_idx]}"
                        _is_null_or_empty "$display_cmd" && display_cmd="$(_i18n EMPTY_SHELL)"
                        echo "  $label: $display_cmd"
                        pane_idx=$((pane_idx + 1))
                    done
                done
                echo ""
            } > /dev/tty

            local _fine_tune_rc=0
            if ask_yes_no "$(_i18n FINE_TUNE)" "N"; then
                _fine_tune_rc=0
            else
                _fine_tune_rc=$?
            fi
            if [[ $_fine_tune_rc -eq 130 ]]; then
                return 130
            fi
            if [[ $_fine_tune_rc -eq 0 ]]; then
                echo ""
                local _tuned_cmds
                _tuned_cmds=$(_init_ask_pane_commands_with_defaults)
                IFS='|' read -ra _init_pane_cmds <<< "$_tuned_cmds"
                _init_pane_cmd_sources=()
                local _tuned_idx
                for _tuned_idx in "${!_init_pane_cmds[@]}"; do
                    _init_pane_cmd_sources+=("user")
                done
            fi
        fi
    else
        log_info "$(_i18n DEFAULT_TEMPLATE)"
        # Default: 2 columns (60/40), col1: 2 panes, col2: 1 pane
        _init_col_count=2
        _init_col_widths=(60 40)
        _init_pane_counts=(2 1)
        _init_pane_heights=(50 50 100)
        _init_pane_cmds=("null" "null" "null")
        _init_pane_cmd_sources=("template" "template" "template")
    fi

    echo ""

    if [[ "$interactive" == true ]]; then
        log_info "$(_i18n PREVIEW)"
        echo ""
        layout_preview_columns_runtime "$_init_col_count" "_init_col_widths" "_init_pane_counts" "_init_pane_cmds" > /dev/tty
    fi

    # Step 3: tmux theme
    log_info "$(_i18n STEP_THEME)"
    echo ""

    local theme="catppuccin-mocha"

    if [[ "$interactive" == true ]]; then
        theme=$(init_command_ask_theme)
    else
        log_info "$(_i18n DEFAULT_THEME): $theme"
    fi

    echo ""

    # Step 4: session persistence
    local persistence_enabled="false"
    log_info "$(_i18n STEP_TMUX_PERSISTENCE)"
    echo ""

    if [[ "$interactive" == true ]]; then
        {
            echo "  $(_i18n DESC_TMUX_PERSISTENCE)"
            echo ""
        } > /dev/tty
        local _persist_rc=0
        if ask_yes_no "$(_i18n ASK_ENABLE_PERSISTENCE)" "N"; then
            _persist_rc=0
        else
            _persist_rc=$?
        fi
        if [[ $_persist_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_persist_rc -eq 0 ]]; then
            persistence_enabled="true"
        fi
    else
        log_info "$(_i18n DEFAULT_PERSISTENCE): disabled"
    fi
    echo ""

    # Generate configuration file
    log_info "$(_i18n GENERATING_CONFIG)"
    echo ""

    if [[ "$global_mode" == true ]]; then
        local global_file
        global_file=$(config_get_global_file)
        _init_generate_config "$theme" "$global_file" "false" "true"
        if [[ -n "$_global_tools_snapshot_yaml" ]]; then
            _init_restore_global_tools_yaml "$global_file" "$_global_tools_snapshot_yaml" || true
        fi
        if [[ "$persistence_enabled" == "true" ]]; then
            _init_apply_persistence_settings "global"
        fi
        config_ensure_scope_fields "$global_file" "global"
        if ! _init_config_tx_commit; then
            return 1
        fi
        log_success "$(_i18n INFO_CONFIG_CREATED) $_global_final_file"
        log_info "$(_i18n INFO_GLOBAL_CONFIG_PRIORITY)"
    else
        _init_ensure_instance_id
        local project_file
        project_file=$(config_get_project_file)
        _init_generate_config "$theme" "$project_file" "false" "false"
        if [[ "$persistence_enabled" == "true" ]]; then
            _init_apply_persistence_settings "project"
        fi
        if ! _init_config_tx_commit; then
            return 1
        fi
        log_success "$(_i18n CONFIG_CREATED)"
    fi

    echo ""
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_success "$(_i18n INIT_COMPLETE)"
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    if [[ "$global_mode" != true ]]; then
        echo ""

        log_info "$(_i18n NEXT_STEPS)"
        echo "  • $(_i18n NEXT_OPEN)"
        echo "  • $(_i18n NEXT_WORKTREE)"
        echo "  • $(_i18n NEXT_LIST)"
        echo "  • $(_i18n NEXT_CONFIG)"
        echo ""
    fi
}

# ──────────────────────────────────────────────────
# Interactive functions
# ──────────────────────────────────────────────────

_init_ask_template() {
    local default_idx="${1:-0}"
    local options=()
    local tpl_id
    for tpl_id in "${_TPL_IDS[@]}"; do
        local name desc
        name=$(_tpl_get "$tpl_id" "NAME")
        desc=$(_init_template_dynamic_desc "$tpl_id")
        options+=("$name — $desc")
    done

    tui_select_menu "$(_i18n SELECT_TEMPLATE)" options "$default_idx" "_init_template_preview_callback" "$_TPL_PREVIEW_LINES"

    echo "${_TPL_IDS[$TUI_SELECTED_INDEX]}"
}

_detect_current_template() {
    local cfg_file="$1"
    local col_count
    col_count=$(yq '.layout.columns | length' "$cfg_file" 2>/dev/null || echo "0")
    ((col_count < 1)) && { echo 0; return; }

    local pane_counts=()
    local ci
    for ((ci=0; ci<col_count; ci++)); do
        local cp
        cp=$(yq ".layout.columns[$ci].panes | length" "$cfg_file" 2>/dev/null || echo "1")
        ((cp < 1)) && cp=1
        pane_counts+=("$cp")
    done

    local panes_str
    panes_str=$(IFS=','; echo "${pane_counts[*]}")

    local idx=0
    local tpl_id
    for tpl_id in "${_TPL_IDS[@]}"; do
        if [[ "$tpl_id" == "custom" ]]; then
            idx=$((idx + 1))
            continue
        fi
        local tpl_cols tpl_panes
        tpl_cols=$(_tpl_get "$tpl_id" "COLS")
        tpl_panes=$(_tpl_get "$tpl_id" "PANES")
        if [[ "$col_count" == "$tpl_cols" && "$panes_str" == "$tpl_panes" ]]; then
            echo "$idx"
            return
        fi
        idx=$((idx + 1))
    done

    # No match => custom (last item)
    echo "$((${#_TPL_IDS[@]} - 1))"
}

_TPL_PREVIEW_LINES=10

_init_command_display_label() {
    local cmd="${1:-null}"
    if _is_null_or_empty "$cmd"; then
        echo "Shell"
        return 0
    fi

    local cmd_bin="${cmd%% *}"
    case "$cmd_bin" in
        claude) echo "Claude Code" ;;
        lazygit) echo "Lazygit" ;;
        codex) echo "Codex" ;;
        opencode) echo "OpenCode" ;;
        yazi) echo "Yazi" ;;
        "") echo "Shell" ;;
        *) echo "$cmd_bin" ;;
    esac
}

_init_template_resolved_commands() {
    local tpl_id="$1"
    local tpl_cmds
    tpl_cmds=$(_tpl_get "$tpl_id" "CMDS")
    tpl_cmds=$(_init_resolve_template_commands "$tpl_cmds")
    _init_apply_tools_default_to_template_commands "$tpl_cmds"
}

_init_template_dynamic_desc() {
    local tpl_id="$1"
    if [[ "$tpl_id" == "custom" ]]; then
        echo "$(_i18n CUSTOM_TPL_DESC)"
        return 0
    fi

    local resolved
    resolved=$(_init_template_resolved_commands "$tpl_id")
    local -a _cmds=()
    IFS='|' read -ra _cmds <<< "$resolved"

    local l1 l2 l3 l4
    l1=$(_init_command_display_label "${_cmds[0]:-null}")
    l2=$(_init_command_display_label "${_cmds[1]:-null}")
    l3=$(_init_command_display_label "${_cmds[2]:-null}")
    l4=$(_init_command_display_label "${_cmds[3]:-null}")

    case "$tpl_id" in
        ai-coding-left) echo "$l1 on left + $l3 + $l2" ;;
        ai-coding-right) echo "$l1 on left + $l2 + $l3" ;;
        ai-coding-half) echo "$l1 + $l2 — simple 50/50 split" ;;
        quad) echo "Four panes — $l1 + $l3 + $l2 + $l4" ;;
        *) echo "$(_tpl_get "$tpl_id" "DESC")" ;;
    esac
}

_init_diagram_repeat() {
    local ch="$1"
    local count="${2:-0}"
    local out=""
    local i
    for ((i=0; i<count; i++)); do
        out+="$ch"
    done
    printf '%s' "$out"
}

_init_diagram_max_width() {
    local max=0
    local s len
    for s in "$@"; do
        len=${#s}
        (( len > max )) && max=$len
    done
    printf '%s' "$max"
}

_init_template_dynamic_diagram() {
    local tpl_id="$1"
    if [[ "$tpl_id" == "custom" ]]; then
        _tpl_custom_diagram
        return 0
    fi

    local resolved
    resolved=$(_init_template_resolved_commands "$tpl_id")
    local -a _cmds=()
    IFS='|' read -ra _cmds <<< "$resolved"

    local l1 l2 l3 l4
    l1=$(_init_command_display_label "${_cmds[0]:-null}")
    l2=$(_init_command_display_label "${_cmds[1]:-null}")
    l3=$(_init_command_display_label "${_cmds[2]:-null}")
    l4=$(_init_command_display_label "${_cmds[3]:-null}")

    case "$tpl_id" in
        ai-coding-left)
            local left_w right_w left_seg right_seg
            left_w=$(_init_diagram_max_width "$l1" "$l2" "(60%, 50%)")
            right_w=$(_init_diagram_max_width "$l3" "(40%)")
            left_seg=$(_init_diagram_repeat "─" $((left_w + 2)))
            right_seg=$(_init_diagram_repeat "─" $((right_w + 2)))

            printf '     ┌%s┬%s┐\n' "$left_seg" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l1" "$right_w" ""
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(60%, 50%)" "$right_w" "$l3"
            printf '     ├%s┤ %-*s │\n' "$left_seg" "$right_w" "(40%)"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l2" "$right_w" ""
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(60%, 50%)" "$right_w" ""
            printf '     └%s┴%s┘\n' "$left_seg" "$right_seg"
            ;;
        ai-coding-right)
            local left_w right_w left_seg right_seg
            left_w=$(_init_diagram_max_width "$l1" "(40%)")
            right_w=$(_init_diagram_max_width "$l2" "$l3" "(60%, 50%)")
            left_seg=$(_init_diagram_repeat "─" $((left_w + 2)))
            right_seg=$(_init_diagram_repeat "─" $((right_w + 2)))

            printf '     ┌%s┬%s┐\n' "$left_seg" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l1" "$right_w" "$l2"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(40%)" "$right_w" "(60%, 50%)"
            printf '     │ %-*s ├%s┤\n' "$left_w" "" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "" "$right_w" "$l3"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "" "$right_w" "(60%, 50%)"
            printf '     └%s┴%s┘\n' "$left_seg" "$right_seg"
            ;;
        ai-coding-half)
            local left_w right_w left_seg right_seg
            left_w=$(_init_diagram_max_width "$l1" "(50%)")
            right_w=$(_init_diagram_max_width "$l2" "(50%)")
            left_seg=$(_init_diagram_repeat "─" $((left_w + 2)))
            right_seg=$(_init_diagram_repeat "─" $((right_w + 2)))

            printf '     ┌%s┬%s┐\n' "$left_seg" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l1" "$right_w" "$l2"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(50%)" "$right_w" "(50%)"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "" "$right_w" ""
            printf '     │ %-*s │ %-*s │\n' "$left_w" "" "$right_w" ""
            printf '     │ %-*s │ %-*s │\n' "$left_w" "" "$right_w" ""
            printf '     └%s┴%s┘\n' "$left_seg" "$right_seg"
            ;;
        quad)
            local left_w right_w left_seg right_seg
            left_w=$(_init_diagram_max_width "$l1" "$l2" "(50%)")
            right_w=$(_init_diagram_max_width "$l3" "$l4" "(50%)")
            left_seg=$(_init_diagram_repeat "─" $((left_w + 2)))
            right_seg=$(_init_diagram_repeat "─" $((right_w + 2)))

            printf '     ┌%s┬%s┐\n' "$left_seg" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l1" "$right_w" "$l3"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(50%)" "$right_w" "(50%)"
            printf '     ├%s┼%s┤\n' "$left_seg" "$right_seg"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "$l2" "$right_w" "$l4"
            printf '     │ %-*s │ %-*s │\n' "$left_w" "(50%)" "$right_w" "(50%)"
            printf '     └%s┴%s┘\n' "$left_seg" "$right_seg"
            ;;
        *)
            _tpl_get "$tpl_id" "DIAGRAM"
            ;;
    esac
}

_init_template_preview_callback() {
    local idx="$1"
    local tpl_id="${_TPL_IDS[$idx]}"
    local diagram
    diagram=$(_init_template_dynamic_diagram "$tpl_id")
    local _cb_lines=0

    if [[ -n "$diagram" ]]; then
        _cursor_clear_line
        _tui_print "\n"
        _cb_lines=$((_cb_lines + 1))
        while IFS= read -r line; do
            _cursor_clear_line
            _tui_puts "$line"
            _tui_print "\n"
            _cb_lines=$((_cb_lines + 1))
        done <<< "$diagram"
        _cursor_clear_line
        _tui_print "\n"
        _cb_lines=$((_cb_lines + 1))
    else
        local i
        for ((i=0; i<$_TPL_PREVIEW_LINES; i++)); do
            _cursor_clear_line
            _tui_print "\n"
            _cb_lines=$((_cb_lines + 1))
        done
    fi

    _TUI_CALLBACK_LINES=$_cb_lines
}

# Configure pane commands one by one (for template fine-tuning)
# Uses _init_pane_counts and _init_col_count from parent scope
# Returns (stdout): '|' separated command list
_init_ask_pane_commands_with_defaults() {
    local cmds=()
    local pane_idx=0
    local ci pi

    for ((ci=0; ci<_init_col_count; ci++)); do
        local pc=${_init_pane_counts[$ci]}
        for ((pi=0; pi<pc; pi++)); do
            local name
            name=$(_init_columns_pane_label "$((ci+1))" "$((pi+1))")
            local default_cmd="${_init_pane_cmds[$pane_idx]:-null}"
            local display_default="$default_cmd"
            _is_null_or_empty "$default_cmd" && display_default="$(_i18n EMPTY_SHELL)"

            {
                echo ""
                log_bold "[$name] ($(_i18n CURRENT): $display_default)"
            } > /dev/tty

            local default_idx
            default_idx=$(_pane_command_resolve_default_index "$default_cmd" "${_AVAILABLE_CMD_DEFAULT_IDX:-0}")

            tui_command_select "  $name $(_i18n PANE_CMD_PROMPT)" _AVAILABLE_CMD_LABELS _AVAILABLE_CMD_VALUES "$default_idx"
            cmds+=("$TUI_SELECTED_CMD")
            pane_idx=$((pane_idx + 1))
        done
    done

    local IFS='|'
    echo "${cmds[*]}"
}

# ──────────────────────────────────────────────────
# Custom columns layout wizard
# ──────────────────────────────────────────────────

_COLUMNS_COL_COUNT=0
_COLUMNS_PANE_COUNTS=()
_COLUMNS_DEFAULT_COL_COUNT=0
_COLUMNS_DEFAULT_PANE_COUNTS=()
_COLUMNS_DEFAULT_PANE_CMDS=()

_init_columns_load_defaults_from_config() {
    local cfg_file="$1"
    _COLUMNS_DEFAULT_COL_COUNT=0
    _COLUMNS_DEFAULT_PANE_COUNTS=()
    _COLUMNS_DEFAULT_PANE_CMDS=()

    [[ -n "$cfg_file" && -f "$cfg_file" ]] || return 0
    check_cmd yq || return 0

    local col_count
    col_count=$(yq '.layout.columns | length' "$cfg_file" 2>/dev/null || echo "0")
    [[ "$col_count" =~ ^[0-9]+$ ]] || col_count=0
    (( col_count < 1 )) && return 0
    _COLUMNS_DEFAULT_COL_COUNT=$col_count

    local c p
    for ((c=0; c<col_count; c++)); do
        local pc
        pc=$(yq ".layout.columns[$c].panes | length" "$cfg_file" 2>/dev/null || echo "1")
        [[ "$pc" =~ ^[0-9]+$ ]] || pc=1
        ((pc < 1)) && pc=1
        _COLUMNS_DEFAULT_PANE_COUNTS+=("$pc")
        for ((p=0; p<pc; p++)); do
            local cc
            cc=$(yq ".layout.columns[$c].panes[$p].command // \"\"" "$cfg_file" 2>/dev/null)
            [[ "$cc" == "null" ]] && cc=""
            _COLUMNS_DEFAULT_PANE_CMDS+=("$cc")
        done
    done
}

_init_ask_columns_layout() {
    local cfg_file="${1:-}"
    local col_count

    _init_columns_load_defaults_from_config "$cfg_file"

    {
        echo ""
        log_bold "$(_i18n LAYOUT_COLUMNS_DESC)"
        echo ""
        if (( _COLUMNS_DEFAULT_COL_COUNT > 0 )); then
            echo "  ($(_i18n CURRENT): $(_i18n_fmt CUSTOM_CURRENT_COLUMNS_FMT "$_COLUMNS_DEFAULT_COL_COUNT"))"
            echo ""
        fi
    } > /dev/tty

    while true; do
        local col_prompt="  $(_i18n CUSTOM_NUM_COLUMNS_PROMPT)"
        if (( _COLUMNS_DEFAULT_COL_COUNT > 0 )); then
            col_prompt+=" [$_COLUMNS_DEFAULT_COL_COUNT]"
        fi
        col_prompt+=": "
        if ! read -r -p "$col_prompt" col_count < /dev/tty; then
            return 130
        fi
        if [[ -z "$col_count" && _COLUMNS_DEFAULT_COL_COUNT -gt 0 ]]; then
            col_count=$_COLUMNS_DEFAULT_COL_COUNT
        fi
        if [[ "$col_count" =~ ^[0-9]+$ ]] && ((col_count >= 1 && col_count <= 5)); then
            break
        fi
        echo "  $(_i18n CUSTOM_INVALID_COLUMNS)" > /dev/tty
    done

    _COLUMNS_COL_COUNT=$col_count
    _COLUMNS_PANE_COUNTS=()

    local c max_panes_per_col=$((10 / col_count))
    ((max_panes_per_col < 1)) && max_panes_per_col=1

    for ((c=0; c<col_count; c++)); do
        local pc
        local current_pc=1
        if (( c < ${#_COLUMNS_DEFAULT_PANE_COUNTS[@]} )); then
            current_pc="${_COLUMNS_DEFAULT_PANE_COUNTS[$c]}"
        fi
        ((current_pc < 1)) && current_pc=1
        ((current_pc > max_panes_per_col)) && current_pc=$max_panes_per_col
        while true; do
            local pane_prompt
            pane_prompt="  $(_i18n_fmt CUSTOM_COL_PANES_PROMPT "$((c+1))" "$max_panes_per_col")"
            pane_prompt+=" [$current_pc]: "
            if ! read -r -p "$pane_prompt" pc < /dev/tty; then
                return 130
            fi
            [[ -z "$pc" ]] && pc=$current_pc
            if [[ "$pc" =~ ^[0-9]+$ ]] && ((pc >= 1 && pc <= max_panes_per_col)); then
                break
            fi
            echo "  $(_i18n_fmt CUSTOM_INVALID_PANES "$max_panes_per_col")" > /dev/tty
        done
        _COLUMNS_PANE_COUNTS+=("$pc")
    done
}

_init_ask_columns_pane_commands() {
    local cfg_file="${1:-}"
    local cmds=()
    local c p
    local cmd_idx=0

    _init_columns_load_defaults_from_config "$cfg_file"

    {
        echo ""
        log_bold "$(_i18n CUSTOM_CONFIGURE_CMDS)"
        echo ""
    } > /dev/tty

    for ((c=0; c<_COLUMNS_COL_COUNT; c++)); do
        local pc=${_COLUMNS_PANE_COUNTS[$c]}
        for ((p=0; p<pc; p++)); do
            local name
            name=$(_init_columns_pane_label "$((c+1))" "$((p+1))")
            local current_cmd=""
            current_cmd=$(_init_columns_resolve_current_cmd "$cmd_idx")
            local display_current="$current_cmd"
            _is_null_or_empty "$display_current" && display_current="$(_i18n EMPTY_SHELL)"
            {
                echo ""
                log_bold "[$name] ($(_i18n CURRENT): $display_current)"
            } > /dev/tty
            local default_idx
            default_idx=$(_init_columns_resolve_prompt_default_index "$current_cmd")
            tui_command_select "  $name $(_i18n PANE_CMD_PROMPT)" _AVAILABLE_CMD_LABELS _AVAILABLE_CMD_VALUES "$default_idx"
            cmds+=("$TUI_SELECTED_CMD")
            cmd_idx=$((cmd_idx + 1))
        done
    done

    local IFS='|'
    echo "${cmds[*]}"
}

# Resolve current command shown in custom pane command wizard.
# Priority:
#   1) Existing pane command from current config
#   2) Global/default tool command (tools.default)
_init_columns_resolve_current_cmd() {
    local cmd_idx="${1:-0}"
    if (( cmd_idx < ${#_COLUMNS_DEFAULT_PANE_CMDS[@]} )); then
        echo "${_COLUMNS_DEFAULT_PANE_CMDS[$cmd_idx]}"
        return 0
    fi
    _pane_command_get_default_command
}

# Resolve default highlighted option in custom pane command selector.
# If current command is empty, prefer global tools.default so changes made in
# this init run take effect immediately in the next step.
_init_columns_resolve_prompt_default_index() {
    local current_cmd="${1:-}"

    if _is_null_or_empty "$current_cmd"; then
        if [[ "${_AVAILABLE_CMD_DEFAULT_REF:-}" == "__custom__" ]]; then
            echo "${_AVAILABLE_CMD_DEFAULT_IDX:-0}"
            return 0
        fi
        local fallback_cmd
        fallback_cmd=$(_pane_command_get_default_command)
        _pane_command_resolve_default_index "$fallback_cmd" "${_AVAILABLE_CMD_DEFAULT_IDX:-0}"
        return 0
    fi

    _pane_command_resolve_default_index "$current_cmd" "${_AVAILABLE_CMD_DEFAULT_IDX:-0}"
}

# ──────────────────────────────────────────────────
# Configuration generation (unified columns format)
# ──────────────────────────────────────────────────

_init_generate_config() {
    local theme="$1"
    local target_file="${2:-$(_workhive_dir)/config.yaml}"
    local include_tools="${3:-false}"
    local include_git_init="${4:-false}"

    _yaml_cmd() {
        local cmd="$1"
        if _is_null_or_empty "$cmd"; then
            echo "null"
        else
            echo "\"${cmd//\"/\\\"}\""
        fi
    }

    local target_dir
    target_dir=$(dirname "$target_file")
    ensure_dir "$target_dir"

    cat > "$target_file" <<EOF
# workhive configuration file
# Version: ${WORKHIVE_VERSION}
# Generated: $(date '+%Y-%m-%d %H:%M:%S')

version: "${WORKHIVE_VERSION}"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Layout configuration (Column-based DSL)
# Each column has independent pane count and sizing
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
layout:
  columns:
EOF

    local cmd_idx=0
    local ci pi
    for ((ci=0; ci<_init_col_count; ci++)); do
        local pc=${_init_pane_counts[$ci]}
        local col_width=${_init_col_widths[$ci]}
        cat >> "$target_file" <<EOF
    - width: ${col_width}
      panes:
EOF
        for ((pi=0; pi<pc; pi++)); do
            local p_height=${_init_pane_heights[$cmd_idx]}
            local p_cmd="${_init_pane_cmds[$cmd_idx]:-null}"
            cat >> "$target_file" <<EOF
        - height: ${p_height}
          command: $(_yaml_cmd "$p_cmd")
EOF
            cmd_idx=$((cmd_idx + 1))
        done
    done

    cat >> "$target_file" <<EOF

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Language (en / zh)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
language: "${WORKHIVE_LANG:-en}"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Worktree management
# sync_files format:
#   - source: ".env"        # required, path relative to git root
#     mode: "copy"          # optional: copy | symlink (default: copy)
# shorthand also supported: sync_files: [".env"]   # equivalent to mode=copy
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
  sync_files: []

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# tmux session configuration
# session_prefix: computed at runtime (wh@<instance_id>@<project>)
# To override, uncomment the line below and set a custom value
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
tmux:
  # session_prefix: null
  mouse_support: true
  env:
    CLAUDE_CODE_NO_FLICKER: 1
  theme: "$theme"
  attach_delay: 1s
  persistence:
    enabled: false                  # true | false
    generation: 0                   # Snapshot generation fence (auto-managed)
    auto_save_mode: hybrid          # hybrid | event | interval
    auto_save_debounce: 3s          # Duration: e.g. 3s, 30s, 1m
    auto_save_interval: 10m         # Duration: e.g. 1m, 10m, 1h
    restore_processes: true         # true | false
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1
    process_list: [":all:"]         # List of processes to capture/restore (e.g. ["vi", "npm", "hermes"]). ":all:" for everything.

    snapshot_history_limit: 8       # Integer >= 1
    snapshot_max_size_mb: 8         # Integer >= 0 (0 = no size cap)

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# maintenance
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
maintenance:
  temp_cleanup:
    max_age_days: 7
  # Optional dedicated test log path.
  # Default is /dev/null (no test log file persisted).
  # To keep a dedicated test file, set this to ".workhive/logs/workhive-test.log".
  # Optional test_log policy keys below inherit from maintenance.log.* when omitted.
  test_log:
    path: "/dev/null"
    # level: info           # inherit log.level
    # format: text          # inherit log.format
    # max_size_mb: 10       # inherit log.max_size_mb
    # max_backups: 5        # inherit log.max_backups
  log:
    path: ".workhive/logs/workhive.log"
    level: info             # debug | info | warn | error
    format: text            # text | json
    max_size_mb: 10         # Integer >= 1, rotation threshold per file
    max_backups: 5          # Integer >= 0, keep N rotated files (.1 ... .N); 0 = truncate mode

EOF

    if [[ "$include_git_init" == "true" ]]; then
        cat >> "$target_file" <<EOF

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Git init safety guard
# whitelist_paths: [] = no restriction (allow anywhere safe)
#                  ["/path/a", "/path/b"] = only allow these dirs and their subdirs
#                  Example:
#                    whitelist_paths:
#                      - "/Users/you/dev"
#                      - "/Users/you/projects"
# dangerous_paths_extra: exact-match blocked directories (auto-generated defaults)
#                        Includes home/system paths and is fully user-editable.
#                        Only the directory itself is blocked, not its subdirectories.
# precedence: hard guard (/) > whitelist_paths > dangerous_paths_extra
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
git_init:
  whitelist_paths: []
  dangerous_paths_extra:
$(_get_system_dangerous_paths_yaml)
EOF
    fi

    if [[ "$include_tools" == "true" ]]; then
        cat >> "$target_file" <<EOF

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Tool registry (global-only)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
tools:
  - tool_name: claude
    label: "Claude Code"
    command: "claude --dangerously-skip-permissions"
    is_ai: true
    default: true
    enabled: true
  - tool_name: codex
    label: "Codex"
    command: "codex"
    is_ai: true
    default: false
    enabled: true
  - tool_name: opencode
    label: "OpenCode"
    command: "opencode"
    is_ai: true
    default: false
    enabled: true
  - tool_name: lazygit
    label: "Lazygit"
    command: "lazygit"
    is_ai: false
    default: false
    enabled: true
  - tool_name: yazi
    label: "Yazi"
    command: "yazi"
    is_ai: false
    default: false
    enabled: true
EOF
    fi
}

_init_ensure_instance_id() {
    local wh_dir
    wh_dir=$(_workhive_dir)
    ensure_dir "$wh_dir"

    _ensure_gitignore_workhive

    local id_file="${wh_dir}/instance_id"
    if [[ -f "$id_file" ]]; then
        log_debug "Instance ID already exists: $(cat "$id_file")"
        return 0
    fi

    local new_id
    new_id=$(od -An -tx1 -N2 /dev/urandom 2>/dev/null | tr -d ' \n')
    printf '%s' "$new_id" > "$id_file"
    log_debug "Generated instance ID: $new_id"
}

# ──────────────────────────────────────────────────
# Dependency detection
# ──────────────────────────────────────────────────

_init_get_known_tool_version() {
    local tool="$1"
    [[ -z "$tool" ]] && return 1
    get_tool_version_by_profile "$tool" "strict"
}

init_command_check_dependencies() {
    local all_ok=true
    local missing_deps=()

    log_info "$(_i18n REQUIRED_DEPS)"

    if check_cmd git; then
        local git_version
        git_version=$(_init_get_known_tool_version "git") || true
        log_success "Git ${git_version:-unknown} ($(_i18n DEP_GIT))"
    else
        log_error "Git — $(_i18n NOT_INSTALLED)"
        missing_deps+=("git")
        all_ok=false
    fi

    if check_cmd tmux; then
        local tmux_version
        tmux_version=$(_init_get_known_tool_version "tmux") || true
        log_success "tmux ${tmux_version:-unknown} ($(_i18n DEP_TMUX))"
    else
        log_error "tmux — $(_i18n NOT_INSTALLED)"
        missing_deps+=("tmux")
        all_ok=false
    fi

    if check_cmd yq; then
        local yq_version
        yq_version=$(_init_get_known_tool_version "yq") || true
        log_success "yq ${yq_version:-unknown} ($(_i18n DEP_YQ))"
    else
        log_error "yq — $(_i18n NOT_INSTALLED)"
        missing_deps+=("yq")
        all_ok=false
    fi

    echo ""
    log_info "$(_i18n PANE_TOOLS)"

    if check_cmd lazygit; then
        local lazygit_version
        lazygit_version=$(_init_get_known_tool_version "lazygit") || true
        log_success "Lazygit ${lazygit_version:-unknown} ($(_i18n DEP_LAZYGIT))"
    else
        log_info "$(_i18n LAZYGIT_HINT)"
    fi

    if check_cmd claude; then
        local claude_version
        claude_version=$(_init_get_known_tool_version "claude") || true
        log_success "Claude Code ${claude_version:+$claude_version }($(_i18n DEP_CLAUDE))"
    else
        log_info "$(_i18n CLAUDE_HINT)"
    fi

    if check_cmd codex; then
        local codex_version
        codex_version=$(_init_get_known_tool_version "codex") || true
        log_success "Codex ${codex_version:+$codex_version }($(_i18n DEP_CODEX))"
    else
        log_info "$(_i18n CODEX_HINT)"
    fi

    if check_cmd opencode; then
        local opencode_version
        opencode_version=$(_init_get_known_tool_version "opencode") || true
        log_success "OpenCode ${opencode_version:+$opencode_version }($(_i18n DEP_OPENCODE))"
    else
        log_info "$(_i18n OPENCODE_HINT)"
    fi

    if check_cmd yazi; then
        local yazi_version
        yazi_version=$(_init_get_known_tool_version "yazi") || true
        log_success "yazi ${yazi_version:-unknown} ($(_i18n YAZI_DESC))"
    else
        log_info "$(_i18n YAZI_HINT)"
    fi

    if [[ "$all_ok" == false ]]; then
        echo ""
        log_error "$(_i18n DEPS_MISSING_LIST): ${missing_deps[*]}"
        echo ""

        local platform
        platform=$(platform_detect)

        log_info "$(_i18n INSTALL_GUIDE)"
        case "$platform" in
            macos)
                local dep
                for dep in "${missing_deps[@]}"; do
                    case "$dep" in
                        claude-code) echo "  npm install -g @anthropic-ai/claude-code" ;;
                        *) echo "  brew install $dep" ;;
                    esac
                done
                ;;
            linux)
                echo "  # Install using system package manager:"
                local dep
                for dep in "${missing_deps[@]}"; do
                    case "$dep" in
                        lazygit) echo "  # Lazygit: https://github.com/jesseduffield/lazygit#installation" ;;
                        yq) echo "  # yq: https://github.com/mikefarah/yq#install" ;;
                        claude-code) echo "  npm install -g @anthropic-ai/claude-code" ;;
                        *) echo "  sudo apt install $dep  # or equivalent package manager" ;;
                    esac
                done
                ;;
            *)
                local dep
                for dep in "${missing_deps[@]}"; do
                    case "$dep" in
                        git) echo "  Git: https://git-scm.com/downloads" ;;
                        tmux) echo "  tmux: https://github.com/tmux/tmux/wiki/Installing" ;;
                        yq) echo "  yq: https://github.com/mikefarah/yq#install" ;;
                        lazygit) echo "  Lazygit: https://github.com/jesseduffield/lazygit#installation" ;;
                        claude-code) echo "  npm install -g @anthropic-ai/claude-code" ;;
                    esac
                done
                ;;
        esac

        return 1
    fi

    return 0
}

# ──────────────────────────────────────────────────
# Theme selection
# ──────────────────────────────────────────────────

init_command_ask_theme() {
    local preview_cb="_theme_preview_callback"
    local options=(
        "Catppuccin Mocha (default)"
        "Tokyo Night"
        "Dracula"
        "Nord"
        "Gruvbox"
        "Rosé Pine"
        "Everforest"
        "One Dark"
    )

    tui_select_menu "$(_i18n SELECT_THEME)" options 0 "$preview_cb"

    case "$TUI_SELECTED_INDEX" in
        1) echo "tokyo-night" ;;
        2) echo "dracula" ;;
        3) echo "nord" ;;
        4) echo "gruvbox" ;;
        5) echo "rose-pine" ;;
        6) echo "everforest" ;;
        7) echo "onedark" ;;
        *) echo "catppuccin-mocha" ;;
    esac
}

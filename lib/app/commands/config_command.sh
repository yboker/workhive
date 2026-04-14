#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Config Command
# Interactive configuration modification command (App layer)
# Unified Column-based DSL — no legacy layout modes
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_CONFIG_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _CONFIG_COMMAND_SH_LOADED=1

source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/platform/detector.sh"
source "${LIB_DIR}/infra/persistence/manager.sh"
source "${LIB_DIR}/infra/deps/manager.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/utils/interactive.sh"
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/app/shared/theme_helper.sh"
source "${LIB_DIR}/app/shared/command_builder.sh"
source "${LIB_DIR}/app/shared/layout_visual.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/app/commands/init_command.sh"

config_command_help() {
    _help_dispatch "_config_command_help"
}

_config_bool_state_label() {
    local v="${1:-false}"
    if [[ "$v" == "true" ]]; then
        echo "$(_i18n DEPS_ENABLED)"
    else
        echo "$(_i18n DEPS_DISABLED)"
    fi
}

# Print current config summary (unified columns format)
config_command_print_current() {
    if ! config_project_exists; then
        log_info "$(_i18n ERR_CONFIG_NOT_FOUND) $(_workhive_dir)/config.yaml"
        return 1
    fi

    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_bold "   📋 $(_i18n TITLE_CONFIG_SUMMARY)"
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    log_info "$(_i18n LABEL_LAYOUT_MODE)"
    _config_print_layout_visual "columns" "project"
    echo ""

    log_info "$(_i18n LABEL_TMUX_CONFIG)"
    local theme mouse_support session_prefix
    theme=$(config_get "tmux.theme" 2>/dev/null || echo "catppuccin-mocha")
    mouse_support=$(config_get "tmux.mouse_support" 2>/dev/null || echo "true")
    session_prefix=$(session_get_prefix)

    echo "  $(_i18n LABEL_SESSION_PREFIX) $session_prefix"
    echo "  $(_i18n LABEL_MOUSE) $mouse_support"
    echo "  $(_i18n LABEL_THEME) $theme"
    echo ""

    log_info "$(_i18n LABEL_WT_MANAGEMENT)"
    local wt_base_path wt_auto_create wt_auto_delete
    wt_base_path=$(config_get "worktree.base_path" 2>/dev/null || echo ".workhive/worktrees")
    wt_auto_create=$(config_get "worktree.auto_create" 2>/dev/null || echo "true")
    wt_auto_delete=$(config_get "worktree.auto_delete" 2>/dev/null || echo "true")

    echo "  $(_i18n LABEL_BASE_PATH) $wt_base_path"
    echo "  $(_i18n LABEL_AUTO_CREATE) $(_config_bool_state_label "$wt_auto_create")"
    echo "  $(_i18n LABEL_AUTO_DELETE) $(_config_bool_state_label "$wt_auto_delete")"
    echo ""

    local persist_enabled
    persist_enabled=$(persistence_cfg_enabled)
    log_info "$(_i18n MODIFY_TMUX_PERSISTENCE_TITLE):"
    echo "  $(_i18n DEPS_STATUS_STATE): $persist_enabled"
    if [[ "$persist_enabled" == "true" ]]; then
        local persist_interval
        persist_interval=$(persistence_cfg_auto_save_interval)
        echo "  $(_i18n DEPS_STATUS_AUTOSAVE): ${persist_interval}"
    fi
    echo ""

    return 0
}

config_command_ask_modify() {
    local prompt="$1"
    if ! ask_yes_no "$prompt" "N"; then
        return 1
    fi
    return 0
}

_config_yaml_cmd_val() {
    local v="$1"
    if _is_null_or_empty "$v"; then
        echo "null"
    else
        v="${v//\\/\\\\}"
        v="${v//\"/\\\"}"
        echo "\"$v\""
    fi
}

_config_cleanup_stale_layout_temp_files() {
    local tmp_dir="${TMPDIR:-/tmp}"
    local stale_days
    stale_days=$(config_get "maintenance.temp_cleanup.max_age_days" "7" 2>/dev/null || echo "7")
    [[ "$stale_days" =~ ^[0-9]+$ ]] || stale_days=7
    (( stale_days < 1 )) && stale_days=7
    local stale_minutes=$((stale_days * 1440))

    [[ -d "$tmp_dir" ]] || return 0

    cleanup_stale_files_by_name "$tmp_dir" "$stale_minutes" 'workhive-layout.*.yaml'
    cleanup_stale_files_by_name "$tmp_dir" "$stale_minutes" 'workhive-layout.*'
}

_config_layout_mktemp() {
    _config_cleanup_stale_layout_temp_files
    local tmp_base
    tmp_base=$(mktemp "${TMPDIR:-/tmp}/workhive-layout.XXXXXX") || return 1
    local tmp_yaml="${tmp_base}.yaml"
    mv -- "$tmp_base" "$tmp_yaml" 2>/dev/null || {
        cleanup_file_quiet "$tmp_base"
        return 1
    }
    echo "$tmp_yaml"
}

_config_write_layout_yaml_file() {
    local out_file="$1"
    local col_count="$2"
    local widths_name="$3"
    local pane_counts_name="$4"
    local pane_heights_name="$5"
    local pane_cmds_name="$6"

    local -n _widths_ref="$widths_name"
    local -n _pane_counts_ref="$pane_counts_name"
    local -n _pane_heights_ref="$pane_heights_name"
    local -n _pane_cmds_ref="$pane_cmds_name"

    {
        echo "columns:"
        local cmd_idx=0
        local ci pi
        for ((ci=0; ci<col_count; ci++)); do
            local col_width="${_widths_ref[$ci]}"
            local pc="${_pane_counts_ref[$ci]}"
            echo "  - width: $col_width"
            echo "    panes:"
            for ((pi=0; pi<pc; pi++)); do
                local p_height="${_pane_heights_ref[$cmd_idx]}"
                local p_cmd="${_pane_cmds_ref[$cmd_idx]:-}"
                echo "      - height: $p_height"
                echo "        command: $(_config_yaml_cmd_val "$p_cmd")"
                cmd_idx=$((cmd_idx + 1))
            done
        done
    } > "$out_file"
}

_config_get_for_scope() {
    local key="$1"
    local scope="${2:-project}"
    if [[ "$scope" == "global" ]]; then
        local gf
        gf=$(config_get_global_file)
        if [[ -f "$gf" ]] && check_cmd yq; then
            yq ".$key" "$gf" 2>/dev/null || echo ""
        else
            echo ""
        fi
    else
        config_get "$key" 2>/dev/null || echo ""
    fi
}

# Interactive layout modification: read current columns, let user reconfigure
# Writes unified columns DSL to config file
_config_command_modify_layout() {
    local scope="$1"
    local _cfg_file="$2"
    local _config_set_extra=()
    [[ "$scope" == "global" ]] && _config_set_extra=("--global")

    echo ""
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_info "$(_i18n MODIFY_LAYOUT_TITLE)"
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    _init_build_available_commands

    _init_ask_columns_layout "$_cfg_file" || return $?
    local new_col_count=$_COLUMNS_COL_COUNT
    local new_pane_counts=("${_COLUMNS_PANE_COUNTS[@]}")
    local new_pane_cmds=()

    local _custom_cmds
    _custom_cmds=$(_init_ask_columns_pane_commands "$_cfg_file") || return $?
    IFS='|' read -ra new_pane_cmds <<< "$_custom_cmds"

    local _preview_widths=()
    local c
    for ((c=0; c<new_col_count; c++)); do
        _preview_widths+=($((100 / new_col_count)))
    done

    log_info "$(_i18n PREVIEW)"
    echo ""
    layout_preview_columns_runtime "$new_col_count" "_preview_widths" "new_pane_counts" "new_pane_cmds" > /dev/tty

    # Write unified columns config in a single atomic commit
    if check_cmd yq; then
        local _tmp_layout_file
        _tmp_layout_file=$(_config_layout_mktemp)
        [[ -n "$_tmp_layout_file" ]] || return 1
        local _tmp_widths=()
        local _tmp_pane_counts=()
        local _tmp_pane_heights=()
        local _tmp_pane_cmds=()

        local p
        for ((c=0; c<new_col_count; c++)); do
            _tmp_widths+=($((100 / new_col_count)))
            _tmp_pane_counts+=("${new_pane_counts[$c]}")
            local _pc=${new_pane_counts[$c]}
            for ((p=0; p<_pc; p++)); do
                _tmp_pane_heights+=($((100 / _pc)))
            done
        done
        _tmp_pane_cmds=("${new_pane_cmds[@]}")

        _config_write_layout_yaml_file "$_tmp_layout_file" "$new_col_count" "_tmp_widths" "_tmp_pane_counts" "_tmp_pane_heights" "_tmp_pane_cmds"
        if ! config_atomic_yq_edit "$_cfg_file" '.layout = load(strenv(LAYOUT_FILE))' "LAYOUT_FILE=$_tmp_layout_file"; then
            cleanup_file_quiet "$_tmp_layout_file"
            return 1
        fi
        cleanup_file_quiet "$_tmp_layout_file"
        cache_invalidate "config:*"
    fi
    echo ""
}

config_command_ask_pane_command() {
    local pane_name="$1"
    local config_path="$2"
    local scope="${3:-project}"
    local current_cmd
    current_cmd=$(_config_get_for_scope "$config_path" "$scope")

    local display_current
    if _is_null_or_empty "$current_cmd"; then
        display_current="$(_i18n EMPTY_SHELL)"
    else
        display_current="$current_cmd"
    fi

    {
        echo ""
        log_bold "[$pane_name] ($(_i18n CURRENT): $display_current)"
    } > /dev/tty

    _init_build_available_commands

    local default_idx
    default_idx=$(_pane_command_resolve_default_index "$current_cmd" "${_AVAILABLE_CMD_DEFAULT_IDX:-0}")

    tui_command_select "  $pane_name $(_i18n PANE_CMD_PROMPT)" _AVAILABLE_CMD_LABELS _AVAILABLE_CMD_VALUES "$default_idx"
    echo "$TUI_SELECTED_CMD"
}

config_command_ask_theme() {
    local scope="${1:-project}"
    local current_theme
    current_theme=$(_config_get_for_scope "tmux.theme" "$scope")
    _is_null_or_empty "$current_theme" && current_theme="catppuccin-mocha"

    {
        echo ""
        log_bold "$(_i18n MODIFY_TMUX_TITLE)"
        echo "  ($(_i18n CURRENT): $current_theme)"
        echo ""
    } > /dev/tty

    local preview_cb="_theme_preview_callback"
    # shellcheck disable=SC2034
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
    local default_idx=0
    case "$current_theme" in
        tokyo-night) default_idx=1 ;;
        dracula)     default_idx=2 ;;
        nord)        default_idx=3 ;;
        gruvbox)     default_idx=4 ;;
        rose-pine)   default_idx=5 ;;
        everforest)  default_idx=6 ;;
        onedark)     default_idx=7 ;;
    esac

    tui_select_menu "$(_i18n SELECT_THEME)" options "$default_idx" "$preview_cb"

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

config_command_edit_file() {
    local scope="${1:-}"
    local config_file

    if [[ "$scope" == "--global" ]]; then
        config_file=$(config_get_global_file)
        if [[ ! -f "$config_file" ]]; then
            log_info "$(_i18n INFO_NO_GLOBAL_CONFIG)"
            log_info "  Location: $config_file"
            echo ""
            if ask_yes_no "$(_i18n ASK_CREATE_GLOBAL_CONFIG)" "Y"; then
                echo ""
                init_command_execute --global
                local init_rc=$?
                [[ $init_rc -ne 0 ]] && return $init_rc
                [[ ! -f "$config_file" ]] && return 1
            else
                return 0
            fi
        fi
    else
        config_file="$(_workhive_dir)/config.yaml"
        if [[ ! -f "$config_file" ]]; then
            log_info "$(_i18n ERR_CONFIG_NOT_FOUND) $config_file"
            echo ""
            if ask_yes_no "$(_i18n ASK_CREATE_PROJECT_CONFIG)" "Y"; then
                echo ""
                init_command_execute
                local init_rc=$?
                [[ $init_rc -ne 0 ]] && return $init_rc
                [[ ! -f "$config_file" ]] && return 1
            else
                return 0
            fi
        fi
    fi

    local editor="${EDITOR:-${VISUAL:-vi}}"

    if ! command -v "${editor%% *}" >/dev/null 2>&1; then
        log_warn "$(_i18n MSG_EDITOR_NOT_FOUND) $editor"
        log_info "$(_i18n INFO_TRY_VI)"
        editor="vi"
    fi

    log_info "$(_i18n INFO_OPEN_EDITOR) $editor"
    echo ""

    $editor "$config_file"

    if check_cmd yq; then
        if yq '.' "$config_file" &>/dev/null; then
            log_success "✅ $(_i18n MSG_CONFIG_SAVED)"
            cache_invalidate "config:*"
        else
            log_error "❌ $(_i18n ERR_CONFIG_SYNTAX)"
            return 1
        fi
    else
        log_warn "$(_i18n WARN_NO_YQ_SKIP)"
        cache_invalidate "config:*"
    fi

    return 0
}

# Shared interactive config logic for project or global scope
# Initialize scope-specific variables for the config wizard.
# Sets: _cfg_file, _config_set_extra, _edit_extra, _print_fn, _wizard_title
# Returns: 0 = continue wizard, 1 = user cancelled, 2 = init succeeded (wizard done)
_config_wizard_init_scope() {
    local scope="$1"

    if [[ "$scope" == "global" ]]; then
        _cfg_file=$(config_get_global_file)
        _config_set_extra=("--global")
        _edit_extra=("--global")
        _print_fn="config_command_print_global"
        _wizard_title="🛠️  $(_i18n CONFIG_WIZARD) (Global)"
        if [[ ! -f "$_cfg_file" ]]; then
            log_info "$(_i18n INFO_NO_GLOBAL_CONFIG)"
            log_info "  Location: $_cfg_file"
            echo ""
            if ask_yes_no "$(_i18n ASK_CREATE_GLOBAL_CONFIG)" "Y"; then
                echo ""
                init_command_execute --global
                return 2
            fi
            return 1
        fi
    else
        _cfg_file="$(_workhive_dir)/config.yaml"
        _config_set_extra=()
        _edit_extra=()
        _print_fn="config_command_print_current"
        _wizard_title="🛠️  $(_i18n CONFIG_WIZARD)"
        if ! config_project_exists; then
            log_info "$(_i18n ERR_CONFIG_NOT_FOUND) $_cfg_file"
            echo ""
            if ask_yes_no "$(_i18n ASK_CREATE_PROJECT_CONFIG)" "Y"; then
                echo ""
                init_command_execute
                return 2
            fi
            return 1
        fi
    fi
    return 0
}

# Read current layout from config file into _cur_pane_cmds array.
# Sets: _cur_pane_cmds
_config_wizard_read_current_layout() {
    local cfg_file="$1"
    _cur_pane_cmds=()

    local col_count
    col_count=$(yq '.layout.columns | length' "$cfg_file" 2>/dev/null || echo "0")
    ((col_count < 1)) && col_count=2

    local ci pi
    for ((ci=0; ci<col_count; ci++)); do
        local cp
        cp=$(yq ".layout.columns[$ci].panes | length" "$cfg_file" 2>/dev/null || echo "1")
        ((cp < 1)) && cp=1
        for ((pi=0; pi<cp; pi++)); do
            local cc
            cc=$(yq ".layout.columns[$ci].panes[$pi].command // \"\"" "$cfg_file" 2>/dev/null)
            [[ "$cc" == "null" ]] && cc=""
            _cur_pane_cmds+=("$cc")
        done
    done
}

# Apply template or custom layout selection.
# Sets: _cfg_col_count, _cfg_col_widths, _cfg_pane_counts, _cfg_pane_heights, _cfg_pane_cmds
_config_wizard_apply_template() {
    local cfg_file="$1"
    local tpl_id="$2"
    local tpl_cols
    tpl_cols=$(_tpl_get "$tpl_id" "COLS")

    _cfg_col_count=0
    _cfg_col_widths=()
    _cfg_pane_counts=()
    _cfg_pane_heights=()
    _cfg_pane_cmds=()

    if [[ "$tpl_cols" == "0" || -z "$tpl_cols" ]]; then
        _init_ask_columns_layout "$cfg_file"
        _cfg_col_count=$_COLUMNS_COL_COUNT

        local c
        for ((c=0; c<_cfg_col_count; c++)); do
            _cfg_col_widths+=($((100 / _cfg_col_count)))
            _cfg_pane_counts+=("${_COLUMNS_PANE_COUNTS[$c]}")
            local pc=${_COLUMNS_PANE_COUNTS[$c]}
            local p
            for ((p=0; p<pc; p++)); do
                _cfg_pane_heights+=($((100 / pc)))
            done
        done

        { echo ""; log_success "$(_i18n SELECTED): $(_tpl_get "$tpl_id" "NAME")"; echo ""; } > /dev/tty

        local custom_cmds
        custom_cmds=$(_init_ask_columns_pane_commands "$cfg_file")
        IFS='|' read -ra _cfg_pane_cmds <<< "$custom_cmds"
    else
        _config_wizard_apply_preset_template "$cfg_file" "$tpl_id" "$tpl_cols"
    fi
}

# Apply a preset (non-custom) template, merging current pane commands.
_config_wizard_apply_preset_template() {
    local cfg_file="$1" tpl_id="$2" tpl_cols="$3"
    _cfg_col_count=$tpl_cols

    IFS=',' read -ra _cfg_pane_counts <<< "$(_tpl_get "$tpl_id" "PANES")"
    IFS=',' read -ra _cfg_col_widths <<< "$(_tpl_get "$tpl_id" "WIDTHS")"

    local heights_str
    heights_str=$(_tpl_get "$tpl_id" "HEIGHTS")
    IFS='|' read -ra _height_groups <<< "$heights_str"
    local g
    for g in "${_height_groups[@]}"; do
        IFS=',' read -ra _h_vals <<< "$g"
        local h
        for h in "${_h_vals[@]}"; do
            _cfg_pane_heights+=("$h")
        done
    done

    local tpl_cmds
    tpl_cmds=$(_tpl_get "$tpl_id" "CMDS")
    tpl_cmds=$(_init_resolve_template_commands "$tpl_cmds")
    IFS='|' read -ra _cfg_pane_cmds <<< "$tpl_cmds"

    local pane_idx=0 cur_idx=0
    local merged_cmds=()
    local ci pi
    for ((ci=0; ci<_cfg_col_count; ci++)); do
        local pc=${_cfg_pane_counts[$ci]}
        for ((pi=0; pi<pc; pi++)); do
            if [[ $cur_idx -lt ${#_cur_pane_cmds[@]} ]] && ! _is_null_or_empty "${_cur_pane_cmds[$cur_idx]}"; then
                merged_cmds+=("${_cur_pane_cmds[$cur_idx]}")
            else
                merged_cmds+=("${_cfg_pane_cmds[$pane_idx]:-}")
            fi
            pane_idx=$((pane_idx + 1))
            cur_idx=$((cur_idx + 1))
        done
    done
    _cfg_pane_cmds=("${merged_cmds[@]}")

    {
        echo ""
        log_success "$(_i18n SELECTED): $(_tpl_get "$tpl_id" "NAME")"
        echo ""
        echo "$(_tpl_get "$tpl_id" "DIAGRAM")"
        echo ""

        local _pane_idx=0
        for ((ci=0; ci<_cfg_col_count; ci++)); do
            local _pc=${_cfg_pane_counts[$ci]}
            for ((pi=0; pi<_pc; pi++)); do
                local _label="Col $((ci+1)) Pane $((pi+1))"
                local _display="${_cfg_pane_cmds[$_pane_idx]:-}"
                _is_null_or_empty "$_display" && _display="$(_i18n EMPTY_SHELL)"
                echo "  $_label: $_display"
                _pane_idx=$((_pane_idx + 1))
            done
        done
        echo ""
    } > /dev/tty

    if ask_yes_no "$(_i18n FINE_TUNE)" "N"; then
        echo ""
        local tuned_cmds
        tuned_cmds=$(_config_ask_pane_commands_tuning)
        IFS='|' read -ra _cfg_pane_cmds <<< "$tuned_cmds"
    fi
}

# Write layout columns to config file via yq.
_config_wizard_write_layout() {
    local cfg_file="$1"
    [[ ! -x "$(command -v yq)" ]] && return 0

    local _tmp_layout_file
    _tmp_layout_file=$(_config_layout_mktemp)
    [[ -n "$_tmp_layout_file" ]] || return 1
    _config_write_layout_yaml_file "$_tmp_layout_file" "$_cfg_col_count" "_cfg_col_widths" "_cfg_pane_counts" "_cfg_pane_heights" "_cfg_pane_cmds"
    if ! config_atomic_yq_edit "$cfg_file" '.layout = load(strenv(LAYOUT_FILE))' "LAYOUT_FILE=$_tmp_layout_file"; then
        cleanup_file_quiet "$_tmp_layout_file"
        return 1
    fi
    cleanup_file_quiet "$_tmp_layout_file"
    cache_invalidate "config:*"
}

# Ask and apply persistence settings.
_config_wizard_persistence() {
    local scope="$1"
    log_info "$(_i18n STEP_TMUX_PERSISTENCE)"
    echo ""

    local current_persistence
    current_persistence=$(_config_get_for_scope "tmux.persistence.enabled" "$scope")
    _is_null_or_empty "$current_persistence" && current_persistence="false"
    {
        echo "  $(_i18n DESC_TMUX_PERSISTENCE)"
        echo "  ($(_i18n CURRENT): $current_persistence)"
        echo ""
    } > /dev/tty

    if ask_yes_no "$(_i18n ASK_ENABLE_PERSISTENCE)" "N"; then
        config_set "tmux.persistence.enabled" "true" "${_config_set_extra[@]}"
        config_set "tmux.persistence.auto_save_interval" "$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL" "${_config_set_extra[@]}"
        config_set "tmux.persistence.restore_processes" "$_PERSIST_DEFAULT_RESTORE_PROCESSES" "${_config_set_extra[@]}"
    else
        config_set "tmux.persistence.enabled" "false" "${_config_set_extra[@]}"
    fi
    echo ""
}

_config_command_interactive_impl() {
    local scope="$1"
    local _cfg_file _print_fn _wizard_title
    local _config_set_extra=() _edit_extra=()
    local _cur_pane_cmds=()
    local _cfg_col_count=0
    local _cfg_col_widths=() _cfg_pane_counts=() _cfg_pane_heights=() _cfg_pane_cmds=()

    _config_wizard_init_scope "$scope"
    local _init_rc=$?
    if [[ $_init_rc -eq 2 ]]; then
        return 0
    elif [[ $_init_rc -ne 0 ]]; then
        return $_init_rc
    fi

    i18n_ask_language
    echo ""

    log_bold "$_wizard_title"
    echo ""

    log_info "$(_i18n STEP_DEPS)"
    echo ""
    init_command_check_dependencies
    echo ""

    log_info "$(_i18n STEP_TEMPLATE)"
    echo ""
    $_print_fn
    _init_build_available_commands
    _config_wizard_read_current_layout "$_cfg_file"

    local _modify_layout_rc=0
    if ask_yes_no "$(_i18n MODIFY_LAYOUT)" "N"; then
        _modify_layout_rc=0
    else
        _modify_layout_rc=$?
    fi
    if [[ $_modify_layout_rc -eq 130 ]]; then
        return 130
    fi
    if [[ $_modify_layout_rc -ne 0 ]]; then
        log_info "$(_i18n MSG_CANCELLED)"
        return 0
    fi

    local _detected_tpl_idx
    _detected_tpl_idx=$(_detect_current_template "$_cfg_file")
    local tpl_id
    tpl_id=$(_init_ask_template "$_detected_tpl_idx")

    _config_wizard_apply_template "$_cfg_file" "$tpl_id"
    echo ""

    log_info "$(_i18n PREVIEW)"
    echo ""
    layout_preview_columns_runtime "$_cfg_col_count" "_cfg_col_widths" "_cfg_pane_counts" "_cfg_pane_cmds" > /dev/tty

    _config_wizard_write_layout "$_cfg_file"

    log_info "$(_i18n STEP_THEME)"
    echo ""
    local new_theme
    new_theme=$(config_command_ask_theme "$scope")
    config_set "tmux.theme" "$new_theme" "${_config_set_extra[@]}"
    echo ""

    _config_wizard_persistence "$scope"

    echo ""
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_success "✅ $(_i18n CONFIG_COMPLETE)"
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    $_print_fn

    echo ""
    log_info "$(_i18n_fmt CONFIG_MANUAL_EDIT_HINT "$_cfg_file")"
    if [[ "$scope" == "global" ]]; then
        log_info "$(_i18n CONFIG_TOOLS_MANAGE_HINT)"
    fi

    return 0
}

# Ask pane commands for tuning a preset template (uses _cfg_pane_cmds from caller scope)
_config_ask_pane_commands_tuning() {
    local cmds=()
    local pane_idx=0
    local ci pi

    for ((ci=0; ci<_cfg_col_count; ci++)); do
        local pc=${_cfg_pane_counts[$ci]}
        for ((pi=0; pi<pc; pi++)); do
            local name="Col $((ci+1)) Pane $((pi+1))"
            local default_cmd="${_cfg_pane_cmds[$pane_idx]:-}"
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

config_command_interactive() {
    _config_command_interactive_impl "project"
}

config_command_print_global() {
    local global_file
    global_file=$(config_get_global_file)

    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_bold "   📋 $(_i18n TITLE_GLOBAL_CONFIG)"
    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    if [[ ! -f "$global_file" ]]; then
        log_info "$(_i18n INFO_NO_GLOBAL_CONFIG)"
        log_info "  Location: $global_file"
        echo ""
        return 0
    fi

    log_info "$(_i18n INFO_GLOBAL_CONFIG_FILE) $global_file"
    echo ""

    if check_cmd yq; then
        log_info "$(_i18n LABEL_LAYOUT_MODE)"
        _config_print_layout_visual "columns" "global"
        echo ""

        local theme mouse_support
        theme=$(yq '.tmux.theme // "catppuccin-mocha"' "$global_file" 2>/dev/null || echo "catppuccin-mocha")
        mouse_support=$(yq '.tmux.mouse_support // "true"' "$global_file" 2>/dev/null || echo "true")

        log_info "$(_i18n LABEL_TMUX_CONFIG)"
        echo "  $(_i18n LABEL_MOUSE) $mouse_support"
        echo "  $(_i18n LABEL_THEME) $theme"
        echo ""

        local persist_enabled
        persist_enabled=$(yq '.tmux.persistence.enabled // "false"' "$global_file" 2>/dev/null || echo "false")
        _is_null_or_empty "$persist_enabled" && persist_enabled="false"
        log_info "$(_i18n MODIFY_TMUX_PERSISTENCE_TITLE):"
        echo "  $(_i18n DEPS_STATUS_STATE): $persist_enabled"
        if [[ "$persist_enabled" == "true" ]]; then
            local persist_interval
            persist_interval=$(yq ".tmux.persistence.auto_save_interval // \"${_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL}\"" "$global_file" 2>/dev/null || echo "$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL")
            _is_null_or_empty "$persist_interval" && persist_interval="$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL"
            echo "  $(_i18n DEPS_STATUS_AUTOSAVE): ${persist_interval}"
        fi
        echo ""
    else
        cat "$global_file"
    fi

    return 0
}

config_command_global_interactive() {
    _config_command_interactive_impl "global"
}

_config_command_set() {
    local key="" value="" scope=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --global) scope="--global"; shift ;;
            -*) cli_log_unknown_opt "$1"; return 1 ;;
            *)
                if [[ -z "$key" ]]; then
                    key="$1"
                elif [[ -z "$value" ]]; then
                    value="$1"
                else
                    log_error "$(_i18n ERR_CONFIG_SET_TOO_MANY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    if [[ -z "$key" || -z "$value" ]]; then
        log_error "$(_i18n ERR_CONFIG_SET_USAGE)"
        echo ""
        echo "  workhive config set <key> <value> [--global]"
        return 1
    fi

    if [[ "$scope" == "--global" ]]; then
        config_sync_schema_scope "global"
    else
        config_sync_schema_scope "project"
    fi

    config_set "$key" "$value" $scope
    local rc=$?
    if [[ $rc -eq 0 ]]; then
        log_success "$(_i18n MSG_CONFIG_SET_OK) $key = $value"
    fi
    return $rc
}

_config_command_get() {
    local key="" scope="project"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --global) scope="global"; shift ;;
            -*) cli_log_unknown_opt "$1"; return 1 ;;
            *)
                if [[ -z "$key" ]]; then
                    key="$1"
                else
                    log_error "$(_i18n ERR_CONFIG_GET_TOO_MANY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    if [[ -z "$key" ]]; then
        log_error "$(_i18n ERR_CONFIG_GET_USAGE)"
        echo ""
        echo "  workhive config get <key> [--global]"
        return 1
    fi

    config_sync_schema_scope "$scope"

    local value
    value=$(_config_get_for_scope "$key" "$scope")
    if _is_null_or_empty "$value"; then
        echo "(unset)"
    else
        echo "$value"
    fi
    return 0
}

_config_command_layout_preview() {
    local scope="$1"
    local cfg_file="$2"

    log_bold "📋 $(_i18n TITLE_CONFIG_SUMMARY)"
    echo ""
    if [[ "$scope" == "global" ]]; then
        log_info "$(_i18n INFO_GLOBAL_CONFIG_FILE) $cfg_file"
    else
        log_info "$(_i18n INFO_CURRENT_CONFIG_FILE) $cfg_file"
    fi
    echo ""
    log_info "$(_i18n LABEL_LAYOUT_MODE)"
    _config_print_layout_visual "columns" "$scope"
    echo ""
}

# ──────────────────────────────────────────────────
# git-whitelist sub-command: manage git_init.whitelist_paths in global config
# Usage: workhive config git-whitelist [add <path>|remove <path>|list]
# ──────────────────────────────────────────────────
_config_git_whitelist_resolve_source() {
    local key_file
    key_file=$(config_find_file_with_key "git_init.whitelist_paths" 2>/dev/null || echo "")

    if [[ -z "$key_file" ]]; then
        echo "default"
        return 0
    fi

    local global_file project_file
    global_file=$(config_get_global_file)
    project_file="$(_workhive_dir)/config.yaml"
    if [[ "$key_file" == "$global_file" ]]; then
        echo "global"
    elif [[ "$key_file" == "$project_file" ]]; then
        echo "project"
    else
        echo "default"
    fi
    return 0
}

_config_git_whitelist_source_label() {
    local source="$1"
    case "$source" in
        project) echo "$(_i18n STATUS_LAYOUT_SRC_PROJECT)" ;;
        global) echo "$(_i18n STATUS_LAYOUT_SRC_GLOBAL)" ;;
        *) echo "$(_i18n STATUS_LAYOUT_SRC_DEFAULT)" ;;
    esac
}

_config_git_whitelist_ensure_global_config() {
    if config_global_exists; then
        return 0
    fi

    log_warn "$(_i18n MSG_NO_GLOBAL_CONFIG)"
    echo ""
    local _setup_global_rc=1
    if [[ -t 0 ]]; then
        confirm_action "$(_i18n ASK_SETUP_GLOBAL_CONFIG)" false "Y" 1
        _setup_global_rc=$?
    fi
    if [[ $_setup_global_rc -eq 130 ]]; then
        return 130
    fi
    if [[ $_setup_global_rc -eq 0 ]]; then
        echo ""
        WORKHIVE_SKIP_GLOBAL_CHECK=1 init_command_execute --global
        local rc=$?
        [[ $rc -ne 0 ]] && return $rc
        echo ""
    else
        echo ""
        log_info "$(_i18n HINT_SETUP_GLOBAL)"
        return 1
    fi

    return 0
}

_config_git_whitelist_help() {
    cat <<EOF
$(_i18n CONFIG_GIT_WHITELIST_USAGE_ALL)

  add <path>           $(_i18n CONFIG_GIT_WHITELIST_HELP_ADD)
  edit <old> <new>     $(_i18n CONFIG_GIT_WHITELIST_HELP_EDIT)
  remove <path>        $(_i18n CONFIG_GIT_WHITELIST_HELP_REMOVE)
  list                 $(_i18n CONFIG_GIT_WHITELIST_HELP_LIST)
  manage               $(_i18n CONFIG_GIT_WHITELIST_HELP_MANAGE)
  -h, --help           $(_i18n CONFIG_GIT_WHITELIST_HELP_HELP)
EOF
}

_config_git_whitelist_manage_help() {
    cat <<EOF
$(_i18n CONFIG_GIT_WHITELIST_USAGE_MANAGE)

$(_i18n CONFIG_GIT_WHITELIST_MANAGE_HELP_PURPOSE)

$(_i18n CONFIG_GIT_WHITELIST_MANAGE_HELP_ACTIONS)
  - $(_i18n GIT_WHITELIST_MENU_LIST)
  - $(_i18n GIT_WHITELIST_MENU_ADD)
  - $(_i18n GIT_WHITELIST_MENU_EDIT)
  - $(_i18n GIT_WHITELIST_MENU_REMOVE)
  - $(_i18n GIT_WHITELIST_MENU_DONE)

$(_i18n CONFIG_GIT_WHITELIST_MANAGE_HELP_NOTE)
EOF
}

_config_git_whitelist_manage() {
    if cli_is_help_flag "${1:-}"; then
        _config_git_whitelist_manage_help
        return 0
    fi

    local _manage_usage
    _manage_usage=$(_i18n CONFIG_GIT_WHITELIST_USAGE_MANAGE)
    local _validate_rc=0
    cli_expect_no_args "$_manage_usage" "config git-whitelist manage" "$@" || _validate_rc=$?
    if [[ $_validate_rc -eq 2 ]]; then
        return 0
    fi
    [[ $_validate_rc -ne 0 ]] && return $_validate_rc

    _config_git_whitelist_ensure_global_config || return $?
    if ! [ -t 0 ] || ! [ -t 1 ]; then
        log_error "$(_i18n GIT_WHITELIST_ERR_TTY_REQUIRED)"
        log_info "$(_i18n GIT_WHITELIST_HINT_NON_INTERACTIVE)"
        return 1
    fi

    while true; do
        local action_rc=0
        # shellcheck disable=SC2034
        local menu=(
            "$(_i18n GIT_WHITELIST_MENU_LIST)"
            "$(_i18n GIT_WHITELIST_MENU_ADD)"
            "$(_i18n GIT_WHITELIST_MENU_EDIT)"
            "$(_i18n GIT_WHITELIST_MENU_REMOVE)"
            "$(_i18n GIT_WHITELIST_MENU_DONE)"
        )
        tui_select_menu "$(_i18n GIT_WHITELIST_MENU_TITLE)" menu 0
        case "${TUI_SELECTED_INDEX:-0}" in
            0)
                if ! _config_git_whitelist list; then
                    action_rc=$?
                    [[ $action_rc -eq 130 ]] && return 130
                fi
                echo ""
                ;;
            1)
                local add_path=""
                while true; do
                    interactive_read_line "$(_i18n GIT_WHITELIST_PROMPT_ADD_PATH)" add_path || {
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                        break
                    }
                    [[ -n "$add_path" ]] && interactive_echo_field "$(_i18n GIT_WHITELIST_PROMPT_ADD_PATH)" "$add_path"
                    if ! _config_git_whitelist add "$add_path"; then
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                    else
                        break
                    fi
                done
                echo ""
                ;;
            2)
                local old_path="" new_path=""
                while true; do
                    interactive_read_line "$(_i18n GIT_WHITELIST_PROMPT_EDIT_OLD_PATH)" old_path || {
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                        break
                    }
                    interactive_read_line "$(_i18n GIT_WHITELIST_PROMPT_EDIT_NEW_PATH)" new_path || {
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                        break
                    }
                    [[ -n "$old_path" ]] && interactive_echo_field "$(_i18n GIT_WHITELIST_PROMPT_EDIT_OLD_PATH)" "$old_path"
                    [[ -n "$new_path" ]] && interactive_echo_field "$(_i18n GIT_WHITELIST_PROMPT_EDIT_NEW_PATH)" "$new_path"
                    if ! _config_git_whitelist edit "$old_path" "$new_path"; then
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                    else
                        break
                    fi
                done
                echo ""
                ;;
            3)
                local remove_path=""
                while true; do
                    interactive_read_line "$(_i18n GIT_WHITELIST_PROMPT_REMOVE_PATH)" remove_path || {
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                        break
                    }
                    [[ -n "$remove_path" ]] && interactive_echo_field "$(_i18n GIT_WHITELIST_PROMPT_REMOVE_PATH)" "$remove_path"
                    if ! _config_git_whitelist remove "$remove_path"; then
                        action_rc=$?
                        [[ $action_rc -eq 130 ]] && return 130
                    else
                        break
                    fi
                done
                echo ""
                ;;
            *)
                return 0
                ;;
        esac
    done
}

_config_git_whitelist() {
    local action="${1:-list}"
    local target_path="${2:-}"
    local target_path_new="${3:-}"
    local -a remaining_args=("${@:2}")

    local global_file
    global_file=$(config_get_global_file)
    local resolved=""
    local existing=""

    case "$action" in
        add)
            _config_git_whitelist_ensure_global_config || return $?
            if [[ -z "$target_path" ]]; then
                log_error "$(_i18n CONFIG_GIT_WHITELIST_USAGE_ADD)"
                return 1
            fi
            resolved=$(cd "$target_path" 2>/dev/null && pwd -P) || {
                log_error "$(_i18n ERR_DIR_NOT_EXIST) $target_path"
                return 1
            }
            existing=$(yq '.git_init.whitelist_paths // []' "$global_file" 2>/dev/null || echo "[]")
            if resolved="$resolved" yq -e '.[] | select(. == strenv(resolved))' <<< "$existing" &>/dev/null; then
                log_info "$(_i18n_fmt GIT_WHITELIST_ADDED "$resolved") (already present)"
                return 0
            fi
            config_atomic_yq_edit "$global_file" '.git_init.whitelist_paths += [strenv(resolved)]' "resolved=$resolved" || return 1
            log_success "$(_i18n_fmt GIT_WHITELIST_ADDED "$resolved")"
            ;;
        edit)
            _config_git_whitelist_ensure_global_config || return $?
            if [[ -z "$target_path" || -z "$target_path_new" ]]; then
                log_error "$(_i18n CONFIG_GIT_WHITELIST_USAGE_EDIT)"
                return 1
            fi
            local resolved_old resolved_new
            resolved_old=$(cd "$target_path" 2>/dev/null && pwd -P) || resolved_old="$target_path"
            resolved_new=$(cd "$target_path_new" 2>/dev/null && pwd -P) || {
                log_error "$(_i18n ERR_DIR_NOT_EXIST) $target_path_new"
                return 1
            }

            existing=$(yq '.git_init.whitelist_paths // []' "$global_file" 2>/dev/null || echo "[]")
            if ! resolved="$resolved_old" yq -e '.[] | select(. == strenv(resolved))' <<< "$existing" &>/dev/null; then
                log_warn "$(_i18n_fmt GIT_WHITELIST_NOT_FOUND "$resolved_old")"
                return 1
            fi
            if [[ "$resolved_old" == "$resolved_new" ]]; then
                log_info "$(_i18n_fmt GIT_WHITELIST_UPDATED "$resolved_old" "$resolved_new")"
                return 0
            fi
            if resolved="$resolved_new" yq -e '.[] | select(. == strenv(resolved))' <<< "$existing" &>/dev/null; then
                log_warn "$(_i18n_fmt GIT_WHITELIST_ALREADY_EXISTS "$resolved_new")"
                return 1
            fi

            config_atomic_yq_edit "$global_file" '.git_init.whitelist_paths = (.git_init.whitelist_paths | map(if . == strenv(resolved_old) then strenv(resolved_new) else . end))' \
                "resolved_old=$resolved_old" "resolved_new=$resolved_new" || return 1
            log_success "$(_i18n_fmt GIT_WHITELIST_UPDATED "$resolved_old" "$resolved_new")"
            ;;
        remove)
            _config_git_whitelist_ensure_global_config || return $?
            if [[ -z "$target_path" ]]; then
                log_error "$(_i18n CONFIG_GIT_WHITELIST_USAGE_REMOVE)"
                return 1
            fi
            resolved=$(cd "$target_path" 2>/dev/null && pwd -P) || resolved="$target_path"
            existing=$(yq '.git_init.whitelist_paths // []' "$global_file" 2>/dev/null || echo "[]")
            if ! resolved="$resolved" yq -e '.[] | select(. == strenv(resolved))' <<< "$existing" &>/dev/null; then
                log_warn "$(_i18n_fmt GIT_WHITELIST_NOT_FOUND "$resolved")"
                return 1
            fi
            config_atomic_yq_edit "$global_file" 'del(.git_init.whitelist_paths[] | select(. == strenv(resolved)))' "resolved=$resolved" || return 1
            log_success "$(_i18n_fmt GIT_WHITELIST_REMOVED "$resolved")"
            ;;
        list)
            local paths source source_label
            source=$(_config_git_whitelist_resolve_source)
            source_label=$(_config_git_whitelist_source_label "$source")
            log_info "$(_i18n STATUS_LAYOUT_SOURCE_LABEL) $source_label"
            paths=$(config_get "git_init.whitelist_paths" 2>/dev/null || echo "")
            if [[ "$paths" == "[]" || -z "$paths" ]]; then
                log_info "$(_i18n GIT_WHITELIST_EMPTY)"
            else
                log_info "$(_i18n GIT_WHITELIST_TITLE)"
                echo "$paths" | yq '.[]' 2>/dev/null | while IFS= read -r p; do
                    echo "  - $p"
                done
            fi
            ;;
        manage)
            _config_git_whitelist_manage "${remaining_args[@]}"
            return $?
            ;;
        -h|--help|help)
            _config_git_whitelist_help
            ;;
        *)
            log_error "$(_i18n CONFIG_GIT_WHITELIST_UNKNOWN_ACTION) $action"
            echo ""
            _config_git_whitelist_help
            return 1
            ;;
    esac
    return 0
}

config_command_execute() {
    local edit_mode=false
    local global_mode=false
    local layout_mode=false

    case "${1:-}" in
        set) shift; _config_command_set "$@"; return $? ;;
        get) shift; _config_command_get "$@"; return $? ;;
        git-whitelist) shift; _config_git_whitelist "$@"; return $? ;;
    esac

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) config_command_help; return 0 ;;
            --edit) edit_mode=true; shift ;;
            --global) global_mode=true; shift ;;
            --layout) layout_mode=true; shift ;;
            -*) cli_log_unknown_opt "$1"; echo ""; config_command_help; return 1 ;;
            *) cli_log_no_positional "config"; return 1 ;;
        esac
    done

    if [[ "$layout_mode" == true ]]; then
        if [[ "$edit_mode" == true ]]; then
            log_error "--edit cannot be combined with --layout"
            return 1
        fi
        local _cfg_scope _cfg_file
        if [[ "$global_mode" == true ]]; then
            _cfg_scope="global"
            _cfg_file=$(config_get_global_file)
            if [[ ! -f "$_cfg_file" ]]; then
                log_info "$(_i18n INFO_NO_GLOBAL_CONFIG)"
                log_info "  Location: $_cfg_file"
                echo ""
                if ask_yes_no "$(_i18n ASK_CREATE_GLOBAL_CONFIG)" "Y"; then
                    echo ""
                    init_command_execute --global
                else
                    return 0
                fi
            fi
        else
            _cfg_scope="project"
            _cfg_file="$(_workhive_dir)/config.yaml"
            if [[ ! -f "$_cfg_file" ]]; then
                log_info "$(_i18n ERR_CONFIG_NOT_FOUND) $_cfg_file"
                echo ""
                if ask_yes_no "$(_i18n ASK_CREATE_PROJECT_CONFIG)" "Y"; then
                    echo ""
                    init_command_execute
                else
                    return 0
                fi
            fi
        fi
        [[ -f "$_cfg_file" ]] || return 1
        _config_command_layout_preview "$_cfg_scope" "$_cfg_file"
        local _layout_confirm_rc=0
        if ask_yes_no "$(_i18n MODIFY_LAYOUT)" "N"; then
            _layout_confirm_rc=0
        else
            _layout_confirm_rc=$?
        fi
        if [[ $_layout_confirm_rc -eq 130 ]]; then
            return 130
        fi
        if [[ $_layout_confirm_rc -ne 0 ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
        echo ""
        op_begin "config_layout_interactive" "$_cfg_file" "scope=${_cfg_scope}"
        if ! _config_command_modify_layout "$_cfg_scope" "$_cfg_file"; then
            op_fail "config_layout_interactive" "$_cfg_file" 1 "scope=${_cfg_scope}"
            return 1
        fi
        op_ok "config_layout_interactive" "$_cfg_file" "scope=${_cfg_scope}"
        log_success "$(_i18n MSG_CONFIG_SET_OK) layout"
        return 0
    fi

    if [[ "$global_mode" == true ]]; then
        local _global_cfg
        _global_cfg=$(config_get_global_file)
        if [[ -f "$_global_cfg" ]]; then
            config_sync_schema_scope "global"
        fi
        if [[ "$edit_mode" == true ]]; then
            config_command_edit_file "--global"
        else
            config_command_global_interactive
        fi
        return $?
    fi

    if ! config_project_exists; then
        log_info "$(_i18n ERR_CONFIG_NOT_FOUND) $(_workhive_dir)/config.yaml"
        echo ""
        if ask_yes_no "$(_i18n ASK_CREATE_PROJECT_CONFIG)" "Y"; then
            echo ""
            init_command_execute
            return $?
        else
            return 0
        fi
    fi

    if [[ "$edit_mode" == true ]]; then
        config_sync_schema_scope "project"
        config_command_edit_file
    else
        log_bold "📋 $(_i18n TITLE_CONFIG_MANAGEMENT)"
        echo ""
        log_info "$(_i18n INFO_CURRENT_CONFIG_FILE) $(_workhive_dir)/config.yaml"
        echo ""
        config_sync_schema_scope "project"
        config_command_interactive
    fi

    return 0
}

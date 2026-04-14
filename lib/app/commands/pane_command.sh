#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Pane Command
# tmux pane management command (App layer)
#
# Subcommands:
#   open [worktree]     Start Worktree tool in current pane (prompts to create if not exists)
#   close               Close current pane
#   split [--cmd <cmd>] Split current pane below
#   send <pane> <text>  Send text to target pane
#   list                List all panes in current session
#   current             Show current pane details
#   (no args)           Show help
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_PANE_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _PANE_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/domain/services/pane_state_service.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/app/shared/worktree_collector.sh"
source "${LIB_DIR}/app/shared/worktree_selector.sh"
source "${LIB_DIR}/app/shared/command_builder.sh"

# ──────────────────────────────────────────────────
# Help
# ──────────────────────────────────────────────────

pane_command_help() {
    _help_dispatch "_pane_command_help"
}

# ──────────────────────────────────────────────────
# Per-subcommand help functions (definitions in help_index.sh)
# ──────────────────────────────────────────────────

_pane_open_help()   { _help_dispatch "_pane_open_help"; }
_pane_close_help()  { _help_dispatch "_pane_close_help"; }
_pane_split_help()  { _help_dispatch "_pane_split_help"; }
_pane_send_help()   { _help_dispatch "_pane_send_help"; }
_pane_list_help()   { _help_dispatch "_pane_list_help"; }
_pane_current_help(){ _help_dispatch "_pane_current_help"; }

# ──────────────────────────────────────────────────
# Main entry: Subcommand dispatch
# ──────────────────────────────────────────────────

pane_command_execute() {
    case "${1:-}" in
        open)
            shift
            _pane_cmd_open "$@"
            return $?
            ;;
        close)
            shift
            _pane_cmd_close "$@"
            return $?
            ;;
        split)
            shift
            _pane_cmd_split "$@"
            return $?
            ;;
        send)
            shift
            _pane_cmd_send "$@"
            return $?
            ;;
        list|ls)
            shift
            _pane_cmd_list "$@"
            return $?
            ;;
        current|cur)
            shift
            _pane_cmd_current "$@"
            return $?
            ;;
        -h|--help)
            pane_command_help
            return 0
            ;;
        "")
            pane_command_help
            return 0
            ;;
        -*)
            cli_log_unknown_opt "$1"
            echo ""
            pane_command_help
            return 1
            ;;
        *)
            log_error "$(_i18n ERR_UNKNOWN_CMD) $1"
            echo ""
            pane_command_help
            return 1
            ;;
    esac
}

# ──────────────────────────────────────────────────
# Subcommand: open
# Start Worktree tool in current pane (exec replaces process after command selection)
# Prompts to create if Worktree does not exist
# ──────────────────────────────────────────────────

_pane_cmd_open() {
    local target_worktree=""
    local specified_cmd=""
    local cmd_source="arg"
    local force=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                _pane_open_help
                return 0
                ;;
            --cmd)
                shift
                specified_cmd="${1:-}"
                [[ -z "$specified_cmd" ]] && { log_error "$(_i18n ERR_MISSING_CMD_VALUE)"; return 1; }
                shift
                ;;
            --force|-f)
                force=true
                shift
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                _pane_open_help
                return 1
                ;;
            *)
                if [[ -z "$target_worktree" ]]; then
                    target_worktree="$1"
                else
                    log_error "$(_i18n ERR_ONE_WORKTREE_ONLY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    _pane_require_tmux || return 1

    # Auto-detect current session and pane
    local current_session current_pane_index
    current_session=$(terminal_adapter_get_current_session 2>/dev/null)
    current_pane_index=$(terminal_adapter_get_current_pane_index 2>/dev/null)

    if [[ -z "$current_session" || -z "$current_pane_index" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    local pane_cmd pane_cwd
    pane_cmd=$(terminal_adapter_get_pane_command 2>/dev/null)
    pane_cwd=$(terminal_adapter_get_pane_cwd 2>/dev/null)
    local pane_dir="${pane_cwd##*/}"

    log_bold "📌 $(_i18n TITLE_PANE_CMD)"
    echo ""
    log_info "$(_i18n INFO_SESSION) $current_session"
    log_info "$(_i18n INFO_CURRENT_PANE) $current_pane_index ($pane_cmd @ $pane_dir)"
    echo ""

    # Step 1: Determine target Worktree
    if [[ -z "$target_worktree" ]]; then
        local select_rc=0
        target_worktree=$(_pane_cmd_select_worktree) || select_rc=$?
        if [[ $select_rc -ne 0 || -z "$target_worktree" ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    # Resolve target directory
    local target_dir=""
    if [[ "$target_worktree" == "__MAIN_DIR__" ]]; then
        target_dir=$(_git_root)
        log_info "$(_i18n INFO_TARGET_MAIN) ($target_dir)"
    elif worktree_exists "$target_worktree"; then
        target_dir=$(worktree_get_path "$target_worktree")
        log_info "$(_i18n INFO_TARGET): $target_worktree ($target_dir)"
    else
        if [[ "$force" == true ]]; then
            if ! worktree_create "$target_worktree"; then
                log_error "$(_i18n ERR_WT_CREATE_FAILED)"
                return 1
            fi
            log_success "$(_i18n PANE_WT_CREATED) $target_worktree"
            target_dir=$(worktree_get_path "$target_worktree")
        else
            local confirm_msg
            confirm_msg=$(_i18n_fmt PANE_CONFIRM_CREATE_WT "$target_worktree")
            if ask_yes_no "$confirm_msg" "Y"; then
                if ! worktree_create "$target_worktree"; then
                    log_error "$(_i18n ERR_WT_CREATE_FAILED)"
                    return 1
                fi
                log_success "$(_i18n PANE_WT_CREATED) $target_worktree"
                target_dir=$(worktree_get_path "$target_worktree")
            else
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
        fi
    fi
    echo ""

    # Step 2: Determine command to execute
    local cmd=""
    if [[ -n "$specified_cmd" ]]; then
        cmd=$(_pane_resolve_cmd_alias "$specified_cmd")
        cmd_source="arg"
    else
        cmd=$(_pane_cmd_select_command) || {
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        }
        case "${_PANE_CMD_LAST_SELECTION_KIND:-preset}" in
            preset) cmd_source="preset" ;;
            custom|empty) cmd_source="arg" ;;
            *) cmd_source="arg" ;;
        esac
    fi

    if _is_null_or_empty "$cmd"; then
        log_info "$(_i18n MSG_NO_CMD_CANCELLED)"
        return 0
    fi
    if ! _pane_validate_launch_command "$cmd" "$cmd_source"; then
        return 1
    fi

    echo ""
    log_info "$(_i18n INFO_LAUNCH) $cmd"
    log_debug "Workdir: $target_dir"
    local exec_cmd="$cmd"
    log_debug "Pane $current_pane_index exec: cd $target_dir && $exec_cmd"

    cd "$target_dir" || { log_error "$(_i18n ERR_CD_FAILED_TO) $target_dir"; return 1; }
    exec bash -c "$exec_cmd"
}

# ──────────────────────────────────────────────────
# Subcommand: close
# ──────────────────────────────────────────────────

_pane_cmd_close() {
    if _pane_close_has_help_flag "$@"; then
        _pane_close_help
        return 0
    fi

    local parsed_close_args
    local parse_rc=0
    parsed_close_args=$(_pane_close_parse_args "$@")
    parse_rc=$?
    if [[ "$parse_rc" -ne 0 ]]; then
        [[ "$parse_rc" -eq 2 ]] && return 0
        return 1
    fi

    local force target_index target_pane_id
    IFS='|' read -r force target_index target_pane_id <<< "$parsed_close_args"

    _pane_require_tmux || return 1

    local current_session
    current_session=$(_pane_close_require_session) || return 1

    local pane_count
    pane_count=$(terminal_adapter_count_panes "$current_session" 2>/dev/null)

    if [[ -z "$target_pane_id" && "$pane_count" -le 1 ]]; then
        log_error "$(_i18n ERR_LAST_PANE)"
        log_info "$(_i18n INFO_USE_DESTROY)"
        return 1
    fi

    local target_spec verify_target
    if [[ -n "$target_pane_id" ]]; then
        local resolved_target
        resolved_target=$(_pane_close_resolve_target_by_id "$current_session" "$target_pane_id") || return 1
        IFS='|' read -r target_spec verify_target <<< "$resolved_target"
    elif [[ -n "$target_index" ]]; then
        local resolved_target
        resolved_target=$(_pane_close_resolve_target_by_index "$current_session" "$target_index" "$pane_count") || return 1
        IFS='|' read -r target_spec verify_target <<< "$resolved_target"
    else
        local resolved_target
        resolved_target=$(_pane_close_resolve_target_current "$current_session") || return 1
        IFS='|' read -r target_spec verify_target <<< "$resolved_target"
    fi

    if [[ "$force" != true ]]; then
        if ! ask_yes_no "$(_i18n CONFIRM_CLOSE_PANE)" "N"; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        fi
    fi

    _pane_close_execute "$current_session" "$pane_count" "$target_spec" "$verify_target"
}

_pane_close_has_help_flag() {
    local arg
    for arg in "$@"; do
        case "$arg" in
            -h|--help) return 0 ;;
        esac
    done
    return 1
}

_pane_close_parse_args() {
    local force=false
    local target_index=""
    local target_pane_id=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                _pane_close_help
                return 2
                ;;
            --force|-f)
                force=true
                shift
                ;;
            --id)
                target_pane_id=$(cli_require_option_value_nonflag "--id" "${2:-}") || return 1
                shift 2
                ;;
            -* )
                cli_log_unknown_opt "$1"
                return 1
                ;;
            *)
                if [[ -n "$target_index" ]]; then
                    cli_log_no_positional
                    return 1
                fi
                target_index="$1"
                shift
                ;;
        esac
    done

    if [[ -n "$target_index" && -n "$target_pane_id" ]]; then
        log_error "$(_i18n PANE_CLOSE_TARGET_CONFLICT)"
        return 1
    fi
    if [[ -n "$target_pane_id" && ! "$target_pane_id" =~ ^%[0-9]+$ ]]; then
        log_error "$(_i18n_fmt PANE_CLOSE_INVALID_PANE_ID "$target_pane_id")"
        return 1
    fi

    printf '%s|%s|%s\n' "$force" "$target_index" "$target_pane_id"
}

_pane_close_require_session() {
    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null)
    if [[ -z "$current_session" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi
    printf '%s' "$current_session"
}

_pane_log_invalid_index() {
    local i18n_key="$1"
    local index_value="$2"
    local pane_count="$3"
    log_error "$(_i18n_fmt "$i18n_key" "$index_value" "$((pane_count - 1))")"
}

_pane_parse_table_json_deep_args() {
    local -n _pp_format=$1
    local -n _pp_deep=$2
    local -n _pp_deep_set=$3
    local help_fn="$4"
    local invalid_fmt_key="$5"
    shift 5

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                "$help_fn"
                return 2
                ;;
            --format)
                _pp_format=$(cli_parse_format_value_i18n "${2:-}" '^(table|json)$' "$invalid_fmt_key") || return 1
                shift 2
                ;;
            --deep)
                _pp_deep=true
                _pp_deep_set=true
                shift
                ;;
            *)
                cli_log_unknown_opt "$1"
                return 1
                ;;
        esac
    done
    return 0
}

_pane_apply_json_default_deep() {
    local output_format="${1:-table}"
    local deep_set="${2:-false}"
    local -n _target_deep=$3
    # Keep JSON contract deep by default for scripting compatibility.
    if [[ "$output_format" == "json" && "$deep_set" == false ]]; then
        _target_deep=true
    fi
}

_pane_resolve_current_session() {
    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null)
    if [[ -z "$current_session" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi
    printf '%s\n' "$current_session"
}

_pane_close_resolve_target_by_id() {
    local current_session="$1"
    local target_pane_id="$2"

    local resolved_target resolved_session resolved_window resolved_index resolved_id
    resolved_target=$(terminal_adapter_resolve_pane_target "$target_pane_id" 2>/dev/null)
    if [[ -z "$resolved_target" ]]; then
        log_error "$(_i18n_fmt PANE_CLOSE_PANE_ID_NOT_FOUND "$target_pane_id")"
        return 1
    fi
    IFS='|' read -r resolved_session resolved_window resolved_index resolved_id <<< "$resolved_target"

    if [[ "$resolved_session" != "$current_session" ]]; then
        log_error "$(_i18n_fmt PANE_CLOSE_CROSS_SESSION "$target_pane_id")"
        return 1
    fi

    if _pane_cmd_close__target_is_last_session_pane "$current_session" "$resolved_window" "$resolved_index"; then
        log_error "$(_i18n ERR_LAST_PANE)"
        log_info "$(_i18n INFO_USE_DESTROY)"
        return 1
    fi

    printf '%s|%s\n' "${resolved_window}.${resolved_index}" "$resolved_id"
}

_pane_close_resolve_target_by_index() {
    local current_session="$1"
    local target_index="$2"
    local pane_count="$3"

    if [[ ! "$target_index" =~ ^[0-9]+$ ]]; then
        _pane_log_invalid_index "PANE_CLOSE_INVALID_INDEX" "$target_index" "$pane_count"
        return 1
    fi
    if [[ "$target_index" -ge "$pane_count" ]]; then
        _pane_log_invalid_index "PANE_CLOSE_INVALID_INDEX" "$target_index" "$pane_count"
        return 1
    fi

    local target_entry verify_target
    target_entry=$(terminal_adapter_list_panes_format "$current_session" '#{pane_index}|#{pane_id}' 2>/dev/null | grep -E "^${target_index//%/%%}\|" | head -n1)
    if [[ -z "$target_entry" ]]; then
        _pane_log_invalid_index "PANE_CLOSE_INVALID_INDEX" "$target_index" "$pane_count"
        return 1
    fi
    IFS='|' read -r _unused_index verify_target <<< "$target_entry"

    printf '%s|%s\n' "$verify_target" "$verify_target"
}

_pane_close_resolve_target_current() {
    local current_session="$1"
    local current_pane_id
    current_pane_id=$(terminal_adapter_list_panes_format "$current_session" '#{pane_index}|#{pane_id}' 2>/dev/null | grep -E "^$(terminal_adapter_get_current_pane_index 2>/dev/null)\|" | head -n1 | cut -d'|' -f2)
    printf '|%s\n' "$current_pane_id"
}

_pane_close_execute() {
    local current_session="$1"
    local pane_count="$2"
    local target_spec="$3"
    local verify_target="$4"

    log_info "$(_i18n INFO_CLOSING_PANE)"

    local before_count="$pane_count"
    if [[ -n "$target_spec" ]]; then
        terminal_adapter_kill_pane_target "$current_session" "$target_spec" || return 1
    else
        terminal_adapter_kill_pane || return 1
    fi

    if ! _pane_verify_close_result "$current_session" "$before_count" "$verify_target"; then
        log_error "$(_i18n PANE_CLOSE_VERIFY_FAILED)"
        return 1
    fi

    _pane_trigger_save
}

_pane_trigger_save() {
    if ! declare -f persistence_save >/dev/null 2>&1; then
        source "${LIB_DIR}/infra/persistence/manager.sh" 2>/dev/null || true
    fi

    if declare -f persistence_save >/dev/null 2>&1; then
        persistence_save "event" "pane-command" >/dev/null 2>&1
        local save_rc=$?
        if [[ "$save_rc" -eq 1 ]]; then
            log_warn "$(_i18n LAYOUT_SAVE_FAILED)"
        fi
        return 0
    fi

    if declare -f auto_save_trigger >/dev/null 2>&1; then
        auto_save_trigger >/dev/null 2>&1 || true
    fi
    return 0
}

# ──────────────────────────────────────────────────
# Subcommand: split
# ──────────────────────────────────────────────────

_pane_cmd_split() {
    local split_cmd=""
    local percentage=50

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) _pane_split_help; return 0 ;;
            --cmd)
                shift
                split_cmd="${1:-}"
                [[ -z "$split_cmd" ]] && { log_error "$(_i18n ERR_MISSING_CMD_VALUE)"; return 1; }
                shift
                ;;
            --size)
                shift
                percentage="${1:-50}"
                shift
                ;;
            *) cli_log_unknown_opt "$1"; return 1 ;;
        esac
    done

    _pane_require_tmux || return 1

    log_info "$(_i18n INFO_SPLITTING_PANE)"
    terminal_adapter_split_current_pane "$percentage" "$split_cmd" || return 1
    _pane_trigger_save
}

# ──────────────────────────────────────────────────
# Shared: tmux environment check
# ──────────────────────────────────────────────────

_pane_require_tmux() {
    if [[ -z "${TMUX:-}" ]]; then
        log_error "$(_i18n ERR_MUST_IN_TMUX)"
        log_info "$(_i18n INFO_USE_OPEN)"
        return 1
    fi
    if [[ -z "$(terminal_adapter_get_current_session 2>/dev/null || true)" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        log_info "$(_i18n INFO_USE_OPEN)"
        return 1
    fi
    return 0
}

# ──────────────────────────────────────────────────
# Internal functions: interactive selection
# ──────────────────────────────────────────────────

# Interactive selection of Worktree (reusing worktree collection logic)
# Output (stdout): worktree name, or "__MAIN_DIR__"
# Return: 0 = selection successful, 1 = cancelled
_pane_cmd_select_worktree() {
    worktree_interactive_select \
        --with-main-dir \
        --with-create-new
}

# Interactive selection of command
# Output (stdout): command string
# Return: 0 = success, 1 = cancelled
_pane_cmd_select_command() {
    [[ ! -t 0 ]] && return 1

    _build_pane_command_list
    tui_command_select "$(_i18n SELECT_COMMAND)" _AVAILABLE_CMD_LABELS _AVAILABLE_CMD_VALUES 0

    local selected_raw="${_AVAILABLE_CMD_VALUES[$TUI_SELECTED_INDEX]:-}"
    case "$selected_raw" in
        __custom__) _PANE_CMD_LAST_SELECTION_KIND="custom" ;;
        __empty__) _PANE_CMD_LAST_SELECTION_KIND="empty" ;;
        *)         _PANE_CMD_LAST_SELECTION_KIND="preset" ;;
    esac

    echo "$TUI_SELECTED_CMD"
    return 0
}

# Resolve short command alias to full command string
# Supports: claude, codex, opencode, lazygit, yazi, shell
# Falls back to the raw string if no alias matches
_pane_resolve_cmd_alias() {
    local alias="$1"
    case "$alias" in
        claude)
            echo "claude --dangerously-skip-permissions"
            ;;
        codex)
            echo "codex"
            ;;
        opencode)
            echo "opencode"
            ;;
        lazygit)
            echo "lazygit"
            ;;
        yazi)
            echo "yazi"
            ;;
        shell|bash|zsh)
            echo ""
            ;;
        *)
            echo "$alias"
            ;;
    esac
}

_pane_command_bin() {
    local cmd="${1:-}"
    cmd="${cmd#"${cmd%%[![:space:]]*}"}"
    printf '%s' "${cmd%% *}"
}

_pane_is_known_agent_command() {
    local cmd="${1:-}"
    tools_registry_is_ai_command "$cmd"
}

_pane_is_builtin_preset_command() {
    local cmd="$1"
    case "$cmd" in
        "claude --dangerously-skip-permissions"|codex|opencode|lazygit|yazi) return 0 ;;
        *) return 1 ;;
    esac
}

_pane_is_registry_tool_command() {
    local target_cmd="$1"
    local line _id _label _cmd _rest
    while IFS= read -r line; do
        [[ -z "$line" ]] && continue
        IFS='|' read -r _id _label _cmd _rest <<< "$line"
        [[ "$_cmd" == "$target_cmd" ]] && return 0
    done < <(tools_registry_list 2>/dev/null || true)
    return 1
}

_pane_is_allowed_preset_bin() {
    local cmd_bin="$1"
    case "$cmd_bin" in
        claude|codex|opencode|lazygit|yazi) return 0 ;;
        *) return 1 ;;
    esac
}

_pane_validate_launch_command() {
    local cmd="$1"
    local source_kind="${2:-arg}"  # arg|preset

    _is_null_or_empty "$cmd" && return 0

    # Preset path must stay strict and deterministic. User-provided arg path
    # may legitimately include shell syntax (pipes/redirection/chaining).
    if [[ "$source_kind" == "preset" ]]; then
        if ! _has_shell_metacharacters "$cmd"; then
            log_error "$(_i18n PANE_ERR_REJECTED_UNSAFE_CMD)"
            return 1
        fi
    fi

    local cmd_bin
    cmd_bin=$(_pane_command_bin "$cmd")
    [[ -z "$cmd_bin" ]] && {
        log_error "$(_i18n PANE_ERR_REJECTED_UNSAFE_CMD)"
        return 1
    }
    if ! command -v "$cmd_bin" &>/dev/null; then
        log_error "$(_i18n ERR_CMD_NOT_FOUND) $cmd_bin"
        return 1
    fi

    if [[ "$source_kind" == "preset" ]]; then
        if ! _pane_is_allowed_preset_bin "$cmd_bin"; then
            log_error "$(_i18n_fmt PANE_ERR_PRESET_NOT_ALLOWLIST_FMT "$cmd_bin")"
            return 1
        fi
        if ! _pane_is_builtin_preset_command "$cmd" && ! _pane_is_registry_tool_command "$cmd"; then
            log_error "$(_i18n PANE_ERR_PRESET_UNTRUSTED)"
            return 1
        fi
    fi

    return 0
}

# ──────────────────────────────────────────────────
# Subcommand: send
# Send text to a specific pane (for AI automation)
# ──────────────────────────────────────────────────

_pane_cmd_send() {
    local pane_index=""
    local pane_role=""
    local text=""
    local send_mode="auto"
    local commit_mode="auto"
    local verbose=false
    local diagnose=false
    local send_all=false
    local -a target_panes=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help) _pane_send_help; return 0 ;;
            --mode)
                send_mode=$(cli_require_option_value "--mode" "${2:-}") || return 1
                shift 2
                ;;
            --commit)
                commit_mode=$(cli_require_option_value "--commit" "${2:-}") || return 1
                shift 2
                ;;
            --diagnose) diagnose=true; shift ;;
            --verbose) verbose=true; shift ;;
            --all) send_all=true; shift ;;
            --role)
                shift
                if [[ $# -eq 0 ]]; then
                    log_error "$(_i18n ERR_MISSING_ARG) --role"
                    return 1
                fi
                pane_role="$1"
                shift
                ;;
            -*)
                if [[ "$send_all" == true ]]; then
                    if [[ -z "$text" ]]; then
                        text="$1"
                    else
                        text="$text $1"
                    fi
                    shift
                    continue
                fi
                if [[ -z "$pane_index" && -z "$pane_role" ]]; then
                    cli_log_unknown_opt "$1"
                    return 1
                fi
                if [[ -z "$text" ]]; then
                    text="$1"
                else
                    text="$text $1"
                fi
                shift
                ;;
            *)
                if [[ "$send_all" == true ]]; then
                    if [[ -z "$text" ]]; then
                        text="$1"
                    else
                        text="$text $1"
                    fi
                    shift
                    continue
                fi
                if [[ -z "$pane_index" && -z "$pane_role" ]]; then
                    pane_index="$1"
                elif [[ -z "$text" ]]; then
                    text="$1"
                else
                    text="$text $1"
                fi
                shift
                ;;
        esac
    done

    case "$send_mode" in
        auto|plain|paste|bracketed-paste) ;;
        *)
            log_error "$(_i18n_fmt PANE_SEND_INVALID_MODE_FMT "$send_mode")"
            return 1
            ;;
    esac
    case "$commit_mode" in
        auto|enter|c-m|none) ;;
        *)
            log_error "$(_i18n_fmt PANE_SEND_INVALID_COMMIT_FMT "$commit_mode")"
            return 1
            ;;
    esac

    if [[ "$send_all" == true && ( -n "$pane_index" || -n "$pane_role" ) ]]; then
        log_error "$(_i18n PANE_SEND_TARGET_CONFLICT)"
        return 1
    fi

    if [[ "$send_all" == true && "$diagnose" == true ]]; then
        log_error "$(_i18n PANE_SEND_DIAG_NO_BROADCAST)"
        return 1
    fi

    # Resolve pane index from role if --role was used
    if [[ "$send_all" != true && -n "$pane_role" ]]; then
        local resolved_idx
        resolved_idx=$(layout_resolve_role_index "$pane_role" 2>/dev/null)
        if [[ $? -ne 0 ]] || [[ -z "$resolved_idx" ]]; then
            log_error "$(_i18n_fmt PANE_ROLE_NOT_FOUND "$pane_role")"
            return 1
        fi
        pane_index="$resolved_idx"
    fi

    if [[ "$diagnose" == true ]]; then
        local commit_none=false
        [[ "$commit_mode" == "none" ]] && commit_none=true
        _pane_cmd_send_diagnose "$pane_index" "$pane_role" "$text" "$commit_none"
        return $?
    fi

    if [[ -z "$text" ]]; then
        log_error "$(_i18n PANE_SEND_USAGE)"
        return 1
    fi

    _pane_require_tmux || return 1

    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null || true)

    if [[ -z "$current_session" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    # Validate pane index exists
    local pane_count
    pane_count=$(terminal_adapter_count_panes "$current_session" 2>/dev/null || true)
    if [[ -z "$pane_count" || ! "$pane_count" =~ ^[0-9]+$ ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    if [[ "$send_all" == true ]]; then
        local idx
        for ((idx=0; idx<pane_count; idx++)); do
            log_debug "Broadcasting to ${current_session}:0.${idx}: $text"
            terminal_adapter_send_text "$current_session" "$idx" "$text" "false" "$send_mode" "$commit_mode" || return 1
            if [[ "$verbose" == true ]]; then
                local send_meta
                send_meta=$(terminal_adapter_get_last_send_meta 2>/dev/null || true)
                [[ -z "$send_meta" ]] && send_meta="mode=${send_mode} commit=${commit_mode}"
                log_info "$(_i18n_fmt PANE_SEND_VERBOSE_FMT "${idx}" "$send_meta")"
            fi
        done
        log_info "$(_i18n_fmt PANE_SEND_BROADCAST_OK "$pane_count")"
        return 0
    fi

    if [[ -z "$pane_index" ]]; then
        log_error "$(_i18n PANE_SEND_USAGE)"
        return 1
    fi

    if [[ "$pane_index" == *,* ]]; then
        local -a raw_targets
        IFS=',' read -r -a raw_targets <<< "$pane_index"
        declare -A _seen_targets=()
        local raw_target target
        for raw_target in "${raw_targets[@]}"; do
            target="${raw_target#"${raw_target%%[![:space:]]*}"}"
            target="${target%"${target##*[![:space:]]}"}"
            if [[ -z "$target" || ! "$target" =~ ^[0-9]+$ ]]; then
                _pane_log_invalid_index "PANE_SEND_INVALID_INDEX" "$target" "$pane_count"
                return 1
            fi
            if [[ "$target" -ge "$pane_count" ]]; then
                _pane_log_invalid_index "PANE_SEND_INVALID_INDEX" "$target" "$pane_count"
                return 1
            fi
            if [[ -z "${_seen_targets[$target]+x}" ]]; then
                _seen_targets["$target"]=1
                target_panes+=("$target")
            fi
        done
    else
        if [[ ! "$pane_index" =~ ^[0-9]+$ || "$pane_index" -ge "$pane_count" ]]; then
            _pane_log_invalid_index "PANE_SEND_INVALID_INDEX" "$pane_index" "$pane_count"
            return 1
        fi
        target_panes+=("$pane_index")
    fi

    local target
    for target in "${target_panes[@]}"; do
        log_debug "Sending to ${current_session}:0.${target}: $text"
        terminal_adapter_send_text "$current_session" "$target" "$text" "false" "$send_mode" "$commit_mode" || return 1
        if [[ "$verbose" == true ]]; then
            local send_meta
            send_meta=$(terminal_adapter_get_last_send_meta 2>/dev/null || true)
            [[ -z "$send_meta" ]] && send_meta="mode=${send_mode} commit=${commit_mode}"
            log_info "$(_i18n_fmt PANE_SEND_VERBOSE_FMT "${target}" "$send_meta")"
        fi
    done

    local target_label
    if [[ ${#target_panes[@]} -eq 1 ]]; then
        target_label="pane ${target_panes[0]}"
        [[ -n "$pane_role" ]] && target_label="pane ${target_panes[0]} (role: $pane_role)"
    else
        local joined_targets
        joined_targets=$(IFS=','; printf '%s' "${target_panes[*]}")
        target_label="panes $joined_targets"
    fi
    log_info "$(_i18n PANE_SEND_OK) $target_label"
}

_pane_send_is_interactive_command() {
    local cmd="${1:-}"
    case "$cmd" in
        vim|nvim|vi|nano|less|more|man|top|htop|btop|lazygit|yazi|fzf|watch|tig|ssh|mosh|tmux)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

_pane_cmd_send_diagnose() {
    local pane_index="${1:-}"
    local pane_role="${2:-}"
    local text="${3:-}"
    local commit_none="${4:-false}"

    _pane_require_tmux || return 1

    local current_session
    current_session=$(terminal_adapter_get_current_session 2>/dev/null || true)
    if [[ -z "$current_session" ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    if [[ -z "$pane_index" ]]; then
        pane_index=$(terminal_adapter_get_current_pane_index 2>/dev/null || true)
    fi

    if [[ -z "$pane_index" || ! "$pane_index" =~ ^[0-9]+$ ]]; then
        log_error "$(_i18n PANE_SEND_USAGE)"
        return 1
    fi

    local pane_format pane_rows pane_count
    pane_count=$(terminal_adapter_count_panes "$current_session" 2>/dev/null || true)
    if [[ -z "$pane_count" || ! "$pane_count" =~ ^[0-9]+$ ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi
    if [[ "$pane_index" -ge "$pane_count" ]]; then
        _pane_log_invalid_index "PANE_SEND_INVALID_INDEX" "$pane_index" "$pane_count"
        return 1
    fi

    pane_format="#{pane_index}|#{pane_id}|#{pane_current_command}|#{pane_current_path}|#{pane_dead}|#{pane_in_mode}|#{pane_active}|#{alternate_on}"
    pane_rows=$(terminal_adapter_list_panes_format "$current_session" "$pane_format" 2>/dev/null || true)

    local row pane_id pane_cmd pane_dir pane_dead pane_in_mode pane_active pane_alt
    while IFS= read -r row; do
        local idx
        idx="${row%%|*}"
        if [[ "$idx" == "$pane_index" ]]; then
            IFS='|' read -r _ pane_id pane_cmd pane_dir pane_dead pane_in_mode pane_active pane_alt <<< "$row"
            break
        fi
    done <<< "$pane_rows"

    if [[ -z "$pane_id" ]]; then
        _pane_log_invalid_index "PANE_SEND_INVALID_INDEX" "$pane_index" "$pane_count"
        return 1
    fi

    log_bold "🩺 $(_i18n PANE_SEND_DIAG_TITLE)"
    echo ""
    log_info "$(_i18n INFO_SESSION) $current_session"
    log_info "$(_i18n_fmt PANE_SEND_DIAG_TARGET_FMT "$pane_index")"
    [[ -n "$pane_role" ]] && log_info "$(_i18n_fmt PANE_SEND_DIAG_ROLE_FMT "$pane_role")"
    log_info "$(_i18n PANE_SEND_DIAG_CMD_LABEL) $pane_cmd"
    log_info "$(_i18n PANE_SEND_DIAG_DIR_LABEL) $pane_dir"
    log_info "$(_i18n PANE_SEND_DIAG_STATE_LABEL) active=$pane_active dead=$pane_dead in_mode=$pane_in_mode alternate_on=${pane_alt:-0}"
    if [[ -n "$text" ]]; then
        log_info "$(_i18n_fmt PANE_SEND_DIAG_TEXT_LEN_FMT "${#text}")"
    else
        log_warn "$(_i18n PANE_SEND_DIAG_WARN_EMPTY_TEXT)"
    fi
    if [[ "$commit_none" == true ]]; then
        log_warn "$(_i18n PANE_SEND_DIAG_WARN_NO_ENTER)"
    else
        log_info "$(_i18n PANE_SEND_DIAG_WITH_ENTER)"
    fi
    echo ""

    local has_risk=false
    local conclusion_msg=""
    if [[ "$pane_dead" == "1" ]]; then
        has_risk=true
        log_error "$(_i18n PANE_SEND_DIAG_WARN_PANE_DEAD)"
        conclusion_msg="$(_i18n PANE_SEND_DIAG_CONCLUSION_DEAD)"
    fi
    if [[ "$pane_in_mode" == "1" ]]; then
        has_risk=true
        log_warn "$(_i18n PANE_SEND_DIAG_WARN_IN_MODE)"
        [[ -z "$conclusion_msg" ]] && conclusion_msg="$(_i18n PANE_SEND_DIAG_CONCLUSION_IN_MODE)"
    fi
    if [[ "$commit_none" == true ]]; then
        [[ -z "$conclusion_msg" ]] && conclusion_msg="$(_i18n PANE_SEND_DIAG_CONCLUSION_NO_ENTER)"
    fi
    if _pane_send_is_interactive_command "$pane_cmd"; then
        has_risk=true
        log_warn "$(_i18n_fmt PANE_SEND_DIAG_WARN_INTERACTIVE_FMT "$pane_cmd")"
        [[ -z "$conclusion_msg" ]] && conclusion_msg="$(_i18n_fmt PANE_SEND_DIAG_CONCLUSION_INTERACTIVE_FMT "$pane_cmd")"
    fi
    if [[ -z "$text" ]]; then
        [[ -z "$conclusion_msg" ]] && conclusion_msg="$(_i18n PANE_SEND_DIAG_CONCLUSION_EMPTY_TEXT)"
    fi

    if [[ "$has_risk" == false ]]; then
        log_success "$(_i18n PANE_SEND_DIAG_OK)"
        [[ -z "$conclusion_msg" ]] && conclusion_msg="$(_i18n PANE_SEND_DIAG_CONCLUSION_READY)"
    fi

    echo ""
    log_info "$(_i18n PANE_SEND_DIAG_CONCLUSION_LABEL) $conclusion_msg"
    echo ""
    log_info "$(_i18n PANE_SEND_DIAG_HINT_TITLE)"
    log_info "  1) $(_i18n_fmt PANE_SEND_DIAG_HINT_SMOKE_FMT "$pane_index")"
    log_info "  2) $(_i18n_fmt PANE_SEND_DIAG_HINT_SEND_FMT "$pane_index")"
    log_info "  3) $(_i18n_fmt PANE_SEND_DIAG_HINT_NO_ENTER_FMT "$pane_index")"

    [[ "$pane_dead" == "1" ]] && return 1
    return 0
}

# ──────────────────────────────────────────────────
# Subcommand: list
# List all panes in current session with status
# Uses _dw/_wpad from common.sh for CJK-aware alignment
# ──────────────────────────────────────────────────

_pane_cmd_list() {
    local output_format="table"
    local deep=false
    local deep_set=false

    _pane_parse_table_json_deep_args output_format deep deep_set _pane_list_help "PANE_LIST_INVALID_FORMAT" "$@"
    local parse_rc=$?
    [[ "$parse_rc" == 2 ]] && return 0
    [[ "$parse_rc" -ne 0 ]] && return 1

    _pane_require_tmux || return 1

    local current_session
    current_session=$(_pane_resolve_current_session) || return 1
    _pane_apply_json_default_deep "$output_format" "$deep_set" deep

    local pane_format="#{pane_index}|#{pane_id}|#{pane_current_command}|#{pane_current_path}|#{pane_width}x#{pane_height}|#{pane_dead}"
    local panes
    panes=$(terminal_adapter_list_panes_format "$current_session" "$pane_format" 2>/dev/null)

    local current_pane
    current_pane=$(terminal_adapter_get_current_pane_index 2>/dev/null)

    if [[ "$output_format" == "json" ]]; then
        local session_json current_pane_json
        session_json=$(json_escape "$current_session")
        if [[ -n "$panes" && "$current_pane" =~ ^[0-9]+$ ]]; then
            current_pane_json="$current_pane"
        else
            current_pane_json="null"
        fi
        printf '{"session":"%s","current_pane_index":%s,"panes":[' "$session_json" "$current_pane_json"
        if [[ -n "$panes" ]]; then
            _pane_build_json_rows "$panes" "$current_pane" "$current_session" "$deep"
        fi
        printf ']}\n'
        return 0
    fi

    log_bold "📋 $(_i18n PANE_LIST_TITLE)"
    echo ""
    log_info "$(_i18n INFO_SESSION) $current_session"
    echo ""

    if [[ -z "$panes" ]]; then
        log_warn "$(_i18n PANE_LIST_EMPTY)"
        return 0
    fi

    if [[ "$deep" != true ]]; then
        local _h1 _h2 _h3 _h4 _h5
        _h1="$(_i18n PANE_LIST_HDR_INDEX)"
        _h2="$(_i18n PANE_LIST_HDR_CMD)"
        _h3="$(_i18n PANE_LIST_HDR_DIR)"
        _h4="$(_i18n PANE_LIST_HDR_SIZE)"
        _h5="$(_i18n PANE_LIST_HDR_STATUS)"

        local _w1 _w2 _w3 _w4
        _w1=$(_dw "$_h1"); _w2=$(_dw "$_h2")
        _w3=$(_dw "$_h3"); _w4=$(_dw "$_h4")

        local -a _pd_idx=() _pd_cmd=() _pd_dir=() _pd_size=() _pd_status=()
        while IFS='|' read -r idx pane_id cmd path size dead; do
            [[ -z "$idx" ]] && continue
            local dir_name st
            dir_name="${path##*/}"
            st="●"; [[ "$dead" == "1" ]] && st="✖"
            [[ "$idx" == "$current_pane" ]] && st="$st ◀"

            _pd_idx+=("$idx"); _pd_cmd+=("$cmd")
            _pd_dir+=("$dir_name"); _pd_size+=("$size"); _pd_status+=("$st")
            (( ${#idx} > _w1 )) && _w1=${#idx}
            (( ${#cmd} > _w2 )) && _w2=${#cmd}
            (( ${#dir_name} > _w3 )) && _w3=${#dir_name}
            (( ${#size} > _w4 )) && _w4=${#size}
        done <<< "$panes"

        (( _w1 += 2, _w2 += 2, _w3 += 2, _w4 += 2 ))

        printf "  %s %s %s %s %s\n" \
            "$(_wpad $_w1 "$_h1")" "$(_wpad $_w2 "$_h2")" \
            "$(_wpad $_w3 "$_h3")" "$(_wpad $_w4 "$_h4")" \
            "$(_wpad 8 "$_h5")"

        local _s1 _s2 _s3 _s4 _s5 _h5_dw
        printf -v _s1 '%*s' "$_w1" ""; _s1="${_s1// /─}"
        printf -v _s2 '%*s' "$_w2" ""; _s2="${_s2// /─}"
        printf -v _s3 '%*s' "$_w3" ""; _s3="${_s3// /─}"
        printf -v _s4 '%*s' "$_w4" ""; _s4="${_s4// /─}"
        _h5_dw=$(_dw "$_h5")
        printf -v _s5 '%*s' "$_h5_dw" ""; _s5="${_s5// /─}"
        printf "  %s %s %s %s %s\n" "$_s1" "$_s2" "$_s3" "$_s4" "$_s5"

        local i
        for ((i=0; i<${#_pd_idx[@]}; i++)); do
            printf "  %s %s %s %s %s\n" \
                "$(_wpad $_w1 "${_pd_idx[$i]}")" "$(_wpad $_w2 "${_pd_cmd[$i]}")" \
                "$(_wpad $_w3 "${_pd_dir[$i]}")" "$(_wpad $_w4 "${_pd_size[$i]}")" \
                "$(_wpad 8 "${_pd_status[$i]}")"
        done

        echo ""
        return 0
    fi

    declare -A _agent_names=()
    declare -A _agent_statuses=()
    declare -A _agent_sources=()
    declare -A _agent_confidences=()
    declare -A _agent_reasons=()
    local state_mode="simple"
    [[ "$deep" == true ]] && state_mode="deep"
    while IFS='|' read -r _ai_idx _ai_pid _ai_ppid _ai_name _ai_status _ai_source _ai_confidence _ai_reason; do
        [[ -z "$_ai_idx" ]] && continue
        _agent_names["$_ai_idx"]="$_ai_name"
        _agent_statuses["$_ai_idx"]="$_ai_status"
        _agent_sources["$_ai_idx"]="$_ai_source"
        _agent_confidences["$_ai_idx"]="$_ai_confidence"
        _agent_reasons["$_ai_idx"]="$_ai_reason"
    done < <(pane_state_collect_session "$current_session" "$state_mode" 2>/dev/null)

    local _h1 _h2 _h3 _h4 _h5 _h6 _h7
    _h1="$(_i18n PANE_LIST_HDR_INDEX)"
    _h2="$(_i18n PANE_LIST_HDR_CMD)"
    _h3="$(_i18n PANE_LIST_HDR_DIR)"
    _h4="$(_i18n PANE_LIST_HDR_SIZE)"
    _h5="$(_i18n PANE_LIST_HDR_STATUS)"
    _h6="$(_i18n PANE_LIST_HDR_AGENT)"
    _h7="$(_i18n PANE_LIST_HDR_AGENT_STATUS)"

    local _w1 _w2 _w3 _w4 _w6 _w7
    _w1=$(_dw "$_h1"); _w2=$(_dw "$_h2")
    _w3=$(_dw "$_h3"); _w4=$(_dw "$_h4")
    _w6=$(_dw "$_h6"); _w7=$(_dw "$_h7")

    local -a _pd_idx=() _pd_cmd=() _pd_dir=() _pd_size=() _pd_status=() _pd_agent=() _pd_agent_st=()
    while IFS='|' read -r idx pane_id cmd path size dead; do
        [[ -z "$idx" ]] && continue
        local dir_name st agent_disp agent_st_disp
        dir_name="${path##*/}"
        st="●"; [[ "$dead" == "1" ]] && st="✖"
        [[ "$idx" == "$current_pane" ]] && st="$st ◀"

        agent_disp=$(_pane_agent_name_display "${_agent_names[$idx]:-}")
        local raw_status="${_agent_statuses[$idx]:-unknown}"
        agent_st_disp=$(_pane_agent_status_display "$raw_status")

        _pd_idx+=("$idx"); _pd_cmd+=("$cmd")
        _pd_dir+=("$dir_name"); _pd_size+=("$size"); _pd_status+=("$st")
        _pd_agent+=("$agent_disp"); _pd_agent_st+=("$agent_st_disp")
        (( ${#idx} > _w1 )) && _w1=${#idx}
        (( ${#cmd} > _w2 )) && _w2=${#cmd}
        (( ${#dir_name} > _w3 )) && _w3=${#dir_name}
        (( ${#size} > _w4 )) && _w4=${#size}
        local _adw; _adw=$(_dw "$agent_disp"); (( _adw > _w6 )) && _w6=$_adw
        local _asdw; _asdw=$(_dw "$agent_st_disp"); (( _asdw > _w7 )) && _w7=$_asdw
    done <<< "$panes"

    (( _w1 += 2, _w2 += 2, _w3 += 2, _w4 += 2, _w6 += 2, _w7 += 2 ))

    printf "  %s %s %s %s %s %s %s\n" \
        "$(_wpad $_w1 "$_h1")" "$(_wpad $_w2 "$_h2")" \
        "$(_wpad $_w3 "$_h3")" "$(_wpad $_w4 "$_h4")" \
        "$(_wpad 8 "$_h5")" "$(_wpad $_w6 "$_h6")" "$_h7"

    local _s1 _s2 _s3 _s4 _s5 _s6 _s7 _h5_dw
    printf -v _s1 '%*s' "$_w1" ""; _s1="${_s1// /─}"
    printf -v _s2 '%*s' "$_w2" ""; _s2="${_s2// /─}"
    printf -v _s3 '%*s' "$_w3" ""; _s3="${_s3// /─}"
    printf -v _s4 '%*s' "$_w4" ""; _s4="${_s4// /─}"
    _h5_dw=$(_dw "$_h5")
    printf -v _s5 '%*s' "$_h5_dw" ""; _s5="${_s5// /─}"
    printf -v _s6 '%*s' "$_w6" ""; _s6="${_s6// /─}"
    printf -v _s7 '%*s' "$_w7" ""; _s7="${_s7// /─}"
    printf "  %s %s %s %s %s %s %s\n" "$_s1" "$_s2" "$_s3" "$_s4" "$_s5" "$_s6" "$_s7"

    local i
    for ((i=0; i<${#_pd_idx[@]}; i++)); do
        printf "  %s %s %s %s %s %s %s\n" \
            "$(_wpad $_w1 "${_pd_idx[$i]}")" "$(_wpad $_w2 "${_pd_cmd[$i]}")" \
            "$(_wpad $_w3 "${_pd_dir[$i]}")" "$(_wpad $_w4 "${_pd_size[$i]}")" \
            "$(_wpad 8 "${_pd_status[$i]}")" "$(_wpad $_w6 "${_pd_agent[$i]}")" \
            "${_pd_agent_st[$i]}"
    done

    echo ""
}

_pane_cmd_current() {
    local output_format="table"
    local deep=false
    local deep_set=false

    _pane_parse_table_json_deep_args output_format deep deep_set _pane_current_help "PANE_CURRENT_INVALID_FORMAT" "$@"
    local parse_rc=$?
    [[ "$parse_rc" == 2 ]] && return 0
    [[ "$parse_rc" -ne 0 ]] && return 1

    _pane_require_tmux || return 1

    local current_session
    current_session=$(_pane_resolve_current_session) || return 1
    _pane_apply_json_default_deep "$output_format" "$deep_set" deep

    local current_pane
    current_pane=$(terminal_adapter_get_current_pane_index 2>/dev/null)
    if [[ -z "$current_pane" || ! "$current_pane" =~ ^[0-9]+$ ]]; then
        log_error "$(_i18n ERR_CANNOT_DETECT_TMUX)"
        return 1
    fi

    local pane_format="#{pane_index}|#{pane_id}|#{pane_current_command}|#{pane_current_path}|#{pane_width}x#{pane_height}|#{pane_dead}|#{pane_pid}|#{@workhive_agent_state}|#{@workhive_agent_state_source}"
    local panes current_row
    panes=$(terminal_adapter_list_panes_format "$current_session" "$pane_format" 2>/dev/null)
    current_row=$(printf '%s\n' "$panes" | awk -F'|' -v idx="$current_pane" '$1==idx{print; exit}')
    if [[ -z "$current_row" ]]; then
        log_error "$(_i18n_fmt PANE_CURRENT_NOT_FOUND "$current_pane")"
        return 1
    fi

    local idx pane_id cmd path size dead pane_pid raw_state raw_source
    IFS='|' read -r idx pane_id cmd path size dead pane_pid raw_state raw_source <<< "$current_row"
    local dir_name
    dir_name="${path##*/}"

    local agent_name="" agent_status="unknown" agent_source="probe" agent_confidence="low" agent_reason="UNKNOWN"
    local single_state
    if [[ "$deep" == true ]]; then
        single_state=$(pane_state_collect_one "$idx" "$pane_id" "$pane_pid" "$cmd" 2>/dev/null || true)
    else
        single_state=$(pane_state_collect_one_simple "$idx" "$pane_id" "$pane_pid" "$cmd" "$raw_state" "$raw_source" 2>/dev/null || true)
    fi
    if [[ -n "$single_state" ]]; then
        IFS='|' read -r _ai_idx _ai_pid _ai_ppid _ai_name _ai_status _ai_source _ai_confidence _ai_reason <<< "$single_state"
        agent_name="$_ai_name"
        agent_status="${_ai_status:-unknown}"
        agent_source="${_ai_source:-probe}"
        agent_confidence="${_ai_confidence:-low}"
        agent_reason="${_ai_reason:-UNKNOWN}"
    fi

    local width height
    width="${size%x*}"
    height="${size#*x}"
    [[ "$width" =~ ^[0-9]+$ ]] || width=0
    [[ "$height" =~ ^[0-9]+$ ]] || height=0

    if [[ "$output_format" == "json" ]]; then
        local session_json pane_json
        session_json=$(json_escape "$current_session")
        pane_json=$(_pane_build_json_object \
            "$idx" "$pane_id" "$cmd" "$path" "$width" "$height" \
            "true" "$( [[ "$dead" == "1" ]] && printf 'true' || printf 'false' )" \
            "$agent_name" "$agent_status" "$agent_source" "$agent_confidence" "$agent_reason")
        printf '{"session":"%s","pane":%s}\n' "$session_json" "$pane_json"
        return 0
    fi

    local agent_st_disp
    agent_st_disp=$(_pane_agent_status_display "$agent_status")
    agent_name=$(_pane_agent_name_display "$agent_name")

    log_bold "📌 $(_i18n PANE_CURRENT_TITLE)"
    echo ""
    log_info "$(_i18n INFO_SESSION) $current_session"
    echo ""
    log_info "$(_i18n PANE_CURRENT_LABEL_INDEX) $idx"
    log_info "$(_i18n PANE_CURRENT_LABEL_ID) $pane_id"
    log_info "$(_i18n PANE_CURRENT_LABEL_CMD) $cmd"
    log_info "$(_i18n PANE_CURRENT_LABEL_DIR) $dir_name"
    log_info "$(_i18n PANE_CURRENT_LABEL_PATH) $path"
    log_info "$(_i18n PANE_CURRENT_LABEL_SIZE) $size"
    log_info "$(_i18n PANE_CURRENT_LABEL_STATUS) $( [[ "$dead" == "1" ]] && printf '✖' || printf '●' )"
    log_info "$(_i18n PANE_CURRENT_LABEL_AGENT) $agent_name"
    log_info "$(_i18n PANE_CURRENT_LABEL_AGENT_STATUS) $agent_st_disp"
    if [[ "$deep" == true ]]; then
        log_info "$(_i18n PANE_CURRENT_LABEL_STATE_SOURCE) $agent_source"
        log_info "$(_i18n PANE_CURRENT_LABEL_STATE_CONFIDENCE) $agent_confidence"
        log_info "$(_i18n PANE_CURRENT_LABEL_STATE_REASON) $agent_reason"
    fi
    echo ""
}

_pane_verify_close_result() {
    local session_name="$1"
    local before_count="$2"
    local verify_target="$3"

    if [[ -n "$verify_target" ]]; then
        local remaining_panes
        remaining_panes=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}|#{pane_id}' 2>/dev/null)
        [[ -n "$remaining_panes" ]] || return 1
        if printf '%s\n' "$remaining_panes" | grep -Eq "^${verify_target//%/%%}\||\|${verify_target//%/%%}$"; then
            return 1
        fi
        return 0
    fi

    local after_count
    after_count=$(terminal_adapter_count_panes "$session_name" 2>/dev/null)
    [[ -n "$after_count" ]] || return 1
    [[ "$after_count" -eq $((before_count - 1)) ]] || return 1

    return 0
}

_pane_agent_status_display() {
    local raw_status="${1:-unknown}"
    case "$raw_status" in
        running) printf '%s' "$(_i18n AGENT_STATUS_RUNNING)" ;;
        waiting_input|waiting) printf '%s' "$(_i18n AGENT_STATUS_WAITING)" ;;
        idle) printf '%s' "$(_i18n AGENT_STATUS_IDLE)" ;;
        unknown) printf '%s' "$(_i18n AGENT_STATUS_UNKNOWN)" ;;
        stopped) printf '%s' "$(_i18n AGENT_STATUS_STOPPED)" ;;
        *) printf '%s' "$(_i18n AGENT_STATUS_NONE)" ;;
    esac
}

_pane_agent_name_display() {
    local agent_name="${1:-}"
    if [[ -z "$agent_name" ]]; then
        printf '%s' "$(_i18n AGENT_STATUS_NONE)"
        return 0
    fi
    printf '%s' "$agent_name"
}

_pane_build_json_object() {
    local idx="$1"
    local pane_id="$2"
    local cmd="$3"
    local path="$4"
    local width="$5"
    local height="$6"
    local is_current="$7"
    local is_dead="$8"
    local agent_name="$9"
    local agent_status="${10}"
    local agent_source="${11}"
    local agent_confidence="${12}"
    local agent_reason="${13}"

    local dir_name
    dir_name="${path##*/}"

    local pane_id_json cmd_json path_json dir_json agent_name_json agent_status_json agent_source_json agent_confidence_json agent_reason_json
    pane_id_json=$(json_escape "$pane_id")
    cmd_json=$(json_escape "$cmd")
    path_json=$(json_escape "$path")
    dir_json=$(json_escape "$dir_name")
    agent_name_json=$(json_escape "$agent_name")
    agent_status_json=$(json_escape "$agent_status")
    agent_source_json=$(json_escape "$agent_source")
    agent_confidence_json=$(json_escape "$agent_confidence")
    agent_reason_json=$(json_escape "$agent_reason")

    printf '{"pane_index":%s,"pane_id":"%s","current_command":"%s","current_path":"%s","dir_name":"%s","size":{"width":%s,"height":%s},"is_current":%s,"is_dead":%s,"agent_name":"%s","agent_status":"%s","agent_state_source":"%s","agent_state_confidence":"%s","agent_state_reason":"%s"}' \
        "$idx" "$pane_id_json" "$cmd_json" "$path_json" "$dir_json" "$width" "$height" "$is_current" "$is_dead" \
        "$agent_name_json" "$agent_status_json" "$agent_source_json" "$agent_confidence_json" "$agent_reason_json"
}

_pane_cmd_close__target_is_last_session_pane() {
    local session_name="$1"
    local target_window="$2"
    local target_index="$3"

    local pane_listing
    pane_listing=$(terminal_adapter_list_panes_format "$session_name" '#{window_index}|#{pane_index}|#{pane_id}' 2>/dev/null)
    [[ -n "$pane_listing" ]] || return 1

    local total_panes=0
    local target_window_panes=0
    while IFS='|' read -r window_index pane_index _pane_id; do
        [[ -z "$window_index" ]] && continue
        total_panes=$((total_panes + 1))
        if [[ "$window_index" == "$target_window" ]]; then
            target_window_panes=$((target_window_panes + 1))
        fi
    done <<< "$pane_listing"

    [[ "$total_panes" -eq 1 && "$target_window_panes" -eq 1 ]]
}

_pane_build_json_rows() {
    local panes="$1"
    local current_pane="$2"
    local current_session="$3"
    local deep="${4:-false}"

    declare -A _agent_names=()
    declare -A _agent_statuses=()
    declare -A _agent_sources=()
    declare -A _agent_confidences=()
    declare -A _agent_reasons=()
    local state_mode="simple"
    [[ "$deep" == true ]] && state_mode="deep"
    while IFS='|' read -r _ai_idx _ai_pid _ai_ppid _ai_name _ai_status _ai_source _ai_confidence _ai_reason; do
        [[ -z "$_ai_idx" ]] && continue
        _agent_names["$_ai_idx"]="$_ai_name"
        _agent_statuses["$_ai_idx"]="$_ai_status"
        _agent_sources["$_ai_idx"]="$_ai_source"
        _agent_confidences["$_ai_idx"]="$_ai_confidence"
        _agent_reasons["$_ai_idx"]="$_ai_reason"
    done < <(pane_state_collect_session "$current_session" "$state_mode" 2>/dev/null)

    local first=true
    while IFS='|' read -r idx pane_id cmd path size dead; do
        [[ -z "$idx" ]] && continue

        local agent_name agent_status agent_source agent_confidence agent_reason width height
        agent_name="${_agent_names[$idx]:-}"
        agent_status="${_agent_statuses[$idx]:-unknown}"
        agent_source="${_agent_sources[$idx]:-probe}"
        agent_confidence="${_agent_confidences[$idx]:-low}"
        agent_reason="${_agent_reasons[$idx]:-UNKNOWN}"
        width="${size%x*}"
        height="${size#*x}"
        [[ "$width" =~ ^[0-9]+$ ]] || width=0
        [[ "$height" =~ ^[0-9]+$ ]] || height=0

        [[ "$first" == true ]] || printf ','
        first=false
        printf '\n    %s' "$(_pane_build_json_object \
            "$idx" "$pane_id" "$cmd" "$path" "$width" "$height" \
            "$( [[ "$idx" == "$current_pane" ]] && printf 'true' || printf 'false' )" \
            "$( [[ "$dead" == "1" ]] && printf 'true' || printf 'false' )" \
            "$agent_name" "$agent_status" "$agent_source" "$agent_confidence" "$agent_reason")"
    done <<< "$panes"

    if [[ "$first" == false ]]; then
        printf '\n  '
    fi
}

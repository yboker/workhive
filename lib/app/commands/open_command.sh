#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Open Command
# Create/enter Worktree (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_OPEN_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _OPEN_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/workspace_service.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/utils/tui.sh"
source "${LIB_DIR}/app/shared/worktree_collector.sh"
source "${LIB_DIR}/app/shared/worktree_selector.sh"
source "${LIB_DIR}/app/shared/command_builder.sh"
source "${LIB_DIR}/app/commands/gc_command.sh"
source "${LIB_DIR}/utils/progress.sh"

# Default attach delay: 0 (immediate attach after readiness).
# Operators can override via config key `tmux.attach_delay`.
readonly _OPEN_DELAY_BEFORE_ATTACH=0
readonly _OPEN_PANE_SUMMARY_TIMEOUT_SEC=2

# Display usage help
open_command_help() {
    _help_dispatch "_open_command_help"
}

# Execute open command
# Arguments: $@ = command line arguments
open_command_execute() {
    local branch=""
    local auto_attach=true
    local worktree_mode=true
    local force_recreate=false

    # Try to get project root (Git repository root)
    # Fall back to current directory if not a git repo
    local project_root
    project_root=$(_git_root)

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --no-attach)
                auto_attach=false
                shift
                ;;
            --force|-f)
                force_recreate=true
                shift
                ;;
            --force-layout)
                # Backward compatibility: treat force-layout as strict force rebuild.
                force_recreate=true
                shift
                ;;
            -h|--help)
                open_command_help
                return 0
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                open_command_help
                return 1
                ;;
            *)
                if [[ -z "$branch" ]]; then
                    branch="$1"
                else
                    log_error "$(_i18n ERR_ONE_BRANCH_ONLY)"
                    return 1
                fi
                shift
                ;;
        esac
    done

    # Step 1: determine target Worktree
    if [[ -z "$branch" ]]; then
        local select_exit=0
        branch=$(_open_command_interactive_select) || select_exit=$?

        if [[ $select_exit -ne 0 || -z "$branch" ]]; then
            log_info "$(_i18n MSG_CANCELLED)"
            return 0
        elif [[ "$branch" == "__MAIN_DIR__" ]]; then
            worktree_mode=false
            branch=$(_resolve_current_branch_name)
            if [[ -z "$branch" ]]; then
                log_error "$(_i18n ERR_BRANCH_EMPTY)"
                return 1
            fi
            log_info "$(_i18n INFO_MAIN_DIR_MODE) ($branch)"
        fi
    else
        local main_repo_branch=""
        main_repo_branch=$(_git_current_branch)
        if [[ -n "$main_repo_branch" && "$branch" == "$main_repo_branch" ]]; then
            worktree_mode=false
            echo ""
            log_warn "$(_i18n INFO_MAIN_BRANCH_MODE)"
            log_info "  → Using project root directory (no worktree isolation)"
            echo ""
        fi
    fi

    # Branch name sanitization: warn and confirm if name will be converted
    if [[ "$worktree_mode" == true ]] && _branch_needs_sanitize "$branch"; then
        if [[ "$force_recreate" != true ]]; then
            if ! _branch_confirm_sanitize "$branch"; then
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
        fi
    fi

    # Calculate subpath offset (user cwd relative to git root)
    # e.g.: git root = /a, cwd = /a/b -> offset = "b"
    local subpath_offset
    subpath_offset=$(_git_subpath_offset)

    if ! git rev-parse HEAD &>/dev/null; then
        log_warn "$(_i18n MSG_GIT_NO_COMMITS)"
        echo ""
        if [[ "$force_recreate" == true ]] || ask_yes_no "$(_i18n ASK_CREATE_INITIAL_COMMIT)" "Y"; then
            echo ""
            if ! git commit --allow-empty -m "Initial commit" &>/dev/null; then
                log_error "$(_i18n ERR_GIT_COMMIT_FAILED)"
                return 1
            fi
            log_success "$(_i18n MSG_GIT_INIT_SUCCESS)"
            echo ""
        else
            echo ""
            log_info "$(_i18n ERR_RUN_INIT)"
            echo "  workhive init"
            return 1
        fi
    fi

    _ensure_workhive_dir

    # Worktree mode: confirm creation when worktree does not exist
    if [[ "$worktree_mode" == true ]] && ! worktree_exists "$branch"; then
        if [[ "$force_recreate" != true ]]; then
            local _confirm_msg
            _confirm_msg=$(_i18n_fmt ASK_CREATE_WT "$branch")
            echo ""
            if ! ask_yes_no "$_confirm_msg" "Y"; then
                echo ""
                log_info "$(_i18n MSG_CANCELLED)"
                return 0
            fi
            echo ""
        fi
    fi

    log_bold "🚀 $(_i18n INFO_OPENING_WT) $branch"
    echo ""

    local _open_t_total; _open_t_total=$(perf_timer_start)

    # ──────────────────────────────────────────────────
    # Steps 1-2: Working directory + session (Domain layer transactional orchestration)
    # ──────────────────────────────────────────────────
    local _open_t_ws; _open_t_ws=$(perf_timer_start)
    local _open_ws_rc=0
    workspace_open "$branch" "$worktree_mode" "$project_root" "$force_recreate" "false" "$force_recreate" || _open_ws_rc=$?
    perf_timer_log "open.workspace_open" "$_open_t_ws" "branch=${branch} rc=${_open_ws_rc}"
    if [[ "$_open_ws_rc" != "0" ]]; then
        return 1
    fi

    # workdir_base: worktree/project root directory (used for session creation)
    # workdir: actual directory pane commands cd to (with subpath offset appended)
    local workdir_base="$WORKSPACE_RESULT_DIR"
    local workdir="$workdir_base"
    if [[ -n "$subpath_offset" ]]; then
        local offset_dir="${workdir_base}/${subpath_offset}"
        if [[ -d "$offset_dir" ]]; then
            workdir="$offset_dir"
        else
            log_debug "Subpath offset '${subpath_offset}' not found in workdir, using root"
        fi
    fi
    local session_name
    session_name=$(session_get_name "$branch")

    # ──────────────────────────────────────────────────
    # Step 3: deterministic open strategy
    # 1) existing session -> use as-is
    # 2) restored snapshot -> use as-is
    # 3) otherwise create from config.yaml
    # force (-f): always rebuild from config.yaml
    # ──────────────────────────────────────────────────
    local _open_t_layout; _open_t_layout=$(perf_timer_start)
    local _layout_source="${WORKSPACE_OPEN_STRATEGY:-config}"
    case "${WORKSPACE_OPEN_STRATEGY:-config}" in
    config)
        log_info "$(_i18n STEP_3_4) $(_i18n STEP_3_LAYOUT) (columns)..."
        log_info "$(_i18n INFO_LAYOUT_SOURCE): $(_i18n INFO_LAYOUT_SOURCE_CONFIG)"
        if ! _open_build_layout_from_config "$branch" "$workdir_base" "$workdir"; then
            return 1
        fi
        _layout_source="config"
        ;;
    session)
        log_info "$(_i18n STEP_3_4) $(_i18n STEP_3_LAYOUT) (columns)..."
        log_info "$(_i18n INFO_LAYOUT_SOURCE): $(_i18n INFO_LAYOUT_SOURCE_SESSION)"
        local _current_panes
        _current_panes=$(_open_current_pane_count "$branch")
        log_success "$(_i18n MSG_LAYOUT_EXISTS_SKIP) (${_current_panes} pane)"
        echo ""
        _layout_source="session"
        ;;
    snapshot)
        log_info "$(_i18n STEP_3_4) $(_i18n STEP_3_LAYOUT) (snapshot)..."
        log_info "$(_i18n INFO_LAYOUT_SOURCE): $(_i18n INFO_LAYOUT_SOURCE_SNAPSHOT)"
        log_success "$(_i18n PERSIST_LAYOUT_SKIP_SNAPSHOT)"
        echo ""
        _layout_source="snapshot"
        ;;
    *)
        log_error "Unsupported open strategy: ${WORKSPACE_OPEN_STRATEGY:-<empty>}"
        return 1
        ;;
    esac

    # Force mode contract: always rebuild from config.yaml.
    if [[ "$force_recreate" == true && "$_layout_source" != "config" ]]; then
        log_info "$(_i18n STEP_3_4) $(_i18n STEP_3_LAYOUT) (columns)..."
        log_info "$(_i18n INFO_LAYOUT_SOURCE): $(_i18n INFO_LAYOUT_SOURCE_CONFIG)"
        if ! _open_build_layout_from_config "$branch" "$workdir_base" "$workdir"; then
            return 1
        fi
        _layout_source="config"
    fi

    perf_timer_log "open.layout" "$_open_t_layout" "source=${_layout_source}"

    _open_gc_hint

    if [[ "$auto_attach" == true ]]; then
        log_info "$(_i18n STEP_4_4) $(_i18n MSG_ATTACHING)"
        log_success "$(_i18n MSG_WORKSPACE_READY)"
        echo ""

        _open_print_workspace_summary "$branch" "$workdir" "$session_name" "$_layout_source"

        local _attach_delay
        _attach_delay=$(config_get "tmux.attach_delay" 2>/dev/null || true)
        _is_null_or_empty "$_attach_delay" && _attach_delay="${_OPEN_DELAY_BEFORE_ATTACH}s"
        local _attach_delay_seconds
        _attach_delay_seconds=$(duration_to_seconds "$_attach_delay" 2>/dev/null || echo "")
        [[ "$_attach_delay_seconds" =~ ^[0-9]+$ ]] || _attach_delay_seconds="$_OPEN_DELAY_BEFORE_ATTACH"

        log_info "$(_i18n HINT_DETACH)"
        if [[ "$_attach_delay_seconds" -gt 0 ]]; then
            sleep "$_attach_delay_seconds"
        fi

        perf_timer_log "open.total" "$_open_t_total" "branch=${branch} strategy=${_layout_source} delay=${_attach_delay_seconds}"
        session_apply_deferred_async "$session_name"
        session_attach "$branch"
    else
        log_info "$(_i18n STEP_4_4) $(_i18n STEP_4_SKIP_ATTACH)"
        session_apply_deferred "$session_name"
        log_success "$(_i18n MSG_WORKSPACE_CREATED)"
        echo ""
        log_info "$(_i18n MSG_USE_ATTACH)"
        echo "  workhive attach $branch"
    fi
}

_open_build_layout_from_config() {
    local branch="$1"
    local layout_workdir="$2"
    local pane_workdir="$3"
    local _expected_panes
    _expected_panes=$(_layout_expected_pane_count)

    log_info "$(_i18n MSG_CREATE_LAYOUT) (columns)..."
    if ! layout_create "$branch" "$layout_workdir" "$pane_workdir"; then
        log_error "$(_i18n ERR_LAYOUT_CREATE_FAILED)"
        return 1
    fi
    log_success "$(_i18n MSG_LAYOUT_CREATED) ($_expected_panes pane)"
    if ! _open_apply_pane_commands "$branch" "$pane_workdir"; then
        return 1
    fi
    layout_select_pane "$branch" 0
    echo ""
    return 0
}

# ──────────────────────────────────────────────────
# GC hint: detect merged branches and prompt user to run gc
# ──────────────────────────────────────────────────
_open_gc_hint() {
    local merged_count=0
    local _gc_branch
    while IFS= read -r _gc_branch; do
        [[ -z "$_gc_branch" ]] && continue
        merged_count=$((merged_count + 1))
    done < <(_gc_get_merged_branches 2>/dev/null)

    if [[ $merged_count -gt 0 ]]; then
        echo ""
        log_info "$(_i18n_fmt GC_HINT_MERGED_FMT "$merged_count")"
    fi
}

_open_current_pane_count() {
    local branch="$1"
    local session_name
    session_name=$(session_get_name "$branch")
    local pane_count
    pane_count=$(terminal_adapter_count_panes "$session_name" 2>/dev/null || echo "0")
    [[ "$pane_count" =~ ^[0-9]+$ ]] || pane_count=0
    echo "$pane_count"
}

# Apply pane commands from config.yaml (single source of truth).
_open_apply_pane_commands() {
    local branch="$1"
    local workdir="$2"

    log_info "$(_i18n MSG_APPLY_CMD_CONFIG)..."
    if layout_apply_commands "$branch" "$workdir"; then
        log_success "$(_i18n MSG_CMD_CONFIG_APPLIED)"
        return 0
    else
        log_warn "$(_i18n WARN_CMD_CONFIG_FAILED)"
        return 1
    fi
}

# Print workspace summary (branch, path, layout, pane commands)
_open_print_workspace_summary() {
    local branch="$1"
    local workdir="$2"
    local session_name="$3"
    local layout_source="${4:-config}"
    local layout_source_label="$(_i18n INFO_LAYOUT_SOURCE_CONFIG)"
    case "$layout_source" in
        session)  layout_source_label="$(_i18n INFO_LAYOUT_SOURCE_SESSION)" ;;
        snapshot) layout_source_label="$(_i18n INFO_LAYOUT_SOURCE_SNAPSHOT)" ;;
    esac

    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    log_info "$(_i18n COL_BRANCH): $branch"
    log_info "$(_i18n COL_PATH): $workdir"
    log_info "$(_i18n COL_SESSION): $session_name"
    log_info "$(_i18n INFO_LAYOUT_SOURCE): $layout_source_label"
    echo ""

    log_info "$(_i18n COL_LAYOUT) (columns):"

    _open_print_pane_lines "$branch"

    log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
}

# Format and print a single pane info line (label + command or "empty shell")
_open_print_pane_line() {
    local label="$1" cmd="$2"
    local display_cmd
    display_cmd=$(_cmd_display_label "$cmd")
    local line="  • $label: $display_cmd"
    echo "$line"
    _log_to_file "INFO" "$line"
}

_open_print_pane_lines() {
    local branch="${1:-}"
    local pane_lines=""

    pane_lines=$(_open_collect_pane_lines "$branch")
    if [[ -z "$pane_lines" ]]; then
        _open_print_pane_summary_unavailable
        return 0
    fi

    local _role _cmd _label
    while IFS='|' read -r _role _cmd; do
        [[ -z "$_role" ]] && continue
        _label=$(layout_pane_role_label "$_role")
        _open_print_pane_line "$_label" "$_cmd"
    done <<< "$pane_lines"
    return 0
}

_open_print_pane_summary_unavailable() {
    local line="  • $(_i18n MSG_PANE_SUMMARY_UNAVAILABLE)"
    echo "$line"
    _log_to_file "INFO" "$line"
}

_open_collect_pane_lines() {
    local branch="${1:-}"
    local pane_lines=""
    local strategy="${WORKSPACE_OPEN_STRATEGY:-config}"

    if [[ -n "$branch" && ( "$strategy" == "snapshot" || "$strategy" == "session" ) ]]; then
        pane_lines=$(_open_collect_runtime_pane_lines_with_timeout "$branch" "$_OPEN_PANE_SUMMARY_TIMEOUT_SEC")
        local runtime_rc=$?
        if [[ $runtime_rc -ne 0 ]]; then
            log_debug "Runtime pane summary skipped (rc=${runtime_rc}), fallback to config pane summary"
        fi
        if [[ "$strategy" == "snapshot" && -n "$pane_lines" ]] && ! _open_runtime_pane_has_active_commands "$branch" "$pane_lines"; then
            pane_lines=""
        fi
    fi

    if [[ -z "$pane_lines" ]]; then
        pane_lines=$(layout_collect_pane_commands 2>/dev/null || true)
    fi

    if [[ -z "$pane_lines" && -n "$branch" ]]; then
        pane_lines=$(_open_collect_runtime_raw_pane_lines_with_timeout "$branch" "$_OPEN_PANE_SUMMARY_TIMEOUT_SEC")
    fi

    printf '%s\n' "$pane_lines"
    return 0
}

_open_collect_runtime_pane_lines_with_timeout() {
    local branch="${1:-}"
    local timeout_sec="${2:-$_OPEN_PANE_SUMMARY_TIMEOUT_SEC}"
    [[ "$timeout_sec" =~ ^[0-9]+$ ]] || timeout_sec="$_OPEN_PANE_SUMMARY_TIMEOUT_SEC"

    local tmp_file
    tmp_file=$(mktemp "/tmp/workhive-open-pane-lines.XXXXXX") || return 1

    (
        layout_collect_runtime_pane_commands "$branch"
    ) > "$tmp_file" 2>/dev/null &
    local collector_pid=$!

    local waited=0
    local max_ticks=$((timeout_sec * 20))
    while kill -0 "$collector_pid" 2>/dev/null; do
        if (( waited >= max_ticks )); then
            kill "$collector_pid" 2>/dev/null || true
            wait "$collector_pid" 2>/dev/null || true
            cleanup_file_quiet "$tmp_file"
            return 124
        fi
        sleep 0.05
        waited=$((waited + 1))
    done

    wait "$collector_pid" 2>/dev/null || true
    local pane_lines=""
    pane_lines=$(<"$tmp_file")
    cleanup_file_quiet "$tmp_file"
    [[ -n "$pane_lines" ]] && printf '%s\n' "$pane_lines"
    return 0
}

_open_collect_runtime_raw_pane_lines_with_timeout() {
    local branch="${1:-}"
    local timeout_sec="${2:-$_OPEN_PANE_SUMMARY_TIMEOUT_SEC}"
    [[ "$timeout_sec" =~ ^[0-9]+$ ]] || timeout_sec="$_OPEN_PANE_SUMMARY_TIMEOUT_SEC"

    local tmp_file
    tmp_file=$(mktemp "/tmp/workhive-open-pane-lines-raw.XXXXXX") || return 1

    (
        _open_collect_runtime_raw_pane_lines "$branch"
    ) > "$tmp_file" 2>/dev/null &
    local collector_pid=$!

    local waited=0
    local max_ticks=$((timeout_sec * 20))
    while kill -0 "$collector_pid" 2>/dev/null; do
        if (( waited >= max_ticks )); then
            kill "$collector_pid" 2>/dev/null || true
            wait "$collector_pid" 2>/dev/null || true
            cleanup_file_quiet "$tmp_file"
            return 124
        fi
        sleep 0.05
        waited=$((waited + 1))
    done

    wait "$collector_pid" 2>/dev/null || true
    local pane_lines=""
    pane_lines=$(<"$tmp_file")
    cleanup_file_quiet "$tmp_file"
    [[ -n "$pane_lines" ]] && printf '%s\n' "$pane_lines"
    return 0
}

_open_collect_runtime_raw_pane_lines() {
    local branch="${1:-}"
    [[ -z "$branch" ]] && return 1

    local session_name
    session_name=$(session_get_name "$branch")
    [[ -z "$session_name" ]] && return 1

    if declare -f pane_meta_has_metadata >/dev/null 2>&1 && \
       pane_meta_has_metadata "$session_name" "0" 2>/dev/null; then
        _open_collect_runtime_raw_from_metadata "$session_name"
        return $?
    fi

    _open_collect_runtime_raw_from_tmux "$session_name"
}

_open_collect_runtime_raw_from_metadata() {
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
        printf '%s|%s\n' "$role" "${desired_cmd:-}"
        emitted=$((emitted + 1))
    done <<< "$pane_indices"

    [[ $emitted -gt 0 ]]
}

_open_collect_runtime_raw_from_tmux() {
    local session_name="$1"

    # Diagnostic fallback: uses pane_current_command (NOT the source of truth).
    local runtime_lines
    runtime_lines=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}|#{pane_current_command}' 2>/dev/null || true)
    [[ -z "$runtime_lines" ]] && return 1

    local _idx _cmd
    local emitted=0
    while IFS='|' read -r _idx _cmd; do
        [[ -z "${_idx:-}" ]] && continue
        [[ "$_idx" =~ ^[0-9]+$ ]] || continue
        printf 'pane_%s|%s\n' "$_idx" "$_cmd"
        emitted=$((emitted + 1))
    done <<< "$runtime_lines"

    [[ $emitted -gt 0 ]]
}

_open_runtime_pane_has_active_commands() {
    local branch="${1:-}"
    local pane_lines="${2:-}"

    local session_name=""
    if [[ -n "$branch" ]]; then
        session_name=$(session_get_name "$branch" 2>/dev/null || echo "")
    fi

    if [[ -n "$session_name" ]] && declare -f pane_meta_has_metadata >/dev/null 2>&1 && \
       pane_meta_has_metadata "$session_name" "0" 2>/dev/null; then
        local pane_indices
        pane_indices=$(terminal_adapter_list_panes_format "$session_name" '#{pane_index}' 2>/dev/null || true)
        local _idx
        while IFS= read -r _idx; do
            [[ -z "${_idx:-}" || ! "$_idx" =~ ^[0-9]+$ ]] && continue
            local state
            state=$(pane_meta_get "$session_name" "$_idx" "state" 2>/dev/null || true)
            [[ "$state" == "running" || "$state" == "launching" ]] && return 0
        done <<< "$pane_indices"
        return 1
    fi

    _open_pane_lines_has_non_shell_command "$pane_lines"
}

_open_is_shell_command_name() {
    local cmd="${1:-}"
    cmd=$(basename "$cmd" 2>/dev/null || echo "$cmd")
    case "$cmd" in
        bash|zsh|sh|fish|dash|ksh|tcsh|csh) return 0 ;;
        *) return 1 ;;
    esac
}

_open_pane_lines_has_non_shell_command() {
    # Diagnostic fallback: infers from pane_current_command (NOT source of truth).
    local pane_lines="${1:-}"
    local _role _cmd
    while IFS='|' read -r _role _cmd; do
        [[ -z "$_role" ]] && continue
        _cmd="${_cmd#"${_cmd%%[![:space:]]*}"}"
        _cmd="${_cmd%"${_cmd##*[![:space:]]}"}"
        [[ -z "$_cmd" ]] && continue
        if ! _open_is_shell_command_name "$_cmd"; then
            return 0
        fi
    done <<< "$pane_lines"
    return 1
}

# Resolve the current branch name for main directory mode.
# Falls back to detached HEAD short hash, then directory basename.
_resolve_current_branch_name() {
    local name=""
    name=$(_git_current_branch)
    if [[ -z "$name" ]] && git rev-parse --short HEAD &>/dev/null; then
        name="detached-$(git rev-parse --short HEAD 2>/dev/null)"
    fi
    [[ -z "$name" ]] && name="${PWD##*/}"
    echo "$name"
}

_open_command_interactive_select() {
    worktree_interactive_select \
        --with-main-dir \
        --with-create-new \
        --title "$(_i18n MSG_SELECT_OR_CREATE_WT)"
}

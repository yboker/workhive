#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# List Command
# Global dashboard: display comprehensive status of all Worktrees (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_LIST_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _LIST_COMMAND_SH_LOADED=1

# Dependencies
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/progress.sh"
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/domain/services/workspace_status_service.sh"
source "${LIB_DIR}/domain/services/worktree_status_cache_service.sh"
source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/persistence/manager.sh"
source "${LIB_DIR}/app/shared/layout_visual.sh"
source "${LIB_DIR}/app/shared/command_builder.sh"

# Collection strategy: controls git dirty status computation cost.
#   full — include `git status --porcelain` per worktree (interactive table)
#   fast — skip dirty status, show only existence/session/activity
#          (--watch mode, --format json machine-readable output)
readonly _STATUS_STRATEGY_FULL="full"
readonly _STATUS_STRATEGY_FAST="fast"

# Display usage help
list_command_help() {
    _help_dispatch "_list_command_help"
}

list_command_execute() {
    _list_command_execute "$@"
}

_status_project_root() {
    local project_root="${_WORKHIVE_GIT_ROOT:-}"
    if [[ -z "$project_root" ]]; then
        project_root=$(_git_root 2>/dev/null || pwd)
    fi
    printf '%s\n' "$project_root"
}

# ──────────────────────────────────────────────────
# Collect all branch data (used by output format functions)
# Output: global arrays _STATUS_BRANCHES, _STATUS_DATA (associative array)
# Perf: uses 2 bulk tmux queries instead of N per-branch calls
# ──────────────────────────────────────────────────
declare -ga _STATUS_BRANCHES=()
declare -g _STATUS_BASELINE_BRANCH=""

# Associative array: key = "branch:field", value = value
declare -gA _STATUS_DATA=()

_status_reset_state() {
    _STATUS_BRANCHES=()
    unset _STATUS_DATA
    declare -gA _STATUS_DATA=()
    _STATUS_BASELINE_BRANCH=""
}

_status_warm_caches() {
    # Resolve once in parent shell to avoid repeated subprocesses.
    declare -g _WORKHIVE_GIT_ROOT=""
    declare -g _WORKHIVE_DIR_CACHED=""
    declare -g _SESSION_PREFIX_CACHED=""
    declare -g _WORKTREE_BASE_PATH_CACHED=""

    _WORKHIVE_GIT_ROOT=$(_git_root 2>/dev/null) || true
    _WORKHIVE_DIR_CACHED=$(_workhive_dir 2>/dev/null) || true
    _SESSION_PREFIX_CACHED=$(session_get_prefix)
    _WORKTREE_BASE_PATH_CACHED=$(worktree_get_base_path 2>/dev/null) || true
    _layout_normalize 2>/dev/null || true
}

_status_collect_data() {
    _status_collect_data_with_strategy "$_STATUS_STRATEGY_FULL"
}

_status_collect_data_with_strategy() {
    local strategy="${1:-$_STATUS_STRATEGY_FULL}"
    local t_total; t_total=$(perf_timer_start)

    _status_reset_state
    _status_warm_caches

    local project_root
    project_root=$(_status_project_root)

    # ── Phase 1: Bulk tmux session + pane collection (single source) ──
    workspace_status_reset
    workspace_status_collect_sessions "$project_root"

    # ── Phase 2: Bulk worktree mapping (single git worktree list) ─────
    workspace_status_collect_worktrees

    # ── Phase 3: Build project-family session list from bulk data ──────
    local family_prefix
    family_prefix=$(_session_get_family_prefix)

    local sessions=""
    local _s
    for _s in "${!_WSS_SESSION_IN_PROJECT[@]}"; do
        [[ "$_s" == "${family_prefix}"* ]] && sessions+="${_s}"$'\n'
    done
    if [[ -z "$sessions" ]]; then
        sessions=$(session_list_project_family 2>/dev/null || true)
    fi

    # ── Phase 4: Merge branches from worktree mapping + sessions ──────
    local -A seen_branches
    local branch session

    for branch in $(workspace_status_managed_branches); do
        [[ -z "$branch" ]] && continue
        if [[ -z "${seen_branches[$branch]:-}" ]]; then
            _STATUS_BRANCHES+=("$branch")
            seen_branches[$branch]=1
        fi
    done

    while IFS= read -r session; do
        [[ -z "$session" ]] && continue
        local raw_branch
        raw_branch=$(session_extract_branch "$session")
        branch="$raw_branch"
        local real_branch
        real_branch=$(workspace_status_branch_for_sanitized "$raw_branch")
        [[ -n "$real_branch" ]] && branch="$real_branch"
        if [[ -z "${seen_branches[$branch]:-}" ]]; then
            _STATUS_BRANCHES+=("$branch")
            seen_branches[$branch]=1
        fi
    done <<< "$sessions"

    _STATUS_BASELINE_BRANCH=$(_git_main_branch 2>/dev/null || _git_current_branch 2>/dev/null || echo "")
    if [[ -n "$_STATUS_BASELINE_BRANCH" && -z "${seen_branches[$_STATUS_BASELINE_BRANCH]:-}" ]]; then
        _STATUS_BRANCHES+=("$_STATUS_BASELINE_BRANCH")
        seen_branches["$_STATUS_BASELINE_BRANCH"]=1
    fi

    if [[ -n "$_STATUS_BASELINE_BRANCH" ]]; then
        local -a _ordered_branches=()
        local _b
        _ordered_branches+=("$_STATUS_BASELINE_BRANCH")
        for _b in "${_STATUS_BRANCHES[@]}"; do
            [[ "$_b" == "$_STATUS_BASELINE_BRANCH" ]] && continue
            _ordered_branches+=("$_b")
        done
        _STATUS_BRANCHES=("${_ordered_branches[@]}")
    fi

    # ── Phase 5: Build per-branch session aggregates ──────────────────
    local -A _branch_session_count
    local -A _branch_active_count
    local -A _branch_detached_count
    local -A _branch_session_details

    while IFS= read -r session; do
        [[ -z "$session" ]] && continue

        local raw_branch session_attached session_status
        raw_branch=$(session_extract_branch "$session")
        branch="$raw_branch"
        local real_branch
        real_branch=$(workspace_status_branch_for_sanitized "$raw_branch")
        [[ -n "$real_branch" ]] && branch="$real_branch"

        session_attached=$(workspace_status_session_attached "$session")
        if [[ "$session_attached" == "1" ]]; then
            session_status="active"
        else
            session_status="detached"
        fi

        _branch_session_count["$branch"]=$(( ${_branch_session_count["$branch"]:-0} + 1 ))
        if [[ "$session_status" == "active" ]]; then
            _branch_active_count["$branch"]=$(( ${_branch_active_count["$branch"]:-0} + 1 ))
        else
            _branch_detached_count["$branch"]=$(( ${_branch_detached_count["$branch"]:-0} + 1 ))
        fi
        _branch_session_details["$branch"]+="${session}|${session_status}"$'\n'
    done <<< "$sessions"

    # ── Phase 6: Per-branch status (git via cache, no per-session tmux) ─
    local t_git; t_git=$(perf_timer_start)
    local _expected_panes
    _expected_panes=$(_layout_expected_pane_count 2>/dev/null || echo "2")

    for branch in "${_STATUS_BRANCHES[@]}"; do
        local has_worktree="false"
        local wt_path="-"
        local session_status="-"
        local session_count=0
        local active_count=0
        local detached_count=0
        local has_layout="false"
        local git_status="-"
        local last_activity="-"

        # Worktree path from bulk mapping (no per-branch computation)
        wt_path=$(workspace_status_worktree_path_for_branch "$branch")
        if [[ -n "$wt_path" && -d "$wt_path" ]]; then
            has_worktree="true"
            if [[ "$strategy" == "$_STATUS_STRATEGY_FULL" ]]; then
                git_status=$(wt_status_cache_git_status "$wt_path")
            fi
            local commit_epoch
            commit_epoch=$(wt_status_cache_commit_epoch "$wt_path" "$branch" "$project_root")
            last_activity=$(format_relative_time "$commit_epoch")
        else
            wt_path="-"
            local commit_epoch
            commit_epoch=$(wt_status_cache_commit_epoch "-" "$branch" "$project_root")
            last_activity=$(format_relative_time "$commit_epoch")
        fi

        session_count=${_branch_session_count["$branch"]:-0}
        active_count=${_branch_active_count["$branch"]:-0}
        detached_count=${_branch_detached_count["$branch"]:-0}
        session_status=$(_status_session_state_from_counts "$session_count" "$active_count" "$detached_count")

        # Session details — activity from bulk snapshot, no per-session tmux call
        local session_details=""
        if [[ "$session_count" -gt 0 ]]; then
            local _line _session_name _session_status _session_pane_count _session_layout _session_activity_epoch _session_recent_activity
            while IFS= read -r _line; do
                [[ -z "$_line" ]] && continue
                _session_name="${_line%%|*}"
                _session_status="${_line#*|}"
                _session_pane_count=$(workspace_status_session_pane_count "$_session_name")
                _session_layout="false"
                if [[ "$_session_pane_count" -ge "$_expected_panes" ]]; then
                    _session_layout="true"
                    has_layout="true"
                fi
                _session_activity_epoch=$(workspace_status_session_activity "$_session_name")
                _session_recent_activity=$(format_relative_time "$_session_activity_epoch")
                session_details+="${_session_activity_epoch}|${_session_name}|${_session_status}|${_session_layout}|${_session_recent_activity}"$'\n'
            done <<< "${_branch_session_details[$branch]:-}"
        fi

        _STATUS_DATA["$branch:worktree"]="$has_worktree"
        _STATUS_DATA["$branch:path"]="$wt_path"
        _STATUS_DATA["$branch:session"]="$session_status"
        _STATUS_DATA["$branch:session_count"]="$session_count"
        _STATUS_DATA["$branch:session_active_count"]="$active_count"
        _STATUS_DATA["$branch:session_detached_count"]="$detached_count"
        _STATUS_DATA["$branch:session_details"]="$session_details"
        _STATUS_DATA["$branch:layout"]="$has_layout"
        _STATUS_DATA["$branch:git_status"]="$git_status"
        _STATUS_DATA["$branch:last_activity"]="$last_activity"
    done
    perf_timer_log "ls.git_status" "$t_git" "strategy=${strategy} branches=${#_STATUS_BRANCHES[@]}"
    perf_timer_log "ls.collect_total" "$t_total" "strategy=${strategy} branches=${#_STATUS_BRANCHES[@]}"
}

# ──────────────────────────────────────────────────
# Execute list command
# ──────────────────────────────────────────────────
_list_command_execute() {
    local format="table"
    local watch_mode=false
    local watch_interval=5
    local layout_detail=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                list_command_help
                return 0
                ;;
            --layout)
                layout_detail=true
                shift
                ;;
            --watch)
                watch_mode=true
                shift
                ;;
            --interval)
                local interval_value=""
                interval_value=$(cli_require_option_value_nonflag "--interval" "${2:-}") || return 1
                if ! [[ "$interval_value" =~ ^[0-9]+\.?[0-9]*$ ]] || [[ "$interval_value" == "0" ]]; then
                    log_error "$(_i18n STATUS_INTERVAL_NAN)"
                    return 1
                fi
                watch_interval="$interval_value"
                shift 2
                ;;
            --format)
                format=$(cli_parse_format_value "${2:-}" '^(table|json)$' 'table, json') || return 1
                shift 2
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                list_command_help
                return 1
                ;;
            *)
                cli_log_no_positional
                echo ""
                list_command_help
                return 1
                ;;
        esac
    done

    _status_validate_mode_options "$layout_detail" "$format" "$watch_mode" || return 1

    if [[ "$layout_detail" == true ]]; then
        _status_output_layout
        return $?
    fi

    # Collect data
    _status_collect_data_for_format "$format"

    # If no spaces found
    if [[ ${#_STATUS_BRANCHES[@]} -eq 0 ]]; then
        _status_render_empty_result "$format"
        return 0
    fi

    # Output data
    _status_output_by_format "$format" "normal"

    if [[ "$watch_mode" == true ]]; then
        _status_watch_loop "$format" "$watch_interval"
        echo ""
        return 0
    fi
}

_status_validate_mode_options() {
    local layout_detail="${1:-false}"
    local format="${2:-table}"
    local watch_mode="${3:-false}"

    # --layout cannot be combined with --format or --watch
    if [[ "$layout_detail" == true && ( "$format" != "table" || "$watch_mode" == true ) ]]; then
        log_error "$(_i18n ERR_LAYOUT_MUTEX)"
        return 1
    fi

    # --watch in json mode would break machine-readable output contract
    if [[ "$watch_mode" == true ]] && _status_is_json_format "$format"; then
        log_error "$(_i18n ERR_WATCH_JSON_MUTEX)"
        return 1
    fi
    return 0
}

_status_is_json_format() {
    local format="${1:-table}"
    [[ "$format" == "json" ]]
}

_status_should_show_progress() {
    local format="${1:-table}"
    ! _status_is_json_format "$format"
}

_status_render_empty_table_hint() {
    log_info "$(_i18n MSG_NO_ACTIVE_WT)"
    echo ""
    log_info "$(_i18n MSG_CREATE_NEW_SPACE)"
    echo "  workhive open <worktree>"
}

_status_render_empty_result() {
    local format="${1:-table}"
    if _status_is_json_format "$format"; then
        _status_output_json
        return 0
    fi
    _status_render_empty_table_hint
}

_status_watch_loop() {
    local format="${1:-table}"
    local watch_interval="${2:-5}"
    (
        local _status_watch_interrupted=false
        _status_watch_cleanup() {
            trap - INT
        }
        trap '_status_watch_cleanup' EXIT
        trap '_status_watch_interrupted=true' INT
        while true; do
            _status_render_watch_frame "$format" "$watch_interval"
            sleep "$watch_interval" || true
            [[ "$_status_watch_interrupted" == true ]] && break
        done
        _status_watch_cleanup
    )
}

_status_render_watch_footer() {
    local watch_interval="${1:-5}"
    echo ""
    log_info "$(_i18n STATUS_REFRESHING_EVERY) ${watch_interval}$(_i18n STATUS_SECONDS) ($(_i18n STATUS_CTRL_C_STOP))"
}

_status_render_watch_body() {
    local format="${1:-table}"
    _status_collect_data_with_strategy "$_STATUS_STRATEGY_FAST"
    if [[ ${#_STATUS_BRANCHES[@]} -gt 0 ]]; then
        _status_output_by_format "$format" "watch"
    else
        log_info "$(_i18n MSG_NO_ACTIVE_WORKSPACES)"
    fi
}

_status_render_watch_frame() {
    local format="${1:-table}"
    local watch_interval="${2:-5}"
    clear
    _status_render_watch_body "$format"
    _status_render_watch_footer "$watch_interval"
}

_status_select_strategy() {
    local format="${1:-table}"
    if _status_is_json_format "$format"; then
        printf '%s' "$_STATUS_STRATEGY_FAST"
    else
        printf '%s' "$_STATUS_STRATEGY_FULL"
    fi
}

_status_collect_data_for_format() {
    local format="${1:-table}"
    local strategy
    strategy=$(_status_select_strategy "$format")
    local show_progress=false
    if _status_should_show_progress "$format"; then
        show_progress=true
    fi
    if [[ "$show_progress" == true ]]; then
        progress_start "$(_i18n MSG_LOADING_WORKSPACES 2>/dev/null || echo 'Loading workspaces...')"
    fi
    _status_collect_data_with_strategy "$strategy"
    if [[ "$show_progress" == true ]]; then
        progress_stop
    fi
}

_status_output_watch_mode() {
    local format="${1:-table}"
    case "$format" in
        json)   _status_output_json ;;
        *)      _status_output_table_compact ;;
    esac
}

_status_output_normal_mode() {
    local format="${1:-table}"
    if _status_is_json_format "$format"; then
        _status_output_json
    else
        _status_output_table
    fi
}

_status_output_by_format() {
    local format="${1:-table}"
    local mode="${2:-normal}"

    if [[ "$mode" == "watch" ]]; then
        _status_output_watch_mode "$format"
        return
    fi

    _status_output_normal_mode "$format"
}

_status_session_totals() {
    local active_count=0
    local detached_count=0
    local branch
    for branch in "${_STATUS_BRANCHES[@]}"; do
        active_count=$((active_count + ${_STATUS_DATA["$branch:session_active_count"]:-0}))
        detached_count=$((detached_count + ${_STATUS_DATA["$branch:session_detached_count"]:-0}))
    done
    printf '%s|%s\n' "$active_count" "$detached_count"
}

_status_worktree_mark() {
    local has_worktree="${1:-false}"
    _status_bool_mark "$has_worktree"
}

_status_bool_mark() {
    local value="${1:-false}"
    if [[ "$value" == "true" ]]; then
        echo "✓"
    else
        echo "✗"
    fi
}

_status_render_workspace_row() {
    local branch="$1"
    local has_wt="${_STATUS_DATA["$branch:worktree"]}"
    local session_st="${_STATUS_DATA["$branch:session"]}"
    local session_count="${_STATUS_DATA["$branch:session_count"]:-0}"
    local session_active_count="${_STATUS_DATA["$branch:session_active_count"]:-0}"
    local session_detached_count="${_STATUS_DATA["$branch:session_detached_count"]:-0}"
    local wt_mark
    wt_mark=$(_status_worktree_mark "$has_wt")

    local session_display
    session_display=$(_status_session_display "$session_st" "$session_count" "$session_active_count" "$session_detached_count")

    local git_st="${_STATUS_DATA["$branch:git_status"]}"
    local activity="${_STATUS_DATA["$branch:last_activity"]}"
    printf "  %s %s %s %s %s\n" \
        "$(_wpad 20 "$branch")" "$(_wpad 10 "$wt_mark")" "$(_wpad 18 "$session_display")" \
        "$(_wpad 14 "$git_st")" "$activity"
}

_status_render_table_header() {
    local active_count="${1:-0}"
    local detached_count="${2:-0}"
    local total_count="${3:-0}"

    log_bold "📊 $(_i18n STATUS_TITLE) (${total_count} $(_i18n MSG_COUNT))"
    echo ""
    echo -e "  $(_i18n STATUS_SUMMARY): ${GREEN}${active_count}${NC} $(_i18n STATUS_ACTIVE), ${YELLOW}${detached_count}${NC} $(_i18n STATUS_DETACHED), ${total_count} $(_i18n STATUS_TOTAL)"
    echo ""
    printf "  %s %s %s %s %s\n" \
        "$(_wpad 20 "$(_i18n COL_BRANCH)")" "$(_wpad 10 "$(_i18n COL_WORKTREE)")" "$(_wpad 18 "$(_i18n COL_SESSION)")" \
        "$(_wpad 14 "$(_i18n STATUS_COL_GIT)")" "$(_i18n STATUS_COL_ACTIVITY)"
    echo "  ──────────────────────────────────────────────────────────────────────────────────────────"
}

# ──────────────────────────────────────────────────
# Shared table rendering (DRY: used by both table and compact)
# ──────────────────────────────────────────────────
_status_render_summary_header() {
    local active_count detached_count
    IFS='|' read -r active_count detached_count <<< "$(_status_session_totals)"
    _status_render_table_header "$active_count" "$detached_count" "${#_STATUS_BRANCHES[@]}"
}

_status_render_rows() {
    _status_render_summary_header

    for branch in "${_STATUS_BRANCHES[@]}"; do
        _status_render_workspace_row "$branch"
    done
}

_status_session_label() {
    case "$1" in
        active)   echo "● $(_i18n STATUS_ACTIVE)" ;;
        detached) echo "○ $(_i18n STATUS_DETACHED)" ;;
        *)        echo "◐ $(_i18n STATUS_MIXED)" ;;
    esac
}

_status_session_none_label() {
    echo "- none"
}

_status_session_state_from_counts() {
    local session_count="${1:-0}"
    local active_count="${2:-0}"
    local detached_count="${3:-0}"

    if [[ "$session_count" -le 0 ]]; then
        echo "-"
    elif [[ "$active_count" -gt 0 && "$detached_count" -eq 0 ]]; then
        echo "active"
    elif [[ "$active_count" -eq 0 && "$detached_count" -gt 0 ]]; then
        echo "detached"
    else
        echo "mixed"
    fi
}

_status_session_display() {
    local session_st="${1:-}"
    local session_count="${2:-0}"
    local session_active_count="${3:-0}"
    local session_detached_count="${4:-0}"

    if [[ "$session_count" -eq 0 ]]; then
        _status_session_none_label
        return 0
    fi

    if [[ "$session_count" -eq 1 ]]; then
        case "$session_st" in
            active|detached) _status_session_label "$session_st" ;;
            *)        _status_session_none_label ;;
        esac
        return 0
    fi

    case "$session_st" in
        active|detached) echo "$(_status_session_label "$session_st") x${session_count}" ;;
        *)               echo "$(_status_session_label mixed) ${session_active_count}/${session_detached_count}" ;;
    esac
}

_status_session_details_for_branch() {
    local branch="${1:-}"
    [[ -n "$branch" ]] || return 0
    printf '%s' "${_STATUS_DATA["$branch:session_details"]:-}"
}

_status_session_detail_lines_normalized() {
    local branch="${1:-}"
    local _detail_line _detail_epoch _detail_name _detail_status _detail_layout _detail_activity
    while IFS= read -r _detail_line; do
        [[ -z "$_detail_line" ]] && continue
        IFS='|' read -r _detail_epoch _detail_name _detail_status _detail_layout _detail_activity <<< "$_detail_line"
        [[ "$_detail_layout" == "true" ]] || _detail_layout="false"
        if [[ -z "${_detail_epoch:-}" || ! "${_detail_epoch:-}" =~ ^[0-9]+$ ]]; then
            _detail_epoch=0
        fi
        printf '%s|%s|%s|%s|%s\n' \
            "$_detail_epoch" "$_detail_name" "$_detail_status" "$_detail_layout" "$_detail_activity"
    done <<< "$(_status_session_details_for_branch "$branch")"
}

_status_collect_session_rows_for_table() {
    local branch _line _epoch _session_name _session_status _session_layout _session_recent_activity _wt_mark
    for branch in "${_STATUS_BRANCHES[@]}"; do
        [[ "${_STATUS_DATA["$branch:session_count"]:-0}" -gt 0 ]] || continue
        _wt_mark=$(_status_worktree_mark "${_STATUS_DATA["$branch:worktree"]}")
        while IFS='|' read -r _epoch _session_name _session_status _session_layout _session_recent_activity; do
            [[ -z "$_session_name" ]] && continue
            printf '%s|%s|%s|%s|%s|%s|%s\n' \
                "$_epoch" "$branch" "$_wt_mark" "$_session_name" "$_session_status" "$_session_layout" "$_session_recent_activity"
        done < <(_status_session_detail_lines_normalized "$branch")
    done
}

_status_has_session_details() {
    local branch
    for branch in "${_STATUS_BRANCHES[@]}"; do
        [[ "${_STATUS_DATA["$branch:session_count"]:-0}" -gt 0 ]] && return 0
    done
    return 1
}

_status_render_session_details_table() {
    _status_has_session_details || return 0

    echo ""
    log_bold "🧭 $(_i18n STATUS_SESSION_DETAILS_TITLE)"
    echo ""
    printf "  %s %s %s %s %s %s\n" \
        "$(_wpad 20 "$(_i18n COL_BRANCH)")" "$(_wpad 10 "$(_i18n COL_WORKTREE)")" "$(_wpad 42 "$(_i18n COL_SESSION)")" \
        "$(_wpad 10 "$(_i18n STATUS_DETAIL_COL_STATE)")" "$(_wpad 8 "$(_i18n COL_LAYOUT)")" "$(_i18n STATUS_DETAIL_COL_RECENT_SESSION)"
    echo "  ────────────────────────────────────────────────────────────────────────────────────────────────────────────────"

    local _rows _ignored_epoch _session_name _session_status _session_layout _session_recent_activity
    local _status_label _layout_mark _branch _wt_mark
    _rows=$(_status_collect_session_rows_for_table)
    [[ -z "$_rows" ]] && return 0

    local _sorted_rows
    _sorted_rows=$(printf '%s' "$_rows" | sort -t'|' -k1,1nr)
    while IFS='|' read -r _ignored_epoch _branch _wt_mark _session_name _session_status _session_layout _session_recent_activity; do
        [[ -z "$_session_name" ]] && continue
        _status_label=$(_status_session_label "$_session_status")
        _layout_mark=$(_status_bool_mark "$_session_layout")
        printf "  %s %s %s %s %s %s\n" \
            "$(_wpad 20 "$_branch")" "$(_wpad 10 "$_wt_mark")" "$(_wpad 42 "$_session_name")" \
            "$(_wpad 10 "$_status_label")" "$(_wpad 8 "$_layout_mark")" "$_session_recent_activity"
    done <<< "$_sorted_rows"
}

# ──────────────────────────────────────────────────
# Full table output (with legend/actions)
# ──────────────────────────────────────────────────
_status_output_table() {
    _status_render_rows
    _status_render_session_details_table

    _status_render_layout_section
    _status_render_persistence_section

    echo ""
    log_info "$(_i18n STATUS_LEGEND)"
    echo "  ● = $(_i18n STATUS_LEGEND_ACTIVE)  ○ = $(_i18n STATUS_LEGEND_DETACHED)  ✓ = $(_i18n MSG_LEGEND_EXISTS)  ✗ = $(_i18n MSG_LEGEND_NOT_EXISTS)"
    echo ""
    log_info "$(_i18n MSG_ACTIONS)"
    echo "  • $(_i18n MSG_ENTER_WORKSPACE)  workhive attach <worktree>"
    echo "  • $(_i18n MSG_DESTROY_WT)  workhive remove <worktree>"
}

# ──────────────────────────────────────────────────
# Compact table output (no legend/actions, for watch mode)
# ──────────────────────────────────────────────────
_status_output_table_compact() {
    _status_render_rows
}

# ──────────────────────────────────────────────────
# JSON format output
# ──────────────────────────────────────────────────
_status_json_begin() {
    echo "{"
}

_status_json_end() {
    echo "}"
}

_status_json_emit_summary() {
    local active_count="${1:-0}"
    local detached_count="${2:-0}"
    echo "  \"summary\": {"
    echo "    \"total\": ${#_STATUS_BRANCHES[@]},"
    echo "    \"active\": $active_count,"
    echo "    \"detached\": $detached_count"
    echo "  },"
}

_status_json_emit_session_items() {
    local branch="${1:-}"
    local _first_session=true _detail_epoch _detail_name _detail_status _detail_layout _detail_activity
    while IFS='|' read -r _detail_epoch _detail_name _detail_status _detail_layout _detail_activity; do
        [[ -z "$_detail_name" ]] && continue
        [[ "$_first_session" == false ]] && echo ","
        _first_session=false
        printf '          {"name": "%s", "status": "%s", "layout": %s, "last_activity": "%s", "activity_epoch": %s}' \
            "$(json_escape "$_detail_name")" "$(json_escape "$_detail_status")" \
            "$_detail_layout" "$(json_escape "$_detail_activity")" "$_detail_epoch"
    done < <(_status_session_detail_lines_normalized "$branch")
    [[ "$_first_session" == false ]] && echo ""
}

_status_json_emit_workspace_item() {
    local branch="${1:-}"
    local safe_branch safe_path safe_git safe_activity safe_session
    local session_count session_active_count session_detached_count
    safe_branch=$(json_escape "$branch")
    safe_path=$(json_escape "${_STATUS_DATA["$branch:path"]}")
    safe_git=$(json_escape "${_STATUS_DATA["$branch:git_status"]}")
    safe_activity=$(json_escape "${_STATUS_DATA["$branch:last_activity"]}")
    safe_session=$(json_escape "${_STATUS_DATA["$branch:session"]}")
    session_count=${_STATUS_DATA["$branch:session_count"]:-0}
    session_active_count=${_STATUS_DATA["$branch:session_active_count"]:-0}
    session_detached_count=${_STATUS_DATA["$branch:session_detached_count"]:-0}

    cat <<EOF
    {
      "branch": "$safe_branch",
      "worktree": {
        "exists": ${_STATUS_DATA["$branch:worktree"]},
        "path": "$safe_path"
      },
      "session": {
        "status": "$safe_session",
        "count": $session_count,
        "active_count": $session_active_count,
        "detached_count": $session_detached_count,
        "sessions": [
EOF
    _status_json_emit_session_items "$branch"
    cat <<EOF
        ]
      },
      "layout": {
        "valid": ${_STATUS_DATA["$branch:layout"]}
      },
      "git_status": "$safe_git",
      "last_activity": "$safe_activity"
    }
EOF
}

_status_json_emit_workspaces() {
    echo "  \"workspaces\": ["
    local first=true
    local branch
    for branch in "${_STATUS_BRANCHES[@]}"; do
        [[ "$first" == false ]] && echo ","
        first=false
        _status_json_emit_workspace_item "$branch"
    done
    echo ""
    echo "  ],"
}

_status_json_emit_persistence() {
    _status_load_persistence_metadata "json"
    echo "  \"persistence\": {"
    echo "    \"enabled\": $_STATUS_PERSIST_ENABLED"
    if [[ "$_STATUS_PERSIST_ENABLED" == "true" ]]; then
        echo "    ,\"auto_save_interval\": ${_STATUS_PERSIST_INTERVAL:-0}"
        echo "    ,\"last_backup\": \"$(json_escape "$_STATUS_PERSIST_LAST_BACKUP")\""
    fi
    echo "  }"
}

_status_output_json() {
    _status_json_begin

    # Summary
    local active_count detached_count
    IFS='|' read -r active_count detached_count <<< "$(_status_session_totals)"
    _status_json_emit_summary "$active_count" "$detached_count"

    _status_json_emit_workspaces

    _status_json_emit_persistence
    _status_json_end
}

# Shared layout header block used by default table and --layout view.
_status_render_layout_header() {
    local config_source="$1"
    local expected_panes="$2"
    printf "  %s %s\n" "$(_wpad 16 "$(_i18n STATUS_LAYOUT_MODE_LABEL)")" "columns"
    printf "  %s %s\n" "$(_wpad 16 "$(_i18n STATUS_LAYOUT_SOURCE_LABEL)")" "$(_layout_config_source_label "$config_source")"
    printf "  %s %s\n" "$(_wpad 16 "$(_i18n STATUS_LAYOUT_PANES_LABEL)")" "$expected_panes"
}

# Shared pane-command block used by default table and --layout view.
_status_render_layout_pane_commands() {
    local _role _cmd _label
    while IFS='|' read -r _role _cmd; do
        [[ -z "$_role" ]] && continue
        _label=$(layout_pane_role_label "$_role")
        printf "  %s %s\n" "$(_wpad 22 "$_label")" "$(_cmd_display_label "$_cmd")"
    done < <(layout_collect_pane_commands)
}

_status_layout_metadata() {
    local config_source expected_panes
    config_source=$(_layout_config_source)
    expected_panes=$(_layout_expected_pane_count)
    printf '%s|%s\n' "$config_source" "$expected_panes"
}

_STATUS_LAYOUT_CONFIG_SOURCE=""
_STATUS_LAYOUT_EXPECTED_PANES=""

_status_load_layout_metadata() {
    IFS='|' read -r _STATUS_LAYOUT_CONFIG_SOURCE _STATUS_LAYOUT_EXPECTED_PANES <<< "$(_status_layout_metadata)"
}

_status_persistence_last_backup_label() {
    local mode="${1:-table}"
    local backup_mode empty_label
    IFS='|' read -r backup_mode empty_label <<< "$(_status_persistence_backup_profile "$mode")"
    persistence_latest_snapshot_time_or "$backup_mode" "$empty_label"
}

_status_persistence_backup_profile() {
    local mode="${1:-table}"
    if [[ "$mode" == "json" ]]; then
        echo "iso|none"
    else
        echo "|unknown"
    fi
}

_status_persistence_interval_value() {
    persistence_cfg_auto_save_interval 2>/dev/null || echo ""
}

_status_persistence_metadata() {
    local mode="${1:-table}"
    local enabled
    local interval=""
    local last_backup="none"

    enabled=$(persistence_enabled_bool)
    if [[ "$enabled" == "true" ]]; then
        interval=$(_status_persistence_interval_value)
        last_backup=$(_status_persistence_last_backup_label "$mode")
    fi

    printf '%s|%s|%s\n' "$enabled" "$interval" "$last_backup"
}

_STATUS_PERSIST_ENABLED="false"
_STATUS_PERSIST_INTERVAL=""
_STATUS_PERSIST_LAST_BACKUP="none"

_status_load_persistence_metadata() {
    local mode="${1:-table}"
    local metadata
    metadata=$(_status_persistence_metadata "$mode")
    IFS='|' read -r _STATUS_PERSIST_ENABLED _STATUS_PERSIST_INTERVAL _STATUS_PERSIST_LAST_BACKUP <<< "$metadata"
}

_status_render_layout_title() {
    echo ""
    log_bold "📐 $(_i18n STATUS_LAYOUT_TITLE)"
    echo ""
}

_status_layout_separator() {
    echo "  ────────────────────────────────────────"
}

_status_layout_validation_mark() {
    local actual_panes="${1:-0}"
    local expected_panes="${2:-0}"
    if [[ "$actual_panes" -ge "$expected_panes" ]]; then
        echo "${GREEN}✓ $(_i18n STATUS_LAYOUT_VALID)${NC} ($(_i18n STATUS_LAYOUT_EXPECTED): $expected_panes)"
    else
        echo "${RED}✗ $(_i18n STATUS_LAYOUT_INVALID)${NC} ($(_i18n STATUS_LAYOUT_EXPECTED): $expected_panes)"
    fi
}

_status_render_layout_intro() {
    local config_source="${1:-}"
    local expected_panes="${2:-}"
    if [[ -z "$config_source" || -z "$expected_panes" ]]; then
        IFS='|' read -r config_source expected_panes <<< "$(_status_layout_metadata)"
    fi
    _status_render_layout_title
    _status_render_layout_header "$config_source" "$expected_panes"
    echo ""
}

_status_render_layout_intro_from_metadata() {
    _status_load_layout_metadata
    _status_render_layout_intro "$_STATUS_LAYOUT_CONFIG_SOURCE" "$_STATUS_LAYOUT_EXPECTED_PANES"
}

# ──────────────────────────────────────────────────
# Layout summary section (shown in default status table)
# ──────────────────────────────────────────────────
_status_render_layout_section() {
    _status_render_layout_intro_from_metadata
    _status_render_layout_pane_commands
}

# ──────────────────────────────────────────────────
# Persistence section (status table)
# ──────────────────────────────────────────────────
_status_render_persistence_enabled() {
    printf "  %s %b\n" "$(_wpad 16 "$(_i18n STATUS_SNAPSHOT_STATUS_LABEL)")" "${GREEN}$(_i18n PERSIST_ENABLED)${NC}"
    printf "  %s %s\n" "$(_wpad 16 "$(_i18n STATUS_SNAPSHOT_AUTOSAVE_LABEL)")" "${_STATUS_PERSIST_INTERVAL}"
    printf "  %s %s\n" "$(_wpad 16 "$(_i18n STATUS_SNAPSHOT_LAST_BACKUP_LABEL)")" "$_STATUS_PERSIST_LAST_BACKUP"
}

_status_render_persistence_disabled() {
    printf "  %s %b\n" "$(_wpad 16 "$(_i18n STATUS_SNAPSHOT_STATUS_LABEL)")" "${YELLOW}$(_i18n PERSIST_DISABLED)${NC}"
    echo "  $(_i18n STATUS_SNAPSHOT_ENABLE_HINT)"
}

_status_render_persistence_section() {
    echo ""
    log_bold "💾 $(_i18n STATUS_SNAPSHOT_TITLE)"
    echo ""

    _status_load_persistence_metadata "table"
    if [[ "$_STATUS_PERSIST_ENABLED" == "true" ]]; then
        _status_render_persistence_enabled
    else
        _status_render_persistence_disabled
    fi
}

_status_collect_layout_sessions() {
    local session_prefix="$1"
    local -n _sl_out_ref=$2
    local _sname _pcount
    while IFS='|' read -r _sname _pcount; do
        [[ -z "$_sname" ]] && continue
        [[ "$_sname" == "${session_prefix}@"* ]] || continue
        _sl_out_ref+=("$_sname|$_pcount")
    done < <(tmux list-sessions -F "#{session_name}|#{session_windows}" 2>/dev/null || true)
}

_status_render_layout_validation_row() {
    local session_name="$1"
    local expected_panes="$2"
    local branch actual_panes valid_mark
    branch=$(session_extract_branch "$session_name")
    actual_panes=$(tmux display-message -t "$session_name" -p '#{window_panes}' 2>/dev/null || echo "0")
    valid_mark=$(_status_layout_validation_mark "$actual_panes" "$expected_panes")
    printf "  %-22s %b\n" "$branch" "$valid_mark"
}

_status_render_layout_validation_section() {
    local expected_panes="$1"
    local -n _slv_sessions_ref=$2
    local _entry _sname
    log_info "$(_i18n STATUS_LAYOUT_VALIDATION)"
    _status_layout_separator
    for _entry in "${_slv_sessions_ref[@]}"; do
        _sname="${_entry%%|*}"
        _status_render_layout_validation_row "$_sname" "$expected_panes"
    done
}

_status_render_layout_detail_header() {
    _config_print_layout_visual "columns" "project"
    echo ""
    log_info "$(_i18n STATUS_LAYOUT_PANE_CMDS)"
    _status_layout_separator
    _status_render_layout_pane_commands
    echo ""
}

# ──────────────────────────────────────────────────
# Layout detail output (--layout flag)
# ──────────────────────────────────────────────────
_status_output_layout() {
    _status_render_layout_intro_from_metadata
    local expected_panes="$_STATUS_LAYOUT_EXPECTED_PANES"
    _status_render_layout_detail_header

    # Session layout validation (only if there are active sessions)
    local session_prefix
    session_prefix=$(session_get_prefix)

    # Collect sessions belonging to this project
    local -a _layout_sessions=()
    local _sname _entry
    _status_collect_layout_sessions "$session_prefix" _layout_sessions

    if [[ ${#_layout_sessions[@]} -gt 0 ]]; then
        _status_render_layout_validation_section "$expected_panes" _layout_sessions
    fi

    echo ""
    return 0
}

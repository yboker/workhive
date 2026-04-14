#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Doctor Command
# Environment health check (App layer)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_DOCTOR_COMMAND_SH_LOADED:-}" ]] && return 0
readonly _DOCTOR_COMMAND_SH_LOADED=1

# Dependencies (deps and persistence lazy-loaded by --full)
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/progress.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/adapters/terminal/factory.sh"

# Show usage help
doctor_command_help() {
    _help_dispatch "_doctor_command_help"
}

# ──────────────────────────────────────────────────
# Get tool version (single tool, synchronous)
# Args: $1 = command name
# Returns (stdout): version string or empty
_doctor_get_version() {
    local cmd="$1"

    get_tool_version_by_profile "$cmd" "relaxed" || printf '\n'
}

# Parallel version check: run all --version calls concurrently
# Populates associative array _DOCTOR_VERSIONS[tool]=version
declare -gA _DOCTOR_VERSIONS=()
readonly _DOCTOR_REQUIRED_TOOLS=("git" "tmux" "yq")
readonly _DOCTOR_PANE_TOOLS=("lazygit" "claude" "codex" "opencode" "yazi")
_doctor_prefetch_versions() {
    _DOCTOR_VERSIONS=()

    local tmpdir
    tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/wh-doctor.XXXXXX") || return 1
    local _doctor_tmpdir="$tmpdir"
    local _doctor_prev_return_trap
    _doctor_prev_return_trap="$(trap -p RETURN || true)"
    local _doctor_prefetch_cleanup
    _doctor_prefetch_cleanup() {
        cleanup_dir_quiet "${_doctor_tmpdir:-}"
    }
    trap '_doctor_prefetch_cleanup' RETURN

    local -a tools=("$@")
    [[ ${#tools[@]} -eq 0 ]] && tools=("git" "tmux" "yq" "lazygit" "claude" "codex" "opencode" "yazi")
    local -a pids=()
    local tool
    for tool in "${tools[@]}"; do
        if command -v "$tool" &>/dev/null; then
            _doctor_get_version "$tool" > "$tmpdir/$tool" 2>/dev/null &
            pids+=("$!")
        fi
    done

    local pid
    for pid in "${pids[@]}"; do
        wait "$pid" 2>/dev/null || true
    done

    for tool in "${tools[@]}"; do
        if [[ -f "$tmpdir/$tool" ]]; then
            _DOCTOR_VERSIONS["$tool"]=$(<"$tmpdir/$tool")
        fi
    done

    _doctor_prefetch_cleanup
    if [[ -n "$_doctor_prev_return_trap" ]]; then
        eval "$_doctor_prev_return_trap"
    else
        trap - RETURN
    fi
}

_doctor_prefetch_versions_for_mode() {
    local full_mode="${1:-false}"
    local context_msg="${2:-doctor prefetch failed; continuing without cached versions}"

    if [[ "$full_mode" == "true" ]]; then
        _doctor_prefetch_versions
    else
        _doctor_prefetch_versions "${_DOCTOR_REQUIRED_TOOLS[@]}"
    fi
    local prefetch_rc=$?
    if [[ $prefetch_rc -ne 0 ]]; then
        log_debug "$context_msg"
    fi
    return 0
}

# Fast version lookup from pre-fetched cache
_doctor_cached_version() {
    echo "${_DOCTOR_VERSIONS[$1]:-}"
}

_doctor_tool_status() {
    local tool="$1"
    local installed=false
    local version=""
    if check_cmd "$tool"; then
        installed=true
        version=$(_doctor_cached_version "$tool")
    fi
    printf '%s|%s\n' "$installed" "$version"
}

_DOCTOR_TOOL_INSTALLED="false"
_DOCTOR_TOOL_VERSION=""

_doctor_load_tool_status() {
    local tool="$1"
    local status
    status=$(_doctor_tool_status "$tool")
    IFS='|' read -r _DOCTOR_TOOL_INSTALLED _DOCTOR_TOOL_VERSION <<< "$status"
}

_doctor_print_json_tool_object() {
    local tool="$1"
    local installed="$2"
    local version="$3"
    local safe_tool safe_ver safe_hint
    safe_tool=$(json_escape "$tool")
    safe_ver=$(json_escape "$version")
    safe_hint=$(json_escape "$(_doctor_install_hint "$tool")")
    printf '      {"name": "%s", "installed": %s, "version": "%s", "install_hint": "%s"}' \
        "$safe_tool" "$installed" "$safe_ver" "$safe_hint"
}

_doctor_print_json_tools_array() {
    local -n _dj_tools_ref=$1
    local count_missing="${2:-false}"
    local -n _dj_issues_ref=$3

    local first=true
    local tool
    for tool in "${_dj_tools_ref[@]}"; do
        [[ "$first" == false ]] && echo ","
        first=false
        _doctor_load_tool_status "$tool"
        if [[ "$count_missing" == "true" && "$_DOCTOR_TOOL_INSTALLED" != "true" ]]; then
            _doctor_increment_issues _dj_issues_ref
        fi
        _doctor_print_json_tool_object "$tool" "$_DOCTOR_TOOL_INSTALLED" "$_DOCTOR_TOOL_VERSION"
    done
    echo ""
}

_doctor_increment_issues() {
    local -n _di_ref=$1
    ((_di_ref++)) || true
}

_doctor_render_tool_check_row() {
    local tool="$1"
    local desc="$2"
    local missing_style="${3:-required}" # required|optional

    _doctor_load_tool_status "$tool"
    if [[ "$_DOCTOR_TOOL_INSTALLED" == "true" ]]; then
        echo -e "    ${GREEN}✓${NC} $tool $_DOCTOR_TOOL_VERSION ($desc)"
        return 0
    fi

    case "$missing_style" in
        optional)
            echo -e "    ${YELLOW}○${NC} $tool — $(_i18n DOCTOR_MISSING) ($desc)"
            ;;
        *)
            echo -e "    ${RED}✗${NC} $tool — $(_i18n DOCTOR_MISSING) ($desc)"
            ;;
    esac
    echo -e "      $(_i18n DOCTOR_INSTALL_HINT) $(_doctor_install_hint "$tool")"
    return 1
}

_doctor_render_tools_group() {
    local title="$1"
    local missing_style="$2" # required|optional
    local count_missing="${3:-false}"
    local -n _dr_tools_ref=$4
    local -n _dr_issues_ref=$5
    local trailing_newline="${6:-true}"

    log_bold "  $title"
    local tool desc
    for tool in "${_dr_tools_ref[@]}"; do
        desc=$(_doctor_get_desc "$tool")
        if ! _doctor_render_tool_check_row "$tool" "$desc" "$missing_style"; then
            if [[ "$count_missing" == "true" ]]; then
                _doctor_increment_issues _dr_issues_ref
            fi
        fi
    done
    [[ "$trailing_newline" == "true" ]] && echo ""
    return 0
}

# ──────────────────────────────────────────────────
# Get tool install hints
# ──────────────────────────────────────────────────
_doctor_install_hint() {
    local cmd="$1"
    case "$cmd" in
        git)      echo "brew install git / apt install git" ;;
        tmux)     echo "brew install tmux / apt install tmux" ;;
        yq)       echo "brew install yq / go install github.com/mikefarah/yq/v4@latest" ;;
        lazygit)  echo "brew install lazygit" ;;
        claude)   echo "npm install -g @anthropic-ai/claude-code" ;;
        codex)    echo "npm install -g @openai/codex" ;;
        opencode) echo "npm i -g opencode-ai@latest" ;;
        fzf)      echo "brew install fzf / apt install fzf" ;;
        yazi)     echo "brew install yazi" ;;
        delta)    echo "brew install git-delta" ;;
        starship) echo "brew install starship / curl -sS https://starship.rs/install.sh | sh" ;;
        zoxide)   echo "brew install zoxide" ;;
        atuin)    echo "brew install atuin" ;;
        *)        echo "" ;;
    esac
}

_doctor_get_desc() {
    local cmd="$1"
    case "$cmd" in
        git)      _i18n DEP_GIT ;;
        tmux)     _i18n DEP_TMUX ;;
        yq)       _i18n DEP_YQ ;;
        lazygit)  _i18n DEP_LAZYGIT ;;
        claude)   _i18n DEP_CLAUDE ;;
        codex)    _i18n DEP_CODEX ;;
        opencode) _i18n DEP_OPENCODE ;;
        fzf)      _i18n DEP_FZF ;;
        yazi)     _i18n YAZI_DESC ;;
        delta)    _i18n DEP_DELTA ;;
        starship) _i18n DEP_STARSHIP ;;
        zoxide)   _i18n DEP_ZOXIDE ;;
        atuin)    _i18n DEP_ATUIN ;;
        *)        echo "" ;;
    esac
}

_doctor_is_full_mode() {
    [[ "${_DOCTOR_FULL_MODE:-false}" == true ]]
}

_doctor_prepare_runtime() {
    if _doctor_is_full_mode; then
        source "${LIB_DIR}/infra/deps/manager.sh"
        source "${LIB_DIR}/infra/persistence/manager.sh"
        _doctor_prefetch_versions_for_mode "true" "doctor prefetch failed in full mode; continuing without cached versions"
    else
        _doctor_prefetch_versions_for_mode "false" "doctor prefetch failed; continuing without cached versions"
    fi
}

_doctor_should_show_progress() {
    _doctor_is_full_mode
}

_doctor_output_table_with_progress() {
    local show_progress=false
    if _doctor_should_show_progress; then
        show_progress=true
    fi
    if [[ "$show_progress" == true ]]; then
        progress_start "$(_i18n MSG_CHECKING_ENV 2>/dev/null || echo 'Checking environment...')"
    fi
    _doctor_output_table
    if [[ "$show_progress" == true ]]; then
        progress_stop
    fi
}

_doctor_output_by_format() {
    local format="${1:-table}"
    case "$format" in
        json) _doctor_output_json ;;
        *)    _doctor_output_table_with_progress ;;
    esac
}

# ──────────────────────────────────────────────────
# Execute doctor command
# ──────────────────────────────────────────────────
doctor_command_execute() {
    local format="table"
    _DOCTOR_FULL_MODE=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            -h|--help)
                doctor_command_help
                return 0
                ;;
            --full)
                _DOCTOR_FULL_MODE=true
                shift
                ;;
            --format)
                format=$(cli_parse_format_value "${2:-}" '^(table|json)$' 'table, json') || return 1
                shift 2
                ;;
            -*)
                cli_log_unknown_opt "$1"
                echo ""
                doctor_command_help
                return 1
                ;;
            *)
                cli_log_no_positional "doctor"
                echo ""
                doctor_command_help
                return 1
                ;;
        esac
    done

    _doctor_prepare_runtime
    _doctor_output_by_format "$format"
}

# ──────────────────────────────────────────────────
# Table format output — section helpers
# Each helper updates the shared issues counter via nameref
# ──────────────────────────────────────────────────
_doctor_table_required_deps() {
    local -n _dt_issues=$1
    _doctor_render_tools_group "$(_i18n DOCTOR_REQUIRED):" "required" "true" _DOCTOR_REQUIRED_TOOLS _dt_issues "false"
    if check_cmd yq && ! config_check_yq_version 2>/dev/null; then
        echo -e "    ${RED}✗${NC} $(_i18n ERR_YQ_VERSION_INCOMPATIBLE)"
        _doctor_increment_issues _dt_issues
    fi
    echo ""
}

_doctor_table_pane_tools() {
    local _dummy_issues=0
    _doctor_render_tools_group "$(_i18n PANE_TOOLS)" "optional" "false" _DOCTOR_PANE_TOOLS _dummy_issues
}

_doctor_config_valid_message() {
    if [[ "$_DOCTOR_CONFIG_SCOPE" == "project" ]]; then
        _i18n DOCTOR_CONFIG_OK
    else
        _i18n DOCTOR_CONFIG_GLOBAL_ONLY
    fi
}

_doctor_config_invalid_message() {
    if [[ "$_DOCTOR_CONFIG_SCOPE" == "global" ]]; then
        echo "$(_i18n DOCTOR_CONFIG_INVALID) (global)"
    else
        _i18n DOCTOR_CONFIG_INVALID
    fi
}

_doctor_table_config_check() {
    local -n _dc_issues=$1
    log_bold "  $(_i18n DOCTOR_CONFIG_CHECK)"
    _doctor_collect_config_status
    if [[ "$_DOCTOR_CONFIG_EXISTS" != true ]]; then
        echo -e "    ${YELLOW}○${NC} $(_i18n DOCTOR_CONFIG_MISSING)"
    elif [[ "$_DOCTOR_CONFIG_VALID" == true ]]; then
        echo -e "    ${GREEN}✓${NC} $(_doctor_config_valid_message)"
    else
        echo -e "    ${RED}✗${NC} $(_doctor_config_invalid_message)"
        _doctor_increment_issues _dc_issues
    fi
    echo ""
}

_DOCTOR_CONFIG_EXISTS=false
_DOCTOR_CONFIG_VALID=false
_DOCTOR_CONFIG_SCOPE=""
_DOCTOR_GIT_IN_REPO=false
_DOCTOR_GIT_BRANCH=""

_doctor_is_valid_config_file() {
    local cfg_file="$1"
    [[ -f "$cfg_file" ]] || return 1
    check_cmd yq || return 1
    yq '.' "$cfg_file" &>/dev/null
}

_doctor_set_config_scope_status() {
    local cfg_file="$1"
    local scope="$2"
    _DOCTOR_CONFIG_EXISTS=true
    _DOCTOR_CONFIG_SCOPE="$scope"
    if _doctor_is_valid_config_file "$cfg_file"; then
        _DOCTOR_CONFIG_VALID=true
    fi
}

_doctor_collect_config_status() {
    _DOCTOR_CONFIG_EXISTS=false
    _DOCTOR_CONFIG_VALID=false
    _DOCTOR_CONFIG_SCOPE=""

    local _project_cfg _global_cfg
    _project_cfg="$(_workhive_dir)/config.yaml"
    _global_cfg=$(config_get_global_file)

    if [[ -f "$_project_cfg" ]]; then
        _doctor_set_config_scope_status "$_project_cfg" "project"
        return 0
    fi

    if [[ -f "$_global_cfg" ]]; then
        _doctor_set_config_scope_status "$_global_cfg" "global"
        return 0
    fi

    return 0
}

_doctor_collect_git_status() {
    _DOCTOR_GIT_IN_REPO=false
    _DOCTOR_GIT_BRANCH=""
    if git_is_inside_work_tree; then
        _DOCTOR_GIT_IN_REPO=true
        _DOCTOR_GIT_BRANCH=$(_git_current_branch)
    fi
}

_doctor_config_has_issue() {
    [[ "$_DOCTOR_CONFIG_EXISTS" == true && "$_DOCTOR_CONFIG_VALID" != true ]]
}

_doctor_git_has_issue() {
    [[ "$_DOCTOR_GIT_IN_REPO" != true ]]
}

_doctor_table_tmux_version() {
    local -n _dtv_issues=$1
    log_bold "  $(_i18n DOCTOR_TMUX_CHECK)"
    if check_cmd tmux; then
        terminal_adapter_init 2>/dev/null || true
        local tmux_ver
        tmux_ver=$(terminal_adapter_get_version 2>/dev/null || echo "")
        if [[ -n "$tmux_ver" && "$tmux_ver" != "0.0" ]]; then
            local major minor parsed
            parsed=$(parse_version_major_minor "$tmux_ver" 2>/dev/null || echo "")
            IFS='|' read -r major minor <<< "$parsed"
            if [[ -z "$major" || -z "$minor" ]]; then
                major=0
                minor=0
            fi
            if [[ "$major" -ge 3 && "$minor" -ge 3 ]] || [[ "$major" -gt 3 ]]; then
                echo -e "    ${GREEN}✓${NC} tmux $tmux_ver (popups supported)"
            else
                echo -e "    ${YELLOW}○${NC} tmux $tmux_ver ($(_i18n DOCTOR_TMUX_VERSION_LOW))"
            fi
        fi
    else
        echo -e "    ${RED}✗${NC} tmux $(_i18n DOCTOR_MISSING)"
        _doctor_increment_issues _dtv_issues
    fi
    echo ""
}

_doctor_table_git_check() {
    local -n _dg_issues=$1
    log_bold "  $(_i18n DOCTOR_GIT_CHECK)"
    _doctor_collect_git_status
    if ! _doctor_git_has_issue; then
        echo -e "    ${GREEN}✓${NC} Git repo (${_DOCTOR_GIT_BRANCH:-unknown})"
    else
        echo -e "    ${RED}✗${NC} $(_i18n DOCTOR_NOT_GIT_REPO)"
        _doctor_increment_issues _dg_issues
    fi
    echo ""
}

_doctor_table_deps() {
    local -n _dd_issues=$1
    _deps_load_providers
    local dep_ids
    dep_ids=$(deps_list_ids)
    if [[ -n "$dep_ids" ]]; then
        log_bold "  🔌 $(_i18n DEPS_TITLE)"
        local dep_id
        while IFS= read -r dep_id; do
            [[ -z "$dep_id" ]] && continue
            deps_doctor "$dep_id"
            local doc_rc=$?
            [[ $doc_rc -ne 0 ]] && _doctor_increment_issues _dd_issues
        done <<< "$dep_ids"
        echo ""
    fi
}

_doctor_render_table_summary() {
    local issues="${1:-0}"
    if [[ "$issues" -eq 0 ]]; then
        log_success "$(_i18n DOCTOR_ALL_OK)"
    else
        log_warn "$(_i18n DOCTOR_HAS_ISSUES): $issues"
    fi
}

_doctor_table_full_mode_checks() {
    local -n _df_issues=$1
    log_bold "  💾 $(_i18n PERSIST_TITLE)"
    persistence_doctor
    echo ""
    _doctor_table_deps _df_issues
}

_doctor_table_core_checks() {
    local -n _dc_issues=$1
    _doctor_table_required_deps _dc_issues
    _doctor_table_config_check _dc_issues
    _doctor_table_tmux_version _dc_issues
    _doctor_table_git_check _dc_issues
}

_doctor_output_table() {
    local issues=0
    log_bold "🏥 $(_i18n DOCTOR_TITLE)"
    echo ""

    if _doctor_is_full_mode; then
        _doctor_table_pane_tools
    fi

    _doctor_table_core_checks issues

    if _doctor_is_full_mode; then
        _doctor_table_full_mode_checks issues
    fi

    _doctor_render_table_summary "$issues"
}

_doctor_json_emit_config() {
    local -n _djc_issues_ref=$1
    _doctor_collect_config_status_and_issues _djc_issues_ref
    echo "  \"config\": {\"exists\": ${_DOCTOR_CONFIG_EXISTS}, \"valid\": ${_DOCTOR_CONFIG_VALID}},"
}

_doctor_json_emit_git() {
    local -n _djg_issues_ref=$1
    _doctor_collect_git_status_and_issues _djg_issues_ref
    local safe_branch
    safe_branch=$(json_escape "$_DOCTOR_GIT_BRANCH")
    echo "  \"git\": {\"in_repo\": ${_DOCTOR_GIT_IN_REPO}, \"branch\": \"$safe_branch\"},"
}

_doctor_collect_config_status_and_issues() {
    local -n _dccai_issues_ref=$1
    _doctor_collect_status_and_issues "_doctor_collect_config_status" "_doctor_config_has_issue" _dccai_issues_ref
    return 0
}

_doctor_collect_git_status_and_issues() {
    local -n _dcgai_issues_ref=$1
    _doctor_collect_status_and_issues "_doctor_collect_git_status" "_doctor_git_has_issue" _dcgai_issues_ref
    return 0
}

_doctor_collect_status_and_issues() {
    local collect_fn="$1"
    local issue_fn="$2"
    local -n _dcsai_issues_ref=$3

    "$collect_fn"
    "$issue_fn" && _doctor_increment_issues _dcsai_issues_ref
    return 0
}

_doctor_json_emit_dependencies() {
    local -n _djd_issues_ref=$1
    echo "  \"dependencies\": {"
    echo "    \"required\": ["
    _doctor_print_json_tools_array _DOCTOR_REQUIRED_TOOLS "true" _djd_issues_ref
    if _doctor_is_full_mode; then
        echo "    ],"
        echo "    \"pane_tools\": ["
        _doctor_print_json_tools_array _DOCTOR_PANE_TOOLS "false" _djd_issues_ref
        echo "    ]"
    else
        echo "    ]"
    fi
    echo "  },"
}

_doctor_persistence_last_backup_iso() {
    local backup_time
    backup_time=$(persistence_latest_snapshot_time_or "iso" "")
    if [[ -z "$backup_time" ]]; then
        return 1
    fi
    printf '%s\n' "$backup_time"
}

_doctor_json_emit_persistence() {
    local persist_enabled
    persist_enabled=$(persistence_enabled_bool)
    echo -n "  \"persistence\": {\"enabled\": $persist_enabled"
    if [[ "$persist_enabled" == "true" ]]; then
        local backup_time
        backup_time=$(_doctor_persistence_last_backup_iso) || backup_time=""
        if [[ -n "$backup_time" ]]; then
            echo -n ", \"last_backup\": \"$(json_escape "$backup_time")\""
        fi
    fi
    echo "},"
}

_doctor_json_emit_summary() {
    local issues="${1:-0}"
    echo "  \"issues\": $issues"
}

_doctor_json_begin() {
    echo "{"
}

_doctor_json_end() {
    echo "}"
}

_doctor_json_emit_runtime_sections() {
    local -n _djrs_issues_ref=$1
    _doctor_json_emit_config _djrs_issues_ref
    _doctor_json_emit_git _djrs_issues_ref
    if _doctor_is_full_mode; then
        _doctor_json_emit_persistence
    fi
}

# ──────────────────────────────────────────────────
# JSON format output
# ──────────────────────────────────────────────────
_doctor_output_json() {
    local issues=0

    _doctor_json_begin
    _doctor_json_emit_dependencies issues
    _doctor_json_emit_runtime_sections issues

    _doctor_json_emit_summary "$issues"
    _doctor_json_end
}

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Config Loader
# Configuration loader - multi-tier config merging
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_CONFIG_LOADER_SH_LOADED:-}" ]] && return 0
readonly _CONFIG_LOADER_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/infra/cache/manager.sh"
[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

# Single source of truth for version
readonly WORKHIVE_VERSION="0.9.0"

# Validate yq version (requires 4.x+, 3.x API is incompatible)
# Returns: 0 = compatible, 1 = incompatible or not installed
_CONFIG_YQ_VERSION_CHECKED=false
_CONFIG_YQ_VERSION_OK=false
config_check_yq_version() {
    if [[ "$_CONFIG_YQ_VERSION_CHECKED" == true ]]; then
        [[ "$_CONFIG_YQ_VERSION_OK" == true ]]
        return $?
    fi

    if ! check_cmd yq; then
        _CONFIG_YQ_VERSION_CHECKED=true
        _CONFIG_YQ_VERSION_OK=false
        return 1
    fi

    local yq_raw yq_ver yq_major
    yq_raw=$(yq --version 2>/dev/null | head -1)
    yq_ver=$(extract_version_by_pattern "$yq_raw" '[0-9]+\.[0-9]+\.[0-9]+') || yq_ver=""

    # Require mikefarah/yq implementation (Go). Python yq/jq wrapper is incompatible.
    if [[ "$yq_raw" != *"mikefarah/yq"* ]]; then
        log_error "$(_i18n ERR_YQ_IMPL_INCOMPATIBLE)"
        log_info "$(_i18n HINT_INSTALL_YQ)"
        _CONFIG_YQ_VERSION_CHECKED=true
        _CONFIG_YQ_VERSION_OK=false
        return 1
    fi

    yq_major="${yq_ver%%.*}"
    if [[ -z "$yq_ver" || -z "$yq_major" || ! "$yq_major" =~ ^[0-9]+$ || "$yq_major" -lt 4 ]]; then
        log_error "$(_i18n ERR_YQ_VERSION_INCOMPATIBLE)"
        log_info "$(_i18n HINT_INSTALL_YQ)"
        _CONFIG_YQ_VERSION_CHECKED=true
        _CONFIG_YQ_VERSION_OK=false
        return 1
    fi

    # Optional exact pin for environments requiring deterministic yq behavior.
    local required_yq_version="${WORKHIVE_YQ_REQUIRED_VERSION:-}"
    if [[ -n "$required_yq_version" && "$yq_ver" != "$required_yq_version" ]]; then
        log_error "$(_i18n_fmt ERR_YQ_VERSION_PIN_MISMATCH_FMT "$required_yq_version" "$yq_ver")"
        log_info "$(_i18n HINT_YQ_PIN_ENV)"
        _CONFIG_YQ_VERSION_CHECKED=true
        _CONFIG_YQ_VERSION_OK=false
        return 1
    fi

    _CONFIG_YQ_VERSION_CHECKED=true
    _CONFIG_YQ_VERSION_OK=true
    return 0
}

config_get_default_tools_yaml() {
    cat <<'EOF'
default: "__empty__"
list:
  - tool_name: claude
    label: "Claude Code"
    command: "claude --dangerously-skip-permissions"
    is_ai: true
    enabled: true
  - tool_name: codex
    label: "Codex"
    command: "codex"
    is_ai: true
    enabled: true
  - tool_name: opencode
    label: "OpenCode"
    command: "opencode"
    is_ai: true
    enabled: true
  - tool_name: lazygit
    label: "Lazygit"
    command: "lazygit"
    is_ai: false
    enabled: true
  - tool_name: yazi
    label: "Yazi"
    command: "yazi"
    is_ai: false
    enabled: true
EOF
}

# Configuration priority:
# 1. Project-level (.workhive/config.yaml)
# 2. User-level (~/.config/workhive/config.yaml)
# 3. Built-in defaults

# Built-in default configuration
config_get_defaults() {
    cat <<EOF
version: "${WORKHIVE_VERSION}"

layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: null

worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
  sync_files: []

tmux:
  session_prefix: null
  mouse_support: true
  interaction_profile: "workspace"
  env:
    CLAUDE_CODE_NO_FLICKER: 1
  theme: "catppuccin-mocha"
  persistence:
    enabled: false
    generation: 0
    auto_save_mode: hybrid
    auto_save_debounce: 3s
    auto_save_interval: 10m
    restore_processes: true
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1
    process_list: [":all:"]

    snapshot_history_limit: 8
    snapshot_max_size_mb: 8
  attach_delay: 1s

maintenance:
  temp_cleanup:
    max_age_days: 7
  # Optional dedicated test log path.
  # Default is /dev/null (no test log file persisted).
  # To keep a dedicated test file, set this to ".workhive/logs/workhive-test.log".
  test_log:
    path: "/dev/null"
  log:
    path: ".workhive/logs/workhive.log"
    level: "info"
    format: "text"
    max_size_mb: 10
    max_backups: 5
EOF
}

# Load a configuration file
# Args: $1 = config file path
# Returns: 0 = success, 1 = file not found or invalid
config_load_file() {
    local config_file="$1"

    if [[ ! -f "$config_file" ]]; then
        log_debug "$(_i18n DEBUG_CONFIG_FILE_NOT_EXISTS) $config_file"
        return 1
    fi

    if [[ ! -r "$config_file" ]]; then
        log_error "$(_i18n ERR_CONFIG_NOT_READABLE) $config_file"
        return 1
    fi

    # Validate YAML syntax (if yq is available)
    if check_cmd yq; then
        if ! yq '.' "$config_file" &>/dev/null; then
            log_warn "$(_i18n ERR_CONFIG_SYNTAX_PREFIX) $config_file"
            return 1
        fi
        local _scope
        _scope=$(_config_scope_for_file "$config_file")
        if ! _config_validate_schema_file "$config_file" "$_scope"; then
            return 1
        fi
    fi

    # Cache the config content
    local cache_key="config:file:$config_file"
    local content
    if ! content=$(cat "$config_file" 2>/dev/null); then
        log_warn "$(_i18n_fmt WARN_CONFIG_READ_FAILED_FMT "$config_file")"
        return 1
    fi
    cache_set "$cache_key" "$content" "$CACHE_TTL_CONFIG_FILE"

    echo "$content"
    return 0
}

# Get a configuration value
# Args: $1 = config key (supports dot-path, e.g. layout.main_pane.command), $2 = default value (optional)
# Returns (stdout): config value; "null" means key exists but value is null; empty string means key not found
# Behavior: 3-tier config priority (project > user > defaults)
# Cached project config path (avoids repeated git rev-parse)
_CONFIG_PROJECT_PATH=""
_config_project_path() {
    if [[ -z "$_CONFIG_PROJECT_PATH" ]]; then
        _CONFIG_PROJECT_PATH="$(config_get_project_file)"
    fi
    echo "$_CONFIG_PROJECT_PATH"
}

# Cached yq availability (avoids repeated `command -v` lookups)
_CONFIG_HAS_YQ=""
_config_has_yq() {
    if [[ -z "$_CONFIG_HAS_YQ" ]]; then
        if command -v yq &>/dev/null; then _CONFIG_HAS_YQ=1; else _CONFIG_HAS_YQ=0; fi
    fi
    [[ "$_CONFIG_HAS_YQ" == "1" ]]
}

# Fast text-based YAML read supporting top-level and 2-level nested keys
# Supports: "language" (top-level), "tmux.theme" (2-level)
# Returns value via stdout, empty string if not found
_config_fast_get() {
    local key="$1" file="$2"
    [[ ! -f "$file" ]] && return 1

    if [[ "$key" == *.* ]]; then
        local parent="${key%%.*}"
        local child="${key#*.}"
        if [[ "$child" == *.* ]]; then
            return 1
        fi
        awk -v p="$parent" -v c="$child" '
            $0 ~ "^"p":" { found=1; next }
            found && /^[^ ]/ { exit }
            found && $0 ~ "^  "c":" {
                sub(/^  [^:]+:[[:space:]]*/,"")
                gsub(/^"|"$/,"")
                print
                exit
            }
        ' "$file" 2>/dev/null
    else
        sed -n "s/^${key}:[[:space:]]*\"\{0,1\}\(.*[^\"]\\)\"\{0,1\}[[:space:]]*$/\1/p" "$file" 2>/dev/null | head -1
    fi
}

config_get() {
    local key="$1"
    local default_value="${2:-}"

    if [[ ! "$key" =~ ^[a-zA-Z0-9_.]+$ ]]; then
        log_debug "config_get: rejected unsafe key: $key"
        printf '%s' "$default_value"
        return 0
    fi

    local cache_key="config:value:$key"

    local cached
    if cached=$(cache_get "$cache_key" 2>/dev/null); then
        if [[ "$cached" == "__WORKHIVE_NULL__" ]]; then
            printf '%s' "$default_value"
            return 0
        fi
        printf '%s' "$cached"
        return 0
    fi

    local value=""
    local _user_config="${XDG_CONFIG_HOME:-$HOME/.config}/workhive/config.yaml"

    # Fast path: top-level and 2-level keys use awk/sed (no yq fork needed)
    local _dot_count="${key//[^.]}"
    if [[ ${#_dot_count} -le 1 ]]; then
        # Try relative path first (avoids git rev-parse), then git root
        if [[ -f ".workhive/config.yaml" ]]; then
            _config_validate_file_once ".workhive/config.yaml" "project" || return 1
            value=$(_config_fast_get "$key" ".workhive/config.yaml") || true
        fi
        if _is_null_or_empty "$value"; then
            if [[ -z "${_CONFIG_GIT_ROOT_CACHED+x}" ]]; then
                _CONFIG_GIT_ROOT_CACHED="${_WORKHIVE_GIT_ROOT:-$(git rev-parse --show-toplevel 2>/dev/null)}" || _CONFIG_GIT_ROOT_CACHED=""
            fi
            if [[ -n "$_CONFIG_GIT_ROOT_CACHED" && -f "$_CONFIG_GIT_ROOT_CACHED/.workhive/config.yaml" ]]; then
                _config_validate_file_once "$_CONFIG_GIT_ROOT_CACHED/.workhive/config.yaml" "project" || return 1
                value=$(_config_fast_get "$key" "$_CONFIG_GIT_ROOT_CACHED/.workhive/config.yaml") || true
            fi
        fi
        if _is_null_or_empty "$value"; then
            _config_validate_file_once "$_user_config" "global" || return 1
            value=$(_config_fast_get "$key" "$_user_config") || true
        fi
        if [[ -n "$value" && "$value" != "null" ]]; then
            cache_set "$cache_key" "$value" "$CACHE_TTL_CONFIG_VALUE"
            printf '%s' "$value"
            return 0
        fi
    fi

    # Full path: use yq for nested keys (or when sed fast path misses)
    # Prepend "... comments="" |" to strip inline YAML comments from output.
    local _project_config
    _project_config="$(_config_project_path)"
    if _config_has_yq; then
        if [[ -f "$_project_config" ]]; then
            _config_validate_file_once "$_project_config" "project" || return 1
            value=$(yq '... comments="" | '".$key" "$_project_config" 2>/dev/null) || value=""
        fi

        if _is_null_or_empty "$value"; then
            if [[ -f "$_user_config" ]]; then
                _config_validate_file_once "$_user_config" "global" || return 1
                value=$(yq '... comments="" | '".$key" "$_user_config" 2>/dev/null) || value=""
            fi
        fi

        if _is_null_or_empty "$value"; then
            value=$(config_get_defaults | yq ".$key" 2>/dev/null) || value=""
        fi
    fi

    [[ "$value" == "null" ]] && value=""

    if [[ -n "$value" ]]; then
        cache_set "$cache_key" "$value" "$CACHE_TTL_CONFIG_VALUE"
        printf '%s' "$value"
    elif [[ -n "$default_value" ]]; then
        cache_set "$cache_key" "__WORKHIVE_NULL__" "$CACHE_TTL_CONFIG_VALUE"
        printf '%s' "$default_value"
    else
        cache_set "$cache_key" "__WORKHIVE_NULL__" "$CACHE_TTL_CONFIG_VALUE"
    fi

    return 0
}

# Find the first config file that has a non-null value for the given key.
# Returns the file path via stdout, or empty string if not found.
# Searches: project → user → (no defaults for array keys)
config_find_file_with_key() {
    local key="$1"
    local _cfg_files=(
        "$(_workhive_dir)/config.yaml"
        "${XDG_CONFIG_HOME:-$HOME/.config}/workhive/config.yaml"
    )
    if ! check_cmd yq; then
        return 1
    fi
    local _cfg
    for _cfg in "${_cfg_files[@]}"; do
        if [[ -f "$_cfg" ]]; then
            local _val
            _val=$(yq ".$key" "$_cfg" 2>/dev/null) || continue
            if ! _is_null_or_empty "$_val"; then
                echo "$_cfg"
                return 0
            fi
        fi
    done
    return 1
}

# Get merged tmux.env entries from global + project config.
# Output format: KEY=VALUE (one per line)
# Priority: project > global
_config_get_merged_env_entries_for_path() {
    local path="$1"
    if ! _config_has_yq; then
        return 0
    fi

    local _user_config="${XDG_CONFIG_HOME:-$HOME/.config}/workhive/config.yaml"
    local _project_config
    _project_config="$(_config_project_path)"

    if [[ -f "$_user_config" ]]; then
        _config_validate_file_once "$_user_config" "global" || return 1
    fi
    if [[ -f "$_project_config" ]]; then
        _config_validate_file_once "$_project_config" "project" || return 1
    fi

    local -a merge_sources=()
    if [[ -f "$_user_config" ]]; then
        merge_sources+=("$_user_config")
    fi
    if [[ -f "$_project_config" ]]; then
        merge_sources+=("$_project_config")
    fi

    if [[ "${#merge_sources[@]}" -eq 0 ]]; then
        return 0
    fi

    yq ea -r ". as \$item ireduce ({}; . * \$item ) | (${path} // {}) | select(type == \"!!map\") | to_entries[]? | \"\\(.key)=\\(.value|tostring)\"" "${merge_sources[@]}" 2>/dev/null || true
}

# Get merged tmux.env entries from global + project config.
# Output format: KEY=VALUE (one per line)
# Priority: project > global
config_get_tmux_env_entries() {
    _config_get_merged_env_entries_for_path ".tmux.env"
}

config_get_persistence_auto_save_env_entries() {
    _config_get_merged_env_entries_for_path ".tmux.persistence.auto_save_env"
}

config_get_persistence_pre_kill_env_entries() {
    _config_get_merged_env_entries_for_path ".tmux.persistence.pre_kill_env"
}

config_build_env_assignment_prefix_from_getter() {
    local getter_name="$1"
    [[ -n "$getter_name" ]] || return 0
    declare -f "$getter_name" >/dev/null 2>&1 || return 0

    local prefix=""
    local entry key value escaped_value
    while IFS= read -r entry; do
        [[ -z "$entry" ]] && continue
        key="${entry%%=*}"
        value="${entry#*=}"
        [[ -z "$key" ]] && continue
        if ! [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
            continue
        fi
        escaped_value=$(printf '%q' "$value")
        prefix+="${key}=${escaped_value} "
    done < <("$getter_name" 2>/dev/null || true)

    printf '%s' "$prefix"
}

config_managed_env_keys() {
    cat <<'EOF'
CLAUDE_CODE_NO_FLICKER
WORKHIVE_SAVE_LOCK_QUIET
WORKHIVE_SAVE_QUALITY_QUIET
WORKHIVE_SAVE_QUALITY_MAX_RETRIES
EOF
}

config_unset_managed_env_vars() {
    local key
    while IFS= read -r key; do
        [[ -n "$key" ]] || continue
        unset "$key"
    done < <(config_managed_env_keys)
}

config_build_unset_managed_env_script() {
    local script=""
    local key
    while IFS= read -r key; do
        [[ -n "$key" ]] || continue
        script+="unset ${key}; "
    done < <(config_managed_env_keys)
    printf '%s' "$script"
}

# Get global config directory (XDG standard, cross-platform)
# macOS/Linux: ~/.config/workhive
# Windows (Git Bash/MSYS): $APPDATA/workhive or ~/.config/workhive
config_get_global_dir() {
    if [[ -n "${WORKHIVE_CONFIG_DIR:-}" ]]; then
        echo "$WORKHIVE_CONFIG_DIR"
        return 0
    fi

    local platform
    platform=$(platform_detect 2>/dev/null || echo "unknown")

    case "$platform" in
        windows-git-bash)
            if [[ -n "${APPDATA:-}" ]]; then
                echo "${APPDATA}/workhive"
            else
                echo "${XDG_CONFIG_HOME:-$HOME/.config}/workhive"
            fi
            ;;
        *)
            echo "${XDG_CONFIG_HOME:-$HOME/.config}/workhive"
            ;;
    esac
}

config_cleanup_stale_temp_files() {
    local target_dir="$1"
    local stale_minutes="${2:-}"
    if [[ -z "$stale_minutes" ]]; then
        local stale_days
        stale_days=$(config_get "maintenance.temp_cleanup.max_age_days" "7" 2>/dev/null || echo "7")
        [[ "$stale_days" =~ ^[0-9]+$ ]] || stale_days=7
        (( stale_days < 1 )) && stale_days=7
        stale_minutes=$((stale_days * 1440))
    fi

    [[ -d "$target_dir" ]] || return 0
    [[ "$stale_minutes" =~ ^[0-9]+$ ]] || stale_minutes=$((7 * 1440))

    cleanup_stale_files_by_name "$target_dir" "$stale_minutes" 'config.atomic.*'
    cleanup_stale_files_by_name "$target_dir" "$stale_minutes" 'config.yaml.stage.*'
}

config_atomic_yq_edit() {
    local config_file="$1"
    local expr="$2"
    shift 2 || true
    local -a env_pairs=("$@")

    if ! config_check_yq_version; then
        return 1
    fi
    [[ -f "$config_file" ]] || return 1

    local cfg_dir
    cfg_dir=$(dirname "$config_file")
    config_cleanup_stale_temp_files "$cfg_dir"

    op_begin "config_atomic_edit" "$config_file" "expr=${expr}"
    (
        local tmp_file
        tmp_file=$(mktemp "${cfg_dir}/config.atomic.XXXXXX") || exit 1

        if ! cp -f -- "$config_file" "$tmp_file"; then
            cleanup_file_quiet "$tmp_file"
            exit 1
        fi

        if [[ ${#env_pairs[@]} -gt 0 ]]; then
            if ! env "${env_pairs[@]}" yq -i "$expr" "$tmp_file"; then
                cleanup_file_quiet "$tmp_file"
                exit 1
            fi
        else
            if ! yq -i "$expr" "$tmp_file"; then
                cleanup_file_quiet "$tmp_file"
                exit 1
            fi
        fi

        if ! mv -f -- "$tmp_file" "$config_file"; then
            cleanup_file_quiet "$tmp_file"
            exit 1
        fi
        exit 0
    )
    local rc=$?
    if [[ $rc -ne 0 ]]; then
        op_fail "config_atomic_edit" "$config_file" "$rc"
        return "$rc"
    fi
    op_ok "config_atomic_edit" "$config_file"
    return 0
}

declare -A _CONFIG_VALIDATED_FILES=()

_config_scope_for_file() {
    local config_file="$1"
    local global_file
    global_file=$(config_get_global_file)
    if [[ "$config_file" == "$global_file" ]]; then
        echo "global"
    else
        echo "project"
    fi
}

_config_seed_global_defaults() {
    local config_file="$1"
    local dangerous_paths_yaml
    dangerous_paths_yaml=$(_get_system_dangerous_paths_yaml)
    [[ -z "$dangerous_paths_yaml" ]] && dangerous_paths_yaml="[]"
    local default_tools_yaml
    default_tools_yaml=$(config_get_default_tools_yaml)
    check_cmd yq || return 0
    if [[ "$(yq '.tools | type' "$config_file" 2>/dev/null || echo "")" == "!!seq" ]]; then
        config_atomic_yq_edit "$config_file" '
            .tools = {
              "default": ([.tools[]? | select((.default // false) == true).tool_name | select(. != "")] | .[0] // ""),
              "list": (.tools | map(del(.default)))
            }
        ' >/dev/null 2>&1 || return 1
    fi
    config_atomic_yq_edit "$config_file" '
        .git_init = (.git_init // {}) |
        .git_init.whitelist_paths = (.git_init.whitelist_paths // []) |
        .git_init.dangerous_paths_extra = (
          .git_init.dangerous_paths_extra // (strenv(DANGEROUS_PATHS_YAML) | from_yaml)
        ) |
        .maintenance = (.maintenance // {}) |
        .maintenance.temp_cleanup = (.maintenance.temp_cleanup // {}) |
        .maintenance.temp_cleanup.max_age_days = (.maintenance.temp_cleanup.max_age_days // (strenv(DEFAULT_TEMP_CLEANUP_DAYS) | tonumber)) |
        .maintenance.test_log = (.maintenance.test_log // {}) |
        .maintenance.test_log.path = (.maintenance.test_log.path // strenv(DEFAULT_TEST_LOG_PATH)) |
        .maintenance.log = (.maintenance.log // {}) |
        .maintenance.log.path = (.maintenance.log.path // strenv(DEFAULT_LOG_PATH)) |
        .maintenance.log.level = (.maintenance.log.level // strenv(DEFAULT_LOG_LEVEL)) |
        .maintenance.log.format = (.maintenance.log.format // strenv(DEFAULT_LOG_FORMAT)) |
        .maintenance.log.max_size_mb = (.maintenance.log.max_size_mb // (strenv(DEFAULT_LOG_MAX_SIZE_MB) | tonumber)) |
        .maintenance.log.max_backups = (.maintenance.log.max_backups // (strenv(DEFAULT_LOG_MAX_BACKUPS) | tonumber)) |
        .tools = (.tools // (strenv(DEFAULT_TOOLS_YAML) | from_yaml))
    ' \
        "DANGEROUS_PATHS_YAML=$dangerous_paths_yaml" \
        "DEFAULT_TOOLS_YAML=$default_tools_yaml" \
        "DEFAULT_TEMP_CLEANUP_DAYS=7" \
        "DEFAULT_TEST_LOG_PATH=/dev/null" \
        "DEFAULT_LOG_PATH=.workhive/logs/workhive.log" \
        "DEFAULT_LOG_LEVEL=info" \
        "DEFAULT_LOG_FORMAT=text" \
        "DEFAULT_LOG_MAX_SIZE_MB=10" \
        "DEFAULT_LOG_MAX_BACKUPS=5" >/dev/null 2>&1 || return 1
}

_config_ensure_global_fields() {
    local config_file="$1"
    local scope="${2:-project}"
    if [[ "$scope" == "global" || "$scope" == "--global" ]]; then
        _config_seed_global_defaults "$config_file" || return 1
    fi
    return 0
}

# Public wrapper used by commands to ensure scope-specific seeded fields
# are present before schema validation/runtime use.
config_ensure_scope_fields() {
    local config_file="$1"
    local scope="${2:-project}"
    _config_ensure_global_fields "$config_file" "$scope"
}

_config_path_allowed() {
    local p="$1"
    local scope="$2"
    case "$p" in
        version|language|layout|layout.columns|layout.columns.[0-9]*|layout.columns.[0-9]*.width|layout.columns.[0-9]*.panes|layout.columns.[0-9]*.panes.[0-9]*|layout.columns.[0-9]*.panes.[0-9]*.height|layout.columns.[0-9]*.panes.[0-9]*.command|layout.columns.[0-9]*.panes.[0-9]*.role|worktree|worktree.base_path|worktree.auto_create|worktree.auto_delete|worktree.sync_files|worktree.sync_files.[0-9]*|worktree.sync_files.[0-9]*.source|worktree.sync_files.[0-9]*.mode|tmux|tmux.session_prefix|tmux.mouse_support|tmux.interaction_profile|tmux.env|tmux.env.*|tmux.theme|tmux.attach_delay|tmux.persistence|tmux.persistence.enabled|tmux.persistence.generation|tmux.persistence.auto_save_mode|tmux.persistence.auto_save_debounce|tmux.persistence.auto_save_interval|tmux.persistence.restore_processes|tmux.persistence.auto_save_env|tmux.persistence.auto_save_env.*|tmux.persistence.pre_kill_env|tmux.persistence.pre_kill_env.*|tmux.persistence.snapshot_history_limit|tmux.persistence.snapshot_max_size_mb|tmux.persistence.process_list|tmux.persistence.process_list.[0-9]*|maintenance|maintenance.temp_cleanup|maintenance.temp_cleanup.max_age_days|maintenance.test_log|maintenance.test_log.path|maintenance.test_log.level|maintenance.test_log.format|maintenance.test_log.max_size_mb|maintenance.test_log.max_backups|maintenance.log|maintenance.log.path|maintenance.log.level|maintenance.log.format|maintenance.log.max_size_mb|maintenance.log.max_backups)
            return 0
            ;;
        git_init|git_init.whitelist_paths|git_init.whitelist_paths.[0-9]*|git_init.dangerous_paths_extra|git_init.dangerous_paths_extra.[0-9]*|tools|tools.default|tools.list|tools.list.[0-9]*|tools.list.[0-9]*.tool_name|tools.list.[0-9]*.label|tools.list.[0-9]*.command|tools.list.[0-9]*.is_ai|tools.list.[0-9]*.enabled)
            [[ "$scope" == "global" ]]
            return $?
            ;;
        *)
            return 1
            ;;
    esac
}

_config_validate_duration_key() {
    local config_file="$1"
    local key="$2"
    local raw
    raw=$(yq ".$key" "$config_file" 2>/dev/null || echo "")
    _is_null_or_empty "$raw" && return 0
    if ! is_duration_string "$raw"; then
        log_error "Invalid duration for '$key': '$raw' (expected format like 3s, 1m, 2h)"
        return 1
    fi
    return 0
}

_config_validate_sync_files() {
    local config_file="$1"
    local count
    count=$(yq '.worktree.sync_files | length' "$config_file" 2>/dev/null || echo "0")
    [[ "$count" == "null" ]] && return 0
    [[ "$count" =~ ^[0-9]+$ ]] || {
        log_error "Invalid worktree.sync_files: expected an array"
        return 1
    }

    local i
    for ((i = 0; i < count; i++)); do
        local item_type
        item_type=$(yq ".worktree.sync_files[$i] | type" "$config_file" 2>/dev/null || echo "null")
        case "$item_type" in
            "!!map")
                local source_path mode
                source_path=$(yq ".worktree.sync_files[$i].source" "$config_file" 2>/dev/null || echo "")
                mode=$(yq ".worktree.sync_files[$i].mode" "$config_file" 2>/dev/null || echo "")

                if _is_null_or_empty "$source_path"; then
                    log_error "Invalid worktree.sync_files[$i]: missing required 'source' field"
                    log_info 'Example: worktree.sync_files: [{source: ".env", mode: "copy"}]'
                    return 1
                fi

                if [[ -n "$mode" && "$mode" != "null" ]]; then
                    case "$mode" in
                        copy|symlink) ;;
                        *)
                            log_error "Invalid worktree.sync_files[$i].mode: '$mode' (allowed: copy|symlink)"
                            return 1
                            ;;
                    esac
                fi
                ;;
            "!!str")
                local shorthand_source
                shorthand_source=$(yq ".worktree.sync_files[$i]" "$config_file" 2>/dev/null || echo "")
                if _is_null_or_empty "$shorthand_source"; then
                    log_error "Invalid worktree.sync_files[$i]: empty string is not allowed"
                    return 1
                fi
                ;;
            *)
                log_error "Invalid worktree.sync_files[$i]: expected object with 'source' and optional 'mode'"
                return 1
                ;;
        esac
    done

    return 0
}

_config_validate_env_map() {
    local config_file="$1"
    local path="$2"
    local label="$3"
    local env_type
    env_type=$(yq "${path} | type" "$config_file" 2>/dev/null || echo "!!null")
    case "$env_type" in
        "!!null") return 0 ;;
        "!!map") ;;
        *)
            log_error "Invalid ${label}: expected a map/object of KEY: value pairs"
            return 1
            ;;
    esac

    local key value_type
    while IFS=$'\t' read -r key value_type; do
        [[ -z "$key" ]] && continue

        if ! [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
            log_error "Invalid ${label} key: '$key' (expected [A-Za-z_][A-Za-z0-9_]*)"
            return 1
        fi

        case "$value_type" in
            "!!str"|"!!int"|"!!bool"|"!!float") ;;
            *)
                log_error "Invalid ${label}.${key}: expected scalar value (string/number/bool)"
                return 1
                ;;
        esac
    done < <(yq -r "${path} | to_entries[]? | [.key, (.value | type)] | @tsv" "$config_file" 2>/dev/null || true)

    return 0
}

_config_validate_schema_file() {
    local config_file="$1"
    local scope="${2:-project}"
    [[ -f "$config_file" ]] || return 0
    check_cmd yq || return 0

    local deprecated_keys=(
        "tmux.persistence.auto_save_debounce_seconds"
        "tmux.persistence.auto_save_heartbeat_interval"
        "tmux.persistence.auto_restore"
        "persistence"
        "init_state"
    )
    local dep
    for dep in "${deprecated_keys[@]}"; do
        local dep_val
        dep_val=$(yq ".$dep" "$config_file" 2>/dev/null || echo "")
        if ! _is_null_or_empty "$dep_val"; then
            log_error "Deprecated config key found: '$dep' in $config_file"
            return 1
        fi
    done

    local mode
    mode=$(yq '.tmux.persistence.auto_save_mode' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$mode" && "$mode" != "null" ]]; then
        case "$mode" in
            event|interval|hybrid) ;;
            *)
                log_error "Invalid tmux.persistence.auto_save_mode: '$mode' (allowed: event|interval|hybrid)"
                return 1
                ;;
        esac
    fi

    local interaction_profile
    interaction_profile=$(yq '.tmux.interaction_profile' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$interaction_profile" && "$interaction_profile" != "null" ]]; then
        case "$interaction_profile" in
            workspace|compatible) ;;
            *)
                log_error "Invalid tmux.interaction_profile: '$interaction_profile' (allowed: workspace|compatible)"
                return 1
                ;;
        esac
    fi

    local duration_keys=(
        "tmux.attach_delay"
        "tmux.persistence.auto_save_debounce"
        "tmux.persistence.auto_save_interval"
    )
    local dkey
    for dkey in "${duration_keys[@]}"; do
        _config_validate_duration_key "$config_file" "$dkey" || return 1
    done
    _config_validate_sync_files "$config_file" || return 1
    _config_validate_env_map "$config_file" '.tmux.env' 'tmux.env' || return 1
    _config_validate_env_map "$config_file" '.tmux.persistence.auto_save_env' 'tmux.persistence.auto_save_env' || return 1
    _config_validate_env_map "$config_file" '.tmux.persistence.pre_kill_env' 'tmux.persistence.pre_kill_env' || return 1

    local log_level log_format log_max_size_mb log_max_backups
    local test_log_level test_log_format test_log_max_size_mb test_log_max_backups
    local log_path test_log_path
    log_path=$(yq '.maintenance.log.path' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$log_path" && "$log_path" != "null" ]]; then
        if [[ "$(yq '.maintenance.log.path | type' "$config_file" 2>/dev/null || echo "")" != "!!str" ]]; then
            log_error "Invalid maintenance.log.path: expected string path"
            return 1
        fi
    fi

    test_log_path=$(yq '.maintenance.test_log.path' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$test_log_path" && "$test_log_path" != "null" ]]; then
        if [[ "$(yq '.maintenance.test_log.path | type' "$config_file" 2>/dev/null || echo "")" != "!!str" ]]; then
            log_error "Invalid maintenance.test_log.path: expected string path"
            return 1
        fi
    fi

    test_log_level=$(yq '.maintenance.test_log.level' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$test_log_level" && "$test_log_level" != "null" ]]; then
        case "$test_log_level" in
            debug|info|warn|error) ;;
            *)
                log_error "Invalid maintenance.test_log.level: '$test_log_level' (allowed: debug|info|warn|error)"
                return 1
                ;;
        esac
    fi

    test_log_format=$(yq '.maintenance.test_log.format' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$test_log_format" && "$test_log_format" != "null" ]]; then
        case "$test_log_format" in
            text|json) ;;
            *)
                log_error "Invalid maintenance.test_log.format: '$test_log_format' (allowed: text|json)"
                return 1
                ;;
        esac
    fi

    test_log_max_size_mb=$(yq '.maintenance.test_log.max_size_mb' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$test_log_max_size_mb" && "$test_log_max_size_mb" != "null" ]]; then
        if ! [[ "$test_log_max_size_mb" =~ ^[0-9]+$ ]] || (( test_log_max_size_mb < 1 )); then
            log_error "Invalid maintenance.test_log.max_size_mb: '$test_log_max_size_mb' (expected integer >= 1)"
            return 1
        fi
    fi

    test_log_max_backups=$(yq '.maintenance.test_log.max_backups' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$test_log_max_backups" && "$test_log_max_backups" != "null" ]]; then
        if ! [[ "$test_log_max_backups" =~ ^[0-9]+$ ]]; then
            log_error "Invalid maintenance.test_log.max_backups: '$test_log_max_backups' (expected integer >= 0)"
            return 1
        fi
    fi

    log_level=$(yq '.maintenance.log.level' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$log_level" && "$log_level" != "null" ]]; then
        case "$log_level" in
            debug|info|warn|error) ;;
            *)
                log_error "Invalid maintenance.log.level: '$log_level' (allowed: debug|info|warn|error)"
                return 1
                ;;
        esac
    fi

    log_format=$(yq '.maintenance.log.format' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$log_format" && "$log_format" != "null" ]]; then
        case "$log_format" in
            text|json) ;;
            *)
                log_error "Invalid maintenance.log.format: '$log_format' (allowed: text|json)"
                return 1
                ;;
        esac
    fi

    log_max_size_mb=$(yq '.maintenance.log.max_size_mb' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$log_max_size_mb" && "$log_max_size_mb" != "null" ]]; then
        if ! [[ "$log_max_size_mb" =~ ^[0-9]+$ ]] || (( log_max_size_mb < 1 )); then
            log_error "Invalid maintenance.log.max_size_mb: '$log_max_size_mb' (expected integer >= 1)"
            return 1
        fi
    fi

    log_max_backups=$(yq '.maintenance.log.max_backups' "$config_file" 2>/dev/null || echo "")
    if [[ -n "$log_max_backups" && "$log_max_backups" != "null" ]]; then
        if ! [[ "$log_max_backups" =~ ^[0-9]+$ ]]; then
            log_error "Invalid maintenance.log.max_backups: '$log_max_backups' (expected integer >= 0)"
            return 1
        fi
    fi

    local paths
    paths=$(yq '.. | path | join(".")' "$config_file" 2>/dev/null || echo "")
    local p
    while IFS= read -r p; do
        [[ -z "$p" ]] && continue
        _config_path_allowed "$p" "$scope" || {
            log_error "Unknown or disallowed config key: '$p' in $config_file"
            return 1
        }
    done <<< "$paths"
    return 0
}

_config_validate_file_once() {
    local config_file="$1"
    local scope="$2"
    [[ -f "$config_file" ]] || return 0
    if [[ "$scope" == "project" && "${WORKHIVE_SKIP_PROJECT_CONFIG_VALIDATE:-0}" == "1" ]]; then
        return 0
    fi
    local cache_key="${config_file}|${scope}"
    if [[ -n "${_CONFIG_VALIDATED_FILES[$cache_key]+set}" ]]; then
        return 0
    fi
    _config_validate_schema_file "$config_file" "$scope" || return 1
    _CONFIG_VALIDATED_FILES["$cache_key"]=1
    return 0
}

config_sync_schema_file() {
    local config_file="$1"
    local scope="${2:-project}"
    [[ "$scope" == "--global" ]] && scope="global"
    [[ -n "$config_file" && -f "$config_file" ]] || return 0
    _config_validate_schema_file "$config_file" "$scope"
}

config_sync_schema_scope() {
    local scope="${1:-project}"
    local config_file
    if [[ "$scope" == "global" || "$scope" == "--global" ]]; then
        config_file=$(config_get_global_file)
        _config_ensure_global_fields "$config_file" "global" || return 1
    else
        config_file=$(config_get_project_file)
    fi
    config_sync_schema_file "$config_file" "$scope"
}

config_get_global_file() {
    if [[ -n "${WORKHIVE_GLOBAL_CONFIG_FILE:-}" ]]; then
        echo "$WORKHIVE_GLOBAL_CONFIG_FILE"
        return 0
    fi
    echo "$(config_get_global_dir)/config.yaml"
}

config_get_project_file() {
    if [[ -n "${WORKHIVE_PROJECT_CONFIG_FILE:-}" ]]; then
        echo "$WORKHIVE_PROJECT_CONFIG_FILE"
        return 0
    fi
    echo "$(_workhive_dir 2>/dev/null)/config.yaml"
}

# Set a configuration value (project-level or global)
# Args: $1 = config key, $2 = config value, $3 = --global (optional)
config_set() {
    local key="$1"
    local value="$2"
    local scope="${3:-}"
    local previous_persistence_enabled=""

    if [[ ! "$key" =~ ^[a-zA-Z0-9_.]+$ ]]; then
        log_error "$(_i18n_fmt ERR_CONFIG_SET_UNSAFE_KEY_FMT "$key")"
        return 1
    fi

    local config_file
    if [[ "$scope" == "--global" ]]; then
        config_file=$(config_get_global_file)
        local global_dir
        global_dir=$(dirname "$config_file")
        mkdir -p "$global_dir" 2>/dev/null || true
        if [[ ! -f "$config_file" ]]; then
            config_get_defaults > "$config_file"
            _config_seed_global_defaults "$config_file" || return 1
        fi
    else
        config_file=$(config_get_project_file)
        local project_dir
        project_dir=$(dirname "$config_file")
        ensure_dir "$project_dir"
        if [[ ! -f "$config_file" ]]; then
            config_get_defaults > "$config_file"
        fi
    fi

    config_sync_schema_file "$config_file" "$scope"

    if check_cmd yq; then
        if [[ "$key" == "tmux.persistence.enabled" ]]; then
            previous_persistence_enabled=$(yq '.tmux.persistence.enabled // "false"' "$config_file" 2>/dev/null || echo "false")
            [[ "$previous_persistence_enabled" == "null" || -z "$previous_persistence_enabled" ]] && previous_persistence_enabled="false"
        fi

        if [[ "$value" == "true" || "$value" == "false" || "$value" =~ ^[0-9]+$ ]]; then
            config_atomic_yq_edit "$config_file" ".$key = $value" || return 1
        else
            config_atomic_yq_edit "$config_file" ".$key = strenv(VALUE)" "VALUE=$value" || return 1
        fi

        if [[ "$key" == "tmux.persistence.enabled" && "$value" == "true" && "$previous_persistence_enabled" != "true" ]]; then
            local current_generation next_generation
            current_generation=$(yq '.tmux.persistence.generation // 0' "$config_file" 2>/dev/null || echo "0")
            [[ "$current_generation" =~ ^[0-9]+$ ]] || current_generation=0
            next_generation=$((current_generation + 1))
            config_atomic_yq_edit "$config_file" ".tmux.persistence.generation = ${next_generation}" || return 1
            log_info "$(_i18n_fmt PERSIST_GENERATION_BUMPED_FMT "$next_generation")"
        fi

        config_sync_schema_file "$config_file" "$scope" || return 1

        cache_invalidate "config:*"

        log_debug "$(_i18n DEBUG_CONFIG_UPDATED) $key = $value ($config_file)"
        return 0
    else
        log_error "$(_i18n ERR_NO_YQ)"
        return 1
    fi
}

# Check if any config file exists (project-level or global)
config_exists() {
    # Fast check: try relative path before git rev-parse (~27ms saved)
    [[ -f ".workhive/config.yaml" ]] && return 0
    [[ -f "$(_workhive_dir)/config.yaml" ]] && return 0
    local global_file
    global_file=$(config_get_global_file)
    [[ -f "$global_file" ]]
}

# Check if project-level config exists
config_project_exists() {
    if [[ -n "${WORKHIVE_PROJECT_CONFIG_FILE:-}" ]]; then
        [[ -f "${WORKHIVE_PROJECT_CONFIG_FILE}" ]]
        return $?
    fi
    [[ -f ".workhive/config.yaml" ]] && return 0
    [[ -f "$(_workhive_dir)/config.yaml" ]]
}

# Check if global config exists
config_global_exists() {
    local global_file
    global_file=$(config_get_global_file)
    [[ -f "$global_file" ]]
}

config_get_global_init_lock_dir() {
    echo "$(config_get_global_dir)/.init.lock"
}

config_global_init_in_progress() {
    local lock_dir
    lock_dir=$(config_get_global_init_lock_dir)
    [[ -d "$lock_dir" ]] || return 1

    # Expire very old lock directories (defensive against abrupt termination + PID reuse).
    local now_ts lock_ts lock_age
    now_ts=$(date +%s 2>/dev/null || echo "")
    if [[ "$(uname)" == "Darwin" ]]; then
        lock_ts=$(stat -f %m "$lock_dir" 2>/dev/null || echo "")
    else
        lock_ts=$(stat -c %Y "$lock_dir" 2>/dev/null || echo "")
    fi
    if [[ "$now_ts" =~ ^[0-9]+$ && "$lock_ts" =~ ^[0-9]+$ ]]; then
        lock_age=$((now_ts - lock_ts))
        if [[ "$lock_age" -gt 3600 ]]; then
            cleanup_dir_quiet "$lock_dir"
            return 1
        fi
    fi

    local pid=""
    if [[ -f "${lock_dir}/pid" ]]; then
        read -r pid < "${lock_dir}/pid" 2>/dev/null || pid=""
    fi

    if [[ -n "$pid" ]] && [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null; then
        local proc_cmd
        proc_cmd=$(ps -p "$pid" -o command= 2>/dev/null || echo "")
        # Only treat as active lock when the running command is a workhive init.
        # This avoids PID-reuse false positives from unrelated workhive commands.
        if [[ "$proc_cmd" == *"workhive"* && "$proc_cmd" == *" init"* ]]; then
            return 0
        fi
    fi

    cleanup_dir_quiet "$lock_dir"
    return 1
}

config_global_init_lock_acquire() {
    local lock_dir
    lock_dir=$(config_get_global_init_lock_dir)
    mkdir -p "$(dirname "$lock_dir")" 2>/dev/null || true

    local attempts=0
    while ! mkdir "$lock_dir" 2>/dev/null; do
        if config_global_init_in_progress; then
            return 1
        fi
        attempts=$((attempts + 1))
        [[ "$attempts" -ge 2 ]] && return 1
    done

    printf '%s\n' "$$" > "${lock_dir}/pid" 2>/dev/null || true
    return 0
}

config_global_init_lock_release() {
    local lock_dir
    lock_dir=$(config_get_global_init_lock_dir)
    cleanup_dir_quiet "$lock_dir"
}

# Check if global config exists and no global init transaction lock is active.
config_global_ready() {
    local global_file
    global_file=$(config_get_global_file)
    [[ -f "$global_file" ]] || return 1

    config_global_init_in_progress && return 1
    return 0
}

# Initialize project config
config_init() {
    local wh_dir
    wh_dir=$(_workhive_dir)
    ensure_dir "$wh_dir"

    if config_project_exists; then
        log_warn "$(_i18n INFO_CONFIG_EXISTS) ${wh_dir}/config.yaml"
        return 1
    fi

    log_info "$(_i18n INFO_CREATING_CONFIG)"
    config_get_defaults > "${wh_dir}/config.yaml"
    config_sync_schema_file "${wh_dir}/config.yaml" "project"

    log_success "$(_i18n INFO_CONFIG_CREATED) ${wh_dir}/config.yaml"
    return 0
}

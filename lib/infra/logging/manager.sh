#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Logging Manager
# Centralized logging module (config-driven, global access)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_LOGGING_MANAGER_SH_LOADED:-}" ]] && return 0
readonly _LOGGING_MANAGER_SH_LOADED=1

# Log config defaults
readonly _LOG_DEFAULT_MAX_SIZE_MB=10
readonly _LOG_DEFAULT_MAX_BACKUPS=5
readonly _LOG_DEFAULT_LEVEL="info"
readonly _LOG_DEFAULT_FORMAT="text"

# Log file path (lazily initialized to avoid creating files outside project dirs)
_LOG_FILE=""
_LOG_INITIALIZED=false
_WORKHIVE_EXPECTED_ERROR_LOGGED=false
_LOG_MAX_SIZE_BYTES=0
_LOG_MAX_BACKUPS=5
_LOG_LEVEL="info"
_LOG_FORMAT="text"
_LOG_CHANNEL="runtime"
_LOG_LIGHT_NOTED=""
_LOG_SETTINGS_READY=false

_log_resolve_path() {
    local raw="${1:-}"
    [[ -z "$raw" ]] && return 1
    if [[ "$raw" == /* ]]; then
        printf '%s' "$raw"
        return 0
    fi
    local base
    base=$(_git_root 2>/dev/null || pwd)
    printf '%s/%s' "$base" "$raw"
}

_log_global_config_file() {
    if [[ -n "${WORKHIVE_GLOBAL_CONFIG_FILE:-}" ]]; then
        printf '%s' "$WORKHIVE_GLOBAL_CONFIG_FILE"
        return 0
    fi
    local cfg_dir
    cfg_dir="${WORKHIVE_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/workhive}"
    printf '%s/config.yaml' "$cfg_dir"
}

_log_config_get_key() {
    local key="$1"
    local project_cfg
    project_cfg="$(_workhive_dir)/config.yaml"
    local global_cfg
    global_cfg="$(_log_global_config_file)"
    local val=""

    if check_cmd yq; then
        if [[ -f "$project_cfg" ]]; then
            val=$(yq ".$key" "$project_cfg" 2>/dev/null || echo "")
        fi
        if [[ -z "$val" || "$val" == "null" ]]; then
            if [[ -f "$global_cfg" ]]; then
                val=$(yq ".$key" "$global_cfg" 2>/dev/null || echo "")
            fi
        fi
    fi

    if [[ -z "$val" || "$val" == "null" ]]; then
        printf '%s' ""
    else
        printf '%s' "$val"
    fi
}

_log_config_get_key_explicit() {
    local key="$1"
    local cfg_file=""
    local val=""

    if ! check_cmd yq; then
        return 1
    fi

    if declare -F config_find_file_with_key >/dev/null 2>&1; then
        cfg_file=$(config_find_file_with_key "$key" 2>/dev/null || true)
    fi

    [[ -z "$cfg_file" ]] && return 1

    val=$(yq ".$key" "$cfg_file" 2>/dev/null || echo "")
    if [[ -z "$val" || "$val" == "null" ]]; then
        return 1
    fi

    printf '%s' "$val"
}

_log_policy_get_with_fallback() {
    local primary="$1"
    local fallback="$2"
    local val=""
    val=$(_log_config_get_key "$primary")
    if [[ -z "$val" || "$val" == "null" ]]; then
        val=$(_log_config_get_key "$fallback")
    fi
    printf '%s' "$val"
}

_log_resolve_settings() {
    local max_size_mb max_backups level format
    local channel
    channel="${_LOG_CHANNEL:-runtime}"

    max_size_mb="${WORKHIVE_LOG_MAX_SIZE_MB:-}"
    if [[ -z "$max_size_mb" ]]; then
        if [[ "$channel" == "test" ]]; then
            max_size_mb=$(_log_policy_get_with_fallback "maintenance.test_log.max_size_mb" "maintenance.log.max_size_mb")
        else
            max_size_mb=$(_log_config_get_key "maintenance.log.max_size_mb")
        fi
    fi
    [[ -z "$max_size_mb" ]] && max_size_mb="$_LOG_DEFAULT_MAX_SIZE_MB"
    if ! [[ "$max_size_mb" =~ ^[0-9]+$ ]] || (( max_size_mb < 1 )); then
        max_size_mb="$_LOG_DEFAULT_MAX_SIZE_MB"
    fi
    _LOG_MAX_SIZE_BYTES=$((max_size_mb * 1024 * 1024))
    if [[ -n "${WORKHIVE_LOG_MAX_SIZE:-}" && "${WORKHIVE_LOG_MAX_SIZE}" =~ ^[0-9]+$ ]]; then
        _LOG_MAX_SIZE_BYTES="${WORKHIVE_LOG_MAX_SIZE}"
    fi

    max_backups="${WORKHIVE_LOG_MAX_BACKUPS:-}"
    if [[ -z "$max_backups" ]]; then
        if [[ "$channel" == "test" ]]; then
            max_backups=$(_log_policy_get_with_fallback "maintenance.test_log.max_backups" "maintenance.log.max_backups")
        else
            max_backups=$(_log_config_get_key "maintenance.log.max_backups")
        fi
    fi
    [[ -z "$max_backups" ]] && max_backups="$_LOG_DEFAULT_MAX_BACKUPS"
    if ! [[ "$max_backups" =~ ^[0-9]+$ ]]; then
        max_backups="$_LOG_DEFAULT_MAX_BACKUPS"
    fi
    _LOG_MAX_BACKUPS="$max_backups"

    level="${WORKHIVE_LOG_LEVEL:-}"
    if [[ -z "$level" ]]; then
        if [[ "$channel" == "test" ]]; then
            level=$(_log_policy_get_with_fallback "maintenance.test_log.level" "maintenance.log.level")
        else
            level=$(_log_config_get_key "maintenance.log.level")
        fi
    fi
    [[ -z "$level" ]] && level="$_LOG_DEFAULT_LEVEL"
    level=$(printf '%s' "$level" | tr '[:upper:]' '[:lower:]')
    case "$level" in
        debug|info|warn|error) ;;
        *) level="$_LOG_DEFAULT_LEVEL" ;;
    esac
    _LOG_LEVEL="$level"

    format="${WORKHIVE_LOG_FORMAT:-}"
    if [[ -z "$format" ]]; then
        if [[ "$channel" == "test" ]]; then
            format=$(_log_policy_get_with_fallback "maintenance.test_log.format" "maintenance.log.format")
        else
            format=$(_log_config_get_key "maintenance.log.format")
        fi
    fi
    [[ -z "$format" ]] && format="$_LOG_DEFAULT_FORMAT"
    format=$(printf '%s' "$format" | tr '[:upper:]' '[:lower:]')
    case "$format" in
        text|json) ;;
        *) format="$_LOG_DEFAULT_FORMAT" ;;
    esac
    _LOG_FORMAT="$format"
    _LOG_SETTINGS_READY=true
}

_log_ensure_settings() {
    if [[ "$_LOG_SETTINGS_READY" != true ]]; then
        _log_resolve_settings
    fi
}

_log_level_rank() {
    case "${1:-info}" in
        debug|DEBUG) echo "10" ;;
        info|INFO) echo "20" ;;
        warn|WARN) echo "30" ;;
        error|ERROR) echo "40" ;;
        *) echo "20" ;;
    esac
}

_log_should_write_level() {
    local message_level="$1"
    local msg_rank threshold_rank
    msg_rank=$(_log_level_rank "$message_level")
    threshold_rank=$(_log_level_rank "$_LOG_LEVEL")
    (( msg_rank >= threshold_rank ))
}

_log_json_escape() {
    local s="${1:-}"
    s="${s//\\/\\\\}"
    s="${s//\"/\\\"}"
    s="${s//$'\n'/\\n}"
    s="${s//$'\r'/\\r}"
    s="${s//$'\t'/\\t}"
    printf '%s' "$s"
}

# Initialize log file (only enabled when .workhive directory exists)
_log_init() {
    [[ "$_LOG_INITIALIZED" == true ]] && return 0

    _LOG_INITIALIZED=true

    # Explicit log file override (used by tests / operators who want
    # to separate runtime logs from project-local .workhive logs).
    if [[ -n "${WORKHIVE_LOG_FILE:-}" ]]; then
        if [[ "${WORKHIVE_TEST_MODE:-0}" == "1" ]]; then
            _LOG_CHANNEL="test"
        else
            _LOG_CHANNEL="runtime"
        fi
        _LOG_SETTINGS_READY=false
        _log_resolve_settings
        _LOG_FILE="${WORKHIVE_LOG_FILE}"
        local _override_dir
        _override_dir=$(dirname "$_LOG_FILE")
        mkdir -p "$_override_dir" 2>/dev/null || true
        _log_rotate
        return 0
    fi

    # Dedicated test-mode routing:
    # WORKHIVE_TEST_LOG_FILE > maintenance.test_log.path > maintenance.log.path > WORKHIVE_TEST_LOG_DIR fallback
    if [[ "${WORKHIVE_TEST_MODE:-0}" == "1" ]]; then
        _LOG_CHANNEL="test"
        _LOG_SETTINGS_READY=false
        _log_resolve_settings
        local test_file=""
        if [[ -n "${WORKHIVE_TEST_LOG_FILE:-}" ]]; then
            test_file="${WORKHIVE_TEST_LOG_FILE}"
        else
            test_file=$(_log_config_get_key_explicit "maintenance.test_log.path" || true)
            if [[ -n "$test_file" ]]; then
                test_file=$(_log_resolve_path "$test_file")
            else
                local runtime_file
                runtime_file=$(_log_config_get_key_explicit "maintenance.log.path" || true)
                if [[ -n "$runtime_file" ]]; then
                    test_file=$(_log_resolve_path "$runtime_file")
                else
                    local test_dir
                    test_dir="${WORKHIVE_TEST_LOG_DIR:-/tmp/workhive-test-logs}"
                    test_file="${test_dir}/workhive-test-${$}.log"
                fi
            fi
        fi
        _LOG_FILE="$test_file"
        local _test_dir
        _test_dir=$(dirname "$_LOG_FILE")
        mkdir -p "$_test_dir" 2>/dev/null || true
        _log_rotate
        return 0
    fi

    # Runtime log path override from config (project/global)
    _LOG_CHANNEL="runtime"
    _LOG_SETTINGS_READY=false
    _log_resolve_settings
    local cfg_log_path
    cfg_log_path=$(_log_config_get_key_explicit "maintenance.log.path" || true)
    if [[ -n "$cfg_log_path" ]]; then
        _LOG_FILE=$(_log_resolve_path "$cfg_log_path")
        local _cfg_dir
        _cfg_dir=$(dirname "$_LOG_FILE")
        mkdir -p "$_cfg_dir" 2>/dev/null || true
        _log_rotate
        return 0
    fi

    # Only enable file logging inside project directories (where .workhive/ exists)
    local _wh_dir
    _wh_dir=$(_workhive_dir 2>/dev/null) || return 0
    if [[ -d "$_wh_dir" ]]; then
        _LOG_FILE="${_wh_dir}/logs/workhive.log"
        mkdir -p "${_wh_dir}/logs"
        _log_rotate
    fi
}

# Log size control: rotate file when exceeding threshold.
# _LOG_MAX_BACKUPS:
#   0 => truncate current file
#   N => keep N backup files: file.1 .. file.N
_log_rotate() {
    [[ -z "$_LOG_FILE" || ! -f "$_LOG_FILE" ]] && return 0
    _log_ensure_settings

    local file_size
    file_size=$(wc -c < "$_LOG_FILE" 2>/dev/null | tr -d ' ')
    [[ -z "$file_size" ]] && return 0

    if [[ "$_LOG_MAX_SIZE_BYTES" =~ ^[0-9]+$ ]] && [[ "$file_size" -gt "$_LOG_MAX_SIZE_BYTES" ]]; then
        if [[ "$_LOG_MAX_BACKUPS" =~ ^[0-9]+$ ]] && (( _LOG_MAX_BACKUPS > 0 )); then
            local i
            local oldest="${_LOG_FILE}.${_LOG_MAX_BACKUPS}"
            cleanup_file_quiet "$oldest"
            for ((i=_LOG_MAX_BACKUPS-1; i>=1; i--)); do
                if [[ -f "${_LOG_FILE}.${i}" ]]; then
                    mv -f -- "${_LOG_FILE}.${i}" "${_LOG_FILE}.$((i + 1))" 2>/dev/null || true
                fi
            done
            mv -f -- "$_LOG_FILE" "${_LOG_FILE}.1" 2>/dev/null || true
            : > "$_LOG_FILE" 2>/dev/null || true
            return 0
        fi
        : > "$_LOG_FILE" 2>/dev/null || true
    fi
}

# Disable file logging (used by destroy when log directory is deleted)
_log_disable_file() {
    _LOG_FILE=""
}

# Write a single line to the log file
# Args: $1 = level (DEBUG/INFO/WARN/ERROR), $2 = message
_log_to_file() {
    if [[ "${WORKHIVE_FILE_LOG_MODE:-normal}" == "startup-light" ]]; then
        if [[ -z "$_LOG_LIGHT_NOTED" ]]; then
            _LOG_LIGHT_NOTED=1
            [[ "${WORKHIVE_DEBUG:-}" == "1" ]] && echo -e "${BLUE:-}[DEBUG]${NC:-} File logging suspended (startup-light mode)"
        fi
        return 0
    fi
    [[ -z "$_LOG_FILE" ]] && _log_init
    [[ -z "$_LOG_FILE" ]] && return 0
    _log_ensure_settings

    # Safety check: verify log directory still exists (prevent write failures after destroy)
    local _log_dir
    _log_dir=$(dirname "$_LOG_FILE")
    if [[ ! -d "$_log_dir" ]]; then
        _LOG_FILE=""
        return 0
    fi

    local level="$1"
    local msg="$2"
    _log_should_write_level "$level" || return 0
    local ts
    printf -v ts '%(%Y-%m-%d %H:%M:%S)T' -1
    local line=""
    if [[ "$_LOG_FORMAT" == "json" ]]; then
        line="{\"ts\":\"$(_log_json_escape "$ts")\",\"level\":\"$(_log_json_escape "$level")\",\"msg\":\"$(_log_json_escape "$msg")\"}"
    else
        line=$(printf '[%s] [%-5s] %s' "$ts" "$level" "$msg")
    fi
    if ! printf '%s\n' "$line" >> "$_LOG_FILE" 2>/dev/null; then
        # Degrade gracefully when the log path becomes unwritable at runtime.
        _LOG_FILE=""
        return 0
    fi
    _log_rotate
    return 0
}

# ──────────────────────────────────────────────────
# Logging functions (dual-write: console + file)
# ──────────────────────────────────────────────────
log_info() {
    echo -e "${BLUE}ℹ️  ${NC}$1"
    _log_to_file "INFO" "$1"
}

log_success() {
    echo -e "${GREEN}✓  ${NC}$1"
    _log_to_file "INFO" "✓ $1"
}

log_warn() {
    echo -e "${YELLOW}⚠️  ${NC}$1"
    _log_to_file "WARN" "$1"
}

log_error() {
    echo -e "${RED}✗  ${NC}$1" >&2
    _log_to_file "ERROR" "$1"
    _WORKHIVE_EXPECTED_ERROR_LOGGED=true
}

log_bold() {
    echo -e "${BOLD}$1${NC}"
    _log_to_file "INFO" "$1"
}

log_debug() {
    _log_to_file "DEBUG" "$1"
    if [[ "${WORKHIVE_DEBUG:-}" == "1" ]]; then
        echo -e "${BLUE}[DEBUG]${NC} $1"
    fi
}

# ──────────────────────────────────────────────────
# Operation audit logging (file-only, non-intrusive)
# For create/update/delete style actions and troubleshooting.
# ──────────────────────────────────────────────────
_op_log() {
    local level="$1"
    local action="$2"
    local phase="$3"
    local target="$4"
    local detail="${5:-}"
    if [[ -n "$detail" ]]; then
        detail="${detail//$'\n'/ }"
        detail="${detail//$'\r'/ }"
        if ((${#detail} > 240)); then
            detail="${detail:0:240}...(len=${#detail})"
        fi
    fi
    local line="[OP][${phase}] action=${action} target=${target}"
    [[ -n "$detail" ]] && line="${line} detail=${detail}"
    _log_to_file "$level" "$line"
}

op_begin() {
    local action="$1"
    local target="$2"
    local detail="${3:-}"
    _op_log "INFO" "$action" "BEGIN" "$target" "$detail"
}

op_ok() {
    local action="$1"
    local target="$2"
    local detail="${3:-}"
    _op_log "INFO" "$action" "OK" "$target" "$detail"
}

op_fail() {
    local action="$1"
    local target="$2"
    local rc="${3:-1}"
    local detail="${4:-}"
    _op_log "ERROR" "$action" "FAIL" "$target" "rc=${rc}${detail:+ ${detail}}"
}

# ──────────────────────────────────────────────────
# Phase-level performance timing
#
# Usage:
#   local t; t=$(perf_timer_start)
#   ... work ...
#   perf_timer_log "ls.bulk_tmux" "$t"
#
# Log format (stable, grep-friendly):
#   [PERF] phase=ls.bulk_tmux elapsed_ms=42
# ──────────────────────────────────────────────────

perf_timer_start() {
    if [[ -r /proc/uptime ]]; then
        awk '{printf "%.0f", $1 * 1000}' /proc/uptime
    else
        perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000' 2>/dev/null \
            || printf '%s000' "$(date +%s)"
    fi
}

perf_timer_log() {
    local phase="$1"
    local start_ms="$2"
    local detail="${3:-}"
    local end_ms
    end_ms=$(perf_timer_start)
    local elapsed_ms=$(( end_ms - start_ms ))
    local line="[PERF] phase=${phase} elapsed_ms=${elapsed_ms}"
    [[ -n "$detail" ]] && line="${line} ${detail}"
    _log_to_file "INFO" "$line"
}

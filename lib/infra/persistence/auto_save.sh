#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Auto-Save Strategy
# Hybrid strategy:
# - Event-triggered saves (layout/pane/session activity)
# - Optional timer fallback
# Both paths are deduplicated by runtime signature + debounce.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PERSISTENCE_AUTO_SAVE_SH_LOADED:-}" ]] && return 0
readonly _PERSISTENCE_AUTO_SAVE_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/infra/persistence/save.sh"

readonly _AUTO_SAVE_MARKER="workhive-auto-save"
readonly _AUTO_SAVE_HOOKS=(
    client-detached
    client-session-changed
    session-closed
    pane-exited
    pane-focus-in
    after-select-pane
    after-split-window
    after-kill-pane
    after-move-pane
    after-resize-pane
    after-swap-pane
    after-break-pane
    after-join-pane
    after-new-window
    after-kill-window
    after-move-window
    window-linked
    window-unlinked
    after-select-layout
    after-rename-window
    after-rename-session
)

# Escape a string for safe embedding inside single-quoted shell fragments.
# Replaces every ' with '\'' (end quote, escaped quote, reopen quote).
_auto_save_sq_escape() {
    local val="$1"
    printf '%s' "${val//\'/\'\"\'\"\'}"
}

# ──────────────────────────────────────────────────
# Install auto-save hooks on a session
# Called during session creation (session_apply_config)
# $1 = session_name
# ──────────────────────────────────────────────────
auto_save_install_hooks() {
    local session_name="$1"
    [[ -z "$session_name" ]] && return 1

    if ! _auto_save_is_enabled; then
        # Config may have been toggled off while session is alive.
        # Proactively remove old hooks/options to stop future triggers.
        auto_save_remove_hooks "$session_name"
        return 0
    fi

    local workhive_bin
    workhive_bin=$(_auto_save_find_bin)
    [[ -z "$workhive_bin" ]] && return 1

    local mode
    mode=$(_auto_save_get_mode)
    if [[ "$mode" == "interval" ]]; then
        tmux set-option -t "$session_name" "@${_AUTO_SAVE_MARKER}" "timer-only" 2>/dev/null || true
        log_debug "Auto-save hooks skipped (mode=interval): $session_name"
        return 0
    fi

    local hook_name
    local tmux_cli
    tmux_cli=$(tmux_runtime_cli_prefix)
    for hook_name in "${_AUTO_SAVE_HOOKS[@]}"; do
        local trigger_cmd
        trigger_cmd=$(_auto_save_build_trigger_cmd "$session_name" "$workhive_bin" "$hook_name")
        [[ -n "$trigger_cmd" ]] || continue
        tmux set-hook -t "$session_name" "$hook_name" "run-shell -b '$trigger_cmd'" 2>/dev/null || true
    done

    tmux set-option -t "$session_name" "@${_AUTO_SAVE_MARKER}" "active" 2>/dev/null || true

    local _as_runtime_server=""
    if declare -f tmux_runtime_server_name >/dev/null 2>&1; then
        _as_runtime_server=$(tmux_runtime_server_name 2>/dev/null || echo "unknown")
    fi
    log_debug "Auto-save hooks installed on: $session_name server=${_as_runtime_server}"
}

# ──────────────────────────────────────────────────
# Start the background auto-save timer
# Creates a timer loop that runs save at intervals
# $1 = session_name (to scope the timer)
# ──────────────────────────────────────────────────
auto_save_start_timer() {
    local session_name="$1"
    [[ -z "$session_name" ]] && return 1

    if ! _auto_save_is_enabled; then
        auto_save_remove_hooks "$session_name"
        return 0
    fi

    local mode
    mode=$(_auto_save_get_mode)

    local interval_seconds=0
    case "$mode" in
        event)
            log_debug "Auto-save timer skipped (mode=event): $session_name"
            return 0
            ;;
        interval|hybrid|*)
            interval_seconds=$(_auto_save_get_interval_seconds)
            ;;
    esac
    [[ "$interval_seconds" =~ ^[0-9]+$ ]] || interval_seconds=0
    [[ "$interval_seconds" -le 0 ]] && return 0
    local workhive_bin
    workhive_bin=$(_auto_save_find_bin)
    [[ -z "$workhive_bin" ]] && return 1

    local existing_interval
    existing_interval=$(tmux show-options -t "$session_name" -v "@workhive-save-interval" 2>/dev/null || echo "")
    if [[ -n "$existing_interval" ]]; then
        log_debug "Auto-save timer already running for: $session_name"
        return 0
    fi

    tmux set-option -t "$session_name" "@workhive-save-interval" "$interval_seconds" 2>/dev/null || true

    local sq_session
    sq_session=$(_auto_save_sq_escape "$session_name")
    local tmux_cli
    tmux_cli=$(_auto_save_sq_escape "$(tmux_runtime_cli_prefix)")
    local trigger_cmd
    trigger_cmd=$(_auto_save_build_trigger_cmd "$session_name" "$workhive_bin" "timer")
    [[ -n "$trigger_cmd" ]] || return 1

    tmux run-shell -t "$session_name" -b \
        "while ${tmux_cli} has-session -t '${sq_session}' 2>/dev/null; do ${trigger_cmd}; sleep ${interval_seconds}; done" \
        2>/dev/null || true

    log_debug "Auto-save timer started: mode=${mode}, interval=${interval_seconds}s, session=$session_name"
}

# ──────────────────────────────────────────────────
# Remove auto-save hooks from a session
# $1 = session_name
# ──────────────────────────────────────────────────
auto_save_remove_hooks() {
    local session_name="$1"
    [[ -z "$session_name" ]] && return 0

    local hook_name
    for hook_name in "${_AUTO_SAVE_HOOKS[@]}"; do
        tmux set-hook -t "$session_name" -u "$hook_name" 2>/dev/null || true
    done
    tmux set-option -t "$session_name" -u "@${_AUTO_SAVE_MARKER}" 2>/dev/null || true
    tmux set-option -t "$session_name" -u "@workhive-save-interval" 2>/dev/null || true
    tmux set-option -t "$session_name" -u "@workhive-save-last-ts" 2>/dev/null || true
    tmux set-option -t "$session_name" -u "@workhive-runtime-signature" 2>/dev/null || true

    log_debug "Auto-save hooks removed from: $session_name"
}

# ──────────────────────────────────────────────────
# Trigger an immediate save (called from hooks/timer)
# This is a lightweight wrapper for use in run-shell contexts
# ──────────────────────────────────────────────────
auto_save_trigger() {
    persistence_save 2>/dev/null || true
}

# ──────────────────────────────────────────────────
# Check if auto-save is enabled in config
# ──────────────────────────────────────────────────
_auto_save_is_enabled() {
    persistence_is_enabled
}

# Get the auto-save interval in seconds (0 = disabled)
_auto_save_get_interval_seconds() {
    local raw sec
    raw=$(persistence_cfg_auto_save_interval)
    sec=$(duration_to_seconds "$raw" 2>/dev/null || echo "")
    [[ "$sec" =~ ^[0-9]+$ ]] || sec=$(duration_to_seconds "$_PERSIST_DEFAULT_AUTO_SAVE_INTERVAL" 2>/dev/null || echo "")
    [[ "$sec" =~ ^[0-9]+$ ]] || sec="0"
    echo "$sec"
}

_auto_save_get_mode() {
    local mode
    mode=$(persistence_cfg_auto_save_mode 2>/dev/null || echo "$_PERSIST_DEFAULT_AUTO_SAVE_MODE")
    case "$mode" in
        event|interval|hybrid) ;;
        *) mode="$_PERSIST_DEFAULT_AUTO_SAVE_MODE" ;;
    esac
    echo "$mode"
}

_auto_save_get_debounce_seconds() {
    local raw sec
    raw=$(persistence_cfg_auto_save_debounce 2>/dev/null || echo "$_PERSIST_DEFAULT_AUTO_SAVE_DEBOUNCE")
    sec=$(duration_to_seconds "$raw" 2>/dev/null || echo "")
    [[ "$sec" =~ ^[0-9]+$ ]] || sec=$(duration_to_seconds "$_PERSIST_DEFAULT_AUTO_SAVE_DEBOUNCE" 2>/dev/null || echo "")
    [[ "$sec" =~ ^[0-9]+$ ]] || sec="0"
    echo "$sec"
}

# Find the workhive binary path
_auto_save_find_bin() {
    resolve_workhive_bin
}

_auto_save_build_trigger_cmd() {
    local session_name="$1"
    local workhive_bin="$2"
    local trigger_name="${3:-unknown}"
    [[ -n "$session_name" && -n "$workhive_bin" ]] || return 1

    local project_root
    project_root=$(_git_root 2>/dev/null || echo "")
    [[ -n "$project_root" ]] || return 1

    local prefix
    prefix=$(session_get_prefix 2>/dev/null || echo "")
    local debounce
    debounce=$(_auto_save_get_debounce_seconds)

    local sq_root sq_prefix sq_bin sq_session sq_debounce sq_trigger
    sq_root=$(_auto_save_sq_escape "$project_root")
    sq_prefix=$(_auto_save_sq_escape "$prefix")
    sq_bin=$(_auto_save_sq_escape "$workhive_bin")
    sq_session=$(_auto_save_sq_escape "$session_name")
    sq_debounce=$(_auto_save_sq_escape "$debounce")
    sq_trigger=$(_auto_save_sq_escape "$trigger_name")
    local env_prefix
    env_prefix=$(config_build_env_assignment_prefix_from_getter "config_get_persistence_auto_save_env_entries")
    local unset_prefix
    unset_prefix=$(config_build_unset_managed_env_script)

    printf '%s' "cd '${sq_root}' && ${unset_prefix}WORKHIVE_SAVE_PREFIX='${sq_prefix}' WORKHIVE_AUTO_SAVE_SESSION='${sq_session}' WORKHIVE_AUTO_SAVE_DEBOUNCE='${sq_debounce}' WORKHIVE_AUTO_SAVE_TRIGGER='${sq_trigger}' ${env_prefix}'${sq_bin}' _internal_save_runtime_debounced 2>/dev/null || true"
}

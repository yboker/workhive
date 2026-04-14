#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Tmux Adapter Implementation
# tmux adapter implementation
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_TERMINAL_TMUX_SH_LOADED:-}" ]] && return 0
readonly _TERMINAL_TMUX_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/base.sh"
source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/config/loader.sh"
source "${LIB_DIR}/utils/common.sh"
[[ -n "${LIB_DIR:-}" ]] && source "${LIB_DIR}/utils/i18n.sh" 2>/dev/null || true

_TERMINAL_TMUX_LAST_SEND_CLIENT="generic"
_TERMINAL_TMUX_LAST_SEND_MODE="plain"
_TERMINAL_TMUX_LAST_SEND_COMMIT="enter"
_TERMINAL_TMUX_LAST_SEND_CACHE_HIT="0"
_TERMINAL_TMUX_LAST_ATTACH_STATUS="unknown"
_TERMINAL_TMUX_LAST_ATTACH_STRATEGY="unknown"

terminal_adapter_name() {
    echo "tmux"
}

terminal_adapter_check_available() {
    command -v tmux &>/dev/null
}

_terminal_tmux_detect_client_size() {
    local cols="" rows=""

    if [[ -n "${TMUX:-}" ]]; then
        local size
        size=$(tmux display-message -p '#{client_width}|#{client_height}' 2>/dev/null || echo "")
        cols="${size%%|*}"
        rows="${size##*|}"
    fi

    if [[ -z "$cols" || -z "$rows" || ! "$cols" =~ ^[0-9]+$ || ! "$rows" =~ ^[0-9]+$ || "$cols" -le 0 || "$rows" -le 0 ]]; then
        local stty_size=""
        if [[ -t 1 ]]; then
            stty_size=$(stty size 2>/dev/null || echo "")
            rows="${stty_size%% *}"
            cols="${stty_size##* }"
        fi
    fi

    if [[ -z "$cols" || -z "$rows" || ! "$cols" =~ ^[0-9]+$ || ! "$rows" =~ ^[0-9]+$ || "$cols" -le 0 || "$rows" -le 0 ]]; then
        cols="${COLUMNS:-}"
        rows="${LINES:-}"
    fi

    if [[ "$cols" =~ ^[0-9]+$ && "$rows" =~ ^[0-9]+$ && "$cols" -gt 0 && "$rows" -gt 0 ]]; then
        printf '%s|%s\n' "$cols" "$rows"
        return 0
    fi

    return 1
}

_terminal_tmux_current_socket_path() {
    local tmux_env="${TMUX:-}"
    [[ -n "$tmux_env" ]] || return 1

    local socket_path="${tmux_env%%,*}"
    [[ -n "$socket_path" ]] || return 1
    printf '%s\n' "$socket_path"
}

_terminal_tmux_current_server_name() {
    local socket_path
    socket_path=$(_terminal_tmux_current_socket_path) || return 1
    basename "$socket_path"
}

_terminal_tmux_select_attach_strategy() {
    if [[ -z "${TMUX:-}" ]]; then
        printf 'outside-tmux\n'
        return 0
    fi

    local current_server target_server
    current_server=$(_terminal_tmux_current_server_name 2>/dev/null || printf '%s' '')
    target_server=$(tmux_runtime_server_name)

    if [[ -n "$current_server" && "$current_server" == "$target_server" ]]; then
        printf 'same-server\n'
        return 0
    fi

    printf 'cross-server\n'
}

_terminal_tmux_pre_attach_geometry_reconcile() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local size cols rows
    size=$(_terminal_tmux_detect_client_size 2>/dev/null || echo "")
    cols="${size%%|*}"
    rows="${size##*|}"

    if [[ "$cols" =~ ^[0-9]+$ && "$rows" =~ ^[0-9]+$ && "$cols" -gt 0 && "$rows" -gt 0 ]]; then
        log_debug "tmux pre-attach geometry reconcile: session=${session_name} size=${cols}x${rows}"
        _terminal_tmux_apply_geometry "$session_name" "$cols" "$rows"
    else
        log_debug "tmux pre-attach geometry reconcile: session=${session_name} size=unknown(skip)"
    fi
}

_terminal_tmux_set_window_size_latest() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local win_idx
    while IFS= read -r win_idx; do
        [[ -z "$win_idx" ]] && continue
        tmux set-window-option -t "${session_name}:${win_idx}" window-size latest 2>/dev/null || true
    done < <(tmux list-windows -t "$session_name" -F '#{window_index}' 2>/dev/null || true)
}

_terminal_tmux_refresh_clients_for_session() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local client_tty refreshed=0
    while IFS= read -r client_tty; do
        [[ -z "$client_tty" ]] && continue
        tmux refresh-client -t "$client_tty" 2>/dev/null || true
        refreshed=$((refreshed + 1))
    done < <(tmux list-clients -t "$session_name" -F '#{client_tty}' 2>/dev/null || true)
    log_debug "tmux refresh-clients: session=${session_name} refreshed=${refreshed}"
}

_terminal_tmux_reset_pane_modes() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local pane_id pane_mode reset_count=0
    while IFS='|' read -r pane_id pane_mode; do
        [[ -n "$pane_id" ]] || continue
        [[ "${pane_mode:-0}" == "1" ]] || continue
        tmux send-keys -t "$pane_id" -X cancel 2>/dev/null || true
        reset_count=$((reset_count + 1))
    done < <(tmux list-panes -t "$session_name" -F '#{pane_id}|#{pane_in_mode}' 2>/dev/null || true)

    if (( reset_count > 0 )); then
        _op_log "INFO" "tmux_reset_modes" "ok" "$session_name" "count=${reset_count}"
        log_debug "tmux reset pane modes: session=${session_name} count=${reset_count}"
    fi
}

_terminal_tmux_reset_session_key_table() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    tmux set-option -t "$session_name" key-table root 2>/dev/null || true
    log_debug "tmux reset key-table: session=${session_name} key_table=root"
}

_terminal_tmux_ensure_runtime_capabilities() {
    # TUI agents (Claude Code, Codex, Hermes) require focus-events for correct
    # redraw, focus-change handling, and input state management. The dedicated
    # workhive tmux server does not inherit user ~/.tmux.conf, so enforce this
    # on every config/apply and attach path, not only during session creation.
    tmux set-option -g focus-events on 2>/dev/null || true
}

_terminal_tmux_normalize_interactive_state() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    _terminal_tmux_ensure_runtime_capabilities
    _terminal_tmux_reset_session_key_table "$session_name"
    _terminal_tmux_reset_pane_modes "$session_name"
}

_terminal_tmux_log_client_state() {
    local session_name="$1"
    local phase="${2:-unknown}"
    [[ -n "$session_name" ]] || return 0

    local clients
    clients=$(tmux list-clients -t "$session_name" -F '#{client_tty}:#{client_session}:#{client_width}x#{client_height}:#{client_key_table}:#{client_prefix}:#{client_readonly}:#{client_control_mode}' 2>/dev/null | tr '\n' ';' || true)
    [[ -n "$clients" ]] || clients="none"
    _op_log "INFO" "tmux_client" "$phase" "$session_name" "clients=${clients}"
    log_debug "tmux client-state: phase=${phase} session=${session_name} clients=${clients}"
}

_terminal_tmux_log_render_state() {
    local session_name="$1"
    local phase="${2:-unknown}"
    [[ -n "$session_name" ]] || return 0

    local sizes panes
    sizes=$(tmux display-message -p -t "$session_name" '#{client_width}x#{client_height}|#{window_width}x#{window_height}' 2>/dev/null || echo "unknown|unknown")
    panes=$(tmux list-panes -t "$session_name" -F '#{pane_index}:#{pane_active}:#{pane_in_mode}:#{alternate_on}:#{pane_width}x#{pane_height}:#{cursor_x},#{cursor_y}:#{pane_title}' 2>/dev/null | tr '\n' ';' || true)
    _op_log "INFO" "tmux_render" "$phase" "$session_name" "sizes=${sizes} panes=${panes}"
    log_debug "tmux render-state: phase=${phase} session=${session_name} sizes=${sizes} panes=${panes}"
}

_terminal_tmux_log_geometry_drift() {
    local session_name="$1"
    [[ -n "$session_name" ]] || return 0

    local sizes client_size window_size
    sizes=$(tmux display-message -p -t "$session_name" '#{client_width}x#{client_height}|#{window_width}x#{window_height}' 2>/dev/null || echo "")
    client_size="${sizes%%|*}"
    window_size="${sizes##*|}"
    [[ -z "$client_size" || -z "$window_size" || "$sizes" == "$client_size" ]] && return 0

    if [[ "$client_size" != "$window_size" ]]; then
        log_debug "tmux geometry drift detected: session=${session_name} client=${client_size} window=${window_size}"
    fi
}

_terminal_tmux_apply_geometry() {
    local session_name="$1"
    local cols="$2"
    local rows="$3"
    [[ -n "$session_name" ]] || return 0

    tmux resize-window -t "$session_name" -x "$cols" -y "$rows" 2>/dev/null || true
    # resize-window can leave window-size in manual mode. Restore latest to keep
    # runtime resizing deterministic and avoid stale edge areas after drag-resize.
    _terminal_tmux_set_window_size_latest "$session_name"
    terminal_adapter_reflow_session_panes "$session_name" >/dev/null 2>&1 || true
    _terminal_tmux_normalize_interactive_state "$session_name"
    _terminal_tmux_refresh_clients_for_session "$session_name"
    _terminal_tmux_log_geometry_drift "$session_name"
    _terminal_tmux_log_render_state "$session_name" "post_geometry_apply"
    _terminal_tmux_log_client_state "$session_name" "post_geometry_apply"
}

terminal_adapter_create_session() {
    local session_name="$1"
    local workdir="$2"
    local runtime_server
    runtime_server=$(tmux_runtime_server_name 2>/dev/null || echo "unknown")
    local size cols rows
    size=$(_terminal_tmux_detect_client_size 2>/dev/null || echo "")
    cols="${size%%|*}"
    rows="${size##*|}"

    if [[ "$cols" =~ ^[0-9]+$ && "$rows" =~ ^[0-9]+$ && "$cols" -gt 0 && "$rows" -gt 0 ]]; then
        log_debug "tmux create-session: session=${session_name} server=${runtime_server} size=${cols}x${rows}"
        tmux new-session -d -x "$cols" -y "$rows" -s "$session_name" -c "$workdir"
    else
        log_debug "tmux create-session: session=${session_name} server=${runtime_server} size=default(detached)"
        tmux new-session -d -s "$session_name" -c "$workdir"
    fi
}

terminal_adapter_has_session() {
    local session_name="$1"
    tmux has-session -t "$session_name" 2>/dev/null
}

terminal_adapter_attach_session() {
    local session_name="$1"
    _TERMINAL_TMUX_LAST_ATTACH_STATUS="unknown"
    _TERMINAL_TMUX_LAST_ATTACH_STRATEGY=$(_terminal_tmux_select_attach_strategy)
    log_debug "tmux attach strategy: session=${session_name} strategy=${_TERMINAL_TMUX_LAST_ATTACH_STRATEGY}"

    case "$_TERMINAL_TMUX_LAST_ATTACH_STRATEGY" in
    same-server)
        _terminal_tmux_pre_attach_geometry_reconcile "$session_name"
        _terminal_tmux_normalize_interactive_state "$session_name"
        _terminal_tmux_log_render_state "$session_name" "pre_switch_client"
        _terminal_tmux_log_client_state "$session_name" "pre_switch_client"
        tmux switch-client -t "$session_name" && {
            _TERMINAL_TMUX_LAST_ATTACH_STATUS="attached"
            return 0
        }
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
        return 1
        ;;
    outside-tmux|cross-server)
        _terminal_tmux_pre_attach_geometry_reconcile "$session_name"
        _terminal_tmux_normalize_interactive_state "$session_name"
        _terminal_tmux_log_render_state "$session_name" "pre_attach"
        _terminal_tmux_log_client_state "$session_name" "pre_attach"
        local _attach_rc=0
        TMUX="" tmux attach -t "$session_name" || _attach_rc=$?
        # rc=0: normal detach; rc=1: session ended or not found
        # Only propagate if session genuinely doesn't exist (pre-check passed, so this
        # means it died during attach — treat as success since user saw the session)
        if [[ "$_attach_rc" -ne 0 ]]; then
            if ! tmux has-session -t "$session_name" 2>/dev/null; then
                _TERMINAL_TMUX_LAST_ATTACH_STATUS="ended"
                log_debug "$(_i18n_fmt DEBUG_ATTACH_ENDED_FMT "$session_name" "$_attach_rc")"
                return 0
            fi
            _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
            return "$_attach_rc"
        fi
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="detached"
        return 0
        ;;
    *)
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
        log_debug "tmux attach strategy selector returned unsupported value: ${_TERMINAL_TMUX_LAST_ATTACH_STRATEGY}"
        return 1
        ;;
    esac
}

terminal_adapter_get_last_attach_status() {
    printf '%s\n' "${_TERMINAL_TMUX_LAST_ATTACH_STATUS:-unknown}"
}

terminal_adapter_kill_session() {
    local session_name="$1"
    tmux kill-session -t "$session_name" 2>/dev/null
}

terminal_adapter_detach_session() {
    tmux detach-client 2>/dev/null
}

terminal_adapter_split_vertical() {
    local session_name="$1"
    local pane_target="${2:-0}"
    local percentage="${3:-50}"
    local workdir="${4:-}"

    if [[ -n "$workdir" ]]; then
        tmux split-window -h -p "$percentage" -c "$workdir" -t "${session_name}:0.${pane_target}"
        return $?
    fi
    tmux split-window -h -p "$percentage" -t "${session_name}:0.${pane_target}"
}

terminal_adapter_split_horizontal() {
    local session_name="$1"
    local pane_target="${2:-0}"
    local percentage="${3:-50}"
    local workdir="${4:-}"

    if [[ -n "$workdir" ]]; then
        tmux split-window -v -p "$percentage" -c "$workdir" -t "${session_name}:0.${pane_target}"
        return $?
    fi
    tmux split-window -v -p "$percentage" -t "${session_name}:0.${pane_target}"
}

terminal_adapter_send_keys() {
    local session_name="$1"
    local pane_index="$2"
    local keys="$3"

    # First send C-u (clear line) to prevent the first character from being captured by a race condition (e.g., captured by an existing process)
    # Then send the actual command string followed by Enter (C-m)
    tmux send-keys -t "${session_name}:0.${pane_index}" C-u "$keys" C-m
}

terminal_adapter_send_text() {
    local session_name="$1"
    local pane_index="$2"
    local text="$3"
    local no_enter="${4:-false}"
    local send_mode="${5:-auto}"
    local commit_mode="${6:-auto}"
    local target="${session_name}:0.${pane_index}"

    if [[ "$no_enter" == "true" ]]; then
        commit_mode="none"
    fi

    _terminal_tmux_send_text_with_auto "$target" "$text" "$send_mode" "$commit_mode" || {
        log_debug "tmux adapter: send_text failed for ${target} (mode=${send_mode}, commit=${commit_mode})"
        return 1
    }
}

terminal_adapter_get_last_send_meta() {
    printf 'client=%s mode=%s commit=%s cache_hit=%s\n' \
        "${_TERMINAL_TMUX_LAST_SEND_CLIENT:-generic}" \
        "${_TERMINAL_TMUX_LAST_SEND_MODE:-plain}" \
        "${_TERMINAL_TMUX_LAST_SEND_COMMIT:-enter}" \
        "${_TERMINAL_TMUX_LAST_SEND_CACHE_HIT:-0}"
}

_terminal_tmux_send_text_with_auto() {
    local target="$1"
    local text="$2"
    local send_mode="${3:-auto}"
    local commit_mode="${4:-auto}"

    local current_cmd client
    current_cmd=$(tmux display-message -p -t "$target" "#{pane_current_command}" 2>/dev/null || true)
    client=$(_terminal_tmux_detect_client "$current_cmd")
    _TERMINAL_TMUX_LAST_SEND_CLIENT="$client"
    _TERMINAL_TMUX_LAST_SEND_CACHE_HIT="0"

    local cached_mode="" cached_commit=""
    if [[ "$send_mode" == "auto" && "$commit_mode" == "auto" ]]; then
        local cached_pair
        cached_pair=$(_terminal_tmux_load_cached_strategy "$target" "$client" || true)
        if [[ -n "$cached_pair" ]]; then
            cached_mode="${cached_pair%%|*}"
            cached_commit="${cached_pair##*|}"
        fi
    fi

    local -a candidates=()
    if [[ -n "$cached_mode" && -n "$cached_commit" ]]; then
        candidates+=("${cached_mode}|${cached_commit}")
    fi
    _terminal_tmux_append_strategy_candidates candidates "$client" "$send_mode" "$commit_mode"

    local seen=""
    local candidate mode commit
    for candidate in "${candidates[@]}"; do
        mode="${candidate%%|*}"
        commit="${candidate##*|}"
        [[ "$seen" == *"|${mode}|${commit}|"* ]] && continue
        seen="${seen}|${mode}|${commit}|"
        if _terminal_tmux_send_text_with_strategy "$target" "$text" "$mode" "$commit"; then
            _TERMINAL_TMUX_LAST_SEND_MODE="$mode"
            _TERMINAL_TMUX_LAST_SEND_COMMIT="$commit"
            if [[ -n "$cached_mode" && "$mode" == "$cached_mode" && "$commit" == "$cached_commit" ]]; then
                _TERMINAL_TMUX_LAST_SEND_CACHE_HIT="1"
            fi
            if [[ "$send_mode" == "auto" && "$commit_mode" == "auto" ]]; then
                _terminal_tmux_store_cached_strategy "$target" "$client" "$mode" "$commit"
            fi
            return 0
        fi
    done

    return 1
}

_terminal_tmux_append_strategy_candidates() {
    local arr_name="$1"
    local client="$2"
    local send_mode="${3:-auto}"
    local commit_mode="${4:-auto}"
    local -a base_candidates=()
    local -a filtered_candidates=()

    case "$client" in
        codex)
            base_candidates=(
                "bracketed-paste|c-m"
                "bracketed-paste|enter"
                "paste|enter"
                "plain|enter"
                "plain|c-m"
            )
            ;;
        claude)
            base_candidates=(
                "plain|enter"
                "paste|enter"
                "plain|c-m"
                "bracketed-paste|enter"
            )
            ;;
        *)
            base_candidates=(
                "plain|enter"
                "paste|enter"
                "bracketed-paste|enter"
                "plain|c-m"
            )
            ;;
    esac

    if [[ "$commit_mode" == "none" ]]; then
        if [[ "$send_mode" == "auto" ]]; then
            filtered_candidates=("plain|none" "paste|none" "bracketed-paste|none")
        else
            filtered_candidates=("${send_mode}|none")
        fi
    else
        local item item_mode item_commit
        for item in "${base_candidates[@]}"; do
            item_mode="${item%%|*}"
            item_commit="${item##*|}"
            if [[ "$send_mode" != "auto" && "$item_mode" != "$send_mode" ]]; then
                continue
            fi
            if [[ "$commit_mode" != "auto" && "$item_commit" != "$commit_mode" ]]; then
                continue
            fi
            filtered_candidates+=("$item")
        done
    fi

    if [[ ${#filtered_candidates[@]} -eq 0 && "$send_mode" != "auto" && "$commit_mode" != "auto" ]]; then
        filtered_candidates=("${send_mode}|${commit_mode}")
    fi

    # shellcheck disable=SC2178
    local -n out_arr="$arr_name"
    local entry
    for entry in "${filtered_candidates[@]}"; do
        out_arr+=("$entry")
    done
}

_terminal_tmux_send_text_with_strategy() {
    local target="$1"
    local text="$2"
    local send_mode="$3"
    local commit_mode="$4"

    case "$send_mode" in
        plain)
            tmux send-keys -t "$target" -l "$text" 2>/dev/null || return 1
            ;;
        paste)
            tmux set-buffer -- "$text" 2>/dev/null || return 1
            tmux paste-buffer -t "$target" 2>/dev/null || return 1
            ;;
        bracketed-paste)
            tmux set-buffer -- "$text" 2>/dev/null || return 1
            tmux paste-buffer -p -t "$target" 2>/dev/null || return 1
            ;;
        *)
            return 1
            ;;
    esac

    case "$commit_mode" in
        none)
            return 0
            ;;
        enter)
            tmux send-keys -t "$target" Enter 2>/dev/null || return 1
            ;;
        c-m)
            tmux send-keys -t "$target" C-m 2>/dev/null || return 1
            ;;
        *)
            return 1
            ;;
    esac
    return 0
}

_terminal_tmux_detect_client() {
    local cmd="${1:-}"
    case "$cmd" in
        codex|*codex*) echo "codex" ;;
        claude|*claude*) echo "claude" ;;
        *) echo "generic" ;;
    esac
}

_terminal_tmux_strategy_cache_file() {
    if [[ -n "${WORKHIVE_PANE_SEND_CACHE_FILE:-}" ]]; then
        echo "$WORKHIVE_PANE_SEND_CACHE_FILE"
        return 0
    fi
    echo "${TMPDIR:-/tmp}/workhive-pane-send-strategies.cache"
}

_terminal_tmux_load_cached_strategy() {
    local target="$1"
    local client="$2"
    local cache_file
    cache_file=$(_terminal_tmux_strategy_cache_file)

    [[ -f "$cache_file" ]] || return 1

    local row mode commit
    row=$(awk -F'|' -v t="$target" -v c="$client" '($1==t && $2==c){print $0}' "$cache_file" 2>/dev/null | tail -n 1) || return 1
    [[ -n "$row" ]] || return 1
    mode=$(echo "$row" | awk -F'|' '{print $3}')
    commit=$(echo "$row" | awk -F'|' '{print $4}')
    [[ "$mode" =~ ^(plain|paste|bracketed-paste)$ ]] || return 1
    [[ "$commit" =~ ^(enter|c-m)$ ]] || return 1
    echo "${mode}|${commit}"
}

_terminal_tmux_store_cached_strategy() {
    local target="$1"
    local client="$2"
    local mode="$3"
    local commit="$4"
    local cache_file
    cache_file=$(_terminal_tmux_strategy_cache_file)

    [[ "$mode" =~ ^(plain|paste|bracketed-paste)$ ]] || return 0
    [[ "$commit" =~ ^(enter|c-m)$ ]] || return 0

    mkdir -p "$(dirname "$cache_file")" 2>/dev/null || true
    local tmp_file="${cache_file}.tmp.$$"
    if [[ -f "$cache_file" ]]; then
        awk -F'|' -v t="$target" -v c="$client" '!( $1==t && $2==c ) {print $0}' "$cache_file" > "$tmp_file" 2>/dev/null || : > "$tmp_file"
    else
        : > "$tmp_file"
    fi
    printf '%s|%s|%s|%s\n' "$target" "$client" "$mode" "$commit" >> "$tmp_file"
    mv -- "$tmp_file" "$cache_file" 2>/dev/null || true
}

# Spawn startup command directly in pane (without simulated typing echo).
# Args: $1=session_name, $2=pane_index, $3=command, $4=workdir(optional)
terminal_adapter_build_startup_runner_script() {
    # Clear visible content before launching the real command.
    # Only the visible screen is cleared (\033[2J + \033[H); scrollback is
    # intentionally preserved so that TUI apps (hermes, codex) remain
    # scrollable via tmux copy-mode.
    local unset_script
    unset_script=$(config_build_unset_managed_env_script)
    printf '%s' "${unset_script}clear 2>/dev/null || true; printf \"\\033[2J\\033[H\" || true; _startup_interrupted=false; trap '_startup_interrupted=true' INT TERM; (trap - INT TERM; eval -- \"\$1\"); _startup_rc=\$?; trap - INT TERM; if [[ \"\$_startup_interrupted\" == true ]]; then exit 130; fi; if (( _startup_rc >= 128 )); then exit \"\$_startup_rc\"; fi; exec \"\$2\" -l"
}

_terminal_tmux_clear_pane_history() {
    local target="$1"
    [[ -n "$target" ]] || return 0
    tmux clear-history -t "$target" 2>/dev/null || true
}

terminal_adapter_spawn_command_target() {
    local target="$1"
    local command="$2"
    local workdir="${3:-}"
    local shell_bin="${SHELL:-/bin/zsh}"
    local runner_script
    runner_script=$(terminal_adapter_build_startup_runner_script)
    local pane_geom
    pane_geom=$(tmux display-message -p -t "$target" '#{pane_width}x#{pane_height}' 2>/dev/null || echo "unknown")
    log_debug "tmux spawn-target: target=${target} geom=${pane_geom} cmd=${command%% *}"
    _terminal_tmux_clear_pane_history "$target"

    if [[ -n "$workdir" ]]; then
        tmux respawn-pane -k -t "$target" -c "$workdir" "$shell_bin" -lc "$runner_script" _ "$command" "$shell_bin" 2>/dev/null
        local rc=$?
        [[ $rc -ne 0 ]] && log_warn "tmux spawn-target failed: target=${target} rc=${rc} workdir='${workdir}' cmd='${command%% *}'"
        return $rc
    fi
    tmux respawn-pane -k -t "$target" "$shell_bin" -lc "$runner_script" _ "$command" "$shell_bin" 2>/dev/null
    local rc=$?
    [[ $rc -ne 0 ]] && log_warn "tmux spawn-target failed: target=${target} rc=${rc} workdir='<none>' cmd='${command%% *}'"
    return $rc
}

terminal_adapter_spawn_login_shell_target() {
    local target="$1"
    local workdir="${2:-}"
    local pane_geom
    pane_geom=$(tmux display-message -p -t "$target" '#{pane_width}x#{pane_height}' 2>/dev/null || echo "unknown")
    log_debug "tmux spawn-login-shell: target=${target} geom=${pane_geom} workdir='${workdir:-<none>}'"
    _terminal_tmux_clear_pane_history "$target"

    if [[ -n "$workdir" ]]; then
        tmux respawn-pane -k -t "$target" -c "$workdir" 2>/dev/null
        local rc=$?
        [[ $rc -ne 0 ]] && log_warn "tmux spawn-login-shell failed: target=${target} rc=${rc} workdir='${workdir}'"
        return $rc
    fi

    tmux respawn-pane -k -t "$target" 2>/dev/null
    local rc=$?
    [[ $rc -ne 0 ]] && log_warn "tmux spawn-login-shell failed: target=${target} rc=${rc} workdir='<none>'"
    return $rc
}

terminal_adapter_spawn_pane_command() {
    local session_name="$1"
    local pane_index="$2"
    local command="$3"
    local workdir="${4:-}"
    terminal_adapter_spawn_command_target "${session_name}:0.${pane_index}" "$command" "$workdir"
}

terminal_adapter_reflow_session_panes() {
    local session_name="$1"
    [[ -z "$session_name" ]] && return 1

    local line pane_id pane_w pane_h
    while IFS='|' read -r pane_id pane_w pane_h; do
        [[ -z "$pane_id" ]] && continue
        [[ "$pane_w" =~ ^[0-9]+$ ]] || continue
        [[ "$pane_h" =~ ^[0-9]+$ ]] || continue
        tmux resize-pane -t "$pane_id" -x "$pane_w" -y "$pane_h" 2>/dev/null || true
    done < <(tmux list-panes -t "$session_name" -F '#{pane_id}|#{pane_width}|#{pane_height}' 2>/dev/null || true)

    return 0
}

terminal_adapter_list_sessions() {
    tmux list-sessions -F "#{session_name}" 2>/dev/null || echo ""
}

# Get the number of panes in a session
terminal_adapter_count_panes() {
    local session_name="$1"
    tmux display-message -t "$session_name" -p '#{window_panes}' 2>/dev/null || echo "0"
}

# Get current tmux session name (must be called from within tmux)
terminal_adapter_get_current_session() {
    tmux display-message -p '#{session_name}' 2>/dev/null
}

# Get current pane index (must be called from within tmux)
terminal_adapter_get_current_pane_index() {
    tmux display-message -p '#{pane_index}' 2>/dev/null
}

# Get the running command in the current pane
terminal_adapter_get_pane_command() {
    tmux display-message -p '#{pane_current_command}' 2>/dev/null
}

# Get the working directory of the current pane
terminal_adapter_get_pane_cwd() {
    tmux display-message -p '#{pane_current_path}' 2>/dev/null
}

terminal_adapter_attach_session_readonly() {
    local session_name="$1"
    _TERMINAL_TMUX_LAST_ATTACH_STATUS="unknown"
    _TERMINAL_TMUX_LAST_ATTACH_STRATEGY=$(_terminal_tmux_select_attach_strategy)
    log_debug "tmux readonly attach strategy: session=${session_name} strategy=${_TERMINAL_TMUX_LAST_ATTACH_STRATEGY}"

    case "$_TERMINAL_TMUX_LAST_ATTACH_STRATEGY" in
    same-server)
        _terminal_tmux_pre_attach_geometry_reconcile "$session_name"
        _terminal_tmux_normalize_interactive_state "$session_name"
        _terminal_tmux_log_render_state "$session_name" "pre_switch_client_readonly"
        _terminal_tmux_log_client_state "$session_name" "pre_switch_client_readonly"
        tmux switch-client -t "$session_name" -r && {
            _TERMINAL_TMUX_LAST_ATTACH_STATUS="attached"
            return 0
        }
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
        return 1
        ;;
    outside-tmux|cross-server)
        _terminal_tmux_pre_attach_geometry_reconcile "$session_name"
        _terminal_tmux_normalize_interactive_state "$session_name"
        _terminal_tmux_log_render_state "$session_name" "pre_attach_readonly"
        _terminal_tmux_log_client_state "$session_name" "pre_attach_readonly"
        local _attach_rc=0
        TMUX="" tmux attach -t "$session_name" -r || _attach_rc=$?
        if [[ "$_attach_rc" -ne 0 ]]; then
            if ! tmux has-session -t "$session_name" 2>/dev/null; then
                _TERMINAL_TMUX_LAST_ATTACH_STATUS="ended"
                log_debug "$(_i18n_fmt DEBUG_ATTACH_ENDED_FMT "$session_name" "$_attach_rc")"
                return 0
            fi
            _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
            return "$_attach_rc"
        fi
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="detached"
        return 0
        ;;
    *)
        _TERMINAL_TMUX_LAST_ATTACH_STATUS="error"
        log_debug "tmux readonly attach strategy selector returned unsupported value: ${_TERMINAL_TMUX_LAST_ATTACH_STRATEGY}"
        return 1
        ;;
    esac
}

# Select a specific pane
terminal_adapter_select_pane() {
    local session_name="$1"
    local pane_index="$2"
    tmux select-pane -t "${session_name}:0.${pane_index}" 2>/dev/null
}

# Resize a pane
terminal_adapter_resize_pane() {
    local session_name="$1"
    local pane_index="$2"
    local direction="$3"
    local amount="${4:-5}"

    local flag=""
    case "$direction" in
        up)    flag="-U" ;;
        down)  flag="-D" ;;
        left)  flag="-L" ;;
        right) flag="-R" ;;
    esac

    tmux resize-pane -t "${session_name}:0.${pane_index}" "$flag" "$amount" 2>/dev/null
}

# Get session info (JSON format)
terminal_adapter_get_session_info() {
    local session_name="$1"
    tmux display-message -t "$session_name" -p \
        '{"name":"#{session_name}","windows":#{session_windows},"panes":#{window_panes},"created":"#{session_created}"}' 2>/dev/null
}

# Kill the current pane (tmux auto-redistributes space to siblings)
# Returns: 1 if this is the last pane in the session (cannot kill)
terminal_adapter_kill_pane() {
    local pane_count
    pane_count=$(tmux display-message -p '#{window_panes}' 2>/dev/null || echo "1")
    if [[ "$pane_count" -le 1 ]]; then
        return 1
    fi
    tmux kill-pane 2>/dev/null
}

terminal_adapter_kill_pane_target() {
    local session_name="$1"
    local pane_target="$2"

    if [[ "$pane_target" == %* ]]; then
        tmux kill-pane -t "$pane_target" 2>/dev/null
        return
    fi

    tmux kill-pane -t "${session_name}:${pane_target}" 2>/dev/null
}

terminal_adapter_resolve_pane_target() {
    local pane_id="$1"
    local pane_rows
    pane_rows=$(tmux list-panes -a -F '#{session_name}|#{window_index}|#{pane_index}|#{pane_id}' 2>/dev/null) || {
        log_debug "tmux adapter: resolve_pane_target failed to list panes"
        return 1
    }

    local session_name window_index pane_index current_pane_id
    while IFS='|' read -r session_name window_index pane_index current_pane_id; do
        [[ "$current_pane_id" == "$pane_id" ]] || continue
        printf '%s|%s|%s|%s\n' "$session_name" "$window_index" "$pane_index" "$current_pane_id"
        return 0
    done <<< "$pane_rows"

    log_debug "tmux adapter: pane target not found for pane_id=${pane_id}"
    return 1
}

# Split the current pane vertically (top/bottom), preserving left/right structure
# $1 - percentage for new pane (optional, default 50)
# $2 - command to run in new pane (optional)
terminal_adapter_split_current_pane() {
    local percentage="${1:-50}"
    local cmd="${2:-}"

    if ! _is_null_or_empty "$cmd"; then
        # -P -F prints the new pane's ID so we can target it precisely
        local new_pane_id
        new_pane_id=$(tmux split-window -v -p "$percentage" -P -F '#{pane_id}' 2>/dev/null)
        sleep 0.1
        if [[ -n "$new_pane_id" ]]; then
            tmux send-keys -t "$new_pane_id" C-u "$cmd" C-m 2>/dev/null
        fi
    else
        tmux split-window -v -p "$percentage" 2>/dev/null
    fi
}

terminal_adapter_apply_config() {
    local session_name="$1"

    # Clean historical session hooks/options before installing current hooks.
    # Otherwise freshly installed diagnostic hooks can be removed immediately.
    _tmux_cleanup_legacy_statusbar_watcher "$session_name"
    _tmux_apply_session_protection "$session_name"
    _tmux_apply_diagnostic_hooks "$session_name"
    _tmux_apply_base_options "$session_name"
    _tmux_apply_true_color "$session_name"

    # Propagate language setting into tmux session
    if [[ -n "${WORKHIVE_LANG:-}" ]]; then
        tmux set-environment -t "$session_name" WORKHIVE_LANG "$WORKHIVE_LANG" 2>/dev/null || true
    fi

    local tmux_major tmux_minor
    if ! _tmux_detect_version; then
        return 0
    fi
    tmux_major=$_TMUX_MAJOR
    tmux_minor=$_TMUX_MINOR

    if [[ "$tmux_major" -gt 3 || ("$tmux_major" -eq 3 && "$tmux_minor" -ge 2) ]]; then
        tmux set-option -t "$session_name" pane-border-lines single 2>/dev/null || true
    fi

    _tmux_apply_theme "$session_name"

    if [[ "$tmux_major" -gt 3 || ("$tmux_major" -eq 3 && "$tmux_minor" -ge 3) ]]; then
        _tmux_apply_popup_keybindings "$session_name"
    fi
}

_tmux_cleanup_legacy_statusbar_watcher() {
    local session_name="$1"
    local option
    for option in \
        @workhive_statusbar_segment \
        @workhive_statusbar_original_right \
        @workhive_statusbar_original_saved \
        @workhive_waiting_count \
        @workhive_running_count \
        @workhive_watcher_pid \
        @workhive_watcher_generation \
        @workhive_watcher_baseline_border \
        @workhive_watcher_baseline_active_border \
        @workhive_watcher_attention_color \
        @workhive_watcher_hooks_installed; do
        tmux set-option -t "$session_name" -u "$option" 2>/dev/null || true
    done

    local pane option_p
    while IFS= read -r pane; do
        [[ -z "$pane" ]] && continue
        for option_p in \
            @workhive_watcher_ui_state \
            @workhive_watcher_last_agent_state \
            @workhive_agent_state \
            @workhive_agent_state_source \
            @workhive_agent_state_seq \
            @workhive_agent_state_updated_at \
            @workhive_agent_running_since; do
            tmux set-option -pt "$pane" -u "$option_p" 2>/dev/null || true
        done
    done < <(tmux list-panes -t "$session_name" -F '#{pane_id}' 2>/dev/null || true)

    local hook_name
    for hook_name in pane-focus-in after-split-window after-kill-pane; do
        tmux set-hook -u -t "$session_name" "$hook_name" 2>/dev/null || true
    done
}

_tmux_apply_session_protection() {
    local session_name="$1"
    local tmux_cli
    tmux_cli=$(tmux_runtime_cli_prefix)
    tmux set-option -t "$session_name" destroy-unattached off 2>/dev/null || log_debug "Failed to set destroy-unattached for $session_name"
    tmux set-option -t "$session_name" exit-unattached off 2>/dev/null || log_debug "Failed to set exit-unattached for $session_name"
    tmux set-option -t "$session_name" remain-on-exit on 2>/dev/null || log_debug "Failed to set remain-on-exit for $session_name"
    tmux set-option -t "$session_name" remain-on-exit-format \
        '#{?pane_dead,#[fg=yellow]⚠ Process exited (#{pane_dead_status}). Press Enter to restart or Ctrl+B x to close pane.,}' \
        2>/dev/null || true
    tmux set-hook -t "$session_name" pane-died \
        "set-option -t '#{session_name}' key-table workhive-dead 2>/dev/null" 2>/dev/null || true
    tmux bind-key -T workhive-dead Enter if-shell \
        "${tmux_cli} display-message -p '#{pane_dead}' | grep -q 1" \
        "respawn-pane -k; set-option -t '#{session_name}' key-table root" \
        "send-keys Enter; set-option -t '#{session_name}' key-table root" 2>/dev/null || true
}

_tmux_apply_diagnostic_hooks() {
    local session_name="$1"
    local _diag_log
    local tmux_cli
    tmux_cli=$(tmux_runtime_cli_prefix)
    _diag_log="$(_workhive_dir)/logs/workhive.log"
    if [[ -d "$(dirname "$_diag_log")" ]]; then
        tmux set-hook -t "$session_name" pane-died \
            "run-shell 'echo \"[\$(date +%Y-%m-%dT%H:%M:%S)] [DIAG] pane-died: session=#{session_name} pane=#{pane_id} cmd=#{pane_current_command}\" >> \"${_diag_log}\"'" 2>/dev/null || true
        tmux set-hook -t "$session_name" client-attached \
            "run-shell '${tmux_cli} set-option -t \"#{session_name}\" key-table root 2>/dev/null || true; for pane in \$(${tmux_cli} list-panes -t \"#{session_name}\" -F \"#{pane_id}|#{pane_in_mode}\" 2>/dev/null | awk -F\"|\" '\''\$2==1{print \$1}'\''); do ${tmux_cli} send-keys -t \"\$pane\" -X cancel 2>/dev/null || true; done; echo \"[\$(date +%Y-%m-%dT%H:%M:%S)] [DIAG] client-attached: session=#{session_name} tty=#{client_tty} client_key_table=#{client_key_table} session_key_table=\$(${tmux_cli} show-options -v -t \"#{session_name}\" key-table 2>/dev/null || echo root) panes=\$(${tmux_cli} list-panes -t \"#{session_name}\" -F \"##{pane_index}:##{pane_active}:##{pane_in_mode}:##{alternate_on}:##{cursor_x},##{cursor_y}:##{pane_title}\" 2>/dev/null | tr \"\\n\" \";\" || true)\" >> \"${_diag_log}\"'" 2>/dev/null || true
        tmux set-hook -t "$session_name" client-session-changed \
            "run-shell 'echo \"[\$(date +%Y-%m-%dT%H:%M:%S)] [DIAG] client-session-changed: session=#{session_name} tty=#{client_tty} client_key_table=#{client_key_table}\" >> \"${_diag_log}\"'" 2>/dev/null || true
    fi
}

_tmux_get_interaction_profile() {
    local profile
    profile=$(config_get "tmux.interaction_profile" 2>/dev/null || echo "workspace")
    case "$profile" in
        workspace|compatible) ;;
        *) profile="workspace" ;;
    esac
    echo "$profile"
}

# ──────────────────────────────────────────────────
# Wheel Scroll Strategy (Strategy Pattern)
#
# State-machine-based wheel behavior. Both profiles use the same
# two-level tmux runtime checks (pane_in_mode, alternate_on) —
# no command-name special-casing.
#
# "workspace" profile decision chain:
#   1. pane_in_mode   → send-keys -M     (continue scrolling inside copy-mode)
#   2. alternate_on   → send-keys -M     (TUI apps manage own viewport)
#   3. fallback (up)  → copy-mode -eu    (enter copy-mode + page-up; -e auto-exits at bottom)
#   3. fallback (down)→ send-keys -M     (no-op outside copy-mode; scrolls inside it)
#
# "compatible" profile always enters copy-mode -e for any non-alternate pane.
#
# Why -eu instead of "copy-mode -e; send-keys -M":
#   The old pattern caused frame-stacking because it entered copy-mode
#   then re-injected the wheel event, producing double-render artifacts.
#   -eu atomically enters copy-mode and pages up without re-injecting.
# ──────────────────────────────────────────────────

_tmux_build_workspace_wheel_up() {
    # L1: pane_in_mode → send-keys -M  (continue scrolling inside copy-mode)
    # L2: alternate_on → send-keys -M  (TUI apps manage their own viewport)
    # L3: fallback     → copy-mode -eu (enter copy-mode + page-up; -e auto-exits at bottom)
    #
    # Why copy-mode -eu (not copy-mode -e + send-keys -M):
    #   -eu enters copy-mode and pages up in one atomic operation.
    #   The old "copy-mode -e; send-keys -M" caused frame-stacking because
    #   it entered copy-mode then forwarded the wheel event, creating a
    #   double-render. Pure -eu avoids this by not re-injecting the wheel
    #   event while still enabling history viewing.
    #   -e ensures copy-mode auto-exits when the user scrolls back to bottom,
    #   preventing panes from getting stuck in copy-mode.
    #
    # tmux nested if-shell quoting: each nesting level doubles backslash-quote escaping.
    cat <<'TMUX_BIND'
if-shell -F "#{pane_in_mode}" "send-keys -M" "if-shell -F \"#{alternate_on}\" \"send-keys -M\" \"copy-mode -eu\""
TMUX_BIND
}

_tmux_build_workspace_wheel_down() {
    # L1: pane_in_mode → send-keys -M  (scroll down inside copy-mode; -e auto-exits at bottom)
    # L2: alternate_on → send-keys -M  (TUI apps manage their own viewport)
    # L3: fallback     → send-keys -M  (no-op when not in copy-mode; allows natural scroll-down)
    cat <<'TMUX_BIND'
if-shell -F "#{pane_in_mode}" "send-keys -M" "if-shell -F \"#{alternate_on}\" \"send-keys -M\" \"send-keys -M\""
TMUX_BIND
}

_tmux_build_compatible_wheel_up() {
    cat <<'TMUX_BIND'
if-shell -F "#{pane_in_mode}" "send-keys -M" "if-shell -F \"#{alternate_on}\" \"send-keys -M\" \"copy-mode -e; send-keys -M\""
TMUX_BIND
}

_tmux_build_compatible_wheel_down() {
    cat <<'TMUX_BIND'
if-shell -F "#{pane_in_mode}" "send-keys -M" "if-shell -F \"#{alternate_on}\" \"send-keys -M\" \"send-keys -M\""
TMUX_BIND
}

_tmux_install_mouse_bindings() {
    local workspace_wheel_up workspace_wheel_down
    local compatible_wheel_up compatible_wheel_down

    workspace_wheel_up=$(_tmux_build_workspace_wheel_up)
    workspace_wheel_down=$(_tmux_build_workspace_wheel_down)
    compatible_wheel_up=$(_tmux_build_compatible_wheel_up)
    compatible_wheel_down=$(_tmux_build_compatible_wheel_down)

    tmux bind-key -T root WheelUpPane \
        if-shell -F '#{==:#{@workhive_interaction_profile},workspace}' \
        "$workspace_wheel_up" \
        "$compatible_wheel_up" 2>/dev/null || true
    tmux bind-key -T root WheelDownPane \
        if-shell -F '#{==:#{@workhive_interaction_profile},workspace}' \
        "$workspace_wheel_down" \
        "$compatible_wheel_down" 2>/dev/null || true
}

_tmux_apply_base_options() {
    local session_name="$1"
    local mouse_support
    local interaction_profile
    mouse_support=$(config_get "tmux.mouse_support" 2>/dev/null || echo "true")
    interaction_profile=$(_tmux_get_interaction_profile)
    tmux set-option -t "$session_name" "@workhive_interaction_profile" "$interaction_profile" 2>/dev/null || true
    _terminal_tmux_ensure_runtime_capabilities

    if [[ "$mouse_support" == "true" ]]; then
        _tmux_install_mouse_bindings
        tmux set-option -t "$session_name" mouse on 2>/dev/null || true
    else
        tmux set-option -t "$session_name" mouse off 2>/dev/null || true
    fi
    log_debug "tmux interaction-profile: session=${session_name} profile=${interaction_profile} mouse=${mouse_support}"
    tmux set-option -t "$session_name" status on 2>/dev/null || true
    tmux set-option -t "$session_name" status-position bottom 2>/dev/null || true
}

_tmux_apply_true_color() {
    local session_name="$1"

    tmux set-option -s default-terminal "screen-256color" 2>/dev/null || true

    local _tc_set
    _tc_set=$(tmux show-options -sv terminal-overrides 2>/dev/null || true)
    if [[ "$_tc_set" != *"xterm-256color:Tc"* ]]; then
        tmux set-option -sa terminal-overrides ",xterm-256color:Tc:RGB" 2>/dev/null || true
    fi
    if [[ "$_tc_set" != *"tmux-256color:Tc"* ]]; then
        tmux set-option -sa terminal-overrides ",tmux-256color:Tc:RGB" 2>/dev/null || true
    fi

    tmux set-environment -t "$session_name" COLORTERM "truecolor" 2>/dev/null || true
    tmux set-environment -t "$session_name" TERM "tmux-256color" 2>/dev/null || true
}

# Tmux version detection (cached in module-level vars)
_TMUX_MAJOR=""
_TMUX_MINOR=""

_tmux_detect_version() {
    if [[ -n "$_TMUX_MAJOR" ]]; then
        return 0
    fi
    local raw_version
    raw_version=$(get_tool_version_by_profile "tmux" "relaxed") || raw_version=""
    if [[ -z "$raw_version" ]]; then
        log_debug "$(_i18n DEBUG_TMUX_VERSION_FAIL)"
        return 1
    fi
    local parsed major minor
    parsed=$(parse_version_major_minor "$raw_version") || return 1
    IFS='|' read -r major minor <<< "$parsed"
    _TMUX_MAJOR="$major"
    _TMUX_MINOR="$minor"
    return 0
}

# Theme color registry (table-driven)
_tmux_get_theme_colors() {
    local theme="$1"
    case "$theme" in
        catppuccin-mocha) echo "#1e1e2e #cdd6f4 #89b4fa #45475a #89b4fa" ;;
        tokyo-night)      echo "#1a1b26 #c0caf5 #7aa2f7 #3b4261 #7aa2f7" ;;
        nord)             echo "#2e3440 #d8dee9 #88c0d0 #4c566a #88c0d0" ;;
        gruvbox)          echo "#282828 #ebdbb2 #fabd2f #504945 #fabd2f" ;;
        dracula)          echo "#282a36 #f8f8f2 #bd93f9 #44475a #bd93f9" ;;
        rose-pine)        echo "#191724 #e0def4 #c4a7e7 #26233a #c4a7e7" ;;
        everforest)       echo "#2d353b #d3c6aa #a7c080 #475258 #a7c080" ;;
        onedark)          echo "#282c34 #abb2bf #61afef #3e4452 #61afef" ;;
        *)                echo "#1e1e2e #cdd6f4 #89b4fa #45475a #89b4fa" ;;
    esac
}

_tmux_apply_theme() {
    local session_name="$1"

    local theme
    theme=$(config_get "tmux.theme" 2>/dev/null || echo "catppuccin-mocha")
    _is_null_or_empty "$theme" && theme="catppuccin-mocha"

    local colors
    colors=$(_tmux_get_theme_colors "$theme")
    local bg fg accent border border_active
    read -r bg fg accent border border_active <<< "$colors"

    log_debug "$(_i18n MSG_APPLYING_THEME) $theme"

    tmux set-option -t "$session_name" pane-border-style "fg=$border" 2>/dev/null || true
    tmux set-option -t "$session_name" pane-active-border-style "fg=$border_active" 2>/dev/null || true

    tmux set-option -t "$session_name" status-style "bg=$bg fg=$fg" 2>/dev/null || true
    tmux set-option -t "$session_name" status-left-length 40 2>/dev/null || true
    tmux set-option -t "$session_name" status-right-length 60 2>/dev/null || true
    tmux set-option -t "$session_name" status-left "#[bg=$accent,fg=$bg,bold]  #S #[bg=$bg,fg=$accent] " 2>/dev/null || true

    tmux set-option -t "$session_name" window-status-format "#[fg=$fg,dim]  #I.#P:#W " 2>/dev/null || true
    tmux set-option -t "$session_name" window-status-current-format "#[fg=$accent,bold]  #I.#P:#W " 2>/dev/null || true
    tmux set-option -t "$session_name" window-status-separator "" 2>/dev/null || true
}

_tmux_apply_popup_keybindings() {
    local session_name="$1"

    # Prefix+g — lazygit popup
    if command -v lazygit &>/dev/null; then
        tmux bind-key -T prefix g display-popup \
            -d "#{pane_current_path}" -w 80% -h 80% -E "lazygit" 2>/dev/null || true
    fi

    # Prefix+y — yazi file browser popup
    if command -v yazi &>/dev/null; then
        tmux bind-key -T prefix y display-popup \
            -d "#{pane_current_path}" -w 80% -h 80% -E "yazi" 2>/dev/null || true
    fi

    # Prefix+n — notes popup for current workspace branch
    local _notes_dir
    _notes_dir="$(_workhive_dir)/notes"
    [[ -d "$_notes_dir" ]] || mkdir -p "$_notes_dir" 2>/dev/null || true
    local _branch="${session_name##*@}"
    local _notes_file="${_notes_dir}/${_branch}.md"
    tmux bind-key -T prefix n display-popup \
        -d "#{pane_current_path}" -w 80% -h 80% \
        -E "touch '${_notes_file}' && \${EDITOR:-vim} '${_notes_file}'" 2>/dev/null || true

    # Prefix+L — layout configuration hint
    local _workhive_bin
    _workhive_bin=$(resolve_workhive_bin)
    local _workhive_bin_q
    _workhive_bin_q=$(printf '%q' "$_workhive_bin")
    tmux bind-key -T prefix L run-shell \
        "${_workhive_bin_q} config --help >/dev/null 2>&1; tmux display-message 'Layout config: use workhive config --layout'" 2>/dev/null || true
    # Prefix+w — attach to another workspace session (replaces removed 'switch' command)
    tmux bind-key -T prefix w run-shell \
        "${_workhive_bin_q} attach 2>/dev/null || tmux display-message 'Attach: use workhive attach'" 2>/dev/null || true
}

# Get current window's layout string (for persistence)
# Args: $1 = session name
# Returns (stdout): tmux layout string (e.g. "a]b,180x45,0,0{90x45,0,0,1,89x45,91,0,2}")
terminal_adapter_get_layout_string() {
    local session_name="$1"
    tmux list-windows -t "$session_name" -F "#{window_layout}" 2>/dev/null | head -n1
}

terminal_adapter_get_window_count() {
    local session_name="$1"
    tmux display-message -t "$session_name" -p '#{session_windows}' 2>/dev/null || echo "0"
}

terminal_adapter_is_session_attached() {
    local session_name="$1"
    local attached_raw
    attached_raw=$(tmux display-message -t "$session_name" -p '#{session_attached}' 2>/dev/null) || return 1
    [[ "${attached_raw:-0}" == "1" ]]
}

terminal_adapter_get_window_size() {
    local session_name="$1"
    tmux display-message -t "$session_name" -p '#{window_width}x#{window_height}' 2>/dev/null
}

terminal_adapter_resize_window() {
    local session_name="$1"
    local width="$2"
    local height="$3"
    _terminal_tmux_apply_geometry "$session_name" "$width" "$height"
}

terminal_adapter_list_panes_format() {
    local session_name="$1"
    local format="$2"
    tmux list-panes -t "$session_name" -F "$format" 2>/dev/null || {
        log_debug "tmux adapter: list_panes_format failed for session=${session_name}"
        return 1
    }
}

terminal_adapter_set_after_kill_pane_hook() {
    local session_name="$1"
    local hook_cmd="$2"
    tmux set-hook -t "$session_name" -u after-kill-pane 2>/dev/null || true
    tmux set-hook -t "$session_name" after-kill-pane "$hook_cmd" 2>/dev/null || true
}

terminal_adapter_run_shell() {
    local cmd="$1"
    tmux run-shell "$cmd" 2>/dev/null
}

terminal_adapter_display_message() {
    local session_name="$1"
    local message="$2"
    local duration="${3:-3000}"

    tmux set-option -t "$session_name" display-time "$duration" 2>/dev/null || true
    tmux display-message -t "$session_name" " $message" 2>/dev/null || true
}

terminal_adapter_set_option() {
    local session_name="$1"
    local option="$2"
    local value="$3"

    tmux set-option -t "$session_name" "$option" "$value" 2>/dev/null || true
}

# ──────────────────────────────────────────────────
# Pane-level option (user-defined @workhive_* metadata)
# ──────────────────────────────────────────────────

terminal_adapter_set_pane_option() {
    local session_name="$1"
    local pane_index="$2"
    local option="$3"
    local value="$4"
    tmux set-option -pt "${session_name}:0.${pane_index}" "$option" "$value" 2>/dev/null || true
}

terminal_adapter_get_pane_option() {
    local session_name="$1"
    local pane_index="$2"
    local option="$3"
    tmux show-options -pt "${session_name}:0.${pane_index}" -v "$option" 2>/dev/null || true
}

terminal_adapter_unset_pane_option() {
    local session_name="$1"
    local pane_index="$2"
    local option="$3"
    tmux set-option -pt "${session_name}:0.${pane_index}" -u "$option" 2>/dev/null || true
}

terminal_adapter_rename_session() {
    local old_session="$1"
    local new_session="$2"
    tmux rename-session -t "$old_session" "$new_session" 2>/dev/null
}

terminal_adapter_select_layout() {
    local session_name="$1"
    local layout="${2:-tiled}"
    tmux select-layout -t "$session_name" "$layout" 2>/dev/null
}

terminal_adapter_get_version() {
    _tmux_detect_version && echo "${_TMUX_MAJOR}.${_TMUX_MINOR}" || echo "0.0"
}

# ──────────────────────────────────────────────────
# Window operations (multi-window session support)
# ──────────────────────────────────────────────────

# Create a new window in an existing session
# Args: $1 = session_name, $2 = window_name, $3 = working_dir
terminal_adapter_new_window() {
    local session_name="$1"
    local window_name="$2"
    local working_dir="${3:-$(pwd)}"

    tmux new-window -t "$session_name" -n "$window_name" -c "$working_dir" 2>/dev/null
}

# Rename a window
# Args: $1 = session_name, $2 = window_index, $3 = new_name
terminal_adapter_rename_window() {
    local session_name="$1"
    local window_index="$2"
    local new_name="$3"

    tmux rename-window -t "${session_name}:${window_index}" "$new_name" 2>/dev/null
}

# Select (focus) a window
# Args: $1 = session_name, $2 = window_index
terminal_adapter_select_window() {
    local session_name="$1"
    local window_index="$2"

    tmux select-window -t "${session_name}:${window_index}" 2>/dev/null
}

# Send keys to a specific window's pane
# Args: $1 = session_name, $2 = window_index, $3 = pane_index, $4 = keys
terminal_adapter_send_keys_to_window() {
    local session_name="$1"
    local window_index="$2"
    local pane_index="$3"
    local keys="$4"

    tmux send-keys -t "${session_name}:${window_index}.${pane_index}" C-u "$keys" C-m 2>/dev/null
}

# List all window names for a session
# Args: $1 = session_name
# Returns (stdout): newline-separated list of window names
terminal_adapter_list_windows() {
    local session_name="$1"
    tmux list-windows -t "$session_name" -F "#{window_name}" 2>/dev/null || echo ""
}

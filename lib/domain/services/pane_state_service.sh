#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Pane State Service (Domain Layer)
# Unified pane runtime state detection for AI agents.
# Output states: running | waiting_input | idle | unknown
#
# Declarative Pane Metadata (Plan C):
# Each pane carries persistent metadata via tmux pane options
# (@workhive_pane_*) as the single source of truth for pane
# identity and lifecycle state.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PANE_STATE_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _PANE_STATE_SERVICE_SH_LOADED=1

source "${LIB_DIR}/infra/adapters/terminal/tmux_runtime.sh"
source "${LIB_DIR}/infra/tools/registry.sh"

# ──────────────────────────────────────────────────
# Declarative Pane Metadata API
#
# Fields stored as tmux pane options:
#   @workhive_pane_role            — positional key, e.g. "col_0_pane_0"
#   @workhive_pane_kind            — "agent" | "shell" | "tool"
#   @workhive_pane_desired_command — expected startup command (may be empty)
#   @workhive_pane_state           — "pending" | "launching" | "running" | "failed" | "idle"
#   @workhive_pane_last_error      — last error summary (may be empty)
#   @workhive_pane_updated_at      — ISO timestamp of last state change
# ──────────────────────────────────────────────────

readonly _PANE_META_PREFIX="@workhive_pane_"
readonly _PANE_META_FIELDS="role kind desired_command state last_error updated_at"

_pane_meta_classify_kind() {
    local cmd="${1:-}"
    if [[ -z "$cmd" ]]; then
        printf 'shell'
        return 0
    fi

    local cmd_bin="${cmd%% *}"
    cmd_bin="${cmd_bin##*/}"
    local normalized_name=""

    if normalized_name=$(tools_registry_normalize_ai_command "$cmd_bin" 2>/dev/null) && [[ -n "$normalized_name" ]]; then
        printf 'agent'
        return 0
    fi

    case "$cmd_bin" in
        lazygit|yazi|mc|ranger|k9s|htop|top|less|more|tail)
            printf 'tool' ;;
        vi|vim|nvim|emacs|nano)
            printf 'tool' ;;
        *)
            printf 'shell' ;;
    esac
}

_pane_meta_now() {
    date "+%Y-%m-%dT%H:%M:%S%z" 2>/dev/null || printf '%s' "unknown"
}

pane_meta_set() {
    local session_name="$1"
    local pane_index="$2"
    local field="$3"
    local value="$4"
    terminal_adapter_set_pane_option "$session_name" "$pane_index" "${_PANE_META_PREFIX}${field}" "$value"
}

pane_meta_get() {
    local session_name="$1"
    local pane_index="$2"
    local field="$3"
    terminal_adapter_get_pane_option "$session_name" "$pane_index" "${_PANE_META_PREFIX}${field}"
}

pane_meta_clear_one() {
    local session_name="$1"
    local pane_index="$2"
    local field="$3"
    terminal_adapter_unset_pane_option "$session_name" "$pane_index" "${_PANE_META_PREFIX}${field}"
}

pane_meta_clear_all() {
    local session_name="$1"
    local pane_index="$2"
    local field
    for field in $_PANE_META_FIELDS; do
        pane_meta_clear_one "$session_name" "$pane_index" "$field"
    done
}

pane_meta_write_identity() {
    local session_name="$1"
    local pane_index="$2"
    local role="$3"
    local kind="$4"
    local desired_command="${5:-}"

    pane_meta_set "$session_name" "$pane_index" "role" "$role"
    pane_meta_set "$session_name" "$pane_index" "kind" "$kind"
    pane_meta_set "$session_name" "$pane_index" "desired_command" "$desired_command"
    pane_meta_set "$session_name" "$pane_index" "state" "pending"
    pane_meta_set "$session_name" "$pane_index" "last_error" ""
    pane_meta_set "$session_name" "$pane_index" "updated_at" "$(_pane_meta_now)"

    # C1: Semantic classification log
    local normalized_name=""
    if [[ -n "$desired_command" ]]; then
        local cmd_bin="${desired_command%% *}"
        cmd_bin="${cmd_bin##*/}"
        normalized_name=$(tools_registry_normalize_ai_command "$cmd_bin" 2>/dev/null || true)
    fi
    local classifier_source="fallback"
    if [[ "$kind" == "agent" && -n "$normalized_name" ]]; then
        classifier_source="tool_registry"
    elif [[ "$kind" == "tool" ]]; then
        classifier_source="known_tool_list"
    fi
    log_debug "pane_meta_classify: session=$session_name pane=$pane_index desired=${desired_command:-(empty)} normalized=${normalized_name:-(empty)} kind=$kind source=$classifier_source"

    # C3: Semantic mismatch warning
    if [[ -n "$desired_command" && "$kind" == "shell" ]]; then
        log_warn "pane_meta_mismatch: desired_command is non-empty but kind=shell session=$session_name pane=$pane_index desired=$desired_command"
    fi
    if [[ "$kind" == "agent" && -z "$normalized_name" ]]; then
        log_warn "pane_meta_mismatch: kind=agent but normalization result is empty session=$session_name pane=$pane_index desired=${desired_command:-(empty)}"
    fi
}

pane_meta_transition_state() {
    local session_name="$1"
    local pane_index="$2"
    local new_state="$3"
    local error_msg="${4:-}"

    case "$new_state" in
        pending|launching|running|failed|idle) ;;
        *)
            log_debug "pane_meta_transition_state: invalid state=$new_state pane=$pane_index"
            return 1
            ;;
    esac

    pane_meta_set "$session_name" "$pane_index" "state" "$new_state"
    pane_meta_set "$session_name" "$pane_index" "updated_at" "$(_pane_meta_now)"
    if [[ "$new_state" == "failed" && -n "$error_msg" ]]; then
        pane_meta_set "$session_name" "$pane_index" "last_error" "$error_msg"
    fi
}

pane_meta_read_all() {
    local session_name="$1"
    local pane_index="$2"
    local role kind desired_command state last_error updated_at
    role=$(pane_meta_get "$session_name" "$pane_index" "role")
    kind=$(pane_meta_get "$session_name" "$pane_index" "kind")
    desired_command=$(pane_meta_get "$session_name" "$pane_index" "desired_command")
    state=$(pane_meta_get "$session_name" "$pane_index" "state")
    last_error=$(pane_meta_get "$session_name" "$pane_index" "last_error")
    updated_at=$(pane_meta_get "$session_name" "$pane_index" "updated_at")
    printf '%s|%s|%s|%s|%s|%s\n' \
        "${role:-}" "${kind:-}" "${desired_command:-}" \
        "${state:-}" "${last_error:-}" "${updated_at:-}"
}

pane_meta_has_metadata() {
    local session_name="$1"
    local pane_index="$2"
    local role
    role=$(pane_meta_get "$session_name" "$pane_index" "role")
    [[ -n "$role" ]]
}

_pane_state_cmd_bin() {
    local cmd="${1:-}"
    cmd="${cmd#"${cmd%%[![:space:]]*}"}"
    printf '%s' "${cmd%% *}"
}

_pane_state_protocol_to_state() {
    local state="${1:-}"
    case "$state" in
        running) printf '%s' 'running' ;;
        waiting|waiting_for_user) printf '%s' 'waiting_input' ;;
        idle|none|completed|failed) printf '%s' 'idle' ;;
        unknown) printf '%s' 'unknown' ;;
        *) printf '%s' '' ;;
    esac
}

pane_state_to_legacy() {
    local state="${1:-unknown}"
    case "$state" in
        running) printf '%s' 'running' ;;
        waiting_input) printf '%s' 'waiting' ;;
        *) printf '%s' 'none' ;;
    esac
}

_pane_state_detect_agent_name() {
    local pane_id="$1"
    local pane_cmd="$2"

    local source
    source=$(tmux show-options -pt "$pane_id" -v @workhive_agent_state_source 2>/dev/null || printf '%s' '')
    local source_name="${source##*:}"
    if [[ -n "$source_name" && "$source_name" != "$source" ]]; then
        local normalized_source
        normalized_source=$(tools_registry_normalize_ai_command "$source_name" 2>/dev/null || printf '%s' '')
        if [[ -n "$normalized_source" ]]; then
            printf '%s' "$normalized_source"
            return 0
        fi
    fi

    local cmd_bin
    cmd_bin=$(_pane_state_cmd_bin "$pane_cmd")
    local normalized_cmd
    normalized_cmd=$(tools_registry_normalize_ai_command "$cmd_bin" 2>/dev/null || printf '%s' '')
    if [[ -n "$normalized_cmd" ]]; then
        printf '%s' "$normalized_cmd"
        return 0
    fi

    printf '%s' ''
}

_pane_state_detect_agent_name_from_pid_tree() {
    local pane_pid="${1:-}"
    [[ "$pane_pid" =~ ^[0-9]+$ ]] || return 1
    _pane_state_pid_exists "$pane_pid" || return 1

    local -a pids=("$pane_pid")
    local child
    while IFS= read -r child; do
        [[ -n "$child" ]] && pids+=("$child")
    done < <(_pane_state_collect_descendants "$pane_pid")

    local pid
    for pid in "${pids[@]}"; do
        local proc_comm proc_bin normalized
        proc_comm=$(ps -p "$pid" -o comm= 2>/dev/null | awk 'NR==1 {print $1}')
        [[ -n "$proc_comm" ]] || continue
        proc_bin="${proc_comm##*/}"
        normalized=$(tools_registry_normalize_ai_command "$proc_bin" 2>/dev/null || printf '%s' '')
        if [[ -n "$normalized" ]]; then
            printf '%s' "$normalized"
            return 0
        fi
    done
    return 1
}

_pane_state_protocol_state() {
    local pane_id="$1"
    local raw
    raw=$(tmux show-options -pt "$pane_id" -v @workhive_agent_state 2>/dev/null || printf '%s' '')
    _pane_state_protocol_to_state "$raw"
}

_pane_state_pid_exists() {
    local pid="$1"
    [[ "$pid" =~ ^[0-9]+$ ]] || return 1
    kill -0 "$pid" 2>/dev/null
}

_pane_state_collect_descendants() {
    local root_pid="$1"
    command -v pgrep >/dev/null 2>&1 || return 0
    [[ "$root_pid" =~ ^[0-9]+$ ]] || return 0

    local -a queue=("$root_pid")
    local -a out=()
    declare -A seen=()

    while [[ ${#queue[@]} -gt 0 ]]; do
        local current="${queue[0]}"
        queue=("${queue[@]:1}")

        local child
        while IFS= read -r child; do
            [[ "$child" =~ ^[0-9]+$ ]] || continue
            [[ -n "${seen[$child]+x}" ]] && continue
            seen["$child"]=1
            out+=("$child")
            queue+=("$child")
        done < <(pgrep -P "$current" 2>/dev/null || true)
    done

    local pid
    for pid in "${out[@]}"; do
        printf '%s\n' "$pid"
    done
}

_pane_state_ps_triplet() {
    local pid="$1"
    ps -p "$pid" -o state= -o pcpu= -o comm= 2>/dev/null | awk 'NR==1 {print $1 "|" $2 "|" $3}'
}

_pane_state_cpu_ge_one() {
    local cpu="${1:-0}"
    awk -v c="$cpu" 'BEGIN {exit ((c+0) >= 1.0 ? 0 : 1)}' >/dev/null 2>&1
}

_pane_state_probe() {
    local pane_pid="$1"
    local pane_cmd="$2"
    local agent_name="$3"
    if [[ ! "$pane_pid" =~ ^[0-9]+$ ]]; then
        local pane_cmd_bin fallback_name
        pane_cmd_bin=$(_pane_state_cmd_bin "$pane_cmd")
        if [[ -n "$pane_cmd_bin" ]] && tools_registry_is_ai_command "$pane_cmd_bin"; then
            fallback_name=$(tools_registry_normalize_ai_command "$pane_cmd_bin" 2>/dev/null || printf '%s' '')
            [[ -z "$fallback_name" ]] && fallback_name="$pane_cmd_bin"
            printf 'running|low|INVALID_PANE_PID|%s\n' "$fallback_name"
        else
            printf '%s\n' "unknown|low|INVALID_PANE_PID"
        fi
        return 0
    fi
    _pane_state_pid_exists "$pane_pid" || { printf '%s\n' "idle|medium|PANE_PID_EXITED"; return 0; }

    local pane_cmd_bin
    pane_cmd_bin=$(_pane_state_cmd_bin "$pane_cmd")

    local ai_found=false
    local ai_active=false
    local ai_blocked=true
    local probe_error=false
    local detected_agent_name="${agent_name:-}"

    local -a pids=("$pane_pid")
    local child
    while IFS= read -r child; do
        [[ -n "$child" ]] && pids+=("$child")
    done < <(_pane_state_collect_descendants "$pane_pid")

    local pid
    for pid in "${pids[@]}"; do
        local triplet
        triplet=$(_pane_state_ps_triplet "$pid")
        if [[ -z "$triplet" ]]; then
            continue
        fi
        local proc_state proc_cpu proc_comm
        IFS='|' read -r proc_state proc_cpu proc_comm <<< "$triplet"
        [[ -n "$proc_state" && -n "$proc_comm" ]] || { probe_error=true; continue; }

        local proc_bin="${proc_comm##*/}"
        if tools_registry_is_ai_command "$proc_bin"; then
            ai_found=true
            if [[ -z "$detected_agent_name" ]]; then
                detected_agent_name=$(tools_registry_normalize_ai_command "$proc_bin" 2>/dev/null || printf '%s' '')
            fi

            case "${proc_state:0:1}" in
                R|U|D)
                    ai_active=true
                    ai_blocked=false
                    ;;
                S|I|T)
                    ;;
                *)
                    ai_blocked=false
                    ;;
            esac

            if _pane_state_cpu_ge_one "$proc_cpu"; then
                ai_active=true
                ai_blocked=false
            fi
        fi
    done

    if [[ "$ai_found" == false && -n "$pane_cmd_bin" ]] && tools_registry_is_ai_command "$pane_cmd_bin"; then
        ai_found=true
        ai_active=true
        ai_blocked=false
        if [[ -z "$detected_agent_name" ]]; then
            detected_agent_name=$(tools_registry_normalize_ai_command "$pane_cmd_bin" 2>/dev/null || printf '%s' '')
        fi
    fi

    local result_state result_confidence result_reason
    if [[ "$ai_found" == false ]]; then
        result_state="idle" result_confidence="high" result_reason="NO_AI_PROCESS"
    elif [[ "$ai_active" == true ]]; then
        result_state="running" result_confidence="medium" result_reason="AI_PROCESS_ACTIVE"
    elif [[ "$ai_blocked" == true ]]; then
        result_state="waiting_input" result_confidence="medium" result_reason="AI_PROCESS_BLOCKED"
    elif [[ "$probe_error" == true ]]; then
        result_state="unknown" result_confidence="low" result_reason="PROBE_PARTIAL"
    else
        result_state="unknown" result_confidence="low" result_reason="INSUFFICIENT_SIGNAL"
    fi

    # C2: Pane state probe log
    log_debug "pane_state_probe: pid=$pane_pid cmd=$pane_cmd agent=${detected_agent_name:-(none)} ai_found=$ai_found ai_active=$ai_active state=$result_state confidence=$result_confidence reason=$result_reason"

    # C3: Mismatch warning — AI descendant found but state is idle
    if [[ "$ai_found" == true && "$result_state" == "idle" ]]; then
        log_warn "pane_state_mismatch: AI process found in descendants but state=idle pid=$pane_pid agent=${detected_agent_name:-(none)}"
    fi

    printf '%s|%s\n' "${result_state}|${result_confidence}|${result_reason}" "${detected_agent_name:-}"
}

pane_state_collect_one() {
    local pane_index="$1"
    local pane_id="$2"
    local pane_pid="$3"
    local pane_cmd="$4"
    [[ -n "$pane_index" && -n "$pane_id" ]] || return 1

    local agent_name
    agent_name=$(_pane_state_detect_agent_name "$pane_id" "$pane_cmd")

    local state source confidence reason
    local protocol_state
    protocol_state=$(_pane_state_protocol_state "$pane_id")
    if [[ -n "$protocol_state" ]]; then
        state="$protocol_state"
        source="protocol"
        confidence="high"
        reason="PROTOCOL_STATE"
        if [[ -z "$agent_name" ]]; then
            # Protocol state exists but source metadata might be absent; do a
            # one-time process-tree fallback for accurate agent labeling.
            agent_name=$(_pane_state_detect_agent_name_from_pid_tree "$pane_pid" 2>/dev/null || printf '%s' '')
        fi
    else
        local probe
        local probe_agent_name=""
        probe=$(_pane_state_probe "$pane_pid" "$pane_cmd" "$agent_name")
        IFS='|' read -r state confidence reason probe_agent_name <<< "$probe"
        if [[ -z "$agent_name" && -n "$probe_agent_name" ]]; then
            agent_name="$probe_agent_name"
        fi
        source="probe"
    fi

    printf '%s|%s|%s|%s|%s|%s|%s|%s\n' \
        "$pane_index" "$pane_id" "${pane_pid:-0}" "${agent_name:-}" \
        "${state:-unknown}" "${source:-probe}" "${confidence:-low}" "${reason:-UNKNOWN}"
}

_pane_state_collect_session_row_smart() {
    local pane_index="$1"
    local pane_id="$2"
    local pane_pid="$3"
    local pane_cmd="$4"
    local raw_state="$5"
    local raw_source="$6"

    local agent_name=""
    local source_name="${raw_source##*:}"
    if [[ -n "$source_name" && "$source_name" != "$raw_source" ]]; then
        agent_name=$(tools_registry_normalize_ai_command "$source_name" 2>/dev/null || printf '%s' '')
    fi
    if [[ -z "$agent_name" ]]; then
        local cmd_bin normalized_cmd
        cmd_bin=$(_pane_state_cmd_bin "$pane_cmd")
        normalized_cmd=$(tools_registry_normalize_ai_command "$cmd_bin" 2>/dev/null || printf '%s' '')
        [[ -n "$normalized_cmd" ]] && agent_name="$normalized_cmd"
    fi

    local state source confidence reason
    local protocol_state
    protocol_state=$(_pane_state_protocol_to_state "$raw_state")
    if [[ -n "$protocol_state" ]]; then
        state="$protocol_state"
        source="protocol"
        confidence="high"
        reason="PROTOCOL_STATE"
    else
        if [[ -z "$agent_name" ]]; then
            state="idle"
            source="probe"
            confidence="high"
            reason="NO_AI_PROCESS"
        else
            local probe probe_agent_name=""
            probe=$(_pane_state_probe "$pane_pid" "$pane_cmd" "$agent_name")
            IFS='|' read -r state confidence reason probe_agent_name <<< "$probe"
            source="probe"
            if [[ -z "$agent_name" && -n "$probe_agent_name" ]]; then
                agent_name="$probe_agent_name"
            fi
        fi
    fi

    printf '%s|%s|%s|%s|%s|%s|%s|%s\n' \
        "$pane_index" "$pane_id" "${pane_pid:-0}" "${agent_name:-}" \
        "${state:-unknown}" "${source:-probe}" "${confidence:-low}" "${reason:-UNKNOWN}"
}

_pane_state_collect_session_row_simple() {
    local pane_index="$1"
    local pane_id="$2"
    local pane_pid="$3"
    local pane_cmd="$4"
    local raw_state="$5"
    local raw_source="$6"

    local agent_name=""
    local source_name="${raw_source##*:}"
    if [[ -n "$source_name" && "$source_name" != "$raw_source" ]]; then
        agent_name=$(tools_registry_normalize_ai_command "$source_name" 2>/dev/null || printf '%s' '')
    fi
    if [[ -z "$agent_name" ]]; then
        local cmd_bin normalized_cmd
        cmd_bin=$(_pane_state_cmd_bin "$pane_cmd")
        normalized_cmd=$(tools_registry_normalize_ai_command "$cmd_bin" 2>/dev/null || printf '%s' '')
        [[ -n "$normalized_cmd" ]] && agent_name="$normalized_cmd"
    fi

    local state source confidence reason
    local protocol_state
    protocol_state=$(_pane_state_protocol_to_state "$raw_state")
    if [[ -n "$protocol_state" ]]; then
        state="$protocol_state"
        source="protocol"
        confidence="high"
        reason="PROTOCOL_STATE"
    else
        if [[ -z "$agent_name" ]]; then
            state="idle"
            source="heuristic"
            confidence="high"
            reason="NO_AI_PROCESS"
        else
            state="unknown"
            source="heuristic"
            confidence="low"
            reason="INSUFFICIENT_SIGNAL"
        fi
    fi

    printf '%s|%s|%s|%s|%s|%s|%s|%s\n' \
        "$pane_index" "$pane_id" "${pane_pid:-0}" "${agent_name:-}" \
        "${state:-unknown}" "${source:-probe}" "${confidence:-low}" "${reason:-UNKNOWN}"
}

pane_state_collect_one_simple() {
    local pane_index="$1"
    local pane_id="$2"
    local pane_pid="$3"
    local pane_cmd="$4"
    local raw_state="$5"
    local raw_source="$6"
    [[ -n "$pane_index" && -n "$pane_id" ]] || return 1

    if [[ -z "$raw_state" ]]; then
        raw_state=$(tmux show-options -pt "$pane_id" -v @workhive_agent_state 2>/dev/null || printf '%s' '')
    fi
    if [[ -z "$raw_source" ]]; then
        raw_source=$(tmux show-options -pt "$pane_id" -v @workhive_agent_state_source 2>/dev/null || printf '%s' '')
    fi

    _pane_state_collect_session_row_simple \
        "$pane_index" "$pane_id" "$pane_pid" "$pane_cmd" "$raw_state" "$raw_source"
}

# Output rows:
# pane_index|pane_id|pane_pid|agent_name|state|source|confidence|reason
# NOTE: pane_current_command used here is diagnostic/display data only,
# NOT a source of truth for business decisions. See declarative metadata
# (pane_meta_*) for authoritative pane state.
pane_state_collect_session() {
    local session_name="$1"
    local strategy="${2:-deep}"
    [[ -n "$session_name" ]] || return 1

    case "$strategy" in
        simple)
            local line
            while IFS= read -r line; do
                [[ -n "$line" ]] || continue
                local pane_index pane_id pane_pid pane_cmd raw_state raw_source
                IFS='|' read -r pane_index pane_id pane_pid pane_cmd raw_state raw_source <<< "$line"
                [[ -n "$pane_index" && -n "$pane_id" ]] || continue
                if [[ -z "$pane_cmd" && -n "$pane_pid" && ! "$pane_pid" =~ ^[0-9]+$ ]]; then
                    pane_cmd="$pane_pid"
                    pane_pid="0"
                fi

                _pane_state_collect_session_row_simple \
                    "$pane_index" "$pane_id" "$pane_pid" "$pane_cmd" "$raw_state" "$raw_source"
            done < <(tmux list-panes -t "$session_name" -F "#{pane_index}|#{pane_id}|#{pane_pid}|#{pane_current_command}|#{@workhive_agent_state}|#{@workhive_agent_state_source}" 2>/dev/null)
            ;;
        smart)
            local line
            while IFS= read -r line; do
                [[ -n "$line" ]] || continue
                local pane_index pane_id pane_pid pane_cmd raw_state raw_source
                IFS='|' read -r pane_index pane_id pane_pid pane_cmd raw_state raw_source <<< "$line"
                [[ -n "$pane_index" && -n "$pane_id" ]] || continue
                if [[ -z "$pane_cmd" && -n "$pane_pid" && ! "$pane_pid" =~ ^[0-9]+$ ]]; then
                    pane_cmd="$pane_pid"
                    pane_pid="0"
                fi

                _pane_state_collect_session_row_smart \
                    "$pane_index" "$pane_id" "$pane_pid" "$pane_cmd" "$raw_state" "$raw_source"
            done < <(tmux list-panes -t "$session_name" -F "#{pane_index}|#{pane_id}|#{pane_pid}|#{pane_current_command}|#{@workhive_agent_state}|#{@workhive_agent_state_source}" 2>/dev/null)
            ;;
        deep|*)
            local line
            while IFS= read -r line; do
                [[ -n "$line" ]] || continue
                local pane_index pane_id pane_pid pane_cmd
                IFS='|' read -r pane_index pane_id pane_pid pane_cmd <<< "$line"
                [[ -n "$pane_index" && -n "$pane_id" ]] || continue
                if [[ -z "$pane_cmd" && -n "$pane_pid" && ! "$pane_pid" =~ ^[0-9]+$ ]]; then
                    pane_cmd="$pane_pid"
                    pane_pid="0"
                fi

                pane_state_collect_one "$pane_index" "$pane_id" "$pane_pid" "$pane_cmd"
            done < <(tmux list-panes -t "$session_name" -F "#{pane_index}|#{pane_id}|#{pane_pid}|#{pane_current_command}" 2>/dev/null)
            ;;
    esac
}

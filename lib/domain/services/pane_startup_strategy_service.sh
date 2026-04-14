#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Pane Startup Strategy Service (Domain Layer)
#
# Strategy selector + executor for pane bootstrap behavior.
# Keeps startup policies centralized and prevents branching logic
# from spreading across command/layout layers.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

[[ -n "${_PANE_STARTUP_STRATEGY_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _PANE_STARTUP_STRATEGY_SERVICE_SH_LOADED=1

source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/pane_state_service.sh"

# Strategy names:
# - command: run desired command in pane
# - shell_init: empty-command pane, initialize clean shell via no-op startup
# - noop: no dir + no command
pane_startup_select_strategy() {
    local desired_cmd="${1:-}"
    local workdir="${2:-}"

    if ! _is_null_or_empty "$desired_cmd"; then
        printf 'command\n'
        return 0
    fi
    if [[ -n "$workdir" ]]; then
        printf 'shell_init\n'
        return 0
    fi
    printf 'noop\n'
}

pane_startup_target_state() {
    local strategy="$1"
    local desired_cmd="${2:-}"

    case "$strategy" in
        command)
            local kind
            kind=$(_pane_meta_classify_kind "$desired_cmd")
            if [[ "$kind" == "agent" || "$kind" == "tool" ]]; then
                printf 'running\n'
            else
                printf 'idle\n'
            fi
            ;;
        shell_init|noop)
            printf 'idle\n'
            ;;
        *)
            printf 'idle\n'
            ;;
    esac
}

pane_startup_execute() {
    local strategy="$1"
    local branch="$2"
    local pane_index="$3"
    local desired_cmd="$4"
    local workdir="${5:-}"

    case "$strategy" in
        command)
            session_send_startup_command "$branch" "$pane_index" "$desired_cmd" "$workdir"
            ;;
        shell_init)
            session_prepare_shell_pane "$branch" "$pane_index" "$workdir"
            ;;
        noop)
            return 0
            ;;
        *)
            log_error "pane_startup_execute: unsupported strategy=${strategy}"
            return 1
            ;;
    esac
}

pane_startup_error_message() {
    local strategy="$1"
    local desired_cmd="${2:-}"

    case "$strategy" in
        command) printf 'startup command failed: %s\n' "$desired_cmd" ;;
        shell_init) printf 'empty-shell startup failed\n' ;;
        noop) printf 'noop startup failed\n' ;;
        *) printf 'startup strategy failed: %s\n' "$strategy" ;;
    esac
}

# Unified entry for pane startup state-machine transitions.
# Returns: 0 on success, 1 on failure.
pane_startup_apply() {
    local branch="$1"
    local session_name="$2"
    local pane_role="$3"
    local pane_index="$4"
    local desired_cmd="${5:-}"
    local workdir="${6:-}"

    local strategy
    strategy=$(pane_startup_select_strategy "$desired_cmd" "$workdir")
    log_debug "pane_startup_apply: role=${pane_role} idx=${pane_index} strategy=${strategy} cmd='${desired_cmd:-<empty>}' dir='${workdir:-<empty>}'"

    if [[ "$strategy" == "command" ]]; then
        pane_meta_transition_state "$session_name" "$pane_index" "launching" 2>/dev/null || true
    fi

    if pane_startup_execute "$strategy" "$branch" "$pane_index" "$desired_cmd" "$workdir"; then
        local target_state
        target_state=$(pane_startup_target_state "$strategy" "$desired_cmd")
        pane_meta_transition_state "$session_name" "$pane_index" "$target_state" 2>/dev/null || true
        log_debug "pane_startup_apply: success role=${pane_role} idx=${pane_index} strategy=${strategy} state=${target_state}"
        return 0
    fi

    local error_msg
    error_msg=$(pane_startup_error_message "$strategy" "$desired_cmd")
    pane_meta_transition_state "$session_name" "$pane_index" "failed" "$error_msg" 2>/dev/null || true
    log_error "pane_startup_apply: failed role=${pane_role} idx=${pane_index} strategy=${strategy} err=${error_msg}"
    return 1
}

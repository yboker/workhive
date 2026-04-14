#!/usr/bin/env bash
[[ -n "${_PROGRESS_SH_LOADED:-}" ]] && return 0
readonly _PROGRESS_SH_LOADED=1

_PROGRESS_PID=""
_PROGRESS_ACTIVE=false
_PROGRESS_EXIT_TRAP_PREV=""
_PROGRESS_EXIT_TRAP_MANAGED=false

_progress_spinner_chars=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')

_progress_select_strategy() {
    if [[ "${WORKHIVE_MACHINE_OUTPUT:-0}" == "1" ]]; then
        echo "none"
        return 0
    fi
    if [[ ! -t 2 ]]; then
        echo "none"
        return 0
    fi
    echo "spinner"
}

progress_start() {
    local message="${1:-Processing...}"
    if [[ "$_PROGRESS_ACTIVE" == true && -n "${_PROGRESS_PID:-}" ]]; then
        progress_stop
    fi
    local strategy
    strategy=$(_progress_select_strategy)
    if [[ "$strategy" == "none" ]]; then
        _PROGRESS_ACTIVE=false
        _PROGRESS_PID=""
        return 0
    fi

    # Register cleanup only when spinner output is actually enabled.
    _progress_register_exit_cleanup

    _PROGRESS_ACTIVE=true
    local delay_ms="${WORKHIVE_PROGRESS_DELAY_MS:-120}"
    if ! [[ "$delay_ms" =~ ^[0-9]+$ ]]; then
        delay_ms=120
    fi
    local delay_sec
    delay_sec=$(awk "BEGIN { printf \"%.3f\", (${delay_ms} / 1000) }")
    (
        trap 'exit 0' TERM HUP
        sleep "$delay_sec" || true
        # If command already finished during delay window, skip rendering.
        # Parent kill will terminate this process before first frame.
        local i=0
        local len=${#_progress_spinner_chars[@]}
        while true; do
            printf '\r  %s %s' "${_progress_spinner_chars[$((i % len))]}" "$message" >&2
            sleep 0.1
            i=$((i + 1))
        done
    ) &
    _PROGRESS_PID=$!
    disown "$_PROGRESS_PID" 2>/dev/null || true
}

_progress_cleanup_on_exit() {
    if [[ -n "${_PROGRESS_PID:-}" ]]; then
        kill "$_PROGRESS_PID" 2>/dev/null || true
        printf '\r\033[K' >&2
    fi
}

_progress_register_exit_cleanup() {
    if [[ "$_PROGRESS_EXIT_TRAP_MANAGED" == true ]]; then
        return 0
    fi

    local existing_trap
    existing_trap=$(trap -p EXIT 2>/dev/null || true)

    if [[ -z "$existing_trap" ]]; then
        _PROGRESS_EXIT_TRAP_PREV=""
        trap '_progress_cleanup_on_exit' EXIT
        _PROGRESS_EXIT_TRAP_MANAGED=true
        return 0
    fi

    if [[ "$existing_trap" == *"_progress_cleanup_on_exit"* ]]; then
        return 0
    fi

    _PROGRESS_EXIT_TRAP_PREV="$existing_trap"
    local existing_body="${existing_trap#trap -- \'}"
    existing_body="${existing_body%\' EXIT}"
    trap "_progress_cleanup_on_exit; ${existing_body}" EXIT
    _PROGRESS_EXIT_TRAP_MANAGED=true
}

_progress_unregister_exit_cleanup() {
    if [[ "$_PROGRESS_EXIT_TRAP_MANAGED" != true ]]; then
        return 0
    fi

    if [[ -n "$_PROGRESS_EXIT_TRAP_PREV" ]]; then
        eval "$_PROGRESS_EXIT_TRAP_PREV"
    else
        trap - EXIT
    fi
    _PROGRESS_EXIT_TRAP_PREV=""
    _PROGRESS_EXIT_TRAP_MANAGED=false
}

progress_stop() {
    if [[ "$_PROGRESS_ACTIVE" == true && -n "$_PROGRESS_PID" ]]; then
        kill "$_PROGRESS_PID" 2>/dev/null || true
        wait "$_PROGRESS_PID" 2>/dev/null || true
        printf '\r\033[K' >&2
        _PROGRESS_PID=""
        _PROGRESS_ACTIVE=false
    fi
    _progress_unregister_exit_cleanup
}

progress_step() {
    [[ "$(_progress_select_strategy)" == "none" ]] && return 0
    local current="$1"
    local total="$2"
    local message="${3:-}"
    [[ "$total" -le 0 ]] && total=1
    local pct=$((current * 100 / total))
    local filled=$((pct / 5))
    local empty=$((20 - filled))
    local bar=""
    local i
    for ((i=0; i<filled; i++)); do bar+="█"; done
    for ((i=0; i<empty; i++)); do bar+="░"; done
    printf '\r  [%s] %d/%d %s' "$bar" "$current" "$total" "$message" >&2
    if [[ $current -eq $total ]]; then
        printf '\n' >&2
    fi
}

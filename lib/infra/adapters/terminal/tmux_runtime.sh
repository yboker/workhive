#!/usr/bin/env bash

[[ -n "${_TERMINAL_TMUX_RUNTIME_SH_LOADED:-}" ]] && return 0
readonly _TERMINAL_TMUX_RUNTIME_SH_LOADED=1

_TMUX_RUNTIME_SERVER_NAME_CACHED=""
_TMUX_RUNTIME_SERVER_ROOT_CACHED=""

_tmux_runtime_sanitize_segment() {
    local value="${1:-}"
    value="${value//[^[:alnum:]_-]/-}"
    value="${value#-}"
    value="${value%-}"
    [[ -n "$value" ]] || value="workhive"
    printf '%s' "$value"
}

tmux_runtime_server_name() {
    if [[ -n "${WORKHIVE_TMUX_SERVER_NAME:-}" ]]; then
        printf '%s\n' "$WORKHIVE_TMUX_SERVER_NAME"
        return 0
    fi

    local root
    root=$(_git_root 2>/dev/null || pwd)
    if [[ -n "$_TMUX_RUNTIME_SERVER_NAME_CACHED" && "$_TMUX_RUNTIME_SERVER_ROOT_CACHED" == "$root" ]]; then
        printf '%s\n' "$_TMUX_RUNTIME_SERVER_NAME_CACHED"
        return 0
    fi

    local project instance_id
    project=$(basename "$root" 2>/dev/null || printf '%s' "workhive")
    project=$(_tmux_runtime_sanitize_segment "$project")

    instance_id=""
    if [[ -f "$(_workhive_dir 2>/dev/null)/instance_id" ]]; then
        instance_id=$(cat "$(_workhive_dir 2>/dev/null)/instance_id" 2>/dev/null || printf '%s' '')
    fi
    instance_id=$(_tmux_runtime_sanitize_segment "${instance_id:-default}")

    _TMUX_RUNTIME_SERVER_ROOT_CACHED="$root"
    _TMUX_RUNTIME_SERVER_NAME_CACHED="workhive-${project}-${instance_id}"
    printf '%s\n' "$_TMUX_RUNTIME_SERVER_NAME_CACHED"
}

tmux_runtime_cli_prefix() {
    local server_name
    server_name=$(tmux_runtime_server_name)
    printf 'tmux -L %q' "$server_name"
}

tmux() {
    local server_name
    server_name=$(tmux_runtime_server_name)
    command tmux -L "$server_name" "$@"
}

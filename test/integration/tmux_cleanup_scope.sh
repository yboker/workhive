#!/usr/bin/env bash

[[ -n "${_TEST_TMUX_CLEANUP_SCOPE_SH_LOADED:-}" ]] && return 0
readonly _TEST_TMUX_CLEANUP_SCOPE_SH_LOADED=1

_tmux_cleanup_normalize_scope() {
    local scope_dir="$1"

    if [[ -d "$scope_dir" ]]; then
        scope_dir=$(cd "$scope_dir" 2>/dev/null && pwd -P)
    fi

    scope_dir="${scope_dir%/}"
    [[ -n "$scope_dir" ]] || scope_dir='/'

    printf '%s\n' "$scope_dir"
}

_tmux_cleanup_session_matches_scope() {
    local session_name="$1"
    local scope_dir="$2"
    local pane_path=""

    while IFS= read -r pane_path; do
        [[ -n "$pane_path" ]] || continue
        case "$pane_path" in
            "$scope_dir"|"$scope_dir"/*)
                return 0
                ;;
        esac
    done < <(tmux list-panes -t "$session_name" -F '#{pane_current_path}' 2>/dev/null || true)

    return 1
}

_tmux_cleanup_sanitize_project_name() {
    local name="$1"

    if declare -f _sanitize_project_name >/dev/null 2>&1; then
        _sanitize_project_name "$name"
        return 0
    fi

    name="${name//./-}"
    name="${name//:/-}"
    name="${name// /-}"
    name="${name//\$/-}"
    name="${name//\`/-}"
    name="${name//!/-}"
    name="${name//#/-}"
    name="${name//[~]/-}"
    name="${name//^/-}"
    name="${name//\*/-}"
    name="${name//\?/-}"
    name="${name//\[/-}"
    name="${name//\]/-}"

    printf '%s\n' "$name"
}

tmux_cleanup_kill_sessions_in_scope() {
    local scope_dir="$1"
    local normalized_scope=""
    local session_name=""

    command -v tmux &>/dev/null || return 0
    [[ -n "$scope_dir" ]] || return 0

    normalized_scope=$(_tmux_cleanup_normalize_scope "$scope_dir")

    while IFS= read -r session_name; do
        [[ -n "$session_name" ]] || continue
        if _tmux_cleanup_session_matches_scope "$session_name" "$normalized_scope"; then
            tmux kill-session -t "$session_name" 2>/dev/null || true
        fi
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
}

tmux_cleanup_kill_workhive_sessions_in_scope() {
    local scope_dir="$1"
    local normalized_scope=""
    local scope_project=""
    local session_name=""

    command -v tmux &>/dev/null || return 0
    [[ -n "$scope_dir" ]] || return 0

    normalized_scope=$(_tmux_cleanup_normalize_scope "$scope_dir")
    scope_project=$(_tmux_cleanup_sanitize_project_name "$(basename "$normalized_scope")")

    while IFS= read -r session_name; do
        [[ -n "$session_name" ]] || continue
        case "$session_name" in
            *"@${scope_project}@"*) ;;
            *)
                continue
                ;;
        esac
        # Kill if pane path matches scope OR if pane path check fails
        # (handles macOS /tmp->/private/tmp symlink and deleted tmp dirs)
        if _tmux_cleanup_session_matches_scope "$session_name" "$normalized_scope"; then
            tmux kill-session -t "$session_name" 2>/dev/null || true
        elif [[ "$scope_dir" == /tmp/* || "$scope_dir" == /private/tmp/* ]]; then
            tmux kill-session -t "$session_name" 2>/dev/null || true
        fi
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
}

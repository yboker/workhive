#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Layout Rebuild Helper
# Shared logic for force-rebuilding layout (open --force-layout, attach --force-layout)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent duplicate load
[[ -n "${_LAYOUT_REBUILD_HELPER_SH_LOADED:-}" ]] && return 0
readonly _LAYOUT_REBUILD_HELPER_SH_LOADED=1

# Dependencies (layout_service pulls in session_service)
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/infra/persistence/manager.sh"

# layout_rebuild_force — Full layout rebuild: kill session, create session,
# create layout, apply pane commands from config.yaml, select pane.
#
# Args:
#   $1 = branch (worktree name)
#   $2 = workdir (session root path for session_create/layout_create)
#   $3 = pane_workdir (optional, for pane cd; default: workdir)
#   $4 = strict_template_rebuild (optional, true/false):
#        true  => skip snapshot backup/restore paths (config.yaml only rebuild)
#        false => keep backup/rollback behavior
#
# Returns:
#   0 = success
#   1 = session_create failed
#   2 = layout_create failed (session exists but layout incomplete)
#   3 = rebuild failed, previous session restored from snapshot
#
# Caller responsibilities: logging, error handling (e.g. ask_yes_no on return 2)
layout_rebuild_force() {
    local branch="$1"
    local workdir="$2"
    local pane_workdir="${3:-$workdir}"
    local strict_template_rebuild="${4:-false}"
    local session_name
    session_name=$(session_get_name "$branch")

    local had_session=false
    local backup_saved=false
    if session_exists "$session_name"; then
        had_session=true
        if [[ "$strict_template_rebuild" != "true" ]] && \
            persistence_save_session "$branch" "pre-layout-rebuild" "layout_rebuild_force" 2>/dev/null; then
            backup_saved=true
        fi
    fi

    # Kill may fail if session doesn't exist — that's OK, we'll create a fresh one
    session_kill "$branch" "$strict_template_rebuild" 2>/dev/null || true
    if ! session_create "$branch" "$workdir"; then
        if [[ "$strict_template_rebuild" != "true" && "$had_session" == true && "$backup_saved" == true ]]; then
            if persistence_restore_session "$branch" 2>/dev/null; then
                return 3
            fi
        fi
        return 1
    fi
    if ! layout_create "$branch" "$workdir"; then
        if [[ "$strict_template_rebuild" != "true" && "$had_session" == true && "$backup_saved" == true ]]; then
            session_kill "$branch" "$strict_template_rebuild" 2>/dev/null || true
            if persistence_restore_session "$branch" 2>/dev/null; then
                return 3
            fi
        fi
        return 2
    fi

    layout_apply_commands "$branch" "$pane_workdir" 2>/dev/null || true
    layout_select_pane "$branch" 0
    return 0
}

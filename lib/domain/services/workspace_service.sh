#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Workspace Service
# Domain layer: workspace lifecycle orchestration
# Encapsulates transactional creation/destruction of worktree + session + layout
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Source guard: prevent double-loading
[[ -n "${_WORKSPACE_SERVICE_SH_LOADED:-}" ]] && return 0
readonly _WORKSPACE_SERVICE_SH_LOADED=1

# Dependencies (Domain layer only)
source "${LIB_DIR}/domain/services/worktree_service.sh"
source "${LIB_DIR}/domain/services/session_service.sh"
source "${LIB_DIR}/domain/services/layout_service.sh"
source "${LIB_DIR}/utils/common.sh"
source "${LIB_DIR}/utils/i18n.sh"
source "${LIB_DIR}/infra/persistence/manager.sh"

# ──────────────────────────────────────────────────
# Transaction state (module-level for trap callback access)
# ──────────────────────────────────────────────────
_WS_TX_WORKTREE_CREATED=false
_WS_TX_SESSION_CREATED=false
_WS_TX_BRANCH=""
_WS_TX_COMPLETED=false

_workspace_prepare_force_recreate() {
    local branch="$1"
    local session_name="$2"

    [[ -n "$branch" && -n "$session_name" ]] || return 0

    local current_session=""
    current_session=$(terminal_adapter_get_current_session 2>/dev/null || true)
    [[ -n "$current_session" ]] || return 0

    if [[ "$current_session" == "$session_name" ]]; then
        log_info "$(_i18n HINT_FORCE_RECREATE_OUTSIDE_SESSION)"
        log_info "Force recreate: detaching current tmux client before rebuilding ${session_name}"
        session_detach 2>/dev/null || true
    fi

    return 0
}

# Transaction rollback: clean up already-created resources
_workspace_rollback() {
    if [[ "$_WS_TX_COMPLETED" == true ]]; then
        return 0
    fi
    log_warn "$(_i18n MSG_OPEN_INTERRUPTED)"
    if [[ "$_WS_TX_SESSION_CREATED" == true ]]; then
        log_info "$(_i18n MSG_ROLLBACK_SESSION)"
        session_kill "$_WS_TX_BRANCH" 2>/dev/null || true
    fi
    if [[ "$_WS_TX_WORKTREE_CREATED" == true ]]; then
        log_info "$(_i18n MSG_ROLLBACK_WORKTREE)"
        worktree_remove "$_WS_TX_BRANCH" "true" 2>/dev/null || true
    fi
}

# ──────────────────────────────────────────────────
# Step 1: Prepare working directory (worktree or project root)
# Args: $1=branch, $2=worktree_mode, $3=project_root
# Result: returns working directory via _WS_PREPARED_DIR
# Returns: 0=success, 1=failure
# ──────────────────────────────────────────────────
_WS_PREPARED_DIR=""

workspace_prepare_directory() {
    local branch="$1"
    local worktree_mode="$2"
    local project_root="$3"
    _WS_PREPARED_DIR=""

    if [[ "$worktree_mode" == "true" ]]; then
        local workdir
        workdir=$(worktree_get_path "$branch")

        if worktree_exists "$branch"; then
            log_success "$(_i18n INFO_WT_EXISTS) $workdir"

            if ! worktree_validate "$branch"; then
                log_error "$(_i18n ERR_WT_CORRUPTED)"
                return 1
            fi
        else
            if ! worktree_create "$branch"; then
                log_error "$(_i18n ERR_WT_CREATE_FAILED)"
                return 1
            fi
            _WS_TX_WORKTREE_CREATED=true
            worktree_sync_files "$workdir"
        fi
        _WS_PREPARED_DIR="$workdir"
    else
        log_success "$(_i18n INFO_WORKING_DIR) $project_root"
        _WS_PREPARED_DIR="$project_root"
    fi
}

# ──────────────────────────────────────────────────
# Step 2: Ensure session exists
# Args: $1=branch, $2=workdir, $3=force_recreate, $4=strict_template_rebuild
# Returns: 0=success, 1=failure
# ──────────────────────────────────────────────────
workspace_ensure_session() {
    local branch="$1"
    local workdir="$2"
    local force_recreate="${3:-false}"
    local strict_template_rebuild="${4:-false}"

    log_info "$(_i18n STEP_2_4) $(_i18n STEP_2_PREP_SESSION)"

    local session_name
    session_name=$(session_get_name "$branch")

    local _ws_runtime_server=""
    if declare -f tmux_runtime_server_name >/dev/null 2>&1; then
        _ws_runtime_server=$(tmux_runtime_server_name 2>/dev/null || echo "unknown")
    fi

    if [[ "$force_recreate" == true ]]; then
        WORKSPACE_OPEN_STRATEGY="config"
        log_debug "workspace open: strategy=config(force) session=${session_name} server=${_ws_runtime_server}"
        _workspace_prepare_force_recreate "$branch" "$session_name" || return 1
        if session_exists "$session_name"; then
            log_warn "$(_i18n MSG_FORCE_DESTROY_SESSION)"
            session_kill "$branch" "$strict_template_rebuild"
        fi
        if ! snapshot_purge_all 2>/dev/null; then
            log_warn "$(_i18n WARN_FORCE_SNAPSHOT_PURGE_FAILED)"
        fi
        # Force mode must rebuild from config directly and skip snapshot restore.
        if ! session_create "$branch" "$workdir"; then
            log_error "$(_i18n ERR_SESSION_CREATE_FAILED)"
            return 1
        fi
        _WS_TX_SESSION_CREATED=true
        log_success "$(_i18n MSG_SESSION_RECREATED) $session_name"
        return 0
    fi

    if session_exists "$session_name"; then
        log_success "$(_i18n MSG_SESSION_EXISTS) $session_name"
        WORKSPACE_USED_EXISTING_SESSION=true
        WORKSPACE_OPEN_STRATEGY="session"
        log_debug "workspace open: strategy=session(reuse) session=${session_name} server=${_ws_runtime_server}"
    else
        # Session does not exist: try snapshot restore first (if persistence enabled)
        if persistence_try_restore_session "$branch" 2>/dev/null; then
            WORKSPACE_RESTORED_FROM_SNAPSHOT=true
            _WS_TX_SESSION_CREATED=true
            WORKSPACE_OPEN_STRATEGY="snapshot"
            log_debug "workspace open: strategy=snapshot session=${session_name} server=${_ws_runtime_server}"
            log_success "$(_i18n MSG_SESSION_RECREATED) $session_name"
        else
            if ! session_create "$branch" "$workdir"; then
                log_error "$(_i18n ERR_SESSION_CREATE_FAILED)"
                return 1
            fi
            _WS_TX_SESSION_CREATED=true
            WORKSPACE_OPEN_STRATEGY="config"
            log_debug "workspace open: strategy=config(new) session=${session_name} server=${_ws_runtime_server}"
        fi
    fi

    return 0
}

# Result variable: after workspace_open succeeds, callers read this to get the working directory
WORKSPACE_RESULT_DIR=""

# Whether session was restored from a snapshot (callers check this to skip layout creation)
WORKSPACE_RESTORED_FROM_SNAPSHOT=false

# Whether open flow is reusing an already-running session.
WORKSPACE_USED_EXISTING_SESSION=false

# Unified strategy selected during workspace open:
# session | snapshot | config
WORKSPACE_OPEN_STRATEGY="config"

# ──────────────────────────────────────────────────
# Full workspace creation orchestration (transactional)
# Args: $1=branch, $2=worktree_mode, $3=project_root,
#        $4=force_recreate, $5=force_layout, $6=strict_template_rebuild
# Result: returns working directory path via WORKSPACE_RESULT_DIR
# Returns: 0=success, 1=failure
# ──────────────────────────────────────────────────
workspace_open() {
    local branch="$1"
    local worktree_mode="$2"
    local project_root="$3"
    local force_recreate="${4:-false}"
    # shellcheck disable=SC2034
    local force_layout="${5:-false}"
    local strict_template_rebuild="${6:-false}"

    WORKSPACE_RESULT_DIR=""
    # shellcheck disable=SC2034
    WORKSPACE_RESTORED_FROM_SNAPSHOT=false
    # shellcheck disable=SC2034
    WORKSPACE_USED_EXISTING_SESSION=false
    # shellcheck disable=SC2034
    WORKSPACE_OPEN_STRATEGY="config"

    _WS_TX_WORKTREE_CREATED=false
    _WS_TX_SESSION_CREATED=false
    _WS_TX_BRANCH="$branch"
    _WS_TX_COMPLETED=false

    # Transaction rollback should run on shell exit/error, without intercepting
    # Ctrl+C in interactive reads or downstream commands.
    trap '_workspace_rollback' EXIT ERR

    # Step 1: Prepare working directory
    log_info "$(_i18n STEP_1_4) $(_i18n STEP_1_PREP_DIR)"
    if ! workspace_prepare_directory "$branch" "$worktree_mode" "$project_root"; then
        _workspace_rollback
        trap - EXIT ERR
        return 1
    fi
    local workdir="$_WS_PREPARED_DIR"
    if [[ -z "$workdir" ]]; then
        log_error "$(_i18n ERR_WORKSPACE_PREP_EMPTY_PATH)"
        _workspace_rollback
        trap - EXIT ERR
        return 1
    fi
    echo ""

    # Step 2: Ensure session exists
    local _ws_ensure_rc=0
    workspace_ensure_session "$branch" "$workdir" "$force_recreate" "$strict_template_rebuild" || _ws_ensure_rc=$?
    if [[ "$_ws_ensure_rc" != "0" ]]; then
        _workspace_rollback
        trap - EXIT ERR
        return "$_ws_ensure_rc"
    fi
    echo ""

    _WS_TX_COMPLETED=true
    trap - EXIT ERR

    # shellcheck disable=SC2034
    WORKSPACE_RESULT_DIR="$workdir"
    return 0
}

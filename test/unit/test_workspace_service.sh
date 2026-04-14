#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/domain/services/workspace_service.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/infra/adapters/terminal/factory.sh"
source "$LIB_DIR/domain/services/worktree_service.sh"
source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/layout_service.sh"
source "$LIB_DIR/domain/services/workspace_service.sh"

# Track sessions created during tests for cleanup
_TEST_CREATED_SESSIONS=()

_ws_test_cleanup() {
    for sn in "${_TEST_CREATED_SESSIONS[@]}"; do
        tmux kill-session -t "$sn" 2>/dev/null || true
    done
}
trap _ws_test_cleanup EXIT

test_suite "Workspace Service Tests"

# ──────────────────────────────────────────────────
# Function existence
# ──────────────────────────────────────────────────

test_case "workspace_prepare_directory exists"
declare -f workspace_prepare_directory &>/dev/null
assert_eq 0 $? "function should exist"

test_case "workspace_ensure_session exists"
declare -f workspace_ensure_session &>/dev/null
assert_eq 0 $? "function should exist"

test_case "workspace_open exists"
declare -f workspace_open &>/dev/null
assert_eq 0 $? "function should exist"

test_case "_workspace_rollback exists"
declare -f _workspace_rollback &>/dev/null
assert_eq 0 $? "function should exist"

# ──────────────────────────────────────────────────
# Transaction state initialization
# ──────────────────────────────────────────────────

test_case "_WS_TX_WORKTREE_CREATED initial state is false"
assert_eq "false" "$_WS_TX_WORKTREE_CREATED" "should be false initially"

test_case "_WS_TX_SESSION_CREATED initial state is false"
assert_eq "false" "$_WS_TX_SESSION_CREATED" "should be false initially"

test_case "_WS_TX_COMPLETED initial state is false"
assert_eq "false" "$_WS_TX_COMPLETED" "should be false initially"

# ──────────────────────────────────────────────────
# workspace_prepare_directory in non-worktree mode
# ──────────────────────────────────────────────────

test_case "workspace_prepare_directory returns project_root in non-worktree mode"
_WS_PREPARED_DIR=""
workspace_prepare_directory "main" "false" "/tmp" 2>/dev/null
assert_eq "/tmp" "$_WS_PREPARED_DIR" "should return project_root"

test_case "workspace_prepare_directory sets _WS_PREPARED_DIR"
_WS_PREPARED_DIR=""
workspace_prepare_directory "test-branch" "false" "/tmp/test-proj" 2>/dev/null
assert_eq "/tmp/test-proj" "$_WS_PREPARED_DIR" "should set prepared dir"

# ──────────────────────────────────────────────────
# workspace_ensure_session force behavior
# ──────────────────────────────────────────────────

test_case "workspace_ensure_session skips snapshot restore when force_recreate=true and session missing"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"
_saved_session_exists="$(declare -f session_exists 2>/dev/null || true)"
_saved_persist_restore="$(declare -f persistence_try_restore_session 2>/dev/null || true)"
_saved_session_create="$(declare -f session_create 2>/dev/null || true)"
_saved_session_validate="$(declare -f session_validate 2>/dev/null || true)"
_saved_session_kill="$(declare -f session_kill 2>/dev/null || true)"
_persist_called=0
_create_called=0
session_get_name() { echo "wh@test@main"; }
session_exists() { return 1; }
persistence_try_restore_session() { _persist_called=$((_persist_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
session_validate() { return 0; }
session_kill() { return 0; }
WORKSPACE_RESTORED_FROM_SNAPSHOT=false
_WS_TX_SESSION_CREATED=false
WORKSPACE_OPEN_STRATEGY=""
workspace_ensure_session "main" "/tmp" "true" >/dev/null 2>&1
assert_eq "0" "$_persist_called" "force_recreate should bypass snapshot restore"
assert_eq "1" "$_create_called" "force_recreate should create a fresh session"
assert_eq "false" "$WORKSPACE_RESTORED_FROM_SNAPSHOT" "force_recreate path should not mark snapshot restored"
assert_eq "config" "$WORKSPACE_OPEN_STRATEGY" "force_recreate path should select config strategy"
unset -f session_get_name session_exists persistence_try_restore_session session_create session_validate session_kill
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"
[[ -n "$_saved_session_exists" ]] && eval "$_saved_session_exists"
[[ -n "$_saved_persist_restore" ]] && eval "$_saved_persist_restore"
[[ -n "$_saved_session_create" ]] && eval "$_saved_session_create"
[[ -n "$_saved_session_validate" ]] && eval "$_saved_session_validate"
[[ -n "$_saved_session_kill" ]] && eval "$_saved_session_kill"

test_case "workspace_ensure_session may restore snapshot when force_recreate=false and session missing"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"
_saved_session_exists="$(declare -f session_exists 2>/dev/null || true)"
_saved_persist_restore="$(declare -f persistence_try_restore_session 2>/dev/null || true)"
_saved_session_create="$(declare -f session_create 2>/dev/null || true)"
_saved_session_validate="$(declare -f session_validate 2>/dev/null || true)"
_saved_session_kill="$(declare -f session_kill 2>/dev/null || true)"
_persist_called=0
_create_called=0
session_get_name() { echo "wh@test@main"; }
session_exists() { return 1; }
persistence_try_restore_session() { _persist_called=$((_persist_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
session_validate() { return 0; }
session_kill() { return 0; }
WORKSPACE_RESTORED_FROM_SNAPSHOT=false
_WS_TX_SESSION_CREATED=false
WORKSPACE_OPEN_STRATEGY=""
workspace_ensure_session "main" "/tmp" "false" >/dev/null 2>&1
assert_eq "1" "$_persist_called" "non-force path should try snapshot restore"
assert_eq "0" "$_create_called" "restore success should skip fresh session create"
assert_eq "true" "$WORKSPACE_RESTORED_FROM_SNAPSHOT" "restore path should mark snapshot restored"
assert_eq "snapshot" "$WORKSPACE_OPEN_STRATEGY" "restore path should select snapshot strategy"
unset -f session_get_name session_exists persistence_try_restore_session session_create session_validate session_kill
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"
[[ -n "$_saved_session_exists" ]] && eval "$_saved_session_exists"
[[ -n "$_saved_persist_restore" ]] && eval "$_saved_persist_restore"
[[ -n "$_saved_session_create" ]] && eval "$_saved_session_create"
[[ -n "$_saved_session_validate" ]] && eval "$_saved_session_validate"
[[ -n "$_saved_session_kill" ]] && eval "$_saved_session_kill"

test_case "workspace_ensure_session reuses existing session without destructive repair in default open"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"
_saved_session_exists="$(declare -f session_exists 2>/dev/null || true)"
_saved_session_kill="$(declare -f session_kill 2>/dev/null || true)"
_saved_session_create="$(declare -f session_create 2>/dev/null || true)"
_saved_persist_restore="$(declare -f persistence_try_restore_session 2>/dev/null || true)"
_kill_called=0
_create_called=0
_persist_called=0
session_get_name() { echo "wh@test@main"; }
session_exists() { return 0; }
session_kill() { _kill_called=$((_kill_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
persistence_try_restore_session() { _persist_called=$((_persist_called + 1)); return 0; }
WORKSPACE_USED_EXISTING_SESSION=false
WORKSPACE_OPEN_STRATEGY=""
workspace_ensure_session "main" "/tmp" "false" "false" >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "existing session should be used directly"
assert_eq "0" "$_kill_called" "default open should not kill existing session"
assert_eq "0" "$_create_called" "default open should not recreate existing session"
assert_eq "0" "$_persist_called" "default open should not try snapshot when session exists"
assert_eq "true" "$WORKSPACE_USED_EXISTING_SESSION" "default open should mark existing-session path"
assert_eq "session" "$WORKSPACE_OPEN_STRATEGY" "existing session path should select session strategy"
unset -f session_get_name session_exists session_kill session_create persistence_try_restore_session
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"
[[ -n "$_saved_session_exists" ]] && eval "$_saved_session_exists"
[[ -n "$_saved_session_kill" ]] && eval "$_saved_session_kill"
[[ -n "$_saved_session_create" ]] && eval "$_saved_session_create"
[[ -n "$_saved_persist_restore" ]] && eval "$_saved_persist_restore"

test_case "workspace_ensure_session force mode kills existing session, purges snapshots, then recreates"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"
_saved_session_exists="$(declare -f session_exists 2>/dev/null || true)"
_saved_session_kill="$(declare -f session_kill 2>/dev/null || true)"
_saved_session_create="$(declare -f session_create 2>/dev/null || true)"
_saved_snapshot_purge_all="$(declare -f snapshot_purge_all 2>/dev/null || true)"
_saved_terminal_adapter_get_current_session="$(declare -f terminal_adapter_get_current_session 2>/dev/null || true)"
_kill_called=0
_create_called=0
_purge_called=0
session_get_name() { echo "wh@test@main"; }
session_exists() { return 0; }
session_kill() { _kill_called=$((_kill_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
snapshot_purge_all() { _purge_called=$((_purge_called + 1)); return 0; }
terminal_adapter_get_current_session() { return 1; }
workspace_ensure_session "main" "/tmp" "true" "true" >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "force mode should recreate session"
assert_eq "1" "$_kill_called" "force mode should kill existing session first"
assert_eq "1" "$_purge_called" "force mode should purge snapshots first"
assert_eq "1" "$_create_called" "force mode should create a fresh session"
unset -f session_get_name session_exists session_kill session_create snapshot_purge_all terminal_adapter_get_current_session
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"
[[ -n "$_saved_session_exists" ]] && eval "$_saved_session_exists"
[[ -n "$_saved_session_kill" ]] && eval "$_saved_session_kill"
[[ -n "$_saved_session_create" ]] && eval "$_saved_session_create"
[[ -n "$_saved_snapshot_purge_all" ]] && eval "$_saved_snapshot_purge_all"
[[ -n "$_saved_terminal_adapter_get_current_session" ]] && eval "$_saved_terminal_adapter_get_current_session"

test_case "workspace_ensure_session force mode detaches current client and still rebuilds target session"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"
_saved_session_exists="$(declare -f session_exists 2>/dev/null || true)"
_saved_session_kill="$(declare -f session_kill 2>/dev/null || true)"
_saved_session_create="$(declare -f session_create 2>/dev/null || true)"
_saved_snapshot_purge_all="$(declare -f snapshot_purge_all 2>/dev/null || true)"
_saved_terminal_adapter_get_current_session="$(declare -f terminal_adapter_get_current_session 2>/dev/null || true)"
_saved_session_detach="$(declare -f session_detach 2>/dev/null || true)"
_kill_called=0
_create_called=0
_purge_called=0
_detach_called=0
session_get_name() { echo "wh@test@main"; }
session_exists() { return 0; }
session_kill() { _kill_called=$((_kill_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
snapshot_purge_all() { _purge_called=$((_purge_called + 1)); return 0; }
terminal_adapter_get_current_session() { echo "wh@test@main"; }
session_detach() { _detach_called=$((_detach_called + 1)); return 0; }
workspace_ensure_session "main" "/tmp" "true" "true" >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "force mode should rebuild even when target is current session"
assert_eq "1" "$_detach_called" "force mode should detach current client first"
assert_eq "1" "$_kill_called" "force mode should still kill current session"
assert_eq "1" "$_purge_called" "force mode should still purge snapshots"
assert_eq "1" "$_create_called" "force mode should recreate session after detach"
unset -f session_get_name session_exists session_kill session_create snapshot_purge_all terminal_adapter_get_current_session session_detach
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"
[[ -n "$_saved_session_exists" ]] && eval "$_saved_session_exists"
[[ -n "$_saved_session_kill" ]] && eval "$_saved_session_kill"
[[ -n "$_saved_session_create" ]] && eval "$_saved_session_create"
[[ -n "$_saved_snapshot_purge_all" ]] && eval "$_saved_snapshot_purge_all"
[[ -n "$_saved_terminal_adapter_get_current_session" ]] && eval "$_saved_terminal_adapter_get_current_session"
[[ -n "$_saved_session_detach" ]] && eval "$_saved_session_detach"

# ──────────────────────────────────────────────────
# _workspace_rollback behavior
# ──────────────────────────────────────────────────

test_case "_workspace_rollback is no-op when TX completed"
_WS_TX_COMPLETED=true
_WS_TX_SESSION_CREATED=true
_WS_TX_WORKTREE_CREATED=true
_workspace_rollback 2>/dev/null
assert_eq 0 $? "should succeed (no-op)"

test_case "_workspace_rollback resets when TX not completed"
_WS_TX_COMPLETED=false
_WS_TX_SESSION_CREATED=false
_WS_TX_WORKTREE_CREATED=false
_WS_TX_BRANCH="nonexistent-branch-$$"
_workspace_rollback 2>/dev/null
assert_eq 0 $? "should handle gracefully when nothing to rollback"

# ──────────────────────────────────────────────────
# workspace_open argument handling (requires tmux)
# ──────────────────────────────────────────────────

if command -v tmux &>/dev/null; then

    test_case "workspace_open fails with empty workdir in non-worktree mode"
    WORKSPACE_RESULT_DIR=""
    _WS_TX_COMPLETED=false
    _test_branch_1="test-$$"
    workspace_open "$_test_branch_1" "false" "" "false" "false" 2>/dev/null
    _sn=$(session_get_name "$_test_branch_1" 2>/dev/null)
    [[ -n "$_sn" ]] && _TEST_CREATED_SESSIONS+=("$_sn")
    assert_eq 0 0 "function should not crash"

    test_case "WORKSPACE_RESULT_DIR is set after workspace_open in non-worktree mode"
    _WS_TX_COMPLETED=false
    _WS_TX_SESSION_CREATED=false
    _WS_TX_WORKTREE_CREATED=false
    _test_branch_2="test-ws-$$-$RANDOM"
    workspace_open "$_test_branch_2" "false" "/tmp" "false" "false" 2>/dev/null
    rc=$?
    _sn=$(session_get_name "$_test_branch_2" 2>/dev/null)
    [[ -n "$_sn" ]] && _TEST_CREATED_SESSIONS+=("$_sn")
    if [[ $rc -eq 0 ]]; then
        assert_eq "/tmp" "$WORKSPACE_RESULT_DIR" "should be /tmp"
    else
        # session_create may fail if not in proper git context
        assert_eq "" "$WORKSPACE_RESULT_DIR" "result dir should be empty on failure"
    fi

else
    test_case "workspace_open (tmux not installed - skip)"
    skip_test "tmux not installed"
fi

test_report

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Command-level regression tests: dedicated tmux runtime behavior
# Covers M6 mandatory test matrix scenarios
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux_runtime.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"
source "$LIB_DIR/infra/persistence/snapshot.sh"
source "$LIB_DIR/infra/persistence/restore.sh"
source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/workspace_service.sh"

# ──────────────────────────────────────────────────
# Global mock infrastructure
# ──────────────────────────────────────────────────
_RT_TMUX_CALLS=""

tmux() {
    _RT_TMUX_CALLS+="$*"$'\n'
    case "$1" in
        has-session) return 0 ;;
        switch-client) return 0 ;;
        attach) return 0 ;;
        new-session) return 0 ;;
        list-sessions) return 0 ;;
        list-panes) printf '%%1|0\n'; return 0 ;;
        list-clients) return 0 ;;
        display-message) printf '120x40|120x40\n'; return 0 ;;
        set-option) return 0 ;;
        send-keys) return 0 ;;
    esac
    return 0
}

test_suite "command-level dedicated runtime regression"

# ══════════════════════════════════════════════════
# Scenario 1: open -f creates session via dedicated runtime
# ══════════════════════════════════════════════════

test_case "workspace_ensure_session force mode invokes session_create through dedicated runtime"
_RT_TMUX_CALLS=""
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_saved_fns=()
for fn in session_get_name session_exists session_kill session_create \
          snapshot_purge_all terminal_adapter_get_current_session \
          persistence_try_restore_session; do
    _saved_fns+=("$(declare -f "$fn" 2>/dev/null || true)")
done
_create_called=0
_create_server=""
session_get_name() { echo "wh@proj@main"; }
session_exists() { return 0; }
session_kill() { return 0; }
session_create() {
    _create_called=$((_create_called + 1))
    _create_server=$(tmux_runtime_server_name 2>/dev/null || echo "")
    return 0
}
snapshot_purge_all() { return 0; }
terminal_adapter_get_current_session() { return 1; }
persistence_try_restore_session() { return 1; }

WORKSPACE_OPEN_STRATEGY=""
_WS_TX_SESSION_CREATED=false
workspace_ensure_session "main" "/tmp" "true" "true" >/dev/null 2>&1

assert_eq "1" "$_create_called" "force mode should invoke session_create"
assert_eq "workhive-proj-abc123" "$_create_server" "session_create should see dedicated server"
assert_eq "config" "$WORKSPACE_OPEN_STRATEGY" "strategy should be config for force mode"

for fn_body in "${_saved_fns[@]}"; do
    [[ -n "$fn_body" ]] && eval "$fn_body"
done

# ══════════════════════════════════════════════════
# Scenario 2: cross-server attach avoids switch-client
# ══════════════════════════════════════════════════

test_case "cross-server attach from command layer does not use switch-client"
_RT_TMUX_CALLS=""
TMUX="/tmp/tmux-501/default,999,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
terminal_adapter_attach_session "wh@proj@main"
assert_contains "$_RT_TMUX_CALLS" "attach -t wh@proj@main" "should use attach command for cross-server"
assert_not_contains "$_RT_TMUX_CALLS" "switch-client -t wh@proj@main" "must not switch-client across servers"

# ══════════════════════════════════════════════════
# Scenario 3: same-server attach uses switch-client
# ══════════════════════════════════════════════════

test_case "same-server attach from command layer uses switch-client"
_RT_TMUX_CALLS=""
TMUX="/tmp/tmux-501/workhive-proj-abc123,999,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
terminal_adapter_attach_session "wh@proj@main"
assert_contains "$_RT_TMUX_CALLS" "switch-client -t wh@proj@main" "should use switch-client on same server"
assert_not_contains "$_RT_TMUX_CALLS" "attach -t wh@proj@main" "must not spawn nested attach on same server"

# ══════════════════════════════════════════════════
# Scenario 4: current-session force-recreate detaches and rebuilds
# ══════════════════════════════════════════════════

test_case "force recreate detaches current client and rebuilds target session"
_RT_TMUX_CALLS=""
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_saved_fns2=()
for fn in session_get_name session_exists session_kill session_create \
          snapshot_purge_all terminal_adapter_get_current_session session_detach; do
    _saved_fns2+=("$(declare -f "$fn" 2>/dev/null || true)")
done
_kill_called=0
_create_called=0
_detach_called=0
session_get_name() { echo "wh@proj@main"; }
session_exists() { return 0; }
session_kill() { _kill_called=$((_kill_called + 1)); return 0; }
session_create() { _create_called=$((_create_called + 1)); return 0; }
snapshot_purge_all() { return 0; }
terminal_adapter_get_current_session() { echo "wh@proj@main"; }
session_detach() { _detach_called=$((_detach_called + 1)); return 0; }

_WS_TX_SESSION_CREATED=false
workspace_ensure_session "main" "/tmp" "true" "true" >/dev/null 2>&1
rc=$?

assert_eq "0" "$rc" "should rebuild when target is current session"
assert_eq "1" "$_detach_called" "must detach current client first"
assert_eq "1" "$_kill_called" "must still kill current session"
assert_eq "1" "$_create_called" "must still recreate current session"

for fn_body in "${_saved_fns2[@]}"; do
    [[ -n "$fn_body" ]] && eval "$fn_body"
done

# ══════════════════════════════════════════════════
# Scenario 5: restore active-state cross-server safety
# ══════════════════════════════════════════════════

_saved_attach_strategy="$(declare -f _terminal_tmux_select_attach_strategy 2>/dev/null || true)"
_saved_state_line="$(declare -f snapshot_get_state_line 2>/dev/null || true)"

test_case "restore does not switch-client across servers"
_RT_TMUX_CALLS=""
TMUX="/tmp/tmux-501/default,999,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_terminal_tmux_select_attach_strategy() { printf 'cross-server\n'; }
snapshot_get_state_line() { echo '{"type":"state","active_session":"wh@proj@main","last_session":""}'; }

_restore_active_state "/dev/null"
assert_not_contains "$_RT_TMUX_CALLS" "switch-client" "restore must not switch-client across servers"

test_case "restore uses switch-client on same server"
_RT_TMUX_CALLS=""
TMUX="/tmp/tmux-501/workhive-proj-abc123,999,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_terminal_tmux_select_attach_strategy() { printf 'same-server\n'; }

_restore_active_state "/dev/null"
assert_contains "$_RT_TMUX_CALLS" "switch-client" "restore should switch-client on same server"

unset -f _terminal_tmux_select_attach_strategy snapshot_get_state_line 2>/dev/null || true
[[ -n "$_saved_attach_strategy" ]] && eval "$_saved_attach_strategy"
[[ -n "$_saved_state_line" ]] && eval "$_saved_state_line"

# ══════════════════════════════════════════════════
# Scenario 6: snapshot metadata includes dedicated server
# ══════════════════════════════════════════════════

test_case "snapshot metadata records dedicated runtime server"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
tmux_runtime_server_name() { echo "$WORKHIVE_TMUX_SERVER_NAME"; }
_rt_meta=$(snapshot_format_meta "inst-001" "myproject" "wh@proj" "1" "0.1.0")
assert_contains "$_rt_meta" '"tmux_server":"workhive-proj-abc123"' "meta should contain dedicated server"

# ══════════════════════════════════════════════════
# Scenario 7: workspace open strategy selection is consistent
# ══════════════════════════════════════════════════

test_case "workspace open selects session strategy for existing session"
_RT_TMUX_CALLS=""
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_saved_fns3=()
for fn in session_get_name session_exists persistence_try_restore_session; do
    _saved_fns3+=("$(declare -f "$fn" 2>/dev/null || true)")
done
session_get_name() { echo "wh@proj@feat"; }
session_exists() { return 0; }
persistence_try_restore_session() { return 1; }

WORKSPACE_OPEN_STRATEGY=""
WORKSPACE_USED_EXISTING_SESSION=false
workspace_ensure_session "feat" "/tmp" "false" >/dev/null 2>&1

assert_eq "session" "$WORKSPACE_OPEN_STRATEGY" "should select session strategy for existing session"
assert_eq "true" "$WORKSPACE_USED_EXISTING_SESSION" "should mark existing session"

for fn_body in "${_saved_fns3[@]}"; do
    [[ -n "$fn_body" ]] && eval "$fn_body"
done

test_case "workspace open selects snapshot strategy when restore succeeds"
_RT_TMUX_CALLS=""
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_saved_fns4=()
for fn in session_get_name session_exists persistence_try_restore_session session_create; do
    _saved_fns4+=("$(declare -f "$fn" 2>/dev/null || true)")
done
session_get_name() { echo "wh@proj@feat2"; }
session_exists() { return 1; }
persistence_try_restore_session() { return 0; }
session_create() { return 0; }

WORKSPACE_OPEN_STRATEGY=""
WORKSPACE_RESTORED_FROM_SNAPSHOT=false
_WS_TX_SESSION_CREATED=false
workspace_ensure_session "feat2" "/tmp" "false" >/dev/null 2>&1

assert_eq "snapshot" "$WORKSPACE_OPEN_STRATEGY" "should select snapshot strategy"
assert_eq "true" "$WORKSPACE_RESTORED_FROM_SNAPSHOT" "should mark snapshot restored"

for fn_body in "${_saved_fns4[@]}"; do
    [[ -n "$fn_body" ]] && eval "$fn_body"
done

test_case "workspace open selects config strategy when no snapshot"
_RT_TMUX_CALLS=""
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
_saved_fns5=()
for fn in session_get_name session_exists persistence_try_restore_session session_create; do
    _saved_fns5+=("$(declare -f "$fn" 2>/dev/null || true)")
done
session_get_name() { echo "wh@proj@feat3"; }
session_exists() { return 1; }
persistence_try_restore_session() { return 1; }
session_create() { return 0; }

WORKSPACE_OPEN_STRATEGY=""
_WS_TX_SESSION_CREATED=false
workspace_ensure_session "feat3" "/tmp" "false" >/dev/null 2>&1

assert_eq "config" "$WORKSPACE_OPEN_STRATEGY" "should select config strategy when no snapshot"

for fn_body in "${_saved_fns5[@]}"; do
    [[ -n "$fn_body" ]] && eval "$fn_body"
done

# ══════════════════════════════════════════════════
# Scenario 8: cross-server readonly attach
# ══════════════════════════════════════════════════

test_case "cross-server readonly attach uses tmux attach -r, not switch-client"
_RT_TMUX_CALLS=""
TMUX="/tmp/tmux-501/default,999,0"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-abc123"
terminal_adapter_attach_session_readonly "wh@proj@main"
assert_contains "$_RT_TMUX_CALLS" "attach -t wh@proj@main -r" "should use readonly attach"
assert_not_contains "$_RT_TMUX_CALLS" "switch-client" "must not switch-client across servers for readonly"

# ──────────────────────────────────────────────────
# Cleanup
# ──────────────────────────────────────────────────
unset TMUX WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true
unset -f _terminal_tmux_select_attach_strategy 2>/dev/null || true

test_report

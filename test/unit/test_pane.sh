#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: pane command (AI automation core)
# Tests split targeting, send precision, list output,
# and multi-pane orchestration scenarios
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
export ROOT_DIR
BIN_DIR="$ROOT_DIR/bin"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"

_PANE_TEST_SESSION=""
_pane_cleanup_tmux_session() {
    if ! command -v tmux &>/dev/null; then
        return 0
    fi
    test_tmux_cleanup_registered_sessions
    if [[ -n "${_PANE_TEST_SESSION:-}" ]]; then
        test_tmux_kill_session "$_PANE_TEST_SESSION"
    fi
}
trap _pane_cleanup_tmux_session EXIT

test_suite "pane command — startup runner semantics"

test_case "tmux startup runner exits on interrupt instead of falling back to shell"
runner_script=$(terminal_adapter_build_startup_runner_script)
assert_contains "$runner_script" "unset CLAUDE_CODE_NO_FLICKER;" "runner should clear managed startup env inherited from parent shell"
assert_contains "$runner_script" "unset WORKHIVE_SAVE_LOCK_QUIET;" "runner should clear managed persistence env inherited from parent shell"
assert_contains "$runner_script" "_startup_interrupted=false" "runner should track interrupts explicitly"
assert_contains "$runner_script" "trap '_startup_interrupted=true' INT TERM" "runner should capture interrupt state"
assert_contains "$runner_script" 'if [[ "$_startup_interrupted" == true ]]; then exit 130; fi' "runner should stop on Ctrl+C instead of reopening a shell"
assert_contains "$runner_script" 'if (( _startup_rc >= 128 )); then exit "$_startup_rc"; fi' "runner should preserve signaled exits"
assert_contains "$runner_script" 'exec "$2" -l' "runner should still reopen shell after normal command exit"

# ──────────────────────────────────────────────────
# Part 1: CLI argument parsing (no tmux required)
# ──────────────────────────────────────────────────

test_suite "pane command — CLI argument parsing"

test_case "pane --help shows all subcommands"
output=$("$BIN_DIR/workhive" pane --help 2>&1)
assert_contains "$output" "open" "should list open"
assert_contains "$output" "close" "should list close"
assert_contains "$output" "split" "should list split"
assert_contains "$output" "send" "should list send"
assert_contains "$output" "list" "should list list"
assert_contains "$output" "current" "should list current"
assert_not_contains "$output" "layout" "should not list removed layout subcommand"

test_case "pane split --help documents --cmd and --size"
output=$("$BIN_DIR/workhive" pane split --help 2>&1)
assert_contains "$output" "--cmd" "should document --cmd option"
assert_contains "$output" "--size" "should document --size option"

test_case "pane send --help documents mode/commit/verbose/role/diagnose"
output=$("$BIN_DIR/workhive" pane send --help 2>&1)
assert_contains "$output" "--all" "should document --all"
assert_contains "$output" "--mode" "should document --mode"
assert_contains "$output" "--commit" "should document --commit"
assert_contains "$output" "--verbose" "should document --verbose"
assert_contains "$output" "--role" "should document --role"
assert_contains "$output" "--diagnose" "should document --diagnose"

test_case "pane open --help documents --cmd and --force"
output=$("$BIN_DIR/workhive" pane open --help 2>&1)
assert_contains "$output" "--cmd" "should document --cmd"
assert_contains "$output" "--force" "should document --force"

test_case "pane i18n: help works in Chinese"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane --help 2>&1)
assert_contains "$output" "pane" "should show pane text in Chinese mode"

test_case "pane i18n foundation keys exist for role and agent columns"
assert_not_empty "$(_i18n PANE_ROLE_LABEL)" "role label should be defined"
assert_not_empty "$(_i18n PANE_LIST_HDR_AGENT)" "agent column label should be defined"
assert_not_empty "$(_i18n PANE_LIST_HDR_AGENT_STATUS)" "agent status column label should be defined"
assert_not_empty "$(_i18n AGENT_STATUS_NONE)" "agent empty status should be defined"
assert_not_empty "$(_i18n AGENT_STATUS_IDLE)" "agent idle status should be defined"
assert_not_empty "$(_i18n AGENT_STATUS_UNKNOWN)" "agent unknown status should be defined"
assert_not_empty "$(_i18n AGENT_NOTIFY_FINISHED)" "agent finished notification template should be defined"
assert_not_empty "$(_i18n AGENT_NOTIFY_WAITING)" "agent waiting notification template should be defined"

test_case "pane unknown subcommand shows error"
rc=0; output=$("$BIN_DIR/workhive" pane foobar 2>&1) || rc=$?
assert_eq 1 "$rc" "should fail with exit 1"

test_case "pane split unknown option shows error"
rc=0; output=$("$BIN_DIR/workhive" pane split --unknown 2>&1) || rc=$?
assert_eq 1 "$rc" "should fail with exit 1"

# ──────────────────────────────────────────────────
# Part 2: _pane_resolve_cmd_alias
# ──────────────────────────────────────────────────

test_suite "pane command — alias resolution"

source "$LIB_DIR/domain/services/worktree_service.sh" 2>/dev/null || true
source "$LIB_DIR/domain/services/session_service.sh" 2>/dev/null || true
source "$LIB_DIR/domain/services/layout_service.sh" 2>/dev/null || true
source "$LIB_DIR/utils/tui.sh" 2>/dev/null || true
source "$LIB_DIR/app/shared/worktree_collector.sh" 2>/dev/null || true
source "$LIB_DIR/app/shared/worktree_selector.sh" 2>/dev/null || true
source "$LIB_DIR/app/shared/command_builder.sh" 2>/dev/null || true
source "$LIB_DIR/infra/persistence/manager.sh" 2>/dev/null || true
source "$LIB_DIR/app/commands/pane_command.sh" 2>/dev/null || true

test_case "claude alias resolves to full command"
result=$(_pane_resolve_cmd_alias "claude")
assert_contains "$result" "dangerously-skip-permissions" "claude should include permissions flag"

test_case "codex alias resolves"
result=$(_pane_resolve_cmd_alias "codex")
assert_eq "codex" "$result" "codex should resolve to codex"

test_case "lazygit alias resolves"
result=$(_pane_resolve_cmd_alias "lazygit")
assert_eq "lazygit" "$result" "lazygit should resolve to lazygit"

test_case "shell alias returns empty string"
result=$(_pane_resolve_cmd_alias "shell")
assert_eq "" "$result" "shell alias should return empty"

test_case "unknown alias passes through"
result=$(_pane_resolve_cmd_alias "npm run dev")
assert_eq "npm run dev" "$result" "unknown alias should pass through"

test_suite "pane command — launch command validation"

test_case "preset command rejects shell metacharacters"
rc=0
_pane_validate_launch_command "codex; whoami" "preset" >/dev/null 2>&1 || rc=$?
assert_eq 1 "$rc" "preset command with shell metacharacters should be rejected"

test_case "preset command rejects non-allowlisted binary"
rc=0
_pane_validate_launch_command "ls -la" "preset" >/dev/null 2>&1 || rc=$?
assert_eq 1 "$rc" "preset command outside enterprise allowlist should be rejected"

test_case "arg command allows plain existing command without metacharacters"
rc=0
_pane_validate_launch_command "ls -la" "arg" >/dev/null 2>&1 || rc=$?
assert_eq 0 "$rc" "user arg command should allow plain command without metacharacters"

test_case "arg command allows shell metacharacters"
rc=0
_pane_validate_launch_command "printf 'a|b' | tr '|' ':'" "arg" >/dev/null 2>&1 || rc=$?
assert_eq 0 "$rc" "user arg command should allow shell metacharacters"

test_suite "pane command — split command flow"

test_case "pane split --cmd passes command to split adapter and saves layout"
_pane_split_percentage=""
_pane_split_cmd=""
_pane_split_saved=0
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_split_current=$(declare -f terminal_adapter_split_current_pane)
_pane_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_require_tmux() { return 0; }
terminal_adapter_split_current_pane() {
    _pane_split_percentage="$1"
    _pane_split_cmd="$2"
    return 0
}
_pane_trigger_save() {
    _pane_split_saved=$((_pane_split_saved + 1))
    return 0
}
sleep() { return 0; }

rc=0
_pane_cmd_split --cmd "claude --dangerously-skip-permissions" || rc=$?

unset -f _pane_require_tmux terminal_adapter_split_current_pane _pane_trigger_save sleep
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_split_current"
builtin eval "$_pane_orig_trigger_save"
assert_eq 0 "$rc" "split command should succeed"
assert_eq "50" "$_pane_split_percentage" "should use default split percentage"
assert_eq "claude --dangerously-skip-permissions" "$_pane_split_cmd" "should forward full command to split adapter"
assert_eq "1" "$_pane_split_saved" "should trigger layout save once"

test_suite "pane command — close targeting and list json"

test_case "pane close current pane uses current-pane kill path"
_pane_close_target=""
_pane_close_saved=0
_pane_close_verified=0
_pane_close_current_kill=0
_pane_close_verify_target=""
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_current_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_kill_current=$(declare -f terminal_adapter_kill_pane)
_pane_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_orig_verify_close=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "0"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
0|%10
1|%11
2|%12
EOF
}
terminal_adapter_count_panes() {
    if [[ "$1" == "wh@demo" ]]; then
        echo "3"
    else
        echo "2"
    fi
}
terminal_adapter_kill_pane() {
    _pane_close_current_kill=$((_pane_close_current_kill + 1))
    return 0
}
terminal_adapter_kill_pane_target() {
    _pane_close_target="$1|$2"
    return 0
}
_pane_trigger_save() {
    _pane_close_saved=$((_pane_close_saved + 1))
    return 0
}
_pane_verify_close_result() {
    _pane_close_verified=$((_pane_close_verified + 1))
    _pane_close_verify_target="$3"
    return 0
}

rc=0
_pane_cmd_close --force >/dev/null 2>&1 || rc=$?

unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format terminal_adapter_count_panes terminal_adapter_kill_pane terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_current_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_count_panes"
[[ -n "$_pane_orig_kill_current" ]] && builtin eval "$_pane_orig_kill_current" || unset -f terminal_adapter_kill_pane 2>/dev/null || true
[[ -n "$_pane_orig_kill_target" ]] && builtin eval "$_pane_orig_kill_target" || unset -f terminal_adapter_kill_pane_target 2>/dev/null || true
builtin eval "$_pane_orig_trigger_save"
[[ -n "$_pane_orig_verify_close" ]] && builtin eval "$_pane_orig_verify_close" || unset -f _pane_verify_close_result 2>/dev/null || true
assert_eq 0 "$rc" "current pane close should succeed"
assert_eq "1" "$_pane_close_current_kill" "should use current-pane kill path"
assert_eq "" "$_pane_close_target" "should not use targeted kill path"
assert_eq "1" "$_pane_close_saved" "should trigger save for current-pane close"
assert_eq "1" "$_pane_close_verified" "should verify pane count after close"
assert_eq "%10" "$_pane_close_verify_target" "should verify current-pane close by pane id"

test_case "pane close current pane fails if current pane id cannot be resolved"
_pane_close_current_kill=0
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_current_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_kill_current=$(declare -f terminal_adapter_kill_pane)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "0"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
1|%11
2|%12
EOF
}
terminal_adapter_count_panes() { echo "3"; }
terminal_adapter_kill_pane() {
    _pane_close_current_kill=$((_pane_close_current_kill + 1))
    return 0
}

rc=0
output=$(_pane_cmd_close --force 2>&1) || rc=$?

unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format terminal_adapter_count_panes terminal_adapter_kill_pane
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_current_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_count_panes"
[[ -n "$_pane_orig_kill_current" ]] && builtin eval "$_pane_orig_kill_current" || unset -f terminal_adapter_kill_pane 2>/dev/null || true
assert_eq 1 "$rc" "current close should fail when pane id cannot be resolved"
assert_eq "0" "$_pane_close_current_kill" "should not kill pane without stable pane id"
assert_true "echo \"$output\" | grep -Eq 'verification failed|校验失败'" "should report verification-related failure"

test_case "pane close <index> targets the specified pane and triggers save"
_pane_close_target=""
_pane_close_saved=0
_pane_close_verified=0
_pane_close_verify_target=""
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_orig_verify_close=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
0|%10
1|%11
2|%12
EOF
}
terminal_adapter_count_panes() {
    if [[ "$1" == "wh@demo" ]]; then
        echo "3"
    else
        echo "2"
    fi
}
terminal_adapter_kill_pane_target() {
    _pane_close_target="$1|$2"
    return 0
}
_pane_trigger_save() {
    _pane_close_saved=$((_pane_close_saved + 1))
    return 0
}
_pane_verify_close_result() {
    _pane_close_verified=$((_pane_close_verified + 1))
    _pane_close_verify_target="$3"
    return 0
}

rc=0
_pane_cmd_close --force 1 >/dev/null 2>&1 || rc=$?

unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_list_panes_format terminal_adapter_count_panes terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_count_panes"
[[ -n "$_pane_orig_kill_target" ]] && builtin eval "$_pane_orig_kill_target" || unset -f terminal_adapter_kill_pane_target 2>/dev/null || true
builtin eval "$_pane_orig_trigger_save"
[[ -n "$_pane_orig_verify_close" ]] && builtin eval "$_pane_orig_verify_close" || unset -f _pane_verify_close_result 2>/dev/null || true
assert_eq 0 "$rc" "targeted close by index should succeed"
assert_eq "wh@demo|%11" "$_pane_close_target" "should kill the requested pane by stable pane id"
assert_eq "1" "$_pane_close_saved" "should trigger save for targeted close"
assert_eq "1" "$_pane_close_verified" "should verify pane count after close"
assert_eq "%11" "$_pane_close_verify_target" "should verify targeted close by resolved pane id"

test_case "pane close --id targets the resolved pane in current session"
_pane_close_target=""
_pane_close_saved=0
_pane_close_verified=0
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_resolve=$(declare -f terminal_adapter_resolve_pane_target)
_pane_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_orig_verify_close=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() {
    if [[ "$1" == "wh@demo" ]]; then
        echo "3"
    else
        echo "2"
    fi
}
terminal_adapter_resolve_pane_target() {
    echo "wh@demo|2|1|%42"
    return 0
}
terminal_adapter_kill_pane_target() {
    _pane_close_target="$1|$2"
    return 0
}
_pane_trigger_save() {
    _pane_close_saved=$((_pane_close_saved + 1))
    return 0
}
_pane_verify_close_result() {
    _pane_close_verified=$((_pane_close_verified + 1))
    return 0
}

rc=0
_pane_cmd_close --force --id %42 >/dev/null 2>&1 || rc=$?

unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_resolve_pane_target terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
[[ -n "$_pane_orig_resolve" ]] && builtin eval "$_pane_orig_resolve" || unset -f terminal_adapter_resolve_pane_target 2>/dev/null || true
[[ -n "$_pane_orig_kill_target" ]] && builtin eval "$_pane_orig_kill_target" || unset -f terminal_adapter_kill_pane_target 2>/dev/null || true
builtin eval "$_pane_orig_trigger_save"
[[ -n "$_pane_orig_verify_close" ]] && builtin eval "$_pane_orig_verify_close" || unset -f _pane_verify_close_result 2>/dev/null || true
assert_eq 0 "$rc" "targeted close by pane id should succeed"
assert_eq "wh@demo|2.1" "$_pane_close_target" "should kill the resolved pane target with window context"
assert_eq "1" "$_pane_close_saved" "should trigger save for pane-id close"
assert_eq "1" "$_pane_close_verified" "should verify pane-id close result"

test_case "pane close --id rejects malformed pane id"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_require_tmux() { return 0; }
rc=0
output=$(_pane_cmd_close --force --id pane-1 2>&1) || rc=$?
unset -f _pane_require_tmux
builtin eval "$_pane_orig_require_tmux"
assert_eq 1 "$rc" "malformed pane id should fail"
assert_contains "$output" "pane id" "should mention pane id"

test_case "pane close --id rejects pane from another session"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_resolve=$(declare -f terminal_adapter_resolve_pane_target)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "3"; }
terminal_adapter_resolve_pane_target() {
    echo "other-session|1|0|%42"
    return 0
}
rc=0
output=$(_pane_cmd_close --force --id %42 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_resolve_pane_target
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
[[ -n "$_pane_orig_resolve" ]] && builtin eval "$_pane_orig_resolve" || unset -f terminal_adapter_resolve_pane_target 2>/dev/null || true
assert_eq 1 "$rc" "cross-session pane id should fail"
assert_true "echo \"$output\" | grep -Eq 'another session|另一个会话'" "should distinguish cross-session pane ids"

test_case "pane list --format json outputs approved contract"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_agents=$(declare -f pane_state_collect_session)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "1"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
0|%10|bash|/tmp/main|120x30|0
1|%11|claude|/tmp/feature|80x20|0
EOF
}
pane_state_collect_session() {
    cat <<'EOF'
0|%10|101||idle|probe|high|NO_AGENT
1|%11|202|claude|running|probe|medium|AI_PROCESS_ACTIVE
EOF
}
rc=0
output=$(_pane_cmd_list --format json 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format pane_state_collect_session
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_agents"
assert_eq 0 "$rc" "json list should succeed"
assert_contains "$output" '"session":"wh@demo"' "should include session name"
assert_contains "$output" '"current_pane_index":1' "should include current pane index"
assert_contains "$output" '"panes":[' "should include panes array"
assert_contains "$output" '"pane_index":0' "should use pane_index field"
assert_contains "$output" '"pane_id":"%10"' "should include pane id field"
assert_contains "$output" '"current_command":"bash"' "should include current command field"
assert_contains "$output" '"current_path":"/tmp/main"' "should include current path field"
assert_contains "$output" '"dir_name":"main"' "should include dir_name field"
assert_contains "$output" '"size":{"width":120,"height":30}' "should include structured size"
assert_contains "$output" '"is_current":true' "should mark current pane"
assert_contains "$output" '"is_dead":false' "should include dead state"
assert_contains "$output" '"agent_name":"claude"' "should include agent_name"
assert_contains "$output" '"agent_status":"running"' "should include normalized agent status"
assert_contains "$output" '"agent_state_source":"probe"' "should include state source"
assert_contains "$output" '"agent_state_confidence":"medium"' "should include state confidence"
assert_contains "$output" '"agent_state_reason":"AI_PROCESS_ACTIVE"' "should include state reason"
assert_not_contains "$output" '"index":' "should not use legacy index field"
assert_not_contains "$output" '"command":' "should not use legacy command field"
assert_not_contains "$output" '"path":' "should not use legacy path field"
assert_not_contains "$output" '"directory":' "should not use legacy directory field"
assert_not_contains "$output" '"status":' "should not use legacy status field"
assert_not_contains "$output" '"agent":' "should not use legacy agent field"

test_case "pane list --format json handles empty pane list"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_agents=$(declare -f pane_state_collect_session)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "0"; }
terminal_adapter_list_panes_format() { return 0; }
pane_state_collect_session() { return 0; }
rc=0
output=$(_pane_cmd_list --format json 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format pane_state_collect_session
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_agents"
assert_eq 0 "$rc" "empty json list should succeed"
assert_contains "$output" '"session":"wh@demo"' "should keep session in empty payload"
assert_contains "$output" '"current_pane_index":null' "should use null current pane index when no panes exist"
assert_contains "$output" '"panes":[]' "should return an empty panes array"

test_case "pane current --format json outputs current pane contract"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_agent_one=$(declare -f pane_state_collect_one)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "1"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
0|%10|bash|/tmp/main|120x30|0|101
1|%11|claude|/tmp/feature|80x20|0|202
EOF
}
pane_state_collect_one() {
    echo "1|%11|202|claude|running|probe|medium|AI_PROCESS_ACTIVE"
}
rc=0
output=$(_pane_cmd_current --format json 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format pane_state_collect_one
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_agent_one"
assert_eq 0 "$rc" "json current should succeed"
assert_contains "$output" '"session":"wh@demo"' "should include session name"
assert_contains "$output" '"pane":{' "should include pane object"
assert_contains "$output" '"pane_index":1' "should include current pane index"
assert_contains "$output" '"pane_id":"%11"' "should include pane id"
assert_contains "$output" '"current_command":"claude"' "should include current command"
assert_contains "$output" '"current_path":"/tmp/feature"' "should include current path"
assert_contains "$output" '"dir_name":"feature"' "should include directory basename"
assert_contains "$output" '"size":{"width":80,"height":20}' "should include structured size"
assert_contains "$output" '"is_current":true' "should always mark current pane"
assert_contains "$output" '"agent_name":"claude"' "should include agent name"
assert_contains "$output" '"agent_status":"running"' "should include normalized status"
assert_contains "$output" '"agent_state_source":"probe"' "should include state source"
assert_contains "$output" '"agent_state_confidence":"medium"' "should include state confidence"
assert_contains "$output" '"agent_state_reason":"AI_PROCESS_ACTIVE"' "should include state reason"

test_case "pane current fails when current pane index does not exist in pane list"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_get_pane=$(declare -f terminal_adapter_get_current_pane_index)
_pane_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_orig_agent_one=$(declare -f pane_state_collect_one)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_get_current_pane_index() { echo "9"; }
terminal_adapter_list_panes_format() {
    cat <<'EOF'
0|%10|bash|/tmp/main|120x30|0|101
1|%11|claude|/tmp/feature|80x20|0|202
EOF
}
pane_state_collect_one() { return 0; }
rc=0
output=$(_pane_cmd_current 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_get_current_pane_index terminal_adapter_list_panes_format pane_state_collect_one
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_get_pane"
builtin eval "$_pane_orig_list"
builtin eval "$_pane_orig_agent_one"
assert_eq 1 "$rc" "current pane missing from pane list should fail"
assert_contains "$output" "pane" "should mention pane in the error message"

# ──────────────────────────────────────────────────
# Part 3: pane layout removed
# ──────────────────────────────────────────────────

test_suite "pane command — removed subcommands"

test_case "pane layout subcommand is removed"
output=$("$BIN_DIR/workhive" pane layout 2>&1 || true)
assert_contains "$output" "Unknown command"
assert_contains "$output" "workhive pane <subcommand>"

test_suite "pane command — send enter behavior"

test_case "pane send sends Enter key by default"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
_pane_cmd_send 2 "hello" >/dev/null 2>&1 || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send should succeed"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 -l hello" "should send text literally first"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 Enter" "should send Enter key by default"

test_case "pane send --commit none does not send Enter"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
_pane_cmd_send 2 "hello" --commit none >/dev/null 2>&1 || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send --commit none should succeed"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 -l hello" "should still send text literally"
assert_not_contains "$_mock_calls" "send-keys -t wh@demo:0.2 Enter" "should not send Enter when --commit none is set"
assert_not_contains "$_mock_calls" "send-keys -t wh@demo:0.2 C-m" "should not send C-m when --commit none is set"

test_case "pane send rejects legacy --no-enter option"
rc=0
output=$(_pane_cmd_send --no-enter 2 "hello" 2>&1) || rc=$?
assert_eq 1 "$rc" "legacy --no-enter should be rejected"
assert_contains "$output" "Unknown option" "should report unknown option for --no-enter"

test_case "pane send --mode bracketed-paste --commit c-m uses tmux paste pipeline"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
_pane_cmd_send 2 "hello" --mode bracketed-paste --commit c-m >/dev/null 2>&1 || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send with explicit bracketed-paste+c-m should succeed"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "set-buffer -- hello" "should set tmux buffer before paste"
assert_contains "$_mock_calls" "paste-buffer -p -t wh@demo:0.2" "should use bracketed paste"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 C-m" "should commit with C-m"

test_case "pane send auto mode detects codex and writes cache for pane key"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
_CACHE_LOG="/tmp/wh_pane_send_cache_$$"
rm -f "$_CACHE_LOG"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    if [[ "$1" == "display-message" && "$2" == "-p" && "$3" == "-t" && "$4" == "wh@demo:0.2" ]]; then
        echo "codex"
        return 0
    fi
    return 0
}
rc=0
WORKHIVE_PANE_SEND_CACHE_FILE="$_CACHE_LOG" _pane_cmd_send 2 "hello codex" >/dev/null 2>&1 || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send auto for codex should succeed"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "paste-buffer -p -t wh@demo:0.2" "codex auto mode should use bracketed paste by default"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 C-m" "codex auto mode should use C-m by default"
assert_file_exists "$_CACHE_LOG" "auto mode should create pane send cache"
assert_contains "$(cat "$_CACHE_LOG")" "wh@demo:0.2|codex|bracketed-paste|c-m" "cache should store target-specific strategy"

test_case "pane send rejects invalid --mode value"
rc=0
output=$(_pane_cmd_send 2 "hello" --mode invalid-mode 2>&1) || rc=$?
assert_eq 1 "$rc" "invalid --mode should fail"
assert_contains "$output" "Invalid --mode" "should include invalid mode guidance"

test_case "pane send rejects invalid --commit value"
rc=0
output=$(_pane_cmd_send 2 "hello" --commit invalid-commit 2>&1) || rc=$?
assert_eq 1 "$rc" "invalid --commit should fail"
assert_contains "$output" "Invalid --commit" "should include invalid commit guidance"

test_case "pane send --verbose prints resolved strategy metadata"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    if [[ "$1" == "display-message" && "$2" == "-p" && "$3" == "-t" && "$4" == "wh@demo:0.2" ]]; then
        echo "codex"
        return 0
    fi
    return 0
}
rc=0
output=$(_pane_cmd_send 2 "hello" --verbose 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send --verbose should succeed"
assert_contains "$output" "strategy:" "verbose output should include strategy label"
assert_contains "$output" "client=codex" "verbose output should include detected client"
assert_contains "$output" "mode=bracketed-paste" "verbose output should include resolved mode"
assert_contains "$output" "commit=c-m" "verbose output should include resolved commit key"

test_case "pane send --diagnose is read-only and does not send keys"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_orig_list_panes=$(declare -f terminal_adapter_list_panes_format)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
terminal_adapter_list_panes_format() {
    echo "2|%12|zsh|/tmp/workhive|0|0|1|0"
}
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
output=$(_pane_cmd_send 2 --diagnose 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_list_panes_format tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
builtin eval "$_pane_orig_list_panes"
assert_eq 0 "$rc" "pane send --diagnose should succeed"
assert_contains "$output" "Diagnose" "should print diagnose heading"
assert_contains "$output" "Conclusion:" "should print conclusion classification"
_mock_calls=$(cat "$_MOCK_LOG")
assert_not_contains "$_mock_calls" "send-keys" "diagnose mode must not send keys"

test_case "pane send --all broadcasts to all panes"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "3"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
output=$(_pane_cmd_send --all "echo hi" 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "pane send --all should succeed"
assert_contains "$output" "broadcast" "should print broadcast success message"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.0 -l echo\ hi" "should send text to pane 0"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.0 Enter" "should send Enter to pane 0"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.1 -l echo\ hi" "should send text to pane 1"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.1 Enter" "should send Enter to pane 1"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 -l echo\ hi" "should send text to pane 2"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 Enter" "should send Enter to pane 2"

test_case "pane send --all rejects --role"
rc=0
output=$(_pane_cmd_send --all --role primary "pwd" 2>&1) || rc=$?
assert_eq 1 "$rc" "pane send --all with --role should fail"
assert_contains "$output" "--all" "should mention --all conflict"

test_case "pane send --all rejects --diagnose"
rc=0
output=$(_pane_cmd_send --all "pwd" --diagnose 2>&1) || rc=$?
assert_eq 1 "$rc" "pane send --all with --diagnose should fail"
assert_contains "$output" "diagnose" "should mention diagnose incompatibility"

test_case "pane send comma-separated pane indices sends to each target"
_MOCK_LOG="/tmp/wh_pane_send_mock_$$"
: > "$_MOCK_LOG"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
tmux() {
    local rendered=""
    local arg
    for arg in "$@"; do
        rendered+=$(printf '%q ' "$arg")
    done
    echo "CMD:${rendered}" >> "$_MOCK_LOG"
    return 0
}
rc=0
output=$(_pane_cmd_send 1,2,3 "echo ok" 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes tmux
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 0 "$rc" "comma-separated pane send should succeed"
assert_contains "$output" "panes 1,2,3" "should report multi-pane target list"
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.1 -l echo\ ok" "should send text to pane 1"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.1 Enter" "should send Enter to pane 1"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 -l echo\ ok" "should send text to pane 2"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.2 Enter" "should send Enter to pane 2"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.3 -l echo\ ok" "should send text to pane 3"
assert_contains "$_mock_calls" "send-keys -t wh@demo:0.3 Enter" "should send Enter to pane 3"

test_case "pane send comma-separated pane indices rejects invalid target"
_pane_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_orig_get_session=$(declare -f terminal_adapter_get_current_session)
_pane_orig_count_panes=$(declare -f terminal_adapter_count_panes)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh@demo"; }
terminal_adapter_count_panes() { echo "4"; }
rc=0
output=$(_pane_cmd_send 1,abc,3 "echo ok" 2>&1) || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes
builtin eval "$_pane_orig_require_tmux"
builtin eval "$_pane_orig_get_session"
builtin eval "$_pane_orig_count_panes"
assert_eq 1 "$rc" "invalid pane in comma-separated list should fail"
assert_contains "$output" "abc" "should include invalid target in error"

# ──────────────────────────────────────────────────
# Part 4: tmux adapter split targeting (mock tests)
# ──────────────────────────────────────────────────

test_suite "pane command — split targeting precision"

test_case "split_current_pane captures pane ID from split-window -P"
_MOCK_LOG="/tmp/wh_mock_$$"
: > "$_MOCK_LOG"
tmux() {
    echo "CMD:$*" >> "$_MOCK_LOG"
    if [[ "$1" == "split-window" ]]; then
        echo "%42"
    fi
    return 0
}
terminal_adapter_split_current_pane "30" "echo hello"
unset -f tmux
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "split-window -v -p 30 -P -F" "should use -P -F to get pane ID"
assert_contains "$_mock_calls" "send-keys -t %42" "should send to captured pane ID"

test_case "split_current_pane without command does simple split"
: > "$_MOCK_LOG"
tmux() {
    echo "CMD:$*" >> "$_MOCK_LOG"
    return 0
}
terminal_adapter_split_current_pane "50" ""
unset -f tmux
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "split-window -v -p 50" "should do simple split"
assert_not_contains "$_mock_calls" "send-keys" "should NOT send any keys"

test_case "resolve_pane_target preserves window context for pane id"
resolved_target=""
tmux() {
    if [[ "$1" == "list-panes" && "$2" == "-a" ]]; then
        printf 'wh-test|1|2|%%42\n'
        return 0
    fi
    return 1
}
resolved_target=$(terminal_adapter_resolve_pane_target "%42")
unset -f tmux
assert_eq "wh-test|1|2|%42" "$resolved_target" "should include window index and pane index"

test_case "kill_pane_target accepts fully qualified tmux target"
: > "$_MOCK_LOG"
tmux() {
    echo "CMD:$*" >> "$_MOCK_LOG"
    return 0
}
terminal_adapter_kill_pane_target "wh-test" "1.2"
unset -f tmux
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "kill-pane -t wh-test:1.2" "should target the specified window and pane"
assert_not_contains "$_mock_calls" "wh-test:0.1.2" "should not force window 0 when target already includes window"

test_case "pane close index uses stable pane target"
_pane_test_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_test_orig_current_session=$(declare -f terminal_adapter_get_current_session)
_pane_test_orig_count=$(declare -f terminal_adapter_count_panes)
_pane_test_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_test_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_test_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_test_orig_verify=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh-test"; }
terminal_adapter_count_panes() { echo "3"; }
terminal_adapter_list_panes_format() {
    if [[ "$1" == "wh-test" ]]; then
        printf '2|%%42\n'
        return 0
    fi
    return 1
}
_pane_kill_target_capture=""
terminal_adapter_kill_pane_target() {
    _pane_kill_target_capture="$2"
    return 0
}
_pane_trigger_save() { return 0; }
_pane_verify_close_result() { return 0; }
rc=0
_pane_cmd_close 2 --force || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_list_panes_format terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result
builtin eval "$_pane_test_orig_require_tmux"
builtin eval "$_pane_test_orig_current_session"
builtin eval "$_pane_test_orig_count"
builtin eval "$_pane_test_orig_list"
builtin eval "$_pane_test_orig_kill_target"
builtin eval "$_pane_test_orig_trigger_save"
builtin eval "$_pane_test_orig_verify"
assert_eq 0 "$rc" "close should succeed"
assert_eq "%42" "$_pane_kill_target_capture" "index-based close should use pane id as stable target"

test_case "pane close --id rejects closing the session's last pane"
_pane_test_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_test_orig_current_session=$(declare -f terminal_adapter_get_current_session)
_pane_test_orig_count=$(declare -f terminal_adapter_count_panes)
_pane_test_orig_resolve=$(declare -f terminal_adapter_resolve_pane_target)
_pane_test_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_test_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_test_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_test_orig_verify=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh-test"; }
terminal_adapter_count_panes() { echo "1"; }
terminal_adapter_resolve_pane_target() { echo "wh-test|0|0|%42"; }
terminal_adapter_list_panes_format() { printf '0|0|%%42\n'; }
_pane_kill_target_capture=""
terminal_adapter_kill_pane_target() {
    _pane_kill_target_capture="$2"
    return 0
}
_pane_trigger_save() { return 0; }
_pane_verify_close_result() { return 0; }
rc=0
_pane_cmd_close --id %42 --force || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_resolve_pane_target terminal_adapter_list_panes_format terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result
builtin eval "$_pane_test_orig_require_tmux"
builtin eval "$_pane_test_orig_current_session"
builtin eval "$_pane_test_orig_count"
builtin eval "$_pane_test_orig_resolve"
builtin eval "$_pane_test_orig_list"
builtin eval "$_pane_test_orig_kill_target"
builtin eval "$_pane_test_orig_trigger_save"
builtin eval "$_pane_test_orig_verify"
assert_eq 1 "$rc" "close by pane id should reject the last pane in the session"
assert_eq "" "$_pane_kill_target_capture" "should not attempt to kill the last pane"

test_case "pane close --id allows closing a one-pane window when session has other windows"
_pane_test_orig_require_tmux=$(declare -f _pane_require_tmux)
_pane_test_orig_current_session=$(declare -f terminal_adapter_get_current_session)
_pane_test_orig_count=$(declare -f terminal_adapter_count_panes)
_pane_test_orig_resolve=$(declare -f terminal_adapter_resolve_pane_target)
_pane_test_orig_list=$(declare -f terminal_adapter_list_panes_format)
_pane_test_orig_kill_target=$(declare -f terminal_adapter_kill_pane_target)
_pane_test_orig_trigger_save=$(declare -f _pane_trigger_save)
_pane_test_orig_verify=$(declare -f _pane_verify_close_result)
_pane_require_tmux() { return 0; }
terminal_adapter_get_current_session() { echo "wh-test"; }
terminal_adapter_count_panes() {
    if [[ "$1" == "wh-test" ]]; then
        echo "1"
    else
        echo "0"
    fi
}
terminal_adapter_resolve_pane_target() { echo "wh-test|1|0|%42"; }
terminal_adapter_list_panes_format() { printf '0|0|%%10\n1|0|%%42\n'; }
_pane_kill_target_capture=""
terminal_adapter_kill_pane_target() {
    _pane_kill_target_capture="$2"
    return 0
}
_pane_trigger_save() { return 0; }
_pane_verify_close_result() { return 0; }
_pane_cmd_close__target_is_last_session_pane() { return 1; }
rc=0
_pane_cmd_close --id %42 --force || rc=$?
unset -f _pane_require_tmux terminal_adapter_get_current_session terminal_adapter_count_panes terminal_adapter_resolve_pane_target terminal_adapter_list_panes_format terminal_adapter_kill_pane_target _pane_trigger_save _pane_verify_close_result _pane_cmd_close__target_is_last_session_pane
builtin eval "$_pane_test_orig_require_tmux"
builtin eval "$_pane_test_orig_current_session"
builtin eval "$_pane_test_orig_count"
builtin eval "$_pane_test_orig_resolve"
builtin eval "$_pane_test_orig_list"
builtin eval "$_pane_test_orig_kill_target"
builtin eval "$_pane_test_orig_trigger_save"
builtin eval "$_pane_test_orig_verify"
assert_eq 0 "$rc" "close by pane id should allow closing a one-pane window when session has other windows"
assert_eq "1.0" "$_pane_kill_target_capture" "pane id path should use resolved window-aware target"

test_case "pane verify close fails when pane-state lookup fails"
_pane_test_orig_count=$(declare -f terminal_adapter_count_panes)
_pane_test_orig_list=$(declare -f terminal_adapter_list_panes_format)
terminal_adapter_count_panes() {
    echo "3"
}
terminal_adapter_list_panes_format() {
    return 1
}
rc=0
_pane_verify_close_result "wh-test" "3" "%42" || rc=$?
unset -f terminal_adapter_count_panes terminal_adapter_list_panes_format
builtin eval "$_pane_test_orig_count"
builtin eval "$_pane_test_orig_list"
assert_eq 1 "$rc" "verification should fail when remaining pane state cannot be read"

test_case "pane verify close succeeds when target pane disappears even if active window count is unchanged"
_pane_test_orig_count=$(declare -f terminal_adapter_count_panes)
_pane_test_orig_list=$(declare -f terminal_adapter_list_panes_format)
terminal_adapter_count_panes() {
    echo "3"
}
terminal_adapter_list_panes_format() {
    printf '0|%%10\n1|%%11\n'
}
rc=0
_pane_verify_close_result "wh-test" "3" "%42" || rc=$?
unset -f terminal_adapter_count_panes terminal_adapter_list_panes_format
builtin eval "$_pane_test_orig_count"
builtin eval "$_pane_test_orig_list"
assert_eq 0 "$rc" "verification should rely on pane disappearance, not active window pane count"

test_case "split with high percentage works"
: > "$_MOCK_LOG"
tmux() {
    echo "CMD:$*" >> "$_MOCK_LOG"
    if [[ "$1" == "split-window" ]]; then
        echo "%55"
    fi
    return 0
}
terminal_adapter_split_current_pane "80" "npm test"
unset -f tmux
_mock_calls=$(cat "$_MOCK_LOG")
assert_contains "$_mock_calls" "-p 80" "should use 80% split"
rm -f "$_MOCK_LOG"

# ──────────────────────────────────────────────────
# Part 4: Live tmux pane operations
# ──────────────────────────────────────────────────

if command -v tmux &>/dev/null; then
    _PANE_TEST_SESSION=$(test_tmux_make_session_name "pane-live")

    tmux new-session -d -s "$_PANE_TEST_SESSION" -c "/tmp" 2>/dev/null

    if tmux has-session -t "$_PANE_TEST_SESSION" 2>/dev/null; then

        test_suite "pane command — live tmux split + send + list"

        test_case "split creates a new pane (count increases)"
        count_before=$(tmux list-panes -t "$_PANE_TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
        tmux split-window -v -t "$_PANE_TEST_SESSION" 2>/dev/null
        count_after=$(tmux list-panes -t "$_PANE_TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
        assert_eq "$((count_before + 1))" "$count_after" "pane count should increase by 1"

        test_case "split with -P -F returns valid pane ID"
        new_id=$(tmux split-window -v -t "$_PANE_TEST_SESSION" -P -F '#{pane_id}' 2>/dev/null)
        assert_not_empty "$new_id" "pane ID should not be empty"
        if [[ "$new_id" == %* ]]; then rc=0; else rc=1; fi
        assert_eq 0 "$rc" "pane ID should start with %"

        test_case "send-keys to specific pane ID works"
        tmux send-keys -t "$new_id" "echo __PANE_TEST_MARKER__" C-m 2>/dev/null
        sleep 0.3
        captured=$(tmux capture-pane -t "$new_id" -p 2>/dev/null)
        assert_contains "$captured" "__PANE_TEST_MARKER__" "marker should appear in target pane"

        test_case "send-keys to pane ID does NOT affect other panes"
        # Get the first pane
        first_pane=$(tmux list-panes -t "$_PANE_TEST_SESSION" -F '#{pane_id}' 2>/dev/null | head -1)
        first_content=$(tmux capture-pane -t "$first_pane" -p 2>/dev/null)
        assert_not_contains "$first_content" "__PANE_TEST_MARKER__" "marker should NOT appear in first pane"

        test_case "list-panes returns all panes with data"
        panes=$(tmux list-panes -t "$_PANE_TEST_SESSION" -F '#{pane_index}|#{pane_current_command}|#{pane_width}x#{pane_height}' 2>/dev/null)
        line_count=$(echo "$panes" | wc -l | tr -d ' ')
        if [[ "$line_count" -ge 3 ]]; then rc=0; else rc=1; fi
        assert_eq 0 "$rc" "should have at least 3 panes"

        test_case "terminal_adapter_count_panes returns correct count"
        adapter_count=$(terminal_adapter_count_panes "$_PANE_TEST_SESSION")
        assert_eq "$line_count" "$adapter_count" "adapter count should match tmux count"

        # ── Multi-pane split precision test ──
        test_suite "pane command — multi-pane split precision (AI orchestration)"

        test_case "sequential splits target correct panes (not {bottom})"
        # Kill all extra panes to start fresh
        tmux kill-session -t "$_PANE_TEST_SESSION" 2>/dev/null || true
        tmux new-session -d -s "$_PANE_TEST_SESSION" -c "/tmp" 2>/dev/null
        sleep 0.2

        # Simulate AI orchestration: split twice from first pane
        # Split 1: creates pane below (should be pane index 1)
        pane1=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -P -F '#{pane_id}' 2>/dev/null)
        sleep 0.1

        # Split 2: split again from original pane 0 (should be a new pane between 0 and 1)
        tmux select-pane -t "${_PANE_TEST_SESSION}:0.0" 2>/dev/null
        pane2=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -P -F '#{pane_id}' 2>/dev/null)
        sleep 0.1

        # Send unique markers to each pane
        tmux send-keys -t "$pane1" "echo MARKER_PANE1" C-m 2>/dev/null
        tmux send-keys -t "$pane2" "echo MARKER_PANE2" C-m 2>/dev/null
        sleep 0.3

        # Verify markers landed in correct panes
        p1_content=$(tmux capture-pane -t "$pane1" -p 2>/dev/null)
        p2_content=$(tmux capture-pane -t "$pane2" -p 2>/dev/null)
        assert_contains "$p1_content" "MARKER_PANE1" "pane1 should have MARKER_PANE1"
        assert_not_contains "$p1_content" "MARKER_PANE2" "pane1 should NOT have MARKER_PANE2"
        assert_contains "$p2_content" "MARKER_PANE2" "pane2 should have MARKER_PANE2"
        assert_not_contains "$p2_content" "MARKER_PANE1" "pane2 should NOT have MARKER_PANE1"

        test_case "3-way split: each pane receives independent commands"
        tmux kill-session -t "$_PANE_TEST_SESSION" 2>/dev/null || true
        tmux new-session -d -s "$_PANE_TEST_SESSION" -c "/tmp" 2>/dev/null
        sleep 0.2

        p_a=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -P -F '#{pane_id}' 2>/dev/null)
        p_b=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -P -F '#{pane_id}' 2>/dev/null)
        p_c=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -P -F '#{pane_id}' 2>/dev/null)
        sleep 0.1

        tmux send-keys -t "$p_a" "echo TASK_A" C-m 2>/dev/null
        tmux send-keys -t "$p_b" "echo TASK_B" C-m 2>/dev/null
        tmux send-keys -t "$p_c" "echo TASK_C" C-m 2>/dev/null
        sleep 0.3

        ca=$(tmux capture-pane -t "$p_a" -p 2>/dev/null)
        cb=$(tmux capture-pane -t "$p_b" -p 2>/dev/null)
        cc=$(tmux capture-pane -t "$p_c" -p 2>/dev/null)
        assert_contains "$ca" "TASK_A" "pane A should have TASK_A"
        assert_contains "$cb" "TASK_B" "pane B should have TASK_B"
        assert_contains "$cc" "TASK_C" "pane C should have TASK_C"
        assert_not_contains "$ca" "TASK_B" "pane A should not have TASK_B"
        assert_not_contains "$cb" "TASK_C" "pane B should not have TASK_C"

        test_case "terminal_adapter_split_current_pane with cmd targets correct pane (live)"
        tmux kill-session -t "$_PANE_TEST_SESSION" 2>/dev/null || true
        tmux new-session -d -s "$_PANE_TEST_SESSION" -c "/tmp" 2>/dev/null
        # Add a second column to make it a multi-pane layout
        tmux split-window -h -t "$_PANE_TEST_SESSION" 2>/dev/null
        sleep 0.2
        count_before=$(terminal_adapter_count_panes "$_PANE_TEST_SESSION")

        # Set TMUX var to emulate being inside this session
        TMUX="/tmp/tmux-$(id -u)/default,$$,0"
        export TMUX
        # Split from pane 0 with a command
        # shellcheck disable=SC2155
        local_new_id=$(tmux split-window -v -t "${_PANE_TEST_SESSION}:0.0" -p 50 -P -F '#{pane_id}' 2>/dev/null)
        sleep 0.1
        if [[ -n "$local_new_id" ]]; then
            tmux send-keys -t "$local_new_id" C-u "echo SPLIT_LIVE_TEST" C-m 2>/dev/null
            sleep 0.3
            live_content=$(tmux capture-pane -t "$local_new_id" -p 2>/dev/null)
            assert_contains "$live_content" "SPLIT_LIVE_TEST" "live split target should have test marker"

            # Verify the other panes do NOT have the marker
            other_panes=$(tmux list-panes -t "$_PANE_TEST_SESSION" -F '#{pane_id}' 2>/dev/null | grep -v "^${local_new_id}$")
            for op in $other_panes; do
                op_content=$(tmux capture-pane -t "$op" -p 2>/dev/null)
                assert_not_contains "$op_content" "SPLIT_LIVE_TEST" "other pane $op should NOT have marker"
            done
        else
            skip_test "split-window -P not available"
        fi
        unset TMUX

    else
        test_case "tmux session creation (skip - tmux server not accessible)"
        skip_test "Cannot create tmux session"
    fi
else
    test_case "pane live tests (tmux not installed)"
    skip_test "tmux not installed"
fi

# ──────────────────────────────────────────────────
# Part 5: Shell command tests
# ──────────────────────────────────────────────────

test_suite "shell command — integration path"

test_case "shell --help shows subcommands"
output=$("$BIN_DIR/workhive" shell --help 2>&1)
assert_contains "$output" "init" "should show init"
assert_contains "$output" "remove" "should show remove"
assert_contains "$output" "show" "should show show"

test_case "shell init --dry-run uses global config dir"
output=$("$BIN_DIR/workhive" shell init --dry-run 2>&1)
_expected_shell_dir="${XDG_CONFIG_HOME:-$HOME/.config}/workhive/shell"
assert_contains "$output" "$_expected_shell_dir" "should use global config path"
assert_not_contains "$output" "configs/shell" "should NOT use project path"

test_case "shell show outputs integration script"
output=$("$BIN_DIR/workhive" shell show 2>&1)
assert_contains "$output" "workhive" "should contain workhive"
assert_contains "$output" "Module: completions" "should contain completions module scaffold"

test_case "shell unknown subcommand shows error"
rc=0; output=$("$BIN_DIR/workhive" shell badcmd 2>&1) || rc=$?
assert_eq 1 "$rc" "should fail"

test_case "shell init --shell invalid shows error"
rc=0; output=$("$BIN_DIR/workhive" shell init --shell powershell 2>&1) || rc=$?
assert_eq 1 "$rc" "should fail for unsupported shell"

test_report

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/adapters/terminal/
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/adapters/terminal/factory.sh"

test_suite "lib/infra/adapters/terminal/ (Factory & Adapters)"

# ──────────────────────────────────────────────────
# factory.sh
# ──────────────────────────────────────────────────

test_case "terminal_adapter_init succeeds"
terminal_adapter_reset
terminal_adapter_init >/dev/null 2>&1
assert_eq "0" "$?"

test_case "terminal_adapter_get returns adapter name"
result=$(terminal_adapter_get)
assert_not_empty "$result" "should return adapter name"

test_case "terminal_adapter_get returns tmux or screen"
result=$(terminal_adapter_get)
if [[ "$result" == "tmux" || "$result" == "screen" ]]; then
    assert_eq "0" "0" "valid adapter: $result"
else
    assert_eq "tmux_or_screen" "$result" "unexpected adapter"
fi

test_case "terminal_adapter_init is idempotent"
terminal_adapter_init >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "second init should succeed"

test_case "terminal_adapter_reset clears adapter"
terminal_adapter_reset
assert_eq "" "$CURRENT_TERMINAL_ADAPTER" "should be empty after reset"

test_case "terminal_adapter_init with invalid adapter fails"
terminal_adapter_reset
WORKHIVE_TERMINAL_ADAPTER="nonexistent_adapter" terminal_adapter_init >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "invalid adapter should fail"

# Restore normal adapter
terminal_adapter_reset
terminal_adapter_init >/dev/null 2>&1

# ──────────────────────────────────────────────────
# tmux adapter (if available)
# ──────────────────────────────────────────────────

if tmux_usable; then
    test_suite "lib/infra/adapters/terminal/tmux.sh"

    # Force load tmux adapter
    terminal_adapter_reset
    source "$LIB_DIR/infra/adapters/terminal/tmux.sh"

    test_case "terminal_adapter_name returns tmux"
    result=$(terminal_adapter_name)
    assert_eq "tmux" "$result"

    test_case "terminal_adapter_check_available returns 0"
    terminal_adapter_check_available
    assert_eq "0" "$?"

    test_case "terminal_adapter_has_session returns 1 for non-existent"
    terminal_adapter_has_session "nonexistent_session_xyz_$$" || rc=$?
    assert_eq "1" "${rc:-0}"

    test_case "terminal_adapter_kill_session is safe for non-existent"
    terminal_adapter_kill_session "nonexistent_session_xyz_$$" 2>/dev/null
    # Should not crash
    assert_eq "0" "0" "no crash"

    test_case "terminal_adapter_list_sessions returns string"
    result=$(terminal_adapter_list_sessions)
    # May return empty string, but should not error
    assert_eq "0" "$?" "should not error"

    # Create temporary session for testing more features
    TEST_SESSION=""
    test_tmux_new_session TEST_SESSION "adapter-main" "/tmp" || true

    if tmux has-session -t "$TEST_SESSION" 2>/dev/null; then
        test_case "terminal_adapter_has_session returns 0 for existing"
        terminal_adapter_has_session "$TEST_SESSION"
        assert_eq "0" "$?"

        test_case "terminal_adapter_list_sessions includes test session"
        result=$(terminal_adapter_list_sessions)
        assert_contains "$result" "$TEST_SESSION"

        test_case "terminal_adapter_apply_config does not crash"
        terminal_adapter_apply_config "$TEST_SESSION" >/dev/null 2>&1
        assert_eq "0" "$?" "apply config should not crash"

        test_case "terminal_adapter_apply_config sets window status with window and pane index"
        window_status_format=$(tmux show-option -t "$TEST_SESSION" -v window-status-format 2>/dev/null || true)
        window_status_current_format=$(tmux show-option -t "$TEST_SESSION" -v window-status-current-format 2>/dev/null || true)
        assert_contains "$window_status_format" "#I.#P:#W" "window status format should include window and pane index"
        assert_contains "$window_status_current_format" "#I.#P:#W" "current window status format should include window and pane index"

        test_case "terminal_adapter_send_keys does not crash"
        terminal_adapter_send_keys "$TEST_SESSION" "0" "echo test" >/dev/null 2>&1
        assert_eq "0" "$?" "send keys should not crash"

        test_case "terminal_adapter_send_text returns 0 for valid pane"
        terminal_adapter_send_text "$TEST_SESSION" "0" "echo test" "false" >/dev/null 2>&1
        assert_eq "0" "$?" "send text should succeed"

        test_case "terminal_adapter_send_text returns 1 for invalid pane"
        rc=0
        terminal_adapter_send_text "$TEST_SESSION" "99" "echo test" "false" >/dev/null 2>&1 || rc=$?
        assert_eq "1" "${rc:-0}" "send text should fail with invalid pane"

        test_case "terminal_adapter_list_panes_format returns 0 for valid session"
        result=$(terminal_adapter_list_panes_format "$TEST_SESSION" '#{pane_index}|#{pane_id}' 2>/dev/null)
        rc=$?
        assert_eq "0" "$rc" "list panes should succeed"
        assert_not_empty "$result" "list panes output should not be empty"

        test_case "terminal_adapter_list_panes_format returns 1 for invalid session"
        rc=0
        terminal_adapter_list_panes_format "nonexistent_session_xyz_$$" '#{pane_index}|#{pane_id}' >/dev/null 2>&1 || rc=$?
        assert_eq "1" "${rc:-0}" "invalid session should fail"

        test_case "terminal_adapter_resolve_pane_target resolves existing pane id"
        pane_id=$(tmux list-panes -t "$TEST_SESSION" -F '#{pane_id}' 2>/dev/null | head -n1)
        result=$(terminal_adapter_resolve_pane_target "$pane_id" 2>/dev/null)
        rc=$?
        assert_eq "0" "$rc" "resolve should succeed"
        IFS='|' read -r resolved_session resolved_window resolved_pane resolved_id <<< "$result"
        assert_eq "$TEST_SESSION" "${resolved_session:-}" "resolved session should match"
        assert_eq "$pane_id" "${resolved_id:-}" "resolved pane id should match"
        [[ "${resolved_window:-}" =~ ^[0-9]+$ ]] && _window_numeric=1 || _window_numeric=0
        [[ "${resolved_pane:-}" =~ ^[0-9]+$ ]] && _pane_numeric=1 || _pane_numeric=0
        assert_eq "1" "$_window_numeric" "resolved window index should be numeric"
        assert_eq "1" "$_pane_numeric" "resolved pane index should be numeric"

        test_case "terminal_adapter_resolve_pane_target returns 1 for missing pane id"
        rc=0
        terminal_adapter_resolve_pane_target "%999999" >/dev/null 2>&1 || rc=$?
        assert_eq "1" "${rc:-0}" "missing pane id should fail"

        test_case "terminal_adapter_count_panes returns count for mock session"
        count=$(terminal_adapter_count_panes "$TEST_SESSION")
        assert_not_empty "$count" "should return pane count"
        assert_eq "1" "$count" "single pane session should return 1"

        test_case "terminal_adapter_spawn_command_target supports shell metacharacters safely"
        marker_file=$(mktemp /tmp/workhive-spawn-marker.XXXXXX)
        rm -f "$marker_file"
        quoted_marker=$(printf '%q' "$marker_file")
        complex_cmd="printf 'a|b' | tr '|' ':' > ${quoted_marker}; true"
        terminal_adapter_spawn_command_target "${TEST_SESSION}:0.0" "$complex_cmd" "/tmp" >/dev/null 2>&1
        assert_eq "0" "$?" "spawn command should succeed"
        for _try in 1 2 3 4 5 6 7 8 9 10; do
            [[ -f "$marker_file" ]] && break
            sleep 0.1
        done
        assert_true "[[ -f \"$marker_file\" ]]" "marker file should be created by complex command"
        marker_content=$(cat "$marker_file" 2>/dev/null || true)
        assert_eq "a:b" "$marker_content" "complex command output should match"
        rm -f "$marker_file" 2>/dev/null || true

        # Cleanup
        test_tmux_kill_session "$TEST_SESSION"
    fi

    test_case "terminal_adapter_get_current_session returns something or empty"
    result=$(terminal_adapter_get_current_session 2>/dev/null || true)
    # May be empty when not inside tmux; should have value when inside tmux; should not crash
    assert_eq "0" "$?" "should not crash"

    test_case "terminal_adapter_attach_session_readonly exists as function"
    declare -f terminal_adapter_attach_session_readonly >/dev/null 2>&1
    assert_eq "0" "$?" "function should exist"

    test_case "terminal_adapter_attach_session sets status=ended when attach fails and session disappears"
    (
        TMUX=""
        tmux() {
            if [[ "$1" == "attach" ]]; then
                return 1
            fi
            if [[ "$1" == "has-session" ]]; then
                return 1
            fi
            return 0
        }
        terminal_adapter_attach_session "wh@test" >/dev/null 2>&1
        rc=$?
        status=$(terminal_adapter_get_last_attach_status 2>/dev/null || true)
        [[ "$rc" -eq 0 ]] || exit 1
        [[ "$status" == "ended" ]] || exit 1
        exit 0
    )
    assert_eq "0" "$?" "ended attach should be treated as success with explicit status"

    test_case "terminal_adapter_attach_session reconciles geometry before external attach"
    _attach_geom_log=$(mktemp /tmp/workhive-attach-geom.XXXXXX)
    (
        TMUX=""
        COLUMNS=141
        LINES=53
        tmux() {
            case "$1" in
                resize-window)
                    printf '%s\n' "resize-window $*" >> "$_attach_geom_log"
                    return 0
                    ;;
                list-panes)
                    printf '%s\n' "%1|141|53"
                    return 0
                    ;;
                resize-pane)
                    printf '%s\n' "resize-pane $*" >> "$_attach_geom_log"
                    return 0
                    ;;
                attach)
                    return 1
                    ;;
                has-session)
                    return 1
                    ;;
                *)
                    return 0
                    ;;
            esac
        }
        terminal_adapter_attach_session "wh@test" >/dev/null 2>&1
        rc=$?
        status=$(terminal_adapter_get_last_attach_status 2>/dev/null || true)
        [[ "$rc" -eq 0 ]] || exit 1
        [[ "$status" == "ended" ]] || exit 1
        exit 0
    )
    assert_eq "0" "$?" "attach should still be treated as ended-success"
    _attach_geom_calls=$(cat "$_attach_geom_log" 2>/dev/null || true)
    assert_contains "$_attach_geom_calls" "resize-window -t wh@test -x 141 -y 53" "pre-attach should resize window to current terminal size"
    assert_contains "$_attach_geom_calls" "resize-pane -t %1 -x 141 -y 53" "pre-attach should reflow panes"
    rm -f "$_attach_geom_log" 2>/dev/null || true

    test_case "terminal_adapter_attach_session sets status=error when attach fails but session still exists"
    (
        TMUX=""
        tmux() {
            if [[ "$1" == "attach" ]]; then
                return 1
            fi
            if [[ "$1" == "has-session" ]]; then
                return 0
            fi
            return 0
        }
        terminal_adapter_attach_session "wh@test" >/dev/null 2>&1
        rc=$?
        status=$(terminal_adapter_get_last_attach_status 2>/dev/null || true)
        [[ "$rc" -eq 1 ]] || exit 1
        [[ "$status" == "error" ]] || exit 1
        exit 0
    )
    assert_eq "0" "$?" "real attach error should propagate with explicit status"

    test_case "terminal_adapter_select_pane does not crash for non-existent session"
    terminal_adapter_select_pane "nonexistent_session_xyz_$$" "0" 2>/dev/null || true
    assert_eq "0" "0" "no crash"

    test_case "terminal_adapter_resize_pane does not crash for non-existent session"
    terminal_adapter_resize_pane "nonexistent_session_xyz_$$" "0" "down" 5 2>/dev/null || true
    assert_eq "0" "0" "no crash"

    test_case "terminal_adapter_get_session_info does not crash for non-existent session"
    result=$(terminal_adapter_get_session_info "nonexistent_session_xyz_$$" 2>/dev/null || true)
    assert_eq "0" "$?" "should not crash"

    test_case "terminal_adapter_create_session creates session"
    TEST_SESSION2=$(test_tmux_make_session_name "adapter-create")
    terminal_adapter_create_session "$TEST_SESSION2" "/tmp" >/dev/null 2>&1
    terminal_adapter_has_session "$TEST_SESSION2"
    assert_eq "0" "$?" "session should exist"
    # Cleanup
    test_tmux_kill_session "$TEST_SESSION2"

    # -- Layout persistence related functions --

    test_case "terminal_adapter_get_layout_string returns layout for session"
    TEST_SESSION3=""
    test_tmux_new_session TEST_SESSION3 "adapter-layout" "/tmp" || true
    result=$(terminal_adapter_get_layout_string "$TEST_SESSION3" 2>/dev/null)
    assert_not_empty "$result" "should return layout string"
    test_tmux_kill_session "$TEST_SESSION3"

    test_case "terminal_adapter_get_layout_string returns empty for non-existent"
    result=$(terminal_adapter_get_layout_string "nonexistent_$$" 2>/dev/null)
    assert_empty "$result" "should return empty for non-existent session"

else
    test_case "tmux adapter tests (tmux not available - skip)"
    skip_test "tmux not available or unusable"
fi

# ──────────────────────────────────────────────────
# base.sh (abstract interface)
# ──────────────────────────────────────────────────

test_suite "lib/infra/adapters/terminal/base.sh"

test_case "base adapter interface defines expected functions"
# Verify all interface functions exist
for fn in terminal_adapter_name terminal_adapter_check_available \
           terminal_adapter_create_session terminal_adapter_has_session \
           terminal_adapter_attach_session terminal_adapter_kill_session \
           terminal_adapter_split_vertical terminal_adapter_split_horizontal \
           terminal_adapter_send_keys terminal_adapter_apply_config \
           terminal_adapter_list_sessions \
           terminal_adapter_get_layout_string; do
    if declare -f "$fn" >/dev/null 2>&1; then
        : # exists
    else
        assert_eq "exists" "missing" "function $fn should exist"
    fi
done
assert_eq "0" "0" "all interface functions exist"

# ──────────────────────────────────────────────────
# New adapter methods (abstract interface)
# ──────────────────────────────────────────────────

test_case "terminal_adapter_get_window_count is defined"
type terminal_adapter_get_window_count &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_is_session_attached is defined"
type terminal_adapter_is_session_attached &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_get_window_size is defined"
type terminal_adapter_get_window_size &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_resize_window is defined"
type terminal_adapter_resize_window &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_list_panes_format is defined"
type terminal_adapter_list_panes_format &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_set_after_kill_pane_hook is defined"
type terminal_adapter_set_after_kill_pane_hook &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_run_shell is defined"
type terminal_adapter_run_shell &>/dev/null
assert_eq "0" "$?"

test_case "terminal_adapter_get_layout_string is defined"
type terminal_adapter_get_layout_string &>/dev/null
assert_eq "0" "$?"

test_report

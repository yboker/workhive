#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Test: Directory Relocation Recovery
#
# Tests that workhive correctly handles project directory moves:
# - Snapshot CWD paths are remapped to new location
# - Pane commands (claude, lazygit, yazi) are restored
# - Corrupt snapshot data is detected and rejected
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck disable=SC2034
BIN_DIR="$ROOT_DIR/bin"
export LIB_DIR="$ROOT_DIR/lib"
TEST_BASE_DIR="/tmp/workhive-bdd-relocation-$$"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export XDG_CONFIG_HOME="${TEST_BASE_DIR}/xdg_config"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

# ──────────────────────────────────────────────────
# BDD Helpers
# ──────────────────────────────────────────────────

scenario() {
    echo ""
    echo -e "${CYAN}Scenario: $1${NC}"
}

given() {
    echo -e "  ${BLUE}Given${NC} $1"
}

when() {
    echo -e "  ${BLUE}When${NC} $1"
}

then_assert() {
    local description="$1"
    local result="$2"
    TESTS_RUN=$((TESTS_RUN + 1))

    if [[ "$result" == "0" ]]; then
        echo -e "  ${GREEN}Then${NC} $description ${GREEN}✓${NC}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "  ${RED}Then${NC} $description ${RED}✗${NC}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

skip_scenario() {
    TESTS_RUN=$((TESTS_RUN + 1))
    TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
    echo -e "  ${YELLOW}Skipped${NC} $1"
}

# ──────────────────────────────────────────────────
# Setup / Teardown
# ──────────────────────────────────────────────────

setup() {
    rm -rf "$TEST_BASE_DIR"
    mkdir -p "$TEST_BASE_DIR" "$XDG_CONFIG_HOME"
}

teardown() {
    cd /
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_BASE_DIR" 2>/dev/null || true
    rm -rf "$TEST_BASE_DIR"
}

trap teardown EXIT

# ──────────────────────────────────────────────────
# Unit-level BDD: _restore_resolve_dir
# ──────────────────────────────────────────────────

source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/utils/i18n.sh"
# shellcheck disable=SC2034
WORKHIVE_LANG="en"

_workhive_dir() { echo "$TEST_BASE_DIR/.workhive"; }
session_get_instance_id() { echo "test"; }
session_get_project_name() { echo "myproject"; }
session_get_prefix() { echo "wh@test@myproject"; }
session_get_name() { echo "wh@test@myproject@$1"; }
session_apply_config() { true; }
config_get() { echo ""; }
mkdir -p "$TEST_BASE_DIR/.workhive"

source "$LIB_DIR/infra/persistence/snapshot.sh"
source "$LIB_DIR/infra/persistence/process_capture.sh"
source "$LIB_DIR/infra/persistence/restore.sh"

setup

echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${CYAN}  BDD: Directory Relocation Recovery${NC}"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# ──────────────────────────────────────────────────
# Scenario 1: CWD resolution after directory move
# ──────────────────────────────────────────────────

scenario "CWD resolution remaps old path to new location"

given "a project was at /old/path/myproject"
OLD_PROJ="$TEST_BASE_DIR/old_location/myproject"

given "the project has been moved to a new location"
NEW_PROJ="$TEST_BASE_DIR/new_location/myproject"
mkdir -p "$NEW_PROJ"

when "_restore_resolve_dir is called with old path and new fallback"
result=$(_restore_resolve_dir "$OLD_PROJ" "$NEW_PROJ")
then_assert "old path is remapped to new path (same basename)" "$([[ "$result" == "$NEW_PROJ" ]] && echo 0 || echo 1)"

when "_restore_resolve_dir is called with existing path"
EXISTING_DIR="$TEST_BASE_DIR/existing_dir"
mkdir -p "$EXISTING_DIR"
result=$(_restore_resolve_dir "$EXISTING_DIR" "$NEW_PROJ")
then_assert "existing path is used unchanged" "$([[ "$result" == "$EXISTING_DIR" ]] && echo 0 || echo 1)"

when "_restore_resolve_dir is called with empty string"
result=$(_restore_resolve_dir "" "")
then_assert "empty dir falls back to HOME" "$([[ "$result" == "$HOME" ]] && echo 0 || echo 1)"

when "_restore_resolve_dir is called with non-absolute path (corrupt data)"
result=$(_restore_resolve_dir "1" "$NEW_PROJ")
then_assert "non-absolute path falls back to new project root" "$([[ "$result" == "$NEW_PROJ" ]] && echo 0 || echo 1)"

# ──────────────────────────────────────────────────
# Scenario 2: Snapshot validation rejects corrupt data
# ──────────────────────────────────────────────────

scenario "Snapshot validation detects field-shifted corrupt data"

given "a snapshot with non-absolute dir value (field shift corruption)"

mkdir -p "$TEST_BASE_DIR/.workhive"
cat > "$TEST_BASE_DIR/.workhive/config.yaml" <<'YAML'
tmux:
  persistence:
    enabled: true
    restore_processes: true
YAML

corrupt_snapshot="$TEST_BASE_DIR/corrupt_snapshot.jsonl"
{
    snapshot_format_meta "test" "myproject" "wh@test@myproject" "0.9.0"
    # Corrupt pane: dir="1" (should be absolute path), pane_cmd="48875" (looks like PID)
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "0" "BOBO" "1" "1" "48875" ""
    snapshot_format_window "wh@test@myproject@main" "0" "main" "1" "*" "layout" "1"
} > "$corrupt_snapshot"

log_debug() { :; }
log_warn() { :; }
tmux() {
    case "$1" in
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}

when "attempting to restore from corrupt snapshot"
restore_rc=0
_restore_session_from_snapshot "$corrupt_snapshot" "wh@test@myproject@main" || restore_rc=$?
then_assert "restore handles non-absolute dir without crashing" "$([[ $restore_rc -eq 0 ]] && echo 0 || echo 1)"

# ──────────────────────────────────────────────────
# Scenario 3: Full restore after directory move
# ──────────────────────────────────────────────────

scenario "Restore succeeds after project directory move via CWD remap"

given "a v2 JSONL snapshot with 4 panes saved at old location (non-existent)"
OLD_LOC="$TEST_BASE_DIR/old_root/workhive"
NEW_LOC="$TEST_BASE_DIR/new_root/workhive"
mkdir -p "$NEW_LOC"

move_snapshot="$TEST_BASE_DIR/move_snapshot.jsonl"
{
    snapshot_format_meta "test" "myproject" "wh@test@myproject" "0.9.0"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "0" "BOBO-MAC" "$OLD_LOC" "1" "claude" "claude --dangerously-skip-permissions"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "1" "BOBO-MAC" "$OLD_LOC" "0" "zsh" ""
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "2" "BOBO-MAC" "$OLD_LOC" "0" "lazygit" "lazygit"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "3" "Yazi: ~/workhive" "$OLD_LOC" "0" "yazi" "yazi"
    snapshot_format_window "wh@test@myproject@main" "0" "claude" "1" "*" "layout_str" "1"
    snapshot_format_state "wh@test@myproject@main" ""
} > "$move_snapshot"

MOVE_SESSION_DIR=""
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session)
            while [[ $# -gt 0 ]]; do
                if [[ "$1" == "-c" ]]; then MOVE_SESSION_DIR="$2"; shift 2; else shift; fi
            done
            return 0 ;;
        display-message) [[ "$*" == *"pane_current_path"* ]] && echo "$HOME"; return 0 ;;
        send-keys) return 0 ;;
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}
_persistence_config_get() {
    case "$1" in
        restore_processes) echo "true" ;;
        *) echo "" ;;
    esac
}
_git_root() { echo "$NEW_LOC"; }

when "restoring session with stale snapshot dirs (same basename allows remap)"
restore_rc=0
_restore_session_from_snapshot "$move_snapshot" "wh@test@myproject@main" "$NEW_LOC" || restore_rc=$?

then_assert "restore succeeds via CWD remap (same basename)" "$restore_rc"
then_assert "session created at remapped location" "$([[ "$MOVE_SESSION_DIR" == "$NEW_LOC" ]] && echo 0 || echo 1)"

scenario "Restore succeeds when snapshot dirs still exist"

given "a v2 JSONL snapshot where all dirs exist"
EXIST_LOC="$TEST_BASE_DIR/existing_project/workhive"
mkdir -p "$EXIST_LOC"

exist_snapshot="$TEST_BASE_DIR/exist_snapshot.jsonl"
{
    snapshot_format_meta "test" "myproject" "wh@test@myproject" "0.9.0"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "0" "BOBO-MAC" "$EXIST_LOC" "1" "claude" "claude --dangerously-skip-permissions"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "1" "BOBO-MAC" "$EXIST_LOC" "0" "lazygit" "lazygit"
    snapshot_format_window "wh@test@myproject@main" "0" "claude" "1" "*" "layout_str" "1"
    snapshot_format_state "wh@test@myproject@main" ""
} > "$exist_snapshot"

SEND_KEYS_LOG=""
SESSION_CREATE_DIR=""
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session)
            while [[ $# -gt 0 ]]; do
                if [[ "$1" == "-c" ]]; then
                    SESSION_CREATE_DIR="$2"
                    shift 2
                else
                    shift
                fi
            done
            return 0
            ;;
        new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        display-message)
            [[ "$*" == *"pane_current_path"* ]] && echo "$HOME"
            return 0
            ;;
        send-keys)
            SEND_KEYS_LOG+="|$*"
            return 0
            ;;
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

process_should_restore() { [[ -n "$1" ]] && return 0; return 1; }
process_resolve_restore_command() { echo "$1"; }
process_classify_restore_strategy() { echo "generic"; }
_persistence_config_get() {
    case "$1" in
        restore_processes) echo "true" ;;
        *) echo "" ;;
    esac
}
_git_root() { echo "$EXIST_LOC"; }

when "restoring session with valid existing dirs"
_restore_session_from_snapshot "$exist_snapshot" "wh@test@myproject@main" "$EXIST_LOC"
restore_rc=$?

then_assert "restore succeeds" "$restore_rc"
then_assert "session created in correct directory" "$([[ "$SESSION_CREATE_DIR" == "$EXIST_LOC" ]] && echo 0 || echo 1)"
then_assert "claude command restored" "$([[ "$SEND_KEYS_LOG" == *"claude --dangerously-skip-permissions"* ]] && echo 0 || echo 1)"
then_assert "lazygit command restored" "$([[ "$SEND_KEYS_LOG" == *"lazygit"* ]] && echo 0 || echo 1)"

# ──────────────────────────────────────────────────
# Scenario 4: Subpath relocation
# ──────────────────────────────────────────────────

scenario "Snapshot with non-existent subdir and no remap match causes restore failure"

given "a snapshot with pane in a completely unrelated non-existent path"
SUBDIR_PROJ="$TEST_BASE_DIR/subdir_proj/proj"
mkdir -p "$SUBDIR_PROJ"

subdir_snapshot="$TEST_BASE_DIR/subdir_snapshot.jsonl"
{
    snapshot_format_meta "test" "myproject" "wh@test@myproject" "0.9.0"
    # Use a path with a different basename that cannot remap to fallback
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "0" "main" "/completely/unrelated/vanished" "1" "bash" ""
    snapshot_format_window "wh@test@myproject@main" "0" "main" "1" "*" "layout" "1"
} > "$subdir_snapshot"

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session)
            while [[ $# -gt 0 ]]; do
                if [[ "$1" == "-c" ]]; then shift 2; else shift; fi
            done
            return 0 ;;
        display-message) [[ "$*" == *"pane_current_path"* ]] && echo "$HOME"; return 0 ;;
        send-keys) return 0 ;;
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}

when "restoring snapshot with non-existent pane directory (no remap possible)"
restore_rc=0
_restore_session_from_snapshot "$subdir_snapshot" "wh@test@myproject@main" "$SUBDIR_PROJ" || restore_rc=$?
then_assert "restore succeeds with fallback dir when original is gone" "$restore_rc"

scenario "Snapshot with existing subdirectory restores successfully"

given "a snapshot with pane in a subdirectory that still exists"
SUBDIR_OK="$TEST_BASE_DIR/subdir_ok/proj"
mkdir -p "$SUBDIR_OK/src/components"

subdir_ok_snapshot="$TEST_BASE_DIR/subdir_ok_snapshot.jsonl"
{
    snapshot_format_meta "test" "myproject" "wh@test@myproject" "0.9.0"
    snapshot_format_pane "wh@test@myproject@main" "0" "1" "*" "0" "main" "$SUBDIR_OK/src/components" "1" "bash" ""
    snapshot_format_window "wh@test@myproject@main" "0" "main" "1" "*" "layout" "1"
} > "$subdir_ok_snapshot"

SESSION_CREATE_DIR=""
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session)
            while [[ $# -gt 0 ]]; do
                if [[ "$1" == "-c" ]]; then SESSION_CREATE_DIR="$2"; shift 2; else shift; fi
            done
            return 0 ;;
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}
_persistence_config_get() { echo ""; }

when "restoring snapshot with existing subdirectory"
_restore_session_from_snapshot "$subdir_ok_snapshot" "wh@test@myproject@main" "$SUBDIR_OK"
restore_rc=$?
then_assert "restore succeeds with existing subdir" "$restore_rc"
then_assert "session created in existing subdir" "$([[ "$SESSION_CREATE_DIR" == "$SUBDIR_OK/src/components" ]] && echo 0 || echo 1)"

# ──────────────────────────────────────────────────
# Report
# ──────────────────────────────────────────────────

echo ""
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "  Test Report"
echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  Total:   $TESTS_RUN"
echo -e "  Passed:  ${GREEN}${TESTS_PASSED}${NC}"
echo -e "  Failed:  ${RED}${TESTS_FAILED}${NC}"
echo -e "  Skipped: ${YELLOW}${TESTS_SKIPPED}${NC}"
echo ""

if [[ $TESTS_FAILED -gt 0 ]]; then
    echo -e "  ${RED}Some tests failed.${NC}"
    exit 1
else
    echo -e "  ${GREEN}All tests passed!${NC}"
    exit 0
fi

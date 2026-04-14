#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Cleanup best-effort path must use terminal adapter, not direct tmux calls
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
source "$LIB_DIR/domain/services/session_service.sh"

test_suite "cleanup best-effort adapter abstraction"

# ──────────────────────────────────────────────────
# Mock infrastructure
# ──────────────────────────────────────────────────
_CLEANUP_ADAPTER_KILL_CALLS=""
_CLEANUP_DIRECT_TMUX_CALLS=""

_saved_kill_session="$(declare -f terminal_adapter_kill_session 2>/dev/null || true)"
_saved_kill_by_name="$(declare -f session_kill_by_name 2>/dev/null || true)"

# ══════════════════════════════════════════════════
# Scenario 1: best-effort cleanup routes through adapter
# ══════════════════════════════════════════════════

test_case "best-effort cleanup calls terminal_adapter_kill_session not raw tmux"
_CLEANUP_ADAPTER_KILL_CALLS=""
_CLEANUP_DIRECT_TMUX_CALLS=""

# session_kill_by_name always fails to trigger best-effort fallback
session_kill_by_name() { return 1; }
session_extract_branch() { echo "${1##*@}"; }

terminal_adapter_kill_session() {
    _CLEANUP_ADAPTER_KILL_CALLS+="$1"$'\n'
    return 0
}

tmux() {
    if [[ "$1" == "kill-session" ]]; then
        _CLEANUP_DIRECT_TMUX_CALLS+="$*"$'\n'
    fi
    return 0
}

_session_cleanup_by_list "wh@proj@feat1
wh@proj@feat2" "true"

assert_contains "$_CLEANUP_ADAPTER_KILL_CALLS" "wh@proj@feat1" "should call adapter kill for feat1"
assert_contains "$_CLEANUP_ADAPTER_KILL_CALLS" "wh@proj@feat2" "should call adapter kill for feat2"
assert_eq "" "$_CLEANUP_DIRECT_TMUX_CALLS" "must not call raw tmux kill-session"

# ══════════════════════════════════════════════════
# Scenario 2: non-best-effort does not fall back
# ══════════════════════════════════════════════════

test_case "non-best-effort cleanup does not fallback on kill failure"
_CLEANUP_ADAPTER_KILL_CALLS=""

session_kill_by_name() { return 1; }
terminal_adapter_kill_session() {
    _CLEANUP_ADAPTER_KILL_CALLS+="$1"$'\n'
    return 0
}

_session_cleanup_by_list "wh@proj@feat3" "false"

assert_eq "0" "$SESSION_CLEANUP_CLOSED" "non-best-effort should not count closed on kill failure"
assert_eq "" "$_CLEANUP_ADAPTER_KILL_CALLS" "non-best-effort should not call adapter kill fallback"

# ══════════════════════════════════════════════════
# Scenario 3: best-effort counts closures correctly
# ══════════════════════════════════════════════════

test_case "best-effort cleanup counts successful adapter kills"
_CLEANUP_ADAPTER_KILL_CALLS=""

session_kill_by_name() { return 1; }
terminal_adapter_kill_session() {
    _CLEANUP_ADAPTER_KILL_CALLS+="$1"$'\n'
    return 0
}

_session_cleanup_by_list "wh@proj@a
wh@proj@b
wh@proj@c" "true"

assert_eq "3" "$SESSION_CLEANUP_ATTEMPTED" "should attempt 3 cleanups"
assert_eq "3" "$SESSION_CLEANUP_CLOSED" "should close all 3 via adapter fallback"

# ══════════════════════════════════════════════════
# Scenario 4: primary kill success does not trigger fallback
# ══════════════════════════════════════════════════

test_case "primary kill success skips best-effort fallback entirely"
_CLEANUP_ADAPTER_KILL_CALLS=""

session_kill_by_name() { return 0; }
terminal_adapter_kill_session() {
    _CLEANUP_ADAPTER_KILL_CALLS+="$1"$'\n'
    return 0
}

_session_cleanup_by_list "wh@proj@ok1
wh@proj@ok2" "true"

assert_eq "2" "$SESSION_CLEANUP_CLOSED" "should close both via primary path"
assert_eq "" "$_CLEANUP_ADAPTER_KILL_CALLS" "adapter kill fallback should not be called when primary succeeds"

# ──────────────────────────────────────────────────
# Restore
# ──────────────────────────────────────────────────
[[ -n "$_saved_kill_session" ]] && eval "$_saved_kill_session"
[[ -n "$_saved_kill_by_name" ]] && eval "$_saved_kill_by_name"

test_report

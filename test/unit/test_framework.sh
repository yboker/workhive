#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Minimal Unit Test Framework for Bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Colors
_TF_GREEN='\033[0;32m'
_TF_RED='\033[0;31m'
_TF_YELLOW='\033[1;33m'
_TF_BLUE='\033[0;34m'
_TF_NC='\033[0m'

# Counters
_TF_TOTAL=0
_TF_PASSED=0
_TF_FAILED=0
_TF_SKIPPED=0

# Current test name
_TF_CURRENT_TEST=""

# Test tmux session policy: all test-created sessions use a dedicated prefix.
_TF_TMUX_TEST_PREFIX="${WORKHIVE_TEST_TMUX_PREFIX:-WH-TEST}"
_TF_TMUX_CREATED_SESSIONS=()
_TF_TMUX_SESSION_SEQ=0

# Test/runtime log isolation: unit tests must never write into project
# .workhive/logs/workhive.log. Route to dedicated test log files by default.
export WORKHIVE_TEST_MODE=1
export WORKHIVE_LANG="${WORKHIVE_LANG:-en}"
export WORKHIVE_TEST_LOG_DIR="${WORKHIVE_TEST_LOG_DIR:-/tmp/workhive-test-logs}"
mkdir -p "$WORKHIVE_TEST_LOG_DIR" 2>/dev/null || true

# Start a test suite
test_suite() {
    echo ""
    echo -e "${_TF_BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${_TF_NC}"
    echo -e "${_TF_BLUE}  $1${_TF_NC}"
    echo -e "${_TF_BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${_TF_NC}"
}

# Start a test case
test_case() {
    _TF_CURRENT_TEST="$1"
    _TF_TOTAL=$((_TF_TOTAL + 1))
}

# Assert equal
assert_eq() {
    local expected="$1"
    local actual="$2"
    local msg="${3:-}"

    if [[ "$expected" == "$actual" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected: ${_TF_GREEN}${expected}${_TF_NC}"
        echo -e "    actual:   ${_TF_RED}${actual}${_TF_NC}"
    fi
}

# Assert not equal
assert_ne() {
    local unexpected="$1"
    local actual="$2"
    local msg="${3:-}"

    if [[ "$unexpected" != "$actual" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    unexpected: ${_TF_RED}${unexpected}${_TF_NC}"
    fi
}

# Assert contains
assert_contains() {
    local haystack="$1"
    local needle="$2"
    local msg="${3:-}"

    if echo "$haystack" | grep -qF -- "$needle"; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected to contain: ${_TF_GREEN}${needle}${_TF_NC}"
        echo -e "    actual: ${_TF_RED}${haystack}${_TF_NC}"
    fi
}

# Assert not contains
assert_not_contains() {
    local haystack="$1"
    local needle="$2"
    local msg="${3:-}"

    if ! echo "$haystack" | grep -qF -- "$needle"; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected NOT to contain: ${_TF_RED}${needle}${_TF_NC}"
    fi
}

# Assert true (command returns 0)
assert_true() {
    local cmd="$1"
    local msg="${2:-}"

    if eval "$cmd"; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected true, got: ${_TF_RED}false${_TF_NC}"
    fi
}

# Assert success (exit code 0)
assert_success() {
    local exit_code="$1"
    local msg="${2:-}"

    if [[ "$exit_code" -eq 0 ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected exit code 0, got: ${_TF_RED}${exit_code}${_TF_NC}"
    fi
}

# Assert failure (exit code non-zero)
assert_failure() {
    local exit_code="$1"
    local msg="${2:-}"

    if [[ "$exit_code" -ne 0 ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected non-zero exit code, got: ${_TF_RED}0${_TF_NC}"
    fi
}

# Assert empty
assert_empty() {
    local actual="$1"
    local msg="${2:-}"

    if [[ -z "$actual" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected empty, got: ${_TF_RED}${actual}${_TF_NC}"
    fi
}

# Assert not empty
assert_not_empty() {
    local actual="$1"
    local msg="${2:-}"

    if [[ -n "$actual" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    expected non-empty value"
    fi
}

# Assert file exists
assert_file_exists() {
    local path="$1"
    local msg="${2:-}"

    if [[ -f "$path" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    file not found: ${_TF_RED}${path}${_TF_NC}"
    fi
}

# Assert directory exists
assert_dir_exists() {
    local path="$1"
    local msg="${2:-}"

    if [[ -d "$path" ]]; then
        _TF_PASSED=$((_TF_PASSED + 1))
        echo -e "  ${_TF_GREEN}✓${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
    else
        _TF_FAILED=$((_TF_FAILED + 1))
        echo -e "  ${_TF_RED}✗${_TF_NC} ${_TF_CURRENT_TEST}${msg:+ ($msg)}"
        echo -e "    directory not found: ${_TF_RED}${path}${_TF_NC}"
    fi
}

# Skip test
skip_test() {
    local reason="${1:-}"
    _TF_SKIPPED=$((_TF_SKIPPED + 1))
    echo -e "  ${_TF_YELLOW}⊘${_TF_NC} ${_TF_CURRENT_TEST} (SKIPPED${reason:+: $reason})"
}

_tf_tmux_sanitize_label() {
    local raw="${1:-session}"
    printf '%s' "$raw" | sed -E 's/[^A-Za-z0-9._-]+/-/g'
}

test_tmux_make_session_name() {
    local label="${1:-session}"
    local sanitized
    sanitized=$(_tf_tmux_sanitize_label "$label")
    _TF_TMUX_SESSION_SEQ=$((_TF_TMUX_SESSION_SEQ + 1))
    printf '%s@%s@%s@%s' "$_TF_TMUX_TEST_PREFIX" "$sanitized" "$$" "$_TF_TMUX_SESSION_SEQ"
}

test_tmux_register_session() {
    local session_name="${1:-}"
    [[ -z "$session_name" ]] && return 1
    _TF_TMUX_CREATED_SESSIONS+=("$session_name")
}

test_tmux_new_session() {
    local __out_var="$1"
    local label="${2:-session}"
    local workdir="${3:-${TMPDIR:-/tmp}}"
    local session_name
    session_name=$(test_tmux_make_session_name "$label")
    tmux new-session -d -s "$session_name" -c "$workdir" >/dev/null 2>&1 || return 1
    test_tmux_register_session "$session_name"
    printf -v "$__out_var" '%s' "$session_name"
    return 0
}

test_tmux_kill_session() {
    local session_name="${1:-}"
    [[ -z "$session_name" ]] && return 0
    tmux kill-session -t "$session_name" >/dev/null 2>&1 || true
}

test_tmux_cleanup_registered_sessions() {
    local sn
    for sn in "${_TF_TMUX_CREATED_SESSIONS[@]}"; do
        [[ -z "$sn" ]] && continue
        test_tmux_kill_session "$sn"
    done
    _TF_TMUX_CREATED_SESSIONS=()
}

# Check whether tmux is installed and can actually create sessions in this environment.
# This handles sandboxed cases where `tmux` exists but socket access is denied.
tmux_usable() {
    if [[ -n "${_TF_TMUX_USABLE_CACHED:-}" ]]; then
        [[ "${_TF_TMUX_USABLE_CACHED}" == "1" ]]
        return $?
    fi

    if ! command -v tmux &>/dev/null; then
        _TF_TMUX_USABLE_CACHED=0
        return 1
    fi

    local probe_session
    probe_session="$(test_tmux_make_session_name "probe")"
    if tmux new-session -d -s "$probe_session" -c "${TMPDIR:-/tmp}" >/dev/null 2>&1; then
        test_tmux_kill_session "$probe_session"
        _TF_TMUX_USABLE_CACHED=1
        return 0
    fi

    _TF_TMUX_USABLE_CACHED=0
    return 1
}

# Print final test report
test_report() {
    echo ""
    echo -e "${_TF_BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${_TF_NC}"
    echo -e "  Test Report"
    echo -e "${_TF_BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${_TF_NC}"
    echo ""
    echo -e "  Total:   $_TF_TOTAL"
    echo -e "  Passed:  ${_TF_GREEN}$_TF_PASSED${_TF_NC}"
    echo -e "  Failed:  ${_TF_RED}$_TF_FAILED${_TF_NC}"
    echo -e "  Skipped: ${_TF_YELLOW}$_TF_SKIPPED${_TF_NC}"
    echo ""

    if [[ $_TF_FAILED -eq 0 ]]; then
        echo -e "  ${_TF_GREEN}All tests passed!${_TF_NC}"
    else
        echo -e "  ${_TF_RED}Some tests failed.${_TF_NC}"
    fi
    echo ""

    [[ $_TF_FAILED -eq 0 ]]
}

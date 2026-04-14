#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: test/integration/tmux_cleanup_scope.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

source "$SCRIPT_DIR/test_framework.sh"
source "$ROOT_DIR/test/integration/tmux_cleanup_scope.sh"

test_suite "test/integration/tmux_cleanup_scope.sh"

declare -a _MOCK_TMUX_SESSIONS=()
declare -a _MOCK_TMUX_KILLED=()
declare -A _MOCK_TMUX_PANES=()

_mock_tmux_cleanup_reset() {
    _MOCK_TMUX_SESSIONS=()
    _MOCK_TMUX_KILLED=()
    _MOCK_TMUX_PANES=()
}

_mock_tmux_killed_output() {
    printf '%s\n' "${_MOCK_TMUX_KILLED[@]}"
}

tmux() {
    local cmd="${1:-}"
    shift || true

    case "$cmd" in
        list-sessions)
            printf '%s\n' "${_MOCK_TMUX_SESSIONS[@]}"
            return 0
            ;;
        list-panes)
            local session_name=""
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    -t)
                        session_name="$2"
                        shift 2
                        ;;
                    *)
                        shift
                        ;;
                esac
            done
            printf '%s\n' "${_MOCK_TMUX_PANES[$session_name]:-}"
            return 0
            ;;
        kill-session)
            if [[ "${1:-}" == "-t" && -n "${2:-}" ]]; then
                _MOCK_TMUX_KILLED+=("$2")
            fi
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

test_case "tmux_cleanup_kill_sessions_in_scope kills every matching session"
_scope_dir="/tmp/workhive-cleanup-scope-general-$$"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("wh@test@feature" "manual-session" "out-of-scope")
_MOCK_TMUX_PANES["wh@test@feature"]="${_scope_dir}"
_MOCK_TMUX_PANES["manual-session"]="${_scope_dir}/nested"
_MOCK_TMUX_PANES["out-of-scope"]="/elsewhere"
tmux_cleanup_kill_sessions_in_scope "$_scope_dir"
_killed_output=$(_mock_tmux_killed_output)
assert_contains "$_killed_output" "wh@test@feature" "matching workhive session should be killed"
assert_contains "$_killed_output" "manual-session" "matching foreign session should be killed by general helper"
assert_not_contains "$_killed_output" "out-of-scope" "out-of-scope session should not be killed"

test_case "tmux_cleanup_kill_workhive_sessions_in_scope filters non-workhive sessions"
_scope_dir="/tmp/workhive-cleanup-scope-workhive-$$"
_scope_project=$(basename "$_scope_dir")
_mock_tmux_cleanup_reset
_workhive_session="wh@abcd@${_scope_project}@feature"
_foreign_workhive_session="wh@abcd@other-project@feature"
_MOCK_TMUX_SESSIONS=("${_workhive_session}" "manual-session" "${_foreign_workhive_session}")
_MOCK_TMUX_PANES["${_workhive_session}"]="${_scope_dir}"
_MOCK_TMUX_PANES["manual-session"]="${_scope_dir}"
_MOCK_TMUX_PANES["${_foreign_workhive_session}"]="${_scope_dir}"
tmux_cleanup_kill_workhive_sessions_in_scope "$_scope_dir"
_killed_output=$(_mock_tmux_killed_output)
assert_contains "$_killed_output" "${_workhive_session}" "workhive session in matching project scope should be killed"
assert_not_contains "$_killed_output" "manual-session" "non-workhive session in scope must be preserved"
assert_not_contains "$_killed_output" "${_foreign_workhive_session}" "workhive session from another project must be preserved"

# ──────────────────────────────────────────────────
# _tmux_cleanup_normalize_scope
# ──────────────────────────────────────────────────

test_case "_tmux_cleanup_normalize_scope strips trailing slash"
result=$(_tmux_cleanup_normalize_scope "/tmp/test/")
assert_eq "/tmp/test" "$result"

test_case "_tmux_cleanup_normalize_scope returns / for empty"
result=$(_tmux_cleanup_normalize_scope "")
assert_eq "/" "$result"

test_case "_tmux_cleanup_normalize_scope preserves valid path"
result=$(_tmux_cleanup_normalize_scope "/tmp/mypath")
assert_eq "/tmp/mypath" "$result"

# ──────────────────────────────────────────────────
# _tmux_cleanup_sanitize_project_name
# ──────────────────────────────────────────────────

test_case "_tmux_cleanup_sanitize_project_name replaces special chars"
result=$(_tmux_cleanup_sanitize_project_name "my.project:v2")
assert_eq "my-project-v2" "$result"

test_case "_tmux_cleanup_sanitize_project_name handles metacharacters"
result=$(_tmux_cleanup_sanitize_project_name 'test$`!#~^*?[]')
assert_not_contains "$result" '$'
assert_not_contains "$result" '`'
assert_not_contains "$result" '!'
assert_not_contains "$result" '#'
assert_not_contains "$result" '*'
assert_not_contains "$result" '?'

test_case "_tmux_cleanup_sanitize_project_name preserves plain names"
result=$(_tmux_cleanup_sanitize_project_name "my-project")
assert_eq "my-project" "$result"

# ──────────────────────────────────────────────────
# Edge cases: empty scope, no sessions
# ──────────────────────────────────────────────────

test_case "tmux_cleanup_kill_sessions_in_scope returns 0 for empty scope"
_mock_tmux_cleanup_reset
tmux_cleanup_kill_sessions_in_scope ""
assert_eq "0" "$?" "empty scope should succeed silently"

test_case "tmux_cleanup_kill_sessions_in_scope handles no matching sessions"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("unrelated-session")
_MOCK_TMUX_PANES["unrelated-session"]="/somewhere/else"
tmux_cleanup_kill_sessions_in_scope "/tmp/no-match-$$"
_killed_output=$(_mock_tmux_killed_output)
assert_empty "$_killed_output" "no sessions should be killed when none match"

test_case "tmux_cleanup_kill_workhive_sessions_in_scope returns 0 for empty scope"
_mock_tmux_cleanup_reset
tmux_cleanup_kill_workhive_sessions_in_scope ""
assert_eq "0" "$?" "empty scope should succeed silently"

test_case "_tmux_cleanup_session_matches_scope returns 0 for matching scope"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("test-match")
_MOCK_TMUX_PANES["test-match"]="/tmp/scope-match-test"
_tmux_cleanup_session_matches_scope "test-match" "/tmp/scope-match-test"
assert_success $?

test_case "_tmux_cleanup_session_matches_scope returns 1 for non-matching scope"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("test-no-match")
_MOCK_TMUX_PANES["test-no-match"]="/other/path"
_tmux_cleanup_session_matches_scope "test-no-match" "/tmp/scope-match-test"
assert_failure $?

test_case "_tmux_cleanup_session_matches_scope matches nested paths"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("test-nested")
_MOCK_TMUX_PANES["test-nested"]="/tmp/scope-match-test/sub/path"
_tmux_cleanup_session_matches_scope "test-nested" "/tmp/scope-match-test"
assert_success $?

test_case "tmux_cleanup_kill_sessions_in_scope kills nested-path sessions"
_scope_dir="/tmp/workhive-cleanup-nested-$$"
_mock_tmux_cleanup_reset
_MOCK_TMUX_SESSIONS=("nested-session")
_MOCK_TMUX_PANES["nested-session"]="${_scope_dir}/deep/nested/path"
tmux_cleanup_kill_sessions_in_scope "$_scope_dir"
_killed_output=$(_mock_tmux_killed_output)
assert_contains "$_killed_output" "nested-session" "nested path sessions should be killed"

test_report

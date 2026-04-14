#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Tests: Slash Branch Name Deduplication
# Ensures branches with slashes (e.g. "one/demo01") do not
# produce duplicate entries in ls/status output.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
WORKHIVE_BIN="$BIN_DIR/workhive"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"

BLUE='\033[0;34m'
NC='\033[0m'

scenario() { echo -e "\n${BLUE}  Scenario: $1${NC}"; }
given()    { echo -e "    Given $1"; }
when()     { echo -e "    When $1"; }

TEST_DIR="${TMPDIR:-/tmp}/workhive-bdd-slash-$$"
rm -rf "$TEST_DIR" 2>/dev/null || true
mkdir -p "$TEST_DIR"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"

TEST_REPO_DIR="$TEST_DIR/test-repo"
mkdir -p "$TEST_REPO_DIR"

cleanup() {
    tmux_cleanup_kill_sessions_in_scope "$TEST_DIR"
    _cleanup_wh_test_sessions
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

_tmux_usable() {
    command -v tmux &>/dev/null || return 1
    local probe="WH-TEST@slash-probe@$$"
    tmux new-session -d -s "$probe" 2>/dev/null || return 1
    tmux kill-session -t "$probe" 2>/dev/null || true
    return 0
}

_cleanup_wh_test_sessions() {
    command -v tmux &>/dev/null || return 0
    tmux list-sessions -F '#{session_name}' 2>/dev/null | grep '^WH-TEST@' | while IFS= read -r sn; do
        [[ -z "$sn" ]] && continue
        tmux kill-session -t "$sn" 2>/dev/null || true
    done || true
}

if ! _tmux_usable; then
    echo "tmux not found, skipping slash branch tests"
    exit 0
fi

# ════════════════════════════════════════════════
# Setup: init a test repo
# ════════════════════════════════════════════════

cd "$TEST_REPO_DIR" || exit 1
git init -q
git commit --allow-empty -m "init" -q
$WORKHIVE_BIN init --non-interactive --force >/dev/null 2>&1

# ════════════════════════════════════════════════
# Feature: Slash branch names produce single entry
# ════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Slash Branch Deduplication${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_suite "BDD: workhive open with slash branch"

scenario "Open a branch with slash creates single entry"
given "workhive is initialized"
when "user creates worktree and session for feature/test-slash"

# Create worktree + session manually (open requires interactive TTY for confirmation)
(
    cd "$TEST_REPO_DIR" || exit 1
    export LIB_DIR="$ROOT_DIR/lib"
    export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/utils/i18n.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/config/loader.sh"
    source "$LIB_DIR/domain/services/worktree_service.sh"
    source "$LIB_DIR/domain/services/session_service.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    terminal_adapter_init >/dev/null 2>&1
    worktree_create "feature/test-slash" >/dev/null 2>&1
    wt_path=$(worktree_get_path "feature/test-slash")
    session_name=$(session_get_name "feature/test-slash")
    session_create "feature/test-slash" "$wt_path" >/dev/null 2>&1
    echo "$session_name"
) > "$TEST_DIR/_session_name" 2>/dev/null

test_case "THEN workhive ls shows exactly 1 entry for the slash branch"
ls_output=$($WORKHIVE_BIN ls --format simple 2>/dev/null)
slash_count=$(echo "$ls_output" | grep -c "feature" || true)
assert_eq "1" "$slash_count" "should have exactly 1 entry containing 'feature'"

test_case "THEN workhive ls shows real branch name (feature/test-slash)"
ls_table=$($WORKHIVE_BIN ls 2>&1)
assert_contains "$ls_table" "feature/test-slash"

test_case "THEN workhive ls branch column does NOT show sanitized name"
# The path column will naturally contain the sanitized dir name, so check simple format (branch only)
assert_not_contains "$ls_output" "feature-test-slash"

test_case "THEN workhive list shows exactly 1 entry for the slash branch (table mode)"
list_output=$($WORKHIVE_BIN list --format table 2>/dev/null)
list_count=$(echo "$list_output" | grep -c "feature" || true)
assert_eq "1" "$list_count" "should have exactly 1 entry containing 'feature'"

test_case "THEN workhive list shows real branch name"
list_table=$($WORKHIVE_BIN list 2>&1)
assert_contains "$list_table" "feature/test-slash"

test_case "THEN workhive list does NOT show sanitized name as separate entry"
assert_not_contains "$list_table" "feature-test-slash"

# ════════════════════════════════════════════════
# Feature: No ERROR logs for sessions without tmux session
# ════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: No Spurious ERROR Logs${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_suite "BDD: no ERROR logs in ls/list for missing sessions"

scenario "list command does not produce ERROR for branches without sessions"
when "user runs workhive ls"

test_case "THEN ls output does not contain ERROR"
ls_full=$($WORKHIVE_BIN ls 2>&1)
assert_not_contains "$ls_full" "ERROR"

test_case "THEN list output does not contain ERROR"
list_full=$($WORKHIVE_BIN list 2>&1)
assert_not_contains "$list_full" "ERROR"

# Cleanup the slash branch session and worktree
$WORKHIVE_BIN remove "feature/test-slash" --force --no-confirm >/dev/null 2>&1 || true

test_report

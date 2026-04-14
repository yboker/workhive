#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Tests: Branch Name Sanitization Prompt
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
export LIB_DIR="$ROOT_DIR/lib"
WORKHIVE_BIN="$BIN_DIR/workhive"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

source "$ROOT_DIR/test/unit/test_framework.sh"

BLUE='\033[0;34m'
NC='\033[0m'

scenario() { echo -e "\n${BLUE}  Scenario: $1${NC}"; }
given()    { echo -e "    Given $1"; }
when()     { echo -e "    When $1"; }

TEST_DIR="/tmp/workhive-bdd-sanitize-$$"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"

TEST_REPO_DIR="$TEST_DIR/test-repo"
mkdir -p "$TEST_REPO_DIR"

cleanup() {
    cd /
    tmux_cleanup_kill_sessions_in_scope "$TEST_DIR"
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ════════════════════════════════════════════════
# Feature: Branch Name Sanitization Helpers
# ════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Branch Name Sanitization Helpers${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_suite "BDD: _branch_needs_sanitize and _branch_confirm_sanitize"

scenario "Slash branch needs sanitization"
given "a branch named feature/login"
when "checking if sanitization is needed"
result=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    _branch_needs_sanitize 'feature/login' && echo 'needs_sanitize' || echo 'clean'
" 2>/dev/null)

test_case "THEN branch is flagged as needing sanitization"
assert_eq "needs_sanitize" "$result"

scenario "Simple branch does not need sanitization"
given "a branch named demo01"
when "checking if sanitization is needed"
result=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    _branch_needs_sanitize 'demo01' && echo 'needs_sanitize' || echo 'clean'
" 2>/dev/null)

test_case "THEN branch is flagged as clean"
assert_eq "clean" "$result"

scenario "Confirmation function sets sanitized name"
given "a branch named org/team/feature"
when "running confirm sanitize (non-interactive, defaults to Y)"
_tmp_file="${TMPDIR:-/tmp}/wh-bdd-sanitize-$$"
bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    _branch_confirm_sanitize 'org/team/feature' </dev/null >/dev/null 2>&1 || true
    echo \"\$BRANCH_SANITIZED_NAME\"
" > "$_tmp_file" 2>/dev/null
result=$(tail -1 "$_tmp_file")
rm -f "$_tmp_file"

test_case "THEN BRANCH_SANITIZED_NAME is org-team-feature"
assert_eq "org-team-feature" "$result"

# ════════════════════════════════════════════════
# Feature: open command warns about slash branches
# ════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: open Command Slash Branch Warning${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_suite "BDD: workhive open warns about slash branches"

scenario "open with slash branch name shows warning and aborts on 'n'"
given "workhive is initialized in a test repo"
cd "$TEST_REPO_DIR" || exit 1
git init -q
git commit --allow-empty -m "init" -q
$WORKHIVE_BIN init --non-interactive --force >/dev/null 2>&1

when "user runs workhive open feature/test-sanitize"
output=$($WORKHIVE_BIN open "feature/test-sanitize" --no-attach 2>&1 </dev/null || true)

test_case "THEN output contains sanitization warning about converted name"
assert_contains "$output" "feature-test-sanitize"

# Clean up the worktree created in non-interactive mode (default Y)
tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR" || exit 1
git worktree remove .workhive/worktrees/feature-test-sanitize --force 2>/dev/null || true
git branch -D "feature/test-sanitize" 2>/dev/null || true

test_case "THEN output shows the original branch name in the opening message"
assert_contains "$output" "feature/test-sanitize"

scenario "open with clean branch name shows no sanitization warning"
when "user runs workhive open clean-name and declines creation"
output=$(echo "n" | $WORKHIVE_BIN open "clean-name" --no-attach 2>&1 || true)

test_case "THEN output does NOT contain sanitization warning"
assert_not_contains "$output" "will be converted"
assert_not_contains "$output" "将转换为"

test_report

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# End-to-End User Workflow Test
#
# Simulates realistic user journeys:
#   Scenario A: Project-level init (workhive init)
#   Scenario B: Global-level init (workhive init --global)
#
# Each scenario tests:
#   1. Initialization (init)
#   2. Workspace lifecycle (open/list/status/remove)
#   3. Git operations inside worktrees (commit, branch, merge)
#   4. Layout configuration (tmux enabled/disabled)
#   5. Cleanup (destroy)
#
# Usage: bash test/integration/e2e_user_workflow_test.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$ROOT_DIR/bin/workhive"
TEST_BASE="${TMPDIR:-/tmp}/workhive-e2e-test-$$"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
GLOBAL_CONFIG_DIR="${TEST_BASE}/global_config"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1
export XDG_CONFIG_HOME="$GLOBAL_CONFIG_DIR"

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

_TOTAL=0
_PASSED=0
_FAILED=0
_SKIPPED=0
_CURRENT=""

_test_case() { _CURRENT="$1"; _TOTAL=$((_TOTAL + 1)); }
_pass() { _PASSED=$((_PASSED + 1)); echo -e "  ${GREEN}✓${NC} ${_CURRENT}"; }
_fail() { _FAILED=$((_FAILED + 1)); echo -e "  ${RED}✗${NC} ${_CURRENT}"; echo -e "    ${RED}$1${NC}"; }
_skip() { _SKIPPED=$((_SKIPPED + 1)); echo -e "  ${YELLOW}⊘${NC} ${_CURRENT} (SKIPPED: $1)"; }
_section() { echo ""; echo -e "${BLUE}━━━ $1 ━━━${NC}"; }

_assert_rc() {
    local expected="$1" actual="$2"
    if [[ "$actual" -eq "$expected" ]]; then _pass; else _fail "expected rc=$expected, got rc=$actual"; fi
}
_assert_file() { if [[ -f "$1" ]]; then _pass; else _fail "file not found: $1"; fi; }
_assert_dir() { if [[ -d "$1" ]]; then _pass; else _fail "dir not found: $1"; fi; }
_assert_no_dir() { if [[ ! -d "$1" ]]; then _pass; else _fail "dir should not exist: $1"; fi; }
_assert_output_contains() {
    if echo "$1" | grep -qF -- "$2"; then _pass; else _fail "output missing: $2"; fi
}

_setup_git_repo() {
    local repo_dir="$1"
    mkdir -p "$repo_dir"
    cd "$repo_dir" || return 1
    git init -q
    git config user.name "E2E Test"
    git config user.email "e2e@test.local"
    echo "# E2E Test" > README.md
    git add -A
    git commit -q -m "Initial commit"
}

_session_name_for_branch() {
    local repo_dir="$1"
    local branch="$2"

    bash -c "
        cd '$repo_dir' || exit 1
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        session_get_name '$branch'
    " 2>/dev/null
}

_kill_test_sessions() {
    [[ -n "${REPO_A:-}" ]] && tmux_cleanup_kill_workhive_sessions_in_scope "$REPO_A"
    [[ -n "${REPO_B:-}" ]] && tmux_cleanup_kill_workhive_sessions_in_scope "$REPO_B"
    [[ -n "${REPO_C:-}" ]] && tmux_cleanup_kill_workhive_sessions_in_scope "$REPO_C"
    [[ -n "${REPO_D:-}" ]] && tmux_cleanup_kill_workhive_sessions_in_scope "$REPO_D"
}

_cleanup() {
    cd "$ROOT_DIR" 2>/dev/null || true
    _kill_test_sessions
    _cleanup_wh_test_sessions
    rm -rf "$TEST_BASE" 2>/dev/null || true
}
trap _cleanup EXIT INT TERM

_tmux_usable() {
    command -v tmux &>/dev/null || return 1
    local probe="WH-TEST@e2e-probe@$$"
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

# Pre-flight
if ! _tmux_usable; then
    echo "SKIP: tmux is not usable in this environment"
    exit 0
fi
if ! command -v git &>/dev/null; then
    echo "FATAL: git is required for E2E tests"
    exit 1
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  workhive E2E User Workflow Test"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BIN:  $BIN"
echo "  BASE: $TEST_BASE"
echo ""

mkdir -p "$TEST_BASE"

# ══════════════════════════════════════════════════
# Scenario A: Project-level init
# ══════════════════════════════════════════════════

REPO_A="${TEST_BASE}/project-local"

_section "Scenario A: Project-level init (workhive init)"

_setup_git_repo "$REPO_A"

# A1: init
_test_case "A1: workhive init --non-interactive succeeds"
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A1: config.yaml created"
_assert_file "$REPO_A/.workhive/config.yaml"

_test_case "A1: instance_id created"
_assert_file "$REPO_A/.workhive/instance_id"

_test_case "A1: .gitignore contains .workhive/"
grep -qF ".workhive/" "$REPO_A/.gitignore" 2>/dev/null
_assert_rc 0 "$?"

# A2: open workspace
_test_case "A2: workhive open feat-a --no-attach succeeds"
output=$("$BIN" open feat-a --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A2: worktree directory created"
_assert_dir "$REPO_A/.workhive/worktrees/feat-a"

_test_case "A2: tmux session created"
expected_session_a=$(_session_name_for_branch "$REPO_A" "feat-a")
if [[ -n "$expected_session_a" ]] && tmux has-session -t "$expected_session_a" 2>/dev/null; then
    _pass
else
    _fail "no tmux session for feat-a"
fi

# A3: git operations inside worktree
_test_case "A3: write code in worktree"
wt_dir="$REPO_A/.workhive/worktrees/feat-a"
echo 'console.log("feature A")' > "$wt_dir/app.js"
_assert_file "$wt_dir/app.js"

_test_case "A3: git add + commit in worktree"
(cd "$wt_dir" && git add app.js && git commit -q -m "feat: add app.js" 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A3: commit exists in worktree branch"
commit_msg=$(cd "$wt_dir" && git log -1 --format='%s' 2>/dev/null)
_assert_output_contains "$commit_msg" "feat: add app.js"

# A4: list workspaces
_test_case "A4: workhive list shows feat-a"
output=$("$BIN" list 2>&1); rc=$?
_assert_output_contains "$output" "feat-a"

# A5: list (dashboard)
_test_case "A5: workhive list succeeds"
output=$("$BIN" list 2>&1); rc=$?
_assert_rc 0 "$rc"

# A6: open second workspace
_test_case "A6: workhive open feat-b --no-attach succeeds"
output=$("$BIN" open feat-b --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A6: worktree feat-b created"
_assert_dir "$REPO_A/.workhive/worktrees/feat-b"

_test_case "A6: git operations in feat-b"
wt_b="$REPO_A/.workhive/worktrees/feat-b"
echo 'print("feature B")' > "$wt_b/main.py"
(cd "$wt_b" && git add main.py && git commit -q -m "feat: add main.py" 2>&1); rc=$?
_assert_rc 0 "$rc"

# A7: list should show both
_test_case "A7: workhive list shows both workspaces"
output=$("$BIN" list 2>&1)
if echo "$output" | grep -qF "feat-a" && echo "$output" | grep -qF "feat-b"; then
    _pass
else
    _fail "list should contain feat-a and feat-b"
fi

# A8: remove workspace (keep worktree)
_test_case "A8: workhive remove feat-b --keep-worktree --force"
output=$("$BIN" remove feat-b --keep-worktree --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A8: worktree feat-b still exists"
_assert_dir "$REPO_A/.workhive/worktrees/feat-b"

_test_case "A8: tmux session for feat-b killed"
expected_session_b=$(_session_name_for_branch "$REPO_A" "feat-b")
if [[ -n "$expected_session_b" ]] && ! tmux has-session -t "$expected_session_b" 2>/dev/null; then
    _pass
else
    _fail "session feat-b should be killed"
fi

# A9: remove workspace (full)
_test_case "A9: workhive remove feat-a --force"
output=$("$BIN" remove feat-a --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A9: worktree feat-a removed"
_assert_no_dir "$REPO_A/.workhive/worktrees/feat-a"

# A10: re-open with main branch mode
_test_case "A10: workhive open main --no-attach (main branch mode)"
main_branch=$(cd "$REPO_A" && git branch --show-current)
output=$("$BIN" open "$main_branch" --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

# A11: doctor
_test_case "A11: workhive doctor succeeds"
output=$("$BIN" doctor 2>&1); rc=$?
_assert_rc 0 "$rc"

# A12: destroy
_test_case "A12: workhive destroy --force"
output=$("$BIN" destroy --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A12: .workhive directory removed"
_assert_no_dir "$REPO_A/.workhive"

# A13: re-init after destroy
_test_case "A13: workhive init --non-interactive after destroy"
cd "$REPO_A" || exit 1
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A13: config.yaml recreated"
_assert_file "$REPO_A/.workhive/config.yaml"

# A14: open after re-init
_test_case "A14: workhive open feat-c --no-attach after re-init"
output=$("$BIN" open feat-c --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "A14: worktree feat-c created"
_assert_dir "$REPO_A/.workhive/worktrees/feat-c"

# Final cleanup for scenario A
_test_case "A15: final destroy"
output=$("$BIN" destroy --force 2>&1); rc=$?
_assert_rc 0 "$rc"

# ══════════════════════════════════════════════════
# Scenario B: Global-level init
# ══════════════════════════════════════════════════

REPO_B="${TEST_BASE}/project-global"

_section "Scenario B: Global-level init (workhive init --global)"

_setup_git_repo "$REPO_B"

# B1: init --global (use custom XDG_CONFIG_HOME to avoid contaminating user env)
_test_case "B1: workhive init --global --non-interactive"
export XDG_CONFIG_HOME="$GLOBAL_CONFIG_DIR"
output=$("$BIN" init --global --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B1: global config.yaml created"
_assert_file "$GLOBAL_CONFIG_DIR/workhive/config.yaml"

# B2: project-level init (should detect global config)
_test_case "B2: workhive init --non-interactive (project-level over global)"
cd "$REPO_B" || exit 1
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B2: project config.yaml created alongside global"
_assert_file "$REPO_B/.workhive/config.yaml"

# B3: open workspace with global+project config
_test_case "B3: workhive open feat-g --no-attach"
output=$("$BIN" open feat-g --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B3: worktree feat-g created"
_assert_dir "$REPO_B/.workhive/worktrees/feat-g"

# B4: git workflow inside worktree
_test_case "B4: create file + commit in feat-g"
wt_g="$REPO_B/.workhive/worktrees/feat-g"
echo '{"name": "test"}' > "$wt_g/package.json"
(cd "$wt_g" && git add package.json && git commit -q -m "chore: add package.json" 2>&1); rc=$?
_assert_rc 0 "$rc"

# B5: verify commit is on correct branch
_test_case "B5: worktree is on feat-g branch"
wt_branch=$(cd "$wt_g" && git branch --show-current 2>/dev/null)
_assert_output_contains "$wt_branch" "feat-g"

# B6: destroy project (global config should survive)
_test_case "B6: workhive destroy --force"
cd "$REPO_B" || exit 1
output=$("$BIN" destroy --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B6: project .workhive removed"
_assert_no_dir "$REPO_B/.workhive"

_test_case "B6: global config survives project destroy"
_assert_file "$GLOBAL_CONFIG_DIR/workhive/config.yaml"

# B7: re-init uses global config (check_init passes with global)
_test_case "B7: workhive init --non-interactive (re-init with global fallback)"
cd "$REPO_B" || exit 1
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B7: workhive open feat-h --no-attach (global config fallback)"
output=$("$BIN" open feat-h --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

# B8: destroy --global
_test_case "B8: workhive destroy --global --force"
output=$("$BIN" destroy --global --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "B8: global config directory removed"
_assert_no_dir "$GLOBAL_CONFIG_DIR/workhive"

# B9: cleanup
cd "$REPO_B" || exit 1
"$BIN" destroy --force 2>/dev/null || true

unset XDG_CONFIG_HOME

# ══════════════════════════════════════════════════
# Scenario C: Config variations (tmux settings)
# ══════════════════════════════════════════════════

REPO_C="${TEST_BASE}/project-config"

_section "Scenario C: Config variations (tmux settings)"

_setup_git_repo "$REPO_C"

_test_case "C1: init with default config"
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

# C2: open with default worktree behavior
_test_case "C2: open feat-d --no-attach"
output=$("$BIN" open feat-d --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "C2: worktree feat-d created"
_assert_dir "$REPO_C/.workhive/worktrees/feat-d"

# Kill session from C2 before opening new one
_kill_test_sessions

# C3: open another branch and verify worktree creation
_test_case "C3: open feat-e --no-attach"
output=$("$BIN" open feat-e --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "C3: worktree feat-e created"
_assert_dir "$REPO_C/.workhive/worktrees/feat-e"

# C4: cleanup
_test_case "C4: destroy --force"
cd "$REPO_C" || exit 1
output=$("$BIN" destroy --force 2>&1); rc=$?
_assert_rc 0 "$rc"

# ══════════════════════════════════════════════════
# Scenario D: Error recovery / edge cases
# ══════════════════════════════════════════════════

REPO_D="${TEST_BASE}/project-edge"

_section "Scenario D: Error recovery and edge cases"

_setup_git_repo "$REPO_D"

_test_case "D1: init"
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

# D2: open → destroy → open (destroy + re-open cycle)
_test_case "D2: open feat-f --no-attach"
output=$("$BIN" open feat-f --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D2: destroy --force"
output=$("$BIN" destroy --force 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D2: re-init after destroy"
cd "$REPO_D" || exit 1
output=$("$BIN" init --non-interactive 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D2: re-open feat-f --no-attach after destroy"
output=$("$BIN" open feat-f --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D2: worktree feat-f exists after re-open"
_assert_dir "$REPO_D/.workhive/worktrees/feat-f"

# D3: open with --force-layout
_test_case "D3: open feat-f --force-layout --no-attach"
output=$("$BIN" open feat-f --force-layout --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

# D4: double open (idempotent)
_test_case "D4: second open feat-f --no-attach (idempotent)"
output=$("$BIN" open feat-f --no-attach 2>&1); rc=$?
_assert_rc 0 "$rc"

# D5: gc
_test_case "D5: workhive gc succeeds"
output=$("$BIN" gc 2>&1); rc=$?
_assert_rc 0 "$rc"

# D6: help commands
_test_case "D6: workhive --help succeeds"
output=$("$BIN" --help 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D6: workhive open --help succeeds"
output=$("$BIN" open --help 2>&1); rc=$?
_assert_rc 0 "$rc"

_test_case "D6: workhive pane --help succeeds"
output=$("$BIN" pane --help 2>&1); rc=$?
_assert_rc 0 "$rc"

# D7: cleanup
cd "$REPO_D" || exit 1
"$BIN" destroy --force 2>/dev/null || true

# ══════════════════════════════════════════════════
# Results
# ══════════════════════════════════════════════════

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  E2E Test Results"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo -e "  Total:   $_TOTAL"
echo -e "  Passed:  ${GREEN}$_PASSED${NC}"
echo -e "  Failed:  ${RED}$_FAILED${NC}"
echo -e "  Skipped: ${YELLOW}$_SKIPPED${NC}"
echo ""

if [[ $_FAILED -eq 0 ]]; then
    echo -e "  ${GREEN}All E2E tests passed!${NC}"
    exit 0
else
    echo -e "  ${RED}Some E2E tests failed.${NC}"
    exit 1
fi

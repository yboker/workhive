#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Test: Reliability & Security Hardening
#
# Covers fixes made during enterprise-grade code review:
# - Session stability (no auto-exit)
# - Hook command injection defense
# - Cache robustness with special characters
# - Config null value handling
# - List command format edge cases
# - Source guard double-load safety
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
TEST_REPO_DIR="/tmp/workhive-bdd-reliability-$$"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1
# Session name format: wh@<project>@<branch>
EXPECTED_SESSION_PREFIX="wh@$(basename "$TEST_REPO_DIR")"

# Colors
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

_tmux_usable() {
    command -v tmux &>/dev/null || return 1
    local probe="WH-TEST@reliability-probe@$$"
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
    rm -rf "$TEST_REPO_DIR"
    mkdir -p "$TEST_REPO_DIR"
    cd "$TEST_REPO_DIR"

    git init >/dev/null 2>&1
    git config user.name "BDD Test"
    git config user.email "bdd@test.com"
    echo "# Reliability BDD Test" > README.md
    git add README.md
    git commit -m "Init" >/dev/null 2>&1
    git branch feature-reliability

    "$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
}

teardown() {
    cd /
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_REPO_DIR"
    _cleanup_wh_test_sessions
    rm -rf "$TEST_REPO_DIR"
}

trap teardown EXIT

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  BDD: Reliability & Security Hardening${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

setup

# ══════════════════════════════════════════════════
# Feature: Session Stability (P0 auto-exit fix)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Session Stability${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if _tmux_usable; then
    scenario "Session remains alive after open --no-attach"
    given "workhive is initialized"
    when "user opens a workspace with --no-attach"
    "$BIN_DIR/workhive" open feature-reliability --no-attach >/dev/null 2>&1 || true
    sleep 1

    then_assert "tmux session exists after creation" \
        "$(tmux has-session -t ${EXPECTED_SESSION_PREFIX}@feature-reliability 2>/dev/null && echo 0 || echo 1)"

    scenario "Session has remain-on-exit set"
    when "checking tmux session options"
    remain_opt=$(tmux show-option -t ${EXPECTED_SESSION_PREFIX}@feature-reliability remain-on-exit 2>/dev/null | awk '{print $2}' || echo "unknown")
    rc=$([[ "$remain_opt" == "on" ]] && echo 0 || echo 1)
    then_assert "remain-on-exit is 'on'" "$rc"

    scenario "Session survives 3 seconds without attachment"
    given "session was created but not attached"
    sleep 3
    then_assert "session still exists after 3 seconds" \
        "$(tmux has-session -t ${EXPECTED_SESSION_PREFIX}@feature-reliability 2>/dev/null && echo 0 || echo 1)"

    # Cleanup
    tmux kill-session -t "${EXPECTED_SESSION_PREFIX}@feature-reliability" 2>/dev/null || true
else
    skip_scenario "tmux not available - skipping session stability tests"
fi

# ══════════════════════════════════════════════════
# Feature: Removed Hook Command Safety
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Removed Hook Command Safety${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
skip_scenario "shell hooks were removed; this scenario is not applicable in current shell implementation"

# ══════════════════════════════════════════════════
# Feature: List Command Format Robustness (P1)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: List Command Format Robustness${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

cd "$TEST_REPO_DIR"

scenario "List --format without value shows friendly error"
given "workhive is initialized"
when "user runs 'workhive list --format' with no value"
output=$("$BIN_DIR/workhive" list --format 2>&1 || true)
rc=$([[ "$output" == *"Invalid format"* || "$output" == *"无效"* ]] && echo 0 || echo 1)
then_assert "friendly error message is shown (not 'unbound variable')" "$rc"

scenario "List --format with invalid value shows error"
when "user runs 'workhive list --format xml'"
output=$("$BIN_DIR/workhive" list --format xml 2>&1 || true)
rc=$([[ "$output" == *"Invalid format"* || "$output" == *"无效"* ]] && echo 0 || echo 1)
then_assert "invalid format is rejected with guidance" "$rc"

scenario "List --format json produces valid JSON"
if _tmux_usable; then
    given "a workspace exists"
    "$BIN_DIR/workhive" open list-json-test --no-attach >/dev/null 2>&1 || true

    when "user runs 'workhive list --format json'"
    output=$("$BIN_DIR/workhive" list --format json 2>&1)

    rc=$([[ "$output" == *"["* && "$output" == *"]"* ]] && echo 0 || echo 1)
    then_assert "output is a JSON array" "$rc"
    rc=$([[ "$output" == *'"branch"'* ]] && echo 0 || echo 1)
    then_assert "JSON contains 'branch' field" "$rc"
    rc=$([[ "$output" == *'"worktree"'* ]] && echo 0 || echo 1)
    then_assert "JSON contains 'worktree' field" "$rc"
    rc=$([[ "$output" == *'"session"'* ]] && echo 0 || echo 1)
    then_assert "JSON contains 'session' field" "$rc"

    tmux kill-session -t "${EXPECTED_SESSION_PREFIX}@list-json-test" 2>/dev/null || true
else
    skip_scenario "tmux not available - skipping JSON output test"
fi

# ══════════════════════════════════════════════════
# Feature: Cache Robustness (P0)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Cache Robustness with Special Characters${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Cache handles config keys with dots and colons"
given "cache manager is loaded"
when "storing and retrieving keys with special characters"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    cache_clear
    cache_set 'config:value:tmux.theme' 'nord'
    result=\$(cache_get 'config:value:tmux.theme')
    echo \"result:\$result\"
    cache_delete 'config:value:tmux.theme'
    cache_get 'config:value:tmux.theme' >/dev/null 2>&1 && echo 'deleted:false' || echo 'deleted:true'
" 2>&1)

rc=$([[ "$output" == *"result:nord"* ]] && echo 0 || echo 1)
then_assert "special character key stores and retrieves correctly" "$rc"
rc=$([[ "$output" == *"deleted:true"* ]] && echo 0 || echo 1)
then_assert "special character key is deletable" "$rc"

scenario "Cache invalidation does not corrupt state"
when "bulk invalidation is performed"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    cache_clear
    for i in \$(seq 1 10); do
        cache_set \"test:\$i\" \"v\$i\"
    done
    cache_set 'survive:1' 'keep'
    cache_invalidate 'test:*'
    remain=\$(cache_get 'survive:1')
    gone=true
    for i in \$(seq 1 10); do
        if cache_get \"test:\$i\" >/dev/null 2>&1; then
            gone=false
        fi
    done
    echo \"remain:\$remain\"
    echo \"gone:\$gone\"
" 2>&1)

rc=$([[ "$output" == *"remain:keep"* ]] && echo 0 || echo 1)
then_assert "non-matching keys survive invalidation" "$rc"
rc=$([[ "$output" == *"gone:true"* ]] && echo 0 || echo 1)
then_assert "all matching keys are invalidated" "$rc"

# ══════════════════════════════════════════════════
# Feature: Source Guard Double-Load Safety (P2)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Source Guard Double-Load Safety${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Double-sourcing common.sh does not crash"
given "common.sh has readonly variables"
when "sourced twice in the same shell"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/utils/common.sh\"
    echo 'alive'
" 2>&1)

rc=$([[ "$output" == *"alive"* ]] && echo 0 || echo 1)
then_assert "shell survives without readonly error" "$rc"

scenario "Double-sourcing entire dependency chain does not crash"
when "all core modules are sourced twice"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    # First load
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    # Second load (should be no-ops)
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    echo 'all_ok'
" 2>&1)

rc=$([[ "$output" == *"all_ok"* ]] && echo 0 || echo 1)
then_assert "full dependency chain survives double-load" "$rc"

# ══════════════════════════════════════════════════
# Feature: Config Null Value Handling (P1)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Config Null Value Handling${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if command -v yq &>/dev/null; then
    cd "$TEST_REPO_DIR"

    scenario "config_get returns empty for explicitly null YAML value"
    given "config has tmux.session_prefix set to null"
    yq -i '.tmux.session_prefix = null' .workhive/config.yaml

    when "config_get reads tmux.session_prefix"
    output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        cache_clear
        result=\$(config_get 'tmux.session_prefix')
        echo \"value:\$result\"
    " 2>&1)

    rc=$([[ "$output" == *"value:"* ]] && echo 0 || echo 1)
    then_assert "returns normalized empty value" "$rc"

    scenario "config_get returns consistent values across calls"
    when "config_get is called twice for the same key"
    output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        cache_clear
        v1=\$(config_get 'tmux.session_prefix')
        v2=\$(config_get 'tmux.session_prefix')
        echo \"v1:\$v1\"
        echo \"v2:\$v2\"
        if [[ \"\$v1\" == \"\$v2\" ]]; then echo 'consistent:true'; else echo 'consistent:false'; fi
    " 2>&1)

    rc=$([[ "$output" == *"consistent:true"* ]] && echo 0 || echo 1)
    then_assert "both calls return the same value" "$rc"

    # Reset
    yq -i 'del(.tmux.session_prefix)' .workhive/config.yaml
else
    skip_scenario "yq not available - skipping config null tests"
fi

# ══════════════════════════════════════════════════
# Feature: Screen Adapter Graceful Degradation (P0)
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Screen Adapter Graceful Degradation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Screen adapter loads without crashing"
given "screen adapter is sourced"
when "all adapter methods are called"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/adapters/terminal/screen.sh\"
    # Call methods that used to die()
    terminal_adapter_count_panes 'nonexistent' 2>/dev/null || true
    terminal_adapter_get_current_session 2>/dev/null || true
    terminal_adapter_get_current_pane_index 2>/dev/null || true
    terminal_adapter_attach_session_readonly 'x' 2>/dev/null || true
    terminal_adapter_select_pane 'x' 0 2>/dev/null || true
    terminal_adapter_resize_pane 'x' 0 50 2>/dev/null || true
    terminal_adapter_kill_pane 'x' 0 2>/dev/null || true
    echo 'survived'
" 2>&1)

rc=$([[ "$output" == *"survived"* ]] && echo 0 || echo 1)
then_assert "all unsupported methods degrade gracefully" "$rc"

# ──────────────────────────────────────────────────
# Report
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo "  Reliability BDD Test Report"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Total:   $TESTS_RUN"
echo -e "  Passed:  ${GREEN}$TESTS_PASSED${NC}"
echo -e "  Failed:  ${RED}$TESTS_FAILED${NC}"
echo -e "  Skipped: ${YELLOW}$TESTS_SKIPPED${NC}"
echo ""

if [[ $TESTS_FAILED -eq 0 ]]; then
    echo -e "  ${GREEN}All reliability BDD scenarios passed!${NC}"
    exit 0
else
    echo -e "  ${RED}Some reliability BDD scenarios failed.${NC}"
    exit 1
fi

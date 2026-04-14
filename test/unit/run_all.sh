#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Run All Unit Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_TMUX_PREFIX="${WORKHIVE_TEST_TMUX_PREFIX:-WH-TEST}"

# Capture existing tmux sessions before tests to avoid killing user sessions
_SESSIONS_BEFORE=""
if command -v tmux &>/dev/null; then
    _SESSIONS_BEFORE=$(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
fi

# Cleanup test-leaked sessions on exit
_run_all_cleanup() {
    if ! command -v tmux &>/dev/null; then
        return 0
    fi
    local sessions_after
    sessions_after=$(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
    [[ -z "$sessions_after" ]] && return 0

    local leaked=0
    while IFS= read -r sn; do
        [[ -z "$sn" ]] && continue
        # Always purge test-owned sessions by dedicated prefix.
        if [[ "$sn" == "${TEST_TMUX_PREFIX}"@* ]]; then
            tmux kill-session -t "$sn" 2>/dev/null || true
            leaked=$((leaked + 1))
            continue
        fi
        # Legacy cleanup: `wh-pane-test-*` came from older pane unit tests and is always safe to purge.
        if [[ "$sn" == wh-pane-test-* ]]; then
            tmux kill-session -t "$sn" 2>/dev/null || true
            leaked=$((leaked + 1))
            continue
        fi
        # Skip sessions that existed before tests
        if echo "$_SESSIONS_BEFORE" | grep -qxF "$sn"; then
            continue
        fi
        # Kill workhive test sessions (wh@ prefix or known test prefixes)
        if [[ "$sn" == wh@* || "$sn" == whst*@* || "$sn" == whbst*@* ]]; then
            tmux kill-session -t "$sn" 2>/dev/null || true
            leaked=$((leaked + 1))
        fi
    done <<< "$sessions_after"
    if [[ $leaked -gt 0 ]]; then
        echo ""
        echo -e "  ${BLUE}[cleanup]${NC} Killed $leaked leaked test session(s)"
    fi
}
trap _run_all_cleanup EXIT

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

export WORKHIVE_SKIP_GLOBAL_CHECK=1
export WORKHIVE_LANG=en
export WORKHIVE_TEST_TMUX_PREFIX="$TEST_TMUX_PREFIX"
export WORKHIVE_TEST_LOG_DIR="/tmp/workhive-test-logs/run-$$-$(date +%s)"
mkdir -p "$WORKHIVE_TEST_LOG_DIR" 2>/dev/null || true

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     workhive Unit Test Runner             ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════╝${NC}"
echo -e "${BLUE}Test logs:${NC} $WORKHIVE_TEST_LOG_DIR"

TOTAL_SUITES=0
PASSED_SUITES=0
FAILED_SUITES=0
FAILED_NAMES=()

# Run tests in dependency order
TEST_FILES=(
    "test_common.sh"
    "test_i18n.sh"
    "test_cache.sh"
    "test_platform.sh"
    "test_config.sh"
    "test_adapter.sh"
    "test_worktree.sh"
    "test_session.sh"
    "test_layout.sh"
    "test_worktree_collector.sh"
    "test_json_escape.sh"
    "test_source_guards.sh"
    "test_commands.sh"
    "test_list.sh"
    "test_new_commands.sh"
    "test_new_features.sh"
    "test_confirmation_noninteractive.sh"
    "test_version.sh"
    "test_worktree_safety.sh"
    "test_command_builder.sh"
    "test_upgrade.sh"
    "test_tmux_cleanup_scope.sh"
    "test_workspace_service.sh"
    "test_deps_manager.sh"
    "test_tmux_persistence.sh"
    "test_persistence.sh"
    "test_persistence_strategy.sh"
    "test_branch_sanitize.sh"
    "test_tool_adapter.sh"
    "test_tools_registry.sh"
)

for test_file in "${TEST_FILES[@]}"; do
    test_path="$SCRIPT_DIR/$test_file"

    if [[ ! -f "$test_path" ]]; then
        echo -e "${RED}[MISSING]${NC} $test_file"
        FAILED_SUITES=$((FAILED_SUITES + 1))
        FAILED_NAMES+=("$test_file")
        continue
    fi

    TOTAL_SUITES=$((TOTAL_SUITES + 1))

    echo ""
    echo -e "${BLUE}Running: $test_file${NC}"
    echo "──────────────────────────────────────────────"

    if bash "$test_path"; then
        PASSED_SUITES=$((PASSED_SUITES + 1))
    else
        FAILED_SUITES=$((FAILED_SUITES + 1))
        FAILED_NAMES+=("$test_file")
    fi
done

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║     Final Summary                            ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════╝${NC}"
echo ""
echo "  Test Suites:  $TOTAL_SUITES"
echo -e "  Passed:       ${GREEN}$PASSED_SUITES${NC}"
echo -e "  Failed:       ${RED}$FAILED_SUITES${NC}"

if [[ ${#FAILED_NAMES[@]} -gt 0 ]]; then
    echo ""
    echo -e "  ${RED}Failed suites:${NC}"
    for name in "${FAILED_NAMES[@]}"; do
        echo -e "    ${RED}✗${NC} $name"
    done
fi

echo ""

if [[ $FAILED_SUITES -eq 0 ]]; then
    echo -e "  ${GREEN}All test suites passed!${NC}"
    exit 0
else
    echo -e "  ${RED}Some test suites failed.${NC}"
    exit 1
fi

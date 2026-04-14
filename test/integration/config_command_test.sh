#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Config Command Integration Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
# shellcheck disable=SC2034
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
PASS=0
FAIL=0
TOTAL=0

# Test directory
TEST_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT="$TEST_DIR/../.."

# Temporary test directory
TEMP_TEST_DIR="/tmp/workhive-config-test-$$"
# shellcheck disable=SC2034
CONFIG_FILE="$TEMP_TEST_DIR/.workhive/config.yaml"

# Cleanup function
cleanup() {
    rm -rf "$TEMP_TEST_DIR"
}

# Force Chinese output
export WORKHIVE_LANG=zh

# Set up test environment
setup_test() {
    rm -rf "$TEMP_TEST_DIR"
    mkdir -p "$TEMP_TEST_DIR"
    cd "$TEMP_TEST_DIR" || return 1

    # Initialize git repository
    git init -q
    git config user.email "test@example.com"
    git config user.name "Test User"
    echo "test" > test.txt
    git add test.txt
    git commit -q -m "initial commit"
}

# Helper: assert output contains specified string
_assert_output_contains() {
    local test_name="$1"
    local output="$2"
    local expected="$3"

    ((TOTAL++))
    echo -n "Test $TOTAL: $test_name... "

    if echo "$output" | grep -q "$expected"; then
        echo -e "${GREEN}PASS${NC}"
        ((PASS++))
    else
        echo -e "${RED}FAIL${NC}"
        ((FAIL++))
    fi
}

_assert_output_not_contains() {
    local test_name="$1"
    local output="$2"
    local unexpected="$3"

    ((TOTAL++))
    echo -n "Test $TOTAL: $test_name... "

    if echo "$output" | grep -q "$unexpected"; then
        echo -e "${RED}FAIL${NC}"
        ((FAIL++))
    else
        echo -e "${GREEN}PASS${NC}"
        ((PASS++))
    fi
}

# Test functions
test_help() {
    local output
    output=$("$REPO_ROOT/bin/workhive" config --help 2>&1)
    _assert_output_contains "config --help should display help" "$output" "用法: workhive config"
}

test_edit_mode() {
    "$REPO_ROOT/bin/workhive" init --non-interactive --force >/dev/null 2>&1

    local output
    # shellcheck disable=SC2209
    output=$(EDITOR=cat "$REPO_ROOT/bin/workhive" config --edit 2>&1)
    _assert_output_contains "config --edit should open editor" "$output" "使用编辑器打开配置文件"
}

test_interactive_no_changes() {
    "$REPO_ROOT/bin/workhive" init --non-interactive --force >/dev/null 2>&1

    local output
    output=$(echo "" | "$REPO_ROOT/bin/workhive" config 2>&1)
    _assert_output_contains "config interactive mode with no changes" "$output" "配置修改完成"
}

test_config_exists_check() {
    local output
    output=$("$REPO_ROOT/bin/workhive" config 2>&1 || true)
    _assert_output_not_contains "config should work after init" "$output" "配置文件不存在"
}

test_config_print_summary() {
    "$REPO_ROOT/bin/workhive" init --non-interactive --force >/dev/null 2>&1

    local output
    output=$("$REPO_ROOT/bin/workhive" config 2>&1 || true)
    _assert_output_contains "config should print current config summary" "$output" "当前配置摘要"
}

test_config_with_editor_var() {
    local output
    output=$(EDITOR=cat VISUAL=cat "$REPO_ROOT/bin/workhive" config --edit 2>&1)
    _assert_output_contains "config --edit should use EDITOR env var" "$output" "编辑器打开配置文件"
}

test_invalid_option() {
    local output
    output=$("$REPO_ROOT/bin/workhive" config --invalid 2>&1 || true)
    _assert_output_contains "config with invalid option should error" "$output" "未知选项"
}

test_config_positional_arg() {
    local output
    output=$("$REPO_ROOT/bin/workhive" config somearg 2>&1 || true)
    _assert_output_contains "config with positional arg should error" "$output" "不接受位置参数"
}

test_config_init_deps_available() {
    ((TOTAL++))
    echo -n "Test $TOTAL: config_command.sh should have init_command.sh functions available... "

    local result
    result=$(bash -c '
        set -euo pipefail
        export LIB_DIR="'"$REPO_ROOT"'/lib"
        source "$LIB_DIR/utils/common.sh"
        source "$LIB_DIR/infra/cache/manager.sh"
        source "$LIB_DIR/infra/platform/detector.sh"
        source "$LIB_DIR/app/commands/config_command.sh"
        if type -t _init_build_available_commands &>/dev/null; then
            echo "OK"
        else
            echo "MISSING"
        fi
    ' 2>&1)

    if echo "$result" | grep -q "OK"; then
        echo -e "${GREEN}PASS${NC}"
        ((PASS++))
    else
        echo -e "${RED}FAIL${NC} (got: $result)"
        ((FAIL++))
    fi
}

test_config_no_command_not_found_error() {
    ((TOTAL++))
    echo -n "Test $TOTAL: config interactive should not produce 'command not found' errors... "

    "$REPO_ROOT/bin/workhive" init --non-interactive --force >/dev/null 2>&1

    local output
    output=$(echo "n" | "$REPO_ROOT/bin/workhive" config 2>&1 || true)

    if echo "$output" | grep -q "command not found"; then
        echo -e "${RED}FAIL${NC}"
        echo "  Error found: $(echo "$output" | grep 'command not found')"
        ((FAIL++))
    else
        echo -e "${GREEN}PASS${NC}"
        ((PASS++))
    fi
}

# Main test flow
main() {
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Config Command Integration Tests"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""

    trap cleanup EXIT

    # Set up test environment
    setup_test

    # Run tests
    test_help
    test_edit_mode
    test_interactive_no_changes
    test_config_exists_check
    test_config_print_summary
    test_config_with_editor_var
    test_invalid_option
    test_config_positional_arg
    test_config_init_deps_available
    test_config_no_command_not_found_error

    # Cleanup
    cleanup

    # Restore original directory
    cd "$REPO_ROOT" || exit 1

    # Print results
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Test Results: $PASS/$TOTAL passed"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    if [[ $FAIL -gt 0 ]]; then
        echo -e "${RED}FAILED: $FAIL tests failed${NC}"
        exit 1
    else
        echo -e "${GREEN}ALL TESTS PASSED${NC}"
        exit 0
    fi
}

main

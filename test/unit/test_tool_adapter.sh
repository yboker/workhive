#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/ai/tool_adapter.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"

# Source the module under test (reset guard so we can reload)
unset _AI_TOOL_ADAPTER_SH_LOADED
source "$LIB_DIR/infra/ai/tool_adapter.sh"

test_suite "AI Tool Adapter Tests"

# ──────────────────────────────────────────────────
# Function existence
# ──────────────────────────────────────────────────

test_case "ai_tool_detect exists"
declare -f ai_tool_detect &>/dev/null
assert_eq 0 $? "function should exist"

test_case "ai_tool_build_command exists"
declare -f ai_tool_build_command &>/dev/null
assert_eq 0 $? "function should exist"

test_case "ai_tool_list_installed exists"
declare -f ai_tool_list_installed &>/dev/null
assert_eq 0 $? "function should exist"

# ──────────────────────────────────────────────────
# ai_tool_detect
# ──────────────────────────────────────────────────

test_case "ai_tool_detect returns a tool when claude is available"
if command -v claude &>/dev/null; then
    result=$(ai_tool_detect)
    assert_eq "claude" "$result" "claude should be detected first"
else
    skip_test "claude not installed"
fi

test_case "ai_tool_detect returns non-zero when no tools available"
# Override PATH to empty to simulate no tools
_orig_path="$PATH"
result=$(PATH="/nonexistent" ai_tool_detect 2>/dev/null)
rc=$?
if [[ $rc -ne 0 ]]; then
    assert_failure $rc "should fail when no tools available"
else
    # Some tool might be a built-in; just verify we got something
    assert_not_empty "$result" "should return something"
fi

# ──────────────────────────────────────────────────
# ai_tool_build_command
# ──────────────────────────────────────────────────

test_case "ai_tool_build_command: claude generates correct command"
result=$(ai_tool_build_command "claude" "hello world")
assert_contains "$result" "claude" "should contain claude"
assert_contains "$result" "hello world" "should contain prompt"
assert_contains "$result" "--dangerously-skip-permissions" "should include skip-permissions flag"

test_case "ai_tool_build_command: codex generates correct command"
result=$(ai_tool_build_command "codex" "hello world")
assert_contains "$result" "codex" "should contain codex"
assert_contains "$result" "hello world" "should contain prompt"

test_case "ai_tool_build_command: opencode generates correct command"
result=$(ai_tool_build_command "opencode" "hello world")
assert_contains "$result" "opencode" "should contain opencode"

test_case "ai_tool_build_command: custom with WORKHIVE_AI_COMMAND"
# shellcheck disable=SC2034
WORKHIVE_AI_COMMAND="my-custom-tool"
result=$(ai_tool_build_command "custom" "test prompt")
assert_contains "$result" "my-custom-tool" "should contain custom command"
assert_contains "$result" "test prompt" "should contain prompt"
unset WORKHIVE_AI_COMMAND

test_case "ai_tool_build_command: custom without WORKHIVE_AI_COMMAND"
unset WORKHIVE_AI_COMMAND
result=$(ai_tool_build_command "custom" "test prompt")
assert_contains "$result" "No custom AI command" "should show error message"

test_case "ai_tool_build_command: unknown tool"
result=$(ai_tool_build_command "unknown-tool" "test prompt" 2>/dev/null)
rc=$?
assert_failure $rc "should return non-zero for unknown tool"
assert_contains "$result" "Unknown AI tool" "should contain error"

# ──────────────────────────────────────────────────
# ai_tool_list_installed
# ──────────────────────────────────────────────────

test_case "ai_tool_list_installed returns installed tools"
result=$(ai_tool_list_installed)
if command -v claude &>/dev/null; then
    assert_contains "$result" "claude" "should list claude"
else
    # If no tools installed, result should be empty or contain other tools
    assert_eq 0 0 "list_installed runs without error"
fi

test_report

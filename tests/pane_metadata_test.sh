#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/base.sh"

# ──────────────────────────────────────────────────
# In-memory pane option store for testing (no tmux needed)
# ──────────────────────────────────────────────────
declare -A _TEST_PANE_OPTIONS=()

terminal_adapter_set_pane_option() {
    local session_name="$1" pane_index="$2" option="$3" value="$4"
    _TEST_PANE_OPTIONS["${session_name}:${pane_index}:${option}"]="$value"
}

terminal_adapter_get_pane_option() {
    local session_name="$1" pane_index="$2" option="$3"
    local key="${session_name}:${pane_index}:${option}"
    printf '%s' "${_TEST_PANE_OPTIONS[$key]:-}"
}

terminal_adapter_unset_pane_option() {
    local session_name="$1" pane_index="$2" option="$3"
    local key="${session_name}:${pane_index}:${option}"
    unset '_TEST_PANE_OPTIONS[$key]' 2>/dev/null || true
}

source "$LIB_DIR/domain/services/pane_state_service.sh"

test_suite "pane declarative metadata API"

# ──────────────────────────────────────────────────
# Test: _pane_meta_classify_kind
# ──────────────────────────────────────────────────

test_case "classify_kind: empty command -> shell"
result=$(_pane_meta_classify_kind "")
assert_eq "shell" "$result" "empty command should be shell"

test_case "classify_kind: claude -> agent"
result=$(_pane_meta_classify_kind "claude --dangerously-skip-permissions")
assert_eq "agent" "$result" "claude should be agent"

test_case "classify_kind: codex -> agent"
result=$(_pane_meta_classify_kind "codex")
assert_eq "agent" "$result" "codex should be agent"

test_case "classify_kind: lazygit -> tool"
result=$(_pane_meta_classify_kind "lazygit")
assert_eq "tool" "$result" "lazygit should be tool"

test_case "classify_kind: vim -> tool"
result=$(_pane_meta_classify_kind "vim")
assert_eq "tool" "$result" "vim should be tool"

test_case "classify_kind: unknown command -> shell"
result=$(_pane_meta_classify_kind "my-custom-script")
assert_eq "shell" "$result" "unknown command should be shell"

# ──────────────────────────────────────────────────
# Test: pane_meta_write_identity + pane_meta_read_all
# ──────────────────────────────────────────────────

test_case "write_identity stores role/kind/desired_command/state"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
role=$(pane_meta_get "wh@test" "0" "role")
kind=$(pane_meta_get "wh@test" "0" "kind")
desired=$(pane_meta_get "wh@test" "0" "desired_command")
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "main" "$role" "role should be main"
assert_eq "agent" "$kind" "kind should be agent"
assert_eq "claude" "$desired" "desired_command should be claude"
assert_eq "pending" "$state" "initial state should be pending"

test_case "write_identity stores updated_at timestamp"
updated_at=$(pane_meta_get "wh@test" "0" "updated_at")
assert_not_empty "$updated_at" "updated_at should not be empty"

test_case "write_identity empty command stores empty desired_command"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "1" "col_1_pane_0" "shell" ""
desired=$(pane_meta_get "wh@test" "1" "desired_command")
assert_eq "" "$desired" "desired_command should be empty for shell pane"

# ──────────────────────────────────────────────────
# Test: pane_meta_read_all
# ──────────────────────────────────────────────────

test_case "read_all returns pipe-separated metadata"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "codex"
all=$(pane_meta_read_all "wh@test" "0")
assert_contains "$all" "main|agent|codex|pending|" "should contain identity fields"

# ──────────────────────────────────────────────────
# Test: pane_meta_transition_state
# ──────────────────────────────────────────────────

test_case "transition_state: pending -> launching"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
pane_meta_transition_state "wh@test" "0" "launching"
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "launching" "$state" "state should be launching"

test_case "transition_state: launching -> running"
pane_meta_transition_state "wh@test" "0" "running"
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "running" "$state" "state should be running"

test_case "transition_state: launching -> failed with error"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
pane_meta_transition_state "wh@test" "0" "failed" "command not found"
state=$(pane_meta_get "wh@test" "0" "state")
last_error=$(pane_meta_get "wh@test" "0" "last_error")
assert_eq "failed" "$state" "state should be failed"
assert_eq "command not found" "$last_error" "last_error should be set"

test_case "transition_state: invalid state rejected"
rc=0
pane_meta_transition_state "wh@test" "0" "invalid_state" 2>/dev/null || rc=$?
assert_ne 0 "$rc" "invalid state should return non-zero"

# ──────────────────────────────────────────────────
# Test: pane_meta_clear_all
# ──────────────────────────────────────────────────

test_case "clear_all removes all metadata fields"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
pane_meta_clear_all "wh@test" "0"
role=$(pane_meta_get "wh@test" "0" "role")
state=$(pane_meta_get "wh@test" "0" "state")
assert_eq "" "$role" "role should be empty after clear"
assert_eq "" "$state" "state should be empty after clear"

# ──────────────────────────────────────────────────
# Test: pane_meta_has_metadata
# ──────────────────────────────────────────────────

test_case "has_metadata returns true when metadata exists"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
rc=0
pane_meta_has_metadata "wh@test" "0" || rc=$?
assert_eq 0 "$rc" "should detect existing metadata"

test_case "has_metadata returns false when no metadata"
_TEST_PANE_OPTIONS=()
rc=0
pane_meta_has_metadata "wh@test" "0" || rc=$?
assert_ne 0 "$rc" "should return false for empty pane"

# ──────────────────────────────────────────────────
# Test: multiple panes isolation
# ──────────────────────────────────────────────────

test_case "metadata isolation between panes"
_TEST_PANE_OPTIONS=()
pane_meta_write_identity "wh@test" "0" "main" "agent" "claude"
pane_meta_write_identity "wh@test" "1" "col_1_pane_0" "shell" ""
pane_meta_write_identity "wh@test" "2" "col_1_pane_1" "tool" "lazygit"

role0=$(pane_meta_get "wh@test" "0" "role")
role1=$(pane_meta_get "wh@test" "1" "role")
role2=$(pane_meta_get "wh@test" "2" "role")
kind0=$(pane_meta_get "wh@test" "0" "kind")
kind1=$(pane_meta_get "wh@test" "1" "kind")
kind2=$(pane_meta_get "wh@test" "2" "kind")

assert_eq "main" "$role0" "pane 0 role"
assert_eq "col_1_pane_0" "$role1" "pane 1 role"
assert_eq "col_1_pane_1" "$role2" "pane 2 role"
assert_eq "agent" "$kind0" "pane 0 kind"
assert_eq "shell" "$kind1" "pane 1 kind"
assert_eq "tool" "$kind2" "pane 2 kind"

# ──────────────────────────────────────────────────
# E1: Production-path semantic classification tests
#
# These tests use production code paths (not stubs) to verify
# that configured AI tools are correctly classified.
# ──────────────────────────────────────────────────

test_case "classify_kind: hermes -> agent (production path)"
result=$(_pane_meta_classify_kind "hermes")
assert_eq "agent" "$result" "hermes must be classified as agent via production path"

test_case "classify_kind: hermes via tools_registry_is_ai_command"
rc=0
tools_registry_is_ai_command "hermes" 2>/dev/null || rc=$?
assert_eq 0 "$rc" "hermes must be recognized as AI command by production registry"

test_case "classify_kind: hermes via tools_registry_normalize_ai_command"
normalized=$(tools_registry_normalize_ai_command "hermes" 2>/dev/null || echo "")
assert_eq "hermes" "$normalized" "hermes must normalize to stable non-empty value"

test_case "classify_kind: opencode -> agent (production path)"
result=$(_pane_meta_classify_kind "opencode")
assert_eq "agent" "$result" "opencode must be classified as agent"

test_case "classify_kind: aider -> agent (production path)"
result=$(_pane_meta_classify_kind "aider")
assert_eq "agent" "$result" "aider must be classified as agent"

test_case "classify_kind: gemini -> agent (production path)"
result=$(_pane_meta_classify_kind "gemini")
assert_eq "agent" "$result" "gemini must be classified as agent"

test_case "classify_kind: goose -> agent (production path)"
result=$(_pane_meta_classify_kind "goose")
assert_eq "agent" "$result" "goose must be classified as agent"

test_case "classify_kind: cline -> agent (production path)"
result=$(_pane_meta_classify_kind "cline")
assert_eq "agent" "$result" "cline must be classified as agent"

# Boundary: verify tool commands are never misclassified as agent
test_case "classify_kind: yazi is tool, not agent"
result=$(_pane_meta_classify_kind "yazi")
assert_eq "tool" "$result" "yazi must remain tool, not agent"

test_case "classify_kind: htop is tool, not agent"
result=$(_pane_meta_classify_kind "htop")
assert_eq "tool" "$result" "htop must remain tool, not agent"

# Boundary: commands with arguments
test_case "classify_kind: hermes with args -> agent"
result=$(_pane_meta_classify_kind "hermes --some-flag")
assert_eq "agent" "$result" "hermes with arguments must still be agent"

test_case "classify_kind: full path hermes -> agent"
result=$(_pane_meta_classify_kind "/usr/local/bin/hermes")
assert_eq "agent" "$result" "full path hermes must still be agent"

# Boundary: codex platform variant binary
test_case "classify_kind: codex variant -> agent"
result=$(_pane_meta_classify_kind "codex-x86_64-apple-darwin")
assert_eq "agent" "$result" "codex platform variant must be agent"

test_report

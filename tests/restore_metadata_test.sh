#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/persistence/restore.sh"

test_suite "restore pane metadata"

declare -A _TEST_META=()

pane_meta_write_identity() {
    local session_name="$1"
    local pane_index="$2"
    local role="$3"
    local kind="$4"
    local desired_command="$5"
    _TEST_META["${session_name}:${pane_index}:role"]="$role"
    _TEST_META["${session_name}:${pane_index}:kind"]="$kind"
    _TEST_META["${session_name}:${pane_index}:desired_command"]="$desired_command"
    _TEST_META["${session_name}:${pane_index}:state"]="pending"
    _TEST_META["${session_name}:${pane_index}:last_error"]=""
    _TEST_META["${session_name}:${pane_index}:updated_at"]="now"
}

pane_meta_transition_state() {
    local session_name="$1"
    local pane_index="$2"
    local new_state="$3"
    _TEST_META["${session_name}:${pane_index}:state"]="$new_state"
}

pane_meta_set() {
    local session_name="$1"
    local pane_index="$2"
    local field="$3"
    local value="$4"
    _TEST_META["${session_name}:${pane_index}:${field}"]="$value"
}

_pane_meta_classify_kind() {
    local cmd="${1:-}"
    case "$cmd" in
        codex|claude|hermes*) echo "agent" ;;
        lazygit|vim|nvim) echo "tool" ;;
        *) echo "shell" ;;
    esac
}

test_case "snapshot metadata keeps failed state and error context"
_TEST_META=()
_SP_PANE_ROLE="main"
_SP_PANE_KIND="agent"
_SP_PANE_DESIRED_CMD="codex"
_SP_PANE_STATE="failed"
_SP_PANE_LAST_ERROR="startup timeout"
_SP_PANE_UPDATED_AT="2026-04-11T12:03:00+0800"
_SP_FULL_CMD='codex'
_SP_PANE_CMD='zsh'
_restore_pane_write_metadata "wh@test" "0" "false"
assert_eq "failed" "${_TEST_META["wh@test:0:state"]:-}" "failed state should be preserved"
assert_eq "startup timeout" "${_TEST_META["wh@test:0:last_error"]:-}" "last_error should be restored"
assert_eq "2026-04-11T12:03:00+0800" "${_TEST_META["wh@test:0:updated_at"]:-}" "updated_at should be restored"

test_case "legacy snapshot uses neutral pane_<idx> role"
_TEST_META=()
_SP_PANE_ROLE=""
_SP_PANE_KIND=""
_SP_PANE_DESIRED_CMD=""
_SP_PANE_STATE=""
_SP_PANE_LAST_ERROR=""
_SP_PANE_UPDATED_AT=""
_SP_FULL_CMD='zsh -lc codex'
_SP_PANE_CMD='zsh'
_restore_pane_write_metadata "wh@test" "2" "true"
assert_eq "pane_2" "${_TEST_META["wh@test:2:role"]:-}" "legacy role should be pane_<idx>"

test_report

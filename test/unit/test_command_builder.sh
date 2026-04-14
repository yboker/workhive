#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/app/shared/command_builder.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/app/shared/command_builder.sh"

test_suite "Command Builder Tests"

# ──────────────────────────────────────────────────
# _build_pane_command_list
# ──────────────────────────────────────────────────

test_case "_build_pane_command_list populates arrays"
_build_pane_command_list
assert_not_empty "${_AVAILABLE_CMD_LABELS[*]:-}" "labels should not be empty"
assert_not_empty "${_AVAILABLE_CMD_VALUES[*]:-}" "values should not be empty"

test_case "_build_pane_command_list always has empty shell option"
_build_pane_command_list
found_empty=false
for val in "${_AVAILABLE_CMD_VALUES[@]}"; do
    [[ "$val" == "__empty__" ]] && found_empty=true
done
assert_eq "true" "$found_empty" "should have __empty__ option"

test_case "_build_pane_command_list always has custom input option"
_build_pane_command_list
last_val="${_AVAILABLE_CMD_VALUES[${#_AVAILABLE_CMD_VALUES[@]}-1]}"
assert_eq "__custom__" "$last_val" "last value should be __custom__"

test_case "_build_pane_command_list labels and values have same count"
_build_pane_command_list
assert_eq "${#_AVAILABLE_CMD_LABELS[@]}" "${#_AVAILABLE_CMD_VALUES[@]}" "counts should match"

test_case "_build_pane_command_list sets _AVAILABLE_CMD_EMPTY_IDX"
_build_pane_command_list
# __empty__ is always the second-to-last entry
expected_idx=$(( ${#_AVAILABLE_CMD_VALUES[@]} - 2 ))
assert_eq "$expected_idx" "$_AVAILABLE_CMD_EMPTY_IDX" "empty idx should point to __empty__"
assert_eq "__empty__" "${_AVAILABLE_CMD_VALUES[$_AVAILABLE_CMD_EMPTY_IDX]}" "idx should resolve to __empty__"

test_case "_build_pane_command_list detects installed tools"
_build_pane_command_list
if command -v lazygit &>/dev/null; then
    found_lazygit=false
    for val in "${_AVAILABLE_CMD_VALUES[@]}"; do
        [[ "$val" == "lazygit" ]] && found_lazygit=true
    done
    assert_eq "true" "$found_lazygit" "lazygit should be in list when installed"
else
    skip_test "lazygit not installed"
fi

test_report

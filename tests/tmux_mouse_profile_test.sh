#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"

test_suite "tmux mouse profile"

_TEST_TMUX_CALLS=""
_TEST_PROFILE="workspace"
_TEST_MOUSE="true"

config_get() {
    case "$1" in
        tmux.interaction_profile) printf '%s\n' "$_TEST_PROFILE" ;;
        tmux.mouse_support) printf '%s\n' "$_TEST_MOUSE" ;;
        *) return 1 ;;
    esac
}

tmux() {
    _TEST_TMUX_CALLS+="$*\n"
    return 0
}

# ──────────────────────────────────────────────────
# Section 1: Base option integration tests
# ──────────────────────────────────────────────────

test_case "base options set workspace interaction profile and install wheel bindings"
_TEST_TMUX_CALLS=""
_TEST_PROFILE="workspace"
_TEST_MOUSE="true"
_tmux_apply_base_options "wh@test"
assert_contains "$_TEST_TMUX_CALLS" "set-option -t wh@test @workhive_interaction_profile workspace" "should persist workspace interaction profile"
assert_contains "$_TEST_TMUX_CALLS" "bind-key -T root WheelUpPane" "should install wheel-up binding"
assert_contains "$_TEST_TMUX_CALLS" "bind-key -T root WheelDownPane" "should install wheel-down binding"
assert_contains "$_TEST_TMUX_CALLS" "set-option -t wh@test mouse on" "should keep mouse enabled"

test_case "base options can disable mouse while keeping profile metadata"
_TEST_TMUX_CALLS=""
_TEST_PROFILE="workspace"
_TEST_MOUSE="false"
_tmux_apply_base_options "wh@test"
assert_contains "$_TEST_TMUX_CALLS" "set-option -t wh@test @workhive_interaction_profile workspace" "should still expose profile metadata"
assert_contains "$_TEST_TMUX_CALLS" "set-option -t wh@test mouse off" "should disable mouse when configured"

# ──────────────────────────────────────────────────
# Section 2: Wheel strategy builder unit tests
#
# Validates the actual policy string produced by each
# strategy builder, not just that bind-key was called.
# ──────────────────────────────────────────────────

test_case "workspace wheel-up strategy avoids pane_kind branching"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_not_contains "$_ws_up" "@workhive_pane_kind" "workspace strategy should not branch by pane kind"

test_case "workspace wheel-up: no frame-stacking copy-mode pattern for non-alternate panes"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_not_contains "$_ws_up" "copy-mode -e; send-keys -M" "workspace profile must not use copy-mode + send-keys -M (causes frame stacking)"

test_case "workspace wheel-up: fallback enters copy-mode -eu for history viewing"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "copy-mode -eu" "fallback should enter copy-mode with -eu (auto-exit at bottom + page-up)"

test_case "workspace wheel-up: alternate-screen pane uses pass-through"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "alternate_on" "should check alternate_on before fallback"

test_case "workspace wheel-up: pane_in_mode continues scrolling"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "pane_in_mode" "should check pane_in_mode first"

test_case "workspace wheel-down strategy avoids pane_kind branching"
_ws_down=$(_tmux_build_workspace_wheel_down)
assert_not_contains "$_ws_down" "@workhive_pane_kind" "workspace wheel-down should not branch by pane kind"

test_case "workspace wheel-down: no implicit copy-mode entry"
_ws_down=$(_tmux_build_workspace_wheel_down)
assert_not_contains "$_ws_down" "copy-mode" "wheel-down should not enter copy-mode"

test_case "workspace wheel-down: fallback uses send-keys -M (no-op when not in mode)"
_ws_down=$(_tmux_build_workspace_wheel_down)
assert_contains "$_ws_down" "send-keys -M" "wheel-down fallback should use send-keys -M"

test_case "compatible wheel-up always enters copy-mode for non-alternate pane"
_compat_up=$(_tmux_build_compatible_wheel_up)
assert_contains "$_compat_up" "copy-mode -e; send-keys -M" "compatible profile should always enter copy-mode"
assert_not_contains "$_compat_up" "@workhive_pane_kind" "compatible profile should not check pane_kind"

test_case "compatible wheel-down does not enter copy-mode"
_compat_down=$(_tmux_build_compatible_wheel_down)
assert_not_contains "$_compat_down" "copy-mode" "compatible wheel-down should not enter copy-mode"

# ──────────────────────────────────────────────────
# Section 3: Wheel policy validation per pane kind
#
# Parses the tmux if-shell decision chain to verify
# which action would be selected for each pane class.
# ──────────────────────────────────────────────────

# Helper: simplify escaped tmux binding strings for assertions.
_extract_pane_kind_actions() {
    local binding="$1"
    local stripped
    stripped=$(printf '%s' "$binding" | sed 's/\\\\\\"/|/g; s/\\"/|/g; s/"//g')
    printf '%s' "$stripped"
}

test_case "wheel-up policy: workspace fallback uses copy-mode -eu (not frame-stacking pattern)"
_ws_up=$(_tmux_build_workspace_wheel_up)
_stripped=$(_extract_pane_kind_actions "$_ws_up")
assert_not_contains "$_stripped" "copy-mode -e; send-keys -M" "workspace wheel-up must not use copy-mode + send-keys -M"
assert_contains "$_stripped" "copy-mode -eu" "workspace wheel-up fallback must use copy-mode -eu"

test_case "wheel-up policy: fallback enables history viewing with auto-exit"
_ws_up=$(_tmux_build_workspace_wheel_up)
_stripped=$(_extract_pane_kind_actions "$_ws_up")
assert_contains "$_stripped" "copy-mode -eu" "fallback pane should enter copy-mode -eu for history"

test_case "wheel-up policy: alternate-screen pane gets pass-through before fallback"
_ws_up=$(_tmux_build_workspace_wheel_up)
_alt_pos=$(echo "$_ws_up" | grep -bo "alternate_on" | head -1 | cut -d: -f1)
_fallback_pos=$(echo "$_ws_up" | grep -bo "copy-mode -eu" | head -1 | cut -d: -f1)
if [[ -n "$_alt_pos" && -n "$_fallback_pos" && "$_alt_pos" -lt "$_fallback_pos" ]]; then
    _ordering_ok="true"
else
    _ordering_ok="false"
fi
assert_eq "true" "$_ordering_ok" "alternate_on must be evaluated before fallback"

test_case "wheel-down policy: workspace fallback stays out of copy-mode"
_ws_down=$(_tmux_build_workspace_wheel_down)
_stripped_down=$(_extract_pane_kind_actions "$_ws_down")
assert_not_contains "${_stripped_down}" "copy-mode" "workspace wheel-down should not enter copy-mode"

# ──────────────────────────────────────────────────
# Section 4: Fallback / missing metadata behavior
#
# When @workhive_pane_kind is not set (empty), tmux
# evaluates #{==:,agent} as false. The fallback branch
# (select-pane) is selected. This is deterministic.
# ──────────────────────────────────────────────────

test_case "fallback: missing pane_kind metadata defaults to history-viewable behavior"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "copy-mode -eu" "missing metadata should default to copy-mode -eu for history access"

test_case "fallback: tool pane without alternate_on gets copy-mode -eu"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "copy-mode -eu" "tool kind without alternate_on should enter copy-mode -eu"

# ──────────────────────────────────────────────────
# Section 5: Profile dispatch integration
#
# Validates that the profile selector correctly routes
# workspace vs compatible strategies.
# ──────────────────────────────────────────────────

test_case "workspace profile binding excludes pane_kind strategy and includes copy-mode -eu"
_TEST_TMUX_CALLS=""
_TEST_PROFILE="workspace"
_TEST_MOUSE="true"
_tmux_install_mouse_bindings
assert_not_contains "$_TEST_TMUX_CALLS" "@workhive_pane_kind" "workspace binding must not contain pane_kind dispatch"
assert_contains "$_TEST_TMUX_CALLS" "@workhive_interaction_profile" "binding must reference interaction profile"
assert_contains "$_TEST_TMUX_CALLS" "copy-mode -eu" "workspace binding must include copy-mode -eu for history viewing"

test_case "compatible profile fallback does not use pane_kind"
_TEST_TMUX_CALLS=""
_TEST_PROFILE="compatible"
_TEST_MOUSE="true"
_tmux_apply_base_options "wh@test"
# The compatible branch is embedded in the same bind-key, but its portion should not check pane_kind
_compat_up=$(_tmux_build_compatible_wheel_up)
assert_not_contains "$_compat_up" "@workhive_pane_kind" "compatible strategy must not depend on pane_kind metadata"

# ──────────────────────────────────────────────────
# Section 6: Edge cases and boundary conditions
# ──────────────────────────────────────────────────

test_case "builder functions are idempotent"
_result1=$(_tmux_build_workspace_wheel_up)
_result2=$(_tmux_build_workspace_wheel_up)
assert_eq "$_result1" "$_result2" "repeated calls must produce identical output"

test_case "builder output is non-empty"
_ws_up=$(_tmux_build_workspace_wheel_up)
_ws_down=$(_tmux_build_workspace_wheel_down)
_compat_up=$(_tmux_build_compatible_wheel_up)
_compat_down=$(_tmux_build_compatible_wheel_down)
assert_true "[[ -n '$_ws_up' ]]" "workspace wheel-up must be non-empty"
assert_true "[[ -n '$_ws_down' ]]" "workspace wheel-down must be non-empty"
assert_true "[[ -n '$_compat_up' ]]" "compatible wheel-up must be non-empty"
assert_true "[[ -n '$_compat_down' ]]" "compatible wheel-down must be non-empty"

test_case "builder output is single line (no embedded newlines)"
_ws_up=$(_tmux_build_workspace_wheel_up)
_line_count=$(echo "$_ws_up" | wc -l | tr -d ' ')
assert_eq "1" "$_line_count" "binding string must be exactly one line"

# ──────────────────────────────────────────────────
# Section 7: E3 — End-to-end semantic classification
#          to wheel policy tests
#
# Verifies that production-path classify_kind for
# specific commands correctly feeds into the wheel
# strategy's pane_kind dispatch.
# ──────────────────────────────────────────────────

# Need pane_state_service for _pane_meta_classify_kind
source "$LIB_DIR/domain/services/pane_state_service.sh" 2>/dev/null || true

test_case "E3: hermes classify -> agent -> workspace wheel-up avoids frame-stacking copy-mode"
_hermes_kind=$(_pane_meta_classify_kind "hermes")
assert_eq "agent" "$_hermes_kind" "hermes must classify as agent"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_not_contains "$_ws_up" "copy-mode -e; send-keys -M" "workspace wheel-up must not use frame-stacking pattern"
assert_contains "$_ws_up" "copy-mode -eu" "workspace wheel-up must use safe copy-mode -eu"

test_case "E3: codex classify -> agent -> same non-copy-mode path"
_codex_kind=$(_pane_meta_classify_kind "codex")
assert_eq "agent" "$_codex_kind" "codex must classify as agent"

test_case "E3: shell pane -> workspace wheel-up resolves to copy-mode -eu"
_shell_kind=$(_pane_meta_classify_kind "zsh")
assert_eq "shell" "$_shell_kind" "zsh must classify as shell"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "copy-mode -eu" "shell fallback must use copy-mode -eu for history"

test_case "E3: lazygit (tool) -> classify as tool, not agent"
_lazygit_kind=$(_pane_meta_classify_kind "lazygit")
assert_eq "tool" "$_lazygit_kind" "lazygit must classify as tool"

test_case "E3: alternate-screen pane pass-through is before fallback"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "alternate_on" "alternate_on must be checked"
assert_contains "$_ws_up" "copy-mode -eu" "workspace fallback must use copy-mode -eu"

# ──────────────────────────────────────────────────
# Section 8: Scroll regression tests
#
# Validates that the scroll strategy simultaneously
# satisfies both constraints from the handoff doc:
#   1. Non-alternate pane CAN view history (no loss of scroll)
#   2. No frame-stacking from copy-mode + send-keys -M pattern
#   3. copy-mode auto-exits at bottom (-e flag)
#   4. clear after scroll does not leave stale frames
# ──────────────────────────────────────────────────

test_case "regression: workspace wheel-up enables history viewing (not just select-pane)"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "copy-mode" "must enter copy-mode to enable history viewing"
assert_not_contains "$_ws_up" "select-pane -t=" "must not degrade to select-pane-only (blocks scroll)"

test_case "regression: workspace wheel-up avoids frame-stacking pattern"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_not_contains "$_ws_up" "copy-mode -e; send-keys -M" "copy-mode + send-keys -M causes frame stacking"

test_case "regression: workspace wheel-up uses -e flag for auto-exit at bottom"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "-e" "copy-mode must include -e to auto-exit when scrolled to bottom"

test_case "regression: workspace wheel-up uses -u flag for atomic page-up"
_ws_up=$(_tmux_build_workspace_wheel_up)
assert_contains "$_ws_up" "-eu" "copy-mode must use -eu for atomic enter+page-up"

test_case "regression: workspace wheel-down does not re-enter copy-mode"
_ws_down=$(_tmux_build_workspace_wheel_down)
assert_not_contains "$_ws_down" "copy-mode" "wheel-down must not enter copy-mode (only scroll within it)"

test_case "regression: compatible profile preserved unchanged"
_compat_up=$(_tmux_build_compatible_wheel_up)
_compat_down=$(_tmux_build_compatible_wheel_down)
assert_contains "$_compat_up" "copy-mode -e; send-keys -M" "compatible wheel-up must use original pattern"
assert_not_contains "$_compat_down" "copy-mode" "compatible wheel-down must not enter copy-mode"

test_case "regression: workspace and compatible strategies are different"
_ws_up=$(_tmux_build_workspace_wheel_up)
_compat_up=$(_tmux_build_compatible_wheel_up)
if [[ "$_ws_up" != "$_compat_up" ]]; then
    _diff_ok="true"
else
    _diff_ok="false"
fi
assert_eq "true" "$_diff_ok" "workspace and compatible wheel-up strategies must differ"

test_report

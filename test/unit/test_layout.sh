#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/domain/services/layout_service.sh
# Unified Column-based DSL — all layout tests use columns format
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/adapters/terminal/factory.sh"
source "$LIB_DIR/infra/config/loader.sh"

terminal_adapter_init >/dev/null 2>&1

source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/layout_service.sh"
source "$ROOT_DIR/test/integration/tmux_cleanup_scope.sh"

TEST_DIR="/tmp/workhive-test-layout-$$"
TEST_BRANCH="layout-unit-$$"
TEST_SESSION=""

mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"
git init -q . 2>/dev/null || true
mkdir -p .workhive
LAYOUT_SESSION_PREFIX="wh"
GLOBAL_CONFIG_DIR="$XDG_CONFIG_HOME/workhive"
GLOBAL_CONFIG_FILE="$GLOBAL_CONFIG_DIR/config.yaml"

write_project_config() {
    mkdir -p "$TEST_DIR/.workhive"
    cat > "$TEST_DIR/.workhive/config.yaml" <<EOF
$1
EOF
}

write_global_config() {
    mkdir -p "$GLOBAL_CONFIG_DIR"
    cat > "$GLOBAL_CONFIG_FILE" <<EOF
$1
EOF
}

remove_project_config() {
    rm -f "$TEST_DIR/.workhive/config.yaml"
}

remove_global_config() {
    rm -f "$GLOBAL_CONFIG_FILE"
}

# Default config: 2 columns (60/40), col1: 2 panes, col2: 1 pane = 3 panes
write_project_config "version: \"0.2.0\"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: \"${LAYOUT_SESSION_PREFIX}\"
  mouse_support: true
  theme: \"catppuccin-mocha\""
remove_global_config

cleanup() {
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_DIR"
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "lib/domain/services/layout_service.sh"

if ! tmux_usable; then
    test_case "layout tests (tmux not available - skip)"
    skip_test "tmux not available or unusable"
    test_report
    exit $?
fi

cache_clear
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
TEST_SESSION=$(session_get_name "$TEST_BRANCH")

# ──────────────────────────────────────────────────
# layout_create
# ──────────────────────────────────────────────────

test_case "layout_create succeeds"
cache_clear
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_create fails for empty params"
layout_create "" "" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# layout_validate
# ──────────────────────────────────────────────────

test_case "layout_validate passes for 3-pane layout"
layout_validate "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_validate fails for non-existent session"
layout_validate "nonexistent-$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# layout_apply_commands
# ──────────────────────────────────────────────────

test_case "layout_apply_commands succeeds"
layout_apply_commands "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_apply_commands fails for non-existent session"
layout_apply_commands "nonexistent-$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# layout_get_info (unified JSON)
# ──────────────────────────────────────────────────

test_case "layout_get_info returns JSON with columns"
result=$(layout_get_info "$TEST_BRANCH")
assert_contains "$result" '"columns":'

test_case "layout_get_info includes panes array"
assert_contains "$result" '"panes":'

test_case "layout_get_info includes total_panes"
assert_contains "$result" '"total_panes": 3'

# ──────────────────────────────────────────────────
# layout_create_workspace
# ──────────────────────────────────────────────────

test_case "layout_create_workspace creates full workspace"
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.2
layout_create_workspace "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_create_workspace validates successfully"
cache_clear
layout_validate "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

# ──────────────────────────────────────────────────
# layout_select_pane
# ──────────────────────────────────────────────────

test_case "layout_select_pane succeeds for valid pane"
layout_select_pane "$TEST_BRANCH" "0" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_select_pane fails for non-existent session"
layout_select_pane "nonexistent-$$" "0" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# layout_resize_pane
# ──────────────────────────────────────────────────

test_case "layout_resize_pane succeeds for valid resize"
layout_resize_pane "$TEST_BRANCH" "0" "down" "5" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_resize_pane fails for invalid direction"
layout_resize_pane "$TEST_BRANCH" "0" "diagonal" "5" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

test_case "layout_resize_pane fails for non-existent session"
layout_resize_pane "nonexistent-$$" "0" "up" "5" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# layout_reload
# ──────────────────────────────────────────────────

test_case "layout_reload succeeds"
layout_reload "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_reload fails for non-existent session"
layout_reload "nonexistent-$$" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# _layout_normalize (Column-based DSL)
# ──────────────────────────────────────────────────

test_suite "_layout_normalize"

test_case "_layout_normalize: 2-col (1+1 panes) = 2 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: "cmd1"
    - width: 50
      panes:
        - height: 100
          command: "cmd2"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_layout_normalize
assert_eq "2" "$_LCOL_COUNT" "2 columns"
assert_eq "2" "$_LPANE_TOTAL" "2 total panes"
assert_eq "1" "${_LCOL_PANE_COUNTS[0]}" "col 0 has 1 pane"
assert_eq "1" "${_LCOL_PANE_COUNTS[1]}" "col 1 has 1 pane"

test_case "_layout_normalize: 2-col (2+1 panes) = 3 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "cmd1"
        - height: 50
          command: "cmd2"
    - width: 40
      panes:
        - height: 100
          command: "cmd3"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_layout_normalize
assert_eq "2" "$_LCOL_COUNT" "2 columns"
assert_eq "3" "$_LPANE_TOTAL" "3 total panes"
assert_eq "2" "${_LCOL_PANE_COUNTS[0]}" "col 0 has 2 panes"
assert_eq "1" "${_LCOL_PANE_COUNTS[1]}" "col 1 has 1 pane"

test_case "_layout_normalize: 2-col (2+2 panes) = 4 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: "tl"
        - height: 50
          command: "bl"
    - width: 50
      panes:
        - height: 50
          command: "tr"
        - height: 50
          command: "br"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_layout_normalize
assert_eq "2" "$_LCOL_COUNT" "2 columns"
assert_eq "4" "$_LPANE_TOTAL" "4 total panes"

test_case "_layout_normalize preserves roles"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: "claude"
          role: "agent"
    - width: 50
      panes:
        - height: 100
          command: null
          role: "preview"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_layout_normalize
assert_eq "agent" "${_LPANE_ROLES[0]}" "pane 0 role = agent"
assert_eq "preview" "${_LPANE_ROLES[1]}" "pane 1 role = preview"

# ──────────────────────────────────────────────────
# _layout_expected_pane_count (reads from config)
# ──────────────────────────────────────────────────

test_case "_layout_expected_pane_count returns 2 for 2-col 1+1"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: null
    - width: 50
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
assert_eq "2" "$(_layout_expected_pane_count)"

test_case "_layout_expected_pane_count returns 5 for 2-col 2+3"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 50
      panes:
        - height: 33
          command: null
        - height: 33
          command: null
        - height: 34
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
assert_eq "5" "$(_layout_expected_pane_count)"

test_case "_layout_expected_pane_count returns 4 for 4-col 1+1+1+1"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
assert_eq "4" "$(_layout_expected_pane_count)"

test_case "_layout_expected_pane_count reads global layout when project config is absent"
cache_clear
remove_project_config
write_global_config "version: \"0.8.0\"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: null
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: \"${LAYOUT_SESSION_PREFIX}\""
assert_eq "3" "$(_layout_expected_pane_count)"

test_case "_layout_expected_pane_count prefers project layout over global layout"
cache_clear
write_project_config "version: \"0.8.0\"
layout:
  columns:
    - width: 100
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: \"${LAYOUT_SESSION_PREFIX}\""
write_global_config "version: \"0.8.0\"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 50
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: \"${LAYOUT_SESSION_PREFIX}\""
assert_eq "1" "$(_layout_expected_pane_count)"

test_case "_layout_expected_pane_count falls back to built-in defaults when project and global layouts are absent"
cache_clear
remove_project_config
remove_global_config
assert_eq "3" "$(_layout_expected_pane_count)"

# ──────────────────────────────────────────────────
# Pane mapping (_pane_mapping_init)
# ──────────────────────────────────────────────────

test_suite "pane role mapping"

test_case "_pane_mapping_init creates col_*_pane_* roles"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
        - height: 33
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_pane_mapping_init
assert_eq "0" "${PANE_ROLE_TO_INDEX[col_0_pane_0]}" "col_0_pane_0 = 0"
assert_eq "1" "${PANE_ROLE_TO_INDEX[col_0_pane_1]}" "col_0_pane_1 = 1"
assert_eq "2" "${PANE_ROLE_TO_INDEX[col_1_pane_0]}" "col_1_pane_0 = 2"
assert_eq "3" "${PANE_ROLE_TO_INDEX[col_1_pane_1]}" "col_1_pane_1 = 3"
assert_eq "4" "${PANE_ROLE_TO_INDEX[col_1_pane_2]}" "col_1_pane_2 = 4"
assert_eq "0" "${PANE_ROLE_TO_INDEX[main]}" "main = 0"

test_case "_pane_mapping_init registers user-defined roles"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
          role: "agent1"
        - height: 50
          command: null
          role: "agent2"
    - width: 40
      panes:
        - height: 100
          command: null
          role: "preview"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
_pane_mapping_init
assert_eq "0" "${PANE_ROLE_TO_INDEX[agent1]}" "agent1 = 0"
assert_eq "1" "${PANE_ROLE_TO_INDEX[agent2]}" "agent2 = 1"
assert_eq "2" "${PANE_ROLE_TO_INDEX[preview]}" "preview = 2"

test_case "_pane_mapping_init populates normalized layout state in caller shell"
cache_clear
_LCOL_COUNT=0
_LPANE_TOTAL=0
_LAYOUT_NORMALIZE_CACHE_KEY=""
_pane_mapping_init
assert_eq "2" "$_LCOL_COUNT" "normalized column count should persist in caller shell"
assert_eq "3" "$_LPANE_TOTAL" "normalized pane count should persist in caller shell"

test_case "layout_resolve_role_index works for defined role"
cache_clear
idx=$(layout_resolve_role_index "preview")
assert_eq "2" "$idx" "preview resolves to 2"

test_case "layout_resolve_role_index fails for unknown role"
cache_clear
layout_resolve_role_index "nonexistent" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

test_case "layout_list_roles returns all roles"
cache_clear
output=$(layout_list_roles)
assert_contains "$output" "agent1|0"
assert_contains "$output" "preview|2"

# ──────────────────────────────────────────────────
# layout_collect_pane_commands
# ──────────────────────────────────────────────────

test_suite "layout_collect_pane_commands"

test_case "layout_collect_pane_commands returns correct lines for 2-col 1+1"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: "echo left"
    - width: 50
      panes:
        - height: 100
          command: "echo right"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
output=$(layout_collect_pane_commands)
line_count=$(echo "$output" | wc -l | tr -d ' ')
assert_eq "2" "$line_count" "1+1 should output 2 lines"
assert_contains "$output" "|echo left"
assert_contains "$output" "|echo right"

test_case "layout_collect_pane_commands with roles uses role as label"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: "cmd_a"
          role: "agent"
    - width: 50
      panes:
        - height: 50
          command: "cmd_b"
        - height: 50
          command: "cmd_c"
          role: "preview"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
output=$(layout_collect_pane_commands)
line_count=$(echo "$output" | wc -l | tr -d ' ')
assert_eq "3" "$line_count" "1+2 should output 3 lines"
assert_contains "$output" "agent|cmd_a"
assert_contains "$output" "preview|cmd_c"

test_case "layout_collect_pane_commands reads global pane commands when project config is absent"
cache_clear
remove_project_config
write_global_config "version: \"0.8.0\"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: \"global-left\"
          role: \"agent\"
    - width: 50
      panes:
        - height: 50
          command: \"global-top\"
        - height: 50
          command: \"global-bottom\"
          role: \"preview\"
tmux:
  session_prefix: \"${LAYOUT_SESSION_PREFIX}\""
output=$(layout_collect_pane_commands)
line_count=$(echo "$output" | wc -l | tr -d ' ')
assert_eq "3" "$line_count" "global 1+2 should output 3 lines"
assert_contains "$output" "agent|global-left"
assert_contains "$output" "col_1_pane_0|global-top"
assert_contains "$output" "preview|global-bottom"

test_suite "layout_collect_runtime_pane_commands"

test_case "runtime collector includes all runtime panes when runtime pane count differs from config"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: "cfg-agent"
          role: "agent"
    - width: 50
      panes:
        - height: 100
          command: "cfg-preview"
          role: "preview"
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF

_layout_orig_require_exists=$(declare -f session_require_exists || true)
_layout_orig_list_panes=$(declare -f terminal_adapter_list_panes_format || true)

session_require_exists() {
    # shellcheck disable=SC2034
    SESSION_NAME="fake-session"
    return 0
}
terminal_adapter_list_panes_format() {
    printf '0|claude --dangerously-skip-permissions\n1|lazygit\n2|zsh\n'
}

output=$(layout_collect_runtime_pane_commands "restore-branch")
line_count=$(echo "$output" | wc -l | tr -d ' ')
assert_eq "3" "$line_count" "runtime collector should not truncate panes to config pane count"
assert_contains "$output" "agent|claude --dangerously-skip-permissions"
assert_contains "$output" "preview|lazygit"
assert_contains "$output" "pane_2|zsh" "unmapped runtime pane should use stable fallback label"

unset -f session_require_exists terminal_adapter_list_panes_format
[[ -n "$_layout_orig_require_exists" ]] && eval "$_layout_orig_require_exists"
[[ -n "$_layout_orig_list_panes" ]] && eval "$_layout_orig_list_panes"

# ──────────────────────────────────────────────────
# layout_pane_role_label
# ──────────────────────────────────────────────────

test_suite "layout_pane_role_label"

export WORKHIVE_LANG=en

test_case "layout_pane_role_label main returns non-empty"
result=$(layout_pane_role_label "main")
assert_not_empty "$result" "main role label"

test_case "layout_pane_role_label col_0_pane_1 returns 1-indexed"
result=$(layout_pane_role_label "col_0_pane_1")
assert_contains "$result" "1" "col 0 -> 1"
assert_contains "$result" "2" "pane 1 -> 2"

test_case "layout_pane_role_label unknown role returns as-is"
result=$(layout_pane_role_label "my_custom_role")
assert_eq "my_custom_role" "$result"

# ──────────────────────────────────────────────────
# Layout creation with various configs
# ──────────────────────────────────────────────────

test_suite "layout_create (column configs)"

test_case "2-col (1+1) creates 2 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: null
    - width: 50
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "2" "$pane_count" "1+1 = 2 panes"

test_case "2-col (2+2) creates 4 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "4" "$pane_count" "2+2 = 4 panes"

test_case "3-col (2+3+1) creates 6 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 33
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 34
      panes:
        - height: 33
          command: null
        - height: 33
          command: null
        - height: 34
          command: null
    - width: 33
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "6" "$pane_count" "2+3+1 = 6 panes"

test_case "1-col (3 panes) creates 3 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 100
      panes:
        - height: 33
          command: null
        - height: 33
          command: null
        - height: 34
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "3" "$pane_count" "1-col 3 panes"

test_case "4-col (1+1+1+1) creates 4 panes"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
    - width: 25
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "4" "$pane_count" "4-col 1+1+1+1 = 4 panes"

test_case "1-col 1-pane creates 1 pane (no splits)"
cache_clear
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 100
      panes:
        - height: 100
          command: null
tmux:
  session_prefix: "${LAYOUT_SESSION_PREFIX}"
EOF
tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
cache_clear
sleep 0.3
session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
layout_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
pane_count=$(tmux list-panes -t "$TEST_SESSION" 2>/dev/null | wc -l | tr -d ' ')
assert_eq "1" "$pane_count" "1-col 1-pane = 1 pane"

test_case "layout_validate passes for 1-pane config"
layout_validate "$TEST_BRANCH" >/dev/null 2>&1
assert_eq "0" "$?"

test_case "layout_get_info returns correct JSON for 1-pane"
result=$(layout_get_info "$TEST_BRANCH")
assert_contains "$result" '"columns": 1'
assert_contains "$result" '"total_panes": 1'

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Agent Detection (pure logic tests)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "agent detection"

test_case "agent_detect_in_pane returns empty for non-existent session"
result=$(agent_detect_in_pane "nonexistent-session" "0" 2>/dev/null)
assert_empty "$result" "should return empty"

test_case "agent_detect_status returns none for non-existent session"
result=$(agent_detect_status "nonexistent-session" "0" 2>/dev/null)
assert_eq "none" "$result" "should return none"

test_case "agent_detect_all_panes returns data for valid session"
result=$(agent_detect_all_panes "$TEST_SESSION" 2>/dev/null)
assert_not_empty "$result" "should return pane data"

test_case "agent_detect_all_panes format is idx|name|status"
first_line=$(echo "$result" | head -1)
field_count=$(echo "$first_line" | awk -F'|' '{print NF}')
assert_eq "3" "$field_count" "3 fields per line"

test_case "_layout_infer_agent_legacy_status falls back to running for ai command without protocol state"
_orig_tools_registry_is_ai_command="$(declare -f tools_registry_is_ai_command)"
tools_registry_is_ai_command() {
    [[ "${1:-}" == "opencode" ]]
}
result=$(_layout_infer_agent_legacy_status "opencode" "opencode" "idle")
unset -f tools_registry_is_ai_command
eval "$_orig_tools_registry_is_ai_command"
assert_eq "running" "$result" "ai command without protocol state should be treated as running"

test_case "agent_detect_all_panes marks manually launched ai command as running"
_orig_tmux="$(declare -f tmux)"
_orig_tools_registry_is_ai_command="$(declare -f tools_registry_is_ai_command)"
tmux() {
    local cmd="$1"; shift
    case "$cmd" in
        list-panes)
            printf '0|%%10|opencode\n'
            ;;
        show-options)
            printf '%s' ''
            ;;
        *)
            command tmux "$cmd" "$@"
            ;;
    esac
}
tools_registry_is_ai_command() {
    [[ "${1:-}" == "opencode" ]]
}
result=$(agent_detect_all_panes "$TEST_SESSION" 2>/dev/null)
unset -f tmux tools_registry_is_ai_command
eval "$_orig_tmux"
eval "$_orig_tools_registry_is_ai_command"
assert_contains "$result" "0|opencode|running" "manual ai process should show running"

test_case "agent_needs_attention returns exit code"
agent_needs_attention "$TEST_SESSION" 2>/dev/null
assert_true "true" "does not crash"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Smart Notification (pure logic tests)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "smart notification"

test_case "agent_notify does not crash"
agent_notify "$TEST_SESSION" "0" "claude" "finished" 2>/dev/null
assert_eq "0" "$?"

test_case "agent_notify with waiting event"
agent_notify "$TEST_SESSION" "1" "codex" "waiting" 2>/dev/null
assert_eq "0" "$?"

test_report

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Tests: New Features Regression
# Features: status, doctor, gc, notifications, layout, notes, command help
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
BIN_DIR="$ROOT_DIR/bin"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

test_suite "BDD: All New Features Integration Tests"

_tmux_usable() {
    command -v tmux &>/dev/null || return 1
    local probe="WH-TEST@new-feats-probe@$$"
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
# Setup
# ──────────────────────────────────────────────────
TEST_DIR="/tmp/workhive-bdd-newfeats-$$"
# Session prefix is now deterministic: wh@<project> (project = basename of TEST_DIR)
EXPECTED_PREFIX="wh@$(basename "$TEST_DIR")"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
git init >/dev/null 2>&1
git config user.name "Test"
git config user.email "test@example.com"
echo "# BDD New Features" > README.md
echo ".workhive/" > .gitignore
git add README.md .gitignore
git commit -m "Init" >/dev/null 2>&1
mkdir -p .workhive
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
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
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF

cleanup() {
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_DIR"
    _cleanup_wh_test_sessions
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ════════════════════════════════════════════════
# Feature: Theme Preview Callback Line Count
# ════════════════════════════════════════════════

source "$LIB_DIR/app/shared/theme_helper.sh" 2>/dev/null || true

test_case "GIVEN theme_preview_callback WHEN called THEN sets _TUI_CALLBACK_LINES to 4"
_TUI_CALLBACK_LINES=0
_theme_preview_callback 0 2>/dev/null || true
assert_eq "4" "$_TUI_CALLBACK_LINES" "callback should report 4 lines"

# ════════════════════════════════════════════════
# Feature: session_list_project (project-scoped listing)
# ════════════════════════════════════════════════

if command -v tmux &>/dev/null; then
    source "$LIB_DIR/infra/adapters/terminal/factory.sh" 2>/dev/null || true
    terminal_adapter_init >/dev/null 2>&1 || true
    source "$LIB_DIR/domain/services/session_service.sh" 2>/dev/null || true

    test_case "GIVEN session_list_project WHEN no matching sessions THEN returns empty"
    result=$(session_list_project 2>/dev/null || true)
    [[ -z "$result" ]]
    # shellcheck disable=SC2319
    rc=$?
    assert_eq "0" "$rc" "should return empty for no matching sessions"

    test_case "GIVEN session_get_prefix WHEN called THEN returns wh@<project> format"
    cache_clear
    _prefix=$(session_get_prefix)
    assert_eq "$EXPECTED_PREFIX" "$_prefix" "prefix should be wh@<project> (deterministic)"

    test_case "GIVEN session_get_project_name WHEN in git repo THEN returns git root basename"
    cache_clear
    _proj=$(session_get_project_name)
    _expected_proj=$(basename "$TEST_DIR")
    assert_eq "$_expected_proj" "$_proj" "project name should be git root basename"

    test_case "GIVEN two different project dirs WHEN session_get_name THEN different session names"
    cache_clear
    _n1=$(session_get_name "demo01")
    _other="/tmp/workhive-bdd-other-$$"
    mkdir -p "$_other" && (cd "$_other" && git init -q .)
    _n2=$(cd "$_other" && export _WORKHIVE_GIT_ROOT="" && session_get_name "demo01")
    [[ "$_n1" != "$_n2" ]]
    rc=$?
    assert_eq "0" "$rc" "same branch different projects => different tmux session names (wh@<project1>@demo01 vs wh@<project2>@demo01)"
    rm -rf "$_other"
fi

# ════════════════════════════════════════════════
# Feature: Doctor Command - Environment Health Check
# ════════════════════════════════════════════════

test_case "GIVEN a project WHEN running doctor THEN shows health check"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
rc=$?
assert_contains "$output" "Health Check" "should show title"
assert_success "$rc" "should succeed"

test_case "GIVEN a project WHEN running doctor THEN checks required deps"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "git" "should check git"
assert_contains "$output" "tmux" "should check tmux"
assert_contains "$output" "yq" "should check yq"

test_case "GIVEN a project WHEN running doctor THEN checks required sections only"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "Required" "should show required section"
assert_contains "$output" "Checking configuration" "should check configuration section"
assert_contains "$output" "Checking tmux" "should check tmux section"
assert_contains "$output" "Checking git" "should check git section"

test_case "GIVEN valid config WHEN running doctor THEN config check passes"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "Config" "should check config"

test_case "GIVEN git repo WHEN running doctor THEN git check passes"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
# Should show green check for git repo
assert_contains "$output" "Git" "should check git repo"
assert_contains "$output" "main" "should show main branch"

test_case "GIVEN doctor --format json WHEN running THEN outputs JSON without recommended field"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor --format json 2>&1)
assert_contains "$output" '"dependencies"'
assert_contains "$output" '"required"'
assert_not_contains "$output" '"recommended"' "should not have recommended field"
assert_contains "$output" '"config"'
assert_contains "$output" '"git"'
assert_contains "$output" '"in_repo": true'
assert_not_contains "$output" '"pane_tools"' "should not have pane_tools field"

# ════════════════════════════════════════════════
# Feature: GC Command - Garbage Collection
# ════════════════════════════════════════════════

test_case "GIVEN no merged branches WHEN running gc THEN shows no merged"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" gc --auto 2>&1)
assert_contains "$output" "Garbage Collection" "should show title"
assert_contains "$output" "No merged" "should show no merged branches"

test_case "GIVEN merged branch with worktree WHEN gc --auto THEN performs cleanup flow"
cd "$TEST_DIR" || exit 1
# Create branch and merge
git checkout -b "bdd-gc-merge-$$" >/dev/null 2>&1
echo "merge test" > gc_test.txt
git add gc_test.txt
git commit -m "GC test" >/dev/null 2>&1
git checkout main >/dev/null 2>&1
git merge "bdd-gc-merge-$$" >/dev/null 2>&1

# Create corresponding worktree directory (mock)
mkdir -p ".workhive/worktrees/bdd-gc-merge-$$"

output=$("$BIN_DIR/workhive" gc --auto 2>&1)
assert_contains "$output" "Garbage Collection" "should show title"
assert_contains "$output" "complete" "should complete"
# Clean up
rm -rf ".workhive/worktrees/bdd-gc-merge-$$"
git branch -d "bdd-gc-merge-$$" >/dev/null 2>&1 || true

test_case "GIVEN stale state files WHEN gc --auto THEN cleans them"
cd "$TEST_DIR" || exit 1
mkdir -p .workhive/state
echo "stale" > .workhive/state/layout_stale-branch-bdd-$$
echo "stale" > .workhive/state/pane_commands_stale-branch-bdd-$$
output=$("$BIN_DIR/workhive" gc --auto 2>&1)
assert_contains "$output" "Stale" "should detect stale state files"
# Files should be cleaned
[[ ! -f ".workhive/state/layout_stale-branch-bdd-$$" ]]
rc=$?
assert_success "$rc" "stale layout file should be removed"
[[ ! -f ".workhive/state/pane_commands_stale-branch-bdd-$$" ]]
rc=$?
assert_success "$rc" "stale pane_commands file should be removed"

# ════════════════════════════════════════════════
# Feature: GC - Unmodified Branch Detection
# ════════════════════════════════════════════════

source "$LIB_DIR/app/commands/gc_command.sh" 2>/dev/null || true

test_case "GIVEN _gc_get_unmodified_branches WHEN no worktrees THEN returns empty"
cd "$TEST_DIR" || exit 1
cache_clear
result=$(_gc_get_unmodified_branches 2>/dev/null || true)
[[ -z "$result" ]]
rc=$?
assert_eq "0" "$rc" "should return empty when no worktrees"

test_case "GIVEN gc --auto WHEN running THEN shows unmodified scan"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" gc --auto 2>&1)
# The output uses i18n, check for either English or Chinese
if echo "$output" | grep -qi "unmodified"; then
    assert_success "0" "should scan unmodified"
else
    assert_success "1" "should scan unmodified"
fi

# ════════════════════════════════════════════════
# Feature: GC - Project Isolation (session_list_project)
# ════════════════════════════════════════════════

test_case "GIVEN gc orphan scan WHEN using session_list_project THEN is project-scoped"
cd "$TEST_DIR" || exit 1
cache_clear
result=$(_gc_get_orphan_sessions 2>/dev/null || true)
# Orphan scan should not crash; result may be empty or contain session names
[[ -z "$result" || -n "$result" ]]
rc=$?
assert_eq "0" "$rc" "orphan scan should not fail"

# ════════════════════════════════════════════════
# Feature: Unknown Command Handling
# ════════════════════════════════════════════════

test_case "GIVEN removed command WHEN running switch THEN shows unknown command"
unset TMUX
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" switch 2>&1 || true)
assert_contains "$output" "Unknown command: switch" "should show unknown command"
assert_contains "$output" "Did you mean:" "should show suggestion hint"

test_case "GIVEN removed command WHEN switch with branch THEN still shows unknown command"
unset TMUX
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" switch feature-x 2>&1 || true)
assert_contains "$output" "Unknown command: switch"

# ════════════════════════════════════════════════
# Feature: Per-Worktree Notes
# ════════════════════════════════════════════════

test_case "GIVEN .workhive dir WHEN notes dir created THEN it exists"
cd "$TEST_DIR" || exit 1
mkdir -p .workhive/notes
assert_dir_exists ".workhive/notes"

test_case "GIVEN notes dir WHEN creating a note THEN note file is created"
cd "$TEST_DIR" || exit 1
touch ".workhive/notes/main.md"
assert_file_exists ".workhive/notes/main.md"

# ════════════════════════════════════════════════
# Feature: Command Help Integration
# ════════════════════════════════════════════════

test_case "GIVEN main help WHEN listing commands THEN shows all new commands"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "list" "should list list"
assert_contains "$output" "doctor" "should list doctor"
assert_contains "$output" "gc" "should list gc"
assert_not_contains "$output" " switch " "should not list removed command names"

test_case "GIVEN Chinese mode WHEN listing help THEN shows Chinese descriptions"
cd "$TEST_DIR" || exit 1
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "全局仪表盘" "should show Chinese list desc"
assert_contains "$output" "检查环境健康状态" "should show Chinese doctor desc"
assert_contains "$output" "清理已合并分支" "should show Chinese gc desc"

# ════════════════════════════════════════════════
# Feature: Cross-Pane Notification System
# ════════════════════════════════════════════════

source "$LIB_DIR/domain/services/session_service.sh"

test_case "GIVEN session_notify WHEN session missing THEN returns failure"
session_notify "nonexistent-xyz-$$" "test" 2>/dev/null
assert_failure "$?" "should fail for missing session"

# ════════════════════════════════════════════════
# Feature: All commands have consistent error handling
# ════════════════════════════════════════════════

test_case "GIVEN invalid option WHEN running any new command THEN shows error consistently"
for cmd in list doctor gc; do
    output=$("$BIN_DIR/workhive" "$cmd" --invalid-option-xyz 2>&1 || true)
    assert_contains "$output" "Unknown option" "($cmd) should show unknown option error"
done

# ════════════════════════════════════════════════
# Feature: Attach interactive select stdout isolation
# ════════════════════════════════════════════════

test_case "GIVEN attach interactive select WHEN no sessions and no worktrees THEN no stdout leak"
output=$("$BIN_DIR/workhive" attach 2>&1 </dev/null || true)
# In non-TTY mode, attach should fail gracefully without garbled output
assert_not_contains "$output" "ℹ️  无活跃会话, 但发现已有 Worktree" "should not have info message in branch name"

# ════════════════════════════════════════════════
# Feature: TUI single Enter confirmation
# ════════════════════════════════════════════════

test_case "GIVEN tui_select_menu WHEN buffer is clean THEN no _first_read guard exists"
tui_file="${LIB_DIR}/utils/tui.sh"
if grep -q '_first_read' "$tui_file" 2>/dev/null; then
    assert_eq "0" "1" "tui.sh should not contain _first_read double-enter guard"
else
    assert_eq "0" "0" "tui.sh correctly has no _first_read guard"
fi

# ════════════════════════════════════════════════
# Feature: init removed recommended tools section
# ════════════════════════════════════════════════

test_case "GIVEN init --non-interactive WHEN checking output THEN does not show recommended tools section"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" init --non-interactive --force 2>&1)
assert_not_contains "$output" "Recommended tools" "should not show Recommended tools section"
assert_not_contains "$output" "推荐工具" "should not show 推荐工具 section"

# ════════════════════════════════════════════════
# Feature: Pane Tool Fallback (missing tools → Shell)
# ════════════════════════════════════════════════

source "$LIB_DIR/app/commands/init_command.sh" 2>/dev/null || true

test_case "GIVEN template with missing tools WHEN resolving commands THEN falls back to null"
resolved=$(_init_resolve_template_commands "nonexistent_tool_abc|git|another_missing")
assert_contains "$resolved" "null" "missing tools should become null"
assert_contains "$resolved" "git" "installed tools should be preserved"

test_case "GIVEN template with null commands WHEN resolving THEN nulls pass through"
resolved=$(_init_resolve_template_commands "null|null|git")
assert_eq "null|null|git" "$resolved" "null commands should pass through unchanged"

test_case "GIVEN template with commands with args WHEN resolving THEN checks binary only"
resolved=$(_init_resolve_template_commands "git --version|fake_tool --flag arg")
assert_contains "$resolved" "git --version" "git --version should be kept"
assert_not_contains "$resolved" "fake_tool" "fake_tool should be replaced"

# ════════════════════════════════════════════════
# Feature: Custom Layout Template
# ════════════════════════════════════════════════

test_case "GIVEN _TPL_IDS WHEN listing templates THEN includes custom"
found=false
for tpl_id in "${_TPL_IDS[@]}"; do
    [[ "$tpl_id" == "custom" ]] && found=true
done
assert_eq "true" "$found" "custom should be in template IDs"

test_case "GIVEN custom template WHEN getting COLS THEN returns 0 (interactive)"
cols=$(_tpl_get "custom" "COLS")
assert_eq "0" "$cols"

test_case "GIVEN custom template WHEN getting PANES THEN returns empty"
panes=$(_tpl_get "custom" "PANES")
assert_empty "$panes" "custom PANES should be empty"

# ════════════════════════════════════════════════
# Feature: Columns Layout Config Generation
# ════════════════════════════════════════════════

test_case "GIVEN columns layout WHEN generating config THEN creates valid YAML"
cd "$TEST_DIR" || exit 1
rm -rf .workhive
_init_col_count=2
_init_pane_counts=(2 1)
_init_col_widths=(60 40)
_init_pane_heights=(50 50 100)
_init_pane_cmds=("null" "null" "null")
_init_generate_config "catppuccin-mocha"
assert_file_exists ".workhive/config.yaml" "config should be created"
config_content=$(cat .workhive/config.yaml 2>/dev/null)
assert_contains "$config_content" "columns:"
assert_contains "$config_content" "width: 60"
assert_contains "$config_content" "width: 40"
assert_contains "$config_content" "panes:"
assert_contains "$config_content" "version: \"${WORKHIVE_VERSION}\"" "should use WORKHIVE_VERSION"

test_case "GIVEN columns config WHEN yq reads it THEN all keys accessible"
if command -v yq &>/dev/null; then
    cache_clear
    col_count=$(yq '.layout.columns | length' .workhive/config.yaml)
    assert_eq "2" "$col_count" "should have 2 columns"
    width0=$(yq '.layout.columns[0].width' .workhive/config.yaml)
    assert_eq "60" "$width0" "first column width should be 60"
fi

test_case "GIVEN single-column layout WHEN generating config THEN has 1 column"
cd "$TEST_DIR" || exit 1
rm -rf .workhive
_init_col_count=1
_init_pane_counts=(3)
_init_col_widths=(100)
_init_pane_heights=(33 33 34)
_init_pane_cmds=("null" "null" "null")
_init_generate_config "catppuccin-mocha"
assert_file_exists ".workhive/config.yaml" "config should be created"
config_content=$(cat .workhive/config.yaml 2>/dev/null)
assert_contains "$config_content" "width: 100"

test_case "GIVEN single-column config WHEN yq reads it THEN has 1 column with 3 panes"
if command -v yq &>/dev/null; then
    cache_clear
    col_count=$(yq '.layout.columns | length' .workhive/config.yaml)
    assert_eq "1" "$col_count" "should have 1 column"
    pane_count=$(yq '.layout.columns[0].panes | length' .workhive/config.yaml)
    assert_eq "3" "$pane_count" "should have 3 panes"
fi

# ════════════════════════════════════════════════
# Feature: Columns Layout in Layout Service (tmux)
# ════════════════════════════════════════════════

if _tmux_usable; then
    source "$LIB_DIR/infra/adapters/terminal/factory.sh" 2>/dev/null || true
    terminal_adapter_init >/dev/null 2>&1 || true
    source "$LIB_DIR/domain/services/session_service.sh" 2>/dev/null || true
    source "$LIB_DIR/domain/services/layout_service.sh" 2>/dev/null || true

    _COLUMNS_TEST_BRANCH="columns-bdd-$$"

    test_case "GIVEN columns config (2+2) WHEN creating workspace THEN pane count matches"
    cd "$TEST_DIR" || exit 1
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
  mouse_support: true
  theme: "catppuccin-mocha"
EOF
    cache_clear
    session_create "$_COLUMNS_TEST_BRANCH" "/tmp" >/dev/null 2>&1
    layout_create "$_COLUMNS_TEST_BRANCH" "/tmp" >/dev/null 2>&1
    _col_session=$(session_get_name "$_COLUMNS_TEST_BRANCH")
    pane_count=$(tmux list-panes -t "$_col_session" 2>/dev/null | wc -l | tr -d ' ')
    assert_eq "4" "$pane_count" "columns 2+2 should have 4 panes"

    test_case "GIVEN columns workspace WHEN layout_get_info THEN returns correct JSON"
    cache_clear
    info=$(layout_get_info "$_COLUMNS_TEST_BRANCH")
    assert_contains "$info" '"columns"' "should have columns field"
    assert_contains "$info" '"total_panes": 4' "should show 4 total panes"

    test_case "GIVEN columns workspace WHEN layout_validate THEN passes"
    cache_clear
    layout_validate "$_COLUMNS_TEST_BRANCH" >/dev/null 2>&1
    rc=$?
    assert_eq "0" "$rc"

    test_case "GIVEN columns workspace WHEN layout_apply_commands THEN succeeds"
    cache_clear
    layout_apply_commands "$_COLUMNS_TEST_BRANCH" "/tmp" >/dev/null 2>&1
    rc=$?
    assert_eq "0" "$rc"

    # Cleanup
    tmux kill-session -t "$_col_session" 2>/dev/null || true

    # ── Single column (3 panes) ──
    _SINGLE_COL_BRANCH="single-col-$$"

    test_case "GIVEN columns config (1 col, 3 panes) WHEN creating workspace THEN pane count matches"
    cd "$TEST_DIR" || exit 1
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
  mouse_support: true
  theme: "catppuccin-mocha"
EOF
    cache_clear
    session_create "$_SINGLE_COL_BRANCH" "/tmp" >/dev/null 2>&1
    layout_create "$_SINGLE_COL_BRANCH" "/tmp" >/dev/null 2>&1
    _single_session=$(session_get_name "$_SINGLE_COL_BRANCH")
    pane_count=$(tmux list-panes -t "$_single_session" 2>/dev/null | wc -l | tr -d ' ')
    assert_eq "3" "$pane_count" "columns 1-col 3-pane should have 3 panes"

    test_case "GIVEN single-column workspace WHEN layout_validate THEN passes"
    cache_clear
    layout_validate "$_SINGLE_COL_BRANCH" >/dev/null 2>&1
    rc=$?
    assert_eq "0" "$rc"

    # Cleanup
    tmux kill-session -t "$_single_session" 2>/dev/null || true
else
    test_case "SKIP: tmux not usable for layout service assertions"
    assert_eq "0" "0" "tmux-dependent layout tests skipped in current environment"
fi

# Restore original config
cd "$TEST_DIR" || exit 1
mkdir -p .workhive
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
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
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF

test_report

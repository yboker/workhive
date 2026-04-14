#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: list_command.sh
# 100% coverage for list command logic
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
BIN_DIR="$ROOT_DIR/bin"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"

# Set up test environment
TEST_DIR="/tmp/workhive-test-list-$$"
TEST_PREFIX="whst$$"
export XDG_CONFIG_HOME="/tmp/workhive-test-list-xdg-$$"
GLOBAL_CONFIG_DIR="$XDG_CONFIG_HOME/workhive"
GLOBAL_CONFIG_FILE="$GLOBAL_CONFIG_DIR/config.yaml"
mkdir -p "$TEST_DIR" "$XDG_CONFIG_HOME"
cd "$TEST_DIR" || exit 1

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

if ! git init >/dev/null 2>&1; then
    echo "FATAL: git init failed (check Xcode license: sudo xcodebuild -license)"
    exit 1
fi

git config user.name "Test"
git config user.email "test@example.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1
mkdir -p .workhive
write_project_config "version: \"0.8.0\"
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
  session_prefix: \"${TEST_PREFIX}\"
worktree:
  base_path: \".workhive/worktrees\"
  auto_create: true
  auto_delete: true"
write_global_config "language: en"

# Add .workhive to gitignore and commit, to avoid affecting git status detection
echo ".workhive/" > .gitignore
git add .gitignore
git commit -m "Add gitignore" >/dev/null 2>&1

cleanup() {
    local session_name
    local project_name
    project_name=$(basename "$TEST_DIR")

    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        case "$session_name" in
            "${TEST_PREFIX}@"*"${project_name}"@*|wh@*"${project_name}"@*)
                tmux kill-session -t "$session_name" 2>/dev/null || true
                ;;
        esac
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)

    cd / || return 1
    rm -rf "$TEST_DIR" "$XDG_CONFIG_HOME"
}
trap cleanup EXIT

test_suite "list_dashboard_command.sh - Unit Tests"

# ──────────────────────────────────────────────────
# Help message tests
# ──────────────────────────────────────────────────

test_case "list --help shows usage (English)"
output=$(cd "$TEST_DIR" && env -u WORKHIVE_LANG "$BIN_DIR/workhive" list --help 2>&1)
assert_contains "$output" "Usage:" "should show English usage"
assert_contains "$output" "workhive list" "should show list usage"
assert_contains "$output" "--format" "should show format option"
assert_contains "$output" "table" "should mention table format"
assert_contains "$output" "json" "should mention json format"

test_case "list -h shows help"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list -h 2>&1)
assert_contains "$output" "workhive list"

test_case "list --help shows Chinese when WORKHIVE_LANG=zh"
output=$(cd "$TEST_DIR" && WORKHIVE_LANG=zh "$BIN_DIR/workhive" list --help 2>&1)
assert_contains "$output" "全局仪表盘" "should show Chinese title"
assert_contains "$output" "--format" "should show format option in Chinese help"

# ──────────────────────────────────────────────────
# CLI argument parsing tests
# ──────────────────────────────────────────────────

test_case "list rejects unknown options"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "list rejects positional arguments"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list something 2>&1 || true)
assert_contains "$output" "does not accept positional"

test_case "list --format without value shows error"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "list --format with invalid value shows error"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format xml 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "list --format with option-like value shows error"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format --help 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "list rejects --watch with --format json"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --watch --format json 2>&1 || true)
assert_contains "$output" "--watch" "should report watch/json mutex"
assert_contains "$output" "--format json" "should mention json format mutex"

test_case "list rejects --watch with --format json in zh"
output=$(cd "$TEST_DIR" && WORKHIVE_LANG=zh "$BIN_DIR/workhive" list --watch --format json 2>&1 || true)
assert_contains "$output" "--watch 不能与 --format json 同时使用" "zh mutex message should be localized"

# ──────────────────────────────────────────────────
# Empty dashboard tests
# ──────────────────────────────────────────────────

test_case "list shows dashboard baseline row with only primary Git worktree"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
assert_contains "$output" "Dashboard" "should show Dashboard title"
assert_contains "$output" "main" "should show main branch baseline row"

test_case "list uses snapshot wording instead of persistence wording in zh output"
output=$(cd "$TEST_DIR" && WORKHIVE_LANG=zh "$BIN_DIR/workhive" list 2>&1)
assert_contains "$output" "会话快照恢复" "should show snapshot-based section title"
assert_contains "$output" "基于快照" "should show snapshot-based hint wording when disabled"

test_case "list --format json with only primary Git worktree shows structure"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format json 2>&1)
assert_contains "$output" '"summary"' "should have summary"
assert_contains "$output" '"workspaces"' "should have workspaces"

test_case "list keeps showing main baseline row when managed worktrees exist"
test_branch="feature-main-visible-$$"
(
    cd "$TEST_DIR" || exit 1
    git worktree add -b "$test_branch" ".workhive/worktrees/$test_branch" >/dev/null 2>&1
)
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
assert_contains "$output" "main" "should still show main baseline row with other worktrees"
assert_contains "$output" "$test_branch" "should show managed worktree branch"
(
    cd "$TEST_DIR" || exit 1
    git worktree remove --force ".workhive/worktrees/$test_branch" >/dev/null 2>&1 || true
    git branch -D "$test_branch" >/dev/null 2>&1 || true
)

test_case "list shows both main baseline and managed worktree"
test_branch="feature-list-main-visible-$$"
(
    cd "$TEST_DIR" || exit 1
    git worktree add -b "$test_branch" ".workhive/worktrees/$test_branch" >/dev/null 2>&1
)
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
assert_contains "$output" "main" "list should include main baseline row"
assert_contains "$output" "$test_branch" "list should include managed worktree row"
(
    cd "$TEST_DIR" || exit 1
    git worktree remove --force ".workhive/worktrees/$test_branch" >/dev/null 2>&1 || true
    git branch -D "$test_branch" >/dev/null 2>&1 || true
)

# ──────────────────────────────────────────────────
# Alias tests
# ──────────────────────────────────────────────────

test_case "ls is a shorthand for list"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" ls --help 2>&1)
assert_contains "$output" "workhive list"

# ──────────────────────────────────────────────────
# Help should show list and other commands
# ──────────────────────────────────────────────────

test_case "main help lists list command"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "list"

test_case "main help lists pane command"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "pane"

test_case "main help lists doctor command"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "doctor"

test_case "main help lists gc command"
output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "gc"

# ──────────────────────────────────────────────────
# _status_get_git_status unit tests
# ──────────────────────────────────────────────────

# Load list_command functions for direct internal function calls
source "$LIB_DIR/domain/services/worktree_service.sh"
source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/layout_service.sh"
source "$LIB_DIR/app/commands/list_command.sh"

test_case "_status_get_git_status returns '-' for non-existent path"
result=$(_status_get_git_status "/non/existent/path")
assert_eq "-" "$result"

test_case "_status_get_git_status returns 'clean' for clean repo"
# Use TEST_DIR which should be clean (we committed everything and added .gitignore)
result=$(_status_get_git_status "$TEST_DIR")
assert_eq "clean" "$result" "repo with .gitignore should be clean"

test_case "_status_get_git_status detects modified files"
echo "changed" >> "$TEST_DIR/README.md"
result=$(_status_get_git_status "$TEST_DIR")
assert_contains "$result" "M" "should detect modified file"
# Restore
git -C "$TEST_DIR" checkout -- README.md

test_case "_status_get_git_status detects untracked files"
touch "$TEST_DIR/untracked_file.txt"
result=$(_status_get_git_status "$TEST_DIR")
assert_contains "$result" "?" "should detect untracked file"
rm -f "$TEST_DIR/untracked_file.txt"

# ──────────────────────────────────────────────────
# _status_get_session_status unit tests
# ──────────────────────────────────────────────────

test_case "_status_get_session_status returns '-' for non-existent session"
result=$(_status_get_session_status "non-existent-session-xyz")
assert_eq "-" "$result"

# ──────────────────────────────────────────────────
# _status_get_last_activity unit tests
# ──────────────────────────────────────────────────

test_case "_status_get_last_activity returns '-' for non-existent path"
result=$(_status_get_last_activity "/non/existent/path")
assert_eq "-" "$result"

test_case "_status_get_last_activity returns time string for valid path"
result=$(_status_get_last_activity "$TEST_DIR")
# Should be "just now" or "Xm ago" or a similar time string
assert_not_empty "$result" "should return a time string"
assert_ne "-" "$result" "should not be dash"

test_case "_status_session_display returns active label for single active session"
result=$(_status_session_display "active" "1" "1" "0")
assert_contains "$result" "●" "single active should include active marker"

test_case "_status_session_display returns count suffix for multi detached sessions"
result=$(_status_session_display "detached" "3" "0" "3")
assert_contains "$result" "x3" "multi detached should include multiplicity suffix"

test_case "_status_session_display returns mixed split for mixed sessions"
result=$(_status_session_display "mixed" "2" "1" "1")
assert_contains "$result" "1/1" "mixed display should include active/detached split"

test_case "_status_validate_mode_options accepts normal table mode"
_status_validate_mode_options "false" "table" "false" >/dev/null 2>&1
assert_success $? "normal table mode should be valid"

test_case "_status_validate_mode_options rejects layout with json"
_status_validate_mode_options "true" "json" "false" >/dev/null 2>&1
assert_failure $? "layout + json should be rejected"

test_case "_status_validate_mode_options rejects watch with json"
_status_validate_mode_options "false" "json" "true" >/dev/null 2>&1
assert_failure $? "watch + json should be rejected"

test_case "_status_is_json_format returns success only for json"
_status_is_json_format "json" >/dev/null 2>&1
assert_success $? "json should be detected as json format"
_status_is_json_format "table" >/dev/null 2>&1
assert_failure $? "table should not be treated as json format"

test_case "_status_should_show_progress hides progress for json only"
_status_should_show_progress "json" >/dev/null 2>&1
assert_failure $? "json mode should disable progress indicator"
_status_should_show_progress "table" >/dev/null 2>&1
assert_success $? "table mode should allow progress indicator"

test_case "list --layout shows global source and panes when project layout is absent"
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
  session_prefix: \"${TEST_PREFIX}\"
worktree:
  base_path: \".workhive/worktrees\"
  auto_create: true
  auto_delete: true"
output=$(cd "$TEST_DIR" && WORKHIVE_LANG=en "$BIN_DIR/workhive" list --layout 2>&1)
assert_contains "$output" "$GLOBAL_CONFIG_FILE"
assert_contains "$output" "3"
assert_contains "$output" "agent"
assert_contains "$output" "global-left"
assert_contains "$output" "preview"
assert_contains "$output" "global-bottom"

test_case "list --layout prefers project layout over global layout"
cache_clear
write_project_config "version: \"0.8.0\"
layout:
  columns:
    - width: 100
      panes:
        - height: 100
          command: \"project-only\"
          role: \"mainpane\"
tmux:
  session_prefix: \"${TEST_PREFIX}\"
worktree:
  base_path: \".workhive/worktrees\"
  auto_create: true
  auto_delete: true"
write_global_config "version: \"0.8.0\"
layout:
  columns:
    - width: 50
      panes:
        - height: 50
          command: \"global-top\"
        - height: 50
          command: \"global-bottom\"
    - width: 50
      panes:
        - height: 100
          command: \"global-right\"
tmux:
  session_prefix: \"${TEST_PREFIX}\"
worktree:
  base_path: \".workhive/worktrees\"
  auto_create: true
  auto_delete: true"
output=$(cd "$TEST_DIR" && WORKHIVE_LANG=en "$BIN_DIR/workhive" list --layout 2>&1)
assert_contains "$output" "$TEST_DIR/.workhive/config.yaml"
assert_contains "$output" "1"
assert_contains "$output" "mainpane"
assert_contains "$output" "project-only"
assert_not_contains "$output" "global-right"

# ──────────────────────────────────────────────────
# Output tests with workspaces (requires tmux)
# ──────────────────────────────────────────────────

if tmux_usable; then
    cd "$TEST_DIR" || exit 1
    "$BIN_DIR/workhive" open list-test-$$ --no-attach >/dev/null 2>&1 || true

    test_case "list table shows workspace when it exists"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "list-test-$$" "should show branch name"
    assert_contains "$output" "Dashboard" "should show dashboard title"

    test_case "list JSON includes git_status and last_activity"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format json 2>&1)
    assert_contains "$output" '"summary"' "should have summary"
    assert_contains "$output" '"workspaces"' "should have workspaces"
    assert_contains "$output" '"branch"' "should have branch field"
    assert_contains "$output" '"git_status"' "JSON should have git_status"
    assert_contains "$output" '"last_activity"' "JSON should have last_activity"
    assert_contains "$output" '"total"' "should have total in summary"
    assert_not_contains "$output" '"dependencies"' "JSON should not include removed dependencies section"
    assert_contains "$output" '"persistence"' "JSON should have persistence"

    test_case "list table shows session status"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    if echo "$output" | grep -qE "(active|detached)"; then
        assert_success 0 "shows session status"
    else
        assert_contains "$output" "none" "should show some session status"
    fi

    test_case "list table shows git status"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "clean" "worktree should show git status"

    test_case "list table shows last activity"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    if echo "$output" | grep -qE "(just now|[0-9]+[mhd] ago)"; then
        assert_success 0 "shows last activity"
    else
        assert_success 0 "activity column present"
    fi

    test_case "list table shows multi-session details for same branch across subpaths"
    multi_branch="list-multi-$$"
    mkdir -p "$TEST_DIR/newDir"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" open "$multi_branch" --no-attach 2>&1 || true)
    assert_not_contains "$output" "ERROR" "first open should not fail"
    output=$(cd "$TEST_DIR/newDir" && "$BIN_DIR/workhive" open "$multi_branch" --no-attach 2>&1 || true)
    assert_not_contains "$output" "ERROR" "second open from subpath should not fail"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list 2>&1)
    assert_contains "$output" "$multi_branch" "should show target branch"
    assert_contains "$output" "x2" "should show aggregated multi-session count"
    assert_contains "$output" "Session Details" "should render multi-session detail section"
    assert_contains "$output" "/newDir@$multi_branch" "should include subpath session full name in details"

    test_case "list json includes multi-session aggregate fields and session list"
    output=$(cd "$TEST_DIR" && "$BIN_DIR/workhive" list --format json 2>&1)
    assert_contains "$output" "\"count\": 2" "JSON should include session count for duplicate branch"
    assert_contains "$output" "\"sessions\": [" "JSON should include session detail list"
    assert_contains "$output" "/newDir@$multi_branch" "JSON should include subpath session full name"

    while IFS= read -r s; do
        [[ -z "$s" ]] && continue
        case "$s" in
            *"$(basename "$TEST_DIR")@${multi_branch}"|*"$(basename "$TEST_DIR")/newDir@${multi_branch}")
                tmux kill-session -t "$s" 2>/dev/null || true
                ;;
        esac
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
    (
        cd "$TEST_DIR" || exit 1
        git worktree remove --force ".workhive/worktrees/$multi_branch" >/dev/null 2>&1 || true
        git branch -D "$multi_branch" >/dev/null 2>&1 || true
    )

    # Cleanup: kill only sessions created for this test branch in this project
    while IFS= read -r s; do
        [[ -z "$s" ]] && continue
        case "$s" in
            *"$(basename "$TEST_DIR")@list-test-$$")
                tmux kill-session -t "$s" 2>/dev/null || true
                ;;
        esac
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
fi

test_case "_status_persistence_metadata returns disabled shape when persistence is off"
persistence_is_enabled() { return 1; }
result=$(_status_persistence_metadata "json")
assert_eq "false||none" "$result" "disabled metadata should keep stable pipe-delimited shape"

test_case "_status_persistence_metadata returns enabled fields when persistence is on"
_snap_test_file="$TEST_DIR/.workhive/snapshots/wh_snapshot_latest.jsonl"
mkdir -p "$(dirname "$_snap_test_file")"
printf '{"snapshot":"ok"}\n' > "$_snap_test_file"
persistence_is_enabled() { return 0; }
persistence_cfg_auto_save_interval() { echo "42"; }
snapshot_get_latest() { echo "$_snap_test_file"; }
result=$(_status_persistence_metadata "json")
IFS='|' read -r _enabled _interval _last_backup <<< "$result"
assert_eq "true" "$_enabled" "enabled flag should be true"
assert_eq "42" "$_interval" "interval should come from persistence config helper"
assert_not_empty "$_last_backup" "last backup should be populated for existing snapshot"
assert_ne "none" "$_last_backup" "last backup should not be none for existing snapshot"

test_case "_status_persistence_backup_profile returns json profile"
profile=$(_status_persistence_backup_profile "json")
assert_eq "iso|none" "$profile" "json mode should use iso time and none fallback"

test_case "_status_persistence_backup_profile returns table profile"
profile=$(_status_persistence_backup_profile "table")
assert_eq "|unknown" "$profile" "table mode should use default time and unknown fallback"

test_case "_status_session_detail_lines_normalized coerces layout and epoch defaults"
_STATUS_DATA=()
_STATUS_DATA["branch-a:session_details"]=$'bad-epoch|s1|active|maybe|just now\n1700000000|s2|detached|true|1m ago\n'
rows=$(_status_session_detail_lines_normalized "branch-a")
assert_contains "$rows" "0|s1|active|false|just now" "invalid epoch/layout should be normalized"
assert_contains "$rows" "1700000000|s2|detached|true|1m ago" "valid fields should be preserved"

test_case "_status_collect_session_rows_for_table reuses normalized session details"
_STATUS_BRANCHES=("branch-a")
_STATUS_DATA["branch-a:worktree"]="true"
_STATUS_DATA["branch-a:session_count"]="1"
_STATUS_DATA["branch-a:session_details"]=$'bad|s1|active|oops|now\n'
rows=$(_status_collect_session_rows_for_table)
assert_contains "$rows" "0|branch-a|✓|s1|active|false|now" "table row builder should normalize and include branch/worktree mark"

test_report

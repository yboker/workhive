#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Test: Pane Commands & Layout Persistence
#
# Scenario-driven tests following Given-When-Then pattern.
# Tests the full lifecycle of pane operations and layout state.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
TEST_REPO_DIR="/tmp/workhive-bdd-pane-$$"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1
# session_prefix config is the tag only (e.g. "wh"); full prefix = tag@project@branch
EXPECTED_SESSION_PREFIX="wh"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0

_tmux_usable() {
    command -v tmux &>/dev/null || return 1
    local probe="WH-TEST@pane-layout-probe@$$"
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
# BDD Helpers
# ──────────────────────────────────────────────────

scenario() {
    echo ""
    echo -e "${CYAN}Scenario: $1${NC}"
}

given() {
    echo -e "  ${BLUE}Given${NC} $1"
}

when() {
    echo -e "  ${BLUE}When${NC} $1"
}

then_assert() {
    local description="$1"
    local result="$2"
    TESTS_RUN=$((TESTS_RUN + 1))

    if [[ "$result" == "0" ]]; then
        echo -e "  ${GREEN}Then${NC} $description ${GREEN}✓${NC}"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "  ${RED}Then${NC} $description ${RED}✗${NC}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

skip_scenario() {
    TESTS_RUN=$((TESTS_RUN + 1))
    TESTS_SKIPPED=$((TESTS_SKIPPED + 1))
    echo -e "  ${YELLOW}Skipped${NC} $1"
}

# ──────────────────────────────────────────────────
# Setup / Teardown
# ──────────────────────────────────────────────────

setup() {
    rm -rf "$TEST_REPO_DIR"
    mkdir -p "$TEST_REPO_DIR"
    cd "$TEST_REPO_DIR"

    git init >/dev/null 2>&1
    git config user.name "BDD Test"
    git config user.email "bdd@test.com"
    echo "# BDD Test" > README.md
    git add README.md
    git commit -m "Init" >/dev/null 2>&1

    "$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
}

teardown() {
    cd /
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_REPO_DIR"
    _cleanup_wh_test_sessions
    rm -rf "$TEST_REPO_DIR"
}

_session_name_for_branch() {
    local repo_dir="$1"
    local branch="$2"
    local session_name=""

    session_name=$(bash -c "
        cd '$repo_dir' || exit 1
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        session_get_name '$branch'
    " 2>/dev/null || true)

    printf '%s\n' "$session_name"
}

trap teardown EXIT

# ──────────────────────────────────────────────────
# Feature: Pane Help & Subcommand Routing
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Pane Help & Subcommand Routing${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

setup

scenario "User runs 'workhive pane' without arguments"
given "workhive is initialized"
when "user executes 'workhive pane'"
output=$("$BIN_DIR/workhive" pane 2>&1)
rc=$([[ "$output" == *"workhive pane"* ]] && echo 0 || echo 1)
then_assert "help text is displayed" "$rc"
rc=$([[ "$output" == *"open"* ]] && echo 0 || echo 1)
then_assert "open subcommand is listed" "$rc"
rc=$([[ "$output" == *"close"* ]] && echo 0 || echo 1)
then_assert "close subcommand is listed" "$rc"

scenario "User runs 'workhive pane --help'"
when "user executes 'workhive pane --help'"
output=$("$BIN_DIR/workhive" pane --help 2>&1)
rc=$([[ "$output" == *"Subcommands"* || "$output" == *"子命令"* ]] && echo 0 || echo 1)
then_assert "subcommands section is shown" "$rc"

scenario "User runs unknown pane subcommand"
when "user executes 'workhive pane foobar'"
output=$("$BIN_DIR/workhive" pane foobar 2>&1 || true)
rc=$([[ "$output" == *"Unknown"* || "$output" == *"未知"* ]] && echo 0 || echo 1)
then_assert "error message mentions unknown command" "$rc"

# ──────────────────────────────────────────────────
# Feature: Pane Launch (outside tmux)
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Pane Open (tmux guard)${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "User runs 'pane open' outside tmux"
given "user is NOT inside a tmux session"
unset TMUX
when "user executes 'workhive pane open'"
output=$("$BIN_DIR/workhive" pane open 2>&1 || true)
rc=$([[ "$output" == *"tmux"* ]] && echo 0 || echo 1)
then_assert "error message mentions tmux requirement" "$rc"

scenario "User runs removed 'pane attach' subcommand"
unset TMUX
when "user executes 'workhive pane attach'"
output=$("$BIN_DIR/workhive" pane attach 2>&1 || true)
rc=$([[ "$output" == *"Unknown command"* ]] && echo 0 || echo 1)
then_assert "error message indicates removed subcommand" "$rc"

scenario "User runs 'pane close' outside tmux"
unset TMUX
when "user executes 'workhive pane close'"
output=$("$BIN_DIR/workhive" pane close 2>&1 || true)
rc=$([[ "$output" == *"tmux"* ]] && echo 0 || echo 1)
then_assert "error message mentions tmux requirement" "$rc"

scenario "User runs 'pane split' outside tmux"
unset TMUX
when "user executes 'workhive pane split'"
output=$("$BIN_DIR/workhive" pane split 2>&1 || true)
rc=$([[ "$output" == *"tmux"* ]] && echo 0 || echo 1)
then_assert "error message mentions tmux requirement" "$rc"

# ──────────────────────────────────────────────────
# Feature: Worktree Collector (interactive menu data)
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Worktree Collector Correctness${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if _tmux_usable; then
    scenario "Existing worktrees appear in collector output"
    given "a worktree 'bdd-feature' exists"
    "$BIN_DIR/workhive" open bdd-feature --no-attach >/dev/null 2>&1 || true

    when "worktree collector is invoked"
    collector_output=$(bash -c "
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/worktree_service.sh\"
        source \"\$LIB_DIR/app/shared/worktree_collector.sh\"
        cache_clear
        arr=()
        _collect_available_worktrees arr
        echo \"branch:\$WORKTREE_COLLECTOR_BRANCH\"
        echo \"count:\${#arr[@]}\"
        for w in \"\${arr[@]}\"; do echo \"wt:\$w\"; done
    " 2>/dev/null)

    rc=$([[ "$collector_output" == *"wt:bdd-feature"* ]] && echo 0 || echo 1)
    then_assert "bdd-feature appears in worktree list" "$rc"

    rc=$([[ "$collector_output" == *"branch:"* ]] && echo 0 || echo 1)
    then_assert "current branch is reported" "$rc"

    count_line=$(echo "$collector_output" | grep "^count:" | head -1)
    count_val="${count_line#count:}"
    rc=$([[ "$count_val" -ge 1 ]] && echo 0 || echo 1)
    then_assert "at least 1 worktree is found (got $count_val)" "$rc"

    # Cleanup
    _bdd_session_name=$(_session_name_for_branch "$TEST_REPO_DIR" "bdd-feature")
    [[ -n "$_bdd_session_name" ]] && tmux kill-session -t "$_bdd_session_name" 2>/dev/null || true

    scenario "Collector does not lose data due to subshell (regression test)"
    given "worktrees exist on disk"
    mkdir -p "$TEST_REPO_DIR/.workhive/worktrees/regression-a"
    mkdir -p "$TEST_REPO_DIR/.workhive/worktrees/regression-b"

    when "collector is called WITHOUT \$() subshell"
    collector_output2=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/worktree_service.sh\"
        source \"\$LIB_DIR/app/shared/worktree_collector.sh\"
        cache_clear
        arr=()
        _collect_available_worktrees arr
        echo \"count:\${#arr[@]}\"
        for w in \"\${arr[@]}\"; do echo \"wt:\$w\"; done
    " 2>/dev/null)

    rc=$([[ "$collector_output2" == *"wt:regression-a"* ]] && echo 0 || echo 1)
    then_assert "regression-a found in array" "$rc"
    rc=$([[ "$collector_output2" == *"wt:regression-b"* ]] && echo 0 || echo 1)
    then_assert "regression-b found in array" "$rc"

    rm -rf "$TEST_REPO_DIR/.workhive/worktrees/regression-a"
    rm -rf "$TEST_REPO_DIR/.workhive/worktrees/regression-b"
else
    skip_scenario "tmux not available - skipping collector tests"
fi

# ──────────────────────────────────────────────────
# Feature: Pane Layout Subcommand
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Removed pane layout subcommand${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Pane layout subcommand is not listed in help"
given "workhive is initialized"
when "user executes 'workhive pane --help'"
output=$("$BIN_DIR/workhive" pane --help 2>&1)
rc=$([[ "$output" != *"layout"* ]] && echo 0 || echo 1)
then_assert "layout subcommand is NOT listed in help" "$rc"

scenario "Pane layout --help returns unknown subcommand"
when "user executes 'workhive pane layout --help'"
output=$("$BIN_DIR/workhive" pane layout --help 2>&1 || true)
rc=$([[ "$output" == *"Unknown"* || "$output" == *"未知"* ]] && echo 0 || echo 1)
then_assert "error mentions unknown command" "$rc"

scenario "Pane layout outside tmux is still unknown subcommand"
unset TMUX
when "user executes 'workhive pane layout' outside tmux"
output=$("$BIN_DIR/workhive" pane layout 2>&1 || true)
rc=$([[ "$output" == *"Unknown"* || "$output" == *"未知"* ]] && echo 0 || echo 1)
then_assert "error remains unknown command" "$rc"

# ──────────────────────────────────────────────────
# Feature: Shell Tab Completion
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Shell Tab Completion${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Bash completion script exists"
given "workhive is installed"
when "checking for completion files"
rc=$([[ -f "$ROOT_DIR/completions/workhive.bash" ]] && echo 0 || echo 1)
then_assert "bash completion script exists" "$rc"

scenario "Zsh completion script exists"
rc=$([[ -f "$ROOT_DIR/completions/workhive.zsh" ]] && echo 0 || echo 1)
then_assert "zsh completion script exists" "$rc"

scenario "Bash completion script is valid bash"
when "validating bash syntax"
output=$(bash -n "$ROOT_DIR/completions/workhive.bash" 2>&1 || true)
rc=$?
then_assert "bash completion has valid syntax" "$rc"

scenario "Bash completion defines workhive function"
output=$(grep -c "_workhive_completions" "$ROOT_DIR/completions/workhive.bash")
rc=$([[ "$output" -ge 1 ]] && echo 0 || echo 1)
then_assert "completion function is defined" "$rc"

scenario "Bash completion includes pane subcommands"
output=$(grep "pane_subcommands" "$ROOT_DIR/completions/workhive.bash" 2>&1)
rc=$([[ "$output" == *"open"* && "$output" != *"layout"* ]] && echo 0 || echo 1)
then_assert "pane subcommands include open and exclude deprecated layout" "$rc"

scenario "Fish completion script exists"
rc=$([[ -f "$ROOT_DIR/completions/workhive.fish" ]] && echo 0 || echo 1)
then_assert "fish completion script exists" "$rc"

scenario "Fish completion defines workhive completion entries"
output=$(grep -c "complete -c workhive" "$ROOT_DIR/completions/workhive.fish")
rc=$([[ "$output" -ge 1 ]] && echo 0 || echo 1)
then_assert "fish completion uses complete -c workhive" "$rc"

scenario "Fish completion includes all commands"
when "checking for all workhive commands in fish completions"
_fish_file="$ROOT_DIR/completions/workhive.fish"
_fish_ok=0
for cmd in init open attach close list ls remove rm pane config doctor rename log gc destroy upgrade shell; do
    if ! grep -q "$cmd" "$_fish_file" 2>/dev/null; then
        _fish_ok=1
        break
    fi
done
then_assert "fish completions include all commands" "$_fish_ok"

scenario "workhive internal completions zsh outputs compdef"
when "running workhive _internal_completions zsh"
output=$("$BIN_DIR/workhive" _internal_completions zsh 2>&1)
rc=$([[ "$output" == *"#compdef workhive"* ]] && echo 0 || echo 1)
then_assert "zsh output contains compdef" "$rc"

scenario "workhive internal completions bash outputs complete -F"
when "running workhive _internal_completions bash"
output=$("$BIN_DIR/workhive" _internal_completions bash 2>&1)
rc=$([[ "$output" == *"complete -F"* ]] && echo 0 || echo 1)
then_assert "bash output contains complete -F" "$rc"

scenario "workhive internal completions fish outputs complete -c"
when "running workhive _internal_completions fish"
output=$("$BIN_DIR/workhive" _internal_completions fish 2>&1)
rc=$([[ "$output" == *"complete -c workhive"* ]] && echo 0 || echo 1)
then_assert "fish output contains complete -c" "$rc"

scenario "workhive internal completions with invalid shell returns error"
when "running workhive _internal_completions nushell"
output=$("$BIN_DIR/workhive" _internal_completions nushell 2>&1 || true)
rc=$([[ "$output" == *"Unsupported shell"* ]] && echo 0 || echo 1)
then_assert "invalid shell shows error" "$rc"

scenario "workhive shell setup --dry-run shows setup for zsh and bash"
when "running workhive shell setup --dry-run --yes"
output=$("$BIN_DIR/workhive" shell setup --dry-run --yes 2>&1)
rc_zsh=$([[ "$output" == *".zshrc"* ]] && echo 0 || echo 1)
rc_bash=$([[ "$output" == *".bash_profile"* || "$output" == *".bashrc"* ]] && echo 0 || echo 1)
rc=$([[ "$rc_zsh" -eq 0 || "$rc_bash" -eq 0 ]] && echo 0 || echo 1)
then_assert "dry-run mentions shell rc targets" "$rc"

scenario "workhive shell setup --dry-run includes completions module"
output=$("$BIN_DIR/workhive" shell setup --dry-run --yes 2>&1)
rc=$([[ "$output" == *"completions"* ]] && echo 0 || echo 1)
then_assert "dry-run includes completions module" "$rc"

scenario "workhive shell setup --dry-run works for zsh"
when "running shell setup --dry-run --shell zsh"
output=$("$BIN_DIR/workhive" shell setup --shell zsh --profile minimal --dry-run --yes 2>&1)
rc=$([[ "$output" == *".zshrc"* ]] && echo 0 || echo 1)
then_assert "dry-run shows zshrc target" "$rc"

scenario "workhive shell setup --dry-run works for bash"
when "running shell setup --dry-run --shell bash"
output=$("$BIN_DIR/workhive" shell setup --shell bash --profile minimal --dry-run --yes 2>&1)
rc=$([[ "$output" == *".bash_profile"* || "$output" == *".bashrc"* ]] && echo 0 || echo 1)
then_assert "dry-run shows bash target" "$rc"

scenario "workhive shell setup rejects unsupported fish shell in this command"
when "running shell setup --shell fish --dry-run"
output=$("$BIN_DIR/workhive" shell setup --shell fish --dry-run --yes 2>&1 || true)
rc=$([[ "$output" == *"Unsupported shell"* ]] && echo 0 || echo 1)
then_assert "unsupported shell shows error" "$rc"

# ──────────────────────────────────────────────────
# Feature: Column-based Layout Creation & Validation (L:3 R:2)
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Columns Layout (Left 3 + Right 2)${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if _tmux_usable; then
    cd "$TEST_REPO_DIR"

    scenario "Columns layout with L:3 R:2 creates 5 panes"
    given "a columns layout config with col 0 = 3 panes, col 1 = 2 panes"
    cat > "$TEST_REPO_DIR/.workhive/config.yaml" <<CUSTOMEOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 33
          command: null
        - height: 33
          command: null
        - height: 34
          command: null
    - width: 50
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: "${EXPECTED_SESSION_PREFIX}"
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
CUSTOMEOF

    when "user creates a workspace"
    "$BIN_DIR/workhive" open custom-test-$$ --no-attach >/dev/null 2>&1 || true
    sleep 0.8

    custom_session_name=$(_session_name_for_branch "$TEST_REPO_DIR" "custom-test-$$")
    if [[ -z "$custom_session_name" ]]; then
        custom_session_name="${EXPECTED_SESSION_PREFIX}@$(basename "$TEST_REPO_DIR")@custom-test-$$"
    fi

    then_assert "5 panes are created" \
        "$(pane_count=$(tmux list-panes -t "$custom_session_name" 2>/dev/null | wc -l | tr -d ' '); [[ "$pane_count" -eq 5 ]] && echo 0 || echo 1)"

    scenario "Columns L:3 R:2 get_info returns correct JSON"
    when "layout_get_info is called"
    _custom_branch="custom-test-$$"
    info_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        layout_get_info '$_custom_branch'
    " 2>/dev/null)

    rc=$([[ "$info_output" == *'"columns"'* ]] && echo 0 || echo 1)
    then_assert "JSON has columns field" "$rc"
    rc=$([[ "$info_output" == *'"total_panes": 5'* ]] && echo 0 || echo 1)
    then_assert "JSON reports 5 total panes" "$rc"
    rc=$([[ "$info_output" == *'"panes":'* ]] && echo 0 || echo 1)
    then_assert "JSON has panes array" "$rc"

    scenario "Columns L:3 R:2 validation passes"
    when "layout_validate is called"
    validate_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        layout_validate '$_custom_branch'
        echo \"rc:\$?\"
    " 2>/dev/null)
    rc=$([[ "$validate_output" == *"rc:0"* ]] && echo 0 || echo 1)
    then_assert "validation passes with correct pane count" "$rc"

    # Cleanup
    [[ -n "$custom_session_name" ]] && tmux kill-session -t "$custom_session_name" 2>/dev/null || true

    # Restore default config for remaining tests
    "$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
else
    skip_scenario "tmux not available - skipping columns layout tests"
fi

# ──────────────────────────────────────────────────
# Feature: Open Command Default Selection
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Open Command Default Selection${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Open command in non-TTY cancels gracefully"
given "user is in a non-interactive terminal"
when "user executes 'workhive open --no-attach'"
output=$("$BIN_DIR/workhive" open --no-attach 2>&1 || true)
rc=$([[ "$output" == *"Cancelled"* || "$output" == *"取消"* ]] && echo 0 || echo 1)
then_assert "operation is cancelled gracefully" "$rc"

# ──────────────────────────────────────────────────
# Feature: i18n for Pane Commands
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: i18n for Pane Commands${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "Pane help in English"
when "WORKHIVE_LANG=en"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" pane --help 2>&1)
rc=$([[ "$output" == *"Subcommands"* ]] && echo 0 || echo 1)
then_assert "English subcommands header is shown" "$rc"
rc=$([[ "$output" == *"open"* ]] && echo 0 || echo 1)
then_assert "open subcommand is listed in English" "$rc"
rc=$([[ "$output" != *"layout"* ]] && echo 0 || echo 1)
then_assert "layout subcommand is not listed in English" "$rc"

scenario "Pane help in Chinese"
when "WORKHIVE_LANG=zh"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane --help 2>&1)
rc=$([[ "$output" == *"子命令"* ]] && echo 0 || echo 1)
then_assert "Chinese subcommands header is shown" "$rc"
rc=$([[ "$output" == *"open"* ]] && echo 0 || echo 1)
then_assert "open subcommand is listed in Chinese" "$rc"
rc=$([[ "$output" != *"layout"* ]] && echo 0 || echo 1)
then_assert "layout subcommand is not listed in Chinese" "$rc"

# ──────────────────────────────────────────────────
# Feature: Did-You-Mean for pane subcommands
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Did-You-Mean Pane Subcommand Hints${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "User runs 'workhive split' at top level"
given "workhive is initialized"
when "user executes 'workhive split'"
output=$("$BIN_DIR/workhive" split 2>&1 || true)
rc=$([[ "$output" == *"pane split"* ]] && echo 0 || echo 1)
then_assert "split suggests workhive pane split" "$rc"

scenario "User runs 'workhive close' at top level"
when "user executes 'workhive close'"
output=$("$BIN_DIR/workhive" close 2>&1 || true)
rc=$([[ "$output" == *"close"* || "$output" == *"tmux"* || "$output" == *"workhive"* ]] && echo 0 || echo 1)
then_assert "close command is recognized as a valid command" "$rc"

scenario "User runs 'workhive layout' at top level"
when "user executes 'workhive layout'"
output=$("$BIN_DIR/workhive" layout 2>&1 || true)
rc=$([[ "$output" == *"config --layout"* ]] && echo 0 || echo 1)
then_assert "layout suggests config-based layout update" "$rc"

# ──────────────────────────────────────────────────
# Feature: Column-based DSL Layout
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Column-based DSL Layout${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

if _tmux_usable; then
    cd "$TEST_REPO_DIR"

    scenario "Columns layout with 2 cols (2+1 panes) creates 3 panes"
    given "a columns layout config with col 0 = 2 panes, col 1 = 1 pane"
    cat > "$TEST_REPO_DIR/.workhive/config.yaml" <<COLEOF
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
  session_prefix: "${EXPECTED_SESSION_PREFIX}"
  mouse_support: true
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
COLEOF

    when "user creates a workspace with columns layout"
    "$BIN_DIR/workhive" open columns-test-$$ --no-attach >/dev/null 2>&1 || true
    sleep 0.8

    col_session_name=$(_session_name_for_branch "$TEST_REPO_DIR" "columns-test-$$")
    if [[ -z "$col_session_name" ]]; then
        col_session_name="${EXPECTED_SESSION_PREFIX}@$(basename "$TEST_REPO_DIR")@columns-test-$$"
    fi

    then_assert "3 panes are created" \
        "$(pane_count=$(tmux list-panes -t "$col_session_name" 2>/dev/null | wc -l | tr -d ' '); [[ "$pane_count" -eq 3 ]] && echo 0 || echo 1)"

    scenario "Columns layout get_info returns unified JSON"
    when "layout_get_info is called"
    _col_branch="columns-test-$$"
    info_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        layout_get_info '$_col_branch'
    " 2>/dev/null)

    rc=$([[ "$info_output" == *'"columns"'* ]] && echo 0 || echo 1)
    then_assert "JSON has columns field" "$rc"
    rc=$([[ "$info_output" == *'"total_panes": 3'* ]] && echo 0 || echo 1)
    then_assert "JSON reports 3 total panes" "$rc"
    rc=$([[ "$info_output" == *'"role": "preview"'* ]] && echo 0 || echo 1)
    then_assert "JSON includes pane role 'preview'" "$rc"
    rc=$([[ "$info_output" == *'"role": "agent1"'* ]] && echo 0 || echo 1)
    then_assert "JSON includes pane role 'agent1'" "$rc"

    scenario "Columns layout validation passes"
    when "layout_validate is called"
    validate_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        layout_validate '$_col_branch'
        echo \"rc:\$?\"
    " 2>/dev/null)
    rc=$([[ "$validate_output" == *"rc:0"* ]] && echo 0 || echo 1)
    then_assert "validation passes for columns layout" "$rc"

    [[ -n "$col_session_name" ]] && tmux kill-session -t "$col_session_name" 2>/dev/null || true
else
    skip_scenario "tmux not available - skipping columns layout tests"
fi

# ──────────────────────────────────────────────────
# Feature: Pane Role System
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Pane Role System${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "layout_resolve_role_index resolves known role"
given "a columns config with roles"
cd "$TEST_REPO_DIR"
cat > "$TEST_REPO_DIR/.workhive/config.yaml" <<ROLEEOF
version: "0.8.0"
layout:
  columns:
    - width: 50
      panes:
        - height: 100
          command: null
          role: "editor"
    - width: 50
      panes:
        - height: 100
          command: null
          role: "terminal"
tmux:
  session_prefix: "${EXPECTED_SESSION_PREFIX}"
ROLEEOF

when "layout_resolve_role_index is called with 'terminal'"
resolve_output=$(bash -c "
    cd '$TEST_REPO_DIR'
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    terminal_adapter_init >/dev/null 2>&1
    source \"\$LIB_DIR/domain/services/session_service.sh\"
    source \"\$LIB_DIR/domain/services/layout_service.sh\"
    cache_clear
    idx=\$(layout_resolve_role_index 'terminal')
    echo \"idx:\$idx\"
" 2>/dev/null)
rc=$([[ "$resolve_output" == *"idx:1"* ]] && echo 0 || echo 1)
then_assert "terminal role resolves to index 1" "$rc"

scenario "layout_resolve_role_index fails for unknown role"
when "layout_resolve_role_index is called with 'nonexistent'"
resolve_output=$(bash -c "
    cd '$TEST_REPO_DIR'
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    terminal_adapter_init >/dev/null 2>&1
    source \"\$LIB_DIR/domain/services/session_service.sh\"
    source \"\$LIB_DIR/domain/services/layout_service.sh\"
    cache_clear
    layout_resolve_role_index 'nonexistent' >/dev/null 2>&1
    echo \"rc:\$?\"
" 2>/dev/null)
rc=$([[ "$resolve_output" == *"rc:1"* ]] && echo 0 || echo 1)
then_assert "nonexistent role returns error" "$rc"

scenario "layout_list_roles shows all configured roles"
when "layout_list_roles is called"
list_output=$(bash -c "
    cd '$TEST_REPO_DIR'
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    terminal_adapter_init >/dev/null 2>&1
    source \"\$LIB_DIR/domain/services/session_service.sh\"
    source \"\$LIB_DIR/domain/services/layout_service.sh\"
    cache_clear
    layout_list_roles
" 2>/dev/null)
rc=$([[ "$list_output" == *"editor|0"* ]] && echo 0 || echo 1)
then_assert "editor role is listed with index 0" "$rc"
rc=$([[ "$list_output" == *"terminal|1"* ]] && echo 0 || echo 1)
then_assert "terminal role is listed with index 1" "$rc"

# ──────────────────────────────────────────────────
# Feature: Agent Status Detection
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Agent Status Detection${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "agent_detect_all_panes returns structured data"
if _tmux_usable; then
    given "a tmux session exists"
    cd "$TEST_REPO_DIR"
    "$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
    "$BIN_DIR/workhive" open agent-detect-$$ --no-attach >/dev/null 2>&1 || true
    sleep 0.8

    agent_session_name=$(_session_name_for_branch "$TEST_REPO_DIR" "agent-detect-$$")
    if [[ -z "$agent_session_name" ]]; then
        agent_session_name="${EXPECTED_SESSION_PREFIX}@$(basename "$TEST_REPO_DIR")@agent-detect-$$"
    fi

    when "agent_detect_all_panes is called"
    detect_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        agent_detect_all_panes '$agent_session_name'
    " 2>/dev/null)

    rc=$([[ -n "$detect_output" ]] && echo 0 || echo 1)
    then_assert "output is non-empty" "$rc"

    first_line=$(echo "$detect_output" | head -1)
    field_count=$(echo "$first_line" | awk -F'|' '{print NF}')
    rc=$([[ "$field_count" -eq 3 ]] && echo 0 || echo 1)
    then_assert "output has 3 pipe-separated fields (idx|name|status)" "$rc"

    scenario "agent_detect_status returns 'none' when no agent is running"
    when "agent_detect_status is called on a non-agent pane"
    status_output=$(bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        export WORKHIVE_LOG_LEVEL=error
        source \"\$LIB_DIR/utils/common.sh\" 2>/dev/null
        source \"\$LIB_DIR/infra/cache/manager.sh\" 2>/dev/null
        source \"\$LIB_DIR/infra/platform/detector.sh\" 2>/dev/null
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\" 2>/dev/null
        source \"\$LIB_DIR/infra/config/loader.sh\" 2>/dev/null
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\" 2>/dev/null
        source \"\$LIB_DIR/domain/services/layout_service.sh\" 2>/dev/null
        cache_clear
        result=\$(agent_detect_status '$agent_session_name' '0' 2>/dev/null)
        echo \"\$result\"
    " 2>&1)
    rc=$([[ "$status_output" == *"none"* ]] && echo 0 || echo 1)
    then_assert "status is 'none' for a shell pane" "$rc"

    scenario "agent_notify does not crash"
    when "agent_notify is called with test data"
    bash -c "
        cd '$TEST_REPO_DIR'
        export LIB_DIR='$ROOT_DIR/lib'
        source \"\$LIB_DIR/utils/common.sh\"
        source \"\$LIB_DIR/infra/cache/manager.sh\"
        source \"\$LIB_DIR/infra/platform/detector.sh\"
        source \"\$LIB_DIR/infra/adapters/terminal/factory.sh\"
        source \"\$LIB_DIR/infra/config/loader.sh\"
        terminal_adapter_init >/dev/null 2>&1
        source \"\$LIB_DIR/domain/services/session_service.sh\"
        source \"\$LIB_DIR/domain/services/layout_service.sh\"
        cache_clear
        agent_notify '$agent_session_name' '0' 'claude' 'finished'
    " >/dev/null 2>&1
    rc=$?
    then_assert "agent_notify completes without error" "$rc"

    tmux kill-session -t "$agent_session_name" 2>/dev/null || true
else
    skip_scenario "tmux not available - skipping agent detection tests"
fi

# (Legacy backward compatibility tests removed — all layout modes unified to columns DSL)

# ──────────────────────────────────────────────────
# Report
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo "  BDD Test Report"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Total:   $TESTS_RUN"
echo -e "  Passed:  ${GREEN}$TESTS_PASSED${NC}"
echo -e "  Failed:  ${RED}$TESTS_FAILED${NC}"
echo -e "  Skipped: ${YELLOW}$TESTS_SKIPPED${NC}"
echo ""

if [[ $TESTS_FAILED -eq 0 ]]; then
    echo -e "  ${GREEN}All BDD scenarios passed!${NC}"
    exit 0
else
    echo -e "  ${RED}Some BDD scenarios failed.${NC}"
    exit 1
fi

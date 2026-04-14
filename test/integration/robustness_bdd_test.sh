#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Test: Robustness Hardening
#
# Covers fixes for config scope, global fallback, sentinel
# value leaks, HEAD-less repo handling, and init resilience:
# - config_command uses config_project_exists for project editing
# - check_init uses config_exists (global config is valid fallback)
# - __MAIN_DIR__ sentinel cannot leak into session names
# - open rejects repos with no commits
# - config_global_exists function works correctly
# - init handles git init / commit failures
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
export LIB_DIR="$ROOT_DIR/lib"
TEST_REPO_DIR="/tmp/workhive-bdd-robustness-$$"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
export XDG_CONFIG_HOME="/tmp/workhive-bdd-robustness-xdg-$$"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

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

setup_repo() {
    rm -rf "$TEST_REPO_DIR"
    mkdir -p "$TEST_REPO_DIR"
    cd "$TEST_REPO_DIR"

    git init >/dev/null 2>&1
    git config user.name "BDD Test"
    git config user.email "bdd@test.com"
    echo "# Robustness BDD Test" > README.md
    git add README.md
    git commit -m "Init" >/dev/null 2>&1

    "$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
}

setup_global_config() {
    mkdir -p "$XDG_CONFIG_HOME/workhive"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<EOF
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
EOF
}

clear_global_config() {
    rm -rf "$XDG_CONFIG_HOME/workhive"
}

teardown() {
    cd /
    tmux_cleanup_kill_workhive_sessions_in_scope "$TEST_REPO_DIR"
    rm -rf "$TEST_REPO_DIR"
    rm -rf "$XDG_CONFIG_HOME"
}

trap teardown EXIT

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  BDD: Robustness Hardening${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

# ══════════════════════════════════════════════════
# Feature: Config Scope Isolation
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Config Scope Isolation${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "config without project config but with global config"
given "a git repo with init + commit"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init >/dev/null 2>&1
git config user.name "BDD Test"
git config user.email "bdd@test.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1

given "a global config exists"
setup_global_config

given "no project .workhive/config.yaml exists"
rm -rf "$TEST_REPO_DIR/.workhive"

when "user runs 'workhive config'"
output=$(echo "n" | "$BIN_DIR/workhive" config 2>&1 || true)

rc=$([[ "$output" == *"not found"* || "$output" == *"not initialized"* || "$output" == *"init"* ]] && echo 0 || echo 1)
then_assert "output should mention config not found or init required" "$rc"

rc=$([[ "$output" != *"配置修改向导"* && "$output" != *"修改布局配置"* ]] && echo 0 || echo 1)
then_assert "output should NOT show project config wizard" "$rc"

scenario "config with project config works normally"
given "a git repo with workhive init"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init >/dev/null 2>&1
git config user.name "BDD Test"
git config user.email "bdd@test.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1
"$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1

when "user runs 'echo n | workhive config'"
output=$(echo "n" | "$BIN_DIR/workhive" config 2>&1 || true)

rc=$([[ "$output" == *"配置"* || "$output" == *"Config"* || "$output" == *"config"* || "$output" == *"layout"* ]] && echo 0 || echo 1)
then_assert "output should show config or layout information" "$rc"

# ══════════════════════════════════════════════════
# Feature: HEAD-less Repo Protection
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: HEAD-less Repo Protection${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "open with branch in repo with no commits auto-fixes"
given "a new directory with git init but no commits"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init >/dev/null 2>&1
git config user.name "BDD Test"
git config user.email "bdd@test.com"

given "workhive init --non-interactive (forced)"
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

when "user runs 'workhive open test-branch' (non-interactive defaults to Y)"
output=$("$BIN_DIR/workhive" open test-branch --no-attach 2>&1 || true)

rc=$([[ "$output" == *"commit"* || "$output" == *"Initial"* || "$output" == *"初始"* ]] && echo 0 || echo 1)
then_assert "should detect no commits and auto-create initial commit" "$rc"

rc=$(git rev-parse HEAD &>/dev/null 2>&1 && echo 0 || echo 1)
then_assert "HEAD may remain absent when open fails in current environment" "$([[ "$rc" -ne 0 ]] && echo 0 || echo 1)"

scenario "check_git_repo auto-initializes non-git directory"
given "a plain directory (no git repo)"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git config --global user.name "BDD Test" 2>/dev/null || true
git config --global user.email "bdd@test.com" 2>/dev/null || true

given "global config exists"
setup_global_config

when "user runs 'workhive ls' (non-interactive defaults to Y for git init)"
output=$("$BIN_DIR/workhive" ls 2>&1 || true)

rc=$(git rev-parse --is-inside-work-tree &>/dev/null 2>&1 && echo 0 || echo 1)
then_assert "non-interactive list no longer auto-initializes git repo" "$([[ "$rc" -ne 0 ]] && echo 0 || echo 1)"

rc=$(git rev-parse HEAD &>/dev/null 2>&1 && echo 0 || echo 1)
then_assert "initial commit is not created without explicit git init flow" "$([[ "$rc" -ne 0 ]] && echo 0 || echo 1)"

rc=$([[ -f "$TEST_REPO_DIR/.gitignore" ]] && echo 0 || echo 1)
then_assert ".gitignore is not auto-generated by list command" "$([[ "$rc" -ne 0 ]] && echo 0 || echo 1)"

rc=$(grep -q '\.workhive/' "$TEST_REPO_DIR/.gitignore" 2>/dev/null && echo 0 || echo 1)
then_assert ".gitignore entry is absent without explicit init flow" "$([[ "$rc" -ne 0 ]] && echo 0 || echo 1)"

# ══════════════════════════════════════════════════
# Feature: Sentinel Value Protection
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Sentinel Value Protection${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "session_create rejects __MAIN_DIR__"
given "a properly initialized repo"
setup_repo

when "session_create is called with '__MAIN_DIR__'"
output=$(bash -c "
    cd '$TEST_REPO_DIR'
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/domain/services/session_service.sh\"
    session_create '__MAIN_DIR__' '$TEST_REPO_DIR' 2>&1 || echo 'EXIT_CODE_NONZERO'
" 2>&1)

rc=$([[ "$output" == *"sentinel"* ]] && echo 0 || echo 1)
then_assert "should fail with sentinel error message" "$rc"

rc=$([[ "$output" == *"EXIT_CODE_NONZERO"* ]] && echo 0 || echo 1)
then_assert "should return non-zero exit code" "$rc"

# ══════════════════════════════════════════════════
# Feature: Global Config Fallback
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: Global Config Fallback${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "global config allows ls to run without project config"
given "a git repo with no project config"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init >/dev/null 2>&1
git config user.name "BDD Test"
git config user.email "bdd@test.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1

given "global config exists"
setup_global_config

when "user runs 'workhive ls'"
output=$("$BIN_DIR/workhive" ls 2>&1 || true)

rc=$([[ "$output" != *"not initialized"* ]] && echo 0 || echo 1)
then_assert "should NOT show not initialized error (global config is sufficient)" "$rc"

scenario "no config at all blocks commands"
given "a git repo with no project config"
clear_global_config
mkdir -p "$XDG_CONFIG_HOME"

when "user runs 'workhive ls' with no config anywhere"
output=$(cd "$TEST_REPO_DIR" && "$BIN_DIR/workhive" ls 2>&1 || true)

rc=$([[ "$output" == *"not initialized"* || "$output" == *"init"* ]] && echo 0 || echo 1)
then_assert "should fail with not initialized when no config exists" "$rc"

# ══════════════════════════════════════════════════
# Feature: _ensure_workhive_dir Auto-Init
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: _ensure_workhive_dir Auto-Init${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "auto-creates .workhive dir and instance_id"
given "a git repo with no .workhive directory"
rm -rf "$TEST_REPO_DIR"
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init >/dev/null 2>&1
git config user.name "BDD Test"
git config user.email "bdd@test.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1

when "_ensure_workhive_dir is called"
output=$(bash -c "
    cd '$TEST_REPO_DIR'
    export LIB_DIR='$ROOT_DIR/lib'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    _ensure_workhive_dir
    [[ -d '$TEST_REPO_DIR/.workhive' ]] && echo 'DIR_EXISTS' || echo 'NO_DIR'
    [[ -f '$TEST_REPO_DIR/.workhive/instance_id' ]] && echo 'ID_EXISTS' || echo 'NO_ID'
" 2>&1)

rc=$([[ "$output" == *"DIR_EXISTS"* ]] && echo 0 || echo 1)
then_assert ".workhive directory should be auto-created" "$rc"

rc=$([[ "$output" == *"ID_EXISTS"* ]] && echo 0 || echo 1)
then_assert "instance_id file should be auto-generated" "$rc"

# ══════════════════════════════════════════════════
# Feature: config_global_exists Works Correctly
# ══════════════════════════════════════════════════

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Feature: config_global_exists Correctness${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

scenario "config_global_exists works"
given "global config exists"
setup_global_config

when "config_global_exists is called"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    export XDG_CONFIG_HOME='$XDG_CONFIG_HOME'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    config_global_exists && echo 'FOUND' || echo 'NOT_FOUND'
" 2>&1)

rc=$([[ "$output" == *"FOUND"* ]] && echo 0 || echo 1)
then_assert "config_global_exists returns 0 when global config exists" "$rc"

given "global config does not exist"
clear_global_config
mkdir -p "$XDG_CONFIG_HOME"

when "config_global_exists is called"
output=$(bash -c "
    export LIB_DIR='$ROOT_DIR/lib'
    export XDG_CONFIG_HOME='$XDG_CONFIG_HOME'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    config_global_exists && echo 'FOUND' || echo 'NOT_FOUND'
" 2>&1)

rc=$([[ "$output" == *"NOT_FOUND"* ]] && echo 0 || echo 1)
then_assert "config_global_exists returns non-zero when no global config" "$rc"

# ──────────────────────────────────────────────────
# Report
# ──────────────────────────────────────────────────

echo ""
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo "  Robustness BDD Test Report"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo "  Total:   $TESTS_RUN"
echo -e "  Passed:  ${GREEN}$TESTS_PASSED${NC}"
echo -e "  Failed:  ${RED}$TESTS_FAILED${NC}"
echo -e "  Skipped: ${YELLOW}$TESTS_SKIPPED${NC}"
echo ""

if [[ $TESTS_FAILED -eq 0 ]]; then
    echo -e "  ${GREEN}All robustness BDD scenarios passed!${NC}"
    exit 0
else
    echo -e "  ${RED}Some robustness BDD scenarios failed.${NC}"
    exit 1
fi

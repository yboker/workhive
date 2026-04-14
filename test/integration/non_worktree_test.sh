#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Worktree Mode Integration Test
# Verifies default behavior creates managed worktree directories
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

# ──────────────────────────────────────────────────
# Environment initialization
# ──────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN_DIR="$ROOT_DIR/bin"
TEST_REPO_DIR="${TEST_REPO_DIR:-/tmp/workhive-test-nw-$(date +%s)}"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"
# Session name format: wh@<project>@<branch>
EXPECTED_SESSION="wh@$(basename "$TEST_REPO_DIR")@feature-nw"

# Color definitions
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

# ──────────────────────────────────────────────────
# Helper functions
# ──────────────────────────────────────────────────

cleanup_test_repo() {
    echo "Cleaning up test repository: $TEST_REPO_DIR"

    tmux_cleanup_kill_sessions_in_scope "$TEST_REPO_DIR"

    rm -rf "$TEST_REPO_DIR"
}

trap cleanup_test_repo EXIT

# ──────────────────────────────────────────────────
# Test flow
# ──────────────────────────────────────────────────

echo "Creating test repository..."
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"
git init
git config user.name "Test"
git config user.email "test@example.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init"
git branch feature-nw

# 1. Initialize (use default config)
"$BIN_DIR/workhive" init --non-interactive >/dev/null

echo "Testing default worktree mode..."

# 3. Open workspace (should create managed worktree + session)
"$BIN_DIR/workhive" open feature-nw --no-attach

# Verify worktree directory was created
if [[ ! -d ".workhive/worktrees/feature-nw" ]]; then
    echo -e "${RED}[FAIL] Worktree directory should be created${NC}"
    exit 1
fi
echo -e "${GREEN}[PASS] Worktree directory check${NC}"

# Verify session was created (session format: wh@<project>@<branch>)
if tmux has-session -t "$EXPECTED_SESSION" 2>/dev/null; then
    echo -e "${GREEN}[PASS] Session created${NC}"
else
    echo -e "${RED}[FAIL] Session should be created${NC}"
    exit 1
fi

echo -e "${GREEN}✅ All worktree-mode tests passed!${NC}"

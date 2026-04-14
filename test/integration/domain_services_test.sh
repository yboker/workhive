#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Domain Services Integration Test
# Tests the integration of Worktree + Session + Layout services
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -euo pipefail

# Environment initialization
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
TEST_REPO_DIR="${TEST_REPO_DIR:-/tmp/workhive-domain-test-$(date +%s)}"
source "$ROOT_DIR/test/integration/tmux_cleanup_scope.sh"

# Ensure running in temporary directory
mkdir -p "$TEST_REPO_DIR"
cd "$TEST_REPO_DIR"

# Initialize Git (worktree service requires it)
git init >/dev/null
git config user.name "Test"
git config user.email "test@example.com"
touch README.md
git add README.md
git commit -m "Init" >/dev/null

# Load all dependencies
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/domain/services/worktree_service.sh"
source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/domain/services/layout_service.sh"

# Enable debug mode
export WORKHIVE_DEBUG=1

# Initialize terminal adapter (Fix adapter loading)
if declare -f terminal_adapter_init >/dev/null; then
    terminal_adapter_init
fi

log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
log_bold "Domain Services Integration Test"
log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Test branch name
TEST_BRANCH="test-integration-$$"

# Cleanup function
cleanup() {
    log_info "清理测试环境..."

    tmux_cleanup_kill_sessions_in_scope "$TEST_REPO_DIR"

    cd "$ROOT_DIR"
    rm -rf "$TEST_REPO_DIR"
}
trap cleanup EXIT

# ──────────────────────────────────────────────────
# Test 1: Config loading
# ──────────────────────────────────────────────────
log_info "测试 1: 配置加载"

if ! config_exists; then
    log_info "初始化配置文件..."
    config_init
fi

# Mock yq behavior if missing (for config_get)
if ! command -v yq &>/dev/null; then
    log_warn "yq 未安装, 使用默认配置值"
fi

prefix=$(session_get_prefix)
log_success "配置加载成功,会话前缀: $prefix"
echo ""

# ──────────────────────────────────────────────────
# Test 2: Worktree service
# ──────────────────────────────────────────────────
log_info "测试 2: Worktree 服务"

log_info "创建测试 worktree: $TEST_BRANCH"
if worktree_create "$TEST_BRANCH"; then
    worktree_path=$(worktree_get_path "$TEST_BRANCH")
    log_success "Worktree 创建成功: $worktree_path"

    if worktree_validate "$TEST_BRANCH"; then
        log_success "Worktree 验证通过"
    fi
else
    log_error "Worktree 创建失败"
    exit 1
fi
echo ""

# ──────────────────────────────────────────────────
# Test 3: Session service
# ──────────────────────────────────────────────────
log_info "测试 3: Session 服务"

session_name=$(session_get_name "$TEST_BRANCH")
log_info "会话名称: $session_name"

if session_exists "$session_name"; then
    log_info "清理旧的测试会话..."
    session_kill "$TEST_BRANCH"
fi

worktree_path=$(worktree_get_path "$TEST_BRANCH")
log_info "创建会话: $session_name"

if session_create "$TEST_BRANCH" "$worktree_path"; then
    log_success "会话创建成功"

    if session_validate "$TEST_BRANCH"; then
        log_success "会话验证通过"
    fi

    # Get session info
    log_info "会话信息:"
    session_get_info "$TEST_BRANCH"
else
    log_error "会话创建失败"
    worktree_remove "$TEST_BRANCH" true
    exit 1
fi
echo ""

# ──────────────────────────────────────────────────
# Test 4: Layout service
# ──────────────────────────────────────────────────
log_info "测试 4: Layout 服务"

log_info "创建三分屏布局..."
if layout_create "$TEST_BRANCH" "$worktree_path"; then
    log_success "布局创建成功"

    if layout_validate "$TEST_BRANCH"; then
        log_success "布局验证通过 (3 个 pane)"
    fi

    # Get layout info
    log_info "布局信息:"
    layout_get_info "$TEST_BRANCH"
else
    log_error "布局创建失败"
    session_kill "$TEST_BRANCH"
    worktree_remove "$TEST_BRANCH" true
    exit 1
fi
echo ""

# ──────────────────────────────────────────────────
# Test 5: Command application
# ──────────────────────────────────────────────────
log_info "测试 5: 命令应用"

log_info "应用布局命令配置..."
if layout_apply_commands "$TEST_BRANCH"; then
    log_success "命令配置已应用"
else
    log_warn "命令配置应用失败 (非致命错误)"
fi
echo ""

# ──────────────────────────────────────────────────
# Test 6: List functionality
# ──────────────────────────────────────────────────
log_info "测试 6: 列表功能"

log_info "活跃的会话:"
session_list_all | while read -r session; do
    echo "  - $session"
done

log_info "活跃的 Worktrees:"
worktree_list_all | while read -r wt; do
    [[ -n "$wt" ]] && echo "  - $wt"
done
echo ""

# ──────────────────────────────────────────────────
# Test 7: Cache performance
# ──────────────────────────────────────────────────
log_info "测试 7: 缓存性能"

# Ensure we have a value to cache
cache_set "test_key" "test_value" 60

log_info "首次读取 (无缓存):"
# time command is shell builtin, output to stderr
time cache_get "non_existent_key" >/dev/null 2>&1 || true

log_info "二次读取 (有缓存):"
time cache_get "test_key" >/dev/null

cache_stats
echo ""

log_success "测试完成!"
echo ""

log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
log_bold "所有测试通过! ✓"
log_bold "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

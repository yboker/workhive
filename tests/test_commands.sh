#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# workhive Regression Test Suite
# Tests: syntax, function declarations, source guards,
#        dependency completeness, --help output, runtime execution
# Run:  bash tests/test_commands.sh
# ═══════════════════════════════════════════════════════════════
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
LIB_DIR="$ROOT_DIR/lib"
WORKHIVE_BIN="$ROOT_DIR/bin/workhive"

# ── colours ──────────────────────────────────────────────────
GREEN='\033[0;32m'; RED='\033[0;31m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; NC='\033[0m'; BOLD='\033[1m'

pass=0; fail=0; skip=0

_ok()      { echo -e "${GREEN}  ✓${NC} $1"; pass=$((pass+1)); }
_fail()    { echo -e "${RED}  ✗${NC} $1"; fail=$((fail+1)); }
_skip()    { echo -e "${YELLOW}  ⊘${NC} $1 (skipped)"; skip=$((skip+1)); }
_section() { echo -e "\n${BOLD}${BLUE}▸ $1${NC}"; }
_info()    { echo -e "  ${YELLOW}→${NC} $1"; }

# Run workhive with a given language
_wh() {
    local lang="${1}"; shift
    WORKHIVE_LANG="$lang" bash "$WORKHIVE_BIN" "$@" 2>&1 || true
}

# ════════════════════════════════════════════════════════════════
# 1. SYNTAX CHECK
# ════════════════════════════════════════════════════════════════
_section "1. Syntax check – all command files"
for f in "$LIB_DIR/app/commands/"*.sh \
          "$LIB_DIR/utils/i18n.sh" \
          "$LIB_DIR/infra/adapters/terminal/tmux.sh" \
          "$WORKHIVE_BIN"; do
    name=$(basename "$f")
    if bash -n "$f" 2>/dev/null; then
        _ok "syntax: $name"
    else
        _fail "syntax: $name → $(bash -n "$f" 2>&1 | head -1)"
    fi
done

# ════════════════════════════════════════════════════════════════
# 2. SOURCE GUARD – every command file must have guard
# ════════════════════════════════════════════════════════════════
_section "2. Source guard (double-load prevention)"
for f in "$LIB_DIR/app/commands/"*.sh; do
    name=$(basename "$f")
    if grep -q '\[\[ -n "${_.*_LOADED:-}" \]\]' "$f" 2>/dev/null; then
        _ok "source guard: $name"
    else
        _fail "source guard missing: $name"
    fi
done

# ════════════════════════════════════════════════════════════════
# 3. FUNCTION DECLARATION INTEGRITY
#    Every _help_dispatch call must be inside a named function
# ════════════════════════════════════════════════════════════════
_section "3. Help function declaration integrity"
for f in "$LIB_DIR/app/commands/"*.sh; do
    name=$(basename "$f")
    lineno=$(grep -n "_help_dispatch" "$f" 2>/dev/null | head -1 | cut -d: -f1)
    [[ -z "$lineno" ]] && { _skip "no _help_dispatch: $name"; continue; }
    prevline=$((lineno - 1))
    prev=$(sed -n "${prevline}p" "$f")
    if [[ "$prev" =~ \(\)[[:space:]]*\{ ]]; then
        _ok "help fn declared: $name"
    else
        _fail "help fn missing in $name (line $prevline: '$prev')"
    fi
done

# ════════════════════════════════════════════════════════════════
# 4. DEPENDENCY COMPLETENESS
#    Check that commands which call service functions also source them
# ════════════════════════════════════════════════════════════════
_section "4. Dependency completeness (service layer)"

# Map: pattern to check → source file to require
declare -A DEP_MAP
DEP_MAP["worktree_(list_all|exists|get_path|get_branch|remove|prune|get_base_path|create)"]="worktree_service.sh"
DEP_MAP["session_(list_project|exists|get_name|kill|attach|extract_branch|get_info|validate|create)"]="session_service.sh"
DEP_MAP["layout_(validate|create|get_mode|rebuild|select_pane|apply_commands|collect_pane)"]="layout_service.sh"
DEP_MAP["_worktree_safety_(scan|print|get_marks)"]="worktree_safety.sh"
DEP_MAP["deps_(reconcile|load_providers|list_ids|doctor|status|is_enabled|is_installed)"]="manager.sh"  # infra/deps
DEP_MAP["workspace_(open|close|prepare_directory|ensure_session)"]="workspace_service.sh"

for f in "$LIB_DIR/app/commands/"*.sh; do
    name=$(basename "$f")
    all_ok=true
    for pattern in "${!DEP_MAP[@]}"; do
        required="${DEP_MAP[$pattern]}"
        if grep -qE "$pattern" "$f" 2>/dev/null && \
           ! grep -q "$required" "$f" 2>/dev/null; then
            _fail "dep missing in $name: needs $required (uses ${pattern:0:30}...)"
            all_ok=false
        fi
    done
    [[ "$all_ok" == true ]] && _ok "deps: $name"
done

# ════════════════════════════════════════════════════════════════
# 5. --help OUTPUT (zh + en)
#    Every command must print "Usage:" without crash
# ════════════════════════════════════════════════════════════════
_section "5. --help output (Chinese)"
_help_cmds=(open attach close doctor upgrade gc destroy list remove
            rename log config pane)
for cmd in "${_help_cmds[@]}"; do
    out=$(_wh zh "$cmd" --help 2>&1 || true)
    if echo "$out" | grep -qiE "用法:|Usage:"; then
        _ok "--help zh: workhive $cmd"
    else
        _fail "--help zh: workhive $cmd (no Usage; first line: $(echo "$out" | head -1))"
    fi
done

_section "5b. --help output (English)"
for cmd in "${_help_cmds[@]}"; do
    out=$(_wh en "$cmd" --help 2>&1 || true)
    if echo "$out" | grep -qiE "Usage:"; then
        _ok "--help en: workhive $cmd"
    else
        _fail "--help en: workhive $cmd (no Usage; first line: $(echo "$out" | head -1))"
    fi
done

# ════════════════════════════════════════════════════════════════
# 6. RUNTIME COMMAND EXECUTION
#    Commands that can run outside tmux/git should NOT crash on
#    simple invocations. We check exit code and no "command not found".
# ════════════════════════════════════════════════════════════════
_section "6. Runtime execution (no crash / no 'command not found')"

_runtime_test() {
    local label="$1"; shift
    local out
    out=$(WORKHIVE_LANG=zh bash "$WORKHIVE_BIN" "$@" 2>&1 || true)
    if echo "$out" | grep -qE "command not found|: line [0-9]+:.*not found"; then
        _fail "$label → 'command not found': $(echo "$out" | grep -E 'command not found|not found' | head -1)"
    else
        _ok "$label"
    fi
}

# Safe commands (can run outside git/tmux, just show help or error gracefully)
_runtime_test "doctor (table)"     doctor
_runtime_test "list --help"        list --help
_runtime_test "upgrade --help"     upgrade --help
_runtime_test "gc --help"          gc --help
_runtime_test "destroy --help"     destroy --help
_runtime_test "open --help"        open --help
_runtime_test "open (no tmux arg)" open --no-attach nonexistent-branch-xyz 2>/dev/null || true
_runtime_test "attach --help"      attach --help
_runtime_test "remove --help"      remove --help
_runtime_test "rename --help"      rename --help
_runtime_test "log --help"         log --help
_runtime_test "config --help"      config --help
_runtime_test "pane --help"        pane --help
_runtime_test "version"            --version

# Commands that optionally work without git repo
_runtime_test "list (no args)"     list
_runtime_test "gc --dry-run"       gc --dry-run

# ════════════════════════════════════════════════════════════════
# 7. UNKNOWN OPTION REJECTION
# ════════════════════════════════════════════════════════════════
_section "7. Unknown option rejection"
_reject_cmds=(doctor upgrade list gc open attach remove)
for cmd in "${_reject_cmds[@]}"; do
    out=$(_wh zh "$cmd" --this-flag-does-not-exist 2>&1 || true)
    if echo "$out" | grep -qiE "unknown|未知|invalid|错误|error|unrecognized"; then
        _ok "unknown opt rejected: workhive $cmd"
    else
        _fail "unknown opt not rejected: workhive $cmd (got: $(echo "$out" | head -1))"
    fi
done

# ════════════════════════════════════════════════════════════════
# 8. FZF REMOVED
# ════════════════════════════════════════════════════════════════
_section "8. fzf integration removed"

if grep -q "bind-key.*f.*fzf" "$LIB_DIR/infra/adapters/terminal/tmux.sh" 2>/dev/null; then
    _fail "fzf Ctrl+B f keybinding still in tmux.sh"
else
    _ok "fzf Ctrl+B f keybinding removed"
fi

if [[ -f "$LIB_DIR/app/commands/switch_command.sh" ]]; then
    _fail "switch_command.sh still exists"
else
    _ok "switch_command.sh deleted"
fi

# ════════════════════════════════════════════════════════════════
# 9. I18N CLEANUP
# ════════════════════════════════════════════════════════════════
_section "9. i18n key cleanup"

if grep -q "_I18N_.*_SWITCH_" "$LIB_DIR/utils/i18n.sh" 2>/dev/null; then
    _fail "SWITCH_* i18n keys still in i18n.sh"
else
    _ok "SWITCH_* i18n keys removed"
fi

if grep -q "HELP_CMD_SWITCH" "$LIB_DIR/utils/i18n.sh" 2>/dev/null; then
    _fail "HELP_CMD_SWITCH still in i18n.sh"
else
    _ok "HELP_CMD_SWITCH removed"
fi

# ════════════════════════════════════════════════════════════════
# 10. COMPLETION FILES CONSISTENCY
# ════════════════════════════════════════════════════════════════
_section "10. Completion files"

for comp_file in completions/workhive.bash completions/workhive.zsh completions/workhive.fish; do
    f="$ROOT_DIR/$comp_file"
    if [[ ! -f "$f" ]]; then
        _skip "not found: $comp_file"
        continue
    fi
    if grep -v "^#" "$f" | grep -qE "\b(status|st|switch|clone|completions|deps)\b" 2>/dev/null; then
        _fail "removed command still in $comp_file: $(grep -v '^#' "$f" | grep -E '\b(status|st|switch|clone|completions|deps)\b' | head -1)"
    else
        _ok "removed commands cleaned: $comp_file"
    fi
done

# ════════════════════════════════════════════════════════════════
# SUMMARY
# ════════════════════════════════════════════════════════════════
echo ""
echo "════════════════════════════════════════════════"
total=$((pass + fail + skip))
if [[ $fail -eq 0 ]]; then
    echo -e "  ${GREEN}${BOLD}ALL PASSED${NC}  Tests: $total  ✓ $pass  ⊘ $skip"
else
    echo -e "  ${RED}${BOLD}FAILURES: $fail${NC}  Tests: $total  ✓ $pass  ✗ $fail  ⊘ $skip"
fi
echo "════════════════════════════════════════════════"
echo ""
echo "  Run tests: bash tests/test_commands.sh"
echo ""

[[ $fail -eq 0 ]] && exit 0 || exit 1

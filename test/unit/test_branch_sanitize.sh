#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Branch name sanitization helpers
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"

# ──────────────────────────────────────────────────
# _sanitize_branch_name
# ──────────────────────────────────────────────────
test_suite "Branch Sanitize — _sanitize_branch_name"

test_case "simple branch name unchanged"
result=$(_sanitize_branch_name "demo01")
assert_eq "demo01" "$result"

test_case "slash converted to dash"
result=$(_sanitize_branch_name "feature/login")
assert_eq "feature-login" "$result"

test_case "multiple slashes converted"
result=$(_sanitize_branch_name "org/team/feature")
assert_eq "org-team-feature" "$result"

test_case "dot converted to dash"
result=$(_sanitize_branch_name "v1.0.0")
assert_eq "v1-0-0" "$result"

test_case "colon converted to dash"
result=$(_sanitize_branch_name "fix:bug")
assert_eq "fix-bug" "$result"

test_case "mixed special chars converted"
result=$(_sanitize_branch_name "feature/v1.0:fix")
assert_eq "feature-v1-0-fix" "$result"

test_case "already clean name unchanged"
result=$(_sanitize_branch_name "my-feature-branch")
assert_eq "my-feature-branch" "$result"

# ──────────────────────────────────────────────────
# _branch_needs_sanitize
# ──────────────────────────────────────────────────
test_suite "Branch Sanitize — _branch_needs_sanitize"

test_case "simple name does not need sanitize"
rc=0; _branch_needs_sanitize "demo01" || rc=$?
assert_failure "$rc"

test_case "slash name needs sanitize"
rc=0; _branch_needs_sanitize "feature/login" || rc=$?
assert_success "$rc"

test_case "dot name needs sanitize"
rc=0; _branch_needs_sanitize "v1.0.0" || rc=$?
assert_success "$rc"

test_case "dash name does not need sanitize"
rc=0; _branch_needs_sanitize "my-feature" || rc=$?
assert_failure "$rc"

test_case "underscore name does not need sanitize"
rc=0; _branch_needs_sanitize "my_feature" || rc=$?
assert_failure "$rc"

test_case "colon name needs sanitize"
rc=0; _branch_needs_sanitize "fix:bug" || rc=$?
assert_success "$rc"

test_case "tilde name needs sanitize"
rc=0; _branch_needs_sanitize "branch~1" || rc=$?
assert_success "$rc"

test_case "caret name needs sanitize"
rc=0; _branch_needs_sanitize "branch^2" || rc=$?
assert_success "$rc"

# ──────────────────────────────────────────────────
# _branch_confirm_sanitize (non-interactive, default behavior)
# ──────────────────────────────────────────────────
test_suite "Branch Sanitize — _branch_confirm_sanitize"

test_case "_branch_confirm_sanitize exists"
assert_true "declare -f _branch_confirm_sanitize >/dev/null"

test_case "sets BRANCH_SANITIZED_NAME for slash branch"
# /dev/null as stdin makes ask_yes_no use default ("Y"), so function returns 0 and sets the variable
_tmp_file="${TMPDIR:-/tmp}/wh-sanitize-test-$$"
bash -c "
    export LIB_DIR='$LIB_DIR'
    source \"\$LIB_DIR/utils/common.sh\"
    source \"\$LIB_DIR/infra/cache/manager.sh\"
    source \"\$LIB_DIR/infra/platform/detector.sh\"
    source \"\$LIB_DIR/infra/config/loader.sh\"
    source \"\$LIB_DIR/utils/i18n.sh\"
    _branch_confirm_sanitize 'feature/login' </dev/null >/dev/null 2>&1 || true
    echo \"\$BRANCH_SANITIZED_NAME\"
" > "$_tmp_file" 2>/dev/null
result=$(tail -1 "$_tmp_file")
rm -f "$_tmp_file"
assert_eq "feature-login" "$result"

test_case "sets BRANCH_SANITIZED_NAME for clean branch"
BRANCH_SANITIZED_NAME=""
_branch_confirm_sanitize "demo01" 2>/dev/null
assert_eq "demo01" "$BRANCH_SANITIZED_NAME"

test_case "returns 0 for clean branch (no prompt)"
rc=0; _branch_confirm_sanitize "demo01" 2>/dev/null || rc=$?
assert_success "$rc"

test_report

#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Source Guard Mechanism
# Verifies all lib modules have source guards to prevent double-loading
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

source "$SCRIPT_DIR/test_framework.sh"

test_suite "Source Guard Mechanism"

# ──────────────────────────────────────────────────
# Verify all lib modules have _LOADED guard
# ──────────────────────────────────────────────────

_check_guard() {
    local file="$1"
    local relative="${file#$ROOT_DIR/}"
    test_case "source guard in $relative"
    if grep -q '_LOADED:-' "$file"; then
        assert_eq "0" "0" "has guard"
    else
        assert_eq "has guard" "missing guard" "$relative missing source guard"
    fi
}

while IFS= read -r -d '' file; do
    _check_guard "$file"
done < <(find "$ROOT_DIR/lib" -name '*.sh' -print0)

# ──────────────────────────────────────────────────
# Verify source guard prevents re-execution
# ──────────────────────────────────────────────────

test_case "common.sh guard prevents duplicate readonly error"
# Source it twice; should not produce "readonly variable" error
export LIB_DIR="$ROOT_DIR/lib"
output=$(
    source "$LIB_DIR/utils/common.sh" 2>&1
    source "$LIB_DIR/utils/common.sh" 2>&1
)
rc=$?
assert_eq "0" "$rc" "double source should not error"
assert_not_contains "${output:-}" "readonly" "no readonly error on double source"

test_case "cache manager guard prevents duplicate declare"
output=$(
    source "$LIB_DIR/utils/common.sh" 2>&1
    source "$LIB_DIR/infra/cache/manager.sh" 2>&1
    source "$LIB_DIR/infra/cache/manager.sh" 2>&1
)
rc=$?
assert_eq "0" "$rc" "double source should not error"

test_case "i18n.sh guard prevents duplicate load"
output=$(
    source "$LIB_DIR/utils/i18n.sh" 2>&1
    source "$LIB_DIR/utils/i18n.sh" 2>&1
)
rc=$?
assert_eq "0" "$rc" "double source should not error"

test_report

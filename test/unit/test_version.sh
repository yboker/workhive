#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Version Single Source
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"

test_suite "Version Single Source"

test_case "WORKHIVE_VERSION is defined"
if [[ -n "${WORKHIVE_VERSION:-}" ]]; then rc=0; else rc=1; fi
assert_eq "0" "$rc" "WORKHIVE_VERSION should be set"

test_case "WORKHIVE_VERSION matches semver pattern"
if [[ "$WORKHIVE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then rc=0; else rc=1; fi
assert_eq "0" "$rc" "version should be semver: $WORKHIVE_VERSION"

test_case "bin/workhive uses WORKHIVE_VERSION (not hardcoded)"
# Check that bin/workhive does not have a hardcoded version string
# It should reference WORKHIVE_VERSION variable
bin_content=$(cat "$ROOT_DIR/bin/workhive")
# The version display should use $WORKHIVE_VERSION
if echo "$bin_content" | grep -q 'WORKHIVE_VERSION'; then
    assert_eq "0" "0" "bin/workhive references WORKHIVE_VERSION"
else
    assert_eq "0" "1" "bin/workhive should reference WORKHIVE_VERSION"
fi

test_case "workhive --version output contains version number"
result=$("$ROOT_DIR/bin/workhive" --version 2>&1 || true)
assert_contains "$result" "$WORKHIVE_VERSION" "version output should contain WORKHIVE_VERSION"

test_report

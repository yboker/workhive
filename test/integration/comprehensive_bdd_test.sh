#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# BDD Integration Tests: Comprehensive Coverage
# Tests for Steps 1-8 changes
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

# Test framework
source "$ROOT_DIR/test/unit/test_framework.sh"

# Core dependencies
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"

_BDD_TEST_TMPDIR="${TMPDIR:-/tmp}/workhive-bdd-comprehensive-$$"
mkdir -p "$_BDD_TEST_TMPDIR"
cleanup() {
    rm -rf "$_BDD_TEST_TMPDIR"
    cache_clear 2>/dev/null || true
}
trap cleanup EXIT

test_suite "Comprehensive BDD Tests (Steps 1-8)"

# ──────────────────────────────────────────────────
# Scenario: Null/empty value handling (Step 5)
# ──────────────────────────────────────────────────

test_case "GIVEN yq returns 'null' WHEN checking value THEN _is_null_or_empty returns true"
yq_output="null"
_is_null_or_empty "$yq_output" && result="true" || result="false"
assert_eq "true" "$result"

test_case "GIVEN empty config value WHEN checking THEN _is_null_or_empty returns true"
empty_val=""
_is_null_or_empty "$empty_val" && result="true" || result="false"
assert_eq "true" "$result"

test_case "GIVEN valid config value WHEN checking THEN _is_null_or_empty returns false"
valid_val="left-split"
_is_null_or_empty "$valid_val" && result="true" || result="false"
assert_eq "false" "$result"

# ──────────────────────────────────────────────────
# Scenario: JSON escape safety (Step 7)
# ──────────────────────────────────────────────────

test_case "GIVEN string with injection attempt WHEN json_escape THEN special chars escaped"
malicious='"; rm -rf / #'
result=$(json_escape "$malicious")
assert_contains "$result" '\"' "double quotes should be escaped"
# Escaped string is safe for JSON embedding (quote is \", injection neutralized in JSON context)

test_case "GIVEN string with unicode WHEN json_escape THEN preserved"
unicode_str="你好世界"
result=$(json_escape "$unicode_str")
assert_eq "你好世界" "$result" "unicode should pass through"

test_case "GIVEN string with backslash and quote WHEN json_escape THEN escaped"
special='backslash\ quote"'
result=$(json_escape "$special")
assert_eq 'backslash\\ quote\"' "$result" "backslash and quote escaped per RFC 8259"

test_case "GIVEN string with newline tab cr WHEN json_escape THEN escaped"
special=$'a\nb\tc\rd'
result=$(json_escape "$special")
assert_contains "$result" '\n' "newline escaped"
assert_contains "$result" '\t' "tab escaped"
assert_contains "$result" '\r' "cr escaped"

# ──────────────────────────────────────────────────
# Scenario: Git root caching (Step 6)
# ──────────────────────────────────────────────────

test_case "GIVEN fresh environment WHEN calling _git_root THEN returns valid path"
unset _WORKHIVE_GIT_ROOT
result=$(_git_root)
[[ -d "$result" ]]
# shellcheck disable=SC2319
rc=$?
assert_eq "0" "$rc" "should return existing directory"

test_case "GIVEN cached git root WHEN calling _git_root again THEN returns same value"
unset _WORKHIVE_GIT_ROOT
first=$(_git_root)
second=$(_git_root)
assert_eq "$first" "$second"

# ──────────────────────────────────────────────────
# Scenario: Cache performance with printf (Step 6)
# ──────────────────────────────────────────────────

test_case "GIVEN cache with TTL WHEN setting and getting THEN works correctly"
cache_set "perf_test" "value123" 60
result=$(cache_get "perf_test")
assert_eq "value123" "$result"

test_case "GIVEN cache WHEN setting empty value THEN retrieves empty"
cache_set "empty_val" "" 60
result=$(cache_get "empty_val")
assert_eq "" "$result"

# ──────────────────────────────────────────────────
# Scenario: Config negative caching (Step 5)
# ──────────────────────────────────────────────────

test_case "GIVEN nonexistent config key WHEN config_get with default THEN returns default"
cache_invalidate "config:*"
result=$(config_get "totally.nonexistent.key.abc" "fallback_value")
assert_eq "fallback_value" "$result" "should return provided default for missing key"

test_case "GIVEN nonexistent config key WHEN config_get without default THEN returns empty"
cache_invalidate "config:*"
result=$(config_get "totally.nonexistent.key.def")
assert_eq "" "$result" "missing key without default should return empty"

# ──────────────────────────────────────────────────
# Scenario: Version consistency (Step 8)
# ──────────────────────────────────────────────────

test_case "GIVEN WORKHIVE_VERSION WHEN checking format THEN matches semver"
[[ "$WORKHIVE_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
# shellcheck disable=SC2319
rc=$?
assert_eq "0" "$rc" "version should be semver: ${WORKHIVE_VERSION:-unset}"

# ──────────────────────────────────────────────────
# Scenario: i18n completeness (Step 8)
# ──────────────────────────────────────────────────

test_case "GIVEN plugin i18n keys WHEN querying en THEN returns translation"
# shellcheck disable=SC2034
export WORKHIVE_LANG=en
result=$(_i18n PLUGIN_LOADING)
[[ "$result" != "PLUGIN_LOADING" ]]
# shellcheck disable=SC2319
rc=$?
assert_eq "0" "$rc" "PLUGIN_LOADING should have en translation"

test_case "GIVEN new plugin i18n keys WHEN querying zh THEN returns Chinese"
# shellcheck disable=SC2034
export WORKHIVE_LANG=zh
result=$(_i18n PLUGIN_LOADING)
[[ "$result" != "PLUGIN_LOADING" ]]
# shellcheck disable=SC2319
rc=$?
assert_eq "0" "$rc" "PLUGIN_LOADING should have zh translation"

test_case "GIVEN new adapter i18n keys WHEN querying en THEN returns translation"
# shellcheck disable=SC2034
export WORKHIVE_LANG=en
result=$(_i18n ERR_ADAPTER_UNSUPPORTED)
[[ "$result" != "ERR_ADAPTER_UNSUPPORTED" ]]
# shellcheck disable=SC2319
rc=$?
assert_eq "0" "$rc" "ERR_ADAPTER_UNSUPPORTED should have en translation"

# ──────────────────────────────────────────────────
# Scenario: Boundary conditions
# ──────────────────────────────────────────────────

test_case "GIVEN _is_null_or_empty with special chars THEN handles correctly"
_is_null_or_empty '$null' && result="true" || result="false"
# shellcheck disable=SC2319
assert_eq "false" "$result" "dollar-null is not null"

test_case "GIVEN _is_null_or_empty with newline THEN handles correctly"
_is_null_or_empty $'\n' && result="true" || result="false"
assert_eq "false" "$result" "newline is not null/empty"

test_case "GIVEN json_escape with very long string THEN handles correctly"
long_str=$(printf 'a%.0s' {1..1000})
result=$(json_escape "$long_str")
assert_eq "$long_str" "$result" "long plain string should pass through"

test_case "GIVEN cache with special key chars WHEN get/set THEN works"
cache_set "key:with:colons" "value1" 60
result=$(cache_get "key:with:colons")
assert_eq "value1" "$result"

test_case "GIVEN cache with dot key WHEN get/set THEN works"
cache_set "key.with.dots" "value2" 60
result=$(cache_get "key.with.dots")
assert_eq "value2" "$result"

test_report

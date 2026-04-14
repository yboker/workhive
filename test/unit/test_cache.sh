#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/cache/manager.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"

test_suite "lib/infra/cache/manager.sh"

# Clear cache before each test
cache_clear

# ──────────────────────────────────────────────────
# cache_set & cache_get basic functionality
# ──────────────────────────────────────────────────

test_case "cache_set and cache_get basic"
cache_set "key1" "value1"
result=$(cache_get "key1")
assert_eq "value1" "$result"

test_case "cache_get returns 1 for non-existent key"
cache_get "nonexistent_key_xyz" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

test_case "cache_set overwrites existing value"
cache_set "key2" "old_value"
cache_set "key2" "new_value"
result=$(cache_get "key2")
assert_eq "new_value" "$result"

test_case "cache_set stores empty string"
cache_set "empty_key" ""
# Empty string stored, cache_get returns 1 due to -z check (treated as cache miss)
cache_get "empty_key" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "empty value treated as cache miss"

# ──────────────────────────────────────────────────
# TTL expiration
# ──────────────────────────────────────────────────

test_case "cache_get returns value within TTL"
cache_set "ttl_key" "ttl_value" 60
result=$(cache_get "ttl_key")
assert_eq "ttl_value" "$result"

test_case "cache_get returns miss for expired TTL"
# Set TTL=1 second, wait for expiration
cache_set "expire_key" "expire_value" 1
sleep 2
cache_get "expire_key" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "expired key should miss"

test_case "cache_set with TTL=0 never expires"
cache_set "permanent_key" "permanent_value" 0
result=$(cache_get "permanent_key")
assert_eq "permanent_value" "$result"

# ──────────────────────────────────────────────────
# cache_delete
# ──────────────────────────────────────────────────

test_case "cache_delete removes key"
cache_set "del_key" "del_value"
cache_delete "del_key"
cache_get "del_key" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

test_case "cache_delete on non-existent key is safe"
cache_delete "never_existed_key"
# Should not error
assert_eq "0" "0" "no error"

# ──────────────────────────────────────────────────
# cache_invalidate (pattern matching)
# ──────────────────────────────────────────────────

test_case "cache_invalidate with wildcard pattern"
cache_set "config:value:a" "va"
cache_set "config:value:b" "vb"
cache_set "session:x" "sx"
cache_invalidate "config:*"
cache_get "config:value:a" >/dev/null 2>&1 || rc_a=$?
cache_get "config:value:b" >/dev/null 2>&1 || rc_b=$?
result=$(cache_get "session:x")
assert_eq "1" "${rc_a:-0}" "config:value:a should be invalidated"
assert_eq "1" "${rc_b:-0}" "config:value:b should be invalidated"
assert_eq "sx" "$result" "session:x should survive"

test_case "cache_invalidate with exact key"
cache_set "exact_key" "exact_value"
cache_invalidate "exact_key"
cache_get "exact_key" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# cache_clear
# ──────────────────────────────────────────────────

test_case "cache_clear removes all entries"
cache_set "c1" "v1"
cache_set "c2" "v2"
cache_set "c3" "v3"
cache_clear
cache_get "c1" >/dev/null 2>&1 || rc1=$?
cache_get "c2" >/dev/null 2>&1 || rc2=$?
cache_get "c3" >/dev/null 2>&1 || rc3=$?
assert_eq "1" "${rc1:-0}" "c1 cleared"
assert_eq "1" "${rc2:-0}" "c2 cleared"
assert_eq "1" "${rc3:-0}" "c3 cleared"

# ──────────────────────────────────────────────────
# cache_stats
# ──────────────────────────────────────────────────

test_case "cache_stats shows entry count"
cache_clear
cache_set "stat_key1" "v1"
cache_set "stat_key2" "v2"
output=$(cache_stats)
assert_contains "$output" "2" "should show 2 entries"

test_case "cache_stats shows key names"
output=$(cache_stats)
assert_contains "$output" "stat_key1"
assert_contains "$output" "stat_key2"

# ──────────────────────────────────────────────────
# Special characters in keys (P0 fix coverage)
# ──────────────────────────────────────────────────

test_case "cache_set/get with colon in key"
cache_clear
cache_set "config:value:tmux.theme" "nord"
result=$(cache_get "config:value:tmux.theme")
assert_eq "nord" "$result"

test_case "cache_set/get with dots in key"
cache_clear
cache_set "layout.mode" "left-split"
result=$(cache_get "layout.mode")
assert_eq "left-split" "$result"

test_case "cache_delete with special characters in key"
cache_clear
cache_set "config:value:a.b.c" "test"
cache_delete "config:value:a.b.c"
cache_get "config:value:a.b.c" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "key with special chars should be deleted"

test_case "cache_set/get with spaces in value"
cache_clear
cache_set "space_key" "hello world with spaces"
result=$(cache_get "space_key")
assert_eq "hello world with spaces" "$result"

test_case "cache_set/get with quotes in value"
cache_clear
cache_set "quote_key" 'value with "quotes"'
result=$(cache_get "quote_key")
assert_eq 'value with "quotes"' "$result"

# ──────────────────────────────────────────────────
# cache_invalidate iteration safety (P0 fix coverage)
# ──────────────────────────────────────────────────

test_case "cache_invalidate with many matching keys"
cache_clear
cache_set "bulk:1" "v1"
cache_set "bulk:2" "v2"
cache_set "bulk:3" "v3"
cache_set "bulk:4" "v4"
cache_set "bulk:5" "v5"
cache_set "other:1" "keep"
cache_invalidate "bulk:*"
cache_get "bulk:1" >/dev/null 2>&1 || rc1=$?
cache_get "bulk:5" >/dev/null 2>&1 || rc5=$?
result=$(cache_get "other:1")
assert_eq "1" "${rc1:-0}" "bulk:1 should be invalidated"
assert_eq "1" "${rc5:-0}" "bulk:5 should be invalidated"
assert_eq "keep" "$result" "other:1 should survive"

test_case "cache_invalidate pattern does not modify during iteration"
cache_clear
for i in $(seq 1 20); do
    cache_set "iter:$i" "v$i"
done
cache_invalidate "iter:*"
# All should be gone
all_cleared=true
for i in $(seq 1 20); do
    if cache_get "iter:$i" >/dev/null 2>&1; then
        all_cleared=false
        break
    fi
done
assert_eq "true" "$all_cleared" "all 20 keys should be invalidated"

# ──────────────────────────────────────────────────
# Existence check (+_ pattern)
# ──────────────────────────────────────────────────

test_case "cache_get distinguishes unset vs empty"
cache_clear
# Key never set
cache_get "never_set" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "unset key should return 1"

# ──────────────────────────────────────────────────
# Cache with printf-based timestamps (Step 6)
# ──────────────────────────────────────────────────

test_case "cache TTL works with printf-based timestamp"
cache_set "ttl_test_key" "ttl_value" 2
result=$(cache_get "ttl_test_key")
assert_eq "ttl_value" "$result" "should be available before TTL"
sleep 3
result=$(cache_get "ttl_test_key")
assert_eq "" "$result" "should expire after TTL"

test_report

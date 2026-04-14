#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: json_escape function (common.sh)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"

# json_escape is in common.sh (already sourced above)

test_suite "json_escape (RFC 8259 compliance)"

# ──────────────────────────────────────────────────
# Basic escaping
# ──────────────────────────────────────────────────

test_case "escapes backslash"
result=$(json_escape 'hello\world')
assert_eq 'hello\\world' "$result"

test_case "escapes double quote"
result=$(json_escape 'hello "world"')
assert_eq 'hello \"world\"' "$result"

test_case "escapes newline"
result=$(json_escape $'hello\nworld')
assert_eq 'hello\nworld' "$result"

test_case "escapes tab"
result=$(json_escape $'hello\tworld')
assert_eq 'hello\tworld' "$result"

test_case "escapes carriage return"
result=$(json_escape $'hello\rworld')
assert_eq 'hello\rworld' "$result"

test_case "escapes backspace"
result=$(json_escape $'hello\010world')
assert_eq 'hello\bworld' "$result"

test_case "escapes form feed"
result=$(json_escape $'hello\014world')
assert_eq 'hello\fworld' "$result"

# ──────────────────────────────────────────────────
# No false escaping
# ──────────────────────────────────────────────────

test_case "plain string unchanged"
result=$(json_escape 'hello world')
assert_eq 'hello world' "$result"

test_case "empty string unchanged"
result=$(json_escape '')
assert_eq '' "$result"

test_case "path with slashes unchanged"
result=$(json_escape '/home/user/project')
assert_eq '/home/user/project' "$result"

# ──────────────────────────────────────────────────
# Combined escaping
# ──────────────────────────────────────────────────

test_case "multiple special chars in one string"
result=$(json_escape $'path\\to\t"file"\n')
assert_eq 'path\\to\t\"file\"\n' "$result"

test_case "backslash before quote"
result=$(json_escape '\"')
assert_eq '\\\"' "$result"

# ──────────────────────────────────────────────────
# printf %s output (no trailing newline)
# ──────────────────────────────────────────────────

test_case "output uses printf (no trailing newline)"
raw=$(json_escape 'test'; echo "END")
assert_eq "testEND" "$raw" "printf should not add newline"

test_report

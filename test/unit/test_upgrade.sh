#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/app/commands/upgrade_command.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
export BIN_DIR="$ROOT_DIR/bin"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/utils/tui.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/app/commands/upgrade_command.sh"

test_suite "Upgrade Command Tests"

# ──────────────────────────────────────────────────
# Help output
# ──────────────────────────────────────────────────

test_case "upgrade_command_help outputs help text (en)"
export WORKHIVE_LANG=en
output=$(upgrade_command_help 2>&1)
assert_contains "$output" "upgrade" "should mention upgrade"
assert_contains "$output" "--check" "should mention --check"
assert_contains "$output" "--version" "should mention --version"
assert_contains "$output" "--force" "should mention --force"

test_case "upgrade_command_help outputs help text (zh)"
export WORKHIVE_LANG=zh
output=$(upgrade_command_help 2>&1)
assert_contains "$output" "upgrade" "should mention upgrade"
assert_contains "$output" "--check" "should mention --check"

# ──────────────────────────────────────────────────
# _upgrade_version_gt
# ──────────────────────────────────────────────────

test_case "_upgrade_version_gt: 1.0.1 > 1.0.0 returns 0"
_upgrade_version_gt "1.0.1" "1.0.0"
assert_eq 0 $? "1.0.1 should be greater than 1.0.0"

test_case "_upgrade_version_gt: 1.0.0 == 1.0.0 returns 1"
_upgrade_version_gt "1.0.0" "1.0.0"
assert_eq 1 $? "equal versions should return 1"

test_case "_upgrade_version_gt: 2.0.0 > 1.9.9 returns 0"
_upgrade_version_gt "2.0.0" "1.9.9"
assert_eq 0 $? "2.0.0 should be greater than 1.9.9"

test_case "_upgrade_version_gt: 0.10.0 > 0.9.0 returns 0"
_upgrade_version_gt "0.10.0" "0.9.0"
assert_eq 0 $? "0.10.0 should be greater than 0.9.0"

test_case "_upgrade_version_gt: 0.1.0 < 0.2.0 returns 1"
_upgrade_version_gt "0.1.0" "0.2.0"
assert_eq 1 $? "0.1.0 should not be greater than 0.2.0"

test_case "_upgrade_version_gt: 0.8.0 > 0.7.2 returns 0"
_upgrade_version_gt "0.8.0" "0.7.2"
assert_eq 0 $? "0.8.0 should be greater than 0.7.2"

# ──────────────────────────────────────────────────
# _upgrade_detect_install_type
# ──────────────────────────────────────────────────

test_case "_upgrade_detect_install_type returns git or development"
install_type=$(_upgrade_detect_install_type)
assert_not_empty "$install_type" "should return a type"
# In test env, it should be "git" (we're in a git repo) or "development"
assert_true '[[ "$install_type" == "git" || "$install_type" == "development" ]]' \
    "type should be git or development"

# ──────────────────────────────────────────────────
# upgrade_command_execute argument parsing
# ──────────────────────────────────────────────────

test_case "upgrade_command_execute --help returns 0"
output=$(upgrade_command_execute --help 2>&1)
rc=$?
assert_eq 0 "$rc" "help should return 0"
assert_contains "$output" "upgrade" "help output should mention upgrade"

test_case "upgrade_command_execute rejects unknown option"
output=$(upgrade_command_execute --nonexistent 2>&1)
rc=$?
assert_eq 1 "$rc" "unknown option should fail"

test_case "upgrade_command_execute --version without value fails"
output=$(upgrade_command_execute --version 2>&1)
rc=$?
assert_eq 1 "$rc" "missing version value should fail"

test_case "upgrade_command_execute --version with invalid format fails"
output=$(upgrade_command_execute --version "abc" 2>&1)
rc=$?
assert_eq 1 "$rc" "invalid version format should fail"

test_report

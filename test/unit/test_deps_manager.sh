#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/deps/manager.sh
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

TEST_DIR="${TMPDIR:-/tmp}/workhive-test-deps-$$"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"

cleanup() { cd / || return 1; rm -rf "$TEST_DIR"; }
trap cleanup EXIT

# ──────────────────────────────────────────────────
# Function existence
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Function existence"

unset _DEPS_MANAGER_SH_LOADED 2>/dev/null || true
unset _DEPS_REGISTRY_LOADED 2>/dev/null || true
unset _DEPS_REGISTRY 2>/dev/null || true
source "$LIB_DIR/infra/deps/manager.sh"

test_case "deps_list_ids exists"
assert_true "declare -f deps_list_ids >/dev/null"

test_case "deps_get_name exists"
assert_true "declare -f deps_get_name >/dev/null"

test_case "deps_get_desc exists"
assert_true "declare -f deps_get_desc >/dev/null"

test_case "deps_get_scope exists"
assert_true "declare -f deps_get_scope >/dev/null"

test_case "deps_is_enabled exists"
assert_true "declare -f deps_is_enabled >/dev/null"

test_case "deps_is_installed exists"
assert_true "declare -f deps_is_installed >/dev/null"

test_case "deps_install exists"
assert_true "declare -f deps_install >/dev/null"

test_case "deps_uninstall exists"
assert_true "declare -f deps_uninstall >/dev/null"

test_case "deps_status exists"
assert_true "declare -f deps_status >/dev/null"

test_case "deps_doctor exists"
assert_true "declare -f deps_doctor >/dev/null"

test_case "deps_reconcile exists"
assert_true "declare -f deps_reconcile >/dev/null"

test_case "deps_status_summary exists"
assert_true "declare -f deps_status_summary >/dev/null"

test_case "_deps_load_providers exists"
assert_true "declare -f _deps_load_providers >/dev/null"

# ──────────────────────────────────────────────────
# Provider loading
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Provider loading"

test_case "providers directory exists"
assert_dir_exists "$LIB_DIR/infra/deps/providers"

test_case "tmux_persistence provider is registered after load"
_DEPS_REGISTRY_LOADED=false
_deps_load_providers
result="${_DEPS_REGISTRY[tmux_persistence]:-}"
assert_not_empty "$result" "should be registered"

test_case "deps_list_ids excludes placeholder providers"
ids=$(deps_list_ids)
assert_not_contains "$ids" "tmux_persistence"

# ──────────────────────────────────────────────────
# Provider metadata (placeholder)
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Provider metadata (placeholder)"

test_case "tmux_persistence provider file is loaded (placeholder)"
result="${_DEPS_REGISTRY[tmux_persistence]:-}"
assert_not_empty "$result" "placeholder provider should be registered"

test_case "tmux_persistence has no dep functions (placeholder)"
rc=0; declare -f dep_tmux_persistence_name >/dev/null 2>&1 || rc=$?
assert_failure "$rc"

test_case "deps_get_scope returns project for unknown provider"
scope=$(deps_get_scope "nonexistent_dep")
assert_eq "project" "$scope"

test_case "deps_get_name returns id for unknown provider"
name=$(deps_get_name "nonexistent_dep")
assert_eq "nonexistent_dep" "$name"

test_case "deps_get_desc returns empty for unknown provider"
desc=$(deps_get_desc "nonexistent_dep")
assert_empty "$desc"

# ──────────────────────────────────────────────────
# Enable/disable logic
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Enable/disable logic"

test_case "deps_is_enabled returns false for unknown dep"
rc=0; deps_is_enabled "nonexistent_dep" || rc=$?
assert_failure "$rc"

# ──────────────────────────────────────────────────
# Install/uninstall with unknown dep
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Install/uninstall error handling"

test_case "deps_install returns error for unknown dep"
rc=0; deps_install "nonexistent_dep" 2>/dev/null || rc=$?
assert_failure "$rc"

test_case "deps_uninstall returns error for unknown dep"
rc=0; deps_uninstall "nonexistent_dep" 2>/dev/null || rc=$?
assert_failure "$rc"

# ──────────────────────────────────────────────────
# Reconcile logic (mock test)
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Reconcile"

test_case "deps_reconcile runs without error when disabled"
rc=0; deps_reconcile >/dev/null 2>&1 || rc=$?
assert_success "$rc"

# ──────────────────────────────────────────────────
# Upgrade functions
# ──────────────────────────────────────────────────
test_suite "Deps Manager — Upgrade functions"

test_case "deps_upgrade exists"
assert_true "declare -f deps_upgrade >/dev/null"

test_case "deps_upgrade_all exists"
assert_true "declare -f deps_upgrade_all >/dev/null"

test_case "deps_upgrade returns error for unknown dep"
rc=0; deps_upgrade "nonexistent_dep" 2>/dev/null || rc=$?
assert_failure "$rc"

test_case "deps_upgrade returns error for dep without upgrade function"
# Create a minimal mock provider without upgrade
eval 'dep_mock_no_upgrade_name() { echo "mock"; }'
eval 'dep_mock_no_upgrade_install() { return 0; }'
eval 'dep_mock_no_upgrade_is_installed() { return 0; }'
_DEPS_REGISTRY["mock_no_upgrade"]="/dev/null"
rc=0; deps_upgrade "mock_no_upgrade" 2>/dev/null || rc=$?
assert_failure "$rc"
unset '_DEPS_REGISTRY[mock_no_upgrade]'

test_case "deps_upgrade_all sets result variables"
# shellcheck disable=SC2034
DEPS_UPGRADE_RESULT_UPGRADED=0
# shellcheck disable=SC2034
DEPS_UPGRADE_RESULT_FAILED=0
# shellcheck disable=SC2034
DEPS_UPGRADE_RESULT_SKIPPED=0
deps_upgrade_all >/dev/null 2>&1
assert_true "[[ -n \"\$DEPS_UPGRADE_RESULT_UPGRADED\" ]]"
assert_true "[[ -n \"\$DEPS_UPGRADE_RESULT_FAILED\" ]]"
assert_true "[[ -n \"\$DEPS_UPGRADE_RESULT_SKIPPED\" ]]"

test_report

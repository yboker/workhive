#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/tools/registry.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"

# Source the module under test (reset guard so we can reload)
unset _CONFIG_LOADER_SH_LOADED _TOOLS_REGISTRY_SH_LOADED _CACHE_MANAGER_SH_LOADED

# Set up a temp global config dir so tests don't depend on real config
_TEST_TMP_DIR=$(mktemp -d /tmp/workhive-registry-test-XXXXXX)
export XDG_CONFIG_HOME="$_TEST_TMP_DIR"
mkdir -p "$XDG_CONFIG_HOME/workhive"

# Create a test global config with tools
cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'YAML'
tools:
  default: test-claude
  list:
    - tool_name: test-claude
      label: "Test Claude"
      command: "echo mock-claude"
      is_ai: true
      enabled: true
    - tool_name: test-codex
      label: "Test Codex"
      command: "echo mock-codex"
      is_ai: true
      enabled: true
    - tool_name: test-lazygit
      label: "Test Lazygit"
      command: "echo mock-lazygit"
      is_ai: false
      enabled: true
    - tool_name: test-disabled
      label: "Disabled Tool"
      command: "echo disabled"
      is_ai: false
      enabled: false
YAML

source "$LIB_DIR/infra/tools/registry.sh"

test_suite "Tools Registry Tests"

# ──────────────────────────────────────────────────
# Function existence
# ──────────────────────────────────────────────────

test_case "tools_registry_list exists"
declare -f tools_registry_list &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_load exists"
declare -f tools_registry_load &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_list_ai exists"
declare -f tools_registry_list_ai &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_get_default_idx exists"
declare -f tools_registry_get_default_idx &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_get_ai_default_idx exists"
declare -f tools_registry_get_ai_default_idx &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_get_command exists"
declare -f tools_registry_get_command &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_find_by_name exists"
declare -f tools_registry_find_by_name &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_index_by_name exists"
declare -f tools_registry_index_by_name &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_get_default exists"
declare -f tools_registry_get_default &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_is_installed exists"
declare -f tools_registry_is_installed &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_detect_ai exists"
declare -f tools_registry_detect_ai &>/dev/null
assert_eq 0 $? "function should exist"

test_case "tools_registry_normalize_ai_command exists"
declare -f tools_registry_normalize_ai_command &>/dev/null
assert_eq 0 $? "function should exist"

test_case "_tools_registry_command_bin trims leading spaces"
result=$(_tools_registry_command_bin "   echo hello")
assert_eq "echo" "$result" "command bin parser should ignore leading spaces"

test_case "_tools_registry_resolve_command_bin extracts bin without install check"
_tools_registry_resolve_command_bin "echo hello" "false"
assert_success $? "resolve helper should parse command binary"
assert_eq "echo" "${REPLY:-}" "resolve helper should return parsed binary in REPLY"

test_case "_tools_registry_resolve_command_bin fails on empty command"
_tools_registry_resolve_command_bin "" "false" >/dev/null 2>&1
assert_failure $? "empty command should fail bin resolution"

test_case "tools_registry_validate accepts built-in semantic config"
tools_registry_validate >/dev/null 2>&1
assert_success $? "registry validation should accept the configured tool set"

test_case "tools_registry_get_command returns command for configured tool"
cmd=$(tools_registry_get_command "test-codex" 2>/dev/null)
assert_eq "echo mock-codex" "$cmd" "registry lookup should expose configured command"

test_case "tools_registry_find_by_name returns canonical row for configured tool"
row=$(tools_registry_find_by_name "test-codex" 2>/dev/null || true)
assert_contains "$row" $'test-codex\ttest-codex\tTest Codex\techo mock-codex\ttrue\ttrue\tfalse' "find_by_name should expose canonical normalized row"

test_case "tools_registry_index_by_name returns list index for configured tool"
idx=$(tools_registry_index_by_name "test-codex" 2>/dev/null || true)
assert_eq "1" "$idx" "index query should match tools.list ordering"

test_case "tools_registry_list exposes enabled tools without disabled entries"
result=$(tools_registry_list 2>/dev/null)
assert_contains "$result" "test-claude" "enabled tool should be listed"
assert_contains "$result" "test-codex" "enabled tool should be listed"
assert_contains "$result" "test-lazygit" "enabled non-AI tool should be listed"
assert_not_contains "$result" "test-disabled" "disabled tool should stay hidden from list output"

test_case "tools_registry_list_ai exposes only enabled AI tools"
result=$(tools_registry_list_ai 2>/dev/null)
assert_contains "$result" "test-claude" "AI list should include enabled AI tool"
assert_contains "$result" "test-codex" "AI list should include enabled AI tool"
assert_not_contains "$result" "test-lazygit" "AI list should exclude non-AI tools"
assert_not_contains "$result" "test-disabled" "AI list should exclude disabled tools"

test_case "_tools_registry_find_config_cached does not cache failed lookups"
(
    tools_registry_reset_cache
    _TOOLS_REGISTRY_CFG_CACHE="/tmp/stale-tools-cache"
    _tools_registry_find_config_into() { return 1; }
    _tools_registry_find_config_cached >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    [[ "$_TOOLS_REGISTRY_CFG_CACHE_SET" == "false" ]] || exit 1
    [[ -z "$_TOOLS_REGISTRY_CFG_CACHE" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "failed config lookup should not poison cache state"

test_case "_tools_registry_yq returns non-zero when config lookup fails"
(
    _tools_registry_find_config_cached_into() { return 1; }
    _tools_registry_yq '.tools.default // ""' >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "_tools_registry_yq should fail fast on config lookup error"

test_case "_tools_registry_yq returns non-zero on invalid yq expression and cleans tmp cache"
(
    global_cfg_path=$(config_get_global_file)
    global_cfg_backup="${global_cfg_path}.bak.$$"
    mv "$global_cfg_path" "$global_cfg_backup"
    tools_registry_reset_cache

    _tools_registry_yq '.tools.list[???' >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    [[ "$_TOOLS_REGISTRY_CFG_CACHE_SET" == "false" ]] || exit 1
    [[ -z "$_TOOLS_REGISTRY_CFG_CACHE" ]] || exit 1

    mv "$global_cfg_backup" "$global_cfg_path"
    tools_registry_reset_cache
    exit 0
)
assert_eq "0" "$?" "invalid yq expression should fail and clear fallback temp cache"

test_case "_tools_registry_find_config_cached_into honors explicit tmp marker instead of filename pattern"
(
    tools_registry_reset_cache
    _tools_registry_find_config_into() {
        local out_var="$1"
        _TOOLS_REGISTRY_LAST_CFG_IS_TMP=false
        printf -v "$out_var" '%s' "/tmp/workhive-tools.looks-like-temp-but-is-not.yaml"
        return 0
    }
    cfg_path=""
    _tools_registry_find_config_cached_into cfg_path || exit 1
    [[ "$cfg_path" == "/tmp/workhive-tools.looks-like-temp-but-is-not.yaml" ]] || exit 1
    [[ "$_TOOLS_REGISTRY_CFG_IS_TMP" == "false" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "tmp cleanup marker must come from source metadata, not path pattern"

test_case "_tools_registry_get_existing_cfg_into returns readable cfg path when available"
(
    cfg_path=""
    _tools_registry_get_existing_cfg_into cfg_path || exit 1
    [[ -n "$cfg_path" ]] || exit 1
    [[ -f "$cfg_path" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "existing cfg helper should return a readable file path"

test_case "_tools_registry_cleanup_cached_tmp only deletes cached tmp file"
(
    tools_registry_reset_cache
    cached_tmp_file=$(mktemp /tmp/workhive-tools-cache-target.XXXXXX)
    other_file=$(mktemp /tmp/workhive-tools-other.XXXXXX)
    _TOOLS_REGISTRY_CFG_CACHE="$cached_tmp_file"
    _TOOLS_REGISTRY_CFG_CACHE_SET=true
    _TOOLS_REGISTRY_CFG_IS_TMP=true
    _tools_registry_cleanup_cached_tmp "$other_file"
    [[ ! -e "$cached_tmp_file" ]] || exit 1
    [[ -e "$other_file" ]] || exit 1
    rm -f "$other_file" 2>/dev/null || true
    exit 0
)
assert_eq "0" "$?" "cleanup should only remove cached tmp file"

# ──────────────────────────────────────────────────
# tools_registry_list (requires yq)
# ──────────────────────────────────────────────────

if ! command -v yq &>/dev/null; then
    log_warn "yq not available, skipping registry content tests"
else


test_case "_tools_registry_load_rows returns normalized TSV records for all configured rows"
rows=$(_tools_registry_load_rows 2>/dev/null)
assert_not_empty "$rows" "normalized loader should emit registry rows"
row_count=$(printf '%s\n' "$rows" | wc -l | tr -d ' ')
assert_eq "4" "$row_count" "normalized loader should retain all configured rows including disabled ones"
assert_contains "$rows" $'test-claude\ttest-claude\tTest Claude\techo mock-claude\ttrue\ttrue\ttrue' "first row should include normalized columns in order"
assert_contains "$rows" $'test-codex\ttest-codex\tTest Codex\techo mock-codex\ttrue\ttrue\tfalse' "second enabled row should keep non-default flag"
assert_contains "$rows" $'test-lazygit\ttest-lazygit\tTest Lazygit\techo mock-lazygit\ttrue\tfalse\tfalse' "non-AI row should preserve row shape and false flags"
assert_contains "$rows" $'test-disabled\ttest-disabled\tDisabled Tool\techo disabled\tfalse\tfalse\tfalse' "disabled row should remain in normalized output"
first_row=$(printf '%s\n' "$rows" | sed -n '1p')
second_row=$(printf '%s\n' "$rows" | sed -n '2p')
third_row=$(printf '%s\n' "$rows" | sed -n '3p')
fourth_row=$(printf '%s\n' "$rows" | sed -n '4p')
assert_eq $'test-claude\ttest-claude\tTest Claude\techo mock-claude\ttrue\ttrue\ttrue' "$first_row" "normalized output should preserve config row order for first row"
assert_eq $'test-codex\ttest-codex\tTest Codex\techo mock-codex\ttrue\ttrue\tfalse' "$second_row" "normalized output should preserve config row order for second row"
assert_eq $'test-lazygit\ttest-lazygit\tTest Lazygit\techo mock-lazygit\ttrue\tfalse\tfalse' "$third_row" "normalized output should preserve config row order for third row"
assert_eq $'test-disabled\ttest-disabled\tDisabled Tool\techo disabled\tfalse\tfalse\tfalse' "$fourth_row" "normalized output should preserve config row order for disabled row"

test_case "tools_registry_list_ai propagates tools_registry_list failure"
(
    tools_registry_list() { return 1; }
    tools_registry_list_ai >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "AI list should fail when base registry list fails"

test_case "tools_registry_get_default_idx returns 0 for first-default tool"
idx=$(tools_registry_get_default_idx 2>/dev/null)
assert_eq "0" "$idx" "default idx should be 0 (test-claude is configured as tools.default)"

test_case "tools_registry_get_ai_default_idx returns 0 for first-default AI tool"
idx=$(tools_registry_get_ai_default_idx 2>/dev/null)
assert_eq "0" "$idx" "AI default idx should be 0 (test-claude is tools.default)"

test_case "tools_registry_get_default returns default installed tool id"
default_tool=$(tools_registry_get_default 2>/dev/null || true)
assert_eq "test-claude" "$default_tool" "default query should return enabled default tool id"

test_case "tools_registry_get_command returns command for known id"
cmd=$(tools_registry_get_command "test-claude" 2>/dev/null)
assert_eq "echo mock-claude" "$cmd" "should return the command string"

test_case "tools_registry_get_command returns non-zero for unknown id"
tools_registry_get_command "non-existent-id" 2>/dev/null
rc=$?
assert_failure $rc "should return non-zero for unknown id"

test_case "tools_registry_get_command returns non-zero for empty id"
tools_registry_get_command "" 2>/dev/null
rc=$?
assert_failure $rc "empty id should be rejected"

test_case "tools_registry_detect_ai returns first AI tool id"
first_ai=$(tools_registry_detect_ai 2>/dev/null)
assert_eq "test-claude" "$first_ai" "should return test-claude (first enabled AI tool)"

test_case "tools_registry_detect_ai propagates tools_registry_list failure"
(
    tools_registry_list() { return 1; }
    tools_registry_detect_ai >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "detect_ai should fail when base registry list fails"

test_case "tools_registry_is_ai_command accepts codex variant binary names"
tools_registry_is_ai_command "codex-x86_64-apple-darwin" >/dev/null 2>&1
rc=$?
assert_eq 0 "$rc" "codex platform-variant process name should be recognized as AI command"

test_case "tools_registry_normalize_ai_command maps codex variant to canonical name"
normalized=$(tools_registry_normalize_ai_command "codex-x86_64-apple-darwin" 2>/dev/null || true)
assert_eq "codex" "$normalized" "codex variant should normalize to canonical codex"

test_case "tools_registry_normalize_ai_command maps claude variant to canonical name"
normalized=$(tools_registry_normalize_ai_command "claude-x86_64-apple-darwin" 2>/dev/null || true)
assert_eq "claude" "$normalized" "claude variant should normalize to canonical claude"

test_case "tools registry defaults temp file uses randomized mktemp path"
global_cfg_path=$(config_get_global_file)
global_cfg_backup="${global_cfg_path}.bak.$$"
mv "$global_cfg_path" "$global_cfg_backup"
tools_registry_reset_cache

tmp_cfg_1=$(_tools_registry_find_config_cached)
tools_registry_reset_cache
tmp_cfg_2=$(_tools_registry_find_config_cached)

assert_true "[[ -f \"$tmp_cfg_1\" ]]" "first fallback temp file should exist"
assert_true "[[ -f \"$tmp_cfg_2\" ]]" "second fallback temp file should exist"
assert_true "[[ \"$tmp_cfg_1\" != \"$tmp_cfg_2\" ]]" "fallback temp files should be randomized"

test_case "tools_registry_reset_cache removes cached temp config file"
tools_registry_reset_cache
_tools_registry_find_config_cached_into tmp_cfg_reset
assert_true "[[ -f \"$tmp_cfg_reset\" ]]" "cached temp config should exist before reset"
tools_registry_reset_cache
assert_true "[[ ! -e \"$tmp_cfg_reset\" ]]" "cached temp config should be deleted on reset"

rm -f "$tmp_cfg_1" "$tmp_cfg_2"
mv "$global_cfg_backup" "$global_cfg_path"
tools_registry_reset_cache

test_case "tools_registry_get_default_ref cleans fallback temp cache"
global_cfg_path=$(config_get_global_file)
global_cfg_backup="${global_cfg_path}.bak.$$"
mv "$global_cfg_path" "$global_cfg_backup"
tools_registry_reset_cache

default_ref=$(tools_registry_get_default_ref 2>/dev/null || true)
assert_eq "__empty__" "$default_ref" "fallback defaults should expose __empty__ default ref"
assert_eq "false" "$_TOOLS_REGISTRY_CFG_CACHE_SET" "temp cfg cache should be released after default-ref lookup"
assert_empty "$_TOOLS_REGISTRY_CFG_CACHE" "temp cfg path should be cleared after cleanup"

mv "$global_cfg_backup" "$global_cfg_path"
tools_registry_reset_cache

test_case "tools_registry_get_default_ref cleans stale tmp cache when cfg file is missing"
_TOOLS_REGISTRY_CFG_CACHE="/tmp/workhive-tools-stale-missing-$$"
_TOOLS_REGISTRY_CFG_CACHE_SET=true
_TOOLS_REGISTRY_CFG_IS_TMP=true
_default_ref_out=$(mktemp /tmp/workhive-tools-default-ref.XXXXXX)
tools_registry_get_default_ref > "$_default_ref_out" 2>/dev/null || true
default_ref=$(cat "$_default_ref_out")
rm -f "$_default_ref_out"
assert_empty "$default_ref" "missing cfg should return empty default ref"
assert_eq "false" "$_TOOLS_REGISTRY_CFG_CACHE_SET" "missing cfg path should release temp cache flag"
assert_empty "$_TOOLS_REGISTRY_CFG_CACHE" "missing cfg path should clear cached temp path"

test_case "_tools_registry_yq cleans fallback temp cache"
global_cfg_path=$(config_get_global_file)
global_cfg_backup="${global_cfg_path}.bak.$$"
mv "$global_cfg_path" "$global_cfg_backup"
tools_registry_reset_cache

query_result=$(_tools_registry_yq '.tools.default // ""' 2>/dev/null || true)
assert_eq "__empty__" "$query_result" "internal yq helper should read fallback defaults"
assert_eq "false" "$_TOOLS_REGISTRY_CFG_CACHE_SET" "internal yq helper should release temp cfg cache"
assert_empty "$_TOOLS_REGISTRY_CFG_CACHE" "internal yq helper should clear temp cfg path"

mv "$global_cfg_backup" "$global_cfg_path"
tools_registry_reset_cache

test_case "tools_registry_validate cleans fallback temp cache"
global_cfg_path=$(config_get_global_file)
global_cfg_backup="${global_cfg_path}.bak.$$"
mv "$global_cfg_path" "$global_cfg_backup"
tools_registry_reset_cache

tools_registry_validate >/dev/null 2>&1
validate_rc=$?
assert_eq "0" "$validate_rc" "validate should succeed on fallback defaults"
assert_eq "false" "$_TOOLS_REGISTRY_CFG_CACHE_SET" "validate should release temp cfg cache"
assert_empty "$_TOOLS_REGISTRY_CFG_CACHE" "validate should clear temp cfg path"

mv "$global_cfg_backup" "$global_cfg_path"
tools_registry_reset_cache

fi # end yq check

# ──────────────────────────────────────────────────
# command_builder integration
# ──────────────────────────────────────────────────

unset _COMMAND_BUILDER_SH_LOADED
source "$LIB_DIR/app/shared/command_builder.sh"

test_case "_build_pane_command_list populates arrays"
_build_pane_command_list
assert_not_empty "${_AVAILABLE_CMD_LABELS[*]:-}" "_AVAILABLE_CMD_LABELS should be populated"
assert_not_empty "${_AVAILABLE_CMD_VALUES[*]:-}" "_AVAILABLE_CMD_VALUES should be populated"

test_case "_build_pane_command_list includes empty shell option"
_build_pane_command_list
assert_not_empty "${_AVAILABLE_CMD_EMPTY_IDX:-}" "_AVAILABLE_CMD_EMPTY_IDX should be set"


# ──────────────────────────────────────────────────
# Cleanup
# ──────────────────────────────────────────────────

rm -rf "$_TEST_TMP_DIR"

test_report

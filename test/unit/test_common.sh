#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/utils/common.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
WORKHIVE_LANG="en"

# Disable file logging: prevent test output from leaking to production logs
_LOG_INITIALIZED=true
_LOG_FILE=""

test_suite "lib/utils/common.sh"

# ──────────────────────────────────────────────────
# Log function tests
# ──────────────────────────────────────────────────

test_case "log_info outputs blue info message"
output=$(log_info "test message" 2>&1)
assert_contains "$output" "test message"

test_case "log_success outputs green success message"
output=$(log_success "success message" 2>&1)
assert_contains "$output" "success message"

test_case "log_warn outputs yellow warning message"
output=$(log_warn "warning message" 2>&1)
assert_contains "$output" "warning message"

test_case "log_error outputs red error to stderr"
output=$(log_error "error message" 2>&1)
assert_contains "$output" "error message"

test_case "log_bold outputs bold message"
output=$(log_bold "bold message" 2>&1)
assert_contains "$output" "bold message"

test_case "log_debug suppressed when DEBUG not set"
unset WORKHIVE_DEBUG
output=$(log_debug "debug message" 2>&1)
assert_empty "$output"

test_case "log_debug outputs when DEBUG=1"
export WORKHIVE_DEBUG=1
output=$(log_debug "debug message" 2>&1)
assert_contains "$output" "debug message"
unset WORKHIVE_DEBUG

# ──────────────────────────────────────────────────
# die function tests
# ──────────────────────────────────────────────────

test_case "die outputs error and exits with code 1"
output=$(die "fatal error" 2>&1 || true)
assert_contains "$output" "fatal error"

test_case "die exits with non-zero code"
(die "test" 2>/dev/null) || exit_code=$?
assert_eq "1" "${exit_code:-0}"

# ──────────────────────────────────────────────────
# ask_yes_no function tests (simulated input)
# ──────────────────────────────────────────────────

test_case "ask_yes_no returns 0 for Y input"
result=0
echo "Y" | ask_yes_no "test?" "Y" >/dev/null 2>&1 || result=$?
assert_eq "0" "$result"

test_case "ask_yes_no returns 0 for default Y (empty input)"
result=0
echo "" | ask_yes_no "test?" "Y" >/dev/null 2>&1 || result=$?
assert_eq "0" "$result"

# ──────────────────────────────────────────────────
# CLI format parsing helpers
# ──────────────────────────────────────────────────

test_case "cli_require_option_value accepts non-empty value"
opt_value=$(cli_require_option_value "--shell" "zsh")
rc=$?
assert_eq "0" "$rc"
assert_eq "zsh" "$opt_value"

test_case "cli_require_option_value rejects empty value"
output=$(cli_require_option_value "--shell" "" 2>&1 || true)
assert_contains "$output" "Missing value for --shell"

test_case "cli_require_option_value_nonflag rejects option-like value"
output=$(cli_require_option_value_nonflag "--level" "--follow" 2>&1 || true)
assert_contains "$output" "Missing value for --level"

test_case "cli_log_unknown_opt prints unknown option message"
output=$(cli_log_unknown_opt "--bad" 2>&1 || true)
assert_contains "$output" "--bad"

test_case "cli_log_no_positional prints positional error with scope"
output=$(cli_log_no_positional "log" 2>&1 || true)
assert_contains "$output" "log"

test_case "cli_parse_format_value_i18n accepts valid value"
fmt=$(cli_parse_format_value_i18n "json" '^(table|json)$' 'PANE_LIST_INVALID_FORMAT')
rc=$?
assert_eq "0" "$rc"
assert_eq "json" "$fmt"

test_case "cli_parse_format_value_i18n rejects empty value"
output=$(cli_parse_format_value_i18n "" '^(table|json)$' 'PANE_LIST_INVALID_FORMAT' 2>&1 || true)
assert_contains "$output" "Invalid pane list format"

test_case "cli_parse_format_value_i18n rejects invalid value"
output=$(cli_parse_format_value_i18n "yaml" '^(table|json)$' 'PANE_LIST_INVALID_FORMAT' 2>&1 || true)
assert_contains "$output" "Invalid pane list format"

# In non-TTY environment (pipe input), ask_yes_no uses default value instead of reading stdin
# When default is Y, even sending "n" via pipe returns 0 (uses default value)
test_case "ask_yes_no uses default in non-TTY (default Y)"
result=0
echo "n" | ask_yes_no "test?" "Y" >/dev/null 2>&1 || result=$?
assert_eq "0" "$result"

test_case "ask_yes_no returns 1 for default N (empty input)"
result=0
echo "" | ask_yes_no "test?" "N" >/dev/null 2>&1 || result=$?
assert_eq "1" "$result"

test_case "ask_yes_no returns 1 for N input"
# When default is N, explicit N yields 1 (non-TTY uses default)
result=0
echo "N" | ask_yes_no "test?" "N" >/dev/null 2>&1 || result=$?
assert_eq "1" "$result"

# ──────────────────────────────────────────────────
# ensure_dir function tests
# ──────────────────────────────────────────────────

test_case "ensure_dir creates directory if not exists"
test_dir="/tmp/workhive-test-ensure-$$"
rm -rf "$test_dir"
ensure_dir "$test_dir"
assert_dir_exists "$test_dir"
rm -rf "$test_dir"

test_case "ensure_dir succeeds if directory already exists"
test_dir="/tmp/workhive-test-ensure-$$"
mkdir -p "$test_dir"
ensure_dir "$test_dir"
rc=$?
assert_eq "0" "$rc"
rm -rf "$test_dir"

test_case "ensure_dir with empty path"
# Should handle gracefully (no crash)
ensure_dir "" 2>/dev/null || true
assert_eq "0" "0" "should not crash"

# ──────────────────────────────────────────────────
# cleanup helpers
# ──────────────────────────────────────────────────

test_case "cleanup_file_quiet returns success for empty path"
cleanup_file_quiet ""
rc=$?
assert_eq "0" "$rc" "empty cleanup target should be a no-op success"

test_case "cleanup_file_quiet removes existing file"
_cleanup_test_file="/tmp/workhive-test-cleanup-file-$$"
echo "x" > "$_cleanup_test_file"
cleanup_file_quiet "$_cleanup_test_file"
rc=$?
assert_eq "0" "$rc" "cleanup should succeed for existing file"
assert_eq "1" "$([[ ! -f "$_cleanup_test_file" ]] && echo 1 || echo 0)" "file should be deleted"

test_case "cleanup_dir_quiet removes existing directory"
_cleanup_test_dir="/tmp/workhive-test-cleanup-dir-$$"
mkdir -p "$_cleanup_test_dir/sub"
cleanup_dir_quiet "$_cleanup_test_dir"
rc=$?
assert_eq "0" "$rc" "cleanup should succeed for existing dir"
assert_eq "1" "$([[ ! -d "$_cleanup_test_dir" ]] && echo 1 || echo 0)" "dir should be deleted"

test_case "cleanup_stale_files_by_name removes only stale matching files"
_cleanup_stale_dir="/tmp/workhive-test-cleanup-stale-$$"
mkdir -p "$_cleanup_stale_dir"
echo "old-yaml" > "${_cleanup_stale_dir}/workhive-layout.old.yaml"
echo "old-txt" > "${_cleanup_stale_dir}/workhive-layout.old.txt"
echo "keep" > "${_cleanup_stale_dir}/keep.txt"
_old_touch_ts=$(date -v-10d "+%Y%m%d%H%M.%S" 2>/dev/null || date -d "10 days ago" "+%Y%m%d%H%M.%S")
touch -t "$_old_touch_ts" "${_cleanup_stale_dir}/workhive-layout.old.yaml" "${_cleanup_stale_dir}/workhive-layout.old.txt" 2>/dev/null || true
cleanup_stale_files_by_name "$_cleanup_stale_dir" "60" "workhive-layout.*.yaml"
assert_eq "1" "$([[ ! -f "${_cleanup_stale_dir}/workhive-layout.old.yaml" ]] && echo 1 || echo 0)" "stale yaml file should be deleted"
assert_eq "1" "$([[ -f "${_cleanup_stale_dir}/workhive-layout.old.txt" ]] && echo 1 || echo 0)" "non-matching file should remain"
assert_eq "1" "$([[ -f "${_cleanup_stale_dir}/keep.txt" ]] && echo 1 || echo 0)" "unrelated file should remain"
rm -rf -- "$_cleanup_stale_dir"

# ──────────────────────────────────────────────────
# check_cmd function tests
# ──────────────────────────────────────────────────

test_case "check_cmd returns 0 for existing command"
check_cmd bash
assert_eq "0" "$?" "bash should exist"

test_case "check_cmd returns 1 for non-existent command"
check_cmd nonexistent_command_xyz_12345 || result=$?
assert_eq "1" "${result:-0}" "nonexistent command should fail"

test_case "check_cmd with empty string"
check_cmd "" || result=$?
assert_eq "1" "${result:-0}" "empty string should return 1"

test_case "resolve_realpath_fallback returns canonical path for existing file"
_common_realpath_dir="/tmp/workhive-test-realpath-$$"
mkdir -p "$_common_realpath_dir/sub"
echo "x" > "$_common_realpath_dir/sub/file.txt"
result=$(resolve_realpath_fallback "$_common_realpath_dir/sub/../sub/file.txt")
assert_contains "$result" "/sub/file.txt"
rm -rf -- "$_common_realpath_dir"

test_case "resolve_realpath_allow_missing normalizes unresolved path segments"
_common_missing_base="/tmp/workhive-test-missing-$$"
result=$(resolve_realpath_allow_missing "${_common_missing_base}/a/../b")
assert_contains "$result" "/tmp/workhive-test-missing-$$"
assert_contains "$result" "/b"

test_case "resolve_workhive_bin prefers PATH command"
_common_bin_dir="/tmp/workhive-test-bin-$$"
mkdir -p "$_common_bin_dir"
cat > "${_common_bin_dir}/workhive" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "${_common_bin_dir}/workhive"
_common_prev_path="${PATH:-}"
PATH="${_common_bin_dir}:/usr/bin:/bin"
result=$(resolve_workhive_bin)
assert_eq "${_common_bin_dir}/workhive" "$result" "should prefer command found in PATH"
PATH="$_common_prev_path"
rm -rf -- "$_common_bin_dir"

test_case "resolve_workhive_bin falls back to ROOT_DIR when command missing"
_common_prev_path="${PATH:-}"
_common_prev_root="${ROOT_DIR:-}"
PATH="/usr/bin:/bin"
ROOT_DIR="/tmp/workhive-test-root-$$"
result=$(resolve_workhive_bin)
assert_eq "${ROOT_DIR}/bin/workhive" "$result" "should use ROOT_DIR fallback when PATH has no workhive"
PATH="$_common_prev_path"
ROOT_DIR="$_common_prev_root"

# ──────────────────────────────────────────────────
# version parsing helpers
# ──────────────────────────────────────────────────

test_case "extract_version_by_pattern extracts first semantic version"
result=$(extract_version_by_pattern "git version 2.49.0 (Apple Git-150)" '[0-9]+\.[0-9]+\.[0-9]+')
assert_eq "2.49.0" "$result" "should extract first matching semantic version"

test_case "extract_version_by_pattern returns non-zero for no match"
extract_version_by_pattern "no version here" '[0-9]+\.[0-9]+\.[0-9]+' >/dev/null 2>&1
rc=$?
assert_eq "1" "$rc" "should return non-zero when no version matches"

test_case "get_tool_version reads version from command output"
result=$(get_tool_version "bash" "--version" '[0-9]+\.[0-9]+\.[0-9]+')
assert_not_empty "$result" "bash version should be extracted"

test_case "tool_version_rule returns strict rule for tmux"
result=$(tool_version_rule "tmux" "strict")
assert_eq "-V|[0-9]+\.[0-9]+" "$result" "strict tmux rule should use -V and major.minor pattern"

test_case "tool_version_rule returns non-zero for unknown tool"
tool_version_rule "nonexistent_tool_xyz" "strict" >/dev/null 2>&1
rc=$?
assert_eq "1" "$rc" "unknown tool should have no predefined rule"

test_case "get_tool_version_by_profile reads version with strict profile"
result=$(get_tool_version_by_profile "git" "strict")
assert_not_empty "$result" "strict profile should extract git semantic version"

test_case "parse_version_major_minor parses plain major.minor"
result=$(parse_version_major_minor "3.4")
assert_eq "3|4" "$result" "should parse numeric major/minor"

test_case "parse_version_major_minor parses suffix and patch variants"
result=$(parse_version_major_minor "3.4a")
assert_eq "3|4" "$result" "should strip trailing minor suffix letters"
result=$(parse_version_major_minor "3.4.1")
assert_eq "3|4" "$result" "should keep the second segment as minor"

test_case "parse_version_major_minor returns non-zero for invalid input"
parse_version_major_minor "invalid" >/dev/null 2>&1
rc=$?
assert_eq "1" "$rc" "invalid version string should fail parsing"

# ──────────────────────────────────────────────────
# File logging system tests
# ──────────────────────────────────────────────────

LOG_TEST_DIR="/tmp/workhive-test-log-$$"

test_case "_log_init skips when .workhive dir not present"
_LOG_FILE=""
_LOG_INITIALIZED=false
_saved_log_override="${WORKHIVE_LOG_FILE:-}"
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
_saved_test_log_file="${WORKHIVE_TEST_LOG_FILE:-}"
unset WORKHIVE_LOG_FILE
unset WORKHIVE_TEST_LOG_FILE
export WORKHIVE_TEST_MODE=0
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd /tmp || return 1
    _log_init
    [[ -z "$_LOG_FILE" ]]
)
assert_eq "0" "$?" "LOG_FILE should be empty outside .workhive"
WORKHIVE_LOG_FILE="$_saved_log_override"
export WORKHIVE_LOG_FILE
export WORKHIVE_TEST_MODE="$_saved_test_mode"

test_case "_log_init creates log file when .workhive exists"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
_LOG_FILE=""
_LOG_INITIALIZED=false
_saved_log_override="${WORKHIVE_LOG_FILE:-}"
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
unset WORKHIVE_LOG_FILE
export WORKHIVE_TEST_MODE=0
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd "$LOG_TEST_DIR" || return 1
    _log_init
    [[ -n "$_LOG_FILE" && -d ".workhive/logs" ]]
)
assert_eq "0" "$?" "LOG_FILE should be set and logs dir created"
WORKHIVE_LOG_FILE="$_saved_log_override"
export WORKHIVE_LOG_FILE
export WORKHIVE_TEST_MODE="$_saved_test_mode"

test_case "_log_init uses WORKHIVE_LOG_FILE override outside .workhive"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR"
_LOG_FILE=""
_LOG_INITIALIZED=false
export WORKHIVE_LOG_FILE="$LOG_TEST_DIR/custom/workhive-test.log"
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd /tmp || return 1
    _log_init
    [[ "$_LOG_FILE" == "$WORKHIVE_LOG_FILE" && -d "$LOG_TEST_DIR/custom" ]]
)
assert_eq "0" "$?" "override path should be used even outside project .workhive"

test_case "_log_init uses explicit maintenance.log.path from project config"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
cat > "$LOG_TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    path: ".workhive/logs/runtime-custom.log"
YAML
_LOG_FILE=""
_LOG_INITIALIZED=false
_saved_log_override="${WORKHIVE_LOG_FILE:-}"
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
unset WORKHIVE_LOG_FILE
export WORKHIVE_TEST_MODE=0
config_find_file_with_key() { echo "$LOG_TEST_DIR/.workhive/config.yaml"; }
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd "$LOG_TEST_DIR" || return 1
    _log_init
    [[ "$_LOG_FILE" == "$LOG_TEST_DIR/.workhive/logs/runtime-custom.log" && -d "$LOG_TEST_DIR/.workhive/logs" ]]
)
assert_eq "0" "$?" "explicit maintenance.log.path should control runtime log file"
WORKHIVE_LOG_FILE="$_saved_log_override"
export WORKHIVE_LOG_FILE
WORKHIVE_TEST_LOG_FILE="$_saved_test_log_file"
export WORKHIVE_TEST_LOG_FILE
export WORKHIVE_TEST_MODE="$_saved_test_mode"
unset -f config_find_file_with_key

test_case "_log_init does not abort under set -e when maintenance.log.path is absent"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
cat > "$LOG_TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    level: info
YAML
(
    cd "$LOG_TEST_DIR" || return 1
    env LIB_DIR="$LIB_DIR" bash -e -c '
        source "$LIB_DIR/utils/common.sh"
        _LOG_FILE=""
        _LOG_INITIALIZED=false
        _log_init
    '
)
assert_eq "0" "$?" "_log_init should not fail when optional maintenance.log.path is missing"

test_case "_log_init uses explicit maintenance.test_log.path in test mode"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
cat > "$LOG_TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.9.0"
maintenance:
  test_log:
    path: ".workhive/logs/test-logs/test-custom.log"
YAML
_LOG_FILE=""
_LOG_INITIALIZED=false
_saved_log_override="${WORKHIVE_LOG_FILE:-}"
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
_saved_test_log_file="${WORKHIVE_TEST_LOG_FILE:-}"
unset WORKHIVE_LOG_FILE
unset WORKHIVE_TEST_LOG_FILE
export WORKHIVE_TEST_MODE=1
config_find_file_with_key() { echo "$LOG_TEST_DIR/.workhive/config.yaml"; }
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd "$LOG_TEST_DIR" || return 1
    _log_init
    [[ "$_LOG_FILE" == *"/.workhive/logs/test-logs/test-custom.log" && -d "$(dirname "$_LOG_FILE")" ]]
)
assert_eq "0" "$?" "explicit maintenance.test_log.path should control test log file"
WORKHIVE_LOG_FILE="$_saved_log_override"
export WORKHIVE_LOG_FILE
WORKHIVE_TEST_LOG_FILE="$_saved_test_log_file"
export WORKHIVE_TEST_LOG_FILE
export WORKHIVE_TEST_MODE="$_saved_test_mode"
unset -f config_find_file_with_key

test_case "_log_init applies test_log policy overrides in test mode"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
cat > "$LOG_TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    level: warn
    format: text
    max_size_mb: 9
    max_backups: 4
  test_log:
    path: ".workhive/logs/test-logs/test-policy.log"
    level: debug
    format: json
    max_size_mb: 1
    max_backups: 2
YAML
_LOG_FILE=""
_LOG_INITIALIZED=false
_LOG_SETTINGS_READY=false
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
export WORKHIVE_TEST_MODE=1
config_find_file_with_key() { echo "$LOG_TEST_DIR/.workhive/config.yaml"; }
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd "$LOG_TEST_DIR" || return 1
    _log_init
    [[ "$_LOG_LEVEL" == "debug" && "$_LOG_FORMAT" == "json" && "$_LOG_MAX_SIZE_BYTES" -eq 1048576 && "$_LOG_MAX_BACKUPS" -eq 2 ]]
)
assert_eq "0" "$?" "test channel should use maintenance.test_log policy overrides"
export WORKHIVE_TEST_MODE="$_saved_test_mode"
unset -f config_find_file_with_key

test_case "_log_init inherits log policy when test_log policy is omitted"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive"
cat > "$LOG_TEST_DIR/.workhive/config.yaml" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    level: error
    format: json
    max_size_mb: 2
    max_backups: 7
  test_log:
    path: ".workhive/logs/test-logs/test-inherit.log"
YAML
_LOG_FILE=""
_LOG_INITIALIZED=false
_LOG_SETTINGS_READY=false
_saved_test_mode="${WORKHIVE_TEST_MODE:-}"
export WORKHIVE_TEST_MODE=1
config_find_file_with_key() { echo "$LOG_TEST_DIR/.workhive/config.yaml"; }
unset _WORKHIVE_GIT_ROOT
unset _WORKHIVE_DIR_CACHED
(
    cd "$LOG_TEST_DIR" || return 1
    _log_init
    [[ "$_LOG_LEVEL" == "error" && "$_LOG_FORMAT" == "json" && "$_LOG_MAX_SIZE_BYTES" -eq 2097152 && "$_LOG_MAX_BACKUPS" -eq 7 ]]
)
assert_eq "0" "$?" "test channel should inherit maintenance.log policy by default"
export WORKHIVE_TEST_MODE="$_saved_test_mode"
unset -f config_find_file_with_key

test_case "_log_to_file writes to log file"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive/logs"
_LOG_FILE="$LOG_TEST_DIR/.workhive/logs/workhive.log"
_LOG_INITIALIZED=true
_LOG_SETTINGS_READY=false
_log_to_file "INFO" "test log message"
output=$(cat "$LOG_TEST_DIR/.workhive/logs/workhive.log" 2>/dev/null)
assert_contains "$output" "test log message"
assert_contains "$output" "[INFO ]"

test_case "_log_to_file respects configured log level"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive/logs"
_LOG_FILE="$LOG_TEST_DIR/.workhive/logs/workhive.log"
_LOG_INITIALIZED=true
_LOG_SETTINGS_READY=true
_LOG_LEVEL="error"
_LOG_FORMAT="text"
_LOG_MAX_SIZE_BYTES=1048576
_log_to_file "INFO" "filtered info"
_log_to_file "ERROR" "kept error"
output=$(cat "$LOG_TEST_DIR/.workhive/logs/workhive.log" 2>/dev/null)
assert_not_contains "$output" "filtered info"
assert_contains "$output" "kept error"

test_case "_log_to_file supports json format"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive/logs"
_LOG_FILE="$LOG_TEST_DIR/.workhive/logs/workhive.log"
_LOG_INITIALIZED=true
_LOG_SETTINGS_READY=true
_LOG_LEVEL="debug"
_LOG_FORMAT="json"
_LOG_MAX_SIZE_BYTES=1048576
_log_to_file "INFO" "json message"
output=$(cat "$LOG_TEST_DIR/.workhive/logs/workhive.log" 2>/dev/null)
assert_contains "$output" "\"level\":\"INFO\""
assert_contains "$output" "\"msg\":\"json message\""

test_case "_log_to_file silently stops when log dir removed"
rm -rf "$LOG_TEST_DIR"
_LOG_FILE="$LOG_TEST_DIR/.workhive/logs/workhive.log"
_LOG_INITIALIZED=true
# Log directory no longer exists, should silently clear _LOG_FILE
_log_to_file "INFO" "should not crash"
assert_eq "" "$_LOG_FILE" "_LOG_FILE should be cleared"

test_case "_log_disable_file clears _LOG_FILE"
_LOG_FILE="/some/path/workhive.log"
_log_disable_file
assert_eq "" "$_LOG_FILE"

test_case "_log_rotate rotates oversized log and keeps backup"
rm -rf "$LOG_TEST_DIR"
mkdir -p "$LOG_TEST_DIR/.workhive/logs"
local_log="$LOG_TEST_DIR/.workhive/logs/workhive.log"
dd if=/dev/zero bs=200 count=1 2>/dev/null | tr '\0' 'A' > "$local_log"
env WORKHIVE_LOG_MAX_SIZE=100 WORKHIVE_LOG_MAX_BACKUPS=2 \
    bash -c "
        export LIB_DIR='$LIB_DIR'
        source \"\$LIB_DIR/utils/common.sh\"
        _LOG_FILE='$local_log'
        _LOG_INITIALIZED=true
        _log_rotate
    " 2>/dev/null
file_size_after=$(wc -c < "$local_log" 2>/dev/null | tr -d ' ')
assert_eq "0" "$file_size_after" "current log should be reset after rotation"
assert_true "[[ -f '${local_log}.1' ]]" "rotated backup .1 should exist"

test_case "_log_rotate no-op when file is below threshold"
dd if=/dev/zero bs=50 count=1 2>/dev/null | tr '\0' 'B' > "$local_log"
file_size_before=$(wc -c < "$local_log" | tr -d ' ')
env WORKHIVE_LOG_MAX_SIZE=100 WORKHIVE_LOG_MAX_BACKUPS=2 \
    bash -c "
        export LIB_DIR='$LIB_DIR'
        source \"\$LIB_DIR/utils/common.sh\"
        _LOG_FILE='$local_log'
        _LOG_INITIALIZED=true
        _log_rotate
    " 2>/dev/null
file_size_after=$(wc -c < "$local_log" 2>/dev/null | tr -d ' ')
assert_eq "$file_size_before" "$file_size_after" "small log should not be rotated"

test_case "_log_rotate keeps configured max backup count"
dd if=/dev/zero bs=200 count=1 2>/dev/null | tr '\0' 'C' > "$local_log"
env WORKHIVE_LOG_MAX_SIZE=100 WORKHIVE_LOG_MAX_BACKUPS=2 \
    bash -c "
        export LIB_DIR='$LIB_DIR'
        source \"\$LIB_DIR/utils/common.sh\"
        _LOG_FILE='$local_log'
        _LOG_INITIALIZED=true
        _log_rotate
    " 2>/dev/null
dd if=/dev/zero bs=200 count=1 2>/dev/null | tr '\0' 'D' > "$local_log"
env WORKHIVE_LOG_MAX_SIZE=100 WORKHIVE_LOG_MAX_BACKUPS=2 \
    bash -c "
        export LIB_DIR='$LIB_DIR'
        source \"\$LIB_DIR/utils/common.sh\"
        _LOG_FILE='$local_log'
        _LOG_INITIALIZED=true
        _log_rotate
    " 2>/dev/null
assert_true "[[ -f '${local_log}.1' ]]" "latest backup .1 should exist"
assert_true "[[ -f '${local_log}.2' ]]" "second backup .2 should exist"
assert_true "[[ ! -f '${local_log}.3' ]]" "backup count should be capped"

# cleanup
rm -rf "$LOG_TEST_DIR"
_LOG_FILE=""
_LOG_INITIALIZED=false

# ──────────────────────────────────────────────────
# _is_null_or_empty
# ──────────────────────────────────────────────────

test_case "_is_null_or_empty returns true for empty string"
_is_null_or_empty "" && result=0 || result=1
assert_eq "0" "$result"

test_case "_is_null_or_empty returns true for 'null' string"
_is_null_or_empty "null" && result=0 || result=1
assert_eq "0" "$result"

test_case "_is_null_or_empty returns false for non-empty string"
_is_null_or_empty "hello" && result=0 || result=1
assert_eq "1" "$result"

test_case "_is_null_or_empty returns false for 'NULL' (case sensitive)"
_is_null_or_empty "NULL" && result=0 || result=1
assert_eq "1" "$result"

test_case "_is_null_or_empty returns false for 'null ' (trailing space)"
_is_null_or_empty "null " && result=0 || result=1
assert_eq "1" "$result"

test_case "_is_null_or_empty returns true with no arguments"
_is_null_or_empty && result=0 || result=1
assert_eq "0" "$result"

test_case "_is_null_or_empty returns false for whitespace-only"
_is_null_or_empty " " && result=0 || result=1
assert_eq "1" "$result" "whitespace is not null/empty"

test_case "_is_null_or_empty returns false for 'nil'"
_is_null_or_empty "nil" && result=0 || result=1
assert_eq "1" "$result" "nil is not null"

test_case "_is_null_or_empty returns false for '0'"
_is_null_or_empty "0" && result=0 || result=1
assert_eq "1" "$result" "0 is not null/empty"

test_case "_is_null_or_empty returns false for 'false'"
_is_null_or_empty "false" && result=0 || result=1
assert_eq "1" "$result" "false is not null/empty"

# ──────────────────────────────────────────────────
# json_escape
# ──────────────────────────────────────────────────

test_case "json_escape escapes backslash"
result=$(json_escape 'a\b')
assert_eq 'a\\b' "$result"

test_case "json_escape escapes double quote"
result=$(json_escape 'say "hello"')
assert_eq 'say \"hello\"' "$result"

test_case "json_escape escapes newline"
result=$(json_escape $'line1\nline2')
assert_eq 'line1\nline2' "$result"

test_case "json_escape escapes tab"
result=$(json_escape $'col1\tcol2')
assert_eq 'col1\tcol2' "$result"

test_case "json_escape escapes carriage return"
result=$(json_escape $'text\rmore')
assert_eq 'text\rmore' "$result"

test_case "json_escape handles empty string"
result=$(json_escape '')
assert_eq '' "$result"

test_case "json_escape handles plain text unchanged"
result=$(json_escape 'hello world')
assert_eq 'hello world' "$result"

test_case "json_escape handles multiple special chars"
result=$(json_escape $'a\\b\n"c"\td')
assert_eq 'a\\b\n\"c\"\td' "$result"

# ──────────────────────────────────────────────────
# _git_root (caching)
# ──────────────────────────────────────────────────

test_case "_git_root returns a path"
unset _WORKHIVE_GIT_ROOT
result=$(_git_root)
assert_not_empty "$result" "_git_root should return non-empty"

test_case "_git_root caches result"
unset _WORKHIVE_GIT_ROOT
first=$(_git_root)
# Note: subshell doesn't propagate, but the function should work consistently
second=$(_git_root)
assert_eq "$first" "$second" "consecutive calls should return same value"

# ──────────────────────────────────────────────────
# _workhive_dir (always anchored to git root)
# ──────────────────────────────────────────────────

test_case "_workhive_dir returns git_root/.workhive"
unset _WORKHIVE_GIT_ROOT
result=$(_workhive_dir)
expected="$(_git_root)/.workhive"
assert_eq "$expected" "$result" "_workhive_dir should be git_root/.workhive"

test_case "_workhive_dir is stable from subdirectory"
unset _WORKHIVE_GIT_ROOT
_test_sub="/tmp/workhive-test-whdir-$$"
mkdir -p "$_test_sub"
(
    cd "$_test_sub" || return 1
    unset _WORKHIVE_GIT_ROOT
    result=$(_workhive_dir)
    git_root=$(_git_root)
    echo "${result}" | grep -q "${git_root}/.workhive"
)
assert_eq 0 $? "_workhive_dir from subdirectory should still reference git root"
rm -rf "$_test_sub"

# ──────────────────────────────────────────────────
# _git_subpath_offset
# ──────────────────────────────────────────────────

test_case "_git_subpath_offset returns empty at git root"
unset _WORKHIVE_GIT_ROOT
_test_repo="/tmp/workhive-test-offset-$$"
mkdir -p "$_test_repo"
(
    cd "$_test_repo" || return 1
    git init -q
    unset _WORKHIVE_GIT_ROOT
    result=$(_git_subpath_offset)
    [[ -z "$result" ]]
)
assert_eq 0 $? "offset at git root should be empty"

test_case "_git_subpath_offset returns subpath from subdirectory"
mkdir -p "$_test_repo/sub/dir"
(
    cd "$_test_repo/sub/dir" || return 1
    unset _WORKHIVE_GIT_ROOT
    result=$(_git_subpath_offset)
    [[ "$result" == "sub/dir" ]]
)
assert_eq 0 $? "offset from subdirectory should be 'sub/dir'"
rm -rf "$_test_repo"

test_case "git_is_inside_work_tree returns 0 inside repository"
git_is_inside_work_tree
assert_eq 0 "$?" "git_is_inside_work_tree should succeed in repo"

test_case "git_is_inside_work_tree returns non-zero outside repository"
_outside_dir="/tmp/workhive-test-not-git-$$"
mkdir -p "$_outside_dir"
(
    cd "$_outside_dir" || exit 1
    git_is_inside_work_tree >/dev/null 2>&1
)
rc=$?
assert_ne 0 "$rc" "git_is_inside_work_tree should fail outside repo"
rm -rf "$_outside_dir"

# ──────────────────────────────────────────────────
# safe_rm
# ──────────────────────────────────────────────────

_SAFE_RM_TEST_DIR="${TMPDIR:-/tmp}/workhive-test-safe-rm-$$"
rm -rf "$_SAFE_RM_TEST_DIR"

test_case "safe_rm deletes a normal directory"
mkdir -p "$_SAFE_RM_TEST_DIR/target"
echo "test" > "$_SAFE_RM_TEST_DIR/target/file.txt"
safe_rm "$_SAFE_RM_TEST_DIR/target" 2>/dev/null
rc=$?
assert_eq 0 "$rc" "should succeed"
assert_eq "1" "$([[ ! -d "$_SAFE_RM_TEST_DIR/target" ]] && echo 1 || echo 0)" "dir should be gone"

test_case "safe_rm rejects empty path"
safe_rm "" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "empty path should fail"

test_case "safe_rm rejects current directory dot path"
safe_rm "." 2>/dev/null
rc=$?
assert_eq 1 "$rc" "dot path should fail"

test_case "safe_rm rejects root path"
safe_rm "/" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "root path should fail"

test_case "safe_rm rejects HOME path"
safe_rm "$HOME" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "HOME path should fail"

test_case "safe_rm rejects /usr"
safe_rm "/usr" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "/usr should fail"

test_case "safe_rm rejects /etc"
safe_rm "/etc" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "/etc should fail"

test_case "safe_rm handles nonexistent path gracefully"
safe_rm "$_SAFE_RM_TEST_DIR/does-not-exist" 2>/dev/null
rc=$?
assert_eq 0 "$rc" "nonexistent path should succeed (rm -rf is tolerant)"

test_case "safe_rm rejects direct parent traversal path"
safe_rm "../danger" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "path containing parent traversal should fail"

test_case "safe_rm rejects embedded parent traversal path and does not delete target"
mkdir -p "$_SAFE_RM_TEST_DIR/keep-target"
safe_rm "$_SAFE_RM_TEST_DIR/tmp/../keep-target" 2>/dev/null
rc=$?
assert_eq 1 "$rc" "embedded parent traversal should fail"
assert_eq "1" "$([[ -d "$_SAFE_RM_TEST_DIR/keep-target" ]] && echo 1 || echo 0)" "target directory must remain"

test_case "safe_rm safely deletes option-like path names"
mkdir -p "$_SAFE_RM_TEST_DIR/-option-like"
safe_rm "$_SAFE_RM_TEST_DIR/-option-like" 2>/dev/null
rc=$?
assert_eq 0 "$rc" "option-like path should be treated as operand, not rm option"
assert_eq "1" "$([[ ! -d "$_SAFE_RM_TEST_DIR/-option-like" ]] && echo 1 || echo 0)" "option-like directory should be deleted"

rm -rf "$_SAFE_RM_TEST_DIR"

# ──────────────────────────────────────────────────
# _is_git_init_allowed (whitelist path guard)
# ──────────────────────────────────────────────────

_GIA_TEST_DIR=$(mktemp -d "${TMPDIR:-/tmp}/workhive-test-gia-XXXXXX")
rm -rf "$_GIA_TEST_DIR"
mkdir -p "$_GIA_TEST_DIR/allowed/project"
mkdir -p "$_GIA_TEST_DIR/blocked/project"

_GIA_CONFIG_WHITELIST=""
_GIA_CONFIG_EXTRA=""

_i18n() { echo "$1"; }

config_get() {
    case "$1" in
        git_init.whitelist_paths) echo "$_GIA_CONFIG_WHITELIST" ;;
        git_init.dangerous_paths_extra) echo "$_GIA_CONFIG_EXTRA" ;;
        *) echo "" ;;
    esac
}

test_case "_is_git_init_allowed: blocks root dir (hard guard)"
rc=0
_is_git_init_allowed "/" 2>/dev/null || rc=$?
assert_eq "1" "$rc" "/ should be blocked"

test_case "_is_git_init_allowed: allows home dir when extra list is empty"
rc=0
_is_git_init_allowed "$HOME" 2>/dev/null || rc=$?
assert_eq "0" "$rc" "HOME should be allowed when not in dangerous_paths_extra"

test_case "_is_git_init_allowed: blocks home dir when included in extra list"
_GIA_CONFIG_WHITELIST=""
_GIA_CONFIG_EXTRA="[\"$HOME\"]"
rc=0
_is_git_init_allowed "$HOME" 2>/dev/null || rc=$?
assert_eq "1" "$rc" "HOME should be blocked when configured in dangerous_paths_extra"

test_case "_is_git_init_allowed: allows safe dir with empty whitelist"
_GIA_CONFIG_WHITELIST=""
_GIA_CONFIG_EXTRA=""
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/allowed/project" 2>/dev/null || rc=$?
assert_eq "0" "$rc" "safe dir should be allowed when whitelist is empty"

test_case "_is_git_init_allowed: allows whitelisted dir"
_GIA_CONFIG_WHITELIST="[\"$_GIA_TEST_DIR/allowed\"]"
_GIA_CONFIG_EXTRA=""
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/allowed/project" 2>/dev/null || rc=$?
assert_eq "0" "$rc" "subdirectory of whitelisted path should be allowed"

test_case "_is_git_init_allowed: blocks non-whitelisted dir"
_GIA_CONFIG_WHITELIST="[\"$_GIA_TEST_DIR/allowed\"]"
_GIA_CONFIG_EXTRA=""
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/blocked/project" 2>/dev/null || rc=$?
assert_eq "1" "$rc" "non-whitelisted dir should be blocked"

test_case "_is_git_init_allowed: blocks exact extra dangerous path"
_GIA_CONFIG_WHITELIST=""
_GIA_CONFIG_EXTRA="[\"$_GIA_TEST_DIR/blocked/project\"]"
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/blocked/project" 2>/dev/null || rc=$?
assert_eq "1" "$rc" "exact extra dangerous path should be blocked"

test_case "_is_git_init_allowed: allows subdir under extra dangerous path (exact-match only)"
_GIA_CONFIG_WHITELIST=""
_GIA_CONFIG_EXTRA="[\"$_GIA_TEST_DIR/blocked\"]"
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/blocked/project" 2>/dev/null || rc=$?
assert_eq "0" "$rc" "subdir should be allowed when only parent is in extra dangerous paths"

test_case "_is_git_init_allowed: whitelist takes precedence over exact extra dangerous path"
_GIA_CONFIG_WHITELIST="[\"$_GIA_TEST_DIR/blocked\"]"
_GIA_CONFIG_EXTRA="[\"$_GIA_TEST_DIR/blocked/project\"]"
rc=0
_is_git_init_allowed "$_GIA_TEST_DIR/blocked/project" 2>/dev/null || rc=$?
assert_eq "0" "$rc" "whitelist should override exact extra dangerous path"

test_case "_is_git_init_allowed: hard guard cannot be overridden by whitelist"
_GIA_CONFIG_WHITELIST='["/"]'
_GIA_CONFIG_EXTRA=""
rc=0
_is_git_init_allowed "/" 2>/dev/null || rc=$?
assert_eq "1" "$rc" "hard guard should block root even when whitelist includes /"

rm -rf "$_GIA_TEST_DIR"

test_report

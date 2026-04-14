#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Builtin Persistence Module
# Tests snapshot I/O, field escaping, process capture,
# manager config queries, and snapshot lifecycle.
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/utils/i18n.sh"
# shellcheck disable=SC2034
WORKHIVE_LANG="en"

# Create isolated test environment
TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

# Stub functions that persistence modules depend on
_workhive_dir() { echo "$TEST_DIR/.workhive"; }
session_get_instance_id() { echo "abcd"; }
session_get_project_name() { echo "test-project"; }
session_get_prefix() { echo "wh@abcd@test-project"; }
session_get_name() { echo "wh@abcd@test-project@$1"; }
session_apply_config() { true; }
config_get() {
    case "$1" in
        tmux.persistence.enabled) echo "true" ;;
        tmux.persistence.auto_save_interval) echo "1m" ;;
        tmux.persistence.restore_processes) echo "true" ;;
        tmux.persistence.process_list) echo "" ;;
        *) echo "" ;;
    esac
}

mkdir -p "$TEST_DIR/.workhive"

# Isolate from real global config (prevent test pollution)
export XDG_CONFIG_HOME="$TEST_DIR/xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

# Create project config so _persistence_config_get finds values here
cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
tmux:
  persistence:
    enabled: true
    auto_save_interval: 1m
    auto_save_debounce: 3s
    restore_processes: true
YAML

# Load persistence modules
source "$LIB_DIR/infra/persistence/snapshot.sh"
source "$LIB_DIR/infra/persistence/process_capture.sh"
source "$LIB_DIR/infra/persistence/save.sh"
source "$LIB_DIR/infra/persistence/restore.sh"
source "$LIB_DIR/infra/persistence/auto_save.sh"
source "$LIB_DIR/infra/persistence/manager.sh"
source "$LIB_DIR/app/commands/open_command.sh"

# Keep persistence stubs authoritative after command modules are sourced.
session_get_prefix() { echo "wh@abcd@test-project"; }
session_get_name() { echo "wh@abcd@test-project@$1"; }

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot Module Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Snapshot I/O"

test_case "snapshot_get_project_dir returns .workhive/snapshots"
result=$(snapshot_get_project_dir)
assert_contains "$result" ".workhive/snapshots"

test_case "snapshot_get_global_dir returns data dir with instance_id"
result=$(snapshot_get_global_dir "abcd")
assert_contains "$result" "workhive/snapshots/abcd"

test_case "snapshot_new_file_path creates directory and valid path"
result=$(snapshot_new_file_path)
assert_contains "$result" "wh_snapshot_archive_"
assert_contains "$result" ".jsonl"
assert_not_contains "$result" "XXXXXXXXXX" "temp name should not contain literal mktemp template"
assert_true "[[ -d '$(dirname "$result")' ]]" "snapshot dir should exist"

test_case "snapshot_get_latest returns empty when no snapshots"
result=$(snapshot_get_latest 2>/dev/null || echo "")
assert_empty "$result"

test_suite "Persistence — Save Lock"

test_case "_save_lock_is_stale returns true for dead pid lock"
_lock_dir="$(snapshot_get_dir)/.save.lock"
mkdir -p "$_lock_dir"
echo "999999" > "${_lock_dir}/pid"
rc=1
_save_lock_is_stale "$_lock_dir" && rc=0
assert_eq 0 "$rc" "dead pid lock should be considered stale"
rm -rf "$_lock_dir"

test_case "_save_lock_is_stale returns false for live pid lock"
_lock_dir="$(snapshot_get_dir)/.save.lock"
mkdir -p "$_lock_dir"
echo "$$" > "${_lock_dir}/pid"
rc=0
_save_lock_is_stale "$_lock_dir" || rc=$?
assert_eq 1 "$rc" "live pid lock should not be considered stale"
rm -rf "$_lock_dir"

test_case "_save_lock_acquire reclaims stale lock and acquires new lock"
_lock_dir="$(snapshot_get_dir)/.save.lock"
mkdir -p "$_lock_dir"
echo "999999" > "${_lock_dir}/pid"
rc=0
_save_lock_acquire || rc=$?
assert_eq 0 "$rc" "should reclaim stale lock and acquire successfully"
assert_true "[[ -d '$_SAVE_LOCK_DIR' ]]" "lock directory should exist after acquire"
_save_lock_release
assert_true "[[ ! -d '$_lock_dir' ]]" "lock directory should be removed after release"

test_case "_save_lock_log_contention uses warn by default"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
unset WORKHIVE_SAVE_LOCK_QUIET || true
_save_lock_log_contention
assert_eq "1" "$_persist_log_warn_called" "default path should emit warn"
assert_eq "0" "$_persist_log_debug_called" "default path should not emit debug"
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "_save_lock_log_contention uses debug when quiet mode is enabled"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
export WORKHIVE_SAVE_LOCK_QUIET=1
_save_lock_log_contention
assert_eq "0" "$_persist_log_warn_called" "quiet path should suppress warn"
assert_eq "1" "$_persist_log_debug_called" "quiet path should emit debug"
unset WORKHIVE_SAVE_LOCK_QUIET || true
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "_save_lock_log_contention uses warn when quiet env is not configured"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
unset WORKHIVE_SAVE_LOCK_QUIET || true
_save_lock_log_contention
assert_eq "1" "$_persist_log_warn_called" "lock contention should warn when quiet env is absent"
assert_eq "0" "$_persist_log_debug_called" "lock contention should not emit debug without quiet env"
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "_save_quality_log_rejected_final uses warn by default"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
unset WORKHIVE_SAVE_QUALITY_QUIET || true
_save_quality_log_rejected_final "3"
assert_eq "1" "$_persist_log_warn_called" "default quality-gate path should emit warn"
assert_eq "0" "$_persist_log_debug_called" "default quality-gate path should not emit debug"
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "_save_quality_log_rejected_final uses debug when quiet mode is enabled"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
export WORKHIVE_SAVE_QUALITY_QUIET=1
_save_quality_log_rejected_final "3"
assert_eq "0" "$_persist_log_warn_called" "quiet quality-gate path should suppress warn"
assert_eq "1" "$_persist_log_debug_called" "quiet quality-gate path should emit debug"
unset WORKHIVE_SAVE_QUALITY_QUIET || true
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "_save_quality_log_rejected_final uses warn when quiet env is not configured"
_persist_log_warn_called=0
_persist_log_debug_called=0
_persist_orig_log_warn=$(declare -f log_warn || true)
_persist_orig_log_debug=$(declare -f log_debug || true)
log_warn() { _persist_log_warn_called=$((_persist_log_warn_called + 1)); }
log_debug() { _persist_log_debug_called=$((_persist_log_debug_called + 1)); }
unset WORKHIVE_SAVE_QUALITY_QUIET || true
_save_quality_log_rejected_final "3"
assert_eq "1" "$_persist_log_warn_called" "quality gate should warn when quiet env is absent"
assert_eq "0" "$_persist_log_debug_called" "quality gate should not emit debug without quiet env"
unset -f log_warn log_debug
[[ -n "$_persist_orig_log_warn" ]] && eval "$_persist_orig_log_warn"
[[ -n "$_persist_orig_log_debug" ]] && eval "$_persist_orig_log_debug"

test_case "persistence_save quality gate retries and promotes only healthy candidate"
_persist_saved_quality_ok="$(declare -f _save_snapshot_quality_ok || true)"
_persist_saved_preflight="$(declare -f _save_preflight || true)"
_persist_saved_lock_acquire="$(declare -f _save_lock_acquire || true)"
_persist_saved_lock_release="$(declare -f _save_lock_release || true)"
_persist_saved_snapshot_new_file_path="$(declare -f snapshot_new_file_path || true)"
_persist_saved_write_snapshot="$(declare -f _save_write_snapshot || true)"
_persist_saved_snapshot_has_changed="$(declare -f snapshot_has_changed || true)"
_persist_saved_snapshot_update_last="$(declare -f snapshot_update_last || true)"
_persist_saved_snapshot_get_latest="$(declare -f snapshot_get_latest || true)"
_persist_saved_collect_dormant="$(declare -f _save_collect_dormant_records || true)"
_persist_saved_sleep="$(declare -f sleep || true)"
_persist_saved_snapshot_cleanup="$(declare -f snapshot_cleanup || true)"

_save_preflight() { _SAVE_PREFIX="wh@abcd@test-project"; _SAVE_MATCHING="wh@abcd@test-project@main"; return 0; }
_save_lock_acquire() { return 0; }
_save_lock_release() { :; }
snapshot_get_latest() { echo ""; }
_save_collect_dormant_records() { :; }
_persist_quality_counter_file="$TEST_DIR/.workhive/snapshots/.quality-counter"
echo "0" > "$_persist_quality_counter_file"
snapshot_new_file_path() {
    local idx=0
    idx=$(cat "$_persist_quality_counter_file" 2>/dev/null || echo "0")
    [[ "$idx" =~ ^[0-9]+$ ]] || idx=0
    idx=$((idx + 1))
    echo "$idx" > "$_persist_quality_counter_file"
    echo "$TEST_DIR/.workhive/snapshots/candidate-${idx}.jsonl"
}
_save_write_snapshot() { printf '{"type":"meta","version":"3","schema":"workhive.snapshot"}\n' > "$1"; }
snapshot_has_changed() { return 0; }
_persist_quality_checks=0
_save_snapshot_quality_ok() {
    _persist_quality_checks=$((_persist_quality_checks + 1))
    [[ "$_persist_quality_checks" -ge 2 ]]
}
_persist_updated_target=""
snapshot_update_last() { _persist_updated_target="$1"; return 0; }
snapshot_cleanup() { :; }
sleep() { :; }

export WORKHIVE_SAVE_QUALITY_MAX_RETRIES=3
persistence_save "event" "quality-retry-test"
rc=$?
unset WORKHIVE_SAVE_QUALITY_MAX_RETRIES

assert_eq "0" "$rc" "save should succeed when a later retry passes quality gate"
assert_contains "$_persist_updated_target" "candidate-2.jsonl" "second candidate should be promoted"
assert_eq "2" "$_persist_quality_checks" "quality gate should be evaluated for each retry candidate"

unset -f _save_snapshot_quality_ok _save_preflight _save_lock_acquire _save_lock_release snapshot_new_file_path _save_write_snapshot snapshot_has_changed snapshot_update_last snapshot_get_latest _save_collect_dormant_records sleep snapshot_cleanup
[[ -n "$_persist_saved_quality_ok" ]] && eval "$_persist_saved_quality_ok"
[[ -n "$_persist_saved_preflight" ]] && eval "$_persist_saved_preflight"
[[ -n "$_persist_saved_lock_acquire" ]] && eval "$_persist_saved_lock_acquire"
[[ -n "$_persist_saved_lock_release" ]] && eval "$_persist_saved_lock_release"
[[ -n "$_persist_saved_snapshot_new_file_path" ]] && eval "$_persist_saved_snapshot_new_file_path"
[[ -n "$_persist_saved_write_snapshot" ]] && eval "$_persist_saved_write_snapshot"
[[ -n "$_persist_saved_snapshot_has_changed" ]] && eval "$_persist_saved_snapshot_has_changed"
[[ -n "$_persist_saved_snapshot_update_last" ]] && eval "$_persist_saved_snapshot_update_last"
[[ -n "$_persist_saved_snapshot_get_latest" ]] && eval "$_persist_saved_snapshot_get_latest"
[[ -n "$_persist_saved_collect_dormant" ]] && eval "$_persist_saved_collect_dormant"
[[ -n "$_persist_saved_sleep" ]] && eval "$_persist_saved_sleep"
[[ -n "$_persist_saved_snapshot_cleanup" ]] && eval "$_persist_saved_snapshot_cleanup"

test_case "persistence_save quality gate gives up after retry limit and keeps LKG"
_persist_saved_quality_ok="$(declare -f _save_snapshot_quality_ok || true)"
_persist_saved_preflight="$(declare -f _save_preflight || true)"
_persist_saved_lock_acquire="$(declare -f _save_lock_acquire || true)"
_persist_saved_lock_release="$(declare -f _save_lock_release || true)"
_persist_saved_snapshot_new_file_path="$(declare -f snapshot_new_file_path || true)"
_persist_saved_write_snapshot="$(declare -f _save_write_snapshot || true)"
_persist_saved_snapshot_has_changed="$(declare -f snapshot_has_changed || true)"
_persist_saved_snapshot_update_last="$(declare -f snapshot_update_last || true)"
_persist_saved_snapshot_get_latest="$(declare -f snapshot_get_latest || true)"
_persist_saved_collect_dormant="$(declare -f _save_collect_dormant_records || true)"
_persist_saved_sleep="$(declare -f sleep || true)"
_persist_saved_snapshot_cleanup="$(declare -f snapshot_cleanup || true)"

_save_preflight() { _SAVE_PREFIX="wh@abcd@test-project"; _SAVE_MATCHING="wh@abcd@test-project@main"; return 0; }
_save_lock_acquire() { return 0; }
_save_lock_release() { :; }
snapshot_get_latest() { echo ""; }
_save_collect_dormant_records() { :; }
_persist_quality_counter_file="$TEST_DIR/.workhive/snapshots/.quality-counter-fail"
echo "0" > "$_persist_quality_counter_file"
snapshot_new_file_path() {
    local idx=0
    idx=$(cat "$_persist_quality_counter_file" 2>/dev/null || echo "0")
    [[ "$idx" =~ ^[0-9]+$ ]] || idx=0
    idx=$((idx + 1))
    echo "$idx" > "$_persist_quality_counter_file"
    echo "$TEST_DIR/.workhive/snapshots/candidate-fail-${idx}.jsonl"
}
_save_write_snapshot() { printf '{"type":"meta","version":"3","schema":"workhive.snapshot"}\n' > "$1"; }
snapshot_has_changed() { return 0; }
_persist_quality_checks=0
_save_snapshot_quality_ok() {
    _persist_quality_checks=$((_persist_quality_checks + 1))
    return 1
}
_persist_updated_target=""
snapshot_update_last() { _persist_updated_target="$1"; return 0; }
snapshot_cleanup() { :; }
sleep() { :; }

export WORKHIVE_SAVE_QUALITY_MAX_RETRIES=2
persistence_save "event" "quality-fail-test"
rc=$?
unset WORKHIVE_SAVE_QUALITY_MAX_RETRIES

assert_failure "$rc" "save should fail when all retry candidates fail quality gate"
assert_empty "$_persist_updated_target" "failed quality gate should not promote a new latest snapshot"
assert_eq "2" "$_persist_quality_checks" "quality gate should stop after configured retry limit"

unset -f _save_snapshot_quality_ok _save_preflight _save_lock_acquire _save_lock_release snapshot_new_file_path _save_write_snapshot snapshot_has_changed snapshot_update_last snapshot_get_latest _save_collect_dormant_records sleep snapshot_cleanup
[[ -n "$_persist_saved_quality_ok" ]] && eval "$_persist_saved_quality_ok"
[[ -n "$_persist_saved_preflight" ]] && eval "$_persist_saved_preflight"
[[ -n "$_persist_saved_lock_acquire" ]] && eval "$_persist_saved_lock_acquire"
[[ -n "$_persist_saved_lock_release" ]] && eval "$_persist_saved_lock_release"
[[ -n "$_persist_saved_snapshot_new_file_path" ]] && eval "$_persist_saved_snapshot_new_file_path"
[[ -n "$_persist_saved_write_snapshot" ]] && eval "$_persist_saved_write_snapshot"
[[ -n "$_persist_saved_snapshot_has_changed" ]] && eval "$_persist_saved_snapshot_has_changed"
[[ -n "$_persist_saved_snapshot_update_last" ]] && eval "$_persist_saved_snapshot_update_last"
[[ -n "$_persist_saved_snapshot_get_latest" ]] && eval "$_persist_saved_snapshot_get_latest"
[[ -n "$_persist_saved_collect_dormant" ]] && eval "$_persist_saved_collect_dormant"
[[ -n "$_persist_saved_sleep" ]] && eval "$_persist_saved_sleep"
[[ -n "$_persist_saved_snapshot_cleanup" ]] && eval "$_persist_saved_snapshot_cleanup"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# JSON Escaping Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — JSON Escaping"

test_case "snapshot_escape_json_string preserves normal path"
result=$(snapshot_escape_json_string "/Users/test/project")
assert_eq "/Users/test/project" "$result"

test_case "snapshot_parse_json_string_literal restores escaped path"
snapshot_parse_json_string_literal '"path\\\\with\\\\slashes"'
assert_success $?
assert_contains "$REPLY" "path"
assert_contains "$REPLY" "with"
assert_contains "$REPLY" "slashes"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Header Generation & Parsing Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Header"

test_case "snapshot_write_header generates valid header"
header=$(snapshot_write_header "abcd" "test-project" "wh@abcd@test-project" "1.0.0")
assert_contains "$header" "workhive snapshot v3"
assert_contains "$header" "instance_id: abcd"
assert_contains "$header" "project: test-project"

test_case "snapshot_parse_header extracts instance_id"
tmp_file="$TEST_DIR/test_header.txt"
snapshot_write_header "abcd" "myproject" "wh@abcd@myproject" "1.0.0" > "$tmp_file"
result=$(snapshot_parse_header "$tmp_file" "instance_id")
assert_eq "abcd" "$result"

test_case "snapshot_parse_header extracts project"
result=$(snapshot_parse_header "$tmp_file" "project")
assert_eq "myproject" "$result"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot Version & V2 JSONL Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Snapshot Version & V2 JSONL"

test_case "snapshot_detect_version detects v2 from meta jsonl"
v2_file="$TEST_DIR/snapshot_v2_detect.txt"
printf '%s\n' '{"type":"meta","version":"2","instance_id":"abcd","project":"test","session_prefix":"wh@abcd@test"}' > "$v2_file"
result=$(snapshot_detect_version "$v2_file")
assert_eq "2" "$result"

test_case "snapshot_detect_version detects v2 meta regardless of key order and whitespace"
v2_file_mixed="$TEST_DIR/snapshot_v2_detect_mixed.txt"
printf '%s\n' '  {"version":"2","instance_id":"abcd","project":"test","type":"meta","session_prefix":"wh@abcd@test"}' > "$v2_file_mixed"
result=$(snapshot_detect_version "$v2_file_mixed")
assert_eq "2" "$result"

test_case "snapshot_detect_version rejects non-JSONL files"
v1_file="$TEST_DIR/snapshot_v1_detect.txt"
printf '%s\n' '# workhive snapshot v1' > "$v1_file"
result=$(snapshot_detect_version "$v1_file")
assert_empty "$result"

test_case "snapshot_escape_json_string escapes quote backslash tab and newline"
result=$(snapshot_escape_json_string $'a"b\\c\td\ne')
assert_eq 'a\"b\\c\td\ne' "$result"

test_case "snapshot_parse_json_string_literal keeps escaped backslash-n literal"
snapshot_parse_json_string_literal '"\\n"'
assert_success $?
assert_eq '\n' "$REPLY"

test_case "snapshot_parse_json_string_literal decodes newline escape"
snapshot_parse_json_string_literal '"line1\nline2"'
assert_success $?
assert_eq $'line1\nline2' "$REPLY"

test_case "snapshot_escape_json_string preserves dash-prefixed values"
result=$(snapshot_escape_json_string "-n")
assert_eq "-n" "$result"

test_case "snapshot_parse_json_fields supports empty single-field values"
line='{"type":"meta","session_prefix":""}'
snapshot_parse_json_fields "$line" session_prefix
assert_success $?
assert_eq "1" "${_SNAP_JSON_FIELD_COUNT:-0}"

test_case "snapshot_parse_json_fields preserves trailing empty field positions"
line='{"type":"pane","session":"s1","full_command":""}'
snapshot_parse_json_fields "$line" session full_command
assert_success $?
assert_eq "2" "${_SNAP_JSON_FIELD_COUNT:-0}"
assert_eq "s1" "${_SNAP_JSON_FIELD_0:-}"
assert_eq "" "${_SNAP_JSON_FIELD_1:-}"

test_case "snapshot_parse_json_fields fails on malformed json"
line='{"type":"pane","session":"s1"'
snapshot_parse_json_fields "$line" session
assert_failure $? "malformed json should fail parsing"

test_case "snapshot_parse_json_fields fails when yq compatibility check fails"
(
    _SNAPSHOT_YQ_READY=0
    config_check_yq_version() { return 1; }
    line='{"type":"pane","session":"s1"}'
    snapshot_parse_json_fields "$line" session >/dev/null 2>&1
    rc=$?
    unset -f config_check_yq_version
    exit "$rc"
)
assert_failure $? "incompatible yq should fail JSON field parsing"

test_case "snapshot_parse_json_fields works when compatibility checker is unavailable"
(
    _SNAPSHOT_YQ_READY=0
    unset -f config_check_yq_version
    line='{"type":"pane","session":"s1"}'
    snapshot_parse_json_fields "$line" session >/dev/null 2>&1
)
assert_success $? "parser should remain compatible without config compatibility helper"

test_case "snapshot_parse_pane_line does not leak previous trailing values"
line=$(snapshot_format_pane "wh@test@main" "0" "1" "*" "0" "main" "/tmp/one" "1" "bash" "vim a.sh")
snapshot_parse_pane_line "$line"
line=$(snapshot_format_pane "wh@test@main" "0" "1" "*" "1" "two" "/tmp/two" "1" "bash" "")
snapshot_parse_pane_line "$line"
assert_eq "" "${_SP_FULL_CMD:-}"

test_case "snapshot_format_pane produces json line"
line=$(snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp/project" "1" "bash" "vim app.sh")
assert_contains "$line" '"type":"pane"'
assert_contains "$line" '"session":"wh@abcd@test@main"'
assert_contains "$line" '"full_command":"vim app.sh"'

test_case "snapshot_parse_pane_line supports v2 jsonl records"
line=$(snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp/a b" "1" "bash" "vim \"a b\"")
snapshot_parse_pane_line "$line"
assert_eq "wh@abcd@test@main" "$_SP_SESSION"
assert_eq "/tmp/a b" "$_SP_DIR"
assert_eq "vim \"a b\"" "$_SP_FULL_CMD"

test_case "v2 snapshot parsing does not require python3"
python3() { return 127; }
line=$(snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp/no-python" "1" "bash" "vim app.sh")
snapshot_parse_pane_line "$line"
assert_eq "wh@abcd@test@main" "$_SP_SESSION"
assert_eq "/tmp/no-python" "$_SP_DIR"
unset -f python3

test_case "persistence_has_snapshot matches exact session name only"
current_snapshot_file="$TEST_DIR/has_snapshot_exact.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test-project" "1.0.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "/tmp/main" "1" "vim" "vim main.ts"
    snapshot_format_pane "wh@abcd@test-project@main-2" "0" "1" "*" "0" "main-2" "/tmp/main2" "1" "bash" ""
} > "$current_snapshot_file"

_saved_snapshot_list_restore_candidates="$(declare -f snapshot_list_restore_candidates 2>/dev/null || true)"
_saved_session_get_name="$(declare -f session_get_name 2>/dev/null || true)"

snapshot_list_restore_candidates() { echo "$current_snapshot_file"; }
session_get_name() { echo "wh@abcd@test-project@$1"; }

persistence_has_snapshot "main"
assert_success $?

persistence_has_snapshot "mai"
assert_failure $?

unset -f snapshot_list_restore_candidates
unset -f session_get_name
[[ -n "$_saved_snapshot_list_restore_candidates" ]] && eval "$_saved_snapshot_list_restore_candidates"
[[ -n "$_saved_session_get_name" ]] && eval "$_saved_session_get_name"

test_case "snapshot accessors normalize v2 records for legacy callers"
v2_data="$TEST_DIR/snapshot_v2_data.txt"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp/main" "1" "bash" ""
    snapshot_format_pane "wh@abcd@test@feature" "0" "1" "*" "0" "feature" "/tmp/feature" "1" "bash" ""
    snapshot_format_window "wh@abcd@test@main" "0" "main" "1" "*" "layout" "1"
    snapshot_format_state "wh@abcd@test@main" "wh@abcd@test@feature"
} > "$v2_data"
pane_lines=$(snapshot_get_pane_lines "$v2_data")
assert_contains "$pane_lines" '"type":"pane"'
assert_contains "$pane_lines" '"session":"wh@abcd@test@main"'
window_lines=$(snapshot_get_window_lines "$v2_data")
assert_contains "$window_lines" '"type":"window"'
assert_contains "$window_lines" '"session":"wh@abcd@test@main"'
state_line=$(snapshot_get_state_line "$v2_data")
assert_contains "$state_line" '"type":"state"'
assert_contains "$state_line" '"active_session":"wh@abcd@test@main"'
sessions=$(snapshot_get_sessions "$v2_data")
assert_contains "$sessions" "wh@abcd@test@main"
assert_contains "$sessions" "wh@abcd@test@feature"

test_case "_snapshot_get_lines_by_type supports all/first modes"
lines_all=$(_snapshot_get_lines_by_type "$v2_data" "pane" "all")
assert_contains "$lines_all" '"type":"pane"' "all mode should include pane lines"
line_first=$(_snapshot_get_lines_by_type "$v2_data" "state" "first")
assert_contains "$line_first" '"type":"state"' "first mode should return first matching line"

test_case "_snapshot_get_pane_sessions emits pane session names"
pane_sessions=$(_snapshot_get_pane_sessions "$v2_data")
assert_contains "$pane_sessions" "wh@abcd@test@main" "pane sessions helper should include main session"
assert_contains "$pane_sessions" "wh@abcd@test@feature" "pane sessions helper should include feature session"

test_case "snapshot_validate supports v2 instance_id check"
v2_valid="$TEST_DIR/snapshot_v2_validate.txt"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp" "1" "bash" ""
} > "$v2_valid"
snapshot_validate "$v2_valid" "abcd"
assert_success $?

(snapshot_validate "$v2_valid" "wxyz")
assert_failure $?

test_case "snapshot_validate supports v2 meta with key reordering"
v2_valid_mixed="$TEST_DIR/snapshot_v2_validate_mixed.txt"
{
    printf '%s\n' ' {"version":"3","project":"test","type":"meta","instance_id":"abcd","schema":"workhive.snapshot","session_prefix":"wh@abcd@test"}'
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp" "1" "bash" ""
} > "$v2_valid_mixed"
snapshot_validate "$v2_valid_mixed" "abcd"
assert_success $?

test_case "snapshot_validate rejects v1 legacy format"
v1_valid="$TEST_DIR/snapshot_v1_validate.txt"
{
    snapshot_write_header "abcd" "test" "wh@abcd@test" "1.0"
    printf "pane\twh@abcd@test@main\t0\t1\t*\t0\ttitle\t/tmp\t1\tbash\t\n"
} > "$v1_valid"
(snapshot_validate "$v1_valid" "")
assert_failure $?

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Format & Parse Record Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Record Format & Parse"

test_case "snapshot_format_pane produces json line with pane type"
line=$(snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "bash" "/Users/test" "1" "bash" "")
assert_contains "$line" '"type":"pane"'
assert_contains "$line" '"session":"wh@abcd@test@main"'

test_case "snapshot_format_window produces json line with window type"
line=$(snapshot_format_window "wh@abcd@test@main" "0" "bash" "1" "*" "layout-string" "")
assert_contains "$line" '"type":"window"'
assert_contains "$line" '"window_layout":"layout-string"'

test_case "snapshot_format_state produces state line"
line=$(snapshot_format_state "wh@abcd@test@main" "")
assert_contains "$line" '"type":"state"'

test_case "snapshot_parse_pane_line extracts fields correctly"
line=$(snapshot_format_pane "wh@test@main" "0" "1" "*" "0" "title" "/Users/test" "1" "bash" "vim test.txt")
snapshot_parse_pane_line "$line"
assert_eq "wh@test@main" "$_SP_SESSION"
assert_eq "0" "$_SP_WIN_IDX"
assert_eq "0" "$_SP_PANE_IDX"
assert_eq "/Users/test" "$_SP_DIR"
assert_eq "bash" "$_SP_PANE_CMD"

test_case "snapshot_parse_window_line extracts fields correctly"
line=$(snapshot_format_window "wh@test@main" "0" "bash" "1" "*" "some-layout" "1")
snapshot_parse_window_line "$line"
assert_eq "wh@test@main" "$_SW_SESSION"
assert_eq "0" "$_SW_WIN_IDX"
assert_eq "bash" "$_SW_WIN_NAME"
assert_eq "some-layout" "$_SW_LAYOUT"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot Validation Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Snapshot Validation"

test_case "snapshot_validate returns 1 for non-existent file"
(snapshot_validate "/nonexistent/file.txt" "abcd")
assert_failure $?

test_case "snapshot_validate succeeds for valid v2 snapshot"
valid_snap="$TEST_DIR/valid_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "$valid_snap"
snapshot_validate "$valid_snap" "abcd"
assert_success $?

test_case "snapshot_validate fails for mismatched instance_id"
(snapshot_validate "$valid_snap" "wxyz")
assert_failure $?

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot Lifecycle Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Snapshot Lifecycle"

test_case "snapshot_update_last creates latest jsonl file"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
test_snap="$snap_dir/wh_snapshot_tmp.jsonl"
echo "test data" > "$test_snap"
snapshot_update_last "$test_snap"
assert_true "[[ -f '${snap_dir}/wh_snapshot_latest.jsonl' ]]" "latest snapshot should exist"

test_case "snapshot_get_latest returns jsonl latest"
result=$(snapshot_get_latest)
assert_eq "${snap_dir}/wh_snapshot_latest.jsonl" "$result"

test_case "snapshot_get_latest returns empty when only legacy txt exists"
rm -f "${snap_dir}/wh_snapshot_latest.jsonl"
echo "legacy data" > "${snap_dir}/wh_snapshot_latest.txt"
result=$(snapshot_get_latest 2>/dev/null || echo "")
assert_empty "$result"

# Restore jsonl fixture for subsequent tests.
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "${snap_dir}/wh_snapshot_latest.jsonl"

test_case "snapshot_has_changed detects identical files"
dup_snap="$snap_dir/wh_snapshot_tmp.jsonl"
cp "${snap_dir}/wh_snapshot_latest.jsonl" "$dup_snap"
(snapshot_has_changed "$dup_snap" "${snap_dir}/wh_snapshot_latest.jsonl")
rc=$?
assert_eq "1" "$rc" "identical snapshots should return 1"

test_case "snapshot_has_changed detects different files"
diff_snap="$snap_dir/wh_snapshot_tmp2.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@feature" "0" "1" "*" "0" "title" "/tmp/feature" "1" "bash" ""
} > "$diff_snap"
snapshot_has_changed "$diff_snap" "${snap_dir}/wh_snapshot_latest.jsonl"
assert_success $? "different snapshots should return 0"

test_case "snapshot_count returns correct number"
count=$(snapshot_count)
[[ "$count" =~ ^[0-9]+$ ]] && [[ "$count" -ge 1 ]]
assert_success $? "snapshot count should be numeric and >= 1"

test_case "snapshot_list_files returns single latest file"
files=$(snapshot_list_files)
assert_not_empty "$files"

test_case "snapshot_get_sessions extracts session names"
sessions_snap="$snap_dir/wh_snapshot_sessions.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
    snapshot_format_pane "wh@abcd@test@feature" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "$sessions_snap"
result=$(snapshot_get_sessions "$sessions_snap")
assert_contains "$result" "wh@abcd@test@main"
assert_contains "$result" "wh@abcd@test@feature"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Process Capture Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Process Capture"

test_case "process_should_restore returns 1 for bash"
process_should_restore "bash"
assert_failure $?

test_case "process_should_restore returns 1 for zsh"
process_should_restore "zsh"
assert_failure $?

test_case "process_should_restore returns 0 for vim"
process_should_restore "vim"
assert_success $?

test_case "process_should_restore returns 0 for lazygit"
process_should_restore "lazygit"
assert_success $?

test_case "process_should_restore returns 0 for claude"
process_should_restore "claude"
assert_success $?

test_case "process_should_restore returns 1 for empty"
process_should_restore ""
assert_failure $?

test_case "process_should_restore custom list"
process_should_restore "python" "" "python node ruby"
assert_success $?

test_case "process_should_restore custom list excludes unlisted"
process_should_restore "vim" "" "python node ruby"
assert_failure $?

test_case "process_resolve_restore_command returns original for unknown"
result=$(process_resolve_restore_command "htop" "/tmp")
assert_eq "htop" "$result"

test_case "process_resolve_restore_command handles vim with Session.vim"
session_vim_dir="$TEST_DIR/vim_session_test"
mkdir -p "$session_vim_dir"
touch "$session_vim_dir/Session.vim"
result=$(process_resolve_restore_command "vim somefile.py" "$session_vim_dir")
assert_eq "vim -S" "$result"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Manager Config Queries
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Manager"

test_case "persistence_is_enabled returns true when config says enabled"
persistence_is_enabled
assert_success $?

test_case "persistence_is_enabled returns false when disabled"
# Temporarily set enabled=false in project config
yq -i '.tmux.persistence.enabled = false' "$TEST_DIR/.workhive/config.yaml" 2>/dev/null
persistence_is_enabled
rc=$?
# Restore enabled=true
yq -i '.tmux.persistence.enabled = true' "$TEST_DIR/.workhive/config.yaml" 2>/dev/null
assert_failure $rc "should return 1 when disabled"

test_case "persistence_enabled_bool returns true when enabled"
result=$(persistence_enabled_bool)
assert_eq "true" "$result"

test_case "persistence_enabled_bool returns false when disabled"
yq -i '.tmux.persistence.enabled = false' "$TEST_DIR/.workhive/config.yaml" 2>/dev/null
result=$(persistence_enabled_bool)
yq -i '.tmux.persistence.enabled = true' "$TEST_DIR/.workhive/config.yaml" 2>/dev/null
assert_eq "false" "$result"

test_case "persistence_has_snapshot returns 1 when no snapshot exists"
# Clean up snapshot dir
rm -rf "$TEST_DIR/.workhive/snapshots"
mkdir -p "$TEST_DIR/.workhive/snapshots"
persistence_has_snapshot "nonexistent-branch"
assert_failure $?

test_case "persistence_latest_snapshot_info returns empty path with none when snapshot is missing"
snapshot_info=$(persistence_latest_snapshot_info)
snapshot_path="${snapshot_info%%|*}"
snapshot_time="${snapshot_info#*|}"
assert_empty "$snapshot_path"
assert_eq "none" "$snapshot_time"

test_case "persistence_latest_snapshot_info returns path and time when snapshot exists"
snapshot_file=$(snapshot_new_file_path)
snapshot_update_last "$snapshot_file"
latest_file=$(snapshot_get_latest)
snapshot_info=$(persistence_latest_snapshot_info)
snapshot_path="${snapshot_info%%|*}"
snapshot_time="${snapshot_info#*|}"
assert_eq "$latest_file" "$snapshot_path"
assert_not_empty "$snapshot_time"

test_case "persistence_latest_snapshot_time_or returns fallback when snapshot is missing"
rm -f "$TEST_DIR/.workhive/snapshots/wh_snapshot_latest.jsonl"
result=$(persistence_latest_snapshot_time_or "iso" "unknown")
assert_eq "unknown" "$result"

test_case "persistence_latest_snapshot_time_or returns time when snapshot exists"
snapshot_file=$(snapshot_new_file_path)
snapshot_update_last "$snapshot_file"
result=$(persistence_latest_snapshot_time_or "iso" "unknown")
assert_not_empty "$result"
assert_ne "unknown" "$result"

test_case "persistence_status outputs correct fields"
status_output=$(persistence_status)
assert_contains "$status_output" "enabled"
assert_contains "$status_output" "auto_save_interval"

test_case "_persistence_config_get handles non-associative cache variable"
_saved_cache_decl=$(declare -p _PERSISTENCE_CFG_CACHE 2>/dev/null || true)
unset _PERSISTENCE_CFG_CACHE
declare -a _PERSISTENCE_CFG_CACHE=()
value=$(_persistence_config_get "enabled" "false" 2>/dev/null)
rc=$?
unset _PERSISTENCE_CFG_CACHE
if [[ -n "$_saved_cache_decl" ]]; then
    eval "$_saved_cache_decl"
else
    declare -A _PERSISTENCE_CFG_CACHE=()
fi
assert_success $rc
assert_eq "true" "$value"

test_case "_auto_save_sq_escape preserves value when re-embedded in single quotes"
raw_path="/tmp/it's-worktree"
escaped_path=$(_auto_save_sq_escape "$raw_path")
roundtrip_path=""  # initialized before eval assignment (SC2154)
eval "roundtrip_path='${escaped_path}'"
assert_eq "$raw_path" "$roundtrip_path"

test_case "_auto_save_build_trigger_cmd uses configured persistence env assignments"
_saved_git_root_for_auto_cmd="$(declare -f _git_root || true)"
_saved_prefix_for_auto_cmd="$(declare -f session_get_prefix || true)"
_saved_find_bin_for_auto_cmd="$(declare -f _auto_save_find_bin || true)"
_saved_auto_save_env_getter="$(declare -f config_get_persistence_auto_save_env_entries || true)"
_git_root() { echo "$TEST_DIR"; }
session_get_prefix() { echo "wh@abcd@test-project"; }
_auto_save_find_bin() { echo "/tmp/workhive-bin"; }
config_get_persistence_auto_save_env_entries() {
    cat <<'EOF'
WORKHIVE_SAVE_LOCK_QUIET=1
WORKHIVE_SAVE_QUALITY_QUIET=1
EOF
}
cmd=$(_auto_save_build_trigger_cmd "wh@abcd@test-project@main" "/tmp/workhive-bin")
assert_contains "$cmd" "WORKHIVE_SAVE_LOCK_QUIET=1" "trigger command should keep lock quiet mode"
assert_contains "$cmd" "WORKHIVE_SAVE_QUALITY_QUIET=1" "trigger command should keep quality gate quiet mode"
unset -f _git_root session_get_prefix _auto_save_find_bin config_get_persistence_auto_save_env_entries
[[ -n "$_saved_git_root_for_auto_cmd" ]] && eval "$_saved_git_root_for_auto_cmd"
[[ -n "$_saved_prefix_for_auto_cmd" ]] && eval "$_saved_prefix_for_auto_cmd"
[[ -n "$_saved_find_bin_for_auto_cmd" ]] && eval "$_saved_find_bin_for_auto_cmd"
[[ -n "$_saved_auto_save_env_getter" ]] && eval "$_saved_auto_save_env_getter"

test_case "_auto_save_build_trigger_cmd clears inherited managed env before applying config env"
cmd=$(_auto_save_build_trigger_cmd "wh@abcd@test-project@main" "/tmp/workhive-bin")
assert_contains "$cmd" "unset CLAUDE_CODE_NO_FLICKER;" "auto-save trigger should clear inherited startup env"
assert_contains "$cmd" "unset WORKHIVE_SAVE_LOCK_QUIET;" "auto-save trigger should clear inherited persistence env"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Additional Snapshot Coverage Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Additional Coverage"

test_case "snapshot_cleanup returns 0 (no-op in single-file model)"
snapshot_cleanup
assert_success $?

test_case "snapshot_get_dir delegates to snapshot_get_project_dir"
result1=$(snapshot_get_dir)
result2=$(snapshot_get_project_dir)
assert_eq "$result1" "$result2"

test_case "snapshot_format_meta includes all required fields"
line=$(snapshot_format_meta "test-id" "my-project" "wh@test-id@my-project" "2.0.0")
assert_contains "$line" '"type":"meta"'
assert_contains "$line" '"version":"3"'
assert_contains "$line" '"instance_id":"test-id"'
assert_contains "$line" '"project":"my-project"'
assert_contains "$line" '"session_prefix":"wh@test-id@my-project"'
assert_contains "$line" '"workhive_version":"2.0.0"'
assert_contains "$line" '"schema":"workhive.snapshot"'

test_case "snapshot_format_window includes all required fields"
line=$(snapshot_format_window "wh@test@main" "0" "main-win" "1" "*" "layout-str" "1")
assert_contains "$line" '"type":"window"'
assert_contains "$line" '"session":"wh@test@main"'
assert_contains "$line" '"window_index":"0"'
assert_contains "$line" '"window_name":"main-win"'
assert_contains "$line" '"window_layout":"layout-str"'

test_case "snapshot_format_state includes all required fields"
line=$(snapshot_format_state "wh@test@main" "wh@test@feature")
assert_contains "$line" '"type":"state"'
assert_contains "$line" '"active_session":"wh@test@main"'
assert_contains "$line" '"last_session":"wh@test@feature"'

test_case "snapshot_format_state handles empty last_session"
line=$(snapshot_format_state "wh@test@main" "")
assert_contains "$line" '"last_session":""'

test_case "snapshot_has_session_exact returns 0 for existing session"
exact_file="$TEST_DIR/has_session_exact_test.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp" "1" "bash" ""
    snapshot_format_pane "wh@abcd@test@feature" "0" "1" "*" "0" "feat" "/tmp" "1" "bash" ""
} > "$exact_file"
snapshot_has_session_exact "$exact_file" "wh@abcd@test@main"
assert_success $?

test_case "snapshot_has_session_exact returns 1 for non-existent session"
snapshot_has_session_exact "$exact_file" "wh@abcd@test@nonexistent"
assert_failure $?

test_case "snapshot_has_session_exact returns 1 for partial match"
snapshot_has_session_exact "$exact_file" "wh@abcd@test@mai"
assert_failure $?

test_case "snapshot_has_session_exact returns 1 for empty file"
empty_file="$TEST_DIR/has_session_empty.jsonl"
: > "$empty_file"
snapshot_has_session_exact "$empty_file" "any"
assert_failure $?

test_case "snapshot_has_session_exact returns 1 for non-existent file"
snapshot_has_session_exact "/nonexistent/path/file.jsonl" "any"
assert_failure $?

test_case "snapshot_session_health_score returns weighted score for active session"
health_file="$TEST_DIR/health_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "main" "/tmp" "1" "vim" "vim app.sh"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "1" "shell" "/tmp" "1" "bash" ""
} > "$health_file"
health_score=$(snapshot_session_health_score "$health_file" "wh@abcd@test@main")
assert_eq "50" "$health_score" "score should follow weighted formula for full+active pane mix"

test_case "snapshot_session_health_score returns 0 for missing session"
missing_score=$(snapshot_session_health_score "$health_file" "wh@abcd@test@missing" 2>/dev/null)
assert_eq "0" "$missing_score" "missing session should report zero score"

test_case "snapshot_escape_json_string handles carriage return"
result=$(snapshot_escape_json_string $'hello\rworld')
assert_eq 'hello\rworld' "$result"

test_case "snapshot_escape_json_string handles empty string"
result=$(snapshot_escape_json_string "")
assert_eq "" "$result"

test_case "snapshot_parse_json_string_literal handles null"
snapshot_parse_json_string_literal 'null'
assert_success $?
assert_eq "" "$REPLY"

test_case "snapshot_parse_json_string_literal rejects unquoted value"
snapshot_parse_json_string_literal 'noquotes'
assert_failure $?

test_case "snapshot_parse_json_string_literal handles tab escape"
snapshot_parse_json_string_literal '"col1\tcol2"'
assert_success $?
assert_eq $'col1\tcol2' "$REPLY"

test_case "snapshot_parse_json_type returns type from JSON line"
result=$(snapshot_parse_json_type '{"type":"pane","session":"s1"}')
assert_eq "pane" "$result"

test_case "snapshot_parse_json_type returns empty for no-type JSON"
result=$(snapshot_parse_json_type '{"session":"s1"}')
assert_eq "" "$result"

test_case "snapshot_parse_json_string_field extracts single field"
result=$(snapshot_parse_json_string_field '{"type":"pane","session":"s1"}' "session")
assert_eq "s1" "$result"

test_case "snapshot_parse_json_string_field returns empty for missing key"
result=$(snapshot_parse_json_string_field '{"type":"pane"}' "nonexistent")
assert_eq "" "$result"

test_case "snapshot_detect_version returns empty for empty file"
empty_version="$TEST_DIR/snapshot_empty_version.txt"
: > "$empty_version"
result=$(snapshot_detect_version "$empty_version" 2>/dev/null || echo "")
assert_empty "$result"

test_case "snapshot_detect_version returns empty for nonexistent file"
result=$(snapshot_detect_version "/nonexistent/file.txt" 2>/dev/null || echo "")
assert_empty "$result"

test_case "snapshot_validate passes with empty expected_id"
valid_file="$TEST_DIR/snapshot_validate_empty_id.jsonl"
{
    snapshot_format_meta "abcd" "test" "wh@abcd@test" "1.0"
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "$valid_file"
snapshot_validate "$valid_file" ""
assert_success $?

test_case "snapshot_update_last moves tmp to latest"
_cleanup_dir="$TEST_DIR/.workhive/snapshots_cleanup_test"
mkdir -p "$_cleanup_dir"
echo "test" > "$_cleanup_dir/wh_snapshot_tmp.jsonl"
ln -s "old_target" "$_cleanup_dir/last" 2>/dev/null || true
snapshot_update_last "$_cleanup_dir/wh_snapshot_tmp.jsonl"
assert_true "[[ -f '${_cleanup_dir}/wh_snapshot_latest.jsonl' ]]" "latest should exist"
assert_true "[[ ! -f '${_cleanup_dir}/wh_snapshot_tmp.jsonl' ]]" "tmp should be moved"

test_case "snapshot_has_changed returns 0 when last file missing"
snapshot_has_changed "/any/file" "/nonexistent/last"
assert_success $?

test_case "snapshot_format_pane roundtrip preserves fields"
line=$(snapshot_format_pane "wh@test@main" "0" "1" "*" "0" "mytitle" "/Users/test dir" "1" "bash" "vim a.sh")
snapshot_parse_pane_line "$line"
assert_eq "wh@test@main" "$_SP_SESSION"
assert_eq "0" "$_SP_WIN_IDX"
assert_eq "1" "$_SP_WIN_ACTIVE"
assert_eq "0" "$_SP_PANE_IDX"
assert_eq "1" "$_SP_PANE_ACTIVE"
assert_eq "bash" "$_SP_PANE_CMD"

test_case "snapshot_format_window roundtrip preserves fields"
line=$(snapshot_format_window "wh@test@main" "0" "winname" "1" "*" "layout-str" "1")
snapshot_parse_window_line "$line"
assert_eq "wh@test@main" "$_SW_SESSION"
assert_eq "0" "$_SW_WIN_IDX"
assert_eq "winname" "$_SW_WIN_NAME"
assert_eq "layout-str" "$_SW_LAYOUT"

test_case "snapshot_escape_json_string handles tab in value"
result=$(snapshot_escape_json_string "col1	col2")
assert_not_contains "$result" $'\t' "tab should be escaped"
snapshot_parse_json_string_literal "\"$result\""
assert_success $?
assert_contains "$REPLY" $'\t' "tab should be restored"

test_case "snapshot_get_global_dir without instance_id returns base path"
result=$(snapshot_get_global_dir "")
assert_not_contains "$result" "//" "should not have double slashes"
assert_contains "$result" "workhive/snapshots"

test_case "snapshot_get_global_dir with instance_id appends it"
result=$(snapshot_get_global_dir "abcd")
assert_contains "$result" "workhive/snapshots/abcd"

test_case "snapshot_matches_generation allows legacy snapshots when generation is 0"
legacy_snapshot="$TEST_DIR/legacy_generation_zero.jsonl"
{
    printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot","instance_id":"abcd","project":"test","session_prefix":"wh@abcd@test"}'
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "$legacy_snapshot"
snapshot_matches_generation "$legacy_snapshot"
assert_success $?

test_case "snapshot_matches_generation rejects legacy snapshots when generation is bumped"
bumped_snapshot="$TEST_DIR/legacy_generation_mismatch.jsonl"
{
    printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot","instance_id":"abcd","project":"test","session_prefix":"wh@abcd@test"}'
    snapshot_format_pane "wh@abcd@test@main" "0" "1" "*" "0" "title" "/tmp" "1" "bash" ""
} > "$bumped_snapshot"
_orig_persist_cfg_generation="$(declare -f persistence_cfg_generation 2>/dev/null || true)"
persistence_cfg_generation() { echo "2"; }
(snapshot_matches_generation "$bumped_snapshot")
assert_failure $? "legacy snapshot should be rejected after generation bump"
unset -f persistence_cfg_generation
[[ -n "$_orig_persist_cfg_generation" ]] && eval "$_orig_persist_cfg_generation"

test_report

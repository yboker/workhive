#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: Persistence Restore, Diagnostics & Reconciliation
# Split from test_persistence.sh for maintainability.
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
source "$LIB_DIR/infra/persistence/save.sh"
source "$LIB_DIR/infra/persistence/restore.sh"
source "$LIB_DIR/infra/persistence/auto_save.sh"
source "$LIB_DIR/infra/persistence/manager.sh"
source "$LIB_DIR/app/commands/open_command.sh"

session_get_prefix() { echo "wh@abcd@test-project"; }
session_get_name() { echo "wh@abcd@test-project@$1"; }

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Diagnostics & CWD Restore Semantics
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Diagnostics"

test_case "restore sends cd when snapshot cwd differs from current pane cwd"
repo_dir="$TEST_DIR/repo-cwd"
mkdir -p "$repo_dir"
snapshot_file="$TEST_DIR/restore_cwd_diff_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$repo_dir" "1" "bash" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

SEND_KEYS_CALLS=""
CURRENT_PANE_DIR="/tmp/not-repo"
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        display-message)
            if [[ "$*" == *"pane_current_path"* ]]; then
                echo "$CURRENT_PANE_DIR"
            fi
            return 0
            ;;
        send-keys)
            SEND_KEYS_CALLS+="|$*"
            return 0
            ;;
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "wh@abcd@test-project@main"
assert_contains "$SEND_KEYS_CALLS" "cd ${repo_dir}"

test_case "restore keeps HOME cwd semantics and sends cd HOME when snapshot is HOME"
TEST_HOME_DIR="$TEST_DIR/home-cwd"
mkdir -p "$TEST_HOME_DIR"
export HOME="$TEST_HOME_DIR"
snapshot_file="$TEST_DIR/restore_home_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$HOME" "1" "bash" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

SEND_KEYS_CALLS=""
CURRENT_PANE_DIR="/tmp/not-home"
_restore_session_from_snapshot "$snapshot_file" "wh@abcd@test-project@main"
assert_contains "$SEND_KEYS_CALLS" "cd ${HOME}"

test_case "restore sends empty Enter for idle shell panes to normalize prompt rendering"
snapshot_file="$TEST_DIR/restore_idle_shell_prompt_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$repo_dir" "1" "zsh" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

SEND_KEYS_CALLS=""
CURRENT_PANE_DIR="$repo_dir"
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        display-message)
            if [[ "$*" == *"pane_current_path"* ]]; then
                echo "$CURRENT_PANE_DIR"
            fi
            return 0
            ;;
        capture-pane)
            echo "%"
            return 0
            ;;
        send-keys)
            SEND_KEYS_CALLS+="|$*"
            return 0
            ;;
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "$(session_get_name main)"
assert_contains "$SEND_KEYS_CALLS" "C-m"

test_case "restore skips empty Enter when shell prompt marker is not detected"
snapshot_file="$TEST_DIR/restore_idle_shell_prompt_no_marker_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$repo_dir" "1" "zsh" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

SEND_KEYS_CALLS=""
CURRENT_PANE_DIR="$repo_dir"
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        display-message)
            if [[ "$*" == *"pane_current_path"* ]]; then
                echo "$CURRENT_PANE_DIR"
            fi
            return 0
            ;;
        capture-pane)
            echo "$"
            return 0
            ;;
        send-keys)
            SEND_KEYS_CALLS+="|$*"
            return 0
            ;;
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "$(session_get_name main)"
assert_not_contains "$SEND_KEYS_CALLS" "C-m"

test_case "restore unwraps tmux startup wrapper full_command and restores tool process"
snapshot_file="$TEST_DIR/restore_wrapped_cmd_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$repo_dir" "1" "zsh" '/bin/zsh -lc clear >/dev/null 2>&1 || true; printf "\033[2J\033[H\033[3J" >/dev/null 2>&1 || true; _startup_interrupted=false; trap '\''_startup_interrupted=true'\'' INT TERM; (trap - INT TERM; eval -- "$1"); _startup_rc=$?; trap - INT TERM; if [[ "$_startup_interrupted" == true ]]; then exit 130; fi; if (( _startup_rc >= 128 )); then exit "$_startup_rc"; fi; exec "$2" -l _ CLAUDE_CODE_NO_FLICKER=1 WORKHIVE_SAVE_LOCK_QUIET=1 codex /bin/zsh'
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

SPAWN_CMD_CAPTURE=""
terminal_adapter_spawn_command_target() {
    local _target="$1"
    local _cmd="$2"
    local _workdir="$3"
    SPAWN_CMD_CAPTURE="${_target}|${_cmd}|${_workdir}"
    return 0
}
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        display-message)
            if [[ "$*" == *"pane_current_path"* ]]; then
                echo "$repo_dir"
            fi
            return 0
            ;;
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "$(session_get_name main)"
assert_contains "$SPAWN_CMD_CAPTURE" "|codex|"

test_case "save diagnostics include pane cwd pane command full command and save reason"
SAVE_DEBUG_LOGS=""
WORKHIVE_DEBUG=1
process_capture_full_command() { echo "vim app.sh"; }
log_debug() { SAVE_DEBUG_LOGS+="|$1"; }
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        list-panes)
            printf "wh@abcd@test-project@main\t0\t1\t*\t0\tmain\t/tmp/save-diag\t1\tbash\t123\n"
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

_save_dump_panes "wh@abcd@test-project" "before-kill-session" "remove_command" >/dev/null
assert_contains "$SAVE_DEBUG_LOGS" "cmd=bash"
assert_contains "$SAVE_DEBUG_LOGS" "full=vim (...)"

# WORKHIVE_DEBUG off by default to avoid cross-test leakage
WORKHIVE_DEBUG=0

# Reset tmux stub to minimal default for later tests
unset -f tmux
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

# Reset capture helper to test stub default
unset -f process_capture_full_command
process_capture_full_command() { echo ""; }

test_case "save keeps snapshot stdout clean in debug mode"
WORKHIVE_DEBUG=1
snapshot_file="$TEST_DIR/clean_snapshot_debug.txt"

# Intentionally write debug logs to stdout to prove save path keeps snapshot stream clean.
log_debug() {
    if [[ "${WORKHIVE_DEBUG:-}" == "1" ]]; then
        echo "[DEBUG] $1"
    fi
}

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        list-sessions)
            if [[ "$*" == *"-F"* ]]; then
                printf "wh@abcd@test-project@main\n"
            fi
            return 0
            ;;
        list-panes)
            printf "wh@abcd@test-project@main\t0\t1\t*\t0\tmain\t/tmp/clean-debug\t1\tbash\t123\n"
            return 0
            ;;
        list-windows)
            printf "wh@abcd@test-project@main\t0\tmain\t1\t*\tlayout\t1\n"
            return 0
            ;;
        display-message)
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

snapshot_get_latest() { echo ""; }
snapshot_new_file_path() { echo "$snapshot_file"; }
snapshot_update_last() { return 0; }
snapshot_has_changed() { return 0; }
process_capture_full_command() { echo "vim app.sh"; }

persistence_save "auto-save" "unit-test" >/dev/null
assert_success $?
snapshot_content=$(cat "$snapshot_file")
assert_contains "$snapshot_content" '"type":"pane"'
assert_not_contains "$snapshot_content" "[DEBUG]"

# Restore snapshot helpers to real functions for subsequent tests.
unset -f snapshot_get_latest
unset -f snapshot_new_file_path
unset -f snapshot_update_last
unset -f snapshot_has_changed

source "$LIB_DIR/infra/persistence/snapshot.sh"

test_case "persistence_save writes current jsonl snapshot records"
WORKHIVE_DEBUG=0
snapshot_file="$TEST_DIR/save_v2_snapshot.txt"

log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_error() { :; }

snapshot_get_latest() { echo ""; }
snapshot_new_file_path() { echo "$snapshot_file"; }
snapshot_update_last() { return 0; }
snapshot_has_changed() { return 0; }
process_capture_full_command() { echo "vim app.sh"; }

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        list-sessions)
            if [[ "$*" == *"-F"* ]]; then
                printf "wh@abcd@test-project@main\n"
            fi
            return 0
            ;;
        list-panes)
            printf "wh@abcd@test-project@main\t0\t1\t*\t0\tmain\t/tmp/save-v2\t1\tbash\t123\n"
            return 0
            ;;
        list-windows)
            printf "wh@abcd@test-project@main\t0\tmain\t1\t*\tlayout\t1\n"
            return 0
            ;;
        display-message)
            if [[ "$*" == *"client_session"* ]]; then
                printf "wh@abcd@test-project@main\n"
            else
                printf "\n"
            fi
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

persistence_save "auto-save" "unit-test" >/dev/null
assert_success $?

snapshot_content=$(cat "$snapshot_file")
assert_contains "$snapshot_content" '"type":"meta"'
assert_contains "$snapshot_content" '"type":"pane"'
assert_contains "$snapshot_content" '"type":"window"'
assert_contains "$snapshot_content" '"type":"state"'

# Restore snapshot helpers to real functions for following tests.
unset -f snapshot_get_latest
unset -f snapshot_new_file_path
unset -f snapshot_update_last
unset -f snapshot_has_changed
source "$LIB_DIR/infra/persistence/snapshot.sh"

test_case "persistence_save carries dormant sessions from previous snapshot"
WORKHIVE_DEBUG=0
snapshot_file="$TEST_DIR/save_v2_with_dormant_snapshot.txt"
previous_file="$TEST_DIR/save_v2_previous_snapshot.txt"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "0.9.0"
    snapshot_format_pane "wh@abcd@test-project@feature-old" "0" "1" "*" "0" "old" "/tmp/old" "1" "bash" "vim old.sh"
    snapshot_format_window "wh@abcd@test-project@feature-old" "0" "old" "1" "*" "layout" "1"
    snapshot_format_state "wh@abcd@test-project@feature-old" ""
} > "$previous_file"

log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_error() { :; }

snapshot_get_latest() { echo "$previous_file"; }
snapshot_new_file_path() { echo "$snapshot_file"; }
snapshot_update_last() { return 0; }
snapshot_has_changed() { return 0; }
process_capture_full_command() { echo "vim app.sh"; }

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        list-sessions)
            if [[ "$*" == *"-F"* ]]; then
                printf "wh@abcd@test-project@main\n"
            fi
            return 0
            ;;
        list-panes)
            printf "wh@abcd@test-project@main\t0\t1\t*\t0\tmain\t/tmp/save-v2\t1\tbash\t123\n"
            return 0
            ;;
        list-windows)
            printf "wh@abcd@test-project@main\t0\tmain\t1\t*\tlayout\t1\n"
            return 0
            ;;
        display-message)
            if [[ "$*" == *"client_session"* ]]; then
                printf "wh@abcd@test-project@main\n"
            else
                printf "\n"
            fi
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

persistence_save "auto-save" "unit-test" >/dev/null
assert_success $?
snapshot_content=$(cat "$snapshot_file")
assert_contains "$snapshot_content" '"session":"wh@abcd@test-project@main"'
assert_contains "$snapshot_content" '"session":"wh@abcd@test-project@feature-old"'

# Restore snapshot helpers to real functions for following tests.
unset -f snapshot_get_latest
unset -f snapshot_new_file_path
unset -f snapshot_update_last
unset -f snapshot_has_changed
source "$LIB_DIR/infra/persistence/snapshot.sh"

test_case "persistence_save carries current dormant records and excludes running sessions"
WORKHIVE_DEBUG=0
snapshot_file="$TEST_DIR/save_v2_dormant_excl_snapshot.jsonl"
previous_file="$TEST_DIR/save_v2_dormant_excl_previous.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "0.9.0"
    snapshot_format_pane "wh@abcd@test-project@feature-old" "0" "1" "*" "0" "old" "/tmp/old-dormant" "1" "bash" "vim legacy.sh"
    snapshot_format_window "wh@abcd@test-project@feature-old" "0" "old" "1" "*" "layout" "1"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main-old" "/tmp/old-main" "1" "bash" "vim old-main.sh"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main-old" "1" "*" "layout" "1"
    snapshot_format_state "wh@abcd@test-project@main" ""
} > "$previous_file"

log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_error() { :; }

snapshot_get_latest() { echo "$previous_file"; }
snapshot_new_file_path() { echo "$snapshot_file"; }
snapshot_update_last() { return 0; }
snapshot_has_changed() { return 0; }
process_capture_full_command() { echo "vim app.sh"; }

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        list-sessions)
            if [[ "$*" == *"-F"* ]]; then
                printf "wh@abcd@test-project@main\n"
            fi
            return 0
            ;;
        list-panes)
            printf "wh@abcd@test-project@main\t0\t1\t*\t0\tmain\t/tmp/save-v2\t1\tbash\t123\n"
            return 0
            ;;
        list-windows)
            printf "wh@abcd@test-project@main\t0\tmain\t1\t*\tlayout\t1\n"
            return 0
            ;;
        display-message)
            if [[ "$*" == *"client_session"* ]]; then
                printf "wh@abcd@test-project@main\n"
            else
                printf "\n"
            fi
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

persistence_save "auto-save" "unit-test" >/dev/null
assert_success $?
snapshot_content=$(cat "$snapshot_file")
assert_contains "$snapshot_content" '"session":"wh@abcd@test-project@feature-old"'
assert_contains "$snapshot_content" '"dir":"/tmp/old-dormant"'
assert_not_contains "$snapshot_content" '/tmp/old-main'

# Restore snapshot helpers to real functions for following tests.
unset -f snapshot_get_latest
unset -f snapshot_new_file_path
unset -f snapshot_update_last
unset -f snapshot_has_changed
source "$LIB_DIR/infra/persistence/snapshot.sh"

test_case "restore and manager diagnostics include cwd restore and keep skip-layout detail in debug logs"
RESTORE_DEBUG_LOGS=""
MANAGER_DEBUG_LOGS=""
MANAGER_INFO_LOGS=""
repo_dir="$TEST_DIR/repo-diag"
mkdir -p "$repo_dir"
snapshot_file="$TEST_DIR/restore_diag_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$repo_dir" "1" "bash" "vim app.sh"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

log_debug() {
    RESTORE_DEBUG_LOGS+="|$1"
    MANAGER_DEBUG_LOGS+="|$1"
}
log_info() { MANAGER_INFO_LOGS+="|$1"; }
_persistence_config_get() { echo "true"; }
process_should_restore() { return 0; }
process_resolve_restore_command() { echo "vim app.sh"; }
persistence_has_snapshot() { return 0; }
snapshot_get_latest() { echo "$snapshot_file"; }
cache_invalidate() { return 0; }
session_get_name() { echo "wh@abcd@test-project@$1"; }

CURRENT_PANE_DIR="/tmp/restore-diag-current"
SEND_KEYS_CALLS=""
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window|switch-client)
            return 0
            ;;
        display-message)
            if [[ "$*" == *"pane_current_path"* ]]; then
                echo "$CURRENT_PANE_DIR"
            fi
            return 0
            ;;
        send-keys)
            SEND_KEYS_CALLS+="|$*"
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "wh@abcd@test-project@main"
persistence_try_restore_session "main"
assert_contains "$RESTORE_DEBUG_LOGS" "resolved_cwd=${repo_dir}"
assert_contains "$RESTORE_DEBUG_LOGS" "cd_sent="
assert_contains "$RESTORE_DEBUG_LOGS" "full_cmd_restored=true"
assert_contains "$MANAGER_DEBUG_LOGS" "fallback=layout_create"
assert_not_contains "$MANAGER_INFO_LOGS" "skip layout create"

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Restore Validation & Fail-safe Tests
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

test_suite "Persistence — Removed Hook Reconciliation"

test_case "open_command.sh no longer contains reconciliation hook callback"
(grep -q 'client_reconciliation' "$LIB_DIR/app/commands/open_command.sh" && exit 0 || exit 1)
assert_eq "1" "$?" "open_command.sh should not contain reconciliation hook call"

test_case "attach_command.sh does not trigger client reconciliation hook directly"
(grep -q 'client_reconciliation' "$LIB_DIR/app/commands/attach_command.sh" && exit 0 || exit 1)
assert_eq "1" "$?" "attach_command.sh should not contain reconciliation hook call"

test_case "open command restored path still auto-attaches without hook callbacks"
_OPEN_RESTORE_ATTACH_CALLS=""

_open_orig_workspace_open_def="$(declare -f workspace_open)"
_open_orig_session_get_name_def="$(declare -f session_get_name)"
_open_orig_open_ensure_layout_def="$(declare -f _open_ensure_layout)"
_open_orig_open_gc_hint_def="$(declare -f _open_gc_hint)"
_open_orig_open_print_summary_def="$(declare -f _open_print_workspace_summary)"
_open_orig_session_attach_def="$(declare -f session_attach)"
_open_orig_ensure_workhive_dir_def="$(declare -f _ensure_workhive_dir)"
_open_orig_deps_reconcile_def="$(declare -f deps_reconcile)"
_open_orig_config_get_def="$(declare -f config_get)"
_open_orig_git_current_branch_def="$(declare -f _git_current_branch)"
_open_orig_git_subpath_offset_def="$(declare -f _git_subpath_offset)"
_open_orig_git_root_def="$(declare -f _git_root)"

workspace_open() {
    WORKSPACE_RESULT_DIR="$TEST_DIR/open-restore-workdir"
    # shellcheck disable=SC2034
    WORKSPACE_RESTORED_FROM_SNAPSHOT=true
    mkdir -p "$WORKSPACE_RESULT_DIR"
    return 0
}
session_get_name() { printf 'wh@test@%s\n' "$1"; }
_open_ensure_layout() { return 0; }
_open_gc_hint() { :; }
_open_print_workspace_summary() { :; }
session_attach() {
    _OPEN_RESTORE_ATTACH_CALLS+="|$1"
    return 0
}
_ensure_workhive_dir() { :; }
deps_reconcile() { :; }
config_get() {
    case "$1" in
        tmux.attach_delay) echo "0" ;;
        *) echo "" ;;
    esac
}
_git_current_branch() { echo "main"; }
_git_subpath_offset() { echo ""; }
_git_root() { echo "$TEST_DIR"; }

open_command_execute "restore-hook-branch"
assert_contains "$_OPEN_RESTORE_ATTACH_CALLS" "restore-hook-branch" "restored auto-attach path should still attach"

_OPEN_RESTORE_ATTACH_CALLS=""
open_command_execute "restore-hook-branch" --no-attach
assert_empty "$_OPEN_RESTORE_ATTACH_CALLS" "--no-attach path should not call session_attach"

unset -f workspace_open session_get_name _open_ensure_layout _open_gc_hint _open_print_workspace_summary session_attach _ensure_workhive_dir deps_reconcile config_get _git_current_branch _git_subpath_offset _git_root
[[ -n "$_open_orig_workspace_open_def" ]] && eval "$_open_orig_workspace_open_def"
[[ -n "$_open_orig_session_get_name_def" ]] && eval "$_open_orig_session_get_name_def"
[[ -n "$_open_orig_open_ensure_layout_def" ]] && eval "$_open_orig_open_ensure_layout_def"
[[ -n "$_open_orig_open_gc_hint_def" ]] && eval "$_open_orig_open_gc_hint_def"
[[ -n "$_open_orig_open_print_summary_def" ]] && eval "$_open_orig_open_print_summary_def"
[[ -n "$_open_orig_session_attach_def" ]] && eval "$_open_orig_session_attach_def"
[[ -n "$_open_orig_ensure_workhive_dir_def" ]] && eval "$_open_orig_ensure_workhive_dir_def"
[[ -n "$_open_orig_deps_reconcile_def" ]] && eval "$_open_orig_deps_reconcile_def"
[[ -n "$_open_orig_config_get_def" ]] && eval "$_open_orig_config_get_def"
[[ -n "$_open_orig_git_current_branch_def" ]] && eval "$_open_orig_git_current_branch_def"
[[ -n "$_open_orig_git_subpath_offset_def" ]] && eval "$_open_orig_git_subpath_offset_def"
[[ -n "$_open_orig_git_root_def" ]] && eval "$_open_orig_git_root_def"

test_case "open restored snapshot should not trigger layout rebuild when layout_validate fails"
_OPEN_RESTORE_LAYOUT_ENSURE_CALLED=0

_open_orig_workspace_open_def="$(declare -f workspace_open)"
_open_orig_session_get_name_def="$(declare -f session_get_name)"
_open_orig_open_ensure_layout_def="$(declare -f _open_ensure_layout)"
_open_orig_layout_validate_def="$(declare -f layout_validate)"
_open_orig_open_gc_hint_def="$(declare -f _open_gc_hint)"
_open_orig_open_print_summary_def="$(declare -f _open_print_workspace_summary)"
_open_orig_session_attach_def="$(declare -f session_attach)"
_open_orig_ensure_workhive_dir_def="$(declare -f _ensure_workhive_dir)"
_open_orig_deps_reconcile_def="$(declare -f deps_reconcile)"
_open_orig_config_get_def="$(declare -f config_get)"
_open_orig_git_current_branch_def="$(declare -f _git_current_branch)"
_open_orig_git_subpath_offset_def="$(declare -f _git_subpath_offset)"
_open_orig_git_root_def="$(declare -f _git_root)"

workspace_open() {
    WORKSPACE_RESULT_DIR="$TEST_DIR/open-restore-workdir-validate-fail"
    # shellcheck disable=SC2034
    WORKSPACE_RESTORED_FROM_SNAPSHOT=true
    # shellcheck disable=SC2034
    WORKSPACE_USED_EXISTING_SESSION=false
    mkdir -p "$WORKSPACE_RESULT_DIR"
    return 0
}
session_get_name() { printf 'wh@test@%s\n' "$1"; }
layout_validate() { return 1; }
_open_ensure_layout() {
    _OPEN_RESTORE_LAYOUT_ENSURE_CALLED=$((_OPEN_RESTORE_LAYOUT_ENSURE_CALLED + 1))
    return 0
}
_open_gc_hint() { :; }
_open_print_workspace_summary() { :; }
session_attach() { return 0; }
_ensure_workhive_dir() { :; }
deps_reconcile() { :; }
config_get() {
    case "$1" in
        tmux.attach_delay) echo "0" ;;
        *) echo "" ;;
    esac
}
_git_current_branch() { echo "main"; }
_git_subpath_offset() { echo ""; }
_git_root() { echo "$TEST_DIR"; }

open_command_execute "main"
assert_eq "0" "$_OPEN_RESTORE_LAYOUT_ENSURE_CALLED" "restored snapshot path should skip config-based layout rebuild"

unset -f workspace_open session_get_name layout_validate _open_ensure_layout _open_gc_hint _open_print_workspace_summary session_attach _ensure_workhive_dir deps_reconcile config_get _git_current_branch _git_subpath_offset _git_root
[[ -n "$_open_orig_workspace_open_def" ]] && eval "$_open_orig_workspace_open_def"
[[ -n "$_open_orig_session_get_name_def" ]] && eval "$_open_orig_session_get_name_def"
[[ -n "$_open_orig_open_ensure_layout_def" ]] && eval "$_open_orig_open_ensure_layout_def"
[[ -n "$_open_orig_layout_validate_def" ]] && eval "$_open_orig_layout_validate_def"
[[ -n "$_open_orig_open_gc_hint_def" ]] && eval "$_open_orig_open_gc_hint_def"
[[ -n "$_open_orig_open_print_summary_def" ]] && eval "$_open_orig_open_print_summary_def"
[[ -n "$_open_orig_session_attach_def" ]] && eval "$_open_orig_session_attach_def"
[[ -n "$_open_orig_ensure_workhive_dir_def" ]] && eval "$_open_orig_ensure_workhive_dir_def"
[[ -n "$_open_orig_deps_reconcile_def" ]] && eval "$_open_orig_deps_reconcile_def"
[[ -n "$_open_orig_config_get_def" ]] && eval "$_open_orig_config_get_def"
[[ -n "$_open_orig_git_current_branch_def" ]] && eval "$_open_orig_git_current_branch_def"
[[ -n "$_open_orig_git_subpath_offset_def" ]] && eval "$_open_orig_git_subpath_offset_def"
[[ -n "$_open_orig_git_root_def" ]] && eval "$_open_orig_git_root_def"

test_case "restore rejects malformed pane records with non-numeric indices"
snapshot_file="$TEST_DIR/restore_invalid_pane_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "invalid-win" "1" "*" "invalid-pane" "main" "/tmp/invalid" "1" "bash" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "wh@abcd@test-project@main"
assert_failure $? "malformed pane indices should fail restore"

test_case "persistence_restore_session returns failure when internal restore fails"
RESTORE_SUCCESS_LOGS=""
snapshot_file="$TEST_DIR/restore_session_failure_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "/tmp/main" "1" "bash" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_success() { RESTORE_SUCCESS_LOGS+="|$1"; }
snapshot_get_latest() { echo "$snapshot_file"; }
snapshot_validate() { return 0; }
ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF="$(declare -f _restore_session_from_snapshot)"
_restore_session_from_snapshot() { return 1; }

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

persistence_restore_session "main"
assert_failure $? "should return failure when restore phase fails"
assert_empty "$RESTORE_SUCCESS_LOGS" "should not log restore success on failure"
eval "$ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF"

test_case "restore_all_sessions continues after one session restore failure"
RESTORE_CALLS=""
RESTORE_WARN_LOGS=""

log_debug() { :; }
log_warn() { RESTORE_WARN_LOGS+="|$1"; }
snapshot_get_sessions() {
    printf "wh@abcd@test-project@main\n"
    printf "wh@abcd@test-project@feature\n"
}
ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF="$(declare -f _restore_session_from_snapshot)"
_restore_session_from_snapshot() {
    local _snapshot="$1"
    local _session="$2"
    RESTORE_CALLS+="|${_session}"
    [[ "$_snapshot" == "$TEST_DIR/dummy_snapshot.txt" ]] || return 1
    if [[ "$_session" == "wh@abcd@test-project@main" ]]; then
        return 1
    fi
    return 0
}

tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_all_sessions "$TEST_DIR/dummy_snapshot.txt"
assert_success $?
assert_contains "$RESTORE_CALLS" "|wh@abcd@test-project@main"
assert_contains "$RESTORE_CALLS" "|wh@abcd@test-project@feature"
assert_contains "$RESTORE_WARN_LOGS" "Failed to restore session"
eval "$ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF"

test_case "restore rejects malformed window records with non-numeric index"
RESTORE_WARN_LOGS=""
_test_invalid_win_dir="$TEST_DIR/invalid-window-dir"
mkdir -p "$_test_invalid_win_dir"
snapshot_file="$TEST_DIR/restore_invalid_window_snapshot.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$_test_invalid_win_dir" "1" "bash" ""
    snapshot_format_window "wh@abcd@test-project@main" "invalid-win" "main" "1" "*" "layout" "1"
} > "$snapshot_file"

log_warn() { RESTORE_WARN_LOGS+="|$1"; }
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 1
            ;;
        new-session|new-window|split-window|resize-pane|select-layout|rename-window|select-pane|select-window)
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

_restore_session_from_snapshot "$snapshot_file" "wh@abcd@test-project@main"
assert_failure $? "malformed window index should fail restore"
assert_contains "$RESTORE_WARN_LOGS" "Invalid window data in snapshot"

test_case "snapshot_history_limit respects config and env fallback"
persistence_cfg_snapshot_history_limit() { echo "7"; }
limit_value="$(snapshot_history_limit)"
assert_eq "7" "$limit_value" "config value should take precedence"
unset -f persistence_cfg_snapshot_history_limit

WORKHIVE_SNAPSHOT_HISTORY_LIMIT="5"
limit_value="$(snapshot_history_limit)"
assert_eq "5" "$limit_value" "env fallback should be used when config getter is unavailable"
unset WORKHIVE_SNAPSHOT_HISTORY_LIMIT

test_case "snapshot_max_size_mb respects config and env fallback"
persistence_cfg_snapshot_max_size_mb() { echo "128"; }
max_mb_value="$(snapshot_max_size_mb)"
assert_eq "128" "$max_mb_value" "config max size should take precedence"
unset -f persistence_cfg_snapshot_max_size_mb

WORKHIVE_SNAPSHOT_MAX_SIZE_MB="64"
max_mb_value="$(snapshot_max_size_mb)"
assert_eq "64" "$max_mb_value" "env max size fallback should be used when config getter is unavailable"
unset WORKHIVE_SNAPSHOT_MAX_SIZE_MB

test_case "snapshot_cleanup keeps latest target and prunes old archives by configured limit"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
f1="$snap_dir/wh_snapshot_archive_20260324T100001+0800.jsonl"
f2="$snap_dir/wh_snapshot_archive_20260324T100002+0800.jsonl"
f3="$snap_dir/wh_snapshot_archive_20260324T100003+0800.jsonl"
f4="$snap_dir/wh_snapshot_archive_20260324T100004+0800.jsonl"
printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot"}' > "$f1"
printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot"}' > "$f2"
printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot"}' > "$f3"
printf '%s\n' '{"type":"meta","version":"3","schema":"workhive.snapshot"}' > "$f4"
ln -sfn "$(basename "$f4")" "$snap_dir/wh_snapshot_latest.jsonl"

persistence_cfg_snapshot_history_limit() { echo "2"; }
snapshot_cleanup
unset -f persistence_cfg_snapshot_history_limit

assert_success $? "cleanup should succeed"
assert_file_exists "$f4" "latest target must be preserved"
assert_file_exists "$f3" "newest archive within limit should be kept"
if [[ -f "$f2" ]]; then
    assert_eq "absent" "present" "second-oldest archive should be pruned when limit=2"
else
    assert_eq "absent" "absent" "second-oldest archive should be pruned when limit=2"
fi
if [[ -f "$f1" ]]; then
    assert_eq "absent" "present" "oldest archive should be pruned"
else
    assert_eq "absent" "absent" "oldest archive should be pruned"
fi

test_case "snapshot_cleanup prunes by max size budget"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
lf="$snap_dir/wh_snapshot_archive_20260324T120000+0800.jsonl"
s1="$snap_dir/wh_snapshot_archive_20260324T120001+0800.jsonl"
s2="$snap_dir/wh_snapshot_archive_20260324T120002+0800.jsonl"

printf '{"type":"meta","version":"3","schema":"workhive.snapshot"}\n' > "$lf"
head -c 700000 /dev/zero | tr '\0' 'A' >> "$lf"
cp "$lf" "$s1"
cp "$lf" "$s2"
ln -sfn "$(basename "$lf")" "$snap_dir/wh_snapshot_latest.jsonl"

persistence_cfg_snapshot_history_limit() { echo "10"; }
persistence_cfg_snapshot_max_size_mb() { echo "0"; }
snapshot_cleanup
assert_file_exists "$s2" "when size cap disabled, newer archive should remain"

persistence_cfg_snapshot_max_size_mb() { echo "1"; } # 1MB budget including latest target
snapshot_cleanup
unset -f persistence_cfg_snapshot_history_limit
unset -f persistence_cfg_snapshot_max_size_mb

assert_success $? "cleanup with size cap should succeed"
assert_file_exists "$lf" "latest target should remain under size cap"
if [[ -f "$s1" || -f "$s2" ]]; then
    assert_eq "absent" "present" "archives exceeding size budget should be pruned"
else
    assert_eq "absent" "absent" "archives exceeding size budget should be pruned"
fi

test_case "persistence_restore_session skips unhealthy latest and falls back to healthy history snapshot"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
healthy_file="$snap_dir/wh_snapshot_archive_20260324T110001+0800.jsonl"
unhealthy_file="$snap_dir/wh_snapshot_archive_20260324T110002+0800.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$healthy_file"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "zsh" ""
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$unhealthy_file"
ln -sfn "$(basename "$unhealthy_file")" "$snap_dir/wh_snapshot_latest.jsonl"

RESTORE_PATHS=""
ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF="$(declare -f _restore_session_from_snapshot)"
_restore_session_from_snapshot() {
    local _file="$1"
    RESTORE_PATHS+="|$(basename "$_file")"
    [[ "$_file" == "$healthy_file" ]]
}
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}
log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_success() { :; }

persistence_restore_session "main"
assert_success $? "restore should succeed via healthy fallback snapshot"
assert_contains "$RESTORE_PATHS" "$(basename "$healthy_file")" "healthy snapshot should be attempted"
assert_not_contains "$RESTORE_PATHS" "$(basename "$unhealthy_file")" "unhealthy latest should be skipped before restore execution"
eval "$ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF"

test_case "persistence_restore_session respects max restore attempts limit"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
limit_file_1="$snap_dir/wh_snapshot_archive_20260324T120001+0800.jsonl"
limit_file_2="$snap_dir/wh_snapshot_archive_20260324T120002+0800.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$limit_file_1"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$limit_file_2"

RESTORE_PATHS=""
ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF="$(declare -f _restore_session_from_snapshot)"
ORIGINAL_SNAPSHOT_LIST_RESTORE_CANDIDATES_DEF="$(declare -f snapshot_list_restore_candidates)"
_restore_session_from_snapshot() {
    local _file="$1"
    RESTORE_PATHS+="|$(basename "$_file")"
    if [[ "$_file" == "$limit_file_1" ]]; then
        return 1
    fi
    return 0
}
snapshot_list_restore_candidates() {
    printf '%s\n' "$limit_file_1"
    printf '%s\n' "$limit_file_2"
}
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}
log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_success() { :; }

export WORKHIVE_RESTORE_MAX_ATTEMPTS=1
persistence_restore_session "main"
limit_rc=$?
unset WORKHIVE_RESTORE_MAX_ATTEMPTS
assert_failure "$limit_rc" "restore should fail after first failed attempt when max_attempts=1"
assert_contains "$RESTORE_PATHS" "$(basename "$limit_file_1")" "first candidate should be attempted"
assert_not_contains "$RESTORE_PATHS" "$(basename "$limit_file_2")" "second candidate should be skipped once attempt limit is reached"
eval "$ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF"
eval "$ORIGINAL_SNAPSHOT_LIST_RESTORE_CANDIDATES_DEF"

test_case "persistence_restore_session supports unlimited attempts when max_attempts=0"
snap_dir="$TEST_DIR/.workhive/snapshots"
mkdir -p "$snap_dir"
unlimited_file_1="$snap_dir/wh_snapshot_archive_20260324T130001+0800.jsonl"
unlimited_file_2="$snap_dir/wh_snapshot_archive_20260324T130002+0800.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$unlimited_file_1"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
} > "$unlimited_file_2"

RESTORE_PATHS=""
ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF="$(declare -f _restore_session_from_snapshot)"
ORIGINAL_SNAPSHOT_LIST_RESTORE_CANDIDATES_DEF="$(declare -f snapshot_list_restore_candidates)"
_restore_session_from_snapshot() {
    local _file="$1"
    RESTORE_PATHS+="|$(basename "$_file")"
    [[ "$_file" == "$unlimited_file_2" ]]
}
snapshot_list_restore_candidates() {
    printf '%s\n' "$unlimited_file_1"
    printf '%s\n' "$unlimited_file_2"
}
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session) return 1 ;;
        *) return 0 ;;
    esac
}
log_debug() { :; }
log_info() { :; }
log_warn() { :; }
log_success() { :; }

export WORKHIVE_RESTORE_MAX_ATTEMPTS=0
persistence_restore_session "main"
unlimited_rc=$?
unset WORKHIVE_RESTORE_MAX_ATTEMPTS
assert_success "$unlimited_rc" "unlimited attempts should allow fallback candidate restore"
assert_contains "$RESTORE_PATHS" "$(basename "$unlimited_file_1")" "first candidate should be attempted"
assert_contains "$RESTORE_PATHS" "$(basename "$unlimited_file_2")" "second candidate should be attempted in unlimited mode"
eval "$ORIGINAL_RESTORE_SESSION_FROM_SNAPSHOT_DEF"
eval "$ORIGINAL_SNAPSHOT_LIST_RESTORE_CANDIDATES_DEF"

test_case "snapshot_has_changed ignores volatile meta fields and deduplicates identical semantic payload"
snap_a="$TEST_DIR/same_payload_a.jsonl"
snap_b="$TEST_DIR/same_payload_b.jsonl"
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.0"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
    snapshot_format_state "wh@abcd@test-project@main" ""
} > "$snap_a"
sleep 1
{
    snapshot_format_meta "abcd" "test-project" "wh@abcd@test-project" "1.1"
    snapshot_format_pane "wh@abcd@test-project@main" "0" "1" "*" "0" "main" "$TEST_DIR" "1" "codex" "codex"
    snapshot_format_window "wh@abcd@test-project@main" "0" "main" "1" "*" "layout" "1"
    snapshot_format_state "wh@abcd@test-project@main" ""
} > "$snap_b"

change_rc=0
SNAP_A="$snap_a" SNAP_B="$snap_b" LIB_DIR="$LIB_DIR" bash -lc '
    source "$LIB_DIR/infra/persistence/snapshot.sh"
    snapshot_has_changed "$SNAP_B" "$SNAP_A"
' >/dev/null 2>&1 || change_rc=$?
assert_eq "1" "$change_rc" "same semantic payload should be treated as unchanged"

test_case "auto-save mode getter falls back to hybrid on invalid value"
persistence_cfg_auto_save_mode() { echo "invalid-mode"; }
mode_value="$(_auto_save_get_mode)"
assert_eq "hybrid" "$mode_value" "invalid mode should fallback to hybrid"
unset -f persistence_cfg_auto_save_mode

test_case "auto-save hooks install in event mode but skip in interval mode"
AUTO_SAVE_TMUX_CALLS=""
_auto_save_find_bin() { echo "/bin/workhive"; }
_auto_save_build_trigger_cmd() { echo "echo save WORKHIVE_AUTO_SAVE_TRIGGER='${3:-unknown}'"; }
persistence_cfg_auto_save_mode() { echo "event"; }
tmux() {
    local cmd="$1"
    shift
    AUTO_SAVE_TMUX_CALLS+="|${cmd}|$*"
    return 0
}
auto_save_install_hooks "wh@test@main"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-hook|-t wh@test@main client-detached"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-hook|-t wh@test@main after-kill-window"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-hook|-t wh@test@main session-closed"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "WORKHIVE_AUTO_SAVE_TRIGGER='client-detached'"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "WORKHIVE_AUTO_SAVE_TRIGGER='after-kill-window'"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-option|-t wh@test@main @workhive-auto-save active"

AUTO_SAVE_TMUX_CALLS=""
persistence_cfg_auto_save_mode() { echo "interval"; }
auto_save_install_hooks "wh@test@main"
assert_not_contains "$AUTO_SAVE_TMUX_CALLS" "set-hook|-t wh@test@main"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-option|-t wh@test@main @workhive-auto-save timer-only"
unset -f persistence_cfg_auto_save_mode

test_case "auto-save timer behavior matches mode"
AUTO_SAVE_TMUX_CALLS=""
_auto_save_find_bin() { echo "/bin/workhive"; }
_auto_save_build_trigger_cmd() { echo "echo save WORKHIVE_AUTO_SAVE_TRIGGER='${3:-unknown}'"; }
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        show-options)
            echo ""
            return 0
            ;;
        *)
            AUTO_SAVE_TMUX_CALLS+="|${cmd}|$*"
            return 0
            ;;
    esac
}

persistence_cfg_auto_save_mode() { echo "event"; }
auto_save_start_timer "wh@test@main"
assert_not_contains "$AUTO_SAVE_TMUX_CALLS" "run-shell"

AUTO_SAVE_TMUX_CALLS=""
persistence_cfg_auto_save_mode() { echo "interval"; }
persistence_cfg_auto_save_interval() { echo "2m"; }
auto_save_start_timer "wh@test@main"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-option|-t wh@test@main @workhive-save-interval 120"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "run-shell|-t wh@test@main -b"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "WORKHIVE_AUTO_SAVE_TRIGGER='timer'"

AUTO_SAVE_TMUX_CALLS=""
persistence_cfg_auto_save_mode() { echo "hybrid"; }
persistence_cfg_auto_save_interval() { echo "7m"; }
auto_save_start_timer "wh@test@main"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "set-option|-t wh@test@main @workhive-save-interval 420"
assert_contains "$AUTO_SAVE_TMUX_CALLS" "run-shell|-t wh@test@main -b"
unset -f persistence_cfg_auto_save_mode
unset -f persistence_cfg_auto_save_interval

test_case "debounced runtime save skips repeated trigger within debounce window"
SAVE_IF_CHANGED_CALLS=0
ORIG_PERSISTENCE_SAVE_IF_RUNTIME_CHANGED_DEF="$(declare -f persistence_save_if_runtime_changed)"
persistence_save_if_runtime_changed() {
    SAVE_IF_CHANGED_CALLS=$((SAVE_IF_CHANGED_CALLS + 1))
    return 0
}
date() { echo "100"; }
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        show-options)
            echo "99"
            return 0
            ;;
        set-option)
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}
debounce_rc=0
persistence_save_runtime_debounced "wh@test@main" "3" || debounce_rc=$?
assert_eq "2" "$debounce_rc" "debounced call should be skipped"
assert_eq "0" "$SAVE_IF_CHANGED_CALLS" "no save-if-changed call when debounced"
eval "$ORIG_PERSISTENCE_SAVE_IF_RUNTIME_CHANGED_DEF"
unset -f date

test_case "runtime signature deduplicates unchanged state and saves on change"
SAVE_CALLS=0
SIG_OPTION=""
RUNTIME_VARIANT="A"
persistence_save() {
    SAVE_CALLS=$((SAVE_CALLS + 1))
    return 0
}
tmux() {
    local cmd="$1"
    shift
    case "$cmd" in
        has-session)
            return 0
            ;;
        list-panes)
            if [[ "$RUNTIME_VARIANT" == "A" ]]; then
                echo "wh@test@main|0|0|/a|zsh|1|pane"
            else
                echo "wh@test@main|0|0|/b|zsh|1|pane"
            fi
            return 0
            ;;
        list-windows)
            if [[ "$RUNTIME_VARIANT" == "A" ]]; then
                echo "wh@test@main|0|main|layoutA|1"
            else
                echo "wh@test@main|0|main|layoutB|1"
            fi
            return 0
            ;;
        show-options)
            echo "$SIG_OPTION"
            return 0
            ;;
        set-option)
            SIG_OPTION="$4"
            return 0
            ;;
        *)
            return 0
            ;;
    esac
}

runtime_rc=0
persistence_save_if_runtime_changed "wh@test@main" || runtime_rc=$?
assert_eq "0" "$runtime_rc" "first changed runtime should save"
assert_eq "1" "$SAVE_CALLS" "first runtime change should trigger save"

runtime_rc=0
persistence_save_if_runtime_changed "wh@test@main" || runtime_rc=$?
assert_eq "2" "$runtime_rc" "same runtime should be deduplicated"
assert_eq "1" "$SAVE_CALLS" "no additional save for unchanged runtime"

RUNTIME_VARIANT="B"
runtime_rc=0
persistence_save_if_runtime_changed "wh@test@main" || runtime_rc=$?
assert_eq "0" "$runtime_rc" "changed runtime should save again"
assert_eq "2" "$SAVE_CALLS" "runtime change should trigger second save"

test_report

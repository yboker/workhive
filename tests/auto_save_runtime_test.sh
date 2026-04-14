#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux_runtime.sh"

test_suite "auto-save hooks dedicated runtime safety"

test_case "tmux_runtime_cli_prefix generates correct dedicated server prefix"
WORKHIVE_TMUX_SERVER_NAME="workhive-myproj-abc123"
cli=$(tmux_runtime_cli_prefix)
assert_eq "tmux -L workhive-myproj-abc123" "$cli" "CLI prefix should target dedicated server"

test_case "auto_save_build_trigger_cmd embeds workhive binary, not raw tmux"
source "$LIB_DIR/infra/persistence/auto_save.sh"
_git_root() { echo "/tmp/test-project"; }
session_get_prefix() { echo "wh@abc123@myproj"; }
resolve_workhive_bin() { echo "/usr/local/bin/workhive"; }
_auto_save_get_debounce_seconds() { echo "3"; }
WORKHIVE_TMUX_SERVER_NAME="workhive-myproj-abc123"
trigger=$(_auto_save_build_trigger_cmd "wh@abc123@myproj@main" "/usr/local/bin/workhive" "timer" 2>/dev/null || echo "")
assert_contains "$trigger" "workhive" "trigger should reference workhive binary"
assert_contains "$trigger" "_internal_save_runtime_debounced" "trigger should call debounced save"
assert_not_contains "$trigger" "tmux -L default" "trigger must not reference default tmux server"

test_case "auto_save_start_timer loop uses tmux_runtime_cli_prefix for has-session check"
source "$LIB_DIR/infra/persistence/auto_save.sh"
WORKHIVE_TMUX_SERVER_NAME="workhive-myproj-abc123"
_auto_save_is_enabled() { return 0; }
_auto_save_get_mode() { echo "hybrid"; }
_auto_save_get_interval_seconds() { echo "60"; }
resolve_workhive_bin() { echo "/usr/local/bin/workhive"; }
_git_root() { echo "/tmp/test-project"; }
session_get_prefix() { echo "wh@abc123@myproj"; }
_auto_save_get_debounce_seconds() { echo "3"; }
_TIMER_RUN_SHELL_CMD=""
tmux() {
    case "$1" in
        show-options) echo "" ;;
        set-option) return 0 ;;
        run-shell)
            shift
            while [[ "$1" == -* ]]; do shift; done
            _TIMER_RUN_SHELL_CMD="$*"
            return 0
            ;;
    esac
    return 0
}
auto_save_start_timer "wh@abc123@myproj@main"
assert_contains "$_TIMER_RUN_SHELL_CMD" "tmux -L" "timer loop should use runtime CLI prefix for has-session"
assert_contains "$_TIMER_RUN_SHELL_CMD" "workhive-myproj-abc123" "timer loop should use dedicated server name"

unset WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true

test_report

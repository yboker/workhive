#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"
source "$LIB_DIR/infra/persistence/process_capture.sh"

test_suite "persistence process capture"

test_case "process_should_restore accepts shell -lc wrappers"
rc=0
process_should_restore "zsh" "zsh -lc codex" "codex" || rc=$?
assert_eq 0 "$rc" "zsh -lc wrapper should be restorable when command is allowed"

test_case "process_should_restore rejects bare shell"
rc=0
process_should_restore "zsh" "zsh" ":all:" || rc=$?
assert_ne 0 "$rc" "bare shell should not be restored"

test_case "process_unwrap_restore_command extracts wrapped startup command"
wrapped='/bin/zsh -lc clear 2>/dev/null || true; printf "\033[2J\033[H\033[3J" || true; trap ":" INT TERM; (trap - INT TERM; eval -- "$1"); exec "$2" -l _ claude --dangerously-skip-permissions /bin/zsh'
unwrapped=$(process_unwrap_restore_command "$wrapped")
assert_eq "claude --dangerously-skip-permissions" "$unwrapped" "unwrap should recover original startup command"

test_case "tmux startup runner keeps visual reset on pane stdout"
runner_script=$(terminal_adapter_build_startup_runner_script)
assert_contains "$runner_script" 'clear 2>/dev/null || true' "clear should only silence stderr"
assert_contains "$runner_script" 'printf "\033[2J\033[H" || true' "ansi clear should write to pane stdout"
assert_not_contains "$runner_script" 'printf "\033[2J\033[H" >/dev/null' "ansi clear must not be redirected away from pane"

test_report

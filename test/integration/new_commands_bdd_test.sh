#!/usr/bin/env bash
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
WORKHIVE_BIN="$ROOT_DIR/bin/workhive"
export WORKHIVE_LANG=en
export WORKHIVE_SKIP_GLOBAL_CHECK=1

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$SCRIPT_DIR/tmux_cleanup_scope.sh"

TEST_DIR="/tmp/workhive-bdd-newcmds-$$"
# shellcheck disable=SC2034
TEST_PREFIX="whbc$$"

mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
git init -q .
git config user.name "BDD Test"
git config user.email "bdd@test.com"
git commit --allow-empty -m "init" -q

cleanup() {
    tmux_cleanup_kill_sessions_in_scope "$TEST_DIR"
    cd /
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "BDD: workhive log"

test_case "workhive init succeeds"
rc=0; $WORKHIVE_BIN init --non-interactive --force >/dev/null 2>&1 || rc=$?
assert_success "$rc"

test_case "workhive log --help shows help"
output=$($WORKHIVE_BIN log --help 2>&1)
assert_contains "$output" "workhive log"

test_case "workhive log shows recent entries"
rc=0; output=$($WORKHIVE_BIN log 2>&1) || rc=$?
assert_success "$rc" "log command exits without crash"

test_case "workhive log --lines 10 works"
rc=0; $WORKHIVE_BIN log --lines 10 >/dev/null 2>&1 || rc=$?
assert_success "$rc"

test_case "workhive log --level info filters"
rc=0; $WORKHIVE_BIN log --level info >/dev/null 2>&1 || rc=$?
assert_success "$rc"

test_suite "BDD: workhive rename"

test_case "rename --help shows usage"
output=$($WORKHIVE_BIN rename --help 2>&1)
assert_contains "$output" "workhive rename"

test_case "rename without args fails"
rc=0; $WORKHIVE_BIN rename 2>/dev/null || rc=$?
assert_failure "$rc"

test_case "rename main branch fails"
main_branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "main")
rc=0; $WORKHIVE_BIN rename "$main_branch" "new-name" 2>/dev/null || rc=$?
assert_failure "$rc"

test_suite "BDD: workhive list --watch"

test_case "list --help includes --watch"
output=$($WORKHIVE_BIN list --help 2>&1)
assert_contains "$output" "--watch"

test_case "list --help includes --interval"
assert_contains "$output" "--interval"

test_suite "BDD: workhive list dynamic width"

test_case "list handles long branch names"
rc=0; output=$($WORKHIVE_BIN list 2>&1) || rc=$?
assert_success "$rc"

# ══════════════════════════════════════════════════
# BDD: pane subcommand --help (each is distinct)
# ══════════════════════════════════════════════════

test_suite "BDD: pane subcommand independent help"

test_case "pane --help shows overview"
output=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane --help 2>&1)
assert_contains "$output" "workhive pane <subcommand>" "overview should show <subcommand>"

test_case "pane open --help is distinct from overview"
overview=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane --help 2>&1)
open_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane open --help 2>&1)
assert_ne "$overview" "$open_help" "open help should differ"
assert_contains "$open_help" "workhive pane open" "should show open-specific usage"

test_case "pane close --help is distinct from overview"
close_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane close --help 2>&1)
assert_ne "$overview" "$close_help" "close help should differ"
assert_contains "$close_help" "workhive pane close"

test_case "pane split --help is distinct from overview"
split_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane split --help 2>&1)
assert_ne "$overview" "$split_help" "split help should differ"
assert_contains "$split_help" "workhive pane split"

test_case "pane send --help is distinct from overview"
send_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane send --help 2>&1)
assert_ne "$overview" "$send_help" "send help should differ"
assert_contains "$send_help" "workhive pane send"

test_case "pane list --help is distinct from overview"
list_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane list --help 2>&1)
assert_ne "$overview" "$list_help" "list help should differ"
assert_contains "$list_help" "workhive pane list"

test_case "pane current --help is distinct from overview"
current_help=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane current --help 2>&1)
assert_ne "$overview" "$current_help" "current help should differ"
assert_contains "$current_help" "workhive pane current"

test_case "pane layout is removed"
layout_output=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane layout --help 2>&1 || true)
assert_contains "$layout_output" "Unknown command"
assert_contains "$layout_output" "workhive pane <subcommand>"

test_case "pane attach is removed"
attach_output=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane attach --help 2>&1 || true)
assert_contains "$attach_output" "Unknown command"
assert_contains "$attach_output" "workhive pane <subcommand>"

# ══════════════════════════════════════════════════
# BDD: pane send / list outside tmux
# ══════════════════════════════════════════════════

test_suite "BDD: pane send/list outside tmux"

test_case "pane send outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane send 1 "test" 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane list outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane list 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane list --format json outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane list --format json 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane current outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane current 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane current --format json outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane current --format json 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane close --help suggests rm for last pane"
output=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane close --help 2>&1)
assert_contains "$output" "use 'workhive rm' instead"
assert_not_contains "$output" "workhive destroy"

test_case "pane close -f outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane close -f 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

test_case "pane close --force outside tmux shows error"
unset TMUX
rc=0; output=$($WORKHIVE_BIN pane close --force 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "tmux" "should mention tmux requirement"

# ══════════════════════════════════════════════════
# BDD: pane open --cmd flag
# ══════════════════════════════════════════════════

test_suite "BDD: pane open --cmd flag"

test_case "pane open --cmd without value shows error"
rc=0; output=$(env WORKHIVE_LANG=en $WORKHIVE_BIN pane open dev01 --cmd 2>&1) || rc=$?
assert_failure "$rc"
assert_contains "$output" "requires a value" "should show missing cmd error"

test_case "pane open -f flag accepted (does not show unknown option)"
output=$($WORKHIVE_BIN pane open --help 2>&1)
assert_contains "$output" "--force" "help should document --force"

# ══════════════════════════════════════════════════
# BDD: completions include new subcommands
# ══════════════════════════════════════════════════

test_suite "BDD: completions include new pane subcommands"

test_case "zsh completions include send subcommand"
output=$($WORKHIVE_BIN _internal_completions zsh 2>&1)
assert_contains "$output" "send" "should include send in completions"

test_case "zsh completions include list subcommand"
assert_contains "$output" "list" "should include list in completions"

test_case "zsh completions include current subcommand"
assert_contains "$output" "current" "should include current in completions"

test_case "zsh completions include pane close --id and pane list --format"
assert_contains "$output" "--id" "should include pane close --id in zsh completions"
assert_contains "$output" "--format" "should include pane list --format in zsh completions"
assert_contains "$output" "json" "should include json format in zsh completions"

test_case "bash completions include send subcommand"
output=$($WORKHIVE_BIN _internal_completions bash 2>&1)
assert_contains "$output" "send" "should include send in bash completions"

test_case "bash completions include list subcommand"
assert_contains "$output" "list" "should include list in bash completions"

test_case "bash completions include current subcommand"
assert_contains "$output" "current" "should include current in bash completions"

test_case "bash completions include pane close --id and pane list --format"
assert_contains "$output" "--id --force -f --help -h" "should include pane close --id in bash completions"
assert_contains "$output" "--format --help -h" "should include pane list --format in bash completions"

test_case "fish completions include send subcommand"
output=$($WORKHIVE_BIN _internal_completions fish 2>&1)
assert_contains "$output" "send" "should include send in fish completions"

test_case "fish completions include list subcommand"
assert_contains "$output" "list" "should include list in fish completions"

test_case "fish completions include current subcommand"
assert_contains "$output" "current" "should include current in fish completions"

test_case "fish completions include pane close --id and pane list --format"
assert_contains "$output" "-l id" "should include pane close id in fish completions"
assert_contains "$output" "-l format" "should include pane list format in fish completions"
assert_contains "$output" "table json" "should include json format in fish completions"

test_report

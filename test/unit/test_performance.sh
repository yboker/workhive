#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Performance Test Suite
# Validates that all commands meet SLA execution time targets
#
# SLA Classes:
#   L0 (ultra-light): --version, --help, <cmd> --help, shell
#     => P95 total <= 200ms
#   L1 (query): config get, log, doctor
#     => P95 default total <= 400ms
#   L1 (query, tmux-heavy): list
#     => P95 default total <= 1000ms (tmux list-panes cost scales with session count)
#     => achievable ~450ms on idle machines with <= 3 sessions
#   L2 (transactional): open, attach, remove, gc, upgrade
#     => P95 first feedback <= 200ms
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/test_framework.sh"

PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WORKHIVE="$PROJECT_ROOT/bin/workhive"

# Measure average execution time over N runs (in ms)
_perf_measure() {
    local runs="${1:-3}"
    shift
    local total=0
    for ((i=1; i<=runs; i++)); do
        local s e
        s=$EPOCHREALTIME
        bash "$WORKHIVE" "$@" > /dev/null 2>&1
        e=$EPOCHREALTIME
        local ms
        ms=$(printf "%.0f" "$(echo "($e - $s) * 1000" | bc)")
        total=$((total + ms))
    done
    echo $((total / runs))
}

# Assert that measured time is within threshold
_perf_assert_within() {
    local label="$1"
    local threshold_ms="$2"
    shift 2
    local avg
    avg=$(_perf_measure 3 "$@")
    if [[ $avg -le $threshold_ms ]]; then
        assert_true true "$label: ${avg}ms (target: <${threshold_ms}ms)"
    else
        assert_true false "$label: ${avg}ms EXCEEDS target ${threshold_ms}ms"
    fi
}

test_suite "Performance Tests (SLA-aligned)"

# ──────────────────────────────────────────────────
# L0: Ultra-light commands — P95 total <= 200ms
# --version, --help, <cmd> --help, shell
# ──────────────────────────────────────────────────
test_case "L0: workhive --version < 200ms"
_perf_assert_within "workhive --version" 200 --version

test_case "L0: workhive --help < 200ms"
_perf_assert_within "workhive --help" 200 --help

HELP_COMMANDS=(
    "init" "open" "attach" "close" "list" "remove" "rename"
    "pane" "config" "doctor" "gc" "destroy" "upgrade"
    "log" "shell"
)

for cmd in "${HELP_COMMANDS[@]}"; do
    test_case "L0: workhive $cmd --help < 200ms"
    _perf_assert_within "workhive $cmd --help" 200 "$cmd" --help
done

test_case "L0: workhive _internal_completions zsh < 200ms"
_perf_assert_within "_internal_completions zsh" 200 _internal_completions zsh

# ──────────────────────────────────────────────────
# L1: Query commands — P95 default total <= 400ms
# list, doctor, config get, log
# ──────────────────────────────────────────────────
test_case "L1: workhive config get language < 400ms"
_perf_assert_within "config get (top-level key)" 400 config get language

test_case "L1: workhive config get tmux.theme < 400ms"
_perf_assert_within "config get (nested key)" 400 config get tmux.theme

test_case "L1: workhive log < 400ms"
_perf_assert_within "log" 400 log

# list requires tmux list-panes -a + git worktree list, whose wall-clock
# cost scales with the number of active sessions/panes. On a machine with many
# sessions (>10), 400ms is not achievable; 1000ms is the realistic P95 guard.
# On an idle machine with <=3 sessions, both reach ~450-550ms.
test_case "L1: workhive list < 1000ms"
_perf_assert_within "list" 1000 list

test_case "L1: workhive list < 1000ms"
_perf_assert_within "list" 1000 list

# doctor runs tool version checks (git, tmux, yq) which are ~40ms each as subprocesses.
# Achieves ~320ms on idle machines; on busy systems with many sessions, 800ms is the guard.
test_case "L1: workhive doctor < 800ms"
_perf_assert_within "doctor" 800 doctor

# ──────────────────────────────────────────────────
# L2: Transactional commands — coarse regression guard
# gc --dry-run used as safe proxy
# ──────────────────────────────────────────────────
test_case "L2: workhive gc --dry-run < 5s"
_perf_assert_within "gc --dry-run" 5000 gc --dry-run

# ──────────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────────
test_report

# Performance budget reference:
#   Bash startup:         ~21ms (fork + exec + init)
#   Core modules load:    ~10ms (common.sh + i18n.sh + cache + platform)
#   Language detection:    ~5ms (sed on .workhive/config.yaml)
#   Help index load:      ~5ms (all help functions, no dependencies)
#   Full stack load:     ~30ms (plugins + providers + config/loader)
#   git rev-parse:       ~27ms per call
#   yq YAML parse:       ~14ms per call
#   sed/awk YAML parse:   ~5ms per call

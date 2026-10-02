#!/usr/bin/env bash
#
# Reproducible micro-benchmarks for the workhive CLI, backed by hyperfine.
#
# What it measures (hermetic HOME, real tmux, real git):
#
#   1. workhive --version                          binary startup baseline
#   2. workhive workspace list --view json          project query
#   3. workhive session list --view json            global query
#   4. workhive workspace open <ws> --no-attach     cold workspace creation
#      (each round removes the workspace first, so every run provisions a
#      fresh worktree and tmux session)
#   5. workhive pane list --view json               in-session pane query
#      (runs with TMUX/TMUX_PANE/TMUX_SESSION injected from a live session;
#      skipped with a notice when the environment injection cannot be
#      established on this host)
#
# Requirements: hyperfine, tmux, git, and a release build of workhive
# (built automatically when missing). Everything runs under a temporary
# HOME; the tmux server and temp directory are cleaned up on exit.
#
# Usage:
#   scripts/bench.sh                # all benchmarks
#   WORKHIVE_BIN=/path/to/workhive scripts/bench.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

for tool in hyperfine tmux git; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
        echo "FAIL: '${tool}' is required. Install it first (e.g. brew install ${tool})." >&2
        exit 2
    fi
done

BIN="${WORKHIVE_BIN:-${ROOT_DIR}/target/release/workhive}"
if [ ! -x "${BIN}" ]; then
    echo "release binary not found; building..." >&2
    cargo build --release
fi
if [ ! -x "${BIN}" ]; then
    echo "FAIL: workhive binary not found at ${BIN}." >&2
    exit 2
fi

echo "workhive: ${BIN} ($("${BIN}" --version))"
echo "tmux:     $(tmux -V)"
echo "host:     $(uname -sm)"
echo

# ── Hermetic environment ────────────────────────────────────────────────
BENCH_HOME="$(mktemp -d)"
PROJECT_DIR="${BENCH_HOME}/bench-project"
BENCH_WS="bench-ws"
BENCH_TMUX_TMPDIR="${BENCH_HOME}/tmux-sockets"
mkdir -p "${BENCH_TMUX_TMPDIR}"
export TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}"

cleanup() {
    TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}" tmux kill-server 2>/dev/null || true
    rm -rf "${BENCH_HOME}"
}
trap cleanup EXIT

wh() {
    HOME="${BENCH_HOME}" XDG_CONFIG_HOME="${BENCH_HOME}/.config" TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}" "${BIN}" "$@"
}

# One-time setup mirroring the first-run flow: global config, trusted
# root, git repo, project init.
wh init --global --force >/dev/null
wh project roots add "${BENCH_HOME}" --force >/dev/null
mkdir -p "${PROJECT_DIR}"
git -C "${PROJECT_DIR}" init --initial-branch=main --quiet
git -C "${PROJECT_DIR}" commit --allow-empty -m init --quiet
(cd "${PROJECT_DIR}" && wh init --force >/dev/null)

HYPERFINE_COMMON=(--warmup 2 --min-runs 10)

# hyperfine receives command STRINGS that it re-parses through a shell, so
# every interpolated path must be shell-quoted here or paths containing
# spaces would break. `printf %q` produces a token that round-trips
# through that inner shell unchanged.
Q_BIN="$(printf '%q' "${BIN}")"
Q_HOME="$(printf '%q' "${BENCH_HOME}")"
Q_PROJ="$(printf '%q' "${PROJECT_DIR}")"
Q_TMUX_TMPDIR="$(printf '%q' "${BENCH_TMUX_TMPDIR}")"
ENV_PREFIX="HOME=${Q_HOME} XDG_CONFIG_HOME=${Q_HOME}/.config TMUX_TMPDIR=${Q_TMUX_TMPDIR}"

echo "── 1/5 binary startup (--version) ──"
hyperfine "${HYPERFINE_COMMON[@]}" \
    "${ENV_PREFIX} ${Q_BIN} --version"

echo "── 2/5 workspace list --view json ──"
hyperfine "${HYPERFINE_COMMON[@]}" \
    "cd ${Q_PROJ} && ${ENV_PREFIX} ${Q_BIN} workspace list --view json"

echo "── 3/5 session list --view json ──"
hyperfine "${HYPERFINE_COMMON[@]}" \
    "${ENV_PREFIX} ${Q_BIN} session list --view json"

echo "── 4/5 workspace open --no-attach (cold: fresh worktree + tmux session per run) ──"
hyperfine --warmup 1 --min-runs 5 \
    --prepare "cd ${Q_PROJ} && ${ENV_PREFIX} ${Q_BIN} workspace remove ${BENCH_WS} -f >/dev/null 2>&1 || true" \
    "cd ${Q_PROJ} && ${ENV_PREFIX} ${Q_BIN} workspace open ${BENCH_WS} --no-attach"

echo "── 5/5 pane list --view json (inside a live multi-pane session) ──"
# Open a workspace once and keep its session alive, then inject the tmux
# environment a shell inside that session would see.
(cd "${PROJECT_DIR}" && wh workspace open "${BENCH_WS}" --no-attach >/dev/null 2>&1 || true)
SESSION_JSON="$(wh session list --view json)"
SESSION="$(printf '%s' "${SESSION_JSON}" | python3 -c 'import json,sys; rows=json.load(sys.stdin)["sessions"]; print(rows[0]["session"] if rows else "")')"
if [ -z "${SESSION}" ]; then
    echo "SKIP: no live session found; pane list benchmark not run." >&2
else
    SOCKET="$(TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}" tmux display-message -p -F '#{socket_path}' 2>/dev/null || true)"
    PANE_ID="$(TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}" tmux list-panes -t "${SESSION}" -F '#{pane_id}' 2>/dev/null | head -1 || true)"
    PANE_COUNT="$(TMUX_TMPDIR="${BENCH_TMUX_TMPDIR}" tmux list-panes -t "${SESSION}" 2>/dev/null | wc -l | tr -d ' ')"
    if [ -z "${SOCKET}" ] || [ -z "${PANE_ID}" ]; then
        echo "SKIP: could not resolve tmux socket/pane for in-session injection." >&2
    else
        echo "session: ${SESSION} (${PANE_COUNT} panes)"
        Q_TMUX="$(printf '%q' "${SOCKET},0,0")"
        Q_PANE="$(printf '%q' "${PANE_ID}")"
        Q_SESSION="$(printf '%q' "${SESSION}")"
        hyperfine "${HYPERFINE_COMMON[@]}" \
            "cd ${Q_PROJ} && ${ENV_PREFIX} TMUX=${Q_TMUX} TMUX_PANE=${Q_PANE} TMUX_SESSION=${Q_SESSION} ${Q_BIN} pane list --view json"
    fi
fi

echo
echo "done. Environment was hermetic (temp HOME); tmux servers cleaned up on exit."

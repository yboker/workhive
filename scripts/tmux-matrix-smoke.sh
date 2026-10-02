#!/usr/bin/env bash
#
# Real-tmux smoke matrix: run the gated (ignored) real-tmux lifecycle
# tests against one or more tmux binaries and report pass/fail per
# version.
#
# Usage:
#   scripts/tmux-matrix-smoke.sh                    # system tmux only
#   scripts/tmux-matrix-smoke.sh /opt/tmux-3.3a/bin/tmux /opt/tmux-3.5/bin/tmux
#
# Each argument must be an executable tmux binary. For every binary the
# script prepends its directory to PATH and runs a fixed smoke subset of
# the ignored integration tests (real tmux lifecycle: open, list,
# reuse, destroy). Any failure exits non-zero and names the tmux
# version that failed.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT_DIR}"

# Fixed smoke subset: full open/list/attach/destroy lifecycle, session
# reuse, and the agent dispatch/query/remove closed loop. Small enough
# to run per matrix entry, broad enough to catch version-specific tmux
# breakage in create/list/kill paths.
SMOKE_FILTERS=(
    e2e_open_list_attach_destroy_lifecycle
    e2e_open_twice_reuses_session
    e2e_agent_dispatch_query_remove_closed_loop
)

if [ "$#" -eq 0 ]; then
    if ! command -v tmux >/dev/null 2>&1; then
        echo "FAIL: no tmux on PATH and no binaries passed as arguments." >&2
        exit 2
    fi
    set -- "$(command -v tmux)"
fi

failures=()

for tmux_bin in "$@"; do
    if [ ! -x "${tmux_bin}" ]; then
        echo "FAIL: '${tmux_bin}' is not an executable tmux binary." >&2
        exit 2
    fi
    tmux_dir="$(cd "$(dirname "${tmux_bin}")" && pwd)"
    version="$("${tmux_bin}" -V)"
    echo "═══ smoke: ${version} (${tmux_bin}) ═══"

    ok=1
    for filter in "${SMOKE_FILTERS[@]}"; do
        echo "+ cargo test -p workhive-cli --test cli_integration -- --ignored --exact ${filter}"
        if ! PATH="${tmux_dir}:${PATH}" cargo test -p workhive-cli \
            --test cli_integration -- --ignored --exact "${filter}"; then
            echo "FAIL: ${filter} failed under ${version}." >&2
            ok=0
        fi
    done

    if [ "${ok}" -eq 1 ]; then
        echo "PASS: ${version}"
    else
        failures+=("${version}")
    fi
done

if [ "${#failures[@]}" -gt 0 ]; then
    echo "FAIL: smoke failed for: ${failures[*]}" >&2
    exit 1
fi
echo "matrix smoke passed for all ${#} tmux binar$( [ "$#" -eq 1 ] && echo y || echo ies )."

#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/persistence/save.sh"

test_suite "persistence save fail-open"

tmux() {
    case "$1" in
        list-panes|list-windows)
            return 1
            ;;
    esac
    return 0
}

snapshot_format_pane() { echo "pane"; }
snapshot_format_window() { echo "window"; }
process_capture_full_command() { return 1; }

test_case "_save_dump_panes returns success when tmux list-panes is unavailable"
rc=0
output=$(_save_dump_panes "wh" 2>/dev/null) || rc=$?
assert_eq 0 "$rc" "pane dump should fail-open"
assert_eq "" "$output" "pane dump should emit nothing on unavailable tmux list"

test_case "_save_dump_windows returns success when tmux list-windows is unavailable"
rc=0
output=$(_save_dump_windows "wh" 2>/dev/null) || rc=$?
assert_eq 0 "$rc" "window dump should fail-open"
assert_eq "" "$output" "window dump should emit nothing on unavailable tmux list"

test_report

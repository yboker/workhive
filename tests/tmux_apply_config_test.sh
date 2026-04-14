#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh"

test_suite "tmux apply config"

_TEST_APPLY_ORDER=""
_tmux_cleanup_legacy_statusbar_watcher() { _TEST_APPLY_ORDER+="cleanup>"; }
_tmux_apply_session_protection() { _TEST_APPLY_ORDER+="protect>"; }
_tmux_apply_diagnostic_hooks() { _TEST_APPLY_ORDER+="diag>"; }
_tmux_apply_base_options() { _TEST_APPLY_ORDER+="base>"; }
_tmux_apply_true_color() { _TEST_APPLY_ORDER+="color>"; }
_tmux_detect_version() { _TMUX_MAJOR="3"; _TMUX_MINOR="3"; return 0; }
_tmux_apply_theme() { _TEST_APPLY_ORDER+="theme>"; }
_tmux_apply_popup_keybindings() { _TEST_APPLY_ORDER+="popup>"; }
tmux() { return 0; }

test_case "terminal_adapter_apply_config cleans legacy hooks before installing diagnostics"
_TEST_APPLY_ORDER=""
terminal_adapter_apply_config "wh@test"
assert_contains "$_TEST_APPLY_ORDER" "cleanup>protect>diag>base>color>theme>popup>" "cleanup must run before diagnostic hooks"

test_report

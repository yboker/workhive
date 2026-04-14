#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/utils/i18n.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/i18n.sh"

test_suite "lib/utils/i18n.sh"

# ──────────────────────────────────────────────────
# _i18n function: Chinese translations
# ──────────────────────────────────────────────────

test_case "_i18n returns Chinese text when WORKHIVE_LANG=zh"
WORKHIVE_LANG="zh"
result=$(_i18n INIT_WIZARD)
assert_eq "workhive 初始化向导" "$result"

test_case "_i18n returns Chinese layout text"
WORKHIVE_LANG="zh"
result=$(_i18n LAYOUT_LEFT)
assert_contains "$result" "left-split"

test_case "_i18n returns Chinese custom input"
WORKHIVE_LANG="zh"
result=$(_i18n CUSTOM_INPUT)
assert_eq "自定义输入..." "$result"

# ──────────────────────────────────────────────────
# _i18n function: English translations
# ──────────────────────────────────────────────────

test_case "_i18n returns English text when WORKHIVE_LANG=en"
WORKHIVE_LANG="en"
result=$(_i18n INIT_WIZARD)
assert_eq "workhive init wizard" "$result"

test_case "_i18n returns English layout text"
WORKHIVE_LANG="en"
result=$(_i18n LAYOUT_LEFT)
assert_contains "$result" "left-split"

test_case "_i18n returns English custom input"
WORKHIVE_LANG="en"
result=$(_i18n CUSTOM_INPUT)
assert_eq "Custom input..." "$result"

# ──────────────────────────────────────────────────
# _i18n function: fallback behavior
# ──────────────────────────────────────────────────

test_case "_i18n falls back to English for unknown language"
WORKHIVE_LANG="fr"
result=$(_i18n INIT_WIZARD)
assert_eq "workhive init wizard" "$result"

test_case "_i18n returns key name for unknown key"
WORKHIVE_LANG="en"
result=$(_i18n NONEXISTENT_KEY_XYZ)
assert_eq "NONEXISTENT_KEY_XYZ" "$result"

test_case "_i18n falls back to English when lang is empty"
WORKHIVE_LANG=""
result=$(_i18n INIT_WIZARD)
assert_eq "workhive init wizard" "$result"

# ──────────────────────────────────────────────────
# _i18n function: all theme translation keys
# ──────────────────────────────────────────────────

test_case "_i18n LAYOUT_HALF exists in both languages"
WORKHIVE_LANG="en"
result_en=$(_i18n LAYOUT_HALF)
WORKHIVE_LANG="zh"
result_zh=$(_i18n LAYOUT_HALF)
assert_contains "$result_en" "half-split"
assert_contains "$result_zh" "half-split"

test_case "_i18n SELECT_THEME exists in both languages"
WORKHIVE_LANG="en"
result_en=$(_i18n SELECT_THEME)
WORKHIVE_LANG="zh"
result_zh=$(_i18n SELECT_THEME)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys: ERR_UNKNOWN_OPT, ERR_NO_POSITIONAL, MSG_CANCELLED
# ──────────────────────────────────────────────────

test_case "_i18n ERR_UNKNOWN_OPT exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_UNKNOWN_OPT)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_UNKNOWN_OPT)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n ERR_NO_POSITIONAL exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_NO_POSITIONAL)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_NO_POSITIONAL)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n MSG_CANCELLED exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n MSG_CANCELLED)
WORKHIVE_LANG="zh"
result_zh=$(_i18n MSG_CANCELLED)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys: ERR_NOT_INITIALIZED, ERR_UNKNOWN_CMD
# ──────────────────────────────────────────────────

test_case "_i18n ERR_NOT_INITIALIZED exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_NOT_INITIALIZED)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_NOT_INITIALIZED)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n ERR_UNKNOWN_CMD exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_UNKNOWN_CMD)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_UNKNOWN_CMD)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys: HELP_DESC, HELP_USAGE, HELP_COMMANDS
# ──────────────────────────────────────────────────

test_case "_i18n HELP_DESC exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n HELP_DESC)
WORKHIVE_LANG="zh"
result_zh=$(_i18n HELP_DESC)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n HELP_USAGE exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n HELP_USAGE)
WORKHIVE_LANG="zh"
result_zh=$(_i18n HELP_USAGE)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n HELP_COMMANDS exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n HELP_COMMANDS)
WORKHIVE_LANG="zh"
result_zh=$(_i18n HELP_COMMANDS)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys: ERR_BRANCH_EMPTY, ERR_NOT_GIT_REPO
# ──────────────────────────────────────────────────

test_case "_i18n ERR_BRANCH_EMPTY exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_BRANCH_EMPTY)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_BRANCH_EMPTY)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n ERR_NOT_GIT_REPO exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n ERR_NOT_GIT_REPO)
WORKHIVE_LANG="zh"
result_zh=$(_i18n ERR_NOT_GIT_REPO)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys: TITLE_DESTROY, MSG_DESTROY_COMPLETE
# ──────────────────────────────────────────────────

test_case "_i18n TITLE_DESTROY exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n TITLE_DESTROY)
WORKHIVE_LANG="zh"
result_zh=$(_i18n TITLE_DESTROY)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n MSG_DESTROY_COMPLETE exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n MSG_DESTROY_COMPLETE)
WORKHIVE_LANG="zh"
result_zh=$(_i18n MSG_DESTROY_COMPLETE)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# i18n_ask_language: defaults to English in non-TTY environment
# ──────────────────────────────────────────────────

test_case "i18n_ask_language defaults to en in non-TTY"
# Test in subshell (pipe creates subshell, variables don't propagate back)
result=$(WORKHIVE_LANG="" bash -c '
    source "'"$LIB_DIR"'/utils/i18n.sh"
    i18n_ask_language 2>/dev/null
    echo "$WORKHIVE_LANG"
')
assert_eq "en" "$result"

test_case "i18n_ask_language preserves existing WORKHIVE_LANG"
WORKHIVE_LANG="zh"
i18n_ask_language 2>/dev/null || true
assert_eq "zh" "$WORKHIVE_LANG"

# ──────────────────────────────────────────────────
# yazi and new tool i18n keys
# ──────────────────────────────────────────────────

test_case "_i18n YAZI_DESC exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n YAZI_DESC)
WORKHIVE_LANG="zh"
result_zh=$(_i18n YAZI_DESC)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

test_case "_i18n YAZI_HINT exists in en and zh"
WORKHIVE_LANG="en"
result_en=$(_i18n YAZI_HINT)
WORKHIVE_LANG="zh"
result_zh=$(_i18n YAZI_HINT)
assert_not_empty "$result_en"
assert_not_empty "$result_zh"

# ──────────────────────────────────────────────────
# New i18n keys from Steps 1-8
# ──────────────────────────────────────────────────

test_case "_i18n CONFIRM_CLOSE_PANE exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n CONFIRM_CLOSE_PANE)
assert_not_contains "$result" "CONFIRM_CLOSE_PANE" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n CONFIRM_CLOSE_PANE)
assert_not_contains "$result" "CONFIRM_CLOSE_PANE" "zh should have translation"

test_case "_i18n CONFIRM_KILL_SESSION exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n CONFIRM_KILL_SESSION)
assert_not_contains "$result" "CONFIRM_KILL_SESSION" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n CONFIRM_KILL_SESSION)
assert_not_contains "$result" "CONFIRM_KILL_SESSION" "zh should have translation"

test_case "_i18n MSG_PANE_CMDS_RESTORED exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n MSG_PANE_CMDS_RESTORED)
assert_not_contains "$result" "MSG_PANE_CMDS_RESTORED" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n MSG_PANE_CMDS_RESTORED)
assert_not_contains "$result" "MSG_PANE_CMDS_RESTORED" "zh should have translation"

test_case "_i18n PLUGIN_LOADING exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n PLUGIN_LOADING)
assert_not_contains "$result" "PLUGIN_LOADING" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n PLUGIN_LOADING)
assert_not_contains "$result" "PLUGIN_LOADING" "zh should have translation"

test_case "_i18n ERR_ADAPTER_UNSUPPORTED exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n ERR_ADAPTER_UNSUPPORTED)
assert_not_contains "$result" "ERR_ADAPTER_UNSUPPORTED" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n ERR_ADAPTER_UNSUPPORTED)
assert_not_contains "$result" "ERR_ADAPTER_UNSUPPORTED" "zh should have translation"

test_case "_i18n PLUGIN_NONE_LOADED exists in en and zh"
WORKHIVE_LANG=en; result=$(_i18n PLUGIN_NONE_LOADED)
assert_not_contains "$result" "PLUGIN_NONE_LOADED" "en should have translation"
WORKHIVE_LANG=zh; result=$(_i18n PLUGIN_NONE_LOADED)
assert_not_contains "$result" "PLUGIN_NONE_LOADED" "zh should have translation"

test_report

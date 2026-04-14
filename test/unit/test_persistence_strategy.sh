#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/infra/persistence/process_capture.sh"

test_suite "Persistence — Strategy-aware restore"

test_case "Strategy classification for shell commands"
result=$(process_classify_restore_strategy "bash" "")
assert_eq "shell" "$result" "Bash should be classified as shell"

result=$(process_classify_restore_strategy "zsh" "")
assert_eq "shell" "$result" "Zsh should be classified as shell"

test_case "Strategy classification for TUI commands"
result=$(process_classify_restore_strategy "lazygit" "")
assert_eq "tui" "$result" "Lazygit should be classified as TUI"

result=$(process_classify_restore_strategy "htop" "")
assert_eq "tui" "$result" "Htop should be classified as TUI"

test_case "Strategy classification for AI commands"
result=$(process_classify_restore_strategy "claude" "")
assert_eq "ai" "$result" "Claude should be classified as AI"

result=$(process_classify_restore_strategy "opencode" "")
assert_eq "ai" "$result" "Opencode should be classified as AI"

test_case "Strategy classification for generic commands"
result=$(process_classify_restore_strategy "node" "node index.js")
assert_eq "generic" "$result" "Node should be classified as generic"

result=$(process_classify_restore_strategy "python" "python script.py")
assert_eq "generic" "$result" "Python should be classified as generic"

test_report

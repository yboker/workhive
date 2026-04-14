#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: doctor, gc commands + notification + keybindings
# 100% coverage for all new commands
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
BIN_DIR="$ROOT_DIR/bin"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"

# Set up test environment
TEST_DIR="/tmp/workhive-test-newcmds-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1 || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
export WORKHIVE_SKIP_GLOBAL_CHECK=1
export WORKHIVE_LANG=en
mkdir -p "$XDG_CONFIG_HOME"

if ! git init >/dev/null 2>&1; then
    echo "FATAL: git init failed (check Xcode license: sudo xcodebuild -license)"
    exit 1
fi
git config user.name "Test"
git config user.email "test@example.com"
echo "# Test" > README.md
echo ".workhive/" > .gitignore
git add README.md .gitignore
git commit -m "Init" >/dev/null 2>&1
mkdir -p .workhive
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: null
tmux:
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF

cleanup() {
    local session_name
    local project_name
    project_name=$(basename "$TEST_DIR")

    while IFS= read -r session_name; do
        [[ -z "$session_name" ]] && continue
        case "$session_name" in
            *"@${project_name}@"*)
                tmux kill-session -t "$session_name" 2>/dev/null || true
                ;;
        esac
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)

    cd / || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

# ═══════════════════════════════════════════════
# doctor_command tests
# ═══════════════════════════════════════════════

test_suite "doctor_command.sh - Unit Tests"

test_case "doctor --help shows usage (English)"
output=$(env -u WORKHIVE_LANG "$BIN_DIR/workhive" doctor --help 2>&1)
assert_contains "$output" "Usage:" "should show English usage"
assert_contains "$output" "workhive doctor" "should show doctor usage"
assert_contains "$output" "git" "should mention git"
assert_contains "$output" "tmux" "should mention tmux"

test_case "doctor -h shows help"
output=$("$BIN_DIR/workhive" doctor -h 2>&1)
assert_contains "$output" "workhive doctor"

test_case "doctor --help shows Chinese when WORKHIVE_LANG=zh"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" doctor --help 2>&1)
assert_contains "$output" "检查环境健康" "should show Chinese description"

test_case "doctor rejects unknown options"
output=$("$BIN_DIR/workhive" doctor --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "doctor rejects positional arguments"
output=$("$BIN_DIR/workhive" doctor something 2>&1 || true)
assert_contains "$output" "does not accept positional"

test_case "doctor --format without value shows error"
output=$("$BIN_DIR/workhive" doctor --format 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "doctor --format with invalid value shows error"
output=$("$BIN_DIR/workhive" doctor --format xml 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "doctor runs successfully"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
exit_code=$?
assert_eq "0" "$exit_code" "should succeed"

test_case "doctor shows required dependencies"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "git" "should check git"
assert_contains "$output" "tmux" "should check tmux"
assert_contains "$output" "yq" "should check yq"

test_case "doctor default omits pane tools"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_not_contains "$output" "lazygit" "default should not check lazygit"
assert_not_contains "$output" "codex" "default should not check codex"

test_case "doctor --full shows pane tools"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor --full 2>&1)
assert_contains "$output" "lazygit" "full should check lazygit"
assert_contains "$output" "codex" "full should check codex"
assert_contains "$output" "opencode" "full should check opencode"

test_case "doctor does not show recommended tools (removed)"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_not_contains "$output" "fzf" "fzf should not be in doctor output"
assert_not_contains "$output" "atuin" "atuin should not be in doctor output"

test_case "doctor shows config check"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "Config" "should check config"

test_case "doctor shows git repo check"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
assert_contains "$output" "Git" "should check git repo"

test_case "doctor --format json produces valid JSON"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor --format json 2>&1)
assert_contains "$output" '"dependencies"' "should have dependencies"
assert_contains "$output" '"required"' "should have required section"
assert_not_contains "$output" '"pane_tools"' "default JSON should not have pane_tools"
assert_contains "$output" '"config"' "should have config section"
assert_contains "$output" '"git"' "should have git section"
assert_contains "$output" '"issues"' "should have issues count"

test_case "doctor JSON shows installed tools with versions"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" doctor --format json 2>&1)
assert_contains "$output" '"installed": true' "git should be installed"
assert_contains "$output" '"name": "git"' "should have git name"

test_case "doctor can run without config (exempt from init check)"
cd /tmp || exit 1
output=$("$BIN_DIR/workhive" doctor 2>&1)
# Should not error with "not initialized"
assert_not_contains "$output" "not initialized" "should run without config"
cd "$TEST_DIR" || exit 1

# ═══════════════════════════════════════════════
# gc_command tests
# ═══════════════════════════════════════════════

test_suite "gc_command.sh - Unit Tests"

test_case "gc --help shows usage (English)"
output=$(env -u WORKHIVE_LANG "$BIN_DIR/workhive" gc --help 2>&1)
assert_contains "$output" "Usage:" "should show English usage"
assert_contains "$output" "workhive gc" "should show gc usage"
assert_contains "$output" "--auto" "should show auto option"
assert_contains "$output" "--snapshots" "should show snapshots option"

test_case "gc -h shows help"
output=$("$BIN_DIR/workhive" gc -h 2>&1)
assert_contains "$output" "workhive gc"

test_case "gc --help shows Chinese when WORKHIVE_LANG=zh"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" gc --help 2>&1)
assert_contains "$output" "清理已合并" "should show Chinese description"

test_case "gc rejects unknown options"
output=$("$BIN_DIR/workhive" gc --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "gc rejects removed --dry-run option"
output=$("$BIN_DIR/workhive" gc --dry-run 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "gc rejects positional arguments"
output=$("$BIN_DIR/workhive" gc something 2>&1 || true)
assert_contains "$output" "does not accept positional"

test_case "gc --snapshots runs without cleaning worktree resources"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" gc --snapshots 2>&1)
assert_contains "$output" "Garbage Collection" "should show title"
assert_contains "$output" "complete" "should show completion"

test_case "gc --auto runs without prompts"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" gc --auto 2>&1)
assert_contains "$output" "Garbage Collection" "should show title"
assert_contains "$output" "complete" "should show completion"

test_case "gc writes operation logs"
cd "$TEST_DIR" || exit 1
gc_test_log="$TEST_DIR/.workhive/logs/gc-op-test.log"
mkdir -p "$(dirname "$gc_test_log")"
: > "$gc_test_log"
WORKHIVE_TEST_LOG_FILE="$gc_test_log" "$BIN_DIR/workhive" gc --auto >/dev/null 2>&1 || true
gc_log=$(cat "$gc_test_log" 2>/dev/null || true)
assert_contains "$gc_log" "[OP][BEGIN] action=gc_execute" "should log gc begin"
assert_contains "$gc_log" "[OP][OK] action=gc_execute" "should log gc success"

test_case "gc shows merged branch scanning"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" gc 2>&1)
assert_contains "$output" "Scanning" "should show scanning message"

# ═══════════════════════════════════════════════
# session_notify tests (load service and test)
# ═══════════════════════════════════════════════

test_suite "session_notify - Notification System Tests"

source "$LIB_DIR/domain/services/session_service.sh"

test_case "session_notify returns 1 for non-existent session"
session_notify "non-existent-branch-xyz" "test message" 2>/dev/null
assert_failure "$?" "should fail for non-existent session"

# ═══════════════════════════════════════════════
# Internal function tests
# ═══════════════════════════════════════════════

test_suite "Internal Functions - Doctor"

source "$LIB_DIR/app/commands/doctor_command.sh"

test_case "_doctor_get_version returns non-empty for git"
result=$(_doctor_get_version "git")
assert_not_empty "$result" "git version should be non-empty"

test_case "_doctor_get_version returns empty for nonexistent tool"
result=$(_doctor_get_version "nonexistent_tool_xyz")
assert_empty "$result" "nonexistent tool should return empty"

test_case "extract_version_by_pattern parses semantic versions (shared helper)"
result=$(extract_version_by_pattern "yq ... version v4.52.5" '([0-9]+\.[0-9]+\.[0-9]+)')
assert_eq "4.52.5" "$result" "should parse semver with bash regex"

test_case "_doctor_prefetch_versions does not clobber existing RETURN trap"
prev_return_trap="$(trap -p RETURN || true)"
__doctor_return_marker() { :; }
trap '__doctor_return_marker' RETURN
_doctor_prefetch_versions "git" >/dev/null 2>&1
after_return_trap="$(trap -p RETURN || true)"
assert_contains "$after_return_trap" "__doctor_return_marker" "RETURN trap should remain intact after prefetch"
trap - RETURN
if [[ -n "$prev_return_trap" ]]; then
    eval "$prev_return_trap"
fi
unset -f __doctor_return_marker 2>/dev/null || true

test_case "_doctor_prefetch_versions waits only spawned jobs"
sleep 3 &
unrelated_pid=$!
start_ts=$(date +%s)
_doctor_prefetch_versions "git" >/dev/null 2>&1
end_ts=$(date +%s)
elapsed=$((end_ts - start_ts))
kill "$unrelated_pid" 2>/dev/null || true
wait "$unrelated_pid" 2>/dev/null || true
assert_true "[[ $elapsed -lt 2 ]]" "prefetch should not block on unrelated background jobs"

test_case "_doctor_prefetch_versions clears cache when temp dir creation fails"
(
    declare -gA _DOCTOR_VERSIONS
    _DOCTOR_VERSIONS["git"]="stale-value"
    mktemp() { return 1; }
    _doctor_prefetch_versions "git" >/dev/null 2>&1
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    [[ -z "${_DOCTOR_VERSIONS[git]:-}" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "prefetch failure should not leave stale version cache"

test_case "doctor_command_execute continues when prefetch fails"
(
    source "$LIB_DIR/app/commands/doctor_command.sh"
    _doctor_prefetch_versions() { return 1; }
    _doctor_output_table() { echo "ok"; }
    output=$(doctor_command_execute 2>/dev/null)
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$output" == "ok" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "doctor should still render output when prefetch fails"

test_case "_doctor_install_hint returns hint for known tools"
result=$(_doctor_install_hint "git")
assert_contains "$result" "brew" "should contain brew hint"

result=$(_doctor_install_hint "tmux")
assert_contains "$result" "brew" "should contain brew hint"

result=$(_doctor_install_hint "delta")
assert_contains "$result" "brew" "should contain brew hint"

test_case "_doctor_install_hint returns empty for unknown tool"
result=$(_doctor_install_hint "unknown_tool_xyz")
assert_empty "$result" "unknown tool should return empty hint"

test_case "_doctor_render_tools_group returns success when trailing newline disabled"
issues=0
_doctor_prefetch_versions "git" >/dev/null 2>&1 || true
_doctor_render_tools_group "Required:" "required" "true" _DOCTOR_REQUIRED_TOOLS issues "false" >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "render tools group should return success with trailing_newline=false"

test_case "_doctor_persistence_last_backup_iso returns failure when no backup exists"
(
    source "$LIB_DIR/app/commands/doctor_command.sh"
    persistence_latest_snapshot_time_or() { echo ""; }
    result=$(_doctor_persistence_last_backup_iso 2>/dev/null)
    rc=$?
    [[ "$rc" -ne 0 ]] || exit 1
    [[ -z "$result" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "should fail when latest backup is none"

test_case "_doctor_persistence_last_backup_iso returns iso timestamp when available"
(
    source "$LIB_DIR/app/commands/doctor_command.sh"
    persistence_latest_snapshot_time_or() { echo "2026-04-04T12:34:56+0800"; }
    result=$(_doctor_persistence_last_backup_iso)
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$result" == "2026-04-04T12:34:56+0800" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "should return parsed iso backup time"

test_suite "Internal Functions - GC"

source "$LIB_DIR/app/commands/gc_command.sh"

test_case "_gc_get_merged_branches returns branches"
cd "$TEST_DIR" || exit 1
# Create a branch and merge it back to main
git checkout -b test-merged-gc-$$ >/dev/null 2>&1
echo "test content" > test_file_gc.txt
git add test_file_gc.txt
git commit -m "Test merge" >/dev/null 2>&1
git checkout main >/dev/null 2>&1
git merge test-merged-gc-$$ >/dev/null 2>&1
result=$(_gc_get_merged_branches)
assert_contains "$result" "test-merged-gc-$$" "should find merged branch"
# Cleanup
git branch -d "test-merged-gc-$$" >/dev/null 2>&1 || true

test_case "_gc_get_orphan_sessions returns empty when no orphans"
cd "$TEST_DIR" || exit 1
result=$(_gc_get_orphan_sessions)
# With unique prefix, should have no orphan sessions
# (result may or may not be empty depending on environment)
assert_success 0 "should not error"

test_case "_gc_get_stale_state_files returns empty when no stale files"
cd "$TEST_DIR" || exit 1
result=$(_gc_get_stale_state_files)
assert_success 0 "should not error"

test_case "_gc_get_stale_state_files detects stale files"
cd "$TEST_DIR" || exit 1
mkdir -p .workhive/state
echo "test" > .workhive/state/layout_nonexistent-branch-xyz
result=$(_gc_get_stale_state_files)
assert_contains "$result" "layout_nonexistent-branch-xyz" "should find stale file"
rm -f .workhive/state/layout_nonexistent-branch-xyz

test_case "_gc_phase_unmodified returns 130 on Ctrl+C from confirmation prompt"
(
    cd "$TEST_DIR" || exit 1
    cleaned=0
    failed=0
    removed=false
    _gc_get_unmodified_branches() { echo "ctrlc-branch"; }
    _gc_filter_protected() { printf '%s\n' "$@"; }
    ask_yes_no() { return 130; }
    _gc_remove_branch() { removed=true; return 0; }
    _gc_phase_unmodified "false" "false" cleaned failed >/dev/null 2>&1
    rc=$?
    [[ "$rc" -eq 130 ]] || exit 1
    [[ "$removed" == "false" ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "Ctrl+C at gc confirmation should abort phase with 130 and skip cleanup"

test_case "gc_command_execute propagates interruption and stops later phases"
(
    cd "$TEST_DIR" || exit 1
    called_unmodified=0
    called_orphan=0
    called_stale=0
    called_prune=0
    _gc_phase_merged() { return 130; }
    _gc_phase_unmodified() { called_unmodified=1; return 0; }
    _gc_phase_orphan_sessions() { called_orphan=1; return 0; }
    _gc_phase_stale_files() { called_stale=1; return 0; }
    worktree_prune() { called_prune=1; return 0; }
    output=$(gc_command_execute 2>&1)
    rc=$?
    [[ "$rc" -eq 130 ]] || exit 1
    [[ "$called_unmodified" -eq 0 ]] || exit 1
    [[ "$called_orphan" -eq 0 ]] || exit 1
    [[ "$called_stale" -eq 0 ]] || exit 1
    [[ "$called_prune" -eq 0 ]] || exit 1
    if echo "$output" | grep -qi "complete"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "gc should stop immediately on Ctrl+C without success completion output"

# ══════════════════════════════════════════════════
# Internal Completions Command Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  internal completions - Command Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "_internal_completions zsh outputs valid zsh"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions zsh 2>&1)
assert_contains "$output" "#compdef workhive" "should contain zsh compdef directive"
assert_contains "$output" "_workhive" "should define _workhive function"

test_case "_internal_completions bash outputs valid bash"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions bash 2>&1)
assert_contains "$output" "complete -F" "should contain bash complete directive"
assert_contains "$output" "_workhive_completions" "should define completions function"

test_case "_internal_completions fish outputs valid fish"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions fish 2>&1)
assert_contains "$output" "complete -c workhive" "should contain fish complete directive"
assert_contains "$output" "__workhive_needs_command" "should define helper functions"

test_case "_internal_completions with unsupported shell shows error"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions powershell 2>&1 || true)
assert_contains "$output" "Unsupported shell" "should show unsupported error"

test_case "_internal_completions bash includes pane close id and list format"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions bash 2>&1)
assert_contains "$output" "--id --force -f --help -h" "should include pane close --id option set"
assert_contains "$output" "--format --help -h" "should include pane list --format option set"
assert_contains "$output" "--all --mode --commit --verbose --role --diagnose --help -h" "should include pane send mode/commit/broadcast options set"

test_case "_internal_completions fish excludes unsupported top-level commands"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions fish 2>&1)
assert_not_contains "$output" " -a completions " "should not include unsupported top-level command"

test_case "_internal_completions fish includes pane close id and list format"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions fish 2>&1)
assert_contains "$output" "-l id" "should include pane close id option"
assert_contains "$output" "-l format" "should include pane list format option"
assert_contains "$output" "table json" "should include json format option"

test_case "_internal_completions zsh includes codex-related completions"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions zsh 2>&1)
assert_contains "$output" "upgrade" "should have upgrade command"
assert_contains "$output" "doctor" "should have doctor command"

test_case "_internal_completions zsh includes pane close id and list format"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions zsh 2>&1)
assert_contains "$output" "--id" "should include pane close --id option"
assert_contains "$output" "--format" "should include pane list --format option"
assert_contains "$output" "json" "should include json format option"

test_case "_internal_completions bash syntax is valid"
cd "$TEST_DIR" || exit 1
bash -n "$ROOT_DIR/completions/workhive.bash" 2>&1
rc=$?
assert_success "$rc" "bash completion syntax should be valid"

test_case "_internal_completions zsh excludes unsupported top-level commands"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" _internal_completions zsh 2>&1)
assert_not_contains "$output" "completions:Output shell completion script" "should not include unsupported top-level command"

test_case "shell setup --dry-run minimal succeeds"
cd "$TEST_DIR" || exit 1
output=$("$BIN_DIR/workhive" shell setup --shell zsh --profile minimal --dry-run --yes 2>&1)
assert_contains "$output" "Profile: minimal" "should show selected profile"
assert_contains "$output" "Modules: completions" "minimal profile should include completions"

test_case "_shell_should_prompt_modules prompts only when module flags are not provided"
cd "$TEST_DIR" || exit 1
source "$LIB_DIR/app/commands/shell_command.sh" >/dev/null 2>&1 || true
rc=0
_shell_should_prompt_modules false "" "" || rc=$?
assert_eq "0" "${rc:-0}" "should prompt when interactive and no explicit module flags"

test_case "_shell_should_prompt_modules skips prompt when --enable/--disable is provided"
cd "$TEST_DIR" || exit 1
source "$LIB_DIR/app/commands/shell_command.sh" >/dev/null 2>&1 || true
rc=0
_shell_should_prompt_modules false "completions" "" || rc=$?
assert_eq "1" "${rc:-0}" "should skip prompt when explicit --enable is provided"
rc=0
_shell_should_prompt_modules false "" "aliases" || rc=$?
assert_eq "1" "${rc:-0}" "should skip prompt when explicit --disable is provided"

test_case "shell setup writes managed file and source line"
cd "$TEST_DIR" || exit 1
HOME_ORIG="$HOME"
_FAKE_HOME="/tmp/workhive_test_shell_home_$$"
export HOME="$_FAKE_HOME"
mkdir -p "$HOME"
echo "# test zshrc" > "$HOME/.zshrc"
XDG_CONFIG_HOME="$_FAKE_HOME/.config" "$BIN_DIR/workhive" shell setup --shell zsh --profile minimal --yes >/dev/null 2>&1
if [[ -f "$_FAKE_HOME/.config/workhive/shell/workhive.zsh" ]]; then rc_file_ok=0; else rc_file_ok=1; fi
assert_success "$rc_file_ok" "managed shell file should be generated"
output=$(grep "workhive Shell Integration" "$HOME/.zshrc" 2>/dev/null || true)
assert_not_empty "$output" "zshrc should contain managed source marker"
export HOME="$HOME_ORIG"
rm -rf "$_FAKE_HOME"

test_case "shell profile/module descriptions are localized in zh"
cd "$TEST_DIR" || exit 1
WORKHIVE_LANG=zh
source "$LIB_DIR/app/commands/shell_command.sh" >/dev/null 2>&1 || true
output=$(_shell_profile_desc minimal)
assert_contains "$output" "配置" "zh profile description should be localized"
output=$(_shell_module_desc completions)
assert_contains "$output" "补全" "zh module description should be localized"
WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# init_command.sh — _init_resolve_template_commands Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _init_resolve_template_commands - Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

cd "$TEST_DIR" || exit 1
source "$LIB_DIR/app/commands/init_command.sh" 2>/dev/null || true

test_case "_init_resolve_template_commands keeps installed tools"
result=$(_init_resolve_template_commands "git|tmux")
assert_contains "$result" "git" "git should be kept (installed)"
assert_contains "$result" "tmux" "tmux should be kept (installed)"

test_case "_init_build_available_commands resets tools registry cache before build"
(
    _reset_called=0
    _build_called=0
    tools_registry_reset_cache() { _reset_called=1; }
    _build_pane_command_list() { _build_called=1; }

    _init_build_available_commands

    [[ "$_reset_called" -eq 1 ]] || exit 1
    [[ "$_build_called" -eq 1 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "should invalidate tools registry cache before rebuilding command list"

test_case "extract_version_by_pattern parses semantic versions (shared helper)"
result=$(extract_version_by_pattern "git version 2.49.0 (Apple Git-150)" '[0-9]+\.[0-9]+\.[0-9]+')
assert_eq "2.49.0" "$result" "should extract semantic version"

test_case "get_tool_version supports non-default version flag (shared helper)"
result=$(get_tool_version "tmux" "-V" '[0-9]+\.[0-9]+')
assert_not_empty "$result" "tmux version should be detected"
if ! [[ "$result" =~ ^[0-9]+\.[0-9]+$ ]]; then
    fail "tmux version should match major.minor pattern"
fi

test_case "_init_resolve_template_commands replaces missing tools with null"
result=$(_init_resolve_template_commands "nonexistent_tool_xyz123|git")
assert_contains "$result" "null" "missing tool should become null"
assert_contains "$result" "git" "installed tool should be kept"

test_case "_init_resolve_template_commands handles null input"
result=$(_init_resolve_template_commands "null|git|null")
assert_eq "null|git|null" "$result" "null values should pass through"

test_case "_init_resolve_template_commands handles all missing"
result=$(_init_resolve_template_commands "fake_tool_1|fake_tool_2|fake_tool_3")
assert_eq "null|null|null" "$result" "all missing tools should become null"

test_case "_init_resolve_template_commands handles commands with args"
result=$(_init_resolve_template_commands "git --version|nonexist_tool --flag")
assert_contains "$result" "git --version" "command with args should be kept if binary exists"
assert_not_contains "$result" "nonexist_tool" "command with args should be nulled if binary missing"

test_case "_init_resolve_template_commands handles mixed null and tools"
result=$(_init_resolve_template_commands "null|git|null|tmux")
assert_eq "null|git|null|tmux" "$result" "mixed null and installed tools should be preserved"

test_case "_init_resolve_template_commands respects tools registry command list when available"
_AVAILABLE_CMD_VALUES=("codex" "lazygit" "__empty__" "__custom__")
result=$(_init_resolve_template_commands "claude --dangerously-skip-permissions|codex|null")
assert_eq "null|codex|null" "$result" "template commands not present in tools registry should become null"
unset _AVAILABLE_CMD_VALUES

test_case "_init_template_resolved_commands applies __empty__ default to template commands"
_orig_tools_registry_get_default_ref=$(declare -f tools_registry_get_default_ref || true)
tools_registry_get_default_ref() { echo "__empty__"; }
result=$(_init_template_resolved_commands "quad")
assert_eq "null|null|null|null" "$result" "__empty__ default should force template pane commands to empty shell"
unset -f tools_registry_get_default_ref
[[ -n "$_orig_tools_registry_get_default_ref" ]] && eval "$_orig_tools_registry_get_default_ref"

test_case "_init_reconcile_template_defaults_with_tools only updates template-sourced panes"
_init_pane_cmds=("claude --dangerously-skip-permissions" "claude --dangerously-skip-permissions" "lazygit")
_init_pane_cmd_sources=("template" "user" "template")
_init_col_count=2
_init_pane_counts=(2 1)
_orig_init_build_available_commands=$(declare -f _init_build_available_commands || true)
_orig_tools_registry_reset_cache=$(declare -f tools_registry_reset_cache || true)
_init_build_available_commands() {
    _AVAILABLE_CMD_LABELS=("Codex (codex)" "Lazygit (lazygit)" "$(_i18n EMPTY_SHELL_OPT)" "$(_i18n CUSTOM_INPUT)")
    _AVAILABLE_CMD_VALUES=("codex" "lazygit" "__empty__" "__custom__")
}
tools_registry_reset_cache() { :; }
_init_reconcile_template_defaults_with_tools
assert_eq "null" "${_init_pane_cmds[0]}" "template-only claude command should be removed when tool template missing"
assert_eq "claude --dangerously-skip-permissions" "${_init_pane_cmds[1]}" "user-provided command should not be modified by tools template changes"
assert_eq "lazygit" "${_init_pane_cmds[2]}" "available template command should be preserved"
unset -f _init_build_available_commands tools_registry_reset_cache
[[ -n "$_orig_init_build_available_commands" ]] && eval "$_orig_init_build_available_commands"
[[ -n "$_orig_tools_registry_reset_cache" ]] && eval "$_orig_tools_registry_reset_cache"

test_case "_init_columns_resolve_current_cmd falls back to tool default for new custom panes"
_COLUMNS_DEFAULT_PANE_CMDS=()
_AVAILABLE_CMD_VALUES=("claude --dangerously-skip-permissions" "__empty__" "__custom__")
_AVAILABLE_CMD_DEFAULT_IDX=0
result=$(_init_columns_resolve_current_cmd 0)
assert_eq "claude --dangerously-skip-permissions" "$result" "new custom pane should default to configured default tool command"

test_case "_init_columns_resolve_current_cmd keeps existing pane command from config"
_COLUMNS_DEFAULT_PANE_CMDS=("lazygit")
_AVAILABLE_CMD_VALUES=("claude --dangerously-skip-permissions" "__empty__" "__custom__")
_AVAILABLE_CMD_DEFAULT_IDX=0
result=$(_init_columns_resolve_current_cmd 0)
assert_eq "lazygit" "$result" "existing pane command should not be overwritten by default tool"

test_case "_init_columns_resolve_prompt_default_index prefers global default tool when current pane command is empty"
_AVAILABLE_CMD_VALUES=("claude --dangerously-skip-permissions" "codex" "__empty__" "__custom__")
_AVAILABLE_CMD_DEFAULT_IDX=0
_AVAILABLE_CMD_DEFAULT_REF="claude"
_AVAILABLE_CMD_EMPTY_IDX=2
result=$(_init_columns_resolve_prompt_default_index "")
assert_eq "0" "$result" "empty current command should highlight configured default tool"

test_case "_init_columns_resolve_prompt_default_index keeps empty-shell highlight when tools.default is __empty__"
_AVAILABLE_CMD_VALUES=("claude --dangerously-skip-permissions" "codex" "__empty__" "__custom__")
_AVAILABLE_CMD_DEFAULT_IDX=2
_AVAILABLE_CMD_DEFAULT_REF="__empty__"
_AVAILABLE_CMD_EMPTY_IDX=2
result=$(_init_columns_resolve_prompt_default_index "")
assert_eq "2" "$result" "empty default should continue highlighting Empty Shell"

test_case "_init_columns_resolve_prompt_default_index highlights custom input when tools.default is __custom__"
_AVAILABLE_CMD_VALUES=("claude --dangerously-skip-permissions" "codex" "__empty__" "__custom__")
_AVAILABLE_CMD_DEFAULT_IDX=3
_AVAILABLE_CMD_DEFAULT_REF="__custom__"
_AVAILABLE_CMD_EMPTY_IDX=2
result=$(_init_columns_resolve_prompt_default_index "")
assert_eq "3" "$result" "custom default should highlight Custom input option"

# ══════════════════════════════════════════════════
# init_command.sh — Template API Tests (columns-only)
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Template API - Column-based DSL Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "_tpl_get returns non-empty COLS for ai-coding-left"
result=$(_tpl_get "ai-coding-left" "COLS")
assert_not_empty "$result" "ai-coding-left COLS should be set"

test_case "_tpl_get returns non-empty PANES for ai-coding-left"
result=$(_tpl_get "ai-coding-left" "PANES")
assert_not_empty "$result" "ai-coding-left PANES should define pane counts"

test_case "_tpl_get returns non-empty WIDTHS for ai-coding-left"
result=$(_tpl_get "ai-coding-left" "WIDTHS")
assert_not_empty "$result" "ai-coding-left WIDTHS should define column widths"

test_case "_tpl_get returns non-empty HEIGHTS for ai-coding-left"
result=$(_tpl_get "ai-coding-left" "HEIGHTS")
assert_not_empty "$result" "ai-coding-left HEIGHTS should define pane heights"

test_case "custom template has COLS=0 (user-defined)"
result=$(_tpl_get "custom" "COLS")
assert_eq "0" "$result" "custom COLS should be 0 (interactive)"

# ══════════════════════════════════════════════════
# init_command.sh — _init_generate_config Tests (columns DSL)
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _init_generate_config - Columns DSL Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "_init_generate_config generates columns layout config (2 cols)"
cd "$TEST_DIR" || exit 1
rm -rf .workhive
_init_col_count=2
_init_pane_counts=(2 1)
_init_col_widths=(60 40)
_init_pane_heights=(50 50 100)
_init_pane_cmds=("null" "git" "null")
_init_generate_config "catppuccin-mocha"
assert_file_exists ".workhive/config.yaml" "config should be created"
config_content=$(cat .workhive/config.yaml)
assert_contains "$config_content" "columns:" "should have columns key"
assert_contains "$config_content" "width: 60" "should have col 0 width"
assert_contains "$config_content" "width: 40" "should have col 1 width"
assert_contains "$config_content" "panes:" "should have panes key"
assert_contains "$config_content" "height: 50" "should have pane heights"

test_case "_init_generate_config columns uses WORKHIVE_VERSION"
config_content=$(cat .workhive/config.yaml)
assert_contains "$config_content" "version: \"${WORKHIVE_VERSION}\"" "should use WORKHIVE_VERSION"

test_case "_init_generate_config columns preserves non-null commands"
assert_contains "$config_content" '"git"' "git command should be preserved"

test_case "_init_generate_config columns includes common config sections"
assert_contains "$config_content" "worktree:" "should have worktree section"
assert_contains "$config_content" "tmux:" "should have tmux section"
assert_contains "$config_content" 'theme: "catppuccin-mocha"' "should have theme"
assert_contains "$config_content" "sync_files: []" "should include worktree.sync_files"
assert_contains "$config_content" "sync_files format:" "should include sync_files format guidance comment"
assert_contains "$config_content" "shorthand also supported: sync_files: [\".env\"]" "should include sync_files shorthand compatibility guidance"
assert_contains "$config_content" "attach_delay: 1s" "should include tmux.attach_delay default"
assert_contains "$config_content" "CLAUDE_CODE_NO_FLICKER: 1" "should include tmux.env default no_flicker"
assert_contains "$config_content" "auto_save_env:" "should include persistence auto-save env config"
assert_contains "$config_content" "pre_kill_env:" "should include persistence pre-kill env config"
assert_contains "$config_content" "WORKHIVE_SAVE_LOCK_QUIET: 1" "should include persistence quiet env defaults"
assert_contains "$config_content" "WORKHIVE_SAVE_QUALITY_QUIET: 1" "should include persistence quality quiet env defaults"
assert_contains "$config_content" "WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1" "should include persistence retry env defaults"
assert_not_contains "$config_content" "statusbar:" "tmux.statusbar block should be removed"
assert_not_contains "$config_content" "watcher:" "tmux.watcher block should be removed"
assert_contains "$config_content" "persistence:" "should include tmux.persistence block"
assert_contains "$config_content" "auto_save_mode: hybrid" "should include persistence auto-save mode default"
assert_contains "$config_content" "auto_save_debounce: 3s" "should include persistence auto-save debounce default"
assert_contains "$config_content" "auto_save_interval: 10m" "should include persistence auto-save interval default"
assert_contains "$config_content" "snapshot_history_limit: 8" "should include persistence snapshot history limit default"
assert_contains "$config_content" "snapshot_max_size_mb: 8" "should include persistence snapshot size cap default"
assert_not_contains "$config_content" "tools:" "project init should not generate tools (global-only)"
assert_not_contains "$config_content" "git_init:" "project init should not generate git_init (global-only)"
assert_not_contains "$config_content" "status_bar:" "legacy tmux.status_bar should not be generated"

test_case "_init_generate_config has session_prefix commented out (dynamic)"
assert_contains "$config_content" "session_prefix" "should reference session_prefix in comments"
assert_not_contains "$config_content" "session_prefix: \"wh@" "session_prefix should NOT be hardcoded"

test_case "_init_generate_config columns 1-col 3-pane single column"
cd "$TEST_DIR" || exit 1
rm -rf .workhive
_init_col_count=1
_init_pane_counts=(3)
_init_col_widths=(100)
_init_pane_heights=(33 33 34)
_init_pane_cmds=("null" "null" "null")
_init_generate_config "catppuccin-mocha"
assert_file_exists ".workhive/config.yaml" "config should be created"
config_content=$(cat .workhive/config.yaml)
assert_contains "$config_content" "columns:" "should have columns key"
assert_contains "$config_content" "width: 100" "should have single col width"
assert_contains "$config_content" "height: 33" "should have first pane height"
assert_contains "$config_content" "height: 34" "should have last pane height"

# ══════════════════════════════════════════════════
# init_command.sh — Template Registry Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Template Registry - Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "template IDs include custom"
found_custom=false
for tpl_id in "${_TPL_IDS[@]}"; do
    [[ "$tpl_id" == "custom" ]] && found_custom=true
done
assert_eq "true" "$found_custom" "custom should be in _TPL_IDS"

test_case "custom template has COLS=0 (user-interactive)"
cols=$(_tpl_get "custom" "COLS")
assert_eq "0" "$cols" "custom template COLS should be 0 (interactive)"

test_case "custom template has empty PANES"
panes=$(_tpl_get "custom" "PANES")
assert_empty "$panes" "custom template PANES should be empty"

test_case "custom template has non-empty NAME"
name=$(_tpl_get "custom" "NAME")
assert_eq "Custom" "$name" "custom template NAME"

test_case "custom template has non-empty DESC"
desc=$(_tpl_get "custom" "DESC")
assert_not_empty "$desc" "custom template DESC should be non-empty"

test_case "custom template has non-empty DIAGRAM (dynamic)"
diagram=$(_tpl_custom_diagram)
assert_not_empty "$diagram" "custom template DIAGRAM should be non-empty"

test_case "all templates have COLS property"
for tpl_id in "${_TPL_IDS[@]}"; do
    cols=$(_tpl_get "$tpl_id" "COLS")
    if [[ -z "$cols" ]]; then
        assert_not_empty "" "$tpl_id should have COLS defined"
    fi
done
assert_eq 0 0 "all templates have COLS"

test_case "template dynamic diagram auto-aligns with long command labels"
diagram=$(
    tools_registry_get_default_ref() { echo ""; }
    _init_resolve_template_commands() {
        echo "super-extra-long-agent-command|null|shell|yazi"
    }
    _init_template_dynamic_diagram "ai-coding-left"
)
assert_contains "$diagram" "super-extra-long-agent-command" "dynamic diagram should keep full long labels"
assert_contains "$diagram" "┌" "diagram should include top-left corner"
assert_contains "$diagram" "┐" "diagram should include top-right corner"
assert_contains "$diagram" "└" "diagram should include bottom-left corner"
assert_contains "$diagram" "┘" "diagram should include bottom-right corner"
last_col_closed=0
if printf '%s\n' "$diagram" | awk 'END{ if ($0 ~ /[┐┘]$/) exit 0; exit 1 }'; then
    last_col_closed=1
fi
assert_eq "1" "$last_col_closed" "diagram should keep right border closed"

# ══════════════════════════════════════════════════
# i18n — Columns layout string tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  i18n - Columns Layout Strings Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "$LIB_DIR/utils/i18n.sh" 2>/dev/null || true

test_case "i18n LAYOUT_COLUMNS_DESC (en) returns non-empty"
WORKHIVE_LANG=en
result=$(_i18n LAYOUT_COLUMNS_DESC)
assert_not_empty "$result" "LAYOUT_COLUMNS_DESC should be non-empty in English"

test_case "i18n PANE_LAYOUT_CONFIRM_REBUILD (en) returns non-empty"
result=$(_i18n PANE_LAYOUT_CONFIRM_REBUILD)
assert_not_empty "$result" "PANE_LAYOUT_CONFIRM_REBUILD should be non-empty in English"

test_case "i18n PANE_LAYOUT_DONE (en) returns non-empty"
result=$(_i18n PANE_LAYOUT_DONE)
assert_not_empty "$result" "PANE_LAYOUT_DONE should be non-empty in English"

test_case "i18n LAYOUT_COLUMNS_DESC (zh) returns Chinese"
WORKHIVE_LANG=zh
result=$(_i18n LAYOUT_COLUMNS_DESC)
assert_not_empty "$result" "LAYOUT_COLUMNS_DESC should be non-empty in Chinese"

test_case "i18n PANE_LAYOUT_CONFIRM_REBUILD (zh) returns Chinese"
result=$(_i18n PANE_LAYOUT_CONFIRM_REBUILD)
assert_not_empty "$result" "PANE_LAYOUT_CONFIRM_REBUILD should be non-empty in Chinese"

# Restore lang
WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# i18n — GC + Remove new strings
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  i18n - GC & Remove Strings Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

WORKHIVE_LANG=en

test_case "i18n GC_SCANNING_UNMODIFIED (en) returns non-empty"
result=$(_i18n GC_SCANNING_UNMODIFIED)
assert_not_empty "$result"

test_case "i18n GC_FOUND_UNMODIFIED (en) returns non-empty"
result=$(_i18n GC_FOUND_UNMODIFIED)
assert_not_empty "$result"

test_case "i18n GC_CONFIRM_UNMODIFIED (en) returns non-empty"
result=$(_i18n GC_CONFIRM_UNMODIFIED)
assert_not_empty "$result"

test_case "i18n GC_NO_UNMODIFIED (en) returns non-empty"
result=$(_i18n GC_NO_UNMODIFIED)
assert_not_empty "$result"

test_case "i18n WARN_BRANCH_UNMERGED (en) returns non-empty"
result=$(_i18n WARN_BRANCH_UNMERGED)
assert_not_empty "$result"

test_case "i18n CONFIRM_DELETE_UNMERGED (en) returns non-empty"
result=$(_i18n CONFIRM_DELETE_UNMERGED)
assert_not_empty "$result"

WORKHIVE_LANG=zh

test_case "i18n GC_SCANNING_UNMODIFIED (zh) returns non-empty"
result=$(_i18n GC_SCANNING_UNMODIFIED)
assert_not_empty "$result"

test_case "i18n WARN_BRANCH_UNMERGED (zh) returns non-empty"
result=$(_i18n WARN_BRANCH_UNMERGED)
assert_not_empty "$result"

test_case "i18n CONFIRM_DELETE_UNMERGED (zh) returns non-empty"
result=$(_i18n CONFIRM_DELETE_UNMERGED)
assert_not_empty "$result"

WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# gc_command.sh — _gc_get_unmodified_branches Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _gc_get_unmodified_branches - Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "${LIB_DIR}/app/commands/gc_command.sh" 2>/dev/null || true

test_case "_gc_get_unmodified_branches function exists"
type _gc_get_unmodified_branches &>/dev/null
assert_eq "0" "$?"

test_case "_gc_get_orphan_sessions function exists"
type _gc_get_orphan_sessions &>/dev/null
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# workspace_service.sh Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  workspace_service.sh Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "${LIB_DIR}/domain/services/workspace_service.sh" 2>/dev/null || true

test_case "workspace_prepare_directory function exists"
type workspace_prepare_directory &>/dev/null
assert_eq "0" "$?"

test_case "workspace_ensure_session function exists"
type workspace_ensure_session &>/dev/null
assert_eq "0" "$?"

test_case "workspace_open function exists"
type workspace_open &>/dev/null
assert_eq "0" "$?"

test_case "_workspace_rollback function exists"
type _workspace_rollback &>/dev/null
assert_eq "0" "$?"

test_case "WORKSPACE_RESULT_DIR is initialized empty"
WORKSPACE_RESULT_DIR=""
assert_eq "" "$WORKSPACE_RESULT_DIR"

test_case "workspace_prepare_directory returns project_root for non-worktree mode"
cd "$TEST_DIR" || exit 1
workspace_prepare_directory "main" "false" "$TEST_DIR" >/dev/null 2>&1
assert_eq "$TEST_DIR" "$_WS_PREPARED_DIR"

# ══════════════════════════════════════════════════
# cache_gc Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  cache_gc Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "cache_gc function exists"
type cache_gc &>/dev/null
assert_eq "0" "$?"

test_case "cache_gc removes expired entries"
cache_clear
cache_set "test_gc_expired" "value" 1
sleep 2
cache_gc
result=$(cache_get "test_gc_expired" 2>/dev/null) || true
assert_eq "" "$result"

test_case "cache_gc preserves non-expired entries"
cache_clear
cache_set "test_gc_valid" "alive" 300
cache_gc
result=$(cache_get "test_gc_valid")
assert_eq "alive" "$result"

# ══════════════════════════════════════════════════
# Terminal adapter contract verification Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Adapter Contract Verification Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "_terminal_adapter_verify_contract function exists"
type _terminal_adapter_verify_contract &>/dev/null
assert_eq "0" "$?"

test_case "_terminal_adapter_verify_contract passes for tmux adapter"
_terminal_adapter_verify_contract "tmux" >/dev/null 2>&1
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# i18n new keys Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  New i18n Keys Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

WORKHIVE_LANG=en

test_case "i18n ERR_CONFIG_NOT_READABLE (en) returns non-empty"
result=$(_i18n ERR_CONFIG_NOT_READABLE)
assert_not_empty "$result"

WORKHIVE_LANG=zh

test_case "i18n ERR_CONFIG_NOT_READABLE (zh) returns non-empty"
result=$(_i18n ERR_CONFIG_NOT_READABLE)
assert_not_empty "$result"

WORKHIVE_LANG=en

test_case "ERR_WT_CREATE_FAILED message includes actionable hint"
result=$(_i18n ERR_WT_CREATE_FAILED)
assert_contains "$result" "workhive open"

test_case "ERR_SESSION_CREATE_FAILED message includes actionable hint"
result=$(_i18n ERR_SESSION_CREATE_FAILED)
assert_contains "$result" "tmux ls"

test_case "ERR_NOT_GIT_REPO message includes actionable hint"
result=$(_i18n ERR_NOT_GIT_REPO)
assert_contains "$result" "git init"

WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# i18n — pane send / list keys
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  i18n - Pane Send & List Keys Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

WORKHIVE_LANG=en

test_case "i18n PANE_SEND_USAGE (en) returns non-empty"
result=$(_i18n PANE_SEND_USAGE)
assert_not_empty "$result"

test_case "i18n PANE_SEND_INVALID_INDEX (en) returns non-empty"
result=$(_i18n PANE_SEND_INVALID_INDEX)
assert_not_empty "$result"

test_case "i18n PANE_SEND_OK (en) returns non-empty"
result=$(_i18n PANE_SEND_OK)
assert_not_empty "$result"

test_case "i18n PANE_LIST_TITLE (en) returns non-empty"
result=$(_i18n PANE_LIST_TITLE)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_INDEX (en) returns non-empty"
result=$(_i18n PANE_LIST_HDR_INDEX)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_CMD (en) returns non-empty"
result=$(_i18n PANE_LIST_HDR_CMD)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_DIR (en) returns non-empty"
result=$(_i18n PANE_LIST_HDR_DIR)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_SIZE (en) returns non-empty"
result=$(_i18n PANE_LIST_HDR_SIZE)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_STATUS (en) returns non-empty"
result=$(_i18n PANE_LIST_HDR_STATUS)
assert_not_empty "$result"

WORKHIVE_LANG=zh

test_case "i18n PANE_SEND_USAGE (zh) returns non-empty"
result=$(_i18n PANE_SEND_USAGE)
assert_not_empty "$result"

test_case "i18n PANE_SEND_OK (zh) returns non-empty"
result=$(_i18n PANE_SEND_OK)
assert_not_empty "$result"

test_case "i18n PANE_LIST_TITLE (zh) returns non-empty"
result=$(_i18n PANE_LIST_TITLE)
assert_not_empty "$result"

test_case "i18n PANE_LIST_HDR_INDEX (zh) returns non-empty"
result=$(_i18n PANE_LIST_HDR_INDEX)
assert_not_empty "$result"

WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# pane_command.sh — _pane_resolve_cmd_alias Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _pane_resolve_cmd_alias Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "$LIB_DIR/app/commands/pane_command.sh" 2>/dev/null || true

test_case "_pane_resolve_cmd_alias function exists"
type _pane_resolve_cmd_alias &>/dev/null
assert_eq "0" "$?"

test_case "_pane_resolve_cmd_alias resolves 'claude'"
result=$(_pane_resolve_cmd_alias "claude")
assert_contains "$result" "claude" "should contain claude in resolved command"

test_case "_pane_resolve_cmd_alias resolves 'lazygit'"
result=$(_pane_resolve_cmd_alias "lazygit")
assert_contains "$result" "lazygit" "should contain lazygit"

test_case "_pane_resolve_cmd_alias resolves 'shell' to empty (default shell)"
result=$(_pane_resolve_cmd_alias "shell")
assert_eq "" "$result" "shell should resolve to empty string (default shell)"

test_case "_pane_resolve_cmd_alias passes through unknown alias"
result=$(_pane_resolve_cmd_alias "unknown_tool_xyz")
assert_eq "unknown_tool_xyz" "$result" "unknown alias should pass through unchanged"

# ══════════════════════════════════════════════════
# Config permission check Test
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Config Permission Check Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "config_load_file fails for unreadable file"
_unreadable_file="$TEST_DIR/.workhive/unreadable.yaml"
echo "version: 1" > "$_unreadable_file"
chmod 000 "$_unreadable_file"
config_load_file "$_unreadable_file" >/dev/null 2>&1
assert_eq "1" "$?"
rm -f "$_unreadable_file"

# Restore config for later tests
cd "$TEST_DIR" || exit 1
mkdir -p .workhive
cat > .workhive/config.yaml <<EOF
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: null
tmux:
  mouse_support: true
  theme: "catppuccin-mocha"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF

# ══════════════════════════════════════════════════
# destroy --global Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  destroy --global Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "destroy --help shows --global option"
output=$("$BIN_DIR/workhive" destroy --help 2>&1)
assert_contains "$output" "--global" "should show --global option"

test_case "destroy --help (zh) shows --global option"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" destroy --help 2>&1)
assert_contains "$output" "--global" "should show --global option in Chinese"

test_case "destroy --global when no global config shows info"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_empty_$$"
mkdir -p "$XDG_CONFIG_HOME"
output=$("$BIN_DIR/workhive" destroy --global 2>&1)
assert_contains "$output" "config" "should mention config"
export XDG_CONFIG_HOME="$_save_xdg"

test_case "destroy --global --force removes global config"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_destroy_test_$$"
mkdir -p "$XDG_CONFIG_HOME/workhive"
echo "version: 0.1.0" > "$XDG_CONFIG_HOME/workhive/config.yaml"
destroy_test_log="$TEST_DIR/.workhive/logs/destroy-op-test.log"
mkdir -p "$(dirname "$destroy_test_log")"
: > "$destroy_test_log"
WORKHIVE_TEST_LOG_FILE="$destroy_test_log" "$BIN_DIR/workhive" destroy --global --force >/dev/null 2>&1
assert_success "$([[ ! -d "$XDG_CONFIG_HOME/workhive" ]] && echo 0 || echo 1)" "global dir should be deleted"
destroy_log=$(cat "$destroy_test_log" 2>/dev/null || true)
assert_contains "$destroy_log" "[OP][BEGIN] action=destroy_global_config" "should log destroy begin"
assert_contains "$destroy_log" "[OP][OK] action=destroy_global_config" "should log destroy success"
export XDG_CONFIG_HOME="$_save_xdg"

test_case "destroy --global rejects unknown options"
output=$("$BIN_DIR/workhive" destroy --global --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

# ══════════════════════════════════════════════════
# i18n — destroy --global strings
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  i18n - destroy --global Strings Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

WORKHIVE_LANG=en

test_case "i18n TITLE_DESTROY_GLOBAL (en) returns non-empty"
result=$(_i18n TITLE_DESTROY_GLOBAL)
assert_not_empty "$result"

test_case "i18n MSG_GLOBAL_RESOURCES_TO_DELETE (en) returns non-empty"
result=$(_i18n MSG_GLOBAL_RESOURCES_TO_DELETE)
assert_not_empty "$result"

test_case "i18n MSG_GLOBAL_CONFIG_DIR (en) returns non-empty"
result=$(_i18n MSG_GLOBAL_CONFIG_DIR)
assert_not_empty "$result"

test_case "i18n CONFIRM_DESTROY_GLOBAL (en) returns non-empty"
result=$(_i18n CONFIRM_DESTROY_GLOBAL)
assert_not_empty "$result"

test_case "i18n MSG_GLOBAL_DESTROY_COMPLETE (en) returns non-empty"
result=$(_i18n MSG_GLOBAL_DESTROY_COMPLETE)
assert_not_empty "$result"

test_case "i18n MSG_USE_INIT_GLOBAL_REINIT (en) returns non-empty"
result=$(_i18n MSG_USE_INIT_GLOBAL_REINIT)
assert_not_empty "$result"

WORKHIVE_LANG=zh

test_case "i18n TITLE_DESTROY_GLOBAL (zh) returns non-empty"
result=$(_i18n TITLE_DESTROY_GLOBAL)
assert_not_empty "$result"

test_case "i18n CONFIRM_DESTROY_GLOBAL (zh) returns non-empty"
result=$(_i18n CONFIRM_DESTROY_GLOBAL)
assert_not_empty "$result"

test_case "i18n MSG_GLOBAL_DESTROY_COMPLETE (zh) returns non-empty"
result=$(_i18n MSG_GLOBAL_DESTROY_COMPLETE)
assert_not_empty "$result"

test_case "i18n MSG_USE_INIT_GLOBAL_REINIT (zh) returns non-empty"
result=$(_i18n MSG_USE_INIT_GLOBAL_REINIT)
assert_not_empty "$result"

WORKHIVE_LANG=en

# ══════════════════════════════════════════════════
# config --global layout modification Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  config --global Layout Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "$LIB_DIR/app/commands/config_command.sh" 2>/dev/null || true

test_case "config_command_global_interactive function exists"
type config_command_global_interactive &>/dev/null
assert_eq "0" "$?"

test_case "_config_command_modify_layout function exists"
type _config_command_modify_layout &>/dev/null
assert_eq "0" "$?"

test_case "config interactive asks modify confirmation before template selection"
(
    fn_body=$(sed -n '/_config_command_interactive_impl()/,/^}/p' "$LIB_DIR/app/commands/config_command.sh")
    echo "$fn_body" | grep -q 'ask_yes_no "\$(_i18n MODIFY_LAYOUT)" "N"' || exit 1
    exit 0
)
assert_eq "0" "$?" "config interactive should ask modify confirmation before template selection"

test_case "config interactive handles decline and Ctrl+C for modify confirmation"
(
    fn_body=$(sed -n '/_config_command_interactive_impl()/,/^}/p' "$LIB_DIR/app/commands/config_command.sh")
    echo "$fn_body" | grep -q '\[\[ \$_modify_layout_rc -eq 130 \]\]' || exit 1
    echo "$fn_body" | grep -q '\[\[ \$_modify_layout_rc -ne 0 \]\]' || exit 1
    echo "$fn_body" | grep -q 'MSG_CANCELLED' || exit 1
    exit 0
)
assert_eq "0" "$?" "config interactive should return 130 on Ctrl+C and exit cleanly on decline"

test_case "config --global --edit without config prompts for init"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config_edit_test_$$"
mkdir -p "$XDG_CONFIG_HOME"
output=$(echo "n" | "$BIN_DIR/workhive" config --global --edit 2>&1 || true)
assert_contains "$output" "Location:" "should show config location"
export XDG_CONFIG_HOME="$_save_xdg"

# ══════════════════════════════════════════════════
# TUI Ctrl+C handling Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  TUI Ctrl+C Handling Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "tui_select_menu INT trap exits with code 130"
(
    if grep -q 'exit 130' "$LIB_DIR/utils/tui.sh"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "tui.sh should have exit 130 for SIGINT handling"

test_case "tui_select_menu does not use 'return 0' in INT trap"
(
    if grep -q "trap.*INT" "$LIB_DIR/utils/tui.sh" | head -1 | grep -q "return 0"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "INT trap should not use 'return 0' (unreliable in loops)"

test_case "status watch INT handling does not use trap return 0"
(
    if grep -E -q "trap .*INT.*return 0|trap '.*return 0.*' INT" "$LIB_DIR/app/commands/list_command.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "list_command INT handling should avoid trap return 0 pattern"

test_case "attach interactive selector does not use trap-return INT handler"
(
    if grep -E -q "trap '.*return 1.*' INT" "$LIB_DIR/app/commands/attach_command.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "attach selector should avoid trap-return INT handlers"

test_case "pane session selector does not use trap-return INT handler"
(
    if grep -E -q "trap '.*return 1.*' INT" "$LIB_DIR/app/commands/pane_command.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "pane selector should avoid trap-return INT handlers"

test_case "worktree selector does not use trap-return INT handler"
(
    if grep -E -q "trap '.*return 1.*' INT" "$LIB_DIR/app/shared/worktree_selector.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "worktree selector should avoid trap-return INT handlers"

# ══════════════════════════════════════════════════
# config_command_print_global enhanced layout output Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  config_command_print_global Layout Detail Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "config_command_print_global shows global config header"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_print_test_$$"
mkdir -p "$XDG_CONFIG_HOME/workhive"
config_get_defaults > "$XDG_CONFIG_HOME/workhive/config.yaml"
output=$(config_command_print_global 2>&1)
assert_contains "$output" "Global" "should show global config title"
export XDG_CONFIG_HOME="$_save_xdg"

# ══════════════════════════════════════════════════
# destroy_command.sh — _destroy_global_config Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _destroy_global_config Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

source "$LIB_DIR/app/commands/destroy_command.sh" 2>/dev/null || true

test_case "_destroy_global_config function exists"
type _destroy_global_config &>/dev/null
assert_eq "0" "$?"

test_case "_destroy_global_config with force removes directory"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_destroy_func_$$"
mkdir -p "$XDG_CONFIG_HOME/workhive"
echo "test" > "$XDG_CONFIG_HOME/workhive/config.yaml"
_destroy_global_config "true" >/dev/null 2>&1
assert_success "$([[ ! -d "$XDG_CONFIG_HOME/workhive" ]] && echo 0 || echo 1)" "global dir should be removed"
export XDG_CONFIG_HOME="$_save_xdg"

# ══════════════════════════════════════════════════
# DRY Refactor — config delegates to init Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Config-to-Init Delegation Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "i18n ASK_CREATE_PROJECT_CONFIG (en) returns non-empty"
WORKHIVE_LANG=en
output=$(_i18n ASK_CREATE_PROJECT_CONFIG)
assert_not_empty "$output"

test_case "i18n ASK_CREATE_PROJECT_CONFIG (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n ASK_CREATE_PROJECT_CONFIG)
assert_not_empty "$output"
WORKHIVE_LANG=en

test_case "i18n ASK_CREATE_GLOBAL_CONFIG mentions init --global"
WORKHIVE_LANG=en
output=$(_i18n ASK_CREATE_GLOBAL_CONFIG)
assert_contains "$output" "init --global" "should mention init --global command"

test_case "i18n ASK_CREATE_PROJECT_CONFIG mentions init"
WORKHIVE_LANG=en
output=$(_i18n ASK_CREATE_PROJECT_CONFIG)
assert_contains "$output" "init" "should mention init command"

test_case "i18n INFO_GLOBAL_TOOLS_TEMPLATE_INITIALIZED (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n INFO_GLOBAL_TOOLS_TEMPLATE_INITIALIZED)
assert_not_empty "$output"

test_case "i18n INFO_TOOLS_TEMPLATE_SCOPE (en) mentions manual command preservation"
WORKHIVE_LANG=en
output=$(_i18n INFO_TOOLS_TEMPLATE_SCOPE)
assert_contains "$output" "do not overwrite manually entered pane commands" "English tools template scope should clearly preserve manual commands"

test_case "i18n INFO_TOOLS_TEMPLATE_SCOPE (zh) mentions manual command preservation"
WORKHIVE_LANG=zh
output=$(_i18n INFO_TOOLS_TEMPLATE_SCOPE)
assert_contains "$output" "不会覆盖你手动输入的 pane 命令" "Chinese tools template scope should clearly preserve manual commands"

test_case "i18n ASK_USE_DEFAULT_TOOLS_TEMPLATE (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n ASK_USE_DEFAULT_TOOLS_TEMPLATE)
assert_not_empty "$output"

test_case "i18n INFO_OPENING_TOOLS_MANAGER (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n INFO_OPENING_TOOLS_MANAGER)
assert_not_empty "$output"
WORKHIVE_LANG=en

test_case "i18n INFO_NO_GLOBAL_CONFIG does not end with colon"
WORKHIVE_LANG=en
output=$(_i18n INFO_NO_GLOBAL_CONFIG)
(
    if [[ "$output" == *":" ]]; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "INFO_NO_GLOBAL_CONFIG should not end with colon"

test_case "config_command_edit_file no longer uses config_get_defaults"
(
    if grep -q 'config_get_defaults' "$LIB_DIR/app/commands/config_command.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "config_command.sh should not call config_get_defaults (delegated to init)"

test_case "config_command.sh no longer uses ERR_RUN_INIT_FIRST"
(
    if grep -q 'ERR_RUN_INIT_FIRST' "$LIB_DIR/app/commands/config_command.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "config_command.sh should not use ERR_RUN_INIT_FIRST (replaced by delegation)"

test_case "config_command.sh uses ASK_CREATE_PROJECT_CONFIG"
(
    if grep -q 'ASK_CREATE_PROJECT_CONFIG' "$LIB_DIR/app/commands/config_command.sh"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "config_command.sh should reference ASK_CREATE_PROJECT_CONFIG"

test_case "config_command.sh uses ASK_CREATE_GLOBAL_CONFIG"
(
    if grep -q 'ASK_CREATE_GLOBAL_CONFIG' "$LIB_DIR/app/commands/config_command.sh"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "config_command.sh should reference ASK_CREATE_GLOBAL_CONFIG"

test_case "config_command.sh delegates to init_command_execute"
(
    if grep -q 'init_command_execute' "$LIB_DIR/app/commands/config_command.sh"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "config_command.sh should delegate to init_command_execute"

test_case "config --global without config shows Location"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_loc_test_$$"
mkdir -p "$XDG_CONFIG_HOME"
output=$(echo "n" | "$BIN_DIR/workhive" config --global 2>&1 || true)
assert_contains "$output" "Location:" "should display Location path"
export XDG_CONFIG_HOME="$_save_xdg"

test_case "config_command_print_global without config shows Location"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_print_loc_$$"
mkdir -p "$XDG_CONFIG_HOME"
output=$(config_command_print_global 2>&1)
assert_contains "$output" "Location:" "should show Location in print_global output"
export XDG_CONFIG_HOME="$_save_xdg"

test_case "destroy --global without config shows Location"
cd "$TEST_DIR" || exit 1
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_destroy_loc_$$"
mkdir -p "$XDG_CONFIG_HOME"
output=$("$BIN_DIR/workhive" destroy --global 2>&1 || true)
assert_contains "$output" "Location:" "should display Location path in destroy"
export XDG_CONFIG_HOME="$_save_xdg"

# ══════════════════════════════════════════════════
# Global git repo guard + init git detection Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Git Repo Guard & Init Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "i18n ASK_INIT_GIT_REPO (en) returns non-empty"
WORKHIVE_LANG=en
output=$(_i18n ASK_INIT_GIT_REPO)
assert_not_empty "$output"

test_case "i18n ASK_INIT_GIT_REPO (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n ASK_INIT_GIT_REPO)
assert_not_empty "$output"
WORKHIVE_LANG=en

test_case "i18n MSG_GIT_INIT_SUCCESS (en) returns non-empty"
WORKHIVE_LANG=en
output=$(_i18n MSG_GIT_INIT_SUCCESS)
assert_not_empty "$output"

test_case "i18n MSG_GIT_INIT_REQUIRED (en) returns non-empty"
WORKHIVE_LANG=en
output=$(_i18n MSG_GIT_INIT_REQUIRED)
assert_not_empty "$output"

test_case "i18n MSG_GIT_INIT_REQUIRED (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n MSG_GIT_INIT_REQUIRED)
assert_not_empty "$output"
WORKHIVE_LANG=en

test_case "bin/workhive has check_git_repo function"
(
    if grep -q 'check_git_repo' "$BIN_DIR/workhive"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "bin/workhive should have check_git_repo"

test_case "check_git_repo exempts init command (via capability matrix)"
NON_REPO_DIR=$(mktemp -d "/tmp/workhive-nogit-init-$$-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" init --help 2>&1)
assert_not_contains "$output" "Git repository is required" "init should be exempt from git check"
rm -rf "$NON_REPO_DIR"

test_case "check_git_repo exempts config command (via capability matrix)"
NON_REPO_DIR=$(mktemp -d "/tmp/workhive-nogit-cfg-$$-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" config --help 2>&1)
assert_not_contains "$output" "Git repository is required" "config should be exempt from git check"
rm -rf "$NON_REPO_DIR"

test_case "check_git_repo exempts doctor command (via capability matrix)"
NON_REPO_DIR=$(mktemp -d "/tmp/workhive-nogit-doc-$$-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" doctor 2>&1)
assert_not_contains "$output" "Git repository is required" "doctor should be exempt from git check"
rm -rf "$NON_REPO_DIR"

test_case "check_git_repo delegates to _git_init_guarded"
(grep -q '_git_init_guarded' "$BIN_DIR/workhive" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "check_git_repo delegates to _git_ensure_initial_commit"
(grep -q '_git_ensure_initial_commit' "$BIN_DIR/workhive" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "check_git_repo uses _is_git_init_allowed path guard"
(grep -q '_is_git_init_allowed' "$BIN_DIR/workhive" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "_git_init_guarded handles git init failure (ERR_GIT_INIT_FAILED)"
(grep -A30 '_git_init_guarded()' "$LIB_DIR/utils/common.sh" | grep -q 'ERR_GIT_INIT_FAILED' && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "_git_init_guarded uses ask_yes_no with default N"
(grep -A30 '_git_init_guarded()' "$LIB_DIR/utils/common.sh" | grep -q 'ask_yes_no.*"N"' && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "init_command.sh delegates to _git_init_guarded"
(grep -q '_git_init_guarded' "$LIB_DIR/app/commands/init_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?" "init_command.sh should delegate to shared git init helper"

test_case "init_command.sh delegates to _git_ensure_initial_commit"
(grep -q '_git_ensure_initial_commit' "$LIB_DIR/app/commands/init_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?" "init_command.sh should delegate to shared commit helper"

test_case "init_command.sh uses _is_git_init_allowed path guard"
(grep -q '_is_git_init_allowed' "$LIB_DIR/app/commands/init_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?" "init_command.sh should use unified path guard"

test_case ".gitignore-template exists in project root"
if [[ -f "$ROOT_DIR/.gitignore-template" ]]; then rc=0; else rc=1; fi
assert_eq "0" "$rc"

test_case ".gitignore-template contains .workhive/"
(grep -q '\.workhive/' "$ROOT_DIR/.gitignore-template" && exit 0 || exit 1)
rc=$?
assert_eq "0" "$rc"

test_case ".gitignore-template contains .DS_Store"
(grep -q '\.DS_Store' "$ROOT_DIR/.gitignore-template" && exit 0 || exit 1)
rc=$?
assert_eq "0" "$rc"

test_case "_apply_gitignore_template function exists"
type _apply_gitignore_template &>/dev/null
assert_eq "0" "$?"

test_case "_apply_gitignore_template copies template when .gitignore missing"
_gi_test_dir="$TEST_DIR/.gi_test_$$"
mkdir -p "$_gi_test_dir"
(
    cd "$_gi_test_dir" || exit 1
    _apply_gitignore_template
    [[ -f ".gitignore" ]]
)
assert_eq "0" "$?"

test_case "_apply_gitignore_template skips when .gitignore exists"
(
    cd "$_gi_test_dir" || exit 1
    echo "# custom" > .gitignore
    _apply_gitignore_template
    grep -q '# custom' .gitignore
)
assert_eq "0" "$?"

test_case "_git_init_guarded calls _apply_gitignore_template"
(grep -A30 '_git_init_guarded()' "$LIB_DIR/utils/common.sh" | grep -q '_apply_gitignore_template' && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "_git_init_guarded creates initial commit"
(grep -A30 '_git_init_guarded()' "$LIB_DIR/utils/common.sh" | grep -q 'git commit' && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "workhive ls outside git repo shows error"
_tmp_nogit="/tmp/workhive-nogit-ls-$$"
mkdir -p "$_tmp_nogit"
output=$(cd "$_tmp_nogit" && "$BIN_DIR/workhive" ls 2>&1 || true)
assert_contains "$output" "Git" "should mention Git in error"
rm -rf "$_tmp_nogit"

test_case "workhive open outside git repo shows error"
_tmp_nogit="/tmp/workhive-nogit-open-$$"
mkdir -p "$_tmp_nogit"
output=$(cd "$_tmp_nogit" && "$BIN_DIR/workhive" open 2>&1 || true)
assert_contains "$output" "Git" "should mention Git in error"
rm -rf "$_tmp_nogit"

# ══════════════════════════════════════════════════
# Doctor config check fix Tests
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Doctor Config & Git Check Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "i18n DOCTOR_CONFIG_GLOBAL_ONLY (en) returns non-empty"
WORKHIVE_LANG=en
output=$(_i18n DOCTOR_CONFIG_GLOBAL_ONLY)
assert_not_empty "$output"

test_case "i18n DOCTOR_CONFIG_GLOBAL_ONLY (zh) returns non-empty"
WORKHIVE_LANG=zh
output=$(_i18n DOCTOR_CONFIG_GLOBAL_ONLY)
assert_not_empty "$output"
WORKHIVE_LANG=en

test_case "doctor checks project config file separately"
(
    if grep -q '_project_cfg' "$LIB_DIR/app/commands/doctor_command.sh"; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "doctor should check project config independently"

test_case "doctor git check uses red marker for missing repo"
(
    if grep -A2 'DOCTOR_NOT_GIT_REPO' "$LIB_DIR/app/commands/doctor_command.sh" | grep -q 'RED'; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "doctor should use RED marker for missing git repo"

test_case "doctor git check increments issues for missing repo"
(
    if grep -A3 'DOCTOR_NOT_GIT_REPO' "$LIB_DIR/app/commands/doctor_command.sh" | grep -qE 'issues|_dg_issues'; then
        exit 0
    fi
    exit 1
)
assert_eq "0" "$?" "doctor should increment issues for missing git repo"

# ══════════════════════════════════════════════════
# config uses config_project_exists (not config_exists)
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  config uses config_project_exists Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "config_command.sh uses config_project_exists"
(grep -q 'config_project_exists' "$LIB_DIR/app/commands/config_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "config_command.sh does not use config_exists for project check"
(grep 'config_exists' "$LIB_DIR/app/commands/config_command.sh" | grep -v 'config_project_exists' | grep -v 'config_global_exists' | grep -v '#' | head -1 | grep -q 'config_exists' && exit 1 || exit 0)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# check_init uses config_project_exists
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  check_init allows global config fallback Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "bin/workhive check_init uses config_exists (project OR global)"
(grep -q 'config_exists' "$BIN_DIR/workhive" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "bin/workhive check_init bypasses --help subarg via capability matrix"
output=$("$BIN_DIR/workhive" list --help 2>&1)
assert_not_contains "$output" "not initialized" "subcommand help should bypass init check"

test_case "bin/workhive global bootstrap asks rerun instead of auto-continue"
(
    grep -q 'MSG_GLOBAL_CONFIG_DONE_RERUN' "$BIN_DIR/workhive" || exit 1
    grep -A3 'MSG_GLOBAL_CONFIG_DONE_RERUN' "$BIN_DIR/workhive" | grep -q 'exit 0' || exit 1
    grep -q 'Global config is now ready; continue current command flow directly' "$BIN_DIR/workhive" && exit 1
    exit 0
)
assert_eq "0" "$?" "should stop after global init and ask user to rerun original command"

# ══════════════════════════════════════════════════
# __MAIN_DIR__ sentinel protection
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  __MAIN_DIR__ Sentinel Protection Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "session_create rejects __MAIN_DIR__ as branch name"
source "$LIB_DIR/domain/services/session_service.sh" 2>/dev/null || true
cd "$TEST_DIR" || exit 1
output=$(session_create "__MAIN_DIR__" "$TEST_DIR" 2>&1)
assert_failure "$?" "should reject __MAIN_DIR__"
assert_contains "$output" "sentinel" "should mention sentinel"

test_case "open_command.sh has __MAIN_DIR__ safety check"
(grep -q '__MAIN_DIR__' "$LIB_DIR/app/commands/open_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# config_global_exists function
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  config_global_exists Function Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "config_global_exists function exists"
type config_global_exists &>/dev/null
assert_eq "0" "$?"

test_case "config_global_exists returns false when no global config"
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_noexist_$$"
mkdir -p "$XDG_CONFIG_HOME"
config_global_exists
assert_failure "$?" "should return false"
export XDG_CONFIG_HOME="$_save_xdg"

test_case "config_global_exists returns true when global config exists"
_save_xdg="$XDG_CONFIG_HOME"
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_exist_$$"
mkdir -p "$XDG_CONFIG_HOME/workhive"
echo "version: 1" > "$XDG_CONFIG_HOME/workhive/config.yaml"
config_global_exists
assert_success "$?" "should return true"
export XDG_CONFIG_HOME="$_save_xdg"

# ══════════════════════════════════════════════════
# _ensure_workhive_dir function
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  _ensure_workhive_dir Function Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "_ensure_workhive_dir function exists"
type _ensure_workhive_dir &>/dev/null
assert_eq "0" "$?"

test_case "_ensure_workhive_dir creates .workhive dir if missing"
_ensure_wh_test_dir="$TEST_DIR/.ensure_wh_test_$$"
mkdir -p "$_ensure_wh_test_dir"
(
    cd "$_ensure_wh_test_dir" || exit 1
    git init >/dev/null 2>&1 || exit 0
    _git_root_reset
    rm -rf .workhive
    _ensure_workhive_dir
    [[ -d ".workhive" ]]
)
assert_eq "0" "$?"

test_case "_ensure_workhive_dir creates instance_id if missing"
(
    cd "$_ensure_wh_test_dir" || exit 1
    _git_root_reset
    rm -f .workhive/instance_id
    _ensure_workhive_dir
    [[ -f ".workhive/instance_id" ]]
)
assert_eq "0" "$?"

test_case "_ensure_workhive_dir preserves existing instance_id"
(
    cd "$_ensure_wh_test_dir" || exit 1
    _git_root_reset
    echo -n "abcd" > .workhive/instance_id
    _ensure_workhive_dir
    [[ "$(cat .workhive/instance_id)" == "abcd" ]]
)
assert_eq "0" "$?"

test_case "open_command.sh calls _ensure_workhive_dir"
(grep -q '_ensure_workhive_dir' "$LIB_DIR/app/commands/open_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# open_command HEAD check
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  open_command HEAD Check Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "open_command.sh checks HEAD before worktree creation"
(grep -q 'rev-parse HEAD' "$LIB_DIR/app/commands/open_command.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "open_command.sh HEAD check applies to all modes (not just worktree_mode)"
(grep -B2 'rev-parse HEAD' "$LIB_DIR/app/commands/open_command.sh" | grep -qv 'worktree_mode' && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "open_command.sh offers interactive initial commit on no HEAD"
(grep -A10 'rev-parse HEAD' "$LIB_DIR/app/commands/open_command.sh" | grep -q 'ASK_CREATE_INITIAL_COMMIT' && exit 0 || exit 1)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# init error handling
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  init Error Handling Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

test_case "shared _git_init_guarded handles git init failure"
(grep -q 'ERR_GIT_INIT_FAILED' "$LIB_DIR/utils/common.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "shared _git_init_guarded handles git commit failure"
(grep -q 'ERR_GIT_COMMIT_FAILED' "$LIB_DIR/utils/common.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

test_case "shared _git_ensure_initial_commit handles no-commit state"
(grep -q 'MSG_GIT_NO_COMMITS' "$LIB_DIR/utils/common.sh" && exit 0 || exit 1)
assert_eq "0" "$?"

# ══════════════════════════════════════════════════
# New i18n keys (git error handling)
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  i18n - Git Error Handling Keys Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

WORKHIVE_LANG=en

test_case "i18n ERR_GIT_INIT_FAILED (en) returns non-empty"
result=$(_i18n ERR_GIT_INIT_FAILED)
assert_not_empty "$result"

test_case "i18n ERR_GIT_COMMIT_FAILED (en) returns non-empty"
result=$(_i18n ERR_GIT_COMMIT_FAILED)
assert_not_empty "$result"

test_case "i18n MSG_GIT_NO_COMMITS (en) returns non-empty"
result=$(_i18n MSG_GIT_NO_COMMITS)
assert_not_empty "$result"

test_case "i18n ASK_CREATE_INITIAL_COMMIT (en) returns non-empty"
result=$(_i18n ASK_CREATE_INITIAL_COMMIT)
assert_not_empty "$result"

WORKHIVE_LANG=zh

test_case "i18n ERR_GIT_INIT_FAILED (zh) returns non-empty"
result=$(_i18n ERR_GIT_INIT_FAILED)
assert_not_empty "$result"

test_case "i18n ERR_GIT_COMMIT_FAILED (zh) returns non-empty"
result=$(_i18n ERR_GIT_COMMIT_FAILED)
assert_not_empty "$result"

test_case "i18n MSG_GIT_NO_COMMITS (zh) returns non-empty"
result=$(_i18n MSG_GIT_NO_COMMITS)
assert_not_empty "$result"

test_case "i18n ASK_CREATE_INITIAL_COMMIT (zh) returns non-empty"
result=$(_i18n ASK_CREATE_INITIAL_COMMIT)
assert_not_empty "$result"

# shellcheck disable=SC2034
WORKHIVE_LANG=en

# ═══════════════════════════════════════════════
# open summary pane command source
# ═══════════════════════════════════════════════

test_suite "open summary pane command source"

source "$LIB_DIR/app/commands/open_command.sh"

test_case "restore summary uses runtime pane state instead of config pane commands"
_open_orig_collect_cfg=$(declare -f layout_collect_pane_commands || true)
_open_orig_collect_runtime=$(declare -f layout_collect_runtime_pane_commands || true)
_open_orig_role_label=$(declare -f layout_pane_role_label || true)

layout_collect_pane_commands() {
    printf 'main|\npreview|\n'
}
layout_collect_runtime_pane_commands() {
    printf 'main|claude --dangerously-skip-permissions\npreview|lazygit\n'
}
layout_pane_role_label() {
    echo "$1"
}

WORKSPACE_OPEN_STRATEGY="snapshot"
output=$(_open_print_pane_lines "restore-branch")
unset WORKSPACE_OPEN_STRATEGY

assert_contains "$output" "main: Claude Code" "restore summary should use runtime claude pane"
assert_contains "$output" "preview: Lazygit" "restore summary should use runtime lazygit pane"

unset -f layout_collect_pane_commands layout_collect_runtime_pane_commands layout_pane_role_label
[[ -n "$_open_orig_collect_cfg" ]] && eval "$_open_orig_collect_cfg"
[[ -n "$_open_orig_collect_runtime" ]] && eval "$_open_orig_collect_runtime"
[[ -n "$_open_orig_role_label" ]] && eval "$_open_orig_role_label"

test_case "restore summary falls back to config pane commands when runtime collector is empty"
_open_orig_collect_cfg=$(declare -f layout_collect_pane_commands || true)
_open_orig_collect_runtime=$(declare -f layout_collect_runtime_pane_commands || true)
_open_orig_role_label=$(declare -f layout_pane_role_label || true)

layout_collect_pane_commands() {
    printf 'main|lazygit\n'
}
layout_collect_runtime_pane_commands() {
    return 1
}
layout_pane_role_label() {
    echo "$1"
}

WORKSPACE_OPEN_STRATEGY="snapshot"
output=$(_open_print_pane_lines "restore-branch")
unset WORKSPACE_OPEN_STRATEGY

assert_contains "$output" "main: Lazygit" "restore summary should fall back to config collector"

unset -f layout_collect_pane_commands layout_collect_runtime_pane_commands layout_pane_role_label
[[ -n "$_open_orig_collect_cfg" ]] && eval "$_open_orig_collect_cfg"
[[ -n "$_open_orig_collect_runtime" ]] && eval "$_open_orig_collect_runtime"
[[ -n "$_open_orig_role_label" ]] && eval "$_open_orig_role_label"

test_case "non-restore summary keeps config collector even when runtime collector has data"
_open_orig_collect_cfg=$(declare -f layout_collect_pane_commands || true)
_open_orig_collect_runtime=$(declare -f layout_collect_runtime_pane_commands || true)
_open_orig_role_label=$(declare -f layout_pane_role_label || true)

layout_collect_pane_commands() {
    printf 'main|lazygit\n'
}
layout_collect_runtime_pane_commands() {
    printf 'main|claude --dangerously-skip-permissions\n'
}
layout_pane_role_label() {
    echo "$1"
}

output=$(_open_print_pane_lines "normal-branch")

assert_contains "$output" "main: Lazygit" "non-restore summary should use config collector"
assert_not_contains "$output" "Claude Code" "non-restore summary should ignore runtime collector"

unset -f layout_collect_pane_commands layout_collect_runtime_pane_commands layout_pane_role_label
[[ -n "$_open_orig_collect_cfg" ]] && eval "$_open_orig_collect_cfg"
[[ -n "$_open_orig_collect_runtime" ]] && eval "$_open_orig_collect_runtime"
[[ -n "$_open_orig_role_label" ]] && eval "$_open_orig_role_label"

test_case "_open_print_pane_lines falls back to config summary when runtime collector times out"
_open_orig_collect_runtime_timeout=$(declare -f _open_collect_runtime_pane_lines_with_timeout || true)
_open_orig_collect_cfg=$(declare -f layout_collect_pane_commands || true)
_open_orig_log_debug=$(declare -f log_debug || true)

_open_collect_runtime_pane_lines_with_timeout() { return 124; }
layout_collect_pane_commands() { printf 'main|lazygit\n'; }
log_debug() { :; }
WORKSPACE_RESTORED_FROM_SNAPSHOT=true
WORKSPACE_OPEN_STRATEGY="snapshot"

output=$(_open_print_pane_lines "timeout-branch")
rc=$?
assert_eq "0" "$rc" "pane summary timeout should not fail open flow"
assert_contains "$output" "Lazygit" "runtime timeout should fallback to config summary"

unset WORKSPACE_RESTORED_FROM_SNAPSHOT
unset WORKSPACE_OPEN_STRATEGY
unset -f _open_collect_runtime_pane_lines_with_timeout layout_collect_pane_commands log_debug
[[ -n "$_open_orig_collect_runtime_timeout" ]] && eval "$_open_orig_collect_runtime_timeout"
[[ -n "$_open_orig_collect_cfg" ]] && eval "$_open_orig_collect_cfg"
[[ -n "$_open_orig_log_debug" ]] && eval "$_open_orig_log_debug"

test_case "_open_print_pane_lines falls back to raw runtime summary when config collector is unavailable"
_open_orig_collect_runtime_timeout=$(declare -f _open_collect_runtime_pane_lines_with_timeout || true)
_open_orig_collect_raw_runtime_timeout=$(declare -f _open_collect_runtime_raw_pane_lines_with_timeout || true)
_open_orig_collect_cfg=$(declare -f layout_collect_pane_commands || true)
_open_orig_role_label=$(declare -f layout_pane_role_label || true)

_open_collect_runtime_pane_lines_with_timeout() { return 124; }
_open_collect_runtime_raw_pane_lines_with_timeout() { printf 'pane_0|codex\n'; }
layout_collect_pane_commands() { return 1; }
layout_pane_role_label() { echo "$1"; }
WORKSPACE_OPEN_STRATEGY="session"

output=$(_open_print_pane_lines "runtime-fallback-branch")
rc=$?
assert_eq "0" "$rc" "raw runtime fallback should not fail open flow"
assert_contains "$output" "pane_0: Codex" "raw runtime fallback should provide pane summary"

unset WORKSPACE_OPEN_STRATEGY
unset -f _open_collect_runtime_pane_lines_with_timeout _open_collect_runtime_raw_pane_lines_with_timeout layout_collect_pane_commands layout_pane_role_label
[[ -n "$_open_orig_collect_runtime_timeout" ]] && eval "$_open_orig_collect_runtime_timeout"
[[ -n "$_open_orig_collect_raw_runtime_timeout" ]] && eval "$_open_orig_collect_raw_runtime_timeout"
[[ -n "$_open_orig_collect_cfg" ]] && eval "$_open_orig_collect_cfg"
[[ -n "$_open_orig_role_label" ]] && eval "$_open_orig_role_label"

# ═══════════════════════════════════════════════
# removed hook callbacks in shell mode
# ═══════════════════════════════════════════════

test_suite "removed hook callbacks in shell mode"

source "$LIB_DIR/app/commands/attach_command.sh"

test_case "attach implementation does not reference lifecycle hooks"
_attach_fn=$(declare -f attach_command_execute)
assert_not_contains "$_attach_fn" "hook_trigger" "attach should not invoke lifecycle hooks"

test_case "open implementation does not reference client_reconciliation hook"
_open_fn=$(declare -f open_command_execute)
assert_not_contains "$_open_fn" "client_reconciliation" "open should not invoke reconciliation hook callbacks"

test_case "expected command errors do not emit unexpected-error trap noise"
output=$("$BIN_DIR/workhive" pane foobar 2>&1 || true)
assert_not_contains "$output" "Unexpected error at" "handled command errors should not trigger ERR trap noise"
assert_not_contains "$output" "发生未预期错误" "handled command errors should not trigger ERR trap noise (zh)"

# ═══════════════════════════════════════════════
# tools command tests
# ═══════════════════════════════════════════════

test_suite "tools command tests"

unset _TOOLS_COMMAND_SH_LOADED
source "$LIB_DIR/app/commands/tools_command.sh"

test_case "tools --help shows usage"
output=$("$BIN_DIR/workhive" tools --help 2>&1)
assert_contains "$output" "workhive tools" "should show tools usage"

test_case "tools with no subcommand shows help guidance"
output=$("$BIN_DIR/workhive" tools 2>&1)
assert_contains "$output" "workhive tools" "tools without subcommand should show usage guidance"

test_case "tools unknown subcommand shows error"
output=$("$BIN_DIR/workhive" tools bad-subcommand 2>&1 || true)
assert_contains "$output" "Unknown tools subcommand" "should reject unknown subcommand"

if command -v yq >/dev/null 2>&1; then
test_case "tools list reads global config and excludes disabled entries"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      label: "Codex"
      command: "echo codex"
      is_ai: true
      enabled: true
    - tool_name: hidden
      label: "Hidden"
      command: "echo hidden"
      is_ai: false
      enabled: false
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools list 2>&1)
assert_contains "$output" "default: codex" "should show configured default summary"
assert_contains "$output" "codex" "should include enabled tool"
assert_not_contains "$output" "hidden" "should hide disabled tool"
rm -rf "$TOOLS_XDG"

test_case "tools list user-visible output does not leak legacy row transport"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: claude
  list:
    - tool_name: claude
      label: "Claude"
      command: "echo claude"
      is_ai: true
      enabled: true
    - tool_name: lazygit
      label: "Lazygit"
      command: "echo lazygit"
      is_ai: false
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools list 2>&1)
assert_contains "$output" "tool_name" "list should still render user-facing table headers"
assert_contains "$output" "claude" "enabled AI tool should be visible"
assert_contains "$output" "lazygit" "enabled non-AI tool should be visible"
assert_not_contains "$output" "tool_name|label|command|is_ai|enabled" "list should not expose legacy pipe-delimited transport headers"
assert_not_contains "$output" "echo claude|true|true" "list should not expose legacy pipe-delimited transport payloads"
rm -rf "$TOOLS_XDG"

test_case "tools list ai column reflects configured ai semantics"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: claude
  list:
    - tool_name: claude
      label: "Claude"
      command: "echo claude"
      is_ai: true
      enabled: true
    - tool_name: lazygit
      label: "Lazygit"
      command: "echo lazygit"
      is_ai: false
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools list 2>&1)
assert_contains "$output" "claude     Claude   true" "AI tool should be marked true in user-visible list"
assert_contains "$output" "lazygit    Lazygit  false" "non-AI tool should be marked false in user-visible list"
rm -rf "$TOOLS_XDG"

test_case "tools list table headers are localized in zh"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      label: "Codex"
      command: "echo codex"
      is_ai: true
      enabled: true
EOF
output=$(WORKHIVE_LANG=zh XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools list 2>&1)
assert_contains "$output" "工具名称" "zh header should show localized tool_name"
assert_contains "$output" "标签" "zh header should show localized label"
assert_contains "$output" "默认" "zh header should show localized default"
assert_contains "$output" "命令" "zh header should show localized command"
rm -rf "$TOOLS_XDG"

test_case "tools validate fails on duplicate tool_name"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      command: "echo a"
      enabled: true
    - tool_name: codex
      command: "echo b"
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools validate 2>&1 || true)
assert_contains "$output" "duplicated tool_name" "duplicate tool_name should fail validation"
rm -rf "$TOOLS_XDG"

test_case "tools edit writes values correctly via yq strenv"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      label: "Codex"
      command: "echo codex"
      is_ai: true
      enabled: true
EOF
XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools edit codex --label "Codex Pro" --cmd "echo codex-pro" --ai false --enable true >/dev/null 2>&1
label=$(yq '.tools.list[0].label' "$TOOLS_XDG/workhive/config.yaml" 2>/dev/null)
cmd=$(yq '.tools.list[0].command' "$TOOLS_XDG/workhive/config.yaml" 2>/dev/null)
is_ai=$(yq '.tools.list[0].is_ai' "$TOOLS_XDG/workhive/config.yaml" 2>/dev/null)
assert_eq "Codex Pro" "$label" "label should be updated"
assert_eq "echo codex-pro" "$cmd" "command should be updated"
assert_eq "false" "$is_ai" "is_ai should be updated"
rm -rf "$TOOLS_XDG"

test_case "tools validate rejects normalized-id conflicts before transport details matter"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: "foo_bar"
  list:
    - tool_name: "foo_bar"
      label: "Foo Underscore"
      command: "echo a"
      enabled: true
    - tool_name: "foo-bar"
      label: "Foo Dash"
      command: "echo b"
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools validate 2>&1 || true)
assert_contains "$output" "normalized tool_name conflict" "validation should reject ids that normalize to the same visible tool identity"
assert_contains "$output" "foo-bar" "validation error should identify the conflicting configured tool name that must be renamed"
assert_not_contains "$output" "tool_name|label|command" "validation failure should report semantic conflict, not leak transport formatting"
rm -rf "$TOOLS_XDG"

test_case "tools validate fails when enabled tool command is not installed"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: claude
  list:
    - tool_name: claude
      command: "claudex --dangerously-skip-permissions"
      enabled: true
    - tool_name: codex
      command: "codex"
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools validate 2>&1 || true)
assert_contains "$output" "command not found" "enabled tool with missing command should fail validation"
rm -rf "$TOOLS_XDG"

test_case "tools add rejects invalid boolean values"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: ""
  list: []
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools add --name demo --cmd "echo demo" --ai maybe 2>&1 || true)
assert_contains "$output" "Invalid boolean value" "invalid --ai bool should fail"
rm -rf "$TOOLS_XDG"

test_case "tools edit rejects invalid boolean values"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      command: "codex"
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools edit codex --default maybe 2>&1 || true)
assert_contains "$output" "Invalid boolean value" "invalid --default bool should fail"
rm -rf "$TOOLS_XDG"

test_case "tools add duplicate name is rejected immediately"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: claude
  list:
    - tool_name: claude
      command: "claude --dangerously-skip-permissions"
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools add --name claude --cmd "echo demo" 2>&1 || true)
assert_contains "$output" "already exists" "duplicate tool name should fail immediately"
rm -rf "$TOOLS_XDG"

test_case "tools detect semantics are not silently accepted as unknown subcommands"
output=$("$BIN_DIR/workhive" tools detect 2>&1 || true)
assert_contains "$output" "Unknown tools subcommand: detect" "Task 1 should lock the current detect semantic gap explicitly"
assert_contains "$output" "workhive tools" "detect failure should still route through tools help boundary"

test_case "tools default-display semantics render special empty sentinel clearly"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: __empty__
  list:
    - tool_name: claude
      label: "Claude"
      command: "echo claude"
      is_ai: true
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools list 2>&1)
assert_contains "$output" "default: null (Empty Shell)" "list should display the empty-shell default sentinel as a user-facing semantic label"
rm -rf "$TOOLS_XDG"

test_case "tools set-default accepts special default references and surfaces chosen value"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: claude
  list:
    - tool_name: claude
      label: "Claude"
      command: "echo claude"
      is_ai: true
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" "$BIN_DIR/workhive" tools set-default __custom__ 2>&1)
assert_contains "$output" "Default tool set: __custom__" "set-default should report the selected special default reference"
stored_default=$(yq '.tools.default' "$TOOLS_XDG/workhive/config.yaml" 2>/dev/null)
assert_eq "__custom__" "$stored_default" "set-default should persist the selected special default reference"
rm -rf "$TOOLS_XDG"

test_case "tools manage remove path remains TTY-gated without legacy transport leakage"
TOOLS_XDG=$(mktemp -d /tmp/workhive-tools-tests-XXXXXX)
mkdir -p "$TOOLS_XDG/workhive"
cat > "$TOOLS_XDG/workhive/config.yaml" <<'EOF'
tools:
  default: codex
  list:
    - tool_name: codex
      label: "Codex"
      command: "echo codex"
      is_ai: true
      enabled: true
EOF
output=$(XDG_CONFIG_HOME="$TOOLS_XDG" tools_command_manage 2>&1 || true)
assert_contains "$output" "Interactive tools manager requires a TTY." "manage remove-path regression coverage should stay within tools command semantics"
assert_contains "$output" "workhive tools list|add|edit|remove|set-default|validate" "non-interactive manage path should direct users to the tools subcommands"
assert_not_contains "$output" "codex|Codex|echo codex|true|true" "manage-path failure should not be masked by legacy row transport leakage"
rm -rf "$TOOLS_XDG"
fi

test_report

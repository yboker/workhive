#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/domain/services/session_service.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/adapters/terminal/factory.sh"
source "$LIB_DIR/infra/config/loader.sh"

# Temporary directory for testing (initialize git repo to ensure _git_root works correctly)
TEST_DIR="/tmp/workhive-test-session-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
git init -q .
mkdir -p .workhive
echo 'version: "0.2.0"' > .workhive/config.yaml
export _WORKHIVE_GIT_ROOT=""
# Isolate global config to prevent user-level config from affecting test results
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

# Initialize adapter
terminal_adapter_init >/dev/null 2>&1

source "$LIB_DIR/domain/services/session_service.sh"

TEST_BRANCH="unit-session-$$"
TEST_SESSION=""

cleanup() {
    if [[ -n "$TEST_SESSION" ]]; then
        tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
    fi
    cd / || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "lib/domain/services/session_service.sh"

# ──────────────────────────────────────────────────
# source-time bootstrap regression (_internal_save)
# ──────────────────────────────────────────────────

test_case "session_service source skips adapter init for _internal_save"
(
    set -uo pipefail
    export LIB_DIR="$ROOT_DIR/lib"

    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"

    _marker="/tmp/workhive-test-internal-save-init-$$"
    rm -f "$_marker"

    terminal_adapter_init() {
        : > "$_marker"
    }

    export COMMAND="_internal_save"
    source "$LIB_DIR/domain/services/session_service.sh"

    [[ ! -f "$_marker" ]]
)
assert_eq "0" "$?" "adapter init must be skipped in _internal_save path"

test_case "session_service source skips adapter init for _internal_save_runtime_debounced"
(
    set -uo pipefail
    export LIB_DIR="$ROOT_DIR/lib"

    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"

    _marker="/tmp/workhive-test-internal-save-debounce-init-$$"
    rm -f "$_marker"

    terminal_adapter_init() {
        : > "$_marker"
    }

    export COMMAND="_internal_save_runtime_debounced"
    source "$LIB_DIR/domain/services/session_service.sh"

    [[ ! -f "$_marker" ]]
)
assert_eq "0" "$?" "adapter init must be skipped in _internal_save_runtime_debounced path"

# ──────────────────────────────────────────────────
# Lazy adapter bootstrap: sourcing does NOT call terminal_adapter_init
# ──────────────────────────────────────────────────

test_case "session_service source does NOT call terminal_adapter_init for normal commands"
_marker="/tmp/workhive-test-lazy-boot-$$"
rm -f "$_marker"
bash -c '
    set -uo pipefail
    export LIB_DIR="'"$ROOT_DIR"'/lib"
    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"
    terminal_adapter_init() { : > "'"$_marker"'"; }
    export COMMAND="status"
    source "$LIB_DIR/domain/services/session_service.sh"
    [[ ! -f "'"$_marker"'" ]]
'
assert_eq "0" "$?" "sourcing session_service must not call terminal_adapter_init at load time"
rm -f "$_marker"

test_case "pure naming helpers work without adapter init"
bash -c '
    set -uo pipefail
    export LIB_DIR="'"$ROOT_DIR"'/lib"
    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"
    cd "'"$TEST_DIR"'" || exit 1
    terminal_adapter_init() { echo "SHOULD NOT BE CALLED" >&2; return 1; }
    export COMMAND="status"
    source "$LIB_DIR/domain/services/session_service.sh"
    session_get_prefix >/dev/null && session_get_name "test-branch" >/dev/null && session_extract_branch "wh@proj@test-branch" >/dev/null
'
assert_eq "0" "$?" "naming helpers must work without adapter bootstrap"

test_case "_session_ensure_terminal_adapter helper exists"
bash -c '
    set -uo pipefail
    export LIB_DIR="'"$ROOT_DIR"'/lib"
    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"
    export COMMAND="status"
    source "$LIB_DIR/domain/services/session_service.sh"
    declare -f _session_ensure_terminal_adapter >/dev/null
'
assert_eq "0" "$?" "_session_ensure_terminal_adapter must be defined"

test_case "session_get_info_by_name helper exists"
bash -c '
    set -uo pipefail
    export LIB_DIR="'"$ROOT_DIR"'/lib"
    source "$LIB_DIR/utils/common.sh"
    source "$LIB_DIR/infra/cache/manager.sh"
    source "$LIB_DIR/infra/platform/detector.sh"
    source "$LIB_DIR/infra/adapters/terminal/factory.sh"
    source "$LIB_DIR/infra/config/loader.sh"
    export COMMAND="status"
    source "$LIB_DIR/domain/services/session_service.sh"
    declare -f session_get_info_by_name >/dev/null
'
assert_eq "0" "$?" "session_get_info_by_name must be defined after source"

# ──────────────────────────────────────────────────
# session_get_prefix
# ──────────────────────────────────────────────────

test_case "session_get_prefix returns wh@<project> when config has no session_prefix"
cache_clear
export _WORKHIVE_GIT_ROOT=""
# Ensure config has no tmux.session_prefix (or null) — defaults apply
result=$(session_get_prefix)
expected_prefix="wh@$(basename "$TEST_DIR")"
assert_eq "$expected_prefix" "$result"

test_case "session_get_prefix uses config when set"
cache_clear
# Set tmux.session_prefix in project config — replaces "wh" tag only
if command -v yq &>/dev/null; then
    config_set "tmux.session_prefix" "custom-prefix" >/dev/null 2>&1
    result=$(session_get_prefix)
    expected_prefix="custom-prefix@$(basename "$TEST_DIR")"
    assert_eq "$expected_prefix" "$result" "should return config value when set"
    # Restore minimal config for later tests
    echo 'version: "0.2.0"' > .workhive/config.yaml
    cache_invalidate "config:*" 2>/dev/null || true
else
    skip_test "yq not installed"
fi

test_case "session_get_prefix strips shell metacharacters from configured prefix"
cache_clear
if command -v yq &>/dev/null; then
    config_set "tmux.session_prefix" "bad'prefix;rm -rf /" >/dev/null 2>&1
    result=$(session_get_prefix)
    assert_not_contains "$result" "'" "should not contain single quotes"
    assert_not_contains "$result" ";" "should not contain shell separators"
    assert_not_contains "$result" " " "should not contain spaces"
    assert_contains "$result" "badprefixrm-rf" "should preserve safe alphanumeric content"
    echo 'version: "0.2.0"' > .workhive/config.yaml
    cache_invalidate "config:*" 2>/dev/null || true
else
    skip_test "yq not installed"
fi

# ──────────────────────────────────────────────────
# session_get_name
# ──────────────────────────────────────────────────

test_case "session_get_name generates wh@<project>@<branch> format"
export _WORKHIVE_GIT_ROOT=""
result=$(session_get_name "feature-x")
expected_prefix="wh@$(basename "$TEST_DIR")"
assert_eq "${expected_prefix}@feature-x" "$result"

test_case "session_get_name includes wh@ prefix"
result=$(session_get_name "test")
expected_prefix="wh@$(basename "$TEST_DIR")"
assert_contains "$result" "${expected_prefix}@"

# ──────────────────────────────────────────────────
# tmux.env startup command injection
# ──────────────────────────────────────────────────

test_case "_session_get_tmux_env_prefix escapes values and filters invalid keys"
(
    config_get_tmux_env_entries() {
        cat <<'EOF'
CLAUDE_CODE_NO_FLICKER=1
SPACED=hello world
BAD-KEY=ignored
EOF
    }
    _SESSION_TMUX_ENV_PREFIX_CACHED=""
    result=$(_session_get_tmux_env_prefix)
    assert_contains "$result" "CLAUDE_CODE_NO_FLICKER=1 " "should include valid scalar env key"
    assert_contains "$result" "SPACED=hello\\ world " "should shell-escape values"
    assert_not_contains "$result" "BAD-KEY" "invalid key should be ignored at runtime"
)
assert_eq "0" "$?" "_session_get_tmux_env_prefix should build safe env assignment prefix"

test_case "session_send_startup_command prepends tmux.env before adapter spawn"
(
    _session_ensure_terminal_adapter() { return 0; }
    session_require_exists() {
        SESSION_NAME="wh@test@main"
        return 0
    }
    config_get_tmux_env_entries() {
        cat <<'EOF'
CLAUDE_CODE_NO_FLICKER=1
FOO=bar baz
EOF
    }
    terminal_adapter_spawn_command_target() {
        local target="$1"
        local command="$2"
        local workdir="$3"
        [[ "$target" == "wh@test@main:0.0" ]] || return 1
        [[ "$workdir" == "/tmp/workdir" ]] || return 1
        [[ "$command" == "CLAUDE_CODE_NO_FLICKER=1 FOO=bar\\ baz claude" ]] || return 1
        return 0
    }

    _SESSION_TMUX_ENV_PREFIX_CACHED=""
    session_send_startup_command "main" "0" "claude" "/tmp/workdir"
)
assert_eq "0" "$?" "startup command should include tmux.env assignments before command"

# ──────────────────────────────────────────────────
# session_exists
# ──────────────────────────────────────────────────

test_case "session_exists returns 1 for non-existent"
cache_clear
rc=0
session_exists "nonexistent-session-$$" || rc=$?
assert_eq "1" "${rc:-0}"

# ──────────────────────────────────────────────────
# session_create (requires tmux)
# ──────────────────────────────────────────────────

if tmux_usable; then
    test_case "session_create creates session"
    cache_clear
    session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
    TEST_SESSION=$(session_get_name "$TEST_BRANCH")
    assert_eq "0" "$?"

    test_case "session_exists returns 0 after create"
    cache_clear
    session_exists "$TEST_SESSION"
    assert_eq "0" "$?"

    test_case "session_create is idempotent"
    session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
    assert_eq "0" "$?" "should succeed for existing session"

    test_case "session_create fails for empty branch"
    rc=0
    session_create "" "/tmp" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    test_case "session_create fails for non-existent workdir"
    rc=0
    session_create "bad-dir-$$" "/nonexistent/dir/xyz" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    # ──────────────────────────────────────────────────
    # session_validate
    # ──────────────────────────────────────────────────

    test_case "session_validate passes for active session"
    cache_clear
    session_validate "$TEST_BRANCH" >/dev/null 2>&1
    assert_eq "0" "$?"

    test_case "session_validate fails for non-existent"
    rc=0
    session_validate "nonexistent-$$" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    # ──────────────────────────────────────────────────
    # session_get_info
    # ──────────────────────────────────────────────────

    test_case "session_get_info returns JSON for existing session"
    result=$(session_get_info "$TEST_BRANCH")
    assert_contains "$result" '"exists": true'
    assert_contains "$result" '"name":'

    test_case "session_get_info returns false for non-existent"
    result=$(session_get_info "nonexistent-$$" 2>/dev/null || true)
    assert_contains "$result" '"exists": false'

    # ──────────────────────────────────────────────────
    # session_send_command
    # ──────────────────────────────────────────────────

    test_case "session_send_command succeeds for active session"
    session_send_command "$TEST_BRANCH" "0" "echo test" >/dev/null 2>&1
    assert_eq "0" "$?"

    test_case "session_send_command fails for non-existent"
    rc=0
    session_send_command "nonexistent-$$" "0" "echo test" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    # ──────────────────────────────────────────────────
    # session_list_all
    # ──────────────────────────────────────────────────

    test_case "session_list_all includes created session"
    cache_clear
    result=$(session_list_all)
    assert_contains "$result" "$TEST_SESSION"

    # ──────────────────────────────────────────────────
    # session_apply_config
    # ──────────────────────────────────────────────────

    test_case "session_apply_config succeeds"
    session_apply_config "$TEST_SESSION" >/dev/null 2>&1
    assert_eq "0" "$?"

    # ──────────────────────────────────────────────────
    # session_kill
    # ──────────────────────────────────────────────────

    test_case "session_kill closes session"
    cache_clear
    unset -f persistence_save_before_kill 2>/dev/null || true
    session_kill "$TEST_BRANCH" >/dev/null 2>&1
    assert_eq "0" "$?"

    test_case "session_kill lazy-loads persistence manager helper"
    declare -f persistence_save_before_kill >/dev/null 2>&1
    assert_eq "0" "$?" "persistence_save_before_kill should be defined after session_kill"

    test_case "session_exists returns 1 after kill"
    cache_clear
    rc=0
    session_exists "$TEST_SESSION" || rc=$?
    assert_eq "1" "${rc:-0}"

    test_case "session_kill warns for non-existent"
    rc=0
    session_kill "nonexistent-$$" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    test_case "session_kill_by_name closes existing session directly"
    cache_clear
    session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
    TEST_SESSION=$(session_get_name "$TEST_BRANCH")
    session_kill_by_name "$TEST_SESSION" "$TEST_BRANCH" >/dev/null 2>&1
    assert_eq "0" "$?"

    test_case "session_kill_by_name returns 1 for non-existent full session"
    rc=0
    session_kill_by_name "wh@fake@project@nonexistent-$$" "nonexistent-$$" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}"

    TEST_SESSION=""  # Already closed

    # ──────────────────────────────────────────────────
    # session_extract_branch -- extract branch from session name
    # ──────────────────────────────────────────────────

    test_case "session_extract_branch extracts branch from full session name"
    result=$(session_extract_branch "wh@c161@my-project@feature-x")
    assert_eq "feature-x" "$result"

    test_case "session_extract_branch handles branch with hyphens"
    result=$(session_extract_branch "wh@c161@workhive-test@test-demo1")
    assert_eq "test-demo1" "$result"

    test_case "session_extract_branch handles session without instance_id"
    result=$(session_extract_branch "wh@my-project@my-branch")
    assert_eq "my-branch" "$result"

    test_case "session_extract_branch handles project name with hyphens"
    result=$(session_extract_branch "wh@abcd@my-cool-project@fix-bug")
    assert_eq "fix-bug" "$result"

    # ──────────────────────────────────────────────────
    # session_get_name sanitization (P4)
    # ──────────────────────────────────────────────────

    test_case "session_get_name maps / to - for hierarchical branches"
    _name_with_slash=$(session_get_name "feature/foo")
    assert_contains "$_name_with_slash" "feature-foo" "should map / to -"

    test_case "session_get_name preserves normal branch names"
    _name_normal=$(session_get_name "my-branch")
    assert_contains "$_name_normal" "my-branch" "should preserve normal name"

    # ──────────────────────────────────────────────────
    # session_list_project — project-scoped session listing
    # ──────────────────────────────────────────────────

    test_case "session_list_project function exists"
    type session_list_project &>/dev/null
    assert_eq "0" "$?" "session_list_project should be defined"

    # ──────────────────────────────────────────────────
    # Cross-project prefix isolation
    # ──────────────────────────────────────────────────

    test_case "session_get_prefix uses different prefix in different git repos"
    _other_dir="/tmp/workhive-test-other-$$"
    mkdir -p "$_other_dir"
    (cd "$_other_dir" && git init -q .)
    _other_prefix=$(cd "$_other_dir" && export _WORKHIVE_GIT_ROOT="" && session_get_prefix)
    _this_prefix=$(export _WORKHIVE_GIT_ROOT="" && session_get_prefix)
    if [[ "$_other_prefix" != "$_this_prefix" ]]; then rc=0; else rc=1; fi
    assert_eq "0" "$rc" "different repos should have wh@<project1> vs wh@<project2>"
    rm -rf "$_other_dir"

    test_case "session_get_name avoids cross-project collisions"
    export _WORKHIVE_GIT_ROOT=""
    _name1=$(session_get_name "demo01")
    _other_dir2="/tmp/workhive-test-proj2-$$"
    mkdir -p "$_other_dir2"
    (cd "$_other_dir2" && git init -q .)
    _name2=$(cd "$_other_dir2" && export _WORKHIVE_GIT_ROOT="" && session_get_name "demo01")
    if [[ "$_name1" != "$_name2" ]]; then rc=0; else rc=1; fi
    assert_eq "0" "$rc" "same branch in different projects should have wh@<proj1>@xxx vs wh@<proj2>@xxx"
    rm -rf "$_other_dir2"

    # ──────────────────────────────────────────────────
    # session_require_exists
    # ──────────────────────────────────────────────────

    # Recreate session for session_require_exists tests (killed earlier)
    cache_clear
    session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
    TEST_SESSION=$(session_get_name "$TEST_BRANCH")

    test_case "session_require_exists returns 0 for existing session"
    SESSION_NAME=""
    session_require_exists "$TEST_BRANCH" >/dev/null 2>&1
    assert_eq "0" "$?" "should return 0 for existing session"

    test_case "session_require_exists sets SESSION_NAME"
    expected_name=$(session_get_name "$TEST_BRANCH")
    assert_eq "$expected_name" "$SESSION_NAME" "SESSION_NAME should be set"

    test_case "session_require_exists returns 1 for non-existent session"
    session_require_exists "nonexistent-branch-$$" >/dev/null 2>&1
    assert_eq "1" "$?" "should return 1 for non-existent session"

    # ──────────────────────────────────────────────────
    # session_get_project_name — basename + subpath
    # ──────────────────────────────────────────────────

    test_case "session_get_project_name returns basename at git root"
    export _WORKHIVE_GIT_ROOT=""
    _proj=$(session_get_project_name)
    assert_eq "$(basename "$TEST_DIR")" "$_proj" "should return basename when at git root"

    test_case "session_get_project_name appends subpath in subdirectory"
    mkdir -p "$TEST_DIR/sub/deep"
    export _WORKHIVE_GIT_ROOT=""
    _proj=$(cd "$TEST_DIR/sub/deep" && export _WORKHIVE_GIT_ROOT="" && session_get_project_name)
    assert_eq "$(basename "$TEST_DIR")/sub/deep" "$_proj" "should append subpath"
    rm -rf "$TEST_DIR/sub"

    test_case "session_get_project_name no trailing slash at root"
    export _WORKHIVE_GIT_ROOT=""
    _proj=$(session_get_project_name)
    assert_not_contains "$_proj" "/" "should have no / when at git root"

    # ──────────────────────────────────────────────────
    # session_extract_branch with / in project name
    # ──────────────────────────────────────────────────

    test_case "session_extract_branch works when project contains /"
    result=$(session_extract_branch "wh@0f2f@python/chedui_excel@feature-login")
    assert_eq "feature-login" "$result" "should extract branch after last @"

    test_case "session_extract_branch works with deep project path"
    result=$(session_extract_branch "wh@0f2f@python/a/b/c@main")
    assert_eq "main" "$result"

    # ──────────────────────────────────────────────────
    # _sanitize_project_name
    # ──────────────────────────────────────────────────

    test_case "_sanitize_project_name preserves /"
    result=$(_sanitize_project_name "a/b/c")
    assert_eq "a/b/c" "$result" "should keep / intact"

    test_case "_sanitize_project_name replaces . and : but keeps /"
    result=$(_sanitize_project_name "my.project:v2/sub/dir")
    assert_eq "my-project-v2/sub/dir" "$result"

    # ──────────────────────────────────────────────────
    # session_get_instance_id
    # ──────────────────────────────────────────────────

    test_case "session_get_instance_id returns empty when no instance_id file"
    _inst_id=$(session_get_instance_id)
    # TEST_DIR has .workhive but no instance_id file
    assert_empty "$_inst_id" "should be empty without instance_id file"

    test_case "session_get_instance_id returns value when file exists"
    echo "a1b2" > "$TEST_DIR/.workhive/instance_id"
    _inst_id=$(session_get_instance_id)
    assert_eq "a1b2" "$_inst_id" "should return stored id"
    rm -f "$TEST_DIR/.workhive/instance_id"

    # ──────────────────────────────────────────────────
    # session_create rejects __MAIN_DIR__ sentinel
    # ──────────────────────────────────────────────────

    test_case "session_create rejects __MAIN_DIR__ sentinel"
    rc=0
    session_create "__MAIN_DIR__" "/tmp" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}" "should reject sentinel value"

    # ──────────────────────────────────────────────────
    # session_create_window
    # ──────────────────────────────────────────────────

    test_case "session_create_window function exists"
    type session_create_window &>/dev/null
    assert_eq "0" "$?" "session_create_window should be defined"

    # ──────────────────────────────────────────────────
    # session_rename_window
    # ──────────────────────────────────────────────────

    test_case "session_rename_window function exists"
    type session_rename_window &>/dev/null
    assert_eq "0" "$?" "session_rename_window should be defined"

    # ──────────────────────────────────────────────────
    # session_select_window
    # ──────────────────────────────────────────────────

    test_case "session_select_window function exists"
    type session_select_window &>/dev/null
    assert_eq "0" "$?" "session_select_window should be defined"

    # ──────────────────────────────────────────────────
    # session_send_command_to_window
    # ──────────────────────────────────────────────────

    test_case "session_send_command_to_window function exists"
    type session_send_command_to_window &>/dev/null
    assert_eq "0" "$?" "session_send_command_to_window should be defined"

    # ──────────────────────────────────────────────────
    # session_detach
    # ──────────────────────────────────────────────────

    test_case "session_detach function exists"
    type session_detach &>/dev/null
    assert_eq "0" "$?" "session_detach should be defined"

    # ──────────────────────────────────────────────────
    # session_notify
    # ──────────────────────────────────────────────────

    test_case "session_notify fails for non-existent session"
    rc=0
    session_notify "nonexistent-$$" "hello" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}" "should fail for non-existent"

    # ──────────────────────────────────────────────────
    # _sanitize_session_prefix_tag
    # ──────────────────────────────────────────────────

    test_case "_sanitize_session_prefix_tag defaults to wh for empty"
    result=$(_sanitize_session_prefix_tag "")
    assert_eq "wh" "$result" "empty tag should default to wh"

    test_case "_sanitize_session_prefix_tag strips special chars"
    result=$(_sanitize_session_prefix_tag "bad;tag!")
    assert_eq "badtag" "$result" "should strip special characters"

    test_case "_sanitize_session_prefix_tag preserves valid tag"
    result=$(_sanitize_session_prefix_tag "my-tag_1")
    assert_eq "my-tag_1" "$result" "should preserve valid characters"

    # ──────────────────────────────────────────────────
    # session_create_window with live tmux session
    # ──────────────────────────────────────────────────

    test_case "session_create_window creates window in existing session"
    cache_clear
    session_create "$TEST_BRANCH" "/tmp" >/dev/null 2>&1
    TEST_SESSION=$(session_get_name "$TEST_BRANCH")
    session_create_window "$TEST_SESSION" "test-window" "/tmp" >/dev/null 2>&1
    assert_eq "0" "$?" "should succeed for existing session"

    test_case "session_create_window fails for non-existent session"
    rc=0
    session_create_window "nonexistent-session-$$" "win" "/tmp" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}" "should fail for non-existent session"

    # ──────────────────────────────────────────────────
    # session_rename_window with live tmux session
    # ──────────────────────────────────────────────────

    test_case "session_rename_window succeeds silently"
    session_rename_window "$TEST_SESSION" "0" "renamed" >/dev/null 2>&1
    assert_eq "0" "$?"

    # ──────────────────────────────────────────────────
    # session_select_window with live tmux session
    # ──────────────────────────────────────────────────

    test_case "session_select_window succeeds silently"
    session_select_window "$TEST_SESSION" "0" >/dev/null 2>&1
    assert_eq "0" "$?"

    # ──────────────────────────────────────────────────
    # session_send_command_to_window with live tmux session
    # ──────────────────────────────────────────────────

    test_case "session_send_command_to_window succeeds for valid session"
    session_send_command_to_window "$TEST_SESSION" "0" "0" "echo test" >/dev/null 2>&1
    assert_eq "0" "$?"

    # ──────────────────────────────────────────────────
    # session_notify with live tmux session
    # ──────────────────────────────────────────────────

    test_case "session_notify succeeds for active session"
    cache_clear
    session_notify "$TEST_BRANCH" "hello world" >/dev/null 2>&1
    assert_eq "0" "$?"

    # ──────────────────────────────────────────────────
    # session_attach (requires a non-tmux environment)
    # ──────────────────────────────────────────────────

    test_case "session_attach fails for non-existent session"
    rc=0
    session_attach "nonexistent-session-$$" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}" "should fail for non-existent session"

    # Cleanup test session
    cache_clear
    tmux kill-session -t "$TEST_SESSION" 2>/dev/null || true
    TEST_SESSION=""

else
    test_case "session tests (tmux not available - skip)"
    skip_test "tmux not available or unusable"
fi

# ──────────────────────────────────────────────────
# attach outcome logging contract
# ──────────────────────────────────────────────────

test_case "_session_log_attach_outcome returns 0 and warns on ended status"
(
    _warn_called=0
    _debug_called=0
    terminal_adapter_get_last_attach_status() { echo "ended"; }
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }

    _session_log_attach_outcome "wh@test@ended" "0"
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$_warn_called" -eq 1 ]] || exit 1
    [[ "$_debug_called" -eq 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "ended status should preserve success rc and emit warn log"

test_case "_session_log_attach_outcome returns failure and debug on error status"
(
    _warn_called=0
    _debug_called=0
    terminal_adapter_get_last_attach_status() { echo "error"; }
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }

    _session_log_attach_outcome "wh@test@error" "1"
    rc=$?
    [[ "$rc" -eq 1 ]] || exit 1
    [[ "$_warn_called" -eq 0 ]] || exit 1
    [[ "$_debug_called" -eq 1 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "error status should preserve failure rc and emit debug log"

test_case "session_attach branch path keeps ended-status success semantics"
(
    _session_ensure_terminal_adapter() { return 0; }
    session_require_exists() {
        SESSION_NAME="wh@test@main"
        return 0
    }
    terminal_adapter_attach_session() { return 0; }
    terminal_adapter_get_last_attach_status() { echo "ended"; }
    _warn_called=0
    _debug_called=0
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }
    log_info() { :; }

    session_attach "main" >/dev/null 2>&1
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$_warn_called" -eq 1 ]] || exit 1
    [[ "$_debug_called" -eq 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_attach should return success and warn when attach status is ended"

test_case "session_attach branch path propagates error-status failure semantics"
(
    _session_ensure_terminal_adapter() { return 0; }
    session_require_exists() {
        SESSION_NAME="wh@test@main"
        return 0
    }
    terminal_adapter_attach_session() { return 1; }
    terminal_adapter_get_last_attach_status() { echo "error"; }
    _warn_called=0
    _debug_called=0
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }
    log_info() { :; }

    session_attach "main" >/dev/null 2>&1 || rc=$?
    [[ "${rc:-0}" -eq 1 ]] || exit 1
    [[ "$_warn_called" -eq 0 ]] || exit 1
    [[ "$_debug_called" -eq 1 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_attach should propagate failure and debug-log on error status"

test_case "session_attach_by_name keeps ended-status success semantics"
(
    _session_ensure_terminal_adapter() { return 0; }
    session_exists() { return 0; }
    terminal_adapter_attach_session() { return 0; }
    terminal_adapter_get_last_attach_status() { echo "ended"; }
    _warn_called=0
    _debug_called=0
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }
    log_info() { :; }
    log_error() { :; }

    session_attach_by_name "wh@test@main" >/dev/null 2>&1
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$_warn_called" -eq 1 ]] || exit 1
    [[ "$_debug_called" -eq 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_attach_by_name should return success and warn when attach status is ended"

test_case "session_attach_by_name propagates error-status failure semantics"
(
    _session_ensure_terminal_adapter() { return 0; }
    session_exists() { return 0; }
    terminal_adapter_attach_session() { return 1; }
    terminal_adapter_get_last_attach_status() { echo "error"; }
    _warn_called=0
    _debug_called=0
    log_warn() { _warn_called=1; }
    log_debug() { _debug_called=1; }
    log_info() { :; }
    log_error() { :; }

    session_attach_by_name "wh@test@main" >/dev/null 2>&1 || rc=$?
    [[ "${rc:-0}" -eq 1 ]] || exit 1
    [[ "$_warn_called" -eq 0 ]] || exit 1
    [[ "$_debug_called" -eq 1 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_attach_by_name should propagate failure and debug-log on error status"

test_case "session_kill_by_name waits until session is actually gone before success"
(
    _session_ensure_terminal_adapter() { return 0; }
    terminal_adapter_get_current_session() { printf '\n'; }
    persistence_save_before_kill() { return 0; }
    terminal_adapter_kill_session() { return 0; }

    _has_session_calls=0
    terminal_adapter_has_session() {
        ((_has_session_calls += 1))
        # 1st: pre-check (exists), 2nd-3rd: waiting, 4th: gone
        if (( _has_session_calls < 4 )); then
            return 0
        fi
        return 1
    }

    _op_ok_called=0
    _op_fail_called=0
    op_begin() { :; }
    op_ok() { _op_ok_called=1; }
    op_fail() { _op_fail_called=1; }
    log_info() { :; }
    log_warn() { :; }
    log_error() { :; }
    log_success() { :; }
    cache_invalidate() { :; }

    WORKHIVE_SESSION_KILL_WAIT_MAX_ATTEMPTS=6
    WORKHIVE_SESSION_KILL_WAIT_INTERVAL_SEC=0
    session_kill_by_name "wh@test@main" "main" >/dev/null 2>&1
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$_op_ok_called" -eq 1 ]] || exit 1
    [[ "$_op_fail_called" -eq 0 ]] || exit 1
    [[ "$_has_session_calls" -ge 4 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_kill_by_name should confirm disappearance before reporting success"

test_case "session_kill_by_name fails when session does not disappear in time"
(
    _session_ensure_terminal_adapter() { return 0; }
    terminal_adapter_get_current_session() { printf '\n'; }
    persistence_save_before_kill() { return 0; }
    terminal_adapter_kill_session() { return 0; }
    terminal_adapter_has_session() { return 0; }

    _op_ok_called=0
    _op_fail_called=0
    op_begin() { :; }
    op_ok() { _op_ok_called=1; }
    op_fail() { _op_fail_called=1; }
    log_info() { :; }
    log_warn() { :; }
    log_error() { :; }
    log_success() { :; }
    cache_invalidate() { :; }

    WORKHIVE_SESSION_KILL_WAIT_MAX_ATTEMPTS=2
    WORKHIVE_SESSION_KILL_WAIT_INTERVAL_SEC=0
    rc=0
    session_kill_by_name "wh@test@main" "main" >/dev/null 2>&1 || rc=$?
    [[ "${rc:-0}" -eq 1 ]] || exit 1
    [[ "$_op_ok_called" -eq 0 ]] || exit 1
    [[ "$_op_fail_called" -eq 1 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_kill_by_name should fail fast when kill completion times out"

test_case "_session_pre_kill_snapshot_best_effort uses configured pre-kill env assignments"
(
    config_get_persistence_pre_kill_env_entries() {
        cat <<'EOF'
WORKHIVE_SAVE_LOCK_QUIET=1
WORKHIVE_SAVE_QUALITY_QUIET=1
WORKHIVE_SAVE_QUALITY_MAX_RETRIES=2
EOF
    }
    persistence_save_before_kill() {
        [[ "${WORKHIVE_SAVE_LOCK_QUIET:-}" == "1" ]] || return 1
        [[ "${WORKHIVE_SAVE_QUALITY_QUIET:-}" == "1" ]] || return 1
        [[ "${WORKHIVE_SAVE_QUALITY_MAX_RETRIES:-}" == "2" ]] || return 1
        return 0
    }

    _session_pre_kill_snapshot_best_effort "main"
)
assert_eq "0" "$?" "_session_pre_kill_snapshot_best_effort should source env assignments from config"

test_case "_session_pre_kill_snapshot_best_effort clears inherited managed env when config omits them"
(
    export WORKHIVE_SAVE_LOCK_QUIET=1
    export WORKHIVE_SAVE_QUALITY_QUIET=1
    export WORKHIVE_SAVE_QUALITY_MAX_RETRIES=9
    config_get_persistence_pre_kill_env_entries() { return 0; }
    persistence_save_before_kill() {
        [[ -z "${WORKHIVE_SAVE_LOCK_QUIET:-}" ]] || return 1
        [[ -z "${WORKHIVE_SAVE_QUALITY_QUIET:-}" ]] || return 1
        [[ -z "${WORKHIVE_SAVE_QUALITY_MAX_RETRIES:-}" ]] || return 1
        return 0
    }

    _session_pre_kill_snapshot_best_effort "main"
)
assert_eq "0" "$?" "_session_pre_kill_snapshot_best_effort should not inherit managed env from parent shell"

test_case "session_kill_by_name does not block on pre-kill snapshot timeout"
(
    _session_ensure_terminal_adapter() { return 0; }
    terminal_adapter_get_current_session() { printf '\n'; }
    terminal_adapter_kill_session() { return 0; }
    terminal_adapter_has_session() {
        ((_has_session_calls += 1))
        [[ "$_has_session_calls" -le 1 ]]
    }

    persistence_save_before_kill() { sleep 5; }

    _has_session_calls=0
    _op_ok_called=0
    _op_fail_called=0
    op_begin() { :; }
    op_ok() { _op_ok_called=1; }
    op_fail() { _op_fail_called=1; }
    log_info() { :; }
    log_warn() { :; }
    log_error() { :; }
    log_success() { :; }
    log_debug() { :; }
    cache_invalidate() { :; }

    WORKHIVE_PRE_KILL_SAVE_TIMEOUT_SEC=0
    WORKHIVE_SESSION_KILL_WAIT_MAX_ATTEMPTS=4
    WORKHIVE_SESSION_KILL_WAIT_INTERVAL_SEC=0
    session_kill_by_name "wh@test@main" "main" >/dev/null 2>&1
    rc=$?
    [[ "$rc" -eq 0 ]] || exit 1
    [[ "$_op_ok_called" -eq 1 ]] || exit 1
    [[ "$_op_fail_called" -eq 0 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "session_kill_by_name should proceed even when pre-kill snapshot times out"

test_report

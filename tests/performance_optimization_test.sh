#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/utils/i18n.sh"
source "$LIB_DIR/infra/logging/manager.sh"

# ──────────────────────────────────────────────────
# Test stubs (no tmux/git needed)
# ──────────────────────────────────────────────────
_git_root() { printf '%s' "/tmp/wh-perf-test-repo"; }
_workhive_dir() { printf '%s' "/tmp/wh-perf-test-repo/.workhive"; }

# ══════════════════════════════════════════════════
# Section 1: worktree_status_cache_service
# ══════════════════════════════════════════════════

source "$LIB_DIR/domain/services/worktree_status_cache_service.sh"

test_suite "worktree status cache service"

# ── TTL cache hit ─────────────────────────────────

test_case "cache returns stored value within TTL"
_WT_STATUS_GIT_STATUS=()
_WT_STATUS_GIT_STATUS_TS=()
_WT_STATUS_GIT_STATUS["/tmp/test-wt"]="3M 1A"
_WT_STATUS_GIT_STATUS_TS["/tmp/test-wt"]=$(date +%s)
result=$(wt_status_cache_git_status "/tmp/test-wt" 60)
assert_eq "3M 1A" "$result" "should return cached value"

# ── TTL cache miss (expired) ─────────────────────

test_case "cache recomputes after TTL expires"
_WT_STATUS_GIT_STATUS=()
_WT_STATUS_GIT_STATUS_TS=()
_WT_STATUS_GIT_STATUS["/tmp/nonexistent-wt"]="old-value"
_WT_STATUS_GIT_STATUS_TS["/tmp/nonexistent-wt"]=1000000
result=$(wt_status_cache_git_status "/tmp/nonexistent-wt" 1)
assert_eq "-" "$result" "expired cache should recompute (nonexistent dir -> -)"

# ── Invalidation ─────────────────────────────────

test_case "invalidation clears all cached data"
_WT_STATUS_GIT_STATUS["/a"]="clean"
_WT_STATUS_COMMIT_EPOCH["/a:main"]="12345"
wt_status_cache_invalidate
assert_eq "0" "${#_WT_STATUS_GIT_STATUS[@]}" "git status cache should be empty"
assert_eq "0" "${#_WT_STATUS_COMMIT_EPOCH[@]}" "commit epoch cache should be empty"

# ── Commit epoch for non-existent path ───────────

test_case "commit epoch returns 0 for nonexistent worktree"
_WT_STATUS_COMMIT_EPOCH=()
_WT_STATUS_COMMIT_EPOCH_TS=()
result=$(wt_status_cache_commit_epoch "/no/such/path" "" "" 60)
assert_eq "0" "$result" "nonexistent path should return 0"

# ── Compute git status for nonexistent dir ───────

test_case "compute git status returns - for nonexistent dir"
result=$(_wt_status_compute_git_status "/no/such/path")
assert_eq "-" "$result" "nonexistent dir should produce -"

# ══════════════════════════════════════════════════
# Section 2: workspace_status_service
# ══════════════════════════════════════════════════

worktree_get_base_path() { printf '%s' "/tmp/wh-perf-test-repo/.workhive/worktrees"; }

source "$LIB_DIR/domain/services/workspace_status_service.sh"

# Stub tmux for testing — must be defined AFTER sourcing the service
tmux() {
    case "$1" in
        list-sessions)
            printf '%s\n' "wh@1@proj@main|1|1712345678"
            printf '%s\n' "wh@1@proj@dev|0|1712345600"
            printf '%s\n' "other-session|0|1712340000"
            ;;
        list-panes)
            printf '%s\n' "wh@1@proj@main|/tmp/wh-perf-test-repo/worktrees/main"
            printf '%s\n' "wh@1@proj@main|/tmp/wh-perf-test-repo/worktrees/main"
            printf '%s\n' "wh@1@proj@dev|/tmp/wh-perf-test-repo/worktrees/dev"
            printf '%s\n' "other-session|/other/project"
            ;;
    esac
}

# Stub git for worktree listing
git() {
    case "$1" in
        worktree)
            if [[ "${2:-}" == "list" ]]; then
                cat <<'EOF'
worktree /tmp/wh-perf-test-repo
branch refs/heads/main

worktree /tmp/wh-perf-test-repo/.workhive/worktrees/feature-x
branch refs/heads/feature/x

EOF
            fi
            ;;
    esac
}

test_suite "workspace status service (bulk collection)"

# ── Bulk tmux collection ─────────────────────────

test_case "bulk tmux collects session attached status"
workspace_status_reset
workspace_status_collect_sessions "/tmp/wh-perf-test-repo"
result=$(workspace_status_session_attached "wh@1@proj@main")
assert_eq "1" "$result" "main session should be attached"

test_case "bulk tmux collects session activity epoch"
result=$(workspace_status_session_activity "wh@1@proj@main")
assert_eq "1712345678" "$result" "activity epoch should match"

test_case "bulk tmux collects pane count"
result=$(workspace_status_session_pane_count "wh@1@proj@main")
assert_eq "2" "$result" "main session should have 2 panes"

test_case "bulk tmux identifies project sessions"
workspace_status_session_in_project "wh@1@proj@main"
rc=$?
assert_eq "0" "$rc" "main session should be in project"

test_case "bulk tmux excludes non-project sessions"
workspace_status_session_in_project "other-session"
rc=$?
assert_eq "1" "$rc" "other session should not be in project"

test_case "detached session has attached=0"
result=$(workspace_status_session_attached "wh@1@proj@dev")
assert_eq "0" "$result" "dev session should be detached"

# ── Bulk worktree mapping ────────────────────────

test_case "bulk worktree maps branch to path"
workspace_status_collect_worktrees
result=$(workspace_status_worktree_path_for_branch "feature/x")
assert_eq "/tmp/wh-perf-test-repo/.workhive/worktrees/feature-x" "$result" "feature/x should map to path"

test_case "sanitized-to-real branch mapping"
result=$(workspace_status_branch_for_sanitized "feature-x")
assert_eq "feature/x" "$result" "feature-x should map to feature/x"

test_case "managed_branches lists only managed worktrees"
result=$(workspace_status_managed_branches | sort)
assert_contains "$result" "feature/x" "should include managed worktree branch"

test_case "non-managed worktree (repo root) excluded from branch mapping"
result=$(workspace_status_worktree_path_for_branch "main")
assert_empty "$result" "main (repo root, not under base_path) should not appear in managed mapping"

# ── Reset ────────────────────────────────────────

test_case "reset clears all state"
workspace_status_reset
result=$(workspace_status_session_attached "wh@1@proj@main")
assert_eq "0" "$result" "after reset, attached should default to 0"
result=$(workspace_status_worktree_path_for_branch "feature/x")
assert_empty "$result" "after reset, worktree mapping should be empty"

# ══════════════════════════════════════════════════
# Section 3: perf_timer API
# ══════════════════════════════════════════════════

test_suite "perf timer API"

test_case "perf_timer_start returns numeric value"
t=$(perf_timer_start)
assert_true '[[ "$t" =~ ^[0-9]+$ ]]' "start should be numeric milliseconds"

test_case "perf_timer_log writes to log file"
local_log="/tmp/wh-perf-test-$$.log"
_LOG_FILE="$local_log"
_LOG_INITIALIZED=true
t=$(perf_timer_start)
sleep 0.05
perf_timer_log "test.phase" "$t" "extra=data"
if [[ -f "$local_log" ]]; then
    log_content=$(cat "$local_log")
    assert_contains "$log_content" "[PERF]" "log should contain PERF tag"
    assert_contains "$log_content" "phase=test.phase" "log should contain phase name"
    assert_contains "$log_content" "elapsed_ms=" "log should contain elapsed_ms"
    assert_contains "$log_content" "extra=data" "log should contain detail"
else
    assert_eq "file_exists" "file_missing" "log file should exist"
fi
rm -f "$local_log" 2>/dev/null

# ══════════════════════════════════════════════════
# Section 4: ls collection strategy (fast vs full)
# ══════════════════════════════════════════════════

test_suite "ls collection strategy"

# These constants are defined in list_command.sh. Verify via grep
# to avoid sourcing the full command layer with all its dependencies.
test_case "fast strategy constant is defined in list_command.sh"
fast_def=$(grep -c '_STATUS_STRATEGY_FAST="fast"' "$ROOT_DIR/lib/app/commands/list_command.sh" || true)
assert_eq "1" "$fast_def" "fast constant should be defined exactly once"

test_case "full strategy constant is defined in list_command.sh"
full_def=$(grep -c '_STATUS_STRATEGY_FULL="full"' "$ROOT_DIR/lib/app/commands/list_command.sh" || true)
assert_eq "1" "$full_def" "full constant should be defined exactly once"

test_case "watch body uses fast strategy"
watch_fast=$(grep -c '_STATUS_STRATEGY_FAST' "$ROOT_DIR/lib/app/commands/list_command.sh" || true)
assert_true '[[ "$watch_fast" -ge 2 ]]' "fast strategy should be used in watch path"

test_case "normal collect uses full strategy"
normal_full=$(grep -c '_STATUS_STRATEGY_FULL' "$ROOT_DIR/lib/app/commands/list_command.sh" || true)
assert_true '[[ "$normal_full" -ge 2 ]]' "full strategy should be used in normal path"

# ══════════════════════════════════════════════════
# Section 5: worktree_create prune-on-failure
# ══════════════════════════════════════════════════

test_suite "worktree_create prune-on-failure pattern"

test_case "worktree_create no longer has unconditional prune"
unconditional_prune=$(grep -c 'git worktree prune$' "$ROOT_DIR/lib/domain/services/worktree_service.sh" || true)
assert_eq "0" "$unconditional_prune" "no unconditional bare 'git worktree prune' should exist in worktree_create"

test_case "prune is called only after add failure (retry pattern)"
retry_prune=$(grep -A1 'worktree add failed' "$ROOT_DIR/lib/domain/services/worktree_service.sh" | grep -c 'git worktree prune' || true)
assert_eq "1" "$retry_prune" "prune should appear in the failure-retry block"

test_case "_worktree_add_or_create helper is defined"
type _worktree_add_or_create &>/dev/null
rc=$?
assert_success "$rc" "_worktree_add_or_create should be a defined function"

test_case "_worktree_add_or_create does not call prune"
body=$(declare -f _worktree_add_or_create 2>/dev/null)
assert_not_contains "$body" "prune" "_worktree_add_or_create must not call prune itself"

# ══════════════════════════════════════════════════
# Section 6: session_apply_deferred exists
# ══════════════════════════════════════════════════

test_suite "session bootstrap phasing"

test_case "session_apply_config does not contain autosave hooks"
config_body=$(sed -n '/^session_apply_config()/,/^}/p' "$ROOT_DIR/lib/domain/services/session_service.sh")
assert_not_contains "$config_body" "auto_save_install_hooks" "apply_config must not contain autosave hooks"
assert_not_contains "$config_body" "auto_save_start_timer" "apply_config must not contain autosave timer"

test_case "session_apply_deferred is defined and contains autosave"
deferred_body=$(sed -n '/^session_apply_deferred()/,/^}/p' "$ROOT_DIR/lib/domain/services/session_service.sh")
assert_not_empty "$deferred_body" "session_apply_deferred should exist in session_service.sh"
assert_contains "$deferred_body" "auto_save_install_hooks" "deferred must contain autosave hooks"
assert_contains "$deferred_body" "auto_save_start_timer" "deferred must contain autosave timer"

test_case "restore.sh calls session_apply_deferred after session_apply_config"
restore_deferred=$(grep -c 'session_apply_deferred' "$ROOT_DIR/lib/infra/persistence/restore.sh" || true)
assert_true '[[ "$restore_deferred" -ge 1 ]]' "restore path should call session_apply_deferred"

# ══════════════════════════════════════════════════
# Section 7: open_command attach delay default
# ══════════════════════════════════════════════════

test_suite "open command attach delay"

test_case "default attach delay is 0 (immediate after readiness)"
delay_val=$(grep 'readonly _OPEN_DELAY_BEFORE_ATTACH=' "$ROOT_DIR/lib/app/commands/open_command.sh" | head -1)
assert_contains "$delay_val" "=0" "default delay should be 0"

test_case "attach path skips sleep when delay is 0"
skip_sleep=$(grep -A2 'attach_delay_seconds.*-gt 0' "$ROOT_DIR/lib/app/commands/open_command.sh" | head -3)
assert_contains "$skip_sleep" "sleep" "conditional sleep should exist"

test_case "auto-attach path: deferred runs async before attach (not blocking)"
deferred_async_before_attach=$(grep -B2 'session_attach' "$ROOT_DIR/lib/app/commands/open_command.sh" | grep -c 'session_apply_deferred_async' || true)
assert_true '[[ "$deferred_async_before_attach" -ge 1 ]]' "deferred_async should be called before attach in auto-attach path"

test_case "auto-attach path: synchronous deferred does NOT appear before attach"
sync_deferred_before_attach=$(grep -B2 'session_attach' "$ROOT_DIR/lib/app/commands/open_command.sh" | grep -cv 'deferred_async' | head -1 || true)
sync_deferred_attach_line=$(grep -n 'session_apply_deferred[^_]' "$ROOT_DIR/lib/app/commands/open_command.sh" || true)
attach_line=$(grep -n 'session_attach' "$ROOT_DIR/lib/app/commands/open_command.sh" | head -1 || true)
attach_lineno="${attach_line%%:*}"
deferred_before_attach_count=0
while IFS= read -r dline; do
    [[ -z "$dline" ]] && continue
    dlineno="${dline%%:*}"
    if [[ "$dlineno" =~ ^[0-9]+$ ]] && (( dlineno < attach_lineno )); then
        # Check if this is inside auto-attach block (after auto_attach == true check)
        deferred_before_attach_count=$((deferred_before_attach_count + 1))
    fi
done <<< "$sync_deferred_attach_line"
# The sync deferred should only appear in no-attach path, not in auto-attach path
# We verify no sync deferred_call appears in the immediate lines before session_attach
direct_sync=$(grep -B1 'session_attach' "$ROOT_DIR/lib/app/commands/open_command.sh" | grep -c 'session_apply_deferred[^_]' || true)
assert_eq "0" "$direct_sync" "synchronous session_apply_deferred must not be directly before session_attach"

test_case "session_apply_deferred_async is defined and runs in background"
async_body=$(sed -n '/^session_apply_deferred_async()/,/^}/p' "$ROOT_DIR/lib/domain/services/session_service.sh")
assert_not_empty "$async_body" "session_apply_deferred_async should exist"
assert_contains "$async_body" "session_apply_deferred" "async variant should call sync deferred"
assert_contains "$async_body" "&" "async variant should background the call"

test_case "open command has perf timing for workspace_open"
perf_ws=$(grep -c 'perf_timer_log.*open.workspace_open' "$ROOT_DIR/lib/app/commands/open_command.sh" || true)
assert_true '[[ "$perf_ws" -ge 1 ]]' "open.workspace_open perf log should exist"

test_case "open command has perf timing for layout"
perf_layout=$(grep -c 'perf_timer_log.*open.layout' "$ROOT_DIR/lib/app/commands/open_command.sh" || true)
assert_true '[[ "$perf_layout" -ge 1 ]]' "open.layout perf log should exist"

test_case "open command has perf timing for total"
perf_total=$(grep -c 'perf_timer_log.*open.total' "$ROOT_DIR/lib/app/commands/open_command.sh" || true)
assert_true '[[ "$perf_total" -ge 1 ]]' "open.total perf log should exist"

# ══════════════════════════════════════════════════
# Section 8: ls branch order stability (Finding 1)
# ══════════════════════════════════════════════════

test_suite "ls branch order stability"

# Override git stub to produce multiple worktrees in known order
git() {
    case "$1" in
        worktree)
            if [[ "${2:-}" == "list" ]]; then
                cat <<'EOF'
worktree /tmp/wh-perf-test-repo
branch refs/heads/main

worktree /tmp/wh-perf-test-repo/.workhive/worktrees/alpha
branch refs/heads/alpha

worktree /tmp/wh-perf-test-repo/.workhive/worktrees/beta
branch refs/heads/beta

worktree /tmp/wh-perf-test-repo/.workhive/worktrees/gamma
branch refs/heads/gamma

EOF
            fi
            ;;
    esac
}

test_case "managed_branches preserves git worktree list order"
workspace_status_reset
workspace_status_collect_worktrees
result=$(workspace_status_managed_branches)
expected=$'alpha\nbeta\ngamma'
assert_eq "$expected" "$result" "branches must appear in git worktree list order"

test_case "managed_branches order is stable across repeated calls"
result1=$(workspace_status_managed_branches)
result2=$(workspace_status_managed_branches)
result3=$(workspace_status_managed_branches)
assert_eq "$result1" "$result2" "first and second call must produce same order"
assert_eq "$result2" "$result3" "second and third call must produce same order"

test_case "reset clears ordered branches array"
workspace_status_reset
result=$(workspace_status_managed_branches)
assert_empty "$result" "after reset, managed_branches should be empty"

test_case "_WSS_WT_ORDERED_BRANCHES is populated by collect_worktrees"
workspace_status_reset
workspace_status_collect_worktrees
assert_eq "3" "${#_WSS_WT_ORDERED_BRANCHES[@]}" "ordered array should have 3 entries"
assert_eq "alpha" "${_WSS_WT_ORDERED_BRANCHES[0]}" "first entry should be alpha"
assert_eq "beta" "${_WSS_WT_ORDERED_BRANCHES[1]}" "second entry should be beta"
assert_eq "gamma" "${_WSS_WT_ORDERED_BRANCHES[2]}" "third entry should be gamma"

# ══════════════════════════════════════════════════
# Section 9: JSON/fast strategy contract (Finding 3)
# ══════════════════════════════════════════════════

test_suite "JSON/fast/full strategy contract"

test_case "_status_select_strategy defined in list_command.sh"
strategy_fn=$(grep -c '_status_select_strategy()' "$ROOT_DIR/lib/app/commands/list_command.sh" || true)
assert_true '[[ "$strategy_fn" -ge 1 ]]' "_status_select_strategy function should be defined"

test_case "strategy selector returns fast for json format"
json_strategy=$(grep -A5 '_status_select_strategy()' "$ROOT_DIR/lib/app/commands/list_command.sh" | grep -c 'FAST' || true)
assert_true '[[ "$json_strategy" -ge 1 ]]' "json format should map to FAST strategy"

test_case "strategy selector returns full for table format"
table_strategy=$(grep -A5 '_status_select_strategy()' "$ROOT_DIR/lib/app/commands/list_command.sh" | grep -c 'FULL' || true)
assert_true '[[ "$table_strategy" -ge 1 ]]' "table format should map to FULL strategy"

test_case "_status_collect_data_for_format calls strategy selector"
collect_uses_selector=$(grep -A5 '_status_collect_data_for_format()' "$ROOT_DIR/lib/app/commands/list_command.sh" | grep -c '_status_select_strategy' || true)
assert_true '[[ "$collect_uses_selector" -ge 1 ]]' "collect_data_for_format should use strategy selector"

test_case "service comment documents json uses fast strategy"
service_comment=$(grep 'fast.*json\|json.*fast' "$ROOT_DIR/lib/domain/services/workspace_status_service.sh" || true)
assert_not_empty "$service_comment" "service header should document json->fast relationship"

test_case "command comment documents json uses fast strategy"
cmd_comment=$(grep -A1 'fast.*skip\|skip.*fast' "$ROOT_DIR/lib/app/commands/list_command.sh" | grep -c 'json' || true)
assert_true '[[ "$cmd_comment" -ge 1 ]]' "command header should document json->fast relationship"

# ══════════════════════════════════════════════════
# Section 10: deferred async first-frame contract (Finding 2)
# ══════════════════════════════════════════════════

test_suite "deferred async first-frame contract"

test_case "no-attach path still uses synchronous deferred"
no_attach_deferred=$(grep -A5 'STEP_4_SKIP_ATTACH' "$ROOT_DIR/lib/app/commands/open_command.sh" | grep 'session_apply_deferred' | grep -cv '_async' || true)
assert_true '[[ "$no_attach_deferred" -ge 1 ]]' "no-attach path should call synchronous deferred"

test_case "auto-attach path does NOT call synchronous deferred"
# Between perf_timer_log open.total and session_attach, only async variant should appear
attach_block=$(sed -n '/perf_timer_log.*open.total/,/session_attach/p' "$ROOT_DIR/lib/app/commands/open_command.sh")
sync_in_attach_block=$(echo "$attach_block" | grep -c 'session_apply_deferred[^_]' || true)
assert_eq "0" "$sync_in_attach_block" "sync deferred must not appear in auto-attach critical path"

test_case "auto-attach path calls async deferred"
async_in_attach_block=$(echo "$attach_block" | grep -c 'session_apply_deferred_async' || true)
assert_true '[[ "$async_in_attach_block" -ge 1 ]]' "async deferred must appear in auto-attach path"

# ══════════════════════════════════════════════════
# Section 11: pane metadata consistency (Finding 4)
# ══════════════════════════════════════════════════

test_suite "pane metadata consistency"

source "$LIB_DIR/domain/services/pane_state_service.sh" 2>/dev/null || true

test_case "hermes classifies as agent kind"
kind=$(_pane_meta_classify_kind "hermes")
assert_eq "agent" "$kind" "hermes must classify as agent"

test_case "claude classifies as agent kind"
kind=$(_pane_meta_classify_kind "claude")
assert_eq "agent" "$kind" "claude must classify as agent"

test_case "codex classifies as agent kind"
kind=$(_pane_meta_classify_kind "codex")
assert_eq "agent" "$kind" "codex must classify as agent"

test_case "Python does NOT classify as agent"
kind=$(_pane_meta_classify_kind "Python")
assert_eq "shell" "$kind" "Python (runtime process) must classify as shell, not agent"

test_case "restore path uses desired_command for kind classification (legacy snapshot)"
restore_legacy_block=$(sed -n '/^    else$/,/^    fi$/p' "$ROOT_DIR/lib/infra/persistence/restore.sh" | head -20)
assert_contains "$restore_legacy_block" "_SP_PANE_DESIRED_CMD" "legacy restore must check desired_command before full_cmd"

test_case "restore path reclassifies kind from desired_command when snapshot kind is wrong"
restore_reclassify=$(grep -c 'reclassified_kind' "$ROOT_DIR/lib/infra/persistence/restore.sh" || true)
assert_true '[[ "$restore_reclassify" -ge 1 ]]' "restore must reclassify kind from desired_command"

test_case "restore reclassification uses _pane_meta_classify_kind"
restore_classify_call=$(grep 'reclassified_kind.*_pane_meta_classify_kind' "$ROOT_DIR/lib/infra/persistence/restore.sh" || true)
assert_not_empty "$restore_classify_call" "reclassification must use _pane_meta_classify_kind"

test_case "restore logs kind correction when snapshot kind differs from reclassified"
kind_correction_log=$(grep 'kind corrected' "$ROOT_DIR/lib/infra/persistence/restore.sh" || true)
assert_not_empty "$kind_correction_log" "kind correction must be logged for debugging"

test_case "wheel strategy depends only on @workhive_pane_kind metadata, not command names"
source "$LIB_DIR/infra/adapters/terminal/tmux.sh" 2>/dev/null || true
ws_up=$(_tmux_build_workspace_wheel_up)
assert_not_contains "$ws_up" "hermes" "wheel policy must not reference specific command names"
assert_not_contains "$ws_up" "claude" "wheel policy must not reference specific command names"
assert_not_contains "$ws_up" "codex" "wheel policy must not reference specific command names"
assert_contains "$ws_up" "@workhive_pane_kind" "wheel policy must use semantic pane_kind metadata"

test_case "save path reads pane_kind from tmux option, not from pane_command"
save_kind_read=$(grep '@workhive_pane_kind' "$ROOT_DIR/lib/infra/persistence/save.sh" || true)
assert_not_empty "$save_kind_read" "save must read kind from tmux pane option"

# ══════════════════════════════════════════════════

test_report

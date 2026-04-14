#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/infra/config/loader.sh
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"

# Temporary directory for testing
TEST_DIR="/tmp/workhive-test-config-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
# Isolate global config to prevent user-level config from affecting test results
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
mkdir -p "$XDG_CONFIG_HOME"

cleanup() {
    cd / || return 1
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

test_suite "lib/infra/config/loader.sh"

# ──────────────────────────────────────────────────
# config_get_defaults
# ──────────────────────────────────────────────────

test_case "config_get_defaults returns valid YAML"
defaults=$(config_get_defaults)
assert_contains "$defaults" "version:" "has version"
assert_contains "$defaults" "layout:" "has layout"
assert_contains "$defaults" "worktree:" "has worktree"
assert_contains "$defaults" "tmux:" "has tmux"

test_case "config_get_defaults contains version"
defaults=$(config_get_defaults)
assert_contains "$defaults" "\"${WORKHIVE_VERSION}\""

test_case "config_get_defaults includes columns layout"
defaults=$(config_get_defaults)
assert_contains "$defaults" "columns:" "has columns layout"
assert_contains "$defaults" "width:" "has column width"
assert_contains "$defaults" "panes:" "has panes"
assert_contains "$defaults" "height:" "has pane height"

test_case "config_get_defaults includes worktree settings"
defaults=$(config_get_defaults)
assert_not_contains "$defaults" "worktree.enabled" "removed key should not appear in defaults"
assert_not_contains "$defaults" "enabled: true" "worktree.enabled should be removed from defaults"
assert_contains "$defaults" 'base_path: ".workhive/worktrees"'
assert_contains "$defaults" "auto_create: true"
assert_contains "$defaults" "auto_delete: true"

test_case "config_get_defaults includes maintenance temp cleanup settings"
defaults=$(config_get_defaults)
assert_contains "$defaults" "maintenance:" "should include maintenance block"
assert_contains "$defaults" "temp_cleanup:" "should include temp_cleanup block"
assert_contains "$defaults" "max_age_days: 7" "temp cleanup default should be 7 days"
assert_contains "$defaults" 'path: ".workhive/logs/workhive.log"' "runtime log path default should exist"
assert_contains "$defaults" 'path: "/dev/null"' "test log path default should exist"
assert_contains "$defaults" "max_backups: 5" "log rotation backup count default should exist"

test_case "config_get_defaults includes tmux settings"
defaults=$(config_get_defaults)
assert_contains "$defaults" "mouse_support: true"
assert_contains "$defaults" "CLAUDE_CODE_NO_FLICKER: 1" "tmux env defaults should be written into config defaults"
assert_contains "$defaults" "auto_save_env:" "persistence auto-save env defaults should be written into config defaults"
assert_contains "$defaults" "pre_kill_env:" "persistence pre-kill env defaults should be written into config defaults"
assert_contains "$defaults" "WORKHIVE_SAVE_LOCK_QUIET: 1" "persistence quiet env default should live in config"
assert_contains "$defaults" "WORKHIVE_SAVE_QUALITY_QUIET: 1" "persistence quality quiet env default should live in config"
assert_contains "$defaults" "WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1" "pre-kill retry env default should live in config"
assert_not_contains "$defaults" "status_bar:" "legacy tmux.status_bar should not be generated"
assert_contains "$defaults" 'theme: "catppuccin-mocha"'

test_case "config_get_defaults no longer includes tmux statusbar and watcher settings"
defaults=$(config_get_defaults)
assert_not_contains "$defaults" "statusbar:" "tmux.statusbar block should be removed"
assert_not_contains "$defaults" "watcher:" "tmux.watcher block should be removed"

test_case "config_get_defaults includes persistence retention settings"
defaults=$(config_get_defaults)
assert_contains "$defaults" "auto_save_mode: hybrid" "should include auto-save mode default"
assert_contains "$defaults" "auto_save_debounce: 3s" "should include auto-save debounce default"
assert_contains "$defaults" "auto_save_interval: 10m" "should include auto-save interval default"
assert_contains "$defaults" "snapshot_history_limit: 8" "should include snapshot history limit default"
assert_contains "$defaults" "snapshot_max_size_mb: 8" "should include snapshot max size default"

test_case "i18n PERSIST_GENERATION_BUMPED_FMT supports en and zh"
WORKHIVE_LANG=en
msg_en=$(_i18n_fmt PERSIST_GENERATION_BUMPED_FMT "3")
assert_contains "$msg_en" "Persistence generation bumped to 3" "english persistence generation message should be formatted"
WORKHIVE_LANG=zh
msg_zh=$(_i18n_fmt PERSIST_GENERATION_BUMPED_FMT "3")
assert_contains "$msg_zh" "持久化代际已提升到 3" "chinese persistence generation message should be formatted"

test_case "config_get_defaults has session_prefix null"
defaults=$(config_get_defaults)
assert_contains "$defaults" "session_prefix" "should have session_prefix key"
assert_contains "$defaults" "session_prefix: null" "default should be null"

# ──────────────────────────────────────────────────
# config_exists
# ──────────────────────────────────────────────────

test_case "config_project_exists returns 1 when no project config"
rm -rf .workhive
config_project_exists || rc=$?
assert_eq "1" "${rc:-0}"

test_case "config_exists returns 0 when config present"
mkdir -p .workhive
echo "version: \"0.6.0\"" > .workhive/config.yaml
config_exists
assert_eq "0" "$?"

test_case "config_exists checks global config too"
rm -rf .workhive
_global_dir=$(config_get_global_dir)
if [[ -f "$_global_dir/config.yaml" ]]; then
    config_exists
    assert_eq "0" "$?"
fi

# ──────────────────────────────────────────────────
# config_get_global_dir / config_get_global_file
# ──────────────────────────────────────────────────

test_case "config_get_global_dir returns XDG path"
result=$(config_get_global_dir)
# Should end with /workhive (XDG config dir for workhive)
assert_contains "$result" "workhive" "should contain workhive"
# On macOS/Linux typically ~/.config/workhive
if [[ "$result" == *"/workhive" ]]; then rc=0; else rc=1; fi
assert_eq "0" "$rc" "path should end with /workhive"

test_case "config_get_global_file returns config.yaml path"
result=$(config_get_global_file)
assert_contains "$result" "workhive" "should contain workhive"
assert_contains "$result" "config.yaml" "should contain config.yaml"

test_case "config_get_global_file respects override file path"
override_file="$TEST_DIR/.xdg_config/workhive/config.stage.yaml"
result=$(WORKHIVE_GLOBAL_CONFIG_FILE="$override_file" config_get_global_file)
assert_eq "$override_file" "$result" "override path should take precedence"

test_case "config_get_project_file respects override file path"
project_override="$TEST_DIR/.workhive/config.stage.yaml"
result=$(WORKHIVE_PROJECT_CONFIG_FILE="$project_override" config_get_project_file)
assert_eq "$project_override" "$result" "project override path should take precedence"

if command -v yq &>/dev/null; then
    test_case "config_global_ready ignores lock when pid is not a workhive process"
    mkdir -p "$XDG_CONFIG_HOME/workhive"
    mkdir -p "$XDG_CONFIG_HOME/workhive/.init.lock"
    printf '%s\n' "$$" > "$XDG_CONFIG_HOME/workhive/.init.lock/pid"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'EOF'
version: "0.9.0"
EOF
    rc=0
    config_global_ready || rc=$?
    assert_eq "0" "${rc:-0}" "non-workhive pid lock should be treated as stale"

    test_case "config_global_ready removes stale lock and returns true"
    mkdir -p "$XDG_CONFIG_HOME/workhive/.init.lock"
    printf '%s\n' "999999" > "$XDG_CONFIG_HOME/workhive/.init.lock/pid"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'EOF'
version: "0.8.0"
tmux:
  theme: "catppuccin-mocha"
EOF
    config_global_ready
    assert_eq "0" "$?" "stale lock should be auto-cleaned and global config treated as ready"
    assert_true "[[ ! -d '$XDG_CONFIG_HOME/workhive/.init.lock' ]]" "stale init lock should be removed"
fi

# ──────────────────────────────────────────────────
# config_init
# ──────────────────────────────────────────────────

test_case "config_init creates default config"
rm -rf .workhive
config_init >/dev/null 2>&1
assert_file_exists ".workhive/config.yaml" "config file created"
init_log_path=$(yq -r '.maintenance.log.path' .workhive/config.yaml 2>/dev/null || echo "")
assert_eq ".workhive/logs/workhive.log" "$init_log_path" "init should write maintenance.log.path"
init_test_log_path=$(yq -r '.maintenance.test_log.path' .workhive/config.yaml 2>/dev/null || echo "")
assert_eq "/dev/null" "$init_test_log_path" "init should write maintenance.test_log.path"

test_case "config_init fails if config already exists"
# config already exists from previous test
config_init >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}" "should fail when config exists"

# ──────────────────────────────────────────────────
# config_load_file
# ──────────────────────────────────────────────────

test_case "config_load_file returns 1 for non-existent file"
config_load_file "/tmp/nonexistent_config_xyz.yaml" >/dev/null 2>&1 || rc=$?
assert_eq "1" "${rc:-0}"

test_case "config_load_file loads existing file"
cat > "$TEST_DIR/test_config.yaml" <<'EOF'
version: "0.9.0"
tmux:
  theme: "nord"
EOF
output=$(config_load_file "$TEST_DIR/test_config.yaml" 2>/dev/null)
assert_contains "$output" 'theme: "nord"'

# ──────────────────────────────────────────────────
# config_get (with yq)
# ──────────────────────────────────────────────────

if command -v yq &>/dev/null; then
    test_case "config_get reads project config value"
    rm -rf .workhive
    mkdir -p .workhive
    cat > .workhive/config.yaml <<'EOF'
version: "0.6.0"
tmux:
  theme: "nord"
EOF
    cache_clear
    result=$(config_get "tmux.theme")
    assert_eq "nord" "$result"

    test_case "config_get reads nested value"
    result=$(config_get "tmux.theme")
    assert_eq "nord" "$result"

    test_case "config_get falls back to defaults for missing key"
    cache_clear
    result=$(config_get "tmux.mouse_support")
    assert_eq "true" "$result"

    test_case "config_get caches result"
    cache_clear
    config_get "tmux.theme" >/dev/null
    # Second call should hit cache
    cached=$(cache_get "config:value:tmux.theme" 2>/dev/null || echo "MISS")
    assert_eq "nord" "$cached"

    # ──────────────────────────────────────────────────
    # config_set (with yq)
    # ──────────────────────────────────────────────────

    test_case "config_set updates value"
    cache_clear
    config_set "tmux.theme" "gruvbox" >/dev/null 2>&1
    result=$(config_get "tmux.theme")
    assert_eq "gruvbox" "$result"

    test_case "config_set invalidates cache"
    cache_set "config:value:tmux.theme" "old_cached_value" 60
    config_set "tmux.theme" "tokyo-night" >/dev/null 2>&1
    # Cache should be invalidated
    cache_get "config:value:tmux.theme" >/dev/null 2>&1 || rc=$?
    assert_eq "1" "${rc:-0}" "cache should be invalidated"

    test_case "config_set with --global writes to global config"
    _global_test_dir="/tmp/workhive-test-global-config-$$"
    mkdir -p "$_global_test_dir"
    WORKHIVE_CONFIG_DIR="$_global_test_dir" config_set "tmux.theme" "global-theme" "--global" >/dev/null 2>&1
    result=$(yq -r '.tmux.theme' "$_global_test_dir/config.yaml" 2>/dev/null)
    assert_eq "global-theme" "$result" "global config should be updated"
    rm -rf "$_global_test_dir"

    test_case "config_set writes to project override file when provided"
    rm -rf .workhive
    mkdir -p .workhive
    cp /dev/null .workhive/config.stage.yaml
    config_get_defaults > .workhive/config.stage.yaml
    WORKHIVE_PROJECT_CONFIG_FILE="$TEST_DIR/.workhive/config.stage.yaml" \
        config_set "tmux.theme" "project-stage-theme" >/dev/null 2>&1
    result=$(yq -r '.tmux.theme' "$TEST_DIR/.workhive/config.stage.yaml" 2>/dev/null)
    assert_eq "project-stage-theme" "$result" "project override file should be updated"

test_case "config_sync_schema_file rejects deprecated duration keys"
    legacy_cfg="$TEST_DIR/legacy-config.yaml"
    cat > "$legacy_cfg" <<'YAML'
version: "0.8.0"
tmux:
  persistence:
    auto_save_debounce_seconds: 3
YAML
    rc=0
    config_sync_schema_file "$legacy_cfg" || rc=$?
    assert_eq "1" "${rc:-0}" "deprecated key should fail strict schema validation"

    test_case "config_sync_schema_file rejects global-only keys in project scope"
    scoped_cfg="$TEST_DIR/project-scoped-config.yaml"
    cat > "$scoped_cfg" <<'YAML'
version: "0.9.0"
git_init:
  whitelist_paths:
    - "/tmp/demo"
tools:
  - tool_name: codex
YAML
    rc=0
    config_sync_schema_file "$scoped_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "project config should reject global-only keys"

    test_case "config_sync_schema_file allows global-only keys in global scope"
    scoped_global_cfg="$TEST_DIR/global-scoped-config.yaml"
    cat > "$scoped_global_cfg" <<'YAML'
version: "0.9.0"
git_init:
  whitelist_paths: []
YAML
    config_sync_schema_file "$scoped_global_cfg" "global"
    assert_eq "0" "$?" "global scope should allow git_init"

    test_case "config_sync_schema_file accepts sync_files string shorthand"
    bad_sync_cfg="$TEST_DIR/bad-sync-files-config.yaml"
    cat > "$bad_sync_cfg" <<'YAML'
version: "0.9.0"
worktree:
  sync_files:
    - ".env"
YAML
    config_sync_schema_file "$bad_sync_cfg" "project"
    assert_eq "0" "$?" "sync_files string shorthand should pass schema validation"

    test_case "config_sync_schema_file accepts sync_files object format"
    good_sync_cfg="$TEST_DIR/good-sync-files-config.yaml"
    cat > "$good_sync_cfg" <<'YAML'
version: "0.9.0"
worktree:
  sync_files:
    - source: ".env"
      mode: "copy"
YAML
    config_sync_schema_file "$good_sync_cfg" "project"
    assert_eq "0" "$?" "sync_files object format should pass schema validation"

    test_case "config_sync_schema_file rejects invalid tmux.env key names"
    bad_tmux_env_key_cfg="$TEST_DIR/bad-tmux-env-key.yaml"
    cat > "$bad_tmux_env_key_cfg" <<'YAML'
version: "0.9.0"
tmux:
  env:
    "BAD-KEY": "1"
YAML
    rc=0
    config_sync_schema_file "$bad_tmux_env_key_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "tmux.env key should match env var naming rules"

    test_case "config_sync_schema_file rejects non-scalar tmux.env values"
    bad_tmux_env_value_cfg="$TEST_DIR/bad-tmux-env-value.yaml"
    cat > "$bad_tmux_env_value_cfg" <<'YAML'
version: "0.9.0"
tmux:
  env:
    CLAUDE_CODE_FLAGS:
      - "--foo"
YAML
    rc=0
    config_sync_schema_file "$bad_tmux_env_value_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "tmux.env values must be scalar"

    test_case "config_sync_schema_file accepts maintenance log path keys"
    log_path_cfg="$TEST_DIR/log-path-config.yaml"
    cat > "$log_path_cfg" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    path: ".workhive/logs/runtime.log"
  test_log:
    path: ".workhive/logs/test-logs/test.log"
YAML
    config_sync_schema_file "$log_path_cfg" "project"
    assert_eq "0" "$?" "maintenance log path keys should pass schema validation"

    test_case "config_sync_schema_file rejects non-string maintenance.log.path"
    bad_log_path_cfg="$TEST_DIR/bad-log-path-config.yaml"
    cat > "$bad_log_path_cfg" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    path:
      nested: true
YAML
    rc=0
    config_sync_schema_file "$bad_log_path_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "maintenance.log.path must be string"

    test_case "config_sync_schema_file rejects non-string maintenance.test_log.path"
    bad_test_log_path_cfg="$TEST_DIR/bad-test-log-path-config.yaml"
    cat > "$bad_test_log_path_cfg" <<'YAML'
version: "0.9.0"
maintenance:
  test_log:
    path:
      nested: true
YAML
    rc=0
    config_sync_schema_file "$bad_test_log_path_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "maintenance.test_log.path must be string"

    test_case "config_sync_schema_file rejects non-integer maintenance.log.max_backups"
    bad_log_backups_cfg="$TEST_DIR/bad-log-backups-config.yaml"
    cat > "$bad_log_backups_cfg" <<'YAML'
version: "0.9.0"
maintenance:
  log:
    max_backups: "five"
YAML
    rc=0
    config_sync_schema_file "$bad_log_backups_cfg" "project" || rc=$?
    assert_eq "1" "${rc:-0}" "maintenance.log.max_backups must be integer >= 0"

    test_case "config_get_tmux_env_entries returns empty when no config defines tmux env"
    rm -rf "$XDG_CONFIG_HOME/workhive" .workhive
    cache_clear
    merged_env=$(config_get_tmux_env_entries)
    assert_eq "" "$merged_env" "tmux env entries should be empty without explicit config"

    test_case "config_get_tmux_env_entries merges global and project with project override"
    mkdir -p "$XDG_CONFIG_HOME/workhive" .workhive
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  env:
    CLAUDE_CODE_NO_FLICKER: "0"
    GLOBAL_ONLY: "enabled"
YAML
    cat > ".workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  env:
    CLAUDE_CODE_NO_FLICKER: "1"
    PROJECT_ONLY: "yes"
YAML
    cache_clear
    merged_env=$(config_get_tmux_env_entries | sort)
    assert_contains "$merged_env" "CLAUDE_CODE_NO_FLICKER=1" "project value should override global"
    assert_contains "$merged_env" "GLOBAL_ONLY=enabled" "global-only key should be present"
    assert_contains "$merged_env" "PROJECT_ONLY=yes" "project-only key should be present"
    assert_not_contains "$merged_env" "CLAUDE_CODE_NO_FLICKER=0" "overridden global value should not remain"

    test_case "config_get_persistence_auto_save_env_entries returns empty when config removes auto-save env"
    rm -rf "$XDG_CONFIG_HOME/workhive" .workhive
    mkdir -p .workhive
    cat > ".workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  persistence:
    enabled: true
YAML
    cache_clear
    merged_env=$(config_get_persistence_auto_save_env_entries)
    assert_eq "" "$merged_env" "auto-save env entries should be empty when config omits them"

    test_case "config_get_persistence_auto_save_env_entries merges global and project with project override"
    mkdir -p "$XDG_CONFIG_HOME/workhive"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  persistence:
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: "0"
      GLOBAL_ONLY: "enabled"
YAML
    cat > ".workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  persistence:
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: "1"
      PROJECT_ONLY: "yes"
YAML
    cache_clear
    merged_env=$(config_get_persistence_auto_save_env_entries | sort)
    assert_contains "$merged_env" "WORKHIVE_SAVE_LOCK_QUIET=1" "project value should override global"
    assert_contains "$merged_env" "GLOBAL_ONLY=enabled" "global-only key should be present"
    assert_contains "$merged_env" "PROJECT_ONLY=yes" "project-only key should be present"
    assert_not_contains "$merged_env" "WORKHIVE_SAVE_LOCK_QUIET=0" "overridden global value should not remain"

    test_case "config_get_persistence_pre_kill_env_entries returns explicit config only"
    cat > ".workhive/config.yaml" <<'YAML'
version: "0.9.0"
tmux:
  persistence:
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: "1"
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: "2"
YAML
    cache_clear
    merged_env=$(config_get_persistence_pre_kill_env_entries | sort)
    assert_contains "$merged_env" "WORKHIVE_SAVE_LOCK_QUIET=1" "pre-kill env should include configured lock quiet"
    assert_contains "$merged_env" "WORKHIVE_SAVE_QUALITY_MAX_RETRIES=2" "pre-kill env should include configured retry limit"

    test_case "config_ensure_scope_fields seeds global dangerous paths defaults"
    seeded_global_cfg="$TEST_DIR/global-seeded-config.yaml"
    config_get_defaults > "$seeded_global_cfg"
    config_ensure_scope_fields "$seeded_global_cfg" "global"
    dangerous_count=$(yq -r '.git_init.dangerous_paths_extra | length' "$seeded_global_cfg" 2>/dev/null)
    assert_true "[[ ${dangerous_count:-0} -gt 0 ]]" "dangerous paths defaults should be non-empty"

    test_case "config_ensure_scope_fields does not override existing global git_init/tools"
    preserved_global_cfg="$TEST_DIR/global-preserved-config.yaml"
    cat > "$preserved_global_cfg" <<'YAML'
version: "0.9.0"
git_init:
  whitelist_paths:
    - "/safe/work"
  dangerous_paths_extra:
    - "/custom-danger"
tools:
  default: custom
  list:
    - tool_name: custom
      label: "Custom"
      command: "custom"
      is_ai: false
      enabled: true
YAML
    config_ensure_scope_fields "$preserved_global_cfg" "global"
    preserved_danger=$(yq -r '.git_init.dangerous_paths_extra[0]' "$preserved_global_cfg" 2>/dev/null)
    preserved_tool=$(yq -r '.tools.list[0].tool_name' "$preserved_global_cfg" 2>/dev/null)
    assert_eq "/custom-danger" "$preserved_danger" "existing dangerous_paths_extra should be preserved"
    assert_eq "custom" "$preserved_tool" "existing tools should be preserved"

    test_case "config_atomic_yq_edit updates file via staged atomic replace"
    atomic_file="$TEST_DIR/atomic-config.yaml"
    config_get_defaults > "$atomic_file"
    _LOG_FILE="$TEST_DIR/config-atomic.log"
    _LOG_INITIALIZED=true
    : > "$_LOG_FILE"
    config_atomic_yq_edit "$atomic_file" '.tmux.mouse_support = false' >/dev/null 2>&1
    result=$(yq -r '.tmux.mouse_support' "$atomic_file" 2>/dev/null)
    atomic_log=$(cat "$_LOG_FILE" 2>/dev/null || true)
    assert_eq "false" "$result" "atomic helper should update config content"
    assert_contains "$atomic_log" "[OP][BEGIN] action=config_atomic_edit" "should log begin"
    assert_contains "$atomic_log" "[OP][OK] action=config_atomic_edit" "should log success"

    test_case "config_atomic_yq_edit logs FAIL once on invalid expression"
    : > "$_LOG_FILE"
    rc=0
    config_atomic_yq_edit "$atomic_file" 'invalid(' >/dev/null 2>&1 || rc=$?
    atomic_log=$(cat "$_LOG_FILE" 2>/dev/null || true)
    fail_count=$(echo "$atomic_log" | grep -c "\\[OP\\]\\[FAIL\\] action=config_atomic_edit" || true)
    assert_failure "$rc" "invalid yq expression should fail"
    assert_eq "1" "$fail_count" "should emit exactly one FAIL record"

    test_case "config_cleanup_stale_temp_files removes stale atomic/stage files only"
    stale_dir="$TEST_DIR/stale-temp-files"
    mkdir -p "$stale_dir"
    old_atomic="$stale_dir/config.atomic.old"
    old_stage="$stale_dir/config.yaml.stage.old"
    fresh_atomic="$stale_dir/config.atomic.fresh"
    marker_file="$stale_dir/keep.txt"
    : > "$old_atomic"
    : > "$old_stage"
    : > "$fresh_atomic"
    : > "$marker_file"
    touch -t 202001010101 "$old_atomic" "$old_stage"
    config_cleanup_stale_temp_files "$stale_dir" 30
    assert_true "[[ ! -f '$old_atomic' ]]" "stale atomic temp should be removed"
    assert_true "[[ ! -f '$old_stage' ]]" "stale stage temp should be removed"
    assert_true "[[ -f '$fresh_atomic' ]]" "fresh atomic temp should be preserved"
    assert_true "[[ -f '$marker_file' ]]" "non-temp file should be preserved"

    test_case "config_atomic_yq_edit prunes stale temp files in config directory"
    prune_file="$TEST_DIR/prune-config.yaml"
    config_get_defaults > "$prune_file"
    prune_dir=$(dirname "$prune_file")
    stale_atomic2="$prune_dir/config.atomic.stale-prune"
    stale_stage2="$prune_dir/config.yaml.stage.stale-prune"
    : > "$stale_atomic2"
    : > "$stale_stage2"
    touch -t 202001010101 "$stale_atomic2" "$stale_stage2"
    config_atomic_yq_edit "$prune_file" '.tmux.mouse_support = true' >/dev/null 2>&1
    assert_true "[[ ! -f '$stale_atomic2' ]]" "stale config.atomic files should be pruned before edit"
    assert_true "[[ ! -f '$stale_stage2' ]]" "stale config.yaml.stage files should be pruned before edit"

    test_case "config_set handles special characters safely"
    config_set "tmux.theme" 'test "quotes" & stuff' >/dev/null 2>&1
    result=$(config_get "tmux.theme")
    assert_contains "$result" "test" "should contain test"

    test_case "config_get/config_set work with tmux.session_prefix"
    cache_clear
    config_set "tmux.session_prefix" "my-custom-prefix" >/dev/null 2>&1
    result=$(config_get "tmux.session_prefix")
    assert_eq "my-custom-prefix" "$result" "should read back session_prefix"
    config_set "tmux.session_prefix" "null" >/dev/null 2>&1
    result=$(config_get "tmux.session_prefix")
    assert_eq "" "$result" "null YAML value is normalized to empty"

    # ──────────────────────────────────────────────────
    # config_get null value handling (P1 fix coverage)
    # ──────────────────────────────────────────────────

    test_case "config_get returns empty for null YAML value"
    cache_clear
    rm -rf .workhive
    mkdir -p .workhive
    cat > .workhive/config.yaml <<'YAMLEOF'
version: "0.7.0"
tmux:
  session_prefix: null
YAMLEOF
    result=$(config_get "tmux.session_prefix")
    assert_eq "" "$result" "null YAML value normalized to empty"

    test_case "config_get returns consistent empty on repeated calls"
    result2=$(config_get "tmux.session_prefix")
    assert_eq "" "$result2" "repeated call should return same empty"

    test_case "config_get returns empty for missing key without default"
    cache_clear
    result=$(config_get "nonexistent.deep.key")
    assert_eq "" "$result" "missing key returns empty when no default"

    # ──────────────────────────────────────────────────
    # config_get output consistency (printf vs echo)
    # ──────────────────────────────────────────────────

    test_case "config_get output has no trailing newline in value"
    cache_clear
    cat > .workhive/config.yaml <<'YAMLEOF'
version: "0.6.0"
tmux:
  theme: "test-theme"
YAMLEOF
    result=$(config_get "tmux.theme")
    assert_eq "test-theme" "$result" "value should have no extra whitespace"

    # ──────────────────────────────────────────────────
    # Negative caching / nonexistent keys
    # config_get normalizes yq "null" to empty and falls back to default_value
    # ──────────────────────────────────────────────────

    test_case "config_get returns default for nonexistent key with default arg"
    cache_clear
    result=$(config_get "nonexistent.key.xyz" "my_default")
    assert_eq "my_default" "$result" "should return provided default"

    test_case "config_get returns empty when no default for nonexistent key"
    cache_clear
    result=$(config_get "another.nonexistent.key")
    assert_eq "" "$result"

    test_case "config_get returns default for nonexistent key"
    cache_clear
    result=$(config_get "missing.key" "hello world")
    assert_eq "hello world" "$result" "should return provided default"
else
    test_case "config_get (yq not available - skip)"
    skip_test "yq not installed"

    test_case "config_set (yq not available - skip)"
    skip_test "yq not installed"
fi

# ──────────────────────────────────────────────────
# config_check_yq_version
# ──────────────────────────────────────────────────

if command -v yq &>/dev/null; then
    test_case "config_check_yq_version passes with yq v4+"
    config_check_yq_version 2>/dev/null
    assert_eq 0 $? "yq v4 should pass"
else
    test_case "config_check_yq_version fails when yq not available"
    config_check_yq_version 2>/dev/null
    assert_eq 1 $? "missing yq should fail"
fi

# ──────────────────────────────────────────────────
# config_find_file_with_key
# ──────────────────────────────────────────────────

if command -v yq &>/dev/null; then
    test_case "config_find_file_with_key returns project config for existing key"
    _find_cfg_test_dir="${TMPDIR:-/tmp}/workhive-test-find-cfg-$$"
    rm -rf "$_find_cfg_test_dir"
    mkdir -p "$_find_cfg_test_dir/.workhive"
    cat > "$_find_cfg_test_dir/.workhive/config.yaml" <<'YAML'
layout:
  columns:
    - width: 100
      panes:
        - height: 100
          command: null
YAML
    (
        cd "$_find_cfg_test_dir" || return 1
        git init -q 2>/dev/null || true
        result=$(config_find_file_with_key "layout.columns" 2>/dev/null)
        if [[ -n "$result" ]]; then
            exit 0
        else
            exit 1
        fi
    )
    assert_eq 0 $? "should find config file with layout.columns"

    test_case "config_find_file_with_key returns empty for nonexistent key"
    (
        cd "$_find_cfg_test_dir" || return 1
        result=$(config_find_file_with_key "totally.made.up.key.xyz" 2>/dev/null)
        [[ -z "$result" ]]
    )
    assert_eq 0 $? "should return empty for nonexistent key"
    rm -rf "$_find_cfg_test_dir"
else
    test_case "config_find_file_with_key (yq not available - skip)"
    skip_test "yq not installed"
fi

test_report

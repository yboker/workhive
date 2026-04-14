#!/usr/bin/env bash

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"

source "$ROOT_DIR/test/unit/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/persistence/snapshot.sh"

test_suite "snapshot pane metadata round-trip"

# ──────────────────────────────────────────────────
# format + parse: pane with metadata
# ──────────────────────────────────────────────────

test_case "snapshot_format_pane includes metadata when pane_role is set"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "0" "pane0" "/tmp" "1" "zsh" "claude --dangerously-skip-permissions" "col_0_pane_0" "agent" "claude --dangerously-skip-permissions" "running" "boot failed" "2026-04-11T12:00:00+0800")
assert_contains "$line" '"pane_role":"col_0_pane_0"' "should contain pane_role"
assert_contains "$line" '"pane_kind":"agent"' "should contain pane_kind"
assert_contains "$line" '"pane_desired_command":"claude --dangerously-skip-permissions"' "should contain pane_desired_command"
assert_contains "$line" '"pane_state":"running"' "should contain pane_state"
assert_contains "$line" '"pane_last_error":"boot failed"' "should contain pane_last_error"
assert_contains "$line" '"pane_updated_at":"2026-04-11T12:00:00+0800"' "should contain pane_updated_at"

test_case "snapshot_format_pane omits metadata when pane_role is empty"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "0" "pane0" "/tmp" "1" "zsh" "" "" "" "" "")
assert_not_contains "$line" '"pane_role"' "should NOT contain pane_role"
assert_not_contains "$line" '"pane_kind"' "should NOT contain pane_kind"

test_case "snapshot_parse_pane_line reads metadata fields"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "1" "title1" "/home" "0" "node" "lazygit" "col_1_pane_1" "tool" "lazygit" "running" "none" "2026-04-11T12:00:01+0800")
snapshot_parse_pane_line "$line"
assert_eq "col_1_pane_1" "$_SP_PANE_ROLE" "role should match"
assert_eq "tool" "$_SP_PANE_KIND" "kind should match"
assert_eq "lazygit" "$_SP_PANE_DESIRED_CMD" "desired_command should match"
assert_eq "running" "$_SP_PANE_STATE" "state should match"
assert_eq "none" "$_SP_PANE_LAST_ERROR" "last_error should match"
assert_eq "2026-04-11T12:00:01+0800" "$_SP_PANE_UPDATED_AT" "updated_at should match"
assert_eq "wh@main" "$_SP_SESSION" "session should still parse"
assert_eq "1" "$_SP_PANE_IDX" "pane_index should still parse"

# ──────────────────────────────────────────────────
# backward compat: legacy pane without metadata
# ──────────────────────────────────────────────────

test_case "snapshot_parse_pane_line handles legacy pane (no metadata)"
line=$(snapshot_format_pane "wh@dev" "0" "0" "-" "2" "bash" "/var" "1" "bash" "vim foo.sh")
snapshot_parse_pane_line "$line"
assert_eq "wh@dev" "$_SP_SESSION" "session should parse"
assert_eq "2" "$_SP_PANE_IDX" "pane_index should parse"
assert_eq "vim foo.sh" "$_SP_FULL_CMD" "full_command should parse"
assert_eq "" "$_SP_PANE_ROLE" "role should be empty for legacy"
assert_eq "" "$_SP_PANE_KIND" "kind should be empty for legacy"
assert_eq "" "$_SP_PANE_DESIRED_CMD" "desired_command should be empty for legacy"
assert_eq "" "$_SP_PANE_STATE" "state should be empty for legacy"
assert_eq "" "$_SP_PANE_LAST_ERROR" "last_error should be empty for legacy"
assert_eq "" "$_SP_PANE_UPDATED_AT" "updated_at should be empty for legacy"

# ──────────────────────────────────────────────────
# special chars in desired_command
# ──────────────────────────────────────────────────

test_case "snapshot round-trip preserves special characters in desired_command"
line=$(snapshot_format_pane "wh@feat" "0" "1" "-" "0" "p0" "/tmp" "1" "zsh" "claude --dangerously-skip-permissions" "main" "agent" 'claude --dangerously-skip-permissions --model "opus"' "running" "" "")
snapshot_parse_pane_line "$line"
assert_eq 'claude --dangerously-skip-permissions --model "opus"' "$_SP_PANE_DESIRED_CMD" "desired_command with quotes should round-trip"

# ──────────────────────────────────────────────────
# mixed: multiple panes, some with metadata, some without
# ──────────────────────────────────────────────────

test_case "mixed panes: metadata isolation across lines"
line_with=$(snapshot_format_pane "wh@mix" "0" "1" "-" "0" "p0" "/a" "1" "zsh" "codex" "main" "agent" "codex" "running" "" "")
line_without=$(snapshot_format_pane "wh@mix" "0" "0" "-" "1" "p1" "/b" "0" "bash" "" "" "" "" "")

snapshot_parse_pane_line "$line_with"
assert_eq "agent" "$_SP_PANE_KIND" "first pane kind should be agent"
assert_eq "running" "$_SP_PANE_STATE" "first pane state should be running"

snapshot_parse_pane_line "$line_without"
assert_eq "" "$_SP_PANE_ROLE" "second pane role should be empty (reset)"
assert_eq "" "$_SP_PANE_KIND" "second pane kind should be empty (reset)"
assert_eq "" "$_SP_PANE_STATE" "second pane state should be empty (reset)"

# ──────────────────────────────────────────────────
# M3: snapshot meta includes tmux_server field
# ──────────────────────────────────────────────────

test_case "snapshot_format_meta includes tmux_server from runtime"
source "$LIB_DIR/infra/adapters/terminal/tmux_runtime.sh"
WORKHIVE_TMUX_SERVER_NAME="workhive-myproj-abc123"
meta=$(snapshot_format_meta "inst001" "myproj" "wh@inst001@myproj" "1.0.0" "0")
assert_contains "$meta" '"tmux_server":"workhive-myproj-abc123"' "meta should include tmux_server"
assert_contains "$meta" '"schema":"workhive.snapshot"' "meta should keep schema"

test_case "snapshot_format_meta tmux_server is parseable"
WORKHIVE_TMUX_SERVER_NAME="workhive-proj-xyz"
meta=$(snapshot_format_meta "inst002" "proj" "wh@inst002@proj" "2.0.0" "1")
server_val=$(snapshot_parse_json_string_field "$meta" "tmux_server")
assert_eq "workhive-proj-xyz" "$server_val" "tmux_server should be parseable from meta"
unset WORKHIVE_TMUX_SERVER_NAME 2>/dev/null || true

# ──────────────────────────────────────────────────
# E4: Snapshot format for corrected pane metadata
#
# Verifies that hermes pane is serialized with
# pane_kind=agent, matching production classification.
# ──────────────────────────────────────────────────

test_case "E4: snapshot_format_pane serializes hermes as agent"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "2" "BOBO-MAC" "/tmp" "0" "zsh" "hermes" "col_1_pane_0" "agent" "hermes" "running" "" "2026-04-12T19:44:45+0800")
assert_contains "$line" '"pane_kind":"agent"' "hermes pane must be serialized as agent"
assert_contains "$line" '"pane_desired_command":"hermes"' "hermes desired command must be preserved"
assert_contains "$line" '"pane_state":"running"' "hermes running state must be preserved"

test_case "E4: snapshot round-trip preserves hermes as agent"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "2" "BOBO-MAC" "/tmp" "0" "zsh" "hermes" "col_1_pane_0" "agent" "hermes" "running" "" "2026-04-12T19:44:45+0800")
snapshot_parse_pane_line "$line"
assert_eq "agent" "$_SP_PANE_KIND" "round-trip pane_kind must be agent"
assert_eq "hermes" "$_SP_PANE_DESIRED_CMD" "round-trip desired_command must be hermes"
assert_eq "running" "$_SP_PANE_STATE" "round-trip state must be running"

test_case "E4: restore path still prefers pane_desired_command"
line=$(snapshot_format_pane "wh@main" "0" "1" "-" "2" "t" "/tmp" "0" "zsh" '/bin/zsh -lc clear; hermes /bin/zsh' "col_1_pane_0" "agent" "hermes" "running" "" "")
snapshot_parse_pane_line "$line"
assert_eq "hermes" "$_SP_PANE_DESIRED_CMD" "restore should use pane_desired_command"
assert_contains "$_SP_FULL_CMD" "hermes" "full_command still contains hermes for forensics"

test_report

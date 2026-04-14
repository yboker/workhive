#!/usr/bin/env bash
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Unit Tests: lib/app/commands/ (help, CLI parsing)
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
export LIB_DIR="$ROOT_DIR/lib"
BIN_DIR="$ROOT_DIR/bin"
export WORKHIVE_LANG="en"

source "$SCRIPT_DIR/test_framework.sh"
source "$LIB_DIR/utils/common.sh"
source "$LIB_DIR/infra/cache/manager.sh"
source "$LIB_DIR/infra/platform/detector.sh"
source "$LIB_DIR/infra/config/loader.sh"
source "$LIB_DIR/domain/services/session_service.sh"
source "$LIB_DIR/app/commands/close_command.sh"

# Requires git repo and config
TEST_DIR="/tmp/workhive-test-commands-$$"
mkdir -p "$TEST_DIR"
cd "$TEST_DIR" || exit 1
export XDG_CONFIG_HOME="$TEST_DIR/.xdg_config"
export WORKHIVE_SKIP_GLOBAL_CHECK=1
mkdir -p "$XDG_CONFIG_HOME"
git init >/dev/null 2>&1
git config user.name "Test"
git config user.email "test@example.com"
echo "# Test" > README.md
git add README.md
git commit -m "Init" >/dev/null 2>&1
mkdir -p .workhive
cat > .workhive/config.yaml <<'EOF'
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 100
          command: null
    - width: 40
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: "wh"
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

test_suite "lib/app/commands/ (Help & CLI)"

# ──────────────────────────────────────────────────
# Main CLI help and version
# ──────────────────────────────────────────────────

test_case "workhive --help shows usage"
output=$("$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "workhive" "should contain app name"
assert_contains "$output" "init" "should list init command"
assert_contains "$output" "open" "should list open command"
assert_contains "$output" "attach" "should list attach command"
assert_contains "$output" "list" "should list list command"
assert_contains "$output" "remove" "should list remove command"
assert_contains "$output" "tools" "should list tools command"

test_case "workhive --help shows English when WORKHIVE_LANG=en"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "Usage:" "should show English Usage"
assert_contains "$output" "Commands:" "should show English Commands"

test_case "WORKHIVE_LANG=zh workhive --help shows Chinese"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "用法:" "should show Chinese 用法"
assert_contains "$output" "命令:" "should show Chinese 命令"

test_case "workhive --version shows version"
output=$("$BIN_DIR/workhive" --version 2>&1)
assert_contains "$output" "v${WORKHIVE_VERSION}"

test_case "workhive -h shows help"
output=$("$BIN_DIR/workhive" -h 2>&1)
assert_contains "$output" "workhive"

test_case "workhive -v shows version"
output=$("$BIN_DIR/workhive" -v 2>&1)
assert_contains "$output" "v${WORKHIVE_VERSION}"

test_case "workhive unknown command shows error"
output=$("$BIN_DIR/workhive" unknown_command_xyz 2>&1 || true)
assert_contains "$output" "Unknown command"

test_case "unsupported command fails without preflight side effects"
SAFE_OUTSIDE_REPO_PARENT=$(mktemp -d "${TMPDIR:-/tmp}/workhive-test-outside-XXXXXX")
NON_REPO_DIR=$(mktemp -d "$SAFE_OUTSIDE_REPO_PARENT/unknown-cmd-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" status 2>&1 || true)
assert_contains "$output" "status" "unknown command output should mention input command"
assert_not_contains "$output" "Initialize Git repository here?" "unknown command should not trigger git init prompt"
assert_not_contains "$output" "A Git repository is required for project-level operations." "unknown command should not run git repo preflight"
assert_not_contains "$output" "Git repository initialized." "unknown command should not initialize git before unknown-command handling"
if test -d "$NON_REPO_DIR/.git"; then rc=0; else rc=1; fi
assert_failure "$rc" "unknown command should not create a git repository"
rm -rf "$NON_REPO_DIR"

test_case "workhive with no args shows help"
output=$("$BIN_DIR/workhive" 2>&1)
assert_contains "$output" "workhive"

# ──────────────────────────────────────────────────
# init --help
# ──────────────────────────────────────────────────

test_case "init --help shows usage"
output=$("$BIN_DIR/workhive" init --help 2>&1)
assert_contains "$output" "workhive init"

test_case "init --help shows English when WORKHIVE_LANG=en"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" init --help 2>&1)
assert_contains "$output" "Usage:" "should show English"
assert_contains "$output" "workhive init" "should show init usage"

test_case "init -h shows help"
output=$("$BIN_DIR/workhive" init -h 2>&1)
assert_contains "$output" "workhive init"

test_case "init rejects unknown options"
output=$("$BIN_DIR/workhive" init --invalid-option 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "init rejects positional arguments"
output=$("$BIN_DIR/workhive" init something 2>&1 || true)
assert_contains "$output" "does not accept positional"

# ──────────────────────────────────────────────────
# open --help
# ──────────────────────────────────────────────────

test_case "open --help shows usage"
output=$("$BIN_DIR/workhive" open --help 2>&1)
assert_contains "$output" "workhive open"

test_case "open -h shows help"
output=$("$BIN_DIR/workhive" open -h 2>&1)
assert_contains "$output" "workhive open"

test_case "open rejects unknown options"
output=$("$BIN_DIR/workhive" open --bad 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "open with no args in non-TTY cancels gracefully"
output=$("$BIN_DIR/workhive" open --no-attach 2>&1 || true)
# Non-TTY environment: interactive selector does not appear, operation cancelled
assert_contains "$output" "Cancelled"

test_case "open help documents open vs attach difference"
output=$("$BIN_DIR/workhive" open --help 2>&1)
assert_contains "$output" "Difference from attach"

# ──────────────────────────────────────────────────
# attach --help
# ──────────────────────────────────────────────────

test_case "attach --help shows usage"
output=$("$BIN_DIR/workhive" attach --help 2>&1)
assert_contains "$output" "workhive attach"

test_case "attach with no args cancels in non-interactive mode"
output=$("$BIN_DIR/workhive" attach 2>&1 || true)
# Non-TTY environment: interactive selector does not appear, silently cancelled
assert_contains "$output" "Cancelled"

test_case "attach rejects unknown options"
output=$("$BIN_DIR/workhive" attach --xyz 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "attach command does not render raw JSON for session info"
(
    _file="$LIB_DIR/app/commands/attach_command.sh"
    if grep -qE 'session_get_info_by_name.*\|.*(grep|sed)' "$_file"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "attach should print human-readable session info instead of raw JSON"

# ──────────────────────────────────────────────────
# list --help
# ──────────────────────────────────────────────────

test_case "list --help shows usage"
output=$("$BIN_DIR/workhive" list --help 2>&1)
assert_contains "$output" "workhive list"

test_case "list rejects invalid format"
output=$("$BIN_DIR/workhive" list --format xml 2>&1 || true)
assert_contains "$output" "Invalid format"

test_case "list --format without value shows error"
output=$("$BIN_DIR/workhive" list --format 2>&1 || true)
# Should not crash with "unbound variable" under set -u
assert_contains "$output" "Invalid format" "missing value should produce friendly error"

test_case "list --format with option-like value shows error"
output=$("$BIN_DIR/workhive" list --format --sessions-only 2>&1 || true)
assert_contains "$output" "Invalid format" "option as value should produce error"

test_case "list rejects removed legacy filter options"
output=$("$BIN_DIR/workhive" list --sessions-only --worktrees-only 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "list rejects positional arguments"
output=$("$BIN_DIR/workhive" list something 2>&1 || true)
assert_contains "$output" "does not accept positional"

# ──────────────────────────────────────────────────
# remove/rm --help
# ──────────────────────────────────────────────────

test_case "remove --help shows usage"
output=$("$BIN_DIR/workhive" remove --help 2>&1)
assert_contains "$output" "remove"
assert_contains "$output" "--keep-worktree"
assert_contains "$output" "--force"

test_case "remove with no args cancels in non-interactive mode"
output=$("$BIN_DIR/workhive" remove 2>&1 || true)
# Non-TTY environment: interactive selector does not appear, silently cancelled
assert_contains "$output" "Cancelled"

test_case "rm is a shorthand for remove"
output=$("$BIN_DIR/workhive" rm --help 2>&1)
assert_contains "$output" "remove"

# ──────────────────────────────────────────────────
# list/ls shorthand
# ──────────────────────────────────────────────────

test_case "ls is a shorthand for list"
output=$("$BIN_DIR/workhive" ls --help 2>&1)
assert_contains "$output" "list"

test_case "ls produces same output as list"
_strip_spinner() { perl -pe 's/\r[^\n]*//g; s/\e\[K//g'; }
output_ls=$("$BIN_DIR/workhive" ls 2>&1 | _strip_spinner)
output_list=$("$BIN_DIR/workhive" list 2>&1 | _strip_spinner)
assert_eq "$output_ls" "$output_list" "ls and list should produce identical output"

# ──────────────────────────────────────────────────
# config command
# ──────────────────────────────────────────────────

test_case "config --help shows English when WORKHIVE_LANG=en"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" config --help 2>&1)
assert_contains "$output" "Usage:" "should show English"
assert_contains "$output" "workhive config" "should show config usage"

test_case "config shows edit instruction"
output=$("$BIN_DIR/workhive" config 2>&1)
assert_contains "$output" "config.yaml"

test_case "config --layout previews current layout before modification"
output=$("$BIN_DIR/workhive" config --layout 2>&1)
assert_contains "$output" "Current config summary" "should preview config/layout before asking"
assert_contains "$output" "Layout mode:" "should display current layout section"
assert_contains "$output" "Cancelled" "non-interactive mode should cancel layout edit"

test_case "tools manage --help is supported"
output=$("$BIN_DIR/workhive" tools manage --help 2>&1)
assert_contains "$output" "Usage: workhive tools manage"
assert_contains "$output" "Purpose:"
assert_contains "$output" "Available actions:"
assert_contains "$output" "Done"

test_case "tools list --help is supported"
output=$("$BIN_DIR/workhive" tools list --help 2>&1)
assert_contains "$output" "Usage: workhive tools list"

test_case "tools list ai rejects legacy positional filter syntax"
output=$("$BIN_DIR/workhive" tools list ai 2>&1 || true)
assert_contains "$output" "tools list does not accept positional arguments" "list should reject legacy ai positional filter"
assert_contains "$output" "Usage: workhive tools list" "legacy ai syntax should show list usage"

test_case "tools validate --help is supported"
output=$("$BIN_DIR/workhive" tools validate --help 2>&1)
assert_contains "$output" "Usage: workhive tools validate"

if command -v yq &>/dev/null; then
    test_case "config git-whitelist --help is supported"
    output=$("$BIN_DIR/workhive" config git-whitelist --help 2>&1)
    assert_contains "$output" "Usage: workhive config git-whitelist"
    assert_contains "$output" "manage"

    test_case "config git-whitelist manage --help includes purpose and actions"
    output=$("$BIN_DIR/workhive" config git-whitelist manage --help 2>&1)
    assert_contains "$output" "Usage: workhive config git-whitelist manage"
    assert_contains "$output" "Purpose:"
    assert_contains "$output" "Available actions:"
    assert_contains "$output" "Remove path"

    test_case "config git-whitelist list reads effective config from defaults when global is absent"
    rm -f "$XDG_CONFIG_HOME/workhive/config.yaml"
    cache_clear
    output=$("$BIN_DIR/workhive" config git-whitelist list 2>&1)
    assert_contains "$output" "Source: Built-in default"
    assert_contains "$output" "Git init whitelist is empty"

    test_case "config git-whitelist list reads from global config"
    mkdir -p "$XDG_CONFIG_HOME/workhive"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'EOF'
version: "0.9.0"
git_init:
  whitelist_paths:
    - "/tmp/workhive-safe"
EOF
    cache_clear
    cache_invalidate "config:*" 2>/dev/null || true
    output=$("$BIN_DIR/workhive" config git-whitelist list 2>&1)
    assert_contains "$output" "Source: Global config"

    test_case "config git-whitelist manage requires tty in non-interactive shell"
    output=$("$BIN_DIR/workhive" config git-whitelist manage 2>&1 || true)
    assert_contains "$output" "requires a TTY"
    assert_contains "$output" "workhive config git-whitelist"

    test_case "config git-whitelist edit validates required args"
    output=$("$BIN_DIR/workhive" config git-whitelist edit /tmp/only-one-arg 2>&1 || true)
    assert_contains "$output" "Usage: workhive config git-whitelist edit"

    test_case "config git-whitelist manage remove rejects positional paths"
    output=$("$BIN_DIR/workhive" config git-whitelist manage remove /tmp/workhive-safe 2>&1 || true)
    assert_contains "$output" "config git-whitelist manage does not accept positional arguments" "manage should reject scripted remove path input"
    assert_contains "$output" "Usage: workhive config git-whitelist manage" "manage remove regression should preserve help usage"
fi

# ──────────────────────────────────────────────────
# pane --help and subcommands
# ──────────────────────────────────────────────────

test_case "pane --help shows overview with all subcommands"
output=$("$BIN_DIR/workhive" pane --help 2>&1)
assert_contains "$output" "workhive pane <subcommand>" "should show subcommand usage"
assert_contains "$output" "open" "should list open subcommand"
assert_contains "$output" "close" "should list close subcommand"
assert_contains "$output" "split" "should list split subcommand"
assert_contains "$output" "send" "should list send subcommand"
assert_contains "$output" "list" "should list list subcommand"

test_case "pane -h shows help"
output=$("$BIN_DIR/workhive" pane -h 2>&1)
assert_contains "$output" "workhive pane"

test_case "pane with no args shows help (not tmux error)"
unset TMUX
output=$("$BIN_DIR/workhive" pane 2>&1)
assert_contains "$output" "workhive pane"

test_case "pane rejects unknown options"
output=$("$BIN_DIR/workhive" pane --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "pane rejects unknown subcommand"
output=$("$BIN_DIR/workhive" pane nonexistent 2>&1 || true)
assert_contains "$output" "Unknown command"

# ── pane open ──

test_case "pane open --help shows open-specific help"
output=$("$BIN_DIR/workhive" pane open --help 2>&1)
assert_contains "$output" "workhive pane open" "should show open-specific usage"
assert_contains "$output" "--cmd" "should document --cmd option"
assert_contains "$output" "--force" "should document --force option"
assert_not_contains "$output" "workhive pane <subcommand>" "should NOT show overview help"

test_case "pane open --help (en) shows English"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" pane open --help 2>&1)
assert_contains "$output" "Usage:" "should show English"

test_case "pane open --help (zh) shows Chinese"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane open --help 2>&1)
assert_contains "$output" "用法:" "should show Chinese"

test_case "pane open requires tmux (fails outside tmux)"
unset TMUX
output=$("$BIN_DIR/workhive" pane open 2>&1 || true)
assert_contains "$output" "tmux"

test_case "pane open rejects unknown options"
unset TMUX
output=$("$BIN_DIR/workhive" pane open --bad 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "pane open --cmd without value shows error"
output=$("$BIN_DIR/workhive" pane open dev01 --cmd 2>&1 || true)
assert_contains "$output" "requires a value" "should show missing cmd value error"

# ── pane attach removed ──

test_case "pane attach is removed (unknown subcommand)"
output=$("$BIN_DIR/workhive" pane attach 2>&1 || true)
assert_contains "$output" "Unknown command"
assert_contains "$output" "workhive pane <subcommand>" "should show pane overview help"

# ── pane close ──

test_case "pane close --help shows close-specific help"
output=$("$BIN_DIR/workhive" pane close --help 2>&1)
assert_contains "$output" "workhive pane close" "should show close-specific usage"
assert_contains "$output" "--force" "should document --force option"
assert_not_contains "$output" "workhive pane <subcommand>" "should NOT show overview"

test_case "pane close requires tmux (fails outside tmux)"
unset TMUX
output=$("$BIN_DIR/workhive" pane close 2>&1 || true)
assert_contains "$output" "tmux"

test_case "pane close rejects unknown options"
unset TMUX
output=$("$BIN_DIR/workhive" pane close --bad 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "pane close --id without value shows error"
unset TMUX
output=$("$BIN_DIR/workhive" pane close --id 2>&1 || true)
assert_contains "$output" "Missing value for --id"

test_case "pane close rejects index and --id together"
unset TMUX
output=$("$BIN_DIR/workhive" pane close 1 --id %3 2>&1 || true)
assert_contains "$output" "cannot be used together"

# ── pane split ──

test_case "pane split --help shows split-specific help"
output=$("$BIN_DIR/workhive" pane split --help 2>&1)
assert_contains "$output" "workhive pane split" "should show split-specific usage"
assert_contains "$output" "--cmd" "should document --cmd option"
assert_contains "$output" "--size" "should document --size option"
assert_not_contains "$output" "workhive pane <subcommand>" "should NOT show overview"

test_case "pane split requires tmux (fails outside tmux)"
unset TMUX
output=$("$BIN_DIR/workhive" pane split 2>&1 || true)
assert_contains "$output" "tmux"

test_case "pane split rejects unknown options"
unset TMUX
output=$("$BIN_DIR/workhive" pane split --bad 2>&1 || true)
assert_contains "$output" "Unknown option"

# ── pane send ──

test_case "pane send --help shows send-specific help"
output=$("$BIN_DIR/workhive" pane send --help 2>&1)
assert_contains "$output" "workhive pane send" "should show send-specific usage"
assert_contains "$output" "pane_index" "should document pane_index arg"
assert_contains "$output" "--all" "should document --all option"
assert_contains "$output" "--commit" "should document --commit option"
assert_contains "$output" "--diagnose" "should document --diagnose option"
assert_not_contains "$output" "workhive pane <subcommand>" "should NOT show overview"

test_case "pane send --help (en) shows English"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" pane send --help 2>&1)
assert_contains "$output" "Usage:" "should show English"

test_case "pane send --help (zh) shows Chinese"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane send --help 2>&1)
assert_contains "$output" "用法:" "should show Chinese"

test_case "pane send requires tmux (fails outside tmux)"
unset TMUX
output=$("$BIN_DIR/workhive" pane send 1 "test" 2>&1 || true)
assert_contains "$output" "tmux"

test_case "pane send without args shows usage error"
unset TMUX
output=$("$BIN_DIR/workhive" pane send 2>&1 || true)
assert_contains "$output" "Usage" "should show usage error for missing args"

# ── pane list ──

test_case "pane list --help shows list-specific help"
output=$("$BIN_DIR/workhive" pane list --help 2>&1)
assert_contains "$output" "workhive pane list" "should show list-specific usage"
assert_not_contains "$output" "workhive pane <subcommand>" "should NOT show overview"

test_case "pane list --help (en) shows English"
output=$(WORKHIVE_LANG=en "$BIN_DIR/workhive" pane list --help 2>&1)
assert_contains "$output" "Usage:" "should show English"

test_case "pane list --help (zh) shows Chinese"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane list --help 2>&1)
assert_contains "$output" "用法:" "should show Chinese"

test_case "pane list requires tmux (fails outside tmux)"
unset TMUX
output=$("$BIN_DIR/workhive" pane list 2>&1 || true)
assert_contains "$output" "tmux"

test_case "pane list rejects unknown options"
unset TMUX
output=$("$BIN_DIR/workhive" pane list --bad 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "pane ls alias works same as pane list"
output=$("$BIN_DIR/workhive" pane ls --help 2>&1)
assert_contains "$output" "workhive pane list" "should show list help for ls alias"

# ── pane layout removed ──

test_case "pane layout is removed (unknown subcommand)"
output=$("$BIN_DIR/workhive" pane layout 2>&1 || true)
assert_contains "$output" "Unknown command"
assert_contains "$output" "workhive pane <subcommand>" "should show pane overview help"

# ── pane overview: bilingual ──

test_case "pane help shows in Chinese when WORKHIVE_LANG=zh"
output=$(WORKHIVE_LANG=zh "$BIN_DIR/workhive" pane --help 2>&1)
assert_contains "$output" "子命令"
assert_contains "$output" "open"
assert_contains "$output" "send"
assert_contains "$output" "list"

# ── each subcommand help is distinct from overview ──

test_case "subcommand helps are distinct from pane overview"
overview=$("$BIN_DIR/workhive" pane --help 2>&1)
open_help=$("$BIN_DIR/workhive" pane open --help 2>&1)
close_help=$("$BIN_DIR/workhive" pane close --help 2>&1)
split_help=$("$BIN_DIR/workhive" pane split --help 2>&1)
send_help=$("$BIN_DIR/workhive" pane send --help 2>&1)
list_help=$("$BIN_DIR/workhive" pane list --help 2>&1)
assert_ne "$overview" "$open_help" "open help should differ from overview"
assert_ne "$overview" "$close_help" "close help should differ from overview"
assert_ne "$overview" "$split_help" "split help should differ from overview"
assert_ne "$overview" "$send_help" "send help should differ from overview"
assert_ne "$overview" "$list_help" "list help should differ from overview"

# ──────────────────────────────────────────────────
# destroy --help
# ──────────────────────────────────────────────────

test_case "destroy --help shows usage"
output=$("$BIN_DIR/workhive" destroy --help 2>&1)
assert_contains "$output" "destroy"
assert_contains "$output" "--force"

test_case "destroy -h shows help"
output=$("$BIN_DIR/workhive" destroy -h 2>&1)
assert_contains "$output" "destroy"

test_case "destroy rejects unknown options"
output=$("$BIN_DIR/workhive" destroy --invalid 2>&1 || true)
assert_contains "$output" "Unknown option"

test_case "main help lists pane command"
output=$("$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "pane"

test_case "main help lists destroy command"
output=$("$BIN_DIR/workhive" --help 2>&1)
assert_contains "$output" "destroy"

# ──────────────────────────────────────────────────
# config command dependency resolution
# ──────────────────────────────────────────────────

test_case "config_command.sh sources init_command.sh dependencies"
(
    source "$LIB_DIR/app/commands/config_command.sh" 2>/dev/null
    if type -t _init_build_available_commands &>/dev/null; then
        exit 0
    else
        exit 1
    fi
)
assert_eq "0" "$?" "_init_build_available_commands should be available after sourcing config_command.sh"

test_case "config_command.sh has _config_command_modify_layout available"
(
    source "$LIB_DIR/app/commands/config_command.sh" 2>/dev/null
    if type -t _config_command_modify_layout &>/dev/null; then
        exit 0
    else
        exit 1
    fi
)
assert_eq "0" "$?" "_config_command_modify_layout should be available after sourcing config_command.sh"

# ──────────────────────────────────────────────────
# --debug mode
# ──────────────────────────────────────────────────

test_case "debug mode keeps list dashboard functional"
output=$("$BIN_DIR/workhive" --debug list 2>&1)
assert_contains "$output" "Dashboard" "should still render list dashboard"

# ──────────────────────────────────────────────────
# init --non-interactive (dependency detection)
# ──────────────────────────────────────────────────

test_case "init --non-interactive --force regenerates config"
"$BIN_DIR/workhive" init --non-interactive --force >/dev/null 2>&1
assert_eq "0" "$?"

test_case "init --non-interactive --force writes persistence retention defaults"
config_after_init=$(cat .workhive/config.yaml 2>/dev/null || true)
assert_contains "$config_after_init" "auto_save_mode: hybrid"
assert_contains "$config_after_init" "auto_save_debounce: 3s"
assert_contains "$config_after_init" "auto_save_interval: 10m"
assert_contains "$config_after_init" "snapshot_history_limit: 8"
assert_contains "$config_after_init" "snapshot_max_size_mb: 8"

test_case "_init_config_tx_begin defaults seed skips copying existing config"
source "$LIB_DIR/app/commands/init_command.sh"
_saved_cp_fn="$(declare -f cp 2>/dev/null || true)"
_init_tx_cp_called=0
cp() {
    _init_tx_cp_called=$((_init_tx_cp_called + 1))
    command cp "$@"
}
_init_tx_tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/workhive-init-tx.XXXXXX")"
_init_tx_final="$_init_tx_tmpdir/config.yaml"
cat > "$_init_tx_final" <<'EOF'
legacy: true
EOF
_init_prev_exit_trap="$(trap -p EXIT || true)"
_init_config_tx_begin "$_init_tx_final" "global" "defaults"
assert_eq "0" "$_init_tx_cp_called" "defaults seed should avoid copying legacy config"
_init_config_tx_rollback >/dev/null 2>&1 || true
trap - EXIT ERR
if [[ -n "$_init_prev_exit_trap" ]]; then
    eval "$_init_prev_exit_trap"
fi
rm -rf "$_init_tx_tmpdir"
unset -f cp
[[ -n "$_saved_cp_fn" ]] && eval "$_saved_cp_fn"

test_case "_init_config_tx_begin existing seed preserves copy behavior"
source "$LIB_DIR/app/commands/init_command.sh"
_saved_cp_fn="$(declare -f cp 2>/dev/null || true)"
_init_tx_cp_called=0
cp() {
    _init_tx_cp_called=$((_init_tx_cp_called + 1))
    command cp "$@"
}
_init_tx_tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/workhive-init-tx.XXXXXX")"
_init_tx_final="$_init_tx_tmpdir/config.yaml"
cat > "$_init_tx_final" <<'EOF'
legacy: true
EOF
_init_prev_exit_trap="$(trap -p EXIT || true)"
_init_config_tx_begin "$_init_tx_final" "global"
assert_eq "1" "$_init_tx_cp_called" "existing seed should keep copy-first behavior"
_init_config_tx_rollback >/dev/null 2>&1 || true
trap - EXIT ERR
if [[ -n "$_init_prev_exit_trap" ]]; then
    eval "$_init_prev_exit_trap"
fi
rm -rf "$_init_tx_tmpdir"
unset -f cp
[[ -n "$_saved_cp_fn" ]] && eval "$_saved_cp_fn"

test_case "_init_config_tx_begin trap should not swallow INT/TERM"
(
    trap_line=$(sed -n '/_init_config_tx_begin()/,/^}/p' "$LIB_DIR/app/commands/init_command.sh" | grep -E "trap '.*_init_config_tx_rollback" | head -1 || true)
    [[ -n "$trap_line" ]] || exit 1
    if echo "$trap_line" | grep -Eq '\bINT\b|\bTERM\b'; then
        exit 1
    fi
    if ! echo "$trap_line" | grep -Eq '\bEXIT\b' || ! echo "$trap_line" | grep -Eq '\bERR\b'; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "init config transaction trap should only use EXIT/ERR"

test_case "_init_restore_global_tools_yaml supports in-memory YAML snapshot"
(
    source "$LIB_DIR/app/commands/init_command.sh"
    _init_tools_tmpdir="$(mktemp -d "${TMPDIR:-/tmp}/workhive-init-tools-yaml.XXXXXX")"
    _init_tools_cfg="$_init_tools_tmpdir/config.yaml"
    cat > "$_init_tools_cfg" <<'EOF'
version: "0.9.0"
tools:
  default: "__empty__"
  list: []
EOF

    snapshot_yaml='default: codex
list:
  - tool_name: codex
    label: Codex
    command: codex
    is_ai: true
    enabled: true'

    _init_restore_global_tools_yaml "$_init_tools_cfg" "$snapshot_yaml" >/dev/null 2>&1 || exit 1
    restored_default=$(yq -r '.tools.default // ""' "$_init_tools_cfg" 2>/dev/null || echo "")
    restored_cmd=$(yq -r '.tools.list[0].command // ""' "$_init_tools_cfg" 2>/dev/null || echo "")
    [[ "$restored_default" == "codex" ]] || exit 1
    [[ "$restored_cmd" == "codex" ]] || exit 1
    rm -rf "$_init_tools_tmpdir"
    exit 0
)
assert_eq "0" "$?" "init tools snapshot restore should work without temp files"

test_case "ask_yes_no contains explicit INT interruption handling"
(
    fn_body=$(sed -n '/ask_yes_no()/,/^}/p' "$LIB_DIR/utils/common.sh")
    echo "$fn_body" | grep -q "_ask_interrupted=true" || exit 1
    echo "$fn_body" | grep -q "return 130" || exit 1
    exit 0
)
assert_eq "0" "$?" "ask_yes_no should map Ctrl+C interruption to return code 130"

test_case "init --global propagates ask_yes_no interruption"
(
    source "$LIB_DIR/app/commands/init_command.sh"
    _save_xdg="$XDG_CONFIG_HOME"
    export XDG_CONFIG_HOME="$TEST_DIR/.xdg_init_interrupt_$$"
    mkdir -p "$XDG_CONFIG_HOME/workhive"
    cat > "$XDG_CONFIG_HOME/workhive/config.yaml" <<'EOF'
version: "0.9.0"
layout:
  columns:
    - width: 100
      panes:
        - height: 100
          command: null
EOF
    ask_yes_no() { return 130; }
    init_command_execute --global >/dev/null 2>&1
    rc=$?
    unset -f ask_yes_no
    export XDG_CONFIG_HOME="$_save_xdg"
    [[ $rc -eq 130 ]] || exit 1
    exit 0
)
assert_eq "0" "$?" "init global overwrite prompt interruption should abort with 130"

# ──────────────────────────────────────────────────
# list output in various formats
# ──────────────────────────────────────────────────

# First create a workspace for testing list
if tmux_usable; then
    "$BIN_DIR/workhive" open cmd-test-$$ --no-attach >/dev/null 2>&1 || true

    test_case "list --format table shows table"
    output=$("$BIN_DIR/workhive" list --format table 2>&1)
    assert_contains "$output" "cmd-test-$$"

    test_case "list --format json produces JSON object"
    output=$("$BIN_DIR/workhive" list --format json 2>&1)
    assert_contains "$output" '"summary"'
    assert_contains "$output" '"workspaces"'

    # Cleanup
    while IFS= read -r s; do
        [[ -z "$s" ]] && continue
        case "$s" in
            "wh@$(basename "$TEST_DIR")@cmd-test-$$")
                tmux kill-session -t "$s" 2>/dev/null || true
                ;;
        esac
    done < <(tmux list-sessions -F '#{session_name}' 2>/dev/null || true)
fi

# ──────────────────────────────────────────────────
# attach interactive selector: stdout should not leak info messages
# ──────────────────────────────────────────────────

test_case "attach interactive select outputs only branch name to stdout (no log_info leak)"
(
    _file="$LIB_DIR/app/commands/attach_command.sh"
    _in_select=false
    _leak_found=false
    while IFS= read -r _line; do
        if [[ "$_line" =~ _attach_command_interactive_select\(\) ]]; then
            _in_select=true
            continue
        fi
        if [[ "$_in_select" == true ]]; then
            if echo "$_line" | grep -qE '^\s*(log_info|log_error|log_warn)\s' && ! echo "$_line" | grep -q '/dev/tty'; then
                _leak_found=true
                break
            fi
            if echo "$_line" | grep -qE '^\s*echo\s+""\s*$' && ! echo "$_line" | grep -q '/dev/tty'; then
                _leak_found=true
                break
            fi
            if echo "$_line" | grep -qE '^\s*echo\s+"  workhive' && ! echo "$_line" | grep -q '/dev/tty'; then
                _leak_found=true
                break
            fi
        fi
    done < "$_file"

    if [[ "$_leak_found" == true ]]; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "log_info/echo in _attach_command_interactive_select should redirect to /dev/tty"

# ──────────────────────────────────────────────────
# TUI: _first_read logic removed (no longer requires pressing Enter twice)
# ──────────────────────────────────────────────────

test_case "tui_select_menu does not have _first_read double-enter guard"
(
    if grep -q '_first_read' "$LIB_DIR/utils/tui.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "tui.sh should not contain _first_read (double-enter bug)"

test_case "tui frame supports scenario-based redraw mode (tmux anchor / terminal lines)"
(
    if ! grep -q '_cursor_save()' "$LIB_DIR/utils/tui.sh"; then
        exit 1
    fi
    if ! grep -q '_cursor_restore()' "$LIB_DIR/utils/tui.sh"; then
        exit 1
    fi
    if ! grep -q 'WORKHIVE_TUI_REDRAW_MODE' "$LIB_DIR/utils/tui.sh"; then
        exit 1
    fi
    if ! grep -q '\[\[ -n "\${TMUX:-}" \]\]' "$LIB_DIR/utils/tui.sh"; then
        exit 1
    fi
    exit 0
)
assert_eq "0" "$?" "tui redraw should support tmux anchor + terminal line-rewind modes"

test_case "tui_select_menu falls back to default in non-interactive shell"
if ! command -v python3 >/dev/null 2>&1; then
    skip_test "python3 not installed"
else
    _tui_non_tty_rc=0
    _tui_non_tty_out=$(LIB_DIR_FOR_TEST="$LIB_DIR" python3 - <<'PY'
import os
import subprocess
import sys

lib_dir = os.environ["LIB_DIR_FOR_TEST"]
script = (
    'set -eEuo pipefail; '
    f'LIB_DIR="{lib_dir}"; '
    'source "$LIB_DIR/utils/tui.sh"; '
    'opts=("left" "right"); '
    'TUI_SELECTED_INDEX=""; '
    'tui_select_menu "Prompt" opts 1 ""; '
    'printf "idx=%s\\n" "$TUI_SELECTED_INDEX"'
)
result = subprocess.run(
    ["bash", "-lc", script],
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
    start_new_session=True,
)
sys.stdout.write(result.stdout)
sys.exit(result.returncode)
PY
) || _tui_non_tty_rc=$?
    assert_success "$_tui_non_tty_rc" "should not error when /dev/tty is not available"
    assert_contains "$_tui_non_tty_out" "idx=1" "should return default index"
fi

test_case "cursor restore is safe when tty is unavailable"
if ! command -v python3 >/dev/null 2>&1; then
    skip_test "python3 not installed"
else
    _tui_cursor_rc=0
    _tui_cursor_out=$(LIB_DIR_FOR_TEST="$LIB_DIR" python3 - <<'PY'
import os
import subprocess
import sys

lib_dir = os.environ["LIB_DIR_FOR_TEST"]
script = (
    'set -eEuo pipefail; '
    f'LIB_DIR="{lib_dir}"; '
    'source "$LIB_DIR/utils/tui.sh"; '
    '_cursor_show; '
    'printf "cursor-ok\\n"'
)
result = subprocess.run(
    ["bash", "-lc", script],
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
    start_new_session=True,
)
sys.stdout.write(result.stdout)
sys.exit(result.returncode)
PY
) || _tui_cursor_rc=$?
    assert_success "$_tui_cursor_rc" "_cursor_show should be no-op if tty write fails"
    assert_contains "$_tui_cursor_out" "cursor-ok" "script should continue after _cursor_show"
fi

test_case "tui_command_select custom input falls back to null when tty is unavailable"
if ! command -v python3 >/dev/null 2>&1; then
    skip_test "python3 not installed"
else
    _tui_custom_rc=0
    _tui_custom_out=$(LIB_DIR_FOR_TEST="$LIB_DIR" python3 - <<'PY'
import os
import subprocess
import sys

lib_dir = os.environ["LIB_DIR_FOR_TEST"]
script = (
    'set -eEuo pipefail; '
    f'LIB_DIR="{lib_dir}"; '
    'source "$LIB_DIR/utils/tui.sh"; '
    'labels=("Shell" "Custom input..."); '
    'values=("__empty__" "__custom__"); '
    'TUI_SELECTED_CMD=""; '
    'tui_command_select "Select" labels values 1; '
    'printf "cmd=%s\\n" "$TUI_SELECTED_CMD"'
)
result = subprocess.run(
    ["bash", "-lc", script],
    stdin=subprocess.DEVNULL,
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
    start_new_session=True,
)
sys.stdout.write(result.stdout)
sys.exit(result.returncode)
PY
) || _tui_custom_rc=$?
    assert_success "$_tui_custom_rc" "custom input path should not crash when /dev/tty is unavailable"
    assert_contains "$_tui_custom_out" "cmd=null" "should fall back to null when custom input cannot be read"
fi

# ──────────────────────────────────────────────────
# Did-you-mean: pane subcommands used at top level
# ──────────────────────────────────────────────────

test_case "workhive close outside tmux shows error"
_dym_output=$("$BIN_DIR/workhive" close 2>&1 || true)
assert_contains "$_dym_output" "tmux" "should mention tmux requirement"

test_case "close_command accepts sanitized workhive session prefix"
cache_clear
if command -v yq &>/dev/null; then
    cat > .workhive/config.yaml <<'EOF'
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 100
          command: null
    - width: 40
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: "bad'prefix;rm -rf /"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF
    cache_invalidate "config:*" 2>/dev/null || true
    export TMUX="/tmp/fake,1,0"
    terminal_adapter_get_current_session() { echo "badprefixrm-rf@cmd-test"; }
    _close_do_detach() { return 0; }
    rc=0
    close_command_execute --force >/dev/null 2>&1 || rc=$?
    unset -f terminal_adapter_get_current_session _close_do_detach
    unset TMUX
    cat > .workhive/config.yaml <<'EOF'
version: "0.8.0"
layout:
  columns:
    - width: 60
      panes:
        - height: 100
          command: null
    - width: 40
      panes:
        - height: 50
          command: null
        - height: 50
          command: null
tmux:
  session_prefix: "wh"
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
EOF
    cache_invalidate "config:*" 2>/dev/null || true
    assert_success "$rc" "should recognize sanitized current session as workhive session"
else
    skip_test "yq not installed"
fi

test_case "_close_do_kill detaches before killing current session"
_close_orig_detach=$(declare -f session_detach || true)
_close_orig_kill_by_name=$(declare -f session_kill_by_name || true)
_close_orig_cache_invalidate=$(declare -f cache_invalidate || true)

_close_call_seq=""
session_detach() { _close_call_seq="${_close_call_seq}D"; return 0; }
session_kill_by_name() { _close_call_seq="${_close_call_seq}K"; return 0; }
cache_invalidate() { return 0; }

_close_do_kill "main" "wh@test@main" >/dev/null 2>&1
rc=$?
assert_eq "0" "$rc" "terminate action should succeed"
assert_eq "DK" "$_close_call_seq" "close kill path should detach first, then kill session"

unset -f session_detach session_kill_by_name cache_invalidate
[[ -n "$_close_orig_detach" ]] && eval "$_close_orig_detach"
[[ -n "$_close_orig_kill_by_name" ]] && eval "$_close_orig_kill_by_name"
[[ -n "$_close_orig_cache_invalidate" ]] && eval "$_close_orig_cache_invalidate"

# ══════════════════════════════════════════════════
# Command capability matrix contract tests
# Verify that L0/L1/L2 command classes receive correct
# preflight treatment (git/init/tmux/full-stack/plugins)
# ══════════════════════════════════════════════════

echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${BLUE}  Command Capability Matrix Contract Tests${NC}"
echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"

SAFE_OUTSIDE_REPO_PARENT=$(mktemp -d "${TMPDIR:-/tmp}/workhive-test-outside-XXXXXX")

test_case "L0: _internal_completions zsh bypasses git/tmux/init preflight"
NON_REPO_DIR=$(mktemp -d "$SAFE_OUTSIDE_REPO_PARENT/l0-comp-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" _internal_completions zsh 2>&1)
rc=$?
assert_success "$rc" "_internal_completions zsh should succeed outside git repo"
assert_contains "$output" "#compdef" "should output zsh completion"
assert_not_contains "$output" "Initialize Git repository" "should not trigger git init prompt"
assert_not_contains "$output" "not initialized" "should not check init"
rm -rf "$NON_REPO_DIR"

test_case "L0: shell --help bypasses git/tmux/init preflight"
NON_REPO_DIR=$(mktemp -d "$SAFE_OUTSIDE_REPO_PARENT/l0-shell-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" shell --help 2>&1)
rc=$?
assert_success "$rc" "shell --help should succeed outside git repo"
assert_not_contains "$output" "Initialize Git repository" "should not trigger git init prompt"
rm -rf "$NON_REPO_DIR"

test_case "L0: list --help keeps subcommand-help fast path"
output=$("$BIN_DIR/workhive" list --help 2>&1)
assert_contains "$output" "workhive list" "should show list help"
assert_success "$?" "should succeed"

test_case "L0: doctor --help keeps subcommand-help fast path"
output=$("$BIN_DIR/workhive" doctor --help 2>&1)
assert_contains "$output" "workhive doctor" "should show doctor help"
assert_success "$?" "should succeed"

test_case "L1: doctor runs outside initialized repo without git/init prompts"
NON_REPO_DIR=$(mktemp -d "$SAFE_OUTSIDE_REPO_PARENT/l1-doc-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" doctor 2>&1)
assert_not_contains "$output" "Initialize Git repository" "doctor should not trigger git init prompt"
assert_not_contains "$output" "not initialized" "doctor should not check init"
rm -rf "$NON_REPO_DIR"

test_case "unknown command fails before any preflight side effects"
NON_REPO_DIR=$(mktemp -d "$SAFE_OUTSIDE_REPO_PARENT/removed-cmd-XXXXXX")
output=$(cd "$NON_REPO_DIR" && "$BIN_DIR/workhive" status 2>&1 || true)
assert_not_contains "$output" "Initialize Git repository" "unknown command should not trigger git init"
if test -d "$NON_REPO_DIR/.git"; then _rc=0; else _rc=1; fi
assert_failure "$_rc" "unknown command should not create git repo"
rm -rf "$NON_REPO_DIR"

test_case "L1: _command_capability function exists in bin/workhive"
(grep -q '_command_capability' "$BIN_DIR/workhive" && exit 0 || exit 1)
assert_eq "0" "$?" "bin/workhive should have _command_capability function"

rm -rf "$SAFE_OUTSIDE_REPO_PARENT"

test_report

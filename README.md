[English](README.md) | [Chinese](README.zh-CN.md)

# workhive

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Shell: Bash 4.3+](https://img.shields.io/badge/Shell-Bash%204.3%2B-blue.svg)](https://www.gnu.org/software/bash/)
[![Platform: macOS / Linux / WSL2](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20WSL2-lightgrey.svg)]()

**One command to orchestrate your panes, commands, and sessions.**  
A terminal workspace orchestrator built on Git worktrees and tmux.

[Pain Points](#have-you-faced-these-problems) • [Solution](#how-workhive-solves-them) • [Quick Start](#quick-start) • [Commands](#command-reference) • [Best Practices](#best-practices) • [Configuration](#configuration)

---

## Have You Faced These Problems?

### Branch Switching Hell

> You're deep in `feature-a` with Claude Code running, when a hotfix lands. `git stash` → switch branch → fix bug → switch back → `git stash pop` → merge conflicts...

workhive gives each branch its **own isolated working directory**. Develop in parallel with zero interference. Switch branches? You don't—you keep multiple branches open at once, each fully independent.

### SSH Disconnect, Environment Gone

> SSH into a remote server to run Claude Code. Network hiccups, connection drops. Process gone, context lost, start over from scratch.

workhive runs everything inside tmux. **Nothing is lost on disconnect while the tmux session is still running.** Reconnect via SSH, run `workhive attach feature-x`, and your three-pane layout, Claude Code session, and Lazygit state are exactly as you left them.

### Rebuilding Your Environment Every Day

> Open terminal, manually split panes, launch Claude Code, open another pane for Lazygit, another for diff review... repeat this ritual daily.

`workhive open feature-x`—one command. Creates the worktree, starts the tmux session, applies the three-pane layout, and launches your configured tools in each pane. **Zero manual setup. Straight into flow state.**

### Remote Server, No Access to Your Session

> Away from your desk, phone buzzes with an alert. SSH into the jump host, but what was that tmux session called? The service was running in the foreground and died with the connection.

workhive sessions **persist by design**. All processes run inside tmux; disconnects don't affect them. `workhive ls` shows active sessions, and `workhive attach` reconnects to any running one. SSH from your phone + Claude Code `--dangerously-skip-permissions`, and fix issues from the couch.

### Not Enough Windows for AI Coding

> Claude Code writing code on one side, need git diff for review, and a shell for tests. Manual splits, resizing, constant switching—attention shattered.

workhive's three-pane layout is built for AI coding: Claude Code in one pane, Lazygit in another, Shell in the third. **Layout is preconfigured. Focus on coding.**

---

## How workhive Solves Them

| Pain Point | workhive Solution |
|------------|-------------------|
| Branch switching requires stash / conflicts | Each branch gets its own Git Worktree—parallel development, zero interference |
| SSH disconnect loses environment | tmux keeps sessions running; `workhive attach` reconnects to running sessions |
| Manual environment setup every day | `workhive open` in one command: worktree + tmux + layout + tool launch |
| Remote firefighting, can't find sessions | `workhive ls` lists all workspaces, `workhive attach` connects directly |
| AI coding window management chaos | Preset three-pane layout: Claude Code + Lazygit + Shell, each in place |
| Environment not reproducible | `.workhive/config.yaml` is version-controlled; teams share the same setup |

### Who Is workhive For?

- **Heavy AI coding users** — Daily use of Claude Code / Codex / OpenCode and similar tools; need multi-pane workflows
- **Remote developers** — SSH development; need disconnect recovery and fast reattach + snapshot restore
- **Parallel branch developers** — Multiple features or hotfixes at once; tired of stash and branch switching
- **CLI enthusiasts** — Prefer terminal workflows; value efficiency and automation

### What It Isn't

workhive is **not** a terminal emulator. It is a **workflow orchestrator inside the terminal**. Use any terminal you like; workhive manages the development environment inside it.

---

## Core Features

- **Deterministic workspace open** — `workhive open <branch>` always follows one strategy: reuse live session, otherwise restore healthy snapshot, otherwise rebuild from `.workhive/config.yaml`
- **Force rebuild when needed** — `workhive open -f <branch>` skips reuse/restore and rebuilds strictly from config
- **Dedicated tmux runtime** — Each project instance uses its own tmux server; it does not interfere with your default tmux server
- **Parallel branch isolation** — Each branch gets its own worktree, session, panes, and runtime metadata
- **Fast status paths** — `workhive ls` uses bulk collection; `--watch` and `--format json` use the lighter fast strategy
- **Snapshot restore** — Built-in tmux snapshot save/restore for windows, panes, layout, working directories, and configured commands
- **Unified pane interaction policy** — Mouse-wheel behavior is driven by pane semantics (`agent` / `tool` / `shell`), not per-command hacks
- **Cross-platform** — macOS / Linux / WSL2

### Layout Diagrams (Column-based DSL)

workhive uses a unified **Column-based DSL** for all layouts. Each column has independent pane counts, widths, and heights.

```
AI Coding (2 cols, 2+1):         Quad (2 cols, 2+2):
┌──────────┬──────────────┐      ┌──────────┬──────────┐
│ Claude   │              │      │ Claude   │ Lazygit  │
│ (60%,50%)│  Lazygit     │      │ (50%,50%)│ (50%,70%)│
├──────────┤  (40%,100%)  │      ├──────────┼──────────┤
│ Shell    │              │      │ Shell    │ yazi     │
│ (60%,50%)│              │      │ (50%,50%)│ (50%,30%)│
└──────────┴──────────────┘      └──────────┴──────────┘

3-col (3 cols, 2+3+1):           Single column (1 col, 3):
┌────────┬────────┬────────┐     ┌─────────────────────┐
│ C1P1   │ C2P1   │        │     │ Pane 1 (100%,33%)   │
│ (33%)  │ (33%)  │        │     ├─────────────────────┤
├────────┼────────┤ C3P1   │     │ Pane 2 (100%,33%)   │
│ C1P2   │ C2P2   │ (34%)  │     ├─────────────────────┤
│ (33%)  ├────────┤        │     │ Pane 3 (100%,34%)   │
│        │ C2P3   │        │     └─────────────────────┘
└────────┴────────┴────────┘
```

---

## System Requirements

### Required Dependencies

| Dependency | Min Version | Purpose | Installation |
|------------|-------------|---------|--------------|
| Git | 2.20+ | Version control & Worktree | macOS: `brew install git`, Linux: `apt install git` |
| tmux | 3.2+ | Terminal multiplexer | macOS: `brew install tmux`, Linux: `apt install tmux` |
| yq | 4.x | YAML config parsing | macOS: `brew install yq`, Linux: [Install guide](https://github.com/mikefarah/yq#install) |

> **Note:** Bash 4.3+ is required as the shell runtime but is preinstalled on most systems. Only Git, tmux, and yq need explicit installation.

### Pane Tools (optional, launchable in a pane)

| Tool | Purpose | Installation |
|------|---------|--------------|
| [Claude Code](https://github.com/anthropics/claude-code) | AI coding assistant | `npm install -g @anthropic-ai/claude-code` |
| [Codex](https://github.com/openai/codex) | OpenAI AI coding agent | `npm install -g @openai/codex` |
| [OpenCode](https://github.com/anomalyco/opencode) | AI coding agent | `npm i -g opencode-ai@latest` |
| [Lazygit](https://github.com/jesseduffield/lazygit) | Git TUI | macOS: `brew install lazygit`, Linux: [Install guide](https://github.com/jesseduffield/lazygit#installation) |
| [yazi](https://github.com/sxyazi/yazi) | Terminal file browser | macOS: `brew install yazi`, Linux: [Install guide](https://github.com/sxyazi/yazi#installation) |

> **Pane tool fallback:** During `workhive init`, if a pane tool referenced by a template is not installed, that pane falls back to an empty shell. You can change pane commands later via `workhive config`.

**Supported platforms:** macOS / Linux / WSL2

### Runtime Behavior

workhive includes built-in tmux snapshot restore. It saves/restores windows, panes, layout, working directories, and configured commands. It does **not** restore terminal scrollback history. Configure it via `tmux.persistence.*`.

Mouse-wheel behavior is also runtime-controlled:

- `workspace` profile: agent panes scroll transcript via tmux copy-mode, shell panes do not enter copy-mode implicitly
- `compatible` profile: closer to default tmux wheel behavior

---

## Quick Start

### 1. Install

**Method A: Install via script (recommended)**

```bash
# Install latest version
curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash

# Install specific version
curl -fsSL https://raw.githubusercontent.com/yboker/workhive/main/install.sh | bash -s -- --version v0.8.0
```

**Method B: Clone the repository**

```bash
# Clone the repository
git clone https://github.com/yboker/workhive.git

# Add to PATH (recommended)
echo 'export PATH="$PATH:/path/to/workhive/bin"' >> ~/.zshrc
source ~/.zshrc

# Or create an alias (optional shortcut)
echo 'alias wh="/path/to/workhive/bin/workhive"' >> ~/.zshrc
source ~/.zshrc
```

Use the install script or clone the repository directly.

### Shell Completions

Tab completion is available for **Zsh**, **Bash**, and **Fish**.

**Recommended one-command setup** — managed by `workhive shell`:

```bash
workhive shell setup --profile minimal --enable completions --yes
```

Interactive `workhive shell setup` shows a profile menu (`minimal` / `standard` / `advanced`) first, then optional module toggles.
When `--enable` or `--disable` is provided, module-by-module customization is skipped and flags are applied directly.

### 2. Initialize Project

In your project root:

```bash
cd your-project
workhive init
```

The interactive wizard will guide you through:

- Layout mode and pane commands
- tmux theme selection (with live preview)

This creates `.workhive/config.yaml` for project-scoped workspace orchestration.

> **Write safety & consistency:** `workhive init` (project/global) uses transactional staging + atomic replace.  
> `workhive config` and `workhive tools` also use atomic config writes for higher reliability and safer interruption handling.

### 3. Start Developing

```bash
# Create and enter a Worktree
workhive open feature-x

# In another terminal, work on another branch
workhive open bugfix-123

# List all workspaces
workhive ls

# Reconnect after SSH disconnect (session must still exist)
workhive attach feature-x

# Clean up when done
workhive rm feature-x
```

**`open` behavior (important):**

- `workhive open <branch>`: deterministic strategy = reuse existing tmux session first, then try healthy snapshot restore, otherwise rebuild from `.workhive/config.yaml`.
- `workhive open -f <branch>`: force rebuild from `.workhive/config.yaml` (destroys old session and recreates layout/commands).
- `workhive open -f <branch>` **inside the target session**: still forces rebuild from `.workhive/config.yaml`; workhive detaches the current client first, then recreates the target session in place.

**`ls` behavior (important):**

- `workhive ls` interactive table uses the full collection strategy, including dirty-status collection.
- `workhive ls --watch` uses the fast strategy to avoid repeated heavy Git scans.
- `workhive ls --format json` also uses the fast strategy for machine-readable output.

**Dedicated tmux runtime:**

workhive creates a **dedicated tmux server** per project instance (named `workhive-<project>-<instance_id>`). All sessions, panes, hooks, and persistence commands target this dedicated server exclusively. This prevents interference with your default tmux server or other workhive project instances. The `workhive attach` command automatically selects the correct attach strategy:

- **Same server** (already inside the dedicated tmux server): uses `switch-client` for instant session switching.
- **Cross server** (inside a different tmux server): spawns a fresh `tmux attach` to avoid invalid cross-server `switch-client`.
- **Outside tmux**: spawns a direct `tmux attach`.

**Mouse-wheel behavior:**

- Agent panes such as Claude / Codex / Hermes scroll by pane metadata (`@workhive_pane_kind=agent`), not by command-name special cases.
- Shell panes keep the safer workspace behavior unless you explicitly switch `tmux.interaction_profile` to `compatible`.

---


## Command Reference

See [help/reference/commands.md](help/reference/commands.md) for the full command reference.
See [help/README.md](help/README.md) for documentation structure and navigation.

### `workhive tools` — Global Tool Registry

Manage the global tool registry in `~/.config/workhive/config.yaml`.

```bash
workhive init --global
workhive tools list
workhive tools add --name claude --cmd "claude --dangerously-skip-permissions" --ai true --default true
workhive tools edit claude --cmd "claude --resume"
workhive tools remove claude
workhive tools set-default codex
workhive tools validate
workhive tools manage
```

`tools` is global-only. Project-level `tools` config is ignored.

---

## Best Practices

### Keyboard Shortcuts

#### tmux Built-in Shortcuts

All workhive workspaces run inside tmux. The tmux **prefix key** is `Ctrl+B` (press Ctrl+B first, then the next key).

| Shortcut | Action |
|----------|--------|
| `Ctrl+B d` | **Detach** session (run in background, safe for SSH disconnect) |
| `Ctrl+B o` | Switch to the **next pane** (cycles through all panes) |
| `Ctrl+B ;` | Switch to the **previous pane** |
| `Ctrl+B ↑/↓/←/→` | Move to the pane in that **direction** |
| `Ctrl+B z` | **Maximize/restore** the current pane (toggle zoom) |
| `Ctrl+B [` | Enter **scroll/copy mode** (press `q` to exit) |
| `Ctrl+B x` | Close the current pane (with confirmation) |

#### workhive Enhanced Shortcuts

workhive injects these shortcuts when creating sessions (requires tmux 3.3+):

| Shortcut | Action | Dependency |
|----------|--------|------------|
| `Ctrl+B g` | Popup Lazygit (80% screen) | Lazygit |
| `Ctrl+B y` | Popup yazi file browser (80% screen) | yazi |
| `Ctrl+B n` | Popup notes editor for current workspace | — |
| `Ctrl+B w` | Attach to another workspace session | — |
| `Ctrl+B L` | Show layout config hint | — |

Popups float over the current pane. Press `Esc` or `q` to close and restore the layout. Useful for quick Git status, file browsing, or notes without leaving Claude Code.

> If tmux is below 3.3 or tools are missing, shortcuts are not injected; normal usage is unaffected.

### Pane Navigation Workflows

**Scenario: Switch between panes in a three-pane layout**

```
┌─────────────┬──────────────┐
│ Claude Code  │              │
│ (Pane 0)     │  Lazygit     │
│              │  (Pane 2)    │
├─────────────┤              │
│ Shell        │              │
│ (Pane 1)     │              │
└─────────────┴──────────────┘
```

- From Claude Code → Lazygit: `Ctrl+B →` (right arrow)
- From Lazygit → Claude Code: `Ctrl+B ←` (left arrow)
- From Claude Code → Shell: `Ctrl+B ↓` (down arrow)
- From Shell → Claude Code: `Ctrl+B ↑` (up arrow)
- Cycle through all panes: `Ctrl+B o` repeatedly

**Scenario: Temporarily maximize a pane**

When you need full screen for Claude Code or Lazygit:

1. Press `Ctrl+B z` to **zoom** the current pane to full screen
2. Do your work
3. Press `Ctrl+B z` again to **restore** the layout

### Common Operations

**Scenario: Close a pane inside the workspace**

```bash
# Method 1: Use workhive command (inside tmux)
workhive pane close                  # With confirmation
workhive pane close -f               # Skip confirmation (AI automation)

# Method 2: Use tmux shortcut
# Ctrl+B x → confirm with 'y'

# Method 3: Exit the process in the pane
# e.g., type 'exit' in a shell, or 'q' in Lazygit
```

> Note: You cannot close the last pane in a session. Use `workhive rm <worktree>` to close the entire workspace.

**Scenario: Open a new tool in the current pane**

```bash
# Interactive: choose Worktree then choose command
workhive pane open

# Direct: specify worktree
workhive pane open feature-x

# Non-interactive: specify worktree + command (for AI automation)
workhive pane open feature-x --cmd claude

# Auto-create worktree + open tool (fully non-interactive)
workhive pane open feature-x --cmd claude -f
```

**Scenario: Add a new pane below the current one**

```bash
# Empty shell
workhive pane split

# With a specific command
workhive pane split --cmd "npm run dev"
workhive pane split --cmd lazygit

# Split + open AI tool for a task branch (common in multi-task workflows)
workhive pane split --cmd "workhive pane open task-auth --cmd claude -f"
```

**Scenario: Send text to another pane (AI automation)**

```bash
# Send a command to pane 1
workhive pane send 1 "npm test"

# Send text without pressing Enter
workhive pane send 2 "hello" --commit none

# Auto-adapt injection/commit behavior (default)
workhive pane send 3 "run the task" --mode auto --commit auto

# Send to selected panes
workhive pane send 1,2,3 "npm test"

# Broadcast same text to all panes
workhive pane send --all "npm test"

# List panes to find indices
workhive pane list
# Includes AI agent runtime states: running / waiting / idle / unknown

# Show the current pane details
workhive pane current
```

**Scenario: Change layout config**

```bash
# Update layout config only (no runtime pane/session mutation)
workhive config --layout
workhive config --global --layout
```

### Workflow Tips

1. **Don't manually split tmux panes** — Let workhive manage the layout. Use `workhive pane split` if you need more panes, and use `workhive config --layout` to adjust layout config.

2. **Use `--no-attach` for batch creation** — When setting up multiple workspaces:
   ```bash
   workhive open feature-a --no-attach
   workhive open feature-b --no-attach
   workhive attach feature-a  # Enter the one you want
   ```

3. **SSH recovery in 3 seconds** — After reconnecting via SSH:
   ```bash
   workhive ls              # See what's running
   workhive attach feature-x  # Pick up where you left off
   ```

4. **Version-control your config** — Add `.workhive/config.yaml` to your repo to share environment setup across your team.

5. **Use popup shortcuts for quick tasks** — `Ctrl+B g` for Git, `Ctrl+B y` for file browsing, `Ctrl+B w` to switch workspace — no need to leave your current pane.

---


## Configuration

See [help/reference/configuration.md](help/reference/configuration.md) (English) and [help/reference/configuration.zh-CN.md](help/reference/configuration.zh-CN.md) (Chinese) for the full configuration reference.

### Config Locations

1. **Project config** (highest priority): `.workhive/config.yaml`
2. **User config**: `~/.config/workhive/config.yaml` (or `$XDG_CONFIG_HOME/workhive/config.yaml`)
3. **Built-in defaults** (lowest priority)

### Common Keys (Generated by `workhive init`)

`workhive init` writes this structure into config files. `workhive config` validates schema strictly and does not backfill legacy keys.

```yaml
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
  sync_files: []

tmux:
  # session_prefix: null
  mouse_support: true
  interaction_profile: "workspace"
  env:
    CLAUDE_CODE_NO_FLICKER: 1
  attach_delay: 0s
  theme: "catppuccin-mocha"
  persistence:
    enabled: false
    generation: 0
    auto_save_mode: hybrid
    auto_save_debounce: 3s
    auto_save_interval: 10m
    restore_processes: true
    process_list: [":all:"]

    snapshot_history_limit: 8
    snapshot_max_size_mb: 8

tools:
  default: claude
  list:
    - tool_name: claude
      label: Claude
      command: "claude --dangerously-skip-permissions"
      is_ai: true
      enabled: true
    - tool_name: codex
      label: Codex
      command: "codex"
      is_ai: true
      enabled: true
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `tmux.mouse_support` | bool | `true` | Controls tmux mouse mode (`on`/`off`) |
| `tmux.interaction_profile` | string | `"workspace"` | Mouse-wheel interaction policy: `workspace` keeps shell panes out of implicit copy-mode; `compatible` keeps tmux-style scroll behavior |
| `tmux.env` | map | `{ CLAUDE_CODE_NO_FLICKER: 1 }` | Environment variables injected into pane startup commands (project overrides global) |
| `tmux.attach_delay` | duration | `0s` | Optional delay before auto-attach; when unset, workhive defaults to immediate attach |
| `worktree.sync_files` | array | `[]` | File/dir sync rules for new worktrees |
| `tmux.persistence.enabled` | bool | `false` | Enables snapshot save/restore for tmux sessions |
| `tmux.persistence.generation` | int | `0` | Snapshot generation fence; incremented automatically when re-enabling snapshot restore |
| `tmux.persistence.auto_save_mode` | string | `"hybrid"` | Auto-save strategy: `hybrid`, `event`, or `interval` |
| `tmux.persistence.auto_save_debounce` | duration | `3s` | Debounce window for event-triggered saves |
| `tmux.persistence.auto_save_interval` | duration | `10m` | Timer interval used by `interval`/`hybrid` modes |
| `tmux.persistence.auto_save_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1 }` | Env vars injected into internal auto-save runs. Only explicit config values are used |
| `tmux.persistence.pre_kill_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1, WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1 }` | Env vars injected into best-effort pre-kill snapshot saves. Only explicit config values are used |
| `tmux.persistence.snapshot_history_limit` | int | `8` | Max archived snapshots to keep (excluding `wh_snapshot_latest.jsonl` and its target) |
| `tmux.persistence.snapshot_max_size_mb` | int | `8` | Total snapshot archive size cap (`0` = disabled) |
| `maintenance.temp_cleanup.max_age_days` | int | `7` | Auto-clean stale temp config files older than N days |
| `maintenance.log.path` | string | `".workhive/logs/workhive.log"` | Runtime file log path (absolute, or relative to git root) |
| `maintenance.test_log.path` | string | `"/dev/null"` | Test-mode file log path (`/dev/null` means no persisted test log file by default) |
| `maintenance.test_log.level` | string | inherit `maintenance.log.level` | Optional test-log threshold override: `debug`, `info`, `warn`, `error` |
| `maintenance.test_log.format` | string | inherit `maintenance.log.format` | Optional test-log format override: `text` or one-line `json` |
| `maintenance.test_log.max_size_mb` | int | inherit `maintenance.log.max_size_mb` | Optional per-file test-log size threshold in MB for rotation |
| `maintenance.test_log.max_backups` | int | inherit `maintenance.log.max_backups` | Optional number of rotated test-log backups (`0` = truncate mode) |
| `maintenance.log.level` | string | `"info"` | File log threshold: `debug`, `info`, `warn`, `error` |
| `maintenance.log.format` | string | `"text"` | File log format: `text` or one-line `json` |
| `maintenance.log.max_size_mb` | int | `10` | Per-file log size threshold in MB for rotation |
| `maintenance.log.max_backups` | int | `5` | Number of rotated backups to keep (`0` = truncate mode) |
| `tools.default` | string | `"__empty__"` | Global default tool reference (`tool_name` or special refs `__empty__`, `__custom__`) |
| `tools.list` | array | built-in list | Global pane tool registry records |
| `tools.list[].tool_name` | string | — | Tool name (globally unique, used by `set-default`) |
| `tools.list[].label` | string | `tool_name` | Display label in `workhive tools list` |
| `tools.list[].command` | string | — | Actual command to run |
| `tools.list[].is_ai` | bool | `false` | Marks the tool as AI-capable for AI detection/listing |
| `tools.list[].enabled` | bool | `true` | Whether the tool is selectable/launchable |

### Value Guide (What You Can Put)

| Field | Allowed values/format | Meaning |
|------|------------------------|---------|
| `tmux.persistence.enabled` | `true` / `false` | Enables/disables snapshot save + restore |
| `tmux.persistence.generation` | integer `>= 0` | Snapshot generation fence; auto-increments when `enabled` switches `false -> true` |
| `tmux.persistence.auto_save_mode` | `hybrid` / `event` / `interval` | `hybrid` = event + timer, `event` = event only, `interval` = timer only |
| `tmux.persistence.auto_save_debounce` | duration: `3s`, `30s`, `1m`, `2h` | Debounce window for event-triggered saves |
| `tmux.persistence.auto_save_interval` | duration: `1m`, `10m`, `1h` | Timer interval for `interval`/`hybrid` |
| `tmux.persistence.restore_processes` | `true` / `false` | Whether to restart pane processes on restore |
| `tmux.persistence.process_list` | `[":all:"]` / `["vi", "npm", "hermes"]` | List of processes to capture and restore. Default is all. |

| `tmux.persistence.snapshot_history_limit` | integer `>= 1` | Max archived snapshots kept (excluding `wh_snapshot_latest.jsonl` and its target) |
| `tmux.persistence.snapshot_max_size_mb` | integer `>= 0` | Max total archive size in MB (`0` = unlimited by size) |
| `maintenance.log.path` | absolute path or git-root-relative path | Runtime log output file path |
| `maintenance.test_log.path` | absolute path or git-root-relative path | Test-mode log output file path (used when explicitly configured) |
| `maintenance.test_log.level` | `debug` / `info` / `warn` / `error` | Optional test-log level override; inherits `maintenance.log.level` when omitted |
| `maintenance.test_log.format` | `text` / `json` | Optional test-log format override; inherits `maintenance.log.format` when omitted |
| `maintenance.test_log.max_size_mb` | integer `>= 1` | Optional test-log rotation threshold; inherits `maintenance.log.max_size_mb` when omitted |
| `maintenance.test_log.max_backups` | integer `>= 0` | Optional test-log backup count; inherits `maintenance.log.max_backups` when omitted |
| `maintenance.log.level` | `debug` / `info` / `warn` / `error` | Minimum log level written to file |
| `maintenance.log.format` | `text` / `json` | File log encoding format |
| `maintenance.log.max_size_mb` | integer `>= 1` | Per-file size threshold for rotation |
| `maintenance.log.max_backups` | integer `>= 0` | Number of rotated backups kept (`0` = truncate mode) |
| `tmux.attach_delay` | duration: `0s`, `1s`, `2s`, `5s` | Optional delay before auto-attach |
| `tmux.env` | map of `KEY: scalar` | Startup env vars for pane commands. Keys must match `[A-Za-z_][A-Za-z0-9_]*` |
| `tmux.persistence.auto_save_env` | map of `KEY: scalar` | Env vars for internal auto-save commands. Keys must match `[A-Za-z_][A-Za-z0-9_]*` |
| `tmux.persistence.pre_kill_env` | map of `KEY: scalar` | Env vars for pre-kill snapshot saves. Keys must match `[A-Za-z_][A-Za-z0-9_]*` |
| `tools.default` | `tool_name` / `__empty__` / `__custom__` | Default tool selection (`__empty__` = empty shell, `__custom__` = prompt custom command) |

### Log Path Defaults (Runtime vs Test)

workhive now separates runtime logs and test logs by default, and both are configurable:

```yaml
maintenance:
  log:
    path: ".workhive/logs/workhive.log"
    level: info
    format: text
    max_size_mb: 10
    max_backups: 5
  test_log:
    path: "/dev/null"   # set ".workhive/logs/workhive-test.log" to persist dedicated test log file
```

Notes:
- Both paths support absolute paths or git-root-relative paths.
- `workhive init` writes both keys into new config files by default.
- If `maintenance.test_log.level/format/max_size_mb/max_backups` are omitted, they inherit from `maintenance.log.*`.
- In pane runtime, snapshot quality/lock contention messages are quiet by default and still recorded to file logs.

> `tools` is global-only. Manage it with `workhive tools ...`.
> `git_init.*` and `tools.*` are global-only and are not persisted in project config.
> Before first use, run `workhive init --global`.
> Recommended: `workhive tools list|add|edit|remove|set-default|validate|manage`.

### Snapshot Restore Tuning (Quick Commands)

```bash
# Enable snapshot restore
workhive config set tmux.persistence.enabled true

# Recommended small defaults for most projects
workhive config set tmux.persistence.snapshot_history_limit 8
workhive config set tmux.persistence.snapshot_max_size_mb 8

# Auto-save strategy
workhive config set tmux.persistence.auto_save_mode hybrid
workhive config set tmux.persistence.auto_save_debounce 3s
workhive config set tmux.persistence.auto_save_interval 10m
```

### Git Init Safety Guard

When you run workhive in a non-Git directory, it can offer to run `git init` for you. To prevent accidental initialization in dangerous locations, workhive applies a multi-layer safety guard.

**Evaluation policy (in priority order):**
1. **Built-in hard guard** — Root (`/`) is always blocked
2. **`git_init.whitelist_paths`** — If non-empty, whitelisted directories (and subdirectories) are allowed immediately
3. **`git_init.dangerous_paths_extra`** — User-configurable exact-match blocklist (auto-populated with home/system defaults during `workhive init --global`)
4. **User confirmation** — Directories passing all checks still require explicit `y` before `git init` runs

**Manage the whitelist via CLI:**

```bash
workhive config git-whitelist add ~/projects     # Allow git init under ~/projects
workhive config git-whitelist remove ~/projects  # Remove from whitelist
workhive config git-whitelist list               # Show current whitelist
```

**Configuration reference (global config `~/.config/workhive/config.yaml`):**

```yaml
git_init:
  whitelist_paths:
    - "/Users/me/projects"
    - "/Users/me/work"
  dangerous_paths_extra:
    - "/tmp"
    - "/var"
    # ... auto-generated system paths
```

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `git_init.whitelist_paths` | string[] | `[]` | Directories (and subdirs) allowed for auto git-init (global-only) |
| `git_init.dangerous_paths_extra` | string[] | *OS-specific* | Additional blocked directories (exact-match only); auto-populated on `workhive init --global` (global-only) |

> **Note:** Root (`/`) is protected by a built-in hard guard. On macOS, `dangerous_paths_extra` defaults include home (`$HOME`), `/System`, `/Applications`, `/Library`, and user folders (Desktop, Documents, etc.). On Linux, defaults include home plus `/boot`, `/sbin`, `/proc`, `/sys`, `/dev`, and similar system paths. You can edit `dangerous_paths_extra` manually in the global config file.
> If a path appears in both whitelist and dangerous_paths_extra, whitelist wins. The only non-overridable rule is root (`/`) hard guard.

---

## Typical Workflows

### Parallel Development on Multiple Branches

```bash
workhive open feature-x          # Terminal 1: work on feature-x
workhive open feature-y          # Terminal 2: work on feature-y (fully isolated)
workhive ls                      # List all workspaces
workhive attach feature-y        # Switch to another running workspace session
workhive rm feature-x            # Clean up when done
```

### SSH Session Recovery

```bash
# Create environment locally
workhive open feature-x
# Ctrl+B d to detach (or disconnect SSH)

# Later, reconnect via SSH
ssh server && cd project
workhive attach feature-x        # Reconnect to existing session
# If session no longer exists, recreate with:
# workhive open feature-x
```

### Remote Firefighting (Mobile SSH)

```bash
# SSH from phone
ssh server && cd project
workhive ls                       # See all workspaces
workhive attach my-service        # Reconnect to previous running session
# Claude Code still running, continue from there
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `WORKHIVE_DEBUG` | Enable debug output | unset |
| `WORKHIVE_PLATFORM` | Override platform detection | auto |
| `WORKHIVE_TERMINAL_ADAPTER` | Force adapter (`tmux` / `screen`) | `auto` |
| `WORKHIVE_TMUX_SERVER_NAME` | Dedicated tmux server name (set automatically by runtime) | `workhive-<project>-<hash>` |

```bash
WORKHIVE_DEBUG=1 workhive open feature-x   # Enable debug
workhive --debug open feature-x            # Same
```

---

## Architecture

workhive uses a layered architecture with clear separation of concerns:

```
bin/workhive                    # CLI entry & command dispatch
├── lib/app/commands/           # Application: 15 public commands
│                                 (init, open, attach, close, pane, list, remove,
│                                  config, doctor, gc, destroy, rename, upgrade,
│                                  log, shell)
├── lib/app/shared/             # Shared helpers: worktree collector/selector/safety,
│                                 command builder, theme helper, layout visual/rebuild,
│                                 help index
├── lib/domain/services/        # Domain: worktree, session, layout, workspace
├── lib/infra/                  # Infrastructure
│   ├── adapters/terminal/      #   Terminal adapters (tmux / screen)
│   ├── ai/                     #   AI tool adapter (detection + command building)
│   ├── cache/                  #   In-memory cache (TTL, batch invalidation)
│   ├── config/                 #   Config loader (3-tier priority)
│   ├── persistence/            #   Session snapshot restore (snapshot, save, restore, auto-save, process capture)
│   ├── platform/               #   Platform detection & path handling
│   └── tools/                  #   Tool registry (pane tool management)
├── lib/utils/                  # Utilities: logging, TUI, i18n, common helpers
└── configs/                    # Recommended terminal configs (Ghostty, etc.)
```

**Patterns:**

- **Adapter** — tmux/screen adapter interface
- **Factory** — Auto-select adapter by platform
- **Strategy** — Environment-aware attach/restore behavior (same-server / cross-server / outside-tmux)
- **Dedicated Runtime** — Each project instance gets its own tmux server via `tmux_runtime.sh`
- **Source Guard** — Prevent duplicate loading
- **3-Tier Config** — Project → User → Default priority

---

## Testing

workhive includes unit and integration tests.

### Run Tests

```bash
# All unit tests (30 suites)
bash test/unit/run_all.sh

# Integration tests (requires tmux)
bash test/integration/full_workflow_test.sh
bash test/integration/domain_services_test.sh
bash test/integration/non_worktree_test.sh
bash test/integration/e2e_user_workflow_test.sh

# BDD tests
bash test/integration/list_bdd_test.sh
bash test/integration/new_features_bdd_test.sh
bash test/integration/comprehensive_bdd_test.sh
bash test/integration/reliability_bdd_test.sh
bash test/integration/pane_layout_bdd_test.sh
bash test/integration/config_command_test.sh
bash test/integration/robustness_bdd_test.sh
bash test/integration/new_commands_bdd_test.sh
bash test/integration/dir_relocation_bdd_test.sh
bash test/integration/branch_sanitize_bdd_test.sh
bash test/integration/slash_branch_bdd_test.sh
```

### Coverage

| Suite | Coverage |
|-------|----------|
| `unit/` | Utils, cache, config, platform, i18n, JSON escape, source guards, version, adapter, branch sanitize, layout, pane, session, worktree, tools registry |
| `unit/test_commands.sh` | CLI argument parsing, help, version |
| `unit/test_list.sh` | List command dashboard: data collection, formatting, Git status |
| `unit/test_new_commands.sh` | doctor, gc commands, session notify, and keybindings |
| `unit/test_persistence.sh` | Snapshot restore module: snapshot format, save, process capture, manager, lifecycle |
| `unit/test_persistence_restore.sh` | Snapshot restore flow: diagnostics, CWD restore semantics, client reconciliation |
| `unit/test_persistence_security.sh` | Snapshot restore security: mktemp, command validation with injection rejection (including tui/ai), log sanitization, CWD relocation |
| `unit/test_worktree_safety.sh` | Worktree safety scan, dirty detection, marks display |
| `unit/test_tools_registry.sh` | Tool registry: list, filter, detect AI, command lookup |
| `integration/full_workflow_test.sh` | init → open → list → attach → remove end-to-end |
| `integration/e2e_user_workflow_test.sh` | End-to-end user workflow scenarios |
| `integration/domain_services_test.sh` | Worktree + Session + Layout integration |
| `integration/non_worktree_test.sh` | Non-Worktree single directory mode |
| `integration/list_bdd_test.sh` | List command dashboard BDD scenarios |
| `integration/new_features_bdd_test.sh` | doctor, gc, command-surface and layout BDD scenarios |
| `integration/comprehensive_bdd_test.sh` | Cross-feature integration scenarios |
| `integration/reliability_bdd_test.sh` | Error handling, edge cases, recovery |
| `integration/pane_layout_bdd_test.sh` | Pane and layout management |
| `integration/config_command_test.sh` | Config command integration |
| `integration/robustness_bdd_test.sh` | Config fallback, sentinel protection, init error handling |
| `integration/new_commands_bdd_test.sh` | rename and log commands |
| `integration/dir_relocation_bdd_test.sh` | Directory relocation recovery, stale snapshot fallback |
| `integration/branch_sanitize_bdd_test.sh` | Branch name sanitization in session names |
| `integration/slash_branch_bdd_test.sh` | Branches with slash separators |

---

## Contributing

Contributions are welcome. Please:

1. **Fork** the repo and create a feature branch
2. Ensure code passes `shellcheck`
3. Add tests for new features
4. Open a Pull Request with a clear description

### Development Setup

```bash
git clone https://github.com/yboker/workhive.git
cd workhive

# Verify environment
bash test/unit/run_all.sh
bash test/integration/full_workflow_test.sh
```

### Code Style

- Follow [Google Shell Style Guide](https://google.github.io/styleguide/shellguide.html)
- Use `set -euo pipefail` for error handling
- Document public functions
- Use `local` for variables to avoid scope pollution

---

## License

[MIT](LICENSE)

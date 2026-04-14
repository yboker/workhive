# workhive — Quick Start Guide

> **One command to orchestrate your panes, commands, and sessions.**

[English](#overview) | [Chinese](README.zh-CN.md)

---

## Overview

workhive is a terminal workspace orchestrator built on **Git worktrees + tmux**. It automates workspace creation, session management, and pane layout orchestration.

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    CLI Layer (bin/workhive)                  │
├──────────────┬──────────────┬───────────────┬───────────────┤
│  Application │   Domain     │ Infrastructure│  Middleware    │
│  Commands    │   Services   │  Adapters     │    Plugins    │
│  (open,      │  (session,   │  (tmux/screen,│  (plugin load,│
│   attach,    │   worktree,  │   config,     │   security)   │
│   list, ...) │   layout,    │   cache)      │               │
│              │   workspace) │               │               │
├──────────────┴──────────────┴───────────────┴───────────────┤
│                    Utilities (i18n, tui, progress)           │
└─────────────────────────────────────────────────────────────┘
```

**Design Patterns Used:**
- **Hexagonal Architecture** — Domain logic decoupled from infrastructure (terminal adapters)
- **Strategy Pattern** — Terminal multiplexer adapters (`tmux.sh`, `screen.sh`) behind `base.sh` interface
- **Factory Pattern** — `factory.sh` dynamically loads the correct adapter
- **Template Method** — Layout modes (`half-split`, `left-split`, etc.) share creation pipeline
- **Three-tier Config Merging** — Default → Global → Project config cascade

---

## Prerequisites

| Dependency | Min Version | Install |
|------------|-------------|---------|
| **Git** | 2.20+ | `brew install git` / `apt install git` |
| **tmux** | 3.2+ | `brew install tmux` / `apt install tmux` |
| **yq** | 4.x | `brew install yq` |
| Bash | 4.3+ | Usually preinstalled |

---

## From Zero to AI Workspace (5 Minutes)

### Step 1: Install workhive

```bash
git clone https://github.com/yboker/workhive.git ~/workhive
cd ~/workhive && chmod +x bin/workhive

# Add to PATH (add to ~/.zshrc or ~/.bashrc)
export PATH="$HOME/workhive/bin:$PATH"

# Enable shell completions (optional)
source ~/workhive/completions/workhive.zsh   # for zsh
# source ~/workhive/completions/workhive.bash # for bash
```

### Step 2: Initialize your project

```bash
cd /path/to/your/project

workhive init
```

The interactive wizard will:
1. Detect your language (`en` / `zh`)
2. Let you choose a layout mode (template) with live preview
3. Configure pane commands for each pane (auto-detects installed tools)
4. Let you pick a tmux theme from 8 built-in themes
5. Generate `.workhive/config.yaml`

### Step 3: Open your first workspace

```bash
# Open on the main branch (single-directory mode)
workhive open

# Or create a new branch workspace (worktree mode)
workhive open feature-login
```

That's it! You now have a tmux session with your configured layout and tools running.

### Step 4: Daily workflow

```bash
# Start a new feature
workhive open feature-payment

# List all active workspaces
workhive ls

# Check workspace dashboard
workhive list

# Switch workspace (inside tmux)
workhive attach feature-login

# Clean up merged branches
workhive gc --auto
```

---

## Command Reference

### Core Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `workhive init` | | Initialize workhive in a Git project (interactive wizard) |
| `workhive open [branch]` | | Create/enter a workspace (worktree + tmux + layout) |
| `workhive attach [branch]` | | Attach to an existing workspace session |
| `workhive list` | `ls` | List all active workspaces (with safety marks) |
| `workhive list` | `ls` | Dashboard view of all workspaces |
| `workhive remove <branch>` | `rm` | Remove a workspace (worktree + session) with safety warnings |

### Session Management

| Command | Description |
|---------|-------------|
| `workhive rename <old> <new>` | Rename branch + worktree + session + state files |
| `workhive destroy [--global]` | Remove ALL workspaces in current project (with safety scan) |

### Layout & Panes

| Command | Description |
|---------|-------------|
| `workhive config --layout [--global]` | Interactive layout editor (config only) |
| `workhive pane open [worktree] [--cmd <cmd>]` | Open a tool in the current pane |
| `workhive pane split [--cmd <cmd>] [--size <pct>]` | Split current pane below |
| `workhive pane close [--id <pane_id>] [-f]` | Close a pane |
| `workhive pane send <pane> <text> [--mode <mode>] [--commit <mode>]` | Send text to a pane |
| `workhive pane list [--format table|json]` | List pane runtime state |

### Configuration

| Command | Description |
|---------|-------------|
| `workhive config` | Show current configuration |
| `workhive config edit` | Open config in editor (interactive) |
| `workhive config get <key>` | Get a config value |
| `workhive config set <key> <value>` | Set a config value |

### Maintenance

| Command | Description |
|---------|-------------|
| `workhive gc [--auto]` | Clean up merged branches, orphaned worktrees, stale sessions |
| `workhive doctor` | Health check: verify dependencies and config |
| `workhive log [--tail N]` | View workhive operation log |
| `workhive upgrade [--check]` | Self-upgrade to latest version (supports git & install.sh) |
| `workhive shell setup [--dry-run]` | Configure shell integration and completions |

---

## config.yaml Reference

After `workhive init`, your project has `.workhive/config.yaml`:

```yaml
version: "0.9.0"

# Layout configuration
layout:
  mode: "half-split"    # half-split | left-split | right-split | quad | custom

  # half-split example (2 panes)
  left_pane:
    width: 50
    command: "claude --dangerously-skip-permissions"

  right_pane:
    width: 50
    command: "lazygit"

# Language: en | zh
language: "en"

# Worktree management
worktree:
  base_path: ".workhive/worktrees"
  auto_create: true
  auto_delete: true
  sync_files: []          # Files to copy into new worktrees (e.g., [".env", ".env.local"])

# tmux session settings
tmux:
  session_prefix: null    # Custom session prefix (default: auto-generated)
  mouse_support: true
  env:
    CLAUDE_CODE_NO_FLICKER: 1
  theme: "catppuccin-mocha"
  persistence:
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1
```

### Layout Modes

```
half-split (2 panes):            left-split (3 panes):
┌──────────┬──────────┐          ┌──────────┬──────────────┐
│          │          │          │ Pane 1   │              │
│  Pane 1  │  Pane 2  │          │ (60%,50%)│   Pane 3     │
│  (50%)   │  (50%)   │          ├──────────┤   (40%)      │
│          │          │          │ Pane 2   │              │
└──────────┴──────────┘          │ (60%,50%)│              │
                                 └──────────┴──────────────┘

right-split (3 panes):           quad (4 panes):
┌──────────┬──────────────┐      ┌──────────┬──────────┐
│          │  Pane 2      │      │ Pane 1   │ Pane 3   │
│ Pane 1   │  (60%,50%)   │      │ (50%)    │ (50%)    │
│ (40%)    ├──────────────┤      ├──────────┼──────────┤
│          │  Pane 3      │      │ Pane 2   │ Pane 4   │
│          │  (60%,50%)   │      │ (50%)    │ (50%)    │
└──────────┴──────────────┘      └──────────┴──────────┘
```

### Custom Layout

```yaml
layout:
  mode: "custom"
  custom:
    rows: 3
    cols: 2
    row_0_col_0:
      command: "claude"
    row_1_col_0:
      command: "lazygit"
    row_2_col_0:
      command: ""
    row_0_col_1:
      command: "opencode"
    row_1_col_1:
      command: ""
    row_2_col_1:
      command: ""
```

### tmux Themes

8 built-in themes available via `workhive init` or `workhive config`:

| Theme | Description |
|-------|-------------|
| `catppuccin-mocha` | Warm pastel tones (default) |
| `tokyo-night` | Cool blue/purple dark theme |
| `dracula` | Classic dark with vibrant accents |
| `nord` | Arctic, cool blue tones |
| `gruvbox` | Retro warm earth tones |
| `rose-pine` | Soft, muted elegance |
| `everforest` | Green-inspired nature palette |
| `onedark` | Atom One Dark inspired |

---

## Safety Features

workhive includes several built-in safety mechanisms to protect your work:

- **Worktree Safety Scanner** — Before `destroy`, `remove`, or `gc`, workhive scans for uncommitted changes and unmerged commits, displaying a prominent warning box with file-level details
- **Safe Delete (`safe_rm`)** — All directory deletions go through a safety wrapper that rejects dangerous paths (`/`, `$HOME`, `/usr`, etc.) and resolves symlinks to prevent traversal attacks
- **Plugin File Permission Checks** — Plugins must not be world-writable; symlinks must resolve within the plugins directory
- **Config Key Injection Prevention** — Config key names are validated against `^[a-zA-Z0-9_.]+$` to prevent yq command injection
- **Branch Name Sanitization** — Special characters in branch names are normalized to prevent path traversal and tmux session name issues

---

## Real-World Scenarios

### Scenario 1: Solo AI Coding (Claude Code + Lazygit)

```yaml
layout:
  mode: "half-split"
  left_pane:
    width: 60
    command: "claude --dangerously-skip-permissions"
  right_pane:
    width: 40
    command: "lazygit"
```

```bash
workhive init          # choose half-split template
workhive open main     # enter workspace
# Left pane: Claude Code writing code
# Right pane: Lazygit for git operations
```

### Scenario 2: Multi-AI Agents (Claude + OpenCode)

```yaml
layout:
  mode: "half-split"
  left_pane:
    width: 50
    command: "claude --dangerously-skip-permissions"
  right_pane:
    width: 50
    command: "opencode"
```

### Scenario 3: Full Dev Environment (AI + Git + Shell)

```yaml
layout:
  mode: "left-split"
  top_left_pane:
    command: "claude --dangerously-skip-permissions"
  bottom_left_pane:
    command: "lazygit"
  right_pane:
    width: 40
    command: ""    # empty shell for tests, builds, etc.
```

### Scenario 4: Parallel Feature Development

```bash
# Start two features simultaneously
workhive open feature-auth
workhive open feature-payment

# Check all workspace dashboard
workhive list

# Switch between them (inside tmux)
workhive attach feature-payment

# After merge, clean up
workhive gc --auto
```

### Scenario 5: SSH Remote + Disconnect Recovery

```bash
# SSH into server, start working
ssh dev-server
cd /project && workhive open feature-x

# ... network drops ...

# Reconnect later
ssh dev-server
cd /project && workhive attach feature-x
# Everything is exactly as you left it!

# After server reboot
workhive open feature-x    # recreates session from config
```

### Scenario 6: Single-Directory Mode (No Worktree)

```bash
# Open workspace on current directory (no branch argument)
workhive open
# Creates tmux session + layout without creating a worktree
# Useful for main branch or when worktrees are not needed
```

---

## Session Naming Convention

workhive tmux sessions follow this naming pattern:

```
wh@<instance_id>@<project>@<branch>
```

Example: `wh@e4fc@myproject@feature-login`

- `wh` — workhive prefix
- `instance_id` — first 4 chars of project instance hash (allows multiple workhive instances per project)
- `project` — Git project directory name
- `branch` — sanitized branch name (special chars replaced with `-`)

---

## Plugin System

Place executable scripts in `.workhive/plugins/`:

```bash
mkdir -p .workhive/plugins
cat > .workhive/plugins/my-plugin.sh << 'EOF'
#!/usr/bin/env bash
PLUGIN_NAME="my-plugin"
PLUGIN_VERSION="1.0.0"

plugin_init_my_plugin() {
    echo "Plugin loaded: my-plugin"
}
EOF
chmod +x .workhive/plugins/my-plugin.sh
```

Plugin security: files must not be world-writable, and symlinks must resolve within the plugins directory.

---

## Upgrade

```bash
# Check for updates
workhive upgrade --check

# Upgrade to latest
workhive upgrade

# Install specific version
workhive upgrade --version v0.9.0

# Force reinstall
workhive upgrade --force
```

workhive detects whether it was installed via `git clone` or `install.sh` and uses the appropriate upgrade method.

---

## Terminal Configuration: Ghostty

workhive ships an example [Ghostty](https://ghostty.org) configuration in `configs/ghostty/config`.

### Install Config

```bash
mkdir -p ~/.config/ghostty
cp configs/ghostty/config ~/.config/ghostty/config
```

### Keyboard Shortcuts (SAND Mnemonic)

**S** = Split, **A** = Across, **N** = Navigate, **D** = Destroy

| Category | Shortcut | Action |
|----------|----------|--------|
| **Split** | `Cmd+D` | Split right (vertical) |
| | `Cmd+Shift+D` | Split down (horizontal) |
| **Across** | `Cmd+T` | New tab |
| | `Cmd+Shift+←/→` | Previous/Next tab |
| **Navigate** | `Cmd+Alt+←/→/↑/↓` | Navigate between splits |
| | `Cmd+Shift+F` | Zoom/unzoom split |
| **Destroy** | `Cmd+W` | Close panel/tab |
| **Utility** | `` Ctrl+` `` | Toggle quick terminal (Quake-style) |
| | `Cmd+Shift+,` | Reload config |

### Recommended Workflow

```
┌─────────────────────┬──────────────┐
│                     │              │
│   Claude Code       │   Lazygit    │
│   (AI Agent)        │   (Git UI)   │
│                     ├──────────────┤
│                     │   yazi       │
│                     │ (File Browse)│
│                     │              │
└─────────────────────┴──────────────┘
```

### Notes

- [Ghostty](https://ghostty.org) (macOS / Linux)
- A Nerd Font if you want terminal icons to render correctly

---

## Adding More Terminal Configs

To add configurations for other terminals (Alacritty, WezTerm, kitty, etc.), create a directory under `configs/` with the terminal name and include the config file:

```
configs/
├── README.md              # This file
├── ghostty/
│   └── config
├── alacritty/             # Example: add Alacritty config
│   └── alacritty.toml
└── wezterm/               # Example: add WezTerm config
    └── wezterm.lua
```

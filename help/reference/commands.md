## Command Reference

### Command Overview

| Command | Alias | Description |
|---------|-------|-------------|
| `workhive init` | — | Interactive configuration setup |
| `workhive config` | — | View or edit configuration |
| `workhive tools` | — | Manage global tools registry |
| `workhive open` | — | Create or enter Worktree |
| `workhive attach` | — | Attach to existing session |
| `workhive close` | — | Detach or terminate current workspace |
| `workhive pane` | — | Manage panes (open / close / split) |
| `workhive list` | `workhive ls` | Global workspace dashboard |
| `workhive remove` | `workhive rm` | Remove Worktree and session |
| `workhive doctor` | — | Check environment health |
| `workhive rename` | — | Rename a workspace (worktree + session + state) |
| `workhive gc` | — | Clean up merged branches and stale resources |
| `workhive destroy` | — | Remove all workhive resources |
| `workhive shell` | — | Manage shell integration (aliases, prompt) |
| `workhive upgrade` | — | Check for updates and upgrade |
| `workhive log` | — | View workhive logs |

### `workhive init` — Initialize Configuration

Creates `.workhive/config.yaml` in the current Git repository, or `~/.config/workhive/config.yaml` in global mode.

```bash
workhive init                      # Interactive setup wizard (project-level)
workhive init --global             # Initialize global config (user-level defaults)
workhive init --non-interactive    # Use defaults, skip prompts
workhive init --force              # Overwrite existing config
```

| Option | Description |
|--------|-------------|
| `--global` | Initialize global config (`~/.config/workhive/config.yaml`) |
| `--non-interactive` | Use defaults, skip all prompts |
| `--force` | Force re-initialization, overwrite existing config |
| `-h, --help` | Show help |

**Project mode** (`workhive init`): Checks project-level config only. If no `.workhive/config.yaml` exists, enters the full setup wizard (language → template → theme). Also generates a unique instance ID for session isolation.

**Global mode** (`workhive init --global`): Checks global config only. Creates user-level defaults that apply to all projects without their own config.

**Flow:** Check dependencies (Git, tmux, yq) → Choose workflow template (including Custom) → Configure pane commands → Choose tmux theme → Generate `.workhive/config.yaml`.

> **Custom template:** Choose "Custom" to define a column-based layout interactively. You specify the number of columns, panes per column, and commands for each pane. All templates generate a unified Column-based DSL configuration.

---

### `workhive config` — Configuration Management

View and modify workhive configuration.

```bash
workhive config                    # Interactive view/edit
workhive config --edit              # Open config in editor
workhive config set <key> <value>  # Set a config value
workhive config set <key> <value> --global  # Set a global config value
workhive config get <key>          # Get a config value
workhive config get <key> --global # Get a global config value
```

| Subcommand / Option | Description |
|---------------------|-------------|
| `set <key> <value>` | Set a config value using dot-path key (e.g. `tmux.theme`) |
| `get <key>` | Get a config value using dot-path key |
| `--edit` | Open config file in editor |
| `--global` | Target global config instead of project config |
| `-h, --help` | Show help |

**Examples:**

```bash
workhive config set tmux.theme tokyo-night
workhive config set tmux.persistence.enabled true --global
workhive config get tmux.theme
```

---

### `workhive tools` — Global Tools Registry

Manage global tools registry (`~/.config/workhive/config.yaml`).

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

| Subcommand | Description |
|-----------|-------------|
| `list` | List configured tools |
| `add` | Add a tool (`--name`, `--cmd` required) |
| `edit` | Edit an existing tool |
| `remove` | Remove a tool |
| `set-default` | Set default tool |
| `validate` | Validate tools config |
| `manage` | Open interactive manager |
| `-h, --help` | Show help |

> `tools` is global-only. Project-level tool config is ignored.
> Run `workhive init --global` once before first use.

---

### `workhive open [worktree]` — Create or Enter Worktree

Creates an isolated development environment for a branch. Creates Worktree and session if they don't exist. If the session already exists, attaches directly without recreating.

```bash
workhive open feature-x                # Create Worktree + session + layout
workhive open                          # Interactive: existing workspace / new branch / main dir
workhive open feature-x --force-layout # Rebuild layout with latest config.yaml
workhive open feature-x --force        # Destroy old session and recreate everything
workhive open feature-x --no-attach    # Create but don't attach (for batch creation)
```

| Argument/Option | Description |
|----------------|-------------|
| `<worktree>` | Worktree name (branch name); omit for interactive menu |
| `--force-layout` | Force rebuild layout from `config.yaml` (use after editing config) |
| `--force, -f` | Force recreate session and layout (destroys old session) |
| `--no-attach` | Don't attach after creation (for batch creation) |
| `-h, --help` | Show help |

**Flow:** Create Git Worktree → `.workhive/worktrees/<name>` → Create tmux session → Apply layout and start commands → Attach.

> **open vs attach:**
>
> - `workhive open` — Creates Worktree + session + layout if needed, then attaches. If the session already exists and layout is intact, it simply attaches (no recreation).
> - `workhive attach` — Attaches to an existing tmux session only. It never creates worktrees or sessions.
>
> **When to use which:**
>
> | Scenario | Command |
> |----------|---------|
> | First time entering a branch | `workhive open feature-x` |
> | SSH reconnect / detach recovery (session still running) | `workhive attach feature-x` |
> | After editing `config.yaml` (width/height/commands) | `workhive open feature-x --force-layout` or `workhive attach feature-x --force-layout` |
> | Complete reset (destroy and recreate) | `workhive open feature-x --force` |

---

### `workhive attach [session|branch]` — Attach to Existing Session

Reconnect to an existing tmux session.

```bash
workhive attach feature-x                # Attach via branch shortcut
workhive attach wh@id@proj@feature-x     # Attach via full session name
workhive attach                          # Interactive: choose from active sessions
workhive attach main -r                  # Read-only mode (tmux 3.0+)
workhive attach feature-x --force-layout # Rebuild layout with latest config.yaml
```

| Argument/Option | Description |
|-----------------|-------------|
| `<session|branch>` | Existing session name, or its branch shortcut; omit for interactive menu |
| `--force-layout` | Force rebuild layout from `config.yaml` (use after editing config) |
| `-r, --read-only` | Read-only; view only (requires tmux 3.0+) |
| `-h, --help` | Show help |

**Behavior:** If the session exists, attaches directly (uses `switch-client` if already in tmux). If the session does not exist, attach fails and lists available active sessions. Use `workhive open <worktree>` to create a new workspace/session. With `--force-layout`, layout is rebuilt from the latest `config.yaml` for the target existing session.

---

---

### `workhive close` — Close Current Workspace

> **Important:** This command can ONLY be used inside a tmux pane (i.e. inside a workhive workspace).

Close or detach from the current workhive workspace. Presents a TUI selection menu with two options:

1. **Detach only** (default, recommended) — Leave the session running in the background. You can return later with `workhive attach`.
2. **Terminate session** — Kill the tmux session entirely.

```bash
workhive close                  # Interactive: choose detach or terminate
workhive close --force          # Detach immediately (keep session)
workhive close --kill           # Terminate session directly (skip menu)
```

| Option | Description |
|--------|-------------|
| `--force, -f` | Detach immediately (skip menu, keep session) |
| `--kill` | Terminate session directly (skip selection menu) |
| `-h, --help` | Show help |

**Notes:**
- **Detach** preserves your session state — all panes, running processes, and layout remain intact
- **Terminate** only closes the tmux session; does **not** remove the worktree or branch
- To fully remove a workspace, use: `workhive rm <branch>`
- To re-enter after detach: `workhive attach <branch>`

> **close vs remove:**
>
> | Command | Session | Worktree | Branch |
> |---------|---------|----------|--------|
> | `workhive close` (detach) | ⏸️ Preserved | ❌ Kept | ❌ Kept |
> | `workhive close` (terminate) | ✅ Killed | ❌ Kept | ❌ Kept |
> | `workhive rm <branch>` | ✅ Killed | ✅ Removed | ✅ Deleted |

---

### `workhive list` / `workhive ls` — Global Workspace Dashboard

Display a comprehensive dashboard of all Worktree workspaces with session status, layout validity, Git status, and last activity.

```bash
workhive list                    # Table format dashboard
workhive ls --format json        # JSON output for scripting
workhive list --layout           # Show layout details
workhive list --watch            # Watch mode (auto-refresh)
workhive list --interval 2       # Watch refresh interval (seconds)
```

| Option | Description |
|--------|-------------|
| `--format <table\|json>` | Output format; default: table |
| `--layout` | Show layout details |
| `--watch` | Auto-refresh dashboard |
| `--interval <seconds>` | Refresh interval in watch mode (default: 5) |
| `-h, --help` | Show help |

**Output columns:**

| Column | Description |
|--------|-------------|
| Branch | Git branch name |
| Worktree | Whether Worktree directory exists (✓/✗) |
| Session | Session status: ● active / ○ detached / - none |
| Layout | Whether layout is valid (✓/✗) |
| Git Status | Working tree status: clean / 3M 1? / - |
| Last Activity | Time since last activity: just now / 5m ago / 2h ago |

**Use cases:**

- **Overview at a glance** — See all workspaces, their health, and Git status in one view
- **Scripting** — Use `--format json` for programmatic access to workspace state
- **Stale workspace detection** — Identify workspaces with no recent activity

> Note: `list/ls` is the single workspace overview command.

---

### `workhive pane` — Manage Panes (Open / Close / Split / Send / List / Current)

Manage panes in the current tmux session. Eight subcommands:

```bash
# Open tool in current pane
workhive pane open                        # Interactive: select Worktree → select command
workhive pane open dev01                  # cd to dev01, then select command
workhive pane open dev01 --cmd claude     # Non-interactive: open claude directly
workhive pane open dev01 --cmd claude -f  # Auto-create worktree + open claude

# Close current pane
workhive pane close                  # Close pane (with confirmation)
workhive pane close -f               # Close pane (skip confirmation)

# Split current pane below
workhive pane split                  # New shell below current pane
workhive pane split --cmd lazygit    # New pane below running lazygit

# Send text to a pane
workhive pane send 1 "npm test"      # Send "npm test" + Enter to pane 1
workhive pane send 2 "hello" --commit none  # Send text without Enter
workhive pane send 3 "run task" --mode auto --commit auto  # Auto-adapt by target pane
workhive pane send 1,2,3 "npm test"  # Send to selected panes
workhive pane send --all "npm test"  # Broadcast to all panes in current session

# List panes
workhive pane list                   # List panes with runtime state + AI agent status

# Show current pane details
workhive pane current                # Show detailed metadata for current pane

# Change layout config
workhive config --layout
workhive config --global --layout
```

| Subcommand / Option | Description |
|---------------------|-------------|
| `open [worktree]` | Open a tool in the current pane |
| `open --cmd <alias>` | Skip interactive menu; aliases: `claude`, `codex`, `opencode`, `lazygit`, `yazi`, `shell` |
| `open -f, --force` | Auto-create worktree if not exists (skip confirmation) |
| `close` | Close the current pane (cannot close the last pane) |
| `close -f, --force` | Close without confirmation (for AI automation) |
| `split [--cmd <cmd>]` | Split below current pane, preserving left/right layout |
| `split --size <pct>` | Split with custom percentage (default: 50) |
| `send <pane> <text>` | Send text to a specific pane (literal mode, CJK-safe) |
| `send <p1,p2,...> <text>` | Send text to multiple specified panes |
| `send --all <text>` | Broadcast text to all panes in current session |
| `send --mode <mode>` | Injection mode: `auto`, `plain`, `paste`, `bracketed-paste` |
| `send --commit <mode>` | Commit behavior: `auto`, `enter`, `c-m`, `none` |
| `send --verbose` | Print resolved send strategy metadata for each target pane |
| `list` | List all panes with index, command, directory, size, status, AI agent, and agent status (`running`/`waiting`/`idle`/`unknown`) |
| `current` | Show detailed metadata for the current pane (supports `--format table|json`) |
| `-h, --help` | Show help (each subcommand has its own --help) |

`pane list --format json` additionally includes per-pane state metadata:
`agent_status`, `agent_state_source`, `agent_state_confidence`, `agent_state_reason`.

**Design:** Each subcommand has its own `--help` output. For example, `workhive pane split --help` shows split-specific help, while `workhive pane --help` shows the overview.

> **Command hierarchy:**
>
> - `workhive open` — **Workspace-level**: creates or enters an entire Worktree workspace (with tmux session + layout)
> - `workhive pane open` — **Pane-level**: opens a tool inside the current pane of an existing workspace

### Multi-Task Parallel Development (AI Skill)

workhive includes a built-in skill for AI coding assistants (Claude Code, Codex, OpenCode) that enables **multi-task parallel development**. When you provide a numbered list of tasks, the AI can:

1. **Analyze & group** related tasks into modules
2. **Create worktrees** and open AI tools in parallel panes
3. **Dispatch tasks** to each pane's AI instance
4. **Monitor progress** via git log and pane output
5. **Merge & cleanup** all branches back to main

**Trigger phrases:** "use workhive multi-task", "parallel development", "split tasks into branches", or simply provide a numbered task list.

**Example workflow:**
```bash
# AI splits panes and opens claude for each task group
workhive pane split --cmd "workhive pane open task-auth --cmd claude -f"
workhive pane split --cmd "workhive pane open task-search --cmd claude -f"

# AI sends tasks to each pane
workhive pane send 2 "Implement user login and registration. Commit when done."
workhive pane send 3 "Create search API and results page. Commit when done."

# AI monitors and merges when done
git merge task-auth --no-ff -m "merge: auth features"
workhive rm task-auth --force
```

The skill is located at `skills/workhive/SKILL.md` and is automatically loaded by Claude Code, Cursor, and other AI tools that support skills

---

### `workhive remove` / `workhive rm` — Remove Worktree

Close tmux session and optionally remove Worktree and Git branch.

```bash
workhive rm feature-x                    # Full removal (session + Worktree + branch)
workhive rm feature-x --keep-worktree    # Close session only, keep code and branch
workhive rm feature-x --force            # Force remove (discard uncommitted changes)
```

| Argument/Option | Description |
|-----------------|-------------|
| `<worktree>` | Workspace name (required) |
| `--keep-worktree` | Keep Worktree and Git branch |
| `--remove-worktree` | Remove Worktree and branch (default) |
| `--force` | Skip confirmation for uncommitted changes |
| `-h, --help` | Show help |

**Flow:** Close tmux session → Remove Worktree (optional) → Remove Git branch (optional). Default behavior is controlled by `worktree.auto_delete` in config. Prompts for confirmation when there are uncommitted changes.

---

### `workhive doctor` — Check Environment Health

Run a comprehensive health check of your development environment. Verifies dependencies, configuration, tmux compatibility, and Git repository status.

```bash
workhive doctor                  # Table format health report
workhive doctor --format json    # JSON output for CI/scripting
```

| Option | Description |
|--------|-------------|
| `--format <table\|json>` | Output format; default: table |
| `-h, --help` | Show help |

**Checks performed:**

| Category | Tools | Severity |
|----------|-------|----------|
| Required dependencies | git, tmux, yq | ✗ Error (blocks usage) |
| Pane tools | lazygit, claude, codex, opencode, yazi | ○ Info (optional, defaults to Shell if missing) |
| Config validation | `.workhive/config.yaml` syntax | ✗ Error if invalid |
| tmux version | Popup support (3.3+) | ○ Warning if below 3.3 |
| Git repository | Working tree detection | ○ Info |

**Use cases:**

- **Initial setup** — Verify all dependencies are installed before first use
- **Troubleshooting** — Diagnose why a feature isn't working
- **CI/CD** — Validate environment readiness in automated pipelines with `--format json`
- **Team onboarding** — Share `workhive doctor` output to identify missing tools

---

### `workhive gc` — Garbage Collection

Clean up worktrees for branches that have been merged, orphan sessions without worktrees, and stale state files. Keeps your workspace tidy.

```bash
workhive gc                # Interactive cleanup with confirmation
workhive gc --auto         # Auto cleanup without confirmation
workhive gc --snapshots    # Clean snapshot files only
```

| Option | Description |
|--------|-------------|
| `--auto` | Auto-clean without confirmation prompts |
| `--snapshots` | Clean snapshot files only (`.workhive/snapshots/`) |
| `-h, --help` | Show help |

**What gets cleaned:**

| Resource | Condition | Action |
|----------|-----------|--------|
| Merged branch worktrees | Branch merged into main | Remove worktree + session + branch |
| Orphan sessions | tmux session exists but no worktree | Kill session |
| Stale state files | Files in `.workhive/state/` with no matching worktree or session | Delete files |

**Use cases:**

- **Post-merge cleanup** — After merging PRs, clean up leftover worktrees and sessions in one command
- **Disk space recovery** — Remove worktrees for branches that are no longer needed
- **Session hygiene** — Kill orphan tmux sessions that lost their worktrees
- **Automated maintenance** — Use `--auto` in cron jobs or CI for periodic cleanup
- **Snapshot reset** — Use `--snapshots` when you want to discard saved session snapshots only

**Flow:**
1. Scan for branches merged into main → remove their worktrees, sessions, and branches
2. Detect orphan sessions (session without worktree) → kill them
3. Find stale state files → delete them
4. Run `git worktree prune` to clean Git's worktree records

---

### `workhive destroy` — Remove All workhive Resources

Remove all workhive resources and reset to an uninitialized state.

```bash
workhive destroy                  # Interactive confirmation, then destroy
workhive destroy --force          # Skip confirmation, destroy immediately
```

| Option | Description |
|--------|-------------|
| `--force` | Skip all confirmation prompts |
| `-h, --help` | Show help |

**This command will:**
1. Close all workhive-managed tmux sessions
2. Delete all Worktrees (code + Git branches)
3. Delete `.workhive/` directory (config, logs, cache)

**Notes:** Irreversible. Uncommitted code will be lost. Does not delete project code or `.git`. Run `workhive init` to reinitialize afterward.

---

### `workhive rename` — Rename a Workspace

Rename a workhive workspace — Git branch, worktree directory, tmux session, and state files are all updated atomically with automatic rollback on failure.

```bash
workhive rename old-branch new-branch         # Rename workspace
workhive rename old-branch new-branch --force # Force rename even if session is attached
```

| Argument/Option | Description |
|-----------------|-------------|
| `<old_branch>` | Current branch/worktree name |
| `<new_branch>` | New branch/worktree name |
| `--force, -f` | Force rename even if session is attached |
| `-h, --help` | Show help |

**Flow:** Rename Git branch → Move worktree directory → Rename tmux session → Rename state files. Fails if the old branch doesn't exist or the new name is already taken. Cannot rename the main branch. Uses transactional rollback if any step fails.

---

### `workhive log` — View Logs

View workhive operation logs with filtering and real-time follow.

```bash
workhive log                        # Show last 50 lines
workhive log --follow               # Real-time follow (like tail -f)
workhive log --lines 100            # Show last 100 lines
workhive log --level error          # Filter by level: debug|info|warn|error
workhive log --clear                # Clear log file
```

| Option | Description |
|--------|-------------|
| `--follow` | Real-time log following |
| `--lines <N>` | Number of lines to show (default: 50) |
| `--clear` | Clear the log file |
| `--level <level>` | Filter by level: `debug` / `info` / `warn` / `error` |
| `-h, --help` | Show help |

---

### `workhive shell` — Shell Integration

Manage shell integration from one entrypoint (minimal by default, module-based extensions).

```bash
workhive shell setup                                 # Interactive setup
workhive shell setup --shell zsh --profile minimal  # Minimal setup
workhive shell setup --profile standard --enable zoxide --yes
workhive shell init --shell zsh                     # Alias of setup
workhive shell remove                # Remove integration
workhive shell show                  # Print integration script
```

| Subcommand | Description |
|-----------|-------------|
| `setup` | Generate and apply shell integration from profile + modules |
| `init` | Alias of `setup` |
| `remove` | Remove shell integration from your rc file |
| `show` | Print the integration script to stdout |

| Option | Description |
|--------|-------------|
| `--shell <shell>` | Target shell: `zsh`, `bash` (default: auto-detect) |
| `--profile <name>` | Profile: `minimal` / `standard` / `advanced` |
| `--enable <csv>` | Enable modules: `completions,aliases,history,zoxide,starship,ssh-term-fix` (skips per-module interactive customization when provided) |
| `--disable <csv>` | Disable modules from selected profile (skips per-module interactive customization when provided) |
| `--yes` | Non-interactive mode (defaults to `minimal`) |
| `--dry-run` | Preview changes without modifying files |
| `-h, --help` | Show help |

**What it provides:**

| Feature | Description |
|---------|-------------|
| Git aliases | `g`, `ga`, `gst`, `gco`, `gd`, `gp`, `gl`, etc. |
| Directory aliases | `ll`, `la`, `...`, `....`, `md` |
| Smart history | 50k entries, dedup, shared across sessions |
| Zoxide integration | Smart directory jumping (`z` command, if installed) |
| Starship prompt | Modern cross-shell prompt (if installed) |
| Optimized compinit | Cached completions rebuild (zsh, ~30ms saved) |

> Managed integration scripts are stored at `~/.config/workhive/shell/workhive.{zsh,bash}`. `setup` maintains a single `source` entry in your rc file.

---

### `workhive upgrade` — Check for Updates and Upgrade

Update workhive to the latest version from GitHub.

```bash
workhive upgrade                     # Upgrade to latest version
workhive upgrade --check             # Check if update is available
workhive upgrade --version v0.3.0    # Install specific version
```

| Option | Description |
|--------|-------------|
| `--check` | Check for updates only (don't install) |
| `--version <tag>` | Install specific version |
| `--force` | Force reinstall even if already up-to-date |
| `-h, --help` | Show help |

**Auto-detects install method:**
- **Git clone** — Uses `git pull` to update
- **Install script** — Re-runs `install.sh` with same prefix
- **Development** — Shows manual update instructions

> `upgrade --check` requires GitHub network access. In offline/restricted networks, it may only print a warning and fallback link.

---

### Global Options

```bash
workhive --version               # Show version
workhive --help                  # Show help
workhive --debug <command>       # Enable debug output
```

---

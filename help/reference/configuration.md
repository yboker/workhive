## Configuration

Config file: `.workhive/config.yaml`, created by `workhive init` or editable manually.
Language: [English](configuration.md) | [Chinese](configuration.zh-CN.md)

### Full Configuration Reference

Every key in `config.yaml` is listed below. All keys are optional — omitted keys use built-in defaults.
`workhive init` writes this schema. `workhive config` validates keys strictly and does not backfill legacy keys.

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `version` | string | (current) | Config file version (auto-set by `workhive init`) |
| `language` | string | `"en"` | UI language: `"en"` (English) or `"zh"` (Chinese) |
| **Layout (Column-based DSL)** | | | |
| `layout.columns` | array | (2-col default) | Array of column definitions |
| `layout.columns[].width` | int | 50 | Column width as percentage of total width |
| `layout.columns[].panes` | array | — | Array of pane definitions for this column |
| `layout.columns[].panes[].height` | int | 50 | Pane height as percentage of column height |
| `layout.columns[].panes[].command` | string\|null | null | Command to run in pane (null = empty shell) |
| `layout.columns[].panes[].role` | string\|null | null | Optional pane role for role-based addressing |
| **Worktree** | | | |
| `worktree.base_path` | string | `".workhive/worktrees"` | Worktree storage directory (relative to git root) |
| `worktree.auto_create` | bool | `true` | Auto-create worktree on `open` |
| `worktree.auto_delete` | bool | `true` | Auto-delete worktree on `remove` |
| `worktree.sync_files` | array | `[]` | Files to sync to new worktrees (see below) |
| `worktree.sync_files[].source` | string | — | Source file/dir path (relative to git root) |
| `worktree.sync_files[].mode` | string | `"copy"` | Sync mode: `"copy"` or `"symlink"` |
| **tmux** | | | |
| | | | **Dedicated runtime:** workhive runs a dedicated tmux server per project instance (`workhive-<project>-<instance_id>`). All tmux commands are routed through this server via `tmux -L <server>`. This isolates workhive from the default tmux server and other project instances. Attach strategy is selected automatically: `same-server` (`switch-client`), `cross-server` (fresh `tmux attach`), or `outside-tmux` (direct attach). |
| `tmux.session_prefix` | string\|null | null | Custom session name prefix (null = auto: `wh@<id>@<project>`) |
| `tmux.mouse_support` | bool | `true` | Enable tmux mouse support |
| `tmux.interaction_profile` | string | `"workspace"` | Mouse-wheel interaction policy. `workspace`: agent panes scroll transcript by pane metadata, shell panes do not enter copy-mode implicitly. `compatible`: closer to default tmux wheel behavior |
| `tmux.env` | map | `{ CLAUDE_CODE_NO_FLICKER: 1 }` | Startup env vars injected into pane commands (`project > global`) |
| `tmux.theme` | string | `"catppuccin-mocha"` | Status bar theme (see available themes below) |
| `tmux.attach_delay` | duration | `0s` | Optional delay before auto-attach. When unset, workhive defaults to immediate attach after readiness checks |
| **Snapshot Restore** | | | |
| `tmux.persistence.enabled` | bool | `false` | Enable snapshot save/restore for tmux sessions |
| `tmux.persistence.generation` | int | `0` | Snapshot generation fence; incremented automatically when re-enabling snapshot restore |
| `tmux.persistence.auto_save_mode` | string | `"hybrid"` | Auto-save strategy: `hybrid` (event + timer), `event`, or `interval` |
| `tmux.persistence.auto_save_debounce` | duration | `3s` | Debounce window for event-triggered saves |
| `tmux.persistence.auto_save_interval` | duration | `10m` | Timer interval used by `interval`/`hybrid` modes |
| `tmux.persistence.auto_save_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1 }` | Env vars injected into internal auto-save runs. Only explicit config values are used |
| `tmux.persistence.pre_kill_env` | map | `{ WORKHIVE_SAVE_LOCK_QUIET: 1, WORKHIVE_SAVE_QUALITY_QUIET: 1, WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1 }` | Env vars injected into pre-kill snapshot saves. Only explicit config values are used |
| `tmux.persistence.restore_processes` | bool | `true` | Restart pane processes on restore |
| `tmux.persistence.snapshot_history_limit` | int | `8` | Max archived snapshots to keep (excluding `wh_snapshot_latest.jsonl` and its target) |
| `tmux.persistence.snapshot_max_size_mb` | int | `8` | Total archived snapshot size cap in MB (`0` = disabled) |
| `maintenance.temp_cleanup.max_age_days` | int | `7` | Auto-clean stale temp config files older than N days |
| `maintenance.log.path` | string | `".workhive/logs/workhive.log"` | Runtime file log path (absolute, or relative to git root) |
| `maintenance.test_log.path` | string | `"/dev/null"` | Test-mode file log path (`/dev/null` means no persisted test log file) |
| `maintenance.test_log.level` | string | inherit `maintenance.log.level` | Optional test-log threshold override: `debug`, `info`, `warn`, `error` |
| `maintenance.test_log.format` | string | inherit `maintenance.log.format` | Optional test-log format override: `text` or one-line `json` |
| `maintenance.test_log.max_size_mb` | int | inherit `maintenance.log.max_size_mb` | Optional per-file test-log size threshold in MB for rotation |
| `maintenance.test_log.max_backups` | int | inherit `maintenance.log.max_backups` | Optional number of rotated test-log backups (`0` = truncate mode) |
| `maintenance.log.level` | string | `"info"` | File log threshold: `debug`, `info`, `warn`, `error` |
| `maintenance.log.format` | string | `"text"` | File log format: `text` or one-line `json` |
| `maintenance.log.max_size_mb` | int | `10` | Per-file log size threshold in MB for rotation |
| `maintenance.log.max_backups` | int | `5` | Number of rotated backups to keep (`0` = truncate mode) |
| **Git Init Safety Guard (global-only)** | | | |
| `git_init.whitelist_paths` | string[] | `[]` | Directories (and subdirs) allowed for auto git-init |
| `git_init.dangerous_paths_extra` | string[] | *OS-specific* | Additional blocked directories (exact-match only); auto-populated on `workhive init --global` |
| **Tools Registry** | | | |
| `tools.default` | string | `"__empty__"` | Default tool ref (`tool_name` or special refs `__empty__`, `__custom__`) |
| `tools.list` | array | built-in list | Global pane tool registry records |
| `tools.list[].tool_name` | string | — | Tool name (globally unique, used by `set-default`) |
| `tools.list[].label` | string | `tool_name` | Display label in `workhive tools list` |
| `tools.list[].command` | string | — | Executable command |
| `tools.list[].is_ai` | bool | `false` | Mark as AI-capable tool for AI detection/listing |
| `tools.list[].enabled` | bool | `true` | Enable/disable from picker |

> `tools` is global-only. Prefer `workhive tools list|add|edit|remove|set-default|validate|manage` for lifecycle management.
> `git_init.*` and `tools.*` are global-only. Project config does not persist these keys.

> **Command values:** Use `null` for an empty shell, or a quoted string like `"claude --dangerously-skip-permissions"` for a specific tool. The first word of the command must be an installed executable.

> **Available themes:** `catppuccin-mocha` · `tokyo-night` · `dracula` · `nord` · `gruvbox` · `rose-pine` · `everforest` · `onedark`

### Config Priority

1. **Project** — `.workhive/config.yaml` (highest)
2. **User** — `~/.config/workhive/config.yaml`
3. **Built-in defaults** (lowest)

### Layout Configuration (Column-based DSL)

workhive uses a unified **Column-based DSL** for all layouts. Every layout is defined as an array of columns, each with independent width and pane definitions.

#### Width & Height

`width` and `height` values are **percentages** (0-100):

| Property | Meaning | Scope |
|----------|---------|-------|
| `columns[].width` | Column width as percentage of total width | Each column |
| `columns[].panes[].height` | Pane height as percentage of its column | Each pane |

> **How to apply**: After editing `.workhive/config.yaml`, use `--force-layout` to rebuild:
>
> ```bash
> workhive open <branch> --force-layout    # Rebuild layout with latest config
> workhive attach <branch> --force-layout  # Same effect via attach
> ```
>
> Alternative methods:
>
> ```bash
> workhive config --layout
> workhive config --global --layout
> workhive remove <branch>     # Or: close + reopen
> workhive open <branch>
> ```

#### AI Coding Layout (2 columns, 2+1 panes)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null                    # Empty shell
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

#### Quad Layout (2 columns, 2+2 panes)

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 70
          command: "lazygit"
        - height: 30
          command: "yazi"
```

#### Multi-Agent Layout (2 columns, 4+1 panes with roles)

```yaml
layout:
  columns:
    - width: 50
      panes:
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent1"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent2"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent3"
        - height: 25
          command: "claude --dangerously-skip-permissions"
          role: "agent4"
    - width: 50
      panes:
        - height: 100
          command: null
          role: "preview"
```

> **Pane roles** allow addressing panes by name: `workhive pane send --role preview "preview path/to/file"`

#### 3-Column Layout (3 columns)

```yaml
layout:
  columns:
    - width: 33
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 34
      panes:
        - height: 100
          command: "lazygit"
    - width: 33
      panes:
        - height: 50
          command: "npm run dev"
        - height: 50
          command: "npm test -- --watch"
```

#### Single Column (1 column, 3 stacked panes)

```yaml
layout:
  columns:
    - width: 100
      panes:
        - height: 33
          command: "claude --dangerously-skip-permissions"
        - height: 33
          command: null
        - height: 34
          command: "lazygit"
```

### Worktree Configuration

```yaml
worktree:
  base_path: ".workhive/worktrees"     # Worktree directory
  auto_create: true                    # Auto-create on open
  auto_delete: true                    # Auto-delete on remove
  sync_files:                              # Files to sync to new worktrees
    - source: ".env"
      mode: "copy"                         # copy: duplicate file
    - source: "node_modules"
      mode: "symlink"                      # symlink: create symbolic link
```

When a new worktree is created via `workhive open`, files listed in `sync_files` are automatically copied or symlinked from the project root. This ensures environment files (`.env`), dependencies (`node_modules`), and other untracked files are available immediately.

### tmux Configuration

```yaml
tmux:
  # session_prefix: null               # Auto-computed: wh@<instance_id>@<project>
  mouse_support: true                  # Mouse support
  interaction_profile: "workspace"     # Mouse-wheel policy
  env:
    CLAUDE_CODE_NO_FLICKER: 1          # Startup env vars for pane commands
  attach_delay: 0s                      # Optional delay before auto-attach
  theme: "catppuccin-mocha"            # Theme
  persistence:
    auto_save_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
    pre_kill_env:
      WORKHIVE_SAVE_LOCK_QUIET: 1
      WORKHIVE_SAVE_QUALITY_QUIET: 1
      WORKHIVE_SAVE_QUALITY_MAX_RETRIES: 1
```

**Session naming:** workhive automatically generates a unique 4-character hex instance ID per project (stored in `.workhive/instance_id`). Session names follow `wh@<id>@<project>@<branch>`, and the dedicated tmux server follows `workhive-<project>-<instance_id>`. Override the session prefix by uncommenting `session_prefix` with a custom value.

**`tmux.env` / `tmux.persistence.*_env` key rules:** keys must match `[A-Za-z_][A-Za-z0-9_]*`; values must be scalar (`string`/`number`/`bool`). During merge, project config overrides global config. If a config file omits one of these env maps, workhive does not inject fallback env values for that map at runtime.

**Available themes:** `catppuccin-mocha` | `tokyo-night` | `dracula` | `nord` | `gruvbox` | `rose-pine` | `everforest` | `onedark`

### Session Snapshot Restore

workhive includes a built-in tmux snapshot restore module. It saves/restores session structure via snapshots: windows, panes, layout, working directories, and running commands. This works at both **project** and **global** configuration levels.

> Persistence is tmux-specific, so its config lives under the `tmux.persistence` namespace.

#### Enable Snapshot Restore

```bash
# Option 1: Interactive configuration
workhive config              # Select "Session Persistence" (snapshot restore) → Yes

# Option 2: CLI (project-level)
workhive config set tmux.persistence.enabled true

# Option 3: Global (applies to all projects)
workhive config set tmux.persistence.enabled true --global

# During initialization
workhive init                # Step 4 will ask about persistence
```

#### Configuration (`config.yaml`)

```yaml
tmux:
  persistence:
    enabled: true               # Enable/disable snapshot save and restore
    generation: 0               # Snapshot generation fence (usually do not edit manually)
    auto_save_mode: hybrid      # Auto-save strategy: hybrid / event / interval
    auto_save_debounce: 3s  # Event-save debounce in seconds
    auto_save_interval: 10m       # Timer interval in minutes (shared by hybrid/interval)
    restore_processes: true     # Restart pane processes on restore
    snapshot_history_limit: 8   # Max archived snapshots (excluding latest pointer and its target)
    snapshot_max_size_mb: 8     # Max total archived snapshot size (MB)
```

#### Field Value Guide (Allowed Values)

| Field | Type | Allowed values/format | Meaning |
|------|------|------------------------|---------|
| `tmux.persistence.enabled` | bool | `true` / `false` | Enables snapshot save/restore |
| `tmux.persistence.generation` | int | integer `>= 0` | Snapshot generation fence. It auto-increments when `enabled` switches `false -> true` |
| `tmux.persistence.auto_save_mode` | enum | `hybrid` / `event` / `interval` | `hybrid` = event + timer, `event` = event only, `interval` = timer only |
| `tmux.persistence.auto_save_debounce` | duration | e.g. `3s`, `30s`, `1m`, `2h` | Debounce window for event-triggered saves |
| `tmux.persistence.auto_save_interval` | duration | e.g. `1m`, `10m`, `1h` | Timer interval used in `interval`/`hybrid` mode |
| `tmux.persistence.restore_processes` | bool | `true` / `false` | Whether to restart pane processes on restore |
| `tmux.persistence.snapshot_history_limit` | int | `>= 1` | Max archived snapshots kept (excluding `wh_snapshot_latest.jsonl` and its target) |
| `tmux.persistence.snapshot_max_size_mb` | int | `>= 0` | Max total archived snapshot size in MB (`0` = no size limit) |

Notes:
- Duration keys currently support `s` / `m` / `h` suffixes (seconds/minutes/hours).
- `tmux.attach_delay` is also a duration key (common values: `0s`, `1s`, `2s`, `5s`).

#### What Gets Saved

| Item | Saved? | Notes |
|------|--------|-------|
| Windows and panes | ✅ Yes | Layout, sizes, positions |
| Working directories | ✅ Yes | Each pane's current directory |
| Pane commands | ✅ Yes | Running commands (lazygit, claude, etc.) |
| Environment variables | ❌ No | Shell env vars are not saved |
| Pane content/history | ❌ No | Terminal scrollback/history is not stored in snapshots; transcript scrolling is handled at runtime by tmux interaction policy |

#### Snapshot Storage

Snapshots are stored per-project in `.workhive/snapshots/` as JSONL files:

- `wh_snapshot_latest.jsonl` points to the latest snapshot.
- Archived snapshots use `wh_snapshot_archive_<timestamp>.jsonl`.
- `snapshot_history_limit` counts archived snapshots only (excluding `wh_snapshot_latest.jsonl` and its target).

#### How It Works

```
workhive open/attach
    │
    ├── persistence.enabled = false?
    │   └── Create session from config.yaml layout (standard flow)
    │
    └── persistence.enabled = true?
        ├── Snapshot exists
        │   ├── All pane directories still exist?
        │   │   └── Restore from snapshot: rebuild windows/panes/layout/cwd
        │   │       └── restore_processes = true? → restart restorable processes
        │   │
        │   └── Any pane directory missing? (e.g. project moved)
        │       └── Discard snapshot, create session from config.yaml layout
        │
        └── No snapshot?
            └── Create session from config.yaml layout (standard flow)

While session is running:
    ├── auto_save_mode=event: save on pane/window/layout/session events only
    ├── auto_save_mode=interval: save by timer (auto_save_interval)
    ├── Event-driven saves are debounced by auto_save_debounce
    ├── Save snapshot on user detach
    ├── Save snapshot before workhive close
    └── auto_save_mode=hybrid: event-driven save + timer (auto_save_interval)

Snapshot cleanup:
    ├── Prune by count (snapshot_history_limit)
    └── Prune by total size (snapshot_max_size_mb)
```

**Key points:**
- Snapshots use versioned JSONL records (current schema in meta line)
- Snapshots save tmux session structure (windows, pane layout, cwd, commands), not terminal content
- On restore, pane directories are validated with automatic relocation remap — if the project directory has moved but maintains the same basename, CWDs are remapped to the new location. If remap fails, the snapshot is considered stale and the session falls back to `config.yaml` layout
- Restore commands use strategy-aware validation: TUI/AI tools (vim, lazygit, claude, etc.) are restored directly; other commands are checked for injection metacharacters (`;`, `&&`, `` ` ``, `$()`) before execution. This conservative mode may skip restoring commands with complex shell syntax
- Snapshot writes are semantic-deduplicated; unchanged runtime state does not generate a new archive
- Temporary snapshot files use `mktemp` for unpredictable filenames, preventing symlink clobber attacks
- Debug logs record only command names and argument counts (never raw arguments) to prevent information leaks
- Default restorable processes include: `vim`, `nvim`, `lazygit`, `yazi`, `htop`, `top`, etc.

#### Tuning Examples

```bash
# Enable persistence
workhive config set tmux.persistence.enabled true

# Small-footprint defaults (recommended for most projects)
workhive config set tmux.persistence.snapshot_history_limit 8
workhive config set tmux.persistence.snapshot_max_size_mb 8

# Auto-save strategy
workhive config set tmux.persistence.auto_save_mode hybrid
workhive config set tmux.persistence.auto_save_debounce 3s
workhive config set tmux.persistence.auto_save_interval 10m
```

#### Checking Snapshot Restore Status

```bash
# Via the list command
workhive list          # Shows snapshot restore status and latest snapshot time at the bottom

# Via the doctor command
workhive doctor        # Shows snapshot module health, snapshot count, latest snapshot

# View snapshot config
workhive config        # Shows enabled/auto_save_interval
```

#### Disabling

```bash
workhive config set tmux.persistence.enabled false
```

### Example Configurations

**AI-assisted development (Claude + Shell + Lazygit):**

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "claude --dangerously-skip-permissions"
        - height: 50
          command: null
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

**Frontend (editor + Lazygit + dev server):**

```yaml
layout:
  columns:
    - width: 40
      panes:
        - height: 100
          command: "nvim"
    - width: 60
      panes:
        - height: 50
          command: "lazygit"
        - height: 50
          command: "npm run dev"
```

**Backend (editor + tests + Git):**

```yaml
layout:
  columns:
    - width: 60
      panes:
        - height: 50
          command: "nvim"
        - height: 50
          command: "go test -v ./..."
    - width: 40
      panes:
        - height: 100
          command: "lazygit"
```

---

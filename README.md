# workhive

[Chinese documentation](README.zh-CN.md)

workhive is a Rust CLI for tmux-based development workspaces. It combines a git worktree, a tmux session, pane layout restoration, and project configuration into one project-scoped workspace workflow.

## Why workhive?

Working on multiple features at once usually means juggling git branches, terminal windows, and hand-built pane layouts. workhive keeps that state explicit and repeatable:

- **Project-scoped workspaces**: `workhive workspace open feature-auth` creates or resumes an isolated workspace for one branch.
- **tmux runtime management**: each workspace has a predictable tmux session and pane layout.
- **Opt-in snapshot restore**: when enabled, pane layout changes are saved automatically and restored by `workspace open`.
- **Trusted project roots**: global safety policy controls where Workhive may initialize projects, create worktrees, and write runtime state.
- **Runtime control plane**: pane messaging goes through a typed local supervisor instead of ad hoc `tmux send-keys` calls.
- **Global visibility**: project and session lists make it possible to inspect registered projects and active sessions without remembering tmux names.

## Installation

workhive ships as a single binary; the recommended path is installing a
prebuilt artifact — no Rust toolchain required.

Supported runtime platforms are macOS and Linux. Windows users should run
workhive inside WSL2 with Linux `git` and `tmux`; native Windows, PowerShell,
CMD, and Git for Windows are not supported runtime environments.

### Install the prebuilt binary (recommended)

Download the `workhive` binary distributed for your platform, then place it on
your `PATH`:

```bash
install -m 755 workhive ~/.local/bin/workhive   # or /usr/local/bin
workhive --version
```

### Build from source (contributors)

Requires Rust stable (MSRV 1.95.0+):

```bash
cargo build --release
install -m 755 target/release/workhive ~/.local/bin/workhive
```

## Quick Start

```bash
# Initialize global configuration
workhive init --global

# Initialize a project inside a git repository
cd ~/my-project
workhive init

# Open or create a workspace
workhive workspace open feature-auth

# List workspaces in the current project
workhive workspace list

# Attach to an existing workspace
workhive workspace attach feature-auth

# Close the current workspace from inside a workhive pane
workhive workspace close
```

If a project has not been initialized, `workhive workspace open` can guide initialization interactively. In non-interactive use, pass `--init` to explicitly allow project initialization.

## Command Model

workhive keeps system commands at the top level and groups resource operations under resource names.

| Scope | Commands |
|-------|----------|
| System | `workhive init`, `workhive config`, `workhive doctor`, `workhive completion` |
| Workspace | `workhive workspace open`, `list`, `attach`, `close`, `remove`, `destroy` |
| Project index | `workhive project list`, `reindex`, `roots` |
| Sessions | `workhive session list`, `attach` |
| Panes | `workhive pane split`, `close`, `list`, `show`, `send` |
| Worktrees | `workhive workspace worktree list`, `remove` |

Object-scoped `ls` aliases are supported, for example `workhive workspace ls`,
`workhive project ls`, `workhive session ls`, `workhive pane ls`, and
`workhive workspace worktree ls`. Top-level shortcuts such as `workhive ls`
and `workhive worktree` are not part of the command surface.

### Workspace Commands

| Command | Description |
|---------|-------------|
| `workhive workspace open [workspace]` | Open or create a workspace in the current project. Without a name, shows an interactive selector. |
| `workhive workspace open <workspace> --no-attach` | Create or resume the workspace without attaching to tmux. |
| `workhive workspace open <workspace> --mode manual\|primary-assisted` | Select the workspace interaction mode. Default is `manual`. |
| `workhive workspace open <workspace> -f\|--force` | Recreate runtime state from the configured layout and discard the saved snapshot. |
| `workhive workspace open <workspace> --init` | Allow project initialization when the project is not initialized. Required for non-interactive auto-init. |
| `workhive workspace open <workspace> --attach-existing-branch` | Attach a pre-existing local branch that has no Workhive-managed worktree. Without the flag, a name collision prompts for confirmation (interactive) or fails (non-interactive). |
| `workhive workspace list [--view auto\|table\|compact\|json]` | List workspaces in the current project. The default view adapts to terminal width. |
| `workhive workspace attach [workspace]` | Attach to an existing workspace session. Without a name, shows an interactive selector. |
| `workhive workspace close [--kill] [-f\|--force]` | Detach from the current workspace, or terminate its runtime with `--kill`. Must run inside a workhive pane. |
| `workhive workspace remove <workspace> [--delete-branch] [--keep-branch] [-f\|--force]` | Remove one managed workspace. Workhive-created branches are deleted by default; `--delete-branch` authorizes deleting user-owned branches; `--keep-branch` keeps the branch in all cases. |
| `workhive workspace destroy [--delete-branch] [--keep-branch] [-f\|--force]` | Remove all workhive resources for the current project, including workspaces, runtime sessions, snapshots, and `.workhive/`. Branch flags follow the same semantics as `remove`. |

### System and Resource Commands

| Command | Description |
|---------|-------------|
| `workhive init --global` | Create or update global configuration with the interactive wizard. |
| `workhive init` | Initialize workhive for the current project. |
| `workhive config show [--global] [--raw]` | Show the effective project or global configuration. |
| `workhive config explain <key>` | Explain valid values and semantics for a configuration key. |
| `workhive config edit [--global]` | Edit configuration through the interactive wizard. |
| `workhive config set <key> <value> [--global]` | Set a TOML configuration value. |
| `workhive config unset <key> [--global]` | Remove a TOML configuration value. |
| `workhive config env-set <key> <value> [--global]` | Set an environment variable in `.env`. |
| `workhive config env-unset <key> [--global]` | Remove an environment variable from `.env`. |
| `workhive doctor` | Run diagnostic checks for tmux, session lookup, snapshots, PTY availability, and attach readiness. |
| `workhive project list [--view auto\|table\|compact\|json]` | List registered projects from the global index. |
| `workhive project reindex [--root <path>] [--config-roots] [--clear-first] [--dry-run] [-f\|--force]` | Rebuild the global project index. Without explicit roots, configured trusted roots are used. |
| `workhive project roots list\|add\|remove\|edit\|status` | Manage trusted project root directories in global configuration. |
| `workhive session list [--view auto\|table\|compact\|json]` | List active workspace sessions across registered projects. |
| `workhive session attach [target]` | Attach to a Workhive-managed session globally. Target may be a session name, `project/workspace`, or an unambiguous workspace name. |
| `workhive workspace worktree list` | List Workhive-managed linked worktrees under `.workhive/worktrees/`. Alias: `ls`. |
| `workhive workspace worktree remove [worktree] [--delete-branch] [--keep-branch] [--force]` | Remove a managed linked worktree. Without a target, interactive terminals show a selector. Branch flags follow the same semantics as `workspace remove`. |
| `workhive pane split [--direction right\|down\|left\|up] [--cmd <command>] [-f\|--force]` | Split the current pane. Default direction is `down`. |
| `workhive pane close [-f\|--force]` | Close the current pane. |
| `workhive pane list` | List panes in the current session. |
| `workhive pane show [pane]` | Show information about the current pane, or a target pane by its `pane list` number (e.g. `2`) or tmux id (e.g. `%2`). Alias: `current`. |
| `workhive pane send <pane> <text> [--no-enter] [-f\|--force]` | Send text to a target pane. A verified AI receiver pane gets the literal text plus a structured submit event; `--no-enter` inserts text without submitting. Shell, TUI, and unverified panes are refused unless `--force` explicitly opts into raw keystroke input. |

## Shell Completion

Use `workhive completion install` to install completion for the current user.
Workhive detects the current shell from `$SHELL`. Pass a shell name only when
you want to override detection.

Supported shells:

```bash
workhive completion install
workhive completion install zsh
workhive completion install bash
workhive completion install fish
```

PowerShell completion generation is available for shell integration, but it
does not make native Windows a supported runtime platform.

Install locations:

| Shell | Installed file |
|-------|----------------|
| bash | `~/.config/workhive/completions/bash/workhive` |
| zsh | `~/.config/workhive/completions/zsh/_workhive` |
| fish | `~/.config/workhive/completions/fish/workhive.fish` |
| powershell | `~/.config/workhive/completions/powershell/_workhive.ps1` |
| elvish | `~/.config/workhive/completions/elvish/workhive-completions.elv` |

After installation, open a new terminal or reload the current shell:

| Shell | Current-terminal activation |
|-------|-----------------------------|
| zsh | `source ~/.zshrc` |
| bash | `source ~/.bashrc` or `source ~/.bash_profile`, depending on your setup |
| fish | `exec fish` |
| powershell | `. $PROFILE` if your profile imports Workhive completions |
| elvish | reload `rc.elv` after adding the installed script to it |

For zsh, `workhive completion install` also writes an idempotent Workhive
managed block to `~/.zshrc` so zsh can load the script from Workhive's global
configuration directory.

Completion covers commands, subcommands, flags, value enums such as
`--view auto|table|compact|json`, and object-scoped aliases such as
`workhive workspace ls`. Bash, zsh, and fish also include dynamic candidates
for workspace names and pane targets when Workhive state is available.

## Configuration

Configuration is TOML. Global config sets defaults; project config can override them.

| Path | Purpose |
|------|---------|
| `~/.config/workhive/config.toml` | Global defaults for language, layout, tmux naming, snapshots, logging, tools, and discovery. |
| `~/.config/workhive/.env` | Global environment variables for workhive-managed commands. |
| `~/.config/workhive/logs/` | Global file logs when file logging scope is `global`. |
| `~/.config/workhive/projects.json` | Global project registry. |
| `<project>/.workhive/config.toml` | Optional project configuration created by `workhive init`. |
| `<project>/.workhive/.env` | Project environment variables. |
| `<project>/.workhive/snapshots/` | Workspace layout snapshots when snapshots are enabled. |
| `<project>/.workhive/runtime/control-plane/` | Local Unix socket supervisor state for pane runtime control. |
| `<project>/.workhive/runtime/pane-identity/` | Durable pane launch identity used for titles and pane inspection. |
| `<project>/.workhive/logs/` | Project file logs when file logging scope is `project`. |
| `<project>/.workhive/instance_id` | Project instance identifier used in tmux naming. |
| `<project>/.workhive/worktrees/` | Managed git worktree directories. |

`workhive workspace open --init` creates the runtime `.workhive/` structure needed to open a workspace, but it does not create `config.toml`; project configuration is created by `workhive init`.

### Environment Files (`.env`)

Workhive injects effective environment variables into every pane process spawned
by `workhive workspace open` (and by snapshot restore). Variables flow from
`.env` files through the runtime port into `tmux new-session -e KEY=VALUE` /
`tmux split-window -e KEY=VALUE`, so the launched program inherits them
directly — no shell-string construction.

Syntax (`KEY=VALUE`, never YAML):

```dotenv
# Workhive environment variables.
# Syntax: KEY=VALUE  (one per line; YAML-style "KEY: VALUE" is not accepted)
# Blank lines and lines starting with `#` are ignored.
#
# Global file: ~/.config/workhive/.env
# Project file: <project>/.workhive/.env
# Project entries override global entries with the same key.

# Claude Code fullscreen rendering / reduced flicker.
# See: https://code.claude.com/docs/en/fullscreen#enable-fullscreen-rendering
# CLAUDE_CODE_NO_FLICKER=1
```

Notes:

- New `.env` files are created from this self-documenting template by
  `workhive init` and `workhive config env-set`.
- An invalid line (for example `CLAUDE_CODE_NO_FLICKER: 1`) is **not** silently
  ignored — Workhive emits a structured warning naming the file path, line
  number, and the typed reason, while still loading the rest of the file.
- During `workspace open`, a redacted summary of injected variables is printed
  to stderr: source breakdown, total count, and key names. Raw values are
  **never** logged or printed, even for keys that do not look secret.
- Edit `.env` with any text editor or via `workhive config env-set <key>
  <value>` / `workhive config env-unset <key>`. These commands preserve the
  standard header and never log raw values.

### Snapshots

Snapshots persist a workspace's pane layout under `<project>/.workhive/snapshots/`
so subsequent `workhive workspace open <name>` calls restore the previous shape
even if the tmux session was killed. Snapshots are an opt-in feature.

- The init wizard's "enable workspace snapshots?" prompt now defaults to **No**.
  Existing user configurations are never silently rewritten by upgrades.
- Set `[snapshot] enabled = true` in `~/.config/workhive/config.toml` (or in the
  project's `.workhive/config.toml`) to turn snapshots on; project values
  override global values.

### Trusted Project Directories

Workhive's trusted-roots policy gates where `workhive init`, `workspace open
--init`, worktree provisioning, and runtime state may be created. The first-run
wizard requires an explicit choice:

- The "configure trusted project directories now?" prompt requires an explicit
  `Yes` or `No`.
- Choosing **Yes** opens an interactive path entry loop. Each entry is
  validated:
  - paths must be absolute (or `~`-expanded), not relative;
  - paths must exist and resolve to a directory;
  - duplicate entries are rejected;
  - obviously-overly-broad paths (`/`, `~`, `$HOME`) are rejected unless
    `project roots add --force` is used afterwards.
- Tab completion is available inside the wizard's path field (single match
  fills, multiple matches show candidates, no match shows a hint).
- Empty input is **not** treated as "no restriction" — the wizard insists on
  at least one trusted root before continuing.
- Choosing **No** leaves the trusted-roots set empty; you can add roots later
  with `workhive project roots add <path>`.

### Runtime Control Plane

`workhive workspace open` starts a detached local supervisor for the workspace.
Pane operations such as `workhive pane send` talk to that supervisor through a
typed Unix socket protocol. The supervisor checks for stale sockets with a
ping, self-heals once when needed, and exits with the workspace lifecycle.

This keeps pane messaging, pane status, and future orchestration behind a Rust
domain boundary. tmux remains the current adapter, but product behavior does
not depend on callers constructing raw tmux command strings.

### Pane Identity and Titles

Workhive records launch identity for managed panes under
`<project>/.workhive/runtime/pane-identity/`. That identity is used when showing
pane information and reasserting pane titles around attach, so panes launched as
`codex`, `claude`, or another configured tool do not collapse to generic
foreground names such as `shell`.

### AI Assistant Activity

Workhive observes what Claude Code, Codex, and OpenCode panes are doing and
renders a per-pane activity state. **Workhive does not install provider-side
hooks or plugins.** The automatic preparation pass that runs during
`init --global` and again at AI pane spawn time is a cleanup-and-notify
lifecycle:

- It sweeps any Workhive-owned hook entries, hook scripts, and plugin files
  left behind by earlier releases. User-authored entries at the same paths
  are preserved verbatim, and any rewrite of a user-owned config file is
  preceded by a timestamped backup copy.
- The only remaining write path is the optional, sentinel-owned Codex
  `notify` wrapper (disabled by default; see the feature gates below). The
  wrapper records a completion event through Workhive's internal recorder
  and forwards to any pre-existing user `notify` command, fail-open.

Activity signals instead come from Workhive-side observation: a
terminal-status probe that conservatively parses the visible terminal UI,
the pane supervisor's foreground-process observation, OS process
observation, and a tmux fallback. Hook/plugin events remain the
highest-accuracy source when they are present (for example, user-installed
hooks that invoke Workhive's recorder endpoint).

**Activity telemetry is fail-open.** A missing or failing signal source may
only degrade Workhive's per-pane activity accuracy — it never blocks a
Claude / Codex prompt, tool call, or session.

`workhive pane list` renders the detection source as a dedicated `source`
column (separate from the activity column so the activity column stays
concise on narrow terminals):

| `source` | Meaning |
|----------|---------|
| `hook` | A Claude/Codex hook event reported the activity (high accuracy). |
| `plugin` | An OpenCode plugin event reported the activity (high accuracy). |
| `terminal-status` | Workhive's terminal-status probe parsed a conservative state from the visible terminal UI (medium accuracy). |
| `observer` | The Workhive supervisor saw the AI tool in the foreground, but no higher-accuracy signal has arrived yet (medium accuracy). |
| `process` | OS process observation is the strongest available source (medium accuracy). |
| `tmux` | tmux fallback is the strongest available source (low accuracy). |
| `none` | No usable signal yet (low accuracy). |

When a lower-accuracy source is in play, `pane list` prints a conservative
post-table legend — source ordering by accuracy is
`completion signal > terminal-status > process > tmux > none` — and guidance
on relaunching the AI tool through Workhive so it can observe the session
directly.

Use `workhive pane show` for the full AI activity source, accuracy, stale
status, and guidance. If accuracy stays degraded, `workhive doctor` can
still check the general Workhive environment.

#### AI Integration Lifecycle States

Every Workhive-managed AI provider goes through the same typed lifecycle.
The state is auxiliary information (`pane show` surfaces the user-visible
projection); Workhive never asks you to drive these transitions manually.

| State | Meaning |
|-------|---------|
| `Unsupported` | The provider has no Workhive integration model (no hook/plugin shape we know how to install). |
| `ProviderUnavailable` | The provider binary is not on `PATH` on this machine; nothing to install yet. |
| `NotInstalled` | The provider is available but the hook/plugin has not been prepared yet. |
| `PrepareFailed` | A prior automatic prepare hit a transient or schema-invalid condition; the pane lifecycle stays fail-open and `pane show` carries the actionable reason. |
| `Installed` | Workhive hook/plugin artifacts are installed and current at provider scope. This is a lifecycle model state for provider-level diagnostics or verification; the pane-level `pane list` / `pane show` projection never reports it and progresses directly to `PendingFirstEvent` / `Active` / `Stale`. |
| `ReviewRequired` | The hook/plugin is on disk, but the provider gates it behind a user approval step (Codex `/hooks`). |
| `PendingFirstEvent` | The hook/plugin is installed (and approved if required), but no event has been seen yet for this pane. |
| `Active` | A fresh hook/plugin event has been observed for this pane. |
| `Stale` | The most recent hook/plugin event is older than the freshness window; activity falls back to lower-confidence sources. |

#### Feature Gates

The AI integration lifecycle — the cleanup/notify prepare pass, hook/plugin
activity ingestion, and the post-table guidance block — sits behind a single
feature gate. The gate is **enabled by default**. You can disable it
globally (all providers off) or per provider. Disabling a provider is
**completely silent**:

- no provider config file is touched by the prepare pass;
- no activity event from that provider's hook/plugin is read as a source;
- no `pane list` / `pane show` guidance is rendered for that provider.

A disabled provider is *not* a lifecycle failure — `pane show` simply has
no AI section for that pane, and the activity column falls back to the
observation chain like any other terminal.

Related gates:

- `[features.ai_integrations.providers.codex.notify]` — opt-in Codex
  `notify` completion wrapper. Default: `false`.
- `[features.pane_observation.terminal_status]` — the terminal-status
  probe. Default: `true`.

```toml
[features.ai_integrations]
enabled = true                    # global switch, default = true

[features.ai_integrations.providers.codex]
enabled = false                   # disable one provider

[features.ai_integrations.providers.codex.notify]
enabled = true                    # opt in to the Codex notify wrapper

[features.pane_observation.terminal_status]
enabled = true                    # terminal-status probe, default = true
```

Setting `[features.ai_integrations].enabled = false` disables every
provider; per-provider overrides are then ignored. Project configs inherit
from global and may flip any of these switches independently — there is no
"reset to global" semantic to remember.

### Local Notifications

Workhive can emit local desktop notifications for AI pane transitions (on
macOS via the system notification daemon; when unavailable, notifications
degrade to log lines and never block the CLI). Delivery is conservative:
only high-confidence, fresh activity transitions are considered, every
attempt is deduplicated, and no prompt or message content ever enters a
notification.

The gate lives under `[features.notifications]` with per-category noise
controls:

```toml
[features.notifications]
enabled = true                    # master switch, default = true

[features.notifications.permission_needed]
enabled = true                    # default = true

[features.notifications.failed]
enabled = true                    # default = true

[features.notifications.completed]
enabled = false                   # default = false (low-noise)

[features.notifications.all_panes_complete]
enabled = false                   # default = false
```

### Configuration Example

```toml
language = "en"

[tmux.naming]
server_template = "whsrv-{instance_id}-{project}"
session_template = "wh@{instance_id}@{project}@{workspace}"
max_component_len = 24

[layout]
template = "ai_coding_dual"

[[layout.columns]]
width = 35

[[layout.columns.panes]]
height = 100
launch = { type = "Tool", name = "lazygit" }

[[layout.columns]]
width = 65

[[layout.columns.panes]]
height = 50
launch = { type = "Tool", name = "codex" }

[[layout.columns.panes]]
height = 50
launch = { type = "EmptyShell" }

[snapshot]
# Wizard default is `false`; set to `true` to opt in to layout snapshots.
enabled = false

[logging.console]
enabled = true
level = "warn"
format = "compact"

[logging.file]
enabled = true
level = "debug"
format = "compact"
scope = "global"
base_name = "workhive.log"
rotation = "size_mb(10)"
retention = "max_files(7)"
```

### Logging

Console logs go to stderr in compact human-readable format by default. File logging is configurable through `[logging.file]`.

- `rotation = "size_mb(N)"` rotates the active `.log` file when it reaches about `N` MiB.
- `retention = "max_files(N)"` keeps at most `N` rotated files; use `unlimited` to keep all rotated files.
- `format = "compact"` is intended for humans; `json` is available for machine parsing.
- File logging failure never blocks CLI execution.

### Layout Templates

Built-in templates are `ai_coding_left`, `ai_coding_dual`, `half_split`, `quad`, `single`, and `custom`. Templates expand into explicit `layout.columns` entries. The columns are the source of truth after the config is written.

### Pane Command Presets

The init wizard detects installed tools and offers them as pane launch presets:

| Tool | Purpose |
|------|---------|
| `claude` | Claude Code agent |
| `codex` | Codex agent |
| `opencode` | OpenCode agent |
| `lazygit` | Git TUI |
| `yazi` | File manager |
| `htop` | System monitor |
| `btm` | System monitor |

Only tools found on `$PATH` are shown. You can also leave panes empty or enter custom commands.

## Architecture

```text
workhive-cli
  clap command parsing, miette error display, tracing logging,
  interactive wizard and pane-oriented CLI rendering

workhive-core
  domain models, use cases, error types, i18n keys, and external ports

workhive-infra
  system git and tmux adapters, TOML config adapter, snapshot storage,
  local control-plane supervisor, pane launch identity storage,
  static English and Chinese catalogs, filesystem/project index adapters

workhive-testkit
  in-memory fakes and fixtures for isolated tests
```

### Design Principles

- **Hexagonal boundaries**: core use cases depend on ports, not concrete git, tmux, filesystem, or config implementations.
- **Enum-first Rust modeling**: finite domain variants use enums and exhaustive `match`.
- **Typed errors**: core and infra use `thiserror`; CLI maps errors to user-facing `miette` reports and exit codes.
- **Internationalized CLI**: user-facing messages resolve through i18n keys.
- **Replaceable runtime adapters**: tmux and system git stay behind ports so future adapters can be introduced without rewriting use cases.

### Key Ports

Core ports include `GitPort`, `TmuxPort`, `ConfigPort`, `WorkspaceFsPort`, `ProjectIndexPort`, `ProjectDiscoveryPort`, `SnapshotStorePort`, `SnapshotRuntimePort`, `WorkspaceRuntimePort`, `PaneRuntimePort`, `PaneRegistryPort`, `PaneLaunchIdentityPort`, `DependencyPort`, `I18nPort`, and `EnvironmentPort`.

## Session Naming

Default tmux naming:

- Session: `wh@{instance_id}@{project}@{workspace}`
- Server: `whsrv-{instance_id}-{project}`

Each initialized project gets an `instance_id` stored in `.workhive/instance_id`, which keeps tmux names stable and avoids collisions across projects.

## System Requirements

| Tool | Required | Purpose |
|------|----------|---------|
| git | Yes | Repository detection and worktree management |
| tmux | Yes | Terminal sessions and panes |
| Rust stable (MSRV 1.95.0+) | Build only | Required when building from source |

## License

MIT OR Apache-2.0

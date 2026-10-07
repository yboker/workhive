# workhive

[中文文档](README.zh-CN.md)

**A workspace opener for Git worktrees and terminal runtimes.**

Workhive prepares a project workspace: a Git worktree, a runtime workspace, and
an optional pane layout for tools such as Claude Code, Codex, OpenCode, and
lazygit.
Workhive currently supports two runtime backends:

- **tmux**: Workhive creates and manages a tmux server/session and its panes.
- **Herdr**: Workhive creates or adopts the Git checkout, then registers it as a
  Herdr workspace and uses Herdr's workspace, tab, and pane APIs. Herdr owns the
  native session lifecycle.

## Installation

Prebuilt binaries are available from the
[GitHub Releases page](https://github.com/yboker/workhive/releases).
Workhive runs on macOS and Linux. On Windows, use WSL2 with Linux `git` and the
selected runtime. Native Windows, PowerShell, CMD, and Git for Windows are not
supported runtime environments.

```bash
install -m 755 workhive ~/.local/bin/workhive
workhive --version
```

To build from source, use Rust stable (MSRV 1.95.0+):

```bash
cargo build --release
install -m 755 target/release/workhive ~/.local/bin/workhive
```

## Quick Start

```bash
workhive init --global
cd ~/my-project
workhive init
workhive workspace open feature-auth
```

Useful follow-up commands:

```bash
workhive workspace list
workhive workspace attach feature-auth
workhive workspace close  # tmux current workspace
workhive workspace remove feature-auth
```

To return to a native Herdr session after a restart, run the global resume command
from any directory:

```bash
workhive session resume
workhive session resume workhive-managed
```

Without a name, `session resume` uses the global `herdr.session` setting and falls
back to `workhive-managed`. It delegates session and runtime restoration to Herdr and does
not create project state. Use `workspace open` for project-scoped workspace creation
or explicit rebuilding.

`workspace open` can guide project initialization in an interactive terminal.
For a non-interactive call, use `--init` explicitly.

## Command Surface

Run `workhive --help` or `<command> --help` for the complete, versioned help.
The resource commands are:

| Area | Commands |
|------|----------|
| System | `init`, `config`, `doctor`, `completion` |
| Projects | `project list`, `project reindex`, `project roots ...` |
| Workspaces | `workspace open`, `list`, `attach`, `close`, `remove`, `destroy` |
| Worktrees | `workspace worktree list`, `remove` |
| Sessions | `session list`, `attach`, `resume` |
| Panes | `pane split`, `close`, `list`, `show`, `send` |
| Runtime service | `runtime stop` |

Object-scoped `ls` aliases are available for `workspace`, `project`, `session`,
`pane`, and `workspace worktree`.

### Workspace commands

```text
workspace open [workspace] [--no-attach] [--init] [--force]
                     [--mode manual|auto] [--attach-existing-branch]
workspace list [--view auto|table|compact|json]
workspace attach [workspace]
workspace close [--kill] [--force]
workspace remove <workspace> [--delete-branch|--keep-branch] [--force]
workspace destroy [--delete-branch|--keep-branch] [--force]
workspace worktree list
workspace worktree remove [worktree] [--delete-branch|--keep-branch] [--force]
```

`open` creates or reuses the Git worktree and runtime workspace. `--force`
rebuilds the selected runtime from the effective configuration. `remove` and
`destroy` remove Workhive-managed resources; branch deletion is guarded and can
be disabled with `--keep-branch`.

`workspace close` is the tmux current-workspace detach/kill command. For Herdr,
use `workspace attach`, `session attach`, or `workspace remove` as needed.

### Configuration and inspection

```text
init [--global] [--backend tmux|herdr]
config show|explain|edit|set|unset|env-set|env-unset
doctor [--fix]
project list [--view auto|table|compact|json]
project reindex [--root <path>] [--config-roots] [--clear-first] [--dry-run] [--force]
project roots list|add|remove|edit|status
session list [--view auto|table|compact|json]
session attach [target]
session resume [session]
runtime stop [--force]
```

`project roots` controls the trusted directories in which Workhive may create
project state. `doctor` diagnoses configuration, backend readiness, persisted
bindings, and attach prerequisites. `runtime stop` only stops a backend service
that Workhive can prove it owns.

### Pane commands

```text
pane split [--direction right|down|left|up] [--cmd <command>] [--force]
pane close [--force]
pane list [--view auto|table|compact|json]
pane show [pane]
pane send <pane> <text> [--no-enter] [--force]
```

Pane operations are available through both backends, but their mechanics differ.
With tmux, pane targets may be a pane number or tmux id such as `%2`; shell/TUI
panes require `--force` when targeted for raw input. With Herdr, targets are Herdr
pane identities and Workhive calls Herdr's pane APIs. A Herdr attach focuses the
persisted workspace and uses Herdr's native session attach path.

`pane send` sends text to a selected pane. Verified AI panes receive the text
through the backend runtime; shell, TUI, and unverified panes require
`--force`.

## Runtime Backends

### tmux

With `backend = "tmux"`, Workhive:

- creates or reuses a shared tmux server and a uniquely named session;
- creates panes from the configured layout and launches their commands;
- supports tmux attach, detach, pane split/close/send, titles, mouse mode, and
  OSC 52 clipboard configuration;
- observes pane foreground processes and terminal state, with optional activity
  integrations and local notifications.

The default session name is:

```text
wh@{instance_id}@{project}@{workspace}
```

The shared tmux server is named `default` by default. `workspace open`,
`workspace attach`, and `session attach` can be invoked from another tmux client.

### Herdr

With `backend = "herdr"`, Workhive:

- creates or adopts the Git checkout using Workhive's Git/worktree policy;
- registers an existing checkout with Herdr using `worktree.open`;
- applies the configured layout through Herdr;
- lists, focuses, splits, closes, renames, and sends input to Herdr panes;
- shows Herdr's native session hierarchy through `session list` and delegates
  whole-session attach to Herdr.

The global default Herdr session is `workhive-managed`; project `herdr.session` overrides
it. Existing persisted workspace bindings remain authoritative until the selected
workspace is explicitly rebuilt with `--force`.

## Configuration

Configuration is TOML. The backend setting accepts `tmux` or `herdr`, and
project configuration overrides global configuration.

```bash
workhive init --global --backend tmux
workhive config set backend herdr --global
workhive config set herdr.session my-session
workhive config set tmux.mouse false --global
workhive config set tmux.clipboard false
```

Important locations:

| Path | Purpose |
|------|---------|
| `~/.config/workhive/config.toml` | Global configuration |
| `~/.config/workhive/.env` | Global environment variables |
| `~/.config/workhive/projects.json` | Project index |
| `<project>/.workhive/config.toml` | Project configuration |
| `<project>/.workhive/.env` | Project environment variables |
| `<project>/.workhive/instance_id` | Stable project identity used by tmux naming |
| `<project>/.workhive/worktrees/` | Workhive-managed Git worktrees |
| `<project>/.workhive/runtime/` | Runtime control and pane identity state |

`workspace open` injects the effective `.env` variables into programs it
launches. The syntax is `KEY=VALUE`; invalid lines produce warnings and raw
values are not printed in the injection summary.

The init wizard can configure the layout and pane launch commands. Built-in
presets include `claude`, `codex`, `opencode`, `lazygit`, `yazi`, `htop`, and
`btm` when those programs are available on `PATH`. These are launch presets for
commands found on `PATH`.

Optional configuration groups include:

- `[features.ai_integrations]` for provider-specific activity signals and the
  opt-in Codex `notify` wrapper;
- `[features.pane_observation.terminal_status]` for terminal-state observation;
- `[features.notifications]` for local notifications.

These features provide pane activity information and local notifications.

## Shell Completion

```bash
workhive completion install
workhive completion install bash
workhive completion install zsh
workhive completion install fish
workhive completion script bash
```

Supported script types are `bash`, `zsh`, `fish`, `powershell`, and `elvish`.

## Safety and Non-interactive Use

Workhive requires a trusted project root before creating project state. In
non-interactive scripts, add it first with:

```bash
workhive project roots add /path/to/parent
workhive init
workhive workspace open feature-auth --no-attach --init
```

Interactive selectors and confirmations require a TTY. Use explicit workspace
names and `--force`/`-f` where appropriate in automation. Machine-readable
views are available with `--view json`.

Workhive uses typed errors and stable exit categories for successful execution,
invalid arguments, missing resources, runtime failures, and user interruption.

## Requirements

| Tool | Required when | Purpose |
|------|---------------|---------|
| Git | Always | Repository and worktree management |
| tmux | `backend = "tmux"` | Sessions and panes |
| Herdr server | `backend = "herdr"` | Herdr workspaces, tabs, and panes |
| Rust stable 1.95.0+ | Building from source | Build toolchain |

## License

MIT

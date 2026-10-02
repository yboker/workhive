//! CLI command definitions using clap derive.
//!
//! Workhive exposes its CLI as a small set of system commands plus resource
//! objects: `project`, `workspace`, `session`, `runtime`, and `pane`. Each
//! resource has its own action subcommand. Internal/runtime-only commands
//! live under the hidden `internal` group.

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

/// workhive — terminal workspace manager for AI coding and remote development.
///
/// Manages projects, workspaces, sessions, and panes from a single
/// command-line interface.
#[derive(Parser, Debug)]
#[command(
    name = "workhive",
    version,
    about = "Terminal workspace manager for AI coding and remote development",
    long_about = None,
    help_template = "\
{about}

Usage: {usage}

System Commands:
  init         Initialize workhive configuration
  config       View or modify configuration
  doctor       Run diagnostic checks on the workhive environment
  completion   Install shell completions or print completion scripts
  update       Update workhive to the latest version

Resource Commands:
  project      Manage registered workhive projects
  workspace    Manage workspaces in the current project
  session      Manage active workspace sessions
  runtime      Manage backend runtime services
  pane         Manage panes in the current workspace

Options:
{options}
",
    propagate_version = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Enable verbose logging output.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

/// Top-level commands.
///
/// System commands (`init`, `config`, `doctor`) live at the top level.
/// All resource-scoped operations are grouped under `project`, `workspace`,
/// `session`, and `pane`. Runtime-only commands are hidden under `internal`.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize workhive configuration.
    Init {
        /// Initialize global configuration instead of project configuration.
        #[arg(long)]
        global: bool,

        /// Explicit runtime backend. Global non-interactive init requires it.
        #[arg(long, value_parser = ["tmux", "herdr"])]
        backend: Option<String>,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// View or modify configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Run diagnostic checks on the workhive environment.
    ///
    /// Checks configured runtime reachability, persisted binding consistency,
    /// workspace lookup, PTY availability, and attach readiness. Also detects identity drift
    /// (stored project_root differs from current directory). With --fix,
    /// interactively repairs detected issues.
    Doctor {
        /// Automatically fix detected issues (interactive confirmation required for non-trivial fixes).
        #[arg(long)]
        fix: bool,
    },

    /// Install shell completions or print completion scripts.
    ///
    /// `workhive completion install` installs completions for the detected
    /// shell. Pass a shell name to override detection.
    ///
    /// Supported shells: `bash`, `zsh`, `fish`, `powershell`, `elvish`.
    Completion {
        #[command(subcommand)]
        action: CompletionAction,
    },

    /// Update workhive to the latest version.
    ///
    /// Downloads and installs the latest release from GitHub.
    /// Supports macOS and Linux (including WSL).
    ///
    /// For private repositories, set GITHUB_TOKEN or WORKHIVE_GITHUB_TOKEN:
    ///   export GITHUB_TOKEN=your_token
    Update {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,

        /// Dry run - show what would be done without making changes.
        #[arg(long)]
        dry_run: bool,

        /// Allow pre-release versions.
        #[arg(long)]
        prerelease: bool,

        /// Install a specific version (e.g., "2.1.0").
        #[arg(long, value_name = "VERSION")]
        target_version: Option<String>,
    },

    /// Manage registered workhive projects.
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Manage workspaces in the current project.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },

    /// Manage active workspace sessions.
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

    /// Manage backend runtime services.
    Runtime {
        #[command(subcommand)]
        action: RuntimeAction,
    },

    /// Manage panes in the current workspace.
    Pane {
        #[command(subcommand)]
        action: PaneAction,
    },

    /// Internal runtime commands (hooks, supervisors).
    ///
    /// These commands are invoked by Workhive itself (e.g. tmux hooks). They
    /// are not part of the user-facing CLI surface and may change without
    /// notice.
    #[command(hide = true)]
    Internal {
        #[command(subcommand)]
        action: InternalAction,
    },
}

/// Shell completion actions.
#[derive(Subcommand, Debug)]
pub enum CompletionAction {
    /// Install shell completions for the current user.
    Install {
        /// Target shell. Defaults to the current shell when omitted.
        #[arg(value_enum)]
        shell: Option<Shell>,
    },

    /// Print a shell completion script to stdout.
    Script {
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Render mode for `workspace list` and `project list`.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq, Default)]
pub enum ListView {
    /// Select best view from terminal width.
    #[default]
    Auto,
    /// Force table view.
    Table,
    /// Force compact list view.
    Compact,
    /// Emit machine-readable JSON.
    Json,
}

/// Project subcommands.
#[derive(Subcommand, Debug)]
pub enum ProjectAction {
    /// List all registered Workhive projects from the global index.
    ///
    /// Alias: `ls` (object-scoped only).
    #[command(alias = "ls")]
    List {
        /// Output view mode.
        #[arg(long, value_enum, default_value_t = ListView::Auto)]
        view: ListView,
    },

    /// Rebuild the global project index by scanning root directories.
    ///
    /// When neither `--root` nor `--config-roots` is given, the configured
    /// trusted project roots (from `[projects].trusted_roots`, with the legacy
    /// `[discovery].project_roots` as a fallback alias) are used by default.
    Reindex {
        /// Root directory to scan (can be specified multiple times).
        #[arg(long)]
        root: Vec<String>,

        /// Force the use of trusted roots from global config even when `--root` is also given.
        #[arg(long)]
        config_roots: bool,

        /// Clear old index before rebuilding.
        #[arg(long)]
        clear_first: bool,

        /// Only show what would be found, don't write index.
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Manage trusted project root directories.
    ///
    /// Trusted project roots gate where Workhive may create `.workhive/`,
    /// worktrees, and runtime state, and they feed `project reindex` and the
    /// global `project list` / `session list` index.
    Roots {
        #[command(subcommand)]
        action: ProjectRootsAction,
    },
}

/// `project roots` subcommands.
#[derive(Subcommand, Debug)]
pub enum ProjectRootsAction {
    /// Show the configured trusted project roots.
    #[command(alias = "ls")]
    List,

    /// Add a directory to the trusted-root list.
    Add {
        /// Path to add (absolute, or starting with `~/`).
        path: String,

        /// Skip the safety check that rejects overly broad paths (e.g. `/`, `$HOME`).
        #[arg(long)]
        force: bool,
    },

    /// Remove a directory from the trusted-root list.
    Remove {
        /// Path to remove.
        path: String,

        /// Remove without confirming, even if indexed projects live under the path.
        ///
        /// Required in non-interactive mode when affected projects exist.
        /// Removing a trusted root never deletes any project or `.workhive/`
        /// state — it only narrows the safety policy.
        #[arg(long)]
        force: bool,
    },

    /// Interactively edit the trusted project roots.
    ///
    /// Lets you view current roots, add new ones, remove existing ones, and
    /// commit the change in a single session. Modifications are persisted
    /// via `toml_edit`, preserving comments and formatting in
    /// `~/.config/workhive/config.toml`.
    Edit,

    /// Show the trusted roots and the current project-index status.
    Status,
}

/// Workspace subcommands.
#[derive(Subcommand, Debug)]
pub enum WorkspaceAction {
    /// Open a workspace.
    ///
    /// If no workspace name is given, an interactive selector lists the
    /// workspaces available in the current project. For uninitialized
    /// projects, use `--init` in non-interactive mode or confirm
    /// interactively when prompted.
    Open {
        /// Workspace name (optional — shows selector if omitted).
        workspace: Option<String>,

        /// Open the workspace without attaching to it.
        #[arg(long)]
        no_attach: bool,

        /// Force-recreate the workspace from the configured layout.
        ///
        /// Replaces any existing workspace runtime state before opening a
        /// fresh workspace from config.
        #[arg(short = 'f', long = "force")]
        force: bool,

        /// Workspace interaction mode.
        ///
        /// `manual` (default): panes operate independently.
        /// `primary-assisted`: enables primary-pane orchestration for
        /// coordinating work across worker panes.
        #[arg(long, default_value = "manual")]
        mode: String,

        /// Explicitly opt-in to auto-initializing an uninitialized project.
        ///
        /// Required in non-interactive mode. In interactive mode, the user
        /// is prompted for confirmation instead.
        #[arg(long)]
        init: bool,

        /// Attach a pre-existing local branch that has no Workhive-managed
        /// worktree.
        ///
        /// Without this flag, opening a workspace whose name collides with
        /// an existing branch prompts for confirmation in an interactive
        /// terminal (default No) and fails in non-interactive mode. The
        /// flag records the branch as user-attached so removal keeps it by
        /// default.
        #[arg(long)]
        attach_existing_branch: bool,
    },

    /// List workspaces in the current project.
    ///
    /// Alias: `ls`. Only object-scoped `ls` is supported; `workhive ls` is
    /// rejected at the top level.
    #[command(alias = "ls")]
    List {
        /// Output view mode.
        #[arg(long, value_enum, default_value_t = ListView::Auto)]
        view: ListView,
    },

    /// Attach to a workspace.
    ///
    /// If no workspace name is given, an interactive selector lists the
    /// active workspaces in the current project for you to pick from.
    Attach {
        /// Workspace name (optional — shows selector if omitted).
        workspace: Option<String>,
    },

    /// Close the current workspace.
    ///
    /// Must be run from inside a Workhive-managed pane. Only the workspace
    /// that owns the current pane is closed; other workspaces in the
    /// project are not affected.
    Close {
        /// Terminate the workspace runtime instead of just detaching.
        #[arg(long)]
        kill: bool,

        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Remove one workspace from the current project.
    ///
    /// Removes only the named workspace; other workspaces and
    /// project-level state are not affected.
    Remove {
        /// Workspace selector (workspace name or backing branch name).
        workspace: String,
        /// Authorize deleting a user-owned backing branch (pre-existing or
        /// of unknown provenance). Workhive-created branches are deleted by
        /// default; this flag is only needed for branches Workhive did not
        /// create.
        #[arg(long)]
        delete_branch: bool,
        /// Keep the backing branch even when Workhive created it.
        ///
        /// Takes precedence over `--delete-branch`.
        #[arg(long)]
        keep_branch: bool,
        /// Skip confirmation prompts.
        #[arg(short, long)]
        force: bool,
    },

    /// Destroy all Workhive resources in the current project.
    ///
    /// Project-wide destructive operation: removes every workspace, every
    /// Workhive runtime session and the project's `.workhive/` directory.
    /// Other projects are not affected.
    Destroy {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
        /// Authorize deleting user-owned backing branches (pre-existing or
        /// of unknown provenance) for the destroyed worktrees. Workhive-
        /// created branches are deleted by default.
        #[arg(long)]
        delete_branch: bool,
        /// Keep all backing branches, including Workhive-created ones.
        ///
        /// Takes precedence over `--delete-branch`.
        #[arg(long)]
        keep_branch: bool,
    },

    /// Manage git worktrees backing workspaces in the current project.
    ///
    /// Canonical command path for worktree management.
    Worktree {
        #[command(subcommand)]
        action: Option<WorktreeAction>,
    },
}

/// Session subcommands.
///
/// `session attach` is global: it does not require a project context,
/// does not create `.workhive/`, does not materialize worktrees, and does
/// not write the project index. It only attaches to an already-existing
/// runtime/tmux session that Workhive can resolve from the global session
/// listing. Project-scoped attach lives under `workspace attach`.
#[derive(Subcommand, Debug)]
pub enum SessionAction {
    /// List active workspace sessions across all registered projects.
    ///
    /// Alias: `ls` (object-scoped only).
    #[command(alias = "ls")]
    List {
        /// Output view mode.
        #[arg(long, value_enum, default_value_t = ListView::Auto)]
        view: ListView,
    },

    /// Attach to a workhive-managed session globally.
    ///
    /// No `target`: interactive selection across all known sessions.
    /// Target may be a complete session name, `project/workspace`, or a
    /// workspace name (only when unambiguous across projects).
    Attach {
        /// Optional target: <session> | <project/workspace> | <workspace>.
        target: Option<String>,
    },
}

/// Runtime service lifecycle commands.
#[derive(Subcommand, Debug)]
pub enum RuntimeAction {
    /// Stop a Workhive-managed backend service.
    Stop {
        /// Stop even if the managed server still has workspaces, panes, or agents.
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
}

/// Worktree subcommands.
#[derive(Subcommand, Debug)]
pub enum WorktreeAction {
    /// List Workhive-managed linked worktrees (alias: `ls`).
    ///
    /// Only worktrees under `<project>/.workhive/worktrees/` are listed.
    /// External git worktrees and the main project root are excluded by
    /// design.
    #[command(alias = "ls")]
    List,

    /// Remove a Workhive-managed linked worktree.
    ///
    /// Removable candidates are restricted to worktrees under
    /// `<project>/.workhive/worktrees/`; the main project root and any
    /// external linked worktrees are never offered. Without a target
    /// argument an interactive selector opens in TTY; in non-interactive
    /// mode the worktree name is required.
    Remove {
        /// Worktree name or branch. Optional in interactive terminals;
        /// required when stdin is not a TTY.
        worktree: Option<String>,

        /// Skip the dirty/unmerged safety block. Without `--force` a
        /// risky worktree is refused unless the user confirms twice.
        #[arg(long)]
        force: bool,

        /// Authorize deleting a user-owned backing branch (pre-existing or
        /// of unknown provenance). Workhive-created branches are deleted by
        /// default; this flag is only needed for branches Workhive did not
        /// create. Matches `workspace remove`/`destroy` semantics.
        #[arg(long)]
        delete_branch: bool,

        /// Keep the backing branch even when Workhive created it.
        ///
        /// Takes precedence over `--delete-branch`.
        #[arg(long)]
        keep_branch: bool,
    },
}

/// Config subcommands.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Display current configuration.
    Show {
        /// Show global configuration instead of project configuration.
        #[arg(long)]
        global: bool,

        /// Output raw TOML instead of the visual overview.
        #[arg(long)]
        raw: bool,
    },

    /// Explain valid values and semantics for a config key.
    Explain {
        /// Configuration key to explain.
        key: String,
    },

    /// Interactively edit configuration (preview then wizard).
    Edit {
        /// Edit global configuration.
        #[arg(long)]
        global: bool,
    },

    /// Set a configuration value.
    Set {
        /// Configuration key (dot-separated, e.g., `tmux.server_name` or `herdr.session`).
        key: String,
        /// New value.
        value: String,
        /// Modify global configuration.
        #[arg(long)]
        global: bool,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Remove a configuration value.
    Unset {
        /// Configuration key to remove.
        key: String,
        /// Modify global configuration.
        #[arg(long)]
        global: bool,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Set an environment variable in `.env`.
    EnvSet {
        /// Variable name.
        key: String,
        /// Variable value.
        value: String,
        /// Modify global `.env`.
        #[arg(long)]
        global: bool,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Remove an environment variable from `.env`.
    EnvUnset {
        /// Variable name to remove.
        key: String,
        /// Modify global `.env`.
        #[arg(long)]
        global: bool,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },
}

impl ConfigAction {
    /// Whether this action targets the global config.
    pub fn is_global(&self) -> bool {
        match self {
            Self::Show { global, .. }
            | Self::Edit { global }
            | Self::Set { global, .. }
            | Self::Unset { global, .. }
            | Self::EnvSet { global, .. }
            | Self::EnvUnset { global, .. } => *global,
            Self::Explain { .. } => false,
        }
    }
}

/// Typed direction for `pane split`, mirroring `workhive_core::domain::PaneDirection`.
///
/// A dedicated `ValueEnum` keeps clap validating invalid values at the CLI
/// boundary with a localized-ready error, instead of accepting any string and
/// silently falling back to `right`.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
#[value(rename_all = "lower")]
pub enum PaneSplitDirectionArg {
    Right,
    Down,
    Left,
    Up,
}

impl From<PaneSplitDirectionArg> for workhive_core::domain::PaneDirection {
    fn from(value: PaneSplitDirectionArg) -> Self {
        match value {
            PaneSplitDirectionArg::Right => workhive_core::domain::PaneDirection::Right,
            PaneSplitDirectionArg::Down => workhive_core::domain::PaneDirection::Down,
            PaneSplitDirectionArg::Left => workhive_core::domain::PaneDirection::Left,
            PaneSplitDirectionArg::Up => workhive_core::domain::PaneDirection::Up,
        }
    }
}

/// Pane subcommands.
#[derive(Subcommand, Debug)]
pub enum PaneAction {
    /// Split the current pane.
    Split {
        /// Direction of the split. Default is `down` so multi-pane layouts
        /// stack vertically, matching the stacked workflow most users expect.
        #[arg(long, value_enum, default_value_t = PaneSplitDirectionArg::Down)]
        direction: PaneSplitDirectionArg,
        /// Command to run in the new pane.
        #[arg(long)]
        cmd: Option<String>,
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// Close the current pane.
    Close {
        /// Skip confirmation prompt.
        #[arg(short, long)]
        force: bool,
    },

    /// List all panes in the current session.
    ///
    /// Alias: `ls` (object-scoped only).
    #[command(alias = "ls")]
    List {
        /// Output view mode. `--view json` emits a machine-readable
        /// `{"panes": [...]}` document on stdout for scripted callers.
        #[arg(long, value_enum, default_value_t = ListView::Auto)]
        view: ListView,
    },

    /// Show information about a pane.
    ///
    /// With no argument, show the current pane. With a target argument,
    /// show the pane identified by its `pane list` number (e.g. `2`) or
    /// its tmux target id (e.g. `%2`).
    #[command(alias = "current")]
    Show {
        /// Optional target pane: a pane number from `workhive pane list`
        /// (e.g. `2`) or a tmux target id (e.g. `%2`). Omit to show the
        /// current pane.
        pane: Option<String>,
    },

    // CLI-help boundary: every `///` doc-comment on this variant is
    // rendered verbatim by clap as the EN `about` for
    // `workhive pane approve-hooks --help`. The alias is a hidden
    // compatibility entry only; it performs no provider input.
    //
    // The EN i18n string at key `cli.help.cmd.pane.approve_hooks`
    // is intentionally byte-equivalent to the first line of this
    // doc-comment, so the EN render is identical whether clap
    // uses the derive default (HOME not pinned, no zh-CN override)
    // or the localizer rewrite path. The zh-CN translation lives
    // alongside it.
    /// Deprecated no-op alias kept for script compatibility. Workhive no longer installs provider-side hooks, so no approval is required and this command performs no action.
    #[command(hide = true)]
    ApproveHooks {
        /// Target pane (id or role). The argument is accepted for backward compatibility and is otherwise ignored.
        pane: String,
    },

    /// Send text to a target pane.
    ///
    /// Default semantics: a verified AI message receiver pane gets the
    /// literal text plus a structured submit event. Shell, TUI, and
    /// unverified panes are refused by default — chat-shaped text is
    /// never silently treated as a shell command or raw keystroke
    /// injection. Pass `--no-enter` to deliver text only (insert without
    /// submit). Pass `--force` to opt into the explicit raw-keystroke
    /// override.
    Send {
        /// Target pane identifier.
        pane: String,
        /// Text to send.
        text: String,
        /// Insert text without the structured submit event (no Enter
        /// submitted on safe receivers). The flag does NOT relax the
        /// shell/TUI/unknown-target refusal: those panes always need
        /// `--force`.
        #[arg(long)]
        no_enter: bool,
        /// Skip confirmation prompt AND opt into the explicit raw-
        /// keystroke override for shell, TUI, or unverified panes.
        /// With `--force`, text is typed into the target as raw
        /// keyboard input — treat it as an advanced operator action
        /// that may disrupt shell or TUI state.
        #[arg(short, long)]
        force: bool,
    },
}

/// Supported AI-assistant integration provider names parsed by clap.
///
/// Used exclusively by the hidden `internal activity hook --provider <X>`
/// command that the installed hook scripts invoke. The user-facing
/// `workhive integrations` surface does not exist; preparation happens
/// automatically inside the pane supervisor and `init --global`, keyed off the typed
/// [`workhive_core::domain::AiActivityProvider`].
///
/// `none` is intentionally excluded — it is the fallback used by the
/// merge policy when no integration is installed.
#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "lower")]
pub enum IntegrationProviderArg {
    /// Claude Code (Anthropic). Source:
    /// <https://code.claude.com/docs/en/hooks>.
    Claude,
    /// Codex (OpenAI). Source:
    /// <https://developers.openai.com/codex/hooks>.
    Codex,
    /// OpenCode. Source:
    /// <https://opencode.ai/docs/plugins/>.
    Opencode,
}

impl IntegrationProviderArg {
    /// Map the CLI surface to the core provider enum.
    pub fn as_core(self) -> workhive_core::domain::AiActivityProvider {
        match self {
            Self::Claude => workhive_core::domain::AiActivityProvider::ClaudeCode,
            Self::Codex => workhive_core::domain::AiActivityProvider::Codex,
            Self::Opencode => workhive_core::domain::AiActivityProvider::Opencode,
        }
    }
}

mod internal;

pub use internal::*;

#[cfg(test)]
#[path = "../cli_tests.rs"]
mod tests;

//! Locale-aware rewriting of the [`clap::Command`] tree and CLI errors.
//!
//! `clap` emits help/usage/parse errors at parse time, before any
//! [`crate::context::CommandContext`] (and therefore the configured locale)
//! is constructed by handlers. To deliver Chinese help/usage/error output
//! when the user has set `language = "zh-CN"` globally, we:
//!
//! 1. Resolve the user's preferred locale early in `main` (via a dedicated
//!    [`peek_locale`] helper that re-uses the same [`workhive_infra::config`]
//!    adapter the rest of the binary depends on).
//! 2. Rewrite the static `clap::Command` tree produced by the derive macros
//!    so that command/subcommand `about` strings, the top-level
//!    `help_template`, and per-argument help descriptions are taken from
//!    the configured i18n catalog.
//! 3. Render parse errors through Workhive's own localized formatter so
//!    `error:` / `Usage:` / etc. appear in the configured locale instead
//!    of clap's hardcoded English block. Exit code 2 is preserved.
//! 4. Localize visible [`crate::error::CliError`] rendering to the same
//!    locale after dispatch.
//!
//! Command identifiers and flag names are deliberately *not* localized
//! (governance §12.2: "命令名和 flag 不翻译"). What this module touches:
//!
//! - top-level `about` and `help_template`
//! - top-level subcommand `about` (init, config, doctor, completion,
//!   project, workspace, session, pane)
//! - object-scoped subcommand `about` (workspace open/list/attach/close/...)
//! - per-argument `help` text for public commands (workspace/pane/etc.)
//! - parse-error rendering, including unknown subcommand/option, missing
//!   required argument, invalid value, and the catch-all template
//! - [`crate::error::CliError`] rendering for runtime errors

use clap::Command;
use clap::error::{ContextKind, ContextValue, Error as ClapError, ErrorKind};
use workhive_core::domain::Locale;
use workhive_core::i18n_keys as keys;
use workhive_core::ports::I18nPort;

/// Resolve the user's preferred locale by reading the global config file.
///
/// This is intentionally a lightweight side-channel read: it must run
/// *before* `clap` parses arguments so we can hand a localized
/// [`Command`] tree to clap. Any error (missing config, IO error,
/// validation failure) falls back to [`Locale::En`] silently — never
/// blocks startup.
pub(crate) fn peek_locale() -> Locale {
    let adapter = workhive_infra::config::TomlConfigAdapter::new();
    use workhive_core::ports::ConfigPort;
    match adapter.load_global() {
        Ok(Some(cfg)) => cfg.language.unwrap_or(Locale::En),
        _ => Locale::En,
    }
}

/// Rewrite a `clap::Command` tree to render in the requested locale.
///
/// English locale is a no-op (the derive-macro defaults are already
/// English). For Chinese, this swaps `about` strings on every command
/// node we know about and replaces the top-level `help_template`.
pub(crate) fn localize_command(cmd: Command, locale: Locale, i18n: &dyn I18nPort) -> Command {
    if locale != Locale::ZhCn {
        return cmd;
    }
    let cmd = rewrite_root(cmd, i18n);
    let cmd = localize_nested_help_chrome_recursive(cmd, i18n);
    localize_builtin_flags_recursive(cmd, i18n)
}

/// Set a command's localized `about` text AND clear its `long_about`
/// so the doc-comment-derived English long form does not override the
/// zh-CN short form when the user runs `--help` on the subcommand.
///
/// clap's `--help` for a subcommand renders `long_about` if set,
/// otherwise `about`. Doc comments populate both, so calling `about(zh)`
/// alone leaves the English long form in place. By clearing
/// `long_about` we let clap fall back to our localized `about`,
/// guaranteeing a clean Chinese description without translating every
/// long-form paragraph.
fn set_about_zh(cmd: Command, i18n: &dyn I18nPort, key: &str) -> Command {
    cmd.about(i18n.resolve(Locale::ZhCn, key))
        .long_about(clap::builder::Resettable::<clap::builder::StyledStr>::Reset)
}

/// Recursively apply zh-CN help rendering to every visible subcommand node.
///
/// The root command uses a bespoke help template that splits top-level commands
/// into "system" and "resource" groups. Nested commands should use clap's
/// normal structural rendering, but with localized headings and templates.
fn localize_nested_help_chrome_recursive(mut cmd: Command, i18n: &dyn I18nPort) -> Command {
    // Walk all subcommands, including hidden compatibility aliases such as
    // `pane approve-hooks`. Hidden only suppresses listing in the parent's
    // `Commands:` block; users can still invoke `<alias> --help` directly
    // and that help must render with localized clap chrome
    // (Usage/Commands/Options) when running zh-CN.
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|sc| sc.get_name().to_owned())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(name, |sub| {
            let sub = localize_help_chrome(sub, i18n);
            localize_nested_help_chrome_recursive(sub, i18n)
        });
    }
    cmd = localize_auto_help_subcommand_recursive(cmd, i18n);
    cmd
}

/// Suppress clap's auto-generated `help` subcommand on the root and
/// every nested node when running zh-CN.
///
/// Reason: clap's auto-help line "Print this message or the help of the
/// given subcommand(s)" is hardcoded in English and is materialized
/// only during clap's internal `build()` pass — too late for
/// `mut_subcommand("help", …)` rewrites without tripping clap's
/// "Fatal internal error" invariant in `arg.rs`. Disabling the auto
/// subcommand is the supported public-API path.
///
/// This is locale-only: English users keep `workhive help workspace`,
/// zh-CN users still have the always-available `--help` / `-h` on every
/// command, and the about text already documents that. No behavior
/// change for any other locale.
fn localize_auto_help_subcommand_recursive(mut cmd: Command, _i18n: &dyn I18nPort) -> Command {
    if !cmd.is_disable_help_subcommand_set() {
        cmd = cmd.disable_help_subcommand(true);
    }
    cmd
}

fn localize_help_chrome(mut cmd: Command, i18n: &dyn I18nPort) -> Command {
    // Pick a shape-specific nested help template based on the structure
    // of THIS command node, so leaf commands stop rendering empty
    // `命令:` / `参数:` sections in their `--help` output.
    //
    // This stays a generic CLI-localization fix: the decision is driven
    // by `clap` introspection (`get_subcommands` / `get_arguments`) and
    // never special-cases a particular subcommand name. clap remains the
    // single source of truth for command structure — we only swap which
    // localized template string is fed back to it.
    //
    // Visibility matches what `clap` would actually render: hidden
    // subcommands and hidden arguments do not contribute to the
    // `{subcommands}` / `{positionals}` blocks, so we ignore them when
    // deciding whether the heading would have any content.
    let has_visible_subcommand = cmd.get_subcommands().any(|sc| !sc.is_hide_set());
    let has_visible_positional = cmd
        .get_arguments()
        .any(|arg| arg.get_short().is_none() && arg.get_long().is_none() && !arg.is_hide_set());

    let template_key = match (has_visible_subcommand, has_visible_positional) {
        (true, true) => keys::CLI_HELP_NESTED_TEMPLATE,
        (true, false) => keys::CLI_HELP_NESTED_TEMPLATE_SUBCOMMANDS_ONLY,
        (false, true) => keys::CLI_HELP_NESTED_TEMPLATE_POSITIONALS_ONLY,
        (false, false) => keys::CLI_HELP_NESTED_TEMPLATE_OPTIONS_ONLY,
    };

    cmd = cmd
        .help_template(i18n.resolve(Locale::ZhCn, template_key))
        .subcommand_help_heading(i18n.resolve(Locale::ZhCn, keys::CLI_HELP_COMMANDS_HEADING))
        .next_help_heading(i18n.resolve(Locale::ZhCn, keys::CLI_HELP_OPTIONS_HEADING));

    let arg_ids: Vec<(String, bool, bool)> = cmd
        .get_arguments()
        .map(|arg| {
            let id = arg.get_id().to_string();
            let is_positional = arg.get_short().is_none() && arg.get_long().is_none();
            let has_possible_values = matches!(id.as_str(), "view" | "shell");
            (id, is_positional, has_possible_values)
        })
        .collect();
    for (id, is_positional, has_possible_values) in arg_ids {
        let heading = if is_positional {
            i18n.resolve(Locale::ZhCn, keys::CLI_HELP_ARGUMENTS_HEADING)
        } else {
            i18n.resolve(Locale::ZhCn, keys::CLI_HELP_OPTIONS_HEADING)
        };
        cmd = cmd.mut_arg(id, |arg| {
            let arg = arg.help_heading(heading);
            if has_possible_values {
                arg.hide_possible_values(true)
            } else {
                arg
            }
        });
    }
    cmd
}

/// Recursively localize the auto-generated `--help` and `--version` flag
/// descriptions for every command in the tree.
///
/// clap derive does not materialize the auto-generated `help`/`version`
/// args on the Command tree until `Command::build()` runs (right before
/// argument parsing), so `mut_arg` and `get_arguments()` cannot see
/// them at this stage. We work around this by disabling the auto flag
/// on every applicable command and injecting an equivalent localized
/// argument with [`clap::ArgAction::HelpLong`] / [`clap::ArgAction::Version`].
/// The behavior (`-h`, `--help`, `-V`, `--version`) is unchanged from
/// the derive default; only the description shown in `--help` output is
/// localized.
fn localize_builtin_flags_recursive(mut cmd: Command, i18n: &dyn I18nPort) -> Command {
    use clap::{Arg, ArgAction};
    if !cmd.is_disable_help_flag_set() {
        let help_arg = Arg::new("help")
            .short('h')
            .long("help")
            .action(ArgAction::Help)
            .help(i18n.resolve(Locale::ZhCn, keys::CLI_ARG_HELP));
        cmd = cmd.disable_help_flag(true).arg(help_arg);
    }
    if !cmd.is_disable_version_flag_set() && cmd.get_version().is_some() {
        let version_arg = Arg::new("version")
            .short('V')
            .long("version")
            .action(ArgAction::Version)
            .help(i18n.resolve(Locale::ZhCn, keys::CLI_ARG_VERSION));
        cmd = cmd.disable_version_flag(true).arg(version_arg);
    }
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|sc| sc.get_name().to_owned())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(name, |sub| localize_builtin_flags_recursive(sub, i18n));
    }
    cmd
}

fn rewrite_root(mut cmd: Command, i18n: &dyn I18nPort) -> Command {
    // Top-level about + help template. The auto-generated `help`
    // subcommand is materialized by clap during `build()` and is not
    // present on the bare `Cli::command()` tree, so we deliberately do
    // not call `mut_subcommand("help", …)` here — its short description
    // is documented inside our zh-CN `help_template` block instead.
    cmd = set_about_zh(cmd, i18n, keys::CLI_ABOUT)
        .help_template(i18n.resolve(Locale::ZhCn, keys::CLI_HELP_TEMPLATE));

    // Top-level argument: --verbose.
    cmd = set_arg_help(cmd, "verbose", i18n, keys::CLI_ARG_VERBOSE);

    // Top-level public subcommands.
    cmd = cmd
        .mut_subcommand("init", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_INIT);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_INIT_GLOBAL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("config", |c| rewrite_config(c, i18n))
        .mut_subcommand("doctor", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_DOCTOR)
        })
        .mut_subcommand("completion", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_COMPLETION);
            c.mut_subcommand("install", |c| {
                let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_COMPLETION_INSTALL);
                set_arg_help(c, "shell", i18n, keys::CLI_ARG_COMPLETION_INSTALL_SHELL)
            })
            .mut_subcommand("script", |c| {
                let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_COMPLETION_SCRIPT);
                set_arg_help(c, "shell", i18n, keys::CLI_ARG_COMPLETION_SCRIPT_SHELL)
            })
        })
        .mut_subcommand("project", |c| rewrite_project(c, i18n))
        .mut_subcommand("workspace", |c| rewrite_workspace(c, i18n))
        .mut_subcommand("session", |c| rewrite_session(c, i18n))
        .mut_subcommand("runtime", |c| rewrite_runtime(c, i18n))
        .mut_subcommand("pane", |c| rewrite_pane(c, i18n));

    cmd
}

fn rewrite_project(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_PROJECT)
        .mut_subcommand("list", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_LIST);
            set_arg_help(c, "view", i18n, keys::CLI_ARG_LIST_VIEW)
        })
        .mut_subcommand("reindex", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_REINDEX);
            let c = set_arg_help(c, "root", i18n, keys::CLI_ARG_PROJECT_REINDEX_ROOT);
            let c = set_arg_help(
                c,
                "config_roots",
                i18n,
                keys::CLI_ARG_PROJECT_REINDEX_CONFIG_ROOTS,
            );
            let c = set_arg_help(
                c,
                "clear_first",
                i18n,
                keys::CLI_ARG_PROJECT_REINDEX_CLEAR_FIRST,
            );
            let c = set_arg_help(c, "dry_run", i18n, keys::CLI_ARG_PROJECT_REINDEX_DRY_RUN);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("roots", |c| rewrite_project_roots(c, i18n))
}

fn rewrite_project_roots(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS)
        .mut_subcommand("list", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS_LIST)
        })
        .mut_subcommand("add", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS_ADD)
        })
        .mut_subcommand("remove", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS_REMOVE)
        })
        .mut_subcommand("edit", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS_EDIT)
        })
        .mut_subcommand("status", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PROJECT_ROOTS_STATUS)
        })
}

fn rewrite_workspace(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_WORKSPACE)
        .mut_subcommand("open", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_OPEN);
            let c = set_arg_help(c, "workspace", i18n, keys::CLI_ARG_WORKSPACE_NAME_OPT);
            let c = set_arg_help(c, "no_attach", i18n, keys::CLI_ARG_OPEN_NO_ATTACH);
            let c = set_arg_help(c, "force", i18n, keys::CLI_ARG_OPEN_FORCE);
            let c = set_arg_help(c, "mode", i18n, keys::CLI_ARG_OPEN_MODE);
            let c = set_arg_help(c, "init", i18n, keys::CLI_ARG_OPEN_INIT);
            set_arg_help(
                c,
                "attach_existing_branch",
                i18n,
                keys::CLI_ARG_OPEN_ATTACH_EXISTING_BRANCH,
            )
        })
        .mut_subcommand("list", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_LIST);
            set_arg_help(c, "view", i18n, keys::CLI_ARG_LIST_VIEW)
        })
        .mut_subcommand("attach", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_ATTACH);
            set_arg_help(c, "workspace", i18n, keys::CLI_ARG_WORKSPACE_NAME_OPT)
        })
        .mut_subcommand("close", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_CLOSE);
            let c = set_arg_help(c, "kill", i18n, keys::CLI_ARG_CLOSE_KILL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("remove", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_REMOVE);
            let c = set_arg_help(c, "workspace", i18n, keys::CLI_ARG_WORKSPACE_NAME);
            let c = set_arg_help(c, "delete_branch", i18n, keys::CLI_ARG_REMOVE_DELETE_BRANCH);
            let c = set_arg_help(c, "keep_branch", i18n, keys::CLI_ARG_REMOVE_KEEP_BRANCH);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("destroy", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_DESTROY);
            let c = set_arg_help(
                c,
                "delete_branch",
                i18n,
                keys::CLI_ARG_DESTROY_DELETE_BRANCH,
            );
            let c = set_arg_help(c, "keep_branch", i18n, keys::CLI_ARG_DESTROY_KEEP_BRANCH);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("worktree", |c| rewrite_workspace_worktree(c, i18n))
}

fn rewrite_workspace_worktree(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_WORKSPACE_WORKTREE)
        .mut_subcommand("list", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_WORKTREE_LIST)
        })
        .mut_subcommand("remove", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_WORKSPACE_WORKTREE_REMOVE);
            let c = set_arg_help(c, "worktree", i18n, keys::CLI_ARG_WORKTREE_NAME_OPT);
            let c = set_arg_help(c, "force", i18n, keys::CLI_ARG_WORKTREE_FORCE);
            let c = set_arg_help(
                c,
                "delete_branch",
                i18n,
                keys::CLI_ARG_WORKTREE_DELETE_BRANCH,
            );
            set_arg_help(c, "keep_branch", i18n, keys::CLI_ARG_WORKTREE_KEEP_BRANCH)
        })
}

fn rewrite_session(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_SESSION)
        .mut_subcommand("list", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_SESSION_LIST);
            set_arg_help(c, "view", i18n, keys::CLI_ARG_LIST_VIEW)
        })
        .mut_subcommand("attach", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_SESSION_ATTACH);
            set_arg_help(c, "target", i18n, keys::CLI_ARG_WORKSPACE_NAME_OPT)
        })
        .mut_subcommand("resume", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_SESSION_RESUME);
            set_arg_help(c, "session", i18n, keys::CLI_ARG_SESSION_RESUME_SESSION)
        })
}

fn rewrite_runtime(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_RUNTIME).mut_subcommand("stop", |c| {
        let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_RUNTIME_STOP);
        set_arg_help(c, "force", i18n, keys::CLI_ARG_RUNTIME_FORCE)
    })
}

fn rewrite_pane(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_PANE)
        .mut_subcommand("split", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_SPLIT);
            let c = set_arg_help(c, "direction", i18n, keys::CLI_ARG_PANE_SPLIT_DIR);
            let c = set_arg_help(c, "cmd", i18n, keys::CLI_ARG_PANE_SPLIT_CMD);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("close", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_CLOSE);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("list", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_LIST)
        })
        .mut_subcommand("show", |c| {
            set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_SHOW)
        })
        .mut_subcommand("send", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_SEND);
            let c = set_arg_help(c, "pane", i18n, keys::CLI_ARG_PANE_TARGET);
            let c = set_arg_help(c, "text", i18n, keys::CLI_ARG_PANE_SEND_TEXT);
            let c = set_arg_help(c, "no_enter", i18n, keys::CLI_ARG_PANE_SEND_NO_ENTER);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        // The hidden `approve-hooks` alias must not leak English in
        // zh-CN direct `--help` output.
        // `mut_subcommand` traverses hidden subcommands too — they are
        // hidden from the parent's `Commands:` listing but still
        // resolvable, so we localize their direct help here for
        // parity with non-hidden commands. The `about` carries the
        // deprecation framing and the Codex `/hooks` trust reminder;
        // see the corresponding i18n key docs in
        // `workhive-core/src/i18n_keys.rs`.
        .mut_subcommand("approve-hooks", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_PANE_APPROVE_HOOKS);
            set_arg_help(c, "pane", i18n, keys::CLI_ARG_PANE_APPROVE_HOOKS_PANE)
        })
}

fn rewrite_config(cmd: Command, i18n: &dyn I18nPort) -> Command {
    set_about_zh(cmd, i18n, keys::CLI_HELP_CMD_CONFIG)
        .mut_subcommand("show", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_SHOW);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL);
            set_arg_help(c, "raw", i18n, keys::CLI_ARG_CONFIG_RAW)
        })
        .mut_subcommand("edit", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_EDIT);
            set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL)
        })
        .mut_subcommand("set", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_SET);
            let c = set_arg_help(c, "key", i18n, keys::CLI_ARG_CONFIG_KEY);
            let c = set_arg_help(c, "value", i18n, keys::CLI_ARG_CONFIG_VALUE);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("unset", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_UNSET);
            let c = set_arg_help(c, "key", i18n, keys::CLI_ARG_CONFIG_KEY);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("env-set", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_ENV_SET);
            let c = set_arg_help(c, "key", i18n, keys::CLI_ARG_CONFIG_KEY);
            let c = set_arg_help(c, "value", i18n, keys::CLI_ARG_CONFIG_VALUE);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("env-unset", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_ENV_UNSET);
            let c = set_arg_help(c, "key", i18n, keys::CLI_ARG_CONFIG_KEY);
            let c = set_arg_help(c, "global", i18n, keys::CLI_ARG_CONFIG_GLOBAL);
            set_arg_help(c, "force", i18n, keys::CLI_ARG_FORCE)
        })
        .mut_subcommand("explain", |c| {
            let c = set_about_zh(c, i18n, keys::CLI_HELP_CMD_CONFIG_EXPLAIN);
            set_arg_help(c, "key", i18n, keys::CLI_ARG_CONFIG_KEY)
        })
}

/// Replace the `help` description on a single argument by its derive id.
///
/// `Command::mut_arg` panics if the named argument does not exist on the
/// receiver. We match clap derive's snake_case id convention so the call
/// sites stay close to the actual `cli.rs` definitions.
fn set_arg_help(cmd: Command, arg_id: &str, i18n: &dyn I18nPort, key: &str) -> Command {
    let translated = i18n.resolve(Locale::ZhCn, key);
    // Override the short help AND clear the doc-comment-derived English
    // `long_help` so `--help` (which renders the long form) falls back to
    // the localized short help instead of leaking English for multi-line
    // argument descriptions.
    cmd.mut_arg(arg_id, |a| {
        a.help(translated)
            .long_help(clap::builder::Resettable::<clap::builder::StyledStr>::Reset)
    })
}

/// Localized hint appended after a clap parse error in zh-CN locale.
///
/// Used as the trailing line in [`format_parse_error`] so users see a
/// short Chinese pointer to the help command after every parse failure.
pub(crate) fn zh_parse_help_hint(i18n: &dyn I18nPort) -> String {
    i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_HELP_HINT)
}

/// Categorize a clap parse outcome so the entry-point can decide whether
/// to delegate rendering to clap (help/version display) or render the
/// Workhive-localized error block itself.
pub(crate) fn is_display_event(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

/// Render a clap parse error in the requested locale.
///
/// Returns a fully formatted multi-line block (without trailing newline)
/// that the caller writes to stderr. The output never contains clap's
/// English "error:" / "Usage:" tokens when `locale == Locale::ZhCn`;
/// instead it uses the workhive i18n catalog for those headings and a
/// localized message body keyed off [`ErrorKind`].
///
/// English locale falls back to clap's default rendering verbatim so
/// existing English UX is preserved.
pub(crate) fn format_parse_error(err: &ClapError, locale: Locale, i18n: &dyn I18nPort) -> String {
    if locale != Locale::ZhCn {
        // Trim trailing newline to keep our caller in charge of line
        // termination and avoid double-blank-line drift.
        let mut s = err.render().to_string();
        while s.ends_with('\n') {
            s.pop();
        }
        return s;
    }

    let mut out = String::new();
    let prefix = i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_ERROR_PREFIX);
    let body = render_zh_message(err, i18n);
    out.push_str(&prefix);
    if !prefix.ends_with(' ') && !body.starts_with(' ') {
        out.push(' ');
    }
    out.push_str(&body);

    if let Some(usage) = extract_usage_text(err) {
        let zh_usage = rewrite_usage_heading(&usage, i18n);
        out.push_str("\n\n");
        out.push_str(&zh_usage);
    }

    out.push_str("\n\n");
    out.push_str(&zh_parse_help_hint(i18n));
    out
}

fn render_zh_message(err: &ClapError, i18n: &dyn I18nPort) -> String {
    let kind = err.kind();
    match kind {
        ErrorKind::InvalidSubcommand
        | ErrorKind::UnknownArgument
        | ErrorKind::NoEquals
        | ErrorKind::InvalidUtf8 => {
            // For unknown command/subcommand and unknown option, clap stores
            // the offending input in `InvalidSubcommand` / `InvalidArg`.
            if let Some(name) = string_ctx(err, ContextKind::InvalidSubcommand) {
                let mut msg = i18n
                    .resolve(Locale::ZhCn, keys::CLI_PARSE_UNKNOWN_SUBCOMMAND)
                    .replace("{0}", &name);
                if let Some(suggestion) = string_ctx(err, ContextKind::SuggestedSubcommand) {
                    let tip = i18n
                        .resolve(Locale::ZhCn, keys::CLI_PARSE_UNKNOWN_SUBCOMMAND_TIP)
                        .replace("{0}", &suggestion);
                    msg.push_str("\n  ");
                    msg.push_str(&tip);
                }
                return msg;
            }
            if let Some(arg) = string_ctx(err, ContextKind::InvalidArg) {
                return i18n
                    .resolve(Locale::ZhCn, keys::CLI_PARSE_UNKNOWN_OPTION)
                    .replace("{0}", &arg);
            }
            i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_GENERIC)
        }
        ErrorKind::MissingRequiredArgument
        | ErrorKind::MissingSubcommand
        | ErrorKind::TooFewValues => {
            let arg = strings_ctx(err, ContextKind::InvalidArg)
                .unwrap_or_else(|| string_ctx(err, ContextKind::InvalidArg).unwrap_or_default());
            i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_MISSING_REQUIRED)
                .replace("{0}", &arg)
        }
        ErrorKind::InvalidValue
        | ErrorKind::ValueValidation
        | ErrorKind::TooManyValues
        | ErrorKind::WrongNumberOfValues => {
            let arg = string_ctx(err, ContextKind::InvalidArg).unwrap_or_default();
            let value = string_ctx(err, ContextKind::InvalidValue).unwrap_or_default();
            if arg.contains("backend") {
                return i18n
                    .resolve(Locale::ZhCn, keys::CLI_PARSE_BACKEND_UNAVAILABLE)
                    .replace("{0}", &value);
            }
            i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_INVALID_VALUE)
                .replace("{0}", &value)
                .replace("{1}", &arg)
        }
        // Catch-all: surface whatever short message clap built (rendered
        // without ANSI styling), prefixed with our localized error: token.
        // This keeps the renderer locale-correct on heading/usage even for
        // less common error kinds without an explicit Chinese template.
        _ => {
            let raw = err.render().to_string();
            extract_first_line_after_error_token(&raw)
                .unwrap_or_else(|| i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_GENERIC))
        }
    }
}

fn string_ctx(err: &ClapError, kind: ContextKind) -> Option<String> {
    err.context().find_map(|(k, v)| {
        if k == kind {
            match v {
                ContextValue::String(s) => Some(s.clone()),
                ContextValue::StyledStr(s) => Some(s.to_string()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn strings_ctx(err: &ClapError, kind: ContextKind) -> Option<String> {
    err.context().find_map(|(k, v)| {
        if k == kind {
            match v {
                ContextValue::Strings(items) => Some(items.join(", ")),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn extract_usage_text(err: &ClapError) -> Option<String> {
    err.context().find_map(|(k, v)| {
        if k == ContextKind::Usage {
            match v {
                ContextValue::StyledStr(s) => Some(s.to_string()),
                ContextValue::String(s) => Some(s.clone()),
                _ => None,
            }
        } else {
            None
        }
    })
}

/// Replace clap's hardcoded English "Usage:" heading with the localized one.
fn rewrite_usage_heading(usage: &str, i18n: &dyn I18nPort) -> String {
    let zh_heading = i18n.resolve(Locale::ZhCn, keys::CLI_PARSE_USAGE_HEADING);
    if let Some(rest) = usage.strip_prefix("Usage:") {
        // Preserve clap's original whitespace after the heading.
        let mut out = String::with_capacity(zh_heading.len() + rest.len());
        out.push_str(&zh_heading);
        // Avoid duplicating a leading colon if zh heading already ends with one.
        if !zh_heading.ends_with('：') && !zh_heading.ends_with(':') {
            out.push(':');
        }
        out.push_str(rest);
        out
    } else {
        usage.to_owned()
    }
}

/// Pull the short message portion from clap's rendered error block.
///
/// clap formats real errors as `error: <message>` followed by usage and a
/// help hint. For catch-all `ErrorKind` variants we surface the message
/// after the `error:` token while ignoring the rest of the styled block.
fn extract_first_line_after_error_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start();
    let body = trimmed.strip_prefix("error:").unwrap_or(trimmed);
    let line = body.lines().next()?.trim();
    if line.is_empty() {
        None
    } else {
        Some(line.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use clap::FromArgMatches;
    use workhive_infra::i18n::StaticI18n;

    fn parse_err(args: &[&str]) -> ClapError {
        let i18n = StaticI18n::new();
        let mut cmd = localize_command(crate::cli::Cli::command(), Locale::ZhCn, &i18n);
        match cmd.try_get_matches_from_mut(args.iter().copied()) {
            Ok(matches) => match crate::cli::Cli::from_arg_matches(&matches) {
                Ok(_) => panic!("expected parse failure for {args:?}"),
                Err(e) => e,
            },
            Err(e) => e,
        }
    }

    #[test]
    fn english_locale_is_a_noop() {
        let cmd = crate::cli::Cli::command();
        let original_about = cmd
            .get_about()
            .map(|s| s.to_string())
            .expect("derive sets about");
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::En, &i18n);
        assert_eq!(
            cmd.get_about().map(|s| s.to_string()).unwrap_or_default(),
            original_about,
            "English locale must not rewrite the command tree"
        );
    }

    #[test]
    fn zh_cn_rewrites_top_level_about_and_template() {
        let cmd = crate::cli::Cli::command();
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::ZhCn, &i18n);
        let about = cmd
            .get_about()
            .map(|s| s.to_string())
            .expect("about must remain set after rewrite");
        assert!(
            about.contains("终端工作区管理器"),
            "zh-CN about should contain Chinese, got: {about}",
        );
    }

    #[test]
    fn zh_cn_rewrites_resource_subcommand_about() {
        let cmd = crate::cli::Cli::command();
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::ZhCn, &i18n);
        let workspace = cmd
            .find_subcommand("workspace")
            .expect("workspace subcommand exists");
        let about = workspace
            .get_about()
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert!(
            about.contains("管理工作区"),
            "zh-CN workspace about should be Chinese, got: {about}",
        );
    }

    #[test]
    fn zh_cn_rewrites_object_scoped_action_about() {
        let cmd = crate::cli::Cli::command();
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::ZhCn, &i18n);
        let workspace = cmd
            .find_subcommand("workspace")
            .expect("workspace subcommand exists");
        let open = workspace
            .find_subcommand("open")
            .expect("open action exists");
        let about = open.get_about().map(|s| s.to_string()).unwrap_or_default();
        assert!(
            about.contains("打开或恢复工作区"),
            "zh-CN open about should be Chinese, got: {about}",
        );
    }

    #[test]
    fn zh_cn_rewrites_argument_help_descriptions() {
        let cmd = crate::cli::Cli::command();
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::ZhCn, &i18n);

        let workspace = cmd.find_subcommand("workspace").expect("workspace exists");
        let open = workspace.find_subcommand("open").expect("open exists");
        let no_attach_help = open
            .get_arguments()
            .find(|a| a.get_id().as_str() == "no_attach")
            .and_then(|a| a.get_help())
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert!(
            no_attach_help.contains("不要启动运行时客户端"),
            "no_attach help must be Chinese, got: {no_attach_help}",
        );
    }

    #[test]
    fn zh_cn_does_not_translate_flag_tokens() {
        // Flag long names must stay English even when help is Chinese.
        let cmd = crate::cli::Cli::command();
        let i18n = StaticI18n::new();
        let cmd = localize_command(cmd, Locale::ZhCn, &i18n);
        let workspace = cmd.find_subcommand("workspace").expect("workspace exists");
        let open = workspace.find_subcommand("open").expect("open exists");
        let no_attach_long = open
            .get_arguments()
            .find(|a| a.get_id().as_str() == "no_attach")
            .and_then(|a| a.get_long())
            .map(|s| s.to_string())
            .unwrap_or_default();
        assert_eq!(no_attach_long, "no-attach");
    }

    #[test]
    fn zh_parse_hint_renders_chinese() {
        let i18n = StaticI18n::new();
        let hint = zh_parse_help_hint(&i18n);
        assert!(
            hint.contains("完整用法"),
            "zh hint should be Chinese, got: {hint}",
        );
    }

    #[test]
    fn format_unknown_subcommand_in_zh() {
        let err = parse_err(&["workhive", "ls"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            block.contains("错误：") || block.starts_with("错误"),
            "missing zh error prefix in: {block}",
        );
        assert!(
            block.contains("未知的子命令"),
            "missing zh unknown subcommand text in: {block}",
        );
        assert!(
            !block.contains("error:"),
            "must not contain English 'error:' token: {block}",
        );
        assert!(
            !block.to_lowercase().contains("usage:"),
            "must not contain English 'Usage:' token: {block}",
        );
        assert!(
            block.contains("用法："),
            "missing zh usage heading in: {block}",
        );
        assert!(
            block.contains("完整用法"),
            "missing zh help hint in: {block}",
        );
    }

    #[test]
    fn format_unknown_option_in_zh() {
        let err = parse_err(&["workhive", "workspace", "open", "--bogus"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            block.contains("未知的选项"),
            "missing zh unknown option text in: {block}",
        );
        assert!(block.contains("--bogus"), "missing flag token in: {block}");
        assert!(
            !block.contains("error:"),
            "must not contain English 'error:' token: {block}",
        );
    }

    #[test]
    fn format_missing_required_in_zh() {
        let err = parse_err(&["workhive", "workspace", "remove"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            block.contains("缺少必填参数"),
            "missing zh missing-required text in: {block}",
        );
        assert!(
            !block.contains("error:"),
            "must not contain English 'error:' token: {block}",
        );
    }

    #[test]
    fn format_invalid_value_in_zh() {
        let err = parse_err(&["workhive", "workspace", "list", "--view", "bogus"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            block.contains("不合法") || block.contains("invalid"),
            "missing zh invalid value text in: {block}",
        );
        assert!(
            !block.contains("error:"),
            "must not contain English 'error:' token: {block}",
        );
    }

    #[test]
    fn format_invalid_backend_in_zh_names_the_backend() {
        let err = parse_err(&["workhive", "init", "--global", "--backend", "bogus"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            block.contains("运行时后端 'bogus' 不可用"),
            "unavailable backend must reuse the dedicated zh copy: {block}",
        );
    }

    #[test]
    fn english_locale_keeps_clap_default_rendering() {
        let err = parse_err(&["workhive", "ls"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::En, &i18n);
        assert!(
            block.contains("error:"),
            "EN locale must keep clap's default 'error:' token: {block}",
        );
    }

    #[test]
    fn zh_block_omits_clap_for_more_information_token() {
        // clap appends `For more information, try '--help'.` in English.
        // Our localized block must not echo that token at all; instead
        // the trailing line is the zh help hint.
        let err = parse_err(&["workhive", "workspace", "remove"]);
        let i18n = StaticI18n::new();
        let block = format_parse_error(&err, Locale::ZhCn, &i18n);
        assert!(
            !block.contains("For more information"),
            "must not contain clap's English help-hint: {block}",
        );
    }

    #[test]
    fn is_display_event_matches_help_and_version() {
        assert!(is_display_event(ErrorKind::DisplayHelp));
        assert!(is_display_event(ErrorKind::DisplayVersion));
        assert!(is_display_event(
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        ));
        assert!(!is_display_event(ErrorKind::InvalidSubcommand));
        assert!(!is_display_event(ErrorKind::MissingRequiredArgument));
    }

    // ── clap panic prevention ──
    //
    // The tests below pin two contracts so a future edit cannot reintroduce
    // a dangling-argument crash in the localized clap command tree:
    //
    // 1. `clap::CommandFactory::command()` must pass `debug_assert()` for the
    //    English derive output AND for the zh-CN rewritten command tree. This
    //    catches dangling `mut_arg` references (the failure mode that fired
    //    during `workhive workspace close` before the dangling `enter` arg was
    //    removed) at test time, not in production.
    //
    // 2. Every visible help node must render `--help` without panicking. The
    //    recursive walker materializes clap's internal `build()` step for both
    //    locales by calling `try_get_matches_from_mut(&[<path>, "--help"])`,
    //    which is exactly the path users hit in zh-CN where mut_arg / mut_subcommand
    //    rewrites historically tripped clap's internal invariants.

    /// English locale: `Cli::command()` must satisfy `debug_assert()` on every
    /// node. This is clap's own self-test for the derive output and would
    /// catch the dangling-`enter` panic at compile-time-equivalent severity.
    #[test]
    fn command_factory_passes_debug_assert_en() {
        let cmd = crate::cli::Cli::command();
        let mut cmd = localize_command(cmd, Locale::En, &StaticI18n::new());
        cmd.build();
        cmd.debug_assert();
    }

    /// zh-CN locale: same contract, but the command tree has been rewritten
    /// by `localize_command`. This is the path the user actually hits at
    /// runtime when they have set `language = "zh-CN"`, so a panic here
    /// translates directly into a startup crash for zh-CN users.
    #[test]
    fn command_factory_passes_debug_assert_zh_cn() {
        let cmd = crate::cli::Cli::command();
        let mut cmd = localize_command(cmd, Locale::ZhCn, &StaticI18n::new());
        cmd.build();
        cmd.debug_assert();
    }

    /// Walk every reachable command path and assert that `--help` parses
    /// without panicking on the rewritten command tree. Produces a deeper
    /// guarantee than `debug_assert()` because it runs clap's full parse
    /// pipeline for every (locale, path) pair.
    fn assert_help_traversal_succeeds_for_locale(locale: Locale) {
        let i18n = StaticI18n::new();
        let cmd = localize_command(crate::cli::Cli::command(), locale, &i18n);

        // Collect every visible subcommand path as `Vec<&str>` rooted at
        // the binary name. Hidden subcommands (e.g. `internal`) are
        // excluded because they have intentionally-undocumented help.
        fn collect_paths(cmd: &Command, prefix: Vec<String>, out: &mut Vec<Vec<String>>) {
            out.push(prefix.clone());
            for sub in cmd.get_subcommands() {
                if sub.is_hide_set() {
                    continue;
                }
                let mut next = prefix.clone();
                next.push(sub.get_name().to_owned());
                collect_paths(sub, next, out);
            }
        }

        let mut paths: Vec<Vec<String>> = Vec::new();
        collect_paths(&cmd, vec!["workhive".to_owned()], &mut paths);

        for path in &paths {
            // Build a fresh tree per traversal because `try_get_matches_from_mut`
            // mutates the receiver internally.
            let mut sub_cmd = localize_command(crate::cli::Cli::command(), locale, &i18n);
            let mut argv = path.clone();
            argv.push("--help".to_owned());

            match sub_cmd.try_get_matches_from_mut(argv.iter()) {
                Ok(_) => panic!("--help should never produce a parsed match, path: {path:?}"),
                Err(err) => {
                    // `--help` always returns an `Err` whose `kind` is
                    // `DisplayHelp`. Any other error means the tree could
                    // not even render help — which is the precise panic
                    // mode this test prevents.
                    assert!(
                        is_display_event(err.kind()),
                        "expected DisplayHelp for path {path:?} (locale={locale:?}), got {:?}: {err}",
                        err.kind()
                    );
                }
            }
        }
        assert!(
            !paths.is_empty(),
            "help traversal must visit at least the root command"
        );
    }

    /// English locale: every visible subcommand path renders `--help`
    /// cleanly, including the long-tail commands (`workhive workspace
    /// worktree remove --help`, `workhive workspace close --help`, …).
    #[test]
    fn help_traversal_does_not_panic_en() {
        assert_help_traversal_succeeds_for_locale(Locale::En);
    }

    /// zh-CN locale: same property, but on the rewritten command tree
    /// where `mut_arg` / `mut_subcommand` rewrites were the historical
    /// source of `Argument 'enter' is undefined` style panics.
    #[test]
    fn help_traversal_does_not_panic_zh_cn() {
        assert_help_traversal_succeeds_for_locale(Locale::ZhCn);
    }

    /// Render `--help` for a fully-qualified command path on the
    /// rewritten zh-CN command tree and return the formatted string
    /// clap would have printed. Used by the leaf-command help-shape
    /// regression tests below.
    fn render_help_zh(path: &[&str]) -> String {
        let i18n = StaticI18n::new();
        let cmd = localize_command(crate::cli::Cli::command(), Locale::ZhCn, &i18n);
        let mut sub_cmd = cmd;
        for segment in path.iter().skip(1) {
            sub_cmd = sub_cmd
                .find_subcommand(*segment)
                .cloned()
                .unwrap_or_else(|| panic!("missing subcommand {segment:?} for path {path:?}"));
        }
        sub_cmd.render_help().to_string()
    }

    /// `workhive pane list --help` is a leaf command with neither
    /// visible subcommands nor positional arguments. The zh-CN help
    /// block must not render the bare `命令:` and `参数:` headings.
    #[test]
    fn zh_cn_leaf_help_omits_empty_commands_and_arguments_sections() {
        let help = render_help_zh(&["workhive", "pane", "list"]);
        assert!(
            !help.contains("命令:\n选项"),
            "leaf pane list --help must not show empty 命令: heading:\n{help}",
        );
        assert!(
            !help.contains("命令:\n参数"),
            "leaf pane list --help must not show empty 命令: heading:\n{help}",
        );
        assert!(
            !help.contains("参数:\n选项"),
            "leaf pane list --help must not show empty 参数: heading:\n{help}",
        );
        // Sanity: the localized usage and options chrome must still
        // render in Chinese (no leak of clap's English chrome).
        assert!(
            help.contains("用法:"),
            "leaf pane list --help must still localize the 用法: heading:\n{help}",
        );
        assert!(
            help.contains("选项:"),
            "leaf pane list --help must still localize the 选项: heading:\n{help}",
        );
    }

    /// `workhive pane send --help` is a leaf command with positional
    /// arguments but no subcommands. The zh-CN help block must keep the
    /// populated `参数:` section and drop the otherwise-empty `命令:`
    /// section.
    #[test]
    fn zh_cn_leaf_with_positionals_keeps_arguments_drops_commands_section() {
        let help = render_help_zh(&["workhive", "pane", "send"]);
        assert!(
            help.contains("参数:"),
            "pane send --help must still render 参数: heading:\n{help}",
        );
        assert!(
            !help.contains("命令:\n参数"),
            "pane send --help must not render empty 命令: heading before 参数::\n{help}",
        );
        assert!(
            !help.contains("命令:\n选项"),
            "pane send --help must not render empty 命令: heading before 选项::\n{help}",
        );
    }

    /// `workhive pane --help` is a category command with visible
    /// subcommands but no positional arguments. The zh-CN help block
    /// must keep `命令:` and drop the otherwise-empty `参数:` section.
    #[test]
    fn zh_cn_category_with_subcommands_keeps_commands_drops_arguments_section() {
        let help = render_help_zh(&["workhive", "pane"]);
        assert!(
            help.contains("命令:"),
            "pane --help must still render 命令: heading:\n{help}",
        );
        assert!(
            !help.contains("参数:\n选项"),
            "pane --help must not render empty 参数: heading before 选项::\n{help}",
        );
    }
}

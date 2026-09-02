//! `workhive completion` handlers.
//!
//! Generates a shell completion script for the requested shell to standard
//! output using `clap_complete`, or installs that script into Workhive's
//! global configuration directory.
//!
//! Hidden subcommands (anything tagged `#[command(hide = true)]` in
//! [`crate::cli::Cli`], such as the `internal` command tree) are not
//! emitted in the generated script because `clap_complete::generate`
//! consumes the same hidden flags clap uses for help rendering.
//!
//! In addition to the static command tree, this module appends a small
//! per-shell dynamic-candidate hook so that:
//!
//! - `workspace open|attach|remove <TAB>` completes workspace names
//! - `pane send <TAB>` completes pane targets
//!
//! The dynamic hook contains **no business logic** — it simply calls
//! `workhive internal completion candidates <kind>` and feeds the
//! resulting one-per-line stdout into the shell's completion engine.

use std::{collections::HashSet, env, fs, io, path::PathBuf};

use clap::{Command, CommandFactory};
use clap_complete::{Shell, generate};
use workhive_core::domain::Locale;
use workhive_core::i18n_keys as k;
use workhive_core::ports::I18nPort;
use workhive_infra::config::resolve_global_config_dir;

use crate::cli::Cli;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::localize::localize_command;

/// Binary name embedded in completion scripts.
pub(crate) const BINARY_NAME: &str = "workhive";
const ZSH_RC_BEGIN: &str = "# >>> workhive completion >>>";
const ZSH_RC_END: &str = "# <<< workhive completion <<<";

/// Render a shell completion script for `shell` to the given writer,
/// localizing every command description through the i18n catalog.
///
/// The
/// completion output must respect the user's `language` preference.
/// Calling `Cli::command()` directly would emit clap's English
/// `about` strings even under `language = "zh-CN"`, leaking phrases
/// like "Attach to a workspace" into the generated zsh / bash
/// scripts. We funnel the same `localize_command` rewriter that
/// powers `--help` rendering, then strip hidden subcommands and let
/// `clap_complete::generate` produce the script.
///
/// Pulled out as a small free function so unit tests can capture
/// output without driving the whole CLI binary.
pub(crate) fn write_completions<W: io::Write>(
    shell: Shell,
    locale: Locale,
    i18n: &dyn I18nPort,
    writer: &mut W,
) {
    let localized = localize_command(Cli::command(), locale, i18n);
    let mut cmd = strip_hidden_subcommands(localized);
    generate(shell, &mut cmd, BINARY_NAME, writer);
    let _ = writer.write_all(dynamic_hook(shell).as_bytes());
}

/// Recursively rebuild `cmd` without any subcommand whose `hide` flag is set.
///
/// **Why we rebuild instead of mutating in place:**
/// clap_complete 4.6's static shell generators (bash/zsh/fish/elvish/
/// powershell) still emit completion entries for subcommands flagged
/// `#[command(hide = true)]` (clap-rs/clap#1335, #2541). Without
/// filtering, `workhive completion script zsh` would advertise the entire
/// `internal` tree (`internal snapshot monitor`, `internal snapshot
/// notify`, ...). Those commands exist only for tmux hooks and the
/// snapshot supervisor and must never be advertised to end users.
///
/// `clap::Command` does not expose a `remove_subcommand` API, so we
/// rebuild rather than mutate in place. We rely on the `string` cargo
/// feature of clap (enabled at workspace level) to satisfy
/// `Str: From<String>`, which lets us reuse owned `String`s without
/// `Box::leak`. The rebuild copies over the structural fields that
/// completion generators actually consume:
/// - command name
/// - all args (with value parsers / value enums / short/long flags
///   / value names — preserved through `Arg::clone`)
/// - all aliases (visible + hidden)
/// - non-hidden subcommands (recursively)
/// - `version`, `about`, `long_about` (kept so generators still surface
///   `--version` / `--help` properly in shells like fish)
fn strip_hidden_subcommands(cmd: Command) -> Command {
    let mut rebuilt = Command::new(cmd.get_name().to_owned());

    if let Some(version) = cmd.get_version() {
        rebuilt = rebuilt.version(version.to_owned());
    }
    if let Some(about) = cmd.get_about() {
        rebuilt = rebuilt.about(about.to_string());
    }
    if let Some(long_about) = cmd.get_long_about() {
        rebuilt = rebuilt.long_about(long_about.to_string());
    }

    // Preserve auto-flag/subcommand suppression that the localizer
    // installs via `disable_help_flag` / `disable_version_flag` /
    // `disable_help_subcommand`. Without this, the rebuilt command
    // re-acquires clap's auto-generated `help` flag, which then
    // collides with the localized `help` arg the localizer injected
    // and triggers a duplicate-argument debug assertion at
    // `Command::build()` time.
    if cmd.is_disable_help_flag_set() {
        rebuilt = rebuilt.disable_help_flag(true);
    }
    if cmd.is_disable_version_flag_set() {
        rebuilt = rebuilt.disable_version_flag(true);
    }
    if cmd.is_disable_help_subcommand_set() {
        rebuilt = rebuilt.disable_help_subcommand(true);
    }

    for arg in cmd.get_arguments() {
        rebuilt = rebuilt.arg(arg.clone());
    }
    let visible_aliases: HashSet<String> = cmd
        .get_visible_aliases()
        .map(|alias| alias.to_owned())
        .collect();
    for alias in &visible_aliases {
        rebuilt = rebuilt.visible_alias(alias.to_owned());
    }
    for alias in cmd.get_all_aliases() {
        if !visible_aliases.contains(alias) {
            rebuilt = rebuilt.alias(alias.to_owned());
        }
    }
    for sub in cmd.get_subcommands() {
        if sub.is_hide_set() {
            continue;
        }
        rebuilt = rebuilt.subcommand(strip_hidden_subcommands(sub.clone()));
    }
    rebuilt
}

/// Emit the per-shell dynamic-candidate hook to append after the static
/// clap_complete script.
///
/// Each shell's hook is tiny on purpose: it never queries git, tmux, or
/// the project index directly. It only invokes
/// `workhive internal completion candidates <kind>` and feeds the result
/// into the shell's completion engine. The actual candidate logic is
/// owned by Rust use cases under `workhive_core::usecases::completion`,
/// reached through the hidden CLI boundary.
fn dynamic_hook(shell: Shell) -> String {
    match shell {
        Shell::Bash => bash_dynamic_hook().to_owned(),
        Shell::Zsh => zsh_dynamic_hook().to_owned(),
        Shell::Fish => fish_dynamic_hook().to_owned(),
        // PowerShell and Elvish do not get a custom dynamic hook in
        // this pass — their static completion still works for the rest
        // of the surface. Workspace/pane dynamic candidates remain as
        // a future extension if a Workhive user requests it.
        _ => String::new(),
    }
}

/// Bash dynamic dispatcher.
///
/// Wraps the clap-generated `_workhive` function so the completion
/// callback short-circuits to dynamic candidates for the four explicit
/// positional argument positions, and otherwise falls through to the
/// static script. The dispatcher uses `compgen -W` to filter the
/// dynamic candidate list against the user's current prefix.
fn bash_dynamic_hook() -> &'static str {
    r#"
# Workhive dynamic completion hook (appended by `workhive completion script bash`).
# Wraps the clap_complete static dispatcher above. Calls
# `workhive internal completion candidates <kind>` to obtain dynamic
# workspace / pane candidates without embedding any git/tmux logic in
# the shell.
_workhive_dynamic_kind() {
    # Args: position-of-cur word, rest of words array
    local cwd="$1"
    if [ "$cwd" -lt 3 ]; then
        echo ""
        return
    fi
    local first="${COMP_WORDS[1]}"
    local second="${COMP_WORDS[2]}"
    case "$first" in
        workspace)
            case "$second" in
                open|attach|remove)
                    if [ "$cwd" -eq 3 ]; then
                        echo "workspace"
                        return
                    fi
                    ;;
            esac
            ;;
        pane)
            if [ "$second" = "send" ] && [ "$cwd" -eq 3 ]; then
                echo "pane"
                return
            fi
            ;;
    esac
    echo ""
}

_workhive_dynamic() {
    local cur="${COMP_WORDS[COMP_CWORD]}"
    local kind
    kind="$(_workhive_dynamic_kind "$COMP_CWORD")"
    if [ -n "$kind" ] && [[ "$cur" != -* ]]; then
        local raw
        raw="$(workhive internal completion candidates "$kind" 2>/dev/null)"
        # shellcheck disable=SC2207
        COMPREPLY=( $(compgen -W "$raw" -- "$cur") )
        return 0
    fi
    _workhive "$@"
}
complete -F _workhive_dynamic -o nosort -o bashdefault -o default workhive
"#
}

/// Zsh dynamic dispatcher.
///
/// Reuses the clap-generated `_workhive` function via `compdef` then
/// re-registers a wrapper that intercepts the four explicit positional
/// argument positions and serves dynamic candidates from
/// `workhive internal completion candidates`. Uses `_describe` so zsh
/// renders candidates with proper grouping.
fn zsh_dynamic_hook() -> &'static str {
    r#"
# Workhive dynamic completion hook (appended by `workhive completion script zsh`).
# Intercepts positional candidate slots (workspace open/attach/remove,
# pane send) and routes to `workhive internal completion candidates`,
# falling back to the clap-generated `_workhive` function above for
# everything else.
_workhive_dynamic_dispatch() {
    local words_before
    words_before=( ${(z)BUFFER} )
    local nwords=${#words_before[@]}
    local cur=${words_before[CURRENT]}
    if [[ $nwords -ge 3 ]]; then
        local first=${words_before[2]}
        local second=${words_before[3]}
        if [[ $first == "workspace" && ( $second == "open" || $second == "attach" || $second == "remove" ) && $CURRENT -eq 4 && $cur != -* ]]; then
            local -a candidates
            candidates=( ${(f)"$(command workhive internal completion candidates workspace 2>/dev/null)"} )
            _describe 'workspace' candidates
            return
        fi
        if [[ $first == "pane" && $second == "send" && $CURRENT -eq 4 && $cur != -* ]]; then
            local -a candidates
            candidates=( ${(f)"$(command workhive internal completion candidates pane 2>/dev/null)"} )
            _describe 'pane' candidates
            return
        fi
    fi
    _workhive "$@"
}
compdef _workhive_dynamic_dispatch workhive
"#
}

/// Fish dynamic completion hook.
///
/// Fish lets us register additional `complete` lines after the
/// clap-generated block. Each registration delegates the candidate
/// fetch to `workhive internal completion candidates` so the shell
/// itself stays free of business logic.
fn fish_dynamic_hook() -> &'static str {
    r#"
# Workhive dynamic completion hook (appended by `workhive completion script fish`).
# Adds dynamic candidates for the positional slots that need them. Each
# `complete` line shells out to `workhive internal completion candidates`,
# never to git/tmux directly.
function __workhive_dynamic_workspace
    workhive internal completion candidates workspace 2>/dev/null
end
function __workhive_dynamic_pane
    workhive internal completion candidates pane 2>/dev/null
end
complete -c workhive -n '__fish_seen_subcommand_from workspace; and __fish_seen_subcommand_from open' -f -a '(__workhive_dynamic_workspace)'
complete -c workhive -n '__fish_seen_subcommand_from workspace; and __fish_seen_subcommand_from attach' -f -a '(__workhive_dynamic_workspace)'
complete -c workhive -n '__fish_seen_subcommand_from workspace; and __fish_seen_subcommand_from remove' -f -a '(__workhive_dynamic_workspace)'
complete -c workhive -n '__fish_seen_subcommand_from pane; and __fish_seen_subcommand_from send' -f -a '(__workhive_dynamic_pane)'
"#
}

/// Entry point for the `workhive completion script <shell>` command.
///
/// Uses the same locale-aware command tree as `--help` rendering so
/// the generated descriptions follow the user's `language` preference.
pub(crate) fn cmd_completions(shell: Shell, ctx: &CommandContext) -> Result<(), CliError> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    write_completions(shell, ctx.locale(), &ctx.i18n, &mut handle);
    Ok(())
}

fn cmd_completions_install(shell: Shell, ctx: &CommandContext) -> Result<(), CliError> {
    let result = install_completions(shell, ctx)?;
    eprintln!("{}", result.message(ctx));
    Ok(())
}

/// Entry point for `workhive completion install [shell]`.
pub(crate) fn cmd_completion_install(
    shell: Option<Shell>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let shell = match shell {
        Some(shell) => shell,
        None => detect_shell()?,
    };
    cmd_completions_install(shell, ctx)
}

/// Best-effort shell auto-detection for the current process.
///
/// Wraps the private [`detect_shell`] so callers outside this module
/// (notably the global-init wizard guidance flow) can ask "do we
/// know what shell the user is in?" without conflating an unknown
/// shell with an actual failure.
pub(crate) fn detect_current_shell() -> Option<Shell> {
    detect_shell().ok()
}

/// Decision input for [`plan_completion_guidance`].
///
/// Pure data so the planner stays trivially testable: the caller
/// captures the runtime facts (interactive TTY? auto-detected shell?)
/// once, then this module decides how to present the next step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompletionGuidanceInput {
    /// `true` when stdin is a TTY and the wizard ran interactively.
    /// Drives whether we may *ask* the user to install completions.
    /// When `false` we MUST only print a manual hint — silently
    /// installing into a non-interactive session is forbidden because
    /// it would risk modifying the user's `~/.zshrc`
    /// without consent.
    pub interactive: bool,
    /// Result of [`detect_current_shell`]. `None` means we could not
    /// auto-detect and the user must use the explicit
    /// `workhive completion install <shell>` form.
    pub detected_shell: Option<Shell>,
}

/// Planned next action for completion guidance after global init.
///
/// * On a TTY with a known shell, ask Yes/No before doing anything
///   that could touch the user's shell startup files.
/// * On a TTY where the shell is unknown, do NOT guess — instead
///   surface the explicit `workhive completion install <shell>`
///   command so the user picks deliberately.
/// * Without a TTY (CI, scripts, piped stdin), ONLY print the manual
///   hint. Never prompt and never install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionGuidancePlan {
    /// Interactive flow: explain, then ask Yes/No. On Yes, reuse
    /// `cmd_completions_install` with this shell.
    OfferInteractive { shell: Shell },
    /// Interactive flow but auto-detection failed: print the
    /// explicit-form hint instead of prompting Yes/No (the user must
    /// pick a shell).
    PromptExplicitShell,
    /// Non-interactive flow: only print the manual command. The
    /// detected shell, when known, is included in the hint to make
    /// the next step a copy-paste command.
    PrintManualHint { shell: Option<Shell> },
}

/// Pure planner: combine the runtime facts into a guidance plan.
///
/// Has no side effects, no I/O, no terminal interaction. The actual
/// rendering / prompting / installing happens in
/// [`render_completion_guidance_lines`] and the `cmd_init` wiring.
pub(crate) fn plan_completion_guidance(input: CompletionGuidanceInput) -> CompletionGuidancePlan {
    match (input.interactive, input.detected_shell) {
        (true, Some(shell)) => CompletionGuidancePlan::OfferInteractive { shell },
        (true, None) => CompletionGuidancePlan::PromptExplicitShell,
        (false, detected) => CompletionGuidancePlan::PrintManualHint { shell: detected },
    }
}

/// Localized lines that the wizard prints (or assembles into a
/// confirm prompt) for a given guidance plan.
///
/// Returned as a small struct rather than a single pre-formatted
/// string so tests can inspect each piece independently and the
/// caller can reuse the intro line without repeating it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompletionGuidanceCopy {
    /// Multi-line intro that explains what `workhive completion
    /// install` does. Always included so the user understands what
    /// they would be opting into.
    pub intro: String,
    /// Yes/No question text used by the confirm widget when the plan
    /// is [`CompletionGuidancePlan::OfferInteractive`]. Empty for
    /// non-interactive plans.
    pub prompt: String,
    /// Hint to print AFTER the answer (or instead of asking, for
    /// non-interactive / unknown-shell plans). Always includes the
    /// literal command `workhive completion install`,
    /// so users can copy-paste even when
    /// they didn't take the prompt.
    pub manual_hint: String,
}

/// Render the localized intro / prompt / manual-hint lines for a
/// guidance plan.
///
/// The contract: every plan must produce a
/// `manual_hint` that mentions `workhive completion install`. The
/// non-detected variant additionally tells the user about the
/// `<shell>` argument so they can disambiguate.
pub(crate) fn render_completion_guidance_lines(
    plan: CompletionGuidancePlan,
    locale: Locale,
    i18n: &dyn I18nPort,
) -> CompletionGuidanceCopy {
    let intro = i18n.resolve(locale, k::INIT_GLOBAL_COMPLETION_INTRO);
    let manual_hint = match plan {
        CompletionGuidancePlan::OfferInteractive { shell } => i18n.resolve_with_args(
            locale,
            k::INIT_GLOBAL_COMPLETION_MANUAL_HINT_WITH_SHELL,
            &[&shell.to_string()],
        ),
        CompletionGuidancePlan::PromptExplicitShell => {
            i18n.resolve(locale, k::INIT_GLOBAL_COMPLETION_SHELL_NOT_DETECTED)
        }
        CompletionGuidancePlan::PrintManualHint { shell: Some(shell) } => i18n.resolve_with_args(
            locale,
            k::INIT_GLOBAL_COMPLETION_MANUAL_HINT_WITH_SHELL,
            &[&shell.to_string()],
        ),
        CompletionGuidancePlan::PrintManualHint { shell: None } => {
            i18n.resolve(locale, k::INIT_GLOBAL_COMPLETION_MANUAL_HINT)
        }
    };
    let prompt = match plan {
        CompletionGuidancePlan::OfferInteractive { .. } => {
            i18n.resolve(locale, k::INIT_GLOBAL_COMPLETION_PROMPT)
        }
        _ => String::new(),
    };
    CompletionGuidanceCopy {
        intro,
        prompt,
        manual_hint,
    }
}

/// Entry point used by `execute_init` for the global flow.
///
/// Steps:
///
/// 1. Snapshot runtime facts (TTY + shell auto-detect).
/// 2. Plan the next action via [`plan_completion_guidance`].
/// 3. Print the intro and either prompt the user (interactive,
///    known shell) or print the localized manual hint.
/// 4. On an explicit Yes, reuse [`cmd_completions_install`] so the
///    same install routine drives both the standalone command and
///    the wizard path. On a No / non-interactive run / unknown
///    shell, do NOT install — only educate.
///
/// This function is the ONLY place in `init --global` that may
/// trigger shell-completion install logic. Every code path that
/// reaches `cmd_completions_install` is gated by an explicit user
/// confirmation, satisfying the "no silent shell-rc
/// modification" requirement.
pub(crate) fn guide_completion_install_after_global_init(
    ctx: &CommandContext,
    locale: Locale,
    interactive: bool,
) -> Result<(), CliError> {
    let plan = plan_completion_guidance(CompletionGuidanceInput {
        interactive,
        detected_shell: detect_current_shell(),
    });
    let copy = render_completion_guidance_lines(plan, locale, &ctx.i18n);

    eprintln!();
    eprintln!("{}", copy.intro);

    match plan {
        CompletionGuidancePlan::OfferInteractive { shell } => {
            let confirmed = super::confirm_with_transcript(ctx, locale, &copy.prompt, true)?;
            if confirmed {
                cmd_completions_install(shell, ctx)?;
                let installed = ctx.i18n.resolve_with_args(
                    locale,
                    k::INIT_GLOBAL_COMPLETION_INSTALLED,
                    &[&shell.to_string()],
                );
                eprintln!("{installed}");
            } else {
                let declined = ctx.i18n.resolve(locale, k::INIT_GLOBAL_COMPLETION_DECLINED);
                eprintln!("{declined}");
                eprintln!("{}", copy.manual_hint);
            }
        }
        CompletionGuidancePlan::PromptExplicitShell
        | CompletionGuidancePlan::PrintManualHint { .. } => {
            eprintln!("{}", copy.manual_hint);
        }
    }
    Ok(())
}

struct CompletionInstallResult {
    shell: Shell,
    script_path: PathBuf,
    rc_path: Option<PathBuf>,
}

impl CompletionInstallResult {
    fn message(&self, ctx: &CommandContext) -> String {
        let locale = ctx.locale();
        let activation = activation_hint(self.shell, self.rc_path.as_deref(), ctx);
        let shell_name = self.shell.to_string();
        let script_path = display_path_human(&self.script_path);
        let installed = ctx.i18n.resolve_with_args(
            locale,
            k::COMPLETIONS_INSTALLED,
            &[&shell_name, &script_path],
        );
        format!("{installed}\n{activation}")
    }
}

/// Best-effort home abbreviation for a `std::path::Path`.
///
/// Mirrors `crate::path_display::display_path_for_humans` but adapted
/// to `std::path::Path`, which `clap_complete` returns. Falls back to
/// the lossy display when the path is not valid UTF-8.
fn display_path_human(path: &std::path::Path) -> String {
    if let Some(s) = path.to_str() {
        crate::path_display::display_path_for_humans(camino::Utf8Path::new(s))
    } else {
        path.display().to_string()
    }
}

fn activation_hint(
    shell: Shell,
    rc_path: Option<&std::path::Path>,
    ctx: &CommandContext,
) -> String {
    let locale = ctx.locale();
    let i18n = &ctx.i18n;
    match shell {
        Shell::Zsh => match rc_path {
            Some(rc_path) => {
                let path = display_path_human(rc_path);
                i18n.resolve_with_args(locale, k::COMPLETIONS_ACTIVATE_ZSH_RC, &[&path])
            }
            None => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_ZSH_NO_RC),
        },
        Shell::Bash => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_BASH),
        Shell::Fish => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_FISH),
        Shell::PowerShell => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_POWERSHELL),
        Shell::Elvish => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_ELVISH),
        _ => i18n.resolve(locale, k::COMPLETIONS_ACTIVATE_GENERIC),
    }
}

fn install_completions(
    shell: Shell,
    ctx: &CommandContext,
) -> Result<CompletionInstallResult, CliError> {
    let path = completion_install_path(shell)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            CliError::Runtime(format!(
                "failed to create completion directory '{}': {e}",
                parent.display()
            ))
        })?;
    }

    let mut buf = Vec::new();
    write_completions(shell, ctx.locale(), &ctx.i18n, &mut buf);
    fs::write(&path, buf).map_err(|e| {
        CliError::Runtime(format!(
            "failed to write completion script '{}': {e}",
            path.display()
        ))
    })?;

    let rc_path = match shell {
        Shell::Zsh => Some(ensure_zshrc_loads_workhive_completions()?),
        _ => None,
    };

    Ok(CompletionInstallResult {
        shell,
        script_path: path,
        rc_path,
    })
}

fn detect_shell() -> Result<Shell, CliError> {
    if let Some(shell) = env::var_os("SHELL")
        .and_then(|v| PathBuf::from(v).file_name().map(|name| name.to_owned()))
        .and_then(|name| name.into_string().ok())
    {
        let shell = shell
            .trim_start_matches('-')
            .rsplit_once('/')
            .map_or(shell.as_str(), |(_, name)| name);
        if shell.contains("bash") {
            return Ok(Shell::Bash);
        }
        if shell.contains("zsh") {
            return Ok(Shell::Zsh);
        }
        if shell.contains("fish") {
            return Ok(Shell::Fish);
        }
        if shell.contains("elvish") {
            return Ok(Shell::Elvish);
        }
        if shell.contains("pwsh") || shell.contains("powershell") {
            return Ok(Shell::PowerShell);
        }
    }

    Err(CliError::Runtime(
        "could not detect the current shell; run `workhive completion install <shell>`".to_owned(),
    ))
}

fn completion_install_path(shell: Shell) -> Result<PathBuf, CliError> {
    let base = PathBuf::from(resolve_global_config_dir()?.as_str()).join("completions");
    match shell {
        Shell::Bash => Ok(base.join("bash/workhive")),
        Shell::Zsh => Ok(base.join("zsh/_workhive")),
        Shell::Fish => Ok(base.join("fish/workhive.fish")),
        Shell::PowerShell => Ok(base.join("powershell/_workhive.ps1")),
        Shell::Elvish => Ok(base.join("elvish/workhive-completions.elv")),
        _ => Err(CliError::Runtime(format!(
            "completion install is not implemented for {shell}"
        ))),
    }
}

fn ensure_zshrc_loads_workhive_completions() -> Result<PathBuf, CliError> {
    let rc_path = home_dir()?.join(".zshrc");
    let completion_dir =
        PathBuf::from(resolve_global_config_dir()?.as_str()).join("completions/zsh");
    let block = format!(
        "{ZSH_RC_BEGIN}\nfpath=({completion_dir} $fpath)\nautoload -Uz compinit\ncompinit\n{ZSH_RC_END}\n",
        completion_dir = completion_dir.display()
    );
    let existing = match fs::read_to_string(&rc_path) {
        Ok(content) => content,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(CliError::Runtime(format!(
                "failed to read zsh config '{}': {e}",
                rc_path.display()
            )));
        }
    };

    if existing.contains(ZSH_RC_BEGIN) && existing.contains(ZSH_RC_END) {
        return Ok(rc_path);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    if !updated.is_empty() {
        updated.push('\n');
    }
    updated.push_str(&block);

    fs::write(&rc_path, updated).map_err(|e| {
        CliError::Runtime(format!(
            "failed to update zsh config '{}': {e}",
            rc_path.display()
        ))
    })?;
    Ok(rc_path)
}

fn home_dir() -> Result<PathBuf, CliError> {
    env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Runtime("HOME is not set; cannot install completions".to_owned()))
}

#[cfg(test)]
#[path = "completions_tests.rs"]
mod tests;

use super::*;
use workhive_infra::i18n::StaticI18n;

fn render(shell: Shell) -> String {
    render_with_locale(shell, Locale::En)
}

fn render_with_locale(shell: Shell, locale: Locale) -> String {
    let i18n = StaticI18n::new();
    let mut buf = Vec::new();
    write_completions(shell, locale, &i18n, &mut buf);
    String::from_utf8(buf).expect("completion scripts are utf-8")
}

fn bash_resource_block<'a>(script: &'a str, resource: &str) -> &'a str {
    let marker = format!("        workhive__subcmd__{resource})");
    let start = script
        .find(&marker)
        .unwrap_or_else(|| panic!("bash completion missing block for {resource}"));
    let rest = &script[start..];
    let end = rest
        .find("        ;;")
        .unwrap_or_else(|| panic!("bash completion block for {resource} has no terminator"));
    &rest[..end]
}

fn assert_bash_resource_candidates(script: &str, resource: &str, expected: &[&str]) {
    let block = bash_resource_block(script, resource);
    let opts_line = block
        .lines()
        .find(|line| line.trim_start().starts_with("opts="))
        .unwrap_or_else(|| panic!("bash completion block for {resource} has no opts line"));
    for candidate in expected {
        assert!(
            opts_line
                .split('"')
                .nth(1)
                .unwrap_or("")
                .split_whitespace()
                .any(|token| token == *candidate),
            "bash completion for {resource} missing candidate `{candidate}` in opts line: {opts_line}"
        );
    }
}

fn assert_fish_resource_candidate(script: &str, resource: &str, candidate: &str) {
    let resource_guard = format!("__fish_workhive_using_subcommand {resource};");
    let candidate_arg = format!("-a \"{candidate}\"");
    assert!(
        script
            .lines()
            .any(|line| line.contains(&resource_guard) && line.contains(&candidate_arg)),
        "fish completion for {resource} missing candidate `{candidate}`"
    );
}

fn zsh_resource_commands_block<'a>(script: &'a str, resource: &str) -> &'a str {
    let marker = format!("_workhive__subcmd__{resource}_commands() {{");
    let start = script
        .find(&marker)
        .unwrap_or_else(|| panic!("zsh completion missing commands block for {resource}"));
    let rest = &script[start..];
    let end = rest
        .find("    _describe")
        .unwrap_or_else(|| panic!("zsh completion block for {resource} has no _describe"));
    &rest[..end]
}

fn assert_zsh_resource_candidate(script: &str, resource: &str, candidate: &str) {
    let block = zsh_resource_commands_block(script, resource);
    let prefix = format!("'{candidate}:");
    assert!(
        block
            .lines()
            .any(|line| line.trim_start().starts_with(&prefix)),
        "zsh completion for {resource} missing candidate `{candidate}` in block:\n{block}"
    );
}

#[test]
fn bash_script_contains_public_commands_and_object_aliases() {
    let script = render(Shell::Bash);
    // Top-level system + resource commands must be present.
    for needle in [
        "init",
        "config",
        "doctor",
        "completion",
        "project",
        "workspace",
        "session",
        "pane",
    ] {
        assert!(
            script.contains(needle),
            "bash completion missing token '{needle}'",
        );
    }
    assert!(
        !script.contains("completions"),
        "bash completion should not expose removed `completions` command",
    );
    // `ls` is an intentional non-visible alias under each resource —
    // it stays parseable but no longer appears in completion output
    // because we hide alias chrome from `--help`. The visible
    // candidate completion stays `list`.
    for resource in ["project", "workspace", "session", "pane"] {
        assert_bash_resource_candidates(&script, resource, &["list"]);
    }
}

#[test]
fn zsh_script_does_not_expose_internal_command() {
    let script = render(Shell::Zsh);
    // The static clap command tree must not advertise `internal`.
    // Our dynamic hook itself does mention `internal completion
    // candidates` (intentional — that's the read-only query path),
    // but it does NOT advertise `internal` as a user-completion
    // candidate. Strip the dynamic hook block before checking.
    let static_only = script
        .split("Workhive dynamic completion hook")
        .next()
        .expect("split returns non-empty prefix");
    assert!(
        !static_only.contains("internal"),
        "zsh static completion must not include hidden `internal` command",
    );
}

#[test]
fn fish_script_does_not_expose_internal_command() {
    let script = render(Shell::Fish);
    let static_only = script
        .split("Workhive dynamic completion hook")
        .next()
        .expect("split returns non-empty prefix");
    assert!(
        !static_only.contains("internal"),
        "fish static completion must not include hidden `internal` command",
    );
}

#[test]
fn supports_all_documented_shells() {
    let i18n = StaticI18n::new();
    for shell in [
        Shell::Bash,
        Shell::Zsh,
        Shell::Fish,
        Shell::PowerShell,
        Shell::Elvish,
    ] {
        let mut buf = Vec::new();
        write_completions(shell, Locale::En, &i18n, &mut buf);
        assert!(
            !buf.is_empty(),
            "completion output must not be empty for {shell:?}",
        );
    }
}

#[test]
fn bash_script_preserves_view_value_enum() {
    // `workspace list --view <Tab>` must surface the `ListView`
    // variants from cli.rs. clap_complete encodes value enum
    // variants directly into the generated script for bash.
    let script = render(Shell::Bash);
    for variant in ["auto", "table", "compact", "json"] {
        assert!(
            script.contains(variant),
            "bash completion lost --view value enum '{variant}'",
        );
    }
}

#[test]
fn zsh_script_object_list_completion() {
    let script = render(Shell::Zsh);
    // `ls` is an intentional non-visible alias under each resource;
    // it is still parseable as a command but no longer surfaces in
    // shell completion because we hide alias chrome from `--help`.
    for resource in ["project", "workspace", "session", "pane"] {
        assert_zsh_resource_candidate(&script, resource, "list");
    }
}

#[test]
fn fish_script_object_list_completion() {
    let script = render(Shell::Fish);
    // Same rationale as the zsh test above: `ls` is parseable but
    // hidden from completion.
    for resource in ["project", "workspace", "session", "pane"] {
        assert_fish_resource_candidate(&script, resource, "list");
    }
}

#[test]
fn fish_script_preserves_public_long_flags() {
    // Fish encodes long flags via `complete -l <name>` (without the
    // leading `--`). Each public long flag must appear with this
    // form so fish offers it during tab completion.
    let script = render(Shell::Fish);
    for flag in ["no-attach", "force", "verbose", "help", "version"] {
        assert!(
            script.contains(&format!("-l {flag}")),
            "fish completion lost public long flag '--{flag}'",
        );
    }
}

#[test]
fn bash_script_preserves_short_force_flag() {
    let script = render(Shell::Bash);
    // -f for --force is a deliberate keep from the 2026-04-30 cleanup.
    assert!(
        script.contains("-f"),
        "bash completion lost -f short flag for --force",
    );
}

#[test]
fn no_internal_subcommand_in_any_shell_static_block() {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let script = render(shell);
        // The dynamic hook block legitimately references
        // `internal completion candidates` because that IS the
        // read-only query path. Restrict the check to the static
        // clap tree by splitting on the dynamic hook marker.
        let static_only = script
            .split("Workhive dynamic completion hook")
            .next()
            .expect("split returns prefix");
        assert!(
            !static_only.contains(" internal ")
                && !static_only.contains(" internal\n")
                && !static_only.contains("'internal'")
                && !static_only.contains("\"internal\""),
            "{shell:?} static completion exposed hidden `internal` token",
        );
    }
}

// ── Dynamic completion hook contract ──

#[test]
fn bash_dynamic_hook_calls_internal_completion_candidates() {
    let script = render(Shell::Bash);
    assert!(
        script.contains("workhive internal completion candidates"),
        "bash hook must delegate to `workhive internal completion candidates`",
    );
    assert!(
        script.contains("complete -F _workhive_dynamic"),
        "bash hook must register dynamic dispatcher for `workhive`",
    );
}

#[test]
fn bash_dynamic_hook_routes_workspace_and_pane_kinds() {
    let script = render(Shell::Bash);
    // We don't constrain the exact shell syntax, but the hook MUST
    // mention every dynamic-completion target.
    for resource in ["workspace", "pane"] {
        assert!(
            script.contains(resource),
            "bash hook missing dynamic kind '{resource}'",
        );
    }
    for action in ["open", "attach", "remove", "send"] {
        assert!(
            script.contains(action),
            "bash hook missing action '{action}' for dynamic completion",
        );
    }
}

#[test]
fn zsh_dynamic_hook_calls_internal_completion_candidates() {
    let script = render(Shell::Zsh);
    assert!(
        script.contains("workhive internal completion candidates"),
        "zsh hook must delegate to `workhive internal completion candidates`",
    );
    assert!(
        script.contains("compdef _workhive_dynamic_dispatch workhive"),
        "zsh hook must reroute completion through the dynamic dispatcher",
    );
}

#[test]
fn fish_dynamic_hook_calls_internal_completion_candidates() {
    let script = render(Shell::Fish);
    assert!(
        script.contains("workhive internal completion candidates workspace"),
        "fish hook must request workspace candidates",
    );
    assert!(
        script.contains("workhive internal completion candidates pane"),
        "fish hook must request pane candidates",
    );
    // Fish must register dynamic completion for ALL three workspace
    // commands.
    for action in ["open", "attach", "remove"] {
        assert!(
            script.contains(&format!("__fish_seen_subcommand_from {action}")),
            "fish hook missing dynamic completion for `workspace {action}`",
        );
    }
}

/// English phrases that must NOT appear in zh-CN completion scripts.
///
/// Command descriptions in shell completions must follow the user's
/// `language` preference, not clap's hard-coded English `about`.
const FORBIDDEN_EN_DESCRIPTIONS: &[&str] = &[
    "Attach to a workspace",
    "Close the current workspace",
    "Destroy all Workhive resources",
    "Print this message or the help of the given subcommand(s)",
];

#[test]
fn zsh_completion_script_zh_cn_contains_no_english_descriptions() {
    let script = render_with_locale(Shell::Zsh, Locale::ZhCn);
    for phrase in FORBIDDEN_EN_DESCRIPTIONS {
        assert!(
            !script.contains(phrase),
            "zsh zh-CN completion still contains English description: {phrase}\n--- script ---\n{script}",
        );
    }
}

#[test]
fn bash_completion_script_zh_cn_contains_no_english_descriptions() {
    let script = render_with_locale(Shell::Bash, Locale::ZhCn);
    for phrase in FORBIDDEN_EN_DESCRIPTIONS {
        assert!(
            !script.contains(phrase),
            "bash zh-CN completion still contains English description: {phrase}\n--- script ---\n{script}",
        );
    }
}

#[test]
fn fish_completion_script_zh_cn_contains_no_english_descriptions() {
    let script = render_with_locale(Shell::Fish, Locale::ZhCn);
    for phrase in FORBIDDEN_EN_DESCRIPTIONS {
        assert!(
            !script.contains(phrase),
            "fish zh-CN completion still contains English description: {phrase}\n--- script ---\n{script}",
        );
    }
}

#[test]
fn zsh_zh_cn_completion_preserves_dynamic_hook_and_hidden_filter() {
    let script = render_with_locale(Shell::Zsh, Locale::ZhCn);
    assert!(
        script.contains("compdef _workhive_dynamic_dispatch workhive"),
        "zsh zh-CN completion lost dynamic dispatcher",
    );
    let static_only = script
        .split("Workhive dynamic completion hook")
        .next()
        .expect("split returns prefix");
    assert!(
        !static_only.contains("internal"),
        "zsh zh-CN completion exposed hidden internal command",
    );
}

#[test]
fn bash_zh_cn_completion_preserves_dynamic_hook_and_hidden_filter() {
    let script = render_with_locale(Shell::Bash, Locale::ZhCn);
    assert!(
        script.contains("complete -F _workhive_dynamic"),
        "bash zh-CN completion lost dynamic dispatcher",
    );
    let static_only = script
        .split("Workhive dynamic completion hook")
        .next()
        .expect("split returns prefix");
    assert!(
        !static_only.contains(" internal ") && !static_only.contains("'internal'"),
        "bash zh-CN completion exposed hidden internal command",
    );
}

#[test]
fn powershell_and_elvish_have_no_dynamic_hook() {
    // We intentionally do not ship dynamic hooks for these shells
    // yet — the static script alone is the documented surface.
    for shell in [Shell::PowerShell, Shell::Elvish] {
        let script = render(shell);
        assert!(
            !script.contains("Workhive dynamic completion hook"),
            "{shell:?} should not include the dynamic completion hook",
        );
    }
}

// ── Goal 3: completion guidance after global init ────────────
//
// The wizard MUST explain `workhive completion install`, ask before
// doing anything that could touch the user's shell startup files,
// and surface a clear manual command when prompting is not safe.

#[test]
fn plan_completion_guidance_interactive_with_shell_offers_install() {
    let plan = plan_completion_guidance(CompletionGuidanceInput {
        interactive: true,
        detected_shell: Some(Shell::Zsh),
    });
    assert_eq!(
        plan,
        CompletionGuidancePlan::OfferInteractive { shell: Shell::Zsh },
        "TTY + known shell must produce an OfferInteractive plan so \
             the wizard can ask Yes/No before touching shell rc files"
    );
}

#[test]
fn plan_completion_guidance_interactive_unknown_shell_prompts_explicit_form() {
    let plan = plan_completion_guidance(CompletionGuidanceInput {
        interactive: true,
        detected_shell: None,
    });
    assert_eq!(
        plan,
        CompletionGuidancePlan::PromptExplicitShell,
        "TTY + unknown shell must NOT silently guess; the wizard \
             surfaces the explicit `workhive completion install <shell>` form"
    );
}

#[test]
fn plan_completion_guidance_non_tty_only_prints_manual_hint() {
    for shell in [None, Some(Shell::Bash), Some(Shell::Fish)] {
        let plan = plan_completion_guidance(CompletionGuidanceInput {
            interactive: false,
            detected_shell: shell,
        });
        assert_eq!(
            plan,
            CompletionGuidancePlan::PrintManualHint { shell },
            "non-TTY runs MUST never prompt or install — they only \
                 print the manual command (shell={shell:?})"
        );
    }
}

#[test]
fn render_offer_interactive_includes_intro_prompt_and_manual_hint() {
    let i18n = StaticI18n::new();
    let copy = render_completion_guidance_lines(
        CompletionGuidancePlan::OfferInteractive { shell: Shell::Zsh },
        Locale::En,
        &i18n,
    );
    assert!(
        copy.intro.contains("workhive completion install"),
        "intro must explain the command users would be opting into: {:?}",
        copy.intro
    );
    assert!(
        !copy.prompt.is_empty(),
        "OfferInteractive plan must produce a non-empty Yes/No prompt"
    );
    assert!(
        copy.manual_hint.contains("workhive completion install"),
        "manual hint must always include the literal command: {:?}",
        copy.manual_hint
    );
    assert!(
        copy.manual_hint.to_lowercase().contains("zsh"),
        "manual hint with detected shell must include the shell name: {:?}",
        copy.manual_hint
    );
}

#[test]
fn render_prompt_explicit_shell_includes_shell_argument_form() {
    let i18n = StaticI18n::new();
    let copy = render_completion_guidance_lines(
        CompletionGuidancePlan::PromptExplicitShell,
        Locale::En,
        &i18n,
    );
    assert!(
        copy.manual_hint
            .contains("workhive completion install <shell>"),
        "shell-not-detected hint must surface the explicit \
             `workhive completion install <shell>` form: {:?}",
        copy.manual_hint
    );
    assert!(
        copy.prompt.is_empty(),
        "PromptExplicitShell plan must NOT carry a Yes/No prompt — \
             the user picks a shell explicitly"
    );
}

#[test]
fn render_print_manual_hint_includes_command() {
    let i18n = StaticI18n::new();
    let copy_unknown = render_completion_guidance_lines(
        CompletionGuidancePlan::PrintManualHint { shell: None },
        Locale::En,
        &i18n,
    );
    assert!(
        copy_unknown
            .manual_hint
            .contains("workhive completion install"),
        "manual hint must always include the install command: {:?}",
        copy_unknown.manual_hint
    );
    assert!(
        copy_unknown.prompt.is_empty(),
        "non-interactive plan must never produce a Yes/No prompt"
    );
    let copy_with_shell = render_completion_guidance_lines(
        CompletionGuidancePlan::PrintManualHint {
            shell: Some(Shell::Bash),
        },
        Locale::En,
        &i18n,
    );
    assert!(
        copy_with_shell.manual_hint.to_lowercase().contains("bash"),
        "manual hint with detected shell must mention it: {:?}",
        copy_with_shell.manual_hint
    );
}

/// Localization regression: each guidance copy field must be
/// available in BOTH locales. A missing key falls through to the
/// raw key string, so we assert the resolved text is not the
/// literal `init.global_completion.intro` token.
#[test]
fn render_guidance_lines_are_localized_in_both_locales() {
    let i18n = StaticI18n::new();
    for locale in [Locale::En, Locale::ZhCn] {
        let copy = render_completion_guidance_lines(
            CompletionGuidancePlan::OfferInteractive { shell: Shell::Fish },
            locale,
            &i18n,
        );
        assert_ne!(
            copy.intro, "init.global_completion.intro",
            "intro must resolve to localized copy for {locale:?}"
        );
        assert_ne!(
            copy.prompt, "init.global_completion.prompt",
            "prompt must resolve to localized copy for {locale:?}"
        );
        assert_ne!(
            copy.manual_hint, "init.global_completion.manual_hint_with_shell",
            "manual hint must resolve to localized copy for {locale:?}"
        );
    }
}

#[test]
fn dynamic_hook_does_not_call_git_or_tmux_directly() {
    // Dynamic completion shell glue
    // must not embed git/tmux/project business logic. The hook
    // must only invoke `workhive internal completion candidates`.
    // We scan for *invocations*, not comment substrings.
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let script = render(shell);
        let mut parts = script.split("Workhive dynamic completion hook");
        let _ = parts.next();
        let hook = parts.next().unwrap_or("");
        // Strip comment lines so a comment that mentions "git" or
        // "tmux" by name does not trip the contract test. Bash,
        // zsh and fish all use `#` to start a comment line.
        let executable: String = hook
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "git rev-parse",
            "git status",
            "git branch",
            "tmux list-sessions",
            "tmux list-panes",
            "tmux list-windows",
            "tmux display-message",
        ] {
            assert!(
                !executable.contains(forbidden),
                "{shell:?} dynamic hook embeds forbidden invocation '{forbidden}'",
            );
        }
    }
}

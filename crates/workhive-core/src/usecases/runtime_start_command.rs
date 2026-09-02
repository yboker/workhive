//! Shared classifier for tmux `pane_start_command` strings.
//!
//! Both runtime adapters (`workhive-infra::runtime`,
//! `workhive-infra::snapshot_runtime`) and the snapshot capture
//! pipeline (`workhive-core::usecases::snapshot_capture`) need to
//! map a tmux start-command string back into typed launch
//! information. This module is the single source of truth for that
//! mapping so the three call sites cannot drift from each other.
//!
//! ## Why this matters
//!
//! The Workhive supervisor wraps tool launches as
//! `'/path/to/workhive' 'internal' 'pane-supervisor' '--tool' 'claude'
//! '--' 'claude' '--no-banner'`. Without a shared classifier each
//! call site rolled its own ad-hoc parsing and ended up persisting
//! the wrapper as a user `InlineCommand`. The next `workspace open`
//! then replayed that "user" command via visible `send_input`.
//!
//! The classifier returns a typed [`StartCommandClassification`].
//! Workhive's internal supervisor wrapper is **never** classified
//! as `InlineCommand`: when the `--tool <name>` flag is parsable the
//! wrapper resolves to `Tool { name }`; otherwise it resolves to
//! `InternalSupervisorWrapper { tool: None }` and the caller treats
//! that as non-restorable rather than as a user-visible command.

use crate::domain::{CommandSpec, PaneLaunchOrigin, PaneLaunchRef, ToolName};

/// Typed classification of a runtime start-command string.
///
/// All three callers (two runtime adapters + snapshot capture) map
/// this enum to their domain shapes; the classifier itself never
/// touches `PaneLaunchIntent` so the core stays free of the
/// snapshot-format vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartCommandClassification {
    /// No usable command observed (None / blank string).
    Empty,
    /// Looks like a known shell (`zsh`, `bash`, ...). Carries no
    /// durable user intent; promoting it to `EmptyShell` would
    /// silently overwrite a tool that just exited.
    Shell,
    /// Catalog tool by leading basename (e.g. `claude`, `codex`).
    Tool { name: ToolName },
    /// Workhive's own pane-supervisor wrapper.
    ///
    /// `tool` is `Some` when `--tool <name>` parsed cleanly, `None`
    /// for malformed wrappers (missing/empty `--tool`).
    InternalSupervisorWrapper { tool: Option<ToolName> },
    /// User-launched command that is neither shell, tool, nor
    /// supervisor wrapper.
    InlineCommand { spec: CommandSpec },
}

/// Classify a raw tmux `pane_start_command` string.
///
/// `raw` may be `None` (the runtime did not surface a start command)
/// or contain a Workhive supervisor wrapper, a plain tool basename,
/// a shell-quoted argv, or an inline user command. Tokenization
/// understands single-quoted tokens with the standard `'\''` escape
/// idiom emitted by [`crate::domain::CommandSpec`] and the tool
/// instrumentation adapter, so the classifier consumes the same
/// shape tmux produces for `respawn-pane -k <quoted argv>`.
pub fn classify_start_command(raw: Option<&str>) -> StartCommandClassification {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return StartCommandClassification::Empty;
    };

    // A whole command captured wrapped in matching outer double quotes
    // (e.g. `"cursor-agent --yolo"`) must be tokenized by its inner
    // argv. Without this normalization the tokenizer splits it into
    // `"cursor-agent` + `--yolo"` and the leading quote prevents the
    // basename from matching a catalog tool. Argument-level quoting
    // such as `python -c "print('hi')"` does not begin with a double
    // quote and is therefore left untouched.
    let normalized = strip_matching_outer_double_quotes(raw);

    let tokens = tokenize_start_command(normalized);
    if tokens.is_empty() {
        return StartCommandClassification::Empty;
    }

    let head_basename = basename(&tokens[0]);

    // Workhive supervisor wrapper detection. The shape pinned by
    // `workhive-infra::tool_launch_instrumentation::build_supervisor_argv`
    // is `<workhive> internal pane-supervisor ...`. We deliberately
    // require the literal `internal pane-supervisor` pair so an
    // accidentally-named user binary called `workhive` cannot
    // shadow this branch.
    if head_basename == "workhive" || head_basename == "wh" {
        let is_supervisor_wrapper = tokens.get(1).map(String::as_str) == Some("internal")
            && tokens.get(2).map(String::as_str) == Some("pane-supervisor");
        if is_supervisor_wrapper {
            return StartCommandClassification::InternalSupervisorWrapper {
                tool: parse_tool_flag(&tokens),
            };
        }
        // Workhive CLI invoked for something else (e.g. `workhive
        // status`). Treat as inline so we surface it truthfully
        // rather than silently labelling it as a tool.
        return StartCommandClassification::InlineCommand {
            spec: CommandSpec::new(tokens[0].clone(), tokens.iter().skip(1).cloned()),
        };
    }

    if is_likely_shell(&head_basename) {
        return StartCommandClassification::Shell;
    }

    // Classify by exact canonical basename only. `is_known_tool`
    // intentionally does NOT carry an alias table: a tmux
    // `pane_start_command` like `agent --yolo` is too low-confidence
    // by itself to claim the user's Cursor Agent binary, because
    // `agent` is a generic executable name that may be unrelated on
    // other machines or projects. Alias resolution is performed by
    // higher-confidence sources — the effective `ToolCatalog`
    // (built by `global_tool_catalog_config_for_environment`) and
    // the live OS process observer feeding `pane_observation` —
    // both of which carry catalog metadata declaring aliases such as
    // `agent` through explicit provider configuration.
    if is_known_tool(&head_basename.to_lowercase())
        && let Ok(name) = ToolName::new(head_basename.to_lowercase())
    {
        return StartCommandClassification::Tool { name };
    }

    // Anything else: persist the user's command verbatim. We
    // preserve the *raw* program token (path or basename) so the
    // round-tripped record reflects exactly what tmux observed.
    StartCommandClassification::InlineCommand {
        spec: CommandSpec::new(tokens[0].clone(), tokens.iter().skip(1).cloned()),
    }
}

/// Classify an already-tokenized `InlineCommand { program, args }`
/// shape persisted in a snapshot.
///
/// ## Why a separate entry point
///
/// [`classify_start_command`] consumes a single raw
/// `pane_start_command` string from tmux and re-tokenizes it with
/// the shell-quote-aware [`tokenize_start_command`]. Snapshots do
/// not store raw start-command strings; they store
/// `PaneLaunchIntent::InlineCommand { program, args }` where
/// `program` and `args` are already split slots. We must NOT
/// re-tokenize those slots — doing so would split tokens that
/// legitimately contain whitespace (e.g. an inline command argv
/// like `python -c "print('hi')"`).
///
/// ## Legacy quoted wrapper shapes
///
/// Legacy snapshots may persist the Workhive supervisor
/// wrapper with a path-qualified, possibly shell-quoted `program`
/// token, e.g.:
///
/// * `program = "workhive"`
/// * `program = "/Users/.../target/release/workhive"`
/// * `program = "'/Users/.../target/release/workhive'"` (older
///   capture path that did not strip tmux's outer single quotes)
/// * `program = "\"'/Users/.../target/release/workhive'\""` (older
///   capture path with both layers preserved)
///
/// All four shapes are semantically the Workhive internal
/// supervisor wrapper and must be neutralized identically.
/// Detection is done by **executable basename after stripping
/// outer shell quotes**, exactly as
/// [`classify_start_command`] does for raw start-command strings.
/// Identification thus stays anchored to the same "workhive
/// internal pane-supervisor" signature in both paths.
///
/// ## Identity preservation
///
/// Real user commands like `workhive status` or
/// `/usr/local/bin/workhive status` are NOT wrappers and are
/// preserved as `InlineCommand` with the original (un-stripped)
/// `program` and `args` so persistence round-trips exactly.
///
/// ## Tool resolution
///
/// When the wrapper signature matches we run [`parse_tool_flag`]
/// over the **quote-stripped** args copy, mirroring the
/// raw-string classifier. This recovers `--tool codex` even when
/// the legacy capture path persisted `args = ["'--tool'",
/// "'codex'", ...]`.
pub fn classify_inline_command(program: &str, args: &[String]) -> StartCommandClassification {
    // A known-tool command stored as a whole command wrapped in
    // matching outer double quotes and then whitespace-split across the
    // program/arg slots — e.g. `program = "\"cursor-agent"`,
    // `args = ["--yolo\""]` — is tokenization pollution, not a
    // user-authored inline command: replaying it raw makes the shell
    // search for an executable literally named `"cursor-agent`. We only
    // recover it when the de-quoted leading token is a canonical
    // catalog tool basename, keeping confidence high and leaving real
    // inline commands untouched.
    if let Some(name) = recover_quote_split_tool(program, args) {
        return StartCommandClassification::Tool { name };
    }

    let program_unquoted = strip_outer_shell_quotes(program);
    let head_basename = basename(program_unquoted).to_ascii_lowercase();

    let normalized_args: Vec<String> = args
        .iter()
        .map(|arg| strip_outer_shell_quotes(arg).to_owned())
        .collect();

    if head_basename == "workhive" || head_basename == "wh" {
        let is_supervisor_wrapper = normalized_args.first().map(String::as_str) == Some("internal")
            && normalized_args.get(1).map(String::as_str) == Some("pane-supervisor");
        if is_supervisor_wrapper {
            return StartCommandClassification::InternalSupervisorWrapper {
                tool: parse_tool_flag(&normalized_args),
            };
        }
        // Real `workhive <subcommand>` invocation. Preserve the
        // original `program`/`args` slots verbatim so persistence
        // semantics do not silently mutate. The runtime-string
        // classifier behaves the same way for `workhive status`.
        return StartCommandClassification::InlineCommand {
            spec: CommandSpec::new(program.to_owned(), args.iter().cloned()),
        };
    }

    // Non-workhive program: this is a user-chosen inline command.
    // We deliberately do NOT classify as Shell/Tool here; those
    // categories are derived from live `pane_start_command`
    // observations, not from snapshot-persisted `InlineCommand`
    // slots that the user explicitly chose to record.
    StartCommandClassification::InlineCommand {
        spec: CommandSpec::new(program.to_owned(), args.iter().cloned()),
    }
}

/// Strip a single layer of matching outer shell quotes (`'..'` or
/// `"..."`), repeatedly, until the value is unquoted.
///
/// Older capture paths could persist a `program` token wrapped in
/// either single quotes (tmux's quoting style) or both single and
/// double quotes (when one capture path quoted on top of another).
/// The loop normalizes any nesting depth observed in real
/// snapshots without expanding internal escapes — this is purely
/// outer-quote stripping for identification, never for command
/// construction.
fn strip_outer_shell_quotes(value: &str) -> &str {
    let mut current = value.trim();
    loop {
        let bytes = current.as_bytes();
        if bytes.len() < 2 {
            return current;
        }
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        let stripped = (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"');
        if !stripped {
            return current;
        }
        current = &current[1..current.len() - 1];
    }
}

/// Strip a single matching pair of outer double quotes that wraps an
/// entire command string.
///
/// Targets the quote-pollution shape where a whole tool command was
/// captured as `"cursor-agent --yolo"` (matching outer double quotes
/// around the full argv). Only a single outer layer is removed and
/// only when the inner content carries no further double quote, so
/// argument-level quoting such as `python -c "print('hi')"` — which
/// does not begin with a double quote — is never affected.
fn strip_matching_outer_double_quotes(value: &str) -> &str {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        let inner = trimmed[1..trimmed.len() - 1].trim();
        if !inner.contains('"') {
            return inner;
        }
    }
    trimmed
}

/// Recover a catalog tool from a persisted `InlineCommand` whose whole
/// command was wrapped in matching outer double quotes and then
/// scattered across the program/arg slots by a whitespace split.
///
/// The pollution shape always begins with a stray leading double quote
/// on the `program` slot and ends with the matching closing quote on
/// the final slot, e.g. `program = "\"cursor-agent"`,
/// `args = ["--yolo\""]`, or the single-slot form
/// `program = "\"cursor-agent --yolo\""` with no args. After repairing
/// the scattered quote layer the leading token is matched by canonical
/// basename; recovery only succeeds for known catalog tools so genuine
/// user inline commands fall through to verbatim preservation.
fn recover_quote_split_tool(program: &str, args: &[String]) -> Option<ToolName> {
    let program_inner = program.trim().strip_prefix('"')?;

    let mut tokens: Vec<String> = Vec::with_capacity(args.len() + 1);
    tokens.push(program_inner.to_owned());
    tokens.extend(args.iter().cloned());

    let last = tokens.last_mut()?;
    *last = last.trim_end().strip_suffix('"')?.to_owned();

    let candidate = tokens.join(" ");
    let repaired = tokenize_start_command(&candidate);
    let head_basename = basename(repaired.first()?).to_ascii_lowercase();
    if is_known_tool(&head_basename) {
        return ToolName::new(head_basename).ok();
    }
    None
}

/// Convert a [`StartCommandClassification`] into the runtime-adapter
/// shape ([`PaneLaunchOrigin`] = [`PaneLaunchRef`]).
///
/// Guarantees:
/// - `Empty` / `Shell` -> `None` (caller treats as "no opinion")
/// - `Tool` -> `Some(Tool { name })`
/// - `InternalSupervisorWrapper { Some(tool) }` -> `Some(Tool { tool })`
/// - `InternalSupervisorWrapper { None }` -> `None`
///   (NEVER promoted to `InlineCommand`; that was the bug)
/// - `InlineCommand` -> `Some(InlineCommand { spec })`
pub fn classification_to_launch_origin(
    classification: StartCommandClassification,
) -> Option<PaneLaunchRef> {
    match classification {
        StartCommandClassification::Empty | StartCommandClassification::Shell => None,
        StartCommandClassification::Tool { name } => Some(PaneLaunchOrigin::Tool { name }),
        StartCommandClassification::InternalSupervisorWrapper { tool } => {
            tool.map(|name| PaneLaunchOrigin::Tool { name })
        }
        StartCommandClassification::InlineCommand { spec } => {
            Some(PaneLaunchOrigin::InlineCommand { spec })
        }
    }
}

/// Strip directory components from a token (`/usr/local/bin/claude`
/// -> `claude`). Returns the input unchanged when there is no
/// separator.
fn basename(token: &str) -> String {
    token.rsplit('/').next().unwrap_or(token).to_owned()
}

/// Parse the value of `--tool <name>` (or `--tool=<name>`) out of an
/// argv-tokenized supervisor wrapper. Returns `None` when the flag is
/// absent, terminated by `--`, or the value is empty / non-tool-shaped.
///
/// Close/open closed-loop: `clap`-style argument parsers (and
/// users typing the wrapper by hand) accept both `--tool codex` and
/// `--tool=codex`. The classifier must recognize both spellings so
/// the snapshot capture path round-trips the wrapper back to
/// `ToolRef` in either case.
fn parse_tool_flag(tokens: &[String]) -> Option<ToolName> {
    for (idx, token) in tokens.iter().enumerate() {
        if token == "--" {
            return None;
        }
        if token == "--tool" {
            let value = tokens.get(idx + 1)?;
            if value == "--" || value.is_empty() {
                return None;
            }
            return ToolName::new(value.to_lowercase()).ok();
        }
        if let Some(value) = token.strip_prefix("--tool=") {
            if value.is_empty() {
                return None;
            }
            return ToolName::new(value.to_lowercase()).ok();
        }
    }
    None
}

/// Tokenize a tmux `pane_start_command` string.
///
/// tmux emits start commands using single-quoted shell escaping
/// (`'foo' 'bar' '--tool' 'claude'`) when the command was installed
/// via `respawn-pane -k <quoted argv>`. Plain commands (`claude`,
/// `python -m http.server`) are emitted as space-separated tokens.
/// We accept both forms.
///
/// Inside a single-quoted run, every character is literal until the
/// closing `'`. Outside any quote, `\<ch>` introduces a literal
/// `<ch>` (this is what makes the shell idiom `'\''` round-trip
/// embedded apostrophes: close-quote, escaped quote, reopen-quote).
///
/// Note: this is **parsing for classification only**, not building
/// a shell command. The external-command policy forbids `shlex` for command
/// construction; this hand-rolled tokenizer is purely receive-side.
fn tokenize_start_command(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    // `had_token` separates "no token yet" from "token started but
    // currently empty" (e.g. an empty `''` quoted argument). The
    // shell-escape idiom relies on the latter being preserved across
    // the close/reopen cycle.
    let mut had_token = false;
    let mut in_single_quote = false;
    let mut chars = raw.chars();

    while let Some(ch) = chars.next() {
        if in_single_quote {
            if ch == '\'' {
                in_single_quote = false;
                continue;
            }
            cur.push(ch);
            had_token = true;
            continue;
        }

        if ch == '\'' {
            in_single_quote = true;
            had_token = true;
            continue;
        }
        if ch == '\\' {
            // Outside a single-quoted run, a backslash consumes the
            // next character literally. This is exactly what the
            // shell does for `\'`, the `'\''` idiom's middle two
            // chars.
            if let Some(esc) = chars.next() {
                cur.push(esc);
                had_token = true;
            }
            continue;
        }
        if ch.is_whitespace() {
            if had_token {
                out.push(std::mem::take(&mut cur));
                had_token = false;
            }
            continue;
        }
        cur.push(ch);
        had_token = true;
    }

    if had_token {
        out.push(cur);
    }
    out
}

fn is_likely_shell(basename: &str) -> bool {
    matches!(
        basename,
        "zsh" | "bash" | "sh" | "fish" | "nushell" | "nu" | "dash" | "ksh" | "csh" | "tcsh" | "ash"
    )
}

/// Built-in canonical tool basenames recognized by the
/// pane_start_command classifier.
///
/// Membership is strict canonical-basename matching only. Aliases
/// like Cursor Agent's `agent` symlink are intentionally NOT listed
/// here: a tmux start-command string alone cannot prove an
/// arbitrary `agent` executable on PATH is the Cursor Agent binary.
/// Catalog-driven alias resolution lives in higher-confidence
/// sources (effective `ToolCatalog` exact `process_names`, live OS
/// process observation).
fn is_known_tool(basename: &str) -> bool {
    matches!(
        basename,
        "claude" | "codex" | "aider" | "cursor-agent" | "lazygit" | "gitui" | "yazi" | "broot"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supervisor_wrapper_for(tool: &str) -> String {
        // Mirrors the layout produced by
        // `workhive-infra::tool_launch_instrumentation::build_supervisor_argv`.
        format!(
            "'/usr/local/bin/workhive' 'internal' 'pane-supervisor' \
             '--runtime-root' '/proj/.workhive/runtime' \
             '--group' 'wh-srv' '--workspace' 'ws' '--unit' '%5' \
             '--tool' '{tool}' '--' '{tool}' '--no-banner'"
        )
    }

    // ── Plain, non-supervised inputs ──────────────────────────────

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(
            classify_start_command(None),
            StartCommandClassification::Empty
        );
        assert_eq!(
            classify_start_command(Some("")),
            StartCommandClassification::Empty
        );
        assert_eq!(
            classify_start_command(Some("   ")),
            StartCommandClassification::Empty
        );
    }

    #[test]
    fn plain_known_tool_is_tool() {
        match classify_start_command(Some("claude")) {
            StartCommandClassification::Tool { name } => assert_eq!(name.as_str(), "claude"),
            other => panic!("expected Tool, got {other:?}"),
        }
        match classify_start_command(Some("/usr/local/bin/codex")) {
            StartCommandClassification::Tool { name } => assert_eq!(name.as_str(), "codex"),
            other => panic!("expected Tool, got {other:?}"),
        }
    }

    /// `cursor-agent --yolo` must classify as the catalog tool
    /// `cursor-agent` (canonical basename match). Snapshot recovery
    /// and live `pane ls` agree on this case because the catalog
    /// also keys the entry under `cursor-agent`.
    #[test]
    fn cursor_agent_canonical_basename_classifies_as_tool() {
        for raw in ["cursor-agent --yolo", "/usr/local/bin/cursor-agent --yolo"] {
            match classify_start_command(Some(raw)) {
                StartCommandClassification::Tool { name } => assert_eq!(
                    name.as_str(),
                    "cursor-agent",
                    "canonical basename `{raw}` must resolve to cursor-agent"
                ),
                other => panic!("expected Tool(cursor-agent) for `{raw}`, got {other:?}"),
            }
        }
    }

    /// Bare `agent` MUST NOT be promoted to Cursor Agent by the
    /// tmux pane_start_command classifier on its own. `agent` is a
    /// generic basename whose identity depends on the user's
    /// catalog/PATH, not on a hardcoded alias table inside this
    /// classifier — that mapping is too low-confidence without
    /// reliable environment/catalog evidence.
    /// Catalog-driven recognition lives in
    /// `global_tool_catalog_config_for_environment` (which keeps
    /// `agent` as an exact `process_names` alias of the canonical
    /// `cursor-agent` entry) and in live OS process observation;
    /// the start-command classifier defers to those higher-confidence
    /// sources by preserving the user's raw command verbatim.
    #[test]
    fn bare_agent_basename_is_inline_command_not_cursor_agent() {
        for raw in ["agent --yolo", "/Users/me/.local/bin/agent --yolo"] {
            match classify_start_command(Some(raw)) {
                StartCommandClassification::InlineCommand { spec } => {
                    assert!(
                        spec.program().ends_with("agent"),
                        "bare `agent` basename must round-trip as the user's inline \
                         command, got program={:?}",
                        spec.program()
                    );
                    assert_eq!(
                        spec.args(),
                        &["--yolo".to_owned()],
                        "user-supplied args must survive verbatim, got {:?}",
                        spec.args()
                    );
                }
                other => panic!(
                    "expected InlineCommand for bare `{raw}` (start-command \
                     classifier must not hardcode `agent` -> `cursor-agent`), \
                     got {other:?}"
                ),
            }
        }
    }

    /// Lookalike basenames such as `cursor-agent-extra` must not be
    /// silently promoted to a known tool. They flow through as
    /// `InlineCommand` so the user sees their original command,
    /// matching the exact-basename policy used by the catalog
    /// matcher.
    #[test]
    fn lookalike_cursor_agent_names_are_inline_commands() {
        for raw in ["cursor-agent-extra", "my-cursor-agent", "agentx"] {
            match classify_start_command(Some(raw)) {
                StartCommandClassification::InlineCommand { spec } => {
                    assert_eq!(
                        spec.program(),
                        raw,
                        "lookalike `{raw}` must remain a user inline command"
                    );
                }
                other => panic!("expected InlineCommand for lookalike `{raw}`, got {other:?}"),
            }
        }
    }

    /// A whole-command string wrapped in matching outer double quotes
    /// (e.g. `"cursor-agent --yolo"`) classifies by its inner argv, so a
    /// known catalog tool resolves to the typed tool launch path instead
    /// of being split into `"cursor-agent` + `--yolo"` and treated as an
    /// unknown program.
    #[test]
    fn whole_command_double_quoted_known_tool_is_tool() {
        let cases = [
            ("\"cursor-agent --yolo\"", "cursor-agent"),
            (
                "\"codex --dangerously-bypass-approvals-and-sandbox\"",
                "codex",
            ),
            ("\"claude --dangerously-skip-permissions\"", "claude"),
            ("\"/usr/local/bin/cursor-agent --yolo\"", "cursor-agent"),
        ];
        for (raw, expected) in cases {
            match classify_start_command(Some(raw)) {
                StartCommandClassification::Tool { name } => assert_eq!(
                    name.as_str(),
                    expected,
                    "whole-command quoted `{raw}` must resolve to {expected}"
                ),
                other => panic!("expected Tool({expected}) for `{raw}`, got {other:?}"),
            }
        }
    }

    /// Argument-level quoting must NOT be treated as whole-command
    /// quoting. `python -c "print(...)"` does not begin with a double
    /// quote, so the outer-quote normalization leaves it as a genuine
    /// inline command.
    #[test]
    fn argument_only_quoting_remains_inline_command() {
        match classify_start_command(Some("python -c \"print('hi')\"")) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "python");
                assert_eq!(spec.args().first().map(String::as_str), Some("-c"));
            }
            other => panic!("expected InlineCommand for argument-only quoting, got {other:?}"),
        }
    }

    #[test]
    fn plain_shell_is_shell() {
        for shell in ["zsh", "bash", "sh", "fish", "/bin/zsh"] {
            assert!(
                matches!(
                    classify_start_command(Some(shell)),
                    StartCommandClassification::Shell
                ),
                "shell input {shell} did not classify as Shell"
            );
        }
    }

    #[test]
    fn unknown_user_command_is_inline() {
        match classify_start_command(Some("python -m http.server")) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "python");
                assert_eq!(spec.args(), &["-m".to_owned(), "http.server".to_owned()]);
            }
            other => panic!("expected InlineCommand, got {other:?}"),
        }
    }

    // ── Supervisor wrapper inputs ─────────────────────────────────

    #[test]
    fn supervisor_wrapper_with_tool_resolves_to_tool() {
        for tool in ["claude", "codex", "aider"] {
            let raw = supervisor_wrapper_for(tool);
            match classify_start_command(Some(&raw)) {
                StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                    assert_eq!(name.as_str(), tool, "tool basename for {raw}");
                }
                other => panic!("expected wrapper with tool {tool}, got {other:?}"),
            }
        }
    }

    #[test]
    fn supervisor_wrapper_with_equals_tool_flag_resolves_to_tool() {
        // Closed-loop: clap-style `--tool=codex` form must
        // parse identically to the space form so snapshot capture
        // recovers ToolRef regardless of how the wrapper argv is
        // emitted.
        let raw = "'/usr/local/bin/workhive' 'internal' 'pane-supervisor' \
                   '--workspace' 'wh@iid@proj@main' '--tool=codex' \
                   '--' 'codex'";
        match classify_start_command(Some(raw)) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool codex from --tool=codex, got {other:?}"),
        }
    }

    #[test]
    fn malformed_supervisor_wrapper_is_wrapper_with_no_tool() {
        // Missing `--tool` flag entirely.
        let raw = "'/usr/local/bin/workhive' 'internal' 'pane-supervisor' \
                   '--runtime-root' '/r' '--' 'claude'";
        let classification = classify_start_command(Some(raw));
        assert!(
            matches!(
                classification,
                StartCommandClassification::InternalSupervisorWrapper { tool: None }
            ),
            "missing --tool must produce wrapper with tool=None, got {classification:?}"
        );
    }

    #[test]
    fn malformed_supervisor_wrapper_with_empty_tool_is_no_tool() {
        let raw = "'/usr/local/bin/workhive' 'internal' 'pane-supervisor' '--tool' ''";
        let classification = classify_start_command(Some(raw));
        assert!(
            matches!(
                classification,
                StartCommandClassification::InternalSupervisorWrapper { tool: None }
            ),
            "empty --tool value must produce wrapper with tool=None, got {classification:?}"
        );
    }

    #[test]
    fn workhive_non_supervisor_invocation_is_inline_command() {
        // `workhive status` etc. — workhive CLI but not the
        // supervisor wrapper. The spec must NOT label this as a
        // tool, and must NOT label it as the supervisor wrapper.
        let raw = "'/usr/local/bin/workhive' 'status' '--json'";
        match classify_start_command(Some(raw)) {
            StartCommandClassification::InlineCommand { spec } => {
                // basename derivation is left to the caller; the
                // classifier preserves the raw token.
                assert_eq!(spec.program(), "/usr/local/bin/workhive");
                assert_eq!(spec.args(), &["status".to_owned(), "--json".to_owned()]);
            }
            other => panic!("expected InlineCommand for non-supervisor workhive, got {other:?}"),
        }
    }

    // ── Mapping to PaneLaunchOrigin ───────────────────────────────

    #[test]
    fn supervisor_wrapper_with_tool_maps_to_tool_origin() {
        let raw = supervisor_wrapper_for("claude");
        let origin = classification_to_launch_origin(classify_start_command(Some(&raw)));
        match origin {
            Some(PaneLaunchRef::Tool { name }) => assert_eq!(name.as_str(), "claude"),
            other => panic!("expected Tool origin, got {other:?}"),
        }
    }

    #[test]
    fn malformed_supervisor_wrapper_maps_to_none_origin() {
        let raw = "'/usr/local/bin/workhive' 'internal' 'pane-supervisor' '--' 'claude'";
        let origin = classification_to_launch_origin(classify_start_command(Some(raw)));
        assert!(
            origin.is_none(),
            "malformed wrapper must NEVER promote to a launch origin (would persist as \
             InlineCommand and replay via visible send_input next open)"
        );
    }

    #[test]
    fn shell_maps_to_none_origin() {
        let origin = classification_to_launch_origin(classify_start_command(Some("zsh")));
        assert!(origin.is_none());
    }

    #[test]
    fn empty_maps_to_none_origin() {
        let origin = classification_to_launch_origin(classify_start_command(None));
        assert!(origin.is_none());
    }

    #[test]
    fn plain_tool_maps_to_tool_origin() {
        let origin = classification_to_launch_origin(classify_start_command(Some("codex")));
        match origin {
            Some(PaneLaunchRef::Tool { name }) => assert_eq!(name.as_str(), "codex"),
            other => panic!("expected Tool origin, got {other:?}"),
        }
    }

    #[test]
    fn unknown_command_maps_to_inline_origin() {
        let origin =
            classification_to_launch_origin(classify_start_command(Some("python -m http.server")));
        match origin {
            Some(PaneLaunchRef::InlineCommand { spec }) => {
                assert_eq!(spec.program(), "python");
            }
            other => panic!("expected InlineCommand origin, got {other:?}"),
        }
    }

    // ── Tokenizer edge cases ──────────────────────────────────────

    // ── Snapshot inline-command classification ────────────────────
    //
    // These tests cover legacy polluted
    // snapshots that persist the supervisor wrapper as
    // `InlineCommand { program, args }` with several program
    // shapes. The classifier must recognize every one of them.

    fn supervisor_wrapper_args() -> Vec<String> {
        // Mirrors the clean argv slots produced by
        // `build_supervisor_argv`: no surrounding quotes per arg.
        vec![
            "internal".to_owned(),
            "pane-supervisor".to_owned(),
            "--runtime-root".to_owned(),
            "/proj/.workhive/runtime".to_owned(),
            "--group".to_owned(),
            "wh-srv".to_owned(),
            "--workspace".to_owned(),
            "wh@iid@proj@main".to_owned(),
            "--unit".to_owned(),
            "%5".to_owned(),
            "--tool".to_owned(),
            "codex".to_owned(),
            "--".to_owned(),
            "codex".to_owned(),
        ]
    }

    #[test]
    fn classify_inline_command_recognizes_bare_workhive_program() {
        let args = supervisor_wrapper_args();
        match classify_inline_command("workhive", &args) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool=codex, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_recognizes_path_qualified_workhive_program() {
        let args = supervisor_wrapper_args();
        match classify_inline_command("/Users/me/target/release/workhive", &args) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool=codex, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_recognizes_single_quoted_workhive_program() {
        let args = supervisor_wrapper_args();
        match classify_inline_command("'/Users/me/target/release/workhive'", &args) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool=codex, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_recognizes_nested_double_and_single_quoted_workhive_program() {
        // Worst case observed in older legacy persisted state: the
        // outer double-quote wraps the inner tmux single-quoted
        // path token. Nesting depth must not block detection.
        let args = supervisor_wrapper_args();
        match classify_inline_command("\"'/Users/me/target/release/workhive'\"", &args) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool=codex, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_recovers_tool_when_args_themselves_are_quoted() {
        // Defends against an even older capture path that stored
        // each arg with surrounding single quotes (naive whitespace
        // split of tmux's quoted start-command output). The
        // classifier must strip those per-arg quotes for the
        // signature match and `--tool` resolution.
        let args = vec![
            "'internal'".to_owned(),
            "'pane-supervisor'".to_owned(),
            "'--tool'".to_owned(),
            "'codex'".to_owned(),
            "'--'".to_owned(),
            "'codex'".to_owned(),
        ];
        match classify_inline_command("'/Users/me/target/release/workhive'", &args) {
            StartCommandClassification::InternalSupervisorWrapper { tool: Some(name) } => {
                assert_eq!(name.as_str(), "codex");
            }
            other => panic!("expected wrapper with tool=codex from quoted args, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_preserves_real_workhive_status_command() {
        // `workhive status` is a legitimate user command, not the
        // supervisor wrapper. It must NOT be neutralized; the
        // original program/args slots round-trip unchanged.
        let args = vec!["status".to_owned(), "--json".to_owned()];
        match classify_inline_command("/usr/local/bin/workhive", &args) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "/usr/local/bin/workhive");
                assert_eq!(spec.args(), &args[..]);
            }
            other => panic!("expected InlineCommand for `workhive status`, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_preserves_bare_workhive_status_command() {
        // Same as above but with a non-path-qualified bare program
        // token. Still preserved because args do not match the
        // supervisor wrapper signature.
        let args = vec!["status".to_owned()];
        match classify_inline_command("workhive", &args) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "workhive");
                assert_eq!(spec.args(), &args[..]);
            }
            other => panic!("expected InlineCommand for bare `workhive status`, got {other:?}"),
        }
    }

    /// An inline command whose matching outer double quotes are
    /// scattered across the program/arg slots
    /// (`InlineCommand { program: "\"cursor-agent", args: ["--yolo\""] }`)
    /// normalizes back to the catalog tool, so a known tool resolves to
    /// the typed tool launch path instead of being replayed as the raw
    /// `"cursor-agent --yolo"` shell command.
    #[test]
    fn classify_inline_command_recovers_quote_split_known_tool() {
        let cases: [(&str, &[&str], &str); 3] = [
            ("\"cursor-agent", &["--yolo\""], "cursor-agent"),
            (
                "\"codex",
                &["--dangerously-bypass-approvals-and-sandbox\""],
                "codex",
            ),
            ("\"claude", &["--dangerously-skip-permissions\""], "claude"),
        ];
        for (program, args, expected) in cases {
            let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
            match classify_inline_command(program, &args) {
                StartCommandClassification::Tool { name } => assert_eq!(
                    name.as_str(),
                    expected,
                    "quote-split `{program} {args:?}` must recover {expected}"
                ),
                other => panic!("expected Tool({expected}) for quote-split shape, got {other:?}"),
            }
        }
    }

    /// Single-slot pollution: the whole quoted command landed in the
    /// `program` slot with no separate args. Still recoverable.
    #[test]
    fn classify_inline_command_recovers_single_slot_quote_split_tool() {
        match classify_inline_command("\"cursor-agent --yolo\"", &[]) {
            StartCommandClassification::Tool { name } => assert_eq!(name.as_str(), "cursor-agent"),
            other => panic!("expected Tool(cursor-agent) for single-slot shape, got {other:?}"),
        }
    }

    /// A quote-split shape whose leading token is NOT a catalog tool
    /// must remain a user inline command — recovery is high-confidence
    /// only.
    #[test]
    fn classify_inline_command_quote_split_unknown_program_stays_inline() {
        let args = vec!["script.py\"".to_owned()];
        match classify_inline_command("\"python", &args) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "\"python");
            }
            other => {
                panic!("expected InlineCommand for unknown quote-split program, got {other:?}")
            }
        }
    }

    #[test]
    fn classify_inline_command_preserves_unrelated_user_command() {
        let args = vec!["-m".to_owned(), "http.server".to_owned()];
        match classify_inline_command("python", &args) {
            StartCommandClassification::InlineCommand { spec } => {
                assert_eq!(spec.program(), "python");
                assert_eq!(spec.args(), &args[..]);
            }
            other => panic!("expected InlineCommand for unrelated user command, got {other:?}"),
        }
    }

    #[test]
    fn classify_inline_command_demotes_malformed_wrapper_to_no_tool() {
        // Wrapper signature matches but `--tool` is missing.
        // Mirrors the raw-string classifier's behavior so neither
        // path can persist the wrapper as a replayable inline
        // command.
        let args = vec![
            "internal".to_owned(),
            "pane-supervisor".to_owned(),
            "--runtime-root".to_owned(),
            "/r".to_owned(),
            "--".to_owned(),
            "claude".to_owned(),
        ];
        let classification = classify_inline_command("/Users/me/workhive", &args);
        assert!(
            matches!(
                classification,
                StartCommandClassification::InternalSupervisorWrapper { tool: None }
            ),
            "malformed wrapper must produce wrapper with tool=None, got {classification:?}",
        );
    }

    // ── Tokenizer edge cases ──────────────────────────────────────

    #[test]
    fn tokenizer_handles_quoted_argument_with_apostrophe() {
        // shell_single_quote escapes `it's fine` as `'it'\''s fine'`
        let raw = "'echo' 'it'\\''s fine'";
        let tokens = tokenize_start_command(raw);
        assert_eq!(tokens, vec!["echo".to_owned(), "it's fine".to_owned()]);
    }

    #[test]
    fn tokenizer_collapses_internal_whitespace_outside_quotes() {
        let tokens = tokenize_start_command("  foo   bar   baz  ");
        assert_eq!(
            tokens,
            vec!["foo".to_owned(), "bar".to_owned(), "baz".to_owned()]
        );
    }

    #[test]
    fn tokenizer_preserves_whitespace_inside_quotes() {
        let tokens = tokenize_start_command("'foo bar' 'baz'");
        assert_eq!(tokens, vec!["foo bar".to_owned(), "baz".to_owned()]);
    }
}

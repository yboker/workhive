#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;

/// Load the effective `.env` for a `workspace open` invocation.
///
/// Returns the merged effective env (global ∪ project, project keys win).
/// Failures from the optional `.env` overlay are degraded into a warning
/// log + empty result so they cannot block `open` (fail-open guidance
/// for optional config layers).
///
/// The implementation deliberately stays in the CLI layer because:
/// - it owns the "render warnings" decision (i18n, stderr);
/// - it can decide which file to display in human-facing messages.
///
/// Pure parse/merge logic lives in
/// [`workhive_core::usecases::env::resolve_effective_env`].
///
/// Returns both the merged effective env and the list of per-line
/// parse issues (with the source path of each issue) so the caller can
/// surface localized, user-visible warnings for malformed `.env` lines
/// (a malformed line such as "KEY: VALUE" must show path, line, and
/// reason in `workspace open`; the raw line and value must NOT leak).
pub(crate) fn resolve_open_effective_env(
    ctx: &CommandContext,
    project_root: &camino::Utf8Path,
) -> (
    workhive_core::usecases::env::EffectiveEnv,
    Vec<workhive_core::usecases::env::EffectiveEnvIssue>,
) {
    use workhive_core::usecases::env;
    match env::resolve_effective_env(&ctx.config, Some(project_root), None, None) {
        Ok((effective, issues)) => (effective, issues),
        Err(e) => {
            tracing::warn!(
                error = %e,
                project = %project_root,
                "failed to load .env overlay; continuing with no env injection"
            );
            (env::EffectiveEnv::default(), Vec::new())
        }
    }
}

/// Format a single `.env` parse warning line.
///
/// Pure helper used by [`print_env_parse_warnings`]; exposed for
/// default-running unit tests that assert the user-visible warning
/// surface (path + line + localized reason) and the secret-safe
/// redaction policy (no raw line, no value) without spinning a real
/// tmux server. The reason text is always rendered through the i18n
/// catalog for the supplied locale, so this also exercises the
/// English / Chinese parse-reason copy.
pub(crate) fn format_env_parse_warning(
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: workhive_core::domain::Locale,
    issue: &workhive_core::usecases::env::EffectiveEnvIssue,
) -> String {
    use workhive_core::i18n_keys as k;
    let line_str = issue.issue.line.to_string();
    let reason_key = issue.issue.kind.i18n_key();
    let reason_text = i18n.resolve(locale, reason_key);
    i18n.resolve_with_args(
        locale,
        k::OPEN_ENV_PARSE_WARNING,
        &[issue.source.as_str(), &line_str, &reason_text],
    )
}

/// Surface localized warnings for every malformed `.env` line found
/// during `workspace open`.
///
/// Output format is exactly
/// "warning: .env at <path> line <n>: <localized reason>". The raw
/// line is never echoed, and the value side is never read or logged.
pub(crate) fn print_env_parse_warnings(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    issues: &[workhive_core::usecases::env::EffectiveEnvIssue],
) {
    if issues.is_empty() {
        return;
    }

    for issue in issues {
        let msg = format_env_parse_warning(&ctx.i18n, locale, issue);
        eprintln!("{msg}");
    }
}

/// Build the user-visible injection summary lines for the resolved
/// effective `.env`.
///
/// Returns a vector ordered exactly the way the CLI prints them on
/// stderr: `[header, key_item_1, key_item_2, ...]`. Returns an empty
/// vector when no env vars were injected so callers can short-circuit
/// without printing a stray header. This is the pure half of
/// [`print_env_injection_summary`]; exposed so default-running tests
/// can verify that the malformed-key was dropped at parse-time and is
/// never listed in the summary, while a valid same-file key still
/// surfaces.
pub(crate) fn format_env_injection_summary_lines(
    i18n: &dyn workhive_core::ports::I18nPort,
    locale: workhive_core::domain::Locale,
    effective: &workhive_core::usecases::env::EffectiveEnv,
) -> Vec<String> {
    use workhive_core::i18n_keys as k;
    use workhive_core::usecases::env::EnvSource;

    if effective.is_empty() {
        return Vec::new();
    }

    let total = effective.len();
    let total_str = total.to_string();
    let global_only_count = effective
        .entries
        .iter()
        .filter(|e| e.source == EnvSource::Global)
        .count();
    let project_only_count = effective
        .entries
        .iter()
        .filter(|e| e.source == EnvSource::Project)
        .count();
    let override_count = effective.override_count;

    let header = if global_only_count == total {
        i18n.resolve_with_args(locale, k::OPEN_ENV_INJECTED_GLOBAL_ONLY, &[&total_str])
    } else if project_only_count == total {
        i18n.resolve_with_args(locale, k::OPEN_ENV_INJECTED_PROJECT_ONLY, &[&total_str])
    } else {
        let global_str = global_only_count.to_string();
        let project_str = (project_only_count + override_count).to_string();
        i18n.resolve_with_args(
            locale,
            k::OPEN_ENV_INJECTED_MIXED,
            &[&total_str, &global_str, &project_str],
        )
    };

    let mut lines = Vec::with_capacity(1 + effective.entries.len());
    lines.push(header);
    for entry in &effective.entries {
        lines.push(i18n.resolve_with_args(locale, k::OPEN_ENV_KEY_ITEM, &[entry.key.as_str()]));
    }
    lines
}

/// Render the user-visible injection summary on stderr.
///
/// Always counts and key names; never raw values. Implements the
/// env-injection observability requirement.
pub(crate) fn print_env_injection_summary(
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
    effective: &workhive_core::usecases::env::EffectiveEnv,
) {
    let lines = format_env_injection_summary_lines(&ctx.i18n, locale, effective);
    if lines.is_empty() {
        return;
    }
    for line in &lines {
        eprintln!("{line}");
    }

    tracing::info!(
        env_count = effective.len(),
        global_count = effective.global_count,
        project_count = effective.project_count,
        override_count = effective.override_count,
        keys = ?effective.entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        "open: injected effective .env into workspace runtime"
    );
}

/// Load effective layout: gather configs via I/O, then delegate
/// the precedence rule to `usecases::config_mgmt::resolve_effective_layout`.
pub(crate) fn load_effective_layout(
    ctx: &CommandContext,
    project_root: Option<&camino::Utf8Path>,
) -> LayoutConfig {
    let project_config = project_root.and_then(|root| match ctx.config.load_project(root) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(
                error = %e,
                project_root = %root,
                fallback = "global or default layout",
                "failed to load project config for layout"
            );
            None
        }
    });

    let global_config = match ctx.config.load_global() {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!(
                error = %e,
                fallback = "default layout",
                "failed to load global config for layout"
            );
            None
        }
    };

    usecases::config_mgmt::resolve_effective_layout(project_config.as_ref(), global_config.as_ref())
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::super::test_support::*;
    #[allow(unused_imports)]
    use super::*;
    #[allow(unused_imports)]
    use camino::Utf8PathBuf;
    #[allow(unused_imports)]
    use workhive_core::domain::{CommandSpec, PaneLaunchRef, ToolCatalog, ToolName};
    #[allow(unused_imports)]
    use workhive_core::ports::{
        ColumnConfig, CreateWorkspaceRuntimeRequest, LayoutConfig, LayoutTemplate, PaneConfig,
        WorkspaceRuntimePort,
    };
    #[allow(unused_imports)]
    use workhive_testkit::fakes::{FakeI18n, FakeRuntime};

    // ── `.env` parse-warning rendering ──
    //
    // These tests live alongside the production formatters so the
    // user-visible contract (path + line + localized reason; never the
    // raw line; never the value) can be enforced default-running, even
    // when the surrounding `workspace open` flow needs a real tmux
    // server that the CI worker does not have. The full real-tmux
    // surface is also covered by the gated CLI integration tests in
    // `crates/workhive-cli/tests/cli_integration.rs`:
    //
    // - `e2e_open_injects_env_visible_via_printenv_in_real_pane`
    //   (proves the global+project merge inside a real pane), and
    // - `e2e_open_emits_localized_env_parse_warning_and_redacts_raw_line`
    //   (proves the same warning surface via real `workhive workspace open`
    //    stderr).

    use workhive_core::usecases::env::{
        EffectiveEnv, EffectiveEnvEntry, EffectiveEnvIssue, EnvSource,
    };
    use workhive_infra::i18n::StaticI18n;

    #[test]
    fn format_env_parse_warning_includes_path_line_and_localized_yaml_reason_en() {
        let i18n = StaticI18n::new();
        let issue = yaml_issue("/home/dev/.config/workhive/.env", 1);

        let rendered = format_env_parse_warning(&i18n, Locale::En, &issue);

        assert!(
            rendered.contains("/home/dev/.config/workhive/.env"),
            "EN warning must echo .env source path, got: {rendered:?}"
        );
        assert!(
            rendered.contains(" line 1"),
            "EN warning must echo 1-based line number, got: {rendered:?}"
        );
        assert!(
            rendered.contains("KEY=VALUE"),
            "EN reason must mention KEY=VALUE syntax for YAML-style input, got: {rendered:?}"
        );
        assert!(
            rendered.starts_with("warning:"),
            "EN warning must use the localized warning prefix, got: {rendered:?}"
        );
    }

    /// Same render-time guarantee in zh-CN. Drives the production
    /// `open.env.parse_warning` Chinese template so we know the
    /// localized warning copy actually exists in the catalog and
    /// renders without dropping the path / line / reason fragments.
    #[test]
    fn format_env_parse_warning_includes_path_line_and_localized_yaml_reason_zh() {
        let i18n = StaticI18n::new();
        let issue = yaml_issue("/home/dev/.config/workhive/.env", 1);

        let rendered = format_env_parse_warning(&i18n, Locale::ZhCn, &issue);

        assert!(
            rendered.starts_with("警告："),
            "zh-CN warning must use Chinese warning prefix, got: {rendered:?}"
        );
        assert!(
            rendered.contains("/home/dev/.config/workhive/.env"),
            "zh-CN warning must echo .env source path verbatim, got: {rendered:?}"
        );
        assert!(
            rendered.contains("第 1 行"),
            "zh-CN warning must echo Chinese line-number phrasing, got: {rendered:?}"
        );
        assert!(
            rendered.contains("KEY=VALUE"),
            "zh-CN reason must reference KEY=VALUE syntax, got: {rendered:?}"
        );
        assert!(
            rendered.contains("YAML"),
            "zh-CN reason must reference YAML in the parse warning, got: {rendered:?}"
        );
    }

    /// Secret-safe redaction: the user-reported scenario writes
    /// `CLAUDE_CODE_NO_FLICKER: 1` and `API_KEY: super-secret-value`
    /// in a global `.env`. The warning the user sees in stderr must
    /// NOT echo any of those raw line contents — neither the YAML key
    /// they typed, nor the value side, nor the literal line text.
    #[test]
    fn format_env_parse_warning_omits_raw_line_contents_for_yaml_input() {
        let i18n = StaticI18n::new();
        let issue = yaml_issue("/home/dev/.config/workhive/.env", 2);

        // Render in both supported locales so the redaction guarantee
        // applies to every catalog, not just the default language.
        for locale in [Locale::En, Locale::ZhCn] {
            let rendered = format_env_parse_warning(&i18n, locale, &issue);
            for forbidden in [
                "CLAUDE_CODE_NO_FLICKER: 1",
                "API_KEY: super-secret-value",
                "super-secret-value",
                "API_KEY",
                "CLAUDE_CODE_NO_FLICKER",
                ": 1",
                ": super-secret-value",
            ] {
                assert!(
                    !rendered.contains(forbidden),
                    "{locale:?} warning must not leak raw line/value fragment {forbidden:?}, got: {rendered:?}"
                );
            }
        }
    }

    /// Render-time guarantee for the `MissingEquals` reason: even if
    /// the user wrote a free-form line (e.g. `CLAUDE_CODE_NO_FLICKER 1`),
    /// the warning surface must still be path + line + localized reason
    /// only. The raw line contents must never appear in stderr.
    #[test]
    fn format_env_parse_warning_omits_raw_line_contents_for_missing_equals_input() {
        let i18n = StaticI18n::new();
        let issue = EffectiveEnvIssue {
            source: Utf8PathBuf::from("/home/dev/proj/.workhive/.env"),
            issue: workhive_core::ports::EnvLineIssue {
                line: 5,
                kind: workhive_core::ports::EnvLineIssueKind::MissingEquals,
            },
        };

        let rendered = format_env_parse_warning(&i18n, Locale::En, &issue);

        assert!(rendered.contains("missing '=' separator"), "{rendered:?}");
        assert!(rendered.contains(" line 5"), "{rendered:?}");
        assert!(
            !rendered.contains("CLAUDE_CODE_NO_FLICKER 1")
                && !rendered.contains("CLAUDE_CODE_NO_FLICKER"),
            "missing-equals warning must not echo raw line contents, got: {rendered:?}"
        );
    }

    /// Injection-summary contract: a malformed line never reaches
    /// `EffectiveEnv.entries` (it is filtered out at parse time and
    /// surfaced as an `EffectiveEnvIssue` instead). This test fixes
    /// that contract at the render boundary so future refactors
    /// cannot accidentally start listing dropped malformed keys in
    /// the user-visible summary.
    #[test]
    fn format_env_injection_summary_lists_good_env_only_when_malformed_key_was_dropped() {
        let i18n = StaticI18n::new();
        let effective = EffectiveEnv {
            entries: vec![EffectiveEnvEntry {
                key: "GOOD_ENV".to_owned(),
                value: "ok".to_owned(),
                source: EnvSource::Global,
            }],
            global_path: Some(Utf8PathBuf::from("/home/dev/.config/workhive/.env")),
            project_path: None,
            global_count: 1,
            project_count: 0,
            override_count: 0,
        };

        let lines = format_env_injection_summary_lines(&i18n, Locale::En, &effective);

        assert!(
            !lines.is_empty(),
            "non-empty effective env must produce a summary block"
        );
        let joined = lines.join("\n");
        assert!(
            joined.contains("GOOD_ENV"),
            "summary must list the valid key, got: {joined:?}"
        );
        // The malformed YAML-style key never reached `effective.entries`
        // (it was rejected by the parser), so it must never appear in
        // the user-visible per-key list either.
        assert!(
            !joined.contains("CLAUDE_CODE_NO_FLICKER"),
            "summary must not list a key that was dropped at parse time, got: {joined:?}"
        );
        // And the YAML-style value side must not surface anywhere in
        // the summary block (defense in depth — the summary already
        // never reads values, but make sure no code path leaks them).
        assert!(
            !joined.contains("super-secret-value"),
            "summary must never contain raw .env values, got: {joined:?}"
        );
    }

    /// Empty `EffectiveEnv` short-circuits to an empty line list so
    /// the CLI does not emit a stray "injected 0 variable(s)" header
    /// when the user has no `.env` files.
    #[test]
    fn format_env_injection_summary_returns_no_lines_for_empty_effective_env() {
        let i18n = StaticI18n::new();
        let effective = EffectiveEnv::default();
        let lines = format_env_injection_summary_lines(&i18n, Locale::En, &effective);
        assert!(
            lines.is_empty(),
            "no env vars ⇒ no summary lines, got: {lines:?}"
        );
    }

    // ── Supervisor liveness probe ──
    //
    // `workspace open` MUST NOT trust raw `socket_path.exists()` as a
    // liveness signal. The probe helper classifies each ping outcome
    // into a typed `SupervisorProbeOutcome` so the open path drives the
    // right recovery branch (reuse / respawn / degraded summary).
    //
    // These tests exercise the helper without spinning up the full
    // composition: a missing socket is the most common stale state, and
    // a UDS file present without a live listener is the exact failure
    // mode the file-existence check used to mask.
}

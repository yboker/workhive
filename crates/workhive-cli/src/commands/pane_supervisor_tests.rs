use super::*;

#[test]
fn build_run_rejects_empty_runtime_root() {
    let err = build_supervisor_run(
        String::new(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        None,
        None,
        Vec::new(),
        vec!["echo".to_owned()],
    )
    .unwrap_err();
    match err {
        CliError::Runtime(msg) => assert!(msg.contains("--runtime-root")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn build_run_rejects_missing_program_argv() {
    let err = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        None,
        None,
        Vec::new(),
        Vec::new(),
    )
    .unwrap_err();
    match err {
        CliError::Runtime(msg) => assert!(msg.contains("missing program")),
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn build_run_preserves_typed_argv_without_shell_parsing() {
    let run = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        Some("claude".to_owned()),
        Some("/usr/local/bin/claude".to_owned()),
        Vec::new(),
        vec![
            "claude".to_owned(),
            "--dangerously-skip-permissions".to_owned(),
            "with spaces and $unexpanded".to_owned(),
        ],
    )
    .expect("valid input");
    assert_eq!(run.program, "claude");
    assert_eq!(
        run.args,
        vec![
            "--dangerously-skip-permissions".to_owned(),
            "with spaces and $unexpanded".to_owned(),
        ]
    );
    assert_eq!(run.command_basename, "claude");
    assert_eq!(run.program_path.as_deref(), Some("/usr/local/bin/claude"));
    assert_eq!(run.unit_id.as_str(), "u");
}

#[test]
fn build_run_derives_basename_from_program_when_tool_omitted() {
    let run = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        None,
        None,
        Vec::new(),
        vec!["/usr/local/bin/lazygit".to_owned()],
    )
    .expect("valid input");
    assert_eq!(run.command_basename, "lazygit");
    assert_eq!(run.program, "/usr/local/bin/lazygit");
    assert!(run.args.is_empty());
}

#[test]
fn build_run_parses_env_vars_into_btreemap() {
    let run = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        Some("claude".to_owned()),
        None,
        vec![
            "IS_SANDBOX=1".to_owned(),
            "FOO=bar=baz".to_owned(),
            "EMPTY=".to_owned(),
        ],
        vec![
            "claude".to_owned(),
            "--dangerously-skip-permissions".to_owned(),
        ],
    )
    .expect("valid input");
    assert_eq!(run.env.get("IS_SANDBOX").map(String::as_str), Some("1"));
    assert_eq!(run.env.get("FOO").map(String::as_str), Some("bar=baz"));
    assert_eq!(run.env.get("EMPTY").map(String::as_str), Some(""));
    assert_eq!(run.env.len(), 3);
}

#[test]
fn build_run_rejects_malformed_env_without_equals() {
    let err = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        None,
        None,
        vec!["NO_EQUALS_SIGN".to_owned()],
        vec!["echo".to_owned()],
    )
    .unwrap_err();
    match err {
        CliError::Runtime(msg) => assert!(
            msg.contains("malformed") && msg.contains("KEY=VALUE"),
            "error must describe the malformed entry: {msg}"
        ),
        other => panic!("expected Runtime error, got {other:?}"),
    }
}

#[test]
fn build_run_rejects_env_with_empty_key() {
    let err = build_supervisor_run(
        "/tmp/runtime".to_owned(),
        "g".to_owned(),
        "ws".to_owned(),
        "u".to_owned(),
        None,
        None,
        vec!["=value".to_owned()],
        vec!["echo".to_owned()],
    )
    .unwrap_err();
    match err {
        CliError::Runtime(msg) => assert!(
            msg.contains("key must not be empty"),
            "error must describe the empty key: {msg}"
        ),
        other => panic!("expected Runtime error, got {other:?}"),
    }
}

/// Transient I/O failures must fail open: the supervisor still
/// proceeds to spawn the child so a flaky disk does not block the
/// developer. `classify_prepare_failure` returns `Ok(())` in this
/// branch.
#[test]
fn classify_transient_failure_fails_open() {
    use crate::commands::integrations::error::IntegrationError;
    use crate::context::CommandContext;
    let ctx = CommandContext::new();
    // Build a transient I/O error directly (the `io_err`
    // constructor is `pub(super)` inside the integrations module
    // and unreachable from here — the enum variant itself is
    // `pub(crate)` so we can construct it inline).
    let err: IntegrationError = IntegrationError::Io {
        context: "write",
        path: camino::Utf8PathBuf::from("/tmp/settings.json"),
        source: std::io::Error::other("transient"),
    };
    let result = classify_prepare_failure(err, AiActivityProvider::ClaudeCode, &ctx);
    result.expect("transient I/O failure must fail open, not abort the spawn");
}

// ── supervisor_feature_gate: project-over-global resolution ──
//
// These tests pin the four critical behaviors of the spawn-time
// gate evaluation:
//
// 1. With no project root and no config, the built-in default
//    resolves to `Resolved(everything enabled)`.
// 2. With a project root supplied, project values override
//    global values (project-over-global).
// 3. A `load_global` failure surfaces as `Unavailable`
//    (fail-closed write path), not a fall-back to enabled.
// 4. A `load_project` failure surfaces as `Unavailable`
//    (fail-closed write path), not a fall-back to global-only.

fn features_disable_codex() -> workhive_core::ports::FeaturesConfig {
    use workhive_core::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };
    FeaturesConfig {
        ai_integrations: Some(AiIntegrationsConfig {
            enabled: None,
            providers: AiIntegrationProvidersConfig {
                claude_code: None,
                codex: Some(AiIntegrationProviderConfig {
                    enabled: Some(false),
                    ..Default::default()
                }),
                opencode: None,
            },
        }),
        pane_observation: None,
        notifications: None,
    }
}

fn features_enable_all() -> workhive_core::ports::FeaturesConfig {
    use workhive_core::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };
    FeaturesConfig {
        ai_integrations: Some(AiIntegrationsConfig {
            enabled: Some(true),
            providers: AiIntegrationProvidersConfig {
                claude_code: Some(AiIntegrationProviderConfig {
                    enabled: Some(true),
                    ..Default::default()
                }),
                codex: Some(AiIntegrationProviderConfig {
                    enabled: Some(true),
                    ..Default::default()
                }),
                opencode: Some(AiIntegrationProviderConfig {
                    enabled: Some(true),
                    ..Default::default()
                }),
            },
        }),
        pane_observation: None,
        notifications: None,
    }
}

#[test]
fn supervisor_gate_resolves_to_builtin_defaults_when_no_config() {
    let fake = workhive_testkit::fakes::FakeConfig::new();
    let resolution = supervisor_feature_gate(&fake, None);
    match resolution {
        FeatureGateResolution::Resolved(gate) => {
            assert!(gate.globally_enabled());
            assert!(gate.is_provider_enabled(AiActivityProvider::Codex));
        }
        FeatureGateResolution::Unavailable { reason } => {
            panic!("expected Resolved, got Unavailable: {reason}");
        }
    }
}

/// Project config disabling Codex must override a global config
/// that enables Codex. This is the spawn-time
/// project-over-global gate contract.
#[test]
fn supervisor_gate_project_disable_overrides_global_enable_for_codex() {
    use workhive_core::ports::AppConfig;
    let project_root = camino::Utf8PathBuf::from("/proj");
    let global = AppConfig {
        features: Some(features_enable_all()),
        ..Default::default()
    };
    let project = AppConfig {
        features: Some(features_disable_codex()),
        ..Default::default()
    };
    let fake = workhive_testkit::fakes::FakeConfig::new()
        .with_global(global)
        .with_project(project);

    let resolution = supervisor_feature_gate(&fake, Some(project_root.as_path()));
    match resolution {
        FeatureGateResolution::Resolved(gate) => {
            assert!(
                gate.globally_enabled(),
                "global table left ai_integrations.enabled = true after project merge"
            );
            assert!(
                !gate.is_provider_enabled(AiActivityProvider::Codex),
                "project disable for codex must override the global enable",
            );
            assert!(
                gate.is_provider_enabled(AiActivityProvider::ClaudeCode),
                "other providers must remain enabled when only codex is disabled at project scope",
            );
        }
        FeatureGateResolution::Unavailable { reason } => {
            panic!("expected Resolved, got Unavailable: {reason}");
        }
    }
}

/// `prepare_provider_if_enabled` already short-circuits to
/// `Ok(None)` when the merged gate disables the provider. This
/// test wires it end-to-end with the project-over-global
/// resolution above so a project-level Codex disable produces
/// the same gate-blocked silent outcome regardless of global
/// state.
#[test]
fn supervisor_gate_project_disable_for_codex_yields_gate_blocked_prepare() {
    use crate::commands::integrations::prepare_provider_if_enabled;
    use workhive_core::ports::AppConfig;
    let project_root = camino::Utf8PathBuf::from("/proj");
    let global = AppConfig {
        features: Some(features_enable_all()),
        ..Default::default()
    };
    let project = AppConfig {
        features: Some(features_disable_codex()),
        ..Default::default()
    };
    let fake = workhive_testkit::fakes::FakeConfig::new()
        .with_global(global)
        .with_project(project);

    let resolution = supervisor_feature_gate(&fake, Some(project_root.as_path()));
    let gate = match resolution {
        FeatureGateResolution::Resolved(g) => g,
        FeatureGateResolution::Unavailable { reason } => {
            panic!("expected Resolved, got Unavailable: {reason}")
        }
    };
    let outcome = prepare_provider_if_enabled(AiActivityProvider::Codex, &gate)
        .expect("prepare must not error on gate-blocked path");
    assert!(
        outcome.is_none(),
        "project-disabled Codex must produce gate-blocked Ok(None) so the supervisor writes nothing",
    );
}

/// A `load_global` failure must NOT fall back to
/// `FeatureGate::builtin_default()` (which would be silently
/// "enabled"). It must surface as `Unavailable` so the supervisor
/// fails closed on the write path.
#[test]
fn supervisor_gate_unavailable_when_global_config_load_fails() {
    use workhive_core::errors::ConfigError;
    let fake =
        workhive_testkit::fakes::FakeConfig::new().with_global_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/etc/workhive/config.toml"),
            message: "synthetic global parse failure".to_owned(),
        });
    let resolution = supervisor_feature_gate(&fake, None);
    match resolution {
        FeatureGateResolution::Resolved(_) => {
            panic!("global load failure must NOT fall back to Resolved/enabled");
        }
        FeatureGateResolution::Unavailable { reason } => {
            assert!(
                reason.contains("global"),
                "reason must name the failing scope: {reason}"
            );
        }
    }
}

/// A `load_project` failure must also surface as `Unavailable`
/// for the same fail-closed reason.
#[test]
fn supervisor_gate_unavailable_when_project_config_load_fails() {
    use workhive_core::errors::ConfigError;
    let project_root = camino::Utf8PathBuf::from("/proj");
    let fake =
        workhive_testkit::fakes::FakeConfig::new().with_project_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/proj/.workhive/config.toml"),
            message: "synthetic project parse failure".to_owned(),
        });
    let resolution = supervisor_feature_gate(&fake, Some(project_root.as_path()));
    match resolution {
        FeatureGateResolution::Resolved(_) => {
            panic!("project load failure must NOT fall back to Resolved/enabled");
        }
        FeatureGateResolution::Unavailable { reason } => {
            assert!(
                reason.contains("project"),
                "reason must name the failing scope: {reason}"
            );
        }
    }
}

// ── Notification config fail-closed gate ─────────────────────────

/// When neither global nor project config
/// returns an error and neither declares a notifications block,
/// the supervisor MUST receive `Trusted(None)` — that case still
/// hands `None` to the delivery use case so built-in defaults
/// apply. This pins the "absent config != untrusted config"
/// semantic.
#[test]
fn supervisor_notifications_trusted_none_when_no_config_present() {
    let fake = workhive_testkit::fakes::FakeConfig::new();
    let decision = resolve_supervisor_notifications_config(&fake, None);
    match decision {
        NotificationsConfigDecision::Trusted(cfg) => assert!(
            cfg.is_none(),
            "absent config must produce Trusted(None), got {cfg:?}"
        ),
        other => panic!("absent config must NOT trip the fail-closed gate, got {other:?}"),
    }
}

/// When the user has explicitly disabled
/// notifications via the master switch, the supervisor MUST
/// receive `Trusted(Some(cfg))` carrying `enabled = Some(false)`
/// so the downstream policy refuses delivery via
/// `DeliveryRefusal::FeatureGateDisabled`. This is the "disabled
/// gate" test the review checklist requires.
#[test]
fn supervisor_notifications_trusted_disabled_when_user_disabled_gate() {
    use workhive_core::ports::{AppConfig, FeaturesConfig, NotificationsConfig};
    let cfg = AppConfig {
        features: Some(FeaturesConfig {
            notifications: Some(NotificationsConfig {
                enabled: Some(false),
                ..NotificationsConfig::default()
            }),
            ..FeaturesConfig::default()
        }),
        ..AppConfig::default()
    };
    let fake = workhive_testkit::fakes::FakeConfig::new().with_global(cfg);
    let decision = resolve_supervisor_notifications_config(&fake, None);
    match decision {
        NotificationsConfigDecision::Trusted(Some(n)) => {
            assert!(
                !n.is_enabled(),
                "disabled-gate config must produce is_enabled()=false"
            );
        }
        other => panic!("disabled gate must be Trusted(Some(disabled_cfg)), got {other:?}"),
    }
}

/// The critical fail-closed case: when global config
/// load/parse fails, the supervisor MUST observe
/// `Untrusted { reason }` — never `Trusted(None)` (which would
/// activate built-in defaults). The supervisor's delivery path
/// short-circuits on this decision and emits no notifications.
/// Previously the `unwrap_or(None)` pattern swallowed the error
/// and the system happily fired notifications using built-in
/// defaults the user could not have authorized.
#[test]
fn supervisor_notifications_fail_closed_on_global_config_error() {
    use workhive_core::errors::ConfigError;
    let fake =
        workhive_testkit::fakes::FakeConfig::new().with_global_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/etc/workhive/config.toml"),
            message: "synthetic global parse failure".to_owned(),
        });
    let decision = resolve_supervisor_notifications_config(&fake, None);
    match decision {
        NotificationsConfigDecision::Trusted(_) => panic!(
            "global config load failure must NOT downgrade to Trusted; \
                 that would silently fall back to built-in defaults"
        ),
        NotificationsConfigDecision::Untrusted { reason } => assert!(
            reason.contains("global"),
            "reason must name the failing scope: {reason}"
        ),
    }
}

// -----------------------------------------------------------------
// resolve_tool_exit_behavior tests (managed-tool exit policy).
//
// These verify that the CLI-side resolver implements the typed
// contract: trusted absence falls to the built-in default, an
// explicit user value is honored exactly, and any config-load or
// parse failure must surface as Untrusted so the caller can refuse
// to silently apply `respawn_shell`.
//
//   - absent everywhere        -> Resolved(RespawnShell) (built-in)
//   - global only              -> Resolved(global value)
//   - project overrides global -> Resolved(project value)
//   - config-load failure      -> Untrusted { reason }
//                                 NEVER silently degrades to
//                                 the built-in default.
// -----------------------------------------------------------------

fn exit_cfg(behavior: ManagedToolExitBehavior) -> workhive_core::ports::AppConfig {
    use workhive_core::ports::{AppConfig, PaneExitConfig, PaneRuntimeConfig};
    AppConfig {
        pane: Some(PaneRuntimeConfig {
            exit: Some(PaneExitConfig {
                tool_exit_behavior: Some(behavior),
            }),
        }),
        ..AppConfig::default()
    }
}

#[test]
fn resolve_tool_exit_behavior_defaults_to_respawn_shell_when_unconfigured() {
    let fake = workhive_testkit::fakes::FakeConfig::new();
    let decision = resolve_tool_exit_behavior(&fake, None);
    assert_eq!(
        decision,
        ToolExitBehaviorDecision::Resolved(ManagedToolExitBehavior::RespawnShell)
    );
}

#[test]
fn resolve_tool_exit_behavior_honors_global_close_pane() {
    let fake = workhive_testkit::fakes::FakeConfig::new()
        .with_global(exit_cfg(ManagedToolExitBehavior::ClosePane));
    let decision = resolve_tool_exit_behavior(&fake, None);
    assert_eq!(
        decision,
        ToolExitBehaviorDecision::Resolved(ManagedToolExitBehavior::ClosePane)
    );
}

#[test]
fn resolve_tool_exit_behavior_project_overrides_global() {
    let project_root = camino::Utf8PathBuf::from("/proj");
    let fake = workhive_testkit::fakes::FakeConfig::new()
        .with_global(exit_cfg(ManagedToolExitBehavior::ClosePane))
        .with_project(exit_cfg(ManagedToolExitBehavior::RespawnShell));
    let decision = resolve_tool_exit_behavior(&fake, Some(project_root.as_path()));
    assert_eq!(
        decision,
        ToolExitBehaviorDecision::Resolved(ManagedToolExitBehavior::RespawnShell)
    );
}

/// Global config load/parse failure MUST surface as `Untrusted`,
/// not silently downgrade to the built-in default. A write-side
/// effect (replacement-shell exec) on the back of unreadable
/// config would honor a behavior the user never expressed.
#[test]
fn resolve_tool_exit_behavior_returns_untrusted_when_global_config_fails() {
    use workhive_core::errors::ConfigError;
    let fake =
        workhive_testkit::fakes::FakeConfig::new().with_global_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/etc/workhive/config.toml"),
            message: "synthetic parse failure".to_owned(),
        });
    let decision = resolve_tool_exit_behavior(&fake, None);
    match decision {
        ToolExitBehaviorDecision::Resolved(b) => panic!(
            "global config load failure must NOT silently resolve to {:?}; \
                 that would apply a behavior the user never expressed",
            b
        ),
        ToolExitBehaviorDecision::Untrusted { reason } => assert!(
            reason.contains("global"),
            "reason must name the failing scope: {reason}"
        ),
    }
}

/// Project config load/parse failure must ALSO surface as
/// `Untrusted`. A broken project file is not safe to swallow
/// in favor of "use the global value": the project may have
/// deliberately wanted `close_pane` and we cannot tell.
#[test]
fn resolve_tool_exit_behavior_returns_untrusted_when_project_config_fails() {
    use workhive_core::errors::ConfigError;
    let project_root = camino::Utf8PathBuf::from("/proj");
    let fake = workhive_testkit::fakes::FakeConfig::new()
        .with_global(exit_cfg(ManagedToolExitBehavior::ClosePane))
        .with_project_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/proj/.workhive/config.toml"),
            message: "synthetic project parse failure".to_owned(),
        });
    let decision = resolve_tool_exit_behavior(&fake, Some(project_root.as_path()));
    match decision {
        ToolExitBehaviorDecision::Resolved(b) => panic!(
            "project config load failure must NOT silently resolve to {:?}; \
                 that would erase the project's expressed exit-policy intent",
            b
        ),
        ToolExitBehaviorDecision::Untrusted { reason } => assert!(
            reason.contains("project"),
            "reason must name the failing scope: {reason}"
        ),
    }
}

/// Project config load/parse failure must
/// behave identically — typed `Untrusted`, never `Trusted(None)`.
#[test]
fn supervisor_notifications_fail_closed_on_project_config_error() {
    use workhive_core::errors::ConfigError;
    let project_root = camino::Utf8PathBuf::from("/proj");
    let fake =
        workhive_testkit::fakes::FakeConfig::new().with_project_error(ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("/proj/.workhive/config.toml"),
            message: "synthetic project parse failure".to_owned(),
        });
    let decision = resolve_supervisor_notifications_config(&fake, Some(project_root.as_path()));
    match decision {
        NotificationsConfigDecision::Trusted(_) => {
            panic!("project config load failure must NOT downgrade to Trusted")
        }
        NotificationsConfigDecision::Untrusted { reason } => assert!(
            reason.contains("project"),
            "reason must name the failing scope: {reason}"
        ),
    }
}

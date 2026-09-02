use super::*;
use crate::ports::{DiscoveryConfig, LayoutConfig, SnapshotConfig};
use camino::Utf8PathBuf;

#[test]
fn overlay_wizard_preserves_fields_wizard_does_not_manage() {
    let existing = AppConfig {
        language: Some(crate::domain::Locale::ZhCn),
        discovery: Some(DiscoveryConfig {
            project_roots: vec![Utf8PathBuf::from("/home/user/projects")],
        }),
        worktree_root: Some(Utf8PathBuf::from("/worktrees")),
        snapshot_root: Some(Utf8PathBuf::from("/snapshots")),
        runtime_root: Some(Utf8PathBuf::from("/runtime")),
        ..Default::default()
    };

    let wizard = AppConfig {
        language: Some(crate::domain::Locale::En),
        layout: Some(LayoutConfig::default()),
        snapshot: Some(SnapshotConfig::default()),
        discovery: None,
        worktree_root: None,
        snapshot_root: None,
        runtime_root: None,
        ..Default::default()
    };

    let result = overlay_wizard_config(&existing, &wizard);

    assert_eq!(result.language, Some(crate::domain::Locale::En));
    assert!(result.layout.is_some());
    assert!(result.snapshot.is_some());

    assert!(result.discovery.is_some());
    let roots = &result.discovery.unwrap().project_roots;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0], Utf8PathBuf::from("/home/user/projects"));

    assert_eq!(result.worktree_root, Some(Utf8PathBuf::from("/worktrees")));
    assert_eq!(result.snapshot_root, Some(Utf8PathBuf::from("/snapshots")));
    assert_eq!(result.runtime_root, Some(Utf8PathBuf::from("/runtime")));
}

#[test]
fn overlay_wizard_takes_wizard_value_when_both_set() {
    let existing = AppConfig {
        language: Some(crate::domain::Locale::ZhCn),
        snapshot: Some(SnapshotConfig {
            enabled: false,
            ..SnapshotConfig::default()
        }),
        ..Default::default()
    };

    let wizard = AppConfig {
        language: Some(crate::domain::Locale::En),
        snapshot: Some(SnapshotConfig::default()),
        ..Default::default()
    };

    let result = overlay_wizard_config(&existing, &wizard);
    assert_eq!(result.language, Some(crate::domain::Locale::En));
    assert!(result.snapshot.unwrap().enabled);
}

#[test]
fn overlay_wizard_inherits_existing_when_wizard_has_none() {
    let existing = AppConfig {
        language: Some(crate::domain::Locale::ZhCn),
        tmux: Some(TmuxConfig::default()),
        ..Default::default()
    };

    let wizard = AppConfig::default();

    let result = overlay_wizard_config(&existing, &wizard);
    assert_eq!(result.language, Some(crate::domain::Locale::ZhCn));
    assert!(result.tmux.is_some());
}

#[test]
fn effective_layout_project_overrides_global() {
    use crate::ports::LayoutTemplate;
    let project = AppConfig {
        layout: Some(LayoutConfig {
            template: LayoutTemplate::AiCodingLeft,
            ..Default::default()
        }),
        ..Default::default()
    };
    let global = AppConfig {
        layout: Some(LayoutConfig::default()),
        ..Default::default()
    };
    let result = resolve_effective_layout(Some(&project), Some(&global));
    assert_eq!(result.template, LayoutTemplate::AiCodingLeft);
}

#[test]
fn effective_layout_falls_back_to_global() {
    let global = AppConfig {
        layout: Some(LayoutConfig::default()),
        ..Default::default()
    };
    let result = resolve_effective_layout(None, Some(&global));
    assert!(result == LayoutConfig::default());
}

#[test]
fn effective_layout_returns_default_when_both_none() {
    let result = resolve_effective_layout(None, None);
    assert!(result == LayoutConfig::default());
}

#[test]
fn effective_layout_skips_project_without_layout() {
    let project = AppConfig::default();
    let global = AppConfig {
        layout: Some(LayoutConfig::default()),
        ..Default::default()
    };
    let result = resolve_effective_layout(Some(&project), Some(&global));
    assert!(result == LayoutConfig::default());
}

// ── Managed-tool exit behavior resolution ──

#[test]
fn effective_tool_exit_behavior_defaults_to_respawn_shell() {
    assert_eq!(
        resolve_effective_tool_exit_behavior(None, None),
        crate::ports::ManagedToolExitBehavior::RespawnShell
    );
}

#[test]
fn effective_tool_exit_behavior_inherits_from_global_when_project_absent() {
    let global = AppConfig {
        pane: Some(crate::ports::PaneRuntimeConfig {
            exit: Some(crate::ports::PaneExitConfig {
                tool_exit_behavior: Some(crate::ports::ManagedToolExitBehavior::ClosePane),
            }),
        }),
        ..Default::default()
    };
    let project = AppConfig::default();
    assert_eq!(
        resolve_effective_tool_exit_behavior(Some(&project), Some(&global)),
        crate::ports::ManagedToolExitBehavior::ClosePane
    );
}

#[test]
fn effective_tool_exit_behavior_project_overrides_global() {
    let global = AppConfig {
        pane: Some(crate::ports::PaneRuntimeConfig {
            exit: Some(crate::ports::PaneExitConfig {
                tool_exit_behavior: Some(crate::ports::ManagedToolExitBehavior::ClosePane),
            }),
        }),
        ..Default::default()
    };
    let project = AppConfig {
        pane: Some(crate::ports::PaneRuntimeConfig {
            exit: Some(crate::ports::PaneExitConfig {
                tool_exit_behavior: Some(crate::ports::ManagedToolExitBehavior::RespawnShell),
            }),
        }),
        ..Default::default()
    };
    assert_eq!(
        resolve_effective_tool_exit_behavior(Some(&project), Some(&global)),
        crate::ports::ManagedToolExitBehavior::RespawnShell
    );
}

#[test]
fn merge_configs_propagates_pane_runtime_table() {
    let global = AppConfig {
        pane: Some(crate::ports::PaneRuntimeConfig {
            exit: Some(crate::ports::PaneExitConfig {
                tool_exit_behavior: Some(crate::ports::ManagedToolExitBehavior::ClosePane),
            }),
        }),
        ..Default::default()
    };
    let project = AppConfig::default();
    let merged = merge_configs(&global, &project);
    assert_eq!(
        merged
            .pane
            .and_then(|p| p.exit)
            .map(|e| e.resolved_tool_exit_behavior()),
        Some(crate::ports::ManagedToolExitBehavior::ClosePane)
    );
}

#[test]
fn config_set_rejects_session_template_without_instance_id() {
    let mut config = AppConfig::default();
    let result = apply_set(
        &mut config,
        "tmux.naming.session_template",
        "wh@{project}@{workspace}",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("{instance_id}"));
}

#[test]
fn config_set_rejects_server_template_without_instance_id() {
    let mut config = AppConfig::default();
    let result = apply_set(
        &mut config,
        "tmux.naming.server_template",
        "whsrv-{project}",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("{instance_id}"));
}

#[test]
fn config_set_accepts_valid_session_template() {
    let mut config = AppConfig::default();
    let result = apply_set(
        &mut config,
        "tmux.naming.session_template",
        "wh@{instance_id}@{project}@{workspace}",
    );
    assert!(result.is_ok());
}

#[test]
fn config_set_accepts_valid_server_template() {
    let mut config = AppConfig::default();
    let result = apply_set(
        &mut config,
        "tmux.naming.server_template",
        "whsrv-{instance_id}-{project}",
    );
    assert!(result.is_ok());
}

// ── Logging config set/unset ──

#[test]
fn set_console_level() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.console.level", "trace").unwrap();
    let c = config.logging.unwrap().console.unwrap();
    assert_eq!(c.level, Some(crate::ports::LogLevel::Trace));
}

#[test]
fn set_console_format() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.console.format", "json").unwrap();
    let c = config.logging.unwrap().console.unwrap();
    assert_eq!(c.format, Some(crate::ports::LogFormat::Json));
}

#[test]
fn set_console_enabled() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.console.enabled", "false").unwrap();
    let c = config.logging.unwrap().console.unwrap();
    assert_eq!(c.enabled, Some(false));
}

#[test]
fn set_file_level() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.level", "error").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.level, Some(crate::ports::LogLevel::Error));
}

#[test]
fn set_file_base_name_valid() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.base_name", "my-app.log").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.base_name.as_deref(), Some("my-app.log"));
}

#[test]
fn set_file_base_name_rejects_invalid() {
    let mut config = AppConfig::default();
    let result = apply_set(&mut config, "logging.file.base_name", "bad.txt");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains(".log"));
}

#[test]
fn set_file_directory() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.directory", "/tmp/logs").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.directory.unwrap().as_str(), "/tmp/logs");
}

#[test]
fn set_file_enabled() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.enabled", "true").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.enabled, Some(true));
}

#[test]
fn set_rejects_invalid_log_level() {
    let mut config = AppConfig::default();
    let result = apply_set(&mut config, "logging.console.level", "verbose");
    assert!(result.is_err());
}

#[test]
fn set_rejects_invalid_log_format() {
    let mut config = AppConfig::default();
    let result = apply_set(&mut config, "logging.console.format", "xml");
    assert!(result.is_err());
}

#[test]
fn set_rejects_unknown_logging_key() {
    let mut config = AppConfig::default();
    let result = apply_set(&mut config, "logging.console.foobar", "value");
    assert!(result.is_err());
}

#[test]
fn unset_console_level() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            console: Some(crate::ports::ConsoleLogConfig {
                level: Some(crate::ports::LogLevel::Trace),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.console.level").unwrap();
    let c = config.logging.unwrap().console.unwrap();
    assert_eq!(c.level, None);
}

#[test]
fn unset_console_section() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            console: Some(crate::ports::ConsoleLogConfig::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.console").unwrap();
    assert!(config.logging.unwrap().console.is_none());
}

#[test]
fn unset_file_section() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            file: Some(crate::ports::FileLogConfig::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.file").unwrap();
    assert!(config.logging.unwrap().file.is_none());
}

#[test]
fn merge_configs_preserves_logging() {
    let global = AppConfig {
        logging: Some(crate::ports::LogConfig {
            console: Some(crate::ports::ConsoleLogConfig {
                level: Some(crate::ports::LogLevel::Debug),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = AppConfig::default();
    let merged = merge_configs(&global, &project);
    let log = merged.logging.unwrap();
    assert_eq!(
        log.console.unwrap().level,
        Some(crate::ports::LogLevel::Debug)
    );
}

#[test]
fn merge_configs_project_overrides_logging() {
    let global = AppConfig {
        logging: Some(crate::ports::LogConfig {
            console: Some(crate::ports::ConsoleLogConfig {
                level: Some(crate::ports::LogLevel::Warn),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = AppConfig {
        logging: Some(crate::ports::LogConfig {
            console: Some(crate::ports::ConsoleLogConfig {
                level: Some(crate::ports::LogLevel::Trace),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = merge_configs(&global, &project);
    let log = merged.logging.unwrap();
    assert_eq!(
        log.console.unwrap().level,
        Some(crate::ports::LogLevel::Trace)
    );
}

// ── features.ai_integrations merge ──

#[test]
fn merge_configs_inherits_global_features_when_project_missing() {
    use crate::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };
    let global = AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: Some(false),
                providers: AiIntegrationProvidersConfig {
                    codex: Some(AiIntegrationProviderConfig {
                        enabled: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }),
            pane_observation: None,
            notifications: None,
        }),
        ..Default::default()
    };
    let project = AppConfig::default();
    let merged = merge_configs(&global, &project);
    let ai = merged
        .features
        .as_ref()
        .and_then(|f| f.ai_integrations.as_ref())
        .expect("features.ai_integrations should be inherited");
    assert_eq!(ai.enabled, Some(false));
    assert_eq!(
        ai.providers.codex.as_ref().and_then(|p| p.enabled),
        Some(false)
    );
}

#[test]
fn merge_configs_project_overrides_global_features() {
    use crate::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };
    let global = AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: Some(false),
                providers: AiIntegrationProvidersConfig {
                    codex: Some(AiIntegrationProviderConfig {
                        enabled: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }),
            pane_observation: None,
            notifications: None,
        }),
        ..Default::default()
    };
    let project = AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: Some(true),
                providers: AiIntegrationProvidersConfig {
                    codex: Some(AiIntegrationProviderConfig {
                        enabled: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }),
            pane_observation: None,
            notifications: None,
        }),
        ..Default::default()
    };
    let merged = merge_configs(&global, &project);
    let ai = merged
        .features
        .as_ref()
        .and_then(|f| f.ai_integrations.as_ref())
        .unwrap();
    assert_eq!(ai.enabled, Some(true));
    assert_eq!(
        ai.providers.codex.as_ref().and_then(|p| p.enabled),
        Some(true)
    );
}

#[test]
fn merge_configs_partial_project_features_inherits_global_per_provider() {
    use crate::ports::{
        AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig,
        FeaturesConfig,
    };
    // Global disables codex; project disables claude_code but doesn't
    // mention codex. The merged view must keep both disables.
    let global = AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: None,
                providers: AiIntegrationProvidersConfig {
                    codex: Some(AiIntegrationProviderConfig {
                        enabled: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }),
            pane_observation: None,
            notifications: None,
        }),
        ..Default::default()
    };
    let project = AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: None,
                providers: AiIntegrationProvidersConfig {
                    claude_code: Some(AiIntegrationProviderConfig {
                        enabled: Some(false),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }),
            pane_observation: None,
            notifications: None,
        }),
        ..Default::default()
    };
    let merged = merge_configs(&global, &project);
    let ai = merged
        .features
        .as_ref()
        .and_then(|f| f.ai_integrations.as_ref())
        .unwrap();
    assert_eq!(
        ai.providers.codex.as_ref().and_then(|p| p.enabled),
        Some(false),
        "global codex disable should survive when project doesn't mention it"
    );
    assert_eq!(
        ai.providers.claude_code.as_ref().and_then(|p| p.enabled),
        Some(false),
        "project claude_code disable should apply"
    );
}

// ── Rotation set/unset ──

#[test]
fn set_file_rotation_never() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.rotation", "never").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.rotation, Some(crate::ports::LogRotation::Never));
}

#[test]
fn set_file_rotation_daily() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.rotation", "daily").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.rotation, Some(crate::ports::LogRotation::Daily));
}

#[test]
fn set_file_rotation_hourly() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.rotation", "hourly").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.rotation, Some(crate::ports::LogRotation::Hourly));
}

#[test]
fn set_file_rotation_size_mb() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.rotation", "size_mb(50)").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.rotation, Some(crate::ports::LogRotation::SizeMb(50)));
}

#[test]
fn set_file_rotation_size_mb_zero_rejected() {
    let mut config = AppConfig::default();
    let err = apply_set(&mut config, "logging.file.rotation", "size_mb(0)");
    assert!(err.is_err());
}

#[test]
fn set_file_rotation_invalid_rejected() {
    let mut config = AppConfig::default();
    let err = apply_set(&mut config, "logging.file.rotation", "weekly");
    assert!(err.is_err());
}

#[test]
fn set_file_scope_project() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.scope", "project").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.scope, Some(crate::ports::FileLogScope::Project));
}

// ── Retention set/unset ──

#[test]
fn set_file_retention_unlimited() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.retention", "unlimited").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.retention, Some(crate::ports::LogRetention::Unlimited));
}

#[test]
fn set_file_retention_max_files() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.retention", "max_files(14)").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(
        file.retention,
        Some(crate::ports::LogRetention::MaxFiles(14))
    );
}

#[test]
fn set_file_retention_max_files_zero_rejected() {
    let mut config = AppConfig::default();
    let err = apply_set(&mut config, "logging.file.retention", "max_files(0)");
    assert!(err.is_err());
}

#[test]
fn set_file_retention_invalid_rejected() {
    let mut config = AppConfig::default();
    let err = apply_set(&mut config, "logging.file.retention", "keep_days(7)");
    assert!(err.is_err());
}

#[test]
fn set_file_scope_global() {
    let mut config = AppConfig::default();
    apply_set(&mut config, "logging.file.scope", "global").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.scope, Some(crate::ports::FileLogScope::Global));
}

// ── Unset rotation / retention ──

#[test]
fn unset_file_rotation_resets_to_default() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            file: Some(crate::ports::FileLogConfig {
                rotation: Some(crate::ports::LogRotation::Never),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.file.rotation").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.rotation, None);
}

#[test]
fn unset_file_retention_resets_to_default() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            file: Some(crate::ports::FileLogConfig {
                retention: Some(crate::ports::LogRetention::Unlimited),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.file.retention").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.retention, None);
}

#[test]
fn unset_file_scope_resets_to_inherited() {
    let mut config = AppConfig {
        logging: Some(crate::ports::LogConfig {
            file: Some(crate::ports::FileLogConfig {
                scope: Some(crate::ports::FileLogScope::Project),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    apply_unset(&mut config, "logging.file.scope").unwrap();
    let file = config.logging.unwrap().file.unwrap();
    assert_eq!(file.scope, None);
}

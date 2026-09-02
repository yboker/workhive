#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

// ── EnvLineIssueKind ↔ i18n key mapping ──
//
// Every
// typed reason MUST have a localized key, otherwise the user sees
// a missing-translation placeholder and the warning becomes
// useless.

#[test]
fn env_line_issue_kind_i18n_keys_are_distinct_and_namespaced() {
    use crate::i18n_keys as k;
    let pairs = [
        (
            EnvLineIssueKind::LooksLikeYaml,
            k::OPEN_ENV_PARSE_REASON_LOOKS_LIKE_YAML,
        ),
        (
            EnvLineIssueKind::MissingEquals,
            k::OPEN_ENV_PARSE_REASON_MISSING_EQUALS,
        ),
        (
            EnvLineIssueKind::EmptyKey,
            k::OPEN_ENV_PARSE_REASON_EMPTY_KEY,
        ),
        (
            EnvLineIssueKind::InvalidKeyChars,
            k::OPEN_ENV_PARSE_REASON_INVALID_KEY_CHARS,
        ),
    ];
    for (kind, expected) in pairs {
        assert_eq!(kind.i18n_key(), expected);
        assert!(
            expected.starts_with("open.env.parse_reason."),
            "key '{expected}' must live under the open.env.parse_reason namespace"
        );
    }
    // Distinct keys per variant.
    let mut keys: Vec<&'static str> = pairs.iter().map(|(_, k)| *k).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), pairs.len(), "all reason keys must be unique");
}

// ── session_template must contain {instance_id} ──

#[test]
fn session_template_missing_instance_id_is_rejected() {
    let settings = NamingSettings {
        server_template: None,
        session_template: Some("wh@{project}@{workspace}".to_owned()),
        sanitize: None,
        max_component_len: None,
    };
    let err = settings.validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("{instance_id}"),
        "error should mention missing placeholder: {msg}"
    );
}

#[test]
fn session_template_with_all_required_passes() {
    let settings = NamingSettings {
        server_template: None,
        session_template: Some("wh@{instance_id}@{project}@{workspace}".to_owned()),
        sanitize: None,
        max_component_len: None,
    };
    assert!(settings.validate().is_ok());
}

// ── server_template must contain {instance_id} ──

#[test]
fn server_template_missing_instance_id_is_rejected() {
    let settings = NamingSettings {
        server_template: Some("whsrv-{project}".to_owned()),
        session_template: None,
        sanitize: None,
        max_component_len: None,
    };
    let err = settings.validate().unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("{instance_id}"),
        "error should mention missing placeholder: {msg}"
    );
}

#[test]
fn server_template_with_all_required_passes() {
    let settings = NamingSettings {
        server_template: Some("whsrv-{instance_id}-{project}".to_owned()),
        session_template: None,
        sanitize: None,
        max_component_len: None,
    };
    assert!(settings.validate().is_ok());
}

// ── server_template still rejects {workspace} and {branch} ──

#[test]
fn server_template_rejects_workspace() {
    let settings = NamingSettings {
        server_template: Some("whsrv-{instance_id}-{project}-{workspace}".to_owned()),
        session_template: None,
        sanitize: None,
        max_component_len: None,
    };
    let err = settings.validate().unwrap_err();
    assert!(err.to_string().contains("{workspace}"));
}

#[test]
fn server_template_rejects_branch() {
    let settings = NamingSettings {
        server_template: Some("whsrv-{instance_id}-{project}-{branch}".to_owned()),
        session_template: None,
        sanitize: None,
        max_component_len: None,
    };
    let err = settings.validate().unwrap_err();
    assert!(err.to_string().contains("{branch}"));
}

// ── AppConfig::validate() propagates naming errors ──

#[test]
fn app_config_validate_rejects_bad_session_template() {
    let config = AppConfig {
        tmux: Some(TmuxConfig {
            server_name: "test".to_owned(),
            naming: Some(NamingSettings {
                server_template: None,
                session_template: Some("bad-{project}-{workspace}".to_owned()),
                sanitize: None,
                max_component_len: None,
            }),
        }),
        ..Default::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("{instance_id}"));
}

#[test]
fn app_config_validate_rejects_bad_server_template() {
    let config = AppConfig {
        tmux: Some(TmuxConfig {
            server_name: "test".to_owned(),
            naming: Some(NamingSettings {
                server_template: Some("whsrv-{project}".to_owned()),
                session_template: None,
                sanitize: None,
                max_component_len: None,
            }),
        }),
        ..Default::default()
    };
    let err = config.validate().unwrap_err();
    assert!(err.to_string().contains("{instance_id}"));
}

// ── LogConfig validation ──

#[test]
fn file_log_config_rejects_empty_base_name() {
    let cfg = FileLogConfig {
        base_name: Some(String::new()),
        ..Default::default()
    };
    let err = cfg.validate("logging.file").unwrap_err();
    assert!(err.to_string().contains("base_name"));
}

#[test]
fn file_log_config_rejects_non_dot_log_suffix() {
    let cfg = FileLogConfig {
        base_name: Some("app.txt".to_owned()),
        ..Default::default()
    };
    let err = cfg.validate("logging.file").unwrap_err();
    assert!(err.to_string().contains(".log"));
}

#[test]
fn file_log_config_accepts_dot_log_suffix() {
    let cfg = FileLogConfig {
        base_name: Some("workhive.log".to_owned()),
        ..Default::default()
    };
    assert!(cfg.validate("logging.file").is_ok());
}

#[test]
fn log_rotation_rejects_zero_size() {
    let rotation = LogRotation::SizeMb(0);
    let err = rotation.validate("test.rotation").unwrap_err();
    assert!(err.to_string().contains("must be > 0"));
}

#[test]
fn log_rotation_accepts_positive_size() {
    assert!(LogRotation::SizeMb(10).validate("test.rotation").is_ok());
}

#[test]
fn log_retention_rejects_zero_max_files() {
    let retention = LogRetention::MaxFiles(0);
    let err = retention.validate("test.retention").unwrap_err();
    assert!(err.to_string().contains("must be > 0"));
}

#[test]
fn log_retention_accepts_positive_max_files() {
    assert!(LogRetention::MaxFiles(7).validate("test.retention").is_ok());
}

#[test]
fn log_retention_unlimited_passes() {
    assert!(LogRetention::Unlimited.validate("test.retention").is_ok());
}

#[test]
fn log_config_validate_propagates_file_errors() {
    let config = LogConfig {
        console: None,
        file: Some(FileLogConfig {
            base_name: Some("bad.txt".to_owned()),
            ..Default::default()
        }),
    };
    assert!(config.validate().is_err());
}

#[test]
fn log_config_validate_accepts_valid_config() {
    let config = LogConfig {
        console: Some(ConsoleLogConfig::default()),
        file: Some(FileLogConfig::default()),
    };
    assert!(config.validate().is_ok());
}

// ── LogConfig merge semantics ──

#[test]
fn log_config_merge_global_only() {
    let global = LogConfig {
        console: Some(ConsoleLogConfig {
            level: Some(LogLevel::Debug),
            ..Default::default()
        }),
        file: Some(FileLogConfig::default()),
    };
    let project = LogConfig::default();
    let merged = LogConfig::merge(&global, &project);
    assert_eq!(
        merged.console.as_ref().map(|c| c.level),
        Some(Some(LogLevel::Debug))
    );
    assert!(merged.file.is_some());
}

#[test]
fn log_config_merge_project_overrides_global() {
    let global = LogConfig {
        console: Some(ConsoleLogConfig {
            level: Some(LogLevel::Warn),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = LogConfig {
        console: Some(ConsoleLogConfig {
            level: Some(LogLevel::Trace),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = LogConfig::merge(&global, &project);
    assert_eq!(
        merged.console.as_ref().map(|c| c.level),
        Some(Some(LogLevel::Trace))
    );
}

#[test]
fn log_config_merge_partial_project_console_inherits_global_fields() {
    let global = LogConfig {
        console: Some(ConsoleLogConfig {
            enabled: Some(true),
            level: Some(LogLevel::Warn),
            format: Some(LogFormat::Json),
        }),
        ..Default::default()
    };
    let project = LogConfig {
        console: Some(ConsoleLogConfig {
            level: Some(LogLevel::Trace),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = LogConfig::merge(&global, &project);
    let console = merged.console.expect("console should exist");
    assert_eq!(console.enabled, Some(true));
    assert_eq!(console.level, Some(LogLevel::Trace));
    assert_eq!(console.format, Some(LogFormat::Json));
}

#[test]
fn log_config_merge_project_explicit_disable() {
    let global = LogConfig {
        file: Some(FileLogConfig {
            enabled: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = LogConfig {
        file: Some(FileLogConfig {
            enabled: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = LogConfig::merge(&global, &project);
    assert_eq!(merged.file.as_ref().and_then(|f| f.enabled), Some(false));
}

#[test]
fn log_config_merge_unspecified_inherits() {
    let global = LogConfig {
        file: Some(FileLogConfig {
            level: Some(LogLevel::Trace),
            rotation: Some(LogRotation::Hourly),
            format: Some(LogFormat::Json),
            ..Default::default()
        }),
        ..Default::default()
    };
    let project = LogConfig {
        file: Some(FileLogConfig {
            format: Some(LogFormat::Compact),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = LogConfig::merge(&global, &project);
    let file = merged.file.expect("file should exist");
    assert_eq!(file.level, Some(LogLevel::Trace));
    assert_eq!(file.rotation, Some(LogRotation::Hourly));
    assert_eq!(file.format, Some(LogFormat::Compact));
}

// ── Effective config with defaults ──

#[test]
fn effective_console_returns_defaults_when_none() {
    let config = LogConfig::default();
    let console = config.effective_console();
    assert_eq!(console.level, Some(LogLevel::Warn));
    assert!(console.is_enabled());
}

#[test]
fn effective_file_returns_defaults_when_none() {
    let config = LogConfig::default();
    let file = config.effective_file();
    assert_eq!(file.level, Some(LogLevel::Debug));
    assert_eq!(file.format, Some(LogFormat::Compact));
    assert_eq!(file.rotation, Some(LogRotation::SizeMb(10)));
    assert_eq!(file.retention, Some(LogRetention::MaxFiles(7)));
    assert_eq!(file.base_name.as_deref(), Some("workhive.log"));
    assert!(file.is_enabled());
}

// ── Serde round-trip ──

#[test]
fn log_level_serde_roundtrip() {
    let level = LogLevel::Debug;
    let json = serde_json::to_string(&level).unwrap();
    assert_eq!(json, "\"debug\"");
    let deserialized: LogLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, level);
}

#[test]
fn log_rotation_serde_roundtrip() {
    let rotation = LogRotation::SizeMb(50);
    let json = serde_json::to_string(&rotation).unwrap();
    let deserialized: LogRotation = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, rotation);
}

#[test]
fn log_retention_serde_roundtrip() {
    let retention = LogRetention::MaxFiles(7);
    let json = serde_json::to_string(&retention).unwrap();
    let deserialized: LogRetention = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, retention);
}

#[test]
fn log_rotation_deserializes_legacy_map_shape() {
    let parsed: LogRotation = serde_json::from_str(r#"{"size_mb":25}"#).unwrap();
    assert_eq!(parsed, LogRotation::SizeMb(25));
}

#[test]
fn log_retention_deserializes_legacy_map_shape() {
    let parsed: LogRetention = serde_json::from_str(r#"{"max_files":9}"#).unwrap();
    assert_eq!(parsed, LogRetention::MaxFiles(9));
}

// ── Managed tool exit behavior ──

#[test]
fn managed_tool_exit_behavior_default_is_respawn_shell() {
    assert_eq!(
        ManagedToolExitBehavior::default(),
        ManagedToolExitBehavior::RespawnShell
    );
}

#[test]
fn managed_tool_exit_behavior_parses_canonical_values() {
    assert_eq!(
        ManagedToolExitBehavior::from_str_value("respawn_shell"),
        Some(ManagedToolExitBehavior::RespawnShell)
    );
    assert_eq!(
        ManagedToolExitBehavior::from_str_value("close_pane"),
        Some(ManagedToolExitBehavior::ClosePane)
    );
}

#[test]
fn managed_tool_exit_behavior_rejects_unknown_value() {
    let toml_text = r#"
[pane.exit]
tool_exit_behavior = "kill_session"
"#;
    let err = toml::from_str::<AppConfig>(toml_text).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("kill_session"),
        "deserialization error must surface the offending value: {msg}"
    );
    assert!(
        msg.contains("respawn_shell") && msg.contains("close_pane"),
        "deserialization error must list the accepted values: {msg}"
    );
}

#[test]
fn managed_tool_exit_behavior_serializes_as_canonical_string() {
    let cfg = PaneExitConfig {
        tool_exit_behavior: Some(ManagedToolExitBehavior::ClosePane),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(
        json.contains(r#""tool_exit_behavior":"close_pane""#),
        "unexpected serialization: {json}"
    );
}

#[test]
fn pane_exit_resolved_default_when_field_absent() {
    let cfg = PaneExitConfig::default();
    assert_eq!(
        cfg.resolved_tool_exit_behavior(),
        ManagedToolExitBehavior::RespawnShell
    );
}

#[test]
fn pane_exit_resolved_honors_explicit_close_pane() {
    let cfg = PaneExitConfig {
        tool_exit_behavior: Some(ManagedToolExitBehavior::ClosePane),
    };
    assert_eq!(
        cfg.resolved_tool_exit_behavior(),
        ManagedToolExitBehavior::ClosePane
    );
}

#[test]
fn pane_runtime_config_merge_project_overrides_global() {
    let global = PaneRuntimeConfig {
        exit: Some(PaneExitConfig {
            tool_exit_behavior: Some(ManagedToolExitBehavior::ClosePane),
        }),
    };
    let project = PaneRuntimeConfig {
        exit: Some(PaneExitConfig {
            tool_exit_behavior: Some(ManagedToolExitBehavior::RespawnShell),
        }),
    };
    let merged = PaneRuntimeConfig::merge(Some(&global), Some(&project))
        .expect("merge of two Some values yields Some");
    assert_eq!(
        merged
            .exit
            .expect("exit table preserved")
            .resolved_tool_exit_behavior(),
        ManagedToolExitBehavior::RespawnShell
    );
}

#[test]
fn pane_runtime_config_merge_project_only_field_inherits_others() {
    // Global authors close_pane; project authors a table but leaves
    // the field unset. Project's omission must NOT clobber the
    // global field — project wins only when project sets it.
    let global = PaneRuntimeConfig {
        exit: Some(PaneExitConfig {
            tool_exit_behavior: Some(ManagedToolExitBehavior::ClosePane),
        }),
    };
    let project = PaneRuntimeConfig {
        exit: Some(PaneExitConfig::default()),
    };
    let merged =
        PaneRuntimeConfig::merge(Some(&global), Some(&project)).expect("Some + Some -> Some");
    assert_eq!(
        merged
            .exit
            .expect("exit table preserved")
            .resolved_tool_exit_behavior(),
        ManagedToolExitBehavior::ClosePane
    );
}

#[test]
fn pane_runtime_config_merge_returns_none_when_both_absent() {
    assert!(PaneRuntimeConfig::merge(None, None).is_none());
}

#[test]
fn pane_runtime_config_parses_from_toml_table() {
    let toml_text = r#"
[pane.exit]
tool_exit_behavior = "close_pane"
"#;
    let cfg: AppConfig = toml::from_str(toml_text).expect("parses");
    let resolved = cfg
        .pane
        .as_ref()
        .expect("pane table present")
        .exit
        .as_ref()
        .expect("exit table present")
        .resolved_tool_exit_behavior();
    assert_eq!(resolved, ManagedToolExitBehavior::ClosePane);
}

#[test]
fn app_config_validate_accepts_pane_runtime_config() {
    let cfg = AppConfig {
        pane: Some(PaneRuntimeConfig {
            exit: Some(PaneExitConfig {
                tool_exit_behavior: Some(ManagedToolExitBehavior::RespawnShell),
            }),
        }),
        ..Default::default()
    };
    assert!(cfg.validate().is_ok());
}

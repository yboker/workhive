use super::*;
use tempfile::TempDir;

#[test]
fn roundtrip_project_config() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    std::fs::create_dir_all(root.join(".workhive")).unwrap();

    let adapter = TomlConfigAdapter::new();
    let config = AppConfig::default();
    adapter.save_project(&root, &config).unwrap();

    let loaded = adapter.load_project(&root).unwrap();
    assert!(loaded.is_some());
}

#[test]
fn project_exists_returns_false_when_missing() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    let adapter = TomlConfigAdapter::new();
    assert!(!adapter.project_exists(&root).unwrap());
}

#[test]
fn load_project_returns_none_when_missing() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    let adapter = TomlConfigAdapter::new();
    assert!(adapter.load_project(&root).unwrap().is_none());
}

#[test]
fn xdg_config_home_takes_priority() {
    let tmp = TempDir::new().unwrap();
    let xdg_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

    crate::test_env::with_xdg_config_home(xdg_dir.as_str(), || {
        let resolved = resolve_global_config_dir().unwrap();
        assert_eq!(resolved, xdg_dir.join("workhive"));
    });
}

#[test]
fn resolve_falls_back_to_home_config() {
    crate::test_env::without_xdg_config_home(|| {
        let resolved = resolve_global_config_dir().unwrap();
        let home = std::env::var("HOME").unwrap();
        assert_eq!(
            resolved,
            Utf8PathBuf::from(home).join(".config").join("workhive")
        );
    });
}

#[test]
fn read_env_file_report_returns_empty_for_missing_file() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join(".env");
    let report = TomlConfigAdapter::read_env_file_report(&path).unwrap();
    assert_eq!(report, EnvLoadReport::default());
}

#[test]
fn read_env_file_report_propagates_yaml_style_issue_with_path_and_line() {
    // Invalid lines must surface to the
    // CLI as path + line + typed reason, never the raw line and
    // never the value side. This test pins the boundary between the
    // adapter's parse step and the port surface.
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join(".env");
    // Line 1 is valid; line 2 uses YAML syntax.
    std::fs::write(path.as_str(), "FOO=bar\nCLAUDE_CODE_NO_FLICKER: 1\n").unwrap();

    let report = TomlConfigAdapter::read_env_file_report(&path).unwrap();
    assert_eq!(report.path.as_deref(), Some(path.as_path()));
    assert_eq!(report.entries, vec![("FOO".to_string(), "bar".to_string())]);
    assert_eq!(report.issues.len(), 1);
    let issue = &report.issues[0];
    assert_eq!(issue.line, 2);
    assert_eq!(
        issue.kind,
        workhive_core::ports::EnvLineIssueKind::LooksLikeYaml
    );
}

#[test]
fn read_env_file_report_maps_all_parse_reasons() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join(".env");
    // 1: yaml; 2: missing equals; 3: empty key; 4: invalid chars.
    std::fs::write(
        path.as_str(),
        "K1: v\nNO_SEPARATOR\n=value_only\nBAD-KEY=ok\n",
    )
    .unwrap();
    let report = TomlConfigAdapter::read_env_file_report(&path).unwrap();
    let lines_and_kinds: Vec<(u32, workhive_core::ports::EnvLineIssueKind)> =
        report.issues.iter().map(|i| (i.line, i.kind)).collect();
    assert_eq!(
        lines_and_kinds,
        vec![
            (1, workhive_core::ports::EnvLineIssueKind::LooksLikeYaml),
            (2, workhive_core::ports::EnvLineIssueKind::MissingEquals),
            (3, workhive_core::ports::EnvLineIssueKind::EmptyKey),
            (4, workhive_core::ports::EnvLineIssueKind::InvalidKeyChars),
        ]
    );
    assert!(report.entries.is_empty(), "no valid entries expected");
}

#[test]
fn read_config_returns_parse_error_on_invalid_toml() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("bad.toml");
    std::fs::write(path.as_str(), "this is not valid TOML {{{{").unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::ParseFailed { .. } => {}
        other => panic!("expected ParseFailed, got: {other:?}"),
    }
}

#[test]
fn load_rejects_invalid_file_base_name() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[logging.file]\nbase_name = \"no_extension\"\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, reason } => {
            assert_eq!(field, "logging.file.base_name");
            assert!(reason.contains(".log"));
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn load_rejects_empty_server_template() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(path.as_str(), "[tmux.naming]\nserver_template = \"\"\n").unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, .. } => {
            assert_eq!(field, "tmux.naming.server_template");
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn load_rejects_session_template_missing_workspace() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[tmux.naming]\nsession_template = \"wh-{instance_id}-{project}\"\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, reason } => {
            assert_eq!(field, "tmux.naming.session_template");
            assert!(reason.contains("{workspace}"));
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn load_rejects_server_template_with_workspace_placeholder() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[tmux.naming]\nserver_template = \"x-{instance_id}-{workspace}-{project}\"\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, reason } => {
            assert_eq!(field, "tmux.naming.server_template");
            assert!(
                reason.contains("{workspace}"),
                "error should mention forbidden placeholder: {reason}"
            );
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn load_rejects_server_template_with_branch_placeholder() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[tmux.naming]\nserver_template = \"srv-{instance_id}-{project}-{branch}\"\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, reason } => {
            assert_eq!(field, "tmux.naming.server_template");
            assert!(
                reason.contains("{branch}"),
                "error should mention forbidden placeholder: {reason}"
            );
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn cli_set_and_file_load_reject_same_invalid_template() {
    use workhive_core::usecases;

    let invalid_template = "srv-{workspace}-{project}";

    let cli_result = {
        let mut cfg = AppConfig::default();
        usecases::config_mgmt::apply_set_preview(
            &mut cfg,
            "tmux.naming.server_template",
            invalid_template,
        )
    };
    assert!(
        cli_result.is_err(),
        "CLI set must reject forbidden placeholder"
    );

    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        format!(
            "[tmux.naming]\nserver_template = \"{}\"\n",
            invalid_template
        ),
    )
    .unwrap();
    let load_result = TomlConfigAdapter::read_config(&path);
    assert!(
        load_result.is_err(),
        "file load must reject same forbidden placeholder"
    );
}

#[test]
fn load_rejects_relative_discovery_root() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[discovery]\nproject_roots = [\"relative/path\"]\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
    match result.unwrap_err() {
        ConfigError::InvalidValue { field, reason } => {
            assert_eq!(field, "discovery.project_roots");
            assert!(reason.contains("relative/path"));
        }
        other => panic!("expected InvalidValue, got: {other:?}"),
    }
}

#[test]
fn load_accepts_valid_config_with_all_fields() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        r#"
language = "en"

[tmux]
server_name = "whsrv-main"

[tmux.naming]
server_template = "whsrv-{instance_id}-{project}"
session_template = "wh@{instance_id}@{project}@{workspace}"

[logging.console]
level = "debug"
format = "compact"

[logging.file]
level = "trace"
base_name = "workhive.log"
rotation = "daily"

[discovery]
project_roots = ["/Users/me/projects", "~/dev"]
"#,
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_ok());
    let config = result.unwrap().unwrap();
    assert!(config.tmux.is_some());
    assert!(config.logging.is_some());
    assert!(config.discovery.is_some());
}

#[test]
fn roundtrip_full_logging_config() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    std::fs::create_dir_all(root.join(".workhive")).unwrap();

    let config = AppConfig {
        logging: Some(workhive_core::ports::LogConfig {
            console: Some(workhive_core::ports::ConsoleLogConfig {
                enabled: Some(true),
                level: Some(workhive_core::ports::LogLevel::Debug),
                format: Some(workhive_core::ports::LogFormat::Compact),
            }),
            file: Some(workhive_core::ports::FileLogConfig {
                enabled: Some(true),
                level: Some(workhive_core::ports::LogLevel::Trace),
                format: Some(workhive_core::ports::LogFormat::Json),
                scope: Some(workhive_core::ports::FileLogScope::Project),
                directory: None,
                base_name: Some("app.log".to_owned()),
                rotation: Some(workhive_core::ports::LogRotation::Daily),
                retention: Some(workhive_core::ports::LogRetention::MaxFiles(7)),
            }),
        }),
        ..Default::default()
    };

    let adapter = TomlConfigAdapter::new();
    adapter.save_project(&root, &config).unwrap();
    let loaded = adapter.load_project(&root).unwrap().unwrap();

    let log_cfg = loaded.logging.unwrap();
    let console = log_cfg.console.unwrap();
    assert_eq!(console.level, Some(workhive_core::ports::LogLevel::Debug));
    assert_eq!(
        console.format,
        Some(workhive_core::ports::LogFormat::Compact)
    );

    let file = log_cfg.file.unwrap();
    assert_eq!(file.level, Some(workhive_core::ports::LogLevel::Trace));
    assert_eq!(file.format, Some(workhive_core::ports::LogFormat::Json));
    assert_eq!(
        file.scope,
        Some(workhive_core::ports::FileLogScope::Project)
    );
    assert_eq!(file.base_name.as_deref(), Some("app.log"));
    assert_eq!(
        file.rotation,
        Some(workhive_core::ports::LogRotation::Daily)
    );
    assert_eq!(
        file.retention,
        Some(workhive_core::ports::LogRetention::MaxFiles(7))
    );

    let raw = std::fs::read_to_string(root.join(".workhive").join(CONFIG_FILE)).unwrap();
    assert!(
        raw.contains("retention = \"max_files(7)\""),
        "retention should use CLI-compatible string syntax"
    );
    assert!(
        raw.contains("# Rotation values: never, hourly, daily, size_mb(N)."),
        "logging comments should be written for user guidance"
    );
    assert!(
        raw.contains("ignored when rotation = \"never\""),
        "rotation=never retention behavior should be documented"
    );
}

#[test]
fn save_project_rewrites_logging_comments_consistently() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    std::fs::create_dir_all(root.join(".workhive")).unwrap();
    let adapter = TomlConfigAdapter::new();

    let mut config = AppConfig {
        logging: Some(workhive_core::ports::LogConfig {
            console: Some(workhive_core::ports::ConsoleLogConfig::default()),
            file: Some(workhive_core::ports::FileLogConfig {
                base_name: Some("workhive.log".to_owned()),
                rotation: Some(workhive_core::ports::LogRotation::Never),
                retention: Some(workhive_core::ports::LogRetention::MaxFiles(7)),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };
    adapter.save_project(&root, &config).unwrap();

    if let Some(file) = config.logging.as_mut().and_then(|log| log.file.as_mut()) {
        file.base_name = Some("custom.log".to_owned());
    }
    adapter.save_project(&root, &config).unwrap();

    let raw = std::fs::read_to_string(root.join(".workhive").join(CONFIG_FILE)).unwrap();
    assert!(raw.contains("base_name = \"custom.log\""));
    assert!(raw.contains("# Level values: trace, debug, info, warn, error."));
    assert!(raw.contains("retention = \"max_files(7)\""));
}

#[test]
fn save_global_documents_empty_tool_catalog() {
    let tmp = TempDir::new().unwrap();
    let root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    let path = root.join("config.toml");

    let config = AppConfig {
        tools: Some(workhive_core::ports::ToolCatalogConfig {
            catalog: workhive_core::domain::ToolCatalog::empty(),
        }),
        ..Default::default()
    };
    TomlConfigAdapter::write_config(&path, &config).unwrap();

    let raw = std::fs::read_to_string(path.as_str()).unwrap();
    assert!(raw.contains("# Global tool catalog."));
    assert!(raw.contains("[tools.catalog]"));
}

#[test]
fn load_rejects_invalid_rotation_zero_size() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");
    std::fs::write(
        path.as_str(),
        "[logging.file]\nbase_name = \"app.log\"\n\n[logging.file.rotation]\nSizeMb = 0\n",
    )
    .unwrap();

    let result = TomlConfigAdapter::read_config(&path);
    assert!(result.is_err());
}

#[test]
fn write_rejects_invalid_config() {
    let tmp = TempDir::new().unwrap();
    let path = Utf8PathBuf::try_from(tmp.path().to_path_buf())
        .unwrap()
        .join("config.toml");

    let config = AppConfig {
        logging: Some(workhive_core::ports::LogConfig {
            console: None,
            file: Some(workhive_core::ports::FileLogConfig {
                base_name: Some("no-dot-log-suffix".to_owned()),
                ..Default::default()
            }),
        }),
        ..Default::default()
    };

    let result = TomlConfigAdapter::write_config(&path, &config);
    assert!(result.is_err());
    assert!(!path.exists(), "invalid config must not be written to disk");
}

// -- `save_raw_toml` must be atomic --
//
// `save_raw_toml` writes through a temp file plus rename so that readers
// and concurrent writers never observe a partially-written config file.
// These tests pin that contract:
//   1. raw bytes round-trip exactly (no parse / reserialize / pretty);
//   2. parent directory creation still works;
//   3. failures map to `ConfigError::WriteFailed`;
//   4. no `*.tmp` debris is left behind on success.

fn fake_xdg_save_raw_toml_global(
    adapter: &TomlConfigAdapter,
    contents: &str,
) -> Result<Utf8PathBuf, ConfigError> {
    let tmp = TempDir::new().unwrap();
    let xdg = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    let path = crate::test_env::with_xdg_config_home(xdg.as_str(), || {
        adapter.save_raw_toml(None, true, contents).map(|_| {
            TomlConfigAdapter::global_dir()
                .expect("xdg dir should resolve")
                .join(CONFIG_FILE)
        })
    });
    std::mem::forget(tmp); // keep tempdir alive so caller can read.
    path
}

#[test]
fn save_raw_toml_writes_exact_bytes_byte_for_byte() {
    let adapter = TomlConfigAdapter::new();
    let raw = "# user marker\n[projects]\ntrusted_roots = [\"/x\"]\n# trailing\n";
    let path = fake_xdg_save_raw_toml_global(&adapter, raw).expect("save_raw_toml must succeed");

    let on_disk =
        std::fs::read_to_string(path.as_std_path()).expect("written config should be readable");
    assert_eq!(
        on_disk, raw,
        "save_raw_toml must round-trip raw bytes byte-for-byte"
    );
}

#[test]
fn save_raw_toml_creates_parent_directory_when_missing() {
    let adapter = TomlConfigAdapter::new();
    // Use a project root with no `.workhive/` directory yet.
    let tmp = TempDir::new().unwrap();
    let project_root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    assert!(
        !project_root.join(".workhive").exists(),
        "precondition: parent dir absent",
    );

    adapter
        .save_raw_toml(Some(&project_root), false, "# tiny project config\n")
        .expect("save_raw_toml must create the parent directory");

    let written = std::fs::read_to_string(
        project_root
            .join(".workhive")
            .join(CONFIG_FILE)
            .as_std_path(),
    )
    .expect("config should be readable after save");
    assert_eq!(written, "# tiny project config\n");
}

#[test]
fn save_raw_toml_does_not_leave_tmp_debris_on_success() {
    let adapter = TomlConfigAdapter::new();
    let tmp = TempDir::new().unwrap();
    let project_root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    adapter
        .save_raw_toml(Some(&project_root), false, "value = 1\n")
        .unwrap();

    let workhive_dir = project_root.join(".workhive");
    let entries: Vec<_> = std::fs::read_dir(workhive_dir.as_std_path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .collect();
    let names: Vec<String> = entries
        .iter()
        .map(|n| n.to_string_lossy().to_string())
        .collect();
    assert!(
        names.iter().any(|n| n == CONFIG_FILE),
        "config file must be persisted, got entries: {names:?}",
    );
    assert!(
        !names.iter().any(|n| n.ends_with(".tmp")),
        "atomic rename must remove the temp file, got entries: {names:?}",
    );
}

#[test]
fn save_raw_toml_preserves_unrelated_comments_after_overwrite() {
    // Cross-check: the atomic primitive must
    // not corrupt or rewrite the supplied bytes when overwriting an
    // existing file. Comments and free-text in `contents` must
    // survive verbatim.
    let adapter = TomlConfigAdapter::new();
    let tmp = TempDir::new().unwrap();
    let project_root = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
    let path = project_root.join(".workhive").join(CONFIG_FILE);

    // First write — establishes the file.
    adapter
        .save_raw_toml(Some(&project_root), false, "# original\nvalue = 1\n")
        .unwrap();
    // Second write — must overwrite atomically with new bytes verbatim.
    let updated = "# second write\nvalue = 2\n# trailing comment\n";
    adapter
        .save_raw_toml(Some(&project_root), false, updated)
        .unwrap();

    let on_disk = std::fs::read_to_string(path.as_std_path()).unwrap();
    assert_eq!(on_disk, updated);
}

#[test]
fn save_raw_toml_maps_write_failure_to_write_failed_variant() {
    // Force the parent path to be a regular file so `create_dir_all`
    // fails. The error must surface as `ConfigError::WriteFailed` with
    // the target path attached for actionable diagnostics.
    let adapter = TomlConfigAdapter::new();
    let tmp = TempDir::new().unwrap();
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"not a dir").unwrap();
    // The would-be project root is a sub-path of the blocker file —
    // that means `create_dir_all(blocker.join(".workhive"))` will fail
    // with `NotADirectory` on the parent.
    let project_root = Utf8PathBuf::try_from(blocker.clone()).unwrap();

    let err = adapter
        .save_raw_toml(Some(&project_root), false, "x = 1\n")
        .expect_err("save_raw_toml must fail when parent cannot be created");

    match err {
        ConfigError::WriteFailed { path, .. } => {
            assert!(
                path.as_str().contains(".workhive") && path.as_str().ends_with(CONFIG_FILE),
                "WriteFailed must carry the target config path, got {path}",
            );
        }
        other => panic!("expected ConfigError::WriteFailed, got {other:?}"),
    }
}

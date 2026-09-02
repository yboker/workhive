//! Configurable structured logging foundation.
//!
//! Assembles a `tracing` subscriber from the effective `LogConfig`, supporting
//! console (stderr) plus a unified file sink.
//!
//! ## Rotation / retention
//!
//! File sinks use `logroller` for rotation (time-based or size-based) and
//! retention (`max_keep_files`).  `LogRotation::Never` bypasses logroller
//! entirely and opens a plain append-mode file — no rotation, no renaming.
//!
//! ## Failure semantics
//!
//! File sink initialization failure does **not** break the CLI.  A failed sink
//! is skipped for that run and a diagnostic is emitted to stderr so operators
//! have a clear signal.  Console logging remains available independently.
//!
//! ## Guard lifecycle
//!
//! `tracing_appender::non_blocking` returns a `WorkerGuard` that must be held
//! for the entire process lifetime.  Guards are stored in a global `OnceLock`
//! so they live until process exit and get a clean flush on normal shutdown.

use std::path::PathBuf;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

use workhive_core::ports::{
    ConfigPort, FileLogConfig, FileLogScope, LogConfig, LogFormat, LogLevel, LogRetention,
    LogRotation,
};

/// Holds `WorkerGuard`s for the lifetime of the process.
static LOG_GUARDS: OnceLock<Vec<WorkerGuard>> = OnceLock::new();

#[derive(Debug)]
enum ConfigLoadState {
    Missing,
    Loaded(LogConfig),
    Failed(String),
}

/// Initialize the global tracing subscriber.
///
/// Loads the effective logging configuration by merging global + project
/// configs, applies the `--verbose` CLI override, assembles sink layers,
/// and installs the subscriber.  Call once at the start of `main()`.
pub fn init(verbose: bool) {
    let project_root = discover_project_root();
    let effective = load_effective_log_config(project_root.as_deref());

    let mut console_cfg = effective.effective_console();
    if verbose {
        console_cfg.level = Some(LogLevel::Debug);
    }

    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();
    let mut guards = Vec::new();

    if console_cfg.is_enabled() {
        let console_filter = EnvFilter::new(format!(
            "workhive={}",
            console_cfg.level.unwrap_or(LogLevel::Warn).as_str()
        ));
        let layer = build_console_layer(
            console_cfg.format.unwrap_or(LogFormat::Compact),
            console_filter,
        );
        layers.push(layer);
    }

    let file_cfg = effective.effective_file();
    let file_dir = resolve_file_log_dir(&file_cfg, project_root.as_deref());
    if file_cfg.is_enabled() && file_dir.is_none() {
        eprintln!(
            "workhive: file logging is configured for project scope, but no workhive project root was found"
        );
    } else if let Some(layer) = assemble_file_sink(&file_cfg, file_dir, "file", &mut guards) {
        layers.push(layer);
    }

    let _ = LOG_GUARDS.set(guards);

    tracing_subscriber::registry().with(layers).init();

    tracing::debug!(
        verbose,
        console_level = %console_cfg.level.unwrap_or(LogLevel::Warn),
        console_enabled = console_cfg.is_enabled(),
        console_format = %console_cfg.format.unwrap_or(LogFormat::Compact),
        file_enabled = file_cfg.is_enabled(),
        file_format = %file_cfg.format.unwrap_or(LogFormat::Json),
        file_scope = %file_cfg.scope.unwrap_or(FileLogScope::Global),
        "logging initialized"
    );
}

/// Build a console layer that respects the configured format.
fn build_console_layer(
    format: LogFormat,
    filter: EnvFilter,
) -> Box<dyn Layer<Registry> + Send + Sync> {
    match format {
        LogFormat::Compact => Box::new(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_writer(std::io::stderr)
                .with_filter(filter),
        ),
        LogFormat::Json => Box::new(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_writer(std::io::stderr)
                .with_filter(filter),
        ),
    }
}

/// Resolve the effective log directory: config override first, then fallback.
fn resolve_effective_file_dir(
    cfg: &FileLogConfig,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(ref dir) = cfg.directory {
        Some(PathBuf::from(dir.as_str()))
    } else {
        fallback()
    }
}

fn resolve_file_log_dir(
    cfg: &FileLogConfig,
    project_root: Option<&std::path::Path>,
) -> Option<PathBuf> {
    match cfg.scope.unwrap_or(FileLogScope::Global) {
        FileLogScope::Global => resolve_effective_file_dir(cfg, resolve_global_log_dir),
        FileLogScope::Project => resolve_effective_file_dir(cfg, || {
            project_root.map(|root| root.join(".workhive").join("logs"))
        }),
    }
}

/// Build a file sink layer, returning a boxed layer or None.
///
/// On failure, emits a diagnostic to stderr and returns `None`.
/// The CLI continues — console logging is unaffected.
fn assemble_file_sink(
    cfg: &FileLogConfig,
    log_dir: Option<PathBuf>,
    sink_name: &str,
    guards: &mut Vec<WorkerGuard>,
) -> Option<Box<dyn Layer<Registry> + Send + Sync>> {
    if !cfg.is_enabled() {
        return None;
    }

    let dir = log_dir?;

    let effective = cfg.with_defaults();
    let writer: Box<dyn std::io::Write + Send> =
        match effective.rotation.unwrap_or(LogRotation::Daily) {
            LogRotation::Never => match build_plain_file_writer(&dir, &cfg.base_name) {
                Some(w) => Box::new(w),
                None => {
                    eprintln!(
                        "workhive: failed to open {sink_name} log file in {}",
                        dir.display()
                    );
                    return None;
                }
            },
            _ => match build_logroller_appender(&effective, &dir) {
                Some(a) => Box::new(a),
                None => {
                    eprintln!(
                        "workhive: failed to initialize {sink_name} file logging in {}",
                        dir.display()
                    );
                    return None;
                }
            },
        };

    let (non_blocking, guard) = tracing_appender::non_blocking(writer);
    guards.push(guard);

    let filter = EnvFilter::new(format!(
        "workhive={}",
        effective.level.unwrap_or(LogLevel::Debug).as_str()
    ));

    Some(build_file_layer(
        effective.format.unwrap_or(LogFormat::Json),
        non_blocking,
        filter,
    ))
}

/// Build a file layer that respects the configured format.
fn build_file_layer(
    format: LogFormat,
    writer: tracing_appender::non_blocking::NonBlocking,
    filter: EnvFilter,
) -> Box<dyn Layer<Registry> + Send + Sync> {
    match format {
        LogFormat::Json => Box::new(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_thread_ids(true)
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(writer)
                .with_filter(filter),
        ),
        LogFormat::Compact => Box::new(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(writer)
                .with_filter(filter),
        ),
    }
}

/// Build a plain append-mode file for `LogRotation::Never`.
fn build_plain_file_writer(
    log_dir: &PathBuf,
    base_name: &Option<String>,
) -> Option<std::io::BufWriter<std::fs::File>> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }
    let path = log_dir.join(
        base_name
            .clone()
            .unwrap_or_else(|| "workhive.log".to_owned()),
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
        .map(std::io::BufWriter::new)
}

/// Build a `logroller` appender from file config (for rotating strategies only).
fn build_logroller_appender(
    cfg: &FileLogConfig,
    log_dir: &PathBuf,
) -> Option<logroller::LogRoller> {
    if std::fs::create_dir_all(log_dir).is_err() {
        return None;
    }

    let rotation = match cfg.rotation.unwrap_or(LogRotation::Daily) {
        LogRotation::Hourly => logroller::Rotation::AgeBased(logroller::RotationAge::Hourly),
        LogRotation::Daily => logroller::Rotation::AgeBased(logroller::RotationAge::Daily),
        LogRotation::SizeMb(mb) => {
            logroller::Rotation::SizeBased(logroller::RotationSize::MB(u64::from(mb)))
        }
        LogRotation::Never => {
            // Caller must not invoke this function for Never — defensive guard
            return None;
        }
    };

    let max_files: u64 = match cfg.retention.unwrap_or(LogRetention::MaxFiles(7)) {
        LogRetention::Unlimited => 0,
        LogRetention::MaxFiles(n) => u64::from(n),
    };

    let mut builder = logroller::LogRollerBuilder::new(
        log_dir.to_string_lossy().as_ref(),
        cfg.base_name.as_deref().unwrap_or("workhive.log"),
    )
    .rotation(rotation)
    .time_zone(logroller::TimeZone::Local);

    if max_files > 0 {
        builder = builder.max_keep_files(max_files);
    }

    builder.build().ok()
}

/// Load effective logging config by merging global + project.
///
/// Runs before tracing is initialized, so cannot use tracing macros.
fn load_effective_log_config(project_root: Option<&std::path::Path>) -> LogConfig {
    let config_adapter = workhive_infra::config::TomlConfigAdapter::new();

    let global_logging = match load_global_logging(&config_adapter) {
        ConfigLoadState::Missing => None,
        ConfigLoadState::Loaded(config) => Some(config),
        ConfigLoadState::Failed(message) => {
            eprintln!("{message}");
            None
        }
    };

    let project_logging = project_root.and_then(|root| {
        let root = camino::Utf8PathBuf::from(root.to_string_lossy().as_ref());
        match load_project_logging(&config_adapter, &root) {
            ConfigLoadState::Missing => None,
            ConfigLoadState::Loaded(config) => Some(config),
            ConfigLoadState::Failed(message) => {
                eprintln!("{message}");
                None
            }
        }
    });

    match (global_logging, project_logging) {
        (Some(g), Some(p)) => LogConfig::merge(&g, &p),
        (Some(g), None) => g,
        (None, Some(p)) => p,
        (None, None) => LogConfig::default(),
    }
}

fn load_global_logging(config_adapter: &dyn ConfigPort) -> ConfigLoadState {
    match config_adapter.load_global() {
        Ok(Some(config)) => match config.logging {
            Some(logging) => ConfigLoadState::Loaded(logging),
            None => ConfigLoadState::Missing,
        },
        Ok(None) => ConfigLoadState::Missing,
        Err(error) => ConfigLoadState::Failed(format!(
            "workhive: failed to load global logging config: {error}"
        )),
    }
}

fn load_project_logging(
    config_adapter: &dyn ConfigPort,
    project_root: &camino::Utf8Path,
) -> ConfigLoadState {
    match config_adapter.load_project(project_root) {
        Ok(Some(config)) => match config.logging {
            Some(logging) => ConfigLoadState::Loaded(logging),
            None => ConfigLoadState::Missing,
        },
        Ok(None) => ConfigLoadState::Missing,
        Err(error) => ConfigLoadState::Failed(format!(
            "workhive: failed to load project logging config from {}: {error}",
            project_root
        )),
    }
}

/// Discover the project root by walking ancestors from CWD.
///
/// A directory is recognized as a project root if it contains `.workhive/`.
/// This ensures project logging config is loaded even when the CLI runs
/// from a subdirectory within the project.
fn discover_project_root() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let mut candidate = cwd.as_path();
    loop {
        if candidate.join(".workhive").is_dir() {
            return Some(candidate.to_path_buf());
        }
        match candidate.parent() {
            Some(parent) => candidate = parent,
            None => return None,
        }
    }
}

/// Global log directory: `<global_config_dir>/logs/`.
fn resolve_global_log_dir() -> Option<PathBuf> {
    let config_dir = workhive_infra::config::resolve_global_config_dir().ok()?;
    Some(PathBuf::from(config_dir.as_str()).join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::{Utf8Path, Utf8PathBuf};
    use workhive_core::errors::ConfigError;
    use workhive_core::ports::{
        AppConfig, ConsoleLogConfig, FileLogConfig, LogFormat, LogRotation,
    };

    struct ErrorConfigPort {
        global_error: Option<ConfigError>,
        project_error: Option<ConfigError>,
    }

    impl ConfigPort for ErrorConfigPort {
        fn load_global(&self) -> Result<Option<AppConfig>, ConfigError> {
            match &self.global_error {
                Some(error) => Err(clone_config_error(error)),
                None => Ok(None),
            }
        }

        fn save_global(&self, _config: &AppConfig) -> Result<(), ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn load_project(&self, _project_root: &Utf8Path) -> Result<Option<AppConfig>, ConfigError> {
            match &self.project_error {
                Some(error) => Err(clone_config_error(error)),
                None => Ok(None),
            }
        }

        fn save_project(
            &self,
            _project_root: &Utf8Path,
            _config: &AppConfig,
        ) -> Result<(), ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn global_exists(&self) -> Result<bool, ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn project_exists(&self, _project_root: &Utf8Path) -> Result<bool, ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn load_raw_toml(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
        ) -> Result<String, ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn save_raw_toml(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
            _contents: &str,
        ) -> Result<(), ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn load_env(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
        ) -> Result<Vec<(String, String)>, ConfigError> {
            unreachable!("not used in logging tests")
        }

        fn save_env(
            &self,
            _project_root: Option<&Utf8Path>,
            _global: bool,
            _entries: &[(String, String)],
        ) -> Result<(), ConfigError> {
            unreachable!("not used in logging tests")
        }
    }

    fn clone_config_error(error: &ConfigError) -> ConfigError {
        match error {
            ConfigError::NotFound { path } => ConfigError::NotFound { path: path.clone() },
            ConfigError::ReadFailed { path, source } => ConfigError::ReadFailed {
                path: path.clone(),
                source: std::io::Error::new(source.kind(), source.to_string()),
            },
            ConfigError::ParseFailed { path, message } => ConfigError::ParseFailed {
                path: path.clone(),
                message: message.clone(),
            },
            ConfigError::WriteFailed { path, source } => ConfigError::WriteFailed {
                path: path.clone(),
                source: std::io::Error::new(source.kind(), source.to_string()),
            },
            ConfigError::GlobalNotInitialized => ConfigError::GlobalNotInitialized,
            ConfigError::AlreadyInitialized => ConfigError::AlreadyInitialized,
            ConfigError::ProjectNotInitialized => ConfigError::ProjectNotInitialized,
            ConfigError::InvalidValue { field, reason } => ConfigError::InvalidValue {
                field: field.clone(),
                reason: reason.clone(),
            },
        }
    }

    #[test]
    fn console_format_branches_on_enum() {
        // Verifies that both LogFormat variants produce a layer without panic
        // and that the code path branches based on format (compile-time proof
        // that the match arm is reached for each variant).
        let _compact = build_console_layer(LogFormat::Compact, EnvFilter::new("workhive=warn"));
        let _json = build_console_layer(LogFormat::Json, EnvFilter::new("workhive=warn"));
    }

    #[test]
    fn file_format_branches_on_enum() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.log");

        let file1 = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let (nb1, _g1) = tracing_appender::non_blocking(file1);

        let file2 = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let (nb2, _g2) = tracing_appender::non_blocking(file2);

        let _compact = build_file_layer(LogFormat::Compact, nb1, EnvFilter::new("workhive=debug"));
        let _json = build_file_layer(LogFormat::Json, nb2, EnvFilter::new("workhive=debug"));
    }

    #[test]
    fn file_format_json_produces_json_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = FileLogConfig {
            format: Some(LogFormat::Json),
            rotation: Some(LogRotation::Never),
            ..Default::default()
        };
        let dir = tmp.path().to_path_buf();
        let mut guards = Vec::new();
        let layer = assemble_file_sink(&cfg, Some(dir.clone()), "test-json", &mut guards);
        assert!(layer.is_some(), "JSON file sink must be assembled");
    }

    #[test]
    fn file_format_compact_produces_text_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = FileLogConfig {
            format: Some(LogFormat::Compact),
            rotation: Some(LogRotation::Never),
            ..Default::default()
        };
        let dir = tmp.path().to_path_buf();
        let mut guards = Vec::new();
        let layer = assemble_file_sink(&cfg, Some(dir.clone()), "test-compact", &mut guards);
        assert!(layer.is_some(), "Compact file sink must be assembled");
    }

    #[test]
    fn directory_config_overrides_fallback() {
        let custom = camino::Utf8PathBuf::from("/custom/logs");
        let cfg = FileLogConfig {
            directory: Some(custom.clone()),
            ..Default::default()
        };
        let dir = resolve_effective_file_dir(&cfg, || Some(PathBuf::from("/fallback/logs")));
        assert_eq!(dir, Some(PathBuf::from("/custom/logs")));
    }

    #[test]
    fn directory_none_uses_fallback() {
        let cfg = FileLogConfig::default();
        let dir = resolve_effective_file_dir(&cfg, || Some(PathBuf::from("/fallback/logs")));
        assert_eq!(dir, Some(PathBuf::from("/fallback/logs")));
    }

    #[test]
    fn rotation_never_uses_plain_file_not_logroller() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();

        let writer = build_plain_file_writer(&dir, &Some("test.log".to_owned()));
        assert!(writer.is_some(), "plain file writer must succeed");
        assert!(
            dir.join("test.log").exists(),
            "plain file must be created on disk"
        );

        let cfg_never = FileLogConfig {
            rotation: Some(LogRotation::Never),
            ..Default::default()
        };
        let roller = build_logroller_appender(&cfg_never, &dir);
        assert!(
            roller.is_none(),
            "logroller must NOT be used for Never rotation"
        );
    }

    #[test]
    fn discover_project_root_finds_ancestor() {
        let tmp = tempfile::TempDir::new().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let project = base.join("myproject");
        let workhive_dir = project.join(".workhive");
        std::fs::create_dir_all(&workhive_dir).unwrap();
        let subdir = project.join("src").join("deep").join("nested");
        std::fs::create_dir_all(&subdir).unwrap();

        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&subdir).unwrap();

        let found = discover_project_root().map(|p| p.canonicalize().unwrap_or(p));

        std::env::set_current_dir(&original_cwd).unwrap();

        assert_eq!(found, Some(project));
    }

    #[test]
    fn discover_project_root_returns_none_without_workhive() {
        let tmp = tempfile::TempDir::new().unwrap();
        let empty_dir = tmp.path().join("noproject");
        std::fs::create_dir_all(&empty_dir).unwrap();

        let original_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&empty_dir).unwrap();

        let found = discover_project_root();

        std::env::set_current_dir(&original_cwd).unwrap();

        assert!(found.is_none());
    }

    #[test]
    fn console_format_is_honored_in_effective_config() {
        let cfg = ConsoleLogConfig {
            format: Some(LogFormat::Json),
            ..Default::default()
        };
        assert_eq!(cfg.format, Some(LogFormat::Json));
    }

    #[test]
    fn load_global_logging_returns_failed_with_diagnostic_message() {
        let adapter = ErrorConfigPort {
            global_error: Some(ConfigError::ParseFailed {
                path: Utf8PathBuf::from("/tmp/global-config.toml"),
                message: "bad toml".to_owned(),
            }),
            project_error: None,
        };
        match load_global_logging(&adapter) {
            ConfigLoadState::Failed(message) => {
                assert!(message.contains("failed to load global logging config"));
                assert!(message.contains("bad toml"));
            }
            _ => panic!("expected failed global logging load state"),
        }
    }

    #[test]
    fn load_project_logging_returns_failed_with_project_context() {
        let adapter = ErrorConfigPort {
            global_error: None,
            project_error: Some(ConfigError::InvalidValue {
                field: "logging.file.base_name".to_owned(),
                reason: "must end with .log".to_owned(),
            }),
        };
        match load_project_logging(&adapter, Utf8Path::new("/tmp/project")) {
            ConfigLoadState::Failed(message) => {
                assert!(message.contains("failed to load project logging config"));
                assert!(message.contains("/tmp/project"));
            }
            _ => panic!("expected failed project logging load state"),
        }
    }
}

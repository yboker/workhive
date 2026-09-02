#[allow(unused_imports)]
use camino::{Utf8Path, Utf8PathBuf};
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::domain::{Locale, PaneLaunchRef, ToolCatalog};
#[allow(unused_imports)]
use crate::errors::ConfigError;

/// Log level for filtering tracing events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// Convert to the string accepted by `EnvFilter`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Output format for a log sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable compact text.
    Compact,
    /// Structured JSON (grep/jq-friendly).
    Json,
}

impl std::fmt::Display for LogFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compact => f.write_str("compact"),
            Self::Json => f.write_str("json"),
        }
    }
}

/// Scope for the unified file log sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileLogScope {
    /// Write logs under the global config directory.
    Global,
    /// Write logs under the current project's `.workhive/logs/` directory.
    Project,
}

impl std::fmt::Display for FileLogScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => f.write_str("global"),
            Self::Project => f.write_str("project"),
        }
    }
}

/// Rotation policy for file log sinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRotation {
    /// No rotation — single growing file.
    Never,
    /// Rotate at the given size threshold in megabytes.
    SizeMb(u32),
    /// Rotate every hour.
    Hourly,
    /// Rotate every day (at midnight).
    Daily,
}

impl LogRotation {
    /// Validate that size-based rotation has a positive threshold.
    pub fn validate(&self, field: &str) -> Result<(), crate::errors::ConfigError> {
        if let Self::SizeMb(0) = self {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: "size_mb rotation threshold must be > 0".to_owned(),
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for LogRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Never => f.write_str("never"),
            Self::SizeMb(mb) => write!(f, "size_mb({mb})"),
            Self::Hourly => f.write_str("hourly"),
            Self::Daily => f.write_str("daily"),
        }
    }
}

/// Retention policy for rotated log files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogRetention {
    /// Keep all rotated files (no cleanup).
    Unlimited,
    /// Keep at most N rotated files.
    MaxFiles(u32),
}

impl LogRetention {
    /// Validate retention values.
    pub fn validate(&self, field: &str) -> Result<(), crate::errors::ConfigError> {
        if let Self::MaxFiles(0) = self {
            return Err(crate::errors::ConfigError::InvalidValue {
                field: field.to_owned(),
                reason: "max_files retention must be > 0".to_owned(),
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for LogRetention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unlimited => f.write_str("unlimited"),
            Self::MaxFiles(n) => write!(f, "max_files({n})"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LogRotationSerde {
    String(String),
    SizeMap {
        #[serde(alias = "SizeMb")]
        size_mb: u32,
    },
}

impl From<LogRotation> for LogRotationSerde {
    fn from(value: LogRotation) -> Self {
        Self::String(value.to_string())
    }
}

impl TryFrom<LogRotationSerde> for LogRotation {
    type Error = String;

    fn try_from(value: LogRotationSerde) -> Result<Self, Self::Error> {
        match value {
            LogRotationSerde::String(raw) => parse_log_rotation(&raw),
            LogRotationSerde::SizeMap { size_mb } => Ok(LogRotation::SizeMb(size_mb)),
        }
    }
}

impl Serialize for LogRotation {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        LogRotationSerde::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LogRotation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = LogRotationSerde::deserialize(deserializer)?;
        Self::try_from(repr).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum LogRetentionSerde {
    String(String),
    MaxFilesMap {
        #[serde(alias = "MaxFiles")]
        max_files: u32,
    },
}

impl From<LogRetention> for LogRetentionSerde {
    fn from(value: LogRetention) -> Self {
        Self::String(value.to_string())
    }
}

impl TryFrom<LogRetentionSerde> for LogRetention {
    type Error = String;

    fn try_from(value: LogRetentionSerde) -> Result<Self, Self::Error> {
        match value {
            LogRetentionSerde::String(raw) => parse_log_retention(&raw),
            LogRetentionSerde::MaxFilesMap { max_files } => Ok(LogRetention::MaxFiles(max_files)),
        }
    }
}

impl Serialize for LogRetention {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        LogRetentionSerde::from(*self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LogRetention {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr = LogRetentionSerde::deserialize(deserializer)?;
        Self::try_from(repr).map_err(serde::de::Error::custom)
    }
}

fn parse_log_rotation(raw: &str) -> Result<LogRotation, String> {
    let value = raw.trim();
    match value {
        "never" => Ok(LogRotation::Never),
        "hourly" => Ok(LogRotation::Hourly),
        "daily" => Ok(LogRotation::Daily),
        _ => parse_wrapped_u32(value, "size_mb")
            .map(LogRotation::SizeMb)
            .ok_or_else(|| {
                format!("invalid log rotation '{value}', expected never|hourly|daily|size_mb(N)")
            }),
    }
}

fn parse_log_retention(raw: &str) -> Result<LogRetention, String> {
    let value = raw.trim();
    match value {
        "unlimited" => Ok(LogRetention::Unlimited),
        _ => parse_wrapped_u32(value, "max_files")
            .map(LogRetention::MaxFiles)
            .ok_or_else(|| {
                format!("invalid log retention '{value}', expected unlimited|max_files(N)")
            }),
    }
}

fn parse_wrapped_u32(value: &str, tag: &str) -> Option<u32> {
    let prefix = format!("{tag}(");
    let inner = value.strip_prefix(prefix.as_str())?.strip_suffix(')')?;
    inner.parse::<u32>().ok()
}

/// Console (stderr) log sink configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsoleLogConfig {
    /// Whether the console sink is active. Default: `true`.
    #[serde(default)]
    pub enabled: Option<bool>,

    /// Minimum log level for console output.
    #[serde(default)]
    pub level: Option<LogLevel>,

    /// Output format for the console sink.
    #[serde(default)]
    pub format: Option<LogFormat>,
}

impl ConsoleLogConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_level() -> LogLevel {
        LogLevel::Warn
    }
    fn default_format() -> LogFormat {
        LogFormat::Compact
    }

    /// Whether the sink is effectively enabled (defaults to `true`).
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(Self::default_enabled())
    }

    /// Resolve an effective config by filling defaults for missing fields.
    pub fn with_defaults(&self) -> Self {
        Self {
            enabled: Some(self.is_enabled()),
            level: Some(self.level.unwrap_or(Self::default_level())),
            format: Some(self.format.unwrap_or(Self::default_format())),
        }
    }

    /// Merge project-level console config over global-level.
    pub fn merge(global: &Self, project: &Self) -> Self {
        Self {
            enabled: project.enabled.or(global.enabled),
            level: project.level.or(global.level),
            format: project.format.or(global.format),
        }
    }
}

/// File log sink configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileLogConfig {
    /// Whether the file sink is active. Default: `true`.
    #[serde(default)]
    pub enabled: Option<bool>,

    /// Minimum log level for the file sink.
    #[serde(default)]
    pub level: Option<LogLevel>,

    /// Output format for the file sink.
    #[serde(default)]
    pub format: Option<LogFormat>,

    /// Logical location for the file sink. Default: `global`.
    #[serde(default)]
    pub scope: Option<FileLogScope>,

    /// Override directory for log files. `None` = use default path.
    #[serde(default)]
    pub directory: Option<Utf8PathBuf>,

    /// Base name for the log file (without rotation suffix). Default: `"workhive.log"`.
    #[serde(default)]
    pub base_name: Option<String>,

    /// Rotation policy. Default: `SizeMb(10)`.
    #[serde(default)]
    pub rotation: Option<LogRotation>,

    /// Retention policy. Default: `MaxFiles(7)`.
    #[serde(default)]
    pub retention: Option<LogRetention>,
}

impl FileLogConfig {
    fn default_enabled() -> bool {
        true
    }
    fn default_level() -> LogLevel {
        LogLevel::Debug
    }
    fn default_format() -> LogFormat {
        LogFormat::Compact
    }
    fn default_scope() -> FileLogScope {
        FileLogScope::Global
    }
    fn default_base_name() -> String {
        "workhive.log".to_owned()
    }
    fn default_rotation() -> LogRotation {
        LogRotation::SizeMb(10)
    }
    fn default_retention() -> LogRetention {
        LogRetention::MaxFiles(7)
    }

    /// Whether the sink is effectively enabled (defaults to `true`).
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(Self::default_enabled())
    }

    /// Resolve an effective config by filling defaults for missing fields.
    pub fn with_defaults(&self) -> Self {
        Self {
            enabled: Some(self.is_enabled()),
            level: Some(self.level.unwrap_or(Self::default_level())),
            format: Some(self.format.unwrap_or(Self::default_format())),
            scope: Some(self.scope.unwrap_or(Self::default_scope())),
            directory: self.directory.clone(),
            base_name: Some(
                self.base_name
                    .clone()
                    .unwrap_or_else(Self::default_base_name),
            ),
            rotation: Some(self.rotation.unwrap_or(Self::default_rotation())),
            retention: Some(self.retention.unwrap_or(Self::default_retention())),
        }
    }

    /// Merge project-level file config over global-level.
    pub fn merge(global: &Self, project: &Self) -> Self {
        Self {
            enabled: project.enabled.or(global.enabled),
            level: project.level.or(global.level),
            format: project.format.or(global.format),
            scope: project.scope.or(global.scope),
            directory: project.directory.clone().or(global.directory.clone()),
            base_name: project.base_name.clone().or(global.base_name.clone()),
            rotation: project.rotation.or(global.rotation),
            retention: project.retention.or(global.retention),
        }
    }

    /// Validate file log configuration fields.
    pub fn validate(&self, prefix: &str) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref base_name) = self.base_name {
            if base_name.is_empty() {
                return Err(crate::errors::ConfigError::InvalidValue {
                    field: format!("{prefix}.base_name"),
                    reason: "base_name must not be empty".to_owned(),
                });
            }
            if !base_name.ends_with(".log") {
                return Err(crate::errors::ConfigError::InvalidValue {
                    field: format!("{prefix}.base_name"),
                    reason: format!("'{base_name}' must end with .log"),
                });
            }
        }
        if let Some(rotation) = self.rotation {
            rotation.validate(&format!("{prefix}.rotation"))?;
        }
        if let Some(retention) = self.retention {
            retention.validate(&format!("{prefix}.retention"))?;
        }
        Ok(())
    }
}

fn merge_optional_console(
    global: &Option<ConsoleLogConfig>,
    project: &Option<ConsoleLogConfig>,
) -> Option<ConsoleLogConfig> {
    match (global, project) {
        (Some(g), Some(p)) => Some(ConsoleLogConfig::merge(g, p)),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), None) => Some(g.clone()),
        (None, None) => None,
    }
}

fn merge_optional_file(
    global: &Option<FileLogConfig>,
    project: &Option<FileLogConfig>,
) -> Option<FileLogConfig> {
    match (global, project) {
        (Some(g), Some(p)) => Some(FileLogConfig::merge(g, p)),
        (None, Some(p)) => Some(p.clone()),
        (Some(g), None) => Some(g.clone()),
        (None, None) => None,
    }
}

/// Logging configuration — typed, layered, configurable.
///
/// Supports two independent sinks: console (stderr) and file.
/// The file sink chooses its logical location via [`FileLogScope`].
/// Project config overrides global config via [`LogConfig::merge`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogConfig {
    /// Console (stderr) sink. `None` = use defaults.
    #[serde(default)]
    pub console: Option<ConsoleLogConfig>,

    /// Unified file sink. `None` = use defaults.
    #[serde(default)]
    pub file: Option<FileLogConfig>,
}

impl LogConfig {
    /// Resolve effective console config (with defaults).
    pub fn effective_console(&self) -> ConsoleLogConfig {
        self.console.clone().unwrap_or_default().with_defaults()
    }

    /// Resolve effective file config (with defaults).
    pub fn effective_file(&self) -> FileLogConfig {
        self.file.clone().unwrap_or_default().with_defaults()
    }

    /// Merge project-level logging config over global-level.
    pub fn merge(global: &LogConfig, project: &LogConfig) -> LogConfig {
        LogConfig {
            console: merge_optional_console(&global.console, &project.console),
            file: merge_optional_file(&global.file, &project.file),
        }
    }

    /// Validate all configured sinks.
    pub fn validate(&self) -> Result<(), crate::errors::ConfigError> {
        if let Some(ref file) = self.file {
            file.validate("logging.file")?;
        }
        Ok(())
    }
}

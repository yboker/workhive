//! `init` use case — global and project initialization.
//!
//! Project initialization is transactional:
//! 1. Acquire cross-process OS lock (`.workhive.init.lock` in project root).
//! 2. Re-check state under lock; skip if already initialized.
//! 3. Write all artifacts to a staging directory (`.workhive.staging.{pid}.{suffix}`).
//! 4. Atomic rename staging → `.workhive`.
//! 5. Update project index after commit; failures are returned while `.workhive/` remains intact.
//! 6. Release lock via RAII guard drop.
//!
//! On failure before commit, the staging RAII guard removes the temp dir.

use camino::Utf8PathBuf;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::Locale;
use crate::errors::{ConfigError, IndexError, ProjectInitError, WorkspaceFsError};
use crate::ports::{
    AppConfig, ConfigPort, ProjectIndexPort, ProjectInitLockPort, ProjectSummary, WorkspaceFsPort,
};

/// Errors from the `init_project` use case.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    WorkspaceFs(#[from] WorkspaceFsError),

    #[error(transparent)]
    Index(#[from] IndexError),

    #[error("system clock is before UNIX epoch — cannot generate instance ID")]
    SystemClock,
}

/// Generate a 4-character hex instance ID for project isolation.
///
/// Uses system time + process ID as entropy source (no external crate needed).
/// Each `init` produces a unique ID; `destroy` removes it.
pub fn generate_instance_id() -> Result<String, InitError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| InitError::SystemClock)?
        .subsec_nanos();
    let pid = std::process::id();
    let combined = nanos.wrapping_mul(31).wrapping_add(pid);
    Ok(format!("{:04x}", combined & 0xFFFF))
}

/// Initialize global configuration at `~/.config/workhive/`.
/// Accepts a fully-constructed `AppConfig` built by the CLI wizard.
pub fn init_global(config_port: &dyn ConfigPort, config: AppConfig) -> Result<(), ConfigError> {
    if config_port.global_exists()? {
        return Err(ConfigError::AlreadyInitialized);
    }
    config_port.save_global(&config)?;
    tracing::info!("global config initialized");
    Ok(())
}

/// Reinitialize global config even if it already exists.
/// Accepts a fully-constructed `AppConfig` built by the CLI wizard.
pub fn init_global_force(
    config_port: &dyn ConfigPort,
    config: AppConfig,
) -> Result<(), ConfigError> {
    config_port.save_global(&config)?;
    tracing::info!("global config (re)initialized");
    Ok(())
}

/// Initialize a project for workhive management (non-transactional path).
///
/// **Callers outside tests should prefer `init_project_transactional`.**
///
/// Inherits global defaults (e.g., language) into the new project config.
/// If `language_override` is `Some`, it takes precedence over the global default.
pub fn init_project(
    config_port: &dyn ConfigPort,
    ws_fs: &dyn WorkspaceFsPort,
    index: &dyn ProjectIndexPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
    language_override: Option<Locale>,
) -> Result<(), InitError> {
    if !config_port.global_exists()? {
        return Err(ConfigError::GlobalNotInitialized.into());
    }

    let global_config = config_port.load_global()?.unwrap_or_default();
    let language = language_override.or(global_config.language);
    let project_config = AppConfig {
        language,
        layout: global_config.layout,
        snapshot: global_config.snapshot,
        ..AppConfig::default()
    };

    ws_fs.create_workhive_dir(project_root.as_ref())?;

    if ws_fs.read_instance_id(project_root.as_ref()).is_none() {
        let id = generate_instance_id()?;
        ws_fs.write_instance_id(project_root.as_ref(), &id)?;
        tracing::info!(instance_id = %id, "generated new instance_id");
    }

    config_port.save_project(project_root.as_ref(), &project_config)?;

    index.upsert_project(&ProjectSummary {
        project_name: project_name.to_owned(),
        project_root: project_root.clone(),
        config_path: project_root.join(".workhive/config.toml"),
        last_opened_at: None,
        last_updated_at: None,
    })?;

    tracing::info!(project = project_name, ?language, "project initialized");
    Ok(())
}

/// Transactional project initialization with cross-process locking and staging.
///
/// Both `workhive init` and `workhive workspace open --init` should call this to ensure
/// consistent locking, staging, and rollback behavior.
///
/// Steps:
/// 1. Acquire cross-process lock.
/// 2. Re-check initialization state under lock.
/// 3. Build artifacts in a staging directory.
/// 4. Atomic-rename staging to `.workhive`.
/// 5. Update project index (failure is recoverable).
///
/// Returns `true` if initialization was performed, `false` if already initialized.
pub fn init_project_transactional(
    config_port: &dyn ConfigPort,
    ws_fs: &dyn WorkspaceFsPort,
    index: &dyn ProjectIndexPort,
    lock_port: &dyn ProjectInitLockPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
    language_override: Option<Locale>,
) -> Result<bool, ProjectInitError> {
    if !config_port.global_exists()? {
        return Err(ConfigError::GlobalNotInitialized.into());
    }

    let global_config = config_port.load_global()?.unwrap_or_default();
    let language = language_override.or(global_config.language);
    let project_config = AppConfig {
        language,
        layout: global_config.layout,
        snapshot: global_config.snapshot,
        ..AppConfig::default()
    };

    init_project_transactional_with_config(
        config_port,
        ws_fs,
        index,
        lock_port,
        project_root,
        project_name,
        project_config,
    )
}

/// Transactional project initialization using an explicit project config.
///
/// This is used by `workhive init`, where the wizard has already produced the
/// exact project configuration that must be committed atomically.
pub fn init_project_transactional_with_config(
    config_port: &dyn ConfigPort,
    ws_fs: &dyn WorkspaceFsPort,
    index: &dyn ProjectIndexPort,
    lock_port: &dyn ProjectInitLockPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
    project_config: AppConfig,
) -> Result<bool, ProjectInitError> {
    if !config_port.global_exists()? {
        return Err(ConfigError::GlobalNotInitialized.into());
    }

    tracing::info!(
        project = project_name,
        root = %project_root,
        "starting transactional project initialization"
    );

    let _lock_guard = lock_port.acquire_init_lock(project_root.as_ref())?;
    tracing::info!(project = project_name, "acquired init lock");

    let state = crate::usecases::project_state::resolve_init_state(ws_fs, project_root.as_ref());

    match state {
        crate::domain::ProjectInitState::Initialized { instance_id } => {
            if config_port.project_exists(project_root.as_ref())? {
                tracing::info!(
                    project = project_name,
                    instance_id = %instance_id,
                    "project already initialized, skipping"
                );
                return Ok(false);
            }

            tracing::info!(
                project = project_name,
                instance_id = %instance_id,
                "project has runtime state but no config.toml; completing explicit init"
            );
            config_port.save_project(project_root.as_ref(), &project_config)?;
            index.upsert_project(&ProjectSummary {
                project_name: project_name.to_owned(),
                project_root: project_root.clone(),
                config_path: project_root.join(".workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })?;
            return Ok(true);
        }
        crate::domain::ProjectInitState::PartiallyInitialized { .. } => {
            tracing::warn!(
                project = project_name,
                ?state,
                "detected partial initialization, removing before re-init"
            );
            ws_fs.remove_workhive_dir(project_root.as_ref())?;
        }
        crate::domain::ProjectInitState::Uninitialized => {}
    }

    let language = project_config.language;

    let staging_dir_name = format!(
        ".workhive.staging.{}.{}",
        std::process::id(),
        generate_staging_suffix()?
    );
    let staging_root = project_root.join(&staging_dir_name);

    tracing::debug!(staging = %staging_root, "creating staging directory");

    let mut staging_guard = StagingCleanupGuard::new(ws_fs, staging_root.clone());

    ws_fs.create_workhive_dir(staging_root.as_ref())?;

    let instance_id = generate_instance_id()?;
    ws_fs.write_instance_id(staging_root.as_ref(), &instance_id)?;
    tracing::info!(instance_id = %instance_id, "generated instance_id in staging");

    config_port.save_project(staging_root.as_ref(), &project_config)?;
    tracing::debug!("wrote project config to staging");

    ws_fs
        .commit_staged_workhive(staging_root.as_ref(), project_root.as_ref())
        .map_err(|e| ProjectInitError::CommitFailed {
            root: project_root.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;

    staging_guard.disarm();
    tracing::info!(
        project = project_name,
        "committed staging to .workhive (atomic rename)"
    );

    ws_fs.cleanup_staging(staging_root.as_ref());

    index.upsert_project(&ProjectSummary {
        project_name: project_name.to_owned(),
        project_root: project_root.clone(),
        config_path: project_root.join(".workhive/config.toml"),
        last_opened_at: None,
        last_updated_at: None,
    })?;

    tracing::info!(
        project = project_name,
        instance_id = %instance_id,
        ?language,
        "project initialized (transactional)"
    );
    Ok(true)
}

/// Runtime-only project initialization for `workhive workspace open` auto-init.
///
/// Creates `.workhive/` with runtime directories and instance_id, but does
/// NOT write `.workhive/config.toml`. The project config is only created by
/// explicit `workhive init`.
///
/// Returns `true` if initialization was performed, `false` if already initialized.
pub fn init_project_runtime_only(
    config_port: &dyn ConfigPort,
    ws_fs: &dyn WorkspaceFsPort,
    index: &dyn ProjectIndexPort,
    lock_port: &dyn ProjectInitLockPort,
    project_root: &Utf8PathBuf,
    project_name: &str,
) -> Result<bool, ProjectInitError> {
    if !config_port.global_exists()? {
        return Err(ConfigError::GlobalNotInitialized.into());
    }

    tracing::info!(
        project = project_name,
        root = %project_root,
        "starting runtime-only project initialization (no config.toml)"
    );

    let _lock_guard = lock_port.acquire_init_lock(project_root.as_ref())?;
    tracing::info!(project = project_name, "acquired init lock");

    let state = crate::usecases::project_state::resolve_init_state(ws_fs, project_root.as_ref());

    match state {
        crate::domain::ProjectInitState::Initialized { instance_id } => {
            tracing::info!(
                project = project_name,
                instance_id = %instance_id,
                "project already initialized; refreshing global index entry"
            );
            // The `.workhive/` runtime already exists, but the global index
            // entry can be absent or stale independent of runtime
            // initialization (for example after an index rebuild or a project
            // relocation). Refresh it idempotently so a successful open always
            // leaves the project discoverable, without re-running
            // initialization. An index write failure propagates instead of
            // being silently swallowed, since it would otherwise break global
            // discovery.
            index.upsert_project(&ProjectSummary {
                project_name: project_name.to_owned(),
                project_root: project_root.clone(),
                config_path: project_root.join(".workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })?;
            return Ok(false);
        }
        crate::domain::ProjectInitState::PartiallyInitialized { .. } => {
            tracing::warn!(
                project = project_name,
                ?state,
                "detected partial initialization, removing before re-init"
            );
            ws_fs.remove_workhive_dir(project_root.as_ref())?;
        }
        crate::domain::ProjectInitState::Uninitialized => {}
    }

    let staging_dir_name = format!(
        ".workhive.staging.{}.{}",
        std::process::id(),
        generate_staging_suffix()?
    );
    let staging_root = project_root.join(&staging_dir_name);

    tracing::debug!(staging = %staging_root, "creating staging directory (runtime-only)");

    let mut staging_guard = StagingCleanupGuard::new(ws_fs, staging_root.clone());

    ws_fs.create_workhive_dir(staging_root.as_ref())?;

    let instance_id = generate_instance_id()?;
    ws_fs.write_instance_id(staging_root.as_ref(), &instance_id)?;
    tracing::info!(instance_id = %instance_id, "generated instance_id in staging (runtime-only)");

    ws_fs
        .commit_staged_workhive(staging_root.as_ref(), project_root.as_ref())
        .map_err(|e| ProjectInitError::CommitFailed {
            root: project_root.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;

    staging_guard.disarm();
    tracing::info!(
        project = project_name,
        "committed staging to .workhive (runtime-only, no config.toml)"
    );

    ws_fs.cleanup_staging(staging_root.as_ref());

    ws_fs.ensure_gitignore_workhive_entry(project_root.as_ref())?;

    index.upsert_project(&ProjectSummary {
        project_name: project_name.to_owned(),
        project_root: project_root.clone(),
        config_path: project_root.join(".workhive/config.toml"),
        last_opened_at: None,
        last_updated_at: None,
    })?;

    tracing::info!(
        project = project_name,
        instance_id = %instance_id,
        "project runtime initialized (no config.toml)"
    );
    Ok(true)
}

fn generate_staging_suffix() -> Result<String, InitError> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| InitError::SystemClock)?
        .as_nanos();
    let random = generate_instance_id()?;
    Ok(format!("{nanos:x}.{random}"))
}

/// RAII guard that cleans up the staging directory on drop (rollback).
///
/// After a successful commit, call `disarm()` to prevent rollback cleanup.
struct StagingCleanupGuard<'a> {
    ws_fs: &'a dyn WorkspaceFsPort,
    path: Utf8PathBuf,
    armed: bool,
}

impl<'a> StagingCleanupGuard<'a> {
    fn new(ws_fs: &'a dyn WorkspaceFsPort, path: Utf8PathBuf) -> Self {
        Self {
            ws_fs,
            path,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingCleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            tracing::warn!(
                staging = %self.path,
                "rolling back staged project initialization"
            );
            self.ws_fs.cleanup_staging(self.path.as_ref());
        }
    }
}

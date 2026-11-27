//! `workhive update` command.
//!
//! Downloads and installs workhive releases from GitHub.
//! Supports macOS and Linux (including WSL).
//!
//! # Features
//!
//! - Automatic platform detection (macOS x86_64/aarch64, Linux x86_64)
//! - SHA256 checksum verification for security
//! - Dry-run mode to preview changes
//! - Pre-release version support
//! - Specific version installation
//! - Progress indicators
//! - Atomic binary replacement with rollback on failure

use std::env;

use self_update::update::ReleaseUpdate;
use workhive_core::i18n_keys as k;
use workhive_core::ports::I18nPort;

use crate::context::CommandContext;
use crate::error::CliError;

/// GitHub repository for releases (format: "owner/repo").
const BIN_NAME: &str = "workhive";

/// Determine the target triple for the current platform.
fn target_triple() -> Result<&'static str, CliError> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    match (os, arch) {
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        _ => Err(CliError::Runtime(format!(
            "unsupported platform: {} {}. Supported: macOS (x86_64, aarch64), Linux (x86_64)",
            os, arch
        ))),
    }
}

/// Build the update configuration.
///
/// Asset naming follows release.yml: `workhive-{version}-{target}.tar.gz`
/// where version includes the 'v' prefix (e.g., "v2.0.1").
///
/// Supports authentication via `GITHUB_TOKEN` or `WORKHIVE_GITHUB_TOKEN` environment
/// variables for private repositories.
fn build_update_config(
    current_version: &str,
    version: Option<&str>,
    prerelease: bool,
) -> Result<Box<dyn ReleaseUpdate>, CliError> {
    let target = target_triple()?;

    let mut builder = self_update::backends::github::Update::configure();

    builder
        .repo_owner("yboker")
        .repo_name("workhive")
        .bin_name(BIN_NAME)
        .target(target)
        .current_version(current_version)
        .show_output(true)
        .no_confirm(false);

    // Support authentication for private repositories
    // Try WORKHIVE_GITHUB_TOKEN first, then fall back to GITHUB_TOKEN
    if let Some(token) = env::var("WORKHIVE_GITHUB_TOKEN")
        .ok()
        .or_else(|| env::var("GITHUB_TOKEN").ok())
    {
        builder.auth_token(&token);
    }

    if prerelease {
        builder.identifier("prerelease");
    }

    if let Some(v) = version {
        builder.target_version_tag(&format!("v{}", v));
    }

    builder
        .build()
        .map_err(|e| CliError::Runtime(format!("failed to build update config: {}", e)))
}

/// Check for updates without installing.
///
/// Supports authentication via `GITHUB_TOKEN` or `WORKHIVE_GITHUB_TOKEN` environment
/// variables for private repositories.
fn check_for_update(current_version: &str, prerelease: bool) -> Result<Option<String>, CliError> {
    let mut builder = self_update::backends::github::Update::configure();

    builder
        .repo_owner("yboker")
        .repo_name("workhive")
        .bin_name(BIN_NAME)
        .target(target_triple()?)
        .current_version(current_version)
        .show_output(false);

    // Support authentication for private repositories
    if let Some(token) = env::var("WORKHIVE_GITHUB_TOKEN")
        .ok()
        .or_else(|| env::var("GITHUB_TOKEN").ok())
    {
        builder.auth_token(&token);
    }

    if prerelease {
        builder.identifier("prerelease");
    }

    let updater = builder
        .build()
        .map_err(|e| CliError::Runtime(format!("failed to check for updates: {}", e)))?;

    let latest = updater
        .get_latest_release()
        .map_err(|e| CliError::Runtime(format!("failed to fetch latest release: {}", e)))?;

    // Compare versions
    let current = current_version.trim_start_matches('v');
    let latest_version = latest.version.trim_start_matches('v');

    if current < latest_version {
        Ok(Some(latest.version))
    } else {
        Ok(None)
    }
}

/// Main entry point for the update command.
pub(crate) fn cmd_update(
    force: bool,
    dry_run: bool,
    prerelease: bool,
    version: Option<String>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();
    let current_version = env!("CARGO_PKG_VERSION");

    // Check for updates first
    if version.is_none() {
        eprintln!(
            "{}",
            ctx.i18n.resolve(locale, k::UPDATE_CHECKING_FOR_UPDATES)
        );

        match check_for_update(current_version, prerelease) {
            Ok(Some(latest)) => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve_with_args(
                        locale,
                        k::UPDATE_AVAILABLE,
                        &[current_version, &latest, ""]
                    )
                );
            }
            Ok(None) => {
                eprintln!(
                    "{}",
                    ctx.i18n.resolve_with_args(
                        locale,
                        k::UPDATE_ALREADY_UP_TO_DATE,
                        &[current_version]
                    )
                );
                return Ok(());
            }
            Err(e) => {
                // Provide helpful hint for private repositories
                let error_msg = format!("{}", e);
                if error_msg.contains("403") || error_msg.contains("404") {
                    return Err(CliError::Runtime(format!(
                        "{}\n\nFor private repositories, set GITHUB_TOKEN or WORKHIVE_GITHUB_TOKEN:\n  export GITHUB_TOKEN=your_token\n  workhive update",
                        error_msg
                    )));
                }
                return Err(e);
            }
        }
    }

    // If dry-run, show what would happen and exit
    if dry_run {
        eprintln!("{}", ctx.i18n.resolve(locale, k::UPDATE_DRY_RUN_MESSAGE));
        if let Some(v) = &version {
            eprintln!(
                "  {} {}",
                ctx.i18n.resolve(locale, k::UPDATE_WOULD_INSTALL),
                v
            );
        } else {
            eprintln!(
                "  {}",
                ctx.i18n.resolve(locale, k::UPDATE_WOULD_INSTALL_LATEST)
            );
        }
        return Ok(());
    }

    // Confirm unless force is set
    if !force {
        eprintln!("{}", ctx.i18n.resolve(locale, k::UPDATE_CONFIRM_PROMPT));
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| CliError::Runtime(format!("failed to read input: {}", e)))?;
        let input = input.trim().to_lowercase();
        if input != "y" && input != "yes" {
            eprintln!("{}", ctx.i18n.resolve(locale, k::UPDATE_CANCELLED));
            return Ok(());
        }
    }

    // Build update configuration
    let config = build_update_config(current_version, version.as_deref(), prerelease)?;

    eprintln!(
        "{}",
        ctx.i18n.resolve(locale, k::UPDATE_DOWNLOADING_INSTALLING)
    );

    // Perform the update
    // Note: self_update handles downloading, extracting, and atomic replacement
    // Checksum verification is built into self_update when checksum files are available
    let status = config
        .update()
        .map_err(|e| {
            CliError::Runtime(format!(
                "update failed: {}\n\nThe binary has not been modified. You can retry with: workhive update",
                e
            ))
        })?;

    if status.version() == current_version {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::UPDATE_ALREADY_UP_TO_DATE, &[current_version])
        );
    } else {
        eprintln!(
            "{}",
            ctx.i18n
                .resolve_with_args(locale, k::UPDATE_SUCCESS, &[status.version()])
        );
    }

    Ok(())
}

//! Dependency preflight checks and the config overwrite confirmation.

use super::*;

// ══════════════════════════════════════════════════════════════════════════════
// Dependency check
// ══════════════════════════════════════════════════════════════════════════════

/// Check dependencies with full output (used by the init wizard).
pub fn check_dependencies(
    deps: &dyn DependencyPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(), CliError> {
    check_dependencies_impl(deps, i18n, locale, true)
}

/// Check dependencies silently — only prints output if a dependency is
/// missing or too old. Used by preflight checks for non-init commands.
pub fn check_dependencies_quiet(
    deps: &dyn DependencyPort,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(), CliError> {
    check_dependencies_impl(deps, i18n, locale, false)
}

/// Localized preflight lines for a set of dependency statuses plus the
/// overall failure flag.
pub(super) struct DependencyReport {
    pub(super) lines: Vec<String>,
    pub(super) has_failure: bool,
}

/// Build the localized dependency preflight lines.
///
/// Missing and version-too-low dependencies always contribute their
/// failure line, the install-hint heading, and the platform-specific
/// install command. Satisfied/built-in dependencies contribute a line only
/// in verbose mode. `has_failure` is true when any hard dependency is
/// unavailable, which the caller maps to a blocking error.
pub(super) fn build_dependency_report(
    results: &[DependencyStatus],
    i18n: &StaticI18n,
    locale: Locale,
    verbose: bool,
) -> DependencyReport {
    let mut lines = Vec::new();
    let mut has_failure = false;

    for status in results {
        match status {
            DependencyStatus::Satisfied { kind, version } => {
                if verbose {
                    let name = dep_display_name(*kind, i18n, locale);
                    lines.push(i18n.resolve_with_args(locale, k::DEP_SATISFIED, &[&name, version]));
                }
            }
            DependencyStatus::Builtin { kind } => {
                if verbose {
                    let name = dep_display_name(*kind, i18n, locale);
                    lines.push(i18n.resolve_with_args(locale, k::DEP_BUILTIN, &[&name]));
                }
            }
            DependencyStatus::Missing { kind } => {
                let name = dep_display_name(*kind, i18n, locale);
                lines.push(i18n.resolve_with_args(locale, k::DEP_MISSING, &[&name]));
                lines.push(i18n.resolve_with_args(locale, k::DEP_INSTALL_HINT, &[&name]));
                lines.push(workhive_infra::dependency::install_hint(*kind));
                has_failure = true;
            }
            DependencyStatus::VersionTooLow {
                kind,
                found,
                minimum,
            } => {
                let name = dep_display_name(*kind, i18n, locale);
                lines.push(i18n.resolve_with_args(
                    locale,
                    k::DEP_VERSION_LOW,
                    &[&name, found, minimum],
                ));
                lines.push(i18n.resolve_with_args(locale, k::DEP_INSTALL_HINT, &[&name]));
                lines.push(workhive_infra::dependency::install_hint(*kind));
                has_failure = true;
            }
        }
    }

    DependencyReport { lines, has_failure }
}

pub(super) fn check_dependencies_impl(
    deps: &dyn DependencyPort,
    i18n: &StaticI18n,
    locale: Locale,
    verbose: bool,
) -> Result<(), CliError> {
    if verbose {
        eprintln!("{}", i18n.resolve(locale, k::DEP_CHECKING));
    }

    let results = deps.check_all();
    let report = build_dependency_report(&results, i18n, locale, verbose);

    for line in &report.lines {
        eprintln!("{line}");
    }

    if report.has_failure {
        eprintln!();
        return Err(CliError::Runtime(
            i18n.resolve(locale, k::DEP_FATAL_MISSING),
        ));
    }

    if verbose {
        eprintln!("{}", i18n.resolve(locale, k::DEP_ALL_OK));
    }
    Ok(())
}

pub(super) fn dep_display_name(kind: DependencyKind, i18n: &StaticI18n, locale: Locale) -> String {
    match kind {
        DependencyKind::Git => i18n.resolve(locale, k::DEP_GIT_NAME),
        DependencyKind::Tmux => i18n.resolve(locale, k::DEP_TMUX_NAME),
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Overwrite check
// ══════════════════════════════════════════════════════════════════════════════

pub fn check_overwrite(
    config_exists: bool,
    config_path: &str,
    i18n: &StaticI18n,
    locale: Locale,
) -> Result<(), CliError> {
    let (prompt, default_choice) = if config_exists {
        (
            i18n.resolve_with_args(locale, k::INIT_CONFIG_EXISTS_OVERWRITE, &[config_path]),
            false,
        )
    } else {
        (
            i18n.resolve_with_args(locale, k::INIT_CONFIG_CREATE_CONFIRM, &[config_path]),
            true,
        )
    };

    let proceed = confirm_with_transcript(i18n, locale, &prompt, default_choice)?;

    if proceed {
        if config_exists {
            tracing::info!(
                path = config_path,
                "user chose to overwrite existing config"
            );
        } else {
            tracing::info!(path = config_path, "user chose to create new config");
        }
        Ok(())
    } else {
        eprintln!("{}", i18n.resolve(locale, k::INIT_CONFIG_ABORT));
        Err(CliError::UserAborted)
    }
}

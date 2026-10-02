#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use crate::commands::*;
use workhive_core::ports::{
    ConfigPort, DiscoveredProject, DiscoveryWarning, ProjectDiscoveryPort, WorkspaceIdentityPort,
};

fn check_backend_config_dependencies(
    backend: &workhive_core::domain::RuntimeBackendId,
    ctx: &CommandContext,
    locale: workhive_core::domain::Locale,
) -> Result<(), CliError> {
    if *backend == workhive_core::domain::RuntimeBackendId::herdr() {
        wizard::check_dependency_binaries_for_backend(&*ctx.deps, backend, &ctx.i18n, locale, true)
    } else {
        wizard::check_dependencies_for_backend(&*ctx.deps, backend, &ctx.i18n, locale, true)
    }
}

fn discover_initialized_projects(
    config: &dyn ConfigPort,
    discovery: &dyn ProjectDiscoveryPort,
    home: Option<&camino::Utf8Path>,
) -> Result<Vec<DiscoveredProject>, CliError> {
    let global = config.load_global()?;
    let roots =
        workhive_core::usecases::trusted_roots::trusted_roots_from_config(global.as_ref(), home);
    let report = discovery.discover_projects_report(roots.roots())?;
    if let Some(DiscoveryWarning::UnreadableDescendant { path, reason }) = report.warnings.first() {
        return Err(CliError::Runtime(format!(
            "cannot determine global backend impact because '{path}' could not be read: {reason}"
        )));
    }
    Ok(report
        .projects
        .into_iter()
        .filter(|project| project.initialized)
        .collect())
}

pub(crate) fn global_backend_impact(
    ctx: &CommandContext,
    target: &workhive_core::domain::RuntimeBackendId,
) -> Result<Vec<(camino::Utf8PathBuf, usize)>, CliError> {
    let discovery = ctx.composition.project_discovery();
    let projects = discover_initialized_projects(
        ctx.config.as_ref(),
        &discovery,
        super::home_path().as_deref(),
    )?;
    let mut impact = Vec::new();
    for project in projects {
        if ctx
            .config
            .load_project(&project.project_root)?
            .and_then(|config| config.backend)
            .is_some()
        {
            continue;
        }
        let identities = ctx
            .composition
            .workspace_identity(&project.project_root)
            .list_workspace_identities()?;
        let affected = identities
            .iter()
            .filter(|record| &record.runtime_binding.backend != target)
            .count();
        if affected > 0 {
            impact.push((project.project_root, affected));
        }
    }
    Ok(impact)
}

fn project_backend_impact(
    ctx: &CommandContext,
    root: &camino::Utf8Path,
    target: &workhive_core::domain::RuntimeBackendId,
) -> Result<Vec<(camino::Utf8PathBuf, usize)>, CliError> {
    let identities = ctx
        .composition
        .workspace_identity(root)
        .list_workspace_identities()?;
    let affected = identities
        .iter()
        .filter(|record| &record.runtime_binding.backend != target)
        .count();
    Ok((affected > 0)
        .then(|| (root.to_path_buf(), affected))
        .into_iter()
        .collect())
}

fn print_backend_impact(
    ctx: &CommandContext,
    project_root: Option<&camino::Utf8Path>,
    global: bool,
    target: &workhive_core::domain::RuntimeBackendId,
) -> Result<(), CliError> {
    let impact = if global {
        global_backend_impact(ctx, target)?
    } else {
        project_root
            .ok_or_else(|| {
                CliError::Runtime("project root is required for project backend changes".to_owned())
            })
            .and_then(|root| project_backend_impact(ctx, root, target))?
    };
    let workspace_count = impact.iter().map(|(_, count)| count).sum::<usize>();
    eprintln!(
        "Runtime replacement impact: {} project(s), {} workspace(s)",
        impact.len(),
        workspace_count
    );
    for (root, count) in impact {
        eprintln!("  {root}: {count} workspace(s)");
    }
    Ok(())
}

// ── TOML edit helpers ──

pub(crate) fn toml_edit_set(doc: &mut toml_edit::DocumentMut, key: &str, value: &str) {
    if let Some((table, field)) = key.rsplit_once('.') {
        let t = doc
            .entry(table)
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
        if let Some(t) = t.as_table_mut() {
            t[field] = toml_edit::value(value);
        }
    } else {
        doc[key] = toml_edit::value(value);
    }
}

pub(crate) fn toml_edit_unset(doc: &mut toml_edit::DocumentMut, key: &str) {
    if let Some((table, field)) = key.rsplit_once('.') {
        if let Some(t) = doc.get_mut(table).and_then(|i| i.as_table_mut()) {
            t.remove(field);
        }
    } else {
        doc.remove(key);
    }
}

fn render_backend_edit(
    config: &dyn workhive_core::ports::ConfigPort,
    project_root: Option<&camino::Utf8Path>,
    global: bool,
    value: Option<&str>,
) -> Result<(String, String, workhive_core::ports::AppConfig), CliError> {
    let raw = config.load_raw_toml(project_root, global)?;
    let mut doc = raw.parse::<toml_edit::DocumentMut>().map_err(|error| {
        workhive_core::errors::ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("<config>"),
            message: error.to_string(),
        }
    })?;
    let existing: workhive_core::ports::AppConfig =
        toml::from_str(&raw).map_err(|error| workhive_core::errors::ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("<config>"),
            message: error.to_string(),
        })?;
    existing.validate()?;
    match value {
        Some(value) => toml_edit_set(&mut doc, "backend", value),
        None => toml_edit_unset(&mut doc, "backend"),
    }
    let updated = doc.to_string();
    let parsed: workhive_core::ports::AppConfig = toml::from_str(&updated).map_err(|error| {
        workhive_core::errors::ConfigError::ParseFailed {
            path: camino::Utf8PathBuf::from("<config>"),
            message: error.to_string(),
        }
    })?;
    parsed.validate()?;
    Ok((raw, updated, parsed))
}

fn apply_backend_edit_with_replacement(
    ctx: &CommandContext,
    project_root: Option<&camino::Utf8Path>,
    global: bool,
    value: Option<&str>,
) -> Result<(), CliError> {
    let (previous, updated, project_config) =
        render_backend_edit(&*ctx.config, project_root, global, value)?;
    if global {
        let target = project_config
            .backend
            .clone()
            .unwrap_or_else(workhive_core::domain::RuntimeBackendId::tmux);
        let impact = global_backend_impact(ctx, &target)?;
        let roots = impact.into_iter().map(|(root, _)| root).collect::<Vec<_>>();
        return replace_global_backend(ctx, &roots, &target, &updated, &previous);
    }
    let Some(root) = project_root else {
        return Err(CliError::Runtime(
            "project backend change requires a project root".to_owned(),
        ));
    };
    let global_config = ctx.config.load_global()?;
    let target = usecases::config_mgmt::resolve_effective_backend(
        Some(&project_config),
        global_config.as_ref(),
    );
    replace_project_backend(ctx, root, &target, &updated, &previous)
}

pub(crate) fn print_config_diff(before: &str, after: &str, i18n: &StaticI18n, locale: Locale) {
    use workhive_core::i18n_keys as k;
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());
    let mut has_diff = false;
    for i in 0..max {
        let old = before_lines.get(i).copied().unwrap_or("");
        let new = after_lines.get(i).copied().unwrap_or("");
        if old != new {
            if !has_diff {
                eprintln!("--- {}", i18n.resolve(locale, k::CONFIG_DIFF_BEFORE));
                eprintln!("+++ {}", i18n.resolve(locale, k::CONFIG_DIFF_AFTER));
                has_diff = true;
            }
            if !old.is_empty() {
                eprintln!("- {old}");
            }
            if !new.is_empty() {
                eprintln!("+ {new}");
            }
        }
    }
}

#[cfg(test)]
mod backend_impact_tests {
    use super::discover_initialized_projects;
    use camino::Utf8PathBuf;
    use workhive_core::ports::{AppConfig, DiscoveredProject, DiscoveryWarning, ProjectsConfig};
    use workhive_testkit::fakes::{FakeConfig, FakeDiscovery};

    fn config_with_root() -> FakeConfig {
        let config = AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: vec![Utf8PathBuf::from("/projects")],
            }),
            ..AppConfig::default()
        };
        FakeConfig::new().with_global(config)
    }

    #[test]
    fn global_backend_impact_uses_current_discovery() {
        let config = config_with_root();
        let discovery = FakeDiscovery::new();
        discovery.add_project(DiscoveredProject {
            project_name: "project".to_owned(),
            project_root: Utf8PathBuf::from("/projects/project"),
            instance_id: "a1b2".to_owned(),
            initialized: true,
        });

        let projects = discover_initialized_projects(&config, &discovery, None).unwrap();

        assert_eq!(discovery.call_count(), 1);
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn global_backend_impact_fails_on_incomplete_discovery() {
        let config = config_with_root();
        let discovery = FakeDiscovery::new();
        discovery.add_warning(DiscoveryWarning::UnreadableDescendant {
            path: Utf8PathBuf::from("/projects/private"),
            reason: "permission denied".to_owned(),
        });

        let error = discover_initialized_projects(&config, &discovery, None).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("cannot determine global backend impact")
        );
    }
}

// ── Config ──

/// Unified config mutation operation.
enum ConfigMutation {
    TomlSet { key: String, value: String },
    TomlUnset { key: String },
    EnvSet { key: String, value: String },
    EnvUnset { key: String },
}

impl ConfigMutation {
    fn preview_and_confirm(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
        force: bool,
        ctx: &CommandContext,
    ) -> Result<(), CliError> {
        use workhive_core::i18n_keys as k;
        let locale = ctx.locale();

        match self {
            Self::TomlSet { key, value } => {
                let raw = ctx.config.load_raw_toml(project_root, global)?;
                let mut doc: toml_edit::DocumentMut = raw
                    .parse()
                    .unwrap_or_else(|_| toml_edit::DocumentMut::new());
                let before = doc.to_string();
                toml_edit_set(&mut doc, key, value);
                let after = doc.to_string();
                print_config_diff(&before, &after, &ctx.i18n, locale);
                if key == "backend" {
                    let target = match value.as_str() {
                        "tmux" => workhive_core::domain::RuntimeBackendId::tmux(),
                        "herdr" => workhive_core::domain::RuntimeBackendId::herdr(),
                        _ => {
                            return Err(CliError::Validation(
                                workhive_core::errors::ValidationError::InvalidConfigKey {
                                    key: key.clone(),
                                    reason: "backend must be 'tmux' or 'herdr'".to_owned(),
                                },
                            ));
                        }
                    };
                    print_backend_impact(ctx, project_root, global, &target)?;
                }
                confirm::require_confirm(
                    &ctx.i18n
                        .resolve_with_args(locale, k::CONFIG_SET_PREVIEW, &[key, value]),
                    force,
                )
            }
            Self::TomlUnset { key } => {
                let raw = ctx.config.load_raw_toml(project_root, global)?;
                let mut doc: toml_edit::DocumentMut = raw
                    .parse()
                    .unwrap_or_else(|_| toml_edit::DocumentMut::new());
                let before = doc.to_string();
                toml_edit_unset(&mut doc, key);
                let after = doc.to_string();
                print_config_diff(&before, &after, &ctx.i18n, locale);
                if key == "backend" {
                    let target = if global {
                        workhive_core::domain::RuntimeBackendId::tmux()
                    } else {
                        let global_config = ctx.config.load_global()?;
                        usecases::config_mgmt::resolve_effective_backend(
                            None,
                            global_config.as_ref(),
                        )
                    };
                    print_backend_impact(ctx, project_root, global, &target)?;
                }
                confirm::require_confirm(
                    &ctx.i18n
                        .resolve_with_args(locale, k::CONFIG_SET_PREVIEW, &[key, ""]),
                    force,
                )
            }
            Self::EnvSet { key, value } => {
                let entries = ctx.config.load_env(project_root, global)?;
                let old = entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str());
                match old {
                    Some(old) => {
                        eprintln!("--- .env");
                        eprintln!("+++ .env");
                        eprintln!("- {key}={old}");
                        eprintln!("+ {key}={value}");
                    }
                    None => {
                        eprintln!("+++ .env");
                        eprintln!("+ {key}={value}");
                    }
                }
                confirm::require_confirm(
                    &ctx.i18n
                        .resolve_with_args(locale, k::CONFIG_ENV_SET_PREVIEW, &[key, value]),
                    force,
                )
            }
            Self::EnvUnset { key } => {
                let entries = ctx.config.load_env(project_root, global)?;
                let old = entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str());
                if let Some(old) = old {
                    eprintln!("--- .env");
                    eprintln!("- {key}={old}");
                }
                confirm::require_confirm(
                    &ctx.i18n
                        .resolve_with_args(locale, k::CONFIG_ENV_UNSET_PREVIEW, &[key]),
                    force,
                )
            }
        }
    }

    fn apply(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
        ctx: &CommandContext,
    ) -> Result<(), CliError> {
        match self {
            Self::TomlSet { key, value } if key == "backend" => {
                apply_backend_edit_with_replacement(ctx, project_root, global, Some(value))?;
            }
            Self::TomlSet { key, value } => {
                usecases::config_mgmt::set_config_value(
                    &ctx.config,
                    project_root,
                    global,
                    key,
                    value,
                )?;
            }
            Self::TomlUnset { key } if key == "backend" => {
                apply_backend_edit_with_replacement(ctx, project_root, global, None)?;
            }
            Self::TomlUnset { key } => {
                usecases::config_mgmt::unset_config_value(&ctx.config, project_root, global, key)?;
            }
            Self::EnvSet { key, value } => {
                usecases::config_mgmt::env_set(&ctx.config, project_root, global, key, value)?;
            }
            Self::EnvUnset { key } => {
                usecases::config_mgmt::env_unset(&ctx.config, project_root, global, key)?;
            }
        }
        Ok(())
    }

    fn success_key(&self) -> &'static str {
        use workhive_core::i18n_keys as k;
        match self {
            Self::TomlSet { .. } => k::CONFIG_SET_SUCCESS,
            Self::TomlUnset { .. } => k::CONFIG_UNSET_SUCCESS,
            Self::EnvSet { .. } => k::CONFIG_ENV_SET_SUCCESS,
            Self::EnvUnset { .. } => k::CONFIG_ENV_UNSET_SUCCESS,
        }
    }
}

pub(crate) fn cmd_config(
    action: ConfigAction,
    original_command: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    let project_root = match &action {
        ConfigAction::Explain { .. } => None,
        _ if action.is_global() => None,
        _ => Some(detect_project()?.0),
    };

    match action {
        ConfigAction::Show { global, raw } => {
            // For `Show`, `project_root` is `Some` exactly when the scope is
            // non-global (see the resolution match above), so this mapping
            // is total.
            let scope = ConfigScope::from_detected_root(project_root.as_deref());
            let source = resolve_config_source_line(scope, ctx, locale)?;
            eprintln!("{source}");
            if raw {
                eprintln!("{}", ctx.i18n.resolve(locale, k::CONFIG_SHOW_HEADER));
                let raw_toml = ctx.config.load_raw_toml(project_root.as_deref(), global)?;
                if raw_toml.trim().is_empty() {
                    let cfg = usecases::config_mgmt::show_config(
                        &ctx.config,
                        project_root.as_deref(),
                        global,
                    )?;
                    let toml_str = toml::to_string_pretty(&cfg)
                        .expect("AppConfig serialization must not fail");
                    println!("{toml_str}");
                } else {
                    println!("{raw_toml}");
                }
            } else {
                let cfg = usecases::config_mgmt::show_config(
                    &ctx.config,
                    project_root.as_deref(),
                    global,
                )?;
                render_config_overview(&cfg, scope, ctx, locale)?;
            }
            Ok(())
        }
        ConfigAction::Explain { key } => {
            let explanation = match key.as_str() {
                "herdr.session" => {
                    "herdr.session selects the native Herdr session for newly opened workspaces.\n\
                     Project configuration overrides the global setting; otherwise the built-in\n\
                     default is 'workhive'. Existing workspace bindings are not migrated when this changes.\n\
                     The value must be a non-empty session name without path separators or control characters."
                }
                "backend" => {
                    "backend selects the runtime used for new and rebuilt workspaces.\n\
                     Accepted values: tmux, herdr.\n\
                     Project configuration overrides the global selection; changing an effective backend\n\
                     through `config set` or global init reviews impact and transactionally replaces\n\
                     affected workspace bindings."
                }
                "logging.file.retention" => {
                    "logging.file.retention accepts 'unlimited' or 'max_files(N)'.\n\
                     When logging.file.rotation = 'never', retention is ignored because \
                     no rotated files are produced.\n\
                     Examples: unlimited, max_files(7), max_files(30)"
                }
                "logging.file.rotation" => {
                    "logging.file.rotation controls how log files are rotated.\n\
                     Accepted values: never, hourly, daily, size_mb(N).\n\
                     'never' keeps a single log file without rotation.\n\
                     'size_mb(N)' rotates when the file exceeds N megabytes.\n\
                     Examples: never, hourly, daily, size_mb(10), size_mb(50)"
                }
                "logging.file.format" => {
                    "logging.file.format controls the output format of log entries.\n\
                     Accepted values: compact, json.\n\
                     'compact' produces human-readable single-line output.\n\
                     'json' produces structured JSON for machine parsing.\n\
                     Examples: compact, json"
                }
                "layout.template" => {
                    "layout.template selects a predefined pane layout.\n\
                     Available templates:\n\
                     - ai_coding_left: AI-assisted coding layout with 3 panes\n\
                     - ai_coding_dual: dual AI-assisted coding layout with 3 panes\n\
                     - half_split: two vertical panes\n\
                     - quad: four panes in a grid\n\
                     - custom: define your own column/row layout\n\
                     - single: one pane\n\
                     Examples: ai_coding_left, ai_coding_dual, half_split, quad, single, custom"
                }
                _ => {
                    return Err(CliError::Validation(
                        workhive_core::errors::ValidationError::InvalidConfigKey {
                            key,
                            reason: "no explain entry available for this key".to_owned(),
                        },
                    ));
                }
            };
            println!("{explanation}");
            Ok(())
        }
        ConfigAction::Edit { global } => cmd_config_edit(global, original_command, ctx),
        ConfigAction::Set {
            key,
            value,
            global,
            force,
        } => {
            if key == "backend" {
                let backend = match value.as_str() {
                    "tmux" => workhive_core::domain::RuntimeBackendId::tmux(),
                    "herdr" => workhive_core::domain::RuntimeBackendId::herdr(),
                    _ => {
                        return Err(workhive_core::errors::ConfigError::InvalidValue {
                            field: key,
                            reason: "expected 'tmux' or 'herdr'".to_owned(),
                        }
                        .into());
                    }
                };
                check_backend_config_dependencies(&backend, ctx, locale)?;
            }
            let mutation = ConfigMutation::TomlSet { key, value };
            mutation.preview_and_confirm(project_root.as_deref(), global, force, ctx)?;
            mutation.apply(project_root.as_deref(), global, ctx)?;
            eprintln!("{}", ctx.i18n.resolve(locale, mutation.success_key()));
            Ok(())
        }
        ConfigAction::Unset { key, global, force } => {
            if key == "backend" {
                let global_config = ctx.config.load_global()?;
                let backend = if global {
                    workhive_core::domain::RuntimeBackendId::tmux()
                } else {
                    usecases::config_mgmt::resolve_effective_backend(None, global_config.as_ref())
                };
                check_backend_config_dependencies(&backend, ctx, locale)?;
            }
            let mutation = ConfigMutation::TomlUnset { key };
            mutation.preview_and_confirm(project_root.as_deref(), global, force, ctx)?;
            mutation.apply(project_root.as_deref(), global, ctx)?;
            eprintln!("{}", ctx.i18n.resolve(locale, mutation.success_key()));
            Ok(())
        }
        ConfigAction::EnvSet {
            key,
            value,
            global,
            force,
        } => {
            let mutation = ConfigMutation::EnvSet { key, value };
            mutation.preview_and_confirm(project_root.as_deref(), global, force, ctx)?;
            mutation.apply(project_root.as_deref(), global, ctx)?;
            eprintln!("{}", ctx.i18n.resolve(locale, mutation.success_key()));
            Ok(())
        }
        ConfigAction::EnvUnset { key, global, force } => {
            let mutation = ConfigMutation::EnvUnset { key };
            mutation.preview_and_confirm(project_root.as_deref(), global, force, ctx)?;
            mutation.apply(project_root.as_deref(), global, ctx)?;
            eprintln!("{}", ctx.i18n.resolve(locale, mutation.success_key()));
            Ok(())
        }
    }
}

/// Global-vs-project scope of a config flow, resolved exactly once at the
/// flow entry.
///
/// The project arm carries a non-optional root, so downstream code consumes
/// a resolved value instead of re-unwrapping the same `Option` at every
/// use site.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ConfigScope<'a> {
    /// Operate on the global config only.
    Global,
    /// Operate within the detected project rooted at `root`.
    Project { root: &'a camino::Utf8Path },
}

impl<'a> ConfigScope<'a> {
    /// Build the scope from an already-detected optional project root:
    /// `None` means global scope, `Some(root)` means project scope.
    pub(crate) fn from_detected_root(root: Option<&'a camino::Utf8Path>) -> Self {
        match root {
            None => Self::Global,
            Some(root) => Self::Project { root },
        }
    }

    /// Project root for the project arm; `None` in global scope.
    pub(crate) fn project_root(&self) -> Option<&'a camino::Utf8Path> {
        match self {
            Self::Global => None,
            Self::Project { root } => Some(root),
        }
    }

    /// Whether this scope targets the global config.
    pub(crate) fn is_global(&self) -> bool {
        matches!(self, Self::Global)
    }
}

pub(crate) fn resolve_config_source(
    scope: ConfigScope<'_>,
    ctx: &CommandContext,
) -> Result<(String, String), CliError> {
    let global_path = display_global_config_path();

    match scope {
        ConfigScope::Global => Ok(("global".to_owned(), global_path)),
        ConfigScope::Project { root } => {
            let has_project_config = ctx.config.project_exists(root)?;
            let (scope_label, path) = if has_project_config {
                ("project".to_owned(), display_project_config_path())
            } else {
                ("global".to_owned(), global_path)
            };
            Ok((scope_label, path))
        }
    }
}

pub(crate) fn display_global_config_path() -> String {
    match workhive_infra::config::resolve_global_config_dir() {
        Ok(dir) => {
            let abs = dir.join("config.toml");
            crate::path_display::display_path_for_humans(abs.as_path())
        }
        Err(_) => "~/.config/workhive/config.toml".to_owned(),
    }
}

pub(crate) fn display_project_config_path() -> String {
    ".workhive/config.toml".to_owned()
}

pub(crate) fn resolve_config_source_line(
    scope: ConfigScope<'_>,
    ctx: &CommandContext,
    locale: Locale,
) -> Result<String, CliError> {
    use workhive_core::i18n_keys as k;

    let (scope_raw, path) = resolve_config_source(scope, ctx)?;
    let scope = if scope_raw == "project" {
        ctx.i18n.resolve(locale, k::INIT_SCOPE_PROJECT)
    } else {
        ctx.i18n.resolve(locale, k::INIT_SCOPE_GLOBAL)
    };
    Ok(ctx
        .i18n
        .resolve_with_args(locale, k::CONFIG_SOURCE_LINE, &[&scope, &path]))
}

/// Interactive config edit: preview current config, ask to modify, run wizard.
pub(crate) fn cmd_config_edit(
    global: bool,
    original_command: &str,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    use workhive_core::i18n_keys as k;
    let locale = ctx.locale();

    // Resolve global-vs-project scope exactly once; everything below
    // consumes the typed `scope` instead of re-unwrapping an `Option`.
    let detected_root = if global {
        None
    } else {
        Some(detect_project()?.0)
    };
    let scope = ConfigScope::from_detected_root(detected_root.as_deref());

    if let ConfigScope::Project { root } = scope {
        let ws_fs = ctx.composition.workspace_fs();
        let _ = ensure_project_initialized_or_delegate(
            root,
            original_command,
            MissingProjectInitFallback::ConfigError,
            &ws_fs,
            ctx,
        )?;
    }

    let cfg = usecases::config_mgmt::show_config(&ctx.config, scope.project_root(), global)?;

    render_config_overview(&cfg, scope, ctx, locale)?;

    // Resolve the file `config edit` will actually write to. This is the
    // target the user is editing, which is *always* the requested scope's
    // file -- distinct from the merged effective source the overview shows.
    let (target_scope_label, target_path, target_exists) = match scope {
        ConfigScope::Global => (
            ctx.i18n.resolve(locale, k::INIT_SCOPE_GLOBAL),
            display_global_config_path(),
            ctx.config.global_exists()?,
        ),
        ConfigScope::Project { root } => (
            ctx.i18n.resolve(locale, k::INIT_SCOPE_PROJECT),
            display_project_config_path(),
            ctx.config.project_exists(root)?,
        ),
    };
    let target_path_human =
        crate::path_display::display_path_for_humans(camino::Utf8Path::new(&target_path));

    println!(
        "\n  {}",
        ctx.i18n.resolve_with_args(
            locale,
            k::CONFIG_EDIT_TARGET_LINE,
            &[&target_scope_label, &target_path_human],
        )
    );
    // Default project edit may create a fresh project override; warn the
    // user explicitly so they can distinguish "I'm editing the global
    // config that shows up in the effective view" from "I'm creating a
    // brand new project override file". `--global` is not a creation
    // surprise so this notice is project-only.
    if !global && !target_exists {
        println!(
            "  {}",
            ctx.i18n.resolve_with_args(
                locale,
                k::CONFIG_EDIT_WILL_CREATE_PROJECT_CONFIG,
                &[&target_path_human],
            )
        );
    }

    let confirm_prompt = ctx.i18n.resolve_with_args(
        locale,
        k::CONFIG_EDIT_CONFIRM_TARGET,
        &[&target_scope_label, &target_path_human],
    );
    let want_modify =
        crate::commands::confirm_with_transcript(ctx, locale, &confirm_prompt, false)?;

    if !want_modify {
        return Ok(());
    }

    let config_path = target_path.clone();
    let config_exists = target_exists;

    let wizard_config = wizard::run_wizard(wizard::WizardRunRequest {
        force: false,
        config_exists,
        config_path: &config_path,
        target: if global {
            wizard::InitConfigTarget::Global
        } else {
            wizard::InitConfigTarget::Project
        },
        backend: if global { cfg.backend.clone() } else { None },
        deps: &*ctx.deps,
        exec_checker: &ctx.composition.executable_checker(),
        config_port: &ctx.config,
        i18n: &ctx.i18n,
        locale,
    })?;

    // Merge wizard output onto existing config to preserve fields the wizard
    // does not manage discovery, worktree_root, or runtime_root.
    match scope {
        ConfigScope::Global => {
            let existing = ctx.config.load_global()?.unwrap_or_default();
            let merged = usecases::config_mgmt::overlay_wizard_config(&existing, &wizard_config);
            usecases::init::init_global_force(&ctx.config, merged)?;
        }
        ConfigScope::Project { root } => {
            let existing = ctx.config.load_project(root)?.unwrap_or_default();
            let merged = usecases::config_mgmt::overlay_wizard_config(&existing, &wizard_config);
            ctx.config.save_project(root, &merged)?;
        }
    }

    eprintln!("{}", ctx.i18n.resolve(locale, k::CONFIG_EDIT_SUCCESS));
    Ok(())
}

#[cfg(test)]
mod backend_edit_tests {
    use super::*;
    use workhive_core::ports::ConfigPort;
    use workhive_testkit::fakes::FakeConfig;

    #[test]
    fn backend_edit_rejects_removed_auto_mode() {
        let config = FakeConfig::new();
        config
            .save_raw_toml(None, true, "backend = \"auto\"\n")
            .unwrap();

        let error = render_backend_edit(&config, None, true, Some("tmux")).unwrap_err();
        assert!(error.to_string().contains("unsupported backend 'auto'"));
    }
}

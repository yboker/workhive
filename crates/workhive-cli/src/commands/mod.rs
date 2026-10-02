use std::io::IsTerminal;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::{
    Locale, NamingContext, PaneDirection, ResolvedPaneLaunch, SessionName, SessionNamingPolicy,
    ToolCatalog, WorkspaceId,
};

use workhive_core::ports::{
    ConfigPort, EnvironmentPort, GitPort, I18nPort, LayoutConfig, PaneRuntimePort,
    ProjectDiscoveryPort, ProjectIndexPort, WorkspaceFsPort, WorkspaceRuntimePort,
};
use workhive_core::usecases;
use workhive_infra::i18n::StaticI18n;

use crate::cli::{ConfigAction, ListView, PaneAction};
use crate::composition::CliComposition;
use crate::context::CommandContext;
use crate::error::CliError;
use crate::{confirm, policy, table, ui, wizard};

pub(crate) mod activity;
pub(crate) mod attach_usage;
pub(crate) mod close;
pub(crate) mod completions;
pub(crate) mod control_plane;
pub(crate) mod doctor;
pub(crate) mod integrations;
pub(crate) mod internal_completion;
pub(crate) mod list;
pub(crate) mod list_render;
pub(crate) mod notification_trigger;
pub(crate) mod open;
pub(crate) mod open_readiness;
pub(crate) mod pane;
pub(crate) mod pane_activity_overlay;
pub(crate) mod pane_control_plane;
pub(crate) mod pane_list;
pub(crate) mod pane_metrics_human;
pub(crate) mod pane_metrics_view;
pub(crate) mod pane_send;
pub(crate) mod pane_show;
pub(crate) mod pane_supervisor;
pub(crate) mod pane_title_guard;
pub(crate) mod pane_view_data;
pub(crate) mod project;
pub(crate) mod reconcile_worker;
mod reconciliation_retry;
pub(crate) mod session;
pub(crate) mod update;
pub(crate) mod worktree;

pub(crate) use activity::*;
pub(crate) use close::*;
pub(crate) use completions::*;
pub(crate) use control_plane::*;
pub(crate) use doctor::*;
// `integrations` is an *internal* automatic lifecycle module, not a
// user CLI handler. It is imported directly by call sites
// (pane_supervisor, project::execute_init) via
// `crate::commands::integrations::prepare_provider` and intentionally
// not wildcard-re-exported.
pub(crate) use internal_completion::*;
pub(crate) use list::*;
#[allow(unused_imports)]
pub(crate) use list_render::*;
pub(crate) use open::*;
pub(crate) use pane::*;
pub(crate) use pane_supervisor::*;
pub(crate) use pane_title_guard::*;
pub(crate) use project::*;
pub(crate) use reconcile_worker::*;
pub(crate) use session::*;
pub(crate) use update::*;
pub(crate) use worktree::*;

/// Result of resolving trusted-visible project index state.
///
/// Preserves physical `NotFound` vs `Loaded` distinction for UI rendering,
/// includes filtered projects and setup metadata.
#[derive(Debug)]
pub(crate) enum TrustedProjectIndexState {
    /// Index file does not exist (legitimate initial state).
    NotFound {
        /// Setup state for UI rendering.
        setup: workhive_core::usecases::trusted_roots::DiscoverySetupState,
    },
    /// Index exists but contains no projects after trusted-root filtering.
    Empty {
        /// Setup state for UI rendering.
        setup: workhive_core::usecases::trusted_roots::DiscoverySetupState,
    },
    /// Index exists and contains projects after trusted-root filtering.
    Loaded {
        /// Filtered projects visible under trusted roots.
        projects: Vec<workhive_core::ports::ProjectSummary>,
    },
}

/// Resolve trusted-visible project index state with physical NotFound preservation.
///
/// User-facing read surfaces must call this instead of raw `list_projects()` so
/// projects under removed roots disappear immediately without waiting for
/// background reconciliation to physically prune the index.
///
/// Preserves physical `NotFound` vs `Loaded` distinction and includes resolved
/// trusted-root visibility decision and setup metadata needed by UI rendering.
/// Config read errors propagate to callers for proper error handling.
pub(crate) fn resolve_trusted_project_index(
    ctx: &CommandContext,
) -> Result<TrustedProjectIndexState, CliError> {
    let index = ctx.composition.project_index()?;
    resolve_trusted_project_index_kernel(&index, ctx.config.as_ref())
}

/// Private resolver kernel accepting explicit project index and config ports.
fn resolve_trusted_project_index_kernel(
    index: &dyn workhive_core::ports::ProjectIndexPort,
    config: &dyn workhive_core::ports::ConfigPort,
) -> Result<TrustedProjectIndexState, CliError> {
    use workhive_core::ports::IndexState;
    use workhive_core::usecases::trusted_roots::{
        SetupStateProbe, filter_projects_by_trusted_roots, resolve_setup_state,
        trusted_roots_from_config,
    };

    let state = index.list_projects()?;

    // Load config directly and propagate errors
    let global_cfg = config.load_global()?;
    let home = std::env::var("HOME")
        .ok()
        .and_then(|h| Utf8PathBuf::try_from(std::path::PathBuf::from(h)).ok());
    let trusted = trusted_roots_from_config(global_cfg.as_ref(), home.as_deref());
    let trusted_roots_configured = !trusted.is_empty();

    match state {
        IndexState::NotFound => {
            let setup = resolve_setup_state(SetupStateProbe {
                trusted_roots_configured,
                index_present: false,
                indexed_project_count: 0,
            });
            Ok(TrustedProjectIndexState::NotFound { setup })
        }
        IndexState::Loaded(v) => {
            let indexed_project_count = v.len();
            let filtered = filter_projects_by_trusted_roots(v, &trusted);

            if filtered.is_empty() {
                let setup = resolve_setup_state(SetupStateProbe {
                    trusted_roots_configured,
                    index_present: true,
                    indexed_project_count,
                });
                Ok(TrustedProjectIndexState::Empty { setup })
            } else {
                Ok(TrustedProjectIndexState::Loaded { projects: filtered })
            }
        }
    }
}

pub(crate) fn localized_bool(ctx: &CommandContext, locale: Locale, value: bool) -> String {
    use workhive_core::i18n_keys as k;

    if value {
        ctx.i18n.resolve(locale, k::BOOL_YES)
    } else {
        ctx.i18n.resolve(locale, k::BOOL_NO)
    }
}

/// Resolve the effective interaction policy from layered config.
///
/// `project_root` is `None` for global-only resolution;
/// when present, project config overrides global.
/// Returns a typed error on config read/parse failure instead of
/// silently defaulting to enabled.
pub(crate) fn resolve_interaction_policy(
    config: &dyn ConfigPort,
    project_root: Option<&camino::Utf8Path>,
) -> Result<workhive_core::ports::RuntimeInteractionPolicy, CliError> {
    let effective = usecases::config_mgmt::show_config(config, project_root, false)?;
    let tmux = effective.tmux_or_default();
    Ok(workhive_core::ports::RuntimeInteractionPolicy {
        mouse_enabled: tmux.resolved_mouse(),
        clipboard_enabled: tmux.resolved_clipboard(),
    })
}

/// Build an `AttachWorkspaceRequest` from a selected `SessionRecord`.
///
/// Used by both global workspace-attach and session-attach paths.
/// Resolves project mouse/clipboard overrides from `record.project_root`.
pub(crate) fn build_attach_request(
    config: &dyn ConfigPort,
    record: &workhive_core::usecases::sessions::SessionRecord,
) -> Result<workhive_core::ports::AttachWorkspaceRequest, CliError> {
    let project_root = camino::Utf8Path::new(&record.project_root);
    let policy = resolve_interaction_policy(config, Some(project_root))?;
    Ok(workhive_core::ports::AttachWorkspaceRequest {
        group: record.binding.scope_id.clone(),
        workspace_name: record.binding.workspace_id.clone(),
        interaction_policy: policy,
    })
}

pub(crate) fn print_choice_transcript(prompt: &str, choice: &str) {
    eprintln!("  {prompt} → {choice}");
}

pub(crate) fn confirm_with_transcript(
    ctx: &CommandContext,
    locale: Locale,
    prompt: &str,
    default: bool,
) -> Result<bool, CliError> {
    let value = crate::ui::screens::confirm::run(prompt, default)?;
    print_choice_transcript(prompt, &localized_bool(ctx, locale, value));
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::resolve_interaction_policy;
    use workhive_core::ports::{AppConfig, NamingSettings, TmuxConfig};

    #[test]
    fn interaction_policy_merge_project_overrides_global() {
        let global = TmuxConfig {
            mouse: Some(true),
            clipboard: Some(true),
            ..TmuxConfig::default()
        };
        let project = TmuxConfig {
            mouse: Some(false),
            clipboard: None,
            ..TmuxConfig::default()
        };
        let merged = TmuxConfig::merge(&global, &project);
        assert!(!merged.resolved_mouse());
        assert!(merged.resolved_clipboard());
    }

    #[test]
    fn interaction_policy_partial_naming_inherits_global_fields() {
        let global = TmuxConfig {
            naming: Some(NamingSettings {
                server_template: Some("whsrv-{instance_id}-{project}".to_owned()),
                session_template: Some("wh@{instance_id}@{project}@{workspace}".to_owned()),
                sanitize: Some(true),
                max_component_len: Some(30),
            }),
            ..TmuxConfig::default()
        };
        let project = TmuxConfig {
            naming: Some(NamingSettings {
                session_template: Some("custom@{instance_id}@{project}@{workspace}".to_owned()),
                server_template: None,
                sanitize: None,
                max_component_len: None,
            }),
            ..TmuxConfig::default()
        };
        let merged = TmuxConfig::merge(&global, &project);
        let naming = merged.naming.expect("naming should be present");
        assert_eq!(
            naming.server_template,
            Some("whsrv-{instance_id}-{project}".to_owned()),
            "server_template should inherit from global"
        );
        assert_eq!(
            naming.session_template,
            Some("custom@{instance_id}@{project}@{workspace}".to_owned()),
            "session_template should use project override"
        );
        assert_eq!(naming.sanitize, Some(true), "sanitize should inherit");
        assert_eq!(
            naming.max_component_len,
            Some(30),
            "max_component_len should inherit"
        );
    }

    #[test]
    fn interaction_policy_explicit_server_name_distinguishable_from_omission() {
        let global = TmuxConfig {
            server_name: Some("custom-global".to_owned()),
            ..TmuxConfig::default()
        };
        let project_explicit = TmuxConfig {
            server_name: Some(TmuxConfig::default_server_name()),
            ..TmuxConfig::default()
        };
        let project_omitted = TmuxConfig {
            server_name: None,
            ..TmuxConfig::default()
        };
        let merged_explicit = TmuxConfig::merge(&global, &project_explicit);
        let merged_omitted = TmuxConfig::merge(&global, &project_omitted);
        assert_eq!(
            merged_explicit.resolved_server_name(),
            TmuxConfig::default_server_name(),
            "explicit project override to default value should take effect"
        );
        assert_eq!(
            merged_omitted.resolved_server_name(),
            "custom-global",
            "omitted project server_name should inherit global"
        );
    }

    // ─── interaction_policy_error_ tests ─────────────────────────────────

    #[test]
    fn interaction_policy_error_invalid_global_config() {
        use workhive_core::errors::ConfigError;

        let config =
            workhive_testkit::fakes::FakeConfig::new().with_global_error(ConfigError::ReadFailed {
                path: "~/.config/workhive/config.toml".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated parse error",
                ),
            });

        let result = resolve_interaction_policy(&config, None);
        assert!(
            result.is_err(),
            "invalid global config must propagate through resolve_interaction_policy"
        );
    }

    #[test]
    fn interaction_policy_error_invalid_project_config() {
        use workhive_core::errors::ConfigError;

        let config = workhive_testkit::fakes::FakeConfig::new()
            .with_global(AppConfig::default())
            .with_project_error(ConfigError::ReadFailed {
                path: "/proj/.workhive/config.toml".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated project error",
                ),
            });

        let result = resolve_interaction_policy(&config, Some(camino::Utf8Path::new("/proj")));
        assert!(
            result.is_err(),
            "invalid project config must propagate through resolve_interaction_policy"
        );
    }

    // ─── global_attach_request_ tests ────────────────────────────────────

    fn make_session_record(
        project_root: &str,
        server: &str,
        session: &str,
    ) -> workhive_core::usecases::sessions::SessionRecord {
        workhive_core::usecases::sessions::SessionRecord {
            binding: workhive_core::domain::RuntimeBinding::tmux(server, session),
            project_name: "test-proj".to_owned(),
            project_root: project_root.to_owned(),
            workspace: "main".to_owned(),
            runtime_label: None,
            attached: false,
        }
    }

    #[test]
    fn global_attach_request_uses_record_fields() {
        let config = workhive_testkit::fakes::FakeConfig::new().with_global(AppConfig::default());
        let record = make_session_record("/projects/demo", "whsrv-main-demo", "wh@main@demo@feat");

        let request = super::build_attach_request(&config, &record).expect("must succeed");

        assert_eq!(request.group, "whsrv-main-demo");
        assert_eq!(request.workspace_name, "wh@main@demo@feat");
    }

    #[test]
    fn global_attach_request_resolves_project_policy() {
        let config = workhive_testkit::fakes::FakeConfig::new()
            .with_global(AppConfig {
                tmux: Some(TmuxConfig {
                    mouse: Some(true),
                    clipboard: Some(true),
                    ..TmuxConfig::default()
                }),
                ..AppConfig::default()
            })
            .with_project(AppConfig {
                tmux: Some(TmuxConfig {
                    mouse: Some(false),
                    clipboard: Some(false),
                    ..TmuxConfig::default()
                }),
                ..AppConfig::default()
            });
        let record = make_session_record("/projects/my-project", "whsrv-main", "wh@main@proj@ws");

        let request = super::build_attach_request(&config, &record).expect("must succeed");

        assert!(
            !request.interaction_policy.mouse_enabled,
            "must use project mouse override"
        );
        assert!(
            !request.interaction_policy.clipboard_enabled,
            "must use project clipboard override"
        );
    }

    #[test]
    fn global_attach_request_config_error_propagates() {
        use workhive_core::errors::ConfigError;

        let config =
            workhive_testkit::fakes::FakeConfig::new().with_global_error(ConfigError::ReadFailed {
                path: "~/.config/workhive/config.toml".into(),
                source: std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated error",
                ),
            });
        let record = make_session_record("/projects/demo", "srv", "sess");

        let result = super::build_attach_request(&config, &record);
        assert!(result.is_err(), "config error must propagate");
    }

    // ─── resolve_trusted_project_index tests ──────────────────────────────

    #[test]
    fn resolve_trusted_project_index_not_found_returns_not_found_state() {
        use workhive_core::ports::AppConfig;
        use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

        let config = FakeConfig::new().with_global(AppConfig::default());
        let index = FakeProjectIndex::with_not_found();

        let result = super::resolve_trusted_project_index_kernel(&index, &config);
        assert!(result.is_ok());
        match result.unwrap() {
            super::TrustedProjectIndexState::NotFound { setup } => {
                assert_eq!(
                    setup,
                    workhive_core::usecases::trusted_roots::DiscoverySetupState::NoTrustedRootsConfigured
                );
            }
            other => panic!("Expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn resolve_trusted_project_index_loaded_empty_returns_empty_state() {
        use camino::Utf8PathBuf;
        use workhive_core::ports::{AppConfig, ProjectsConfig};
        use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

        let config = FakeConfig::new().with_global(AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: vec![Utf8PathBuf::from("/trusted")],
            }),
            ..AppConfig::default()
        });
        let index = FakeProjectIndex::new(); // Empty index

        let result = super::resolve_trusted_project_index_kernel(&index, &config);
        assert!(result.is_ok());
        match result.unwrap() {
            super::TrustedProjectIndexState::Empty { setup } => {
                assert_eq!(
                    setup,
                    workhive_core::usecases::trusted_roots::DiscoverySetupState::TrustedRootsConfiguredIndexEmpty
                );
            }
            other => panic!("Expected Empty, got {:?}", other),
        }
    }

    #[test]
    fn resolve_trusted_project_index_filters_by_trusted_roots() {
        use camino::Utf8PathBuf;
        use workhive_core::ports::{AppConfig, ProjectIndexPort, ProjectSummary, ProjectsConfig};
        use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

        let config = FakeConfig::new().with_global(AppConfig {
            projects: Some(ProjectsConfig {
                trusted_roots: vec![Utf8PathBuf::from("/trusted")],
            }),
            ..AppConfig::default()
        });
        let index = FakeProjectIndex::new();
        index
            .upsert_project(&ProjectSummary {
                project_name: "trusted-proj".to_owned(),
                project_root: Utf8PathBuf::from("/trusted/proj"),
                config_path: Utf8PathBuf::from("/trusted/proj/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })
            .unwrap();
        index
            .upsert_project(&ProjectSummary {
                project_name: "untrusted-proj".to_owned(),
                project_root: Utf8PathBuf::from("/untrusted/proj"),
                config_path: Utf8PathBuf::from("/untrusted/proj/.workhive/config.toml"),
                last_opened_at: None,
                last_updated_at: None,
            })
            .unwrap();

        let result = super::resolve_trusted_project_index_kernel(&index, &config);
        assert!(result.is_ok());
        match result.unwrap() {
            super::TrustedProjectIndexState::Loaded { projects } => {
                assert_eq!(projects.len(), 1, "Should filter to only trusted projects");
                assert_eq!(projects[0].project_name, "trusted-proj");
            }
            other => panic!("Expected Loaded, got {:?}", other),
        }
    }

    #[test]
    fn resolve_trusted_project_index_config_error_propagates_typed() {
        use workhive_core::errors::ConfigError;
        use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

        let config = FakeConfig::new().with_global_error(ConfigError::ReadFailed {
            path: "/config.toml".into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        });
        let index = FakeProjectIndex::new();

        let result = super::resolve_trusted_project_index_kernel(&index, &config);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::CliError::Config(err) => match err {
                ConfigError::ReadFailed { path, source } => {
                    assert_eq!(path, "/config.toml");
                    assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
                }
                other => panic!("Expected ReadFailed, got {:?}", other),
            },
            other => panic!("Expected Config error, got {:?}", other),
        }
    }

    #[test]
    fn resolve_trusted_project_index_index_error_propagates_typed() {
        use workhive_core::errors::IndexError;
        use workhive_core::ports::AppConfig;
        use workhive_testkit::fakes::{FakeConfig, FakeProjectIndex};

        let config = FakeConfig::new().with_global(AppConfig::default());
        let index = FakeProjectIndex::new();
        index.set_list_error(IndexError::ReadFailed {
            path: "/index.json".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "not found"),
        });

        let result = super::resolve_trusted_project_index_kernel(&index, &config);
        assert!(result.is_err());
        match result.unwrap_err() {
            crate::error::CliError::Index(err) => match err {
                IndexError::ReadFailed { path, source } => {
                    assert_eq!(path, "/index.json");
                    assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
                }
                other => panic!("Expected ReadFailed, got {:?}", other),
            },
            other => panic!("Expected Index error, got {:?}", other),
        }
    }
}

#[allow(unused_imports)]
use super::*;
use crate::error::CliError;
#[allow(unused_imports)]
use camino::Utf8PathBuf;
#[allow(unused_imports)]
use workhive_core::ports::{ChangeKind, ChangedFile, WorktreeInfo};
#[allow(unused_imports)]
use workhive_core::usecases::worktree_safety::{
    UnmergedCommitSummary, WorktreeRisk, WorktreeSafetyReport,
};

#[test]
fn project_reinit_does_not_close_shared_herdr_sessions() {
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_core::domain::RuntimeBackendId;
    use workhive_testkit::fakes::FakeConfig;

    let root = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let state = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();
    let ctx = CommandContext::with_test_boundaries(
        Box::new(
            FakeConfig::new()
                .with_global(Default::default())
                .with_config_dir(state),
        ),
        FakeProcessLauncher::new(),
    );

    cleanup_project_sessions(
        &root,
        "project",
        Some("old-instance"),
        &RuntimeBackendId::herdr(),
        &ctx,
    )
    .unwrap();
}

#[test]
fn init_strategy_allows_completion_prompt_only_for_interactive_runs() {
    assert!(
        init_completion_guidance_interactive(InitStrategy::Interactive),
        "interactive init may offer the post-init completion prompt"
    );
    assert!(
        !init_completion_guidance_interactive(InitStrategy::NonInteractive { force: true }),
        "--force must keep the completion guidance non-interactive"
    );
    assert!(
        !init_completion_guidance_interactive(InitStrategy::NonInteractive { force: false }),
        "piped or CI init must keep the completion guidance non-interactive"
    );
}

// ── trusted_roots_global_init_ — final-step decision logic ────────

use crate::commands::project::setup::{
    GlobalInitTrustedRootPrompt, global_init_prompt_default_yes,
    resolve_global_init_trusted_root_prompt,
};

#[test]
fn trusted_roots_global_init_no_roots_offers_configure_with_default_yes() {
    let prompt = resolve_global_init_trusted_root_prompt(0);
    assert_eq!(prompt, GlobalInitTrustedRootPrompt::OfferConfigure);
    assert!(
        global_init_prompt_default_yes(prompt),
        "no-roots prompt must default to Yes (open editor)"
    );
}

#[test]
fn trusted_roots_global_init_existing_roots_offers_view_modify_with_default_no() {
    let prompt = resolve_global_init_trusted_root_prompt(3);
    assert_eq!(
        prompt,
        GlobalInitTrustedRootPrompt::OfferViewModify { count: 3 }
    );
    assert!(
        !global_init_prompt_default_yes(prompt),
        "existing-roots prompt must default to No (skip editor)"
    );
}

#[test]
fn trusted_roots_global_init_single_root_offers_view_modify() {
    let prompt = resolve_global_init_trusted_root_prompt(1);
    assert_eq!(
        prompt,
        GlobalInitTrustedRootPrompt::OfferViewModify { count: 1 }
    );
}

struct GlobalInitAnswer {
    answer: Result<bool, CliError>,
    call_count: std::cell::RefCell<usize>,
}

impl GlobalInitAnswer {
    fn new(answer: Result<bool, CliError>) -> Self {
        Self {
            answer,
            call_count: std::cell::RefCell::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.call_count.borrow()
    }
}

impl GlobalInitTrustedRootsInput for GlobalInitAnswer {
    fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
        *self.call_count.borrow_mut() += 1;
        match &self.answer {
            Ok(answer) => Ok(*answer),
            Err(err) => Err(CliError::Runtime(err.to_string())),
        }
    }
}

#[test]
fn trusted_roots_reconciliation_entry_global_init_editor_save_launches() {
    use crate::commands::project::roots_cmd::{
        RootsEditorInput, cmd_project_roots_status_with_writer,
    };
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_testkit::fakes::FakeConfig;

    struct SaveRoot(camino::Utf8PathBuf, std::cell::Cell<usize>);
    impl RootsEditorInput for SaveRoot {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            let call = self.1.get();
            self.1.set(call + 1);
            Ok(if call == 0 { 0 } else { 2 })
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            Ok(self.0.to_string())
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            Ok(true)
        }
    }

    let root = tempfile::tempdir().unwrap();
    let root = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf()).unwrap();
    let state = tempfile::tempdir().unwrap();
    let state = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();
    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext::with_test_boundaries(
        Box::new(
            FakeConfig::new()
                .with_global(Default::default())
                .with_config_dir(state),
        ),
        launcher.clone(),
    );

    run_global_init_trusted_roots_step(
        &[],
        &ctx,
        Locale::En,
        true,
        &GlobalInitAnswer::new(Ok(true)),
        &SaveRoot(root, std::cell::Cell::new(0)),
    )
    .unwrap();
    assert_eq!(launcher.launch_reconciliation_call_count(), 1);
    let mut output = Vec::new();
    cmd_project_roots_status_with_writer(&ctx, &mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("1"));
}

#[test]
fn trusted_roots_global_init_editor_outcome_unchanged_continues_normally() {
    use crate::commands::project::roots_cmd::RootsEditorInput;
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_testkit::fakes::FakeConfig;

    // Editor that immediately exits without changes (Unchanged outcome)
    struct UnchangedEditor;
    impl RootsEditorInput for UnchangedEditor {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            // Return "Exit" option immediately
            Ok(2)
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            unreachable!()
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            Ok(true)
        }
    }

    let state = tempfile::tempdir().unwrap();
    let state = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();
    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext::with_test_boundaries(
        Box::new(
            FakeConfig::new()
                .with_global(Default::default())
                .with_config_dir(state),
        ),
        launcher.clone(),
    );

    // Should succeed even with Unchanged outcome
    run_global_init_trusted_roots_step(
        &[],
        &ctx,
        Locale::En,
        true,
        &GlobalInitAnswer::new(Ok(true)),
        &UnchangedEditor,
    )
    .unwrap();
    // No reconciliation should be launched since nothing changed
    assert_eq!(launcher.launch_reconciliation_call_count(), 0);
}

#[test]
fn trusted_roots_global_init_editor_outcome_cancelled_continues_normally() {
    use crate::commands::project::roots_cmd::RootsEditorInput;
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_testkit::fakes::FakeConfig;

    // Editor that simulates user cancellation via UserAborted error
    struct CancelledEditor;
    impl RootsEditorInput for CancelledEditor {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            Err(CliError::UserAborted)
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            unreachable!()
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            Ok(true)
        }
    }

    let state = tempfile::tempdir().unwrap();
    let state = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();
    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext::with_test_boundaries(
        Box::new(
            FakeConfig::new()
                .with_global(Default::default())
                .with_config_dir(state),
        ),
        launcher.clone(),
    );

    // UserAborted maps to EditorOutcome::Cancelled, which continues normally
    let result = run_global_init_trusted_roots_step(
        &[],
        &ctx,
        Locale::En,
        true,
        &GlobalInitAnswer::new(Ok(true)),
        &CancelledEditor,
    );
    assert!(result.is_ok(), "UserAborted should continue normally");
    // Zero post-save side effects: no reconciliation launched
    assert_eq!(launcher.launch_reconciliation_call_count(), 0);
}

#[test]
fn trusted_roots_global_init_confirmation_error_propagates() {
    struct UnusedEditor;
    impl crate::commands::project::roots_cmd::RootsEditorInput for UnusedEditor {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            unreachable!()
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            unreachable!()
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            unreachable!()
        }
    }
    let ctx = CommandContext::new();
    let result = run_global_init_trusted_roots_step(
        &[],
        &ctx,
        Locale::En,
        true,
        &GlobalInitAnswer::new(Err(CliError::Runtime("terminal failed".to_owned()))),
        &UnusedEditor,
    );
    assert!(
        matches!(result, Err(CliError::Runtime(message)) if message.contains("terminal failed"))
    );
}

// ── trusted_roots_admission_flow_ — post-editor continuation ──────

use crate::commands::project::setup::is_project_admitted_after_editor;

#[test]
fn trusted_roots_admission_flow_covered_project_continues_after_save() {
    let project_root = camino::Utf8Path::new("/dev/projects/myapp");
    let saved_roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(
        is_project_admitted_after_editor(project_root, &saved_roots, None),
        "project under saved root must be admitted → continue"
    );
}

#[test]
fn trusted_roots_admission_flow_uncovered_project_returns_to_menu() {
    let project_root = camino::Utf8Path::new("/other/myapp");
    let saved_roots = vec![camino::Utf8PathBuf::from("/dev/projects")];
    assert!(
        !is_project_admitted_after_editor(project_root, &saved_roots, None),
        "project outside saved roots must not be admitted → return to menu"
    );
}

#[test]
fn trusted_roots_admission_flow_empty_roots_after_save_not_admitted() {
    let project_root = camino::Utf8Path::new("/dev/projects/myapp");
    let saved_roots: Vec<camino::Utf8PathBuf> = vec![];
    assert!(
        !is_project_admitted_after_editor(project_root, &saved_roots, None),
        "empty roots after save must not admit any project"
    );
}

#[test]
fn trusted_roots_admission_flow_tilde_expansion_admits_correctly() {
    let home = camino::Utf8Path::new("/Users/me");
    let project_root = camino::Utf8Path::new("/Users/me/dev/myapp");
    let saved_roots = vec![camino::Utf8PathBuf::from("~/dev")];
    assert!(
        is_project_admitted_after_editor(project_root, &saved_roots, Some(home)),
        "tilde-expanded root must admit descendant project"
    );
}

/// Wrapper type to implement ConfigPort for Rc<RefCell<FakeConfig>>.
struct FakeConfigWrapper(std::rc::Rc<std::cell::RefCell<workhive_testkit::fakes::FakeConfig>>);

impl workhive_core::ports::ConfigPort for FakeConfigWrapper {
    fn load_global(
        &self,
    ) -> Result<Option<workhive_core::ports::AppConfig>, workhive_core::errors::ConfigError> {
        self.0.borrow().load_global()
    }

    fn save_global(
        &self,
        config: &workhive_core::ports::AppConfig,
    ) -> Result<(), workhive_core::errors::ConfigError> {
        self.0.borrow().save_global(config)
    }

    fn load_project(
        &self,
        project_root: &camino::Utf8Path,
    ) -> Result<Option<workhive_core::ports::AppConfig>, workhive_core::errors::ConfigError> {
        self.0.borrow().load_project(project_root)
    }

    fn save_project(
        &self,
        project_root: &camino::Utf8Path,
        config: &workhive_core::ports::AppConfig,
    ) -> Result<(), workhive_core::errors::ConfigError> {
        self.0.borrow().save_project(project_root, config)
    }

    fn global_exists(&self) -> Result<bool, workhive_core::errors::ConfigError> {
        self.0.borrow().global_exists()
    }

    fn project_exists(
        &self,
        project_root: &camino::Utf8Path,
    ) -> Result<bool, workhive_core::errors::ConfigError> {
        self.0.borrow().project_exists(project_root)
    }

    fn load_raw_toml(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
    ) -> Result<String, workhive_core::errors::ConfigError> {
        self.0.borrow().load_raw_toml(project_root, global)
    }

    fn save_raw_toml(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
        contents: &str,
    ) -> Result<(), workhive_core::errors::ConfigError> {
        self.0
            .borrow()
            .save_raw_toml(project_root, global, contents)
    }

    fn load_env(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
    ) -> Result<Vec<(String, String)>, workhive_core::errors::ConfigError> {
        self.0.borrow().load_env(project_root, global)
    }

    fn save_env(
        &self,
        project_root: Option<&camino::Utf8Path>,
        global: bool,
        entries: &[(String, String)],
    ) -> Result<(), workhive_core::errors::ConfigError> {
        self.0.borrow().save_env(project_root, global, entries)
    }

    fn global_config_dir(&self) -> Result<camino::Utf8PathBuf, workhive_core::errors::ConfigError> {
        self.0.borrow().global_config_dir()
    }
}

#[test]
fn preserve_restore_helpers_roundtrip_authored_trusted_roots() {
    use workhive_testkit::fakes::FakeConfig;

    // Existing global config with a [projects] section containing
    // comments and trusted roots.
    let existing_toml = r#"language = "en"

[projects]
# User-authored trusted directories
trusted_roots = ["/dev/projects", "/home/user/repos"]

[tmux]
server_name = "workhive"
"#;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed the FakeConfig with the existing raw TOML.
    let fake_config = FakeConfig::new().with_config_dir(state_dir.clone());
    fake_config
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Preserve step: extract [projects] before overwrite.
    let preserved = super::preserve_global_projects_toml(&fake_config).unwrap();
    assert!(
        preserved.is_some(),
        "must extract existing [projects] table"
    );

    // Simulate init_global_force: save a new config without [projects].
    let new_config = workhive_core::ports::AppConfig {
        language: Some(workhive_core::domain::Locale::En),
        projects: None,
        ..Default::default()
    };
    fake_config.save_global(&new_config).unwrap();

    // After overwrite, the raw TOML should not contain trusted_roots.
    let after_overwrite = fake_config.load_raw_toml(None, true).unwrap();
    assert!(
        !after_overwrite.contains("trusted_roots"),
        "overwrite must not contain roots before restore"
    );

    // Restore step: merge [projects] back.
    super::restore_global_projects_toml(&fake_config, &preserved.unwrap()).unwrap();

    // Verify the restored TOML contains the original roots and comment.
    let restored = fake_config.load_raw_toml(None, true).unwrap();
    assert!(
        restored.contains("/dev/projects"),
        "restored TOML must contain first root"
    );
    assert!(
        restored.contains("/home/user/repos"),
        "restored TOML must contain second root"
    );
    assert!(
        restored.contains("User-authored trusted directories"),
        "restored TOML must preserve the original comment"
    );
}

#[test]
fn global_init_preserve_read_failure_aborts_before_save() {
    use workhive_core::errors::ConfigError;
    use workhive_testkit::fakes::FakeConfig;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed with valid TOML so global_exists() returns true
    let valid_toml = r#"
language = "en"
"#;
    let fake_config = FakeConfig::new().with_config_dir(state_dir.clone());
    fake_config.save_raw_toml(None, true, valid_toml).unwrap();

    // Now inject the error for load_raw_toml
    let fake_config = fake_config.with_load_raw_toml_error(ConfigError::ReadFailed {
        path: camino::Utf8PathBuf::from("<fake-global-config>"),
        source: std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "simulated read failure",
        ),
    });

    // Preserve step must return an error, not silently convert to None
    let result = super::preserve_global_projects_toml(&fake_config);
    assert!(
        result.is_err(),
        "read failure must abort, not silently return None"
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("failed to read global config"),
        "error must mention read failure"
    );
}

#[test]
fn global_init_preserve_parse_failure_aborts_before_save() {
    use workhive_testkit::fakes::FakeConfig;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed with invalid TOML and ensure global_exists() returns true
    let invalid_toml = "this is not valid TOML [[[[";
    let fake_config = FakeConfig::new()
        .with_config_dir(state_dir.clone())
        .with_global_exists();
    fake_config.save_raw_toml(None, true, invalid_toml).unwrap();

    // Preserve step must return an error on parse failure
    let result = super::preserve_global_projects_toml(&fake_config);
    assert!(
        result.is_err(),
        "parse failure must abort, not silently return None"
    );
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("failed to parse global config"),
        "error must mention parse failure"
    );
}

#[test]
fn execute_init_global_force_noninteractive_preserves_projects_table() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_testkit::fakes::{FakeConfig, FakeDependency};

    // Existing global config with a [projects] section containing
    // comments and trusted roots.
    let existing_toml = r#"language = "en"

[projects]
# User-authored trusted directories
trusted_roots = ["/dev/projects", "/home/user/repos"]

[tmux]
server_name = "workhive"
"#;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed the FakeConfig with the existing raw TOML.
    let fake_config = FakeConfig::new().with_config_dir(state_dir.clone());
    fake_config
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Wrap in Rc<RefCell> for shared access to check call count.
    let fake_config = Rc::new(RefCell::new(fake_config));

    // Create a minimal CommandContext with the instrumented config.
    let deps = FakeDependency::new();
    let composition = crate::composition::CliComposition::new();
    let ctx = CommandContext {
        composition,
        config: Box::new(FakeConfigWrapper(Rc::clone(&fake_config))),
        deps: Box::new(deps),
        i18n: workhive_infra::i18n::StaticI18n::new(),
        pane_registry: workhive_infra::pane_registry::InMemoryPaneRegistry::new(),
        verbose: false,
        process_launcher: Box::new(crate::context::FakeProcessLauncher::new()),
    };

    // Execute init with force mode (non-interactive, no editor prompt).
    let result = super::execute_init(
        super::InitMode::Global,
        super::InitStrategy::NonInteractive { force: true },
        Some(workhive_core::domain::RuntimeBackendId::tmux()),
        &ctx,
    );

    // Init should succeed.
    assert!(result.is_ok(), "execute_init should succeed: {:?}", result);

    // save_global should have been called exactly once.
    assert_eq!(
        fake_config.borrow().save_global_call_count(),
        1,
        "save_global must be called exactly once"
    );

    // The preserved [projects] table must be restored after the overwrite.
    let final_toml = fake_config.borrow().load_raw_toml(None, true).unwrap();
    assert!(
        final_toml.contains("trusted_roots"),
        "final TOML must contain preserved trusted_roots"
    );
    assert!(
        final_toml.contains("/dev/projects"),
        "final TOML must contain the first root"
    );
    assert!(
        final_toml.contains("/home/user/repos"),
        "final TOML must contain the second root"
    );
    assert!(
        final_toml.contains("# User-authored trusted directories"),
        "final TOML must preserve the original comment"
    );
}

#[test]
fn execute_init_global_force_read_failure_aborts_before_save() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_core::errors::ConfigError;
    use workhive_testkit::fakes::{FakeConfig, FakeDependency};

    let existing_toml = r#"language = "en"

[projects]
trusted_roots = ["/dev/projects"]
"#;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed the FakeConfig with the existing raw TOML.
    let fake_config = FakeConfig::new().with_config_dir(state_dir.clone());
    fake_config
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Inject a read failure for load_raw_toml.
    fake_config.set_load_raw_toml_error(ConfigError::ReadFailed {
        path: camino::Utf8PathBuf::from("<fake-global-config>"),
        source: std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "simulated read failure",
        ),
    });

    // Wrap in Rc<RefCell> for shared access to check call count.
    let fake_config = Rc::new(RefCell::new(fake_config));

    let deps = FakeDependency::new();
    let composition = crate::composition::CliComposition::new();
    let ctx = CommandContext {
        composition,
        config: Box::new(FakeConfigWrapper(Rc::clone(&fake_config))),
        deps: Box::new(deps),
        i18n: workhive_infra::i18n::StaticI18n::new(),
        pane_registry: workhive_infra::pane_registry::InMemoryPaneRegistry::new(),
        verbose: false,
        process_launcher: Box::new(crate::context::FakeProcessLauncher::new()),
    };

    // Execute init with force mode.
    let result = super::execute_init(
        super::InitMode::Global,
        super::InitStrategy::NonInteractive { force: true },
        Some(workhive_core::domain::RuntimeBackendId::tmux()),
        &ctx,
    );

    // Init should fail with the read error.
    assert!(result.is_err(), "execute_init should fail on read error");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("failed to read global config"),
        "error must mention read failure"
    );

    // save_global must not have been called.
    assert_eq!(
        fake_config.borrow().save_global_call_count(),
        0,
        "save_global must not be called when preservation read fails"
    );
}

#[test]
fn execute_init_global_force_parse_failure_aborts_before_save() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_testkit::fakes::{FakeConfig, FakeDependency};

    // Invalid TOML that will fail to parse.
    let invalid_toml = "this is not valid TOML [[[[";

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed the FakeConfig with invalid TOML and ensure global_exists() returns true.
    let fake_config = FakeConfig::new()
        .with_config_dir(state_dir.clone())
        .with_global_exists();
    fake_config.save_raw_toml(None, true, invalid_toml).unwrap();

    // Wrap in Rc<RefCell> for shared access to check call count.
    let fake_config = Rc::new(RefCell::new(fake_config));

    let deps = FakeDependency::new();
    let composition = crate::composition::CliComposition::new();
    let ctx = CommandContext {
        composition,
        config: Box::new(FakeConfigWrapper(Rc::clone(&fake_config))),
        deps: Box::new(deps),
        i18n: workhive_infra::i18n::StaticI18n::new(),
        pane_registry: workhive_infra::pane_registry::InMemoryPaneRegistry::new(),
        verbose: false,
        process_launcher: Box::new(crate::context::FakeProcessLauncher::new()),
    };

    // Execute init with force mode.
    let result = super::execute_init(
        super::InitMode::Global,
        super::InitStrategy::NonInteractive { force: true },
        Some(workhive_core::domain::RuntimeBackendId::tmux()),
        &ctx,
    );

    // Init should fail with the parse error.
    assert!(result.is_err(), "execute_init should fail on parse error");
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("failed to parse global config"),
        "error must mention parse failure"
    );

    // save_global must not have been called.
    assert_eq!(
        fake_config.borrow().save_global_call_count(),
        0,
        "save_global must not be called when preservation parse fails"
    );
}

#[test]
fn global_init_backend_change_uses_transactional_replacement() {
    use crate::commands::project::roots_cmd::RootsEditorInput;
    use crate::context::{CommandContext, FakeProcessLauncher};
    use workhive_testkit::fakes::FakeConfig;

    struct NoOpEditor;
    impl RootsEditorInput for NoOpEditor {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            unreachable!()
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            unreachable!()
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            unreachable!()
        }
    }

    struct NoOpCompletion;
    impl super::CompletionGuidance for NoOpCompletion {
        fn guide(
            &self,
            _: &CommandContext,
            _: workhive_core::domain::Locale,
            _: bool,
        ) -> Result<(), CliError> {
            Ok(())
        }
    }

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();
    let config = std::rc::Rc::new(std::cell::RefCell::new(
        FakeConfig::new()
            .with_global(workhive_core::ports::AppConfig {
                backend: Some(workhive_core::domain::RuntimeBackendId::tmux()),
                projects: Some(workhive_core::ports::ProjectsConfig {
                    trusted_roots: vec![state_dir.clone()],
                }),
                ..Default::default()
            })
            .with_config_dir(state_dir),
    ));
    let ctx = CommandContext {
        composition: crate::composition::CliComposition::new(),
        config: Box::new(FakeConfigWrapper(std::rc::Rc::clone(&config))),
        deps: Box::new(workhive_testkit::fakes::FakeDependency::new()),
        i18n: workhive_infra::i18n::StaticI18n::new(),
        pane_registry: workhive_infra::pane_registry::InMemoryPaneRegistry::new(),
        verbose: false,
        process_launcher: Box::new(FakeProcessLauncher::new()),
    };
    let app_config = workhive_core::ports::AppConfig {
        backend: Some(workhive_core::domain::RuntimeBackendId::herdr()),
        ..Default::default()
    };

    super::execute_global_init_kernel(
        &ctx,
        app_config,
        false,
        &GlobalInitAnswer::new(Ok(false)),
        &NoOpEditor,
        &NoOpCompletion,
    )
    .unwrap();

    assert_eq!(config.borrow().save_global_call_count(), 0);
    let persisted: workhive_core::ports::AppConfig =
        toml::from_str(&config.borrow().load_raw_toml(None, true).unwrap()).unwrap();
    assert_eq!(
        persisted.backend,
        Some(workhive_core::domain::RuntimeBackendId::herdr())
    );
}

#[test]
fn execute_global_init_kernel_confirmation_false_preserves_authored_toml() {
    use crate::commands::project::roots_cmd::RootsEditorInput;
    use crate::context::{CommandContext, FakeProcessLauncher};
    use std::cell::RefCell;
    use std::rc::Rc;
    use workhive_testkit::fakes::{FakeConfig, FakeDependency};

    // Existing global config with authored [projects] section containing
    // comments and trusted roots.
    let existing_toml = r#"language = "en"

[projects]
# User-authored trusted directories
trusted_roots = ["/dev/projects", "/home/user/repos"]

[tmux]
server_name = "workhive"
"#;

    let state = tempfile::tempdir().unwrap();
    let state_dir = camino::Utf8PathBuf::from_path_buf(state.path().to_path_buf()).unwrap();

    // Seed the FakeConfig with the existing raw TOML.
    let fake_config = FakeConfig::new().with_config_dir(state_dir.clone());
    fake_config
        .save_raw_toml(None, true, existing_toml)
        .unwrap();

    // Wrap in Rc<RefCell> for shared access to check call count.
    let fake_config = Rc::new(RefCell::new(fake_config));

    let deps = FakeDependency::new();
    let composition = crate::composition::CliComposition::new();
    let launcher = FakeProcessLauncher::new();
    let ctx = CommandContext {
        composition,
        config: Box::new(FakeConfigWrapper(Rc::clone(&fake_config))),
        deps: Box::new(deps),
        i18n: workhive_infra::i18n::StaticI18n::new(),
        pane_registry: workhive_infra::pane_registry::InMemoryPaneRegistry::new(),
        verbose: false,
        process_launcher: Box::new(launcher.clone()),
    };

    // Editor that fails if called (proves editor is never opened when user declines)
    struct NeverCalledEditor;
    impl RootsEditorInput for NeverCalledEditor {
        fn select(&self, _: &str, _: &[String], _: usize) -> Result<usize, CliError> {
            unreachable!("editor must not be opened when user declines confirmation")
        }
        fn text_input(&self, _: &str, _: &str) -> Result<String, CliError> {
            unreachable!("editor must not be opened when user declines confirmation")
        }
        fn confirm(&self, _: &str, _: bool) -> Result<bool, CliError> {
            unreachable!("editor must not be opened when user declines confirmation")
        }
    }

    // No-op completion guidance (avoids TTY prompts in test environment)
    struct NoOpCompletionGuidance;
    impl super::CompletionGuidance for NoOpCompletionGuidance {
        fn guide(
            &self,
            _: &CommandContext,
            _: workhive_core::domain::Locale,
            _: bool,
        ) -> Result<(), CliError> {
            Ok(())
        }
    }

    // Build default app config (simulating what wizard/build_default_config produces)
    let app_config = workhive_core::ports::AppConfig {
        language: Some(workhive_core::domain::Locale::En),
        projects: None, // New config has no [projects] section
        ..Default::default()
    };

    // Call the production kernel with interactive=true and confirmation=false (user declines)
    // No-op completion guidance avoids TTY prompts in test environment
    let confirmation_input = GlobalInitAnswer::new(Ok(false));
    let result = super::execute_global_init_kernel(
        &ctx,
        app_config,
        true, // interactive=true (terminal enabled)
        &confirmation_input,
        &NeverCalledEditor,
        &NoOpCompletionGuidance,
    );

    // Kernel should succeed without opening editor
    assert!(
        result.is_ok(),
        "confirmation=false must succeed without error: {:?}",
        result
    );

    // Confirmation must have been called exactly once (proving input was exercised)
    assert_eq!(
        confirmation_input.call_count(),
        1,
        "confirmation must be called exactly once with interactive=true"
    );

    // save_global must have been called exactly once (proving overwrite happened)
    assert_eq!(
        fake_config.borrow().save_global_call_count(),
        1,
        "save_global must be called exactly once (overwrite occurred)"
    );

    // Editor must not have been called (proven by NeverCalledEditor)
    // Zero reconciliation launches (editor never opened, no changes)
    assert_eq!(
        launcher.launch_reconciliation_call_count(),
        0,
        "confirmation=false must not launch reconciliation"
    );

    // Authored TOML must survive the overwrite/restore cycle
    let final_toml = fake_config.borrow().load_raw_toml(None, true).unwrap();
    assert!(
        final_toml.contains("trusted_roots"),
        "final TOML must contain preserved trusted_roots"
    );
    assert!(
        final_toml.contains("/dev/projects"),
        "final TOML must contain the first root"
    );
    assert!(
        final_toml.contains("/home/user/repos"),
        "final TOML must contain the second root"
    );
    assert!(
        final_toml.contains("# User-authored trusted directories"),
        "final TOML must preserve the original comment"
    );
}

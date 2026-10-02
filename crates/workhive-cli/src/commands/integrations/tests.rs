//! Tests for the automatic AI-integration preparation lifecycle.
//!
//! Current product contract (the assertions in this file):
//!
//! 1. Provider preparation does NOT generate provider hook tables,
//!    hook trust state, hook script files, hook entries, or the
//!    OpenCode Workhive plugin.
//! 2. The Codex `notify` wrapper is still installed (and removable)
//!    when the feature gate enables it. The wrapper records
//!    completion/notification evidence; it does not represent a
//!    hook/plugin lifecycle.
//! 3. Legacy cleanup removes Workhive-owned hook entries / hook
//!    scripts / OpenCode plugin while preserving every
//!    user-authored hook / plugin / `notify` value. Cleanup is
//!    idempotent (no spurious backup, no spurious rewrite), and
//!    surfaces parse/I/O failures as typed `IntegrationError` rather
//!    than masquerading them as "already clean".
//! 4. The lifecycle is feature-gate aware: when the AI integration
//!    feature gate is disabled (globally or per-provider), prepare
//!    short-circuits silently and touches no third-party config.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tempfile::TempDir;
use toml_edit::DocumentMut;
use workhive_core::domain::AiActivityProvider;
use workhive_core::ports::{
    AiIntegrationProviderConfig, AiIntegrationProvidersConfig, AiIntegrationsConfig, AppConfig,
    CodexNotifyConfig, ExecutableAvailabilityPort, FeaturesConfig,
};
use workhive_core::usecases::ai_integrations::FeatureGate;

use super::cleanup::CleanupAction;
use super::{
    PreparationOutcome, opportunistic_prepare_discoverable, plan_cleanup, prepare_provider,
    prepare_provider_if_enabled, prepare_provider_with_config,
};

// ── Test fixtures ─────────────────────────────────────────────────────

/// Build a minimal merged `AppConfig` whose Codex notify gate is on.
/// Used by the few tests that exercise the Codex notify wrapper.
fn codex_notify_enabled_config() -> AppConfig {
    AppConfig {
        features: Some(FeaturesConfig {
            ai_integrations: Some(AiIntegrationsConfig {
                enabled: Some(true),
                providers: AiIntegrationProvidersConfig {
                    codex: Some(AiIntegrationProviderConfig {
                        enabled: Some(true),
                        hook_detail: None,
                        notify: Some(CodexNotifyConfig {
                            enabled: Some(true),
                        }),
                    }),
                    ..AiIntegrationProvidersConfig::default()
                },
            }),
            ..FeaturesConfig::default()
        }),
        ..AppConfig::default()
    }
}

/// Serializes tests that mutate process-global env vars
/// (`HOME`, `XDG_CONFIG_HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`).
/// `cargo test` runs unit tests in parallel by default and any two
/// tests here would otherwise race on env state.
static ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    keys: Vec<&'static str>,
    prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl EnvGuard {
    fn install(values: &[(&'static str, &str)]) -> Self {
        let mut prev = Vec::with_capacity(values.len());
        let mut keys = Vec::with_capacity(values.len());
        for (k, v) in values {
            prev.push((*k, env::var_os(k)));
            unsafe {
                env::set_var(k, v);
            }
            keys.push(*k);
        }
        Self { keys, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.prev.iter() {
            match value {
                Some(v) => unsafe { env::set_var(key, v) },
                None => unsafe { env::remove_var(key) },
            }
        }
        for k in &self.keys {
            if !self.prev.iter().any(|(pk, _)| pk == k) {
                unsafe { env::remove_var(k) };
            }
        }
    }
}

/// Isolated `$HOME` + `$XDG_CONFIG_HOME` sandbox. Each test creates
/// one and references its `home`/`xdg` paths to plant fixtures.
/// Dropping the fixture restores the previous env vars and removes
/// the tempdir contents.
struct FakeHome {
    home: PathBuf,
    xdg: PathBuf,
    _env: EnvGuard,
    _tmp: TempDir,
}

impl FakeHome {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let xdg = tmp.path().join("xdg");
        let env = EnvGuard::install(&[
            ("HOME", home.to_str().unwrap()),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ]);
        Self {
            home,
            xdg,
            _env: env,
            _tmp: tmp,
        }
    }

    fn claude_settings(&self) -> PathBuf {
        let dir = self.home.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn codex_config(&self) -> PathBuf {
        let dir = self.home.join(".codex");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    fn opencode_plugin(&self) -> PathBuf {
        let dir = self.xdg.join("opencode").join("plugins");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("workhive.mjs")
    }
}

/// Count the `<file>.workhive-backup-*` siblings produced by the
/// `sentinel::backup_file` helper. Used to assert that no spurious
/// backup is written on an idempotent / no-op cleanup pass.
fn count_workhive_backups(target: &Path) -> usize {
    let parent = match target.parent() {
        Some(p) => p,
        None => return 0,
    };
    let name = match target.file_name().and_then(|n| n.to_str()) {
        Some(s) => s,
        None => return 0,
    };
    let prefix = format!("{name}.workhive-backup-");
    let read = match std::fs::read_dir(parent) {
        Ok(rd) => rd,
        Err(_) => return 0,
    };
    read.filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with(&prefix))
                .unwrap_or(false)
        })
        .count()
}

// ── Acceptance 1: provider preparation does not install hooks/plugins ─

/// Claude prepare must not create a `~/.claude/settings.json`, must
/// not write any hook entries, and must not install a hook script
/// under `<global_config>/integrations/claude_code/` for a fresh
/// install where the user has not configured Claude at all.
#[test]
fn claude_prepare_does_not_install_any_hook_artifacts_on_fresh_box() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();

    let outcome = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "fresh box must report AlreadyCurrent (nothing to clean, nothing to install)",
    );

    let settings = fake.home.join(".claude").join("settings.json");
    assert!(
        !settings.exists(),
        "Claude settings.json must not be created by prepare on a fresh box",
    );
    let script_dir = fake
        .xdg
        .join("workhive")
        .join("integrations")
        .join("claude_code");
    assert!(
        !script_dir.exists() || std::fs::read_dir(&script_dir).unwrap().next().is_none(),
        "no Workhive hook script directory contents must be created",
    );
}

/// Codex prepare must not write `[features].hooks`, must not write
/// any `[[hooks.*]]` table, and must not install a hook script for a
/// fresh install where the user has not configured Codex at all.
/// (The Codex `notify` wrapper installs only when the feature gate
/// is enabled; the default gate is off, so this fresh box stays
/// silent.)
#[test]
fn codex_prepare_does_not_install_hook_tables_or_script_on_fresh_box() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();

    let outcome = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "fresh box must report AlreadyCurrent for Codex too",
    );

    let config = fake.home.join(".codex").join("config.toml");
    assert!(
        !config.exists(),
        "Codex config.toml must not be created by prepare on a fresh box",
    );
    let script_dir = fake.xdg.join("workhive").join("integrations").join("codex");
    assert!(
        !script_dir.exists() || std::fs::read_dir(&script_dir).unwrap().next().is_none(),
        "no Codex hook script must be created",
    );
}

/// OpenCode prepare must not drop a plugin into the OpenCode plugins
/// directory on a fresh box.
#[test]
fn opencode_prepare_does_not_install_plugin_on_fresh_box() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();

    let outcome = prepare_provider(AiActivityProvider::Opencode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "fresh box must report AlreadyCurrent for OpenCode",
    );

    let plugin = fake
        .xdg
        .join("opencode")
        .join("plugins")
        .join("workhive.mjs");
    assert!(
        !plugin.exists(),
        "OpenCode plugin must not be created by prepare on a fresh box",
    );
}

// ── Acceptance 2: legacy cleanup removes Workhive-owned material ──────

/// A legacy install left a Workhive-managed entry in Claude's
/// `settings.json` alongside a user's own entry. The prepare pass
/// must delete only the Workhive entry, preserve the user entry
/// verbatim, and report `Updated`.
#[test]
fn claude_prepare_cleans_up_legacy_workhive_entries_and_preserves_user_hook() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    std::fs::write(
        &settings,
        r#"{
          "hooks": {
            "UserPromptSubmit": [
              { "hooks": [
                  { "type": "command", "command": "/user/own.sh" }
              ] },
              { "hooks": [
                  { "type": "command",
                    "command": "/bin/sh -c 'exit 0' workhive-managed-hook /tmp/workhive-claude-hook.sh",
                    "_workhive_managed": true }
              ] }
            ]
          },
          "unrelated": "keep-me"
        }"#,
    )
    .unwrap();

    let outcome = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::Updated,
        "cleanup of a legacy managed entry must report Updated",
    );

    let raw = std::fs::read_to_string(&settings).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        doc.get("unrelated").and_then(|v| v.as_str()).unwrap(),
        "keep-me",
        "non-hook keys must survive cleanup",
    );
    let prompt_hooks = doc
        .get("hooks")
        .and_then(|v| v.get("UserPromptSubmit"))
        .and_then(|v| v.as_array())
        .expect("UserPromptSubmit array exists");
    let mut saw_user = false;
    let mut saw_workhive = false;
    for group in prompt_hooks {
        let Some(inner) = group.get("hooks").and_then(|v| v.as_array()) else {
            continue;
        };
        for entry in inner {
            let cmd = entry.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let managed = entry
                .get("_workhive_managed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if cmd == "/user/own.sh" {
                saw_user = true;
            }
            if managed {
                saw_workhive = true;
            }
        }
    }
    assert!(saw_user, "user-authored hook must be preserved");
    assert!(
        !saw_workhive,
        "Workhive-managed hook entry must be removed by cleanup",
    );
}

/// A legacy install left a Workhive-managed `[[hooks.*]]` table in
/// Codex's `config.toml`. Cleanup must drop the Workhive group and
/// keep the user's group untouched. The `features.hooks` /
/// `features.codex_hooks` keys that earlier releases wrote are no
/// longer added.
#[test]
fn codex_prepare_cleans_up_legacy_workhive_hook_table_and_preserves_user_group() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    std::fs::write(
        &config,
        r#"
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/user/own.sh"

[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/bin/sh -c 'exit 0' workhive-managed-hook /tmp/workhive-codex-hook.sh"
statusMessage = "Workhive managed hook (review and approve in /hooks to enable activity tracking)"
managed_by = "workhive"
"#,
    )
    .unwrap();

    let outcome = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::Updated,
        "cleanup of a legacy managed Codex hook group must report Updated",
    );

    let raw = std::fs::read_to_string(&config).unwrap();
    let doc: DocumentMut = raw.parse().unwrap();
    let user_groups = doc["hooks"]["UserPromptSubmit"].as_array_of_tables();
    let user_group_count = user_groups.map(|g| g.len()).unwrap_or(0);
    assert_eq!(
        user_group_count, 1,
        "user matcher group must remain, Workhive group must be removed",
    );
    let inner = user_groups.unwrap().get(0).unwrap()["hooks"]
        .as_array_of_tables()
        .unwrap();
    let kept_command = inner.get(0).unwrap()["command"].as_str().unwrap();
    assert_eq!(
        kept_command, "/user/own.sh",
        "user-authored Codex hook command must survive cleanup",
    );
}

/// A legacy install left a Workhive-sentinel-tagged plugin file at
/// `~/.config/opencode/plugins/workhive.mjs`. Cleanup must delete it.
#[test]
fn opencode_prepare_cleans_up_legacy_workhive_plugin() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let plugin = fake.opencode_plugin();
    std::fs::write(
        &plugin,
        "// WORKHIVE-GENERATED workhive opencode plugin\nexport default async function () {}\n",
    )
    .unwrap();

    let outcome = prepare_provider(AiActivityProvider::Opencode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::Updated,
        "cleanup of a legacy Workhive plugin must report Updated",
    );
    assert!(
        !plugin.exists(),
        "Workhive-sentinel plugin must be removed by cleanup",
    );
}

/// A user-authored plugin file with no Workhive sentinel sitting at
/// the same path must be preserved verbatim and not deleted.
#[test]
fn opencode_cleanup_preserves_user_authored_plugin_at_workhive_slot() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let plugin = fake.opencode_plugin();
    let user_body = "// user-authored, not Workhive\nexport default async () => ({})\n";
    std::fs::write(&plugin, user_body).unwrap();

    let outcome = prepare_provider(AiActivityProvider::Opencode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "user-authored file at the Workhive slot must not trigger an Updated report",
    );
    let after = std::fs::read_to_string(&plugin).unwrap();
    assert_eq!(after, user_body, "user-authored plugin must be preserved");
}

// ── Acceptance 3: cleanup error handling + parse-error surfacing ──────

/// Second prepare on an already-clean config must be a true no-op.
#[test]
fn cleanup_is_idempotent_across_providers() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _fake = FakeHome::new();

    for provider in [
        AiActivityProvider::ClaudeCode,
        AiActivityProvider::Codex,
        AiActivityProvider::Opencode,
    ] {
        let first = prepare_provider(provider).unwrap();
        let second = prepare_provider(provider).unwrap();
        assert_eq!(
            first,
            PreparationOutcome::AlreadyCurrent,
            "first prepare on a fresh box must be AlreadyCurrent for {provider:?}",
        );
        assert_eq!(
            second,
            PreparationOutcome::AlreadyCurrent,
            "second prepare must also be AlreadyCurrent for {provider:?}",
        );
    }
}

/// Schema-invalid (but syntactically valid) Claude settings file:
/// e.g. user wrote `hooks` as a string rather than an object.
/// Cleanup classifies the file as already-clean and leaves the
/// user's bytes untouched.
#[test]
fn claude_cleanup_preserves_user_bytes_on_schema_invalid_settings_json() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    let original = r#"{ "hooks": "user-owned malformed shape", "unrelated": "keep-me" }"#;
    std::fs::write(&settings, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "schema-invalid but parseable JSON must be classified as already-clean",
    );
    let after = std::fs::read_to_string(&settings).unwrap();
    assert_eq!(
        after, original,
        "schema-invalid user settings.json must not be overwritten by cleanup",
    );
    assert_eq!(
        count_workhive_backups(&settings),
        0,
        "no backup must be created on a no-op cleanup pass",
    );
}

/// Syntactically malformed Claude `settings.json` (unterminated
/// string) must surface as `IntegrationError::ParseJson`, not be
/// silently absorbed as "already clean". The user's bytes are
/// preserved byte-for-byte.
#[test]
fn claude_cleanup_surfaces_parse_error_on_malformed_json() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    let original = "{ \"hooks\": \"unterminated";
    std::fs::write(&settings, original).unwrap();

    let err = prepare_provider(AiActivityProvider::ClaudeCode)
        .expect_err("malformed JSON must propagate as a typed error, not AlreadyCurrent");
    assert!(
        matches!(err, super::IntegrationError::ParseJson { .. }),
        "expected ParseJson, got {err:?}",
    );
    let after = std::fs::read_to_string(&settings).unwrap();
    assert_eq!(
        after, original,
        "malformed user settings.json must be preserved byte-for-byte",
    );
    assert_eq!(
        count_workhive_backups(&settings),
        0,
        "no backup must be created when cleanup fails to parse",
    );
}

/// Syntactically malformed Codex `config.toml` must surface as
/// `IntegrationError::ParseToml`, not be silently absorbed as
/// "already clean". The user's bytes are preserved byte-for-byte.
#[test]
fn codex_cleanup_surfaces_parse_error_on_malformed_toml() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    let original = "[hooks.UserPromptSubmit\nmissing_close_bracket = true";
    std::fs::write(&config, original).unwrap();

    let err = prepare_provider(AiActivityProvider::Codex)
        .expect_err("malformed TOML must propagate as a typed error, not AlreadyCurrent");
    assert!(
        matches!(err, super::IntegrationError::ParseToml { .. }),
        "expected ParseToml, got {err:?}",
    );
    let after = std::fs::read_to_string(&config).unwrap();
    assert_eq!(
        after, original,
        "malformed user config.toml must be preserved byte-for-byte",
    );
    assert_eq!(
        count_workhive_backups(&config),
        0,
        "no backup must be created when cleanup fails to parse",
    );
}

// ── Acceptance 4: backup idempotency / no spurious writes ─────────────

/// A Claude `settings.json` that contains only user-authored hooks
/// (no Workhive-managed entries) must not produce a backup or any
/// rewrite when cleanup runs.
#[test]
fn claude_cleanup_writes_no_backup_for_user_only_hooks() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    let original = r#"{
      "hooks": {
        "UserPromptSubmit": [
          { "hooks": [
              { "type": "command", "command": "/user/own.sh" }
          ] }
        ]
      }
    }"#;
    std::fs::write(&settings, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "user-only hooks must classify as AlreadyCurrent",
    );
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        original,
        "user-authored settings.json must be byte-identical after cleanup",
    );
    assert_eq!(
        count_workhive_backups(&settings),
        0,
        "no backup file must be created when no Workhive entry is removed",
    );
}

/// A Codex `config.toml` that contains only user-authored
/// `[[hooks.*]]` tables (no Workhive `managed_by = "workhive"`)
/// must not produce a backup or any rewrite when cleanup runs.
#[test]
fn codex_cleanup_writes_no_backup_for_user_only_hooks() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    let original = r#"
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/user/own.sh"
"#;
    std::fs::write(&config, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "user-only Codex hooks must classify as AlreadyCurrent",
    );
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        original,
        "user-authored config.toml must be byte-identical after cleanup",
    );
    assert_eq!(
        count_workhive_backups(&config),
        0,
        "no backup file must be created when no Workhive entry is removed",
    );
}

/// A second prepare pass against a config that was already
/// cleaned by an earlier pass must not create a second backup
/// nor rewrite the file.
#[test]
fn claude_cleanup_writes_no_backup_on_second_pass_after_removal() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    std::fs::write(
        &settings,
        r#"{
          "hooks": {
            "UserPromptSubmit": [
              { "hooks": [
                  { "type": "command",
                    "command": "/bin/sh -c 'exit 0' workhive",
                    "_workhive_managed": true }
              ] }
            ]
          }
        }"#,
    )
    .unwrap();

    let first = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(first, PreparationOutcome::Updated);
    let backups_after_first = count_workhive_backups(&settings);
    assert_eq!(
        backups_after_first, 1,
        "first cleanup pass must create exactly one backup",
    );
    let body_after_first = std::fs::read_to_string(&settings).unwrap();

    let second = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(
        second,
        PreparationOutcome::AlreadyCurrent,
        "second pass must be a no-op",
    );
    assert_eq!(
        count_workhive_backups(&settings),
        backups_after_first,
        "second pass must not create another backup",
    );
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        body_after_first,
        "second pass must not rewrite the file",
    );
}

/// Same idempotency check for Codex: rerun after a successful
/// removal pass must not create a second backup.
#[test]
fn codex_cleanup_writes_no_backup_on_second_pass_after_removal() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    std::fs::write(
        &config,
        r#"
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/bin/sh -c 'exit 0' workhive"
managed_by = "workhive"
"#,
    )
    .unwrap();

    let first = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(first, PreparationOutcome::Updated);
    let backups_after_first = count_workhive_backups(&config);
    assert_eq!(
        backups_after_first, 1,
        "first cleanup pass must create exactly one backup",
    );
    let body_after_first = std::fs::read_to_string(&config).unwrap();

    let second = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(
        second,
        PreparationOutcome::AlreadyCurrent,
        "second pass must be a no-op",
    );
    assert_eq!(
        count_workhive_backups(&config),
        backups_after_first,
        "second pass must not create another backup",
    );
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        body_after_first,
        "second pass must not rewrite the file",
    );
}

/// Removal of a Workhive-managed Claude entry must take exactly one
/// backup of the pre-cleanup bytes, and the backup must contain
/// the user's original content.
#[test]
fn claude_cleanup_writes_backup_only_when_removing_workhive_entry() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    let original = r#"{
          "hooks": {
            "UserPromptSubmit": [
              { "hooks": [
                  { "type": "command", "command": "/user/own.sh" }
              ] },
              { "hooks": [
                  { "type": "command",
                    "command": "/bin/sh -c 'exit 0' workhive",
                    "_workhive_managed": true }
              ] }
            ]
          }
        }"#;
    std::fs::write(&settings, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::ClaudeCode).unwrap();
    assert_eq!(outcome, PreparationOutcome::Updated);
    assert_eq!(
        count_workhive_backups(&settings),
        1,
        "cleanup must take exactly one backup",
    );
    let dir = settings.parent().unwrap();
    let backup_path = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("settings.json.workhive-backup-"))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .expect("backup sibling must exist");
    assert_eq!(
        std::fs::read_to_string(&backup_path).unwrap(),
        original,
        "backup must contain the pre-cleanup bytes verbatim",
    );
}

/// Same backup-only-on-write contract for Codex `config.toml`.
#[test]
fn codex_cleanup_writes_backup_only_when_removing_workhive_entry() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    let original = r#"
[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/user/own.sh"

[[hooks.UserPromptSubmit]]

[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "/bin/sh -c 'exit 0' workhive"
managed_by = "workhive"
"#;
    std::fs::write(&config, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(outcome, PreparationOutcome::Updated);
    assert_eq!(
        count_workhive_backups(&config),
        1,
        "cleanup must take exactly one backup",
    );
    let dir = config.parent().unwrap();
    let backup_path = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.starts_with("config.toml.workhive-backup-"))
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .expect("backup sibling must exist");
    assert_eq!(
        std::fs::read_to_string(&backup_path).unwrap(),
        original,
        "backup must contain the pre-cleanup bytes verbatim",
    );
}

// ── Acceptance 5: Codex notify wrapper preservation ───────────────────

/// When the Codex notify gate is on, prepare must install the
/// notify wrapper into Codex `config.toml`. The wrapper is the
/// command-native completion-evidence path and is unrelated to
/// provider hook lifecycle.
#[test]
fn codex_prepare_installs_notify_wrapper_when_gate_enabled() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    std::fs::write(&config, "").unwrap();

    let cfg = codex_notify_enabled_config();
    let outcome = prepare_provider_with_config(AiActivityProvider::Codex, Some(&cfg)).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::Updated,
        "first notify install must report Updated",
    );

    let raw = std::fs::read_to_string(&config).unwrap();
    assert!(
        raw.contains("workhive-notify-begin"),
        "Codex notify wrapper sentinel must be present in config.toml when gate is on, got: {raw}",
    );
    assert!(
        raw.contains("internal activity codex-notify"),
        "wrapper body must invoke the codex-notify recorder, got: {raw}",
    );
    assert!(
        raw.contains("WORKHIVE_BIN"),
        "wrapper body must reference ${{WORKHIVE_BIN}} for runtime resolution, got: {raw}",
    );
}

/// When the Codex notify gate is off, prepare must NOT install the
/// notify wrapper (the feature-gate silent contract).
#[test]
fn codex_prepare_does_not_install_notify_when_gate_off() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    let original = "# user comment\n";
    std::fs::write(&config, original).unwrap();

    let outcome = prepare_provider(AiActivityProvider::Codex).unwrap();
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "notify gate off + no Workhive material to clean → AlreadyCurrent",
    );
    let after = std::fs::read_to_string(&config).unwrap();
    assert_eq!(
        after, original,
        "Codex config.toml must be byte-identical when notify gate is off",
    );
    assert_eq!(
        count_workhive_backups(&config),
        0,
        "no backup must be created when nothing is rewritten",
    );
}

/// A pre-existing user-authored `notify = "..."` shell-command value
/// must not be destroyed by the Codex notify wrapper install. The
/// install path captures the user's command and emits a wrapper whose
/// body invokes the recorder then `exec`s the user's command,
/// preserving the user's downstream notify behavior.
#[test]
fn codex_notify_install_does_not_destroy_user_notify_value() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    std::fs::write(&config, "notify = \"/user/notify\"\n").unwrap();

    let cfg = codex_notify_enabled_config();
    let _ = prepare_provider_with_config(AiActivityProvider::Codex, Some(&cfg)).unwrap();

    let raw = std::fs::read_to_string(&config).unwrap();
    assert!(
        raw.contains("/user/notify"),
        "user-authored notify value must survive the notify install, got: {raw}",
    );
    assert!(
        raw.contains("exec "),
        "wrapper must chain to user notify via exec, got: {raw}",
    );
}

/// Running `prepare_provider_with_config` twice with the notify gate
/// on must install the wrapper exactly once: the second run sees a
/// byte-identical config and must not create another backup nor
/// emit a duplicate sentinel region.
#[test]
fn codex_notify_install_is_idempotent_no_duplicate_sentinel() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let config = fake.codex_config();
    std::fs::write(&config, "").unwrap();

    let cfg = codex_notify_enabled_config();
    let first = prepare_provider_with_config(AiActivityProvider::Codex, Some(&cfg)).unwrap();
    assert_eq!(first, PreparationOutcome::Updated);
    let body_after_first = std::fs::read_to_string(&config).unwrap();
    let backups_after_first = count_workhive_backups(&config);
    assert_eq!(
        backups_after_first, 1,
        "first install must take exactly one backup",
    );
    let begin_count_first = body_after_first.matches("workhive-notify-begin").count();
    assert_eq!(
        begin_count_first, 1,
        "exactly one sentinel BEGIN line must appear after the first install",
    );

    let second = prepare_provider_with_config(AiActivityProvider::Codex, Some(&cfg)).unwrap();
    assert_eq!(
        second,
        PreparationOutcome::AlreadyCurrent,
        "second install run must be a no-op",
    );
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        body_after_first,
        "config.toml must be byte-identical after the idempotent second install",
    );
    assert_eq!(
        count_workhive_backups(&config),
        backups_after_first,
        "second install run must not create another backup",
    );
}

// ── Acceptance 6: inspectable cleanup plan/report ─────────────────────

/// On a fresh box, `plan_cleanup` must classify every visited
/// target as `Missing` with `would_write = false`. The inspection
/// pass must not touch the filesystem.
#[test]
fn plan_cleanup_reports_missing_for_fresh_box() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _fake = FakeHome::new();

    for provider in [
        AiActivityProvider::ClaudeCode,
        AiActivityProvider::Codex,
        AiActivityProvider::Opencode,
    ] {
        let report = plan_cleanup(provider).expect("plan_cleanup succeeds on fresh box");
        assert!(
            !report.any_write(),
            "fresh-box plan must report no writes for {provider:?}, got: {:?}",
            report.steps,
        );
        for step in &report.steps {
            assert_eq!(
                step.action,
                CleanupAction::Missing,
                "every step on a fresh box must be Missing for {provider:?}, got: {:?}",
                step,
            );
            assert_eq!(step.provider, provider);
            assert!(
                !step.would_write,
                "Missing step must not request a write for {provider:?}",
            );
        }
    }
}

/// `plan_cleanup` must report a `Remove` step when the official
/// tool config carries Workhive-managed material. The plan path
/// must not write to disk.
#[test]
fn plan_cleanup_reports_remove_for_workhive_managed_artifacts() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    std::fs::write(
        &settings,
        r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
            { "type": "command", "command": "x", "_workhive_managed": true } ] } ] } }"#,
    )
    .unwrap();
    let config = fake.codex_config();
    std::fs::write(
        &config,
        r#"
[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "x"
managed_by = "workhive"
"#,
    )
    .unwrap();
    let plugin = fake.opencode_plugin();
    std::fs::write(
        &plugin,
        "// WORKHIVE-GENERATED plugin\nexport default async () => ({});\n",
    )
    .unwrap();

    let pre_settings = std::fs::read_to_string(&settings).unwrap();
    let pre_config = std::fs::read_to_string(&config).unwrap();
    let pre_plugin = std::fs::read_to_string(&plugin).unwrap();

    for (provider, target) in [
        (AiActivityProvider::ClaudeCode, settings.clone()),
        (AiActivityProvider::Codex, config.clone()),
        (AiActivityProvider::Opencode, plugin.clone()),
    ] {
        let report = plan_cleanup(provider).expect("plan_cleanup succeeds");
        let primary = report
            .steps
            .iter()
            .find(|s| s.target.as_std_path() == target)
            .unwrap_or_else(|| {
                panic!(
                    "plan_cleanup({provider:?}) must report a step for {}, got: {:?}",
                    target.display(),
                    report.steps,
                )
            });
        assert_eq!(
            primary.action,
            CleanupAction::Remove,
            "Workhive-managed material at {} must classify as Remove, got: {primary:?}",
            target.display(),
        );
        assert!(primary.would_write, "Remove step must request a write");
        assert!(
            report.any_write(),
            "report must aggregate any_write = true when at least one step removes material",
        );
    }

    // The plan pass must not write to disk.
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), pre_settings);
    assert_eq!(std::fs::read_to_string(&config).unwrap(), pre_config);
    assert_eq!(std::fs::read_to_string(&plugin).unwrap(), pre_plugin);
    assert_eq!(count_workhive_backups(&settings), 0);
    assert_eq!(count_workhive_backups(&config), 0);
    assert_eq!(count_workhive_backups(&plugin), 0);
}

/// `plan_cleanup` must classify a user-authored OpenCode plugin
/// file (no Workhive sentinel) as `Preserve` and not propose any
/// write.
#[test]
fn plan_cleanup_reports_preserve_for_user_authored_opencode_plugin() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let plugin = fake.opencode_plugin();
    std::fs::write(
        &plugin,
        "// user-authored plugin\nexport default async () => ({});\n",
    )
    .unwrap();

    let report = plan_cleanup(AiActivityProvider::Opencode).unwrap();
    let step = report
        .steps
        .iter()
        .find(|s| s.target.as_std_path() == plugin)
        .expect("plan must include the opencode plugin path");
    assert_eq!(step.action, CleanupAction::Preserve);
    assert!(!step.would_write);
    assert!(!report.any_write());
}

/// `plan_cleanup` must classify a Claude settings.json that contains
/// only user-authored hook entries as `AlreadyClean` (file present,
/// no Workhive material to remove).
#[test]
fn plan_cleanup_reports_already_clean_for_user_only_claude_hooks() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    std::fs::write(
        &settings,
        r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
            { "type": "command", "command": "/user/own.sh" } ] } ] } }"#,
    )
    .unwrap();

    let report = plan_cleanup(AiActivityProvider::ClaudeCode).unwrap();
    let step = report
        .steps
        .iter()
        .find(|s| s.target.as_std_path() == settings)
        .expect("plan must include the claude settings path");
    assert_eq!(step.action, CleanupAction::AlreadyClean);
    assert!(!step.would_write);
    assert!(!report.any_write());
}

// ── Acceptance 7: feature-gate lifecycle behavior ─────────────────────

/// `AiActivityProvider::None` is the degenerate "no provider" sentinel
/// and must produce an `AlreadyCurrent` outcome that touches no
/// third-party config.
#[test]
fn prepare_provider_none_writes_no_third_party_config() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();

    let outcome = prepare_provider(AiActivityProvider::None).unwrap();
    assert_eq!(outcome, PreparationOutcome::AlreadyCurrent);

    assert!(!fake.home.join(".claude").join("settings.json").exists());
    assert!(!fake.home.join(".codex").join("config.toml").exists());
    assert!(
        !fake
            .xdg
            .join("opencode")
            .join("plugins")
            .join("workhive.mjs")
            .exists()
    );
}

/// `prepare_provider_if_enabled` must return `Ok(None)` when the
/// feature gate disables the provider — no config inspection occurs,
/// nothing is written, no warning is surfaced.
#[test]
fn prepare_provider_if_enabled_returns_none_when_gated_off() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    // Plant a Workhive-managed entry that *would* be cleaned if
    // the gate were on, to prove the gate truly short-circuits.
    let settings = fake.claude_settings();
    let original = r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
        { "type": "command", "command": "x", "_workhive_managed": true } ] } ] } }"#;
    std::fs::write(&settings, original).unwrap();

    let gate = FeatureGate::from_switches(true, false, true, true);
    let result = prepare_provider_if_enabled(AiActivityProvider::ClaudeCode, &gate).unwrap();
    assert!(
        result.is_none(),
        "gated-off provider must short-circuit with Ok(None)",
    );
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        original,
        "gated-off provider must not touch user config",
    );
}

/// `prepare_provider_if_enabled` must return `Ok(Some(_))` for an
/// enabled provider on a fresh box (no-op outcome).
#[test]
fn prepare_provider_if_enabled_returns_some_when_enabled() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _fake = FakeHome::new();
    let gate = FeatureGate::from_switches(true, true, true, true);

    let result = prepare_provider_if_enabled(AiActivityProvider::ClaudeCode, &gate).unwrap();
    assert_eq!(result, Some(PreparationOutcome::AlreadyCurrent));
}

/// `opportunistic_prepare_discoverable` must short-circuit when the
/// global feature gate is off. Even if every provider executable is
/// "available", no provider is visited.
#[test]
fn opportunistic_prepare_short_circuits_when_globally_disabled() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    // Plant a Workhive-managed entry that would normally be cleaned.
    let settings = fake.claude_settings();
    let original = r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
        { "type": "command", "command": "x", "_workhive_managed": true } ] } ] } }"#;
    std::fs::write(&settings, original).unwrap();

    let gate = FeatureGate::from_switches(false, true, true, true);
    let outcome = opportunistic_prepare_discoverable(&AlwaysAvailable, &gate);
    assert_eq!(outcome, PreparationOutcome::AlreadyCurrent);
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        original,
        "global gate off must skip every provider entirely",
    );
}

/// `opportunistic_prepare_discoverable` must skip providers whose
/// executable is not on `PATH`. Combined with the per-provider gate
/// behavior, this confirms the gate-and-discover-and-prepare contract.
#[test]
fn opportunistic_prepare_skips_non_discoverable_providers() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    let settings = fake.claude_settings();
    let original = r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
        { "type": "command", "command": "x", "_workhive_managed": true } ] } ] } }"#;
    std::fs::write(&settings, original).unwrap();

    let gate = FeatureGate::from_switches(true, true, true, true);
    let outcome = opportunistic_prepare_discoverable(&NoneAvailable, &gate);
    assert_eq!(
        outcome,
        PreparationOutcome::AlreadyCurrent,
        "no discoverable executable → no provider prepared",
    );
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        original,
        "no-PATH must not touch user config",
    );
}

/// `opportunistic_prepare_discoverable` must visit only the
/// providers the gate permits, even when every executable is
/// discoverable.
#[test]
fn opportunistic_prepare_visits_only_gate_enabled_providers() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let fake = FakeHome::new();
    // Plant Workhive-managed material for all three providers.
    let claude_settings = fake.claude_settings();
    let claude_original = r#"{ "hooks": { "UserPromptSubmit": [ { "hooks": [
        { "type": "command", "command": "x", "_workhive_managed": true } ] } ] } }"#;
    std::fs::write(&claude_settings, claude_original).unwrap();

    let codex_config = fake.codex_config();
    let codex_original = r#"
[[hooks.UserPromptSubmit]]
[[hooks.UserPromptSubmit.hooks]]
type = "command"
command = "x"
managed_by = "workhive"
"#;
    std::fs::write(&codex_config, codex_original).unwrap();

    let plugin = fake.opencode_plugin();
    std::fs::write(
        &plugin,
        "// WORKHIVE-GENERATED plugin\nexport default async () => ({});\n",
    )
    .unwrap();

    // Enable global + Codex only.
    let gate = FeatureGate::from_switches(true, false, true, false);
    let outcome = opportunistic_prepare_discoverable(&AlwaysAvailable, &gate);
    assert_eq!(
        outcome,
        PreparationOutcome::Updated,
        "Codex must be cleaned when its per-provider gate is on",
    );

    // Codex was cleaned.
    assert!(
        !std::fs::read_to_string(&codex_config)
            .unwrap()
            .contains("managed_by = \"workhive\"")
    );

    // Claude + OpenCode were NOT visited: their material is intact.
    assert_eq!(
        std::fs::read_to_string(&claude_settings).unwrap(),
        claude_original,
        "gated-off Claude must remain untouched",
    );
    assert!(
        plugin.exists(),
        "gated-off OpenCode plugin must remain untouched",
    );
}

// ── Test doubles for `ExecutableAvailabilityPort` ─────────────────────

struct AlwaysAvailable;
impl ExecutableAvailabilityPort for AlwaysAvailable {
    fn is_available(&self, _program: &str) -> bool {
        true
    }
}

struct NoneAvailable;
impl ExecutableAvailabilityPort for NoneAvailable {
    fn is_available(&self, _program: &str) -> bool {
        false
    }
}

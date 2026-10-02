//! `workhive internal pane-supervisor` handler.
//!
//! Hidden subcommand spawned by Workhive itself when launching a tool
//! pane: the supervisor process becomes the pane's foreground PID and
//! writes authoritative metadata under
//! `<runtime_root>/<group>/<workspace>/panes/<unit>.json`. When the
//! supervised child exits, the supervisor records the final state and
//! propagates the child's exit code.
//!
//! ## Why this is a CLI handler, not a library entry point
//!
//! Workhive's pane runtime can only inject *one* command into a tmux
//! pane (via `send-keys`). To make the supervisor become the pane's
//! tracked process, the wrapped command sent into the pane is itself
//! `workhive internal pane-supervisor … -- <real program> <real args>`.
//! That requires a real CLI subcommand, not a library API.
//!
//! ## Argv contract
//!
//! Everything after `--` is treated as typed argv for the wrapped
//! tool. The handler does not reparse a shell string: argv is
//! preserved as a `Vec<String>` from clap directly into
//! [`PaneSupervisorRun`].

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::{AiActivityProvider, UnitId};
use workhive_infra::pane_supervisor::runner::{
    PaneSupervisorOutcome, PaneSupervisorRun, run_supervisor,
};

use crate::commands::integrations::{
    IntegrationError, PreparationOutcome, PrepareErrorKind, prepare_provider_if_enabled,
};
use crate::context::CommandContext;
use crate::error::CliError;
use workhive_core::ports::{ConfigPort, I18nPort, ManagedToolExitBehavior};
use workhive_core::usecases::ai_integrations::{FeatureGate, FeatureGateResolution};
use workhive_core::usecases::config_mgmt::resolve_effective_tool_exit_behavior;

/// Validate the structured arguments for the supervisor subcommand
/// and produce a [`PaneSupervisorRun`] ready for [`run_supervisor`].
///
/// Pure: no I/O, fully unit-testable. Returns a structured `CliError`
/// for empty argv so the integration test suite can exercise the
/// validation path without spawning a child.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_supervisor_run(
    runtime_root: String,
    group: String,
    workspace: String,
    unit: String,
    tool: Option<String>,
    program_path: Option<String>,
    env_vars: Vec<String>,
    argv: Vec<String>,
) -> Result<PaneSupervisorRun, CliError> {
    if runtime_root.is_empty() {
        return Err(CliError::Runtime(
            "internal pane-supervisor: --runtime-root must not be empty".to_owned(),
        ));
    }
    if group.is_empty() {
        return Err(CliError::Runtime(
            "internal pane-supervisor: --group must not be empty".to_owned(),
        ));
    }
    if workspace.is_empty() {
        return Err(CliError::Runtime(
            "internal pane-supervisor: --workspace must not be empty".to_owned(),
        ));
    }
    if unit.is_empty() {
        return Err(CliError::Runtime(
            "internal pane-supervisor: --unit must not be empty".to_owned(),
        ));
    }
    let mut iter = argv.into_iter();
    let program = match iter.next() {
        Some(p) if !p.is_empty() => p,
        _ => {
            return Err(CliError::Runtime(
                "internal pane-supervisor: missing program after `--` separator".to_owned(),
            ));
        }
    };
    let args: Vec<String> = iter.collect();
    let command_basename = tool.clone().unwrap_or_else(|| derive_basename(&program));

    let mut env = std::collections::BTreeMap::new();
    for entry in &env_vars {
        let Some((key, value)) = entry.split_once('=') else {
            return Err(CliError::Runtime(format!(
                "internal pane-supervisor: malformed --env entry '{}': expected KEY=VALUE format",
                entry,
            )));
        };
        if key.is_empty() {
            return Err(CliError::Runtime(format!(
                "internal pane-supervisor: malformed --env entry '{}': key must not be empty",
                entry,
            )));
        }
        env.insert(key.to_owned(), value.to_owned());
    }

    Ok(PaneSupervisorRun {
        runtime_root: Utf8PathBuf::from(runtime_root),
        group,
        workspace,
        unit_id: UnitId::new(unit),
        command_basename,
        program_path,
        program,
        args,
        env,
    })
}

/// Derive a short basename from a program path. Mirrors the helper in
/// [`workhive_infra::tool_launch_instrumentation`] so the metadata
/// produced by the supervisor matches what the wrapper-side path
/// recorded.
fn derive_basename(program: &str) -> String {
    program
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(program)
        .to_owned()
}

/// `workhive internal pane-supervisor` handler.
///
/// Returns the exit code the CLI process should propagate. Spawn
/// failures map to `127` so consumers can distinguish "no such
/// program" from a child that ran and exited normally.
///
/// The supervisor performs a provider-scoped automatic prepare
/// *before* spawning the child. Errors during prepare are classified
/// by [`IntegrationError::classify`]:
///
/// * Safety refusals (would clobber user-authored material) fail
///   closed — the supervisor returns a localized [`CliError`]
///   without spawning the child, so the user is forced to resolve
///   the conflict instead of unknowingly running with an inert hook.
/// * Transient I/O / parse errors fail open with a localized
///   `tracing::warn!` line. The supervisor still spawns the child;
///   pane activity simply degrades to a lower-accuracy source until
///   the next launch retries.
//
// `clippy::too_many_arguments` is permitted here: this is a CLI
// dispatcher that mirrors the typed `clap` derive on
// `Commands::InternalPaneSupervisor`. Folding the args into a struct
// would push the same clap shape into the cli module and add
// non-trivial churn that the file-size discipline explicitly cautions against.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_internal_pane_supervisor(
    project_root: Option<String>,
    runtime_root: String,
    group: String,
    workspace: String,
    unit: Option<String>,
    tool: Option<String>,
    program_path: Option<String>,
    env_vars: Vec<String>,
    argv: Vec<String>,
    ctx: &CommandContext,
) -> Result<i32, CliError> {
    let unit = unit
        .or_else(workhive_infra::runtime::inherited_runtime_unit_id)
        .ok_or_else(|| {
            CliError::Runtime(
                "pane supervisor requires a runtime-provided unit identity".to_owned(),
            )
        })?;
    let run = build_supervisor_run(
        runtime_root,
        group,
        workspace,
        unit,
        tool,
        program_path,
        env_vars,
        argv,
    )?;
    // Mandatory provider-scoped prepare immediately before Workhive
    // spawns Claude/Codex/OpenCode. This is the strong fallback:
    // even if `init --global` predates the provider install, the
    // very first pane Workhive launches for the provider self-heals
    // its hook/plugin wiring.
    //
    // Provider detection routes through the typed
    // `AiActivityProvider::from_tool_basename` metadata — no
    // substring matching on argv. Non-AI panes (shell, lazygit, …)
    // resolve to `AiActivityProvider::None`, which `prepare_provider`
    // contractually treats as a no-op so we never write Claude/
    // Codex/OpenCode config from a non-AI launch.
    //
    // `project_root` arrives as typed argv from
    // `tool_launch_instrumentation` so the supervisor evaluates the
    // **project-over-global** AI integrations feature gate, identical
    // to `pane list` / `pane show`. Reconstructing it by truncating
    // `runtime_root` at runtime would be a forbidden low-confidence
    // string heuristic.
    let project_root_path = project_root.as_deref().map(Utf8Path::new);
    maybe_prepare_for_provider(&run.command_basename, ctx, project_root_path)?;
    let outcome = run_supervisor(run.clone());
    let code = outcome.to_exit_code();
    let spawn_failed = matches!(&outcome, PaneSupervisorOutcome::SpawnFailed(_));
    if let PaneSupervisorOutcome::SpawnFailed(err) = &outcome {
        tracing::error!(error = %err, "internal pane-supervisor: spawn failed");
        // Surface a localized one-line message on stderr so the user
        // sees "why" before the replacement shell takes over (under
        // respawn_shell) or the pane closes (under close_pane). Either
        // way the supervisor must not vanish silently.
        let err_text = err.to_string();
        let msg = ctx.i18n.resolve_with_args(
            ctx.locale(),
            workhive_core::i18n_keys::PANE_SUPERVISOR_SPAWN_FAILED,
            &[run.program.as_str(), err_text.as_str()],
        );
        eprintln!("{msg}");
    }
    // Activity Intelligence: fire the unified `notification_trigger`
    // evaluator at supervisor exit so even legacy-only paths (where
    // ACP events were not appended through
    // `record_activity_dual_write`) still get one
    // notification-evaluation pass. The same trigger is also wired
    // into the live ACP ingestion sites
    // (`workhive internal activity record`, `… hook`,
    // `… codex-notify`) so `permission-needed` and `failed` events
    // surface within milliseconds of being recorded — not at
    // supervisor exit.
    //
    // The unified trigger uses
    // `workhive_infra::notification::production_notification_port()`
    // which composes the macOS osascript user-attention adapter with
    // the legacy `LocalLogNotificationPort` diagnostic adapter.
    // `osascript` failures are swallowed; the log adapter always
    // emits the structured `workhive.notification` breadcrumb, so
    // the prior behavior remains intact while real OS notifications
    // are delivered on the primary platform.
    //
    // Fail-open: any error loading ACP events or config is logged at
    // `debug` and ignored — the supervisor's primary job (propagating
    // the child's exit code) must not be blocked by notification failure.
    deliver_supervisor_notifications(&run, project_root_path, ctx);
    apply_managed_tool_exit_policy(&run, project_root_path, ctx, spawn_failed, code)
}

/// Apply the `pane.exit.tool_exit_behavior` policy after the
/// supervised child has exited (or after spawn failed).
///
/// Policy resolution semantics:
///
/// * **Config absent** (no global file, or the field is missing) —
///   the built-in default `respawn_shell` applies. This is the only
///   path that defaults to respawn.
/// * **Config trusted and explicit** — the user's `respawn_shell`
///   or `close_pane` value is honored exactly.
/// * **Config untrusted** (load failure, parse failure, invalid
///   value) — the supervisor **must not** silently fall back to
///   `respawn_shell`. The user neither configured nor expressed
///   that behavior. We degrade to `close_pane` (the pre-policy
///   default) and surface a single localized stderr line so the
///   user knows why their pane closed and where to look.
///
/// When the resolved behavior is `RespawnShell` this function
/// attempts to replace the current process image with a fresh
/// interactive shell; on success the call never returns. When the
/// behavior is `ClosePane` — or when the respawn attempt itself
/// fails — the supervisor returns the child's exit code so the
/// pane closes normally.
///
/// The `spawn_failed` flag is informational: spawn failures
/// short-circuit ahead of policy resolution so the documented exit
/// code (e.g. 127) stays observable to CI matrices and integration
/// tests.
fn apply_managed_tool_exit_policy(
    run: &PaneSupervisorRun,
    project_root: Option<&Utf8Path>,
    ctx: &CommandContext,
    spawn_failed: bool,
    code: i32,
) -> Result<i32, CliError> {
    // Spawn failure is structurally distinct from "the supervised
    // child exited". The policy targets managed *child* exit
    // (normal, signal, user-driven). When the child never started —
    // wrong path, missing binary, permission denied — the right
    // user-visible outcome is the documented spawn-failed exit code
    // (127). Silently respawning a shell here would hide the failure
    // from CI matrices and integration tests, and break the
    // `pane_supervisor_does_not_write_third_party_config_for_shell_launches`
    // contract that asserts the spawn-failed surface stays observable.
    if spawn_failed {
        tracing::debug!(
            unit_id = %run.unit_id,
            exit_code = code,
            "pane supervisor: spawn failed; bypassing exit policy to preserve exit-code visibility"
        );
        return Ok(code);
    }

    let decision = resolve_tool_exit_behavior(&ctx.config, project_root);
    let behavior = match decision {
        ToolExitBehaviorDecision::Resolved(behavior) => {
            tracing::debug!(
                unit_id = %run.unit_id,
                exit_code = code,
                behavior = behavior.as_str(),
                "pane supervisor: applying managed tool exit policy"
            );
            behavior
        }
        ToolExitBehaviorDecision::Untrusted { reason } => {
            // Fail-closed. We refuse to apply `respawn_shell` when
            // the config could not be trusted, because that would
            // honor a behavior the user neither set nor accepted.
            // Degrading to `close_pane` keeps the pre-policy default
            // and preserves the child's exit code in the pane scroll.
            tracing::warn!(
                unit_id = %run.unit_id,
                exit_code = code,
                reason = %reason,
                "pane supervisor: exit-policy config untrusted; refusing to respawn shell, pane will close"
            );
            let msg = ctx.i18n.resolve_with_args(
                ctx.locale(),
                workhive_core::i18n_keys::PANE_SUPERVISOR_EXIT_BEHAVIOR_UNTRUSTED,
                &[reason.as_str()],
            );
            eprintln!("{msg}");
            return Ok(code);
        }
    };

    match behavior {
        ManagedToolExitBehavior::ClosePane => Ok(code),
        ManagedToolExitBehavior::RespawnShell => {
            let cwd = project_root;
            match workhive_infra::replacement_shell::exec_replacement_shell(cwd) {
                Ok(()) => {
                    // Unreachable on Unix (exec replaces the process).
                    // Non-Unix fallback inside `exec_replacement_shell`
                    // calls `process::exit` itself before returning, so
                    // reaching here means the call truly returned
                    // `Ok(())` without execing — degrade gracefully by
                    // propagating the child's exit code.
                    Ok(code)
                }
                Err(err) => {
                    // Fail open: log + degrade to "pane closes" rather
                    // than abort, mirroring the pre-policy behavior.
                    // We use the existing supervisor error path: print
                    // a localized line on stderr, propagate the code.
                    tracing::warn!(
                        unit_id = %run.unit_id,
                        error = %err,
                        "pane supervisor: replacement shell exec failed; pane will close"
                    );
                    let err_text = err.to_string();
                    let msg = ctx.i18n.resolve_with_args(
                        ctx.locale(),
                        workhive_core::i18n_keys::PANE_SUPERVISOR_REPLACEMENT_SHELL_FAILED,
                        &[err_text.as_str()],
                    );
                    eprintln!("{msg}");
                    Ok(code)
                }
            }
        }
    }
}

/// Typed decision returned by [`resolve_tool_exit_behavior`].
///
/// The supervisor distinguishes three states even though only two
/// `ManagedToolExitBehavior` variants exist on the wire:
///
/// * `Resolved(behavior)` — config was absent or successfully
///   loaded *and* the resolved value is one of the typed variants
///   (`RespawnShell` / `ClosePane`). The supervisor honors it.
///   `Resolved(RespawnShell)` is the built-in default when no
///   global or project config sets the field.
/// * `Untrusted { reason }` — at least one config scope failed to
///   load, failed to parse, or carried an invalid value. The
///   supervisor refuses to silently fall back to `respawn_shell`
///   here; instead the caller degrades to `close_pane` so the
///   user's exit code stays visible and the user is told why.
///
/// This mirrors [`NotificationsConfigDecision`] further down in
/// this module: write-path effects must never run on the back of
/// "could not read config" being treated as "use the default".
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ToolExitBehaviorDecision {
    /// Trusted, typed value the supervisor should honor.
    Resolved(ManagedToolExitBehavior),
    /// At least one config scope could not be trusted. The caller
    /// MUST refuse to apply `respawn_shell` and degrade to a safe
    /// observable behavior (`close_pane`).
    Untrusted { reason: String },
}

/// Resolve the managed-tool exit behavior using the same
/// project-over-global precedence as every other config consumer.
///
/// Returns a typed decision so the caller distinguishes
/// "trusted absent → built-in default" from "config could not be
/// trusted → user-visible degraded path". Pure: no I/O once the
/// `ConfigPort` returns.
pub(crate) fn resolve_tool_exit_behavior(
    config: &impl ConfigPort,
    project_root: Option<&Utf8Path>,
) -> ToolExitBehaviorDecision {
    let global = match config.load_global() {
        Ok(opt) => opt,
        Err(err) => {
            return ToolExitBehaviorDecision::Untrusted {
                reason: format!("global config untrusted: {err}"),
            };
        }
    };
    let project = match project_root {
        Some(root) => match config.load_project(root) {
            Ok(opt) => opt,
            Err(err) => {
                return ToolExitBehaviorDecision::Untrusted {
                    reason: format!("project config untrusted ({root}): {err}"),
                };
            }
        },
        None => None,
    };
    ToolExitBehaviorDecision::Resolved(resolve_effective_tool_exit_behavior(
        project.as_ref(),
        global.as_ref(),
    ))
}

/// Typed decision returned by
/// [`resolve_supervisor_notifications_config`].
///
/// `Trusted(Some(cfg))` — user-configured notifications block was
/// loaded successfully. `Trusted(None)` — both global and project
/// configs loaded successfully and neither declares a notifications
/// block; the supervisor passes `None` to the delivery use case which
/// then applies the built-in defaults (this matches the semantics
/// before the bug: a missing config is not a trust failure).
/// `Untrusted` — at least one config scope failed to load or parse.
/// In that case the supervisor MUST NOT emit notifications because
/// the user's intended feature gates / per-category settings cannot
/// be honored.
#[derive(Debug)]
pub(crate) enum NotificationsConfigDecision {
    Trusted(Option<workhive_core::ports::NotificationsConfig>),
    Untrusted { reason: String },
}

/// Load global + project config and produce a
/// typed decision the supervisor's delivery path can switch on.
///
/// A bare `unwrap_or(None)` pattern would silently downgrade any
/// `ConfigError` to "no config" — which downstream would interpret as
/// "use built-in defaults" (enabled). Instead, a failed load surfaces
/// as [`NotificationsConfigDecision::Untrusted`] and the caller
/// refuses to emit any notification.
///
/// Generic over `&impl ConfigPort` so the production `TomlConfigAdapter`
/// and the testkit's `FakeConfig` (with `with_global_error` /
/// `with_project_error`) can both drive it.
pub(crate) fn resolve_supervisor_notifications_config<C>(
    config_port: &C,
    project_root: Option<&Utf8Path>,
) -> NotificationsConfigDecision
where
    C: workhive_core::ports::ConfigPort + ?Sized,
{
    let global = match config_port.load_global() {
        Ok(g) => g,
        Err(e) => {
            return NotificationsConfigDecision::Untrusted {
                reason: format!("global config untrusted: {e}"),
            };
        }
    };
    let project = if let Some(r) = project_root {
        match config_port.load_project(r) {
            Ok(p) => p,
            Err(e) => {
                return NotificationsConfigDecision::Untrusted {
                    reason: format!("project config untrusted: {e}"),
                };
            }
        }
    } else {
        None
    };
    let merged = match (global, project) {
        (Some(g), Some(p)) => {
            Some(workhive_core::usecases::config_mgmt::resolve_effective_config(&g, &p))
        }
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };
    NotificationsConfigDecision::Trusted(
        merged.and_then(|cfg| cfg.features.and_then(|f| f.notifications)),
    )
}

/// Activity Intelligence v1 (D3): fire notification delivery after the
/// supervised child exits. Fail-open for *internal* pipeline failures
/// (ACP event read errors): any I/O error loading ACP events is logged
/// at `debug` and silently swallowed so the supervisor's primary job
/// (propagating the child's exit code) is never blocked.
///
/// Fail-**closed** for *config trust* failures:
/// when global or project config cannot be loaded or parsed, the
/// supervisor MUST NOT emit notifications, because the user's
/// configured feature gates / per-category settings cannot be trusted.
/// The supervisor itself continues running and propagates the child's
/// exit code. Previously this path used `unwrap_or(None)` to silently
/// downgrade a load/parse error to "no config" — which then handed the
/// downstream delivery layer the *built-in defaults* (notifications
/// enabled), letting the system emit notifications the user might have
/// disabled.
///
/// Uses [`workhive_infra::notification::LocalLogNotificationPort`] as
/// the production adapter. Selecting any
/// `Noop*` adapter here on the production path is forbidden — see the
/// adapter's module docs for rationale.
fn deliver_supervisor_notifications(
    run: &PaneSupervisorRun,
    project_root: Option<&Utf8Path>,
    ctx: &CommandContext,
) {
    use crate::commands::notification_trigger::{
        NotificationTriggerSource, evaluate_after_acp_append,
    };
    use workhive_core::domain::activity::PaneCorrelationKey;
    use workhive_core::ports::ClockPort as _;
    use workhive_infra::clock::SystemClock;

    let pane_key = PaneCorrelationKey::new(
        run.group.clone(),
        run.workspace.clone(),
        run.unit_id.as_str().to_string(),
    );
    let effective_root: &Utf8Path = if let Some(r) = project_root {
        r
    } else {
        run.runtime_root.as_path()
    };
    let event_store = ctx.composition.activity_event_store(effective_root);
    let port = workhive_infra::notification::production_notification_port();
    let now_ms = SystemClock.now_unix_ms();
    // Persistent outbox so supervisor-exit triggered
    // deliveries do not duplicate the event-driven trigger that fires
    // mid-turn from `cmd_internal_activity_record`/`hook`/`codex-notify`.
    let outbox_store = workhive_infra::notification_outbox_store::FsNotificationOutboxStore::new(
        run.runtime_root.clone(),
    );
    evaluate_after_acp_append(
        &ctx.config,
        project_root,
        &event_store,
        Some(&outbox_store),
        &pane_key,
        NotificationTriggerSource::SupervisorExit,
        port.as_ref(),
        now_ms,
    );
    tracing::debug!(
        unit_id = %run.unit_id,
        "pane supervisor: notification trigger evaluation complete"
    );
}

/// Run the silent automatic-prepare lifecycle for the provider
/// that the supervisor is about to spawn, if any. Routes the typed
/// [`IntegrationError`] through [`PrepareErrorKind`] to decide
/// between fail-closed (safety refusal) and fail-open (transient).
///
/// The AI integrations feature gate is resolved as
/// **project-over-global** through [`supervisor_feature_gate`],
/// mirroring the gate evaluation used by `pane list` / `pane show`.
/// When the gate is disabled globally or for this provider — or when
/// the resolution itself is [`FeatureGateResolution::Unavailable`]
/// because config could not be trusted — the supervisor performs
/// zero integration writes and emits no user-facing output. Only a
/// single `tracing::debug!` breadcrumb is recorded so the
/// fail-closed write path is observable in logs.
fn maybe_prepare_for_provider(
    command_basename: &str,
    ctx: &CommandContext,
    project_root: Option<&Utf8Path>,
) -> Result<(), CliError> {
    let provider = AiActivityProvider::from_tool_basename(command_basename);
    if matches!(provider, AiActivityProvider::None) {
        return Ok(());
    }
    let resolution = supervisor_feature_gate(&ctx.config, project_root);
    let gate = match resolution {
        FeatureGateResolution::Resolved(gate) => gate,
        FeatureGateResolution::Unavailable { reason } => {
            // Feature gate read/parse failure MUST NOT fall back to
            // enabled. The supervisor's automatic prepare is a write
            // path; failing closed is the only safe option when the
            // gate cannot be trusted.
            tracing::debug!(
                provider = provider.wire_name(),
                reason = %reason,
                "pane supervisor: AI integrations feature gate unavailable; skipping prepare (fail-closed write path)"
            );
            return Ok(());
        }
    };
    match prepare_provider_if_enabled(provider, &gate) {
        Ok(None) => Ok(()), // gate-blocked: silent
        Ok(Some(PreparationOutcome::AlreadyCurrent)) => {
            tracing::debug!(
                provider = provider.wire_name(),
                "pane supervisor: integration already up to date"
            );
            Ok(())
        }
        Ok(Some(PreparationOutcome::Updated)) => {
            tracing::debug!(
                provider = provider.wire_name(),
                "pane supervisor: integration prepared"
            );
            Ok(())
        }
        Err(err) => classify_prepare_failure(err, provider, ctx),
    }
}

/// Resolve the AI integrations feature gate for the supervisor.
///
/// Spawn-time prepare MUST consult the project-over-global effective
/// feature gate:
///
/// * When a typed `project_root` is supplied by
///   `tool_launch_instrumentation`, the supervisor merges global +
///   project config via
///   [`workhive_core::usecases::config_mgmt::resolve_effective_config`],
///   yielding the same gate the pane render path uses.
/// * When `project_root` is absent, the supervisor resolves the
///   gate from global config only.
/// * On **any** config load failure the supervisor returns
///   [`FeatureGateResolution::Unavailable`] so the write path can
///   fail closed — never "fall back to enabled".
fn supervisor_feature_gate(
    config: &dyn ConfigPort,
    project_root: Option<&Utf8Path>,
) -> FeatureGateResolution {
    let global = match config.load_global() {
        Ok(opt) => opt,
        Err(err) => {
            tracing::warn!(
                error = %err,
                "pane supervisor: failed to load global config for AI integrations feature gate; skipping prepare (fail-closed)"
            );
            return FeatureGateResolution::Unavailable {
                reason: format!("failed to load global config: {err}"),
            };
        }
    };
    let project = match project_root {
        Some(root) => match config.load_project(root) {
            Ok(opt) => opt,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    project_root = %root,
                    "pane supervisor: failed to load project config for AI integrations feature gate; skipping prepare (fail-closed)"
                );
                return FeatureGateResolution::Unavailable {
                    reason: format!("failed to load project config: {err}"),
                };
            }
        },
        None => None,
    };
    let merged = match (global, project) {
        (Some(g), Some(p)) => {
            Some(workhive_core::usecases::config_mgmt::resolve_effective_config(&g, &p))
        }
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };
    FeatureGateResolution::Resolved(FeatureGate::from_config(merged.as_ref()))
}

/// Convert a typed [`IntegrationError`] into the supervisor's
/// fail-open / fail-closed boundary signal. Safety refusals become
/// a [`CliError`] carrying the localized message; transient errors
/// are logged at `warn` and the supervisor continues.
fn classify_prepare_failure(
    err: IntegrationError,
    provider: AiActivityProvider,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let locale = ctx.locale();
    let message = err.localized_message(&ctx.i18n, locale, provider);
    match err.classify() {
        PrepareErrorKind::Transient => {
            tracing::warn!(
                provider = provider.wire_name(),
                kind = "transient",
                error = %err,
                "pane supervisor: automatic integration prepare failed; continuing to spawn child (activity rendering will degrade)"
            );
            // Emit a localized warning on stderr so the user does
            // not believe the integration is fully prepared. The
            // supervisor still proceeds to spawn the child — activity
            // simply degrades to a lower-accuracy source until the
            // next launch.
            eprintln!("{message}");
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "pane_supervisor_tests.rs"]
mod tests;

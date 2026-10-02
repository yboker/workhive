//! `workhive internal activity record` and `workhive internal activity hook` handlers.
//!
//! These are the entry points installed hook scripts and the OpenCode
//! plugin invoke for every provider lifecycle event:
//!
//! * `record` — reads a Workhive-flavoured `ActivityRecordInput` JSON
//!   payload from stdin. Used by internal tests and by the OpenCode
//!   plugin once it has translated its event into the canonical
//!   schema.
//! * `hook --provider <claude|codex|opencode>` — reads the *official*
//!   provider stdin payload (e.g. Claude Code hook JSON containing
//!   `hook_event_name`) and adapts it to the canonical fact before
//!   persisting it. This is the entry point Workhive-generated hook
//!   shell scripts pipe their stdin into.
//!
//! Architecture / boundary rules:
//! - The CLI only reads stdin and resolves the runtime root /
//!   provider / pane correlation from environment variables that
//!   Workhive itself populated when it spawned the tool. Payload
//!   identity fields (`group`, `workspace`, `unit_id`) are *advisory*
//!   and are validated against the env-derived trusted tuple — they
//!   never act as the routing key. See review High 1.
//! - All policy, schema validation, provider mismatch checks, and
//!   merge rules live in [`workhive_core::usecases::ai_activity`].
//! - The store implementation comes from
//!   [`workhive_infra::pane_activity::FsPaneActivityStore`].

use std::io::Read;

use camino::{Utf8Path, Utf8PathBuf};
use workhive_core::domain::AI_ACTIVITY_SCHEMA_VERSION;
use workhive_core::domain::AiActivityProvider;
use workhive_core::domain::activity::{
    ActivityEvent, ActivityEventKind, ActivityOutcome, ObservationConfidence, PaneCorrelationKey,
    ProviderEventKind, ProviderKind, ProviderSignalSource, SignalChannel,
};
use workhive_core::i18n_keys;
use workhive_core::ports::ActivityEventStore as _;
use workhive_core::ports::{ConfigPort, I18nPort};
use workhive_core::usecases::ai_activity::{
    ActivityControlPlaneGate, ActivityRecordError, ActivityRecordInput, map_claude_event,
    map_codex_event, map_opencode_event, record_activity_dual_write,
};
use workhive_core::usecases::ai_integrations::FeatureGate;
use workhive_infra::pane_activity::FsPaneActivityStore;

use crate::cli::IntegrationProviderArg;
use crate::commands::notification_trigger::{NotificationTriggerSource, evaluate_after_acp_append};
use crate::context::CommandContext;
use crate::error::CliError;

/// Environment variable that pins the runtime root for hook-driven
/// activity recording. Set by the pane supervisor when it spawns the
/// AI-assistant child process; missing means we cannot route the
/// event to the right pane and must refuse the write.
const ENV_RUNTIME_ROOT: &str = "WORKHIVE_RUNTIME_ROOT";
/// Environment variable that pins the expected provider for the
/// current pane. Used to reject events whose JSON `provider` field
/// does not match what Workhive launched.
const ENV_TOOL_PROVIDER: &str = "WORKHIVE_TOOL_PROVIDER";
/// Trusted pane-identity environment variables injected by the
/// supervisor. These are the routing source of truth — payload
/// fields are validated against them so a hostile or buggy hook
/// payload cannot rewrite the destination pane key.
const ENV_GROUP: &str = "WORKHIVE_GROUP";
const ENV_WORKSPACE: &str = "WORKHIVE_WORKSPACE";
const ENV_UNIT_ID: &str = "WORKHIVE_UNIT_ID";
/// Optional override for the foreground tool name surfaced in the
/// fact (e.g. `bash` vs `claude`). Defaults to the canonical tool
/// name when absent.
const ENV_TOOL_NAME: &str = "WORKHIVE_TOOL_NAME";

/// Trusted correlation tuple resolved from the env vars Workhive
/// itself set when spawning the tool process. We never accept
/// `group`/`workspace`/`unit_id` directly from stdin.
struct TrustedCorrelation {
    runtime_root: Utf8PathBuf,
    provider: AiActivityProvider,
    group: String,
    workspace: String,
    unit_id: String,
    tool_name: String,
}

fn resolve_trusted_correlation(ctx: &CommandContext) -> Result<TrustedCorrelation, CliError> {
    let runtime_root = required_env(
        ctx,
        ENV_RUNTIME_ROOT,
        i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_RUNTIME_ROOT,
    )?;
    let expected_provider_wire = required_env(
        ctx,
        ENV_TOOL_PROVIDER,
        i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_PROVIDER,
    )?;
    let expected_provider =
        AiActivityProvider::from_wire(&expected_provider_wire).ok_or_else(|| {
            CliError::Runtime(localized_error_with_arg(
                ctx,
                i18n_keys::ACTIVITY_RECORD_ERROR_UNKNOWN_PROVIDER,
                &expected_provider_wire,
            ))
        })?;
    if matches!(expected_provider, AiActivityProvider::None) {
        return Err(CliError::Runtime(localized_error(
            ctx,
            i18n_keys::ACTIVITY_RECORD_ERROR_NO_PROVIDER_ACTIVE,
        )));
    }
    let group = required_env(
        ctx,
        ENV_GROUP,
        i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_GROUP,
    )?;
    let workspace = required_env(
        ctx,
        ENV_WORKSPACE,
        i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_WORKSPACE,
    )?;
    let unit_id = required_env(
        ctx,
        ENV_UNIT_ID,
        i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_UNIT_ID,
    )?;
    let tool_name = std::env::var(ENV_TOOL_NAME).unwrap_or_else(|_| match expected_provider {
        AiActivityProvider::ClaudeCode => "claude".to_owned(),
        AiActivityProvider::Codex => "codex".to_owned(),
        AiActivityProvider::Opencode => "opencode".to_owned(),
        AiActivityProvider::None => String::new(),
    });
    Ok(TrustedCorrelation {
        runtime_root: Utf8PathBuf::from(runtime_root),
        provider: expected_provider,
        group,
        workspace,
        unit_id,
        tool_name,
    })
}

fn required_env(ctx: &CommandContext, key: &str, missing_i18n: &str) -> Result<String, CliError> {
    let value =
        std::env::var(key).map_err(|_| CliError::Runtime(localized_error(ctx, missing_i18n)))?;
    if value.is_empty() {
        return Err(CliError::Runtime(localized_error(ctx, missing_i18n)));
    }
    Ok(value)
}

/// Read JSON from stdin, validate it, and persist a single activity
/// fact. Failures map to [`CliError::Runtime`] so the caller (a hook
/// script) can fail loudly. Hooks are expected to suppress non-zero
/// exits if they want fully silent fall-through; we are deliberately
/// not silent here so misconfigurations surface quickly.
///
/// Payload identity fields (`group`, `workspace`, `unit_id`) are
/// validated against the env-derived trusted tuple and rejected if
/// they disagree; they never act as the routing key.
pub(crate) fn cmd_internal_activity_record(ctx: &CommandContext) -> Result<(), CliError> {
    let trust = resolve_trusted_correlation(ctx)?;
    let trimmed = read_stdin_payload(ctx)?;
    let mut input: ActivityRecordInput = serde_json::from_str(&trimmed).map_err(|e| {
        CliError::Runtime(format!(
            "{}: {e}",
            localized_error(ctx, i18n_keys::ACTIVITY_RECORD_ERROR_INVALID_JSON)
        ))
    })?;
    // Reject payload identity drift loudly — this prevents a hostile
    // hook from rewriting the destination pane key even before the
    // store's sanitiser runs.
    enforce_trusted_identity(ctx, &input, &trust)?;
    // Authoritative override: pin identity to the env tuple so the
    // store cannot be tricked by payload values that happen to pass
    // sanitisation but still drift from the supervisor's truth.
    input.group = trust.group.clone();
    input.workspace = trust.workspace.clone();
    input.unit_id = trust.unit_id.clone();

    let legacy_store = FsPaneActivityStore::new(trust.runtime_root.clone());
    let event_store = ctx
        .composition
        .activity_event_store_from_runtime_root(&trust.runtime_root);
    let gate = build_acp_gate_from_runtime_root(ctx, &trust.runtime_root, trust.provider);
    record_activity_dual_write(&input, trust.provider, &legacy_store, &event_store, gate)
        .map_err(|err| map_record_error(ctx, err))?;
    fire_notification_trigger(
        ctx,
        &trust,
        &event_store,
        NotificationTriggerSource::ActivityRecord,
    );
    Ok(())
}

/// Read a provider-native stdin JSON payload and adapt it to a
/// Workhive [`ActivityRecordInput`] before persisting it.
///
/// `provider` is supplied by the generated hook script via clap, not
/// by the payload, so the parser never has to guess. The env-derived
/// trusted tuple still pins `runtime_root`, `group`, `workspace`,
/// `unit_id`. The `WORKHIVE_TOOL_PROVIDER` env var must agree with
/// the CLI argument; mismatch is rejected.
pub(crate) fn cmd_internal_activity_hook(
    provider_arg: IntegrationProviderArg,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    let trust = resolve_trusted_correlation(ctx)?;
    let cli_provider = provider_arg.as_core();
    if trust.provider != cli_provider {
        return Err(CliError::Runtime(localized_error(
            ctx,
            i18n_keys::ACTIVITY_RECORD_ERROR_PROVIDER_MISMATCH,
        )));
    }
    let trimmed = read_stdin_payload(ctx)?;
    let raw: serde_json::Value = serde_json::from_str(&trimmed).map_err(|e| {
        CliError::Runtime(format!(
            "{}: {e}",
            localized_error(ctx, i18n_keys::ACTIVITY_RECORD_ERROR_INVALID_JSON)
        ))
    })?;
    let (event, state, confidence, reason) = adapt_provider_payload(ctx, trust.provider, &raw)?;
    let now_ms = current_unix_ms();
    let input = ActivityRecordInput {
        schema_version: AI_ACTIVITY_SCHEMA_VERSION,
        provider: trust.provider.wire_name().to_owned(),
        event,
        tool_name: trust.tool_name.clone(),
        group: trust.group.clone(),
        workspace: trust.workspace.clone(),
        unit_id: trust.unit_id.clone(),
        state,
        confidence,
        observed_at_unix_ms: now_ms,
        reason,
    };
    let legacy_store = FsPaneActivityStore::new(trust.runtime_root.clone());
    let event_store = ctx
        .composition
        .activity_event_store_from_runtime_root(&trust.runtime_root);
    let gate = build_acp_gate_from_runtime_root(ctx, &trust.runtime_root, trust.provider);
    record_activity_dual_write(&input, trust.provider, &legacy_store, &event_store, gate)
        .map_err(|err| map_record_error(ctx, err))?;
    fire_notification_trigger(
        ctx,
        &trust,
        &event_store,
        NotificationTriggerSource::ActivityHook,
    );
    Ok(())
}

/// Hidden production endpoint invoked by the Workhive-managed Codex
/// `notify` shell wrapper installed under `~/.codex/config.toml`.
///
/// # Contract
///
/// * **Trigger**: Codex spawns the wrapper with positional arguments
///   describing the turn that just completed. We **never** inspect
///   those arguments — Codex's `notify` callback is by spec a
///   completion-only signal and we mirror that exact semantics: one
///   appended event per invocation, of kind
///   [`ActivityEventKind::TurnNotifiedComplete`], no
///   `started`/`working`/`tool-running`/`permission-needed` events.
/// * **Trusted correlation**: identical to
///   [`cmd_internal_activity_hook`]: pane group/workspace/unit_id and
///   runtime root are resolved from the `WORKHIVE_*` env vars the
///   pane supervisor injected when spawning Codex (see
///   `pane_supervisor::runner`). The wrapper script forwards no
///   stdin so we cannot accept payload-supplied identity; this is by
///   design.
/// * **Provider pin**: rejected when `WORKHIVE_TOOL_PROVIDER` is not
///   `codex`. A non-Codex pane has no business invoking the Codex
///   notify endpoint and a misroute would corrupt the ACP read
///   model. Rejection is silent (warn log + exit 0) per fail-open.
/// * **Provider gate**: if the resolved feature gate disables Codex
///   integration globally or per-provider, the event is dropped
///   without writing anything. The legacy latest-fact store is
///   intentionally **not** touched — notify is an ACP-only signal
///   and there is no canonical `AI_ACTIVITY_SCHEMA_VERSION` mapping
///   for "notify completion".
/// * **Fail-open**: every internal failure (missing env, unknown
///   provider, store unavailable, append I/O error) is logged at
///   `debug`/`warn` and the process exits `0`. The Codex turn must
///   never be blocked by Workhive recording.
///
/// # Why a dedicated CLI subcommand
///
/// The hook adapter at [`cmd_internal_activity_hook`] reads provider
/// hook payloads from stdin and feeds them through
/// [`record_activity_dual_write`] (which targets both the legacy
/// latest-fact store and the ACP store). Notify is fundamentally
/// different:
///
/// * Codex's `notify` callback receives no JSON payload.
/// * Notify is a single bit of evidence ("turn done") at medium
///   confidence; it must NOT participate in the legacy latest-fact
///   schema, otherwise it would advertise a fabricated `state`/
///   `confidence` tuple the spec does not define.
///
/// So this handler bypasses [`record_activity_dual_write`] and
/// appends one normalized event to the ACP event store directly,
/// honoring the `SignalChannel::NotifyCallback` +
/// `ProviderEventKind::NotifyCompletion` contract pinned in the
/// review.
///
/// `_args` are accepted for completeness (so the wrapper can pass
/// `"$@"` verbatim) but never read. Argument inspection would risk
/// leaking prompt/turn content into the event stream — which is
/// explicitly forbidden.
pub(crate) fn cmd_internal_activity_codex_notify(
    _args: Vec<String>,
    ctx: &CommandContext,
) -> Result<(), CliError> {
    // Fail-open envelope: ANY error inside is logged + dropped, never
    // surfaced to Codex. The CLI exit code stays 0.
    if let Err(err) = record_codex_notify_completion_inner(ctx) {
        tracing::warn!(
            error = %err,
            "internal activity codex-notify: dropping fact (fail-open per requirement)"
        );
    }
    Ok(())
}

/// Inner implementation of [`cmd_internal_activity_codex_notify`].
///
/// Returning `Result<(), String>` makes the fail-open envelope
/// trivially testable: any returned error is treated as a "drop the
/// fact" outcome by the caller. The string is purely diagnostic and
/// never reaches Codex.
fn record_codex_notify_completion_inner(ctx: &CommandContext) -> Result<(), String> {
    let trust =
        resolve_trusted_correlation(ctx).map_err(|e| format!("trusted correlation: {e:?}"))?;

    // Notify is Codex-only by spec. A non-Codex pane invoking this
    // endpoint is either a routing bug (we should not write) or a
    // hostile environment override (we MUST not write). Drop the
    // event silently.
    if !matches!(trust.provider, AiActivityProvider::Codex) {
        return Err(format!(
            "provider mismatch: notify endpoint requires codex, got {}",
            trust.provider.wire_name()
        ));
    }

    // Honor the same gate as the hook adapter: a disabled feature
    // gate must completely silence the side effect (no writes, no
    // diagnostics beyond the existing fail-closed debug logs in
    // `build_acp_gate_from_runtime_root`). Per the gate-resolver
    // contract this is fail-CLOSED for ACP append.
    let gate = build_acp_gate_from_runtime_root(ctx, &trust.runtime_root, trust.provider);
    if !gate.allows_acp_append() {
        return Err("ACP gate disabled for Codex on this project; dropping notify".to_owned());
    }

    let pane = PaneCorrelationKey::new(
        trust.group.clone(),
        trust.workspace.clone(),
        trust.unit_id.clone(),
    );
    let event = ActivityEvent::new(
        pane,
        ProviderKind::Codex,
        SignalChannel::NotifyCallback,
        ProviderSignalSource::Notify,
        ActivityEventKind::TurnNotifiedComplete,
        ActivityOutcome::Success,
        // Notify is medium confidence per the requirement: lifecycle-
        // hook completion is `High`, but notify lacks the trusted
        // payload to be promoted further. The ACP normalizer pins
        // `Medium` for `NotifyCompletion` and we mirror it here.
        ObservationConfidence::Medium,
        current_unix_ms(),
        // No detail: notify carries no display-safe payload and
        // inspecting `_args` would risk leaking prompt content.
        None,
    );

    // Pin the invariant in code, not just in docs: the
    // notify endpoint MUST NEVER produce any non-completion kind.
    // If a future refactor swaps this constructor for a normalizer
    // call, the assertion fires loudly in debug builds before any
    // bad event reaches disk.
    debug_assert_eq!(
        event.kind,
        ActivityEventKind::TurnNotifiedComplete,
        "notify must never derive working/started/tool-running/permission-needed events",
    );
    debug_assert_eq!(event.channel, SignalChannel::NotifyCallback);
    debug_assert_eq!(event.source, ProviderSignalSource::Notify);

    let store = ctx
        .composition
        .activity_event_store_from_runtime_root(&trust.runtime_root);
    store
        .append(&event)
        .map_err(|err| format!("append ACP event: {err}"))?;
    fire_notification_trigger(ctx, &trust, &store, NotificationTriggerSource::CodexNotify);
    Ok(())
}

/// Compose the production [`NotificationPort`] and invoke
/// [`evaluate_after_acp_append`] for the just-recorded ACP event.
///
/// Every ACP event ingestion path must trigger a
/// notification evaluation immediately, so `permission-needed` and
/// `failed` events surface to the user within milliseconds of being
/// recorded — not at supervisor exit. The cross-pane
/// `all-panes-complete` evaluation is wired up here too so the
/// workspace-level notification fires from the same boundary.
///
/// All errors inside the trigger are swallowed via `tracing` per the
/// fail-open contract; the recording call site never observes them.
fn fire_notification_trigger(
    ctx: &CommandContext,
    trust: &TrustedCorrelation,
    event_store: &workhive_infra::activity_event_store::FsActivityEventStore,
    source: NotificationTriggerSource,
) {
    let pane = PaneCorrelationKey::new(
        trust.group.clone(),
        trust.workspace.clone(),
        trust.unit_id.clone(),
    );
    let project_root = trust.runtime_root.parent().and_then(Utf8Path::parent);
    let port = workhive_infra::notification::production_notification_port();
    // Durable, append-only outbox so that two
    // logical invocations of `evaluate_after_acp_append` over the
    // same ACP event history do not re-deliver the same
    // notification. The store is local-only, sentinel-owned under
    // the runtime root, and fail-open inside the trigger.
    let outbox_store = workhive_infra::notification_outbox_store::FsNotificationOutboxStore::new(
        trust.runtime_root.clone(),
    );
    evaluate_after_acp_append(
        &ctx.config,
        project_root,
        event_store,
        Some(&outbox_store),
        &pane,
        source,
        port.as_ref(),
        current_unix_ms(),
    );
}

/// Pin the matching ProviderEvent shape next to the handler so a
/// reader of the codex-notify path can verify what observation kind
/// the equivalent normalizer-driven path would produce. This is a
/// `const _` shape assertion only — no runtime cost.
#[allow(dead_code)]
const _NOTIFY_PROVIDER_EVENT_KIND_PIN: ProviderEventKind = ProviderEventKind::NotifyCompletion;

/// Derive the project root from the trusted runtime root and resolve
/// the activity-control-plane gate from layered config.
///
/// The hidden hook entry points do not take a `--project-root`
/// argument because they are invoked by Workhive-generated hook
/// scripts under a pane the supervisor already pinned. We rely on
/// the supervisor having staged the runtime root under
/// `<project_root>/.workhive/runtime`, so the project root is two
/// directory levels above `runtime_root`.
///
/// Fail-closed semantics: if the project root cannot be derived
/// (`runtime_root` has fewer than two parents) **or** either config
/// load fails, the gate is set to
/// [`ActivityControlPlaneGate::all_disabled`] so the ACP append is
/// skipped silently. The legacy latest-fact write is unaffected.
fn build_acp_gate_from_runtime_root(
    ctx: &CommandContext,
    runtime_root: &Utf8Path,
    provider: AiActivityProvider,
) -> ActivityControlPlaneGate {
    let Some(project_root) = runtime_root.parent().and_then(Utf8Path::parent) else {
        tracing::debug!(
            runtime_root = %runtime_root,
            "ACP gate fail-closed: runtime_root has no <project>/.workhive ancestor"
        );
        return ActivityControlPlaneGate::all_disabled();
    };
    let global = match ctx.config.load_global() {
        Ok(opt) => opt,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "ACP gate fail-closed: failed to load global config"
            );
            return ActivityControlPlaneGate::all_disabled();
        }
    };
    let project = match ctx.config.load_project(project_root) {
        Ok(opt) => opt,
        Err(err) => {
            tracing::debug!(
                error = %err,
                "ACP gate fail-closed: failed to load project config"
            );
            return ActivityControlPlaneGate::all_disabled();
        }
    };
    let merged = match (global, project) {
        (Some(g), Some(p)) => {
            Some(workhive_core::usecases::config_mgmt::resolve_effective_config(&g, &p))
        }
        (Some(g), None) => Some(g),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    };
    let gate = FeatureGate::from_config(merged.as_ref());
    ActivityControlPlaneGate {
        globally_enabled: gate.globally_enabled(),
        provider_enabled: gate.is_provider_enabled(provider),
    }
}

fn read_stdin_payload(ctx: &CommandContext) -> Result<String, CliError> {
    let mut buf = String::new();
    std::io::stdin().read_to_string(&mut buf).map_err(|e| {
        CliError::Runtime(format!(
            "{}: {e}",
            localized_error(ctx, i18n_keys::ACTIVITY_RECORD_ERROR_READ_STDIN)
        ))
    })?;
    let trimmed = buf.trim().to_owned();
    if trimmed.is_empty() {
        return Err(CliError::Runtime(localized_error(
            ctx,
            i18n_keys::ACTIVITY_RECORD_ERROR_EMPTY_STDIN,
        )));
    }
    Ok(trimmed)
}

fn enforce_trusted_identity(
    ctx: &CommandContext,
    input: &ActivityRecordInput,
    trust: &TrustedCorrelation,
) -> Result<(), CliError> {
    // Empty identity fields in the payload are tolerated (the env
    // tuple wins) — a missing value cannot be evidence of a hostile
    // override. A *different* non-empty value is a hard rejection.
    let mismatched = (!input.group.is_empty() && input.group != trust.group)
        || (!input.workspace.is_empty() && input.workspace != trust.workspace)
        || (!input.unit_id.is_empty() && input.unit_id != trust.unit_id);
    if mismatched {
        return Err(CliError::Runtime(localized_error(
            ctx,
            i18n_keys::ACTIVITY_RECORD_ERROR_IDENTITY_MISMATCH,
        )));
    }
    Ok(())
}

fn map_record_error(ctx: &CommandContext, err: ActivityRecordError) -> CliError {
    match err {
        ActivityRecordError::InvalidJson(detail)
        | ActivityRecordError::InvalidFields(detail)
        | ActivityRecordError::WriteFailed(detail) => CliError::Runtime(detail),
        ActivityRecordError::ProviderMismatch { expected, actual } => CliError::Runtime(format!(
            "{}: expected={expected} actual={actual}",
            localized_error(ctx, i18n_keys::ACTIVITY_RECORD_ERROR_PROVIDER_MISMATCH)
        )),
    }
}

fn current_unix_ms() -> i64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Translate one provider-native stdin payload to a typed
/// `(event_name, state, confidence, reason)` tuple.
///
/// Documented JSON shapes:
/// - Claude Code (<https://code.claude.com/docs/en/hooks>): the
///   payload is a JSON object containing a top-level
///   `hook_event_name` string. Some events additionally embed
///   `tool_name`.
/// - Codex (<https://developers.openai.com/codex/hooks>): same
///   contract — a JSON object with a top-level `hook_event_name`
///   string per hook invocation.
/// - OpenCode plugins (<https://opencode.ai/docs/plugins/>) emit
///   `event` objects with a `type` discriminator. The Workhive
///   plugin forwards the type string verbatim under
///   `event_type` so this code path stays a pure JSON parser.
fn adapt_provider_payload(
    ctx: &CommandContext,
    provider: AiActivityProvider,
    raw: &serde_json::Value,
) -> Result<
    (
        String,
        workhive_core::domain::AiActivityState,
        workhive_core::domain::AiActivityConfidence,
        Option<String>,
    ),
    CliError,
> {
    let event_field = match provider {
        AiActivityProvider::ClaudeCode | AiActivityProvider::Codex => "hook_event_name",
        AiActivityProvider::Opencode => "event_type",
        AiActivityProvider::None => {
            return Err(CliError::Runtime(localized_error(
                ctx,
                i18n_keys::ACTIVITY_RECORD_ERROR_NO_PROVIDER_ACTIVE,
            )));
        }
    };
    let event_name = raw
        .get(event_field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            CliError::Runtime(localized_error_with_arg(
                ctx,
                i18n_keys::ACTIVITY_RECORD_ERROR_MISSING_EVENT_FIELD,
                event_field,
            ))
        })?;
    let mapped = match provider {
        AiActivityProvider::ClaudeCode => map_claude_event(event_name),
        AiActivityProvider::Codex => map_codex_event(event_name),
        AiActivityProvider::Opencode => map_opencode_event(event_name),
        AiActivityProvider::None => None,
    };
    let (state, confidence) = mapped.ok_or_else(|| {
        // Unknown / unsupported event name is not an error — the
        // hook script should remain silent. We map it to a structured
        // CliError that the shell wrapper swallows.
        CliError::Runtime(localized_error_with_arg(
            ctx,
            i18n_keys::ACTIVITY_RECORD_ERROR_UNHANDLED_EVENT,
            event_name,
        ))
    })?;
    // Pick a display-safe reason from documented payload fields.
    // Claude/Codex `PreToolUse` / `PostToolUse` payloads contain
    // `tool_name`; the same field doubles as the plugin's tool
    // identity in OpenCode. Anything else is omitted to avoid leaking
    // user content.
    let reason = raw
        .get("tool_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    Ok((event_name.to_owned(), state, confidence, reason))
}

fn localized_error(ctx: &CommandContext, key: &str) -> String {
    ctx.i18n.resolve(ctx.locale(), key)
}

fn localized_error_with_arg(ctx: &CommandContext, key: &str, arg: &str) -> String {
    format!("{}: {arg}", ctx.i18n.resolve(ctx.locale(), key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use workhive_core::domain::{AiActivityConfidence, AiActivityState};

    /// Adapter: Claude Code payload with `hook_event_name = UserPromptSubmit`
    /// must map to (`Processing`, `Hook`) per `map_claude_event`.
    #[test]
    fn adapt_claude_user_prompt_submit_maps_to_processing() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "abc",
        });
        let (event, state, conf, reason) =
            adapt_provider_payload(&ctx, AiActivityProvider::ClaudeCode, &raw).unwrap();
        assert_eq!(event, "UserPromptSubmit");
        assert_eq!(state, AiActivityState::Processing);
        assert_eq!(conf, AiActivityConfidence::Hook);
        assert!(reason.is_none());
    }

    /// Adapter: Claude Code PreToolUse with `tool_name` should
    /// surface the tool name as a display-safe reason.
    #[test]
    fn adapt_claude_pre_tool_use_surfaces_tool_name() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
        });
        let (_, state, _, reason) =
            adapt_provider_payload(&ctx, AiActivityProvider::ClaudeCode, &raw).unwrap();
        assert_eq!(state, AiActivityState::ToolRunning);
        assert_eq!(reason.as_deref(), Some("Bash"));
    }

    /// Adapter: Codex payload uses the same `hook_event_name` field
    /// per the official Codex hooks documentation.
    #[test]
    fn adapt_codex_pre_tool_use_maps_to_tool_running() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "git_status",
        });
        let (event, state, conf, reason) =
            adapt_provider_payload(&ctx, AiActivityProvider::Codex, &raw).unwrap();
        assert_eq!(event, "PreToolUse");
        assert_eq!(state, AiActivityState::ToolRunning);
        assert_eq!(conf, AiActivityConfidence::Hook);
        assert_eq!(reason.as_deref(), Some("git_status"));
    }

    /// Adapter: OpenCode plugin events use `event_type` because the
    /// official plugin API exposes events as `{ event: { type: ... } }`
    /// and the Workhive plugin forwards `type` verbatim.
    #[test]
    fn adapt_opencode_tool_execute_before_maps_to_tool_running() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({
            "event_type": "tool.execute.before",
            "tool_name": "edit",
        });
        let (event, state, conf, reason) =
            adapt_provider_payload(&ctx, AiActivityProvider::Opencode, &raw).unwrap();
        assert_eq!(event, "tool.execute.before");
        assert_eq!(state, AiActivityState::ToolRunning);
        assert_eq!(conf, AiActivityConfidence::Plugin);
        assert_eq!(reason.as_deref(), Some("edit"));
    }

    /// Adapter: missing `hook_event_name` is rejected with the
    /// dedicated localized error key.
    #[test]
    fn adapt_claude_payload_without_event_field_is_rejected() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({ "session_id": "abc" });
        let err = adapt_provider_payload(&ctx, AiActivityProvider::ClaudeCode, &raw).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("hook_event_name"),
            "error must name the missing field, got: {msg}"
        );
    }

    /// Adapter: unknown event names produce an explicit error rather
    /// than silently mapping to `Unknown`. The shell wrapper swallows
    /// this so unknown events keep the previous on-disk state intact.
    #[test]
    fn adapt_claude_payload_with_unknown_event_is_rejected() {
        let ctx = CommandContext::new();
        let raw = serde_json::json!({
            "hook_event_name": "TotallyMadeUpEvent",
        });
        let err = adapt_provider_payload(&ctx, AiActivityProvider::ClaudeCode, &raw).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("TotallyMadeUpEvent"),
            "error must name the unknown event, got: {msg}"
        );
    }

    /// Gate derivation: a runtime root that does not resolve to any
    /// `<project>/.workhive/runtime` ancestor must fail closed for
    /// ACP. The legacy latest-fact path is unaffected because it
    /// uses `trust.runtime_root` directly.
    #[test]
    fn build_acp_gate_from_runtime_root_fails_closed_when_no_project_ancestor() {
        let ctx = CommandContext::new();
        // Two parents would yield `Some(...)`, so use a single-segment path.
        let runtime_root = Utf8PathBuf::from("just_runtime");
        let gate = build_acp_gate_from_runtime_root(
            &ctx,
            runtime_root.as_path(),
            AiActivityProvider::ClaudeCode,
        );
        assert_eq!(
            gate,
            ActivityControlPlaneGate::all_disabled(),
            "runtime_root without a <project>/.workhive ancestor must fail closed"
        );
    }

    /// Gate derivation: an empty runtime root path also fails closed
    /// (no parents to peel back to the project root).
    #[test]
    fn build_acp_gate_from_runtime_root_fails_closed_on_empty_path() {
        let ctx = CommandContext::new();
        let runtime_root = Utf8PathBuf::new();
        let gate = build_acp_gate_from_runtime_root(
            &ctx,
            runtime_root.as_path(),
            AiActivityProvider::Codex,
        );
        assert_eq!(gate, ActivityControlPlaneGate::all_disabled());
    }

    /// Gate derivation: a runtime root that resolves to a project
    /// root with no config files falls back to built-in defaults,
    /// which keep AI integrations globally enabled. This pins the
    /// "happy path" so a future refactor cannot silently change the
    /// default-enabled semantics for the dual-write code path.
    #[test]
    fn build_acp_gate_from_runtime_root_uses_builtin_defaults_when_no_config_present() {
        let ctx = CommandContext::new();
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("project root is utf8");
        let runtime_root = project_root.join(".workhive").join("runtime");
        let gate = build_acp_gate_from_runtime_root(
            &ctx,
            runtime_root.as_path(),
            AiActivityProvider::ClaudeCode,
        );
        // Built-in defaults: globally enabled, all known providers
        // enabled. We assert allows_acp_append to keep this test
        // resilient to future split between the two fields.
        assert!(
            gate.allows_acp_append(),
            "missing config must fall back to built-in defaults"
        );
    }

    // ── Codex notify endpoint tests ─────────────────────────────────────
    //
    // The notify endpoint resolves identity from process-global
    // environment variables (`WORKHIVE_*`), so all tests in this
    // module that exercise it must serialize through `ENV_LOCK` to
    // avoid clobbering each other under `cargo test`'s default
    // parallelism.

    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Snapshot the current value of one env var so a test can
    /// restore it on drop.
    struct EnvSnapshot {
        keys: Vec<&'static str>,
        prev: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvSnapshot {
        fn install(values: &[(&'static str, &str)]) -> Self {
            let mut prev = Vec::with_capacity(values.len());
            let mut keys = Vec::with_capacity(values.len());
            for (k, v) in values {
                prev.push((*k, std::env::var_os(k)));
                unsafe {
                    std::env::set_var(k, v);
                }
                keys.push(*k);
            }
            Self { keys, prev }
        }

        fn clear(&self, extra: &[&str]) {
            for k in extra {
                unsafe {
                    std::env::remove_var(k);
                }
            }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (k, v) in self.prev.drain(..) {
                unsafe {
                    match v {
                        Some(val) => std::env::set_var(k, val),
                        None => std::env::remove_var(k),
                    }
                }
            }
            self.keys.clear();
        }
    }

    /// Helper: assemble the trusted env tuple a real
    /// pane-supervisor-launched Codex would see. Returns the
    /// runtime root + project root so the test can also point the
    /// ACP store at the right directory.
    fn install_codex_trusted_env(
        project_root: &Utf8Path,
    ) -> (EnvSnapshot, Utf8PathBuf, Utf8PathBuf) {
        let runtime_root = project_root.join(".workhive").join("runtime");
        std::fs::create_dir_all(runtime_root.as_std_path()).expect("create runtime root");
        let snapshot = EnvSnapshot::install(&[
            (ENV_RUNTIME_ROOT, runtime_root.as_str()),
            (ENV_TOOL_PROVIDER, AiActivityProvider::Codex.wire_name()),
            (ENV_GROUP, "test-group"),
            (ENV_WORKSPACE, "test-workspace"),
            (ENV_UNIT_ID, "u-1"),
        ]);
        (snapshot, runtime_root, project_root.to_path_buf())
    }

    /// A well-formed notify invocation must
    /// append exactly one ACP event of kind `TurnNotifiedComplete`,
    /// with `SignalChannel::NotifyCallback` and
    /// `ProviderSignalSource::Notify`. NO other event kind may be
    /// emitted by the notify path — completion-only is the spec.
    #[test]
    fn codex_notify_appends_single_turn_notified_complete_event() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("project root is utf8");
        let (_env, runtime_root, _) = install_codex_trusted_env(&project_root);

        let ctx = CommandContext::new();
        let result = cmd_internal_activity_codex_notify(vec!["arg1".into(), "arg2".into()], &ctx);
        assert!(
            result.is_ok(),
            "notify must always exit Ok (fail-open envelope)"
        );

        // Read back through the same store the handler used.
        let store = ctx
            .composition
            .activity_event_store_from_runtime_root(&runtime_root);
        let pane = PaneCorrelationKey::new("test-group", "test-workspace", "u-1");
        let events = store.events_for_pane(&pane).expect("read pane events");
        assert_eq!(
            events.len(),
            1,
            "notify endpoint must append exactly one event per invocation, got {events:#?}"
        );
        let event = &events[0];
        assert_eq!(
            event.kind,
            ActivityEventKind::TurnNotifiedComplete,
            "notify must produce TurnNotifiedComplete and never any \
             working/started/tool-running/permission-needed kind"
        );
        assert_eq!(event.channel, SignalChannel::NotifyCallback);
        assert_eq!(event.source, ProviderSignalSource::Notify);
        assert_eq!(event.provider, ProviderKind::Codex);
        assert_eq!(event.confidence, ObservationConfidence::Medium);
        assert_eq!(event.outcome, ActivityOutcome::Success);
        assert!(
            event.detail.is_none(),
            "notify must never persist payload-derived detail (no prompt content), got: {:?}",
            event.detail
        );
    }

    /// Notify never emits any non-completion
    /// kind. We assert each forbidden kind explicitly so a future
    /// refactor that adds a `started`/`tool_running`/etc. branch to
    /// the notify path fails this test.
    #[test]
    fn codex_notify_never_emits_non_completion_kinds() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("project root is utf8");
        let (_env, runtime_root, _) = install_codex_trusted_env(&project_root);

        let ctx = CommandContext::new();
        for invocation in 0..3 {
            cmd_internal_activity_codex_notify(vec![format!("inv-{invocation}")], &ctx).unwrap();
        }

        let store = ctx
            .composition
            .activity_event_store_from_runtime_root(&runtime_root);
        let pane = PaneCorrelationKey::new("test-group", "test-workspace", "u-1");
        let events = store.events_for_pane(&pane).expect("read pane events");
        assert_eq!(events.len(), 3, "three invocations → three events");
        for event in &events {
            assert_ne!(
                event.kind,
                ActivityEventKind::PromptSubmitted,
                "notify must NOT emit PromptSubmitted"
            );
            assert_ne!(
                event.kind,
                ActivityEventKind::ToolStarted,
                "notify must NOT emit ToolStarted"
            );
            assert_ne!(
                event.kind,
                ActivityEventKind::ToolFinished,
                "notify must NOT emit ToolFinished"
            );
            assert_ne!(
                event.kind,
                ActivityEventKind::PermissionRequested,
                "notify must NOT emit PermissionRequested"
            );
            assert_ne!(
                event.kind,
                ActivityEventKind::PermissionResolved,
                "notify must NOT emit PermissionResolved"
            );
            assert_ne!(
                event.kind,
                ActivityEventKind::TurnCompleted,
                "notify must NOT emit TurnCompleted (that is the \
                 hook-attested high-confidence kind; notify is medium)"
            );
            assert_eq!(event.kind, ActivityEventKind::TurnNotifiedComplete);
        }
    }

    /// Notify is fail-open. When the trusted
    /// env tuple is missing the handler must STILL exit Ok so the
    /// upstream Codex turn is never blocked.
    #[test]
    fn codex_notify_fails_open_when_trusted_env_missing() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Snapshot whatever is currently set so we can restore it.
        let snapshot = EnvSnapshot::install(&[]);
        snapshot.clear(&[
            ENV_RUNTIME_ROOT,
            ENV_TOOL_PROVIDER,
            ENV_GROUP,
            ENV_WORKSPACE,
            ENV_UNIT_ID,
        ]);

        let ctx = CommandContext::new();
        let result = cmd_internal_activity_codex_notify(vec![], &ctx);
        assert!(
            result.is_ok(),
            "notify must exit Ok even with no trusted env (fail-open contract)"
        );
    }

    /// Notify rejects (drops silently) when
    /// the pane was launched as a non-Codex provider. This prevents
    /// a misrouted invocation from corrupting the ACP read model
    /// for a Claude/OpenCode pane.
    #[test]
    fn codex_notify_drops_silently_when_pane_provider_is_not_codex() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().expect("tempdir");
        let project_root =
            Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).expect("project root is utf8");
        let runtime_root = project_root.join(".workhive").join("runtime");
        std::fs::create_dir_all(runtime_root.as_std_path()).expect("create runtime root");
        let _env = EnvSnapshot::install(&[
            (ENV_RUNTIME_ROOT, runtime_root.as_str()),
            (
                ENV_TOOL_PROVIDER,
                AiActivityProvider::ClaudeCode.wire_name(),
            ),
            (ENV_GROUP, "test-group"),
            (ENV_WORKSPACE, "test-workspace"),
            (ENV_UNIT_ID, "u-1"),
        ]);

        let ctx = CommandContext::new();
        cmd_internal_activity_codex_notify(vec![], &ctx).unwrap();

        // No event should have been written for the Codex provider.
        let store = ctx
            .composition
            .activity_event_store_from_runtime_root(&runtime_root);
        let pane = PaneCorrelationKey::new("test-group", "test-workspace", "u-1");
        let events = store.events_for_pane(&pane).unwrap_or_default();
        assert!(
            events.is_empty(),
            "notify must drop silently on provider mismatch, got: {events:#?}"
        );
    }
}
